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
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
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
    Loading,
    Ready(Arc<[u8]>),
    Failed(String),
}

pub struct ArtLoader {
    entries: Mutex<HashMap<String, Entry>>,
    jobs: Mutex<Sender<(String, egui::Context)>>,
}

impl ArtLoader {
    pub fn install(ctx: &egui::Context) -> Arc<ArtLoader> {
        let (tx, rx) = channel::<(String, egui::Context)>();
        let loader = Arc::new(ArtLoader { entries: Mutex::default(), jobs: Mutex::new(tx) });
        let rx = Arc::new(Mutex::new(rx));
        for n in 0..WORKERS {
            let rx = rx.clone();
            let weak = Arc::downgrade(&loader);
            std::thread::Builder::new()
                .name(format!("art-{n}"))
                .spawn(move || {
                    loop {
                        let job = rx.lock().unwrap().recv();
                        let Ok((uri, ctx)) = job else { return };
                        let result = fetch(&uri);
                        let Some(loader) = weak.upgrade() else { return };
                        let mut map = loader.entries.lock().unwrap();
                        // Forgotten while it downloaded: don't keep it.
                        if map.contains_key(&uri) {
                            map.insert(uri, match result {
                                Ok(b) => Entry::Ready(b.into()),
                                Err(e) => Entry::Failed(e),
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
}

/// The disk copy, else a download (saved for next time).
fn fetch(uri: &str) -> Result<Vec<u8>, String> {
    let path = file_for(uri);
    if let Ok(b) = std::fs::read(&path) {
        // Mark it used, so cleanup removes old ones first.
        if let Ok(f) = std::fs::File::options().append(true).open(&path) {
            let _ = f.set_modified(SystemTime::now());
        }
        return Ok(b);
    }
    let bytes = crate::net::download(uri, |_, _| {})?;
    if bytes.is_empty() {
        return Err("empty picture".into());
    }
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
        if !(uri.starts_with("https://") || uri.starts_with("http://")) {
            return Err(LoadError::NotSupported);
        }
        let mut map = self.entries.lock().unwrap();
        match map.get(uri) {
            Some(Entry::Ready(b)) => Ok(BytesPoll::Ready { size: None, bytes: Bytes::Shared(b.clone()), mime: mime_of(b) }),
            Some(Entry::Loading) => Ok(BytesPoll::Pending { size: None }),
            Some(Entry::Failed(e)) => Err(LoadError::Loading(e.clone())),
            None => {
                map.insert(uri.to_string(), Entry::Loading);
                let _ = self.jobs.lock().unwrap().send((uri.to_string(), ctx.clone()));
                Ok(BytesPoll::Pending { size: None })
            }
        }
    }

    fn forget(&self, uri: &str) {
        self.entries.lock().unwrap().remove(uri);
    }

    fn forget_all(&self) {
        self.entries.lock().unwrap().clear();
    }

    fn byte_size(&self) -> usize {
        self.entries
            .lock()
            .unwrap()
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
fn trim_dir(folder: &std::path::Path, limit: u64) {
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
