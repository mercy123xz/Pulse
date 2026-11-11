package app.pulse.android

import org.json.JSONObject
import java.io.IOException
import java.net.URLEncoder

/** Song info, covers and charts from Deezer's free API (no account or key). */
object Deezer {
    private const val BASE = "https://api.deezer.com"

    fun search(q: String): List<Song> = songs("$BASE/search?q=${URLEncoder.encode(q, "UTF-8")}&limit=40")

    /** Today's top songs. */
    fun top(): List<Song> = songs("$BASE/chart/0/tracks?limit=40")

    private fun songs(url: String): List<Song> {
        val text = Net.client.newCall(okhttp3.Request.Builder().url(url).build()).execute().use {
            if (!it.isSuccessful) throw IOException("Deezer didn't answer (HTTP ${it.code}).")
            it.body.string()
        }
        val o = JSONObject(text)
        o.optJSONObject("error")?.let { throw IOException("Deezer: ${it.optString("message")}") }
        val data = o.optJSONArray("data") ?: return emptyList()
        return (0 until data.length()).map { data.getJSONObject(it) }
            .filter { it.optString("type", "track") == "track" && it.optBoolean("readable", true) }
            .map { t ->
                val album = t.optJSONObject("album")
                Song(
                    id = t.optLong("id").toString(),
                    title = t.optString("title"),
                    artist = t.optJSONObject("artist")?.optString("name").orEmpty(),
                    album = album?.optString("title").orEmpty(),
                    cover = album?.optString("cover_big")?.ifEmpty { null },
                    duration = t.optInt("duration"),
                )
            }
    }
}
