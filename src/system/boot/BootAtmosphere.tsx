import { useEffect, useRef } from "react";

/**
 * U-01 启动氛围层（实机反馈：开场动画重做——动态、真实、沉浸、电影级）。
 *
 * 红线不动：进度/日志仍 100% 后端真实事件驱动，本层是纯装饰性氛围，
 * 绝不伪造进度时间线。
 *
 * 氛围构成（电影布光法 + 游戏加载屏质地）：
 * - 深空尘埃场：Canvas 2D 三层深度光尘，30fps 上限（壁纸级功耗，不抢加载预算），
 *   亮度随真实进度微升（dustBoost 0.97→1.05）
 * - 偶发流星：真实随机间隔 6–14s 一颗（与加载进度无关的纯氛围，非时间线），
 *   尾迹渐隐 + 头部 6px 径向辉光 + 亮点
 * - 极光层：两片 token 色柔光极缓旋转漂移（transform-only，60s/90s 周期）
 * - 扫描线：3px 节距静态细纹（CRT/游戏加载屏质地，无动画）
 * - 晕影 + 地平线微光：四周压暗聚焦字标（底部一点暖光接地）
 * - 胶片颗粒：feTurbulence 噪点 4% 透明度步进抖动（真实胶片质感）
 * - 氛围强度随真实进度微升（--atm-boost 0.35→0.55）：视觉与真实加载挂钩
 * - 诚实降级：reduce-motion / safeMode / static 档 → 粒子与流星关闭；
 *   极光与颗粒是 CSS 常驻层（reduce-motion 下也停摆）。
 */

interface Mote {
  x: number;
  y: number;
  z: number; // 深度 0.15..1（亮度与视差权重）
  r: number;
  vx: number;
  vy: number;
  phase: number;
  sway: number;
  tw: number;
}

interface Comet {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number; // 0..1 递减
  len: number; // 尾长 px
}

function budgetFor(perfMode: string | undefined, reduceMotion: boolean, safeMode: boolean): number {
  if (reduceMotion || safeMode) return 0;
  switch (perfMode) {
    case "high": return 90;
    case "balanced": return 60;
    case "eco": return 30;
    case "static": return 0;
    default: return 60;
  }
}

export function BootAtmosphere(props: {
  /** 0..1 真实进度（rAF 平滑值）：氛围强度微升（装饰反馈，不替代进度条）。 */
  progress: number;
  reduceMotion?: boolean;
  safeMode?: boolean;
  perfMode?: string;
}): React.ReactElement {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const motesRef = useRef<Mote[]>([]);
  const cometRef = useRef<Comet | null>(null);
  const nextCometAtRef = useRef(0);
  const boostRef = useRef(0.35); // 真实进度 → 尘埃亮度微升（canvas 侧，与 CSS --atm-boost 同源）
  const budget = budgetFor(props.perfMode, props.reduceMotion === true, props.safeMode === true);

  // 初始化粒子群（budget 变化时重建；归一化坐标，resize 不重排）
  useEffect(() => {
    if (budget <= 0) {
      motesRef.current = [];
      return;
    }
    const motes: Mote[] = [];
    for (let i = 0; i < budget; i++) {
      motes.push({
        x: Math.random(),
        y: Math.random(),
        z: 0.15 + Math.random() * 0.85,
        r: 0.4 + Math.random() * 1.4,
        vx: (Math.random() - 0.5) * 0.004,
        vy: -(0.001 + Math.random() * 0.005),
        phase: Math.random() * Math.PI * 2,
        sway: 0.002 + Math.random() * 0.008,
        tw: 0.25 + Math.random() * 0.8,
      });
    }
    motesRef.current = motes;
  }, [budget]);

  // 真实进度 → 氛围强度（CSS 变量驱动，不触发 canvas 重排；同步 canvas 亮度源）
  useEffect(() => {
    const p = Math.min(1, Math.max(0, props.progress));
    const boost = 0.35 + p * 0.2;
    boostRef.current = boost;
    const el = rootRef.current;
    if (el) el.style.setProperty("--atm-boost", boost.toFixed(3));
  }, [props.progress]);

  // Canvas 尘埃场 + 偶发流星（30fps 上限；hidden 暂停）
  useEffect(() => {
    if (budget <= 0) return;
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

    let raf = 0;
    let last = 0;
    let running = !document.hidden;
    const FRAME_MS = 1000 / 30;
    nextCometAtRef.current = performance.now() + 4000 + Math.random() * 6000;

    const spawnComet = (now: number): void => {
      // 顶部 1/3 区域入场，斜向 35–55°，速度 ~1.2px/ms(dpr 缩放)
      const fromLeft = Math.random() < 0.5;
      const sp = (1.0 + Math.random() * 0.5) * dpr;
      const ang = (30 + Math.random() * 25) * (Math.PI / 180);
      cometRef.current = {
        x: (fromLeft ? 0.08 + Math.random() * 0.3 : 0.62 + Math.random() * 0.3) * w,
        y: (0.02 + Math.random() * 0.25) * h,
        vx: Math.cos(ang) * sp * (fromLeft ? 1 : -1),
        vy: Math.sin(ang) * sp,
        life: 1,
        len: (60 + Math.random() * 90) * dpr,
      };
      nextCometAtRef.current = now + 6000 + Math.random() * 8000;
    };

    const loop = (now: number): void => {
      if (!running) return;
      raf = requestAnimationFrame(loop);
      if (last === 0) {
        last = now;
        return;
      }
      const dtMs = now - last;
      if (dtMs < FRAME_MS - 2) return;
      const dt = Math.min(0.1, dtMs / 1000);
      last = now;
      resize();
      const t = now / 1000;

      ctx.clearRect(0, 0, w, h);
      // 尘埃光点（亮度随真实进度微升：boost 0.35→0.55 ⇒ 系数 0.96→1.05）
      const dustBoost = 0.82 + boostRef.current * 0.42;
      for (const p of motesRef.current) {
        p.x += (p.vx + Math.sin(t * 0.35 + p.phase) * p.sway) * dt;
        p.y += p.vy * dt;
        if (p.y < -0.04) {
          p.y = 1.04;
          p.x = Math.random();
        }
        if (p.x < -0.04) p.x = 1.04;
        else if (p.x > 1.04) p.x = -0.04;
        const px = p.x * w;
        const py = p.y * h;
        const twinkle = 0.3 + 0.7 * (0.5 + 0.5 * Math.sin(t * p.tw * Math.PI * 2 + p.phase));
        const alpha = Math.min(0.6, (0.05 + p.z * 0.3) * twinkle * dustBoost);
        ctx.beginPath();
        ctx.arc(px, py, p.r * p.z * dpr, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(226, 236, 252, ${alpha.toFixed(3)})`;
        ctx.fill();
      }
      // 流星：头部辉光（径向渐变）+ 尾迹渐隐 + 头部亮点（唯一瞬时态元素）
      const comet = cometRef.current;
      if (comet) {
        comet.x += comet.vx * dtMs;
        comet.y += comet.vy * dtMs;
        comet.life -= dt * 0.55;
        if (comet.life <= 0 || comet.x < -80 || comet.x > w + 80 || comet.y > h + 80) {
          cometRef.current = null;
        } else {
          const nx = comet.vx / Math.hypot(comet.vx, comet.vy);
          const ny = comet.vy / Math.hypot(comet.vx, comet.vy);
          const tx = comet.x - nx * comet.len;
          const ty = comet.y - ny * comet.len;
          const g = ctx.createLinearGradient(comet.x, comet.y, tx, ty);
          g.addColorStop(0, `rgba(230, 240, 255, ${(0.5 * comet.life).toFixed(3)})`);
          g.addColorStop(1, "rgba(230, 240, 255, 0)");
          ctx.strokeStyle = g;
          ctx.lineWidth = 1.2 * dpr;
          ctx.beginPath();
          ctx.moveTo(comet.x, comet.y);
          ctx.lineTo(tx, ty);
          ctx.stroke();
          // 头部辉光：6px 径向柔光（低于尾迹亮度，只做质感不加戏）
          const hr = 6 * dpr;
          const hg = ctx.createRadialGradient(comet.x, comet.y, 0, comet.x, comet.y, hr);
          hg.addColorStop(0, `rgba(230, 240, 255, ${(0.3 * comet.life).toFixed(3)})`);
          hg.addColorStop(1, "rgba(230, 240, 255, 0)");
          ctx.fillStyle = hg;
          ctx.beginPath();
          ctx.arc(comet.x, comet.y, hr, 0, Math.PI * 2);
          ctx.fill();
          ctx.beginPath();
          ctx.arc(comet.x, comet.y, 1.4 * dpr, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(240, 246, 255, ${(0.85 * comet.life).toFixed(3)})`;
          ctx.fill();
        }
      } else if (now >= nextCometAtRef.current) {
        spawnComet(now);
      }
    };
    raf = requestAnimationFrame(loop);

    const onVis = (): void => {
      running = !document.hidden;
      if (running) {
        last = 0;
        raf = requestAnimationFrame(loop);
      }
    };
    document.addEventListener("visibilitychange", onVis);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
      document.removeEventListener("visibilitychange", onVis);
    };
  }, [budget]);

  const staticOnly = budget <= 0;
  return (
    <div ref={rootRef} className={`boot-atmosphere${staticOnly ? " static-only" : ""}`} aria-hidden="true">
      {/* 极光层 ×2：token 色柔光极缓旋转（transform-only） */}
      <div className="boot-aurora a1" />
      <div className="boot-aurora a2" />
      {/* 尘埃 + 流星画布（30fps） */}
      {!staticOnly && <canvas ref={canvasRef} className="boot-atm-canvas" />}
      {/* 扫描线：3px 节距静态细纹（CRT/游戏加载屏质地，无动画） */}
      <div className="boot-scan" />
      {/* 地平线微光：字标下方一点暖光接地 */}
      <div className="boot-horizon" />
      {/* 晕影：四周压暗聚焦中央（电影布光） */}
      <div className="boot-vignette" />
      {/* 胶片颗粒：feTurbulence 噪点步进抖动 */}
      <div className="boot-grain" />
    </div>
  );
}
