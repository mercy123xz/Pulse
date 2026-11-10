//! Downloading songs for offline listening.

use crate::deezer::Track;
use crate::settings::Quality;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

/// A file name that's safe on Windows: "Artist - Title [id].m4a".
pub fn file_name(t: &Track, ext: &str) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c })
            .collect::<String>()
            .trim()
            .trim_end_matches('.')
            .chars()
            .take(80)
            .collect()
    };
    let id: String = t.id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!("{} - {} [{id}].{ext}", clean(&t.artist.name), clean(&t.title))
}

/// Size of the pieces a song is fetched in, and how many are fetched at the same time.
/// YouTube slows down one long request, but not several short ones side by side.
const PIECE: u64 = 1024 * 1024;
const PARALLEL: usize = 4;

/// Downloads `t` into `dir`. `progress` gets 0.0 – 1.0.
/// Returns the saved file and the YouTube video id (empty if the song was already kept
/// from playing it, in which case nothing is downloaded at all).
pub fn download(t: &Track, known_video: Option<String>, quality: Quality, dir: &Path, progress: &(dyn Fn(f32) + Sync)) -> Result<(PathBuf, String), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {} ({e})", dir.display()))?;
    // Played recently and still kept: copy it, no download needed.
    if let Some(kept) = crate::audiocache::find(&t.id) {
        let ext = kept.extension().and_then(|x| x.to_str()).unwrap_or("m4a");
        let path = dir.join(file_name(t, ext));
        if std::fs::copy(&kept, &path).is_ok() {
            progress(1.0);
            return Ok((path, known_video.unwrap_or_default()));
        }
    }
    let (src, video) = crate::youtube::resolve(t, known_video, quality, &|_| {})?;
    let ext = match src.codec {
        crate::audio::Codec::Mp3 => "mp3",
        _ => "m4a",
    };
    let path = dir.join(file_name(t, ext));
    let part = path.with_extension(format!("{ext}.part"));
    let result = fetch(&src.url, &src.headers, src.content_length, &part, progress);
    match result {
        Ok(0) | Err(_) => {
            let _ = std::fs::remove_file(&part);
            Err(result.err().unwrap_or_else(|| "The download was empty.".into()))
        }
        Ok(_) => {
            std::fs::rename(&part, &path).map_err(|e| format!("Couldn't save the song ({e})"))?;
            progress(1.0);
            Ok((path, video))
        }
    }
}

/// Downloads `url` into the file `to`: several pieces side by side when the server allows
/// it, else one after another. Returns the number of bytes saved.
pub fn fetch(url: &str, headers: &[(String, String)], size_hint: Option<u64>, to: &Path, progress: &(dyn Fn(f32) + Sync)) -> Result<u64, String> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    let save = |e: std::io::Error| format!("Couldn't save the song ({e})");
    let mut file = std::fs::File::create(to).map_err(save)?;

    // The first piece also tells us the size, and whether pieces are possible at all.
    let mut first = with_retries(|| crate::net::get_range(url, headers, 0, Some(PIECE - 1)))?;
    let total = first.total.or(size_hint);
    let mut head = Vec::new();
    first.body.read_to_end(&mut head).map_err(|e| format!("Download interrupted ({e})"))?;
    file.write_all(&head).map_err(save)?;
    let have = head.len() as u64;
    let Some(total) = total.filter(|_| first.partial) else {
        // The whole file came in one go (or its size is a mystery): carry on in order.
        return if first.partial { sequential(url, headers, file, have, progress) } else { Ok(have) };
    };
    if have >= total {
        return Ok(have);
    }
    if have != PIECE.min(total) {
        return Err("Download interrupted (short piece).".into());
    }

    let starts: Vec<u64> = (PIECE..total).step_by(PIECE as usize).collect();
    let (next, done, failed) = (AtomicUsize::new(0), AtomicU64::new(have), AtomicBool::new(false));
    let error = Mutex::new(None::<String>);
    let file = Mutex::new(file);
    std::thread::scope(|s| {
        for _ in 0..PARALLEL.min(starts.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= starts.len() || failed.load(Ordering::SeqCst) {
                        return;
                    }
                    let (start, end) = (starts[i], (starts[i] + PIECE).min(total) - 1);
                    let piece = with_retries(|| {
                        let mut r = crate::net::get_range(url, headers, start, Some(end))?;
                        let mut v = Vec::with_capacity((end - start + 1) as usize);
                        r.body.read_to_end(&mut v).map_err(|e| format!("Download interrupted ({e})"))?;
                        if v.len() as u64 != end - start + 1 { Err("Download interrupted (short piece).".to_string()) } else { Ok(v) }
                    });
                    let written = piece.and_then(|v| {
                        let mut f = file.lock().unwrap();
                        f.seek(std::io::SeekFrom::Start(start)).and_then(|_| f.write_all(&v)).map_err(save)?;
                        Ok(v.len() as u64)
                    });
                    match written {
                        Ok(n) => progress(done.fetch_add(n, Ordering::SeqCst).saturating_add(n) as f32 / total as f32),
                        Err(e) => {
                            failed.store(true, Ordering::SeqCst);
                            error.lock().unwrap().get_or_insert(e);
                            return;
                        }
                    }
                }
            });
        }
    });
    match error.into_inner().unwrap() {
        Some(e) => Err(e),
        None => Ok(done.load(Ordering::SeqCst)),
    }
}

/// The rest of a file whose size isn't known, in 4 MB requests one after another.
fn sequential(url: &str, headers: &[(String, String)], mut file: std::fs::File, mut have: u64, progress: &(dyn Fn(f32) + Sync)) -> Result<u64, String> {
    const CHUNK: u64 = 4 * 1024 * 1024;
    loop {
        let mut r = match with_retries(|| crate::net::get_range(url, headers, have, Some(have + CHUNK - 1))) {
            Ok(r) => r,
            // Asking past the end is how the end shows itself.
            Err(_) if have > 0 => return Ok(have),
            Err(e) => return Err(e),
        };
        let mut v = Vec::new();
        r.body.read_to_end(&mut v).map_err(|e| format!("Download interrupted ({e})"))?;
        file.write_all(&v).map_err(|e| format!("Couldn't save the song ({e})"))?;
        have += v.len() as u64;
        if let Some(t) = r.total {
            progress(have as f32 / t.max(1) as f32);
            if have >= t {
                return Ok(have);
            }
        }
        if (v.len() as u64) < CHUNK {
            return Ok(have);
        }
    }
}

/// Tries up to five times, waiting a little longer each time.
fn with_retries<T>(mut f: impl FnMut() -> Result<T, String>) -> Result<T, String> {
    let mut tries = 0;
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) if tries >= 4 => return Err(e),
            Err(_) => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(400 * tries));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 6 MB file from a server that limits each connection (like YouTube): the pieces
    /// arrive side by side, much sooner than one connection could, and in the right places.
    #[test]
    fn downloads_in_pieces_side_by_side() {
        let data: Vec<u8> = (0..6_300_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
        let dir = std::env::temp_dir().join(format!("pulse-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let most = std::sync::Mutex::new(0.0f32);
        let note = |p: f32| {
            let mut m = most.lock().unwrap();
            *m = m.max(p);
        };

        let url = crate::audio::tests::slow_server(data.clone(), 1_000_000, true);
        let t0 = std::time::Instant::now();
        assert_eq!(fetch(&url, &[], None, &dir.join("a.part"), &note).unwrap(), data.len() as u64);
        let took = t0.elapsed().as_secs_f32();
        println!("6.3 MB at 1 MB/s per connection: {took:.1} s");
        assert!(took < 4.5, "one connection would need 6.3 s, took {took:.1} s");
        assert!(std::fs::read(dir.join("a.part")).unwrap() == data, "every byte in its place");
        assert!((*most.lock().unwrap() - 1.0).abs() < 1e-3);

        // A server that only sends whole files still works.
        let small = data[..300_000].to_vec();
        let url = crate::audio::tests::slow_server(small.clone(), 5_000_000, false);
        assert_eq!(fetch(&url, &[], None, &dir.join("b.part"), &note).unwrap(), small.len() as u64);
        assert!(std::fs::read(dir.join("b.part")).unwrap() == small);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_names_are_safe() {
        let t = Track {
            id: "dz:42".into(),
            title: "What? / Why: \"Yes\"".into(),
            artist: crate::deezer::ArtistRef { name: "AC/DC".into(), ..Default::default() },
            ..Default::default()
        };
        assert_eq!(file_name(&t, "m4a"), "AC_DC - What_ _ Why_ _Yes_ [dz42].m4a");
    }
}
