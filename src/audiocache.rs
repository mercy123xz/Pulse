//! Songs you've played, kept on disk for a while so the next time they start at once:
//! no search, no link to find, nothing to download. The least recently played go first
//! when the space fills up (Settings › Storage), and "Clear cache" removes them all.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

/// Most disk space for kept songs, in megabytes. 0 switches it off.
static LIMIT_MB: AtomicU64 = AtomicU64::new(1000);

pub fn set_limit_mb(mb: u64) {
    LIMIT_MB.store(mb, Ordering::Relaxed);
    if mb == 0 {
        let _ = std::fs::remove_dir_all(dir());
    }
}

pub fn enabled() -> bool {
    LIMIT_MB.load(Ordering::Relaxed) > 0
}

pub fn dir() -> PathBuf {
    crate::settings::data_dir().join("cache").join("audio")
}

/// Where song `id`'s audio is kept ("dz:123" → "dz_123.m4a").
pub fn path_for(id: &str, ext: &str) -> PathBuf {
    path_in(&dir(), id, ext)
}

fn path_in(dir: &Path, id: &str, ext: &str) -> PathBuf {
    let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).take(80).collect();
    // Different ids must never share a file, even when their safe names match.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    dir.join(format!("{safe}-{:08x}.{ext}", h as u32))
}

/// The kept copy of song `id`, if there is one. Marks it as just used.
pub fn find(id: &str) -> Option<PathBuf> {
    if !enabled() {
        return None;
    }
    find_in(&dir(), id)
}

fn find_in(dir: &Path, id: &str) -> Option<PathBuf> {
    let path = ["m4a", "mp3"].iter().map(|ext| path_in(dir, id, ext)).find(|p| p.is_file())?;
    if let Ok(f) = std::fs::File::options().append(true).open(&path) {
        let _ = f.set_modified(SystemTime::now());
    }
    Some(path)
}

/// Whether song `id` is kept (without marking it as used).
pub fn has(id: &str) -> bool {
    enabled() && ["m4a", "mp3"].iter().any(|ext| path_for(id, ext).is_file())
}

/// Keeps a fully downloaded song. Quietly does nothing if it can't be written.
pub fn store(path: &Path, bytes: &[u8]) {
    let limit = LIMIT_MB.load(Ordering::Relaxed) * 1024 * 1024;
    if limit == 0 || bytes.is_empty() || bytes.len() as u64 > limit / 2 {
        return;
    }
    store_with_limit(path, bytes, limit);
}

/// Keeps a song that was downloaded straight to a file (`from`), moving it to `path`.
/// The file is removed if it can't be kept.
pub fn adopt(from: &Path, path: &Path) -> bool {
    let limit = LIMIT_MB.load(Ordering::Relaxed) * 1024 * 1024;
    let size = std::fs::metadata(from).map(|m| m.len()).unwrap_or(0);
    let Some(folder) = path.parent() else { return false };
    let ok = limit > 0 && size > 0 && size <= limit / 2 && std::fs::create_dir_all(folder).is_ok() && std::fs::rename(from, path).is_ok();
    if ok {
        crate::artcache::trim_dir(folder, limit);
    } else {
        let _ = std::fs::remove_file(from);
    }
    ok
}

fn store_with_limit(path: &Path, bytes: &[u8], limit: u64) {
    let Some(folder) = path.parent() else { return };
    if std::fs::create_dir_all(folder).is_err() {
        return;
    }
    let part = path.with_extension("part");
    if std::fs::write(&part, bytes).is_ok() && std::fs::rename(&part, path).is_err() {
        let _ = std::fs::remove_file(&part);
    }
    crate::artcache::trim_dir(folder, limit);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_songs_finds_them_and_drops_the_oldest_when_full() {
        let dir = std::env::temp_dir().join(format!("pulse-audiocache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let song = vec![7u8; 400_000];
        // Room for two songs and a bit.
        for (n, id) in ["dz:1", "dz:2", "dz:3"].iter().enumerate() {
            store_with_limit(&path_in(&dir, id, "m4a"), &song, 1_000_000);
            // File times can be coarse: make sure "older" really is older.
            let t = SystemTime::now() - std::time::Duration::from_secs(100 - n as u64 * 10);
            if let Ok(f) = std::fs::File::options().append(true).open(path_in(&dir, id, "m4a")) {
                let _ = f.set_modified(t);
            }
        }
        store_with_limit(&path_in(&dir, "dz:4", "mp3"), &song, 1_000_000);
        assert!(find_in(&dir, "dz:1").is_none(), "the oldest went first");
        assert_eq!(std::fs::read(find_in(&dir, "dz:4").unwrap()).unwrap(), song);
        assert!(find_in(&dir, "nope").is_none());
        // Ids that look alike once cleaned up don't share a file.
        assert_ne!(path_in(&dir, "a:b", "m4a"), path_in(&dir, "a_b", "m4a"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
