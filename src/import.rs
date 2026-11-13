//! Importing playlists from YouTube, Spotify and SoundCloud.
//!
//! Pulse reads the song list from public pages (no accounts or keys):
//! - YouTube / YouTube Music: through yt-dlp.
//! - Spotify: the public embed page of a playlist or album (what websites show
//!   when they embed a Spotify player) lists its songs, up to 100. For longer playlists
//!   you can paste the songs themselves: in Spotify, Ctrl+A then Ctrl+C on a playlist copies
//!   a link to every song, and Pulse reads each song's public embed page.
//! - SoundCloud: the public playlist page names the first songs; the rest are
//!   looked up one by one through SoundCloud's public oEmbed address (what
//!   websites use to embed a SoundCloud player).
//!
//! Each song is then matched to Deezer for its cover and details. Audio still
//! comes from YouTube; for YouTube playlists Pulse plays the exact videos.

use crate::deezer::{AlbumRef, ArtistRef, Track};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    YouTube,
    Spotify,
    SoundCloud,
    /// A Pulse share code.
    Shared,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::YouTube => "YouTube",
            Source::Spotify => "Spotify",
            Source::SoundCloud => "SoundCloud",
            Source::Shared => "shared",
        }
    }
}

/// A song as listed by the other service.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub title: String,
    pub artist: String,
    /// Seconds (0 if unknown).
    pub duration: u32,
    pub art: Option<String>,
    /// The YouTube video, for YouTube playlists.
    pub video: Option<String>,
}

/// Shared with the import window: what's happening and how far along.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub status: String,
    pub done: usize,
    pub total: usize,
}

/// A finished import: the playlist's name, its songs (with the YouTube video to play,
/// when known) and how many matched a Deezer song.
pub struct Imported {
    pub name: String,
    /// Who shared it (share codes only).
    pub owner: Option<String>,
    pub tracks: Vec<(Track, Option<String>)>,
    pub matched: usize,
    /// Something to tell you afterwards (songs Spotify's link left out).
    pub note: Option<String>,
}

/// Spotify ids (22 letters and digits) of every song link in the text, in order, once each.
/// Spotify copies songs as one link per line; a one-line text box may run them together.
pub fn spotify_track_ids(text: &str) -> Vec<String> {
    let mut ids: Vec<String> = vec![];
    for marker in ["track/", "track:"] {
        let mut rest = text;
        while let Some(i) = rest.find(marker) {
            rest = &rest[i + marker.len()..];
            let id: String = rest.chars().take(22).collect();
            if id.len() == 22 && id.chars().all(|c| c.is_ascii_alphanumeric()) && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    // Keep the order they were pasted in, whichever spelling they used.
    ids.sort_by_key(|id| text.find(id.as_str()));
    ids
}

pub fn detect(url: &str) -> Result<Source, String> {
    if crate::share::is_code(url) {
        return Ok(Source::Shared);
    }
    let u = url.trim().to_lowercase();
    if u.contains("youtube.com/") || u.contains("youtu.be/") {
        Ok(Source::YouTube)
    } else if u.contains("spotify.com/") {
        Ok(Source::Spotify)
    } else if u.contains("soundcloud.com/") {
        Ok(Source::SoundCloud)
    } else {
        Err("Paste a link to a YouTube, Spotify or SoundCloud playlist, or a Pulse share code.".into())
    }
}

/// Runs a whole import. `cancel` stops it early.
pub fn run(url: &str, progress: &Arc<Mutex<Progress>>, cancel: &AtomicBool) -> Result<Imported, String> {
    let set = |status: &str, done: usize, total: usize| {
        *progress.lock().unwrap() = Progress { status: status.to_string(), done, total };
    };
    let source = detect(url)?;
    if source == Source::Shared {
        return shared(url, &set, cancel);
    }
    set(&format!("Reading the {} playlist…", source.name()), 0, 0);
    let mut note = None;
    let (name, items) = match source {
        Source::YouTube => youtube(url, &|s| set(s, 0, 0))?,
        Source::Spotify if spotify_track_ids(url).len() > 1 => {
            spotify_songs(&spotify_track_ids(url), &|done, total| set("Reading the Spotify songs…", done, total), cancel)?
        }
        Source::Spotify => {
            let (name, items) = spotify(url)?;
            if items.len() >= 100
                && let Some(("playlist", id)) = spotify_id(url).as_ref().map(|(k, id)| (*k, id.clone()))
                && let Some(total) = spotify_playlist_size(&id)
                && total > items.len()
            {
                note = Some(format!(
                    "Spotify's link only shares the first {} of {total} songs. To bring them all: open the playlist in Spotify, press Ctrl+A, then Ctrl+C, and paste here.",
                    items.len()
                ));
            }
            (name, items)
        }
        Source::SoundCloud => soundcloud(url, &|done, total| set("Reading the SoundCloud playlist…", done, total), cancel)?,
        Source::Shared => unreachable!("share codes return early"),
    };
    if items.is_empty() {
        return Err("That playlist is empty, private, or couldn't be read.".into());
    }
    let total = items.len();
    let mut tracks = vec![];
    let mut matched = 0;
    for (i, item) in items.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Import cancelled.".into());
        }
        set("Matching songs…", i, total);
        let mut found = crate::deezer::find_track(&item.artist, &item.title, item.duration).ok().flatten();
        if found.is_none() && source != Source::Spotify && !item.artist.is_empty() {
            // Uploaders write "Artist - Song" and "Song - Artist"; try the other way round.
            std::thread::sleep(std::time::Duration::from_millis(220));
            found = crate::deezer::find_track(&item.title, &item.artist, item.duration).ok().flatten();
        }
        // Stay under Deezer's limit of 50 requests per 5 seconds (each match uses up to 2).
        std::thread::sleep(std::time::Duration::from_millis(220));
        let t = match found {
            Some(t) => {
                matched += 1;
                t
            }
            None => {
                // Not on Deezer: take the cover from iTunes, else the video's picture (YouTube),
                // else none (Pulse fills it in from YouTube once the song is looked up).
                let mut item = item.clone();
                if let Some(art) = crate::itunes::cover(&item.artist, &item.title) {
                    item.art = Some(art);
                }
                let mut t = unmatched(&item, source, i);
                // TIDAL knows the song's real length, whether it swears and who's on it.
                if let Some(song) = crate::tidal::find(&item.artist, &item.title, item.duration) {
                    with_tidal(&mut t, &song);
                }
                t
            }
        };
        if !tracks.iter().any(|(x, _): &(Track, Option<String>)| x.id == t.id) {
            tracks.push((t, item.video.clone()));
        }
    }
    set("Done", total, total);
    Ok(Imported { name, owner: None, tracks, matched, note })
}

/// A Pulse share code: Deezer songs are fetched by number, the rest are in the code.
fn shared(code: &str, set: &dyn Fn(&str, usize, usize), cancel: &AtomicBool) -> Result<Imported, String> {
    let s = crate::share::decode(code)?;
    let total = s.t.len();
    let mut tracks = vec![];
    for (i, e) in s.t.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Import cancelled.".into());
        }
        set("Getting songs…", i, total);
        match e {
            crate::share::Entry::Deezer(n) => {
                if let Ok(t) = crate::deezer::track_by_id(*n) {
                    tracks.push((t, None));
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
            crate::share::Entry::Song { t, a, d, c } => tracks.push((crate::share::song_track(t, a, *d, c, i), None)),
        }
    }
    let matched = tracks.len();
    Ok(Imported { name: s.n, owner: Some(s.o).filter(|o| !o.is_empty()), tracks, matched, note: None })
}

/// A song that isn't on Deezer: kept with what the other service said about it.
/// Pulse still plays it from YouTube.
fn unmatched(item: &Item, source: Source, n: usize) -> Track {
    let (main, also) = crate::deezer::split_artists(&item.artist);
    let key = item.video.clone().unwrap_or_else(|| format!("{}-{}-{n}", crate::youtube::norm(&item.artist), crate::youtube::norm(&item.title)));
    Track {
        id: format!("imp:{}:{key}", source.name().to_lowercase()),
        title: item.title.clone(),
        duration: item.duration,
        artist: ArtistRef { id: String::new(), name: if main.is_empty() { "Unknown artist".into() } else { main }, picture: None },
        featured: also.into_iter().map(|name| ArtistRef { name, ..Default::default() }).collect(),
        album: None::<AlbumRef>,
        cover_small: item.art.clone(),
        cover_big: item.art.clone(),
        ..Default::default()
    }
}

/// Fills in what TIDAL knows about a song Deezer doesn't have.
fn with_tidal(t: &mut Track, song: &crate::tidal::Song) {
    if song.duration >= 60 {
        t.duration = song.duration;
    }
    t.explicit = song.explicit;
    for name in song.artists.iter().skip(1) {
        if !t.artist.name.eq_ignore_ascii_case(name) && !t.featured.iter().any(|a| a.name.eq_ignore_ascii_case(name)) {
            t.featured.push(ArtistRef { name: name.clone(), ..Default::default() });
        }
    }
}

// ---------------------------------------------------------------------------
// YouTube
// ---------------------------------------------------------------------------

fn youtube(url: &str, status: &dyn Fn(&str)) -> Result<(String, Vec<Item>), String> {
    let (title, entries) = crate::youtube::playlist(url, status)?;
    let items = entries
        .into_iter()
        .map(|e| {
            let channel = e.channel.clone().or(e.uploader.clone()).unwrap_or_default();
            let (artist, title) = split_video_title(&e.title, &channel);
            Item { title, artist, duration: e.duration.unwrap_or(0.0) as u32, art: Some(youtube_thumb(&e.id)), video: Some(e.id) }
        })
        .collect();
    Ok((title, items))
}

/// A video's 16:9 picture (Pulse shows the middle square of it).
pub fn youtube_thumb(video: &str) -> String {
    format!("https://i.ytimg.com/vi/{video}/mqdefault.jpg")
}

/// "Artist - Song (Official Video)" → ("Artist", "Song"); otherwise the channel is the artist.
pub fn split_video_title(title: &str, channel: &str) -> (String, String) {
    let channel = channel.trim().trim_end_matches(" - Topic").trim_end_matches("VEVO").trim().to_string();
    let mut t = title.to_string();
    // Drop "(Official Video)", "[Lyrics]" and similar.
    for (open, close) in [('(', ')'), ('[', ']')] {
        while let Some(a) = t.find(open) {
            let Some(b) = t[a..].find(close) else { break };
            let inside = t[a + 1..a + b].to_lowercase();
            let junk = ["official", "video", "audio", "lyric", "visualizer", "visualiser", "hd", "4k", "mv", "music"];
            if junk.iter().any(|j| inside.contains(j)) {
                t.replace_range(a..a + b + 1, "");
            } else {
                break;
            }
        }
    }
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    for sep in [" - ", " – ", " — ", " | "] {
        if let Some((a, b)) = t.split_once(sep) {
            return (a.trim().to_string(), b.trim().to_string());
        }
    }
    (channel, t.trim().to_string())
}

// ---------------------------------------------------------------------------
// Spotify
// ---------------------------------------------------------------------------

/// "https://open.spotify.com/intl-de/playlist/ID?si=…" → ("playlist", "ID").
pub fn spotify_id(url: &str) -> Option<(&'static str, String)> {
    let path = url.split(['?', '#']).next()?;
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    for (i, p) in parts.iter().enumerate() {
        let kind = match *p {
            "playlist" => "playlist",
            "album" => "album",
            "track" => "track",
            _ => continue,
        };
        let id = parts.get(i + 1)?;
        if id.chars().all(|c| c.is_ascii_alphanumeric()) && !id.is_empty() {
            return Some((kind, id.to_string()));
        }
    }
    None
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SpEntity {
    name: Option<String>,
    title: Option<String>,
    subtitle: Option<String>,
    duration: Option<u64>,
    #[serde(rename = "trackList")]
    track_list: Vec<SpTrack>,
    #[serde(rename = "coverArt")]
    cover_art: Option<SpCover>,
    /// A single song's page names its artists here (it has no subtitle).
    artists: Vec<SpArtist>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SpArtist {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SpTrack {
    title: String,
    subtitle: String,
    duration: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SpCover {
    sources: Vec<SpImage>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SpImage {
    url: String,
}

/// The songs a Spotify link lists as (artists, title, seconds), for the live matching test.
#[cfg(test)]
pub(crate) fn spotify_for_test(url: &str) -> Result<Vec<(String, String, u32)>, String> {
    Ok(spotify(url)?.1.into_iter().map(|i| (i.artist, i.title, i.duration)).collect())
}

fn spotify(url: &str) -> Result<(String, Vec<Item>), String> {
    let (kind, id) = spotify_id(url).ok_or("That doesn't look like a Spotify playlist or album link.")?;
    let html = crate::net::get_page(&format!("https://open.spotify.com/embed/{kind}/{id}"))?;
    parse_spotify_embed(&html)
}

/// Songs pasted from Spotify (links copied with Ctrl+A, Ctrl+C), read one by one from each
/// song's public embed page.
fn spotify_songs(ids: &[String], progress: &dyn Fn(usize, usize), cancel: &AtomicBool) -> Result<(String, Vec<Item>), String> {
    use std::sync::atomic::AtomicUsize;
    // Four pages at a time: a long playlist takes seconds, not minutes.
    let found: Vec<Mutex<Option<Item>>> = ids.iter().map(|_| Mutex::new(None)).collect();
    let (next, done) = (AtomicUsize::new(0), AtomicUsize::new(0));
    std::thread::scope(|s| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                s.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= ids.len() || cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        *found[i].lock().unwrap() = spotify_song(&ids[i]);
                        done.fetch_add(1, Ordering::Relaxed);
                    }
                })
            })
            .collect();
        while !workers.iter().all(|w| w.is_finished()) {
            progress(done.load(Ordering::Relaxed), ids.len());
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    if cancel.load(Ordering::Relaxed) {
        return Err("Import cancelled.".into());
    }
    let items: Vec<Item> = found.into_iter().filter_map(|m| m.into_inner().ok().flatten()).collect();
    if items.is_empty() {
        return Err("Pulse couldn't read those Spotify songs. Check your connection and try again.".into());
    }
    Ok(("Spotify songs".into(), items))
}

/// One song's title and artist from its public embed page (tried twice: Spotify
/// sometimes answers "slow down").
fn spotify_song(id: &str) -> Option<Item> {
    for attempt in 0..2 {
        if let Ok(html) = crate::net::get_page(&format!("https://open.spotify.com/embed/track/{id}"))
            && let Ok((_, mut found)) = parse_spotify_embed(&html)
            && !found.is_empty()
        {
            return Some(found.remove(0));
        }
        std::thread::sleep(std::time::Duration::from_millis(600 * (attempt + 1)));
    }
    None
}

/// How many songs a public Spotify playlist has, from its page's description
/// ("Playlist · Spotify · 150 items · 2.1M saves").
fn spotify_playlist_size(id: &str) -> Option<usize> {
    let html = crate::net::get_page(&format!("https://open.spotify.com/playlist/{id}")).ok()?;
    playlist_size_from_page(&html)
}

pub fn playlist_size_from_page(html: &str) -> Option<usize> {
    let desc = between(html, "<meta property=\"og:description\" content=\"", "\"").or_else(|| between(html, "<meta name=\"description\" content=\"", "\""))?;
    desc.split('·').find_map(|part| {
        let mut w = part.split_whitespace();
        let n = w.next()?.replace(',', "");
        matches!(w.next()?, "items" | "songs" | "item" | "song").then(|| n.parse().ok()).flatten()
    })
}

pub fn parse_spotify_embed(html: &str) -> Result<(String, Vec<Item>), String> {
    let json =
        between(html, "<script id=\"__NEXT_DATA__\" type=\"application/json\">", "</script>").ok_or("Spotify's page has changed; Pulse couldn't read it.")?;
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| "Spotify's page has changed; Pulse couldn't read it.")?;
    let entity = v.pointer("/props/pageProps/state/data/entity").cloned().ok_or("This Spotify playlist is private or doesn't exist.")?;
    let e: SpEntity = serde_json::from_value(entity).map_err(|_| "Spotify's page has changed; Pulse couldn't read it.")?;
    let cover = e.cover_art.and_then(|c| c.sources.into_iter().map(|s| s.url).find(|u| !u.is_empty()));
    let name = e.name.or(e.title.clone()).unwrap_or_else(|| "Spotify playlist".into());
    let mut items: Vec<Item> = e
        .track_list
        .into_iter()
        .map(|t| Item {
            title: t.title,
            // "Artist A, Artist B": all of them, so featured artists are kept.
            artist: t.subtitle.replace('\u{a0}', " ").trim().to_string(),
            duration: (t.duration / 1000) as u32,
            // The embed page has no per-song pictures (only the playlist's own cover).
            art: None,
            video: None,
        })
        .collect();
    // A single song link.
    if items.is_empty()
        && let Some(title) = e.title
    {
        let artist = e.subtitle.filter(|s| !s.is_empty()).unwrap_or_else(|| e.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "));
        items.push(Item { title, artist, duration: (e.duration.unwrap_or(0) / 1000) as u32, art: cover, video: None });
    }
    Ok((name, items))
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let a = s.find(start)? + start.len();
    let b = s[a..].find(end)? + a;
    Some(&s[a..b])
}

// ---------------------------------------------------------------------------
// SoundCloud
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct ScHydration {
    hydratable: String,
    data: serde_json::Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ScPlaylist {
    title: String,
    tracks: Vec<ScTrack>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ScTrack {
    id: u64,
    title: Option<String>,
    duration: Option<u64>,
    artwork_url: Option<String>,
    user: Option<ScUser>,
    publisher_metadata: Option<ScPublisher>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ScUser {
    username: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ScPublisher {
    artist: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OEmbed {
    title: String,
    author_name: String,
    thumbnail_url: Option<String>,
}

fn soundcloud(url: &str, progress: &dyn Fn(usize, usize), cancel: &AtomicBool) -> Result<(String, Vec<Item>), String> {
    let clean = url.trim().split(['?', '#']).next().unwrap_or(url).to_string();
    let html = crate::net::get_page(&clean)?;
    let (title, tracks) = parse_soundcloud_page(&html)?;
    let total = tracks.len();
    let mut items = vec![];
    for (i, t) in tracks.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Import cancelled.".into());
        }
        progress(i, total);
        match t.title.clone() {
            Some(title) => items.push(sc_item(&t, title)),
            None => {
                // Only the first songs are on the page; look the others up by number.
                let api = format!("https://api.soundcloud.com/tracks/{}", t.id);
                let o = crate::net::get_page(&format!("https://soundcloud.com/oembed?format=json&url={}", crate::net::encode(&api)))
                    .ok()
                    .and_then(|body| serde_json::from_str::<OEmbed>(&body).ok());
                if let Some(o) = o {
                    // oEmbed titles read "Song by Uploader".
                    let suffix = format!(" by {}", o.author_name);
                    let song = o.title.strip_suffix(&suffix).unwrap_or(&o.title).to_string();
                    let (artist, song) = split_sc_title(&song, &o.author_name);
                    items.push(Item { title: song, artist, duration: 0, art: o.thumbnail_url, video: None });
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        }
    }
    Ok((title, items))
}

fn parse_soundcloud_page(html: &str) -> Result<(String, Vec<ScTrack>), String> {
    let json = between(html, "window.__sc_hydration = ", ";</script>").ok_or("SoundCloud's page has changed; Pulse couldn't read it.")?;
    let parts: Vec<ScHydration> = serde_json::from_str(json).map_err(|_| "SoundCloud's page has changed; Pulse couldn't read it.")?;
    let pl = parts.into_iter().find(|p| p.hydratable == "playlist").ok_or("That isn't a SoundCloud playlist or album link (it should contain /sets/).")?;
    let p: ScPlaylist = serde_json::from_value(pl.data).map_err(|_| "SoundCloud's page has changed; Pulse couldn't read it.")?;
    Ok((p.title, p.tracks))
}

fn sc_item(t: &ScTrack, title: String) -> Item {
    let uploader = t.user.as_ref().map(|u| u.username.clone()).unwrap_or_default();
    let (artist, song) = match t.publisher_metadata.as_ref().and_then(|p| p.artist.clone()).filter(|a| !a.trim().is_empty()) {
        Some(a) => (a, title),
        None => split_sc_title(&title, &uploader),
    };
    Item {
        title: song,
        artist,
        duration: (t.duration.unwrap_or(0) / 1000) as u32,
        art: t.artwork_url.as_ref().map(|u| u.replace("-large.", "-t500x500.")),
        video: None,
    }
}

/// SoundCloud uploads are often titled "Artist - Song".
fn split_sc_title(title: &str, uploader: &str) -> (String, String) {
    match title.split_once(" - ") {
        Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
        None => (uploader.to_string(), title.trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_services() {
        assert_eq!(detect("https://www.youtube.com/playlist?list=PL123").unwrap(), Source::YouTube);
        assert_eq!(detect("https://music.youtube.com/playlist?list=x").unwrap(), Source::YouTube);
        assert_eq!(detect("https://open.spotify.com/playlist/37i9dQ?si=a").unwrap(), Source::Spotify);
        assert_eq!(detect("https://soundcloud.com/a/sets/b").unwrap(), Source::SoundCloud);
        assert!(detect("https://example.com").is_err());
        assert_eq!(spotify_id("https://open.spotify.com/intl-de/playlist/37i9dQZF1DX?si=abc"), Some(("playlist", "37i9dQZF1DX".into())));
        assert_eq!(spotify_id("https://open.spotify.com/album/1A2b3C"), Some(("album", "1A2b3C".into())));
        assert_eq!(spotify_id("https://open.spotify.com/user/x"), None);
    }

    #[test]
    fn cleans_up_video_titles() {
        assert_eq!(split_video_title("Tame Impala - Dracula (Official Video)", "TameImpalaVEVO"), ("Tame Impala".into(), "Dracula".into()));
        assert_eq!(split_video_title("Dracula", "Tame Impala - Topic"), ("Tame Impala".into(), "Dracula".into()));
        assert_eq!(split_video_title("Song (feat. X) [Lyrics]", "Chan"), ("Chan".into(), "Song (feat. X)".into()));
    }

    #[test]
    fn reads_pasted_spotify_songs() {
        let lines = "https://open.spotify.com/track/190jyVPHYjAqEaOGmMzdyk\nhttps://open.spotify.com/track/0VjIjW4GlUZAMYd2vXMi3b?si=x\n";
        assert_eq!(spotify_track_ids(lines), ["190jyVPHYjAqEaOGmMzdyk", "0VjIjW4GlUZAMYd2vXMi3b"]);
        // Run together by a one-line box, plus the app's "spotify:track:" form and a repeat.
        let joined =
            "https://open.spotify.com/track/190jyVPHYjAqEaOGmMzdykspotify:track:0VjIjW4GlUZAMYd2vXMi3b https://open.spotify.com/track/190jyVPHYjAqEaOGmMzdyk";
        assert_eq!(spotify_track_ids(joined), ["190jyVPHYjAqEaOGmMzdyk", "0VjIjW4GlUZAMYd2vXMi3b"]);
        assert!(spotify_track_ids("https://open.spotify.com/playlist/37i9dQZF1DX5Ejj0EkURtP").is_empty());
        let page = r#"<meta property="og:description" content="Playlist · Spotify · 1,150 items · 2.1M saves">"#;
        assert_eq!(playlist_size_from_page(page), Some(1150));
        assert_eq!(playlist_size_from_page("<html></html>"), None);
    }

    #[test]
    fn reads_a_single_spotify_song_page() {
        // A song's own page: no subtitle, the artists are listed instead (as Spotify serves it).
        let html = r#"<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{"type":"track","name":"Beauty And A Beat","title":"Beauty And A Beat","artists":[{"name":"Justin Bieber"},{"name":"Nicki Minaj"}],"duration":227986}}}}}}</script>"#;
        let (_, items) = parse_spotify_embed(html).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].title.as_str(), items[0].artist.as_str(), items[0].duration), ("Beauty And A Beat", "Justin Bieber, Nicki Minaj", 227));
    }

    #[test]
    fn reads_spotify_embed_pages() {
        let html = r#"<html><script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{"name":"Top Hits","coverArt":{"sources":[{"url":"https://i.scdn.co/x"}]},"trackList":[{"title":"Loser","subtitle":"Tame Impala","duration":223069},{"title":"Song","subtitle":"A, B","duration":1000}]}}}}}}</script></html>"#;
        let (name, items) = parse_spotify_embed(html).unwrap();
        assert_eq!(name, "Top Hits");
        assert_eq!(items.len(), 2);
        // The playlist's cover isn't used as a song's cover.
        assert_eq!(items[0], Item { title: "Loser".into(), artist: "Tame Impala".into(), duration: 223, art: None, video: None });
        assert_eq!(items[1].artist, "A, B");
        assert!(parse_spotify_embed("<html></html>").is_err());
    }

    #[test]
    fn reads_soundcloud_pages() {
        let html = r#"<script>window.__sc_hydration = [{"hydratable":"user","data":{}},{"hydratable":"playlist","data":{"title":"rap playlist","tracks":[{"id":1,"title":"Shotta Flow 3","duration":181000,"user":{"username":"NLE Choppa"},"publisher_metadata":{"artist":"NLE Choppa"}},{"id":635210229,"kind":"track"}]}}];</script>"#;
        let (title, tracks) = parse_soundcloud_page(html).unwrap();
        assert_eq!(title, "rap playlist");
        assert_eq!(tracks.len(), 2);
        let first = sc_item(&tracks[0], tracks[0].title.clone().unwrap());
        assert_eq!((first.artist.as_str(), first.title.as_str(), first.duration), ("NLE Choppa", "Shotta Flow 3", 181));
        assert!(tracks[1].title.is_none());
        assert_eq!(split_sc_title("Bottles Poppin - Superr", "RAP"), ("Bottles Poppin".into(), "Superr".into()));
        assert!(parse_soundcloud_page("<html>nothing</html>").is_err());
    }
}
