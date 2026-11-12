// Pulse accounts and public playlists.
//
// A Cloudflare Worker with a D1 database (binding name: DB). See server/README.md.
//
// What it stores: your username, your email address (only used to send you a code if you
// forget your password; never shown to anyone), a salted hash of your password (never the
// password), sign-in sessions (only a hash of each session key), your profile text, and
// the playlists you make public. Deleting your account deletes all of it.
//
// Sending email goes through EmailJS (your own Gmail, no domain or postal address
// needed): EMAILJS_SERVICE_ID, EMAILJS_TEMPLATE_ID, EMAILJS_PUBLIC_KEY and the secret
// EMAILJS_PRIVATE_KEY. Brevo (BREVO_API_KEY + MAIL_FROM) or Resend (RESEND_API_KEY +
// MAIL_FROM) work too. See server/README.md.

const ALLOWED_ORIGINS = ["https://mercy123xz.github.io", "https://pulse.is-a.software"];

// Limits
const MAX_BODY = 1_500_000; // bytes
const MAX_TRACKS = 2000;
const MAX_PLAYLISTS = 200; // public playlists per account
const SESSION_DAYS = 90;
const PBKDF2_ITERATIONS = 100_000; // the most Workers allow
const SIGNUPS_PER_IP_PER_HOUR = 5;
const FAILED_LOGINS_PER_15_MIN = 10;
const HIDE_AFTER_REPORTS = 3;
const RESET_MINUTES = 15; // how long a reset code works
const RESET_TRIES = 5; // wrong codes before it stops working
const RESET_MAILS_PER_IP_PER_HOUR = 5;
const RESET_MAILS_PER_ACCOUNT_PER_HOUR = 3;
const RESET_CHECKS_PER_IP_PER_15_MIN = 20;

// Pictures may only come from the music services Pulse uses, so a published playlist
// can't make other people's Pulse load pictures from (and reveal them to) any server.
const PICTURE_HOSTS = [/^cdn-images\.dzcdn\.net$/, /^e-cdns-images\.dzcdn\.net$/, /^i\.ytimg\.com$/, /^i\.scdn\.co$/, /^is\d+-ssl\.mzstatic\.com$/, /^i\d*\.sndcdn\.com$/];

const RESERVED = new Set([
  "admin", "administrator", "pulse", "support", "help", "official", "staff", "mod", "moderator", "root", "system",
  "api", "www", "settings", "login", "signup", "me", "user", "users", "null", "undefined", "deezer", "spotify", "youtube",
]);

export default {
  async fetch(request, env) {
    const origin = request.headers.get("Origin") || "";
    // DEV_ORIGIN: a local copy of the website, for testing (server/serve.mjs sets it).
    const allowed = env.DEV_ORIGIN ? [...ALLOWED_ORIGINS, env.DEV_ORIGIN] : ALLOWED_ORIGINS;
    const cors = {
      // The app sends no Origin. Pulse's own website reads public pages and can sign in.
      "Access-Control-Allow-Origin": allowed.includes(origin) ? origin : ALLOWED_ORIGINS[0],
      "Access-Control-Allow-Methods": "GET, POST, PUT, DELETE, OPTIONS",
      "Access-Control-Allow-Headers": "Content-Type, Authorization",
      "Access-Control-Max-Age": "86400",
      Vary: "Origin",
    };
    const reply = (status, data) =>
      new Response(JSON.stringify(data), { status, headers: { ...cors, "Content-Type": "application/json", "Cache-Control": "no-store" } });
    if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: cors });
    // Changes come from the app or from Pulse's own website: any other website can't sign
    // in or change anything for you. (Sign-ins are a key sent in a header, not a cookie, so
    // another site can't borrow one either.)
    if (request.method !== "GET" && origin && !allowed.includes(origin)) return reply(403, { error: "Not allowed from a website." });

    try {
      const url = new URL(request.url);
      const ip = request.headers.get("CF-Connecting-IP") || "unknown";
      const ctx = { env, request, ip, reply, web: !!origin, now: Math.floor(Date.now() / 1000) };
      return await route(ctx, request.method, url.pathname.replace(/\/+$/, "") || "/");
    } catch (e) {
      if (e instanceof Reply) return reply(e.status, { error: e.message });
      console.error(e);
      return reply(500, { error: "Something went wrong on the server. Please try again." });
    }
  },
};

class Reply extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}
const fail = (status, message) => {
  throw new Reply(status, message);
};

async function route(c, method, path) {
  const m = (re) => path.match(re);
  let p;
  if (method === "GET" && path === "/") return c.reply(200, { ok: true, service: "pulse-accounts" });
  if (method === "POST" && path === "/signup") return signup(c);
  if (method === "POST" && path === "/login") return login(c);
  if (method === "POST" && path === "/logout") return logout(c);
  if (method === "GET" && path === "/me") return me(c);
  if (method === "PUT" && path === "/me") return updateMe(c);
  if (method === "POST" && path === "/me/password") return changePassword(c);
  if (method === "DELETE" && path === "/me") return deleteMe(c);
  if (method === "PUT" && path === "/me/email") return changeEmail(c);
  if (method === "POST" && path === "/reset/start") return resetStart(c);
  if (method === "POST" && path === "/reset/finish") return resetFinish(c);
  if (method === "POST" && path === "/playlists") return publish(c);
  if ((p = m(/^\/playlists\/([A-Za-z0-9]{10})$/))) {
    if (method === "GET") return getPlaylist(c, p[1]);
    if (method === "PUT") return updatePlaylist(c, p[1]);
    if (method === "DELETE") return deletePlaylist(c, p[1]);
  }
  if (method === "GET" && (p = m(/^\/users\/([A-Za-z0-9_.]{1,20})$/))) return getUser(c, p[1]);
  if (method === "GET" && path === "/search") return searchUsers(c);
  if (method === "POST" && path === "/report") return report(c);
  if (method === "GET" && path === "/fixes") return songFixes(c);
  fail(404, "Not found.");
}

// ---------------------------------------------------------------- helpers

async function body(c) {
  const text = await c.request.text();
  if (text.length > MAX_BODY) fail(413, "That's too big.");
  try {
    const b = JSON.parse(text);
    if (!b || typeof b !== "object" || Array.isArray(b)) throw 0;
    return b;
  } catch {
    fail(400, "Couldn't read that.");
  }
}

const str = (v, max) => (typeof v === "string" ? v.trim().slice(0, max) : "");
const enc = new TextEncoder();
const hex = (buf) => [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
const randomHex = (bytes) => hex(crypto.getRandomValues(new Uint8Array(bytes)));
const sha256 = async (s) => hex(await crypto.subtle.digest("SHA-256", enc.encode(s)));

function randomId() {
  const abc = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
  const bytes = crypto.getRandomValues(new Uint8Array(10));
  return [...bytes].map((b) => abc[b % abc.length]).join("");
}

async function hashPassword(password, saltHex) {
  const key = await crypto.subtle.importKey("raw", enc.encode(password), "PBKDF2", false, ["deriveBits"]);
  const salt = new Uint8Array(saltHex.match(/../g).map((h) => parseInt(h, 16)));
  return hex(await crypto.subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt, iterations: PBKDF2_ITERATIONS }, key, 256));
}

function sameText(a, b) {
  if (a.length !== b.length) return false;
  let d = 0;
  for (let i = 0; i < a.length; i++) d |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return d === 0;
}

function checkUsername(u) {
  if (u.length < 3 || u.length > 20) fail(400, "Usernames are 3 to 20 characters.");
  if (!/^[a-z0-9_.]+$/.test(u)) fail(400, "Usernames can use letters, numbers, _ and . only.");
  if (/^[._]|[._]$|\.\./.test(u)) fail(400, "Usernames can't start or end with . or _, or have .. in them.");
  if (RESERVED.has(u)) fail(400, "That username is taken.");
}

function checkPassword(p, username) {
  if (typeof p !== "string" || p.length < 8) fail(400, "Passwords need at least 8 characters.");
  if (p.length > 200) fail(400, "That password is too long.");
  if (p.toLowerCase().includes(username)) fail(400, "Your password can't contain your username.");
}

function checkEmail(e) {
  if (!e) fail(400, "Enter your email address. It's how you get back in if you forget your password.");
  if (e.length > 254 || !/^[^\s@]+@[^\s@]+\.[^\s@.]{2,}$/.test(e)) fail(400, "That email address doesn't look right.");
}

async function emailFree(c, email, exceptId = 0) {
  const row = await c.env.DB.prepare("SELECT id FROM users WHERE email = ? AND id != ?").bind(email, exceptId).first();
  if (row) fail(409, "That email address is already used by another account.");
}

// A user by username, or by email if it has an @ in it.
const findUser = (c, who) =>
  c.env.DB.prepare(`SELECT id, username, email, pass_hash, pass_salt, bio, created FROM users WHERE ${who.includes("@") ? "email" : "username"} = ?`).bind(who).first();

const emailjsReady = (env) => !!(env.EMAILJS_SERVICE_ID && env.EMAILJS_TEMPLATE_ID && env.EMAILJS_PUBLIC_KEY && env.EMAILJS_PRIVATE_KEY);
const mailReady = (env) => typeof env.MAIL_HOOK === "function" || emailjsReady(env) || !!(env.MAIL_FROM && (env.BREVO_API_KEY || env.RESEND_API_KEY));

// Sends one email through EmailJS, Brevo or Resend, whichever is set up. `params` are
// the values an EmailJS template fills in.
async function sendMail(env, to, subject, text, html, params = {}) {
  if (typeof env.MAIL_HOOK === "function") return env.MAIL_HOOK({ to, subject, text, html, params }); // tests
  if (emailjsReady(env)) {
    const r = await fetch("https://api.emailjs.com/api/v1.0/email/send", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        service_id: env.EMAILJS_SERVICE_ID,
        template_id: env.EMAILJS_TEMPLATE_ID,
        user_id: env.EMAILJS_PUBLIC_KEY,
        accessToken: env.EMAILJS_PRIVATE_KEY,
        // "to_email" is ours; "email" is what EmailJS's ready-made templates use.
        template_params: { to_email: to, email: to, subject, message: text, ...params },
      }),
    });
    if (!r.ok) {
      console.error("emailjs", r.status, await r.text().catch(() => ""));
      fail(502, "The email couldn't be sent. Please try again in a few minutes.");
    }
    return;
  }
  const from = String(env.MAIL_FROM || "");
  const m = from.match(/^\s*(.*?)\s*<([^>]+)>\s*$/);
  const sender = m ? { name: m[1] || "Pulse", email: m[2] } : { name: "Pulse", email: from.trim() };
  if (!sender.email || !(env.BREVO_API_KEY || env.RESEND_API_KEY)) fail(503, "Password reset isn't set up on the server yet.");
  const r = env.BREVO_API_KEY
    ? await fetch("https://api.brevo.com/v3/smtp/email", {
        method: "POST",
        headers: { "api-key": env.BREVO_API_KEY, "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify({ sender, to: [{ email: to }], subject, textContent: text, htmlContent: html }),
      })
    : await fetch("https://api.resend.com/emails", {
        method: "POST",
        headers: { Authorization: `Bearer ${env.RESEND_API_KEY}`, "Content-Type": "application/json" },
        body: JSON.stringify({ from: `${sender.name} <${sender.email}>`, to: [to], subject, text, html }),
      });
  if (!r.ok) {
    console.error("mail", r.status, await r.text().catch(() => ""));
    fail(502, "The email couldn't be sent. Please try again in a few minutes.");
  }
}

// Counts something per key in the last `secs` seconds (and records this one).
async function overLimit(c, kind, key, max, secs) {
  const db = c.env.DB;
  await db.prepare("DELETE FROM attempts WHERE at < ?").bind(c.now - 86400).run();
  const row = await db.prepare("SELECT COUNT(*) AS n FROM attempts WHERE kind = ? AND key = ? AND at >= ?").bind(kind, key, c.now - secs).first();
  return (row?.n || 0) >= max;
}
const note = (c, kind, key) => c.env.DB.prepare("INSERT INTO attempts (kind, key, at) VALUES (?, ?, ?)").bind(kind, key, c.now).run();

async function newSession(c, userId) {
  const token = randomHex(32);
  await c.env.DB.prepare("INSERT INTO sessions (token_hash, user_id, created, expires) VALUES (?, ?, ?, ?)")
    .bind(await sha256(token), userId, c.now, c.now + SESSION_DAYS * 86400)
    .run();
  return token;
}

// The signed-in user, or a 401.
async function auth(c) {
  const h = c.request.headers.get("Authorization") || "";
  const token = h.startsWith("Bearer ") ? h.slice(7).trim() : "";
  if (!/^[0-9a-f]{64}$/.test(token)) fail(401, "Please sign in again.");
  const row = await c.env.DB.prepare(
    "SELECT u.id, u.username, u.email, u.bio, u.created, s.token_hash FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = ? AND s.expires > ?",
  )
    .bind(await sha256(token), c.now)
    .first();
  if (!row) fail(401, "Please sign in again.");
  return row;
}

// Keeps only what Pulse needs from each song, so a playlist can't carry anything else.
function cleanTracks(list) {
  if (!Array.isArray(list)) fail(400, "Those songs couldn't be read.");
  if (list.length > MAX_TRACKS) fail(400, `Public playlists can have up to ${MAX_TRACKS} songs.`);
  const pic = (v) => {
    if (typeof v !== "string" || v.length > 400) return null;
    try {
      const u = new URL(v);
      return u.protocol === "https:" && PICTURE_HOSTS.some((re) => re.test(u.hostname)) ? v : null;
    } catch {
      return null;
    }
  };
  const ref = (a) => (a && typeof a === "object" ? { id: str(a.id, 40), name: str(a.name, 200), picture: pic(a.picture) } : { id: "", name: "", picture: null });
  const out = [];
  for (const t of list) {
    if (!t || typeof t !== "object") continue;
    const id = str(t.id, 120);
    // Songs on your own computer have file paths in them: never published.
    if (!id || id.startsWith("local:")) continue;
    const title = str(t.title, 300);
    if (!title) continue;
    out.push({
      id,
      title,
      duration: Number.isInteger(t.duration) && t.duration >= 0 && t.duration < 100000 ? t.duration : 0,
      artist: ref(t.artist),
      album: t.album && typeof t.album === "object" ? { id: str(t.album.id, 40), title: str(t.album.title, 300) } : null,
      cover_small: pic(t.cover_small),
      cover_big: pic(t.cover_big),
      explicit: t.explicit === true,
      featured: Array.isArray(t.featured) ? t.featured.slice(0, 10).map(ref) : [],
    });
  }
  return out;
}

const publicUser = (u) => ({ username: u.username, bio: u.bio || "", created: u.created });
// Only ever sent to the account's owner.
const ownUser = (u) => ({ ...publicUser(u), email: u.email || "" });

// ---------------------------------------------------------------- accounts

async function signup(c) {
  // New accounts are made in the app (a sign-up form on the open web would mostly invite bots).
  if (c.web) fail(403, "Create your account in the Pulse app: Settings → Account.");
  const b = await body(c);
  const username = str(b.username, 40).toLowerCase();
  checkUsername(username);
  checkPassword(b.password, username);
  // Pulse 1.0 doesn't send an email at all: those accounts are made without one, and
  // Pulse 1.1 asks for it when it opens. Newer versions always send it.
  const email = "email" in b ? str(b.email, 300).toLowerCase() : null;
  if (email !== null) checkEmail(email);
  if (await overLimit(c, "signup", c.ip, SIGNUPS_PER_IP_PER_HOUR, 3600)) fail(429, "Too many new accounts from here. Please try again later.");
  const taken = await c.env.DB.prepare("SELECT 1 FROM users WHERE username = ?").bind(username).first();
  if (taken) fail(409, "That username is taken.");
  if (email) await emailFree(c, email);
  const salt = randomHex(16);
  const hash = await hashPassword(b.password, salt);
  const res = await c.env.DB.prepare("INSERT INTO users (username, email, pass_hash, pass_salt, bio, created) VALUES (?, ?, ?, ?, '', ?)")
    .bind(username, email, hash, salt, c.now)
    .run();
  await note(c, "signup", c.ip);
  const id = res.meta?.last_row_id;
  const token = await newSession(c, id);
  return c.reply(201, { token, user: { username, bio: "", created: c.now, email: email || "" } });
}

async function login(c) {
  const b = await body(c);
  const username = str(b.username, 300).toLowerCase(); // or an email address
  const password = typeof b.password === "string" ? b.password : "";
  const key = `${username}|${c.ip}`;
  if (await overLimit(c, "login", key, FAILED_LOGINS_PER_15_MIN, 900)) fail(429, "Too many tries. Please wait 15 minutes.");
  const u = username ? await findUser(c, username) : null;
  // The same work either way, so timing doesn't tell whether a username exists.
  const hash = await hashPassword(password.slice(0, 200), u ? u.pass_salt : "00000000000000000000000000000000");
  if (!u || !sameText(hash, u.pass_hash)) {
    await note(c, "login", key);
    fail(401, "Wrong username or password.");
  }
  const token = await newSession(c, u.id);
  return c.reply(200, { token, user: ownUser(u) });
}

async function logout(c) {
  const u = await auth(c);
  await c.env.DB.prepare("DELETE FROM sessions WHERE token_hash = ?").bind(u.token_hash).run();
  return c.reply(200, { ok: true });
}

async function me(c) {
  const u = await auth(c);
  const lists = await c.env.DB.prepare("SELECT id, name, count, cover, updated FROM playlists WHERE user_id = ? ORDER BY updated DESC").bind(u.id).all();
  return c.reply(200, { user: ownUser(u), playlists: lists.results || [] });
}

async function updateMe(c) {
  const u = await auth(c);
  const b = await body(c);
  const bio = str(b.bio, 300);
  await c.env.DB.prepare("UPDATE users SET bio = ? WHERE id = ?").bind(bio, u.id).run();
  return c.reply(200, { user: { ...ownUser(u), bio } });
}

async function changePassword(c) {
  const u = await auth(c);
  const b = await body(c);
  const row = await c.env.DB.prepare("SELECT pass_hash, pass_salt FROM users WHERE id = ?").bind(u.id).first();
  if (!sameText(await hashPassword(String(b.old || "").slice(0, 200), row.pass_salt), row.pass_hash)) fail(401, "Your current password is wrong.");
  checkPassword(b.new, u.username);
  const salt = randomHex(16);
  await c.env.DB.prepare("UPDATE users SET pass_hash = ?, pass_salt = ? WHERE id = ?").bind(await hashPassword(b.new, salt), salt, u.id).run();
  // Signed out everywhere else.
  await c.env.DB.prepare("DELETE FROM sessions WHERE user_id = ? AND token_hash != ?").bind(u.id, u.token_hash).run();
  return c.reply(200, { ok: true });
}

async function changeEmail(c) {
  const u = await auth(c);
  const b = await body(c);
  const row = await c.env.DB.prepare("SELECT pass_hash, pass_salt FROM users WHERE id = ?").bind(u.id).first();
  // A password too, so someone with only your session can't point resets at themselves.
  if (!sameText(await hashPassword(String(b.password || "").slice(0, 200), row.pass_salt), row.pass_hash)) fail(401, "That password is wrong.");
  const email = str(b.email, 300).toLowerCase();
  checkEmail(email);
  await emailFree(c, email, u.id);
  await c.env.DB.prepare("UPDATE users SET email = ? WHERE id = ?").bind(email, u.id).run();
  await c.env.DB.prepare("DELETE FROM resets WHERE user_id = ?").bind(u.id).run();
  return c.reply(200, { user: { ...ownUser(u), email } });
}

// Forgot password, step 1: email a code to the account's address. The reply is the same
// whether or not the account exists, so this can't be used to find out who has one.
async function resetStart(c) {
  const b = await body(c);
  const who = str(b.who, 300).toLowerCase();
  if (!who) fail(400, "Enter your username or email address.");
  if (!mailReady(c.env)) fail(503, "Password reset isn't set up on the server yet.");
  if (await overLimit(c, "reset", c.ip, RESET_MAILS_PER_IP_PER_HOUR, 3600)) fail(429, "Too many codes asked for from here. Please try again later.");
  await note(c, "reset", c.ip);
  const sent = { ok: true, message: "If that account has an email address, a code is on its way. Check your spam folder if it doesn't arrive in a minute." };
  const u = await findUser(c, who);
  if (!u || !u.email) return c.reply(200, sent);
  if (await overLimit(c, "reset-user", String(u.id), RESET_MAILS_PER_ACCOUNT_PER_HOUR, 3600)) return c.reply(200, sent);
  await note(c, "reset-user", String(u.id));
  const code = String(crypto.getRandomValues(new Uint32Array(1))[0] % 1_000_000).padStart(6, "0");
  await c.env.DB.prepare("INSERT OR REPLACE INTO resets (user_id, code_hash, expires, tries) VALUES (?, ?, ?, 0)")
    .bind(u.id, await sha256(`${u.id}:${code}`), c.now + RESET_MINUTES * 60)
    .run();
  const text = `Hi @${u.username},\n\nYour Pulse code is ${code}\n\nEnter it in Pulse to choose a new password. It works for ${RESET_MINUTES} minutes.\n\nIf you didn't ask for this, you can ignore this email: your password hasn't changed.`;
  const html = `<div style="font-family:Segoe UI,Arial,sans-serif;max-width:440px;margin:auto;padding:24px;color:#1b1a20">
<p style="margin:0 0 16px">Hi @${u.username},</p>
<p style="margin:0 0 8px">Your Pulse code is</p>
<p style="font-size:32px;font-weight:700;letter-spacing:6px;margin:0 0 16px;color:#6d28d9">${code}</p>
<p style="margin:0 0 16px">Enter it in Pulse to choose a new password. It works for ${RESET_MINUTES} minutes.</p>
<p style="margin:0;color:#6b6880;font-size:13px">If you didn't ask for this, you can ignore this email: your password hasn't changed.</p></div>`;
  // Where the request came from, so the owner can tell if it wasn't them.
  const cf = c.request.cf || {};
  const place = [cf.city, cf.region, cf.country].filter(Boolean).join(", ") || "an unknown location";
  const when = new Date(c.now * 1000).toUTCString();
  await sendMail(c.env, u.email, `${code} is your Pulse code`, text, html, { username: u.username, code, minutes: String(RESET_MINUTES), ip: c.ip, location: place, time: when });
  return c.reply(200, sent);
}

// Forgot password, step 2: the code and a new password. Signs you in.
async function resetFinish(c) {
  const b = await body(c);
  const who = str(b.who, 300).toLowerCase();
  const code = str(b.code, 20).replace(/\s/g, "");
  if (await overLimit(c, "reset-check", c.ip, RESET_CHECKS_PER_IP_PER_15_MIN, 900)) fail(429, "Too many tries. Please wait 15 minutes.");
  await note(c, "reset-check", c.ip);
  const expired = "That code has expired. Ask for a new one.";
  const u = who ? await findUser(c, who) : null;
  const row = u ? await c.env.DB.prepare("SELECT code_hash, expires, tries FROM resets WHERE user_id = ?").bind(u.id).first() : null;
  if (!row || row.expires < c.now) fail(400, expired);
  if (!/^\d{6}$/.test(code) || !sameText(await sha256(`${u.id}:${code}`), row.code_hash)) {
    if (row.tries + 1 >= RESET_TRIES) {
      await c.env.DB.prepare("DELETE FROM resets WHERE user_id = ?").bind(u.id).run();
      fail(400, "Too many wrong codes. Ask for a new one.");
    }
    await c.env.DB.prepare("UPDATE resets SET tries = tries + 1 WHERE user_id = ?").bind(u.id).run();
    fail(400, "That code is wrong.");
  }
  checkPassword(b.password, u.username);
  const salt = randomHex(16);
  const db = c.env.DB;
  await db.batch([
    db.prepare("UPDATE users SET pass_hash = ?, pass_salt = ? WHERE id = ?").bind(await hashPassword(b.password, salt), salt, u.id),
    db.prepare("DELETE FROM resets WHERE user_id = ?").bind(u.id),
    db.prepare("DELETE FROM sessions WHERE user_id = ?").bind(u.id), // signed out everywhere
  ]);
  const token = await newSession(c, u.id);
  return c.reply(200, { token, user: ownUser(u) });
}

async function deleteMe(c) {
  const u = await auth(c);
  const b = await body(c);
  const row = await c.env.DB.prepare("SELECT pass_hash, pass_salt FROM users WHERE id = ?").bind(u.id).first();
  if (!sameText(await hashPassword(String(b.password || "").slice(0, 200), row.pass_salt), row.pass_hash)) fail(401, "That password is wrong.");
  const db = c.env.DB;
  await db.batch([
    db.prepare("DELETE FROM reports WHERE playlist_id IN (SELECT id FROM playlists WHERE user_id = ?)").bind(u.id),
    db.prepare("DELETE FROM playlists WHERE user_id = ?").bind(u.id),
    db.prepare("DELETE FROM sessions WHERE user_id = ?").bind(u.id),
    db.prepare("DELETE FROM resets WHERE user_id = ?").bind(u.id),
    db.prepare("DELETE FROM users WHERE id = ?").bind(u.id),
  ]);
  return c.reply(200, { ok: true });
}

// ---------------------------------------------------------------- playlists

function playlistFields(b) {
  const name = str(b.name, 100);
  if (!name) fail(400, "Give the playlist a name.");
  const tracks = cleanTracks(b.tracks);
  if (!tracks.length) fail(400, "There are no songs to share (songs from your computer can't be shared).");
  const cover = tracks.map((t) => t.cover_big || t.cover_small).find(Boolean) || null;
  return { name, description: str(b.description, 300), tracks, cover };
}

async function publish(c) {
  const u = await auth(c);
  const f = playlistFields(await body(c));
  const n = await c.env.DB.prepare("SELECT COUNT(*) AS n FROM playlists WHERE user_id = ?").bind(u.id).first();
  if ((n?.n || 0) >= MAX_PLAYLISTS) fail(400, `You can have up to ${MAX_PLAYLISTS} public playlists.`);
  let id = randomId();
  while (await c.env.DB.prepare("SELECT 1 FROM playlists WHERE id = ?").bind(id).first()) id = randomId();
  await c.env.DB.prepare(
    "INSERT INTO playlists (id, user_id, name, description, tracks, count, cover, created, updated, hidden) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0)",
  )
    .bind(id, u.id, f.name, f.description, JSON.stringify(f.tracks), f.tracks.length, f.cover, c.now, c.now)
    .run();
  return c.reply(201, { id, count: f.tracks.length });
}

async function ownPlaylist(c, id) {
  const u = await auth(c);
  const row = await c.env.DB.prepare("SELECT id, user_id FROM playlists WHERE id = ?").bind(id).first();
  if (!row) fail(404, "That playlist isn't public anymore.");
  if (row.user_id !== u.id) fail(403, "That's someone else's playlist.");
  return u;
}

async function updatePlaylist(c, id) {
  await ownPlaylist(c, id);
  const f = playlistFields(await body(c));
  await c.env.DB.prepare("UPDATE playlists SET name = ?, description = ?, tracks = ?, count = ?, cover = ?, updated = ? WHERE id = ?")
    .bind(f.name, f.description, JSON.stringify(f.tracks), f.tracks.length, f.cover, c.now, id)
    .run();
  return c.reply(200, { id, count: f.tracks.length });
}

async function deletePlaylist(c, id) {
  await ownPlaylist(c, id);
  const db = c.env.DB;
  await db.batch([db.prepare("DELETE FROM reports WHERE playlist_id = ?").bind(id), db.prepare("DELETE FROM playlists WHERE id = ?").bind(id)]);
  return c.reply(200, { ok: true });
}

async function getPlaylist(c, id) {
  const row = await c.env.DB.prepare(
    "SELECT p.id, p.name, p.description, p.tracks, p.count, p.cover, p.created, p.updated, p.hidden, u.username FROM playlists p JOIN users u ON u.id = p.user_id WHERE p.id = ?",
  )
    .bind(id)
    .first();
  if (!row || row.hidden) fail(404, "That playlist isn't public anymore.");
  const { tracks, hidden, ...rest } = row;
  return c.reply(200, { ...rest, tracks: JSON.parse(tracks) });
}

async function getUser(c, name) {
  const u = await c.env.DB.prepare("SELECT id, username, bio, created FROM users WHERE username = ?").bind(name.toLowerCase()).first();
  if (!u) fail(404, "There's no one called that.");
  const lists = await c.env.DB.prepare("SELECT id, name, count, cover, updated FROM playlists WHERE user_id = ? AND hidden = 0 ORDER BY updated DESC").bind(u.id).all();
  return c.reply(200, { user: publicUser(u), playlists: lists.results || [] });
}

async function searchUsers(c) {
  const q = (new URL(c.request.url).searchParams.get("q") || "").toLowerCase().replace(/[^a-z0-9_.]/g, "").slice(0, 20);
  if (q.length < 2) return c.reply(200, { users: [] });
  const rows = await c.env.DB.prepare("SELECT username, bio, created FROM users WHERE username >= ? AND username < ? ORDER BY username LIMIT 10")
    .bind(q, q + "￿")
    .all();
  return c.reply(200, { users: rows.results || [] });
}

// Song fixes: real lengths for songs the catalog lists as a 30-second clip, and the video
// to play for songs that keep finding the wrong one. Read-only here; added in Cloudflare.
async function songFixes(c) {
  const ids = [...new Set((new URL(c.request.url).searchParams.get("ids") || "").split(","))]
    .map((s) => s.trim())
    .filter((s) => /^[a-z]+:[\w.-]{1,64}$/i.test(s))
    .slice(0, 100);
  const fixes = {};
  if (ids.length) {
    const rows = await c.env.DB.prepare(`SELECT id, duration, video FROM song_fixes WHERE id IN (${ids.map(() => "?").join(",")})`)
      .bind(...ids)
      .all();
    for (const r of rows.results || []) {
      const fix = {};
      if (Number.isInteger(r.duration) && r.duration > 0) fix.duration = r.duration;
      if (r.video && /^[\w-]{6,20}$/.test(r.video)) fix.video = r.video;
      if (Object.keys(fix).length) fixes[r.id] = fix;
    }
  }
  return c.reply(200, { fixes });
}

async function report(c) {
  const b = await body(c);
  const id = str(b.playlist, 20);
  const reason = str(b.reason, 500);
  if (!/^[A-Za-z0-9]{10}$/.test(id)) fail(400, "Which playlist?");
  if (await overLimit(c, "report", c.ip, 20, 3600)) fail(429, "Too many reports. Please try again later.");
  const p = await c.env.DB.prepare("SELECT id FROM playlists WHERE id = ?").bind(id).first();
  if (!p) fail(404, "That playlist isn't public anymore.");
  const reporter = await sha256(`${c.ip}|${id}`);
  await c.env.DB.prepare("INSERT OR IGNORE INTO reports (playlist_id, reporter, reason, at) VALUES (?, ?, ?, ?)").bind(id, reporter, reason, c.now).run();
  await note(c, "report", c.ip);
  // Enough different people reported it: hidden until the owner of Pulse looks at it.
  const n = await c.env.DB.prepare("SELECT COUNT(*) AS n FROM reports WHERE playlist_id = ?").bind(id).first();
  if ((n?.n || 0) >= HIDE_AFTER_REPORTS) await c.env.DB.prepare("UPDATE playlists SET hidden = 1 WHERE id = ?").bind(id).run();
  return c.reply(200, { ok: true });
}
