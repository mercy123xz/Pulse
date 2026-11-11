// Runs the accounts Worker on http://127.0.0.1:8787 with a local SQLite database, for
// trying Pulse against it (PULSE_DEBUG_ACCOUNTS=http://127.0.0.1:8787).
import { createServer } from "node:http";
import worker from "./worker.js";
import { makeD1 } from "./d1.mjs";
// Emails aren't sent: the last one is printed and can be read at /__mail.
let lastMail = null;
const env = { DB: makeD1(process.argv[2] || ":memory:"), DEV_ORIGIN: "http://localhost:8765", MAIL_HOOK: (m) => { lastMail = m; console.log(`mail to ${m.to}: ${m.subject}`); } };
createServer(async (req, res) => {
  if (req.url === "/__mail") { res.writeHead(200, { "Content-Type": "application/json" }); return res.end(JSON.stringify(lastMail)); }
  const chunks = [];
  for await (const c of req) chunks.push(c);
  const headers = new Headers();
  for (const [k, v] of Object.entries(req.headers)) if (typeof v === "string") headers.set(k, v);
  headers.set("CF-Connecting-IP", req.socket.remoteAddress || "127.0.0.1");
  const body = ["GET", "HEAD"].includes(req.method) ? undefined : Buffer.concat(chunks);
  const r = await worker.fetch(new Request("http://127.0.0.1:8787" + req.url, { method: req.method, headers, body }), env);
  res.writeHead(r.status, Object.fromEntries(r.headers));
  res.end(Buffer.from(await r.arrayBuffer()));
}).listen(8787, "127.0.0.1", () => console.log("accounts server on http://127.0.0.1:8787"));
