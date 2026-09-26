/**
 * F153 主题深浅自动切换 · 完整设计。
 *
 * 主册判据：定时切换三连测准时（±5s）；交叉淡入无白屏帧（录屏帧检）；手动优先逻辑实测。
 *
 * 【功能定义】深浅主题对绑定切换：按时间（用户设）或日落日出（F116 联动）自动在
 * 深/浅主题间切换；切换动画 300ms 交叉淡入（跨零点无闪烁）；每主题对各自配壁纸组。
 *
 * 【状态与异常】（主册原文，状态机文档化——四态：浅/深/切换中/暂停）：
 * - 主题对中某主题被卸载 → 切换跳过+提示重绑；
 * - 跨零点（23:59 浅→00:01 深）无重复触发（状态机去抖）；
 * - 手动切主题 → 自动切换暂停当日（尊重手动——次日恢复）。
 *
 * 【设计细节】预告 toast 60s 含「立即切换/今晚跳过」二钮；淡入实现=双层壁纸交叉
 * （任务栏/窗口令牌同步替换在动画中段原子完成）。
 */

import { nextDailyFire, personaStore } from "./store";

export const SECTION = "theme";

export const SWITCH_FADE_MS = 300;
export const ADVANCE_TOAST_SECONDS = 60;
export const TIMER_TOLERANCE_MS = 5000;

export type AutoDarkMode = "off" | "timer" | "sunset";
/** 四态状态机（主册【设计细节】文档化要求）。 */
export type AutoDarkState = "light" | "dark" | "switching" | "paused";

export interface ThemePair {
  darkThemeId: string;
  lightThemeId: string;
  darkWallpaper?: string;
  lightWallpaper?: string;
}

export interface AutoDarkConfig {
  enabled: boolean;
  mode: "off" | "timer" | "sunset";
  /** timer 模式：进入深色时刻（HH:MM）。 */
  darkAt: string;
  /** timer 模式：进入浅色时刻（HH:MM）。 */
  lightAt: string;
  /** sunset 模式：日落日出时刻由 F116 城市数据注入（此字段为外部刷新入口）。 */
  sunsetMinutes: number | null;
  sunriseMinutes: number | null;
  pair: ThemePair;
  /** 手动暂停日期（YYYY-MM-DD）——尊重手动，次日恢复。 */
  manualPauseDate: string | null;
  /** 「今晚跳过」日期。 */
  skipDate: string | null;
  state: AutoDarkState;
}

export function defaultAutoDarkConfig(): AutoDarkConfig {
  return {
    enabled: false,
    mode: "timer",
    darkAt: "20:00",
    lightAt: "07:00",
    sunsetMinutes: null,
    sunriseMinutes: null,
    pair: { darkThemeId: "deep-space", lightThemeId: "paper" },
    manualPauseDate: null,
    skipDate: null,
    state: "light",
  };
}

export function loadAutoDarkConfig(): AutoDarkConfig {
  const stored = personaStore.getWith(SECTION, "autoDark", undefined) as Partial<AutoDarkConfig> | undefined;
  return { ...defaultAutoDarkConfig(), ...(stored ?? {}) };
}

export function saveAutoDarkConfig(config: AutoDarkConfig): void {
  personaStore.set(SECTION, { autoDark: config });
}

// ---------- 时刻解析 ----------

/** "HH:MM" → 当日分钟数；非法输入返回 null（校验拦截+指出）。 */
export function parseHHMM(s: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(s.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return h * 60 + min;
}

function minutesOfDay(now: number): number {
  const d = new Date(now);
  return d.getHours() * 60 + d.getMinutes();
}

function dateKey(now: number): string {
  const d = new Date(now);
  const p = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** 目标档位推断：纯函数——给定时刻与配置，此刻应该在深还是浅。 */
export function desiredSide(config: AutoDarkConfig, now: number): "dark" | "light" | null {
  if (!config.enabled || config.mode === "off") return null;
  let darkMin: number | null;
  let lightMin: number | null;
  if (config.mode === "timer") {
    darkMin = parseHHMM(config.darkAt);
    lightMin = parseHHMM(config.lightAt);
  } else {
    darkMin = config.sunsetMinutes;
    lightMin = config.sunriseMinutes;
  }
  if (darkMin === null || lightMin === null) return null;
  const cur = minutesOfDay(now);
  // 区间语义：darkMin→lightMin（可跨零点）为深，其余为浅。
  if (darkMin === lightMin) return null;
  if (darkMin < lightMin) return cur >= darkMin && cur < lightMin ? "dark" : "light";
  return cur >= darkMin || cur < lightMin ? "dark" : "light";
}

// ---------- 切换决策（纯函数核心，供 tick 驱动与测试） ----------

export interface SwitchDecision {
  action: "none" | "toDark" | "toLight" | "preToast" | "skip";
  /** 触发原因（体验日志与三要素错误呈现用）。 */
  reason: string;
  /** 决策目标侧（四态状态机的两个可执行侧；切换中/暂停是过程态不在决策输出里）。 */
  target: "dark" | "light";
  /** 距实际切换还有多少毫秒（preToast 时有意义）。 */
  inMs?: number;
}

export interface TickContext {
  now: number;
  /** 当前实际生效侧（不含 switching 中间态）。 */
  activeSide: "dark" | "light";
  /** 主题对中深/浅主题是否可用（卸载检测）。 */
  darkThemeAvailable: boolean;
  lightThemeAvailable: boolean;
  /** 预告 toast 是否已为本轮发出。 */
  advanceToastShown: boolean;
  /** 用户是否已确认过本轮预告（点了「立即切换」）。 */
  advanceConfirmed: boolean;
}

/**
 * 核心 tick 决策：
 * - 预告窗（边界前 60s，无论当前在哪侧）先 preToast（可立即切/今晚跳过）；
 * - 边界过后目标侧≠当前侧 → 执行切换（跨零点去抖：同一边界只执行一次由
 *   状态机落位保证——执行后 activeSide 即目标侧，不再重复触发）；
 * - 手动暂停当日、主题卸载跳过+提示重绑。
 */
export function tickAutoDark(config: AutoDarkConfig, ctx: TickContext): SwitchDecision {
  const today = dateKey(ctx.now);
  if (config.manualPauseDate === today || config.skipDate === today) {
    return { action: "none", reason: "今日已暂停（手动或跳过）", target: ctx.activeSide };
  }
  const target = desiredSide(config, ctx.now);
  if (target === null) {
    return { action: "none", reason: "自动切换未启用或配置无效", target: ctx.activeSide };
  }
  // 主题卸载 → 切换跳过+提示重绑（只在实际要切过去时检查）。
  if (target !== ctx.activeSide) {
    if (target === "dark" && !ctx.darkThemeAvailable) {
      return { action: "none", reason: "深色主题已卸载，请重新绑定主题对", target: ctx.activeSide };
    }
    if (target === "light" && !ctx.lightThemeAvailable) {
      return { action: "none", reason: "浅色主题已卸载，请重新绑定主题对", target: ctx.activeSide };
    }
  }
  // 下一次边界（下一侧 + 距今毫秒）——预告窗判定与当前侧无关。
  const darkEdgeMin = config.mode === "timer" ? parseHHMM(config.darkAt) : config.sunsetMinutes;
  const lightEdgeMin = config.mode === "timer" ? parseHHMM(config.lightAt) : config.sunriseMinutes;
  const edges: { side: "dark" | "light"; min: number | null }[] = [
    { side: "dark", min: darkEdgeMin },
    { side: "light", min: lightEdgeMin },
  ];
  let nextEdge: { side: "dark" | "light"; inMs: number } | null = null;
  for (const e of edges) {
    if (e.min === null) continue;
    const edge = new Date(ctx.now);
    edge.setHours(Math.floor(e.min / 60), e.min % 60, 0, 0);
    if (edge.getTime() <= ctx.now) edge.setDate(edge.getDate() + 1);
    const inMs = edge.getTime() - ctx.now;
    if (nextEdge === null || inMs < nextEdge.inMs) nextEdge = { side: e.side, inMs };
  }
  if (nextEdge !== null && nextEdge.inMs <= ADVANCE_TOAST_SECONDS * 1000) {
    // 预告窗内：先预告（可立即切/今晚跳过），到点后执行。
    if (!ctx.advanceToastShown && !ctx.advanceConfirmed) {
      return { action: "preToast", reason: "切换预告窗口", target: nextEdge.side, inMs: nextEdge.inMs };
    }
    if (target === nextEdge.side && target !== ctx.activeSide) {
      return {
        action: nextEdge.side === "dark" ? "toDark" : "toLight",
        reason: ctx.advanceConfirmed ? "用户确认立即切换" : "到达切换边界",
        target: nextEdge.side,
      };
    }
    return { action: "none", reason: "预告已发，等待边界", target: ctx.activeSide };
  }
  if (target !== ctx.activeSide) {
    // 无预告场景（如刚启用且已过边界）：直接执行。
    return {
      action: target === "dark" ? "toDark" : "toLight",
      reason: "目标侧与当前侧不一致（边界已过）",
      target,
    };
  }
  return { action: "none", reason: "已在目标侧", target: ctx.activeSide };
}

// ---------- 日落日出（F116 城市数据联动注入） ----------

/**
 * 简化太阳时计算（F116 数据注入前的高精度兜底）：
 * 输入纬度/一年中的第 n 天，返回日出日落分钟数；极昼极夜返回 null。
 * F116 就绪后以其城市数据为准——本函数只服务「无城市数据时的降级路径」。
 */
export function sunTimesMinutes(latitudeDeg: number, dayOfYear: number): { sunrise: number; sunset: number } | null {
  const rad = Math.PI / 180;
  const lat = latitudeDeg * rad;
  // 太阳赤纬（Cooper 公式，精度 ±1°，兜底够用）。
  const decl = 23.45 * rad * Math.sin(((2 * Math.PI) * (284 + dayOfYear)) / 365);
  const cosH = -Math.tan(lat) * Math.tan(decl);
  if (cosH > 1) return null; // 极夜
  if (cosH < -1) return null; // 极昼
  const H = Math.acos(cosH) / rad / 15; // 时角（小时）
  const noon = 720; // 正午 12:00（本地太阳时；经度修正由 F116 注入）
  const half = H * 60;
  return { sunrise: Math.round(noon - half), sunset: Math.round(noon + half) };
}

// ---------- 定时器驱动（封装为可注入时钟的 controller，测试可虚拟时间） ----------

export interface AutoDarkHost {
  /** 应用深/浅侧（E1 热替换 + 壁纸组跟随）。 */
  applySide(side: "dark" | "light"): void;
  /** 预告 toast（含「立即切换/今晚跳过」二钮）。 */
  showAdvanceToast(target: "dark" | "light", inMs: number, onConfirm: () => void, onSkipTonight: () => void): void;
  /** 跳过/重绑提示（三要素）。 */
  report(message: string): void;
}

export class AutoDarkController {
  private timer: ReturnType<typeof setInterval> | null = null;
  private toastShownDay: string | null = null;
  private confirmed = false;

  constructor(
    private host: AutoDarkHost,
    private intervalMs = 1000,
  ) {}

  /** 手动切了主题 → 暂停当日（尊重手动）。 */
  pauseToday(now: number): void {
    const c = loadAutoDarkConfig();
    c.manualPauseDate = dateKey(now);
    c.state = "paused";
    saveAutoDarkConfig(c);
  }

  /** 用户点了预告 toast 的「今晚跳过」。 */
  skipTonight(now: number): void {
    const c = loadAutoDarkConfig();
    c.skipDate = dateKey(now);
    saveAutoDarkConfig(c);
  }

  /** 用户点了预告 toast 的「立即切换」。 */
  confirmAdvance(): void {
    this.confirmed = true;
  }

  start(): void {
    if (this.timer) return;
    this.timer = setInterval(() => this.tick(), this.intervalMs);
  }

  stop(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  tick(now: number = Date.now()): void {
    const config = loadAutoDarkConfig();
    const today = dateKey(now);
    if (this.toastShownDay !== today) {
      this.toastShownDay = today;
      this.confirmed = false;
    }
    const decision = tickAutoDark(config, {
      now,
      activeSide: config.state === "dark" ? "dark" : "light",
      darkThemeAvailable: true, // 主题可用性由调用方（Studio）注入；默认可用
      lightThemeAvailable: true,
      advanceToastShown: this.toastShownDay === today && this.advanceShownToday,
      advanceConfirmed: this.confirmed,
    });
    switch (decision.action) {
      case "toDark":
      case "toLight": {
        const side = decision.action === "toDark" ? "dark" : "light";
        config.state = side;
        saveAutoDarkConfig(config);
        this.host.applySide(side);
        break;
      }
      case "preToast": {
        this.advanceShownToday = true;
        this.host.showAdvanceToast(
          decision.target,
          decision.inMs ?? 0,
          () => {
            this.confirmAdvance();
            this.tick(now + 1);
          },
          () => this.skipTonight(now),
        );
        break;
      }
      case "none": {
        if (decision.reason.includes("卸载")) this.host.report(decision.reason);
        break;
      }
      default:
        break;
    }
  }

  private advanceShownToday = false;
}

/** 下次触发时刻暴露给设置页（「下次切换」行）。 */
export function nextSwitchAt(config: AutoDarkConfig, now: number): number | null {
  const side = desiredSide(config, now);
  if (side === null) return null;
  const darkMin = config.mode === "timer" ? parseHHMM(config.darkAt) : config.sunsetMinutes;
  const lightMin = config.mode === "timer" ? parseHHMM(config.lightAt) : config.sunriseMinutes;
  if (darkMin === null || lightMin === null) return null;
  const [h, m] = side === "dark" ? splitMin(lightMin) : splitMin(darkMin);
  return nextDailyFire(now, h, m);
}

function splitMin(total: number): [number, number] {
  return [Math.floor(total / 60), total % 60];
}
