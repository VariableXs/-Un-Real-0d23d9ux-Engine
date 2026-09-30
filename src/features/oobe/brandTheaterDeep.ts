/**
 * UNREAL-X AI-03 · 品牌剧场深化（族0021~0028 · X00501~X00700，V 线承载）。
 *
 * 五族前置参数模型（X00501~X00650）：动态标识档位、声景包络、色温曲线、
 * 字标动势、倒计时美学、转场语法、情绪板取色、启动无障碍叙事。
 * 铁律：全部时长/色彩经 CSS 令牌（--dur-* / --ease-*）换算，禁裸值；
 * reduce-motion 自动降级；HC 主题不参与取色与色温曲线。
 */

// ---- 族0021 动态标识 ----

export type MarkTier = "static" | "breath" | "rhythm" | "theater" | "egg";

export const MARK_TIERS: readonly MarkTier[] = ["static", "breath", "rhythm", "theater", "egg"] as const;

export const DEFAULT_MARK_TIER: MarkTier = "breath";

/** 标识档位合法性（非法档回默认）。 */
export function clampMarkTier(tier: string): MarkTier {
  return (MARK_TIERS as readonly string[]).includes(tier) ? (tier as MarkTier) : DEFAULT_MARK_TIER;
}

/** 档位 → 动效参数（帧数 8/16/24/32/40，均落在 30fps 预算内）。 */
export function markMotionParams(tier: MarkTier): { frames: number; durationMs: number; ease: string } {
  const rank = MARK_TIERS.indexOf(tier);
  const frames = 8 * (rank + 1);
  return { frames, durationMs: frames * 33, ease: tier === "theater" || tier === "egg" ? "var(--ease-emphasized)" : "var(--ease-standard)" };
}

/** 低配/省电降级链：egg→theater→rhythm→breath→static。 */
export function degradeMarkTier(tier: MarkTier, cpuCores: number, batterySaver: boolean): MarkTier {
  if (batterySaver || cpuCores <= 2) return "static";
  if (cpuCores <= 4 && MARK_TIERS.indexOf(tier) >= 3) return "rhythm";
  return tier;
}

// ---- 族0022 声景 2.0 ----

export interface SoundscapeEnvelope {
  attackMs: number;
  holdMs: number;
  releaseMs: number;
  muted: boolean;
}

/** 开机/关机/提示 三声景默认包络（总时长对齐 --dur 档位体系）。 */
export const SOUNDSCAPES: Readonly<Record<string, SoundscapeEnvelope>> = {
  boot: { attackMs: 120, holdMs: 360, releaseMs: 520, muted: false },
  shutdown: { attackMs: 80, holdMs: 200, releaseMs: 720, muted: false },
  hint: { attackMs: 10, holdMs: 40, releaseMs: 50, muted: false },
} as const;

/** 勿扰降级：静音并把首尾段钳到 ≤50ms。 */
export function applyDnd(env: SoundscapeEnvelope): SoundscapeEnvelope {
  return { attackMs: Math.min(env.attackMs, 50), holdMs: env.holdMs, releaseMs: Math.min(env.releaseMs, 50), muted: true };
}

/** 声景令牌换算：包络总时长 → CSS transition-duration 字符串。 */
export function soundscapeToken(env: SoundscapeEnvelope): string {
  return `${env.attackMs + env.holdMs + env.releaseMs}ms`;
}

// ---- 族0023 色温曲线 ----

/** 昼夜曲线：t 为 0..=1440 分钟，正午 6500K → 午夜 2700K。 */
export function circadianKelvin(tMin: number): number {
  const t = ((Math.round(tMin) % 1440) + 1440) % 1440;
  const phase = Math.abs(t - 720) / 720;
  return Math.round(6500 - phase * 3800);
}

/** 色温 → CSS 颜色令牌（semitone 档位表，避免连续裸值）。 */
export const KELVIN_TOKENS: Readonly<Record<number, string>> = {
  2700: "var(--w2-warm-deep)",
  3800: "var(--w2-warm)",
  5000: "var(--w2-neutral)",
  6500: "var(--w2-day)",
} as const;

export function kelvinToken(k: number): string {
  const steps = Object.keys(KELVIN_TOKENS)
    .map(Number)
    .sort((a, b) => Math.abs(a - k) - Math.abs(b - k));
  return KELVIN_TOKENS[steps[0]!]!;
}

// ---- 族0024 字标动势 ----

/** 逐字入场延迟（stagger），reduce-motion 折半。 */
export function glyphDelay(index: number, staggerMs: number, reduceMotion = false): number {
  return Math.max(0, index) * staggerMs * (reduceMotion ? 0.5 : 1);
}

/** 字标总时长。 */
export function wordmarkTotal(len: number, staggerMs: number, glyphMs: number, reduceMotion = false): number {
  if (len <= 0) return 0;
  return glyphDelay(len - 1, staggerMs, reduceMotion) + glyphMs;
}

/** 脉动相位 → 字距偏移（-100..100），静态档恒 0。 */
export function pulseOffset(phase: number, tier: MarkTier): number {
  if (tier === "static") return 0;
  const table = [0, 70, 100, 70, 0, -70, -100, -70];
  return table[((phase % 8) + 8) % 8]!;
}

// ---- 族0025 倒计时美学 ----

export type Urgency = "normal" | "warn" | "danger";

export function urgencyOf(secs: number): Urgency {
  if (secs < 10) return "danger";
  if (secs < 60) return "warn";
  return "normal";
}

/** 紧迫度 → 语义令牌（状态语义，禁做装饰色）。 */
export function urgencyToken(u: Urgency): string {
  return u === "danger" ? "var(--danger)" : u === "warn" ? "var(--warn)" : "var(--text-primary)";
}

/** 进度环 dashoffset 百分比（0..=100，非法输入回 0）。 */
export function ringPct(remaining: number, total: number): number {
  if (!Number.isFinite(total) || total <= 0 || !Number.isFinite(remaining)) return 0;
  return Math.min(100, Math.max(0, (remaining / total) * 100));
}

// ---- 族0026 转场语法 ----

export type TransitionKind = "fade" | "slide" | "reveal";

export interface TransitionSpec {
  durMs: number;
  scaleFrom: number;
  shiftPx: number;
  ease: string;
}

const TRANSITIONS: Readonly<Record<TransitionKind, TransitionSpec>> = {
  fade: { durMs: 170, scaleFrom: 1, shiftPx: 0, ease: "var(--ease-standard)" },
  slide: { durMs: 240, scaleFrom: 0.98, shiftPx: 8, ease: "var(--ease-emphasized)" },
  reveal: { durMs: 360, scaleFrom: 0.96, shiftPx: -12, ease: "var(--ease-emphasized)" },
} as const;

export function transitionIn(kind: TransitionKind): TransitionSpec {
  return TRANSITIONS[kind];
}

/** 退出转场：时长 80%、位移反向。 */
export function transitionOut(kind: TransitionKind): TransitionSpec {
  const t = TRANSITIONS[kind];
  return { durMs: Math.round(t.durMs * 0.8), scaleFrom: t.scaleFrom, shiftPx: -t.shiftPx, ease: t.ease };
}

/** reduce-motion 全量降级：80ms 纯淡入淡出。 */
export function transitionReduce(): TransitionSpec {
  return { durMs: 80, scaleFrom: 1, shiftPx: 0, ease: "linear" };
}

// ---- 族0027 情绪板 2.0 ----

export interface MoodTriad {
  accent: string;
  neighbor: string;
  complement: string;
}

/** OKLCH 千分位 → CSS 颜色串。 */
function oklch(l: number, c: number, h: number): string {
  return `oklch(${(l / 1000).toFixed(3)} ${(c / 1000).toFixed(3)} ${h.toFixed(1)})`;
}

/** 情绪板三色组：主色 + 邻近色(+30°) + 补色(+330°)，L/C 钳制对齐取色流水线
 *  （L/C/H 均为千分位口径：L 550~720 / C ≤130，与 C 线 ai03.rs 同一公式）。 */
export function moodTriad(hueDeg: number): MoodTriad {
  const h = ((Math.round(hueDeg) % 360) + 360) % 360;
  const clampL = (l: number) => Math.min(720, Math.max(550, l));
  const clampC = (c: number) => Math.min(130, c);
  return {
    accent: oklch(680, 100, h),
    neighbor: oklch(clampL(700), clampC(110), (h + 30) % 360),
    complement: oklch(clampL(560), clampC(70), (h + 330) % 360),
  };
}

/** 对比度门禁：accent 亮度 ≥0.62 配深字，否则浅字。 */
export function moodFgDark(accentLightness: number): boolean {
  return accentLightness >= 0.62;
}

/** HC 红线：HC 主题永不参与取色流水线（固定黑白黄）。 */
export function moodForbiddenInHc(): boolean {
  return true;
}

// ---- 族0028 启动无障碍 2.0 ----

/** 启动步骤读屏文案（越界与终态静默，长度 ≤20 字）。 */
export const BOOT_NARRATION: readonly string[] = [
  "启动开始，正在准备系统",
  "品牌展示中，可按 Esc 跳过",
  "自检进行中，请稍候",
  "即将进入桌面",
  "欢迎回来",
] as const;

export function narrationAt(step: number): string {
  return step >= 0 && step < BOOT_NARRATION.length ? BOOT_NARRATION[step]! : "";
}

/** 字幕开关：用户偏好优先于默认。 */
export function captionEnabled(pref: boolean | undefined, defaultOn: boolean): boolean {
  return pref ?? defaultOn;
}

/** AA 对比近似门禁：OKLCH L 差 ≥0.35。 */
export function contrastOk(fgL: number, bgL: number): boolean {
  return Math.abs(fgL - bgL) >= 0.35;
}
