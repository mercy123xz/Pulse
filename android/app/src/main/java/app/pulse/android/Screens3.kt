package app.pulse.android

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.CircularProgressIndicator
import androidx.compose.material.LinearProgressIndicator
import androidx.compose.material.OutlinedTextField
import androidx.compose.material.Text
import androidx.compose.material.TextButton
import androidx.compose.material.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// ---------------------------------------------------------------------- helpers

fun copyText(context: Context, text: String) {
    context.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("Pulse", text))
}

/** Android's share sheet (Messages, Discord, …). */
fun shareText(context: Context, text: String, title: String) {
    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
    context.startActivity(Intent.createChooser(send, title).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
}

@Composable
fun Field(label: String, value: String, onChange: (String) -> Unit, password: Boolean = false, keyboard: KeyboardType = KeyboardType.Text, single: Boolean = true) {
    OutlinedTextField(
        value = value, onValueChange = onChange, label = { Text(label) }, singleLine = single,
        visualTransformation = if (password) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None,
        keyboardOptions = KeyboardOptions(keyboardType = if (password) KeyboardType.Password else keyboard),
        colors = TextFieldDefaults.outlinedTextFieldColors(textColor = P.text, focusedBorderColor = P.accent, focusedLabelColor = P.accentText, cursorColor = P.accent, unfocusedLabelColor = P.muted, unfocusedBorderColor = P.divider),
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
    )
}

/** Runs `work` off the main thread; `busy` while it runs, `error` if it throws. */
class Job {
    var busy by mutableStateOf(false)
    var error by mutableStateOf<String?>(null)
    var info by mutableStateOf<String?>(null)
}

@Composable
fun rememberJob() = remember { Job() }

@Composable
fun JobStatus(job: Job) {
    job.error?.let { Text(it, color = Color(0xFFFF8A80), fontSize = 14.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp).semantics { contentDescription = "Error: $it" }) }
    job.info?.let { Text(it, color = P.accentText, fontSize = 14.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp)) }
    if (job.busy) LinearProgressIndicator(color = P.accent, backgroundColor = P.raised, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp))
}

// ---------------------------------------------------------------------- your account

@Composable
fun AccountScreen() {
    Column(Modifier.fillMaxSize()) {
        TopBar("Account")
        if (Account.username == null) SignIn() else SignedIn(Account.username!!)
    }
}

@Composable
private fun SignIn() {
    var mode by remember { mutableStateOf("in") } // in, up, forgot, reset
    var who by remember { mutableStateOf("") }
    var mail by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    val job = rememberJob()
    val scope = rememberCoroutineScope()
    fun run(work: () -> Unit, then: () -> Unit = {}) {
        if (job.busy) return
        job.busy = true
        job.error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching(work) }
            job.busy = false
            r.onSuccess { then() }.onFailure { job.error = it.message ?: "Something went wrong." }
        }
    }
    LazyColumn {
        item {
            Title(
                when (mode) { "up" -> "Create your account"; "forgot" -> "Forgot your password?"; "reset" -> "Choose a new password"; else -> "Sign in to Pulse" },
                Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp),
            )
            Text(
                when (mode) {
                    "up" -> "An account lets you share playlists and have a profile. Everything else works without one."
                    "forgot" -> "Enter your username or email. We'll email you a 6-digit code. Check your spam folder too."
                    "reset" -> "Enter the code from the email and your new password."
                    else -> "The same account as on the Pulse app for Windows."
                },
                color = P.muted, fontSize = 15.sp, modifier = Modifier.padding(16.dp),
            )
            when (mode) {
                "up" -> {
                    Field("Username", who, { who = it.lowercase().filter { c -> c.isLetterOrDigit() || c == '_' || c == '.' }.take(20) })
                    Field("Email (for password resets)", mail, { mail = it }, keyboard = KeyboardType.Email)
                    Field("Password (at least 8 characters)", password, { password = it }, password = true)
                }
                "forgot" -> Field("Username or email", who, { who = it })
                "reset" -> {
                    Field("6-digit code", code, { code = it.filter { c -> c.isDigit() }.take(6) }, keyboard = KeyboardType.Number)
                    Field("New password", password, { password = it }, password = true)
                }
                else -> {
                    Field("Username or email", who, { who = it })
                    Field("Password", password, { password = it }, password = true)
                }
            }
            JobStatus(job)
            Box(Modifier.padding(16.dp)) {
                when (mode) {
                    "up" -> Pill("Create account") {
                        when {
                            who.length < 3 -> job.error = "Usernames are 3 to 20 characters."
                            !Account.validEmail(mail) -> job.error = "That email address doesn't look right."
                            password.length < 8 -> job.error = "Passwords need at least 8 characters."
                            else -> run({ Account.signUp(who, mail, password) })
                        }
                    }
                    "forgot" -> Pill("Send code") { run({ job.info = null; val m = Account.resetStart(who); job.info = m }) { mode = "reset" } }
                    "reset" -> Pill("Save and sign in") { run({ Account.resetFinish(who, code, password) }) }
                    else -> Pill("Sign in") { run({ Account.signIn(who, password) }) }
                }
            }
            Row(Modifier.padding(horizontal = 8.dp)) {
                if (mode != "in") TextButton(onClick = { mode = "in"; job.error = null }) { Text("Sign in instead", color = P.accentText) }
                if (mode == "in") TextButton(onClick = { mode = "up"; job.error = null }) { Text("Create an account", color = P.accentText) }
                if (mode == "in") TextButton(onClick = { mode = "forgot"; job.error = null }) { Text("Forgot password?", color = P.accentText) }
            }
            val uri = androidx.compose.ui.platform.LocalUriHandler.current
            TextButton(onClick = { uri.openUri(Account.SITE + "privacy/") }, modifier = Modifier.padding(horizontal = 8.dp)) { Text("Privacy Policy", color = P.muted) }
        }
    }
}

@Composable
private fun SignedIn(name: String) {
    val app = LocalApp.current
    val context = LocalContext.current
    var tries by remember { mutableStateOf(0) }
    val me by load(name, tries) { Account.me() }
    var bio by remember { mutableStateOf<String?>(null) }
    val job = rememberJob()
    val scope = rememberCoroutineScope()
    LazyColumn {
        item {
            Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(64.dp).clip(CircleShape).background(P.accent), contentAlignment = Alignment.Center) {
                    Text(name.take(1).uppercase(), color = Color.White, fontSize = 28.sp, fontWeight = FontWeight.Bold)
                }
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text("@$name", color = P.text, fontSize = 22.sp, fontWeight = FontWeight.ExtraBold)
                    if (Account.email.isNotEmpty()) Text(Account.email, color = P.muted, fontSize = 14.sp)
                }
            }
            Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Pill("Share profile", filled = false) { shareText(context, Account.profileLink(name), "Share your Pulse profile") }
                Pill("Sign out", filled = false) { scope.launch { withContext(Dispatchers.IO) { Account.signOut() } } }
            }
        }
        item {
            Loading(me, { tries++ }) { (person, lists) ->
                Column {
                    if (bio == null) bio = person.bio
                    Section("About you")
                    Field("Bio (shown on your profile)", bio ?: "", { bio = it.take(300) }, single = false)
                    JobStatus(job)
                    Box(Modifier.padding(horizontal = 16.dp, vertical = 6.dp)) {
                        Pill("Save bio", filled = false) {
                            job.busy = true
                            job.error = null
                            scope.launch {
                                val r = withContext(Dispatchers.IO) { runCatching { Account.setBio(bio ?: "") } }
                                job.busy = false
                                r.onSuccess { app.tell("Bio saved") }.onFailure { job.error = it.message }
                            }
                        }
                    }
                    Section("Your public playlists")
                    if (lists.isEmpty()) Message("Make a playlist public from its page (Share), and it shows up here and on your profile.")
                    for (l in lists) PlaylistRow(l.name, "${l.count} songs", l.cover) { app.go(Screen.PublicPage(l.id)) }
                    Spacer(Modifier.height(24.dp))
                }
            }
        }
    }
}

// ---------------------------------------------------------------------- other people's things

@Composable
fun ProfileScreen(s: Screen.ProfilePage) {
    val app = LocalApp.current
    val context = LocalContext.current
    var tries by remember { mutableStateOf(0) }
    val page by load(s.name, tries) { Account.profile(s.name) }
    Column(Modifier.fillMaxSize()) {
        TopBar("@${s.name}") { IconButton(Ic.SHARE, "Share profile") { shareText(context, Account.profileLink(s.name), "Share @${s.name}'s profile") } }
        Loading(page, { tries++ }) { (person, lists) ->
            LazyColumn {
                item {
                    Column(Modifier.fillMaxWidth().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Box(Modifier.size(96.dp).clip(CircleShape).background(P.accent), contentAlignment = Alignment.Center) {
                            Text(person.username.take(1).uppercase(), color = Color.White, fontSize = 40.sp, fontWeight = FontWeight.Bold)
                        }
                        Spacer(Modifier.height(10.dp))
                        Text("@${person.username}", color = P.text, fontSize = 24.sp, fontWeight = FontWeight.ExtraBold)
                        if (person.bio.isNotEmpty()) Text(person.bio, color = P.muted, fontSize = 15.sp, textAlign = TextAlign.Center, modifier = Modifier.padding(top = 6.dp))
                    }
                    Section("Public playlists")
                    if (lists.isEmpty()) Message("No public playlists yet.")
                }
                items(lists, key = { it.id }) { l -> PlaylistRow(l.name, "${l.count} songs", l.cover) { app.go(Screen.PublicPage(l.id)) } }
            }
        }
    }
}

@Composable
fun PublicScreen(s: Screen.PublicPage) {
    val app = LocalApp.current
    val context = LocalContext.current
    var tries by remember { mutableStateOf(0) }
    val page by load(s.id, tries) { Account.playlist(s.id) }
    Column(Modifier.fillMaxSize()) {
        TopBar("Shared playlist")
        Loading(page, { tries++ }) { p ->
            LazyColumn {
                item {
                    PageHeader(p.name, "By @${p.owner} · ${p.songs.size} songs", p.cover ?: p.songs.firstOrNull()?.cover, placeholder = Ic.PLAYLISTS)
                    Text(
                        "@${p.owner}", color = P.accentText, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth().clickable(role = Role.Button) { app.go(Screen.ProfilePage(p.owner)) }.padding(8.dp),
                    )
                    PlayButtons(p.songs)
                    Row(Modifier.padding(horizontal = 16.dp, vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Pill("Save to my playlists", Ic.PLUS, filled = false) {
                            val mine = Library.newPlaylist(p.name, p.songs)
                            app.tell("Saved as “${mine.name}”")
                        }
                        Pill("Share", filled = false) { shareText(context, Account.playlistLink(p.id), "Share “${p.name}”") }
                    }
                }
                itemsIndexed(p.songs, key = { i, t -> "$i-${t.id}" }) { i, t -> SongRow(t) { app.play(p.songs, i) } }
                item { Spacer(Modifier.height(24.dp)) }
            }
        }
    }
}

/** Share, make public and the share code, for one of your playlists. */
@Composable
fun ShareDialog(p: Playlist, close: () -> Unit) {
    val context = LocalContext.current
    val app = LocalApp.current
    val job = rememberJob()
    val scope = rememberCoroutineScope()
    fun run(work: () -> Unit, done: String) {
        job.busy = true
        job.error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching(work) }
            job.busy = false
            r.onSuccess { app.tell(done) }.onFailure { job.error = it.message }
        }
    }
    androidx.compose.material.AlertDialog(
        onDismissRequest = close,
        title = { Text("Share “${p.name}”", color = P.text) },
        text = {
            Column {
                Text("A share code works anywhere: paste it into Import on a phone or the PC app. A public link needs a Pulse account.", color = P.muted, fontSize = 14.sp)
                Spacer(Modifier.height(10.dp))
                ShareOption("Copy share code") { copyText(context, ShareCode.encode(p.name, Account.username.orEmpty(), p.songs)); app.tell("Share code copied") }
                if (p.publicId != null) {
                    ShareOption("Share link") { shareText(context, Account.playlistLink(p.publicId), "Share “${p.name}”") }
                    ShareOption("Update the public copy") { run({ Account.update(p.publicId, p.name, p.songs) }, "Public copy updated") }
                    ShareOption("Make private") { run({ Account.unpublish(p.publicId); Library.setPublicId(p.id, null) }, "Made private") }
                } else if (Account.username != null) {
                    ShareOption("Make public and share link") {
                        job.busy = true
                        job.error = null
                        scope.launch {
                            val r = withContext(Dispatchers.IO) { runCatching { Account.publish(p.name, p.songs) } }
                            job.busy = false
                            r.onSuccess { id -> Library.setPublicId(p.id, id); shareText(context, Account.playlistLink(id), "Share “${p.name}”") }
                                .onFailure { job.error = it.message }
                        }
                    }
                } else {
                    ShareOption("Sign in to make it public") { close(); app.go(Screen.AccountPage) }
                }
                JobStatus(job)
            }
        },
        confirmButton = { TextButton(onClick = close) { Text("Done", color = P.accentText) } },
        backgroundColor = P.surface,
    )
}

@Composable
private fun ShareOption(text: String, onClick: () -> Unit) {
    Text(
        text, color = P.text, fontSize = 16.sp, fontWeight = FontWeight.SemiBold,
        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).clip(RoundedCornerShape(8.dp)).clickable(role = Role.Button, onClick = onClick).padding(vertical = 13.dp),
    )
}

// ---------------------------------------------------------------------- import

@Composable
fun ImportScreen(s: Screen.ImportPage) {
    val app = LocalApp.current
    var text by remember { mutableStateOf(s.text) }
    var status by remember { mutableStateOf("") }
    var done by remember { mutableStateOf(0) }
    var total by remember { mutableStateOf(0) }
    var cancel by remember { mutableStateOf(false) }
    val job = rememberJob()
    val scope = rememberCoroutineScope()
    val kind = Importer.detect(text)
    Column(Modifier.fillMaxSize()) {
        TopBar("Import a playlist")
        LazyColumn {
            item {
                Text(
                    "Paste a link to a YouTube, YouTube Music, Spotify or SoundCloud playlist, or a Pulse share code or playlist link. " +
                        "To bring a playlist from the Pulse app on your PC: right-click it there, choose Copy share code, and paste it here.",
                    color = P.muted, fontSize = 15.sp, modifier = Modifier.padding(16.dp),
                )
                Field("Link or share code", text, { text = it }, single = false)
                Text(
                    if (text.isBlank()) "" else kind?.let { "Looks like: $it" } ?: "Pulse can't import that. Check the link.",
                    color = if (kind != null) P.accentText else P.muted, fontSize = 13.sp, modifier = Modifier.padding(horizontal = 16.dp),
                )
                if (job.busy) {
                    Text(if (total > 0) "$status $done of $total" else status, color = P.text, fontSize = 15.sp, modifier = Modifier.padding(16.dp))
                    if (total > 0) {
                        LinearProgressIndicator(progress = done.toFloat() / total, color = P.accent, backgroundColor = P.raised, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp))
                    } else {
                        LinearProgressIndicator(color = P.accent, backgroundColor = P.raised, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp))
                    }
                }
                job.error?.let { Text(it, color = Color(0xFFFF8A80), fontSize = 14.sp, modifier = Modifier.padding(16.dp)) }
                job.info?.let { Text(it, color = P.accentText, fontSize = 14.sp, modifier = Modifier.padding(16.dp)) }
                Row(Modifier.padding(16.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    if (job.busy) {
                        Pill("Cancel", filled = false) { cancel = true }
                    } else {
                        Pill("Import", Ic.PLUS) {
                            if (Account.readLink(text)?.first == "profile") {
                                app.openLink(text)
                                return@Pill
                            }
                            if (kind == null) {
                                job.error = "Paste a link to a YouTube, Spotify or SoundCloud playlist, or a Pulse share code."
                                return@Pill
                            }
                            job.busy = true
                            job.error = null
                            job.info = null
                            cancel = false
                            scope.launch {
                                val r = withContext(Dispatchers.IO) {
                                    runCatching {
                                        Importer.run(text, { st, d, t -> status = st; done = d; total = t }, { cancel })
                                    }
                                }
                                job.busy = false
                                r.onSuccess { imp ->
                                    if (imp.songs.isEmpty()) {
                                        job.error = "None of those songs could be found."
                                    } else {
                                        val p = Library.newPlaylist(imp.name, imp.songs)
                                        app.tell("Imported ${imp.songs.size} songs")
                                        job.info = imp.note
                                        app.go(Screen.PlaylistPage(p.id))
                                    }
                                }.onFailure { job.error = it.message ?: "The import didn't work." }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------- pick artists

/** "Pick artists you like": the first time Pulse opens. Picks become artists you follow. */
@Composable
fun PickArtistsScreen() {
    val app = LocalApp.current
    var genre by remember { mutableStateOf(0) }
    var text by remember { mutableStateOf("") }
    var query by remember { mutableStateOf("") }
    var tries by remember { mutableStateOf(0) }
    val picked = remember { mutableStateListOf<Artist>() }
    LaunchedEffect(text) {
        delay(450)
        query = text.trim()
    }
    val shown by load(genre, query, tries) { if (query.length >= 2) Deezer.searchArtists(query) else Deezer.genreArtists(genre) }
    fun finish(save: Boolean) {
        Library.pickArtists(if (save) picked.toList() else emptyList())
        if (save && picked.isNotEmpty()) app.tell("Following ${picked.size} artists. Your home page is built from them.")
        if (app.screen == Screen.PickArtists) app.back()
    }
    Column(Modifier.fillMaxSize()) {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp)) {
            Title("Pick artists you like")
            Text("Pick 3 or more. Your home page fills with music like theirs.", color = P.muted, fontSize = 15.sp, modifier = Modifier.padding(top = 4.dp))
        }
        Field("Search artists", text, { text = it })
        LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(vertical = 6.dp)) {
            item { Choice("Popular", genre == 0 && query.isEmpty()) { genre = 0; text = "" } }
            items(Deezer.GENRES) { (id, name) -> Choice(name, genre == id && query.isEmpty()) { genre = id; text = "" } }
        }
        Box(Modifier.weight(1f)) {
            Loading(shown, { tries++ }) { artists ->
                LazyVerticalGrid(columns = GridCells.Adaptive(104.dp), contentPadding = PaddingValues(12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    items(artists, key = { it.id }) { a ->
                        val on = picked.any { it.id == a.id }
                        Column(
                            Modifier.clip(RoundedCornerShape(12.dp)).clickable(role = Role.Checkbox) { if (on) picked.removeAll { it.id == a.id } else picked.add(a) }
                                .semantics(mergeDescendants = true) { selected = on }.padding(4.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            Box(contentAlignment = Alignment.BottomEnd) {
                                Cover(a.picture, Modifier.size(96.dp).then(if (on) Modifier.border(3.dp, P.accent, CircleShape) else Modifier), CircleShape, Ic.PERSON)
                                if (on) Box(Modifier.size(28.dp).clip(CircleShape).background(P.accent), contentAlignment = Alignment.Center) { Icon(Ic.CHECK, Color.White, 16.dp) }
                            }
                            Spacer(Modifier.height(6.dp))
                            Text(a.name, color = if (on) P.accentText else P.text, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center)
                        }
                    }
                }
            }
        }
        Row(Modifier.fillMaxWidth().background(P.surface).padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = { finish(false) }) { Text("Skip", color = P.muted, fontSize = 16.sp) }
            Spacer(Modifier.weight(1f))
            if (picked.size < 3) {
                Text("Pick ${3 - picked.size} more", color = P.muted, fontSize = 15.sp, modifier = Modifier.padding(end = 12.dp))
            } else {
                Pill("Continue (${picked.size})") { finish(true) }
            }
        }
    }
}

// ---------------------------------------------------------------------- For you (on Home)

/** Rows built from the artists you follow and play most, as on the desktop app. */
@Composable
fun ForYou() {
    val seeds = remember(Library.followed.size, Library.played.size) {
        val day = (System.currentTimeMillis() / 86_400_000L).toInt()
        val top = Library.topArtists().take(2).map { Artist(it.id, it.name, it.picture) }
        // A few of the artists you follow, a different few each day.
        val follows = Library.followed.toList().let { f -> if (f.isEmpty()) f else List(f.size) { f[(it + day) % f.size] } }
        (top + follows).distinctBy { it.id }.take(4)
    }
    seeds.forEachIndexed { i, a ->
        if (i % 2 == 0) AlsoListen(a) else SongsLike(a)
    }
}

@Composable
private fun AlsoListen(a: Artist) {
    val r by load(a.id, "also") { Deezer.related(a.id) }
    val v = r
    if (v is Loaded.Done && v.value.isNotEmpty()) {
        Section("Listeners of ${a.name} also listen to")
        ArtistRow(v.value)
    }
}

@Composable
private fun SongsLike(a: Artist) {
    val app = LocalApp.current
    val r by load(a.id, "like") { Deezer.songsLike(a.id) }
    val v = r
    if (v is Loaded.Done && v.value.isNotEmpty()) {
        Section("Songs like ${a.name}")
        LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            itemsIndexed(v.value.take(12), key = { _, s -> s.id }) { i, s -> Card(s.title, s.artist, s.cover) { app.play(v.value, i) } }
        }
    }
}

// ---------------------------------------------------------------------- lyrics

@Composable
fun LyricsScreen(s: Screen.LyricsPage) {
    Column(Modifier.fillMaxSize()) {
        TopBar("Lyrics")
        Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Cover(s.song.cover, Modifier.size(56.dp))
            Spacer(Modifier.width(12.dp))
            Column {
                Text(s.song.title, color = P.text, fontSize = 18.sp, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(s.song.artist, color = P.muted, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        LyricsView(s.song, Modifier.weight(1f))
    }
}

/**
 * A song's lyrics. While that song is playing, synced lyrics follow along: the line being
 * sung is lit and kept in view, and tapping a line jumps there.
 */
@Composable
fun LyricsView(song: Song, modifier: Modifier = Modifier) {
    val app = LocalApp.current
    var tries by remember { mutableStateOf(0) }
    val words by load(song.id, tries) { Lyrics.get(song) }
    Box(modifier.fillMaxWidth()) {
        Loading(words, { tries++ }) { w ->
            when {
                w == null -> Message("No lyrics for this song yet.")
                w.instrumental && w.synced.isEmpty() && w.plain == null -> Message("This one's an instrumental.")
                w.synced.isNotEmpty() -> {
                    val playingThis = app.player.song?.id == song.id
                    val line = if (playingThis) w.lineAt(app.player.position / 1000f + 0.25f) else -1
                    val list = rememberLazyListState()
                    LaunchedEffect(line) { if (line >= 0) list.animateScrollToItem((line - 2).coerceAtLeast(0)) }
                    LazyColumn(state = list, contentPadding = PaddingValues(vertical = 24.dp)) {
                        itemsIndexed(w.synced) { i, (at, text) ->
                            val now = i == line
                            Text(
                                text.ifEmpty { "♪" },
                                color = when { now -> P.text; i < line -> P.faint; else -> P.muted },
                                fontSize = if (now) 26.sp else 22.sp, fontWeight = FontWeight.ExtraBold,
                                modifier = Modifier.fillMaxWidth().clickable(enabled = playingThis, role = Role.Button) { app.seek((at * 1000).toLong()) }.padding(horizontal = 20.dp, vertical = 8.dp),
                            )
                        }
                        item { Text("Lyrics from LRCLIB", color = P.faint, fontSize = 12.sp, modifier = Modifier.padding(20.dp)) }
                    }
                }
                else -> LazyColumn(contentPadding = PaddingValues(20.dp)) {
                    item {
                        Text(w.plain.orEmpty(), color = P.text, fontSize = 18.sp, lineHeight = 28.sp)
                        Text("Lyrics from LRCLIB", color = P.faint, fontSize = 12.sp, modifier = Modifier.padding(top = 20.dp))
                    }
                }
            }
        }
    }
}

/** People whose usernames start with what you typed (for Search). */
@Composable
fun PeopleRow(query: String) {
    val app = LocalApp.current
    val r by load(query, "people") { Account.search(query) }
    val v = r
    if (v is Loaded.Done && v.value.isNotEmpty()) {
        Section("People")
        LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            items(v.value, key = { it.username }) { p ->
                Column(
                    Modifier.width(96.dp).clip(RoundedCornerShape(10.dp)).clickable(role = Role.Button) { app.go(Screen.ProfilePage(p.username)) }.semantics(mergeDescendants = true) {},
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Box(Modifier.size(80.dp).clip(CircleShape).background(P.accent), contentAlignment = Alignment.Center) {
                        Text(p.username.take(1).uppercase(), color = Color.White, fontSize = 30.sp, fontWeight = FontWeight.Bold)
                    }
                    Text("@${p.username}", color = P.text, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 6.dp))
                }
            }
        }
    }
}

@Composable
fun BusyDot() = CircularProgressIndicator(color = P.accent, strokeWidth = 2.dp, modifier = Modifier.size(20.dp))
