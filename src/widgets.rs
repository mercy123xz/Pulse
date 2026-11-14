//! Reusable, custom-drawn UI pieces (buttons, artwork, sliders, track rows...).

use crate::i18n::tr;
use crate::deezer::Track;
use crate::theme::{self, *};
use egui::{
    Align2, Color32, CornerRadius, FontId, Key, Modifiers, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2,
    vec2,
};
use egui_phosphor::regular as icon;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

/// Lays out one line of text, cutting it off with "…" if it is wider than `max_w`.
pub fn one_line(ui: &Ui, text: &str, font: FontId, color: Color32, max_w: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(1.0));
    ui.painter().layout_job(job)
}

/// Draws one truncated line and returns its rect.
pub fn paint_line(ui: &Ui, pos: Pos2, text: &str, font: FontId, color: Color32, max_w: f32) -> Rect {
    let g = one_line(ui, text, font, color, max_w);
    let rect = Rect::from_min_size(pos, g.size());
    ui.painter().galley(pos, g, color);
    rect
}

pub fn fmt_duration(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn fmt_count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}K", n as f64 / 1e3).replace(".0K", "K"),
        _ => format!("{:.1}M", n as f64 / 1e6).replace(".0M", "M"),
    }
}

// ---------------------------------------------------------------------------
// Artwork
// ---------------------------------------------------------------------------

/// Draws album art from a URL into `rect` (only if visible, so off-screen
/// images aren't downloaded). Shows a placeholder until it loads.
/// Stands in for a picture URL: draws the Liked Songs heart art.
pub const LIKED_ART: &str = "pulse://liked";

pub fn artwork(ui: &Ui, rect: Rect, url: Option<&str>, radius: CornerRadius) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    if url == Some(LIKED_ART) {
        liked_art(ui, rect, radius);
        return;
    }
    ui.painter().rect_filled(rect, radius, pal().surface_active);
    paint_icon(ui.painter(), rect.center(), icon::MUSIC_NOTES, regular(rect.height() * 0.38), pal().text_faint);
    if let Some(url) = url {
        crate::artcache::drawn(url);
        let mut img = egui::Image::new(url).corner_radius(radius).show_loading_spinner(false);
        // YouTube video pictures are wide: show their middle square instead of squashing them.
        if url.contains("i.ytimg.com/") {
            let uv = if url.contains("hqdefault") {
                // 4:3 with black bars above and below the 16:9 picture.
                Rect::from_min_max(pos2(0.21875, 0.125), pos2(0.78125, 0.875))
            } else {
                Rect::from_min_max(pos2(0.21875, 0.0), pos2(0.78125, 1.0))
            };
            img = img.uv(uv);
        }
        paint_loaded(ui, img, rect);
    }
}

/// Paints a picture once it's there. One that couldn't be downloaded (offline, a dead
/// link) leaves the placeholder underneath, instead of egui's red "failed" mark.
fn paint_loaded(ui: &Ui, img: egui::Image, rect: Rect) {
    if img.load_for_size(ui.ctx(), rect.size()).is_ok() {
        img.paint_at(ui, rect);
    }
}

/// Liked Songs' cover: a filled heart on a diagonal gradient of the theme's accent.
fn liked_art(ui: &Ui, rect: Rect, radius: CornerRadius) {
    let dark = pal().accent;
    let light = pal().accent_soft.lerp_to_gamma(Color32::WHITE, 0.3);
    // The rounded outline, filled as a fan with each point colored by how far along
    // the top-left → bottom-right diagonal it is.
    let mut outline = vec![];
    egui::epaint::tessellator::path::rounded_rectangle(&mut outline, rect, radius.into());
    let span = (rect.width() + rect.height()).max(1.0);
    let at = |p: Pos2| light.lerp_to_gamma(dark, ((p.x - rect.left()) + (p.y - rect.top())) / span);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), at(rect.center()));
    for p in &outline {
        mesh.colored_vertex(*p, at(*p));
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    paint_icon(ui.painter(), rect.center() + vec2(0.0, rect.height() * 0.02), icon::HEART, filled_icon(rect.height() * 0.42), Color32::WHITE);
}

/// Vertical gradient (used behind page headers).
/// Like `artwork`, but fills `rect` without stretching: a square picture is cropped
/// to the rect's shape. `focus_y` (0 = top, 1 = bottom) picks which band to keep.
pub fn artwork_cover(ui: &Ui, rect: Rect, url: Option<&str>, radius: CornerRadius, focus_y: f32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().rect_filled(rect, radius, pal().surface_active);
    let Some(url) = url else { return };
    let aspect = rect.width() / rect.height().max(1.0);
    // Deezer's artist and cover pictures are square.
    let uv = if aspect >= 1.0 {
        let h = 1.0 / aspect;
        let top = ((1.0 - h) * focus_y).clamp(0.0, 1.0 - h);
        Rect::from_min_max(pos2(0.0, top), pos2(1.0, top + h))
    } else {
        let w = aspect;
        let left = (1.0 - w) / 2.0;
        Rect::from_min_max(pos2(left, 0.0), pos2(left + w, 1.0))
    };
    crate::artcache::drawn(url);
    paint_loaded(ui, egui::Image::new(url).uv(uv).corner_radius(radius).show_loading_spinner(false), rect);
}

pub fn vgradient(ui: &Ui, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    ui.painter().add(egui::Shape::mesh(mesh));
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

/// Gives a custom-drawn widget a screen-reader name and a visible keyboard-focus ring.
/// (egui only gives keyboard focus via Tab, so the ring never shows on mouse clicks.)
/// `label` is a closure so the text is only built when egui actually asks for it
/// (screen reader running, or focus just changed), not on every repaint.
pub fn a11y(ui: &Ui, resp: &Response, kind: WidgetType, label: impl Fn() -> String) {
    resp.widget_info(|| WidgetInfo::labeled(kind, ui.is_enabled(), label()));
    focus_ring(ui, resp);
}

/// Draws the focus ring, and scrolls a widget into view when Tab moves focus to it
/// (egui only does that on its own when a screen reader asks).
pub fn focus_ring(ui: &Ui, resp: &Response) {
    if resp.gained_focus() {
        resp.scroll_to_me(None);
    }
    if resp.has_focus() {
        let r = CornerRadius::same((resp.rect.height() / 2.0).min(10.0) as u8);
        ui.painter().rect_stroke(resp.rect.expand(2.0), r, Stroke::new(2.0, pal().accent_soft), egui::StrokeKind::Outside);
    }
}

/// A round, borderless icon button. `tip` doubles as its screen-reader name.
/// A small rounded button with an icon and a short label ("Settings", "Light").
pub fn labeled_button(ui: &mut Ui, glyph: &str, label: &str, active: bool) -> Response {
    let font = med(12.5);
    let g = one_line(ui, label, font, pal().text, 200.0);
    let w = 12.0 + 16.0 + 6.0 + g.size().x + 12.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
    a11y(ui, &resp, WidgetType::Button, || label.to_owned());
    focus_ring(ui, &resp);
    let hovered = resp.hovered();
    let fill = if active { pal().surface_active } else if hovered { pal().surface_hover } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, CornerRadius::same(16), fill);
    let col = if active { pal().accent_soft } else if hovered { pal().text } else { pal().text_muted };
    let text_col = if active || hovered { pal().text } else { pal().text_muted };
    paint_icon(ui.painter(), pos2(rect.left() + 20.0, rect.center().y), glyph, regular(16.0), col);
    ui.painter().galley(pos2(rect.left() + 34.0, rect.center().y - g.size().y / 2.0), g, text_col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn icon_button(ui: &mut Ui, glyph: &str, size: f32, color: Color32, tip: &str) -> Response {
    icon_button_impl(ui, glyph, size, color, tip, None)
}

/// An icon button that is switched on or off (shuffle, repeat). `name` is the
/// screen-reader name; the on/off state is announced separately.
pub fn icon_toggle(ui: &mut Ui, glyph: &str, size: f32, color: Color32, tip: &str, name: &str, on: bool) -> Response {
    icon_button_impl(ui, glyph, size, color, tip, Some((name, on)))
}

fn icon_button_impl(ui: &mut Ui, glyph: &str, size: f32, color: Color32, tip: &str, toggle: Option<(&str, bool)>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size + 12.0), Sense::click());
    match toggle {
        Some((name, on)) => resp_selected(ui, &resp, on, name),
        None => a11y(ui, &resp, WidgetType::Button, || tip.to_owned()),
    }
    let hovered = resp.hovered();
    if hovered {
        ui.painter().circle_filled(rect.center(), rect.width() / 2.0, pal().surface_hover);
    }
    let c = if hovered && color == pal().text_muted { pal().text } else { color };
    paint_icon(ui.painter(), rect.center(), glyph, regular(size), c);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// Small dot-indicator used under active toggle buttons (shuffle / repeat).
pub fn active_dot(ui: &Ui, resp: &Response) {
    ui.painter().circle_filled(pos2(resp.rect.center().x, resp.rect.bottom() - 1.0), 2.0, pal().accent_soft);
}

/// The big round accent "play" button.
pub fn play_button(ui: &mut Ui, size: f32, playing: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    a11y(ui, &resp, WidgetType::Button, || if playing { "Pause" } else { "Play" }.to_owned());
    // Hover is shown by the grow animation only: white on the lighter accent is too faint.
    let t = ui.ctx().animate_bool(resp.id, resp.hovered());
    let r = size / 2.0 * (1.0 + 0.05 * t);
    let style = &parts().play_button;
    ui.painter().circle_filled(rect.center(), r, style.fill.unwrap_or(pal().accent));
    if let Some(b) = style.border {
        ui.painter().circle_stroke(rect.center(), r, b);
    }
    let glyph = if playing { icon::PAUSE } else { icon::PLAY };
    paint_icon(ui.painter(), rect.center(), glyph, filled_icon(size * 0.42), style.color.unwrap_or(Color32::WHITE));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A pill-shaped toggle (e.g. "This week" / "This month").
pub fn pill(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let label = tr(label);
    let probe = ui.painter().layout_no_wrap(label.to_owned(), med(12.5), pal().text);
    let size = vec2(probe.size().x + 26.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, label));
    focus_ring(ui, &resp);
    let (fill, fg) = if selected {
        (pal().text, pal().bg)
    } else if resp.hovered() {
        (pal().surface_active, pal().text)
    } else {
        (pal().surface_hover, pal().text)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(15), fill);
    paint_icon(ui.painter(), rect.center(), label, med(12.5), fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Outline button with icon + label ("Shuffle", "See all", ...).
pub fn ghost_button(ui: &mut Ui, glyph: &str, label: &str) -> Response {
    let label = tr(label);
    let g = ui.painter().layout_no_wrap(label.to_owned(), med(13.0), pal().text);
    let icon_w = if glyph.is_empty() { 0.0 } else { 22.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + icon_w + 28.0, 34.0), Sense::click());
    a11y(ui, &resp, WidgetType::Button, || label.to_owned());
    focus_ring(ui, &resp);
    let hovered = resp.hovered();
    let stroke = if hovered { Stroke::new(1.0, pal().text_muted) } else { Stroke::new(1.0, pal().border) };
    let fill = if hovered { pal().surface_hover } else { Color32::TRANSPARENT };
    ui.painter().rect(rect, CornerRadius::same(17), fill, stroke, egui::StrokeKind::Inside);
    // The icon is drawn on its own so it's centered by its drawing, like every other icon.
    let left = rect.left() + 14.0;
    if !glyph.is_empty() {
        paint_icon(ui.painter(), pos2(left + 8.0, rect.center().y), glyph, regular(15.0), pal().text);
    }
    ui.painter().galley(pos2(left + icon_w, rect.center().y - g.size().y / 2.0), g, pal().text);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A text "link" (e.g. artist name) that underlines on hover.
/// The song's artists as links: "Main, Featured 1, Featured 2". Returns the main
/// artist's link and, if a featured artist was clicked, that artist's id.
pub fn artist_links(ui: &Ui, id: egui::Id, pos: Pos2, t: &Track, font: FontId, color: Color32, max_w: f32) -> (Response, Option<crate::deezer::ArtistRef>) {
    let main = link_text(ui, id.with("artist"), pos, &t.artist.name, font.clone(), color, max_w);
    let mut x = main.rect.right();
    let mut clicked = None;
    for (k, a) in t.featured.iter().enumerate() {
        let room = pos.x + max_w - x;
        if room < 30.0 {
            break;
        }
        let comma = ui.painter().text(pos2(x, pos.y), Align2::LEFT_TOP, ", ", font.clone(), color);
        x = comma.right();
        let r = link_text(ui, id.with(("feat", k)), pos2(x, pos.y), &a.name, font.clone(), color, pos.x + max_w - x);
        x = r.rect.right();
        if r.clicked() {
            clicked = Some(a.clone());
        }
    }
    (main, clicked)
}

pub fn link_text(ui: &Ui, rect_id: egui::Id, pos: Pos2, text: &str, font: FontId, color: Color32, max_w: f32) -> Response {
    let g = one_line(ui, text, font, color, max_w);
    let rect = Rect::from_min_size(pos, g.size());
    let resp = ui.interact(rect, rect_id, Sense::click());
    a11y(ui, &resp, WidgetType::Link, || text.to_owned());
    let col = if resp.hovered() { pal().text } else { color };
    ui.painter().galley_with_override_text_color(pos, g, col);
    if resp.hovered() {
        ui.painter().hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(1.0, col));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------------------
// Sliders
// ---------------------------------------------------------------------------

/// A thin, modern slider. `value` and `buffered` are 0..=1.
/// Returns (response, value while dragging / clicked value).
/// When focused with Tab, ←/→ move it by 5% and mark the response as changed.
pub fn slim_slider(ui: &mut Ui, width: f32, value: f32, buffered: Option<f32>, label: &str) -> (Response, Option<f32>) {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, 16.0), Sense::click_and_drag());
    resp.widget_info(|| WidgetInfo::slider(true, (value * 100.0).round() as f64, label));
    focus_ring(ui, &resp);
    let hot = resp.hovered() || resp.dragged() || resp.has_focus();
    let track_h = if hot { 5.0 } else { 4.0 };
    let track = Rect::from_center_size(rect.center(), vec2(rect.width(), track_h));
    let r = CornerRadius::same(3);

    let mut new_val = None;
    if let Some(p) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked() || resp.drag_stopped())
    {
        new_val = Some(((p.x - track.left()) / track.width()).clamp(0.0, 1.0));
    }
    if resp.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(resp.id, egui::EventFilter { horizontal_arrows: true, ..Default::default() })
        });
        let step = ui.input(|i| i.key_pressed(Key::ArrowRight) as i32 - i.key_pressed(Key::ArrowLeft) as i32);
        if step != 0 {
            new_val = Some((value + step as f32 * 0.05).clamp(0.0, 1.0));
            resp.mark_changed();
        }
    }
    let shown = new_val.unwrap_or(value).clamp(0.0, 1.0);

    let p = ui.painter();
    p.rect_filled(track, r, pal().surface_active);
    if let Some(b) = buffered {
        let mut br = track;
        br.set_right(track.left() + track.width() * b.clamp(0.0, 1.0));
        p.rect_filled(br, r, pal().text_faint);
    }
    let mut fill = track;
    fill.set_right(track.left() + track.width() * shown);
    p.rect_filled(fill, r, if hot { pal().accent_soft } else { pal().text });
    if hot {
        p.circle_filled(pos2(fill.right(), track.center().y), 6.5, pal().text);
    }
    (resp.on_hover_cursor(egui::CursorIcon::PointingHand), new_val)
}

// ---------------------------------------------------------------------------
// Cards (playlists, artists)
// ---------------------------------------------------------------------------

pub struct CardInfo<'a> {
    pub id: egui::Id,
    pub art: Option<&'a str>,
    pub title: &'a str,
    pub subtitle: String,
    pub round: bool,
    /// A label on the picture, like "Orbit".
    pub badge: Option<&'a str>,
}

pub enum CardClick {
    None,
    Open,
    Play,
    /// The pointer has rested on the card: a good moment to load its page ahead.
    Preview,
}

/// True once when the pointer has rested on something for a quarter second.
pub fn hover_intent(ui: &Ui, id: egui::Id, hovered: bool) -> bool {
    let key = id.with("hover_intent");
    if !hovered {
        ui.data_mut(|d| d.remove::<f64>(key));
        return false;
    }
    let now = ui.input(|i| i.time);
    match ui.data(|d| d.get_temp::<f64>(key)) {
        None => {
            ui.data_mut(|d| d.insert_temp(key, now));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(260));
            false
        }
        // Negative: already reported for this hover.
        Some(start) if start >= 0.0 && now - start >= 0.25 => {
            ui.data_mut(|d| d.insert_temp(key, -1.0));
            true
        }
        _ => false,
    }
}

/// A readable fallback for artists without a picture.
pub fn artist_monogram(ui: &Ui, rect: Rect, name: &str, radius: CornerRadius) {
    ui.painter().rect_filled(rect, radius, pal().surface_active.lerp_to_gamma(pal().accent, 0.18));
    let initial = name.chars().find(|c| c.is_alphanumeric()).unwrap_or('?').to_uppercase().to_string();
    paint_icon(ui.painter(), rect.center(), &initial, heavy(rect.height() * 0.42), pal().text);
}
pub fn card(ui: &mut Ui, w: f32, info: &CardInfo) -> CardClick {
    let h = w + 66.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    a11y(ui, &resp, WidgetType::Button, || format!("{}, {}", info.title, info.subtitle));
    let hovered = resp.hovered();
    let style = &parts().card;
    let radius = style.radius.map(CornerRadius::same).unwrap_or(r_lg());
    if hovered {
        ui.painter().rect_filled(rect, radius, pal().surface_hover);
    } else if let Some(fill) = style.fill {
        ui.painter().rect_filled(rect, radius, fill);
    }
    if let Some(b) = style.border {
        ui.painter().rect_stroke(rect, radius, b, egui::StrokeKind::Inside);
    }
    let pad = style.padding.unwrap_or(10.0);
    let art_rect = Rect::from_min_size(rect.min + vec2(pad, pad), Vec2::splat(w - pad * 2.0));
    let radius = if info.round { CornerRadius::same(((w - pad * 2.0) / 2.0).min(255.0) as u8) } else { r_md() };
    if info.round && info.art.is_none_or(|url| url.is_empty()) {
        artist_monogram(ui, art_rect, info.title, radius);
    } else {
        artwork(ui, art_rect, info.art, radius);
    }
    if let Some(b) = info.badge {
        // Darken the bottom of the picture a little and put the label on it.
        vgradient(ui, Rect::from_min_max(pos2(art_rect.left(), art_rect.bottom() - 46.0), art_rect.max), Color32::TRANSPARENT, Color32::from_black_alpha(150));
        let g = one_line(ui, b, heavy(15.0), Color32::WHITE, art_rect.width() - 20.0);
        let label = Rect::from_min_size(pos2(art_rect.left() + 8.0, art_rect.bottom() - 32.0), g.size() + vec2(12.0, 6.0));
        ui.painter().rect_filled(label, CornerRadius::same(4), pal().accent);
        ui.painter().galley(label.min + vec2(6.0, 3.0), g, Color32::WHITE);
    }

    let text_w = w - pad * 2.0;
    paint_line(ui, pos2(rect.left() + pad, art_rect.bottom() + 10.0), info.title, semi(13.5), pal().text, text_w);
    paint_line(ui, pos2(rect.left() + pad, art_rect.bottom() + 30.0), &info.subtitle, regular(12.0), pal().text_muted, text_w);

    // Floating play button appears on hover.
    let mut out = if resp.clicked() {
        CardClick::Open
    } else if hover_intent(ui, info.id, hovered) {
        CardClick::Preview
    } else {
        CardClick::None
    };
    let t = ui.ctx().animate_bool(info.id.with("hover"), hovered);
    if t > 0.0 && !info.round {
        let c = pos2(art_rect.right() - 26.0, art_rect.bottom() - 26.0 + (1.0 - t) * 8.0);
        let btn = Rect::from_center_size(c, Vec2::splat(42.0));
        let b = ui.interact(btn, info.id.with("play"), Sense::click());
        a11y(ui, &b, WidgetType::Button, || format!("Play {}", info.title));
        let grow = if b.hovered() { 1.06 } else { 1.0 };
        ui.painter().circle_filled(c + vec2(0.0, 3.0), 22.0, Color32::from_black_alpha((90.0 * t) as u8));
        let pb = &parts().play_button;
        ui.painter().circle_filled(c, 21.0 * grow, pb.fill.unwrap_or(pal().accent).gamma_multiply(t));
        paint_icon(ui.painter(), c, icon::PLAY, filled_icon(17.0), pb.color.unwrap_or(Color32::WHITE).gamma_multiply(t));
        if b.clicked() {
            out = CardClick::Play;
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    out
}

/// Lays out a responsive grid of cards. `max_rows` limits how many rows show.
pub fn card_grid<T>(ui: &mut Ui, items: &[T], max_rows: Option<usize>, mut each: impl FnMut(&mut Ui, f32, &T)) {
    let gap = 8.0;
    let avail = ui.available_width();
    let cols = ((avail + gap) / (176.0 + gap)).floor().max(2.0) as usize;
    let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let limit = max_rows.map(|r| r * cols).unwrap_or(usize::MAX);
    for row in items.iter().take(limit).collect::<Vec<_>>().chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for item in row {
                each(ui, w, item);
            }
        });
        ui.add_space(gap);
    }
}

// ---------------------------------------------------------------------------
// Track table
// ---------------------------------------------------------------------------

/// Things a user can do to a track from a list.
#[derive(Clone)]
pub enum TrackEvent {
    Play(usize),
    /// The pointer rests on the song's row: find its audio now, so pressing it plays at once.
    Warm(Track),
    /// The pointer rests on the song's title: load the page it would open.
    Preview(Track),
    /// Move an upcoming song in the queue: (its row in "Next up", rows to move; negative = up).
    MoveInQueue(usize, isize),
    PlayNext(Track),
    AddToQueue(Track),
    ToggleLike(Track),
    AddToPlaylist(usize, Track),
    NewPlaylistWith(Track),
    NewPlaylistWithMany(Vec<Track>),
    /// Add to Liked Songs (never removes, unlike ToggleLike).
    Like(Track),
    /// A row was clicked (Ctrl: add/remove it, Shift: select up to it).
    Select { list: String, index: usize, ctrl: bool, shift: bool },
    SelectAll { list: String, len: usize },
    ClearSelection,
    /// "Go to song": the song's own page.
    OpenSongPage(Track),
    /// A featured artist's name was clicked (Deezer artist id).
    OpenArtistRef(crate::deezer::ArtistRef),
    RemoveFromPlaylist(usize),
    RemoveFromQueue(usize),
    OpenArtist(Track),
    OpenAlbum(String),
    ShowCredits(Track),
    Share(Track, ShareTarget),
    /// The song's title was clicked: open its album with the song highlighted.
    OpenSong(Track),
    Download(Track),
    RemoveDownload(Track),
}

/// Where a "Copy link" from the Share menu points.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShareTarget {
    YouTube,
    Spotify,
    Deezer,
}

/// What the song menu needs to know about where it was opened.
pub struct MenuCtx<'a> {
    pub liked: bool,
    pub playlists: &'a [String],
    /// Row number in the list it was opened from (None for the player bar).
    pub index: Option<usize>,
    pub removable: bool,
    pub queue_mode: bool,
    pub downloaded: bool,
}

/// The right-click menu for a song. Used by every song list and the player bar.
pub fn track_menu(ui: &mut Ui, t: &Track, cx: &MenuCtx, ev: &mut Vec<TrackEvent>) {
    menu_begin(ui);
    if let Some(i) = cx.index
        && menu_item(ui, icon::PLAY, "Play")
    {
        ev.push(TrackEvent::Play(i));
    }
    submenu(ui, icon::PLUS, "Add to playlist", |ui| {
        if menu_item(ui, icon::PLUS, "New playlist…") {
            ev.push(TrackEvent::NewPlaylistWith(t.clone()));
        }
        if !cx.playlists.is_empty() {
            menu_separator(ui);
        }
        for (pi, name) in cx.playlists.iter().enumerate() {
            if menu_item(ui, icon::PLAYLIST, name) {
                ev.push(TrackEvent::AddToPlaylist(pi, t.clone()));
            }
        }
    });
    let like_label = if cx.liked { "Remove from Liked Songs" } else { "Save to Liked Songs" };
    if menu_item(ui, icon::HEART, like_label) {
        ev.push(TrackEvent::ToggleLike(t.clone()));
    }
    if !cx.queue_mode {
        if menu_item(ui, icon::QUEUE, "Add to queue") {
            ev.push(TrackEvent::AddToQueue(t.clone()));
        }
        if menu_item(ui, icon::ARROW_BEND_DOWN_RIGHT, "Play next") {
            ev.push(TrackEvent::PlayNext(t.clone()));
        }
    }
    menu_separator(ui);
    if crate::deezer::is_deezer_id(&t.artist.id) && menu_item(ui, icon::USER, "Go to artist") {
        ev.push(TrackEvent::OpenArtist(t.clone()));
    }
    if let Some(al) = t.album.as_ref().filter(|a| crate::deezer::is_deezer_id(&a.id))
        && menu_item(ui, icon::VINYL_RECORD, "Go to album")
    {
        ev.push(TrackEvent::OpenAlbum(al.id.clone()));
    }
    if !crate::local::is_local(&t.id) {
        if cx.downloaded {
            if menu_item(ui, icon::TRASH, tr("Remove download")) {
                ev.push(TrackEvent::RemoveDownload(t.clone()));
            }
        } else if menu_item(ui, icon::DOWNLOAD_SIMPLE, tr("Download")) {
            ev.push(TrackEvent::Download(t.clone()));
        }
    }
    if menu_item(ui, icon::MUSIC_NOTE, "Go to song") {
        ev.push(TrackEvent::OpenSongPage(t.clone()));
    }
    if menu_item(ui, icon::IDENTIFICATION_CARD, "View credits") {
        ev.push(TrackEvent::ShowCredits(t.clone()));
    }
    menu_separator(ui);
    submenu(ui, icon::SHARE_FAT, "Share", |ui| {
        for (glyph, label, target) in [
            (icon::YOUTUBE_LOGO, "Copy YouTube link", ShareTarget::YouTube),
            (icon::SPOTIFY_LOGO, "Copy Spotify link", ShareTarget::Spotify),
            (icon::LINK, "Copy Deezer link", ShareTarget::Deezer),
        ] {
            if target == ShareTarget::Deezer && t.deezer_number().is_none() {
                continue;
            }
            if menu_item(ui, glyph, label) {
                ev.push(TrackEvent::Share(t.clone(), target));
            }
        }
    });
    if cx.removable {
        menu_separator(ui);
        if menu_item(ui, icon::TRASH, "Remove from this playlist")
            && let Some(i) = cx.index
        {
            ev.push(TrackEvent::RemoveFromPlaylist(i));
        }
    }
    if cx.queue_mode
        && let Some(i) = cx.index
    {
        menu_separator(ui);
        if i > 0 {
            if menu_item(ui, icon::ARROW_LINE_UP, "Move to top") {
                ev.push(TrackEvent::MoveInQueue(i, -(i as isize)));
            }
            if menu_item(ui, icon::ARROW_UP, "Move up") {
                ev.push(TrackEvent::MoveInQueue(i, -1));
            }
        }
        if menu_item(ui, icon::ARROW_DOWN, "Move down") {
            ev.push(TrackEvent::MoveInQueue(i, 1));
        }
        if menu_item(ui, icon::X, "Remove from queue") {
            ev.push(TrackEvent::RemoveFromQueue(i));
        }
    }
}

pub struct TableCtx<'a> {
    pub now_playing: Option<&'a str>,
    pub is_playing: bool,
    pub liked: &'a HashSet<String>,
    pub playlists: &'a [String],
    /// Set when showing one of your own playlists (enables "Remove from playlist").
    pub removable: bool,
    /// Set when showing the queue (enables "Remove from queue").
    pub queue_mode: bool,
    /// Downloaded songs (shown with a small arrow).
    pub downloads: &'a HashMap<String, std::path::PathBuf>,
    /// When songs were added (shows the "Date added" column).
    pub added: Option<&'a HashMap<String, u64>>,
    /// YouTube views (shows the "Views" column).
    pub views: Option<&'a HashMap<String, (u64, u64)>>,
    /// A song to point out (opened by clicking its title somewhere else).
    pub highlight: Option<(&'a str, f64)>,
    /// Selected rows, and the list they're in.
    pub selection: (&'a str, &'a HashSet<usize>),
    /// Number shown for the first row (queue shows "next up" starting at 1).
    pub index_offset: usize,
}

const ROW_H: f32 = 56.0;

struct Cols {
    num: Rect,
    title: Rect,
    album: Option<Rect>,
    views: Option<Rect>,
    date: Option<Rect>,
    like: Rect,
    time: Rect,
}

fn columns(row: Rect, with_date: bool, with_views: bool) -> Cols {
    let w = row.width();
    let x0 = row.left();
    let col = |a: f32, b: f32| Rect::from_x_y_ranges(a..=b, row.y_range());
    let num = col(x0, x0 + 44.0);
    let time = col(row.right() - 64.0, row.right() - 12.0);
    let like = col(time.left() - 40.0, time.left());
    let date = (with_date && w > 900.0).then(|| col(like.left() - 150.0, like.left() - 12.0));
    let after_views = date.map(|d| d.left()).unwrap_or(like.left());
    let views = (with_views && w > 640.0).then(|| col(after_views - 130.0, after_views - 20.0));
    let wide = w > 720.0;
    let album_right = views.map(|v| v.left()).unwrap_or(after_views) - 16.0;
    let album_w = (w * if date.is_some() { 0.24 } else { 0.3 }).min(320.0);
    let album = wide.then(|| col(album_right - album_w, album_right));
    let title_right = album.map(|a| a.left()).unwrap_or(like.left()) - 16.0;
    Cols { num, title: col(num.right() + 4.0, title_right), album, views, date, like, time }
}

/// 5003515 → "5,003,515".
pub fn fmt_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "Date added" like Spotify: "3 days ago" for recent songs, else the date.
pub fn fmt_added(when: u64, now: u64) -> String {
    let age = now.saturating_sub(when);
    let day = 86_400;
    match age {
        a if a < day => "Today".into(),
        a if a < 2 * day => "Yesterday".into(),
        a if a < 7 * day => format!("{} days ago", a / day),
        a if a < 28 * day => {
            let w = a / (7 * day);
            if w == 1 { "1 week ago".into() } else { format!("{w} weeks ago") }
        }
        _ => {
            // Days since 1970 → calendar date (Howard Hinnant's algorithm).
            let z = (when / day) as i64 + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let y = yoe + era * 400 + i64::from(m <= 2);
            const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            format!("{} {d}, {y}", MONTHS[(m - 1) as usize])
        }
    }
}

pub fn track_table_header(ui: &mut Ui, with_date: bool, with_views: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let c = columns(rect, with_date, with_views);
    let f = med(11.5);
    let p = ui.painter();
    paint_icon(p, c.num.center(), "#", f.clone(), pal().text_faint);
    p.text(c.title.left_center(), Align2::LEFT_CENTER, tr("Title").to_uppercase(), f.clone(), pal().text_faint);
    if let Some(a) = c.album {
        p.text(a.left_center(), Align2::LEFT_CENTER, tr("Album").to_uppercase(), f.clone(), pal().text_faint);
    }
    if let Some(d) = c.date {
        p.text(d.left_center(), Align2::LEFT_CENTER, tr("Date added").to_uppercase(), f.clone(), pal().text_faint);
    }
    if let Some(v) = c.views {
        let r = p.text(v.right_center(), Align2::RIGHT_CENTER, tr("Views").to_uppercase(), f.clone(), pal().text_faint);
        ui.interact(r, ui.id().with("views_hdr"), Sense::hover()).on_hover_text(tr("Views on YouTube"));
    }
    p.text(c.time.right_center(), Align2::RIGHT_CENTER, icon::CLOCK_COUNTER_CLOCKWISE, regular(14.0), pal().text_faint);
    p.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, pal().border));
    ui.add_space(6.0);
}

pub fn track_table(ui: &mut Ui, id_salt: &str, tracks: &[Track], cx: &TableCtx, events: &mut Vec<TrackEvent>) {
    track_table_header(ui, cx.added.is_some(), cx.views.is_some());
    let base = ui.id().with(id_salt);
    // This list's selected rows (selection belongs to one list at a time).
    let empty = HashSet::new();
    let selected = if cx.selection.0 == id_salt { cx.selection.1 } else { &empty };
    ui.spacing_mut().item_spacing.y = 0.0;
    // Only the rows on screen (plus a few either side, so Tab and the arrow keys can step
    // onto the next one) are built. Long playlists stay as quick as short ones.
    let top = ui.cursor().top();
    let clip = ui.clip_rect();
    const MARGIN: usize = 6;
    let first = (((clip.top() - top) / ROW_H).floor().max(0.0) as usize).saturating_sub(MARGIN).min(tracks.len());
    let last = ((((clip.bottom() - top) / ROW_H).ceil().max(0.0) as usize) + MARGIN).clamp(first, tracks.len());
    // The song you came here for is scrolled into view once, even from far down the list.
    if let Some((hid, at)) = cx.highlight
        && let Some(i) = tracks.iter().position(|t| t.id == hid)
        && !(first..last).contains(&i)
    {
        let key = egui::Id::new(("highlight_scrolled", hid, at.to_bits()));
        if !ui.ctx().data(|d| d.get_temp::<bool>(key).unwrap_or(false)) {
            let y = top + i as f32 * ROW_H;
            ui.scroll_to_rect(egui::Rect::from_min_size(egui::pos2(ui.max_rect().left(), y), vec2(1.0, ROW_H)), Some(egui::Align::Center));
            ui.ctx().data_mut(|d| d.insert_temp(key, true));
        }
    }
    if first > 0 {
        ui.allocate_space(vec2(ui.available_width(), first as f32 * ROW_H));
    }
    for (i, t) in tracks.iter().enumerate().take(last).skip(first) {
        track_row(ui, base.with(i), id_salt, i, t, tracks, selected, cx, events);
    }
    if last < tracks.len() {
        ui.allocate_space(vec2(ui.available_width(), (tracks.len() - last) as f32 * ROW_H));
    }
    ui.spacing_mut().item_spacing.y = 6.0;

    // Keyboard, while this list has a selection (and nothing is being typed).
    if !selected.is_empty() && !ui.ctx().egui_wants_keyboard_input() {
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::A)) {
            events.push(TrackEvent::SelectAll { list: id_salt.to_string(), len: tracks.len() });
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            events.push(TrackEvent::ClearSelection);
        }
        if cx.removable && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete)) {
            let mut rows: Vec<usize> = selected.iter().copied().collect();
            rows.sort_unstable_by(|a, b| b.cmp(a));
            events.extend(rows.into_iter().map(TrackEvent::RemoveFromPlaylist));
            events.push(TrackEvent::ClearSelection);
        }
        let focused_row = ui.ctx().memory(|m| m.focused()).is_some();
        if !focused_row && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
            && let Some(first) = selected.iter().min()
        {
            events.push(TrackEvent::Play(*first));
        }
    }
}

/// The right-click menu for several selected songs at once.
fn group_menu(ui: &mut Ui, rows: &[usize], tracks: &[Track], cx: &TableCtx, ev: &mut Vec<TrackEvent>) {
    menu_begin(ui);
    let picked: Vec<Track> = rows.iter().filter_map(|i| tracks.get(*i).cloned()).collect();
    menu_header(ui, &format!("{} {}", picked.len(), tr("songs selected")));
    menu_separator(ui);
    if menu_item(ui, icon::PLAY, "Play") {
        ev.push(TrackEvent::Play(rows[0]));
    }
    submenu(ui, icon::PLUS, "Add to playlist", |ui| {
        if menu_item(ui, icon::PLUS, "New playlist…") {
            ev.push(TrackEvent::NewPlaylistWithMany(picked.clone()));
        }
        if !cx.playlists.is_empty() {
            menu_separator(ui);
        }
        for (pi, name) in cx.playlists.iter().enumerate() {
            if menu_item(ui, icon::PLAYLIST, name) {
                ev.extend(picked.iter().map(|t| TrackEvent::AddToPlaylist(pi, t.clone())));
            }
        }
    });
    if menu_item(ui, icon::HEART, "Save to Liked Songs") {
        ev.extend(picked.iter().map(|t| TrackEvent::Like(t.clone())));
    }
    if !cx.queue_mode && menu_item(ui, icon::QUEUE, "Add to queue") {
        ev.extend(picked.iter().map(|t| TrackEvent::AddToQueue(t.clone())));
    }
    if picked.iter().any(|t| !crate::local::is_local(&t.id) && !cx.downloads.contains_key(&t.id)) && menu_item(ui, icon::DOWNLOAD_SIMPLE, "Download") {
        ev.extend(picked.iter().map(|t| TrackEvent::Download(t.clone())));
    }
    if cx.removable || cx.queue_mode {
        menu_separator(ui);
        let label = if cx.queue_mode { "Remove from queue" } else { "Remove from this playlist" };
        if menu_item(ui, icon::TRASH, label) {
            let mut desc = rows.to_vec();
            desc.sort_unstable_by(|a, b| b.cmp(a));
            for i in desc {
                ev.push(if cx.queue_mode { TrackEvent::RemoveFromQueue(i) } else { TrackEvent::RemoveFromPlaylist(i) });
            }
            ev.push(TrackEvent::ClearSelection);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn track_row(ui: &mut Ui, id: egui::Id, list: &str, i: usize, t: &Track, tracks: &[Track], selected: &HashSet<usize>, cx: &TableCtx, ev: &mut Vec<TrackEvent>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    // Rows just off screen are registered too, so Tab can reach them
    // (focusing one then scrolls it into view, see `focus_ring`).
    let resp = ui.interact(rect, id, Sense::click());
    a11y(ui, &resp, WidgetType::Button, || match crate::fixes::shown_length(t) {
        0 => format!("{} by {}", t.title, t.all_artists()),
        n => format!("{} by {}, {}", t.title, t.all_artists(), fmt_duration(n)),
    });
    // The song you came here for is scrolled into view once, even from far down the list.
    if let Some((hid, at)) = cx.highlight
        && hid == t.id
    {
        let key = egui::Id::new(("highlight_scrolled", hid, at.to_bits()));
        if !ui.ctx().data(|d| d.get_temp::<bool>(key).unwrap_or(false)) {
            resp.scroll_to_me(Some(egui::Align::Center));
            ui.ctx().data_mut(|d| d.insert_temp(key, true));
        }
    }
    if !ui.is_rect_visible(rect) {
        return;
    }
    let is_current = cx.now_playing == Some(t.id.as_str());
    // The heart, artist and album links sit on top of the row, so "the row is hovered"
    // is "the mouse is anywhere over it" (else the heart vanishes when you point at it).
    let hovered = resp.hovered() || resp.context_menu_opened() || resp.has_focus() || ui.rect_contains_pointer(rect);
    // Resting on a row for a moment: get the song ready (most clicks come right after).
    if hovered && !is_current {
        let key = egui::Id::new("row_hover_since");
        let now = ui.input(|i| i.time);
        let (row, since) = ui.ctx().data(|d| d.get_temp::<(egui::Id, f64)>(key)).unwrap_or((egui::Id::NULL, now));
        if row != id {
            ui.ctx().data_mut(|d| d.insert_temp(key, (id, now)));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(220));
        } else if now - since > 0.2 {
            // Sent while the pointer stays; the app starts it once.
            ev.push(TrackEvent::Warm(t.clone()));
        }
    }
    let style = &parts().track_row;
    let radius = style.radius.map(CornerRadius::same).unwrap_or(r_md());
    let is_selected = selected.contains(&i);
    if is_selected {
        ui.painter().rect_filled(rect, radius, pal().surface_active);
        if hovered {
            ui.painter().rect_filled(rect, radius, Color32::from_white_alpha(6));
        }
    } else if hovered {
        ui.painter().rect_filled(rect, radius, style.fill.unwrap_or(pal().surface_hover));
    } else if is_current {
        ui.painter().rect_filled(rect, radius, pal().accent.gamma_multiply(0.08));
    }
    if let Some(b) = style.border {
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, b);
    }
    if let Some((hid, _)) = cx.highlight
        && hid == t.id
    {
        // The song you came here for stays tinted for a moment.
        ui.painter().rect_filled(rect, radius, pal().accent.gamma_multiply(0.22));
    }
    let c = columns(rect, cx.added.is_some(), cx.views.is_some());

    // Number / play icon / "now playing" bars
    if hovered {
        let glyph = if is_current && cx.is_playing { icon::PAUSE } else { icon::PLAY };
        paint_icon(ui.painter(), c.num.center(), glyph, filled_icon(15.0), pal().text);
    } else if is_current && cx.is_playing {
        equalizer(ui, c.num.center());
    } else {
        let col = if is_current { pal().accent_soft } else { pal().text_muted };
        paint_icon(ui.painter(), c.num.center(), (i + 1 + cx.index_offset).to_string(), med(13.0), col);
    }

    // Art + title + artist
    let art = Rect::from_min_size(pos2(c.title.left(), rect.center().y - 20.0), Vec2::splat(40.0));
    // Ultra-low resource: no pictures in long lists (a plain tile instead).
    let pic = if crate::perf::on(crate::perf::Extra::ListArtwork) { t.art_small() } else { None };
    artwork(ui, art, pic, CornerRadius::same(5));
    let tx = art.right() + 12.0;
    let tw = c.title.right() - tx;
    let title_col = if is_current { pal().accent_soft } else { pal().text };
    let title_link = link_text(ui, id.with("title"), pos2(tx, rect.center().y - 17.0), &t.title, med(13.5), title_col, tw);
    let mut artist_x = tx;
    if cx.downloads.contains_key(&t.id) {
        paint_icon(ui.painter(), pos2(tx + 7.0, rect.center().y + 11.0), icon::ARROW_CIRCLE_DOWN, filled_icon(14.0), pal().accent_soft);
        artist_x += 19.0;
    }
    if t.explicit {
        // "E" badge for explicit lyrics, like other music apps.
        let badge = Rect::from_min_size(pos2(artist_x, rect.center().y + 4.0), vec2(14.0, 14.0));
        ui.painter().rect_filled(badge, CornerRadius::same(3), pal().text_muted);
        paint_icon(ui.painter(), badge.center(), "E", semi(9.5), pal().bg);
        artist_x += 19.0;
    }
    let (artist, featured_click) = artist_links(ui, id, pos2(artist_x, rect.center().y + 3.0), t, regular(12.5), pal().text_muted, tw - (artist_x - tx));
    if let Some(a) = featured_click {
        ev.push(TrackEvent::OpenArtistRef(a));
    }

    let mut album_link = None;
    if let (Some(a), Some(al)) = (c.album, t.album.as_ref()) {
        album_link = Some(link_text(
            ui,
            id.with("album"),
            pos2(a.left(), a.center().y - 8.0),
            &al.title,
            regular(12.5),
            pal().text_muted,
            a.width(),
        ));
    }

    if let (Some(v), Some((n, _))) = (c.views, cx.views.and_then(|m| m.get(&t.id))) {
        ui.painter().text(v.right_center(), Align2::RIGHT_CENTER, fmt_thousands(*n), regular(12.5), pal().text_muted);
    }
    if let (Some(d), Some(when)) = (c.date, cx.added.and_then(|a| a.get(&t.id))) {
        let text = fmt_added(*when, crate::app::now_unix());
        paint_line(ui, pos2(d.left(), d.center().y - 8.0), tr(&text), regular(12.5), pal().text_muted, d.width());
    }

    // Heart
    let liked = cx.liked.contains(&t.id);
    let heart = ui.interact(Rect::from_center_size(c.like.center(), Vec2::splat(28.0)), id.with("like"), Sense::click());
    resp_selected(ui, &heart, liked, "Like");
    if liked || hovered || heart.has_focus() {
        let (font, col) = if liked {
            (filled_icon(16.0), pal().accent_soft)
        } else {
            (regular(16.0), if heart.hovered() { pal().text } else { pal().text_muted })
        };
        paint_icon(ui.painter(), c.like.center(), icon::HEART, font, col);
    }
    // A song whose length isn't known yet (Deezer lists only a clip) shows none.
    let length = crate::fixes::shown_length(t);
    if length > 0 {
        ui.painter().text(c.time.right_center(), Align2::RIGHT_CENTER, fmt_duration(length), regular(12.5), pal().text_muted);
    }

    // Interactions
    // Resting on the title (which opens the song's album or page) loads that page ahead.
    if hover_intent(ui, title_link.id, title_link.hovered()) {
        ev.push(TrackEvent::Preview(t.clone()));
    }
    if heart.clicked() {
        ev.push(TrackEvent::ToggleLike(t.clone()));
    } else if title_link.clicked() {
        ev.push(TrackEvent::OpenSong(t.clone()));
    } else if artist.clicked() {
        ev.push(TrackEvent::OpenArtist(t.clone()));
    } else if album_link.as_ref().is_some_and(|r| r.clicked()) {
        ev.push(TrackEvent::OpenAlbum(t.album.as_ref().map(|a| a.id.clone()).unwrap_or_default()));
    } else if resp.double_clicked() {
        ev.push(TrackEvent::Play(i));
    } else if resp.clicked() {
        let (enter, mods) = ui.input(|inp| (inp.key_pressed(Key::Enter), inp.modifiers));
        if enter || pointer_in(ui, c.num) {
            // Enter on a focused row, or the play icon in the number column.
            ev.push(TrackEvent::Play(i));
        } else {
            // A click selects, like Spotify (Ctrl: add/remove, Shift: a range).
            ev.push(TrackEvent::Select { list: list.to_string(), index: i, ctrl: mods.command, shift: mods.shift });
        }
    }
    if resp.secondary_clicked() && !is_selected {
        // Right-clicking a song outside the selection selects just that song.
        ev.push(TrackEvent::Select { list: list.to_string(), index: i, ctrl: false, shift: false });
    }
    // In the queue, Alt+↑/↓ moves the focused song and Delete removes it.
    if resp.has_focus() && cx.queue_mode {
        if ui.input_mut(|inp| inp.consume_key(Modifiers::ALT, Key::ArrowUp)) && i > 0 {
            ev.push(TrackEvent::MoveInQueue(i, -1));
        }
        if ui.input_mut(|inp| inp.consume_key(Modifiers::ALT, Key::ArrowDown)) {
            ev.push(TrackEvent::MoveInQueue(i, 1));
        }
        if ui.input_mut(|inp| inp.consume_key(Modifiers::NONE, Key::Delete)) {
            ev.push(TrackEvent::RemoveFromQueue(i));
        }
    }
    // Keyboard equivalents of the right-click menu for the focused row.
    if resp.has_focus() && !cx.queue_mode {
        let plain = |key| ui.input_mut(|i| i.modifiers.is_none() && i.consume_key(Modifiers::NONE, key));
        if plain(Key::Q) {
            ev.push(TrackEvent::AddToQueue(t.clone()));
        }
        if plain(Key::N) {
            ev.push(TrackEvent::PlayNext(t.clone()));
        }
    }

    let group: Vec<usize> = if is_selected && selected.len() > 1 {
        let mut v: Vec<usize> = selected.iter().copied().collect();
        v.sort_unstable();
        v
    } else {
        vec![]
    };
    resp.context_menu(|ui| {
        if !group.is_empty() {
            group_menu(ui, &group, tracks, cx, ev);
            return;
        }
        let mcx = MenuCtx {
            liked,
            playlists: cx.playlists,
            index: Some(i),
            removable: cx.removable,
            queue_mode: cx.queue_mode,
            downloaded: cx.downloads.contains_key(&t.id),
        };
        track_menu(ui, t, &mcx, ev);
    });
}

fn pointer_in(ui: &Ui, r: Rect) -> bool {
    ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| r.contains(p))
}

/// Screen-reader name + on/off state for a toggle (like, shuffle, repeat...).
pub fn resp_selected(ui: &Ui, resp: &Response, on: bool, label: &str) {
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, label));
    focus_ring(ui, resp);
}

// ---------------------------------------------------------------------------
// Menus: rounded rows with a fixed icon column, soft hover, and a quick fade-in.
// ---------------------------------------------------------------------------

/// Call first inside every menu: tightens the spacing and fades the menu in.
pub fn menu_begin(ui: &mut Ui) {
    ui.spacing_mut().item_spacing.y = 1.0;
    ui.set_min_width(210.0);
    let id = ui.id().with("menu_fade");
    let now = ui.input(|i| i.time);
    let frame = ui.ctx().cumulative_frame_nr();
    // A menu that wasn't drawn last frame has just opened.
    let (start, last) = ui.data(|d| d.get_temp::<(f64, u64)>(id)).unwrap_or((now, 0));
    let start = if last + 1 < frame { now } else { start };
    ui.data_mut(|d| d.insert_temp(id, (start, frame)));
    let t = ((now - start) / 0.12).clamp(0.0, 1.0) as f32;
    if t < 1.0 && !crate::theme::reduce_motion() {
        ui.multiply_opacity(1.0 - (1.0 - t).powi(3));
        ui.ctx().request_repaint();
    }
}

/// Words that mark an item as destructive (shown in red).
fn is_danger(label: &str) -> bool {
    ["Delete", "Remove"].iter().any(|w| label.starts_with(w))
}

/// One menu row. `trailing` is drawn at the right (a caret for submenus).
fn menu_row(ui: &mut Ui, glyph: &str, label: &str, trailing: Option<&str>, open: bool) -> Response {
    let danger = is_danger(label);
    let label = tr(label);
    let font = med(13.0);
    let g = one_line(ui, label, font, pal().text, 360.0);
    let min_w = 44.0 + g.size().x + if trailing.is_some() { 36.0 } else { 18.0 };
    // In menus (justified), this fills the menu's width.
    let resp = ui.allocate_response(vec2(min_w, 32.0), Sense::click());
    let rect = resp.rect;
    a11y(ui, &resp, WidgetType::Button, || label.to_owned());
    let hot = resp.hovered() || resp.has_focus() || open;
    let t = ui.ctx().animate_bool_with_time(resp.id.with("hover"), hot, 0.08);
    if t > 0.0 {
        let fill = if danger { Color32::from_rgb(220, 70, 70).gamma_multiply(0.16) } else { pal().surface_hover.lerp_to_gamma(pal().accent, 0.10) };
        ui.painter().rect_filled(rect, r_sm().at_least(6), fill.gamma_multiply(t));
    }
    let base = if danger { Color32::from_rgb(235, 105, 105) } else { pal().text };
    let icon_col = if danger { base } else if hot { pal().accent_soft } else { pal().text_muted };
    // Icons sit in a fixed column, centered by their box, so they line up row to row.
    paint_icon(ui.painter(), pos2(rect.left() + 20.0, rect.center().y), glyph, regular(16.0), icon_col);
    let text_col = if hot || danger { base } else { pal().text.gamma_multiply(0.92) };
    ui.painter().galley(pos2(rect.left() + 40.0, rect.center().y - g.size().y / 2.0), g, text_col);
    if let Some(tr_glyph) = trailing {
        paint_icon(ui.painter(), pos2(rect.right() - 14.0, rect.center().y), tr_glyph, regular(12.0), pal().text_muted);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn menu_item(ui: &mut Ui, glyph: &str, label: &str) -> bool {
    let clicked = menu_row(ui, glyph, label, None, false).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// A row that opens a submenu to the right.
pub fn submenu(ui: &mut Ui, glyph: &str, label: &str, add: impl FnOnce(&mut Ui)) {
    let id = egui::containers::menu::SubMenu::id_from_widget_id(ui.next_auto_id());
    let open = egui::Popup::is_id_open(ui.ctx(), id);
    let resp = menu_row(ui, glyph, label, Some(icon::CARET_RIGHT), open);
    egui::containers::menu::SubMenu::new().show(ui, &resp, |ui| {
        menu_begin(ui);
        add(ui);
    });
}

/// A thin divider between groups of menu items.
pub fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(1.0), 9.0), Sense::hover());
    let w = ui.min_rect().width().max(rect.width());
    let x = ui.min_rect().left();
    ui.painter().hline(x + 8.0..=x + w - 8.0, rect.center().y, Stroke::new(1.0, pal().border));
}

/// A small heading at the top of a menu.
pub fn menu_header(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(egui::RichText::new(tr(text)).font(semi(11.0)).color(pal().text_faint));
    });
    ui.add_space(2.0);
}

/// The "…" button that opens a menu below it.
pub fn dots_menu(ui: &mut Ui, size: f32, tip: &str, add: impl FnOnce(&mut Ui)) {
    let resp = icon_button(ui, icon::DOTS_THREE, size, pal().text_muted, tip);
    egui::Popup::menu(&resp).show(|ui| {
        menu_begin(ui);
        add(ui);
    });
}

/// Animated "now playing" bars.
pub fn equalizer(ui: &Ui, center: Pos2) {
    // With animations off (Gaming Mode, Reduce motion), or behind another window (a
    // game), the bars stand still.
    let moving = !crate::theme::reduce_motion() && ui.input(|i| i.focused);
    let t = if moving { ui.input(|i| i.time) as f32 } else { 0.7 };
    for k in 0..3 {
        let phase = t * (5.0 + k as f32 * 1.7) + k as f32 * 1.3;
        let h = 4.0 + (phase.sin() * 0.5 + 0.5) * 9.0;
        let x = center.x - 5.0 + k as f32 * 5.0;
        let r = Rect::from_min_max(pos2(x - 1.5, center.y + 6.5 - h), pos2(x + 1.5, center.y + 6.5));
        ui.painter().rect_filled(r, CornerRadius::same(1), pal().accent_soft);
    }
    if moving {
        // 15 times a second is smooth enough for three little bars.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(66));
    }
}

// ---------------------------------------------------------------------------
// Loading skeletons & messages
// ---------------------------------------------------------------------------

pub fn skeleton_rows(ui: &mut Ui, n: usize) {
    let t = ui.input(|i| i.time) as f32;
    let a = 0.55 + 0.45 * (t * 3.0).sin().abs();
    let col = pal().surface_hover.gamma_multiply(a);
    for k in 0..n {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
        let c = columns(rect, false, false);
        let art = Rect::from_min_size(pos2(c.title.left(), rect.center().y - 20.0), Vec2::splat(40.0));
        ui.painter().rect_filled(art, CornerRadius::same(5), col);
        let w1 = 140.0 + ((k * 37) % 90) as f32;
        ui.painter().rect_filled(Rect::from_min_size(pos2(art.right() + 12.0, rect.center().y - 13.0), vec2(w1, 10.0)), CornerRadius::same(4), col);
        ui.painter().rect_filled(Rect::from_min_size(pos2(art.right() + 12.0, rect.center().y + 5.0), vec2(w1 * 0.55, 8.0)), CornerRadius::same(4), col);
    }
    if !crate::theme::reduce_motion() && ui.input(|i| i.focused) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// Centered message with an icon, optional button. Returns true if the button was clicked.
pub fn empty_state(ui: &mut Ui, glyph: &str, title: &str, body: &str, button: Option<&str>) -> bool {
    let (title, body, button) = (tr(title), tr(body), button.map(tr));
    let mut clicked = false;
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(glyph).font(regular(44.0)).color(pal().text_faint));
        ui.add_space(8.0);
        ui.label(egui::RichText::new(title).font(semi(17.0)));
        ui.add_space(2.0);
        ui.label(egui::RichText::new(body).color(pal().text_muted));
        if let Some(b) = button {
            ui.add_space(12.0);
            clicked = ghost_button(ui, icon::ARROW_CLOCKWISE, b).clicked();
        }
    });
    clicked
}

pub fn section_title(ui: &mut Ui, title: &str, action: Option<&str>) -> bool {
    let (title, action) = (tr(title), action.map(tr));
    let mut clicked = false;
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).font(theme::heavy(20.0)));
        if let Some(a) = action {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.add(egui::Label::new(egui::RichText::new(a).font(semi(12.5)).color(pal().text_muted)).sense(Sense::click()));
                if r.hovered() {
                    ui.painter().hline(r.rect.x_range(), r.rect.bottom(), Stroke::new(1.0, pal().text_muted));
                }
                clicked = r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
            });
        }
    });
    ui.add_space(8.0);
    clicked
}

#[cfg(test)]
mod added_tests {
    #[test]
    fn date_added_reads_like_spotify() {
        let now = 1_794_200_000; // Nov 2026
        assert_eq!(super::fmt_added(now - 60, now), "Today");
        assert_eq!(super::fmt_added(now - 3 * 86_400, now), "3 days ago");
        assert_eq!(super::fmt_added(now - 15 * 86_400, now), "2 weeks ago");
        assert_eq!(super::fmt_added(1_700_000_000, now), "Nov 14, 2023");
        assert_eq!(super::fmt_thousands(5_003_515), "5,003,515");
        assert_eq!(super::fmt_thousands(739), "739");
    }
}
