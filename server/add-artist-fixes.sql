-- Run once in Cloudflare (D1 → pulse-accounts → Console) to add the artist fixes list.
CREATE TABLE IF NOT EXISTS artist_fixes (artist TEXT NOT NULL, item TEXT NOT NULL, action TEXT NOT NULL, note TEXT, PRIMARY KEY (artist, item));
