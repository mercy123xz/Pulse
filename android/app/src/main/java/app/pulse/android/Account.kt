package app.pulse.android

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException

/** Someone's account, as others see it. */
data class Person(val username: String, val bio: String = "")

/** A public playlist in a list (a profile's, or yours). */
data class Listed(val id: String, val name: String, val count: Int, val cover: String?)

/** A public playlist with its songs. */
data class PublicList(val id: String, val name: String, val owner: String, val cover: String?, val songs: List<Song>)

/**
 * Pulse accounts: the same server and the same accounts as the desktop app. Optional:
 * only for sharing playlists and having a profile. The sign-in key is kept in the app's
 * private storage on this phone.
 */
object Account {
    private const val BASE = "https://pulse-accounts.y0hyotuube.workers.dev"
    const val SITE = "https://mercy123xz.github.io/Pulse/"
    private lateinit var prefs: android.content.SharedPreferences

    /** Signed in as (null when signed out). */
    var username by mutableStateOf<String?>(null)
        private set
    var email by mutableStateOf("")
        private set
    private var token: String? = null

    fun load(context: Context) {
        prefs = context.getSharedPreferences("account", Context.MODE_PRIVATE)
        token = prefs.getString("token", null)
        username = prefs.getString("username", null)?.takeIf { token != null }
        email = prefs.getString("email", "") ?: ""
    }

    private fun keep(t: String?, user: String?, mail: String) {
        token = t
        email = mail
        prefs.edit().putString("token", t).putString("username", user).putString("email", mail).apply()
        android.os.Handler(android.os.Looper.getMainLooper()).post { username = user }
    }

    // ------------------------------------------------------------------ talking to the server

    private val JSON_TYPE = "application/json".toMediaType()

    /** One request. Throws IOException with the server's own message when it says no. */
    private fun call(method: String, path: String, body: JSONObject? = null, signedIn: Boolean = false): JSONObject {
        val b = Request.Builder().url(BASE + path)
        if (signedIn) b.header("Authorization", "Bearer " + (token ?: throw IOException("Please sign in first.")))
        when (method) {
            "GET" -> b.get()
            "DELETE" -> if (body != null) b.delete(body.toString().toRequestBody(JSON_TYPE)) else b.delete()
            else -> b.method(method, (body ?: JSONObject()).toString().toRequestBody(JSON_TYPE))
        }
        val (code, text) = try {
            Net.client.newCall(b.build()).execute().use { it.code to it.body.string() }
        } catch (e: IOException) {
            throw IOException("Couldn't reach the Pulse accounts server. Check your connection.")
        }
        val v = runCatching { JSONObject(text) }.getOrDefault(JSONObject())
        if (code in 200..299) return v
        val message = v.optString("error").ifEmpty { "The accounts server returned an error (HTTP $code)." }
        if (code == 401 && signedIn && message == "Please sign in again.") keep(null, null, "")
        throw IOException(message)
    }

    private fun signedInAs(v: JSONObject) {
        val t = v.optString("token").ifEmpty { throw IOException("No sign-in key came back.") }
        val user = v.optJSONObject("user")
        keep(t, user?.optString("username").orEmpty(), user?.optString("email").orEmpty())
    }

    fun signUp(user: String, mail: String, password: String) =
        signedInAs(call("POST", "/signup", JSONObject().put("username", user.trim()).put("email", mail.trim()).put("password", password)))

    /** `who`: your username or email address. */
    fun signIn(who: String, password: String) = signedInAs(call("POST", "/login", JSONObject().put("username", who.trim()).put("password", password)))

    fun signOut() {
        runCatching { call("POST", "/logout", signedIn = true) }
        keep(null, null, "")
    }

    /** Forgot password, step 1: emails a code. Returns the server's message. */
    fun resetStart(who: String): String =
        call("POST", "/reset/start", JSONObject().put("who", who.trim())).optString("message").ifEmpty { "A code is on its way. Check your spam folder too." }

    /** Step 2: the code and a new password. Signs you in. */
    fun resetFinish(who: String, code: String, password: String) =
        signedInAs(call("POST", "/reset/finish", JSONObject().put("who", who.trim()).put("code", code.trim()).put("password", password)))

    fun setBio(bio: String) {
        call("PUT", "/me", JSONObject().put("bio", bio), signedIn = true)
    }

    /** Your bio and your public playlists. */
    fun me(): Pair<Person, List<Listed>> {
        val v = call("GET", "/me", signedIn = true)
        return person(v.optJSONObject("user")) to listed(v.optJSONArray("playlists"))
    }

    fun profile(name: String): Pair<Person, List<Listed>> {
        if (!validUsername(name)) throw IOException("There's no one called that.")
        val v = call("GET", "/users/" + name.lowercase())
        return person(v.optJSONObject("user")) to listed(v.optJSONArray("playlists"))
    }

    fun search(q: String): List<Person> {
        val clean = q.lowercase().filter { it.isLetterOrDigit() || it == '_' || it == '.' }.take(20)
        if (clean.length < 2) return emptyList()
        val a = runCatching { call("GET", "/search?q=$clean").optJSONArray("users") }.getOrNull() ?: return emptyList()
        return (0 until a.length()).map { person(a.getJSONObject(it)) }
    }

    fun playlist(id: String): PublicList {
        if (!validId(id)) throw IOException("That playlist link isn't right.")
        val v = call("GET", "/playlists/$id")
        val a = v.optJSONArray("tracks") ?: JSONArray()
        val songs = (0 until a.length()).mapNotNull { fromServer(a.getJSONObject(it)) }
        return PublicList(v.optString("id"), v.optString("name"), v.optString("username"), v.optString("cover").ifEmpty { null }, songs)
    }

    /** Publishes a playlist; returns its public id. */
    fun publish(name: String, songs: List<Song>): String =
        call("POST", "/playlists", JSONObject().put("name", name).put("tracks", toServer(songs)), signedIn = true).optString("id")
            .ifEmpty { throw IOException("No playlist id came back.") }

    fun update(id: String, name: String, songs: List<Song>) {
        call("PUT", "/playlists/$id", JSONObject().put("name", name).put("tracks", toServer(songs)), signedIn = true)
    }

    fun unpublish(id: String) {
        call("DELETE", "/playlists/$id", signedIn = true)
    }

    private fun person(o: JSONObject?) = Person(o?.optString("username").orEmpty(), o?.optString("bio").orEmpty())

    private fun listed(a: JSONArray?) = (0 until (a?.length() ?: 0)).map {
        val o = a!!.getJSONObject(it)
        Listed(o.optString("id"), o.optString("name"), o.optInt("count"), o.optString("cover").ifEmpty { null })
    }

    // ------------------------------------------------------------------ songs, as the desktop app writes them

    /** Deezer songs are "dz:<number>" on the desktop app; imported ones keep their own ids. */
    private fun fromServer(o: JSONObject): Song? {
        val raw = o.optString("id").ifEmpty { return null }
        val artist = o.optJSONObject("artist")
        val album = o.optJSONObject("album")
        return Song(
            id = raw.removePrefix("dz:"),
            title = o.optString("title"),
            artist = artist?.optString("name").orEmpty(),
            album = album?.optString("title").orEmpty(),
            cover = o.optString("cover_big").ifEmpty { o.optString("cover_small") }.ifEmpty { null },
            duration = o.optInt("duration"),
            artistId = artist?.optString("id").orEmpty(),
            albumId = album?.optString("id").orEmpty(),
            artistPicture = artist?.optString("picture")?.ifEmpty { null },
        )
    }

    private fun toServer(songs: List<Song>) = JSONArray().also { a ->
        for (s in songs) {
            a.put(
                JSONObject()
                    .put("id", if (s.onDeezer) "dz:${s.id}" else s.id)
                    .put("title", s.title)
                    .put("duration", s.duration)
                    .put("artist", JSONObject().put("id", s.artistId).put("name", s.artist).put("picture", s.artistPicture ?: JSONObject.NULL))
                    .put("album", if (s.albumId.isNotEmpty() || s.album.isNotEmpty()) JSONObject().put("id", s.albumId).put("title", s.album) else JSONObject.NULL)
                    .put("cover_small", s.cover ?: JSONObject.NULL)
                    .put("cover_big", s.cover ?: JSONObject.NULL)
                    .put("explicit", false)
                    .put("featured", JSONArray()),
            )
        }
    }

    // ------------------------------------------------------------------ links

    fun validUsername(u: String) = u.length in 1..20 && u.all { it.isLetterOrDigit() || it == '_' || it == '.' }
    fun validId(id: String) = id.length == 10 && id.all { it.isLetterOrDigit() }
    fun validEmail(e: String): Boolean {
        val t = e.trim()
        val at = t.indexOf('@')
        return at > 0 && !t.contains(' ') && t.indexOf('@', at + 1) < 0 && t.substring(at + 1).split('.').let { p -> p.size >= 2 && p.all { it.isNotEmpty() } }
    }

    fun playlistLink(id: String) = "${SITE}p.html?id=$id"
    fun profileLink(name: String) = "${SITE}u.html?name=$name"

    /** What a pasted Pulse link points at: a playlist id or a username. */
    fun readLink(text: String): Pair<String, String>? {
        val t = text.trim()
        if (!t.contains("/Pulse/") && !t.contains("pulse.is-a.software")) return null
        val query = t.substringAfter('?', "").substringBefore('#')
        for (pair in query.split('&')) {
            val k = pair.substringBefore('=')
            val v = pair.substringAfter('=', "")
            if (k == "id" && validId(v) && t.contains("/p.html")) return "playlist" to v
            if (k == "name" && validUsername(v) && t.contains("/u.html")) return "profile" to v.lowercase()
        }
        return null
    }
}
