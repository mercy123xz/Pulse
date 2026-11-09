/* The themes section's live preview: a small working copy of Pulse, drawn in HTML.
 * Pick a theme (and its dark or light version) and the preview takes that theme's real
 * colors and layout from assets/themes. Inside it you can switch pages, search, open an
 * album, like songs and play them. The songs and artists are made up; nothing plays.
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
    { id: "default", name: "Default", desc: "Dark violet, with a light version", r: 10, glow: true,
      modes: [["Dark", "0b0b0e 0f0f13 0f0f13 16161b 1d1d24 24242d 2c2c36 ededf1 9696a4 80808e 7c3aed a78bfa"],
              ["Light", "fafafc f3f3f7 f3f3f7 ffffff e8e8ee dedee6 d4d4de 14141a 4e4e5c 646472 7c3aed 6d28d9"]] },
    { id: "blossom", name: "Blossom", desc: "Soft rose, round corners, big sidebar cover", r: 18, bigart: true,
      modes: [["Dusk", "1c1519 160f13 160f13 261c22 33262e 3f2f39 4a3742 fbeef4 d4b3c3 c2a1b2 b8336a ff9cc4"],
              ["Petal", "fff5f8 fdebf1 fdebf1 ffffff f8dbe6 f2cbda ecc4d3 2a1520 5e3447 6e4054 b02a62 9c1f53"]] },
    { id: "mono", name: "Dreary Mono", desc: "Greyscale, square corners, capital titles", r: 2, upper: true,
      modes: [["Mono", "141414 0e0e0e 0e0e0e 1c1c1c 262626 303030 3a3a3a e6e6e6 ababab 9a9a9a 5a5a5a d6d6d6"],
              ["Paper", "f2f2f0 e8e8e6 e8e8e6 fafaf8 dddddb d0d0ce c4c4c2 1a1a1a 424242 505050 3d3d3d 262626"]] },
    { id: "lunar", name: "Lunar", desc: "Night blues, icon sidebar, floating panel", r: 14, icons: true, inset: true, glow: true,
      modes: [["Midnight", "0f1424 090d18 090d18 161c30 1f2740 29324f 313b5c e8ecff a9b2d6 97a1c8 4b5fd6 a3b3ff"],
              ["Eclipse", "101010 000000 000000 181818 242424 2e2e2e 383838 f0e9dc bdb29e a99e8a 94561a f5b45a"]] },
    { id: "aurora", name: "Aurora", desc: "Northern-light greens that glow with the music", r: 22, inset: true, glow: true, left: true,
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
    return { id: i, title: s[0], album: a, artist: a.artist, len: s[2], views: Math.round(1e6 + ((i * 7919) % 97) * 1.37e6) };
  });
  var GENRES = ["All genres", "Pop", "Rap/Hip Hop", "Rock", "Dance", "R&B"];

  // ------------------------------------------------------------------ Icons
  var I = {
    home: "M3 11.5 12 4l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z",
    chart: "M4 20V10m6 10V4m6 16v-7m6 7V8",
    heart: "M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10Z",
    clock: "M12 7v5l3 2M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0",
    search: "m20 20-4.5-4.5M17 10.5a6.5 6.5 0 1 1-13 0 6.5 6.5 0 0 1 13 0",
    play: "M8 5.5v13l11-6.5z",
    pause: "M8 5h3v14H8zm5 0h3v14h-3z",
    next: "M6 5.5v13l9-6.5zM17 5v14",
    prev: "M18 5.5v13l-9-6.5zM7 5v14",
    shuffle: "M4 7h3c4 0 6 10 10 10h3M4 17h3c1.5 0 2.6-1.4 3.6-3M14 9.6C15 8 16.1 7 17 7h3m-2-2 2 2-2 2m0 6 2 2-2 2",
    repeat: "M5 11V9a2 2 0 0 1 2-2h12l-3-3m3 13H7a2 2 0 0 1-2-2v-2m14 0v2l-3 3",
    vol: "M4 10v4h4l5 4V6L8 10zm12-1.5a4 4 0 0 1 0 7"
  };
  function svg(name, filled) {
    return '<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="' + (filled ? "currentColor" : "none") +
      '" stroke="currentColor" stroke-width="' + (filled ? 0 : 2) + '" stroke-linecap="round" stroke-linejoin="round"><path d="' + I[name] + '"/></svg>';
  }
  function esc(s) { return String(s).replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; }); }
  function art(a, cls) { return '<span class="d-art ' + (cls || "") + '" style="--a1:' + a.c[0] + ";--a2:" + a.c[1] + '"></span>'; }
  function time(s) { s = Math.max(0, Math.floor(s)); return Math.floor(s / 60) + ":" + ("0" + (s % 60)).slice(-2); }

  // ------------------------------------------------------------------ State
  var st = { theme: 0, mode: 0, view: "home", album: null, genre: 0, q: "", liked: { 0: 1, 4: 1, 9: 1 }, song: 0, playing: false, at: 37 };

  // ------------------------------------------------------------------ Shell
  host.innerHTML =
    '<div class="d-pick" role="group" aria-label="Theme">' +
      THEMES.map(function (t, i) {
        return '<button type="button" class="d-theme" data-i="' + i + '" aria-pressed="false">' +
          '<span class="d-sw" style="--s1:#' + t.modes[0][1].split(" ")[0] + ";--s2:#" + t.modes[0][1].split(" ")[11] + '"></span>' +
          "<span><b>" + esc(t.name) + "</b><small>" + esc(t.desc) + "</small></span></button>";
      }).join("") +
      '<div class="d-modes" role="group" aria-label="Version"></div>' +
      '<p class="d-hint">Click around: it works like the real app.</p>' +
    "</div>" +
    '<div class="d-app" role="group" aria-label="Interactive preview of Pulse">' +
      '<div class="d-title" aria-hidden="true"><span>Pulse</span><i></i><i></i><i></i></div>' +
      '<div class="d-body"><nav class="d-side" aria-label="Preview pages"></nav><div class="d-main"><div class="d-glow" aria-hidden="true"></div><div class="d-top">' +
        '<label class="d-search">' + svg("search") + '<input type="search" placeholder="Search songs, artists…" aria-label="Search the preview"></label></div>' +
        '<div class="d-view"></div></div></div>' +
      '<div class="d-player"></div>' +
    "</div>";

  var app = host.querySelector(".d-app"), side = host.querySelector(".d-side"), view = host.querySelector(".d-view");
  var player = host.querySelector(".d-player"), modes = host.querySelector(".d-modes"), search = host.querySelector(".d-search input");

  function applyTheme() {
    var t = THEMES[st.theme], cols = t.modes[st.mode][1].split(" ");
    C.forEach(function (k, i) { app.style.setProperty("--" + k, "#" + cols[i]); });
    app.style.setProperty("--r", t.r + "px");
    ["icons", "inset", "glow", "upper", "bigart", "left"].forEach(function (k) { app.classList.toggle("is-" + k, !!t[k]); });
    app.classList.toggle("is-light", st.mode === 1);
    host.querySelectorAll(".d-theme").forEach(function (b, i) { b.setAttribute("aria-pressed", String(i === st.theme)); });
    modes.innerHTML = t.modes.map(function (m, i) {
      return '<button type="button" data-m="' + i + '" aria-pressed="' + (i === st.mode) + '">' + esc(m[0]) + "</button>";
    }).join("");
    drawSide();
    drawPlayer();
  }

  function drawSide() {
    var t = THEMES[st.theme];
    var nav = [["home", "home", "Home"], ["charts", "chart", "Charts"], ["liked", "heart", "Liked Songs"], ["recent", "clock", "Recently Played"]];
    var cur = SONGS[st.song];
    side.innerHTML = '<div class="d-brand"><span class="d-logo"></span><b>Pulse</b></div>' +
      nav.map(function (n) {
        var on = st.view === n[0] || (n[0] === "home" && st.view === "album");
        return '<button type="button" class="d-nav' + (on ? " on" : "") + '" data-view="' + n[0] + '" aria-label="' + n[2] + '"' + (on ? ' aria-current="page"' : "") + ">" +
          svg(n[1]) + "<span>" + n[2] + "</span>" + (n[0] === "liked" ? "<em>" + Object.keys(st.liked).length + "</em>" : "") + "</button>";
      }).join("") +
      (t.bigart ? '<div class="d-sidecover">' + art(cur.album, "big") + "<b>" + esc(cur.title) + "</b><small>" + esc(cur.artist) + "</small></div>" : "");
  }

  function rows(list, opts) {
    if (!list.length) return '<p class="d-empty">' + esc(opts && opts.empty || "Nothing here yet.") + "</p>";
    return '<div class="d-rows" role="list">' + list.map(function (s, i) {
      var now = s.id === st.song;
      return '<div class="d-row' + (now ? " now" : "") + '" role="listitem"><button type="button" class="d-rowplay" data-play="' + s.id + '" aria-label="Play ' + esc(s.title) + '">' +
        '<span class="d-n">' + (now && st.playing ? '<span class="d-eq"><i></i><i></i><i></i></span>' : i + 1) + "</span>" + art(s.album) +
        '<span class="d-t"><b>' + esc(s.title) + "</b><small>" + esc(s.artist) + "</small></span>" +
        (opts && opts.views ? '<span class="d-v">' + s.views.toLocaleString("en-US") + "</span>" : '<span class="d-v">' + esc(s.album.title) + "</span>") +
        '<span class="d-len">' + time(s.len) + "</span></button>" +
        '<button type="button" class="d-like' + (st.liked[s.id] ? " on" : "") + '" data-like="' + s.id + '" aria-pressed="' + !!st.liked[s.id] + '" aria-label="Like ' + esc(s.title) + '">' + svg("heart", !!st.liked[s.id]) + "</button></div>";
    }).join("") + "</div>";
  }

  function drawView() {
    var h = "";
    if (st.q) {
      var q = st.q.toLowerCase();
      var hits = SONGS.filter(function (s) { return (s.title + " " + s.artist + " " + s.album.title).toLowerCase().indexOf(q) >= 0; });
      h = "<h3>Results for “" + esc(st.q) + "”</h3>" + rows(hits, { empty: "No songs match. Try “night” or “gold”." });
    } else if (st.view === "home") {
      var hour = new Date().getHours();
      h = "<h3>" + (hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening") + "</h3>" +
        '<div class="d-quick">' + '<button type="button" class="d-q" data-view="liked"><span class="d-art liked">' + svg("heart", true) + "</span><b>Liked Songs</b></button>" +
        ALBUMS.slice(0, 5).map(function (a) { return '<button type="button" class="d-q" data-album="' + a.id + '">' + art(a) + "<b>" + esc(a.title) + "</b></button>"; }).join("") + "</div>" +
        "<h4>Your Orbits</h4><div class=\"d-cards\">" + ALBUMS.slice(0, 4).map(function (a, i) {
          return '<button type="button" class="d-card" data-album="' + a.id + '">' + art(a, "orbit") + '<span class="d-badge">Orbit</span><b>' + esc(a.artist) + " Orbit</b><small>" + esc(a.artist) + " and similar artists</small></button>";
        }).join("") + "</div>";
    } else if (st.view === "charts") {
      var list = SONGS.slice();
      if (st.genre) list = list.filter(function (s, i) { return (i + st.genre) % 2 === 0 || i % (st.genre + 2) === 0; });
      h = "<h3>Charts</h3><p class=\"d-sub\">The most popular songs right now</p><div class=\"d-genres\">" +
        GENRES.map(function (g, i) { return '<button type="button" data-genre="' + i + '" aria-pressed="' + (i === st.genre) + '">' + esc(g) + "</button>"; }).join("") +
        "</div>" + rows(list.slice(0, 8), { views: true });
    } else if (st.view === "liked") {
      h = "<h3>Liked Songs</h3><p class=\"d-sub\">" + Object.keys(st.liked).length + " songs</p>" +
        rows(SONGS.filter(function (s) { return st.liked[s.id]; }), { empty: "Tap the heart on any song to keep it here." });
    } else if (st.view === "recent") {
      h = "<h3>Recently Played</h3><p class=\"d-sub\">Stored only on this computer</p>" + rows([SONGS[st.song]].concat(SONGS.filter(function (s) { return s.id !== st.song; }).slice(3, 7)));
    } else if (st.view === "album") {
      var a = ALBUMS.filter(function (x) { return x.id === st.album; })[0];
      var tracks = SONGS.filter(function (s) { return s.album === a; });
      h = '<div class="d-hero">' + art(a, "hero") + '<div><small>ALBUM</small><h3>' + esc(a.title) + "</h3><p class=\"d-sub\">" + esc(a.artist) + " · " + tracks.length + " songs</p>" +
        '<button type="button" class="d-bigplay" data-play="' + tracks[0].id + '" aria-label="Play ' + esc(a.title) + '">' + svg("play", true) + "</button></div></div>" + rows(tracks);
    }
    view.innerHTML = h;
    view.scrollTop = 0;
  }

  function drawPlayer() {
    var s = SONGS[st.song];
    var controls = '<div class="d-ctrls"><button type="button" aria-label="Shuffle">' + svg("shuffle") + '</button><button type="button" data-step="-1" aria-label="Previous">' + svg("prev", true) +
      '</button><button type="button" class="d-pp" aria-label="' + (st.playing ? "Pause" : "Play") + '">' + svg(st.playing ? "pause" : "play", true) +
      '</button><button type="button" data-step="1" aria-label="Next">' + svg("next", true) + '</button><button type="button" aria-label="Repeat">' + svg("repeat") + "</button></div>";
    player.innerHTML = '<div class="d-now">' + art(s.album) + '<span class="d-t"><b>' + esc(s.title) + "</b><small>" + esc(s.artist) + "</small></span>" +
      '<button type="button" class="d-like' + (st.liked[s.id] ? " on" : "") + '" data-like="' + s.id + '" aria-pressed="' + !!st.liked[s.id] + '" aria-label="Like ' + esc(s.title) + '">' + svg("heart", !!st.liked[s.id]) + "</button></div>" +
      '<div class="d-mid">' + controls + '<div class="d-prog"><span class="d-cur">' + time(st.at) + '</span><span class="d-bar"><i style="width:' + (st.at / s.len * 100).toFixed(1) + '%"></i></span><span>' + time(s.len) + "</span></div></div>" +
      '<div class="d-vol">' + svg("vol") + '<span class="d-bar"><i style="width:70%"></i></span></div>';
    app.style.setProperty("--g1", s.album.c[0]);
    app.style.setProperty("--g2", s.album.c[1]);
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
    if (!b || !host.contains(b)) return;
    var d = b.dataset;
    if (d.i != null) { st.theme = +d.i; st.mode = 0; applyTheme(); return; }
    if (d.m != null) { st.mode = +d.m; applyTheme(); return; }
    if (d.like != null) {
      var id = +d.like;
      if (st.liked[id]) delete st.liked[id]; else st.liked[id] = 1;
      drawPlayer(); drawSide(); drawView();
      return;
    }
    if (d.play != null) { play(+d.play); return; }
    if (d.view) { st.view = d.view; st.q = ""; search.value = ""; drawSide(); drawView(); return; }
    if (d.album) { st.view = "album"; st.album = d.album; st.q = ""; search.value = ""; drawSide(); drawView(); return; }
    if (d.genre != null) { st.genre = +d.genre; drawView(); return; }
    if (d.step) { st.song = (st.song + SONGS.length + +d.step) % SONGS.length; st.at = 0; drawPlayer(); drawSide(); drawView(); tick(); return; }
    if (b.classList.contains("d-pp")) { play(st.song); }
  });
  search.addEventListener("input", function () { st.q = search.value.trim(); drawView(); });

  applyTheme();
  drawView();
})();
