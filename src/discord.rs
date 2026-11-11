//! Discord status: "Listening to Pulse" with the song, its artists, the cover and
//! a progress bar on your Discord profile. Off until you turn it on in Settings.
//!
//! Pulse talks to the Discord app on this PC through its local connection (a named
//! pipe on Windows); nothing goes over the internet from Pulse, and nothing happens
//! when Discord isn't running. A background thread owns the connection, sends only
//! changes, and stays inside Discord's limit of a few updates per 20 seconds.

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

/// Pulse's application on Discord (it's public: it only names the app).
const APP_ID: &str = "1569481347553038476";
/// Shown when a song has no cover Discord can load (local files), and as the small badge.
const LOGO: &str = "https://mercy123xz.github.io/Pulse/img/apple-touch-icon.png";

fn app_id() -> String {
    // Developer check: PULSE_DEBUG_DISCORD_ID=<id> tries another application.
    std::env::var("PULSE_DEBUG_DISCORD_ID").unwrap_or_else(|_| APP_ID.to_string())
}

/// How the connection to Discord is going, for Settings to show.
#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    /// Turned off.
    Off,
    /// Looking for the Discord app.
    Looking,
    /// No Discord app found on this PC.
    NotRunning,
    /// Discord is there, but Windows wouldn't let Pulse connect.
    Blocked,
    /// Connected; showing a song (true) or nothing playing (false).
    Connected(bool),
    /// Discord answered with an error.
    Refused(String),
}

static LINK: std::sync::Mutex<Option<Link>> = std::sync::Mutex::new(None);

fn set_link(l: Link) {
    if let Ok(mut g) = LINK.lock() {
        *g = Some(l);
    }
}

/// How the connection to Discord is going.
pub fn link() -> Link {
    LINK.lock().ok().and_then(|g| g.clone()).unwrap_or(Link::Off)
}

/// Whether this build of Pulse can show a Discord status at all.
pub fn available() -> bool {
    !app_id().is_empty()
}

/// What to show.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub title: String,
    pub artists: String,
    pub album: Option<String>,
    /// An https address Discord can load.
    pub cover: Option<String>,
    /// When the song started and will end (ms since 1970), while it plays.
    pub times: Option<(i64, i64)>,
}

impl Song {
    /// Close enough that Discord needn't hear about it (the start time drifts a few
    /// milliseconds from frame to frame).
    fn same_as(&self, other: &Song) -> bool {
        let times_close = match (self.times, other.times) {
            (Some((a, _)), Some((b, _))) => (a - b).abs() < 2000,
            (None, None) => true,
            _ => false,
        };
        self.title == other.title && self.artists == other.artists && self.cover == other.cover && times_close
    }
}

/// The Discord status, kept by the app.
#[derive(Default)]
pub struct Presence {
    tx: Option<Sender<Option<Song>>>,
    shown: Option<Song>,
}

impl Presence {
    /// Called every frame with what should show; only changes are passed on.
    pub fn update(&mut self, enabled: bool, want: Option<Song>) {
        if !enabled || !available() {
            // Dropping the sender ends the thread, which closes the connection: Discord
            // then clears the status by itself.
            self.tx = None;
            self.shown = None;
            return;
        }
        let unchanged = match (&self.shown, &want) {
            (Some(a), Some(b)) => a.same_as(b),
            (None, None) => self.tx.is_some(),
            _ => false,
        };
        if unchanged {
            return;
        }
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = channel();
            std::thread::Builder::new().name("discord".into()).spawn(move || run(rx)).ok();
            tx
        });
        if tx.send(want.clone()).is_err() {
            self.tx = None;
        }
        self.shown = want;
    }
}

/// The background thread: connects when Discord is running, sends the newest status.
fn run(rx: Receiver<Option<Song>>) {
    set_link(Link::Looking);
    run_inner(rx);
    set_link(Link::Off);
}

fn run_inner(rx: Receiver<Option<Song>>) {
    let mut conn: Option<Conn> = None;
    let mut want: Option<Song> = None;
    let mut dirty = false;
    let mut last_sent: Option<Instant> = None;
    let mut next_try = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(w) => {
                want = w;
                dirty = true;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while let Ok(w) = rx.try_recv() {
            want = w;
            dirty = true;
        }
        if !dirty {
            continue;
        }
        if conn.is_none() {
            if Instant::now() < next_try {
                continue;
            }
            match Conn::open() {
                Ok(c) => conn = Some(c),
                Err(why) => {
                    // Discord isn't running (or isn't signed in): look again in a while.
                    set_link(why);
                    next_try = Instant::now() + Duration::from_secs(20);
                    continue;
                }
            }
        }
        // Discord allows about five updates per 20 seconds.
        if last_sent.is_some_and(|t| t.elapsed() < Duration::from_secs(4)) {
            continue;
        }
        let Some(c) = conn.as_mut() else { continue };
        match c.set(want.as_ref()) {
            Ok(None) => {
                dirty = false;
                last_sent = Some(Instant::now());
                set_link(Link::Connected(want.is_some()));
            }
            Ok(Some(refused)) => {
                dirty = false;
                last_sent = Some(Instant::now());
                set_link(Link::Refused(refused));
            }
            Err(_) => {
                conn = None;
                set_link(Link::Looking);
                next_try = Instant::now() + Duration::from_secs(5);
            }
        }
    }
}

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

/// A connection to the Discord app.
struct Conn {
    pipe: Box<dyn Pipe>,
    nonce: u64,
}

impl Conn {
    /// Connects to the first Discord app that answers, or says why none did.
    fn open() -> Result<Conn, Link> {
        let mut why = Link::NotRunning;
        for path in candidates() {
            let pipe = match connect(&path) {
                Ok(p) => p,
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    why = Link::Blocked;
                    continue;
                }
                Err(_) => continue,
            };
            let mut c = Conn { pipe, nonce: 0 };
            match c.handshake() {
                Ok(()) => return Ok(c),
                Err(e) => why = Link::Refused(e.to_string()),
            }
        }
        Err(why)
    }

    fn send(&mut self, op: u32, payload: &Value) -> std::io::Result<()> {
        let body = serde_json::to_vec(payload)?;
        let mut frame = Vec::with_capacity(8 + body.len());
        frame.extend_from_slice(&op.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(&body);
        self.pipe.write_all(&frame)?;
        self.pipe.flush()
    }

    fn recv(&mut self) -> std::io::Result<(u32, Value)> {
        let mut head = [0u8; 8];
        self.pipe.read_exact(&mut head)?;
        let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if len > 64 * 1024 {
            return Err(std::io::Error::other("Discord sent too much"));
        }
        let mut body = vec![0u8; len];
        self.pipe.read_exact(&mut body)?;
        Ok((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
    }

    fn handshake(&mut self) -> std::io::Result<()> {
        self.send(0, &json!({ "v": 1, "client_id": app_id() }))?;
        let (op, v) = self.recv()?;
        if op == 1 && v["evt"] == "READY" {
            return Ok(());
        }
        // Discord closes the connection with a reason ("Invalid Client ID", ...).
        let why = v["message"].as_str().or(v["data"]["message"].as_str()).unwrap_or("it didn't accept the connection");
        crate::crash::note(&format!("Discord: {why}"));
        Err(std::io::Error::other(why.to_string()))
    }

    /// Sends the status. Ok(Some(reason)) if Discord didn't take it.
    fn set(&mut self, song: Option<&Song>) -> std::io::Result<Option<String>> {
        self.nonce += 1;
        let activity = song.map_or(Value::Null, activity);
        self.send(1, &json!({ "cmd": "SET_ACTIVITY", "args": { "pid": std::process::id(), "activity": activity }, "nonce": self.nonce.to_string() }))?;
        let (op, v) = self.recv()?;
        if op == 2 {
            return Err(std::io::Error::other("Discord closed the connection"));
        }
        if v["evt"] == "ERROR" {
            let why = v["data"]["message"].as_str().unwrap_or("unknown reason").to_string();
            crate::crash::note(&format!("Discord didn't take the status: {why}"));
            return Ok(Some(why));
        }
        Ok(None)
    }
}

/// Discord wants 2 to 128 characters in its text fields.
fn clip(s: &str) -> String {
    let mut out: String = s.chars().take(128).collect();
    while out.chars().count() < 2 {
        out.push(' ');
    }
    out
}

fn activity(s: &Song) -> Value {
    let mut a = json!({
        "type": 2, // "Listening to"
        "details": clip(&s.title),
        "state": clip(&s.artists),
        "assets": {
            "large_image": s.cover.as_deref().unwrap_or(LOGO),
            "large_text": clip(s.album.as_deref().unwrap_or(&s.title)),
            "small_image": LOGO,
            "small_text": if s.times.is_some() { "Pulse" } else { "Paused" },
        },
    });
    if let Some((start, end)) = s.times {
        a["timestamps"] = json!({ "start": start, "end": end });
    }
    a
}

/// The places the Discord app listens, in the order it picks them.
fn candidates() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    out.extend((0..10).map(|i| std::path::PathBuf::from(format!(r"\\.\pipe\discord-ipc-{i}"))));
    #[cfg(unix)]
    {
        let dirs: Vec<std::path::PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
            .iter()
            .filter_map(|v| std::env::var_os(v).map(Into::into))
            .chain([std::path::PathBuf::from("/tmp")])
            .collect();
        for dir in dirs {
            for sub in ["", "app/com.discordapp.Discord", "snap.discord"] {
                out.extend((0..10).map(|i| dir.join(sub).join(format!("discord-ipc-{i}"))));
            }
        }
    }
    out
}

fn connect(path: &std::path::Path) -> std::io::Result<Box<dyn Pipe>> {
    #[cfg(windows)]
    {
        std::fs::OpenOptions::new().read(true).write(true).open(path).map(|f| Box::new(f) as Box<dyn Pipe>)
    }
    #[cfg(unix)]
    {
        let s = std::os::unix::net::UnixStream::connect(path)?;
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        Ok(Box::new(s))
    }
}

/// What the player is doing, as a status. `position` is how far into the song it is.
pub fn song_status(t: &crate::deezer::Track, playing: bool, position: Duration) -> Song {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    let times = (playing && t.duration > 0).then(|| {
        let start = now - position.as_millis() as i64;
        (start, start + i64::from(t.duration) * 1000)
    });
    Song {
        title: t.title.clone(),
        artists: t.all_artists(),
        album: t.album.as_ref().map(|a| a.title.clone()).filter(|a| !a.is_empty()),
        cover: t.art_big().filter(|u| u.starts_with("https://")).map(str::to_owned),
        times,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_drifts_dont_count_as_changes() {
        let a = Song { title: "Baby".into(), artists: "Lil Baby".into(), album: None, cover: None, times: Some((10_000, 100_000)) };
        let drift = Song { times: Some((10_400, 100_400)), ..a.clone() };
        let seek = Song { times: Some((40_000, 130_000)), ..a.clone() };
        let paused = Song { times: None, ..a.clone() };
        assert!(a.same_as(&drift));
        assert!(!a.same_as(&seek));
        assert!(!a.same_as(&paused));
    }

    #[test]
    fn text_fits_discords_limits() {
        assert_eq!(clip("A").chars().count(), 2);
        assert_eq!(clip(&"x".repeat(300)).chars().count(), 128);
        let a = activity(&Song { title: "A".into(), artists: "B".into(), album: None, cover: None, times: None });
        assert_eq!(a["type"], 2);
        assert_eq!(a["assets"]["small_text"], "Paused");
        assert!(a.get("timestamps").is_none());
    }
}
