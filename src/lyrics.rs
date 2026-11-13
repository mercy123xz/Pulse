//! Song lyrics from LRCLIB (lrclib.net): a free, open lyrics database made for music
//! players. No key needed. Many songs have time-synced lyrics (each line with the
//! moment it's sung), so the current line can be highlighted; others have plain text.
//!
//! Lyrics are only asked for when you open them, and every answer (including "none
//! found") is kept on disk, so a song's lyrics are fetched once.

use crate::deezer::Track;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const API: &str = "https://lrclib.net/api";
/// "Not found" is asked again after a week (LRCLIB keeps adding songs).
const RETRY_MISSING_AFTER: u64 = 7 * 24 * 3600;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lyrics {
    /// (seconds into the song, line), in order. Empty when only plain lyrics exist.
    pub synced: Vec<(f32, String)>,
    pub plain: Option<String>,
    pub instrumental: bool,
    /// The Genius page, when the lyrics came from Genius (LRCLIB otherwise).
    #[serde(default)]
    pub genius: Option<String>,
}

impl Lyrics {
    pub fn is_empty(&self) -> bool {
        self.synced.is_empty() && self.plain.as_deref().is_none_or(|p| p.trim().is_empty()) && !self.instrumental
    }

    /// The line being sung at `secs` (its index), if the lyrics are synced.
    pub fn line_at(&self, secs: f32) -> Option<usize> {
        let i = self.synced.partition_point(|(t, _)| *t <= secs);
        i.checked_sub(1)
    }

    /// When the next line starts after `secs`, for waking up exactly then.
    pub fn next_change(&self, secs: f32) -> Option<f32> {
        self.synced.iter().map(|(t, _)| *t).find(|t| *t > secs)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    #[serde(default)]
    synced_lyrics: Option<String>,
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    duration: f32,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    /// When it was looked up (seconds since 1970).
    at: u64,
    /// 2 once Genius was asked too; older "not found" answers are asked again.
    #[serde(default)]
    v: u32,
    lyrics: Option<Lyrics>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn dir() -> PathBuf {
    crate::settings::data_dir().join("cache").join("lyrics")
}

fn file_for(t: &Track) -> PathBuf {
    let key = format!("{}|{}|{}", t.id, t.title, t.artist.name);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    dir().join(format!("{h:016x}.json"))
}

/// The song's lyrics: from the disk copy if there is one, else from LRCLIB.
/// `Ok(None)` means LRCLIB doesn't have them.
pub fn get(t: &Track) -> Result<Option<Lyrics>, String> {
    let path = file_for(t);
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(saved) = serde_json::from_str::<Saved>(&text)
        && (saved.lyrics.is_some() || (saved.v >= 2 && now().saturating_sub(saved.at) < RETRY_MISSING_AFTER))
    {
        return Ok(saved.lyrics);
    }
    let found = fetch(t)?;
    let _ = std::fs::create_dir_all(dir());
    if let Ok(json) = serde_json::to_string(&Saved { at: now(), v: 2, lyrics: found.clone() }) {
        let _ = std::fs::write(&path, json);
    }
    Ok(found)
}

fn fetch(t: &Track) -> Result<Option<Lyrics>, String> {
    // "Baby (feat. DaBaby)" is listed as "Baby".
    let title = crate::youtube::core_title(&t.title);
    let title = if title.trim().is_empty() { t.title.clone() } else { title };
    let artist = t.artist.name.clone();
    let duration = t.duration.to_string();
    let album = t.album.as_ref().map(|a| a.title.clone()).unwrap_or_default();

    // 1. Exact match: title, artist, album and length (LRCLIB allows ±2 seconds).
    let mut q: Vec<(&str, &str)> = vec![("track_name", &title), ("artist_name", &artist)];
    if !album.is_empty() {
        q.push(("album_name", &album));
    }
    if t.duration > 0 {
        q.push(("duration", &duration));
    }
    match request::<Record>("/get", &q) {
        Ok(Some(r)) => return Ok(Some(from_record(r))),
        Ok(None) => {}
        Err(e) => return Err(e),
    }
    // 2. Search by title and artist; take the closest length.
    let list = request::<Vec<Record>>("/search", &[("track_name", &title), ("artist_name", &artist)])?.unwrap_or_default();
    let best = list
        .into_iter()
        .filter(|r| r.synced_lyrics.is_some() || r.plain_lyrics.is_some() || r.instrumental)
        .filter(|r| t.duration == 0 || r.duration == 0.0 || (r.duration - t.duration as f32).abs() <= 8.0)
        .min_by(|a, b| {
            let d = |r: &Record| (r.duration - t.duration as f32).abs() - if r.synced_lyrics.is_some() { 3.0 } else { 0.0 };
            d(a).total_cmp(&d(b))
        });
    if let Some(r) = best {
        return Ok(Some(from_record(r)));
    }
    // 3. Genius, for songs LRCLIB doesn't have (plain text only).
    Ok(crate::genius::lyrics(&artist, &t.title).map(|(text, url)| Lyrics { plain: Some(text), genius: Some(url), ..Default::default() }))
}

/// One LRCLIB request; `Ok(None)` when it has nothing.
fn request<T: for<'de> Deserialize<'de>>(path: &str, query: &[(&str, &str)]) -> Result<Option<T>, String> {
    let mut req = crate::net::api().get(format!("{API}{path}"));
    for (k, v) in query {
        req = req.query(*k, *v);
    }
    match req.call() {
        Ok(mut resp) => resp.body_mut().read_json::<T>().map(Some).map_err(|e| format!("Couldn't read the lyrics ({e})")),
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(crate::net::friendly_error(e)),
    }
}

fn from_record(r: Record) -> Lyrics {
    Lyrics { synced: r.synced_lyrics.as_deref().map(parse_lrc).unwrap_or_default(), plain: r.plain_lyrics.filter(|p| !p.trim().is_empty()), instrumental: r.instrumental, genius: None }
}

/// Reads LRC text: "[01:02.50]Line" (a line can have several times: "[00:10][01:10]Chorus").
pub fn parse_lrc(text: &str) -> Vec<(f32, String)> {
    let mut out = vec![];
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut times = vec![];
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            match parse_time(tag) {
                Some(t) => times.push(t),
                // A tag like [ar:Artist]: not a lyric line.
                None => {
                    times.clear();
                    rest = "";
                    break;
                }
            }
            rest = &stripped[end + 1..];
        }
        let line = rest.trim().to_string();
        for t in times {
            out.push((t, line.clone()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn parse_time(tag: &str) -> Option<f32> {
    let (m, s) = tag.split_once(':')?;
    let m: f32 = m.trim().parse().ok()?;
    let s: f32 = s.trim().parse().ok()?;
    (m >= 0.0 && (0.0..60.0).contains(&s)).then_some(m * 60.0 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_lines_and_repeated_times_are_read() {
        let l = parse_lrc("[ar:Someone]\n[00:01.50]First\n[00:10.00][00:30.00]Chorus\n[00:20.25]\n[bad]text");
        assert_eq!(l, vec![(1.5, "First".into()), (10.0, "Chorus".into()), (20.25, String::new()), (30.0, "Chorus".into())]);
    }

    #[test]
    fn the_current_line_follows_the_time() {
        let lyr = Lyrics { synced: parse_lrc("[00:05]a\n[00:10]b\n[00:15]c"), ..Default::default() };
        assert_eq!(lyr.line_at(2.0), None);
        assert_eq!(lyr.line_at(5.0), Some(0));
        assert_eq!(lyr.line_at(12.0), Some(1));
        assert_eq!(lyr.line_at(99.0), Some(2));
        assert_eq!(lyr.next_change(12.0), Some(15.0));
        assert_eq!(lyr.next_change(20.0), None);
    }
}
