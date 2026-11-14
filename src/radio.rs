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
pub fn for_song(seed: &Track, video: Option<String>, avoid: &Avoid) -> Vec<Pick> {
    let video = video.or_else(|| crate::youtube::music_video(seed));
    let mut picks = match video.as_deref().map(youtube_radio) {
        Some(Ok(items)) => match_to_deezer(items.into_iter().filter(|i| Some(&i.video) != video.as_ref()).collect(), 32),
        _ => vec![],
    };
    // YouTube Music had little or nothing: fill up from Deezer.
    if picks.len() < 12 {
        picks.extend(deezer_radio(seed).into_iter().map(|track| Pick { track, video: None }));
    }
    tidy(seed, picks, avoid)
}

// ---------------------------------------------------------------- YouTube Music's radio

/// The songs YouTube Music plays after this video (its "radio" for the song), in order.
pub fn youtube_radio(video: &str) -> Result<Vec<RadioItem>, String> {
    let body = serde_json::json!({
        "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20241106.01.00", "hl": "en" } },
        "videoId": video,
        "playlistId": format!("RDAMVM{video}"),
        "isAudioOnly": true,
    });
    let mut resp = crate::net::api()
        .post("https://music.youtube.com/youtubei/v1/next?prettyPrint=false")
        .header("Origin", "https://music.youtube.com")
        .send_json(&body)
        .map_err(crate::net::friendly_error)?;
    let reply: serde_json::Value =
        resp.body_mut().with_config().limit(8 * 1024 * 1024).read_json().map_err(|e| format!("Unexpected YouTube Music reply ({e})"))?;
    Ok(radio_items(&reply))
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
    let artist_id = if seed.has_artist_page() { Some(seed.artist.id.clone()) } else { radio.iter().find(|t| norm(&t.artist.name) == norm(&seed.artist.name)).map(|t| t.artist.id.clone()) };
    let related = artist_id.map(|id| deezer::related_top_songs(&id, 6, 3)).unwrap_or_default();
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
    // A few songs per artist: more of the artist you picked than of anyone else.
    let seed_cap = (kept.len() / 6).max(3);
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

/// Reorders as little as possible so no artist plays twice in a row (the first song isn't
/// by `after`'s artist either, as that's who just played). A song that has to wait goes in
/// as soon as it can.
fn spread_out(picks: Vec<Pick>, after: &str) -> Vec<Pick> {
    let mut out: Vec<Pick> = Vec::with_capacity(picks.len());
    let mut waiting: Vec<Pick> = vec![];
    let mut last = after.to_string();
    for p in picks {
        // Something that had to wait can go now?
        if let Some(i) = waiting.iter().position(|w| artist_key(&w.track) != last) {
            let w = waiting.remove(i);
            last = artist_key(&w.track);
            out.push(w);
        }
        if artist_key(&p.track) == last {
            waiting.push(p);
        } else {
            last = artist_key(&p.track);
            out.push(p);
        }
    }
    // The rest, wherever they fit; one that can't go anywhere is dropped.
    while let Some(i) = waiting.iter().position(|w| artist_key(&w.track) != last) {
        let w = waiting.remove(i);
        last = artist_key(&w.track);
        out.push(w);
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
        assert_ne!(who[0], "Frank Ocean", "the song you picked was Frank Ocean's: someone else first");
        assert!(who.windows(2).all(|w| w[0] != w[1]), "{who:?}");
        assert_eq!(who.iter().filter(|a| *a == "Daniel Caesar").count(), 2, "{who:?}");
        assert_eq!(who.iter().filter(|a| *a == "Frank Ocean").count(), 2, "{who:?}");
        // The radio's order is kept as far as it can be.
        assert_eq!(who[..3], ["Daniel Caesar", "Frank Ocean", "Daniel Caesar"]);
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
