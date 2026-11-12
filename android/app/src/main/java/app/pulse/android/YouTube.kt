package app.pulse.android

import org.schabi.newpipe.extractor.ServiceList
import org.schabi.newpipe.extractor.search.SearchInfo
import org.schabi.newpipe.extractor.services.youtube.linkHandler.YoutubeSearchQueryHandlerFactory
import org.schabi.newpipe.extractor.stream.DeliveryMethod
import org.schabi.newpipe.extractor.stream.StreamInfo
import org.schabi.newpipe.extractor.stream.StreamInfoItem
import android.content.Context
import org.json.JSONObject
import org.schabi.newpipe.extractor.services.youtube.YoutubeJavaScriptPlayerManager
import java.io.File
import java.io.IOException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.abs

/**
 * Finds a song on YouTube and gets a link to its audio, like the desktop app does with
 * yt-dlp: search YouTube Music's songs and ordinary videos (at the same time), score each
 * result by its title, artist and length, and take the best one's highest-quality audio.
 *
 * To start songs quickly: which video goes with which song is remembered on the phone,
 * links are kept for a while, the next songs are looked up ahead of time, and the same
 * song is never looked up twice at once.
 */
object YouTube {
    /** Song id → the YouTube video picked for it, so playing it again skips the search. */
    private val picked = ConcurrentHashMap<String, String>()

    /** Song id → its audio link and when it was found (links stop working after a few hours). */
    private val links = ConcurrentHashMap<String, Pair<String, Long>>()

    /** Lookups running right now, so asking again waits for the same one. */
    private val running = ConcurrentHashMap<String, CompletableFuture<String>>()

    private val pool = Executors.newFixedThreadPool(4)
    private var file: File? = null
    private val saving = AtomicBoolean(false)

    /** Reads the remembered matches, and gets YouTube ready so the first song starts sooner. */
    fun start(context: Context) {
        val f = File(context.filesDir, "matches.json")
        file = f
        pool.execute {
            runCatching {
                val o = JSONObject(f.readText())
                for (k in o.keys()) picked.putIfAbsent(k, o.getString(k))
            }
            // Fetches YouTube's player code once now instead of during the first song.
            runCatching { YoutubeJavaScriptPlayerManager.getSignatureTimestamp("dQw4w9WgXcQ") }
        }
    }

    private fun saveMatches() {
        val f = file ?: return
        if (!saving.compareAndSet(false, true)) return
        pool.execute {
            Thread.sleep(2000) // several new matches in a row are written once
            saving.set(false)
            runCatching {
                // Keeps the newest few thousand.
                val o = JSONObject()
                picked.entries.toList().takeLast(5000).forEach { (k, v) -> o.put(k, v) }
                val tmp = File(f.parentFile, "matches.json.tmp")
                tmp.writeText(o.toString())
                if (!tmp.renameTo(f)) f.writeText(o.toString())
            }
        }
    }

    fun audioUrl(song: Song): String {
        links[song.id]?.let { (url, at) -> if (System.currentTimeMillis() - at < 2 * 3600_000L) return url }
        val mine = CompletableFuture<String>()
        val job = running.putIfAbsent(song.id, mine) ?: mine
        if (job !== mine) {
            // Already being looked up (ahead of time, most likely): wait for that one.
            return try {
                job.get()
            } catch (e: java.util.concurrent.ExecutionException) {
                throw (e.cause as? IOException) ?: IOException(e.cause?.message ?: "Couldn't get this song.")
            }
        }
        try {
            val url = lookUp(song)
            mine.complete(url)
            return url
        } catch (e: Exception) {
            mine.completeExceptionally(e)
            throw (e as? IOException) ?: IOException(e.message ?: "Couldn't get this song.", e)
        } finally {
            running.remove(song.id, mine)
        }
    }

    private fun lookUp(song: Song): String {
        val video = song.video ?: picked[song.id] ?: find(song).also {
            picked[song.id] = it
            saveMatches()
        }
        val info = StreamInfo.getInfo(ServiceList.YouTube, video)
        val best = info.audioStreams
            .filter { it.isUrl && it.deliveryMethod == DeliveryMethod.PROGRESSIVE_HTTP }
            .maxByOrNull { it.averageBitrate }
            ?: throw IOException("YouTube has no audio for this song right now.")
        links[song.id] = best.content to System.currentTimeMillis()
        return best.content
    }

    /** Looks songs up in the background, so they start at once when their turn comes. */
    fun prefetch(songs: List<Song>) {
        for (s in songs) {
            if (links.containsKey(s.id) || running.containsKey(s.id)) continue
            pool.execute { runCatching { audioUrl(s) } }
        }
    }

    /** A song that wouldn't play: look it up from scratch next time (maybe another video). */
    fun forget(songId: String) {
        links.remove(songId)
        if (picked.remove(songId) != null) saveMatches()
    }

    private fun find(song: Song): String {
        val yt = ServiceList.YouTube
        val query = "${song.artist} ${song.title}"
        // Both searches at once; YouTube Music's songs are preferred when they match.
        val searches = listOf(YoutubeSearchQueryHandlerFactory.MUSIC_SONGS, YoutubeSearchQueryHandlerFactory.VIDEOS).map { kind ->
            CompletableFuture.supplyAsync({
                runCatching {
                    SearchInfo.getInfo(yt, yt.searchQHFactory.fromQuery(query, listOf(kind), "")).relatedItems.filterIsInstance<StreamInfoItem>()
                }.getOrDefault(emptyList())
            }, searchPool)
        }
        for (search in searches) {
            val found = runCatching { search.get() }.getOrDefault(emptyList())
            val best = found.take(10).maxByOrNull { score(song, it) }
            if (best != null && score(song, best) > 0) return best.url
        }
        throw IOException("Couldn't find “${song.title}” on YouTube.")
    }

    /** Searches have their own threads, so a lookup waiting on them never blocks another's. */
    private val searchPool = Executors.newCachedThreadPool()

    private fun simple(s: String) = s.lowercase().replace(Regex("[^\\p{L}\\p{N} ]"), " ").replace(Regex("\\s+"), " ").trim()

    /** Higher is better: the right title, the right artist, the right length, the original. */
    private fun score(song: Song, item: StreamInfoItem): Int {
        val name = simple(item.name)
        val title = simple(song.title.substringBefore(" (").substringBefore(" - "))
        val artist = simple(song.artist)
        var s = 1
        if (title.isNotEmpty() && name.contains(title)) s += 3
        if (artist.isNotEmpty() && (name.contains(artist) || simple(item.uploaderName ?: "").contains(artist))) s += 2
        if (song.duration > 0 && item.duration > 0) {
            val diff = abs(item.duration - song.duration)
            s += when {
                diff <= 3 -> 3
                diff <= 10 -> 1
                diff > 30 -> -3
                else -> 0
            }
        }
        for (word in listOf("live", "cover", "remix", "sped up", "slowed", "karaoke", "instrumental")) {
            if (name.contains(word) && !simple(song.title).contains(word)) s -= 2
        }
        return s
    }
}
