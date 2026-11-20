//! Remembers where the window was, and only restores it if that still makes sense.
//!
//! eframe's own window saving restored a minimized window as it was reported (Windows says
//! -32000,-32000 and a tiny size), so Pulse sometimes opened squashed in the top-left
//! corner. This keeps its own small file, never records a minimized or odd window, and
//! checks a saved one against the screens that are connected now.

use std::path::PathBuf;

pub const DEFAULT_SIZE: [f32; 2] = [1280.0, 800.0];
pub const MIN_SIZE: [f32; 2] = [980.0, 620.0];

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WinState {
    /// Top-left of the window and the size of its contents, in points.
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub maximized: bool,
    /// Points per pixel when saved, to compare with the screens (in pixels) on load.
    pub scale: f32,
}

fn file() -> PathBuf {
    crate::settings::data_dir().join("window.json")
}

/// The screens together, in pixels: (left, top, right, bottom). None if unknown.
#[cfg(windows)]
fn screen() -> Option<[f32; 4]> {
    unsafe extern "system" {
        fn GetSystemMetrics(index: i32) -> i32;
    }
    // SAFETY: plain query with no pointers.
    let m = |i| unsafe { GetSystemMetrics(i) } as f32;
    let (x, y, w, h) = (m(76), m(77), m(78), m(79)); // virtual screen
    (w > 0.0 && h > 0.0).then_some([x, y, x + w, y + h])
}
#[cfg(not(windows))]
fn screen() -> Option<[f32; 4]> {
    None
}

impl WinState {
    /// A window worth restoring: big enough, not absurd, and with its title bar on a screen.
    pub fn sane(&self, screen: Option<[f32; 4]>) -> bool {
        let finite = self.pos.iter().chain(&self.size).chain([&self.scale]).all(|v| v.is_finite());
        if !finite || self.scale <= 0.0 || self.size[0] < MIN_SIZE[0] || self.size[1] < MIN_SIZE[1] || self.size[0] > 16_000.0 || self.size[1] > 16_000.0 {
            return false;
        }
        // Windows parks minimized windows around -32000.
        if self.pos[0] <= -10_000.0 || self.pos[1] <= -10_000.0 {
            return false;
        }
        let Some([l, t, r, b]) = screen else { return true };
        let (x, y, w) = (self.pos[0] * self.scale, self.pos[1] * self.scale, self.size[0] * self.scale);
        // At least 100 px of the title bar must be on a screen, so it can still be grabbed.
        x + w >= l + 100.0 && x <= r - 100.0 && y >= t - 10.0 && y <= b - 60.0
    }

    /// The saved state, if there is one and it's usable.
    pub fn load() -> Option<WinState> {
        let state: WinState = serde_json::from_str(&std::fs::read_to_string(file()).ok()?).ok()?;
        state.sane(screen()).then_some(state)
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(file(), json);
        }
    }

    /// Reads the window's current state, or None while it's minimized, full screen or still
    /// opening (no reliable numbers then).
    pub fn current(ctx: &egui::Context, previous: Option<WinState>) -> Option<WinState> {
        let v = ctx.input(|i| i.viewport().clone());
        if v.minimized.unwrap_or(false) || v.fullscreen.unwrap_or(false) {
            return None;
        }
        let scale = ctx.pixels_per_point();
        let maximized = v.maximized.unwrap_or(false);
        let mut s = previous.unwrap_or(WinState { pos: [0.0; 2], size: DEFAULT_SIZE, maximized, scale });
        s.maximized = maximized;
        s.scale = scale;
        // A maximized window keeps the size and place it had before, to go back to.
        if !maximized {
            let (outer, inner) = (v.outer_rect?, v.inner_rect?);
            s.pos = [outer.min.x, outer.min.y];
            s.size = [inner.width(), inner.height()];
        }
        s.sane(screen()).then_some(s)
    }
}

static LAST: std::sync::Mutex<Option<WinState>> = std::sync::Mutex::new(None);
static WRITTEN: std::sync::Mutex<Option<WinState>> = std::sync::Mutex::new(None);

/// Call every frame: notes the window's state while it's trustworthy.
pub fn track(ctx: &egui::Context) {
    let mut last = LAST.lock().unwrap();
    if let Some(now) = WinState::current(ctx, *last) {
        *last = Some(now);
    }
}

/// Writes the last good state to disk if it changed (called with the regular saves).
pub fn flush() {
    let last = *LAST.lock().unwrap();
    let mut written = WRITTEN.lock().unwrap();
    if let Some(s) = last.filter(|s| Some(*s) != *written) {
        s.save();
        *written = Some(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(x: f32, y: f32, w: f32, h: f32) -> WinState {
        WinState { pos: [x, y], size: [w, h], maximized: false, scale: 1.0 }
    }
    const SCREEN: Option<[f32; 4]> = Some([0.0, 0.0, 1920.0, 1080.0]);

    #[test]
    fn a_normal_window_is_restored() {
        assert!(state(100.0, 80.0, 1280.0, 800.0).sane(SCREEN));
    }

    #[test]
    fn a_minimized_window_is_never_restored() {
        assert!(!state(-32000.0, -32000.0, 160.0, 28.0).sane(SCREEN));
        assert!(!state(-32000.0, -32000.0, 1280.0, 800.0).sane(None));
        assert!(!state(0.0, 0.0, 160.0, 28.0).sane(SCREEN), "a tiny window is not worth restoring");
    }

    #[test]
    fn a_window_left_on_a_disconnected_screen_is_not_restored() {
        assert!(!state(2500.0, 100.0, 1280.0, 800.0).sane(SCREEN));
        assert!(!state(100.0, 1500.0, 1280.0, 800.0).sane(SCREEN));
        assert!(state(-1000.0, 100.0, 1280.0, 800.0).sane(SCREEN), "partly off the left edge is still grabbable");
    }

    #[test]
    fn scale_is_taken_into_account() {
        let hidpi = WinState { scale: 2.0, ..state(1000.0, 100.0, 1280.0, 800.0) };
        assert!(!hidpi.sane(SCREEN), "2000 px across on a 1920 px screen");
    }

    #[test]
    fn nan_and_huge_sizes_are_rejected() {
        assert!(!state(f32::NAN, 0.0, 1280.0, 800.0).sane(None));
        assert!(!state(0.0, 0.0, 99_999.0, 800.0).sane(None));
    }
}
