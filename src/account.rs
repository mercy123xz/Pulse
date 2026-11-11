//! Pulse accounts: signing in, publishing playlists, public profiles.
//!
//! Talks to the accounts server (server/ in the repo, a Cloudflare Worker). The sign-in
//! key is kept in its own file in Pulse's data folder (not in library backups) and sent
//! only to that server. Your password is sent once to sign in and never stored.

use crate::deezer::Track;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Mutex;

/// The accounts server. Empty until it's set up (see server/README.md); online
/// features stay hidden until then.
const ACCOUNTS_URL: &str = "https://pulse-accounts.y0hyotuube.workers.dev";

/// Where public links point: the website's page for a playlist or a profile.
pub const SITE: &str = "https://mercy123xz.github.io/Pulse/";

fn base() -> String {
    // Developer check: PULSE_DEBUG_ACCOUNTS points Pulse at a test server.
    std::env::var("PULSE_DEBUG_ACCOUNTS").unwrap_or_else(|_| ACCOUNTS_URL.to_string()).trim_end_matches('/').to_string()
}

/// Are online features set up in this build?
pub fn available() -> bool {
    !base().is_empty()
}

/// You, signed in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub token: String,
    pub username: String,
    /// Where a code goes if you forget your password ("" if none was added yet).
    #[serde(default)]
    pub email: String,
}

/// A public profile.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct User {
    pub username: String,
    pub bio: String,
    pub created: u64,
}

/// A public playlist, as listed on a profile.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Listed {
    pub id: String,
    pub name: String,
    pub count: u64,
    pub cover: Option<String>,
    pub updated: u64,
}

impl Listed {
    /// As a card in a grid of playlists (the id says it's a Pulse playlist).
    pub fn info(&self, owner: &str) -> crate::deezer::PlaylistInfo {
        crate::deezer::PlaylistInfo {
            id: format!("{PREFIX}{}", self.id),
            title: self.name.clone(),
            picture: self.cover.clone(),
            creator: owner.to_string(),
            nb_tracks: self.count,
        }
    }
}

/// Pulse playlist ids in a list of playlists start with this.
pub const PREFIX: &str = "pulse:";

/// A profile and its public playlists.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub user: User,
    pub playlists: Vec<Listed>,
}

/// A public playlist with its songs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Public {
    pub id: String,
    pub name: String,
    pub description: String,
    pub username: String,
    pub cover: Option<String>,
    pub created: u64,
    pub updated: u64,
    pub tracks: Vec<Track>,
}

impl Public {
    pub fn info(&self) -> crate::deezer::PlaylistInfo {
        crate::deezer::PlaylistInfo {
            id: format!("{PREFIX}{}", self.id),
            title: self.name.clone(),
            picture: self.cover.clone(),
            creator: self.username.clone(),
            nb_tracks: self.tracks.len() as u64,
        }
    }
}

// ---------------------------------------------------------------- the saved session

fn session_file() -> PathBuf {
    crate::settings::data_dir().join("account.json")
}

static SESSION: Mutex<Option<Option<Session>>> = Mutex::new(None);

/// Who's signed in on this computer.
pub fn session() -> Option<Session> {
    let mut s = SESSION.lock().unwrap();
    if s.is_none() {
        let read = std::fs::read(session_file()).ok().and_then(|b| serde_json::from_slice(&b).ok());
        *s = Some(read);
    }
    s.clone().flatten()
}

fn set_session(new: Option<Session>) {
    match &new {
        Some(s) => {
            let _ = std::fs::create_dir_all(crate::settings::data_dir());
            let _ = std::fs::write(session_file(), serde_json::to_vec(s).unwrap_or_default());
        }
        None => {
            let _ = std::fs::remove_file(session_file());
        }
    }
    *SESSION.lock().unwrap() = Some(new);
}

// ---------------------------------------------------------------- calls

/// One request to the server. The server's own message comes back on failure
/// ("That username is taken."). A 401 on a signed-in call means the session ended.
fn call(method: &str, path: &str, body: Option<Value>, signed_in: bool) -> Result<Value, String> {
    let url = format!("{}{path}", base());
    if base().is_empty() {
        return Err("Online features aren't set up in this version of Pulse.".into());
    }
    let token = if signed_in { Some(session().ok_or("Please sign in first.")?.token) } else { None };
    let agent = crate::net::api();
    let auth = token.map(|t| format!("Bearer {t}"));
    let result = match (method, body) {
        ("GET", _) => {
            let mut r = agent.get(&url).config().http_status_as_error(false).build();
            if let Some(a) = &auth {
                r = r.header("Authorization", a);
            }
            r.call()
        }
        ("DELETE", None) => {
            let mut r = agent.delete(&url).config().http_status_as_error(false).build();
            if let Some(a) = &auth {
                r = r.header("Authorization", a);
            }
            r.call()
        }
        (m, body) => {
            let body = body.unwrap_or(json!({}));
            let mut r = match m {
                "PUT" => agent.put(&url),
                "DELETE" => agent.delete(&url).force_send_body(),
                _ => agent.post(&url),
            }
            .config()
            .http_status_as_error(false)
            .build();
            if let Some(a) = &auth {
                r = r.header("Authorization", a);
            }
            r.send_json(&body)
        }
    };
    let mut resp = result.map_err(crate::net::friendly_error)?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(8 * 1024 * 1024).read_to_string().map_err(|e| format!("Couldn't read the reply ({e})"))?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(v);
    }
    let message =
        v.get("error").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| format!("The accounts server returned an error (HTTP {status})."));
    // The session ended (not just a wrong password, which is a 401 too).
    if status == 401 && signed_in && message == "Please sign in again." {
        set_session(None);
    }
    Err(message)
}

fn parse<T: for<'de> Deserialize<'de>>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|_| "The accounts server sent something Pulse couldn't read.".to_string())
}

fn signed_in_as(v: &Value) -> Result<Session, String> {
    let token = v.get("token").and_then(Value::as_str).ok_or("No sign-in key came back.")?.to_string();
    let username = v.pointer("/user/username").and_then(Value::as_str).unwrap_or_default().to_string();
    let email = v.pointer("/user/email").and_then(Value::as_str).unwrap_or_default().to_string();
    let s = Session { token, username, email };
    set_session(Some(s.clone()));
    Ok(s)
}

pub fn sign_up(username: &str, email: &str, password: &str) -> Result<Session, String> {
    let v = call("POST", "/signup", Some(json!({ "username": username.trim(), "email": email.trim(), "password": password })), false)?;
    signed_in_as(&v)
}

/// `username` can also be the account's email address.
pub fn sign_in(username: &str, password: &str) -> Result<Session, String> {
    let v = call("POST", "/login", Some(json!({ "username": username.trim(), "password": password })), false)?;
    signed_in_as(&v)
}

/// Signs out here (and tells the server, if it can be reached).
pub fn sign_out() {
    let _ = call("POST", "/logout", None, true);
    set_session(None);
}

/// Asks the server who you are (in case your email was changed on another computer).
pub fn refresh() -> Result<Session, String> {
    let v = call("GET", "/me", None, true)?;
    let mut s = session().ok_or("Please sign in first.")?;
    s.email = v.pointer("/user/email").and_then(Value::as_str).unwrap_or_default().to_string();
    set_session(Some(s.clone()));
    Ok(s)
}

/// Adds or changes the email that password-reset codes go to.
pub fn set_email(email: &str, password: &str) -> Result<(), String> {
    call("PUT", "/me/email", Some(json!({ "email": email.trim(), "password": password })), true)?;
    if let Some(mut s) = session() {
        s.email = email.trim().to_lowercase();
        set_session(Some(s));
    }
    Ok(())
}

/// Forgot password, step 1: emails a code to the account (`who`: username or email).
/// Returns the server's message.
pub fn reset_start(who: &str) -> Result<String, String> {
    let v = call("POST", "/reset/start", Some(json!({ "who": who.trim() })), false)?;
    Ok(v.get("message").and_then(Value::as_str).unwrap_or("A code is on its way.").to_string())
}

/// Forgot password, step 2: the code from the email and a new password. Signs you in.
pub fn reset_finish(who: &str, code: &str, password: &str) -> Result<Session, String> {
    let v = call("POST", "/reset/finish", Some(json!({ "who": who.trim(), "code": code.trim(), "password": password })), false)?;
    signed_in_as(&v)
}

/// Roughly an email address (the server checks properly).
pub fn valid_email(e: &str) -> bool {
    let e = e.trim();
    match e.split_once('@') {
        Some((a, b)) => {
            !a.is_empty() && !e.contains(char::is_whitespace) && !b.contains('@') && b.split('.').count() >= 2 && b.split('.').all(|p| !p.is_empty())
        }
        None => false,
    }
}

pub fn set_bio(bio: &str) -> Result<(), String> {
    call("PUT", "/me", Some(json!({ "bio": bio })), true).map(|_| ())
}

pub fn change_password(old: &str, new: &str) -> Result<(), String> {
    call("POST", "/me/password", Some(json!({ "old": old, "new": new })), true).map(|_| ())
}

/// Deletes your account and everything you published.
pub fn delete_account(password: &str) -> Result<(), String> {
    call("DELETE", "/me", Some(json!({ "password": password })), true)?;
    set_session(None);
    Ok(())
}

/// Songs that can be published: not the ones on your computer (their ids hold file paths).
pub fn shareable(tracks: &[Track]) -> Vec<&Track> {
    tracks.iter().filter(|t| !crate::local::is_local(&t.id)).collect()
}

/// Publishes a playlist; returns its public id.
pub fn publish(name: &str, tracks: &[Track]) -> Result<String, String> {
    let v = call("POST", "/playlists", Some(json!({ "name": name, "tracks": shareable(tracks) })), true)?;
    v.get("id").and_then(Value::as_str).map(str::to_owned).ok_or_else(|| "No playlist id came back.".into())
}

/// Updates a published playlist (after you changed it).
pub fn update(id: &str, name: &str, tracks: &[Track]) -> Result<(), String> {
    call("PUT", &format!("/playlists/{id}"), Some(json!({ "name": name, "tracks": shareable(tracks) })), true).map(|_| ())
}

/// Makes a playlist private again (removes the public copy).
pub fn unpublish(id: &str) -> Result<(), String> {
    call("DELETE", &format!("/playlists/{id}"), None, true).map(|_| ())
}

pub fn playlist(id: &str) -> Result<Public, String> {
    check_id(id)?;
    parse(call("GET", &format!("/playlists/{id}"), None, false)?)
}

pub fn profile(username: &str) -> Result<Profile, String> {
    if !valid_username(username) {
        return Err("There's no one called that.".into());
    }
    parse(call("GET", &format!("/users/{}", username.to_lowercase()), None, false)?)
}

/// Usernames that start with `q` (up to 10).
pub fn search(q: &str) -> Vec<User> {
    let q: String = q.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.').take(20).collect();
    if q.len() < 2 || !available() {
        return vec![];
    }
    call("GET", &format!("/search?q={q}"), None, false)
        .ok()
        .and_then(|v| v.get("users").cloned())
        .and_then(|u| serde_json::from_value(u).ok())
        .unwrap_or_default()
}

pub fn report(id: &str, reason: &str) -> Result<(), String> {
    check_id(id)?;
    call("POST", "/report", Some(json!({ "playlist": id, "reason": reason })), false).map(|_| ())
}

fn check_id(id: &str) -> Result<(), String> {
    if id.len() == 10 && id.chars().all(|c| c.is_ascii_alphanumeric()) { Ok(()) } else { Err("That playlist link isn't right.".into()) }
}

pub fn valid_username(u: &str) -> bool {
    (1..=20).contains(&u.len()) && u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

// ---------------------------------------------------------------- links

pub fn playlist_link(id: &str) -> String {
    format!("{SITE}p.html?id={id}")
}

pub fn profile_link(username: &str) -> String {
    format!("{SITE}u.html?name={username}")
}

/// What a pasted link points at.
#[derive(Debug, PartialEq)]
pub enum Link {
    Playlist(String),
    Profile(String),
}

/// Reads a Pulse playlist or profile link ("…/Pulse/p.html?id=AbCdEfGh23", "…/u.html?name=mercy").
pub fn read_link(text: &str) -> Option<Link> {
    let t = text.trim();
    if !t.contains("/Pulse/") && !t.contains("pulse.is-a.software") {
        return None;
    }
    let query = t.split_once('?')?.1.split('#').next().unwrap_or("");
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("id", v)) if check_id(v).is_ok() && t.contains("/p.html") => return Some(Link::Playlist(v.to_string())),
            Some(("name", v)) if valid_username(v) && t.contains("/u.html") => return Some(Link::Profile(v.to_lowercase())),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pulse_links() {
        assert_eq!(read_link("https://mercy123xz.github.io/Pulse/p.html?id=AbCdEfGh23"), Some(Link::Playlist("AbCdEfGh23".into())));
        assert_eq!(read_link(" https://mercy123xz.github.io/Pulse/u.html?name=Mercy_1#x "), Some(Link::Profile("mercy_1".into())));
        assert_eq!(read_link(&playlist_link("Zz23456789")), Some(Link::Playlist("Zz23456789".into())));
        assert_eq!(read_link(&profile_link("joe")), Some(Link::Profile("joe".into())));
        assert_eq!(read_link("https://mercy123xz.github.io/Pulse/p.html?id=short"), None);
        assert_eq!(read_link("https://example.com/p.html?id=AbCdEfGh23"), None);
        assert_eq!(read_link("osamason"), None);
    }

    #[test]
    fn songs_on_your_computer_are_never_published() {
        let local = Track { id: "local:C:\\Users\\me\\Music\\a.mp3".into(), title: "a".into(), ..Default::default() };
        let online = Track { id: "dz:1".into(), title: "b".into(), ..Default::default() };
        let list = [local, online];
        let out = shareable(&list);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "dz:1");
    }

    /// The whole flow against a running server: `node server/serve.mjs`, then
    /// `PULSE_DEBUG_ACCOUNTS=http://127.0.0.1:8787 cargo test online_flow -- --ignored`.
    #[test]
    #[ignore]
    fn online_flow() {
        let dir = std::env::temp_dir().join(format!("pulse-online-{}", std::process::id()));
        unsafe { std::env::set_var("PULSE_DATA_DIR", &dir) };
        let name = format!("t{}", std::process::id());
        let mail = format!("{name}@example.com");
        assert!(sign_up(&name, "", "a good password").unwrap_err().contains("email"));
        let s = sign_up(&name, &format!("  {}  ", mail.to_uppercase()), "a good password").unwrap();
        assert_eq!((s.username.as_str(), s.email.as_str()), (name.as_str(), mail.as_str()));
        assert_eq!(session().unwrap(), s, "kept for next time");
        assert!(dir.join("account.json").is_file());
        let song = |id: &str, title: &str| {
            let mut t = Track { id: id.into(), title: title.into(), duration: 100, ..Default::default() };
            t.artist.name = "OsamaSon".into();
            t.cover_big = Some("https://cdn-images.dzcdn.net/images/cover/x/500x500.jpg".into());
            t
        };
        let tracks =
            vec![song("dz:1", "Get Away"), Track { id: "local:C:\\me\\a.mp3".into(), title: "mine".into(), ..Default::default() }, song("dz:2", "Blonde")];
        let id = publish("Rage", &tracks).unwrap();
        let p = playlist(&id).unwrap();
        assert_eq!((p.name.as_str(), p.username.as_str(), p.tracks.len()), ("Rage", name.as_str(), 2));
        assert_eq!(p.tracks[1].title, "Blonde");
        assert_eq!(p.cover.as_deref(), Some("https://cdn-images.dzcdn.net/images/cover/x/500x500.jpg"));
        update(&id, "Rage 2", &tracks[..1]).unwrap();
        assert_eq!(playlist(&id).unwrap().tracks.len(), 1);
        set_bio("hi").unwrap();
        let prof = profile(&name).unwrap();
        assert_eq!((prof.user.bio.as_str(), prof.playlists.len(), prof.playlists[0].name.as_str()), ("hi", 1, "Rage 2"));
        assert!(search(&name[..3]).iter().any(|u| u.username == name));
        assert_eq!(sign_in(&name, "wrong password").unwrap_err(), "Wrong username or password.");
        assert_eq!(sign_up(&name, "x@example.com", "a good password").unwrap_err(), "That username is taken.");
        assert_eq!(sign_in(&mail, "a good password").unwrap().username, name, "sign in with email");
        let mail2 = format!("{name}.2@example.com");
        assert_eq!(set_email(&mail2, "wrong password").unwrap_err(), "That password is wrong.");
        set_email(&mail2, "a good password").unwrap();
        assert_eq!(refresh().unwrap().email, mail2);
        // Forgot password: the local server keeps the email it would have sent.
        assert!(reset_start(&name).unwrap().contains("code"));
        let sent: Value = crate::net::api().get(&format!("{}/__mail", base())).call().unwrap().body_mut().read_json().unwrap();
        assert_eq!(sent["to"], mail2.as_str());
        let code = sent["subject"].as_str().unwrap()[..6].to_string();
        let wrong = if code == "000000" { "111111" } else { "000000" };
        assert_eq!(reset_finish(&name, wrong, "reset password 1").unwrap_err(), "That code is wrong.");
        let s = reset_finish(&mail2, &code, "reset password 1").unwrap();
        assert_eq!((s.username.as_str(), s.email.as_str()), (name.as_str(), mail2.as_str()));
        change_password("reset password 1", "a newer password").unwrap();
        unpublish(&id).unwrap();
        assert_eq!(playlist(&id).unwrap_err(), "That playlist isn't public anymore.");
        report(&"A".repeat(10), "x").unwrap_err();
        delete_account("a newer password").unwrap();
        assert!(session().is_none() && !dir.join("account.json").exists(), "signed out and forgotten");
        assert_eq!(profile(&name).unwrap_err(), "There's no one called that.");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
