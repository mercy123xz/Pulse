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
) {
    fun toJson(): String = JSONObject()
        .put("id", id).put("title", title).put("artist", artist).put("album", album)
        .put("cover", cover ?: "").put("duration", duration)
        .toString()

    companion object {
        fun fromJson(s: String): Song {
            val o = JSONObject(s)
            return Song(
                o.getString("id"), o.optString("title"), o.optString("artist"), o.optString("album"),
                o.optString("cover").ifEmpty { null }, o.optInt("duration"),
            )
        }
    }
}
