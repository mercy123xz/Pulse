package app.pulse.android

import android.app.PendingIntent
import android.content.Intent
import androidx.annotation.OptIn
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import org.schabi.newpipe.extractor.services.youtube.YoutubeParsingHelper
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong

/**
 * Plays the music. As a media session service it keeps playing with the screen off, and
 * Android gives it the notification, lock-screen controls and headset/Bluetooth buttons.
 *
 * The app hands it songs (as Deezer info); here each one is found on YouTube and its audio
 * link put in before it plays.
 */
@OptIn(UnstableApi::class)
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null
    private val lookups = Executors.newFixedThreadPool(3)

    override fun onCreate() {
        super.onCreate()
        val requests = AtomicLong()
        val http = OkHttpDataSource.Factory(Net.client)
        val youtube = ResolvingDataSource.Factory(http) { spec -> forYouTube(spec, requests) }
        val player = ExoPlayer.Builder(this)
            .setMediaSourceFactory(DefaultMediaSourceFactory(youtube))
            .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true)
            .setHandleAudioBecomingNoisy(true) // pause when headphones are unplugged
            .setWakeMode(C.WAKE_MODE_NETWORK) // keep streaming with the screen off
            .build()
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        session = MediaSession.Builder(this, player).setCallback(Lookup()).setSessionActivity(open).build()
    }

    /** Songs added by the app arrive without audio: find each on YouTube first. */
    private inner class Lookup : MediaSession.Callback {
        override fun onAddMediaItems(
            mediaSession: MediaSession,
            controller: MediaSession.ControllerInfo,
            mediaItems: MutableList<MediaItem>,
        ): ListenableFuture<MutableList<MediaItem>> {
            val done = SettableFuture.create<MutableList<MediaItem>>()
            val jobs = mediaItems.map { item -> lookups.submit<MediaItem?> { runCatching { withAudio(item) }.getOrNull() } }
            Thread {
                val found = jobs.mapNotNull { runCatching { it.get() }.getOrNull() }.toMutableList()
                if (found.isEmpty() && mediaItems.isNotEmpty()) {
                    done.setException(java.io.IOException("Couldn't find this song on YouTube."))
                } else {
                    done.set(found)
                }
            }.start()
            return done
        }
    }

    private fun withAudio(item: MediaItem): MediaItem {
        if (item.localConfiguration != null) return item
        val song = Song.fromJson(item.mediaId)
        return item.buildUpon().setUri(YouTube.audioUrl(song)).build()
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
        lookups.shutdownNow()
        super.onDestroy()
    }

    companion object {
        /** YouTube's audio links want what its own players send (as NewPipe does it). */
        private val POST_BODY = byteArrayOf(0x78, 0)

        fun forYouTube(spec: DataSpec, requests: AtomicLong): DataSpec {
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
