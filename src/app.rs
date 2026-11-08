//! App state and logic: navigation, data loading, the play queue, likes and playlists.
//! The drawing code lives in `ui.rs`.

use crate::api::{self, Playlist, TimeRange, Track, User};
use crate::audio::{AudioEngine, LoadState};
use crate::widgets::TrackEvent;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Home,
    Trending,
    Search(String),
    Liked,
    Recent,
    Queue,
    Playlist(String),
    Artist(String),
    Local(usize),
}

/// Data downloaded for a page.
pub enum Page {
    Home { top: Vec<Track>, underground: Vec<Track>, playlists: Vec<Playlist> },
    Tracks(Vec<Track>),
    Search { tracks: Vec<Track>, users: Vec<User>, playlists: Vec<Playlist> },
    Playlist { info: Playlist, tracks: Vec<Track> },
    Artist { user: User, tracks: Vec<Track> },
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
        }
    }
}

const STORAGE_KEY: &str = "pulse_state_v1";

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

    // trending filters
    pub trending_time: TimeRange,
    pub trending_genre: Option<String>,

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

    // overlays
    pub toast: Option<(String, f64)>,
    pub new_playlist: Option<(String, Option<Track>)>,
    pub rename_playlist: Option<(usize, String)>,
    /// Playlist waiting for "are you sure?" before it is deleted.
    pub confirm_delete: Option<usize>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::install(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let saved: Saved = cc.storage.and_then(|s| eframe::get_value(s, STORAGE_KEY)).unwrap_or_default();
        crate::theme::set_light(&cc.egui_ctx, saved.light_mode);
        let (tx, rx) = channel();
        let audio = AudioEngine::new();
        audio.set_volume(saved.volume);

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
            trending_time: TimeRange::Week,
            trending_genre: None,
            queue_pos: 0,
            unshuffled: None,
            seek_preview: None,
            muted_volume: None,
            failures_in_row: 0,
            last_title: String::new(),
            toast: None,
            new_playlist: None,
            rename_playlist: None,
            confirm_delete: None,
        };
        if let Some(err) = app.audio.device_error.clone() {
            app.toast(&err, &cc.egui_ctx);
        }
        app.ensure_loaded(&cc.egui_ctx);
        app
    }

    pub fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.saved.last_track = self.current.clone();
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
            Route::Trending => format!(
                "trending:{}:{:?}",
                self.trending_genre.as_deref().unwrap_or("all"),
                self.trending_time
            ),
            Route::Search(q) => format!("search:{q}"),
            Route::Playlist(id) => format!("playlist:{id}"),
            Route::Artist(id) => format!("artist:{id}"),
            Route::Liked | Route::Recent | Route::Queue | Route::Local(_) => return None,
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
            Route::Home => Box::new(|| {
                let top = api::trending_tracks(None, TimeRange::Week, 20)?;
                let playlists = api::trending_playlists(12).unwrap_or_default();
                let underground = api::underground_tracks(10).unwrap_or_default();
                Ok(Page::Home { top, underground, playlists })
            }),
            Route::Trending => {
                let (genre, time) = (self.trending_genre.clone(), self.trending_time);
                Box::new(move || api::trending_tracks(genre.as_deref(), time, 100).map(Page::Tracks))
            }
            Route::Search(q) => Box::new(move || {
                let tracks = api::search_tracks(&q)?;
                let users = api::search_users(&q).unwrap_or_default();
                let playlists = api::search_playlists(&q).unwrap_or_default();
                Ok(Page::Search { tracks, users, playlists })
            }),
            Route::Playlist(id) => Box::new(move || {
                let info = api::playlist(&id)?;
                let tracks = api::playlist_tracks(&id)?;
                Ok(Page::Playlist { info, tracks })
            }),
            Route::Artist(id) => Box::new(move || {
                let user = api::user(&id)?;
                let tracks = api::user_tracks(&id)?;
                Ok(Page::Artist { user, tracks })
            }),
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
                }
            }
            _ => {}
        }

        if self.is_playing() || self.audio.load_state() == LoadState::Loading {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        // Window title shows the current song.
        let title = match &self.current {
            Some(t) if self.is_playing() => format!("{} · {} — Pulse", t.title, t.user.name),
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
        self.audio.load(track.stream_url(), false, ctx.clone());
        self.audio.set_volume(self.saved.volume);
        self.needs_load = false;
        self.seek_preview = None;
        // Recently played: newest first, no duplicates, max 50.
        self.saved.recent.retain(|t| t.id != track.id);
        self.saved.recent.insert(0, track.clone());
        self.saved.recent.truncate(50);
        self.current = Some(track);
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
        match self.saved.repeat {
            Repeat::One => self.start_current(ctx),
            _ if self.queue_pos + 1 < self.queue.len() || self.saved.repeat == Repeat::All => self.next(ctx),
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
        crate::theme::set_light(ctx, self.saved.light_mode);
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

    /// Applies what the user did in a track list. `list` is the list that was shown.
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
                TrackEvent::OpenArtist(id) => self.navigate(Route::Artist(id), ctx),
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
