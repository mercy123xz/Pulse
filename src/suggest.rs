//! What the search box suggests while you type, ranked the way music apps do it:
//!
//! 1. Searches to finish what you're typing: your own past searches first, then what
//!    other people search for (YouTube's public suggestions), then names that go with
//!    the best match ("osamason psykotic").
//! 2. The artist you most likely mean, then a mix of their albums, songs and playlists,
//!    scored by how well they match, how popular they are, and what's in your library
//!    (songs you liked or played and albums or artists you saved come first).

use crate::deezer::{AlbumInfo, ArtistInfo, PlaylistInfo, SearchResults, Track, name_match};
use std::collections::{HashMap, HashSet};

/// What's in your library, for ranking.
#[derive(Default)]
pub struct Library<'a> {
    pub liked_songs: HashSet<&'a str>,
    pub liked_albums: HashSet<&'a str>,
    pub liked_playlists: HashSet<&'a str>,
    pub followed: HashSet<&'a str>,
    /// Artist id → how often you've played them.
    pub played_artists: HashMap<&'a str, u32>,
    pub recent_songs: HashSet<&'a str>,
}

/// One row under the search box.
#[derive(Debug, Clone)]
pub enum Pick {
    Artist(ArtistInfo),
    Album(AlbumInfo),
    Playlist(PlaylistInfo),
    Song(Box<Track>),
}

impl Pick {
    /// Is this saved in your library (liked, followed)?
    pub fn saved(&self, lib: &Library) -> bool {
        match self {
            Pick::Artist(a) => lib.followed.contains(a.id.as_str()),
            Pick::Album(a) => lib.liked_albums.contains(a.id.as_str()),
            Pick::Playlist(p) => lib.liked_playlists.contains(p.id.as_str()),
            Pick::Song(t) => lib.liked_songs.contains(t.id.as_str()),
        }
    }
}

/// Lowercase with single spaces, for comparing searches.
fn key(s: &str) -> String {
    crate::app::fold(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Up to `max` searches that finish what was typed. What you typed comes first, then your
/// own past searches, then popular ones, then ones made from the best results.
pub fn completions(typed: &str, recent: &[String], popular: &[String], results: Option<&SearchResults>, max: usize) -> Vec<String> {
    let q = key(typed);
    if q.is_empty() {
        return vec![];
    }
    let mut out: Vec<String> = vec![typed.trim().to_string()];
    let mut seen: HashSet<String> = HashSet::from([q.clone()]);
    let mut add = |s: String, out: &mut Vec<String>| {
        let k = key(&s);
        if !k.is_empty() && k.starts_with(&q) && seen.insert(k) {
            out.push(s);
        }
    };
    for r in recent {
        add(r.trim().to_lowercase(), &mut out);
    }
    for p in popular {
        add(p.trim().to_lowercase(), &mut out);
    }
    // From the results: the artist's name, then the artist with their best-known albums and songs.
    if let Some(r) = results
        && let Some(a) = r.artists.first().filter(|a| name_match(&a.name, typed) >= 2)
    {
        let name = a.name.to_lowercase();
        add(name.clone(), &mut out);
        for al in r.albums.iter().filter(|al| al.artist.id == a.id).take(3) {
            add(format!("{name} {}", al.title.to_lowercase()), &mut out);
        }
        for t in r.tracks.iter().filter(|t| t.artist.id == a.id).take(3) {
            add(format!("{name} {}", t.title.to_lowercase()), &mut out);
        }
    }
    out.truncate(max);
    out
}

/// Reads YouTube's suggestion reply: `["typed",["one","two",...], ...]`, or the same
/// wrapped in a function call (`window.google.ac.h([...])`).
pub fn parse_popular(body: &str) -> Vec<String> {
    let start = body.find('[');
    let end = body.rfind(']');
    let (Some(a), Some(b)) = (start, end) else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&body[a..=b]) else { return vec![] };
    let Some(list) = v.get(1).and_then(|l| l.as_array()) else { return vec![] };
    list.iter()
        .filter_map(|x| x.as_str().map(str::to_owned).or_else(|| x.get(0).and_then(|y| y.as_str()).map(str::to_owned)))
        .filter(|s| !s.trim().is_empty())
        .take(10)
        .collect()
}

/// What other people search for that starts with `typed` (YouTube's public suggestions).
/// Empty if the service can't be reached.
pub fn popular(typed: &str) -> Vec<String> {
    let url = format!("https://suggestqueries.google.com/complete/search?client=firefox&ds=yt&hl=en&q={}", crate::net::encode(typed.trim()));
    crate::net::get_page(&url).map(|b| parse_popular(&b)).unwrap_or_default()
}

/// The rows under the completions: the artist you most likely mean, then up to `max - 1`
/// albums, songs and playlists, best first.
pub fn picks(typed: &str, r: &SearchResults, lib: &Library, max: usize) -> Vec<Pick> {
    let mut out = vec![];
    // The top artist: ranked by name and fans already; artists you follow or play win ties.
    let mut artists: Vec<&ArtistInfo> = r.artists.iter().filter(|a| name_match(&a.name, typed) >= 2).collect();
    let artist_score = |a: &ArtistInfo| {
        let mut s = ((a.fans + 1) as f64).log10();
        if lib.followed.contains(a.id.as_str()) {
            s += 3.0;
        }
        if let Some(n) = lib.played_artists.get(a.id.as_str()) {
            s += 1.0 + (*n as f64).ln_1p();
        }
        s + name_match(&a.name, typed) as f64
    };
    artists.sort_by(|a, b| artist_score(b).total_cmp(&artist_score(a)));
    let top = artists.first().copied();
    if let Some(a) = top {
        out.push(Pick::Artist(a.clone()));
    }
    let top_id = top.map(|a| a.id.as_str());

    let mut scored: Vec<(f64, Pick)> = vec![];
    for (i, al) in r.albums.iter().enumerate() {
        let by_top = top_id == Some(al.artist.id.as_str());
        let m = name_match(&al.title, typed).max(name_match(&al.artist.name, typed));
        if m == 0 && !by_top {
            continue;
        }
        let mut s = m as f64 + if by_top { 3.0 } else { 0.0 } + (1.0 - i as f64 / 30.0).max(0.0) * 1.5;
        if al.record_type == "album" {
            s += 0.5;
        }
        if lib.liked_albums.contains(al.id.as_str()) {
            s += 4.0;
        }
        scored.push((s, Pick::Album(al.clone())));
    }
    for t in &r.tracks {
        let by_top = top_id == Some(t.artist.id.as_str());
        let m = name_match(&t.title, typed).max(name_match(&t.artist.name, typed));
        if m == 0 && !by_top {
            continue;
        }
        let mut s = m as f64 + if by_top { 2.0 } else { 0.0 } + ((t.rank + 1) as f64).log10() / 2.5;
        if lib.liked_songs.contains(t.id.as_str()) {
            s += 4.0;
        }
        if lib.recent_songs.contains(t.id.as_str()) {
            s += 2.0;
        }
        scored.push((s, Pick::Song(Box::new(t.clone()))));
    }
    for (i, p) in r.playlists.iter().enumerate() {
        let m = name_match(&p.title, typed);
        if m == 0 {
            continue;
        }
        // Playlists rank below the artist's own music unless you saved them.
        let mut s = m as f64 + 1.0 - i as f64 / 12.0;
        if lib.liked_playlists.contains(p.id.as_str()) {
            s += 5.0;
        }
        scored.push((s, Pick::Playlist(p.clone())));
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    // A mix: at most three of any kind.
    let mut per_kind: HashMap<u8, usize> = HashMap::new();
    for (_, p) in scored {
        if out.len() >= max {
            break;
        }
        let kind = match p {
            Pick::Artist(_) => 0,
            Pick::Album(_) => 1,
            Pick::Playlist(_) => 2,
            Pick::Song(_) => 3,
        };
        let n = per_kind.entry(kind).or_default();
        if *n < 3 {
            *n += 1;
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deezer::ArtistRef;

    fn artist(id: &str, name: &str, fans: u64) -> ArtistInfo {
        ArtistInfo { id: id.into(), name: name.into(), fans, ..Default::default() }
    }
    fn album(id: &str, title: &str, by: &ArtistInfo, kind: &str) -> AlbumInfo {
        AlbumInfo {
            id: id.into(),
            title: title.into(),
            artist: ArtistRef { id: by.id.clone(), name: by.name.clone(), picture: None },
            record_type: kind.into(),
            ..Default::default()
        }
    }
    fn song(id: &str, title: &str, by: &ArtistInfo, rank: u64) -> Track {
        let mut t = Track { id: id.into(), title: title.into(), rank, ..Default::default() };
        t.artist.id = by.id.clone();
        t.artist.name = by.name.clone();
        t
    }

    fn results() -> SearchResults {
        let osama = artist("1", "OsamaSon", 21_088);
        let kifoh = artist("2", "kifoh", 40);
        SearchResults {
            artists: vec![osama.clone(), artist("3", "Osaka Monaurail", 1_666)],
            albums: vec![
                album("a1", "psykotic", &osama, "album"),
                album("a2", "Osama Season", &osama, "album"),
                album("a3", "Flex Musix", &osama, "album"),
                album("a4", "Osama", &kifoh, "single"),
            ],
            tracks: vec![song("t1", "Lil O", &osama, 181_336), song("t2", "Troops", &osama, 291_182)],
            playlists: vec![PlaylistInfo { id: "p1".into(), title: "OsamaSon mix".into(), ..Default::default() }],
            ..Default::default()
        }
    }

    #[test]
    fn the_artist_comes_first_then_a_mix_of_their_music() {
        let r = results();
        let picks = picks("osamason", &r, &Library::default(), 6);
        assert!(matches!(&picks[0], Pick::Artist(a) if a.name == "OsamaSon"));
        let albums = picks.iter().filter(|p| matches!(p, Pick::Album(_))).count();
        assert!(albums <= 3);
        assert!(picks.iter().all(|p| !matches!(p, Pick::Album(a) if a.artist.name == "kifoh")), "kifoh's 'Osama' doesn't match 'osamason'");
    }

    #[test]
    fn your_library_comes_first() {
        let r = results();
        let lib = Library { liked_songs: HashSet::from(["t1"]), ..Default::default() };
        let picks = picks("osamason", &r, &lib, 6);
        assert!(matches!(&picks[1], Pick::Song(t) if t.id == "t1"), "the liked song is right after the artist: {picks:?}");
        assert!(picks[1].saved(&lib));
    }

    #[test]
    fn completions_put_yours_first_then_popular_then_the_artists_music() {
        let r = results();
        let recent = vec!["osamason setlist".to_string(), "drake".to_string()];
        let popular = vec!["osamason unreleased".to_string(), "OsamaSon Setlist".to_string(), "something else".to_string()];
        let c = completions("osamason", &recent, &popular, Some(&r), 6);
        assert_eq!(c[0], "osamason");
        assert_eq!(c[1], "osamason setlist");
        assert_eq!(c[2], "osamason unreleased");
        assert!(c.contains(&"osamason psykotic".to_string()));
        assert_eq!(c.iter().filter(|s| s.contains("setlist")).count(), 1, "no repeats");
        assert!(completions("  ", &recent, &popular, None, 4).is_empty());
    }

    #[test]
    fn reads_the_popular_searches_reply() {
        assert_eq!(parse_popular(r#"["yeat",["yeat bell","yeat unreleased"],[],{}]"#), ["yeat bell", "yeat unreleased"]);
        assert_eq!(parse_popular(r#"window.google.ac.h(["hxg",[["hxg 5g",0],["hxg lifestyle",0]],{}])"#), ["hxg 5g", "hxg lifestyle"]);
        assert!(parse_popular("<html>blocked</html>").is_empty());
    }
}
