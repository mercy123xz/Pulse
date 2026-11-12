// The home page's moving parts: the hero light that follows the cursor, magnetic buttons,
// the screenshot that settles as you scroll, the Browse tabs, the Orbits picture, and the
// player and equalizer demos (which really play and shape the demo track, in the browser).
// Nothing here is needed to read the page or download Pulse.
(function () {
  "use strict";
  var still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  var fine = matchMedia("(hover: hover) and (pointer: fine)").matches;
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var clamp = function (v, a, b) { return Math.max(a, Math.min(b, v)); };

  // ------------------------------------------------------------------ hero light
  var hero = $(".hero");
  if (hero && fine && !still) {
    var hx = 0.7, hy = 0.4, tx = hx, ty = hy, raf = 0;
    var step = function () {
      hx += (tx - hx) * 0.08;
      hy += (ty - hy) * 0.08;
      hero.style.setProperty("--mx", hx.toFixed(4));
      hero.style.setProperty("--my", hy.toFixed(4));
      raf = Math.abs(tx - hx) + Math.abs(ty - hy) > 0.001 ? requestAnimationFrame(step) : 0;
    };
    hero.addEventListener("pointermove", function (e) {
      var r = hero.getBoundingClientRect();
      tx = (e.clientX - r.left) / r.width;
      ty = (e.clientY - r.top) / r.height;
      if (!raf) raf = requestAnimationFrame(step);
    }, { passive: true });
  }

  // ------------------------------------------------------------------ magnetic buttons
  if (fine && !still) {
    document.querySelectorAll("[data-magnetic]").forEach(function (b) {
      b.addEventListener("pointermove", function (e) {
        var r = b.getBoundingClientRect();
        var x = (e.clientX - r.left - r.width / 2) / r.width;
        var y = (e.clientY - r.top - r.height / 2) / r.height;
        b.style.transform = "translate(" + (x * 8).toFixed(1) + "px," + (y * 6).toFixed(1) + "px)";
      });
      b.addEventListener("pointerleave", function () { b.style.transform = ""; });
    });
  }

  // ------------------------------------------------------------------ scroll-linked bits
  // The big screenshot leans back until it's in view; the Orbits turn as you scroll.
  var show = $("#showcase"), orbit = $("#orbit"), vs = $(".vs");
  var ticking = false;
  var onScroll = function () {
    ticking = false;
    var vh = innerHeight;
    if (show) {
      var r = show.getBoundingClientRect();
      show.style.setProperty("--p", clamp((vh - r.top) / (vh * 0.75), 0, 1).toFixed(3));
    }
    if (orbit) {
      var o = orbit.getBoundingClientRect();
      orbit.style.setProperty("--turn", ((vh - o.top) * 0.08).toFixed(2) + "deg");
    }
  };
  if (!still) {
    addEventListener("scroll", function () { if (!ticking) { ticking = true; requestAnimationFrame(onScroll); } }, { passive: true });
    addEventListener("resize", onScroll);
    onScroll();
  }
  if (vs && "IntersectionObserver" in window) {
    var vo = new IntersectionObserver(function (es) { if (es[0].isIntersecting) { vs.classList.add("seen"); vo.disconnect(); } }, { threshold: 0.35 });
    vo.observe(vs);
  } else if (vs) vs.classList.add("seen");

  // ------------------------------------------------------------------ Browse tabs
  var list = $(".tabs");
  if (list) {
    var tabs = [].slice.call(list.querySelectorAll('[role="tab"]'));
    var pill = list.querySelector(".tab-pill");
    var place = function (t) {
      pill.style.width = t.offsetWidth + "px";
      pill.style.transform = "translateX(" + (t.offsetLeft) + "px)";
    };
    var pick = function (t, focus) {
      tabs.forEach(function (o) {
        var on = o === t;
        o.setAttribute("aria-selected", String(on));
        o.tabIndex = on ? 0 : -1;
        document.getElementById(o.getAttribute("aria-controls")).hidden = !on;
      });
      place(t);
      if (focus) t.focus();
    };
    tabs.forEach(function (t, i) {
      t.addEventListener("click", function () { pick(t); });
      t.addEventListener("keydown", function (e) {
        var n = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
        if (e.key === "Home") n = -i;
        if (e.key === "End") n = tabs.length - 1 - i;
        if (n) { e.preventDefault(); pick(tabs[(i + n + tabs.length) % tabs.length], true); }
      });
    });
    var start = function () { place(tabs.filter(function (t) { return t.getAttribute("aria-selected") === "true"; })[0] || tabs[0]); };
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(start);
    start();
    addEventListener("resize", start);
  }

  // ------------------------------------------------------------------ the demo track
  // One small audio graph for the player and the equalizer: the track, six peaking filters
  // (60 Hz to 15 kHz, Q 1, ±12 dB: the same as Pulse's), the same pre-amp, and an analyser
  // for the waveform. Loaded only when someone presses play.
  var BANDS = [60, 150, 400, 1000, 2400, 15000];
  var PRESETS = [
    ["Flat", [0, 0, 0, 0, 0, 0]],
    ["Bass Booster", [6, 4.5, 1, 0, 0, 0]],
    ["Bass Reducer", [-6, -4, -1, 0, 0, 0]],
    ["Treble Booster", [0, 0, 0, 1.5, 4, 6]],
    ["Vocal Booster", [-2, -1, 2.5, 4, 2.5, 0]],
    ["Loudness", [5, 3, 0, -1, 2, 5]],
    ["Acoustic", [4, 3, 1, 2, 3, 3]],
    ["Electronic", [4.5, 3.5, 0, -1.5, 2, 4.5]],
  ];
  var gains = [0, 0, 0, 0, 0, 0];
  var SRC = "media/pulse-launch.mp4";
  var ctx = null, buffer = null, filters = [], pre = null, analyser = null, node = null;
  var playing = false, loading = false, startedAt = 0, offset = 0, loop = true;
  var listeners = [];
  var emit = function () { listeners.forEach(function (f) { f(); }); };

  var fmt = function (s) { s = Math.max(0, Math.floor(s)); return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0"); };
  var duration = function () { return buffer ? buffer.duration : 0; };
  var position = function () {
    if (!buffer) return 0;
    if (!playing) return offset;
    var p = ctx.currentTime - startedAt;
    return loop ? p % buffer.duration : Math.min(p, buffer.duration);
  };
  var applyGains = function () {
    if (!ctx) return;
    var t = ctx.currentTime;
    filters.forEach(function (f, i) { f.gain.setTargetAtTime(gains[i], t, 0.03); });
    var boost = Math.max(0, Math.max.apply(null, gains));
    pre.gain.setTargetAtTime(Math.pow(10, (-boost * 0.5) / 20), t, 0.03);
  };
  var ensure = function () {
    if (buffer) return Promise.resolve();
    var AC = window.AudioContext || window.webkitAudioContext;
    if (!AC) return Promise.reject(new Error("no audio"));
    ctx = ctx || new AC();
    loading = true;
    emit();
    return fetch(SRC).then(function (r) { if (!r.ok) throw new Error(r.status); return r.arrayBuffer(); })
      .then(function (data) { return new Promise(function (ok, no) { ctx.decodeAudioData(data, ok, no); }); })
      .then(function (b) {
        buffer = b;
        filters = BANDS.map(function (f) {
          var q = ctx.createBiquadFilter();
          q.type = "peaking";
          q.frequency.value = Math.min(f, ctx.sampleRate * 0.45);
          q.Q.value = 1;
          return q;
        });
        pre = ctx.createGain();
        analyser = ctx.createAnalyser();
        analyser.fftSize = 256;
        analyser.smoothingTimeConstant = 0.78;
        filters.reduce(function (a, f) { a.connect(f); return f; }, pre).connect(analyser);
        analyser.connect(ctx.destination);
        applyGains();
        peaks = peaksOf(b, 96);
        loading = false;
        emit();
      })
      .catch(function (e) { loading = false; emit(); throw e; });
  };
  var stopNode = function () { if (node) { node.onended = null; try { node.stop(); } catch (e) {} node.disconnect(); node = null; } };
  var play = function (from) {
    return ensure().then(function () {
      if (ctx.state === "suspended") ctx.resume();
      stopNode();
      offset = from == null ? offset : from;
      if (offset >= buffer.duration - 0.05) offset = 0;
      node = ctx.createBufferSource();
      node.buffer = buffer;
      node.loop = loop;
      node.connect(pre);
      node.start(0, offset);
      startedAt = ctx.currentTime - offset;
      node.onended = function () { if (!loop && playing) { playing = false; offset = 0; emit(); } };
      playing = true;
      emit();
    });
  };
  var pause = function () { offset = position(); stopNode(); playing = false; emit(); };
  var seek = function (t) { t = clamp(t, 0, duration()); if (playing) play(t); else { offset = t; emit(); } };
  var toggle = function () { return playing ? (pause(), Promise.resolve()) : play(); };
  // Another tab of sound (the launch video) starting: pause the demo.
  var launch = document.getElementById("launch");
  if (launch) launch.addEventListener("play", function () { if (!launch.muted && playing) pause(); });

  // A waveform from the real track: the loudest point in each slice.
  var peaks = null;
  function peaksOf(b, n) {
    var d = b.getChannelData(0), size = Math.floor(d.length / n), out = [], max = 0;
    for (var i = 0; i < n; i++) {
      var m = 0;
      for (var j = i * size, e = j + size; j < e; j += 16) { var v = Math.abs(d[j]); if (v > m) m = v; }
      out.push(m);
      if (m > max) max = m;
    }
    return out.map(function (v) { return Math.pow(v / (max || 1), 0.8); });
  }

  // ------------------------------------------------------------------ the player demo
  var pd = $("#pd");
  if (pd) {
    var canvas = $("#pd-wave"), g = canvas.getContext("2d");
    var cur = $("#pd-cur"), dur = $("#pd-dur"), status = $("#pd-status"), playBtn = $("#pd-play"), loopBtn = $("#pd-loop");
    var bars = [].slice.call(pd.querySelectorAll(".pd-art i"));
    // Before the track loads: a quiet, even shape (nothing here is a real waveform yet).
    var idle = [];
    for (var k = 0; k < 96; k++) idle.push(0.25 + 0.12 * Math.sin(k * 0.45) * Math.sin(k * 0.13));
    var bins = null, live = new Array(96).fill(0);
    var styles = function () {
      var cs = getComputedStyle(document.documentElement);
      return { on: cs.getPropertyValue("--accent-soft").trim() || "#a78bfa", off: cs.getPropertyValue("--border").trim() || "#2c2c36", text: cs.getPropertyValue("--text").trim() };
    };
    var colors = styles();
    new MutationObserver(function () { colors = styles(); draw(); }).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    var size = function () {
      var r = canvas.getBoundingClientRect(), dpr = Math.min(devicePixelRatio || 1, 2);
      canvas.width = Math.round(r.width * dpr);
      canvas.height = Math.round(r.height * dpr);
      g.setTransform(dpr, 0, 0, dpr, 0, 0);
      draw();
    };
    var draw = function () {
      var w = canvas.clientWidth, h = canvas.clientHeight;
      if (!w) return;
      g.clearRect(0, 0, w, h);
      var shape = peaks || idle, n = shape.length, gap = 2, bw = (w - gap * (n - 1)) / n;
      var done = duration() ? position() / duration() : 0;
      if (analyser && playing) {
        bins = bins || new Uint8Array(analyser.frequencyBinCount);
        analyser.getByteFrequencyData(bins);
      }
      for (var i = 0; i < n; i++) {
        var target = playing && bins ? bins[Math.floor((i / n) * bins.length * 0.7)] / 255 : 0;
        live[i] += (target - live[i]) * 0.35;
        var v = shape[i] * 0.8 + live[i] * 0.35;
        var bh = Math.max(3, Math.min(h, v * h));
        g.fillStyle = i / n < done ? colors.on : colors.off;
        g.globalAlpha = i / n < done ? 1 : 0.9;
        var x = i * (bw + gap), y = (h - bh) / 2;
        if (g.roundRect) { g.beginPath(); g.roundRect(x, y, bw, bh, Math.min(bw / 2, 2)); g.fill(); } else g.fillRect(x, y, bw, bh);
      }
      g.globalAlpha = 1;
    };
    var frame = 0;
    var loopDraw = function () {
      draw();
      cur.textContent = fmt(position());
      var pct = duration() ? Math.round((position() / duration()) * 100) : 0;
      canvas.setAttribute("aria-valuenow", String(pct));
      canvas.setAttribute("aria-valuetext", fmt(position()) + " of " + fmt(duration()));
      // The cover's bars move with the music.
      if (!still && bins) {
        bars.forEach(function (b, i) { b.style.transform = "scaleY(" + (0.75 + (bins[2 + i * 6] / 255) * 0.55).toFixed(3) + ")"; });
      }
      frame = playing ? requestAnimationFrame(loopDraw) : 0;
    };
    listeners.push(function () {
      pd.classList.toggle("playing", playing);
      pd.classList.toggle("loading", loading);
      playBtn.setAttribute("aria-label", playing ? "Pause" : "Play");
      dur.textContent = fmt(duration());
      cur.textContent = fmt(position());
      status.textContent = loading ? "Loading the track…" : playing ? "Playing in your browser." : buffer ? "Paused." : "Press play to load the track (1.8 MB).";
      if (!playing) bars.forEach(function (b) { b.style.transform = ""; });
      if (playing && !frame) frame = requestAnimationFrame(loopDraw);
      draw();
    });
    playBtn.addEventListener("click", function () {
      toggle().catch(function () { status.textContent = "This browser couldn't play the track."; });
    });
    $("#pd-restart").addEventListener("click", function () { seek(0); });
    loopBtn.addEventListener("click", function () {
      loop = !loop;
      loopBtn.setAttribute("aria-pressed", String(loop));
      loopBtn.classList.toggle("on", loop);
      if (playing) play(position());
    });
    loopBtn.classList.toggle("on", loop);
    // Click or drag on the waveform to jump; arrow keys move 5 seconds.
    var scrub = function (e) {
      var r = canvas.getBoundingClientRect();
      var t = clamp((e.clientX - r.left) / r.width, 0, 1);
      if (!buffer) { play().then(function () { seek(t * duration()); }).catch(function () {}); return; }
      seek(t * duration());
    };
    var down = false;
    canvas.addEventListener("pointerdown", function (e) { down = true; canvas.setPointerCapture(e.pointerId); scrub(e); });
    canvas.addEventListener("pointermove", function (e) { if (down && buffer) { offset = clamp(((e.clientX - canvas.getBoundingClientRect().left) / canvas.clientWidth) * duration(), 0, duration()); if (!playing) emit(); } });
    canvas.addEventListener("pointerup", function (e) { if (down) { down = false; if (buffer && playing) seek(offset); } });
    canvas.addEventListener("keydown", function (e) {
      if (!buffer) return;
      var d = e.key === "ArrowRight" ? 5 : e.key === "ArrowLeft" ? -5 : 0;
      if (e.key === "Home") { e.preventDefault(); seek(0); }
      if (d) { e.preventDefault(); seek(position() + d); }
      if (e.key === " " || e.key === "Enter") { e.preventDefault(); toggle(); }
    });
    new ResizeObserver(size).observe(canvas);
    // Stop when it scrolls far away (it's a demo, not background music).
    if ("IntersectionObserver" in window) {
      var near = new IntersectionObserver(function (es) {
        if (!es[0].isIntersecting && playing && !document.getElementById("eq").matches(":hover")) {
          var eqBox = document.getElementById("eq").getBoundingClientRect();
          if (eqBox.bottom < 0 || eqBox.top > innerHeight) pause();
        }
      }, { rootMargin: "60% 0px" });
      near.observe(pd);
    }
  }

  // ------------------------------------------------------------------ the equalizer demo
  var eq = $("#eq");
  if (eq) {
    var presetBox = eq.querySelector(".eq-presets"), bandBox = eq.querySelector(".eq-bands");
    var curve = eq.querySelector(".curve"), area = eq.querySelector(".area");
    var say = $("#eq-say"), eqPlay = $("#eq-play");
    var inputs = [];
    var label = function (f) { return f >= 1000 ? f / 1000 + " kHz" : f + " Hz"; };
    var db = function (v) { return (v > 0 ? "+" : v < 0 ? "−" : "") + Math.abs(v).toFixed(1).replace(/\.0$/, "") + " dB"; };
    BANDS.forEach(function (f, i) {
      var wrap = document.createElement("div");
      wrap.className = "eq-band";
      var id = "eq-" + i;
      wrap.innerHTML = '<output for="' + id + '">0 dB</output><input id="' + id + '" type="range" min="-12" max="12" step="0.5" value="0" aria-label="' + label(f) + '"><label for="' + id + '">' + label(f).replace(" ", "&nbsp;") + "</label>";
      bandBox.appendChild(wrap);
      var input = wrap.querySelector("input"), out = wrap.querySelector("output");
      input.addEventListener("input", function () {
        gains[i] = parseFloat(input.value);
        out.textContent = db(gains[i]);
        input.setAttribute("aria-valuetext", db(gains[i]));
        markPreset();
        refresh();
      });
      inputs.push({ input: input, out: out });
    });
    var presetBtns = PRESETS.map(function (p) {
      var b = document.createElement("button");
      b.type = "button";
      b.textContent = p[0];
      b.setAttribute("aria-pressed", "false");
      b.addEventListener("click", function () { setGains(p[1]); });
      presetBox.appendChild(b);
      return b;
    });
    var markPreset = function () {
      PRESETS.forEach(function (p, i) { presetBtns[i].setAttribute("aria-pressed", String(p[1].every(function (v, j) { return v === gains[j]; }))); });
    };
    var anim = 0;
    var setGains = function (to) {
      var from = gains.slice(), t0 = performance.now(), len = still ? 0 : 420;
      cancelAnimationFrame(anim);
      var tick = function (now) {
        var k = len ? Math.min(1, (now - t0) / len) : 1;
        var e = 1 - Math.pow(1 - k, 3);
        for (var i = 0; i < 6; i++) {
          gains[i] = Math.round((from[i] + (to[i] - from[i]) * e) * 2) / 2;
          if (k === 1) gains[i] = to[i];
          inputs[i].input.value = gains[i];
          inputs[i].out.textContent = db(gains[i]);
          inputs[i].input.setAttribute("aria-valuetext", db(gains[i]));
        }
        refresh();
        if (k < 1) anim = requestAnimationFrame(tick); else markPreset();
      };
      anim = requestAnimationFrame(tick);
    };
    // The curve: what the six filters do together, worked out the way Pulse does it.
    var response = function (freq) {
      var rate = 48000, w = (2 * Math.PI * freq) / rate, total = 0;
      for (var i = 0; i < 6; i++) {
        if (!gains[i]) continue;
        var A = Math.pow(10, gains[i] / 40), w0 = (2 * Math.PI * Math.min(BANDS[i], rate * 0.45)) / rate, alpha = Math.sin(w0) / 2, c = Math.cos(w0);
        var b0 = 1 + alpha * A, b1 = -2 * c, b2 = 1 - alpha * A, a0 = 1 + alpha / A, a1 = -2 * c, a2 = 1 - alpha / A;
        var cw = Math.cos(w), sw = Math.sin(w), c2 = Math.cos(2 * w), s2 = Math.sin(2 * w);
        var nr = b0 + b1 * cw + b2 * c2, ni = -(b1 * sw + b2 * s2), dr = a0 + a1 * cw + a2 * c2, di = -(a1 * sw + a2 * s2);
        total += 10 * Math.log10((nr * nr + ni * ni) / (dr * dr + di * di));
      }
      return total;
    };
    var refresh = function () {
      var pts = [];
      for (var x = 0; x <= 600; x += 6) {
        var f = 20 * Math.pow(1000, x / 600);
        var y = 75 - clamp(response(f), -12, 12) * (75 / 12) * 0.92;
        pts.push(x + " " + y.toFixed(1));
      }
      var d = "M" + pts.join(" L");
      curve.setAttribute("d", d);
      area.setAttribute("d", d + " L600 150 L0 150 Z");
      applyGains();
    };
    refresh();
    markPreset();
    var eqSay = function () {
      eqPlay.textContent = playing ? "Pause the demo track" : "Play the demo track";
      say.textContent = loading ? "Loading the track…" : playing ? "You're hearing these settings on the demo track." : "Press Play to hear these settings on the demo track.";
    };
    listeners.push(eqSay);
    eqPlay.addEventListener("click", function () { toggle().catch(function () { say.textContent = "This browser couldn't play the track."; }); });
  }
})();
