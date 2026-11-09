//! The lyrics page: the playing song's lyrics, the line being sung highlighted and
//! kept in view. Click a line to jump there.

use crate::app::{App, LyricsState};
use crate::i18n::tr;
use crate::theme::*;
use crate::widgets::*;
use egui::{Align, Layout, RichText, Sense, Ui, vec2};
use egui_phosphor::regular as icon;

pub fn lyrics_page(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let Some(t) = app.current.clone() else {
        empty_state(ui, icon::MICROPHONE_STAGE, "Nothing playing", "Play a song to see its lyrics here.", None);
        return;
    };

    // Header: the song, the Genius link and the text options.
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
            if ghost_button(ui, icon::ARROW_SQUARE_OUT, "View on Genius").on_hover_text(tr("The song's page on Genius, with notes on what the lyrics mean")).clicked() {
                ctx.open_url(egui::OpenUrl::new_tab(crate::lyrics::genius_url(&t)));
            }
            ui.add_space(8.0);
            let s = &mut app.saved.settings;
            let align_icon = if s.lyrics_center { icon::TEXT_ALIGN_CENTER } else { icon::TEXT_ALIGN_LEFT };
            if icon_button(ui, align_icon, 17.0, pal().text_muted, tr("Center or left-align")).clicked() {
                s.lyrics_center = !s.lyrics_center;
            }
            for (n, label) in [(2u8, "A+"), (1, "A"), (0, "A−")] {
                if pill(ui, label, s.lyrics_size == n).on_hover_text(tr("Text size")).clicked() {
                    s.lyrics_size = n;
                }
            }
        });
    });
    ui.add_space(14.0);

    let state = match app.lyrics_for(&t, &ctx) {
        LyricsState::Loading => None,
        LyricsState::Ready(l) => Some(Ok(l.clone())),
        LyricsState::Missing => Some(Err(String::new())),
        LyricsState::Failed(e) => Some(Err(e.clone())),
    };
    let lyr = match state {
        None => {
            skeleton_rows(ui, 6);
            return;
        }
        Some(Err(e)) if e.is_empty() => {
            empty_state(ui, icon::MICROPHONE_STAGE, "No lyrics for this song yet", "LRCLIB doesn't have them. You can still read them on Genius.", None);
            return;
        }
        Some(Err(e)) => {
            if empty_state(ui, icon::WIFI_SLASH, "Couldn't load the lyrics", &e, Some("Try again")) {
                app.lyrics.remove(&t.id);
            }
            return;
        }
        Some(Ok(l)) => l,
    };
    if lyr.instrumental && lyr.synced.is_empty() && lyr.plain.is_none() {
        empty_state(ui, icon::MUSIC_NOTES, "Instrumental", "This song has no lyrics.", None);
        return;
    }

    let s = &app.saved.settings;
    let size = [17.0, 22.0, 28.0][s.lyrics_size.min(2) as usize];
    let center = s.lyrics_center;
    let layout = if center { Layout::top_down(Align::Center) } else { Layout::top_down(Align::Min) };

    if lyr.synced.is_empty() {
        // Plain lyrics: no timing, so nothing to highlight.
        ui.with_layout(layout, |ui| {
            for line in lyr.plain.as_deref().unwrap_or("").lines() {
                ui.label(RichText::new(if line.is_empty() { " " } else { line }).font(semi(size * 0.85)).color(pal().text));
            }
        });
        ui.add_space(20.0);
        ui.with_layout(layout, source_note);
        return;
    }

    // Synced: highlight the line being sung.
    let pos = if app.needs_load { 0.0 } else { app.audio.position().as_secs_f32() };
    let current = lyr.line_at(pos);
    let scroll_key = egui::Id::new(("lyrics_scrolled", &t.id));
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
            let r = ui.add(egui::Label::new(RichText::new(text).font(heavy(size)).color(col)).sense(Sense::click()));
            let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
            if r.clicked() {
                seek = Some(*at);
            }
            // Keep the sung line in the middle of the view (once per new line).
            if current == Some(i) && scrolled_to != Some(i) {
                let anim = if reduce_motion() { egui::style::ScrollAnimation::none() } else { egui::style::ScrollAnimation::duration(0.3) };
                ui.scroll_to_rect_animation(r.rect, Some(Align::Center), anim);
                ui.data_mut(|d| d.insert_temp(scroll_key, Some(i)));
            }
        }
    });
    if let Some(at) = seek
        && t.duration > 0
    {
        app.seek_fraction((at / t.duration as f32).clamp(0.0, 1.0));
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

fn source_note(ui: &mut Ui) {
    ui.label(RichText::new(tr("Lyrics from LRCLIB, a free, open lyrics database.")).font(regular(11.5)).color(pal().text_faint));
}
