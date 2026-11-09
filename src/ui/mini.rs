//! The mini player: a small window that stays on top of games and other apps, with
//! the cover, the song, previous / play-pause / next and a thin progress line.
//!
//! Kept minimal on purpose: one fixed size, no animations, and it only redraws when
//! something changes or once a second while music plays.

use crate::app::App;
use crate::i18n::tr;
use crate::theme::*;
use crate::widgets::*;
use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2, ViewportBuilder, ViewportCommand, ViewportId, pos2, vec2};
use egui_phosphor::regular as icon;

const SIZE: [f32; 2] = [320.0, 72.0];

pub fn show(app: &mut App, ctx: &egui::Context) {
    let s = &app.saved.settings;
    if !s.mini_open {
        return;
    }
    // "Always on top" is set when the window is made: changing it makes a new one.
    let id = ViewportId::from_hash_of(("mini_player", s.mini_on_top));
    let mut b = ViewportBuilder::default()
        .with_title("Pulse mini player")
        .with_inner_size(SIZE)
        .with_resizable(false)
        .with_decorations(false)
        .with_taskbar(false);
    if let Some(p) = s.mini_pos {
        b = b.with_position(p);
    }
    if s.mini_on_top {
        b = b.with_always_on_top();
    }
    ctx.show_viewport_immediate(id, b, |ui, _| {
        if ui.input(|i| i.viewport().close_requested()) {
            app.saved.settings.mini_open = false;
            return;
        }
        // Remember where it was put, so it opens there next time.
        if let Some(o) = ui.input(|i| i.viewport().outer_rect) {
            app.saved.settings.mini_pos = Some([o.min.x, o.min.y]);
        }
        draw(app, ui);
    });
}

fn draw(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect();
    // Square corners: the window itself is square, so rounded ones would show its background.
    ui.painter().rect(full, CornerRadius::ZERO, pal().player, Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);

    // Drag anywhere to move it; double-click opens Pulse; right-click for options.
    let bg = ui.interact(full, ui.id().with("mini_bg"), Sense::click_and_drag());
    if bg.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if bg.double_clicked() {
        show_main(&ctx);
    }
    bg.context_menu(|ui| {
        menu_begin(ui);
        if menu_item(ui, icon::ARROW_SQUARE_OUT, "Open Pulse") {
            show_main(&ctx);
        }
        let on_top = app.saved.settings.mini_on_top;
        if menu_item(ui, if on_top { icon::CHECK } else { "" }, "Always on top") {
            app.saved.settings.mini_on_top = !on_top;
        }
        menu_separator(ui);
        if menu_item(ui, icon::X, "Close mini player") {
            app.saved.settings.mini_open = false;
        }
    });

    let Some(t) = app.current.clone() else {
        ui.painter().text(full.center(), Align2::CENTER_CENTER, tr("Nothing playing"), med(13.0), pal().text_muted);
        close_button(app, ui, full);
        return;
    };

    // Cover, then the song, then the three buttons at the right.
    let art = Rect::from_min_size(pos2(full.left() + 10.0, full.center().y - 26.0), Vec2::splat(52.0));
    artwork(ui, art, t.art_small(), CornerRadius::same(7));
    let controls_c = pos2(full.right() - 62.0, full.center().y);
    let tx = art.right() + 10.0;
    let tw = controls_c.x - 48.0 - tx;
    paint_line(ui, pos2(tx, full.center().y - 18.0), &t.title, semi(13.0), pal().text, tw);
    paint_line(ui, pos2(tx, full.center().y + 1.0), &t.all_artists(), regular(11.5), pal().text_muted, tw);

    let playing = app.is_playing();
    let button = |ui: &Ui, c: Pos2, glyph: &str, name: &str, main: bool| -> bool {
        let r = Rect::from_center_size(c, Vec2::splat(if main { 32.0 } else { 26.0 }));
        let resp = ui.interact(r, ui.id().with(("mini_btn", name)), Sense::click());
        a11y(ui, &resp, egui::WidgetType::Button, || name.to_owned());
        if main {
            ui.painter().circle_filled(c, 15.0, if resp.hovered() { pal().text.gamma_multiply(0.85) } else { pal().text });
            paint_icon(ui.painter(), c, glyph, filled_icon(14.0), pal().bg);
        } else {
            if resp.hovered() {
                ui.painter().circle_filled(c, 13.0, pal().surface_hover);
            }
            paint_icon(ui.painter(), c, glyph, filled_icon(14.0), pal().text);
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    };
    if button(ui, controls_c - vec2(34.0, 0.0), icon::SKIP_BACK, "Previous", false) {
        app.previous(&ctx);
    }
    if button(ui, controls_c, if playing { icon::PAUSE } else { icon::PLAY }, if playing { "Pause" } else { "Play" }, true) {
        app.toggle_play(&ctx);
    }
    if button(ui, controls_c + vec2(34.0, 0.0), icon::SKIP_FORWARD, "Next", false) {
        app.next(&ctx);
    }

    // A thin progress line along the bottom.
    let pos = if app.needs_load { 0.0 } else { app.audio.position().as_secs_f32() };
    let frac = (pos / t.duration.max(1) as f32).clamp(0.0, 1.0);
    let line = Rect::from_min_max(pos2(full.left() + 12.0, full.bottom() - 5.0), pos2(full.right() - 12.0, full.bottom() - 3.0));
    ui.painter().rect_filled(line, CornerRadius::same(1), pal().surface_active);
    ui.painter().rect_filled(Rect::from_min_max(line.min, pos2(line.left() + line.width() * frac, line.bottom())), CornerRadius::same(1), pal().accent_soft);

    close_button(app, ui, full);
}

/// A small × in the corner while the pointer is over the window.
fn close_button(app: &mut App, ui: &Ui, full: Rect) {
    if !ui.rect_contains_pointer(full) {
        return;
    }
    let r = Rect::from_center_size(pos2(full.right() - 11.0, full.top() + 11.0), Vec2::splat(16.0));
    let resp = ui.interact(r, ui.id().with("mini_close"), Sense::click());
    a11y(ui, &resp, egui::WidgetType::Button, || tr("Close mini player").to_owned());
    if resp.hovered() {
        ui.painter().circle_filled(r.center(), 8.0, pal().surface_active);
    }
    paint_icon(ui.painter(), r.center(), icon::X, regular(10.0), pal().text_muted);
    if resp.on_hover_text(tr("Close mini player")).clicked() {
        app.saved.settings.mini_open = false;
    }
}

fn show_main(ctx: &egui::Context) {
    ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Visible(true));
    ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
}
