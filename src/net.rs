//! Small HTTP helpers shared by the Deezer client, the YouTube setup and the audio engine.

use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

pub const USER_AGENT: &str = concat!("Pulse/", env!("CARGO_PKG_VERSION"), " (https://github.com/mercy123xz/Pulse)");

/// Client for small API calls (fails fast if the network is down).
pub fn api() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent(USER_AGENT)
            .build()
            .into()
    })
}

/// Client for audio and file downloads: no limit on how long a body may take.
pub fn downloads() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .user_agent(USER_AGENT)
            .build()
            .into()
    })
}

pub fn friendly_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(429) => "The server is rate-limiting us. Wait a few seconds and try again.".into(),
        ureq::Error::StatusCode(code) => format!("The server returned an error (HTTP {code})."),
        ureq::Error::Timeout(_) => "The request timed out. Check your internet connection.".into(),
        ureq::Error::Io(_) | ureq::Error::ConnectionFailed | ureq::Error::HostNotFound => {
            "Couldn't connect. Check your internet connection.".into()
        }
        other => format!("Network error: {other}"),
    }
}

/// One HTTP response body for a byte range of a file.
pub struct RangeResponse {
    pub body: Box<dyn Read + Send>,
    /// Size of the whole file, if the server said.
    pub total: Option<u64>,
    /// True if the server sent only the requested range (206), false if it sent the whole file.
    pub partial: bool,
}

/// Requests bytes `start..=end` (or `start..` if `end` is None) of `url`.
pub fn get_range(url: &str, headers: &[(String, String)], start: u64, end: Option<u64>) -> Result<RangeResponse, String> {
    let range = match end {
        Some(e) => format!("bytes={start}-{e}"),
        None => format!("bytes={start}-"),
    };
    let mut req = downloads().get(url).header("Range", &range);
    for (k, v) in headers {
        // Our own Range header wins; skip ones that would confuse the server.
        if !k.eq_ignore_ascii_case("range") && !k.eq_ignore_ascii_case("accept-encoding") {
            req = req.header(k.as_str(), v.as_str());
        }
    }
    let resp = req.call().map_err(friendly_error)?;
    let partial = resp.status() == 206;
    if !partial && start > 0 {
        return Err("The server can't resume this download.".into());
    }
    let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let total = if partial {
        header("content-range").and_then(|cr| cr.rsplit('/').next().and_then(|t| t.trim().parse::<u64>().ok()))
    } else {
        header("content-length").and_then(|v| v.parse::<u64>().ok())
    };
    let body = resp.into_body().into_with_config().limit(u64::MAX).reader();
    Ok(RangeResponse { body: Box::new(body), total, partial })
}

/// Downloads `url` into memory, reporting progress as (bytes so far, total if known).
pub fn download(url: &str, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String> {
    let resp = downloads().get(url).call().map_err(friendly_error)?;
    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = resp.into_body().into_with_config().limit(u64::MAX).reader();
    let mut out = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut chunk = vec![0u8; 256 * 1024];
    loop {
        let n = reader.read(&mut chunk).map_err(|e| format!("Download interrupted ({e})"))?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..n]);
        progress(out.len() as u64, total);
    }
    Ok(out)
}

/// Downloads a small text file (like a checksum list).
pub fn get_text(url: &str) -> Result<String, String> {
    let mut resp = downloads().get(url).call().map_err(friendly_error)?;
    resp.body_mut().read_to_string().map_err(|e| format!("Download interrupted ({e})"))
}

/// Downloads a web page as text, the way a browser asks for it.
pub fn get_page(url: &str) -> Result<String, String> {
    let mut resp = api()
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36 Pulse")
        .header("Accept-Language", "en-US,en;q=0.9")
        .call()
        .map_err(friendly_error)?;
    resp.body_mut().with_config().limit(20 * 1024 * 1024).read_to_string().map_err(|e| format!("Download interrupted ({e})"))
}

/// Percent-encodes text for use inside a URL.
pub fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
