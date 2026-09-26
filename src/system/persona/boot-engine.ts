/**
 * F164/F165 锁屏与开机引擎深化 · 锁屏状态机 + 尝试节流 + 开机粒子确定性模拟 +
 * 烘帧清单。
 *
 * 主册判据延伸：
 * - F164【设计细节】「唤醒路径：ACPI 唤醒→合成器恢复→锁屏层首帧（F053 时间线
 *   登记新节点）」「密码输入框聚焦时时间缩小上移（让位布局预演）」「锁屏期间
 *   通知不显示内容只计数」。
 * - F165【设计细节】「粒子色=强调色令牌映射表」「密度三档粒子数（1200/600/200）」
 *   「预览复用动画本体（同一渲染代码——预览即真话）」→ 粒子模拟用确定性种子
 *   （同种子同画面——预览与真播逐帧一致的可验证基础）。
 */

import { PARTICLE_COUNTS, particlePalette, type ParticleDensity, type ParticlePalette } from "./bootskin";
import { BOOT_DURATION_MS } from "./store";

// ---------- 锁屏状态机 ----------

export type LockPhase = "locked" | "unlocking" | "failed" | "unlocked";

export interface LockStateInput {
  phase: LockPhase;
  /** 连续失败次数（节流窗口内）。 */
  failedAttempts: number;
  /** 自上次失败起的毫秒。 */
  sinceLastFailMs: number;
}

export const THROTTLE_THRESHOLD = 5;      // 连续 5 次失败
export const THROTTLE_WINDOW_MS = 30_000; // 30 秒内
export const THROTTLE_WAIT_MS = 15_000;   // 触发后等待 15 秒

export interface LockMachineOutput {
  next: LockPhase;
  /** 节流剩余毫秒（>0 = 拒绝输入并显示剩余时间——防暴力枚举）。 */
  throttleRemainMs: number;
  message: string | null;
}

/** 锁屏解锁状态机（纯函数——每帧/每事件驱动）。 */
export function lockMachineStep(input: LockStateInput, event: { type: "unlock-try" | "unlock-ok" | "lock" }): LockMachineOutput {
  const throttled = input.failedAttempts >= THROTTLE_THRESHOLD
    && input.sinceLastFailMs < THROTTLE_WINDOW_MS
    ? Math.max(0, THROTTLE_WAIT_MS - input.sinceLastFailMs)
    : 0;
  switch (event.type) {
    case "lock":
      return { next: "locked", throttleRemainMs: 0, message: null };
    case "unlock-ok":
      return { next: "unlocked", throttleRemainMs: 0, message: null };
    case "unlock-try": {
      if (throttled > 0) {
        return { next: "locked", throttleRemainMs: throttled, message: `尝试过于频繁，请约 ${Math.ceil(throttled / 1000)} 秒后再试` };
      }
      // 失败由调用方回写 failedAttempts；状态机侧保持 locked。
      return { next: "locked", throttleRemainMs: 0, message: null };
    }
    default:
      return { next: input.phase, throttleRemainMs: throttled, message: null };
  }
}

/** 通知摘要（隐私默认——只计数不显内容）。 */
export interface NotificationDigest {
  count: number;
  /** 按应用计数（应用名可见——隐私边界内）。 */
  byApp: Record<string, number>;
}

export function buildNotificationDigest(entries: { appName: string }[]): NotificationDigest {
  const byApp: Record<string, number> = {};
  for (const e of entries) byApp[e.appName] = (byApp[e.appName] ?? 0) + 1;
  return { count: entries.length, byApp };
}

/** 密码框聚焦时的让位布局（预演参数——时间缩小上移给输入让位）。 */
export function focusRetreatTransform(focused: boolean): { scale: number; translateY: number; transitionMs: number } {
  return focused
    ? { scale: 0.7, translateY: -48, transitionMs: 200 }
    : { scale: 1, translateY: 0, transitionMs: 200 };
}

// ---------- 开机粒子确定性模拟（四幕 · 种子驱动） ----------

export interface ParticleState {
  id: number;
  x: number;
  y: number;
  vx: number;
  vy: number;
  /** 幕归属（0-3：汇聚/成型/点亮/交接）。 */
  act: 0 | 1 | 2 | 3;
  size: number;
  color: "base" | "highlight";
}

export const ACT_BOUNDS_MS: readonly number[] = [0, 2600, 5200, 6800, BOOT_DURATION_MS]; // 四幕边界（8s 结构内）

export function actOf(elapsedMs: number): 0 | 1 | 2 | 3 {
  for (let i = ACT_BOUNDS_MS.length - 2; i >= 0; i--) {
    if (elapsedMs >= (ACT_BOUNDS_MS[i] ?? 0)) return i as 0 | 1 | 2 | 3;
  }
  return 0;
}

/** mulberry32 确定性 PRNG（同种子同画面——预览即真话的数学基础）。 */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** 初始化粒子群（种子确定性；密度档决定数量；强调色决定 base/highlight 配比）。 */
export function seedParticles(density: ParticleDensity, seed: number, accentHex: string, screenW = 1920, screenH = 1080): ParticleState[] {
  const rand = mulberry32(seed);
  const palette: ParticlePalette = particlePalette(accentHex);
  void palette;
  const count = PARTICLE_COUNTS[density];
  const out: ParticleState[] = [];
  for (let i = 0; i < count; i++) {
    out.push({
      id: i,
      x: rand() * screenW,
      y: rand() * screenH,
      vx: (rand() - 0.5) * 0.6,
      vy: (rand() - 0.5) * 0.6,
      act: 0,
      size: 1 + Math.floor(rand() * 3),
      color: rand() < 0.15 ? "highlight" : "base", // 15% 高光配比
    });
  }
  return out;
}

/** 单步推进（dt 毫秒）：幕一自由漂移→幕二向中心汇聚→幕三沿星徽轮廓点亮→幕四交接淡出。 */
export function stepParticles(particles: ParticleState[], elapsedMs: number, dtMs: number, screenW = 1920, screenH = 1080): ParticleState[] {
  const act = actOf(elapsedMs);
  const cx = screenW / 2;
  const cy = screenH / 2;
  const dt = Math.min(100, dtMs) / 16.6; // 归一化到帧步长
  return particles.map((p) => {
    if (act === 0) {
      // 幕一：自由漂移。
      let x = p.x + p.vx * dt;
      let y = p.y + p.vy * dt;
      if (x < 0 || x > screenW) { p = { ...p, vx: -p.vx }; x = Math.min(screenW, Math.max(0, x)); }
      if (y < 0 || y > screenH) { p = { ...p, vy: -p.vy }; y = Math.min(screenH, Math.max(0, y)); }
      return { ...p, x, y, act };
    }
    if (act === 1) {
      // 幕二：向中心汇聚（速度随距离衰减——聚而不挤）。
      const dx = cx - p.x;
      const dy = cy - p.y;
      const dist = Math.hypot(dx, dy) || 1;
      const pull = Math.min(6, dist / 200) * dt;
      return { ...p, x: p.x + (dx / dist) * pull * 60, y: p.y + (dy / dist) * pull * 60, act };
    }
    if (act === 2) {
      // 幕三：点亮——围绕中心按 ID 角度排布到星徽轮廓环带。
      const angle = (p.id / Math.max(1, particles.length)) * Math.PI * 2 + elapsedMs / 4000;
      const radius = 120 + (p.id % 5) * 18;
      const tx = cx + Math.cos(angle) * radius;
      const ty = cy + Math.sin(angle) * radius * 0.92;
      return { ...p, x: p.x + (tx - p.x) * 0.12 * dt, y: p.y + (ty - p.y) * 0.12 * dt, act };
    }
    // 幕四：交接——向外淡出（透明度由渲染层按幕进度控制）。
    const dx = p.x - cx;
    const dy = p.y - cy;
    const dist = Math.hypot(dx, dy) || 1;
    return { ...p, x: p.x + (dx / dist) * 1.4 * dt, y: p.y + (dy / dist) * 1.4 * dt, act };
  });
}

// ---------- 烘帧清单（关机空闲批次执行——F068 按需装载联动） ----------

export interface BakeManifestItem {
  density: ParticleDensity;
  /** 采样帧率（烘帧 30fps 足够——播放端插值到 60）。 */
  sampleFps: number;
  frameCount: number;
  /** 种子（同种子可复现——烘焙产物可验证）。 */
  seed: number;
  estimatedBytes: number;
}

export const BAKE_SAMPLE_FPS = 30;

export function bakeManifest(density: ParticleDensity, seed: number): BakeManifestItem {
  const frameCount = Math.round((BOOT_DURATION_MS / 1000) * BAKE_SAMPLE_FPS); // 240 帧
  return {
    density,
    sampleFps: BAKE_SAMPLE_FPS,
    frameCount,
    seed,
    estimatedBytes: PARTICLE_COUNTS[density] * frameCount * 8, // 每粒子每帧 8 字节（x,y 短整型×2）
  };
}

/** 三档烘帧清单（资产预算对拍——F068 上限 80MB 的分布核算）。 */
export function bakeManifestAll(seed: number): BakeManifestItem[] {
  return (Object.keys(PARTICLE_COUNTS) as ParticleDensity[]).map((d) => bakeManifest(d, seed));
}
