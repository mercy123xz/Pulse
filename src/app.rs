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
}

/// Data downloaded for a page.
pub enum Page {
    Home(Chart),
    Tracks(Vec<Track>),
    Search(SearchResults),
    Playlist { info: PlaylistInfo, tracks: Vec<Track> },
    Album { info: AlbumInfo, tracks: Vec<Track> },
    Artist(ArtistPage),
}

pub enum Remote {
    Loading,
    Ready(Box<Page>),
    Failed(String),
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
    pub seek_preview: Option<f32>,
    pub muted_volume: Option<f32>,
    failures_in_row: u32,
    last_title: String,

    // YouTube
    /// Shared with background threads; copied into `saved` when saving.
    youtube_ids: Arc<Mutex<HashMap<String, String>>>,
    /// Audio links found ahead of time for upcoming songs (song id → link, found at).
    prefetched: Arc<Mutex<HashMap<String, (StreamSource, Instant)>>>,
    prefetching: Option<String>,

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
    pub new_playlist: Option<(String, Option<Track>)>,
    pub rename_playlist: Option<(usize, String)>,
    /// Playlist waiting for "are you sure?" before it is deleted.
    pub confirm_delete: Option<usize>,
    /// "View credits" window: the song and its credits once loaded.
    pub credits: Option<(Track, Option<Result<crate::deezer::Credits, String>>)>,
    credits_rx: Option<Receiver<Result<crate::deezer::Credits, String>>>,
    /// Size of the cache folders, measured when Settings opens.
    pub cache_size: Option<u64>,
    /// The song last given to the audio engine as "next" (None = not told yet).
    told_next: Option<Option<String>>,

    // local files
    pub local_files: Vec<Track>,
    pub local_scanning: bool,
    local_rx: Option<Receiver<Vec<Track>>>,

    // downloads
    pub download_queue: std::collections::VecDeque<Track>,
    /// The song downloading now and how far along it is.
    pub downloading: Option<(Track, Arc<Mutex<f32>>)>,
    download_rx: Option<Receiver<DownloadResult>>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::install(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let saved: Saved = cc.storage.and_then(|s| eframe::get_value(s, STORAGE_KEY)).unwrap_or_default();
        crate::theme::apply(&cc.egui_ctx, saved.light_mode, saved.settings.high_contrast);
        crate::i18n::set_language(saved.settings.language);
        let (tx, rx) = channel();
        let audio = AudioEngine::new();
        audio.set_volume(saved.volume);

        let youtube_ids = Arc::new(Mutex::new(saved.youtube_ids.clone()));
        let mut app = Self {
            audio,
            liked_ids: saved.liked.iter().map(|t| t.id.clone()).collect(),
            current: saved.last_track.clone(),
            needs_load: saved.last_track.is_some(),
            queue: saved.last_track.clone().into_iter().collect(),
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
            queue_pos: 0,
            unshuffled: None,
            seek_preview: None,
            muted_volume: None,
            failures_in_row: 0,
            last_title: String::new(),
            youtube_ids,
            prefetched: Arc::new(Mutex::new(HashMap::new())),
            prefetching: None,
            autoplay_rx: None,
            autoplay_for: None,
            advance_when_autoplay_arrives: false,
            autoplay_ids: HashSet::new(),
            toast: None,
            new_playlist: None,
            rename_playlist: None,
            confirm_delete: None,
            credits: None,
            credits_rx: None,
            cache_size: None,
            told_next: None,
            local_files: vec![],
            local_scanning: false,
            local_rx: None,
            download_queue: Default::default(),
            downloading: None,
            download_rx: None,
        };
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
        let mut ids = self.youtube_ids.lock().unwrap().clone();
        if ids.len() > 5000 {
            ids.clear(); // keep the saved file small; ids are found again when needed
        }
        self.saved.youtube_ids = ids;
        eframe::set_value(storage, STORAGE_KEY, &self.saved);
    }

    // ---------------------------------------------------------------- navigation

    pub fn navigate(&mut self, route: Route, ctx: &egui::Context) {
        if route == self.route {
            return;
        }
        let old = std::mem::replace(&mut self.route, route);
        // Typing in search replaces the search page instead of piling up history.
        if !matches!((&old, &self.route), (Route::Search(_), Route::Search(_))) {
            self.back.push(old);
        }
        self.forward.clear();
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
        Some(match &self.route {
            Route::Home => "home".into(),
            Route::Charts => format!("charts:{}", self.chart_genre.0),
            Route::Search(q) => format!("search:{q}"),
            Route::Playlist(id) => format!("playlist:{id}"),
            Route::Album(id) => format!("album:{id}"),
            Route::Artist(id) => format!("artist:{id}"),
            Route::Liked | Route::Recent | Route::Queue | Route::Local(_) | Route::LocalFiles | Route::Settings => return None,
        })
    }

    /// Starts downloading the current page's data if we don't have it yet.
    pub fn ensure_loaded(&mut self, ctx: &egui::Context) {
        let Some(key) = self.route_key() else { return };
        if self.cache.contains_key(&key) {
            return;
        }
        self.cache.insert(key.clone(), Remote::Loading);
        let job: Box<dyn FnOnce() -> Result<Page, String> + Send> = match self.route.clone() {
            Route::Home => Box::new(|| deezer::chart().map(Page::Home)),
            Route::Charts => {
                let genre = self.chart_genre.0;
                Box::new(move || deezer::chart_tracks(genre).map(Page::Tracks))
            }
            Route::Search(q) => Box::new(move || deezer::search(&q).map(Page::Search)),
            Route::Playlist(id) => Box::new(move || deezer::playlist(&id).map(|(info, tracks)| Page::Playlist { info, tracks })),
            Route::Album(id) => Box::new(move || deezer::album(&id).map(|(info, tracks)| Page::Album { info, tracks })),
            Route::Artist(id) => Box::new(move || deezer::artist_page(&id).map(Page::Artist)),
            _ => return,
        };
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send((key, job()));
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
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.receive_credits();
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
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        // Window title shows the current song.
        let title = match &self.current {
            Some(t) if self.is_playing() => format!("{} · {} — Pulse", t.title, t.artist.name),
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
        self.saved.light_mode = !self.saved.light_mode;
        crate::theme::apply(ctx, self.saved.light_mode, self.saved.settings.high_contrast);
    }

    // ---------------------------------------------------------------- settings

    /// Puts changed settings into effect. `before` is how they were.
    pub fn apply_settings(&mut self, before: &crate::settings::Settings, ctx: &egui::Context) {
        let s = self.saved.settings.clone();
        crate::i18n::set_language(s.language);
        if s.high_contrast != before.high_contrast {
            crate::theme::apply(ctx, self.saved.light_mode, s.high_contrast);
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
            self.toast("Removed from Liked Songs", ctx);
        } else {
            self.liked_ids.insert(t.id.clone());
            self.saved.liked.insert(0, t.clone());
            self.toast("Saved to Liked Songs", ctx);
        }
    }

    pub fn create_playlist(&mut self, name: String, first: Option<Track>, ctx: &egui::Context) {
        let name = if name.trim().is_empty() {
            format!("My Playlist #{}", self.saved.playlists.len() + 1)
        } else {
            name.trim().to_string()
        };
        self.saved.playlists.push(LocalPlaylist { name: name.clone(), tracks: first.into_iter().collect() });
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
    fn prefetch_next(&mut self) {
        let Some(next) = self.queue.get(self.queue_pos + 1).cloned() else { return };
        if self.prefetching.as_deref() == Some(next.id.as_str()) {
            return;
        }
        self.prefetching = Some(next.id.clone());
        if self.prefetched.lock().unwrap().contains_key(&next.id)
            || crate::local::is_local(&next.id)
            || self.saved.downloads.contains_key(&next.id)
        {
            return;
        }
        let ids = self.youtube_ids.clone();
        let prefetched = self.prefetched.clone();
        let quality = self.saved.settings.stream_quality;
        std::thread::spawn(move || {
            let known = ids.lock().unwrap().get(&next.id).cloned();
            if let Ok((src, video)) = crate::youtube::resolve(&next, known, quality, &|_| {}) {
                ids.lock().unwrap().insert(next.id.clone(), video);
                let mut p = prefetched.lock().unwrap();
                p.retain(|_, (_, at)| at.elapsed() < LINK_LIFETIME);
                p.insert(next.id.clone(), (src, Instant::now()));
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
                            p.tracks.push(t);
                            let msg = format!("Added to “{}”", p.name);
                            self.toast(&msg, ctx);
                        }
                    }
                }
                TrackEvent::NewPlaylistWith(t) => self.new_playlist = Some((String::new(), Some(t))),
                TrackEvent::RemoveFromPlaylist(i) => {
                    if let Route::Local(pi) = self.route
                        && let Some(p) = self.saved.playlists.get_mut(pi)
                        && i < p.tracks.len()
                    {
                        p.tracks.remove(i);
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
                TrackEvent::OpenAlbum(id) => self.navigate(Route::Album(id), ctx),
                TrackEvent::ShowCredits(t) => self.show_credits(t, ctx),
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
        self.tick(&ctx);
        crate::ui::draw(self, ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        App::save(self, storage);
    }

    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(15)
    }
}

#[cfg(test)]
mod share_tests {
    use super::*;

    #[test]
    fn share_links_point_to_the_right_place() {
        let t = Track { id: "dz:123".into(), title: "Song A".into(), ..Default::default() };
        assert_eq!(share_link(&t, ShareTarget::YouTube, Some("abcdefghijk")), "https://www.youtube.com/watch?v=abcdefghijk");
        assert!(share_link(&t, ShareTarget::YouTube, None).starts_with("https://www.youtube.com/results?search_query="));
        assert!(share_link(&t, ShareTarget::Spotify, None).contains("Song%20A"));
        assert_eq!(share_link(&t, ShareTarget::Deezer, None), "https://www.deezer.com/track/123");
    }
}
