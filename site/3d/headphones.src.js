// A pair of headphones in 3D for the download section: they turn to follow the cursor,
// can be dragged to spin, and pulse when clicked. Built from simple shapes in code (no
// model file). Bundled with three.js into ../headphones.js: see 3d/README.md.
//
// Loaded only when the section comes into view; draws only while it's on screen and the
// tab is visible. With "reduce motion" on, it's drawn once and stays still.

import {
  ACESFilmicToneMapping, BoxGeometry, CanvasTexture, CatmullRomCurve3, CircleGeometry, Color, CylinderGeometry, DirectionalLight,
  ExtrudeGeometry, Group, LatheGeometry, Mesh, MeshBasicMaterial, MeshPhysicalMaterial, MeshStandardMaterial, PMREMGenerator,
  PerspectiveCamera, PlaneGeometry, PointLight, RepeatWrapping, SRGBColorSpace, Scene, Shape, SphereGeometry, TorusGeometry,
  Vector2, Vector3, WebGLRenderer,
} from "three";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";

export function mount(canvas) {
  let renderer;
  try {
    renderer = new WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: "low-power" });
  } catch (e) {
    return null; // no WebGL: the section just shows its text
  }
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.outputColorSpace = SRGBColorSpace;
  renderer.toneMapping = ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.05;

  const scene = new Scene();
  const pmrem = new PMREMGenerator(renderer);
  scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.03).texture;
  scene.environmentIntensity = 0.9;
  pmrem.dispose();

  const camera = new PerspectiveCamera(32, 1, 0.1, 50);
  camera.position.set(0, 0.15, 7.2);

  // Light: a soft key from the top left, Pulse purple from behind, a cool fill.
  const key = new DirectionalLight(0xffffff, 1.6);
  key.position.set(-3, 4, 5);
  scene.add(key);
  const rim = new PointLight(0x8b5cf6, 28, 12);
  rim.position.set(2.5, 1.5, -3);
  scene.add(rim);
  const fill = new PointLight(0x60a5fa, 12, 12);
  fill.position.set(-3, -2, 3);
  scene.add(fill);

  // ---------------------------------------------------------------- textures (drawn in code)
  const canvasTexture = (w, h, paint, repeat = [1, 1]) => {
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    paint(c.getContext("2d"), w, h);
    const t = new CanvasTexture(c);
    t.wrapS = t.wrapT = RepeatWrapping;
    t.repeat.set(repeat[0], repeat[1]);
    t.anisotropy = 4;
    return t;
  };
  // Leather grain: soft noise.
  const grain = canvasTexture(256, 256, (g, w, h) => {
    const img = g.createImageData(w, h);
    for (let i = 0; i < img.data.length; i += 4) {
      const v = 120 + Math.random() * 70;
      img.data[i] = img.data[i + 1] = img.data[i + 2] = v;
      img.data[i + 3] = 255;
    }
    g.putImageData(img, 0, 0);
    g.filter = "blur(1px)";
    g.drawImage(g.canvas, 0, 0);
  }, [6, 6]);
  // Brushed metal: fine streaks one way.
  const brushed = canvasTexture(512, 64, (g, w, h) => {
    g.fillStyle = "#808080";
    g.fillRect(0, 0, w, h);
    for (let i = 0; i < 900; i++) {
      const y = Math.random() * h;
      g.strokeStyle = `rgba(${Math.random() < 0.5 ? "255,255,255" : "0,0,0"},${Math.random() * 0.18})`;
      g.lineWidth = Math.random() * 0.8 + 0.2;
      g.beginPath();
      g.moveTo(0, y);
      g.lineTo(w, y + (Math.random() - 0.5) * 2);
      g.stroke();
    }
  });
  // Speaker fabric inside the cups: a fine mesh of dots.
  const mesh = canvasTexture(128, 128, (g, w, h) => {
    g.fillStyle = "#1b1a20";
    g.fillRect(0, 0, w, h);
    g.fillStyle = "#0b0b0e";
    for (let y = 0; y < h; y += 4) for (let x = (y / 4) % 2 ? 2 : 0; x < w; x += 4) g.fillRect(x, y, 2, 2);
  }, [4, 4]);

  // ---------------------------------------------------------------- materials
  const plastic = new MeshPhysicalMaterial({ color: 0x1a1920, roughness: 0.55, metalness: 0, clearcoat: 0.35, clearcoatRoughness: 0.4, sheen: 0.2, sheenColor: new Color(0x6b6880) });
  const alu = new MeshPhysicalMaterial({ color: 0xb9b6c4, metalness: 1, roughness: 0.32, roughnessMap: brushed, anisotropy: 0.8, clearcoat: 0.2 });
  const steel = new MeshStandardMaterial({ color: 0xcfcdd8, metalness: 1, roughness: 0.25, roughnessMap: brushed });
  const leather = new MeshPhysicalMaterial({ color: 0x24212c, roughness: 0.62, bumpMap: grain, bumpScale: 0.6, sheen: 0.6, sheenColor: new Color(0x8b7fb8), sheenRoughness: 0.5, clearcoat: 0.15, clearcoatRoughness: 0.6 });
  const bandLeather = leather.clone();
  bandLeather.color = new Color(0x2a2240);
  const fabric = new MeshStandardMaterial({ color: 0xffffff, map: mesh, roughness: 0.95 });
  const glow = new MeshStandardMaterial({ color: 0x7c3aed, emissive: new Color(0x7c3aed), emissiveIntensity: 0.8, roughness: 0.4 });

  const phones = new Group();

  // ---------------------------------------------------------------- headband
  const arch = (r, h, from = 0, to = 1, n = 64) => {
    const pts = [];
    for (let i = 0; i <= n; i++) {
      const a = Math.PI * (from + (to - from) * (i / n));
      pts.push(new Vector3(-Math.cos(a) * r, Math.sin(a) * h, 0));
    }
    return new CatmullRomCurve3(pts);
  };
  // A rounded rectangle (x: thickness toward the head, y: width front to back).
  const roundRect = (w, t, r) => {
    const sh = new Shape();
    const x = -t / 2;
    const y = -w / 2;
    sh.moveTo(x + r, y);
    sh.lineTo(x + t - r, y);
    sh.quadraticCurveTo(x + t, y, x + t, y + r);
    sh.lineTo(x + t, y + w - r);
    sh.quadraticCurveTo(x + t, y + w, x + t - r, y + w);
    sh.lineTo(x + r, y + w);
    sh.quadraticCurveTo(x, y + w, x, y + w - r);
    sh.lineTo(x, y + r);
    sh.quadraticCurveTo(x, y, x + r, y);
    return sh;
  };
  // The outer band: flat, covered in soft-touch plastic.
  const band = new Mesh(new ExtrudeGeometry(roundRect(0.3, 0.075, 0.03), { steps: 160, bevelEnabled: false, extrudePath: arch(1.3, 1.62, 0.04, 0.96) }), plastic);
  band.position.y = 0.6;
  phones.add(band);
  // The padded strip under it, in leather.
  const padding = new Mesh(new ExtrudeGeometry(roundRect(0.26, 0.11, 0.05), { steps: 140, bevelEnabled: false, extrudePath: arch(1.19, 1.5, 0.16, 0.84) }), bandLeather);
  padding.position.y = 0.6;
  phones.add(padding);

  // ---------------------------------------------------------------- ear cups
  const glowRings = [];
  const leds = [];
  const CUP_X = 1.3;
  for (const side of [-1, 1]) {
    // Slider: a flat steel strip coming out of the band, with marks for the sizes.
    const slider = new Mesh(new BoxGeometry(0.035, 0.5, 0.16), steel);
    slider.position.set(side * CUP_X, 0.98, 0);
    phones.add(slider);
    for (let m = 0; m < 4; m++) {
      const mark = new Mesh(new BoxGeometry(0.037, 0.008, 0.11), plastic);
      mark.position.set(side * CUP_X, 0.88 + m * 0.07, 0);
      phones.add(mark);
    }

    // Fork: holds the cup at its front and back, with a screw at each side.
    const forkPath = new CatmullRomCurve3(
      Array.from({ length: 33 }, (_, k) => {
        const a = Math.PI * (k / 32);
        return new Vector3(0, Math.sin(a) * 0.78 + 0.04, -Math.cos(a) * 0.7);
      }),
    );
    const fork = new Mesh(new ExtrudeGeometry(roundRect(0.06, 0.05, 0.02), { steps: 64, bevelEnabled: false, extrudePath: forkPath }), alu);
    fork.position.set(side * (CUP_X + 0.02), 0, 0);
    phones.add(fork);
    for (const z of [-1, 1]) {
      const screw = new Mesh(new CylinderGeometry(0.045, 0.045, 0.08, 24), steel);
      screw.rotation.x = Math.PI / 2;
      screw.position.set(side * (CUP_X + 0.02), 0.04, z * 0.7);
      phones.add(screw);
    }

    const cup = new Group();
    cup.position.set(side * CUP_X, 0.02, 0);
    cup.rotation.z = (side * Math.PI) / 2; // faces point inward (along x)
    cup.scale.set(1.16, 1, 1); // a little taller than wide, like real ear cups

    // The shell: a rounded, slightly tapered cup turned around its axis.
    const R = 0.6;
    const H = 0.4;
    const profile = [new Vector2(0, -H / 2 - 0.02)];
    for (let k = 0; k <= 14; k++) {
      const a = (k / 14) * (Math.PI / 2);
      profile.push(new Vector2(R - 0.1 + Math.sin(a) * 0.1, -H / 2 + (1 - Math.cos(a)) * 0.1 - 0.02));
    }
    profile.push(new Vector2(R + 0.015, H / 2 - 0.04), new Vector2(R - 0.02, H / 2), new Vector2(0, H / 2));
    cup.add(new Mesh(new LatheGeometry(profile, 96), plastic));

    // Outer cap: brushed aluminium, a thin purple ring and a small light.
    const cap = new Mesh(new CylinderGeometry(0.44, 0.46, 0.035, 96), alu);
    cap.position.y = -H / 2 - 0.035;
    cup.add(cap);
    const ring = new Mesh(new TorusGeometry(0.48, 0.012, 12, 120), glow);
    ring.rotation.x = Math.PI / 2;
    ring.position.y = -H / 2 - 0.03;
    cup.add(ring);
    glowRings.push(ring);
    const led = new Mesh(new SphereGeometry(0.018, 16, 12), glow.clone());
    led.position.set(0.42, -H / 2 + 0.06, 0.38);
    cup.add(led);
    leds.push(led);

    // Cushion: plump leather, with speaker fabric in the middle.
    const cushionProfile = [];
    for (let k = 0; k <= 24; k++) {
      const a = Math.PI * (k / 24);
      cushionProfile.push(new Vector2(0.44 + Math.sin(a) * 0.17 - (a > Math.PI / 2 ? 0.03 : 0), -Math.cos(a) * 0.13));
    }
    const cushion = new Mesh(new LatheGeometry(cushionProfile, 96), leather);
    cushion.position.y = H / 2 + 0.1;
    cup.add(cushion);
    const inside = new Mesh(new CircleGeometry(0.44, 64), fabric);
    inside.rotation.x = -Math.PI / 2;
    inside.position.y = H / 2 + 0.005;
    cup.add(inside);

    phones.add(cup);
  }

  // A soft shadow underneath, so they sit in the scene.
  const shadowTex = canvasTexture(128, 128, (g, w, h) => {
    const grad = g.createRadialGradient(w / 2, h / 2, 0, w / 2, h / 2, w / 2);
    grad.addColorStop(0, "rgba(0,0,0,.55)");
    grad.addColorStop(0.6, "rgba(0,0,0,.18)");
    grad.addColorStop(1, "rgba(0,0,0,0)");
    g.fillStyle = grad;
    g.fillRect(0, 0, w, h);
  });
  const shadow = new Mesh(new PlaneGeometry(4.2, 1.6), new MeshBasicMaterial({ map: shadowTex, transparent: true, depthWrite: false }));
  shadow.rotation.x = -Math.PI / 2;
  shadow.position.y = -1.25;
  scene.add(shadow);

  phones.rotation.set(0.1, -0.5, 0);
  phones.position.y = -0.35;
  scene.add(phones);

  // ---------------------------------------------------------------- motion
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const rot = { x: 0.1, y: -0.5, vx: 0, vy: 0 };
  const target = { x: 0.1, y: -0.5 };
  let spin = 0; // extra turn from dragging, with momentum
  let spinV = 0;
  let pulse = 0; // 0..1 after a click
  let dragging = null;
  let visible = false;
  let raf = 0;
  let last = performance.now();

  const onMove = (e) => {
    const r = canvas.getBoundingClientRect();
    // Where the cursor is, relative to the headphones (works across the whole section).
    const nx = Math.max(-1.5, Math.min(1.5, (e.clientX - (r.left + r.width / 2)) / (r.width * 0.9)));
    const ny = Math.max(-1.5, Math.min(1.5, (e.clientY - (r.top + r.height / 2)) / (r.height * 0.9)));
    target.y = -0.5 + nx * 0.75;
    target.x = 0.1 + ny * 0.35;
    if (dragging) {
      const dx = e.clientX - dragging.x;
      dragging.x = e.clientX;
      spin += dx * 0.012;
      spinV = dx * 0.012;
      dragging.moved += Math.abs(dx);
    }
    wake();
  };
  const onDown = (e) => {
    dragging = { x: e.clientX, moved: 0 };
    canvas.setPointerCapture?.(e.pointerId);
    canvas.style.cursor = "grabbing";
    wake();
  };
  const onUp = () => {
    if (dragging && dragging.moved < 4) pulse = 1; // a click, not a drag
    dragging = null;
    canvas.style.cursor = "";
    wake();
  };
  const area = canvas.closest(".box") || canvas;
  if (!still) {
    area.addEventListener("pointermove", onMove);
    canvas.addEventListener("pointerdown", onDown);
    window.addEventListener("pointerup", onUp);
  }

  function resize() {
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (!w || !h) return;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  }
  new ResizeObserver(() => {
    resize();
    if (still) draw(0);
    else wake();
  }).observe(canvas);

  function draw(t) {
    // Spring toward where the cursor points; the drag adds its own turn.
    const k = 0.06;
    const d = 0.82;
    rot.vx = (rot.vx + (target.x - rot.x) * k) * d;
    rot.vy = (rot.vy + (target.y - rot.y) * k) * d;
    rot.x += rot.vx;
    rot.y += rot.vy;
    if (!dragging) {
      spinV *= 0.94;
      spin += spinV;
      // Ease back to facing the viewer.
      spin += (0 - spin) * 0.02;
    }
    pulse *= 0.93;
    const float = still ? 0 : Math.sin(t / 900) * 0.06;
    phones.rotation.x = rot.x + Math.sin(t / 1300) * (still ? 0 : 0.02);
    phones.rotation.y = rot.y + spin;
    phones.position.y = -0.35 + float;
    const s = 1 + pulse * 0.06;
    phones.scale.setScalar(s);
    glowRings[0].material.emissiveIntensity = 0.8 + pulse * 3.5;
    for (const led of leds) led.material.emissiveIntensity = 1.5 + Math.sin(t / 420) * 0.8 + pulse * 4;
    rim.intensity = 28 + pulse * 50;
    shadow.material.opacity = 1 - float * 2.5;
    shadow.scale.setScalar(1 - float * 0.6);
    renderer.render(scene, camera);
  }

  // Draws while something is moving; stops once everything has settled.
  function frame(now) {
    raf = 0;
    if (!visible || document.hidden) return;
    draw(now);
    last = now;
    const moving =
      dragging || Math.abs(rot.vx) + Math.abs(rot.vy) > 1e-4 || Math.abs(spinV) > 1e-4 || Math.abs(spin) > 1e-3 || pulse > 0.01;
    // The gentle float keeps it alive while it's on screen (cheap: one small scene).
    if (moving || !still) raf = requestAnimationFrame(frame);
  }
  function wake() {
    if (!raf && visible && !still) raf = requestAnimationFrame(frame);
  }
  new IntersectionObserver((es) => {
    visible = es[0].isIntersecting;
    if (visible) {
      resize();
      if (still) draw(0);
      else wake();
    }
  }).observe(canvas);
  document.addEventListener("visibilitychange", wake);

  resize();
  draw(last);
  return { draw };
}
