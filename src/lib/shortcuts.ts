﻿import type { Lang } from "../i18n/dictionaries";

/**
 * 全局快捷键表（批次E，规格 4.7）：
 * 与 src-tauri/src/shell/winman.rs default_binds() 保持一致。
 * 自定义只存增量（settings.shortcutBinds），整表 = 默认 + 覆盖。
 */

export interface ShortcutAction {
  id: string;
  /** 词典 key（scAct*） */
  labelKey: string;
  accel: string;
  group: "system" | "panel" | "window" | "launch";
}

export const SHORTCUT_ACTIONS: ShortcutAction[] = [
  { id: "explorer", labelKey: "scActExplorer", accel: "super+e", group: "system" },
  { id: "explorerCtrl", labelKey: "scActExplorerCtrl", accel: "ctrl+e", group: "system" },
  { id: "showDesktop", labelKey: "scActShowDesktop", accel: "super+d", group: "window" },
  { id: "toggleHide", labelKey: "scActToggleHide", accel: "ctrl+shift+d", group: "window" },
  { id: "minimizeAll", labelKey: "scActMinimizeAll", accel: "super+m", group: "window" },
  { id: "snapLeft", labelKey: "scActSnapLeft", accel: "super+left", group: "window" },
  { id: "snapRight", labelKey: "scActSnapRight", accel: "super+right", group: "window" },
  { id: "snapUp", labelKey: "scActSnapUp", accel: "super+up", group: "window" },
  { id: "snapDown", labelKey: "scActSnapDown", accel: "super+down", group: "window" },
  { id: "notifyCenter", labelKey: "scActNotify", accel: "super+n", group: "panel" },
  { id: "quickBluetooth", labelKey: "scActQuickBt", accel: "ctrl+alt+b", group: "panel" },
  { id: "quickAudio", labelKey: "scActQuickAudio", accel: "ctrl+alt+o", group: "panel" },
  { id: "dnd", labelKey: "scActDnd", accel: "ctrl+shift+m", group: "panel" },
  ...Array.from({ length: 9 }, (_, i) => ({
    id: `launch${i + 1}`,
    labelKey: "scActLaunchN",
    accel: `super+${i + 1}`,
    group: "launch" as const,
  })),
];

const MODIFIERS = new Set(["ctrl", "alt", "super", "shift"]);
const KNOWN_KEYS = new Set([
  "e", "n", "m", "d", "1", "2", "3", "4", "5", "6", "7", "8", "9",
  "left", "right", "up", "down", "tab",
  "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
  "home", "end", "pageup", "pagedown", "insert", "delete",
]);

/** accel 归一化：小写、修饰键去重排序（ctrl/alt/shift/super 顺序）、键尾。非法返回 null。 */
export function normalizeAccel(raw: string): string | null {
  const parts = raw
    .trim()
    .toLowerCase()
    .split("+")
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length < 1) return null;
  const key = parts[parts.length - 1] as string;
  const mods = parts.slice(0, -1);
  if (!KNOWN_KEYS.has(key)) return null;
  if (mods.some((m) => !MODIFIERS.has(m))) return null;
  if (new Set(mods).size !== mods.length) return null;
  if (mods.length === 0 && !key.startsWith("f") && key !== "delete") return null; // 裸键只允许 F 键
  const order = ["ctrl", "alt", "shift", "super"].filter((m) => mods.includes(m));
  return [...order, key].join("+");
}

/** accel 显示形式（zh/en）。 */
export function prettyAccel(accel: string, lang: Lang): string {
  const superLabel = lang !== "en" ? "Win" : "Win";
  return accel
    .split("+")
    .map((p) =>
      p === "super" ? superLabel : p === "ctrl" ? "Ctrl" : p === "alt" ? "Alt" : p === "shift" ? "Shift" : p.length === 1 ? p.toUpperCase() : p.charAt(0).toUpperCase() + p.slice(1),
    )
    .join(" + ");
}

/** 整表（默认 + settings 覆盖）→ 发往后端的 binds。 */
export function effectiveBinds(overrides: Record<string, string>): { action: string; accel: string }[] {
  return SHORTCUT_ACTIONS.map((a) => ({
    action: a.id,
    accel: overrides[a.id] ?? a.accel,
  }));
}

/** 冲突检测：同一 accel 绑定到多个 action。返回冲突 accel 集合。 */
export function findConflicts(binds: { action: string; accel: string }[]): Set<string> {
  const seen = new Map<string, number>();
  for (const b of binds) seen.set(b.accel, (seen.get(b.accel) ?? 0) + 1);
  return new Set([...seen.entries()].filter(([, n]) => n > 1).map(([accel]) => accel));
}
