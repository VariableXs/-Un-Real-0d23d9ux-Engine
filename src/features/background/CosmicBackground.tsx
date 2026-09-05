import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { createAnimeStarfield, type AnimeStarfieldHandle } from "./starfield/engine";
import type { CustomBg, PerfMode } from "../../lib/settings";

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

/** Perf-mode presets map onto the ten-tier matrix; bgTier overrides. */
function resolveTier(props: BackgroundProps): number {
  if (props.bgTier >= 1) return Math.min(10, Math.round(props.bgTier));
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
      }),
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
      tier: resolveTier(props),
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
  }, [props.theme, props.safeMode, props.reduceMotion, props.perfMode === "static"]);

  // Live option updates without recreating the context.
  useEffect(() => {
    const h = handleRef.current;
    if (!h) return;
    const motion = effectiveMotion(props);
    h.setOptions({
      motion,
      mouseParallax: cb.parallaxStrength * 30,
      tier: resolveTier(props),
      editing: props.editing,
    });
    if (motion <= 0) h.stop();
    else h.start();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.perfMode, props.bgTier, props.editing, cb.dynamicStrength, cb.parallaxStrength, props.reduceMotion, props.safeMode]);

  const isStaticTheme = props.theme === "paper" || props.theme === "minimal-black";
  const overlay = props.starfieldOverlay === true && props.theme === "custom" && useCustomMedia;
  const showCanvas = props.theme === "deep-space" || overlay;
  const filters = `blur(${cb.blur}px) brightness(${cb.brightness}) saturate(${cb.saturation})`;

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
      <div className="bg-mask" style={{ opacity: cb.maskOpacity * (showCanvas ? 0.45 : 1) }} />
      <div className="bg-vignette" style={{ opacity: cb.vignette * (showCanvas ? 0.5 : 1) }} />
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
