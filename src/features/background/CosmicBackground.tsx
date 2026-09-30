import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { createAnimeStarfield, type AnimeStarfieldHandle } from "./starfield/engine";
import type { CustomBg, PerfMode } from "../../lib/settings";
import {
  detectAutoTier,
  persistDowngrade,
  resolveAutoTier,
  tierToBgTier,
  applyTierHints,
  type AutoTier,
} from "../../system/perf/autoTier";

export interface BackgroundProps {
  theme: string;
  perfMode: PerfMode;
  /** 1..10 fixed anime-starfield tier; 0 = smart auto monitor. */
  bgTier: number;
  reduceMotion: boolean;
  safeMode: boolean;
  editing: boolean; // typing → degrade animation
  customBg: CustomBg;
  /** 桌面混合壁纸模式：在自定义媒体之上叠加星空引擎（默认 false，四软件行为不变）。 */
  starfieldOverlay?: boolean;
  /**
   * 桌面壁纸原样渲染（批次E-13）：不叠加模糊/暗罩/暗角 —— blur、mask 是给
   * 应用内文字背景设计的，桌面壁纸应 1:1 清晰显示。仅桌面壁纸层传入，
   * 四款软件内部背景不经过这里，光影零变化。
   */
  plainMedia?: boolean;
}

function effectiveMotion(props: BackgroundProps): number {
  const starfieldActive = props.theme === "deep-space" || (props.theme === "custom" && props.starfieldOverlay === true);
  if (!starfieldActive) return 0;
  if (props.safeMode || props.reduceMotion) return 0;
  const modeFactor: Record<PerfMode, number> = { high: 1, balanced: 0.7, eco: 0.35, static: 0, auto: 0.7 };
  let f = modeFactor[props.perfMode];
  if (props.perfMode === "auto" && props.editing) f = 0.25;
  if (props.editing && (props.perfMode === "eco")) f = 0.12;
  return props.customBg.dynamicStrength * f;
}

/** Perf-mode presets map onto the ten-tier matrix; manual bgTier / L-2 auto tier override. */
function resolveTier(props: BackgroundProps, effTier: number | null): number {
  // L-2：手动覆盖优先（bgTier ≥1），否则自动档位；null = 既有 smart FPS monitor
  if (effTier !== null && effTier >= 0) return Math.min(10, Math.max(0, Math.round(effTier)));
  const byMode: Record<PerfMode, number> = {
    static: 1,
    eco: 3,
    balanced: 6,
    high: 10,
    auto: 0, // smart FPS monitor (L4..L10)
  };
  return byMode[props.perfMode];
}

/**
 * Layered background: WebGL2 anime-indigo starfield (deep-space) or
 * CSS/custom image/video layer, with a center darkening mask so text stays
 * readable. The starfield implements spec chapters 1-6: poisson-disk +
 * simplex-masked organic distribution, manga screentone, depth parallax
 * driven by the mindmap viewport, and the ten-tier performance matrix.
 */
export function CosmicBackground(props: BackgroundProps): React.ReactElement {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const handleRef = useRef<AnimeStarfieldHandle | null>(null);
  const [webglFailed, setWebglFailed] = useState(false);
  const [bgMissing, setBgMissing] = useState<{ path: string } | null>(null);
  const cb = props.customBg;

  // L-2 硬件自动分级：手动覆盖（bgTier≥1 / reduceMotion）优先，否则自动档；
  // 渲染首帧 >3s 看门狗降档一次并按 GPU 哈希持久记忆。
  const [autoTier, setAutoTier] = useState<AutoTier | null>(null);
  useEffect(() => {
    const res = resolveAutoTier(props.safeMode, props.bgTier, props.reduceMotion);
    applyTierHints(res.tier);
    setAutoTier(res.tier);
    // 探测只做一次（档位变化由降档/手动覆盖驱动）
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.safeMode]);

  const effTier =
    props.bgTier >= 1
      ? props.bgTier // 手动覆盖优先，不再被自动改
      : autoTier !== null
        ? tierToBgTier(autoTier)
        : null;
  const downgradeTier = (): void => {
    if (!autoTier || autoTier === "C") return;
    const order: AutoTier[] = ["C", "B", "A", "S"];
    const idx = Math.max(0, order.indexOf(autoTier) - 1);
    const next: AutoTier = order[idx] ?? "C";
    const det = detectAutoTier(props.safeMode);
    persistDowngrade(det.gpuHash, next);
    applyTierHints(next);
    setAutoTier(next);
  };

  const useCustomMedia = props.theme === "custom" && (cb.type === "image" || cb.type === "video");
  void setBgMissing;

  // Validate custom media existence whenever the path changes.
  useEffect(() => {
    if (!useCustomMedia) return;
    const p = cb.type === "image" ? cb.imagePath : cb.videoPath;
    if (!p) return;
    let alive = true;
    import("../../lib/ipc").then(({ ipc }) =>
      ipc.checkPaths([p]).then((res) => {
        if (alive && res.length > 0 && res[0] && !res[0].exists) setBgMissing({ path: p });
        else if (alive) setBgMissing(null);
      }).catch(() => {}),
    ).catch(() => {});
    return () => {
      alive = false;
    };
  }, [useCustomMedia, cb.imagePath, cb.videoPath, cb.type]);

  // WebGL lifecycle for deep-space theme (or desktop hybrid overlay).
  useEffect(() => {
    const starfieldActive = props.theme === "deep-space" || (props.theme === "custom" && props.starfieldOverlay === true);
    if (!starfieldActive) return;
    const canvas = canvasRef.current;
    if (!canvas || props.safeMode || props.reduceMotion || props.perfMode === "static") {
      handleRef.current?.dispose();
      handleRef.current = null;
      return;
    }
    let cancelled = false;
    let onViewport: ((e: Event) => void) | null = null;
    let onTopology: ((e: Event) => void) | null = null;
    const onResize = () => handleRef.current?.resize();
    const onVis = () => {
      if (document.hidden) handleRef.current?.stop();
      else handleRef.current?.start();
    };
    void createAnimeStarfield(canvas, {
      motion: effectiveMotion(props),
      mouseParallax: cb.parallaxStrength * 30,
      tier: resolveTier(props, effTier),
      editing: props.editing,
      // Worker/GL died after the canvas was transferred → CSS fallback.
      onFailed: () => setWebglFailed(true),
    }).then((handle) => {
      if (cancelled) {
        handle?.dispose();
        return;
      }
      if (!handle) {
        setWebglFailed(true);
        return;
      }
      handleRef.current = handle;
      handle.start();
      // L-2 首帧看门狗：>3s 未出帧 → 降档一次并持久记忆（仅自动模式）
      let firstFrame = false;
      const markFirst = (): void => {
        firstFrame = true;
      };
      const watchdog = window.setTimeout(() => {
        if (!firstFrame && props.bgTier === 0) downgradeTier();
      }, 3000);
      requestAnimationFrame(() => {
        window.clearTimeout(watchdog);
        markFirst();
      });

      // Mindmap canvas viewport drives depth parallax + zoom bokeh.
      onViewport = (e: Event): void => {
        const d = (e as CustomEvent<{ x: number; y: number; zoom: number }>).detail;
        handle.setViewport(d);
      };
      // L10 gravity field feed: sparse branch centroids of the current graph.
      onTopology = (e: Event): void => {
        const d = (e as CustomEvent<Array<{ x: number; y: number }>>).detail;
        handle.setTopology(d ?? []);
      };
      window.addEventListener("variable:mm-viewport", onViewport);
      window.addEventListener("variable:mm-topology", onTopology);
      window.addEventListener("resize", onResize);
      document.addEventListener("visibilitychange", onVis);
    });
    return () => {
      cancelled = true;
      if (onViewport) window.removeEventListener("variable:mm-viewport", onViewport);
      if (onTopology) window.removeEventListener("variable:mm-topology", onTopology);
      window.removeEventListener("resize", onResize);
      document.removeEventListener("visibilitychange", onVis);
      handleRef.current?.dispose();
      handleRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.theme, props.safeMode, props.reduceMotion, props.perfMode === "static", effTier]);

  // Live option updates without recreating the context.
  useEffect(() => {
    const h = handleRef.current;
    if (!h) return;
    const motion = effectiveMotion(props);
    h.setOptions({
      motion,
      mouseParallax: cb.parallaxStrength * 30,
      tier: resolveTier(props, effTier),
      editing: props.editing,
    });
    if (motion <= 0) h.stop();
    else h.start();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.perfMode, props.bgTier, props.editing, cb.dynamicStrength, cb.parallaxStrength, props.reduceMotion, props.safeMode, effTier]);

  const isStaticTheme = props.theme === "paper" || props.theme === "minimal-black";
  const overlay = props.starfieldOverlay === true && props.theme === "custom" && useCustomMedia;
  const showCanvas = props.theme === "deep-space" || overlay;
  const filters = props.plainMedia
    ? undefined
    : `blur(${cb.blur}px) brightness(${cb.brightness}) saturate(${cb.saturation})`;

  return (
    <div className={`bg-root theme-${props.theme}`} aria-hidden>
      {showCanvas && !webglFailed && (
        <canvas ref={canvasRef} className={`bg-canvas${overlay ? " bg-canvas-overlay" : ""}`} />
      )}
      {showCanvas && webglFailed && (
        <div className="bg-fallback-nebula">
          {/* Static CSS fallback when WebGL2 is unavailable */}
          <div className="fb-layer l1" />
          <div className="fb-layer l2" />
          <div className="fb-stars" />
        </div>
      )}
      {props.theme === "custom" && cb.type === "color" && (
        <div className="bg-solid" style={{ background: cb.color }} />
      )}
      {props.theme === "custom" && cb.type === "gradient" && (
        <div className="bg-solid" style={{ background: `linear-gradient(160deg, ${cb.gradientFrom}, ${cb.gradientTo})` }} />
      )}
      {useCustomMedia && cb.type === "image" && cb.imagePath && !bgMissing && (
        <div className="bg-media" style={{ filter: filters }}>
          <img src={toAssetUrl(cb.imagePath)} alt="" draggable={false} />
        </div>
      )}
      {useCustomMedia && cb.type === "video" && cb.videoPath && !bgMissing && cb.playVideo && !props.safeMode && (
        <div className="bg-media" style={{ filter: filters }}>
          <video src={toAssetUrl(cb.videoPath)} autoPlay loop muted playsInline />
        </div>
      )}
      {isStaticTheme && <div className="bg-grain" />}
      {/* The WebGL starfield already bakes its own vignette + edit-dim
          (chapter 2.4 / 6.1); the DOM overlays only assist at half strength
          so the scene is not double-darkened. */}
      <div className="bg-mask" style={{ opacity: props.plainMedia ? 0 : cb.maskOpacity * (showCanvas ? 0.45 : 1) }} />
      <div className="bg-vignette" style={{ opacity: props.plainMedia ? 0 : cb.vignette * (showCanvas ? 0.5 : 1) }} />
      {bgMissing && (
        <div className="bg-missing-note">
          背景文件缺失 / Missing background file:
          <code>{bgMissing.path}</code>
          <button type="button" className="btn tiny ghost" onClick={() => uiOpenSettings()}>
            设置 / Settings
          </button>
        </div>
      )}
    </div>
  );
}

function uiOpenSettings(): void {
  import("../../state/uiStore").then(({ uiStore }) => uiStore.setState({ settingsOpen: true }));
}

/** Convert an absolute local path to the Tauri asset protocol URL. */
export function toAssetUrl(absPath: string): string {
  return convertFileSrc(absPath);
}
