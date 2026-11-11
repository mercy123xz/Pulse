//! Backups of your library: likes, playlists, history and profile.
//!
//! One is made automatically each day you use Pulse (the seven newest are kept), and
//! you can make one, export one to any folder, or restore one from Settings › Your
//! data. They're plain JSON files in the data folder; nothing leaves your PC.

use crate::app::Saved;
use std::path::{Path, PathBuf};

const KEEP_DAILY: usize = 7;
const KEEP_BEFORE_RESTORE: usize = 5;

/// Why a backup was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The automatic one each day (the seven newest are kept).
    Daily,
    /// Made with "Back up now" (kept until you delete it).
    Manual,
    /// Your library just before a restore replaced it (the five newest are kept).
    BeforeRestore,
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Kind::Daily => "library-day-",
            Kind::Manual => "library-saved-",
            Kind::BeforeRestore => "library-before-restore-",
        }
    }
}

pub fn dir() -> PathBuf {
    crate::settings::data_dir().join("backups")
}

/// A backup, as shown in Settings.
#[derive(Debug, Clone)]
pub struct Info {
    pub path: PathBuf,
    /// When it was made (seconds since 1970).
    pub made: u64,
    pub liked: usize,
    pub playlists: usize,
    pub kind: Kind,
}

/// Today's date as a number of days since 1970, for "one backup a day".
fn today() -> u64 {
    crate::crash::now() / 86_400
}

/// Writes a backup file, then drops old automatic ones.
pub fn write(saved: &Saved, kind: Kind) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir()).map_err(|e| format!("Couldn't make the backups folder ({e})"))?;
    let stamp = if kind == Kind::Daily { today() } else { crate::crash::now() };
    let path = dir().join(format!("{}{stamp}.json", kind.prefix()));
    write_to(saved, &path)?;
    prune(kind);
    Ok(path)
}

/// Writes your library to any file (Export).
pub fn write_to(saved: &Saved, path: &Path) -> Result<(), String> {
    let json = serde_json::to_vec(&library_only(saved)).map_err(|e| format!("Couldn't save the backup ({e})"))?;
    // Write next to it first, so a crash halfway never leaves a broken backup.
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, json).map_err(|e| format!("Couldn't save the backup ({e})"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Couldn't save the backup ({e})"))
}

/// Your library without the lookups Pulse can redo (view counts, matched videos…),
/// which keeps backups small.
fn library_only(saved: &Saved) -> Saved {
    let mut s = saved.clone();
    s.views.clear();
    s.album_genres.clear();
    s.featured_checked.clear();
    s.last_queue.clear();
    // File locations only mean something on this PC, and restoring keeps this PC's.
    s.downloads.clear();
    s
}

/// Makes today's backup if there isn't one yet. True if it wrote one.
pub fn daily(saved: &Saved) -> bool {
    if dir().join(format!("{}{}.json", Kind::Daily.prefix(), today())).exists() {
        return false;
    }
    write(saved, Kind::Daily).is_ok()
}

/// Keeps the newest automatic backups of a kind; your own are never removed.
fn prune(kind: Kind) {
    let keep = match kind {
        Kind::Daily => KEEP_DAILY,
        Kind::BeforeRestore => KEEP_BEFORE_RESTORE,
        Kind::Manual => return,
    };
    // The number in the name is when it was made, so this needn't open any file.
    let mut old: Vec<(u64, PathBuf)> = std::fs::read_dir(dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_str()?.strip_prefix(kind.prefix())?.strip_suffix(".json")?.parse().ok()?;
                    Some((n, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    old.sort();
    while old.len() > keep {
        let _ = std::fs::remove_file(old.remove(0).1);
    }
}

// ---------------------------------------------------------------- the spare copy

/// A second copy of the library. The main save file (eframe's) is rewritten in place, so a
/// crash or power cut in the middle of saving can leave it cut off. This one is written to
/// a temporary file first and only then put in place, so it's always whole.
pub fn spare_copy() -> PathBuf {
    crate::settings::data_dir().join("library.ron")
}

static SPARE_LAST: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
static SPARE_TURN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SPARE_WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Writes the spare copy (in the background), if the library changed since the last one.
pub fn write_spare(text: &str) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    let hash = h.finish();
    {
        let mut last = SPARE_LAST.lock().unwrap();
        if *last == hash {
            return;
        }
        *last = hash;
    }
    let turn = SPARE_TURN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let text = text.to_owned();
    std::thread::spawn(move || {
        let _one_at_a_time = SPARE_WRITING.lock();
        if SPARE_TURN.load(std::sync::atomic::Ordering::SeqCst) != turn {
            return; // a newer one is waiting to be written
        }
        if write_whole(&spare_copy(), text.as_bytes()).is_err() {
            *SPARE_LAST.lock().unwrap() = 0; // try again next save
        }
    });
}

/// Writes a file so it's never seen half-written: a temporary file, then a rename.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// Your library as Pulse last saved it. `saved` is what the main save file holds under
/// Pulse's name (None if nothing), `main_file` that file. If it can't be read, the spare
/// copy is loaded, or else the newest backup; the damaged file is kept next to the backups.
/// Returns the library and, if it had to be recovered, a message saying what happened.
pub fn load(saved: Option<String>, main_file: Option<&Path>) -> (Saved, Option<String>) {
    load_from(saved, main_file, &spare_copy(), &dir())
}

fn load_from(saved: Option<String>, main_file: Option<&Path>, spare: &Path, backups: &Path) -> (Saved, Option<String>) {
    if let Some(lib) = saved.as_deref().and_then(|t| ron::from_str::<Saved>(t).ok()) {
        return (lib, None);
    }
    let main_bytes = main_file.and_then(|f| std::fs::read(f).ok()).unwrap_or_default();
    let damaged = saved.is_some() || main_bytes.windows(14).any(|w| w == b"pulse_state_v1");
    if !damaged {
        return (Saved::default(), None); // the first time Pulse opens
    }
    // Keep the damaged file, in case there's something in it worth getting back.
    let kept = backups.join(format!("damaged-library-{}.ron", crate::crash::now()));
    let _ = std::fs::create_dir_all(backups);
    let _ = std::fs::write(&kept, &main_bytes);
    if let Some(lib) = std::fs::read_to_string(spare).ok().and_then(|t| ron::from_str::<Saved>(&t).ok()) {
        return (lib, Some("Your library file was damaged, so Pulse opened its spare copy. Nothing was lost.".into()));
    }
    let newest = std::fs::read_dir(backups)
        .map(|rd| rd.flatten().filter_map(|e| info(&e.path())).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .max_by_key(|b| b.made);
    if let Some(b) = newest
        && let Ok(lib) = read(&b.path)
    {
        let mut out = Saved::default();
        restore_into(&mut out, lib);
        let days = crate::crash::now().saturating_sub(b.made) / 86_400;
        let when = match days {
            0 => "today".to_string(),
            1 => "yesterday".to_string(),
            n => format!("{n} days ago"),
        };
        return (out, Some(format!("Your library file was damaged, so Pulse restored the backup from {when}.")));
    }
    (Saved::default(), Some("Your library file was damaged and there was no backup to restore. A copy of it was kept in the backups folder.".into()))
}

/// All backups, newest first.
pub fn list() -> Vec<Info> {
    let mut out: Vec<Info> = std::fs::read_dir(dir()).map(|rd| rd.flatten().filter_map(|e| info(&e.path())).collect()).unwrap_or_default();
    out.sort_by_key(|b| std::cmp::Reverse(b.made));
    out
}

fn info(path: &Path) -> Option<Info> {
    let name = path.file_name()?.to_str()?;
    if !name.starts_with("library-") || !name.ends_with(".json") {
        return None;
    }
    let kind = [Kind::Daily, Kind::Manual, Kind::BeforeRestore].into_iter().find(|k| name.starts_with(k.prefix()))?;
    let made = std::fs::metadata(path).and_then(|m| m.modified()).ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    // Only count the songs and playlists, without building them.
    let counts: Counts = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some(Info { path: path.to_path_buf(), made, liked: counts.liked, playlists: counts.playlists, kind })
}

#[derive(serde::Deserialize)]
struct Counts {
    #[serde(default, deserialize_with = "count")]
    liked: usize,
    #[serde(default, deserialize_with = "count")]
    playlists: usize,
}

fn count<'de, D: serde::Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    <Vec<serde::de::IgnoredAny> as serde::Deserialize>::deserialize(d).map(|v| v.len())
}

/// Reads a backup (or an exported file).
pub fn read(path: &Path) -> Result<Saved, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't open the file ({e})"))?;
    let not_backup = || "That file isn't a Pulse backup.".to_string();
    // Any JSON would read as an empty library (missing parts are filled in), which
    // restoring would then put in place of yours: insist on a library's own parts.
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| not_backup())?;
    let looks_right = ["liked", "playlists", "profile"].iter().all(|k| value.get(k).is_some());
    if !looks_right {
        return Err(not_backup());
    }
    serde_json::from_value::<Saved>(value).map_err(|_| not_backup())
}

/// Puts a backup's library in place of yours. Your settings stay as they are.
pub fn restore_into(current: &mut Saved, backup: Saved) {
    let mut lib = backup;
    // What belongs to this PC rather than to your library stays as it is: settings,
    // the player (volume, queue, what's playing), downloaded files and lookups.
    lib.settings = current.settings.clone();
    lib.volume = current.volume;
    lib.repeat = current.repeat;
    lib.shuffle = current.shuffle;
    lib.light_mode = current.light_mode;
    lib.autoplay = current.autoplay;
    lib.last_track = current.last_track.take();
    lib.last_queue = std::mem::take(&mut current.last_queue);
    lib.last_queue_pos = current.last_queue_pos;
    lib.last_position = current.last_position;
    lib.downloads = std::mem::take(&mut current.downloads);
    lib.featured_checked = std::mem::take(&mut current.featured_checked);
    for (k, v) in std::mem::take(&mut current.youtube_ids) {
        lib.youtube_ids.insert(k, v);
    }
    lib.views.extend(std::mem::take(&mut current.views));
    lib.album_genres.extend(std::mem::take(&mut current.album_genres));
    *current = lib;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backup_round_trips_and_restoring_keeps_settings() {
        let tmp = std::env::temp_dir().join(format!("pulse-backup-test-{}.json", std::process::id()));
        let mut lib = Saved::default();
        lib.liked.push(crate::deezer::Track { id: "dz:1".into(), title: "Song".into(), ..Default::default() });
        lib.views.insert("x".into(), (1, 2));
        write_to(&lib, &tmp).unwrap();
        let back = read(&tmp).unwrap();
        assert_eq!(back.liked.len(), 1);
        assert!(back.views.is_empty(), "lookups aren't backed up");

        let mut current = Saved::default();
        current.settings.mini_on_top = false;
        current.volume = 0.25;
        current.downloads.insert("dz:9".into(), "song.m4a".into());
        restore_into(&mut current, back);
        assert_eq!(current.liked.len(), 1);
        assert!(!current.settings.mini_on_top, "your settings stay");
        assert_eq!(current.volume, 0.25, "the player stays as it is");
        assert!(current.downloads.contains_key("dz:9"), "downloaded files aren't forgotten");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn a_damaged_library_is_recovered() {
        let dir = std::env::temp_dir().join(format!("pulse-recover-test-{}", std::process::id()));
        let backups = dir.join("backups");
        let spare = dir.join("library.ron");
        let main = dir.join("app.ron");
        std::fs::create_dir_all(&backups).unwrap();
        let mut lib = Saved::default();
        lib.liked.push(crate::deezer::Track { id: "dz:1".into(), title: "Song".into(), ..Default::default() });
        let text = ron::ser::to_string(&lib).unwrap();

        // Fine: read as it is, nothing to say.
        let (got, note) = load_from(Some(text.clone()), Some(&main), &spare, &backups);
        assert_eq!((got.liked.len(), note), (1, None));
        // The first time Pulse opens: an empty library, nothing to say.
        let (got, note) = load_from(None, Some(&main), &spare, &backups);
        assert_eq!((got.liked.len(), note), (0, None));

        // Cut off in the middle of saving, no spare copy, no backup: says so, keeps the file.
        std::fs::write(&main, format!("{{\"pulse_state_v1\": \"{}", &text[..text.len() / 2])).unwrap();
        let (got, note) = load_from(None, Some(&main), &spare, &backups);
        assert!(got.liked.is_empty() && note.unwrap().contains("no backup"));
        assert!(std::fs::read_dir(&backups).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("damaged-library-")));

        // With a backup: restored from it.
        let mut older = lib.clone();
        older.liked.push(crate::deezer::Track { id: "dz:2".into(), title: "Old".into(), ..Default::default() });
        write_to(&older, &backups.join("library-day-1.json")).unwrap();
        let (got, note) = load_from(Some(text[..20].to_string()), Some(&main), &spare, &backups);
        assert_eq!(got.liked.len(), 2);
        assert!(note.unwrap().contains("restored the backup from today"));

        // With the spare copy: that comes first, and nothing is lost.
        write_whole(&spare, text.as_bytes()).unwrap();
        let (got, note) = load_from(None, Some(&main), &spare, &backups);
        assert_eq!(got.liked.len(), 1);
        assert!(note.unwrap().contains("Nothing was lost"));
        assert!(!spare.with_extension("tmp").exists(), "no temporary file left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn counts_are_read_without_building_the_library() {
        let c: Counts = serde_json::from_str(r#"{"liked":[{"id":"a"},{"id":"b"}],"playlists":[{"name":"x","tracks":[]}],"profile":{}}"#).unwrap();
        assert_eq!((c.liked, c.playlists), (2, 1));
    }

    #[test]
    fn other_files_are_not_mistaken_for_backups() {
        let tmp = std::env::temp_dir().join(format!("pulse-not-backup-{}.json", std::process::id()));
        std::fs::write(&tmp, b"{\"hello\": [1, 2, 3]}").unwrap();
        assert!(read(&tmp).is_err(), "other JSON isn't taken for an empty library");
        std::fs::write(&tmp, b"not json").unwrap();
        assert!(read(&tmp).is_err());
        let _ = std::fs::remove_file(&tmp);
    }
}
