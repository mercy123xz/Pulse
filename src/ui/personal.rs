//! The personal parts: home page made from what you listen to, Orbits,
//! your profile page, and the welcome screen where you make your profile.

use super::{Hero, album_grid_visits, hero, open_and_play, page_title, tracks_block, with_remote};
use crate::app::{App, Page, Place, Route, Visit, now_unix, top_artists};
use crate::i18n::tr;
use crate::theme::*;
use crate::widgets::*;
use egui::{Align, Color32, CornerRadius, Layout, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use egui_phosphor::regular as icon;

/// "Good evening, tacent" by the time of day.
fn greeting(name: &str) -> String {
    // Local hour isn't known without a time-zone library; the system clock offset is close enough.
    let hour = chrono_hour();
    let part = match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    };
    if name.is_empty() { tr(part).to_string() } else { format!("{}, {name}", tr(part)) }
}

/// The local hour (0–23), from Windows when possible.
fn chrono_hour() -> u32 {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTime {
            year: u16,
            month: u16,
            dow: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            ms: u16,
        }
        unsafe extern "system" {
            fn GetLocalTime(t: *mut SystemTime);
        }
        let mut t = SystemTime { year: 0, month: 0, dow: 0, day: 0, hour: 12, minute: 0, second: 0, ms: 0 };
        // SAFETY: GetLocalTime only writes the struct we pass.
        unsafe { GetLocalTime(&mut t) };
        t.hour as u32
    }
    #[cfg(not(windows))]
    {
        ((now_unix() / 3600) % 24) as u32
    }
}

/// True when there's enough listening history to make the home page personal.
pub fn has_history(app: &App) -> bool {
    !app.saved.played_artists.is_empty() || !app.saved.liked.is_empty() || !app.saved.playlists.is_empty()
}

/// The home page: your shortcuts, Orbits, recent albums and pages, then discovery.
pub fn home(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    ui.add_space(12.0);
    ui.label(RichText::new(greeting(&app.saved.profile.name)).font(heavy(28.0)).color(pal().text));
    ui.add_space(12.0);

    // ---- Quick tiles: Liked Songs, your playlists, recent albums (up to 8)
    let mut tiles: Vec<(Place, String, Option<String>, &'static str)> = vec![];
    if !app.saved.liked.is_empty() {
        tiles.push((Place::Liked, tr("Liked Songs").to_string(), Some(LIKED_ART.to_owned()), icon::HEART));
    }
    for v in &app.saved.visits {
        if tiles.len() >= 8 {
            break;
        }
        if v.place != Place::Liked && !tiles.iter().any(|t| t.0 == v.place) {
            tiles.push((v.place.clone(), v.title.clone(), v.image.clone(), icon::MUSIC_NOTES));
        }
    }
    for (i, p) in app.saved.playlists.iter().enumerate() {
        let place = Place::Yours(p.name.clone());
        if tiles.len() < 8 && !tiles.iter().any(|t| t.0 == place) {
            tiles.push((place, p.name.clone(), app.playlist_art(i), icon::PLAYLIST));
        }
    }
    // Your playlists show their current picture, not the one from when you last opened them.
    for t in tiles.iter_mut() {
        if let Place::Yours(name) = &t.0
            && let Some(i) = app.saved.playlists.iter().position(|p| p.name == *name)
        {
            t.2 = app.playlist_art(i);
        }
    }
    for a in &app.saved.played_albums {
        if tiles.len() < 8 && !tiles.iter().any(|t| t.0 == a.place) {
            tiles.push((a.place.clone(), a.title.clone(), a.image.clone(), icon::VINYL_RECORD));
        }
    }
    if !tiles.is_empty() {
        let mut action = None;
        let gap = 8.0;
        let cols = if ui.available_width() > 900.0 { 4 } else { 2 };
        let w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
        for row in tiles.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (place, title, art, glyph) in row {
                    match quick_tile(ui, w, art.as_deref(), glyph, title, egui::Id::new(("tile", format!("{place:?}")))) {
                        CardClick::Open => action = Some((place.clone(), false)),
                        CardClick::Play => action = Some((place.clone(), true)),
                        CardClick::None => {}
                    }
                }
            });
            ui.add_space(gap);
        }
        if let Some((place, play)) = action
            && let Some(route) = app.route_for(&place)
        {
            if play { open_and_play(app, route, &ctx) } else { app.navigate(route, &ctx) }
        }
    }

    // ---- Orbits: a mix around each of the artists you play most
    let top: Vec<Visit> = top_artists(&app.saved.played_artists, now_unix()).into_iter().take(6).cloned().collect();
    if !top.is_empty() {
        ui.add_space(12.0);
        section_title(ui, "Your Orbits", None);
        ui.label(RichText::new(tr("Mixes that circle the artists you play most")).font(regular(12.5)).color(pal().text_muted));
        ui.add_space(4.0);
        let mut action = None;
        let labels: Vec<String> = top.iter().map(|a| orbit_title(&a.title)).collect();
        let items: Vec<(usize, &Visit)> = top.iter().enumerate().collect();
        card_grid(ui, &items, Some(1), |ui, w, (i, a)| {
            let sub = format!("{} {}", a.title, tr("and similar artists"));
            let info = CardInfo { id: egui::Id::new(("mix", *i)), art: a.image.as_deref(), title: &labels[*i], subtitle: sub, round: false, badge: Some(tr("Orbit")) };
            if let Place::Artist(id) = &a.place {
                match card(ui, w, &info) {
                    CardClick::Open => action = Some((id.clone(), false)),
                    CardClick::Play => action = Some((id.clone(), true)),
                    CardClick::None => {}
                }
            }
        });
        match action {
            Some((id, false)) => app.navigate(Route::Mix(id), &ctx),
            Some((id, true)) => open_and_play(app, Route::Mix(id), &ctx),
            None => {}
        }
    }

    // ---- Jump back in: albums and artists you played lately
    let mut jump: Vec<Visit> = vec![];
    let (mut al, mut ar) = (app.saved.played_albums.iter(), app.saved.played_artists.iter());
    while jump.len() < 12 {
        let a = al.next();
        let b = if jump.len() % 3 == 2 { ar.next() } else { None };
        if a.is_none() && b.is_none() && ar.len() == 0 {
            break;
        }
        jump.extend(a.cloned());
        jump.extend(b.cloned());
        if a.is_none() && b.is_none() {
            jump.extend(ar.next().cloned());
        }
    }
    if !jump.is_empty() {
        section_title(ui, "Jump back in", None);
        album_grid_visits(app, ui, &jump, Some(1));
    }

    // ---- Recents: pages you opened
    let recents: Vec<Visit> = app.saved.visits.iter().take(12).cloned().collect();
    if !recents.is_empty() {
        section_title(ui, "Recents", None);
        album_grid_visits(app, ui, &recents, Some(1));
    }
    ui.add_space(8.0);
}

/// A wide shortcut tile (picture + name), like Spotify's top-of-home grid.
fn quick_tile(ui: &mut Ui, w: f32, art: Option<&str>, glyph: &str, title: &str, id: egui::Id) -> CardClick {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 56.0), Sense::click());
    a11y(ui, &resp, egui::WidgetType::Button, || title.to_owned());
    focus_ring(ui, &resp);
    let hovered = resp.hovered();
    let fill = if hovered { pal().surface_active } else { pal().surface_hover };
    ui.painter().rect_filled(rect, r_md(), fill);
    let art_rect = Rect::from_min_size(rect.min, Vec2::splat(56.0));
    let r = CornerRadius { nw: r_md().nw, sw: r_md().sw, ne: 0, se: 0 };
    if art.is_some() {
        artwork(ui, art_rect, art, r);
    } else {
        ui.painter().rect_filled(art_rect, r, pal().accent);
        vgradient(ui, art_rect, Color32::from_white_alpha(40), Color32::TRANSPARENT);
        paint_icon(ui.painter(), art_rect.center(), glyph, filled_icon(24.0), Color32::WHITE);
    }
    let text_w = w - 56.0 - 64.0;
    paint_line(ui, pos2(art_rect.right() + 12.0, rect.center().y - 9.0), title, semi(13.5), pal().text, text_w);
    let mut out = if resp.clicked() { CardClick::Open } else { CardClick::None };
    let t = ui.ctx().animate_bool(id.with("hover"), hovered);
    if t > 0.0 {
        let c = pos2(rect.right() - 26.0, rect.center().y);
        let b = ui.interact(Rect::from_center_size(c, Vec2::splat(36.0)), id.with("play"), Sense::click());
        a11y(ui, &b, egui::WidgetType::Button, || format!("Play {title}"));
        let pb = &parts().play_button;
        ui.painter().circle_filled(c, 17.0, pb.fill.unwrap_or(pal().accent).gamma_multiply(t));
        paint_icon(ui.painter(), c, icon::PLAY, filled_icon(14.0), pb.color.unwrap_or(Color32::WHITE).gamma_multiply(t));
        if b.clicked() {
            out = CardClick::Play;
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    out
}

/// "Lil Baby Orbit": a mix built around one artist.
pub fn orbit_title(artist: &str) -> String {
    tr("{} Orbit").replace("{}", artist)
}

/// An Orbit page.
pub fn mix_page(app: &mut App, ui: &mut Ui, artist_id: &str) {
    let seed = app.saved.played_artists.iter().find(|a| a.place == Place::Artist(artist_id.to_string())).cloned();
    let name = seed.as_ref().map(|s| s.title.clone()).unwrap_or_default();
    let image = seed.and_then(|s| s.image);
    let title = orbit_title(&name);
    with_remote(app, ui, |app, ui, page| {
        let Page::Tracks(tracks) = page else { return };
        let n = tracks.len();
        let sub = format!("{name} {} · {n} {} · {}", tr("and similar artists"), tr(if n == 1 { "song" } else { "songs" }), tr("Updated daily"));
        hero(ui, &Hero { editable: false, kind: tr("Orbit"), title: &title, subtitle: &sub, art: image.as_deref(), glyph: icon::PLANET, round: false, banner: None, links: &[] });
        app.note_visit(Place::Mix(artist_id.to_string()), &title, "Orbit", image.as_deref(), false);
        let ctx = ui.ctx().clone();
        super::autoplay_page(app, &ctx, tracks);
        super::action_row(app, ui, tracks);
        tracks_block(app, ui, "mix", tracks);
    });
}

/// A round profile picture, or the first letter of the name.
pub fn avatar(ui: &Ui, rect: Rect, app: &App) {
    let p = &app.saved.profile;
    let r = CornerRadius::same((rect.width() / 2.0).min(255.0) as u8);
    match &p.picture {
        Some(pic) => artwork(ui, rect, Some(&crate::settings::file_uri(pic)), r),
        None => {
            ui.painter().circle_filled(rect.center(), rect.width() / 2.0, pal().accent);
            let letter = p.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| icon::USER.to_string());
            let font = if p.name.is_empty() { regular(rect.width() * 0.5) } else { heavy(rect.width() * 0.45) };
            paint_icon(ui.painter(), rect.center(), letter, font, Color32::WHITE);
        }
    }
}

/// Your profile page.
pub fn profile_page(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 232.0), Sense::hover());
    ui.painter().rect_filled(rect, r_lg(), Color32::from_rgb(14, 13, 20).lerp_to_gamma(pal().accent, 0.18));
    vgradient(ui, rect.shrink(1.0), pal().accent.gamma_multiply(0.25), Color32::TRANSPARENT);
    let pic = Rect::from_min_size(pos2(rect.left() + 24.0, rect.center().y - 92.0), Vec2::splat(184.0));
    ui.painter().add(egui::Shadow { offset: [0, 10], blur: 28, spread: 0, color: Color32::from_black_alpha(110) }.as_shape(pic, CornerRadius::same(92)));
    avatar(ui, pic, app);
    let edit = ui.interact(pic, egui::Id::new("profile_pic"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if edit.hovered() {
        ui.painter().circle_filled(pic.center(), 92.0, Color32::from_black_alpha(140));
        paint_icon(ui.painter(), pic.center(), icon::PENCIL_SIMPLE, regular(40.0), Color32::WHITE);
    }
    let tx = pic.right() + 28.0;
    let tw = rect.right() - tx - 24.0;
    let p = app.saved.profile.clone();
    paint_line(ui, pos2(tx, pic.bottom() - 112.0), &tr("Profile").to_uppercase(), semi(11.5), ON_DARK_MUTED, tw);
    let name = if p.name.is_empty() { tr("Your profile").to_string() } else { p.name.clone() };
    let g = one_line(ui, &name, heavy(52.0), ON_DARK, tw);
    let name_rect = Rect::from_min_size(pos2(tx, pic.bottom() - 30.0 - g.size().y), g.size());
    ui.painter().galley(name_rect.min, g, ON_DARK);
    let n_pl = app.saved.playlists.len();
    let since = if p.created > 0 { format!(" · {} {}", tr("Member since"), fmt_added(p.created, u64::MAX / 2).rsplit(' ').next().unwrap_or("")) } else { String::new() };
    let stats = format!("{n_pl} {} · {} {}{since}", tr(if n_pl == 1 { "playlist" } else { "playlists" }), app.saved.liked.len(), tr("liked songs"));
    paint_line(ui, pos2(tx, pic.bottom() - 20.0), &stats, regular(13.5), ON_DARK_MUTED, tw);
    let name_click = ui.interact(name_rect, egui::Id::new("profile_name"), Sense::click()).on_hover_cursor(egui::CursorIcon::Text);
    if edit.clicked() || name_click.clicked() {
        app.profile_editing = Some(ProfileForm::from(&p));
    }
    ui.add_space(18.0);

    ui.horizontal(|ui| {
        if ghost_button(ui, icon::PENCIL_SIMPLE, "Edit profile").clicked() {
            app.profile_editing = Some(ProfileForm::from(&p));
        }
    });
    if !p.bio.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(&p.bio).font(regular(13.5)).color(pal().text_muted));
    }

    // Top artists
    let top: Vec<Visit> = top_artists(&app.saved.played_artists, now_unix()).into_iter().take(12).cloned().collect();
    if !top.is_empty() {
        section_title(ui, "Top artists", None);
        ui.label(RichText::new(tr("Only visible to you")).font(regular(12.0)).color(pal().text_muted));
        ui.add_space(4.0);
        album_grid_visits(app, ui, &top, Some(1));
    }

    // Your playlists, with sharing
    if !app.saved.playlists.is_empty() {
        section_title(ui, "Playlists", None);
        let mut share = None;
        let mut open = None;
        let names: Vec<(usize, String, usize, Option<String>)> = app
            .saved
            .playlists
            .iter()
            .enumerate()
            .map(|(i, pl)| (i, pl.name.clone(), pl.tracks.len(), app.playlist_art(i)))
            .collect();
        for (i, name, n, art) in &names {
            ui.horizontal(|ui| {
                let (r, resp) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::click());
                artwork(ui, r, art.as_deref(), r_sm());
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    let t = ui.add(egui::Label::new(RichText::new(name).font(semi(14.0)).color(pal().text)).sense(Sense::click()));
                    ui.label(RichText::new(format!("{n} {}", tr(if *n == 1 { "song" } else { "songs" }))).font(regular(12.0)).color(pal().text_muted));
                    if t.clicked() || resp.clicked() {
                        open = Some(*i);
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost_button(ui, icon::SHARE_FAT, "Copy share code").clicked() {
                        share = Some(*i);
                    }
                });
            });
            ui.add_space(6.0);
        }
        if let Some(i) = share {
            app.share_playlist(i, &ctx);
        }
        if let Some(i) = open {
            app.navigate(Route::Local(i), &ctx);
        }
    }

    // Recently played
    let recent: Vec<_> = app.saved.recent.iter().take(10).cloned().collect();
    if !recent.is_empty() {
        if section_title(ui, "Recently played", Some("Show all")) {
            app.navigate(Route::Recent, &ctx);
        }
        tracks_block(app, ui, "profile_recent", &recent);
    }
    let _ = page_title;
}

/// The profile form (welcome screen and "Edit profile").
#[derive(Debug, Clone, Default)]
pub struct ProfileForm {
    pub name: String,
    pub bio: String,
    pub picture: Option<std::path::PathBuf>,
    /// First launch ("Create your profile") rather than editing.
    pub welcome: bool,
}

impl ProfileForm {
    pub fn from(p: &crate::app::Profile) -> Self {
        Self { name: p.name.clone(), bio: p.bio.clone(), picture: p.picture.clone(), welcome: false }
    }
}

/// Usernames: 2–30 characters; letters, numbers, spaces, `_ . -`.
pub fn check_name(name: &str) -> Result<(), &'static str> {
    let n = name.trim();
    if n.chars().count() < 2 {
        return Err("Pick a name with at least 2 characters.");
    }
    if n.chars().count() > 30 {
        return Err("Names can be up to 30 characters.");
    }
    if !n.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-')) {
        return Err("Use letters, numbers, spaces, _ . or -");
    }
    Ok(())
}

/// The "Create your profile" / "Edit profile" window.
pub fn profile_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(mut form) = app.profile_editing.take() else { return };
    let mut keep = true;
    let mut save = false;
    egui::Modal::new(egui::Id::new("profile_form")).show(ctx, |ui| {
        ui.set_width(420.0);
        if form.welcome {
            ui.vertical_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(14), pal().accent);
                paint_icon(ui.painter(), r.center(), icon::WAVEFORM, filled_icon(30.0), Color32::WHITE);
                ui.add_space(8.0);
                ui.label(RichText::new(tr("Welcome to Pulse")).font(heavy(24.0)).color(pal().text));
                ui.label(RichText::new(tr("Create your profile. It's saved on this computer only: no email or password.")).font(regular(13.0)).color(pal().text_muted));
            });
        } else {
            ui.label(RichText::new(tr("Edit profile")).font(heavy(20.0)).color(pal().text));
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            // Picture
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::click());
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr("Choose picture"));
            match &form.picture {
                Some(p) => artwork(ui, r, Some(&crate::settings::file_uri(p)), CornerRadius::same(48)),
                None => {
                    ui.painter().circle_filled(r.center(), 48.0, pal().surface_active);
                    paint_icon(ui.painter(), r.center(), icon::CAMERA, regular(30.0), pal().text_muted);
                }
            }
            if resp.hovered() {
                ui.painter().circle_stroke(r.center(), 48.0, Stroke::new(2.0, pal().accent_soft));
            }
            #[cfg(windows)]
            if resp.clicked()
                && let Some(file) = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]).pick_file()
            {
                form.picture = Some(file);
            }
            #[cfg(not(windows))]
            let _ = resp;
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(tr("Username")).font(med(12.5)).color(pal().text_muted));
                let r = ui.add(egui::TextEdit::singleline(&mut form.name).hint_text(tr("What should we call you?")).char_limit(30).desired_width(f32::INFINITY));
                if form.welcome && form.name.is_empty() && !r.has_focus() {
                    r.request_focus();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                ui.add_space(6.0);
                ui.label(RichText::new(tr("About you (optional)")).font(med(12.5)).color(pal().text_muted));
                ui.add(egui::TextEdit::singleline(&mut form.bio).char_limit(120).desired_width(f32::INFINITY));
            });
        });
        let problem = check_name(&form.name).err();
        if let Some(p) = problem
            && !form.name.is_empty()
        {
            ui.add_space(4.0);
            ui.label(RichText::new(tr(p)).font(regular(12.0)).color(pal().accent_soft));
        }
        ui.add_space(14.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if form.welcome { tr("Create profile") } else { tr("Save") };
            if ui.add_enabled(problem.is_none(), egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(pal().accent)).clicked() {
                save = true;
            }
            let cancel = if form.welcome { tr("Later") } else { tr("Cancel") };
            if ui.button(cancel).clicked() {
                keep = false;
                if form.welcome {
                    app.saved.profile.skipped = true;
                }
            }
        });
    });
    if save && check_name(&form.name).is_ok() {
        if let Err(e) = app.save_profile(&form) {
            app.toast(&e, ctx);
        }
        keep = false;
    }
    if keep {
        app.profile_editing = Some(form);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn usernames() {
        assert!(super::check_name("tacent").is_ok());
        assert!(super::check_name("DJ x.y-z_1").is_ok());
        assert!(super::check_name("a").is_err());
        assert!(super::check_name("<script>").is_err());
        assert!(super::check_name(&"a".repeat(31)).is_err());
    }
}
