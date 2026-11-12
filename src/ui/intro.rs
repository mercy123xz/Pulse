//! The first time Pulse opens: the five Pulse bars bounce to a short beat
//! ("bum, ba-dum-dum-DUM", one bar per hit), then the middle one grows and sweeps across
//! the window, and the app is behind it. Every launch after that: a small "Welcome back,
//! @you" in the middle of the window for a moment.
//!
//! A click or a key skips either one. With "Reduce motion" (or Gaming and Ultra-low
//! modes) there's no intro, only the welcome.

use crate::app::App;
use crate::audio::INTRO_HITS;
use crate::i18n::tr;
use crate::theme::*;
use egui::{Color32, CornerRadius, Id, Pos2, Rect, Sense, Stroke, pos2, vec2};
use egui_phosphor::regular as icon;

/// The bars' heights, left to right, relative to the tallest.
const HEIGHTS: [f32; 5] = [0.34, 0.68, 1.0, 0.54, 0.21];
/// When the beat starts (seconds into the intro): a moment to see the bars first.
const LEAD: f64 = 0.35;
/// The sweep: the middle bar grows to the window's height, then its width, then slides away.
const GROW: f64 = 0.25;
const WIDEN: f64 = 0.35;
const SLIDE: f64 = 0.5;

pub enum Intro {
    /// The intro with the beat, every time Pulse opens. `started`: when it began; `played`:
    /// the beat has been started; `back`: Pulse has been opened before, so "Welcome back"
    /// follows it (with this name).
    First { started: Option<f64>, played: bool, back: Option<String> },
    /// "Welcome back" (with your name, if Pulse knows it).
    Welcome { started: Option<f64>, name: String },
}

impl Intro {
    /// What to show as Pulse opens: the intro (then "Welcome back" if you've been here
    /// before), or only "Welcome back" when animations are off.
    pub fn for_launch(first: bool, still: bool, name: String) -> Self {
        if still { Intro::Welcome { started: None, name } } else { Intro::First { started: None, played: false, back: (!first).then_some(name) } }
    }
}

/// Draws the intro or the welcome over everything. Call last in the frame.
pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(intro) = &mut app.intro else { return };
    let now = ctx.input(|i| i.time);
    let skip = ctx.input(|i| i.pointer.any_pressed() || i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })));
    let done = match intro {
        Intro::First { started, played, .. } => {
            let t0 = *started.get_or_insert(now);
            if !*played && now - t0 >= LEAD {
                *played = true;
                let volume = (app.saved.volume * 0.8).clamp(0.15, 0.8);
                app.audio.play_intro(volume);
            }
            let t = now - t0;
            let end = LEAD + INTRO_HITS[4].0 as f64 + 0.1 + GROW + WIDEN + SLIDE;
            if t < end && !skip {
                first(ctx, t);
                ctx.request_repaint();
                false
            } else {
                true
            }
        }
        Intro::Welcome { started, name } => {
            let t = now - *started.get_or_insert(now);
            if t < 1.9 && !(skip && t > 0.1) {
                welcome(ctx, t, name);
                ctx.request_repaint();
                false
            } else {
                true
            }
        }
    };
    if done {
        let mut next = None;
        if let Some(Intro::First { back, .. }) = &mut app.intro {
            next = back.take().map(|name| Intro::Welcome { started: None, name });
            if !app.saved.intro_seen {
                app.saved.intro_seen = true;
                app.save_now = true;
            }
        }
        app.intro = next;
        if app.intro.is_some() {
            ctx.request_repaint();
        }
    }
}

fn ease(x: f64) -> f32 {
    let x = x.clamp(0.0, 1.0) as f32;
    if x < 0.5 { 4.0 * x * x * x } else { 1.0 - (-2.0 * x + 2.0).powi(3) / 2.0 }
}

fn first(ctx: &egui::Context, t: f64) {
    let screen = ctx.content_rect();
    let beat = t - LEAD;
    let last = INTRO_HITS[4].0 as f64;
    let sweep = beat - last - 0.1; // seconds into the sweep (negative before it)
    let accent = pal().accent;
    egui::Area::new(Id::new("pulse_intro")).order(egui::Order::Tooltip).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
        ui.allocate_rect(screen, Sense::click());
        let p = ui.painter();
        // Until the wall covers the window, the intro's own dark background with a purple glow.
        if sweep < GROW + WIDEN {
            p.rect_filled(screen, CornerRadius::ZERO, Color32::from_rgb(11, 11, 14));
            let glow = screen.center();
            // A soft glow: many faint circles, each a little smaller, so it fades smoothly.
            let reach = screen.height().min(screen.width()) * 0.45;
            for i in 0..40 {
                let k = i as f32 / 40.0;
                p.circle_filled(glow, reach * (1.0 - k), Color32::from_rgba_unmultiplied(124, 58, 237, (2.0 + 2.5 * k) as u8));
            }
        }
        // The bars.
        let unit = (screen.height() * 0.16).clamp(70.0, 140.0);
        let w = unit * 0.19;
        let gap = unit * 0.12;
        let total = w * 5.0 + gap * 4.0;
        let left = screen.center().x - total / 2.0;
        let mut middle = Rect::NOTHING;
        if sweep < GROW + WIDEN {
            for (i, h) in HEIGHTS.iter().enumerate() {
                let hit = INTRO_HITS[i].0 as f64;
                let since = beat - hit;
                let bounce = if (0.0..0.34).contains(&since) { (since / 0.34 * std::f64::consts::PI).sin() as f32 } else { 0.0 };
                let height = unit * h * (1.0 + 0.3 * bounce);
                let x = left + i as f32 * (w + gap);
                let cy = screen.center().y - unit * 0.12 * bounce;
                let r = Rect::from_center_size(pos2(x + w / 2.0, cy), vec2(w, height));
                let lit = beat >= last;
                let color = if lit { Color32::from_rgb(196, 181, 253) } else { Color32::WHITE };
                if i == 2 {
                    middle = r;
                }
                if !(i == 2 && sweep > 0.0) {
                    p.rect_filled(r, CornerRadius::same((w / 2.0) as u8), color);
                }
            }
            if beat < 0.0 {
                let name = p.layout_no_wrap("PULSE".into(), semi(15.0), Color32::from_rgb(196, 181, 253));
                p.galley(pos2(screen.center().x - name.size().x / 2.0, screen.center().y + unit * 0.75), name, Color32::WHITE);
            }
        }
        // The sweep: the middle bar becomes a purple wall across the window, then slides off.
        if sweep > 0.0 {
            let start = if middle == Rect::NOTHING { Rect::from_center_size(screen.center(), vec2(w, unit)) } else { middle };
            let wall = if sweep < GROW {
                let k = ease(sweep / GROW);
                Rect::from_min_max(pos2(start.min.x, lerp(start.min.y, screen.min.y, k)), pos2(start.max.x, lerp(start.max.y, screen.max.y, k)))
            } else if sweep < GROW + WIDEN {
                let k = ease((sweep - GROW) / WIDEN);
                Rect::from_min_max(pos2(lerp(start.min.x, screen.min.x, k), screen.min.y), pos2(lerp(start.max.x, screen.max.x, k), screen.max.y))
            } else {
                let k = ease((sweep - GROW - WIDEN) / SLIDE);
                screen.translate(vec2(screen.width() * k, 0.0))
            };
            let radius = if sweep < GROW { (w / 2.0) as u8 } else { 0 };
            p.rect_filled(wall, CornerRadius::same(radius), accent);
        }
    });
}

fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

fn welcome(ctx: &egui::Context, t: f64, name: &str) {
    // Fades in, stays, fades out.
    let a = if t < 0.25 {
        t / 0.25
    } else if t < 1.5 {
        1.0
    } else {
        1.0 - (t - 1.5) / 0.4
    }
    .clamp(0.0, 1.0) as f32;
    let still = reduce_motion();
    let scale = if still { 1.0 } else { 0.96 + 0.04 * a };
    let text = if name.is_empty() { tr("Welcome back").to_string() } else { format!("{}, {name}", tr("Welcome back")) };
    let screen = ctx.content_rect();
    egui::Area::new(Id::new("pulse_welcome")).order(egui::Order::Tooltip).fixed_pos(Pos2::ZERO).interactable(false).show(ctx, |ui| {
        let p = ui.painter();
        let galley = p.layout_no_wrap(text, heavy(19.0 * scale), Color32::WHITE.gamma_multiply(a));
        let bars = 26.0 * scale;
        let size = vec2(galley.size().x + bars + 36.0 + 30.0, 58.0 * scale);
        let r = Rect::from_center_size(screen.center(), size);
        p.rect(
            r,
            CornerRadius::same(16),
            Color32::from_rgba_unmultiplied(22, 22, 27, (205.0 * a) as u8),
            Stroke::new(1.0, Color32::from_white_alpha((26.0 * a) as u8)),
            egui::StrokeKind::Inside,
        );
        // The little Pulse bars, then the words, then a wave.
        let x0 = r.min.x + 18.0;
        for (i, h) in HEIGHTS.iter().enumerate() {
            let bh = 22.0 * scale * h.max(0.3);
            let bx = x0 + i as f32 * 5.5 * scale;
            p.rect_filled(Rect::from_center_size(pos2(bx, r.center().y), vec2(3.5 * scale, bh)), CornerRadius::same(2), pal().accent_soft.gamma_multiply(a));
        }
        let tx = x0 + bars + 4.0;
        p.galley(pos2(tx, r.center().y - galley.size().y / 2.0), galley.clone(), Color32::WHITE);
        paint_icon(
            p,
            pos2(tx + galley.size().x + 16.0, r.center().y),
            icon::HAND_WAVING,
            regular(20.0 * scale),
            Color32::from_rgb(253, 224, 71).gamma_multiply(a),
        );
    });
}
