for q in "hello" "ApolloRed1 Geeked up" "Che RichPhonks" "Drake Gods Plan"; do
  echo "=== $q"; curl -s -w "\nHTTP %{http_code}\n" "https://pulse-accounts.y0hyotuube.workers.dev/tidal/search?q=$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1]))' "$q")" | head -c 2500; echo
done
# run 2
# run 3
# run 4
curl -s https://tidal-music.github.io/tidal-api-reference/tidal-api-oas.json -o oas.json
python3 - <<'PY'
import json
o=json.load(open("oas.json"))
for path in ["/searchResults/{id}", "/searchResults/{id}/relationships/tracks", "/tracks"]:
    g=o["paths"].get(path,{}).get("get",{})
    print("PATH", path)
    for p in g.get("parameters",[]):
        if "$ref" in p: p=o["components"]["parameters"][p["$ref"].split("/")[-1]]
        print("  ", p.get("name"), p.get("in"), p.get("required"), json.dumps(p.get("schema"))[:150], p.get("style"), p.get("explode"), (p.get("description") or "")[:120])
sc=o["components"]["schemas"]
for k in ["Tracks_Attributes","SearchResults_Attributes"]:
    if k in sc: print(k, list(sc[k].get("properties",{}).keys()), sc[k].get("required"))
PY
