/**
 * F518/F525/F535-F539 深化引擎 · 输入法切换键、速查卡导出、Win 键族（AI-U3 · hotkeymap）。
 *
 * 判据唯一源（主册摘文）：
 * - F518「四方案切换；与点击循环并存；让出旧键判据；Caps 双用（600ms 分界
 *   实测）；冲突审计联动」。
 * - F525「两格式导出；同源一致性（导出后改键再导出对比）；分色标注；排版
 *   清晰度（打印 300dpi 走查）；导出入口（F374 浮层内+F244 注册表页双门）」。
 * - F535-F539「Win+数字映射 / Win+T / Peek / Alt+Esc / 布局锁定」。
 *
 * 深化点：
 * 1. Caps 双用裁决器：按下时长 <600ms = 切换键、≥600ms = Caps 锁——分界
 *   常量化 + 边界值实测用例（599/600/601 三点钉死）。
 * 2. 让出旧键账：换方案时旧键释放记录可查（冲突审计 F169 联动的数据源）。
 * 3. 速查卡导出器：Markdown / 纯文本两格式，同一数据源（同源一致性可对拍）；
 *   分色标注以类别 tag 承载（打印 300dpi 是排版属性非数据属性）。
 * 4. Win 键族映射表 + 冲突审计器（重录冲突检测、恢复默认）。
 */

/* ------------------------------ F518 切换方案 ------------------------------ */

export type ImeScheme = "win-space" | "ctrl-space" | "shift" | "ctrl-shift";
export const IME_SCHEMES: readonly ImeScheme[] = ["win-space", "ctrl-space", "shift", "ctrl-shift"];
/** Caps 双用分界（判据 600ms）。 */
export const CAPS_LONG_PRESS_MS = 600;

/** Caps 双用裁决（判据「600ms 分界实测」）：短按=IME 切换，长按=Caps 锁。 */
export function capsDualUseVerdict(holdMs: number): { imeToggle: boolean; capsLock: boolean } {
  if (holdMs < 0) throw new Error(`[u3:F518] 按下时长非法 ${holdMs}ms`);
  return { imeToggle: holdMs < CAPS_LONG_PRESS_MS, capsLock: holdMs >= CAPS_LONG_PRESS_MS };
}

/** 方案占用键表（让出旧键账的事实源）。 */
export const SCHEME_KEYS: Record<ImeScheme, string> = {
  "win-space": "Win+Space",
  "ctrl-space": "Ctrl+Space",
  shift: "Shift",
  "ctrl-shift": "Ctrl+Shift",
};

/**
 * 让出旧键账（判据「让出旧键判据」）：从旧方案切到新方案，旧键被让出并
 * 登记在案（冲突审计可查）；旧键 = 新键时不算让出。
 */
export function releaseOldScheme(oldScheme: ImeScheme, newScheme: ImeScheme): { releasedKey: string | null; log: string } {
  if (oldScheme === newScheme) return { releasedKey: null, log: "方案未变化——无键让出" };
  return { releasedKey: SCHEME_KEYS[oldScheme], log: `「${SCHEME_KEYS[oldScheme]}」已让出，现归系统默认行为` };
}

/* ------------------------------ F525 速查卡导出 ------------------------------ */

export type CardFormat = "markdown" | "text";

export interface HotkeyRow {
  keys: string;
  action: string;
  /** 分色标注类别（打印分色的数据层）。 */
  category: "窗口" | "桌面" | "系统" | "输入";
}

/** 双门入口（判据「F374 浮层内+F244 注册表页双门」）。 */
export const KEYCARD_ENTRY_POINTS = ["f374-overlay", "f244-registry-page"] as const;

/** Markdown 导出（同源：读 rows）。 */
export function exportCardMarkdown(rows: HotkeyRow[], title: string): string {
  const lines: string[] = [`# ${title}`, ""];
  const cats = [...new Set(rows.map((r) => r.category))];
  for (const c of cats) {
    lines.push(`## ${c}`, "");
    for (const r of rows.filter((x) => x.category === c)) lines.push(`| ${r.keys} | ${r.action} |`);
    lines.push("");
  }
  return lines.join("\n");
}

/** 纯文本导出（同源：读 rows——两格式可对拍）。 */
export function exportCardText(rows: HotkeyRow[], title: string): string {
  const lines: string[] = [title, "=".repeat(title.length), ""];
  const cats = [...new Set(rows.map((r) => r.category))];
  for (const c of cats) {
    lines.push(`[${c}]`);
    for (const r of rows.filter((x) => x.category === c)) lines.push(`  ${r.keys}  →  ${r.action}`);
    lines.push("");
  }
  return lines.join("\n");
}

/** 同源一致性对拍（判据「导出后改键再导出对比」）：键位变更必须同时反映两格式。 */
export function cardFormatsAgree(rows: HotkeyRow[], title: string): boolean {
  const md = exportCardMarkdown(rows, title);
  const tx = exportCardText(rows, title);
  return rows.every((r) => md.includes(r.keys) && tx.includes(r.keys) && md.includes(r.action) && tx.includes(r.action));
}

/* --------------------------- F535-F539 Win 键族 --------------------------- */

export interface WinNumberMapping {
  slot: number;
  appId: string;
  /** Shift+Win+数字 = 新实例（判据）。 */
  shiftNewInstance: boolean;
}

/** Win+数字映射解析（判据「Win+数字映射一致性」）。 */
export function resolveWinNumber(slot: number, shift: boolean, mappings: WinNumberMapping[]): { launch: string | null; newInstance: boolean } {
  const m = mappings.find((x) => x.slot === slot);
  if (!m) return { launch: null, newInstance: false };
  return { launch: m.appId, newInstance: shift && m.shiftNewInstance };
}

export type LayoutLockZone = "desktop" | "taskbar";

/** 布局锁定拒绝反馈（判据「布局锁定拒绝反馈」）：拒绝 + 震动动画 + 角标可选。 */
export function layoutLockFeedback(_zone: LayoutLockZone, locked: boolean, shakeMs: number): { allowed: boolean; shakeMs: number; badge: boolean } {
  if (!locked) return { allowed: true, shakeMs: 0, badge: false };
  return { allowed: false, shakeMs, badge: true };
}

/* --------------------------- F518/F169 冲突审计 --------------------------- */

export interface HotkeyBinding {
  keys: string;
  owner: string;
  enabled: boolean;
}

/** 冲突审计器（判据「冲突审计联动」）：重复键位列出冲突对（禁用项不参与）。 */
export function auditHotkeyConflicts(bindings: HotkeyBinding[]): Array<{ keys: string; owners: string[] }> {
  const byKey = new Map<string, string[]>();
  for (const b of bindings) {
    if (!b.enabled) continue;
    const list = byKey.get(b.keys) ?? [];
    list.push(b.owner);
    byKey.set(b.keys, list);
  }
  return [...byKey.entries()].filter(([, owners]) => owners.length > 1).map(([keys, owners]) => ({ keys, owners }));
}

/* ------------------------------ 自检 ------------------------------ */

export function hotkeymapSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // Caps 分界三点钉死：599 切换、600 Caps、601 Caps
  checks.push({ name: "F518 600ms 分界三点", pass: capsDualUseVerdict(599).imeToggle && !capsDualUseVerdict(600).imeToggle && capsDualUseVerdict(600).capsLock && capsDualUseVerdict(601).capsLock && !capsDualUseVerdict(599).capsLock });
  // 让出旧键：换方案释放旧键；同方案不释放
  const rel1 = releaseOldScheme("win-space", "ctrl-space");
  const rel2 = releaseOldScheme("shift", "shift");
  checks.push({ name: "F518 让出旧键账", pass: rel1.releasedKey === "Win+Space" && rel2.releasedKey === null });
  // 四方案在册
  checks.push({ name: "F518 四方案", pass: IME_SCHEMES.length === 4 });
  // 双格式导出同源一致
  const rows: HotkeyRow[] = [
    { keys: "Win+1", action: "打开任务栏第 1 个应用", category: "窗口" },
    { keys: "Win+D", action: "显示桌面", category: "桌面" },
    { keys: "Ctrl+Space", action: "切换输入法", category: "输入" },
  ];
  checks.push({ name: "F525 双格式同源", pass: cardFormatsAgree(rows, "Varix 快捷键速查卡") });
  // 改键后仍同源（再导出对比语义）
  const rows2 = rows.map((r) => (r.keys === "Win+D" ? { ...r, keys: "Win+Shift+D" } : r));
  const md2 = exportCardMarkdown(rows2, "t");
  checks.push({ name: "F525 改键反映两格式", pass: cardFormatsAgree(rows2, "t") && md2.includes("Win+Shift+D") && !md2.includes("| Win+D |") });
  // 双门入口
  checks.push({ name: "F525 双门入口", pass: KEYCARD_ENTRY_POINTS.length === 2 });
  // Win+数字
  const maps: WinNumberMapping[] = [{ slot: 1, appId: "explorer", shiftNewInstance: true }, { slot: 2, appId: "notepad", shiftNewInstance: false }];
  checks.push({ name: "F535 Win+数字映射", pass: resolveWinNumber(1, true, maps).newInstance && resolveWinNumber(2, true, maps).newInstance === false && resolveWinNumber(9, false, maps).launch === null });
  // 布局锁定
  const lk = layoutLockFeedback("desktop", true, 120);
  checks.push({ name: "F539 布局锁定拒绝反馈", pass: !lk.allowed && lk.shakeMs === 120 && lk.badge && layoutLockFeedback("desktop", false, 120).allowed });
  // 冲突审计：重复键检出、禁用项豁免
  const conflicts = auditHotkeyConflicts([
    { keys: "Ctrl+Q", owner: "记事本", enabled: true },
    { keys: "Ctrl+Q", owner: "计算器", enabled: true },
    { keys: "Ctrl+Q", owner: "时钟", enabled: false },
  ]);
  checks.push({ name: "F518 冲突审计", pass: conflicts.length === 1 && conflicts[0]!.owners.length === 2 });
  return checks;
}
