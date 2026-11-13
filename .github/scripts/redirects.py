"""Builds the old site address (mercy123xz.github.io/Pulse/) as forwarding pages: one per
page of site/, each sending visitors to the same page, query and #part on the new site."""
import os, sys, shutil

NEW = "https://listen2pulse.github.io/"
src, out = sys.argv[1], sys.argv[2]
shutil.rmtree(out, ignore_errors=True)
PAGE = """<!doctype html><html lang="en"><head><meta charset="utf-8">
<title>Pulse has moved</title>
<meta name="robots" content="noindex">
<link rel="canonical" href="{url}">
<script>location.replace("{url}" + location.search + location.hash);</script>
<meta http-equiv="refresh" content="1; url={url}">
</head><body style="background:#0b0b0e;color:#ddd;font-family:sans-serif;text-align:center;padding-top:20vh">
Pulse has moved to <a style="color:#a78bfa" href="{url}">{url}</a>
</body></html>
"""
count = 0
for root, _, files in os.walk(src):
    for f in files:
        if not f.endswith(".html"):
            continue
        rel = os.path.relpath(os.path.join(root, f), src).replace(os.sep, "/")
        path = rel[: -len("index.html")] if rel.endswith("index.html") else rel
        dest = os.path.join(out, rel)
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        open(dest, "w").write(PAGE.format(url=NEW + path))
        count += 1
# Images shared in old posts (Discord embeds, social cards) keep working too.
if os.path.isdir(os.path.join(src, "img")):
    shutil.copytree(os.path.join(src, "img"), os.path.join(out, "img"))
open(os.path.join(out, "404.html"), "w").write(PAGE.format(url=NEW))
print(f"{count} forwarding pages")
