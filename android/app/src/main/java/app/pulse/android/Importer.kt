package app.pulse.android

import android.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import org.schabi.newpipe.extractor.ListExtractor
import org.schabi.newpipe.extractor.ServiceList
import org.schabi.newpipe.extractor.StreamingService
import org.schabi.newpipe.extractor.playlist.PlaylistInfo
import org.schabi.newpipe.extractor.stream.StreamInfoItem
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.util.zip.Deflater
import java.util.zip.Inflater

/** A finished import: the playlist's name, its songs, and how many matched a Deezer song. */
class Imported(val name: String, val songs: List<Song>, val matched: Int, val note: String? = null)

/**
 * Pulse share codes: a whole playlist packed into one line ("pulse1:…"), the same format
 * as the desktop app's "Copy share code", so playlists go both ways between PC and phone.
 * Deezer songs are just their number; others carry title, artist, length and cover.
 */
object ShareCode {
    const val PREFIX = "pulse1:"
    private const val FLAGS = Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP

    fun isCode(s: String) = s.trim().startsWith(PREFIX)

    fun encode(name: String, owner: String, songs: List<Song>): String {
        val t = JSONArray()
        for (s in songs) {
            if (s.onDeezer) t.put(s.id.toLong())
            else t.put(JSONObject().put("t", s.title).put("a", s.artist).put("d", s.duration).put("c", s.cover ?: JSONObject.NULL))
        }
        val json = JSONObject().put("n", name).put("o", owner).put("t", t).toString().toByteArray()
        val d = Deflater(9, true)
        d.setInput(json)
        d.finish()
        val out = ByteArrayOutputStream()
        val buf = ByteArray(8192)
        while (!d.finished()) out.write(buf, 0, d.deflate(buf))
        d.end()
        return PREFIX + Base64.encodeToString(out.toByteArray(), FLAGS)
    }

    /** The name, who shared it, and its entries (a Deezer number, or a song's details). */
    fun decode(code: String): Triple<String, String, List<Any>> {
        val bad = IOException("That share code is incomplete or damaged. Copy it again.")
        val body = code.trim().removePrefix(PREFIX).filterNot { it.isWhitespace() }.trimEnd('=')
        val packed = runCatching { Base64.decode(body, FLAGS) }.getOrElse { throw bad }
        val inf = Inflater(true)
        inf.setInput(packed)
        val out = ByteArrayOutputStream()
        val buf = ByteArray(8192)
        try {
            while (!inf.finished()) {
                val n = inf.inflate(buf)
                if (n == 0 && (inf.needsInput() || inf.needsDictionary())) break
                out.write(buf, 0, n)
                if (out.size() > 4 * 1024 * 1024) throw bad
            }
        } catch (e: java.util.zip.DataFormatException) {
            throw bad
        } finally {
            inf.end()
        }
        val o = runCatching { JSONObject(out.toString("UTF-8")) }.getOrElse { throw bad }
        val t = o.optJSONArray("t") ?: throw bad
        val entries = (0 until t.length()).map { i -> t.optJSONObject(i) ?: t.optLong(i) }
        return Triple(o.optString("n", "Shared playlist"), o.optString("o"), entries)
    }
}

/**
 * Importing playlists, like the desktop app: YouTube and SoundCloud playlists (read with
 * NewPipe), Spotify playlists and albums (their public embed page, up to 100 songs, or
 * song links pasted together), Pulse share codes and Pulse playlist links. Each song is
 * matched to Deezer for its cover and details; songs that aren't there are kept as they
 * are, and still play from YouTube.
 */
object Importer {
    private class Item(val title: String, val artist: String, val duration: Int, val art: String?, val video: String?)

    /** What the pasted text is, or null if Pulse can't import it. */
    fun detect(text: String): String? {
        val t = text.trim()
        val u = t.lowercase()
        return when {
            ShareCode.isCode(t) -> "Pulse share code"
            Account.readLink(t)?.first == "playlist" -> "Pulse playlist"
            u.contains("youtube.com/") || u.contains("youtu.be/") -> "YouTube"
            u.contains("spotify.com/") -> "Spotify"
            u.contains("soundcloud.com/") -> "SoundCloud"
            else -> null
        }
    }

    fun run(text: String, progress: (String, Int, Int) -> Unit, cancelled: () -> Boolean): Imported {
        val t = text.trim()
        val kind = detect(t) ?: throw IOException("Paste a link to a YouTube, Spotify or SoundCloud playlist, or a Pulse share code or playlist link.")
        if (kind == "Pulse share code") return shared(t, progress, cancelled)
        if (kind == "Pulse playlist") {
            progress("Getting the playlist…", 0, 0)
            val p = Account.playlist(Account.readLink(t)!!.second)
            return Imported(p.name, p.songs, p.songs.size)
        }
        progress("Reading the $kind playlist…", 0, 0)
        var note: String? = null
        val (name, items) = when (kind) {
            "YouTube" -> newPipe(ServiceList.YouTube, t, progress, cancelled)
            "SoundCloud" -> newPipe(ServiceList.SoundCloud, t, progress, cancelled)
            else -> {
                val ids = spotifyTrackIds(t)
                if (ids.size > 1) spotifySongs(ids, progress, cancelled) else spotify(t).also { (_, list) ->
                    if (list.size >= 100) note = "Spotify's link only shares the first 100 songs. To bring them all: in the Spotify app on a computer, open the playlist, press Ctrl+A then Ctrl+C, and paste here."
                }
            }
        }
        if (items.isEmpty()) throw IOException("That playlist is empty, private, or couldn't be read.")
        val songs = ArrayList<Song>()
        var matched = 0
        items.forEachIndexed { i, item ->
            if (cancelled()) throw IOException("Import cancelled.")
            progress("Matching songs…", i, items.size)
            var found = runCatching { Deezer.findTrack(item.artist, item.title, item.duration) }.getOrNull()
            if (found == null && kind != "Spotify" && item.artist.isNotEmpty()) {
                // Uploaders write "Artist - Song" and "Song - Artist"; try the other way round.
                Thread.sleep(220)
                found = runCatching { Deezer.findTrack(item.title, item.artist, item.duration) }.getOrNull()
            }
            Thread.sleep(220) // Deezer allows about 50 requests per 5 seconds
            val song = if (found != null) {
                matched++
                // A YouTube playlist plays the exact videos in it.
                if (item.video != null) found.copy(video = item.video) else found
            } else {
                val key = item.video ?: "${item.artist}-${item.title}-$i".lowercase().filter { it.isLetterOrDigit() || it == '-' }
                Song(
                    id = "imp:${kind.lowercase()}:$key", title = item.title, artist = item.artist.ifEmpty { "Unknown artist" },
                    album = "", cover = item.art, duration = item.duration, video = item.video,
                )
            }
            if (songs.none { it.id == song.id }) songs.add(song)
        }
        progress("Done", items.size, items.size)
        return Imported(name, songs, matched, note)
    }

    private fun shared(code: String, progress: (String, Int, Int) -> Unit, cancelled: () -> Boolean): Imported {
        val (name, _, entries) = ShareCode.decode(code)
        val songs = ArrayList<Song>()
        entries.forEachIndexed { i, e ->
            if (cancelled()) throw IOException("Import cancelled.")
            progress("Getting songs…", i, entries.size)
            if (e is JSONObject) {
                songs.add(
                    Song(
                        id = "imp:shared:${e.optString("a")}-${e.optString("t")}-$i".lowercase().filter { it.isLetterOrDigit() || it == '-' || it == ':' },
                        title = e.optString("t"), artist = e.optString("a"), album = "",
                        cover = e.optString("c").takeIf { it.isNotEmpty() && it != "null" }, duration = e.optInt("d"),
                    ),
                )
            } else {
                runCatching { Deezer.songById(e.toString()) }.getOrNull()?.let { songs.add(it) }
                Thread.sleep(120)
            }
        }
        return Imported(name, songs, songs.size)
    }

    // ------------------------------------------------------------------ YouTube and SoundCloud

    private fun newPipe(service: StreamingService, url: String, progress: (String, Int, Int) -> Unit, cancelled: () -> Boolean): Pair<String, List<Item>> {
        val info = runCatching { PlaylistInfo.getInfo(service, url) }.getOrElse { throw IOException("That playlist is private or couldn't be read.") }
        val all = ArrayList<StreamInfoItem>(info.relatedItems)
        var page = info.nextPage
        while (page != null && all.size < 1000 && !cancelled()) {
            progress("Reading the playlist… ${all.size} songs", 0, 0)
            val more: ListExtractor.InfoItemsPage<StreamInfoItem> = runCatching { PlaylistInfo.getMoreItems(service, url, page) }.getOrNull() ?: break
            all.addAll(more.items)
            page = if (more.hasNextPage()) more.nextPage else null
        }
        val youtube = service == ServiceList.YouTube
        val items = all.map { s ->
            val channel = s.uploaderName.orEmpty()
            val (artist, title) = if (youtube) splitVideoTitle(s.name.orEmpty(), channel) else (channel to s.name.orEmpty())
            Item(title, artist, s.duration.toInt().coerceAtLeast(0), s.thumbnails.maxByOrNull { it.width }?.url, if (youtube) s.url else null)
        }
        return info.name to items
    }

    /** "Artist - Song (Official Video)" → artist and song; otherwise the channel is the artist. */
    private fun splitVideoTitle(title: String, channel: String): Pair<String, String> {
        val clean = title.replace(Regex("""\s*[\(\[](official|lyric|lyrics|audio|music|visualizer|video|hd|4k)[^\)\]]*[\)\]]""", RegexOption.IGNORE_CASE), "").trim()
        for (sep in listOf(" - ", " – ", " — ", " | ")) {
            val i = clean.indexOf(sep)
            if (i > 0) return clean.substring(0, i).trim() to clean.substring(i + sep.length).trim()
        }
        val artist = channel.removeSuffix(" - Topic").removeSuffix("VEVO").removeSuffix("Vevo").trim()
        return artist to clean
    }

    // ------------------------------------------------------------------ Spotify

    private fun page(url: String): String {
        val req = okhttp3.Request.Builder().url(url).header("User-Agent", PulseDownloader.USER_AGENT).header("Accept-Language", "en").build()
        return Net.client.newCall(req).execute().use {
            if (!it.isSuccessful) throw IOException("Spotify didn't answer (HTTP ${it.code}).")
            it.body.string()
        }
    }

    private fun spotifyId(url: String): Pair<String, String>? {
        val parts = url.substringBefore('?').substringBefore('#').split('/').filter { it.isNotEmpty() }
        for ((i, p) in parts.withIndex()) {
            if (p in listOf("playlist", "album", "track")) {
                val id = parts.getOrNull(i + 1) ?: return null
                if (id.isNotEmpty() && id.all { it.isLetterOrDigit() }) return p to id
            }
        }
        return null
    }

    /** Spotify song ids (22 letters and digits) in pasted text, in order, once each. */
    private fun spotifyTrackIds(text: String): List<String> {
        val ids = LinkedHashMap<String, Int>()
        for (marker in listOf("track/", "track:")) {
            var at = text.indexOf(marker)
            while (at >= 0) {
                val id = text.substring(at + marker.length).take(22)
                if (id.length == 22 && id.all { it.isLetterOrDigit() } && id !in ids) ids[id] = at
                at = text.indexOf(marker, at + 1)
            }
        }
        return ids.entries.sortedBy { it.value }.map { it.key }
    }

    private fun spotify(url: String): Pair<String, List<Item>> {
        val (kind, id) = spotifyId(url) ?: throw IOException("That doesn't look like a Spotify playlist or album link.")
        return parseEmbed(page("https://open.spotify.com/embed/$kind/$id"))
    }

    private fun spotifySongs(ids: List<String>, progress: (String, Int, Int) -> Unit, cancelled: () -> Boolean): Pair<String, List<Item>> {
        val items = ArrayList<Item>()
        ids.forEachIndexed { i, id ->
            if (cancelled()) throw IOException("Import cancelled.")
            progress("Reading the Spotify songs…", i, ids.size)
            for (attempt in 0..1) {
                val found = runCatching { parseEmbed(page("https://open.spotify.com/embed/track/$id")).second }.getOrDefault(emptyList())
                if (found.isNotEmpty()) {
                    items.add(found[0])
                    break
                }
                Thread.sleep(600L * (attempt + 1))
            }
        }
        if (items.isEmpty()) throw IOException("Pulse couldn't read those Spotify songs. Check your connection and try again.")
        return "Spotify songs" to items
    }

    private fun parseEmbed(html: String): Pair<String, List<Item>> {
        val changed = IOException("Spotify's page has changed; Pulse couldn't read it.")
        val start = html.indexOf("<script id=\"__NEXT_DATA__\" type=\"application/json\">").takeIf { it >= 0 } ?: throw changed
        val from = html.indexOf('>', start) + 1
        val json = html.substring(from, html.indexOf("</script>", from).takeIf { it >= 0 } ?: throw changed)
        val entity = runCatching {
            JSONObject(json).getJSONObject("props").getJSONObject("pageProps").getJSONObject("state").getJSONObject("data").getJSONObject("entity")
        }.getOrElse { throw IOException("This Spotify playlist is private or doesn't exist.") }
        val name = entity.optString("name").ifEmpty { entity.optString("title") }.ifEmpty { "Spotify playlist" }
        val list = entity.optJSONArray("trackList") ?: JSONArray()
        val items = (0 until list.length()).map {
            val t = list.getJSONObject(it)
            Item(t.optString("title"), t.optString("subtitle").split(',').first().replace(' ', ' ').trim(), (t.optLong("duration") / 1000).toInt(), null, null)
        }.toMutableList()
        if (items.isEmpty() && entity.optString("title").isNotEmpty()) {
            val artist = entity.optString("subtitle").ifEmpty { entity.optJSONArray("artists")?.optJSONObject(0)?.optString("name").orEmpty() }
            items.add(Item(entity.optString("title"), artist, (entity.optLong("duration") / 1000).toInt(), null, null))
        }
        return name to items
    }
}
