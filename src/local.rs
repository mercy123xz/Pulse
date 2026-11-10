//! Songs from the Music folder ("Local Files").

use crate::audio::{Codec, StreamSource};
use crate::deezer::{AlbumRef, ArtistRef, Track};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Ids of local songs start with this, followed by the file path.
pub const PREFIX: &str = "local:";

pub fn is_local(id: &str) -> bool {
    id.starts_with(PREFIX)
}

/// Finds the music files under `dir` (up to 6 folders deep) and reads their tags.
pub fn scan(dir: &Path) -> Vec<Track> {
    let mut files = vec![];
    collect(dir, 0, &mut files);
    files.sort();
    let covers = crate::settings::data_dir().join("cache").join("covers");
    let mut tracks: Vec<Track> = files.iter().map(|f| read(f, &covers)).collect();
    tracks.sort_by_key(|t| (t.artist.name.to_lowercase(), t.album.as_ref().map(|a| a.title.to_lowercase()), t.title.to_lowercase()));
    tracks
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            collect(&path, depth + 1, out);
        } else if path.extension().and_then(|x| x.to_str()).and_then(Codec::from_extension).is_some() {
            out.push(path);
        }
    }
}

/// Builds a song from a file's tags; falls back to the file name.
fn read(path: &Path, covers: &Path) -> Track {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    let mut t = Track {
        id: format!("{PREFIX}{}", path.display()),
        title: stem.clone(),
        artist: ArtistRef { id: "local".into(), name: "Unknown artist".into(), picture: None },
        ..Default::default()
    };
    // "Artist - Title.mp3" when there are no tags.
    if let Some((a, b)) = stem.split_once(" - ") {
        t.artist.name = a.trim().to_string();
        t.title = b.trim().to_string();
    }
    let Ok(file) = lofty::read_from_path(path) else { return t };
    t.duration = file.properties().duration().as_secs() as u32;
    if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
        if let Some(v) = tag.title().filter(|v| !v.trim().is_empty()) {
            t.title = v.trim().to_string();
        }
        if let Some(v) = tag.artist().filter(|v| !v.trim().is_empty()) {
            t.artist.name = v.trim().to_string();
        }
        if let Some(v) = tag.album().filter(|v| !v.trim().is_empty()) {
            t.album = Some(AlbumRef { id: format!("local-album:{}", v.trim()), title: v.trim().to_string() });
        }
        // Save the cover picture next to Pulse's cache so the UI can show it.
        if let Some(pic) = tag.pictures().first() {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            pic.data().hash(&mut h);
            let ext = match pic.mime_type() {
                Some(lofty::picture::MimeType::Png) => "png",
                _ => "jpg",
            };
            let file = covers.join(format!("{:016x}.{ext}", h.finish()));
            if file.exists() || (std::fs::create_dir_all(covers).is_ok() && std::fs::write(&file, pic.data()).is_ok()) {
                let uri = crate::settings::file_uri(&file);
                t.cover_small = Some(uri.clone());
                t.cover_big = Some(uri);
            }
        }
    }
    t
}

/// How to play a local song.
pub fn source(t: &Track) -> Result<StreamSource, String> {
    let path = t.id.strip_prefix(PREFIX).ok_or("Not a local song")?;
    if !Path::new(path).is_file() {
        return Err("This file was moved or deleted.".into());
    }
    let ext = Path::new(path).extension().and_then(|x| x.to_str()).unwrap_or("");
    let codec = Codec::from_extension(ext).ok_or("Pulse can't play this kind of file.")?;
    Ok(StreamSource { url: format!("file://{path}"), headers: vec![], codec, content_length: None, notice: None, duration: None, keep_as: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_music_files_and_reads_names() {
        let dir = std::env::temp_dir().join(format!("pulse-local-{}", std::process::id()));
        let sub = dir.join("Album");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("Some Artist - Some Song.mp3"), b"not really audio").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let found = scan(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Some Song");
        assert_eq!(found[0].artist.name, "Some Artist");
        assert!(is_local(&found[0].id));
        let src = source(&found[0]).unwrap();
        assert_eq!(src.codec, Codec::Mp3);
        assert!(src.url.starts_with("file://"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
