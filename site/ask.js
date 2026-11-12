/* "Ask about Pulse": a small help box on every page.
 *
 * Questions are answered from the FAQ below, right in the browser: nothing typed is
 * sent anywhere while searching. When the FAQ has no answer, the visitor can send the
 * question (or a problem report) to the GitHub repo, with no account needed: it goes
 * to a small Cloudflare Worker (worker/ in the repo) that checks a Turnstile anti-spam
 * token and opens the issue. Until that's set up, a ready-filled GitHub link is used.
 */
(function () {
  "use strict";

  // The Worker's address and the Turnstile site key (both public). Empty: not set up yet.
  var ENDPOINT = "https://pulse-questions.y0hyotuube.workers.dev/";
  var TURNSTILE_SITE_KEY = "0x4AAAAAAF6OITIxlZXDQjk2";
  var REPO = "https://github.com/mercy123xz/Pulse";
  var root = document.currentScript && document.currentScript.src ? new URL(".", document.currentScript.src).href : "./";

  // ------------------------------------------------------------------ FAQ
  // keys: words that point at an answer (more specific words weigh more).
  var FAQ = [
    { q: "Is Pulse free?", keys: "free cost price pay paid money subscription premium ads advert", a: "Yes. Pulse is free and open source, with no ads and no subscription. A Pulse account is free and optional. It's a non-commercial side project for personal listening." },
    { q: "Is Pulse safe? Windows says it protected my PC.", keys: "safe virus malware trust smartscreen protected warning windows defender blocked unknown publisher run anyway", a: "Windows shows <b>“Windows protected your PC”</b> for any new app that isn't signed with a paid certificate. Click <b>More info</b>, then <b>Run anyway</b>; you'll only see it once. Every installer is built automatically from the public code on GitHub, so anyone can check what's in it." },
    { q: "Does Pulse work on Mac, Linux, Chromebook or phones?", keys: "mac macos apple linux chromebook chromeos android iphone ios phone mobile tablet ipad", a: "Not right now. Pulse is made for <b>Windows 10 and 11</b> (64-bit)." },
    { q: "Do I need an account?", keys: "account sign login log register email password signup", a: "No. Everything works without one, and your likes and playlists are saved on your own PC. A free <b>Pulse account</b> is optional: it lets you make playlists public and have a profile people can find. Make one in <b>Settings → Account</b>." },
    { q: "Where does the music come from?", keys: "music where come source catalog deezer youtube legal spotify songs info", a: "Song info, covers, charts and artist pages come from Deezer's free catalog. The audio itself is streamed from YouTube with yt-dlp. Pulse isn't affiliated with Deezer, YouTube or Spotify." },
    { q: "Why does the first song take a while?", keys: "first slow loading long wait setup tools download 70 mb start", a: "The first time you press play, Pulse downloads its two free playback tools, yt-dlp and Deno (about 70 MB, once). The player bar shows the progress. After that, songs start in a few seconds." },
    { q: "A song won't play.", keys: "song play wont won't doesn't fail error skip skipping broken stuck unavailable", a: "YouTube may not have a full version of that song, or YouTube changed something and yt-dlp hasn't caught up yet. Pulse updates yt-dlp every day, so trying again later often works. Pulse skips to the next song after a failure." },
    { q: "The window flickers or stays blank.", keys: "flicker flickering blank black white screen window graphics gpu glitch crash draw", a: "Open <b>Settings → Compatibility</b>, turn off <b>Enable hardware acceleration</b> and restart Pulse. It then draws with Windows' built-in software renderer." },
    { q: "I forgot my password.", keys: "forgot password reset lost recover code email sign signin login can't cant locked", a: "In Pulse's sign-in window, press <b>Forgot password?</b>, enter your username or email, and Pulse emails you a 6-digit code. Enter it with a new password. The code works for 15 minutes; check your spam folder if it doesn't arrive." },
    { q: "Pulse says my library file was damaged.", keys: "damaged corrupt corrupted library empty lost missing likes playlists gone disappeared restore backup recover", a: "That happens if your PC lost power or Pulse closed in the middle of saving. Pulse fixes it by itself: it opens its spare copy (nothing lost) or the newest daily backup. To pick another backup, go to <b>Settings → Your data</b>." },
    { q: "Does Pulse show lyrics?", keys: "lyrics words lyric karaoke sing synced text", a: "Yes. Press <b>L</b> or the microphone button in the player bar for synced lyrics that follow the song. They come from LRCLIB, a free lyrics library, so a few songs may not have any." },
    { q: "Can Discord show what I'm listening to?", keys: "discord status rich presence profile listening activity legcord vesktop arrpc not working", a: "Yes. Turn on <b>Discord status</b> in Settings. Your Discord profile then shows the song, its cover and a progress bar. Pulse only talks to the Discord app on your PC. Using <b>Legcord</b> or <b>Vesktop</b>? Turn on <b>Rich Presence (arRPC)</b> in its settings. The Discord website can't show it." },
    { q: "What is Gaming Mode?", keys: "gaming mode game fps performance lag slow ultra low resource cpu ram light", a: "Gaming Mode (the <b>Gaming</b> button in the top bar, or Ctrl + G) switches off extras like animations and background lookups while you play games. Ultra-low mode in Settings goes further. The music plays the same either way." },
    { q: "Where are my likes and playlists saved?", keys: "saved save where data folder appdata playlists likes library backup files location", a: "On your PC, in <code>%APPDATA%\\\\pulse-player</code>: likes, playlists, history, profile, settings and themes. Pulse also keeps a spare copy and a backup each day (the seven newest), so a damaged file can be recovered. Only playlists you make public go online, if you use an account." },
    { q: "How do I update Pulse?", keys: "update upgrade new version latest newer install again", a: "Pulse checks for updates once a day and offers to install the new version for you. It only runs an installer whose signature checks out. You can also download the newest installer from this site. Your likes, playlists and settings are kept." },
    { q: "How do I uninstall Pulse?", keys: "uninstall remove delete get rid", a: "Uninstall it from Windows Settings → Apps like any other app. Your library is kept in <code>%APPDATA%\\pulse-player</code> in case you reinstall; delete that folder to remove everything." },
    { q: "Can I listen offline?", keys: "offline download downloads save songs internet without wifi airplane", a: "Yes. Right-click a song (or use a playlist's menu) and choose <b>Download</b>. Downloaded songs play without internet. <b>Settings → Storage</b> lets you choose where they're kept." },
    { q: "Can it play the music files on my PC?", keys: "local files mp3 flac wav my music folder computer own", a: "Yes. Turn on <b>Show Local Files</b> in Settings and Pulse plays the music in your Windows Music folder (MP3, M4A, FLAC, WAV and Ogg)." },
    { q: "Can I import my Spotify or YouTube playlists?", keys: "import spotify youtube soundcloud playlist transfer move bring link", a: "Yes. Use the import button next to <b>Playlists</b> in the sidebar and paste the link to a public Spotify, YouTube or SoundCloud playlist or album. No sign-in needed. YouTube playlists come in whole (up to 5,000 songs). A Spotify link brings the first 100 songs; for a longer one, open it in Spotify, press Ctrl+A then Ctrl+C, and paste." },
    { q: "Is there a Discord or somewhere to talk about Pulse?", keys: "discord server community chat talk help support mastodon social twitter x news contact people follow", a: "Yes! Join the <a href=\"https://discord.gg/9mXy7v9wZ6\">Pulse Discord</a> for help, feedback and showing off your themes, and follow <a href=\"https://x.com/ListenToPulse\">@ListenToPulse on X</a> or <a href=\"https://mastodon.social/@mercy123xz\">@mercy123xz on Mastodon</a> for news and releases." },
    { q: "Can I share a playlist?", keys: "share sharing send friend code playlist", a: "Yes, two ways. With a free Pulse account, open your playlist and press <b>Make public</b>: Pulse copies a link that opens in Pulse or on this website, and changes you make are sent up by themselves. Without an account, share it as a one-line code that holds the song list; your friend pastes it into Pulse." },
    { q: "Can I change how Pulse looks?", keys: "theme themes look color colour dark light mode custom skin spicetify appearance", a: "Pulse has five themes (Default, Blossom, Dreary Mono, Lunar and Aurora), each with dark and light versions. You can make your own in <b>Settings → Themes</b>; Spicetify color schemes work too. See the <a href=\"" + root + "docs/#themes\">theme guide</a>." },
    { q: "Is there an equalizer or crossfade?", keys: "equalizer eq bass treble crossfade gapless automix mono sound audio quality", a: "Yes: a six-band equalizer with presets, crossfade up to 10 seconds, gapless playback, automix and mono. They're in Settings." },
    { q: "What are the keyboard shortcuts?", keys: "keyboard shortcut shortcuts hotkey keys space ctrl", a: "Space plays or pauses, Ctrl + →/← skips, Ctrl + ↑/↓ changes volume, M mutes, Ctrl + K searches, L shows lyrics, Ctrl + M opens the mini player and Ctrl + G turns Gaming Mode on or off. The <a href=\"" + root + "docs/#keys\">full list is in the docs</a>." },
    { q: "Which languages does Pulse speak?", keys: "language languages spanish portuguese french german translate english", a: "English, Spanish, Portuguese, French and German. Change it in <b>Settings → Language</b>." },
    { q: "How big is Pulse?", keys: "size big small mb storage space ram memory light lightweight heavy", a: "The installer is under 10 MB and Pulse is a native app with no browser inside, so it stays light. The playback tools add about 70 MB the first time you play a song." },
    { q: "Can I build Pulse myself?", keys: "build source code compile rust github contribute developer open", a: "Yes. It's open source and written in Rust: clone the repo and run <code>cargo run --release</code>. The <a href=\"" + root + "docs/#build\">docs explain it</a>." }
  ];

  var STOP = " a an the is are am be to of and or in on at it my me i you your can do does how what why where when which with for from this that there pulse will would should could get any have has".split(" ");

  function words(text) {
    return String(text).toLowerCase().replace(/[’']/g, "").split(/[^a-z0-9]+/).filter(function (w) {
      return w && STOP.indexOf(w) < 0;
    }).map(function (w) {
      // A rough stem, so "playlists", "playing" and "played" meet "playlist" and "play".
      return w.length > 4 ? w.replace(/(ing|ed|es|s)$/, "") : w;
    });
  }
  FAQ.forEach(function (f) { f.k = words(f.keys + " " + f.q); });

  function search(text) {
    var asked = words(text);
    if (!asked.length) return [];
    return FAQ.map(function (f) {
      var score = 0;
      asked.forEach(function (w) {
        if (f.k.indexOf(w) >= 0) score += w.length > 3 ? 2 : 1;
        else if (w.length > 3 && f.k.some(function (k) { return k.length > 3 && (k.indexOf(w) === 0 || w.indexOf(k) === 0); })) score += 1;
      });
      return { f: f, score: score / Math.sqrt(asked.length) };
    }).filter(function (r) { return r.score >= 1.4; }).sort(function (a, b) { return b.score - a.score; }).slice(0, 3);
  }

  // ------------------------------------------------------------------ Look
  var css = [
    ".ask-fab{position:fixed;right:20px;bottom:20px;z-index:50;display:flex;align-items:center;gap:.5rem;padding:.75rem 1.1rem;border:0;border-radius:999px;background:#7c3aed;color:#fff;font:600 .95rem/1 inherit;font-family:inherit;box-shadow:0 10px 30px rgba(124,58,237,.45)}",
    ".ask-fab:hover{background:#6d28d9}.ask-fab:focus-visible,.ask button:focus-visible,.ask a:focus-visible,.ask textarea:focus-visible,.ask input:focus-visible{outline:3px solid #a78bfa;outline-offset:2px}",
    ".ask{margin:0;padding:0;position:fixed;right:20px;bottom:84px;z-index:50;width:min(380px,calc(100vw - 24px));max-height:min(600px,calc(100vh - 110px));display:flex;flex-direction:column;background:#16161b;color:#ededf1;border:1px solid #2c2c36;border-radius:16px;box-shadow:0 30px 80px rgba(0,0,0,.6);overflow:hidden;font-size:.95rem}",
    ".ask[hidden]{display:none}.ask-head{display:flex;align-items:center;gap:.6rem;padding:14px 16px;border-bottom:1px solid #2c2c36;background:#0f0f13}",
    ".ask-head b{flex:1}.ask-x{background:none;border:0;color:#a3a3b0;font-size:1.4rem;line-height:1;padding:4px 8px;border-radius:8px}.ask-x:hover{color:#fff;background:#1d1d24}",
    ".ask-body{flex:1;overflow:auto;padding:14px 16px;display:flex;flex-direction:column;gap:10px}",
    ".ask-msg{padding:10px 12px;border-radius:12px;background:#1d1d24;line-height:1.5}.ask-msg.me{align-self:flex-end;background:#7c3aed;color:#fff;max-width:85%}",
    ".ask-msg h4{margin:0 0 4px;font-size:.92rem;color:#c4b5fd}.ask-msg p{margin:0}.ask-msg code{background:#0b0b0e;padding:1px 5px;border-radius:5px;font-size:.85em}.ask-msg a{color:#c4b5fd}",
    ".ask-chips{display:flex;flex-wrap:wrap;gap:6px}.ask-chip{background:#0f0f13;color:#ededf1;border:1px solid #2c2c36;border-radius:999px;padding:6px 11px;font:inherit;font-size:.85rem}.ask-chip:hover{border-color:#a78bfa}",
    ".ask-more{color:#a3a3b0;font-size:.85rem}.ask-more button{background:none;border:0;color:#c4b5fd;font:inherit;text-decoration:underline;padding:0}",
    ".ask-foot{border-top:1px solid #2c2c36;padding:10px;display:flex;gap:8px;background:#0f0f13}",
    ".ask-foot input{flex:1;min-width:0;background:#16161b;color:#ededf1;border:1px solid #2c2c36;border-radius:10px;padding:10px 12px;font:inherit}",
    ".ask-send{background:#7c3aed;color:#fff;border:0;border-radius:10px;padding:0 14px;font:600 .9rem inherit;font-family:inherit}.ask-send:disabled{opacity:.5}",
    ".ask-form{display:flex;flex-direction:column;gap:8px}.ask-form label{font-size:.85rem;color:#a3a3b0}.ask-form textarea,.ask-form input{background:#0f0f13;color:#ededf1;border:1px solid #2c2c36;border-radius:10px;padding:9px 11px;font:inherit;resize:vertical}",
    ".ask-kind{display:flex;gap:6px}.ask-kind button{flex:1;background:#0f0f13;color:#ededf1;border:1px solid #2c2c36;border-radius:10px;padding:8px;font:inherit;font-size:.88rem}.ask-kind button[aria-pressed=true]{border-color:#a78bfa;background:#241a3d}",
    ".ask-row{display:flex;gap:8px;align-items:center;flex-wrap:wrap}.ask-row .ask-send{padding:9px 14px}.ask-note{font-size:.8rem;color:#80808e}",
    ".ask-status{margin:0;display:flex;gap:8px;align-items:flex-start;padding:9px 11px;border-radius:10px;font-size:.88rem;line-height:1.4}.ask-status[hidden]{display:none}.ask-status svg{flex:none;margin-top:1px}.ask-status.err{background:#3a1520;color:#ffb4c2;border:1px solid #6b2236}.ask-status a{color:inherit}",
    ".ask-done{display:flex;gap:10px;align-items:flex-start;color:#bbf7d0}.ask-done svg{flex:none;width:26px;height:26px;padding:4px;border-radius:50%;background:#14532d;color:#86efac}.ask-done b{color:#fff}.ask-done p{margin:2px 0 0;color:#d4d4dc}.ask-done a{color:#c4b5fd}",
    ".ask-wait{margin:0;font-size:.85rem;color:#c4b5fd}.ask-wait[hidden]{display:none}.ask-send:disabled{cursor:not-allowed}",
    ".ask-hp{position:absolute;left:-9999px;width:1px;height:1px;overflow:hidden}",
    "@media (max-width:520px){.ask{right:12px;bottom:76px}.ask-fab{right:12px;bottom:12px}}"
  ].join("");

  function el(tag, attrs, html) {
    var e = document.createElement(tag);
    for (var k in attrs || {}) e.setAttribute(k, attrs[k]);
    if (html != null) e.innerHTML = html;
    return e;
  }
  function esc(s) { return String(s).replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; }); }

  var style = el("style", {}, css);
  document.head.appendChild(style);

  var fab = el("button", { class: "ask-fab", type: "button", "aria-expanded": "false", "aria-controls": "ask-panel" },
    '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12Z"/></svg>Ask about Pulse');
  var panel = el("div", { class: "ask", id: "ask-panel", role: "dialog", "aria-label": "Ask about Pulse", hidden: "" });
  panel.innerHTML =
    '<div class="ask-head"><b>Ask about Pulse</b><button class="ask-x" type="button" aria-label="Close">×</button></div>' +
    '<div class="ask-body" aria-live="polite"></div>' +
    '<form class="ask-foot"><label class="ask-hp" for="ask-q">Your question</label><input id="ask-q" autocomplete="off" maxlength="300" placeholder="Type a question…"><button class="ask-send" type="submit">Ask</button></form>';
  document.body.appendChild(fab);
  document.body.appendChild(panel);

  var body = panel.querySelector(".ask-body");
  var input = panel.querySelector("#ask-q");

  function add(node) { body.appendChild(node); body.scrollTop = body.scrollHeight; return node; }
  function say(html, me) { return add(el("div", { class: "ask-msg" + (me ? " me" : "") }, html)); }

  function welcome() {
    say("<p>Hi! Ask anything about Pulse. Answers come from this site; nothing you type is sent anywhere unless you choose to send it to the team.</p>");
    var chips = add(el("div", { class: "ask-chips" }));
    ["Is Pulse safe?", "Does it work on Mac?", "A song won't play", "Where are my playlists saved?"].forEach(function (t) {
      var c = el("button", { class: "ask-chip", type: "button" }, esc(t));
      c.addEventListener("click", function () { ask(t); });
      chips.appendChild(c);
    });
  }

  function ask(text) {
    text = String(text).trim();
    if (!text) return;
    say(esc(text), true);
    var found = search(text);
    if (found.length) {
      var best = found[0].f;
      say("<h4>" + esc(best.q) + "</h4><p>" + best.a + "</p>");
      var more = add(el("div", { class: "ask-more" }));
      found.slice(1).forEach(function (r) {
        var b = el("button", { type: "button" }, esc(r.f.q));
        b.addEventListener("click", function () { say("<h4>" + esc(r.f.q) + "</h4><p>" + r.f.a + "</p>"); });
        more.appendChild(document.createTextNode(more.childNodes.length ? " · " : "Related: "));
        more.appendChild(b);
      });
      if (more.childNodes.length) more.appendChild(document.createTextNode(" · "));
      var send = el("button", { type: "button" }, "Not it? Send your question");
      send.addEventListener("click", function () { sendForm(text, "question"); });
      more.appendChild(send);
    } else {
      say("<p>I don't have an answer for that yet. Send it to the Pulse team and it'll be answered on GitHub.</p>");
      sendForm(text, /crash|bug|broken|error|doesn|won|not work|freez|stuck|fail/i.test(text) ? "bug" : "question");
    }
  }

  // ------------------------------------------------------------------ Sending
  var turnstileLoading = null;
  function loadTurnstile() {
    if (!turnstileLoading) {
      turnstileLoading = new Promise(function (ok, fail) {
        var s = el("script", { src: "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit", async: "" });
        s.onload = ok;
        s.onerror = fail;
        document.head.appendChild(s);
      });
    }
    return turnstileLoading;
  }

  function issueLink(kind, text, details) {
    var title = (kind === "bug" ? "[Problem] " : "[Question] ") + text.slice(0, 120);
    var b = (details || text) + "\n\n_Sent from the Pulse website._";
    return REPO + "/issues/new?title=" + encodeURIComponent(title) + "&body=" + encodeURIComponent(b);
  }

  function sendForm(text, kind) {
    var ready = !!(ENDPOINT && TURNSTILE_SITE_KEY);
    var box = add(el("div", { class: "ask-msg" }));
    if (!ready) {
      box.innerHTML = '<p>You can post it on GitHub: <a href="' + esc(issueLink(kind, text)) + '" target="_blank" rel="noopener">open a ready-filled ' + (kind === "bug" ? "problem report" : "question") + "</a> (needs a free GitHub account).</p>";
      return;
    }
    var id = "ask" + Date.now();
    box.innerHTML =
      '<form class="ask-form" novalidate>' +
      '<div class="ask-kind" role="group" aria-label="What is it?"><button type="button" data-k="question" aria-pressed="' + (kind === "question") + '">A question</button><button type="button" data-k="bug" aria-pressed="' + (kind === "bug") + '">A problem</button></div>' +
      '<label for="' + id + 't">Your message</label><textarea id="' + id + 't" rows="4" maxlength="3000" required></textarea>' +
      '<label class="ask-hp">Leave this empty<input name="website" tabindex="-1" autocomplete="off"></label>' +
      '<div class="ask-ts"></div>' +
      '<p class="ask-wait" aria-live="polite">Tick “Verify you are human” above, then press Send.</p>' +
      '<p class="ask-status" role="status" hidden></p>' +
      '<div class="ask-row"><button class="ask-send" type="submit" disabled>Send</button><span class="ask-note">Posted publicly on GitHub, without your name. Don’t include personal info.</span></div>' +
      "</form>";
    var form = box.querySelector("form");
    var area = form.querySelector("textarea");
    area.value = text;
    var kindNow = kind;
    form.querySelectorAll(".ask-kind button").forEach(function (b) {
      b.addEventListener("click", function () {
        kindNow = b.getAttribute("data-k");
        form.querySelectorAll(".ask-kind button").forEach(function (o) { o.setAttribute("aria-pressed", String(o === b)); });
        if (kindNow === "bug" && area.value === text) area.value = text + "\n\nWhat happened:\nWhat I expected:\nWindows version:";
      });
    });
    var token = "";
    var widget = null;
    var sendBtn = form.querySelector(".ask-send"), wait = form.querySelector(".ask-wait");
    function setToken(t, why) {
      token = t || "";
      sendBtn.disabled = !token;
      wait.hidden = !!token;
      if (!token && why) wait.textContent = why;
    }
    loadTurnstile().then(function () {
      widget = window.turnstile.render(form.querySelector(".ask-ts"), {
        sitekey: TURNSTILE_SITE_KEY, theme: "dark", size: "flexible",
        callback: function (t) { setToken(t); },
        "expired-callback": function () { setToken("", "The check expired. Tick the box again, then press Send."); },
        "error-callback": function () { setToken("", "The anti-spam check had a problem. Reload the page and try again."); }
      });
    }).catch(function () {
      wait.hidden = true;
      form.querySelector(".ask-ts").innerHTML = '<p class="ask-note">The anti-spam check couldn’t load. <a href="' + esc(issueLink(kindNow, text)) + '" target="_blank" rel="noopener">Post on GitHub instead</a>.</p>';
    });
    var status = form.querySelector(".ask-status");
    function say(kind, html) {
      status.className = "ask-status " + kind; status.innerHTML = html; status.hidden = !html;
      if (html) status.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
    var ICON_OK = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7"/></svg>';
    var ICON_ERR = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7.5v5.5M12 16.5v.01"/></svg>';
    form.addEventListener("submit", function (e) {
      e.preventDefault();
      var msg = area.value.trim();
      var send = form.querySelector(".ask-send");
      if (msg.length < 5) { say("err", ICON_ERR + "<span>Please write a little more.</span>"); area.focus(); return; }
      if (!token) { say("err", ICON_ERR + "<span>Tick “Verify you are human” first.</span>"); return; }
      say("", "");
      send.disabled = true;
      send.textContent = "Sending…";
      var controller = window.AbortController ? new AbortController() : null;
      var timer = setTimeout(function () { if (controller) controller.abort(); }, 15000);
      fetch(ENDPOINT, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind: kindNow, message: msg, page: location.pathname, website: form.website.value, token: token }),
        signal: controller ? controller.signal : undefined
      }).then(function (r) {
        return r.json().catch(function () { return {}; }).then(function (j) { if (!r.ok) throw new Error(j.error || "Couldn't send it."); return j; });
      }).then(function (j) {
        box.innerHTML = '<div class="ask-done">' + ICON_OK + "<div><b>Sent, thank you!</b><p>" + (j.url ? 'Follow the answer here: <a href="' + esc(j.url) + '" target="_blank" rel="noopener">' + esc("question #" + j.number) + "</a>" : "It’s on its way to the Pulse team.") + "</p></div></div>";
        box.scrollIntoView({ block: "nearest", behavior: "smooth" });
      }).catch(function (err) {
        send.disabled = false;
        send.textContent = "Send";
        if (window.turnstile && widget !== null) { window.turnstile.reset(widget); setToken("", "Tick the box again to retry."); }
        var why = err.name === "AbortError" ? "That took too long." : err.name === "TypeError" ? "Couldn’t reach the server. Check your connection." : err.message;
        say("err", ICON_ERR + "<span>" + esc(why) + ' <a href="' + esc(issueLink(kindNow, msg)) + '" target="_blank" rel="noopener">Post on GitHub instead</a></span>');
      }).then(function () { clearTimeout(timer); });
    });
    area.focus();
  }

  // ------------------------------------------------------------------ Open / close
  var started = false;
  function open() {
    panel.hidden = false;
    fab.setAttribute("aria-expanded", "true");
    if (!started) { started = true; welcome(); }
    input.focus();
  }
  function close() {
    panel.hidden = true;
    fab.setAttribute("aria-expanded", "false");
    fab.focus();
  }
  fab.addEventListener("click", function () { panel.hidden ? open() : close(); });
  panel.querySelector(".ask-x").addEventListener("click", close);
  panel.addEventListener("keydown", function (e) { if (e.key === "Escape") close(); });
  panel.querySelector(".ask-foot").addEventListener("submit", function (e) {
    e.preventDefault();
    ask(input.value);
    input.value = "";
  });
})();
