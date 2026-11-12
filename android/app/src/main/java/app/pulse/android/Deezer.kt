package app.pulse.android

import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException
import java.net.URLEncoder

/** Song info, covers, charts and suggestions from Deezer's free API (no account or key). */
object Deezer {
    private const val BASE = "https://api.deezer.com"

    private fun q(s: String) = URLEncoder.encode(s, "UTF-8")

    private fun get(path: String): JSONObject {
        val req = okhttp3.Request.Builder().url(BASE + path).build()
        val text = Net.client.newCall(req).execute().use {
            if (!it.isSuccessful) throw IOException("Deezer didn't answer (HTTP ${it.code}).")
            it.body.string()
        }
        val o = JSONObject(text)
        o.optJSONObject("error")?.let { throw IOException("Deezer: ${it.optString("message")}") }
        return o
    }

    private fun data(path: String): List<JSONObject> {
        val a = get(path).optJSONArray("data") ?: JSONArray()
        return (0 until a.length()).map { a.getJSONObject(it) }
    }

    // ------------------------------------------------------------------ reading Deezer's answers

    private fun song(t: JSONObject, album: Album? = null): Song? {
        if (t.optString("type", "track") != "track" || !t.optBoolean("readable", true)) return null
        val al = t.optJSONObject("album")
        val ar = t.optJSONObject("artist")
        return Song(
            id = t.optLong("id").toString(),
            title = t.optString("title"),
            artist = ar?.optString("name").orEmpty().ifEmpty { album?.artist.orEmpty() },
            album = al?.optString("title").orEmpty().ifEmpty { album?.title.orEmpty() },
            cover = al?.optString("cover_big")?.ifEmpty { null } ?: album?.cover,
            duration = t.optInt("duration"),
            artistId = ar?.optLong("id")?.takeIf { it > 0 }?.toString() ?: album?.artistId.orEmpty(),
            albumId = al?.optLong("id")?.takeIf { it > 0 }?.toString() ?: album?.id.orEmpty(),
            artistPicture = ar?.optString("picture_big")?.ifEmpty { null },
        )
    }

    private fun songs(list: List<JSONObject>, album: Album? = null) = list.mapNotNull { song(it, album) }

    private fun artist(a: JSONObject) = Artist(
        id = a.optLong("id").toString(),
        name = a.optString("name"),
        picture = a.optString("picture_big").ifEmpty { a.optString("picture_medium") }.ifEmpty { null },
        fans = a.optInt("nb_fan"),
    )

    private fun album(a: JSONObject, artist: Artist? = null): Album {
        val ar = a.optJSONObject("artist")
        val kind = when (a.optString("record_type")) {
            "single" -> "Single"
            "ep" -> "EP"
            "compile" -> "Compilation"
            else -> "Album"
        }
        return Album(
            id = a.optLong("id").toString(),
            title = a.optString("title"),
            artist = ar?.optString("name").orEmpty().ifEmpty { artist?.name.orEmpty() },
            artistId = ar?.optLong("id")?.takeIf { it > 0 }?.toString() ?: artist?.id.orEmpty(),
            cover = a.optString("cover_big").ifEmpty { a.optString("cover_medium") }.ifEmpty { null },
            year = a.optString("release_date").take(4),
            kind = kind,
        )
    }

    // ------------------------------------------------------------------ what the screens ask for

    fun searchSongs(text: String): List<Song> = songs(data("/search?q=${q(text)}&limit=40"))
    fun searchArtists(text: String): List<Artist> = data("/search/artist?q=${q(text)}&limit=12").map(::artist)
    fun searchAlbums(text: String): List<Album> = data("/search/album?q=${q(text)}&limit=12").map { album(it) }

    /** Today's top songs. */
    fun topSongs(limit: Int = 50): List<Song> = songs(data("/chart/0/tracks?limit=$limit"))

    /** The most popular artists right now. */
    fun topArtists(limit: Int = 12): List<Artist> = data("/chart/0/artists?limit=$limit").map(::artist)

    class ArtistPage(val artist: Artist, val top: List<Song>, val albums: List<Album>, val related: List<Artist>)

    fun artistPage(id: String): ArtistPage {
        val a = artist(get("/artist/$id"))
        val top = songs(data("/artist/$id/top?limit=20"))
        val albums = runCatching { data("/artist/$id/albums?limit=60").map { album(it, a) } }.getOrDefault(emptyList())
            // Newest first.
            .sortedByDescending { it.year }
        val related = runCatching { related(id) }.getOrDefault(emptyList())
        return ArtistPage(a, top, albums, related)
    }

    fun related(artistId: String, limit: Int = 12): List<Artist> = data("/artist/$artistId/related?limit=$limit").map(::artist)

    fun albumPage(id: String): Pair<Album, List<Song>> {
        val o = get("/album/$id")
        val a = album(o)
        val tracks = o.optJSONObject("tracks")?.optJSONArray("data") ?: JSONArray()
        return a to songs((0 until tracks.length()).map { tracks.getJSONObject(it) }, a)
    }

    /** Songs like `song`, for Autoplay: its artist's radio, or similar artists' hits. */
    fun similarTo(song: Song): List<Song> {
        val id = song.artistId.ifEmpty { searchArtists(song.artist).firstOrNull()?.id ?: return emptyList() }
        val radio = runCatching { songs(data("/artist/$id/radio?limit=30")) }.getOrDefault(emptyList())
        if (radio.isNotEmpty()) return radio
        return similarArtistsSongs(id, 5)
    }

    private fun similarArtistsSongs(artistId: String, each: Int): List<Song> =
        runCatching { related(artistId, 5) }.getOrDefault(emptyList()).flatMap { a ->
            runCatching { songs(data("/artist/${a.id}/top?limit=$each")) }.getOrDefault(emptyList())
        }

    /**
     * An Orbit: songs like an artist's (their radio) mixed with their own hits, shuffled
     * the same way all day so the mix stays put until tomorrow. As on the desktop app.
     */
    fun orbit(artistId: String): List<Song> {
        var radio = runCatching { songs(data("/artist/$artistId/radio?limit=40")) }.getOrDefault(emptyList())
        val top = songs(data("/artist/$artistId/top?limit=10"))
        if (radio.isEmpty()) radio = similarArtistsSongs(artistId, 6)
        val out = ArrayList<Song>()
        val hits = top.iterator()
        radio.forEachIndexed { i, s ->
            if (i % 4 == 0 && hits.hasNext()) out.add(hits.next())
            out.add(s)
        }
        hits.forEachRemaining { out.add(it) }
        // No song twice, even as a second copy of the same recording.
        val ids = HashSet<String>()
        val names = HashSet<String>()
        val unique = out.filter { ids.add(it.id) && names.add(it.title.lowercase() + "|" + it.artistId) }.toMutableList()
        // Shuffle with today's date as the seed.
        val day = System.currentTimeMillis() / 86_400_000L
        unique.shuffle(java.util.Random(day xor artistId.hashCode().toLong()))
        return unique
    }
}
