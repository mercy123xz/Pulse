import worker from "./worker.js";
import { makeD1 } from "./d1.mjs";
import assert from "node:assert/strict";
const outbox = [];
const env = { DB: makeD1(), MAIL_HOOK: (m) => outbox.push(m) };
let ip = "1.1.1.1";
async function call(method, path, body, { token, origin } = {}) {
  const headers = { "CF-Connecting-IP": ip };
  if (token) headers.Authorization = "Bearer " + token;
  if (origin) headers.Origin = origin;
  const r = await worker.fetch(new Request("https://acc.example" + path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }), env);
  return { status: r.status, data: await r.json() };
}
let r, n = 0; const ok = (c, m) => { assert.ok(c, m); n++; };

r = await call("GET", "/"); ok(r.status === 200 && r.data.ok, "alive");
// signup rules
for (const [u, p, want] of [["ab", "password1", 400], ["Bad Name", "password1", 400], ["admin", "password1", 400], [".joe", "password1", 400], ["joe", "short", 400], ["joe", "xxjoexx123", 400]])
  { r = await call("POST", "/signup", { username: u, password: p, email: u.replace(/\W/g, "") + "@example.com" }); ok(r.status === want, `signup ${u}/${p} -> ${r.status} ${r.data.error}`); }
r = await call("POST", "/signup", { username: "nomail", password: "correct horse" }); ok(r.status === 400 && /email/.test(r.data.error), "email required");
r = await call("POST", "/signup", { username: "nomail", password: "correct horse", email: "not-an-email" }); ok(r.status === 400, "email checked");
r = await call("POST", "/signup", { username: "Mercy_1", password: "correct horse", email: " Mercy@Example.com " }); ok(r.status === 201 && /^[0-9a-f]{64}$/.test(r.data.token) && r.data.user.username === "mercy_1" && r.data.user.email === "mercy@example.com", "signup ok, lowercased");
const tok = r.data.token;
r = await call("POST", "/signup", { username: "mercy_1", password: "another one", email: "x@example.com" }); ok(r.status === 409, "taken");
r = await call("POST", "/signup", { username: "mercy_2", password: "another one", email: "MERCY@example.com" }); ok(r.status === 409 && /email/.test(r.data.error), "email used once");
// nothing secret stored in the clear
const row = env.DB.raw.prepare("SELECT * FROM users").get(); ok(row.pass_hash.length === 64 && !JSON.stringify(row).includes("correct horse"), "password hashed");
ok(!env.DB.raw.prepare("SELECT token_hash FROM sessions").all().some(s => s.token_hash === tok), "only token hash stored");
// login
r = await call("POST", "/login", { username: "mercy_1", password: "wrong pass" }); ok(r.status === 401 && r.data.error === "Wrong username or password.", "wrong password");
r = await call("POST", "/login", { username: "nobody", password: "wrong pass" }); ok(r.status === 401 && r.data.error === "Wrong username or password.", "same message for unknown user");
r = await call("POST", "/login", { username: "MERCY_1", password: "correct horse" }); ok(r.status === 200 && r.data.token !== tok, "login ok");
const tok2 = r.data.token;
r = await call("POST", "/login", { username: "Mercy@Example.com", password: "correct horse" }); ok(r.status === 200 && r.data.user.username === "mercy_1", "login with email");
r = await call("GET", "/me", undefined, { token: tok }); ok(r.data.user.email === "mercy@example.com", "me shows own email");
r = await call("GET", "/me", undefined, { token: tok }); ok(r.status === 200 && r.data.user.username === "mercy_1", "me");
r = await call("GET", "/me", undefined, { token: "f".repeat(64) }); ok(r.status === 401, "bad token");
r = await call("GET", "/me"); ok(r.status === 401, "no token");
r = await call("PUT", "/me", { bio: "  hi there  " }, { token: tok }); ok(r.data.user.bio === "hi there", "bio");
// websites can't change things
r = await call("POST", "/login", { username: "mercy_1", password: "correct horse" }, { origin: "https://evil.example" }); ok(r.status === 403, "no website writes");
r = await call("GET", "/users/mercy_1", undefined, { origin: "https://mercy123xz.github.io" }); ok(r.status === 200, "website can read");
// publish
const tracks = [
  { id: "dz:1", title: "Get Away", duration: 100, artist: { id: "141771412", name: "OsamaSon", picture: "https://cdn-images.dzcdn.net/images/artist/x/56x56.jpg" }, album: { id: "9", title: "psykotic" }, cover_small: "https://cdn-images.dzcdn.net/images/cover/a/56x56.jpg", cover_big: "https://evil.example/track.png", explicit: true, featured: [], secret: "x" },
  { id: "local:C:\\Users\\me\\Music\\a.mp3", title: "private", duration: 1, artist: { id: "", name: "" } },
  { id: "yt:abc", title: "Song", duration: 90, artist: { id: "", name: "Someone" }, cover_big: "https://i.ytimg.com/vi/abc/hqdefault.jpg" },
];
r = await call("POST", "/playlists", { name: "Rage", tracks }, { token: tok }); ok(r.status === 201 && /^[A-Za-z0-9]{10}$/.test(r.data.id) && r.data.count === 2, "publish, local song dropped");
const pid = r.data.id;
r = await call("GET", "/playlists/" + pid); ok(r.status === 200 && r.data.username === "mercy_1" && r.data.tracks.length === 2, "public read");
ok(r.data.tracks[0].cover_big === null && r.data.tracks[0].cover_small.includes("dzcdn") && r.data.tracks[0].secret === undefined, "pictures only from music services, extra fields dropped");
ok(r.data.cover === "https://cdn-images.dzcdn.net/images/cover/a/56x56.jpg", "cover from first allowed picture");
ok(!JSON.stringify(r.data).includes("Users\\\\me"), "no file paths");
r = await call("POST", "/playlists", { name: "only local", tracks: [tracks[1]] }, { token: tok }); ok(r.status === 400, "nothing shareable");
r = await call("POST", "/playlists", { name: "x", tracks: Array(2001).fill(tracks[0]) }, { token: tok }); ok(r.status === 400, "too many songs");
r = await call("POST", "/playlists", { name: "Rage", tracks }); ok(r.status === 401, "publish needs sign in");
// someone else
ip = "2.2.2.2";
r = await call("POST", "/signup", { username: "other", password: "second pass 1", email: "other@example.com" }); const other = r.data.token;
r = await call("PUT", "/playlists/" + pid, { name: "mine now", tracks }, { token: other }); ok(r.status === 403, "can't edit others");
r = await call("DELETE", "/playlists/" + pid, undefined, { token: other }); ok(r.status === 403, "can't delete others");
r = await call("PUT", "/playlists/" + pid, { name: "Rage 2", tracks: [tracks[0]] }, { token: tok2 }); ok(r.status === 200 && r.data.count === 1, "owner updates from other session");
r = await call("GET", "/users/MERCY_1"); ok(r.status === 200 && r.data.playlists.length === 1 && r.data.playlists[0].name === "Rage 2" && r.data.user.bio === "hi there", "profile");
ok(!("pass_hash" in r.data.user) && !("id" in r.data.user) && !("email" in r.data.user), "profile shows nothing private");
r = await call("GET", "/search?q=mer"); ok(r.data.users.length === 1 && r.data.users[0].username === "mercy_1", "user search");
// reports: three different people hide it, the same one can't count twice
for (const a of ["3.3.3.3", "3.3.3.3", "4.4.4.4"]) { ip = a; r = await call("POST", "/report", { playlist: pid, reason: "spam" }); ok(r.status === 200, "report"); }
r = await call("GET", "/playlists/" + pid); ok(r.status === 200, "two reporters: still up");
ip = "5.5.5.5"; await call("POST", "/report", { playlist: pid, reason: "spam" });
r = await call("GET", "/playlists/" + pid); ok(r.status === 404, "three reporters: hidden");
// rate limits
ip = "6.6.6.6";
for (let i = 0; i < 10; i++) await call("POST", "/login", { username: "other", password: "nope nope" });
r = await call("POST", "/login", { username: "other", password: "second pass 1" }); ok(r.status === 429, "login limited after 10 misses");
ip = "7.7.7.7";
for (let i = 0; i < 5; i++) { r = await call("POST", "/signup", { username: "bulk" + i, password: "bulk pass 1", email: `bulk${i}@example.com` }); ok(r.status === 201, "signup " + i); }
r = await call("POST", "/signup", { username: "bulk9", password: "bulk pass 1", email: "bulk9@example.com" }); ok(r.status === 429, "signup limited per IP");
// password change signs out other sessions
r = await call("POST", "/me/password", { old: "wrong", new: "new password 2" }, { token: tok }); ok(r.status === 401, "old password checked");
r = await call("POST", "/me/password", { old: "correct horse", new: "new password 2" }, { token: tok }); ok(r.status === 200, "password changed");
r = await call("GET", "/me", undefined, { token: tok2 }); ok(r.status === 401, "other session signed out");
r = await call("GET", "/me", undefined, { token: tok }); ok(r.status === 200, "this session kept");
// change email: needs the password, can't take someone else's
r = await call("PUT", "/me/email", { email: "new@example.com", password: "wrong" }, { token: tok }); ok(r.status === 401, "email change needs password");
r = await call("PUT", "/me/email", { email: "other@example.com", password: "new password 2" }, { token: tok }); ok(r.status === 409, "email taken");
r = await call("PUT", "/me/email", { email: "New@Example.com", password: "new password 2" }, { token: tok }); ok(r.status === 200 && r.data.user.email === "new@example.com", "email changed");
// forgot password
ip = "9.9.9.9";
outbox.length = 0;
r = await call("POST", "/reset/start", { who: "nobody" }); ok(r.status === 200 && outbox.length === 0, "unknown account: same reply, no mail");
r = await call("POST", "/reset/start", { who: "MERCY_1" }); ok(r.status === 200 && outbox.length === 1 && outbox[0].to === "new@example.com", "code mailed");
let code = outbox[0].subject.slice(0, 6); ok(/^\d{6}$/.test(code) && outbox[0].text.includes(code), "six-digit code");
ok(!JSON.stringify(env.DB.raw.prepare("SELECT * FROM resets").all()).includes(code), "only the code's hash stored");
const wrong = code === "000000" ? "111111" : "000000";
r = await call("POST", "/reset/finish", { who: "mercy_1", code: wrong, password: "reset pass 3" }); ok(r.status === 400 && r.data.error === "That code is wrong.", "wrong code");
r = await call("POST", "/reset/finish", { who: "new@example.com", code, password: "short" }); ok(r.status === 400 && /8/.test(r.data.error), "new password checked");
r = await call("POST", "/reset/finish", { who: "new@example.com", code: code.slice(0, 3) + " " + code.slice(3), password: "reset pass 3" }); ok(r.status === 200 && /^[0-9a-f]{64}$/.test(r.data.token), "reset ok, signed in");
const tok4 = r.data.token;
r = await call("GET", "/me", undefined, { token: tok }); ok(r.status === 401, "reset signs out everywhere else");
r = await call("POST", "/reset/finish", { who: "mercy_1", code, password: "reset pass 4" }); ok(r.status === 400 && /expired/.test(r.data.error), "code works once");
outbox.length = 0;
await call("POST", "/reset/start", { who: "mercy_1" }); code = outbox[0].subject.slice(0, 6);
for (let i = 0; i < 5; i++) r = await call("POST", "/reset/finish", { who: "mercy_1", code: code === "000000" ? "111111" : "000000", password: "reset pass 4" });
ok(r.data.error === "Too many wrong codes. Ask for a new one.", "5 wrong codes and it's gone");
r = await call("POST", "/reset/finish", { who: "mercy_1", code, password: "reset pass 4" }); ok(r.status === 400, "right code no longer works");
// (2 codes mailed to this account so far this hour)
ip = "9.9.9.10"; outbox.length = 0;
await call("POST", "/reset/start", { who: "mercy_1" });
r = await call("POST", "/reset/start", { who: "mercy_1" }); ok(r.status === 200 && outbox.length === 1, "3 mails per account per hour (and the reply doesn't say)");
for (let i = 0; i < 3; i++) await call("POST", "/reset/start", { who: "other" });
r = await call("POST", "/reset/start", { who: "other" }); ok(r.status === 429, "5 asks per address per hour");
const noMail = { DB: env.DB };
r = await worker.fetch(new Request("https://acc.example/reset/start", { method: "POST", headers: { "CF-Connecting-IP": "10.0.0.1" }, body: JSON.stringify({ who: "mercy_1" }) }), noMail);
ok(r.status === 503, "says so when mail isn't set up");
r = await call("POST", "/login", { username: "mercy_1", password: "reset pass 3" }); ok(r.status === 200, "new password works");
// logout, delete
r = await call("POST", "/logout", undefined, { token: tok4 }); r = await call("GET", "/me", undefined, { token: tok4 }); ok(r.status === 401, "logged out");
ip = "8.8.8.8"; r = await call("POST", "/login", { username: "mercy_1", password: "reset pass 3" }); const tok3 = r.data.token;
r = await call("DELETE", "/me", { password: "nope" }, { token: tok3 }); ok(r.status === 401, "delete needs password");
r = await call("DELETE", "/me", { password: "reset pass 3" }, { token: tok3 }); ok(r.status === 200, "deleted");
ok(env.DB.raw.prepare("SELECT COUNT(*) n FROM playlists").get().n === 0 && env.DB.raw.prepare("SELECT COUNT(*) n FROM reports").get().n === 0, "playlists and reports gone");
r = await call("GET", "/users/mercy_1"); ok(r.status === 404, "profile gone");
// junk
r = await worker.fetch(new Request("https://acc.example/signup", { method: "POST", body: "{nope" }), env); ok(r.status === 400, "bad json");
r = await call("GET", "/nothing"); ok(r.status === 404, "404");
console.log(`all ${n} server checks passed`);
