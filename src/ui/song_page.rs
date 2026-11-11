//! A song's own page, like Spotify's: big cover, play / like / download, the artist,
//! credits, the artist's popular songs and the rest of the album.

use super::{Hero, hero, tracks_block, with_remote};
use crate::app::{App, Page, Route};
use crate::deezer::{AlbumInfo, Credits, Track};
use crate::i18n::tr;
use crate::theme::*;
use crate::widgets::*;
use egui::{Align, CornerRadius, Layout, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use egui_phosphor::regular as icon;

pub fn song_page(app: &mut App, ui: &mut Ui, id: &str) {
    if id.starts_with("dz:") {
        with_remote(app, ui, |app, ui, page| {
            let Page::Song(p) = page else { return };
            draw(app, ui, &p.track, Some(&p.credits), p.album.as_ref(), &p.artist_top);
        });
    } else if let Some(t) = app.song_tracks.get(id).cloned() {
        draw(app, ui, &t, None, None, &[]);
    } else {
        empty_state(ui, icon::MUSIC_NOTE, "Song not found", "Open it again from a list.", None);
    }
}

fn draw(app: &mut App, ui: &mut Ui, t: &Track, credits: Option<&Credits>, album: Option<&(AlbumInfo, Vec<Track>)>, top: &[Track]) {
    let ctx = ui.ctx().clone();
    app.ensure_views(std::slice::from_ref(t));

    // Header: "Artist · Album · 2024 · 3:21 · 1,234,567 views"
    // The artists are links; the rest follows them.
    let artists: Vec<&crate::deezer::ArtistRef> = std::iter::once(&t.artist).chain(&t.featured).collect();
    let names: Vec<&str> = artists.iter().map(|a| a.name.as_str()).collect();
    let mut parts = vec![String::new()];
    if let Some((info, _)) = album {
        parts.push(info.title.clone());
        if let Some(y) = info.year() {
            parts.push(y.to_string());
        }
    } else if let Some(al) = &t.album {
        parts.push(al.title.clone());
    }
    if t.duration > 0 {
        parts.push(fmt_duration(t.duration));
    }
    if let Some((v, _)) = app.saved.views.get(&t.id) {
        parts.push(format!("{} {}", fmt_thousands(*v), tr("views")));
    }
    let sub = parts.join(" · ");
    let clicks = hero(
        ui,
        &Hero {
            editable: false,
            kind: tr("Song"),
            title: &t.title,
            subtitle: &sub,
            art: t.art_big(),
            glyph: icon::MUSIC_NOTE,
            round: false,
            banner: None,
            links: &names,
        },
    );
    if let Some(i) = clicks.link {
        let a = artists[i];
        if i == 0 {
            app.open_artist(t, &ctx);
        } else if crate::deezer::is_deezer_id(&a.id) {
            app.navigate(crate::app::Route::Artist(a.id.clone()), &ctx);
        }
    }

    // Play (the album from this song, so the music carries on), like, download, more.
    let list: Vec<Track> = match album {
        Some((_, tracks)) if tracks.iter().any(|x| x.id == t.id) => tracks.clone(),
        _ => vec![t.clone()],
    };
    let start = list.iter().position(|x| x.id == t.id).unwrap_or(0);
    let playing_this = app.is_playing() && app.current.as_ref().is_some_and(|c| c.id == t.id);
    let mut events = vec![];
    ui.horizontal(|ui| {
        ui.set_min_height(56.0);
        if play_button(ui, 56.0, playing_this).clicked() {
            if playing_this {
                app.toggle_play(&ctx);
            } else {
                app.play_list(&list, start, &ctx);
            }
        }
        ui.add_space(6.0);
        let liked = app.liked_ids.contains(&t.id);
        let (font, col) = if liked { (filled_icon(26.0), pal().accent_soft) } else { (regular(26.0), pal().text_muted) };
        let (r, heart) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
        resp_selected(ui, &heart, liked, "Like");
        paint_icon(ui.painter(), r.center(), icon::HEART, font, if heart.hovered() && !liked { pal().text } else { col });
        if heart.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.toggle_like(t, &ctx);
        }
        if !crate::local::is_local(&t.id) {
            let downloaded = app.is_downloaded(&t.id);
            let col = if downloaded { pal().accent_soft } else { pal().text_muted };
            let tip = if downloaded { tr("Downloaded") } else { tr("Download") };
            if icon_button(ui, icon::ARROW_CIRCLE_DOWN, 26.0, col, tip).clicked() && !downloaded {
                app.download(std::slice::from_ref(t), &ctx);
            }
        }
        let names = app.playlist_names();
        dots_menu(ui, 24.0, tr("More options"), |ui| {
            let mcx = MenuCtx {
                liked: app.liked_ids.contains(&t.id),
                playlists: &names,
                index: None,
                removable: false,
                queue_mode: false,
                downloaded: app.is_downloaded(&t.id),
            };
            track_menu(ui, t, &mcx, &mut events);
        });
    });
    if !events.is_empty() {
        app.handle_events(events, std::slice::from_ref(t), &ctx);
    }
    ui.add_space(12.0);

    // The artist, as a row you can click.
    if t.has_artist_page() {
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(420.0), 76.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(row, r_md(), pal().surface_hover);
        }
        let pic = Rect::from_min_size(row.min + vec2(10.0, 10.0), Vec2::splat(56.0));
        let art = t.artist.picture.as_deref().or_else(|| top.first().and_then(|x| x.artist.picture.as_deref()));
        artwork(ui, pic, art, CornerRadius::same(28));
        paint_line(ui, pos2(pic.right() + 14.0, row.center().y - 18.0), &tr("Artist").to_uppercase(), semi(11.0), pal().text_muted, 300.0);
        paint_line(ui, pos2(pic.right() + 14.0, row.center().y - 2.0), &t.artist.name, semi(16.0), pal().text, 300.0);
        a11y(ui, &resp, egui::WidgetType::Button, || format!("{}: {}", tr("Artist"), t.artist.name));
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.open_artist(t, &ctx);
        }
        ui.add_space(8.0);
    }

    // Credits
    if let Some(c) = credits
        && (!c.people.is_empty() || c.label.is_some())
    {
        section_title(ui, "Credits", None);
        let frame = egui::Frame::new().fill(pal().surface).corner_radius(r_lg()).inner_margin(16.0);
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width().min(560.0));
            for (name, role) in &c.people {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).font(med(14.0)).color(pal().text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(tr(role)).font(regular(12.5)).color(pal().text_muted));
                    });
                });
            }
            let mut facts = vec![];
            if let Some(d) = &c.release_date {
                facts.push(format!("{} {d}", tr("Released")));
            }
            if !c.genres.is_empty() {
                facts.push(c.genres.join(", "));
            }
            if let Some(l) = &c.label {
                facts.push(l.clone());
            }
            if !facts.is_empty() {
                ui.add_space(6.0);
                ui.painter().hline(ui.available_rect_before_wrap().x_range(), ui.cursor().top(), Stroke::new(1.0, pal().border));
                ui.add_space(6.0);
                ui.label(RichText::new(facts.join("  ·  ")).font(regular(12.5)).color(pal().text_muted));
            }
        });
    }

    // The artist's popular songs
    let others: Vec<Track> = top.iter().filter(|x| x.id != t.id).take(5).cloned().collect();
    if !others.is_empty() {
        section_title(ui, &format!("{} {}", tr("Popular tracks by"), t.artist.name), None);
        tracks_block(app, ui, "song_top", &others);
    }

    // The rest of the album, with this song highlighted.
    if let Some((info, tracks)) = album {
        ui.add_space(8.0);
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(row, r_md(), pal().surface_hover);
        }
        let cover = Rect::from_min_size(row.min + vec2(8.0, 8.0), Vec2::splat(56.0));
        artwork(ui, cover, info.cover.as_deref(), r_sm());
        let kind = format!("{} · {}", tr(info.kind()), info.year().map(|y| y.to_string()).unwrap_or_default());
        paint_line(ui, pos2(cover.right() + 14.0, row.center().y - 18.0), kind.trim_end_matches(" · "), semi(11.0), pal().text_muted, 400.0);
        paint_line(ui, pos2(cover.right() + 14.0, row.center().y - 2.0), &info.title, semi(16.0), pal().text, 400.0);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.navigate(Route::Album(info.id.clone()), &ctx);
        }
        tracks_block(app, ui, "song_album", tracks);
    }
}
