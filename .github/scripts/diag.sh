curl -s https://tidal-music.github.io/tidal-api-reference/tidal-api-oas.json -o oas.json
python3 - <<'PY'
import json
o=json.load(open("oas.json"))
for path,ops in o["paths"].items():
    if "search" not in path.lower(): continue
    for m,op in ops.items():
        if not isinstance(op,dict): continue
        print(m.upper(), path, "|", (op.get("description") or op.get("summary") or "")[:200])
        for p in op.get("parameters",[]):
            if "$ref" in p: p=o["components"]["parameters"][p["$ref"].split("/")[-1]]
            print("    ", p.get("name"), p.get("in"), (p.get("description") or "")[:160])
        rb=op.get("requestBody")
        if rb: print("    BODY", json.dumps(rb)[:400])
sc=o["components"]["schemas"]
for k in sc:
    if k.lower().startswith("searchresults") and ("create" in k.lower() or "body" in k.lower() or "payload" in k.lower()):
        print(k, json.dumps(sc[k])[:600])
PY
# run 5
