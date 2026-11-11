// Pages for public Pulse playlists (p.html?id=…) and profiles (u.html?name=…).
// Everything shown comes from the accounts server and is written as text, never as HTML.
var PulseShare = (function () {
  // The accounts server (server/ in the repo). Empty until it's set up.
  var ACCOUNTS = "";

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  // Pictures only from the music services Pulse uses (the server checks this too).
  function safePic(url) {
    try {
      var u = new URL(url);
      return u.protocol === "https:" && /(^|\.)(dzcdn\.net|ytimg\.com|scdn\.co|mzstatic\.com|sndcdn\.com)$/.test(u.hostname) ? url : null;
    } catch (e) {
      return null;
    }
  }
  function pic(url, cls) {
    var ok = safePic(url);
    if (!ok) return el("div", cls ? cls + " ph" : "ph");
    var i = el("img", cls);
    i.src = ok;
    i.alt = "";
    i.loading = "lazy";
    i.referrerPolicy = "no-referrer";
    return i;
  }
  function time(s) {
    s = s | 0;
    return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0");
  }
  function show(nodes) {
    var p = document.getElementById("page");
    p.textContent = "";
    nodes.forEach(function (n) { p.appendChild(n); });
  }
  function problem(title, body) {
    show([el("h1", null, title), el("p", null, body)]);
    document.title = title + " · Pulse";
  }
  function load(path, done) {
    if (!ACCOUNTS) return problem("Not available yet", "Public playlists are coming to Pulse soon.");
    fetch(ACCOUNTS + path)
      .then(function (r) { return r.json().then(function (j) { return { ok: r.ok, j: j }; }); })
      .then(function (x) { x.ok ? done(x.j) : problem("Not found", x.j.error || "This link doesn't work anymore."); })
      .catch(function () { problem("Couldn't load this", "Check your connection and try again."); });
  }
  function param(name) {
    return new URLSearchParams(location.search).get(name) || "";
  }
  function copyButton() {
    var b = document.getElementById("copy");
    b.addEventListener("click", function () {
      (navigator.clipboard ? navigator.clipboard.writeText(location.href) : Promise.reject()).then(
        function () { b.textContent = "Copied"; },
        function () { b.textContent = "Copy the address bar"; }
      );
    });
  }

  function playlist() {
    copyButton();
    var id = param("id");
    if (!/^[A-Za-z0-9]{10}$/.test(id)) return problem("Not found", "This link isn't right.");
    load("/playlists/" + id, function (p) {
      document.title = p.name + " · Pulse";
      var top = el("div", "top");
      top.appendChild(pic(p.cover, "cover"));
      var info = el("div");
      info.appendChild(el("p", "kind", "Public playlist"));
      info.appendChild(el("h1", null, p.name));
      var by = el("p");
      var who = el("a", null, "@" + p.username);
      who.href = "u.html?name=" + encodeURIComponent(p.username);
      by.appendChild(who);
      by.appendChild(document.createTextNode(" · " + p.tracks.length + (p.tracks.length === 1 ? " song" : " songs")));
      info.appendChild(by);
      if (p.description) info.appendChild(el("p", null, p.description));
      top.appendChild(info);
      var list = el("ol");
      p.tracks.forEach(function (t, i) {
        var li = el("li");
        li.appendChild(el("span", "n", String(i + 1)));
        li.appendChild(pic(t.cover_small || t.cover_big));
        var tt = el("div", "t");
        tt.appendChild(el("b", null, t.title));
        var artists = [t.artist && t.artist.name].concat((t.featured || []).map(function (a) { return a.name; })).filter(Boolean);
        tt.appendChild(el("span", null, artists.join(", ")));
        li.appendChild(tt);
        li.appendChild(el("span", "d", t.duration ? time(t.duration) : ""));
        list.appendChild(li);
      });
      show([top, list]);
    });
  }

  function profile() {
    copyButton();
    var name = param("name").toLowerCase();
    if (!/^[a-z0-9_.]{1,20}$/.test(name)) return problem("Not found", "This link isn't right.");
    load("/users/" + name, function (p) {
      var u = p.user;
      document.title = "@" + u.username + " · Pulse";
      var top = el("div", "top");
      top.appendChild(el("div", "avatar", u.username.charAt(0).toUpperCase()));
      var info = el("div");
      info.appendChild(el("p", "kind", "Profile"));
      info.appendChild(el("h1", null, "@" + u.username));
      if (u.bio) info.appendChild(el("p", null, u.bio));
      var joined = new Date(u.created * 1000).toLocaleDateString(undefined, { month: "short", year: "numeric" });
      info.appendChild(el("p", null, p.playlists.length + (p.playlists.length === 1 ? " public playlist" : " public playlists") + " · Joined " + joined));
      top.appendChild(info);
      var nodes = [top];
      if (p.playlists.length) {
        var cards = el("div", "cards");
        p.playlists.forEach(function (l) {
          var a = el("a", "card");
          a.href = "p.html?id=" + encodeURIComponent(l.id);
          a.appendChild(pic(l.cover));
          a.appendChild(el("b", null, l.name));
          a.appendChild(el("span", null, l.count + (l.count === 1 ? " song" : " songs")));
          cards.appendChild(a);
        });
        nodes.push(cards);
      }
      show(nodes);
    });
  }

  return { playlist: playlist, profile: profile, _setServer: function (u) { ACCOUNTS = u; } };
})();
