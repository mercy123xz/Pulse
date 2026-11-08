use crate::deezer::DeezerTrack;
use crate::youtube::{resolve_audio_url, search_videos, YoutubeVideo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicSource {
    Deezer,
    YouTube,
    Audius,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artwork: Option<String>,
    pub duration: u64,
    pub source: MusicSource,
    pub stream_url: Option<String>,
}

pub fn search_music(query: &str) -> Result<Vec<NormalizedTrack>, String> {
    let deezer_tracks = crate::deezer::search_tracks(query).unwrap_or_default();
    let youtube_videos = search_videos(query).unwrap_or_default();

    let mut items = Vec::new();

    for track in deezer_tracks.iter().take(10) {
        let matched = youtube_videos.iter().find(|video| youtube_match(track, video));
        let stream_url = match matched {
            Some(video) => Some(resolve_audio_url(&video.id)?),
            None => None,
        };

        items.push(NormalizedTrack {
            id: format!("deezer:{}", track.id),
            title: track.title.clone(),
            artist: track.artist_name().to_string(),
            artwork: track.artwork().map(str::to_string),
            duration: track.duration,
            source: MusicSource::Deezer,
            stream_url,
        });
    }

    if items.is_empty() {
        for video in youtube_videos.into_iter().take(10) {
            items.push(NormalizedTrack {
                id: format!("youtube:{}", video.id),
                title: video.title.clone(),
                artist: video.channel.clone(),
                artwork: None,
                duration: video.duration.unwrap_or_default(),
                source: MusicSource::YouTube,
                stream_url: Some(resolve_audio_url(&video.id)?),
            });
        }
    }

    Ok(items)
}

fn youtube_match(track: &DeezerTrack, video: &YoutubeVideo) -> bool {
    let track_title = normalize(track.title.as_str());
    let track_artist = normalize(track.artist_name());
    let video_title = normalize(video.title.as_str());
    let video_artist = normalize(video.channel.as_str());

    let title_match = track_title == video_title || track_title.contains(&video_title) || video_title.contains(&track_title);
    let artist_match = track_artist.is_empty()
        || video_artist.is_empty()
        || track_artist == video_artist
        || track_artist.contains(&video_artist)
        || video_artist.contains(&track_artist);

    title_match && artist_match
}

fn normalize(value: &str) -> String {
    let mut cleaned = value.to_lowercase();
    cleaned = cleaned
        .chars()
        .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace())
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}
