// Pulse questions and problem reports → GitHub issues.
//
// Two ways in:
// - The "Ask about Pulse" box on the website (POST /). Checked with Turnstile anti-spam,
//   size, a hidden honeypot field and a per-visitor limit. Questions become public issues
//   on the Pulse repo, so answers help everyone; problem reports go to the private
//   pulse-reports repo.
// - The Pulse apps (POST /report): crash reports and "Report a problem", only ever sent
//   when the person presses Send. They go to the private pulse-reports repo, because a
//   crash report says things about someone's PC. Personal bits (user folder names,
//   email addresses) are blanked out here too, in case an app missed one.
// The GitHub keys never reach the website or the apps: they're secrets of this Worker.
//
// Secrets (Cloudflare dashboard → the Worker → Settings → Variables and Secrets):
//   GITHUB_TOKEN      fine-grained token, only the Pulse repo, Issues: read and write
//   REPORTS_TOKEN     fine-grained token, only pulse-reports, Issues: read and write
//   TURNSTILE_SECRET  the Turnstile widget's secret key
// Optional binding: RATE_LIMITER (Rate Limiting), see worker/README.md.

const REPO = "mercy123xz/Pulse";
const REPORTS = "mercy123xz/pulse-reports";
const MAX_DETAILS = 20000;
// The website moved to listen2pulse.github.io (Nov 2026); the old addresses still forward there.
const ALLOWED_ORIGINS = ["https://listen2pulse.github.io", "https://pulse.is-a.software", "https://mercy123xz.github.io"];
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

    const ip = request.headers.get("CF-Connecting-IP") || "unknown";
    if (env.RATE_LIMITER) {
      const { success } = await env.RATE_LIMITER.limit({ key: ip });
      if (!success) return reply(429, { error: "That's a lot of messages. Please try again in a minute." });
    }

    // From the apps: no website, so no Origin (and no Turnstile: there's no browser).
    if (new URL(request.url).pathname.replace(/\/+$/, "") === "/report") {
      if (origin) return reply(403, { error: "Not allowed from a website." });
      return appReport(request, env, reply);
    }
    if (!ALLOWED_ORIGINS.includes(origin)) return reply(403, { error: "Not allowed from here." });

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
    const hostOk = !check.hostname || ALLOWED_ORIGINS.some((o) => o === `https://${check.hostname}`);
    if (!check.success || !hostOk) return reply(403, { error: "The anti-spam check didn't pass. Please try again." });

    const title = `${kind === "bug" ? "[Problem]" : "[Question]"} ${quiet(oneLine(message).slice(0, 100))}`;
    const issueBody = [
      quiet(message),
      "",
      "---",
      `_Sent from the Pulse website${page ? ` (${quiet(page)})` : ""} without a GitHub account. The sender can't see replies unless they come back to this page, so answers here help everyone who asks later._`,
    ].join("\n");

    // Problems go to the private reports repo (when it's set up); questions stay public.
    const toReports = kind === "bug" && env.REPORTS_TOKEN;
    const res = toReports
      ? await openIssue(REPORTS, env.REPORTS_TOKEN, title, scrub(issueBody), ["problem", "website"])
      : await openIssue(REPO, env.GITHUB_TOKEN, title, issueBody, [kind === "bug" ? "bug" : "question", "from website"]);
    if (!res.ok) return reply(502, { error: "GitHub didn't take it right now. Please try again later." });
    const issue = await res.json();
    // The private repo's address means nothing to visitors: don't send it back.
    return reply(201, toReports ? { ok: true, private: true } : { ok: true, number: issue.number, url: issue.html_url });
  },
};

/** Opens an issue; without its labels if they can't be used. */
async function openIssue(repo, token, title, body, labels) {
  const create = (l) =>
    fetch(`https://api.github.com/repos/${repo}/issues`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "pulse-reports",
        "Content-Type": "application/json",
      },
      body: JSON.stringify(l ? { title, body, labels: l } : { title, body }),
    });
  let res = await create(labels);
  if (res.status === 422) res = await create(null);
  return res;
}

/** A crash report or "Report a problem" from the Windows or Android app. */
async function appReport(request, env, reply) {
  if (!env.REPORTS_TOKEN) return reply(503, { error: "Problem reports aren't set up yet." });
  let b;
  try {
    const text = await request.text();
    if (text.length > MAX_DETAILS + MAX_MESSAGE + 2000) return reply(413, { error: "That's too long." });
    b = JSON.parse(text);
  } catch {
    return reply(400, { error: "Couldn't read that." });
  }
  const str = (v, n) => (typeof v === "string" ? v.trim().slice(0, n) : "");
  const kind = b.kind === "crash" ? "crash" : "problem";
  const source = b.source === "android" ? "android" : "windows";
  const version = str(b.version, 40).replace(/[^\w.+-]/g, "");
  const message = str(b.message, MAX_MESSAGE);
  const details = str(b.details, MAX_DETAILS);
  if (kind === "problem" && message.length < 5) return reply(400, { error: "Please write a little more about what happened." });
  if (kind === "crash" && details.length < 20) return reply(400, { error: "The crash report is missing." });
  // Crashes are titled by what went wrong, so the same crash is easy to spot again.
  const what = (details.match(/^What: (.+)$/m) || [])[1] || "";
  const headline = kind === "crash" ? `Crash: ${what || "Pulse closed unexpectedly"}` : `Problem: ${message}`;
  const app = source === "android" ? "Android" : "Windows";
  const title = quiet(scrub(oneLine(headline)).slice(0, 110)) + ` (${app}${version ? " " + version : ""})`;
  const fence = "~~~~";
  const body = [
    message ? quiet(scrub(message)) : "_No message: sent from the crash prompt._",
    "",
    details ? `<details><summary>Technical details</summary>\n\n${fence}text\n${scrub(details).replaceAll(fence, "~ ~ ~ ~")}\n${fence}\n</details>` : "",
    "",
    "---",
    `_From the Pulse ${app} app${version ? ` ${version}` : ""}, sent by its user._`,
  ].join("\n");
  const res = await openIssue(REPORTS, env.REPORTS_TOKEN, title, body, [kind, source]);
  if (!res.ok) return reply(502, { error: "The report couldn't be sent right now. Please try again later." });
  return reply(201, { ok: true });
}

/** Blanks out personal bits: user folder names and email addresses. */
function scrub(s) {
  return s
    .replace(/([A-Za-z]:[\\/](?:Users|Documents and Settings)[\\/])[^\\/"'<>\r\n]+/gi, "$1<user>")
    .replace(/(\/(?:home|Users)\/)[^/\s"'<>]+/g, "$1<user>")
    .replace(/[\w.+-]+@[\w-]+\.[\w.-]+/g, "<email>");
}

/** One line, for the title. */
function oneLine(s) {
  return s.replace(/\s+/g, " ").trim();
}

/** Keeps a message from pinging people (@name) or linking other issues (#12). */
function quiet(s) {
  return s.replace(/@/g, "@​").replace(/#(\d)/g, "#​$1");
}

export { oneLine, quiet, scrub };
