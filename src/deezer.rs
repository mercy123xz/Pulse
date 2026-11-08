use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct DeezerArtist {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct DeezerAlbum {
    pub id: String,
    pub title: String,
    pub cover_medium: Option<String>,
    pub cover_big: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct DeezerTrack {
    pub id: String,
    pub title: String,
    pub duration: u64,
    pub artist: DeezerArtist,
    pub album: Option<DeezerAlbum>,
    pub link: Option<String>,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
struct DeezerSearchResponse {
    data: Vec<DeezerTrack>,
}

pub fn search_tracks(query: &str) -> Result<Vec<DeezerTrack>, String> {
    let encoded = query.replace(' ', "%20");
    let url = format!("https://api.deezer.com/search/track?q={encoded}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .user_agent("Pulse/0.1 (Rust desktop music player)")
        .build()
        .into();
    let mut response = agent.get(&url).call().map_err(|e| format!("Deezer search failed: {e}"))?;
    let payload: DeezerSearchResponse = response
        .body_mut()
        .with_config()
        .limit(2 * 1024 * 1024)
        .read_json()
        .map_err(|e| format!("Deezer search response was malformed: {e}"))?;
    Ok(payload.data)}

impl DeezerTrack {
    pub fn artwork(&self) -> Option<&str> {
        self.album
            .as_ref()
            .and_then(|album| album.cover_big.as_deref().or(album.cover_medium.as_deref()))
    }

    pub fn artist_name(&self) -> &str {
        self.artist.name.as_str()
    }
}
