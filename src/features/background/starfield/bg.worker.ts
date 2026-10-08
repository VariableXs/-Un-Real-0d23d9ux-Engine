import { generateStars, makeStarfieldMask, TIER_SPECS } from "./math";
import { createGpu, type Gpu } from "./renderer";

/**
 * OffscreenCanvas starfield worker (spec chapter 4.2): owns the WebGL2
 * context and the render loop on a dedicated thread, so React re-renders on
 * the main thread can never jank the background. `engine.ts` proxies into it.
 *
 * main → worker : init / opts / vp / topology / cursor / resize / visibility / dispose
 * worker → main : ready / tier / failed
 */

interface InitMsg {
  type: "init";
  canvas: OffscreenCanvas;
  motion: number;
  mouseParallax: number;
  tier: number;
  widthCss: number;
  heightCss: number;
  dpr: number;
}

type InMsg =
  | InitMsg
  | { type: "ping" }
  | { type: "opts"; motion: number; mouseParallax: number; tier: number }
  | { type: "vp"; x: number; y: number; zoom: number }
  | { type: "topology"; centers: Array<{ x: number; y: number }> }
  | { type: "cursor"; x: number; y: number }
  | { type: "resize"; widthCss: number; heightCss: number; dpr: number }
  | { type: "visibility"; hidden: boolean }
  | { type: "dispose" };

const SEED = 20260826;
const MARGIN = 320;

let oc: OffscreenCanvas | null = null;
let gpu: Gpu | null = null;
let disposed = true;
let loopActive = false;
let hidden = false;

let o = { motion: 0.7, mouseParallax: 30, tier: 0 };
let activeLevel = 8;
let widthCss = 1280;
let heightCss = 800;
let devicePixelRatio = 1;

let vp = { x: 0, y: 0, zoom: 1 };
let lastZoom = 1;
let zoomVel = 0;
let lastMoveT = -1e9; // last viewport interaction (fast-move LOD window)
let mouseX = 0;
let mouseY = 0;
const cur = { x: -9999, y: -9999, lt: 0 };

// ---- night cycle removed — cursor state + FPS monitor remain ----
let cursorActive = false;

let spanW = 0;
let spanH = 0;
let timer = 0;
let lastT = 0;
let t0 = 0;
let frameAccum = 0;
let frameN = 0;
let cooldown = 0;

function post(msg: Record<string, unknown>): void {
  (self as unknown as Worker).postMessage(msg);
}

function buildStars(): void {
  if (!gpu) return;
  spanW = widthCss + MARGIN * 2;
  spanH = heightCss + MARGIN * 2;
  const budget = TIER_SPECS[activeLevel - 1]!.starBudget;
  // Base simplex rifts ∪ two crossing galactic rivers.
  const { data } = generateStars({
    viewWidth: widthCss,
    viewHeight: heightCss,
    margin: MARGIN,
    seed: SEED,
    mask: makeStarfieldMask(SEED, spanW, spanH),
    targetCount: budget,
  });
  gpu.uploadStars(data);
  builtLevel = activeLevel;
}
let builtLevel = 0;
let levelTimer = 0;

function evaluateFps(dtMs: number): void {
  const spec = TIER_SPECS[activeLevel - 1]!;
  if (!spec.continuous || cooldown > 0) {
    cooldown--;
    return;
  }
  frameAccum += dtMs;
  if (++frameN < 45) return;
  const fps = 1000 / Math.max(frameAccum / frameN, 1e-3);
  frameAccum = 0;
  frameN = 0;
  if (o.tier !== 0) return;
  if (fps < 80 && activeLevel > 4) {
    activeLevel--;
    cooldown = 90;
    post({ type: "tier", level: activeLevel });
  } else if (fps > 112 && activeLevel < 10) {
    activeLevel++;
    cooldown = 180;
    post({ type: "tier", level: activeLevel });
  }
}

/** Draws exactly one frame; reschedules itself only while looping. */
function tick(now: number): void {
  if (disposed || hidden || !gpu || !oc) return;
  const dt = now - lastT;
  lastT = now;
  evaluateFps(dt);

  // Tier changed → rebuild the star budget once (debounced).
  if (builtLevel !== activeLevel && levelTimer === 0) {
    levelTimer = setTimeout(() => {
      levelTimer = 0;
      if (!disposed) buildStars();
    }, 600) as unknown as number;
  }

  const zDelta = vp.zoom - lastZoom;
  lastZoom = vp.zoom;
  zoomVel += (zDelta * 60 - zoomVel) * 0.18;

  const spec = TIER_SPECS[activeLevel - 1]!;
  // 4K-clarity ladder: top tier supersamples up to 2.5× CSS.
  const dprCap = activeLevel >= 10 ? 2.5 : 2.0;
  const dpr = Math.min(devicePixelRatio, dprCap) * spec.dprScale;
  const w = Math.max(1, Math.round(widthCss * dpr));
  const h = Math.max(1, Math.round(heightCss * dpr));
  if (oc.width !== w || oc.height !== h) {
    oc.width = w;
    oc.height = h;
  }

  const mx = (mouseX / Math.max(1, widthCss) - 0.5) * o.mouseParallax;
  const my = -(mouseY / Math.max(1, heightCss) - 0.5) * o.mouseParallax;

  const timeSec = (now - t0) / 1000;
  const fastMove = now - lastMoveT < 150 || Math.abs(zoomVel) > 0.5; // LOD window

  gpu.draw({
    time: timeSec,
    dpr,
    camX: vp.x + mx,
    camY: vp.y + my,
    spanW,
    spanH,
    margin: MARGIN,
    motion: o.motion,
    cursorX: cur.x,
    cursorY: cur.y,
    zoomVel,
    centerDim: false,
    tier: spec,
    cursorActive,
    fastMove,
  });

  // Level 1 / motion-0: single baked frame per interaction — no idle loop.
  loopActive = o.motion > 0 && spec.continuous;
  if (loopActive) {
    const interval = Math.max(4, Math.round(1000 / Math.max(spec.fpsTarget, 30)));
    timer = setTimeout(tick, interval) as unknown as number;
  }
}

function kick(): void {
  if (disposed || hidden || loopActive || !gpu) return;
  clearTimeout(timer);
  lastT = performance.now();
  tick(lastT);
}

self.onmessage = (ev: MessageEvent<InMsg>): void => {
  const m = ev.data;
  try {
    switch (m.type) {
      case "ping":
        // Environment probe: proves the module worker booted and this script
        // executed, BEFORE the main thread gives up its canvas.
        post({ type: "pong" });
        break;
      case "init": {
        devicePixelRatio = m.dpr;
        widthCss = m.widthCss;
        heightCss = m.heightCss;
        o = { motion: m.motion, mouseParallax: m.mouseParallax, tier: m.tier };
        activeLevel = o.tier >= 1 ? o.tier : 8;
        oc = m.canvas;
        const gl = oc.getContext("webgl2", {
          alpha: false,
          antialias: false,
          powerPreference: "high-performance",
        }) as WebGL2RenderingContext | null;
        if (!gl) {
          post({ type: "failed" });
          return;
        }
        gpu = createGpu(gl);
        if (!gpu) {
          post({ type: "failed" });
          return;
        }
        buildStars();
        disposed = false;
        hidden = false;
        t0 = performance.now();
        lastT = t0;
        post({ type: "ready" });
        kick();
        break;
      }
      case "opts": {
        o = { motion: m.motion, mouseParallax: m.mouseParallax, tier: m.tier };
        if (o.tier >= 1) activeLevel = o.tier;
        else if (activeLevel < 4) activeLevel = 8;
        if (o.motion <= 0) {
          clearTimeout(timer);
          loopActive = false;
        }
        kick();
        break;
      }
      case "vp": {
        vp = { x: m.x, y: m.y, zoom: m.zoom };
        lastMoveT = performance.now(); // fast-move LOD window restarts
        kick(); // L1: re-bake the frame at the new camera offset
        break;
      }
      case "topology": {
        void m.centers; // gravity field removed (spec 0.1) — kept as a no-op
        break;
      }
      case "cursor": {
        cur.x = m.x;
        cur.y = m.y;
        cur.lt = performance.now();
        mouseX = m.x;
        mouseY = m.y;
        cursorActive = m.x >= 0; // proxy sends -9999 when the pointer leaves
        break;
      }
      case "resize": {
        widthCss = m.widthCss;
        heightCss = m.heightCss;
        devicePixelRatio = m.dpr;
        buildStars();
        kick();
        break;
      }
      case "visibility": {
        hidden = m.hidden;
        if (hidden) {
          clearTimeout(timer);
          loopActive = false;
        } else {
          kick();
        }
        break;
      }
      case "dispose": {
        disposed = true;
        loopActive = false;
        clearTimeout(timer);
        gpu?.dispose();
        gpu = null;
        break;
      }
    }
  } catch (err) {
    console.error("[bg.worker]", err);
    post({ type: "failed" });
  }
};
