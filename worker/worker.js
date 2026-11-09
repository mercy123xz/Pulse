// Pulse website → GitHub issues.
//
// The "Ask about Pulse" box on the website sends questions and problem reports here.
// This Worker checks them (Turnstile anti-spam, size, a hidden honeypot field, a
// per-visitor limit) and opens an issue on the repo, so visitors need no GitHub
// account. The GitHub key never reaches the website: it's a secret of this Worker.
//
// Secrets (Cloudflare dashboard → the Worker → Settings → Variables and Secrets):
//   GITHUB_TOKEN      fine-grained token, only this repo, permission Issues: read and write
//   TURNSTILE_SECRET  the Turnstile widget's secret key
// Optional binding: RATE_LIMITER (Rate Limiting), see worker/README.md.

const REPO = "mercy123xz/Pulse";
const ALLOWED_ORIGINS = ["https://mercy123xz.github.io"];
const MAX_MESSAGE = 3000;

export default {
  async fetch(request, env) {
    const origin = request.headers.get("Origin") || "";
    const cors = {
      "Access-Control-Allow-Origin": ALLOWED_ORIGINS.includes(origin) ? origin : ALLOWED_ORIGINS[0],
      "Access-Control-Allow-Methods": "POST, OPTIONS",
      "Access-Control-Allow-Headers": "Content-Type",
      "Access-Control-Max-Age": "86400",
      Vary: "Origin",
    };
    const reply = (status, data) => new Response(JSON.stringify(data), { status, headers: { ...cors, "Content-Type": "application/json" } });

    if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: cors });
    if (request.method !== "POST") return reply(405, { error: "Only POST." });
    if (!ALLOWED_ORIGINS.includes(origin)) return reply(403, { error: "Not allowed from here." });

    const ip = request.headers.get("CF-Connecting-IP") || "unknown";
    if (env.RATE_LIMITER) {
      const { success } = await env.RATE_LIMITER.limit({ key: ip });
      if (!success) return reply(429, { error: "That's a lot of messages. Please try again in a minute." });
    }

    let body;
    try {
      const text = await request.text();
      if (text.length > 20000) return reply(413, { error: "That's too long." });
      body = JSON.parse(text);
    } catch {
      return reply(400, { error: "Couldn't read that." });
    }

    const kind = body.kind === "bug" ? "bug" : "question";
    const message = typeof body.message === "string" ? body.message.trim() : "";
    const page = typeof body.page === "string" ? body.page.slice(0, 100) : "";
    // Bots fill in every field, including the hidden one people never see.
    if (body.website) return reply(200, { ok: true });
    if (message.length < 5) return reply(400, { error: "Please write a little more." });
    if (message.length > MAX_MESSAGE) return reply(400, { error: `Please keep it under ${MAX_MESSAGE} characters.` });

    // Turnstile: proves a person sent it from the website.
    const form = new FormData();
    form.append("secret", env.TURNSTILE_SECRET || "");
    form.append("response", typeof body.token === "string" ? body.token : "");
    form.append("remoteip", ip);
    const check = await fetch("https://challenges.cloudflare.com/turnstile/v0/siteverify", { method: "POST", body: form })
      .then((r) => r.json())
      .catch(() => ({ success: false }));
    const hostOk = !check.hostname || check.hostname === "mercy123xz.github.io";
    if (!check.success || !hostOk) return reply(403, { error: "The anti-spam check didn't pass. Please try again." });

    const title = `${kind === "bug" ? "[Problem]" : "[Question]"} ${quiet(oneLine(message).slice(0, 100))}`;
    const issueBody = [
      quiet(message),
      "",
      "---",
      `_Sent from the Pulse website${page ? ` (${quiet(page)})` : ""} without a GitHub account. The sender can't see replies unless they come back to this page, so answers here help everyone who asks later._`,
    ].join("\n");

    const create = (labels) =>
      fetch(`https://api.github.com/repos/${REPO}/issues`, {
        method: "POST",
        headers: {
          Authorization: `Bearer ${env.GITHUB_TOKEN}`,
          Accept: "application/vnd.github+json",
          "X-GitHub-Api-Version": "2022-11-28",
          "User-Agent": "pulse-website-questions",
          "Content-Type": "application/json",
        },
        body: JSON.stringify(labels ? { title, body: issueBody, labels } : { title, body: issueBody }),
      });
    let res = await create([kind === "bug" ? "bug" : "question", "from website"]);
    // If labels can't be used, still open the issue.
    if (res.status === 422) res = await create(null);
    if (!res.ok) return reply(502, { error: "GitHub didn't take it right now. Please try again later." });
    const issue = await res.json();
    return reply(201, { ok: true, number: issue.number, url: issue.html_url });
  },
};

/** One line, for the title. */
function oneLine(s) {
  return s.replace(/\s+/g, " ").trim();
}

/** Keeps a message from pinging people (@name) or linking other issues (#12). */
function quiet(s) {
  return s.replace(/@/g, "@​").replace(/#(\d)/g, "#​$1");
}

export { oneLine, quiet };
