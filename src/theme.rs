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
};

/// The palette in use. Swapped whole when the theme changes; each switch keeps the
/// old one alive (a few hundred bytes), so reads never need a lock.
static ACTIVE: AtomicPtr<Palette> = AtomicPtr::new(&DARK as *const Palette as *mut Palette);

/// The active palette.
pub fn pal() -> &'static Palette {
    // SAFETY: ACTIVE only ever holds pointers to `'static` palettes (constants or leaked boxes).
    unsafe { &*ACTIVE.load(Ordering::Relaxed) }
}

fn is_light(p: &Palette) -> bool {
    let c = p.bg;
    (c.r() as u32 + c.g() as u32 + c.b() as u32) > 384
}

pub const R_SM: CornerRadius = CornerRadius::same(6);
pub const R_MD: CornerRadius = CornerRadius::same(8);
pub const R_LG: CornerRadius = CornerRadius::same(12);

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

/// Picks the palette for light/dark mode and high contrast, and applies it.
pub fn apply(ctx: &egui::Context, light: bool, high_contrast: bool) {
    let p = match (light, high_contrast) {
        (false, false) => DARK,
        (true, false) => LIGHT,
        (false, true) => HIGH_CONTRAST,
        (true, true) => HIGH_CONTRAST_LIGHT,
    };
    set_palette(ctx, p);
}

/// Makes `p` the active palette and restyles egui's built-in widgets to match.
pub fn set_palette(ctx: &egui::Context, p: Palette) {
    ACTIVE.store(Box::into_raw(Box::new(p)), Ordering::Relaxed);
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
    v.window_corner_radius = R_LG;
    v.menu_corner_radius = R_MD;
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
        state.corner_radius = R_SM;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance of an sRGB color.
    fn luminance(c: Color32) -> f64 {
        let ch = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// Every text color must stay readable (WCAG AA, 4.5:1) on the backgrounds it is drawn on.
    #[test]
    fn text_colors_meet_wcag_aa() {
        for (name, p) in [("DARK", &DARK), ("LIGHT", &LIGHT), ("HIGH_CONTRAST", &HIGH_CONTRAST), ("HIGH_CONTRAST_LIGHT", &HIGH_CONTRAST_LIGHT)] {
            let mut checks = vec![];
            // Body text on every surface, including hovered / selected rows.
            for bg in [p.bg, p.sidebar, p.surface, p.surface_hover, p.surface_active] {
                checks.push(("text", p.text, bg));
                checks.push(("text_muted", p.text_muted, bg));
            }
            // Faint text is only used on plain backgrounds (incl. the search box).
            for bg in [p.bg, p.sidebar, p.surface] {
                checks.push(("text_faint", p.text_faint, bg));
            }
            checks.push(("white on accent", Color32::WHITE, p.accent));
            for bg in [p.bg, p.sidebar, p.surface, p.surface_hover] {
                checks.push(("accent text", p.accent_soft, bg));
            }
            for (what, fg, bg) in checks {
                let ratio = contrast(fg, bg);
                assert!(ratio >= 4.5, "{name}: {what} on {bg:?} is {ratio:.2}:1, needs 4.5:1");
            }
        }
    }
}
