//! Colors, fonts and the overall look of the app.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};


/// All UI colors. Every text color meets WCAG AA (4.5:1) on the backgrounds it is drawn on,
/// and white on `accent` is 5.7:1.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub bg: Color32,
    pub sidebar: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub surface_active: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    /// Accent used for text and icons (lighter in dark mode, darker in light mode).
    pub accent_soft: Color32,
    /// Background of the player bar.
    pub player: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const DARK: Palette = Palette {
    bg: rgb(11, 11, 14),
    sidebar: rgb(15, 15, 19),
    surface: rgb(22, 22, 27),
    surface_hover: rgb(29, 29, 36),
    surface_active: rgb(36, 36, 45),
    border: rgb(44, 44, 54),
    text: rgb(237, 237, 241),
    text_muted: rgb(150, 150, 164),
    text_faint: rgb(128, 128, 142),
    accent: rgb(124, 58, 237),
    accent_soft: rgb(167, 139, 250),
    player: rgb(15, 15, 19),
};

pub const LIGHT: Palette = Palette {
    bg: rgb(250, 250, 252),
    sidebar: rgb(243, 243, 247),
    surface: rgb(255, 255, 255),
    surface_hover: rgb(232, 232, 238),
    surface_active: rgb(222, 222, 230),
    border: rgb(212, 212, 222),
    text: rgb(20, 20, 26),
    text_muted: rgb(78, 78, 92),
    text_faint: rgb(100, 100, 114),
    accent: rgb(124, 58, 237),
    accent_soft: rgb(109, 40, 217),
    player: rgb(243, 243, 247),
};

/// Text drawn on the always-dark banners (home greeting, page headers).
pub const ON_DARK: Color32 = rgb(245, 245, 250);
pub const ON_DARK_MUTED: Color32 = rgb(205, 205, 218);

/// High-contrast version of the dark palette: black background, white text,
/// bright accent and visible borders.
pub const HIGH_CONTRAST: Palette = Palette {
    bg: rgb(0, 0, 0),
    sidebar: rgb(0, 0, 0),
    surface: rgb(10, 10, 10),
    surface_hover: rgb(38, 38, 38),
    surface_active: rgb(56, 56, 56),
    border: rgb(160, 160, 160),
    text: rgb(255, 255, 255),
    text_muted: rgb(220, 220, 220),
    text_faint: rgb(200, 200, 200),
    accent: rgb(110, 40, 230),
    accent_soft: rgb(255, 214, 10),
    player: rgb(0, 0, 0),
};

/// High-contrast version of the light palette.
pub const HIGH_CONTRAST_LIGHT: Palette = Palette {
    bg: rgb(255, 255, 255),
    sidebar: rgb(255, 255, 255),
    surface: rgb(255, 255, 255),
    surface_hover: rgb(225, 225, 225),
    surface_active: rgb(205, 205, 205),
    border: rgb(60, 60, 60),
    text: rgb(0, 0, 0),
    text_muted: rgb(30, 30, 30),
    text_faint: rgb(50, 50, 50),
    accent: rgb(90, 20, 200),
    accent_soft: rgb(75, 0, 170),
    player: rgb(255, 255, 255),
};

/// The look of one named part of the screen, from a theme's user.css.
/// `None` means "use Pulse's normal look".
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PartStyle {
    pub fill: Option<Color32>,
    pub border: Option<Stroke>,
    pub radius: Option<u8>,
    pub padding: Option<f32>,
    pub font_size: Option<f32>,
    pub bold: Option<bool>,
    pub uppercase: Option<bool>,
    pub color: Option<Color32>,
}

impl PartStyle {
    pub const NONE: PartStyle = PartStyle { fill: None, border: None, radius: None, padding: None, font_size: None, bold: None, uppercase: None, color: None };
}

/// The styleable parts. Each matches the function that draws it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Parts {
    pub sidebar: PartStyle,
    pub header: PartStyle,
    pub hero: PartStyle,
    pub player_bar: PartStyle,
    pub card: PartStyle,
    pub track_row: PartStyle,
    pub play_button: PartStyle,
}

impl Parts {
    pub const NONE: Parts = Parts {
        sidebar: PartStyle::NONE,
        header: PartStyle::NONE,
        hero: PartStyle::NONE,
        player_bar: PartStyle::NONE,
        card: PartStyle::NONE,
        track_row: PartStyle::NONE,
        play_button: PartStyle::NONE,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarStyle {
    #[default]
    Full,
    Icons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeroStyle {
    #[default]
    Banner,
    FullBleed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerStyle {
    #[default]
    Centered,
    ControlsLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NowPlayingArt {
    #[default]
    Bar,
    SidebarLarge,
}

/// Layout choices a theme makes in theme.toml.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayoutOpts {
    pub sidebar: SidebarStyle,
    pub hero: HeroStyle,
    pub player: PlayerStyle,
    pub now_playing_art: NowPlayingArt,
    /// Main area drawn as a rounded panel set in from the window edges.
    pub inset: bool,
    /// Soft glows in the colors of the playing song's cover behind the main area.
    pub backdrop: bool,
}

impl LayoutOpts {
    pub const DEFAULT: LayoutOpts = LayoutOpts {
        sidebar: SidebarStyle::Full,
        hero: HeroStyle::Banner,
        player: PlayerStyle::Centered,
        now_playing_art: NowPlayingArt::Bar,
        inset: false,
        backdrop: false,
    };
}

/// Everything the drawing code reads from the current theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Active {
    pub palette: Palette,
    pub parts: Parts,
    pub layout: LayoutOpts,
    /// Small, medium and large corner radius.
    pub radius: [u8; 3],
}

static DEFAULT_ACTIVE: Active = Active { palette: DARK, parts: Parts::NONE, layout: LayoutOpts::DEFAULT, radius: [6, 8, 12] };

/// The theme in use. Swapped whole when the theme changes; each switch keeps the
/// old one alive (about a kilobyte), so reads never need a lock.
static ACTIVE: AtomicPtr<Active> = AtomicPtr::new(&DEFAULT_ACTIVE as *const Active as *mut Active);

pub fn active() -> &'static Active {
    // SAFETY: ACTIVE only ever holds pointers to `'static` values (a static or leaked boxes).
    unsafe { &*ACTIVE.load(Ordering::Relaxed) }
}

/// The active palette.
pub fn pal() -> &'static Palette {
    &active().palette
}

/// The active theme's styled parts.
pub fn parts() -> &'static Parts {
    &active().parts
}

/// The active theme's layout choices.
pub fn layout() -> &'static LayoutOpts {
    &active().layout
}

pub fn r_sm() -> CornerRadius {
    CornerRadius::same(active().radius[0])
}
pub fn r_md() -> CornerRadius {
    CornerRadius::same(active().radius[1])
}
pub fn r_lg() -> CornerRadius {
    CornerRadius::same(active().radius[2])
}

pub fn is_light(p: &Palette) -> bool {
    let c = p.bg;
    (c.r() as u32 + c.g() as u32 + c.b() as u32) > 384
}


pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}
pub fn icon_fill() -> FontFamily {
    FontFamily::Name("phosphor-fill".into())
}

fn icon_only(filled: bool) -> FontFamily {
    FontFamily::Name(if filled { "icon-fill" } else { "icon-regular" }.into())
}

/// Where an icon's drawing sits inside its box, as a fraction of the font size (x right,
/// y down, from the box's center). Most Phosphor icons are centered; some aren't.
fn ink_offset(ch: char, filled: bool) -> egui::Vec2 {
    use skrifa::MetadataProvider;
    use skrifa::instance::{LocationRef, Size};
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<(char, bool), egui::Vec2>>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().unwrap().get(&(ch, filled)) {
        return *v;
    }
    let bytes = if filled { egui_phosphor::Variant::Fill.font_bytes() } else { egui_phosphor::Variant::Regular.font_bytes() };
    let measured = (|| {
        let font = skrifa::FontRef::new(bytes).ok()?;
        let gid = font.charmap().map(ch)?;
        let adv = font.glyph_metrics(Size::unscaled(), LocationRef::default()).advance_width(gid)?;
        // The glyph's stored box covers the whole icon grid, so measure the outline itself.
        let mut b = InkBounds::default();
        let settings = skrifa::outline::DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
        font.outline_glyphs().get(gid)?.draw(settings, &mut b).ok()?;
        if b.x_min > b.x_max {
            return None;
        }
        let m = font.metrics(Size::unscaled(), LocationRef::default());
        let em = (m.ascent - m.descent).max(1.0);
        let dx = ((b.x_min + b.x_max) / 2.0 - adv / 2.0) / em;
        // Font units go up from the baseline; the line box runs from ascent down to descent.
        let dy = (m.ascent - (b.y_min + b.y_max) / 2.0) / em - 0.5;
        Some(egui::vec2(dx, dy))
    })()
    .unwrap_or(egui::Vec2::ZERO);
    cache.lock().unwrap().insert((ch, filled), measured);
    measured
}

/// The box around an outline's points.
struct InkBounds {
    x_min: f32,
    y_min: f32,
    x_max: f32,
    y_max: f32,
}

impl Default for InkBounds {
    fn default() -> Self {
        Self { x_min: f32::MAX, y_min: f32::MAX, x_max: f32::MIN, y_max: f32::MIN }
    }
}

impl InkBounds {
    fn add(&mut self, x: f32, y: f32) {
        self.x_min = self.x_min.min(x);
        self.y_min = self.y_min.min(y);
        self.x_max = self.x_max.max(x);
        self.y_max = self.y_max.max(y);
    }
}

impl skrifa::outline::OutlinePen for InkBounds {
    fn move_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.add(cx, cy);
        self.add(x, y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.add(cx0, cy0);
        self.add(cx1, cy1);
        self.add(x, y);
    }
    fn close(&mut self) {}
}

/// Like `Painter::text` with `Align2::CENTER_CENTER`, but an icon's drawing (not its box)
/// is centered on `center`. Anything that isn't a single icon is drawn as normal text.
pub fn paint_icon(p: &egui::Painter, center: egui::Pos2, text: impl ToString, font: FontId, color: Color32) -> egui::Rect {
    let text = text.to_string();
    let mut chars = text.chars();
    let (Some(ch), None) = (chars.next(), chars.next()) else {
        return p.text(center, egui::Align2::CENTER_CENTER, text, font, color);
    };
    // Phosphor's icons live in the private-use area.
    if !('\u{E000}'..='\u{F8FF}').contains(&ch) {
        return p.text(center, egui::Align2::CENTER_CENTER, text, font, color);
    }
    let filled = font.family == icon_fill();
    let mut off = ink_offset(ch, filled);
    // The play triangle's weight is toward its flat side; nudge it right so it looks centered.
    if ch == egui_phosphor::regular::PLAY.chars().next().unwrap_or_default() {
        off.x -= 0.05;
    }
    let pos = egui::pos2(center.x - off.x * font.size, center.y - off.y * font.size);
    p.text(pos, egui::Align2::CENTER_CENTER, text, FontId::new(font.size, icon_only(filled)), color)
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn med(size: f32) -> FontId {
    FontId::new(size, medium())
}
pub fn semi(size: f32) -> FontId {
    FontId::new(size, semibold())
}
pub fn heavy(size: f32) -> FontId {
    FontId::new(size, bold())
}
pub fn filled_icon(size: f32) -> FontId {
    FontId::new(size, icon_fill())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut fonts, "Inter-Regular", include_bytes!("../assets/fonts/Inter-Regular.ttf"));
    add(&mut fonts, "Inter-Medium", include_bytes!("../assets/fonts/Inter-Medium.ttf"));
    add(&mut fonts, "Inter-SemiBold", include_bytes!("../assets/fonts/Inter-SemiBold.ttf"));
    add(&mut fonts, "Inter-Bold", include_bytes!("../assets/fonts/Inter-Bold.ttf"));

    // Inter first; egui's built-in fonts stay as fallback for emoji / other scripts.
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let family = |main: &str| {
        let mut v = vec![main.to_string()];
        v.extend(fallback.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, family("Inter-Regular"));
    fonts.families.insert(medium(), family("Inter-Medium"));
    fonts.families.insert(semibold(), family("Inter-SemiBold"));
    fonts.families.insert(bold(), family("Inter-Bold"));

    // Icons: outline icons mixed into normal text, filled icons as their own family.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    for fam in [medium(), semibold(), bold()] {
        if let Some(list) = fonts.families.get_mut(&fam) {
            list.insert(1, "phosphor".into());
        }
    }
    egui_phosphor::add_font_bytes_as_family(&mut fonts, "phosphor-fill", egui_phosphor::Variant::Fill.font_bytes());
    // Icon-only families: the line box is Phosphor's own, so icons line up the same in
    // every theme instead of sitting on the text font's baseline.
    fonts.families.insert(icon_only(false), vec!["phosphor".into()]);
    fonts.families.insert(icon_only(true), vec!["phosphor-fill".into()]);

    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.menu_margin = egui::Margin::same(6);
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.spacing.scroll.bar_width = 8.0;
        style.interaction.selectable_labels = false;
        style.text_styles.insert(egui::TextStyle::Body, regular(13.5));
        style.text_styles.insert(egui::TextStyle::Button, med(13.0));
        style.text_styles.insert(egui::TextStyle::Small, regular(11.5));
        style.text_styles.insert(egui::TextStyle::Heading, heavy(26.0));
    });
}

static REDUCE_MOTION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True when the "Reduce motion" setting is on: no moving backgrounds or fades.
pub fn reduce_motion() -> bool {
    REDUCE_MOTION.load(Ordering::Relaxed)
}

pub fn set_reduce_motion(on: bool) {
    REDUCE_MOTION.store(on, Ordering::Relaxed);
}

/// How long an animation of `secs` should run: not at all with Reduce motion, in Gaming
/// Mode or Ultra-low (then everything just appears, and nothing redraws for it).
pub fn motion(secs: f32) -> f32 {
    if reduce_motion() { 0.0 } else { secs }
}

/// 0..1 → 0..1: quick at first, settling gently at the end.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// 0 → 1 over `secs`, counted from the first frame that asks with this `id` (eased). The
/// app only redraws for it while it runs. Forget the id (`ctx.data_mut(|d| d.remove::<f64>(id))`)
/// to play it again.
pub fn appear(ctx: &egui::Context, id: egui::Id, secs: f32) -> f32 {
    let secs = motion(secs);
    if secs <= 0.0 {
        return 1.0;
    }
    let now = ctx.input(|i| i.time);
    let start = ctx.data_mut(|d| *d.get_temp_mut_or(id, now));
    let t = ((now - start) as f32 / secs).clamp(0.0, 1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    ease_out(t)
}

/// Makes `a` the active theme and restyles egui's built-in widgets to match.
pub fn set_active(ctx: &egui::Context, a: Active) {
    if *active() == a {
        return;
    }
    ACTIVE.store(Box::into_raw(Box::new(a)), Ordering::Relaxed);
    let p = pal();
    let light = is_light(p);
    let mut v = if light { Visuals::light() } else { Visuals::dark() };
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.surface;
    v.faint_bg_color = p.surface;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent_soft;
    v.selection.bg_fill = p.accent.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, p.accent_soft);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = r_lg();
    v.menu_corner_radius = r_md();
    let shadow_a = if light { 40 } else { 140 };
    v.popup_shadow = egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(shadow_a) };
    v.window_shadow = v.popup_shadow;
    v.text_cursor.stroke = Stroke::new(1.5, p.accent_soft);

    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text_muted);
    for (state, fill) in [
        (&mut w.inactive, p.surface),
        (&mut w.hovered, p.surface_hover),
        (&mut w.active, p.surface_active),
        (&mut w.open, p.surface_hover),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.corner_radius = r_sm();
        state.bg_stroke = Stroke::NONE;
        state.expansion = 0.0;
    }
    w.inactive.fg_stroke = Stroke::new(1.0, p.text_muted);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    // set_visuals only touches the current theme; set both so the OS theme can't override us.
    ctx.set_visuals_of(egui::Theme::Dark, v.clone());
    ctx.set_visuals_of(egui::Theme::Light, v);
}

/// WCAG relative luminance of an sRGB color.
fn luminance(c: Color32) -> f64 {
    let ch = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

/// WCAG contrast ratio between two colors (1 to 21).
pub fn contrast(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Text that would be hard to read with this palette (WCAG AA needs 4.5:1).
pub fn readability_problems(p: &Palette) -> Vec<String> {
    let mut checks = vec![];
    for (bg_name, bg) in [("background", p.bg), ("sidebar", p.sidebar), ("cards", p.surface), ("hovered rows", p.surface_hover), ("selected rows", p.surface_active)] {
        checks.push(("text", p.text, bg_name, bg));
        checks.push(("secondary text", p.text_muted, bg_name, bg));
    }
    for (bg_name, bg) in [("background", p.bg), ("sidebar", p.sidebar), ("cards", p.surface), ("player bar", p.player)] {
        checks.push(("faint text", p.text_faint, bg_name, bg));
        checks.push(("accent text", p.accent_soft, bg_name, bg));
    }
    checks.push(("text", p.text, "player bar", p.player));
    checks.push(("white text", Color32::WHITE, "accent", p.accent));
    checks
        .into_iter()
        .filter_map(|(what, fg, on, bg)| {
            let r = contrast(fg, bg);
            (r < 4.5).then(|| format!("{what} on {on} is {r:.1}:1 (needs 4.5:1)"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;



    /// Every text color must stay readable (WCAG AA, 4.5:1) on the backgrounds it is drawn on.
    #[test]
    fn text_colors_meet_wcag_aa() {
        for (name, p) in [("DARK", &DARK), ("LIGHT", &LIGHT), ("HIGH_CONTRAST", &HIGH_CONTRAST), ("HIGH_CONTRAST_LIGHT", &HIGH_CONTRAST_LIGHT)] {
            let problems = readability_problems(p);
            assert!(problems.is_empty(), "{name}: {problems:?}");
        }
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn icon_offsets_come_from_the_font() {
        let first = |s: &str| s.chars().next().unwrap();
        // The play triangle's drawing sits right of its box's center, by about a tenth.
        let play = ink_offset(first(egui_phosphor::regular::PLAY), true);
        assert!((play.x - 0.094).abs() < 0.01, "{play:?}");
        assert!(play.y.abs() < 0.02, "{play:?}");
        // Pause and repeat are centered.
        for g in [egui_phosphor::regular::PAUSE, egui_phosphor::regular::REPEAT] {
            let o = ink_offset(first(g), false);
            assert!(o.x.abs() < 0.01 && o.y.abs() < 0.02, "{o:?}");
        }
        // The heart sits a little low.
        assert!(ink_offset(first(egui_phosphor::regular::HEART), true).y > 0.02);
    }
}
