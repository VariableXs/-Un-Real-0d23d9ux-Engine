import { generateStars, makeStarfieldMask, TIER_SPECS } from "./math";
import { createGpu } from "./renderer";

/**
 * Main-thread starfield engine (chapter 4). Owns the WebGL2 context, the
 * packed star buffer and the requestAnimationFrame loop. CPU cost per frame
 * is uniform uploads only (< 0.05 ms); all dynamics live in the shaders.
 */
export interface EngineOptions {
  /** Dynamic strength 0..1 (settings × theme factors). */
  motion: number;
  /** Legacy pointer-parallax strength px (kept subtle next to cam parallax). */
  mouseParallax: number;
  /** 1..10 fixed tier; 0 = smart monitor keeps 80 FPS within L4..L10. */
  tier: number;
  /** While typing: dim starlight 30% inside the center 400px (chapter 6.1). */
  editing: boolean;
  /** Worker-mode only: GL/worker died after the canvas was transferred —
   *  the caller must fall back to the CSS background layer. */
  onFailed?: () => void;
}

export interface Viewport {
  x: number;
  y: number;
  zoom: number;
}

export interface AnimeStarfieldHandle {
  resize: () => void;
  setOptions: (o: Partial<EngineOptions>) => void;
  /** Mindmap canvas viewport → depth parallax + zoom bokeh. */
  setViewport: (vp: Viewport) => void;
  /** Legacy topology feed — no longer consumed (gravity field removed,
   *  spec 0.1); retained so callers stay source-compatible. */
  setTopology: (centers: Array<{ x: number; y: number }>) => void;
  getActiveTier: () => number;
  start: () => void;
  stop: () => void;
  renderOnce: () => void;
  dispose: () => void;
}

const SEED = 20260826;

interface CursorState {
  x: number;
  y: number;
  lt: number;
}

export async function createAnimeStarfield(
  canvas: HTMLCanvasElement,
  opts: EngineOptions,
): Promise<AnimeStarfieldHandle | null> {
  // Chapter 4.2: the OffscreenCanvas worker keeps the background perfectly
  // smooth even under main-thread load. It is OPT-IN via
  // `localStorage.bgWorker = "1"` because a post-transfer GL failure inside
  // the worker can only degrade to the CSS layer. The default inline engine
  // is already 2 draw calls / < 0.05 ms CPU per frame, which meets the 80+
  // FPS targets on its own.
  const wantWorker = typeof localStorage !== "undefined" && localStorage.getItem("bgWorker") === "1";
  if (wantWorker) {
    const proxied = await tryWorkerEngine(canvas, opts);
    if (proxied) return proxied;
  }
  return createMainThreadEngine(canvas, opts);
}

/**
 * Worker proxy: probes the worker with ping/pong (canvas still untouched),
 * then transfers the canvas and mirrors the handle API over postMessage.
 * Resolves null on any pre-transfer failure so the caller can fall back to
 * the main-thread engine; post-transfer failures surface via opts.onFailed.
 */
async function tryWorkerEngine(
  canvas: HTMLCanvasElement,
  opts: EngineOptions,
): Promise<AnimeStarfieldHandle | null> {
  const transferFn = (canvas as HTMLCanvasElement & {
    transferControlToOffscreen?: () => OffscreenCanvas;
  }).transferControlToOffscreen;
  if (typeof transferFn !== "function" || typeof Worker === "undefined") return null;

  let worker: Worker;
  try {
    worker = new Worker(new URL("./bg.worker.ts", import.meta.url), { type: "module" });
  } catch {
    return null;
  }

  const pong = await new Promise<boolean>((resolve) => {
    const timer = window.setTimeout(() => resolve(false), 2500);
    worker.onmessage = (ev: MessageEvent) => {
      if ((ev.data as { type: string }).type === "pong") {
        window.clearTimeout(timer);
        resolve(true);
      }
    };
    worker.onerror = () => {
      window.clearTimeout(timer);
      resolve(false);
    };
    worker.postMessage({ type: "ping" });
  });
  if (!pong) {
    worker.terminate();
    return null;
  }

  let level = opts.tier >= 1 ? opts.tier : 8;
  let disposed = false;
  let curVp: Viewport = { x: 0, y: 0, zoom: 1 };
  const post = (msg: Record<string, unknown>, transfer?: Transferable[]): void => {
    if (!disposed) worker.postMessage(msg, transfer ?? []);
  };
  worker.onerror = () => {
    if (!disposed) opts.onFailed?.();
  };
  worker.onmessage = (ev: MessageEvent) => {
    const m = ev.data as { type: string; level?: number };
    if (m.type === "tier" && typeof m.level === "number") level = m.level;
    else if (m.type === "failed" && !disposed) opts.onFailed?.();
  };

  const off = transferFn.call(canvas);
  post(
    {
      type: "init", canvas: off, motion: opts.motion,
      mouseParallax: opts.mouseParallax, tier: opts.tier,
      widthCss: Math.max(320, window.innerWidth),
      heightCss: Math.max(240, window.innerHeight),
      dpr: window.devicePixelRatio || 1,
    },
    [off],
  );

  const onResize = (): void => post({
    type: "resize",
    widthCss: Math.max(320, window.innerWidth),
    heightCss: Math.max(240, window.innerHeight),
    dpr: window.devicePixelRatio || 1,
  });
    const onMouseMove = (e: MouseEvent): void => post({ type: "cursor", x: e.clientX, y: e.clientY });
    const onDocLeave = (): void => post({ type: "cursor", x: -9999, y: -9999 });
    const onVis = (): void => post({ type: "visibility", hidden: document.hidden });
    window.addEventListener("resize", onResize);
    window.addEventListener("mousemove", onMouseMove, { passive: true });
    document.documentElement.addEventListener("mouseleave", onDocLeave);
    document.addEventListener("visibilitychange", onVis);

  return {
    resize() { onResize(); },
    setOptions(next) {
      const { onFailed: _ignored, ...wire } = next;
      void _ignored;
      post({ type: "opts", ...wire });
    },
    setViewport(vp) {
      curVp = vp;
      post({ type: "vp", x: vp.x, y: vp.y, zoom: vp.zoom });
    },
    setTopology(centers) { post({ type: "topology", centers }); },
    getActiveTier: () => level,
    start() { post({ type: "visibility", hidden: false }); },
    stop() { post({ type: "visibility", hidden: true }); },
    renderOnce() { post({ type: "vp", ...curVp }); },
      dispose() {
        disposed = true;
        post({ type: "dispose" });
        window.removeEventListener("resize", onResize);
        window.removeEventListener("mousemove", onMouseMove);
        document.documentElement.removeEventListener("mouseleave", onDocLeave);
        document.removeEventListener("visibilitychange", onVis);
        window.setTimeout(() => worker.terminate(), 300);
      },
  };
}

function createMainThreadEngine(
  canvas: HTMLCanvasElement,
  opts: EngineOptions,
): AnimeStarfieldHandle | null {
  let gl: WebGL2RenderingContext | null = null;
  try {
    gl = canvas.getContext("webgl2", {
      alpha: false,
      antialias: false, // stars are SDF-smooth already; MSAA buys nothing
      powerPreference: "high-performance",
      preserveDrawingBuffer: false, // TEMP: visual verification export
    }) as WebGL2RenderingContext | null;
  } catch {
    gl = null;
  }
  if (!gl) return null;

  const gpu = createGpu(gl);
  if (!gpu) return null;
  const G = gpu; // alias keeps non-null narrowing inside every closure

  let o: EngineOptions = { ...opts };
  let activeLevel = o.tier >= 1 ? o.tier : 8; // auto boots mid-high

  const margin = 320;
  let viewW = Math.max(320, window.innerWidth);
  let viewH = Math.max(240, window.innerHeight);
  let spanW = viewW + margin * 2;
  let spanH = viewH + margin * 2;

  function buildStars(): void {
    spanW = viewW + margin * 2;
    spanH = viewH + margin * 2;
    const budget = TIER_SPECS[activeLevel - 1]!.starBudget;
    // Base simplex rifts ∪ two crossing galactic rivers: dense luminous
    // star settlements winding through clean empty voids.
    const { data } = generateStars({
      viewWidth: viewW,
      viewHeight: viewH,
      margin,
      seed: SEED,
      mask: makeStarfieldMask(SEED, spanW, spanH),
      targetCount: budget,
    });
    G.uploadStars(data);
    builtLevel = activeLevel;
  }
  let builtLevel = 0;
  let levelTimer = 0;
  buildStars();

  let disposed = false;
  let running = false;
  let rafId = 0;
  let lost = false;

  let vp: Viewport = { x: 0, y: 0, zoom: 1 };
  let lastZoom = vp.zoom;
  let zoomVel = 0;
  let lastMoveT = -1e9; // last WASD/zoom interaction (fast-move LOD window)
  const cur: CursorState = { x: -9999, y: -9999, lt: 0 };
  let mouseX = 0;
  let mouseY = 0;

  let cursorActive = false;

  // ---- auto-tier FPS monitor (smart downgrade / upgrade) ----
  let frameAccum = 0;
  let frameN = 0;
  let cooldown = 0;

  function evaluateFps(dtMs: number): void {
    const spec = TIER_SPECS[activeLevel - 1]!;
    if (!spec.continuous || cooldown > 0) {
      cooldown--;
      return;
    }
    frameAccum += dtMs;
    if (++frameN < 45) return;
    const avg = frameAccum / frameN;
    frameAccum = 0;
    frameN = 0;
    const fps = 1000 / Math.max(avg, 1e-3);
    if (o.tier !== 0) return; // manual lock — monitor only in auto mode
    if (fps < 80 && activeLevel > 4) {
      activeLevel--;
      cooldown = 90;
    } else if (fps > 112 && activeLevel < 10) {
      activeLevel++;
      cooldown = 180;
    }
  }

  const t0 = performance.now();
  let lastT = t0;

  function frame(nowMs: number, force = false): void {
    if (disposed || lost) return;
    // `force` lets renderOnce bake a frame while the loop is stopped
    // (L1 static tier, editing pauses, offline snapshot) — without it the
    // !running guard would silently swallow every direct draw request.
    if (!running && !force) return;
    const dt = nowMs - lastT;
    lastT = nowMs;
    evaluateFps(dt);

    // Tier changed (manual or auto-downgrade) → rebuild the star budget
    // once, debounced so rapid auto steps don't thrash generation.
    if (builtLevel !== activeLevel && levelTimer === 0) {
      levelTimer = window.setTimeout(() => {
        levelTimer = 0;
        if (!disposed) buildStars();
      }, 600);
    }

    // Smoothed zoom velocity feeds bokeh/dispersion (L7).
    const zDelta = vp.zoom - lastZoom;
    lastZoom = vp.zoom;
    zoomVel += (zDelta * 60 - zoomVel) * 0.18;

    // 4K-clarity ladder: the top tier supersamples up to 2.5× CSS (5× the
    // pixels of native 1080p), lower tiers stay at 2× to protect FPS.
    const dprCap = activeLevel >= 10 ? 2.5 : 2.0;
    const dpr = Math.min(window.devicePixelRatio || 1, dprCap) * TIER_SPECS[activeLevel - 1]!.dprScale;
    const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
    const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }

    const mx = (mouseX / window.innerWidth - 0.5) * o.mouseParallax;
    const my = -(mouseY / window.innerHeight - 0.5) * o.mouseParallax;
    const spec = TIER_SPECS[activeLevel - 1]!;
    const timeSec = (nowMs - t0) / 1000;
    // Fast-move LOD: while the canvas was flung within the last 150 ms (or
    // is still zooming hard), skip the L10 ribbon detail pass entirely.
    const fastMove = nowMs - lastMoveT < 150 || Math.abs(zoomVel) > 0.5;

    G.draw({
      time: timeSec,
      dpr,
      camX: vp.x + mx,
      camY: vp.y + my,
      spanW,
      spanH,
      margin,
      motion: o.motion,
      cursorX: cur.x,
      cursorY: cur.y,
      zoomVel,
      centerDim: o.editing,
      tier: spec,
      cursorActive,
      fastMove,
    });

    if (!spec.continuous) {
      // Level 1 baked-static: one clean frame per interaction, no idle loop.
      running = false;
      return;
    }
    rafId = requestAnimationFrame(frame);
  }

  function ensureLoop(): void {
    if (disposed || running || lost) return;
    const spec = TIER_SPECS[activeLevel - 1]!;
    if (!spec.continuous || o.motion <= 0) return;
    running = true;
    lastT = performance.now();
    rafId = requestAnimationFrame(frame);
  }

  const onLost = (e: Event): void => {
    e.preventDefault();
    lost = true;
  };
  const onRestored = (): void => {
    lost = false;
    ensureLoop();
    if (!running) renderOnce();
  };
  canvas.addEventListener("webglcontextlost", onLost);
  canvas.addEventListener("webglcontextrestored", onRestored);

  let regenTimer = 0;
  function onWinResize(): void {
    const nw = Math.max(320, window.innerWidth);
    const nh = Math.max(240, window.innerHeight);
    if (Math.abs(nw - viewW) / viewW < 0.12 && Math.abs(nh - viewH) / viewH < 0.12) {
      if (!running) renderOnce();
      return;
    }
    window.clearTimeout(regenTimer);
    regenTimer = window.setTimeout(() => {
      viewW = nw;
      viewH = nh;
      buildStars();
      if (!running) renderOnce();
    }, 250);
  }
  window.addEventListener("resize", onWinResize);

  function onMouseMove(e: MouseEvent): void {
    cur.x = e.clientX;
    cur.y = e.clientY;
    cur.lt = performance.now();
    mouseX = e.clientX;
    mouseY = e.clientY;
    cursorActive = true;
  }
  const onDocMouseLeave = (): void => {
    cursorActive = false;
  };
  window.addEventListener("mousemove", onMouseMove, { passive: true });
  document.documentElement.addEventListener("mouseleave", onDocMouseLeave);

  function renderOnce(): void {
    if (disposed || lost) return;
    frame(performance.now(), true);
  }

  return {
    resize() {
      onWinResize();
    },
    setOptions(next) {
      o = { ...o, ...next };
      if (o.tier >= 1) activeLevel = o.tier;
      else if (activeLevel < 4) activeLevel = 8; // re-entering auto
      if (o.motion <= 0) {
        this.stop();
        renderOnce();
      } else {
        ensureLoop();
        if (!running && !TIER_SPECS[activeLevel - 1]!.continuous) renderOnce();
      }
    },
    setViewport(next) {
      vp = next;
      lastMoveT = performance.now(); // fast-move LOD window restarts
      const spec = TIER_SPECS[activeLevel - 1]!;
      if (!running) {
        if (spec.continuous && o.motion > 0) ensureLoop();
        else renderOnce(); // L1: redraw the baked frame at the new offset
      }
    },
    setTopology(_centers) {
      void _centers; // gravity field removed (spec 0.1) — kept as a no-op
    },
    getActiveTier: () => activeLevel,
    start() {
      ensureLoop();
      if (!running) renderOnce();
    },
    stop() {
      running = false;
      cancelAnimationFrame(rafId);
    },
    renderOnce,
    dispose() {
      disposed = true;
      running = false;
      cancelAnimationFrame(rafId);
      window.clearTimeout(regenTimer);
      window.clearTimeout(levelTimer);
      window.removeEventListener("resize", onWinResize);
      window.removeEventListener("mousemove", onMouseMove);
      document.documentElement.removeEventListener("mouseleave", onDocMouseLeave);
      canvas.removeEventListener("webglcontextlost", onLost);
      canvas.removeEventListener("webglcontextrestored", onRestored);
      G.dispose();
      // NOTE: deliberately NOT calling WEBGL_lose_context.loseContext() here.
      // canvas.getContext() always hands back the SAME context object, so a
      // later engine on this canvas (React StrictMode double-mount, theme
      // toggles) would inherit a permanently lost context and render nothing.
      // Disposing only our own GL objects is safe and fully reversible.
    },
  };
}
