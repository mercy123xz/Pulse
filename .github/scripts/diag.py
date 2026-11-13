import json, subprocess, urllib.request, urllib.parse
def get(u):
    return json.load(urllib.request.urlopen(u, timeout=30))
print("=== Deezer Che ===")
s = get("https://api.deezer.com/search/artist?q=" + urllib.parse.quote("Che"))
for a in s["data"][:6]:
    print(a["id"], a["name"], a.get("nb_fan"), a.get("nb_album"))
aid = s["data"][0]["id"]
top = get(f"https://api.deezer.com/artist/{aid}/top?limit=10")
print("top:", [(t["title"], t["album"]["id"]) for t in top["data"]])
al = get(f"https://api.deezer.com/artist/{aid}/albums?limit=100")
for a in al["data"]:
    print(a["id"], a["title"], a.get("release_date"), "genre", a.get("genre_id"), "fans", a.get("fans"), a.get("record_type"), "E" if a.get("explicit_lyrics") else "")
for a in al["data"][:40]:
    d = get(f"https://api.deezer.com/album/{a['id']}")
    print("ALBUM", a["title"], "| label:", d.get("label"), "| contributors:", [(c["id"], c["name"]) for c in d.get("contributors", [])], "| artist", d["artist"]["id"])
print("=== Deezer Geeked up ===")
r = get("https://api.deezer.com/search?q=" + urllib.parse.quote('artist:"ApolloRed1" track:"Geeked up"'))
for t in r["data"][:6]:
    print(t["id"], t["title"], t["duration"], "explicit", t.get("explicit_lyrics"), t.get("explicit_content_lyrics"), t["album"]["title"])
print("=== yt-dlp YTM flat ===")
out = subprocess.run(["yt-dlp", "--flat-playlist", "-J", "--playlist-end", "6", "https://music.youtube.com/search?q=" + urllib.parse.quote("ApolloRed1 - Geeked up") + "#songs"], capture_output=True, text=True).stdout
for e in json.loads(out)["entries"]:
    print(json.dumps(e)[:900])
print("=== innertube ===")
body = {"context": {"client": {"clientName": "WEB_REMIX", "clientVersion": "1.20241106.01.00", "hl": "en"}}, "query": "ApolloRed1 Geeked up", "params": "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D"}
req = urllib.request.Request("https://music.youtube.com/youtubei/v1/search?prettyPrint=false", data=json.dumps(body).encode(), headers={"Content-Type": "application/json", "Origin": "https://music.youtube.com"})
d = json.load(urllib.request.urlopen(req, timeout=30))
def walk(o):
    if isinstance(o, dict):
        if "musicResponsiveListItemRenderer" in o:
            yield o["musicResponsiveListItemRenderer"]
        for v in o.values(): yield from walk(v)
    elif isinstance(o, list):
        for v in o: yield from walk(v)
for r in walk(d):
    txt = json.dumps(r)
    vid = r.get("playlistItemData", {}).get("videoId")
    cols = [" ".join(x["text"] for x in c["musicResponsiveListItemFlexColumnRenderer"]["text"].get("runs", [])) for c in r.get("flexColumns", [])]
    print(vid, cols, "EXPLICIT" if "MUSIC_EXPLICIT_BADGE" in txt else "-")
