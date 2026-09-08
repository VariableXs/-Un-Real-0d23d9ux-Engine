/**
 * AI-06 输入手感组 — 数据模型与纯逻辑（U-58/U-59、V-61…V-70）。
 *
 * 红线（承化境/ASCENT 原计划）：
 * - V-61 鼠标参数写回系统须显式确认与回滚（后端 mousefeel.rs 备份→写→可回滚）；
 * - V-66 按键重映射仅单键→单键/修饰键，环境内 capture 层翻译，
 *   不修改四大内置应用任何代码；
 * - 全部默认关闭或等于现状（默认即现状原则）。
 *
 * 本文件只放纯函数与常量（可测）；运行时副作用见
 * src/features/inputFeel/InputFeelRuntime.tsx，设置面板见
 * src/features/settings/InputFeelTab.tsx。
 */

import { clamp } from "./format";

// ---------------------------------------------------------------------------
// V-70 拖拽阈值与防手滑
// ---------------------------------------------------------------------------

/** 默认拖拽启动阈值（px，2..10 可调）。 */
export const DEFAULT_DRAG_THRESHOLD = 4;

/** 触控输入阈值放大倍数（手指精度低于鼠标）。 */
export const TOUCH_THRESHOLD_SCALE = 2;

/**
 * 判定按住移动是否达到拖拽启动阈值。
 * @param dx dy 相对按下点的位移
 * @param threshold 全局统一阈值（px）
 * @param pointerType "touch"/"pen" 自动放大阈值
 */
export function isDragStart(dx: number, dy: number, threshold: number, pointerType?: string): boolean {
  const t = Math.max(2, Math.min(10, threshold));
  const eff = pointerType === "touch" || pointerType === "pen" ? t * TOUCH_THRESHOLD_SCALE : t;
  return Math.hypot(dx, dy) >= eff;
}

// ---------------------------------------------------------------------------
// V-61 鼠标手感面板（环境内参数 + 写回系统）
// ---------------------------------------------------------------------------

export interface MouseParams {
  /** 指针速度（Windows SPI 速度档 1..20）。 */
  speed: number;
  /** 双击间隔（ms，200..900，Windows 默认 500）。 */
  doubleClickMs: number;
  /** 滚轮行数（1..10，默认 3）。 */
  wheelLines: number;
  /** 交换左右键。 */
  swapButtons: boolean;
}

export const DEFAULT_MOUSE_PARAMS: MouseParams = {
  speed: 10,
  doubleClickMs: 500,
  wheelLines: 3,
  swapButtons: false,
};

export function coerceMouseParams(raw: Partial<MouseParams> | undefined | null): MouseParams {
  const d = DEFAULT_MOUSE_PARAMS;
  if (!raw) return { ...d };
  return {
    speed: clamp(Math.round(Number(raw.speed) || d.speed), 1, 20),
    doubleClickMs: clamp(Math.round(Number(raw.doubleClickMs) || d.doubleClickMs), 200, 900),
    // 0 是合法输入（钳到下限 1），不能用 || 短路
    wheelLines: raw.wheelLines === undefined || raw.wheelLines === null || Number.isNaN(Number(raw.wheelLines))
      ? d.wheelLines
      : clamp(Math.round(Number(raw.wheelLines)), 1, 10),
    swapButtons: raw.swapButtons === true,
  };
}

/**
 * 双击测试方块判定：两次点击间隔 ≤ doubleClickMs 且位移 < 6px 视为双击。
 */
export function isTestSquareDoubleHit(
  first: { x: number; y: number; t: number } | null,
  second: { x: number; y: number; t: number },
  doubleClickMs: number,
): boolean {
  if (!first) return false;
  return second.t - first.t <= doubleClickMs && Math.hypot(second.x - first.x, second.y - first.y) < 6;
}

// ---------------------------------------------------------------------------
// V-62 指针方案管理（Windows 13 态标准）
// ---------------------------------------------------------------------------

/** Windows 13 种指针状态（Windows.md 4.1 口径）。 */
export const POINTER_STATES = [
  "normalSelect",       // 正常选择
  "helpSelect",         // 帮助选择
  "workingInBackground", // 后台运行
  "busy",               // 忙
  "precisionSelect",    // 精确选择
  "textSelect",         // 文本选择
  "handwriting",        // 手写
  "unavailable",        // 不可用
  "verticalResize",     // 垂直调整
  "horizontalResize",   // 水平调整
  "diagonalResize1",    // 对角调整 ↘↖
  "diagonalResize2",    // 对角调整 ↗↙
  "move",               // 移动
] as const;

export type PointerState = (typeof POINTER_STATES)[number];

export type PointerSchemeId = "system" | "large" | "high-contrast" | "custom";

/** 内置方案：system = 完全现状（不动 cursor 样式）。 */
export const BUILTIN_POINTER_SCHEMES: PointerSchemeId[] = ["system", "large", "high-contrast"];

/**
 * 校验自定义指针包：13 态逐个核对。
 * @returns 缺失/非法的态列表（空 = 合法）。
 */
export function validateCustomCursorPack(pack: Partial<Record<PointerState, string>>): PointerState[] {
  const bad: PointerState[] = [];
  for (const st of POINTER_STATES) {
    const v = pack[st];
    if (typeof v !== "string" || !/\.(cur|ani)$/i.test(v.trim())) bad.push(st);
  }
  return bad;
}

// ---------------------------------------------------------------------------
// V-66 按键重映射（单键→单键 / 单键→修饰键）
// ---------------------------------------------------------------------------

export interface KeyRemapEntry {
  /** 源键（KeyboardEvent.code，如 "CapsLock"）。 */
  from: string;
  /** 目标键（code 或修饰键 "ControlLeft" 等）。 */
  to: string;
}

/** 允许重映射的源键白名单（字母/数字/F 键/CapsLock/Alt/Menu 等；不含 Esc 与组合）。 */
const REMAP_ALLOWED_FROM = new Set<string>([
  "CapsLock", "ContextMenu", "AltRight", "AltLeft", "ControlRight",
  "ShiftRight", "MetaRight", "Insert", "Delete", "Home", "End",
  "PageUp", "PageDown", "NumLock", "ScrollLock", "Pause",
  ...Array.from({ length: 26 }, (_, i) => `Key${String.fromCharCode(65 + i)}`),
  ...Array.from({ length: 12 }, (_, i) => `F${i + 1}`),
]);

const REMAP_ALLOWED_TO = new Set<string>([
  ...REMAP_ALLOWED_FROM,
  "ControlLeft", "ShiftLeft", "MetaLeft",
]);

export function isRemapFromAllowed(code: string): boolean {
  return REMAP_ALLOWED_FROM.has(code);
}

/** 面板下拉用源键清单（排序展示）。 */
export const REMAP_FROM_KEYS: string[] = [...REMAP_ALLOWED_FROM].sort();

/** 面板下拉用目标键清单（含左右修饰键）。 */
export const REMAP_TO_KEYS: string[] = [...REMAP_ALLOWED_TO].sort();

export function isRemapToAllowed(code: string): boolean {
  return REMAP_ALLOWED_TO.has(code);
}

/** 校验映射表；返回逐条错误（索引 + 原因）。 */
export function validateRemaps(remaps: KeyRemapEntry[]): { index: number; reason: string }[] {
  const errs: { index: number; reason: string }[] = [];
  const seenFrom = new Set<string>();
  remaps.forEach((r, i) => {
    if (!isRemapFromAllowed(r.from)) errs.push({ index: i, reason: "from-not-allowed" });
    if (!isRemapToAllowed(r.to)) errs.push({ index: i, reason: "to-not-allowed" });
    if (r.from === r.to) errs.push({ index: i, reason: "identity" });
    if (seenFrom.has(r.from)) errs.push({ index: i, reason: "duplicate-from" });
    seenFrom.add(r.from);
  });
  return errs;
}

export interface RemapMatchResult {
  /** 命中的映射。 */
  entry: KeyRemapEntry;
  /** 目标键的 code。 */
  code: string;
  /** 目标键的 key 名（用于合成事件）。 */
  key: string;
}

const CODE_TO_KEY: Record<string, string> = {
  CapsLock: "CapsLock", ControlLeft: "Control", ControlRight: "Control",
  AltLeft: "Alt", AltRight: "Alt", ShiftLeft: "Shift", ShiftRight: "Shift",
  MetaLeft: "Meta", MetaRight: "Meta", ContextMenu: "ContextMenu",
  NumLock: "NumLock", ScrollLock: "ScrollLock", Pause: "Pause",
  Insert: "Insert", Delete: "Delete", Home: "Home", End: "End",
  PageUp: "PageUp", PageDown: "PageDown",
};

/**
 * 键按下翻译（capture 层）：命中映射返回目标键信息，未命中返回 null（原键放行）。
 * 仅在无其他（未映射的）修饰键按住时生效——避免劫持系统组合键（Z-09 让位口径）。
 */
export function matchRemap(
  e: { code: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean; repeat: boolean },
  remaps: KeyRemapEntry[],
): RemapMatchResult | null {
  if (e.repeat) return null;
  const hit = remaps.find((r) => r.from === e.code);
  if (!hit) return null;
  // 让位协议：若当前已有其他修饰键按住（组合键场景），不做翻译
  const mods: [boolean, string][] = [
    [e.ctrlKey, "ControlLeft"], [e.altKey, "AltLeft"],
    [e.shiftKey, "ShiftLeft"], [e.metaKey, "MetaLeft"],
  ];
  const heldOther = mods.some(([held, code]) => held && !remaps.some((r) => r.from === code));
  if (heldOther) return null;
  const key = CODE_TO_KEY[hit.to] ?? (hit.to.startsWith("Key") ? hit.to.slice(3).toLowerCase() : hit.to.replace(/^Digit/, ""));
  return { entry: hit, code: hit.to, key };
}

// ---------------------------------------------------------------------------
// V-67 触控板自然滚动方向
// ---------------------------------------------------------------------------

/**
 * 自然滚动判定：开 = 触摸类指针 / 近期无外接鼠标移动的滚轮反转；
 * 鼠标滚轮（近期有鼠标指针移动）恒定保持原生方向。
 *
 * @param naturalOn 开关
 * @param pointerType 最近一次 pointermove 的 pointerType（"" = 未知/滚轮）
 * @param lastMouseMoveMs 最近一次鼠标（pointerType==="mouse"）移动的时间戳；0 = 从未
 */
export function shouldInvertWheel(naturalOn: boolean, pointerType: string, lastMouseMoveMs: number, nowMs: number): boolean {
  if (!naturalOn) return false;
  // 触摸/笔直接反转（触摸语义 = 内容跟随手指）
  if (pointerType === "touch" || pointerType === "pen") return true;
  // 滚轮：2 秒内有鼠标移动 → 视为鼠标滚轮，保持原生方向
  if (lastMouseMoveMs > 0 && nowMs - lastMouseMoveMs < 2000) return false;
  // 其余（触控板双指滚动通常无伴随鼠标移动）→ 自然方向
  return true;
}

// ---------------------------------------------------------------------------
// V-68 打字音效（三种克制音色）
// ---------------------------------------------------------------------------

export type TypingSoundId = "off" | "membrane" | "brown" | "rain";

export const TYPING_SOUNDS: TypingSoundId[] = ["off", "membrane", "brown", "rain"];

/** 音色基频（Hz）与随机 pitch 微变幅度（防听觉疲劳）。 */
export const TYPING_SOUND_PROFILES: Record<Exclude<TypingSoundId, "off">, { base: number; jitter: number; dur: number }> = {
  membrane: { base: 620, jitter: 60, dur: 0.045 }, // 静音薄膜
  brown: { base: 340, jitter: 40, dur: 0.055 },    // 茶轴
  rain: { base: 980, jitter: 140, dur: 0.035 },    // 雨点
};

// ---------------------------------------------------------------------------
// V-63/V-64 指针轨迹与点击涟漪
// ---------------------------------------------------------------------------

/** 轨迹长度三档对应的残影点数。 */
export const TRAIL_LEVELS = [8, 16, 28] as const;

/** 涟漪 240ms；reduce-motion 退化为 80ms 十字标。 */
export const RIPPLE_MS = 240;
export const RIPPLE_REDUCED_MS = 80;

// ---------------------------------------------------------------------------
// V-69 指针精确模式
// ---------------------------------------------------------------------------

export type PrecisionModifier = "alt" | "ctrl" | "shift";

export const DEFAULT_PRECISION_RATIO = 0.4;

/** 降速比例可调 20%–60%。 */
export function coercePrecisionRatio(v: number): number {
  return clamp(v || DEFAULT_PRECISION_RATIO, 0.2, 0.6);
}

// ---------------------------------------------------------------------------
// U-58 键盘全景：覆盖审计表
// ---------------------------------------------------------------------------

export interface CoverageRow {
  /** 操作名（i18n key 前缀 kbCov*）。 */
  opKey: string;
  /** 键盘路径（展示用 accel 串）。 */
  path: string;
  /** 是否已覆盖（未覆盖项在面板标红待清零）。 */
  covered: boolean;
}

/**
 * 覆盖审计表（静态登记 + 快捷键表同源断言见测试）：
 * 桌面级高频操作逐项登记键盘路径；数据与 shortcuts.ts 同源
 * （改键即同步——渲染时以 SHORTCUT_ACTIONS 当前值覆盖 path）。
 */
export const KEYBOARD_COVERAGE: CoverageRow[] = [
  { opKey: "kbCovNewDoc", path: "ctrl+n", covered: true },
  { opKey: "kbCovSearch", path: "ctrl+shift+f", covered: true },
  { opKey: "kbCovSettings", path: "ctrl+,", covered: true },
  { opKey: "kbCovExplorer", path: "ctrl+alt+e", covered: true },
  { opKey: "kbCovExplorerCtrl", path: "ctrl+e", covered: true },
  { opKey: "kbCovShowDesktop", path: "ctrl+alt+d", covered: true },
  { opKey: "kbCovSnapLeft", path: "ctrl+alt+left", covered: true },
  { opKey: "kbCovSnapRight", path: "ctrl+alt+right", covered: true },
  { opKey: "kbCovSnapUp", path: "ctrl+alt+up", covered: true },
  { opKey: "kbCovSnapDown", path: "ctrl+alt+down", covered: true },
  { opKey: "kbCovNotify", path: "ctrl+alt+n", covered: true },
  { opKey: "kbCovClipboard", path: "ctrl+alt+v", covered: true },
  { opKey: "kbCovDnd", path: "ctrl+shift+m", covered: true },
  { opKey: "kbCovMinimizeAll", path: "ctrl+alt+m", covered: true },
  { opKey: "kbCovToggleHide", path: "ctrl+shift+d", covered: true },
  { opKey: "kbCovCheatsheet", path: "ctrl+/", covered: true },
  { opKey: "kbCovWintab", path: "super+tab", covered: true },
  { opKey: "kbCovFocusMode", path: "f11", covered: true },
  { opKey: "kbCovLaunch1", path: "ctrl+alt+1", covered: true },
  { opKey: "kbCovQuickAudio", path: "ctrl+alt+k", covered: true },
];

// ---------------------------------------------------------------------------
// U-59 触控基础
// ---------------------------------------------------------------------------

export type TouchMode = "auto" | "on" | "off";

/** 长按 = 右键菜单延迟（450ms ± 50，系统手感一致）。 */
export const LONG_PRESS_CONTEXT_MS = 450;

/** 触控模式判定。 */
export function touchModeActive(mode: TouchMode, hasTouchSignal: boolean): boolean {
  if (mode === "on") return true;
  if (mode === "off") return false;
  return hasTouchSignal; // auto：首触即切换
}

// ---------------------------------------------------------------------------
// 总设置模型（settings.inputFeel，JSON 持久化）
// ---------------------------------------------------------------------------

export interface InputFeelSettings {
  /** V-61 鼠标手感面板参数（环境内 + 可同步系统）。 */
  mouse: MouseParams;
  /** V-62 指针方案。 */
  pointerScheme: PointerSchemeId;
  /** V-62 自定义 13 态 → 文件路径（pointerScheme === "custom" 时生效）。 */
  customCursors: Partial<Record<PointerState, string>>;
  /** V-63 指针轨迹（默认关闭）。 */
  trailEnabled: boolean;
  trailLevel: 1 | 2 | 3;
  /** V-64 点击涟漪（默认关闭）。 */
  rippleEnabled: boolean;
  /** V-65 大写锁定提示（默认关闭）。 */
  capsLockHint: boolean;
  /** V-66 按键重映射（默认空表）。 */
  keyRemaps: KeyRemapEntry[];
  /** V-67 触控板自然滚动（默认关闭 = 现状）。 */
  naturalScroll: boolean;
  /** V-68 打字音效（默认关闭）。 */
  typingSound: TypingSoundId;
  /** V-69 指针精确模式（默认关闭）。 */
  precisionEnabled: boolean;
  precisionModifier: PrecisionModifier;
  precisionRatio: number;
  precisionCross: boolean;
  /** V-70 拖拽阈值（默认 4px = 现状手感）。 */
  dragThreshold: number;
  /** U-59 触控模式（默认 auto）。 */
  touchMode: TouchMode;
}

export const DEFAULT_INPUT_FEEL: InputFeelSettings = {
  mouse: { ...DEFAULT_MOUSE_PARAMS },
  pointerScheme: "system",
  customCursors: {},
  trailEnabled: false,
  trailLevel: 1,
  rippleEnabled: false,
  capsLockHint: false,
  keyRemaps: [],
  naturalScroll: false,
  typingSound: "off",
  precisionEnabled: false,
  precisionModifier: "alt",
  precisionRatio: DEFAULT_PRECISION_RATIO,
  precisionCross: true,
  dragThreshold: DEFAULT_DRAG_THRESHOLD,
  touchMode: "auto",
};

/** 严格 coerce：任何损坏字段回退默认（默认即现状）。 */
export function coerceInputFeel(raw: unknown): InputFeelSettings {
  const d = DEFAULT_INPUT_FEEL;
  if (!raw || typeof raw !== "object") return structuredClone(d);
  const r = raw as Partial<InputFeelSettings>;
  const out: InputFeelSettings = {
    ...structuredClone(d),
    mouse: coerceMouseParams(r.mouse),
    customCursors: {},
    keyRemaps: [],
  };
  if (BUILTIN_POINTER_SCHEMES.includes(r.pointerScheme as PointerSchemeId) || r.pointerScheme === "custom") {
    out.pointerScheme = r.pointerScheme as PointerSchemeId;
  }
  if (r.customCursors && typeof r.customCursors === "object") {
    for (const st of POINTER_STATES) {
      const v = (r.customCursors as Record<string, unknown>)[st];
      if (typeof v === "string" && v.trim()) out.customCursors[st] = v.trim();
    }
  }
  if (r.trailEnabled === true) out.trailEnabled = true;
  if (r.trailLevel === 2 || r.trailLevel === 3) out.trailLevel = r.trailLevel;
  if (r.rippleEnabled === true) out.rippleEnabled = true;
  if (r.capsLockHint === true) out.capsLockHint = true;
  if (Array.isArray(r.keyRemaps)) {
    out.keyRemaps = r.keyRemaps.filter(
      (e): e is KeyRemapEntry =>
        !!e && typeof e === "object" && typeof e.from === "string" && typeof e.to === "string",
    );
  }
  if (r.naturalScroll === true) out.naturalScroll = true;
  if (TYPING_SOUNDS.includes(r.typingSound as TypingSoundId)) out.typingSound = r.typingSound as TypingSoundId;
  if (r.precisionEnabled === true) out.precisionEnabled = true;
  if (r.precisionModifier === "alt" || r.precisionModifier === "ctrl" || r.precisionModifier === "shift") {
    out.precisionModifier = r.precisionModifier;
  }
  out.precisionRatio = coercePrecisionRatio(Number(r.precisionRatio) || d.precisionRatio);
  if (r.precisionCross === false) out.precisionCross = false;
  out.dragThreshold = clamp(Math.round(Number(r.dragThreshold) || d.dragThreshold), 2, 10);
  if (r.touchMode === "on" || r.touchMode === "off" || r.touchMode === "auto") out.touchMode = r.touchMode;
  return out;
}

// ---------------------------------------------------------------------------
// 运行时全局快照（供无 props 的旧调用点最小 diff 接入，如拖拽阈值）
// ---------------------------------------------------------------------------

let liveInputFeel: InputFeelSettings = structuredClone(DEFAULT_INPUT_FEEL);

/** 运行时更新快照（InputFeelRuntime 在 settings 变化时调用）。 */
export function setLiveInputFeel(s: InputFeelSettings): void {
  liveInputFeel = s;
}

/** 读取当前生效阈值（DesktopIcons / VirtualWindowFrame 拖拽启动判定用）。 */
export function liveDragThreshold(): number {
  return liveInputFeel.dragThreshold;
}
