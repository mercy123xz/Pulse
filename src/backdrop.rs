//! The moving background behind the main area: a few soft glows tinted with the
//! colors of the song that's playing, drifting slowly.
//!
//! Kept light: the cover's color is worked out once per song on a background thread
//! from its small picture, the glows are a handful of triangles, and the screen is
//! only redrawn for the motion (5 times a second: the glows move a few pixels a
//! second, so more would be wasted) while music plays and the window is in front.
//! With "Reduce motion" on it stands still.

use crate::theme::pal;
use egui::{Color32, CornerRadius, Pos2, Rect, Ui, vec2};
use std::sync::mpsc::{Receiver, channel};

#[derive(Default)]
pub struct Backdrop {
    /// The cover we last asked about.
    cover: Option<String>,
    /// Its color, once known.
    target: Option<Color32>,
    /// What's drawn now: moves toward `target` so a new song blends in.
    shown: Option<[f32; 3]>,
    rx: Option<Receiver<(String, Option<Color32>)>>,
    last_time: f64,
}

impl Backdrop {
    /// Starts working out a new cover's color when the song changes.
    pub fn set_cover(&mut self, cover: Option<&str>, ctx: &egui::Context) {
        if self.cover.as_deref() == cover {
            return;
        }
        self.cover = cover.map(str::to_owned);
        let Some(url) = cover.map(str::to_owned) else {
            self.target = None;
            return;
        };
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let color = read_picture(&url).and_then(|b| dominant(&b));
            let _ = tx.send((url, color));
            ctx.request_repaint();
        });
    }

    /// Paints the glows over `rect`. `outside` is the color around a rounded panel (its
    /// corners are painted back over), `moving` says whether to animate.
    pub fn paint(&mut self, ui: &Ui, rect: Rect, radius: CornerRadius, outside: Option<Color32>, moving: bool) {
        if let Some(rx) = &self.rx
            && let Ok((url, c)) = rx.try_recv()
        {
            if self.cover.as_deref() == Some(url.as_str()) {
                self.target = c;
            }
            self.rx = None;
        }
        let now = ui.input(|i| i.time);
        let dt = (now - self.last_time).clamp(0.0, 0.5) as f32;
        self.last_time = now;

        // Blend toward the new song's color over about a second and a half.
        let goal = self.target.unwrap_or(pal().accent);
        let goal = [goal.r() as f32, goal.g() as f32, goal.b() as f32];
        let shown = self.shown.get_or_insert(goal);
        let k = 1.0 - (-dt * 2.2).exp();
        let mut settling = false;
        for i in 0..3 {
            shown[i] += (goal[i] - shown[i]) * k;
            settling |= (goal[i] - shown[i]).abs() > 1.0;
        }
        let base = Color32::from_rgb(shown[0] as u8, shown[1] as u8, shown[2] as u8);

        let light = crate::theme::is_light(pal());
        let strength = if light { 0.20 } else { 0.30 };
        // Slow drift: each glow circles on its own path, once every minute or so.
        let t = if moving { now as f32 } else { 0.0 };
        let (w, h) = (rect.width(), rect.height());
        let size = w.max(h);
        let glows = [
            (base, vec2(-0.28, -0.30), (0.11, 0.07), 0.75),
            (shift_hue(base, 35.0), vec2(0.30, -0.05), (0.08, 0.10), 0.62),
            (pal().accent, vec2(0.0, 0.38), (0.06, 0.09), 0.55),
        ];
        let painter = ui.painter().with_clip_rect(rect);
        for (i, (color, home, (sx, sy), r)) in glows.into_iter().enumerate() {
            let phase = i as f32 * 2.1;
            let c = rect.center() + vec2((home.x + 0.12 * (t * sx + phase).sin()) * w, (home.y + 0.10 * (t * sy + phase).cos()) * h);
            glow(&painter, c, size * r, color.gamma_multiply(strength));
        }
        if let Some(out) = outside {
            mask_corners(&painter, rect, radius, out);
        }
        if moving && ui.input(|i| i.focused) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        } else if settling {
            ui.ctx().request_repaint();
        }
    }
}

/// A round glow: full color in the middle, fading to nothing at `radius`.
fn glow(p: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    const N: usize = 40;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, color);
    // Two rings: the middle one keeps a little color so the fade looks soft, not conical.
    for (f, a) in [(0.45, 0.55), (1.0, 0.0)] {
        for k in 0..N {
            let ang = k as f32 / N as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(center + vec2(ang.cos(), ang.sin()) * radius * f, color.gamma_multiply(a));
        }
    }
    let n = N as u32;
    for k in 0..n {
        let k2 = (k + 1) % n;
        mesh.add_triangle(0, 1 + k, 1 + k2);
        let (a0, a1, b0, b1) = (1 + k, 1 + k2, 1 + n + k, 1 + n + k2);
        mesh.add_triangle(a0, b0, b1);
        mesh.add_triangle(a0, b1, a1);
    }
    p.add(egui::Shape::mesh(mesh));
}

/// Paints the area outside each rounded corner, so glows don't show square corners.
fn mask_corners(p: &egui::Painter, rect: Rect, radius: CornerRadius, color: Color32) {
    let corners = [
        (rect.left_top(), radius.nw as f32, vec2(1.0, 1.0)),
        (rect.right_top(), radius.ne as f32, vec2(-1.0, 1.0)),
        (rect.left_bottom(), radius.sw as f32, vec2(1.0, -1.0)),
        (rect.right_bottom(), radius.se as f32, vec2(-1.0, -1.0)),
    ];
    for (corner, r, dir) in corners {
        if r < 1.0 {
            continue;
        }
        let center = corner + vec2(dir.x * r, dir.y * r);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(corner, color);
        const N: usize = 10;
        for k in 0..=N {
            // From the side along x to the side along y.
            let a = k as f32 / N as f32 * std::f32::consts::FRAC_PI_2;
            let p = center - vec2(dir.x * a.cos() * r, dir.y * a.sin() * r);
            mesh.colored_vertex(p, color);
        }
        for k in 0..N as u32 {
            mesh.add_triangle(0, 1 + k, 2 + k);
        }
        p.add(egui::Shape::mesh(mesh));
    }
}

fn shift_hue(c: Color32, degrees: f32) -> Color32 {
    let mut hsva = egui::ecolor::Hsva::from(c);
    hsva.h = (hsva.h + degrees / 360.0).rem_euclid(1.0);
    hsva.into()
}

fn read_picture(url: &str) -> Option<Vec<u8>> {
    if let Some(path) = url.strip_prefix("file:///").or_else(|| url.strip_prefix("file://")) {
        return std::fs::read(path).ok();
    }
    if url.starts_with("http") {
        return crate::net::download(url, |_, _| {}).ok();
    }
    None
}

/// The picture's main color: colorful, mid-bright pixels count most, so a cover that's
/// mostly black with a red title comes out red rather than grey.
pub fn dominant(bytes: &[u8]) -> Option<Color32> {
    let img = image::load_from_memory(bytes).ok()?.thumbnail(24, 24).to_rgb8();
    let (mut r, mut g, mut b, mut weight) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let (mut ar, mut ag, mut ab, mut n) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for p in img.pixels() {
        let [pr, pg, pb] = p.0.map(|v| v as f32 / 255.0);
        let max = pr.max(pg).max(pb);
        let min = pr.min(pg).min(pb);
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        // Favour color; skip near-black and near-white.
        let w = sat * sat * (1.0 - (max - 0.6).abs()).max(0.0);
        r += pr * w;
        g += pg * w;
        b += pb * w;
        weight += w;
        ar += pr;
        ag += pg;
        ab += pb;
        n += 1.0;
    }
    if n == 0.0 {
        return None;
    }
    let (r, g, b) = if weight > n * 0.02 { (r / weight, g / weight, b / weight) } else { (ar / n, ag / n, ab / n) };
    // Same color, at a brightness that tints without glaring.
    let max = r.max(g).max(b).max(0.05);
    let scale = 0.85 / max;
    let c = |v: f32| ((v * scale).clamp(0.0, 1.0) * 255.0) as u8;
    Some(Color32::from_rgb(c(r), c(g), c(b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(pixels: &[[u8; 3]], w: u32, h: u32) -> Vec<u8> {
        let mut img = image::RgbImage::new(w, h);
        for (i, p) in img.pixels_mut().enumerate() {
            p.0 = pixels[i % pixels.len()];
        }
        let mut out = std::io::Cursor::new(vec![]);
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_dark_cover_with_a_red_title_comes_out_red() {
        // 90% black, 10% red.
        let mut px = vec![[5, 5, 5]; 9];
        px.push([220, 30, 30]);
        let c = dominant(&png(&px, 20, 20)).unwrap();
        assert!(c.r() > 150 && c.g() < 80 && c.b() < 80, "{c:?}");
    }

    #[test]
    fn a_grey_cover_stays_grey() {
        let c = dominant(&png(&[[120, 120, 120]], 8, 8)).unwrap();
        assert!(c.r().abs_diff(c.g()) < 5 && c.g().abs_diff(c.b()) < 5, "{c:?}");
    }
}
