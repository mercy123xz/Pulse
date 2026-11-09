//! Everything you see: sidebar, top bar, pages, the player bar and pop-ups.

use crate::app::{App, Page, Remote, Repeat, Route};
use crate::deezer::{AlbumInfo, ArtistInfo, PlaylistInfo, Track};
use crate::audio::LoadState;
use crate::theme::*;
use crate::widgets::*;
use crate::i18n::tr;

mod personal;
mod settings_page;
mod song_page;

pub use personal::ProfileForm;
use egui::{Align, Align2, Color32, CornerRadius, Frame, Key, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use egui_phosphor::regular as icon;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    shortcuts(app, &ctx);
    let lay = *layout();
    let pt = parts();

    let player_fill = pt.player_bar.fill.unwrap_or(pal().player);
    let player = egui::Panel::bottom("player")
        .exact_size(88.0)
        .frame(Frame::new().fill(player_fill).inner_margin(Margin::symmetric(pt.player_bar.padding.unwrap_or(16.0) as i8, 0)))
        .show_separator_line(false)
        .show(ui, |ui| player_bar(app, ui));
    let top_line = pt.player_bar.border.unwrap_or(Stroke::new(1.0, pal().border));
    ui.painter().hline(player.response.rect.x_range(), player.response.rect.top(), top_line);

    let compact = lay.sidebar == crate::theme::SidebarStyle::Icons;
    let sidebar_fill = pt.sidebar.fill.unwrap_or(pal().sidebar);
    let pad = pt.sidebar.padding.unwrap_or(12.0) as i8;
    let side = egui::Panel::left("sidebar")
        .exact_size(if compact { 76.0 } else { 240.0 })
        .resizable(false)
        .frame(Frame::new().fill(sidebar_fill).inner_margin(Margin { left: pad, right: pad, top: 18, bottom: 12 }))
        .show_separator_line(false)
        .show(ui, |ui| sidebar(app, ui));
    if let Some(b) = pt.sidebar.border {
        ui.painter().vline(side.response.rect.right(), side.response.rect.y_range(), b);
    }

    let header_fill = pt.header.fill.unwrap_or(pal().bg);
    if lay.inset {
        // The main area floats as a rounded panel; the window behind it uses the sidebar color.
        egui::CentralPanel::default().frame(Frame::new().fill(sidebar_fill).inner_margin(Margin { left: 0, right: 8, top: 8, bottom: 8 })).show(ui, |ui| {
            Frame::new().fill(pal().bg).corner_radius(r_lg()).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                Frame::new().fill(pt.header.fill.unwrap_or(Color32::TRANSPARENT)).corner_radius(r_lg()).inner_margin(Margin::symmetric(24, 0)).show(ui, |ui| {
                    ui.set_height(60.0);
                    ui.set_width(ui.available_width());
                    top_bar(app, ui);
                });
                main_scroll(app, ui);
            });
        });
    } else {
        egui::Panel::top("topbar")
            .exact_size(60.0)
            .frame(Frame::new().fill(header_fill).inner_margin(Margin::symmetric(24, 0)))
            .show_separator_line(false)
            .show(ui, |ui| top_bar(app, ui));
        egui::CentralPanel::default().frame(Frame::new().fill(pal().bg)).show(ui, |ui| main_scroll(app, ui));
    }

    dialogs(app, &ctx);
    toast(app, &ctx);
}

/// What the player bar says while a song loads.
fn loading_text(msg: &str) -> String {
    match msg.rsplit_once(' ').filter(|(_, pct)| pct.ends_with('%')) {
        Some((_, pct)) => format!("{} {pct}", tr("Loading…")),
        None => tr("Loading…").to_string(),
    }
}

fn main_scroll(app: &mut App, ui: &mut Ui) {
    egui::ScrollArea::vertical()
        .id_salt(("page", format!("{:?}", app.route)))
        .auto_shrink(false)
        .show(ui, |ui| {
            Frame::new().inner_margin(Margin { left: 28, right: 28, top: 4, bottom: 32 }).show(ui, |ui| {
                page(app, ui);
            });
        });
}

fn shortcuts(app: &mut App, ctx: &egui::Context) {
    use egui::Modifiers;
    let typing = ctx.egui_wants_keyboard_input();
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::K)) {
        app.focus_search = true;
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::ArrowRight)) {
        app.next(ctx);
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::ArrowLeft)) {
        app.previous(ctx);
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::ALT, Key::ArrowLeft)) {
        app.go_back(ctx);
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::ALT, Key::ArrowRight)) {
        app.go_forward(ctx);
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::ArrowUp)) {
        app.set_volume(app.saved.volume + 0.1);
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::ArrowDown)) {
        app.set_volume(app.saved.volume - 0.1);
    }
    if !typing {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space)) {
            app.toggle_play(ctx);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::S)) {
            app.toggle_shuffle();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::R)) {
            app.cycle_repeat();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::M)) {
            app.toggle_mute();
        }
    }
    // Mouse "back" / "forward" side buttons
    if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Extra1)) {
        app.go_back(ctx);
    }
    if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Extra2)) {
        app.go_forward(ctx);
    }
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

fn sidebar(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();

    // Logo
    let compact = compact_sidebar();
    if compact {
        // Centred exactly in the narrow sidebar.
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let r = Rect::from_center_size(row.center(), Vec2::splat(30.0));
        vgradient_rounded(ui, r, pal().accent_soft, pal().accent);
        ui.painter().text(r.center(), Align2::CENTER_CENTER, icon::WAVEFORM, filled_icon(18.0), Color32::WHITE);
    } else {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
            vgradient_rounded(ui, r, pal().accent_soft, pal().accent);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, icon::WAVEFORM, filled_icon(18.0), Color32::WHITE);
            ui.add_space(4.0);
            ui.label(RichText::new("Pulse").font(heavy(19.0)));
        });
    }
    ui.add_space(18.0);

    nav_heading(ui, "Browse");
    if nav_item(ui, icon::HOUSE, "Home", app.route == Route::Home, None).clicked() {
        app.navigate(Route::Home, &ctx);
    }
    if nav_item(ui, icon::FIRE, "Charts", app.route == Route::Charts, None).clicked() {
        app.navigate(Route::Charts, &ctx);
    }
    let searching = matches!(app.route, Route::Search(_));
    if nav_item(ui, icon::MAGNIFYING_GLASS, "Search", searching, None).clicked() {
        app.focus_search = true;
    }

    ui.add_space(14.0);
    nav_heading(ui, "Library");
    let liked_n = app.saved.liked.len();
    if nav_item(ui, icon::HEART, "Liked Songs", app.route == Route::Liked, Some(liked_n)).clicked() {
        app.navigate(Route::Liked, &ctx);
    }
    if nav_item(ui, icon::CLOCK_COUNTER_CLOCKWISE, "Recently Played", app.route == Route::Recent, None).clicked() {
        app.navigate(Route::Recent, &ctx);
    }
    let upcoming = app.queue.len().saturating_sub(app.queue_pos + 1);
    if nav_item(ui, icon::QUEUE, "Queue", app.route == Route::Queue, Some(upcoming)).clicked() {
        app.navigate(Route::Queue, &ctx);
    }
    if app.saved.settings.show_local_files {
        let n = app.local_files.len();
        if nav_item(ui, icon::FOLDER_SIMPLE, "Local Files", app.route == Route::LocalFiles, Some(n)).clicked() {
            app.navigate(Route::LocalFiles, &ctx);
        }
    }
    // Download progress
    if let Some((t, p)) = &app.downloading
        && !compact
    {
        let p = *p.lock().unwrap();
        let left = app.download_queue.len();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.spinner();
            let more = if left > 0 { format!(" (+{left})") } else { String::new() };
            ui.label(RichText::new(format!("{} {:.0}% · {}{more}", tr("Downloading…"), p * 100.0, t.title)).font(regular(11.5)).color(pal().text_muted));
        });
    }

    ui.add_space(14.0);
    if compact {
        nav_heading(ui, "Playlists");
        if nav_item(ui, icon::PLUS, "Create playlist", false, None).clicked() {
            app.new_playlist = Some((String::new(), vec![]));
        }
        if nav_item(ui, icon::ARROW_SQUARE_IN, "Import playlist", false, None).clicked() {
            app.import_open = true;
        }
    } else {
        ui.horizontal(|ui| {
            nav_heading(ui, "Playlists");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, icon::PLUS, 14.0, pal().text_muted, tr("Create playlist")).clicked() {
                    app.new_playlist = Some((String::new(), vec![]));
                }
                if icon_button(ui, icon::ARROW_SQUARE_IN, 14.0, pal().text_muted, tr("Import playlist")).clicked() {
                    app.import_open = true;
                }
            });
        });
    }
    // Blossom-style themes: a big cover of the current song at the bottom of the sidebar.
    let big_art = !compact && layout().now_playing_art == crate::theme::NowPlayingArt::SidebarLarge && app.current.is_some();
    let art_h = if big_art { ui.available_width() + 12.0 } else { 0.0 };

    egui::ScrollArea::vertical().id_salt("sidebar_playlists").auto_shrink([false, true]).max_height((ui.available_height() - art_h).max(40.0)).show(ui, |ui| {
        if app.saved.playlists.is_empty() && !compact {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new(tr("Make your first playlist with +")).color(pal().text_faint).size(12.5));
            });
        }
        for i in 0..app.saved.playlists.len() {
            let name = app.saved.playlists[i].name.clone();
            let count = app.saved.playlists[i].tracks.len();
            let resp = nav_item(ui, icon::PLAYLIST, &name, app.route == Route::Local(i), Some(count));
            if resp.clicked() {
                app.navigate(Route::Local(i), &ctx);
            }
            resp.context_menu(|ui| {
                if menu_item(ui, icon::PENCIL_SIMPLE, "Rename") {
                    app.rename_playlist = Some((i, name.clone()));
                }
                if menu_item(ui, icon::SHARE_FAT, "Copy share code") {
                    app.share_playlist(i, &ctx);
                }
                if menu_item(ui, icon::TRASH, "Delete") {
                    app.confirm_delete = Some(i);
                }
            });
        }
    });
    if big_art && let Some(t) = app.current.clone() {
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            let w = ui.available_width();
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(w), Sense::click());
            artwork(ui, r, t.art_big(), r_lg());
            a11y(ui, &resp, egui::WidgetType::Button, || format!("{}: {} by {}", tr("Now playing"), t.title, t.artist.name));
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.navigate(Route::Queue, &ctx);
            }
        });
    }
}

fn vgradient_rounded(ui: &Ui, r: Rect, top: Color32, bottom: Color32) {
    // A rounded square with a subtle two-tone fill.
    ui.painter().rect_filled(r, CornerRadius::same(8), bottom);
    let top_half = Rect::from_min_max(r.min, pos2(r.max.x, r.center().y));
    ui.painter().rect_filled(top_half, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, top.gamma_multiply(0.35));
}

/// True when the theme wants an icon-only sidebar.
fn compact_sidebar() -> bool {
    layout().sidebar == crate::theme::SidebarStyle::Icons
}

fn nav_heading(ui: &mut Ui, text: &str) {
    if compact_sidebar() {
        ui.add_space(4.0);
        ui.separator();
        return;
    }
    let text = tr(text);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(text.to_uppercase()).font(semi(10.5)).color(pal().text_faint));
    });
    ui.add_space(2.0);
}

fn nav_item(ui: &mut Ui, glyph: &str, label: &str, selected: bool, badge: Option<usize>) -> egui::Response {
    let label = tr(label);
    if compact_sidebar() {
        // Icon only, with the name as a tooltip.
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
        focus_ring(ui, &resp);
        let fill = if selected { pal().surface_active } else if resp.hovered() { pal().surface_hover } else { Color32::TRANSPARENT };
        ui.painter().rect_filled(rect, r_md(), fill);
        let (font, col) = if selected { (filled_icon(20.0), pal().accent_soft) } else { (regular(20.0), pal().text_muted) };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, font, col);
        return resp.on_hover_text(label).on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
    focus_ring(ui, &resp);
    let fill = if selected {
        pal().surface_active
    } else if resp.hovered() {
        pal().surface_hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, r_md(), fill);
    if selected {
        let bar = Rect::from_min_size(pos2(rect.left(), rect.center().y - 9.0), vec2(3.0, 18.0));
        ui.painter().rect_filled(bar, CornerRadius::same(2), pal().accent_soft);
    }
    let icon_col = if selected { pal().accent_soft } else { pal().text_muted };
    let font = if selected { filled_icon(17.0) } else { regular(17.0) };
    ui.painter().text(pos2(rect.left() + 22.0, rect.center().y), Align2::CENTER_CENTER, glyph, font, icon_col);
    let text_col = if selected || resp.hovered() { pal().text } else { pal().text_muted };
    let max_w = rect.width() - 80.0;
    let g = one_line(ui, label, med(13.5), text_col, max_w);
    ui.painter().galley(pos2(rect.left() + 42.0, rect.center().y - g.size().y / 2.0), g, text_col);
    if let Some(n) = badge.filter(|n| *n > 0) {
        // Hovered/selected rows have a lighter background, where faint text would be too dim.
        let badge_col = if selected || resp.hovered() { pal().text_muted } else { pal().text_faint };
        ui.painter().text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, n.to_string(), regular(11.5), badge_col);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------------------
// Top bar: back/forward + search
// ---------------------------------------------------------------------------

fn top_bar(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    ui.horizontal_centered(|ui| {
        let back_col = if app.can_go_back() { pal().text } else { pal().text_faint };
        if icon_button(ui, icon::CARET_LEFT, 17.0, back_col, tr("Back")).clicked() {
            app.go_back(&ctx);
        }
        let fwd_col = if app.can_go_forward() { pal().text } else { pal().text_faint };
        if icon_button(ui, icon::CARET_RIGHT, 17.0, fwd_col, tr("Forward")).clicked() {
            app.go_forward(&ctx);
        }
        ui.add_space(10.0);

        // Search pill
        let w = (ui.available_width() - 150.0).clamp(220.0, 440.0);
        let (rect, _) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
        let id = egui::Id::new("search_box");
        let focused = ui.memory(|m| m.has_focus(id));
        ui.painter().rect(
            rect,
            CornerRadius::same(19),
            pal().surface,
            Stroke::new(1.0, if focused { pal().accent.gamma_multiply(0.8) } else { pal().border }),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(pos2(rect.left() + 18.0, rect.center().y), Align2::CENTER_CENTER, icon::MAGNIFYING_GLASS, regular(15.0), pal().text_muted);

        let edit_rect = Rect::from_min_max(pos2(rect.left() + 34.0, rect.top()), pos2(rect.right() - 60.0, rect.bottom()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(edit_rect).layout(Layout::left_to_right(Align::Center)));
        let resp = child.add(
            egui::TextEdit::singleline(&mut app.search_text)
                .id(id)
                .frame(Frame::NONE)
                .hint_text(RichText::new(tr("Search songs, artists, playlists")).color(pal().text_faint))
                .font(regular(13.5))
                .desired_width(edit_rect.width())
                .vertical_align(Align::Center),
        );
        if app.focus_search {
            resp.request_focus();
            app.focus_search = false;
        }
        if resp.changed() {
            app.search_edited_at = Some(ui.input(|i| i.time));
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            app.search_edited_at = None;
            app.submit_search(&ctx);
        }
        // Clear button or Ctrl K hint
        let right = pos2(rect.right() - 14.0, rect.center().y);
        if !app.search_text.is_empty() {
            let r = Rect::from_center_size(right - vec2(8.0, 0.0), Vec2::splat(22.0));
            let c = ui.interact(r, id.with("clear"), Sense::click());
            a11y(ui, &c, egui::WidgetType::Button, || "Clear search".to_owned());
            ui.painter().text(r.center(), Align2::CENTER_CENTER, icon::X, regular(14.0), if c.hovered() { pal().text } else { pal().text_muted });
            if c.clicked() {
                app.search_text.clear();
            }
        } else {
            let hint = Rect::from_min_max(pos2(right.x - 42.0, right.y - 10.0), pos2(right.x, right.y + 10.0));
            ui.painter().rect(hint, CornerRadius::same(5), pal().surface_hover, Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
            ui.painter().text(hint.center(), Align2::CENTER_CENTER, "Ctrl K", med(10.5), pal().text_muted);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Your profile picture: opens your profile.
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
            personal::avatar(ui, r, app);
            if app.route == Route::Profile || resp.hovered() {
                ui.painter().circle_stroke(r.center(), 18.0, Stroke::new(2.0, pal().accent_soft));
            }
            let who = if app.saved.profile.name.is_empty() { tr("Profile").to_string() } else { app.saved.profile.name.clone() };
            a11y(ui, &resp, egui::WidgetType::Button, || who.clone());
            focus_ring(ui, &resp);
            if resp.on_hover_text(&who).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                if app.saved.profile.created == 0 {
                    app.profile_editing = Some(ProfileForm { welcome: true, ..Default::default() });
                } else {
                    app.navigate(Route::Profile, &ctx);
                }
            }
            ui.add_space(4.0);
            let on_settings = app.route == Route::Settings;
            let gear_col = if on_settings { pal().accent_soft } else { pal().text_muted };
            if icon_button(ui, icon::GEAR_SIX, 18.0, gear_col, tr("Settings")).clicked() {
                app.navigate(Route::Settings, &ctx);
            }
            if app.theme_has_light_and_dark() {
                let (glyph, tip) = if app.saved.light_mode { (icon::MOON, "Switch to dark mode") } else { (icon::SUN, "Switch to light mode") };
                if icon_button(ui, glyph, 17.0, pal().text_muted, tr(tip)).clicked() {
                    app.toggle_theme(&ctx);
                }
            }
            ui.add_space(6.0);
            let r = ui.add(egui::Label::new(RichText::new(tr("Music info by Deezer")).font(med(11.5)).color(pal().text_faint)).sense(Sense::click()));
            if r.on_hover_text("Song info, artwork and charts from Deezer. Audio from YouTube.").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab("https://www.deezer.com"));
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

fn page(app: &mut App, ui: &mut Ui) {
    match app.route.clone() {
        Route::Home => home(app, ui),
        Route::Charts => charts(app, ui),
        Route::Search(q) => search(app, ui, &q),
        Route::Playlist(_) => remote_playlist(app, ui),
        Route::Album(_) => album_page(app, ui),
        Route::Artist(_) => artist(app, ui),
        Route::Settings => settings_page::settings_page(app, ui),
        Route::Mix(id) => personal::mix_page(app, ui, &id),
        Route::Song(id) => song_page::song_page(app, ui, &id),
        Route::Profile => personal::profile_page(app, ui),
        Route::LocalFiles => {
            if app.local_scanning && app.local_files.is_empty() {
                page_title(ui, "Local Files", None);
                skeleton_rows(ui, 8);
            } else {
                let tracks = app.local_files.clone();
                let dir = crate::settings::music_dir().map(|d| d.display().to_string()).unwrap_or_default();
                local_collection(app, ui, "Local Files", &dir, icon::FOLDER_SIMPLE, &tracks, None);
            }
        }
        Route::Liked => {
            let tracks = app.saved.liked.clone();
            let sub = format!("{} songs", tracks.len());
            local_collection(app, ui, "Liked Songs", &sub, icon::HEART, &tracks, None);
        }
        Route::Recent => {
            let tracks = app.saved.recent.clone();
            local_collection(app, ui, "Recently Played", "Your last 50 songs, stored only on this computer", icon::CLOCK_COUNTER_CLOCKWISE, &tracks, None);
            if !tracks.is_empty() {
                ui.add_space(12.0);
                if ghost_button(ui, icon::TRASH, "Clear history").clicked() {
                    app.saved.recent.clear();
                }
            }
        }
        Route::Local(i) => {
            if let Some(p) = app.saved.playlists.get(i).cloned() {
                let sub = format!("Your playlist · {} songs · {}", p.tracks.len(), total_time(&p.tracks));
                local_collection(app, ui, &p.name, &sub, icon::PLAYLIST, &p.tracks, Some(i));
            }
        }
        Route::Queue => queue_page(app, ui),
    }
}

/// Helper that draws a loading / error state, and returns the page when ready.
fn with_remote(app: &mut App, ui: &mut Ui, f: impl FnOnce(&mut App, &mut Ui, &Page)) {
    let Some(key) = app.route_key() else { return };
    if !app.cache.contains_key(&key) {
        let ctx = ui.ctx().clone();
        app.ensure_loaded(&ctx);
    }
    // Temporarily take the page out of the cache so `f` can mutate `app`.
    match app.cache.remove(&key) {
        Some(Remote::Ready(page)) => {
            f(app, ui, &page);
            app.cache.entry(key).or_insert(Remote::Ready(page));
        }
        Some(Remote::Failed(e)) => {
            if empty_state(ui, icon::WARNING_CIRCLE, "Something went wrong", &e, Some("Try again")) {
                let ctx = ui.ctx().clone();
                app.retry(&ctx);
            } else {
                app.cache.insert(key, Remote::Failed(e));
            }
        }
        Some(Remote::Loading) | None => {
            app.cache.insert(key, Remote::Loading);
            ui.add_space(24.0);
            skeleton_rows(ui, 10);
        }
    }
}

fn table_ctx<'a>(app: &'a App, names: &'a [String]) -> TableCtx<'a> {
    TableCtx {
        now_playing: app.current.as_ref().map(|t| t.id.as_str()),
        is_playing: app.is_playing(),
        liked: &app.liked_ids,
        playlists: names,
        removable: false,
        queue_mode: false,
        downloads: &app.saved.downloads,
        added: None,
        views: None,
        highlight: app.highlight.as_ref().map(|(id, at)| (id.as_str(), *at)),
        selection: (app.selection.0.as_str(), &app.selection.1),
        index_offset: 0,
    }
}

/// Draws a track table and applies whatever the user clicked.
fn tracks_block(app: &mut App, ui: &mut Ui, salt: &str, tracks: &[Track]) {
    app.ensure_views(tracks);
    let names = app.playlist_names();
    let mut ev = vec![];
    let mut cx = table_ctx(app, &names);
    cx.views = Some(&app.saved.views);
    track_table(ui, salt, tracks, &cx, &mut ev);
    let ctx = ui.ctx().clone();
    app.handle_events(ev, tracks, &ctx);
}

fn page_title(ui: &mut Ui, title: &str, subtitle: Option<&str>) {
    let (title, subtitle) = (tr(title), subtitle.map(tr));
    ui.add_space(12.0);
    ui.label(RichText::new(title).font(heavy(30.0)));
    if let Some(s) = subtitle {
        ui.add_space(2.0);
        ui.label(RichText::new(s).color(pal().text_muted));
    }
    ui.add_space(8.0);
}

fn home(app: &mut App, ui: &mut Ui) {
    if personal::has_history(app) {
        personal::home(app, ui);
        section_title(ui, "Discover", None);
    } else {
        welcome_banner(ui);
    }

    // Genre chips → charts for that genre
    ui.add_space(14.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (id, name) in crate::deezer::GENRES.iter().take(14) {
            if pill(ui, name, false).clicked() {
                app.chart_genre = (*id, name.to_string());
                let ctx = ui.ctx().clone();
                app.navigate(Route::Charts, &ctx);
            }
        }
    });
    home_discovery(app, ui);
}

/// The "Any song. Any artist." banner, shown until you've listened to something.
fn welcome_banner(ui: &mut Ui) {
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
    let r = r_lg();
    ui.painter().rect_filled(rect, r, Color32::from_rgb(28, 20, 52));
    vgradient(ui, rect.shrink(1.0), pal().accent.gamma_multiply(0.30), Color32::TRANSPARENT);
    ui.painter().text(rect.left_top() + vec2(28.0, 30.0), Align2::LEFT_TOP, "Any song. Any artist.", heavy(28.0), ON_DARK);
    ui.painter().text(
        rect.left_top() + vec2(28.0, 72.0),
        Align2::LEFT_TOP,
        "Search anything and press play: full songs, with autoplay when your queue runs out.",
        regular(14.0),
        ON_DARK_MUTED,
    );
    ui.painter().text(rect.right_center() - vec2(40.0, 0.0), Align2::RIGHT_CENTER, icon::VINYL_RECORD, regular(96.0), pal().accent_soft.gamma_multiply(0.25));
}

/// Today's top songs, popular albums, artists and playlists.
fn home_discovery(app: &mut App, ui: &mut Ui) {
    with_remote(app, ui, |app, ui, page| {
        let Page::Home(chart) = page else { return };
        if section_title(ui, "Top songs right now", Some("See all")) {
            app.chart_genre = (0, "All genres".into());
            let ctx = ui.ctx().clone();
            app.navigate(Route::Charts, &ctx);
        }
        tracks_block(app, ui, "home_top", &chart.tracks);
        if !chart.albums.is_empty() {
            section_title(ui, "Popular albums", None);
            album_grid(app, ui, &chart.albums, Some(2));
        }
        if !chart.artists.is_empty() {
            section_title(ui, "Popular artists", None);
            artist_grid(app, ui, &chart.artists, Some(1));
        }
        if !chart.playlists.is_empty() {
            section_title(ui, "Popular playlists", None);
            playlist_grid(app, ui, &chart.playlists, Some(1));
        }
    });
}

/// "Play" pressed on a card: open the page and start playing once its songs load.
fn open_and_play(app: &mut App, route: Route, ctx: &egui::Context) {
    app.navigate(route.clone(), ctx);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("autoplay_page"), format!("{route:?}")));
}

/// Starts the page's songs if "Play" was pressed on its card.
fn autoplay_page(app: &mut App, ctx: &egui::Context, tracks: &[Track]) {
    let want: Option<String> = ctx.data_mut(|d| d.get_temp(egui::Id::new("autoplay_page")));
    if want.as_deref() == Some(format!("{:?}", app.route).as_str()) {
        ctx.data_mut(|d| d.remove::<String>(egui::Id::new("autoplay_page")));
        app.play_list(tracks, 0, ctx);
    }
}

fn album_grid(app: &mut App, ui: &mut Ui, albums: &[AlbumInfo], rows: Option<usize>) {
    let ctx = ui.ctx().clone();
    let mut action = None;
    card_grid(ui, albums, rows, |ui, w, a| {
        let mut sub = a.kind().to_string();
        if let Some(y) = a.year() {
            sub = format!("{y} · {sub}");
        }
        if !a.artist.name.is_empty() {
            sub = format!("{sub} · {}", a.artist.name);
        }
        let info = CardInfo { id: egui::Id::new(("album", &a.id)), art: a.cover.as_deref(), title: &a.title, subtitle: sub, round: false, badge: None };
        match card(ui, w, &info) {
            CardClick::Open => action = Some((a.id.clone(), false)),
            CardClick::Play => action = Some((a.id.clone(), true)),
            CardClick::None => {}
        }
    });
    match action {
        Some((id, false)) => app.navigate(Route::Album(id), &ctx),
        Some((id, true)) => open_and_play(app, Route::Album(id), &ctx),
        None => {}
    }
}

fn playlist_grid(app: &mut App, ui: &mut Ui, playlists: &[PlaylistInfo], rows: Option<usize>) {
    let ctx = ui.ctx().clone();
    let mut action = None;
    card_grid(ui, playlists, rows, |ui, w, p| {
        let sub = if p.creator.is_empty() { format!("{} songs", p.nb_tracks) } else { format!("Playlist · {}", p.creator) };
        let info = CardInfo { id: egui::Id::new(("pl", &p.id)), art: p.picture.as_deref(), title: &p.title, subtitle: sub, round: false, badge: None };
        match card(ui, w, &info) {
            CardClick::Open => action = Some((p.id.clone(), false)),
            CardClick::Play => action = Some((p.id.clone(), true)),
            CardClick::None => {}
        }
    });
    match action {
        Some((id, false)) => app.navigate(Route::Playlist(id), &ctx),
        Some((id, true)) => open_and_play(app, Route::Playlist(id), &ctx),
        None => {}
    }
}

/// Cards for remembered places (home page, profile).
fn album_grid_visits(app: &mut App, ui: &mut Ui, items: &[crate::app::Visit], rows: Option<usize>) {
    let ctx = ui.ctx().clone();
    let mut action = None;
    card_grid(ui, items, rows, |ui, w, v| {
        let info = CardInfo { id: egui::Id::new(("visit", format!("{:?}", v.place))), art: v.image.as_deref(), title: &v.title, subtitle: tr(&v.subtitle).to_string(), round: v.round, badge: None };
        match card(ui, w, &info) {
            CardClick::Open => action = Some((v.place.clone(), false)),
            CardClick::Play => action = Some((v.place.clone(), true)),
            CardClick::None => {}
        }
    });
    if let Some((place, play)) = action
        && let Some(route) = app.route_for(&place)
    {
        if play { open_and_play(app, route, &ctx) } else { app.navigate(route, &ctx) }
    }
}

fn artist_grid(app: &mut App, ui: &mut Ui, artists: &[ArtistInfo], rows: Option<usize>) {
    let ctx = ui.ctx().clone();
    let mut open = None;
    card_grid(ui, artists, rows, |ui, w, a| {
        let sub = if a.fans > 0 { format!("{} fans", fmt_count(a.fans)) } else { "Artist".into() };
        let info = CardInfo { id: egui::Id::new(("artist", &a.id)), art: a.picture.as_deref(), title: &a.name, subtitle: sub, round: true, badge: None };
        if !matches!(card(ui, w, &info), CardClick::None) {
            open = Some(a.id.clone());
        }
    });
    if let Some(id) = open {
        app.navigate(Route::Artist(id), &ctx);
    }
}

fn charts(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let title = if app.chart_genre.0 == 0 { "Charts".to_string() } else { format!("Charts · {}", app.chart_genre.1) };
    page_title(ui, &title, Some("The 100 most popular songs right now"));

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("genre")
            .selected_text(RichText::new(format!("{}  {}", icon::MUSIC_NOTES, app.chart_genre.1)).font(med(12.5)))
            .width(200.0)
            .height(420.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(app.chart_genre.0 == 0, "All genres").clicked() {
                    app.chart_genre = (0, "All genres".into());
                }
                for (id, name) in crate::deezer::GENRES {
                    if ui.selectable_label(app.chart_genre.0 == *id, *name).clicked() {
                        app.chart_genre = (*id, name.to_string());
                    }
                }
            });
    });
    app.ensure_loaded(&ctx);
    ui.add_space(12.0);

    with_remote(app, ui, |app, ui, page| {
        let Page::Tracks(tracks) = page else { return };
        if tracks.is_empty() {
            empty_state(ui, icon::MUSIC_NOTES, "Nothing here yet", "No chart for this genre right now.", None);
            return;
        }
        action_row(app, ui, tracks);
        tracks_block(app, ui, "charts", tracks);
    });
}

fn shuffle_play(app: &mut App, tracks: &[Track], ctx: &egui::Context) {
    if !app.saved.shuffle {
        app.saved.shuffle = true;
    }
    let start = rand::random_range(0..tracks.len().max(1));
    app.play_list(tracks, start, ctx);
}

fn search(app: &mut App, ui: &mut Ui, q: &str) {
    page_title(ui, &format!("Results for “{q}”"), None);
    with_remote(app, ui, |app, ui, page| {
        let Page::Search(r) = page else { return };
        if r.tracks.is_empty() && r.artists.is_empty() && r.albums.is_empty() && r.playlists.is_empty() {
            empty_state(ui, icon::MAGNIFYING_GLASS, "No results", "Try different words or check the spelling.", None);
            return;
        }
        if !r.artists.is_empty() {
            section_title(ui, "Artists", None);
            artist_grid(app, ui, &r.artists, Some(1));
        }
        if !r.tracks.is_empty() {
            section_title(ui, "Songs", None);
            tracks_block(app, ui, "search_tracks", &r.tracks);
        }
        if !r.albums.is_empty() {
            section_title(ui, "Albums & singles", None);
            album_grid(app, ui, &r.albums, Some(2));
        }
        if !r.playlists.is_empty() {
            section_title(ui, "Playlists", None);
            playlist_grid(app, ui, &r.playlists, Some(1));
        }
    });
}

/// Big header used by playlist / artist / library pages.
struct Hero<'a> {
    /// Your own playlist: the title and cover can be clicked to change them.
    editable: bool,
    kind: &'a str,
    title: &'a str,
    subtitle: &'a str,
    art: Option<&'a str>,
    glyph: &'a str,
    round: bool,
    banner: Option<&'a str>,
}

/// What was clicked in an editable hero.
#[derive(Default)]
struct HeroClicks {
    title: bool,
    art: Option<egui::Response>,
}

fn hero(ui: &mut Ui, h: &Hero) -> HeroClicks {
    let mut clicks = HeroClicks::default();
    let style = &parts().hero;
    let hero_r = style.radius.map(CornerRadius::same).unwrap_or(r_lg());
    // Full-bleed themes show an artist's photo across the whole header.
    let full_bleed = layout().hero == crate::theme::HeroStyle::FullBleed && h.round && h.art.is_some();
    if full_bleed {
        return hero_full_bleed(ui, h, hero_r);
    }
    ui.add_space(8.0);
    let height = 232.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    // Text is light on the normal dark banner; with a theme's own fill it uses the theme's text colors.
    let (on, on_muted) = match (style.color, style.fill) {
        (Some(c), _) => (c, c.gamma_multiply(0.8)),
        (None, Some(_)) => (pal().text, pal().text_muted),
        (None, None) => (ON_DARK, ON_DARK_MUTED),
    };
    if let Some(b) = h.banner {
        artwork(ui, rect, Some(b), hero_r);
        ui.painter().rect_filled(rect, hero_r, Color32::from_black_alpha(150));
    } else if let Some(fill) = style.fill {
        ui.painter().rect_filled(rect, hero_r, fill);
    } else {
        // Dark banner tinted with the theme's accent.
        let base = Color32::from_rgb(14, 13, 20);
        ui.painter().rect_filled(rect, hero_r, base.lerp_to_gamma(pal().accent, 0.12));
        vgradient(ui, rect.shrink(1.0), pal().accent.gamma_multiply(0.22), Color32::TRANSPARENT);
    }
    if let Some(b) = style.border {
        ui.painter().rect_stroke(rect, hero_r, b, egui::StrokeKind::Inside);
    }
    let art_size = 184.0;
    let art_rect = Rect::from_min_size(pos2(rect.left() + 24.0, rect.center().y - art_size / 2.0), Vec2::splat(art_size));
    // shadow
    let radius = if h.round { CornerRadius::same(92) } else { r_md() };
    let shadow = egui::Shadow { offset: [0, 10], blur: 28, spread: 0, color: Color32::from_black_alpha(110) };
    ui.painter().add(shadow.as_shape(art_rect, radius));
    if h.art.is_some() {
        artwork(ui, art_rect, h.art, radius);
    } else {
        ui.painter().rect_filled(art_rect, radius, pal().accent.gamma_multiply(0.55));
        vgradient(ui, art_rect, Color32::from_white_alpha(18), Color32::TRANSPARENT);
        ui.painter().text(art_rect.center(), Align2::CENTER_CENTER, h.glyph, filled_icon(72.0), Color32::WHITE);
    }
    if h.editable {
        let r = ui.interact(art_rect, ui.id().with("hero_art"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        a11y(ui, &r, egui::WidgetType::Button, || tr("Change cover").to_owned());
        if r.hovered() {
            // Pencil overlay, like Spotify.
            ui.painter().rect_filled(art_rect, radius, Color32::from_black_alpha(140));
            ui.painter().text(art_rect.center() - vec2(0.0, 10.0), Align2::CENTER_CENTER, icon::PENCIL_SIMPLE, regular(40.0), Color32::WHITE);
            ui.painter().text(art_rect.center() + vec2(0.0, 26.0), Align2::CENTER_CENTER, tr("Change cover"), med(13.0), Color32::WHITE);
        }
        clicks.art = Some(r);
    }
    let tx = art_rect.right() + 28.0;
    let tw = rect.right() - tx - 24.0;
    let bottom = art_rect.bottom();
    paint_line(ui, pos2(tx, bottom - 112.0), &h.kind.to_uppercase(), semi(11.5), on_muted, tw);
    let title = if style.uppercase == Some(true) { h.title.to_uppercase() } else { h.title.to_string() };
    let size = style.font_size.unwrap_or(if title.chars().count() > 28 { 30.0 } else { 44.0 });
    let font = if style.bold == Some(false) { semi(size) } else { heavy(size) };
    let tg = one_line(ui, &title, font, on, tw);
    let th = tg.size().y;
    let title_rect = Rect::from_min_size(pos2(tx, bottom - 30.0 - th), tg.size());
    ui.painter().galley(title_rect.min, tg, on);
    if h.editable {
        let r = ui.interact(title_rect, ui.id().with("hero_title"), Sense::click()).on_hover_cursor(egui::CursorIcon::Text);
        a11y(ui, &r, egui::WidgetType::Button, || format!("{}: {}", tr("Rename"), h.title));
        clicks.title = r.on_hover_text(tr("Rename")).clicked();
    }
    paint_line(ui, pos2(tx, bottom - 20.0), h.subtitle, regular(13.5), on_muted, tw);
    ui.add_space(18.0);
    clicks
}

/// Header with a full-width photo (themes with `hero = "fullbleed"`, artist pages).
fn hero_full_bleed(ui: &mut Ui, h: &Hero, radius: CornerRadius) -> HeroClicks {
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 320.0), Sense::hover());
    // Fill the banner without stretching: crop the (square) photo to the banner's shape,
    // keeping a little more of the top, where faces usually are.
    artwork_cover(ui, rect, h.art, radius, 0.35);
    // Darken towards the bottom so the name stays readable.
    vgradient(ui, rect, Color32::from_black_alpha(20), Color32::from_black_alpha(200));
    let style = &parts().hero;
    let tw = rect.width() - 56.0;
    let title = if style.uppercase == Some(true) { h.title.to_uppercase() } else { h.title.to_string() };
    // Bottom up: subtitle, then the name, then "ARTIST" above the name's actual height.
    let tg = one_line(ui, &title, heavy(style.font_size.unwrap_or(64.0)), ON_DARK, tw);
    let title_top = rect.bottom() - 46.0 - tg.size().y;
    paint_line(ui, pos2(rect.left() + 28.0, title_top - 20.0), &h.kind.to_uppercase(), semi(11.5), ON_DARK_MUTED, tw);
    ui.painter().galley(pos2(rect.left() + 28.0, title_top), tg, ON_DARK);
    paint_line(ui, rect.left_bottom() + vec2(28.0, -36.0), h.subtitle, regular(14.0), ON_DARK_MUTED, tw);
    ui.add_space(18.0);
    HeroClicks::default()
}

fn remote_playlist(app: &mut App, ui: &mut Ui) {
    with_remote(app, ui, |app, ui, page| {
        let Page::Playlist { info, tracks } = page else { return };
        let mut sub = format!("{} songs · {}", tracks.len(), total_time(tracks));
        if !info.creator.is_empty() {
            sub = format!("{} · {sub}", info.creator);
        }
        hero(ui, &Hero { editable: false, kind: "Playlist", title: &info.title, subtitle: &sub, art: info.picture.as_deref(), glyph: icon::PLAYLIST, round: false, banner: None });
        app.note_visit(crate::app::Place::Playlist(info.id.clone()), &info.title, &format!("Playlist · {}", info.creator), info.picture.as_deref(), false);
        let ctx = ui.ctx().clone();
        autoplay_page(app, &ctx, tracks);
        action_row(app, ui, tracks);
        tracks_block(app, ui, "playlist", tracks);
    });
}

fn album_page(app: &mut App, ui: &mut Ui) {
    with_remote(app, ui, |app, ui, page| {
        let Page::Album { info, tracks } = page else { return };
        let mut sub = info.artist.name.clone();
        if let Some(y) = info.year() {
            sub.push_str(&format!(" · {y}"));
        }
        sub.push_str(&format!(" · {} songs, {}", tracks.len(), total_time(tracks)));
        hero(ui, &Hero { editable: false, kind: info.kind(), title: &info.title, subtitle: &sub, art: info.cover.as_deref(), glyph: icon::VINYL_RECORD, round: false, banner: None });
        app.note_visit(crate::app::Place::Album(info.id.clone()), &info.title, &format!("{} · {}", info.kind(), info.artist.name), info.cover.as_deref(), false);
        let ctx = ui.ctx().clone();
        autoplay_page(app, &ctx, tracks);
        ui.horizontal(|ui| {
            action_row(app, ui, tracks);
            if crate::deezer::is_deezer_id(&info.artist.id) && ghost_button(ui, icon::USER, &info.artist.name).clicked() {
                app.navigate(Route::Artist(info.artist.id.clone()), &ctx);
            }
        });
        tracks_block(app, ui, "album", tracks);
        if let Some(label) = info.label.as_deref().filter(|l| !l.is_empty()) {
            ui.add_space(10.0);
            let date = info.release_date.clone().unwrap_or_default();
            ui.label(RichText::new(format!("{date}  ·  {label}")).font(regular(12.0)).color(pal().text_muted));
        }
    });
}

fn action_row(app: &mut App, ui: &mut Ui, tracks: &[Track]) {
    let ctx = ui.ctx().clone();
    let playing_this = app.is_playing() && app.current.as_ref().is_some_and(|c| tracks.iter().any(|t| t.id == c.id));
    ui.horizontal(|ui| {
        if play_button(ui, 52.0, playing_this).clicked() {
            if playing_this {
                app.toggle_play(&ctx);
            } else {
                app.play_list(tracks, 0, &ctx);
            }
        }
        ui.add_space(6.0);
        if ghost_button(ui, icon::SHUFFLE, "Shuffle").clicked() && !tracks.is_empty() {
            shuffle_play(app, tracks, &ctx);
        }
    });
    ui.add_space(10.0);
}

fn artist(app: &mut App, ui: &mut Ui) {
    with_remote(app, ui, |app, ui, page| {
        let Page::Artist(p) = page else { return };
        let sub = format!("{} fans · {} releases", fmt_count(p.artist.fans), p.artist.albums);
        hero(ui, &Hero { editable: false, kind: "Artist", title: &p.artist.name, subtitle: &sub, art: p.artist.picture_xl.as_deref(), glyph: icon::USER, round: true, banner: None });
        app.note_visit(crate::app::Place::Artist(p.artist.id.clone()), &p.artist.name, "Artist", p.artist.picture.as_deref().or(p.artist.picture_xl.as_deref()), true);
        action_row(app, ui, &p.top);
        let ctx = ui.ctx().clone();

        if let Some(latest) = p.releases.first() {
            section_title(ui, "Latest release", None);
            if latest_release_card(ui, latest) {
                app.navigate(Route::Album(latest.id.clone()), &ctx);
            }
        }

        section_title(ui, "Popular", None);
        if p.top.is_empty() {
            ui.label(RichText::new("No popular songs listed for this artist.").color(pal().text_muted));
        } else {
            // Ten songs, with a button to show all of them.
            let id = egui::Id::new(("artist_all", &p.artist.id));
            let show_all = ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));
            // Most-viewed first (songs whose views aren't known yet keep Deezer's order, after).
            let mut top = p.top.clone();
            app.ensure_views(&top);
            top.sort_by_key(|t| std::cmp::Reverse(app.saved.views.get(&t.id).map(|(v, _)| *v).unwrap_or(0)));
            let shown = if show_all { &top[..] } else { &top[..top.len().min(10)] };
            tracks_block(app, ui, "artist_top", shown);
            if p.top.len() > 10 {
                ui.add_space(6.0);
                let label = if show_all { "Show less".to_string() } else { format!("Show all {}", p.top.len()) };
                if ghost_button(ui, "", &label).clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(id, !show_all));
                }
            }
        }

        if !p.releases.is_empty() {
            section_title(ui, "Discography", None);
            album_grid(app, ui, &p.releases, None);
        }
        if !p.related.is_empty() {
            section_title(ui, "Fans also like", None);
            artist_grid(app, ui, &p.related, Some(1));
        }
    });
}

/// A wide card for an artist's newest album or single. Returns true when clicked.
fn latest_release_card(ui: &mut Ui, a: &AlbumInfo) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(560.0), 128.0), Sense::click());
    a11y(ui, &resp, egui::WidgetType::Button, || format!("Latest release: {}", a.title));
    let fill = if resp.hovered() { pal().surface_active } else { pal().surface_hover };
    ui.painter().rect_filled(rect, r_lg(), fill);
    let art = Rect::from_min_size(rect.min + vec2(12.0, 12.0), Vec2::splat(104.0));
    artwork(ui, art, a.cover.as_deref(), r_md());
    let tx = art.right() + 18.0;
    let tw = rect.right() - tx - 16.0;
    let date = a.release_date.as_deref().map(pretty_date).unwrap_or_default();
    paint_line(ui, pos2(tx, rect.top() + 26.0), &date.to_uppercase(), semi(11.0), pal().text_muted, tw);
    paint_line(ui, pos2(tx, rect.top() + 46.0), &a.title, heavy(20.0), pal().text, tw);
    let songs = if a.nb_tracks == 1 { "1 song".to_string() } else { format!("{} songs", a.nb_tracks) };
    paint_line(ui, pos2(tx, rect.top() + 80.0), &format!("{} · {songs}", a.kind()), regular(13.0), pal().text_muted, tw);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// "2026-11-06" → "Nov 6, 2026".
fn pretty_date(d: &str) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let parts: Vec<&str> = d.split('-').collect();
    match parts.as_slice() {
        [y, m, day] => match (m.parse::<usize>(), day.parse::<u32>()) {
            (Ok(m), Ok(day)) if (1..=12).contains(&m) => format!("{} {day}, {y}", MONTHS[m - 1]),
            _ => d.to_string(),
        },
        _ => d.to_string(),
    }
}

fn local_collection(app: &mut App, ui: &mut Ui, title: &str, sub: &str, glyph: &str, tracks: &[Track], playlist: Option<usize>) {
    let ctx = ui.ctx().clone();
    let kind = if playlist.is_some() { "Playlist" } else { "Library" };
    // Cover: your own picture, else the first song's artwork.
    let custom = playlist.and_then(|i| app.saved.playlists.get(i)).and_then(|p| p.cover.as_ref()).map(|c| crate::settings::file_uri(c));
    let art = custom.clone().or_else(|| if playlist.is_some() { tracks.first().and_then(|t| t.art_big()).map(str::to_owned) } else { None });
    let clicks = hero(ui, &Hero { editable: playlist.is_some(), kind: tr(kind), title: tr(title), subtitle: sub, art: art.as_deref(), glyph, round: false, banner: None });
    match (&app.route, playlist) {
        (_, Some(_)) => app.note_visit(crate::app::Place::Yours(title.to_string()), title, "Playlist", art.as_deref(), false),
        (Route::Liked, _) => app.note_visit(crate::app::Place::Liked, tr("Liked Songs"), "Playlist", None, false),
        _ => {}
    }
    if let Some(i) = playlist {
        if clicks.title {
            app.rename_playlist = Some((i, title.to_string()));
        }
        if let Some(r) = &clicks.art {
            if r.clicked() {
                pick_cover(app, i, &ctx);
            }
            r.context_menu(|ui| {
                if menu_item(ui, icon::IMAGE, "Change cover") {
                    pick_cover(app, i, &ctx);
                }
                if custom.is_some() && menu_item(ui, icon::TRASH, "Remove cover") {
                    app.remove_playlist_cover(i);
                }
            });
        }
    }
    if tracks.is_empty() {
        let body = match (&app.route, playlist) {
            (Route::Liked, _) => "Tap the heart on any song to save it here.",
            (Route::LocalFiles, _) => "Put music files in your Music folder and they'll show up here.",
            (_, Some(_)) => "Right-click any song and choose “Add to playlist”.",
            _ => "Songs you play will show up here.",
        };
        empty_state(ui, glyph, "It's quiet in here", body, None);
        if playlist.is_some() {
            ui.vertical_centered(|ui| {
                if ghost_button(ui, icon::TRASH, "Delete playlist").clicked()
                    && let Route::Local(i) = app.route
                {
                    app.confirm_delete = Some(i);
                }
            });
        }
        return;
    }

    // When songs were added: playlists and Liked Songs keep track of it.
    let added = match (&app.route, playlist) {
        (_, Some(i)) => app.saved.playlists.get(i).map(|p| p.added.clone()),
        (Route::Liked, _) => Some(app.saved.liked_added.clone()),
        _ => None,
    };
    app.ensure_genres(tracks, &ctx);
    // Imported songs: look again on Deezer, and give coverless songs their video's picture.
    app.rematch_imports(tracks, &ctx);
    let coverless: Vec<Track> = tracks.iter().filter(|t| t.cover_small.is_none() && !crate::local::is_local(&t.id)).cloned().collect();
    app.ensure_views(&coverless);
    list_toolbar(app, ui, tracks, playlist, title);
    let chips = crate::app::genre_chips(tracks, &app.saved.album_genres);
    if !chips.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for g in &chips {
                let on = app.list_view.genre.as_deref() == Some(g.as_str());
                if pill(ui, g, on).clicked() {
                    app.list_view.genre = if on { None } else { Some(g.clone()) };
                }
            }
        });
        ui.add_space(8.0);
    }

    let view = crate::app::apply_view(tracks, &app.list_view, added.as_ref(), &app.saved.album_genres);
    if view.is_empty() {
        ui.add_space(20.0);
        ui.label(RichText::new(tr("No songs match.")).color(pal().text_muted));
        return;
    }
    let names = app.playlist_names();
    let mut cx = table_ctx(app, &names);
    cx.removable = playlist.is_some();
    cx.added = added.as_ref();
    let mut ev = vec![];
    track_table(ui, "local", &view, &cx, &mut ev);
    // Rows are numbered by what's on screen; removing goes by the song itself.
    let mut rest = vec![];
    for e in ev {
        match (e, playlist) {
            (TrackEvent::RemoveFromPlaylist(row), Some(i)) => {
                if let Some(t) = view.get(row) {
                    app.remove_from_playlist(i, &t.id);
                }
            }
            (other, _) => rest.push(other),
        }
    }
    app.handle_events(rest, &view, &ctx);
}

/// Asks for a picture (Windows file dialog) and makes it the playlist cover.
fn pick_cover(app: &mut App, i: usize, ctx: &egui::Context) {
    #[cfg(windows)]
    if let Some(file) = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]).pick_file()
        && let Err(e) = app.set_playlist_cover(i, &file)
    {
        app.toast(&e, ctx);
    }
    #[cfg(not(windows))]
    let _ = (app, i, ctx);
}

/// The bar above a song list: play, shuffle, download · search, sort, more.
fn list_toolbar(app: &mut App, ui: &mut Ui, tracks: &[Track], playlist: Option<usize>, title: &str) {
    let ctx = ui.ctx().clone();
    let playing_this = app.is_playing() && app.current.as_ref().is_some_and(|c| tracks.iter().any(|t| t.id == c.id));
    let view = crate::app::apply_view(tracks, &app.list_view, None, &app.saved.album_genres);
    ui.horizontal(|ui| {
        ui.set_min_height(56.0);
        if play_button(ui, 52.0, playing_this).clicked() {
            if playing_this {
                app.toggle_play(&ctx);
            } else {
                app.play_list(&view, 0, &ctx);
            }
        }
        ui.add_space(4.0);
        let sh_col = if app.saved.shuffle { pal().accent_soft } else { pal().text_muted };
        if icon_toggle(ui, icon::SHUFFLE, 26.0, sh_col, tr("Shuffle"), tr("Shuffle"), app.saved.shuffle).clicked() {
            shuffle_play(app, &view, &ctx);
        }
        if app.saved.shuffle {
            let r = ui.min_rect();
            ui.painter().circle_filled(pos2(r.right() - 20.0, r.center().y + 18.0), 2.0, pal().accent_soft);
        }
        // Download: all songs here, or a check once they're all saved.
        let online: Vec<Track> = tracks.iter().filter(|t| !crate::local::is_local(&t.id)).cloned().collect();
        if !online.is_empty() {
            let all = online.iter().all(|t| app.is_downloaded(&t.id));
            let busy = !all && online.iter().any(|t| app.download_queue.iter().any(|q| q.id == t.id) || app.downloading.as_ref().is_some_and(|(d, _)| d.id == t.id));
            if busy {
                ui.add_space(6.0);
                ui.spinner().on_hover_text(tr("Downloading…"));
            } else if all {
                if icon_button(ui, icon::ARROW_CIRCLE_DOWN, 26.0, pal().accent_soft, tr("Downloaded")).clicked() {
                    // Already downloaded: nothing to do.
                }
            } else if icon_button(ui, icon::ARROW_CIRCLE_DOWN, 26.0, pal().text_muted, tr("Download")).clicked() {
                app.download(&online, &ctx);
            }
        }
        if let Some(i) = playlist {
            ui.menu_button(RichText::new(icon::DOTS_THREE).size(22.0).color(pal().text_muted), |ui| {
                if menu_item(ui, icon::PENCIL_SIMPLE, "Rename") {
                    app.rename_playlist = Some((i, title.to_string()));
                }
                if menu_item(ui, icon::IMAGE, "Change cover") {
                    pick_cover(app, i, &ctx);
                }
                if menu_item(ui, icon::SHARE_FAT, "Copy share code") {
                    app.share_playlist(i, &ctx);
                }
                if menu_item(ui, icon::TRASH, "Delete playlist") {
                    app.confirm_delete = Some(i);
                }
            });
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Sort
            let sort = app.list_view.sort;
            ui.menu_button(RichText::new(format!("{}  {}", tr(sort.label()), icon::LIST)).font(med(13.0)).color(pal().text_muted), |ui| {
                ui.label(RichText::new(tr("Sort by")).font(semi(11.0)).color(pal().text_faint));
                for s in crate::app::SortBy::ALL {
                    let check = if s == sort { icon::CHECK } else { " " };
                    if menu_item(ui, check, s.label()) {
                        app.list_view.sort = s;
                    }
                }
            });
            ui.add_space(8.0);
            // Search: an icon that opens into a box.
            if app.list_view.search_open || !app.list_view.search.is_empty() {
                let id = ui.id().with("list_search");
                let r = ui.add(
                    egui::TextEdit::singleline(&mut app.list_view.search)
                        .id(id)
                        .hint_text(tr("Search in playlist"))
                        .desired_width(200.0),
                );
                if app.list_view.search_open && !r.has_focus() && app.list_view.search.is_empty() && r.lost_focus() {
                    app.list_view.search_open = false;
                }
                if app.list_view.search_open && app.list_view.search.is_empty() && !r.lost_focus() {
                    r.request_focus();
                }
            }
            if icon_button(ui, icon::MAGNIFYING_GLASS, 17.0, pal().text_muted, tr("Search in playlist")).clicked() {
                app.list_view.search_open = !app.list_view.search_open;
                if !app.list_view.search_open {
                    app.list_view.search.clear();
                }
            }
        });
    });
    ui.add_space(8.0);
}

fn queue_page(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    page_title(ui, "Queue", None);
    let Some(cur) = app.current.clone() else {
        empty_state(ui, icon::QUEUE, "Nothing playing", "Pick a song and your queue will appear here.", None);
        return;
    };
    section_title(ui, "Now playing", None);
    let names = app.playlist_names();
    let mut ev = vec![];
    let now = [cur];
    track_table(ui, "q_now", &now, &table_ctx(app, &names), &mut ev);
    // "Play" on the now-playing row toggles pause.
    for e in ev {
        match e {
            TrackEvent::Play(_) => app.toggle_play(&ctx),
            other => app.handle_events(vec![other], &now, &ctx),
        }
    }

    let upcoming: Vec<Track> = app.queue.iter().skip(app.queue_pos + 1).cloned().collect();
    let clear = section_title(ui, "Next up", (!upcoming.is_empty()).then_some("Clear"));
    if clear {
        app.queue.truncate(app.queue_pos + 1);
    }
    ui.horizontal(|ui| {
        let on = app.saved.autoplay;
        if pill(ui, if on { "Autoplay: on" } else { "Autoplay: off" }, on).clicked() {
            app.toggle_autoplay();
        }
        ui.label(RichText::new("When your queue ends, keep playing similar songs.").font(regular(12.5)).color(pal().text_muted));
    });
    ui.add_space(8.0);
    if upcoming.is_empty() {
        let msg = if app.saved.autoplay {
            "Nothing queued. Similar songs will play when this one ends."
        } else {
            "Nothing queued. Right-click a song → “Add to queue”."
        };
        ui.label(RichText::new(msg).color(pal().text_muted));
        return;
    }
    let mut cx = table_ctx(app, &names);
    cx.queue_mode = true;
    let mut ev = vec![];
    track_table(ui, "q_next", &upcoming, &cx, &mut ev);
    app.handle_events(ev, &upcoming, &ctx);
}

fn total_time(tracks: &[Track]) -> String {
    let s: u32 = tracks.iter().map(|t| t.duration).sum();
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 { format!("{h} hr {m} min") } else { format!("{m} min") }
}

// ---------------------------------------------------------------------------
// Player bar
// ---------------------------------------------------------------------------

fn player_bar(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect();
    let left_w = (full.width() * 0.28).clamp(220.0, 360.0);
    let right_w = left_w.min(260.0);
    let controls_left = layout().player == crate::theme::PlayerStyle::ControlsLeft;
    let center_w = if controls_left {
        (full.width() * 0.34).clamp(300.0, 520.0)
    } else {
        (full.width() - left_w - right_w - 32.0).clamp(280.0, 640.0)
    };
    // Where the song info and the controls go: song left + controls centred (normal),
    // or controls left + song next to them.
    let (left, center) = if controls_left {
        let c = Rect::from_min_size(full.min, vec2(center_w, full.height()));
        let l = Rect::from_min_size(pos2(c.right() + 28.0, full.top()), vec2(left_w, full.height()));
        (l, c)
    } else {
        (Rect::from_min_size(full.min, vec2(left_w, full.height())), Rect::from_center_size(full.center(), vec2(center_w, full.height())))
    };
    // The small cover hides when the theme shows a big one in the sidebar.
    let small_art = !(layout().now_playing_art == crate::theme::NowPlayingArt::SidebarLarge && !compact_sidebar());

    // ---- now playing
    if let Some(t) = app.current.clone() {
        let art = if small_art {
            Rect::from_min_size(pos2(left.left(), left.center().y - 28.0), Vec2::splat(56.0))
        } else {
            Rect::from_min_size(pos2(left.left() - 14.0, left.center().y), Vec2::ZERO)
        };
        if small_art {
            artwork(ui, art, t.art_big(), CornerRadius::same(6));
        }
        let art_resp = ui.interact(art, egui::Id::new("np_art"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        a11y(ui, &art_resp, egui::WidgetType::Button, || format!("Now playing: {} by {}. Open queue", t.title, t.artist.name));
        if art_resp.clicked() {
            app.navigate(Route::Queue, &ctx);
        }
        let tx = art.right() + 14.0;
        let tw = left.right() - tx - 40.0;
        let title_rect = paint_line(ui, pos2(tx, left.center().y - 18.0), &t.title, semi(13.5), pal().text, tw);
        // Right-click the song title for the song menu (like, share, credits…).
        // Hovering it shows how long the song took to start.
        let mut title_resp = ui.interact(title_rect, egui::Id::new("np_title"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if let Some(t) = app.audio.timing() {
            title_resp = title_resp.on_hover_text(t);
        }
        if title_resp.hovered() {
            ui.painter().hline(title_rect.x_range(), title_rect.bottom() - 1.0, Stroke::new(1.0, pal().text));
        }
        if title_resp.clicked() {
            app.handle_events(vec![TrackEvent::OpenSong(t.clone())], std::slice::from_ref(&t), &ctx);
        }
        let mut np_events = vec![];
        let names = app.playlist_names();
        title_resp.context_menu(|ui| {
            let mcx = MenuCtx {
                liked: app.liked_ids.contains(&t.id),
                playlists: &names,
                index: None,
                removable: false,
                queue_mode: false,
                downloaded: app.is_downloaded(&t.id),
            };
            track_menu(ui, &t, &mcx, &mut np_events);
        });
        art_resp.context_menu(|ui| {
            let mcx = MenuCtx {
                liked: app.liked_ids.contains(&t.id),
                playlists: &names,
                index: None,
                removable: false,
                queue_mode: false,
                downloaded: app.is_downloaded(&t.id),
            };
            track_menu(ui, &t, &mcx, &mut np_events);
        });
        if !np_events.is_empty() {
            app.handle_events(np_events, std::slice::from_ref(&t), &ctx);
        }
        if let LoadState::Resolving(msg) = app.audio.load_state() {
            // Just "Loading…" while it loads (with the percentage during first-time setup).
            paint_line(ui, pos2(tx, left.center().y + 2.0), &loading_text(&msg), regular(12.0), pal().accent_soft, tw);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        } else {
            let a = link_text(ui, egui::Id::new("np_artist"), pos2(tx, left.center().y + 2.0), &t.artist.name, regular(12.5), pal().text_muted, tw);
            if a.clicked() {
                app.open_artist(&t, &ctx);
            }
        }
        let liked = app.liked_ids.contains(&t.id);
        // The heart sits just after the song's title and artist, like Spotify.
        let second_line = match app.audio.load_state() {
            LoadState::Resolving(msg) => loading_text(&msg),
            _ => t.artist.name.clone(),
        };
        let artist_w = one_line(ui, &second_line, regular(12.5), pal().text_muted, tw).size().x;
        let text_right = tx + title_rect.width().max(artist_w);
        let hr = Rect::from_center_size(pos2((text_right + 22.0).min(left.right() - 18.0), left.center().y), Vec2::splat(30.0));
        let h = ui.interact(hr, egui::Id::new("np_like"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        resp_selected(ui, &h, liked, "Like current song");
        let (font, col) = if liked { (filled_icon(18.0), pal().accent_soft) } else { (regular(18.0), if h.hovered() { pal().text } else { pal().text_muted }) };
        ui.painter().text(hr.center(), Align2::CENTER_CENTER, icon::HEART, font, col);
        if h.clicked() {
            app.toggle_like(&t, &ctx);
        }
    } else {
        ui.painter().text(left.left_center(), Align2::LEFT_CENTER, "Pick something to play", regular(13.0), pal().text_faint);
    }

    // ---- controls + progress
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(center).layout(Layout::top_down(Align::Center)));
    cui.add_space(10.0);
    {
        // Each button has a fixed spot measured from the middle, so play sits exactly
        // above the centre of the progress bar whatever size the icons are.
        let (row, _) = cui.allocate_exact_size(vec2(center_w, 40.0), Sense::hover());
        // A slot exactly as wide as its button (icon size + 12), centred on its spot.
        let mut slot = |n: i32, icon: f32| {
            let w = if n == 0 { 40.0 } else { icon + 12.0 };
            let r = Rect::from_center_size(pos2(row.center().x + n as f32 * 50.0, row.center().y), vec2(w, 40.0));
            cui.new_child(egui::UiBuilder::new().max_rect(r).layout(Layout::left_to_right(Align::Center)))
        };
        let ui = &mut slot(-2, 17.0);
        let shuf_col = if app.saved.shuffle { pal().accent_soft } else { pal().text_muted };
        let tip = if app.saved.shuffle { "Shuffle: on (S)" } else { "Shuffle: off (S)" };
        let s = icon_toggle(ui, icon::SHUFFLE, 17.0, shuf_col, tip, "Shuffle", app.saved.shuffle);
        if app.saved.shuffle {
            active_dot(ui, &s);
        }
        if s.clicked() {
            app.toggle_shuffle();
        }
        let ui = &mut slot(-1, 19.0);
        if icon_button(ui, icon::SKIP_BACK, 19.0, pal().text, "Previous (Ctrl ←)").clicked() {
            app.previous(&ctx);
        }
        let ui = &mut slot(0, 40.0);
        let loading = app.audio.load_state().is_busy() && app.current.is_some();
        let (r, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let playing = app.is_playing();
        a11y(ui, &resp, egui::WidgetType::Button, || if playing { "Pause" } else { "Play" }.to_owned());
        ui.painter().circle_filled(r.center(), 19.0, if resp.hovered() { pal().text.gamma_multiply(0.85) } else { pal().text });
        if loading {
            let t = ui.input(|i| i.time) as f32;
            let start = t * 6.0;
            let pts: Vec<_> = (0..20)
                .map(|k| {
                    let a = start + k as f32 * 0.2;
                    r.center() + vec2(a.cos(), a.sin()) * 8.0
                })
                .collect();
            ui.painter().add(egui::Shape::line(pts, Stroke::new(2.2, pal().bg)));
            ui.ctx().request_repaint();
        } else {
            let glyph = if app.is_playing() { icon::PAUSE } else { icon::PLAY };
            let nudge = if app.is_playing() { 0.0 } else { 1.5 };
            ui.painter().text(r.center() + vec2(nudge, 0.0), Align2::CENTER_CENTER, glyph, filled_icon(18.0), pal().bg);
        }
        if resp.on_hover_text("Play / Pause (Space)").clicked() {
            app.toggle_play(&ctx);
        }
        let ui = &mut slot(1, 19.0);
        if icon_button(ui, icon::SKIP_FORWARD, 19.0, pal().text, "Next (Ctrl →)").clicked() {
            app.next(&ctx);
        }
        let (rg, rc, tip) = match app.saved.repeat {
            Repeat::Off => (icon::REPEAT, pal().text_muted, "Repeat: off (R)"),
            Repeat::All => (icon::REPEAT, pal().accent_soft, "Repeat: all (R)"),
            Repeat::One => (icon::REPEAT_ONCE, pal().accent_soft, "Repeat: one (R)"),
        };
        let ui = &mut slot(2, 17.0);
        let rr = icon_button(ui, rg, 17.0, rc, tip);
        if app.saved.repeat != Repeat::Off {
            active_dot(ui, &rr);
        }
        if rr.clicked() {
            app.cycle_repeat();
        }
    }

    // progress
    cui.add_space(2.0);
    cui.horizontal(|ui| {
        let dur = app.current.as_ref().map(|t| t.duration).unwrap_or(0);
        let pos = if app.needs_load { 0.0 } else { app.audio.position().as_secs_f32() };
        let frac = if dur > 0 { pos / dur as f32 } else { 0.0 };
        let shown = app.seek_preview.unwrap_or(frac);
        let time_w = 42.0;
        let bar_w = center_w - time_w * 2.0 - 20.0;
        ui.add_space(2.0);
        ui.allocate_ui_with_layout(vec2(time_w, 16.0), Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(fmt_duration((shown * dur as f32) as u32)).font(regular(11.5)).color(pal().text_muted));
        });
        let buffered = (!app.needs_load && app.current.is_some()).then(|| app.audio.buffered());
        let (resp, val) = slim_slider(ui, bar_w, shown, buffered, "Seek");
        if let Some(v) = val {
            app.seek_preview = Some(v);
        }
        if resp.drag_stopped() || resp.clicked() || resp.changed() {
            if let Some(v) = app.seek_preview.take() {
                app.seek_fraction(v);
            }
        } else if !resp.dragged() {
            app.seek_preview = None;
        }
        ui.allocate_ui_with_layout(vec2(time_w, 16.0), Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new(fmt_duration(dur)).font(regular(11.5)).color(pal().text_muted));
        });
    });

    // ---- right: queue + volume
    let right = Rect::from_min_max(pos2(full.right() - right_w, full.top()), full.max);
    let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(Layout::right_to_left(Align::Center)));
    rui.spacing_mut().item_spacing.x = 6.0;
    let (_, v) = slim_slider(&mut rui, 104.0, app.saved.volume, None, "Volume");
    if let Some(v) = v {
        app.set_volume(v);
    }
    let vol_icon = match app.saved.volume {
        v if v <= 0.0 => icon::SPEAKER_X,
        v if v < 0.5 => icon::SPEAKER_LOW,
        _ => icon::SPEAKER_HIGH,
    };
    let mute_tip = if app.muted_volume.is_some() { "Unmute (M)" } else { "Mute (M)" };
    if icon_button(&mut rui, vol_icon, 17.0, pal().text_muted, mute_tip).clicked() {
        app.toggle_mute();
    }
    let qcol = if app.route == Route::Queue { pal().accent_soft } else { pal().text_muted };
    let q = icon_button(&mut rui, icon::QUEUE, 17.0, qcol, "Queue");
    if q.clicked() {
        if app.route == Route::Queue {
            app.go_back(&ctx);
        } else {
            app.navigate(Route::Queue, &ctx);
        }
    }
}

// ---------------------------------------------------------------------------
// Pop-ups
// ---------------------------------------------------------------------------

fn dialogs(app: &mut App, ctx: &egui::Context) {
    credits_dialog(app, ctx);
    import_dialog(app, ctx);
    personal::profile_dialog(app, ctx);
    if let Some(idx) = app.confirm_delete.take() {
        let name = app.saved.playlists.get(idx).map(|p| p.name.clone()).unwrap_or_default();
        let (mut keep, mut delete) = (true, false);
        egui::Modal::new(egui::Id::new("confirm_delete"))
            .frame(Frame::new().fill(pal().surface).corner_radius(r_lg()).inner_margin(24).stroke(Stroke::new(1.0, pal().border)))
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("Delete playlist?").font(heavy(20.0)));
                ui.add_space(8.0);
                ui.label(RichText::new(format!("“{name}” will be removed from your library. This can't be undone.")).color(pal().text_muted));
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(pal().accent).corner_radius(16)).clicked() {
                        delete = true;
                    }
                    // Cancel gets focus first so Enter never deletes by accident.
                    let cancel = ui.add(egui::Button::new("Cancel").frame_when_inactive(false).corner_radius(16));
                    if ui.memory(|m| m.focused().is_none()) {
                        cancel.request_focus();
                    }
                    if cancel.clicked() {
                        keep = false;
                    }
                });
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    keep = false;
                }
            });
        if delete {
            app.delete_playlist(idx, ctx);
        } else if keep {
            app.confirm_delete = Some(idx);
        }
    }

    if let Some((mut name, track)) = app.new_playlist.take() {
        let mut keep = true;
        let mut create = false;
        egui::Modal::new(egui::Id::new("new_playlist"))
            .frame(Frame::new().fill(pal().surface).corner_radius(r_lg()).inner_margin(24).stroke(Stroke::new(1.0, pal().border)))
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("New playlist").font(heavy(20.0)));
                ui.add_space(12.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text("My awesome playlist")
                        .desired_width(f32::INFINITY)
                        .margin(Margin::symmetric(10, 8)),
                );
                r.request_focus();
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    create = true;
                }
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Create").color(Color32::WHITE)).fill(pal().accent).corner_radius(16)).clicked() {
                        create = true;
                    }
                    if ui.add(egui::Button::new("Cancel").frame_when_inactive(false).corner_radius(16)).clicked() {
                        keep = false;
                    }
                });
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    keep = false;
                }
            });
        if create {
            app.create_playlist(name, track, ctx);
        } else if keep {
            app.new_playlist = Some((name, track));
        }
    }

    if let Some((idx, mut name)) = app.rename_playlist.take() {
        let mut keep = true;
        let mut done = false;
        egui::Modal::new(egui::Id::new("rename_playlist"))
            .frame(Frame::new().fill(pal().surface).corner_radius(r_lg()).inner_margin(24).stroke(Stroke::new(1.0, pal().border)))
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("Rename playlist").font(heavy(20.0)));
                ui.add_space(12.0);
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY).margin(Margin::symmetric(10, 8)));
                r.request_focus();
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    done = true;
                }
                ui.add_space(16.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Save").color(Color32::WHITE)).fill(pal().accent).corner_radius(16)).clicked() {
                        done = true;
                    }
                    if ui.add(egui::Button::new("Cancel").frame_when_inactive(false).corner_radius(16)).clicked() {
                        keep = false;
                    }
                });
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    keep = false;
                }
            });
        if done {
            if let Some(p) = app.saved.playlists.get_mut(idx)
                && !name.trim().is_empty()
            {
                p.name = name.trim().to_string();
            }
        } else if keep {
            app.rename_playlist = Some((idx, name));
        }
    }
}

/// The "Import a playlist" window.
fn import_dialog(app: &mut App, ctx: &egui::Context) {
    if !app.import_open {
        return;
    }
    let mut close = false;
    let mut start = false;
    let modal = egui::Modal::new(egui::Id::new("import")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.label(RichText::new(tr("Import a playlist")).font(heavy(20.0)).color(pal().text));
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Paste a link to a playlist or album from YouTube, Spotify or SoundCloud. Pulse finds each song and makes a playlist of it.")).font(regular(12.5)).color(pal().text_muted));
        ui.add_space(10.0);
        if let Some(job) = &app.import_running {
            let p = job.progress.lock().unwrap().clone();
            ui.horizontal(|ui| {
                ui.spinner();
                let count = if p.total > 0 { format!("  {} / {}", p.done, p.total) } else { String::new() };
                ui.label(RichText::new(format!("{}{count}", tr(&p.status))).font(med(13.0)).color(pal().text));
            });
            if p.total > 0 {
                ui.add(egui::ProgressBar::new(p.done as f32 / p.total as f32).desired_height(6.0).fill(pal().accent));
            }
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(tr("Cancel")).clicked() {
                    app.cancel_import();
                }
            });
            return;
        }
        let r = ui.add(egui::TextEdit::singleline(&mut app.import_url).hint_text("https://open.spotify.com/playlist/…").desired_width(f32::INFINITY));
        if app.import_url.is_empty() && !r.has_focus() {
            r.request_focus();
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for (glyph, name) in [(icon::YOUTUBE_LOGO, "YouTube"), (icon::SPOTIFY_LOGO, "Spotify"), (icon::SOUNDCLOUD_LOGO, "SoundCloud")] {
                ui.label(RichText::new(format!("{glyph} {name}")).font(regular(12.0)).color(pal().text_faint));
            }
        });
        ui.add_space(10.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let ok = !app.import_url.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new(RichText::new(tr("Import")).color(Color32::WHITE)).fill(pal().accent)).clicked() || (enter && ok) {
                start = true;
            }
            if ui.button(tr("Cancel")).clicked() {
                close = true;
            }
        });
    });
    if start {
        app.start_import(ctx);
    }
    if (close || modal.should_close()) && app.import_running.is_none() {
        app.import_open = false;
    }
}

/// The "View credits" window.
fn credits_dialog(app: &mut App, ctx: &egui::Context) {
    let Some((t, data)) = &app.credits else { return };
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("credits")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.horizontal(|ui| {
            let (art, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
            artwork(ui, art, t.art_small(), CornerRadius::same(6));
            ui.vertical(|ui| {
                ui.label(RichText::new("Credits").font(heavy(18.0)).color(pal().text));
                ui.label(RichText::new(&t.title).font(med(13.5)).color(pal().text_muted));
            });
        });
        ui.add_space(10.0);
        match data {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Loading credits…").font(regular(13.0)).color(pal().text_muted));
                });
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).font(regular(13.0)).color(pal().text_muted));
            }
            Some(Ok(c)) => {
                let row = |ui: &mut Ui, label: &str, value: &str| {
                    ui.label(RichText::new(value).font(med(14.0)).color(pal().text));
                    ui.label(RichText::new(label).font(regular(12.0)).color(pal().text_muted));
                    ui.add_space(6.0);
                };
                if c.people.is_empty() {
                    row(ui, "Main artist", &t.artist.name);
                }
                for (name, role) in &c.people {
                    row(ui, role, name);
                }
                ui.separator();
                ui.add_space(4.0);
                if let Some(a) = &c.album {
                    row(ui, "Album", a);
                }
                if let Some(d) = &c.release_date {
                    row(ui, "Released", &pretty_date(d));
                }
                if !c.genres.is_empty() {
                    row(ui, "Genre", &c.genres.join(", "));
                }
                if let Some(l) = &c.label {
                    row(ui, "Label", l);
                }
                if let Some(b) = c.bpm {
                    row(ui, "Tempo", &format!("{b:.0} BPM"));
                }
                if let Some(i) = &c.isrc {
                    row(ui, "ISRC", i);
                }
            }
        }
        ui.add_space(6.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        app.credits = None;
    }
}

fn toast(app: &App, ctx: &egui::Context) {
    let Some((msg, at)) = &app.toast else { return };
    let age = ctx.input(|i| i.time) - at;
    let alpha = if age < 0.15 { age / 0.15 } else if age > 3.0 { (3.5 - age) / 0.5 } else { 1.0 }.clamp(0.0, 1.0) as f32;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("toast"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -104.0))
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_max_width(screen.width() * 0.6);
            ui.multiply_opacity(alpha);
            Frame::new()
                .fill(pal().text)
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(16, 10))
                .shadow(egui::Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(120) })
                .show(ui, |ui| {
                    ui.label(RichText::new(msg).font(med(13.0)).color(pal().bg));
                });
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(30));
}
