import json, urllib.request
def get(u):
    return json.load(urllib.request.urlopen(u, timeout=30))
for aid in [394465971, 234702351, 68823812]:
    a = get(f"https://api.deezer.com/artist/{aid}")
    print("=== ARTIST", aid, a["name"], a.get("nb_fan"), a.get("nb_album"))
    top = get(f"https://api.deezer.com/artist/{aid}/top?limit=10")
    print("top:", [(t["title"], t["album"]["title"], t["rank"]) for t in top["data"]])
    al = get(f"https://api.deezer.com/artist/{aid}/albums?limit=100")
    for x in al["data"]:
        d = get(f"https://api.deezer.com/album/{x['id']}")
        print(x["release_date"], "|", x["title"], "| genre", x.get("genre_id"), "| fans", x.get("fans"), "| label:", d.get("label"), "| upc", d.get("upc"), "| others:", [c["name"] for c in d.get("contributors", []) if c["id"] != aid])
