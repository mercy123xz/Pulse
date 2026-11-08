//! Everything on the Settings page, and where Pulse keeps its files.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Pulse's data folder: %APPDATA%\pulse-player on Windows.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PULSE_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("pulse-player")
}

/// The user's Music folder (C:\Users\<name>\Music on Windows).
pub fn music_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join("Music"))
}

/// A `file:///` address for a picture on this computer, as egui's image loader
/// expects it (on Windows: file:///C:/Users/…; `file://C:/…` would be read as a network path).
pub fn file_uri(path: &std::path::Path) -> String {
    let p = path.display().to_string().replace('\\', "/");
    format!("file:///{}", p.trim_start_matches('/'))
}

/// Where downloaded songs go unless the user picks another folder.
pub fn default_offline_dir() -> PathBuf {
    data_dir().join("offline")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    English,
    Spanish,
    Portuguese,
    French,
    German,
}

impl Language {
    pub const ALL: [Language; 5] = [Language::English, Language::Spanish, Language::Portuguese, Language::French, Language::German];

    /// The language's name in that language, as shown in the picker.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Spanish => "Español",
            Language::Portuguese => "Português",
            Language::French => "Français",
            Language::German => "Deutsch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Quality {
    #[default]
    Auto,
    Low,
    Medium,
    High,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Auto, Quality::Low, Quality::Medium, Quality::High];

    pub fn label(self) -> &'static str {
        match self {
            Quality::Auto => "Automatic",
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
        }
    }

    /// yt-dlp format choice. YouTube's M4A audio comes in about 48 kbps and
    /// 128 kbps; Auto and High take the best one available.
    pub fn ytdlp_format(self) -> &'static str {
        match self {
            Quality::Low => "worstaudio[ext=m4a]/bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/bestaudio[ext=mp3]",
            Quality::Medium => "bestaudio[ext=m4a][abr<=130]/bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/bestaudio[ext=mp3]",
            Quality::Auto | Quality::High => "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/bestaudio[ext=mp3]",
        }
    }
}

/// Equalizer band centres, matching the Settings page.
pub const EQ_BANDS: [f32; 6] = [60.0, 150.0, 400.0, 1000.0, 2400.0, 15000.0];
/// Gain range of each band, in decibels.
pub const EQ_MAX_DB: f32 = 12.0;

/// Built-in equalizer presets: (name, gains in dB per band).
pub const EQ_PRESETS: [(&str, [f32; 6]); 8] = [
    ("Flat", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Bass Booster", [6.0, 4.5, 1.0, 0.0, 0.0, 0.0]),
    ("Bass Reducer", [-6.0, -4.0, -1.0, 0.0, 0.0, 0.0]),
    ("Treble Booster", [0.0, 0.0, 0.0, 1.5, 4.0, 6.0]),
    ("Vocal Booster", [-2.0, -1.0, 2.5, 4.0, 2.5, 0.0]),
    ("Loudness", [5.0, 3.0, 0.0, -1.0, 2.0, 5.0]),
    ("Acoustic", [4.0, 3.0, 1.0, 2.0, 3.0, 3.0]),
    ("Electronic", [4.5, 3.5, 0.0, -1.5, 2.0, 4.5]),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: Language,
    pub stream_quality: Quality,
    pub download_quality: Quality,
    /// Show songs from the Music folder in the sidebar.
    pub show_local_files: bool,
    /// Seconds of overlap between songs (0 = off, up to 10).
    pub crossfade_secs: f32,
    /// Start the next song with no silence in between.
    pub gapless: bool,
    /// Smooth transitions: trims silence at the end of songs and blends into the next.
    pub automix: bool,
    pub mono: bool,
    pub eq_enabled: bool,
    pub eq_gains: [f32; 6],
    /// Preset name, or "Manual" after changing a band by hand.
    pub eq_preset: String,
    /// Your own saved presets.
    pub eq_custom: Vec<(String, [f32; 6])>,
    /// Folder for downloaded songs (None = default).
    pub offline_dir: Option<PathBuf>,
    pub high_contrast: bool,
    pub hardware_acceleration: bool,
    /// Theme name and its color scheme.
    pub theme: String,
    /// Empty = the theme's first scheme.
    pub scheme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::English,
            stream_quality: Quality::Auto,
            download_quality: Quality::High,
            show_local_files: false,
            crossfade_secs: 0.0,
            gapless: true,
            automix: false,
            mono: false,
            eq_enabled: false,
            eq_gains: [0.0; 6],
            eq_preset: "Flat".into(),
            eq_custom: vec![],
            offline_dir: None,
            high_contrast: false,
            hardware_acceleration: true,
            theme: "Default".into(),
            scheme: String::new(),
        }
    }
}

impl Settings {
    pub fn offline_dir(&self) -> PathBuf {
        self.offline_dir.clone().unwrap_or_else(default_offline_dir)
    }
}

// ---------------------------------------------------------------------------
// Hardware acceleration is needed before the window opens, earlier than the
// saved settings load, so it's also kept in a small file of its own.
// ---------------------------------------------------------------------------

fn software_flag() -> PathBuf {
    data_dir().join("software-rendering")
}

pub fn hardware_acceleration_enabled() -> bool {
    !software_flag().exists()
}

pub fn set_hardware_acceleration(on: bool) {
    let flag = software_flag();
    if on {
        let _ = std::fs::remove_file(flag);
    } else {
        let _ = std::fs::create_dir_all(data_dir());
        let _ = std::fs::write(flag, "Pulse draws without the graphics card. Delete this file to undo.\n");
    }
}

/// Size of a folder in bytes (for "clear cache").
pub fn folder_size(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => folder_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

pub fn fmt_bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.0} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.0} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_saves_without_settings_get_defaults() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, Settings::default());
        let s: Settings = serde_json::from_str(r#"{"mono":true,"crossfade_secs":4.0}"#).unwrap();
        assert!(s.mono);
        assert_eq!(s.crossfade_secs, 4.0);
        assert!(s.gapless);
    }

    #[test]
    fn picture_addresses() {
        assert_eq!(file_uri(std::path::Path::new("/home/a/b.png")), "file:///home/a/b.png");
        assert_eq!(file_uri(std::path::Path::new("C:\\Users\\x\\c.png")), "file:///C:/Users/x/c.png");
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(3 << 20), "3 MB");
        assert_eq!(fmt_bytes(3 << 30), "3.0 GB");
    }
}
