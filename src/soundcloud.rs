//! SoundCloud as a second source, for songs YouTube doesn't have (a SoundCloud-only
//! release, say). It only runs after YouTube found nothing it could play, and it uses
//! yt-dlp's own SoundCloud support: public tracks only, no account and no keys of ours.
//!
//! A SoundCloud track is saved like a YouTube video id, with an "sc:" in front of its
//! address, so the next play skips the search.

use crate::audio::StreamSource;
use crate::deezer::Track;
use crate::youtube::{self, Tools};
use serde::Deserialize;
use std::time::Duration;

const PREFIX: &str = "sc:";
const HOME: &str = "https://soundcloud.com/";

/// The id Pulse saves for a SoundCloud track.
pub fn id_for(url: &str) -> String {
    format!("{PREFIX}{url}")
}

/// A saved SoundCloud id's address (None for anything else, or an address that isn't SoundCloud's).
pub fn url_of(id: &str) -> Option<&str> {
    id.strip_prefix(PREFIX).filter(|u| u.starts_with(HOME))
}

pub fn is_soundcloud_id(id: &str) -> bool {
    id.starts_with(PREFIX)
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Entry {
    pub title: String,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    pub webpage_url: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Found {
    entries: Vec<Option<Entry>>,
}

fn parse(json: &str) -> Result<Vec<Entry>, String> {
    let found: Found = serde_json::from_str(json).map_err(|e| format!("Unexpected SoundCloud search result ({e})"))?;
    Ok(found.entries.into_iter().flatten().collect())
}

/// One flat yt-dlp search of SoundCloud's public tracks.
fn search(tools: &Tools, query: &str, limit: u32) -> Result<Vec<Entry>, String> {
    let mut args = youtube::base_args(tools);
    args.extend(["--flat-playlist".into(), "-J".into(), format!("scsearch{limit}:{query}")]);
    parse(&youtube::run(&tools.ytdlp, &args, Duration::from_secs(30))?)
}

impl Entry {
    fn link(&self) -> Option<&str> {
        self.webpage_url.as_deref().filter(|u| u.starts_with(HOME))
    }
}

/// How well a result is the song, or None if it isn't. Strict, because SoundCloud holds
/// covers, remixes and re-uploads of everything: the title must fit, the artist must be
/// the uploader (or named in the title), it must not be another version, and the length
/// must be close.
fn rate(track: &Track, e: &Entry) -> Option<i32> {
    e.link()?;
    if !youtube::title_matches(&track.title, &e.title) || youtube::unwanted_version(&track.title, &e.title) {
        return None;
    }
    let artists: Vec<String> = std::iter::once(&track.artist).chain(&track.featured).map(|a| youtube::norm(&a.name)).filter(|a| !a.is_empty()).collect();
    let by = |text: &str| {
        let t = youtube::norm(text);
        !t.is_empty() && artists.iter().any(|a| t.contains(a.as_str()) || (t.len() >= 4 && a.contains(&t)))
    };
    let by_uploader = by(e.uploader.as_deref().unwrap_or(""));
    if !by_uploader && !by(&e.title) {
        return None;
    }
    let mut score = 50 + if by_uploader { 30 } else { 10 };
    if track.duration >= 60
        && let Some(d) = e.duration
    {
        let diff = (d - track.duration as f64).abs();
        if diff > 25.0 {
            return None;
        }
        score += match diff {
            x if x <= 3.0 => 20,
            x if x <= 8.0 => 10,
            _ => 0,
        };
    }
    Some(score)
}

/// The best result that really is the song.
pub fn pick<'a>(track: &Track, entries: &'a [Entry]) -> Option<&'a Entry> {
    entries.iter().filter_map(|e| rate(track, e).map(|s| (s, e))).max_by_key(|(s, _)| *s).map(|(_, e)| e)
}

/// Searches for the song: with the artist, with the artist's name run together (how a
/// SoundCloud account is often named), then the title alone. The first search that has the song wins.
fn find(tools: &Tools, track: &Track) -> Option<String> {
    let title = youtube::core_title(&track.title);
    let artist = track.artist.name.as_str();
    let mut queries = vec![format!("{artist} {title}"), format!("{} {title}", artist.split_whitespace().collect::<String>()), title];
    queries.dedup();
    for q in queries {
        match search(tools, &q, 8) {
            Ok(entries) => {
                if let Some(e) = pick(track, &entries) {
                    return e.link().map(str::to_string);
                }
            }
            Err(e) => {
                crate::audio::trace(&format!("soundcloud: search failed: {e}"));
                // Rate limited or offline: more searches won't help.
                return None;
            }
        }
    }
    None
}

/// The audio link of one SoundCloud track. Only plain downloadable files: a track that
/// SoundCloud streams in pieces (HLS) isn't played.
pub fn open(tools: &Tools, url: &str, track: &Track) -> Result<StreamSource, String> {
    let mut args = youtube::base_args(tools);
    args.extend(["--no-playlist".into(), "-J".into(), "-f".into(), "bestaudio[protocol=https]/bestaudio[protocol=http]".into(), url.into()]);
    let json = youtube::run(&tools.ytdlp, &args, Duration::from_secs(60)).map_err(|e| {
        crate::audio::trace(&format!("soundcloud: {url}: {e}"));
        "This SoundCloud track can't be played right now.".to_string()
    })?;
    if youtube::is_wrong_video(track, &json) {
        return Err("That SoundCloud track isn't this song.".into());
    }
    let mut src = youtube::source_from_info(&json).map_err(|e| {
        crate::audio::trace(&format!("soundcloud: {url}: {e}"));
        "SoundCloud has this track in a form Pulse can't play.".to_string()
    })?;
    src.notice = Some("Playing from SoundCloud".into());
    Ok(src)
}

/// Looks for the song on SoundCloud and opens it. Returns the audio and the id to save.
pub fn find_and_open(tools: &Tools, track: &Track) -> Result<(StreamSource, String), String> {
    let url = find(tools, track).ok_or("Couldn't find this song on SoundCloud.")?;
    let src = open(tools, &url, track)?;
    Ok((src, id_for(&url)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deezer::ArtistRef;

    fn song(artist: &str, title: &str, secs: u32) -> Track {
        Track { id: "x".into(), title: title.into(), duration: secs, artist: ArtistRef { name: artist.into(), ..Default::default() }, ..Default::default() }
    }
    fn entry(title: &str, uploader: &str, secs: f64, path: &str) -> Entry {
        Entry { title: title.into(), uploader: Some(uploader.into()), duration: Some(secs), webpage_url: Some(format!("{HOME}{path}")) }
    }

    // What SoundCloud returned for "wen we fuckin" (yt-dlp, flat), shortened.
    fn results() -> Vec<Entry> {
        vec![
            entry("wen we walk in2 theparty kno we muggg", "elioe39", 87.87, "elioe39/wen-we-walk-in2-theparty-kno"),
            entry("EVERYTIME WE FUCKIN' KICK", "OZONYX", 303.8, "user-941396819/everytime-we-fuckin-kick"),
            entry("wen we fuckn (WarSemi)", "deathtoricky", 152.717, "deathtoricky/wen-we-fuckn"),
            entry("TRULY - WEN WE PULL UP FREESTYLE", "Tre", 153.17, "tre-93/truly-wen-we-pull-up-freestyle"),
        ]
    }

    #[test]
    fn finds_the_song_by_its_artist_among_other_uploads() {
        let t = song("Death To Ricky", "Wen We Fuckin", 153);
        let e = results();
        assert_eq!(pick(&t, &e).and_then(Entry::link), Some("https://soundcloud.com/deathtoricky/wen-we-fuckn"));
    }

    #[test]
    fn never_plays_a_different_song() {
        let t = song("Death To Ricky", "Wen We Fuckin", 153);
        // Same length, same words in the title, wrong uploader.
        let others = vec![entry("wen we fuckn", "someone else", 152.0, "someone/wen-we-fuckn"), entry("TRULY - WEN WE PULL UP FREESTYLE", "Tre", 153.0, "tre-93/x")];
        assert!(pick(&t, &others).is_none());
        // Right uploader, other song.
        let t2 = song("Death To Ricky", "Depictions", 150);
        assert!(pick(&t2, &[entry("lemon haze", "deathtoricky", 154.8, "deathtoricky/lemon-haze")]).is_none());
    }

    #[test]
    fn remixes_and_wrong_lengths_are_left_out() {
        let t = song("Death To Ricky", "Wen We Fuckin", 153);
        assert!(pick(&t, &[entry("wen we fuckn (sped up)", "deathtoricky", 130.0, "deathtoricky/a")]).is_none());
        assert!(pick(&t, &[entry("wen we fuckn", "deathtoricky", 240.0, "deathtoricky/b")]).is_none());
        assert!(pick(&t, &[entry("wen we fuckn (remix)", "deathtoricky", 152.0, "deathtoricky/c")]).is_none());
    }

    #[test]
    fn only_soundcloud_addresses_count() {
        let t = song("Death To Ricky", "Wen We Fuckin", 153);
        let mut e = entry("wen we fuckn", "deathtoricky", 152.0, "x");
        e.webpage_url = Some("https://example.com/wen-we-fuckn".into());
        assert!(pick(&t, &[e]).is_none());
        assert_eq!(url_of("sc:https://soundcloud.com/a/b"), Some("https://soundcloud.com/a/b"));
        assert_eq!(url_of("sc:https://evil.example/a"), None);
        assert_eq!(url_of("L-AgYKnXN1w"), None);
        assert_eq!(id_for("https://soundcloud.com/a/b"), "sc:https://soundcloud.com/a/b");
        assert!(is_soundcloud_id("sc:anything") && !is_soundcloud_id("L-AgYKnXN1w"));
    }

    #[test]
    fn reads_yt_dlps_search_result() {
        let json = r#"{"entries":[{"id":"1799476753","title":"wen we fuckn (WarSemi)","uploader":"deathtoricky","duration":152.717,"url":"https://api.soundcloud.com/tracks/soundcloud%3Atracks%3A1799476753","webpage_url":"https://soundcloud.com/deathtoricky/wen-we-fuckn"},null,{"title":"no address"}]}"#;
        let found = parse(json).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].link(), Some("https://soundcloud.com/deathtoricky/wen-we-fuckn"));
        assert_eq!(found[1].link(), None);
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"entries":[]}"#).unwrap().is_empty());
    }

    #[test]
    fn a_progressive_track_becomes_a_source() {
        let info = r#"{"title":"wen we fuckn (WarSemi)","duration":152.7,"ext":"mp3","url":"https://cf-media.sndcdn.com/a.mp3","http_headers":{"User-Agent":"x"},"filesize":2443000}"#;
        let src = youtube::source_from_info(info).unwrap();
        assert_eq!(src.codec, crate::audio::Codec::Mp3);
        assert!(src.url.starts_with("https://cf-media.sndcdn.com/"));
        // The wrong track behind a saved link is refused.
        let t = song("Death To Ricky", "Wen We Fuckin", 153);
        assert!(!youtube::is_wrong_video(&t, info));
        assert!(youtube::is_wrong_video(&t, r#"{"title":"lemon haze","duration":300}"#));
    }
}
