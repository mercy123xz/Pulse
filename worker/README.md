# Questions and problem reports → GitHub issues

Two kinds of messages come through this Worker:

- **From the website's Ask box:** questions become public issues on the Pulse repo (so the
  answers help everyone); problem reports go to the private `pulse-reports` repo.
- **From the apps** (`POST /report`): crash reports and *Report a problem* from the Windows
  and Android apps, only when the person presses Send. They go to the private
  `pulse-reports` repo, labelled `crash` or `problem` and `windows` or `android`. User
  folder names and email addresses are blanked out (by the app, and again here).

For the reports repo, add one more secret: `REPORTS_TOKEN`, a fine-grained token with
*Only select repositories* → pulse-reports, *Issues: Read and write*.

The "Ask about Pulse" box on the website answers from its FAQ. When it can't, visitors
can send a question or problem report without a GitHub account. It goes to this
Cloudflare Worker, which checks it and opens an issue on the repo.

What it checks before opening an issue:

- **Turnstile**, Cloudflare's free anti-spam check (no puzzles for most people)
- that it came from the website (`https://pulse.is-a.software`, or the old `https://mercy123xz.github.io`)
- length (5 to 3000 characters) and a hidden field that only bots fill in
- an optional per-visitor rate limit (see below)

`@names` and `#numbers` in messages are defused, so nobody gets pinged and no other
issue gets linked.

## Setup (once)

1. **GitHub token.** <https://github.com/settings/personal-access-tokens/new>:
   fine-grained, *Only select repositories* → Pulse, *Permissions* → Issues: Read and
   write. Nothing else. Copy it.
2. **Turnstile.** Cloudflare dashboard → Turnstile → Add widget. Hostname
   `pulse.is-a.software` and `mercy123xz.github.io`, mode *Managed*. You get a **site key** (public, goes in
   `site/ask.js`) and a **secret key** (goes in the Worker).
3. **Worker.** Workers & Pages → Create → Worker → name it `pulse-questions` → Deploy →
   Edit code → replace everything with `worker.js` → Deploy.
4. **Secrets.** The Worker → Settings → Variables and Secrets → add two of type
   *Secret*: `GITHUB_TOKEN` and `TURNSTILE_SECRET`.
5. **Site.** Put the Worker's address (`https://pulse-questions.<you>.workers.dev`) and
   the Turnstile site key at the top of `site/ask.js` (`ENDPOINT`, `TURNSTILE_SITE_KEY`).

When the token expires, make a new one the same way and replace `GITHUB_TOKEN`.

### Optional: rate limit

With Wrangler, add to `wrangler.toml`:

```toml
[[ratelimits]]
name = "RATE_LIMITER"
namespace_id = "1001"
simple = { limit = 5, period = 60 }
```

The Worker uses it when it's there: at most 5 messages a minute per visitor.
