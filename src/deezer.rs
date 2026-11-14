//! Song info, artwork and charts from the free Deezer API (https://developers.deezer.com/api).
//! The audio itself comes from YouTube (see `youtube.rs`).
//!
//! Every function here blocks while it waits for the network, so the app always
//! calls them from a background thread.

use serde::{Deserialize, Serialize};

const BASE: &str = "https://api.deezer.com";

// ---------------------------------------------------------------------------
// App-facing types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ArtistRef {
    /// Deezer artist id (digits). Songs saved by Pulse 0.1 have an old Audius id here.
    pub id: String,
    pub name: String,
    pub picture: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AlbumRef {
    pub id: String,
    pub title: String,
}

/// Artwork saved by Pulse 0.1 (Audius); read so old likes keep their covers.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LegacyArtwork {
    #[serde(rename = "150x150")]
    pub small: Option<String>,
    #[serde(rename = "480x480")]
    pub medium: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Track {
    /// "dz:<Deezer id>" (songs saved by Pulse 0.1 keep their old Audius id).
    pub id: String,
    pub title: String,
    /// Length in seconds.
    pub duration: u32,
    #[serde(alias = "user")]
    pub artist: ArtistRef,
    pub album: Option<AlbumRef>,
    pub cover_small: Option<String>,
    pub cover_big: Option<String>,
    pub explicit: bool,
    /// Deezer's 30-second preview MP3. Pulse doesn't play it; kept so saved libraries load.
    pub preview: Option<String>,
    #[serde(skip_serializing)]
    pub artwork: Option<LegacyArtwork>,
    /// Featured / other credited artists (besides `artist`).
    pub featured: Vec<ArtistRef>,
    /// Deezer's popularity score (0 if unknown).
    #[serde(skip_serializing_if = "is_zero")]
    pub rank: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Track {
    /// "Quality Control, Lil Baby, DaBaby": the main artist and any featured artists.
    pub fn all_artists(&self) -> String {
        std::iter::once(self.artist.name.as_str()).chain(self.featured.iter().map(|a| a.name.as_str())).collect::<Vec<_>>().join(", ")
    }

    /// Deezer's number for this song, if it came from Deezer.
    pub fn deezer_number(&self) -> Option<u64> {
        self.deezer_id()
    }

    pub fn deezer_id(&self) -> Option<u64> {
        self.id.strip_prefix("dz:").and_then(|s| s.parse().ok())
    }
    pub fn art_small(&self) -> Option<&str> {
        self.cover_small.as_deref().or_else(|| self.artwork.as_ref().and_then(|a| a.small.as_deref().or(a.medium.as_deref())))
    }
    pub fn art_big(&self) -> Option<&str> {
        self.cover_big.as_deref().or_else(|| self.art_small())
    }
    /// True if the artist id is a Deezer one (so the artist page can open).
    pub fn has_artist_page(&self) -> bool {
        is_deezer_id(&self.artist.id)
    }
}

pub fn is_deezer_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtistInfo {
    pub id: String,
    pub name: String,
    pub picture: Option<String>,
    pub picture_xl: Option<String>,
    pub fans: u64,
    pub albums: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlbumInfo {
    pub id: String,
    pub title: String,
    pub cover: Option<String>,
    pub cover_small: Option<String>,
    pub artist: ArtistRef,
    pub release_date: Option<String>,
    /// "album", "single", "ep", "compile"
    pub record_type: String,
    pub nb_tracks: u64,
    pub label: Option<String>,
}

impl AlbumInfo {
    pub fn kind(&self) -> &'static str {
        match self.record_type.as_str() {
            "single" => "Single",
            "ep" => "EP",
            "compile" => "Compilation",
            _ => "Album",
        }
    }
    pub fn year(&self) -> Option<&str> {
        self.release_date.as_deref().and_then(|d| d.get(..4)).filter(|y| *y != "0000")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaylistInfo {
    pub id: String,
    pub title: String,
    pub picture: Option<String>,
    pub creator: String,
    pub nb_tracks: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Chart {
    pub tracks: Vec<Track>,
    pub albums: Vec<AlbumInfo>,
    pub artists: Vec<ArtistInfo>,
    pub playlists: Vec<PlaylistInfo>,
    /// Built from what you play: "Listeners of X also listen to" and "Songs like Y".
    pub for_you: Vec<Suggestion>,
}

/// A row of suggestions built around one artist or song.
#[derive(Debug, Clone)]
pub enum Suggestion {
    /// Artists whose listeners also play `.0` (an artist's name).
    AlsoListen(String, Vec<ArtistInfo>),
    /// Songs by other artists that sound like `.0` (a song or artist name).
    SongsLike(String, Vec<Track>),
}

#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub artists: Vec<ArtistInfo>,
    pub albums: Vec<AlbumInfo>,
    pub playlists: Vec<PlaylistInfo>,
    /// When the top result is an artist: who their listeners also play, and songs like theirs.
    pub around: Option<(String, Vec<ArtistInfo>, Vec<Track>)>,
    /// Pulse users whose names start with what was typed.
    pub users: Vec<crate::account::User>,
}

#[derive(Debug, Clone, Default)]
pub struct ArtistPage {
    pub artist: ArtistInfo,
    pub top: Vec<Track>,
    /// Albums, EPs and singles, newest first.
    pub releases: Vec<AlbumInfo>,
    /// Releases by other artists with the same name, which Deezer files under this one.
    pub namesakes: Vec<AlbumInfo>,
    pub related: Vec<ArtistInfo>,
    /// Songs by other artists that sound like this one.
    pub songs_like: Vec<Track>,
}

/// Deezer's genres (id, name), as used by the charts.
pub const GENRES: &[(u32, &str)] = &[
    (132, "Pop"),
    (116, "Rap/Hip Hop"),
    (165, "R&B"),
    (152, "Rock"),
    (85, "Alternative"),
    (113, "Dance"),
    (106, "Electro"),
    (122, "Reggaeton"),
    (197, "Latin Music"),
    (464, "Metal"),
    (169, "Soul & Funk"),
    (129, "Jazz"),
    (84, "Country"),
    (466, "Folk"),
    (144, "Reggae"),
    (153, "Blues"),
    (98, "Classical"),
    (173, "Films/Games"),
    (2, "African Music"),
    (16, "Asian Music"),
    (75, "Brazilian Music"),
    (81, "Indian Music"),
    (65, "Traditional Mexicano"),
    (71, "Cumbia"),
    (67, "Salsa"),
    (186, "Christian"),
    (95, "Kids"),
];

// ---------------------------------------------------------------------------
// Raw API shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct DzArtist {
    id: u64,
    name: String,
    picture_medium: Option<String>,
    picture_big: Option<String>,
    picture_xl: Option<String>,
    nb_fan: u64,
    nb_album: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct DzAlbum {
    id: u64,
    title: String,
    cover_small: Option<String>,
    cover_medium: Option<String>,
    cover_big: Option<String>,
    cover_xl: Option<String>,
    release_date: Option<String>,
    record_type: Option<String>,
    nb_tracks: u64,
    label: Option<String>,
    artist: Option<DzArtist>,
    /// Only in an artist's release list: how many fans the release has, and its genre.
    fans: u64,
    genre_id: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct DzTrack {
    id: u64,
    title: String,
    duration: u32,
    explicit_lyrics: bool,
    preview: Option<String>,
    artist: DzArtist,
    album: Option<DzAlbum>,
    /// Deezer's popularity score.
    rank: u64,
    /// Every credited artist (only in some replies: song details, artist top songs).
    contributors: Vec<DzArtist>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct DzUser {
    name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct DzPlaylist {
    id: u64,
    title: String,
    nb_tracks: u64,
    picture_medium: Option<String>,
    picture_big: Option<String>,
    user: Option<DzUser>,
    creator: Option<DzUser>,
    tracks: Option<List<DzTrack>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct DzAlbumFull {
    #[serde(flatten)]
    album: DzAlbum,
    tracks: Option<List<DzTrack>>,
}

#[derive(Debug, Clone, Deserialize)]
struct List<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

impl<T> Default for List<T> {
    fn default() -> Self {
        Self { data: Vec::new() }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct DzChart {
    tracks: List<DzTrack>,
    albums: List<DzAlbum>,
    artists: List<DzArtist>,
    playlists: List<DzPlaylist>,
}

#[derive(Deserialize)]
struct DzError {
    error: DzErrorBody,
}

#[derive(Deserialize)]
struct DzErrorBody {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

impl From<DzArtist> for ArtistInfo {
    fn from(a: DzArtist) -> Self {
        Self {
            id: a.id.to_string(),
            name: a.name,
            picture: a.picture_big.clone().or(a.picture_medium),
            picture_xl: a.picture_xl.or(a.picture_big),
            fans: a.nb_fan,
            albums: a.nb_album,
        }
    }
}

fn artist_ref(a: &DzArtist) -> ArtistRef {
    // The 500 px picture: sharp in cards and lists on high-resolution screens.
    ArtistRef { id: a.id.to_string(), name: a.name.clone(), picture: a.picture_big.clone().or(a.picture_medium.clone()) }
}

/// The other credited artists of a song (featured artists), without the main one.
fn featured_from(main: &ArtistRef, contributors: &[DzArtist]) -> Vec<ArtistRef> {
    let mut seen = std::collections::HashSet::new();
    seen.insert(main.id.clone());
    contributors.iter().filter(|c| c.id != 0 && !c.name.is_empty()).map(artist_ref).filter(|a| seen.insert(a.id.clone())).collect()
}

/// Featured artists of one song, from its full details (lists don't include them).
pub fn featured_artists(track_id: u64) -> Result<Vec<ArtistRef>, String> {
    let t: DzTrack = get(&format!("/track/{track_id}"), &[])?;
    Ok(featured_from(&artist_ref(&t.artist), &t.contributors))
}

fn album_info(a: DzAlbum, fallback_artist: Option<&ArtistRef>) -> AlbumInfo {
    AlbumInfo {
        id: a.id.to_string(),
        title: a.title,
        cover: a.cover_big.or(a.cover_xl).or(a.cover_medium.clone()),
        cover_small: a.cover_small.or(a.cover_medium),
        artist: a.artist.as_ref().map(artist_ref).or_else(|| fallback_artist.cloned()).unwrap_or_default(),
        release_date: a.release_date,
        record_type: a.record_type.unwrap_or_else(|| "album".into()),
        nb_tracks: a.nb_tracks,
        label: a.label,
    }
}

/// Converts a Deezer track. `album` fills in album details that the API leaves out
/// of album track lists.
fn track(t: DzTrack, album: Option<&AlbumInfo>) -> Track {
    let (album_ref, small, big) = match (&t.album, album) {
        (Some(a), _) if a.id != 0 => (
            Some(AlbumRef { id: a.id.to_string(), title: a.title.clone() }),
            a.cover_small.clone().or(a.cover_medium.clone()).or(album.and_then(|x| x.cover_small.clone())),
            a.cover_big.clone().or(a.cover_xl.clone()).or(album.and_then(|x| x.cover.clone())),
        ),
        (_, Some(a)) => (Some(AlbumRef { id: a.id.clone(), title: a.title.clone() }), a.cover_small.clone(), a.cover.clone()),
        _ => (None, None, None),
    };
    let main = artist_ref(&t.artist);
    let featured = featured_from(&main, &t.contributors);
    Track {
        id: format!("dz:{}", t.id),
        title: t.title,
        duration: t.duration,
        rank: t.rank,
        featured,
        artist: main,
        album: album_ref,
        cover_small: small,
        cover_big: big,
        explicit: t.explicit_lyrics,
        preview: t.preview.filter(|p| !p.is_empty()),
        artwork: None,
    }
}

fn playlist_info(p: &DzPlaylist) -> PlaylistInfo {
    PlaylistInfo {
        id: p.id.to_string(),
        title: p.title.clone(),
        picture: p.picture_big.clone().or(p.picture_medium.clone()),
        creator: p.creator.as_ref().or(p.user.as_ref()).map(|u| u.name.clone()).unwrap_or_default(),
        nb_tracks: p.nb_tracks,
    }
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// Parses a Deezer response. Deezer reports errors as HTTP 200 with an `error` object.
fn parse<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, String> {
    if let Ok(e) = serde_json::from_str::<DzError>(text) {
        return Err(match e.error.code {
            4 => "Deezer is busy right now. Wait a few seconds and try again.".into(),
            800 => "Not found on Deezer.".into(),
            _ => format!("Deezer error: {}", e.error.message),
        });
    }
    serde_json::from_str(text).map_err(|e| format!("Deezer sent something unexpected ({e})"))
}

fn get<T: for<'de> Deserialize<'de>>(path: &str, query: &[(&str, &str)]) -> Result<T, String> {
    // Developer testing only: read saved replies instead of the network.
    #[cfg(debug_assertions)]
    if let Ok(dir) = std::env::var("PULSE_FIXTURES") {
        let file = format!("{dir}/{}.json", path.trim_start_matches('/').replace('/', "_"));
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
        return parse(&text);
    }
    // The same reply asked for twice in a short while (a song page and its credits both
    // read the song and its album, for example) comes from memory.
    let key = std::iter::once(path.to_string()).chain(query.iter().map(|(k, v)| format!("{k}={v}"))).collect::<Vec<_>>().join("&");
    if let Some(text) = reply_cache::get(&key) {
        return parse(&text);
    }
    // Deezer allows 50 requests per 5 seconds. When it says it's busy, wait and try again
    // (otherwise a busy moment looks like "song not found").
    let mut wait = 700;
    for attempt in 0..4 {
        let mut req = crate::net::api().get(format!("{BASE}{path}"));
        for (k, v) in query {
            req = req.query(*k, *v);
        }
        let mut resp = req.call().map_err(crate::net::friendly_error)?;
        let text = resp.body_mut().with_config().limit(20 * 1024 * 1024).read_to_string().map_err(|e| format!("Couldn't read Deezer's reply ({e})"))?;
        if attempt < 3 && is_busy_reply(&text) {
            std::thread::sleep(std::time::Duration::from_millis(wait));
            wait *= 2;
            continue;
        }
        let parsed = parse(&text);
        if parsed.is_ok() {
            reply_cache::put(key, text);
        }
        return parsed;
    }
    unreachable!()
}

/// Recent Deezer replies, kept for a few minutes.
mod reply_cache {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    const LIFETIME: Duration = Duration::from_secs(10 * 60);
    const MAX: usize = 160;

    type Map = HashMap<String, (Instant, Arc<str>)>;
    fn map() -> &'static Mutex<Map> {
        static M: OnceLock<Mutex<Map>> = OnceLock::new();
        M.get_or_init(Default::default)
    }

    pub fn get(key: &str) -> Option<Arc<str>> {
        let m = map().lock().unwrap();
        m.get(key).filter(|(at, _)| at.elapsed() < LIFETIME).map(|(_, t)| t.clone())
    }

    pub fn put(key: String, text: String) {
        // Big replies (whole playlists) are kept by the page cache instead.
        if text.len() > 512 * 1024 {
            return;
        }
        let mut m = map().lock().unwrap();
        m.retain(|_, (at, _)| at.elapsed() < LIFETIME);
        if m.len() >= MAX
            && let Some(oldest) = m.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone())
        {
            m.remove(&oldest);
        }
        m.insert(key, (Instant::now(), text.into()));
    }
}

/// Deezer's "too many requests" reply (error code 4).
fn is_busy_reply(text: &str) -> bool {
    serde_json::from_str::<DzError>(text).is_ok_and(|e| e.error.code == 4)
}

fn tracks(list: List<DzTrack>) -> Vec<Track> {
    let mut t: Vec<Track> = list.data.into_iter().map(|t| track(t, None)).collect();
    crate::fixes::apply(&mut t);
    t
}

/// Home page: top songs, albums, artists and playlists.
/// The home page: today's charts, plus rows built from what you play. `artists` are
/// (Deezer id, name) of your top artists; `songs` are songs you played lately.
pub fn home(artists: &[(String, String)], songs: &[Track]) -> Result<Chart, String> {
    let (c, for_you) = std::thread::scope(|s| {
        let rows: Vec<_> = artists
            .iter()
            .take(2)
            .map(|(id, name)| s.spawn(move || Suggestion::AlsoListen(name.clone(), related_artists(id, 12))))
            .chain(songs.iter().take(2).map(|t| s.spawn(move || Suggestion::SongsLike(t.title.clone(), songs_like(t)))))
            .collect();
        let c = get::<DzChart>("/chart/0", &[]);
        let rows: Vec<Suggestion> = rows.into_iter().filter_map(|h| h.join().ok()).collect();
        (c, rows)
    });
    let c = c?;
    // Artists first, then songs, alternating so the page doesn't read as two lists.
    let (mut a, mut b): (Vec<_>, Vec<_>) = for_you
        .into_iter()
        .filter(|r| match r {
            Suggestion::AlsoListen(_, v) => v.len() >= 3,
            Suggestion::SongsLike(_, v) => v.len() >= 3,
        })
        .partition(|r| matches!(r, Suggestion::AlsoListen(..)));
    let mut for_you = vec![];
    while !a.is_empty() || !b.is_empty() {
        if !a.is_empty() {
            for_you.push(a.remove(0));
        }
        if !b.is_empty() {
            for_you.push(b.remove(0));
        }
    }
    Ok(Chart {
        tracks: tracks(c.tracks),
        albums: c.albums.data.into_iter().map(|a| album_info(a, None)).collect(),
        artists: c.artists.data.into_iter().map(ArtistInfo::from).collect(),
        playlists: c.playlists.data.iter().map(playlist_info).collect(),
        for_you,
    })
}

/// Artists whose listeners also play this one.
pub fn related_artists(artist_id: &str, limit: usize) -> Vec<ArtistInfo> {
    let found = get::<List<DzArtist>>(&format!("/artist/{artist_id}/related"), &[("limit", &limit.to_string())])
        .map(|l| l.data.into_iter().map(ArtistInfo::from).collect())
        .unwrap_or_default();
    one_per_name(found)
}

/// The most popular artists right now: overall (genre 0) or in one of `GENRES`.
/// For "Pick artists you like".
pub fn popular_artists(genre: u32, limit: usize) -> Result<Vec<ArtistInfo>, String> {
    let path = if genre == 0 { "/chart/0/artists".to_string() } else { format!("/genre/{genre}/artists") };
    get::<List<DzArtist>>(&path, &[("limit", &limit.to_string())]).map(|l| one_per_name(l.data.into_iter().map(ArtistInfo::from).collect()))
}

/// Artists whose name matches what was typed, best known first.
pub fn find_artists(q: &str) -> Result<Vec<ArtistInfo>, String> {
    get::<List<DzArtist>>("/search/artist", &[("q", q), ("limit", "24")]).map(|l| one_per_name(l.data.into_iter().map(ArtistInfo::from).collect()))
}

/// Deezer sometimes has two profiles with the same name ("che" and "Che"): keep the
/// better-known one, in the first one's place.
pub fn one_per_name(artists: Vec<ArtistInfo>) -> Vec<ArtistInfo> {
    let mut out: Vec<ArtistInfo> = vec![];
    for a in artists {
        let key = crate::youtube::norm(&a.name);
        match out.iter_mut().find(|b| crate::youtube::norm(&b.name) == key) {
            Some(b) if a.fans > b.fans => *b = a,
            Some(_) => {}
            None => out.push(a),
        }
    }
    out
}

/// Up to 20 songs by other artists that sound like `t` (from its artist's radio).
pub fn songs_like(t: &Track) -> Vec<Track> {
    let mut out = similar_to(t).unwrap_or_default();
    other_artists_only(&mut out, &t.artist.id, &t.artist.name);
    out.truncate(20);
    out
}

/// Up to 20 songs by other artists that sound like this artist's.
pub fn songs_like_artist(artist_id: &str, name: &str) -> Vec<Track> {
    let mut out = get::<List<DzTrack>>(&format!("/artist/{artist_id}/radio"), &[("limit", "40")]).map(tracks).unwrap_or_default();
    if out.is_empty() {
        out = similar_artists_songs(artist_id);
    }
    other_artists_only(&mut out, artist_id, name);
    out.truncate(20);
    out
}

/// Drops the artist's own songs and repeats, and keeps at most two songs per artist,
/// so "songs like" shows a spread of other artists.
pub fn other_artists_only(list: &mut Vec<Track>, artist_id: &str, name: &str) {
    let me = crate::youtube::norm(name);
    let mut per_artist: std::collections::HashMap<String, usize> = Default::default();
    let mut seen = std::collections::HashSet::new();
    list.retain(|t| {
        if t.artist.id == artist_id || crate::youtube::norm(&t.artist.name) == me || !seen.insert(t.id.clone()) {
            return false;
        }
        let n = per_artist.entry(t.artist.id.clone()).or_default();
        *n += 1;
        *n <= 2
    });
}

/// Top 100 songs, overall (genre 0) or in a genre.
pub fn chart_tracks(genre: u32) -> Result<Vec<Track>, String> {
    get::<List<DzTrack>>(&format!("/chart/{genre}/tracks"), &[("limit", "100")]).map(tracks)
}

static PREFER_EXPLICIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Whether the uncensored version of a song is wanted (Settings → Explicit songs).
pub fn prefer_explicit() -> bool {
    PREFER_EXPLICIT.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_prefer_explicit(on: bool) {
    PREFER_EXPLICIT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Tests that change the explicit setting take turns.
#[cfg(test)]
pub(crate) static EXPLICIT_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// "Nelly, City Spud" / "Nelly feat. City Spud" → ("Nelly", ["City Spud"]). Names
/// joined by "&" stay together: that is often one act ("Simon & Garfunkel").
pub fn split_artists(artists: &str) -> (String, Vec<String>) {
    let lower = artists.to_lowercase();
    let mut cuts = vec![];
    for m in [",", " feat. ", " feat ", " ft. ", " featuring ", " with "] {
        let mut from = 0;
        while let Some(i) = lower[from..].find(m) {
            cuts.push((from + i, m.len()));
            from += i + m.len();
        }
    }
    cuts.sort();
    let mut parts = vec![];
    let mut at = 0;
    for (i, len) in cuts {
        if i >= at {
            parts.push(artists[at..i].trim().to_string());
            at = i + len;
        }
    }
    parts.push(artists[at..].trim().to_string());
    parts.retain(|p| !p.is_empty());
    let main = if parts.is_empty() { String::new() } else { parts.remove(0) };
    (main, parts)
}

/// The artists a title credits: "Ride Wit Me (feat. City Spud)" → ["City Spud"].
pub fn featured_in_title(title: &str) -> Vec<String> {
    let lower = title.to_lowercase();
    let Some(i) = ["(feat", "[feat", "(ft.", "(with ", " feat.", " ft. "].iter().filter_map(|m| lower.find(m)).min() else {
        return vec![];
    };
    let rest = title[i..].trim_start_matches(['(', '[', ' ']);
    let rest = rest.split_once([' ', '.']).map_or("", |(_, r)| r);
    let rest = rest.split([')', ']']).next().unwrap_or("");
    rest.split([',', '&']).map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()
}

/// The title without "(feat. …)", "(Remastered)", " - Radio Edit" and the like.
fn base_title(title: &str) -> String {
    let t = crate::youtube::core_title(title);
    let cut = [" (", " [", " - "].iter().filter_map(|m| t.find(m)).min().unwrap_or(t.len());
    crate::youtube::norm(&t[..cut])
}

/// Same song title: "Firework" is "Firework (2010 Remaster)" but not "Fireworks".
pub fn same_title(a: &str, b: &str) -> bool {
    use crate::youtube::{core_title, norm};
    let (x, y) = (norm(&core_title(a)), norm(&core_title(b)));
    !x.is_empty() && (x == y || base_title(a) == base_title(b))
}

/// The song's artist: "The Black Eyed Peas" is "Black Eyed Peas", "Macklemore & Ryan
/// Lewis" is "Macklemore", but "Made famous by Katy Perry" isn't "Katy Perry".
pub(crate) fn same_artist(wanted: &[String], got: &str) -> bool {
    use crate::youtube::norm;
    let bare = |s: String| s.strip_prefix("the").map(str::to_string).unwrap_or(s);
    let whole = bare(norm(got));
    let parts: Vec<String> = got.split(['&', ',', '/']).chain(got.split(" and ")).map(|p| bare(norm(p))).collect();
    wanted.iter().map(|w| bare(w.clone())).any(|w| !w.is_empty() && (w == whole || parts.contains(&w)))
}

/// Words a candidate's title may add to the source's and still be the same recording
/// ("Firework (2010 Remaster)", "Get Lucky (Radio Edit)"); anything else ("Dallas K
/// Radio Edit", "Bamboo", "Live at …") makes it another version.
fn same_recording(wanted_title: &str, wanted_artists: &str, got: &str) -> bool {
    use crate::youtube::words;
    const FINE: [&str; 21] = [
        "radio",
        "edit",
        "album",
        "version",
        "remaster",
        "remastered",
        "single",
        "explicit",
        "clean",
        "original",
        "mono",
        "stereo",
        "digital",
        "deluxe",
        "feat",
        "ft",
        "featuring",
        "with",
        "and",
        "the",
        "edition",
    ];
    let have = words(&format!("{wanted_title} {wanted_artists}"));
    let got = words(got);
    got.split_whitespace().all(|w| have.contains(&format!(" {w} ")) || FINE.contains(&w) || w.chars().all(|c| c.is_ascii_digit()))
}

/// Karaoke labels and cover acts ("Made famous by …", "Rihanna Cover Band").
pub fn cover_act(artist: &str) -> bool {
    let a = crate::youtube::norm(artist);
    ["madefamous", "coverband", "karaoke", "tribute", "inthestyleof", "originallyperformed", "coverversion", "singalong", "pianoplayers", "lullaby"]
        .iter()
        .any(|m| a.contains(m))
}

/// Which of the candidates is the song, best first. Wrong title, artist or a version the
/// source didn't ask for (cover, instrumental, karaoke, sped up…) are left out, as are
/// lengths far from the source's (a 30-second clip, an extended mix). Then the closest
/// length wins, and among (nearly) equal lengths the most played one: the album version
/// over a karaoke label's copy or a re-release nobody plays.
fn best_match(candidates: Vec<Track>, artist: &str, title: &str, duration: u32) -> Option<Track> {
    use crate::youtube::{core_title, norm, unwanted_version};
    let (main, mut also) = split_artists(artist);
    let others = also.clone();
    also.extend(featured_in_title(title));
    let want_artist = norm(&main);
    let also: Vec<String> = also.iter().map(|a| norm(a)).filter(|a| !a.is_empty()).collect();
    candidates
        .into_iter()
        .filter(|t| {
            let title_ok = same_title(title, &t.title);
            let names: Vec<String> = std::iter::once(&main).chain(others.iter()).map(|a| norm(a)).collect();
            let artist_ok = want_artist.is_empty() || same_artist(&names, &t.artist.name);
            let length_ok = duration == 0 || t.duration == 0 || (t.duration.abs_diff(duration) <= 8.max(duration / 25) && t.duration * 10 >= duration * 6);
            let recording_ok = same_recording(title, artist, &t.title);
            title_ok && artist_ok && length_ok && recording_ok && !unwanted_version(title, &t.title) && !cover_act(&t.artist.name)
        })
        .min_by_key(|t| {
            // Credits the same featured artists (in its title or credits).
            let credited = norm(&t.title) + &t.featured.iter().map(|f| norm(&f.name)).collect::<String>();
            let missing = also.iter().filter(|a| !credited.contains(a.as_str())).count();
            let off = if duration == 0 || t.duration == 0 { 0 } else { t.duration.abs_diff(duration) / 3 };
            let inexact = norm(&core_title(&t.title)) != norm(&core_title(title));
            // The explicit or the clean version, as Settings asks (same length, same title).
            let wrong_version = t.explicit != prefer_explicit();
            (inexact, off, wrong_version, missing, std::cmp::Reverse(t.rank))
        })
}

/// Finds the Deezer song that is `title` by `artist` (for imported playlists).
/// `artist` may list several ("Nelly, City Spud"); `duration` (seconds, 0 if unknown)
/// helps pick the right version. Featured artists are kept.
pub fn find_track(artist: &str, title: &str, duration: u32) -> Result<Option<Track>, String> {
    use crate::youtube::core_title;
    let (main, _) = split_artists(artist);
    // Deezer's precise search first, then a plain one.
    let precise = format!("artist:\"{}\" track:\"{}\"", main.replace('"', ""), core_title(title).replace('"', ""));
    let first: List<DzTrack> = get("/search/track", &[("q", &precise), ("limit", "25")])?;
    let mut found = best_match(tracks(first), artist, title, duration);
    if found.is_none() {
        let plain = format!("{main} {}", core_title(title));
        let second: List<DzTrack> = get("/search/track", &[("q", &plain), ("limit", "25")])?;
        found = best_match(tracks(second), artist, title, duration);
    }
    // Deezer lists only a clip and no fix says how long it is: the source knows.
    if let Some(t) = found.as_mut()
        && t.duration == 0
    {
        t.duration = duration;
    }
    // Featured artists: searches don't list them; the app fills them in from the song's
    // own page once it's in the library (see `App::ensure_featured`).
    Ok(found)
}

/// Leaves out songs named after an artist (one the search names) who doesn't sing them ("Katy Perry Firework" by
/// Electronaut, "Firework (Salute to Katy Perry)"): tributes and covers. A real
/// "(feat. …)" credit stays.
fn without_tributes(q: &str, list: Vec<Track>, artists: &[ArtistInfo]) -> Vec<Track> {
    use crate::youtube::{core_title, norm};
    let typed = norm(q);
    // The artists named in the search: from the artists found, and from the songs found
    // (a search for "Firework Katy Perry" finds no artist by that name).
    let names: Vec<String> =
        artists.iter().map(|a| norm(&a.name)).chain(list.iter().map(|t| norm(&t.artist.name))).filter(|n| n.len() >= 4 && typed.contains(n.as_str())).collect();
    list.into_iter()
        .filter(|t| {
            let title = norm(&core_title(&t.title));
            let own = norm(&t.all_artists());
            let credited = own.clone() + &norm(&t.title);
            // Named after the artist but not by them: a tribute. And when the search names
            // an artist, only their songs (or songs crediting them).
            !names.iter().any(|n| title.contains(n.as_str()) && !own.contains(n.as_str()))
                && (names.is_empty() || names.iter().any(|n| credited.contains(n.as_str())))
        })
        .collect()
}

/// What Deezer offers for a song (both searches), for the live matching test.
#[cfg(test)]
pub(crate) fn candidates_for_test(artist: &str, title: &str) -> Vec<Track> {
    let (main, _) = split_artists(artist);
    let core = crate::youtube::core_title(title);
    let precise = format!("artist:\"{}\" track:\"{}\"", main.replace('"', ""), core.replace('"', ""));
    // Lengths as Deezer lists them (no fixes applied).
    let raw = |l: List<DzTrack>| l.data.into_iter().map(|t| track(t, None)).collect::<Vec<_>>();
    let mut all = get::<List<DzTrack>>("/search/track", &[("q", &precise), ("limit", "25")]).map(raw).unwrap_or_default();
    all.extend(get::<List<DzTrack>>("/search/track", &[("q", &format!("{main} {core}")), ("limit", "25")]).map(raw).unwrap_or_default());
    all
}

/// Search results' songs, cleaned up: one entry per song (Deezer lists the same song from
/// the album, the single, compilations and deluxe editions), the most played version of
/// each, no 30-second clips when the full song is there, no covers/karaoke/instrumentals
/// unless the search asks for them, and the songs that match what was typed, most
/// popular first.
pub fn rank_songs(q: &str, list: Vec<Track>) -> Vec<Track> {
    use crate::youtube::{core_title, norm, unwanted_version, words};
    let asks_cover = cover_act(q);
    let list: Vec<Track> = list.into_iter().filter(|t| !unwanted_version(q, &t.title) && (asks_cover || !cover_act(&t.artist.name))).collect();
    // One per (song, artist): keep the most played full-length version, at the place the
    // first one was.
    let mut out: Vec<Track> = vec![];
    for t in list {
        let key = |x: &Track| (norm(&core_title(&x.title)), x.artist.id.clone());
        match out.iter_mut().find(|o| key(o) == key(&t)) {
            Some(o) => {
                let full = |x: &Track| x.duration >= 60;
                // The explicit or the clean version, as Settings asks.
                let wanted = |x: &Track| x.explicit == prefer_explicit();
                if (full(&t), wanted(&t), t.rank) > (full(o), wanted(o), o.rank) {
                    *o = t;
                }
            }
            None => out.push(t),
        }
    }
    // How well a song matches what was typed: 2 every word typed is in its title or
    // artist and the whole title was typed, 1 every word, 0 otherwise.
    let typed = words(q);
    let typed: Vec<&str> = typed.split_whitespace().collect();
    let tier = |t: &Track| {
        let text = words(&format!("{} {}", t.title, t.all_artists()));
        let all = !typed.is_empty() && typed.iter().all(|w| text.contains(&format!(" {w} ")));
        let title = norm(&core_title(&t.title));
        let whole = !title.is_empty() && norm(q).contains(&title);
        u8::from(all) + u8::from(all && whole)
    };
    // Stable: Deezer's own order among equals.
    out.sort_by_key(|t| {
        let tr = tier(t);
        (std::cmp::Reverse(tr), std::cmp::Reverse(if tr == 2 { t.rank } else { 0 }))
    });
    out
}

pub fn search(q: &str) -> Result<SearchResults, String> {
    let r = search_once(q)?;
    if !r.is_empty() {
        return Ok(r);
    }
    // Nothing found: try again more loosely (no punctuation, then without the last word,
    // which is often half-typed or misspelled).
    let loose: String = q.chars().map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' }).collect();
    let loose = loose.split_whitespace().collect::<Vec<_>>().join(" ");
    if !loose.is_empty() && loose != q {
        let r = search_once(&loose)?;
        if !r.is_empty() {
            return Ok(r);
        }
    }
    match loose.rsplit_once(' ') {
        Some((shorter, _)) if !shorter.trim().is_empty() => search_once(shorter),
        _ => Ok(r),
    }
}

/// Songs, artists, albums and playlists, asked for all at once.
fn search_once(q: &str) -> Result<SearchResults, String> {
    let (t, artists, albums, playlists, users) = std::thread::scope(|s| {
        // More than are shown, so the best known one is among them (see `rank_artists`).
        let artists = s.spawn(|| get::<List<DzArtist>>("/search/artist", &[("q", q), ("limit", "30")]).unwrap_or_default());
        let albums = s.spawn(|| get::<List<DzAlbum>>("/search/album", &[("q", q), ("limit", "24")]).unwrap_or_default());
        let playlists = s.spawn(|| get::<List<DzPlaylist>>("/search/playlist", &[("q", q), ("limit", "12")]).unwrap_or_default());
        let users = s.spawn(|| crate::account::search(q));
        let t = get::<List<DzTrack>>("/search/track", &[("q", q), ("limit", "40")]);
        (t, artists.join().unwrap_or_default(), albums.join().unwrap_or_default(), playlists.join().unwrap_or_default(), users.join().unwrap_or_default())
    });
    let mut ranked = rank_artists(q, artists.data.into_iter().map(ArtistInfo::from).collect());
    ranked.truncate(12);
    // Searched for an artist: also who their listeners play, and songs like theirs.
    let around = ranked.first().filter(|a| name_match(&a.name, q) >= 2).map(|a| {
        let (related, songs) = std::thread::scope(|s| {
            let related = s.spawn(|| related_artists(&a.id, 12));
            let songs = songs_like_artist(&a.id, &a.name);
            (related.join().unwrap_or_default(), songs)
        });
        (a.name.clone(), related, songs)
    });
    Ok(SearchResults {
        tracks: without_tributes(q, rank_songs(q, tracks(t?)), &ranked),
        artists: ranked,
        around,
        users,
        albums: albums.data.into_iter().map(|a| album_info(a, None)).collect(),
        playlists: playlists.data.iter().map(playlist_info).collect(),
    })
}

/// How well an artist's name matches what was typed: 3 the same, 2 the name starts with it
/// ("tyler" → "Tyler, The Creator"), 1 a word in the name starts with it ("Steven Tyler"),
/// 0 neither (Deezer's looser matches).
pub fn name_match(name: &str, typed: &str) -> u8 {
    // Lowercase, no accents, letters and digits only: "Beyoncé" and "beyonce" are the same.
    let key = |s: &str| crate::app::fold(s).chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let (n, q) = (key(name), key(typed));
    if q.is_empty() {
        return 0;
    }
    if n == q || n.strip_prefix("the").is_some_and(|rest| rest == q) {
        return 3;
    }
    if n.starts_with(&q) || n.strip_prefix("the").is_some_and(|rest| rest.starts_with(&q)) {
        return 2;
    }
    let words = crate::app::fold(name);
    let first = crate::app::fold(typed);
    let first = first.split_whitespace().next().unwrap_or_default();
    if !first.is_empty() && words.split_whitespace().any(|w| w.starts_with(first)) {
        return 1;
    }
    0
}

/// Puts the artists you most likely mean first: a name that matches what you typed,
/// weighed against how many fans the artist has. Typing "tyler" puts Tyler, The Creator
/// (millions of fans) above a small artist called just "Tyler".
pub fn rank_artists(typed: &str, mut artists: Vec<ArtistInfo>) -> Vec<ArtistInfo> {
    let score = |a: &ArtistInfo| {
        let m = match name_match(&a.name, typed) {
            3 => 3.0,
            2 => 2.5,
            1 => 1.0,
            _ => -2.0,
        };
        m + ((a.fans + 1) as f64).log10()
    };
    // Stable, so artists that score the same keep Deezer's order.
    artists.sort_by(|a, b| score(b).total_cmp(&score(a)));
    artists
}

/// Finds an artist's Deezer page from their name (for songs that came from YouTube or an
/// import, which don't know it). None when no artist by that name turns up.
pub fn find_artist(name: &str) -> Result<Option<String>, String> {
    // YouTube channels: "The Weeknd - Topic", "TheWeekndVEVO".
    let mut clean = name.trim().trim_end_matches(" - Topic").trim().to_string();
    if let Some(stripped) = clean.strip_suffix("VEVO") {
        clean = stripped.trim().to_string();
    }
    if clean.is_empty() {
        return Ok(None);
    }
    let found: List<DzArtist> = get("/search/artist", &[("q", &clean), ("limit", "10")])?;
    let ranked = rank_artists(&clean, found.data.into_iter().map(ArtistInfo::from).collect());
    Ok(ranked.into_iter().find(|a| name_match(&a.name, &clean) >= 2).map(|a| a.id))
}

impl SearchResults {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.artists.is_empty() && self.albums.is_empty() && self.playlists.is_empty()
    }
}

pub fn artist_page(id: &str) -> Result<ArtistPage, String> {
    let artist: DzArtist = get(&format!("/artist/{id}"), &[])?;
    let me = artist_ref(&artist);
    let top: List<DzTrack> = get(&format!("/artist/{id}/top"), &[("limit", "50")]).unwrap_or_default();
    let albums: List<DzAlbum> = get(&format!("/artist/{id}/albums"), &[("limit", "100")]).unwrap_or_default();
    let related = related_artists(id, 12);
    let songs_like = songs_like_artist(id, &artist.name);
    let top_albums: std::collections::HashSet<u64> = top.data.iter().filter_map(|t| t.album.as_ref().map(|a| a.id)).collect();
    let (own, others) = split_namesakes(albums.data, &top_albums);
    let mut releases: Vec<AlbumInfo> = own.into_iter().map(|a| album_info(a, Some(&me))).collect();
    let mut namesakes: Vec<AlbumInfo> = others.into_iter().map(|a| album_info(a, Some(&me))).collect();
    sort_newest_first(&mut releases);
    sort_newest_first(&mut namesakes);
    // The 10 most popular songs, most popular first (by Deezer's popularity score).
    let mut top = tracks(top);
    top.sort_by_key(|t| std::cmp::Reverse(t.rank));
    top.truncate(10);
    // The artist list doesn't say how many songs a release has; ask for the newest one,
    // which the page shows big.
    if let Some(latest) = releases.first_mut()
        && latest.nb_tracks == 0
        && let Ok(full) = get::<DzAlbum>(&format!("/album/{}", latest.id), &[])
    {
        latest.nb_tracks = full.nb_tracks;
    }
    Ok(ArtistPage { artist: artist.into(), top, releases, namesakes, related, songs_like })
}

/// Deezer sometimes files many different artists who share a name (there are dozens
/// called "Che") under one artist. Their releases then span many unrelated genres. When
/// that happens, only the releases with real listeners, or with one of the artist's
/// popular songs, are kept as the artist's own; the rest are returned separately.
/// An ordinary artist (a few genres) keeps everything.
fn split_namesakes(albums: Vec<DzAlbum>, top_albums: &std::collections::HashSet<u64>) -> (Vec<DzAlbum>, Vec<DzAlbum>) {
    let genres: std::collections::HashSet<i64> = albums.iter().map(|a| a.genre_id).filter(|g| *g > 0).collect();
    if albums.len() < 12 || genres.len() < 5 {
        return (albums, Vec::new());
    }
    let most = albums.iter().map(|a| a.fans).max().unwrap_or(0);
    let enough = (most / 10).max(3);
    let (own, others): (Vec<_>, Vec<_>) = albums.into_iter().partition(|a| a.fans >= enough || top_albums.contains(&a.id));
    if own.is_empty() { (others, Vec::new()) } else { (own, others) }
}

/// Newest release first (Deezer returns them in no useful order).
pub fn sort_newest_first(releases: &mut [AlbumInfo]) {
    releases.sort_by(|a, b| b.release_date.cmp(&a.release_date));
}

/// Who made a song, from Deezer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Credits {
    /// (name, role) — role is e.g. "Main artist" or "Featured".
    pub people: Vec<(String, String)>,
    pub album: Option<String>,
    pub label: Option<String>,
    pub release_date: Option<String>,
    pub genres: Vec<String>,
    pub isrc: Option<String>,
    pub bpm: Option<f32>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DzContributor {
    name: String,
    role: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DzTrackCredits {
    contributors: Vec<DzContributor>,
    isrc: Option<String>,
    bpm: Option<f32>,
    release_date: Option<String>,
    album: Option<DzAlbum>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DzGenre {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DzGenres {
    data: Vec<DzGenre>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DzAlbumLabel {
    label: Option<String>,
    genres: DzGenres,
}

/// One song by its Deezer number.
pub fn track_by_id(id: u64) -> Result<Track, String> {
    get::<DzTrack>(&format!("/track/{id}"), &[]).map(|t| {
        let mut t = [track(t, None)];
        crate::fixes::apply(&mut t);
        let [t] = t;
        t
    })
}

/// An artist's most popular songs.
pub fn artist_top(artist_id: &str, limit: usize) -> Result<Vec<Track>, String> {
    let top: List<DzTrack> = get(&format!("/artist/{artist_id}/top"), &[("limit", &limit.to_string())])?;
    Ok(tracks(top))
}

/// Everything on a song's page.
#[derive(Debug, Clone)]
pub struct SongPage {
    pub track: Track,
    pub credits: Credits,
    pub album: Option<(AlbumInfo, Vec<Track>)>,
    pub artist_top: Vec<Track>,
}

pub fn song_page(id: u64) -> Result<SongPage, String> {
    let track = track_by_id(id)?;
    // The album and the artist's songs at the same time; then the credits, whose song and
    // album replies are already in memory by then.
    let (album, artist_top) = std::thread::scope(|s| {
        let al = s.spawn(|| track.album.as_ref().filter(|a| is_deezer_id(&a.id)).and_then(|a| album(&a.id).ok()));
        let top = if is_deezer_id(&track.artist.id) { artist_top(&track.artist.id, 6).unwrap_or_default() } else { vec![] };
        (al.join().ok().flatten(), top)
    });
    let credits = credits(id).unwrap_or_default();
    Ok(SongPage { track, credits, album, artist_top })
}

/// An album's genres.
pub fn album_genres(id: &str) -> Result<Vec<String>, String> {
    let a: DzAlbumLabel = get(&format!("/album/{id}"), &[])?;
    Ok(a.genres.data.into_iter().map(|g| g.name).filter(|g| !g.is_empty()).collect())
}

/// Song credits: artists and their roles, label, release date, genres.
pub fn credits(track_id: u64) -> Result<Credits, String> {
    let t: DzTrackCredits = get(&format!("/track/{track_id}"), &[])?;
    let mut c = Credits {
        people: t
            .contributors
            .into_iter()
            .map(|p| {
                let role = match p.role.as_str() {
                    "Main" | "" => "Main artist".to_string(),
                    other => other.to_string(),
                };
                (p.name, role)
            })
            .collect(),
        isrc: t.isrc.filter(|s| !s.is_empty()),
        bpm: t.bpm.filter(|b| *b > 0.0),
        release_date: t.release_date.filter(|s| !s.is_empty() && s != "0000-00-00"),
        ..Default::default()
    };
    if let Some(al) = t.album {
        c.album = Some(al.title.clone()).filter(|s| !s.is_empty());
        // The label and genres live on the album; credits still show without them.
        if let Ok(a) = get::<DzAlbumLabel>(&format!("/album/{}", al.id), &[]) {
            c.label = a.label.filter(|s| !s.is_empty());
            c.genres = a.genres.data.into_iter().map(|g| g.name).collect();
        }
    }
    Ok(c)
}

pub fn album(id: &str) -> Result<(AlbumInfo, Vec<Track>), String> {
    let full: DzAlbumFull = get(&format!("/album/{id}"), &[])?;
    let mut list = full.tracks.unwrap_or_default().data;
    let info = album_info(full.album, None);
    // Long albums come in pages: get the rest.
    if (list.len() as u64) < info.nb_tracks {
        list = all_pages(&format!("/album/{id}/tracks"), info.nb_tracks as usize).unwrap_or(list);
    }
    let mut t: Vec<Track> = list.into_iter().map(|t| track(t, Some(&info))).collect();
    crate::fixes::apply(&mut t);
    Ok((info, t))
}

/// Every song of a list that Deezer returns in pages (up to `expected`, max 2,000).
fn all_pages(path: &str, expected: usize) -> Result<Vec<DzTrack>, String> {
    let mut out: Vec<DzTrack> = vec![];
    let cap = expected.clamp(1, 2000);
    while out.len() < cap {
        let index = out.len().to_string();
        let page: List<DzTrack> = get(path, &[("index", &index), ("limit", "100")])?;
        if page.data.is_empty() {
            break;
        }
        out.extend(page.data);
    }
    Ok(out)
}

pub fn playlist(id: &str) -> Result<(PlaylistInfo, Vec<Track>), String> {
    let p: DzPlaylist = get(&format!("/playlist/{id}"), &[])?;
    let info = playlist_info(&p);
    let mut t = p.tracks.map(tracks).unwrap_or_default();
    // Deezer only sends the first part of big playlists here: fetch every song,
    // so playing the playlist plays all of it.
    if (t.len() as u64) < info.nb_tracks
        && let Ok(all) = all_pages(&format!("/playlist/{id}/tracks"), info.nb_tracks as usize)
        && all.len() > t.len()
    {
        t = all.into_iter().map(|x| track(x, None)).collect();
        crate::fixes::apply(&mut t);
    }
    Ok((info, t))
}

/// Songs similar to `t`, for autoplay: the artist's radio, or a search by name
/// for songs saved by older versions of Pulse.
/// An Orbit: songs like an artist's (their radio) mixed with their own hits,
/// shuffled the same way all day so the mix stays put until tomorrow.
pub fn mix(artist_id: &str) -> Result<Vec<Track>, String> {
    let radio: List<DzTrack> = get(&format!("/artist/{artist_id}/radio"), &[("limit", "40")]).unwrap_or_default();
    let top: List<DzTrack> = get(&format!("/artist/{artist_id}/top"), &[("limit", "10")])?;
    let (mut radio, top) = (tracks(radio), tracks(top));
    if radio.is_empty() {
        // No radio for this artist: use the top songs of up to five similar artists.
        radio = similar_artists_songs(artist_id);
    }
    let mut out: Vec<Track> = vec![];
    let mut top = top.into_iter();
    for (i, t) in radio.into_iter().enumerate() {
        if i % 4 == 0
            && let Some(h) = top.next()
        {
            out.push(h);
        }
        out.push(t);
    }
    out.extend(top);
    // No song twice, even as a second copy of the same recording (same title and artist).
    let mut seen = std::collections::HashSet::new();
    let mut seen_names = std::collections::HashSet::new();
    out.retain(|t| seen.insert(t.id.clone()) && seen_names.insert((crate::youtube::norm(&t.title), t.artist.id.clone())));
    // Shuffle with today's date as the seed.
    let day = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0);
    let mut seed = day ^ artist_id.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64));
    for i in (1..out.len()).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        out.swap(i, (seed >> 33) as usize % (i + 1));
    }
    Ok(out)
}

/// The best-known songs of up to `artists` similar artists, `each` per artist, most
/// similar artist first (looked up side by side).
pub fn related_top_songs(artist_id: &str, artists: usize, each: usize) -> Vec<Track> {
    let Ok(related) = get::<List<DzArtist>>(&format!("/artist/{artist_id}/related"), &[("limit", &artists.to_string())]) else {
        return vec![];
    };
    let each = each.to_string();
    let each = each.as_str();
    std::thread::scope(|s| {
        let jobs: Vec<_> = related.data.iter().take(artists).map(|a| s.spawn(move || get::<List<DzTrack>>(&format!("/artist/{}/top", a.id), &[("limit", each)]).map(tracks).unwrap_or_default())).collect();
        jobs.into_iter().filter_map(|j| j.join().ok()).flatten().collect()
    })
}

/// Fallback radio: a few songs from each of the artist's similar artists.
fn similar_artists_songs(artist_id: &str) -> Vec<Track> {
    let Ok(related) = get::<List<DzArtist>>(&format!("/artist/{artist_id}/related"), &[("limit", "5")]) else {
        return vec![];
    };
    related.data.iter().take(5).filter_map(|a| get::<List<DzTrack>>(&format!("/artist/{}/top", a.id), &[("limit", "6")]).ok()).flat_map(tracks).collect()
}

pub fn similar_to(t: &Track) -> Result<Vec<Track>, String> {
    let artist_id = if t.has_artist_page() {
        t.artist.id.clone()
    } else {
        let found: List<DzArtist> = get("/search/artist", &[("q", &t.artist.name), ("limit", "1")])?;
        match found.data.first() {
            Some(a) => a.id.to_string(),
            None => return Ok(vec![]),
        }
    };
    let radio: List<DzTrack> = get(&format!("/artist/{artist_id}/radio"), &[("limit", "30")])?;
    if !radio.data.is_empty() {
        return Ok(tracks(radio));
    }
    // No radio for this artist: use the top songs of similar artists.
    let related: List<DzArtist> = get(&format!("/artist/{artist_id}/related"), &[("limit", "5")])?;
    let mut out = vec![];
    for a in related.data {
        let top: List<DzTrack> = get(&format!("/artist/{}/top", a.id), &[("limit", "5")]).unwrap_or_default();
        out.extend(tracks(top));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    fn release(id: u64, fans: u64, genre: i64) -> DzAlbum {
        DzAlbum { id, fans, genre_id: genre, ..Default::default() }
    }

    #[test]
    fn keeps_namesakes_apart() {
        // One "Che" on Deezer: the real one (lots of fans) plus strangers in other genres.
        let mut albums = vec![release(1, 98, 116), release(2, 83, 116), release(3, 7, 113)];
        for (i, g) in [165, 106, 85, 132, 152, 144, 165, 106, 85, 132].into_iter().enumerate() {
            albums.push(release(10 + i as u64, i as u64 % 2, g));
        }
        let top = std::collections::HashSet::from([3]);
        let (own, others) = split_namesakes(albums, &top);
        assert_eq!(own.iter().map(|a| a.id).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(others.len(), 10);
        // An ordinary artist: one or two genres, everything stays.
        let albums: Vec<DzAlbum> = (0..30).map(|i| release(i, i % 3, if i % 4 == 0 { 132 } else { 116 })).collect();
        let (own, others) = split_namesakes(albums, &top);
        assert_eq!((own.len(), others.len()), (30, 0));
    }

    fn artist(name: &str, fans: u64) -> ArtistInfo {
        ArtistInfo { id: name.into(), name: name.into(), picture: None, picture_xl: None, fans, albums: 0 }
    }

    #[test]
    fn songs_like_leave_out_the_artist_and_spread_the_rest() {
        let t = |id: &str, artist_id: &str, name: &str| {
            let mut t = Track { id: id.into(), ..Default::default() };
            t.artist.id = artist_id.into();
            t.artist.name = name.into();
            t
        };
        let mut radio = vec![
            t("1", "9", "OsamaSon"),
            t("2", "5", "nine vicious"),
            t("3", "5", "nine vicious"),
            t("4", "5", "nine vicious"),
            t("2", "5", "nine vicious"),
            t("5", "6", "Nettspend"),
            t("6", "77", "Osamason"),
        ];
        other_artists_only(&mut radio, "9", "OsamaSon");
        let ids: Vec<&str> = radio.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["2", "3", "5"], "no OsamaSon, no repeats, two per artist");
        let merged = one_per_name(vec![artist("che", 9_000), artist("Nettspend", 50_000), artist("Che", 120_000)]);
        assert_eq!(merged.iter().map(|a| (a.name.as_str(), a.fans)).collect::<Vec<_>>(), [("Che", 120_000), ("Nettspend", 50_000)]);
    }

    #[test]
    fn popular_artists_come_first() {
        // Deezer's own order for "tyler" (November 2026), with fan counts.
        let found = [
            ("Tyler", 373),
            ("Tyler & Ryan", 800),
            ("Bonnie Tyler", 600_274),
            ("Brian Tyler", 113_199),
            ("Tyler, The Creator", 1_394_763),
            ("Taylor Swift", 12_777_520),
            ("Tyler Childers", 42_449),
        ];
        let ranked = rank_artists("tyler", found.iter().map(|(n, f)| artist(n, *f)).collect());
        assert_eq!(ranked[0].name, "Tyler, The Creator");
        assert!(name_match(&ranked[0].name, "tyler") >= 2);
        let ranked = rank_artists("taylor", vec![artist("Taylor", 90), artist("Taylor Swift", 12_777_520), artist("James Taylor", 300_000)]);
        assert_eq!(ranked[0].name, "Taylor Swift");
        assert_eq!(name_match("Beyoncé", "beyonce"), 3);
        assert_eq!(name_match("The Weeknd", "weeknd"), 3);
    }

    use super::*;

    // Trimmed copies of real Deezer replies (captured November 2026).
    const CHART_TRACK: &str = r#"{"id":3818963601,"title":"Dracula (with JENNIE)","title_short":"Dracula (with JENNIE)","duration":209,"rank":982905,"explicit_lyrics":true,"preview":"https://cdnt-preview.dzcdn.net/x.mp3","position":1,"artist":{"id":134790,"name":"Tame Impala","picture_medium":"https://cdn-images.dzcdn.net/images/artist/a/250x250.jpg","picture_xl":"https://cdn-images.dzcdn.net/images/artist/a/1000x1000.jpg","radio":true,"type":"artist"},"album":{"id":910510411,"title":"Dracula (with JENNIE)","cover_small":"https://cdn-images.dzcdn.net/images/cover/b/56x56.jpg","cover_big":"https://cdn-images.dzcdn.net/images/cover/b/500x500.jpg","type":"album"},"type":"track"}"#;

    #[test]
    fn converts_a_track() {
        let t = track(serde_json::from_str(CHART_TRACK).unwrap(), None);
        assert_eq!(t.id, "dz:3818963601");
        assert_eq!(t.deezer_id(), Some(3818963601));
        assert_eq!(t.artist.name, "Tame Impala");
        assert_eq!(t.artist.id, "134790");
        assert!(t.has_artist_page());
        assert_eq!(t.album.as_ref().unwrap().id, "910510411");
        assert!(t.art_small().unwrap().contains("56x56"));
        assert!(t.art_big().unwrap().contains("500x500"));
        assert!(t.explicit);
        assert_eq!(t.duration, 209);
    }

    #[test]
    fn album_tracks_get_the_album_cover() {
        // Album track lists come without covers; the album's own cover is used.
        let json = r#"{"id":1134072211,"title":"Tengo Algo Para Ti","cover_small":"s.jpg","cover_big":"b.jpg","release_date":"2026-11-06","record_type":"single","nb_tracks":1,"artist":{"id":67859242,"name":"ericdoa"},"tracks":{"data":[{"id":1,"title":"Tengo Algo Para Ti","duration":142,"artist":{"id":67859242,"name":"ericdoa"},"album":{"id":1134072211,"title":"Tengo Algo Para Ti","type":"album"}}]}}"#;
        let full: DzAlbumFull = parse(json).unwrap();
        let list = full.tracks.unwrap_or_default();
        let info = album_info(full.album, None);
        let t: Vec<Track> = list.data.into_iter().map(|t| track(t, Some(&info))).collect();
        assert_eq!(info.kind(), "Single");
        assert_eq!(info.year(), Some("2026"));
        assert_eq!(t[0].art_small(), Some("s.jpg"));
        assert_eq!(t[0].art_big(), Some("b.jpg"));
    }

    #[test]
    fn newest_release_first() {
        let mk = |t: &str, d: &str| AlbumInfo { title: t.into(), release_date: Some(d.into()), ..Default::default() };
        let mut r = vec![mk("things with wings", "2022-05-20"), mk("Tengo Algo Para Ti", "2026-11-06"), mk("Backdoor", "2026-04-10")];
        sort_newest_first(&mut r);
        assert_eq!(r[0].title, "Tengo Algo Para Ti");
        assert_eq!(r[2].title, "things with wings");
    }

    #[test]
    fn deezer_errors_become_messages() {
        let e = parse::<DzArtist>(r#"{"error":{"type":"DataException","message":"no data","code":800}}"#).unwrap_err();
        assert_eq!(e, "Not found on Deezer.");
        let e = parse::<DzArtist>(r#"{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}"#).unwrap_err();
        assert!(e.contains("busy"));
    }

    #[test]
    fn old_saved_songs_still_load() {
        // A song as Pulse 0.1 (Audius) saved it.
        let old = r#"{"id":"AK3w5q0","title":"140 Mix","duration":584,"genre":"Dubstep","play_count":1650,"artwork":{"150x150":"https://a/150.jpg","480x480":"https://a/480.jpg"},"user":{"id":"O5lQz","name":"@iLLPeTiLL","handle":"iLLPeTiLL","is_verified":true,"profile_picture":{"150x150":"p.jpg"}},"is_stream_gated":false}"#;
        let t: Track = serde_json::from_str(old).unwrap();
        assert_eq!(t.artist.name, "@iLLPeTiLL");
        assert_eq!(t.art_small(), Some("https://a/150.jpg"));
        assert!(!t.has_artist_page());
        assert_eq!(t.deezer_id(), None);
    }
}

#[cfg(test)]
mod artist_ranking_tests {
    use super::*;

    fn a(name: &str, fans: u64) -> ArtistInfo {
        ArtistInfo { id: name.to_string(), name: name.to_string(), fans, ..Default::default() }
    }

    fn names(v: &[ArtistInfo]) -> Vec<&str> {
        v.iter().map(|a| a.name.as_str()).collect()
    }

    #[test]
    fn the_artist_you_most_likely_mean_comes_first() {
        // The order Deezer might send them in.
        let tyler =
            vec![a("Tyler", 2_100), a("Tyler Childers", 400_000), a("Steven Tyler", 90_000), a("Tyler, The Creator", 6_800_000), a("Bonnie Tyler", 1_200_000)];
        assert_eq!(rank_artists("tyler", tyler)[0].name, "Tyler, The Creator");

        let taylor = vec![a("Taylor", 900), a("Taylor Swift", 31_000_000), a("James Taylor", 700_000), a("Taylor Swift Tribute Band", 1_200)];
        let r = rank_artists("taylor", taylor);
        assert_eq!(names(&r)[..2], ["Taylor Swift", "James Taylor"]);

        // A full name typed exactly still wins over a bigger artist that only starts with it.
        let drake = vec![a("Drake Bell", 900_000), a("Drake", 25_000_000), a("Drake White", 50_000)];
        assert_eq!(rank_artists("drake", drake)[0].name, "Drake");
        let weeknd = vec![a("The Weeknd Tribute", 300), a("The Weeknd", 40_000_000)];
        assert_eq!(rank_artists("the weeknd", weeknd)[0].name, "The Weeknd");
        assert_eq!(rank_artists("weeknd", vec![a("Weekend", 3_000), a("The Weeknd", 40_000_000)])[0].name, "The Weeknd");

        // A small artist with exactly the name typed beats a famous one that doesn't match.
        let r = rank_artists("Ivy", vec![a("Ariana Grande", 30_000_000), a("Ivy", 5_000)]);
        assert_eq!(r[0].name, "Ivy");
    }

    #[test]
    fn names_match_by_how_much_they_share() {
        assert_eq!(name_match("Tyler, The Creator", "tyler the creator"), 3);
        assert_eq!(name_match("The Weeknd", "weeknd"), 3);
        assert_eq!(name_match("Tyler, The Creator", "tyler"), 2);
        assert_eq!(name_match("Steven Tyler", "tyler"), 1);
        assert_eq!(name_match("Beyoncé", "beyonce"), 3);
        assert_eq!(name_match("Adele", "tyler"), 0);
        assert_eq!(name_match("Adele", ""), 0);
    }
}

#[cfg(test)]
mod matching_tests {
    use super::*;

    fn t(id: u64, title: &str, artist: &str, artist_id: &str, duration: u32, rank: u64) -> Track {
        Track {
            id: format!("dz:{id}"),
            title: title.into(),
            duration,
            rank,
            artist: ArtistRef { id: artist_id.into(), name: artist.into(), picture: None },
            ..Default::default()
        }
    }

    #[test]
    fn splits_artist_lists() {
        assert_eq!(split_artists("Nelly, City Spud"), ("Nelly".into(), vec!["City Spud".into()]));
        assert_eq!(split_artists("Nelly feat. City Spud"), ("Nelly".into(), vec!["City Spud".into()]));
        assert_eq!(split_artists("Simon & Garfunkel"), ("Simon & Garfunkel".into(), vec![]));
        assert_eq!(featured_in_title("Ride Wit Me (feat. City Spud)"), vec!["City Spud".to_string()]);
        assert!(featured_in_title("Firework").is_empty());
    }

    #[test]
    fn import_picks_the_real_song() {
        let list = vec![
            t(1, "Ride Wit Me (Instrumental)", "Nelly", "1", 291, 300_000),
            t(2, "Ride Wit Me", "Nelly", "1", 291, 700_000),
            t(3, "Ride Wit Me (feat. City Spud)", "Nelly", "1", 291, 650_000),
            t(4, "Ride Wit Me", "Karaoke Hits", "9", 290, 1_000),
        ];
        // The featured version wins over the bare title; never the instrumental.
        assert_eq!(best_match(list, "Nelly, City Spud", "Ride Wit Me", 291).unwrap().id, "dz:3");
        // Explicit wanted (the default): the uncensored version, even if less played.
        let list = vec![
            Track { explicit: false, ..t(9, "Ride Wit Me", "Nelly", "1", 291, 900_000) },
            Track { explicit: true, ..t(10, "Ride Wit Me", "Nelly", "1", 291, 600_000) },
        ];
        let _turn = EXPLICIT_TEST.lock().unwrap_or_else(|e| e.into_inner());
        set_prefer_explicit(true);
        assert_eq!(best_match(list.clone(), "Nelly", "Ride Wit Me", 291).unwrap().id, "dz:10");
        set_prefer_explicit(false);
        assert_eq!(best_match(list, "Nelly", "Ride Wit Me", 291).unwrap().id, "dz:9");
        set_prefer_explicit(true);
        // A 30-second clip isn't the song.
        let list = vec![t(5, "Firework", "Katy Perry", "2", 30, 900_000), t(6, "Firework", "Katy Perry", "2", 227, 800_000)];
        assert_eq!(best_match(list, "Katy Perry", "Firework", 228).unwrap().id, "dz:6");
        // Same length: the most played version.
        let list = vec![t(7, "Meet Me Halfway", "Black Eyed Peas", "3", 284, 10_000), t(8, "Meet Me Halfway", "Black Eyed Peas", "3", 284, 800_000)];
        assert_eq!(best_match(list, "Black Eyed Peas", "Meet Me Halfway", 284).unwrap().id, "dz:8");
        // Asked for the instrumental: it's allowed.
        let list = vec![t(1, "Ride Wit Me (Instrumental)", "Nelly", "1", 291, 300_000)];
        assert!(best_match(list, "Nelly", "Ride Wit Me (Instrumental)", 291).is_some());
    }

    #[test]
    fn rejects_karaoke_labels_and_lookalike_titles() {
        // What Deezer really offered for these (the friend's playlist).
        let list = vec![t(1, "Fireworks", "Made famous by Katy Perry", "5", 229, 65_960), t(2, "Firework", "Katy Perry", "2", 30, 240_754)];
        assert!(best_match(list, "Katy Perry", "Firework", 227).is_none());
        let list = vec![t(3, "We Found Love (feat. Calvin Harris)", "Rihanna Cover Band", "6", 214, 226_265)];
        assert!(best_match(list, "Rihanna, Calvin Harris", "We Found Love", 215).is_none());
        // Spelled differently, still the same artist.
        let list = vec![t(4, "TiK ToK", "Ke$ha", "7", 199, 900_000)];
        assert!(best_match(list, "Kesha", "TiK ToK", 199).is_some());
        let list = vec![t(5, "Run This Town", "JAŸ-Z", "8", 268, 786_242)];
        assert!(best_match(list, "JAY-Z, Rihanna, Kanye West", "Run This Town", 267).is_some());
        let list = vec![t(6, "Pump It", "The Black Eyed Peas", "9", 215, 921_100)];
        assert!(best_match(list, "Black Eyed Peas", "Pump It", 213).is_some());
    }

    #[test]
    fn only_the_same_recording() {
        assert!(same_recording("Firework", "Katy Perry", "Firework (2010 Remaster)"));
        assert!(same_recording(
            "Get Lucky (Radio Edit) [feat. Pharrell Williams and Nile Rodgers]",
            "Daft Punk",
            "Get Lucky (Radio Edit - feat. Pharrell Williams and Nile Rodgers)"
        ));
        assert!(!same_recording("Die Young", "Kesha", "Die Young (Dallas K Radio Edit)"));
        assert!(!same_recording("Hips Don't Lie (feat. Wyclef Jean)", "Shakira, Wyclef Jean", "Hips Don't Lie - Bamboo (feat. Wyclef Jean)"));
        let artists = vec![];
        let list = vec![
            t(1, "Katy Perry Firework", "Electronaut", "1", 209, 1),
            t(2, "Firework", "Katy Perry", "2", 227, 2),
            t(3, "Low (feat. Katy Perry)", "Flo Rida", "3", 200, 3),
        ];
        let ids: Vec<String> = without_tributes("firework katy perry", list, &artists).into_iter().map(|t| t.id).collect();
        assert_eq!(ids, vec!["dz:2", "dz:3"]);
    }

    #[test]
    fn search_folds_versions_and_puts_the_popular_one_first() {
        let list = vec![
            t(1, "Firework", "Katy Perry", "2", 30, 100),
            t(2, "Firework (Karaoke Version)", "Sing Along", "8", 227, 50),
            t(3, "Firework", "Katy Perry", "2", 227, 900_000),
            t(4, "Firework", "Some Cover Band", "9", 225, 2_000),
            t(5, "Fireworks", "Other", "7", 200, 5_000),
        ];
        let out = rank_songs("firework katy perry", list);
        let ids: Vec<&str> = out.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids[0], "dz:3");
        assert!(!ids.contains(&"dz:1") && !ids.contains(&"dz:2"));
    }
}
