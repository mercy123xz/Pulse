package app.pulse.android

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.size
import androidx.compose.material.MaterialTheme
import androidx.compose.material.darkColors
import androidx.compose.material.lightColors
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Fill
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** The colors every screen draws with. */
@Immutable
data class Palette(
    val dark: Boolean,
    val bg: Color,
    val surface: Color,
    val raised: Color,
    val text: Color,
    val muted: Color,
    val faint: Color,
    val divider: Color,
    val accent: Color,
    val accentSoft: Color,
    /** Accent-colored text and icons on the background (readable in dark and light). */
    val accentText: Color,
    val player: Color,
)

val LocalPalette = staticCompositionLocalOf { palette(true, 0) }

/** The current colors. */
val P: Palette
    @Composable get() = LocalPalette.current

fun palette(dark: Boolean, accent: Int): Palette {
    val (_, strong, soft) = Themes.accents[accent.coerceIn(0, Themes.accents.size - 1)]
    val a = Color(strong)
    val s = Color(soft)
    return if (dark) {
        Palette(
            dark = true,
            bg = Color(0xFF0B0B0E), surface = Color(0xFF16161B), raised = Color(0xFF1D1D24),
            text = Color(0xFFEDEDF1), muted = Color(0xFFA3A3B0), faint = Color(0xFF6E6E7A), divider = Color(0xFF2C2C36),
            accent = a, accentSoft = s, accentText = s,
            player = lerp(Color(0xFF16161B), a, 0.28f),
        )
    } else {
        Palette(
            dark = false,
            bg = Color(0xFFF7F6FB), surface = Color.White, raised = Color(0xFFEDEBF4),
            text = Color(0xFF1B1A20), muted = Color(0xFF5E5C6B), faint = Color(0xFF8C8A98), divider = Color(0xFFDEDCE8),
            accent = a, accentSoft = s, accentText = a,
            player = lerp(Color.White, s, 0.30f),
        )
    }
}

private fun lerp(a: Color, b: Color, t: Float) = Color(
    red = a.red + (b.red - a.red) * t,
    green = a.green + (b.green - a.green) * t,
    blue = a.blue + (b.blue - a.blue) * t,
    alpha = 1f,
)

@Composable
fun PulseTheme(content: @Composable () -> Unit) {
    val dark = when (Library.theme) {
        Themes.DARK -> true
        Themes.LIGHT -> false
        else -> isSystemInDarkTheme()
    }
    val p = remember(dark, Library.accent) { palette(dark, Library.accent) }
    val colors = if (dark) {
        darkColors(primary = p.accent, secondary = p.accentSoft, background = p.bg, surface = p.surface, onBackground = p.text, onSurface = p.text)
    } else {
        lightColors(primary = p.accent, secondary = p.accent, background = p.bg, surface = p.surface, onBackground = p.text, onSurface = p.text)
    }
    val density = LocalDensity.current
    val scaled = if (Library.largeText) Density(density.density, density.fontScale * 1.25f) else density
    CompositionLocalProvider(LocalPalette provides p, LocalDensity provides scaled) {
        MaterialTheme(colors = colors, content = content)
    }
}

/** Icons, drawn from SVG paths on a 24×24 grid (lines, or filled when `fill`). */
object Ic {
    const val SEARCH = "M11 4a7 7 0 1 1 0 14a7 7 0 1 1 0-14z M20 20l-3.6-3.6"
    const val SETTINGS = "M12 9a3 3 0 1 1 0 6a3 3 0 1 1 0-6z M12 2v3 M12 19v3 M4.9 4.9l2.1 2.1 M17 17l2.1 2.1 M2 12h3 M19 12h3 M4.9 19.1l2.1-2.1 M17 7l2.1-2.1"
    const val HEART = "M12 20.5s-7.5-4.6-9.5-9.2C1.2 8.1 3.4 4.5 7 4.5c2 0 3.4 1.1 5 3 1.6-1.9 3-3 5-3 3.6 0 5.8 3.6 4.5 6.8-2 4.6-9.5 9.2-9.5 9.2z"
    const val PLAYLISTS = "M3 6h12 M3 12h12 M3 18h7 M16 14l6 3.5-6 3.5z"
    const val PLAY = "M8 4.6v14.8a1 1 0 0 0 1.5.86l11.6-7.4a1 1 0 0 0 0-1.72L9.5 3.74A1 1 0 0 0 8 4.6z"
    const val PAUSE = "M6.5 4h3.5v16H6.5z M14 4h3.5v16H14z"
    const val NEXT = "M5 5.2l10 6.8-10 6.8z M17 5h2.2v14H17z"
    const val PREV = "M19 5.2L9 12l10 6.8z M4.8 5H7v14H4.8z"
    const val SHUFFLE = "M16 3h5v5 M4 20L21 3 M21 16v5h-5 M15 15l6 6 M4 4l5 5"
    const val REPEAT = "M17 2l4 4-4 4 M3 11v-1a4 4 0 0 1 4-4h14 M7 22l-4-4 4-4 M21 13v1a4 4 0 0 1-4 4H3"
    const val REPEAT_ONE = "M17 2l4 4-4 4 M3 11v-1a4 4 0 0 1 4-4h14 M7 22l-4-4 4-4 M21 13v1a4 4 0 0 1-4 4H3 M11 10l1.5-1v6"
    const val BACK = "M19 12H5 M12 19l-7-7 7-7"
    const val DOWN = "M6 9l6 6 6-6"
    const val MORE = "M10.4 5a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0-3.2 0z M10.4 12a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0-3.2 0z M10.4 19a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0-3.2 0z"
    const val PLUS = "M12 5v14 M5 12h14"
    const val QUEUE = "M3 6h18 M3 12h18 M3 18h11 M18 15v6 M15 18h6"
    const val NOTE = "M9 18V5l12-2v13 M6 15a3 3 0 1 1 0 6a3 3 0 1 1 0-6z M18 13a3 3 0 1 1 0 6a3 3 0 1 1 0-6z"
    const val AUTOPLAY = "M12 12c-2-2.7-4-4-6-4a4 4 0 1 0 0 8c2 0 4-1.3 6-4z M12 12c2 2.7 4 4 6 4a4 4 0 0 0 0-8c-2 0-4 1.3-6 4z"
    const val ORBIT = "M12 9a3 3 0 1 1 0 6a3 3 0 1 1 0-6z M5.6 18.4A9 9 0 0 1 12 3 M18.4 5.6A9 9 0 0 1 12 21"
    const val CLOSE = "M6 6l12 12 M18 6L6 18"
    const val PERSON = "M12 4a4 4 0 1 1 0 8a4 4 0 1 1 0-8z M4 21a8 8 0 0 1 16 0"
    const val ALBUM = "M12 3a9 9 0 1 1 0 18a9 9 0 1 1 0-18z M12 10a2 2 0 1 1 0 4a2 2 0 1 1 0-4z"
    const val CHECK = "M5 12.5l4.5 4.5L19 7.5"
    const val SHARE = "M4 12v7a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-7 M16 6l-4-4-4 4 M12 2v13"
    const val LYRICS = "M4 6h11 M4 11h11 M4 16h7 M17 19V9l4-1 M17 19a2 2 0 1 1-4 0a2 2 0 1 1 4 0z"
    const val IMPORT = "M12 3v12 M7 10l5 5 5-5 M5 21h14"
}

private val parsed = HashMap<String, androidx.compose.ui.graphics.Path>()

private fun path(d: String) = parsed.getOrPut(d) { PathParser().parsePathString(d).toPath() }

/** One icon. `label` is what TalkBack reads (leave it null when the button says it). */
@Composable
fun Icon(d: String, tint: Color, size: Dp = 24.dp, fill: Boolean = false, label: String? = null) {
    val m = if (label != null) Modifier.semantics { contentDescription = label } else Modifier
    Canvas(m.size(size)) {
        val p = path(d)
        scale(this.size.width / 24f, pivot = Offset.Zero) {
            if (fill) drawPath(p, tint, style = Fill)
            else drawPath(p, tint, style = Stroke(width = 2f, cap = StrokeCap.Round, join = StrokeJoin.Round))
        }
    }
}

/** The Pulse mark: five bars on an accent square. */
@Composable
fun PulseLogo(size: Dp) {
    val a = P.accent
    Canvas(Modifier.size(size).semantics { contentDescription = "Pulse" }) {
        val u = this.size.width / 30f
        drawRoundRect(a, Offset.Zero, this.size, CornerRadius(8 * u))
        val bars = listOf(5.8f to (12f to 6f), 9.8f to (8.5f to 13f), 13.8f to (6f to 18f), 17.8f to (10f to 10f), 21.8f to (13f to 4f))
        for ((x, yh) in bars) drawRoundRect(Color.White, Offset(x * u, yh.first * u), Size(2.4f * u, yh.second * u), CornerRadius(1.2f * u))
    }
}
