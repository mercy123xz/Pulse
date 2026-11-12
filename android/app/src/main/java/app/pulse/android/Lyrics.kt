package app.pulse.android

import okhttp3.HttpUrl.Companion.toHttpUrl
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.ConcurrentHashMap

/** A song's words: timed lines (synced), or plain text, or "instrumental". */
class Words(val synced: List<Pair<Float, String>>, val plain: String?, val instrumental: Boolean) {
    /** The line being sung at `secs` (or -1 before the first). */
    fun lineAt(secs: Float): Int = synced.indexOfLast { it.first <= secs }
}

/** Lyrics from LRCLIB, a free and open lyrics library, as on the desktop app. */
object Lyrics {
    private const val API = "https://lrclib.net/api"
    private val cache = ConcurrentHashMap<String, Words>()
    private val none = Words(emptyList(), null, false)

    /** The song's lyrics, or null if LRCLIB doesn't have them. */
    fun get(song: Song): Words? {
        cache[song.id]?.let { return it.takeIf { w -> w !== none } }
        val found = fetch(song)
        cache[song.id] = found ?: none
        return found
    }

    private fun request(path: String, query: List<Pair<String, String>>): String? {
        val url = (API + path).toHttpUrl().newBuilder().apply { query.forEach { (k, v) -> addQueryParameter(k, v) } }.build()
        val req = okhttp3.Request.Builder().url(url).header("User-Agent", "Pulse for Android (https://github.com/mercy123xz/Pulse)").build()
        return Net.client.newCall(req).execute().use {
            when {
                it.code == 404 -> null
                !it.isSuccessful -> throw IOException("Couldn't get the lyrics (HTTP ${it.code}).")
                else -> it.body.string()
            }
        }
    }

    private fun fetch(s: Song): Words? {
        val title = Deezer.coreTitle(s.title).ifBlank { s.title }
        // 1. Exact: title, artist, album and length (LRCLIB allows ±2 seconds).
        val q = mutableListOf("track_name" to title, "artist_name" to s.artist)
        if (s.album.isNotEmpty()) q.add("album_name" to s.album)
        if (s.duration > 0) q.add("duration" to s.duration.toString())
        request("/get", q)?.let { return words(JSONObject(it)) }
        // 2. Search by title and artist; take the closest length, preferring synced lyrics.
        val list = request("/search", listOf("track_name" to title, "artist_name" to s.artist))?.let { JSONArray(it) } ?: return null
        val best = (0 until list.length()).map { list.getJSONObject(it) }
            .filter { !it.isNull("syncedLyrics") || !it.isNull("plainLyrics") || it.optBoolean("instrumental") }
            .filter { s.duration == 0 || it.optDouble("duration", 0.0) == 0.0 || kotlin.math.abs(it.optDouble("duration") - s.duration) <= 8 }
            .minByOrNull { kotlin.math.abs(it.optDouble("duration", 0.0) - s.duration) - if (it.isNull("syncedLyrics")) 0.0 else 3.0 }
        return best?.let(::words)
    }

    private fun words(o: JSONObject): Words {
        val synced = if (o.isNull("syncedLyrics")) emptyList() else parse(o.optString("syncedLyrics"))
        val plain = if (o.isNull("plainLyrics")) null else o.optString("plainLyrics").takeIf { it.isNotBlank() }
        return Words(synced, plain, o.optBoolean("instrumental"))
    }

    /** "[01:02.50]Line" (a line can have several times: "[00:10][01:10]Chorus"). */
    fun parse(text: String): List<Pair<Float, String>> {
        val out = ArrayList<Pair<Float, String>>()
        for (raw in text.lines()) {
            var rest = raw.trim()
            val times = ArrayList<Float>()
            while (rest.startsWith("[")) {
                val end = rest.indexOf(']').takeIf { it > 0 } ?: break
                val tag = rest.substring(1, end)
                val t = time(tag)
                if (t == null) {
                    times.clear()
                    rest = ""
                    break
                }
                times.add(t)
                rest = rest.substring(end + 1)
            }
            for (t in times) out.add(t to rest.trim())
        }
        return out.sortedBy { it.first }
    }

    private fun time(tag: String): Float? {
        val m = tag.substringBefore(':').toIntOrNull() ?: return null
        val s = tag.substringAfter(':', "").toFloatOrNull() ?: return null
        return m * 60 + s
    }
}
