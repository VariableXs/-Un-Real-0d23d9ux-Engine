/**
 * N-11 实况场景渲染层（WeatherLayer，功能全景 L892-908）。
 *
 * - 天气粒子层：canvas 2D，四模式 rain / snow / fall-leaves / sunny + off；
 *   手工模式零网络（见 weatherState.ts 边界注释）；
 * - 点击穿透：层 pointer-events none（默认）/ auto（variable:scene:interactive）；
 * - 降级联动（perfGate 口径）：打字 / 全屏 → 粒子静止；
 *   prefers-reduced-motion → 粒子静止（只画一帧）；
 * - 昼夜引擎：每分钟刷新，对 document.documentElement 设
 *   --scene-tint（rgba 遮罩，夜灯 overlay 方式）与
 *   --scene-accent-shift（OKLCH hue 偏移，仅装饰；主控/主题消费可选）；
 * - z 序：portal 挂进 .desktop-shell（规格要求「壁纸之上、图标之下」：
 *   壁纸层 z-index 0、图标层 z-index 1，本层 z-index 0 且 DOM 序在壁纸之后 →
 *   恰好位于两者之间；找不到 .desktop-shell 时回退 body 并保持点击穿透）。
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { isDegradeActive, isTypingRecent } from "../wallpaper/perfGate";
import { sunPhase, seasonAccent, tintToRgba, PHASE_MASK_ALPHA } from "./daynight";
import { sceneT } from "./labels";
import {
  flagOn,
  loadWeather,
  SCENE_CHANGED_EVENT,
  SCENE_DAYNIGHT,
  SCENE_INTERACTIVE,
  SCENE_SEASON,
  sceneEnabled,
  type Weather,
} from "./weatherState";

interface Particle {
  x: number;
  y: number;
  vy: number;
  r: number;
  seed: number;
  spin: number;
}

function spawn(kind: Weather, w: number, h: number): Particle[] {
  const counts: Record<Weather, number> = { off: 0, sunny: 22, rain: 130, snow: 95, "fall-leaves": 42 };
  const out: Particle[] = [];
  for (let i = 0; i < counts[kind]; i++) {
    out.push({
      x: Math.random() * w,
      y: Math.random() * h,
      vy: 0,
      r: 1,
      seed: Math.random() * Math.PI * 2,
      spin: Math.random() * Math.PI * 2,
    });
  }
  return out;
}

/** 每帧步进 + 绘制（dt 秒）。 */
function stepAndDraw(
  ctx: CanvasRenderingContext2D,
  kind: Weather,
  ps: Particle[],
  w: number,
  h: number,
  dt: number,
  time: number,
): void {
  ctx.clearRect(0, 0, w, h);
  if (kind === "off") return;

  if (kind === "rain") {
    ctx.strokeStyle = "rgba(160, 190, 235, 0.5)";
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    for (const p of ps) {
      p.vy = 820 + (p.seed % 1) * 260;
      const vx = 70;
      p.x += vx * dt;
      p.y += p.vy * dt;
      if (p.y > h + 20) {
        p.y = -20;
        p.x = Math.random() * w;
      }
      if (p.x > w + 20) p.x = -20;
      const len = 14 + (p.seed % 1) * 10;
      ctx.moveTo(p.x, p.y);
      ctx.lineTo(p.x + 4, p.y - len);
    }
    ctx.stroke();
    return;
  }

  if (kind === "snow") {
    ctx.fillStyle = "rgba(235, 242, 255, 0.85)";
    for (const p of ps) {
      p.r = 1.2 + (p.seed % 1) * 2.2;
      p.vy = 40 + (p.seed % 1) * 55;
      p.x += Math.sin(time * 0.8 + p.seed) * 24 * dt;
      p.y += p.vy * dt;
      if (p.y > h + 6) {
        p.y = -6;
        p.x = Math.random() * w;
      }
      ctx.beginPath();
      ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
      ctx.fill();
    }
    return;
  }

  if (kind === "fall-leaves") {
    const colors = ["rgba(196,124,62,0.9)", "rgba(168,102,52,0.9)", "rgba(214,158,88,0.9)", "rgba(150,86,44,0.85)"];
    for (const p of ps) {
      p.r = 4 + (p.seed % 1) * 4;
      p.vy = 60 + (p.seed % 1) * 50;
      p.x += Math.sin(time * 1.1 + p.seed) * 46 * dt;
      p.y += p.vy * dt;
      p.spin += dt * (1 + (p.seed % 1));
      if (p.y > h + 10) {
        p.y = -10;
        p.x = Math.random() * w;
      }
      ctx.save();
      ctx.translate(p.x, p.y);
      ctx.rotate(p.spin);
      ctx.fillStyle = colors[Math.floor(p.seed) % colors.length] as string;
      ctx.beginPath();
      ctx.ellipse(0, 0, p.r, p.r * 0.45, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }
    return;
  }

  // sunny：晴时光斑（暖金柔光，缓慢上浮）
  for (const p of ps) {
    p.r = 10 + (p.seed % 1) * 26;
    p.vy = -(6 + (p.seed % 1) * 10);
    p.x += Math.sin(time * 0.5 + p.seed) * 8 * dt;
    p.y += p.vy * dt;
    if (p.y < -40) {
      p.y = h + 40;
      p.x = Math.random() * w;
    }
    const g = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, p.r);
    g.addColorStop(0, "rgba(255, 226, 160, 0.28)");
    g.addColorStop(1, "rgba(255, 226, 160, 0)");
    ctx.fillStyle = g;
    ctx.beginPath();
    ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
    ctx.fill();
  }
}

function SceneLayer(): React.ReactElement {
  const t = useMemo(() => sceneT(), []);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const lastKeyAt = useRef(0);
  const fullscreenRef = useRef(false);
  const [enabled, setEnabled] = useState(() => sceneEnabled());
  const [weather, setWeather] = useState<Weather>(() => loadWeather());
  const [interactive, setInteractive] = useState(() => flagOn(SCENE_INTERACTIVE));
  const [daynight, setDaynight] = useState(() => flagOn(SCENE_DAYNIGHT));
  const [season, setSeason] = useState(() => flagOn(SCENE_SEASON));
  const [mask, setMask] = useState("");
  const [reduced, setReduced] = useState(
    () => typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  // 仅用于「粒子已静止」提示的重渲染节拍（500ms）；粒子循环本身读 ref，不受影响
  const [gateTick, setGateTick] = useState(0);

  // 配置刷新：面板派发 variable:scene-changed / 其他窗口 storage 同步
  useEffect(() => {
    const refresh = (): void => {
      setEnabled(sceneEnabled());
      setWeather(loadWeather());
      setInteractive(flagOn(SCENE_INTERACTIVE));
      setDaynight(flagOn(SCENE_DAYNIGHT));
      setSeason(flagOn(SCENE_SEASON));
    };
    window.addEventListener(SCENE_CHANGED_EVENT, refresh);
    window.addEventListener("storage", refresh);
    return () => {
      window.removeEventListener(SCENE_CHANGED_EVENT, refresh);
      window.removeEventListener("storage", refresh);
    };
  }, []);

  // 打字源：keydown 时间戳（最近 1s 有按键 = 输入态，perfGate 口径）
  useEffect(() => {
    const onKey = (): void => {
      lastKeyAt.current = Date.now();
    };
    window.addEventListener("keydown", onKey);
    const timer = window.setInterval(() => setGateTick((n) => n + 1), 500);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.clearInterval(timer);
    };
  }, []);

  // 全屏源：sys://fullscreen（DesktopShell 同款；非 Tauri 环境视为非全屏）
  useEffect(() => {
    let disposed = false;
    let un: (() => void) | null = null;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<boolean>("sys://fullscreen", (e) => {
          if (!disposed) fullscreenRef.current = e.payload === true;
        }),
      )
      .then((fn) => {
        if (disposed) fn();
        else un = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, []);

  // 减动效
  useEffect(() => {
    if (typeof matchMedia === "undefined") return;
    const mq = matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = (): void => setReduced(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  // 昼夜：每分钟刷新 --scene-tint / --scene-accent-shift（documentElement 级，主题可消费）
  useEffect(() => {
    const apply = (): void => {
      const rootStyle = document.documentElement.style;
      if (enabled && daynight) {
        const { phase, tint } = sunPhase(new Date());
        const rgba = tintToRgba(tint, PHASE_MASK_ALPHA[phase]);
        rootStyle.setProperty("--scene-tint", rgba);
        setMask(rgba);
      } else {
        rootStyle.setProperty("--scene-tint", "transparent");
        setMask("");
      }
      if (enabled && season) {
        const { hueShift } = seasonAccent(new Date());
        rootStyle.setProperty("--scene-accent-shift", `${hueShift}deg`);
      } else {
        rootStyle.setProperty("--scene-accent-shift", "0deg");
      }
    };
    apply();
    const timer = window.setInterval(apply, 60_000);
    return () => window.clearInterval(timer);
  }, [enabled, daynight, season]);

  // 粒子主循环（静止条件：off / 打字 / 全屏 / 减动效 → 只画一帧；恢复后自动续播）
  useEffect(() => {
    if (!enabled) return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const resize = (): void => {
      const parent = canvas.parentElement;
      const w = parent?.clientWidth || window.innerWidth;
      const h = parent?.clientHeight || window.innerHeight;
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      canvas.width = Math.max(1, Math.round(w * dpr));
      canvas.height = Math.max(1, Math.round(h * dpr));
    };
    resize();

    let ps = spawn(weather, canvas.width, canvas.height);
    const isStatic = (): boolean =>
      weather === "off" ||
      reduced ||
      isDegradeActive({
        typing: isTypingRecent(lastKeyAt.current, Date.now()),
        fullscreen: fullscreenRef.current,
        battery: false, // 场景粒子开销极小，不随电池暂停（视频引擎才是电池红线）
      }).particlesStatic;

    let raf = 0;
    let last = performance.now();
    const frame = (now: number): void => {
      raf = requestAnimationFrame(frame);
      if (isStatic()) return; // 保持最后一帧（静态帧），恢复后自动续播
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      stepAndDraw(ctx, weather, ps, canvas.width, canvas.height, dt, now / 1000);
    };

    stepAndDraw(ctx, weather, ps, canvas.width, canvas.height, 0, 0); // 先画一帧（静止口径下即最终帧）
    if (!isStatic()) raf = requestAnimationFrame(frame);

    const onResize = (): void => {
      resize();
      ps = spawn(weather, canvas.width, canvas.height);
      stepAndDraw(ctx, weather, ps, canvas.width, canvas.height, 0, 0);
    };
    window.addEventListener("resize", onResize);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", onResize);
      ctx.clearRect(0, 0, canvas.width, canvas.height);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, weather, reduced]);

  const gate = isDegradeActive({
    typing: isTypingRecent(lastKeyAt.current, Date.now()),
    fullscreen: fullscreenRef.current,
    battery: false,
  });

  if (!enabled) {
    // 关闭状态保持挂载（监听配置事件，开关切换不重载模块）；渲染零输出
    void gateTick;
    return createPortal(null, document.body);
  }

  return createPortal(
    <div
      className="scene-layer"
      style={{ pointerEvents: interactive ? "auto" : "none" }}
      aria-hidden={!interactive}
    >
      {/* 昼夜色温遮罩（z 序：壁纸 0 → 本层 0（DOM 序在后）→ 图标 1 → 窗口 20+） */}
      <div className="scene-tint" style={{ background: mask }} aria-hidden />
      <canvas ref={canvasRef} className="scene-canvas" aria-hidden />
      {gate.degraded && weather !== "off" && <span className="scene-hint">{t("particleStatic")}</span>}
    </div>,
    document.querySelector(".desktop-shell") ?? document.body,
  );
}

export function WeatherLayer(): React.ReactElement {
  return <SceneLayer />;
}