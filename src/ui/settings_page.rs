//! The Settings page.

use super::page_title;
use crate::app::App;
use crate::i18n::tr;
use crate::settings::{EQ_BANDS, EQ_MAX_DB, EQ_PRESETS, Language, Quality, fmt_bytes};
use crate::theme::*;
use crate::widgets::*;
use egui::{Align, Align2, Color32, CornerRadius, Layout, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use egui_phosphor::regular as icon;

pub fn settings_page(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    page_title(ui, tr("Settings"), None);
    let before = app.saved.settings.clone();
    let autoplay_before = app.saved.autoplay;
    let max_w = ui.available_width().min(860.0);
    ui.set_max_width(max_w);

    // ------------------------------------------------------------ Account
    if crate::account::available() {
        section(ui, tr("Account"));
        super::online::account_settings(app, ui, &|ui, title, body, control| row(ui, title, body, |ui| control(ui)));
    }
    let s = &mut app.saved.settings;

    // ------------------------------------------------------------ Language
    section(ui, tr("Language"));
    row(ui, tr("Choose language"), tr("Changes apply right away."), |ui| {
        egui::ComboBox::from_id_salt("language").selected_text(s.language.native_name()).width(170.0).show_ui(ui, |ui| {
            for l in Language::ALL {
                ui.selectable_value(&mut s.language, l, l.native_name());
            }
        });
    });

    // ------------------------------------------------------------ Themes
    themes_section(app, ui);

    // ------------------------------------------------------------ Autoplay
    section(ui, tr("Autoplay"));
    row(ui, tr("Autoplay"), tr("When your queue ends, keep playing similar songs."), |ui| {
        switch(ui, &mut app.saved.autoplay, tr("Autoplay"));
    });

    // ------------------------------------------------------------ Audio quality
    let s = &mut app.saved.settings;
    section(ui, tr("Audio quality"));
    row(ui, tr("Streaming quality"), tr("Low uses less data. High is the best YouTube offers (usually 128 kbps AAC)."), |ui| {
        quality_combo(ui, "stream_q", &mut s.stream_quality);
    });
    row(ui, tr("Download"), tr("Quality of songs you download for offline listening."), |ui| {
        quality_combo(ui, "download_q", &mut s.download_quality);
    });

    // ------------------------------------------------------------ Library
    section(ui, tr("Your Library"));
    let music = crate::settings::music_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let mut open_import = false;
    row(ui, tr("Import a playlist"), tr("Bring in a playlist from YouTube, Spotify or SoundCloud."), |ui| {
        open_import = ui.button(tr("Import")).clicked();
    });
    if open_import {
        app.import_open = true;
    }
    let s = &mut app.saved.settings;
    row(ui, tr("Show Local Files"), &format!("{} {music}", tr("Songs from your Music folder:")), |ui| {
        switch(ui, &mut s.show_local_files, tr("Show Local Files"));
    });

    // ------------------------------------------------------------ Playback
    section(ui, tr("Playback"));
    row(ui, tr("Crossfade songs"), tr("Fade each song into the next."), |ui| {
        let mut on = s.crossfade_secs > 0.0;
        let was = on;
        switch(ui, &mut on, tr("Crossfade songs"));
        if on != was {
            s.crossfade_secs = if on { 5.0 } else { 0.0 };
        }
    });
    if s.crossfade_secs > 0.0 {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("1 s").font(regular(12.0)).color(pal().text_muted));
            ui.spacing_mut().slider_width = 260.0;
            ui.add(egui::Slider::new(&mut s.crossfade_secs, 1.0..=10.0).step_by(1.0).show_value(false));
            ui.label(RichText::new("10 s").font(regular(12.0)).color(pal().text_muted));
            ui.add_space(8.0);
            ui.label(RichText::new(format!("{:.0} s", s.crossfade_secs)).font(med(13.0)).color(pal().text));
        });
        ui.add_space(8.0);
    }
    row(ui, tr("Explicit songs"), tr("Play the uncensored version of a song when there is one. Off: Pulse picks the clean version instead."), |ui| {
        switch(ui, &mut s.explicit, tr("Explicit songs"));
    });
    row(ui, tr("Gapless playback"), tr("Start the next song with no silence in between."), |ui| {
        switch(ui, &mut s.gapless, tr("Gapless playback"));
    });
    row(ui, tr("Automix"), tr("Allow seamless transitions between songs: skips silent endings and blends into the next song."), |ui| {
        switch(ui, &mut s.automix, tr("Automix"));
    });
    row(ui, tr("Mono audio"), tr("Plays the same sound from left and right speakers."), |ui| {
        switch(ui, &mut s.mono, tr("Mono audio"));
    });
    row(ui, tr("Equalizer"), tr("Shape the sound with presets or by dragging the points."), |ui| {
        switch(ui, &mut s.eq_enabled, tr("Equalizer"));
    });
    if s.eq_enabled {
        equalizer_editor(ui, s);
    }

    // ------------------------------------------------------------ Storage
    section(ui, tr("Storage"));
    let size = *app.cache_size.get_or_insert_with(crate::app::cache_size);
    let mut clear = false;
    row(ui, tr("Cache"), &format!("{} {}", tr("Temporary files Pulse can download again:"), fmt_bytes(size)), |ui| {
        clear = ui.button(tr("Clear cache")).clicked();
    });
    let s = &mut app.saved.settings;
    row(
        ui,
        tr("Space for saved pictures"),
        tr("Covers and artist photos are kept so they aren't downloaded again. The oldest go first when this fills up."),
        |ui| {
            for mb in [100u64, 300, 1000] {
                let label = if mb >= 1000 { format!("{} GB", mb / 1000) } else { format!("{mb} MB") };
                if ui.selectable_label(s.art_cache_mb == mb, label).clicked() {
                    s.art_cache_mb = mb;
                }
            }
        },
    );
    row(
        ui,
        tr("Space for played songs"),
        tr("Songs you've played are kept for a while, so they start at once next time. The ones you haven't played for longest go first."),
        |ui| {
            for mb in [0u64, 500, 1000, 3000] {
                let label = if mb == 0 {
                    tr("Off").to_string()
                } else if mb >= 1000 {
                    format!("{} GB", mb / 1000)
                } else {
                    format!("{mb} MB")
                };
                if ui.selectable_label(s.replay_cache_mb == mb, label).clicked() {
                    s.replay_cache_mb = mb;
                }
            }
        },
    );
    let offline = s.offline_dir();
    row(ui, tr("Offline storage location"), &offline.display().to_string(), |ui| {
        #[cfg(windows)]
        if ui.button(tr("Change location")).clicked()
            && let Some(dir) = rfd::FileDialog::new().set_directory(&offline).pick_folder()
        {
            s.offline_dir = Some(dir);
        }
        if s.offline_dir.is_some() && ui.button(tr("Reset")).clicked() {
            s.offline_dir = None;
        }
    });

    // ------------------------------------------------------------ Performance
    section(ui, tr("Performance"));
    row(
        ui,
        tr("Mode"),
        tr(
            "Gaming Mode pauses the extras while you play a game (Ctrl+G). Ultra-low resource also drops pictures in lists and redraws less. Music plays exactly the same in every mode.",
        ),
        |ui| {
            for m in [crate::perf::Mode::Normal, crate::perf::Mode::Gaming, crate::perf::Mode::UltraLow] {
                if ui.selectable_label(s.perf_mode == m, tr(m.label())).clicked() {
                    s.perf_mode = m;
                }
            }
        },
    );
    if s.perf_mode != crate::perf::Mode::Normal {
        ui.label(RichText::new(tr("Paused in this mode (tick one to keep it on):")).font(semi(12.5)).color(pal().text_muted));
        for e in crate::perf::Extra::ALL.into_iter().filter(|e| e.off_in(s.perf_mode)) {
            let key = crate::perf::key(e).to_string();
            let mut keep = s.perf_keep.contains(&key);
            if ui.checkbox(&mut keep, tr(e.label())).changed() {
                if keep {
                    s.perf_keep.push(key);
                } else {
                    s.perf_keep.retain(|k| *k != key);
                }
            }
        }
        ui.add_space(6.0);
    }

    // ------------------------------------------------------------ Mini player
    section(ui, tr("Mini player"));
    row(
        ui,
        tr("Show the mini player"),
        tr(
            "A small window with the song and controls that stays on top of games (Ctrl+M). Drag it anywhere. Over a game it shows in borderless or windowed mode, not exclusive full screen.",
        ),
        |ui| {
            switch(ui, &mut s.mini_open, tr("Show the mini player"));
        },
    );
    row(ui, tr("Always on top"), tr("Keep it above other windows."), |ui| {
        switch(ui, &mut s.mini_on_top, tr("Always on top"));
    });
    row(ui, tr("Show the lyric being sung"), tr("For songs with synced lyrics, the mini player shows the current line instead of the artists."), |ui| {
        switch(ui, &mut s.mini_lyrics, tr("Show the lyric being sung"));
    });

    // ------------------------------------------------------------ Compatibility
    section(ui, tr("Compatibility"));
    row(ui, tr("High contrast mode"), tr("Stronger colors and borders, easier to read."), |ui| {
        switch(ui, &mut s.high_contrast, tr("High contrast mode"));
    });
    row(ui, tr("Moving background"), tr("In themes with a song-colored background, let it drift slowly while music plays."), |ui| {
        switch(ui, &mut s.moving_background, tr("Moving background"));
    });
    row(ui, tr("Reduce motion"), tr("Turns off moving backgrounds and menu fades."), |ui| {
        switch(ui, &mut s.reduce_motion, tr("Reduce motion"));
    });
    row(
        ui,
        tr("Enable hardware acceleration"),
        tr(
            "Uses your graphics card to draw Pulse. Leave it on: without it Pulse is slower and some effects are off. Turn off only if the window flickers or stays blank. Restart Pulse to apply.",
        ),
        |ui| {
            switch(ui, &mut s.hardware_acceleration, tr("Enable hardware acceleration"));
        },
    );

    // ------------------------------------------------------------ Discord
    if crate::discord::available() {
        section(ui, "Discord");
        row(
            ui,
            tr("Show what you're listening to"),
            tr("Your Discord profile shows the song, artists and cover while Pulse plays. Pulse talks to the Discord app on this PC, so it needs to be open."),
            |ui| {
                switch(ui, &mut app.saved.settings.discord_status, tr("Show what you're listening to"));
            },
        );
        if app.saved.settings.discord_status {
            discord_link(ui);
        }
    }

    your_data_section(app, ui, &ctx);

    // ------------------------------------------------------------ Updates
    updates_section(app, ui, &ctx);
    supporters_section(ui);

    // ------------------------------------------------------------ Troubleshooting
    section(ui, tr("Troubleshooting"));
    row(ui, tr("Diagnostics"), tr("Versions, storage, a self-test and recent problems, for when something isn't working."), |ui| {
        if ui.button(tr("Open")).clicked() {
            app.navigate(crate::app::Route::Diagnostics, &ctx);
        }
    });
    row(ui, tr("Report a problem"), tr("Tell the Pulse team what went wrong. Only the developer can read it."), |ui| {
        if ui.button(tr("Report")).clicked() {
            app.report = Some((String::new(), true));
        }
    });

    ui.add_space(30.0);

    if clear {
        app.clear_cache(&ctx);
    }
    if app.saved.settings != before || app.saved.autoplay != autoplay_before {
        app.apply_settings(&before, &ctx);
    }
}

/// Checking for and installing new versions.
fn updates_section(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
    use crate::update::Status;
    section(ui, tr("Updates"));
    row(
        ui,
        tr("Check for updates"),
        tr(
            "Each time it opens, Pulse asks GitHub whether there's a newer version. Nothing is installed until you choose Update now, and only if it's signed by Pulse.",
        ),
        |ui| {
            switch(ui, &mut app.saved.settings.check_updates, tr("Check for updates"));
        },
    );
    let status = match &app.updater.status {
        Status::Idle => String::new(),
        Status::Checking => tr("Checking…").to_string(),
        Status::UpToDate => tr("You have the newest version.").to_string(),
        Status::Available(r) => format!("{} {} {}", tr("Pulse"), r.version, tr("is available.")),
        Status::Downloading(_, p) => format!("{} {:.0}%", tr("Downloading…"), p.lock().map(|v| *v).unwrap_or(0.0) * 100.0),
        Status::Installing => tr("Installing…").to_string(),
        Status::Failed(e) | Status::InstallFailed(_, e) => e.clone(),
    };
    let body = if status.is_empty() { tr("Installed version").to_string() } else { status };
    let available = matches!(app.updater.status, Status::Available(_) | Status::InstallFailed(..));
    row(ui, &format!("Pulse {}", env!("CARGO_PKG_VERSION")), &body, |ui| {
        if available {
            if ui.add(egui::Button::new(RichText::new(tr("Update now")).color(Color32::WHITE)).fill(pal().accent).corner_radius(14)).clicked() {
                app.start_update(ctx);
            }
        } else {
            ui.add_enabled_ui(!app.updater.busy(), |ui| {
                if ui.button(tr("Check now")).clicked() {
                    app.updater.check(ctx);
                }
            });
        }
    });
}

/// "Early supporters": the people Pulse thanks by name. Read once per session, in the
/// background; nothing shows until there's a list.
fn supporters_section(ui: &mut Ui) {
    use std::sync::{Mutex, OnceLock};
    static LIST: OnceLock<Mutex<Option<Vec<String>>>> = OnceLock::new();
    let list = LIST.get_or_init(|| {
        let ctx = ui.ctx().clone();
        std::thread::spawn(move || {
            if let Ok(names) = crate::account::supporters() {
                *LIST.get().unwrap().lock().unwrap() = Some(names);
                ctx.request_repaint();
            }
        });
        Mutex::new(None)
    });
    let Some(names) = list.lock().unwrap().clone().filter(|n| !n.is_empty()) else { return };
    section(ui, tr("Early supporters"));
    ui.label(RichText::new(tr("The people who believed in Pulse from the start. Thank you!")).color(pal().text_muted));
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for n in names {
            egui::Frame::new().fill(pal().surface).stroke(Stroke::new(1.0, pal().border)).corner_radius(12).inner_margin(egui::Margin::symmetric(10, 4)).show(
                ui,
                |ui| {
                    ui.label(RichText::new(format!("{} @{n}", icon::STAR)).font(med(12.5)).color(pal().text));
                },
            );
        }
    });
}

/// The Windows save or open dialog for a backup file (none elsewhere).
fn ask_backup_file(save: bool) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let d = rfd::FileDialog::new().add_filter("Pulse backup", &["json"]);
        if save { d.set_file_name("pulse-library.json").save_file() } else { d.pick_file() }
    }
    #[cfg(not(windows))]
    {
        let _ = save;
        None
    }
}

/// Backups of your library: automatic daily ones, plus making, exporting, importing
/// and restoring them.
fn your_data_section(app: &mut App, ui: &mut Ui, ctx: &egui::Context) {
    section(ui, tr("Your data"));
    ui.add(
        egui::Label::new(
            RichText::new(tr("Pulse backs up your likes, playlists and history once a day and keeps the last seven. Backups stay on this PC."))
                .color(pal().text_muted),
        )
        .wrap(),
    );
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        if ghost_button(ui, icon::FLOPPY_DISK, "Back up now").clicked() {
            match crate::backup::write(&app.saved, crate::backup::Kind::Manual) {
                Ok(_) => app.toast(tr("Library backed up"), ctx),
                Err(e) => app.toast(&e, ctx),
            }
            app.refresh_backups();
        }
        if ghost_button(ui, icon::EXPORT, "Export…").clicked()
            && let Some(path) = ask_backup_file(true)
        {
            match crate::backup::write_to(&app.saved, &path) {
                Ok(()) => app.toast(tr("Library exported"), ctx),
                Err(e) => app.toast(&e, ctx),
            }
        }
        if ghost_button(ui, icon::DOWNLOAD_SIMPLE, "Import…").clicked()
            && let Some(path) = ask_backup_file(false)
        {
            match crate::backup::read(&path) {
                Ok(lib) => {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    app.pending_restore = Some((name, Box::new(lib)));
                }
                Err(e) => app.toast(&e, ctx),
            }
        }
        if ghost_button(ui, icon::FOLDER_OPEN, "Open folder").clicked() {
            let _ = std::fs::create_dir_all(crate::backup::dir());
            ctx.open_url(egui::OpenUrl::new_tab(crate::settings::file_uri(&crate::backup::dir())));
        }
    });
    // The backups, read in the background when Settings opens (and after making one).
    let Some(list) = app.backup_list(ctx).map(<[_]>::to_vec) else { return };
    ui.add_space(10.0);
    if list.is_empty() {
        ui.label(RichText::new(tr("No backups yet. The first one is made today.")).font(regular(12.5)).color(pal().text_faint));
    }
    let all_id = egui::Id::new("backups_show_all");
    let show_all = ui.data(|d| d.get_temp::<bool>(all_id).unwrap_or(false));
    for b in list.iter().take(if show_all { usize::MAX } else { 6 }) {
        ui.horizontal(|ui| {
            let kind = match b.kind {
                crate::backup::Kind::Daily => tr("Daily"),
                crate::backup::Kind::Manual => tr("Saved by you"),
                crate::backup::Kind::BeforeRestore => tr("Before a restore"),
            };
            ui.label(RichText::new(crate::crash::utc(b.made)).font(med(13.0)).color(pal().text));
            ui.label(
                RichText::new(format!("{kind} · {} {} · {} {}", b.liked, tr("liked"), b.playlists, tr("playlists")))
                    .font(regular(12.5))
                    .color(pal().text_muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new(tr("Restore")).corner_radius(14)).clicked() {
                    match crate::backup::read(&b.path) {
                        Ok(lib) => app.pending_restore = Some((crate::crash::utc(b.made), Box::new(lib))),
                        Err(e) => app.toast(&e, ctx),
                    }
                }
            });
        });
    }
    if list.len() > 6 {
        ui.add_space(4.0);
        let label = if show_all { tr("Show fewer").to_string() } else { format!("{} ({})", tr("Show all"), list.len()) };
        if ui.add(egui::Button::new(RichText::new(label).font(med(12.5)).color(pal().text_muted)).frame_when_inactive(false)).clicked() {
            ui.data_mut(|d| d.insert_temp(all_id, !show_all));
        }
    }
}

/// Theme picker: a preview card per theme, the color scheme, and theme-folder tools.
fn themes_section(app: &mut App, ui: &mut Ui) {
    section(ui, tr("Themes"));
    let current = app.current_theme().map(|t| t.name.clone()).unwrap_or_default();
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
        let supporter = crate::account::session().is_some_and(|s| s.supporter);
        let found = app.saved.settings.found_secret_theme;
        for t in app.themes.iter().filter(|t| crate::themes::offered(t, supporter, found, &current)) {
            let selected = t.name == current;
            let (rect, resp) = ui.allocate_exact_size(vec2(176.0, 136.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, &t.name));
            focus_ring(ui, &resp);
            let scheme = if selected { t.scheme_index(&app.saved.settings.scheme) } else { 0 };
            swatch(ui, Rect::from_min_size(rect.min, vec2(176.0, 100.0)), &t.schemes[scheme].palette);
            let border = if selected {
                Stroke::new(2.5, pal().accent_soft)
            } else if resp.hovered() {
                Stroke::new(1.0, pal().text_muted)
            } else {
                Stroke::new(1.0, pal().border)
            };
            ui.painter().rect_stroke(Rect::from_min_size(rect.min, vec2(176.0, 100.0)), r_md(), border, egui::StrokeKind::Outside);
            // The special themes say what they are, in their own color.
            let special = t.builtin && (t.name == crate::themes::SUPPORTERS_THEME || t.name == crate::themes::SECRET_THEME);
            let name = if !t.builtin {
                format!("{}  ·  {}", t.name, tr("custom"))
            } else if t.name == crate::themes::SUPPORTERS_THEME {
                format!("{} – {}", t.name, tr("Early Support"))
            } else if t.name == crate::themes::SECRET_THEME {
                format!("{} – {}", t.name, tr("Secret"))
            } else {
                t.name.clone()
            };
            // The theme's own accent, from its scheme that matches Pulse's light or dark right now.
            let dark = |c: Color32| (c.r() as u32 + c.g() as u32 + c.b() as u32) < 384;
            let color = if special {
                t.schemes.iter().find(|sc| dark(sc.palette.bg) == dark(pal().bg)).map_or(pal().accent_soft, |sc| sc.palette.accent_soft)
            } else {
                pal().text
            };
            paint_line(ui, pos2(rect.left() + 2.0, rect.top() + 108.0), &name, semi(13.0), color, 172.0);
            let n = t.schemes.len();
            let sub = if n == 1 { "1 scheme".to_string() } else { format!("{n} schemes") };
            paint_line(ui, pos2(rect.left() + 2.0, rect.top() + 124.0), &sub, regular(11.5), pal().text_muted, 172.0);
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !selected {
                pick = Some(t.name.clone());
            }
        }
    });
    if let Some(name) = pick {
        app.saved.settings.theme = name;
        app.saved.settings.scheme.clear();
    }
    ui.add_space(10.0);

    let Some(t) = app.current_theme().cloned() else { return };
    if !t.description.is_empty() || !t.author.is_empty() {
        let by = if t.author.is_empty() { String::new() } else { format!("{} {}. ", tr("By"), t.author) };
        ui.label(RichText::new(format!("{by}{}", t.description)).font(regular(12.5)).color(pal().text_muted));
        ui.add_space(6.0);
    }
    let s = &mut app.saved.settings;
    row(ui, tr("Color scheme"), tr("Each theme can have several color schemes."), |ui| {
        let cur = t.scheme_index(&s.scheme);
        egui::ComboBox::from_id_salt("scheme").selected_text(&t.schemes[cur].name).width(170.0).show_ui(ui, |ui| {
            for (i, sc) in t.schemes.iter().enumerate() {
                if ui.selectable_label(i == cur, &sc.name).clicked() {
                    s.scheme = sc.name.clone();
                }
            }
        });
    });
    let dir = crate::themes::themes_dir();
    row(ui, tr("Theme files"), &format!("{} {}", tr("Add your own themes here. Pulse reloads a theme as soon as you save it:"), dir.display()), |ui| {
        if ui.button(tr("Open themes folder")).clicked() {
            let _ = std::fs::create_dir_all(&dir);
            open_folder(ui.ctx(), &dir);
        }
        if t.builtin && ui.button(tr("Make a copy")).on_hover_text(tr("Copies this theme into the themes folder so you can edit it.")).clicked() {
            match crate::themes::copy_builtin(&t.name) {
                Ok(copy) => open_folder(ui.ctx(), &copy),
                Err(e) => {
                    ui.ctx().copy_text(e);
                }
            }
        }
    });
    if let Some(folder) = &t.folder {
        ui.label(RichText::new(folder.display().to_string()).font(regular(11.5)).color(pal().text_faint));
    }
    if !t.warnings.is_empty() {
        egui::CollapsingHeader::new(RichText::new(format!("{} {} ({})", icon::WARNING, tr("Theme notes"), t.warnings.len())).color(pal().text_muted))
            .id_salt("theme_warnings")
            .show(ui, |ui| {
                for w in &t.warnings {
                    ui.label(RichText::new(format!("• {w}")).font(regular(12.0)).color(pal().text_muted));
                }
            });
    }
}

/// A tiny picture of a theme: sidebar, page, a card, a song row and the accent.
fn swatch(ui: &Ui, r: Rect, p: &crate::theme::Palette) {
    let painter = ui.painter();
    painter.rect_filled(r, r_md(), p.bg);
    let side = Rect::from_min_size(r.min, vec2(44.0, r.height()));
    painter.rect_filled(side, CornerRadius { nw: r_md().nw, sw: r_md().sw, ne: 0, se: 0 }, p.sidebar);
    for k in 0..3 {
        let y = r.top() + 16.0 + k as f32 * 12.0;
        let col = if k == 0 { p.accent_soft } else { p.text_muted };
        painter.rect_filled(Rect::from_min_size(pos2(side.left() + 8.0, y), vec2(26.0, 5.0)), CornerRadius::same(2), col);
    }
    let player = Rect::from_min_max(pos2(r.left(), r.bottom() - 18.0), r.max);
    painter.rect_filled(player, CornerRadius { nw: 0, ne: 0, sw: r_md().sw, se: r_md().se }, p.player);
    painter.circle_filled(player.center(), 5.0, p.text);
    let card = Rect::from_min_size(pos2(side.right() + 10.0, r.top() + 10.0), vec2(48.0, 52.0));
    painter.rect_filled(card, CornerRadius::same(4), p.surface);
    painter.rect_filled(Rect::from_min_size(card.min + vec2(5.0, 5.0), vec2(38.0, 28.0)), CornerRadius::same(3), p.accent.gamma_multiply(0.7));
    painter.rect_filled(Rect::from_min_size(card.min + vec2(5.0, 38.0), vec2(30.0, 4.0)), CornerRadius::same(2), p.text);
    let row = Rect::from_min_size(pos2(card.right() + 8.0, r.top() + 14.0), vec2(r.right() - card.right() - 16.0, 14.0));
    painter.rect_filled(row, CornerRadius::same(3), p.surface_hover);
    painter.rect_filled(Rect::from_min_size(row.min + vec2(4.0, 4.5), vec2(row.width() * 0.6, 5.0)), CornerRadius::same(2), p.text);
    painter.rect_filled(Rect::from_min_size(pos2(row.left() + 4.0, row.bottom() + 8.0), vec2(row.width() * 0.45, 5.0)), CornerRadius::same(2), p.text_muted);
    painter.circle_filled(pos2(r.right() - 16.0, card.bottom() - 6.0), 9.0, p.accent);
}

fn open_folder(ctx: &egui::Context, dir: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = ctx;
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    #[cfg(not(windows))]
    ctx.open_url(egui::OpenUrl::new_tab(format!("file://{}", dir.display())));
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(22.0);
    ui.label(RichText::new(title).font(heavy(17.0)).color(pal().text));
    ui.add_space(8.0);
}

/// One setting: name and explanation on the left, its control on the right.
fn row(ui: &mut Ui, title: &str, body: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let text_w = (ui.available_width() - 260.0).max(240.0);
        ui.vertical(|ui| {
            ui.set_width(text_w);
            ui.label(RichText::new(title).font(med(14.0)).color(pal().text));
            ui.add(egui::Label::new(RichText::new(body).font(regular(12.5)).color(pal().text_muted)).wrap());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
    ui.add_space(12.0);
}

fn quality_combo(ui: &mut Ui, id: &str, q: &mut Quality) {
    egui::ComboBox::from_id_salt(id).selected_text(tr(q.label())).width(150.0).show_ui(ui, |ui| {
        for v in Quality::ALL {
            ui.selectable_value(q, v, tr(v.label()));
        }
    });
}

/// An on/off switch like Spotify's.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(40.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, label));
    focus_ring(ui, &resp);
    let t = ui.ctx().animate_bool(resp.id, *on);
    let track = if *on { pal().accent } else { pal().text_faint };
    ui.painter().rect_filled(rect, CornerRadius::same(11), track);
    let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
    ui.painter().circle_filled(pos2(x, rect.center().y), 8.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The equalizer: preset picker and a curve with one draggable point per band.
fn equalizer_editor(ui: &mut Ui, s: &mut crate::settings::Settings) {
    let frame = egui::Frame::new().fill(pal().surface).corner_radius(r_lg()).inner_margin(18.0);
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Presets")).font(med(13.0)).color(pal().text_muted));
            ui.add_space(8.0);
            egui::ComboBox::from_id_salt("eq_preset").selected_text(tr(&s.eq_preset).to_string()).width(170.0).show_ui(ui, |ui| {
                for (name, gains) in EQ_PRESETS {
                    if ui.selectable_label(s.eq_preset == name, tr(name)).clicked() {
                        s.eq_preset = name.to_string();
                        s.eq_gains = gains;
                    }
                }
                for (name, gains) in s.eq_custom.clone() {
                    if ui.selectable_label(s.eq_preset == name, &name).clicked() {
                        s.eq_preset = name;
                        s.eq_gains = gains;
                    }
                }
            });
            ui.add_space(8.0);
            if s.eq_preset == "Manual" && ui.button(tr("Save preset")).clicked() {
                let name = format!("{} {}", tr("My preset"), s.eq_custom.len() + 1);
                s.eq_custom.push((name.clone(), s.eq_gains));
                s.eq_preset = name;
            }
            let custom = s.eq_custom.iter().position(|(n, _)| *n == s.eq_preset);
            if let Some(i) = custom
                && ui.button(tr("Delete preset")).clicked()
            {
                s.eq_custom.remove(i);
                s.eq_preset = "Manual".into();
            }
        });
        ui.add_space(10.0);
        if eq_curve(ui, &mut s.eq_gains) {
            s.eq_preset = "Manual".into();
        }
        ui.add_space(8.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button(tr("Reset")).clicked() {
                s.eq_gains = [0.0; 6];
                s.eq_preset = "Flat".into();
            }
        });
    });
}

fn band_label(hz: f32) -> String {
    if hz >= 1000.0 {
        let k = hz / 1000.0;
        if k.fract() == 0.0 { format!("{k:.0}KHz") } else { format!("{k:.1}KHz") }
    } else {
        format!("{hz:.0}Hz")
    }
}

/// Draws the curve and lets each point be dragged up and down. Returns true if changed.
fn eq_curve(ui: &mut Ui, gains: &mut [f32; 6]) -> bool {
    let w = ui.available_width();
    let (outer, _) = ui.allocate_exact_size(vec2(w, 200.0), Sense::hover());
    let plot = Rect::from_min_max(outer.min + vec2(52.0, 8.0), outer.max - vec2(20.0, 30.0));
    let p = ui.painter().clone();
    let n = gains.len();
    let x_of = |i: usize| egui::lerp(plot.left()..=plot.right(), i as f32 / (n - 1) as f32);
    let y_of = |db: f32| egui::remap(db, EQ_MAX_DB..=-EQ_MAX_DB, plot.top()..=plot.bottom());

    // Grid: one line per band, a centre line at 0 dB, and the dB labels.
    for (i, hz) in EQ_BANDS.iter().enumerate() {
        p.vline(x_of(i), plot.y_range(), Stroke::new(1.0, pal().border));
        paint_icon(&p, pos2(x_of(i), plot.bottom() + 16.0), band_label(*hz), med(11.5), pal().text_muted);
    }
    p.hline(plot.x_range(), y_of(0.0), Stroke::new(1.0, pal().border));
    p.text(pos2(outer.left(), plot.top()), Align2::LEFT_CENTER, "+12dB", med(11.5), pal().text_muted);
    p.text(pos2(outer.left(), plot.bottom()), Align2::LEFT_CENTER, "-12dB", med(11.5), pal().text_muted);

    // Smooth curve through the points (Catmull-Rom), filled down to the bottom.
    let pts: Vec<egui::Pos2> = (0..n).map(|i| pos2(x_of(i), y_of(gains[i]))).collect();
    let mut curve = vec![];
    for i in 0..n - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(n - 1)];
        for k in 0..16 {
            let t = k as f32 / 16.0;
            let t2 = t * t;
            let t3 = t2 * t;
            let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (c - a) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (3.0 * b - a - 3.0 * c + d) * t3);
            let y = f(p0.y, p1.y, p2.y, p3.y).clamp(plot.top(), plot.bottom());
            curve.push(pos2(f(p0.x, p1.x, p2.x, p3.x), y));
        }
    }
    curve.push(pts[n - 1]);
    let mut mesh = egui::Mesh::default();
    let top = pal().accent.gamma_multiply(0.75);
    let bottom = pal().accent.gamma_multiply(0.05);
    for (k, c) in curve.iter().enumerate() {
        mesh.colored_vertex(*c, top);
        mesh.colored_vertex(pos2(c.x, plot.bottom()), bottom);
        if k > 0 {
            let b = (k as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b + 1, b);
        }
    }
    p.add(mesh);
    p.add(egui::Shape::line(curve, Stroke::new(2.5, pal().accent)));

    // Draggable points.
    let mut changed = false;
    for (i, g) in gains.iter_mut().enumerate() {
        let c = pos2(x_of(i), y_of(*g));
        let hit = Rect::from_center_size(c, Vec2::splat(22.0));
        let resp = ui.interact(hit, ui.id().with(("eq_band", i)), Sense::drag());
        let label = format!("{} {}", band_label(EQ_BANDS[i]), tr("band"));
        resp.widget_info(|| egui::WidgetInfo::slider(true, *g as f64, &label));
        if resp.dragged()
            && let Some(pos) = ui.input(|inp| inp.pointer.interact_pos())
        {
            *g = egui::remap_clamp(pos.y, plot.top()..=plot.bottom(), EQ_MAX_DB..=-EQ_MAX_DB);
            *g = (*g * 2.0).round() / 2.0; // half-dB steps
            changed = true;
        }
        if resp.has_focus() {
            let step = ui.input(|inp| (inp.key_pressed(egui::Key::ArrowUp) as i32 - inp.key_pressed(egui::Key::ArrowDown) as i32) as f32);
            if step != 0.0 {
                *g = (*g + step).clamp(-EQ_MAX_DB, EQ_MAX_DB);
                changed = true;
            }
        }
        focus_ring(ui, &resp);
        let r = if resp.hovered() || resp.dragged() { 7.0 } else { 5.5 };
        p.circle(pos2(x_of(i), y_of(*g)), r, Color32::WHITE, Stroke::new(1.5, pal().accent));
        if resp.hovered() || resp.dragged() {
            resp.on_hover_text(format!("{:+.1} dB", *g));
        }
    }
    changed
}

/// Under the Discord switch: whether Pulse reached Discord, and what to do if not.
fn discord_link(ui: &mut Ui) {
    use crate::discord::Link;
    let (good, text) = match crate::discord::link() {
        Link::Off | Link::Looking => (true, tr("Looking for the Discord app…").to_string()),
        Link::Connected(true) => (true, tr("Connected to Discord. Your status shows the song that's playing.").to_string()),
        Link::Connected(false) => (true, tr("Connected to Discord. Your status shows up when a song plays.").to_string()),
        Link::NotRunning => (
            false,
            tr("Discord isn't open. Open the Discord desktop app; Pulse finds it within 20 seconds. Using Legcord or Vesktop? Turn on Rich Presence (arRPC) in its settings. The Discord website can't show it.").to_string(),
        ),
        Link::Blocked => (
            false,
            tr("Windows wouldn't let Pulse reach Discord. This happens when one of them runs as administrator and the other doesn't: open both normally.").to_string(),
        ),
        Link::Refused(why) => (false, format!("{} {why}", tr("Discord didn't accept the status:"))),
    };
    let color = if good { pal().text_muted } else { Color32::from_rgb(255, 170, 120) };
    ui.horizontal_wrapped(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(text).font(regular(12.5)).color(color));
    });
    ui.label(
        RichText::new(tr("Connected but nothing shows? In Discord, open User Settings → Activity Privacy and turn on “Share my activity”."))
            .font(regular(12.0))
            .color(pal().text_faint),
    );
    ui.add_space(8.0);
    // The connection changes on another thread: look again in a moment.
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
}
