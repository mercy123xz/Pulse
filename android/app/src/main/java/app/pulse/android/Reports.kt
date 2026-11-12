package app.pulse.android

import android.content.Context
import android.os.Build
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject
import java.io.File
import java.io.IOException

/**
 * Crash reports and "Report a problem", as on the desktop app. A crash is written to a
 * file in the app's private folder; the next time Pulse opens it asks whether to send it.
 * Nothing is sent unless you press Send, and email addresses are blanked out first. The
 * report goes to a private list only the developer can read.
 */
object Reports {
    private const val ENDPOINT = "https://pulse-questions.y0hyotuube.workers.dev/report"
    private lateinit var dir: File

    fun install(context: Context) {
        dir = File(context.filesDir, "crash-reports").apply { mkdirs() }
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, e ->
            runCatching {
                val where = e.stackTrace.firstOrNull { it.className.startsWith("app.pulse") } ?: e.stackTrace.firstOrNull()
                val text = buildString {
                    append("Pulse crash report\n\n").append(about()).append('\n')
                    append("What: ").append(e.javaClass.simpleName).append(": ").append(e.message ?: "").append('\n')
                    append("Where: ").append(where?.let { "${it.className.substringAfterLast('.')}.${it.methodName} (${it.fileName}:${it.lineNumber})" } ?: "unknown").append('\n')
                    append("Thread: ").append(thread.name).append("\n\n")
                    append(e.stackTraceToString().take(12_000))
                }
                File(dir, "crash-${System.currentTimeMillis()}.txt").writeText(text)
            }
            previous?.uncaughtException(thread, e)
        }
    }

    /** This phone and this copy of Pulse. */
    fun about(): String =
        "Pulse for Android ${BuildConfig.VERSION_NAME} (build ${BuildConfig.VERSION_CODE})\n" +
            "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}), ${Build.MANUFACTURER} ${Build.MODEL}"

    /** The newest crash you haven't been asked about: (file, report). */
    fun unseen(): Pair<File, String>? {
        val f = dir.listFiles { x -> x.name.startsWith("crash-") }?.maxByOrNull { it.name } ?: return null
        return f to runCatching { f.readText() }.getOrNull().orEmpty()
    }

    /** Done with this crash (sent or not): delete it and any older ones. */
    fun dismiss(file: File) {
        dir.listFiles { x -> x.name.startsWith("crash-") && x.name <= file.name }?.forEach { it.delete() }
    }

    fun scrub(s: String) = s.replace(Regex("""[\w.+-]+@[\w-]+\.[\w.-]+"""), "<email>")

    /** Sends a report. Throws IOException with a message to show if it can't. */
    fun send(crash: Boolean, message: String, details: String) {
        val body = JSONObject()
            .put("kind", if (crash) "crash" else "problem")
            .put("source", "android")
            .put("version", BuildConfig.VERSION_NAME)
            .put("message", scrub(message.trim()))
            .put("details", scrub(details))
        val req = okhttp3.Request.Builder().url(ENDPOINT).post(body.toString().toRequestBody("application/json".toMediaType())).build()
        val (code, text) = try {
            Net.client.newCall(req).execute().use { it.code to it.body.string() }
        } catch (e: IOException) {
            throw IOException("Couldn't reach the server. Check your connection and try again.")
        }
        if (code !in 200..299) {
            throw IOException(runCatching { JSONObject(text).optString("error") }.getOrNull().orEmpty().ifEmpty { "The report couldn't be sent right now." })
        }
    }
}
