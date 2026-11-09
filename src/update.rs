//! Updates: once a day Pulse asks GitHub for the newest release. When there's a newer
//! one, it can download the installer, check it, and run it. Pulse restarts after.
//!
//! An installer is only run when two checks pass. Its SHA-256 must match the one GitHub
//! lists for the file. And it must carry a valid signature made with Pulse's release
//! key: the release workflow signs every installer, and the matching public key is
//! built into Pulse. A file changed anywhere on the way fails the signature check and
//! is never run. Pulse then offers the release page instead.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

const LATEST: &str = "https://api.github.com/repos/mercy123xz/Pulse/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/mercy123xz/Pulse/releases/latest";

/// The public half of Pulse's release signing key (Ed25519). Same key as
/// installer/update-key.pub.pem, which the release workflow checks signatures with.
const PUBLIC_KEY: [u8; 32] = [
    0x84, 0x90, 0x2c, 0x74, 0x7e, 0xd6, 0x58, 0x25, 0x85, 0xe7, 0x38, 0x2b, 0x43, 0xd4, 0x1a, 0xa0, 0x09, 0xa4, 0x94, 0x37, 0xf9, 0x72, 0x27, 0xa4,
    0x29, 0x97, 0xab, 0x84, 0xc9, 0x99, 0x45, 0x05,
];

/// Where the release workflow puts the signature: a comment in the release notes,
/// so it doesn't show on the page and the release only lists the installer.
const SIGNATURE_TAG: &str = "<!-- pulse-signature ed25519:";

/// A release newer than this copy of Pulse.
#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    /// The release's page on GitHub, with its notes.
    pub page: String,
    installer_url: String,
    installer_name: String,
    size: u64,
    /// "sha256:…" as GitHub lists it for the file.
    digest: Option<String>,
    signature: Option<Vec<u8>>,
}

impl Release {
    /// Signed releases can be installed from inside Pulse.
    pub fn signed(&self) -> bool {
        self.signature.is_some()
    }
}

/// What the updater is doing.
#[derive(Clone)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(Release, Arc<Mutex<f32>>),
    /// The installer is running; Pulse closes so it can be replaced.
    Installing,
    /// The check didn't work.
    Failed(String),
    /// Downloading or checking the installer didn't work.
    InstallFailed(Release, String),
}

enum Msg {
    Checked(Result<Option<Release>, String>),
    Downloaded(Result<PathBuf, String>),
}

pub struct Updater {
    pub status: Status,
    rx: Option<Receiver<Msg>>,
    /// The banner was closed (until the next check finds something).
    pub banner_closed: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Self { status: Status::Idle, rx: None, banner_closed: false }
    }
}

impl Updater {
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    /// Asks GitHub for the newest release.
    pub fn check(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        self.status = Status::Checking;
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Checked(latest().map(|r| r.filter(|r| is_newer(&r.version, env!("CARGO_PKG_VERSION"))))));
            ctx.request_repaint();
        });
        self.rx = Some(rx);
    }

    /// Downloads and checks the installer; `poll` runs it once it's ready.
    pub fn install(&mut self, ctx: &egui::Context) {
        let (Status::Available(release) | Status::InstallFailed(release, _)) = self.status.clone() else { return };
        if self.busy() {
            return;
        }
        let progress = Arc::new(Mutex::new(0.0));
        self.status = Status::Downloading(release.clone(), progress.clone());
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = download_checked(&release, &|p| {
                if let Ok(mut v) = progress.lock() {
                    *v = p;
                }
                ctx.request_repaint();
            });
            let _ = tx.send(Msg::Downloaded(result));
            ctx.request_repaint();
        });
        self.rx = Some(rx);
    }

    /// Picks up finished checks and downloads. Returns true when Pulse should close
    /// for the installer.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let msg = match rx.try_recv() {
            Ok(m) => m,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                return false;
            }
        };
        self.rx = None;
        let release = match &self.status {
            Status::Downloading(r, _) => Some(r.clone()),
            _ => None,
        };
        let failed = |e: String| match release.clone() {
            Some(r) => Status::InstallFailed(r, e),
            None => Status::Failed(e),
        };
        match msg {
            Msg::Checked(Ok(Some(r))) => {
                self.banner_closed = false;
                self.status = Status::Available(r);
            }
            Msg::Checked(Ok(None)) => self.status = Status::UpToDate,
            Msg::Checked(Err(e)) => self.status = Status::Failed(e),
            Msg::Downloaded(Ok(path)) => match run_installer(&path) {
                Ok(()) => {
                    self.status = Status::Installing;
                    return true;
                }
                Err(e) => self.status = failed(e),
            },
            Msg::Downloaded(Err(e)) => self.status = failed(e),
        }
        false
    }
}

/// The newest release on GitHub (not drafts or pre-releases), if it has an installer.
fn latest() -> Result<Option<Release>, String> {
    // Developer check: PULSE_DEBUG_RELEASE=<file> reads the answer from a file.
    if let Some(file) = std::env::var_os("PULSE_DEBUG_RELEASE") {
        return parse_release(&std::fs::read_to_string(file).map_err(|e| e.to_string())?);
    }
    let mut resp = crate::net::api().get(LATEST).header("Accept", "application/vnd.github+json").call().map_err(crate::net::friendly_error)?;
    let body = resp.body_mut().with_config().limit(4 * 1024 * 1024).read_to_string().map_err(|e| format!("Couldn't read GitHub's answer ({e})"))?;
    parse_release(&body)
}

fn parse_release(json: &str) -> Result<Option<Release>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| "GitHub's answer didn't make sense".to_string())?;
    let version = v["tag_name"].as_str().unwrap_or_default().trim_start_matches('v').to_string();
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let installer = assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.starts_with("PulseSetup-") && n.ends_with(".exe")));
    let Some(installer) = installer else { return Ok(None) };
    let notes = v["body"].as_str().unwrap_or_default();
    Ok(Some(Release {
        version,
        page: v["html_url"].as_str().unwrap_or(RELEASES_PAGE).to_string(),
        installer_url: installer["browser_download_url"].as_str().unwrap_or_default().to_string(),
        installer_name: installer["name"].as_str().unwrap_or_default().to_string(),
        size: installer["size"].as_u64().unwrap_or(0),
        digest: installer["digest"].as_str().map(str::to_owned),
        signature: signature_in(notes),
    }))
}

/// The signature hidden in the release notes, if there is one.
fn signature_in(notes: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let start = notes.find(SIGNATURE_TAG)? + SIGNATURE_TAG.len();
    let b64 = notes[start..].split("-->").next()?.trim();
    base64::engine::general_purpose::STANDARD.decode(b64).ok().filter(|s| s.len() == 64)
}

/// "0.4.1" newer than "0.4.0"? Anything after a "-" or "+" is ignored.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Option<(u64, u64, u64)> {
        let core = v.trim().trim_start_matches('v').split(['-', '+']).next()?;
        let mut it = core.split('.');
        let mut next = || it.next().map_or(Some(0), |p| p.parse().ok());
        Some((next()?, next()?, next()?))
    }
    matches!((parts(latest), parts(current)), (Some(a), Some(b)) if a > b)
}

/// Whether `signature` is Pulse's release key's signature of `bytes`.
fn signature_ok(bytes: &[u8], signature: &[u8]) -> bool {
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, PUBLIC_KEY).verify(bytes, signature).is_ok()
}

/// Downloads the installer, checks it, and saves it in the temp folder.
fn download_checked(r: &Release, progress: &dyn Fn(f32)) -> Result<PathBuf, String> {
    if r.signature.is_none() {
        return Err(crate::i18n::tr("This release isn't signed, so Pulse won't install it by itself.").to_string());
    }
    // Only GitHub's own addresses: the file must come from the release.
    if !r.installer_url.starts_with("https://github.com/mercy123xz/Pulse/releases/download/") {
        return Err(crate::i18n::tr("The installer isn't on Pulse's release page, so Pulse won't run it.").to_string());
    }
    let bytes = crate::net::download(&r.installer_url, |done, total| {
        let total = total.unwrap_or(r.size).max(1);
        progress((done as f32 / total as f32).min(1.0));
    })?;
    check_installer(r, &bytes)?;
    let dir = std::env::temp_dir().join("pulse-update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't save the update ({e})"))?;
    // The name comes from GitHub: keep only plain characters.
    let name: String = r.installer_name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
    let path = dir.join(if name.ends_with(".exe") { name } else { "PulseSetup.exe".into() });
    std::fs::write(&path, &bytes).map_err(|e| format!("Couldn't save the update ({e})"))?;
    Ok(path)
}

/// The checks a downloaded installer must pass before it's run: the size and SHA-256
/// GitHub lists, and the release key's signature.
fn check_installer(r: &Release, bytes: &[u8]) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let tr = crate::i18n::tr;
    let signature = r.signature.as_deref().ok_or_else(|| tr("This release isn't signed, so Pulse won't install it by itself.").to_string())?;
    if r.size > 0 && bytes.len() as u64 != r.size {
        return Err(tr("The download was incomplete. Try again.").to_string());
    }
    if let Some(digest) = &r.digest {
        let expected = digest.strip_prefix("sha256:").unwrap_or(digest).to_ascii_lowercase();
        let got: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        if got != expected {
            return Err(tr("The download doesn't match the release. It was not installed.").to_string());
        }
    }
    if !signature_ok(bytes, signature) {
        return Err(tr("The installer's signature isn't valid. It was not installed.").to_string());
    }
    Ok(())
}

/// Whether this copy of Pulse came from the installer (its uninstaller sits next to it).
/// A copy run from anywhere else is left alone: updating it means downloading again.
pub fn installed() -> bool {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("unins000.exe").is_file())).unwrap_or(false)
}

/// Runs the installer without its windows. It closes Pulse if needed, replaces it,
/// and starts the new version.
fn run_installer(path: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new(path);
    cmd.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"]);
    cmd.spawn().map(|_| ()).map_err(|e| format!("Couldn't start the installer ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.4.0", "0.3.3"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.3.3", "0.3.3"));
        assert!(!is_newer("0.3.2", "0.3.3"));
        assert!(!is_newer("0.4.0-beta", "0.4.0"));
        assert!(!is_newer("nonsense", "0.3.3"));
    }

    #[test]
    fn the_release_key_checks_signatures() {
        // Signed with the release key by `openssl pkeyutl -sign -rawin`, as the workflow does.
        let sig = test_signature();
        assert!(signature_ok(b"Pulse update signature test", &sig));
        assert!(!signature_ok(b"Pulse update signature test!", &sig), "a changed file fails");
        let mut bad = sig.clone();
        bad[0] ^= 1;
        assert!(!signature_ok(b"Pulse update signature test", &bad), "a changed signature fails");
    }

    fn test_signature() -> Vec<u8> {
        let hex = "64504ff71a066e50e2deb401d05b6cc7ad6c6cde149ac9e7468547e9bdc42278dcf644190e9556894d4941c2b67f84931a98e1212435a8297efac7b61237e708";
        (0..64).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect()
    }

    #[test]
    fn only_an_untouched_signed_installer_passes() {
        use sha2::{Digest, Sha256};
        let file = b"Pulse update signature test";
        let digest: String = Sha256::digest(file).iter().map(|b| format!("{b:02x}")).collect();
        let release = Release {
            version: "9.9.9".into(),
            page: String::new(),
            installer_url: String::new(),
            installer_name: "PulseSetup-9.9.9.exe".into(),
            size: file.len() as u64,
            digest: Some(format!("sha256:{digest}")),
            signature: Some(test_signature()),
        };
        assert!(check_installer(&release, file).is_ok());
        assert!(check_installer(&release, b"Pulse update signature tes").is_err(), "wrong size");
        assert!(check_installer(&release, b"Pulse update signature tesT").is_err(), "changed file, same size");
        let no_digest = Release { digest: None, ..release.clone() };
        assert!(check_installer(&no_digest, file).is_ok());
        assert!(check_installer(&no_digest, b"Pulse update signature tesT").is_err(), "the signature alone catches it");
        let wrong_digest = Release { digest: Some("sha256:00".into()), ..release.clone() };
        assert!(check_installer(&wrong_digest, file).is_err());
        let unsigned = Release { signature: None, ..release };
        assert!(check_installer(&unsigned, file).is_err(), "never run an unsigned file");
    }

    #[test]
    fn reads_a_release_and_its_hidden_signature() {
        let sig = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [7u8; 64]);
        let json = format!(
            r#"{{"tag_name":"v0.4.0","html_url":"https://github.com/mercy123xz/Pulse/releases/tag/v0.4.0",
            "body":"What's new\n\n{SIGNATURE_TAG}{sig} -->",
            "assets":[{{"name":"PulseSetup-0.4.0.exe","size":123,"digest":"sha256:ab",
            "browser_download_url":"https://github.com/mercy123xz/Pulse/releases/download/v0.4.0/PulseSetup-0.4.0.exe"}}]}}"#
        );
        let r = parse_release(&json).unwrap().unwrap();
        assert_eq!(r.version, "0.4.0");
        assert!(r.signed());
        assert_eq!(r.size, 123);
        let unsigned = parse_release(&json.replace(SIGNATURE_TAG, "<!-- ")).unwrap().unwrap();
        assert!(!unsigned.signed());
        assert!(parse_release(r#"{"tag_name":"v1.0.0","assets":[]}"#).unwrap().is_none(), "no installer, nothing to offer");
    }
}
