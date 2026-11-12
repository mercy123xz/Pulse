//! Sending a problem report or a crash report to the Pulse team, only when you press
//! Send. It goes to a small server (worker/ in the repo) that files it in a private
//! list only the developer can read. Your Windows user name and email addresses are
//! blanked out before it leaves your PC.

use serde_json::json;
use std::sync::mpsc::{Receiver, channel};

const ENDPOINT: &str = "https://pulse-questions.y0hyotuube.workers.dev/report";

/// What's being sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Crash,
    Problem,
}

/// Blanks out personal bits: the user folder's name and email addresses.
pub fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let lower = text.to_ascii_lowercase(); // same length as `text`, byte for byte
    let mut i = 0;
    while i < text.len() {
        // "C:\Users\<name>\..." (either slash) → "C:\Users\<user>\..."
        let rest = &lower[i..];
        if (rest.starts_with("\\users\\") || rest.starts_with("/users/")) && i > 0 {
            let head = &text[i..i + 7];
            out.push_str(head);
            let after = &text[i + 7..];
            let end = after.find(['\\', '/', '"', '\'', '\n', '\r', '<', '>']).unwrap_or(after.len());
            if end > 0 {
                out.push_str("<user>");
            }
            i += 7 + end;
            continue;
        }
        let c = text[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    // Email addresses.
    out.split_inclusive(char::is_whitespace)
        .map(|w| {
            let t = w.trim_end();
            let at = t.find('@');
            match at {
                Some(a) if a > 0 && t[a + 1..].contains('.') && !t.starts_with("http") => format!("<email>{}", &w[t.len()..]),
                _ => w.to_string(),
            }
        })
        .collect()
}

/// Sends a report in the background; the answer arrives on the receiver.
pub fn send(kind: Kind, message: &str, details: &str) -> Receiver<Result<(), String>> {
    let (tx, rx) = channel();
    let body = json!({
        "kind": if kind == Kind::Crash { "crash" } else { "problem" },
        "source": "windows",
        "version": env!("CARGO_PKG_VERSION"),
        "message": scrub(message.trim()),
        "details": scrub(details),
    });
    std::thread::spawn(move || {
        let r = crate::net::api().post(ENDPOINT).config().http_status_as_error(false).build().send_json(&body);
        let result = match r {
            Ok(resp) if resp.status().is_success() => Ok(()),
            Ok(mut resp) => {
                let v: serde_json::Value = resp.body_mut().read_json().unwrap_or_default();
                Err(v.get("error").and_then(|e| e.as_str()).unwrap_or("The report couldn't be sent right now. Please try again later.").to_string())
            }
            Err(e) => Err(crate::net::friendly_error(e)),
        };
        let _ = tx.send(result);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::scrub;

    #[test]
    fn personal_bits_are_blanked() {
        let s = scrub("at C:\\Users\\Jacob Smith\\AppData\\Roaming\\pulse-player\\x.ron and C:/Users/jake/Music, mail me: joe.b@gmail.com ok");
        assert!(!s.contains("Jacob") && !s.contains("jake") && !s.contains("joe.b"), "{s}");
        assert!(s.contains("C:\\Users\\<user>\\AppData") && s.contains("C:/Users/<user>/Music") && s.contains("<email>"), "{s}");
        assert_eq!(scrub("Couldn't play: HTTP 403"), "Couldn't play: HTTP 403");
    }
}
