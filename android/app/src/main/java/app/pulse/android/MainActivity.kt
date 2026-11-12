package app.pulse.android

import android.content.ComponentName
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.MoreExecutors
import kotlinx.coroutines.delay

class MainActivity : ComponentActivity() {
    private val app = App()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            PulseTheme {
                CompositionLocalProvider(LocalApp provides app) { Shell() }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        val token = SessionToken(this, ComponentName(this, PlaybackService::class.java))
        val future = MediaController.Builder(this, token).buildAsync()
        future.addListener({
            val c = runCatching { future.get() }.getOrNull() ?: return@addListener
            app.controller = c
            c.addListener(object : Player.Listener {
                override fun onEvents(player: Player, events: Player.Events) = app.player.update(player)
                override fun onPlayerError(error: PlaybackException) {
                    app.tell("Couldn't play this song. Trying the next one…")
                }
            })
            app.player.update(c)
        }, MoreExecutors.directExecutor())
    }

    override fun onStop() {
        app.controller?.release()
        app.controller = null
        super.onStop()
    }
}

@Composable
fun Shell() {
    val app = LocalApp.current
    BackHandler(enabled = app.playerOpen || app.queueOpen || app.stack.size > 1) { app.back() }
    // Keeps the seek bar moving.
    LaunchedEffect(Unit) {
        while (true) {
            delay(500)
            app.controller?.let { c -> if (c.isPlaying) app.player.position = c.currentPosition }
        }
    }
    LaunchedEffect(app.note) {
        if (app.note != null) {
            delay(2500)
            app.note = null
        }
    }
    Box(Modifier.fillMaxSize().background(P.bg).safeDrawingPadding()) {
        Column(Modifier.fillMaxSize()) {
            Box(Modifier.weight(1f).fillMaxWidth()) { Page(app.screen) }
            MiniPlayer()
            BottomBar()
        }
        AnimatedVisibility(app.playerOpen, enter = slideInVertically { it }, exit = slideOutVertically { it }) { FullPlayer() }
        AnimatedVisibility(app.queueOpen, enter = slideInVertically { it }, exit = slideOutVertically { it }) { QueuePage() }
        app.note?.let {
            Text(
                it, color = P.bg, fontSize = 14.sp,
                modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 140.dp, start = 24.dp, end = 24.dp)
                    .clip(RoundedCornerShape(10.dp)).background(P.text).padding(horizontal = 16.dp, vertical = 12.dp)
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
    }
    app.addingToPlaylist?.let { AddToPlaylistDialog(it) }
}
