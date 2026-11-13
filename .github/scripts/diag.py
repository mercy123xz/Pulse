import json, urllib.request, urllib.parse
def get(u):
    return json.load(urllib.request.urlopen(u, timeout=30))
seen = set()
for q in ["COCONUDE", "John Dillinger Che", "Por Siempre Che", "RichPhønks"]:
    r = get("https://api.deezer.com/search/album?q=" + urllib.parse.quote(q))
    for x in r["data"][:5]:
        print(q, "->", x["title"], "| artist", x["artist"]["id"], x["artist"]["name"])
        seen.add(x["artist"]["id"])
for aid in seen:
    a = get(f"https://api.deezer.com/artist/{aid}")
    if a["name"].lower() != "che": continue
    print("=== ARTIST", aid, a["name"], a.get("nb_fan"), a.get("nb_album"))
    top = get(f"https://api.deezer.com/artist/{aid}/top?limit=10")
    print("top:", [(t["title"], t["album"]["title"], t["rank"]) for t in top["data"]])
    al = get(f"https://api.deezer.com/artist/{aid}/albums?limit=100")
    for x in al["data"]:
        d = get(f"https://api.deezer.com/album/{x['id']}")
        tr = d.get("tracks", {}).get("data", [])
        print(x["release_date"], "|", x["title"], "| genre", x.get("genre_id"), "| fans", x.get("fans"), "| E" if x.get("explicit_lyrics") else "| -", "| label:", d.get("label"), "| others:", [c["name"] for c in d.get("contributors", []) if c["id"] != aid], "| rank", max([t["rank"] for t in tr] or [0]))
