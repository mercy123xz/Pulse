//! Everything you see: sidebar, top bar, pages, the player bar and pop-ups.

use crate::app::{App, Page, Remote, Repeat, Route};
use crate::audio::LoadState;
use crate::deezer::{AlbumInfo, ArtistInfo, PlaylistInfo, Track};
use crate::i18n::tr;
use crate::theme::*;
use crate::widgets::*;

mod diag_page;
pub mod intro;
mod lyrics_page;
pub mod mini;
mod online;
mod personal;
pub mod pick_artists;
mod settings_page;
mod song_page;

use egui::{Align, Align2, Color32, CornerRadius, Frame, Key, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use egui_phosphor::regular as icon;
pub use personal::ProfileForm;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    shortcuts(app, &ctx);
    let lay = *layout();
    let pt = parts();

    let player_fill = pt.player_bar.fill.unwrap_or(pal().player);
    let compact = lay.sidebar == crate::theme::SidebarStyle::Icons;
    let sidebar_fill = pt.sidebar.fill.unwrap_or(pal().sidebar);
    let player = egui::Panel::bottom("player")
        .exact_size(88.0)
        .frame(Frame::new().fill(player_fill).inner_margin(Margin::symmetric(pt.player_bar.padding.unwrap_or(16.0) as i8, 0)))
        .show_separator_line(false)
        .show(ui, |ui| player_bar(app, ui));
    let top_line = pt.player_bar.border.unwrap_or(Stroke::new(1.0, pal().border));
    ui.painter().hline(player.response.rect.x_range(), player.response.rect.top(), top_line);

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
    // Lyrics beside whatever page is open.
    if lyrics_page::panel_open(app, ui.ctx()) {
        let panel = egui::Panel::right("lyrics_panel")
            .exact_size(lyrics_page::PANEL_WIDTH)
            .resizable(false)
            .frame(Frame::new().fill(sidebar_fill).inner_margin(Margin { left: 16, right: 14, top: 16, bottom: 0 }))
            .show_separator_line(false)
            .show(ui, |ui| lyrics_page::lyrics_panel(app, ui));
        if !lay.inset {
            ui.painter().vline(panel.response.rect.left(), panel.response.rect.y_range(), Stroke::new(1.0, pal().border));
        }
    }

    let header_fill = pt.header.fill.unwrap_or(pal().bg);
    if lay.inset {
        // The main area floats as a rounded panel; the window behind it uses the sidebar color.
        egui::CentralPanel::default().frame(Frame::new().fill(sidebar_fill).inner_margin(Margin { left: 0, right: 8, top: 8, bottom: 8 })).show(ui, |ui| {
            Frame::new().fill(pal().bg).corner_radius(r_lg()).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                paint_backdrop(app, ui, ui.max_rect(), r_lg(), Some(sidebar_fill));
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
        egui::CentralPanel::default().frame(Frame::new().fill(pal().bg)).show(ui, |ui| {
            paint_backdrop(app, ui, ui.max_rect(), CornerRadius::ZERO, None);
            main_scroll(app, ui);
        });
    }

    dialogs(app, &ctx);
    toast(app, &ctx);
    intro::show(app, &ctx);
}

/// What the player bar says while a song loads.
fn loading_text(msg: &str) -> String {
    match msg.rsplit_once(' ').filter(|(_, pct)| pct.ends_with('%')) {
        Some((_, pct)) => format!("{} {pct}", tr("Loading…")),
        None => tr("Loading…").to_string(),
    }
}

/// The song-colored glows behind the main area, for themes that ask for them.
fn paint_backdrop(app: &mut App, ui: &Ui, rect: Rect, radius: CornerRadius, outside: Option<Color32>) {
    if !layout().backdrop || !crate::perf::on(crate::perf::Extra::ListArtwork) || crate::perf::software() {
        return;
    }
    let cover = app.current.as_ref().and_then(|t| t.art_small()).map(str::to_owned);
    app.backdrop.set_cover(cover.as_deref(), ui.ctx());
    let playing = app.is_playing();
    let moving = playing && app.saved.settings.moving_background && !reduce_motion();
    app.backdrop.paint(ui, rect, radius, outside, moving);
}

/// A strip above the page while an update is ready, downloading or installing.
fn update_banner(app: &mut App, ui: &mut Ui) {
    use crate::update::Status;
    let ctx = ui.ctx().clone();
    let (text, progress, actions) = match &app.updater.status {
        Status::Available(r) if !app.updater.banner_closed => (format!("{} {} {}", tr("Pulse"), r.version, tr("is available.")), None, true),
        Status::Downloading(r, p) => {
            let p = p.lock().map(|v| *v).unwrap_or(0.0);
            (format!("{} {}… {:.0}%", tr("Downloading Pulse"), r.version, p * 100.0), Some(p), false)
        }
        Status::Installing => (tr("Installing the update. Pulse restarts in a moment.").to_string(), None, false),
        Status::InstallFailed(_, e) if !app.updater.banner_closed => (e.clone(), None, true),
        _ => return,
    };
    let failed = matches!(app.updater.status, Status::InstallFailed(..));
    let frame = Frame::new()
        .fill(pal().surface)
        .stroke(Stroke::new(1.0, pal().accent.gamma_multiply(0.6)))
        .corner_radius(r_md())
        .inner_margin(Margin::symmetric(14, 8))
        .outer_margin(Margin { left: 28, right: 28, top: 4, bottom: 6 });
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), Layout::left_to_right(Align::Center), |ui| {
            let glyph = if failed { egui_phosphor::regular::WARNING } else { egui_phosphor::regular::ARROW_CIRCLE_UP };
            ui.label(RichText::new(glyph).size(18.0).color(pal().accent));
            ui.label(RichText::new(text).font(med(13.5)).color(pal().text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if actions {
                    if icon_button(ui, egui_phosphor::regular::X, 16.0, pal().text_muted, "Not now").clicked() {
                        app.updater.banner_closed = true;
                    }
                    let label = if failed { tr("Try again") } else { tr("Update now") };
                    if ui.add(egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(pal().accent).corner_radius(14)).clicked() {
                        app.start_update(&ctx);
                    }
                    let page = match &app.updater.status {
                        Status::Available(r) | Status::InstallFailed(r, _) => r.page.clone(),
                        _ => String::new(),
                    };
                    if ui.add(egui::Button::new(tr("What's new")).frame_when_inactive(false).corner_radius(14)).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(page));
                    }
                } else if let Some(p) = progress {
                    ui.add(egui::ProgressBar::new(p).desired_width(160.0).desired_height(6.0).fill(pal().accent));
                } else {
                    ui.add(egui::Spinner::new().size(16.0).color(pal().accent));
                }
            });
        });
    });
}

fn main_scroll(app: &mut App, ui: &mut Ui) {
    update_banner(app, ui);
    egui::ScrollArea::vertical().id_salt(("page", format!("{:?}", app.route))).auto_shrink(false).show(ui, |ui| {
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
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::G)) {
        app.toggle_gaming(ctx);
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::M)) {
        app.saved.settings.mini_open = !app.saved.settings.mini_open;
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
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::L)) {
            lyrics_page::toggle(app, ctx);
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
        paint_icon(ui.painter(), r.center(), icon::WAVEFORM, filled_icon(18.0), Color32::WHITE);
    } else {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
            vgradient_rounded(ui, r, pal().accent_soft, pal().accent);
            paint_icon(ui.painter(), r.center(), icon::WAVEFORM, filled_icon(18.0), Color32::WHITE);
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

    ui.add_space(14.0);
    nav_heading(ui, "Library");
    let liked_n = app.saved.liked.len();
    if nav_item_art(ui, Some(LIKED_ART), icon::HEART, "Liked Songs", app.route == Route::Liked, Some(liked_n)).clicked() {
        app.navigate(Route::Liked, &ctx);
    }
    let (albums_n, lists_n) = (app.saved.liked_albums.len(), app.saved.liked_playlists.len());
    if nav_item(ui, icon::VINYL_RECORD, "Liked Albums", app.route == Route::LikedAlbums, Some(albums_n)).clicked() {
        app.navigate(Route::LikedAlbums, &ctx);
    }
    if nav_item(ui, icon::PLAYLIST, "Liked Playlists", app.route == Route::LikedPlaylists, Some(lists_n)).clicked() {
        app.navigate(Route::LikedPlaylists, &ctx);
    }
    let following_n = app.saved.followed_artists.len();
    if nav_item(ui, icon::USER, "Your Artists", app.route == Route::FollowedArtists, Some(following_n)).clicked() {
        app.navigate(Route::FollowedArtists, &ctx);
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
    if let Some((t, _)) = app.downloading.first()
        && !compact
    {
        // One line for everything downloading now: the first song's name and how far along they are together.
        let p = app.downloading.iter().map(|(_, p)| *p.lock().unwrap()).sum::<f32>() / app.downloading.len() as f32;
        let left = app.download_queue.len() + app.downloading.len() - 1;
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
    // Room at the very bottom for the "Music info by Deezer" credit.
    let credit_h = if compact { 44.0 } else { 52.0 };

    egui::ScrollArea::vertical().id_salt("sidebar_playlists").auto_shrink([false, true]).max_height((ui.available_height() - art_h - credit_h).max(40.0)).show(ui, |ui| {
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
            let art = app.playlist_art(i);
            let resp = nav_item_art(ui, art.as_deref(), icon::PLAYLIST, &name, app.route == Route::Local(i), Some(count));
            if resp.clicked() {
                app.navigate(Route::Local(i), &ctx);
            }
            resp.context_menu(|ui| {
                menu_begin(ui);
                if menu_item(ui, icon::PENCIL_SIMPLE, "Rename") {
                    app.rename_playlist = Some((i, name.clone()));
                }
                if menu_item(ui, icon::SHARE_FAT, "Copy share code") {
                    app.share_playlist(i, &ctx);
                }
                menu_separator(ui);
                if menu_item(ui, icon::TRASH, "Delete") {
                    app.confirm_delete = Some(i);
                }
            });
        }
    });
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        // Where the song info comes from: a footer row under a thin line, lined up with the
        // rows above it (it used to sit in the top bar).
        let tip = format!("{}\n{}", tr("Music info by Deezer"), "Song info, artwork and charts from Deezer. Audio from YouTube.");
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), if compact { 36.0 } else { 34.0 }), Sense::click());
        a11y(ui, &resp, egui::WidgetType::Link, || tr("Music info by Deezer").to_owned());
        let hot = resp.hovered();
        if hot {
            ui.painter().rect_filled(row, r_md(), pal().surface_hover);
        }
        if compact {
            paint_icon(ui.painter(), row.center(), icon::INFO, regular(17.0), if hot { pal().text } else { pal().text_faint });
        } else {
            paint_icon(ui.painter(), pos2(row.left() + 22.0, row.center().y), icon::INFO, regular(16.0), if hot { pal().text_muted } else { pal().text_faint });
            // "Music info by" quietly, "Deezer" a step brighter, like a credit.
            let mut job = egui::text::LayoutJob::default();
            let by = tr("Music info by Deezer");
            let (lead, name) = by.rsplit_once(' ').unwrap_or(("", by));
            job.append(&format!("{lead} "), 0.0, egui::TextFormat::simple(med(12.0), if hot { pal().text_muted } else { pal().text_faint }));
            job.append(name, 0.0, egui::TextFormat::simple(semi(12.0), if hot { pal().text } else { pal().text_muted }));
            let g = ui.painter().layout_job(job);
            ui.painter().galley(pos2(row.left() + 44.0, row.center().y - g.size().y / 2.0), g, pal().text);
            if hot {
                paint_icon(ui.painter(), pos2(row.right() - 16.0, row.center().y), icon::ARROW_UP_RIGHT, regular(13.0), pal().text_muted);
            }
        }
        // A thin line above it, like the edge of a footer.
        ui.add_space(8.0);
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().hline(line.x_range().shrink(6.0), line.center().y, Stroke::new(1.0, pal().border));
        if resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab("https://www.deezer.com"));
        }
        if big_art && let Some(t) = app.current.clone() {
            ui.add_space(8.0);
            let w = ui.available_width();
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(w), Sense::click());
            artwork(ui, r, t.art_big(), r_lg());
            a11y(ui, &resp, egui::WidgetType::Button, || format!("{}: {} by {}", tr("Now playing"), t.title, t.all_artists()));
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.navigate(Route::Queue, &ctx);
            }
        }
    });
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
    nav_item_art(ui, None, glyph, label, selected, badge)
}

/// A sidebar row. With `art` (a playlist's cover, the Liked Songs heart) the picture
/// takes the icon's place.
fn nav_item_art(ui: &mut Ui, art: Option<&str>, glyph: &str, label: &str, selected: bool, badge: Option<usize>) -> egui::Response {
    if compact_sidebar() {
        // Shorter words fit under the icons; the full name is in the tooltip.
        let full = tr(label);
        let short = tr(match label {
            "Liked Songs" => "Liked",
            "Liked Albums" => "Albums",
            "Liked Playlists" => "Lists",
            "Your Artists" => "Artists",
            "Recently Played" => "Recent",
            "Local Files" => "Files",
            "Create playlist" => "New",
            "Import playlist" => "Import",
            other => other,
        });
        // Icon over a small label, so the narrow sidebar still says what each button is.
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, full));
        focus_ring(ui, &resp);
        let fill = if selected {
            pal().surface_active
        } else if resp.hovered() {
            pal().surface_hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, r_md(), fill);
        let top = pos2(rect.center().x, rect.top() + 18.0);
        if art.is_some() {
            artwork(ui, Rect::from_center_size(top, Vec2::splat(24.0)), art, CornerRadius::same(5));
        } else {
            let (font, col) = if selected { (filled_icon(19.0), pal().accent_soft) } else { (regular(19.0), pal().text_muted) };
            paint_icon(ui.painter(), top, glyph, font, col);
        }
        let text_col = if selected || resp.hovered() { pal().text } else { pal().text_muted };
        let g = one_line(ui, short, med(9.5), text_col, rect.width() - 6.0);
        ui.painter().galley(pos2(rect.center().x - g.size().x / 2.0, rect.bottom() - 8.0 - g.size().y), g, text_col);
        return resp.on_hover_text(full).on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    let label = tr(label);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), if art.is_some() { 40.0 } else { 36.0 }), Sense::click());
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
    let icon_c = pos2(rect.left() + 22.0, rect.center().y);
    if art.is_some() {
        artwork(ui, Rect::from_center_size(icon_c, Vec2::splat(28.0)), art, CornerRadius::same(5));
    } else {
        let icon_col = if selected { pal().accent_soft } else { pal().text_muted };
        let font = if selected { filled_icon(17.0) } else { regular(17.0) };
        paint_icon(ui.painter(), icon_c, glyph, font, icon_col);
    }
    let text_col = if selected || resp.hovered() { pal().text } else { pal().text_muted };
    let max_w = rect.width() - 80.0;
    let g = one_line(ui, label, med(13.5), text_col, max_w);
    ui.painter().galley(pos2(rect.left() + 44.0, rect.center().y - g.size().y / 2.0), g, text_col);
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

/// One line in the search dropdown.
enum Suggestion {
    Recent(String),
    /// A search to finish what you're typing.
    Query(String),
    Pick(Box<crate::suggest::Pick>),
}

/// Under the search box while it's in use. Empty: your recent searches. While typing:
/// searches that finish what you typed (yours, then what other people search for), then
/// the artist you most likely mean and a mix of their music, with what's in your library
/// first (see `suggest`). ↑/↓ choose, Enter opens. True if Enter picked a suggestion.
fn search_dropdown(app: &mut App, ui: &mut Ui, pill: Rect, edit: &egui::Response) -> bool {
    use crate::suggest::Pick;
    let ctx = ui.ctx().clone();
    let area_id = egui::Id::new("search_dropdown");
    let last_rect: Option<Rect> = ui.data(|d| d.get_temp(area_id.with("rect")));
    let over = last_rect.is_some_and(|r| ui.rect_contains_pointer(r) || ctx.pointer_hover_pos().is_some_and(|p| r.contains(p)));
    // Developer check: PULSE_DEBUG_DROPDOWN keeps it open for screenshots.
    if !edit.has_focus() && !over && std::env::var_os("PULSE_DEBUG_DROPDOWN").is_none() {
        ui.data_mut(|d| d.remove::<Rect>(area_id.with("rect")));
        return false;
    }
    let q = app.search_text.trim().to_string();
    if !q.is_empty() {
        app.want_popular_searches(&q, &ctx);
    }
    let lib = crate::suggest::Library {
        liked_songs: app.liked_ids.iter().map(String::as_str).collect(),
        liked_albums: app.saved.liked_albums.iter().map(|a| a.id.as_str()).collect(),
        liked_playlists: app.saved.liked_playlists.iter().map(|p| p.id.as_str()).collect(),
        followed: app.saved.followed_artists.iter().map(|a| a.id.as_str()).collect(),
        played_artists: app
            .saved
            .played_artists
            .iter()
            .filter_map(|v| match &v.place {
                crate::app::Place::Artist(id) => Some((id.as_str(), v.count)),
                _ => None,
            })
            .collect(),
        recent_songs: app.saved.recent.iter().take(50).map(|t| t.id.as_str()).collect(),
    };
    let mut items: Vec<Suggestion> = vec![];
    if q.is_empty() {
        for r in &app.saved.recent_searches {
            items.push(Suggestion::Recent(r.clone()));
        }
    } else {
        let results = app.search_results_for(&q);
        let popular = app.popular_searches.get(&q.to_lowercase()).map(Vec::as_slice).unwrap_or(&[]);
        for c in crate::suggest::completions(&q, &app.saved.recent_searches, popular, results, 4) {
            items.push(Suggestion::Query(c));
        }
        if let Some(r) = results {
            for p in crate::suggest::picks(&q, r, &lib, 6) {
                items.push(Suggestion::Pick(Box::new(p)));
            }
        }
    }
    let saved: Vec<bool> = items.iter().map(|s| matches!(s, Suggestion::Pick(p) if p.saved(&lib))).collect();
    drop(lib);
    let searching = !q.is_empty()
        && !items.iter().any(|s| matches!(s, Suggestion::Pick(_)))
        && (app.search_edited_at.is_some() || matches!(app.cache.get(&format!("search:{q}")), Some(Remote::Loading)));
    if items.is_empty() && !searching {
        return false;
    }
    // Keyboard choice.
    let sel_id = area_id.with("sel");
    let mut sel: Option<usize> = ui.data(|d| d.get_temp(sel_id)).flatten();
    if edit.has_focus() {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)) {
            sel = Some(sel.map_or(0, |s| (s + 1).min(items.len().saturating_sub(1))));
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)) {
            sel = sel.and_then(|s| s.checked_sub(1));
        }
    }
    sel = sel.filter(|s| *s < items.len());
    let mut chosen: Option<usize> = None;
    // The Follow / save button on a row (pressed without opening the row).
    let mut toggled: Option<usize> = None;
    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if enter {
        chosen = sel;
    }
    let mut clear_history = false;
    let area = egui::Area::new(area_id).order(egui::Order::Foreground).fixed_pos(pill.left_bottom() + vec2(0.0, 6.0)).show(&ctx, |ui| {
        Frame::menu(ui.style()).inner_margin(6).show(ui, |ui| {
            ui.set_width(pill.width() - 12.0);
            ui.spacing_mut().item_spacing.y = 1.0;
            if q.is_empty() {
                ui.horizontal(|ui| {
                    menu_header(ui, "Recent searches");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Label::new(RichText::new(tr("Clear history")).font(med(11.5)).color(pal().text_muted)).sense(Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            clear_history = true;
                        }
                    });
                });
            }
            for (k, s) in items.iter().enumerate() {
                // A thin line between the searches and the results.
                if k > 0 && matches!(s, Suggestion::Pick(_)) && matches!(items[k - 1], Suggestion::Query(_)) {
                    ui.add_space(4.0);
                    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                    ui.painter().hline(line.x_range().shrink(8.0), line.center().y, Stroke::new(1.0, pal().border));
                    ui.add_space(4.0);
                }
                let tall = matches!(s, Suggestion::Pick(_));
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), if tall { 52.0 } else { 34.0 }), Sense::click());
                let hot = resp.hovered() || sel == Some(k);
                if hot {
                    ui.painter().rect_filled(rect, r_sm(), pal().surface_hover);
                }
                let mut x = rect.left() + 8.0;
                let mut right = rect.right() - 8.0;
                match s {
                    Suggestion::Recent(text) | Suggestion::Query(text) => {
                        let glyph = if matches!(s, Suggestion::Recent(_)) { icon::CLOCK_COUNTER_CLOCKWISE } else { icon::MAGNIFYING_GLASS };
                        paint_icon(ui.painter(), pos2(x + 9.0, rect.center().y), glyph, regular(15.0), pal().text_muted);
                        x += 30.0;
                        // What you typed in bold, the rest plain, like music apps.
                        let typed = text.len().min(q.len());
                        let bold = text.get(..typed).filter(|p| crate::app::fold(p) == crate::app::fold(&q)).map(|_| typed).unwrap_or(0);
                        let mut job = egui::text::LayoutJob::default();
                        job.append(&text[..bold], 0.0, egui::TextFormat::simple(semi(13.5), pal().text));
                        job.append(&text[bold..], 0.0, egui::TextFormat::simple(regular(13.5), pal().text_muted));
                        let g = ui.painter().layout_job(job);
                        ui.painter().galley(pos2(x, rect.center().y - g.size().y / 2.0), g, pal().text);
                    }
                    Suggestion::Pick(p) => {
                        let (title, sub, art, round) = match p.as_ref() {
                            Pick::Artist(a) => (a.name.clone(), tr("Artist").to_string(), a.picture.clone(), true),
                            Pick::Album(a) => {
                                (a.title.clone(), format!("{} • {}", tr(a.kind()), a.artist.name), a.cover_small.clone().or(a.cover.clone()), false)
                            }
                            Pick::Playlist(p) => (p.title.clone(), format!("{} • {}", tr("Playlist"), p.creator), p.picture.clone(), false),
                            Pick::Song(t) => (t.title.clone(), format!("{} • {}", tr("Song"), t.all_artists()), t.art_small().map(str::to_owned), false),
                        };
                        let r = Rect::from_min_size(pos2(x, rect.center().y - 20.0), Vec2::splat(40.0));
                        artwork(ui, r, art.as_deref(), if round { CornerRadius::same(20) } else { r_sm() });
                        x += 52.0;
                        // Right side: Follow for artists, add / saved for the rest.
                        let on = saved[k];
                        if let Pick::Artist(_) = p.as_ref() {
                            let b = Rect::from_center_size(pos2(right - 42.0, rect.center().y), vec2(84.0, 30.0));
                            let br = ui.interact(b, area_id.with(("follow", k)), Sense::click());
                            ui.painter().rect_stroke(
                                b,
                                CornerRadius::same(15),
                                Stroke::new(1.0, if br.hovered() { pal().text } else { pal().text_faint }),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().text(b.center(), Align2::CENTER_CENTER, tr(if on { "Following" } else { "Follow" }), semi(12.0), pal().text);
                            if br.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                toggled = Some(k);
                            }
                            right = b.left() - 10.0;
                        } else {
                            let b = Rect::from_center_size(pos2(right - 12.0, rect.center().y), Vec2::splat(26.0));
                            let br = ui.interact(b, area_id.with(("save", k)), Sense::click());
                            let (glyph, font, col) = if on {
                                (icon::CHECK_CIRCLE, filled_icon(20.0), pal().accent_soft)
                            } else {
                                (icon::PLUS_CIRCLE, regular(20.0), if br.hovered() { pal().text } else { pal().text_muted })
                            };
                            paint_icon(ui.painter(), b.center(), glyph, font, col);
                            let tip = tr(if on { "Remove from your library" } else { "Save to your library" });
                            if br.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                toggled = Some(k);
                            }
                            right = b.left() - 8.0;
                        }
                        let w = right - x;
                        paint_line(ui, pos2(x, rect.center().y - 16.0), &title, med(13.5), pal().text, w);
                        paint_line(ui, pos2(x, rect.center().y + 2.0), &sub, regular(12.0), pal().text_muted, w);
                    }
                }
                if toggled != Some(k) && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    chosen = Some(k);
                }
            }
            if searching {
                ui.add_space(4.0);
                ui.label(RichText::new(tr("Searching…")).font(regular(12.5)).color(pal().text_muted));
            }
        });
    });
    ui.data_mut(|d| {
        d.insert_temp(area_id.with("rect"), area.response.rect);
        d.insert_temp(sel_id, sel);
    });
    if clear_history {
        app.saved.recent_searches.clear();
    }
    if let Some(k) = toggled
        && let Suggestion::Pick(p) = &items[k]
    {
        match p.as_ref() {
            Pick::Artist(a) => app.toggle_follow(a, &ctx),
            Pick::Album(a) => app.toggle_album(a, &ctx),
            Pick::Playlist(pl) => app.toggle_playlist(pl, &ctx),
            Pick::Song(t) => app.toggle_like(t, &ctx),
        }
        return false;
    }
    let Some(k) = chosen else { return false };
    ui.data_mut(|d| d.insert_temp(sel_id, None::<usize>));
    ui.data_mut(|d| d.remove::<Rect>(area_id.with("rect")));
    ui.memory_mut(|m| m.surrender_focus(edit.id));
    match &items[k] {
        Suggestion::Recent(r) | Suggestion::Query(r) => {
            app.search_text = r.clone();
            app.search_edited_at = None;
            app.remember_search(r);
            app.submit_search(&ctx);
        }
        Suggestion::Pick(p) => {
            app.remember_search(&q);
            match p.as_ref() {
                Pick::Artist(a) => app.navigate(Route::Artist(a.id.clone()), &ctx),
                Pick::Album(a) => app.navigate(Route::Album(a.id.clone()), &ctx),
                Pick::Playlist(pl) => app.navigate(Route::Playlist(pl.id.clone()), &ctx),
                Pick::Song(t) => app.open_song(t, &ctx),
            }
        }
    }
    true
}

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

        // Search: a wide pill (the buttons on the right are icons, so there's room). Pointing
        // at it lifts it a little; while you type it has an accent ring and a soft glow.
        let w = (ui.available_width() - 200.0).clamp(240.0, 560.0);
        let (rect, pill) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click());
        let id = egui::Id::new("search_box");
        let focused = ui.memory(|m| m.has_focus(id));
        let hot = focused || ui.rect_contains_pointer(rect);
        if focused {
            ui.painter().add(egui::Shadow { offset: [0, 0], blur: 16, spread: 1, color: pal().accent.gamma_multiply(0.30) }.as_shape(rect, CornerRadius::same(20)));
        }
        let stroke = if focused { Stroke::new(1.5, pal().accent_soft) } else { Stroke::new(1.0, if hot { pal().text_faint } else { pal().border }) };
        ui.painter().rect(rect, CornerRadius::same(20), if hot { pal().surface_hover } else { pal().surface }, stroke, egui::StrokeKind::Inside);
        paint_icon(ui.painter(), pos2(rect.left() + 21.0, rect.center().y), icon::MAGNIFYING_GLASS, regular(17.0), if focused { pal().text } else { pal().text_muted });
        // A click anywhere on the pill (the icon, the padding) starts typing.
        if pill.on_hover_cursor(egui::CursorIcon::Text).clicked() {
            ui.memory_mut(|m| m.request_focus(id));
        }

        let edit_rect = Rect::from_min_max(pos2(rect.left() + 42.0, rect.top()), pos2(rect.right() - 64.0, rect.bottom()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(edit_rect).layout(Layout::left_to_right(Align::Center)));
        let resp = child.add(
            egui::TextEdit::singleline(&mut app.search_text)
                .id(id)
                .frame(Frame::NONE)
                .hint_text(RichText::new(tr("What do you want to listen to?")).color(pal().text_faint))
                .font(regular(14.0))
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
        let picked = search_dropdown(app, ui, rect, &resp);
        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !picked {
            app.search_edited_at = None;
            let q = app.search_text.trim().to_string();
            app.remember_search(&q);
            app.submit_search(&ctx);
        }
        // Clear button, or the Ctrl K shortcut as a little key cap.
        let right = pos2(rect.right() - 14.0, rect.center().y);
        if !app.search_text.is_empty() {
            let r = Rect::from_center_size(right - vec2(10.0, 0.0), Vec2::splat(26.0));
            let c = ui.interact(r, id.with("clear"), Sense::click());
            a11y(ui, &c, egui::WidgetType::Button, || "Clear search".to_owned());
            if c.hovered() {
                ui.painter().circle_filled(r.center(), 13.0, pal().surface_active);
            }
            paint_icon(ui.painter(), r.center(), icon::X, regular(14.0), if c.hovered() { pal().text } else { pal().text_muted });
            if c.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.search_text.clear();
                ui.memory_mut(|m| m.request_focus(id));
            }
        } else if !focused {
            let keycap = Rect::from_min_max(pos2(right.x - 44.0, right.y - 11.0), pos2(right.x, right.y + 11.0));
            ui.painter().rect(keycap, CornerRadius::same(6), pal().surface_active, Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
            ui.painter().hline(keycap.x_range().shrink(4.0), keycap.bottom() - 1.5, Stroke::new(1.0, pal().border));
            paint_icon(ui.painter(), keycap.center() - vec2(0.0, 0.5), "Ctrl K", med(10.5), pal().text_muted);
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
            ui.add_space(6.0);
            // Icons only (the tooltips say what each does), so the search box has the room.
            let on_settings = app.route == Route::Settings;
            if top_icon(ui, icon::GEAR_SIX, on_settings, tr("Settings")).clicked() {
                app.navigate(Route::Settings, &ctx);
            }
            // Gaming Mode: one click on or off; the tooltip says what's paused.
            let mode = crate::perf::mode();
            let gaming = mode != crate::perf::Mode::Normal;
            let tip = if gaming {
                let off: Vec<&str> = crate::perf::switched_off().into_iter().map(|e| tr(e.label())).collect();
                format!("{} (Ctrl+G)\n{}\n• {}", tr(mode.label()), tr("Paused while it's on (music plays the same):"), off.join("\n• "))
            } else {
                format!("{} (Ctrl+G)", tr("Gaming Mode: pause the extras while you play"))
            };
            if top_icon(ui, icon::GAME_CONTROLLER, gaming, &tip).clicked() {
                app.toggle_gaming(&ctx);
            }
            if app.theme_has_light_and_dark() {
                let (glyph, tip) = if app.saved.light_mode { (icon::MOON, "Switch to dark mode") } else { (icon::SUN, "Switch to light mode") };
                if top_icon(ui, glyph, false, tr(tip)).clicked() {
                    app.toggle_theme(&ctx);
                }
            }
        });
    });
}

/// A round icon button for the top bar. It stays lit while its mode is on; the tooltip (its
/// screen-reader name too) says what it does.
fn top_icon(ui: &mut Ui, glyph: &str, on: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
    a11y(ui, &resp, egui::WidgetType::Button, || tip.lines().next().unwrap_or(tip).to_owned());
    let hovered = resp.hovered();
    let h = ui.ctx().animate_bool(resp.id.with("hover_fade"), hovered);
    if on {
        ui.painter().circle_filled(rect.center(), 19.0, pal().surface_active);
    } else if h > 0.0 {
        ui.painter().circle_filled(rect.center(), 15.0 + 4.0 * h, pal().surface_hover.gamma_multiply(h));
    }
    let col = if on { pal().accent_soft } else { pal().text_muted.lerp_to_gamma(pal().text, h) };
    paint_icon(ui.painter(), rect.center(), glyph, regular(19.0), col);
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
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
        Route::Diagnostics => diag_page::diagnostics_page(app, ui),
        Route::Mix(id) => personal::mix_page(app, ui, &id),
        Route::Song(id) => song_page::song_page(app, ui, &id),
        Route::Profile => personal::profile_page(app, ui),
        Route::Public(_) => online::public_page(app, ui),
        Route::User(_) => online::user_page(app, ui),
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
            let sub = song_count(tracks.len());
            local_collection(app, ui, "Liked Songs", &sub, icon::HEART, &tracks, None);
        }
        Route::LikedAlbums => {
            page_title(ui, "Liked Albums", None);
            let list = app.saved.liked_albums.clone();
            if list.is_empty() {
                empty_state(ui, icon::VINYL_RECORD, "No liked albums yet", "Tap the heart on an album's page to save it here.", None);
            } else {
                album_grid(app, ui, &list, None);
            }
        }
        Route::FollowedArtists => {
            page_title(ui, "Your Artists", None);
            let list = app.saved.followed_artists.clone();
            if list.is_empty() {
                empty_state(ui, icon::USER, "You're not following anyone yet", "Press Follow on an artist's page or in search to see them here.", None);
            } else {
                artist_grid(app, ui, &list, None);
            }
        }
        Route::LikedPlaylists => {
            page_title(ui, "Liked Playlists", None);
            let list = app.saved.liked_playlists.clone();
            if list.is_empty() {
                empty_state(ui, icon::PLAYLIST, "No liked playlists yet", "Tap the heart on a playlist's page to save it here.", None);
            } else {
                playlist_grid(app, ui, &list, None);
            }
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
                let sub = format!("Your playlist · {} · {}", song_count(p.tracks.len()), total_time(&p.tracks));
                local_collection(app, ui, &p.name, &sub, icon::PLAYLIST, &p.tracks, Some(i));
            }
        }
        Route::Queue => queue_page(app, ui),
        Route::Lyrics => lyrics_page::lyrics_page(app, ui),
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
    app.ensure_featured(tracks);
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

/// Rows built from your artists and what you play ("Listeners of X also listen to",
/// "Songs like Y") first, then today's top songs; until there are any, top songs and
/// popular albums, artists and playlists.
fn home_discovery(app: &mut App, ui: &mut Ui) {
    with_remote(app, ui, |app, ui, page| {
        let Page::Home(chart) = page else { return };
        for (k, row) in chart.for_you.iter().enumerate() {
            suggestion_row(app, ui, &format!("home_for_you_{k}"), row);
        }
        if section_title(ui, "Top songs right now", Some("See all")) {
            app.chart_genre = (0, "All genres".into());
            let ctx = ui.ctx().clone();
            app.navigate(Route::Charts, &ctx);
        }
        tracks_block(app, ui, "home_top", &chart.tracks);
        if !chart.for_you.is_empty() {
            return;
        }
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

/// One row of suggestions: artists that listeners of X also play, or songs like Y.
fn suggestion_row(app: &mut App, ui: &mut Ui, salt: &str, row: &crate::deezer::Suggestion) {
    match row {
        crate::deezer::Suggestion::AlsoListen(name, artists) => {
            section_title(ui, &tr("Listeners of {} also listen to").replace("{}", name), None);
            artist_grid(app, ui, artists, Some(1));
        }
        crate::deezer::Suggestion::SongsLike(name, tracks) => {
            section_title(ui, &tr("Songs like {}").replace("{}", name), None);
            tracks_block(app, ui, salt, &tracks[..tracks.len().min(10)]);
        }
    }
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
    let mut preview = None;
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
            CardClick::Preview => preview = Some(a.id.clone()),
            CardClick::None => {}
        }
    });
    if let Some(id) = preview {
        app.preload(Route::Album(id), &ctx);
    }
    match action {
        Some((id, false)) => app.navigate(Route::Album(id), &ctx),
        Some((id, true)) => open_and_play(app, Route::Album(id), &ctx),
        None => {}
    }
}

fn playlist_grid(app: &mut App, ui: &mut Ui, playlists: &[PlaylistInfo], rows: Option<usize>) {
    let ctx = ui.ctx().clone();
    let mut action = None;
    let mut preview = None;
    card_grid(ui, playlists, rows, |ui, w, p| {
        let sub = if !p.creator.is_empty() {
            format!("Playlist · {}", p.creator)
        } else if p.nb_tracks > 0 {
            song_count(p.nb_tracks as usize)
        } else {
            "Playlist".to_string()
        };
        let info = CardInfo { id: egui::Id::new(("pl", &p.id)), art: p.picture.as_deref(), title: &p.title, subtitle: sub, round: false, badge: None };
        match card(ui, w, &info) {
            CardClick::Open => action = Some((p.id.clone(), false)),
            CardClick::Play => action = Some((p.id.clone(), true)),
            CardClick::Preview => preview = Some(p.id.clone()),
            CardClick::None => {}
        }
    });
    // Pulse playlists (public ones) and Deezer's.
    let route = |id: String| match id.strip_prefix(crate::account::PREFIX) {
        Some(p) => Route::Public(p.to_string()),
        None => Route::Playlist(id),
    };
    if let Some(id) = preview {
        app.preload(route(id), &ctx);
    }
    match action {
        Some((id, false)) => app.navigate(route(id), &ctx),
        Some((id, true)) => open_and_play(app, route(id), &ctx),
        None => {}
    }
}

/// Cards for remembered places (home page, profile).
fn album_grid_visits(app: &mut App, ui: &mut Ui, items: &[crate::app::Visit], rows: Option<usize>) {
    let ctx = ui.ctx().clone();
    let mut action = None;
    let mut preview = None;
    card_grid(ui, items, rows, |ui, w, v| {
        let current = match &v.place {
            crate::app::Place::Yours(name) => app.saved.playlists.iter().position(|p| p.name == *name).and_then(|i| app.playlist_art(i)),
            _ => None,
        };
        let art = if v.place == crate::app::Place::Liked { Some(LIKED_ART) } else { current.as_deref().or(v.image.as_deref()) };
        // Orbits saved before the rename were called "Daily Mix 01".
        let title = match &v.place {
            crate::app::Place::Mix(id) => app
                .saved
                .played_artists
                .iter()
                .find(|a| a.place == crate::app::Place::Artist(id.clone()))
                .map(|a| personal::orbit_title(&a.title))
                .unwrap_or_else(|| v.title.clone()),
            _ => v.title.clone(),
        };
        let info = CardInfo {
            id: egui::Id::new(("visit", format!("{:?}", v.place))),
            art,
            title: &title,
            subtitle: tr(&v.subtitle).to_string(),
            round: v.round,
            badge: None,
        };
        match card(ui, w, &info) {
            CardClick::Open => action = Some((v.place.clone(), false)),
            CardClick::Play => action = Some((v.place.clone(), true)),
            CardClick::Preview => preview = Some(v.place.clone()),
            CardClick::None => {}
        }
    });
    if let Some(route) = preview.and_then(|p| app.route_for(&p)) {
        app.preload(route, &ctx);
    }
    if let Some((place, play)) = action
        && let Some(route) = app.route_for(&place)
    {
        if play { open_and_play(app, route, &ctx) } else { app.navigate(route, &ctx) }
    }
}

fn artist_grid(app: &mut App, ui: &mut Ui, artists: &[ArtistInfo], rows: Option<usize>) {
    let ctx = ui.ctx().clone();
    let mut open = None;
    let mut radio = None;
    let mut preview = None;
    card_grid(ui, artists, rows, |ui, w, a| {
        let sub = if a.fans > 0 { format!("{} fans", fmt_count(a.fans)) } else { "Artist".into() };
        let info = CardInfo { id: egui::Id::new(("artist", &a.id)), art: a.picture.as_deref(), title: &a.name, subtitle: sub, round: true, badge: None };
        match card(ui, w, &info) {
            CardClick::Open => open = Some(a.id.clone()),
            // The play button on an artist starts their radio.
            CardClick::Play => radio = Some(a.id.clone()),
            CardClick::Preview => preview = Some(a.id.clone()),
            CardClick::None => {}
        }
    });
    if let Some(id) = preview {
        app.preload(Route::Artist(id), &ctx);
    }
    if let Some(id) = open {
        app.navigate(Route::Artist(id), &ctx);
    }
    if let Some(id) = radio {
        open_and_play(app, Route::Mix(id), &ctx);
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
    let query = q.to_string();
    with_remote(app, ui, |app, ui, page| {
        let Page::Search(r) = page else { return };
        if r.is_empty() {
            empty_state(ui, icon::MAGNIFYING_GLASS, "No results", "Try different words or check the spelling.", None);
            return;
        }
        let ctx = ui.ctx().clone();
        // Filter chips: show one kind of result (no new search needed).
        let id = egui::Id::new(("search_filter", &query));
        let mut filter: u8 = ui.data(|d| d.get_temp(id).unwrap_or(0));
        let kind = |a: &AlbumInfo| a.kind();
        let albums: Vec<AlbumInfo> = r.albums.iter().filter(|a| matches!(kind(a), "Album" | "Compilation")).cloned().collect();
        let eps: Vec<AlbumInfo> = r.albums.iter().filter(|a| kind(a) == "EP").cloned().collect();
        let singles: Vec<AlbumInfo> = r.albums.iter().filter(|a| kind(a) == "Single").cloned().collect();
        let chips: [(u8, &str, bool); 7] = [
            (0, "All", true),
            (1, "Songs", !r.tracks.is_empty()),
            (2, "Artists", !r.artists.is_empty()),
            (3, "Albums", !albums.is_empty()),
            (4, "EPs", !eps.is_empty()),
            (5, "Singles", !singles.is_empty()),
            (6, "Playlists", !r.playlists.is_empty()),
        ];
        ui.horizontal_wrapped(|ui| {
            for (n, label, any) in chips {
                if any && pill(ui, tr(label), filter == n).clicked() {
                    filter = n;
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(id, filter));
        let all = filter == 0;

        // Top result: the best-ranked artist (popular and matching what you typed: "tyler" is
        // Tyler, The Creator, not a small artist called just "Tyler"), else the best song.
        if all {
            let artist = r.artists.first().filter(|a| crate::deezer::name_match(&a.name, &query) >= 2);
            match (artist, r.tracks.first()) {
                (Some(a), _) => {
                    section_title(ui, "Top result", None);
                    artist_grid(app, ui, std::slice::from_ref(a), Some(1));
                }
                (None, Some(t)) => {
                    section_title(ui, "Top result", None);
                    let names = app.playlist_names();
                    let mut ev = vec![];
                    track_table(ui, "search_top", std::slice::from_ref(t), &table_ctx(app, &names), &mut ev);
                    app.handle_events(ev, std::slice::from_ref(t), &ctx);
                }
                _ => {}
            }
        }
        if (all || filter == 2) && !r.artists.is_empty() {
            // On "All": one row of the top artists, with "See all" for the rest.
            if section_title(ui, if all { "Top artists" } else { "Artists" }, all.then_some("See all")) {
                ui.data_mut(|d| d.insert_temp(id, 2u8));
            }
            artist_grid(app, ui, &r.artists, if all { Some(1) } else { None });
        }
        // Searched for an artist: who their listeners also play.
        if let Some((name, related, _)) = &r.around
            && (all || filter == 2)
            && !related.is_empty()
        {
            suggestion_row(app, ui, "search_also", &crate::deezer::Suggestion::AlsoListen(name.clone(), related.clone()));
        }
        // The top songs start loading once the results have stayed up for a moment
        // (not for every letter while you type), so the one you press plays at once.
        if !r.tracks.is_empty() {
            let key = egui::Id::new(("search_warm", &query));
            let now = ui.input(|i| i.time);
            let since: f64 = ui.data_mut(|d| *d.get_temp_mut_or(key, now));
            if now - since >= 0.7 {
                app.warm_list(&r.tracks);
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        if (all || filter == 1) && !r.tracks.is_empty() {
            section_title(ui, "Songs", None);
            let shown = if all { &r.tracks[..r.tracks.len().min(10)] } else { &r.tracks[..] };
            tracks_block(app, ui, "search_tracks", shown);
        }
        // ...and songs by others that sound like theirs.
        if let Some((name, _, songs)) = &r.around
            && (all || filter == 1)
            && songs.len() >= 3
        {
            suggestion_row(app, ui, "search_like", &crate::deezer::Suggestion::SongsLike(name.clone(), songs.clone()));
        }
        for (n, title, list) in [(3, "Albums", &albums), (4, "EPs", &eps), (5, "Singles", &singles)] {
            if (all || filter == n) && !list.is_empty() {
                section_title(ui, title, None);
                album_grid(app, ui, list, if all { Some(1) } else { None });
            }
        }
        if all && !r.users.is_empty() {
            section_title(ui, "Profiles", None);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
                for u in r.users.iter().take(8) {
                    if ghost_button(ui, icon::USER, &format!("@{}", u.username)).clicked() {
                        app.navigate(Route::User(u.username.clone()), &ctx);
                    }
                }
            });
        }
        if (all || filter == 6) && !r.playlists.is_empty() {
            section_title(ui, "Playlists", None);
            playlist_grid(app, ui, &r.playlists, if all { Some(1) } else { None });
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
    /// Clickable names shown before the subtitle (the artists of an album or song).
    links: &'a [&'a str],
}

/// What was clicked in an editable hero.
#[derive(Default)]
struct HeroClicks {
    title: bool,
    art: Option<egui::Response>,
    /// Which of `links` was clicked.
    link: Option<usize>,
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
        paint_icon(ui.painter(), art_rect.center(), h.glyph, filled_icon(72.0), Color32::WHITE);
    }
    if h.editable {
        let r = ui.interact(art_rect, ui.id().with("hero_art"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        a11y(ui, &r, egui::WidgetType::Button, || tr("Change cover").to_owned());
        if r.hovered() {
            // Pencil overlay, like Spotify.
            ui.painter().rect_filled(art_rect, radius, Color32::from_black_alpha(140));
            paint_icon(ui.painter(), art_rect.center() - vec2(0.0, 10.0), icon::PENCIL_SIMPLE, regular(40.0), Color32::WHITE);
            paint_icon(ui.painter(), art_rect.center() + vec2(0.0, 26.0), tr("Change cover"), med(13.0), Color32::WHITE);
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
    let mut sx = tx;
    for (i, name) in h.links.iter().enumerate() {
        if i > 0 {
            let g = one_line(ui, ", ", regular(13.5), on_muted, tw);
            let w = g.size().x;
            ui.painter().galley(pos2(sx, bottom - 20.0), g, on_muted);
            sx += w;
        }
        let room = (tx + tw - sx).max(0.0);
        if room < 24.0 {
            break;
        }
        let r = link_text(ui, ui.id().with(("hero_link", i)), pos2(sx, bottom - 20.0), name, semi(13.5), on, room);
        if r.clicked() {
            clicks.link = Some(i);
        }
        sx = r.rect.right();
    }
    paint_line(ui, pos2(sx, bottom - 20.0), h.subtitle, regular(13.5), on_muted, (tx + tw - sx).max(0.0));
    ui.add_space(18.0);
    clicks
}

/// Header with a full-width photo (themes with `hero = "fullbleed"`, artist pages).
fn hero_full_bleed(ui: &mut Ui, h: &Hero, radius: CornerRadius) -> HeroClicks {
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 320.0), Sense::hover());
    // Deezer's biggest artist photo is 1000 px square. Stretching it across a wide banner
    // made it blurry, so it's never drawn wider than its own pixels: on wide windows it sits
    // on the right and fades into a dark fill on the left.
    let base = Color32::from_rgb(14, 13, 20).lerp_to_gamma(pal().accent, 0.10);
    ui.painter().rect_filled(rect, radius, base);
    let ppp = ui.ctx().pixels_per_point();
    let photo_w = rect.width().min((1000.0 / ppp).max(rect.height()));
    let photo = Rect::from_min_max(pos2(rect.right() - photo_w, rect.top()), rect.max);
    let covers_all = photo_w >= rect.width() - 0.5;
    let pr = if covers_all { radius } else { CornerRadius { nw: 0, sw: 0, ne: radius.ne, se: radius.se } };
    // Crop to the banner's shape, keeping a little more of the top, where faces usually are.
    artwork_cover(ui, photo, h.art, pr, 0.35);
    if !covers_all {
        let fade = Rect::from_min_size(photo.min, vec2(photo_w * 0.45, photo.height()));
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(fade.left_top(), base);
        mesh.colored_vertex(fade.right_top(), Color32::TRANSPARENT);
        mesh.colored_vertex(fade.left_bottom(), base);
        mesh.colored_vertex(fade.right_bottom(), Color32::TRANSPARENT);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        ui.painter().add(egui::Shape::mesh(mesh));
    }
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
        let Page::Playlist { info, tracks } = page else {
            return;
        };
        let mut sub = format!("{} · {}", song_count(tracks.len()), total_time(tracks));
        if !info.creator.is_empty() {
            sub = format!("{} · {sub}", info.creator);
        }
        hero(
            ui,
            &Hero {
                editable: false,
                kind: "Playlist",
                title: &info.title,
                subtitle: &sub,
                art: info.picture.as_deref(),
                glyph: icon::PLAYLIST,
                round: false,
                banner: None,
                links: &[],
            },
        );
        app.note_visit(crate::app::Place::Playlist(info.id.clone()), &info.title, &format!("Playlist · {}", info.creator), info.picture.as_deref(), false);
        let ctx = ui.ctx().clone();
        autoplay_page(app, &ctx, tracks);
        ui.horizontal(|ui| {
            action_row(app, ui, tracks);
            let liked = app.saved.liked_playlists.iter().any(|p| p.id == info.id);
            if save_heart(ui, liked).clicked() {
                // The page knows the real number of songs; lists sometimes don't.
                let mut info = info.clone();
                info.nb_tracks = info.nb_tracks.max(tracks.len() as u64);
                app.toggle_playlist(&info, &ctx);
            }
        });
        tracks_block(app, ui, "playlist", tracks);
    });
}

fn album_page(app: &mut App, ui: &mut Ui) {
    with_remote(app, ui, |app, ui, page| {
        let Page::Album { info, tracks } = page else {
            return;
        };
        let linked = crate::deezer::is_deezer_id(&info.artist.id);
        let mut sub = if linked { String::new() } else { info.artist.name.clone() };
        if let Some(y) = info.year() {
            sub.push_str(&format!(" · {y}"));
        }
        sub.push_str(&format!(" · {}, {}", song_count(tracks.len()), total_time(tracks)));
        let sub = if linked { sub } else { sub.trim_start_matches(" · ").to_string() };
        let names = [info.artist.name.as_str()];
        let links: &[&str] = if linked { &names } else { &[] };
        let clicks = hero(
            ui,
            &Hero {
                editable: false,
                kind: info.kind(),
                title: &info.title,
                subtitle: &sub,
                art: info.cover.as_deref(),
                glyph: icon::VINYL_RECORD,
                round: false,
                banner: None,
                links,
            },
        );
        app.note_visit(
            crate::app::Place::Album(info.id.clone()),
            &info.title,
            &format!("{} · {}", info.kind(), info.artist.name),
            info.cover.as_deref(),
            false,
        );
        let ctx = ui.ctx().clone();
        if clicks.link.is_some() {
            app.navigate(Route::Artist(info.artist.id.clone()), &ctx);
        }
        autoplay_page(app, &ctx, tracks);
        ui.horizontal(|ui| {
            action_row(app, ui, tracks);
            let liked = app.saved.liked_albums.iter().any(|a| a.id == info.id);
            if save_heart(ui, liked).clicked() {
                app.toggle_album(info, &ctx);
            }
            if linked && ghost_button(ui, icon::USER, &info.artist.name).clicked() {
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

/// "Follow" / "Following": an outlined pill, like music apps use.
fn follow_button(ui: &mut Ui, following: bool) -> egui::Response {
    let text = RichText::new(tr(if following { "Following" } else { "Follow" })).font(semi(12.5)).color(pal().text);
    let b = egui::Button::new(text)
        .min_size(vec2(84.0, 30.0))
        .corner_radius(CornerRadius::same(15))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0, if following { pal().text_muted } else { pal().text_faint }));
    ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The heart beside Play on an album or playlist page: saves it to your library.
fn save_heart(ui: &mut Ui, liked: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
    resp_selected(ui, &resp, liked, "Save to your library");
    let (font, col) = if liked { (filled_icon(24.0), pal().accent_soft) } else { (regular(24.0), if resp.hovered() { pal().text } else { pal().text_muted }) };
    paint_icon(ui.painter(), rect.center(), icon::HEART, font, col);
    resp.on_hover_text(tr(if liked { "Remove from your library" } else { "Save to your library" })).on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn action_row(app: &mut App, ui: &mut Ui, tracks: &[Track]) {
    let ctx = ui.ctx().clone();
    app.warm_list(tracks);
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
        hero(
            ui,
            &Hero {
                editable: false,
                kind: "Artist",
                title: &p.artist.name,
                subtitle: &sub,
                art: p.artist.picture_xl.as_deref(),
                glyph: icon::USER,
                round: true,
                banner: None,
                links: &[],
            },
        );
        app.note_visit(
            crate::app::Place::Artist(p.artist.id.clone()),
            &p.artist.name,
            "Artist",
            p.artist.picture.as_deref().or(p.artist.picture_xl.as_deref()),
            true,
        );
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            action_row(app, ui, &p.top);
            let following = app.follows(&p.artist.id);
            if follow_button(ui, following).clicked() {
                app.toggle_follow(&p.artist, &ctx);
            }
        });

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
            // Deezer's ten most popular, already sorted by popularity.
            tracks_block(app, ui, "artist_top", &p.top);
        }

        // Radio: this artist's songs mixed with similar artists'. The names come from
        // "Fans also like", already on this page, so the preview costs no extra requests.
        section_title(ui, "Artist radio", None);
        let with: Vec<&str> = p.related.iter().take(3).map(|a| a.name.as_str()).collect();
        let preview = if with.is_empty() {
            format!("{} {}", p.artist.name, tr("and similar artists"))
        } else {
            format!("{}, {} {}", p.artist.name, with.join(", "), tr("and more"))
        };
        ui.horizontal(|ui| {
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::click());
            artwork(ui, r, p.artist.picture.as_deref(), r_md());
            paint_icon(ui.painter(), r.center(), icon::PLANET, filled_icon(22.0), Color32::WHITE.gamma_multiply(0.9));
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(tr("{} Radio").replace("{}", &p.artist.name)).font(semi(14.5)).color(pal().text));
                ui.label(RichText::new(&preview).font(regular(12.5)).color(pal().text_muted));
            });
            ui.add_space(12.0);
            if ghost_button(ui, icon::PLAY, "Start radio").clicked() {
                open_and_play(app, Route::Mix(p.artist.id.clone()), &ctx);
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.navigate(Route::Mix(p.artist.id.clone()), &ctx);
            }
        });

        if !p.releases.is_empty() {
            section_title(ui, "Discography", None);
            album_grid(app, ui, &p.releases, None);
        }
        // Deezer files other artists with the same name here; keep them out of the way.
        if !p.namesakes.is_empty() {
            let open_id = egui::Id::new(("namesakes", &p.artist.id));
            let open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
            let title = tr("Other artists named {}").replace("{}", &p.artist.name);
            if section_title(ui, &title, Some(if open { "Hide" } else { "Show" })) {
                ui.data_mut(|d| d.insert_temp(open_id, !open));
            }
            ui.label(
                RichText::new(tr("Deezer lists these under the same name, but they're probably different artists."))
                    .font(regular(12.5))
                    .color(pal().text_muted),
            );
            if open {
                ui.add_space(6.0);
                album_grid(app, ui, &p.namesakes, None);
            }
        }
        if p.songs_like.len() >= 3 {
            section_title(ui, &tr("Songs like {}").replace("{}", &p.artist.name), None);
            tracks_block(app, ui, "artist_like", &p.songs_like[..p.songs_like.len().min(10)]);
        }
        if !p.related.is_empty() {
            section_title(ui, &tr("Listeners of {} also listen to").replace("{}", &p.artist.name), None);
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
    paint_line(ui, pos2(tx, rect.top() + 80.0), &release_line(a), regular(13.0), pal().text_muted, tw);
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
    let art = custom.clone().or_else(|| match playlist {
        Some(_) => tracks.iter().find_map(|t| t.art_big()).map(str::to_owned),
        None if app.route == Route::Liked => Some(LIKED_ART.to_owned()),
        None => None,
    });
    let clicks = hero(
        ui,
        &Hero {
            editable: playlist.is_some(),
            kind: tr(kind),
            title: tr(title),
            subtitle: sub,
            art: art.as_deref(),
            glyph,
            round: false,
            banner: None,
            links: &[],
        },
    );
    match (&app.route, playlist) {
        (_, Some(_)) => app.note_visit(crate::app::Place::Yours(title.to_string()), title, "Playlist", art.as_deref(), false),
        (Route::Liked, _) => app.note_visit(crate::app::Place::Liked, tr("Liked Songs"), "Playlist", Some(LIKED_ART), false),
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
                menu_begin(ui);
                if menu_item(ui, icon::IMAGE, "Change cover") {
                    pick_cover(app, i, &ctx);
                }
                if custom.is_some() && menu_item(ui, icon::TRASH, "Remove cover") {
                    app.remove_playlist_cover(i);
                }
            });
        }
    }
    if let Some(i) = playlist {
        online::publish_row(app, ui, i);
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
    app.ensure_featured(tracks);
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
    app.warm_list(&view);
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
            let busy = !all && online.iter().any(|t| app.download_queue.iter().any(|q| q.id == t.id) || app.downloading.iter().any(|(d, _)| d.id == t.id));
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
            dots_menu(ui, 22.0, tr("More options"), |ui| {
                if menu_item(ui, icon::PENCIL_SIMPLE, "Rename") {
                    app.rename_playlist = Some((i, title.to_string()));
                }
                if menu_item(ui, icon::IMAGE, "Change cover") {
                    pick_cover(app, i, &ctx);
                }
                if menu_item(ui, icon::SHARE_FAT, "Copy share code") {
                    app.share_playlist(i, &ctx);
                }
                menu_separator(ui);
                if menu_item(ui, icon::TRASH, "Delete playlist") {
                    app.confirm_delete = Some(i);
                }
            });
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Sort
            let sort = app.list_view.sort;
            let sort_btn = labeled_button(ui, icon::SORT_ASCENDING, tr(sort.label()), false);
            egui::Popup::menu(&sort_btn).show(|ui| {
                menu_begin(ui);
                menu_header(ui, "Sort by");
                for s in crate::app::SortBy::ALL {
                    let check = if s == sort { icon::CHECK } else { "" };
                    if menu_item(ui, check, s.label()) {
                        app.list_view.sort = s;
                    }
                }
            });
            ui.add_space(8.0);
            // Search: an icon that opens into a box.
            if app.list_view.search_open || !app.list_view.search.is_empty() {
                let id = ui.id().with("list_search");
                let r = ui.add(egui::TextEdit::singleline(&mut app.list_view.search).id(id).hint_text(tr("Search in playlist")).desired_width(200.0));
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

    let has_upcoming = app.queue.len() > app.queue_pos + 1;
    let clear = section_title(ui, "Next up", has_upcoming.then_some("Clear"));
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
    if app.queue.len() <= app.queue_pos + 1 {
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
    // Drawn straight from the queue (no copy each frame, which adds up for long queues).
    track_table(ui, "q_next", &app.queue[app.queue_pos + 1..], &cx, &mut ev);
    if !ev.is_empty() {
        let upcoming: Vec<Track> = app.queue[app.queue_pos + 1..].to_vec();
        app.handle_events(ev, &upcoming, &ctx);
    }
}

fn total_time(tracks: &[Track]) -> String {
    let s: u32 = tracks.iter().map(crate::fixes::shown_length).sum();
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 { format!("{h} hr {m} min") } else { format!("{m} min") }
}

// ---------------------------------------------------------------------------
// Player bar
// ---------------------------------------------------------------------------

fn player_bar(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect();
    // The Tung Tung theme: the player bar is a dark wooden shelf (faint grain).
    if tung_theme(app) {
        let shelf = full.expand2(vec2(24.0, 0.0)).intersect(ui.clip_rect());
        let c = pal().text;
        wood_grain(ui, shelf, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 22), 1.0, 0.0);
    }
    let left_w = (full.width() * 0.28).clamp(220.0, 360.0);
    let right_w = left_w.min(260.0);
    let controls_left = layout().player == crate::theme::PlayerStyle::ControlsLeft;
    let center_w = if controls_left { (full.width() * 0.34).clamp(300.0, 520.0) } else { (full.width() - left_w - right_w - 32.0).clamp(280.0, 640.0) };
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
        a11y(ui, &art_resp, egui::WidgetType::Button, || format!("Now playing: {} by {}. Open queue", t.title, t.all_artists()));
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
            let (a, feat) = artist_links(ui, egui::Id::new("np_artist"), pos2(tx, left.center().y + 2.0), &t, regular(12.5), pal().text_muted, tw);
            if a.clicked() {
                app.open_artist(&t, &ctx);
            }
            if let Some(a) = feat {
                app.open_artist_ref(&a, &ctx);
            }
        }
        let liked = app.liked_ids.contains(&t.id);
        // The heart sits just after the song's title and artist, like Spotify.
        let second_line = match app.audio.load_state() {
            LoadState::Resolving(msg) => loading_text(&msg),
            _ => t.all_artists(),
        };
        let artist_w = one_line(ui, &second_line, regular(12.5), pal().text_muted, tw).size().x;
        let text_right = tx + title_rect.width().max(artist_w);
        let hr = Rect::from_center_size(pos2((text_right + 22.0).min(left.right() - 18.0), left.center().y), Vec2::splat(30.0));
        let h = ui.interact(hr, egui::Id::new("np_like"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        resp_selected(ui, &h, liked, "Like current song");
        let (font, col) = if liked { (filled_icon(18.0), pal().accent_soft) } else { (regular(18.0), if h.hovered() { pal().text } else { pal().text_muted }) };
        paint_icon(ui.painter(), hr.center(), icon::HEART, font, col);
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
        let loading = (app.audio.load_state().is_busy() || app.audio.seeking()) && app.current.is_some();
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
            // About 30 times a second (it asked for every frame the screen can show).
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        } else {
            let glyph = if app.is_playing() { icon::PAUSE } else { icon::PLAY };
            paint_icon(ui.painter(), r.center(), glyph, filled_icon(18.0), pal().bg);
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
        let dur = app.current_length();
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
        if tung_theme(app) && app.current.is_some() {
            tung_runner(ui, resp.rect, shown, app.is_playing());
        }
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
    let on_lyrics = app.route == Route::Lyrics;
    let lyrics_on = on_lyrics || lyrics_page::panel_open(app, &ctx);
    if icon_button(&mut rui, icon::MICROPHONE_STAGE, 17.0, if lyrics_on { pal().accent_soft } else { pal().text_muted }, tr("Lyrics (L)")).clicked() {
        lyrics_page::toggle(app, &ctx);
    }
    let mini_on = app.saved.settings.mini_open;
    if icon_button(&mut rui, icon::PICTURE_IN_PICTURE, 17.0, if mini_on { pal().accent_soft } else { pal().text_muted }, tr("Mini player (Ctrl+M)")).clicked() {
        app.saved.settings.mini_open = !mini_on;
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

/// The frame every dialog uses.
fn dialog_frame() -> Frame {
    Frame::new().fill(pal().surface).corner_radius(r_lg()).inner_margin(24).stroke(Stroke::new(1.0, pal().border))
}

/// "Pulse closed unexpectedly last time", with the report to copy.
fn crash_dialog(app: &mut App, ctx: &egui::Context) {
    let Some((file, report)) = app.crash_notice.clone() else {
        return;
    };
    let mut close = false;
    egui::Modal::new(egui::Id::new("crash_notice")).frame(dialog_frame()).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.label(RichText::new(tr("Pulse closed unexpectedly last time")).font(heavy(19.0)));
        ui.add_space(8.0);
        ui.add(egui::Label::new(RichText::new(tr("Sorry about that. A report of what went wrong was saved on your PC. Send it to the Pulse team so it can be fixed: only this report is sent, with your Windows user name blanked out.")).color(pal().text_muted)).wrap());
        ui.add_space(10.0);
        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(&report).font(egui::FontId::monospace(11.5)).color(pal().text_muted)).wrap());
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ghost_button(ui, icon::COPY, "Copy report").clicked() {
                ctx.copy_text(report.clone());
                app.toast(tr("Report copied"), ctx);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let sending = app.report_sending.is_some();
                let send = egui::Button::new(RichText::new(tr(if sending { "Sending…" } else { "Send report" })).color(Color32::WHITE)).fill(pal().accent).corner_radius(16);
                if ui.add_enabled(!sending, send).clicked() {
                    app.report_sending = Some(crate::report::send(crate::report::Kind::Crash, "", &report));
                    close = true;
                }
                if ui.add(egui::Button::new(RichText::new(tr("Not now"))).corner_radius(16)).clicked() {
                    close = true;
                }
            });
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
    });
    if close {
        crate::crash::mark_seen(&file);
        app.crash_notice = None;
    }
}

/// "Report a problem": what happened, in your words, and (if you like) the diagnostics.
fn report_dialog(app: &mut App, ctx: &egui::Context) {
    // A report finished sending (from here or from the crash notice).
    if let Some(rx) = &app.report_sending
        && let Ok(result) = rx.try_recv()
    {
        app.report_sending = None;
        match result {
            Ok(()) => app.toast(tr("Report sent. Thank you!"), ctx),
            Err(e) => app.toast(&format!("{} {e}", tr("Couldn't send the report:")), ctx),
        }
    }
    if app.report_sending.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
    let Some((mut text, mut include)) = app.report.take() else {
        return;
    };
    let (mut open, mut send) = (true, false);
    egui::Modal::new(egui::Id::new("report_problem")).frame(dialog_frame()).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(RichText::new(tr("Report a problem")).font(heavy(19.0)));
        ui.add_space(8.0);
        ui.add(
            egui::Label::new(
                RichText::new(tr("What happened, and what were you doing just before? It goes to the Pulse team (only the developer can read it)."))
                    .color(pal().text_muted),
            )
            .wrap(),
        );
        ui.add_space(10.0);
        ui.add(
            egui::TextEdit::multiline(&mut text)
                .desired_rows(5)
                .desired_width(f32::INFINITY)
                .hint_text(tr("For example: the song stops after a few seconds when I skip quickly"))
                .char_limit(3000),
        );
        ui.add_space(8.0);
        ui.checkbox(&mut include, tr("Include technical details (versions, graphics card, recent problems)"));
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Your Windows user name and email addresses are blanked out before it's sent.")).font(regular(12.0)).color(pal().text_faint));
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ok = text.trim().chars().count() >= 5;
                if ui.add_enabled(ok, egui::Button::new(RichText::new(tr("Send")).color(Color32::WHITE)).fill(pal().accent).corner_radius(16)).clicked() {
                    send = true;
                }
                if ui.add(egui::Button::new(RichText::new(tr("Cancel"))).corner_radius(16)).clicked() {
                    open = false;
                }
            });
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            open = false;
        }
    });
    if send {
        let details = if include { crate::diag::report(&app.diag, &app.saved.settings) } else { String::new() };
        app.report_sending = Some(crate::report::send(crate::report::Kind::Problem, &text, &details));
        app.toast(tr("Sending your report…"), ctx);
    } else if open {
        app.report = Some((text, include));
    }
}

static OWN_PUBLIC: std::sync::Mutex<Option<(String, Option<crate::account::Profile>, std::time::Instant)>> = std::sync::Mutex::new(None);

/// Your public profile (for the profile page), read in the background and kept for a
/// minute. None until it's here.
pub fn own_public_profile(username: &str, ctx: &egui::Context) -> Option<crate::account::Profile> {
    let mut g = OWN_PUBLIC.lock().unwrap();
    let fresh = g.as_ref().is_some_and(|(u, _, at)| u == username && at.elapsed() < std::time::Duration::from_secs(60));
    if !fresh {
        let keep = g.as_ref().filter(|(u, ..)| u == username).and_then(|(_, p, _)| p.clone());
        *g = Some((username.to_string(), keep, std::time::Instant::now()));
        let (name, ctx) = (username.to_string(), ctx.clone());
        std::thread::spawn(move || {
            if let Ok(p) = crate::account::profile(&name) {
                if let Some((u, slot, _)) = OWN_PUBLIC.lock().unwrap().as_mut()
                    && *u == name
                {
                    *slot = Some(p);
                }
                ctx.request_repaint();
            }
        });
    }
    g.as_ref().and_then(|(_, p, _)| p.clone())
}

/// Read your public profile again next time (after you change it).
pub fn forget_own_public_profile() {
    if let Some((_, _, at)) = OWN_PUBLIC.lock().unwrap().as_mut() {
        // Old enough to be read again (if the clock allows going that far back).
        if let Some(old) = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(120)) {
            *at = old;
        }
    }
}

/// One of the Tung Tung theme's pictures, loaded once (`repeat`: tiles when stretched).
fn tung_texture(ctx: &egui::Context, name: &'static str, bytes: &'static [u8], repeat: bool) -> egui::TextureHandle {
    let id = egui::Id::new(("tung_texture", name));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let img = image::load_from_memory(bytes).map(|i| i.to_rgba8()).unwrap_or_default();
    let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    let mut opts = egui::TextureOptions::LINEAR;
    if repeat {
        opts.wrap_mode = egui::TextureWrapMode::Repeat;
    }
    let t = ctx.load_texture(name, color, opts);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// Faint wood grain over `rect` (the Tung Tung theme only): `tint` is the grain's color,
/// `scale` how big the grain is (1.0 = the picture's own size).
fn wood_grain(ui: &Ui, rect: Rect, tint: Color32, scale: f32, v_offset: f32) {
    if crate::perf::mode() == crate::perf::Mode::UltraLow || crate::perf::software() {
        return;
    }
    let tex = tung_texture(ui.ctx(), "tung_wood", include_bytes!("../assets/themes/Tung Tung/wood.png"), true);
    let [w, h] = tex.size().map(|v| v as f32 * scale);
    let uv = Rect::from_min_size(pos2(rect.left() / w, v_offset + rect.top() / h), vec2(rect.width() / w, rect.height() / h));
    ui.painter().image(tex.id(), rect, uv, tint);
}

fn tung_theme(app: &App) -> bool {
    app.saved.settings.theme == crate::themes::SECRET_THEME
}

/// The Tung Tung theme's runner: a small figure that runs along the progress bar, his feet
/// on the bar where the song is. 8 frames at 12 a second while music plays; standing still
/// on the first frame when paused, with reduce motion, or in Gaming and Ultra-low modes.
fn tung_runner(ui: &mut Ui, bar: Rect, at: f32, playing: bool) {
    const FRAMES: usize = 8;
    const FPS: f64 = 12.0;
    const SIZE: f32 = 26.0;
    let tex = tung_texture(ui.ctx(), "tung_runner", include_bytes!("../assets/themes/Tung Tung/runner.png"), false);
    // The bar is a wooden plank he runs on: grain over the whole track, played part too.
    let track = Rect::from_center_size(bar.center(), vec2(bar.width(), 5.0));
    wood_grain(ui, track, Color32::from_black_alpha(110), 0.35, 0.3);
    let moving = playing && !crate::theme::reduce_motion();
    let frame = if moving { (ui.input(|i| i.time) * FPS) as usize % FRAMES } else { 0 };
    if moving {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(1.0 / FPS));
    }
    let x = bar.left() + bar.width() * at.clamp(0.0, 1.0);
    // Feet just on the bar's line (the frame has a little empty space under the feet).
    let rect = Rect::from_center_size(pos2(x, bar.center().y - SIZE / 2.0 + 3.0), vec2(SIZE, SIZE));
    let uv = Rect::from_min_max(pos2(frame as f32 / FRAMES as f32, 0.0), pos2((frame + 1) as f32 / FRAMES as f32, 1.0));
    ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
}

/// "Pulse 1.2.1 is available": asked once each time Pulse opens with an update out, after
/// the intro. "Later" leaves the banner at the top.
fn update_dialog(app: &mut App, ctx: &egui::Context) {
    use crate::update::Status;
    let Status::Available(release) = &app.updater.status else {
        return;
    };
    if app.update_asked || app.intro.is_some() || app.account_dialog.is_some() || app.picker.is_some() || app.report.is_some() {
        return;
    }
    let (version, page) = (release.version.clone(), release.page.clone());
    let (mut later, mut update) = (false, false);
    egui::Modal::new(egui::Id::new("update_available")).frame(dialog_frame()).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(egui_phosphor::regular::ARROW_CIRCLE_UP).size(24.0).color(pal().accent));
            ui.label(RichText::new(format!("{} {} {}", tr("Pulse"), version, tr("is available."))).font(heavy(19.0)));
        });
        ui.add_space(8.0);
        ui.add(egui::Label::new(RichText::new(tr("It only takes a moment: Pulse restarts and your library stays as it is.")).color(pal().text_muted)).wrap());
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(tr("What's new")).frame_when_inactive(false).corner_radius(16)).clicked() {
                ctx.open_url(egui::OpenUrl::new_tab(&page));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new(tr("Update now")).color(Color32::WHITE)).fill(pal().accent).corner_radius(16)).clicked() {
                    update = true;
                }
                if ui.add(egui::Button::new(RichText::new(tr("Later"))).corner_radius(16)).clicked() {
                    later = true;
                }
            });
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            later = true;
        }
    });
    if update || later {
        app.update_asked = true;
    }
    if update {
        app.start_update(ctx);
    }
}

/// Asks before a backup replaces your library.
fn restore_dialog(app: &mut App, ctx: &egui::Context) {
    let Some((label, lib)) = app.pending_restore.take() else {
        return;
    };
    let (mut keep, mut restore) = (true, false);
    egui::Modal::new(egui::Id::new("confirm_restore")).frame(dialog_frame()).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.label(RichText::new(tr("Restore this backup?")).font(heavy(20.0)));
        ui.add_space(8.0);
        let body = format!(
            "{} ({}: {}, {}: {})\n\n{}",
            label,
            tr("Liked Songs"),
            lib.liked.len(),
            tr("Playlists"),
            lib.playlists.len(),
            tr("It replaces your likes, playlists and history. Your settings stay as they are, and your library as it is now is backed up first.")
        );
        ui.add(egui::Label::new(RichText::new(body).color(pal().text_muted)).wrap());
        ui.add_space(16.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new(tr("Restore")).color(Color32::WHITE)).fill(pal().accent).corner_radius(16)).clicked() {
                restore = true;
            }
            let cancel = ui.add(egui::Button::new(tr("Cancel")).frame_when_inactive(false).corner_radius(16));
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
    if restore {
        app.restore_library(*lib, ctx);
    } else if keep {
        app.pending_restore = Some((label, lib));
    }
}

fn dialogs(app: &mut App, ctx: &egui::Context) {
    crash_dialog(app, ctx);
    report_dialog(app, ctx);
    update_dialog(app, ctx);
    restore_dialog(app, ctx);
    credits_dialog(app, ctx);
    import_dialog(app, ctx);
    online::account_dialog(app, ctx);
    personal::profile_dialog(app, ctx);
    // "Pick artists you like": once, when you're starting out, after the other welcome windows.
    if app.picker.is_none() && !app.saved.artists_picked && app.account_dialog.is_none() && app.profile_editing.is_none() && app.intro.is_none() {
        if app.saved.followed_artists.is_empty() && app.saved.played_artists.len() < 5 {
            pick_artists::open(app, ctx);
        } else {
            app.saved.artists_picked = true; // you already have artists to start from
        }
    }
    pick_artists::show(app, ctx);
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
                let r = ui
                    .add(egui::TextEdit::singleline(&mut name).hint_text("My awesome playlist").desired_width(f32::INFINITY).margin(Margin::symmetric(10, 8)));
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

/// A dialog's button: filled with the accent color when it's the main one.
fn dialog_button(ui: &mut Ui, label: &str, primary: bool, enabled: bool) -> egui::Response {
    let text = RichText::new(label).font(semi(13.5)).color(if primary { Color32::WHITE } else { pal().text });
    let b =
        egui::Button::new(text).min_size(vec2(96.0, 36.0)).corner_radius(CornerRadius::same(18)).fill(if primary { pal().accent } else { pal().surface_hover });
    ui.add_enabled(enabled, b)
}

/// The "Import a playlist" window.
fn import_dialog(app: &mut App, ctx: &egui::Context) {
    if !app.import_open {
        return;
    }
    let mut close = false;
    let mut start = false;
    let modal = egui::Modal::new(egui::Id::new("import")).frame(dialog_frame()).show(ctx, |ui| {
        ui.set_width(500.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(tr("Import a playlist")).font(heavy(22.0)).color(pal().text));
        ui.add_space(8.0);
        ui.label(
            RichText::new(tr("Paste a link to a playlist or album. Pulse finds each song and makes a playlist of it."))
                .font(regular(13.5))
                .color(pal().text_muted),
        );
        ui.add_space(18.0);
        if let Some(job) = &app.import_running {
            let p = job.progress.lock().unwrap().clone();
            ui.horizontal(|ui| {
                ui.spinner();
                ui.add_space(4.0);
                let count = if p.total > 0 { format!("  {} / {}", p.done, p.total) } else { String::new() };
                ui.label(RichText::new(format!("{}{count}", tr(&p.status))).font(med(13.5)).color(pal().text));
            });
            ui.add_space(12.0);
            if p.total > 0 {
                ui.add(egui::ProgressBar::new(p.done as f32 / p.total as f32).desired_height(6.0).fill(pal().accent));
            }
            ui.add_space(22.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if dialog_button(ui, tr("Cancel"), false, true).clicked() {
                    app.cancel_import();
                }
            });
            return;
        }
        // The box: outlined, and tall enough to show a pasted list of songs.
        let songs = crate::import::spotify_track_ids(&app.import_url).len();
        let focus_id = egui::Id::new("import_box");
        let focused = ui.memory(|m| m.has_focus(focus_id));
        let r = Frame::new()
            .fill(pal().bg)
            .stroke(Stroke::new(if focused { 1.5 } else { 1.0 }, if focused { pal().accent } else { pal().border }))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(84.0).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut app.import_url)
                            .id(focus_id)
                            .frame(egui::Frame::NONE)
                            .hint_text("https://open.spotify.com/playlist/…")
                            .desired_rows(if songs > 1 { 4 } else { 1 })
                            .desired_width(f32::INFINITY),
                    )
                })
                .inner
            })
            .inner;
        if app.import_url.is_empty() && !r.has_focus() {
            r.request_focus();
        }
        // Enter starts the import (the line break it typed is dropped).
        let enter = r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if enter {
            app.import_url = app.import_url.trim().to_string();
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            if songs > 1 {
                ui.label(RichText::new(format!("{} {songs} {}", icon::SPOTIFY_LOGO, tr("Spotify songs ready to import"))).font(med(12.5)).color(pal().accent_soft));
            } else {
                for (glyph, name) in [(icon::YOUTUBE_LOGO, "YouTube"), (icon::SPOTIFY_LOGO, "Spotify"), (icon::SOUNDCLOUD_LOGO, "SoundCloud")] {
                    ui.label(RichText::new(format!("{glyph}  {name}")).font(regular(12.5)).color(pal().text_faint));
                }
            }
        });
        ui.add_space(16.0);
        // The tip, set apart in its own box.
        Frame::new().fill(pal().surface_hover).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(14, 12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr("Spotify playlist with more than 100 songs?")).font(semi(12.5)).color(pal().text));
            ui.add_space(4.0);
            ui.label(
                RichText::new(tr("A Spotify link only shares the first 100. For all of them: open the playlist in the Spotify app, click a song, press Ctrl+A, then Ctrl+C, and paste here."))
                    .font(regular(12.5))
                    .color(pal().text_muted),
            );
        });
        ui.add_space(22.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let ok = !app.import_url.trim().is_empty();
            if dialog_button(ui, tr("Import"), true, ok).clicked() || (enter && ok) {
                start = true;
            }
            if dialog_button(ui, tr("Cancel"), false, true).clicked() {
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
    let Some((t, data)) = &app.credits else {
        return;
    };
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
                    for a in &t.featured {
                        row(ui, "Featured artist", &a.name);
                    }
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
    let still = reduce_motion();
    let end = crate::app::toast_secs(msg);
    let alpha = if still {
        1.0
    } else if age < 0.15 {
        age / 0.15
    } else if age > end - 0.5 {
        (end - age) / 0.5
    } else {
        1.0
    }
    .clamp(0.0, 1.0) as f32;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("toast")).order(egui::Order::Foreground).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -104.0)).interactable(false).show(
        ctx,
        |ui| {
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
        },
    );
    // Redraw quickly only while it fades in or out; in between, wake up when the fade-out
    // starts (it used to redraw 33 times a second the whole time it showed).
    let next = if still {
        3.5 - age
    } else if !(0.15..=3.0).contains(&age) {
        0.03
    } else {
        3.0 - age
    };
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(next.max(0.01)));
}

/// "1 song" / "12 songs".
fn song_count(n: usize) -> String {
    if n == 1 { "1 song".to_string() } else { format!("{n} songs") }
}

/// "Single · 2024 · 3 songs". The count is left out when Deezer didn't give one,
/// rather than showing "0 songs".
fn release_line(a: &crate::deezer::AlbumInfo) -> String {
    let mut s = crate::i18n::tr(a.kind()).to_string();
    if let Some(y) = a.year() {
        s.push_str(&format!(" · {y}"));
    }
    if a.nb_tracks > 0 {
        s.push_str(&format!(" · {}", song_count(a.nb_tracks as usize)));
    }
    s
}
