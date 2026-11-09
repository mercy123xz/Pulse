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
    /// Moving backgrounds, menu fades and hover animations.
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
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "Off",
            Mode::Gaming => "Gaming Mode",
            Mode::UltraLow => "Ultra-low resource",
        }
    }
}

/// Bits of the extras switched off right now.
static OFF: AtomicU8 = AtomicU8::new(0);
static MODE: AtomicU8 = AtomicU8::new(0);

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
        apply(Mode::Normal, &[]);
    }
}
