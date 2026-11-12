//! The audio engine.
//!
//! Playing a song has two steps, both on a background thread:
//! 1. *Resolve*: work out where the audio is (for Pulse: find the song on YouTube).
//! 2. *Stream*: download it in chunks into a shared buffer while the decoder plays
//!    from that buffer, waiting briefly whenever it catches up with the download.

use crate::dsp::{Processor, SharedEffects, TrackCtl};
use rodio::Source;
use rodio::{Decoder, MixerDeviceSink, Player};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub use crate::dsp::Effects;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Bytes per download request when downloading one piece after another.
/// YouTube slows down single huge requests, so audio is never fetched in one go.
const CHUNK: u64 = 4 * 1024 * 1024;
/// The first piece of a song (enough to start an MP3 right away).
const FIRST_CHUNK: u64 = 1024 * 1024;
/// Size of the pieces fetched side by side after the first one.
const PIECE: u64 = 1024 * 1024;
/// How many pieces download at the same time (kept small: light on the PC and the network).
const PARALLEL: usize = 4;

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
    /// The song's length, if whoever found the audio knows it (a song that starts while
    /// it downloads can't always tell from the file).
    pub duration: Option<Duration>,
    /// Keep a copy here once all of it has downloaded, so it starts at once next time.
    pub keep_as: Option<std::path::PathBuf>,
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
    /// How many readers are open. The download stops when the last one goes.
    readers: std::sync::atomic::AtomicUsize,
}

/// The decoder's view of the buffer: a normal `Read + Seek` "file".
pub struct StreamReader {
    buf: Arc<StreamBuffer>,
    pos: u64,
}

impl StreamReader {
    fn new(buf: &Arc<StreamBuffer>) -> Self {
        buf.readers.fetch_add(1, Ordering::SeqCst);
        StreamReader { buf: buf.clone(), pos: 0 }
    }
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
        if self.buf.readers.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.buf.state.lock().unwrap().cancelled = true;
            self.buf.changed.notify_all();
        }
    }
}

/// Opens the audio and returns a decoder that can start playing right away while
/// the rest of the file keeps downloading in the background.
/// `still_wanted` returns false once the user has moved on to another song.
fn open_streaming_decoder(src: &StreamSource, still_wanted: impl Fn() -> bool + Send + 'static, buffered: Arc<Mutex<f32>>) -> Result<StreamDecoder, String> {
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
        let inner = build_decoder(StreamReader::new(&buf), src.codec, Some(total), true)?;
        return Ok(StreamDecoder::new(inner, None, src.duration));
    }
    let first = crate::net::get_range(&src.url, &src.headers, 0, Some(FIRST_CHUNK - 1))?;
    if !still_wanted() {
        return Err("cancelled".into());
    }
    let total = first.total.or(src.content_length);
    let buf = Arc::new(StreamBuffer::default());
    buf.state.lock().unwrap().total = total;

    // Download thread: keeps filling the buffer. When the server sends pieces of the file
    // (ranges) and its size is known, the rest is fetched as several pieces at once.
    {
        let buf = buf.clone();
        let url = src.url.clone();
        let headers = src.headers.clone();
        let partial = first.partial;
        let body = first.body;
        let keep_as = src.keep_as.clone();
        std::thread::spawn(move || {
            let parallel_from = match total {
                Some(t) if partial && t > FIRST_CHUNK => Some(FIRST_CHUNK),
                _ => None,
            };
            // Start the other pieces right away, while the first one is still arriving.
            let pieces = parallel_from.map(|from| ParallelPieces::start(&url, &headers, from, total.unwrap()));
            let still = || !buf.state.lock().unwrap().cancelled && still_wanted();
            sequential_download(&buf, &url, &headers, body, partial, if pieces.is_some() { Some(FIRST_CHUNK) } else { total }, &buffered, &still);
            if let Some(mut p) = pieces {
                p.finish(&buf, &buffered, total.unwrap(), &still);
            }
            let mut st = buf.state.lock().unwrap();
            st.done = true;
            // All of it arrived (not skipped, not cut short): keep a copy for next time.
            let whole = !st.cancelled && !st.bytes.is_empty() && total.is_none_or(|t| st.bytes.len() as u64 == t);
            let copy = keep_as.filter(|_| whole).map(|p| (p, st.bytes.clone()));
            if !st.cancelled {
                st.total = Some(st.bytes.len() as u64);
            }
            *buffered.lock().unwrap() = 1.0;
            drop(st);
            buf.changed.notify_all();
            if let Some((path, bytes)) = copy {
                crate::audiocache::store(&path, &bytes);
            }
        });
    }

    // An MP4 is opened "forward only": it starts as soon as its first part is here. (Opened
    // the usual way, the decoder first walks through the whole file, so nothing played until
    // every byte had arrived.) Going back in the song opens it again, see `StreamDecoder`.
    let keep = StreamReader::new(&buf); // keeps the download alive if the first try fails
    let forward = src.codec == Codec::Mp4;
    let inner = match build_decoder(StreamReader::new(&buf), src.codec, total, !forward) {
        Ok(d) => d,
        // A file with its index at the end can't start early: wait for all of it.
        Err(_) if forward => return Ok(StreamDecoder::new(build_decoder(StreamReader::new(&buf), src.codec, total, true)?, None, src.duration)),
        Err(e) => return Err(e),
    };
    drop(keep);
    Ok(StreamDecoder::new(inner, forward.then_some(Reopen { buf, codec: src.codec, total }), src.duration))
}

/// What's needed to open a song again from its start.
struct Reopen {
    buf: Arc<StreamBuffer>,
    codec: Codec,
    total: Option<u64>,
}

/// A song's decoder. For audio that plays while it downloads, going back in the song opens
/// the (already downloaded) audio again from the start and moves forward from there.
pub struct StreamDecoder {
    inner: Decoder<StreamReader>,
    reopen: Option<Reopen>,
    /// Where the last jump landed, and the samples played since.
    base: Duration,
    samples: u64,
    /// The length to report when the file itself doesn't say.
    known_duration: Option<Duration>,
}

impl StreamDecoder {
    fn new(inner: Decoder<StreamReader>, reopen: Option<Reopen>, known_duration: Option<Duration>) -> Self {
        StreamDecoder { inner, reopen, base: Duration::ZERO, samples: 0, known_duration }
    }

    fn played(&self) -> Duration {
        let per_sec = self.inner.sample_rate().get() as u64 * self.inner.channels().get() as u64;
        self.base + Duration::from_secs_f64(self.samples as f64 / per_sec.max(1) as f64)
    }
}

impl Iterator for StreamDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let s = self.inner.next();
        self.samples += s.is_some() as u64;
        s
    }
}

impl Source for StreamDecoder {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration().filter(|d| !d.is_zero()).or(self.known_duration)
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        if let Some(r) = &self.reopen
            && pos < self.played()
        {
            // Once everything has arrived, open it for jumping anywhere.
            let done = r.buf.state.lock().unwrap().done;
            let fresh =
                build_decoder(StreamReader::new(&r.buf), r.codec, r.total, done).map_err(|e| rodio::source::SeekError::Other(Arc::new(io::Error::other(e))))?;
            self.inner = fresh;
            if done {
                self.reopen = None;
            }
        }
        self.inner.try_seek(pos)?;
        self.base = pos;
        self.samples = 0;
        Ok(())
    }
}

/// Reads `body` into the buffer, resuming with range requests if the connection drops,
/// until `until` bytes are there (or the end of the file when `until` is None).
#[allow(clippy::too_many_arguments)]
fn sequential_download(
    buf: &StreamBuffer,
    url: &str,
    headers: &[(String, String)],
    mut body: Box<dyn Read + Send>,
    partial: bool,
    until: Option<u64>,
    buffered: &Mutex<f32>,
    still: &dyn Fn() -> bool,
) {
    let mut chunk = vec![0u8; 64 * 1024];
    let mut retries = 0u64;
    let total = buf.state.lock().unwrap().total;
    loop {
        if !still() {
            return;
        }
        match body.read(&mut chunk) {
            Ok(n) if n > 0 => {
                let mut st = buf.state.lock().unwrap();
                st.bytes.extend_from_slice(&chunk[..n]);
                if let Some(t) = st.total {
                    *buffered.lock().unwrap() = st.bytes.len() as f32 / t.max(1) as f32;
                }
                let have = st.bytes.len() as u64;
                drop(st);
                buf.changed.notify_all();
                if until.is_some_and(|u| have >= u) {
                    return;
                }
            }
            result => {
                // This request ended (finished or the connection dropped).
                let have = buf.state.lock().unwrap().bytes.len() as u64;
                let finished = match until {
                    Some(t) => have >= t,
                    None => result.is_ok(), // no size known: a clean end means done
                };
                if finished || (!partial && result.is_ok()) {
                    return;
                }
                if result.is_err() {
                    retries += 1;
                    if !wait_to_retry(retries, still) {
                        return;
                    }
                }
                let end = until.or(total).map(|t| (have + CHUNK).min(t) - 1);
                match crate::net::get_range(url, headers, have, end) {
                    Ok(next) => {
                        body = next.body;
                        retries = 0;
                    }
                    Err(_) => {
                        retries += 1;
                        if !wait_to_retry(retries, still) {
                            return;
                        }
                    }
                }
            }
        }
    }
}

/// Waits before trying a download again after the connection dropped. Short waits at
/// first, then up to 5 seconds, for about a minute in all, so a brief Wi-Fi drop
/// doesn't cut the song short. False when it's time to give up (or the song was left).
fn wait_to_retry(tries: u64, still: &dyn Fn() -> bool) -> bool {
    if tries > 16 {
        return false;
    }
    let wait = Duration::from_millis((400 * tries).min(5000));
    // Wait in small steps so skipping the song stops the wait at once.
    let step = Duration::from_millis(100);
    let mut waited = Duration::ZERO;
    while waited < wait {
        if !still() {
            return false;
        }
        std::thread::sleep(step);
        waited += step;
    }
    still()
}

/// Pieces of a file downloading side by side, added to the buffer in order.
struct ParallelPieces {
    url: String,
    headers: Vec<(String, String)>,
    /// Start of each piece, and its download (None once handed out to a thread).
    starts: Vec<u64>,
    total: u64,
    next_to_start: usize,
    rx: std::sync::mpsc::Receiver<(usize, Result<Vec<u8>, String>)>,
    tx: std::sync::mpsc::Sender<(usize, Result<Vec<u8>, String>)>,
    tries: Vec<u32>,
}

impl ParallelPieces {
    fn start(url: &str, headers: &[(String, String)], from: u64, total: u64) -> Self {
        let starts: Vec<u64> = (from..total).step_by(PIECE as usize).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut p = Self { url: url.to_string(), headers: headers.to_vec(), tries: vec![0; starts.len()], starts, total, next_to_start: 0, rx, tx };
        for _ in 0..PARALLEL {
            p.launch_next();
        }
        p
    }

    fn launch(&self, i: usize) {
        let (url, headers, tx) = (self.url.clone(), self.headers.clone(), self.tx.clone());
        let start = self.starts[i];
        let end = (start + PIECE).min(self.total) - 1;
        std::thread::spawn(move || {
            let r = crate::net::get_range(&url, &headers, start, Some(end)).and_then(|mut r| {
                let mut v = Vec::with_capacity((end - start + 1) as usize);
                r.body.read_to_end(&mut v).map_err(|e| e.to_string())?;
                if v.len() as u64 != end - start + 1 {
                    return Err("short piece".into());
                }
                Ok(v)
            });
            let _ = tx.send((i, r));
        });
    }

    fn launch_next(&mut self) {
        if self.next_to_start < self.starts.len() {
            self.launch(self.next_to_start);
            self.next_to_start += 1;
        }
    }

    /// Adds the pieces to the buffer in order as they arrive; gives up on a piece after 4 tries.
    fn finish(&mut self, buf: &StreamBuffer, buffered: &Mutex<f32>, total: u64, still: &dyn Fn() -> bool) {
        let mut waiting: std::collections::HashMap<usize, Vec<u8>> = Default::default();
        let mut next = 0;
        while next < self.starts.len() {
            if !still() {
                return;
            }
            // The buffer must have reached this piece's start (the first piece is sequential).
            if buf.state.lock().unwrap().bytes.len() as u64 != self.starts[next] {
                return;
            }
            if let Some(v) = waiting.remove(&next) {
                let mut st = buf.state.lock().unwrap();
                st.bytes.extend_from_slice(&v);
                *buffered.lock().unwrap() = st.bytes.len() as f32 / total.max(1) as f32;
                drop(st);
                buf.changed.notify_all();
                next += 1;
                continue;
            }
            match self.rx.recv_timeout(Duration::from_millis(200)) {
                Ok((i, Ok(v))) => {
                    waiting.insert(i, v);
                    self.launch_next();
                }
                Ok((i, Err(_))) => {
                    self.tries[i] += 1;
                    if !wait_to_retry(self.tries[i] as u64, still) {
                        return;
                    }
                    self.launch(i);
                }
                Err(_) => {}
            }
        }
    }
}

/// `anywhere`: the decoder may jump around the file (it then needs all of it to open an MP4).
fn build_decoder(reader: StreamReader, codec: Codec, total: Option<u64>, anywhere: bool) -> Result<Decoder<StreamReader>, String> {
    let (hint, mime) = codec.hint();
    let mut builder = Decoder::builder()
        .with_data(reader)
        .with_hint(hint)
        .with_mime_type(mime)
        .with_seekable(anywhere)
        .with_coarse_seek(total.is_some() && codec == Codec::Mp3);
    if let Some(t) = total {
        builder = builder.with_byte_len(t);
    }
    // Last, because giving the size switches jumping around back on.
    builder = builder.with_seekable(anywhere);
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

type Dec = StreamDecoder;

/// Times the steps of starting a song, from the progress messages it reports.
struct Steps {
    marks: Mutex<Vec<(&'static str, std::time::Instant)>>,
}

impl Steps {
    fn new() -> Self {
        Self { marks: Mutex::new(vec![("getting ready", std::time::Instant::now())]) }
    }

    /// Notes that a new step began (repeated messages of the same step are ignored).
    fn mark(&self, text: &str) {
        let step = match text {
            t if t.starts_with("First-time setup") => "first-time setup",
            t if t.starts_with("Finding") => "finding",
            t if t.starts_with("Loading audio") => "link",
            t if t.starts_with("Updating") => "updating yt-dlp",
            "audio" => "audio",
            _ => "other",
        };
        let mut m = self.marks.lock().unwrap();
        if m.last().is_none_or(|(s, _)| *s != step) {
            m.push((step, std::time::Instant::now()));
        }
    }

    fn summary(&self, started: std::time::Instant) -> String {
        let m = self.marks.lock().unwrap();
        let now = std::time::Instant::now();
        let parts: Vec<String> = m
            .iter()
            .enumerate()
            .map(|(i, (step, at))| (step, m.get(i + 1).map(|(_, t)| *t).unwrap_or(now) - *at))
            .filter(|(_, d)| d.as_secs_f32() >= 0.05)
            .map(|(step, d)| format!("{step} {:.1} s", d.as_secs_f32()))
            .collect();
        format!("Started in {:.1} s: {}", started.elapsed().as_secs_f32(), parts.join(" · "))
    }
}

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
    /// A jump to a part of the song that hasn't downloaded yet: made once it has.
    pending_seek: Option<Duration>,
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
    /// How long the last song took to start, step by step (shown on the song title).
    timing: Arc<Mutex<Option<String>>>,
    alive: Arc<AtomicBool>,
    pub device_error: Option<String>,
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

/// The first-launch intro's beat, "bum, ba-dum-dum-DUM": when each hit lands (seconds
/// after the start), its note (Hz), whether it has a bass thump, its loudness, and the
/// chord on the last one. The intro's bars bounce at the same times.
pub const INTRO_HITS: [(f32, f32, bool, f32); 5] =
    [(0.0, 65.4, true, 0.9), (0.42, 164.8, false, 0.45), (0.56, 196.0, false, 0.5), (0.7, 261.6, false, 0.55), (0.9, 130.8, true, 1.0)];
const INTRO_CHORD: [f32; 3] = [261.6, 329.6, 392.0];

/// The intro's beat as samples (mono), made here: there's no sound file.
pub fn intro_samples(rate: u32) -> Vec<f32> {
    let rate_f = rate as f32;
    let mut out = vec![0f32; (rate_f * 2.0) as usize];
    // One plucked note: a quick bounce down in pitch, a fast attack, then it dies away.
    let mut note = |at: f32, freq: f32, triangle: bool, gain: f32, len: f32, bounce: bool| {
        let start = (at * rate_f) as usize;
        let mut phase = 0f32;
        for i in 0..((len + 0.05) * rate_f) as usize {
            let t = i as f32 / rate_f;
            let f = if bounce { freq * 1.5 * (1.0 / 1.5f32).powf((t / 0.06).min(1.0)) } else { freq };
            phase += std::f32::consts::TAU * f / rate_f;
            let wave = if triangle { std::f32::consts::FRAC_2_PI * phase.sin().asin() } else { phase.sin() };
            let env = if t < 0.012 { 0.0001 * (gain / 0.0001).powf(t / 0.012) } else { gain * (0.0001 / gain).powf(((t - 0.012) / (len - 0.012)).min(1.0)) };
            if let Some(o) = out.get_mut(start + i) {
                *o += wave * env;
            }
        }
    };
    for (at, freq, sub, gain) in INTRO_HITS {
        let len = if sub { 0.55 } else { 0.28 };
        note(at, freq, true, gain * 0.6, len, true);
        if sub {
            note(at, freq / 2.0, false, gain, len + 0.2, true); // the bass thump
        }
        if at > 0.8 {
            for f in INTRO_CHORD {
                note(at, f, false, 0.18, 0.7, false);
            }
        }
    }
    // Softer highs (a gentle low-pass), and overall level.
    let k = 1.0 - (-std::f32::consts::TAU * 2600.0 / rate_f).exp();
    let mut y = 0f32;
    for s in &mut out {
        y += k * (*s - y);
        *s = (y * 0.55).clamp(-1.0, 1.0);
    }
    out
}

/// Start opening the next song when the current one has this long left.
const PREPARE_AHEAD: Duration = Duration::from_secs(45);
/// Gapless: queue the next song this long before the current one ends.
const QUEUE_AHEAD: Duration = Duration::from_secs(4);

impl AudioEngine {
    /// Plays the first-launch intro's beat (alongside whatever else is playing).
    pub fn play_intro(&self, volume: f32) {
        let Some(device) = &self._device else { return };
        let rate = 44_100;
        let samples: Vec<f32> = intro_samples(rate).into_iter().map(|s| s * volume.clamp(0.0, 1.0)).collect();
        device.mixer().add(rodio::buffer::SamplesBuffer::new(std::num::NonZero::new(1).unwrap(), std::num::NonZero::new(rate).unwrap(), samples));
    }

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
                pending_seek: None,
                ctx: None,
            })),
            effects: Arc::new(SharedEffects::default()),
            generation: Arc::new(AtomicU64::new(0)),
            next_generation: Arc::new(AtomicU64::new(0)),
            load_state: Arc::new(Mutex::new((0, LoadState::Idle))),
            notice: Arc::new(Mutex::new(None)),
            timing: Arc::new(Mutex::new(None)),
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
            sh.pending_seek = None;
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
        let timing = self.timing.clone();
        let id = id.to_string();
        *timing.lock().unwrap() = None;

        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let steps = Steps::new();
            let set_state = |s: LoadState| {
                let mut ls = load_state.lock().unwrap();
                if ls.0 == generation {
                    ls.1 = s;
                }
                ctx.request_repaint();
            };
            let current = || gen_counter.load(Ordering::SeqCst) == generation;
            let from_ahead = prepared.is_some();
            let (decoder, buffered) = match prepared {
                Some(p) => (p.decoder, p.buffered),
                None => {
                    let status = |text: &str| {
                        steps.mark(text);
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
                    steps.mark("audio");
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
            *timing.lock().unwrap() =
                Some(if from_ahead { format!("Started in {:.1} s (found ahead of time)", started.elapsed().as_secs_f32()) } else { steps.summary(started) });
            let duration = known_or_long(decoder.total_duration(), expected);
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

    /// How long the current song took to start, e.g. "Started in 3.2 s: finding 1.4 s · link 1.1 s · audio 0.7 s".
    pub fn timing(&self) -> Option<String> {
        self.timing.lock().unwrap().clone()
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
        // Waiting for a jump: the music starts again when the jump is made.
        if sh.pending_seek.is_some() {
            return;
        }
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
        sh.pending_seek = None;
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

    /// The playing song's real length, once its audio is open.
    pub fn length(&self) -> Option<Duration> {
        let sh = self.shared.lock().unwrap();
        sh.playing.as_ref().map(|p| p.duration).filter(|d| !d.is_zero() && *d < UNKNOWN_LENGTH)
    }

    pub fn position(&self) -> Duration {
        let sh = self.shared.lock().unwrap();
        if let Some(to) = sh.pending_seek {
            return to;
        }
        match &sh.playing {
            Some(p) => sh.decks[p.deck].get_pos(),
            None => Duration::ZERO,
        }
    }

    /// Jumps to a place in the song. A place that hasn't downloaded yet is jumped to as
    /// soon as it has: the music waits meanwhile (see `seeking`), the window doesn't.
    pub fn seek(&self, to: Duration) {
        let mut sh = self.shared.lock().unwrap();
        let Some(p) = &sh.playing else { return };
        let (deck, duration) = (p.deck, p.duration);
        let have = *sh.buffered.lock().unwrap();
        if arrived(to, duration, have) {
            sh.pending_seek = None;
            let _ = sh.decks[deck].try_seek(to);
            if !sh.paused {
                sh.decks[deck].play();
            }
        } else {
            sh.pending_seek = Some(to);
            sh.decks[deck].pause();
        }
    }

    /// True while a jump waits for its part of the song to download.
    pub fn seeking(&self) -> bool {
        self.shared.lock().unwrap().pending_seek.is_some()
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

/// Whether the part of a song at `to` has downloaded, given how much of the file is here
/// (0.0 – 1.0). A song's bytes are spread evenly enough over its length for this, with a
/// little extra asked for so the jump doesn't land right on the edge.
fn arrived(to: Duration, duration: Duration, have: f32) -> bool {
    if have >= 1.0 || duration.is_zero() {
        return true;
    }
    let at = (to.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
    have >= (at + 0.04).min(1.0)
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

    let Some(p) = &sh.playing else {
        sh.pending_seek = None;
        return;
    };
    // A jump that was waiting for its part of the song: make it once that part is here.
    if let Some(to) = sh.pending_seek {
        let have = *sh.buffered.lock().unwrap();
        if !arrived(to, p.duration, have) {
            return;
        }
        sh.pending_seek = None;
        let _ = sh.decks[p.deck].try_seek(to);
        if !sh.paused {
            sh.decks[p.deck].play();
        }
        if let Some(ctx) = &sh.ctx {
            ctx.request_repaint();
        }
    }
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
                let duration = known_or_long(decoder.total_duration(), duration);
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
        let silent_end =
            fx.automix && pos > Duration::from_secs(30) && remaining < Duration::from_secs(25) && cur_ctl.silent_for() > Duration::from_millis(1500);
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
pub mod tests {
    use super::*;
    use rodio::Source;
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::Instant;

    /// Serves a file over HTTP, deliberately slowly, to imitate a real network.
    /// With `ranges`, it answers Range requests with 206 like YouTube's servers do.
    pub fn slow_server(data: Vec<u8>, bytes_per_sec: usize, ranges: bool) -> String {
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
                    let range = req.lines().find_map(|l| l.strip_prefix("range: bytes=")).map(|r| {
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
                        None => (format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len()), data.clone()),
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
        StreamSource { url, headers: vec![], codec, content_length: None, notice: None, duration: None, keep_as: None }
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
        // Every kind of song starts while it's still downloading.
        assert!(*buffered.lock().unwrap() < 1.0, "playback starts before the download finishes");

        // Jump to 20 s of 30 s: waits for that part to download, then carries on.
        dec.try_seek(Duration::from_secs(20)).expect("seek");
        let rest = dec.count();
        println!("{file}: samples after seeking to 20 s: {rest} (~{:.1} s)", rest as f32 / 88_200.0);
        assert!(rest > 88_200 * 8 && rest < 88_200 * 11, "about 10 s should remain");
        assert!((*buffered.lock().unwrap() - 1.0).abs() < 1e-6, "download completed");
    }

    /// Going back in a song that's still arriving, then forward again, then back once it's all here.
    fn check_going_back(file: &str, codec: Codec) {
        let Ok(data) = std::fs::read(file) else {
            eprintln!("skipping: {file} not found");
            return;
        };
        let url = slow_server(data, 250_000, true);
        let buffered = Arc::new(Mutex::new(0.0));
        let mut dec = open_streaming_decoder(&source(url, codec), || true, buffered.clone()).expect("decoder");
        let secs = |n: usize| n as f32 / 88_200.0;
        assert_eq!((&mut dec).take(88_200 * 3).count(), 88_200 * 3, "3 s play");
        dec.try_seek(Duration::from_secs(1)).expect("back while downloading");
        assert!(*buffered.lock().unwrap() < 1.0, "still downloading when going back");
        assert_eq!((&mut dec).take(88_200 * 2).count(), 88_200 * 2, "plays on after going back");
        dec.try_seek(Duration::from_secs(25)).expect("forward");
        let n = (&mut dec).take(88_200).count();
        assert_eq!(n, 88_200, "plays after jumping ahead");
        dec.try_seek(Duration::from_secs(10)).expect("back after everything arrived");
        let rest = dec.count();
        assert!((secs(rest) - 20.0).abs() < 1.5, "about 20 s remain after going back to 10 s, got {:.1}", secs(rest));
    }

    #[test]
    fn going_back_in_a_streaming_m4a() {
        check_going_back("/tmp/test_dash.m4a", Codec::Mp4);
        check_going_back("/tmp/test_plain.m4a", Codec::Mp4);
    }

    #[test]
    fn going_back_in_a_streaming_mp3() {
        check_going_back("/tmp/test.mp3", Codec::Mp3);
    }

    /// A 4-minute song from a server that limits each connection's speed (like YouTube):
    /// several pieces download side by side, it starts much sooner than one connection
    /// would allow, and every byte ends up in the right place.
    #[test]
    fn long_song_downloads_in_parallel_pieces() {
        let Ok(data) = std::fs::read("/tmp/test_long.m4a") else {
            eprintln!("skipping: /tmp/test_long.m4a not found");
            return;
        };
        let one_connection = data.len() as f32 / 500_000.0;
        let url = slow_server(data, 500_000, true);
        let buffered = Arc::new(Mutex::new(0.0));
        let t0 = Instant::now();
        let dec = open_streaming_decoder(&source(url, Codec::Mp4), || true, buffered.clone()).expect("decoder");
        let ready = t0.elapsed().as_secs_f32();
        println!("ready after {ready:.1} s (one connection would need ~{one_connection:.1} s)");
        assert!(ready < 1.0, "starts on the first part, long before the download ends");
        assert!(*buffered.lock().unwrap() < 0.6, "still downloading when it starts");
        let samples = dec.count();
        let secs = samples as f32 / 88_200.0;
        assert!((secs - 240.0).abs() < 1.0, "whole song decoded in order: {secs:.1} s");
    }

    /// A song that has fully played through is kept, byte for byte, for next time.
    #[test]
    fn a_fully_downloaded_song_is_kept_for_next_time() {
        let Ok(data) = std::fs::read("/tmp/test_dash.m4a") else {
            eprintln!("skipping: /tmp/test_dash.m4a not found");
            return;
        };
        let dir = std::env::temp_dir().join(format!("pulse-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut src = source(slow_server(data.clone(), 2_000_000, true), Codec::Mp4);
        src.keep_as = Some(dir.join("song.m4a"));
        let dec = open_streaming_decoder(&src, || true, Arc::new(Mutex::new(0.0))).expect("decoder");
        assert!(dec.count() > 88_200 * 25, "played to the end");
        let t0 = Instant::now();
        while !dir.join("song.m4a").is_file() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(std::fs::read(dir.join("song.m4a")).expect("kept"), data);

        // Skipped before it finished: nothing is kept.
        let mut src = source(slow_server(data, 100_000, true), Codec::Mp4);
        src.keep_as = Some(dir.join("skipped.m4a"));
        let dec = open_streaming_decoder(&src, || true, Arc::new(Mutex::new(0.0))).expect("decoder");
        drop(dec);
        std::thread::sleep(Duration::from_millis(400));
        assert!(!dir.join("skipped.m4a").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_jump_waits_only_for_parts_that_have_not_arrived() {
        let song = Duration::from_secs(200);
        assert!(arrived(Duration::from_secs(20), song, 0.5), "already here");
        assert!(!arrived(Duration::from_secs(150), song, 0.5), "not here yet");
        assert!(!arrived(Duration::from_secs(100), song, 0.5), "too close to the edge");
        assert!(arrived(Duration::from_secs(199), song, 1.0), "everything is here");
        assert!(arrived(Duration::from_secs(5), Duration::ZERO, 0.0), "unknown length: just try");
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

#[cfg(test)]
mod intro_tests {
    #[test]
    fn the_intro_beat_is_short_and_doesnt_clip() {
        let s = super::intro_samples(44_100);
        assert_eq!(s.len(), 88_200);
        let peak = s.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.2 && peak < 1.0, "peak {peak}");
        // Each hit is heard: something is playing just after each one starts.
        for (at, ..) in super::INTRO_HITS {
            let i = ((at + 0.03) * 44_100.0) as usize;
            assert!(s[i..i + 400].iter().any(|v| v.abs() > 0.02), "hit at {at}");
        }
    }
}

/// Stands in for a length nobody knows (no crossfade or "next song" until the audio ends).
const UNKNOWN_LENGTH: Duration = Duration::from_secs(24 * 3600);

/// The audio's own length, else the expected one, else "unknown" (never 0: a 0 length
/// would move on to the next song at once).
fn known_or_long(own: Option<Duration>, expected: Duration) -> Duration {
    own.filter(|d| !d.is_zero()).or(Some(expected).filter(|d| !d.is_zero())).unwrap_or(UNKNOWN_LENGTH)
}
