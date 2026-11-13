//! First-time setup: shown once, the first time Pulse opens on a computer, before the intro.
//!
//! 1. Welcome to Pulse
//! 2. Make an account (or log in, or skip: accounts are optional)
//! 3. Pick 3 artists you like
//! 4. Your settings: Gaming Mode, Ultra-low, Discord status, explicit songs
//! 5. Your theme
//! 6. "Perfect, welcome in", then the intro with the beat
//!
//! Frosted-glass panels over a slowly drifting purple backdrop. Every step can be skipped,
//! and everything chosen here can be changed later in Settings.

use super::pick_artists::{self, Picker};
use crate::app::App;
use crate::i18n::tr;
use crate::theme::*;
use egui::{Align, Color32, CornerRadius, Id, Key, Layout, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use std::sync::mpsc::{Receiver, channel};

const STEPS: usize = 5; // the dots: welcome, account, artists, settings, theme

const BASE: Color32 = Color32::from_rgb(14, 8, 26);
const VIOLET: Color32 = Color32::from_rgb(124, 58, 237);
const LAVENDER: Color32 = Color32::from_rgb(196, 181, 253);
const GLASS: Color32 = Color32::from_rgba_premultiplied(18, 14, 30, 150);
const GLASS_EDGE: Color32 = Color32::from_rgba_premultiplied(60, 52, 80, 90);
const TEXT: Color32 = Color32::from_rgb(244, 241, 252);
const MUTED: Color32 = Color32::from_rgb(178, 170, 200);
const FIELD: Color32 = Color32::from_rgba_premultiplied(8, 6, 14, 120);

#[derive(PartialEq, Clone, Copy)]
enum Step {
    Welcome,
    Account,
    Artists,
    Settings,
    Theme,
    Done,
}

pub struct Setup {
    step: Step,
    /// When the current step started (for its fade-in).
    since: f64,
    // Account
    log_in: bool,
    email: String,
    username: String,
    password: String,
    password2: String,
    busy: bool,
    error: Option<String>,
    rx: Option<Receiver<Result<crate::account::Session, String>>>,
    /// The name "welcome in" uses.
    name: String,
    // Artists
    picker: Option<Picker>,
}

impl Setup {
    pub fn new() -> Self {
        Setup {
            step: Step::Welcome,
            since: -1.0,
            log_in: false,
            email: String::new(),
            username: String::new(),
            password: String::new(),
            password2: String::new(),
            busy: false,
            error: None,
            rx: None,
            name: String::new(),
            picker: None,
        }
    }
}

impl Default for Setup {
    fn default() -> Self {
        Self::new()
    }
}

fn go(s: &mut Setup, step: Step) {
    s.step = step;
    s.since = -1.0;
    s.error = None;
}

/// The step after `step`, skipping ones that don't apply.
fn next(step: Step) -> Step {
    let signed_in = crate::account::session().is_some();
    match step {
        Step::Welcome if crate::account::available() && !signed_in => Step::Account,
        Step::Welcome | Step::Account => Step::Artists,
        Step::Artists => Step::Settings,
        Step::Settings => Step::Theme,
        Step::Theme | Step::Done => Step::Done,
    }
}

/// Draws setup over everything, while it's open. Call each frame.
pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(mut s) = app.setup.take() else { return };
    let now = ctx.input(|i| i.time);
    if s.since < 0.0 {
        s.since = now;
    }
    let screen = ctx.content_rect();
    let mut finished = false;
    let fade = ((now - s.since) / 0.35).clamp(0.0, 1.0) as f32;
    let rise = (1.0 - fade).powi(3) * 18.0;
    // The backdrop covers the whole window, so nothing behind it can be clicked.
    egui::Area::new(Id::new("first_setup_bg")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.allocate_rect(screen, Sense::click());
        backdrop(ui, screen, now);
        if s.step == Step::Done {
            ui.multiply_opacity(fade);
            finished = done_step(ui, screen, &s, now);
        }
    });
    if s.step != Step::Done {
        let wide = s.step == Step::Artists || s.step == Step::Theme;
        let w = (screen.width() - 80.0).clamp(380.0, if wide { 760.0 } else { 560.0 });
        let max_h = screen.height() - 60.0;
        egui::Area::new(Id::new("first_setup_panel")).order(egui::Order::Tooltip).anchor(egui::Align2::CENTER_CENTER, [0.0, rise]).constrain_to(screen).show(
            ctx,
            |ui| {
                ui.multiply_opacity(fade);
                egui::Frame::new()
                    .fill(GLASS)
                    .stroke(Stroke::new(1.0, GLASS_EDGE))
                    .corner_radius(CornerRadius::same(22))
                    .inner_margin(Margin::same(32))
                    .shadow(egui::Shadow { offset: [0, 12], blur: 40, spread: 0, color: Color32::from_black_alpha(90) })
                    .show(ui, |ui| {
                        ui.set_width(w - 64.0);
                        ui.style_mut().visuals.override_text_color = Some(TEXT);
                        dots(ui, &s);
                        ui.add_space(18.0);
                        match s.step {
                            Step::Welcome => welcome_step(ui, &mut s),
                            Step::Account => account_step(ui, &mut s),
                            Step::Artists => artists_step(app, ui, &mut s, max_h),
                            Step::Settings => settings_step(app, ui, &mut s),
                            Step::Theme => theme_step(app, ui, &mut s),
                            Step::Done => {}
                        }
                    });
            },
        );
    }
    receive_account(app, &mut s, ctx);
    ctx.request_repaint(); // the backdrop drifts
    if finished {
        app.saved.setup_done = true;
        app.save_now = true;
        let still = app.saved.settings.reduce_motion;
        app.intro = Some(crate::ui::intro::Intro::for_launch(true, still, String::new()));
    } else {
        app.setup = Some(s);
    }
}

/// Deep purple with soft violet lights drifting slowly behind the glass.
fn backdrop(ui: &Ui, r: Rect, now: f64) {
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::ZERO, BASE);
    let t = now as f32;
    let blobs = [
        (0.22 + 0.05 * (t * 0.13).sin(), 0.30 + 0.06 * (t * 0.11).cos(), 0.55, VIOLET, 0.55),
        (0.80 + 0.04 * (t * 0.09).cos(), 0.70 + 0.05 * (t * 0.12).sin(), 0.60, Color32::from_rgb(168, 85, 247), 0.45),
        (0.62 + 0.06 * (t * 0.07).sin(), 0.15 + 0.04 * (t * 0.10).sin(), 0.40, Color32::from_rgb(99, 102, 241), 0.35),
        (0.10 + 0.04 * (t * 0.08).cos(), 0.90 + 0.03 * (t * 0.14).cos(), 0.45, Color32::from_rgb(192, 38, 211), 0.25),
    ];
    for (x, y, size, color, strength) in blobs {
        let c = pos2(r.left() + r.width() * x, r.top() + r.height() * y);
        glow(p, c, r.width().max(r.height()) * size * 0.5, color, strength);
    }
}

/// A round light that fades out to its edge.
fn glow(p: &egui::Painter, c: Pos2, radius: f32, color: Color32, strength: f32) {
    let mut mesh = egui::Mesh::default();
    let center = color.gamma_multiply(strength);
    mesh.colored_vertex(c, center);
    let n = 48;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * radius, Color32::TRANSPARENT);
    }
    for i in 1..=n as u32 {
        mesh.add_triangle(0, i, i + 1);
    }
    p.add(mesh);
}

fn dots(ui: &mut Ui, s: &Setup) {
    let at = match s.step {
        Step::Welcome => 0,
        Step::Account => 1,
        Step::Artists => 2,
        Step::Settings => 3,
        Step::Theme | Step::Done => 4,
    };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
    let total = STEPS as f32 * 8.0 + (STEPS as f32 - 1.0) * 6.0 + 16.0;
    let mut x = rect.center().x - total / 2.0;
    for i in 0..STEPS {
        let w = if i == at { 24.0 } else { 8.0 };
        let col = if i <= at { LAVENDER } else { Color32::from_white_alpha(40) };
        ui.painter().rect_filled(Rect::from_min_size(pos2(x, rect.top()), vec2(w, 8.0)), CornerRadius::same(4), col);
        x += w + 6.0;
    }
}

fn title(ui: &mut Ui, text: &str) {
    ui.vertical_centered(|ui| ui.label(RichText::new(tr(text)).font(heavy(26.0)).color(TEXT)));
}

fn subtitle(ui: &mut Ui, text: &str) {
    ui.vertical_centered(|ui| ui.add(egui::Label::new(RichText::new(tr(text)).font(regular(14.0)).color(MUTED)).wrap()));
}

/// A rounded button: filled violet (primary) or glass (secondary).
fn button(ui: &mut Ui, label: &str, primary: bool, enabled: bool) -> egui::Response {
    let text = RichText::new(tr(label)).font(semi(14.0)).color(if primary { Color32::WHITE } else { TEXT });
    let b = egui::Button::new(text)
        .min_size(vec2(120.0, 40.0))
        .corner_radius(CornerRadius::same(20))
        .fill(if primary { VIOLET } else { Color32::from_white_alpha(14) })
        .stroke(if primary { Stroke::NONE } else { Stroke::new(1.0, Color32::from_white_alpha(30)) });
    ui.add_enabled(enabled, b).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The step's buttons, bottom right: [secondary] [primary]. Returns (primary, secondary) clicked.
fn footer(ui: &mut Ui, primary: &str, ready: bool, secondary: Option<&str>) -> (bool, bool) {
    ui.add_space(22.0);
    let mut out = (false, false);
    // Only as tall as the buttons (a right-to-left layout would otherwise take all the height left).
    let w = ui.available_width();
    ui.allocate_ui_with_layout(vec2(w, 40.0), Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        out.0 = button(ui, primary, true, ready).clicked();
        if let Some(sec) = secondary {
            out.1 = button(ui, sec, false, true).clicked();
        }
    });
    out
}

fn logo(ui: &mut Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let heights = [0.34, 0.68, 1.0, 0.54, 0.21];
    let w = height * 0.19;
    let gap = height * 0.12;
    let total = w * 5.0 + gap * 4.0;
    let t = ui.input(|i| i.time) as f32;
    for (i, h) in heights.iter().enumerate() {
        // A gentle breathing bounce, each bar a little behind the last.
        let b = 1.0 + 0.06 * ((t * 2.4 - i as f32 * 0.5).sin());
        let hh = height * h * b;
        let x = rect.center().x - total / 2.0 + i as f32 * (w + gap);
        let r = Rect::from_min_size(pos2(x, rect.center().y - hh / 2.0), vec2(w, hh));
        ui.painter().rect_filled(r, CornerRadius::same((w / 2.0) as u8), TEXT);
    }
}

fn welcome_step(ui: &mut Ui, s: &mut Setup) {
    ui.add_space(10.0);
    logo(ui, 110.0);
    ui.add_space(22.0);
    title(ui, "Welcome to Pulse!");
    ui.add_space(8.0);
    subtitle(ui, "Let's get you set up. It takes about a minute.");
    let (go_on, _) = footer(ui, "Get started", true, None);
    if go_on || ui.input(|i| i.key_pressed(Key::Enter)) {
        let n = next(Step::Welcome);
        go(s, n);
    }
}

fn field(ui: &mut Ui, label: &str, value: &mut String, password: bool, id: &str) -> egui::Response {
    ui.label(RichText::new(tr(label)).font(med(13.0)).color(MUTED));
    ui.add_space(4.0);
    let r = egui::Frame::new()
        .fill(FIELD)
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(26)))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(value).id(Id::new(id)).password(password).frame(egui::Frame::NONE).text_color(TEXT).desired_width(f32::INFINITY))
        })
        .inner;
    ui.add_space(12.0);
    r
}

fn account_step(ui: &mut Ui, s: &mut Setup) {
    title(ui, if s.log_in { "Log in" } else { "Make an account" });
    ui.add_space(6.0);
    subtitle(
        ui,
        if s.log_in {
            "Welcome back. Your playlists and profile come with you."
        } else {
            "Optional: it gives you a profile, public playlists and your name in the app."
        },
    );
    ui.add_space(20.0);
    let mut enter = false;
    if !s.log_in {
        enter |= field(ui, "Email", &mut s.email, false, "setup_email").lost_focus();
    }
    enter |= field(ui, "Username", &mut s.username, false, "setup_user").lost_focus();
    enter |= field(ui, "Password", &mut s.password, true, "setup_pass").lost_focus();
    if !s.log_in {
        enter |= field(ui, "Password again", &mut s.password2, true, "setup_pass2").lost_focus();
    }
    enter &= ui.input(|i| i.key_pressed(Key::Enter));
    if let Some(e) = &s.error {
        ui.label(RichText::new(tr(e)).font(med(13.0)).color(Color32::from_rgb(252, 165, 165)));
    }
    if s.busy {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(tr(if s.log_in { "Logging in…" } else { "Making your account…" })).color(MUTED));
        });
    }
    ui.add_space(4.0);
    let switch_label = if s.log_in { "New here? Make an account" } else { "Already have an account? Log in" };
    if ui
        .add(egui::Label::new(RichText::new(tr(switch_label)).font(med(13.0)).color(LAVENDER).underline()).sense(Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        s.log_in = !s.log_in;
        s.error = None;
    }
    let primary = if s.log_in { "Log in" } else { "Create account" };
    let (submit, skip) = footer(ui, primary, !s.busy, Some("Skip"));
    if skip {
        go(s, Step::Artists);
        return;
    }
    if (submit || enter) && !s.busy {
        let (user, pass) = (s.username.trim().to_string(), s.password.clone());
        if !crate::account::valid_username(&user) {
            s.error = Some("Usernames are 3–20 letters, numbers or _.".into());
            return;
        }
        if !s.log_in {
            if !s.email.contains('@') {
                s.error = Some("Enter your email address.".into());
                return;
            }
            if s.password != s.password2 {
                s.error = Some("The passwords don't match.".into());
                return;
            }
        }
        s.busy = true;
        s.error = None;
        let (tx, rx) = channel();
        s.rx = Some(rx);
        let (log_in, email, ctx) = (s.log_in, s.email.trim().to_string(), ui.ctx().clone());
        std::thread::spawn(move || {
            let r = if log_in { crate::account::sign_in(&user, &pass) } else { crate::account::sign_up(&user, &email, &pass) };
            let _ = tx.send(r);
            ctx.request_repaint();
        });
    }
}

fn receive_account(app: &mut App, s: &mut Setup, ctx: &egui::Context) {
    let Some(rx) = &s.rx else { return };
    let Ok(r) = rx.try_recv() else { return };
    s.rx = None;
    s.busy = false;
    match r {
        Ok(session) => {
            s.name = session.username.clone();
            s.password.clear();
            s.password2.clear();
            app.account_done(crate::app::AccountDone::SignedIn(session), ctx);
            go(s, Step::Artists);
        }
        Err(e) => s.error = Some(e),
    }
}

fn artists_step(app: &mut App, ui: &mut Ui, s: &mut Setup, max_h: f32) {
    title(ui, if s.name.is_empty() { "Pick 3 artists you like" } else { "Perfect, now pick 3 artists you like" });
    ui.add_space(6.0);
    subtitle(ui, "Your home page starts from them until what you play takes over.");
    ui.add_space(18.0);
    let ctx = ui.ctx().clone();
    let p = s.picker.get_or_insert_with(|| Picker::start(&ctx));
    p.tick(&ctx);
    let now = ctx.input(|i| i.time);
    let grid_h = (max_h - 420.0).clamp(180.0, 420.0);
    pick_artists::body(p, ui, now, grid_h);
    let n = p.picked.len();
    let ready = n >= pick_artists::ENOUGH;
    let label = if ready { tr("Continue").to_string() } else { format!("{} {}", tr("Pick"), pick_artists::ENOUGH - n) };
    let (go_on, skip) = footer(ui, &label, ready, Some("Skip"));
    if go_on {
        for a in p.picked.iter().rev() {
            if !app.follows(&a.id) {
                app.saved.followed_artists.insert(0, a.clone());
            }
        }
        app.cache.remove("home");
    }
    if go_on || skip {
        s.picker = None;
        go(s, Step::Settings);
    }
}

/// One setting: title and a line under it on the left, the switch on the right.
fn setting(ui: &mut Ui, on: &mut bool, name: &str, body: &str, indent: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width() - 70.0);
            ui.label(RichText::new(tr(name)).font(semi(if indent > 0.0 { 14.0 } else { 16.0 })).color(TEXT));
            ui.add(egui::Label::new(RichText::new(tr(body)).font(regular(12.5)).color(MUTED)).wrap());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            changed = crate::widgets::toggle_switch(ui, on, name, VIOLET, Color32::from_white_alpha(40)).changed();
        });
    });
    ui.add_space(16.0);
    changed
}

fn settings_step(app: &mut App, ui: &mut Ui, s: &mut Setup) {
    use crate::perf::Mode;
    title(ui, "Now choose your settings");
    ui.add_space(6.0);
    subtitle(ui, "You can change any of these later in Settings.");
    ui.add_space(24.0);
    let before = app.saved.settings.clone();
    let mut gaming = before.perf_mode != Mode::Normal;
    let mut ultra = before.perf_mode == Mode::UltraLow;
    let mut discord = before.discord_status;
    let mut explicit = before.explicit;
    let mut changed = false;
    changed |= setting(ui, &mut gaming, "Gaming Mode", "Pauses Pulse's extras while you play. Your music stays the same.", 0.0);
    if gaming {
        changed |= setting(ui, &mut ultra, "Ultra-Gaming Mode", "The least Pulse can use: fewer pictures and redraws on top of Gaming Mode.", 22.0);
    }
    changed |= setting(ui, &mut discord, "Discord status", "Shows the song you're playing on your Discord profile.", 0.0);
    changed |= setting(ui, &mut explicit, "Explicit songs", "Plays the uncensored version of a song when there is one.", 0.0);
    if changed {
        let st = &mut app.saved.settings;
        st.perf_mode = match (gaming, ultra) {
            (false, _) => Mode::Normal,
            (true, false) => Mode::Gaming,
            (true, true) => Mode::UltraLow,
        };
        st.discord_status = discord;
        st.explicit = explicit;
        app.apply_settings(&before, ui.ctx());
        app.toast = None; // the switch itself shows the change
    }
    let (go_on, _) = footer(ui, "Continue", true, None);
    if go_on {
        go(s, Step::Theme);
    }
}

fn theme_step(app: &mut App, ui: &mut Ui, s: &mut Setup) {
    title(ui, "Lastly, pick your theme");
    ui.add_space(6.0);
    subtitle(ui, "It changes as you click, so try a few.");
    ui.add_space(20.0);
    let current = app.current_theme().map(|t| t.name.clone()).unwrap_or_default();
    let supporter = crate::account::session().is_some_and(|x| x.supporter);
    let mut pick = None;
    let card_w = ((ui.available_width() - 2.0 * 14.0) / 3.0).floor();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
        for t in app.themes.iter().filter(|t| t.builtin && crate::themes::offered(t, supporter, false, "")) {
            let selected = t.name == current;
            let (rect, resp) = ui.allocate_exact_size(vec2(card_w, card_w * 0.62 + 26.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, &t.name));
            let pic = Rect::from_min_size(rect.min, vec2(card_w, card_w * 0.62));
            super::settings_page::swatch(ui, pic, &t.schemes[0].palette);
            let border = if selected {
                Stroke::new(2.5, LAVENDER)
            } else if resp.hovered() {
                Stroke::new(1.0, Color32::from_white_alpha(90))
            } else {
                Stroke::new(1.0, Color32::from_white_alpha(30))
            };
            ui.painter().rect_stroke(pic, CornerRadius::same(12), border, egui::StrokeKind::Outside);
            let label = if t.name == crate::themes::SUPPORTERS_THEME { format!("{} – {}", t.name, tr("Early Support")) } else { t.name.clone() };
            crate::widgets::paint_line(ui, pos2(rect.left() + 2.0, pic.bottom() + 7.0), &label, semi(13.0), if selected { LAVENDER } else { TEXT }, card_w);
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !selected {
                pick = Some(t.name.clone());
            }
        }
    });
    if let Some(name) = pick {
        let before = app.saved.settings.clone();
        app.saved.settings.theme = name;
        app.saved.settings.scheme.clear();
        app.apply_settings(&before, ui.ctx());
        app.toast = None;
    }
    let (go_on, _) = footer(ui, "Finish", true, None);
    if go_on {
        go(s, Step::Done);
    }
}

/// "Perfect, welcome in": a moment on its own, then the intro takes over. True when done.
fn done_step(ui: &mut Ui, screen: Rect, s: &Setup, now: f64) -> bool {
    let t = now - s.since;
    let text = if s.name.is_empty() { tr("Perfect, welcome in").to_string() } else { format!("{}, @{}", tr("Perfect, welcome in"), s.name) };
    let a = (t / 0.4).clamp(0.0, 1.0) as f32 * (1.0 - ((t - 1.3) / 0.3).clamp(0.0, 1.0) as f32);
    ui.painter().text(screen.center(), egui::Align2::CENTER_CENTER, text, heavy(30.0), TEXT.gamma_multiply(a));
    t > 1.6
}
