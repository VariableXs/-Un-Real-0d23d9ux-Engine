import { useEffect, useMemo, useRef, useState } from "react";
import { toAssetUrl } from "../../features/background/CosmicBackground";
import { ipc } from "../../lib/ipc";

/**
 * 实机反馈：Windows 桌面动态壁纸（Wallpaper Engine 等）在 Variable 里变静态。
 * living 模式 = 图片壁纸活化层，任何静态图都有呼吸感：
 *
 * - 图片本体：Ken Burns 极缓缩放/漂移（60–90s 往返，transform-only 零重排）；
 *   漂移方位按图片路径哈希取种（每张壁纸方向不同，观感不重复）。
 * - 活化粒子层：Canvas 2D 尘埃光点（三层深度视差 + 柔光晕），30fps 上限——
 *   壁纸是氛围不是主角，省下的帧留给窗口。
 * - GIF 壁纸天然活化（<img> 原生播放动图），粒子层照常叠加。
 * - 诚实降级：reduce-motion / safeMode / static 档 → 图片静止、粒子关闭；
 *   图片缺失 → 纯黑（绝不白屏）。
 */
export function LivingWallpaper(props: {
  imagePath: string;
  reduceMotion?: boolean;
  safeMode?: boolean;
  perfMode?: string;
}): React.ReactElement {
  const { imagePath } = props;
  const [missing, setMissing] = useState(false);

  useEffect(() => {
    if (!imagePath) return;
    let alive = true;
    ipc.checkPaths([imagePath]).then((res) => {
      if (alive) setMissing(!(res.length > 0 && res[0]?.exists));
    }).catch(() => {});
    return () => {
      alive = false;
    };
  }, [imagePath]);

  /** Ken Burns 方位种子：路径哈希 → 8 向漂移（每张壁纸方向不同）。 */
  const drift = useMemo(() => {
    let h = 2166136261;
    for (let i = 0; i < imagePath.length; i++) {
      h ^= imagePath.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    const dir = Math.abs(h) % 8;
    const pans: Array<[number, number]> = [
      [1, 0], [0.7, 0.7], [0, 1], [-0.7, 0.7], [-1, 0], [-0.7, -0.7], [0, -1], [0.7, -0.7],
    ];
    const [dx, dy] = pans[dir] ?? [1, 0];
    return { dx, dy, dur: 60 + (Math.abs(h >> 8) % 30) };
  }, [imagePath]);

  if (!imagePath || missing) {
    return <div className="wallpaper wallpaper-solid" aria-hidden />;
  }

  return (
    <div className="wallpaper wallpaper-living" aria-hidden>
      <div
        className="living-media"
        style={{
          ["--kb-dx" as string]: String(drift.dx),
          ["--kb-dy" as string]: String(drift.dy),
          ["--kb-dur" as string]: `${drift.dur}s`,
        }}
      >
        <img src={toAssetUrl(imagePath)} alt="" draggable={false} />
      </div>
      <LivingParticles
        reduceMotion={props.reduceMotion}
        safeMode={props.safeMode}
        perfMode={props.perfMode}
      />
    </div>
  );
}

/** 粒子分档：high=全量 / balanced=中量 / eco=轻量 / static·auto(static 解析)=关。 */
function particleBudget(perfMode: string | undefined, reduceMotion?: boolean, safeMode?: boolean): number {
  if (reduceMotion || safeMode) return 0;
  switch (perfMode) {
    case "high": return 110;
    case "balanced": return 72;
    case "eco": return 34;
    case "static": return 0;
    default: return 72; // auto 未定档前按 balanced 起步
  }
}

interface Mote {
  x: number; y: number;      // 归一化坐标 0..1
  z: number;                 // 深度 0.2..1（视差与亮度权重）
  r: number;                // 基础半径 px
  vx: number; vy: number;    // 归一化速度/秒
  phase: number;            // 摆动相位
  sway: number;              // 摆动幅度
  tw: number;                // 闪烁频率
}

function LivingParticles(props: {
  reduceMotion?: boolean;
  safeMode?: boolean;
  perfMode?: string;
}): React.ReactElement | null {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const budget = particleBudget(props.perfMode, props.reduceMotion, props.safeMode);
  const enabled = budget > 0;

  useEffect(() => {
    if (!enabled) return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d", { alpha: true });
    if (!ctx) return;

    const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
    let w = 0;
    let h = 0;
    const resize = (): void => {
      w = Math.max(1, Math.round(canvas.clientWidth * dpr));
      h = Math.max(1, Math.round(canvas.clientHeight * dpr));
      if (canvas.width !== w) canvas.width = w;
      if (canvas.height !== h) canvas.height = h;
    };
    resize();
    window.addEventListener("resize", resize);

    // 粒子群：上升尘埃 + 少量大光斑（bokeh），全部归一化坐标（resize 不重排）
    const motes: Mote[] = [];
    for (let i = 0; i < budget; i++) {
      const bokeh = i % 9 === 0; // 每 9 颗 1 颗大光斑
      motes.push({
        x: Math.random(),
        y: Math.random(),
        z: 0.2 + Math.random() * 0.8,
        r: bokeh ? 2.5 + Math.random() * 3.5 : 0.4 + Math.random() * 1.3,
        vx: (Math.random() - 0.5) * 0.006,
        vy: -(0.002 + Math.random() * 0.008), // 缓慢上浮
        phase: Math.random() * Math.PI * 2,
        sway: 0.004 + Math.random() * 0.01,
        tw: 0.3 + Math.random() * 0.9,
      });
    }

    // 鼠标视差：偏移量（归一化 -0.5..0.5 × 深度）
    let mx = 0;
    let my = 0;
    let tmx = 0;
    let tmy = 0;
    const onMove = (e: PointerEvent): void => {
      tmx = e.clientX / window.innerWidth - 0.5;
      tmy = e.clientY / window.innerHeight - 0.5;
    };
    window.addEventListener("pointermove", onMove, { passive: true });

    const FRAME_MS = 1000 / 30; // 30fps：壁纸粒子不需要 60
    let raf = 0;
    let last = 0;
    let running = !document.hidden;
    const onVis = (): void => {
      running = !document.hidden;
      if (running) {
        last = 0;
        raf = requestAnimationFrame(loop);
      }
    };
    document.addEventListener("visibilitychange", onVis);

    const loop = (now: number): void => {
      if (!running) return;
      raf = requestAnimationFrame(loop);
      if (last === 0) {
        last = now;
        return;
      }
      const dt = Math.min(0.1, (now - last) / 1000);
      if (dt < FRAME_MS / 1000 - 0.002) return;
      last = now;
      resize();

      // 视差缓动
      mx += (tmx - mx) * 0.04;
      my += (tmy - my) * 0.04;

      ctx.clearRect(0, 0, w, h);
      const t = now / 1000;
      for (const p of motes) {
        p.x += (p.vx + Math.sin(t * 0.4 + p.phase) * p.sway) * dt;
        p.y += p.vy * dt;
        // 环绕：上浮出顶 → 回到底部
        if (p.y < -0.05) {
          p.y = 1.05;
          p.x = Math.random();
        }
        if (p.x < -0.05) p.x = 1.05;
        else if (p.x > 1.05) p.x = -0.05;

        const px = (p.x + mx * 0.03 * p.z) * w;
        const py = (p.y + my * 0.03 * p.z) * h;
        const twinkle = 0.35 + 0.65 * (0.5 + 0.5 * Math.sin(t * p.tw * Math.PI * 2 + p.phase));
        const alpha = Math.min(0.5, 0.08 + p.z * 0.22) * twinkle;
        const r = p.r * p.z * dpr;
        ctx.beginPath();
        ctx.arc(px, py, r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(255, 252, 244, ${alpha.toFixed(3)})`;
        ctx.fill();
        // 柔光晕：大光斑加径向渐变，小尘埃用 shadow（省一次渐变对象）
        if (p.r > 2) {
          const g = ctx.createRadialGradient(px, py, 0, px, py, r * 3.2);
          g.addColorStop(0, `rgba(255, 250, 235, ${(alpha * 0.5).toFixed(3)})`);
          g.addColorStop(1, "rgba(255, 250, 235, 0)");
          ctx.fillStyle = g;
          ctx.beginPath();
          ctx.arc(px, py, r * 3.2, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    };
    raf = requestAnimationFrame(loop);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
      window.removeEventListener("pointermove", onMove);
      document.removeEventListener("visibilitychange", onVis);
    };
  }, [enabled, budget]);

  if (!enabled) return null;
  return <canvas ref={canvasRef} className="living-particles" aria-hidden />;
}
