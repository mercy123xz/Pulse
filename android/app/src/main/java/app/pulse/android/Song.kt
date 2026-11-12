package app.pulse.android

import org.json.JSONObject

/** A song from Deezer's catalog. Audio comes from YouTube when it's played. */
data class Song(
    val id: String,
    val title: String,
    val artist: String,
    val album: String,
    val cover: String?,
    /** Length in seconds (0 if unknown). */
    val duration: Int,
    val artistId: String = "",
    val albumId: String = "",
    /** The artist's photo, when Deezer sent one with the song. */
    val artistPicture: String? = null,
    /** The exact YouTube video to play (songs imported from a YouTube playlist). */
    val video: String? = null,
) {
    /** A song Deezer knows (its id is a number), as opposed to one from an import. */
    val onDeezer: Boolean get() = id.isNotEmpty() && id.all { it.isDigit() }

    fun toJson(): JSONObject = JSONObject()
        .put("id", id).put("title", title).put("artist", artist).put("album", album)
        .put("cover", cover ?: "").put("duration", duration)
        .put("artistId", artistId).put("albumId", albumId).put("artistPicture", artistPicture ?: "")
        .put("video", video ?: "")

    fun toJsonString(): String = toJson().toString()

    companion object {
        fun fromJson(o: JSONObject): Song = Song(
            id = o.getString("id"),
            title = o.optString("title"),
            artist = o.optString("artist"),
            album = o.optString("album"),
            cover = o.optString("cover").ifEmpty { null },
            duration = o.optInt("duration"),
            artistId = o.optString("artistId"),
            albumId = o.optString("albumId"),
            artistPicture = o.optString("artistPicture").ifEmpty { null },
            video = o.optString("video").ifEmpty { null },
        )

        fun fromJson(s: String): Song = fromJson(JSONObject(s))
    }
}

/** An artist on Deezer. */
data class Artist(val id: String, val name: String, val picture: String?, val fans: Int = 0)

/** An album, single or EP on Deezer. */
data class Album(
    val id: String,
    val title: String,
    val artist: String,
    val artistId: String,
    val cover: String?,
    val year: String = "",
    val kind: String = "Album",
)

/** "3:45". */
fun clock(seconds: Int): String = "%d:%02d".format(seconds / 60, seconds % 60)
