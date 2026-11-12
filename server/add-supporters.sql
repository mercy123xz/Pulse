-- Run once in Cloudflare (D1 → pulse-accounts → Console) BEFORE deploying the worker that
-- reads it: early supporters get a badge, the Founders theme and their name in Pulse.
-- New accounts become supporters on their own until there are 100. The accounts made
-- before this worker went live need the UPDATE below (the 100 oldest, or all of them).
ALTER TABLE users ADD COLUMN supporter INTEGER NOT NULL DEFAULT 0;
UPDATE users SET supporter = 1 WHERE id IN (SELECT id FROM users ORDER BY created LIMIT 100);
