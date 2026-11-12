package app.pulse.android

import android.app.PendingIntent
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.annotation.OptIn
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import org.schabi.newpipe.extractor.services.youtube.YoutubeParsingHelper
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong

/**
 * Plays the music. As a media session service it keeps playing with the screen off, and
 * Android gives it the notification, lock-screen controls and headset/Bluetooth buttons.
 *
 * The app hands it songs (as Deezer info). Each song is looked up on YouTube only when the
 * player gets to it, so a queue can be as long as a whole playlist. Autoplay and the play
 * counts behind Orbits live here too, so they keep working with the app closed.
 */
@OptIn(UnstableApi::class)
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null
    private val work = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())

    /** The song Autoplay last looked up suggestions for, so it asks once per song. */
    private var autoplayFor: String? = null
    private var errorsInARow = 0

    override fun onCreate() {
        super.onCreate()
        val requests = AtomicLong()
        val http = OkHttpDataSource.Factory(Net.client)
        val youtube = ResolvingDataSource.Factory(http) { spec -> resolve(spec, requests) }
        val player = ExoPlayer.Builder(this)
            .setMediaSourceFactory(DefaultMediaSourceFactory(youtube))
            .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true)
            .setHandleAudioBecomingNoisy(true) // pause when headphones are unplugged
            .setWakeMode(C.WAKE_MODE_NETWORK) // keep streaming with the screen off
            .build()
        player.addListener(Watcher(player))
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        session = MediaSession.Builder(this, player).setCallback(Songs()).setSessionActivity(open).build()
    }

    /** Songs added by the app carry their info but no audio link: give each a pulse: link. */
    private inner class Songs : MediaSession.Callback {
        override fun onAddMediaItems(
            mediaSession: MediaSession,
            controller: MediaSession.ControllerInfo,
            mediaItems: MutableList<MediaItem>,
        ): ListenableFuture<MutableList<MediaItem>> =
            Futures.immediateFuture(mediaItems.map { withLink(it) }.toMutableList())
    }

    /** Autoplay, play counts, and skipping songs that can't be played. */
    private inner class Watcher(private val player: ExoPlayer) : Player.Listener {
        override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
            val song = mediaItem?.song() ?: return
            Library.notePlayed(song)
            topUp()
        }

        override fun onTimelineChanged(timeline: androidx.media3.common.Timeline, reason: Int) = topUp()
        override fun onRepeatModeChanged(repeatMode: Int) = topUp()

        override fun onIsPlayingChanged(isPlaying: Boolean) {
            if (isPlaying) errorsInARow = 0
        }

        override fun onPlayerError(error: PlaybackException) {
            player.currentMediaItem?.song()?.let { YouTube.forget(it.id) }
            // One song failing shouldn't stop the music: go on to the next (a few times at most).
            errorsInARow++
            if (errorsInARow <= 3 && player.hasNextMediaItem()) {
                player.seekToNextMediaItem()
                player.prepare()
                player.play()
            }
        }

        override fun onPlaybackStateChanged(state: Int) {
            // The queue ran out while Autoplay was still looking: it carries on when they arrive.
            if (state == Player.STATE_ENDED) topUp()
        }

        /** Autoplay: near the end of the queue, add songs like the one playing. */
        private fun topUp() {
            if (!Library.autoplay || player.repeatMode != Player.REPEAT_MODE_OFF) return
            val current = player.currentMediaItem?.song() ?: return
            val left = player.mediaItemCount - 1 - player.currentMediaItemIndex
            if (player.shuffleModeEnabled) {
                if (player.hasNextMediaItem()) return
            } else if (left > 1) {
                return
            }
            if (autoplayFor == current.id) return
            autoplayFor = current.id
            work.execute {
                val found = runCatching { Deezer.similarTo(current) }.getOrDefault(emptyList())
                main.post { addSuggestions(found) }
            }
        }

        private fun addSuggestions(found: List<Song>) {
            val queued = (0 until player.mediaItemCount).mapNotNull { player.getMediaItemAt(it).song()?.id }.toSet()
            val recent = Library.recentSongs.take(30).toSet()
            val add = found.filter { it.id !in queued && it.id !in recent }.distinctBy { it.id }.take(15)
            if (add.isEmpty()) return
            add.forEach { Library.autoplayIds.add(it.id) }
            val ended = player.playbackState == Player.STATE_ENDED
            player.addMediaItems(add.map { withLink(it.item()) })
            if (ended) {
                player.seekToNextMediaItem()
                player.prepare()
                player.play()
            }
        }
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    /** Swiped away from recent apps: keep going if music is playing, otherwise stop. */
    override fun onTaskRemoved(rootIntent: Intent?) {
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        session?.run {
            player.release()
            release()
        }
        session = null
        work.shutdownNow()
        super.onDestroy()
    }

    companion object {
        /** YouTube's audio links want what its own players send (as NewPipe does it). */
        private val POST_BODY = byteArrayOf(0x78, 0)

        fun withLink(item: MediaItem): MediaItem {
            if (item.localConfiguration != null) return item
            val uri = Uri.Builder().scheme("pulse").authority("song").appendQueryParameter("d", item.mediaId).build()
            return item.buildUpon().setUri(uri).build()
        }

        /** Runs on the player's loading thread: finds the song on YouTube when it's needed. */
        fun resolve(spec: DataSpec, requests: AtomicLong): DataSpec {
            if (spec.uri.scheme == "pulse") {
                val song = Song.fromJson(spec.uri.getQueryParameter("d") ?: throw java.io.IOException("No song."))
                return forYouTube(spec.buildUpon().setUri(Uri.parse(YouTube.audioUrl(song))).build(), requests)
            }
            return forYouTube(spec, requests)
        }

        private fun forYouTube(spec: DataSpec, requests: AtomicLong): DataSpec {
            val url = spec.uri.toString()
            if (spec.uri.path?.startsWith("/videoplayback") != true) return spec
            val headers = HashMap(spec.httpRequestHeaders)
            if (YoutubeParsingHelper.isWebStreamingUrl(url)) {
                headers["Origin"] = "https://www.youtube.com"
                headers["Referer"] = "https://www.youtube.com"
                headers["Sec-Fetch-Dest"] = "empty"
                headers["Sec-Fetch-Mode"] = "cors"
                headers["Sec-Fetch-Site"] = "cross-site"
            }
            headers["User-Agent"] =
                if (YoutubeParsingHelper.isVisionOsStreamingUrl(url)) YoutubeParsingHelper.getVisionOsUserAgent(null) else PulseDownloader.USER_AGENT
            val numbered = if (url.contains("&rn=")) url else url + "&rn=" + requests.getAndIncrement()
            return spec.buildUpon()
                .setUri(numbered)
                .setHttpMethod(DataSpec.HTTP_METHOD_POST)
                .setHttpBody(POST_BODY)
                .setHttpRequestHeaders(headers)
                .build()
        }
    }
}

/** A song as the player takes it: the Deezer info rides along as its id. */
fun Song.item(): MediaItem = MediaItem.Builder()
    .setMediaId(toJsonString())
    .setMediaMetadata(
        androidx.media3.common.MediaMetadata.Builder()
            .setTitle(title)
            .setArtist(artist)
            .setAlbumTitle(album)
            .setArtworkUri(cover?.let { Uri.parse(it) })
            .build(),
    )
    .build()

/** The song a queue item is, or null if it isn't one of Pulse's. */
fun MediaItem.song(): Song? = runCatching { Song.fromJson(mediaId) }.getOrNull()
