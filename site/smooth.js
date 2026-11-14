/* Smooth scrolling (Lenis, MIT, vendor/lenis.min.js) on the long pages. On for everyone (the site owner chose motion even
   with "reduce motion" on); touch scrolling stays native. Anchor links glide too, stopping under the menu. */
(function () {
  "use strict";
  if (!window.Lenis) return;
  var root = document.documentElement;
  var lenis = new Lenis({
    duration: 1.1,
    easing: function (t) { return Math.min(1, 1.001 - Math.pow(2, -10 * t)); },
    smoothWheel: true,
    respectReducedMotion: false,
    anchors: { offset: -90 },
    // Boxes that scroll on their own keep their own scrolling.
    prevent: function (node) { return !!(node.closest && node.closest(".ask, .lightbox, .menu-panel, pre, .panels .shot, [data-lenis-prevent]")); }
  });
  window.pulseLenis = lenis;
  requestAnimationFrame(function raf(t) { lenis.raf(t); requestAnimationFrame(raf); });
  // Hold still while the intro covers the page.
  var hold = function () { root.getAttribute("data-intro") === "intro" ? lenis.stop() : lenis.start(); };
  hold();
  new MutationObserver(hold).observe(root, { attributes: true, attributeFilter: ["data-intro"] });
})();
