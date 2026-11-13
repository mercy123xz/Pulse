-- Run once in Cloudflare (D1 → pulse-accounts → Console) BEFORE deploying the worker that
-- reads it: profile pictures other people can see.
ALTER TABLE users ADD COLUMN avatar_v INTEGER NOT NULL DEFAULT 0;
CREATE TABLE IF NOT EXISTS avatars (user_id INTEGER PRIMARY KEY, type TEXT NOT NULL, data TEXT NOT NULL, updated INTEGER NOT NULL);
