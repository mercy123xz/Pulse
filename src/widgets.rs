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
pub fn artwork(ui: &Ui, rect: Rect, url: Option<&str>, radius: CornerRadius) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().rect_filled(rect, radius, pal().surface_active);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon::MUSIC_NOTES,
        regular(rect.height() * 0.38),
        pal().text_faint,
    );
    if let Some(url) = url {
        egui::Image::new(url)
            .corner_radius(radius)
            .show_loading_spinner(false)
            .paint_at(ui, rect);
    }
}

/// Vertical gradient (used behind page headers).
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
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, regular(size), c);
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
    let nudge = if playing { 0.0 } else { size * 0.04 };
    ui.painter().text(
        rect.center() + vec2(nudge, 0.0),
        Align2::CENTER_CENTER,
        glyph,
        filled_icon(size * 0.42),
        style.color.unwrap_or(Color32::WHITE),
    );
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
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, med(12.5), fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Outline button with icon + label ("Shuffle", "See all", ...).
pub fn ghost_button(ui: &mut Ui, glyph: &str, label: &str) -> Response {
    let label = tr(label);
    let text = if glyph.is_empty() { label.to_owned() } else { format!("{glyph}  {label}") };
    let g = ui.painter().layout_no_wrap(text.clone(), med(13.0), pal().text);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 28.0, 34.0), Sense::click());
    a11y(ui, &resp, WidgetType::Button, || label.to_owned());
    let stroke = if resp.hovered() { Stroke::new(1.0, pal().text_muted) } else { Stroke::new(1.0, pal().border) };
    ui.painter().rect(rect, CornerRadius::same(17), Color32::TRANSPARENT, stroke, egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, pal().text);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A text "link" (e.g. artist name) that underlines on hover.
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
}

pub enum CardClick {
    None,
    Open,
    Play,
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
    artwork(ui, art_rect, info.art, radius);

    let text_w = w - pad * 2.0;
    paint_line(ui, pos2(rect.left() + pad, art_rect.bottom() + 10.0), info.title, semi(13.5), pal().text, text_w);
    paint_line(ui, pos2(rect.left() + pad, art_rect.bottom() + 30.0), &info.subtitle, regular(12.0), pal().text_muted, text_w);

    // Floating play button appears on hover.
    let mut out = if resp.clicked() { CardClick::Open } else { CardClick::None };
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
        ui.painter().text(c + vec2(1.5, 0.0), Align2::CENTER_CENTER, icon::PLAY, filled_icon(17.0), pb.color.unwrap_or(Color32::WHITE).gamma_multiply(t));
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
    PlayNext(Track),
    AddToQueue(Track),
    ToggleLike(Track),
    AddToPlaylist(usize, Track),
    NewPlaylistWith(Track),
    RemoveFromPlaylist(usize),
    RemoveFromQueue(usize),
    OpenArtist(Track),
    OpenAlbum(String),
    ShowCredits(Track),
    Share(Track, ShareTarget),
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
    ui.set_min_width(220.0);
    if let Some(i) = cx.index
        && menu_item(ui, icon::PLAY, "Play")
    {
        ev.push(TrackEvent::Play(i));
    }
    ui.menu_button(format!("{}   {}", icon::PLUS, tr("Add to playlist")), |ui| {
        ui.set_min_width(180.0);
        if menu_item(ui, icon::PLUS, "New playlist…") {
            ev.push(TrackEvent::NewPlaylistWith(t.clone()));
        }
        if !cx.playlists.is_empty() {
            ui.separator();
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
    ui.separator();
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
    if menu_item(ui, icon::IDENTIFICATION_CARD, "View credits") {
        ev.push(TrackEvent::ShowCredits(t.clone()));
    }
    ui.separator();
    ui.menu_button(format!("{}   {}", icon::SHARE_FAT, tr("Share")), |ui| {
        ui.set_min_width(200.0);
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
        ui.separator();
        if menu_item(ui, icon::TRASH, "Remove from this playlist")
            && let Some(i) = cx.index
        {
            ev.push(TrackEvent::RemoveFromPlaylist(i));
        }
    }
    if cx.queue_mode {
        ui.separator();
        if menu_item(ui, icon::X, "Remove from queue")
            && let Some(i) = cx.index
        {
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
    /// Number shown for the first row (queue shows "next up" starting at 1).
    pub index_offset: usize,
}

const ROW_H: f32 = 56.0;

struct Cols {
    num: Rect,
    title: Rect,
    album: Option<Rect>,
    date: Option<Rect>,
    like: Rect,
    time: Rect,
}

fn columns(row: Rect, with_date: bool) -> Cols {
    let w = row.width();
    let x0 = row.left();
    let col = |a: f32, b: f32| Rect::from_x_y_ranges(a..=b, row.y_range());
    let num = col(x0, x0 + 44.0);
    let time = col(row.right() - 64.0, row.right() - 12.0);
    let like = col(time.left() - 40.0, time.left());
    let date = (with_date && w > 900.0).then(|| col(like.left() - 150.0, like.left() - 12.0));
    let wide = w > 720.0;
    let album_right = date.map(|d| d.left()).unwrap_or(like.left()) - 16.0;
    let album_w = (w * if date.is_some() { 0.24 } else { 0.3 }).min(320.0);
    let album = wide.then(|| col(album_right - album_w, album_right));
    let title_right = album.map(|a| a.left()).unwrap_or(like.left()) - 16.0;
    Cols { num, title: col(num.right() + 4.0, title_right), album, date, like, time }
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

pub fn track_table_header(ui: &mut Ui, with_date: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let c = columns(rect, with_date);
    let f = med(11.5);
    let p = ui.painter();
    p.text(c.num.center(), Align2::CENTER_CENTER, "#", f.clone(), pal().text_faint);
    p.text(c.title.left_center(), Align2::LEFT_CENTER, tr("Title").to_uppercase(), f.clone(), pal().text_faint);
    if let Some(a) = c.album {
        p.text(a.left_center(), Align2::LEFT_CENTER, tr("Album").to_uppercase(), f.clone(), pal().text_faint);
    }
    if let Some(d) = c.date {
        p.text(d.left_center(), Align2::LEFT_CENTER, tr("Date added").to_uppercase(), f.clone(), pal().text_faint);
    }
    p.text(c.time.right_center(), Align2::RIGHT_CENTER, icon::CLOCK_COUNTER_CLOCKWISE, regular(14.0), pal().text_faint);
    p.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, pal().border));
    ui.add_space(6.0);
}

pub fn track_table(ui: &mut Ui, id_salt: &str, tracks: &[Track], cx: &TableCtx, events: &mut Vec<TrackEvent>) {
    track_table_header(ui, cx.added.is_some());
    let base = ui.id().with(id_salt);
    ui.spacing_mut().item_spacing.y = 0.0;
    for (i, t) in tracks.iter().enumerate() {
        track_row(ui, base.with(i), i, t, cx, events);
    }
    ui.spacing_mut().item_spacing.y = 6.0;
}

fn track_row(ui: &mut Ui, id: egui::Id, i: usize, t: &Track, cx: &TableCtx, ev: &mut Vec<TrackEvent>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    // Register the row even when it's scrolled out of view, so Tab can reach it
    // (focusing it then scrolls it into view, see `focus_ring`).
    let resp = ui.interact(rect, id, Sense::click());
    a11y(ui, &resp, WidgetType::Button, || format!("{} by {}, {}", t.title, t.artist.name, fmt_duration(t.duration)));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let is_current = cx.now_playing == Some(t.id.as_str());
    // The heart, artist and album links sit on top of the row, so "the row is hovered"
    // is "the mouse is anywhere over it" (else the heart vanishes when you point at it).
    let hovered = resp.hovered() || resp.context_menu_opened() || resp.has_focus() || ui.rect_contains_pointer(rect);
    let style = &parts().track_row;
    let radius = style.radius.map(CornerRadius::same).unwrap_or(r_md());
    if hovered {
        ui.painter().rect_filled(rect, radius, style.fill.unwrap_or(pal().surface_hover));
    } else if is_current {
        ui.painter().rect_filled(rect, radius, pal().accent.gamma_multiply(0.08));
    }
    if let Some(b) = style.border {
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, b);
    }
    let c = columns(rect, cx.added.is_some());

    // Number / play icon / "now playing" bars
    if hovered {
        let glyph = if is_current && cx.is_playing { icon::PAUSE } else { icon::PLAY };
        ui.painter().text(c.num.center(), Align2::CENTER_CENTER, glyph, filled_icon(15.0), pal().text);
    } else if is_current && cx.is_playing {
        equalizer(ui, c.num.center());
    } else {
        let col = if is_current { pal().accent_soft } else { pal().text_muted };
        ui.painter().text(c.num.center(), Align2::CENTER_CENTER, (i + 1 + cx.index_offset).to_string(), med(13.0), col);
    }

    // Art + title + artist
    let art = Rect::from_min_size(pos2(c.title.left(), rect.center().y - 20.0), Vec2::splat(40.0));
    artwork(ui, art, t.art_small(), CornerRadius::same(5));
    let tx = art.right() + 12.0;
    let tw = c.title.right() - tx;
    let title_col = if is_current { pal().accent_soft } else { pal().text };
    paint_line(ui, pos2(tx, rect.center().y - 17.0), &t.title, med(13.5), title_col, tw);
    let mut artist_x = tx;
    if cx.downloads.contains_key(&t.id) {
        ui.painter().text(pos2(tx + 7.0, rect.center().y + 11.0), Align2::CENTER_CENTER, icon::ARROW_CIRCLE_DOWN, filled_icon(14.0), pal().accent_soft);
        artist_x += 19.0;
    }
    if t.explicit {
        // "E" badge for explicit lyrics, like other music apps.
        let badge = Rect::from_min_size(pos2(artist_x, rect.center().y + 4.0), vec2(14.0, 14.0));
        ui.painter().rect_filled(badge, CornerRadius::same(3), pal().text_muted);
        ui.painter().text(badge.center(), Align2::CENTER_CENTER, "E", semi(9.5), pal().bg);
        artist_x += 19.0;
    }
    let artist = link_text(
        ui,
        id.with("artist"),
        pos2(artist_x, rect.center().y + 3.0),
        &t.artist.name,
        regular(12.5),
        pal().text_muted,
        tw - (artist_x - tx),
    );

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
        ui.painter().text(c.like.center(), Align2::CENTER_CENTER, icon::HEART, font, col);
    }
    ui.painter().text(c.time.right_center(), Align2::RIGHT_CENTER, fmt_duration(t.duration), regular(12.5), pal().text_muted);

    // Interactions
    if heart.clicked() {
        ev.push(TrackEvent::ToggleLike(t.clone()));
    } else if artist.clicked() {
        ev.push(TrackEvent::OpenArtist(t.clone()));
    } else if album_link.as_ref().is_some_and(|r| r.clicked()) {
        ev.push(TrackEvent::OpenAlbum(t.album.as_ref().map(|a| a.id.clone()).unwrap_or_default()));
    } else if resp.double_clicked() || resp.clicked() && (resp.has_focus() || pointer_in(ui, c.num)) {
        // Single click plays only on the number column; Enter plays when the row has keyboard focus.
        ev.push(TrackEvent::Play(i));
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

    resp.context_menu(|ui| {
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

pub fn menu_item(ui: &mut Ui, glyph: &str, label: &str) -> bool {
    let label = tr(label);
    let clicked = ui.add(egui::Button::new(format!("{glyph}   {label}")).frame_when_inactive(false)).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// Animated "now playing" bars.
pub fn equalizer(ui: &Ui, center: Pos2) {
    let t = ui.input(|i| i.time) as f32;
    for k in 0..3 {
        let phase = t * (5.0 + k as f32 * 1.7) + k as f32 * 1.3;
        let h = 4.0 + (phase.sin() * 0.5 + 0.5) * 9.0;
        let x = center.x - 5.0 + k as f32 * 5.0;
        let r = Rect::from_min_max(pos2(x - 1.5, center.y + 6.5 - h), pos2(x + 1.5, center.y + 6.5));
        ui.painter().rect_filled(r, CornerRadius::same(1), pal().accent_soft);
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
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
        let c = columns(rect, false);
        let art = Rect::from_min_size(pos2(c.title.left(), rect.center().y - 20.0), Vec2::splat(40.0));
        ui.painter().rect_filled(art, CornerRadius::same(5), col);
        let w1 = 140.0 + ((k * 37) % 90) as f32;
        ui.painter().rect_filled(Rect::from_min_size(pos2(art.right() + 12.0, rect.center().y - 13.0), vec2(w1, 10.0)), CornerRadius::same(4), col);
        ui.painter().rect_filled(Rect::from_min_size(pos2(art.right() + 12.0, rect.center().y + 5.0), vec2(w1 * 0.55, 8.0)), CornerRadius::same(4), col);
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
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
    }
}
