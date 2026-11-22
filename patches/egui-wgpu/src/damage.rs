//! PULSE PATCH: redraw only what changed (software drawing, where every pixel costs CPU).
//!
//! The picture of the last frame is kept in a texture of our own. A new frame is compared
//! with the last one triangle by triangle. When the page was only scrolled, the old pixels are
//! shifted and only the strip that scrolled into view, and whatever else changed (a hover
//! highlight, a progress bar), is drawn again. A region that is drawn again is cleared and
//! redrawn with every triangle that touches it, in order, so it comes out the same as in a full
//! frame. Anything that can't be compared safely falls back to drawing the whole frame.

use crate::{Renderer, ScreenDescriptor};
use epaint::{ClippedPrimitive, Primitive, TextureId};

/// Damage is tracked in tiles of this many pixels.
const TILE: i32 = 16;

/// More than this share of the window changed: just draw the whole frame.
const MAX_SHARE: f64 = 0.45;

/// The scrolled picture is copied by whole pixels, so it has to have moved by whole pixels (Pulse
/// lays the scroll area out that way). If it moved by more than this off the pixel grid, draw in full.
const MAX_OFF_GRID: f32 = 1.0 / 16.0;

#[derive(Clone, Copy)]
struct Tri {
    /// Hash of everything but the position: texture, uv, colours, shape.
    inv: u64,
    /// `inv` and the position of the first corner.
    key: u64,
    ax: i32,
    /// Part of the window it can draw to (its bounds inside its clip rectangle).
    vis: [i32; 4],
    /// Corners in pixels.
    v: [[f32; 2]; 3],
    /// Clip rectangle lies inside the scrolling region: moves with the scrolling.
    scroll: bool,
    /// One colour all over (no gradient, no texture detail).
    flat: bool,
    /// Premultiplied colour of a flat triangle.
    color: [u8; 4],
}

/// What a frame draws, compact enough to keep and compare.
pub struct FrameDesc {
    size: [u32; 2],
    ppp: u32,
    clear: [u32; 4],
    /// The scrolling region in pixels, if the app said where it is.
    region: Option<[i32; 4]>,
    tris: Vec<Tri>,
    by_inv: Vec<(u64, u32)>,
    by_key: Vec<(u64, u32)>,
    /// Per clipped primitive: the part of the window its triangles can draw to.
    pub job_bounds: Vec<[i32; 4]>,
    textures: Vec<TextureId>,
}

#[derive(Debug)]
pub enum Plan {
    /// Looks exactly like what is already there.
    Skip,
    /// Nothing to draw, show the picture we have again.
    Present,
    /// Draw everything (the text says why).
    Full(&'static str),
    /// Move the scrolled part up by `-shift` pixels (down if positive), draw the `rects`.
    Partial { shift: i32, rects: Vec<[u32; 4]> },
}

fn mix(h: u64, v: u64) -> u64 {
    (h.rotate_left(23) ^ v).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn fin(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

fn tex_bits(id: TextureId) -> u64 {
    match id {
        TextureId::Managed(i) => i,
        TextureId::User(i) => i | (1 << 63),
    }
}

fn inter(a: [i32; 4], b: [i32; 4]) -> [i32; 4] {
    [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]
}

fn empty(r: [i32; 4]) -> bool {
    r[2] <= r[0] || r[3] <= r[1]
}

impl FrameDesc {
    /// `None` if the frame has something that can't be compared (paint callbacks).
    pub fn describe(
        size: [u32; 2],
        ppp: f32,
        clear: [f32; 4],
        region_points: Option<[f32; 4]>,
        jobs: &[ClippedPrimitive],
    ) -> Option<Self> {
        let screen = [0, 0, size[0] as i32, size[1] as i32];
        let region = region_points.map(|r| {
            let px = |v: f32| (v * ppp).round() as i32;
            inter([px(r[0]), px(r[1]), px(r[2]), px(r[3])], screen)
        });
        let mut tris: Vec<Tri> = Vec::new();
        let mut job_bounds = Vec::with_capacity(jobs.len());
        let mut textures = Vec::new();
        for job in jobs {
            let Primitive::Mesh(m) = &job.primitive else { return None };
            // The same rounding the renderer uses for its scissor rectangle.
            let c = job.clip_rect;
            let clip = inter(
                [(c.min.x * ppp).round() as i32, (c.min.y * ppp).round() as i32, (c.max.x * ppp).round() as i32, (c.max.y * ppp).round() as i32],
                screen,
            );
            let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
            if !empty(clip) {
                let scroll = region.is_some_and(|r| clip[0] >= r[0] - 1 && clip[1] >= r[1] - 1 && clip[2] <= r[2] + 1 && clip[3] <= r[3] + 1);
                let tex = tex_bits(m.texture_id);
                let mut used = false;
                for t in m.indices.chunks_exact(3) {
                    let vs = [m.vertices[t[0] as usize], m.vertices[t[1] as usize], m.vertices[t[2] as usize]];
                    let p = vs.map(|v| [v.pos.x * ppp, v.pos.y * ppp]);
                    let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
                    if area.abs() < 1e-4 {
                        continue;
                    }
                    let lo = [p[0][0].min(p[1][0]).min(p[2][0]), p[0][1].min(p[1][1]).min(p[2][1])];
                    let hi = [p[0][0].max(p[1][0]).max(p[2][0]), p[0][1].max(p[1][1]).max(p[2][1])];
                    let vis = inter([lo[0].floor() as i32, lo[1].floor() as i32, hi[0].ceil() as i32, hi[1].ceil() as i32], clip);
                    if empty(vis) {
                        continue;
                    }
                    let q = |v: f32| (v * 16.0).round() as i32 as u32 as u64;
                    let mut h = mix(mix(0, tex), scroll as u64);
                    for v in &vs {
                        h = mix(h, (v.uv.x.to_bits() as u64) << 32 | v.uv.y.to_bits() as u64);
                        h = mix(h, u32::from_le_bytes(v.color.to_array()) as u64);
                    }
                    h = mix(h, q(p[1][0] - p[0][0]) << 32 | q(p[1][1] - p[0][1]));
                    h = mix(h, q(p[2][0] - p[0][0]) << 32 | q(p[2][1] - p[0][1]));
                    let inv = fin(h);
                    let ax = (p[0][0] * 16.0).round() as i32;
                    let ay = (p[0][1] * 16.0).round() as i32;
                    let flat = vs[0].color == vs[1].color && vs[0].color == vs[2].color && vs[0].uv == vs[1].uv && vs[0].uv == vs[2].uv;
                    tris.push(Tri {
                        inv,
                        key: key_of(inv, ax, ay),
                        ax,
                        vis,
                        v: p,
                        scroll,
                        flat,
                        color: vs[0].color.to_array(),
                    });
                    bounds = [bounds[0].min(vis[0]), bounds[1].min(vis[1]), bounds[2].max(vis[2]), bounds[3].max(vis[3])];
                    used = true;
                }
                if used && !textures.contains(&m.texture_id) {
                    textures.push(m.texture_id);
                }
            }
            job_bounds.push(if bounds[0] == i32::MAX { [0, 0, 0, 0] } else { bounds });
        }
        let mut by_inv: Vec<(u64, u32)> = tris.iter().enumerate().map(|(i, t)| (t.inv, i as u32)).collect();
        by_inv.sort_unstable();
        let mut by_key: Vec<(u64, u32)> = tris.iter().enumerate().map(|(i, t)| (t.key, i as u32)).collect();
        by_key.sort_unstable();
        Some(Self { size, ppp: ppp.to_bits(), clear: clear.map(f32::to_bits), region, tris, by_inv, by_key, job_bounds, textures })
    }

    pub fn triangles(&self) -> usize {
        self.tris.len()
    }
}

fn key_of(inv: u64, ax: i32, ay: i32) -> u64 {
    fin(mix(mix(inv, ax as u32 as u64), ay as u32 as u64))
}

/// Which textures this frame's textures delta touches in a way that changes triangles already
/// drawn (everything but new texels added to the font texture, and brand new textures).
pub fn textures_changed(prev: &FrameDesc, delta: &epaint::textures::TexturesDelta) -> bool {
    delta.set.iter().any(|(id, deltas)| {
        prev.textures.contains(id) && (*id != TextureId::Managed(0) || deltas.iter().any(|d| d.pos.is_none()))
    })
}

struct Tiles {
    w: i32,
    h: i32,
    bits: Vec<bool>,
    size: [i32; 2],
}

impl Tiles {
    fn new(size: [u32; 2]) -> Self {
        let (w, h) = ((size[0] as i32 + TILE - 1) / TILE, (size[1] as i32 + TILE - 1) / TILE);
        Self { w, h, bits: vec![false; (w * h) as usize], size: [size[0] as i32, size[1] as i32] }
    }

    fn mark(&mut self, r: [i32; 4]) {
        let r = inter(r, [0, 0, self.size[0], self.size[1]]);
        if empty(r) {
            return;
        }
        for ty in r[1] / TILE..=(r[3] - 1) / TILE {
            for tx in r[0] / TILE..=(r[2] - 1) / TILE {
                self.bits[(ty * self.w + tx) as usize] = true;
            }
        }
    }

    fn count(&self) -> usize {
        self.bits.iter().filter(|b| **b).count()
    }

    /// Rectangles covering exactly the marked tiles, none overlapping.
    fn rects(&self) -> Vec<[u32; 4]> {
        let mut out: Vec<[u32; 4]> = Vec::new();
        // Runs of the row above that can still grow downwards: (tile x0, tile x1, tile y0).
        let mut open: Vec<(i32, i32, i32)> = Vec::new();
        for ty in 0..=self.h {
            let mut runs = Vec::new();
            if ty < self.h {
                let mut tx = 0;
                while tx < self.w {
                    if self.bits[(ty * self.w + tx) as usize] {
                        let x0 = tx;
                        while tx < self.w && self.bits[(ty * self.w + tx) as usize] {
                            tx += 1;
                        }
                        runs.push((x0, tx));
                    } else {
                        tx += 1;
                    }
                }
            }
            let mut next = Vec::new();
            for (x0, x1, y0) in open.drain(..) {
                if runs.contains(&(x0, x1)) {
                    next.push((x0, x1, y0));
                } else {
                    out.push(self.px(x0, y0, x1, ty));
                }
            }
            for (x0, x1) in runs {
                if !next.iter().any(|(a, b, _)| *a == x0 && *b == x1) {
                    next.push((x0, x1, ty));
                }
            }
            open = next;
        }
        out
    }

    fn px(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> [u32; 4] {
        let (a, b) = ((x0 * TILE).min(self.size[0]), (y0 * TILE).min(self.size[1]));
        let (c, d) = ((x1 * TILE).min(self.size[0]), (y1 * TILE).min(self.size[1]));
        [a as u32, b as u32, (c - a) as u32, (d - b) as u32]
    }
}

/// How far the scrolled part moved between two frames (new minus old position), found by the
/// triangles that look the same in both: the whole pixels it is copied by, and the exact distance.
/// `(0, 0.0)` if there is no clear answer.
fn find_shift(prev: &FrameDesc, new: &FrameDesc) -> (i32, f32) {
    // Per whole-pixel distance: (votes, sum of the exact distances).
    let mut votes: Vec<(i32, u32, f32)> = Vec::new();
    let step = (new.tris.len() / 600).max(1);
    for t in new.tris.iter().step_by(step).filter(|t| t.scroll) {
        let lo = prev.by_inv.partition_point(|e| e.0 < t.inv);
        let hi = prev.by_inv[lo..].partition_point(|e| e.0 == t.inv) + lo;
        // Only triangles that are rare enough to say something.
        if hi == lo || hi - lo > 6 {
            continue;
        }
        for e in &prev.by_inv[lo..hi] {
            let p = &prev.tris[e.1 as usize];
            let (dx, dy) = (t.v[0][0] - p.v[0][0], t.v[0][1] - p.v[0][1]);
            if dx.abs() > 0.1 {
                continue;
            }
            let k = dy.round() as i32;
            match votes.iter_mut().find(|v| v.0 == k) {
                Some(v) => {
                    v.1 += 1;
                    v.2 += dy;
                }
                None => votes.push((k, 1, dy)),
            }
        }
    }
    // Static things vote for 0: what scrolls has to beat them, so a page that only had a hover
    // change reads as not moving.
    let still = votes.iter().find(|v| v.0 == 0).map_or(0, |v| v.1);
    match votes.iter().filter(|v| v.0 != 0).max_by_key(|v| v.1) {
        Some(b) if b.1 >= 4 && b.1 > still => (b.0, b.2 / b.1 as f32),
        _ => (0, 0.0),
    }
}

/// Works out how to draw `new` given `prev`, what is on screen.
pub fn plan(prev: &FrameDesc, new: &FrameDesc, textures_changed: bool, keepalive_due: bool) -> Plan {
    if prev.size != new.size || prev.ppp != new.ppp || prev.clear != new.clear {
        return Plan::Full("window or colour changed");
    }
    if prev.region != new.region {
        return Plan::Full("scroll region changed");
    }
    if textures_changed {
        return Plan::Full("a picture already on screen was replaced");
    }
    let (shift, exact) = if new.region.is_some() { find_shift(prev, new) } else { (0, 0.0) };
    let err = if shift != 0 { exact - shift as f32 } else { 0.0 };
    if err.abs() > MAX_OFF_GRID {
        return Plan::Full("scroll not on whole pixels");
    }
    let mut tiles = Tiles::new(new.size);
    let mut matched = vec![false; prev.tris.len()];
    for t in &new.tris {
        // Where the same triangle would have been in the old frame.
        let key = if t.scroll && shift != 0 { key_of(t.inv, t.ax, ((t.v[0][1] - exact) * 16.0).round() as i32) } else { t.key };
        let mut lo = prev.by_key.partition_point(|e| e.0 < key);
        let mut found = false;
        while lo < prev.by_key.len() && prev.by_key[lo].0 == key {
            let i = prev.by_key[lo].1 as usize;
            if !matched[i] {
                matched[i] = true;
                found = true;
                break;
            }
            lo += 1;
        }
        if !found {
            tiles.mark(t.vis);
        }
    }
    let region = new.region;
    for (i, p) in prev.tris.iter().enumerate() {
        if matched[i] {
            continue;
        }
        let mut r = p.vis;
        if p.scroll && shift != 0 {
            r = [r[0], r[1] + shift, r[2], r[3] + shift];
            if let Some(reg) = region {
                r = inter(r, reg);
            }
        }
        tiles.mark(r);
        // A thing that stays put but sits in the scrolling region was copied along with the rest:
        // its old pixels now sit `shift` further on.
        if shift != 0 && !p.scroll {
            if let Some(reg) = region {
                tiles.mark(inter([p.vis[0], p.vis[1] + shift, p.vis[2], p.vis[3] + shift], reg));
            }
        }
    }
    if shift != 0 {
        let Some(reg) = region else { return Plan::Full("no region") };
        // Rows that scrolled into view have nothing to copy from.
        if shift < 0 {
            tiles.mark([reg[0], reg[3] + shift, reg[2], reg[3]]);
        } else {
            tiles.mark([reg[0], reg[1], reg[2], reg[1] + shift]);
        }
        // Things that stay put but sit under the scrolling part are only safe to copy past if
        // they look the same everywhere they are (one colour, covering the whole region).
        let mut groups: Vec<([u8; 4], Vec<&Tri>)> = Vec::new();
        for t in new.tris.iter().filter(|t| !t.scroll && !empty(inter(t.vis, reg))) {
            let b = [
                t.v.iter().map(|p| p[0]).fold(f32::MAX, f32::min).floor() as i32,
                t.v.iter().map(|p| p[1]).fold(f32::MAX, f32::min).floor() as i32,
                t.v.iter().map(|p| p[0]).fold(f32::MIN, f32::max).ceil() as i32,
                t.v.iter().map(|p| p[1]).fold(f32::MIN, f32::max).ceil() as i32,
            ];
            let wide = b[0] <= reg[0] && b[1] <= reg[1] && b[2] >= reg[2] && b[3] >= reg[3];
            if t.flat && wide {
                match groups.iter_mut().find(|g| g.0 == t.color) {
                    Some(g) => g.1.push(t),
                    None => groups.push((t.color, vec![t])),
                }
            } else {
                tiles.mark(inter(t.vis, reg));
                tiles.mark(inter([t.vis[0], t.vis[1] + shift, t.vis[2], t.vis[3] + shift], reg));
            }
        }
        for (_, g) in &groups {
            if !covers(g, reg, 0) {
                return Plan::Full("something static under the scrolling part isn't plain");
            }
        }
    }
    let dirty = tiles.count();
    if dirty == 0 {
        return if shift == 0 && !keepalive_due { Plan::Skip } else if shift == 0 { Plan::Present } else { Plan::Partial { shift, rects: Vec::new() } };
    }
    let total = (tiles.w * tiles.h) as f64;
    if dirty as f64 > MAX_SHARE * total {
        return Plan::Full("a lot changed");
    }
    Plan::Partial { shift, rects: tiles.rects() }
}

/// Do these triangles together cover the whole region (and a margin of `pad` rows each side)?
fn covers(tris: &[&Tri], reg: [i32; 4], pad: i32) -> bool {
    let (x0, x1) = (reg[0] as f32 + 0.5, reg[2] as f32 - 0.5);
    let (y0, y1) = ((reg[1] - pad) as f32 + 0.5, (reg[3] + pad) as f32 - 0.5);
    for iy in 0..=6 {
        for ix in 0..=6 {
            let p = [x0 + (x1 - x0) * ix as f32 / 6.0, y0 + (y1 - y0) * iy as f32 / 6.0];
            if !tris.iter().any(|t| inside(t, p)) {
                return false;
            }
        }
    }
    true
}

fn inside(t: &Tri, p: [f32; 2]) -> bool {
    let s = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let (d0, d1, d2) = (s(t.v[0], t.v[1]), s(t.v[1], t.v[2]), s(t.v[2], t.v[0]));
    let eps = 1e-3;
    (d0 >= -eps && d1 >= -eps && d2 >= -eps) || (d0 <= eps && d1 <= eps && d2 <= eps)
}

/// The picture of the last frame, kept between frames.
pub struct Canvas {
    textures: [wgpu::Texture; 2],
    cur: usize,
    /// The clear colour, size of the window.
    bg: wgpu::Texture,
    bg_clear: Option<[u32; 4]>,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    prev: Option<FrameDesc>,
    last_present: web_time::Instant,
}

/// How the last frame was drawn, for the log.
#[derive(Default, Clone, Copy, Debug)]
pub struct DamageStats {
    pub skipped: u32,
    pub presented: u32,
    pub shifted: u32,
    pub patched: u32,
    pub full: u32,
    /// Pixels drawn (areas of the redrawn rectangles, whole window for a full frame).
    pub drawn_px: u64,
}

fn make(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat, label: &'static str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn copy(encoder: &mut wgpu::CommandEncoder, src: &wgpu::Texture, from: [u32; 2], dst: &wgpu::Texture, to: [u32; 2], size: [u32; 2]) {
    if size[0] == 0 || size[1] == 0 {
        return;
    }
    fn info(t: &wgpu::Texture, o: [u32; 2]) -> wgpu::TexelCopyTextureInfo<'_> {
        wgpu::TexelCopyTextureInfo {
            texture: t,
            mip_level: 0,
            origin: wgpu::Origin3d { x: o[0], y: o[1], z: 0 },
            aspect: wgpu::TextureAspect::All,
        }
    }
    encoder.copy_texture_to_texture(info(src, from), info(dst, to), wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 });
}

impl Canvas {
    pub fn new(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> Self {
        Self {
            textures: [make(device, size, format, "pulse canvas a"), make(device, size, format, "pulse canvas b")],
            cur: 0,
            bg: make(device, size, format, "pulse canvas background"),
            bg_clear: None,
            size,
            format,
            prev: None,
            last_present: web_time::Instant::now(),
        }
    }

    pub fn fits(&self, size: [u32; 2], format: wgpu::TextureFormat) -> bool {
        self.size == size && self.format == format
    }

    /// The frame looks the same as the picture: only remember what it describes.
    pub fn skip(&mut self, desc: FrameDesc) {
        self.prev = Some(desc);
    }

    /// Forgets the picture (the next frame is drawn in full).
    pub fn invalidate(&mut self) {
        self.prev = None;
    }

    /// How to draw `new`; `None` means the frame must be drawn in full.
    pub fn plan(&self, new: &FrameDesc, delta: &epaint::textures::TexturesDelta, keepalive: core::time::Duration) -> Plan {
        match &self.prev {
            None => Plan::Full("first frame"),
            Some(prev) => plan(prev, new, textures_changed(prev, delta), self.last_present.elapsed() >= keepalive),
        }
    }

    /// Draws the frame as `plan` says into the kept picture and copies it to `target`.
    /// The buffers of `renderer` must already hold `jobs` (`update_buffers`).
    #[expect(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        renderer: &Renderer,
        jobs: &[ClippedPrimitive],
        desc: FrameDesc,
        plan: &Plan,
        clear_color: [f32; 4],
        screen: &ScreenDescriptor,
        target: &wgpu::Texture,
        stats: &mut DamageStats,
    ) {
        let [w, h] = self.size;
        let pass = |encoder: &mut wgpu::CommandEncoder, tex: &wgpu::Texture, load: wgpu::LoadOp<wgpu::Color>, draw: &mut dyn FnMut(&mut wgpu::RenderPass<'static>)| {
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pulse_render"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            draw(&mut rp.forget_lifetime());
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color {
            r: clear_color[0] as f64,
            g: clear_color[1] as f64,
            b: clear_color[2] as f64,
            a: clear_color[3] as f64,
        });
        match plan {
            Plan::Skip => {
                stats.skipped += 1;
                self.prev = Some(desc);
                return;
            }
            Plan::Present => {
                stats.presented += 1;
            }
            Plan::Full(_) => {
                stats.full += 1;
                stats.drawn_px += w as u64 * h as u64;
                pass(encoder, &self.textures[self.cur], clear, &mut |rp| renderer.render(rp, jobs, screen));
                let bits = clear_color.map(f32::to_bits);
                if self.bg_clear != Some(bits) {
                    pass(encoder, &self.bg, clear, &mut |_| {});
                    self.bg_clear = Some(bits);
                }
            }
            Plan::Partial { shift, rects } => {
                let dst = if *shift != 0 { 1 - self.cur } else { self.cur };
                if *shift != 0 {
                    stats.shifted += 1;
                    let reg = desc.region.unwrap_or([0, 0, w as i32, h as i32]);
                    copy(encoder, &self.textures[self.cur], [0, 0], &self.textures[dst], [0, 0], [w, h]);
                    let (rx, rw) = (reg[0] as u32, (reg[2] - reg[0]) as u32);
                    let rows = (reg[3] - reg[1] - shift.abs()) as u32;
                    if *shift < 0 {
                        copy(encoder, &self.textures[self.cur], [rx, (reg[1] - shift) as u32], &self.textures[dst], [rx, reg[1] as u32], [rw, rows]);
                    } else {
                        copy(encoder, &self.textures[self.cur], [rx, reg[1] as u32], &self.textures[dst], [rx, (reg[1] + shift) as u32], [rw, rows]);
                    }
                } else {
                    stats.patched += 1;
                }
                if !rects.is_empty() {
                    for r in rects {
                        copy(encoder, &self.bg, [r[0], r[1]], &self.textures[dst], [r[0], r[1]], [r[2], r[3]]);
                        stats.drawn_px += r[2] as u64 * r[3] as u64;
                    }
                    let bounds = &desc.job_bounds;
                    pass(encoder, &self.textures[dst], wgpu::LoadOp::Load, &mut |rp| {
                        for r in rects {
                            renderer.render_damage(rp, jobs, bounds, screen, *r);
                        }
                    });
                }
                self.cur = dst;
            }
        }
        copy(encoder, &self.textures[self.cur], [0, 0], target, [0, 0], [w, h]);
        self.last_present = web_time::Instant::now();
        self.prev = Some(desc);
    }

    /// The kept picture (for tests).
    pub fn picture(&self) -> &wgpu::Texture {
        &self.textures[self.cur]
    }
}
