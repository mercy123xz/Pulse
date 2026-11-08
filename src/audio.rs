//! The audio engine.
//!
//! Playing a song has two steps, both on a background thread:
//! 1. *Resolve*: work out where the audio is (for Pulse: find the song on YouTube).
//! 2. *Stream*: download it in chunks into a shared buffer while the decoder plays
//!    from that buffer, waiting briefly whenever it catches up with the download.

use rodio::{Decoder, MixerDeviceSink, Player};
use std::io::{self, Read, Seek, SeekFrom};
use crate::dsp::{Processor, SharedEffects, TrackCtl};
use rodio::Source;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub use crate::dsp::Effects;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Bytes per download request. YouTube slows down single huge requests, so audio
/// is fetched in pieces this size, one after another.
const CHUNK: u64 = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Where a song's audio is
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Mp3,
    /// AAC audio in an MP4 / M4A container (what YouTube serves).
    Mp4,
    Flac,
    Wav,
    Ogg,
}

impl Codec {
    /// The codec for a music file, by its extension.
    pub fn from_extension(ext: &str) -> Option<Codec> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "mp3" => Codec::Mp3,
            "m4a" | "mp4" | "aac" | "alac" => Codec::Mp4,
            "flac" => Codec::Flac,
            "wav" | "wave" => Codec::Wav,
            "ogg" | "oga" => Codec::Ogg,
            _ => return None,
        })
    }

    fn hint(self) -> (&'static str, &'static str) {
        match self {
            Codec::Mp3 => ("mp3", "audio/mpeg"),
            Codec::Mp4 => ("m4a", "audio/mp4"),
            Codec::Flac => ("flac", "audio/flac"),
            Codec::Wav => ("wav", "audio/wav"),
            Codec::Ogg => ("ogg", "audio/ogg"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamSource {
    /// A web address, or `file://` + a path for songs on this computer.
    pub url: String,
    /// Extra HTTP headers the server expects.
    pub headers: Vec<(String, String)>,
    pub codec: Codec,
    /// File size, if known in advance.
    pub content_length: Option<u64>,
    /// Something worth telling the user once playback starts.
    pub notice: Option<String>,
}

/// Works out where the audio is. Gets a callback for showing progress
/// ("Finding song on YouTube…").
pub type Resolver = Box<dyn FnOnce(&dyn Fn(&str)) -> Result<StreamSource, String> + Send>;

// ---------------------------------------------------------------------------
// A download buffer that can be read (and seeked) while it is still filling.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct BufState {
    bytes: Vec<u8>,
    total: Option<u64>,
    done: bool,
    cancelled: bool,
}

#[derive(Default)]
struct StreamBuffer {
    state: Mutex<BufState>,
    changed: Condvar,
}

/// The decoder's view of the buffer: a normal `Read + Seek` "file".
pub struct StreamReader {
    buf: Arc<StreamBuffer>,
    pos: u64,
}

impl Read for StreamReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut st = self.buf.state.lock().unwrap();
        // Wait until the download has reached our read position (or finished).
        while (self.pos as usize) >= st.bytes.len() && !st.done && !st.cancelled {
            st = self.buf.changed.wait(st).unwrap();
        }
        let start = self.pos as usize;
        if start >= st.bytes.len() {
            return Ok(0); // end of song
        }
        let n = out.len().min(st.bytes.len() - start);
        out[..n].copy_from_slice(&st.bytes[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let new_pos = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => {
                let mut st = self.buf.state.lock().unwrap();
                let len = loop {
                    if let Some(t) = st.total {
                        break t;
                    }
                    if st.done || st.cancelled {
                        break st.bytes.len() as u64;
                    }
                    st = self.buf.changed.wait(st).unwrap();
                };
                len as i64 + d
            }
        };
        if new_pos < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = new_pos as u64;
        Ok(self.pos)
    }
}

impl Drop for StreamReader {
    fn drop(&mut self) {
        // Song was skipped or finished: tell the download thread to stop.
        self.buf.state.lock().unwrap().cancelled = true;
        self.buf.changed.notify_all();
    }
}

/// Opens the audio and returns a decoder that can start playing right away while
/// the rest of the file keeps downloading in the background.
/// `still_wanted` returns false once the user has moved on to another song.
fn open_streaming_decoder(
    src: &StreamSource,
    still_wanted: impl Fn() -> bool + Send + 'static,
    buffered: Arc<Mutex<f32>>,
) -> Result<Decoder<StreamReader>, String> {
    if let Some(path) = src.url.strip_prefix("file://") {
        // A song on this computer: read it all at once.
        let bytes = std::fs::read(path).map_err(|e| format!("Couldn't open {path} ({e})"))?;
        let total = bytes.len() as u64;
        let buf = Arc::new(StreamBuffer::default());
        {
            let mut st = buf.state.lock().unwrap();
            st.bytes = bytes;
            st.total = Some(total);
            st.done = true;
        }
        *buffered.lock().unwrap() = 1.0;
        return build_decoder(StreamReader { buf, pos: 0 }, src.codec, Some(total));
    }
    let first = crate::net::get_range(&src.url, &src.headers, 0, Some(CHUNK - 1))?;
    if !still_wanted() {
        return Err("cancelled".into());
    }
    let total = first.total.or(src.content_length);
    let buf = Arc::new(StreamBuffer::default());
    buf.state.lock().unwrap().total = total;

    // Download thread: keeps filling the buffer, one chunk at a time.
    {
        let buf = buf.clone();
        let url = src.url.clone();
        let headers = src.headers.clone();
        let partial = first.partial;
        let mut body = first.body;
        std::thread::spawn(move || {
            let mut chunk = vec![0u8; 64 * 1024];
            let mut retries = 0u64;
            loop {
                if buf.state.lock().unwrap().cancelled || !still_wanted() {
                    break;
                }
                match body.read(&mut chunk) {
                    Ok(n) if n > 0 => {
                        let mut st = buf.state.lock().unwrap();
                        st.bytes.extend_from_slice(&chunk[..n]);
                        if let Some(t) = st.total {
                            *buffered.lock().unwrap() = st.bytes.len() as f32 / t.max(1) as f32;
                        }
                        drop(st);
                        buf.changed.notify_all();
                    }
                    result => {
                        // This request ended (finished or the connection dropped).
                        let have = buf.state.lock().unwrap().bytes.len() as u64;
                        let finished = match total {
                            Some(t) => have >= t,
                            None => result.is_ok(), // no size known: a clean end means done
                        };
                        if finished || (!partial && result.is_ok()) {
                            break;
                        }
                        if result.is_err() {
                            retries += 1;
                            if retries > 4 {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(400 * retries));
                        }
                        let end = total.map(|t| (have + CHUNK).min(t) - 1);
                        match crate::net::get_range(&url, &headers, have, end) {
                            Ok(next) => body = next.body,
                            Err(_) => {
                                retries += 1;
                                if retries > 4 {
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(400 * retries));
                            }
                        }
                    }
                }
            }
            let mut st = buf.state.lock().unwrap();
            st.done = true;
            if !st.cancelled {
                st.total = Some(st.bytes.len() as u64);
            }
            *buffered.lock().unwrap() = 1.0;
            drop(st);
            buf.changed.notify_all();
        });
    }

    build_decoder(StreamReader { buf, pos: 0 }, src.codec, total)
}

fn build_decoder(reader: StreamReader, codec: Codec, total: Option<u64>) -> Result<Decoder<StreamReader>, String> {
    let (hint, mime) = codec.hint();
    let mut builder = Decoder::builder()
        .with_data(reader)
        .with_hint(hint)
        .with_mime_type(mime)
        .with_seekable(true)
        .with_coarse_seek(total.is_some() && codec == Codec::Mp3);
    if let Some(t) = total {
        builder = builder.with_byte_len(t);
    }
    builder.build().map_err(|e| format!("Couldn't decode this song ({e})"))
}

// ---------------------------------------------------------------------------
// The engine the UI talks to.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum LoadState {
    Idle,
    /// Working out where the audio is; the text says what's happening.
    Resolving(String),
    /// Audio found, starting to download it.
    Loading,
    Ready,
    Failed(String),
}

impl LoadState {
    pub fn is_busy(&self) -> bool {
        matches!(self, LoadState::Resolving(_) | LoadState::Loading)
    }
}

type Dec = Decoder<StreamReader>;

/// The song on a deck.
struct Playing {
    id: String,
    deck: usize,
    ctl: Arc<TrackCtl>,
    duration: Duration,
}

/// The next song, opened ahead of time so it can start instantly.
struct Prepared {
    id: String,
    decoder: Dec,
    duration: Duration,
    buffered: Arc<Mutex<f32>>,
}

/// What the app says will play next.
struct NextRequest {
    id: String,
    duration: Duration,
    resolver: Option<Resolver>,
}

/// State shared by the engine, its loading threads and the monitor thread.
struct Shared {
    /// Two players ("decks") so one song can fade out while the next fades in.
    decks: Vec<Arc<Player>>,
    playing: Option<Playing>,
    /// Gapless: the next song, already queued behind the current one on the same deck.
    queued: Option<Playing>,
    /// The previous song while it fades out.
    fading_out: Option<(usize, Arc<TrackCtl>)>,
    next: Option<NextRequest>,
    prepared: Option<Prepared>,
    /// Set when the engine moved on to the next song by itself.
    advanced: Option<String>,
    paused: bool,
    volume: f32,
    buffered: Arc<Mutex<f32>>,
    ctx: Option<egui::Context>,
}

pub struct AudioEngine {
    // The output device must stay alive for as long as we want sound.
    _device: Option<MixerDeviceSink>,
    shared: Arc<Mutex<Shared>>,
    effects: Arc<SharedEffects>,
    /// Bumped every time a new song is requested, so a slow download for an
    /// old song can never start playing over a newer one.
    generation: Arc<AtomicU64>,
    /// Same, for opening the next song ahead of time.
    next_generation: Arc<AtomicU64>,
    load_state: Arc<Mutex<(u64, LoadState)>>,
    notice: Arc<Mutex<Option<String>>>,
    alive: Arc<AtomicBool>,
    pub device_error: Option<String>,
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

/// Start opening the next song when the current one has this long left.
const PREPARE_AHEAD: Duration = Duration::from_secs(45);
/// Gapless: queue the next song this long before the current one ends.
const QUEUE_AHEAD: Duration = Duration::from_secs(4);

impl AudioEngine {
    pub fn new() -> Self {
        let (device, decks, device_error) = match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut device) => {
                device.log_on_drop(false);
                let decks = vec![Arc::new(Player::connect_new(device.mixer())), Arc::new(Player::connect_new(device.mixer()))];
                (Some(device), decks, None)
            }
            Err(e) => (None, vec![], Some(format!("No audio output device found ({e})"))),
        };
        let engine = Self {
            _device: device,
            shared: Arc::new(Mutex::new(Shared {
                decks,
                playing: None,
                queued: None,
                fading_out: None,
                next: None,
                prepared: None,
                advanced: None,
                paused: false,
                volume: 1.0,
                buffered: Arc::new(Mutex::new(0.0)),
                ctx: None,
            })),
            effects: Arc::new(SharedEffects::default()),
            generation: Arc::new(AtomicU64::new(0)),
            next_generation: Arc::new(AtomicU64::new(0)),
            load_state: Arc::new(Mutex::new((0, LoadState::Idle))),
            notice: Arc::new(Mutex::new(None)),
            alive: Arc::new(AtomicBool::new(true)),
            device_error,
        };
        engine.spawn_monitor();
        engine
    }

    fn has_output(&self) -> bool {
        !self.shared.lock().unwrap().decks.is_empty()
    }

    /// Changes EQ, mono, crossfade, gapless and automix. Takes effect right away.
    pub fn set_effects(&self, e: Effects) {
        self.effects.set(e);
    }

    /// Starts a song. Returns immediately; finding and streaming the audio happen
    /// on a background thread. `ctx` is used to wake up the UI.
    pub fn load(&self, id: &str, expected: Duration, resolve: Resolver, start_paused: bool, ctx: egui::Context) {
        if !self.has_output() {
            return;
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let prepared = {
            let mut sh = self.shared.lock().unwrap();
            for d in &sh.decks {
                d.clear();
            }
            sh.playing = None;
            sh.queued = None;
            sh.fading_out = None;
            sh.advanced = None;
            sh.paused = start_paused;
            sh.ctx = Some(ctx.clone());
            // Already opened ahead of time (pressing Next, or the song ended)? Use it.
            let ready = sh.prepared.take_if(|p| p.id == id);
            if ready.is_none() {
                sh.prepared = None;
            }
            if sh.next.as_ref().is_some_and(|n| n.id == id) {
                sh.next = None;
            }
            ready
        };
        *self.load_state.lock().unwrap() = (generation, LoadState::Resolving("Getting ready…".into()));

        let gen_counter = self.generation.clone();
        let load_state = self.load_state.clone();
        let notice = self.notice.clone();
        let shared = self.shared.clone();
        let effects = self.effects.clone();
        let id = id.to_string();

        std::thread::spawn(move || {
            let set_state = |s: LoadState| {
                let mut ls = load_state.lock().unwrap();
                if ls.0 == generation {
                    ls.1 = s;
                }
                ctx.request_repaint();
            };
            let current = || gen_counter.load(Ordering::SeqCst) == generation;
            let (decoder, buffered) = match prepared {
                Some(p) => (p.decoder, p.buffered),
                None => {
                    let status = |text: &str| {
                        if current() {
                            set_state(LoadState::Resolving(text.to_owned()));
                        }
                    };
                    let src = match resolve(&status) {
                        Ok(s) => s,
                        Err(e) => return set_state(LoadState::Failed(e)),
                    };
                    if !current() {
                        return; // the user already picked another song
                    }
                    set_state(LoadState::Loading);
                    let still_wanted = {
                        let gen_counter = gen_counter.clone();
                        move || gen_counter.load(Ordering::SeqCst) == generation
                    };
                    let buffered = Arc::new(Mutex::new(0.0));
                    let decoder = match open_streaming_decoder(&src, still_wanted, buffered.clone()) {
                        Ok(d) => d,
                        Err(e) => return set_state(LoadState::Failed(e)),
                    };
                    if let Some(n) = src.notice {
                        *notice.lock().unwrap() = Some(n);
                    }
                    (decoder, buffered)
                }
            };
            if !current() {
                return;
            }
            let duration = decoder.total_duration().filter(|d| !d.is_zero()).unwrap_or(expected);
            let ctl = Arc::new(TrackCtl::default());
            let source = Processor::new(decoder, effects, ctl.clone(), 1.0, false);
            {
                let mut sh = shared.lock().unwrap();
                let deck = 0;
                sh.decks[deck].clear();
                sh.decks[deck].append(source);
                if sh.paused {
                    sh.decks[deck].pause();
                } else {
                    sh.decks[deck].play();
                }
                sh.buffered = buffered;
                sh.playing = Some(Playing { id, deck, ctl, duration });
            }
            set_state(LoadState::Ready);
        });
    }

    /// Tells the engine which song comes next, so it can open it early and
    /// play it gaplessly or crossfade into it. `None` when nothing comes next.
    pub fn set_next(&self, next: Option<(String, Duration, Resolver)>) {
        let mut sh = self.shared.lock().unwrap();
        let same = match (&next, &sh.next) {
            (Some((id, _, _)), Some(n)) => *id == n.id,
            (None, None) => true,
            _ => false,
        };
        let same_prepared = match (&next, &sh.prepared) {
            (Some((id, _, _)), Some(p)) => *id == p.id,
            _ => false,
        };
        if same || same_prepared {
            return;
        }
        self.next_generation.fetch_add(1, Ordering::SeqCst);
        sh.prepared = None;
        sh.next = next.map(|(id, duration, resolver)| NextRequest { id, duration, resolver: Some(resolver) });
    }

    /// The id of the song the engine moved on to by itself (gapless, crossfade,
    /// automix), once. The app then updates its queue.
    pub fn take_advanced(&self) -> Option<String> {
        self.shared.lock().unwrap().advanced.take()
    }

    /// Background thread that handles moving from one song to the next.
    fn spawn_monitor(&self) {
        if !self.has_output() {
            return;
        }
        let shared = self.shared.clone();
        let effects = self.effects.clone();
        let alive = self.alive.clone();
        let next_generation = self.next_generation.clone();
        std::thread::spawn(move || {
            while alive.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(40));
                let fx = effects.get();
                let mut guard = shared.lock().unwrap();
                let sh = &mut *guard;
                monitor_step(sh, &fx, &effects, &shared, &next_generation);
            }
        });
    }

    pub fn load_state(&self) -> LoadState {
        let ls = self.load_state.lock().unwrap();
        if ls.0 == self.generation.load(Ordering::SeqCst) { ls.1.clone() } else { LoadState::Loading }
    }

    /// A message from the last song start, shown once.
    pub fn take_notice(&self) -> Option<String> {
        self.notice.lock().unwrap().take()
    }

    pub fn buffered(&self) -> f32 {
        let b = self.shared.lock().unwrap().buffered.clone();
        *b.lock().unwrap()
    }

    pub fn play(&self) {
        let mut sh = self.shared.lock().unwrap();
        sh.paused = false;
        for d in &sh.decks {
            d.play();
        }
    }

    pub fn pause(&self) {
        let mut sh = self.shared.lock().unwrap();
        sh.paused = true;
        for d in &sh.decks {
            d.pause();
        }
    }

    pub fn is_paused(&self) -> bool {
        let sh = self.shared.lock().unwrap();
        sh.decks.is_empty() || sh.paused
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.next_generation.fetch_add(1, Ordering::SeqCst);
        let mut sh = self.shared.lock().unwrap();
        for d in &sh.decks {
            d.clear();
        }
        sh.playing = None;
        sh.queued = None;
        sh.fading_out = None;
        sh.prepared = None;
        sh.next = None;
        *self.load_state.lock().unwrap() = (self.generation.load(Ordering::SeqCst), LoadState::Idle);
    }

    pub fn set_volume(&self, v: f32) {
        let mut sh = self.shared.lock().unwrap();
        // Perceptual curve: the slider feels linear to the ear.
        sh.volume = v * v;
        for d in &sh.decks {
            d.set_volume(v * v);
        }
    }

    pub fn position(&self) -> Duration {
        let sh = self.shared.lock().unwrap();
        match &sh.playing {
            Some(p) => sh.decks[p.deck].get_pos(),
            None => Duration::ZERO,
        }
    }

    pub fn seek(&self, to: Duration) {
        let sh = self.shared.lock().unwrap();
        if let Some(p) = &sh.playing {
            let _ = sh.decks[p.deck].try_seek(to);
        }
    }

    /// True when the current song has played to the end.
    pub fn finished(&self) -> bool {
        if self.load_state() != LoadState::Ready {
            return false;
        }
        let sh = self.shared.lock().unwrap();
        sh.queued.is_none() && sh.playing.as_ref().is_some_and(|p| sh.decks[p.deck].empty())
    }
}

/// One check of the monitor thread: finish fades, notice gapless switches,
/// open the next song early, and start transitions.
fn monitor_step(sh: &mut Shared, fx: &Effects, effects: &Arc<SharedEffects>, shared: &Arc<Mutex<Shared>>, next_generation: &Arc<AtomicU64>) {
    // The old song finished fading out: stop it.
    if let Some((deck, ctl)) = &sh.fading_out
        && (ctl.fade_finished() || sh.decks[*deck].empty())
    {
        sh.decks[*deck].clear();
        sh.fading_out = None;
    }

    // Gapless: the queued song has started once the deck holds only one song again.
    if let Some(q) = &sh.queued
        && sh.decks[q.deck].len() <= 1
    {
        let q = sh.queued.take().unwrap();
        sh.advanced = Some(q.id.clone());
        sh.playing = Some(q);
        if let Some(ctx) = &sh.ctx {
            ctx.request_repaint();
        }
    }

    let Some(p) = &sh.playing else { return };
    let pos = sh.decks[p.deck].get_pos();
    let remaining = p.duration.saturating_sub(pos);
    let (cur_deck, cur_ctl) = (p.deck, p.ctl.clone());

    // Open the next song ahead of time.
    if remaining < PREPARE_AHEAD
        && let Some(n) = &mut sh.next
        && let Some(resolver) = n.resolver.take()
    {
        let (id, duration) = (n.id.clone(), n.duration);
        let generation = next_generation.load(Ordering::SeqCst);
        let shared = shared.clone();
        let next_generation = next_generation.clone();
        std::thread::spawn(move || {
            let still = {
                let next_generation = next_generation.clone();
                move || next_generation.load(Ordering::SeqCst) == generation
            };
            let Ok(src) = resolver(&|_| {}) else { return };
            if !still() {
                return;
            }
            let buffered = Arc::new(Mutex::new(0.0));
            let Ok(decoder) = open_streaming_decoder(&src, still.clone(), buffered.clone()) else { return };
            let mut sh = shared.lock().unwrap();
            if still() && sh.next.as_ref().is_some_and(|n| n.id == id) {
                let duration = decoder.total_duration().filter(|d| !d.is_zero()).unwrap_or(duration);
                sh.prepared = Some(Prepared { id, decoder, duration, buffered });
            }
        });
    }

    let ready = sh.queued.is_none()
        && sh.fading_out.is_none()
        && match (&sh.prepared, &sh.next) {
            (Some(pr), Some(n)) => pr.id == n.id,
            _ => false,
        };
    if !ready || sh.paused {
        return;
    }
    let xf = fx.transition_secs();
    if xf > 0.0 {
        // Automix: a song that has gone quiet near its end can hand over early.
        let silent_end = fx.automix && pos > Duration::from_secs(30) && remaining < Duration::from_secs(25) && cur_ctl.silent_for() > Duration::from_millis(1500);
        if remaining <= Duration::from_secs_f32(xf) || silent_end {
            let over = Duration::from_secs_f32(if silent_end { xf.min(3.0) } else { xf }).min(remaining.max(Duration::from_millis(300)));
            let pr = sh.prepared.take().unwrap();
            sh.next = None;
            let new_deck = 1 - cur_deck;
            let ctl = Arc::new(TrackCtl::default());
            let source = Processor::new(pr.decoder, effects.clone(), ctl.clone(), 0.0, fx.automix);
            ctl.fade(0.0, 1.0, over);
            sh.decks[new_deck].clear();
            sh.decks[new_deck].set_volume(sh.volume);
            sh.decks[new_deck].append(source);
            sh.decks[new_deck].play();
            cur_ctl.fade(1.0, 0.0, over);
            sh.fading_out = Some((cur_deck, cur_ctl));
            sh.buffered = pr.buffered;
            sh.advanced = Some(pr.id.clone());
            sh.playing = Some(Playing { id: pr.id, deck: new_deck, ctl, duration: pr.duration });
            if let Some(ctx) = &sh.ctx {
                ctx.request_repaint();
            }
        }
    } else if fx.gapless && remaining <= QUEUE_AHEAD {
        // Queue it right behind the current song: no gap at all.
        let pr = sh.prepared.take().unwrap();
        sh.next = None;
        let ctl = Arc::new(TrackCtl::default());
        sh.decks[cur_deck].append(Processor::new(pr.decoder, effects.clone(), ctl.clone(), 1.0, false));
        sh.buffered = pr.buffered;
        sh.queued = Some(Playing { id: pr.id, deck: cur_deck, ctl, duration: pr.duration });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::Source;
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::Instant;

    /// Serves a file over HTTP, deliberately slowly, to imitate a real network.
    /// With `ranges`, it answers Range requests with 206 like YouTube's servers do.
    fn slow_server(data: Vec<u8>, bytes_per_sec: usize, ranges: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut s = stream.unwrap();
                let data = data.clone();
                std::thread::spawn(move || {
                    let mut req = [0u8; 4096];
                    let n = std::io::Read::read(&mut s, &mut req).unwrap_or(0);
                    let req = String::from_utf8_lossy(&req[..n]).to_lowercase();
                    let range = req
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .map(|r| {
                            let (a, b) = r.trim().split_once('-').unwrap();
                            let a: usize = a.parse().unwrap();
                            let b: usize = b.parse().unwrap_or(data.len() - 1).min(data.len() - 1);
                            (a, b)
                        });
                    let (head, body) = match range.filter(|_| ranges) {
                        Some((a, b)) => (
                            format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Type: audio/mp4\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{}\r\nConnection: close\r\n\r\n",
                                b - a + 1,
                                data.len()
                            ),
                            data[a..=b].to_vec(),
                        ),
                        None => (
                            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len()),
                            data.clone(),
                        ),
                    };
                    s.write_all(head.as_bytes()).unwrap();
                    for part in body.chunks((bytes_per_sec / 20).max(1)) {
                        if s.write_all(part).is_err() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                });
            }
        });
        format!("http://{addr}/audio")
    }

    fn source(url: String, codec: Codec) -> StreamSource {
        StreamSource { url, headers: vec![], codec, content_length: None, notice: None }
    }

    fn check_stream_and_seek(file: &str, codec: Codec, ranges: bool) {
        let Ok(data) = std::fs::read(file) else {
            eprintln!("skipping: {file} not found");
            return;
        };
        // ~250 KB/s, so a ~0.5–1.2 MB test file takes a few seconds to download.
        let url = slow_server(data, 250_000, ranges);
        let buffered = Arc::new(Mutex::new(0.0));
        let t0 = Instant::now();
        let mut dec = open_streaming_decoder(&source(url, codec), || true, buffered.clone()).expect("decoder");
        let ready_after = t0.elapsed();
        let first: Vec<_> = (&mut dec).take(44_100 * 2).collect(); // 1 s of stereo audio
        assert_eq!(first.len(), 44_100 * 2, "decoded the first second");
        assert!(first.iter().any(|s| s.abs() > 0.05), "real audio, not silence");
        println!("{file}: ready after {ready_after:?}, buffered {:.0}%", *buffered.lock().unwrap() * 100.0);
        if codec == Codec::Mp3 {
            // MP3 starts while downloading. Symphonia reads an M4A's whole fragment index
            // first, so M4A starts once the file is in (a second or two for a song).
            assert!(*buffered.lock().unwrap() < 1.0, "playback starts before the download finishes");
        }

        // Jump to 20 s of 30 s: waits for that part to download, then carries on.
        dec.try_seek(Duration::from_secs(20)).expect("seek");
        let rest = dec.count();
        println!("{file}: samples after seeking to 20 s: {rest} (~{:.1} s)", rest as f32 / 88_200.0);
        assert!(rest > 88_200 * 8 && rest < 88_200 * 11, "about 10 s should remain");
        assert!((*buffered.lock().unwrap() - 1.0).abs() < 1e-6, "download completed");
    }

    #[test]
    fn mp3_streams_and_seeks() {
        check_stream_and_seek("/tmp/test.mp3", Codec::Mp3, false);
    }

    #[test]
    fn mp3_streams_in_chunks() {
        check_stream_and_seek("/tmp/test.mp3", Codec::Mp3, true);
    }

    /// The layout YouTube uses: fragmented M4A (AAC), downloaded in ranged chunks.
    #[test]
    fn youtube_style_m4a_streams_and_seeks() {
        check_stream_and_seek("/tmp/test_dash.m4a", Codec::Mp4, true);
    }

    #[test]
    fn plain_m4a_streams_and_seeks() {
        check_stream_and_seek("/tmp/test_plain.m4a", Codec::Mp4, true);
    }
}
