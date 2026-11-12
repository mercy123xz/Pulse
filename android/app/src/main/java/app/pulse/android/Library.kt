package app.pulse.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID
import java.util.concurrent.Executors

/** One of your playlists. `publicId`: its id on the Pulse accounts server while it's public. */
data class Playlist(val id: String, val name: String, val songs: List<Song>, val publicId: String? = null)

/** Something you opened or played from: a tile in "Recently played". */
data class Place(val kind: String, val id: String, val title: String, val subtitle: String, val image: String?) {
    companion object {
        const val LIKED = "liked"
        const val ALBUM = "album"
        const val ARTIST = "artist"
        const val PLAYLIST = "playlist"
        const val ORBIT = "orbit"
    }
}

/** An artist you've played, for Orbits: how often, and when last. */
data class Played(val id: String, val name: String, val picture: String?, val count: Int, val last: Long)

/** Theme choices. */
object Themes {
    const val SYSTEM = 0
    const val DARK = 1
    const val LIGHT = 2
    /** Accent colors: name, strong, soft. */
    val accents = listOf(
        Triple("Purple", 0xFF7C3AED, 0xFFA78BFA),
        Triple("Blue", 0xFF2563EB, 0xFF60A5FA),
        Triple("Green", 0xFF059669, 0xFF34D399),
        Triple("Pink", 0xFFDB2777, 0xFFF472B6),
        Triple("Orange", 0xFFEA580C, 0xFFFB923C),
    )
}

/**
 * Everything Pulse keeps on the phone: likes, playlists, what you played, and settings.
 * Saved as one small file in the app's own folder (nothing leaves the phone). Changed only
 * on the main thread; screens redraw by themselves when it changes.
 */
object Library {
    private lateinit var file: File
    private val writer = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())
    private var loaded = false

    val liked = mutableStateListOf<Song>()
    val playlists = mutableStateListOf<Playlist>()
    val recent = mutableStateListOf<Place>()
    val played = mutableStateListOf<Played>()
    /** Ids of the last songs played, newest first (Autoplay skips them). */
    val recentSongs = mutableStateListOf<String>()
    /** Artists you follow (picked at the start, or Follow on their page). */
    val followed = mutableStateListOf<Artist>()
    /** "Pick artists you like" has been shown (picked or skipped). */
    var artistsPicked by mutableStateOf(false)

    var theme by mutableStateOf(Themes.SYSTEM)
    var accent by mutableStateOf(0)
    var largeText by mutableStateOf(false)
    var autoplay by mutableStateOf(true)

    /** Songs Autoplay added to the queue (labelled in it). Not saved. */
    val autoplayIds = HashSet<String>()

    fun load(context: Context) {
        if (loaded) return
        loaded = true
        file = File(context.filesDir, "library.json")
        val spare = File(context.filesDir, "library.json.spare")
        val o = listOf(file, spare).firstNotNullOfOrNull { f -> runCatching { JSONObject(f.readText()) }.getOrNull() } ?: return
        fun songs(a: JSONArray?) = (0 until (a?.length() ?: 0)).mapNotNull { runCatching { Song.fromJson(a!!.getJSONObject(it)) }.getOrNull() }
        liked.addAll(songs(o.optJSONArray("liked")))
        o.optJSONArray("playlists")?.let { a ->
            for (i in 0 until a.length()) {
                val p = a.getJSONObject(i)
                playlists.add(Playlist(p.optString("id"), p.optString("name"), songs(p.optJSONArray("songs")), p.optString("publicId").ifEmpty { null }))
            }
        }
        o.optJSONArray("recent")?.let { a ->
            for (i in 0 until a.length()) {
                val p = a.getJSONObject(i)
                recent.add(Place(p.optString("kind"), p.optString("id"), p.optString("title"), p.optString("subtitle"), p.optString("image").ifEmpty { null }))
            }
        }
        o.optJSONArray("played")?.let { a ->
            for (i in 0 until a.length()) {
                val p = a.getJSONObject(i)
                played.add(Played(p.optString("id"), p.optString("name"), p.optString("picture").ifEmpty { null }, p.optInt("count"), p.optLong("last")))
            }
        }
        o.optJSONArray("recentSongs")?.let { a -> for (i in 0 until a.length()) recentSongs.add(a.optString(i)) }
        o.optJSONArray("followed")?.let { a ->
            for (i in 0 until a.length()) {
                val f = a.getJSONObject(i)
                followed.add(Artist(f.optString("id"), f.optString("name"), f.optString("picture").ifEmpty { null }))
            }
        }
        artistsPicked = o.optBoolean("artistsPicked", false)
        theme = o.optInt("theme", Themes.SYSTEM)
        accent = o.optInt("accent", 0).coerceIn(0, Themes.accents.size - 1)
        largeText = o.optBoolean("largeText", false)
        autoplay = o.optBoolean("autoplay", true)
    }

    private val saveSoon = Runnable { saveNow() }

    /** Saves a moment later (several changes in a row are written once). */
    fun save() {
        if (!loaded) return
        main.removeCallbacks(saveSoon)
        main.postDelayed(saveSoon, 400)
    }

    private fun saveNow() {
        fun arr(list: List<Song>) = JSONArray().also { a -> list.forEach { a.put(it.toJson()) } }
        val o = JSONObject()
            .put("version", 1)
            .put("liked", arr(liked))
            .put("playlists", JSONArray().also { a -> playlists.forEach { a.put(JSONObject().put("id", it.id).put("name", it.name).put("songs", arr(it.songs)).put("publicId", it.publicId ?: "")) } })
            .put("recent", JSONArray().also { a -> recent.forEach { a.put(JSONObject().put("kind", it.kind).put("id", it.id).put("title", it.title).put("subtitle", it.subtitle).put("image", it.image ?: "")) } })
            .put("played", JSONArray().also { a -> played.forEach { a.put(JSONObject().put("id", it.id).put("name", it.name).put("picture", it.picture ?: "").put("count", it.count).put("last", it.last)) } })
            .put("recentSongs", JSONArray(recentSongs.toList()))
            .put("followed", JSONArray().also { a -> followed.forEach { a.put(JSONObject().put("id", it.id).put("name", it.name).put("picture", it.picture ?: "")) } })
            .put("artistsPicked", artistsPicked)
            .put("theme", theme).put("accent", accent).put("largeText", largeText).put("autoplay", autoplay)
        val text = o.toString()
        val target = file
        writer.execute {
            // Written whole to a temporary file first, so a crash never leaves it half-saved.
            runCatching {
                val tmp = File(target.parentFile, "library.json.tmp")
                tmp.writeText(text)
                if (target.exists()) target.copyTo(File(target.parentFile, "library.json.spare"), overwrite = true)
                if (!tmp.renameTo(target)) {
                    target.writeText(text)
                    tmp.delete()
                }
            }
        }
    }

    // ------------------------------------------------------------------ likes

    fun isLiked(song: Song) = liked.any { it.id == song.id }

    fun toggleLike(song: Song) {
        val i = liked.indexOfFirst { it.id == song.id }
        if (i >= 0) liked.removeAt(i) else liked.add(0, song)
        save()
    }

    // ------------------------------------------------------------------ playlists

    fun newPlaylist(name: String, first: Song? = null): Playlist = newPlaylist(name, listOfNotNull(first))

    fun newPlaylist(name: String, songs: List<Song>): Playlist {
        val p = Playlist(UUID.randomUUID().toString(), name.trim().ifEmpty { "My playlist" }, songs.distinctBy { it.id })
        playlists.add(0, p)
        save()
        return p
    }

    fun playlist(id: String) = playlists.firstOrNull { it.id == id }

    private fun change(id: String, f: (Playlist) -> Playlist) {
        val i = playlists.indexOfFirst { it.id == id }
        if (i >= 0) {
            playlists[i] = f(playlists[i])
            save()
        }
    }

    /** Adds a song; false if it was already there. */
    fun addTo(id: String, song: Song): Boolean {
        val p = playlist(id) ?: return false
        if (p.songs.any { it.id == song.id }) return false
        change(id) { it.copy(songs = it.songs + song) }
        return true
    }

    fun removeFrom(id: String, index: Int) = change(id) { p -> p.copy(songs = p.songs.filterIndexed { i, _ -> i != index }) }
    fun rename(id: String, name: String) = change(id) { it.copy(name = name.trim().ifEmpty { it.name }) }
    fun setPublicId(id: String, publicId: String?) = change(id) { it.copy(publicId = publicId) }

    // ------------------------------------------------------------------ artists you follow

    fun follows(id: String) = followed.any { it.id == id }

    fun toggleFollow(a: Artist) {
        val i = followed.indexOfFirst { it.id == a.id }
        if (i >= 0) followed.removeAt(i) else followed.add(0, a)
        save()
    }

    /** Saves the artists picked at the start. */
    fun pickArtists(picked: List<Artist>) {
        for (a in picked) if (!follows(a.id)) followed.add(a)
        artistsPicked = true
        save()
    }

    fun deletePlaylist(id: String) {
        playlists.removeAll { it.id == id }
        recent.removeAll { it.kind == Place.PLAYLIST && it.id == id }
        save()
    }

    // ------------------------------------------------------------------ what you play

    /** Puts a page at the front of "Recently played". */
    fun visit(place: Place) {
        recent.removeAll { it.kind == place.kind && it.id == place.id }
        recent.add(0, place)
        while (recent.size > 12) recent.removeAt(recent.size - 1)
        save()
    }

    /** A song started: counts for its artist's Orbit, and Autoplay won't pick it again soon. */
    fun notePlayed(song: Song) {
        recentSongs.remove(song.id)
        recentSongs.add(0, song.id)
        while (recentSongs.size > 50) recentSongs.removeAt(recentSongs.size - 1)
        if (song.artistId.isNotEmpty()) {
            val now = System.currentTimeMillis() / 1000
            val i = played.indexOfFirst { it.id == song.artistId }
            if (i >= 0) {
                val p = played[i]
                played[i] = p.copy(count = p.count + 1, last = now, picture = song.artistPicture ?: p.picture)
            } else {
                played.add(Played(song.artistId, song.artist, song.artistPicture, 1, now))
            }
            if (played.size > 300) played.remove(played.minBy { it.last })
        }
        save()
    }

    /** Your most-played artists, best first: plays count, recent plays count more. */
    fun topArtists(): List<Played> {
        val now = System.currentTimeMillis() / 1000
        return played.sortedByDescending { it.count / (1.0 + (now - it.last).coerceAtLeast(0) / 86_400.0 / 14.0) }
    }

    fun pickTheme(t: Int) { theme = t; save() }
    fun pickAccent(a: Int) { accent = a; save() }
    fun useLargeText(on: Boolean) { largeText = on; save() }
    fun useAutoplay(on: Boolean) { autoplay = on; save() }
}
