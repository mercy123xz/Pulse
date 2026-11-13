//! App state and logic: navigation, data loading, the play queue, likes and playlists.
//! The drawing code lives in `ui.rs`.

use crate::audio::{AudioEngine, LoadState, Resolver, StreamSource};
use crate::deezer::{self, AlbumInfo, ArtistPage, Chart, PlaylistInfo, SearchResults, Track};
use crate::widgets::{ShareTarget, TrackEvent};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Home,
    Charts,
    Search(String),
    Liked,
    /// Albums and playlists you saved with the heart on their page.
    LikedAlbums,
    LikedPlaylists,
    /// Artists you follow.
    FollowedArtists,
    Recent,
    Queue,
    Playlist(String),
    Album(String),
    Artist(String),
    Local(usize),
    LocalFiles,
    Settings,
    /// An Orbit: a mix built around one of your most-played artists (Deezer artist id).
    Mix(String),
    /// A song's own page (its id, "dz:…" or an imported song's id).
    Song(String),
    Profile,
    /// The playing song's lyrics.
    Lyrics,
    /// Versions, storage and a self-test, for when something isn't working.
    Diagnostics,
    /// A playlist someone made public (its id on the accounts server).
    Public(String),
    /// Someone's public profile (their username).
    User(String),
}

/// A song's lyrics, as far as we know.
pub enum LyricsState {
    Loading,
    Ready(Arc<crate::lyrics::Lyrics>),
    /// LRCLIB doesn't have them.
    Missing,
    Failed(String),
}

/// Data downloaded for a page.
pub enum Page {
    Home(Chart),
    Tracks(Vec<Track>),
    Search(SearchResults),
    Playlist { info: PlaylistInfo, tracks: Vec<Track> },
    Album { info: AlbumInfo, tracks: Vec<Track> },
    Artist(ArtistPage),
    Song(Box<deezer::SongPage>),
    Public(Box<crate::account::Public>),
    User(crate::account::Profile),
}

impl Page {
    /// Every song on the page (for updating details that arrive later).
    pub fn for_each_track(&mut self, mut f: impl FnMut(&mut Track)) {
        match self {
            Page::Home(c) => {
                c.tracks.iter_mut().for_each(&mut f);
                for row in &mut c.for_you {
                    if let deezer::Suggestion::SongsLike(_, t) = row {
                        t.iter_mut().for_each(&mut f);
                    }
                }
            }
            Page::Tracks(t) => t.iter_mut().for_each(&mut f),
            Page::Search(r) => {
                r.tracks.iter_mut().for_each(&mut f);
                if let Some((_, _, t)) = &mut r.around {
                    t.iter_mut().for_each(&mut f);
                }
            }
            Page::Playlist { tracks, .. } | Page::Album { tracks, .. } => tracks.iter_mut().for_each(&mut f),
            Page::Public(p) => p.tracks.iter_mut().for_each(&mut f),
            Page::User(_) => {}
            Page::Artist(p) => {
                p.top.iter_mut().for_each(&mut f);
                p.songs_like.iter_mut().for_each(&mut f);
            }
            Page::Song(p) => {
                f(&mut p.track);
                p.artist_top.iter_mut().for_each(&mut f);
                if let Some((_, t)) = &mut p.album {
                    t.iter_mut().for_each(&mut f);
                }
            }
        }
    }
}

pub enum Remote {
    Loading,
    Ready(Box<Page>),
    Failed(String),
}

/// Somewhere you went or something you played, for the home page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Place {
    Album(String),
    Artist(String),
    Playlist(String),
    /// One of your playlists, by name (positions change when playlists are deleted).
    Yours(String),
    Liked,
    Mix(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    pub place: Place,
    pub title: String,
    pub subtitle: String,
    pub image: Option<String>,
    pub round: bool,
    /// Times played or opened, and when last (Unix seconds).
    pub count: u32,
    pub last: u64,
}

/// Adds or moves `v` to the front of `list` (keeping at most `max`), counting it.
fn bump(list: &mut Vec<Visit>, mut v: Visit, max: usize) {
    if let Some(i) = list.iter().position(|x| x.place == v.place) {
        let old = list.remove(i);
        v.count = old.count + 1;
        if v.image.is_none() {
            v.image = old.image;
        }
    } else {
        v.count = 1;
    }
    list.insert(0, v);
    list.truncate(max);
}

/// Your most-played artists, best first: plays count, recent plays count more.
pub fn top_artists(artists: &[Visit], now: u64) -> Vec<&Visit> {
    let mut v: Vec<(f64, &Visit)> = artists
        .iter()
        .map(|a| {
            let days = now.saturating_sub(a.last) as f64 / 86_400.0;
            (a.count as f64 / (1.0 + days / 14.0), a)
        })
        .collect();
    v.sort_by(|a, b| b.0.total_cmp(&a.0));
    v.into_iter().map(|(_, a)| a).collect()
}

// ---------------------------------------------------------------------------
// Things saved between runs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalPlaylist {
    pub name: String,
    pub tracks: Vec<Track>,
    /// Your own cover picture (a copy kept in Pulse's data folder).
    #[serde(default)]
    pub cover: Option<std::path::PathBuf>,
    /// When each song was added: song id → Unix time in seconds.
    #[serde(default)]
    pub added: HashMap<String, u64>,
    /// Its id on the accounts server while it's public.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_id: Option<String>,
    /// What was last sent there (see `LocalPlaylist::fingerprint`).
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub published: u64,
}

fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}

impl LocalPlaylist {
    /// Changes when the name or songs change: a public playlist is sent again then.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.name.hash(&mut h);
        for t in &self.tracks {
            t.id.hash(&mut h);
            t.title.hash(&mut h);
        }
        h.finish().max(1)
    }
}

/// How a song list is filtered and sorted (the bar above playlists).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListView {
    pub search: String,
    pub search_open: bool,
    pub genre: Option<String>,
    pub sort: SortBy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortBy {
    #[default]
    Custom,
    Title,
    Artist,
    Album,
    RecentlyAdded,
    Duration,
}

impl SortBy {
    pub const ALL: [SortBy; 6] = [SortBy::Custom, SortBy::Title, SortBy::Artist, SortBy::Album, SortBy::RecentlyAdded, SortBy::Duration];

    pub fn label(self) -> &'static str {
        match self {
            SortBy::Custom => "Custom order",
            SortBy::Title => "Title",
            SortBy::Artist => "Artist",
            SortBy::Album => "Album",
            SortBy::RecentlyAdded => "Recently added",
            SortBy::Duration => "Duration",
        }
    }
}

/// A playlist import running in the background.
pub struct ImportJob {
    pub progress: Arc<Mutex<crate::import::Progress>>,
    pub cancel: Arc<std::sync::atomic::AtomicBool>,
    rx: Receiver<Result<crate::import::Imported, String>>,
}

/// Seconds since 1970, for "Date added".
pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Every typed word is found in the song's title, artists or album: as part of a word
/// ("lil" finds "Lil Baby"), or with one letter wrong in longer words ("beyonse").
pub fn track_matches(t: &Track, words: &[String]) -> bool {
    let mut hay = format!("{} {}", t.title, t.all_artists());
    if let Some(a) = &t.album {
        hay.push(' ');
        hay.push_str(&a.title);
    }
    let hay = fold(&hay);
    let hay_words: Vec<&str> = hay.split_whitespace().collect();
    words.iter().all(|w| hay.contains(w.as_str()) || (w.chars().count() >= 5 && hay_words.iter().any(|h| one_edit_apart(w, h))))
}

/// Lowercase, without accents, punctuation turned into spaces: "Beyoncé's" → "beyonce s".
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect()
}

/// True when `a` and `b` differ by at most one letter (added, removed or changed).
fn one_edit_apart(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (mut i, mut j, mut edits) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        match a.len().cmp(&b.len()) {
            std::cmp::Ordering::Greater => i += 1,
            std::cmp::Ordering::Less => j += 1,
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    edits + (a.len() - i) + (b.len() - j) <= 1
}

/// The songs of a list after search, genre filter and sorting.
pub fn apply_view(tracks: &[Track], view: &ListView, added: Option<&HashMap<String, u64>>, genres: &HashMap<String, Vec<String>>) -> Vec<Track> {
    let words: Vec<String> = fold(&view.search).split_whitespace().map(str::to_owned).collect();
    let mut out: Vec<Track> = tracks
        .iter()
        .filter(|t| words.is_empty() || track_matches(t, &words))
        .filter(|t| match &view.genre {
            None => true,
            Some(g) => track_genres(t, genres).iter().any(|x| x == g),
        })
        .cloned()
        .collect();
    let key = |s: &str| s.to_lowercase();
    match view.sort {
        SortBy::Custom => {}
        SortBy::Title => out.sort_by_key(|t| key(&t.title)),
        SortBy::Artist => out.sort_by_key(|t| (key(&t.artist.name), key(&t.title))),
        SortBy::Album => out.sort_by_key(|t| (t.album.as_ref().map(|a| key(&a.title)), key(&t.title))),
        SortBy::Duration => out.sort_by_key(|t| t.duration),
        SortBy::RecentlyAdded => {
            let when = |t: &Track| added.and_then(|a| a.get(&t.id)).copied().unwrap_or(0);
            out.sort_by_key(|t| std::cmp::Reverse(when(t)));
        }
    }
    out
}

/// A song's genres (from its album), if known.
pub fn track_genres<'a>(t: &Track, genres: &'a HashMap<String, Vec<String>>) -> &'a [String] {
    t.album.as_ref().and_then(|a| genres.get(&a.id)).map(Vec::as_slice).unwrap_or(&[])
}

/// The genres in a list, most common first (at most 15).
pub fn genre_chips(tracks: &[Track], genres: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut count: HashMap<&str, usize> = HashMap::new();
    for t in tracks {
        for g in track_genres(t, genres) {
            *count.entry(g.as_str()).or_default() += 1;
        }
    }
    let mut v: Vec<(&str, usize)> = count.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.into_iter().take(15).map(|(g, _)| g.to_string()).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub liked: Vec<Track>,
    pub recent: Vec<Track>,
    pub playlists: Vec<LocalPlaylist>,
    pub volume: f32,
    pub repeat: Repeat,
    pub shuffle: bool,
    pub last_track: Option<Track>,
    pub light_mode: bool,
    /// When the queue runs out, keep playing similar songs (like Spotify).
    pub autoplay: bool,
    /// Which YouTube video plays each song (song id → video id), so repeat plays skip the search.
    pub youtube_ids: HashMap<String, String>,
    pub settings: crate::settings::Settings,
    /// Downloaded songs: song id → file.
    pub downloads: HashMap<String, std::path::PathBuf>,
    /// When each liked song was liked (song id → Unix time).
    pub liked_added: HashMap<String, u64>,
    /// Genres of albums looked up so far (album id → genres), so it happens once.
    pub album_genres: HashMap<String, Vec<String>>,
    /// YouTube view counts: song id → (views, when looked up).
    pub views: HashMap<String, (u64, u64)>,
    /// Artists and albums you've played, most recent first (for the home page).
    pub played_artists: Vec<Visit>,
    pub played_albums: Vec<Visit>,
    /// Pages you opened, most recent first ("Recents" on the home page).
    pub visits: Vec<Visit>,
    pub profile: Profile,
    /// Songs whose featured artists were already looked up.
    pub featured_checked: HashSet<String>,
    /// The "create an account" window was shown when Pulse opened (it's shown once).
    pub account_asked: bool,
    /// Albums and playlists saved with the heart on their page, newest first.
    pub liked_albums: Vec<deezer::AlbumInfo>,
    pub liked_playlists: Vec<deezer::PlaylistInfo>,
    /// Artists you follow, newest first.
    pub followed_artists: Vec<deezer::ArtistInfo>,
    /// "Pick artists you like" was shown (or wasn't needed: you already had a history).
    pub artists_picked: bool,
    /// The first-launch intro played; later launches say "Welcome back".
    pub intro_seen: bool,
    /// First-time setup was finished (or wasn't needed: Pulse was in use before it existed).
    pub setup_done: bool,
    /// Artist pages found by name for songs that didn't know them (name → Deezer artist id).
    pub artist_ids: HashMap<String, String>,
    /// What you searched for lately, newest first.
    pub recent_searches: Vec<String>,
    /// The queue when Pulse was closed, where in it you were, and how far into the song.
    pub last_queue: Vec<Track>,
    pub last_queue_pos: usize,
    pub last_position: f32,
}

/// Your profile, kept on this computer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// A copy of your picture in Pulse's data folder.
    pub picture: Option<std::path::PathBuf>,
    pub bio: String,
    /// When the profile was made (Unix seconds); 0 = not made yet.
    pub created: u64,
    /// "Later" was pressed on the welcome screen.
    pub skipped: bool,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            liked: vec![],
            recent: vec![],
            playlists: vec![],
            volume: 0.8,
            repeat: Repeat::Off,
            shuffle: false,
            last_track: None,
            light_mode: false,
            autoplay: true,
            youtube_ids: HashMap::new(),
            settings: crate::settings::Settings::default(),
            downloads: HashMap::new(),
            liked_added: HashMap::new(),
            album_genres: HashMap::new(),
            views: HashMap::new(),
            played_artists: vec![],
            played_albums: vec![],
            visits: vec![],
            profile: Profile::default(),
            featured_checked: HashSet::new(),
            account_asked: false,
            liked_albums: vec![],
            liked_playlists: vec![],
            followed_artists: vec![],
            artists_picked: false,
            intro_seen: false,
            setup_done: false,
            artist_ids: HashMap::new(),
            recent_searches: vec![],
            last_queue: vec![],
            last_queue_pos: 0,
            last_position: 0.0,
        }
    }
}

const STORAGE_KEY: &str = "pulse_state_v1";

/// Local songs and downloaded songs play from a file on this computer.
fn offline_source(track: &Track, downloaded: Option<&std::path::Path>) -> Option<Result<StreamSource, String>> {
    if crate::local::is_local(&track.id) {
        return Some(crate::local::source(track));
    }
    let path = downloaded.filter(|p| p.is_file())?;
    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("m4a");
    Some(Ok(StreamSource {
        url: format!("file://{}", path.display()),
        headers: vec![],
        codec: crate::audio::Codec::from_extension(ext).unwrap_or(crate::audio::Codec::Mp4),
        content_length: None,
        notice: None,
        duration: None,
        keep_as: None,
    }))
}

/// Loads one song ahead of time: its audio link, then (when played songs are kept) the
/// song itself, so it plays from the disk.
fn warm_one(
    t: &Track,
    ids: &Arc<Mutex<HashMap<String, String>>>,
    prefetched: &Arc<Mutex<HashMap<String, (StreamSource, Instant)>>>,
    quality: crate::settings::Quality,
    failed: &Sender<String>,
    linked: &dyn Fn(),
) {
    let fresh = prefetched.lock().unwrap().get(&t.id).filter(|(_, at)| at.elapsed() < LINK_LIFETIME).map(|(s, _)| s.clone());
    let src = match fresh {
        Some(s) => s,
        None => {
            let known = ids.lock().unwrap().get(&t.id).cloned();
            match crate::youtube::resolve(t, known, quality, &|_| {}) {
                Ok((src, video)) => {
                    ids.lock().unwrap().insert(t.id.clone(), video);
                    let mut p = prefetched.lock().unwrap();
                    p.retain(|_, (_, at)| at.elapsed() < LINK_LIFETIME);
                    p.insert(t.id.clone(), (src.clone(), Instant::now()));
                    src
                }
                Err(_) => {
                    let _ = failed.send(t.id.clone());
                    return;
                }
            }
        }
    };
    // The link is ready: playing it no longer waits for the rest (the download ahead).
    linked();
    if !crate::audiocache::enabled() || crate::audiocache::has(&t.id) || src.url.starts_with("file://") {
        return;
    }
    let ext = if src.codec == crate::audio::Codec::Mp3 { "mp3" } else { "m4a" };
    keep_ahead(&src, &crate::audiocache::path_for(&t.id, ext));
}

/// Downloads a song into the kept-songs store at `dest`. False if it couldn't.
fn keep_ahead(src: &StreamSource, dest: &std::path::Path) -> bool {
    let Some(folder) = dest.parent() else {
        return false;
    };
    if std::fs::create_dir_all(folder).is_err() {
        return false;
    }
    let tmp = dest.with_extension("ahead");
    match crate::downloads::fetch(&src.url, &src.headers, src.content_length, &tmp, &|_| {}) {
        Ok(_) => crate::audiocache::adopt(&tmp, dest),
        Err(_) => {
            let _ = std::fs::remove_file(&tmp);
            false
        }
    }
}

/// Asks for a copy of the song to be kept once it has fully downloaded (see `audiocache`).
fn kept_for_next_time(mut src: StreamSource, track: &Track) -> StreamSource {
    if crate::audiocache::enabled() && src.keep_as.is_none() {
        let ext = if src.codec == crate::audio::Codec::Mp3 { "mp3" } else { "m4a" };
        src.keep_as = Some(crate::audiocache::path_for(&track.id, ext));
    }
    src
}

/// What a finished account call changes.
pub enum AccountDone {
    SignedIn(crate::account::Session),
    /// Who you are, checked when Pulse opens.
    Checked(crate::account::Session),
    /// A reset code was emailed: the window asks for it next.
    CodeSent(String),
    SignedOut,
    Deleted,
    Published {
        name: String,
        id: String,
        fingerprint: u64,
    },
    Synced {
        id: String,
        fingerprint: u64,
    },
    Unpublished {
        id: String,
    },
    /// Something else that worked: a message to show, and a page to load again.
    Done {
        message: String,
        reload: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountMode {
    SignIn,
    SignUp,
    Password,
    Delete,
    Bio,
    /// Forgot password: ask for a code by email.
    Forgot,
    /// Forgot password: the code and a new password.
    Reset,
    /// Add or change your email.
    Email,
}

/// The account window's fields.
pub struct AccountDialog {
    pub mode: AccountMode,
    pub username: String,
    pub password: String,
    /// The new password (Password), or the password again (SignUp).
    pub password2: String,
    pub bio: String,
    pub email: String,
    /// The code from a password-reset email.
    pub code: String,
    pub error: Option<String>,
    /// A message that isn't an error (the code was sent).
    pub note: Option<String>,
    /// Email: your account has none yet, and you need one to continue (or sign out).
    pub required: bool,
    /// Shown by itself when Pulse opens: worded as a welcome, with "Not now".
    pub welcome: bool,
}

impl AccountDialog {
    /// "Add an email": shown until the account has one (or you sign out).
    pub fn email_required() -> Self {
        let mut d = Self::new(AccountMode::Email);
        d.required = true;
        d
    }

    pub fn new(mode: AccountMode) -> Self {
        Self {
            mode,
            username: String::new(),
            password: String::new(),
            password2: String::new(),
            bio: String::new(),
            email: String::new(),
            code: String::new(),
            error: None,
            note: None,
            required: false,
            welcome: false,
        }
    }
}

/// Result of checking an imported song again: (its id, the Deezer song if found now,
/// else a cover from iTunes if there is one).
type Rematch = (String, Option<Track>, Option<String>);

/// A finished download: the song, and its file and YouTube video id (or what went wrong).
type DownloadResult = (Track, Result<(std::path::PathBuf, String), String>);

/// Folders with files Pulse can always download again.
pub fn cache_dirs() -> Vec<std::path::PathBuf> {
    vec![crate::youtube::tools_dir().join("cache"), crate::settings::data_dir().join("cache")]
}

/// How long a message stays up: 3.5 seconds, longer for long ones (time to read them).
pub fn toast_secs(msg: &str) -> f64 {
    3.5 + (msg.chars().count().saturating_sub(70) as f64 * 0.06).min(9.0)
}

/// Total size of the cache folders.
pub fn cache_size() -> u64 {
    cache_dirs().iter().map(|d| crate::settings::folder_size(d)).sum()
}

/// Builds a share link. Pulse knows the exact YouTube video once a song has played;
/// otherwise (and for Spotify, which Pulse doesn't use) the link opens a search.
pub fn share_link(t: &Track, target: ShareTarget, youtube_video: Option<&str>) -> String {
    let query = crate::net::encode(&format!("{} {}", t.artist.name, t.title));
    match target {
        ShareTarget::YouTube => match youtube_video {
            Some(v) => format!("https://www.youtube.com/watch?v={v}"),
            None => format!("https://www.youtube.com/results?search_query={query}"),
        },
        ShareTarget::Spotify => format!("https://open.spotify.com/search/{query}"),
        ShareTarget::Deezer => match t.deezer_number() {
            Some(n) => format!("https://www.deezer.com/track/{n}"),
            None => format!("https://www.deezer.com/search/{query}"),
        },
    }
}

/// YouTube audio links stop working after about six hours; reuse them for less.
const LINK_LIFETIME: Duration = Duration::from_secs(4 * 3600);

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

pub struct App {
    pub audio: AudioEngine,
    pub saved: Saved,
    pub liked_ids: HashSet<String>,

    // navigation
    pub route: Route,
    back: Vec<Route>,
    forward: Vec<Route>,
    pub cache: HashMap<String, Remote>,
    tx: Sender<(String, Result<Page, String>)>,
    rx: Receiver<(String, Result<Page, String>)>,

    // search box
    pub search_text: String,
    pub search_edited_at: Option<f64>,
    pub focus_search: bool,

    // charts filter: (Deezer genre id, name); 0 = all genres
    pub chart_genre: (u32, String),

    // playback
    pub queue: Vec<Track>,
    pub queue_pos: usize,
    unshuffled: Option<Vec<Track>>,
    pub current: Option<Track>,
    /// The current track is shown but hasn't been streamed yet (restored from last session).
    pub needs_load: bool,
    /// Where to pick the last song up after a restart: (song id, seconds in).
    resume_at: Option<(String, f32)>,
    pub seek_preview: Option<f32>,
    pub muted_volume: Option<f32>,
    failures_in_row: u32,
    last_title: String,

    // YouTube
    /// Shared with background threads; copied into `saved` when saving.
    youtube_ids: Arc<Mutex<HashMap<String, String>>>,
    /// Pages being loaded ahead of time (see `preload`).
    preloading: Arc<std::sync::atomic::AtomicUsize>,
    /// Audio links found ahead of time for upcoming songs (song id → link, found at).
    prefetched: Arc<Mutex<HashMap<String, (StreamSource, Instant)>>>,
    /// Songs being loaded ahead of time right now (see `warm`).
    prefetching: Arc<Mutex<HashSet<String>>>,
    /// Songs waiting to be loaded ahead of time, most wanted first.
    warm_queue: Arc<Mutex<VecDeque<Track>>>,
    /// True while the loading-ahead worker runs (there is only ever one).
    warm_busy: Arc<std::sync::atomic::AtomicBool>,
    /// The list whose first songs were last loaded ahead (their ids), so it's done once per visit.
    warmed_list: Vec<String>,
    /// Songs whose lookup failed ahead of time (not retried in the background).
    prefetch_failed: HashSet<String>,
    prefetch_failed_tx: Sender<String>,
    prefetch_failed_rx: Receiver<String>,

    // autoplay
    autoplay_rx: Option<Receiver<Vec<Track>>>,
    /// Song whose similar songs were last fetched (so it happens once per song).
    autoplay_for: Option<String>,
    /// Queue ran out while similar songs were still loading: play them when they arrive.
    advance_when_autoplay_arrives: bool,
    /// Songs added by autoplay (shown with a label in the queue).
    pub autoplay_ids: HashSet<String>,

    // overlays
    pub toast: Option<(String, f64)>,
    /// The "New playlist" window: its name so far and the songs to put in it.
    pub new_playlist: Option<(String, Vec<Track>)>,
    /// Selected songs: which list (its name), the rows, and where a Shift+click range starts.
    pub selection: (String, HashSet<usize>, Option<usize>),
    pub rename_playlist: Option<(usize, String)>,
    /// Playlist waiting for "are you sure?" before it is deleted.
    pub confirm_delete: Option<usize>,
    /// "View credits" window: the song and its credits once loaded.
    pub credits: Option<(Track, Option<Result<crate::deezer::Credits, String>>)>,
    credits_rx: Option<Receiver<Result<crate::deezer::Credits, String>>>,
    /// Featured-artist lookups: waiting, and results coming back.
    featured_queue: std::collections::VecDeque<u64>,
    featured_busy: bool,
    featured_tx: Sender<(String, Option<Vec<deezer::ArtistRef>>)>,
    featured_rx: Receiver<(String, Option<Vec<deezer::ArtistRef>>)>,
    /// Songs whose featured artists couldn't be looked up this time.
    featured_failed: HashSet<String>,
    /// Imported songs that weren't found on Deezer, checked again this session.
    rematch_tried: HashSet<String>,
    rematch_rx: Option<Receiver<Rematch>>,
    /// Songs whose page was opened, for songs that aren't on Deezer (drawn from what Pulse has).
    pub song_tracks: HashMap<String, Track>,
    /// A song to point out on the page it was opened on (id, when).
    pub highlight: Option<(String, f64)>,
    /// The song-colored background (themes with `backdrop = true`).
    pub backdrop: crate::backdrop::Backdrop,
    swept_at: f64,
    /// A crash report from last time that you haven't seen: (file, text).
    pub crash_notice: Option<(std::path::PathBuf, String)>,
    /// "Report a problem" is open: what you've written, and whether to add the diagnostics.
    pub report: Option<(String, bool)>,
    /// A report being sent: the answer arrives here.
    pub report_sending: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    /// Checked for an update since Pulse opened / asked about it (once per launch).
    pub update_checked_launch: bool,
    pub update_asked: bool,
    /// The day (days since 1970) the daily backup was last checked.
    backup_day: u64,
    /// Backups listed in Settings (read when the page opens), and one waiting to be
    /// confirmed before it replaces your library: (where it came from, the library).
    backups: Option<Vec<crate::backup::Info>>,
    backups_rx: Option<Receiver<Vec<crate::backup::Info>>>,
    backups_stale: bool,
    /// Save at the end of this frame instead of waiting for the next autosave.
    pub save_now: bool,
    /// The Diagnostics page's facts and self-test.
    pub diag: crate::diag::Diagnostics,
    /// Checks for, downloads and installs new versions.
    pub updater: crate::update::Updater,
    /// The "Listening to Pulse" status on Discord.
    discord: crate::discord::Presence,
    /// When the music was paused (the Discord status goes away after a while).
    paused_since: Option<f64>,
    pub pending_restore: Option<(String, Box<Saved>)>,
    /// Lyrics looked up this session, by song id.
    pub lyrics: HashMap<String, LyricsState>,
    lyrics_tx: Sender<(String, Result<Option<crate::lyrics::Lyrics>, String>)>,
    lyrics_rx: Receiver<(String, Result<Option<crate::lyrics::Lyrics>, String>)>,
    /// The sign-in / account window, when open.
    pub account_dialog: Option<AccountDialog>,
    account_tx: Sender<Result<AccountDone, String>>,
    account_rx: Receiver<Result<AccountDone, String>>,
    /// Account calls still running.
    pub account_busy: usize,
    /// Public playlists being sent again (their ids), and when public ones were last checked.
    syncing: HashSet<String>,
    synced_at: f64,
    /// What other people search for, by what was typed (filled in the background).
    pub popular_searches: HashMap<String, Vec<String>>,
    popular_rx: Option<(String, Receiver<Vec<String>>)>,
    /// An artist being looked up by name after you clicked it (the name, and the answer).
    artist_lookup: Option<(String, Receiver<Option<String>>)>,
    /// When each loaded page was last opened, so the oldest can be let go of.
    cache_used: HashMap<String, Instant>,
    /// Size of the cache folders, measured when Settings opens.
    pub cache_size: Option<u64>,
    /// The song last given to the audio engine as "next" (None = not told yet).
    told_next: Option<Option<String>>,

    // themes
    pub themes: Vec<crate::themes::Theme>,
    theme_watch: Option<(notify::RecommendedWatcher, Receiver<()>)>,
    theme_changed_at: Option<f64>,

    // local files
    pub local_files: Vec<Track>,
    pub local_scanning: bool,
    local_rx: Option<Receiver<Vec<Track>>>,

    // downloads
    pub download_queue: std::collections::VecDeque<Track>,
    /// The songs downloading now and how far along each is.
    pub downloading: Vec<(Track, Arc<Mutex<f32>>)>,
    download_tx: Sender<DownloadResult>,
    download_rx: Receiver<DownloadResult>,

    /// Search / genre / sort of the list on screen (reset when the page changes).
    pub list_view: ListView,

    /// The "Import a playlist" window: the link typed so far, and the running import.
    pub import_open: bool,
    pub import_url: String,
    pub import_running: Option<ImportJob>,
    /// The profile form, when open (first-launch welcome or "Edit profile").
    pub profile_editing: Option<crate::ui::ProfileForm>,
    /// The first-launch intro, or "Welcome back", while it shows.
    pub intro: Option<crate::ui::intro::Intro>,
    /// "Pick artists you like", while it's open.
    pub picker: Option<crate::ui::pick_artists::Picker>,
    /// First-time setup, while it's open (the first time Pulse opens on a computer).
    pub setup: Option<crate::ui::setup::Setup>,

    // YouTube view counts
    views_queue: std::collections::VecDeque<Track>,
    views_pending: HashSet<String>,
    /// Songs whose lookup found nothing this session (not asked again until restart).
    views_failed: HashSet<String>,
    views_tx: Sender<(String, Option<(String, u64)>)>,
    views_rx: Receiver<(String, Option<(String, u64)>)>,
    genre_rx: Option<Receiver<(String, Option<Vec<String>>)>>,
    /// Albums whose genres couldn't be looked up this time (offline, Deezer busy).
    genre_failed: HashSet<String>,
    /// Said once the window is up (the library had to be recovered).
    startup_note: Option<String>,
    genre_pending: HashSet<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::install(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        // Pictures from the web: saved on disk, let go of in memory when off screen.
        crate::artcache::ArtLoader::install(&cc.egui_ctx);

        // A damaged save file (cut off by a crash or power cut) is recovered from the spare
        // copy or the newest backup, rather than starting over with an empty library.
        let main_file = eframe::storage_dir("pulse-player").map(|d| d.join("app.ron"));
        let (saved, load_note) = crate::backup::load(cc.storage.and_then(|s| s.get_string(STORAGE_KEY)), main_file.as_deref());
        let themes = crate::themes::list();
        crate::i18n::set_language(saved.settings.language);
        crate::deezer::set_prefer_explicit(saved.settings.explicit);
        crate::perf::apply(saved.settings.perf_mode, &saved.settings.perf_keep);
        crate::theme::set_reduce_motion(saved.settings.reduce_motion || !crate::perf::on(crate::perf::Extra::Animations));
        let (tx, rx) = channel();
        let (views_tx, views_rx) = channel();
        let (download_tx, download_rx) = channel();
        let (account_tx, account_rx) = channel();
        let (prefetch_failed_tx, prefetch_failed_rx) = channel();
        let (featured_tx, featured_rx) = channel();
        let (lyrics_tx, lyrics_rx) = channel();
        let audio = AudioEngine::new();
        audio.set_volume(saved.volume);

        let youtube_ids = Arc::new(Mutex::new(saved.youtube_ids.clone()));
        let mut app = Self {
            audio,
            liked_ids: saved.liked.iter().map(|t| t.id.clone()).collect(),
            current: saved.last_track.clone(),
            needs_load: saved.last_track.is_some(),
            // The queue comes back as it was, if it still holds the last song.
            queue: match &saved.last_track {
                Some(t) if saved.last_queue.get(saved.last_queue_pos).is_some_and(|q| q.id == t.id) => saved.last_queue.clone(),
                Some(t) => vec![t.clone()],
                None => vec![],
            },
            resume_at: saved.last_track.as_ref().filter(|_| saved.last_position > 5.0).map(|t| (t.id.clone(), saved.last_position)),
            saved,
            route: Route::Home,
            back: vec![],
            forward: vec![],
            cache: HashMap::new(),
            tx,
            rx,
            search_text: String::new(),
            search_edited_at: None,
            focus_search: false,
            chart_genre: (0, "All genres".into()),
            queue_pos: 0, // set just below
            unshuffled: None,
            seek_preview: None,
            muted_volume: None,
            failures_in_row: 0,
            last_title: String::new(),
            youtube_ids,
            preloading: Default::default(),
            prefetched: Arc::new(Mutex::new(HashMap::new())),
            prefetching: Arc::new(Mutex::new(HashSet::new())),
            warm_queue: Default::default(),
            warm_busy: Default::default(),
            warmed_list: vec![],
            prefetch_failed: HashSet::new(),
            prefetch_failed_tx,
            prefetch_failed_rx,
            autoplay_rx: None,
            autoplay_for: None,
            advance_when_autoplay_arrives: false,
            autoplay_ids: HashSet::new(),
            toast: None,
            new_playlist: None,
            selection: (String::new(), HashSet::new(), None),
            rename_playlist: None,
            confirm_delete: None,
            credits: None,
            credits_rx: None,
            cache_size: None,
            highlight: None,
            backdrop: Default::default(),
            swept_at: 0.0,
            crash_notice: crate::crash::unseen(),
            report: None,
            report_sending: None,
            update_checked_launch: false,
            update_asked: false,
            backup_day: 0,
            backups: None,
            backups_rx: None,
            backups_stale: false,
            save_now: false,
            diag: Default::default(),
            updater: Default::default(),
            discord: Default::default(),
            paused_since: None,
            pending_restore: None,
            lyrics: HashMap::new(),
            lyrics_tx,
            lyrics_rx,
            artist_lookup: None,
            popular_searches: HashMap::new(),
            account_dialog: None,
            account_tx,
            account_rx,
            account_busy: 0,
            syncing: HashSet::new(),
            synced_at: 0.0,
            popular_rx: None,
            cache_used: HashMap::new(),
            song_tracks: HashMap::new(),
            rematch_tried: HashSet::new(),
            featured_queue: Default::default(),
            featured_busy: false,
            featured_tx,
            featured_rx,
            featured_failed: HashSet::new(),
            rematch_rx: None,
            told_next: None,
            themes,
            theme_watch: crate::themes::watch(&cc.egui_ctx),
            theme_changed_at: None,
            local_files: vec![],
            local_scanning: false,
            local_rx: None,
            download_queue: Default::default(),
            downloading: vec![],
            download_tx,
            download_rx,
            list_view: ListView::default(),
            import_open: false,
            import_url: String::new(),
            import_running: None,
            profile_editing: None,
            views_queue: Default::default(),
            views_pending: HashSet::new(),
            views_failed: HashSet::new(),
            views_tx,
            views_rx,
            genre_rx: None,
            intro: None,
            picker: None,
            setup: None,
            genre_failed: HashSet::new(),
            startup_note: None,
            genre_pending: HashSet::new(),
        };
        // Pulse 0.2 had only a light/dark switch: carry it over to the Default theme.
        if app.saved.light_mode && app.saved.settings.theme == "Default" && app.saved.settings.scheme.is_empty() {
            app.saved.settings.scheme = "Light".into();
        }
        if app.queue.len() > 1 {
            app.queue_pos = app.saved.last_queue_pos.min(app.queue.len() - 1);
        }
        crate::single::listen(cc.egui_ctx.clone());
        app.apply_perf(&cc.egui_ctx);
        crate::artcache::set_limit_mb(app.saved.settings.art_cache_mb);
        crate::audiocache::set_limit_mb(app.saved.settings.replay_cache_mb);
        std::thread::spawn(crate::artcache::trim_disk);
        // Signed in with a profile picture your public profile doesn't have yet (made before
        // pictures went online, or on another PC): put it up.
        if let (Some(s), Some(pic)) = (crate::account::session(), app.saved.profile.picture.clone()) {
            std::thread::spawn(move || {
                if let Ok(p) = crate::account::profile(&s.username)
                    && p.user.avatar.is_none()
                {
                    let _ = crate::account::set_avatar(&pic);
                }
            });
        }
        // Real lengths for saved songs Deezer lists only as a clip.
        let clips: Vec<String> = app
            .saved
            .liked
            .iter()
            .chain(&app.saved.recent)
            .chain(app.saved.playlists.iter().flat_map(|p| &p.tracks))
            .chain(&app.queue)
            .filter(|t| crate::fixes::is_clip(t))
            .map(|t| t.id.clone())
            .collect();
        if !clips.is_empty() {
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                crate::fixes::lookup(&clips);
                ctx.request_repaint();
            });
        }
        app.apply_theme(&cc.egui_ctx);
        app.startup_note = load_note;
        // The first time Pulse opens on a computer: first-time setup, then the intro.
        // Anyone who used Pulse before setup existed doesn't get it.
        let fresh = !app.saved.intro_seen && app.saved.profile.created == 0 && app.saved.followed_artists.is_empty();
        if !app.saved.setup_done && fresh {
            app.setup = Some(crate::ui::setup::Setup::new());
            // Setup covers the account and "Pick artists you like" windows.
            app.saved.account_asked = true;
            app.saved.artists_picked = true;
            app.saved.profile.skipped = true;
        } else {
            app.saved.setup_done = true;
        }
        // The intro every time Pulse opens, followed by "Welcome back" after the first time.
        if app.setup.is_none() {
            let name = match crate::account::session() {
                Some(s) => format!("@{}", s.username),
                None => app.saved.profile.name.trim().to_string(),
            };
            // Only "Reduce motion" skips it; Gaming and Ultra-low modes still play it.
            let still = app.saved.settings.reduce_motion;
            app.intro = Some(crate::ui::intro::Intro::for_launch(!app.saved.intro_seen, still, name));
            if still {
                app.saved.intro_seen = true;
            }
        }
        // The first time Pulse opens (and once after updating): offer an account. The local
        // profile window comes after, if you choose "Not now".
        if app.setup.is_some() {
            // First-time setup asks about the account itself.
        } else if crate::account::available() && crate::account::session().is_none() && !app.saved.account_asked {
            app.saved.account_asked = true;
            let mut d = AccountDialog::new(AccountMode::SignUp);
            d.welcome = true;
            app.account_dialog = Some(d);
        } else if crate::account::available() && crate::account::session().is_some() {
            // Signed in: check who you are, and ask for an email if the account has none.
            app.account_job(&cc.egui_ctx, || crate::account::refresh().map(AccountDone::Checked));
        } else if app.saved.profile.created == 0 && !app.saved.profile.skipped {
            app.profile_editing = Some(crate::ui::ProfileForm { welcome: true, ..Default::default() });
        }
        // Forget downloads whose files were deleted outside Pulse.
        app.saved.downloads.retain(|_, path| path.is_file());
        app.audio.set_effects(crate::audio::Effects::from_settings(&app.saved.settings));
        if app.saved.settings.show_local_files {
            app.scan_local_files(&cc.egui_ctx);
        }
        // The window was created before settings loaded; keep the setting in step with how it opened.
        app.saved.settings.hardware_acceleration = crate::settings::hardware_acceleration_enabled();
        if let Some(err) = app.audio.device_error.clone() {
            app.toast(&err, &cc.egui_ctx);
        }
        // Developer check: PULSE_DEBUG_OPEN=playlist:0 (or liked, recent, queue, settings) starts on that page.
        if let Ok(page) = std::env::var("PULSE_DEBUG_OPEN") {
            app.route = match page.as_str() {
                "liked" => Route::Liked,
                "recent" => Route::Recent,
                "queue" => Route::Queue,
                "settings" => Route::Settings,
                "liked-albums" => Route::LikedAlbums,
                "intro" | "welcome" => {
                    app.intro = Some(crate::ui::intro::Intro::for_launch(page == "intro", false, "@mercy".into()));
                    Route::Home
                }
                "pick-artists" => {
                    crate::ui::pick_artists::open(&mut app, &cc.egui_ctx);
                    Route::Home
                }
                "signup" => {
                    app.account_dialog = Some(AccountDialog::new(AccountMode::SignUp));
                    Route::Home
                }
                "signin" | "forgot" | "reset" | "add-email" => {
                    let mut d = match page.as_str() {
                        "signin" => AccountDialog::new(AccountMode::SignIn),
                        "forgot" => AccountDialog::new(AccountMode::Forgot),
                        "reset" => AccountDialog::new(AccountMode::Reset),
                        _ => AccountDialog::email_required(),
                    };
                    if page == "reset" {
                        d.note =
                            Some("If that account has an email address, a code is on its way. Check your spam folder if it doesn't arrive in a minute.".into());
                    }
                    app.account_dialog = Some(d);
                    Route::Home
                }
                p if p.starts_with("public:") => Route::Public(p[7..].to_string()),
                p if p.starts_with("user:") => Route::User(p[5..].to_string()),
                "import" => {
                    app.import_open = true;
                    app.import_url = std::env::var("PULSE_DEBUG_IMPORT").unwrap_or_default();
                    Route::Home
                }
                p if p.starts_with("artist:") => Route::Artist(p[7..].to_string()),
                p if p.starts_with("search:") => {
                    app.search_text = p[7..].to_string();
                    app.focus_search = true;
                    Route::Search(p[7..].to_string())
                }
                p => p.strip_prefix("playlist:").and_then(|n| n.parse().ok()).map(Route::Local).unwrap_or(Route::Home),
            };
        }
        app.ensure_loaded(&cc.egui_ctx);
        app
    }

    pub fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.saved.last_track = self.current.clone();
        // The queue, kept to a few thousand songs around where you are.
        let from = self.queue_pos.saturating_sub(500);
        self.saved.last_queue = self.queue.iter().skip(from).take(3000).cloned().collect();
        self.saved.last_queue_pos = self.queue_pos - from;
        self.saved.last_position = match &self.resume_at {
            // Not started again since the restart: keep the old spot.
            Some((_, secs)) if self.needs_load => *secs,
            _ if self.needs_load => 0.0,
            _ => self.audio.position().as_secs_f32(),
        };
        self.trim_saved();
        // Once a day, a backup of your library (in the background: it's a file write).
        let day = crate::crash::now() / 86_400;
        if day != self.backup_day {
            self.backup_day = day;
            let lib = self.saved.clone();
            std::thread::spawn(move || {
                crate::backup::daily(&lib);
            });
        }
        let mut ids = self.youtube_ids.lock().unwrap().clone();
        if ids.len() > 5000 {
            ids.clear(); // keep the saved file small; ids are found again when needed
        }
        self.saved.youtube_ids = ids;
        match ron::ser::to_string(&self.saved) {
            Ok(text) => {
                crate::backup::write_spare(&text);
                storage.set_string(STORAGE_KEY, text);
            }
            Err(e) => eprintln!("couldn't save the library: {e}"),
        }
    }

    /// Keeps the lookups remembered between runs from growing without end.
    fn trim_saved(&mut self) {
        let s = &mut self.saved;
        if s.views.len() > 6000 {
            // Keep the most recently looked-up counts.
            let mut v: Vec<(String, (u64, u64))> = s.views.drain().collect();
            v.sort_by_key(|(_, (_, at))| std::cmp::Reverse(*at));
            v.truncate(4000);
            s.views = v.into_iter().collect();
        }
        if s.featured_checked.len() > 20000 {
            s.featured_checked.clear(); // only means a few lookups happen again
        }
        if s.album_genres.len() > 5000 {
            s.album_genres.clear();
        }
    }

    // ---------------------------------------------------------------- navigation

    pub fn navigate(&mut self, route: Route, ctx: &egui::Context) {
        // Your own public profile is part of your profile page (one page, not two).
        let route = match route {
            Route::User(name) if crate::account::session().is_some_and(|s| s.username == name) => Route::Profile,
            r => r,
        };
        if route == self.route {
            return;
        }
        // The secret theme: search for it, and there it is.
        if let Route::Search(q) = &route
            && crate::themes::finds_secret(q)
            && !self.saved.settings.found_secret_theme
        {
            self.saved.settings.found_secret_theme = true;
            self.saved.settings.theme = crate::themes::SECRET_THEME.into();
            self.saved.settings.scheme.clear();
            self.apply_theme(ctx);
            self.save_now = true;
            self.toast(crate::i18n::tr("Tung tung tung… Sahur! You found a secret theme. It's in Settings → Themes now."), ctx);
        }
        let old = std::mem::replace(&mut self.route, route);
        // Leaving search for something you found there: remember the search.
        if let (Route::Search(q), false) = (&old, matches!(self.route, Route::Search(_))) {
            self.remember_search(&q.clone());
        }
        // Typing in search replaces the search page instead of piling up history.
        if !matches!((&old, &self.route), (Route::Search(_), Route::Search(_))) {
            self.back.push(old);
        }
        self.forward.clear();
        self.list_view = ListView::default();
        self.highlight = None;
        self.selection.1.clear();
        self.featured_queue.clear();
        // Lookups for the page we left aren't needed any more.
        for t in self.views_queue.drain(..) {
            self.views_pending.remove(&t.id);
        }
        if self.route == Route::Settings {
            self.cache_size = None;
        }
        self.ensure_loaded(ctx);
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }
    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
    pub fn go_back(&mut self, ctx: &egui::Context) {
        if let Some(r) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.route, r));
            self.ensure_loaded(ctx);
        }
    }
    pub fn go_forward(&mut self, ctx: &egui::Context) {
        if let Some(r) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.route, r));
            self.ensure_loaded(ctx);
        }
    }

    /// The cache key for the current page's data (None for pages with only local data).
    pub fn route_key(&self) -> Option<String> {
        self.key_for(&self.route)
    }

    fn key_for(&self, route: &Route) -> Option<String> {
        Some(match route {
            Route::Home => "home".into(),
            Route::Charts => format!("charts:{}", self.chart_genre.0),
            Route::Search(q) => format!("search:{q}"),
            Route::Playlist(id) => format!("playlist:{id}"),
            Route::Album(id) => format!("album:{id}"),
            Route::Artist(id) => format!("artist:{id}"),
            Route::Mix(id) => format!("mix:{id}"),
            Route::Public(id) => format!("public:{id}"),
            Route::User(name) => format!("user:{name}"),
            Route::Song(id) if deezer::is_deezer_id(id.trim_start_matches("dz:")) => {
                format!("song:{id}")
            }
            Route::Song(_) => return None,
            Route::Liked
            | Route::LikedAlbums
            | Route::LikedPlaylists
            | Route::FollowedArtists
            | Route::Recent
            | Route::Queue
            | Route::Local(_)
            | Route::LocalFiles
            | Route::Settings
            | Route::Profile
            | Route::Lyrics
            | Route::Diagnostics => return None,
        })
    }

    /// Starts downloading the current page's data if we don't have it yet.
    pub fn ensure_loaded(&mut self, ctx: &egui::Context) {
        if let Some(k) = self.route_key() {
            self.cache_used.insert(k, Instant::now());
        }
        self.load_route(self.route.clone(), ctx);
    }

    /// Keeps at most 60 loaded pages in memory: the ones opened longest ago go first
    /// (they load again if you go back to them).
    fn trim_page_cache(&mut self) {
        let keep = if crate::perf::mode() == crate::perf::Mode::UltraLow { 20 } else { 60 };
        if self.cache.len() <= keep {
            return;
        }
        let current = self.route_key();
        let back: HashSet<String> = self.back.iter().rev().take(10).filter_map(|r| self.key_for(r)).collect();
        let mut keys: Vec<(String, Option<Instant>)> = self
            .cache
            .iter()
            .filter(|(k, v)| !matches!(v, Remote::Loading) && Some(*k) != current.as_ref() && !back.contains(*k))
            .map(|(k, _)| (k.clone(), self.cache_used.get(k).copied()))
            .collect();
        // Never-opened (loaded ahead) pages first, then the oldest.
        keys.sort_by_key(|(_, at)| *at);
        let extra = self.cache.len() - keep;
        for (k, _) in keys.into_iter().take(extra) {
            self.cache.remove(&k);
            self.cache_used.remove(&k);
        }
    }

    /// Loads a page you're likely to open next (pointing at its card or song), so it opens
    /// at once. Pages already loaded or loading aren't asked for again, and only a couple
    /// of these run at a time.
    pub fn preload(&mut self, route: Route, ctx: &egui::Context) {
        if !crate::perf::on(crate::perf::Extra::Preloading) {
            return;
        }
        if self.preloading.load(std::sync::atomic::Ordering::Relaxed) >= 2 {
            return;
        }
        if self.key_for(&route).is_some_and(|k| self.cache.contains_key(&k)) {
            return;
        }
        self.load_route_with(route, ctx, true);
    }

    fn load_route(&mut self, route: Route, ctx: &egui::Context) {
        self.load_route_with(route, ctx, false);
    }

    fn load_route_with(&mut self, route: Route, ctx: &egui::Context, preload: bool) {
        let Some(key) = self.key_for(&route) else {
            return;
        };
        // Opening a page that failed before (perhaps while loading ahead) tries again.
        if !preload && matches!(self.cache.get(&key), Some(Remote::Failed(_))) {
            self.cache.remove(&key);
        }
        if self.cache.contains_key(&key) {
            return;
        }
        let job: Box<dyn FnOnce() -> Result<Page, String> + Send> = match route {
            Route::Home => {
                // Rows built from what you play: your two top artists and the last two songs
                // you played (by different artists).
                let now = now_unix();
                let artists: Vec<(String, String)> = top_artists(&self.saved.played_artists, now)
                    .into_iter()
                    .filter_map(|v| match &v.place {
                        Place::Artist(id) if deezer::is_deezer_id(id) => Some((id.clone(), v.title.clone())),
                        _ => None,
                    })
                    .take(2)
                    .collect();
                // Not much history yet: the artists you follow (or picked when you started)
                // fill in, a different pair each day.
                let mut artists = artists;
                let followed = &self.saved.followed_artists;
                if artists.len() < 2 && !followed.is_empty() {
                    let start = (now / 86_400) as usize % followed.len();
                    for a in followed.iter().cycle().skip(start).take(followed.len()) {
                        if artists.len() == 2 {
                            break;
                        }
                        if deezer::is_deezer_id(&a.id) && !artists.iter().any(|(id, _)| *id == a.id) {
                            artists.push((a.id.clone(), a.name.clone()));
                        }
                    }
                }
                let mut songs: Vec<Track> = vec![];
                for t in &self.saved.recent {
                    if songs.len() == 2 {
                        break;
                    }
                    if t.has_artist_page() && !songs.iter().any(|s| s.artist.id == t.artist.id) && !artists.iter().any(|(id, _)| *id == t.artist.id) {
                        songs.push(t.clone());
                    }
                }
                Box::new(move || deezer::home(&artists, &songs).map(Page::Home))
            }
            Route::Charts => {
                let genre = self.chart_genre.0;
                Box::new(move || deezer::chart_tracks(genre).map(Page::Tracks))
            }
            Route::Search(q) => Box::new(move || deezer::search(&q).map(Page::Search)),
            Route::Playlist(id) => Box::new(move || deezer::playlist(&id).map(|(info, tracks)| Page::Playlist { info, tracks })),
            Route::Album(id) => Box::new(move || deezer::album(&id).map(|(info, tracks)| Page::Album { info, tracks })),
            Route::Artist(id) => Box::new(move || deezer::artist_page(&id).map(Page::Artist)),
            Route::Mix(id) => Box::new(move || deezer::mix(&id).map(Page::Tracks)),
            Route::Public(id) => Box::new(move || crate::account::playlist(&id).map(|p| Page::Public(Box::new(p)))),
            Route::User(name) => Box::new(move || crate::account::profile(&name).map(Page::User)),
            Route::Song(id) => {
                let Some(n) = id.strip_prefix("dz:").and_then(|n| n.parse::<u64>().ok()) else {
                    return;
                };
                Box::new(move || deezer::song_page(n).map(|p| Page::Song(Box::new(p))))
            }
            _ => return,
        };
        self.cache.insert(key.clone(), Remote::Loading);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let counter = self.preloading.clone();
        if preload {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        std::thread::spawn(move || {
            let result = job();
            if preload {
                counter.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            }
            let _ = tx.send((key, result));
            ctx.request_repaint();
        });
    }

    pub fn retry(&mut self, ctx: &egui::Context) {
        if let Some(k) = self.route_key() {
            self.cache.remove(&k);
        }
        self.ensure_loaded(ctx);
    }

    // ---------------------------------------------------------------- per-frame

    /// Called every frame before drawing: receives finished downloads and
    /// moves on to the next song when one ends.
    /// The song's lyrics, starting the lookup the first time they're wanted.
    pub fn lyrics_for(&mut self, t: &Track, ctx: &egui::Context) -> &LyricsState {
        if !self.lyrics.contains_key(&t.id) {
            // A session's worth at most; the disk copies stay.
            if self.lyrics.len() > 100 {
                self.lyrics.retain(|_, v| matches!(v, LyricsState::Loading));
            }
            self.lyrics.insert(t.id.clone(), LyricsState::Loading);
            let (tx, track, ctx) = (self.lyrics_tx.clone(), t.clone(), ctx.clone());
            std::thread::spawn(move || {
                let r = crate::lyrics::get(&track);
                let _ = tx.send((track.id, r));
                ctx.request_repaint();
            });
        }
        &self.lyrics[&t.id]
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        while let Ok((id, r)) = self.lyrics_rx.try_recv() {
            let state = match r {
                Ok(Some(l)) if !l.is_empty() => LyricsState::Ready(Arc::new(l)),
                Ok(_) => LyricsState::Missing,
                Err(e) => LyricsState::Failed(e),
            };
            self.lyrics.insert(id, state);
        }
        // Now and then, let go of pictures that are no longer on screen.
        let now = ctx.input(|i| i.time);
        if now - self.swept_at > 15.0 {
            self.swept_at = now;
            crate::artcache::sweep(ctx);
            self.trim_page_cache();
        }
        self.receive_credits();
        self.receive_artist_lookup(ctx);
        self.receive_popular_searches();
        self.receive_account(ctx);
        self.sync_public_playlists(ctx);
        self.receive_import(ctx);
        self.receive_rematches();
        self.run_featured(ctx);
        self.run_views(ctx);
        while let Ok(id) = self.prefetch_failed_rx.try_recv() {
            self.prefetch_failed.insert(id);
        }
        self.reload_themes_if_changed(ctx);
        if let Some((_, at)) = &self.highlight {
            let age = ctx.input(|i| i.time) - at;
            if age > 4.0 {
                self.highlight = None;
            } else {
                // Wake once when the highlight should go, not five times a second.
                ctx.request_repaint_after(Duration::from_secs_f64(4.05 - age));
            }
        }
        self.run_downloads(ctx);
        if let Some(rx) = &self.local_rx
            && let Ok(list) = rx.try_recv()
        {
            self.local_files = list;
            self.local_scanning = false;
            self.local_rx = None;
        }
        while let Ok((key, result)) = self.rx.try_recv() {
            let value = match result {
                Ok(page) => Remote::Ready(Box::new(page)),
                Err(e) => {
                    crate::crash::note(&format!("Couldn't load {key}: {e}"));
                    Remote::Failed(e)
                }
            };
            self.cache.insert(key, value);
        }

        // Debounced live search: search 350 ms after the user stops typing.
        if let Some(at) = self.search_edited_at {
            let now = ctx.input(|i| i.time);
            if now - at > 0.35 {
                self.search_edited_at = None;
                self.submit_search(ctx);
            } else {
                ctx.request_repaint_after(Duration::from_millis(60));
            }
        }

        if let Some(id) = self.audio.take_advanced() {
            self.advanced_to(&id, ctx);
        }
        if !self.needs_load && self.current.is_some() {
            self.tell_engine_next();
        }
        match self.audio.load_state() {
            LoadState::Failed(e) if self.current.is_some() && !self.needs_load => {
                crate::crash::note(&format!("Couldn't play: {e}"));
                self.failures_in_row += 1;
                let name = self.current.as_ref().map(|t| t.title.clone()).unwrap_or_default();
                self.toast(&format!("Couldn't play “{name}”: {e}"), ctx);
                self.audio.stop();
                if self.failures_in_row < 3 && self.queue_pos + 1 < self.queue.len() {
                    self.next(ctx);
                }
            }
            LoadState::Ready => {
                self.failures_in_row = 0;
                // After a restart, carry on from where the song was.
                if let Some((id, secs)) = self.resume_at.take()
                    && self.current.as_ref().is_some_and(|c| c.id == id)
                {
                    self.audio.seek(Duration::from_secs_f32(secs));
                }
                if self.audio.finished() {
                    self.on_track_end(ctx);
                } else {
                    self.prefetch_next();
                    self.maybe_fetch_autoplay(ctx);
                }
            }
            _ => {}
        }
        if let Some(n) = self.audio.take_notice() {
            self.toast(&n, ctx);
        }
        self.receive_autoplay(ctx);

        if self.is_playing() || self.audio.load_state().is_busy() {
            // Behind a game or minimized, nobody sees the progress bar: once a second is
            // plenty to move on to the next song.
            let hidden = ctx.input(|i| !i.focused || i.viewport().minimized.unwrap_or(false));
            let every = if hidden { Duration::from_secs(1) } else { crate::perf::playing_redraw() };
            ctx.request_repaint_after(every);
        }

        // Window title shows the current song.
        let title = match &self.current {
            Some(t) if self.is_playing() => format!("{} · {} — Pulse", t.title, t.all_artists()),
            _ => "Pulse".to_string(),
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        if let Some((msg, at)) = &self.toast
            && ctx.input(|i| i.time) - at > toast_secs(msg)
        {
            self.toast = None;
        }
    }

    pub fn submit_search(&mut self, ctx: &egui::Context) {
        let q = self.search_text.trim().to_string();
        if q.is_empty() {
            return;
        }
        // A pasted Pulse link opens what it points at.
        if self.open_link(&q, ctx) {
            self.search_text.clear();
            return;
        }
        self.navigate(Route::Search(q), ctx);
    }

    pub fn remember_search(&mut self, q: &str) {
        let q = q.trim();
        if q.is_empty() {
            return;
        }
        self.saved.recent_searches.retain(|r| !r.eq_ignore_ascii_case(q));
        self.saved.recent_searches.insert(0, q.to_string());
        self.saved.recent_searches.truncate(10);
    }

    /// The search page's results for what's typed, if they've arrived.
    pub fn search_results_for(&self, q: &str) -> Option<&deezer::SearchResults> {
        match self.cache.get(&format!("search:{}", q.trim())) {
            Some(Remote::Ready(p)) => match p.as_ref() {
                Page::Search(r) => Some(r),
                _ => None,
            },
            _ => None,
        }
    }

    /// The update check: every time Pulse opens (a few seconds in, so it doesn't slow the
    /// start), and once a day while it stays open (not in Gaming or Ultra-low mode). Also
    /// closes Pulse when an installer has been started.
    fn updates(&mut self, ctx: &egui::Context) {
        if self.updater.poll() {
            self.save_now = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let s = &mut self.saved.settings;
        let idle = matches!(self.updater.status, crate::update::Status::Idle);
        if s.check_updates && idle && !self.update_checked_launch && ctx.input(|i| i.time) > 4.0 {
            self.update_checked_launch = true;
            s.update_checked_at = crate::crash::now();
            self.updater.check(ctx);
            return;
        }
        if s.check_updates && idle && crate::perf::on(crate::perf::Extra::Lookups) && ctx.input(|i| i.time) > 15.0 {
            let now = crate::crash::now();
            if now.saturating_sub(s.update_checked_at) >= 86_400 {
                s.update_checked_at = now;
                self.updater.check(ctx);
            }
        }
    }

    /// Keeps the Discord status in step with the player. Paused for two minutes, it
    /// goes away: you're not listening any more.
    fn discord_status(&mut self, ctx: &egui::Context) {
        if !self.saved.settings.discord_status {
            self.discord.update(false, None);
            self.paused_since = None;
            return;
        }
        let playing = self.is_playing();
        let now = ctx.input(|i| i.time);
        if playing {
            self.paused_since = None;
        } else if self.paused_since.is_none() {
            self.paused_since = Some(now);
        }
        let gone = self.paused_since.is_some_and(|t| now - t > 120.0);
        // A song only restored from last time (not played yet) isn't shown.
        let want = self.current.as_ref().filter(|_| !self.needs_load && !gone).map(|t| crate::discord::song_status(t, playing, self.audio.position()));
        if want.is_some() && !playing {
            ctx.request_repaint_after(Duration::from_secs(121));
        }
        self.discord.update(true, want);
    }

    /// Starts the update: from the installer when this copy was installed, otherwise
    /// by opening the release page (a copy run from elsewhere isn't replaced).
    pub fn start_update(&mut self, ctx: &egui::Context) {
        let release = match &self.updater.status {
            crate::update::Status::Available(r) | crate::update::Status::InstallFailed(r, _) => r.clone(),
            _ => return,
        };
        if crate::update::installed() && release.signed() {
            self.updater.install(ctx);
        } else {
            ctx.open_url(egui::OpenUrl::new_tab(&release.page));
            let msg = if release.signed() {
                "Download the installer from the release page"
            } else {
                "This update isn't signed, so download it from the release page"
            };
            self.toast(crate::i18n::tr(msg), ctx);
        }
    }

    /// Reads the backups again (the old list shows until the new one is in).
    pub fn refresh_backups(&mut self) {
        self.backups_stale = true;
    }

    /// The backups in Settings, read in the background the first time they're asked for
    /// and after `refresh_backups`.
    pub fn backup_list(&mut self, ctx: &egui::Context) -> Option<&[crate::backup::Info]> {
        if let Some(rx) = &self.backups_rx {
            match rx.try_recv() {
                Ok(list) => {
                    self.backups = Some(list);
                    self.backups_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.backups_rx = None,
            }
        } else if self.backups.is_none() || self.backups_stale {
            self.backups_stale = false;
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(crate::backup::list());
                ctx.request_repaint();
            });
            self.backups_rx = Some(rx);
        }
        self.backups.as_deref()
    }

    /// Puts a backup in place of your library, after backing up the library as it is.
    pub fn restore_library(&mut self, lib: Saved, ctx: &egui::Context) {
        if let Err(e) = crate::backup::write(&self.saved, crate::backup::Kind::BeforeRestore) {
            self.toast(&format!("{} ({e})", crate::i18n::tr("Couldn't back up your library first, so nothing was changed")), ctx);
            return;
        }
        crate::backup::restore_into(&mut self.saved, lib);
        self.liked_ids = self.saved.liked.iter().map(|t| t.id.clone()).collect();
        // Playlist numbers may mean other playlists now, so drop anything that holds one.
        let local = |r: &Route| matches!(r, Route::Local(_));
        if local(&self.route) {
            self.route = Route::Home;
        }
        self.back.retain(|r| !local(r));
        self.forward.retain(|r| !local(r));
        self.selection = Default::default();
        self.rename_playlist = None;
        self.confirm_delete = None;
        self.refresh_backups();
        self.save_now = true;
        self.toast(crate::i18n::tr("Backup restored"), ctx);
    }

    pub fn toast(&mut self, msg: &str, ctx: &egui::Context) {
        self.toast = Some((msg.to_string(), ctx.input(|i| i.time)));
        ctx.request_repaint_after(Duration::from_secs_f64(toast_secs(msg) + 0.1));
    }

    // ---------------------------------------------------------------- playback

    pub fn is_playing(&self) -> bool {
        self.current.is_some() && !self.needs_load && !self.audio.is_paused()
    }

    /// Plays `tracks[index]` and queues up the rest of the list after it.
    pub fn play_list(&mut self, tracks: &[Track], index: usize, ctx: &egui::Context) {
        if tracks.is_empty() {
            return;
        }
        let index = index.min(tracks.len() - 1);
        // Clicking the song that's already playing just pauses/resumes it.
        if self.current.as_ref().is_some_and(|c| c.id == tracks[index].id) && !self.needs_load {
            self.toggle_play(ctx);
            return;
        }
        self.queue = tracks.to_vec();
        self.queue_pos = index;
        self.unshuffled = None;
        if self.saved.shuffle {
            self.apply_shuffle();
        }
        self.start_current(ctx);
    }

    fn start_current(&mut self, ctx: &egui::Context) {
        let Some(track) = self.queue.get(self.queue_pos).cloned() else {
            return;
        };
        self.audio.load(&track.id, Duration::from_secs(track.duration as u64), self.resolver(&track), false, ctx.clone());
        self.audio.set_volume(self.saved.volume);
        self.told_next = None;
        self.needs_load = false;
        self.seek_preview = None;
        // Recently played: newest first, no duplicates, max 50.
        self.saved.recent.retain(|t| t.id != track.id);
        self.saved.recent.insert(0, track.clone());
        self.saved.recent.truncate(50);
        self.record_play(&track);
        self.current = Some(track);
    }

    /// The song that plays after the current one: its queue position and the song.
    fn upcoming(&self) -> Option<(usize, Track)> {
        match self.saved.repeat {
            Repeat::One => self.queue.get(self.queue_pos).map(|t| (self.queue_pos, t.clone())),
            _ if self.queue_pos + 1 < self.queue.len() => Some((self.queue_pos + 1, self.queue[self.queue_pos + 1].clone())),
            Repeat::All if !self.queue.is_empty() => Some((0, self.queue[0].clone())),
            _ => None,
        }
    }

    /// Keeps the audio engine told which song is next (for gapless, crossfade, automix).
    fn tell_engine_next(&mut self) {
        let next = self.upcoming().map(|(_, t)| t);
        let id = next.as_ref().map(|t| t.id.clone());
        if self.told_next.as_ref() == Some(&id) {
            return;
        }
        self.told_next = Some(id);
        let req = next.map(|t| (t.id.clone(), Duration::from_secs(t.duration as u64), self.resolver(&t)));
        self.audio.set_next(req);
    }

    /// The engine moved on to the next song by itself: catch the queue up.
    fn advanced_to(&mut self, id: &str, ctx: &egui::Context) {
        let pos = match self.upcoming() {
            Some((i, t)) if t.id == id => i,
            _ => match self.queue.iter().skip(self.queue_pos).position(|t| t.id == id) {
                Some(i) => self.queue_pos + i,
                None => return,
            },
        };
        self.queue_pos = pos;
        let track = self.queue[pos].clone();
        self.told_next = None;
        self.needs_load = false;
        self.seek_preview = None;
        self.saved.recent.retain(|t| t.id != track.id);
        self.saved.recent.insert(0, track.clone());
        self.saved.recent.truncate(50);
        self.record_play(&track);
        self.current = Some(track);
        self.maybe_fetch_autoplay(ctx);
    }

    pub fn toggle_play(&mut self, ctx: &egui::Context) {
        if self.current.is_none() {
            return;
        }
        if self.needs_load || matches!(self.audio.load_state(), LoadState::Idle | LoadState::Failed(_)) {
            if self.queue.is_empty() {
                self.queue = self.current.clone().into_iter().collect();
                self.queue_pos = 0;
            }
            self.start_current(ctx);
        } else if self.audio.is_paused() {
            self.audio.play();
        } else {
            self.audio.pause();
        }
    }

    pub fn next(&mut self, ctx: &egui::Context) {
        if self.queue.is_empty() {
            return;
        }
        if self.queue_pos + 1 < self.queue.len() {
            self.queue_pos += 1;
        } else if self.saved.repeat == Repeat::All {
            self.queue_pos = 0;
        } else {
            return;
        }
        self.start_current(ctx);
    }

    pub fn previous(&mut self, ctx: &egui::Context) {
        // Like Spotify: "previous" restarts the song unless you're in the first few seconds.
        if self.audio.position().as_secs_f32() > 3.0 || self.queue_pos == 0 {
            if self.needs_load {
                self.start_current(ctx);
            } else {
                self.audio.seek(Duration::ZERO);
            }
            return;
        }
        self.queue_pos -= 1;
        self.start_current(ctx);
    }

    fn on_track_end(&mut self, ctx: &egui::Context) {
        self.maybe_fetch_autoplay(ctx);
        match self.saved.repeat {
            Repeat::One => self.start_current(ctx),
            _ if self.queue_pos + 1 < self.queue.len() || self.saved.repeat == Repeat::All => self.next(ctx),
            _ if self.saved.autoplay && self.autoplay_rx.is_some() => {
                // Similar songs are still loading: continue as soon as they arrive.
                self.advance_when_autoplay_arrives = true;
            }
            _ => {
                // End of the queue: stop, ready to replay from the start of this song.
                self.audio.stop();
                self.needs_load = true;
            }
        }
    }

    /// The current song's length in seconds. Deezer lists some songs as a 30-second clip;
    /// then it's the length of the audio actually playing.
    pub fn current_length(&self) -> u32 {
        let Some(t) = &self.current else { return 0 };
        let listed = crate::fixes::shown_length(t);
        match self.audio.length() {
            Some(real) if !self.needs_load && crate::fixes::is_clip(t) && (60..86_400).contains(&real.as_secs()) => {
                let secs = real.as_secs() as u32;
                if listed != secs {
                    // Shown in the lists from now on too.
                    crate::fixes::played(&t.id, secs);
                }
                secs
            }
            _ => listed,
        }
    }

    pub fn seek_fraction(&mut self, frac: f32) {
        if self.current.is_some() {
            if self.needs_load {
                return;
            }
            self.audio.seek(Duration::from_secs_f32(self.current_length() as f32 * frac));
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        self.saved.volume = v.clamp(0.0, 1.0);
        self.muted_volume = None;
        self.audio.set_volume(self.saved.volume);
    }

    pub fn toggle_mute(&mut self) {
        if let Some(v) = self.muted_volume.take() {
            self.saved.volume = v;
        } else {
            self.muted_volume = Some(self.saved.volume.max(0.05));
            self.saved.volume = 0.0;
        }
        self.audio.set_volume(self.saved.volume);
    }

    pub fn toggle_shuffle(&mut self) {
        self.saved.shuffle = !self.saved.shuffle;
        if self.saved.shuffle {
            self.apply_shuffle();
        } else if let Some(orig) = self.unshuffled.take() {
            let cur = self.queue.get(self.queue_pos).map(|t| t.id.clone());
            self.queue = orig;
            self.queue_pos = cur.and_then(|id| self.queue.iter().position(|t| t.id == id)).unwrap_or(0);
        }
    }

    /// Keeps the current song first and shuffles everything else after it.
    fn apply_shuffle(&mut self) {
        use rand::seq::SliceRandom;
        if self.queue.is_empty() {
            return;
        }
        self.unshuffled = Some(self.queue.clone());
        let current = self.queue.remove(self.queue_pos);
        self.queue.shuffle(&mut rand::rng());
        self.queue.insert(0, current);
        self.queue_pos = 0;
    }

    pub fn cycle_repeat(&mut self) {
        self.saved.repeat = match self.saved.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
    }

    pub fn toggle_theme(&mut self, ctx: &egui::Context) {
        // Switch to the theme's other scheme: dark ↔ light.
        let Some(t) = self.current_theme() else {
            return;
        };
        let cur = t.scheme_index(&self.saved.settings.scheme);
        let want_light = !crate::themes::is_light(&t.schemes[cur].palette);
        if let Some(s) = t.schemes.iter().find(|s| crate::themes::is_light(&s.palette) == want_light) {
            self.saved.settings.scheme = s.name.clone();
            self.saved.light_mode = want_light;
        }
        self.apply_theme(ctx);
    }

    pub fn current_theme(&self) -> Option<&crate::themes::Theme> {
        let name = &self.saved.settings.theme;
        self.themes.iter().find(|t| t.name.eq_ignore_ascii_case(name)).or_else(|| self.themes.first())
    }

    /// True if the current theme has both a dark and a light scheme.
    pub fn theme_has_light_and_dark(&self) -> bool {
        self.current_theme().is_some_and(|t| {
            let light = t.schemes.iter().filter(|s| crate::themes::is_light(&s.palette)).count();
            light > 0 && light < t.schemes.len()
        })
    }

    /// Applies the chosen theme, scheme and high-contrast setting.
    pub fn apply_theme(&mut self, ctx: &egui::Context) {
        let Some(t) = self.current_theme() else {
            return;
        };
        let active = t.active(t.scheme_index(&self.saved.settings.scheme), self.saved.settings.high_contrast);
        self.saved.light_mode = crate::themes::is_light(&active.palette);
        crate::theme::set_active(ctx, active);
    }

    /// Reloads themes after a theme file changed (a moment after the last change).
    fn reload_themes_if_changed(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some((_, rx)) = &self.theme_watch {
            let mut changed = false;
            while rx.try_recv().is_ok() {
                changed = true;
            }
            if changed {
                self.theme_changed_at = Some(now);
            }
        }
        if let Some(at) = self.theme_changed_at {
            if now - at > 0.25 {
                self.theme_changed_at = None;
                self.themes = crate::themes::list();
                self.apply_theme(ctx);
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }

    // ---------------------------------------------------------------- settings

    /// Puts the performance mode into effect (no restart needed).
    pub fn apply_perf(&mut self, ctx: &egui::Context) {
        use crate::perf::{Extra, on};
        let s = &self.saved.settings;
        crate::perf::apply(s.perf_mode, &s.perf_keep);
        crate::theme::set_reduce_motion(s.reduce_motion || !on(Extra::Animations));
        // egui's own small animations (hover fades, opening sections).
        let anim = if on(Extra::Animations) && !s.reduce_motion { 1.0 / 12.0 } else { 0.0 };
        ctx.all_styles_mut(|st| st.animation_time = anim);
        if !on(Extra::Lookups) {
            self.featured_queue.clear();
            self.views_queue.clear();
        }
    }

    /// Gaming Mode on or off (Ultra-low resource counts as on).
    pub fn toggle_gaming(&mut self, ctx: &egui::Context) {
        let before = self.saved.settings.clone();
        self.saved.settings.perf_mode = match before.perf_mode {
            crate::perf::Mode::Normal => crate::perf::Mode::Gaming,
            _ => crate::perf::Mode::Normal,
        };
        self.apply_settings(&before, ctx);
    }

    /// Puts changed settings into effect. `before` is how they were.
    pub fn apply_settings(&mut self, before: &crate::settings::Settings, ctx: &egui::Context) {
        let s = self.saved.settings.clone();
        crate::i18n::set_language(s.language);
        crate::deezer::set_prefer_explicit(s.explicit);
        self.apply_perf(ctx);
        if s.perf_mode != before.perf_mode {
            let msg = match s.perf_mode {
                crate::perf::Mode::Normal => "Gaming Mode off",
                crate::perf::Mode::Gaming => "Gaming Mode on: extras paused, music unchanged",
                crate::perf::Mode::UltraLow => "Ultra-low resource mode on",
            };
            self.toast(crate::i18n::tr(msg), ctx);
        }
        if s.replay_cache_mb != before.replay_cache_mb {
            crate::audiocache::set_limit_mb(s.replay_cache_mb);
            self.cache_size = None;
        }
        if s.art_cache_mb != before.art_cache_mb {
            crate::artcache::set_limit_mb(s.art_cache_mb);
            std::thread::spawn(crate::artcache::trim_disk);
            self.cache_size = None;
        }
        if s.high_contrast != before.high_contrast || s.theme != before.theme || s.scheme != before.scheme {
            self.apply_theme(ctx);
        }
        if s.hardware_acceleration != before.hardware_acceleration {
            crate::settings::set_hardware_acceleration(s.hardware_acceleration);
            self.toast(crate::i18n::tr("Restart Pulse to apply this change."), ctx);
        }
        if s.stream_quality != before.stream_quality {
            // Links found ahead of time were for the old quality.
            self.prefetched.lock().unwrap().clear();
        }
        self.audio.set_effects(crate::audio::Effects::from_settings(&s));
        if s.show_local_files && !before.show_local_files {
            self.scan_local_files(ctx);
        }
        if !s.show_local_files && self.route == Route::LocalFiles {
            self.navigate(Route::Home, ctx);
        }
    }

    /// Looks up genres for the albums in `tracks` that aren't known yet (in the background).
    pub fn ensure_genres(&mut self, tracks: &[Track], ctx: &egui::Context) {
        if let Some(rx) = &self.genre_rx {
            while let Ok((id, g)) = rx.try_recv() {
                self.genre_pending.remove(&id);
                match g {
                    Some(g) => {
                        self.saved.album_genres.insert(id, g);
                    }
                    // Couldn't ask (offline, Deezer busy): not remembered, so it's asked
                    // again next time Pulse opens; skipped for now so it isn't asked in a loop.
                    None => {
                        self.genre_failed.insert(id);
                    }
                }
            }
        }
        if !self.genre_pending.is_empty() {
            return; // one batch at a time
        }
        let missing: Vec<String> = tracks
            .iter()
            .filter_map(|t| t.album.as_ref())
            .map(|a| a.id.clone())
            .filter(|id| deezer::is_deezer_id(id) && !self.saved.album_genres.contains_key(id) && !self.genre_failed.contains(id))
            .collect::<HashSet<_>>()
            .into_iter()
            .take(40)
            .collect();
        if missing.is_empty() {
            return;
        }
        self.genre_pending.extend(missing.iter().cloned());
        let (tx, rx) = channel();
        self.genre_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for id in missing {
                // Albums without genres are remembered as empty, so they aren't asked for again.
                let g = deezer::album_genres(&id).ok();
                if tx.send((id, g)).is_err() {
                    return;
                }
                // One redraw for a batch of answers, not one each.
                ctx.request_repaint_after(Duration::from_millis(800));
                // Stay well under Deezer's limit of 50 requests per 5 seconds.
                std::thread::sleep(Duration::from_millis(150));
            }
        });
    }

    /// Copies a picture into Pulse's data folder and makes it the playlist's cover.
    #[cfg_attr(not(windows), allow(dead_code))] // the picture picker is Windows-only
    pub fn set_playlist_cover(&mut self, i: usize, picture: &std::path::Path) -> Result<(), String> {
        let dir = crate::settings::data_dir().join("covers");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let ext = picture.extension().and_then(|x| x.to_str()).unwrap_or("jpg").to_lowercase();
        // A new name each time, so the old picture isn't shown from memory.
        let dest = dir.join(format!("playlist-{}-{}.{ext}", i, now_unix()));
        std::fs::copy(picture, &dest).map_err(|e| format!("Couldn't copy the picture ({e})"))?;
        if let Some(old) = self.saved.playlists.get_mut(i).and_then(|p| p.cover.replace(dest)) {
            let _ = std::fs::remove_file(old);
        }
        Ok(())
    }

    pub fn remove_playlist_cover(&mut self, i: usize) {
        if let Some(old) = self.saved.playlists.get_mut(i).and_then(|p| p.cover.take()) {
            let _ = std::fs::remove_file(old);
        }
    }

    /// Saves the profile form. A new picture is copied into Pulse's data folder.
    pub fn save_profile(&mut self, f: &crate::ui::ProfileForm) -> Result<(), String> {
        let dir = crate::settings::data_dir().join("covers");
        let mut picture = f.picture.clone();
        if let Some(src) = &f.picture
            && !src.starts_with(&dir)
        {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let ext = src.extension().and_then(|x| x.to_str()).unwrap_or("png").to_lowercase();
            let dest = dir.join(format!("profile-{}.{ext}", now_unix()));
            std::fs::copy(src, &dest).map_err(|e| format!("Couldn't copy the picture ({e})"))?;
            if let Some(old) = self.saved.profile.picture.take() {
                let _ = std::fs::remove_file(old);
            }
            picture = Some(dest);
        }
        let picture_changed = self.saved.profile.picture != picture;
        let p = &mut self.saved.profile;
        p.name = f.name.trim().to_string();
        let bio_changed = p.bio != f.bio.trim();
        p.bio = f.bio.trim().to_string();
        p.picture = picture;
        // Signed in: the same picture on your public profile, for others to see.
        if picture_changed && crate::account::session().is_some() {
            let pic = p.picture.clone();
            std::thread::spawn(move || {
                let _ = match &pic {
                    Some(path) => crate::account::set_avatar(path),
                    None => crate::account::remove_avatar(),
                };
                crate::ui::forget_own_public_profile();
            });
        }
        // Signed in: the same bio on your public profile.
        if bio_changed && crate::account::session().is_some() {
            let bio = p.bio.clone();
            std::thread::spawn(move || {
                let _ = crate::account::set_bio(&bio);
                crate::ui::forget_own_public_profile();
            });
        }
        if p.created == 0 {
            p.created = now_unix();
        }
        Ok(())
    }

    /// Copies a share code for playlist `i` to the clipboard.
    pub fn share_playlist(&mut self, i: usize, ctx: &egui::Context) {
        let Some(p) = self.saved.playlists.get(i) else {
            return;
        };
        let code = crate::share::encode(&p.name, &self.saved.profile.name, &p.tracks);
        ctx.copy_text(code);
        self.toast("Share code copied. Friends paste it into Import playlist.", ctx);
    }

    pub fn remove_from_playlist(&mut self, i: usize, id: &str) {
        if let Some(p) = self.saved.playlists.get_mut(i) {
            p.tracks.retain(|t| t.id != id);
            p.added.remove(id);
        }
    }

    /// Remembers the artist and album of a song you played (for the home page).
    fn record_play(&mut self, t: &Track) {
        let now = now_unix();
        if t.has_artist_page() {
            let image = t.artist.picture.clone().or_else(|| t.art_big().map(str::to_owned));
            let v = Visit {
                place: Place::Artist(t.artist.id.clone()),
                title: t.artist.name.clone(),
                subtitle: "Artist".into(),
                image,
                round: true,
                count: 0,
                last: now,
            };
            bump(&mut self.saved.played_artists, v, 60);
        }
        if let Some(al) = t.album.as_ref().filter(|a| deezer::is_deezer_id(&a.id)) {
            let v = Visit {
                place: Place::Album(al.id.clone()),
                title: al.title.clone(),
                subtitle: t.artist.name.clone(),
                image: t.art_big().map(str::to_owned),
                round: false,
                count: 0,
                last: now,
            };
            bump(&mut self.saved.played_albums, v, 30);
        }
    }

    /// Remembers a page you opened (for "Recents"). Cheap to call every frame.
    /// A playlist's picture: your own cover, else its first song's artwork.
    pub fn playlist_art(&self, i: usize) -> Option<String> {
        let p = self.saved.playlists.get(i)?;
        p.cover.as_ref().map(|c| crate::settings::file_uri(c)).or_else(|| p.tracks.iter().find_map(|t| t.art_small()).map(str::to_owned))
    }

    pub fn note_visit(&mut self, place: Place, title: &str, subtitle: &str, image: Option<&str>, round: bool) {
        if self.saved.visits.first().is_some_and(|v| v.place == place) {
            return;
        }
        let v = Visit { place, title: title.to_string(), subtitle: subtitle.to_string(), image: image.map(str::to_owned), round, count: 0, last: now_unix() };
        bump(&mut self.saved.visits, v, 20);
    }

    /// The page for a remembered place.
    pub fn route_for(&self, place: &Place) -> Option<Route> {
        Some(match place {
            Place::Album(id) => Route::Album(id.clone()),
            Place::Artist(id) => Route::Artist(id.clone()),
            Place::Playlist(id) => Route::Playlist(id.clone()),
            Place::Yours(name) => Route::Local(self.saved.playlists.iter().position(|p| p.name == *name)?),
            Place::Liked => Route::Liked,
            Place::Mix(id) => Route::Mix(id.clone()),
        })
    }

    /// Changes every copy of song `id`: playlists, likes, history, queue and loaded pages.
    fn update_track(&mut self, id: &str, mut f: impl FnMut(&mut Track)) {
        let lists = self.saved.playlists.iter_mut().flat_map(|p| p.tracks.iter_mut());
        let others = self.saved.liked.iter_mut().chain(self.saved.recent.iter_mut()).chain(self.queue.iter_mut()).chain(self.current.iter_mut());
        for t in lists.chain(others).filter(|t| t.id == id) {
            f(t);
        }
        for remote in self.cache.values_mut() {
            if let Remote::Ready(page) = remote {
                page.for_each_track(|t| {
                    if t.id == id {
                        f(t);
                    }
                });
            }
        }
    }

    /// Looks up featured artists for songs in a list (once per song, ever), one at a time.
    pub fn ensure_featured(&mut self, tracks: &[Track]) {
        if !crate::perf::on(crate::perf::Extra::Lookups) {
            return;
        }
        for t in tracks.iter().take(80) {
            if let Some(n) = t.deezer_number()
                && t.featured.is_empty()
                && !self.saved.featured_checked.contains(&t.id)
                && !self.featured_failed.contains(&t.id)
                && !self.featured_queue.contains(&n)
            {
                self.featured_queue.push_back(n);
            }
        }
    }

    fn run_featured(&mut self, ctx: &egui::Context) {
        while let Ok((id, featured)) = self.featured_rx.try_recv() {
            self.featured_busy = false;
            // Couldn't ask (offline, Deezer busy): asked again another time.
            let Some(featured) = featured else {
                self.featured_failed.insert(id);
                continue;
            };
            self.saved.featured_checked.insert(id.clone());
            if !featured.is_empty() {
                self.update_track(&id, |t| t.featured = featured.clone());
            }
        }
        if self.featured_busy {
            return;
        }
        let Some(n) = self.featured_queue.pop_front() else {
            return;
        };
        self.featured_busy = true;
        let tx = self.featured_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // Paced well under Deezer's limit, since this runs alongside everything else.
            std::thread::sleep(Duration::from_millis(250));
            let featured = deezer::featured_artists(n).ok();
            let _ = tx.send((format!("dz:{n}"), featured));
            ctx.request_repaint_after(Duration::from_millis(800));
        });
    }

    /// Looks again for imported songs that weren't found on Deezer (e.g. because Deezer
    /// was busy during the import), and fixes their covers. One batch at a time.
    pub fn rematch_imports(&mut self, tracks: &[Track], ctx: &egui::Context) {
        if self.rematch_rx.is_some() {
            return;
        }
        let todo: Vec<Track> = tracks.iter().filter(|t| t.id.starts_with("imp:") && !self.rematch_tried.contains(&t.id)).take(25).cloned().collect();
        if todo.is_empty() {
            return;
        }
        self.rematch_tried.extend(todo.iter().map(|t| t.id.clone()));
        let (tx, rx) = channel();
        self.rematch_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for t in todo {
                let mut found = deezer::find_track(&t.artist.name, &t.title, t.duration).ok().flatten();
                if found.is_none() {
                    found = deezer::find_track(&t.title, &t.artist.name, t.duration).ok().flatten();
                }
                let cover = if found.is_none() { crate::itunes::cover(&t.artist.name, &t.title) } else { None };
                if tx.send((t.id, found, cover)).is_err() {
                    return;
                }
                ctx.request_repaint();
                std::thread::sleep(Duration::from_millis(250));
            }
        });
    }

    fn receive_rematches(&mut self) {
        let Some(rx) = &self.rematch_rx else { return };
        let mut results = vec![];
        let mut finished = false;
        loop {
            match rx.try_recv() {
                Ok(r) => results.push(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            self.rematch_rx = None;
        }
        for (old, found, cover) in results {
            match found {
                Some(new) => {
                    // Swap in the Deezer song; keep its YouTube video and "date added".
                    if let Some(v) = self.youtube_ids.lock().unwrap().get(&old).cloned() {
                        self.youtube_ids.lock().unwrap().insert(new.id.clone(), v);
                    }
                    for p in &mut self.saved.playlists {
                        if let Some(when) = p.added.remove(&old) {
                            p.added.insert(new.id.clone(), when);
                        }
                    }
                    let new2 = new.clone();
                    self.update_track(&old, |t| *t = new2.clone());
                    if self.liked_ids.remove(&old) {
                        self.liked_ids.insert(new.id.clone());
                    }
                }
                None => {
                    // Songs imported from Spotify before this fix carry the playlist's cover.
                    let spotify = old.starts_with("imp:spotify:");
                    self.update_track(&old, |t| {
                        if cover.is_some() || spotify {
                            t.cover_small = cover.clone();
                            t.cover_big = cover.clone();
                        }
                    });
                }
            }
        }
    }

    /// Asks for YouTube view counts of songs that don't have a recent one.
    pub fn ensure_views(&mut self, tracks: &[Track]) {
        if !crate::perf::on(crate::perf::Extra::Lookups) {
            return;
        }
        const WEEK: u64 = 7 * 86_400;
        let now = now_unix();
        for t in tracks.iter().take(60) {
            if crate::local::is_local(&t.id) || self.views_pending.contains(&t.id) || self.views_failed.contains(&t.id) {
                continue;
            }
            if self.saved.views.get(&t.id).is_some_and(|(_, at)| now.saturating_sub(*at) < WEEK) {
                continue;
            }
            self.views_pending.insert(t.id.clone());
            self.views_queue.push_back(t.clone());
        }
    }

    /// Looks up view counts, three songs at a time (called every frame).
    fn run_views(&mut self, ctx: &egui::Context) {
        while let Ok((id, found)) = self.views_rx.try_recv() {
            self.views_pending.remove(&id);
            if found.is_none() && crate::youtube::tools_ready() {
                self.views_failed.insert(id.clone());
            }
            if let Some((video, n)) = found {
                self.saved.views.insert(id.clone(), (n, now_unix()));
                // Songs without any cover get their video's picture.
                let thumb = crate::import::youtube_thumb(&video);
                self.update_track(&id, |t| {
                    if t.cover_small.is_none() {
                        t.cover_small = Some(thumb.clone());
                        t.cover_big = Some(thumb.clone());
                    }
                });
                // Remember the video too, so playing the song skips the search.
                self.youtube_ids.lock().unwrap().entry(id).or_insert(video);
            }
        }
        let running = self.views_pending.len() - self.views_queue.len();
        if running >= 3 || !crate::youtube::tools_ready() {
            return;
        }
        for _ in running..3 {
            let Some(t) = self.views_queue.pop_front() else {
                break;
            };
            let tx = self.views_tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let r = crate::youtube::views(&t).ok().flatten();
                let _ = tx.send((t.id, r));
                ctx.request_repaint();
            });
        }
    }

    /// Starts importing the playlist at `import_url`.
    pub fn start_import(&mut self, ctx: &egui::Context) {
        let url = self.import_url.trim().to_string();
        // A Pulse playlist link opens its page (save it or copy it from there).
        if self.open_link(&url, ctx) {
            self.import_open = false;
            self.import_url.clear();
            return;
        }
        if let Err(e) = crate::import::detect(&url) {
            self.toast(&e, ctx);
            return;
        }
        let progress = Arc::new(Mutex::new(crate::import::Progress::default()));
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tx, rx) = channel();
        let (p, c, ctx2) = (progress.clone(), cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            // Keep the window updating while the import runs.
            let ticker = {
                let ctx = ctx2.clone();
                let c = c.clone();
                std::thread::spawn(move || {
                    while !c.load(std::sync::atomic::Ordering::Relaxed) {
                        ctx.request_repaint();
                        std::thread::sleep(Duration::from_millis(300));
                    }
                })
            };
            // A failure inside the import ends it with a message instead of leaving it
            // stuck partway forever.
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::import::run(&url, &p, &c)))
                .unwrap_or_else(|_| Err("The import hit a problem partway through. Please try again; if it keeps happening, send a bug report.".into()));
            let _ = tx.send(r);
            c.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = ticker.join();
            ctx2.request_repaint();
        });
        self.import_running = Some(ImportJob { progress, cancel, rx });
    }

    pub fn cancel_import(&mut self) {
        if let Some(job) = self.import_running.take() {
            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Turns a finished import into a new playlist (called every frame).
    fn receive_import(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.import_running else {
            return;
        };
        let result = match job.rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("The import stopped unexpectedly. Please try again.".into()),
        };
        self.import_running = None;
        match result {
            Ok(imp) => {
                let mut name = imp.name.trim().to_string();
                if name.is_empty() {
                    name = "Imported playlist".into();
                }
                if self.saved.playlists.iter().any(|p| p.name == name) {
                    name = format!("{name} (imported)");
                }
                let now = now_unix();
                let mut ids = self.youtube_ids.lock().unwrap();
                for (t, video) in &imp.tracks {
                    if let Some(v) = video {
                        ids.insert(t.id.clone(), v.clone());
                    }
                }
                drop(ids);
                let total = imp.tracks.len();
                let tracks: Vec<Track> = imp.tracks.into_iter().map(|(t, _)| t).collect();
                let added = tracks.iter().map(|t| (t.id.clone(), now)).collect();
                self.saved.playlists.push(LocalPlaylist { name: name.clone(), tracks, cover: None, added, ..Default::default() });
                let i = self.saved.playlists.len() - 1;
                self.import_open = false;
                self.import_url.clear();
                self.navigate(Route::Local(i), ctx);
                let extra = if imp.matched < total { format!(" ({} not found on Deezer, they'll still play)", total - imp.matched) } else { String::new() };
                let from = imp.owner.map(|o| format!(" from {o}")).unwrap_or_default();
                let note = imp.note.map(|n| format!("\n{n}")).unwrap_or_default();
                self.toast(&format!("Imported “{name}”{from}: {total} songs{extra}{note}"), ctx);
            }
            Err(e) => self.toast(&e, ctx),
        }
    }

    /// Reads the Music folder in the background.
    pub fn scan_local_files(&mut self, ctx: &egui::Context) {
        let Some(dir) = crate::settings::music_dir() else {
            return;
        };
        let (tx, rx) = channel();
        self.local_rx = Some(rx);
        self.local_scanning = true;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::local::scan(&dir));
            ctx.request_repaint();
        });
    }

    /// Adds songs to the download list (skipping ones already downloaded or queued).
    pub fn download(&mut self, tracks: &[Track], ctx: &egui::Context) {
        let mut added = 0;
        for t in tracks {
            let busy = self.downloading.iter().any(|(d, _)| d.id == t.id);
            if crate::local::is_local(&t.id) || busy || self.saved.downloads.contains_key(&t.id) || self.download_queue.iter().any(|q| q.id == t.id) {
                continue;
            }
            self.download_queue.push_back(t.clone());
            added += 1;
        }
        if added > 0 {
            let msg = if added == 1 { "Downloading 1 song".to_string() } else { format!("Downloading {added} songs") };
            self.toast(&msg, ctx);
        }
    }

    pub fn remove_download(&mut self, t: &Track, ctx: &egui::Context) {
        self.download_queue.retain(|q| q.id != t.id);
        if let Some(path) = self.saved.downloads.remove(&t.id) {
            let _ = std::fs::remove_file(path);
            self.toast(crate::i18n::tr("Remove download"), ctx);
        }
    }

    pub fn is_downloaded(&self, id: &str) -> bool {
        self.saved.downloads.contains_key(id)
    }

    /// Runs a few downloads at a time and records finished ones (called every frame).
    fn run_downloads(&mut self, ctx: &egui::Context) {
        while let Ok((t, result)) = self.download_rx.try_recv() {
            self.downloading.retain(|(d, _)| d.id != t.id);
            match result {
                Ok((path, video)) => {
                    if !video.is_empty() {
                        self.youtube_ids.lock().unwrap().insert(t.id.clone(), video);
                    }
                    self.saved.downloads.insert(t.id, path);
                }
                Err(e) => self.toast(&format!("Couldn't download “{}”: {e}", t.title), ctx),
            }
        }
        // Three songs side by side; one at a time in Gaming and Ultra-low resource modes.
        let at_once = if crate::perf::mode() == crate::perf::Mode::Normal { 3 } else { 1 };
        while self.downloading.len() < at_once {
            let Some(t) = self.download_queue.pop_front() else {
                break;
            };
            let progress = Arc::new(Mutex::new(0.0));
            self.downloading.push((t.clone(), progress.clone()));
            let known = self.youtube_ids.lock().unwrap().get(&t.id).cloned();
            let quality = self.saved.settings.download_quality;
            let dir = self.saved.settings.offline_dir();
            let tx = self.download_tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let r = crate::downloads::download(&t, known, quality, &dir, &|p| {
                    let mut at = progress.lock().unwrap();
                    *at = p.max(*at);
                    ctx.request_repaint_after(Duration::from_millis(200));
                });
                let _ = tx.send((t, r));
                ctx.request_repaint();
            });
        }
    }

    /// Deletes temporary files: yt-dlp's cache, saved artwork and found audio links.
    pub fn clear_cache(&mut self, ctx: &egui::Context) {
        for dir in cache_dirs() {
            let _ = std::fs::remove_dir_all(dir);
        }
        ctx.forget_all_images();
        self.prefetched.lock().unwrap().clear();
        self.cache_size = None;
        self.toast(crate::i18n::tr("Cache cleared"), ctx);
    }

    // ---------------------------------------------------------------- library

    pub fn toggle_like(&mut self, t: &Track, ctx: &egui::Context) {
        if self.liked_ids.remove(&t.id) {
            self.saved.liked.retain(|x| x.id != t.id);
            self.saved.liked_added.remove(&t.id);
            self.toast("Removed from Liked Songs", ctx);
        } else {
            self.liked_ids.insert(t.id.clone());
            self.saved.liked.insert(0, t.clone());
            self.saved.liked_added.insert(t.id.clone(), now_unix());
            self.toast("Saved to Liked Songs", ctx);
        }
    }

    pub fn create_playlist(&mut self, name: String, first: Vec<Track>, ctx: &egui::Context) {
        let name = if name.trim().is_empty() { format!("My Playlist #{}", self.saved.playlists.len() + 1) } else { name.trim().to_string() };
        let added = first.iter().map(|t| (t.id.clone(), now_unix())).collect();
        let mut seen = HashSet::new();
        let tracks = first.into_iter().filter(|t| seen.insert(t.id.clone())).collect();
        self.saved.playlists.push(LocalPlaylist { name: name.clone(), tracks, cover: None, added, ..Default::default() });
        self.toast(&format!("Created “{name}”"), ctx);
        self.navigate(Route::Local(self.saved.playlists.len() - 1), ctx);
    }

    pub fn delete_playlist(&mut self, idx: usize, ctx: &egui::Context) {
        if idx >= self.saved.playlists.len() {
            return;
        }
        let p = self.saved.playlists.remove(idx);
        // Fix up history entries that point at playlists after the deleted one.
        let fix = |r: &mut Route| {
            if let Route::Local(i) = r {
                if *i == idx {
                    *r = Route::Home;
                } else if *i > idx {
                    *i -= 1;
                }
            }
        };
        fix(&mut self.route);
        self.back.iter_mut().for_each(fix);
        self.forward.iter_mut().for_each(fix);
        self.toast(&format!("Deleted “{}”", p.name), ctx);
    }

    pub fn playlist_names(&self) -> Vec<String> {
        self.saved.playlists.iter().map(|p| p.name.clone()).collect()
    }

    // ------------------------------------------------------------ accounts

    /// Runs an account call in the background; its result is applied in `receive_account`.
    pub fn account_job(&mut self, ctx: &egui::Context, job: impl FnOnce() -> Result<AccountDone, String> + Send + 'static) {
        self.account_busy += 1;
        if let Some(d) = &mut self.account_dialog {
            d.error = None;
        }
        let (tx, ctx) = (self.account_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
    }

    fn receive_account(&mut self, ctx: &egui::Context) {
        while let Ok(r) = self.account_rx.try_recv() {
            self.account_busy = self.account_busy.saturating_sub(1);
            match r {
                Err(e) => match &mut self.account_dialog {
                    Some(d) => d.error = Some(e),
                    None => self.toast(&e, ctx),
                },
                Ok(done) => self.account_done(done, ctx),
            }
        }
    }

    pub(crate) fn account_done(&mut self, done: AccountDone, ctx: &egui::Context) {
        use crate::i18n::tr;
        match done {
            AccountDone::Checked(s) => {
                if s.email.is_empty() && self.account_dialog.is_none() {
                    self.account_dialog = Some(AccountDialog::email_required());
                }
            }
            AccountDone::CodeSent(message) => {
                if let Some(d) = &mut self.account_dialog {
                    d.mode = AccountMode::Reset;
                    d.note = Some(message);
                    d.error = None;
                    d.code.clear();
                    d.password.clear();
                    d.password2.clear();
                }
            }
            AccountDone::SignedIn(s) => {
                // Older accounts have no email yet: one is needed before going on.
                self.account_dialog = s.email.is_empty().then(AccountDialog::email_required);
                // No profile on this computer yet: your username is its name.
                if self.saved.profile.created == 0 {
                    self.saved.profile.name = s.username.clone();
                    self.saved.profile.created = now_unix();
                    self.profile_editing = None;
                }
                self.toast(&format!("{} @{}", tr("Signed in as"), s.username), ctx);
            }
            AccountDone::SignedOut => self.toast(tr("Signed out"), ctx),
            AccountDone::Deleted => {
                for p in &mut self.saved.playlists {
                    p.public_id = None;
                    p.published = 0;
                }
                self.account_dialog = None;
                self.toast(tr("Your account was deleted"), ctx);
            }
            AccountDone::Published { name, id, fingerprint } => {
                if let Some(p) = self.saved.playlists.iter_mut().find(|p| p.name == name && p.public_id.is_none()) {
                    p.public_id = Some(id.clone());
                    p.published = fingerprint;
                }
                ctx.copy_text(crate::account::playlist_link(&id));
                self.toast(tr("Your playlist is public. Link copied."), ctx);
                self.save_now = true;
            }
            AccountDone::Synced { id, fingerprint } => {
                self.syncing.remove(&id);
                if let Some(p) = self.saved.playlists.iter_mut().find(|p| p.public_id.as_deref() == Some(id.as_str())) {
                    p.published = fingerprint;
                }
                self.cache.remove(&format!("public:{id}"));
            }
            AccountDone::Unpublished { id } => {
                if let Some(p) = self.saved.playlists.iter_mut().find(|p| p.public_id.as_deref() == Some(id.as_str())) {
                    p.public_id = None;
                    p.published = 0;
                }
                self.cache.remove(&format!("public:{id}"));
                self.toast(tr("Your playlist is private again"), ctx);
                self.save_now = true;
            }
            AccountDone::Done { message, reload } => {
                self.account_dialog = None;
                if let Some(key) = reload {
                    self.cache.remove(&key);
                }
                if !message.is_empty() {
                    self.toast(&message, ctx);
                }
            }
        }
    }

    /// Makes your playlist number `i` public.
    pub fn publish_playlist(&mut self, i: usize, ctx: &egui::Context) {
        if crate::account::session().is_none() {
            self.account_dialog = Some(AccountDialog::new(AccountMode::SignUp));
            return;
        }
        let Some(p) = self.saved.playlists.get(i) else { return };
        let (name, tracks, fingerprint) = (p.name.clone(), p.tracks.clone(), p.fingerprint());
        self.account_job(ctx, move || crate::account::publish(&name, &tracks).map(|id| AccountDone::Published { name, id, fingerprint }));
    }

    pub fn unpublish_playlist(&mut self, i: usize, ctx: &egui::Context) {
        let Some(id) = self.saved.playlists.get(i).and_then(|p| p.public_id.clone()) else { return };
        self.account_job(ctx, move || crate::account::unpublish(&id).map(|_| AccountDone::Unpublished { id }));
    }

    /// Public playlists you changed are sent again (checked every few seconds, so a run of
    /// edits goes up once).
    fn sync_public_playlists(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now - self.synced_at < 4.0 || crate::account::session().is_none() {
            return;
        }
        self.synced_at = now;
        let changed: Vec<(String, String, Vec<Track>, u64)> = self
            .saved
            .playlists
            .iter()
            .filter_map(|p| {
                let id = p.public_id.clone()?;
                let f = p.fingerprint();
                (f != p.published && !self.syncing.contains(&id)).then(|| (id, p.name.clone(), p.tracks.clone(), f))
            })
            .collect();
        for (id, name, tracks, fingerprint) in changed {
            self.syncing.insert(id.clone());
            self.account_busy += 1;
            let (tx, ctx) = (self.account_tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let r = match crate::account::update(&id, &name, &tracks) {
                    Ok(()) => Ok(AccountDone::Synced { id, fingerprint }),
                    // Deleted on the server (or hidden): it's private now.
                    Err(e) if e.contains("isn't public") => Ok(AccountDone::Unpublished { id }),
                    Err(e) => Err(e),
                };
                let _ = tx.send(r);
                ctx.request_repaint();
            });
        }
    }

    /// Opens a pasted Pulse playlist or profile link. False if it isn't one.
    pub fn open_link(&mut self, text: &str, ctx: &egui::Context) -> bool {
        match crate::account::read_link(text) {
            Some(crate::account::Link::Playlist(id)) => self.navigate(Route::Public(id), ctx),
            Some(crate::account::Link::Profile(name)) => self.navigate(Route::User(name), ctx),
            None => return false,
        }
        true
    }

    /// Copies a public playlist into your own playlists.
    pub fn save_copy(&mut self, p: &crate::account::Public, ctx: &egui::Context) {
        let now = now_unix();
        let mut name = p.name.clone();
        let mut n = 2;
        while self.saved.playlists.iter().any(|x| x.name == name) {
            name = format!("{} ({n})", p.name);
            n += 1;
        }
        let added = p.tracks.iter().map(|t| (t.id.clone(), now)).collect();
        self.saved.playlists.push(LocalPlaylist { name: name.clone(), tracks: p.tracks.clone(), cover: None, added, public_id: None, published: 0 });
        self.save_now = true;
        self.toast(&format!("{} “{name}”", crate::i18n::tr("Saved a copy:")), ctx);
    }

    /// Follows an artist, or stops following them.
    pub fn toggle_follow(&mut self, a: &deezer::ArtistInfo, ctx: &egui::Context) {
        if let Some(i) = self.saved.followed_artists.iter().position(|x| x.id == a.id) {
            self.saved.followed_artists.remove(i);
            self.toast(&format!("{} {}", crate::i18n::tr("Unfollowed"), a.name), ctx);
        } else {
            self.saved.followed_artists.insert(0, a.clone());
            self.toast(&format!("{} {}", crate::i18n::tr("Following"), a.name), ctx);
        }
    }

    pub fn follows(&self, id: &str) -> bool {
        self.saved.followed_artists.iter().any(|a| a.id == id)
    }

    /// Asks (in the background) what other people search for that starts with `typed`.
    /// One question at a time; the newest wins. Not in Gaming and Ultra-low modes.
    pub fn want_popular_searches(&mut self, typed: &str, ctx: &egui::Context) {
        let q = typed.trim().to_lowercase();
        if q.chars().count() < 2 || self.popular_searches.contains_key(&q) || crate::perf::mode() != crate::perf::Mode::Normal {
            return;
        }
        if self.popular_rx.as_ref().is_some_and(|(asked, _)| *asked == q) {
            return;
        }
        let (tx, rx) = channel();
        self.popular_rx = Some((q.clone(), rx));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::suggest::popular(&q));
            ctx.request_repaint();
        });
    }

    fn receive_popular_searches(&mut self) {
        let Some((q, rx)) = &self.popular_rx else { return };
        match rx.try_recv() {
            Ok(list) => {
                let q = q.clone();
                if self.popular_searches.len() > 300 {
                    self.popular_searches.clear();
                }
                self.popular_searches.insert(q, list);
                self.popular_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.popular_rx = None,
            Err(_) => {}
        }
    }

    /// Saves an album to your library, or takes it out again.
    pub fn toggle_album(&mut self, a: &deezer::AlbumInfo, ctx: &egui::Context) {
        if let Some(i) = self.saved.liked_albums.iter().position(|x| x.id == a.id) {
            self.saved.liked_albums.remove(i);
            self.toast("Removed from Liked Albums", ctx);
        } else {
            self.saved.liked_albums.insert(0, a.clone());
            self.toast("Saved to Liked Albums", ctx);
        }
    }

    /// Saves a playlist to your library, or takes it out again.
    pub fn toggle_playlist(&mut self, p: &deezer::PlaylistInfo, ctx: &egui::Context) {
        if let Some(i) = self.saved.liked_playlists.iter().position(|x| x.id == p.id) {
            self.saved.liked_playlists.remove(i);
            self.toast("Removed from Liked Playlists", ctx);
        } else {
            self.saved.liked_playlists.insert(0, p.clone());
            self.toast("Saved to Liked Playlists", ctx);
        }
    }

    /// Opens the song's artist's page.
    pub fn open_artist(&mut self, t: &Track, ctx: &egui::Context) {
        let a = t.artist.clone();
        self.open_artist_ref(&a, ctx);
    }

    /// Opens an artist's page. Songs from imports and YouTube only know the artist's name:
    /// the page is then looked up by name (once; the answer is remembered), and only if no
    /// artist by that name turns up does it search for them instead.
    pub fn open_artist_ref(&mut self, a: &deezer::ArtistRef, ctx: &egui::Context) {
        if deezer::is_deezer_id(&a.id) {
            self.navigate(Route::Artist(a.id.clone()), ctx);
            return;
        }
        if let Some(id) = self.saved.artist_ids.get(&a.name).cloned() {
            self.navigate(Route::Artist(id), ctx);
            return;
        }
        if a.name.trim().is_empty() {
            return;
        }
        let (tx, rx) = channel();
        self.artist_lookup = Some((a.name.clone(), rx));
        let (name, ctx2) = (a.name.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(deezer::find_artist(&name).ok().flatten());
            ctx2.request_repaint();
        });
    }

    /// The answer to an artist looked up by name: open their page, else search for them.
    fn receive_artist_lookup(&mut self, ctx: &egui::Context) {
        let Some((name, rx)) = &self.artist_lookup else {
            return;
        };
        let Ok(found) = rx.try_recv() else { return };
        let name = name.clone();
        self.artist_lookup = None;
        match found {
            Some(id) => {
                self.saved.artist_ids.insert(name.clone(), id.clone());
                // Every saved song by them links straight to the page from now on.
                let lists = self.saved.playlists.iter_mut().flat_map(|p| p.tracks.iter_mut());
                let others = self.saved.liked.iter_mut().chain(self.saved.recent.iter_mut()).chain(self.queue.iter_mut()).chain(self.current.iter_mut());
                for t in lists.chain(others) {
                    if t.artist.name == name && !deezer::is_deezer_id(&t.artist.id) {
                        t.artist.id = id.clone();
                    }
                    for f in t.featured.iter_mut().filter(|f| f.name == name && !deezer::is_deezer_id(&f.id)) {
                        f.id = id.clone();
                    }
                }
                self.navigate(Route::Artist(id), ctx);
            }
            None => {
                self.search_text = name.clone();
                self.navigate(Route::Search(name), ctx);
            }
        }
    }

    pub fn toggle_autoplay(&mut self) {
        self.saved.autoplay = !self.saved.autoplay;
        if !self.saved.autoplay {
            self.advance_when_autoplay_arrives = false;
        }
    }

    // ---------------------------------------------------------------- YouTube

    /// How to get the audio for `track`: a link found ahead of time, or find it on
    /// YouTube now. Pulse only ever plays full songs, never 30-second previews.
    fn resolver(&self, track: &Track) -> Resolver {
        let track = track.clone();
        let ids = self.youtube_ids.clone();
        let prefetched = self.prefetched.clone();
        let in_flight = self.prefetching.clone();
        let offline = self.saved.downloads.get(&track.id).cloned();
        let quality = self.saved.settings.stream_quality;
        Box::new(move |status| {
            // Already being found (the pointer rested on it, or it's next in the queue):
            // wait for that instead of starting over.
            let started = Instant::now();
            while in_flight.lock().unwrap().contains(&track.id) && started.elapsed() < Duration::from_secs(12) {
                std::thread::sleep(Duration::from_millis(40));
            }
            if let Some(src) = offline_source(&track, offline.as_deref()) {
                return src;
            }
            // Played before and still kept: starts at once, nothing to look up.
            if let Some(src) = offline_source(&track, crate::audiocache::find(&track.id).as_deref()) {
                return src;
            }
            if let Some((src, at)) = prefetched.lock().unwrap().remove(&track.id)
                && at.elapsed() < LINK_LIFETIME
            {
                return Ok(kept_for_next_time(src, &track));
            }
            let known = ids.lock().unwrap().get(&track.id).cloned();
            match crate::youtube::resolve(&track, known, quality, status) {
                Ok((src, video)) => {
                    ids.lock().unwrap().insert(track.id.clone(), video);
                    Ok(kept_for_next_time(src, &track))
                }
                Err(e) => {
                    // A saved video id may have gone away: forget it so the next try searches again.
                    ids.lock().unwrap().remove(&track.id);
                    Err(e)
                }
            }
        })
    }

    /// Loads the next two songs in the queue ahead of time, so pressing Next (or the song
    /// ending) starts them right away.
    fn prefetch_next(&mut self) {
        let upcoming: Vec<Track> = self.queue.iter().skip(self.queue_pos + 1).take(2).cloned().collect();
        self.warm(upcoming, true);
    }

    /// Opening a playlist, album or Orbit loads its first songs ahead of time, so the one
    /// you press starts at once. Done once per visit; not in Gaming and Ultra-low modes.
    pub fn warm_list(&mut self, tracks: &[Track]) {
        let first: Vec<String> = tracks.iter().take(3).map(|t| t.id.clone()).collect();
        if first.is_empty() || first == self.warmed_list {
            return;
        }
        self.warmed_list = first;
        if crate::perf::mode() != crate::perf::Mode::Normal {
            return;
        }
        // A new list replaces the last one's songs that haven't started yet.
        self.warm_queue.lock().unwrap().clear();
        self.warm(tracks.iter().take(3).cloned().collect(), false);
    }

    /// Finds one song's audio link right away (the song under the pointer), so pressing it
    /// plays without the YouTube lookup. Only the link, never the whole song; on in every
    /// mode but Ultra-low, since it's the difference between a song starting in about a
    /// second or in several.
    pub fn warm_link(&mut self, t: Track) {
        if crate::perf::mode() == crate::perf::Mode::UltraLow || !self.needs_link(&t) {
            return;
        }
        self.prefetching.lock().unwrap().insert(t.id.clone());
        let (ids, prefetched, in_flight) = (self.youtube_ids.clone(), self.prefetched.clone(), self.prefetching.clone());
        let quality = self.saved.settings.stream_quality;
        std::thread::spawn(move || {
            let known = ids.lock().unwrap().get(&t.id).cloned();
            if let Ok((src, video)) = crate::youtube::resolve(&t, known, quality, &|_| {}) {
                ids.lock().unwrap().insert(t.id.clone(), video);
                let mut p = prefetched.lock().unwrap();
                p.retain(|_, (_, at)| at.elapsed() < LINK_LIFETIME);
                p.insert(t.id.clone(), (src, Instant::now()));
            }
            in_flight.lock().unwrap().remove(&t.id);
        });
    }

    /// Does a song still need its audio link found?
    fn needs_link(&self, t: &Track) -> bool {
        let link_ready = self.prefetched.lock().unwrap().get(&t.id).is_some_and(|(_, at)| at.elapsed() < LINK_LIFETIME);
        !link_ready
            && !crate::local::is_local(&t.id)
            && !self.saved.downloads.contains_key(&t.id)
            && !crate::audiocache::has(&t.id)
            && self.current.as_ref().is_none_or(|c| c.id != t.id)
            && !self.prefetching.lock().unwrap().contains(&t.id)
    }

    /// Does a song still need loading ahead?
    fn needs_warming(&self, t: &Track) -> bool {
        let link_ready = self.prefetched.lock().unwrap().get(&t.id).is_some_and(|(_, at)| at.elapsed() < LINK_LIFETIME);
        !crate::local::is_local(&t.id)
            && !self.saved.downloads.contains_key(&t.id)
            && !self.prefetch_failed.contains(&t.id)
            && !crate::audiocache::has(&t.id)
            && self.current.as_ref().is_none_or(|c| c.id != t.id)
            && !self.prefetching.lock().unwrap().iter().any(|f| f == &t.id || f.strip_prefix("dl:") == Some(t.id.as_str()))
            // Without kept songs only the link is fetched ahead.
            && (crate::audiocache::enabled() || !link_ready)
    }

    /// Adds songs to be loaded ahead (at the front when `first`) and starts the worker.
    /// The worker finds each song's audio link, then (when played songs are kept) downloads
    /// the song into that store, one song at a time so it never competes much with playback.
    fn warm(&mut self, tracks: Vec<Track>, first: bool) {
        let wanted: Vec<Track> = tracks.into_iter().filter(|t| self.needs_warming(t)).collect();
        if wanted.is_empty() {
            return;
        }
        {
            let mut q = self.warm_queue.lock().unwrap();
            q.retain(|t| !wanted.iter().any(|w| w.id == t.id));
            if first {
                for t in wanted.into_iter().rev() {
                    q.push_front(t);
                }
            } else {
                q.extend(wanted);
            }
            q.truncate(8);
        }
        if self.warm_busy.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return; // the worker is running and will get to them
        }
        let queue = self.warm_queue.clone();
        let busy = self.warm_busy.clone();
        let ids = self.youtube_ids.clone();
        let prefetched = self.prefetched.clone();
        let in_flight = self.prefetching.clone();
        let failed = self.prefetch_failed_tx.clone();
        let quality = self.saved.settings.stream_quality;
        std::thread::spawn(move || {
            loop {
                let next = queue.lock().unwrap().pop_front();
                let Some(t) = next else {
                    busy.store(false, std::sync::atomic::Ordering::Release);
                    // Songs added just as it was finishing: carry on, unless another worker took them.
                    if queue.lock().unwrap().is_empty() || busy.swap(true, std::sync::atomic::Ordering::AcqRel) {
                        break;
                    }
                    continue;
                };
                in_flight.lock().unwrap().insert(t.id.clone());
                // While its link is found the song counts as in flight (playing it waits for
                // that); while it's downloaded ahead, as "dl:<id>" (playing it doesn't wait).
                let dl = format!("dl:{}", t.id);
                warm_one(&t, &ids, &prefetched, quality, &failed, &|| {
                    let mut f = in_flight.lock().unwrap();
                    f.remove(&t.id);
                    f.insert(dl.clone());
                });
                let mut f = in_flight.lock().unwrap();
                f.remove(&t.id);
                f.remove(&dl);
            }
        });
    }

    // ---------------------------------------------------------------- autoplay

    /// On the last song of the queue: start loading similar songs to play next.
    fn maybe_fetch_autoplay(&mut self, ctx: &egui::Context) {
        if !self.saved.autoplay || self.saved.repeat != Repeat::Off || self.autoplay_rx.is_some() {
            return;
        }
        if self.queue_pos + 1 < self.queue.len() {
            return;
        }
        let Some(cur) = self.current.clone() else {
            return;
        };
        if !cur.has_artist_page() {
            return; // similar songs come from Deezer; local files have no Deezer artist
        }
        if self.autoplay_for.as_deref() == Some(cur.id.as_str()) {
            return;
        }
        self.autoplay_for = Some(cur.id.clone());
        let (tx, rx) = channel();
        self.autoplay_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(deezer::similar_to(&cur).unwrap_or_default());
            ctx.request_repaint();
        });
    }

    fn receive_autoplay(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.autoplay_rx else { return };
        let Ok(found) = rx.try_recv() else { return };
        self.autoplay_rx = None;
        // Skip songs that were just played or are already queued.
        let recent: HashSet<&str> = self.saved.recent.iter().take(30).map(|t| t.id.as_str()).collect();
        let queued: HashSet<String> = self.queue.iter().map(|t| t.id.clone()).collect();
        let mut added = 0;
        for t in found {
            if added >= 15 || recent.contains(t.id.as_str()) || queued.contains(&t.id) {
                continue;
            }
            self.autoplay_ids.insert(t.id.clone());
            if let Some(u) = &mut self.unshuffled {
                u.push(t.clone());
            }
            self.queue.push(t);
            added += 1;
        }
        if self.advance_when_autoplay_arrives {
            self.advance_when_autoplay_arrives = false;
            if added > 0 {
                self.next(ctx);
            } else {
                self.audio.stop();
                self.needs_load = true;
            }
        }
    }

    /// Applies what the user did in a track list. `list` is the list that was shown.
    /// A song's title was clicked: like Spotify, open its album with the song highlighted
    /// (songs without an album open their own page).
    pub fn open_song(&mut self, t: &Track, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some(al) = t.album.as_ref().filter(|a| deezer::is_deezer_id(&a.id)) {
            self.navigate(Route::Album(al.id.clone()), ctx);
        } else {
            self.song_tracks.insert(t.id.clone(), t.clone());
            self.navigate(Route::Song(t.id.clone()), ctx);
        }
        self.highlight = Some((t.id.clone(), now));
    }

    /// Opens the credits window and loads the credits in the background.
    pub fn show_credits(&mut self, t: Track, ctx: &egui::Context) {
        let (tx, rx) = channel();
        let id = t.deezer_number();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let r = match id {
                Some(id) => crate::deezer::credits(id),
                None => Err("Credits are only available for songs from the catalog.".into()),
            };
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
        self.credits = Some((t, None));
        self.credits_rx = Some(rx);
    }

    /// Checks whether credits finished loading (called every frame).
    pub fn receive_credits(&mut self) {
        if let Some(rx) = &self.credits_rx
            && let Ok(r) = rx.try_recv()
        {
            if let Some((_, slot)) = &mut self.credits {
                *slot = Some(r);
            }
            self.credits_rx = None;
        }
    }

    /// A link to `t` on YouTube, Spotify or Deezer, for the Share menu.
    pub fn share_link(&self, t: &Track, target: ShareTarget) -> String {
        share_link(t, target, self.youtube_ids.lock().unwrap().get(&t.id).map(String::as_str))
    }

    pub fn handle_events(&mut self, events: Vec<TrackEvent>, list: &[Track], ctx: &egui::Context) {
        for ev in events {
            match ev {
                TrackEvent::Play(i) => {
                    if self.route == Route::Queue {
                        // In the queue view, list = upcoming tracks after the current one.
                        self.queue_pos += i + 1;
                        self.start_current(ctx);
                    } else {
                        self.play_list(list, i, ctx);
                    }
                }
                TrackEvent::PlayNext(t) => {
                    let at = if self.queue.is_empty() { 0 } else { self.queue_pos + 1 };
                    self.queue.insert(at, t.clone());
                    if let Some(u) = &mut self.unshuffled {
                        u.push(t.clone());
                    }
                    self.toast(&format!("“{}” will play next", t.title), ctx);
                    if self.current.is_none() {
                        self.queue_pos = 0;
                        self.start_current(ctx);
                    }
                }
                TrackEvent::AddToQueue(t) => {
                    self.queue.push(t.clone());
                    if let Some(u) = &mut self.unshuffled {
                        u.push(t.clone());
                    }
                    self.toast("Added to queue", ctx);
                    if self.current.is_none() {
                        self.queue_pos = self.queue.len() - 1;
                        self.start_current(ctx);
                    }
                }
                TrackEvent::ToggleLike(t) => self.toggle_like(&t, ctx),
                TrackEvent::AddToPlaylist(pi, t) => {
                    if let Some(p) = self.saved.playlists.get_mut(pi) {
                        if p.tracks.iter().any(|x| x.id == t.id) {
                            let msg = format!("Already in “{}”", p.name);
                            self.toast(&msg, ctx);
                        } else {
                            p.added.insert(t.id.clone(), now_unix());
                            p.tracks.push(t);
                            let msg = format!("Added to “{}”", p.name);
                            self.toast(&msg, ctx);
                        }
                    }
                }
                TrackEvent::NewPlaylistWith(t) => self.new_playlist = Some((String::new(), vec![t])),
                TrackEvent::NewPlaylistWithMany(list) => self.new_playlist = Some((String::new(), list)),
                TrackEvent::Like(t) => {
                    if !self.liked_ids.contains(&t.id) {
                        self.liked_ids.insert(t.id.clone());
                        self.saved.liked_added.insert(t.id.clone(), now_unix());
                        self.saved.liked.insert(0, t);
                    }
                }
                TrackEvent::Select { list, index, ctrl, shift } => {
                    let (cur, rows, anchor) = &mut self.selection;
                    if *cur != list {
                        *cur = list;
                        rows.clear();
                        *anchor = None;
                    }
                    match (shift, *anchor) {
                        (true, Some(a)) => {
                            if !ctrl {
                                rows.clear();
                            }
                            rows.extend(a.min(index)..=a.max(index));
                        }
                        _ if ctrl => {
                            if !rows.remove(&index) {
                                rows.insert(index);
                            }
                            *anchor = Some(index);
                        }
                        _ => {
                            rows.clear();
                            rows.insert(index);
                            *anchor = Some(index);
                        }
                    }
                }
                TrackEvent::SelectAll { list, len } => {
                    self.selection = (list, (0..len).collect(), Some(0));
                }
                TrackEvent::ClearSelection => self.selection.1.clear(),
                TrackEvent::Warm(t) => self.warm_link(t),
                TrackEvent::Preview(t) => {
                    let route = match t.album.as_ref().filter(|a| deezer::is_deezer_id(&a.id)) {
                        Some(al) => Route::Album(al.id.clone()),
                        None => Route::Song(t.id.clone()),
                    };
                    self.preload(route, ctx);
                }
                TrackEvent::OpenSongPage(t) => {
                    self.song_tracks.insert(t.id.clone(), t.clone());
                    self.navigate(Route::Song(t.id.clone()), ctx);
                }
                TrackEvent::RemoveFromPlaylist(i) => {
                    if let Route::Local(pi) = self.route
                        && let Some(p) = self.saved.playlists.get_mut(pi)
                        && i < p.tracks.len()
                    {
                        p.tracks.remove(i);
                    }
                }
                TrackEvent::MoveInQueue(i, by) => {
                    let from = self.queue_pos + 1 + i;
                    let last = self.queue.len().saturating_sub(1);
                    let to = (from as isize + by).clamp(self.queue_pos as isize + 1, last as isize) as usize;
                    if from <= last && to != from {
                        let t = self.queue.remove(from);
                        self.queue.insert(to, t);
                    }
                }
                TrackEvent::RemoveFromQueue(i) => {
                    let idx = self.queue_pos + 1 + i;
                    if idx < self.queue.len() {
                        let t = self.queue.remove(idx);
                        if let Some(u) = &mut self.unshuffled
                            && let Some(p) = u.iter().position(|x| x.id == t.id)
                        {
                            u.remove(p);
                        }
                    }
                }
                TrackEvent::OpenArtist(t) => self.open_artist(&t, ctx),
                TrackEvent::OpenArtistRef(a) => self.open_artist_ref(&a, ctx),
                TrackEvent::OpenAlbum(id) => self.navigate(Route::Album(id), ctx),
                TrackEvent::ShowCredits(t) => self.show_credits(t, ctx),
                TrackEvent::OpenSong(t) => self.open_song(&t, ctx),
                TrackEvent::Download(t) => self.download(&[t], ctx),
                TrackEvent::RemoveDownload(t) => self.remove_download(&t, ctx),
                TrackEvent::Share(t, target) => {
                    let link = self.share_link(&t, target);
                    ctx.copy_text(link);
                    self.toast("Link copied", ctx);
                }
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.diag.gpu.is_none() {
            self.diag.gpu = Some(crate::diag::gpu(frame));
            crate::perf::set_software(crate::diag::software(frame));
        }
        self.updates(&ctx);
        self.discord_status(&ctx);
        // Developer check: PULSE_DEBUG_REPAINT=1 prints what asked for each redraw.
        static DEBUG_REPAINT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *DEBUG_REPAINT.get_or_init(|| std::env::var_os("PULSE_DEBUG_REPAINT").is_some()) {
            eprintln!("frame {} causes {:?}", ctx.cumulative_frame_nr(), ctx.repaint_causes());
        }
        self.tick(&ctx);
        if let Some(note) = self.startup_note.take() {
            self.toast(crate::i18n::tr(&note), &ctx);
        }
        // Minimized: nothing to draw (the music and the mini player carry on).
        if !ctx.input(|i| i.viewport().minimized.unwrap_or(false)) {
            crate::ui::draw(self, ui);
        }
        crate::ui::mini::show(self, &ctx);
        if std::mem::take(&mut self.save_now)
            && let Some(storage) = frame.storage_mut()
        {
            App::save(self, storage);
            storage.flush();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        App::save(self, storage);
    }

    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(15)
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;

    fn song(title: &str, artist: &str) -> Track {
        let mut t = Track { title: title.into(), ..Default::default() };
        t.artist.name = artist.into();
        t
    }

    #[test]
    fn local_search_matches_parts_accents_and_small_typos() {
        let words = |q: &str| fold(q).split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        let t = song("Halo", "Beyoncé");
        assert!(track_matches(&t, &words("beyonce")));
        assert!(track_matches(&t, &words("beyonse halo")));
        assert!(track_matches(&t, &words("bey")));
        assert!(!track_matches(&t, &words("adele")));
        assert!(one_edit_apart("drake", "drakes"));
        assert!(!one_edit_apart("drake", "brakes"));
    }
}

#[cfg(test)]
mod share_tests {
    use super::*;

    fn song(id: &str, title: &str, artist: &str, album: &str, secs: u32) -> Track {
        Track {
            id: id.into(),
            title: title.into(),
            duration: secs,
            artist: deezer::ArtistRef { name: artist.into(), ..Default::default() },
            album: Some(deezer::AlbumRef { id: album.into(), title: album.into() }),
            ..Default::default()
        }
    }

    #[test]
    fn remembers_what_you_play() {
        let v = |id: &str, last: u64| Visit {
            place: Place::Artist(id.into()),
            title: id.into(),
            subtitle: String::new(),
            image: None,
            round: true,
            count: 0,
            last,
        };
        let mut list = vec![];
        bump(&mut list, v("a", 100), 3);
        bump(&mut list, v("b", 100), 3);
        bump(&mut list, v("a", 100), 3);
        assert_eq!(list[0].place, Place::Artist("a".into()));
        assert_eq!(list[0].count, 2);
        bump(&mut list, v("c", 100), 3);
        bump(&mut list, v("d", 100), 3);
        assert_eq!(list.len(), 3, "keeps at most 3");
        // Played often long ago vs. a little recently.
        let mut old = v("old", 0);
        old.count = 10;
        let mut new = v("new", 365 * 86_400);
        new.count = 3;
        let both = [old, new];
        let top = top_artists(&both, 365 * 86_400);
        assert_eq!(top[0].title, "new");
    }

    #[test]
    fn playlist_search_genre_and_sort() {
        let list = vec![song("a", "Zebra", "Bee", "1", 200), song("b", "Apple", "Cat", "2", 100), song("c", "Mango", "Ant", "1", 300)];
        let genres = HashMap::from([("1".to_string(), vec!["Rap".to_string()]), ("2".to_string(), vec!["Pop".to_string()])]);
        let added = HashMap::from([("a".to_string(), 10), ("b".to_string(), 30), ("c".to_string(), 20)]);
        let ids = |v: Vec<Track>| v.into_iter().map(|t| t.id).collect::<Vec<_>>();
        let mut view = ListView::default();
        assert_eq!(ids(apply_view(&list, &view, Some(&added), &genres)), ["a", "b", "c"]);
        view.sort = SortBy::Title;
        assert_eq!(ids(apply_view(&list, &view, Some(&added), &genres)), ["b", "c", "a"]);
        view.sort = SortBy::RecentlyAdded;
        assert_eq!(ids(apply_view(&list, &view, Some(&added), &genres)), ["b", "c", "a"]);
        view.sort = SortBy::Artist;
        assert_eq!(ids(apply_view(&list, &view, Some(&added), &genres)), ["c", "a", "b"]);
        view.genre = Some("Rap".into());
        assert_eq!(ids(apply_view(&list, &view, Some(&added), &genres)), ["c", "a"]);
        view.search = "zeb".into();
        assert_eq!(ids(apply_view(&list, &view, Some(&added), &genres)), ["a"]);
        assert_eq!(genre_chips(&list, &genres), ["Rap", "Pop"]);
    }

    #[test]
    fn share_links_point_to_the_right_place() {
        let t = Track { id: "dz:123".into(), title: "Song A".into(), ..Default::default() };
        assert_eq!(share_link(&t, ShareTarget::YouTube, Some("abcdefghijk")), "https://www.youtube.com/watch?v=abcdefghijk");
        assert!(share_link(&t, ShareTarget::YouTube, None).starts_with("https://www.youtube.com/results?search_query="));
        assert!(share_link(&t, ShareTarget::Spotify, None).contains("Song%20A"));
        assert_eq!(share_link(&t, ShareTarget::Deezer, None), "https://www.deezer.com/track/123");
    }
}

#[cfg(test)]
mod ahead_tests {
    use super::*;

    #[test]
    fn songs_loaded_ahead_land_in_the_kept_songs() {
        let song: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        let url = crate::audio::tests::slow_server(song.clone(), 40_000_000, true);
        let src = StreamSource {
            url,
            headers: vec![],
            codec: crate::audio::Codec::Mp4,
            content_length: Some(song.len() as u64),
            notice: None,
            duration: None,
            keep_as: None,
        };
        let dir = std::env::temp_dir().join(format!("pulse-ahead-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::audiocache::set_limit_mb(1024);
        let dest = dir.join("dz_1.m4a");
        assert!(keep_ahead(&src, &dest));
        assert_eq!(std::fs::read(&dest).unwrap(), song);
        assert!(!dest.with_extension("ahead").exists(), "no half file left behind");
        // A server that's gone: nothing kept, nothing left behind.
        let gone = StreamSource { url: "http://127.0.0.1:9/x".into(), ..src };
        assert!(!keep_ahead(&gone, &dir.join("dz_2.m4a")));
        assert!(!dir.join("dz_2.m4a").exists() && !dir.join("dz_2.ahead").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
