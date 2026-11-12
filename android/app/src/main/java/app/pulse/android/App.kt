package app.pulse.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.media3.common.C
import androidx.media3.common.Player
import androidx.media3.session.MediaController

/** The pages you can be on. */
sealed interface Screen {
    data object Home : Screen
    data object Liked : Screen
    data object Playlists : Screen
    data object Search : Screen
    data object Settings : Screen
    data object TopSongs : Screen
    data class ArtistPage(val id: String, val name: String) : Screen
    data class AlbumPage(val id: String, val title: String) : Screen
    data class PlaylistPage(val id: String) : Screen
    data class OrbitPage(val artistId: String, val name: String, val picture: String?) : Screen
}

/** One song in the queue: where it is in the player's list, and whether it's playing. */
data class QueueEntry(val index: Int, val song: Song, val current: Boolean)

/** What the player is doing, for the screens to show. */
class PlayerState {
    var song by mutableStateOf<Song?>(null)
    var playing by mutableStateOf(false)
    var buffering by mutableStateOf(false)
    var position by mutableStateOf(0L)
    var duration by mutableStateOf(0L)
    var shuffle by mutableStateOf(false)
    var repeat by mutableStateOf(Player.REPEAT_MODE_OFF)
    var hasNext by mutableStateOf(false)
    var queue by mutableStateOf<List<QueueEntry>>(emptyList())

    fun update(p: Player) {
        song = p.currentMediaItem?.song()
        playing = p.isPlaying
        buffering = p.playbackState == Player.STATE_BUFFERING
        shuffle = p.shuffleModeEnabled
        repeat = p.repeatMode
        hasNext = p.hasNextMediaItem()
        position = p.currentPosition.coerceAtLeast(0)
        duration = p.duration.takeIf { it > 0 && it != C.TIME_UNSET } ?: ((song?.duration ?: 0) * 1000L)
        // The queue in the order it will play (shuffled or not), from the song playing now.
        val tl = p.currentTimeline
        val list = ArrayList<QueueEntry>()
        var i = p.currentMediaItemIndex
        while (!tl.isEmpty && i != C.INDEX_UNSET && i < p.mediaItemCount && list.size < 200) {
            p.getMediaItemAt(i).song()?.let { list.add(QueueEntry(i, it, list.isEmpty())) }
            i = tl.getNextWindowIndex(i, Player.REPEAT_MODE_OFF, p.shuffleModeEnabled)
        }
        queue = list
    }
}

/** The app's state: where you are, what's playing, and the things you can do. */
class App {
    val stack = mutableStateListOf<Screen>(Screen.Home)
    var playerOpen by mutableStateOf(false)
    var queueOpen by mutableStateOf(false)
    /** A short note at the bottom ("Added to Liked Songs"). */
    var note by mutableStateOf<String?>(null)
    /** Choosing a playlist to add this song to. */
    var addingToPlaylist by mutableStateOf<Song?>(null)
    val player = PlayerState()
    var controller: MediaController? = null

    val screen: Screen get() = stack.last()

    // ------------------------------------------------------------------ moving around

    fun go(s: Screen) {
        playerOpen = false
        queueOpen = false
        if (stack.last() != s) stack.add(s)
    }

    /** A bottom-bar tab: starts that tab fresh. */
    fun tab(s: Screen) {
        playerOpen = false
        queueOpen = false
        stack.clear()
        stack.add(s)
    }

    /** Back: closes the queue or player first, then the page. False when there's nothing to close. */
    fun back(): Boolean {
        when {
            queueOpen -> queueOpen = false
            playerOpen -> playerOpen = false
            stack.size > 1 -> stack.removeAt(stack.size - 1)
            else -> return false
        }
        return true
    }

    fun tell(text: String) {
        note = text
    }

    // ------------------------------------------------------------------ playing

    /** Plays `songs` from `index` (or shuffled), the rest queued after it. */
    fun play(songs: List<Song>, index: Int = 0, shuffle: Boolean = false) {
        val c = controller ?: return tell("The player is starting. Try again in a moment.")
        if (songs.isEmpty()) return
        // A queue of up to 500 songs, starting far enough back to include the one picked.
        val from = (index - 100).coerceAtLeast(0)
        val part = songs.subList(from, (from + 500).coerceAtMost(songs.size))
        val start = if (shuffle) part.indices.random() else index - from
        Library.autoplayIds.clear()
        c.shuffleModeEnabled = shuffle
        c.setMediaItems(part.map { it.item() }, start, 0)
        c.prepare()
        c.play()
    }

    fun toggle() {
        val c = controller ?: return
        if (c.isPlaying) c.pause() else {
            if (c.playbackState == Player.STATE_IDLE) c.prepare()
            c.play()
        }
    }

    fun next() = controller?.seekToNext()
    fun previous() = controller?.seekToPrevious()
    fun seek(ms: Long) = controller?.seekTo(ms)

    fun setShuffle(on: Boolean) {
        controller?.shuffleModeEnabled = on
    }

    fun cycleRepeat() {
        val c = controller ?: return
        c.repeatMode = when (c.repeatMode) {
            Player.REPEAT_MODE_OFF -> Player.REPEAT_MODE_ALL
            Player.REPEAT_MODE_ALL -> Player.REPEAT_MODE_ONE
            else -> Player.REPEAT_MODE_OFF
        }
    }

    fun playNext(song: Song) {
        val c = controller ?: return
        if (c.mediaItemCount == 0) return play(listOf(song))
        c.addMediaItem(c.currentMediaItemIndex + 1, song.item())
        tell("Plays next: ${song.title}")
    }

    fun addToQueue(song: Song) {
        val c = controller ?: return
        if (c.mediaItemCount == 0) return play(listOf(song))
        c.addMediaItem(song.item())
        tell("Added to the queue")
    }

    fun jumpTo(index: Int) {
        val c = controller ?: return
        c.seekToDefaultPosition(index)
        c.play()
    }

    fun removeFromQueue(index: Int) {
        controller?.removeMediaItem(index)
    }

    fun like(song: Song) {
        Library.toggleLike(song)
        tell(if (Library.isLiked(song)) "Added to Liked Songs" else "Removed from Liked Songs")
    }
}

val LocalApp = staticCompositionLocalOf<App> { error("No app") }
