# Pulse accounts server

Accounts, public playlists and profiles for Pulse. It's a Cloudflare Worker with a D1
database, on Cloudflare's free plan. Password-reset codes are emailed from your Gmail through
[EmailJS](https://www.emailjs.com) (free), or through Brevo or Resend.

## Upgrading a server set up before emails (do this before releasing the app that needs it)

1. **Add the new column and table.** Open the `pulse-accounts` database's **Console**, paste all
   of [`add-email.sql`](add-email.sql), and press **Execute**. Run it once only: a second time
   says the `email` column already exists, which is harmless.
2. **Update the code.** Open the Worker, press **Edit code**, replace everything with
   [`worker.js`](worker.js), and press **Deploy**.
3. **Set up email** as in step 7 below.

Existing accounts keep working. Pulse asks each one to add an email the next time it opens.

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
7. **Set up email** (for "Forgot password?"). Codes are sent from your own Gmail through
   [EmailJS](https://www.emailjs.com) (free for 200 emails a month; no domain or postal address):
   1. Make a free EmailJS account. Under **Email Services → Add New Service**, choose **Gmail**,
      connect the Gmail the codes should come from, and press **Create Service**. Note its
      **Service ID**.
   2. Under **Email Templates → Create New Template**, set:
      - **Subject:** `{{code}} is your Pulse code`
      - **Content:** `Hi @{{username}}, your Pulse code is {{code}}. Enter it in Pulse to choose a
        new password. It works for {{minutes}} minutes. If you didn't ask for this, ignore this
        email: your password hasn't changed.`
      - **To Email** (on the right): `{{to_email}}`. **From Name:** `Pulse`.

      Save it and note its **Template ID**.
   3. Under **Account → General**, note your **Public Key** and **Private Key**. Under
      **Account → Security**, turn on **Allow EmailJS API for non-browser applications**
      (the server isn't a browser).
   4. In Cloudflare, open the Worker's **Settings → Variables and Secrets** and add:
      - Text `EMAILJS_SERVICE_ID`, `EMAILJS_TEMPLATE_ID` and `EMAILJS_PUBLIC_KEY`
      - Secret `EMAILJS_PRIVATE_KEY`

      Press **Deploy**.
   5. Try it: in Pulse, press **Forgot password?** when signing in and enter your username.

   Until this is done, "Forgot password?" says it isn't set up yet; everything else works.
   Instead of EmailJS you can use [Brevo](https://www.brevo.com) (`BREVO_API_KEY` and
   `MAIL_FROM` = `Pulse <address>`; it asks for a postal address) or
   [Resend](https://resend.com) (`RESEND_API_KEY` and `MAIL_FROM`; it needs your own domain).

Then put that address in `ACCOUNTS_URL` in `src/account.rs` and `ACCOUNTS` in `site/share.js`.

## What it stores

| Table | What's in it |
|---|---|
| `users` | Username, email address (never shown to anyone, only used for reset codes), a salted PBKDF2 hash of the password (never the password), bio, when you joined |
| `sessions` | A SHA-256 hash of each sign-in key (never the key), and when it expires (90 days) |
| `playlists` | Public playlists: name, description, songs (title, artist, album, ids, cover links), dates |
| `reports` | Reports of public playlists (a hash of who reported, never their address) |
| `resets` | A password-reset code waiting to be used: only its hash, when it expires (15 minutes) and wrong tries |
| `attempts` | Recent sign-ups, failed sign-ins, reset requests and reports, used for the limits below; cleared after a day |

Deleting an account deletes its sessions, reset code, playlists and the reports about them.

## Built-in protections

- Passwords: at least 8 characters, can't contain the username, hashed with PBKDF2-SHA256
  (100,000 rounds, a random salt per account).
- Sign-in: 10 wrong passwords per username and address, then a 15-minute wait. The same
  message is shown for a wrong username and a wrong password.
- Sign-up: 5 new accounts per address per hour. Some names are reserved (admin, pulse, support…).
  An email address can only be on one account.
- Password resets: a 6-digit code, stored only as a hash, that works for 15 minutes and 5 tries.
  At most 3 codes per account and 5 requests per address an hour. The reply is the same whether
  or not the account exists, so it can't be used to find out who has one. A reset signs you out
  everywhere.
- Changing your email needs your password, so someone with only your session can't send your
  reset codes to themselves.
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
`node server/serve.mjs` runs the server locally on port 8787; it doesn't send emails but prints
them, and shows the last one at `/__mail`.

## Song fixes

`song_fixes` lists songs the catalog gets wrong. `GET /fixes?ids=dz:1,dz:2` returns the fixes for
those songs (read-only; nothing in the app can change the list). Add one in Cloudflare (D1 →
pulse-accounts → Console):

```sql
-- A song Deezer lists as a 30-second clip: its real length in seconds.
INSERT OR REPLACE INTO song_fixes (id, duration, video, note) VALUES ('dz:3892384211', 227, NULL, 'Firework: Deezer lists a clip');
-- A song that keeps playing the wrong video: the YouTube video id to play instead.
INSERT OR REPLACE INTO song_fixes (id, duration, video, note) VALUES ('dz:123', NULL, 'dQw4w9WgXcQ', 'wrong video');
```

A song's id is its Deezer number with `dz:` in front (deezer.com/track/3892384211 → `dz:3892384211`). The live match test (`Match test` in Actions)
prints the rows for every clip-only song it finds.
