package app.pulse.android

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.CircularProgressIndicator
import androidx.compose.material.DropdownMenu
import androidx.compose.material.DropdownMenuItem
import androidx.compose.material.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil3.compose.AsyncImage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

// ---------------------------------------------------------------------- loading

/** What a page is loading: nothing yet, the answer, or what went wrong. */
sealed interface Loaded<out T> {
    data object Waiting : Loaded<Nothing>
    data class Done<T>(val value: T) : Loaded<T>
    data class Failed(val message: String) : Loaded<Nothing>
}

/** Loads something off the main thread; loads again when a key changes. */
@Composable
fun <T> load(vararg keys: Any?, block: () -> T): State<Loaded<T>> =
    produceState<Loaded<T>>(Loaded.Waiting, *keys) {
        value = Loaded.Waiting
        value = withContext(Dispatchers.IO) {
            runCatching(block).fold(
                { Loaded.Done(it) },
                { Loaded.Failed("Couldn't reach Deezer. Check your connection and try again.") },
            )
        }
    }

/** A spinner, the content, or a message with Try again. */
@Composable
fun <T> Loading(state: Loaded<T>, retry: () -> Unit, content: @Composable (T) -> Unit) {
    when (state) {
        Loaded.Waiting -> Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) {
            CircularProgressIndicator(color = P.accent, modifier = Modifier.semantics { contentDescription = "Loading" })
        }
        is Loaded.Failed -> Message(state.message, "Try again", retry)
        is Loaded.Done -> content(state.value)
    }
}

@Composable
fun Message(text: String, action: String? = null, onAction: () -> Unit = {}) {
    Column(Modifier.fillMaxWidth().padding(28.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(text, color = P.muted, fontSize = 15.sp, textAlign = TextAlign.Center)
        if (action != null) {
            Spacer(Modifier.height(12.dp))
            Pill(action, onClick = onAction)
        }
    }
}

// ---------------------------------------------------------------------- text and buttons

@Composable
fun Title(text: String, modifier: Modifier = Modifier) {
    Text(
        text, color = P.text, fontSize = 26.sp, fontWeight = FontWeight.ExtraBold,
        modifier = modifier.semantics { heading() },
    )
}

/** A section heading, with an optional "See all". */
@Composable
fun Section(text: String, action: String? = null, onAction: () -> Unit = {}) {
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 22.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(text, color = P.text, fontSize = 19.sp, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f).semantics { heading() })
        if (action != null) {
            Text(
                action, color = P.accentText, fontSize = 14.sp, fontWeight = FontWeight.SemiBold,
                modifier = Modifier.clip(RoundedCornerShape(8.dp)).clickable(role = Role.Button, onClick = onAction).padding(horizontal = 8.dp, vertical = 12.dp),
            )
        }
    }
}

/** A rounded button: filled with the accent, or quiet. */
@Composable
fun Pill(text: String, icon: String? = null, filled: Boolean = true, onClick: () -> Unit) {
    val bg = if (filled) P.accent else P.raised
    val fg = if (filled) Color.White else P.text
    Row(
        Modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(24.dp)).background(bg)
            .clickable(role = Role.Button, onClick = onClick).padding(horizontal = 20.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.Center,
    ) {
        if (icon != null) {
            Icon(icon, fg, 20.dp, fill = icon == Ic.PLAY)
            Spacer(Modifier.width(8.dp))
        }
        Text(text, color = fg, fontSize = 15.sp, fontWeight = FontWeight.Bold)
    }
}

/** A round icon button, 48dp so it's easy to hit. */
@Composable
fun IconButton(d: String, label: String, tint: Color = P.text, size: Dp = 24.dp, fill: Boolean = false, onClick: () -> Unit) {
    Box(
        Modifier.size(48.dp).clip(CircleShape).clickable(role = Role.Button, onClickLabel = label, onClick = onClick)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) { Icon(d, tint, size, fill) }
}

/** Play and Shuffle, at the top of a list of songs. */
@Composable
fun PlayButtons(songs: List<Song>) {
    val app = LocalApp.current
    Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Pill("Play", Ic.PLAY) { app.play(songs) }
        Pill("Shuffle", Ic.SHUFFLE, filled = false) { app.play(songs, shuffle = true) }
    }
}

// ---------------------------------------------------------------------- pictures

@Composable
fun Cover(url: String?, modifier: Modifier, shape: Shape = RoundedCornerShape(8.dp), placeholder: String = Ic.NOTE) {
    Box(modifier.clip(shape).background(P.raised), contentAlignment = Alignment.Center) {
        Icon(placeholder, P.faint, 22.dp)
        if (url != null) AsyncImage(model = url, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
    }
}

/** A card in a row: square for albums and Orbits, round for artists. */
@Composable
fun Card(title: String, subtitle: String?, picture: String?, round: Boolean = false, badge: String? = null, size: Dp = 132.dp, onClick: () -> Unit) {
    Column(
        Modifier.width(size).clip(RoundedCornerShape(10.dp)).clickable(role = Role.Button, onClick = onClick)
            .semantics(mergeDescendants = true) {},
        horizontalAlignment = if (round) Alignment.CenterHorizontally else Alignment.Start,
    ) {
        Box {
            Cover(picture, Modifier.size(size), if (round) CircleShape else RoundedCornerShape(10.dp), if (round) Ic.PERSON else Ic.ALBUM)
            if (badge != null) {
                Text(
                    badge, color = Color.White, fontSize = 11.sp, fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(8.dp).clip(RoundedCornerShape(6.dp)).background(P.accent).padding(horizontal = 6.dp, vertical = 2.dp),
                )
            }
        }
        Spacer(Modifier.height(6.dp))
        Text(title, color = P.text, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis, textAlign = if (round) TextAlign.Center else TextAlign.Start)
        if (!subtitle.isNullOrEmpty()) Text(subtitle, color = P.muted, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
fun ArtistRow(artists: List<Artist>) {
    val app = LocalApp.current
    LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        items(artists, key = { it.id }) { a ->
            Card(a.name, if (a.fans > 0) "${fans(a.fans)} fans" else "Artist", a.picture, round = true, size = 116.dp) { app.go(Screen.ArtistPage(a.id, a.name)) }
        }
    }
}

@Composable
fun AlbumRow(albums: List<Album>) {
    val app = LocalApp.current
    LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        items(albums, key = { it.id }) { a ->
            Card(a.title, listOf(a.year, a.kind).filter { it.isNotEmpty() }.joinToString(" · "), a.cover) { app.go(Screen.AlbumPage(a.id, a.title)) }
        }
    }
}

fun fans(n: Int): String = when {
    n >= 1_000_000 -> "%.1fM".format(n / 1_000_000.0).replace(".0M", "M")
    n >= 1_000 -> "%.1fK".format(n / 1_000.0).replace(".0K", "K")
    else -> n.toString()
}

// ---------------------------------------------------------------------- songs

/**
 * One song: tap to play the list from it. The ⋮ button has Play next, Add to queue,
 * Like, Add to playlist, and the artist and album. `extra` adds items (like Remove).
 */
@Composable
fun SongRow(
    song: Song,
    number: Int? = null,
    showCover: Boolean = true,
    extra: List<Pair<String, () -> Unit>> = emptyList(),
    onClick: () -> Unit,
) {
    val app = LocalApp.current
    val playing = app.player.song?.id == song.id
    var menu by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp)
            .clickable(onClickLabel = "Play", role = Role.Button, onClick = onClick)
            .padding(start = 16.dp, end = 4.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(Modifier.weight(1f).semantics(mergeDescendants = true) {}, verticalAlignment = Alignment.CenterVertically) {
            if (number != null) Text("$number", color = if (playing) P.accentText else P.muted, fontSize = 14.sp, modifier = Modifier.widthIn(min = 28.dp))
            if (showCover) {
                Cover(song.cover, Modifier.size(50.dp))
                Spacer(Modifier.width(12.dp))
            }
            Column(Modifier.weight(1f)) {
                Text(song.title, color = if (playing) P.accentText else P.text, fontWeight = FontWeight.SemiBold, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(song.artist, color = P.muted, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            if (Library.isLiked(song)) Icon(Ic.HEART, P.accentText, 16.dp, fill = true, label = "Liked")
            if (song.duration > 0) {
                Spacer(Modifier.width(8.dp))
                Text(clock(song.duration), color = P.muted, fontSize = 13.sp)
            }
        }
        Box {
            IconButton(Ic.MORE, "More for ${song.title}", P.muted, fill = true) { menu = true }
            SongMenu(song, menu, { menu = false }, extra)
        }
    }
}

@Composable
fun SongMenu(song: Song, open: Boolean, close: () -> Unit, extra: List<Pair<String, () -> Unit>> = emptyList()) {
    val app = LocalApp.current
    DropdownMenu(expanded = open, onDismissRequest = close) {
        fun item(text: String, run: () -> Unit): Pair<String, () -> Unit> = text to run
        val items = buildList {
            add(item("Play next") { app.playNext(song) })
            add(item("Add to queue") { app.addToQueue(song) })
            add(item(if (Library.isLiked(song)) "Remove from Liked Songs" else "Add to Liked Songs") { app.like(song) })
            add(item("Add to playlist…") { app.addingToPlaylist = song })
            if (song.artistId.isNotEmpty()) add(item("Go to artist") { app.go(Screen.ArtistPage(song.artistId, song.artist)) })
            if (song.albumId.isNotEmpty()) add(item("Go to album") { app.go(Screen.AlbumPage(song.albumId, song.album)) })
            addAll(extra)
        }
        for ((text, run) in items) {
            DropdownMenuItem(onClick = { close(); run() }) { Text(text, color = P.text, fontSize = 15.sp) }
        }
    }
}

/** A header for a list page: picture, name, a line under it. */
@Composable
fun PageHeader(title: String, subtitle: String, picture: String?, round: Boolean = false, placeholder: String = Ic.ALBUM) {
    Column(Modifier.fillMaxWidth().padding(top = 8.dp, bottom = 4.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Cover(picture, Modifier.size(200.dp), if (round) CircleShape else RoundedCornerShape(12.dp), placeholder)
        Spacer(Modifier.height(14.dp))
        Text(
            title, color = P.text, fontSize = 24.sp, fontWeight = FontWeight.ExtraBold, textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 24.dp).semantics { heading() },
        )
        if (subtitle.isNotEmpty()) Text(subtitle, color = P.muted, fontSize = 14.sp, textAlign = TextAlign.Center, modifier = Modifier.padding(horizontal = 24.dp, vertical = 4.dp))
    }
}

/** The top bar of a page that isn't a tab: a back button and the page's name. */
@Composable
fun TopBar(title: String = "", actions: @Composable () -> Unit = {}) {
    val app = LocalApp.current
    Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton(Ic.BACK, "Back") { app.back() }
        Text(title, color = P.text, fontSize = 17.sp, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        actions()
    }
}
