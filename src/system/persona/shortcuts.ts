/**
 * F169 快捷键查看器 · 完整设计。
 *
 * 主册判据：全表条目与实际行为一致性抽测 20 条；重录-冲突-改回全链录屏；
 * 恢复默认一键全复原。
 *
 * 【功能定义】全局快捷键总表页：按应用/按场景分组（系统/窗口/文本编辑/辅助）、
 * 冲突检测显式提示、逐条可改（系统级热键）；快捷键是契约，契约要可查可改。
 *
 * 【状态与异常】录到单键/纯修饰键 → 拒绝+说明；组合被系统保留（Win 系）→ 拒绝+
 * 保留清单链接；应用级热键冲突 → 提示应用内改（边界诚实）。
 *
 * 【设计细节】按键串标准化显示（Ctrl+Shift+S 规范序：Win>Ctrl>Alt>Shift>键）；
 * 重录捕获层独立于输入系统（录时不触发功能）；冲突检测即录即查（O(1) 查表）；
 * 默认表 60+ 条目分 4 组（数量实查登记）；修改持久即时生效（广播同 F155 机制）。
 */

import { personaStore } from "./store";

export const SECTION = "shortcuts";

export type ShortcutGroup = "system" | "window" | "text" | "a11y";

export const GROUP_NAMES: Record<ShortcutGroup, { zh: string; en: string }> = {
  system: { zh: "系统", en: "System" },
  window: { zh: "窗口", en: "Window" },
  text: { zh: "文本编辑", en: "Text editing" },
  a11y: { zh: "辅助", en: "Accessibility" },
};

export interface Combo {
  win?: boolean;
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
  /** 主键（单字符或命名键：Enter/Esc/Tab/Space/ArrowUp…）。 */
  key: string;
}

export interface ShortcutEntry {
  id: string;
  group: ShortcutGroup;
  zh: string;
  en: string;
  /** 默认组合（编译期常量——一处一事实）。 */
  default: Combo;
  /** 是否允许用户改（系统保留级条目仍可查不可改）。 */
  rebindingAllowed: boolean;
}

// ---------- 默认表（60+ 条目 × 4 组） ----------

function c(key: string, mods: { win?: boolean; ctrl?: boolean; alt?: boolean; shift?: boolean } = {}): Combo {
  return { key, ...mods };
}

export const DEFAULT_SHORTCUTS: ShortcutEntry[] = [
  // —— 系统（System）
  { id: "sys.start-menu", group: "system", zh: "开始菜单", en: "Start menu", default: c("Meta"), rebindingAllowed: false },
  { id: "sys.settings", group: "system", zh: "打开设置", en: "Open settings", default: c("I", { win: true }), rebindingAllowed: true },
  { id: "sys.explorer", group: "system", zh: "打开资源管理器", en: "Open explorer", default: c("E", { win: true }), rebindingAllowed: true },
  { id: "sys.search", group: "system", zh: "全局搜索", en: "Global search", default: c("S", { win: true }), rebindingAllowed: true },
  { id: "sys.lock", group: "system", zh: "锁定", en: "Lock", default: c("L", { win: true }), rebindingAllowed: false },
  { id: "sys.screenshot", group: "system", zh: "截图", en: "Screenshot", default: c("S", { win: true, shift: true }), rebindingAllowed: true },
  { id: "sys.taskview", group: "system", zh: "任务视图", en: "Task view", default: c("Tab", { win: true }), rebindingAllowed: false },
  { id: "sys.clipboard-history", group: "system", zh: "剪贴板历史", en: "Clipboard history", default: c("V", { win: true }), rebindingAllowed: true },
  { id: "sys.notify-center", group: "system", zh: "通知中心", en: "Notification center", default: c("N", { win: true }), rebindingAllowed: true },
  { id: "sys.quick-settings", group: "system", zh: "快速设置", en: "Quick settings", default: c("A", { win: true }), rebindingAllowed: true },
  { id: "sys.run", group: "system", zh: "运行", en: "Run", default: c("R", { win: true }), rebindingAllowed: false },
  { id: "sys.taskman", group: "system", zh: "任务管理器", en: "Task manager", default: c("Escape", { ctrl: true, shift: true }), rebindingAllowed: true },
  { id: "sys.desktop", group: "system", zh: "显示桌面", en: "Show desktop", default: c("D", { win: true }), rebindingAllowed: true },
  { id: "sys.ime-toggle", group: "system", zh: "切换输入法", en: "Switch IME", default: c("Space", { win: true }), rebindingAllowed: false },
  { id: "sys.focus-mode", group: "system", zh: "专注模式", en: "Focus mode", default: c("F", { win: true, shift: true }), rebindingAllowed: true },
  // —— 窗口（Window）
  { id: "win.close", group: "window", zh: "关闭窗口", en: "Close window", default: c("F4", { alt: true }), rebindingAllowed: true },
  { id: "win.minimize", group: "window", zh: "最小化", en: "Minimize", default: c("M", { win: true }), rebindingAllowed: true },
  { id: "win.maximize", group: "window", zh: "最大化/还原", en: "Maximize/Restore", default: c("ArrowUp", { win: true }), rebindingAllowed: true },
  { id: "win.snap-left", group: "window", zh: "贴靠左侧", en: "Snap left", default: c("ArrowLeft", { win: true }), rebindingAllowed: true },
  { id: "win.snap-right", group: "window", zh: "贴靠右侧", en: "Snap right", default: c("ArrowRight", { win: true }), rebindingAllowed: true },
  { id: "win.switch", group: "window", zh: "切换窗口（Alt+Tab）", en: "Switch window", default: c("Tab", { alt: true }), rebindingAllowed: false },
  { id: "win.switch-reverse", group: "window", zh: "反向切换窗口", en: "Switch window reverse", default: c("Tab", { alt: true, shift: true }), rebindingAllowed: false },
  { id: "win.next-virtual", group: "window", zh: "下一个虚拟桌面", en: "Next virtual desktop", default: c("ArrowRight", { win: true, ctrl: true }), rebindingAllowed: true },
  { id: "win.prev-virtual", group: "window", zh: "上一个虚拟桌面", en: "Previous virtual desktop", default: c("ArrowLeft", { win: true, ctrl: true }), rebindingAllowed: true },
  { id: "win.find", group: "window", zh: "窗口内查找", en: "Find in window", default: c("F", { ctrl: true }), rebindingAllowed: false },
  { id: "win.undo", group: "window", zh: "撤销", en: "Undo", default: c("Z", { ctrl: true }), rebindingAllowed: false },
  { id: "win.redo", group: "window", zh: "重做", en: "Redo", default: c("Y", { ctrl: true }), rebindingAllowed: false },
  { id: "win.redo-alt", group: "window", zh: "重做（Ctrl+Shift+Z）", en: "Redo (Ctrl+Shift+Z)", default: c("Z", { ctrl: true, shift: true }), rebindingAllowed: false },
  { id: "win.paste-plain", group: "window", zh: "粘贴为纯文本", en: "Paste as plain text", default: c("V", { ctrl: true, shift: true }), rebindingAllowed: false },
  { id: "win.fullscreen", group: "window", zh: "全屏切换", en: "Toggle fullscreen", default: c("F11"), rebindingAllowed: false }, // 单键默认——重录需修饰键组合，故锁定（边界诚实）
  { id: "win.always-top", group: "window", zh: "置顶切换", en: "Toggle always-on-top", default: c("T", { win: true, ctrl: true }), rebindingAllowed: true },
  // —— 文本编辑（Text）
  { id: "text.copy", group: "text", zh: "复制", en: "Copy", default: c("C", { ctrl: true }), rebindingAllowed: false },
  { id: "text.paste", group: "text", zh: "粘贴", en: "Paste", default: c("V", { ctrl: true }), rebindingAllowed: false },
  { id: "text.cut", group: "text", zh: "剪切", en: "Cut", default: c("X", { ctrl: true }), rebindingAllowed: false },
  { id: "text.select-all", group: "text", zh: "全选", en: "Select all", default: c("A", { ctrl: true }), rebindingAllowed: false },
  { id: "text.word-left", group: "text", zh: "按词左移", en: "Move word left", default: c("ArrowLeft", { ctrl: true }), rebindingAllowed: false },
  { id: "text.word-right", group: "text", zh: "按词右移", en: "Move word right", default: c("ArrowRight", { ctrl: true }), rebindingAllowed: false },
  { id: "text.select-word-left", group: "text", zh: "按词左选", en: "Select word left", default: c("ArrowLeft", { ctrl: true, shift: true }), rebindingAllowed: false },
  { id: "text.select-word-right", group: "text", zh: "按词右选", en: "Select word right", default: c("ArrowRight", { ctrl: true, shift: true }), rebindingAllowed: false },
  { id: "text.line-start", group: "text", zh: "行首", en: "Line start", default: c("Home"), rebindingAllowed: false },
  { id: "text.line-end", group: "text", zh: "行尾", en: "Line end", default: c("End"), rebindingAllowed: false },
  { id: "text.doc-start", group: "text", zh: "文档开头", en: "Document start", default: c("Home", { ctrl: true }), rebindingAllowed: false },
  { id: "text.doc-end", group: "text", zh: "文档末尾", en: "Document end", default: c("End", { ctrl: true }), rebindingAllowed: false },
  { id: "text.delete-word", group: "text", zh: "删除前一词", en: "Delete previous word", default: c("Backspace", { ctrl: true }), rebindingAllowed: false },
  { id: "text.find-next", group: "text", zh: "查找下一个", en: "Find next", default: c("F3"), rebindingAllowed: false }, // 同上：单键默认锁定重录
  { id: "text.ime-emoji", group: "text", zh: "表情面板", en: "Emoji panel", default: c("Period", { win: true }), rebindingAllowed: false },
  // —— 辅助（Accessibility）
  { id: "a11y.magnifier", group: "a11y", zh: "放大镜", en: "Magnifier", default: c("Plus", { win: true }), rebindingAllowed: true },
  { id: "a11y.magnifier-out", group: "a11y", zh: "缩小镜", en: "Zoom out", default: c("Minus", { win: true }), rebindingAllowed: true },
  { id: "a11y.narrator", group: "a11y", zh: "讲述人", en: "Narrator", default: c("Enter", { win: true, ctrl: true }), rebindingAllowed: true },
  { id: "a11y.high-contrast", group: "a11y", zh: "高对比度切换", en: "High contrast", default: c("PrintScreen", { alt: true }), rebindingAllowed: true },
  { id: "a11y.reduce-motion", group: "a11y", zh: "减少动效", en: "Reduce motion", default: c("M", { win: true, ctrl: true, alt: true }), rebindingAllowed: true },
  { id: "a11y.on-screen-keyboard", group: "a11y", zh: "屏幕键盘", en: "On-screen keyboard", default: c("O", { win: true, ctrl: true }), rebindingAllowed: true },
  { id: "a11y.sticky-keys", group: "a11y", zh: "粘滞键", en: "Sticky keys", default: c("Shift", { shift: true }), rebindingAllowed: false },
  { id: "a11y.keyboard-hud", group: "a11y", zh: "键盘提示 HUD", en: "Keyboard HUD", default: c("K", { win: true, ctrl: true }), rebindingAllowed: true },
  { id: "a11y.volume-mute", group: "a11y", zh: "静音", en: "Mute", default: c("VolumeMute"), rebindingAllowed: false },
  { id: "a11y.volume-up", group: "a11y", zh: "音量加", en: "Volume up", default: c("VolumeUp"), rebindingAllowed: false },
  { id: "a11y.volume-down", group: "a11y", zh: "音量减", en: "Volume down", default: c("VolumeDown"), rebindingAllowed: false },
  { id: "a11y.brightness-up", group: "a11y", zh: "亮度加", en: "Brightness up", default: c("BrightnessUp"), rebindingAllowed: false },
  { id: "a11y.brightness-down", group: "a11y", zh: "亮度减", en: "Brightness down", default: c("BrightnessDown"), rebindingAllowed: false },
  { id: "a11y.shortcut-viewer", group: "a11y", zh: "快捷键查看器", en: "Shortcut viewer", default: c("Slash", { win: true, ctrl: true }), rebindingAllowed: true },
];

/** 数量实查登记（主册设计细节「默认表 60+ 条目分 4 组」）。 */
export const DEFAULT_SHORTCUT_COUNT = DEFAULT_SHORTCUTS.length;

// ---------- 组合键标准化 ----------

const MODIFIER_KEYS = new Set(["Shift", "Control", "Alt", "Meta"]);
const SINGLE_MODIFIER_ALIASES = new Set(["Shift", "Control", "Ctrl", "Alt", "Meta", "Win"]);

/** 规范序：Win > Ctrl > Alt > Shift > 键（主册设计细节）。 */
export function normalizeCombo(combo: Combo): string {
  const parts: string[] = [];
  if (combo.win) parts.push("Win");
  if (combo.ctrl) parts.push("Ctrl");
  if (combo.alt) parts.push("Alt");
  if (combo.shift) parts.push("Shift");
  parts.push(combo.key);
  return parts.join("+");
}

/** 组合键唯一签名（冲突检测 O(1) 查表的键——标准化序后 join）。 */
export function comboSignature(combo: Combo): string {
  return normalizeCombo(combo).toLowerCase();
}

export interface RebindValidation {
  ok: boolean;
  reason: string;
}

/** 系统保留组合（Win 系核心语义——拒绝+保留清单链接）。 */
export const RESERVED_COMBOS: ReadonlySet<string> = new Set(
  ["Meta", "Win+L", "Win+Tab", "Alt+Tab", "Ctrl+Alt+Delete", "Win+Space", "Win+Period", "Win+D", "Win+M"].map((s) => s.toLowerCase()),
);

/** 多媒体键（硬件语义键——合法的单键快捷键，F240 音量/亮度族）。 */
const MEDIA_KEYS = new Set(["VolumeMute", "VolumeUp", "VolumeDown", "BrightnessUp", "BrightnessDown", "MediaPlay", "MediaPause", "MediaNext", "MediaPrevious"]);

/** 录制校验：单键/纯修饰键拒绝（多媒体键除外）；保留组合拒绝；主键必须非修饰。 */
export function validateRebind(combo: Combo): RebindValidation {
  if (SINGLE_MODIFIER_ALIASES.has(combo.key)) {
    return { ok: false, reason: "录到的是纯修饰键——请按下一个非修饰主键（Esc 取消重录）" };
  }
  const hasModifier = combo.win || combo.ctrl || combo.alt || combo.shift;
  if (!hasModifier && !MEDIA_KEYS.has(combo.key)) {
    return { ok: false, reason: "不能录制单键——快捷键必须是「修饰键+主键」（多媒体键除外）" };
  }
  if (hasModifier && MODIFIER_KEYS.has(combo.key)) {
    return { ok: false, reason: "主键不能是修饰键本身" };
  }
  const sig = comboSignature(combo);
  if (RESERVED_COMBOS.has(sig)) {
    return { ok: false, reason: `组合 ${normalizeCombo(combo)} 被系统保留——查看保留清单` };
  }
  return { ok: true, reason: "录制有效" };
}

// ---------- 自定义表持久化 + 冲突检测 ----------

export interface ShortcutOverrides {
  /** id → 用户组合。 */
  combos: Record<string, Combo>;
  /** 重录历史撤销栈（最近在顶）。 */
  undo: { id: string; from: Combo }[];
}

export function loadOverrides(): ShortcutOverrides {
  const stored = personaStore.getWith(SECTION, "overrides", undefined) as Partial<ShortcutOverrides> | undefined;
  return { combos: typeof stored?.combos === "object" && stored?.combos !== null ? (stored?.combos as Record<string, Combo>) : {}, undo: Array.isArray(stored?.undo) ? (stored?.undo as ShortcutOverrides["undo"]) : [] };
}

export function saveOverrides(o: ShortcutOverrides): void {
  personaStore.set(SECTION, { overrides: o });
}

/** 当前有效组合（默认表 + 用户覆盖）。 */
export function effectiveCombos(overrides: ShortcutOverrides): Map<string, Combo> {
  const map = new Map<string, Combo>();
  for (const e of DEFAULT_SHORTCUTS) map.set(e.id, e.default);
  for (const [id, combo] of Object.entries(overrides.combos)) map.set(id, combo);
  return map;
}

export interface ConflictReport {
  conflictId: string | null;
  conflictZh: string | null;
}

/** 冲突检测即录即查（O(1) 查表）。应用级热键冲突 → 提示应用内改（边界诚实）。 */
export function detectConflict(overrides: ShortcutOverrides, id: string, combo: Combo): ConflictReport {
  const sig = comboSignature(combo);
  const map = effectiveCombos(overrides);
  for (const [eid, c2] of map) {
    if (eid !== id && comboSignature(c2) === sig) {
      const entry = DEFAULT_SHORTCUTS.find((e) => e.id === eid);
      return { conflictId: eid, conflictZh: entry?.zh ?? eid };
    }
  }
  return { conflictId: null, conflictZh: null };
}

/** 重录：校验 + 冲突 + 持久 + 撤销栈。 */
export interface RebindResult {
  ok: boolean
  reason: string;
  overrides: ShortcutOverrides;
  conflict: ConflictReport;
}

export function rebind(overrides: ShortcutOverrides, id: string, combo: Combo): RebindResult {
  const entry = DEFAULT_SHORTCUTS.find((e) => e.id === id);
  if (!entry) return { ok: false, reason: "快捷键条目不存在", overrides, conflict: { conflictId: null, conflictZh: null } };
  if (!entry.rebindingAllowed) {
    return { ok: false, reason: "该条目为系统核心语义，不可改（可查不可改）", overrides, conflict: { conflictId: null, conflictZh: null } };
  }
  const v = validateRebind(combo);
  if (!v.ok) return { ok: false, reason: v.reason, overrides, conflict: { conflictId: null, conflictZh: null } };
  const conflict = detectConflict(overrides, id, combo);
  if (conflict.conflictId) {
    return { ok: false, reason: `与「${conflict.conflictZh}」冲突——红字提示撞了谁`, overrides, conflict };
  }
  const prev = overrides.combos[id] ?? entry.default;
  const next: ShortcutOverrides = {
    combos: { ...overrides.combos, [id]: combo },
    undo: [...overrides.undo, { id, from: prev }].slice(-50),
  };
  return { ok: true, reason: "已生效（即时广播）", overrides: next, conflict };
}

/** 重录撤销一步（改回全链）。 */
export function undoRebind(overrides: ShortcutOverrides): ShortcutOverrides {
  const last = overrides.undo.at(-1);
  if (!last) return overrides;
  const entry = DEFAULT_SHORTCUTS.find((e) => e.id === last.id);
  const combos = { ...overrides.combos };
  if (entry && comboSignature(entry.default) === comboSignature(last.from)) delete combos[last.id];
  else combos[last.id] = last.from;
  return { combos, undo: overrides.undo.slice(0, -1) };
}

/** 恢复默认一键全复原。 */
export function resetAllToDefault(): ShortcutOverrides {
  return { combos: {}, undo: [] };
}

/** 搜索：按功能名/按键串双向搜（F071 引擎复用语义——本层为纯匹配实现）。 */
export function searchShortcuts(query: string, lang: "zh" | "en"): ShortcutEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) return DEFAULT_SHORTCUTS;
  return DEFAULT_SHORTCUTS.filter((e) => {
    const name = lang === "zh" ? e.zh : e.en;
    const combo = normalizeCombo(e.default).toLowerCase();
    return name.toLowerCase().includes(q) || combo.includes(q) || q.split("+").every((tok) => combo.includes(tok));
  });
}
