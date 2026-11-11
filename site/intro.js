/* The home page's intro.
 *
 * First visit: "Press to enter". The press plays a short "bum, ba dum dum dum" (made here
 * with the Web Audio API: there's no sound file), each hit bouncing one of the Pulse bars,
 * left to right. On the last hit the middle bar grows and sweeps across the screen, and
 * the site is behind it.
 *
 * Later visits: a small "Welcome back" in the middle of the screen for a second, with
 * your @username if this browser knows it (it learns it when you open your own profile
 * from the Pulse app). Nothing is sent anywhere: it's all kept in this browser.
 *
 * Skipped with "reduce motion" and when the page is opened at a section (#download...).
 * The head of index.html decides which one to show before the page is drawn.
 */
(function () {
  "use strict";
  var root = document.documentElement;
  var mode = root.getAttribute("data-intro");
  var store = {
    get: function (k) { try { return localStorage.getItem(k); } catch (e) { return null; } },
    set: function (k, v) { try { localStorage.setItem(k, v); } catch (e) {} },
  };

  // ------------------------------------------------------------------ sound
  // "bum — ba dum dum DUM": when each hit lands (seconds) and its notes.
  var HITS = [
    { at: 0.0, freq: 65.4, sub: true, gain: 0.9 }, // bum (low C)
    { at: 0.42, freq: 164.8, gain: 0.45 }, // ba (E)
    { at: 0.56, freq: 196.0, gain: 0.5 }, // dum (G)
    { at: 0.7, freq: 261.6, gain: 0.55 }, // dum (C)
    { at: 0.9, freq: 130.8, sub: true, chord: [261.6, 329.6, 392.0], gain: 1.0 }, // DUM
  ];
  var ac = null;

  function play() {
    var Ctx = window.AudioContext || window.webkitAudioContext;
    if (!Ctx) return null;
    try { ac = new Ctx(); } catch (e) { return null; }
    var out = ac.createGain();
    out.gain.value = 0.55;
    // A little warmth: cut the harshest highs.
    var tone = ac.createBiquadFilter();
    tone.type = "lowpass";
    tone.frequency.value = 2600;
    out.connect(tone).connect(ac.destination);
    var t0 = ac.currentTime + 0.05;
    HITS.forEach(function (h) {
      var t = t0 + h.at;
      var len = h.sub ? 0.55 : 0.28;
      note(out, t, h.freq, "triangle", h.gain * 0.6, len, true);
      if (h.sub) note(out, t, h.freq / 2, "sine", h.gain, len + 0.2, true); // the bass thump
      (h.chord || []).forEach(function (f) { note(out, t, f, "sine", 0.18, 0.7, false); });
    });
    return t0;
  }

  // One plucked note: quick attack, a bouncy pitch drop, then it dies away.
  function note(out, t, freq, type, gain, len, bounce) {
    var o = ac.createOscillator();
    var g = ac.createGain();
    o.type = type;
    o.frequency.setValueAtTime(bounce ? freq * 1.5 : freq, t);
    o.frequency.exponentialRampToValueAtTime(freq, t + 0.06);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(gain, t + 0.012);
    g.gain.exponentialRampToValueAtTime(0.0001, t + len);
    o.connect(g).connect(out);
    o.start(t);
    o.stop(t + len + 0.05);
  }

  // ------------------------------------------------------------------ intro
  function intro() {
    var box = document.getElementById("intro");
    if (!box) return finish();
    var enter = box.querySelector(".intro-enter");
    var skip = box.querySelector(".intro-skip");
    var bars = [].slice.call(box.querySelectorAll(".intro-bars i"));
    var started = false;
    var timers = [];
    box.hidden = false;
    setTimeout(function () { enter.focus({ preventScroll: true }); }, 50);

    function go() {
      if (started) return;
      started = true;
      box.classList.add("playing");
      var t0 = play();
      // Line the bars up with the sound's own clock when there is one.
      var lead = t0 && ac ? Math.max(0, (t0 - ac.currentTime) * 1000) : 50;
      HITS.forEach(function (h, i) {
        timers.push(setTimeout(function () { bounce(bars[i], i === HITS.length - 1); }, lead + h.at * 1000));
      });
      timers.push(setTimeout(function () { sweep(bars[2]); }, lead + 1000));
    }

    function bounce(bar, last) {
      bar.classList.remove("hit");
      void bar.offsetWidth; // start the animation again
      bar.classList.add("hit");
      if (last) bars.forEach(function (b) { b.classList.add("hit-all"); });
    }

    // The middle bar grows to the screen's height, widens across it, then slides away.
    function sweep(bar) {
      var r = bar.getBoundingClientRect();
      var wall = document.createElement("div");
      wall.className = "intro-wall";
      document.body.appendChild(wall);
      var W = window.innerWidth;
      var H = window.innerHeight;
      var from = { left: r.left + "px", top: r.top + "px", width: r.width + "px", height: r.height + "px", borderRadius: r.width / 2 + "px" };
      var tall = { left: r.left + "px", top: "0px", width: r.width + "px", height: H + "px", borderRadius: r.width / 2 + "px" };
      var full = { left: "0px", top: "0px", width: W + "px", height: H + "px", borderRadius: "0px" };
      var a = wall.animate([from, tall, full], { duration: 620, easing: "cubic-bezier(.7,0,.3,1)", fill: "forwards" });
      a.onfinish = function () {
        box.hidden = true;
        finish();
        wall.animate([{ transform: "translateX(0)" }, { transform: "translateX(100%)" }], { duration: 520, easing: "cubic-bezier(.7,0,.3,1)", fill: "forwards" }).onfinish =
          function () { wall.remove(); };
      };
    }

    function stop() {
      timers.forEach(clearTimeout);
      if (ac) { try { ac.close(); } catch (e) {} }
      box.hidden = true;
      finish();
    }

    enter.addEventListener("click", go);
    skip.addEventListener("click", stop);
    box.addEventListener("click", function (e) { if (e.target === box) go(); });
    document.addEventListener("keydown", function key(e) {
      if (box.hidden) return document.removeEventListener("keydown", key);
      if (e.key === "Escape") { e.preventDefault(); stop(); }
      else if ((e.key === "Enter" || e.key === " ") && document.activeElement !== skip) { e.preventDefault(); go(); }
    });
  }

  // The site appears: the top of the page eases in, and the visit is remembered.
  function finish() {
    store.set("pulse-visited", "1");
    root.setAttribute("data-intro", "done");
  }

  // ------------------------------------------------------------------ welcome back
  function welcome() {
    var user = store.get("pulse-user");
    var b = document.createElement("div");
    b.className = "welcome";
    b.setAttribute("role", "status");
    var bars = document.createElement("span");
    bars.className = "welcome-bars";
    bars.setAttribute("aria-hidden", "true");
    for (var i = 0; i < 5; i++) bars.appendChild(document.createElement("i"));
    b.appendChild(bars);
    var text = document.createElement("span");
    text.textContent = "Welcome back" + (user && /^[a-z0-9_.]{1,20}$/.test(user) ? ", @" + user : "") + " 👋";
    b.appendChild(text);
    document.body.appendChild(b);
    requestAnimationFrame(function () { b.classList.add("shown"); });
    var gone = false;
    function hide() {
      if (gone) return;
      gone = true;
      b.classList.remove("shown");
      setTimeout(function () { b.remove(); }, 400);
      removeEventListener("scroll", hide);
      removeEventListener("keydown", hide);
    }
    setTimeout(hide, 1400);
    b.addEventListener("click", hide);
    addEventListener("scroll", hide, { passive: true });
    addEventListener("keydown", hide);
  }

  // "Replay intro" in the footer.
  window.pulseReplayIntro = function () {
    var box = document.getElementById("intro");
    if (!box) return;
    scrollTo(0, 0);
    root.setAttribute("data-intro", "intro");
    box.classList.remove("playing");
    // Fresh buttons, so the last run's handlers don't fire too.
    [".intro-enter", ".intro-skip"].forEach(function (s) {
      var old = box.querySelector(s);
      old.parentNode.replaceChild(old.cloneNode(true), old);
    });
    [].forEach.call(box.querySelectorAll(".intro-bars i"), function (b) { b.classList.remove("hit", "hit-all"); });
    intro();
  };

  var replay = document.getElementById("replay-intro");
  if (replay) replay.addEventListener("click", function (e) { e.preventDefault(); window.pulseReplayIntro(); });

  if (mode === "intro") intro();
  else if (mode === "back") welcome();
})();
