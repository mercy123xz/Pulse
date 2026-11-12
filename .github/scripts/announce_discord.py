"""Posts a new Pulse release to the Discord server's #announcements, pinging @everyone.

Run by the Windows build right after it publishes a new version. Needs the repository
secret DISCORD_ANNOUNCE_WEBHOOK (a webhook made in #announcements → Integrations); without
it this does nothing. The text is this version's part of .github/release-notes.md.
"""

import json
import os
import re
import sys
import urllib.request

RAW = "https://raw.githubusercontent.com/mercy123xz/Pulse/main/assets"


def section(notes: str) -> tuple[str, str]:
    """The newest "## Pulse x.y.z" section: (heading, body)."""
    m = re.search(r"^## (Pulse [^\n]+)\n(.*?)(?=^## Pulse |\Z)", notes, re.S | re.M)
    if not m:
        return "", ""
    return m.group(1).strip(), m.group(2).strip()


def to_discord(md: str) -> str:
    """Release-notes Markdown as Discord shows it: bullets, no HTML comments."""
    md = re.sub(r"<!--.*?-->", "", md, flags=re.S)
    md = re.sub(r"^- ", "• ", md, flags=re.M)
    md = re.sub(r"\n{3,}", "\n\n", md)
    return md.strip()


def main() -> int:
    hook = os.environ.get("DISCORD_ANNOUNCE_WEBHOOK", "").strip()
    if not hook:
        print("No DISCORD_ANNOUNCE_WEBHOOK secret: not announcing.")
        return 0
    version, tag = sys.argv[1], sys.argv[2]
    page = f"https://github.com/mercy123xz/Pulse/releases/tag/{tag}"
    heading, body = section(open(".github/release-notes.md", encoding="utf-8").read())
    text = to_discord(body)
    if len(text) > 3500:
        text = text[:3500].rsplit("\n", 1)[0] + f"\n\n…and more in the [full notes]({page})."
    text += "\n\n⬇️ **[Download Pulse](https://mercy123xz.github.io/Pulse/download/)**  ·  Already have it? Pulse will offer the update when you open it."
    payload = {
        "username": "Pulse",
        "avatar_url": f"{RAW}/discord-avatar.png",
        "content": "@everyone",
        "allowed_mentions": {"parse": ["everyone"]},
        "embeds": [
            {
                "title": f"Pulse {version} is out 🎉",
                "url": page,
                "description": text,
                "color": 0x8B5CF6,
                "image": {"url": f"{RAW}/discord-banner-small.png"},
            }
        ],
    }
    req = urllib.request.Request(
        hook + ("&" if "?" in hook else "?") + "wait=true",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "Pulse release announcer"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            print(f"Announced {heading or version} on Discord ({r.status}).")
    except Exception as e:  # an announcement failing shouldn't fail the release
        print(f"::warning::Couldn't announce on Discord: {e}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
