/**
 * F164 锁屏定制深化 · 合成层栈 + 解锁状态机 + 通知摘要布局。
 *
 * 主册判据延伸：
 * - F164「唤醒到可见 ≤2s」——合成器分两拍：第一拍背景+时钟（必须可见），
 *   第二拍其余层渐进（骨架后内容——八章启动节奏感的锁屏版）；
 * - 「密码聚焦让位」（boot-engine focusRetreatTransform 的消费侧深化）：
 *   让位不是跳变——输入法候选弹出时布局做限速动画；
 * - 「通知只计数不显内容」的布局算法：计数徽标的排布与溢出折叠；
 * - 输错节流（boot-engine lockMachineStep 消费侧）：完整状态机含
 *   冷却期、剩余次数、升级锁定。
 */

// ---------- 合成层栈（两拍渐进的第一拍可见性） ----------

export type LockLayer = "backdrop" | "clock" | "date" | "status" | "notifications" | "hint";

export interface LayerSpec {
  layer: LockLayer;
  /** 显示拍（1 = 唤醒即显——第一拍；2 = 渐进拍）。 */
  beat: 1 | 2;
  /** 渐进延迟 ms（第二拍内部阶梯——每层 +80ms）。 */
  delayMs: number;
}

export const LAYER_STACK: readonly LayerSpec[] = [
  { layer: "backdrop", beat: 1, delayMs: 0 },
  { layer: "clock", beat: 1, delayMs: 0 },
  { layer: "date", beat: 2, delayMs: 80 },
  { layer: "status", beat: 2, delayMs: 160 },
  { layer: "notifications", beat: 2, delayMs: 240 },
  { layer: "hint", beat: 2, delayMs: 320 },
];

/** 唤醒后的可见层（elapsed 决定——第一拍恒可见：时钟 0ms 出现）。 */
export function visibleLayers(elapsedMs: number): LockLayer[] {
  return LAYER_STACK.filter((l) => l.beat === 1 || elapsedMs >= l.delayMs).map((l) => l.layer);
}

/** ≤2s 预算拆账：第一拍必须 <100ms（时钟可见即「锁屏已醒」）。 */
export function wakeBudget(): { firstBeatMs: number; fullMs: number; withinBudget: boolean } {
  const firstBeatMs = Math.max(...LAYER_STACK.filter((l) => l.beat === 1).map((l) => l.delayMs));
  const fullMs = Math.max(...LAYER_STACK.map((l) => l.delayMs));
  return { firstBeatMs, fullMs, withinBudget: firstBeatMs < 100 && fullMs < 2000 };
}

// ---------- 背景虚化参数（壁纸在锁屏的毛玻璃——可推导不拍脑袋） ----------

export interface BlurSpec {
  radiusPx: number;
  /** 亮度压暗系数（0..1——1 不压暗）。 */
  brightness: number;
  /** 饱和度提升（毛玻璃质感需要轻微提饱和补偿模糊掉色）。 */
  saturate: number;
}

/** 按壁纸亮度推导虚化参数：亮壁纸压暗多点、暗壁纸少压（时钟可读性优先）。 */
export function blurForWallpaper(meanLuminance: number): BlurSpec {
  const lum = Math.min(1, Math.max(0, meanLuminance));
  // 时钟白字需要底亮度 <0.45——亮壁纸加大压暗。
  const brightness = lum > 0.45 ? Math.max(0.4, 0.9 - lum * 0.6) : 1;
  return { radiusPx: Math.round(28 + (1 - lum) * 12), brightness, saturate: 1.08 };
}

// ---------- 时间问候（文案随时刻——有人味的细节） ----------

export type Greeting = "night" | "dawn" | "morning" | "noon" | "afternoon" | "evening";

export function greetingFor(hour: number): Greeting {
  if (hour < 5) return "night";
  if (hour < 8) return "dawn";
  if (hour < 11) return "morning";
  if (hour < 14) return "noon";
  if (hour < 18) return "afternoon";
  return "evening";
}

export const GREETING_TEXT: Record<Greeting, { zh: string; en: string }> = {
  night: { zh: "夜深了", en: "Late night" },
  dawn: { zh: "早安", en: "Good morning" },
  morning: { zh: "上午好", en: "Good morning" },
  noon: { zh: "午安", en: "Good afternoon" },
  afternoon: { zh: "下午好", en: "Good afternoon" },
  evening: { zh: "晚上好", en: "Good evening" },
};

// ---------- 让位限速（密码聚焦布局动画——消费 boot-engine 变换参数） ----------

export interface RetreatTransform {
  /** 时钟组上移 px（负值向上）。 */
  translateY: number;
  /** 缩放（0.8..1）。 */
  scale: number;
  /** 透明度（时钟让位时略降——注意力让给输入卡）。 */
  opacity: number;
}

/** 让位插值（0..1 进度——限速动画的帧计算，60fps 帧率无关用 easeOutCubic）。 */
export function retreatProgress(t01: number): RetreatTransform {
  const t = Math.min(1, Math.max(0, t01));
  const ease = 1 - (1 - t) ** 3;
  return { translateY: -64 * ease, scale: 1 - 0.16 * ease, opacity: 1 - 0.25 * ease };
}

// ---------- 通知摘要布局（只计数不显内容——计数徽标与溢出折叠） ----------

export interface NotificationGroup {
  appId: string;
  /** 图标名（内容永不进锁屏——只有图标与计数）。 */
  icon: string;
  count: number;
}

export interface DigestLayout {
  /** 参与排布的组（前 4 组）。 */
  shown: NotificationGroup[];
  /** 溢出组折叠后的合计。 */
  overflowCount: number;
  /** 总通知数（状态行「只计数」的数据源）。 */
  total: number;
}

const MAX_SHOWN_GROUPS = 4;

/** 摘要布局：按计数降序取前 4 组，其余折叠合计（计数对拍 F167 同族口径）。 */
export function layoutDigest(groups: NotificationGroup[]): DigestLayout {
  const valid = groups.filter((g) => g.count > 0);
  const total = valid.reduce((s, g) => s + g.count, 0);
  const sorted = [...valid].sort((a, b) => b.count - a.count);
  const shown = sorted.slice(0, MAX_SHOWN_GROUPS);
  const overflowCount = sorted.slice(MAX_SHOWN_GROUPS).reduce((s, g) => s + g.count, 0);
  return { shown, overflowCount, total };
}

// ---------- 解锁状态机（输错节流的完整执法面） ----------

export type UnlockState = "idle" | "verifying" | "cooldown" | "locked";

export interface UnlockConfig {
  /** 最大连续失败次数（达到 → 冷却）。 */
  maxAttempts: number;
  /** 冷却时长 ms（第 n 次冷却 = base × 2^(n-1)，封顶 5 分钟）。 */
  cooldownBaseMs: number;
  /** 冷却封顶 ms。 */
  cooldownMaxMs: number;
}

export const DEFAULT_UNLOCK_CONFIG: UnlockConfig = { maxAttempts: 5, cooldownBaseMs: 15_000, cooldownMaxMs: 300_000 };

export type UnlockOutcome =
  | { type: "accepted" }
  | { type: "rejected"; remaining: number }
  | { type: "cooldown-started"; untilMs: number; durationMs: number }
  | { type: "cooldown-active"; remainingMs: number }
  | { type: "locked-out" };

/**
 * 解锁尝试状态机：
 * - 输错累计到 maxAttempts → 冷却（指数倍增：15s → 30s → 60s → …封顶 5min）；
 * - 冷却期内提交 → 拒绝并报剩余（不延长冷却——惩罚与提示分离，不激怒用户）；
 * - 成功 → 全状态清零（连续失败不跨会话记忆——密码正确即信任）。
 */
export class UnlockMachine {
  private state: UnlockState = "idle";
  private failures = 0;
  private cooldownUntil = 0;
  /** 冷却轮次（第 n 轮冷却时长 = base × 2^(n-1)——按轮次算不按累计失败数，可预期）。 */
  private cooldownRound = 0;

  constructor(private readonly cfg: UnlockConfig = DEFAULT_UNLOCK_CONFIG) {}

  get current(): UnlockState {
    return this.state;
  }

  get remainingAttempts(): number {
    return Math.max(0, this.cfg.maxAttempts - this.failures);
  }

  /** 冷却剩余 ms（0 = 不在冷却）。 */
  cooldownRemaining(now: number): number {
    return Math.max(0, this.cooldownUntil - now);
  }

  /** 提交一次验证。 */
  submit(correct: boolean, now: number): UnlockOutcome {
    if (this.state === "locked") return { type: "locked-out" };
    if (this.cooldownRemaining(now) > 0) {
      this.state = "cooldown";
      return { type: "cooldown-active", remainingMs: this.cooldownRemaining(now) };
    }
    if (correct) {
      this.reset();
      return { type: "accepted" };
    }
    this.failures++;
    if (this.failures >= this.cfg.maxAttempts) {
      this.cooldownRound++;
      const durationMs = Math.min(this.cfg.cooldownMaxMs, this.cfg.cooldownBaseMs * 2 ** (this.cooldownRound - 1));
      this.cooldownUntil = now + durationMs;
      this.state = "cooldown";
      return { type: "cooldown-started", untilMs: this.cooldownUntil, durationMs };
    }
    this.state = "idle";
    return { type: "rejected", remaining: this.remainingAttempts };
  }

  /** 冷却结束（计时器回调——状态回落 idle，失败计数保留冷却后仍生效）。 */
  cooldownExpired(now: number): void {
    if (this.state === "cooldown" && this.cooldownRemaining(now) === 0) this.state = "idle";
  }

  /** 管理级强制锁定（远程擦除前 / 安全策略——显性入口不藏）。 */
  lockOut(): void {
    this.state = "locked";
  }

  reset(): void {
    this.state = "idle";
    this.failures = 0;
    this.cooldownUntil = 0;
    this.cooldownRound = 0;
  }
}
