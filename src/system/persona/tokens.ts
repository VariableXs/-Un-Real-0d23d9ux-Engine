/**
 * F151 主题令牌全集 · 完整设计（E 个性化域地基）。
 *
 * 主册判据（验收标准第一句）：24 色令牌全系统覆盖抽查（30 界面零硬编码——
 * B-1104 联动）；热替换全程无重启无闪烁；JSON schema 校验双向通过。
 *
 * 五类令牌全表（主册【功能定义】）：
 * - 颜色：语义色 24 枚——背景三层/前景两层/强调/成功/警告/危险/边框三档/禁用…
 * - 间距：乙-1 表六档（4/8/12/16/24/32，一处一事实与 theme-studio SP_BASE 同源）
 * - 圆角：三档（控件/卡片/窗口）
 * - 字号：四档（caption/body/title/display）
 * - 动效：F124 五曲线三时长（进入/退出/强调/弹性/线性 × 120/200/320ms）
 *
 * 纪律（主册【设计细节】）：
 * - 语义色不允许直接引用物理色——主题作者只写语义层，换主题才可能全局一致；
 * - 动效令牌含曲线函数与时长双值；
 * - 令牌表带版本戳（旧主题兼容矩阵文档化）；
 * - 导出含「未覆盖令牌清单」（作者自查）。
 *
 * 热替换：令牌表 → documentElement CSS 自定义属性（内联优先级高于 tokens.css
 * 亮度层）；同时桥接 theme-studio 既有 11 变量名（--bg-canvas 等），保证既有
 * 界面即时跟随——这是「全系统覆盖」判据的机械基础，不是口号。
 */

import { personaStore } from "./store";

// ---------- 第一类：颜色 24 枚 ----------

export type ColorGroup =
  | "bg"      // 背景三层
  | "fg"      // 前景两层 + 禁用
  | "brand"   // 强调族
  | "state"   // 成功/警告/危险及其弱底
  | "line"    // 边框三档
  | "misc";   // 遮罩/悬停/焦点/选区

export interface ColorTokenDef {
  key: string;
  zh: string;
  en: string;
  group: ColorGroup;
  /** 半透明令牌：校验对比度前先按 WCAG 规则合成到底色。 */
  translucent?: boolean;
}

export const COLOR_TOKENS: ColorTokenDef[] = [
  { key: "--p-bg-canvas", zh: "画布底色", en: "Canvas", group: "bg" },
  { key: "--p-bg-surface", zh: "面板底色", en: "Surface", group: "bg", translucent: true },
  { key: "--p-bg-raised", zh: "浮层底色", en: "Raised", group: "bg", translucent: true },
  { key: "--p-fg-primary", zh: "主文字", en: "Text primary", group: "fg" },
  { key: "--p-fg-secondary", zh: "次要文字", en: "Text secondary", group: "fg" },
  { key: "--p-fg-disabled", zh: "禁用文字", en: "Text disabled", group: "fg" },
  { key: "--p-accent", zh: "强调色", en: "Accent", group: "brand" },
  { key: "--p-accent-soft", zh: "强调色·弱底", en: "Accent soft", group: "brand", translucent: true },
  { key: "--p-on-accent", zh: "强调色上文字", en: "On accent", group: "brand" },
  { key: "--p-success", zh: "成功", en: "Success", group: "state" },
  { key: "--p-warn", zh: "警告", en: "Warn", group: "state" },
  { key: "--p-danger", zh: "危险", en: "Danger", group: "state" },
  { key: "--p-success-soft", zh: "成功·弱底", en: "Success soft", group: "state", translucent: true },
  { key: "--p-warn-soft", zh: "警告·弱底", en: "Warn soft", group: "state", translucent: true },
  { key: "--p-danger-soft", zh: "危险·弱底", en: "Danger soft", group: "state", translucent: true },
  { key: "--p-border-strong", zh: "边框·重", en: "Border strong", group: "line", translucent: true },
  { key: "--p-border-regular", zh: "边框·常", en: "Border regular", group: "line", translucent: true },
  { key: "--p-border-subtle", zh: "边框·淡", en: "Border subtle", group: "line", translucent: true },
  { key: "--p-bg-disabled", zh: "禁用底色", en: "Disabled bg", group: "misc", translucent: true },
  { key: "--p-scrim", zh: "遮罩", en: "Scrim", group: "misc", translucent: true },
  { key: "--p-hover-lift", zh: "悬停亮层", en: "Hover lift", group: "misc", translucent: true },
  { key: "--p-focus-ring", zh: "焦点环", en: "Focus ring", group: "misc" },
  { key: "--p-selection", zh: "选区底色", en: "Selection", group: "misc", translucent: true },
  { key: "--p-shadow", zh: "投影色", en: "Shadow", group: "misc", translucent: true },
];

/** 24 枚断言：清单与数字必须同步（一处一事实，改一处必改另一处）。 */
export const COLOR_TOKEN_COUNT = 24;

// ---------- 第二类：间距六档（乙-1 表） ----------

export const SPACING_TOKENS: readonly { key: string; px: number; zh: string; en: string }[] = [
  { key: "--p-sp-1", px: 4, zh: "间距一档", en: "Spacing 1" },
  { key: "--p-sp-2", px: 8, zh: "间距二档", en: "Spacing 2" },
  { key: "--p-sp-3", px: 12, zh: "间距三档", en: "Spacing 3" },
  { key: "--p-sp-4", px: 16, zh: "间距四档", en: "Spacing 4" },
  { key: "--p-sp-5", px: 24, zh: "间距五档", en: "Spacing 5" },
  { key: "--p-sp-6", px: 32, zh: "间距六档", en: "Spacing 6" },
] as const;

// ---------- 第三类：圆角三档 ----------

export interface RadiusTable { control: number; card: number; window: number }
export const RADIUS_DEFAULTS: RadiusTable = { control: 8, card: 12, window: 16 };
export const RADIUS_KEYS: readonly { key: string; slot: keyof RadiusTable }[] = [
  { key: "--p-r-control", slot: "control" },
  { key: "--p-r-card", slot: "card" },
  { key: "--p-r-window", slot: "window" },
] as const;

// ---------- 第四类：字号四档 ----------

export interface FontTable { caption: number; body: number; title: number; display: number }
export const FONT_DEFAULTS: FontTable = { caption: 12, body: 15, title: 20, display: 32 };
export const FONT_KEYS: readonly { key: string; slot: keyof FontTable }[] = [
  { key: "--p-fs-caption", slot: "caption" },
  { key: "--p-fs-body", slot: "body" },
  { key: "--p-fs-title", slot: "title" },
  { key: "--p-fs-display", slot: "display" },
] as const;

// ---------- 第五类：动效令牌（F124 五曲线三时长，双值=曲线+时长） ----------

/** F124 五曲线（主册设计细节原文参数，一处一事实）。 */
export type MotionCurve = "enter" | "exit" | "emphasized" | "spring" | "linear";

export const MOTION_CURVES: Record<MotionCurve, { bezier: string; zh: string; en: string }> = {
  enter: { bezier: "cubic-bezier(0.16, 1, 0.3, 1)", zh: "进入", en: "Enter" },
  exit: { bezier: "cubic-bezier(0.7, 0, 0.84, 0)", zh: "退出", en: "Exit" },
  emphasized: { bezier: "cubic-bezier(0.65, 0, 0.35, 1)", zh: "强调", en: "Emphasized" },
  spring: { bezier: "cubic-bezier(0.34, 1.56, 0.64, 1)", zh: "弹性 105% 过冲", en: "Spring (105% overshoot)" },
  linear: { bezier: "linear", zh: "线性（仅进度条）", en: "Linear (progress only)" },
};

/** F124 三时长档（进入=200 / 微反馈=120 / 大面板=320）。 */
export type MotionDuration = "micro" | "standard" | "panel";
export const MOTION_DURATIONS: Record<MotionDuration, { ms: number; zh: string; en: string }> = {
  micro: { ms: 120, zh: "微反馈", en: "Micro feedback" },
  standard: { ms: 200, zh: "进入标准", en: "Standard" },
  panel: { ms: 320, zh: "大面板", en: "Large panel" },
};

export interface MotionToken { curve: MotionCurve; duration: MotionDuration }
export const MOTION_DEFAULTS: Record<MotionCurve, MotionToken> = {
  enter: { curve: "enter", duration: "standard" },
  exit: { curve: "exit", duration: "micro" },
  emphasized: { curve: "emphasized", duration: "panel" },
  spring: { curve: "spring", duration: "micro" },
  linear: { curve: "linear", duration: "panel" },
};

// ---------- 令牌表结构与默认值 ----------

export const TOKEN_TABLE_VERSION = 1;

export interface TokenTable {
  colors: Record<string, string>;       // 24 语义色（hex，#rrggbb 或 #rrggbbaa）
  radius: RadiusTable;                  // 圆角三档（px）
  font: FontTable;                      // 字号四档（px）
  motion: Record<MotionCurve, MotionToken>; // 动效双值
  version: number;
}

const HEX_RE = /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/;

/** 默认令牌表（深色系——与 tokens.css 深色亮度层同气质）。 */
export function defaultTokenTable(): TokenTable {
  return {
    colors: {
      "--p-bg-canvas": "#14141c",
      "--p-bg-surface": "#1c1c26b8",
      "--p-bg-raised": "#22222eb8",
      "--p-fg-primary": "#e8e8f0",
      "--p-fg-secondary": "#a0a0b4",
      "--p-fg-disabled": "#5a5a6c",
      "--p-accent": "#6e7fd4",
      "--p-accent-soft": "#6e7fd429",
      "--p-on-accent": "#ffffff",
      "--p-success": "#5fbf8a",
      "--p-warn": "#d4b45f",
      "--p-danger": "#d4685f",
      "--p-success-soft": "#5fbf8a26",
      "--p-warn-soft": "#d4b45f26",
      "--p-danger-soft": "#d4685f26",
      "--p-border-strong": "#8c8ca040",
      "--p-border-regular": "#8c8ca026",
      "--p-border-subtle": "#8c8ca014",
      "--p-bg-disabled": "#2c2c38b0",
      "--p-scrim": "#00000099",
      "--p-hover-lift": "#ffffff0f",
      "--p-focus-ring": "#8ea0ff",
      "--p-selection": "#6e7fd44d",
      "--p-shadow": "#00000066",
    },
    radius: { ...RADIUS_DEFAULTS },
    font: { ...FONT_DEFAULTS },
    motion: JSON.parse(JSON.stringify(MOTION_DEFAULTS)) as Record<MotionCurve, MotionToken>,
    version: TOKEN_TABLE_VERSION,
  };
}

// ---------- 校验（双向：JSON→对象、对象→JSON 都走同一套判定） ----------

export interface TokenIssue {
  field: string;
  message: string;
  level: "error" | "conflict" | "warning";
}

export interface TokenParseResult {
  ok: boolean;
  issues: TokenIssue[];
  data: TokenTable | null;
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function clampPx(v: unknown, field: string, issues: TokenIssue[], min: number, max: number, fallback: number): number {
  if (typeof v !== "number" || !Number.isFinite(v)) {
    issues.push({ field, message: "须为数字", level: "error" });
    return fallback;
  }
  if (v < min || v > max) {
    issues.push({ field, message: `超出允许区间 ${min}-${max}px，已钳制`, level: "warning" });
  }
  return Math.min(max, Math.max(min, Math.round(v)));
}

/**
 * 令牌表校验：未知令牌报 conflict（不阻断——旧主题兼容矩阵），非法值报 error。
 * 令牌缺失 → 默认值兜底并报 warning（覆盖度标注的输入）。
 */
export function validateTokenTable(raw: unknown): TokenParseResult {
  const issues: TokenIssue[] = [];
  const defaults = defaultTokenTable();
  if (!isRecord(raw)) {
    return { ok: false, issues: [{ field: "", message: "不是 JSON 对象", level: "error" }], data: null };
  }
  const version = raw.version;
  if (version !== TOKEN_TABLE_VERSION) {
    if (typeof version === "number" && version < TOKEN_TABLE_VERSION) {
      issues.push({ field: "version", message: `旧版本令牌表（${version}），按 v${TOKEN_TABLE_VERSION} 兜底缺省`, level: "warning" });
    } else {
      issues.push({ field: "version", message: `version 须为 ${TOKEN_TABLE_VERSION}`, level: "error" });
    }
  }

  // 颜色
  const colors: Record<string, string> = {};
  const rawColors = raw.colors;
  const knownKeys = new Set(COLOR_TOKENS.map((t) => t.key));
  if (isRecord(rawColors)) {
    for (const [k, v] of Object.entries(rawColors)) {
      if (!knownKeys.has(k)) {
        issues.push({ field: `colors.${k}`, message: "未知语义令牌，导入时忽略", level: "conflict" });
        continue;
      }
      if (typeof v !== "string" || !HEX_RE.test(v)) {
        issues.push({ field: `colors.${k}`, message: "颜色值须为 #rrggbb 或 #rrggbbaa", level: "error" });
        continue;
      }
      colors[k] = v;
    }
  } else {
    issues.push({ field: "colors", message: "colors 缺失或不是对象", level: "error" });
  }
  const missing = COLOR_TOKENS.filter((t) => !(t.key in colors));
  for (const m of missing) {
    issues.push({ field: `colors.${m.key}`, message: "令牌缺失，默认值兜底", level: "warning" });
    colors[m.key] = defaults.colors[m.key] ?? "#000000";
  }

  // 圆角
  const radius: RadiusTable = { ...RADIUS_DEFAULTS };
  const rawRadius = raw.radius;
  if (isRecord(rawRadius)) {
    radius.control = clampPx(rawRadius.control, "radius.control", issues, 0, 32, RADIUS_DEFAULTS.control);
    radius.card = clampPx(rawRadius.card, "radius.card", issues, 0, 32, RADIUS_DEFAULTS.card);
    radius.window = clampPx(rawRadius.window, "radius.window", issues, 0, 32, RADIUS_DEFAULTS.window);
  } else {
    issues.push({ field: "radius", message: "radius 缺失，默认值兜底", level: "warning" });
  }

  // 字号
  const font: FontTable = { ...FONT_DEFAULTS };
  const rawFont = raw.font;
  if (isRecord(rawFont)) {
    font.caption = clampPx(rawFont.caption, "font.caption", issues, 10, 20, FONT_DEFAULTS.caption);
    font.body = clampPx(rawFont.body, "font.body", issues, 12, 24, FONT_DEFAULTS.body);
    font.title = clampPx(rawFont.title, "font.title", issues, 14, 40, FONT_DEFAULTS.title);
    font.display = clampPx(rawFont.display, "font.display", issues, 18, 96, FONT_DEFAULTS.display);
  } else {
    issues.push({ field: "font", message: "font 缺失，默认值兜底", level: "warning" });
  }

  // 动效
  const motion = JSON.parse(JSON.stringify(MOTION_DEFAULTS)) as Record<MotionCurve, MotionToken>;
  const rawMotion = raw.motion;
  const curveNames = new Set(Object.keys(MOTION_CURVES));
  if (isRecord(rawMotion)) {
    for (const [k, v] of Object.entries(rawMotion)) {
      if (!curveNames.has(k)) {
        issues.push({ field: `motion.${k}`, message: "未知动效令牌，导入时忽略", level: "conflict" });
        continue;
      }
      if (!isRecord(v)) {
        issues.push({ field: `motion.${k}`, message: "动效令牌须为 {curve,duration}", level: "error" });
        continue;
      }
      if (typeof v.curve === "string" && curveNames.has(v.curve)) {
        motion[k as MotionCurve].curve = v.curve as MotionCurve;
      } else {
        issues.push({ field: `motion.${k}.curve`, message: "曲线名非法", level: "error" });
      }
      if (typeof v.duration === "string" && v.duration in MOTION_DURATIONS) {
        motion[k as MotionCurve].duration = v.duration as MotionDuration;
      } else {
        issues.push({ field: `motion.${k}.duration`, message: "时长档非法", level: "error" });
      }
    }
  }

  const ok = !issues.some((i) => i.level === "error");
  return { ok, issues, data: ok ? { colors, radius, font, motion, version: TOKEN_TABLE_VERSION } : null };
}

// ---------- 覆盖度报告（作者自查） ----------

export interface CoverageReport {
  total: number;
  covered: number;
  missing: string[]; // 未覆盖令牌清单（导出时作者自查）
  unknown: string[]; // 主题包里存在但系统不认识的令牌
}

export function coverageReport(table: TokenTable, provided?: Record<string, string>): CoverageReport {
  const providedKeys = provided ? new Set(Object.keys(provided)) : new Set(Object.keys(table.colors));
  const known = COLOR_TOKENS.map((t) => t.key);
  const missing = known.filter((k) => !providedKeys.has(k));
  const unknown = [...providedKeys].filter((k) => !known.includes(k));
  return { total: COLOR_TOKEN_COUNT, covered: COLOR_TOKEN_COUNT - missing.length, missing, unknown };
}

// ---------- JSON Schema 发布（F126 联动） ----------

/** 生成令牌表 JSON Schema（草稿-07）——F151「JSON Schema 发布（F126）」判据。 */
export function tokenTableJsonSchema(): Record<string, unknown> {
  const colorPattern = "^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$";
  const colorProps: Record<string, unknown> = {};
  for (const t of COLOR_TOKENS) {
    colorProps[t.key] = { type: "string", pattern: colorPattern, title: t.zh, description: t.en };
  }
  return {
    $schema: "https://json-schema.org/draft/2020-12/schema",
    $id: "varix:persona:token-table:v1",
    title: "Varix 主题令牌表 v1",
    type: "object",
    required: ["colors", "radius", "font", "motion", "version"],
    additionalProperties: false,
    properties: {
      version: { const: TOKEN_TABLE_VERSION },
      colors: { type: "object", propertyNames: { enum: COLOR_TOKENS.map((t) => t.key) }, additionalProperties: { type: "string", pattern: colorPattern } },
      radius: {
        type: "object",
        additionalProperties: false,
        properties: {
          control: { type: "integer", minimum: 0, maximum: 32 },
          card: { type: "integer", minimum: 0, maximum: 32 },
          window: { type: "integer", minimum: 0, maximum: 32 },
        },
      },
      font: {
        type: "object",
        additionalProperties: false,
        properties: {
          caption: { type: "integer", minimum: 10, maximum: 20 },
          body: { type: "integer", minimum: 12, maximum: 24 },
          title: { type: "integer", minimum: 14, maximum: 40 },
          display: { type: "integer", minimum: 18, maximum: 96 },
        },
      },
      motion: {
        type: "object",
        propertyNames: { enum: Object.keys(MOTION_CURVES) },
        additionalProperties: {
          type: "object",
          additionalProperties: false,
          properties: {
            curve: { enum: Object.keys(MOTION_CURVES) },
            duration: { enum: Object.keys(MOTION_DURATIONS) },
          },
        },
      },
      ...colorProps,
    },
  };
}

// ---------- 热替换引擎 ----------

/** 令牌 → CSS 变量值展开（纯函数，测试友好）。 */
export function tokenVarValues(table: TokenTable): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(table.colors)) out[k] = v;
  for (const { key, slot } of RADIUS_KEYS) out[key] = `${table.radius[slot]}px`;
  for (const { key, slot } of FONT_KEYS) out[key] = `${table.font[slot]}px`;
  for (const { key, px } of SPACING_TOKENS) out[key] = `${px}px`;
  for (const [name, tok] of Object.entries(table.motion)) {
    out[`--p-motion-${name}`] = MOTION_DURATIONS[tok.duration].ms.toString();
    out[`--p-ease-${name}`] = MOTION_CURVES[tok.curve].bezier;
  }
  return out;
}

/**
 * 既有 theme-studio 变量桥接：persona 语义色 → tokens.css 既有变量名。
 * 让既有界面（未迁移 --p-* 前缀的 30 界面抽查面）即时跟随——零硬编码判据的机械通路。
 */
export const LEGACY_BRIDGE: Record<string, string> = {
  "--p-bg-canvas": "--bg-canvas",
  "--p-bg-surface": "--bg-surface",
  "--p-bg-raised": "--bg-raised",
  "--p-fg-primary": "--text-primary",
  "--p-fg-secondary": "--text-secondary",
  "--p-accent": "--accent",
  "--p-accent-soft": "--accent-soft",
  "--p-success": "--success",
  "--p-warn": "--warn",
  "--p-danger": "--danger",
  "--p-border-regular": "--stroke",
  "--p-border": "--border",
};

export const ALL_PERSONA_VARS: string[] = (() => {
  const vals = tokenVarValues(defaultTokenTable());
  return [...Object.keys(vals), ...Object.values(LEGACY_BRIDGE)];
})();

export function applyTokenTableToDOM(table: TokenTable): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  const vars = tokenVarValues(table);
  for (const [k, v] of Object.entries(vars)) {
    root.style.setProperty(k, v);
    const legacy = LEGACY_BRIDGE[k];
    if (legacy) root.style.setProperty(legacy, v);
  }
  // 桥接 --border（tokens.css L105 中 --border: var(--stroke) 的正身写入）。
  if (vars["--p-border-regular"]) root.style.setProperty("--border", vars["--p-border-regular"]);
}

/** 清除热替换写入的全部变量（放弃/回退路径——令牌表哈希还原验证的对面）。 */
export function clearTokenTableFromDOM(): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  for (const k of ALL_PERSONA_VARS) root.style.removeProperty(k);
  root.style.removeProperty("--border");
}

// ---------- 配置持久化（persona.theme 节） ----------

const SECTION = "theme";
const TOKEN_KEY = "tokenTable";

export function loadTokenTable(): TokenTable {
  const stored = personaStore.getWith(SECTION, TOKEN_KEY, undefined);
  if (stored !== undefined) {
    const r = validateTokenTable(stored);
    if (r.ok && r.data) return r.data;
  }
  return defaultTokenTable();
}

export function saveTokenTable(table: TokenTable): TokenIssue[] {
  const r = validateTokenTable(table);
  if (r.ok && r.data) personaStore.set(SECTION, { [TOKEN_KEY]: r.data });
  return r.issues;
}

/** 单令牌回退（主册交互设计「恢复此令牌默认」）。 */
export function resetToken(table: TokenTable, key: string): TokenTable {
  const d = defaultTokenTable();
  if (key in table.colors) {
    const value = d.colors[key] ?? table.colors[key] ?? "#000000";
    return { ...table, colors: { ...table.colors, [key]: value } };
  }
  return table;
}

/** 令牌表哈希（放弃=零残留的哈希还原验证用；FNV-1a 纯实现）。 */
export function tokenTableHash(table: TokenTable): string {
  const s = JSON.stringify(table);
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16).padStart(8, "0");
}
