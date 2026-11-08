use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YoutubeVideo {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration: Option<u64>,
    pub url: String,
}

pub fn search_videos(query: &str) -> Result<Vec<YoutubeVideo>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(vec![]);
    }

    let output = Command::new("yt-dlp")
        .args([
            "--flat-playlist",
            "--no-warnings",
            "--print",
            "%(title)s|%(id)s|%(channel)s|%(duration)s|%(webpage_url)s",
            &format!("ytsearch10:{query}"),
        ])
        .output()
        .map_err(|_| "yt-dlp is not installed or not available on PATH".to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("YouTube search failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut found = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split('|').collect();
        if parts.len() < 5 {
            continue;
        }

        let title = parts[0].trim();
        let id = parts[1].trim();
        let channel = parts[2].trim();
        let duration = parts[3].trim().parse::<u64>().ok();
        let url = parts[4].trim();
        if title.is_empty() || id.is_empty() {
            continue;
        }

        found.push(YoutubeVideo {
            id: id.to_string(),
            title: title.to_string(),
            channel: channel.to_string(),
            duration,
            url: url.to_string(),
        });
    }

    Ok(found)
}

pub fn resolve_audio_url(video_id: &str) -> Result<String, String> {
    let output = Command::new("yt-dlp")
        .args([
            "--no-playlist",
            "--no-warnings",
            "--format",
            "bestaudio/best",
            "--get-url",
            &format!("https://www.youtube.com/watch?v={video_id}"),
        ])
        .output()
        .map_err(|_| "yt-dlp is not installed or not available on PATH".to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("YouTube stream lookup failed: {stderr}"));
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return Err("yt-dlp did not return a usable stream URL".to_string());
    }

    Ok(url)
}
