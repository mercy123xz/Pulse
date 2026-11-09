/* The themes section's live preview: a small working copy of Pulse, drawn in HTML to
 * match the real app (sidebar sections, top bar, Orbits, Charts, player bar). Pick a
 * theme and its dark or light version: the preview takes that theme's real colors and
 * layout from assets/themes. Inside it you can switch pages, search, open albums, like
 * songs and play them. The songs and artists are made up; nothing actually plays.
 */
(function () {
  "use strict";
  var host = document.querySelector(".js-demo");
  if (!host) return;

  // ------------------------------------------------------------------ Themes
  // Colors from each theme's color.ini (dark version first), corners from user.css,
  // layout from theme.toml.
  var C = ["bg", "side", "player", "card", "hl", "hl2", "bd", "tx", "sub", "faint", "btn", "act"];
  var THEMES = [
    { name: "Default", desc: "Dark violet, with a light version", r: 10, glow: true,
      modes: [["Dark", "0b0b0e 0f0f13 0f0f13 16161b 1d1d24 24242d 2c2c36 ededf1 9696a4 80808e 7c3aed a78bfa"],
              ["Light", "fafafc f3f3f7 f3f3f7 ffffff e8e8ee dedee6 d4d4de 14141a 4e4e5c 646472 7c3aed 6d28d9"]] },
    { name: "Blossom", desc: "Soft rose, round corners, big cover in the sidebar", r: 18, bigart: true,
      modes: [["Dusk", "1c1519 160f13 160f13 261c22 33262e 3f2f39 4a3742 fbeef4 d4b3c3 c2a1b2 b8336a ff9cc4"],
              ["Petal", "fff5f8 fdebf1 fdebf1 ffffff f8dbe6 f2cbda ecc4d3 2a1520 5e3447 6e4054 b02a62 9c1f53"]] },
    { name: "Dreary Mono", desc: "Greyscale, square corners, capital titles", r: 2, upper: true,
      modes: [["Mono", "141414 0e0e0e 0e0e0e 1c1c1c 262626 303030 3a3a3a e6e6e6 ababab 9a9a9a 5a5a5a d6d6d6"],
              ["Paper", "f2f2f0 e8e8e6 e8e8e6 fafaf8 dddddb d0d0ce c4c4c2 1a1a1a 424242 505050 3d3d3d 262626"]] },
    { name: "Lunar", desc: "Night blues, icon sidebar, floating panel", r: 14, icons: true, inset: true, glow: true,
      modes: [["Midnight", "0f1424 090d18 090d18 161c30 1f2740 29324f 313b5c e8ecff a9b2d6 97a1c8 4b5fd6 a3b3ff"],
              ["Eclipse", "101010 000000 000000 181818 242424 2e2e2e 383838 f0e9dc bdb29e a99e8a 94561a f5b45a"]] },
    { name: "Aurora", desc: "Northern-light greens that glow with the music", r: 22, inset: true, glow: true, left: true,
      modes: [["Night", "07100f 0a1514 0d1a19 102221 15302e 1b3b38 1f4541 e6f7f4 8fb8b2 6f9a94 0f766e 5eead4"],
              ["Dawn", "f4fbfa e9f6f4 e9f6f4 ffffff d9eeeb cde8e4 b9dbd6 0c201e 3c5f5a 52736e 115e59 0f766e"]] }
  ];

  // ------------------------------------------------------------------ Music (made up)
  var ALBUMS = [
    { id: "a1", title: "Neon Harbor", artist: "Lumen Avenue", c: ["#ff6a88", "#7b2ff7"] },
    { id: "a2", title: "Slow Static", artist: "The Paper Tides", c: ["#43cea2", "#185a9d"] },
    { id: "a3", title: "Gold Hour", artist: "Mira Sol", c: ["#f7b733", "#fc4a1a"] },
    { id: "a4", title: "Night Drive Club", artist: "VANTA", c: ["#4776e6", "#8e54e9"] },
    { id: "a5", title: "Glasshouse", artist: "Juniper Lane", c: ["#56ab2f", "#a8e063"] },
    { id: "a6", title: "Low Tide", artist: "Okami Sound", c: ["#2c3e50", "#4ca1af"] }
  ];
  var SONGS = [
    ["Afterglow", "a1", 214], ["City of Echoes", "a1", 187], ["Blue Signal", "a2", 236], ["Paper Planes Home", "a2", 201],
    ["Sunday Gold", "a3", 178], ["Honey & Rust", "a3", 243], ["Ninety Miles", "a4", 199], ["Tail Lights", "a4", 225],
    ["Greenhouse Heart", "a5", 192], ["Window Seat", "a5", 208], ["Undertow", "a6", 251], ["Moonlit Ferry", "a6", 196]
  ].map(function (s, i) {
    var a = ALBUMS.filter(function (x) { return x.id === s[1]; })[0];
    return { id: i, title: s[0], album: a, artist: a.artist, len: s[2], views: Math.round(2e6 + ((i * 7919) % 97) * 1.37e6) };
  });
  var GENRES = ["All genres", "Pop", "Rap/Hip Hop", "Rock", "Dance", "R&B"];

  // ------------------------------------------------------------------ Icons (Phosphor-style, like the app)
  var P = {
    home: "M4 10.5 12 4l8 6.5V20h-5.5v-5.5h-5V20H4z",
    homeF: "M3.5 10.4 12 3.5l8.5 6.9V21h-6v-6h-5v6h-6z",
    chart: "M4 20h16M7 16v-5m5 5V6m5 10v-8",
    clock: "M12 7.5V12l3 2M20.5 12a8.5 8.5 0 1 1-17 0 8.5 8.5 0 0 1 17 0",
    queue: "M4 6h12M4 11h12M4 16h7m4 0 5 3v-6z",
    heart: "M12 20s-7.5-4.6-7.5-10.2A4.3 4.3 0 0 1 12 7.2a4.3 4.3 0 0 1 7.5 2.6C19.5 15.4 12 20 12 20Z",
    search: "m20 20-4.4-4.4M17.2 10.6a6.6 6.6 0 1 1-13.2 0 6.6 6.6 0 0 1 13.2 0",
    back: "M15 5l-7 7 7 7", fwd: "M9 5l7 7-7 7",
    sun: "M12 16.5a4.5 4.5 0 1 0 0-9 4.5 4.5 0 0 0 0 9M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4",
    gear: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6M19.4 13.5l1.6 1.2-2 3.4-1.9-.7a7.6 7.6 0 0 1-2 1.2L14.8 21h-4l-.3-2.4a7.6 7.6 0 0 1-2-1.2l-1.9.7-2-3.4 1.6-1.2a7 7 0 0 1 0-2.4L4.6 10l2-3.4 1.9.7a7.6 7.6 0 0 1 2-1.2L10.8 3h4l.3 2.4a7.6 7.6 0 0 1 2 1.2l1.9-.7 2 3.4-1.6 1.2a7 7 0 0 1 0 2.4",
    user: "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8m-7.5 8a7.5 7.5 0 0 1 15 0",
    plus: "M12 5v14M5 12h14", list: "M4 7h10M4 12h10M4 17h6m9-9v9m-3-3 3 3 3-3",
    play: "M8 5.2v13.6L19 12z", pause: "M7.5 5h3.2v14H7.5zm5.8 0h3.2v14h-3.2z",
    next: "M6.5 5.5v13L16 12zM18 5v14", prev: "M17.5 5.5v13L8 12zM6 5v14",
    shuffle: "M3.5 7.5h3c4.5 0 6.5 9 11 9h3M3.5 16.5h3c1.6 0 2.8-1.2 3.8-2.8m3.4-5.4c1-1.6 2.2-2.8 3.8-2.8h3m-2.2-2.2 2.2 2.2-2.2 2.2m0 6.6 2.2 2.2-2.2 2.2",
    repeat: "M4.5 11V9.5a3 3 0 0 1 3-3h12l-2.5-2.5m2.5 9V14.5a3 3 0 0 1-3 3h-12l2.5 2.5",
    vol: "M4 9.5v5h3.5L12 18.5v-13L7.5 9.5zm11.5-1.8a5 5 0 0 1 0 8.6",
    caret: "m7 10 5 5 5-5", note: "M9 18V6l10-2v12M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0m10-2a3 3 0 1 1-6 0 3 3 0 0 1 6 0"
  };
  function ic(name, size, filled) {
    return '<svg viewBox="0 0 24 24" width="' + (size || 16) + '" height="' + (size || 16) + '" aria-hidden="true" fill="' + (filled ? "currentColor" : "none") +
      '" stroke="' + (filled ? "none" : "currentColor") + '" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="' + P[name] + '"/></svg>';
  }
  function esc(s) { return String(s).replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; }); }
  function art(a, cls) { return '<span class="d-art ' + (cls || "") + '" style="--a1:' + a.c[0] + ";--a2:" + a.c[1] + '"></span>'; }
  function time(s) { s = Math.max(0, Math.floor(s)); return Math.floor(s / 60) + ":" + ("0" + (s % 60)).slice(-2); }

  // ------------------------------------------------------------------ State
  var st = { theme: 0, mode: 0, view: "home", album: null, genre: 0, q: "", liked: { 0: 1, 4: 1, 9: 1 }, song: 0, playing: false, at: 37, hist: [] };

  // ------------------------------------------------------------------ Shell
  // The theme cards already in the page stay for phones; the preview is added beside them
  // and shown on wider screens (CSS swaps the two at 760 px).
  var wrap = document.createElement("div");
  wrap.className = "d-wrap";
  host.appendChild(wrap);
  host.classList.add("has-demo");
  wrap.innerHTML =
    '<div class="d-pick">' +
      '<div class="d-themes" role="group" aria-label="Theme">' +
      THEMES.map(function (t, i) {
        var c = t.modes[0][1].split(" ");
        return '<button type="button" class="d-theme" data-i="' + i + '" aria-pressed="false">' +
          '<span class="d-sw" style="--s1:#' + c[0] + ";--s2:#" + c[11] + ";--s3:#" + c[1] + '"></span>' +
          "<span><b>" + esc(t.name) + "</b><small>" + esc(t.desc) + "</small></span></button>";
      }).join("") + "</div>" +
      '<div class="d-modes" role="group" aria-label="Version"></div>' +
      '<p class="d-hint">Click around: it works like the real app.</p>' +
    "</div>" +
    '<div class="d-app" role="group" aria-label="Interactive preview of Pulse">' +
      '<div class="d-body"><nav class="d-side" aria-label="Preview pages"></nav>' +
        '<div class="d-main"><div class="d-glow" aria-hidden="true"></div>' +
          '<div class="d-top">' +
            '<button type="button" class="d-ib" data-hist="back" aria-label="Back">' + ic("back", 17) + '</button>' +
            '<button type="button" class="d-ib" aria-label="Forward" disabled>' + ic("fwd", 17) + "</button>" +
            '<label class="d-search">' + ic("search", 15) + '<input type="search" placeholder="Search songs, ..." aria-label="Search the preview"><kbd>Ctrl K</kbd></label>' +
            '<span class="d-credit">Music info by Deezer</span>' +
            '<button type="button" class="d-ib" data-toggle-mode aria-label="Switch dark or light">' + ic("sun", 17) + "</button>" +
            '<button type="button" class="d-ib" aria-label="Settings">' + ic("gear", 17) + "</button>" +
            '<span class="d-avatar" aria-hidden="true">' + ic("user", 15) + "</span>" +
          "</div>" +
          '<div class="d-view"></div>' +
        "</div></div>" +
      '<div class="d-player"></div>' +
    "</div>";

  var app = wrap.querySelector(".d-app"), side = wrap.querySelector(".d-side"), view = wrap.querySelector(".d-view");
  var player = wrap.querySelector(".d-player"), modes = wrap.querySelector(".d-modes"), search = wrap.querySelector(".d-search input");

  function applyTheme() {
    var t = THEMES[st.theme], cols = t.modes[st.mode][1].split(" ");
    C.forEach(function (k, i) { app.style.setProperty("--" + k, "#" + cols[i]); });
    app.style.setProperty("--r", t.r + "px");
    ["icons", "inset", "glow", "upper", "bigart", "left"].forEach(function (k) { app.classList.toggle("is-" + k, !!t[k]); });
    app.classList.toggle("is-light", st.mode === 1);
    wrap.querySelectorAll(".d-theme").forEach(function (b, i) { b.setAttribute("aria-pressed", String(i === st.theme)); });
    modes.innerHTML = t.modes.map(function (m, i) {
      return '<button type="button" data-m="' + i + '" aria-pressed="' + (i === st.mode) + '">' + esc(m[0]) + "</button>";
    }).join("");
    drawSide();
    drawPlayer();
  }

  function navItem(id, icon, label, extra) {
    var on = st.view === id && !st.q;
    return '<button type="button" class="d-nav' + (on ? " on" : "") + '" data-view="' + id + '"' + (on ? ' aria-current="page"' : "") + ' aria-label="' + label + '">' +
      (icon === "liked" ? '<span class="d-likedart">' + ic("heart", 12, true) + "</span>" : ic(on && icon === "home" ? "homeF" : icon, 18, on && icon === "home")) +
      "<span>" + label + "</span>" + (extra || "") + "</button>";
  }
  function drawSide() {
    var cur = SONGS[st.song];
    side.innerHTML = '<div class="d-brand"><span class="d-logo"><i></i><i></i><i></i><i></i></span><b>Pulse</b></div>' +
      '<p class="d-label">Browse</p>' + navItem("home", "home", "Home") + navItem("charts", "chart", "Charts") +
      '<p class="d-label">Library</p>' + navItem("liked", "liked", "Liked Songs", "<em>" + Object.keys(st.liked).length + "</em>") +
      navItem("recent", "clock", "Recently Played") + navItem("queue", "queue", "Queue") +
      '<p class="d-label d-pl">Playlists<span>' + ic("list", 14) + ic("plus", 14) + "</span></p>" +
      '<p class="d-make">Make your first playlist with +</p>' +
      (THEMES[st.theme].bigart ? '<div class="d-sidecover">' + art(cur.album, "big") + "<b>" + esc(cur.title) + "</b><small>" + esc(cur.artist) + "</small></div>" : "");
  }

  function head(title, sub) {
    return '<div class="d-head"><h3>' + esc(title) + "</h3>" + (sub ? '<p class="d-sub">' + sub + "</p>" : "") + "</div>";
  }
  function table(list, opts) {
    opts = opts || {};
    if (!list.length) return '<p class="d-empty">' + esc(opts.empty || "Nothing here yet.") + "</p>";
    return '<div class="d-table"><div class="d-th" aria-hidden="true"><span>#</span><span>Title</span><span>Album</span><span>' + (opts.views ? "Views" : "") + "</span><span>" + ic("clock", 13) + "</span></div>" +
      list.map(function (s, i) {
        var now = s.id === st.song;
        return '<div class="d-row' + (now ? " now" : "") + '"><button type="button" class="d-rowplay" data-play="' + s.id + '" aria-label="Play ' + esc(s.title) + '">' +
          '<span class="d-n">' + (now && st.playing ? '<span class="d-eq"><i></i><i></i><i></i></span>' : i + 1) + "</span>" +
          '<span class="d-tt">' + art(s.album) + '<span class="d-t"><b>' + esc(s.title) + "</b><small>" + esc(s.artist) + "</small></span></span>" +
          '<span class="d-al">' + esc(s.album.title) + '</span><span class="d-v">' + (opts.views ? s.views.toLocaleString("en-US") : "") + '</span><span class="d-len">' + time(s.len) + "</span></button>" +
          '<button type="button" class="d-like' + (st.liked[s.id] ? " on" : "") + '" data-like="' + s.id + '" aria-pressed="' + !!st.liked[s.id] + '" aria-label="Like ' + esc(s.title) + '">' + ic("heart", 15, !!st.liked[s.id]) + "</button></div>";
      }).join("") + "</div>";
  }
  function actions(firstId) {
    return '<div class="d-acts"><button type="button" class="d-bigplay" data-play="' + firstId + '" aria-label="Play">' + ic("play", 18, true) + '</button><button type="button" class="d-pill" data-play="' + firstId + '">' + ic("shuffle", 14) + " Shuffle</button></div>";
  }

  function drawView() {
    var h = "";
    if (st.q) {
      var q = st.q.toLowerCase();
      var hits = SONGS.filter(function (s) { return (s.title + " " + s.artist + " " + s.album.title).toLowerCase().indexOf(q) >= 0; });
      h = head("Songs", "Results for “" + esc(st.q) + "”") + table(hits, { empty: "No songs match. Try “night” or “gold”." });
    } else if (st.view === "home") {
      var hour = new Date().getHours();
      h = '<h3 class="d-greet">' + (hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening") + "</h3>" +
        '<div class="d-sec"><h4>Your Orbits</h4><p class="d-sub">Mixes that circle the artists you play most</p></div><div class="d-cards">' +
        ALBUMS.slice(0, 4).map(function (a) {
          return '<button type="button" class="d-card" data-album="' + a.id + '"><span class="d-cover">' + art(a, "sq") + '<span class="d-badge">Orbit</span></span><b>' + esc(a.artist) + " Orbit</b><small>" + esc(a.artist) + " and similar artists</small></button>";
        }).join("") + "</div>" +
        '<div class="d-sec"><h4>Jump back in</h4></div><div class="d-cards">' +
        ALBUMS.slice(2, 6).map(function (a) {
          return '<button type="button" class="d-card" data-album="' + a.id + '"><span class="d-cover">' + art(a, "sq") + "</span><b>" + esc(a.title) + "</b><small>" + esc(a.artist) + "</small></button>";
        }).join("") + "</div>";
    } else if (st.view === "charts") {
      var list = SONGS.slice();
      if (st.genre) list = list.filter(function (s, i) { return (i + st.genre) % 2 === 0 || i % (st.genre + 2) === 0; });
      h = head("Charts", "The 100 most popular songs right now") +
        '<div class="d-genre"><button type="button" class="d-select" data-genre-next>' + ic("note", 13) + " " + esc(GENRES[st.genre]) + " " + ic("caret", 13) + "</button></div>" +
        actions(list[0].id) + table(list.slice(0, 10), { views: true });
    } else if (st.view === "liked") {
      var liked = SONGS.filter(function (s) { return st.liked[s.id]; });
      h = '<div class="d-hero"><span class="d-art hero liked">' + ic("heart", 46, true) + '</span><div><small>PLAYLIST</small><h3>Liked Songs</h3><p class="d-sub">' + liked.length + " songs</p></div></div>" +
        (liked.length ? actions(liked[0].id) : "") + table(liked, { empty: "Tap the heart on any song to keep it here." });
    } else if (st.view === "recent") {
      h = head("Recently Played", "Your last 50 songs, stored only on this computer") + table([SONGS[st.song]].concat(SONGS.filter(function (s) { return s.id !== st.song; }).slice(3, 8)));
    } else if (st.view === "queue") {
      var up = SONGS.slice(st.song + 1).concat(SONGS.slice(0, st.song)).slice(0, 6);
      h = head("Queue") + '<div class="d-sec"><h4>Now playing</h4></div>' + table([SONGS[st.song]]) + '<div class="d-sec"><h4>Next up</h4></div>' + table(up);
    } else if (st.view === "album") {
      var a = ALBUMS.filter(function (x) { return x.id === st.album; })[0];
      var tracks = SONGS.filter(function (s) { return s.album === a; });
      h = '<div class="d-hero">' + art(a, "hero") + "<div><small>ALBUM</small><h3>" + esc(a.title) + '</h3><p class="d-sub"><b>' + esc(a.artist) + "</b> · 2025 · " + tracks.length + " songs</p></div></div>" +
        actions(tracks[0].id) + table(tracks);
    }
    view.innerHTML = h;
    view.scrollTop = 0;
  }

  function drawPlayer() {
    var s = SONGS[st.song];
    var ctrls = '<div class="d-ctrls"><button type="button" aria-label="Shuffle">' + ic("shuffle", 17) + '</button><button type="button" data-step="-1" aria-label="Previous">' + ic("prev", 17) +
      '</button><button type="button" class="d-pp" aria-label="' + (st.playing ? "Pause" : "Play") + '">' + ic(st.playing ? "pause" : "play", 17, true) +
      '</button><button type="button" data-step="1" aria-label="Next">' + ic("next", 17) + '</button><button type="button" aria-label="Repeat">' + ic("repeat", 17) + "</button></div>";
    player.innerHTML = '<div class="d-now">' + art(s.album, "now") + '<span class="d-t"><b>' + esc(s.title) + "</b><small>" + esc(s.artist) + "</small></span>" +
      '<button type="button" class="d-like' + (st.liked[s.id] ? " on" : "") + '" data-like="' + s.id + '" aria-pressed="' + !!st.liked[s.id] + '" aria-label="Like ' + esc(s.title) + '">' + ic("heart", 17, !!st.liked[s.id]) + "</button></div>" +
      '<div class="d-mid">' + ctrls + '<div class="d-prog"><span class="d-cur">' + time(st.at) + '</span><span class="d-bar"><i style="width:' + (st.at / s.len * 100).toFixed(1) + '%"></i></span><span>' + time(s.len) + "</span></div></div>" +
      '<div class="d-vol"><button type="button" data-view="queue" aria-label="Queue">' + ic("queue", 17) + "</button>" + ic("vol", 17) + '<span class="d-bar"><i style="width:72%"></i></span></div>';
    app.style.setProperty("--g1", s.album.c[0]);
    app.style.setProperty("--g2", s.album.c[1]);
  }

  function go(viewName, album) {
    st.hist.push([st.view, st.album]);
    st.view = viewName; st.album = album || null; st.q = ""; search.value = "";
    drawSide(); drawView();
  }
  function play(id) {
    if (id === st.song) { st.playing = !st.playing; } else { st.song = id; st.at = 0; st.playing = true; }
    drawPlayer(); drawSide(); drawView();
    tick();
  }

  // ------------------------------------------------------------------ Playback clock (only while playing and on screen)
  var timer = null, visible = true;
  function tick() {
    clearInterval(timer);
    if (!st.playing || !visible) return;
    timer = setInterval(function () {
      st.at += 1;
      var s = SONGS[st.song];
      if (st.at >= s.len) { st.song = (st.song + 1) % SONGS.length; st.at = 0; drawPlayer(); drawSide(); drawView(); return; }
      var bar = player.querySelector(".d-prog .d-bar i"), cur = player.querySelector(".d-cur");
      if (bar) bar.style.width = (st.at / s.len * 100).toFixed(1) + "%";
      if (cur) cur.textContent = time(st.at);
    }, 1000);
  }
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(function (e) { visible = e[0].isIntersecting; tick(); }).observe(app);
  }

  // ------------------------------------------------------------------ Clicks
  host.addEventListener("click", function (e) {
    var b = e.target.closest("button");
    if (!b || !host.contains(b) || b.disabled) return;
    var d = b.dataset;
    if (d.i != null) { st.theme = +d.i; st.mode = 0; applyTheme(); return; }
    if (d.m != null) { st.mode = +d.m; applyTheme(); return; }
    if (d.toggleMode != null) { st.mode = st.mode ? 0 : 1; applyTheme(); return; }
    if (d.like != null) {
      var id = +d.like;
      if (st.liked[id]) delete st.liked[id]; else st.liked[id] = 1;
      drawPlayer(); drawSide(); drawView();
      return;
    }
    if (d.play != null) { play(+d.play); return; }
    if (d.view) { go(d.view); return; }
    if (d.album) { go("album", d.album); return; }
    if (d.hist === "back") { var h = st.hist.pop(); if (h) { st.view = h[0]; st.album = h[1]; st.q = ""; search.value = ""; drawSide(); drawView(); } return; }
    if (d.genreNext != null) { st.genre = (st.genre + 1) % GENRES.length; drawView(); return; }
    if (d.step) { st.song = (st.song + SONGS.length + +d.step) % SONGS.length; st.at = 0; drawPlayer(); drawSide(); drawView(); tick(); return; }
    if (b.classList.contains("d-pp")) { play(st.song); }
  });
  search.addEventListener("input", function () { st.q = search.value.trim(); drawSide(); drawView(); });

  applyTheme();
  drawView();
})();
