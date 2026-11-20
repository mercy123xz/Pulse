//! The audio engine.
//!
//! Playing a song has two steps, both on a background thread:
//! 1. *Resolve*: work out where the audio is (for Pulse: find the song on YouTube).
//! 2. *Stream*: download it in chunks into a shared buffer while the decoder plays
//!    from that buffer, waiting briefly whenever it catches up with the download.

use crate::dsp::{Processor, SharedEffects, TrackCtl};
use crate::resample::Resampled;
use rodio::Source;
use rodio::{Decoder, MixerDeviceSink, Player};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

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
/// How many pieces download at the same time (a few, to beat the per-connection speed limit
/// without loading the PC or the network).
const PARALLEL: usize = 6;
/// The parallel pieces wait until this much of the start of the song has arrived, so on a
/// slow connection they don't take bandwidth from the part needed to start playing.
/// (Benchmarked in `bench_startup`; settable there to compare.)
static HEAD_START: AtomicU64 = AtomicU64::new(128 * 1024);

// ---------------------------------------------------------------------------
// TEMPORARY: playback timing log (logs\playback-debug.log in the data folder).
// Each line: the time, ms since the last song was picked, and what happened.
// Used to find where the wait before a song starts goes. Remove once that's settled.
// ---------------------------------------------------------------------------

static PICKED_AT: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Notes that a song was just picked: later lines count from here. Only stores the time
/// (it runs on the UI thread); the line itself is written by the loading thread.
fn mark_pick() {
    *PICKED_AT.lock().unwrap() = Some(std::time::Instant::now());
}

/// One line in the playback log. Cheap: a few lines per song, written by a logging
/// thread (see `log_in_background`).
pub fn trace(msg: &str) {
    if cfg!(test) {
        return;
    }
    let since = PICKED_AT.lock().unwrap().map(|t| t.elapsed().as_millis()).unwrap_or(0);
    let unix_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    log_in_background(crate::settings::data_dir().join("logs").join("playback-debug.log"), format!("[{unix_ms}] +{since:>6} ms  {msg}"));
}

/// Hands a log line to one logging thread, which writes it. The caller never touches the
/// disk, so this is safe from the UI and audio threads (the first-sound line is written
/// from the audio thread).
pub fn log_in_background(path: std::path::PathBuf, line: String) {
    static LOGGER: std::sync::OnceLock<std::sync::mpsc::Sender<(std::path::PathBuf, String)>> = std::sync::OnceLock::new();
    let tx = LOGGER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<(std::path::PathBuf, String)>();
        std::thread::spawn(move || {
            for (path, line) in rx {
                append_log_line(&path, &line);
            }
        });
        tx
    });
    let _ = tx.send((path, line));
}

/// Appends one line to a debug log: the whole line in a single write, one thread at a
/// time (lines from different threads used to interleave mid-line). Starts the file over
/// past 512 KB.
pub fn append_log_line(path: &std::path::Path, line: &str) {
    use std::io::Write;
    static LOG_LOCK: Mutex<()> = Mutex::new(());
    let _one_at_a_time = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > 512 * 1024) {
        let _ = std::fs::remove_file(path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

/// The server's name only (links carry private, expiring query strings).
fn host_of(url: &str) -> &str {
    url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or("?")
}

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
    open_traced(src, still_wanted, buffered, "song")
}

/// `open_streaming_decoder`, with `label` ("song" or "next") on its playback-log lines.
fn open_traced(src: &StreamSource, still_wanted: impl Fn() -> bool + Send + 'static, buffered: Arc<Mutex<f32>>, label: &'static str) -> Result<StreamDecoder, String> {
    let opened = std::time::Instant::now();
    if let Some(path) = src.url.strip_prefix("file://") {
        trace(&format!("{label}: from disk ({})", path.rsplit(['\\', '/']).next().unwrap_or(path)));
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
        trace(&format!("{label}: read {} KB from disk and decoder ready in {} ms", total / 1024, opened.elapsed().as_millis()));
        return Ok(StreamDecoder::new(inner, None, src.duration).traced(label));
    }
    trace(&format!("{label}: HTTP request to {} ({:?})", host_of(&src.url), src.codec));
    let first = crate::net::get_range(&src.url, &src.headers, 0, Some(FIRST_CHUNK - 1)).inspect_err(|e| trace(&format!("{label}: request failed: {e}")))?;
    trace(&format!(
        "{label}: first response in {} ms ({}, size {})",
        opened.elapsed().as_millis(),
        if first.partial { "206, pieces allowed" } else { "200, one piece" },
        first.total.map(|t| format!("{} KB", t / 1024)).unwrap_or("unknown".into())
    ));
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
            // The other pieces start once the beginning of the song is in (see HEAD_START),
            // while the first one is still arriving.
            let pieces = parallel_from.map(|from| ParallelPieces::start(&url, &headers, from, total.unwrap(), &buf, HEAD_START.load(Ordering::Relaxed)));
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
    let building = std::time::Instant::now();
    let inner = match build_decoder(StreamReader::new(&buf), src.codec, total, !forward) {
        Ok(d) => d,
        // A file with its index at the end can't start early: wait for all of it.
        Err(_) if forward => {
            trace(&format!("{label}: index at the end of the file, waiting for all of it"));
            let d = build_decoder(StreamReader::new(&buf), src.codec, total, true)?;
            trace(&format!("{label}: decoder ready in {} ms (whole file)", building.elapsed().as_millis()));
            return Ok(StreamDecoder::new(d, None, src.duration).traced(label));
        }
        Err(e) => return Err(e),
    };
    drop(keep);
    trace(&format!(
        "{label}: decoder ready in {} ms (with {} KB here), {} ms after the request",
        building.elapsed().as_millis(),
        buf.state.lock().unwrap().bytes.len() / 1024,
        opened.elapsed().as_millis()
    ));
    Ok(StreamDecoder::new(inner, forward.then_some(Reopen { buf, codec: src.codec, total }), src.duration).traced(label))
}

/// What's needed to open a song again from its start.
struct Reopen {
    buf: Arc<StreamBuffer>,
    codec: Codec,
    total: Option<u64>,
}

/// A song's decoder. For audio that plays while it downloads, going back in the song opens
/// the (already downloaded) audio again from the start and moves forward from there.
/// Its sound comes out already at the sound card's rate (see `resample`).
pub struct StreamDecoder {
    inner: Resampled<Decoder<StreamReader>>,
    reopen: Option<Reopen>,
    /// Where the last jump landed, and the samples played since.
    base: Duration,
    samples: u64,
    /// The length to report when the file itself doesn't say.
    known_duration: Option<Duration>,
    /// For the playback log: the label to write once the first sound is handed to the speakers.
    first_sound: Option<&'static str>,
}

impl StreamDecoder {
    fn new(inner: Decoder<StreamReader>, reopen: Option<Reopen>, known_duration: Option<Duration>) -> Self {
        StreamDecoder { inner: Resampled::new(inner, out_rate()), reopen, base: Duration::ZERO, samples: 0, known_duration, first_sound: None }
    }

    fn traced(mut self, label: &'static str) -> Self {
        self.first_sound = Some(label);
        self
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
        if s.is_some()
            && let Some(label) = self.first_sound.take()
        {
            trace(&format!("{label}: first samples sent to the speakers"));
        }
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
        let t = std::time::Instant::now();
        let r = self.seek_inner(pos);
        trace(&format!("seek to {:.1} s: {} in {} ms", pos.as_secs_f32(), if r.is_ok() { "done" } else { "failed" }, t.elapsed().as_millis()));
        r
    }
}

impl StreamDecoder {
    fn seek_inner(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        if let Some(r) = &self.reopen
            && pos < self.played()
        {
            // Once everything has arrived, open it for jumping anywhere.
            let done = r.buf.state.lock().unwrap().done;
            let fresh =
                build_decoder(StreamReader::new(&r.buf), r.codec, r.total, done).map_err(|e| rodio::source::SeekError::Other(Arc::new(io::Error::other(e))))?;
            self.inner = Resampled::new(fresh, out_rate());
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

/// The sound card's sample rate (0 until one opens). Songs are converted to it as they're
/// decoded (see `resample`), so rodio's rough converter never has to.
static OUT_RATE: AtomicU32 = AtomicU32::new(0);

fn out_rate() -> rodio::SampleRate {
    rodio::SampleRate::new(OUT_RATE.load(Ordering::Relaxed)).unwrap_or(rodio::SampleRate::new(48_000).unwrap())
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
    let mut first_bytes = buf.state.lock().unwrap().bytes.is_empty();
    loop {
        if !still() {
            return;
        }
        match body.read(&mut chunk) {
            Ok(n) if n > 0 => {
                if std::mem::take(&mut first_bytes) {
                    trace(&format!("audio data: first {n} bytes arrived"));
                }
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
                if let Err(e) = &result {
                    trace(&format!("audio data: connection dropped at {} KB ({e}), resuming", have / 1024));
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

#[cfg(test)]
mod stale_loader_tests {
    use super::*;
    use std::time::Instant;

    struct InstallGate {
        entered: std::sync::mpsc::Sender<()>,
        resume: std::sync::mpsc::Receiver<()>,
        finished: std::sync::mpsc::Sender<()>,
    }

    static INSTALL_GATES: Mutex<Vec<(std::sync::Weak<Mutex<Shared>>, InstallGate)>> = Mutex::new(Vec::new());

    pub(super) fn wait_to_install(shared: &Arc<Mutex<Shared>>) -> Option<std::sync::mpsc::Sender<()>> {
        let gate = {
            let mut gates = INSTALL_GATES.lock().unwrap();
            gates.iter().position(|(owner, _)| owner.ptr_eq(&Arc::downgrade(shared))).map(|i| gates.remove(i).1)
        };
        gate.map(|g| {
            g.entered.send(()).unwrap();
            g.resume.recv().unwrap();
            g.finished // Dropped when this loader exits, even if it is rejected.
        })
    }

    fn wav_source(url: String) -> StreamSource {
        StreamSource { url, headers: vec![], codec: Codec::Wav, content_length: None, notice: None, duration: None, keep_as: None }
    }

    #[test]
    fn a_loader_waiting_to_install_cannot_replace_a_newer_song() {
        let file = std::env::temp_dir().join(format!("pulse-loader-{}.wav", std::process::id()));
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&8_036u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&96_000u32.to_le_bytes());
        wav.extend_from_slice(b"\x02\0\x10\0data");
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&vec![32; 8_000]);
        std::fs::write(&file, wav).unwrap();
        let (deck, mut output) = Player::new();
        let running = Arc::new(AtomicBool::new(true));
        let alive = running.clone();
        let consumer = std::thread::spawn(move || {
            while alive.load(Ordering::SeqCst) {
                for _ in 0..512 { output.next(); }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        let engine = AudioEngine {
            _device: None,
            shared: Arc::new(Mutex::new(Shared {
                decks: vec![Arc::new(deck)], playing: None, queued: None, fading_out: None,
                next: None, prepared: None, advanced: None, paused: false, volume: 1.0,
                buffered: Arc::new(Mutex::new(0.0)), pending_seek: None, ctx: None,
            })),
            effects: Arc::default(), generation: Arc::default(), next_generation: Arc::default(),
            load_state: Arc::new(Mutex::new((0, LoadState::Idle))), notice: Arc::default(), timing: Arc::default(),
            alive: Arc::default(), device_error: None,
        };
        // Hold the old loader at the installation boundary, after its last unlocked
        // generation check. A blocking resolver alone would only exercise an earlier guard.
        for stop in [false, true] {
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (resume_tx, resume_rx) = std::sync::mpsc::channel();
            let (finished_tx, finished_rx) = std::sync::mpsc::channel();
            INSTALL_GATES.lock().unwrap().push((Arc::downgrade(&engine.shared), InstallGate { entered: entered_tx, resume: resume_rx, finished: finished_tx }));
            let url = format!("file://{}", file.display());
            engine.load("old", Duration::from_secs(1), Box::new(move |_| Ok(wav_source(url))), false, egui::Context::default());
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if stop {
                engine.stop();
            } else {
                let url = format!("file://{}", file.display());
                engine.load("new", Duration::from_secs(1), Box::new(move |_| Ok(wav_source(url))), false, egui::Context::default());
                let deadline = Instant::now() + Duration::from_secs(5);
                while engine.load_state() != LoadState::Ready {
                    assert!(Instant::now() < deadline, "new song should finish loading");
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            resume_tx.send(()).unwrap();
            // Disconnection acknowledges that the old loader returned, without sleeps.
            assert!(matches!(finished_rx.recv_timeout(Duration::from_secs(5)), Err(std::sync::mpsc::RecvTimeoutError::Disconnected)));
            let actual = engine.shared.lock().unwrap().playing.as_ref().map(|p| p.id.clone());
            assert_eq!(actual.as_deref(), if stop { None } else { Some("new") }, "a stale loader installed after the newer request");
        }
        running.store(false, Ordering::SeqCst);
        consumer.join().unwrap();
        std::fs::remove_file(file).unwrap();
    }

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
    /// Starts the first few pieces once `head_start` bytes of the song are in `buf`.
    fn start(url: &str, headers: &[(String, String)], from: u64, total: u64, buf: &Arc<StreamBuffer>, head_start: u64) -> Self {
        let starts: Vec<u64> = (from..total).step_by(PIECE as usize).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let first = starts.len().min(PARALLEL);
        let p = Self { url: url.to_string(), headers: headers.to_vec(), tries: vec![0; starts.len()], starts, total, next_to_start: first, rx, tx };
        let jobs: Vec<(u64, u64)> = p.starts[..first].iter().map(|&s| (s, (s + PIECE).min(total) - 1)).collect();
        let (url, headers, tx, buf) = (p.url.clone(), p.headers.clone(), p.tx.clone(), buf.clone());
        std::thread::spawn(move || {
            {
                let mut st = buf.state.lock().unwrap();
                while (st.bytes.len() as u64) < head_start && !st.done && !st.cancelled {
                    st = buf.changed.wait(st).unwrap();
                }
                if st.cancelled {
                    return;
                }
            }
            for (i, (start, end)) in jobs.into_iter().enumerate() {
                Self::fetch(url.clone(), headers.clone(), tx.clone(), i, start, end);
            }
        });
        p
    }

    fn launch(&self, i: usize) {
        let start = self.starts[i];
        let end = (start + PIECE).min(self.total) - 1;
        Self::fetch(self.url.clone(), self.headers.clone(), self.tx.clone(), i, start, end);
    }

    fn fetch(url: String, headers: Vec<(String, String)>, tx: std::sync::mpsc::Sender<(usize, Result<Vec<u8>, String>)>, i: usize, start: u64, end: u64) {
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

#[cfg(test)]
mod gapless_queue_tests {
    use super::*;

    fn gapless_test_engine() -> (AudioEngine, rodio::queue::SourcesQueueOutput) {
        let (deck, output) = Player::new();
        deck.append(rodio::buffer::SamplesBuffer::new(2.try_into().unwrap(), 48_000.try_into().unwrap(), vec![0.25; 512]));
        let engine = AudioEngine {
            _device: None,
            shared: Arc::new(Mutex::new(Shared {
                decks: vec![Arc::new(deck)],
                playing: Some(Playing { id: "current".into(), deck: 0, ctl: Arc::default(), duration: Duration::from_secs(1) }),
                queued: None, fading_out: None, next: None, prepared: None, advanced: None,
                paused: false, volume: 1.0, buffered: Arc::new(Mutex::new(0.5)), pending_seek: None, ctx: None,
            })),
            effects: Arc::default(), generation: Arc::default(), next_generation: Arc::default(),
            load_state: Arc::new(Mutex::new((0, LoadState::Ready))), notice: Arc::default(),
            timing: Arc::default(), alive: Arc::default(), device_error: None,
        };
        (engine, output)
    }

    fn gapless_prepared(id: &str, sample: i16) -> Prepared {
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&1_060u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x02\0");
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&192_000u32.to_le_bytes());
        wav.extend_from_slice(b"\x04\0\x10\0data");
        wav.extend_from_slice(&1_024u32.to_le_bytes());
        for _ in 0..512 { wav.extend_from_slice(&sample.to_le_bytes()); }
        let total = wav.len() as u64;
        let buf = Arc::new(StreamBuffer::default());
        *buf.state.lock().unwrap() = BufState { bytes: wav, total: Some(total), done: true, cancelled: false };
        let decoder = build_decoder(StreamReader::new(&buf), Codec::Wav, Some(total), true).unwrap();
        Prepared { id: id.into(), decoder: StreamDecoder::new(decoder, None, None), duration: Duration::from_secs(1), buffered: Arc::new(Mutex::new(1.0)) }
    }

    fn queue_gapless_test_song(engine: &AudioEngine, id: &str, sample: i16) {
        engine.set_next(Some((id.into(), Duration::from_secs(1), Box::new(|_| Err("test supplies a prepared decoder".into())))));
        let mut sh = engine.shared.lock().unwrap();
        sh.next.as_mut().unwrap().resolver = None;
        sh.prepared = Some(gapless_prepared(id, sample));
        monitor_step(&mut sh, &Effects { gapless: true, ..Default::default() }, &engine.effects, &engine.shared, &engine.next_generation);
    }

    #[test]
    fn changing_a_queued_gapless_song_preserves_every_current_sample() {
        for replace in [false, true] {
            let (engine, mut output) = gapless_test_engine();
            assert!(output.by_ref().take(128).all(|s| s == 0.25));
            queue_gapless_test_song(&engine, "removed", 16_384);
            if replace {
                queue_gapless_test_song(&engine, "replacement", 24_576);
            } else {
                engine.pause();
                engine.set_next(None);
                engine.play();
            }
            assert!(output.by_ref().take(384).all(|s| s == 0.25), "the current song must remain uninterrupted");
            let expected = if replace { 0.75 } else { 0.0 };
            assert!(output.by_ref().take(512).all(|s| s == expected), "the removed song played (or the replacement had a gap)");
            let mut sh = engine.shared.lock().unwrap();
            monitor_step(&mut sh, &Effects::default(), &engine.effects, &engine.shared, &engine.next_generation);
            assert_eq!(sh.advanced.as_deref(), if replace { Some("replacement") } else { None });
        }
    }

    #[test]
    fn keeping_the_queued_song_or_changing_next_after_it_starts_does_not_cut_it_off() {
        let (engine, mut output) = gapless_test_engine();
        assert!(output.by_ref().take(128).all(|s| s == 0.25));
        queue_gapless_test_song(&engine, "queued", 16_384);
        engine.set_next(Some(("queued".into(), Duration::from_secs(1), Box::new(|_| panic!("already queued")))));
        assert!(output.by_ref().take(384).all(|s| s == 0.25));
        assert_eq!(output.next(), Some(0.5)); // The queued song really started.
        engine.set_next(None);
        assert!(output.by_ref().take(511).all(|s| s == 0.5), "changing Next must not stop a song that has already started");
        let mut sh = engine.shared.lock().unwrap();
        monitor_step(&mut sh, &Effects::default(), &engine.effects, &engine.shared, &engine.next_generation);
        assert_eq!(sh.playing.as_ref().unwrap().id, "queued");
    }

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

/// A gapless song stays removable until the audio thread claims its first sample.
struct GaplessSlot {
    source: Option<Processor<Dec>>,
    started: bool,
}

struct Queued {
    playing: Playing,
    slot: Arc<Mutex<GaplessSlot>>,
    buffered: Arc<Mutex<f32>>,
}

struct GaplessSource {
    slot: Arc<Mutex<GaplessSlot>>,
    inner: Option<Processor<Dec>>,
    channels: rodio::ChannelCount,
    rate: rodio::SampleRate,
    duration: Option<Duration>,
}

impl GaplessSource {
    fn new(source: Processor<Dec>) -> Self {
        let (channels, rate, duration) = (source.channels(), source.sample_rate(), source.total_duration());
        Self { slot: Arc::new(Mutex::new(GaplessSlot { source: Some(source), started: false })), inner: None, channels, rate, duration }
    }
}

impl Iterator for GaplessSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.inner.is_none() {
            let mut slot = self.slot.lock().unwrap();
            self.inner = slot.source.take();
            slot.started = self.inner.is_some();
        }
        self.inner.as_mut()?.next()
    }
}

impl Source for GaplessSource {
    fn current_span_len(&self) -> Option<usize> { None }
    fn channels(&self) -> rodio::ChannelCount { self.channels }
    fn sample_rate(&self) -> rodio::SampleRate { self.rate }
    fn total_duration(&self) -> Option<Duration> { self.duration }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        if let Some(inner) = &mut self.inner {
            inner.try_seek(pos)
        } else if let Some(source) = &mut self.slot.lock().unwrap().source {
            source.try_seek(pos)
        } else {
            Ok(())
        }
    }
}

/// State shared by the engine, its loading threads and the monitor thread.
struct Shared {
    /// Two players ("decks") so one song can fade out while the next fades in.
    decks: Vec<Arc<Player>>,
    playing: Option<Playing>,
    /// Gapless: the next song, already queued behind the current one on the same deck.
    queued: Option<Queued>,
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
        if let Some(d) = &device {
            OUT_RATE.store(d.config().sample_rate().get(), Ordering::Relaxed);
        }
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
        mark_pick();
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
            trace(&format!("--- picked {id}{}", if from_ahead { " (already opened ahead of time)" } else { "" }));
            let (decoder, buffered) = match prepared {
                Some(p) => (p.decoder, p.buffered),
                None => {
                    let status = |text: &str| {
                        if !text.starts_with("First-time setup") || text.ends_with(" 0%") {
                            trace(&format!("status: {text}"));
                        }
                        steps.mark(text);
                        if current() {
                            set_state(LoadState::Resolving(text.to_owned()));
                        }
                    };
                    let src = match resolve(&status) {
                        Ok(s) => s,
                        Err(e) => {
                            trace(&format!("link: failed: {e}"));
                            return set_state(LoadState::Failed(e));
                        }
                    };
                    trace(&format!("link: ready in {} ms", started.elapsed().as_millis()));
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
            trace(&format!("player: song handed to the player, {} ms after it was picked", started.elapsed().as_millis()));
            *timing.lock().unwrap() =
                Some(if from_ahead { format!("Started in {:.1} s (found ahead of time)", started.elapsed().as_secs_f32()) } else { steps.summary(started) });
            let duration = known_or_long(decoder.total_duration(), expected);
            let ctl = Arc::new(TrackCtl::default());
            let source = Processor::new(decoder, effects, ctl.clone(), 1.0, false);
            #[cfg(test)]
            let _finished = stale_loader_tests::wait_to_install(&shared);
            {
                let mut sh = shared.lock().unwrap();
                // Another request may have cleared the decks while this loader waited
                // for the lock. Check again before replacing the current source.
                if !current() {
                    return;
                }
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
            (None, None) => sh.queued.is_none(),
            _ => false,
        };
        let same_prepared = match (&next, &sh.prepared) {
            (Some((id, _, _)), Some(p)) => *id == p.id,
            _ => false,
        };
        let same_queued = next.as_ref().is_some_and(|(id, _, _)| sh.queued.as_ref().is_some_and(|q| q.playing.id == *id));
        if same || same_prepared || same_queued {
            return;
        }
        settle_gapless(&mut sh, true);
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

/// Serialize cancellation with the audio thread claiming the queued source. Once it
/// has started it is the current song, so a Next change must leave it playing.
fn settle_gapless(sh: &mut Shared, cancel_pending: bool) {
    let Some(q) = sh.queued.take() else { return };
    let started = {
        let mut slot = q.slot.lock().unwrap();
        if cancel_pending && !slot.started {
            slot.source = None; // Drop its decoder/readers and stop its download now.
        }
        slot.started
    };
    if started {
        sh.advanced = Some(q.playing.id.clone());
        sh.playing = Some(q.playing);
        sh.buffered = q.buffered;
        if let Some(ctx) = &sh.ctx { ctx.request_repaint(); }
    } else if !cancel_pending {
        sh.queued = Some(q);
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

    settle_gapless(sh, false);

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
            let retained = Arc::new(AtomicBool::new(false));
            let still = {
                let next_generation = next_generation.clone();
                let retained = retained.clone();
                move || retained.load(Ordering::SeqCst) || next_generation.load(Ordering::SeqCst) == generation
            };
            let Ok(src) = resolver(&|_| {}) else { return };
            if !still() {
                return;
            }
            let buffered = Arc::new(Mutex::new(0.0));
            let Ok(decoder) = open_traced(&src, still.clone(), buffered.clone(), "next") else { return };
            let mut sh = shared.lock().unwrap();
            if still() && sh.next.as_ref().is_some_and(|n| n.id == id) {
                let duration = known_or_long(decoder.total_duration(), duration);
                // From here the decoder's readers own the download. Changing Next must
                // not cancel it after this song starts; dropping it still cancels normally.
                retained.store(true, Ordering::SeqCst);
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
        let source = GaplessSource::new(Processor::new(pr.decoder, effects.clone(), ctl.clone(), 1.0, false));
        let slot = source.slot.clone();
        sh.decks[cur_deck].append(source);
        sh.queued = Some(Queued { playing: Playing { id: pr.id, deck: cur_deck, ctl, duration: pr.duration }, slot, buffered: pr.buffered });
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

    /// Like `slow_server`, but closer to a real home connection: every connection shares
    /// ONE bandwidth limit (a second request slows the first down), and each request waits
    /// `latency` before its first byte (connecting + the server answering).
    pub fn shared_link_server(data: Vec<u8>, bytes_per_sec: usize, latency: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        // The moment the shared link is next free.
        let link = Arc::new(Mutex::new(Instant::now()));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut s = stream.unwrap();
                let (data, link) = (data.clone(), link.clone());
                std::thread::spawn(move || {
                    let mut req = [0u8; 4096];
                    let n = std::io::Read::read(&mut s, &mut req).unwrap_or(0);
                    let req = String::from_utf8_lossy(&req[..n]).to_lowercase();
                    let (a, b) = req
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .map(|r| {
                            let (a, b) = r.trim().split_once('-').unwrap();
                            (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap_or(data.len() - 1).min(data.len() - 1))
                        })
                        .unwrap_or((0, data.len() - 1));
                    std::thread::sleep(latency);
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Type: audio/mp4\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{}\r\nConnection: close\r\n\r\n",
                        b - a + 1,
                        data.len()
                    );
                    if s.write_all(head.as_bytes()).is_err() {
                        return;
                    }
                    // 8 KB at a time, each taking its turn on the shared link.
                    for part in data[a..=b].chunks(8 * 1024) {
                        let send_at = {
                            let mut free = link.lock().unwrap();
                            let start = (*free).max(Instant::now());
                            *free = start + Duration::from_secs_f64(part.len() as f64 / bytes_per_sec as f64);
                            *free
                        };
                        if let Some(wait) = send_at.checked_duration_since(Instant::now()) {
                            std::thread::sleep(wait);
                        }
                        if s.write_all(part).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        format!("http://{addr}/audio")
    }

    /// Startup timings for one song on one simulated connection.
    pub fn measure_start(file: &str, codec: Codec, bytes_per_sec: usize, latency: Duration) -> Option<(Duration, Duration, Duration)> {
        let data = std::fs::read(file).ok()?;
        let url = shared_link_server(data, bytes_per_sec, latency);
        let buffered = Arc::new(Mutex::new(0.0));
        let t0 = Instant::now();
        let mut dec = open_streaming_decoder(&source(url, codec), || true, buffered.clone()).expect("decoder");
        let opened = t0.elapsed();
        assert!(dec.next().is_some(), "first sample");
        let first_sample = t0.elapsed();
        // 5 seconds of audio decoded: playback is well under way.
        let _ = (&mut dec).take(44_100 * 2 * 5).count();
        let five_s = t0.elapsed();
        Some((opened, first_sample, five_s))
    }

    /// Run with: cargo test --bin pulse bench_startup -- --ignored --nocapture
    /// Needs /tmp/test_long.m4a and /tmp/test_long.mp3 (4 minutes each, see the report).
    #[test]
    #[ignore]
    fn bench_startup() {
        let latency = Duration::from_millis(150);
        let head = std::env::var("PULSE_HEAD_START").ok().and_then(|v| v.parse().ok()).unwrap_or(HEAD_START.load(Ordering::Relaxed));
        HEAD_START.store(head, Ordering::Relaxed);
        println!("BENCH head start {} KB", head / 1024);
        for (file, codec) in [("/tmp/test_long.m4a", Codec::Mp4), ("/tmp/test_long.mp3", Codec::Mp3)] {
            for (label, bps) in [("2 Mbit/s", 250_000), ("8 Mbit/s", 1_000_000), ("40 Mbit/s", 5_000_000)] {
                let mut runs = vec![];
                for _ in 0..3 {
                    runs.push(measure_start(file, codec, bps, latency).expect("file"));
                }
                runs.sort();
                let (o, f, five) = runs[1];
                println!("BENCH {file} {label}: opened {:>5} ms · first sample {:>5} ms · 5 s decoded {:>5} ms", o.as_millis(), f.as_millis(), five.as_millis());
            }
        }
        // A server that limits each connection (as YouTube does) rather than the whole link:
        // how long the whole song takes to arrive, which is what the parallel pieces are for.
        for file in ["/tmp/test_long.m4a"] {
            let data = std::fs::read(file).unwrap();
            let url = slow_server(data, 500_000, true);
            let buffered = Arc::new(Mutex::new(0.0));
            let t0 = Instant::now();
            let dec = open_streaming_decoder(&source(url, Codec::Mp4), || true, buffered.clone()).expect("decoder");
            while *buffered.lock().unwrap() < 1.0 {
                std::thread::sleep(Duration::from_millis(20));
            }
            println!("BENCH {file} per-connection 4 Mbit/s: whole song in {} ms", t0.elapsed().as_millis());
            drop(dec);
        }
    }

    #[test]
    fn log_lines_from_many_threads_stay_whole() {
        let path = std::env::temp_dir().join(format!("pulse-log-test-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for i in 0..200 {
                        append_log_line(&path, &format!("[{t}] line {i} {}", "x".repeat(40)));
                    }
                })
            })
            .collect();
        for th in threads {
            th.join().unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1600);
        assert!(lines.iter().all(|l| l.starts_with('[') && l.ends_with(&"x".repeat(40)) && l.matches('[').count() == 1));
        let _ = std::fs::remove_file(&path);
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

#[cfg(test)]
mod prepared_download_tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::Instant;

    fn wav_source(url: String) -> StreamSource {
        StreamSource { url, headers: vec![], codec: Codec::Wav, content_length: None, notice: None, duration: None, keep_as: None }
    }

    #[test]
    fn a_prepared_song_keeps_downloading_after_next_changes() {
        for transition in ["next", "gapless", "crossfade"] {
            check_prepared_download(transition);
        }
    }

    fn check_prepared_download(transition: &str) {
        // Generate a WAV here: this regression must run without external audio fixtures.
        const SAMPLES: u32 = 400_000;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + SAMPLES * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&96_000u32.to_le_bytes());
        wav.extend_from_slice(b"\x02\0\x10\0data");
        wav.extend_from_slice(&(SAMPLES * 2).to_le_bytes());
        for _ in 0..SAMPLES {
            wav.extend_from_slice(&8_192i16.to_le_bytes());
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/audio", listener.local_addr().unwrap());
        let (release, wait) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = [0; 4096];
            s.read(&mut req).unwrap();
            write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", wav.len()).unwrap();
            s.write_all(&wav[..128_000]).unwrap();
            // Hold the rest until the prepared song has started and Next has changed.
            wait.recv_timeout(Duration::from_secs(15)).unwrap();
            let _ = s.write_all(&wav[128_000..]);
        });

        // A player with an in-memory output: never opens or plays through a real device.
        let (deck, mut output) = Player::new();
        let (other_deck, other_output) = Player::new();
        let engine = AudioEngine {
            _device: None,
            shared: Arc::new(Mutex::new(Shared {
                decks: vec![Arc::new(deck), Arc::new(other_deck)],
                playing: Some(Playing { id: "first".into(), deck: 0, ctl: Arc::default(), duration: Duration::from_secs(3) }),
                queued: None, fading_out: None, next: None, prepared: None, advanced: None,
                paused: false, volume: 1.0, buffered: Arc::new(Mutex::new(1.0)), pending_seek: None, ctx: None,
            })),
            effects: Arc::default(), generation: Arc::default(), next_generation: Arc::default(),
            load_state: Arc::new(Mutex::new((0, LoadState::Idle))), notice: Arc::default(),
            timing: Arc::default(), alive: Arc::default(), device_error: None,
        };
        engine.set_next(Some(("second".into(), Duration::from_secs(50), Box::new(move |_| Ok(wav_source(url))))));
        {
            let mut sh = engine.shared.lock().unwrap();
            monitor_step(&mut sh, &Effects::default(), &engine.effects, &engine.shared, &engine.next_generation);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while engine.shared.lock().unwrap().prepared.is_none() {
            assert!(Instant::now() < deadline, "song should open before the server sends the rest");
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut started_samples = 0;
        if transition == "next" {
            engine.load("second", Duration::from_secs(50), Box::new(|_| panic!("must use the prepared song")), false, egui::Context::default());
            while engine.load_state() != LoadState::Ready {
                assert!(Instant::now() < deadline, "prepared song should start");
                std::thread::sleep(Duration::from_millis(10));
            }
        } else {
            let fx = Effects { gapless: transition == "gapless", crossfade: if transition == "crossfade" { 1.0 } else { 0.0 }, ..Default::default() };
            let mut sh = engine.shared.lock().unwrap();
            sh.playing.as_mut().unwrap().duration = Duration::ZERO;
            monitor_step(&mut sh, &fx, &engine.effects, &engine.shared, &engine.next_generation);
            if transition == "gapless" {
                // Claim the first sample before checking the handoff. This works both
                // with queue-length detection and with actual source-start detection.
                assert!(output.next().unwrap() > 0.0);
                started_samples = 1;
            }
            monitor_step(&mut sh, &fx, &engine.effects, &engine.shared, &engine.next_generation);
            assert_eq!(sh.advanced.as_deref(), Some("second"));
        }
        assert!(engine.buffered() < 1.0);
        engine.set_next(Some(("third".into(), Duration::from_secs(50), Box::new(|_| Err("not needed".into())))));
        release.send(()).unwrap();
        let output = if transition == "crossfade" { other_output } else { output };
        let audible = started_samples + output.take(SAMPLES as usize + 8_000).filter(|s| *s > 0.0).count();
        server.join().unwrap();
        // A fade starts at zero until the processor's first 256-frame refresh.
        let expected = if transition == "crossfade" { SAMPLES as usize - 256 } else { SAMPLES as usize };
        assert!(audible >= expected, "{transition}: changing Next truncated the song to {audible} samples");
    }

}
