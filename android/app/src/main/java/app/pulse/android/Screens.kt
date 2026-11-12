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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.AlertDialog
import androidx.compose.material.OutlinedTextField
import androidx.compose.material.Switch
import androidx.compose.material.SwitchDefaults
import androidx.compose.material.Text
import androidx.compose.material.TextButton
import androidx.compose.material.TextField
import androidx.compose.material.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import java.util.Calendar

/** Shows whichever page you're on. */
@Composable
fun Page(screen: Screen) {
    when (screen) {
        Screen.Home -> HomePage()
        Screen.Liked -> LikedPage()
        Screen.Playlists -> PlaylistsPage()
        Screen.Search -> SearchPage()
        Screen.Settings -> SettingsPage()
        Screen.TopSongs -> TopSongsPage()
        is Screen.ArtistPage -> ArtistScreen(screen)
        is Screen.AlbumPage -> AlbumScreen(screen)
        is Screen.PlaylistPage -> PlaylistScreen(screen)
        is Screen.OrbitPage -> OrbitScreen(screen)
    }
}

// ---------------------------------------------------------------------- home

@Composable
fun HomePage() {
    val app = LocalApp.current
    val top = Library.topArtists()
    var tries by remember { mutableStateOf(0) }
    val chart by load(tries) { Deezer.topSongs(10) }
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                PulseLogo(40.dp)
                Spacer(Modifier.width(10.dp))
                Row(
                    Modifier.weight(1f).heightIn(min = 48.dp).clip(RoundedCornerShape(24.dp)).background(P.raised)
                        .clickable(role = Role.Button, onClickLabel = "Search") { app.go(Screen.Search) }
                        .padding(horizontal = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(Ic.SEARCH, P.muted, 20.dp)
                    Spacer(Modifier.width(10.dp))
                    Text("Search songs, artists…", color = P.muted, fontSize = 15.sp)
                }
                IconButton(Ic.SETTINGS, "Settings") { app.go(Screen.Settings) }
            }
            Title(greeting(), Modifier.padding(start = 16.dp, end = 16.dp, top = 20.dp))
        }

        // Recently played, as tiles two by two (Liked Songs first if you have any).
        val tiles = buildList {
            if (Library.liked.isNotEmpty() && Library.recent.none { it.kind == Place.LIKED }) {
                add(Place(Place.LIKED, "", "Liked Songs", "", null))
            }
            addAll(Library.recent)
        }.take(6)
        if (tiles.isNotEmpty()) {
            item { Section("Recently played") }
            items(tiles.chunked(2)) { pair ->
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (p in pair) Tile(p, Modifier.weight(1f))
                    if (pair.size == 1) Spacer(Modifier.weight(1f))
                }
            }
        }

        // Your Orbits: a mix around each artist you play most.
        if (top.isNotEmpty()) {
            item {
                Section("Your Orbits")
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    items(top.take(6), key = { it.id }) { a ->
                        Card("${a.name} Orbit", "Their songs and more like them", a.picture, badge = "Orbit") {
                            app.go(Screen.OrbitPage(a.id, a.name, a.picture))
                        }
                    }
                }
            }
            item { ArtistsLike(top.first()) }
        } else {
            item { PopularArtists() }
        }

        item {
            Section("Top songs right now", "See all") { app.go(Screen.TopSongs) }
            Loading(chart, { tries++ }) { songs ->
                Column { songs.take(5).forEachIndexed { i, s -> SongRow(s, number = i + 1) { app.play(songs, i) } } }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

private fun greeting(): String = when (Calendar.getInstance().get(Calendar.HOUR_OF_DAY)) {
    in 5..11 -> "Good morning"
    in 12..17 -> "Good afternoon"
    else -> "Good evening"
}

/** A "Recently played" tile. */
@Composable
fun Tile(p: Place, modifier: Modifier) {
    val app = LocalApp.current
    Row(
        modifier.height(56.dp).clip(RoundedCornerShape(8.dp)).background(P.raised)
            .clickable(role = Role.Button) { open(app, p) }
            .semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (p.kind == Place.LIKED) {
            Box(Modifier.size(56.dp).background(P.accent), contentAlignment = Alignment.Center) { Icon(Ic.HEART, Color.White, 24.dp, fill = true) }
        } else {
            Cover(p.image, Modifier.size(56.dp), if (p.kind == Place.ARTIST) CircleShape else RoundedCornerShape(0.dp), if (p.kind == Place.ARTIST) Ic.PERSON else Ic.ALBUM)
        }
        Text(
            p.title, color = P.text, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 10.dp),
        )
    }
}

private fun open(app: App, p: Place) = when (p.kind) {
    Place.LIKED -> app.go(Screen.Liked)
    Place.ALBUM -> app.go(Screen.AlbumPage(p.id, p.title))
    Place.ARTIST -> app.go(Screen.ArtistPage(p.id, p.title))
    Place.PLAYLIST -> app.go(Screen.PlaylistPage(p.id))
    else -> app.go(Screen.OrbitPage(p.id, p.title.removeSuffix(" Orbit"), p.image))
}

@Composable
fun ArtistsLike(a: Played) {
    val related by load(a.id) { Deezer.related(a.id) }
    val r = related
    if (r is Loaded.Done && r.value.isNotEmpty()) {
        Section("Artists like ${a.name}")
        ArtistRow(r.value)
    }
}

@Composable
fun PopularArtists() {
    val artists by load(Unit) { Deezer.topArtists() }
    val r = artists
    if (r is Loaded.Done && r.value.isNotEmpty()) {
        Section("Popular artists")
        ArtistRow(r.value)
    }
}

@Composable
fun TopSongsPage() {
    val app = LocalApp.current
    var tries by remember { mutableStateOf(0) }
    val chart by load(tries) { Deezer.topSongs(100) }
    Column(Modifier.fillMaxSize()) {
        TopBar("Top songs right now")
        Loading(chart, { tries++ }) { songs ->
            LazyColumn {
                item { PlayButtons(songs) }
                itemsIndexed(songs, key = { _, s -> s.id }) { i, s -> SongRow(s, number = i + 1) { app.play(songs, i) } }
            }
        }
    }
}

// ---------------------------------------------------------------------- search

@Composable
fun SearchPage() {
    val app = LocalApp.current
    var text by remember { mutableStateOf("") }
    var query by remember { mutableStateOf("") }
    var tries by remember { mutableStateOf(0) }
    val focus = remember { FocusRequester() }
    val focusManager = LocalFocusManager.current
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    // Searches a moment after you stop typing.
    LaunchedEffect(text) {
        delay(450)
        if (text.trim().length >= 2) query = text.trim()
    }
    val results by load(query, tries) {
        if (query.isEmpty()) null else Triple(Deezer.searchSongs(query), runCatching { Deezer.searchArtists(query) }.getOrDefault(emptyList()), runCatching { Deezer.searchAlbums(query) }.getOrDefault(emptyList()))
    }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(end = 12.dp, top = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(Ic.BACK, "Back") { app.back() }
            TextField(
                value = text,
                onValueChange = { text = it },
                placeholder = { Text("Songs, artists, albums") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { focusManager.clearFocus(); query = text.trim() }),
                shape = RoundedCornerShape(24.dp),
                colors = TextFieldDefaults.textFieldColors(
                    backgroundColor = P.raised, focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent,
                    cursorColor = P.accent, textColor = P.text, placeholderColor = P.muted,
                ),
                modifier = Modifier.weight(1f).focusRequester(focus).semantics { contentDescription = "Search" },
            )
        }
        if (query.isEmpty()) {
            Message("Search for any song, artist or album.")
        } else {
            Loading(results, { tries++ }) { r ->
                if (r == null) return@Loading
                val (songs, artists, albums) = r
                if (songs.isEmpty() && artists.isEmpty() && albums.isEmpty()) {
                    Message("Nothing found for “$query”.")
                } else {
                    LazyColumn {
                        if (artists.isNotEmpty()) item { Section("Artists"); ArtistRow(artists) }
                        if (songs.isNotEmpty()) {
                            item { Section("Songs") }
                            itemsIndexed(songs, key = { _, s -> s.id }) { i, s -> SongRow(s) { app.play(songs, i) } }
                        }
                        if (albums.isNotEmpty()) item { Section("Albums"); AlbumRow(albums); Spacer(Modifier.height(24.dp)) }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------- artist, album, Orbit

@Composable
fun ArtistScreen(s: Screen.ArtistPage) {
    val app = LocalApp.current
    var tries by remember { mutableStateOf(0) }
    val page by load(s.id, tries) { Deezer.artistPage(s.id) }
    Column(Modifier.fillMaxSize()) {
        TopBar(s.name)
        Loading(page, { tries++ }) { p ->
            LaunchedEffect(s.id) { Library.visit(Place(Place.ARTIST, p.artist.id, p.artist.name, "Artist", p.artist.picture)) }
            LazyColumn {
                item {
                    PageHeader(p.artist.name, if (p.artist.fans > 0) "${fans(p.artist.fans)} fans" else "Artist", p.artist.picture, round = true, placeholder = Ic.PERSON)
                    Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Pill("Play", Ic.PLAY) { app.play(p.top) }
                        Pill("Orbit", Ic.ORBIT, filled = false) { app.go(Screen.OrbitPage(p.artist.id, p.artist.name, p.artist.picture)) }
                    }
                    if (p.top.isNotEmpty()) Section("Popular")
                }
                itemsIndexed(p.top.take(10), key = { _, t -> t.id }) { i, t -> SongRow(t, number = i + 1) { app.play(p.top, i) } }
                if (p.albums.isNotEmpty()) item { Section("Albums and singles"); AlbumRow(p.albums) }
                if (p.related.isNotEmpty()) item { Section("Fans also like"); ArtistRow(p.related) }
                item { Spacer(Modifier.height(24.dp)) }
            }
        }
    }
}

@Composable
fun AlbumScreen(s: Screen.AlbumPage) {
    val app = LocalApp.current
    var tries by remember { mutableStateOf(0) }
    val page by load(s.id, tries) { Deezer.albumPage(s.id) }
    Column(Modifier.fillMaxSize()) {
        TopBar(s.title)
        Loading(page, { tries++ }) { (album, songs) ->
            LaunchedEffect(s.id) { Library.visit(Place(Place.ALBUM, album.id, album.title, album.artist, album.cover)) }
            LazyColumn {
                item {
                    PageHeader(album.title, listOf(album.kind, album.year).filter { it.isNotEmpty() }.joinToString(" · "), album.cover)
                    if (album.artistId.isNotEmpty()) {
                        Text(
                            album.artist, color = P.accentText, fontSize = 15.sp, fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.fillMaxWidth().clickable(role = Role.Button) { app.go(Screen.ArtistPage(album.artistId, album.artist)) }.padding(12.dp),
                            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                        )
                    }
                    PlayButtons(songs)
                }
                itemsIndexed(songs, key = { _, t -> t.id }) { i, t -> SongRow(t, number = i + 1, showCover = false) { app.play(songs, i) } }
                item { Spacer(Modifier.height(24.dp)) }
            }
        }
    }
}

@Composable
fun OrbitScreen(s: Screen.OrbitPage) {
    val app = LocalApp.current
    var tries by remember { mutableStateOf(0) }
    val mix by load(s.artistId, tries) { Deezer.orbit(s.artistId) }
    Column(Modifier.fillMaxSize()) {
        TopBar("${s.name} Orbit")
        Loading(mix, { tries++ }) { songs ->
            LaunchedEffect(s.artistId) { Library.visit(Place(Place.ORBIT, s.artistId, "${s.name} Orbit", "Orbit", s.picture ?: songs.firstOrNull()?.cover)) }
            LazyColumn {
                item {
                    PageHeader("${s.name} Orbit", "${s.name}'s songs mixed with more like them. A new mix every day.", s.picture ?: songs.firstOrNull()?.cover)
                    PlayButtons(songs)
                }
                itemsIndexed(songs, key = { _, t -> t.id }) { i, t -> SongRow(t) { app.play(songs, i) } }
                item { Spacer(Modifier.height(24.dp)) }
            }
        }
    }
}

// ---------------------------------------------------------------------- your music

@Composable
fun LikedPage() {
    val app = LocalApp.current
    val songs = Library.liked.toList()
    LaunchedEffect(Unit) { if (songs.isNotEmpty()) Library.visit(Place(Place.LIKED, "", "Liked Songs", "Playlist", null)) }
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Title("Liked Songs", Modifier.padding(start = 16.dp, top = 20.dp))
            Text("${songs.size} songs", color = P.muted, fontSize = 14.sp, modifier = Modifier.padding(start = 16.dp, top = 2.dp))
            if (songs.isEmpty()) {
                Message("Songs you like show up here. Tap ⋮ on any song, or the heart while it plays.")
            } else {
                PlayButtons(songs)
            }
        }
        itemsIndexed(songs, key = { _, s -> s.id }) { i, s -> SongRow(s) { app.play(songs, i) } }
        item { Spacer(Modifier.height(24.dp)) }
    }
}

@Composable
fun PlaylistsPage() {
    val app = LocalApp.current
    var naming by remember { mutableStateOf(false) }
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Title("Playlists", Modifier.padding(start = 16.dp, top = 20.dp, bottom = 8.dp))
            Box(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) { Pill("New playlist", Ic.PLUS) { naming = true } }
        }
        if (Library.liked.isNotEmpty()) {
            item { PlaylistRow("Liked Songs", "${Library.liked.size} songs", null, liked = true) { app.go(Screen.Liked) } }
        }
        items(Library.playlists.toList(), key = { it.id }) { p ->
            PlaylistRow(p.name, "${p.songs.size} songs", p.songs.firstOrNull()?.cover) { app.go(Screen.PlaylistPage(p.id)) }
        }
        if (Library.playlists.isEmpty()) item { Message("Make a playlist, then add songs to it from the ⋮ next to any song.") }
    }
    if (naming) {
        NameDialog("New playlist", "", "Create", { naming = false }) { name ->
            naming = false
            val p = Library.newPlaylist(name)
            app.go(Screen.PlaylistPage(p.id))
        }
    }
}

@Composable
fun PlaylistRow(name: String, subtitle: String, cover: String?, liked: Boolean = false, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 72.dp).clickable(role = Role.Button, onClick = onClick)
            .semantics(mergeDescendants = true) {}.padding(horizontal = 16.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (liked) {
            Box(Modifier.size(60.dp).clip(RoundedCornerShape(8.dp)).background(P.accent), contentAlignment = Alignment.Center) { Icon(Ic.HEART, Color.White, 26.dp, fill = true) }
        } else {
            Cover(cover, Modifier.size(60.dp), placeholder = Ic.PLAYLISTS)
        }
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(name, color = P.text, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(subtitle, color = P.muted, fontSize = 14.sp)
        }
    }
}

@Composable
fun PlaylistScreen(s: Screen.PlaylistPage) {
    val app = LocalApp.current
    val p = Library.playlist(s.id)
    var renaming by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    if (p == null) {
        Column(Modifier.fillMaxSize()) {
            TopBar()
            Message("This playlist was deleted.")
        }
        return
    }
    LaunchedEffect(s.id) { Library.visit(Place(Place.PLAYLIST, p.id, p.name, "Playlist", p.songs.firstOrNull()?.cover)) }
    Column(Modifier.fillMaxSize()) {
        TopBar(p.name) {
            TextButton(onClick = { renaming = true }) { Text("Rename", color = P.accentText) }
            TextButton(onClick = { deleting = true }) { Text("Delete", color = P.accentText) }
        }
        LazyColumn {
            item {
                PageHeader(p.name, "${p.songs.size} songs", p.songs.firstOrNull()?.cover, placeholder = Ic.PLAYLISTS)
                if (p.songs.isEmpty()) Message("No songs yet. Add some from the ⋮ next to any song.") else PlayButtons(p.songs)
            }
            itemsIndexed(p.songs, key = { i, t -> "$i-${t.id}" }) { i, t ->
                SongRow(t, extra = listOf("Remove from this playlist" to { Library.removeFrom(p.id, i) })) { app.play(p.songs, i) }
            }
            item { Spacer(Modifier.height(24.dp)) }
        }
    }
    if (renaming) NameDialog("Rename playlist", p.name, "Save", { renaming = false }) { renaming = false; Library.rename(p.id, it) }
    if (deleting) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            title = { Text("Delete “${p.name}”?", color = P.text) },
            text = { Text("The playlist goes away. The songs stay where they are.", color = P.muted) },
            confirmButton = { TextButton(onClick = { deleting = false; Library.deletePlaylist(p.id); app.back() }) { Text("Delete", color = P.accentText) } },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text("Cancel", color = P.muted) } },
            backgroundColor = P.surface,
        )
    }
}

/** Asks for a name. */
@Composable
fun NameDialog(title: String, start: String, confirm: String, dismiss: () -> Unit, done: (String) -> Unit) {
    var name by remember { mutableStateOf(start) }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(title, color = P.text) },
        text = {
            OutlinedTextField(
                value = name, onValueChange = { name = it.take(80) }, singleLine = true, label = { Text("Name") },
                colors = TextFieldDefaults.outlinedTextFieldColors(textColor = P.text, focusedBorderColor = P.accent, focusedLabelColor = P.accentText, cursorColor = P.accent),
            )
        },
        confirmButton = { TextButton(onClick = { done(name) }) { Text(confirm, color = P.accentText) } },
        dismissButton = { TextButton(onClick = dismiss) { Text("Cancel", color = P.muted) } },
        backgroundColor = P.surface,
    )
}

/** Picks a playlist to add a song to (or makes a new one). */
@Composable
fun AddToPlaylistDialog(song: Song) {
    val app = LocalApp.current
    var naming by remember { mutableStateOf(false) }
    if (naming) {
        NameDialog("New playlist", "", "Create", { naming = false; app.addingToPlaylist = null }) { name ->
            Library.newPlaylist(name, song)
            app.addingToPlaylist = null
            app.tell("Added to $name")
        }
        return
    }
    AlertDialog(
        onDismissRequest = { app.addingToPlaylist = null },
        title = { Text("Add to playlist", color = P.text) },
        text = {
            Column {
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable(role = Role.Button) { naming = true },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(Ic.PLUS, P.accentText, 22.dp)
                    Spacer(Modifier.width(12.dp))
                    Text("New playlist", color = P.accentText, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                }
                for (p in Library.playlists) {
                    Text(
                        p.name, color = P.text, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable(role = Role.Button) {
                            app.addingToPlaylist = null
                            app.tell(if (Library.addTo(p.id, song)) "Added to ${p.name}" else "It's already in ${p.name}")
                        }.padding(vertical = 13.dp),
                    )
                }
            }
        },
        confirmButton = { TextButton(onClick = { app.addingToPlaylist = null }) { Text("Cancel", color = P.muted) } },
        backgroundColor = P.surface,
    )
}

// ---------------------------------------------------------------------- settings

@Composable
fun SettingsPage() {
    val uri = LocalUriHandler.current
    Column(Modifier.fillMaxSize()) {
        TopBar("Settings")
        LazyColumn {
            item {
                Section("Look")
                Text("Theme", color = P.text, fontSize = 16.sp, modifier = Modifier.padding(horizontal = 16.dp))
                Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for ((value, name) in listOf(Themes.SYSTEM to "Like my phone", Themes.DARK to "Dark", Themes.LIGHT to "Light")) {
                        Choice(name, Library.theme == value) { Library.pickTheme(value) }
                    }
                }
                Text("Color", color = P.text, fontSize = 16.sp, modifier = Modifier.padding(start = 16.dp, top = 12.dp))
                Row(Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                    Themes.accents.forEachIndexed { i, (name, strong, _) ->
                        val on = Library.accent == i
                        Box(
                            Modifier.size(52.dp).clip(CircleShape).clickable(role = Role.RadioButton) { Library.pickAccent(i) }
                                .semantics { contentDescription = name; selected = on },
                            contentAlignment = Alignment.Center,
                        ) {
                            Box(Modifier.size(if (on) 40.dp else 32.dp).clip(CircleShape).background(Color(strong)), contentAlignment = Alignment.Center) {
                                if (on) Box(Modifier.size(12.dp).clip(CircleShape).background(Color.White))
                            }
                        }
                    }
                }
                Toggle("Bigger text", "Makes all text in Pulse larger.", Library.largeText) { Library.useLargeText(it) }

                Section("Playing")
                Toggle("Autoplay", "When the queue ends, keep playing songs like the last one.", Library.autoplay) { Library.useAutoplay(it) }

                Section("About")
                Text("Pulse for Android ${BuildConfig.VERSION_NAME}", color = P.text, fontSize = 15.sp, modifier = Modifier.padding(horizontal = 16.dp))
                Text(
                    "Song info from Deezer, audio from YouTube. Your likes and playlists stay on this phone.",
                    color = P.muted, fontSize = 14.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
                )
                Row(Modifier.padding(horizontal = 8.dp)) {
                    TextButton(onClick = { uri.openUri("https://mercy123xz.github.io/Pulse/") }) { Text("Website", color = P.accentText) }
                    TextButton(onClick = { uri.openUri("https://mercy123xz.github.io/Pulse/privacy/") }) { Text("Privacy Policy", color = P.accentText) }
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }
}

@Composable
fun Choice(text: String, on: Boolean, onClick: () -> Unit) {
    Text(
        text, color = if (on) Color.White else P.text, fontSize = 14.sp, fontWeight = FontWeight.SemiBold,
        modifier = Modifier.heightIn(min = 44.dp).clip(RoundedCornerShape(22.dp)).background(if (on) P.accent else P.raised)
            .clickable(role = Role.RadioButton) { onClick() }.semantics { selected = on }
            .padding(horizontal = 16.dp, vertical = 12.dp),
    )
}

@Composable
fun Toggle(title: String, detail: String, on: Boolean, set: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(role = Role.Switch) { set(!on) }.semantics(mergeDescendants = true) {}
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, color = P.text, fontSize = 16.sp)
            Text(detail, color = P.muted, fontSize = 13.sp)
        }
        Switch(checked = on, onCheckedChange = null, colors = SwitchDefaults.colors(checkedThumbColor = P.accent, checkedTrackColor = P.accentSoft))
    }
}
