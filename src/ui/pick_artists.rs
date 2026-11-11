//! "Pick artists you like": shown once when you start out (after the welcome and your
//! account or profile). The artists you pick are followed, and the home page builds its
//! "Listeners of X also listen to" and "Songs like" rows from them until what you play
//! takes over. Genre buttons narrow the artists shown; the search box finds anyone.

use super::{dialog_button, dialog_frame};
use crate::app::App;
use crate::deezer::{self, ArtistInfo};
use crate::i18n::tr;
use crate::theme::*;
use crate::widgets::*;
use egui::{Align, Color32, CornerRadius, Key, Layout, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as icon;
use std::sync::mpsc::{Receiver, channel};

/// Genres offered (Deezer ids), in the order shown.
const OFFERED: [u32; 12] = [116, 132, 165, 152, 85, 113, 106, 197, 464, 84, 169, 122];
const ENOUGH: usize = 3;

/// A request's number and what came back.
type Answer = (u64, Result<Vec<ArtistInfo>, String>);

#[derive(Default)]
pub struct Picker {
    genres: Vec<u32>,
    query: String,
    shown: Vec<ArtistInfo>,
    picked: Vec<ArtistInfo>,
    loading: bool,
    error: Option<String>,
    /// The newest request, so an older answer arriving late is ignored.
    asked: u64,
    rx: Option<Receiver<Answer>>,
    /// When the search text last changed (it's looked up once typing pauses).
    typed_at: Option<f64>,
}

impl Picker {
    fn load(&mut self, ctx: &egui::Context) {
        self.asked += 1;
        let (n, q, genres) = (self.asked, self.query.trim().to_string(), self.genres.clone());
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.loading = true;
        self.error = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let found = if !q.is_empty() {
                deezer::find_artists(&q)
            } else if genres.is_empty() {
                deezer::popular_artists(0, 36)
            } else {
                // A few from each chosen genre, taking turns, so one genre doesn't fill the grid.
                let lists: Vec<Vec<ArtistInfo>> = genres.iter().take(4).map(|g| deezer::popular_artists(*g, 24).unwrap_or_default()).collect();
                let mut out: Vec<ArtistInfo> = vec![];
                for i in 0..24 {
                    for l in &lists {
                        if let Some(a) = l.get(i)
                            && !out.iter().any(|x| x.id == a.id)
                        {
                            out.push(a.clone());
                        }
                    }
                }
                if out.is_empty() { Err("Couldn't load artists. Check your connection.".into()) } else { Ok(out) }
            };
            let _ = tx.send((n, found));
            ctx.request_repaint();
        });
    }

    fn receive(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok((n, r)) = rx.try_recv() {
            if n != self.asked {
                continue;
            }
            self.loading = false;
            match r {
                Ok(v) => self.shown = v,
                Err(e) => self.error = Some(e),
            }
        }
    }
}

/// Opens the picker (and starts loading popular artists).
pub fn open(app: &mut App, ctx: &egui::Context) {
    let mut p = Picker::default();
    p.load(ctx);
    app.picker = Some(p);
}

/// The window. Call each frame.
pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(p) = &mut app.picker else { return };
    p.receive();
    let now = ctx.input(|i| i.time);
    if let Some(at) = p.typed_at
        && now - at > 0.45
    {
        p.typed_at = None;
        p.load(ctx);
    } else if p.typed_at.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
    let (mut finish, mut skip) = (false, false);
    let screen = ctx.content_rect();
    let width = (screen.width() - 80.0).clamp(420.0, 760.0);
    let grid_h = (screen.height() - 420.0).clamp(220.0, 520.0);
    egui::Modal::new(egui::Id::new("pick_artists")).frame(dialog_frame()).show(ctx, |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(tr("Pick artists you like")).font(heavy(24.0)).color(pal().text));
        ui.add_space(6.0);
        ui.label(
            RichText::new(tr("Choose 3 or more. Your home page starts from them, and from artists like them, until what you play takes over."))
                .font(regular(13.5))
                .color(pal().text_muted),
        );
        ui.add_space(16.0);

        // Search.
        let r =
            egui::Frame::new().fill(pal().surface_hover).corner_radius(CornerRadius::same(18)).inner_margin(egui::Margin::symmetric(14, 9)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    paint_icon(ui.painter(), ui.cursor().min + vec2(8.0, 9.0), icon::MAGNIFYING_GLASS, regular(16.0), pal().text_muted);
                    ui.add_space(22.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut p.query)
                            .id(egui::Id::new("pick_artists_search"))
                            .frame(egui::Frame::NONE)
                            .hint_text(tr("Search for an artist"))
                            .desired_width(f32::INFINITY),
                    )
                })
                .inner
            });
        if r.inner.changed() {
            p.typed_at = Some(now);
        }
        ui.add_space(12.0);

        // Genres.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for g in OFFERED {
                let Some((_, name)) = deezer::GENRES.iter().find(|(id, _)| *id == g) else { continue };
                let on = p.genres.contains(&g);
                if pill(ui, name, on).clicked() {
                    if on {
                        p.genres.retain(|x| *x != g);
                    } else {
                        p.genres.push(g);
                    }
                    p.query.clear();
                    p.typed_at = None;
                    p.load(ui.ctx());
                }
            }
        });
        ui.add_space(14.0);

        // Artists.
        egui::ScrollArea::vertical().max_height(grid_h).min_scrolled_height(grid_h).auto_shrink([false, false]).show(ui, |ui| {
            if let Some(e) = &p.error {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(tr(e)).font(regular(13.5)).color(pal().text_muted)));
                return;
            }
            if p.loading && p.shown.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| ui.spinner());
                return;
            }
            // As many columns as fit, then widened to fill the row.
            let avail = ui.available_width() - 4.0;
            let cols = ((avail + 12.0) / 130.0).floor().max(2.0) as usize;
            let cell = (avail - 12.0 * (cols as f32 - 1.0)) / cols as f32;
            let list = p.shown.clone();
            for row in list.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    for a in row {
                        let picked = p.picked.iter().any(|x| x.id == a.id);
                        if artist(ui, a, picked, cell).clicked() {
                            if picked {
                                p.picked.retain(|x| x.id != a.id);
                            } else {
                                p.picked.push(a.clone());
                            }
                        }
                    }
                });
                ui.add_space(14.0);
            }
        });
        ui.add_space(14.0);

        // Done.
        ui.horizontal(|ui| {
            let n = p.picked.len();
            let note = if n == 0 { tr("None picked yet").to_string() } else { format!("{n} {}", tr("picked")) };
            ui.label(RichText::new(note).font(med(13.0)).color(pal().text_muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let ready = n >= ENOUGH;
                let label = if ready { tr("Continue").to_string() } else { format!("{} {}", tr("Pick"), ENOUGH - n) };
                if dialog_button(ui, &label, true, ready).clicked() {
                    finish = true;
                }
                if dialog_button(ui, tr("Skip"), false, true).clicked() {
                    skip = true;
                }
            });
        });
        if ui.input(|i| i.key_pressed(Key::Enter))
            && p.picked.len() >= ENOUGH
            && !ui.memory(|m| m.focused().is_some_and(|f| f == egui::Id::new("pick_artists_search")))
        {
            finish = true;
        }
    });
    if finish || skip {
        let picked = app.picker.take().map(|p| p.picked).unwrap_or_default();
        app.saved.artists_picked = true;
        app.save_now = true;
        if finish {
            for a in picked.iter().rev() {
                if !app.follows(&a.id) {
                    app.saved.followed_artists.insert(0, a.clone());
                }
            }
            app.cache.remove("home");
            app.toast(tr("Your home page now starts from your artists"), ctx);
        }
    }
}

/// One artist: a round picture with the name under it; a ring and a check when picked.
fn artist(ui: &mut Ui, a: &ArtistInfo, picked: bool, w: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, w + 26.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, picked, &a.name));
    let pic = Rect::from_center_size(pos2(rect.center().x, rect.min.y + w / 2.0), vec2(w - 14.0, w - 14.0));
    if ui.is_rect_visible(rect) {
        let round = CornerRadius::same((pic.width() / 2.0) as u8);
        artwork(ui, pic, a.picture.as_deref(), round);
        let p = ui.painter();
        if picked {
            p.circle_stroke(pic.center(), pic.width() / 2.0 + 3.0, Stroke::new(3.0, pal().accent_soft));
            let badge = pic.right_bottom() - vec2(14.0, 14.0);
            p.circle_filled(badge, 14.0, pal().accent);
            p.circle_stroke(badge, 14.0, Stroke::new(3.0, pal().surface));
            paint_icon(p, badge, icon::CHECK, regular(15.0), Color32::WHITE);
        } else if resp.hovered() {
            p.circle_filled(pic.center(), pic.width() / 2.0, Color32::from_black_alpha(60));
        }
        if resp.has_focus() {
            p.circle_stroke(pic.center(), pic.width() / 2.0 + 6.0, Stroke::new(2.0, pal().text));
        }
        let name = p.layout(a.name.clone(), semi(13.0), pal().text, w);
        let name_pos = pos2(rect.center().x - name.size().x.min(w) / 2.0, pic.max.y + 8.0);
        p.with_clip_rect(rect).galley(name_pos, name, pal().text);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
