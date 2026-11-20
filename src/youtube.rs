//! Full-length audio from YouTube, through the open-source yt-dlp tool.
//!
//! The first time a song plays, Pulse downloads yt-dlp and Deno (the JavaScript
//! engine yt-dlp needs for YouTube) from their official GitHub releases into its
//! data folder, checks them against the published SHA-256 checksums, and keeps
//! yt-dlp up to date afterwards so playback keeps working when YouTube changes.
//!
//! For each song, Pulse searches YouTube for "artist - title", picks the result
//! that best matches (official audio, right length), and asks yt-dlp for the
//! direct audio link.

use crate::audio::{Codec, StreamSource};
use crate::deezer::Track;
use crate::settings::Quality;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Tools: yt-dlp + Deno
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Tools {
    pub ytdlp: PathBuf,
    pub deno: PathBuf,
    pub dir: PathBuf,
}

struct Asset {
    /// File name on the yt-dlp / Deno release page.
    name: &'static str,
    /// Name to save it as.
    save_as: &'static str,
}

// On Windows, the "folder" build of yt-dlp: unlike the single .exe it doesn't unpack
// itself to a temp folder every time it starts, so each song starts 1–3 s sooner.
// It's unzipped into tools\ytdlp\.
#[cfg(windows)]
const YTDLP: Asset = Asset { name: "yt-dlp_win.zip", save_as: "ytdlp/yt-dlp.exe" };
/// True when YTDLP is a zip that unpacks into a folder.
const YTDLP_IS_ZIP: bool = cfg!(windows);
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const YTDLP: Asset = Asset { name: "yt-dlp_linux", save_as: "yt-dlp" };
#[cfg(target_os = "macos")]
const YTDLP: Asset = Asset { name: "yt-dlp_macos", save_as: "yt-dlp" };

#[cfg(windows)]
const DENO: Asset = Asset { name: "deno-x86_64-pc-windows-msvc.zip", save_as: "deno.exe" };
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const DENO: Asset = Asset { name: "deno-x86_64-unknown-linux-gnu.zip", save_as: "deno" };
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const DENO: Asset = Asset { name: "deno-aarch64-apple-darwin.zip", save_as: "deno" };
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const DENO: Asset = Asset { name: "deno-x86_64-apple-darwin.zip", save_as: "deno" };

const YTDLP_RELEASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
const DENO_RELEASE: &str = "https://github.com/denoland/deno/releases/latest/download";

/// Folder where the tools live: %APPDATA%\pulse-player\tools on Windows.
pub fn tools_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PULSE_TOOLS_DIR") {
        return PathBuf::from(dir);
    }
    crate::settings::data_dir().join("tools")
}

/// Only one setup at a time (several songs may ask at once).
static SETUP: Mutex<()> = Mutex::new(());

/// Makes sure yt-dlp and Deno are installed, downloading them the first time.
/// `status` receives progress text for the player bar.
pub fn ensure_tools(status: &dyn Fn(&str)) -> Result<Tools, String> {
    let _guard = SETUP.lock().unwrap_or_else(|e| e.into_inner());
    let tools = installed_tools();
    if tools.ytdlp.is_file() && tools.deno.is_file() {
        update_in_background(&tools);
        measure_startup_once(&tools);
        return Ok(tools);
    }
    std::fs::create_dir_all(&tools.dir).map_err(|e| format!("Couldn't create {} ({e})", tools.dir.display()))?;

    // Rough sizes, only used to show one combined percentage.
    const YTDLP_MB: f64 = 24.0;
    const DENO_MB: f64 = 43.0;
    let show = |done_mb: f64| {
        let pct = (done_mb / (YTDLP_MB + DENO_MB) * 100.0).clamp(0.0, 99.0);
        status(&format!("First-time setup… {pct:.0}%"));
    };
    show(0.0);

    if !tools.ytdlp.is_file() {
        install_ytdlp(&tools, &|mb| show(mb))?;
    }
    if !tools.deno.is_file() {
        let sums = crate::net::get_text(&format!("{DENO_RELEASE}/{}.sha256sum", DENO.name))?;
        let expected = first_sha256(&sums).ok_or("Deno's checksum file is unreadable")?;
        let zip = crate::net::download(&format!("{DENO_RELEASE}/{}", DENO.name), |done, _| show(YTDLP_MB + done as f64 / 1e6))?;
        verify(&zip, &expected, "Deno")?;
        let exe = unzip_one(&zip, DENO.save_as)?;
        write_executable(&tools.deno, &exe)?;
    }
    stamp_updated(&tools);
    Ok(tools)
}

fn installed_tools() -> Tools {
    let dir = tools_dir();
    Tools { ytdlp: dir.join(YTDLP.save_as), deno: dir.join(DENO.save_as), dir }
}

/// The checksum yt-dlp publishes for the current release of our build.
fn latest_ytdlp_sum() -> Result<String, String> {
    let sums = crate::net::get_text(&format!("{YTDLP_RELEASE}/SHA2-256SUMS"))?;
    checksum_for(&sums, YTDLP.name).ok_or_else(|| "yt-dlp's checksum list is missing this file".to_string())
}

/// Downloads, checks and installs yt-dlp. For the folder build, the new copy is
/// unpacked next to the old one and swapped in, so a failed update never breaks it.
fn install_ytdlp(tools: &Tools, progress: &dyn Fn(f64)) -> Result<(), String> {
    let expected = latest_ytdlp_sum()?;
    let bytes = crate::net::download(&format!("{YTDLP_RELEASE}/{}", YTDLP.name), |done, _| progress(done as f64 / 1e6))?;
    verify(&bytes, &expected, "yt-dlp")?;
    let _installing = Installing::start();
    if YTDLP_IS_ZIP {
        let folder = tools.ytdlp.parent().unwrap_or(&tools.dir).to_path_buf();
        let fresh = folder.with_extension("new");
        let _ = std::fs::remove_dir_all(&fresh);
        unzip_all(&bytes, &fresh)?;
        let old = folder.with_extension("old");
        let _ = std::fs::remove_dir_all(&old);
        if folder.exists() {
            std::fs::rename(&folder, &old).map_err(|e| format!("Couldn't replace yt-dlp ({e}); it may be busy"))?;
        }
        std::fs::rename(&fresh, &folder).map_err(|e| format!("Couldn't install yt-dlp ({e})"))?;
        let _ = std::fs::remove_dir_all(&old);
        // The single-file yt-dlp.exe from older Pulse versions isn't needed any more.
        let _ = std::fs::remove_file(tools.dir.join("yt-dlp.exe"));
    } else {
        write_executable(&tools.ytdlp, &bytes)?;
    }
    let _ = std::fs::write(tools.dir.join("yt-dlp.sha256"), &expected);
    Ok(())
}

/// Unpacks a whole zip into `dest` (rejecting paths that would leave it).
fn unzip_all(zip: &[u8], dest: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| format!("Bad yt-dlp download ({e})"))?;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| format!("Bad yt-dlp download ({e})"))?;
        let Some(rel) = f.enclosed_name() else { return Err("Bad yt-dlp download (unsafe file name)".into()) };
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = std::fs::File::create(&out).map_err(|e| format!("Couldn't unpack yt-dlp ({e})"))?;
        std::io::copy(&mut f, &mut file).map_err(|e| format!("Couldn't unpack yt-dlp ({e})"))?;
    }
    Ok(())
}

/// Gets the newest yt-dlp if it changed (checked by its published checksum).
fn update_ytdlp(tools: &Tools) {
    if !YTDLP_IS_ZIP {
        let _installing = Installing::start();
        let _ = run(&tools.ytdlp, &["-U".into()], Duration::from_secs(120));
        return;
    }
    let installed = std::fs::read_to_string(tools.dir.join("yt-dlp.sha256")).unwrap_or_default();
    if let Ok(latest) = latest_ytdlp_sum()
        && latest.trim() != installed.trim()
    {
        let _ = install_ytdlp(tools, &|_| {});
    }
}

/// Finds the checksum for `file` in a `sha256sum`-style list.
fn checksum_for(list: &str, file: &str) -> Option<String> {
    list.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == file && is_sha256(hash)).then(|| hash.to_ascii_lowercase())
    })
}

/// The first SHA-256 in a text (Deno's Windows checksum files are PowerShell output).
fn first_sha256(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_ascii_hexdigit()).find(|w| is_sha256(w)).map(|w| w.to_ascii_lowercase())
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn verify(bytes: &[u8], expected: &str, what: &str) -> Result<(), String> {
    let got = format!("{:x}", Sha256::digest(bytes));
    if got == expected { Ok(()) } else { Err(format!("The {what} download was damaged or tampered with, so Pulse didn't use it. Try again.")) }
}

fn unzip_one(zip: &[u8], name: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| format!("Bad Deno download ({e})"))?;
    let mut file = archive.by_name(name).map_err(|_| format!("{name} is missing from the Deno download"))?;
    let mut out = Vec::new();
    file.read_to_end(&mut out).map_err(|e| format!("Couldn't unpack Deno ({e})"))?;
    Ok(out)
}

fn write_executable(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("download");
    std::fs::write(&tmp, bytes).map_err(|e| format!("Couldn't save {} ({e})", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("Couldn't save {} ({e})", path.display()))
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn stamp_updated(tools: &Tools) {
    let _ = std::fs::write(tools.dir.join("last-update.txt"), now_secs().to_string());
}

/// Runs `yt-dlp -U` in the background at most once a day, so fixes for YouTube
/// changes arrive automatically.
fn update_in_background(tools: &Tools) {
    let stamp = std::fs::read_to_string(tools.dir.join("last-update.txt")).ok().and_then(|s| s.trim().parse::<u64>().ok());
    if stamp.is_some_and(|t| now_secs().saturating_sub(t) < 24 * 3600) {
        return;
    }
    stamp_updated(tools);
    let tools = tools.clone();
    std::thread::spawn(move || update_ytdlp(&tools));
}

/// Runs yt-dlp right now to pick up a fix, after a song failed to play.
fn update_now(tools: &Tools) {
    stamp_updated(tools);
    update_ytdlp(tools);
}

// ---------------------------------------------------------------------------
// Running yt-dlp
// ---------------------------------------------------------------------------

/// Runs a program and returns its output. Gives up after `timeout`.
pub(crate) fn run(program: &Path, args: &[String], timeout: Duration) -> Result<String, String> {
    finish(spawn(program, args, Stdio::null())?, timeout)
}

/// Starts a program with its output captured (and no console window).
fn spawn(program: &Path, args: &[String], stdin: Stdio) -> Result<Child, String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(stdin).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000; // no black console window popping up
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().map_err(|e| format!("Couldn't start {} ({e})", program.display()))
}

/// Waits for a started program and returns its output. Gives up after `timeout`.
fn finish(mut child: Child, timeout: Duration) -> Result<String, String> {
    // Read output on threads so a chatty program can't fill the pipe and stall.
    let mut out_pipe = child.stdout.take().unwrap();
    let mut err_pipe = child.stderr.take().unwrap();
    let out_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut out_pipe, &mut s);
        s
    });
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut err_pipe, &mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("YouTube took too long to answer. Try again.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(e) => return Err(format!("yt-dlp failed ({e})")),
        }
    };
    let out = out_t.join().unwrap_or_default();
    let err = err_t.join().unwrap_or_default();
    if status.success() { Ok(out) } else { Err(explain_error(&err)) }
}

// ---------------------------------------------------------------------------
// A yt-dlp started ahead of time
// ---------------------------------------------------------------------------

/// yt-dlp is a Python program: each copy takes about a second to start before it does
/// anything. While someone is picking songs, one copy is kept started and waiting for a
/// link on its input (`-a -`), so pressing a song skips that second. It's kept for a
/// minute after the last song activity and never in Gaming or Ultra-low mode. Waiting,
/// it uses no CPU and about 50 MB, and it quits by itself when Pulse closes (its input
/// closes, so it has no link to look up).
struct Spare {
    child: Child,
    /// What it was started with (everything but the link).
    args: Vec<String>,
    started: Instant,
}

static SPARE: Mutex<Option<Spare>> = Mutex::new(None);
/// Keep a spare ready until then.
static SPARE_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);
/// A thread is watching for the spare to go unused.
static SPARE_KEEPER: AtomicBool = AtomicBool::new(false);
/// yt-dlp is being replaced: no spare may hold its files open.
static INSTALLING: AtomicBool = AtomicBool::new(false);
/// How long a spare is kept after the last song activity.
const SPARE_FOR: Duration = Duration::from_secs(60);
/// An older spare is replaced, in case yt-dlp was updated since.
const SPARE_MAX_AGE: Duration = Duration::from_secs(10 * 60);

/// Someone is picking songs (pointing at one, opening a list, playing): have a yt-dlp
/// started and waiting for the next song.
pub fn get_ready(quality: Quality) {
    if crate::perf::mode() != crate::perf::Mode::Normal || !tools_ready() {
        discard_spare();
        return;
    }
    *SPARE_UNTIL.lock().unwrap() = Some(Instant::now() + SPARE_FOR);
    let tools = installed_tools();
    let args = link_args(&tools, quality);
    if SPARE.lock().unwrap().as_ref().is_some_and(|s| s.args == args && s.started.elapsed() < SPARE_MAX_AGE) {
        return;
    }
    std::thread::spawn(move || start_spare(&tools, args));
}

fn spare_wanted() -> bool {
    crate::perf::mode() == crate::perf::Mode::Normal && SPARE_UNTIL.lock().unwrap().is_some_and(|t| Instant::now() < t)
}

fn start_spare(tools: &Tools, args: Vec<String>) {
    if INSTALLING.load(Ordering::SeqCst) {
        return;
    }
    {
        let mut spare = SPARE.lock().unwrap();
        if spare.as_ref().is_some_and(|s| s.args == args && s.started.elapsed() < SPARE_MAX_AGE) {
            return;
        }
        if let Some(mut old) = spare.take() {
            let _ = old.child.kill();
            let _ = old.child.wait();
        }
        let mut with_input = args.clone();
        with_input.extend(["-a".into(), "-".into()]);
        if let Ok(child) = spawn(&tools.ytdlp, &with_input, Stdio::piped()) {
            *spare = Some(Spare { child, args, started: Instant::now() });
        }
    }
    // Ends the spare once nobody has picked a song for a while.
    if !SPARE_KEEPER.swap(true, Ordering::SeqCst) {
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(Duration::from_secs(5));
                if !spare_wanted() {
                    discard_spare();
                }
                if SPARE.lock().unwrap().is_none() {
                    SPARE_KEEPER.store(false, Ordering::SeqCst);
                    // A spare started just now: keep watching it (unless another thread is).
                    if SPARE.lock().unwrap().is_none() || SPARE_KEEPER.swap(true, Ordering::SeqCst) {
                        break;
                    }
                }
            }
        });
    }
}

/// The waiting yt-dlp, if it was started with `args` and is still running.
fn take_spare(args: &[String]) -> Option<Child> {
    let mut s = SPARE.lock().unwrap().take()?;
    if s.args == args && s.started.elapsed() < SPARE_MAX_AGE && matches!(s.child.try_wait(), Ok(None)) {
        return Some(s.child);
    }
    let _ = s.child.kill();
    let _ = s.child.wait();
    None
}

fn discard_spare() {
    let spare = SPARE.lock().unwrap().take();
    if let Some(mut s) = spare {
        let _ = s.child.kill();
        let _ = s.child.wait();
    }
}

/// Hands a waiting yt-dlp its link and waits for its answer.
fn finish_spare(mut child: Child, url: &str, timeout: Duration) -> Result<String, String> {
    use std::io::Write;
    if let Some(mut input) = child.stdin.take() {
        let _ = writeln!(input, "{url}");
        // Dropping it closes the input: that's yt-dlp's cue to start.
    }
    finish(child, timeout)
}

/// Marks yt-dlp as being replaced until dropped, with any spare copy stopped first
/// (on Windows a running copy keeps its folder from being swapped).
struct Installing;

impl Installing {
    fn start() -> Self {
        INSTALLING.store(true, Ordering::SeqCst);
        discard_spare();
        Installing
    }
}

impl Drop for Installing {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::SeqCst);
    }
}

/// The installed yt-dlp and Deno versions, for Diagnostics.
pub fn tool_versions() -> (String, String) {
    use crate::i18n::tr;
    // While they're being installed or updated, running them could catch a half-written file.
    let Ok(_guard) = SETUP.try_lock() else {
        let busy = tr("Being set up or updated right now").to_string();
        return (busy.clone(), busy);
    };
    let tools = installed_tools();
    let version = |exe: &Path, prefix: &str| {
        if !exe.is_file() {
            return tr("Not installed yet (downloaded the first time you play a song)").to_string();
        }
        match run(exe, &["--version".into()], Duration::from_secs(30)) {
            Ok(out) => out.lines().next().unwrap_or("").trim().trim_start_matches(prefix).to_string(),
            Err(e) => format!("{} ({e})", tr("Installed, but it doesn't start")),
        }
    };
    (version(&tools.ytdlp, ""), version(&tools.deno, "deno "))
}

/// Turns yt-dlp's error output into something a person can act on.
fn explain_error(stderr: &str) -> String {
    let last = stderr.lines().rev().find(|l| l.contains("ERROR")).unwrap_or_else(|| stderr.lines().last().unwrap_or(""));
    let lower = stderr.to_lowercase();
    if lower.contains("confirm you") && lower.contains("bot") {
        "YouTube wants to check you're not a bot. Try again in a little while.".into()
    } else if lower.contains("video unavailable") || lower.contains("not available in your country") {
        "This song isn't available on YouTube where you are.".into()
    } else if lower.contains("sign in to confirm your age") {
        "YouTube only plays this song for signed-in adults, and no other version of it was found.".into()
    } else if lower.contains("unable to download") || lower.contains("timed out") || lower.contains("getaddrinfo") {
        "Couldn't reach YouTube. Check your internet connection.".into()
    } else {
        let msg = last.trim().trim_start_matches("ERROR:").trim();
        if msg.is_empty() { "YouTube playback failed.".into() } else { format!("YouTube playback failed: {msg}") }
    }
}

pub(crate) fn base_args(tools: &Tools) -> Vec<String> {
    vec![
        "--ignore-config".into(),
        "--no-warnings".into(),
        "--no-progress".into(),
        "--js-runtimes".into(),
        format!("deno:{}", tools.deno.display()),
        "--cache-dir".into(),
        tools.dir.join("cache").display().to_string(),
    ]
}

// ---------------------------------------------------------------------------
// Finding the right video
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SearchEntry {
    pub id: String,
    pub title: String,
    pub duration: Option<f64>,
    pub channel: Option<String>,
    pub uploader: Option<String>,
    pub view_count: Option<u64>,
    /// From YouTube Music only: whether this is the explicit (uncensored) version.
    #[serde(skip)]
    pub explicit: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SearchResult {
    /// yt-dlp puts `null` here for results it couldn't read.
    entries: Vec<Option<SearchEntry>>,
}

/// Lowercases and keeps only letters and digits, so "Tengo Algo Para Ti!" and
/// "tengo algo para ti" compare equal.
/// Accents are dropped and "$" read as "s", so "JAŸ-Z" = "JAY-Z" and "Ke$ha" = "Kesha".
pub fn norm(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            '$' => 's',
            'à'..='å' => 'a',
            'ç' => 'c',
            'è'..='ë' => 'e',
            'ì'..='ï' => 'i',
            'ñ' => 'n',
            'ò'..='ö' | 'ø' => 'o',
            'ù'..='ü' => 'u',
            'ý' | 'ÿ' => 'y',
            c => c,
        })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// The song title without "(feat. …)" / "(with …)" and similar extras.
pub fn core_title(title: &str) -> String {
    let lower = title.to_ascii_lowercase();
    let cut = ["(feat", "(ft.", "(with ", "[feat", " feat.", " ft. "].iter().filter_map(|m| lower.find(m)).min();
    match cut {
        Some(i) => title[..i].trim().to_string(),
        None => title.trim().to_string(),
    }
}

/// Words that mark a version people didn't ask for (unless the song's own title says so).
pub const UNWANTED: [&str; 26] = [
    "live",
    "cover",
    "remix",
    "sped up",
    "slowed",
    "reverb",
    "nightcore",
    "8d",
    "karaoke",
    "instrumental",
    "1 hour",
    "loop",
    "tribute",
    "piano version",
    "acapella",
    "a cappella",
    "lofi",
    "bass boosted",
    "type beat",
    "parody",
    "reaction",
    "mix",
    "rework",
    "extended",
    "made famous",
    "originally performed",
];

/// True if `candidate` is a version (cover, instrumental, sped up…) that `wanted` doesn't ask for.
pub fn unwanted_version(wanted: &str, candidate: &str) -> bool {
    let (w, c) = (words(wanted), words(candidate));
    UNWANTED.iter().any(|bad| {
        let b = format!(" {bad} ");
        c.contains(&b) && !w.contains(&b)
    })
}

/// " word word ": lowercase whole words, so " live " doesn't match "alive".
pub fn words(s: &str) -> String {
    format!(
        " {} ",
        s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

/// The title to look for in a result: without "(feat. …)", "(Remastered)" or "- Radio Edit"
/// style extras, which YouTube's own titles often leave out.
fn gate_title(title: &str) -> String {
    let mut plain = String::new();
    let mut depth = 0;
    for c in core_title(title).chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            c if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let plain = plain.split(" - ").next().unwrap_or("");
    norm(plain)
}

/// Does a result's title name this song? Right length alone isn't enough: another song of
/// the same length must not win. A censored title ("F***in") still counts when what's
/// left of it fits the song's title in order.
pub fn title_matches(song_title: &str, result_title: &str) -> bool {
    let want = gate_title(song_title);
    if want.is_empty() {
        return true;
    }
    let got = norm(result_title);
    if got.contains(&want) {
        return true;
    }
    // The result may be the short form of a longer song title ("Song (Part 2)" -> "Song").
    let short = gate_title(result_title);
    if short.len() >= 3 && want.contains(&short) && short.len() * 10 >= want.len() * 7 {
        return true;
    }
    // A spelling variant ("Fuckin" and "fuckn"): one small slip in a long enough title.
    if want.len() >= 8 && short.len() >= 8 && edit_distance(&want, &short) <= want.len() / 8 {
        return true;
    }
    if result_title.contains('*') && short.len() * 10 >= want.len() * 6 {
        let mut rest = want.chars();
        return short.chars().all(|c| rest.any(|w| w == c));
    }
    false
}

/// How many single-letter changes turn `a` into `b`.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = (diag + (ca != *cb) as usize).min(up + 1).min(row[j] + 1);
            diag = up;
        }
    }
    row[b.len()]
}

/// Scores how well a YouTube result matches the song. Higher is better; below 0 it
/// isn't the song. What counts, strongest first: the right length (a music video or a
/// 30-second clip is the wrong length), the song's own artist (an "Artist - Topic"
/// channel is the official album audio, but only if it's *this* artist's), the title,
/// and no unwanted version (cover, instrumental, live, sped up…) in the title or channel.
pub fn score(track: &Track, e: &SearchEntry) -> i32 {
    let mut s = 0;
    let title = e.title.to_lowercase();
    let channel = e.channel.as_deref().or(e.uploader.as_deref()).unwrap_or("").to_lowercase();
    let want_title = norm(&core_title(&track.title));
    // The main artist and anyone featured: the right channel may be any of them.
    let artists: Vec<String> = std::iter::once(&track.artist).chain(track.featured.iter()).map(|a| norm(&a.name)).filter(|a| !a.is_empty()).collect();
    let by_artist = |text: &str| {
        let t = norm(text);
        artists.iter().any(|a| t.contains(a.as_str()) || (t.len() >= 4 && a.contains(&t)))
    };
    let topic = channel.strip_suffix(" - topic");

    // Length is the strongest signal: music videos and live versions run long, clips short.
    // Deezer lists some songs as a 30-second clip; that isn't the song's real length.
    if let Some(d) = e.duration
        && track.duration >= 60
    {
        let diff = (d - track.duration as f64).abs();
        s += match diff {
            x if x <= 3.0 => 40,
            x if x <= 8.0 => 25,
            x if x <= 20.0 => 5,
            x if x <= 45.0 => -15,
            _ => -40,
        };
        // Much shorter than the song: a preview, a short or a cut.
        if d < track.duration as f64 * 0.7 {
            s -= 30;
        }
    }
    match topic {
        // "Artist - Topic" carries the official album audio, if it's this artist's.
        Some(name) if artists.is_empty() || by_artist(name) => s += 35,
        // Another artist's album audio of the same title is almost always a cover.
        Some(_) => s -= 80,
        None => {}
    }
    if !artists.is_empty() {
        if topic.is_none() && by_artist(&channel) {
            s += 20;
        } else if by_artist(&title) {
            s += 12;
        } else if topic.is_none() {
            // Someone else's upload that doesn't even name the artist.
            s -= 25;
        }
    }
    if !want_title.is_empty() && norm(&title).contains(&want_title) {
        s += 25;
    }
    // A different song with the right length and artist is still the wrong song.
    if !title_matches(&track.title, &e.title) {
        s -= 100;
    }
    if title.contains("official audio") || (title.contains("audio") && !title.contains("video")) {
        s += 8;
    }
    // The music video: often intros, skits or a different edit. The song itself is better.
    if is_video_version(&title) && !is_video_version(&track.title.to_lowercase()) {
        s -= 25;
    }
    let (t_words, c_words, song) = (words(&e.title), words(&channel), words(&track.title));
    for bad in UNWANTED {
        let w = format!(" {bad} ");
        if !song.contains(&w) && (t_words.contains(&w) || c_words.contains(&w)) {
            s -= 150;
        }
    }
    if title.contains("lyrics") || title.contains("lyric video") {
        s -= 2;
    }
    // A song with swearing: the uncensored or the clean version, as Settings asks.
    if track.explicit {
        let clean = ["clean", "censored", "radio edit", "radio version", "clean version"]
            .iter()
            .any(|w| t_words.contains(&format!(" {w} ")) && !song.contains(&format!(" {w} ")));
        let dirty = ["explicit", "uncensored", "dirty"].iter().any(|w| t_words.contains(&format!(" {w} ")));
        s += match (crate::deezer::prefer_explicit(), clean, dirty) {
            (true, true, _) => -30,
            (true, false, true) => 5,
            (false, true, _) => 20,
            (false, false, true) => -15,
            _ => 0,
        };
    }
    // YouTube Music marks the explicit version. The clean one often has the very same
    // title, length and channel, so this is the only way to tell them apart.
    s += match (e.explicit, crate::deezer::prefer_explicit()) {
        (Some(true), true) => 12,
        (Some(true), false) => -20,
        _ => 0,
    };
    s
}

/// All results that look like the song, best first. Empty if nothing fits.
pub fn rank<'a>(track: &Track, entries: &'a [SearchEntry]) -> Vec<&'a SearchEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut scored: Vec<(i32, &SearchEntry)> =
        entries.iter().filter(|e| !e.id.is_empty() && seen.insert(e.id.as_str())).map(|e| (score(track, e), e)).filter(|(s, _)| *s >= 0).collect();
    // Music-video versions only when no audio version fits: they often have an intro or skit.
    let video = |e: &SearchEntry| is_video_version(&e.title.to_lowercase()) && !is_video_version(&track.title.to_lowercase());
    scored.sort_by_key(|(s, e)| (video(e), std::cmp::Reverse(*s)));
    scored.into_iter().map(|(_, e)| e).collect()
}

/// A music video, visualizer or similar: not the plain audio release.
fn is_video_version(title_lower: &str) -> bool {
    ["official video", "music video", "official mv", "(video)", "[video]", "visualizer", "official music video"].iter().any(|m| title_lower.contains(m))
}

/// Picks the best match, or None if nothing looks like the song.
#[cfg(test)]
pub fn pick_best<'a>(track: &Track, entries: &'a [SearchEntry]) -> Option<&'a SearchEntry> {
    rank(track, entries).into_iter().next()
}

/// Runs one flat yt-dlp search (a `ytsearchN:` query or a search page URL).
fn search(tools: &Tools, target: String, limit: u32) -> Result<Vec<SearchEntry>, String> {
    let mut args = base_args(tools);
    args.extend(["--flat-playlist".into(), "-J".into(), "--playlist-end".into(), limit.to_string(), target]);
    let out = run(&tools.ytdlp, &args, Duration::from_secs(40))?;
    let result: SearchResult = serde_json::from_str(&out).map_err(|e| format!("Unexpected YouTube search result ({e})"))?;
    Ok(result.entries.into_iter().flatten().collect())
}

/// Searches YouTube Music's songs directly. Faster than going through yt-dlp, and it
/// says how long each song is, who it's by and whether it's the explicit version.
fn music_search(query: &str) -> Result<Vec<SearchEntry>, String> {
    // "params" picks the Songs tab.
    let body = serde_json::json!({
        "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20241106.01.00", "hl": "en" } },
        "query": query,
        "params": "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D",
    });
    let mut resp = crate::net::api()
        .post("https://music.youtube.com/youtubei/v1/search?prettyPrint=false")
        .header("Origin", "https://music.youtube.com")
        .send_json(&body)
        .map_err(crate::net::friendly_error)?;
    let reply: serde_json::Value =
        resp.body_mut().with_config().limit(8 * 1024 * 1024).read_json().map_err(|e| format!("Unexpected YouTube Music reply ({e})"))?;
    Ok(music_entries(&reply))
}

/// Reads the songs out of a YouTube Music search reply.
fn music_entries(reply: &serde_json::Value) -> Vec<SearchEntry> {
    fn walk<'a>(v: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(item) = m.get("musicResponsiveListItemRenderer") {
                    out.push(item);
                }
                m.values().for_each(|x| walk(x, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut items = Vec::new();
    walk(reply, &mut items);
    let column = |item: &serde_json::Value, i: usize| -> String {
        item["flexColumns"][i]["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"]
            .as_array()
            .map(|runs| runs.iter().filter_map(|r| r["text"].as_str()).collect())
            .unwrap_or_default()
    };
    items
        .into_iter()
        .filter_map(|item| {
            let id = item["playlistItemData"]["videoId"].as_str()?.to_string();
            let title = column(item, 0);
            // "Artist & Artist • Album • 2:36"
            let info = column(item, 1);
            let parts: Vec<&str> = info.split(" • ").collect();
            let duration = parts.last().and_then(|d| parse_clock(d));
            let artists = parts.first().filter(|_| parts.len() > 1).map(|a| a.trim().to_string()).unwrap_or_default();
            let explicit = item.to_string().contains("MUSIC_EXPLICIT_BADGE");
            Some(SearchEntry {
                id,
                title,
                duration,
                // YouTube Music's songs are the album audio, the same as an "Artist - Topic" upload.
                channel: (!artists.is_empty()).then(|| format!("{artists} - Topic")),
                explicit: Some(explicit),
                ..Default::default()
            })
        })
        .collect()
}

/// Searches YouTube the way its own web page does: one request, the same results a
/// `ytsearchN:` search through yt-dlp reads, but in a fraction of a second and without
/// starting a yt-dlp (which takes about a second of CPU each time). If YouTube's reply
/// ever changes shape, yt-dlp's search takes over.
fn web_search(tools: Option<&Tools>, query: &str, limit: usize) -> Result<Vec<SearchEntry>, String> {
    let direct = (|| {
        let body = serde_json::json!({
            "context": { "client": { "clientName": "WEB", "clientVersion": "2.20241106.01.00", "hl": "en", "gl": "US" } },
            "query": query,
        });
        let mut resp = crate::net::api()
            .post("https://www.youtube.com/youtubei/v1/search?prettyPrint=false")
            .header("Origin", "https://www.youtube.com")
            .send_json(&body)
            .map_err(crate::net::friendly_error)?;
        let reply: serde_json::Value =
            resp.body_mut().with_config().limit(8 * 1024 * 1024).read_json().map_err(|e| format!("Unexpected YouTube reply ({e})"))?;
        let entries = web_entries(&reply, limit);
        if entries.is_empty() { Err("YouTube's search reply had no videos".to_string()) } else { Ok(entries) }
    })();
    match (direct, tools) {
        (Ok(entries), _) => Ok(entries),
        (Err(_), Some(tools)) => search(tools, format!("ytsearch{limit}:{query}"), limit as u32),
        (Err(e), None) => Err(e),
    }
}

/// Reads the videos out of a YouTube search reply, in order.
fn web_entries(reply: &serde_json::Value, limit: usize) -> Vec<SearchEntry> {
    fn walk<'a>(v: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
        match v {
            serde_json::Value::Object(m) => {
                // Ordinary videos only (Shorts, playlists and channels have other renderers).
                if let Some(video) = m.get("videoRenderer") {
                    out.push(video);
                    return;
                }
                m.values().for_each(|x| walk(x, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let text = |v: &serde_json::Value| -> Option<String> {
        v["simpleText"].as_str().map(str::to_string).or_else(|| v["runs"].as_array().map(|runs| runs.iter().filter_map(|r| r["text"].as_str()).collect()))
    };
    let mut videos = Vec::new();
    walk(reply, &mut videos);
    videos
        .into_iter()
        .filter_map(|v| {
            let id = v["videoId"].as_str()?.to_string();
            let channel = text(&v["ownerText"]).or_else(|| text(&v["longBylineText"]));
            // "1,234,567 views"; "No views"; a live stream says how many are watching instead.
            let views = text(&v["viewCountText"]).filter(|t| t.contains("view")).map(|t| t.chars().filter(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0));
            Some(SearchEntry {
                id,
                title: text(&v["title"]).unwrap_or_default(),
                duration: text(&v["lengthText"]).and_then(|d| parse_clock(&d)),
                uploader: channel.clone(),
                channel,
                view_count: views,
                explicit: None,
            })
        })
        .take(limit)
        .collect()
}

/// "2:36" or "1:02:03" in seconds.
fn parse_clock(s: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in s.trim().split(':') {
        total = total * 60.0 + part.parse::<u32>().ok()? as f64;
    }
    s.contains(':').then_some(total)
}

/// The two YouTube searches Pulse runs for a song: one with the title in quotes
/// (YouTube then only returns videos that contain it) and one plain.
fn queries(track: &Track) -> [String; 2] {
    let title = core_title(&track.title).replace('"', "");
    [format!("{} \"{title}\"", track.artist.name), format!("{} - {title}", track.artist.name)]
}

/// Searches YouTube and returns the ids of the videos that match `track`, best first.
/// Three searches at once (as long as one): YouTube Music's songs (official album
/// audio), and two ordinary searches for songs YouTube Music doesn't have.
pub fn find_videos(tools: &Tools, track: &Track) -> Result<Vec<String>, String> {
    // A song fix can name the right video (for songs that keep finding the wrong one).
    if let Some(video) = crate::fixes::video(&track.id) {
        return Ok(vec![video]);
    }
    let searched = Instant::now();
    let [quoted, plain] = queries(track);
    let music_url = format!("https://music.youtube.com/search?q={}#songs", crate::net::encode(&plain));
    let (tx, rx) = std::sync::mpsc::channel::<(bool, Result<Vec<SearchEntry>, String>)>();
    // YouTube Music directly; through yt-dlp if that fails.
    {
        let (tx, tools, plain) = (tx.clone(), tools.clone(), plain.clone());
        std::thread::spawn(move || {
            let found = music_search(&plain).ok().filter(|e| !e.is_empty()).map(Ok).unwrap_or_else(|| search(&tools, music_url, 6));
            let _ = tx.send((true, found));
        });
    }
    for query in [quoted, plain] {
        let (tx, tools) = (tx.clone(), tools.clone());
        std::thread::spawn(move || {
            let _ = tx.send((false, web_search(Some(&tools), &query, 8)));
        });
    }
    drop(tx);
    let mut entries = Vec::new();
    let mut error = None;
    // Three searches at once. The song's own album audio from YouTube Music, a clear
    // match, is used as soon as it's found, without waiting for the others: that's
    // most songs, and it saves a second or two before the music starts.
    while let Ok((is_music, r)) = rx.recv_timeout(Duration::from_secs(40)) {
        match r {
            Ok(mut e) => {
                entries.append(&mut e);
                if is_music && rank(track, &entries).first().is_some_and(|best| clear_match(track, best)) {
                    break;
                }
            }
            Err(e) => error = Some(e),
        }
    }
    let ids: Vec<String> = rank(track, &entries).into_iter().map(|e| e.id.clone()).collect();
    crate::audio::trace(&format!("search: {} results, best {}, in {} ms", entries.len(), ids.first().map(String::as_str).unwrap_or("none"), searched.elapsed().as_millis()));
    match (ids.is_empty(), error) {
        (false, _) => Ok(ids),
        (true, Some(e)) if entries.is_empty() => Err(e),
        _ => Err("Couldn't find this song on YouTube.".to_string()),
    }
}

/// The song's album audio on YouTube Music, if one clearly is the song (for autoplay,
/// which starts from YouTube Music's radio for it). One quick request; no yt-dlp.
pub fn music_video(track: &Track) -> Option<String> {
    if let Some(video) = crate::fixes::video(&track.id) {
        return Some(video);
    }
    let [_, plain] = queries(track);
    let entries = music_search(&plain).ok()?;
    let ranked = rank(track, &entries);
    // A clear match, or else the best album audio by the artist: Deezer's title may carry
    // extras YouTube Music's doesn't ("Enter Sandman (Remastered 2021)"), and for a radio
    // the right song in another edition is just as good.
    let topic = |e: &&SearchEntry| e.channel.as_deref().is_some_and(|c| c.to_lowercase().ends_with(" - topic"));
    ranked.iter().find(|e| clear_match(track, e)).or_else(|| ranked.iter().find(|e| topic(e) && score(track, e) >= 40)).map(|e| e.id.clone())
}

/// Only the right song gets this: the artist's own album audio ("Artist - Topic") with the
/// right title, and the right length (or YouTube Music didn't say how long it is).
fn clear_match(track: &Track, e: &SearchEntry) -> bool {
    let topic = e.channel.as_deref().or(e.uploader.as_deref()).unwrap_or("").to_lowercase().ends_with(" - topic");
    let s = score(track, e);
    topic && (s >= 95 || (e.duration.is_none() && s >= 55))
}

/// True once yt-dlp is installed (view counts don't start the first-time setup).
pub fn tools_ready() -> bool {
    let dir = tools_dir();
    dir.join(YTDLP.save_as).is_file() && dir.join(DENO.save_as).is_file()
}

/// The YouTube video Pulse would play for `track`, and its view count. A direct search, so
/// a page full of songs no longer starts a yt-dlp for each one (they used to run three at
/// a time for a minute or more, slowing the song you'd just pressed).
pub fn views(track: &Track) -> Result<Option<(String, u64)>, String> {
    if !tools_ready() {
        return Ok(None);
    }
    let tools = installed_tools();
    let [_, plain] = queries(track);
    let entries = web_search(Some(&tools), &plain, 6)?;
    Ok(rank(track, &entries).into_iter().find_map(|e| e.view_count.map(|v| (e.id.clone(), v))))
}

/// The videos in a YouTube (or YouTube Music) playlist: its title and entries.
pub fn playlist(url: &str, status: &dyn Fn(&str)) -> Result<(String, Vec<SearchEntry>), String> {
    let tools = ensure_tools(status)?;
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Playlist {
        title: Option<String>,
        entries: Vec<Option<SearchEntry>>,
    }
    let mut args = base_args(&tools);
    args.extend(["--flat-playlist".into(), "-J".into(), "--playlist-end".into(), "5000".into(), url.to_string()]);
    let out = run(&tools.ytdlp, &args, Duration::from_secs(300))?;
    let p: Playlist = serde_json::from_str(&out).map_err(|e| format!("Unexpected YouTube reply ({e})"))?;
    let entries: Vec<SearchEntry> = p.entries.into_iter().flatten().filter(|e| !e.id.is_empty()).collect();
    Ok((p.title.unwrap_or_else(|| "YouTube playlist".into()), entries))
}

// ---------------------------------------------------------------------------
// Getting the audio link
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct Format {
    url: Option<String>,
    ext: Option<String>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
    http_headers: std::collections::HashMap<String, String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct VideoInfo {
    #[serde(flatten)]
    format: Format,
    requested_downloads: Vec<Format>,
    /// Unix time when a fresh link starts working (YouTube sometimes delays it).
    available_at: Option<i64>,
    /// The video's length in seconds.
    duration: Option<f64>,
    title: Option<String>,
}

/// How long to wait before a link with `available_at` works. Errors if it's far off.
fn wait_for(available_at: Option<i64>, now: i64) -> Result<Duration, String> {
    match available_at {
        Some(t) if t > now + 30 => Err("YouTube won't release this song's audio yet. Try again in a minute.".into()),
        Some(t) if t > now => Ok(Duration::from_secs((t - now) as u64)),
        _ => Ok(Duration::ZERO),
    }
}

/// Turns yt-dlp's video JSON into something the audio engine can play.
pub(crate) fn source_from_info(json: &str) -> Result<StreamSource, String> {
    let info: VideoInfo = serde_json::from_str(json).map_err(|e| format!("Unexpected YouTube reply ({e})"))?;
    let wait = wait_for(info.available_at, now_secs() as i64)?;
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
    let f = if info.format.url.is_some() { info.format } else { info.requested_downloads.into_iter().next().unwrap_or_default() };
    let url = f.url.ok_or("YouTube didn't give an audio link for this song.")?;
    let codec = match f.ext.as_deref() {
        Some("m4a") | Some("mp4") => Codec::Mp4,
        Some("mp3") => Codec::Mp3,
        other => return Err(format!("YouTube only has this song in a format Pulse can't play yet ({}).", other.unwrap_or("unknown"))),
    };
    let headers = f.http_headers.into_iter().filter(|(k, _)| !k.eq_ignore_ascii_case("accept-encoding")).collect();
    Ok(StreamSource {
        url,
        headers,
        codec,
        content_length: f.filesize.or(f.filesize_approx),
        notice: None,
        duration: info.duration.filter(|d| *d > 0.0 && d.is_finite()).map(Duration::from_secs_f64),
        keep_as: None,
    })
}

/// MEASUREMENT ONLY (for the playback log): how long yt-dlp and Deno take just to start,
/// once per session, on a background thread. Tells how much of a ~1.8 s lookup is
/// starting the programs (which looking up several songs in one run would save).
fn measure_startup_once(tools: &Tools) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let tools = tools.clone();
    ONCE.call_once(move || {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5)); // not while the first song starts
            for (name, program) in [("yt-dlp", &tools.ytdlp), ("Deno", &tools.deno)] {
                let t = Instant::now();
                let ok = run(program, &["--version".to_string()], Duration::from_secs(30)).is_ok();
                crate::audio::trace(&format!("measure: {name} --version took {} ms ({})", t.elapsed().as_millis(), if ok { "ok" } else { "failed" }));
            }
        });
    });
}

/// The video id in a YouTube link (youtube.com/watch?v=…, music.youtube.com, youtu.be/…,
/// /shorts/…), if `text` is one. Ids are 11 letters, digits, `-` or `_`.
pub fn video_id_in_link(text: &str) -> Option<String> {
    let t = text.trim();
    let lower = t.to_lowercase();
    if !(lower.contains("youtube.com/") || lower.contains("youtu.be/")) || t.contains(char::is_whitespace) {
        return None;
    }
    let after = ["v=", "youtu.be/", "/shorts/", "/live/"].iter().find_map(|m| t.find(m).map(|i| &t[i + m.len()..]))?;
    let id: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    (id.len() == 11).then_some(id)
}

/// A YouTube video's title and channel, from YouTube's public oEmbed page (no sign-in,
/// no yt-dlp run). Used to search Pulse's catalog when a video link is pasted in search.
pub fn video_title(video_id: &str) -> Result<(String, String), String> {
    #[derive(Deserialize)]
    struct OEmbed {
        title: String,
        #[serde(default)]
        author_name: String,
    }
    let url = format!("https://www.youtube.com/oembed?format=json&url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3D{video_id}");
    let o: OEmbed = serde_json::from_str(&crate::net::get_text(&url)?).map_err(|e| format!("Unexpected YouTube reply ({e})"))?;
    Ok((o.title, o.author_name))
}

/// EXPERIMENT ONLY: yt-dlp run like `run`, but every stderr line is timed as it arrives
/// (with `-v` that shows each stage: start-up, web page, player, JS challenge, API), and
/// the process's own CPU time is read when it ends (Windows; Deno's isn't included).
struct Measured {
    out: Result<String, String>,
    /// (ms after the start, line)
    lines: Vec<(u128, String)>,
    total_ms: u128,
    cpu_ms: Option<u64>,
}

fn run_measured(program: &Path, args: &[String]) -> Measured {
    let t = Instant::now();
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Measured { out: Err(e.to_string()), lines: vec![], total_ms: 0, cpu_ms: None },
    };
    let mut out_pipe = child.stdout.take().unwrap();
    let err_pipe = child.stderr.take().unwrap();
    let out_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut out_pipe, &mut s);
        s
    });
    let err_t = std::thread::spawn(move || {
        use std::io::BufRead;
        std::io::BufReader::new(err_pipe).lines().map_while(Result::ok).map(|l| (t.elapsed().as_millis(), l)).collect::<Vec<_>>()
    });
    let status = child.wait();
    let total_ms = t.elapsed().as_millis();
    let cpu_ms = process_cpu_ms(&child);
    let out = out_t.join().unwrap_or_default();
    let lines = err_t.join().unwrap_or_default();
    let ok = status.map(|s| s.success()).unwrap_or(false);
    Measured { out: if ok { Ok(out) } else { Err(lines.last().map(|l| l.1.clone()).unwrap_or_default()) }, lines, total_ms, cpu_ms }
}

/// CPU time (user + kernel) a finished child process used, in ms.
#[cfg(windows)]
fn process_cpu_ms(child: &std::process::Child) -> Option<u64> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime(u32, u32);
    unsafe extern "system" {
        fn GetProcessTimes(process: *mut std::ffi::c_void, created: *mut FileTime, exited: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
    }
    let (mut c, mut e, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
    let ok = unsafe { GetProcessTimes(child.as_raw_handle(), &mut c, &mut e, &mut k, &mut u) };
    let ticks = |f: &FileTime| ((f.1 as u64) << 32) | f.0 as u64;
    (ok != 0).then(|| (ticks(&k) + ticks(&u)) / 10_000)
}

#[cfg(not(windows))]
fn process_cpu_ms(_child: &std::process::Child) -> Option<u64> {
    None
}

/// The stage lines worth logging from `-v` output: everything but routine debug lines,
/// plus the first debug line (start-up finished) and the JS challenge ones.
fn stage_lines(lines: &[(u128, String)]) -> Vec<String> {
    let mut out = vec![];
    for (i, (ms, l)) in lines.iter().enumerate() {
        let keep = i == 0 || !l.starts_with("[debug] ") || l.contains("jsc") || l.contains("JS ") || l.contains("Loaded") || l.contains("challenge");
        if keep && out.len() < 30 {
            out.push(format!("{ms:>5} ms  {}", l.chars().take(140).collect::<String>()));
        }
    }
    out
}

/// Recently looked-up video ids, for the batch measurement.
static RECENT_VIDEOS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// EXPERIMENT ONLY, off unless the file `logs\ytdlp-experiment.on` exists in the data
/// folder. After a real lookup (at most once a minute), on a background thread, one at
/// a time:
/// 1. the same lookup with `-v`: when each stage happens, and the CPU it used;
/// 2. the same video with other YouTube clients: time, result, and whether the link
///    answers a range request like the normal one (so the streaming would work);
/// 3. three recent videos plus one that doesn't exist, in ONE yt-dlp run (`-j -i`):
///    when each result arrives, total time and CPU, and that the bad one fails alone.
///
/// The song always plays from the normal lookup; nothing here changes what Pulse uses.
fn experiment_after(tools: &Tools, video_id: &str, quality: Quality) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut recent = RECENT_VIDEOS.lock().unwrap_or_else(|e| e.into_inner());
        recent.retain(|v| v != video_id);
        recent.insert(0, video_id.to_string());
        recent.truncate(6);
    }
    if !crate::settings::data_dir().join("logs").join("ytdlp-experiment.on").is_file() {
        return;
    }
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
            return;
        }
        *last = Some(Instant::now());
    }
    let (tools, video_id) = (tools.clone(), video_id.to_string());
    std::thread::spawn(move || {
        let trace = |m: String| crate::audio::trace(&format!("experiment: {m}"));
        let lookup_args = |clients: &str, extra: &[&str], targets: &[String]| {
            let mut args = base_args(&tools);
            args.extend(extra.iter().map(|s| s.to_string()));
            args.extend(["--no-playlist".into(), "-f".into(), quality.ytdlp_format().into(), "--extractor-args".into(), format!("youtube:player_client={clients}")]);
            args.extend(targets.iter().map(|v| format!("https://www.youtube.com/watch?v={v}")));
            args
        };
        // 1. Stages of the normal lookup.
        let m = run_measured(&tools.ytdlp, &lookup_args("default,-tv_samsung", &["-J", "-v"], std::slice::from_ref(&video_id)));
        trace(format!("{video_id} normal lookup with -v: {} ms total, yt-dlp CPU {} ms, {}", m.total_ms, m.cpu_ms.map(|c| c.to_string()).unwrap_or("?".into()), if m.out.is_ok() { "ok" } else { "failed" }));
        for l in stage_lines(&m.lines) {
            trace(format!("  {l}"));
        }
        // 2. Other clients.
        for clients in ["android_vr", "tv_simply", "web_music", "ios"] {
            let m = run_measured(&tools.ytdlp, &lookup_args(clients, &["-J"], std::slice::from_ref(&video_id)));
            let what = match m.out.and_then(|out| source_from_info(&out)) {
                Ok(src) => {
                    let ranges = match crate::net::get_range(&src.url, &src.headers, 0, Some(1023)) {
                        Ok(r) if r.partial => "range request ok",
                        Ok(_) => "no range support",
                        Err(_) => "link didn't answer",
                    };
                    format!("ok, {:?}, {} KB, {ranges}", src.codec, src.content_length.unwrap_or(0) / 1024)
                }
                Err(e) => format!("failed: {}", e.chars().take(100).collect::<String>()),
            };
            trace(format!("{video_id} with client {clients}: {} ms, CPU {} ms, {what}", m.total_ms, m.cpu_ms.map(|c| c.to_string()).unwrap_or("?".into())));
        }
        // 3. Several videos in one run, one of them made up.
        let mut batch: Vec<String> = RECENT_VIDEOS.lock().unwrap_or_else(|e| e.into_inner()).iter().take(3).cloned().collect();
        batch.push("aaaaaaaaaaa".into());
        let t = Instant::now();
        let args = lookup_args("default,-tv_samsung", &["-j", "-i"], &batch);
        // stdout lines are timed by reading them as they come.
        let mut cmd = Command::new(&tools.ytdlp);
        cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let Ok(mut child) = cmd.spawn() else { return };
        let out = child.stdout.take().unwrap();
        let err = child.stderr.take().unwrap();
        let err_t = std::thread::spawn(move || {
            use std::io::BufRead;
            std::io::BufReader::new(err).lines().map_while(Result::ok).filter(|l| l.starts_with("ERROR")).collect::<Vec<_>>()
        });
        {
            use std::io::BufRead;
            for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                let id = serde_json::from_str::<serde_json::Value>(&line).ok().and_then(|v| v.get("id").and_then(|i| i.as_str()).map(str::to_string)).unwrap_or("?".into());
                let usable = source_from_info(&line).is_ok();
                trace(format!("batch: {id} arrived at {} ms ({})", t.elapsed().as_millis(), if usable { "usable link" } else { "not usable" }));
            }
        }
        let _ = child.wait();
        let cpu = process_cpu_ms(&child);
        let errors = err_t.join().unwrap_or_default();
        trace(format!(
            "batch of {} ({} real + 1 made up): {} ms total, CPU {} ms, {} error line(s): {}",
            batch.len(),
            batch.len() - 1,
            t.elapsed().as_millis(),
            cpu.map(|c| c.to_string()).unwrap_or("?".into()),
            errors.len(),
            errors.first().map(|e| e.chars().take(100).collect::<String>()).unwrap_or_default()
        ));
    });
}

/// yt-dlp's arguments for a song's audio link, all but the link itself.
fn link_args(tools: &Tools, quality: Quality) -> Vec<String> {
    let mut args = base_args(tools);
    args.extend([
        "--no-playlist".into(),
        "-J".into(),
        "-f".into(),
        quality.ytdlp_format().into(),
        // Skip a YouTube client that often hands out broken links, and the HLS and DASH
        // stream lists: Pulse plays the plain audio file, and fetching those lists made
        // every song wait about 0.8 s longer.
        "--extractor-args".into(),
        "youtube:player_client=default,-tv_samsung;skip=hls,dash".into(),
    ]);
    args
}

#[cfg(test)]
fn audio_source(tools: &Tools, video_id: &str, quality: Quality) -> Result<StreamSource, String> {
    audio_source_for(tools, video_id, quality, None)
}

/// Does what yt-dlp found fit the song about to be shown? A video with another title is the
/// wrong one even when its length is close (a saved link to another song on the same album,
/// say): better to look again than to play it.
pub(crate) fn is_wrong_video(track: &Track, json: &str) -> bool {
    let Ok(info) = serde_json::from_str::<VideoInfo>(json) else { return false };
    info.title.is_some_and(|title| !title_matches(&track.title, &title))
}

/// Like `audio_source`; with the song, a video that isn't it is refused.
fn audio_source_for(tools: &Tools, video_id: &str, quality: Quality, track: Option<&Track>) -> Result<StreamSource, String> {
    let args = link_args(tools, quality);
    let url = format!("https://www.youtube.com/watch?v={video_id}");
    let t = Instant::now();
    let out = match take_spare(&args) {
        Some(spare) => {
            let out = finish_spare(spare, &url, Duration::from_secs(60));
            // Ready for the next song.
            if spare_wanted() {
                let tools = tools.clone();
                std::thread::spawn(move || start_spare(&tools, args));
            }
            out
        }
        None => {
            let mut args = args;
            args.push(url);
            run(&tools.ytdlp, &args, Duration::from_secs(60))
        }
    };
    crate::audio::trace(&format!("yt-dlp: audio link for {video_id}: {} in {} ms", if out.is_ok() { "ok" } else { "failed" }, t.elapsed().as_millis()));
    let out = out?;
    if track.is_some_and(|t| is_wrong_video(t, &out)) {
        crate::audio::trace(&format!("yt-dlp: {video_id} is not this song, skipped"));
        return Err("That video isn't this song.".into());
    }
    let src = source_from_info(&out)?;
    experiment_after(tools, video_id, quality);
    Ok(src)
}

/// Everything needed to play `track` from YouTube. `known_video` skips the search
/// when Pulse already knows which video it is. Returns the source and the video id.
pub fn resolve(track: &Track, known_video: Option<String>, quality: Quality, status: &dyn Fn(&str)) -> Result<(StreamSource, String), String> {
    let tools = ensure_tools(status)?;
    // A song fix names its video on purpose: that one isn't second-guessed.
    let checked = crate::fixes::video(&track.id).is_none().then_some(track);
    let attempt = |status: &dyn Fn(&str)| -> Result<(StreamSource, String), String> {
        if let Some(id) = &known_video {
            if let Some(url) = crate::soundcloud::url_of(id) {
                status("Loading audio from SoundCloud…");
                if let Ok(src) = crate::soundcloud::open(&tools, url, track) {
                    return Ok((src, id.clone()));
                }
            } else {
                status("Loading audio…");
                if let Ok(src) = audio_source_for(&tools, id, quality, checked) {
                    return Ok((src, id.clone()));
                }
            }
            // The saved video may be gone: search again.
        }
        // With "Prefer SoundCloud" the SoundCloud search runs at the same time as YouTube's,
        // so a song that isn't on SoundCloud costs no extra waiting.
        let on_soundcloud = crate::soundcloud::first().then(|| {
            let (t, tr) = (tools.clone(), track.clone());
            std::thread::spawn(move || crate::soundcloud::find(&t, &tr, 2))
        });
        status("Finding song…");
        let found = find_videos(&tools, track);
        if let Some(url) = on_soundcloud.and_then(|h| h.join().ok().flatten()) {
            status("Loading audio from SoundCloud…");
            match crate::soundcloud::open(&tools, &url, track) {
                Ok(src) => return Ok((src, crate::soundcloud::id_for(&url))),
                Err(e) => crate::audio::trace(&format!("soundcloud: {e}")),
            }
        }
        let from_youtube = || -> Result<(StreamSource, String), String> {
            let ids = found.clone()?;
            status("Loading audio…");
            // If the best video won't play (removed, age-gated, region-locked), try the next ones.
            let mut first_error = None;
            for id in ids.iter().filter(|id| Some(*id) != known_video.as_ref()).take(3) {
                match audio_source_for(&tools, id, quality, checked) {
                    Ok(src) => return Ok((src, id.clone())),
                    Err(e) => {
                        first_error.get_or_insert(e);
                    }
                }
            }
            Err(first_error.unwrap_or_else(|| "Couldn't find this song on YouTube.".into()))
        };
        match from_youtube() {
            // Offline: SoundCloud would only wait and fail too.
            Err(e) if e.starts_with("Couldn't reach") => Err(e),
            // YouTube has nothing that plays: SoundCloud (unless it was already tried), then
            // YouTube's own message if that fails too.
            Err(e) if crate::soundcloud::first() => Err(e),
            Err(e) => {
                status("Trying SoundCloud…");
                crate::soundcloud::find_and_open(&tools, track).map_err(|sc| {
                    crate::audio::trace(&format!("soundcloud: {sc}"));
                    e
                })
            }
            ok => ok,
        }
    };
    match attempt(status) {
        Ok(v) => Ok(v),
        Err(e) if e.starts_with("YouTube playback failed") || e.starts_with("Unexpected") => {
            // Possibly a YouTube change: grab the newest yt-dlp and try once more.
            status("Updating YouTube support…");
            update_now(&tools);
            attempt(status)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod link_tests {
    use super::video_id_in_link;

    #[test]
    fn finds_the_video_in_youtube_links() {
        for link in [
            "https://www.youtube.com/watch?v=LGzannHdxxU",
            "https://music.youtube.com/watch?v=LGzannHdxxU&si=abc",
            "https://youtu.be/LGzannHdxxU?t=30",
            "youtube.com/shorts/LGzannHdxxU",
            "https://www.youtube.com/watch?app=desktop&v=LGzannHdxxU",
        ] {
            assert_eq!(video_id_in_link(link).as_deref(), Some("LGzannHdxxU"), "{link}");
        }
        for not in ["the beths head in the clouds", "https://open.spotify.com/track/x", "https://www.youtube.com/watch?v=short", "https://www.youtube.com/@thebeths"] {
            assert_eq!(video_id_in_link(not), None, "{not}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deezer::ArtistRef;

    fn song(artist: &str, title: &str, secs: u32) -> Track {
        Track { title: title.into(), duration: secs, artist: ArtistRef { name: artist.into(), ..Default::default() }, ..Default::default() }
    }

    fn entry(id: &str, title: &str, secs: f64, channel: &str) -> SearchEntry {
        SearchEntry { id: id.into(), title: title.into(), duration: Some(secs), channel: Some(channel.into()), ..Default::default() }
    }

    #[test]
    fn a_different_song_of_the_same_length_is_not_a_match() {
        let t = song("KD!", "Depictions", 150);
        let wrong = entry("w", "Something Else", 150.0, "KD! - Topic");
        let right = entry("r", "Depictions", 149.0, "KD! - Topic");
        assert!(pick_best(&t, &[wrong.clone()]).is_none());
        assert_eq!(pick_best(&t, &[wrong, right]).map(|e| e.id.as_str()), Some("r"));
    }

    #[test]
    fn spacing_and_censoring_do_not_hide_the_title() {
        assert!(title_matches("Back2TheBasics", "Back 2 The Basics (Official Audio)"));
        assert!(title_matches("Wen We Fuckin", "Wen We F***in"));
        assert!(title_matches("Song (Remastered 2021)", "Song"));
        assert!(!title_matches("Depictions", "Depression"));
        assert!(title_matches("Wen We Fuckin", "wen we fuckn (WarSemi)"), "a one-letter spelling difference");
        assert!(!title_matches("Kickstand", "Kickback"));
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn audio_beats_the_music_video() {
        let t = song("ericdoa", "Kickstand", 120);
        let video = entry("v", "ericdoa - Kickstand (Official Music Video)", 120.0, "ericdoa");
        let audio = entry("a", "Kickstand", 118.0, "ericdoa");
        assert_eq!(rank(&t, &[video.clone(), audio]).first().map(|e| e.id.as_str()), Some("a"));
        // With only the video available, it still plays.
        assert_eq!(rank(&t, &[video]).len(), 1);
    }

    #[test]
    fn a_stale_link_to_another_song_is_refused() {
        let t = song("KD!", "Depictions", 150);
        assert!(is_wrong_video(&t, r#"{"title":"Other Song","duration":240}"#));
        assert!(is_wrong_video(&t, r#"{"title":"Other Song","duration":151}"#), "same length is not enough");
        assert!(is_wrong_video(&t, r#"{"title":"Other Song"}"#));
        assert!(!is_wrong_video(&t, r#"{"title":"KD! - Depictions","duration":151}"#));
        assert!(!is_wrong_video(&t, r#"{"duration":151}"#), "no title: nothing to compare");
        let w = song("deathtoricky", "wen we fuckn", 152);
        assert!(is_wrong_video(&w, r#"{"title":"all year","duration":150}"#));
        let b = song("KD!", "back2thebasics", 134);
        assert!(is_wrong_video(&b, r#"{"title":"Back Then","duration":134}"#));
        assert!(!is_wrong_video(&b, r#"{"title":"Back2TheBasics","duration":134}"#));
    }

    #[test]
    fn a_clear_match_is_the_artists_album_audio() {
        let t = song("Black Eyed Peas", "Meet Me Halfway", 284);
        assert!(clear_match(&t, &entry("a", "Meet Me Halfway", 284.0, "Black Eyed Peas - Topic")));
        assert!(!clear_match(&t, &entry("b", "Meet Me Halfway", 284.0, "Pop Hits Orchestra - Topic")));
        assert!(!clear_match(&t, &entry("c", "Meet Me Halfway (Live)", 300.0, "Black Eyed Peas - Topic")));
        assert!(!clear_match(&t, &entry("d", "Black Eyed Peas - Meet Me Halfway", 284.0, "BlackEyedPeasVEVO")));
    }

    #[test]
    fn reads_videos_from_a_youtube_search_reply() {
        let reply: serde_json::Value = serde_json::from_str(
            r#"{"contents":{"twoColumnSearchResultsRenderer":{"primaryContents":{"sectionListRenderer":{"contents":[{"itemSectionRenderer":{"contents":[
                {"channelRenderer":{"channelId":"UC1","title":{"simpleText":"Frank Ocean"}}},
                {"videoRenderer":{"videoId":"abc","title":{"runs":[{"text":"Pink + White"}]},"ownerText":{"runs":[{"text":"Frank Ocean - Topic"}]},
                    "lengthText":{"simpleText":"3:05"},"viewCountText":{"simpleText":"123,456,789 views"}}},
                {"reelShelfRenderer":{"items":[{"reelItemRenderer":{"videoId":"short"}}]}},
                {"videoRenderer":{"videoId":"live","title":{"runs":[{"text":"Live "},{"text":"radio"}]},"longBylineText":{"runs":[{"text":"Some channel"}]},
                    "viewCountText":{"runs":[{"text":"1,234"},{"text":" watching"}]}}},
                {"videoRenderer":{"videoId":"new","title":{"simpleText":"New upload"},"ownerText":{"runs":[{"text":"Me"}]},"lengthText":{"simpleText":"1:02:03"},"viewCountText":{"simpleText":"No views"}}}
            ]}}]}}}}}"#,
        )
        .unwrap();
        let found = web_entries(&reply, 8);
        let ids: Vec<&str> = found.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["abc", "live", "new"], "videos only, in order");
        assert_eq!(found[0].title, "Pink + White");
        assert_eq!(found[0].channel.as_deref(), Some("Frank Ocean - Topic"));
        assert_eq!(found[0].duration, Some(185.0));
        assert_eq!(found[0].view_count, Some(123_456_789));
        assert_eq!((found[1].title.as_str(), found[1].channel.as_deref(), found[1].duration, found[1].view_count), ("Live radio", Some("Some channel"), None, None));
        assert_eq!((found[2].duration, found[2].view_count), (Some(3723.0), Some(0)));
        assert_eq!(web_entries(&reply, 1).len(), 1);
    }

    #[test]
    fn the_audio_link_skips_stream_lists_pulse_never_plays() {
        let tools = Tools { ytdlp: "yt-dlp".into(), deno: "deno".into(), dir: "tools".into() };
        let args = link_args(&tools, Quality::Auto);
        assert!(args.iter().any(|a| a.contains("skip=hls,dash") && a.contains("-tv_samsung")), "{args:?}");
        assert!(!args.iter().any(|a| a.contains("youtube.com/watch")), "the link is added per song");
    }

    /// How long pressing a song waits for its audio link, step by step. Needs the network
    /// and yt-dlp: `PULSE_TOOLS_DIR=<tools> cargo test start_speed_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn start_speed_live() {
        let songs = [
            ("Frank Ocean", "Pink + White"),
            ("SZA", "Kill Bill"),
            ("Tame Impala", "The Less I Know The Better"),
            ("Daft Punk", "Get Lucky"),
            ("Metallica", "Enter Sandman"),
            ("Billie Eilish", "BIRDS OF A FEATHER"),
        ];
        let tools = ensure_tools(&|_| {}).expect("yt-dlp");
        let (mut first, mut search_only, mut views_t, mut known, mut ready) = (vec![], vec![], vec![], vec![], vec![]);
        for (artist, title) in songs {
            let Some(t) = crate::deezer::find_track(artist, title, 0).unwrap() else { continue };
            discard_spare();
            *SPARE_UNTIL.lock().unwrap() = None;
            // A song never played before, with nothing found ahead of time.
            let at = Instant::now();
            let (_, video) = resolve(&t, None, Quality::Auto, &|_| {}).expect("a link");
            first.push(at.elapsed().as_secs_f64());
            let at = Instant::now();
            find_videos(&tools, &t).expect("found");
            search_only.push(at.elapsed().as_secs_f64());
            let at = Instant::now();
            let v = views(&t).unwrap();
            views_t.push(at.elapsed().as_secs_f64());
            // A song Pulse already knows the video of.
            let at = Instant::now();
            audio_source(&tools, &video, Quality::Auto).expect("a link");
            known.push(at.elapsed().as_secs_f64());
            // The same, a moment after pointing at songs (a yt-dlp is waiting).
            get_ready(Quality::Auto);
            std::thread::sleep(Duration::from_secs(3));
            let at = Instant::now();
            audio_source(&tools, &video, Quality::Auto).expect("a link");
            ready.push(at.elapsed().as_secs_f64());
            println!("{title} – {artist}: video {video}, views {v:?}");
        }
        discard_spare();
        let median = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        println!("songs: {}", first.len());
        println!("never played, nothing ready: median {:.2} s", median(&mut first));
        println!("  finding the video:        median {:.2} s", median(&mut search_only));
        println!("view count lookup:          median {:.2} s", median(&mut views_t));
        println!("known video, cold yt-dlp:   median {:.2} s", median(&mut known));
        println!("known video, yt-dlp ready:  median {:.2} s", median(&mut ready));
    }

    /// Live check (needs the network): the explicit version of a song whose clean version
    /// looks identical, and an artist name Deezer shares between many artists.
    #[test]
    #[ignore]
    fn live_explicit_and_namesakes() {
        let found = music_search("ApolloRed1 - Geeked up").expect("YouTube Music search");
        println!("YouTube Music: {:?}", found.iter().take(4).map(|e| (&e.id, &e.title, &e.channel, e.duration, e.explicit)).collect::<Vec<_>>());
        let t = song("ApolloRed1", "Geeked up", 156);
        let _turn = crate::deezer::EXPLICIT_TEST.lock().unwrap_or_else(|e| e.into_inner());
        crate::deezer::set_prefer_explicit(true);
        let best = pick_best(&t, &found).expect("a match");
        println!("Picked: {} {:?}", best.id, best.explicit);
        assert_eq!(best.explicit, Some(true));
        let page = crate::deezer::artist_page("234701081").expect("Che's page");
        println!("Che: {:?}", page.releases.iter().map(|a| &a.title).collect::<Vec<_>>());
        println!("Others named Che: {}", page.namesakes.len());
        assert!(page.releases.iter().any(|a| a.title == "RichPhønks"));
        assert!(page.releases.len() < 15 && page.namesakes.len() > 40);
    }

    /// Prints what Deezer, YouTube and SoundCloud return for songs that play wrong.
    /// `PULSE_YTDLP=yt-dlp PULSE_DENO=deno cargo test live_trouble_songs -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_trouble_songs() {
        let tools = Tools { ytdlp: std::env::var("PULSE_YTDLP").unwrap().into(), deno: std::env::var("PULSE_DENO").unwrap().into(), dir: std::env::temp_dir().join("pulse-trouble") };
        println!("yt-dlp {}", run(&tools.ytdlp, &["--version".to_string()], Duration::from_secs(20)).unwrap_or_default().trim());
        let mut a = base_args(&tools);
        a.extend(["--print".into(), "%(title)s | %(duration)s | %(channel)s".into(), "https://www.youtube.com/watch?v=L-AgYKnXN1w".into()]);
        println!("LINK L-AgYKnXN1w: {:?}", run(&tools.ytdlp, &a, Duration::from_secs(60)));
        for q in ["KD! Back2TheBasics", "KD Back2TheBasics", "KD! Depictions", "Death To Ricky Wen We Fuckin", "ericdoa Kickstand"] {
            println!("\n=== Deezer: {q}");
            let Ok(r) = crate::deezer::search(q) else { println!("  search failed"); continue };
            for t in r.tracks.iter().take(3) {
                println!("  {} | {} – {} | {}s | rank {}", t.id, t.all_artists(), t.title, t.duration, t.rank);
            }
            let Some(t) = r.tracks.first() else { continue };
            let [quoted, plain] = queries(t);
            let mut entries = music_search(&plain).unwrap_or_default();
            entries.extend(web_search(Some(&tools), &quoted, 8).unwrap_or_default());
            entries.extend(web_search(Some(&tools), &plain, 8).unwrap_or_default());
            println!("  queries: {quoted} / {plain}; {} results; ranked:", entries.len());
            for e in rank(t, &entries).iter().take(4) {
                println!("    {} | {} | {} | {}s | score {}", e.id, e.title, e.channel.as_deref().unwrap_or("?"), e.duration.unwrap_or(0.0), score(t, e));
            }
        }
        for target in ["scsearch5:lofi hip hop", "scsearch5:Death To Ricky", "scsearch5:Death To Ricky Wen We Fuckin", "scsearch5:deathtoricky wen we", "scsearch5:wen we fuckin"] {
            let mut flat = base_args(&tools);
            flat.extend(["--flat-playlist".into(), "-J".into(), target.into()]);
            let out = run(&tools.ytdlp, &flat, Duration::from_secs(60));
            println!("\nSOUNDCLOUD flat ({target}):");
            match out.map(|o| serde_json::from_str::<serde_json::Value>(&o)) {
                Ok(Ok(v)) => {
                    for e in v["entries"].as_array().into_iter().flatten().take(5) {
                        println!("  {}", serde_json::to_string(&serde_json::json!({"id":e["id"],"title":e["title"],"uploader":e["uploader"],"duration":e["duration"],"url":e["url"],"webpage_url":e["webpage_url"],"ie":e["ie_key"]})).unwrap());
                    }
                    println!("  ({} entries)", v["entries"].as_array().map_or(0, Vec::len));
                }
                other => println!("  failed: {:?}", other.map(|r| r.map(|_| ()).map_err(|e| e.to_string()))),
            }
        }
        // A track's own page, to see what a SoundCloud link gives (a public sample track).
        let mut one = base_args(&tools);
        one.extend(["-J".into(), "https://soundcloud.com/forss/flickermood".into()]);
        match run(&tools.ytdlp, &one, Duration::from_secs(60)).map(|o| serde_json::from_str::<serde_json::Value>(&o)) {
            Ok(Ok(v)) => println!("\nSOUNDCLOUD track: title {} | uploader {} | duration {} | ext {} | url set {} | thumbnail {}", v["title"], v["uploader"], v["duration"], v["ext"], v["url"].is_string(), v["thumbnail"]),
            other => println!("\nSOUNDCLOUD track failed: {:?}", other.map(|r| r.map(|_| ()).map_err(|e| e.to_string()))),
        }
    }

    /// Opens "Wen We Fuckin" from SoundCloud for real and fetches the first bytes of its audio.
    #[test]
    #[ignore]
    fn live_soundcloud_wen_we() {
        let tools = Tools { ytdlp: std::env::var("PULSE_YTDLP").unwrap().into(), deno: std::env::var("PULSE_DENO").unwrap().into(), dir: std::env::temp_dir().join("pulse-sc") };
        let t = song("Death To Ricky", "Wen We Fuckin", 153);
        let t0 = Instant::now();
        let url = crate::soundcloud::find(&tools, &t, 2).expect("found");
        let search_ms = t0.elapsed().as_millis();
        let t1 = Instant::now();
        let _ = std::fs::remove_dir_all(crate::settings::data_dir().join("cache").join("soundcloud"));
        crate::soundcloud::open(&tools, &url, &t).expect("opens");
        println!("TIMING: search {search_ms} ms, download {} ms", t1.elapsed().as_millis());
        let (src, id) = crate::soundcloud::find_and_open(&tools, &t).expect("found on SoundCloud");
        println!("SC id {id}\ncodec {:?} | length {:?} | notice {:?} | file {}", src.codec, src.duration, src.notice, src.url);
        let path = src.url.strip_prefix("file://").unwrap();
        println!("file size {} bytes", std::fs::metadata(path).unwrap().len());
        // Decode it the way the player does and measure what comes out.
        let dec = rodio::Decoder::try_from(std::fs::File::open(path).unwrap()).expect("decoder opens the file");
        let (ch, rate) = (rodio::Source::channels(&dec).get() as f64, rodio::Source::sample_rate(&dec).get() as f64);
        let secs = dec.count() as f64 / (ch * rate);
        println!("decoded {secs:.1} s");
        assert!((secs - 152.7).abs() < 3.0, "the whole track decodes: {secs:.1} s");
        // Saved id plays again without a search.
        let again = crate::soundcloud::open(&tools, crate::soundcloud::url_of(&id).unwrap(), &t);
        let again = again.inspect(|s| println!("reopen is {}", if s.url.starts_with("file://") { "from the saved file" } else { "new" }));
        println!("saved id reopens: {}", again.is_ok());
        assert!(again.is_ok());
        // A song that isn't there is not replaced by another.
        let none = crate::soundcloud::find_and_open(&tools, &song("Death To Ricky", "A Song That Does Not Exist Anywhere", 200));
        println!("missing song: {:?}", none.map(|_| ()));
    }

    #[test]
    fn reads_youtube_music_songs_and_the_explicit_badge() {
        let col = |runs: &[&str]| serde_json::json!({ "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": runs.iter().map(|t| serde_json::json!({ "text": t })).collect::<Vec<_>>() } } });
        let item = |id: &str, title: &str, info: &[&str], explicit: bool| {
            let mut r = serde_json::json!({ "playlistItemData": { "videoId": id }, "flexColumns": [col(&[title]), col(info)] });
            if explicit {
                r["badges"] = serde_json::json!([{ "musicInlineBadgeRenderer": { "icon": { "iconType": "MUSIC_EXPLICIT_BADGE" } } }]);
            }
            serde_json::json!({ "musicResponsiveListItemRenderer": r })
        };
        let reply = serde_json::json!({ "contents": { "items": [
            item("dirty", "Geeked up", &["ApolloRed1", " & ", "OsamaSon", " • ", "Demon Heart Radio", " • ", "2:36"], true),
            item("clean", "Geeked up", &["ApolloRed1", " • ", "Demon Heart Radio", " • ", "2:35"], false),
        ] } });
        let found = music_entries(&reply);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].channel.as_deref(), Some("ApolloRed1 & OsamaSon - Topic"));
        assert_eq!(found[0].duration, Some(156.0));
        assert_eq!((found[0].explicit, found[1].explicit), (Some(true), Some(false)));
        // The same song with the same length: only the badge tells them apart.
        let t = song("ApolloRed1", "Geeked up", 156);
        let _turn = crate::deezer::EXPLICIT_TEST.lock().unwrap_or_else(|e| e.into_inner());
        crate::deezer::set_prefer_explicit(true);
        assert_eq!(pick_best(&t, &found).unwrap().id, "dirty");
        crate::deezer::set_prefer_explicit(false);
        assert_eq!(pick_best(&t, &found).unwrap().id, "clean");
        crate::deezer::set_prefer_explicit(true);
    }

    #[test]
    fn explicit_or_clean_as_asked() {
        let mut t = song("Nelly", "Ride Wit Me", 291);
        t.explicit = true;
        let results = vec![entry("clean", "Nelly - Ride Wit Me (Clean)", 291.0, "Nelly"), entry("dirty", "Nelly - Ride Wit Me (Explicit)", 291.0, "Nelly")];
        let _turn = crate::deezer::EXPLICIT_TEST.lock().unwrap_or_else(|e| e.into_inner());
        crate::deezer::set_prefer_explicit(true);
        assert_eq!(rank(&t, &results)[0].id, "dirty");
        crate::deezer::set_prefer_explicit(false);
        assert_eq!(rank(&t, &results)[0].id, "clean");
        crate::deezer::set_prefer_explicit(true);
    }

    #[test]
    fn skips_covers_instrumentals_and_clips() {
        let t = song("Black Eyed Peas", "Meet Me Halfway", 284);
        let results = vec![
            // Another artist's album audio of the same song: a cover.
            entry("cover", "Meet Me Halfway", 283.0, "Pop Hits Orchestra - Topic"),
            entry("real", "Meet Me Halfway", 284.0, "Black Eyed Peas - Topic"),
            entry("mv", "Black Eyed Peas - Meet Me Halfway (Official Music Video)", 290.0, "BlackEyedPeasVEVO"),
        ];
        let ids: Vec<&str> = rank(&t, &results).iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.first(), Some(&"real"));
        assert!(!ids.contains(&"cover"));

        let t = song("Nelly", "Ride Wit Me", 291);
        let results = vec![
            entry("inst", "Nelly - Ride Wit Me (Instrumental)", 291.0, "Nelly - Topic"),
            entry("inst2", "Ride Wit Me", 291.0, "Instrumental Hip Hop Beats"),
            entry("clip", "Nelly - Ride Wit Me #shorts", 30.0, "Nelly"),
            entry("real", "Ride Wit Me", 291.0, "Nelly - Topic"),
        ];
        let ids: Vec<&str> = rank(&t, &results).iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["real"]);
    }

    #[test]
    fn prefers_official_audio_of_the_right_length() {
        let t = song("ericdoa", "Tengo Algo Para Ti", 142);
        let results = vec![
            entry("mv", "ericdoa - Tengo Algo Para Ti (Official Music Video)", 171.0, "ericdoa"),
            entry("topic", "Tengo Algo Para Ti", 142.0, "ericdoa - Topic"),
            entry("sped", "ericdoa - tengo algo para ti (sped up)", 118.0, "nightcore hub"),
            entry("other", "Tengo Algo Para Ti", 205.0, "Los Angeles Azules - Topic"),
        ];
        assert_eq!(pick_best(&t, &results).unwrap().id, "topic");
    }

    #[test]
    fn skips_remixes_and_live_versions_unless_asked_for() {
        let t = song("Tame Impala", "Dracula (with JENNIE)", 209);
        let results = vec![
            entry("live", "Tame Impala - Dracula (Live at Coachella)", 212.0, "Tame Impala"),
            entry("ok", "Tame Impala, JENNIE - Dracula (Official Audio)", 210.0, "Tame Impala"),
        ];
        assert_eq!(pick_best(&t, &results).unwrap().id, "ok");

        let remix = song("ericdoa", "search & destroy (Double A Remix)", 200);
        let results = vec![entry("r", "ericdoa - search & destroy (Double A Remix)", 200.0, "Double A")];
        assert_eq!(pick_best(&remix, &results).unwrap().id, "r");
    }

    #[test]
    fn alive_is_not_live() {
        let t = song("Example", "Alive", 200);
        let e = entry("a", "Example - Alive (Official Audio)", 200.0, "Example - Topic");
        assert!(score(&t, &e) > 80);
    }

    #[test]
    fn gives_up_on_unrelated_results() {
        let t = song("ericdoa", "fantasize", 150);
        let results = vec![entry("x", "Cooking pasta in 10 minutes", 600.0, "Chef Channel")];
        assert!(pick_best(&t, &results).is_none());
    }

    #[test]
    fn strips_featured_artists_from_titles() {
        assert_eq!(core_title("Dracula (with JENNIE)"), "Dracula");
        assert_eq!(core_title("SLiiDE feat. ericdoa"), "SLiiDE");
        assert_eq!(core_title("heather"), "heather");
    }

    #[test]
    fn reads_checksum_lists() {
        let list = "e51e69e89489f199530f13d32a4342da3ddb2cf34842f364603a066db05afbf3  yt-dlp\nf5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d  yt-dlp.exe\n";
        assert_eq!(checksum_for(list, "yt-dlp.exe").unwrap(), "f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d");
        let ps = "Algorithm : SHA256\nHash      : CBA84B26E0DF5AE7F74FC66B86A95B866872A86292EBD84FA1E959927EDD93BF\nPath      : C:\\a\\deno.zip\n";
        assert_eq!(first_sha256(ps).unwrap(), "cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf");
        let linux = "e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609  deno-x86_64-unknown-linux-gnu.zip";
        assert!(first_sha256(linux).unwrap().starts_with("e890975b"));
    }

    #[test]
    fn reads_the_audio_link_from_ytdlp() {
        let json = r#"{"id":"abc","title":"Song","ext":"m4a","url":"https://rr1.googlevideo.com/videoplayback?expire=1&itag=140","filesize":2297000,"http_headers":{"User-Agent":"Mozilla/5.0","Accept":"*/*","Accept-Encoding":"gzip"}}"#;
        let s = source_from_info(json).unwrap();
        assert_eq!(s.codec, Codec::Mp4);
        assert_eq!(s.content_length, Some(2297000));
        assert!(s.url.contains("itag=140"));
        assert!(s.headers.iter().any(|(k, _)| k == "User-Agent"));
        assert!(!s.headers.iter().any(|(k, _)| k == "Accept-Encoding"));

        let webm = r#"{"ext":"webm","url":"https://x"}"#;
        assert!(source_from_info(webm).is_err());

        let nested = r#"{"requested_downloads":[{"ext":"m4a","url":"https://y","filesize_approx":10}]}"#;
        assert_eq!(source_from_info(nested).unwrap().url, "https://y");
    }

    #[test]
    fn search_results_with_gaps_still_parse() {
        let r: SearchResult = serde_json::from_str(r#"{"entries":[null,{"id":"a","title":"x","duration":10}]}"#).unwrap();
        assert_eq!(r.entries.into_iter().flatten().count(), 1);
    }

    #[test]
    fn ranks_all_good_matches_and_drops_duplicates() {
        let t = song("ericdoa", "tengo algo para ti", 150);
        let list = vec![
            entry("a", "ericdoa - tengo algo para ti (official video)", 190.0, "ericdoa"),
            entry("b", "tengo algo para ti", 150.0, "ericdoa - Topic"),
            entry("b", "tengo algo para ti", 150.0, "ericdoa - Topic"),
            entry("c", "cooking for beginners", 600.0, "Chef"),
        ];
        let ids: Vec<_> = rank(&t, &list).iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["b", "a"]);
    }

    #[test]
    fn searches_with_and_without_quotes() {
        let [q, p] = queries(&song("ericdoa", "tengo \"algo\" para ti (feat. x)", 150));
        assert_eq!(q, "ericdoa \"tengo algo para ti\"");
        assert_eq!(p, "ericdoa - tengo algo para ti");
        assert_eq!(crate::net::encode("a b&c"), "a%20b%26c");
    }

    #[test]
    fn waits_for_delayed_links() {
        assert_eq!(wait_for(None, 100).unwrap(), Duration::ZERO);
        assert_eq!(wait_for(Some(90), 100).unwrap(), Duration::ZERO);
        assert_eq!(wait_for(Some(105), 100).unwrap(), Duration::from_secs(5));
        assert!(wait_for(Some(1000), 100).is_err());
    }

    #[test]
    fn unpacks_the_yt_dlp_folder_build() {
        let Ok(zip) = std::fs::read("/tmp/ytz/w.zip") else {
            eprintln!("skipping: /tmp/ytz/w.zip not found");
            return;
        };
        let dest = std::env::temp_dir().join(format!("pulse-unzip-{}", std::process::id()));
        unzip_all(&zip, &dest).unwrap();
        assert!(dest.join("yt-dlp.exe").is_file());
        assert!(dest.join("_internal").is_dir());
        std::fs::remove_dir_all(dest).unwrap();
        assert!(unzip_all(b"not a zip", Path::new("/tmp/x")).is_err());
    }

    #[test]
    fn explains_common_errors() {
        assert!(explain_error("ERROR: [youtube] x: Sign in to confirm you’re not a bot").contains("bot"));
        assert!(explain_error("ERROR: [youtube] x: Video unavailable").contains("isn't available"));
    }

    /// Downloads the real tools (needs internet access to GitHub). Run with:
    /// cargo test -- --ignored downloads_and_verifies_tools
    #[test]
    #[ignore]
    fn downloads_and_verifies_tools() {
        let dir = std::env::temp_dir().join("pulse-tools-test");
        let _ = std::fs::remove_dir_all(&dir);
        // SAFETY of set_var: single-threaded test setup.
        unsafe { std::env::set_var("PULSE_TOOLS_DIR", &dir) };
        let last = std::sync::Mutex::new(String::new());
        let tools = ensure_tools(&|s| *last.lock().unwrap() = s.to_string()).expect("tools");
        println!("last status: {}", last.lock().unwrap());
        let v = run(&tools.ytdlp, &["--version".into()], Duration::from_secs(60)).expect("yt-dlp runs");
        println!("yt-dlp {}", v.trim());
        let d = run(&tools.deno, &["--version".into()], Duration::from_secs(60)).expect("deno runs");
        println!("{}", d.lines().next().unwrap_or(""));
        // Second call finds them already installed.
        ensure_tools(&|_| {}).expect("tools again");
    }

    /// Matches a real playlist against Deezer and YouTube and prints what Pulse would pick,
    /// flagging anything that looks wrong. Needs the network, so it only runs when asked:
    /// `PULSE_LIVE_SONGS=tests/data/friend-playlist.txt PULSE_LIVE_SPOTIFY=<link>
    /// PULSE_YTDLP=yt-dlp PULSE_DENO=deno cargo test live_match_report -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_match_report() {
        let mut songs: Vec<(String, String, u32)> = vec![];
        if let Ok(url) = std::env::var("PULSE_LIVE_SPOTIFY") {
            let read = crate::import::spotify_for_test(&url).expect("Spotify link");
            println!("Spotify link: {} songs read", read.len());
            for (a, t, d) in read.iter().take(5) {
                println!("  {a} | {t} | {d}s");
            }
            songs.extend(read);
        }
        if let Ok(path) = std::env::var("PULSE_LIVE_SONGS") {
            for line in std::fs::read_to_string(path).expect("songs file").lines().filter(|l| !l.starts_with('#') && l.contains('|')) {
                let p: Vec<&str> = line.split('|').map(str::trim).collect();
                let (m, s) = p[2].split_once(':').unwrap_or(("0", "0"));
                songs.push((p[0].into(), p[1].into(), m.parse::<u32>().unwrap_or(0) * 60 + s.parse::<u32>().unwrap_or(0)));
            }
        }
        // The same song twice (the link and the list) is only checked once.
        let mut clips: Vec<String> = vec![];
        let mut seen = std::collections::HashSet::new();
        songs.retain(|(a, t, _)| seen.insert(format!("{}|{}", norm(a), norm(t))));
        let tools = std::env::var("PULSE_YTDLP").ok().map(|y| Tools {
            ytdlp: y.into(),
            deno: std::env::var("PULSE_DENO").unwrap_or("deno".into()).into(),
            dir: std::env::temp_dir(),
        });
        let (mut ok, mut flagged, mut missing, mut on_tidal) = (0, 0, 0, 0);
        for (n, (artists, title, secs)) in songs.iter().enumerate() {
            std::thread::sleep(std::time::Duration::from_millis(250));
            // The real song listed only as a clip: a row for the song fixes list.
            let (main, others) = crate::deezer::split_artists(artists);
            let names: Vec<String> = std::iter::once(&main).chain(others.iter()).map(|a| norm(a)).collect();
            for c in crate::deezer::candidates_for_test(artists, title) {
                if c.duration <= 30
                    && crate::deezer::same_title(title, &c.title)
                    && crate::deezer::same_artist(&names, &c.artist.name)
                    && !crate::youtube::unwanted_version(title, &c.title)
                {
                    let row =
                        format!("('{}', {secs}, NULL, '{} – {} (Deezer lists a clip)')", c.id, c.artist.name.replace('\'', "''"), c.title.replace('\'', "''"));
                    if !clips.contains(&row) {
                        clips.push(row);
                    }
                }
            }
            let found = crate::deezer::find_track(artists, title, *secs);
            let mut notes: Vec<String> = vec![];
            let mut on_deezer = true;
            let track = match found {
                Ok(Some(t)) => t,
                Ok(None) | Err(_) => {
                    missing += 1;
                    on_deezer = false;
                    println!("{:>3}. NOT ON DEEZER  {artists} – {title} ({secs}s): imported as is, played from YouTube", n + 1);
                    for c in crate::deezer::candidates_for_test(artists, title).iter().take(4) {
                        println!("       had: {} – {} ({}s, rank {})", c.artist.name, c.title, c.duration, c.rank);
                    }
                    let (main, also) = crate::deezer::split_artists(artists);
                    let tidal = crate::tidal::find(artists, title, *secs);
                    match &tidal {
                        Some(s) => {
                            on_tidal += 1;
                            println!("       TIDAL: {} – {} ({}s, explicit {}, {})", s.artists.join(", "), s.title, s.duration, s.explicit, s.isrc);
                        }
                        None => println!("       TIDAL: not found"),
                    }
                    Track {
                        title: title.clone(),
                        duration: *secs,
                        explicit: tidal.as_ref().is_some_and(|s| s.explicit),
                        artist: crate::deezer::ArtistRef { name: main, ..Default::default() },
                        featured: also.into_iter().map(|name| crate::deezer::ArtistRef { name, ..Default::default() }).collect(),
                        ..Default::default()
                    }
                }
            };
            if track.duration.abs_diff(*secs) > 5 {
                notes.push(format!("length {}s vs {secs}s", track.duration));
            }
            if crate::youtube::unwanted_version(title, &track.title) {
                notes.push("unwanted version".into());
            }
            // Featured artists the source credits: in the title, or (filled in by the app) the credits.
            let (_, also) = crate::deezer::split_artists(artists);
            let credits =
                if on_deezer { crate::deezer::featured_artists(track.deezer_number().unwrap_or(0)).unwrap_or_default() } else { track.featured.clone() };
            let credited = norm(&track.title) + &norm(&track.artist.name) + &credits.iter().map(|a| norm(&a.name)).collect::<String>();
            let lost: Vec<&String> = also.iter().filter(|a| !credited.contains(&norm(a))).collect();
            if !lost.is_empty() {
                notes.push(format!("featured not credited: {lost:?}"));
            }
            let mut video = String::new();
            if let Some(tools) = &tools {
                let mut full = track.clone();
                full.featured = credits.clone();
                let [quoted, plain] = queries(&full);
                let music = format!("https://music.youtube.com/search?q={}#songs", crate::net::encode(&plain));
                let mut entries = vec![];
                for target in [music, format!("ytsearch8:{quoted}"), format!("ytsearch8:{plain}")] {
                    entries.extend(search(tools, target, 8).unwrap_or_default());
                }
                match rank(&full, &entries).first() {
                    Some(e) => {
                        let d = e.duration.unwrap_or(0.0);
                        video = format!("  ▶ {} | {} | {}s | score {}", e.title, e.channel.as_deref().unwrap_or("?"), d as u32, score(&full, e));
                        if (d - track.duration as f64).abs() > 8.0 {
                            notes.push(format!("video length {}s", d as u32));
                        }
                    }
                    None => notes.push(format!("no YouTube match among {} results", entries.len())),
                }
            }
            if !on_deezer {
                if !notes.is_empty() {
                    flagged += 1;
                }
                println!("       {}{video}", notes.join("; "));
                continue;
            }
            let tag = if notes.is_empty() {
                ok += 1;
                "ok      "
            } else {
                flagged += 1;
                "CHECK   "
            };
            let feat =
                if credits.is_empty() { String::new() } else { format!(" (with {})", credits.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")) };
            println!(
                "{:>3}. {tag}{artists} – {title} ({secs}s) → dz:{} \"{}\" by {}{feat}, {}s, rank {}  {}",
                n + 1,
                track.deezer_number().unwrap_or(0),
                track.title,
                track.artist.name,
                track.duration,
                track.rank,
                notes.join("; ")
            );
            if !video.is_empty() {
                println!("{video}");
            }
        }
        println!("\nSONG FIXES SQL:\nINSERT OR REPLACE INTO song_fixes (id, duration, video, note) VALUES\n{};", clips.join(",\n"));
        println!("\nSEARCH 'Firework Katy Perry':");
        if let Ok(r) = crate::deezer::search("Firework Katy Perry") {
            for t in r.tracks.iter().take(5) {
                println!("  {} – {} ({}s, rank {})", t.all_artists(), t.title, t.duration, t.rank);
            }
        }
        println!(
            "\nSUMMARY: {ok} ok on Deezer, {missing} not on Deezer (played from YouTube; TIDAL knew {on_tidal} of them), {flagged} to check, of {} songs",
            songs.len()
        );
    }
}
