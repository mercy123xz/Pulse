//! Your taste, for ranking search results the way Spotify does: what you typed decides the
//! order first, and your taste only reorders results that match it equally well.
//!
//! Built on your PC from what Pulse already saves: artists you play (recent plays count
//! more), songs and albums you like or played, artists you follow, the genres of your
//! music, and artists like your top ones (Deezer's related artists, fetched once a day).
//! Nothing is uploaded.

use crate::app::{Place, Saved, Visit, now_unix};
use crate::deezer::{AlbumInfo, ArtistInfo, PlaylistInfo, SearchResults, Track, name_match};
use std::collections::{HashMap, HashSet};

/// Related artists are looked up for this many of your top artists.
pub const RELATED_FROM: usize = 10;
/// How much liking an artist counts against their name match and fans (see `rerank_artists`):
/// following someone (1) is worth about 10,000 times the fans.
pub const ARTIST_WEIGHT: f64 = 4.0;
/// How often related artists are looked up again.
pub const RELATED_EVERY: u64 = 86_400;

#[derive(Debug, Default, Clone)]
pub struct Taste {
    pub liked_songs: HashSet<String>,
    pub liked_albums: HashSet<String>,
    pub liked_playlists: HashSet<String>,
    pub followed: HashSet<String>,
    pub recent_songs: HashSet<String>,
    /// Songs in your own playlists.
    pub playlist_songs: HashSet<String>,
    /// Artist id → how much you like them (0 to 2).
    artists: HashMap<String, f64>,
    /// Album id → played or liked (0 to 1).
    albums: HashMap<String, f64>,
    /// Genre → share of your music (0 to 1), and the genres of albums known so far.
    genres: HashMap<String, f64>,
    album_genres: HashMap<String, Vec<String>>,
}

/// Plays fade with time: a play two weeks ago counts half.
fn faded(v: &Visit, now: u64) -> f64 {
    let days = now.saturating_sub(v.last) as f64 / 86_400.0;
    v.count as f64 / (1.0 + days / 14.0)
}

/// `n` on a 0–1 scale against the biggest `max`, so a few plays already count.
fn scale(n: f64, max: f64) -> f64 {
    if max <= 0.0 { 0.0 } else { (n.ln_1p() / max.ln_1p()).min(1.0) }
}

impl Taste {
    pub fn build(s: &Saved, now: u64) -> Taste {
        let mut t = Taste {
            liked_songs: s.liked.iter().map(|x| x.id.clone()).collect(),
            liked_albums: s.liked_albums.iter().map(|a| a.id.clone()).collect(),
            liked_playlists: s.liked_playlists.iter().map(|p| p.id.clone()).collect(),
            followed: s.followed_artists.iter().map(|a| a.id.clone()).collect(),
            recent_songs: s.recent.iter().map(|x| x.id.clone()).collect(),
            playlist_songs: s.playlists.iter().flat_map(|p| p.tracks.iter().map(|x| x.id.clone())).collect(),
            ..Default::default()
        };

        // Artists: plays, likes, follows, recent songs; artists like your top ones a little.
        let plays: Vec<(&str, f64)> = s.played_artists.iter().filter_map(|v| match &v.place {
            Place::Artist(id) => Some((id.as_str(), faded(v, now))),
            _ => None,
        }).collect();
        let most_played = plays.iter().map(|p| p.1).fold(0.0, f64::max);
        let mut liked_by: HashMap<&str, f64> = HashMap::new();
        for x in &s.liked {
            *liked_by.entry(x.artist.id.as_str()).or_default() += 1.0;
        }
        let most_liked = liked_by.values().copied().fold(0.0, f64::max);
        let mut add = |id: &str, n: f64| *t.artists.entry(id.to_string()).or_default() += n;
        for (id, n) in &plays {
            add(id, scale(*n, most_played));
        }
        for (id, n) in &liked_by {
            add(id, 0.8 * scale(*n, most_liked));
        }
        for a in &s.followed_artists {
            add(&a.id, 1.0);
        }
        for x in &s.recent {
            add(&x.artist.id, 0.05);
        }
        // Artists in your playlists: a song you put there counts, a few of them more.
        for x in s.playlists.iter().flat_map(|p| &p.tracks) {
            add(&x.artist.id, 0.2);
        }
        for (id, w) in &s.taste_related {
            let e = t.artists.entry(id.clone()).or_default();
            *e = e.max(0.4 * w);
        }
        t.artists.retain(|id, n| {
            *n = n.min(2.0);
            !id.is_empty()
        });

        // Albums: liked, or played (more plays, more).
        let album_plays: Vec<(&str, f64)> = s.played_albums.iter().filter_map(|v| match &v.place {
            Place::Album(id) => Some((id.as_str(), faded(v, now))),
            _ => None,
        }).collect();
        let most = album_plays.iter().map(|p| p.1).fold(0.0, f64::max);
        for (id, n) in album_plays {
            t.albums.insert(id.to_string(), 0.6 * scale(n, most));
        }
        for a in &s.liked_albums {
            t.albums.insert(a.id.clone(), 1.0);
        }

        // Genres: how much of what you like and play is in each.
        let mut count: HashMap<&str, f64> = HashMap::new();
        for x in s.liked.iter().chain(&s.recent) {
            for g in crate::app::track_genres(x, &s.album_genres) {
                *count.entry(g.as_str()).or_default() += 1.0;
            }
        }
        let most = count.values().copied().fold(0.0, f64::max);
        t.genres = count.into_iter().map(|(g, n)| (g.to_string(), n / most)).collect();
        // Only what can be scored: genres of albums whose genres you have.
        if !t.genres.is_empty() {
            t.album_genres = s.album_genres.iter().filter(|(_, g)| !g.is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect();
        }
        t
    }

    /// The artists to look up related artists for: your top ones, most liked first.
    pub fn top_artists(&self, n: usize) -> Vec<String> {
        let mut v: Vec<(&String, &f64)> = self.artists.iter().filter(|(id, _)| crate::deezer::is_deezer_id(id)).collect();
        v.sort_by(|a, b| b.1.total_cmp(a.1).then(a.0.cmp(b.0)));
        v.into_iter().take(n).map(|(id, _)| id.clone()).collect()
    }

    pub fn artist(&self, id: &str) -> f64 {
        self.artists.get(id).copied().unwrap_or(0.0)
    }

    pub fn album(&self, id: &str) -> f64 {
        self.albums.get(id).copied().unwrap_or(0.0) + if self.liked_albums.contains(id) { 1.0 } else { 0.0 }
    }

    fn genre(&self, album_id: &str) -> f64 {
        self.album_genres.get(album_id).map_or(0.0, |gs| gs.iter().filter_map(|g| self.genres.get(g)).copied().fold(0.0, f64::max))
    }

    /// How much a song is yours: liked or played, by artists you like, on albums you
    /// play, in your genres.
    pub fn song(&self, t: &Track) -> f64 {
        let mut s = self.artist(&t.artist.id) + 0.5 * t.featured.iter().map(|a| self.artist(&a.id)).fold(0.0, f64::max);
        if self.liked_songs.contains(&t.id) {
            s += 2.0;
        }
        if self.recent_songs.contains(&t.id) {
            s += 1.0;
        }
        if self.playlist_songs.contains(&t.id) {
            s += 1.5;
        }
        if let Some(a) = &t.album {
            s += 0.5 * self.album(&a.id) + 0.5 * self.genre(&a.id);
        }
        s
    }

    /// Whether the search page's top result should be the first song, not the artist whose
    /// name was typed: the whole song title was typed, and the song is more yours than
    /// that artist is (in your playlists, liked, played; an artist you follow stays), so "flying dutchman" finds your
    /// song, not an artist who happens to share its name.
    pub fn song_beats_artist(&self, q: &str, song: &Track, artist: &ArtistInfo) -> bool {
        let typed = crate::youtube::words(q);
        let typed: Vec<&str> = typed.split_whitespace().collect();
        crate::deezer::song_tier(&typed, q, song) == 2 && self.song(song) >= 1.0 && self.song(song) > self.artist(&artist.id) && !self.followed.contains(&artist.id)
    }

    fn playlist(&self, p: &PlaylistInfo) -> f64 {
        if self.liked_playlists.contains(&p.id) { 2.0 } else { 0.0 }
    }

    /// Search results reordered by your taste, without going against what was typed:
    /// songs, albums and playlists only move among neighbours that match it equally
    /// well, and artists only among those whose name starts with it.
    pub fn rerank(&self, q: &str, mut r: SearchResults) -> SearchResults {
        let typed = crate::youtube::words(q);
        let typed: Vec<&str> = typed.split_whitespace().collect();
        sort_runs(&mut r.tracks, |t| crate::deezer::song_tier(&typed, q, t), |t| self.song(t));
        sort_runs(&mut r.albums, |a| album_tier(q, a), |a| self.album(&a.id) + 0.5 * self.artist(&a.artist.id));
        sort_runs(&mut r.playlists, |p| name_match(&p.title, q), |p| self.playlist(p));
        self.rerank_artists(q, &mut r.artists);
        r
    }

    /// Among the artists whose name starts with what was typed (Deezer's order: name and
    /// fans), the ones you follow and play move up, as the search box's top artist does.
    fn rerank_artists(&self, q: &str, artists: &mut [ArtistInfo]) {
        let good: Vec<usize> = (0..artists.len()).filter(|&i| name_match(&artists[i].name, q) >= 2).collect();
        let mut picked: Vec<ArtistInfo> = good.iter().map(|&i| artists[i].clone()).collect();
        let score = |a: &ArtistInfo| name_match(&a.name, q) as f64 + ((a.fans + 1) as f64).log10() + ARTIST_WEIGHT * self.artist(&a.id);
        picked.sort_by(|a, b| score(b).total_cmp(&score(a)));
        for (slot, a) in good.into_iter().zip(picked) {
            artists[slot] = a;
        }
    }
}

/// How well an album matches: its title or its artist's name (see `name_match`).
fn album_tier(q: &str, a: &AlbumInfo) -> u8 {
    name_match(&a.title, q).max(name_match(&a.artist.name, q))
}

/// Taste in steps, so tiny differences don't shuffle results around.
fn step(n: f64) -> u8 {
    (n / 0.5).floor().clamp(0.0, 10.0) as u8
}

/// Reorders each run of neighbours with the same `tier` by `taste`, best first. Everything
/// else keeps its place, so the order search already chose (best match, the song you named
/// first, then its artist's other songs) stays as it is.
fn sort_runs<T>(list: &mut [T], tier: impl Fn(&T) -> u8, taste: impl Fn(&T) -> f64) {
    let tiers: Vec<u8> = list.iter().map(&tier).collect();
    let mut start = 0;
    while start < list.len() {
        let end = (start..list.len()).find(|&i| tiers[i] != tiers[start]).unwrap_or(list.len());
        // Stable: equal taste keeps search's order.
        list[start..end].sort_by_key(|x| std::cmp::Reverse(step(taste(x))));
        start = end;
    }
}

/// A cheap fingerprint of what the taste is built from, so it's rebuilt only when that
/// changes (liking, playing, following), not every frame.
pub fn fingerprint(s: &Saved) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let first = |v: &[Visit]| v.first().map(|x| (x.count, x.last));
    (s.playlists.len(), s.playlists.iter().map(|p| p.tracks.len()).sum::<usize>()).hash(&mut h);
    (s.liked.len(), s.liked.first().map(|t| &t.id), s.recent.first().map(|t| &t.id), s.recent.len()).hash(&mut h);
    (first(&s.played_artists), first(&s.played_albums), s.followed_artists.len(), s.followed_artists.first().map(|a| &a.id)).hash(&mut h);
    (s.liked_albums.len(), s.liked_playlists.len(), s.album_genres.len(), s.taste_related.len(), s.taste_related_at).hash(&mut h);
    // Plays fade by the day.
    (now_unix() / 86_400).hash(&mut h);
    h.finish()
}

/// Related artists of `top` (in order, top first): artist id → weight, 1 for the top
/// artist's, less for the next ones, best weight kept. Your own top artists aren't in it.
pub fn related(top: &[String]) -> Vec<(String, f64)> {
    let mut out: HashMap<String, f64> = HashMap::new();
    for (i, id) in top.iter().enumerate() {
        let w = 1.0 - i as f64 / (top.len() as f64 * 1.5);
        for (j, a) in crate::deezer::related_artists(id, 12).into_iter().enumerate() {
            // Deezer lists the closest first.
            let w = w * (1.0 - j as f64 / 24.0);
            let e = out.entry(a.id).or_default();
            *e = e.max(w);
        }
        // Stay well under Deezer's limit of 50 requests per 5 seconds.
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let mut v: Vec<(String, f64)> = out.into_iter().filter(|(id, _)| !top.contains(id)).collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(120);
    v
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::deezer::ArtistRef;

    fn song(id: &str, title: &str, artist_id: &str, artist: &str, rank: u64) -> Track {
        let mut t = Track { id: id.into(), title: title.into(), rank, ..Default::default() };
        t.artist = ArtistRef { id: artist_id.into(), name: artist.into(), picture: None };
        t
    }
    fn played(id: &str, count: u32, days_ago: u64, now: u64) -> Visit {
        Visit { place: Place::Artist(id.into()), title: String::new(), subtitle: String::new(), image: None, round: true, count, last: now - days_ago * 86_400 }
    }
    fn artist(id: &str, name: &str, fans: u64) -> ArtistInfo {
        ArtistInfo { id: id.into(), name: name.into(), fans, ..Default::default() }
    }

    #[test]
    fn your_artists_song_comes_first_among_equal_matches() {
        let now = 2_000_000_000;
        let mut s = Saved::default();
        s.played_artists = vec![played("2", 30, 1, now)];
        let taste = Taste::build(&s, now);
        // Two songs called "Baghdad": Deezer puts the more popular first.
        let r = SearchResults { tracks: vec![song("a", "Baghdad", "1", "Big Star", 900_000), song("b", "Baghdad", "2", "Your Artist", 10_000)], ..Default::default() };
        let r = taste.rerank("baghdad", r);
        assert_eq!(r.tracks[0].id, "b", "the artist you play wins the tie");
    }

    #[test]
    fn a_better_match_still_wins() {
        let now = 2_000_000_000;
        let mut s = Saved::default();
        s.followed_artists = vec![artist("2", "Your Artist", 10)];
        s.liked = vec![song("b", "Lights Out", "2", "Your Artist", 10)];
        let taste = Taste::build(&s, now);
        // Typed the other artist's exact song: it stays first even though you love "b".
        let r = SearchResults { tracks: vec![song("a", "Bad Habits", "1", "Ed Sheeran", 900_000), song("b", "Lights Out", "2", "Your Artist", 10)], ..Default::default() };
        let r = taste.rerank("bad habits", r);
        assert_eq!(r.tracks[0].id, "a");
    }

    #[test]
    fn songs_only_move_within_their_run() {
        let mut v = vec![(2u8, 0.0, 'a'), (2, 3.0, 'b'), (0, 0.0, 'c'), (2, 5.0, 'd')];
        sort_runs(&mut v, |x| x.0, |x| x.1);
        let order: String = v.iter().map(|x| x.2).collect();
        assert_eq!(order, "bacd", "'d' doesn't jump over 'c', which search put before it");
    }

    #[test]
    fn the_artist_you_follow_comes_first_among_good_name_matches() {
        let now = 2_000_000_000;
        let mut s = Saved::default();
        s.followed_artists = vec![artist("small", "Tyler", 900)];
        let taste = Taste::build(&s, now);
        let r = SearchResults { artists: vec![artist("big", "Tyler, The Creator", 9_000_000), artist("small", "Tyler", 900), artist("x", "Steven Tyler", 5_000_000)], ..Default::default() };
        let r = taste.rerank("tyler", r);
        assert_eq!(r.artists[0].id, "small");
        assert_eq!(r.artists[2].id, "x", "a weaker name match keeps its place");
        // Without the follow, Deezer's order stays.
        let r = Taste::default().rerank("tyler", SearchResults { artists: vec![artist("big", "Tyler, The Creator", 9_000_000), artist("small", "Tyler", 900)], ..Default::default() });
        assert_eq!(r.artists[0].id, "big");
    }

    #[test]
    fn your_playlist_song_beats_an_artist_with_the_same_name() {
        let now = 2_000_000_000;
        let mut s = Saved::default();
        let mut pl = crate::app::LocalPlaylist::default();
        pl.tracks = vec![song("s", "Flying Dutchman", "7", "Bleood", 100)];
        s.playlists = vec![pl];
        let taste = Taste::build(&s, now);
        let mine = song("s", "Flying Dutchman", "7", "Bleood", 100);
        let other = song("o", "Flying Dutchman", "8", "Someone", 900_000);
        let namesake = artist("1", "The Flying Dutchman", 5_000);
        assert!(taste.song_beats_artist("flying dutchman", &mine, &namesake));
        assert!(!taste.song_beats_artist("flying dutchman", &other, &namesake), "a song you don't know doesn't beat the artist");
        // Typing only part of the title doesn't count as asking for the song.
        assert!(!taste.song_beats_artist("flying", &mine, &namesake));
        // Following the artist that was named keeps the artist on top.
        let mut s2 = Saved::default();
        s2.playlists = vec![crate::app::LocalPlaylist { tracks: vec![mine.clone()], ..Default::default() }];
        s2.followed_artists = vec![namesake.clone()];
        assert!(!Taste::build(&s2, now).song_beats_artist("flying dutchman", &mine, &namesake));
    }

    #[test]
    fn related_artists_count_less_than_your_own() {
        let now = 2_000_000_000;
        let mut s = Saved::default();
        s.played_artists = vec![played("1", 5, 0, now)];
        s.taste_related = vec![("9".into(), 1.0)];
        let taste = Taste::build(&s, now);
        assert!(taste.artist("9") > 0.0);
        assert!(taste.artist("9") < taste.artist("1"));
    }

    #[test]
    fn old_plays_count_less() {
        let now = 2_000_000_000;
        let mut s = Saved::default();
        s.played_artists = vec![played("11", 10, 0, now), played("22", 10, 60, now)];
        let taste = Taste::build(&s, now);
        assert!(taste.artist("11") > taste.artist("22"));
        assert_eq!(taste.top_artists(1), ["11"]);
    }

    #[test]
    fn your_genres_lift_songs_from_albums_with_known_genres() {
        let now = 2_000_000_000;
        let mut s = Saved::default();
        let mut mine = song("m", "Mine", "1", "A", 1);
        mine.album = Some(crate::deezer::AlbumRef { id: "al1".into(), ..Default::default() });
        s.liked = vec![mine];
        s.album_genres.insert("al1".into(), vec!["Rap/Hip Hop".into()]);
        s.album_genres.insert("al2".into(), vec!["Rap/Hip Hop".into()]);
        let taste = Taste::build(&s, now);
        let mut other = song("o", "Other", "5", "B", 1);
        other.album = Some(crate::deezer::AlbumRef { id: "al2".into(), ..Default::default() });
        assert!(taste.song(&other) > 0.0);
    }
}
