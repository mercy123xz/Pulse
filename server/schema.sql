-- Pulse accounts database (Cloudflare D1). Run once in the D1 console.

CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  username TEXT NOT NULL UNIQUE,
  pass_hash TEXT NOT NULL,
  pass_salt TEXT NOT NULL,
  bio TEXT NOT NULL DEFAULT '',
  created INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL,
  created INTEGER NOT NULL,
  expires INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_user ON sessions (user_id);

CREATE TABLE IF NOT EXISTS playlists (
  id TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  tracks TEXT NOT NULL,
  count INTEGER NOT NULL,
  cover TEXT,
  created INTEGER NOT NULL,
  updated INTEGER NOT NULL,
  hidden INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS playlists_user ON playlists (user_id);

CREATE TABLE IF NOT EXISTS reports (
  playlist_id TEXT NOT NULL,
  reporter TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  at INTEGER NOT NULL,
  PRIMARY KEY (playlist_id, reporter)
);

CREATE TABLE IF NOT EXISTS attempts (
  kind TEXT NOT NULL,
  key TEXT NOT NULL,
  at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS attempts_lookup ON attempts (kind, key, at);
