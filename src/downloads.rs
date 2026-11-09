//! Downloading songs for offline listening.

use crate::deezer::Track;
use crate::settings::Quality;
use std::io::Write;
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

/// Downloads `t` from YouTube into `dir`. `progress` gets 0.0 – 1.0.
/// Returns the saved file and the YouTube video id.
pub fn download(t: &Track, known_video: Option<String>, quality: Quality, dir: &Path, progress: &dyn Fn(f32)) -> Result<(PathBuf, String), String> {
    let (src, video) = crate::youtube::resolve(t, known_video, quality, &|_| {})?;
    let ext = match src.codec {
        crate::audio::Codec::Mp3 => "mp3",
        _ => "m4a",
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {} ({e})", dir.display()))?;
    let path = dir.join(file_name(t, ext));
    let part = path.with_extension(format!("{ext}.part"));
    let mut file = std::fs::File::create(&part).map_err(|e| format!("Couldn't save the song ({e})"))?;

    // Same 4 MiB pieces as streaming: YouTube slows down single huge requests.
    const CHUNK: u64 = 4 * 1024 * 1024;
    let mut have = 0u64;
    let mut total = src.content_length;
    let mut retries = 0;
    loop {
        let end = total.map(|t| (have + CHUNK).min(t) - 1).or(Some(have + CHUNK - 1));
        match crate::net::get_range(&src.url, &src.headers, have, end) {
            Ok(mut r) => {
                total = total.or(r.total);
                let mut buf = vec![0u8; 64 * 1024];
                let mut got = 0u64;
                loop {
                    match std::io::Read::read(&mut r.body, &mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            file.write_all(&buf[..n]).map_err(|e| format!("Couldn't save the song ({e})"))?;
                            have += n as u64;
                            got += n as u64;
                            if let Some(t) = total {
                                progress(have as f32 / t.max(1) as f32);
                            }
                        }
                        Err(_) => break,
                    }
                }
                if total.is_some_and(|t| have >= t) || (!r.partial && got > 0) || got == 0 {
                    break;
                }
            }
            Err(e) => {
                retries += 1;
                if retries > 4 {
                    let _ = std::fs::remove_file(&part);
                    return Err(e);
                }
                std::thread::sleep(std::time::Duration::from_millis(500 * retries));
            }
        }
    }
    drop(file);
    if have == 0 {
        let _ = std::fs::remove_file(&part);
        return Err("The download was empty.".into());
    }
    std::fs::rename(&part, &path).map_err(|e| format!("Couldn't save the song ({e})"))?;
    progress(1.0);
    Ok((path, video))
}

#[cfg(test)]
mod tests {
    use super::*;

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
