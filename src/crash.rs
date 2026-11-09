//! Crash reports and a short log of recent problems, kept on this PC only.
//!
//! If Pulse crashes, what went wrong is written to a small text file in the data
//! folder, and the next time Pulse opens it offers to copy that report. Nothing is
//! ever sent anywhere.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

pub fn dir() -> PathBuf {
    crate::settings::data_dir().join("crash-reports")
}

fn seen_marker() -> PathBuf {
    dir().join("last-seen.txt")
}

/// Seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// "2026-11-09 13:05 UTC" for a time in seconds since 1970.
pub fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Days to a calendar date (Howard Hinnant's method).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

/// What the report says about this copy of Pulse and the PC.
pub fn about() -> String {
    format!("Pulse {} on {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH)
}

/// Writes a report when Pulse crashes (a panic on the window's thread). A problem on
/// a background thread doesn't close Pulse, so it only goes in the recent-problems log.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed").to_string();
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown problem".into());
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        note(&format!("Problem in {name}: {what} ({place})"));
        if name == "main" {
            let report = format!(
                "Pulse crash report\n\n{}\nWhen: {}\nWhat: {what}\nWhere: {place}\nThread: {name}\n\nRecent problems:\n{}\n",
                about(),
                utc(now()),
                recent().join("\n")
            );
            let _ = std::fs::create_dir_all(dir());
            let _ = std::fs::write(dir().join(format!("crash-{}.txt", now())), report);
            prune();
        }
        default(info);
    }));
}

/// Keeps the ten newest reports.
fn prune() {
    let mut files = reports();
    while files.len() > 10 {
        let _ = std::fs::remove_file(files.remove(0));
    }
}

/// Crash reports, oldest first.
pub fn reports() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir())
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))).collect())
        .unwrap_or_default();
    files.sort();
    files
}

/// A crash since Pulse last opened that you haven't been told about: (file, report).
pub fn unseen() -> Option<(PathBuf, String)> {
    let newest = reports().pop()?;
    let name = newest.file_name()?.to_string_lossy().to_string();
    let seen = std::fs::read_to_string(seen_marker()).unwrap_or_default();
    if seen.trim() == name {
        return None;
    }
    let text = std::fs::read_to_string(&newest).ok()?;
    Some((newest, text))
}

pub fn mark_seen(file: &std::path::Path) {
    if let Some(name) = file.file_name() {
        let _ = std::fs::create_dir_all(dir());
        let _ = std::fs::write(seen_marker(), name.to_string_lossy().as_bytes());
    }
}

// ---------------------------------------------------------------------------
// Recent problems (kept in memory, for crash reports and the diagnostics page)
// ---------------------------------------------------------------------------

static RECENT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Remembers a problem (the 30 newest are kept).
pub fn note(msg: &str) {
    let Ok(mut r) = RECENT.lock() else { return };
    r.push_back(format!("{}  {msg}", utc(now())));
    while r.len() > 30 {
        r.pop_front();
    }
}

pub fn recent() -> Vec<String> {
    RECENT.lock().map(|r| r.iter().cloned().collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_written_in_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc(1_794_204_633), "2026-11-09 06:10 UTC");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00 UTC");
    }

    #[test]
    fn only_the_newest_problems_are_kept() {
        for i in 0..40 {
            note(&format!("problem {i}"));
        }
        let r = recent();
        assert_eq!(r.len(), 30);
        assert!(r.last().unwrap().ends_with("problem 39"));
    }
}
