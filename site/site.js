/* Shared by every page: the light/dark switch, the phone menu, and the scroll-in motion. */
(function () {
  "use strict";
  var root = document.documentElement;
  var nav = document.querySelector(".nav");
  var KEY = "pulse-theme";
  var icon = function (body) { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' + body + "</svg>"; };

  // ------------------------------------------------------------------ Light / dark
  // Follows the system until the visitor picks one; the pick is remembered on this browser.
  function setTheme(t, save) {
    root.classList.add("theme-fade");
    root.dataset.theme = t;
    var meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.content = t === "light" ? "#fafafc" : "#0b0b0e";
    if (save) { try { localStorage.setItem(KEY, t); } catch (e) {} }
    if (themeBtn) {
      themeBtn.setAttribute("aria-label", t === "light" ? "Switch to dark mode" : "Switch to light mode");
      themeBtn.title = themeBtn.getAttribute("aria-label");
    }
    setTimeout(function () { root.classList.remove("theme-fade"); }, 350);
  }
  // Your account: a person icon, or your initial once you're signed in on this site.
  var base = (document.currentScript && document.currentScript.src) || location.href;
  if (nav) {
    var who = null;
    try { who = (JSON.parse(localStorage.getItem("pulse-session") || "null") || {}).username || null; } catch (e) {}
    var acct = document.createElement("a");
    acct.className = "nav-icon account-btn" + (who ? " signed-in" : "");
    acct.href = new URL("account/", base).href;
    acct.setAttribute("aria-label", who ? "Your account (@" + who + ")" : "Sign in");
    acct.title = acct.getAttribute("aria-label");
    if (who) acct.textContent = who.charAt(0).toUpperCase();
    else acct.innerHTML = icon('<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>');
    var before = nav.querySelector(":scope > .btn");
    nav.insertBefore(acct, before || null);
  }
  var themeBtn = null;
  if (nav) {
    themeBtn = document.createElement("button");
    themeBtn.type = "button";
    themeBtn.className = "nav-icon theme-btn";
    themeBtn.innerHTML = icon('<path class="moon" d="M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5Z"/>') .replace("</svg>", "") +
      '<g class="sun"><circle cx="12" cy="12" r="4"/><path d="M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></g></svg>';
    var cta = nav.querySelector(":scope > .btn");
    nav.insertBefore(themeBtn, cta || null);
    themeBtn.addEventListener("click", function () { setTheme(root.dataset.theme === "light" ? "dark" : "light", true); });
    setTheme(root.dataset.theme === "light" ? "light" : "dark", false);
    root.classList.remove("theme-fade");
  }
  // Follow the system's setting while the visitor hasn't chosen.
  if (window.matchMedia) {
    var mq = matchMedia("(prefers-color-scheme: light)");
    var follow = function () {
      var saved = null;
      try { saved = localStorage.getItem(KEY); } catch (e) {}
      if (!saved) setTheme(mq.matches ? "light" : "dark", false);
    };
    if (mq.addEventListener) mq.addEventListener("change", follow);
  }

  // ------------------------------------------------------------------ Phone menu
  var links = nav && nav.querySelector("nav ul");
  if (links) {
    var btn = document.createElement("button");
    btn.type = "button";
    btn.className = "nav-icon menu-btn";
    btn.setAttribute("aria-expanded", "false");
    btn.setAttribute("aria-controls", "menu-panel");
    btn.setAttribute("aria-label", "Menu");
    btn.innerHTML = icon('<path class="i-open" d="M4 7h16M4 12h16M4 17h16"/><path class="i-close" d="M6 6l12 12M18 6 6 18"/>');
    var panel = document.createElement("div");
    panel.className = "menu-panel";
    panel.id = "menu-panel";
    panel.hidden = true;
    links.querySelectorAll("a").forEach(function (a) { panel.appendChild(a.cloneNode(true)); });
    var cta2 = nav.querySelector(":scope > .btn");
    if (cta2) { var c = cta2.cloneNode(true); c.classList.add("btn-primary"); panel.appendChild(c); }
    nav.appendChild(btn);
    nav.appendChild(panel);
    var setOpen = function (open) {
      panel.hidden = !open;
      btn.setAttribute("aria-expanded", String(open));
      btn.setAttribute("aria-label", open ? "Close menu" : "Menu");
    };
    btn.addEventListener("click", function () { setOpen(panel.hidden); if (!panel.hidden) { var f = panel.querySelector("a"); if (f) f.focus(); } });
    panel.addEventListener("click", function (e) { if (e.target.closest("a")) setOpen(false); });
    document.addEventListener("keydown", function (e) { if (e.key === "Escape" && !panel.hidden) { setOpen(false); btn.focus(); } });
    document.addEventListener("click", function (e) { if (!panel.hidden && !nav.contains(e.target)) setOpen(false); });
    window.addEventListener("resize", function () { if (window.innerWidth > 760 && !panel.hidden) setOpen(false); });
  }

  // ------------------------------------------------------------------ Scroll-in motion
  // Things below the first screen rise in as they come into view. Without JavaScript,
  // or with reduced motion, everything simply shows.
  if (!("IntersectionObserver" in window) || (window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches)) return;
  var targets = document.querySelectorAll(
    "main [data-reveal], main section:not(.hero) .kicker, main section:not(.hero) h2, main section:not(.hero) .section-lede, " +
    "main .shot:not(.screen):not([role=tabpanel] .shot), main .card, main .panel, main .meter, main .demo, main .facts li, main .box, " +
    "article section > h2, article section > p, article .flow li, article pre, article table, article .callout"
  );
  var vh = window.innerHeight;
  var list = [];
  targets.forEach(function (el) {
    if (el.closest(".hero") || el.getBoundingClientRect().top < vh * 0.9) return; // already on screen
    el.classList.add("reveal");
    list.push(el);
  });
  // Siblings that appear together come in one after another.
  list.forEach(function (el) {
    var sibs = Array.prototype.filter.call(el.parentNode.children, function (s) { return s.classList.contains("reveal"); });
    var i = sibs.indexOf(el);
    if (i > 0) el.style.setProperty("--d", Math.min(i * 0.07, 0.35) + "s");
  });
  root.classList.add("motion");
  var io = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) {
      if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
    });
  }, { rootMargin: "0px 0px -8% 0px", threshold: 0.08 });
  list.forEach(function (el) { io.observe(el); });
})();
