//! Theme files (Spicetify-style).
//!
//! Each theme is a folder in %APPDATA%\pulse-player\themes\ holding up to three files:
//! - `color.ini`: one or more color schemes. Spicetify's key names work
//!   (main, sidebar, card, text, subtext, button, highlight…).
//! - `user.css`: a small CSS subset for the named parts of the screen
//!   (.sidebar, .header, .hero, .player-bar, .card, .track-row, .play-button).
//! - `theme.toml`: the theme's name and layout choices.
//!
//! Pulse ships Default, Blossom, Dreary Mono, Lunar and Aurora inside the .exe, plus
//! Founders for early supporters and a secret one. A folder
//! with the same name overrides a built-in theme. Anything missing falls back to
//! Pulse's defaults, so a half-finished theme still works.

use crate::theme::{Active, DARK, HIGH_CONTRAST, HIGH_CONTRAST_LIGHT, LIGHT, LayoutOpts, Palette, PartStyle, Parts};
use crate::theme::{HeroStyle, NowPlayingArt, PlayerStyle, SidebarStyle};
use egui::{Color32, Stroke};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Themes built into Pulse: (folder name, color.ini, user.css, theme.toml).
const BUNDLED: [(&str, &str, &str, &str); 7] = [
    (
        "Default",
        include_str!("../assets/themes/Default/color.ini"),
        include_str!("../assets/themes/Default/user.css"),
        include_str!("../assets/themes/Default/theme.toml"),
    ),
    (
        "Blossom",
        include_str!("../assets/themes/Blossom/color.ini"),
        include_str!("../assets/themes/Blossom/user.css"),
        include_str!("../assets/themes/Blossom/theme.toml"),
    ),
    (
        "Dreary Mono",
        include_str!("../assets/themes/Dreary Mono/color.ini"),
        include_str!("../assets/themes/Dreary Mono/user.css"),
        include_str!("../assets/themes/Dreary Mono/theme.toml"),
    ),
    (
        "Lunar",
        include_str!("../assets/themes/Lunar/color.ini"),
        include_str!("../assets/themes/Lunar/user.css"),
        include_str!("../assets/themes/Lunar/theme.toml"),
    ),
    (
        "Aurora",
        include_str!("../assets/themes/Aurora/color.ini"),
        include_str!("../assets/themes/Aurora/user.css"),
        include_str!("../assets/themes/Aurora/theme.toml"),
    ),
    (
        SUPPORTERS_THEME,
        include_str!("../assets/themes/Founders/color.ini"),
        include_str!("../assets/themes/Founders/user.css"),
        include_str!("../assets/themes/Founders/theme.toml"),
    ),
    (
        SECRET_THEME,
        include_str!("../assets/themes/Tung Tung/color.ini"),
        include_str!("../assets/themes/Tung Tung/user.css"),
        include_str!("../assets/themes/Tung Tung/theme.toml"),
    ),
];

/// The secret built-in theme: it shows up once someone searches for it (see
/// `App::navigate`).
pub const SECRET_THEME: &str = "Tung Tung";

/// Does this search find the secret theme? ("tung tung tung sahur", any spacing.)
pub fn finds_secret(query: &str) -> bool {
    crate::youtube::norm(query).contains("tungtungtung")
}

/// The built-in theme only Pulse's early supporters see (a thank-you; see `account`).
pub const SUPPORTERS_THEME: &str = "Founders";

/// Whether this theme shows in the picker: the supporters' theme only for supporters,
/// the secret one once found (and either for anyone already using it).
pub fn offered(t: &Theme, supporter: bool, found_secret: bool, current: &str) -> bool {
    let locked = t.builtin && ((t.name == SUPPORTERS_THEME && !supporter) || (t.name == SECRET_THEME && !found_secret));
    !locked || t.name == current
}

/// One color scheme of a theme.
#[derive(Debug, Clone)]
pub struct Scheme {
    pub name: String,
    pub palette: Palette,
    /// The scheme's keys as written, for `var(--spice-…)` in user.css.
    vars: HashMap<String, Color32>,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub author: String,
    pub description: String,
    pub schemes: Vec<Scheme>,
    pub layout: LayoutOpts,
    css: Css,
    /// The theme's folder (None for built-in themes not copied to disk).
    pub folder: Option<PathBuf>,
    pub builtin: bool,
    /// Things Pulse skipped or that may be hard to read.
    pub warnings: Vec<String>,
}

pub fn themes_dir() -> PathBuf {
    crate::settings::data_dir().join("themes")
}

/// All themes: the built-in ones, then those in the themes folder (by name).
pub fn list() -> Vec<Theme> {
    let mut out: Vec<Theme> = BUNDLED.iter().map(|(name, ini, css, toml)| parse(name, ini, css, toml, None, true)).collect();
    let mut user = vec![];
    if let Ok(entries) = std::fs::read_dir(themes_dir()) {
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                user.push(load_folder(&path));
            }
        }
    }
    user.sort_by_key(|t| t.name.to_lowercase());
    for t in user {
        match out.iter_mut().find(|b| b.name.eq_ignore_ascii_case(&t.name)) {
            Some(slot) => *slot = t,
            None => out.push(t),
        }
    }
    out
}

/// Reads a theme folder. Missing files are fine.
pub fn load_folder(dir: &Path) -> Theme {
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap_or_default();
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("Theme").to_string();
    parse(&name, &read("color.ini"), &read("user.css"), &read("theme.toml"), Some(dir.to_path_buf()), false)
}

/// Builds a theme from its three files' contents. Never fails: problems become warnings.
pub fn parse(folder_name: &str, color_ini: &str, user_css: &str, theme_toml: &str, folder: Option<PathBuf>, builtin: bool) -> Theme {
    let mut warnings = vec![];
    let meta = parse_toml(theme_toml, &mut warnings);
    let mut schemes: Vec<Scheme> = parse_ini(color_ini, &mut warnings)
        .into_iter()
        .map(|(name, vars)| Scheme { palette: palette_from(&vars), name, vars })
        .collect();
    if schemes.is_empty() {
        schemes.push(Scheme { name: "Dark".into(), palette: DARK, vars: HashMap::new() });
    }
    for s in &schemes {
        for p in crate::theme::readability_problems(&s.palette) {
            warnings.push(format!("{}: {p}", s.name));
        }
    }
    let css = parse_css(user_css, &mut warnings);
    Theme {
        name: meta.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| folder_name.to_string()),
        author: meta.author.unwrap_or_default(),
        description: meta.description.unwrap_or_default(),
        schemes,
        layout: meta.layout,
        css,
        folder,
        builtin,
        warnings,
    }
}

impl Theme {
    pub fn scheme_index(&self, name: &str) -> usize {
        self.schemes.iter().position(|s| s.name.eq_ignore_ascii_case(name)).unwrap_or(0)
    }

    /// What the drawing code uses, for one scheme. High contrast keeps the theme's
    /// layout but uses Pulse's high-contrast colors and no custom part colors.
    pub fn active(&self, scheme: usize, high_contrast: bool) -> Active {
        let s = &self.schemes[scheme.min(self.schemes.len() - 1)];
        if high_contrast {
            let light = is_light(&s.palette);
            return Active {
                palette: if light { HIGH_CONTRAST_LIGHT } else { HIGH_CONTRAST },
                parts: Parts::NONE,
                layout: self.layout,
                radius: self.css.radius(),
            };
        }
        Active { palette: s.palette.clone(), parts: self.css.resolve(&s.vars, &s.palette), layout: self.layout, radius: self.css.radius() }
    }
}

pub fn is_light(p: &Palette) -> bool {
    let c = p.bg;
    (c.r() as u32 + c.g() as u32 + c.b() as u32) > 384
}

// ---------------------------------------------------------------------------
// theme.toml
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct TomlTheme {
    name: Option<String>,
    author: Option<String>,
    description: Option<String>,
    layout: TomlLayout,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TomlLayout {
    sidebar: Option<String>,
    hero: Option<String>,
    player: Option<String>,
    now_playing_art: Option<String>,
    inset: Option<bool>,
    backdrop: Option<bool>,
}

struct Meta {
    name: Option<String>,
    author: Option<String>,
    description: Option<String>,
    layout: LayoutOpts,
}

fn parse_toml(text: &str, warnings: &mut Vec<String>) -> Meta {
    let t: TomlTheme = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => {
            warnings.push(format!("theme.toml: {}", e.message()));
            TomlTheme::default()
        }
    };
    let mut layout = LayoutOpts::DEFAULT;
    let mut pick = |key: &str, value: Option<String>, options: &[&str]| -> Option<usize> {
        let v = value?.trim().to_lowercase();
        let i = options.iter().position(|o| *o == v);
        if i.is_none() {
            warnings.push(format!("theme.toml: {key} = \"{v}\" isn't an option ({})", options.join(", ")));
        }
        i
    };
    if let Some(i) = pick("sidebar", t.layout.sidebar, &["full", "icons"]) {
        layout.sidebar = [SidebarStyle::Full, SidebarStyle::Icons][i];
    }
    if let Some(i) = pick("hero", t.layout.hero, &["banner", "fullbleed"]) {
        layout.hero = [HeroStyle::Banner, HeroStyle::FullBleed][i];
    }
    if let Some(i) = pick("player", t.layout.player, &["centered", "controls-left"]) {
        layout.player = [PlayerStyle::Centered, PlayerStyle::ControlsLeft][i];
    }
    if let Some(i) = pick("now_playing_art", t.layout.now_playing_art, &["bar", "sidebar-large"]) {
        layout.now_playing_art = [NowPlayingArt::Bar, NowPlayingArt::SidebarLarge][i];
    }
    if let Some(b) = t.layout.inset {
        layout.inset = b;
    }
    if let Some(b) = t.layout.backdrop {
        layout.backdrop = b;
    }
    Meta { name: t.name, author: t.author, description: t.description, layout }
}

// ---------------------------------------------------------------------------
// color.ini
// ---------------------------------------------------------------------------

/// Reads the schemes: `[Name]` sections of `key = color` lines. `;` and `#` start comments
/// (a `#` right before hex digits is part of the color).
fn parse_ini(text: &str, warnings: &mut Vec<String>) -> Vec<(String, HashMap<String, Color32>)> {
    let mut out: Vec<(String, HashMap<String, Color32>)> = vec![];
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('#') && !line.contains('=') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name.trim().to_string(), HashMap::new()));
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            warnings.push(format!("color.ini line {}: expected key = color", n + 1));
            continue;
        };
        let key = k.trim().to_lowercase();
        // Spicetify allows trailing comments after the value.
        let value = v.split_whitespace().next().unwrap_or("");
        match parse_color(value) {
            Some(c) => {
                if out.is_empty() {
                    out.push(("Base".into(), HashMap::new()));
                }
                out.last_mut().unwrap().1.insert(key, c);
            }
            None => warnings.push(format!("color.ini line {}: \"{value}\" isn't a color", n + 1)),
        }
    }
    out.retain(|(_, vars)| !vars.is_empty());
    out
}

/// Mixes `a` toward `b` by `t` (0 = a, 1 = b).
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Turns a scheme's keys into Pulse's palette. Spicetify and Pulse key names both work.
/// Missing keys are worked out from the ones that are there; with no background or
/// text at all, Pulse's default colors are used.
fn palette_from(vars: &HashMap<String, Color32>) -> Palette {
    let get = |names: &[&str]| names.iter().find_map(|n| vars.get(*n).copied());
    let main = get(&["main", "bg", "background"]);
    let text = get(&["text"]);
    let base = match main {
        Some(m) if (m.r() as u32 + m.g() as u32 + m.b() as u32) > 384 => LIGHT,
        _ => DARK,
    };
    let (main, text) = match (main, text) {
        (Some(m), Some(t)) => (m, t),
        (Some(m), None) => (m, base.text),
        (None, Some(t)) => (base.bg, t),
        (None, None) => (base.bg, base.text),
    };
    let sidebar = get(&["sidebar"]).unwrap_or(main);
    let muted = get(&["subtext", "text-muted"]).unwrap_or_else(|| mix(text, main, 0.35));
    let accent = get(&["button", "accent"]).unwrap_or(base.accent);
    Palette {
        bg: main,
        sidebar,
        surface: get(&["card", "surface"]).unwrap_or_else(|| mix(main, text, 0.05)),
        surface_hover: get(&["highlight", "surface-hover"]).unwrap_or_else(|| mix(main, text, 0.09)),
        surface_active: get(&["highlight-elevated", "selected-row", "surface-active"]).unwrap_or_else(|| mix(main, text, 0.14)),
        border: get(&["border", "misc"]).unwrap_or_else(|| mix(main, text, 0.18)),
        text,
        text_muted: muted,
        text_faint: get(&["text-faint"]).unwrap_or(muted),
        accent,
        accent_soft: get(&["button-active", "accent-text"]).unwrap_or(accent),
        player: get(&["player"]).unwrap_or(sidebar),
    }
}

/// Colors as written in theme files: `fff`, `#ffffff`, `#ffffff80`, `rgb(1, 2, 3)`,
/// `rgba(1, 2, 3, 0.5)`, `white`, `black`, `transparent`.
pub fn parse_color(s: &str) -> Option<Color32> {
    let s = s.trim().trim_end_matches(';').trim();
    let lower = s.to_lowercase();
    match lower.as_str() {
        "white" => return Some(Color32::WHITE),
        "black" => return Some(Color32::BLACK),
        "transparent" => return Some(Color32::TRANSPARENT),
        _ => {}
    }
    if let Some(inner) = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb(")).and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split([',', ' ', '/']).filter(|p| !p.is_empty()).collect();
        if parts.len() < 3 {
            return None;
        }
        let ch = |p: &str| p.trim().parse::<f32>().ok().map(|v| v.clamp(0.0, 255.0) as u8);
        let a = match parts.get(3) {
            Some(p) if p.ends_with('%') => (p.trim_end_matches('%').parse::<f32>().ok()? / 100.0 * 255.0) as u8,
            Some(p) => (p.parse::<f32>().ok()?.clamp(0.0, 1.0) * 255.0) as u8,
            None => 255,
        };
        return Some(Color32::from_rgba_unmultiplied(ch(parts[0])?, ch(parts[1])?, ch(parts[2])?, a));
    }
    let hex = lower.trim_start_matches('#');
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let nib = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v * 17);
    match hex.len() {
        3 => Some(Color32::from_rgb(nib(0)?, nib(1)?, nib(2)?)),
        4 => Some(Color32::from_rgba_unmultiplied(nib(0)?, nib(1)?, nib(2)?, nib(3)?)),
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// user.css
// ---------------------------------------------------------------------------

/// A color in user.css: written out, or `var(--spice-name)` from the scheme.
#[derive(Debug, Clone, PartialEq)]
enum CssColor {
    Literal(Color32),
    Var(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
struct RawPart {
    fill: Option<CssColor>,
    border_color: Option<CssColor>,
    border_width: Option<f32>,
    radius: Option<f32>,
    padding: Option<f32>,
    font_size: Option<f32>,
    bold: Option<bool>,
    uppercase: Option<bool>,
    color: Option<CssColor>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Css {
    parts: HashMap<&'static str, RawPart>,
    radius: [Option<f32>; 3],
}

pub const PART_NAMES: [&str; 7] = ["sidebar", "header", "hero", "player-bar", "card", "track-row", "play-button"];

impl Css {
    fn radius(&self) -> [u8; 3] {
        let d = [6.0, 8.0, 12.0];
        std::array::from_fn(|i| self.radius[i].unwrap_or(d[i]).clamp(0.0, 40.0) as u8)
    }

    fn resolve(&self, vars: &HashMap<String, Color32>, p: &Palette) -> Parts {
        let color = |c: &Option<CssColor>| -> Option<Color32> {
            match c.as_ref()? {
                CssColor::Literal(c) => Some(*c),
                CssColor::Var(name) => vars.get(name).copied().or_else(|| palette_var(p, name)),
            }
        };
        let part = |name: &str| -> PartStyle {
            let Some(r) = self.parts.get(name) else { return PartStyle::NONE };
            let border = match (r.border_width, color(&r.border_color)) {
                (Some(w), c) if w > 0.0 => Some(Stroke::new(w, c.unwrap_or(p.border))),
                (None, Some(c)) => Some(Stroke::new(1.0, c)),
                _ => None,
            };
            PartStyle {
                fill: color(&r.fill),
                border,
                radius: r.radius.map(|v| v.clamp(0.0, 120.0) as u8),
                padding: r.padding.map(|v| v.clamp(0.0, 64.0)),
                font_size: r.font_size.map(|v| v.clamp(8.0, 96.0)),
                bold: r.bold,
                uppercase: r.uppercase,
                color: color(&r.color),
            }
        };
        Parts {
            sidebar: part("sidebar"),
            header: part("header"),
            hero: part("hero"),
            player_bar: part("player-bar"),
            card: part("card"),
            track_row: part("track-row"),
            play_button: part("play-button"),
        }
    }
}

/// Pulse's own palette names, also usable as `var(--accent)` etc.
fn palette_var(p: &Palette, name: &str) -> Option<Color32> {
    Some(match name {
        "main" | "bg" => p.bg,
        "sidebar" => p.sidebar,
        "player" => p.player,
        "card" | "surface" => p.surface,
        "highlight" | "surface-hover" => p.surface_hover,
        "highlight-elevated" | "surface-active" => p.surface_active,
        "border" | "misc" => p.border,
        "text" => p.text,
        "subtext" | "text-muted" => p.text_muted,
        "text-faint" => p.text_faint,
        "button" | "accent" => p.accent,
        "button-active" | "accent-text" => p.accent_soft,
        _ => return None,
    })
}

/// Reads the supported subset of CSS. Unknown selectors and properties are skipped
/// with a warning, the way browsers ignore what they don't understand.
fn parse_css(text: &str, warnings: &mut Vec<String>) -> Css {
    let mut css = Css::default();
    // Remove /* comments */.
    let mut clean = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("/*") {
        clean.push_str(&rest[..i]);
        rest = match rest[i + 2..].find("*/") {
            Some(j) => &rest[i + 2 + j + 2..],
            None => "",
        };
    }
    clean.push_str(rest);

    let mut rest = clean.as_str();
    while let Some(open) = rest.find('{') {
        let selectors = rest[..open].trim().to_string();
        let Some(close) = rest[open..].find('}') else {
            warnings.push(format!("user.css: missing }} after \"{selectors}\""));
            break;
        };
        let body = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];

        for sel in selectors.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if sel == ":root" {
                for (prop, value) in declarations(body) {
                    let i = match prop.as_str() {
                        "--radius-sm" => 0,
                        "--radius-md" => 1,
                        "--radius-lg" => 2,
                        p if p.starts_with("--") => continue, // other variables are fine
                        p => {
                            warnings.push(format!("user.css: :root only takes --radius-sm/md/lg (skipped {p})"));
                            continue;
                        }
                    };
                    css.radius[i] = length(&value);
                }
                continue;
            }
            let Some(name) = sel.strip_prefix('.').and_then(|n| PART_NAMES.iter().find(|p| **p == n)) else {
                warnings.push(format!("user.css: \"{sel}\" isn't a Pulse part ({})", PART_NAMES.map(|p| format!(".{p}")).join(" ")));
                continue;
            };
            let part = css.parts.entry(name).or_default();
            for (prop, value) in declarations(body) {
                apply_property(part, &prop, &value, warnings);
            }
        }
    }
    css
}

fn declarations(body: &str) -> Vec<(String, String)> {
    body.split(';')
        .filter_map(|d| d.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().trim_end_matches("!important").trim().to_string()))
        .filter(|(k, v)| !k.is_empty() && !v.is_empty())
        .collect()
}

fn length(v: &str) -> Option<f32> {
    let v = v.trim().to_lowercase();
    let n = v.trim_end_matches("px").trim_end_matches("pt").trim();
    n.parse::<f32>().ok().filter(|x| x.is_finite() && *x >= 0.0)
}

fn css_color(v: &str) -> Option<CssColor> {
    let v = v.trim();
    if let Some(inner) = v.strip_prefix("var(").and_then(|r| r.strip_suffix(')')) {
        let name = inner.split(',').next().unwrap_or("").trim().trim_start_matches("--");
        let name = name.strip_prefix("spice-").unwrap_or(name);
        return Some(CssColor::Var(name.to_lowercase()));
    }
    parse_color(v).map(CssColor::Literal)
}

fn apply_property(part: &mut RawPart, prop: &str, value: &str, warnings: &mut Vec<String>) {
    let bad = |warnings: &mut Vec<String>| warnings.push(format!("user.css: can't read \"{prop}: {value}\""));
    match prop {
        "background" | "background-color" => match css_color(value) {
            Some(c) => part.fill = Some(c),
            None => bad(warnings),
        },
        "color" => match css_color(value) {
            Some(c) => part.color = Some(c),
            None => bad(warnings),
        },
        "border-color" => match css_color(value) {
            Some(c) => part.border_color = Some(c),
            None => bad(warnings),
        },
        "border-width" => part.border_width = length(value),
        // `border: 1px solid #333` (also -top/-right/…: each part draws its border where it fits).
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            if value.trim() == "none" || value.trim() == "0" {
                part.border_width = Some(0.0);
                return;
            }
            for word in split_css_words(value) {
                if let Some(w) = length(&word) {
                    part.border_width = Some(w);
                } else if let Some(c) = css_color(&word) {
                    part.border_color = Some(c);
                }
            }
            part.border_width.get_or_insert(1.0);
        }
        "border-radius" => match length(value.split_whitespace().next().unwrap_or("")) {
            Some(r) => part.radius = Some(r),
            None => bad(warnings),
        },
        "padding" => match length(value.split_whitespace().next().unwrap_or("")) {
            Some(p) => part.padding = Some(p),
            None => bad(warnings),
        },
        "font-size" => match length(value) {
            Some(s) => part.font_size = Some(s),
            None => bad(warnings),
        },
        "font-weight" => {
            let v = value.trim().to_lowercase();
            part.bold = Some(v == "bold" || v == "bolder" || v.parse::<u32>().is_ok_and(|n| n >= 600));
        }
        "text-transform" => part.uppercase = Some(value.trim().eq_ignore_ascii_case("uppercase")),
        other => warnings.push(format!("user.css: Pulse doesn't support \"{other}\" (skipped)")),
    }
}

/// Splits `1px solid rgb(1, 2, 3)` into words, keeping `rgb(…)` / `var(…)` together.
fn split_css_words(v: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut depth = 0;
    for c in v.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ---------------------------------------------------------------------------
// Live reload
// ---------------------------------------------------------------------------

/// Watches the themes folder; the receiver gets a message whenever a file changes,
/// so theme makers see their edits right away.
pub fn watch(ctx: &egui::Context) -> Option<(notify::RecommendedWatcher, std::sync::mpsc::Receiver<()>)> {
    use notify::Watcher;
    let dir = themes_dir();
    std::fs::create_dir_all(&dir).ok()?;
    write_readme(&dir);
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok_and(|e| !e.kind.is_access()) {
            let _ = tx.send(());
            ctx.request_repaint();
        }
    })
    .ok()?;
    w.watch(&dir, notify::RecursiveMode::Recursive).ok()?;
    Some((w, rx))
}

/// A short guide for theme makers, next to their themes.
fn write_readme(dir: &Path) {
    let file = dir.join("README.txt");
    if file.exists() {
        return;
    }
    let _ = std::fs::write(file, README);
}

/// Copies a built-in theme into the themes folder so it can be edited.
pub fn copy_builtin(name: &str) -> Result<PathBuf, String> {
    let (_, ini, css, toml) = BUNDLED.iter().find(|b| b.0 == name).ok_or("Not a built-in theme")?;
    let dir = themes_dir().join(format!("{name} (my copy)"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let toml = toml.replacen(&format!("name = \"{name}\""), &format!("name = \"{name} (my copy)\""), 1);
    for (file, text) in [("color.ini", *ini), ("user.css", *css), ("theme.toml", toml.as_str())] {
        std::fs::write(dir.join(file), text).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

const README: &str = r#"Pulse themes
============

Each folder here is a theme. A theme folder holds up to three files; any of them can be left out.

color.ini - color schemes
  [Dark]
  main     = 121212     ; background
  sidebar  = 000000
  player   = 000000     ; player bar
  card     = 181818
  highlight = 282828    ; hovered rows
  highlight-elevated = 333333   ; selected rows
  border   = 333333
  text     = ffffff
  subtext  = b3b3b3
  text-faint = 999999
  button   = 1db954     ; accent (buttons, sliders)
  button-active = 1ed760 ; accent used for text and icons
  These are Spicetify's key names, so Spicetify color.ini files work as they are.
  Missing keys are worked out from the others.

user.css - shapes and spacing for named parts
  Parts: .sidebar .header .hero .player-bar .card .track-row .play-button
  Properties: background, color, border, border-color, border-width, border-radius,
              padding, font-size, font-weight, text-transform: uppercase
  Colors: #rrggbb, rgb(), rgba(), or var(--spice-KEY) from color.ini.
  Corner sizes for everything: :root { --radius-sm: 6px; --radius-md: 8px; --radius-lg: 12px; }
  Anything else is skipped (Settings > Themes lists what was skipped).

theme.toml - name and layout
  name = "My Theme"
  author = "Me"
  description = "What it looks like"
  [layout]
  sidebar = "full"            # or "icons"
  hero = "banner"             # or "fullbleed" (full-width artist photo)
  player = "centered"         # or "controls-left"
  now_playing_art = "bar"     # or "sidebar-large"
  inset = false               # true: main area as a rounded panel
  backdrop = false            # true: soft glows in the playing song's colors

Pulse reloads a theme as soon as you save one of its files.
Tip: in Settings > Themes, "Make a copy" puts a built-in theme here to start from.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_scheme_is_readable() {
        for t in list().into_iter().filter(|t| t.builtin) {
            assert!(t.warnings.is_empty(), "{}: {:?}", t.name, t.warnings);
            assert!(t.schemes.len() >= 2, "{} should have a dark and a light scheme", t.name);
        }
    }

    #[test]
    fn bundled_themes_load_their_layouts() {
        let mut w = vec![];
        assert_eq!(parse_toml("[layout]\nplayer = \"controls-left\"", &mut w).layout.player, PlayerStyle::ControlsLeft);
        let all = list();
        let get = |n: &str| all.iter().find(|t| t.name == n).unwrap();
        assert_eq!(get("Lunar").layout.sidebar, SidebarStyle::Icons);
        assert_eq!(get("Lunar").layout.hero, HeroStyle::FullBleed);
        assert!(get("Lunar").layout.inset);
        assert!(get("Aurora").layout.backdrop && get("Default").layout.backdrop);
        assert!(!get("Dreary Mono").layout.backdrop);
        let founders = get(SUPPORTERS_THEME);
        assert!(founders.layout.inset && founders.builtin);
        assert!(!offered(founders, false, true, "Default") && offered(founders, true, false, "Default") && offered(founders, false, false, "Founders"));
        let tung = get(SECRET_THEME);
        assert!(!offered(tung, true, false, "Default") && offered(tung, false, true, "Default"));
        assert!(finds_secret("Tung Tung Tung Sahur") && finds_secret("tungtungtung") && !finds_secret("tung"));
        assert!(offered(get("Lunar"), false, false, "Default"));
        assert_eq!(get("Blossom").layout.now_playing_art, NowPlayingArt::SidebarLarge);
        assert_eq!(get("Dreary Mono").layout.player, PlayerStyle::Centered);
        let mono = get("Dreary Mono").active(0, false);
        assert_eq!(mono.radius, [2, 2, 4]);
        assert_eq!(mono.parts.hero.uppercase, Some(true));
        assert_eq!(mono.parts.play_button.fill, Some(mono.palette.text));
    }

    #[test]
    fn reads_spicetify_color_ini() {
        let ini = "; comment\n[Base]\nmain = 121212 ; background\ntext = #FFFFFF\nbutton = 1db954\nsubtext=b3b3b3\nnonsense = zzz\n";
        let mut w = vec![];
        let schemes = parse_ini(ini, &mut w);
        assert_eq!(schemes.len(), 1);
        let p = palette_from(&schemes[0].1);
        assert_eq!(p.bg, Color32::from_rgb(0x12, 0x12, 0x12));
        assert_eq!(p.text, Color32::WHITE);
        assert_eq!(p.accent, Color32::from_rgb(0x1d, 0xb9, 0x54));
        assert_eq!(p.sidebar, p.bg, "missing keys are worked out");
        assert_eq!(w.len(), 1, "the bad line is reported: {w:?}");
    }

    #[test]
    fn reads_the_css_subset_and_skips_the_rest() {
        let css = "/* hi */ :root { --radius-lg: 20px } .card, .hero { background: var(--spice-card); border-radius: 18px; box-shadow: 0 0 4px red; }\n.player-bar { border-top: 1px solid rgb(10, 20, 30) } .nope { color: red } .hero { text-transform: uppercase; font-weight: 700 }";
        let mut w = vec![];
        let c = parse_css(css, &mut w);
        assert_eq!(c.radius[2], Some(20.0));
        let vars = HashMap::from([("card".to_string(), Color32::from_rgb(1, 2, 3))]);
        let parts = c.resolve(&vars, &DARK);
        assert_eq!(parts.card.fill, Some(Color32::from_rgb(1, 2, 3)));
        assert_eq!(parts.hero.radius, Some(18));
        assert_eq!(parts.hero.uppercase, Some(true));
        assert_eq!(parts.hero.bold, Some(true));
        assert_eq!(parts.player_bar.border, Some(Stroke::new(1.0, Color32::from_rgb(10, 20, 30))));
        assert_eq!(w.len(), 3, "box-shadow twice and .nope: {w:?}");
    }

    #[test]
    fn broken_files_never_crash() {
        let junk = ["", "{{{{", "}}}", ".card {", ":root { --radius-sm: -5px }", "[", "=", "main = ", "[x]\nmain = #12", "name = [1, 2", "\u{0}\u{ffff}", ".card { background: var( ; color: rgb(1) }"];
        for a in junk {
            for b in junk {
                let t = parse("x", a, b, a, None, false);
                let _ = t.active(0, false);
                let _ = t.active(5, true);
            }
        }
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("#fff"), Some(Color32::WHITE));
        assert_eq!(parse_color("1db954"), Some(Color32::from_rgb(0x1d, 0xb9, 0x54)));
        assert_eq!(parse_color("rgba(255, 0, 0, 0.5)"), Some(Color32::from_rgba_unmultiplied(255, 0, 0, 127)));
        assert_eq!(parse_color("nope"), None);
        assert_eq!(parse_color("#12345"), None);
    }
}
