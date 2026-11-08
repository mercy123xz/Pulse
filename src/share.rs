//! Share codes: a whole playlist packed into one line of text, like
//! `pulse1:eJx…`. Send it to a friend; they paste it into Import playlist.
//!
//! Songs from Deezer are stored as just their number (Pulse fetches the rest when
//! importing), so codes stay short. Other songs carry their title, artist and length.

use crate::deezer::{ArtistRef, Track};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

pub const PREFIX: &str = "pulse1:";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shared {
    /// Playlist name.
    pub n: String,
    /// Who shared it (their profile name).
    #[serde(default)]
    pub o: String,
    pub t: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    /// A Deezer song number.
    Deezer(u64),
    /// Any other song.
    Song {
        t: String,
        a: String,
        #[serde(default)]
        d: u32,
        #[serde(default)]
        c: Option<String>,
    },
}

pub fn is_code(s: &str) -> bool {
    s.trim().starts_with(PREFIX)
}

pub fn encode(name: &str, owner: &str, tracks: &[Track]) -> String {
    let t = tracks
        .iter()
        .filter(|t| !crate::local::is_local(&t.id)) // files on your PC can't be shared
        .map(|t| match t.deezer_number() {
            Some(n) => Entry::Deezer(n),
            None => Entry::Song { t: t.title.clone(), a: t.artist.name.clone(), d: t.duration, c: t.cover_big.clone() },
        })
        .collect();
    let json = serde_json::to_vec(&Shared { n: name.to_string(), o: owner.to_string(), t }).unwrap_or_default();
    let packed = miniz_oxide::deflate::compress_to_vec(&json, 9);
    format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(packed))
}

pub fn decode(code: &str) -> Result<Shared, String> {
    let bad = || "That share code is incomplete or damaged. Copy it again.".to_string();
    let body: String = code.trim().strip_prefix(PREFIX).ok_or_else(bad)?.chars().filter(|c| !c.is_whitespace()).collect();
    let packed = URL_SAFE_NO_PAD.decode(body.trim_end_matches('=')).map_err(|_| bad())?;
    // Limit the unpacked size so a bad code can't use lots of memory.
    let json = miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, 4 * 1024 * 1024).map_err(|_| bad())?;
    serde_json::from_slice(&json).map_err(|_| bad())
}

/// A song from a share code that isn't on Deezer.
pub fn song_track(t: &str, a: &str, d: u32, c: &Option<String>, n: usize) -> Track {
    Track {
        id: format!("imp:shared:{}-{}-{n}", crate::youtube::norm(a), crate::youtube::norm(t)),
        title: t.to_string(),
        duration: d,
        artist: ArtistRef { id: String::new(), name: a.to_string(), picture: None },
        cover_small: c.clone(),
        cover_big: c.clone(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_and_stay_short() {
        let mut tracks: Vec<Track> = (0..50).map(|i| Track { id: format!("dz:{}", 3_000_000_000u64 + i), ..Default::default() }).collect();
        tracks.push(Track { id: "imp:x".into(), title: "Song".into(), artist: ArtistRef { name: "Band".into(), ..Default::default() }, duration: 200, ..Default::default() });
        tracks.push(Track { id: "local:C:/music/a.mp3".into(), ..Default::default() });
        let code = encode("Road trip", "tacent", &tracks);
        assert!(code.starts_with(PREFIX));
        assert!(code.len() < 800, "{} characters", code.len());
        let back = decode(&code).unwrap();
        assert_eq!(back.n, "Road trip");
        assert_eq!(back.o, "tacent");
        assert_eq!(back.t.len(), 51, "local files are left out");
        assert_eq!(back.t[0], Entry::Deezer(3_000_000_000));
        assert_eq!(back.t[50], Entry::Song { t: "Song".into(), a: "Band".into(), d: 200, c: None });
        // Pasted with a line break in the middle still works.
        let wrapped = format!("{}\n{}", &code[..40], &code[40..]);
        assert_eq!(decode(&wrapped).unwrap(), back);
    }

    #[test]
    fn bad_codes_are_refused() {
        assert!(decode("pulse1:").is_err());
        assert!(decode("pulse1:!!!!").is_err());
        assert!(decode("hello").is_err());
        assert!(decode(&format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(b"not deflated"))).is_err());
    }
}
