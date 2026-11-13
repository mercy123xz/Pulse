//! Song details from TIDAL, for songs Deezer doesn't have: the real length, whether the
//! song has swearing, and every credited artist. Pulse asks its own accounts server,
//! which holds the TIDAL key; the app never has it.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub version: String,
    pub isrc: String,
    /// Seconds.
    pub duration: u32,
    pub explicit: bool,
    pub popularity: f64,
    pub artists: Vec<String>,
    pub album: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Reply {
    tracks: Vec<Song>,
}

/// TIDAL's version of `title` by `artist`, if it has the song.
pub fn find(artist: &str, title: &str, duration: u32) -> Option<Song> {
    let core = crate::youtube::core_title(title);
    let q = crate::net::encode(&format!("{artist} {core}"));
    let mut resp = crate::net::api().get(&format!("{}/tidal/search?q={q}", crate::account::base())).call().ok()?;
    let reply: Reply = resp.body_mut().read_json().ok()?;
    pick(reply.tracks, artist, title, duration)
}

/// The right song among TIDAL's results: the same title by the same artist, the length
/// closest to the source's, the explicit or clean version as Settings asks, then the
/// most popular. A different version ("Remix", "Live") is never the song.
fn pick(songs: Vec<Song>, artist: &str, title: &str, duration: u32) -> Option<Song> {
    let (main, also) = crate::deezer::split_artists(artist);
    let wanted: Vec<String> = std::iter::once(main).chain(also).map(|a| crate::youtube::norm(&a)).filter(|a| !a.is_empty()).collect();
    let want_explicit = crate::deezer::prefer_explicit();
    songs
        .into_iter()
        .filter(|s| crate::deezer::same_title(title, &s.title) && !crate::youtube::unwanted_version(title, &format!("{} ({})", s.title, s.version)))
        .filter(|s| wanted.is_empty() || s.artists.iter().any(|a| crate::deezer::same_artist(&wanted, a)))
        .min_by(|a, b| {
            let off = |s: &Song| if duration >= 60 { (s.duration as i64 - duration as i64).unsigned_abs() / 4 } else { 0 };
            (off(a), a.explicit != want_explicit).cmp(&(off(b), b.explicit != want_explicit)).then(b.popularity.total_cmp(&a.popularity))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, title: &str, artists: &[&str], duration: u32, explicit: bool, popularity: f64) -> Song {
        Song {
            id: id.into(),
            title: title.into(),
            duration,
            explicit,
            popularity,
            artists: artists.iter().map(|a| a.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn picks_the_song_and_the_version_settings_ask_for() {
        let found = || {
            vec![
                song("other", "Geeked Up", &["FABO", "Jeezy"], 216, true, 0.8),
                song("clean", "Geeked up", &["ApolloRed1", "OsamaSon"], 155, false, 0.05),
                song("dirty", "Geeked up", &["ApolloRed1", "OsamaSon"], 155, true, 0.4),
            ]
        };
        let _turn = crate::deezer::EXPLICIT_TEST.lock().unwrap_or_else(|e| e.into_inner());
        crate::deezer::set_prefer_explicit(true);
        assert_eq!(pick(found(), "ApolloRed1", "Geeked up", 156).unwrap().id, "dirty");
        crate::deezer::set_prefer_explicit(false);
        assert_eq!(pick(found(), "ApolloRed1", "Geeked up", 0).unwrap().id, "clean");
        crate::deezer::set_prefer_explicit(true);
        assert!(pick(found(), "Somebody Else", "Geeked up", 156).is_none());
        let remix = vec![Song { version: "Remix".into(), ..song("r", "Geeked up", &["ApolloRed1"], 155, true, 0.9) }];
        assert!(pick(remix, "ApolloRed1", "Geeked up", 156).is_none());
    }
}
