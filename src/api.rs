//! A small client for the free Audius REST API (https://docs.audius.co/api).
//!
//! Every function here blocks while it waits for the network, so the app
//! always calls them from a background thread (see `app.rs`), never from the
//! UI thread. That keeps the window smooth even on a slow connection.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;

const BASE: &str = "https://api.audius.co/v1";
/// Audius asks every app to identify itself with an `app_name`.
pub const APP_NAME: &str = "pulse-rust-player";

/// One shared HTTP client (re-uses connections, which makes requests faster).
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent("Pulse/0.1 (Rust desktop music player)")
            .build()
            .into()
    })
}

// ---------------------------------------------------------------------------
// Data types. `#[serde(default)]` means "if the field is missing, use an
// empty value instead of failing", which makes us robust to API changes.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Artwork {
    #[serde(rename = "150x150")]
    pub small: Option<String>,
    #[serde(rename = "480x480")]
    pub medium: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CoverPhoto {
    #[serde(rename = "640x")]
    pub small: Option<String>,
    #[serde(rename = "2000x")]
    pub large: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct User {
    pub id: String,
    pub name: String,
    pub is_verified: bool,
    pub follower_count: u64,
    pub track_count: u64,
    pub bio: Option<String>,
    pub location: Option<String>,
    pub profile_picture: Option<Artwork>,
    pub cover_photo: Option<CoverPhoto>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Track {
    pub id: String,
    pub title: String,
    /// Length in seconds.
    pub duration: u32,
    pub genre: Option<String>,
    pub play_count: u64,
    pub artwork: Option<Artwork>,
    pub user: User,
    pub is_streamable: Option<bool>,
    pub is_stream_gated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Playlist {
    pub id: String,
    pub playlist_name: String,
    pub is_album: bool,
    pub artwork: Option<Artwork>,
    pub user: User,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

impl Track {
    /// URL of the audio. Audius redirects this to the fastest server that has the file.
    pub fn stream_url(&self) -> String {
        format!("{BASE}/tracks/{}/stream?app_name={APP_NAME}", self.id)
    }

    pub fn art_small(&self) -> Option<&str> {
        self.artwork.as_ref().and_then(|a| a.small.as_deref().or(a.medium.as_deref()))
    }

    /// Some tracks are locked (pay-to-unlock, etc). We hide those.
    pub fn is_playable(&self) -> bool {
        self.is_streamable.unwrap_or(true) && !self.is_stream_gated && !self.id.is_empty()
    }
}

impl User {
    pub fn avatar(&self) -> Option<&str> {
        self.profile_picture.as_ref().and_then(|a| a.medium.as_deref().or(a.small.as_deref()))
    }
    pub fn banner(&self) -> Option<&str> {
        self.cover_photo.as_ref().and_then(|c| c.large.as_deref().or(c.small.as_deref()))
    }
}

impl Playlist {
    pub fn art(&self) -> Option<&str> {
        self.artwork.as_ref().and_then(|a| a.medium.as_deref().or(a.small.as_deref()))
    }
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

fn get<T: for<'de> Deserialize<'de>>(path: &str, query: &[(&str, &str)]) -> Result<T, String> {
    // Developer testing only: read saved JSON files instead of the network.
    #[cfg(debug_assertions)]
    if let Ok(dir) = std::env::var("PULSE_FIXTURES") {
        let file = format!("{dir}/{}.json", path.replace('/', "_"));
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
        return serde_json::from_str::<Envelope<T>>(&text).map(|e| e.data).map_err(|e| e.to_string());
    }
    let mut req = agent().get(format!("{BASE}{path}")).query("app_name", APP_NAME);
    for (k, v) in query {
        req = req.query(*k, *v);
    }
    let mut resp = req.call().map_err(friendly_error)?;
    let env: Envelope<T> = resp
        .body_mut()
        .with_config()
        .limit(50 * 1024 * 1024)
        .read_json()
        .map_err(|e| format!("Audius sent something unexpected ({e})"))?;
    Ok(env.data)
}

fn friendly_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(429) => "Audius is rate-limiting us. Wait a few seconds and retry.".into(),
        ureq::Error::StatusCode(code) => format!("Audius returned an error (HTTP {code})."),
        ureq::Error::Timeout(_) => "The request timed out. Check your internet connection.".into(),
        other => format!("Couldn't reach Audius: {other}"),
    }
}

fn playable(mut tracks: Vec<Track>) -> Vec<Track> {
    tracks.retain(Track::is_playable);
    tracks
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeRange {
    Week,
    Month,
    Year,
    AllTime,
}

impl TimeRange {
    pub const ALL: [TimeRange; 4] = [Self::Week, Self::Month, Self::Year, Self::AllTime];
    fn param(self) -> &'static str {
        match self {
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
            Self::AllTime => "allTime",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Week => "This week",
            Self::Month => "This month",
            Self::Year => "This year",
            Self::AllTime => "All time",
        }
    }
}

pub fn trending_tracks(genre: Option<&str>, time: TimeRange, limit: u32) -> Result<Vec<Track>, String> {
    let limit = limit.to_string();
    let mut q = vec![("time", time.param()), ("limit", limit.as_str())];
    if let Some(g) = genre {
        q.push(("genre", g));
    }
    get("/tracks/trending", &q).map(playable)
}

pub fn underground_tracks(limit: u32) -> Result<Vec<Track>, String> {
    get("/tracks/trending/underground", &[("limit", &limit.to_string())]).map(playable)
}

pub fn trending_playlists(limit: u32) -> Result<Vec<Playlist>, String> {
    get("/playlists/trending", &[("limit", &limit.to_string()), ("time", "week")])
}

pub fn search_tracks(query: &str) -> Result<Vec<Track>, String> {
    get("/tracks/search", &[("query", query), ("limit", "40")]).map(playable)
}

pub fn search_users(query: &str) -> Result<Vec<User>, String> {
    get("/users/search", &[("query", query), ("limit", "12")])
}

pub fn search_playlists(query: &str) -> Result<Vec<Playlist>, String> {
    get("/playlists/search", &[("query", query), ("limit", "12")])
}

pub fn user(id: &str) -> Result<User, String> {
    get(&format!("/users/{id}"), &[])
}

pub fn user_tracks(id: &str) -> Result<Vec<Track>, String> {
    get(&format!("/users/{id}/tracks"), &[("limit", "50"), ("sort", "plays")]).map(playable)
}

pub fn playlist(id: &str) -> Result<Playlist, String> {
    // This endpoint returns a list with one playlist in it.
    let mut list: Vec<Playlist> = get(&format!("/playlists/{id}"), &[])?;
    list.pop().ok_or_else(|| "Playlist not found".to_string())
}

pub fn playlist_tracks(id: &str) -> Result<Vec<Track>, String> {
    get(&format!("/playlists/{id}/tracks"), &[]).map(playable)
}

/// Genres as Audius names them.
pub const GENRES: &[&str] = &[
    "Electronic", "Hip-Hop/Rap", "Alternative", "Pop", "R&B/Soul", "Lo-Fi", "House", "Techno",
    "Dubstep", "Drum & Bass", "Trap", "Rock", "Metal", "Ambient", "Jazz", "Experimental",
    "Hyperpop", "Latin", "Funk", "Acoustic", "Folk", "Soundtrack", "Classical", "Reggae",
];

/// Opens the audio stream for a track, starting `from` bytes in (used to resume
/// a dropped download). Returns the reader and the full file size if known.
pub fn open_stream(url: &str, from: u64) -> Result<(Box<dyn std::io::Read + Send>, Option<u64>), String> {
    let mut req = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .user_agent("Pulse/0.1 (Rust desktop music player)")
        .build()
        .new_agent()
        .get(url);
    if from > 0 {
        req = req.header("Range", format!("bytes={from}-"));
    }
    let resp = req.call().map_err(friendly_error)?;
    if from > 0 && resp.status() != 206 {
        return Err("server can't resume this download".into());
    }
    let len = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(|len| len + from);
    let reader = resp.into_body().into_with_config().limit(u64::MAX).reader();
    Ok((Box::new(reader), len))
}
