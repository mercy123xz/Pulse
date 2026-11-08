//! The audio engine.
//!
//! Playback starts while the song is still downloading: a background thread
//! keeps appending bytes to a shared buffer, and the decoder reads from that
//! buffer, waiting briefly whenever it catches up with the download.

use rodio::{Decoder, MixerDeviceSink, Player};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

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
struct StreamReader {
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

/// Opens `url` and returns a decoder that can start playing right away while
/// the rest of the file keeps downloading in the background.
/// `still_wanted` returns false once the user has moved on to another song.
fn open_streaming_decoder(
    url: &str,
    still_wanted: impl Fn() -> bool + Send + 'static,
    buffered: Arc<Mutex<f32>>,
) -> Result<Decoder<StreamReader>, String> {
    let (mut body, total) = crate::api::open_stream(url, 0)?;
    if !still_wanted() {
        return Err("cancelled".into());
    }
    let buf = Arc::new(StreamBuffer::default());
    buf.state.lock().unwrap().total = total;

    // Download thread: keeps filling the buffer.
    {
        let buf = buf.clone();
        let url = url.to_owned();
        std::thread::spawn(move || {
            let mut chunk = vec![0u8; 64 * 1024];
            let mut retries = 0;
            loop {
                if buf.state.lock().unwrap().cancelled || !still_wanted() {
                    break;
                }
                match body.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut st = buf.state.lock().unwrap();
                        st.bytes.extend_from_slice(&chunk[..n]);
                        if let Some(t) = st.total {
                            *buffered.lock().unwrap() = st.bytes.len() as f32 / t.max(1) as f32;
                        }
                        drop(st);
                        buf.changed.notify_all();
                    }
                    Err(_) if retries < 3 => {
                        // Connection dropped: pick up where we left off.
                        retries += 1;
                        std::thread::sleep(std::time::Duration::from_millis(500 * retries));
                        let have = buf.state.lock().unwrap().bytes.len() as u64;
                        match crate::api::open_stream(&url, have) {
                            Ok((b, _)) => body = b,
                            Err(_) => continue,
                        }
                    }
                    Err(_) => break,
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
    let mut builder = Decoder::builder()
        .with_data(reader)
        .with_hint("mp3")
        .with_mime_type("audio/mpeg")
        .with_seekable(true)
        .with_coarse_seek(total.is_some());
    if let Some(t) = total {
        builder = builder.with_byte_len(t);
    }
    builder.build().map_err(|e| format!("Couldn't decode this track ({e})"))
}

// ---------------------------------------------------------------------------
// The engine the UI talks to.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum LoadState {
    Idle,
    Loading,
    Ready,
    Failed(String),
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
            device_error,
        }
    }

    /// Starts streaming a song. Returns immediately; the download and decoder
    /// setup happen on a background thread. `ctx` is used to wake up the UI.
    pub fn load(&self, url: String, start_paused: bool, ctx: egui::Context) {
        let Some(player) = self.player.clone() else { return };
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        player.clear();
        *self.load_state.lock().unwrap() = (generation, LoadState::Loading);
        *self.buffered.lock().unwrap() = 0.0;

        let gen_counter = self.generation.clone();
        let load_state = self.load_state.clone();
        let buffered = self.buffered.clone();

        std::thread::spawn(move || {
            let set_state = |s: LoadState| {
                let mut ls = load_state.lock().unwrap();
                if ls.0 == generation {
                    ls.1 = s;
                }
                ctx.request_repaint();
            };

            let still_wanted = {
                let gen_counter = gen_counter.clone();
                move || gen_counter.load(Ordering::SeqCst) == generation
            };
            let decoder = match open_streaming_decoder(&url, still_wanted, buffered) {
                Ok(d) => d,
                Err(e) => return set_state(LoadState::Failed(e)),
            };

            if gen_counter.load(Ordering::SeqCst) != generation {
                return;
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
    fn slow_server(data: Vec<u8>, bytes_per_sec: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut s = stream.unwrap();
                let data = data.clone();
                std::thread::spawn(move || {
                    let mut req = [0u8; 2048];
                    let _ = std::io::Read::read(&mut s, &mut req);
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len()
                    );
                    s.write_all(head.as_bytes()).unwrap();
                    let chunk = bytes_per_sec / 20;
                    for part in data.chunks(chunk) {
                        if s.write_all(part).is_err() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                });
            }
        });
        format!("http://{addr}/track.mp3")
    }

    #[test]
    fn plays_while_downloading_and_can_seek() {
        let Ok(data) = std::fs::read("/tmp/test.mp3") else {
            eprintln!("skipping: /tmp/test.mp3 not found");
            return;
        };
        let total = data.len();
        // ~400 KB/s, so the 1.2 MB file takes ~3 s to download.
        let url = slow_server(data, 400_000);
        let buffered = Arc::new(Mutex::new(0.0));

        let t0 = Instant::now();
        let mut dec = open_streaming_decoder(&url, || true, buffered.clone()).expect("decoder");
        let ready_after = t0.elapsed();
        let first: Vec<_> = (&mut dec).take(44_100 * 2).collect(); // 1 s of stereo audio
        assert_eq!(first.len(), 44_100 * 2, "decoded the first second");
        println!("decoder ready after {ready_after:?}, buffered {:.0}%", *buffered.lock().unwrap() * 100.0);
        assert!(ready_after < Duration::from_secs(2), "playback should start before the download finishes");
        assert!(*buffered.lock().unwrap() < 1.0, "download still running when playback starts");

        // Seek past what has downloaded so far; this waits for the data, then continues.
        dec.try_seek(Duration::from_secs(20)).expect("seek");
        let after: Vec<_> = (&mut dec).take(1000).collect();
        assert_eq!(after.len(), 1000);
        let rest = dec.count();
        println!("samples after seek: {rest} (~{:.1} s)", rest as f32 / 88_200.0);
        assert!(rest > 88_200 * 8 && rest < 88_200 * 11, "about 10 s should remain after seeking to 20 s of 30 s");
        assert!((*buffered.lock().unwrap() - 1.0).abs() < 1e-6, "download complete ({total} bytes)");
    }
}
