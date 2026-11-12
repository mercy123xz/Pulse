//! Lyrics: a full page, and a side panel that sits beside any page while you browse.
//! The line being sung is highlighted and kept in view; click a line to jump there.

use crate::app::{App, LyricsState, Route};
use crate::deezer::Track;
use crate::i18n::tr;
use crate::theme::*;
use crate::widgets::*;
use egui::{Align, CornerRadius, Layout, RichText, Sense, Ui, vec2};
use egui_phosphor::regular as icon;

/// Narrower windows show lyrics as a page instead of a panel (the panel would crowd it).
pub const PANEL_MIN_WINDOW: f32 = 1000.0;
pub const PANEL_WIDTH: f32 = 320.0;

/// The lyrics button and L: the side panel on wide windows, else the lyrics page.
pub fn toggle(app: &mut App, ctx: &egui::Context) {
    if app.route == Route::Lyrics {
        app.go_back(ctx);
    } else if ctx.content_rect().width() >= PANEL_MIN_WINDOW {
        app.saved.settings.lyrics_panel = !app.saved.settings.lyrics_panel;
    } else {
        app.navigate(Route::Lyrics, ctx);
    }
}

/// Whether the side panel is showing.
pub fn panel_open(app: &App, ctx: &egui::Context) -> bool {
    app.saved.settings.lyrics_panel && app.route != Route::Lyrics && ctx.content_rect().width() >= PANEL_MIN_WINDOW
}

pub fn lyrics_page(app: &mut App, ui: &mut Ui) {
    let Some(t) = app.current.clone() else {
        empty_state(ui, icon::MICROPHONE_STAGE, "Nothing playing", "Play a song to see its lyrics here.", None);
        return;
    };

    // Header: the song and the text options.
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
        artwork(ui, r, t.art_small(), r_sm());
        ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(&t.title).font(heavy(20.0)).color(pal().text));
            ui.label(RichText::new(t.all_artists()).font(regular(13.0)).color(pal().text_muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            text_options(app, ui);
        });
    });
    ui.add_space(14.0);
    let size = [17.0, 22.0, 28.0][app.saved.settings.lyrics_size.min(2) as usize];
    lyrics_body(app, ui, &t, size, "page", true);
}

/// The panel beside the page: a small header, then the lyrics in their own scroll area.
pub fn lyrics_panel(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Lyrics")).font(heavy(17.0)).color(pal().text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon_button(ui, icon::X, 16.0, pal().text_muted, tr("Close lyrics (L)")).clicked() {
                app.saved.settings.lyrics_panel = false;
            }
            if icon_button(ui, icon::ARROWS_OUT_SIMPLE, 16.0, pal().text_muted, tr("Open the lyrics page")).clicked() {
                app.navigate(Route::Lyrics, &ctx);
            }
        });
    });
    let Some(t) = app.current.clone() else {
        ui.add_space(24.0);
        ui.label(RichText::new(tr("Play a song to see its lyrics here.")).font(regular(13.0)).color(pal().text_muted));
        return;
    };
    // The song, then the text options.
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
        artwork(ui, r, t.art_small(), CornerRadius::same(6));
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(&t.title).font(semi(13.5)).color(pal().text)).truncate());
            ui.add(egui::Label::new(RichText::new(t.all_artists()).font(regular(12.0)).color(pal().text_muted)).truncate());
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| text_options(app, ui));
    ui.add_space(6.0);
    ui.separator();

    // If you scroll the lyrics yourself, they stop following the song for a few seconds.
    let panel = ui.max_rect();
    let user_key = egui::Id::new("lyrics_panel_user_scroll");
    let now = ui.input(|i| i.time);
    if ui.rect_contains_pointer(panel) && ui.input(|i| i.smooth_scroll_delta.y != 0.0) {
        ui.data_mut(|d| d.insert_temp(user_key, now));
    }
    let last_user: f64 = ui.data(|d| d.get_temp(user_key)).unwrap_or(f64::MIN);
    let follow = now - last_user > 4.0;
    if !follow {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64((4.05 - (now - last_user)).max(0.05)));
    }
    let size = [14.5, 16.5, 20.0][app.saved.settings.lyrics_size.min(2) as usize];
    egui::ScrollArea::vertical().id_salt("lyrics_panel_scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(8.0);
        lyrics_body(app, ui, &t, size, "panel", follow);
        ui.add_space(40.0);
    });
}

/// Text size (A−/A/A+) and alignment.
fn text_options(app: &mut App, ui: &mut Ui) {
    // Laid out right to left (the page header), the same order needs drawing backwards.
    let rtl = ui.layout().prefer_right_to_left();
    let s = &mut app.saved.settings;
    let mut sizes = [(0u8, "A−"), (1, "A"), (2, "A+")];
    let align = |ui: &mut Ui, s: &mut crate::settings::Settings| {
        let align_icon = if s.lyrics_center { icon::TEXT_ALIGN_CENTER } else { icon::TEXT_ALIGN_LEFT };
        if icon_button(ui, align_icon, 17.0, pal().text_muted, tr("Center or left-align")).clicked() {
            s.lyrics_center = !s.lyrics_center;
        }
    };
    if rtl {
        sizes.reverse();
        align(ui, s);
    }
    for (n, label) in sizes {
        if pill(ui, label, s.lyrics_size == n).on_hover_text(tr("Text size")).clicked() {
            s.lyrics_size = n;
        }
    }
    if !rtl {
        align(ui, s);
    }
}

/// The lyrics themselves (or why there are none). `place` keeps the page's and the
/// panel's scroll positions apart; `follow` keeps the sung line in view.
fn lyrics_body(app: &mut App, ui: &mut Ui, t: &Track, size: f32, place: &str, follow: bool) {
    let ctx = ui.ctx().clone();
    let compact = place == "panel";
    let state = match app.lyrics_for(t, &ctx) {
        LyricsState::Loading => None,
        LyricsState::Ready(l) => Some(Ok(l.clone())),
        LyricsState::Missing => Some(Err(String::new())),
        LyricsState::Failed(e) => Some(Err(e.clone())),
    };
    let lyr = match state {
        None => {
            if compact {
                ui.label(RichText::new(tr("Loading…")).font(regular(13.0)).color(pal().text_muted));
            } else {
                skeleton_rows(ui, 6);
            }
            return;
        }
        Some(Err(e)) if e.is_empty() => {
            notice(ui, compact, icon::MICROPHONE_STAGE, "No lyrics for this song yet", "LRCLIB doesn't have them yet.", None);
            return;
        }
        Some(Err(e)) => {
            if notice(ui, compact, icon::WIFI_SLASH, "Couldn't load the lyrics", &e, Some("Try again")) {
                app.lyrics.remove(&t.id);
            }
            return;
        }
        Some(Ok(l)) => l,
    };
    if lyr.instrumental && lyr.synced.is_empty() && lyr.plain.is_none() {
        notice(ui, compact, icon::MUSIC_NOTES, "Instrumental", "This song has no lyrics.", None);
        return;
    }

    let center = app.saved.settings.lyrics_center;
    let layout = if center { Layout::top_down(Align::Center) } else { Layout::top_down(Align::Min) };
    let wrap = |ui: &mut Ui, text: RichText| -> egui::Response { ui.add(egui::Label::new(text).wrap().sense(Sense::click())) };

    if lyr.synced.is_empty() {
        // Plain lyrics: no timing, so nothing to highlight.
        ui.with_layout(layout, |ui| {
            for line in lyr.plain.as_deref().unwrap_or("").lines() {
                wrap(ui, RichText::new(if line.is_empty() { " " } else { line }).font(semi(size * 0.85)).color(pal().text));
            }
        });
        ui.add_space(20.0);
        ui.with_layout(layout, source_note);
        return;
    }

    // Synced: highlight the line being sung.
    let pos = if app.needs_load { 0.0 } else { app.audio.position().as_secs_f32() };
    let current = lyr.line_at(pos);
    let scroll_key = egui::Id::new(("lyrics_scrolled", place, &t.id));
    let scrolled_to: Option<usize> = ui.data(|d| d.get_temp(scroll_key)).flatten();
    let mut seek = None;
    ui.with_layout(layout, |ui| {
        ui.spacing_mut().item_spacing.y = size * 0.45;
        for (i, (at, line)) in lyr.synced.iter().enumerate() {
            let text = if line.is_empty() { "♪" } else { line.as_str() };
            let col = match current {
                Some(c) if c == i => pal().text,
                Some(c) if i < c => pal().text_faint,
                _ => pal().text_muted,
            };
            let r = wrap(ui, RichText::new(text).font(heavy(size)).color(col)).on_hover_cursor(egui::CursorIcon::PointingHand);
            if r.clicked() {
                seek = Some(*at);
            }
            // Keep the sung line in the middle of the view (once per new line).
            if follow && current == Some(i) && scrolled_to != Some(i) {
                let anim = if reduce_motion() { egui::style::ScrollAnimation::none() } else { egui::style::ScrollAnimation::duration(0.3) };
                ui.scroll_to_rect_animation(r.rect, Some(Align::Center), anim);
                ui.data_mut(|d| d.insert_temp(scroll_key, Some(i)));
            }
        }
    });
    let length = app.current_length();
    if let Some(at) = seek
        && length > 0
    {
        app.seek_fraction((at / length as f32).clamp(0.0, 1.0));
        // Jumping is a choice to follow along from there.
        ui.data_mut(|d| d.remove::<f64>(egui::Id::new("lyrics_panel_user_scroll")));
    }
    // Wake up when the next line starts (not on a timer).
    if app.is_playing()
        && let Some(next) = lyr.next_change(pos)
    {
        ctx.request_repaint_after(std::time::Duration::from_secs_f32((next - pos).max(0.05)));
    }
    ui.add_space(30.0);
    ui.with_layout(layout, source_note);
}

/// An empty-state message: the big one on the page, a short one in the panel.
/// True when its button was clicked.
fn notice(ui: &mut Ui, compact: bool, glyph: &str, title: &str, body: &str, button: Option<&str>) -> bool {
    if !compact {
        return empty_state(ui, glyph, title, body, button);
    }
    ui.add_space(10.0);
    ui.label(RichText::new(tr(title)).font(semi(14.0)).color(pal().text));
    ui.add(egui::Label::new(RichText::new(tr(body)).font(regular(12.5)).color(pal().text_muted)).wrap());
    match button {
        Some(b) => {
            ui.add_space(6.0);
            ghost_button(ui, "", b).clicked()
        }
        None => false,
    }
}

fn source_note(ui: &mut Ui) {
    ui.add(egui::Label::new(RichText::new(tr("Lyrics from LRCLIB, a free, open lyrics database.")).font(regular(11.5)).color(pal().text_faint)).wrap());
}
