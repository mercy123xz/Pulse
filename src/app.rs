//! App state and logic: navigation, data loading, the play queue, likes and playlists.
//! The drawing code lives in `ui.rs`.

use crate::audio::{AudioEngine, LoadState, Resolver, StreamSource};
use crate::deezer::{self, AlbumInfo, ArtistPage, Chart, PlaylistInfo, SearchResults, Track};
use crate::widgets::{ShareTarget, TrackEvent};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
}

impl Page {
    /// Every song on the page (for updating details that arrive later).
    pub fn for_each_track(&mut self, mut f: impl FnMut(&mut Track)) {
        match self {
            Page::Home(c) => c.tracks.iter_mut().for_each(&mut f),
            Page::Tracks(t) => t.iter_mut().for_each(&mut f),
            Page::Search(r) => r.tracks.iter_mut().for_each(&mut f),
            Page::Playlist { tracks, .. } | Page::Album { tracks, .. } => tracks.iter_mut().for_each(&mut f),
            Page::Artist(p) => p.top.iter_mut().for_each(&mut f),
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
    }))
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
    /// Songs whose audio link is being found ahead of time (one at a time).
    prefetching: Arc<Mutex<HashSet<String>>>,
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
    featured_tx: Sender<(String, Vec<deezer::ArtistRef>)>,
    featured_rx: Receiver<(String, Vec<deezer::ArtistRef>)>,
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
    /// Lyrics looked up this session, by song id.
    pub lyrics: HashMap<String, LyricsState>,
    lyrics_tx: Sender<(String, Result<Option<crate::lyrics::Lyrics>, String>)>,
    lyrics_rx: Receiver<(String, Result<Option<crate::lyrics::Lyrics>, String>)>,
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
    /// The song downloading now and how far along it is.
    pub downloading: Option<(Track, Arc<Mutex<f32>>)>,
    download_rx: Option<Receiver<DownloadResult>>,

    /// Search / genre / sort of the list on screen (reset when the page changes).
    pub list_view: ListView,

    /// The "Import a playlist" window: the link typed so far, and the running import.
    pub import_open: bool,
    pub import_url: String,
    pub import_running: Option<ImportJob>,
    /// The profile form, when open (first-launch welcome or "Edit profile").
    pub profile_editing: Option<crate::ui::ProfileForm>,

    // YouTube view counts
    views_queue: std::collections::VecDeque<Track>,
    views_pending: HashSet<String>,
    /// Songs whose lookup found nothing this session (not asked again until restart).
    views_failed: HashSet<String>,
    views_tx: Sender<(String, Option<(String, u64)>)>,
    views_rx: Receiver<(String, Option<(String, u64)>)>,
    genre_rx: Option<Receiver<(String, Vec<String>)>>,
    genre_pending: HashSet<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::install(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        // Pictures from the web: saved on disk, let go of in memory when off screen.
        crate::artcache::ArtLoader::install(&cc.egui_ctx);

        let saved: Saved = cc.storage.and_then(|s| eframe::get_value(s, STORAGE_KEY)).unwrap_or_default();
        let themes = crate::themes::list();
        crate::i18n::set_language(saved.settings.language);
        crate::perf::apply(saved.settings.perf_mode, &saved.settings.perf_keep);
        crate::theme::set_reduce_motion(saved.settings.reduce_motion || !crate::perf::on(crate::perf::Extra::Animations));
        let (tx, rx) = channel();
        let (views_tx, views_rx) = channel();
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
            lyrics: HashMap::new(),
            lyrics_tx,
            lyrics_rx,
            cache_used: HashMap::new(),
            song_tracks: HashMap::new(),
            rematch_tried: HashSet::new(),
            featured_queue: Default::default(),
            featured_busy: false,
            featured_tx,
            featured_rx,
            rematch_rx: None,
            told_next: None,
            themes,
            theme_watch: crate::themes::watch(&cc.egui_ctx),
            theme_changed_at: None,
            local_files: vec![],
            local_scanning: false,
            local_rx: None,
            download_queue: Default::default(),
            downloading: None,
            download_rx: None,
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
        std::thread::spawn(crate::artcache::trim_disk);
        app.apply_theme(&cc.egui_ctx);
        // First launch: offer to make a profile.
        if app.saved.profile.created == 0 && !app.saved.profile.skipped {
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
        let mut ids = self.youtube_ids.lock().unwrap().clone();
        if ids.len() > 5000 {
            ids.clear(); // keep the saved file small; ids are found again when needed
        }
        self.saved.youtube_ids = ids;
        eframe::set_value(storage, STORAGE_KEY, &self.saved);
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
        if route == self.route {
            return;
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
            Route::Song(id) if deezer::is_deezer_id(id.trim_start_matches("dz:")) => format!("song:{id}"),
            Route::Song(_) => return None,
            Route::Liked | Route::Recent | Route::Queue | Route::Local(_) | Route::LocalFiles | Route::Settings | Route::Profile | Route::Lyrics => return None,
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
        let Some(key) = self.key_for(&route) else { return };
        // Opening a page that failed before (perhaps while loading ahead) tries again.
        if !preload && matches!(self.cache.get(&key), Some(Remote::Failed(_))) {
            self.cache.remove(&key);
        }
        if self.cache.contains_key(&key) {
            return;
        }
        let job: Box<dyn FnOnce() -> Result<Page, String> + Send> = match route {
            Route::Home => Box::new(|| deezer::chart().map(Page::Home)),
            Route::Charts => {
                let genre = self.chart_genre.0;
                Box::new(move || deezer::chart_tracks(genre).map(Page::Tracks))
            }
            Route::Search(q) => Box::new(move || deezer::search(&q).map(Page::Search)),
            Route::Playlist(id) => Box::new(move || deezer::playlist(&id).map(|(info, tracks)| Page::Playlist { info, tracks })),
            Route::Album(id) => Box::new(move || deezer::album(&id).map(|(info, tracks)| Page::Album { info, tracks })),
            Route::Artist(id) => Box::new(move || deezer::artist_page(&id).map(Page::Artist)),
            Route::Mix(id) => Box::new(move || deezer::mix(&id).map(Page::Tracks)),
            Route::Song(id) => {
                let Some(n) = id.strip_prefix("dz:").and_then(|n| n.parse::<u64>().ok()) else { return };
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
                Err(e) => Remote::Failed(e),
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

        if let Some((_, at)) = &self.toast
            && ctx.input(|i| i.time) - at > 3.5
        {
            self.toast = None;
        }
    }

    pub fn submit_search(&mut self, ctx: &egui::Context) {
        let q = self.search_text.trim().to_string();
        if q.is_empty() {
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

    pub fn toast(&mut self, msg: &str, ctx: &egui::Context) {
        self.toast = Some((msg.to_string(), ctx.input(|i| i.time)));
        ctx.request_repaint_after(Duration::from_millis(3600));
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
        let Some(track) = self.queue.get(self.queue_pos).cloned() else { return };
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

    pub fn seek_fraction(&mut self, frac: f32) {
        if let Some(t) = &self.current {
            if self.needs_load {
                return;
            }
            self.audio.seek(Duration::from_secs_f32(t.duration as f32 * frac));
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
        let Some(t) = self.current_theme() else { return };
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
        let Some(t) = self.current_theme() else { return };
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
        self.apply_perf(ctx);
        if s.perf_mode != before.perf_mode {
            let msg = match s.perf_mode {
                crate::perf::Mode::Normal => "Gaming Mode off",
                crate::perf::Mode::Gaming => "Gaming Mode on: extras paused, music unchanged",
                crate::perf::Mode::UltraLow => "Ultra-low resource mode on",
            };
            self.toast(crate::i18n::tr(msg), ctx);
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
                self.saved.album_genres.insert(id, g);
            }
        }
        if !self.genre_pending.is_empty() {
            return; // one batch at a time
        }
        let missing: Vec<String> = tracks
            .iter()
            .filter_map(|t| t.album.as_ref())
            .map(|a| a.id.clone())
            .filter(|id| deezer::is_deezer_id(id) && !self.saved.album_genres.contains_key(id))
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
                let g = deezer::album_genres(&id).unwrap_or_default();
                if tx.send((id, g)).is_err() {
                    return;
                }
                ctx.request_repaint();
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
        let p = &mut self.saved.profile;
        p.name = f.name.trim().to_string();
        p.bio = f.bio.trim().to_string();
        p.picture = picture;
        if p.created == 0 {
            p.created = now_unix();
        }
        Ok(())
    }

    /// Copies a share code for playlist `i` to the clipboard.
    pub fn share_playlist(&mut self, i: usize, ctx: &egui::Context) {
        let Some(p) = self.saved.playlists.get(i) else { return };
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
            let v = Visit { place: Place::Artist(t.artist.id.clone()), title: t.artist.name.clone(), subtitle: "Artist".into(), image, round: true, count: 0, last: now };
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
                && !self.featured_queue.contains(&n)
            {
                self.featured_queue.push_back(n);
            }
        }
    }

    fn run_featured(&mut self, ctx: &egui::Context) {
        while let Ok((id, featured)) = self.featured_rx.try_recv() {
            self.featured_busy = false;
            self.saved.featured_checked.insert(id.clone());
            if !featured.is_empty() {
                self.update_track(&id, |t| t.featured = featured.clone());
            }
        }
        if self.featured_busy {
            return;
        }
        let Some(n) = self.featured_queue.pop_front() else { return };
        self.featured_busy = true;
        let tx = self.featured_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // Paced well under Deezer's limit, since this runs alongside everything else.
            std::thread::sleep(Duration::from_millis(250));
            let featured = deezer::featured_artists(n).unwrap_or_default();
            let _ = tx.send((format!("dz:{n}"), featured));
            ctx.request_repaint();
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
            let Some(t) = self.views_queue.pop_front() else { break };
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
            let r = crate::import::run(&url, &p, &c);
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
        let Some(job) = &self.import_running else { return };
        let Ok(result) = job.rx.try_recv() else { return };
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
                self.saved.playlists.push(LocalPlaylist { name: name.clone(), tracks, cover: None, added });
                let i = self.saved.playlists.len() - 1;
                self.import_open = false;
                self.import_url.clear();
                self.navigate(Route::Local(i), ctx);
                let extra = if imp.matched < total { format!(" ({} not found on Deezer, they'll still play)", total - imp.matched) } else { String::new() };
                let from = imp.owner.map(|o| format!(" from {o}")).unwrap_or_default();
                self.toast(&format!("Imported “{name}”{from}: {total} songs{extra}"), ctx);
            }
            Err(e) => self.toast(&e, ctx),
        }
    }

    /// Reads the Music folder in the background.
    pub fn scan_local_files(&mut self, ctx: &egui::Context) {
        let Some(dir) = crate::settings::music_dir() else { return };
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
            let busy = self.downloading.as_ref().is_some_and(|(d, _)| d.id == t.id);
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

    /// Runs one download at a time and records finished ones (called every frame).
    fn run_downloads(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.download_rx
            && let Ok((t, result)) = rx.try_recv()
        {
            self.download_rx = None;
            self.downloading = None;
            match result {
                Ok((path, video)) => {
                    self.youtube_ids.lock().unwrap().insert(t.id.clone(), video);
                    self.saved.downloads.insert(t.id, path);
                }
                Err(e) => self.toast(&format!("Couldn't download “{}”: {e}", t.title), ctx),
            }
        }
        if self.downloading.is_none()
            && let Some(t) = self.download_queue.pop_front()
        {
            let progress = Arc::new(Mutex::new(0.0));
            let (tx, rx) = channel();
            self.download_rx = Some(rx);
            self.downloading = Some((t.clone(), progress.clone()));
            let known = self.youtube_ids.lock().unwrap().get(&t.id).cloned();
            let quality = self.saved.settings.download_quality;
            let dir = self.saved.settings.offline_dir();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let r = crate::downloads::download(&t, known, quality, &dir, &|p| {
                    *progress.lock().unwrap() = p;
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
        let name = if name.trim().is_empty() {
            format!("My Playlist #{}", self.saved.playlists.len() + 1)
        } else {
            name.trim().to_string()
        };
        let added = first.iter().map(|t| (t.id.clone(), now_unix())).collect();
        let mut seen = HashSet::new();
        let tracks = first.into_iter().filter(|t| seen.insert(t.id.clone())).collect();
        self.saved.playlists.push(LocalPlaylist { name: name.clone(), tracks, cover: None, added });
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

    /// Opens the artist's page (or searches their name for songs saved by Pulse 0.1).
    pub fn open_artist(&mut self, t: &Track, ctx: &egui::Context) {
        if t.has_artist_page() {
            self.navigate(Route::Artist(t.artist.id.clone()), ctx);
        } else {
            self.search_text = t.artist.name.clone();
            self.navigate(Route::Search(t.artist.name.clone()), ctx);
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
        let offline = self.saved.downloads.get(&track.id).cloned();
        let quality = self.saved.settings.stream_quality;
        Box::new(move |status| {
            if let Some(src) = offline_source(&track, offline.as_deref()) {
                return src;
            }
            if let Some((src, at)) = prefetched.lock().unwrap().remove(&track.id)
                && at.elapsed() < LINK_LIFETIME
            {
                return Ok(src);
            }
            let known = ids.lock().unwrap().get(&track.id).cloned();
            match crate::youtube::resolve(&track, known, quality, status) {
                Ok((src, video)) => {
                    ids.lock().unwrap().insert(track.id.clone(), video);
                    Ok(src)
                }
                Err(e) => {
                    // A saved video id may have gone away: forget it so the next try searches again.
                    ids.lock().unwrap().remove(&track.id);
                    Err(e)
                }
            }
        })
    }

    /// Finds the next song's audio link in the background, so it starts instantly.
    /// Finds the next two songs' audio links in the background, one at a time,
    /// so pressing Next (or the song ending) starts them right away.
    fn prefetch_next(&mut self) {
        if !self.prefetching.lock().unwrap().is_empty() {
            return; // one lookup at a time keeps it light
        }
        let upcoming: Vec<Track> = self.queue.iter().skip(self.queue_pos + 1).take(2).cloned().collect();
        let Some(next) = upcoming.into_iter().find(|t| {
            !self.prefetched.lock().unwrap().get(&t.id).is_some_and(|(_, at)| at.elapsed() < LINK_LIFETIME)
                && !crate::local::is_local(&t.id)
                && !self.saved.downloads.contains_key(&t.id)
                && !self.prefetch_failed.contains(&t.id)
        }) else {
            return;
        };
        self.prefetching.lock().unwrap().insert(next.id.clone());
        let ids = self.youtube_ids.clone();
        let prefetched = self.prefetched.clone();
        let busy = self.prefetching.clone();
        let failed = self.prefetch_failed_tx.clone();
        let quality = self.saved.settings.stream_quality;
        std::thread::spawn(move || {
            let known = ids.lock().unwrap().get(&next.id).cloned();
            match crate::youtube::resolve(&next, known, quality, &|_| {}) {
                Ok((src, video)) => {
                    ids.lock().unwrap().insert(next.id.clone(), video);
                    let mut p = prefetched.lock().unwrap();
                    p.retain(|_, (_, at)| at.elapsed() < LINK_LIFETIME);
                    p.insert(next.id.clone(), (src, Instant::now()));
                }
                Err(_) => {
                    let _ = failed.send(next.id.clone());
                }
            }
            busy.lock().unwrap().remove(&next.id);
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
        let Some(cur) = self.current.clone() else { return };
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
                TrackEvent::OpenArtistById(id) => self.navigate(Route::Artist(id), ctx),
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Developer check: PULSE_DEBUG_REPAINT=1 prints what asked for each redraw.
        static DEBUG_REPAINT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *DEBUG_REPAINT.get_or_init(|| std::env::var_os("PULSE_DEBUG_REPAINT").is_some()) {
            eprintln!("frame {} causes {:?}", ctx.cumulative_frame_nr(), ctx.repaint_causes());
        }
        self.tick(&ctx);
        // Minimized: nothing to draw (the music and the mini player carry on).
        if !ctx.input(|i| i.viewport().minimized.unwrap_or(false)) {
            crate::ui::draw(self, ui);
        }
        crate::ui::mini::show(self, &ctx);
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
        let v = |id: &str, last: u64| Visit { place: Place::Artist(id.into()), title: id.into(), subtitle: String::new(), image: None, round: true, count: 0, last };
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
