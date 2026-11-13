"""Announces a new Pulse release: in the Discord server's #announcements (pinging
@everyone) and on X (@ListenToPulse, through Typefully).

Run by the Windows build right after it publishes a new version. Uses two repository
secrets, and skips whichever is missing:
- DISCORD_ANNOUNCE_WEBHOOK: a webhook made in #announcements → Integrations
- TYPEFULLY_API_KEY: an API key from Typefully's settings

The text comes from this version's part of .github/release-notes.md. The X post is the
`<!-- x: … -->` line in that part (hidden on GitHub), so write one with every release.
`<!-- x: skip -->` announces the release on Discord only; `<!-- x: quiet -->` nowhere.

`python announce.py --check` only checks the Typefully key (it makes a draft and deletes
it): nothing is posted anywhere.
"""

import json
import os
import re
import sys
import urllib.request

RAW = "https://raw.githubusercontent.com/mercy123xz/Pulse/main/assets"
DOWNLOAD = "https://listen2pulse.github.io/download/"
X_ACCOUNT = 363390  # @ListenToPulse in Typefully
TYPEFULLY = "https://api.typefully.com/v2"


def section(notes: str) -> tuple[str, str]:
    """The newest "## Pulse x.y.z" section: (heading, body)."""
    m = re.search(r"^## (Pulse [^\n]+)\n(.*?)(?=^## Pulse |\Z)", notes, re.S | re.M)
    if not m:
        return "", ""
    return m.group(1).strip(), m.group(2).strip()


def x_summary(body: str) -> str:
    m = re.search(r"<!--\s*x:\s*(.*?)\s*-->", body, re.S)
    return " ".join(m.group(1).split()) if m else ""


def to_discord(md: str) -> str:
    """Release-notes Markdown as Discord shows it: bullets, no HTML comments."""
    md = re.sub(r"<!--.*?-->", "", md, flags=re.S)
    md = re.sub(r"^- ", "• ", md, flags=re.M)
    md = re.sub(r"\n{3,}", "\n\n", md)
    return md.strip()


def call(url: str, payload: dict | None, headers: dict, method: str = "POST") -> dict:
    data = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json", **headers}, method=method)
    with urllib.request.urlopen(req, timeout=30) as r:
        text = r.read().decode() or "{}"
        return json.loads(text) if text.strip().startswith("{") else {}


def discord(version: str, page: str, body: str) -> None:
    hook = os.environ.get("DISCORD_ANNOUNCE_WEBHOOK", "").strip()
    if not hook:
        print("No DISCORD_ANNOUNCE_WEBHOOK secret: not announcing on Discord.")
        return
    text = to_discord(body)
    if len(text) > 3500:
        text = text[:3500].rsplit("\n", 1)[0] + f"\n\n…and more in the [full notes]({page})."
    text += f"\n\n⬇️ **[Download Pulse]({DOWNLOAD})**  ·  Already have it? Pulse will offer the update when you open it."
    call(
        hook + ("&" if "?" in hook else "?") + "wait=true",
        {
            "username": "Pulse",
            "avatar_url": f"{RAW}/discord-avatar.png",
            "content": "@everyone",
            "allowed_mentions": {"parse": ["everyone"]},
            "embeds": [{"title": f"Pulse {version} is out 🎉", "url": page, "description": text, "color": 0x8B5CF6, "image": {"url": f"{RAW}/discord-banner-small.png"}}],
        },
        {"User-Agent": "release-bot"},
    )
    print("Announced on Discord.")


def tweet_text(version: str, body: str) -> str:
    summary = x_summary(body) or "Fixes and improvements."
    head = f"Pulse {version} is out 🎉"
    tail = f"\n\nGet it: {DOWNLOAD}"
    # X counts a link as 23 characters.
    room = 280 - len(head) - 2 - len("\n\nGet it: ") - 23
    if len(summary) > room:
        summary = summary[: room - 1].rsplit(" ", 1)[0] + "…"
    return f"{head}\n\n{summary}{tail}"


def soon() -> str:
    import datetime

    at = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(minutes=3)
    return at.replace(microsecond=0).isoformat()


def x(version: str, body: str) -> None:
    key = os.environ.get("TYPEFULLY_API_KEY", "").strip()
    if not key:
        print("No TYPEFULLY_API_KEY secret: not posting on X.")
        return
    if x_summary(body).lower() == "skip":
        print("This release is Discord only: not posting on X.")
        return
    text = tweet_text(version, body)
    call(
        f"{TYPEFULLY}/social-sets/{X_ACCOUNT}/drafts",
        # Scheduled a few minutes ahead: X doesn't let posts with a link be published
        # "now" through an API, but a scheduled one goes out normally.
        {"draft_title": f"Pulse {version} release", "publish_at": soon(), "platforms": {"x": {"enabled": True, "posts": [{"text": text}]}}},
        {"Authorization": f"Bearer {key}"},
    )
    print("Scheduled on X (in 3 minutes):\n" + text)


def check() -> int:
    """Checks the secrets without posting: a Typefully draft made and deleted."""
    print("Discord webhook secret:", "set" if os.environ.get("DISCORD_ANNOUNCE_WEBHOOK", "").strip() else "MISSING")
    key = os.environ.get("TYPEFULLY_API_KEY", "").strip()
    print("Typefully key secret:", "set" if key else "MISSING")
    _, body = section(open(".github/release-notes.md", encoding="utf-8").read())
    print("The X post would be:\n" + tweet_text("x.y.z", body))
    if not key:
        return 1
    auth = {"Authorization": f"Bearer {key}"}
    d = call(f"{TYPEFULLY}/social-sets/{X_ACCOUNT}/drafts", {"draft_title": "Pulse key check (deleted at once)", "platforms": {"x": {"enabled": True, "posts": [{"text": "key check"}]}}}, auth)
    draft = d.get("id") or d.get("draft_id")
    print("Typefully key works: made draft", draft)
    if draft:
        call(f"{TYPEFULLY}/social-sets/{X_ACCOUNT}/drafts/{draft}", None, auth, method="DELETE")
        print("…and deleted it.")
    return 0


def main() -> int:
    if sys.argv[1:] == ["--check"]:
        return check()
    version, tag = sys.argv[1], sys.argv[2]
    page = f"https://github.com/mercy123xz/Pulse/releases/tag/{tag}"
    _, body = section(open(".github/release-notes.md", encoding="utf-8").read())
    if x_summary(body).lower() == "quiet":
        print("A quiet release: not announcing it anywhere.")
        return 0
    # One failing shouldn't stop the other, or the release.
    for name, step in (("Discord", lambda: discord(version, page, body)), ("X", lambda: x(version, body))):
        try:
            step()
        except Exception as e:
            print(f"::warning::Couldn't announce on {name}: {e}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
