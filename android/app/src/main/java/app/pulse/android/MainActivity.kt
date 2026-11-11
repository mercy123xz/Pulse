package app.pulse.android

import android.content.ComponentName
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.CircularProgressIndicator
import androidx.compose.material.MaterialTheme
import androidx.compose.material.Text
import androidx.compose.material.TextField
import androidx.compose.material.TextFieldDefaults
import androidx.compose.material.darkColors
import androidx.compose.material.lightColors
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import coil3.compose.AsyncImage
import com.google.common.util.concurrent.MoreExecutors
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Pulse's colors, as on the desktop app and the website. */
private val Accent = Color(0xFF7C3AED)
private val AccentSoft = Color(0xFFA78BFA)

class MainActivity : ComponentActivity() {
    private var controller: MediaController? = null
    private val ui = PlayerUi()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent { PulseTheme { Home(ui, ::play, ::toggle, ::next) } }
    }

    override fun onStart() {
        super.onStart()
        val token = SessionToken(this, ComponentName(this, PlaybackService::class.java))
        val future = MediaController.Builder(this, token).buildAsync()
        future.addListener({
            val c = runCatching { future.get() }.getOrNull() ?: return@addListener
            controller = c
            c.addListener(object : Player.Listener {
                override fun onEvents(player: Player, events: Player.Events) = ui.update(player)
                override fun onPlayerError(error: PlaybackException) {
                    ui.message = "Couldn't play this song. Try another, or try again in a moment."
                }
            })
            ui.update(c)
        }, MoreExecutors.directExecutor())
    }

    override fun onStop() {
        controller?.release()
        controller = null
        super.onStop()
    }

    /** Plays `songs[index]`, with the next few queued after it. */
    private fun play(songs: List<Song>, index: Int) {
        val c = controller ?: return
        val song = songs[index]
        ui.message = "Finding “${song.title}” on YouTube…"
        c.setMediaItem(song.item())
        c.prepare()
        c.play()
        val next = songs.drop(index + 1).take(4)
        if (next.isNotEmpty()) c.addMediaItems(next.map { it.item() })
    }

    private fun toggle() {
        val c = controller ?: return
        if (c.isPlaying) c.pause() else c.play()
    }

    private fun next() {
        controller?.takeIf { it.hasNextMediaItem() }?.seekToNextMediaItem()
    }
}

/** A song as the player takes it: the Deezer info rides along; the service finds the audio. */
private fun Song.item(): MediaItem = MediaItem.Builder()
    .setMediaId(toJson())
    .setMediaMetadata(
        MediaMetadata.Builder()
            .setTitle(title)
            .setArtist(artist)
            .setAlbumTitle(album)
            .setArtworkUri(cover?.let { android.net.Uri.parse(it) })
            .build(),
    )
    .build()

/** What the mini player shows. */
class PlayerUi {
    var title by mutableStateOf<String?>(null)
    var artist by mutableStateOf("")
    var cover by mutableStateOf<String?>(null)
    var playing by mutableStateOf(false)
    var buffering by mutableStateOf(false)
    var hasNext by mutableStateOf(false)
    var message by mutableStateOf<String?>(null)

    fun update(p: Player) {
        val m = p.mediaMetadata
        title = m.title?.toString()
        artist = m.artist?.toString().orEmpty()
        cover = m.artworkUri?.toString()
        playing = p.isPlaying
        buffering = p.playbackState == Player.STATE_BUFFERING
        hasNext = p.hasNextMediaItem()
        if (p.isPlaying) message = null
    }
}

@Composable
fun PulseTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val colors = if (dark) {
        darkColors(primary = Accent, secondary = AccentSoft, background = Color(0xFF0B0B0E), surface = Color(0xFF16161B), onBackground = Color(0xFFEDEDF1), onSurface = Color(0xFFEDEDF1))
    } else {
        lightColors(primary = Accent, secondary = Accent, background = Color(0xFFF7F6FB), surface = Color.White, onBackground = Color(0xFF1B1A20), onSurface = Color(0xFF1B1A20))
    }
    MaterialTheme(colors = colors, content = content)
}

private val Muted @Composable get() = MaterialTheme.colors.onBackground.copy(alpha = 0.68f)

@Composable
fun Home(ui: PlayerUi, play: (List<Song>, Int) -> Unit, toggle: () -> Unit, next: () -> Unit) {
    var query by remember { mutableStateOf("") }
    var heading by remember { mutableStateOf("Top songs right now") }
    var songs by remember { mutableStateOf<List<Song>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val focus = LocalFocusManager.current

    fun load(q: String) {
        loading = true
        error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { if (q.isBlank()) Deezer.top() else Deezer.search(q) } }
            loading = false
            r.onSuccess {
                songs = it
                heading = if (q.isBlank()) "Top songs right now" else "Songs for “$q”"
                if (it.isEmpty()) error = "Nothing found for “$q”."
            }.onFailure { error = "Couldn't reach Deezer. Check your connection and try again." }
        }
    }
    LaunchedEffect(Unit) { load("") }

    Column(Modifier.fillMaxSize().background(MaterialTheme.colors.background).safeDrawingPadding()) {
        Row(Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            PulseLogo(Modifier.size(32.dp))
            Spacer(Modifier.width(10.dp))
            Text("Pulse", fontSize = 24.sp, fontWeight = FontWeight.ExtraBold, color = MaterialTheme.colors.onBackground, modifier = Modifier.semantics { heading() })
        }
        TextField(
            value = query,
            onValueChange = { query = it },
            placeholder = { Text("Search songs and artists") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = { focus.clearFocus(); load(query) }),
            shape = RoundedCornerShape(24.dp),
            colors = TextFieldDefaults.textFieldColors(
                backgroundColor = MaterialTheme.colors.surface,
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
                cursorColor = Accent,
                textColor = MaterialTheme.colors.onSurface,
                placeholderColor = Muted,
            ),
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).semantics { contentDescription = "Search songs and artists" },
        )
        Text(heading, fontSize = 20.sp, fontWeight = FontWeight.Bold, color = MaterialTheme.colors.onBackground, modifier = Modifier.padding(start = 20.dp, top = 18.dp, bottom = 6.dp).semantics { heading() })

        Box(Modifier.weight(1f).fillMaxWidth()) {
            when {
                loading -> CircularProgressIndicator(color = Accent, modifier = Modifier.align(Alignment.Center))
                error != null -> Text(error!!, color = Muted, modifier = Modifier.align(Alignment.Center).padding(24.dp))
                else -> LazyColumn {
                    itemsIndexed(songs, key = { _, s -> s.id }) { i, s -> SongRow(i + 1, s) { play(songs, i) } }
                }
            }
        }

        ui.message?.let {
            Text(it, color = Muted, fontSize = 13.sp, modifier = Modifier.padding(horizontal = 20.dp, vertical = 6.dp))
        }
        if (ui.title != null) MiniPlayer(ui, toggle, next)
    }
}

@Composable
fun SongRow(n: Int, s: Song, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp)
            .clickable(onClickLabel = "Play", role = Role.Button, onClick = onClick)
            .semantics(mergeDescendants = true) {}
            .padding(horizontal = 16.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("$n", color = Muted, fontSize = 14.sp, modifier = Modifier.width(30.dp))
        Cover(s.cover, Modifier.size(52.dp))
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(s.title, color = MaterialTheme.colors.onBackground, fontWeight = FontWeight.SemiBold, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(s.artist, color = Muted, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (s.duration > 0) Text("%d:%02d".format(s.duration / 60, s.duration % 60), color = Muted, fontSize = 13.sp)
    }
}

@Composable
fun MiniPlayer(ui: PlayerUi, toggle: () -> Unit, next: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(8.dp).clip(RoundedCornerShape(14.dp))
            .background(if (isSystemInDarkTheme()) Color(0xFF2A1F45) else Color(0xFFEDE7FB))
            .padding(horizontal = 10.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Cover(ui.cover, Modifier.size(48.dp))
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f).semantics(mergeDescendants = true) {}) {
            Text(ui.title.orEmpty(), color = MaterialTheme.colors.onBackground, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(ui.artist, color = Muted, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (ui.buffering) CircularProgressIndicator(color = Accent, strokeWidth = 2.dp, modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(4.dp))
        RoundButton(if (ui.playing) "Pause" else "Play", toggle) { PlayPauseGlyph(ui.playing) }
        if (ui.hasNext) RoundButton("Next song", next) { NextGlyph() }
    }
}

@Composable
fun RoundButton(label: String, onClick: () -> Unit, glyph: @Composable () -> Unit) {
    Box(
        Modifier.size(48.dp).clip(CircleShape).clickable(role = Role.Button, onClick = onClick).semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) { glyph() }
}

@Composable
fun PlayPauseGlyph(playing: Boolean) {
    val c = MaterialTheme.colors.onBackground
    Canvas(Modifier.size(22.dp)) {
        if (playing) {
            val w = size.width * 0.28f
            drawRoundRect(c, Offset(size.width * 0.14f, 0f), Size(w, size.height), CornerRadius(3f))
            drawRoundRect(c, Offset(size.width * 0.58f, 0f), Size(w, size.height), CornerRadius(3f))
        } else {
            drawPath(Path().apply { moveTo(size.width * 0.2f, 0f); lineTo(size.width, size.height / 2); lineTo(size.width * 0.2f, size.height); close() }, c)
        }
    }
}

@Composable
fun NextGlyph() {
    val c = MaterialTheme.colors.onBackground
    Canvas(Modifier.size(20.dp)) {
        drawPath(Path().apply { moveTo(0f, 0f); lineTo(size.width * 0.72f, size.height / 2); lineTo(0f, size.height); close() }, c)
        drawRoundRect(c, Offset(size.width * 0.8f, 0f), Size(size.width * 0.2f, size.height), CornerRadius(2f))
    }
}

@Composable
fun Cover(url: String?, modifier: Modifier) {
    Box(modifier.clip(RoundedCornerShape(8.dp)).background(MaterialTheme.colors.surface)) {
        if (url != null) AsyncImage(model = url, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
    }
}

/** The Pulse mark: five bars on a purple square. */
@Composable
fun PulseLogo(modifier: Modifier) {
    Canvas(modifier.semantics { contentDescription = "Pulse" }) {
        val u = size.width / 30f
        drawRoundRect(Accent, Offset.Zero, size, CornerRadius(8 * u))
        val bars = listOf(5.8f to (12f to 6f), 9.8f to (8.5f to 13f), 13.8f to (6f to 18f), 17.8f to (10f to 10f), 21.8f to (13f to 4f))
        for ((x, yh) in bars) drawRoundRect(Color.White, Offset(x * u, yh.first * u), Size(2.4f * u, yh.second * u), CornerRadius(1.2f * u))
    }
}
