// The headphones at the top of the page: an original pair of over-ear headphones, built
// from shapes in code (no model file), lit like a studio product shot. They lean toward
// the cursor, can be dragged around to look at any side (and keep turning for a moment
// when let go), and float gently when left alone.
// Bundled with three.js into ../headphones.js: see 3d/README.md.
//
// Draws only while on screen and the tab is visible. With "reduce motion" on, nothing
// moves by itself: they only turn while being dragged.

import {
  ACESFilmicToneMapping, BackSide, BoxGeometry, CanvasTexture, CapsuleGeometry, CatmullRomCurve3, CircleGeometry, Color, CylinderGeometry,
  DirectionalLight, ExtrudeGeometry, Group, HemisphereLight, LatheGeometry, Mesh, MeshBasicMaterial, MeshPhysicalMaterial, MeshStandardMaterial,
  PCFSoftShadowMap, PMREMGenerator, PerspectiveCamera, PlaneGeometry, PointLight, RepeatWrapping, SRGBColorSpace, Scene, Shape, SphereGeometry,
  TorusGeometry, Vector2, Vector3, WebGLRenderer,
} from "three";
import { RoundedBoxGeometry } from "three/examples/jsm/geometries/RoundedBoxGeometry.js";

// A photo studio to reflect: a dark room with a few soft lights. Black products get their
// shape from what they reflect, so this matters more than the lamps below.
function studio() {
  const room = new Scene();
  room.add(new Mesh(new BoxGeometry(30, 30, 30), new MeshBasicMaterial({ color: 0x07070b, side: BackSide })));
  const panel = (color, power, w, h, pos) => {
    const m = new Mesh(new PlaneGeometry(w, h), new MeshBasicMaterial({ color: new Color(color).multiplyScalar(power) }));
    m.position.set(pos[0], pos[1], pos[2]);
    m.lookAt(0, 0, 0);
    room.add(m);
  };
  panel(0xffffff, 9, 9, 5, [0, 11, 3]); // big soft light overhead
  panel(0xfff4ea, 7, 4, 8, [-10, 3, 6]); // key, front left
  panel(0xe8e6ff, 3.6, 5, 6, [9, 0, 8]); // weak fill, front right
  panel(0x8b5cf6, 8, 2.5, 9, [9, 3, -8]); // purple strip, back right
  panel(0x6d5df6, 5, 2.5, 9, [-10, 1, -7]); // violet strip, back left
  panel(0xffffff, 3, 8, 1.2, [0, -9, 4]); // a little bounce from the floor
  return room;
}

export function mount(canvas) {
  let renderer;
  try {
    renderer = new WebGLRenderer({ canvas, antialias: true, alpha: true, preserveDrawingBuffer: false });
  } catch (e) {
    return null; // no WebGL: the page keeps its still picture
  }
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.outputColorSpace = SRGBColorSpace;
  renderer.toneMapping = ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.15;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = PCFSoftShadowMap;

  const scene = new Scene();
  const pmrem = new PMREMGenerator(renderer);
  scene.environment = pmrem.fromScene(studio(), 0.02).texture;
  scene.environmentIntensity = 1.0;
  pmrem.dispose();

  const FOV = 30;
  const camera = new PerspectiveCamera(FOV, 1, 0.1, 60);

  // Lamps: a white key from the upper left that casts the shadows, purple from behind on
  // the right, violet from behind on the left.
  scene.add(new HemisphereLight(0xbdb8ff, 0x0a0a10, 0.35));
  const key = new DirectionalLight(0xfff6ee, 2.2);
  key.position.set(-3.5, 5, 5.5);
  key.castShadow = true;
  key.shadow.mapSize.set(1024, 1024);
  key.shadow.camera.left = key.shadow.camera.bottom = -2.6;
  key.shadow.camera.right = key.shadow.camera.top = 2.6;
  key.shadow.camera.near = 2;
  key.shadow.camera.far = 16;
  key.shadow.bias = -0.002;
  key.shadow.normalBias = 0.06;
  key.shadow.radius = 5;
  scene.add(key);
  const RIM = 26;
  const rim = new PointLight(0x8b5cf6, RIM, 14);
  rim.position.set(3.2, 1.2, -2.6);
  scene.add(rim);
  const back = new PointLight(0x6d5df6, 14, 14);
  back.position.set(-3.4, 0.4, -2.4);
  scene.add(back);

  // ---------------------------------------------------------------- textures (drawn in code)
  const canvasTexture = (w, h, paint, repeat = [1, 1]) => {
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    paint(c.getContext("2d"), w, h);
    const t = new CanvasTexture(c);
    t.wrapS = t.wrapT = RepeatWrapping;
    t.repeat.set(repeat[0], repeat[1]);
    t.anisotropy = Math.min(8, renderer.capabilities.getMaxAnisotropy());
    return t;
  };
  const noise = (blur, lo, hi) => (g, w, h) => {
    const img = g.createImageData(w, h);
    for (let i = 0; i < img.data.length; i += 4) {
      const v = lo + Math.random() * (hi - lo);
      img.data[i] = img.data[i + 1] = img.data[i + 2] = v;
      img.data[i + 3] = 255;
    }
    g.putImageData(img, 0, 0);
    if (blur) {
      g.filter = `blur(${blur}px)`;
      g.drawImage(g.canvas, 0, 0);
    }
  };
  // Leather: pebbled grain (soft blobs with fine noise on top).
  const grain = canvasTexture(256, 256, (g, w, h) => {
    noise(0, 96, 150)(g, w, h);
    g.filter = "blur(0.6px)";
    g.drawImage(g.canvas, 0, 0);
    g.filter = "none";
    for (let i = 0; i < 1500; i++) {
      const x = Math.random() * w;
      const y = Math.random() * h;
      const r = 2 + Math.random() * 3.5;
      const grad = g.createRadialGradient(x, y, 0, x, y, r);
      grad.addColorStop(0, "rgba(255,255,255,.22)");
      grad.addColorStop(1, "rgba(255,255,255,0)");
      g.fillStyle = grad;
      g.fillRect(x - r, y - r, r * 2, r * 2);
    }
  }, [5, 5]);
  // Soft-touch plastic: a very fine speckle, so highlights aren't mirror-smooth.
  const speckle = canvasTexture(256, 256, noise(0, 110, 146), [10, 10]);
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
  // Speaker cloth inside the cups: a fine weave of dots.
  const weave = canvasTexture(128, 128, (g, w, h) => {
    g.fillStyle = "#17161b";
    g.fillRect(0, 0, w, h);
    g.fillStyle = "#08080a";
    for (let y = 0; y < h; y += 4) for (let x = (y / 4) % 2 ? 2 : 0; x < w; x += 4) g.fillRect(x, y, 2, 2);
  }, [5, 5]);

  // ---------------------------------------------------------------- materials
  const shell = new MeshPhysicalMaterial({ color: 0x17171b, roughness: 0.58, roughnessMap: speckle, bumpMap: speckle, bumpScale: 0.08, metalness: 0, sheen: 0.35, sheenColor: new Color(0x6f6c86), sheenRoughness: 0.6 });
  const satin = new MeshPhysicalMaterial({ color: 0x2b2a33, roughness: 0.34, metalness: 0.3, clearcoat: 0.7, clearcoatRoughness: 0.25 });
  const gunmetal = new MeshPhysicalMaterial({ color: 0x44434d, metalness: 1, roughness: 0.36, roughnessMap: brushed, anisotropy: 0.6, clearcoat: 0.3, clearcoatRoughness: 0.2 });
  const steel = new MeshStandardMaterial({ color: 0xd7d5df, metalness: 1, roughness: 0.2, roughnessMap: brushed });
  const leather = new MeshPhysicalMaterial({ color: 0x131216, roughness: 0.5, bumpMap: grain, bumpScale: 0.9, sheen: 0.5, sheenColor: new Color(0x7d76a6), sheenRoughness: 0.45, clearcoat: 0.25, clearcoatRoughness: 0.55 });
  const cloth = new MeshStandardMaterial({ color: 0xffffff, map: weave, roughness: 1 });
  const dark = new MeshStandardMaterial({ color: 0x050506, roughness: 0.9 });
  const light = new MeshStandardMaterial({ color: 0x1a1030, emissive: new Color(0xa78bfa), emissiveIntensity: 2.2, roughness: 0.4 });

  const phones = new Group();
  const add = (geometry, material, parent = phones) => {
    const m = new Mesh(geometry, material);
    m.castShadow = true;
    m.receiveShadow = true;
    parent.add(m);
    return m;
  };

  // ---------------------------------------------------------------- headband
  const BAND_Y = 0.85;
  const archPoint = (r, h, u) => new Vector3(-Math.cos(Math.PI * u) * r, Math.sin(Math.PI * u) * h + BAND_Y, 0);
  const arch = (r, h, from, to, n = 72) => new CatmullRomCurve3(Array.from({ length: n + 1 }, (_, i) => archPoint(r, h, from + (to - from) * (i / n))));
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
  const along = (shape, path, steps) => new ExtrudeGeometry(shape, { steps, bevelEnabled: false, extrudePath: path, curveSegments: 10 });
  // Shapes swept along the arch get stretched texture coordinates; lay them out again by
  // real distance so grain stays even. With "taper", the ends are rounded off to a point.
  const dress = (geo, r, h, taper) => {
    const pos = geo.attributes.position;
    const uv = geo.attributes.uv;
    const v = new Vector3();
    for (let i = 0; i < pos.count; i++) {
      v.fromBufferAttribute(pos, i);
      const u = Math.atan2((v.y - BAND_Y) / h, -v.x / r) / Math.PI;
      const c = archPoint(r, h, u);
      if (taper) {
        const t = Math.max(0, Math.min(1, (u - taper[0]) / 0.075, (taper[1] - u) / 0.075));
        v.sub(c).multiplyScalar(Math.sqrt(1 - (1 - t) * (1 - t))).add(c);
        pos.setXYZ(i, v.x, v.y, v.z);
      }
      uv.setXY(i, u * 4.4, v.z + Math.hypot(v.x - c.x, v.y - c.y));
    }
    geo.computeVertexNormals();
    return geo;
  };
  // The outer band: flat and wide, soft-touch.
  add(dress(along(roundRect(0.36, 0.07, 0.032), arch(1.3, 1.62, 0.055, 0.945), 180), 1.3, 1.62), shell);
  // The padded strip under it, in leather, rounded off at both ends.
  add(dress(along(roundRect(0.31, 0.16, 0.075), arch(1.195, 1.495, 0.17, 0.83), 170), 1.195, 1.495, [0.17, 0.83]), leather);
  // A thin metal inlay along the top of the band.
  add(along(roundRect(0.05, 0.012, 0.005), arch(1.337, 1.657, 0.3, 0.7), 80), gunmetal);

  // ---------------------------------------------------------------- arms and ear cups
  const CUP_X = 1.3;
  const CUP_Y = 0;
  const TALL = 1.16; // cups are a little taller than wide
  const R = 0.61;
  const H = 0.42;
  const FORK_R = 0.71;
  const FORK_TOP = 0.84;

  for (const side of [-1, 1]) {
    // Where the band ends: a sleeve the slider goes into.
    const sleeve = add(new RoundedBoxGeometry(0.115, 0.44, 0.39, 6, 0.055), shell);
    sleeve.position.set(side * 1.287, 1.19, 0);
    const trim = add(new RoundedBoxGeometry(0.119, 0.02, 0.394, 2, 0.008), gunmetal);
    trim.position.set(side * 1.287, 1.02, 0);
    // Slider: a flat steel strip with marks for the sizes.
    const slider = add(new BoxGeometry(0.03, 0.42, 0.2), steel);
    slider.position.set(side * 1.3, 1.0, 0);
    for (let m = 0; m < 3; m++) {
      const mark = add(new BoxGeometry(0.032, 0.006, 0.12), dark);
      mark.position.set(side * 1.3, 0.91 + m * 0.035, 0);
    }
    // Hub: joins the slider to the fork.
    const hub = add(new RoundedBoxGeometry(0.12, 0.13, 0.3, 5, 0.04), gunmetal);
    hub.position.set(side * 1.31, FORK_TOP + 0.02, 0);

    // Fork: arches over the cup and holds it at the front and back.
    const forkPath = new CatmullRomCurve3(
      Array.from({ length: 41 }, (_, k) => {
        const a = Math.PI * (k / 40);
        return new Vector3(0, Math.sin(a) * FORK_TOP + 0.02, -Math.cos(a) * FORK_R);
      }),
    );
    const fork = add(along(roundRect(0.14, 0.085, 0.034), forkPath, 96), gunmetal);
    fork.position.set(side * 1.31, CUP_Y, 0);
    for (const z of [-1, 1]) {
      const pivot = add(new CylinderGeometry(0.075, 0.075, 0.07, 32), gunmetal);
      pivot.rotation.z = Math.PI / 2;
      pivot.position.set(side * 1.31, CUP_Y + 0.02, z * FORK_R);
      const screw = add(new CylinderGeometry(0.032, 0.032, 0.076, 20), steel);
      screw.rotation.z = Math.PI / 2;
      screw.position.copy(pivot.position);
    }

    // The cup. Built standing up (y is its axis: -y is the outside, +y is the ear side),
    // then turned to face inward.
    const cup = new Group();
    cup.position.set(side * CUP_X, CUP_Y, 0);
    cup.rotation.z = (side * Math.PI) / 2;
    cup.scale.set(TALL, 1, 1);

    // Shell: a gently domed outer face, a rounded shoulder, straight sides.
    const profile = [];
    const CAP = 0.5;
    for (let k = 0; k <= 18; k++) {
      const a = (k / 18) * (Math.PI / 2);
      profile.push(new Vector2(Math.sin(a) * CAP, -H / 2 - Math.cos(a) * 0.075));
    }
    for (let k = 1; k <= 14; k++) {
      const a = (k / 14) * (Math.PI / 2);
      profile.push(new Vector2(CAP + Math.sin(a) * (R - CAP), -H / 2 + (1 - Math.cos(a)) * 0.13));
    }
    for (let k = 0; k <= 8; k++) {
      const a = (k / 8) * (Math.PI / 2);
      profile.push(new Vector2(R - (1 - Math.cos(a)) * 0.05, H / 2 - 0.05 + Math.sin(a) * 0.05));
    }
    profile.push(new Vector2(0, H / 2));
    add(new LatheGeometry(profile, 128), shell, cup);
    // A polished ring where the face meets the shoulder, and a glossy disc in the middle.
    const ring = add(new TorusGeometry(CAP, 0.011, 16, 160), gunmetal, cup);
    ring.rotation.x = Math.PI / 2;
    ring.position.y = -H / 2 - 0.003;
    const disc = add(new CylinderGeometry(0.2, 0.2, 0.012, 96), satin, cup);
    disc.position.y = -H / 2 - 0.071;
    const discRing = add(new TorusGeometry(0.2, 0.006, 12, 96), gunmetal, cup);
    discRing.rotation.x = Math.PI / 2;
    discRing.position.y = -H / 2 - 0.068;

    // Ear side: a base plate, the cushion with its seam, and cloth over the speaker.
    const plate = add(new CylinderGeometry(R - 0.035, R - 0.035, 0.035, 128), dark, cup);
    plate.position.y = H / 2 + 0.012;
    const cushionProfile = [];
    for (let k = 0; k <= 48; k++) {
      // A full, slightly squared-off ring: up the outside, back down the inside.
      const a = -Math.PI / 2 + Math.PI * 2 * (k / 48);
      const sq = (x) => Math.sign(x) * Math.pow(Math.abs(x), 0.8);
      cushionProfile.push(new Vector2(0.512 + sq(Math.cos(a)) * 0.108, sq(Math.sin(a)) * 0.16));
    }
    const cushion = add(new LatheGeometry(cushionProfile, 128), leather, cup);
    cushion.position.y = H / 2 + 0.18;
    const seam = add(new TorusGeometry(0.619, 0.006, 8, 160), dark, cup);
    seam.rotation.x = Math.PI / 2;
    seam.position.y = H / 2 + 0.18;
    const inside = add(new CircleGeometry(0.43, 64), cloth, cup);
    inside.rotation.x = -Math.PI / 2;
    inside.position.y = H / 2 + 0.05;
    phones.add(cup);

    // Small things on the rim, placed by their angle from the bottom (toward the back).
    const onRim = (mesh, angle, across = 0) => {
      const n = new Vector3(0, -Math.cos(angle) / TALL, -Math.sin(angle)).normalize();
      mesh.position.set(side * (CUP_X + across), CUP_Y - Math.cos(angle) * R * TALL, -Math.sin(angle) * R);
      mesh.quaternion.setFromUnitVectors(new Vector3(0, 1, 0), n);
      return mesh;
    };
    if (side > 0) {
      onRim(add(new CylinderGeometry(0.04, 0.04, 0.03, 24), gunmetal), 0.55); // power
      onRim(add(new SphereGeometry(0.012, 12, 8), light), 0.33); // status light
      const port = onRim(add(new RoundedBoxGeometry(0.05, 0.012, 0.11, 3, 0.005), dark), -0.3); // charging port
      port.rotateY(Math.PI / 2);
    } else {
      const rocker = onRim(add(new CapsuleGeometry(0.026, 0.13, 6, 16), gunmetal), 0.6); // volume
      rocker.rotateX(Math.PI / 2);
    }
    // Microphone slots near the top front.
    for (const a of [Math.PI - 0.5, Math.PI - 0.62]) {
      onRim(add(new RoundedBoxGeometry(0.07, 0.008, 0.018, 2, 0.004), dark), -a, 0.03);
    }
  }

  // Centre the whole thing on the origin.
  const model = new Group();
  phones.position.y = -0.9;
  model.add(phones);
  const REST_X = 0.1;
  const REST_Y = -0.62;
  scene.add(model);

  // A soft shadow on the "floor" underneath.
  const shadowTex = canvasTexture(128, 128, (g, w, h) => {
    const grad = g.createRadialGradient(w / 2, h / 2, 0, w / 2, h / 2, w / 2);
    grad.addColorStop(0, "rgba(0,0,0,.6)");
    grad.addColorStop(0.55, "rgba(0,0,0,.2)");
    grad.addColorStop(1, "rgba(0,0,0,0)");
    g.fillStyle = grad;
    g.fillRect(0, 0, w, h);
  });
  const shadow = new Mesh(new PlaneGeometry(4.4, 1.5), new MeshBasicMaterial({ map: shadowTex, transparent: true, depthWrite: false }));
  shadow.rotation.x = -Math.PI / 2;
  shadow.position.y = -2.0;
  scene.add(shadow);

  // ---------------------------------------------------------------- size
  const fit = () => {
    const w = canvas.clientWidth || 1;
    const h = canvas.clientHeight || 1;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    // Far enough back that the model fits both ways.
    const half = 2.15;
    let d = half / Math.tan((FOV * Math.PI) / 360);
    if (camera.aspect < 1) d /= camera.aspect;
    camera.position.set(0, 0.05, d);
    camera.lookAt(0, -0.08, 0);
    camera.updateProjectionMatrix();
  };

  // ---------------------------------------------------------------- movement
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const area = canvas.closest("section") || canvas;
  let followX = 0, followY = 0, wantX = 0, wantY = 0; // where the cursor is, -1..1
  let hover = 0, wantHover = 0;
  let spinY = 0, spinX = 0, velY = 0, velX = 0; // what dragging has added
  let dragging = false, lastX = 0, lastY = 0, lastMove = 0, idleSince = 0;
  let visible = true, running = false, last = 0, clock = 0;

  const draw = () => {
    const bob = still ? 0 : Math.sin(clock * 0.9) * 0.045;
    const sway = still ? 0 : Math.sin(clock * 0.45) * 0.06;
    model.rotation.set(REST_X + spinX + followY * 0.16, REST_Y + spinY + followX * 0.34 + sway, 0);
    model.position.set(followX * 0.1, bob - followY * 0.05, 0);
    model.scale.setScalar(1 + hover * 0.025);
    rim.intensity = RIM * (1 + hover * 0.7);
    rim.position.x = 3.2 - followX * 1.2;
    back.position.y = 0.4 + followY * 0.8;
    light.emissiveIntensity = 2.2 + (still ? 0 : Math.sin(clock * 2.2) * 0.7) + hover * 1.5;
    shadow.material.opacity = 0.9 - bob * 3;
    shadow.scale.setScalar(1 - bob * 0.8);
    shadow.position.x = followX * 0.1;
    renderer.render(scene, camera);
  };

  const settle = (v, to, rate, dt) => v + (to - v) * (1 - Math.exp(-rate * dt));
  const frame = (now) => {
    if (!running) return;
    const dt = Math.min((now - last) / 1000, 0.05);
    last = now;
    clock += dt;
    followX = settle(followX, wantX, 4, dt);
    followY = settle(followY, wantY, 4, dt);
    hover = settle(hover, wantHover, 6, dt);
    if (!dragging) {
      // Keep turning for a moment, then come back round to face the front.
      spinY += velY * dt;
      spinX += velX * dt;
      velY *= Math.exp(-2.4 * dt);
      velX *= Math.exp(-4 * dt);
      spinX = settle(spinX, 0, 2.5, dt);
      if (now - idleSince > 1600 && Math.abs(velY) < 0.4) {
        const home = Math.round(spinY / (Math.PI * 2)) * Math.PI * 2;
        spinY = settle(spinY, home, 1.6, dt);
      }
    }
    draw();
    const moving = dragging || Math.abs(velY) > 0.002 || Math.abs(spinX) > 0.002 || Math.abs(spinY - Math.round(spinY / (Math.PI * 2)) * Math.PI * 2) > 0.002;
    if (still && !moving) {
      running = false;
      return;
    }
    requestAnimationFrame(frame);
  };
  const start = () => {
    if (running || !visible || document.hidden) return;
    running = true;
    last = performance.now();
    requestAnimationFrame(frame);
  };
  const stop = () => {
    running = false;
  };

  if (!still) {
    window.addEventListener("pointermove", (e) => {
      if (e.pointerType !== "mouse" || !visible) return;
      const r = area.getBoundingClientRect();
      wantX = Math.max(-1, Math.min(1, ((e.clientX - r.left) / r.width) * 2 - 1));
      wantY = Math.max(-1, Math.min(1, ((e.clientY - r.top) / r.height) * 2 - 1));
    }, { passive: true });
    document.documentElement.addEventListener("pointerleave", () => {
      wantX = wantY = 0;
    });
  }
  canvas.addEventListener("pointerenter", (e) => {
    if (e.pointerType === "mouse") wantHover = 1;
  });
  canvas.addEventListener("pointerleave", () => {
    wantHover = 0;
  });
  canvas.addEventListener("pointerdown", (e) => {
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    lastMove = performance.now();
    velY = velX = 0;
    canvas.setPointerCapture(e.pointerId);
    canvas.classList.add("grabbing");
    start();
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const now = performance.now();
    const dt = Math.max((now - lastMove) / 1000, 0.008);
    const dx = (e.clientX - lastX) * 0.0085;
    const dy = (e.clientY - lastY) * 0.005;
    spinY += dx;
    spinX = Math.max(-0.7, Math.min(0.7, spinX + dy));
    velY = velY * 0.5 + (dx / dt) * 0.5;
    velX = 0;
    lastX = e.clientX;
    lastY = e.clientY;
    lastMove = now;
  });
  const release = () => {
    if (!dragging) return;
    dragging = false;
    idleSince = performance.now();
    if (idleSince - lastMove > 90) velY = 0; // held still before letting go: no throw
    velY = Math.max(-9, Math.min(9, velY));
    canvas.classList.remove("grabbing");
  };
  canvas.addEventListener("pointerup", release);
  canvas.addEventListener("pointercancel", release);

  new ResizeObserver(() => {
    fit();
    if (!running) draw();
  }).observe(canvas);
  new IntersectionObserver((es) => {
    visible = es[0].isIntersecting;
    if (visible && !still) start();
    else if (!visible) stop();
  }).observe(canvas);
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) stop();
    else if (!still) start();
  });
  canvas.addEventListener("webglcontextlost", (e) => {
    e.preventDefault();
    stop();
    canvas.classList.remove("ready"); // the still picture shows again
  });

  fit();
  draw();
  if (!still) start();
  // For making the still picture (see 3d/README.md): the resting pose as an image.
  return {
    snapshot(type = "image/png", quality) {
      clock = 0;
      followX = followY = hover = spinX = spinY = 0;
      draw();
      return canvas.toDataURL(type, quality);
    },
  };
}
