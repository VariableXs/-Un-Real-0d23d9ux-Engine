/**
 * AI-18 氛围与个性化组 — 设置 schema（U-49/50/53/54、N-33、M-64…M-72、V-71…V-80）。
 *
 * 红线（承 SUMMIT 域 A / 化境域 H / ASCENT 域 A）：
 * - 氛围是减法不是加法：所有新行为默认关闭或等于现状；
 * - 每个默认值评审 =「一年后还会喜欢吗」；动效克制 ≤260ms；
 * - 一切氛围项 opt-in；写坏值一律回落默认（绝不让脏数据弄巧成拙）。
 */

import { clamp } from "../../lib/settings";

// ---------- U-49 氛围音景引擎 ----------
export type SoundscapeScene = "rain" | "forest" | "white" | "pink" | "night";

// ---------- U-53 环境辉光 ----------
export type GlowMode = "off" | "static" | "breath";

// ---------- U-54 节律助手 ----------
export type RhythmKind = "sitting" | "eye" | "drink" | "stretch";
export type RhythmNotify = "count" | "badge" | "notify";

export interface RhythmItem {
  enabled: boolean;
  /** 间隔（分钟）；eye 固定 20-20-20 语义（20min 提醒看远处 20s）。 */
  intervalMin: number;
  notify: RhythmNotify;
}

// ---------- N-33 情绪引擎 ----------
export interface MoodSettings {
  enabled: boolean;
  /** 手动校正滑杆（-1..1 / 0..1；也作为学习信号）。 */
  manualArousal: number;
  manualFocus: number;
}

// ---------- M-65 昼夜壁纸组 ----------
export type DaySlot = "morning" | "day" | "dusk" | "night";

// ---------- M-68 屏保时钟 ----------
export type ScreensaverMode = "off" | "clock" | "black";

// ---------- M-71 悬停延迟 ----------
export type HoverLatency = "fast" | "std" | "slow";

// ---------- V-71 本地精选轮换 ----------
export interface CuratedSettings {
  on: boolean;
  /** 当前索引（0..N-1；关闭轮换后停留当前张）。 */
  index: number;
}

// ---------- V-72 壁纸饱和度与明度 ----------
export interface WallpaperFilter {
  /** 60..100（%）；100 = 原图（逐像素等于原图红线）。 */
  saturation: number;
  /** 80..100（%）。 */
  brightness: number;
}

// ---------- V-74/75/76/77/78 个性化档位 ----------
export type UiDensity = "comfort" | "compact";
export type UiRadius = "round" | "small" | "sharp";
export type FocusRingStyle = "system" | "box" | "underline";
export type IconSizeTier = 16 | 20 | 24;

// ---------- V-79 系统模式与应用模式深浅分离 ----------
export type ThemeTrack = "inherit" | "deep-space" | "paper" | "minimal-black" | "high-contrast";

export interface AmbienceSettings {
  /** U-49：音景场景音量表（scene → 0..1）。 */
  soundscapeVolumes: Record<SoundscapeScene, number>;
  /** U-49：睡眠定时（分钟；0 = 不启用）。 */
  soundscapeSleepMin: number;
  /** U-53：环境辉光。 */
  glow: GlowMode;
  /** U-54：四节律（含总开关 rhythm.enabled 于各 item）。 */
  rhythm: Record<RhythmKind, RhythmItem>;
  /** N-33：情绪引擎（默认关；开启后保守偏移；色温维度 HC 自动禁用）。 */
  mood: MoodSettings;
  /** M-65：昼夜壁纸组。 */
  dayAround: {
    enabled: boolean;
    /** 时段边界（小时，默认 6/12/18/21，可调；严格递增）。 */
    boundaries: [number, number, number, number];
    dirs: Record<DaySlot, string>;
  };
  /** M-68：屏保（off/clock/black + 空闲分钟数，默认 10）。 */
  screensaver: ScreensaverMode;
  screensaverIdleMin: number;
  /** M-69：今日简报卡。 */
  briefing: boolean;
  /** M-70：壁纸收藏（路径列表；壁纸工坊收藏组数据源）。 */
  wallpaperFavorites: string[];
  /** M-71：悬停延迟三档（默认 std = 现状）。 */
  hoverLatency: HoverLatency;
  /** M-72：氛围会话恢复快照（可关 = 零残留隐私偏好）。 */
  sessionRestore: boolean;
  /** V-71：本地精选轮换。 */
  curated: CuratedSettings;
  /** V-72：壁纸饱和度/明度（100/100 = 原图 = 现状）。 */
  wallpaperFilter: WallpaperFilter;
  /** V-73：农历节气与节日（可整体关闭，非 CJK 用户）。 */
  lunarCalendar: boolean;
  /** V-74：界面密度（comfort = 现状）。 */
  uiDensity: UiDensity;
  /** V-75：几何风格（round = 现状）。 */
  uiRadius: UiRadius;
  /** V-76：界面字体偏好（空 = 默认 Segoe UI Variable；CJK 回退链恒在）。 */
  uiFont: string;
  /** V-77：焦点环样式（system = 现状）。 */
  focusRing: FocusRingStyle;
  /** V-78：全局 UI 图标尺寸档位（20 = 现状；与文字缩放解耦）。 */
  iconTier: IconSizeTier;
  /** V-79：应用窗口模式轨道（inherit = 跟随环境主题 = 现状无损迁移）。 */
  appTheme: ThemeTrack;
}

export const DEFAULT_AMBIENCE: AmbienceSettings = {
  soundscapeVolumes: { rain: 0.5, forest: 0.5, white: 0.4, pink: 0.4, night: 0.5 },
  soundscapeSleepMin: 0,
  glow: "off",
  rhythm: {
    sitting: { enabled: false, intervalMin: 50, notify: "badge" },
    eye: { enabled: false, intervalMin: 20, notify: "badge" },
    drink: { enabled: false, intervalMin: 60, notify: "count" },
    stretch: { enabled: false, intervalMin: 90, notify: "count" },
  },
  mood: { enabled: false, manualArousal: 0, manualFocus: 0.5 },
  dayAround: {
    enabled: false,
    boundaries: [6, 12, 18, 21],
    dirs: { morning: "", day: "", dusk: "", night: "" },
  },
  screensaver: "off",
  screensaverIdleMin: 10,
  briefing: false,
  wallpaperFavorites: [],
  hoverLatency: "std",
  sessionRestore: false,
  curated: { on: false, index: 0 },
  wallpaperFilter: { saturation: 100, brightness: 100 },
  lunarCalendar: true,
  uiDensity: "comfort",
  uiRadius: "round",
  uiFont: "",
  focusRing: "system",
  iconTier: 20,
  appTheme: "inherit",
};

const SCENES: readonly SoundscapeScene[] = ["rain", "forest", "white", "pink", "night"];
const RHYTHMS: readonly RhythmKind[] = ["sitting", "eye", "drink", "stretch"];
const SLOTS: readonly DaySlot[] = ["morning", "day", "dusk", "night"];

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function bool(v: unknown, def: boolean): boolean {
  return typeof v === "boolean" ? v : def;
}

function num(v: unknown, def: number, min: number, max: number): number {
  const n = Number(v);
  return Number.isFinite(n) ? clamp(n, min, max) : def;
}

function str(v: unknown, def: string): string {
  return typeof v === "string" ? v : def;
}

function oneOf<T extends string>(v: unknown, allowed: readonly T[], def: T): T {
  return typeof v === "string" && (allowed as readonly string[]).includes(v) ? (v as T) : def;
}

/** coerce：逐字段校验，任何坏值回落默认（氛围组纪律）。 */
export function coerceAmbience(raw: unknown): AmbienceSettings {
  const s = structuredClone(DEFAULT_AMBIENCE);
  if (!isRecord(raw)) return s;
  try {
    if (isRecord(raw.soundscapeVolumes)) {
      for (const sc of SCENES) {
        s.soundscapeVolumes[sc] = num((raw.soundscapeVolumes as Record<string, unknown>)[sc], 0.5, 0, 1);
      }
    }
    s.soundscapeSleepMin = num(raw.soundscapeSleepMin, 0, 0, 480);
    s.glow = oneOf(raw.glow, ["off", "static", "breath"] as const, "off");
    if (isRecord(raw.rhythm)) {
      for (const k of RHYTHMS) {
        const item = (raw.rhythm as Record<string, unknown>)[k];
        if (!isRecord(item)) continue;
        s.rhythm[k] = {
          enabled: bool(item.enabled, false),
          intervalMin: num(item.intervalMin, s.rhythm[k].intervalMin, 1, 240),
          notify: oneOf(item.notify, ["count", "badge", "notify"] as const, s.rhythm[k].notify),
        };
      }
    }
    if (isRecord(raw.mood)) {
      s.mood = {
        enabled: bool(raw.mood.enabled, false),
        manualArousal: num(raw.mood.manualArousal, 0, -1, 1),
        manualFocus: num(raw.mood.manualFocus, 0.5, 0, 1),
      };
    }
    if (isRecord(raw.dayAround)) {
      const b = raw.dayAround.boundaries;
      if (Array.isArray(b) && b.length === 4) {
        const bs = b.map((x, i) => num(x, DEFAULT_AMBIENCE.dayAround.boundaries[i] ?? [6, 12, 18, 21][i] ?? 6, 0, 23));
        // 严格递增才接受（时段语义不可倒置）
        const [b0, b1, b2, b3] = bs;
        if (b0 !== undefined && b1 !== undefined && b2 !== undefined && b3 !== undefined && b0 < b1 && b1 < b2 && b2 < b3) {
          s.dayAround.boundaries = [b0, b1, b2, b3];
        }
      }
      s.dayAround.enabled = bool(raw.dayAround.enabled, false);
      if (isRecord(raw.dayAround.dirs)) {
        for (const slot of SLOTS) {
          s.dayAround.dirs[slot] = str((raw.dayAround.dirs as Record<string, unknown>)[slot], "");
        }
      }
    }
    s.screensaver = oneOf(raw.screensaver, ["off", "clock", "black"] as const, "off");
    s.screensaverIdleMin = num(raw.screensaverIdleMin, 10, 1, 120);
    s.briefing = bool(raw.briefing, false);
    if (Array.isArray(raw.wallpaperFavorites)) {
      s.wallpaperFavorites = raw.wallpaperFavorites.filter((p): p is string => typeof p === "string" && p.trim() !== "").slice(0, 200);
    }
    s.hoverLatency = oneOf(raw.hoverLatency, ["fast", "std", "slow"] as const, "std");
    s.sessionRestore = bool(raw.sessionRestore, false);
    if (isRecord(raw.curated)) {
      s.curated = {
        on: bool(raw.curated.on, false),
        index: Math.max(0, Math.floor(num(raw.curated.index, 0, 0, 1e6))),
      };
    }
    if (isRecord(raw.wallpaperFilter)) {
      s.wallpaperFilter = {
        saturation: num(raw.wallpaperFilter.saturation, 100, 60, 100),
        brightness: num(raw.wallpaperFilter.brightness, 100, 80, 100),
      };
    }
    s.lunarCalendar = bool(raw.lunarCalendar, true);
    s.uiDensity = oneOf(raw.uiDensity, ["comfort", "compact"] as const, "comfort");
    s.uiRadius = oneOf(raw.uiRadius, ["round", "small", "sharp"] as const, "round");
    s.uiFont = str(raw.uiFont, "");
    s.focusRing = oneOf(raw.focusRing, ["system", "box", "underline"] as const, "system");
    const it = Number(raw.iconTier);
    s.iconTier = it === 16 || it === 24 ? (it as IconSizeTier) : 20;
    s.appTheme = oneOf(raw.appTheme, ["inherit", "deep-space", "paper", "minimal-black", "high-contrast"] as const, "inherit");
  } catch {
    return structuredClone(DEFAULT_AMBIENCE);
  }
  return s;
}
