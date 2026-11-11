// A tiny stand-in for Cloudflare D1 on top of Node's built-in SQLite.
import { DatabaseSync } from "node:sqlite";
import { readFileSync } from "node:fs";
export function makeD1(path = ":memory:") {
  const db = new DatabaseSync(path);
  db.exec(readFileSync(new URL("./schema.sql", import.meta.url), "utf8"));
  const stmt = (sql, args = []) => ({
    bind: (...a) => stmt(sql, a),
    first: async () => db.prepare(sql).get(...args) ?? null,
    all: async () => ({ results: db.prepare(sql).all(...args) }),
    run: async () => { const r = db.prepare(sql).run(...args); return { meta: { last_row_id: Number(r.lastInsertRowid), changes: r.changes } }; },
  });
  return { prepare: (sql) => stmt(sql), batch: async (list) => { db.exec("BEGIN"); try { const out = []; for (const s of list) out.push(await s.run()); db.exec("COMMIT"); return out; } catch (e) { db.exec("ROLLBACK"); throw e; } }, raw: db };
}
