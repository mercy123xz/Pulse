-- Run once in Cloudflare (D1 → pulse-accounts → Console) BEFORE deploying the worker that
-- reads it: early supporters get a badge, the Founders theme and their name in Pulse.
ALTER TABLE users ADD COLUMN supporter INTEGER NOT NULL DEFAULT 0;
