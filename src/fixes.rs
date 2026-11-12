//! Song fixes: a short list on Pulse's server of songs the catalog gets wrong. Deezer
//! lists some songs only as a 30-second clip ("Firework" by Katy Perry, in the US); the
//! list has their real length, so Pulse shows 3:47 instead of 0:30. A fix can also name
//! the YouTube video to play for a song that keeps finding the wrong one.
//!
//! Fixes are added by hand in Cloudflare (see server/README.md). The app only reads them,
//! once per song per session, and only for songs that need one (a clip-length song, or
//! the song about to play).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::deezer::Track;

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct Fix {
    /// The song's real length in seconds (0: no change).
    pub duration: u32,
    /// The YouTube video to play ("" or none: search as usual).
    pub video: Option<String>,
}

/// What's known per song id: a fix, or None for "asked, there's none".
static KNOWN: LazyLock<Mutex<HashMap<String, Option<Fix>>>> = LazyLock::new(Default::default);

/// Lengths learned by playing a song (its audio is the real length).
static PLAYED: LazyLock<Mutex<HashMap<String, u32>>> = LazyLock::new(Default::default);

/// A Deezer length this short is the clip, not the song.
pub fn is_clip(t: &Track) -> bool {
    t.id.starts_with("dz:") && t.duration <= 30
}

/// Asks the server about the songs not asked about yet. Errors are left for next time.
pub fn lookup(ids: &[String]) {
    let missing: Vec<String> = {
        let known = KNOWN.lock().unwrap();
        let mut m: Vec<String> = ids.iter().filter(|id| !known.contains_key(*id)).cloned().collect();
        m.sort();
        m.dedup();
        m
    };
    if missing.is_empty() || cfg!(test) {
        return;
    }
    for chunk in missing.chunks(100) {
        if let Ok(found) = crate::account::song_fixes(chunk) {
            let mut known = KNOWN.lock().unwrap();
            for id in chunk {
                known.insert(id.clone(), found.get(id).cloned());
            }
        }
    }
}

/// Fixes the lengths of songs Deezer lists as a clip (fetched songs, before they're shown).
/// Without a fix the length is left blank (0) rather than a wrong 0:30.
pub fn apply(tracks: &mut [Track]) {
    let ids: Vec<String> = tracks.iter().filter(|t| is_clip(t)).map(|t| t.id.clone()).collect();
    if ids.is_empty() {
        return;
    }
    lookup(&ids);
    for t in tracks.iter_mut().filter(|t| is_clip(t)) {
        t.duration = known_length(&t.id).unwrap_or(0);
    }
}

fn known_length(id: &str) -> Option<u32> {
    if let Some(&s) = PLAYED.lock().unwrap().get(id) {
        return Some(s);
    }
    KNOWN.lock().unwrap().get(id).cloned().flatten().map(|f| f.duration).filter(|&d| d > 0)
}

/// The length to show for a song: its own, or for a clip-length song the real one if it's
/// known (else 0: shown blank). Never asks the server, so it's fine to call while drawing.
pub fn shown_length(t: &Track) -> u32 {
    if is_clip(t) { known_length(&t.id).unwrap_or(0) } else { t.duration }
}

/// Remembers a song's real length once its audio is open.
pub fn played(id: &str, secs: u32) {
    PLAYED.lock().unwrap().insert(id.to_string(), secs);
}

/// The video a fix names for this song, if any.
pub fn video(id: &str) -> Option<String> {
    lookup(&[id.to_string()]);
    KNOWN.lock().unwrap().get(id).cloned().flatten().and_then(|f| f.video).filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_lengths_are_blank_until_known() {
        let mut t = vec![Track { id: "dz:1".into(), duration: 30, ..Default::default() }, Track { id: "dz:2".into(), duration: 227, ..Default::default() }];
        assert_eq!(shown_length(&t[0]), 0);
        KNOWN.lock().unwrap().insert("dz:1".into(), Some(Fix { duration: 227, video: None }));
        apply(&mut t);
        assert_eq!((t[0].duration, t[1].duration), (227, 227));
        let clip = Track { id: "dz:3".into(), duration: 30, ..Default::default() };
        played("dz:3", 240);
        assert_eq!(shown_length(&clip), 240);
    }
}
