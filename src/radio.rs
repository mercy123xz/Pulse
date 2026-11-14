//! Autoplay: what plays when your queue runs out, built around the song you picked.
//!
//! Where the songs come from, best first:
//! 1. YouTube Music's radio for that song: the "up next" list YouTube Music itself plays
//!    after it. It's chosen song by song (not just "more by this artist"), already in a
//!    good listening order, and mostly other artists with a few more by the same one.
//! 2. If YouTube Music can't be reached: Deezer's radio for the artist, plus the
//!    best-known songs of similar artists.
//!
//! Every pick is matched to its Deezer song (for the cover, the artist page and likes),
//! and YouTube Music's own audio for it is remembered, so it starts without a search.
//! Then the list is tidied: nothing you just heard or already queued, no second copy of a
//! song, no live / remix / sped-up versions unless the song you picked was one, no
//! tribute acts, a few songs per artist at most, and never the same artist twice in a row.
//!
//! Every function here that reaches the network blocks; the app calls them from a
//! background thread.

use crate::deezer::{self, Track};
use crate::youtube::{core_title, norm, unwanted_version};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One song to queue, and the YouTube video that plays it if YouTube Music named one.
#[derive(Debug, Clone)]
pub struct Pick {
    pub track: Track,
    pub video: Option<String>,
}

/// What kind of video YouTube Music lists: the song's album audio, its music video, or
/// something someone uploaded (a fan edit, a remix, a live clip).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Song,
    MusicVideo,
    Upload,
}

/// One entry of YouTube Music's radio.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioItem {
    pub video: String,
    pub title: String,
    /// Every credited artist, main one first.
    pub artists: Vec<String>,
    pub duration: Option<u32>,
    pub explicit: bool,
    pub kind: Kind,
}

/// Songs not to pick: what was just played and what's already queued.
#[derive(Default)]
pub struct Avoid {
    ids: HashSet<String>,
    songs: HashSet<String>,
}

impl Avoid {
    pub fn add(&mut self, t: &Track) {
        self.ids.insert(t.id.clone());
        self.songs.insert(song_key(t));
    }
    pub fn contains(&self, t: &Track) -> bool {
        self.ids.contains(&t.id) || self.songs.contains(&song_key(t))
    }
}

/// The same song whatever the version: "Get Lucky (Radio Edit - feat. …)" by Daft Punk
/// and "Get Lucky" by Daft Punk share one key.
fn song_key(t: &Track) -> String {
    let title = core_title(&t.title);
    let cut = [" (", " [", " - "].iter().filter_map(|m| title.find(m)).min().unwrap_or(title.len());
    format!("{}|{}", norm(&title[..cut]), artist_key(t))
}

fn artist_key(t: &Track) -> String {
    let n = norm(&t.artist.name);
    if n.is_empty() { t.artist.id.clone() } else { n }
}

// ---------------------------------------------------------------- the whole thing

/// Songs to play after `seed`, best first. `video` is the YouTube video playing it, if
/// known. Empty if nothing could be found (offline, say).
///
/// Staying in the song's genre: YouTube Music's radio sometimes wanders off to whatever's
/// popular (an R&B song followed by chart pop). So songs by artists like the one you picked
/// (Deezer's "fans also like") go first, with their best-known songs added when the radio
/// has few of them, and songs from further afield only now and then. Deezer's genre tags
/// aren't used for this: they're per album and coarse (Frank Ocean's "Blonde" is "Pop").
pub fn for_song(seed: &Track, video: Option<String>, avoid: &Avoid) -> Vec<Pick> {
    let video = video.or_else(|| crate::youtube::music_video(seed));
    // Side by side: YouTube Music's radio for the song, and the artists like this one.
    let (items, similar) = std::thread::scope(|s| {
        let radio = s.spawn(|| video.as_deref().map(|v| youtube_radio(v, 2)).and_then(Result::ok).unwrap_or_default());
        let similar = s.spawn(|| similar_artists(seed));
        (radio.join().unwrap_or_default(), similar.join().unwrap_or_default())
    });
    let picks = match_to_deezer(items.into_iter().filter(|i| Some(&i.video) != video.as_ref()).collect(), 36);
    let close: HashSet<String> = similar.iter().map(|a| norm(&a.name)).collect();
    // Few songs by artists like this one: add their best-known songs.
    let near = picks.iter().filter(|p| close.contains(&artist_key(&p.track))).count();
    let mut extra = vec![];
    if near < 20 {
        let ids: Vec<String> = similar.iter().take(12).map(|a| a.id.clone()).collect();
        extra = deezer::top_songs_of(&ids, 2).into_iter().map(|track| Pick { track, video: None }).collect();
    }
    // Still little to go on: Deezer's radio for the artist.
    if picks.len() + extra.len() < 12 {
        extra.extend(deezer_radio(seed).into_iter().map(|track| Pick { track, video: None }));
    }
    tidy(seed, close_first(picks, extra, &close, &artist_key(seed)), avoid)
}

/// Artists whose fans also play the seed's artist, most alike first.
fn similar_artists(seed: &Track) -> Vec<deezer::ArtistInfo> {
    let id = if seed.has_artist_page() { Some(seed.artist.id.clone()) } else { deezer::find_artist(&seed.artist.name).ok().flatten() };
    id.map(|id| deezer::related_artists(&id, 20)).unwrap_or_default()
}

/// The order of play, six songs at a time: four by artists like the one you picked
/// (`close`: the radio's own picks taking turns with `extra`, their best-known songs), one by
/// the artist you picked (`seed_artist`), and one from further afield in the radio. Whatever
/// runs out, the others fill its places.
fn close_first(radio: Vec<Pick>, extra: Vec<Pick>, close: &HashSet<String>, seed_artist: &str) -> Vec<Pick> {
    let (own, rest): (Vec<Pick>, Vec<Pick>) = radio.into_iter().partition(|p| artist_key(&p.track) == seed_artist);
    let (near, far): (Vec<Pick>, Vec<Pick>) = rest.into_iter().partition(|p| close.contains(&artist_key(&p.track)));
    let (mut near, mut extra, mut own, mut far) = (near.into_iter(), extra.into_iter(), own.into_iter(), far.into_iter());
    let mut out = vec![];
    for turn in 0.. {
        let first = match turn % 6 {
            2 => own.next(),
            5 => far.next(),
            1 | 4 => extra.next().or_else(|| near.next()),
            _ => near.next().or_else(|| extra.next()),
        };
        match first.or_else(|| near.next()).or_else(|| extra.next()).or_else(|| own.next()).or_else(|| far.next()) {
            Some(p) => out.push(p),
            None => break,
        }
    }
    out
}

// ---------------------------------------------------------------- YouTube Music's radio

/// The songs YouTube Music plays after this video (its "radio" for the song), in order:
/// up to `pages` pages of about 50 (later pages hold mostly new songs).
pub fn youtube_radio(video: &str, pages: usize) -> Result<Vec<RadioItem>, String> {
    let context = serde_json::json!({ "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20241106.01.00", "hl": "en" } });
    let mut body = serde_json::json!({ "context": context.clone(), "videoId": video, "playlistId": format!("RDAMVM{video}"), "isAudioOnly": true });
    let mut out: Vec<RadioItem> = vec![];
    for page in 0..pages.max(1) {
        let mut resp = match crate::net::api().post("https://music.youtube.com/youtubei/v1/next?prettyPrint=false").header("Origin", "https://music.youtube.com").send_json(&body) {
            Ok(r) => r,
            Err(e) if page == 0 => return Err(crate::net::friendly_error(e)),
            Err(_) => break, // the first page is enough
        };
        let Ok(reply) = resp.body_mut().with_config().limit(8 * 1024 * 1024).read_json::<serde_json::Value>() else { break };
        for item in radio_items(&reply) {
            if !out.iter().any(|o| o.video == item.video) {
                out.push(item);
            }
        }
        let Some(more) = continuation(&reply) else { break };
        body = serde_json::json!({ "context": context.clone(), "continuation": more });
    }
    Ok(out)
}

/// The key for the radio's next page.
fn continuation(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Object(m) => {
            if let Some(c) = m.get("nextRadioContinuationData").and_then(|d| d["continuation"].as_str()) {
                return Some(c.to_string());
            }
            m.values().find_map(continuation)
        }
        serde_json::Value::Array(a) => a.iter().find_map(continuation),
        _ => None,
    }
}

/// Reads the songs out of YouTube Music's "up next" reply.
pub fn radio_items(reply: &serde_json::Value) -> Vec<RadioItem> {
    fn walk<'a>(v: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(item) = m.get("playlistPanelVideoRenderer") {
                    out.push(item);
                    return;
                }
                m.values().for_each(|x| walk(x, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut found = vec![];
    walk(reply, &mut found);
    let text = |v: &serde_json::Value| -> String { v["runs"].as_array().map(|r| r.iter().filter_map(|x| x["text"].as_str()).collect()).unwrap_or_default() };
    let mut seen = HashSet::new();
    found
        .into_iter()
        .filter_map(|item| {
            let video = item["videoId"].as_str()?.to_string();
            if !seen.insert(video.clone()) {
                return None;
            }
            let title = text(&item["title"]);
            // "Kendrick Lamar & SZA • All the Stars • 2018": each artist is its own run,
            // linked to their page, before the first " • ".
            let runs = item["longBylineText"]["runs"].as_array().cloned().unwrap_or_default();
            let before_dot: Vec<&serde_json::Value> = runs.iter().take_while(|r| r["text"].as_str() != Some(" • ")).collect();
            let mut artists: Vec<String> = before_dot
                .iter()
                .filter(|r| r["navigationEndpoint"]["browseEndpoint"]["browseEndpointContextSupportedConfigs"]["browseEndpointContextMusicConfig"]["pageType"] == "MUSIC_PAGE_TYPE_ARTIST")
                .filter_map(|r| r["text"].as_str().map(str::to_string))
                .collect();
            if artists.is_empty() {
                let all: String = before_dot.iter().filter_map(|r| r["text"].as_str()).collect();
                artists = all.split([',', '&']).map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
            }
            let kind = match item["navigationEndpoint"]["watchEndpoint"]["watchEndpointMusicSupportedConfigs"]["watchEndpointMusicConfig"]["musicVideoType"].as_str() {
                Some("MUSIC_VIDEO_TYPE_ATV") => Kind::Song,
                Some("MUSIC_VIDEO_TYPE_OMV") => Kind::MusicVideo,
                _ => Kind::Upload,
            };
            let duration = parse_clock(&text(&item["lengthText"]));
            let explicit = item["badges"].to_string().contains("MUSIC_EXPLICIT_BADGE");
            (!title.is_empty() && !artists.is_empty()).then_some(RadioItem { video, title, artists, duration, explicit, kind })
        })
        .collect()
}

/// "3:55" or "1:02:03" in seconds.
fn parse_clock(s: &str) -> Option<u32> {
    let mut total = 0;
    for part in s.trim().split(':') {
        total = total * 60 + part.parse::<u32>().ok()?;
    }
    s.contains(':').then_some(total)
}

/// Finds the Deezer song for each radio entry (a few at a time), keeping the radio's order
/// and stopping once `want` are found. Fan uploads are left out: they're edits and remixes
/// more often than songs.
fn match_to_deezer(items: Vec<RadioItem>, want: usize) -> Vec<Pick> {
    let items: Vec<RadioItem> = items.into_iter().filter(|i| i.kind != Kind::Upload).collect();
    let found: Mutex<Vec<(usize, Pick)>> = Mutex::new(vec![]);
    let next = AtomicUsize::new(0);
    let count = AtomicUsize::new(0);
    // Four at a time: quick, and inside Deezer's 50 requests per 5 seconds (a busy reply is retried).
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                loop {
                    if count.load(Ordering::Relaxed) >= want {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { return };
                    if let Some(pick) = match_one(item) {
                        count.fetch_add(1, Ordering::Relaxed);
                        found.lock().unwrap_or_else(|e| e.into_inner()).push((i, pick));
                    }
                }
            });
        }
    });
    let mut found = found.into_inner().unwrap_or_else(|e| e.into_inner());
    found.sort_by_key(|(i, _)| *i);
    found.into_iter().map(|(_, p)| p).collect()
}

fn match_one(item: &RadioItem) -> Option<Pick> {
    let track = deezer::find_track(&item.artists.join(", "), &item.title, item.duration.unwrap_or(0)).ok().flatten()?;
    // YouTube Music's album audio is the right recording if it's the same version (explicit
    // or clean) and the same length; then the song can start without searching YouTube.
    let same_length = item.duration.is_some_and(|d| track.duration == 0 || d.abs_diff(track.duration) <= 3);
    let video = (item.kind == Kind::Song && same_length && item.explicit == track.explicit).then(|| item.video.clone());
    Some(Pick { track, video })
}

// ---------------------------------------------------------------- Deezer, when YouTube Music can't help

/// Songs like `seed` from Deezer: the artist's radio, mixed with the best-known songs of
/// similar artists.
pub fn deezer_radio(seed: &Track) -> Vec<Track> {
    let radio = deezer::similar_to(seed).unwrap_or_default();
    let ids: Vec<String> = similar_artists(seed).into_iter().take(6).map(|a| a.id).collect();
    let related = deezer::top_songs_of(&ids, 3);
    // Two from the radio, then one from a similar artist, and so on.
    let mut out = vec![];
    let (mut a, mut b) = (radio.into_iter(), related.into_iter());
    loop {
        let x = [a.next(), a.next(), b.next()];
        if x.iter().all(Option::is_none) {
            break;
        }
        out.extend(x.into_iter().flatten());
    }
    out
}

// ---------------------------------------------------------------- tidying

/// The picks to play, in order: what's left after the rules at the top of this file.
pub fn tidy(seed: &Track, picks: Vec<Pick>, avoid: &Avoid) -> Vec<Pick> {
    let seed_artist = artist_key(seed);
    let seed_key = song_key(seed);
    let mut seen_ids = HashSet::new();
    let mut seen_songs = HashSet::new();
    let mut per_artist: HashMap<String, usize> = HashMap::new();
    let kept: Vec<Pick> = picks
        .into_iter()
        .filter(|p| {
            let t = &p.track;
            let key = song_key(t);
            !t.id.is_empty()
                && t.id != seed.id
                && key != seed_key
                && !avoid.contains(t)
                && !unwanted_version(&seed.title, &t.title)
                && !(mashup(&t.title) && !mashup(&seed.title))
                && !deezer::cover_act(&t.artist.name)
                && seen_ids.insert(t.id.clone())
                && seen_songs.insert(key)
        })
        .collect();
    // A few songs per artist. The artist you picked comes back now and then (about one song
    // in eight), but autoplay is for hearing other artists like them.
    let seed_cap = (kept.len() / 8).clamp(2, 4);
    let kept: Vec<Pick> = kept
        .into_iter()
        .filter(|p| {
            let a = artist_key(&p.track);
            let cap = if a == seed_artist { seed_cap } else { 2 };
            let n = per_artist.entry(a).or_default();
            *n += 1;
            *n <= cap
        })
        .collect();
    spread_out(kept, &seed_artist)
}

/// Two songs glued together by a fan ("Get busy x gasolina", "Pasto Pastilla Botella X
/// Calabria"): they fill up radios of viral songs but aren't what you'd pick.
fn mashup(title: &str) -> bool {
    let w = crate::youtube::words(title);
    w.contains(" x ") || w.contains(" mashup ") || w.contains(" aftermix ")
}

/// Songs between two by the artist you picked (and before the first of them).
const SEED_ARTIST_GAP: usize = 4;

/// Reorders as little as possible so that no artist plays twice in a row, and the artist
/// you picked (`seed_artist`, who just played) waits: not in the first three songs, then at
/// most one song in four. A song that has to wait goes in as soon as it can; one that never
/// fits is left out.
fn spread_out(picks: Vec<Pick>, seed_artist: &str) -> Vec<Pick> {
    let mut out: Vec<Pick> = Vec::with_capacity(picks.len());
    let mut waiting: Vec<Pick> = vec![];
    let mut last = seed_artist.to_string();
    // Where the artist you picked last played: the song itself, just before the list.
    let mut last_seed: isize = -1;
    let fits = |p: &Pick, out_len: usize, last: &str, last_seed: isize| {
        let a = artist_key(&p.track);
        a != last && (a != seed_artist || out_len as isize - last_seed >= SEED_ARTIST_GAP as isize)
    };
    let place = |p: Pick, out: &mut Vec<Pick>, last: &mut String, last_seed: &mut isize| {
        *last = artist_key(&p.track);
        if *last == seed_artist {
            *last_seed = out.len() as isize;
        }
        out.push(p);
    };
    for p in picks {
        // Songs that had to wait go in first, as soon as they fit.
        while let Some(i) = waiting.iter().position(|w| fits(w, out.len(), &last, last_seed)) {
            let w = waiting.remove(i);
            place(w, &mut out, &mut last, &mut last_seed);
        }
        if fits(&p, out.len(), &last, last_seed) {
            place(p, &mut out, &mut last, &mut last_seed);
        } else {
            waiting.push(p);
        }
    }
    while let Some(i) = waiting.iter().position(|w| fits(w, out.len(), &last, last_seed)) {
        let w = waiting.remove(i);
        place(w, &mut out, &mut last, &mut last_seed);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, title: &str, artist: &str) -> Track {
        let mut t = Track { id: id.into(), title: title.into(), duration: 200, ..Default::default() };
        t.artist.id = format!("a-{}", norm(artist));
        t.artist.name = artist.into();
        t
    }
    fn picks(list: &[(&str, &str, &str)]) -> Vec<Pick> {
        list.iter().map(|(id, title, artist)| Pick { track: song(id, title, artist), video: None }).collect()
    }
    fn artists(p: &[Pick]) -> Vec<String> {
        p.iter().map(|p| p.track.artist.name.clone()).collect()
    }

    #[test]
    fn reads_youtube_musics_up_next_list() {
        let reply = serde_json::json!({ "contents": { "x": [
            { "playlistPanelVideoRenderer": {
                "videoId": "seed1", "title": { "runs": [{ "text": "Pink + White" }] },
                "longBylineText": { "runs": [
                    { "text": "Frank Ocean", "navigationEndpoint": { "browseEndpoint": { "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_ARTIST" } } } } },
                    { "text": " • " }, { "text": "Blonde" }, { "text": " • " }, { "text": "2016" } ] },
                "lengthText": { "runs": [{ "text": "3:04" }] },
                "navigationEndpoint": { "watchEndpoint": { "watchEndpointMusicSupportedConfigs": { "watchEndpointMusicConfig": { "musicVideoType": "MUSIC_VIDEO_TYPE_ATV" } } } } } },
            { "playlistPanelVideoRenderer": {
                "videoId": "v2", "title": { "runs": [{ "text": "All The Stars" }] },
                "longBylineText": { "runs": [
                    { "text": "Kendrick Lamar", "navigationEndpoint": { "browseEndpoint": { "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_ARTIST" } } } } },
                    { "text": " & " },
                    { "text": "SZA", "navigationEndpoint": { "browseEndpoint": { "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_ARTIST" } } } } },
                    { "text": " • " }, { "text": "All the Stars" }, { "text": " • " }, { "text": "2018" } ] },
                "lengthText": { "runs": [{ "text": "3:52" }] },
                "badges": [{ "musicInlineBadgeRenderer": { "icon": { "iconType": "MUSIC_EXPLICIT_BADGE" } } }],
                "navigationEndpoint": { "watchEndpoint": { "watchEndpointMusicSupportedConfigs": { "watchEndpointMusicConfig": { "musicVideoType": "MUSIC_VIDEO_TYPE_ATV" } } } } } },
            { "playlistPanelVideoRenderer": {
                "videoId": "v3", "title": { "runs": [{ "text": "Suavemente x Y Que Fue (EDIT)" }] },
                "longBylineText": { "runs": [{ "text": "SelectaKilla" }, { "text": " • " }, { "text": "2.3M views" }] },
                "lengthText": { "runs": [{ "text": "2:41" }] },
                "navigationEndpoint": { "watchEndpoint": { "watchEndpointMusicSupportedConfigs": { "watchEndpointMusicConfig": { "musicVideoType": "MUSIC_VIDEO_TYPE_UGC" } } } } } },
            { "playlistPanelVideoRenderer": { "videoId": "v2", "title": { "runs": [{ "text": "All The Stars" }] } } }
        ] } });
        let items = radio_items(&reply);
        assert_eq!(items.len(), 3, "the repeat of v2 is left out: {items:?}");
        assert_eq!(items[0], RadioItem { video: "seed1".into(), title: "Pink + White".into(), artists: vec!["Frank Ocean".into()], duration: Some(184), explicit: false, kind: Kind::Song });
        assert_eq!(items[1].artists, ["Kendrick Lamar", "SZA"]);
        assert!(items[1].explicit);
        assert_eq!(items[2].kind, Kind::Upload);
        assert_eq!(items[2].artists, ["SelectaKilla"], "no artist links: the byline's names");
    }

    #[test]
    fn leaves_out_what_you_just_heard_and_other_versions() {
        let seed = song("dz:1", "Pink + White", "Frank Ocean");
        let mut avoid = Avoid::default();
        avoid.add(&song("dz:9", "Kill Bill", "SZA"));
        let out = tidy(
            &seed,
            picks(&[
                ("dz:2", "Pink + White (Live)", "Frank Ocean"),
                ("dz:3", "Pink + White", "Frank Ocean"),
                ("dz:4", "Kill Bill (Sped Up)", "SZA"),
                ("dz:5", "Kill Bill", "SZA"),
                ("dz:6", "Best Part (feat. H.E.R.)", "Daniel Caesar"),
                ("dz:7", "Best Part", "Daniel Caesar"),
                ("dz:8", "Superpowers", "Daniel Caesar"),
                ("dz:10", "Location", "Made Famous by Khalid"),
                ("dz:11", "Location", "Khalid"),
                ("dz:12", "Get busy x gasolina", "studiojenna"),
            ]),
            &avoid,
        );
        let titles: Vec<&str> = out.iter().map(|p| p.track.title.as_str()).collect();
        assert_eq!(titles, ["Best Part (feat. H.E.R.)", "Location", "Superpowers"]);
    }

    #[test]
    fn no_artist_twice_in_a_row_and_a_few_each() {
        let seed = song("dz:0", "Pink + White", "Frank Ocean");
        let out = tidy(
            &seed,
            picks(&[
                ("1", "Ivy", "Frank Ocean"),
                ("2", "Superpowers", "Daniel Caesar"),
                ("3", "Best Part", "Daniel Caesar"),
                ("4", "Get You", "Daniel Caesar"),
                ("5", "Bad Habit", "Steve Lacy"),
                ("6", "Nights", "Frank Ocean"),
                ("7", "Location", "Khalid"),
            ]),
            &Avoid::default(),
        );
        let who = artists(&out);
        assert!(who.windows(2).all(|w| w[0] != w[1]), "{who:?}");
        assert_eq!(who.iter().filter(|a| *a == "Daniel Caesar").count(), 2, "{who:?}");
        // You picked a Frank Ocean song: other artists first, then Frank Ocean now and then.
        assert!(!who[..3].contains(&"Frank Ocean".to_string()), "{who:?}");
        assert_eq!(who.iter().filter(|a| *a == "Frank Ocean").count(), 1, "{who:?}");
        assert_eq!(who, ["Daniel Caesar", "Steve Lacy", "Daniel Caesar", "Frank Ocean", "Khalid"]);
    }

    #[test]
    fn the_artist_you_picked_is_one_song_in_four_at_most() {
        let seed = song("dz:0", "Pink + White", "Frank Ocean");
        // A radio that's half Frank Ocean.
        let list: Vec<(String, String, String)> = (0..40)
            .map(|i| if i % 2 == 0 { (format!("f{i}"), format!("Frank song {i}"), "Frank Ocean".to_string()) } else { (format!("o{i}"), format!("Other song {i}"), format!("Artist {}", i / 2)) })
            .collect();
        let refs: Vec<(&str, &str, &str)> = list.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
        let out = tidy(&seed, picks(&refs), &Avoid::default());
        let frank: Vec<usize> = out.iter().enumerate().filter(|(_, p)| p.track.artist.name == "Frank Ocean").map(|(i, _)| i).collect();
        assert!(frank.len() <= 4 && !frank.is_empty(), "{frank:?}");
        assert!(frank[0] >= 3, "not in the first three: {frank:?}");
        assert!(frank.windows(2).all(|w| w[1] - w[0] >= SEED_ARTIST_GAP), "spaced out: {frank:?}");
        assert!(out.len() >= 20, "the other artists all stay: {}", out.len());
    }

    #[test]
    fn artists_like_yours_first_and_others_now_and_then() {
        // You picked a song by S. A, B and E are like S; X and Y aren't.
        let close: HashSet<String> = ["a", "b", "e"].iter().map(|s| s.to_string()).collect();
        let radio = picks(&[("x1", "Pop hit", "X"), ("a1", "Song a", "A"), ("s1", "Song s", "S"), ("y1", "Other hit", "Y"), ("b1", "Song b", "B"), ("x2", "Pop hit 2", "X")]);
        let extra = picks(&[("e1", "Song e1", "E"), ("e2", "Song e2", "E")]);
        let order: Vec<String> = close_first(radio, extra, &close, "s").into_iter().map(|p| p.track.id).collect();
        // Like-S, like-S, S, like-S, like-S, then one from further afield; the rest after.
        assert_eq!(order, ["a1", "e1", "s1", "b1", "e2", "x1", "y1", "x2"]);
    }

    #[test]
    fn finds_the_next_page_of_the_radio() {
        let reply = serde_json::json!({ "contents": [{ "x": { "continuations": [{ "nextRadioContinuationData": { "continuation": "abc" } }] } }] });
        assert_eq!(continuation(&reply).as_deref(), Some("abc"));
        assert_eq!(continuation(&serde_json::json!({ "a": 1 })), None);
    }

    #[test]
    fn same_song_whatever_the_version() {
        assert_eq!(song_key(&song("1", "Get Lucky (Radio Edit - feat. Pharrell Williams)", "Daft Punk")), song_key(&song("2", "Get Lucky", "Daft Punk")));
        assert_ne!(song_key(&song("1", "Get Lucky", "Daft Punk")), song_key(&song("2", "Get Lucky", "Someone Else")));
    }

    #[test]
    fn reads_clock_times() {
        assert_eq!(parse_clock("3:55"), Some(235));
        assert_eq!(parse_clock("1:02:03"), Some(3723));
        assert_eq!(parse_clock("live"), None);
    }

    /// Prints what autoplay plays after a few songs, from the live services, next to what
    /// it played before (the artist's Deezer radio as it came):
    /// `cargo test radio_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn radio_live() {
        let songs = std::env::var("PULSE_RADIO_SONGS").unwrap_or_else(|_| "Frank Ocean|Pink + White;Metallica|Enter Sandman;Bad Bunny|Tití Me Preguntó;Billie Eilish|BIRDS OF A FEATHER;Daft Punk|Get Lucky".into());
        for pair in songs.split(';') {
            let (artist, title) = pair.split_once('|').unwrap();
            let Some(seed) = deezer::find_track(artist, title, 0).unwrap() else {
                println!("\n== “{title}” by {artist} isn't on Deezer; skipped");
                continue;
            };
            let started = std::time::Instant::now();
            let out = for_song(&seed, None, &Avoid::default());
            let took = started.elapsed().as_secs_f32();
            let old: Vec<Track> = deezer::similar_to(&seed).unwrap_or_default().into_iter().take(15).collect();
            println!("\n== after “{}” by {}: {} songs in {took:.1}s, {} with YouTube Music's audio", seed.title, seed.artist.name, out.len(), out.iter().filter(|p| p.video.is_some()).count());
            println!("  {:<46} | {}", "NOW (first 15)", "BEFORE (Deezer artist radio, first 15)");
            for i in 0..15 {
                let now = out.get(i).map(|p| format!("{} — {}", p.track.title, p.track.artist.name)).unwrap_or_default();
                let was = old.get(i).map(|t| format!("{} — {}", t.title, t.artist.name)).unwrap_or_default();
                println!("  {:<46} | {}", now.chars().take(46).collect::<String>(), was);
            }
            assert!(out.len() >= 10);
        }
    }
}
