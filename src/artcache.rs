//! Pictures (covers, artist photos): kept on disk so the same picture is never
//! downloaded twice, and let go of in memory once they're off screen for a while.
//!
//! egui keeps every picture it has shown (the file, the decoded pixels and the GPU
//! copy) until told to forget it, so browsing hundreds of albums would keep growing.
//! Here each picture drawn is noted, and pictures not drawn for a minute are
//! forgotten (they come back quickly from the disk copy if needed again). The disk
//! copies stay under a size limit, oldest-used removed first.

use egui::load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, LoadError};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// How many pictures download at the same time.
const WORKERS: usize = 4;
/// Pictures not drawn for this long are let go of in memory.
const UNUSED_FOR: Duration = Duration::from_secs(60);

pub fn dir() -> PathBuf {
    crate::settings::data_dir().join("cache").join("art")
}

fn file_for(uri: &str) -> PathBuf {
    // FNV-1a: a stable short name for the address.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in uri.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    dir().join(format!("{h:016x}.img"))
}

enum Entry {
    /// Waiting for a worker, since this moment.
    Loading(Instant),
    Ready(Arc<[u8]>),
    /// Couldn't get it at this moment; tried again after `RETRY_AFTER`.
    Failed(String, Instant),
}

/// A failed picture (network hiccup, server busy) is tried again after this long.
const RETRY_AFTER: Duration = Duration::from_secs(15);
/// A picture still not loaded after this long is asked for again.
const STUCK_AFTER: Duration = Duration::from_secs(45);

/// Pictures waiting for a worker. The newest is fetched first: after clicking through a
/// few pages, the covers on screen now come before the ones of pages already left.
#[derive(Default)]
struct Queue {
    jobs: Mutex<Vec<(String, egui::Context)>>,
    ready: Condvar,
}

pub struct ArtLoader {
    entries: Mutex<HashMap<String, Entry>>,
    queue: Arc<Queue>,
}

impl ArtLoader {
    pub fn install(ctx: &egui::Context) -> Arc<ArtLoader> {
        let queue = Arc::new(Queue::default());
        let loader = Arc::new(ArtLoader { entries: Mutex::default(), queue: queue.clone() });
        for n in 0..WORKERS {
            let queue = queue.clone();
            let weak = Arc::downgrade(&loader);
            std::thread::Builder::new()
                .name(format!("art-{n}"))
                .spawn(move || {
                    loop {
                        let (uri, ctx) = {
                            let mut jobs = queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
                            while jobs.is_empty() {
                                jobs = queue.ready.wait(jobs).unwrap_or_else(|e| e.into_inner());
                            }
                            jobs.pop().expect("not empty")
                        };
                        let Some(loader) = weak.upgrade() else { return };
                        // Let go of while it waited (scrolled away long ago): skip it.
                        if !matches!(loader.entries().get(&uri), Some(Entry::Loading(_))) {
                            continue;
                        }
                        // A broken picture must not take a worker down with it.
                        let result = std::panic::catch_unwind(|| fetch(&uri)).unwrap_or_else(|_| Err("unreadable picture".into()));
                        let mut map = loader.entries();
                        // Forgotten while it downloaded: don't keep it.
                        if map.contains_key(&uri) {
                            map.insert(uri, match result {
                                Ok(b) => Entry::Ready(b.into()),
                                Err(e) => Entry::Failed(e, Instant::now()),
                            });
                        }
                        drop(map);
                        ctx.request_repaint();
                    }
                })
                .ok();
        }
        ctx.add_bytes_loader(loader.clone());
        loader
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn request(&self, uri: &str, ctx: &egui::Context) {
        let mut jobs = self.queue.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.retain(|(u, _)| u != uri);
        jobs.push((uri.to_string(), ctx.clone()));
        drop(jobs);
        self.queue.ready.notify_one();
    }
}

/// No picture is drawn bigger than this many pixels a side (covers are shown at 320 at most).
const MAX_SIDE: u32 = 1280;
/// Pictures with more pixels than this aren't opened at all (decoding them needs gigabytes).
const MAX_PIXELS: u64 = 120_000_000;

/// Makes sure a picture is safe to draw: a very large one is made smaller first.
/// A graphics card takes pictures only up to a certain size (8192 pixels a side with
/// DirectX), and a bigger one used to close Pulse.
pub fn fit(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let dims = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok());
    let Some((w, h)) = dims else { return Ok(bytes) };
    if w.max(h) <= MAX_SIDE {
        return Ok(bytes);
    }
    if too_many_pixels(w, h) {
        return Err(format!("picture too large ({w}×{h})"));
    }
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.thumbnail(MAX_SIDE, MAX_SIDE);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn too_many_pixels(w: u32, h: u32) -> bool {
    w as u64 * h as u64 > MAX_PIXELS
}

/// The file a `file://` address points at ("file:///C:/Users/…" on Windows, "file:///home/…" elsewhere).
fn local_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let b = rest.as_bytes();
    // "/C:/…": drop the slash before the drive letter.
    let rest = if b.len() > 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' { &rest[1..] } else { rest };
    Some(PathBuf::from(rest))
}

/// The disk copy, else a download (saved for next time).
fn fetch(uri: &str) -> Result<Vec<u8>, String> {
    // Your own pictures (playlist covers, profile picture) are read where they are.
    if let Some(file) = local_path(uri) {
        return fit(std::fs::read(&file).map_err(|e| e.to_string())?);
    }
    let path = file_for(uri);
    if let Ok(b) = std::fs::read(&path) {
        // Mark it used, so cleanup removes old ones first.
        if let Ok(f) = std::fs::File::options().append(true).open(&path) {
            let _ = f.set_modified(SystemTime::now());
        }
        return fit(b);
    }
    let bytes = crate::net::download(uri, |_, _| {})?;
    if bytes.is_empty() {
        return Err("empty picture".into());
    }
    let bytes = fit(bytes)?;
    let _ = std::fs::create_dir_all(dir());
    let tmp = path.with_extension("part");
    if std::fs::write(&tmp, &bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
    Ok(bytes)
}

fn mime_of(b: &[u8]) -> Option<String> {
    let m = if b.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if b.starts_with(b"\x89PNG") {
        "image/png"
    } else if b.len() > 12 && &b[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return None;
    };
    Some(m.to_string())
}

impl BytesLoader for ArtLoader {
    fn id(&self) -> &str {
        "pulse::ArtLoader"
    }

    fn load(&self, ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        if !(uri.starts_with("https://") || uri.starts_with("http://") || uri.starts_with("file://")) {
            return Err(LoadError::NotSupported);
        }
        let mut map = self.entries();
        match map.get(uri) {
            Some(Entry::Ready(b)) => return Ok(BytesPoll::Ready { size: None, bytes: Bytes::Shared(b.clone()), mime: mime_of(b) }),
            Some(Entry::Loading(since)) if since.elapsed() < STUCK_AFTER => return Ok(BytesPoll::Pending { size: None }),
            Some(Entry::Failed(e, at)) if at.elapsed() < RETRY_AFTER => {
                // Look again once it's time to try again.
                ctx.request_repaint_after(RETRY_AFTER);
                return Err(LoadError::Loading(e.clone()));
            }
            // Not asked for yet, failed a while ago, or stuck: (re)fetch it.
            _ => {}
        }
        map.insert(uri.to_string(), Entry::Loading(Instant::now()));
        drop(map);
        self.request(uri, ctx);
        Ok(BytesPoll::Pending { size: None })
    }

    fn forget(&self, uri: &str) {
        self.entries().remove(uri);
    }

    fn forget_all(&self) {
        self.entries().clear();
    }

    fn byte_size(&self) -> usize {
        self.entries()
            .values()
            .map(|e| match e {
                Entry::Ready(b) => b.len(),
                _ => 0,
            })
            .sum()
    }
}

// ---------------------------------------------------------------------------
// Letting go of pictures that are off screen
// ---------------------------------------------------------------------------

fn last_drawn() -> &'static Mutex<HashMap<String, Instant>> {
    static M: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

/// Notes that a picture was drawn now.
pub fn drawn(uri: &str) {
    let now = Instant::now();
    let mut m = last_drawn().lock().unwrap();
    match m.get_mut(uri) {
        Some(at) => *at = now,
        None => {
            m.insert(uri.to_string(), now);
        }
    }
}

/// Forgets pictures that haven't been drawn for a minute (call now and then).
pub fn sweep(ctx: &egui::Context) {
    let stale: Vec<String> = {
        let mut m = last_drawn().lock().unwrap();
        let stale: Vec<String> = m.iter().filter(|(_, at)| at.elapsed() > UNUSED_FOR).map(|(u, _)| u.clone()).collect();
        for u in &stale {
            m.remove(u);
        }
        stale
    };
    for u in stale {
        ctx.forget_image(&u);
    }
}

// ---------------------------------------------------------------------------
// Disk limit
// ---------------------------------------------------------------------------

/// The disk copies' size limit, in megabytes (Settings › Storage).
static LIMIT_MB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(300);

pub fn set_limit_mb(mb: u64) {
    LIMIT_MB.store(mb.max(20), std::sync::atomic::Ordering::Relaxed);
}

/// Keeps the picture folder under the limit.
pub fn trim_disk() {
    trim_dir(&dir(), LIMIT_MB.load(std::sync::atomic::Ordering::Relaxed) * 1024 * 1024);
}

/// Removes the least recently used files until the folder is under 80% of `limit` bytes.
pub fn trim_dir(folder: &std::path::Path, limit: u64) {
    let Ok(read) = std::fs::read_dir(folder) else { return };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = read
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= limit {
        return;
    }
    files.sort_by_key(|f| f.0);
    for (_, len, path) in files {
        if total <= limit * 8 / 10 {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_types_are_recognized() {
        assert_eq!(mime_of(&[0xFF, 0xD8, 0xFF]).as_deref(), Some("image/jpeg"));
        assert_eq!(mime_of(b"\x89PNG\r\n").as_deref(), Some("image/png"));
        assert_eq!(mime_of(b"RIFF\0\0\0\0WEBPVP8 ").as_deref(), Some("image/webp"));
        assert_eq!(mime_of(b"<html>"), None);
    }

    #[test]
    fn the_same_address_always_gets_the_same_file() {
        assert_eq!(file_for("https://a/b.jpg"), file_for("https://a/b.jpg"));
        assert_ne!(file_for("https://a/b.jpg"), file_for("https://a/c.jpg"));
    }

    #[test]
    fn old_pictures_go_first_when_over_the_limit() {
        let tmp = std::env::temp_dir().join(format!("pulse-art-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let old = tmp.join("old.img");
        let new = tmp.join("new.img");
        std::fs::write(&old, vec![0u8; 1200]).unwrap();
        std::fs::write(&new, vec![0u8; 1200]).unwrap();
        let f = std::fs::File::options().append(true).open(&old).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
        trim_dir(&tmp, 2000);
        assert!(!old.exists() && new.exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(vec![]);
        image::RgbImage::new(w, h).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn size(b: &[u8]) -> (u32, u32) {
        image::ImageReader::new(std::io::Cursor::new(b)).with_guessed_format().unwrap().into_dimensions().unwrap()
    }

    #[test]
    fn big_pictures_are_made_smaller_and_others_left_alone() {
        let small = png(600, 600);
        assert_eq!(fit(small.clone()).unwrap(), small);
        assert_eq!(size(&fit(png(9000, 4500)).unwrap()), (1280, 640));
        assert_eq!(size(&fit(png(300, 17000)).unwrap()).1, 1280);
        // Not a picture: passed on as it is (it just won't show).
        assert_eq!(fit(b"<html>".to_vec()).unwrap(), b"<html>");
    }

    #[test]
    fn a_picture_that_claims_to_be_enormous_is_refused() {
        assert!(!too_many_pixels(9000, 9000));
        assert!(too_many_pixels(60000, 60000));
    }

    #[test]
    fn file_addresses_become_paths() {
        assert_eq!(local_path("file:///C:/Users/a/x.png").unwrap(), PathBuf::from("C:/Users/a/x.png"));
        assert_eq!(local_path("file:///home/a/x.png").unwrap(), PathBuf::from("/home/a/x.png"));
        assert!(local_path("https://x/y.png").is_none());
    }
}
