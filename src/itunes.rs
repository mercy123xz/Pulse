//! Album covers from Apple's free iTunes Search API (no account or key), for songs
//! Deezer doesn't have.

use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Reply {
    results: Vec<Item>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Item {
    artist_name: String,
    track_name: String,
    artwork_url100: Option<String>,
}

/// A 600×600 cover for `title` by `artist`, if iTunes has the song.
pub fn cover(artist: &str, title: &str) -> Option<String> {
    let term = crate::net::encode(&format!("{artist} {}", crate::youtube::core_title(title)));
    let text = crate::net::get_page(&format!("https://itunes.apple.com/search?term={term}&entity=song&limit=8")).ok()?;
    pick(&text, artist, title)
}

fn pick(json: &str, artist: &str, title: &str) -> Option<String> {
    use crate::youtube::{core_title, norm};
    let reply: Reply = serde_json::from_str(json).ok()?;
    let want_title = norm(&core_title(title));
    let want_artist = norm(artist);
    reply
        .results
        .into_iter()
        .find(|r| {
            let t = norm(&core_title(&r.track_name));
            let a = norm(&r.artist_name);
            let title_ok = !want_title.is_empty() && (t == want_title || t.contains(&want_title) || want_title.contains(&t));
            let artist_ok = want_artist.is_empty() || (!a.is_empty() && (a.contains(&want_artist) || want_artist.contains(&a)));
            title_ok && artist_ok
        })
        .and_then(|r| r.artwork_url100)
        // Apple serves any size: ask for 600×600 instead of 100×100.
        .map(|u| u.replace("100x100bb", "600x600bb"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn picks_the_matching_song() {
        let json = r#"{"resultCount":2,"results":[
            {"wrapperType":"track","kind":"song","artistName":"Someone Else","trackName":"Ready or Not","artworkUrl100":"https://is1-ssl.mzstatic.com/a/100x100bb.jpg"},
            {"wrapperType":"track","kind":"song","artistName":"brakence","trackName":"ready or noT","collectionName":"hypochondriac","artworkUrl100":"https://is1-ssl.mzstatic.com/b/100x100bb.jpg"}]}"#;
        assert_eq!(super::pick(json, "brakence", "ready or noT").as_deref(), Some("https://is1-ssl.mzstatic.com/b/600x600bb.jpg"));
        assert_eq!(super::pick(json, "nobody", "nothing"), None);
        assert_eq!(super::pick("not json", "a", "b"), None);
    }
}
