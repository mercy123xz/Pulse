-- Run once in Cloudflare (D1 → pulse-accounts → Console) to add the song fixes list.
CREATE TABLE IF NOT EXISTS song_fixes (id TEXT PRIMARY KEY, duration INTEGER, video TEXT, note TEXT);
