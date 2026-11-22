//! Gaming Mode and Ultra-Low-Resource Mode: one setting with three levels, where each
//! level switches off a set of extras (and each extra can be kept on by hand).
//!
//! Nothing here touches the audio: buffering, decoding, gapless and crossfade work the
//! same in every mode. What changes is what Pulse does on the side while it plays.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Normal,
    /// For playing games: no extras that use the CPU or network while you play.
    Gaming,
    /// The least Pulse can use: Gaming Mode, plus fewer pictures and redraws.
    UltraLow,
}

/// The extras a mode can switch off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extra {
    /// Moving backgrounds, menu fades and hover effects.
    Animations,
    /// Loading pages ahead when you point at them.
    Preloading,
    /// Looking up YouTube view counts and featured artists in the background.
    Lookups,
    /// Redrawing the progress bar four times a second (otherwise once a second).
    LiveProgress,
    /// Cover pictures in song lists, and the song-colored background.
    ListArtwork,
}

impl Extra {
    pub const ALL: [Extra; 5] = [Extra::Animations, Extra::Preloading, Extra::Lookups, Extra::LiveProgress, Extra::ListArtwork];

    fn bit(self) -> u8 {
        1 << self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Extra::Animations => "Animations and moving backgrounds",
            Extra::Preloading => "Loading pages ahead",
            Extra::Lookups => "Background lookups (view counts, featured artists)",
            Extra::LiveProgress => "Smooth progress bar",
            Extra::ListArtwork => "Covers in song lists and the song-colored background",
        }
    }

    /// Switched off by default in this mode.
    pub fn off_in(self, mode: Mode) -> bool {
        match mode {
            Mode::Normal => false,
            Mode::Gaming => !matches!(self, Extra::ListArtwork),
            Mode::UltraLow => true,
        }
    }
}

impl Mode {
    /// How much a mode saves: Normal 0, Gaming 1, Ultra-low 2.
    pub fn level(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "Off",
            Mode::Gaming => "Gaming Mode",
            Mode::UltraLow => "Ultra-low resource",
        }
    }
}

/// Hands memory Pulse isn't using back to Windows (what Task Manager shows drops). Pages
/// Pulse touches again come back on their own, so nothing breaks; it's only done when a
/// mode that saves memory goes on, not repeatedly.
#[cfg(windows)]
pub fn give_back_memory() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32EmptyWorkingSet(process: isize) -> i32;
    }
    // SAFETY: our own process; failing (it can't on supported Windows) changes nothing.
    unsafe { K32EmptyWorkingSet(GetCurrentProcess()) };
}
#[cfg(not(windows))]
pub fn give_back_memory() {}

/// Bits of the extras switched off right now.
static OFF: AtomicU8 = AtomicU8::new(0);
static MODE: AtomicU8 = AtomicU8::new(0);
/// Pulse is drawn without the graphics card (no graphics
/// driver). Every frame then costs the CPU, so animations and the moving background stay off.
static SOFTWARE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_software(on: bool) {
    SOFTWARE.store(on, Ordering::Relaxed);
    // Software drawing: don't render a frame that looks like the one on screen (patches/egui-wgpu).
    #[cfg(windows)]
    eframe::egui_wgpu::winit::SKIP_IDENTICAL_FRAMES.store(on, Ordering::Relaxed);
}

pub fn software() -> bool {
    SOFTWARE.load(Ordering::Relaxed)
}

/// Frame timing for the playback log, only while drawing in software (WARP), to tell whether
/// slow scrolling is Pulse's own drawing work or the renderer. `started` is when this frame's
/// `ui` began. Every 3 s of continuous drawing one line is logged: the work inside `ui`
/// (average, longest) and the gap between frames (average, longest).
pub fn frame_stats(ctx: &egui::Context, started: std::time::Instant, render_secs: Option<f32>, causes: &[egui::RepaintCause], px: [u32; 2]) {
    use std::time::{Duration, Instant};
    #[derive(Default)]
    struct Stats {
        since: Option<Instant>,
        last: Option<Instant>,
        n: u32,
        work: Duration,
        work_max: Duration,
        gaps: u32,
        gap: Duration,
        gap_max: Duration,
        render: f64,
        render_max: f64,
        renders: u32,
        why: std::collections::HashMap<String, u32>,
    }
    static STATS: std::sync::Mutex<Option<Stats>> = std::sync::Mutex::new(None);
    let now = Instant::now();
    let mut guard = STATS.lock().unwrap_or_else(|e| e.into_inner());
    let st = guard.get_or_insert_with(Stats::default);
    let since = *st.since.get_or_insert(started);
    let work = now.duration_since(started);
    st.n += 1;
    st.work += work;
    st.work_max = st.work_max.max(work);
    // Only gaps under half a second count: longer ones are idle time, not slow drawing.
    if let Some(last) = st.last {
        let gap = started.duration_since(last);
        if gap < Duration::from_millis(500) {
            st.gaps += 1;
            st.gap += gap;
            st.gap_max = st.gap_max.max(gap);
        }
    }
    if let Some(r) = render_secs {
        let r = r as f64 * 1000.0;
        st.renders += 1;
        st.render += r;
        st.render_max = st.render_max.max(r);
    }
    for c in causes {
        *st.why.entry(format!("{}:{} {}", c.file.rsplit(['/', '\\']).next().unwrap_or(c.file), c.line, c.reason)).or_default() += 1;
    }
    st.last = Some(started);
    if now.duration_since(since) >= Duration::from_secs(3) {
        if st.gaps >= 10 {
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            crate::audio::trace(&format!(
                "frames: {} in 3 s, ui work avg {:.1} ms max {:.1} ms, gap between frames avg {:.1} ms max {:.1} ms, whole frame cpu (draw + render, no vsync wait) avg {:.1} ms max {:.1} ms, window {}x{} px",
                st.n,
                ms(st.work) / st.n as f64,
                ms(st.work_max),
                ms(st.gap) / st.gaps as f64,
                ms(st.gap_max),
                st.render / st.renders.max(1) as f64,
                st.render_max,
                px[0],
                px[1]
            ));
        }
        #[cfg(windows)]
        {
            let p = eframe::egui_wgpu::winit::take_stats();
            if st.gaps >= 10 {
                crate::audio::trace(&format!(
                    "painter: {} drawn, {} skipped (same as on screen), last drawn frame: {} draw calls, {} triangles, shaded area {:.2}x the window, render wait avg {:.1} ms",
                    p.painted,
                    p.skipped,
                    p.draws,
                    p.triangles,
                    p.area_px as f64 / (px[0] as f64 * px[1] as f64).max(1.0),
                    p.wait_secs as f64 * 1000.0 / p.painted.max(1) as f64
                ));
            }
        }
        if st.gaps >= 10 {
            crate::audio::trace(&memory_line(ctx));
            let mut v: Vec<_> = st.why.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1));
            let top: Vec<String> = v.iter().take(4).map(|(k, n)| format!("{k} x{n}")).collect();
            crate::audio::trace(&format!("redraw asked by: {}", top.join(" | ")));
        }
        let last = st.last;
        *st = Stats { last, ..Default::default() };
    }
}

/// One log line on where Pulse's memory is: what egui holds for pictures (textures on the
/// graphics side, decoded copies, downloaded files) and, on Windows, the process numbers.
/// `private` isn't changed by the idle trim (Task Manager's working set is); `faults` are pages
/// read back in since the last line.
fn memory_line(ctx: &egui::Context) -> String {
    let mb = |b: usize| b as f64 / 1_048_576.0;
    let l = ctx.loaders();
    let textures: usize = l.texture.lock().iter().map(|t| t.byte_size()).sum();
    let decoded: usize = l.image.lock().iter().map(|t| t.byte_size()).sum();
    let files: usize = l.bytes.lock().iter().map(|t| t.byte_size()).sum();
    let n = ctx.tex_manager().read().num_allocated();
    let mut s = format!(
        "memory: {n} egui textures, picture textures {:.1} MB, decoded pictures kept {:.1} MB, downloaded picture files kept {:.1} MB",
        mb(textures),
        mb(decoded),
        mb(files)
    );
    if let Some(p) = process_memory() {
        s += &format!(", process private {:.0} MB, working set {:.0} MB (Task Manager's number: {:.0} MB), {} page faults since last line", mb(p.private), mb(p.working_set), mb(p.private_working_set), p.faults);
    }
    s
}

struct ProcessMemory {
    private: usize,
    working_set: usize,
    /// What Task Manager's Memory column shows (0 = Windows too old to say).
    private_working_set: usize,
    faults: u32,
}

#[cfg(windows)]
fn process_memory() -> Option<ProcessMemory> {
    use std::sync::atomic::AtomicU32;
    static LAST_FAULTS: AtomicU32 = AtomicU32::new(0);
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_non_paged: usize,
        quota_non_paged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
        // The rest is only there on Windows 10 1809 and later.
        private_working_set: usize,
        shared_commit: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters::default();
    c.cb = std::mem::size_of::<Counters>() as u32;
    // SAFETY: our own process and a struct at least as large as the size passed; on failure `c` stays unused.
    if unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } == 0 {
        // An older Windows: ask for the shorter version.
        c.cb = std::mem::offset_of!(Counters, private_working_set) as u32;
        if unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } == 0 {
            return None;
        }
    }
    let _ = c.shared_commit;
    let before = LAST_FAULTS.swap(c.page_faults, Ordering::Relaxed);
    Some(ProcessMemory { private: c.private, working_set: c.working_set, private_working_set: c.private_working_set, faults: c.page_faults.wrapping_sub(before) })
}
#[cfg(not(windows))]
fn process_memory() -> Option<ProcessMemory> {
    None
}

/// Software drawing only: a frame caused by nothing but the pointer moving (no click, scroll or
/// key) is held back so such frames come at most 15 times a second. (egui asks for an
/// immediate repaint whenever a frame has any input event, so that can't be used to tell.)
/// Frames that look identical to the one on screen are then not rendered at all
/// (patches/egui-wgpu).
pub fn pace(ctx: &egui::Context) {
    use std::time::{Duration, Instant};
    static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    const GAP: Duration = Duration::from_millis(66);
    if !software() {
        return;
    }
    let hover_only = ctx.input(|i| {
        !i.events.is_empty()
            && i.events.iter().all(|e| matches!(e, egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_)))
            && i.smooth_scroll_delta == egui::Vec2::ZERO
            && !i.pointer.any_down()
    });
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if hover_only && let Some(l) = *last {
        let wait = GAP.saturating_sub(l.elapsed());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
    *last = Some(Instant::now());
}

/// Puts a mode into effect. `keep` holds the extras you asked to keep on.
pub fn apply(mode: Mode, keep: &[String]) {
    let mut bits = 0;
    for e in Extra::ALL {
        if e.off_in(mode) && !keep.iter().any(|k| k == key(e)) {
            bits |= e.bit();
        }
    }
    OFF.store(bits, Ordering::Relaxed);
    MODE.store(mode as u8, Ordering::Relaxed);
}

/// The name an extra is saved under.
pub fn key(e: Extra) -> &'static str {
    match e {
        Extra::Animations => "animations",
        Extra::Preloading => "preloading",
        Extra::Lookups => "lookups",
        Extra::LiveProgress => "live_progress",
        Extra::ListArtwork => "list_artwork",
    }
}

pub fn on(e: Extra) -> bool {
    // Every frame costs the CPU in software drawing: no animations, progress bar once a second.
    if matches!(e, Extra::Animations | Extra::LiveProgress) && software() {
        return false;
    }
    OFF.load(Ordering::Relaxed) & e.bit() == 0
}

pub fn mode() -> Mode {
    match MODE.load(Ordering::Relaxed) {
        1 => Mode::Gaming,
        2 => Mode::UltraLow,
        _ => Mode::Normal,
    }
}

/// The extras switched off now, for showing to you.
pub fn switched_off() -> Vec<Extra> {
    Extra::ALL.into_iter().filter(|e| !on(*e)).collect()
}

/// How often to redraw while music plays (the progress bar moves).
pub fn playing_redraw() -> std::time::Duration {
    std::time::Duration::from_millis(if on(Extra::LiveProgress) { 250 } else { 1000 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_switch_off_their_extras_and_you_can_keep_some() {
        apply(Mode::Normal, &[]);
        assert!(Extra::ALL.iter().all(|e| on(*e)));
        apply(Mode::Gaming, &[]);
        assert!(!on(Extra::Animations) && !on(Extra::Lookups) && on(Extra::ListArtwork));
        apply(Mode::Gaming, &["animations".into()]);
        assert!(on(Extra::Animations));
        apply(Mode::UltraLow, &[]);
        assert_eq!(switched_off().len(), Extra::ALL.len());
        // Hover effects follow: gone in Ultra-low, back if you keep Animations.
        assert!(!crate::theme::hover_fx());
        apply(Mode::UltraLow, &["animations".into()]);
        assert!(crate::theme::hover_fx());
        apply(Mode::Gaming, &[]);
        assert!(crate::theme::hover_fx());
        apply(Mode::Normal, &[]);
    }

    #[test]
    fn software_drawing_stops_animations() {
        apply(Mode::Normal, &[]);
        set_software(true);
        assert!(!on(Extra::Animations) && !on(Extra::LiveProgress) && on(Extra::ListArtwork));
        set_software(false);
        assert!(on(Extra::Animations));
    }

    #[test]
    fn levels_go_up_with_savings() {
        assert!(Mode::Normal.level() < Mode::Gaming.level() && Mode::Gaming.level() < Mode::UltraLow.level());
    }
}
