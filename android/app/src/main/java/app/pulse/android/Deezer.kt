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

    /** Genres offered when picking artists (Deezer genre id and name), as on the desktop app. */
    val GENRES = listOf(
        116 to "Rap/Hip Hop", 132 to "Pop", 165 to "R&B", 152 to "Rock", 85 to "Alternative", 113 to "Dance",
        106 to "Electro", 197 to "Latin Music", 464 to "Metal", 84 to "Country", 169 to "Soul & Funk", 122 to "Reggaeton",
    )

    /** Popular artists overall (genre 0) or in one genre. */
    fun genreArtists(genre: Int, limit: Int = 30): List<Artist> =
        if (genre == 0) topArtists(limit) else data("/genre/$genre/artists?limit=$limit").map(::artist)

    /** One song by its Deezer number. */
    fun songById(id: String): Song? = song(get("/track/$id"))

    /** "Songs like X": the artist's radio without the artist's own songs. */
    fun songsLike(artistId: String): List<Song> {
        val radio = runCatching { songs(data("/artist/$artistId/radio?limit=40")) }.getOrDefault(emptyList())
        val others = radio.filter { it.artistId != artistId }
        return (others.ifEmpty { similarArtistsSongs(artistId, 4) }).distinctBy { it.id }.take(20)
    }

    private fun norm(s: String) = s.lowercase().filter { it.isLetterOrDigit() }

    /** The title without "(feat. …)" and similar extras. */
    fun coreTitle(title: String): String {
        val lower = title.lowercase()
        val cut = listOf("(feat", "(ft.", "(with ", "[feat", " feat.", " ft. ").mapNotNull { m -> lower.indexOf(m).takeIf { it >= 0 } }.minOrNull()
        return (if (cut == null) title else title.substring(0, cut)).trim()
    }

    /** The Deezer song that matches a title, artist and length, as the desktop app's import does. */
    fun findTrack(artist: String, title: String, duration: Int): Song? {
        val wantTitle = norm(coreTitle(title))
        val wantArtist = norm(artist)
        fun pick(list: List<Song>): Song? = list.filter { t ->
            val got = norm(coreTitle(t.title))
            val titleOk = wantTitle.isNotEmpty() && (got == wantTitle || got.contains(wantTitle) || wantTitle.contains(got))
            val a = norm(t.artist)
            val artistOk = wantArtist.isEmpty() || (a.isNotEmpty() && (wantArtist.contains(a) || a.contains(wantArtist)))
            val lengthOk = duration == 0 || t.duration == 0 || kotlin.math.abs(t.duration - duration) <= 8
            titleOk && artistOk && lengthOk
        }.minByOrNull { if (duration == 0) 0 else kotlin.math.abs(it.duration - duration) }
        val precise = "artist:\"${artist.replace("\"", "")}\" track:\"${coreTitle(title).replace("\"", "")}\""
        pick(songs(data("/search/track?q=${q(precise)}&limit=10")))?.let { return it }
        return pick(songs(data("/search/track?q=${q("$artist ${coreTitle(title)}")}&limit=10")))
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
