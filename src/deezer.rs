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
}

impl Track {
    /// Deezer's number for this song, if it came from Deezer.
    pub fn deezer_number(&self) -> Option<u64> {
        self.deezer_id()
    }

    pub fn deezer_id(&self) -> Option<u64> {
        self.id.strip_prefix("dz:").and_then(|s| s.parse().ok())
    }
    pub fn art_small(&self) -> Option<&str> {
        self.cover_small
            .as_deref()
            .or_else(|| self.artwork.as_ref().and_then(|a| a.small.as_deref().or(a.medium.as_deref())))
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArtistInfo {
    pub id: String,
    pub name: String,
    pub picture: Option<String>,
    pub picture_xl: Option<String>,
    pub fans: u64,
    pub albums: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
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

#[derive(Debug, Clone, Default, PartialEq)]
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
}

#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub artists: Vec<ArtistInfo>,
    pub albums: Vec<AlbumInfo>,
    pub playlists: Vec<PlaylistInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct ArtistPage {
    pub artist: ArtistInfo,
    pub top: Vec<Track>,
    /// Albums, EPs and singles, newest first.
    pub releases: Vec<AlbumInfo>,
    pub related: Vec<ArtistInfo>,
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
            picture: a.picture_medium.or(a.picture_big.clone()),
            picture_xl: a.picture_xl.or(a.picture_big),
            fans: a.nb_fan,
            albums: a.nb_album,
        }
    }
}

fn artist_ref(a: &DzArtist) -> ArtistRef {
    ArtistRef { id: a.id.to_string(), name: a.name.clone(), picture: a.picture_medium.clone() }
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
    Track {
        id: format!("dz:{}", t.id),
        title: t.title,
        duration: t.duration,
        artist: artist_ref(&t.artist),
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
    let mut req = crate::net::api().get(format!("{BASE}{path}"));
    for (k, v) in query {
        req = req.query(*k, *v);
    }
    let mut resp = req.call().map_err(crate::net::friendly_error)?;
    let text = resp
        .body_mut()
        .with_config()
        .limit(20 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| format!("Couldn't read Deezer's reply ({e})"))?;
    parse(&text)
}

fn tracks(list: List<DzTrack>) -> Vec<Track> {
    list.data.into_iter().map(|t| track(t, None)).collect()
}

/// Home page: top songs, albums, artists and playlists.
pub fn chart() -> Result<Chart, String> {
    let c: DzChart = get("/chart/0", &[])?;
    Ok(Chart {
        tracks: tracks(c.tracks),
        albums: c.albums.data.into_iter().map(|a| album_info(a, None)).collect(),
        artists: c.artists.data.into_iter().map(ArtistInfo::from).collect(),
        playlists: c.playlists.data.iter().map(playlist_info).collect(),
    })
}

/// Top 100 songs, overall (genre 0) or in a genre.
pub fn chart_tracks(genre: u32) -> Result<Vec<Track>, String> {
    get::<List<DzTrack>>(&format!("/chart/{genre}/tracks"), &[("limit", "100")]).map(tracks)
}

pub fn search(q: &str) -> Result<SearchResults, String> {
    let t: List<DzTrack> = get("/search/track", &[("q", q), ("limit", "40")])?;
    let artists: List<DzArtist> = get("/search/artist", &[("q", q), ("limit", "12")]).unwrap_or_default();
    let albums: List<DzAlbum> = get("/search/album", &[("q", q), ("limit", "12")]).unwrap_or_default();
    let playlists: List<DzPlaylist> = get("/search/playlist", &[("q", q), ("limit", "12")]).unwrap_or_default();
    Ok(SearchResults {
        tracks: tracks(t),
        artists: artists.data.into_iter().map(ArtistInfo::from).collect(),
        albums: albums.data.into_iter().map(|a| album_info(a, None)).collect(),
        playlists: playlists.data.iter().map(playlist_info).collect(),
    })
}

pub fn artist_page(id: &str) -> Result<ArtistPage, String> {
    let artist: DzArtist = get(&format!("/artist/{id}"), &[])?;
    let me = artist_ref(&artist);
    let top: List<DzTrack> = get(&format!("/artist/{id}/top"), &[("limit", "50")]).unwrap_or_default();
    let albums: List<DzAlbum> = get(&format!("/artist/{id}/albums"), &[("limit", "100")]).unwrap_or_default();
    let related: List<DzArtist> = get(&format!("/artist/{id}/related"), &[("limit", "12")]).unwrap_or_default();
    let mut releases: Vec<AlbumInfo> = albums.data.into_iter().map(|a| album_info(a, Some(&me))).collect();
    sort_newest_first(&mut releases);
    Ok(ArtistPage {
        artist: artist.into(),
        top: tracks(top),
        releases,
        related: related.data.into_iter().map(ArtistInfo::from).collect(),
    })
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
    let list = full.tracks.unwrap_or_default();
    let info = album_info(full.album, None);
    let t = list.data.into_iter().map(|t| track(t, Some(&info))).collect();
    Ok((info, t))
}

pub fn playlist(id: &str) -> Result<(PlaylistInfo, Vec<Track>), String> {
    let p: DzPlaylist = get(&format!("/playlist/{id}"), &[])?;
    let info = playlist_info(&p);
    let t = p.tracks.map(tracks).unwrap_or_default();
    Ok((info, t))
}

/// Songs similar to `t`, for autoplay: the artist's radio, or a search by name
/// for songs saved by older versions of Pulse.
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
