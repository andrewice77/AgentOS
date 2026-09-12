import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import type { AvatarState } from "$lib/avatar";
import { avatarLog } from "$lib/avatarDebug";

export interface Buddy3DHandle {
  setState(state: AvatarState): void;
  /** Global screen pointer + avatar window rect (physical pixels). */
  setPointer(
    screenX: number,
    screenY: number,
    winX: number,
    winY: number,
    winW: number,
    winH: number,
  ): void;
  /** Gait: walking intensity and screen-space direction (+x right, +y down). */
  setGait(walking: boolean, dirX: number, dirY: number): void;
  /** Full-window opaque present target (roam overlay). Null = local wrap canvas. */
  setPresent(canvas: HTMLCanvasElement | null): void;
  resize(): void;
  dispose(): void;
}

export type Buddy3DFraming = "full" | "bust";

export interface Buddy3DOptions {
  state?: AvatarState;
  /** Follow the OS cursor even outside the webview. */
  live?: boolean;
  framing?: Buddy3DFraming;
  /** Fired on each 3D footfall while walking (dir in screen space). */
  onStep?: (dirX: number, dirY: number) => void;
  /** Stationary full-window canvas used while roaming. */
  presentCanvas?: HTMLCanvasElement | null;
  /** Per-pixel alpha. Linux WebKitGTK must stay false (clone trails). */
  glass?: boolean;
}

const MODEL_URL = "avatar/new/Jesus.glb";

let protoPromise: Promise<THREE.Group> | null = null;

function modelHref(): string {
  return new URL(MODEL_URL, document.baseURI).href;
}

function loadPrototype(): Promise<THREE.Group> {
  if (!protoPromise) {
    const loader = new GLTFLoader();
    protoPromise = loader.loadAsync(modelHref()).then((gltf) => gltf.scene);
  }
  return protoPromise;
}

function instantiateModel(proto: THREE.Group): THREE.Group {
  const instance = proto.clone(true);
  instance.traverse((obj) => {
    if (obj instanceof THREE.Mesh) {
      obj.geometry = obj.geometry.clone();
      if (Array.isArray(obj.material)) {
        obj.material = obj.material.map((m) => prepareMaterial(m.clone()));
      } else {
        obj.material = prepareMaterial(obj.material.clone());
      }
    }
  });
  // glTF faces +X; Three.js camera looks down -Z.
  instance.rotation.y = -Math.PI / 2;
  return instance;
}

function requireNamed(root: THREE.Object3D, name: string): THREE.Object3D {
  const obj = root.getObjectByName(name);
  if (!obj) throw new Error(`Avatar GLB: manca l'oggetto '${name}'`);
  return obj;
}

function makeEyePivot(mesh: THREE.Object3D, parent: THREE.Object3D): THREE.Group {
  parent.updateMatrixWorld(true);
  mesh.updateMatrixWorld(true);
  const center = new THREE.Box3().setFromObject(mesh).getCenter(new THREE.Vector3());
  parent.worldToLocal(center);
  const pivot = new THREE.Group();
  pivot.name = `${mesh.name}Pivot`;
  parent.add(pivot);
  pivot.position.copy(center);
  pivot.attach(mesh);
  return pivot;
}

/** Hinge at the chest-facing edge so arms raise from the shoulder, not the feet. */
function makeArmPivot(mesh: THREE.Object3D, parent: THREE.Object3D): THREE.Group {
  parent.updateMatrixWorld(true);
  mesh.updateMatrixWorld(true);
  const box = new THREE.Box3().setFromObject(mesh);
  const meshC = box.getCenter(new THREE.Vector3());
  const chestC = new THREE.Box3().setFromObject(parent).getCenter(new THREE.Vector3());
  const hinge = meshC.clone();
  const toChest = chestC.clone().sub(meshC);
  toChest.y *= 0.15;
  hinge.add(toChest.multiplyScalar(0.62));
  parent.worldToLocal(hinge);
  const pivot = new THREE.Group();
  pivot.name = `${mesh.name}Pivot`;
  parent.add(pivot);
  pivot.position.copy(hinge);
  pivot.attach(mesh);
  return pivot;
}

function collectEyeMats(root: THREE.Object3D): THREE.MeshStandardMaterial[] {
  const out: THREE.MeshStandardMaterial[] = [];
  root.traverse((obj) => {
    if (!(obj instanceof THREE.Mesh)) return;
    const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
    for (const m of mats) {
      if (m instanceof THREE.MeshStandardMaterial) out.push(m);
    }
  });
  return out;
}

/**
 * Opaque stage color for Linux. WebKitGTK + a transparent window cannot update
 * alpha-0 pixels (that is the clone trail). Every pixel we present is opaque;
 * roam moves the small window, which compositors already know how to damage.
 * Windows/macOS use `glass` instead (clearColor alpha 0, no blit).
 */
const STAGE = 0x0e1512;
const STAGE_CSS = "#0e1512";

function materialName(mat: THREE.Material): string {
  return (mat.name || "").toLowerCase();
}

function isImportedGlass(mat: THREE.Material): boolean {
  const n = materialName(mat);
  return n.includes("vetro") || n.includes("glass");
}

function isEyeMaterial(mat: THREE.MeshStandardMaterial): boolean {
  return (
    mat.emissiveIntensity > 0.4 ||
    mat.emissive.r + mat.emissive.g + mat.emissive.b > 0.4
  );
}

function prepareMaterial(mat: THREE.Material): THREE.Material {
  if (!(mat instanceof THREE.MeshStandardMaterial)) return mat;
  if (isImportedGlass(mat)) {
    // Keep Blender's Vetro mesh + StandardMaterial (Physical shaders break WebKitGTK).
    mat.side = THREE.FrontSide;
    mat.roughness = Math.min(mat.roughness, 0.15);
    mat.metalness = 0;
    mat.envMapIntensity = 1.05;
    mat.transparent = false;
    return mat;
  }
  if (isEyeMaterial(mat)) {
    mat.envMapIntensity = 0.12;
    mat.roughness = 0.48;
    mat.metalness = 0;
    mat.toneMapped = true;
    return mat;
  }
  mat.metalness = 0;
  mat.roughness = 0.68;
  mat.envMapIntensity = 0.28;
  mat.side = THREE.FrontSide;
  if (mat.color.r < 0.03 && mat.color.g < 0.03 && mat.color.b < 0.03) {
    mat.color.setRGB(0.028, 0.03, 0.034);
  }
  return mat;
}

/** LED mouth on the front visor face (+Z in head space after glTF Y-rotation). */
function makeFaceOverlay(mouthX: number, mouthY: number, mouthZ: number): THREE.Mesh {
  const mouthMat = new THREE.MeshStandardMaterial({
    color: 0xffffff,
    emissive: new THREE.Color(0xffffff),
    emissiveIntensity: 3.2,
    roughness: 0.45,
    metalness: 0,
    toneMapped: true,
    depthTest: false,
    depthWrite: false,
    transparent: true,
  });
  const mouth = new THREE.Mesh(new THREE.PlaneGeometry(0.15, 0.042), mouthMat);
  mouth.name = "VisorMouth";
  // PlaneGeometry default normal is +Z — matches the rotated glTF front.
  mouth.position.set(mouthX, mouthY, mouthZ);
  mouth.renderOrder = 12;
  return mouth;
}

function findVisorGlass(root: THREE.Object3D): THREE.Mesh | null {
  let found: THREE.Mesh | null = null;
  root.traverse((obj) => {
    if (found || !(obj instanceof THREE.Mesh)) return;
    const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
    if (mats.some((m) => isImportedGlass(m))) found = obj;
  });
  return found;
}

/** Place the LED mouth in the lower visor opening, centered and in front of the glass. */
function computeMouthPlacement(
  headPivot: THREE.Group,
  eyeLRest: THREE.Vector3,
  eyeRRest: THREE.Vector3,
  visorMesh: THREE.Mesh | null,
): THREE.Vector3 {
  const eyeMidX = (eyeLRest.x + eyeRRest.x) * 0.5;
  const eyeMidY = (eyeLRest.y + eyeRRest.y) * 0.5;
  const eyeFrontZ = Math.max(eyeLRest.z, eyeRRest.z);

  if (visorMesh) {
    headPivot.updateMatrixWorld(true);
    visorMesh.updateMatrixWorld(true);
    const box = new THREE.Box3().setFromObject(visorMesh);
    const min = headPivot.worldToLocal(box.min.clone());
    const max = headPivot.worldToLocal(box.max.clone());
    const frontZ = Math.max(min.z, max.z);
    const centerX = (min.x + max.x) * 0.5;
    const bottomY = Math.min(min.y, max.y);
    const visorHeight = Math.max(max.y, min.y) - bottomY;
    const yGap = THREE.MathUtils.clamp(visorHeight * 0.24, 0.07, 0.14);
    const y = THREE.MathUtils.clamp(
      eyeMidY - yGap,
      bottomY + visorHeight * 0.14,
      eyeMidY - visorHeight * 0.06,
    );
    return new THREE.Vector3(centerX, y, frontZ + 0.016);
  }

  return new THREE.Vector3(eyeMidX, eyeMidY - 0.09, eyeFrontZ + 0.012);
}

/** Head-and-body buddy from the Blender GLB — mouse gaze, blink, desktop lean. */
export async function mountBuddy3D(
  canvas: HTMLCanvasElement,
  options: Buddy3DOptions | AvatarState = "idle",
): Promise<Buddy3DHandle> {
  const opts: Buddy3DOptions =
    typeof options === "string" ? { state: options } : options;
  const live = opts.live === true;
  const framing: Buddy3DFraming = opts.framing ?? "full";
  const onStep = opts.onStep;
  const glass = opts.glass === true;

  const glCanvas = glass ? canvas : document.createElement("canvas");
  const renderer = new THREE.WebGLRenderer({
    canvas: glCanvas,
    antialias: true,
    alpha: glass,
    preserveDrawingBuffer: !glass,
    stencil: false,
    powerPreference: "default",
  });
  if (glass) renderer.setClearColor(0x000000, 0);
  else renderer.setClearColor(STAGE, 1);
  renderer.autoClear = true;
  renderer.toneMapping = THREE.AgXToneMapping;
  renderer.toneMappingExposure = 1.08;
  renderer.outputColorSpace = THREE.SRGBColorSpace;

  canvas.style.pointerEvents = "none";
  canvas.style.opacity = glass ? "1" : "0";
  if (glass) canvas.style.background = "transparent";

  let localPresent: HTMLCanvasElement | null = null;
  if (!glass) {
    const stage = document.createElement("canvas");
    stage.className = "buddy3d-present";
    stage.setAttribute("aria-hidden", "true");
    Object.assign(stage.style, {
      position: "absolute",
      inset: "0",
      width: "100%",
      height: "100%",
      display: "block",
      pointerEvents: "none",
      background: STAGE_CSS,
    });
    canvas.parentElement?.appendChild(stage);
    localPresent = stage;
  }
  void avatarLog(`3d present mode=${glass ? "glass" : "opaque-window"}`);

  function fitPresent(stage: HTMLCanvasElement) {
    const cssW = Math.max(1, stage.clientWidth || canvas.clientWidth || 1);
    const cssH = Math.max(1, stage.clientHeight || canvas.clientHeight || 1);
    const pr = Math.min(window.devicePixelRatio || 1, 2);
    const bw = Math.max(1, Math.round(cssW * pr));
    const bh = Math.max(1, Math.round(cssH * pr));
    if (stage.width !== bw) stage.width = bw;
    if (stage.height !== bh) stage.height = bh;
    return pr;
  }

  function blitPresent() {
    const stage = localPresent;
    if (!stage) return;
    const pctx = stage.getContext("2d", { alpha: false });
    if (!pctx) return;
    fitPresent(stage);
    pctx.setTransform(1, 0, 0, 1, 0, 0);
    pctx.fillStyle = STAGE_CSS;
    pctx.fillRect(0, 0, stage.width, stage.height);
    pctx.drawImage(glCanvas, 0, 0, stage.width, stage.height);
  }

  function debugSnapshot(): Record<string, unknown> {
    if (glass) {
      return { mode: "glass", stageW: canvas.width, stageH: canvas.height, cornerRgba: null };
    }
    const stage = localPresent;
    let corner: number[] | null = null;
    const pctx = stage?.getContext("2d", { alpha: false });
    if (pctx && stage && stage.width > 4 && stage.height > 4) {
      const d = pctx.getImageData(2, 2, 1, 1).data;
      corner = [d[0], d[1], d[2], d[3]];
    }
    return {
      mode: "opaque-window",
      stageW: stage?.width ?? 0,
      stageH: stage?.height ?? 0,
      cornerRgba: corner,
    };
  }
  (
    window as unknown as { __agentosAvatarDebug?: { snapshot: () => Record<string, unknown> } }
  ).__agentosAvatarDebug = { snapshot: debugSnapshot };

  const pmrem = new THREE.PMREMGenerator(renderer);
  const envScene = new RoomEnvironment();
  const envTex = pmrem.fromScene(envScene, 0.04).texture;
  envScene.dispose();

  const scene = new THREE.Scene();
  scene.background = glass ? null : new THREE.Color(STAGE);
  scene.environment = envTex;
  scene.environmentIntensity = 0.5;

  const camera = new THREE.PerspectiveCamera(28, 1, 0.05, 30);
  if (framing === "bust") {
    camera.position.set(0, 1.08, 2.4);
    camera.lookAt(0, 0.98, 0);
  } else {
    camera.position.set(0, 1.02, 4.85);
    camera.lookAt(0, 0.98, 0);
  }

  scene.add(new THREE.HemisphereLight(0xdde3ea, 0x1a1e22, 0.55));
  const key = new THREE.DirectionalLight(0xfff3e4, 1.7);
  key.position.set(2.6, 3.5, 3.1);
  scene.add(key);
  const fill = new THREE.DirectionalLight(0xb9cce8, 0.32);
  fill.position.set(-3.0, 1.1, 1.6);
  scene.add(fill);
  const rim = new THREE.DirectionalLight(0xeef4ff, 0.7);
  rim.position.set(-0.35, 2.4, -3.4);
  scene.add(rim);
  const bounce = new THREE.DirectionalLight(0xffe6cc, 0.16);
  bounce.position.set(0.2, -2.2, 1.8);
  scene.add(bounce);

  const proto = await loadPrototype();
  const model = instantiateModel(proto);

  const character = new THREE.Group();
  character.name = "BuddyCharacter";
  character.add(model);
  scene.add(character);
  character.position.y = -0.16;
  character.updateMatrixWorld(true);

  const headMesh = requireNamed(model, "Head");
  const eyeLMesh = requireNamed(model, "EyeL");
  const eyeRMesh = requireNamed(model, "EyeR");
  const chestMesh = requireNamed(model, "Chest");
  const earMesh = model.getObjectByName("Ear");

  const headBox = new THREE.Box3().setFromObject(headMesh);
  const neck = new THREE.Vector3(
    (headBox.min.x + headBox.max.x) * 0.5,
    headBox.min.y + 0.05,
    (headBox.min.z + headBox.max.z) * 0.5,
  );

  const headPivot = new THREE.Group();
  headPivot.name = "HeadPivot";
  character.add(headPivot);
  headPivot.position.copy(neck);
  headPivot.attach(headMesh);

  const eyeL = makeEyePivot(eyeLMesh, headPivot);
  const eyeR = makeEyePivot(eyeRMesh, headPivot);
  const eyeLRest = eyeL.position.clone();
  const eyeRRest = eyeR.position.clone();

  const visorGlass = findVisorGlass(headMesh);
  const mouthPos = computeMouthPlacement(headPivot, eyeLRest, eyeRRest, visorGlass);
  const mouth = makeFaceOverlay(mouthPos.x, mouthPos.y, mouthPos.z);
  headPivot.add(mouth);
  const mouthMat = mouth.material as THREE.MeshStandardMaterial;

  if (earMesh) {
    // Keep ears with the head so they turn with gaze.
    headPivot.attach(earMesh);
  }

  const chestBox = new THREE.Box3().setFromObject(chestMesh);
  const chestCenter = chestBox.getCenter(new THREE.Vector3());
  const chestPivot = new THREE.Group();
  chestPivot.name = "ChestPivot";
  character.add(chestPivot);
  chestPivot.position.copy(chestCenter);
  chestPivot.attach(chestMesh);

  const shoulderL = character.getObjectByName("ShoulderL");
  const shoulderR = character.getObjectByName("ShoulderR");
  const armL = shoulderL ? makeArmPivot(shoulderL, chestMesh) : null;
  const armR = shoulderR ? makeArmPivot(shoulderR, chestMesh) : null;
  const armLRest = {
    x: armL?.rotation.x ?? 0,
    y: armL?.rotation.y ?? 0,
    z: armL?.rotation.z ?? 0,
  };
  const armRRest = {
    x: armR?.rotation.x ?? 0,
    y: armR?.rotation.y ?? 0,
    z: armR?.rotation.z ?? 0,
  };

  const eyeMats = [...collectEyeMats(eyeLMesh), ...collectEyeMats(eyeRMesh)];
  const eyeBaseColor = eyeMats[0] ? eyeMats[0].color.clone() : new THREE.Color(0xffffff);
  const eyeBaseEmissive = eyeMats[0]?.emissiveIntensity ?? 4;

  const bubble = new THREE.Group();
  bubble.visible = false;
  bubble.scale.setScalar(0.001);
  headPivot.updateMatrixWorld(true);
  headMesh.updateMatrixWorld(true);
  const headWorld = new THREE.Box3().setFromObject(headMesh);
  const headTop = headPivot.worldToLocal(
    new THREE.Vector3(
      (headWorld.min.x + headWorld.max.x) * 0.5,
      headWorld.max.y,
      Math.max(headWorld.min.z, headWorld.max.z),
    ),
  );
  bubble.position.set(headTop.x + 0.16, headTop.y + 0.2, headTop.z + 0.04);
  const bubbleRest = bubble.position.clone();
  headPivot.add(bubble);
  const bubbleMat = new THREE.MeshStandardMaterial({
    color: 0xd8dde4,
    roughness: 0.55,
    metalness: 0.05,
  });
  const disc = new THREE.Mesh(new THREE.CircleGeometry(0.13, 24), bubbleMat);
  bubble.add(disc);
  const tail = new THREE.Mesh(new THREE.CircleGeometry(0.045, 12), bubbleMat);
  tail.position.set(-0.1, -0.13, -0.01);
  bubble.add(tail);
  const dots: THREE.Mesh[] = [];
  const dotMat = new THREE.MeshStandardMaterial({ color: 0x1a2744, roughness: 0.4 });
  for (let i = 0; i < 3; i++) {
    const d = new THREE.Mesh(new THREE.SphereGeometry(0.028, 10, 8), dotMat);
    d.position.set(-0.06 + i * 0.06, 0.01, 0.02);
    bubble.add(d);
    dots.push(d);
  }

  let avatarState: AvatarState = opts.state ?? "idle";
  let raf = 0;
  let disposed = false;
  const clock = new THREE.Clock();
  const localPointer = { x: 0, y: 0 };
  const screenPointer = { x: 0, y: 0, winX: 0, winY: 0, winW: 0, winH: 0, valid: false };
  const gait = { walk: 0, dirX: 0, dirY: 0, wantWalk: false };
  const look = { nx: 0, ny: 0 };
  let walkPhase = 0;
  let lastStepSign = 0;
  let blinkT = 0;
  let nextBlink = 1.6 + Math.random() * 2.2;
  let blinkPhase = 0;
  let winkLeft = false;
  let saccadeT = 0;
  let nextSaccade = 0.5 + Math.random() * 1.4;
  let saccadeX = 0;
  let saccadeY = 0;
  let glanceX = 0;
  let glanceY = 0;
  let nextGlance = 3 + Math.random() * 4;
  let glanceT = 0;
  let lastPointerStamp = 0;
  let bubbleTarget = 0;
  let fidgetT = 0;
  let nextFidget = 2.2 + Math.random() * 2.5;
  let fidgetKind = 1;
  let fidgetHold = 0.55;
  let exprKind = 0;
  let exprT = 0;
  let nextExpr = 0.4;
  let exprState: AvatarState = avatarState;

  function pickExpr(state: AvatarState): number {
    if (state === "thinking") return 1 + Math.floor(Math.random() * 3);
    if (state === "speaking") return 1 + Math.floor(Math.random() * 3);
    if (state === "notification") return 1;
    if (state === "error") return 1;
    if (state === "sleeping") return 1 + Math.floor(Math.random() * 2);
    return 1 + Math.floor(Math.random() * 4);
  }

  function exprDuration(state: AvatarState): number {
    if (state === "thinking") return 1.8 + Math.random() * 1.4;
    if (state === "speaking") return 1.3 + Math.random() * 1.1;
    if (state === "notification") return 2.2;
    if (state === "error") return 2.8;
    if (state === "sleeping") return 3.5 + Math.random() * 2.5;
    return 2.6 + Math.random() * 2.4;
  }

  function tickExpr(dt: number, force = false) {
    if (force || avatarState !== exprState) {
      exprState = avatarState;
      exprKind = pickExpr(avatarState);
      exprT = 0;
      nextExpr = exprDuration(avatarState);
      return;
    }
    exprT += dt;
    if (exprT >= nextExpr) {
      exprKind = pickExpr(avatarState);
      exprT = 0;
      nextExpr = exprDuration(avatarState);
    }
  }

  const onMove = (e: MouseEvent) => {
    localPointer.x = e.clientX;
    localPointer.y = e.clientY;
    lastPointerStamp = performance.now();
  };
  window.addEventListener("mousemove", onMove, { passive: true });

  function fit() {
    const parent = canvas.parentElement;
    const w = Math.max(1, parent?.clientWidth ?? canvas.clientWidth);
    const h = Math.max(1, parent?.clientHeight ?? canvas.clientHeight);
    const pr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
    const pw = Math.max(1, Math.round(w * pr));
    const ph = Math.max(1, Math.round(h * pr));
    canvas.width = pw;
    canvas.height = ph;
    renderer.setPixelRatio(pr);
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  }

  function gazeFromLocal(): { nx: number; ny: number } {
    const rect = (canvas.parentElement ?? canvas).getBoundingClientRect();
    if (rect.width < 4 || rect.height < 4) return { nx: 0, ny: 0 };
    const nx = ((localPointer.x - rect.left) / rect.width) * 2 - 1;
    const ny = -(((localPointer.y - rect.top) / rect.height) * 2 - 1);
    return { nx, ny };
  }

  function gazeFromScreen(): { nx: number; ny: number } {
    const { x, y, winX, winY, winW, winH } = screenPointer;
    if (winW < 4 || winH < 4) return { nx: 0, ny: 0 };
    const cx = winX + winW * 0.5;
    const cy = winY + winH * (framing === "bust" ? 0.42 : 0.38);
    const nx = (x - cx) / Math.max(120, winW * 0.85);
    const ny = -((y - cy) / Math.max(120, winH * 0.85));
    return { nx, ny };
  }

  function updateGaze(dt: number) {
    glanceT += dt;
    if (glanceT >= nextGlance) {
      glanceT = 0;
      nextGlance = 1.8 + Math.random() * 2.8;
      glanceX = (Math.random() - 0.5) * 1.7;
      glanceY = (Math.random() - 0.5) * 0.95;
    }
    const glanceMix = Math.sin(Math.min(1, glanceT / 0.32) * Math.PI);
    const idleNx = glanceX * glanceMix * 0.55;
    const idleNy = glanceY * glanceMix * 0.32;

    const pointerFresh = performance.now() - lastPointerStamp < 2800;
    let nx = idleNx;
    let ny = idleNy;
    if (avatarState === "sleeping") {
      nx = 0;
      ny = 0;
    } else if (live && screenPointer.valid && pointerFresh) {
      const g = gazeFromScreen();
      nx = THREE.MathUtils.clamp(g.nx, -1.15, 1.15);
      ny = THREE.MathUtils.clamp(g.ny, -1, 1);
    } else if (!live && pointerFresh) {
      const g = gazeFromLocal();
      nx = THREE.MathUtils.clamp(g.nx, -1.15, 1.15);
      ny = THREE.MathUtils.clamp(g.ny, -1, 1);
    }
    look.nx = nx;
    look.ny = ny;

    const yaw = nx * 0.72 + (avatarState === "thinking"
      ? exprKind === 2
        ? 0.38
        : exprKind === 3
          ? -0.34
          : 0.08
      : avatarState === "speaking"
        ? exprKind === 2
          ? 0.16
          : exprKind === 3
            ? -0.14
            : 0
        : 0);
    const pitch =
      THREE.MathUtils.clamp(-ny, -1, 1) * (avatarState === "sleeping" ? 0.04 : 0.32) +
      (avatarState === "sleeping"
        ? -0.35 + Math.sin(performance.now() * 0.0004) * 0.06
        : avatarState === "thinking"
          ? (exprKind === 1 ? -0.24 : -0.1)
          : 0);
    headPivot.rotation.y = THREE.MathUtils.lerp(headPivot.rotation.y, yaw, 0.14);
    headPivot.rotation.x = THREE.MathUtils.lerp(headPivot.rotation.x, pitch, 0.14);

    saccadeT += dt;
    if (saccadeT >= nextSaccade) {
      saccadeT = 0;
      nextSaccade = 0.4 + Math.random() * 1.6;
      saccadeX = (Math.random() - 0.5) * 0.028;
      saccadeY = (Math.random() - 0.5) * 0.018;
    }

    let exprDx = 0;
    let exprDy = 0;
    if (avatarState === "thinking") {
      if (exprKind === 1) {
        exprDy = 0.028;
        exprDx = 0.004;
      } else if (exprKind === 2) {
        exprDx = 0.042;
        exprDy = 0.01;
      } else {
        exprDx = -0.038;
        exprDy = 0.018;
      }
    } else if (avatarState === "error") {
      exprDy = -0.022;
    } else if (avatarState === "notification") {
      exprDy = 0.018;
      exprDx = Math.sin(performance.now() * 0.004) * 0.012;
    } else if (avatarState === "speaking") {
      exprDy = 0.006;
      exprDx = exprKind === 2 ? 0.018 : exprKind === 3 ? -0.016 : 0;
    } else if (avatarState === "sleeping") {
      exprDy = -0.04;
      exprDx = 0;
    } else if (exprKind === 3) {
      exprDx = glanceX * 0.02;
      exprDy = 0.01;
    }

    const ex = THREE.MathUtils.clamp(nx, -1, 1) * 0.1 + saccadeX + exprDx;
    const ey = THREE.MathUtils.clamp(ny, -1, 1) * 0.07 + saccadeY + exprDy;
    eyeL.position.x = THREE.MathUtils.lerp(eyeL.position.x, eyeLRest.x + ex, 0.2);
    eyeL.position.y = THREE.MathUtils.lerp(eyeL.position.y, eyeLRest.y + ey, 0.2);
    eyeR.position.x = THREE.MathUtils.lerp(eyeR.position.x, eyeRRest.x + ex, 0.2);
    eyeR.position.y = THREE.MathUtils.lerp(eyeR.position.y, eyeRRest.y + ey, 0.2);
  }

  function updateFace(dt: number, t: number) {
    blinkT += dt;
    if (avatarState === "sleeping") {
      blinkPhase = 0;
      blinkT = 0;
    }
    const speakBoost = avatarState === "speaking" ? 0.4 : avatarState === "thinking" ? 0.8 : 1;
    if (avatarState !== "sleeping" && blinkPhase <= 0 && blinkT >= nextBlink * speakBoost) {
      blinkPhase = 0.0001;
      blinkT = 0;
      nextBlink = 1.7 + Math.random() * 3.0;
      winkLeft = avatarState === "idle" && Math.random() < 0.14;
    }
    let open = 1;
    if (blinkPhase > 0) {
      blinkPhase += dt;
      const p = blinkPhase;
      if (p < 0.05) open = 1 - p / 0.05;
      else if (p < 0.12) open = (p - 0.05) / 0.07;
      else {
        open = 1;
        blinkPhase = 0;
        winkLeft = false;
      }
    }

    let lid = open;
    let wide = 1;
    let tiltL = -0.06;
    let tiltR = 0.06;
    let mouthOn = true;
    let mx = 1.05;
    let my = 0.52;

    if (avatarState === "thinking") {
      mouthOn = exprKind !== 1;
      if (exprKind === 1) {
        lid = Math.min(lid, 0.62);
        wide = 1.08;
        tiltL = -0.18;
        tiltR = 0.18;
        mx = 0.7;
        my = 0.32;
      } else if (exprKind === 2) {
        lid = Math.min(lid, 0.88);
        wide = 1.12;
        tiltL = -0.04;
        tiltR = 0.22;
        mx = 0.9;
        my = 0.4;
      } else {
        lid = Math.min(lid, 0.72);
        wide = 1.06;
        tiltL = -0.22;
        tiltR = 0.04;
        mx = 0.85;
        my = 0.36;
      }
    } else if (avatarState === "sleeping") {
      lid = Math.min(lid, 0.08);
      wide = 0.82;
      tiltL = 0.02;
      tiltR = -0.02;
      mouthOn = false;
    } else if (avatarState === "error") {
      lid = Math.min(lid, 0.48);
      wide = 0.94;
      tiltL = 0.2;
      tiltR = -0.2;
      mx = 0.62;
      my = 0.32;
    } else if (avatarState === "notification") {
      lid = Math.max(lid, 1.18);
      wide = 1.18;
      tiltL = -0.08;
      tiltR = 0.08;
      mx = 0.7;
      my = 1.05;
    } else if (avatarState === "speaking") {
      lid = Math.min(lid, 0.98);
      wide = 1.1;
      tiltL = exprKind === 3 ? -0.16 : -0.06;
      tiltR = exprKind === 2 ? 0.16 : 0.06;
      const talk = 0.55 + Math.abs(Math.sin(t * 12.5)) * 0.7;
      mx = 0.95 + Math.sin(t * 9.5) * 0.22;
      my = talk;
    } else {
      if (exprKind === 1) {
        tiltL = -0.1;
        tiltR = 0.1;
        mx = 1.12;
        my = 0.55;
      } else if (exprKind === 2) {
        lid = Math.min(lid, 0.9);
        mx = 1.2;
        my = 0.7;
        tiltL = -0.14;
        tiltR = 0.14;
      } else if (exprKind === 3) {
        wide = 1.12;
        lid = Math.max(lid, 1.08);
        mx = 0.85;
        my = 0.85;
      } else {
        tiltL = -0.07;
        tiltR = 0.07;
        mx = 1.05;
        my = 0.48;
      }
    }

    const sy = Math.max(0.08, lid);
    const leftSy = winkLeft ? Math.min(sy, Math.max(0.08, open)) : sy;
    const rightSy = winkLeft ? Math.max(0.82, lid) : sy;
    eyeL.scale.y = leftSy;
    eyeR.scale.y = rightSy;
    eyeL.scale.x = THREE.MathUtils.lerp(eyeL.scale.x, wide, 0.18);
    eyeR.scale.x = THREE.MathUtils.lerp(eyeR.scale.x, wide, 0.18);
    eyeL.rotation.z = THREE.MathUtils.damp(eyeL.rotation.z, tiltL, 8, dt);
    eyeR.rotation.z = THREE.MathUtils.damp(eyeR.rotation.z, tiltR, 8, dt);

    mouth.visible = mouthOn;
    if (mouthOn) {
      mouth.scale.x = THREE.MathUtils.damp(mouth.scale.x, mx, 10, dt);
      mouth.scale.y = THREE.MathUtils.damp(mouth.scale.y, Math.max(0.38, my), 12, dt);
    }
    if (avatarState === "error") {
      mouthMat.emissiveIntensity = 0.35;
      mouthMat.color.setHex(0x8a93a0);
    } else {
      mouthMat.emissiveIntensity = 3.2;
      mouthMat.color.copy(eyeBaseColor);
    }
  }

  function updateBubble(dt: number, t: number) {
    const show = avatarState === "thinking" || avatarState === "notification" || avatarState === "sleeping";
    bubbleTarget = show ? 1 : 0;
    const next = THREE.MathUtils.lerp(bubble.scale.x, bubbleTarget || 0.001, show ? 0.18 : 0.22);
    const breathPulse = avatarState === "sleeping" ? 1 + Math.sin(t * 0.65) * 0.12 : 1;
    const notifPulse = avatarState === "notification" ? 1 + Math.sin(t * 6) * 0.08 : 1;
    bubble.scale.setScalar(next * breathPulse * notifPulse);
    bubble.visible = next > 0.05;
    if (show) {
      const dotSpeed = avatarState === "sleeping" ? 1.5 : 6;
      dots.forEach((d, i) => {
        d.position.y = 0.01 + Math.sin(t * dotSpeed + i * 0.9) * 0.022;
      });
      const floatSpeed = avatarState === "sleeping" ? 0.55 : 2.2;
      bubble.position.y = bubbleRest.y + Math.sin(t * floatSpeed) * 0.015;
    }
  }

  function armTargets(
    t: number,
    dt: number,
  ): { l: { x: number; y: number; z: number }; r: { x: number; y: number; z: number } } {
    const l = { x: 0, y: 0, z: 0 };
    const r = { x: 0, y: 0, z: 0 };
    const swing = Math.sin(walkPhase) * 0.7 * gait.walk;
    l.x += swing;
    r.x -= swing;
    l.z += 0.12 * gait.walk;
    r.z += 0.12 * gait.walk;

    if (avatarState === "thinking") {
      if (exprKind === 1) {
        l.z = 1.15;
        l.x = 0.55;
        l.y = 0.28;
        r.z = 0.22;
        r.x = -0.12;
      } else if (exprKind === 2) {
        r.z = 1.05;
        r.x = -0.42;
        r.y = -0.22;
        l.z = 0.2;
        l.x = 0.1;
      } else {
        l.z = 0.55;
        r.z = 0.55;
        l.x = 0.22 + Math.sin(t * 2.4) * 0.18;
        r.x = -0.22 - Math.sin(t * 2.4) * 0.18;
      }
    } else if (avatarState === "speaking") {
      if (exprKind === 1) {
        l.z = 0.55 + Math.sin(t * 5.4) * 0.55;
        r.z = 0.5 + Math.sin(t * 5.4 + 1.4) * 0.5;
        l.x = Math.sin(t * 4.1) * 0.38;
        r.x = Math.sin(t * 4.1 + 0.9) * 0.38;
      } else if (exprKind === 2) {
        l.z = 1.05;
        l.x = 0.35 + Math.sin(t * 7) * 0.22;
        r.z = 0.28;
        r.x = -0.1;
      } else {
        r.z = 1.05;
        r.x = -0.32 + Math.sin(t * 6.2) * 0.2;
        l.z = 0.32;
        l.x = 0.12;
      }
    } else if (avatarState === "notification") {
      const wave = Math.sin(t * 10.2);
      l.z = 1.45 + wave * 0.42;
      l.x = 0.22 + wave * 0.55;
      l.y = 0.18;
      r.z = 1.05 + Math.sin(t * 7.1 + 0.8) * 0.28;
      r.x = -0.28 + Math.sin(t * 5.4) * 0.22;
    } else if (avatarState === "sleeping") {
      l.z = -0.18;
      r.z = -0.18;
      l.x = 0.06;
      r.x = -0.06;
    } else if (avatarState === "error") {
      l.z = -0.35;
      r.z = -0.35;
      l.x = 0.16;
      r.x = -0.16;
    } else if (gait.walk < 0.25) {
      fidgetT += dt;
      if (fidgetT >= nextFidget) {
        fidgetT = 0;
        nextFidget = 2.4 + Math.random() * 3.2;
        fidgetKind = exprKind;
        fidgetHold = 0.7 + Math.random() * 0.55;
      }
      const u = fidgetT / Math.max(0.2, fidgetHold);
      const k = u < 1 ? Math.sin(Math.min(1, u) * Math.PI) : 0;
      if (exprKind === 1) {
        l.z += 1.15 * k;
        l.x += 0.28 * k;
      } else if (exprKind === 2) {
        r.z += 1.05 * k;
        r.y += 0.18 * k;
      } else if (exprKind === 3) {
        l.z += 0.7 * k;
        r.z += 0.7 * k;
        l.y += 0.22 * k;
        r.y -= 0.22 * k;
      } else {
        l.z += 0.45 * k;
        r.z += 0.45 * k;
      }
    }
    return { l, r };
  }

  function updateBody(t: number, dt: number) {
    const pulse = avatarState === "speaking" ? 1 + Math.sin(t * 9) * 0.12 : 1;
    let glow = eyeBaseEmissive;
    if (avatarState === "speaking") glow = eyeBaseEmissive * 1.35 * pulse;
    else if (avatarState === "thinking") glow = eyeBaseEmissive * 1.15;
    else if (avatarState === "notification") glow = eyeBaseEmissive * (1.55 + Math.abs(Math.sin(t * 6.8)) * 1.35);
    else if (avatarState === "sleeping") glow = eyeBaseEmissive * 0.06;
    else if (avatarState === "error") glow = eyeBaseEmissive * 0.18;

    for (const m of eyeMats) {
      m.emissiveIntensity = glow;
      if (avatarState === "error") m.color.setHex(0x8a93a0);
      else if (avatarState === "sleeping") m.color.setHex(0x364855);
      else m.color.copy(eyeBaseColor);
    }

    const want = gait.wantWalk ? 1 : 0;
    gait.walk = THREE.MathUtils.damp(gait.walk, want, 4.2, dt);

    const freq = 8.2;
    walkPhase += dt * freq * (0.18 + gait.walk);
    const step = Math.sin(walkPhase);
    const stepSign = step >= 0 ? 1 : -1;
    if (gait.wantWalk && gait.walk > 0.55 && stepSign !== lastStepSign && lastStepSign !== 0) {
      const len = Math.hypot(gait.dirX, gait.dirY) || 1;
      onStep?.(gait.dirX / len, gait.dirY / len);
    }
    lastStepSign = stepSign;

    const breathRate = avatarState === "sleeping" ? 0.65 : 1.55;
    const breathAmp = avatarState === "sleeping" ? 0.028 : 0.018;
    const idleBounce = 0.012 + Math.sin(t * breathRate) * breathAmp;
    const hop = avatarState === "sleeping" ? 0 : Math.abs(step) * 0.07 * gait.walk;
    let excite = 0;
    if (avatarState === "notification") excite = Math.abs(Math.sin(t * 7.6)) * 0.2;
    else if (avatarState === "speaking") excite = Math.abs(Math.sin(t * 6.4)) * 0.025;
    else if (avatarState === "thinking") excite = Math.sin(t * 1.8) * 0.012;
    character.position.y = -0.12 + idleBounce + hop + excite;

    const roll = step * 0.22 * gait.walk + Math.sin(t * 0.7) * 0.04 * (1 - gait.walk * 0.7);
    character.rotation.z = roll;
    const faceYaw =
      gait.walk > 0.18
        ? THREE.MathUtils.clamp(-gait.dirX, -1, 1) * 1.05 * gait.walk
        : look.nx * 0.58;
    character.rotation.y = THREE.MathUtils.damp(character.rotation.y, faceYaw, 2.6, dt);
    character.rotation.x = THREE.MathUtils.damp(
      character.rotation.x,
      THREE.MathUtils.clamp(gait.dirY, -1, 1) * 0.12 * gait.walk + look.ny * 0.04 * (1 - gait.walk),
      2.8,
      dt,
    );

    chestPivot.position.y =
      chestCenter.y + Math.sin(t * 1.55) * 0.014 + Math.sin(walkPhase * 2) * 0.016 * gait.walk;
    chestPivot.rotation.z = -roll * 0.4;
    chestPivot.rotation.x = Math.sin(walkPhase * 2) * 0.07 * gait.walk;

    const arms = armTargets(t, dt);
    if (armL) {
      armL.rotation.x = THREE.MathUtils.damp(armL.rotation.x, armLRest.x + arms.l.x, 6, dt);
      armL.rotation.y = THREE.MathUtils.damp(armL.rotation.y, armLRest.y + arms.l.y, 6, dt);
      armL.rotation.z = THREE.MathUtils.damp(armL.rotation.z, armLRest.z + arms.l.z, 6, dt);
    }
    if (armR) {
      armR.rotation.x = THREE.MathUtils.damp(armR.rotation.x, armRRest.x + arms.r.x, 6, dt);
      armR.rotation.y = THREE.MathUtils.damp(armR.rotation.y, armRRest.y + arms.r.y, 6, dt);
      armR.rotation.z = THREE.MathUtils.damp(armR.rotation.z, armRRest.z + arms.r.z, 6, dt);
    }

    let headRoll = -roll * 0.45;
    if (avatarState === "sleeping") {
      const nod = Math.sin(t * 0.35) * 0.06;
      headRoll = 0.14 + nod;
    } else if (avatarState === "error") headRoll = -0.22;
    else if (avatarState === "thinking") {
      if (exprKind === 1) headRoll += 0.1 + Math.sin(t * 1.15) * 0.08;
      else if (exprKind === 2) headRoll += -0.16 + Math.sin(t * 0.9) * 0.04;
      else headRoll += 0.18 + Math.sin(t * 1.4) * 0.05;
    } else if (avatarState === "speaking") {
      headRoll += Math.sin(t * (exprKind === 1 ? 3.2 : 2.2)) * (exprKind === 1 ? 0.09 : 0.06);
    } else if (avatarState === "notification") {
      headRoll += Math.sin(t * 5.4) * 0.14;
    } else if (exprKind === 2) {
      headRoll += Math.sin(t * 1.6) * 0.06;
    }
    headPivot.rotation.z = THREE.MathUtils.damp(headPivot.rotation.z, headRoll, 5, dt);
  }

  function tick() {
    if (disposed) return;
    raf = requestAnimationFrame(tick);
    const dt = Math.min(0.05, clock.getDelta());
    const t = clock.elapsedTime;
    tickExpr(dt);
    updateGaze(dt);
    updateFace(dt, t);
    updateBubble(dt, t);
    updateBody(t, dt);
    renderer.render(scene, camera);
    blitPresent();
  }

  fit();
  tick();

  const ro =
    typeof ResizeObserver !== "undefined" ? new ResizeObserver(() => fit()) : null;
  if (canvas.parentElement && ro) ro.observe(canvas.parentElement);

  return {
    setState(s) {
      avatarState = s;
      tickExpr(0, true);
    },
    setPointer(screenX, screenY, winX, winY, winW, winH) {
      const moved =
        Math.abs(screenPointer.x - screenX) > 2 || Math.abs(screenPointer.y - screenY) > 2;
      screenPointer.x = screenX;
      screenPointer.y = screenY;
      screenPointer.winX = winX;
      screenPointer.winY = winY;
      screenPointer.winW = winW;
      screenPointer.winH = winH;
      screenPointer.valid = true;
      if (moved) lastPointerStamp = performance.now();
    },
    setGait(walking, dirX, dirY) {
      gait.wantWalk = walking;
      if (walking) {
        gait.dirX = dirX;
        gait.dirY = dirY;
      }
    },
    setPresent(_el) {
      /* overlay present path removed: compositor cannot key transparency */
    },
    resize: fit,
    dispose() {
      disposed = true;
      cancelAnimationFrame(raf);
      window.removeEventListener("mousemove", onMove);
      ro?.disconnect();
      localPresent?.remove();
      try {
        delete (window as unknown as { __agentosAvatarDebug?: unknown }).__agentosAvatarDebug;
      } catch {
        /* ignore */
      }
      envTex.dispose();
      pmrem.dispose();
      renderer.dispose();
      glCanvas.width = 0;
      glCanvas.height = 0;
      scene.traverse((obj) => {
        if (obj instanceof THREE.Mesh) {
          obj.geometry?.dispose();
          const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
          for (const m of mats) m?.dispose?.();
        }
      });
    },
  };
}
