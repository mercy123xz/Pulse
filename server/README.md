# Pulse accounts server

Accounts, public playlists and profiles for Pulse. It's a Cloudflare Worker with a D1
database, on Cloudflare's free plan. It needs no secrets.

## Set it up (about 10 minutes)

1. **Make the database.** In the Cloudflare dashboard, go to **Storage & databases → D1 SQL
   database → Create**. Name it `pulse-accounts`.
2. **Make its tables.** Open the database, go to the **Console** tab, paste all of
   [`schema.sql`](schema.sql) (one statement per line, no comments: the console can't read
   comments), and press **Execute**. The **Tables** tab should then list `users`,
   `sessions`, `playlists`, `reports` and `attempts`.
3. **Make the Worker.** Go to **Compute (Workers) → Create → Start with Hello World**. Name it
   `pulse-accounts` and press **Deploy**.
4. **Put the code in.** Press **Edit code**, replace everything with
   [`worker.js`](worker.js), and press **Deploy**.
5. **Connect the database.** Go to the Worker's **Bindings → Add binding → D1 database**.
   Set **Variable name** to `DB`, choose `pulse-accounts`, and press **Add binding**.
6. **Check it.** Open the Worker's address (`https://pulse-accounts.<you>.workers.dev/`). It
   should say `{"ok":true,"service":"pulse-accounts"}`.

Then put that address in `ACCOUNTS_URL` in `src/account.rs` and `ACCOUNTS` in `site/share.js`.

## What it stores

| Table | What's in it |
|---|---|
| `users` | Username, a salted PBKDF2 hash of the password (never the password), bio, when you joined |
| `sessions` | A SHA-256 hash of each sign-in key (never the key), and when it expires (90 days) |
| `playlists` | Public playlists: name, description, songs (title, artist, album, ids, cover links), dates |
| `reports` | Reports of public playlists (a hash of who reported, never their address) |
| `attempts` | Recent sign-ups, failed sign-ins and reports, used for the limits below; cleared after a day |

Deleting an account deletes its sessions, playlists and the reports about them.

## Built-in protections

- Passwords: at least 8 characters, can't contain the username, hashed with PBKDF2-SHA256
  (100,000 rounds, a random salt per account).
- Sign-in: 10 wrong passwords per username and address, then a 15-minute wait. The same
  message is shown for a wrong username and a wrong password.
- Sign-up: 5 new accounts per address per hour. Some names are reserved (admin, pulse, support…).
- Only the app can make changes. A website can read public playlists and profiles, but can't
  sign in or publish for anyone.
- Published songs keep only what Pulse needs. Songs from your computer (which contain file
  paths) are dropped. Pictures are only kept from the music services Pulse uses.
- A playlist that 3 different people report is hidden.

## Looking after it

In the database's **Console**:

```sql
-- What has been reported
SELECT r.playlist_id, p.name, u.username, COUNT(*) AS reports, GROUP_CONCAT(r.reason, ' | ')
FROM reports r JOIN playlists p ON p.id = r.playlist_id JOIN users u ON u.id = p.user_id
GROUP BY r.playlist_id ORDER BY reports DESC;

-- Show a hidden playlist again (it was fine)
UPDATE playlists SET hidden = 0 WHERE id = 'XXXXXXXXXX';
DELETE FROM reports WHERE playlist_id = 'XXXXXXXXXX';

-- Remove an account and everything it published
DELETE FROM playlists WHERE user_id = (SELECT id FROM users WHERE username = 'name');
DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE username = 'name');
DELETE FROM users WHERE username = 'name';
```

## Testing

`node server/test.mjs` runs every endpoint against a local SQLite database (Node 22.13+).
