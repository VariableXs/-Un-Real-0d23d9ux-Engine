/**
 * AURORA-10000 · AI-11~AI-15 车道 · 粒子引擎（族0073 节令层 / 族0074 剧场共用）。
 * 单 canvas + rAF；kind 决定粒子形态；reduced-motion 或 hidden 时自动暂停。
 * 不依赖外部资源，全部程序化绘制。
 */

export type ParticleKind =
  | "none" | "rain" | "snow" | "sakura" | "leaves" | "petals" | "stars"
  | "meteor" | "embers" | "fireflies" | "lantern" | "dust" | "mist"
  | "fish" | "butterfly" | "breath" | "lighthouse" | "cat" | "confetti";

export interface EngineOptions {
  /** 目标 FPS 上限（预算 F01747）。 */
  maxFps?: number;
  reducedMotion?: boolean;
}

interface P {
  x: number; y: number; vx: number; vy: number;
  r: number; a: number; rot: number; vr: number; t: number;
}

export class ParticleEngine {
  private canvas: HTMLCanvasElement | null = null;
  private ctx: CanvasRenderingContext2D | null = null;
  private raf = 0;
  private last = 0;
  private parts: P[] = [];
  private w = 0;
  private h = 0;
  private kind: ParticleKind = "none";
  private opts: EngineOptions;

  constructor(opts: EngineOptions = {}) {
    this.opts = opts;
  }

  attach(canvas: HTMLCanvasElement, kind: ParticleKind): void {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.kind = kind;
    this.resize();
    this.start();
  }

  setKind(kind: ParticleKind): void {
    if (this.kind === kind) return;
    this.kind = kind;
    this.parts = [];
  }

  resize(): void {
    const c = this.canvas;
    if (!c) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    this.w = c.clientWidth;
    this.h = c.clientHeight;
    c.width = Math.max(1, Math.round(this.w * dpr));
    c.height = Math.max(1, Math.round(this.h * dpr));
    this.ctx?.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  start(): void {
    if (this.raf || typeof window === "undefined") return;
    if (this.opts.reducedMotion || document.hidden) return; // reduce-motion / 后台即停
    const step = (t: number): void => {
      this.raf = requestAnimationFrame(step);
      const maxFps = this.opts.maxFps ?? 60;
      if (t - this.last < 1000 / maxFps - 1) return;
      this.last = t;
      this.tick();
    };
    this.raf = requestAnimationFrame(step);
  }

  stop(): void {
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
  }

  destroy(): void {
    this.stop();
    this.canvas = null;
    this.ctx = null;
    this.parts = [];
  }

  private count(): number {
    switch (this.kind) {
      case "none": return 0;
      case "rain": case "stars": case "dust": return 90;
      case "snow": case "sakura": case "leaves": case "petals": case "fireflies": case "mist": case "confetti": return 60;
      case "meteor": case "embers": case "lantern": case "fish": case "butterfly": return 24;
      case "breath": case "lighthouse": case "cat": return 1;
      default: return 0;
    }
  }

  private spawn(): P {
    const r = Math.random;
    const base: P = { x: r() * this.w, y: r() * this.h, vx: 0, vy: 0, r: 2 + r() * 3, a: 0.5 + r() * 0.5, rot: r() * Math.PI * 2, vr: (r() - 0.5) * 0.04, t: r() };
    switch (this.kind) {
      case "rain": base.vy = 420 + r() * 260; base.vx = 40; base.r = 1; break;
      case "snow": base.vy = 40 + r() * 50; base.vx = (r() - 0.5) * 24; break;
      case "sakura": case "petals": base.vy = 30 + r() * 40; base.vx = (r() - 0.5) * 30; base.r = 3 + r() * 3; break;
      case "leaves": base.vy = 35 + r() * 45; base.vx = (r() - 0.5) * 40; base.r = 4 + r() * 4; break;
      case "stars": base.vy = 0; base.vx = 0; base.a = r(); break;
      case "meteor": base.x = r() * this.w * 0.8; base.y = -10; base.vx = 220 + r() * 160; base.vy = 160 + r() * 120; base.t = 0; break;
      case "embers": base.vy = -(30 + r() * 60); base.vx = (r() - 0.5) * 20; base.r = 1.5 + r() * 2; break;
      case "fireflies": base.vx = (r() - 0.5) * 30; base.vy = (r() - 0.5) * 30; break;
      case "lantern": base.vy = -(15 + r() * 25); base.vx = (r() - 0.5) * 14; base.r = 5 + r() * 5; break;
      case "dust": base.vy = -(6 + r() * 12); base.vx = (r() - 0.5) * 10; break;
      case "mist": base.vx = 10 + r() * 20; base.r = 60 + r() * 80; base.a = 0.05 + r() * 0.05; break;
      case "fish": base.vx = 30 + r() * 40; base.r = 8 + r() * 8; break;
      case "butterfly": base.vx = (r() - 0.5) * 60; base.vy = (r() - 0.5) * 40; base.r = 4 + r() * 3; break;
      case "confetti": base.y = -10; base.vy = 160 + r() * 160; base.vx = (r() - 0.5) * 80; base.r = 3 + r() * 3; break;
      default: break;
    }
    return base;
  }

  private tick(): void {
    const ctx = this.ctx;
    if (!ctx) return;
    const want = this.count();
    while (this.parts.length < want) this.parts.push(this.spawn());
    if (this.parts.length > want) this.parts.length = want;
    ctx.clearRect(0, 0, this.w, this.h);
    const t = performance.now() / 1000;
    for (const p of this.parts) {
      this.step(p, t);
      this.draw(p, t);
    }
  }

  private step(p: P, t: number): void {
    p.x += p.vx / 60;
    p.y += p.vy / 60;
    p.rot += p.vr;
    p.t += 1 / 60;
    const wrap = (): void => {
      if (p.y > this.h + 20) { p.y = -10; p.x = Math.random() * this.w; }
      if (p.y < -20) { p.y = this.h + 10; p.x = Math.random() * this.w; }
      if (p.x > this.w + 20) p.x = -10;
      if (p.x < -20) p.x = this.w + 10;
    };
    switch (this.kind) {
      case "rain": wrap(); break;
      case "meteor":
        p.x += p.vx / 60; p.y += p.vy / 60;
        if (p.y > this.h || p.x > this.w) { p.x = Math.random() * this.w * 0.8; p.y = -10; }
        break;
      case "stars": break;
      case "fireflies":
        p.vx += (Math.random() - 0.5) * 6;
        p.vy += (Math.random() - 0.5) * 6;
        p.vx = Math.max(-40, Math.min(40, p.vx));
        p.vy = Math.max(-40, Math.min(40, p.vy));
        wrap();
        break;
      case "fish":
        p.y += Math.sin(t * 2 + p.t * 3) * 0.6;
        wrap();
        break;
      case "mist": wrap(); break;
      case "confetti":
        if (p.y > this.h + 20) { p.y = -10; p.x = Math.random() * this.w; }
        break;
      default: wrap(); break;
    }
  }

  private draw(p: P, t: number): void {
    const ctx = this.ctx!;
    ctx.save();
    switch (this.kind) {
      case "rain":
        ctx.strokeStyle = `rgba(180,200,235,${p.a * 0.5})`;
        ctx.beginPath();
        ctx.moveTo(p.x, p.y);
        ctx.lineTo(p.x - p.vx * 0.03, p.y - p.vy * 0.03);
        ctx.stroke();
        break;
      case "stars": {
        const tw = 0.4 + 0.6 * Math.abs(Math.sin(t * 2 + p.t * 7));
        ctx.fillStyle = `rgba(230,235,255,${p.a * tw})`;
        ctx.fillRect(p.x, p.y, p.r, p.r);
        break;
      }
      case "meteor":
        ctx.strokeStyle = "rgba(255,240,200,0.9)";
        ctx.beginPath();
        ctx.moveTo(p.x, p.y);
        ctx.lineTo(p.x - p.vx * 0.12, p.y - p.vy * 0.12);
        ctx.stroke();
        break;
      case "embers":
      case "lantern":
        ctx.fillStyle = this.kind === "embers" ? `rgba(255,150,60,${p.a})` : `rgba(255,120,80,${p.a * 0.9})`;
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fill();
        break;
      case "fireflies": {
        const g = 0.3 + 0.7 * Math.abs(Math.sin(t * 3 + p.t * 5));
        ctx.fillStyle = `rgba(200,255,140,${g})`;
        ctx.beginPath();
        ctx.arc(p.x, p.y, 2.5, 0, Math.PI * 2);
        ctx.fill();
        break;
      }
      case "mist":
        ctx.fillStyle = `rgba(220,230,240,${p.a})`;
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fill();
        break;
      case "fish":
        ctx.fillStyle = "rgba(255,180,120,0.9)";
        ctx.beginPath();
        ctx.ellipse(p.x, p.y, p.r, p.r * 0.5, Math.atan2(0, p.vx), 0, Math.PI * 2);
        ctx.fill();
        ctx.beginPath();
        ctx.moveTo(p.x - p.r, p.y);
        ctx.lineTo(p.x - p.r * 1.5, p.y - p.r * 0.35);
        ctx.lineTo(p.x - p.r * 1.5, p.y + p.r * 0.35);
        ctx.fill();
        break;
      case "butterfly": {
        const flap = Math.abs(Math.sin(t * 8 + p.t * 6));
        ctx.fillStyle = "rgba(240,150,220,0.9)";
        ctx.beginPath();
        ctx.ellipse(p.x - p.r * 0.5, p.y, p.r * flap, p.r * 0.8, p.rot, 0, Math.PI * 2);
        ctx.ellipse(p.x + p.r * 0.5, p.y, p.r * flap, p.r * 0.8, p.rot, 0, Math.PI * 2);
        ctx.fill();
        break;
      }
      case "sakura":
      case "petals":
        ctx.fillStyle = this.kind === "sakura" ? "rgba(255,190,215,0.9)" : "rgba(255,120,140,0.85)";
        ctx.beginPath();
        ctx.ellipse(p.x, p.y, p.r, p.r * 0.55, p.rot, 0, Math.PI * 2);
        ctx.fill();
        break;
      case "leaves":
        ctx.fillStyle = `rgba(220,110,60,${p.a})`;
        ctx.beginPath();
        ctx.ellipse(p.x, p.y, p.r, p.r * 0.5, p.rot, 0, Math.PI * 2);
        ctx.fill();
        break;
      case "snow":
        ctx.fillStyle = `rgba(245,250,255,${p.a * 0.9})`;
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fill();
        break;
      case "dust":
        ctx.fillStyle = `rgba(255,245,220,${p.a * 0.5})`;
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r * 0.6, 0, Math.PI * 2);
        ctx.fill();
        break;
      case "confetti":
        ctx.fillStyle = `hsl(${(p.t * 160) % 360} 90% 65%)`;
        ctx.fillRect(p.x, p.y, p.r, p.r * 1.6);
        break;
      case "breath": {
        // 呼吸引导：4-7-8 节奏圆
        const phase = (t % 19) / 19;
        const s = phase < 4 / 19 ? phase * 19 / 4 : phase < 11 / 19 ? 1 : Math.max(0, 1 - (phase - 11 / 19) * 19 / 8);
        ctx.strokeStyle = "rgba(140,220,210,0.8)";
        ctx.lineWidth = 3;
        ctx.beginPath();
        ctx.arc(this.w / 2, this.h / 2, 40 + s * 90, 0, Math.PI * 2);
        ctx.stroke();
        break;
      }
      case "lighthouse": {
        // 旋转光束
        const ang = t * 0.9;
        const grad = ctx.createLinearGradient(this.w * 0.7, this.h * 0.3, this.w * 0.7 + Math.cos(ang) * this.w, this.h * 0.3 + Math.sin(ang) * this.h);
        grad.addColorStop(0, "rgba(255,240,180,0.35)");
        grad.addColorStop(1, "rgba(255,240,180,0)");
        ctx.fillStyle = grad;
        ctx.beginPath();
        ctx.moveTo(this.w * 0.7, this.h * 0.3);
        ctx.arc(this.w * 0.7, this.h * 0.3, this.w, ang - 0.09, ang + 0.09);
        ctx.fill();
        break;
      }
      case "cat":
        // 桌面猫：蜷卧剪影 + 尾巴摆动
        ctx.fillStyle = "rgba(60,50,50,0.9)";
        ctx.beginPath();
        ctx.ellipse(this.w * 0.8, this.h * 0.8, 60, 40, 0, 0, Math.PI * 2);
        ctx.fill();
        ctx.strokeStyle = "rgba(60,50,50,0.9)";
        ctx.lineWidth = 8;
        ctx.beginPath();
        ctx.moveTo(this.w * 0.8 - 55, this.h * 0.8);
        ctx.quadraticCurveTo(this.w * 0.8 - 100, this.h * 0.8 + Math.sin(t * 2) * 24, this.w * 0.8 - 90, this.h * 0.8 - 30 + Math.sin(t * 2) * 10);
        ctx.stroke();
        break;
      default:
        break;
    }
    ctx.restore();
  }
}
