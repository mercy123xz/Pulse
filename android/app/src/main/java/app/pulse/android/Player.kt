package app.pulse.android

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.CircularProgressIndicator
import androidx.compose.material.Slider
import androidx.compose.material.SliderDefaults
import androidx.compose.material.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.media3.common.Player

/** The bar at the bottom: Liked, Home, Playlists. */
@Composable
fun BottomBar() {
    val app = LocalApp.current
    val root = app.stack.first()
    Row(
        Modifier.fillMaxWidth().background(P.surface).padding(horizontal = 8.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.SpaceAround,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TabButton("Liked", root == Screen.Liked, { Icon(Ic.HEART, it, 24.dp, fill = root == Screen.Liked) }) { app.tab(Screen.Liked) }
        TabButton("Home", root == Screen.Home, { PulseLogo(28.dp) }) { app.tab(Screen.Home) }
        TabButton("Playlists", root == Screen.Playlists, { Icon(Ic.PLAYLISTS, it, 24.dp) }) { app.tab(Screen.Playlists) }
    }
}

@Composable
private fun TabButton(label: String, on: Boolean, icon: @Composable (Color) -> Unit, onClick: () -> Unit) {
    val color = if (on) P.text else P.muted
    Column(
        Modifier.widthIn(min = 80.dp).heightIn(min = 56.dp).clip(RoundedCornerShape(12.dp))
            .clickable(role = Role.Tab, onClick = onClick)
            .semantics(mergeDescendants = true) { stateDescription = if (on) "Selected" else "" },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        icon(color)
        Spacer(Modifier.height(3.dp))
        Text(label, color = color, fontSize = 12.sp, fontWeight = if (on) FontWeight.Bold else FontWeight.SemiBold)
    }
}

/** The small player above the bottom bar. Tap it for the full player. */
@Composable
fun MiniPlayer() {
    val app = LocalApp.current
    val p = app.player
    val song = p.song ?: return
    Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp).clip(RoundedCornerShape(12.dp)).background(P.player)) {
        Row(
            Modifier.fillMaxWidth().clickable(onClickLabel = "Open the player", role = Role.Button) { app.playerOpen = true }
                .padding(start = 8.dp, end = 2.dp, top = 6.dp, bottom = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Cover(song.cover, Modifier.size(46.dp))
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f).semantics(mergeDescendants = true) {}) {
                Text(song.title, color = P.text, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(song.artist, color = P.muted, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            if (p.buffering) CircularProgressIndicator(color = P.accentText, strokeWidth = 2.dp, modifier = Modifier.size(20.dp).semantics { contentDescription = "Loading" })
            val liked = Library.isLiked(song)
            IconButton(Ic.HEART, if (liked) "Remove from Liked Songs" else "Add to Liked Songs", if (liked) P.accentText else P.text, 22.dp, fill = liked) { app.like(song) }
            IconButton(if (p.playing) Ic.PAUSE else Ic.PLAY, if (p.playing) "Pause" else "Play", P.text, 24.dp, fill = true) { app.toggle() }
            if (p.hasNext) IconButton(Ic.NEXT, "Next song", P.text, 22.dp, fill = true) { app.next() }
        }
        // How far into the song.
        val frac = if (p.duration > 0) (p.position.toFloat() / p.duration).coerceIn(0f, 1f) else 0f
        Box(Modifier.fillMaxWidth().height(2.dp).background(P.divider)) {
            Box(Modifier.fillMaxWidth(frac).height(2.dp).background(P.accentText))
        }
    }
}

/** The full player: big cover, seek bar, all the controls. */
@Composable
fun FullPlayer() {
    val app = LocalApp.current
    val p = app.player
    val song = p.song
    var lyrics by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().background(P.bg)) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(Ic.DOWN, "Close the player") { app.playerOpen = false }
            Text("Now playing", color = P.muted, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center, modifier = Modifier.weight(1f))
            IconButton(Ic.LYRICS, if (lyrics) "Hide lyrics" else "Show lyrics", if (lyrics) P.accentText else P.text) { lyrics = !lyrics }
            IconButton(Ic.QUEUE, "Queue") { app.queueOpen = true }
        }
        if (song == null) {
            Message("Nothing is playing.")
            return
        }
        Column(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 24.dp), verticalArrangement = Arrangement.Center) {
            if (lyrics) {
                Box(Modifier.weight(1f).fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(P.surface)) { LyricsView(song, Modifier.fillMaxSize()) }
            } else {
                Cover(song.cover, Modifier.fillMaxWidth().aspectRatio(1f), RoundedCornerShape(14.dp))
            }
            Spacer(Modifier.height(24.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(song.title, color = P.text, fontSize = 22.sp, fontWeight = FontWeight.ExtraBold, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text(
                        song.artist, color = P.accentText, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.clip(RoundedCornerShape(6.dp))
                            .clickable(enabled = song.artistId.isNotEmpty(), role = Role.Button) { app.go(Screen.ArtistPage(song.artistId, song.artist)) }
                            .padding(vertical = 4.dp),
                    )
                }
                val liked = Library.isLiked(song)
                IconButton(Ic.HEART, if (liked) "Remove from Liked Songs" else "Add to Liked Songs", if (liked) P.accentText else P.text, 28.dp, fill = liked) { app.like(song) }
            }
            Spacer(Modifier.height(12.dp))
            SeekBar(p, app)
            Spacer(Modifier.height(8.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                IconButton(Ic.SHUFFLE, if (p.shuffle) "Shuffle is on" else "Shuffle is off", if (p.shuffle) P.accentText else P.muted) { app.setShuffle(!p.shuffle) }
                IconButton(Ic.PREV, "Previous song", P.text, 30.dp, fill = true) { app.previous() }
                Box(
                    Modifier.size(72.dp).clip(CircleShape).background(P.accent)
                        .clickable(role = Role.Button, onClickLabel = if (p.playing) "Pause" else "Play") { app.toggle() }
                        .semantics { contentDescription = if (p.playing) "Pause" else "Play" },
                    contentAlignment = Alignment.Center,
                ) {
                    if (p.buffering) CircularProgressIndicator(color = Color.White, strokeWidth = 2.dp, modifier = Modifier.size(64.dp))
                    Icon(if (p.playing) Ic.PAUSE else Ic.PLAY, Color.White, 30.dp, fill = true)
                }
                IconButton(Ic.NEXT, "Next song", P.text, 30.dp, fill = true) { app.next() }
                val (repeatIcon, repeatLabel) = when (p.repeat) {
                    Player.REPEAT_MODE_ALL -> Ic.REPEAT to "Repeat is on"
                    Player.REPEAT_MODE_ONE -> Ic.REPEAT_ONE to "Repeating this song"
                    else -> Ic.REPEAT to "Repeat is off"
                }
                IconButton(repeatIcon, repeatLabel, if (p.repeat != Player.REPEAT_MODE_OFF) P.accentText else P.muted) { app.cycleRepeat() }
            }
        }
        Row(
            Modifier.fillMaxWidth().clickable(role = Role.Switch) { Library.useAutoplay(!Library.autoplay) }
                .semantics(mergeDescendants = true) { stateDescription = if (Library.autoplay) "On" else "Off" }
                .padding(horizontal = 24.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Ic.AUTOPLAY, if (Library.autoplay) P.accentText else P.muted, 22.dp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text("Autoplay", color = P.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                Text(if (Library.autoplay) "Keeps playing songs like this when the queue ends" else "Off: stops when the queue ends", color = P.muted, fontSize = 13.sp)
            }
            Text(if (Library.autoplay) "On" else "Off", color = if (Library.autoplay) P.accentText else P.muted, fontSize = 14.sp, fontWeight = FontWeight.Bold)
        }
        Spacer(Modifier.height(8.dp))
    }
}

@Composable
private fun SeekBar(p: PlayerState, app: App) {
    // While dragging, the bar follows your finger; the song jumps when you let go.
    var dragging by remember { mutableStateOf<Float?>(null) }
    val total = p.duration.coerceAtLeast(1)
    val shown = dragging ?: (p.position.toFloat() / total).coerceIn(0f, 1f)
    Slider(
        value = shown,
        onValueChange = { dragging = it },
        onValueChangeFinished = {
            dragging?.let { app.seek((it * total).toLong()) }
            dragging = null
        },
        colors = SliderDefaults.colors(thumbColor = P.accentText, activeTrackColor = P.accentText, inactiveTrackColor = P.divider),
        modifier = Modifier.semantics { contentDescription = "Position in the song" },
    )
    Row(Modifier.fillMaxWidth()) {
        Text(clock(((shown * total) / 1000).toInt()), color = P.muted, fontSize = 12.sp, modifier = Modifier.weight(1f))
        Text(clock((p.duration / 1000).toInt()), color = P.muted, fontSize = 12.sp)
    }
}

/** What plays next. Tap a song to jump to it. */
@Composable
fun QueuePage() {
    val app = LocalApp.current
    val q = app.player.queue
    Column(Modifier.fillMaxSize().background(P.bg)) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(Ic.DOWN, "Close the queue") { app.queueOpen = false }
            Text("Queue", color = P.text, fontSize = 17.sp, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
        }
        if (q.isEmpty()) {
            Message("Nothing in the queue.")
            return
        }
        LazyColumn {
            item { Section("Now playing") }
            item { QueueRow(q.first(), app) }
            if (q.size > 1) item { Section("Next up") }
            items(q.drop(1), key = { "${it.index}-${it.song.id}" }) { QueueRow(it, app) }
            if (Library.autoplay && app.player.repeat == Player.REPEAT_MODE_OFF) {
                item { Text("Autoplay adds songs like these when the queue gets short.", color = P.muted, fontSize = 13.sp, modifier = Modifier.padding(16.dp)) }
            }
        }
    }
}

@Composable
private fun QueueRow(e: QueueEntry, app: App) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.weight(1f)) {
            SongRow(
                e.song,
                extra = if (e.current) emptyList() else listOf("Remove from queue" to { app.removeFromQueue(e.index) }),
            ) { if (!e.current) app.jumpTo(e.index) }
        }
        if (e.song.id in Library.autoplayIds) {
            Text(
                "Autoplay", color = P.accentText, fontSize = 11.sp, fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(end = 12.dp).clip(RoundedCornerShape(6.dp)).background(P.raised).padding(horizontal = 6.dp, vertical = 2.dp),
            )
        }
    }
}
