/**
 * AURORA-10000 领域04 · 族0093 快捷键中心（AI-19 批次，勿删）。
 * 热键注册表/冲突检测/方案导入与还原/使用统计/宏录制回放（数据模型层）。
 */

export interface HotkeyEntry {
  id: string;
  label: string;
  /** 规范化组合键（如 "ctrl+shift+f"）。 */
  accel: string;
  scope: "global" | "window";
  /** 使用次数（F02316）。 */
  uses: number;
}

/** 冲突检测（F02302）：返回 accel → 条目列表（>1 即冲突）。 */
export function findHotkeyConflicts(entries: readonly HotkeyEntry[]): Map<string, HotkeyEntry[]> {
  const m = new Map<string, HotkeyEntry[]>();
  for (const e of entries) {
    const arr = m.get(e.accel) ?? [];
    arr.push(e);
    m.set(e.accel, arr);
  }
  return new Map([...m].filter(([, arr]) => arr.length > 1));
}

/** 规范化（小写、去空白、排序修饰键）。 */
export function normalizeAccel(accel: string): string {
  const parts = accel.toLowerCase().split("+").map((s) => s.trim()).filter(Boolean);
  const mods = parts.filter((p) => ["ctrl", "alt", "shift", "meta", "win"].includes(p)).sort();
  const keys = parts.filter((p) => !mods.includes(p));
  return [...mods, ...keys].join("+");
}

/** 录制编辑（F02303）：原始按键序列 → 规范 accel。 */
export function recordAccel(seq: readonly string[]): string { return normalizeAccel(seq.join("+")); }

/** 方案切换（F02314）：预置 Vim/Emacs/单手方案（增量替换）。 */
export const HOTKEY_SCHEMES: Readonly<Record<string, readonly HotkeyEntry[]>> = {
  vim: [{ id: "hk-vim-save", label: "保存", accel: "ctrl+w", scope: "window", uses: 0 }],
  emacs: [{ id: "hk-emacs-save", label: "保存", accel: "ctrl+x ctrl+s", scope: "window", uses: 0 }],
  "left-hand": [{ id: "hk-lh-menu", label: "开始菜单", accel: "ctrl+esc", scope: "global", uses: 0 }],
};

/** 应用方案：按 id 去重合并。 */
export function applyScheme(base: readonly HotkeyEntry[], scheme: readonly HotkeyEntry[]): HotkeyEntry[] {
  const ids = new Set(scheme.map((e) => e.id));
  return [...base.filter((e) => !ids.has(e.id)), ...scheme];
}

/** 一键还原（F02315）。 */
export function restoreDefaults(defaults: readonly HotkeyEntry[]): HotkeyEntry[] { return [...defaults]; }

/** 使用统计 + 最常用榜（F02316）。 */
export function bumpUsage(entries: readonly HotkeyEntry[], id: string): HotkeyEntry[] {
  return entries.map((e) => (e.id === id ? { ...e, uses: e.uses + 1 } : e));
}
export function topUsed(entries: readonly HotkeyEntry[], n = 5): HotkeyEntry[] {
  return [...entries].sort((a, b) => b.uses - a.uses).slice(0, n);
}

/** 游戏模式（F02312）：全屏时全局热键降级为窗口域。 */
export function gameModeScope(e: HotkeyEntry, fullscreen: boolean): "global" | "window" {
  return fullscreen && e.scope === "global" ? "window" : e.scope;
}

/* ---------------- 宏（F02323~F02325） ---------------- */

export interface MacroStep { key: string; delayMs: number; }
export interface Macro { id: string; name: string; steps: MacroStep[]; }

/** 录制：把按键+间隔聚成步（首步延迟 0，后续取真实间隔，上限 2s）。 */
export function recordMacro(name: string, events: readonly { key: string; at: number }[]): Macro {
  const steps: MacroStep[] = [];
  let prev = -1;
  for (const ev of events) {
    steps.push({ key: ev.key, delayMs: prev < 0 ? 0 : Math.min(2000, Math.max(0, ev.at - prev)) });
    prev = ev.at;
  }
  return { id: `macro-${Date.now()}`, name, steps };
}

/** 回放：展开为带延迟的时间线。 */
export function playback(m: Macro): Array<{ key: string; at: number }> {
  let t = 0;
  return m.steps.map((s) => { t += s.delayMs; return { key: s.key, at: t }; });
}
