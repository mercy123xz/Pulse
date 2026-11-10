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
use std::process::{Command, Stdio};
use std::sync::Mutex;
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
        let zip = crate::net::download(&format!("{DENO_RELEASE}/{}", DENO.name), |done, _| {
            show(YTDLP_MB + done as f64 / 1e6)
        })?;
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
    if got == expected {
        Ok(())
    } else {
        Err(format!("The {what} download was damaged or tampered with, so Pulse didn't use it. Try again."))
    }
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
fn run(program: &Path, args: &[String], timeout: Duration) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000; // no black console window popping up
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start {} ({e})", program.display()))?;
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
    if status.success() {
        Ok(out)
    } else {
        Err(explain_error(&err))
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
        "YouTube only plays this song for signed-in adults.".into()
    } else if lower.contains("unable to download") || lower.contains("timed out") || lower.contains("getaddrinfo") {
        "Couldn't reach YouTube. Check your internet connection.".into()
    } else {
        let msg = last.trim().trim_start_matches("ERROR:").trim();
        if msg.is_empty() { "YouTube playback failed.".into() } else { format!("YouTube playback failed: {msg}") }
    }
}

fn base_args(tools: &Tools) -> Vec<String> {
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
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SearchResult {
    /// yt-dlp puts `null` here for results it couldn't read.
    entries: Vec<Option<SearchEntry>>,
}

/// Lowercases and keeps only letters and digits, so "Tengo Algo Para Ti!" and
/// "tengo algo para ti" compare equal.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The song title without "(feat. …)" / "(with …)" and similar extras.
pub fn core_title(title: &str) -> String {
    let lower = title.to_lowercase();
    let cut = ["(feat", "(ft.", "(with ", "[feat", " feat.", " ft. "].iter().filter_map(|m| lower.find(m)).min();
    match cut {
        Some(i) => title[..i].trim().to_string(),
        None => title.trim().to_string(),
    }
}

/// Scores how well a YouTube result matches the song. Higher is better.
pub fn score(track: &Track, e: &SearchEntry) -> i32 {
    let mut s = 0;
    let title = e.title.to_lowercase();
    let channel = e.channel.as_deref().or(e.uploader.as_deref()).unwrap_or("").to_lowercase();
    let want_title = norm(&core_title(&track.title));
    let artist = norm(&track.artist.name);

    // Length is the strongest signal: music videos and live versions run long.
    if let Some(d) = e.duration
        && track.duration > 0
    {
        let diff = (d - track.duration as f64).abs();
        s += match diff {
            x if x <= 3.0 => 40,
            x if x <= 8.0 => 25,
            x if x <= 20.0 => 5,
            x if x <= 45.0 => -15,
            _ => -40,
        };
    }
    // "Artist - Topic" channels carry the official album audio.
    if channel.ends_with(" - topic") {
        s += 30;
    }
    if !artist.is_empty() && (norm(&channel).contains(&artist) || norm(&title).contains(&artist)) {
        s += 20;
    }
    if !want_title.is_empty() && norm(&title).contains(&want_title) {
        s += 25;
    }
    if title.contains("audio") {
        s += 5;
    }
    // Versions people didn't ask for (unless the song title itself says so).
    // Whole words only, so "live" doesn't match "alive".
    let words = |s: &str| format!(" {} ", s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" "));
    let (t_words, song) = (words(&e.title), words(&track.title));
    for bad in ["live", "cover", "remix", "sped up", "slowed", "reverb", "nightcore", "8d", "karaoke", "instrumental", "1 hour", "loop"] {
        let w = format!(" {bad} ");
        if t_words.contains(&w) && !song.contains(&w) {
            s -= 30;
        }
    }
    if title.contains("lyrics") || title.contains("lyric video") {
        s -= 2;
    }
    s
}

/// All results that look like the song, best first. Empty if nothing fits.
pub fn rank<'a>(track: &Track, entries: &'a [SearchEntry]) -> Vec<&'a SearchEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut scored: Vec<(i32, &SearchEntry)> = entries
        .iter()
        .filter(|e| !e.id.is_empty() && seen.insert(e.id.as_str()))
        .map(|e| (score(track, e), e))
        .filter(|(s, _)| *s >= 0)
        .collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored.into_iter().map(|(_, e)| e).collect()
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

/// The two YouTube searches Pulse runs for a song: one with the title in quotes
/// (YouTube then only returns videos that contain it) and one plain.
fn queries(track: &Track) -> [String; 2] {
    let title = core_title(&track.title).replace('"', "");
    [format!("{} \"{title}\"", track.artist.name), format!("{} - {title}", track.artist.name)]
}

/// Searches YouTube and returns the ids of the videos that match `track`, best first.
pub fn find_videos(tools: &Tools, track: &Track) -> Result<Vec<String>, String> {
    // Both searches at once, so this takes as long as one.
    let [quoted, plain] = queries(track);
    let (a, b) = std::thread::scope(|s| {
        let a = s.spawn(|| search(tools, format!("ytsearch8:{quoted}"), 8));
        let b = s.spawn(|| search(tools, format!("ytsearch8:{plain}"), 8));
        (a.join().unwrap_or_else(|_| Err("search crashed".into())), b.join().unwrap_or_else(|_| Err("search crashed".into())))
    });
    let mut entries = Vec::new();
    let mut error = None;
    for r in [a, b] {
        match r {
            Ok(mut e) => entries.append(&mut e),
            Err(e) => error = Some(e),
        }
    }
    let mut ids: Vec<String> = rank(track, &entries).into_iter().map(|e| e.id.clone()).collect();
    if ids.is_empty() {
        // Some songs only show up on YouTube Music.
        let url = format!("https://music.youtube.com/search?q={}#songs", crate::net::encode(&plain));
        if let Ok(music) = search(tools, url, 8) {
            ids = rank(track, &music).into_iter().map(|e| e.id.clone()).collect();
        }
    }
    match (ids.is_empty(), error) {
        (false, _) => Ok(ids),
        (true, Some(e)) if entries.is_empty() => Err(e),
        _ => Err("Couldn't find this song on YouTube.".to_string()),
    }
}

/// True once yt-dlp is installed (view counts don't start the first-time setup).
pub fn tools_ready() -> bool {
    let dir = tools_dir();
    dir.join(YTDLP.save_as).is_file() && dir.join(DENO.save_as).is_file()
}

/// The YouTube video Pulse would play for `track`, and its view count.
pub fn views(track: &Track) -> Result<Option<(String, u64)>, String> {
    if !tools_ready() {
        return Ok(None);
    }
    let tools = installed_tools();
    let [_, plain] = queries(track);
    let entries = search(&tools, format!("ytsearch6:{plain}"), 6)?;
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
fn source_from_info(json: &str) -> Result<StreamSource, String> {
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

/// Asks yt-dlp for the direct audio link of a YouTube video.
pub fn audio_source(tools: &Tools, video_id: &str, quality: Quality) -> Result<StreamSource, String> {
    let mut args = base_args(tools);
    args.extend([
        "--no-playlist".into(),
        "-J".into(),
        "-f".into(),
        quality.ytdlp_format().into(),
        // Skip a YouTube client that often hands out broken links.
        "--extractor-args".into(),
        "youtube:player_client=default,-tv_samsung".into(),
        format!("https://www.youtube.com/watch?v={video_id}"),
    ]);
    let out = run(&tools.ytdlp, &args, Duration::from_secs(60))?;
    source_from_info(&out)
}

/// Everything needed to play `track` from YouTube. `known_video` skips the search
/// when Pulse already knows which video it is. Returns the source and the video id.
pub fn resolve(track: &Track, known_video: Option<String>, quality: Quality, status: &dyn Fn(&str)) -> Result<(StreamSource, String), String> {
    let tools = ensure_tools(status)?;
    let attempt = |status: &dyn Fn(&str)| -> Result<(StreamSource, String), String> {
        if let Some(id) = &known_video {
            status("Loading audio…");
            if let Ok(src) = audio_source(&tools, id, quality) {
                return Ok((src, id.clone()));
            }
            // The saved video may be gone: search again.
        }
        status("Finding song on YouTube…");
        let ids = find_videos(&tools, track)?;
        status("Loading audio…");
        // If the best video won't play (removed, age-gated, region-locked), try the next ones.
        let mut first_error = None;
        for id in ids.iter().filter(|id| Some(*id) != known_video.as_ref()).take(3) {
            match audio_source(&tools, id, quality) {
                Ok(src) => return Ok((src, id.clone())),
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        Err(first_error.unwrap_or_else(|| "Couldn't find this song on YouTube.".into()))
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
mod tests {
    use super::*;
    use crate::deezer::ArtistRef;

    fn song(artist: &str, title: &str, secs: u32) -> Track {
        Track {
            title: title.into(),
            duration: secs,
            artist: ArtistRef { name: artist.into(), ..Default::default() },
            ..Default::default()
        }
    }

    fn entry(id: &str, title: &str, secs: f64, channel: &str) -> SearchEntry {
        SearchEntry { id: id.into(), title: title.into(), duration: Some(secs), channel: Some(channel.into()), uploader: None, view_count: None }
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
}
