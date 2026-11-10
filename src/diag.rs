//! What the Diagnostics page shows: versions, where the disk space goes, and a quick
//! check that Pulse can reach the services it uses. Nothing here is sent anywhere;
//! "Copy report" puts it on the clipboard, without file paths (they hold your name).

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Instant;

/// The facts that take a moment to find (they run programs and add up folders).
#[derive(Clone, Default)]
pub struct Facts {
    pub os: String,
    pub ytdlp: String,
    pub deno: String,
    /// What uses disk space, biggest first.
    pub storage: Vec<(&'static str, u64)>,
}

/// One service the self-test reaches.
pub struct Service {
    pub name: &'static str,
    url: &'static str,
}

pub const SERVICES: [Service; 4] = [
    Service { name: "Deezer (music info)", url: "https://api.deezer.com/chart/0/tracks?limit=1" },
    Service { name: "YouTube (audio)", url: "https://www.youtube.com/generate_204" },
    Service { name: "LRCLIB (lyrics)", url: "https://lrclib.net/api/search?q=hello" },
    Service { name: "GitHub (updates)", url: "https://api.github.com/repos/mercy123xz/Pulse/releases/latest" },
];

#[derive(Clone)]
pub enum Check {
    Running,
    Passed { ms: u64, detail: String },
    Failed(String),
}

/// The page's state, kept while Pulse runs.
#[derive(Default)]
pub struct Diagnostics {
    pub facts: Option<Facts>,
    facts_rx: Option<Receiver<Facts>>,
    /// One per service once the self-test has been run.
    pub checks: Vec<Check>,
    checks_rx: Option<Receiver<(usize, Check)>>,
    /// The graphics adapter, read from the window on the first frame.
    pub gpu: Option<String>,
}

impl Diagnostics {
    /// Picks up finished work, and starts reading the facts if nobody has yet.
    /// `offline` is the downloads folder.
    pub fn poll(&mut self, ctx: &egui::Context, offline: PathBuf) {
        if let Some(rx) = &self.facts_rx {
            match rx.try_recv() {
                Ok(f) => {
                    self.facts = Some(f);
                    self.facts_rx = None;
                }
                Err(TryRecvError::Disconnected) => self.facts_rx = None,
                Err(TryRecvError::Empty) => {}
            }
        } else if self.facts.is_none() {
            self.refresh(ctx, offline);
        }
        if let Some(rx) = &self.checks_rx {
            loop {
                match rx.try_recv() {
                    Ok((i, c)) => {
                        if let Some(slot) = self.checks.get_mut(i) {
                            *slot = c;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.checks_rx = None;
                        break;
                    }
                }
            }
        }
    }

    pub fn loading(&self) -> bool {
        self.facts_rx.is_some()
    }

    /// Reads versions and folder sizes again, in the background.
    pub fn refresh(&mut self, ctx: &egui::Context, offline: PathBuf) {
        if self.facts_rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(gather(offline));
            ctx.request_repaint();
        });
        self.facts_rx = Some(rx);
    }

    pub fn testing(&self) -> bool {
        self.checks_rx.is_some()
    }

    /// Reaches every service at once; results arrive as they finish.
    pub fn run_self_test(&mut self, ctx: &egui::Context) {
        if self.checks_rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.checks = vec![Check::Running; SERVICES.len()];
        for (i, service) in SERVICES.iter().enumerate() {
            let (tx, ctx, url) = (tx.clone(), ctx.clone(), service.url);
            std::thread::spawn(move || {
                let start = Instant::now();
                let result = match check(i, url) {
                    Ok(detail) => Check::Passed { ms: start.elapsed().as_millis() as u64, detail },
                    Err(e) => Check::Failed(e),
                };
                let _ = tx.send((i, result));
                ctx.request_repaint();
            });
        }
        self.checks_rx = Some(rx);
    }
}

/// Asks one service for something small and makes sure the answer makes sense.
fn check(i: usize, url: &str) -> Result<String, String> {
    let mut resp = crate::net::api().get(url).call().map_err(crate::net::friendly_error)?;
    let body = resp.body_mut().with_config().limit(2 * 1024 * 1024).read_to_string().map_err(|e| format!("{} ({e})", crate::i18n::tr("The answer was cut off")))?;
    let json = || serde_json::from_str::<serde_json::Value>(&body).map_err(|_| crate::i18n::tr("It answered, but not with what Pulse expects").to_string());
    match i {
        0 => {
            let v = json()?;
            if v["data"].as_array().is_some_and(|a| !a.is_empty()) { Ok(String::new()) } else { Err(crate::i18n::tr("It answered, but with no songs").into()) }
        }
        2 => json()?.as_array().map(|_| String::new()).ok_or_else(|| crate::i18n::tr("It answered, but not with lyrics").into()),
        3 => {
            let v = json()?;
            let tag = v["tag_name"].as_str().ok_or_else(|| crate::i18n::tr("It answered, but without a release").to_string())?;
            Ok(format!("{} {tag}", crate::i18n::tr("Newest release:")))
        }
        _ => Ok(String::new()),
    }
}

/// Versions and folder sizes. Runs programs, so it's done in the background.
fn gather(offline: PathBuf) -> Facts {
    let (ytdlp, deno) = crate::youtube::tool_versions();
    let data = crate::settings::data_dir();
    let size = |p: &std::path::Path| crate::settings::folder_size(p);
    let cache = data.join("cache");
    let (art, lyrics, songs) = (size(&cache.join("art")), size(&cache.join("lyrics")), size(&cache.join("audio")));
    let tools = crate::youtube::tools_dir();
    let tools_cache = size(&tools.join("cache"));
    let library = eframe::storage_dir("pulse-player").and_then(|d| std::fs::metadata(d.join("app.ron")).ok()).map_or(0, |m| m.len());
    let mut storage = vec![
        ("Downloaded songs", size(&offline)),
        ("Played songs kept", songs),
        ("Album art cache", art),
        ("Lyrics cache", lyrics),
        ("Other cache", size(&cache).saturating_sub(art + lyrics + songs) + tools_cache),
        ("yt-dlp and Deno", size(&tools).saturating_sub(tools_cache)),
        ("Library and settings", library),
        ("Backups", size(&crate::backup::dir())),
        ("Crash reports", size(&crate::crash::dir())),
    ];
    storage.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    Facts { os: os_name(), ytdlp, deno, storage }
}

/// "Windows 11 (build 22631.4460)", or the system's own name elsewhere.
fn os_name() -> String {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("cmd").args(["/c", "ver"]).creation_flags(CREATE_NO_WINDOW).output();
        if let Ok(out) = out {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(v) = text.split("Version ").nth(1).map(|v| v.trim().trim_end_matches(']')) {
                let build = v.split('.').skip(2).collect::<Vec<_>>().join(".");
                let major: u32 = build.split('.').next().and_then(|b| b.parse().ok()).unwrap_or(0);
                let name = if major >= 22000 { "Windows 11" } else { "Windows 10" };
                return format!("{name} (build {build}), {}", std::env::consts::ARCH);
            }
        }
    }
    #[cfg(target_os = "linux")]
    if let Ok(release) = std::fs::read_to_string("/etc/os-release")
        && let Some(name) = release.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))
    {
        return format!("{}, {}", name.trim_matches('"'), std::env::consts::ARCH);
    }
    format!("{}, {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// True when the window is drawn without the graphics card.
pub fn software(frame: &eframe::Frame) -> bool {
    #[cfg(windows)]
    if let Some(rs) = frame.wgpu_render_state() {
        return rs.adapter.get_info().device_type == eframe::egui_wgpu::wgpu::DeviceType::Cpu;
    }
    let _ = frame;
    false
}

/// The graphics adapter drawing the window, and how.
pub fn gpu(frame: &eframe::Frame) -> String {
    #[cfg(windows)]
    if let Some(rs) = frame.wgpu_render_state() {
        let i = rs.adapter.get_info();
        let how = if i.device_type == eframe::egui_wgpu::wgpu::DeviceType::Cpu { "software drawing" } else { "hardware" };
        let driver = if i.driver_info.is_empty() { String::new() } else { format!(", driver {}", i.driver_info) };
        return format!("{} ({:?}, {how}{driver})", i.name, i.backend);
    }
    if let Some(gl) = frame.gl() {
        use eframe::glow::HasContext;
        // SAFETY: reads a string from the window's live OpenGL context.
        let name = unsafe { gl.get_parameter_string(eframe::glow::RENDERER) };
        return format!("{name} (OpenGL)");
    }
    "Unknown".into()
}

/// A short time span ("240 ms", "2.5 s").
fn ms_text(ms: u64) -> String {
    if ms < 1000 { format!("{ms} ms") } else { format!("{:.1} s", ms as f64 / 1000.0) }
}

impl Check {
    /// One line for the page and the report.
    pub fn summary(&self) -> String {
        match self {
            Check::Running => crate::i18n::tr("Checking…").to_string(),
            Check::Passed { ms, detail } if detail.is_empty() => format!("{} ({})", crate::i18n::tr("Working"), ms_text(*ms)),
            Check::Passed { ms, detail } => format!("{} ({}) · {detail}", crate::i18n::tr("Working"), ms_text(*ms)),
            Check::Failed(e) => e.clone(),
        }
    }
}

/// Everything on the page as plain text, for pasting into an issue. No file paths.
pub fn report(d: &Diagnostics, settings: &crate::settings::Settings) -> String {
    let mut out = String::from("Pulse diagnostics\n\n");
    let f = d.facts.clone().unwrap_or_default();
    let line = |out: &mut String, k: &str, v: &str| out.push_str(&format!("{k}: {v}\n"));
    line(&mut out, "Pulse", env!("CARGO_PKG_VERSION"));
    line(&mut out, "System", &f.os);
    line(&mut out, "Graphics", d.gpu.as_deref().unwrap_or("Unknown"));
    line(&mut out, "Hardware acceleration", if settings.hardware_acceleration { "on" } else { "off" });
    line(&mut out, "Performance mode", &format!("{:?}", crate::perf::mode()));
    line(&mut out, "yt-dlp", &f.ytdlp);
    line(&mut out, "Deno", &f.deno);
    out.push_str("\nStorage:\n");
    for (name, n) in &f.storage {
        out.push_str(&format!("  {name}: {}\n", crate::settings::fmt_bytes(*n)));
    }
    if !d.checks.is_empty() {
        out.push_str("\nSelf-test:\n");
        for (s, c) in SERVICES.iter().zip(&d.checks) {
            let mark = match c {
                Check::Passed { .. } => "ok",
                Check::Failed(_) => "FAILED",
                Check::Running => "…",
            };
            out.push_str(&format!("  {} [{mark}]: {}\n", s.name, c.summary()));
        }
    }
    let problems = crate::crash::recent();
    out.push_str(&format!("\nRecent problems ({}):\n", problems.len()));
    for p in problems.iter().rev() {
        out.push_str(&format!("  {p}\n"));
    }
    out.push_str(&format!("\nCrash reports saved: {}\n", crate::crash::reports().len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_has_no_file_paths() {
        let d = Diagnostics { facts: Some(gather(std::env::temp_dir().join("pulse-no-downloads"))), ..Default::default() };
        let text = report(&d, &crate::settings::Settings::default());
        assert!(text.contains("Storage:"));
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
        assert!(home.is_empty() || !text.contains(&home), "the report mustn't show your folders");
    }

    #[test]
    fn times_read_naturally() {
        assert_eq!(ms_text(240), "240 ms");
        assert_eq!(ms_text(2500), "2.5 s");
    }
}
