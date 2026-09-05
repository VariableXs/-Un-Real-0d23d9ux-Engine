import { errMessage, ipc } from "./ipc";
import type { Lang } from "../i18n/dictionaries";

export type ThemeId = "deep-space" | "paper" | "minimal-black" | "custom";
export type PerfMode = "high" | "balanced" | "eco" | "static" | "auto";
export type BgType = "nebula" | "color" | "gradient" | "image" | "video";
/** 桌面环境壁纸模式（M2）：纯黑 / 3D 引力场 / 视频 / 图片 / 混合（媒体+星空叠加）。 */
export type WallpaperMode = "solid" | "gravity" | "video" | "image" | "hybrid";
/**
 * 启动动画（批次A，规格 14.2.1）：控制真实加载完成后的过渡编排。
 * - full：字母落位任务栏 + 任务栏展开 + 图标淡入（阶段4/5 完整编排）
 * - simple：快速交叉淡入
 * - none：直接进入桌面
 * 进度/日志本身始终是真实事件，此设置只影响 ready 之后的过渡形式。
 */
export type BootAnim = "full" | "simple" | "none";
/** 桌面图标三档大小（批次B，规格 4.2.4）：像素为图标底座边长。 */
export type IconSize = 32 | 48 | 64;
/** 窗口控制按钮位置（批次D，规格 4.3.5）：Mac 圆点（默认）/ Windows 风格。 */
export type WinControls = "mac" | "windows";
/** 任务栏停靠位置（批次E，规格 4.4）：底（默认）/ 左 / 右 / 顶。 */
export type TaskbarPos = "bottom" | "left" | "right" | "top";

export interface CustomBg {
  type: BgType;
  color: string;
  gradientFrom: string;
  gradientTo: string;
  imagePath: string;
  videoPath: string;
  blur: number;      // 0..24 px
  brightness: number; // 0.2..1.4
  vignette: number;   // 0..1
  saturation: number; // 0..2
  maskOpacity: number;// 0..1 (center darkening over writing area)
  dynamicStrength: number; // 0..1
  parallaxStrength: number; // 0..1
  playVideo: boolean;
}

export type GridMode = "dot" | "grid" | "iso" | "none";

export interface MindDefaults {
  gridEnabled: boolean;
  snapEnabled: boolean;
  gridMode: GridMode;
  gridColor: string;
  gridOpacity: number; // 0..1
  guidesEnabled: boolean;
  defaultShape: import("../lib/types").NodeShape;
  resizeSensitivity: number; // px threshold for handle activation
  edgeStyle: import("../lib/types").LineStyle;
  edgeAnim: boolean;
  wasdSpeed: number; // px/s base
}

export interface Settings {
  language: Lang;
  theme: ThemeId;
  /** 桌面壁纸模式（桌面环境 L0 显示层；与四软件内部主题互不影响）。 */
  wallpaperMode: WallpaperMode;
  /** 启动动画过渡形式（真实加载完成后的阶段4/5 编排开关）。 */
  bootAnim: BootAnim;
  /** 桌面图标大小三档（32/48/64）。 */
  iconSize: IconSize;
  /** 窗口控制按钮位置（桌面红绿灯 + 软件窗口控制条，规格 4.3.5）。 */
  winControls: WinControls;
  /** 任务栏停靠位置（批次E，规格 4.4）。 */
  taskbarPos: TaskbarPos;
  /** 快捷键自定义（批次E，规格 4.7）：action → accel；空 = 全默认。冲突检测在前端设置页。 */
  shortcutBinds: Record<string, string>;
  /** 🟢 绿灯状态（批次D，规格 4.3.4）：true = 避让 Windows 任务栏。 */
  avoidTaskbar: boolean;
  /** 首次启动欢迎向导已完成（完成后不再显示）。 */
  wizardDone: boolean;
  /** 批次E-6：每日自动换壁纸（本地缓存目录按日期取图，零网络）。 */
  wallpaperDaily: boolean;
  /** 批次E-6：壁纸本地缓存目录（Bing 缓存等自备图片文件夹）。 */
  wallpaperPoolDir: string;
  /** 批次E-6：Win+Tab 多窗口切换器开关（true = Variable 接管 Win+Tab）。 */
  winTabSwitcher: boolean;
  perfMode: PerfMode;
  showStatusBar: boolean;
  editorWidthPct: number; // 58..72
  editorAlign: "center" | "left" | "right";
  fontFamily: string;
  fontSize: number; // 14..22
  lineHeight: number; // 1.5..2.2
  autosaveDelayMs: number; // 400..3000
  uiZoom: number; // 0.85..1.3
  reduceMotion: boolean;
  safeMode: boolean;
  /** Anime starfield performance tier: 1..10 fixed, 0 = smart auto monitor. */
  bgTier: number;
  /** 8.2 用户教学式词典：大白话解释，key 为小写术语。 */
  pvzDictOverrides: Record<string, string>;
  customBg: CustomBg;
  mindDefaults: MindDefaults;
}

export const DEFAULT_SETTINGS: Settings = {
  language: "zh",
  theme: "deep-space",
  wallpaperMode: "gravity",
  bootAnim: "full",
  iconSize: 48,
  winControls: "mac",
  taskbarPos: "bottom",
  shortcutBinds: {},
  avoidTaskbar: false,
  wizardDone: false,
  wallpaperDaily: false,
  wallpaperPoolDir: "",
  winTabSwitcher: true,
  perfMode: "high",
  showStatusBar: true,
  editorWidthPct: 64,
  editorAlign: "center",
  fontFamily: `"Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif`,
  fontSize: 16,
  lineHeight: 1.75,
  autosaveDelayMs: 1200,
  uiZoom: 1,
  reduceMotion: false,
  safeMode: false,
  bgTier: 0,
  pvzDictOverrides: {},
  customBg: {
    type: "nebula",
    color: "#0a1226",
    gradientFrom: "#0a1638",
    gradientTo: "#04070f",
    imagePath: "",
    videoPath: "",
    blur: 6,
    brightness: 0.9,
    vignette: 0.55,
    saturation: 1,
    maskOpacity: 0.35,
    dynamicStrength: 0.5,
    parallaxStrength: 0.4,
    playVideo: true,
  },
  mindDefaults: {
    gridEnabled: true,
    snapEnabled: true,
    gridMode: "grid",
    gridColor: "#1e3a8a", // chapter 6.3: WASD grid dots as faint ice-blue star dust
    gridOpacity: 0.16,
    guidesEnabled: true,
    defaultShape: "rounded",
    resizeSensitivity: 8,
    edgeStyle: "solid",
    edgeAnim: true,
    wasdSpeed: 520,
  },
};

function coerce(raw: Record<string, string>): Settings {
  const s: Settings = structuredClone(DEFAULT_SETTINGS);
  try {
    if (raw["language"] === "en" || raw["language"] === "zh-TW" || raw["language"] === "zh") {
      s.language = raw["language"];
    }
    if (raw["theme"]) s.theme = raw["theme"] as ThemeId;
    if (raw["wallpaperMode"]) s.wallpaperMode = raw["wallpaperMode"] as WallpaperMode;
    if (raw["bootAnim"]) {
      const v = raw["bootAnim"];
      s.bootAnim = v === "simple" || v === "none" ? v : v === "full" ? "full" : s.bootAnim;
    }
    if (raw["iconSize"]) {
      const n = Number(raw["iconSize"]);
      s.iconSize = n === 32 || n === 64 ? (n as IconSize) : n === 48 ? 48 : s.iconSize;
    }
    if (raw["wizardDone"] !== undefined) s.wizardDone = raw["wizardDone"] === "1";
    if (raw["winControls"]) s.winControls = raw["winControls"] === "windows" ? "windows" : "mac";
    if (raw["avoidTaskbar"] !== undefined) s.avoidTaskbar = raw["avoidTaskbar"] === "1";
    if (raw["wallpaperDaily"] !== undefined) s.wallpaperDaily = raw["wallpaperDaily"] === "1";
    if (raw["wallpaperPoolDir"]) s.wallpaperPoolDir = raw["wallpaperPoolDir"];
    if (raw["winTabSwitcher"] !== undefined) s.winTabSwitcher = raw["winTabSwitcher"] !== "0";
    if (raw["perfMode"]) s.perfMode = raw["perfMode"] as PerfMode;
    if (raw["showStatusBar"] !== undefined) s.showStatusBar = raw["showStatusBar"] === "1";
    if (raw["reduceMotion"] !== undefined) s.reduceMotion = raw["reduceMotion"] === "1";
    if (raw["safeMode"] !== undefined) s.safeMode = raw["safeMode"] === "1";
    if (raw["editorWidthPct"]) s.editorWidthPct = clamp(Number(raw["editorWidthPct"]) || DEFAULT_SETTINGS.editorWidthPct, 58, 72);
    if (raw["editorAlign"]) s.editorAlign = raw["editorAlign"] as Settings["editorAlign"];
    if (raw["fontFamily"]) s.fontFamily = raw["fontFamily"];
    if (raw["fontSize"]) s.fontSize = clamp(Number(raw["fontSize"]) || 16, 12, 26);
    if (raw["lineHeight"]) s.lineHeight = clamp(Number(raw["lineHeight"]) || 1.75, 1.3, 2.4);
    if (raw["autosaveDelayMs"]) s.autosaveDelayMs = clamp(Number(raw["autosaveDelayMs"]) || 1200, 300, 5000);
    if (raw["uiZoom"]) s.uiZoom = clamp(Number(raw["uiZoom"]) || 1, 0.85, 1.3);
    if (raw["bgTier"] !== undefined) s.bgTier = clamp(Number(raw["bgTier"]) || 0, 0, 17);
    if (raw["pvzDictOverrides"]) {
      const parsed = JSON.parse(raw["pvzDictOverrides"]) as unknown;
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
          if (typeof v === "string" && v.trim()) s.pvzDictOverrides[k.toLowerCase().trim()] = v.trim();
        }
      }
    }
    if (raw["customBg"]) s.customBg = { ...s.customBg, ...JSON.parse(raw["customBg"]) };
    if (raw["mindDefaults"]) {
      const md = { ...s.mindDefaults, ...JSON.parse(raw["mindDefaults"]) };
      // Clamp numeric ranges so a corrupt stored value (e.g. wasdSpeed 0) can
      // never silently kill WASD navigation or handle activation.
      md.wasdSpeed = clamp(Number(md.wasdSpeed) || DEFAULT_SETTINGS.mindDefaults.wasdSpeed, 200, 1200);
      md.resizeSensitivity = clamp(Number(md.resizeSensitivity) || DEFAULT_SETTINGS.mindDefaults.resizeSensitivity, 2, 40);
      md.gridOpacity = clamp(Number(md.gridOpacity) || DEFAULT_SETTINGS.mindDefaults.gridOpacity, 0, 1);
      s.mindDefaults = md;
    }
  } catch {
    // Corrupt settings fall back to defaults for the affected keys.
  }
  return s;
}

export function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}

const pendingWrites = new Map<string, string>();
let flushTimer: ReturnType<typeof setTimeout> | null = null;

async function flush(): Promise<void> {
  if (pendingWrites.size === 0) return;
  const entries = Object.fromEntries(pendingWrites);
  pendingWrites.clear();
  await ipc.setSettings(entries);
}

export async function loadSettings(): Promise<Settings> {
  try {
    const raw = await ipc.getSettings();
    return coerce(raw);
  } catch {
    return structuredClone(DEFAULT_SETTINGS);
  }
}

/** Persist one setting; failures are surfaced via returned error message key. */
export async function saveSetting(key: keyof Settings & string, value: unknown): Promise<void> {
  let serialized: string;
  if (typeof value === "boolean") serialized = value ? "1" : "0";
  else if (typeof value === "object") serialized = JSON.stringify(value);
  else serialized = String(value);
  pendingWrites.set(key, serialized);
  if (flushTimer) clearTimeout(flushTimer);
  flushTimer = setTimeout(() => {
    flush().catch((e) => {
      const { code } = errMessage(e);
      console.error(`[settings] persist failed (${code})`, e);
      window.dispatchEvent(new CustomEvent("variable:settings-error", { detail: code }));
    });
  }, 350);
  await new Promise<void>((r) => void r());
}
