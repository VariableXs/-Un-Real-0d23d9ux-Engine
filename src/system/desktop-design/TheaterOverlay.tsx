/**
 * AURORA-10000 · AI-11~AI-15 · 族0074 桌面剧场模式（全屏场景 overlay）。
 * 自挂载协议：ai04:open-feature { feature: "design-theater" }；
 * Esc / 关闭按钮退出（ai04:close-feature）。粒子 + 音景来自选中场景档 params。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { activeEntries, designStore } from "./state";
import { AmbienceSynth, type AmbienceKind } from "./ambience";
import { ParticleEngine, type ParticleKind } from "./particles";
import { useLaneLang, LABELS } from "./labels";
import { dispatchClose, installCloseHandler } from "../wallpaper/mount";

type SceneParams = { fx: ParticleKind; sound: AmbienceKind; palette: [string, string] };

function activeScene(): { params: SceneParams; label: string } | null {
  const sel = designStore.getState().selections["f0074"];
  if (!sel) return null;
  const ent = activeEntries(designStore.getState()).find((e) => e.id === sel);
  if (!ent) return null;
  const p = ent.params ?? {};
  const fx = (typeof p.fx === "string" ? p.fx : "stars") as ParticleKind;
  const sound = (typeof p.sound === "string" ? p.sound : "none") as AmbienceKind;
  const palette = Array.isArray(p.palette) && p.palette.length === 2 ? (p.palette as [string, string]) : (["oklch(0.1 0.01 262)", "oklch(0.8 0.05 262)"] as [string, string]);
  return { params: { fx, sound, palette }, label: ent.label.zh };
}

export function TheaterOverlay(): React.JSX.Element {
  const lang = useLaneLang();
  const L = LABELS[lang];
  const scene = useMemo(activeScene, []);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const engineRef = useRef<ParticleEngine | null>(null);
  const synthRef = useRef<AmbienceSynth | null>(null);
  const [entered, setEntered] = useState(false);

  useEffect(() => {
    const off = installCloseHandler(window, "design-theater");
    return off;
  }, []);

  // 播放/停止分两段：先渲染遮罩，用户点击进入后才启动音频（自动播放合规）
  useEffect(() => {
    if (!scene || !canvasRef.current) return;
    const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    const engine = new ParticleEngine({ maxFps: 60, reducedMotion: reduced });
    engine.attach(canvasRef.current, entered ? scene.params.fx : "none");
    engineRef.current = engine;
    const onResize = (): void => engine.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      engine.destroy();
    };
  }, [scene, entered]);

  useEffect(() => {
    if (!scene || !entered) return;
    const synth = new AmbienceSynth();
    synth.play(scene.params.sound);
    synthRef.current = synth;
    return () => synth.stop();
  }, [scene, entered]);

  if (!scene) {
    return (
      <div className="w2-theater" role="dialog" aria-label={L.title}>
        <p>{lang === "en" ? "Pick a theater scene in Design Center first (Family 0074)." : "请先在设计中心 · 族0074 选择一个剧场场景。"}</p>
        <button type="button" onClick={() => dispatchClose("design-theater")} aria-label={L.close}><X size={16} aria-hidden="true" />{L.close}</button>
      </div>
    );
  }

  return (
    <div
      className="w2-theater"
      role="dialog"
      aria-label={`${L.title} · ${scene.label}`}
      style={{ background: `linear-gradient(180deg, ${scene.params.palette[0]}, ${scene.params.palette[1]}22)` }}
    >
      <canvas ref={canvasRef} className="w2-theater-canvas" aria-hidden="true" />
      {!entered && (
        <button type="button" className="w2-theater-enter" onClick={() => setEntered(true)}>
          {L.theaterOpen} · {scene.label}
        </button>
      )}
      <button
        type="button"
        className="w2-theater-close"
        aria-label={L.theaterStop}
        onClick={() => {
          synthRef.current?.stop();
          dispatchClose("design-theater");
        }}
      >
        <X size={16} aria-hidden="true" /> {L.theaterStop}
      </button>
    </div>
  );
}

