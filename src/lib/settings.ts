import { errMessage, ipc } from "./ipc";
import type { Lang } from "../i18n/dictionaries";

export type ThemeId = "deep-space" | "paper" | "minimal-black" | "custom";
export type PerfMode = "high" | "balanced" | "eco" | "static" | "auto";
export type BgType = "nebula" | "color" | "gradient" | "image" | "video";

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
  perfMode: PerfMode;
  launchAnim: boolean;
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
  perfMode: "high",
  launchAnim: true,
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
    if (raw["language"]) s.language = raw["language"] === "en" ? "en" : "zh";
    if (raw["theme"]) s.theme = raw["theme"] as ThemeId;
    if (raw["perfMode"]) s.perfMode = raw["perfMode"] as PerfMode;
    if (raw["launchAnim"] !== undefined) s.launchAnim = raw["launchAnim"] === "1";
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
