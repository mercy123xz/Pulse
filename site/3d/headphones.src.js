// A pair of headphones in 3D for the download section: they turn to follow the cursor,
// can be dragged to spin, and pulse when clicked. Built from simple shapes in code (no
// model file). Bundled with three.js into ../headphones.js: see 3d/README.md.
//
// Loaded only when the section comes into view; draws only while it's on screen and the
// tab is visible. With "reduce motion" on, it's drawn once and stays still.

import {
  ACESFilmicToneMapping, CatmullRomCurve3, Color, CylinderGeometry, DirectionalLight, Group, LatheGeometry, Mesh,
  MeshPhysicalMaterial, MeshStandardMaterial, PMREMGenerator, PerspectiveCamera, PointLight, Scene, SRGBColorSpace,
  TorusGeometry, TubeGeometry, Vector2, Vector3, WebGLRenderer,
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
  scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
  pmrem.dispose();

  const camera = new PerspectiveCamera(32, 1, 0.1, 50);
  camera.position.set(0, 0.15, 7.2);

  // Light: a soft key from the top left, Pulse purple from behind, a cool fill.
  const key = new DirectionalLight(0xffffff, 1.6);
  key.position.set(-3, 4, 5);
  scene.add(key);
  const rim = new PointLight(0x8b5cf6, 40, 12);
  rim.position.set(2.5, 1.5, -3);
  scene.add(rim);
  const fill = new PointLight(0x60a5fa, 12, 12);
  fill.position.set(-3, -2, 3);
  scene.add(fill);

  // Materials
  const shell = new MeshPhysicalMaterial({ color: 0x17151f, metalness: 0.35, roughness: 0.28, clearcoat: 1, clearcoatRoughness: 0.12 });
  const metal = new MeshStandardMaterial({ color: 0xd9d4ee, metalness: 1, roughness: 0.22 });
  const cushion = new MeshStandardMaterial({ color: 0x2b2636, roughness: 0.92, metalness: 0 });
  const glow = new MeshStandardMaterial({ color: 0x7c3aed, emissive: new Color(0x7c3aed), emissiveIntensity: 1.2, roughness: 0.35 });
  const pad = new MeshPhysicalMaterial({ color: 0x241f30, roughness: 0.75, sheen: 1, sheenColor: new Color(0x8b5cf6), sheenRoughness: 0.6 });

  const phones = new Group();

  // Headband: an arch from one ear cup to the other, with a padded strip underneath.
  const arch = (r, h, n = 48) => {
    const pts = [];
    for (let i = 0; i <= n; i++) {
      const a = Math.PI * (i / n);
      pts.push(new Vector3(-Math.cos(a) * r, Math.sin(a) * h, 0));
    }
    return new CatmullRomCurve3(pts);
  };
  const band = new Mesh(new TubeGeometry(arch(1.32, 1.62), 96, 0.075, 20), metal);
  band.position.y = 0.55;
  phones.add(band);
  const padded = new Mesh(new TubeGeometry(arch(1.02, 1.36, 40), 80, 0.13, 20), pad);
  padded.scale.z = 1.9;
  padded.position.y = 0.66;
  phones.add(padded);

  // Ear cups, each in a yoke on a short slider.
  const glowRings = [];
  for (const side of [-1, 1]) {
    const cup = new Group();
    cup.position.set(side * 1.32, 0, 0);
    cup.rotation.z = (side * Math.PI) / 2; // faces point inward (along x)

    // The shell: a rounded cylinder made by turning a profile around its axis.
    const profile = [];
    const R = 0.62;
    const H = 0.42;
    for (let i = 0; i <= 12; i++) {
      const a = (i / 12) * (Math.PI / 2);
      profile.push(new Vector2(Math.sin(a) * 0.12 + (R - 0.12), -H / 2 + (1 - Math.cos(a)) * 0.12 - 0.12));
    }
    profile.push(new Vector2(R, H / 2 - 0.06), new Vector2(R - 0.08, H / 2), new Vector2(0, H / 2));
    profile.unshift(new Vector2(0, -H / 2 - 0.12));
    const body = new Mesh(new LatheGeometry(profile, 64), shell);
    cup.add(body);

    // Purple ring on the outside face, and the cushion on the inside.
    const ring = new Mesh(new TorusGeometry(0.42, 0.028, 16, 80), glow);
    ring.rotation.x = Math.PI / 2;
    ring.position.y = -H / 2 - 0.125;
    cup.add(ring);
    glowRings.push(ring);
    const cap = new Mesh(new CylinderGeometry(0.36, 0.36, 0.02, 64), shell);
    cap.position.y = -H / 2 - 0.13;
    cup.add(cap);

    const ear = new Mesh(new TorusGeometry(0.47, 0.16, 24, 72), cushion);
    ear.rotation.x = Math.PI / 2;
    ear.position.y = H / 2 + 0.08;
    ear.scale.set(1, 1, 0.85);
    cup.add(ear);

    // Yoke: an arc of metal over the top of the cup, from front to back.
    const yoke = new Mesh(new TorusGeometry(0.72, 0.045, 14, 64, Math.PI), metal);
    yoke.rotation.y = Math.PI / 2;
    yoke.position.set(side * 1.32, 0, 0);
    phones.add(yoke);

    const slider = new Mesh(new CylinderGeometry(0.035, 0.035, 0.5, 16), metal);
    slider.position.set(side * 1.32, 0.62, 0);
    phones.add(slider);

    phones.add(cup);
  }

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
    for (const ring of glowRings) ring.material.emissiveIntensity = 1.2 + pulse * 3 + Math.sin(t / 500) * 0.15;
    rim.intensity = 40 + pulse * 60;
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
