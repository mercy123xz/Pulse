package app.pulse.android

import org.schabi.newpipe.extractor.ServiceList
import org.schabi.newpipe.extractor.search.SearchInfo
import org.schabi.newpipe.extractor.services.youtube.linkHandler.YoutubeSearchQueryHandlerFactory
import org.schabi.newpipe.extractor.stream.DeliveryMethod
import org.schabi.newpipe.extractor.stream.StreamInfo
import org.schabi.newpipe.extractor.stream.StreamInfoItem
import java.io.IOException
import java.util.concurrent.ConcurrentHashMap
import kotlin.math.abs

/**
 * Finds a song on YouTube and gets a link to its audio, like the desktop app does with
 * yt-dlp: search YouTube Music's songs (then ordinary videos), score each result by its
 * title, artist and length, and take the best one's highest-quality audio.
 */
object YouTube {
    /** Song id → the YouTube video picked for it, so playing it again skips the search. */
    private val picked = ConcurrentHashMap<String, String>()

    fun audioUrl(song: Song): String {
        val video = picked[song.id] ?: find(song).also { picked[song.id] = it }
        val info = StreamInfo.getInfo(ServiceList.YouTube, video)
        val best = info.audioStreams
            .filter { it.isUrl && it.deliveryMethod == DeliveryMethod.PROGRESSIVE_HTTP }
            .maxByOrNull { it.averageBitrate }
            ?: throw IOException("YouTube has no audio for this song right now.")
        return best.content
    }

    private fun find(song: Song): String {
        val yt = ServiceList.YouTube
        val query = "${song.artist} ${song.title}"
        for (kind in listOf(YoutubeSearchQueryHandlerFactory.MUSIC_SONGS, YoutubeSearchQueryHandlerFactory.VIDEOS)) {
            val found = runCatching {
                SearchInfo.getInfo(yt, yt.searchQHFactory.fromQuery(query, listOf(kind), "")).relatedItems.filterIsInstance<StreamInfoItem>()
            }.getOrDefault(emptyList())
            val best = found.take(10).maxByOrNull { score(song, it) }
            if (best != null && score(song, best) > 0) return best.url
        }
        throw IOException("Couldn't find “${song.title}” on YouTube.")
    }

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
