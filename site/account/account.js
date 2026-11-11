// Your Pulse account on the website: sign in, forgot password, and the account page
// (profile, email, password, public playlists, sign out, delete). New accounts are made
// in the app. Everything shown is written as text, never as HTML.
//
// Signed in, the key is kept in this browser (localStorage "pulse-session"), and sent to
// the accounts server in a header. Signing out deletes it there and here.
(function () {
  "use strict";
  var ACCOUNTS = "https://pulse-accounts.y0hyotuube.workers.dev";
  try { ACCOUNTS = localStorage.getItem("pulse-dev-accounts") || ACCOUNTS; } catch (e) {} // local testing only
  var KEY = "pulse-session";
  var page = document.getElementById("page");

  // ------------------------------------------------------------------ helpers
  function el(tag, attrs, kids) {
    var e = document.createElement(tag);
    for (var k in attrs || {}) {
      if (k === "text") e.textContent = attrs[k];
      else if (k === "class") e.className = attrs[k];
      else if (k.slice(0, 2) === "on") e.addEventListener(k.slice(2), attrs[k]);
      else e.setAttribute(k, attrs[k]);
    }
    (kids || []).forEach(function (c) { if (c) e.appendChild(typeof c === "string" ? document.createTextNode(c) : c); });
    return e;
  }
  function show(nodes, focus) {
    page.textContent = "";
    nodes.forEach(function (n) { if (n) page.appendChild(n); });
    var f = focus && page.querySelector(focus);
    if (f) f.focus();
  }
  function session() {
    try { return JSON.parse(localStorage.getItem(KEY) || "null"); } catch (e) { return null; }
  }
  function setSession(s) {
    try {
      if (s) {
        localStorage.setItem(KEY, JSON.stringify(s));
        localStorage.setItem("pulse-user", s.username); // "Welcome back, @you" on the home page
      } else localStorage.removeItem(KEY);
    } catch (e) {}
  }
  function api(method, path, body) {
    var s = session();
    var headers = { "Content-Type": "application/json" };
    if (s) headers.Authorization = "Bearer " + s.token;
    return fetch(ACCOUNTS + path, { method: method, headers: headers, body: body ? JSON.stringify(body) : undefined })
      .then(function (r) {
        return r.json().catch(function () { return {}; }).then(function (j) {
          if (r.ok) return j;
          if (r.status === 401 && j.error === "Please sign in again.") { setSession(null); signIn("You were signed out. Please sign in again."); }
          throw new Error(j.error || "Something went wrong (" + r.status + ").");
        });
      }, function () { throw new Error("Couldn't reach Pulse. Check your connection and try again."); });
  }
  function field(label, id, type, extra) {
    var input = el("input", Object.assign({ id: id, name: id, type: type || "text" }, extra || {}));
    return [el("label", { for: id, text: label }), input];
  }
  function msg(kind, text) { return el("p", { class: "msg " + kind, role: kind === "bad" ? "alert" : "status", text: text }); }
  // A form whose button shows it's busy, and shows the server's answer underneath.
  function form(children, button, run) {
    var out = el("div");
    var btn = el("button", { class: "main", type: "submit", text: button });
    var f = el("form", { novalidate: "" }, children.concat([el("div", { class: "row" }, [btn]), out]));
    f.addEventListener("submit", function (e) {
      e.preventDefault();
      out.textContent = "";
      btn.disabled = true;
      Promise.resolve()
        .then(function () { return run(f); })
        .then(function (ok) { if (ok) out.appendChild(msg("good", ok)); })
        .catch(function (err) { out.appendChild(msg("bad", err.message)); })
        .then(function () { btn.disabled = false; });
    });
    return f;
  }
  var v = function (f, id) { return f.querySelector("#" + id).value; };
  function need(cond, text) { if (!cond) throw new Error(text); }

  // ------------------------------------------------------------------ signed out
  function signIn(note) {
    document.title = "Sign in · Pulse";
    show([
      el("h1", { text: "Sign in to Pulse" }),
      el("p", { text: "Manage your profile, email, password and public playlists." }),
      note ? msg("info", note) : null,
      el("div", { class: "card" }, [
        form(field("Username or email", "who", "text", { autocomplete: "username" })
          .concat(field("Password", "pass", "password", { autocomplete: "current-password" })), "Sign in", function (f) {
          var who = v(f, "who").trim(), pass = v(f, "pass");
          need(who && pass, "Enter your username or email, and your password.");
          return api("POST", "/login", { username: who, password: pass }).then(signedIn);
        }),
        el("div", { class: "row" }, [el("button", { class: "link", type: "button", text: "Forgot password?", onclick: function () { forgot(); } })]),
      ]),
      el("p", { style: "margin-top:1.2rem" }, ["No account yet? Create one in the Pulse app: Settings → Account. ", el("a", { href: "../download/", text: "Get Pulse" })]),
    ], "#who");
  }
  function signedIn(j) {
    setSession({ token: j.token, username: j.user.username });
    home();
  }

  function forgot(who) {
    document.title = "Forgot password · Pulse";
    show([
      el("h1", { text: "Forgot your password?" }),
      el("p", { text: "Enter your username or email. We'll email you a 6-digit code to choose a new password." }),
      el("div", { class: "card" }, [
        form(field("Username or email", "who", "text", { autocomplete: "username", value: who || "" }), "Send code", function (f) {
          var w = v(f, "who").trim();
          need(w, "Enter your username or email address.");
          return api("POST", "/reset/start", { who: w }).then(function (j) { reset(w, j.message); });
        }),
        el("div", { class: "row" }, [el("button", { class: "link", type: "button", text: "Back to sign in", onclick: function () { signIn(); } })]),
      ]),
    ], "#who");
  }

  function reset(who, note) {
    document.title = "Choose a new password · Pulse";
    show([
      el("h1", { text: "Choose a new password" }),
      el("p", { text: "Enter the code from the email and a new password. Not in your inbox? Check your spam folder. You'll be signed out everywhere else." }),
      note ? msg("info", note) : null,
      el("div", { class: "card" }, [
        form(field("Code", "code", "text", { inputmode: "numeric", autocomplete: "one-time-code", maxlength: "7" })
          .concat(field("New password", "p1", "password", { autocomplete: "new-password" }))
          .concat(field("New password again", "p2", "password", { autocomplete: "new-password" })), "Set password", function (f) {
          var code = v(f, "code").replace(/\s/g, ""), p1 = v(f, "p1");
          need(/^\d{6}$/.test(code), "Enter the 6-digit code from the email.");
          need(p1.length >= 8, "Passwords need at least 8 characters.");
          need(p1 === v(f, "p2"), "The two passwords don't match.");
          return api("POST", "/reset/finish", { who: who, code: code, password: p1 }).then(signedIn);
        }),
        el("div", { class: "row" }, [el("button", { class: "link", type: "button", text: "Send a new code", onclick: function () { forgot(who); } })]),
      ]),
    ], "#code");
  }

  // ------------------------------------------------------------------ signed in
  function home(note) {
    show([el("p", { style: "margin-top:2rem", text: "Loading your account…" })]);
    api("GET", "/me").then(function (j) { account(j, note); }, function (err) { if (session()) show([msg("bad", err.message), retry()]); });
  }
  function retry() { return el("div", { class: "row" }, [el("button", { type: "button", text: "Try again", onclick: function () { home(); } })]); }

  function account(j, note) {
    var u = j.user;
    document.title = "@" + u.username + " · Pulse";
    var joined = new Date(u.created * 1000).toLocaleDateString(undefined, { month: "long", year: "numeric" });
    var lists = j.playlists || [];

    var listCard = el("div", { class: "card" }, [
      el("h2", { text: "Your public playlists" }),
      lists.length
        ? el("ul", { class: "lists" }, lists.map(function (l) {
            var cover = safePic(l.cover) ? el("img", { src: l.cover, alt: "", loading: "lazy", referrerpolicy: "no-referrer" }) : el("div", { class: "ph" });
            var priv = el("button", { type: "button", text: "Make private" });
            priv.addEventListener("click", function () {
              if (!confirm("Make “" + l.name + "” private? Its link will stop working. It stays in your library in the Pulse app.")) return;
              priv.disabled = true;
              api("DELETE", "/playlists/" + encodeURIComponent(l.id)).then(function () { home("“" + l.name + "” is private now."); }, function (e) { priv.disabled = false; alert(e.message); });
            });
            return el("li", null, [cover, el("div", { class: "t" }, [el("b", { text: l.name }), el("span", { text: l.count + (l.count === 1 ? " song" : " songs") })]),
              el("a", { class: "b", href: "../p.html?id=" + encodeURIComponent(l.id), text: "View" }), priv]);
          }))
        : el("p", { text: "None yet. In the Pulse app, open one of your playlists and press “Make public”." }),
    ]);

    show([
      note ? msg("good", note) : null,
      el("div", { class: "who" }, [el("div", { class: "avatar", "aria-hidden": "true", text: u.username.charAt(0).toUpperCase() }), el("div", null, [
        el("h1", { text: "@" + u.username }),
        el("p", { text: "Joined " + joined }),
      ])]),
      el("div", { class: "row" }, [el("a", { class: "b", href: "../u.html?name=" + encodeURIComponent(u.username), text: "View your profile" }), el("button", { type: "button", text: "Sign out", onclick: signOut })]),

      el("div", { class: "card" }, [
        el("h2", { text: "About you" }),
        el("p", { text: "Shown on your public profile." }),
        form([el("label", { for: "bio", class: "sr", text: "About you" }), (function () { var t = el("textarea", { id: "bio", maxlength: "300" }); t.value = u.bio || ""; return t; })()], "Save", function (f) {
          return api("PUT", "/me", { bio: v(f, "bio").trim() }).then(function () { return "Saved."; });
        }),
      ]),

      el("div", { class: "card" }, [
        el("h2", { text: "Email" }),
        u.email ? el("p", null, ["Reset codes go to ", el("b", { text: u.email }), ". It's never shown to anyone."]) : msg("info", "Your account has no email yet. Add one so you can reset your password."),
        el("details", null, [el("summary", { text: u.email ? "Change email" : "Add email" }),
          form(field("New email", "email", "email", { autocomplete: "email" }).concat(field("Your password", "epass", "password", { autocomplete: "current-password" })), "Save email", function (f) {
            var e = v(f, "email").trim();
            need(/^[^\s@]+@[^\s@]+\.[^\s@.]{2,}$/.test(e), "That email address doesn't look right.");
            need(v(f, "epass"), "Enter your password.");
            return api("PUT", "/me/email", { email: e, password: v(f, "epass") }).then(function () { home("Email saved."); });
          })]),
      ]),

      el("div", { class: "card" }, [
        el("h2", { text: "Password" }),
        el("details", null, [el("summary", { text: "Change password" }),
          form(field("Current password", "old", "password", { autocomplete: "current-password" })
            .concat(field("New password", "new1", "password", { autocomplete: "new-password" }))
            .concat(field("New password again", "new2", "password", { autocomplete: "new-password" })), "Change password", function (f) {
            need(v(f, "new1").length >= 8, "Passwords need at least 8 characters.");
            need(v(f, "new1") === v(f, "new2"), "The two passwords don't match.");
            return api("POST", "/me/password", { old: v(f, "old"), new: v(f, "new1") }).then(function () { return "Password changed. You're signed out everywhere else."; });
          })]),
      ]),

      listCard,

      el("div", { class: "card" }, [
        el("h2", { text: "Delete your account" }),
        el("p", { text: "Deletes your account, your public profile and every playlist you made public, right away. Your library in the Pulse app stays. This can't be undone." }),
        el("details", null, [el("summary", { text: "Delete account" }),
          form(field("Your password", "dpass", "password", { autocomplete: "current-password" }), "Delete my account", function (f) {
            need(v(f, "dpass"), "Enter your password.");
            if (!confirm("Delete @" + u.username + " for good?")) return;
            return api("DELETE", "/me", { password: v(f, "dpass") }).then(function () {
              setSession(null);
              try { localStorage.removeItem("pulse-user"); } catch (e) {}
              signIn("Your account was deleted.");
            });
          })]),
      ]),
    ]);
    [].forEach.call(page.querySelectorAll("button.main"), function (b) { if (b.textContent === "Delete my account") b.classList.add("danger"); });
  }

  function signOut() {
    api("POST", "/logout").catch(function () {}).then(function () {
      setSession(null);
      signIn("You're signed out.");
    });
  }

  function safePic(url) {
    try {
      var u = new URL(url);
      return u.protocol === "https:" && /(^|\.)(dzcdn\.net|ytimg\.com|scdn\.co|mzstatic\.com|sndcdn\.com)$/.test(u.hostname);
    } catch (e) { return false; }
  }

  if (session()) home(); else signIn();
})();
