// The home page's moving parts: magnetic buttons, the performance bars and the Orbits
// picture that start when they scroll into view, and the Browse tabs.
// Nothing here is needed to read the page or download Pulse.
(function () {
  "use strict";
  var still = false; // motion for everyone, even with "reduce motion" on (site owner's choice)
  var fine = matchMedia("(hover: hover) and (pointer: fine)").matches;
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var clamp = function (v, a, b) { return Math.max(a, Math.min(b, v)); };

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
  // The big screenshot leans back until it's in view.
  var show = $("#showcase"), orbit = $("#orbit"), vs = $(".vs");
  var ticking = false;
  var onScroll = function () {
    ticking = false;
    var vh = innerHeight;
    if (show) {
      var r = show.getBoundingClientRect();
      show.style.setProperty("--p", clamp((vh - r.top) / (vh * 0.75), 0, 1).toFixed(3));
    }
  };
  if (!still) {
    addEventListener("scroll", function () { if (!ticking) { ticking = true; requestAnimationFrame(onScroll); } }, { passive: true });
    addEventListener("resize", onScroll);
    onScroll();
  }
  // The Orbits go round only while they're on screen.
  if (orbit && "IntersectionObserver" in window) {
    new IntersectionObserver(function (es) { orbit.classList.toggle("on", es[0].isIntersecting); }).observe(orbit);
  }
  if (vs && "IntersectionObserver" in window) {
    var vo = new IntersectionObserver(function (es) { if (es[0].isIntersecting) { vs.classList.add("seen"); vo.disconnect(); } }, { threshold: 0.35 });
    vo.observe(vs);
  } else if (vs) vs.classList.add("seen");

  // ------------------------------------------------------------------ Browse tabs
  var list = $(".tabs");
  if (list) {
    var tabs = [].slice.call(list.querySelectorAll('[role="tab"]'));
    // The section takes a soft wash of colour from the artwork on show, like the app's
    // album pages: the vivid pixels of the screenshot, averaged.
    var section = list.closest("section");
    var tint = function (panel) {
      var img = panel && panel.querySelector("img");
      if (!img || !section) return;
      if (!img.complete || !img.naturalWidth) { img.addEventListener("load", function () { tint(panel); }, { once: true }); return; }
      try {
        var c = document.createElement("canvas"), w = 64, h = Math.max(1, Math.round(64 * img.naturalHeight / img.naturalWidth));
        c.width = w; c.height = h;
        var x = c.getContext("2d", { willReadFrequently: true });
        x.drawImage(img, 0, 0, w, h);
        var d = x.getImageData(0, 0, w, h).data, r = 0, g = 0, b = 0, n = 0;
        for (var i = 0; i < d.length; i += 4) {
          var mx = Math.max(d[i], d[i + 1], d[i + 2]), mn = Math.min(d[i], d[i + 1], d[i + 2]);
          var wgt = mx < 40 ? 0 : (mx - mn) / 255; // skip the app's dark background, favour colour
          r += d[i] * wgt; g += d[i + 1] * wgt; b += d[i + 2] * wgt; n += wgt;
        }
        if (n > 0) section.style.setProperty("--art", "rgb(" + Math.round(r / n) + " " + Math.round(g / n) + " " + Math.round(b / n) + ")");
      } catch (e) {}
    };
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
      tint(document.getElementById(t.getAttribute("aria-controls")));
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
    tint(document.querySelector('[role="tabpanel"]:not([hidden])'));
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(start);
    start();
    addEventListener("resize", start);
  }

})();
