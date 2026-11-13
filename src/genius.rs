//! Lyrics from Genius (genius.com), for songs LRCLIB doesn't have. Genius has no
//! official way to get lyrics, so this reads them off the song's page, the way the
//! lyricsgenius library does: search Genius for the song, open its page, and take
//! the text of the lyrics boxes. Plain text only (Genius lyrics aren't time-synced).
//!
//! Genius can change its pages or turn requests away at any time; then this simply
//! finds nothing and Pulse shows no lyrics, as before.

use serde_json::Value;

/// The song's lyrics and the Genius page they came from.
pub fn lyrics(artist: &str, title: &str) -> Option<(String, String)> {
    let core = crate::youtube::core_title(title);
    let q = crate::net::encode(&format!("{artist} {core}"));
    let reply = crate::net::get_page(&format!("https://genius.com/api/search/song?q={q}&per_page=10")).ok()?;
    let url = best_hit(&serde_json::from_str(&reply).ok()?, artist, title)?;
    let page = crate::net::get_page(&url).ok()?;
    let text = from_page(&page)?;
    Some((text, url))
}

/// The page of the search result that's this song by this artist.
fn best_hit(reply: &Value, artist: &str, title: &str) -> Option<String> {
    let (main, also) = crate::deezer::split_artists(artist);
    let wanted: Vec<String> = std::iter::once(main).chain(also).map(|a| crate::youtube::norm(&a)).filter(|a| !a.is_empty()).collect();
    let mut hits = Vec::new();
    collect_songs(reply, &mut hits);
    hits.into_iter()
        .find(|s| {
            let t = s["title"].as_str().unwrap_or("");
            let by = s["primary_artist"]["name"].as_str().or(s["artist_names"].as_str()).unwrap_or("");
            let all = s["artist_names"].as_str().unwrap_or(by);
            crate::deezer::same_title(title, t)
                && !crate::youtube::unwanted_version(title, t)
                && (wanted.is_empty() || crate::deezer::same_artist(&wanted, by) || crate::deezer::same_artist(&wanted, all))
                // Genius also has translations and fan pages ("Genius Brasil Traduções"…).
                && !by.to_lowercase().starts_with("genius ")
        })
        .and_then(|s| s["url"].as_str().map(str::to_string))
        .filter(|u| u.starts_with("https://genius.com/"))
}

/// Every song in a Genius search reply (they sit under response.sections[].hits[].result).
fn collect_songs<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            if let Some(r) = m.get("result")
                && r.get("url").is_some()
                && r.get("title").is_some()
            {
                out.push(r);
            }
            m.values().for_each(|x| collect_songs(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| collect_songs(x, out)),
        _ => {}
    }
}

/// The lyrics text from a Genius song page: every `data-lyrics-container="true"` box,
/// with line breaks kept and Genius's own extras inside them (headers marked
/// `data-exclude-from-selection`) left out.
fn from_page(html: &str) -> Option<String> {
    const MARK: &str = "data-lyrics-container=\"true\"";
    let mut parts = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(MARK) {
        // Back up to the "<div" that has the mark, then take that whole element.
        let start = rest[..i].rfind('<')?;
        let (inner, after) = element_inner(&rest[start..])?;
        parts.push(text_of(inner));
        rest = after;
    }
    let text = parts.join("\n").lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// For "<div …>inner</div>rest", (inner, rest): nested divs are counted.
fn element_inner(s: &str) -> Option<(&str, &str)> {
    let open_end = s.find('>')? + 1;
    let mut depth = 1;
    let mut i = open_end;
    while depth > 0 {
        let next = s[i..].find('<')? + i;
        if s[next..].starts_with("</div") {
            depth -= 1;
        } else if s[next..].starts_with("<div") {
            depth += 1;
        }
        i = next + 1;
        if depth == 0 {
            let close_end = s[next..].find('>')? + next + 1;
            return Some((&s[open_end..next], &s[close_end..]));
        }
    }
    None
}

/// Text from a bit of HTML: <br> become line breaks, extras Genius marks as not part
/// of the lyrics are dropped, other tags are removed and entities decoded.
fn text_of(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let tag_end = match rest[lt..].find('>') {
            Some(e) => lt + e + 1,
            None => break,
        };
        let tag = &rest[lt..tag_end];
        if tag.starts_with("<br") {
            out.push('\n');
            rest = &rest[tag_end..];
        } else if tag.contains("data-exclude-from-selection=\"true\"") && tag.starts_with("<div") {
            // Skip the whole element.
            rest = element_inner(&rest[lt..]).map(|(_, after)| after).unwrap_or(&rest[tag_end..]);
        } else {
            rest = &rest[tag_end..];
        }
    }
    out.push_str(rest);
    decode(&out)
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail[..tail.len().min(12)].find(';') else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..semi];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_lyrics_boxes_off_a_page() {
        let page = r#"<html><div class="x"><div data-lyrics-container="true" class="Lyrics__Container"><div data-exclude-from-selection="true"><span>12 Contributors</span><div>Song Lyrics</div></div>[Verse 1]<br/>I&#x27;m up at <a href="/x"><span>night</span></a><br/>Can&#39;t sleep &amp; so on</div></div>
<p>ad</p><div data-lyrics-container="true">[Chorus]<br>Geeked up<br/></div></html>"#;
        assert_eq!(from_page(page).unwrap(), "[Verse 1]\nI'm up at night\nCan't sleep & so on\n[Chorus]\nGeeked up");
        assert!(from_page("<html>no lyrics here</html>").is_none());
    }

    #[test]
    fn picks_the_right_search_hit() {
        let reply = serde_json::json!({ "response": { "sections": [{ "type": "song", "hits": [
            { "result": { "title": "Geeked up (Traducción al Español)", "primary_artist": { "name": "Genius Traducciones al Español" }, "url": "https://genius.com/tr" } },
            { "result": { "title": "Geeked Up", "primary_artist": { "name": "Kay Flock" }, "artist_names": "Kay Flock & Gucci Mane", "url": "https://genius.com/kay" } },
            { "result": { "title": "Geeked up", "primary_artist": { "name": "ApolloRed1" }, "artist_names": "ApolloRed1 & OsamaSon", "url": "https://genius.com/Apollored1-and-osamason-geeked-up-lyrics" } },
        ] }] } });
        assert_eq!(best_hit(&reply, "ApolloRed1", "Geeked up").as_deref(), Some("https://genius.com/Apollored1-and-osamason-geeked-up-lyrics"));
        assert_eq!(best_hit(&reply, "OsamaSon", "Geeked up").as_deref(), Some("https://genius.com/Apollored1-and-osamason-geeked-up-lyrics"));
        assert!(best_hit(&reply, "Drake", "Geeked up").is_none());
    }

    /// Live check (needs the network).
    #[test]
    #[ignore]
    fn live_genius_lyrics() {
        for (artist, title) in [("ApolloRed1", "Geeked up"), ("The Killers", "Mr. Brightside")] {
            let found = lyrics(artist, title);
            println!("{artist} – {title}: {:?}", found.as_ref().map(|(t, u)| (u, t.lines().take(4).collect::<Vec<_>>())));
            assert!(found.is_some_and(|(t, _)| t.lines().count() > 5));
        }
    }
}
