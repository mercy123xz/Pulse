//! The audio engine.
//!
//! Playing a song has two steps, both on a background thread:
//! 1. *Resolve*: work out where the audio is (for Pulse: find the song on YouTube).
//! 2. *Stream*: download it in chunks into a shared buffer while the decoder plays
//!    from that buffer, waiting briefly whenever it catches up with the download.

use rodio::{Decoder, MixerDeviceSink, Player};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicU64, Ordering};
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
}

#[derive(Debug, Clone)]
pub struct StreamSource {
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

    let reader = StreamReader { buf, pos: 0 };
    let (hint, mime) = match src.codec {
        Codec::Mp3 => ("mp3", "audio/mpeg"),
        Codec::Mp4 => ("m4a", "audio/mp4"),
    };
    let mut builder = Decoder::builder()
        .with_data(reader)
        .with_hint(hint)
        .with_mime_type(mime)
        .with_seekable(true)
        .with_coarse_seek(total.is_some() && src.codec == Codec::Mp3);
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

pub struct AudioEngine {
    // The output device must stay alive for as long as we want sound.
    _device: Option<MixerDeviceSink>,
    player: Option<Arc<Player>>,
    /// Bumped every time a new song is requested, so a slow download for an
    /// old song can never start playing over a newer one.
    generation: Arc<AtomicU64>,
    load_state: Arc<Mutex<(u64, LoadState)>>,
    /// How much of the current song has downloaded (0.0 – 1.0).
    buffered: Arc<Mutex<f32>>,
    notice: Arc<Mutex<Option<String>>>,
    pub device_error: Option<String>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let (device, player, device_error) = match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut device) => {
                device.log_on_drop(false);
                let player = Arc::new(Player::connect_new(device.mixer()));
                (Some(device), Some(player), None)
            }
            Err(e) => (None, None, Some(format!("No audio output device found ({e})"))),
        };
        Self {
            _device: device,
            player,
            generation: Arc::new(AtomicU64::new(0)),
            load_state: Arc::new(Mutex::new((0, LoadState::Idle))),
            buffered: Arc::new(Mutex::new(0.0)),
            notice: Arc::new(Mutex::new(None)),
            device_error,
        }
    }

    /// Starts a song. Returns immediately; finding and streaming the audio happen
    /// on a background thread. `ctx` is used to wake up the UI.
    pub fn load(&self, resolve: Resolver, start_paused: bool, ctx: egui::Context) {
        let Some(player) = self.player.clone() else { return };
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        player.clear();
        *self.load_state.lock().unwrap() = (generation, LoadState::Resolving("Getting ready…".into()));
        *self.buffered.lock().unwrap() = 0.0;

        let gen_counter = self.generation.clone();
        let load_state = self.load_state.clone();
        let buffered = self.buffered.clone();
        let notice = self.notice.clone();

        std::thread::spawn(move || {
            let set_state = |s: LoadState| {
                let mut ls = load_state.lock().unwrap();
                if ls.0 == generation {
                    ls.1 = s;
                }
                ctx.request_repaint();
            };
            let current = || gen_counter.load(Ordering::SeqCst) == generation;

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
            let decoder = match open_streaming_decoder(&src, still_wanted, buffered) {
                Ok(d) => d,
                Err(e) => return set_state(LoadState::Failed(e)),
            };
            if !current() {
                return;
            }
            if let Some(n) = src.notice {
                *notice.lock().unwrap() = Some(n);
            }
            player.clear();
            player.append(decoder);
            if start_paused {
                player.pause();
            } else {
                player.play();
            }
            set_state(LoadState::Ready);
        });
    }

    pub fn load_state(&self) -> LoadState {
        let ls = self.load_state.lock().unwrap();
        if ls.0 == self.generation.load(Ordering::SeqCst) { ls.1.clone() } else { LoadState::Loading }
    }

    /// A message from the last song start , shown once.
    pub fn take_notice(&self) -> Option<String> {
        self.notice.lock().unwrap().take()
    }

    pub fn buffered(&self) -> f32 {
        *self.buffered.lock().unwrap()
    }

    pub fn play(&self) {
        if let Some(p) = &self.player {
            p.play();
        }
    }

    pub fn pause(&self) {
        if let Some(p) = &self.player {
            p.pause();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.player.as_ref().is_none_or(|p| p.is_paused())
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(p) = &self.player {
            p.clear();
        }
        *self.load_state.lock().unwrap() = (self.generation.load(Ordering::SeqCst), LoadState::Idle);
    }

    pub fn set_volume(&self, v: f32) {
        if let Some(p) = &self.player {
            // Perceptual curve: the slider feels linear to the ear.
            p.set_volume(v * v);
        }
    }

    pub fn position(&self) -> Duration {
        self.player.as_ref().map(|p| p.get_pos()).unwrap_or_default()
    }

    pub fn seek(&self, to: Duration) {
        if let Some(p) = &self.player {
            let _ = p.try_seek(to);
        }
    }

    /// True when the current song has played to the end.
    pub fn finished(&self) -> bool {
        self.load_state() == LoadState::Ready && self.player.as_ref().is_some_and(|p| p.empty())
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
