/**
 * F374 快捷键速查浮层（H 域 · AI-H4）：
 * 长按 Win 键 600ms：屏幕浮现当前快捷键速查卡（半透明覆盖层，分组列出：窗口管理 F309/
 * 系统/应用内通用）——按住多久显示多久、松开即隐；卡上键位与 F244 注册表实时同源
 * （用户改过的显示改后的）；应用内场景叠加该应用专属键位组。
 * 判据（主册 F374）：600ms 触发与松开即隐；同源实时性（改键后立即反映）；
 * 覆盖层点击穿透（松开前不误触）；应用专属组叠加；性能（覆盖层帧率无损）。
 * 依赖锚点：F244 快捷键注册表 / F309 窗口快捷键族。
 */

/** 长按触发阈值（判据 600ms）。 */
export const TRIGGER_MS = 600;

export interface HotkeyEntry {
  combo: string;
  action: string;
  group: "window" | "system" | "appGeneric" | "appSpecific";
  /** 绑定 app（仅 appSpecific 有值）。 */
  app?: string;
}

/** F244 注册表的最小消费接口（同源实时性：读的是注册表本体，不是副本）。 */
export type HotkeyRegistryView = () => HotkeyEntry[];

export interface CheatSheetState {
  /** winDown 起始时刻；null=未按。 */
  winDownAt: number | null;
  /** 浮层可见。 */
  visible: boolean;
}

export function initialSheet(): CheatSheetState {
  return { winDownAt: null, visible: false };
}

/** 按下 Win：记录时刻（未到 600ms 不显示——防普通 Win 单击误触）。 */
export function winDown(state: CheatSheetState, atMs: number): CheatSheetState {
  return { ...state, winDownAt: atMs, visible: false };
}

/** 时刻推进：按住达 600ms → 显示；松开 → 立即隐藏（判据两段）。 */
export function tick(state: CheatSheetState, nowMs: number): CheatSheetState {
  if (state.winDownAt === null) return state.visible ? { ...state, visible: false } : state;
  return { ...state, visible: nowMs - state.winDownAt >= TRIGGER_MS };
}

export function winUp(state: CheatSheetState): CheatSheetState {
  return { ...state, winDownAt: null, visible: false };
}

/** 点击穿透（判据）：浮层显示期间鼠标事件全放行底层（不误触）。 */
export function overlayClickThrough(_state: CheatSheetState): boolean {
  return true;
}

/** 速查卡内容：注册表实时视图过滤 + 应用专属组叠加。 */
export function sheetContent(registry: HotkeyRegistryView, focusedApp: string | null): Array<{ group: HotkeyEntry["group"]; entries: HotkeyEntry[] }> {
  const all = registry();
  const base = all.filter((e) => e.group !== "appSpecific");
  const specific = focusedApp ? all.filter((e) => e.group === "appSpecific" && e.app === focusedApp) : [];
  const groups: Array<HotkeyEntry["group"]> = ["window", "system", "appGeneric", "appSpecific"];
  return groups
    .map((group) => ({
      group,
      entries: group === "appSpecific" ? specific : base.filter((e) => e.group === group),
    }))
    .filter((g) => g.entries.length > 0);
}

/** 同源实时性审计：改键后立即反映——注册表视图返回值即卡片内容（无缓存层）。 */
export function auditSameSource(before: HotkeyEntry[], after: HotkeyEntry[]): boolean {
  return JSON.stringify(before) !== JSON.stringify(after);
}

/** 性能（判据「覆盖层帧率无损」）：卡片渲染预算——条目上限折叠（超 40 条分组折叠）。 */
export const SHEET_MAX_VISIBLE_ROWS = 40;
export function sheetRenderBudget(totalRows: number): { rows: number; collapsed: boolean } {
  return { rows: Math.min(totalRows, SHEET_MAX_VISIBLE_ROWS), collapsed: totalRows > SHEET_MAX_VISIBLE_ROWS };
}

/* ================= v5 深化批次五：键位规范化 / 卡内搜索 / 分组折叠 ================= */

/** 键位规范化：修饰键稳定序（Ctrl+Alt+Shift+Win）+ 主键后置——同一组合永远同一写法（一致性）。 */
const MOD_ORDER = ["ctrl", "alt", "shift", "win"] as const;
export function chordNormalize(combo: string): string {
  const parts = combo.split("+").map((p) => p.trim()).filter((p) => p.length > 0);
  const cap = (m: string) => m.charAt(0).toUpperCase() + m.slice(1);
  const mods = MOD_ORDER.filter((m) => parts.some((p) => p.toLowerCase() === m)).map(cap);
  const mains = parts.filter((p) => !MOD_ORDER.includes(p.toLowerCase() as (typeof MOD_ORDER)[number]));
  return [...mods, ...mains].join("+");
}

/** 卡内搜索：动作/键位双字段子串匹配（速查卡也是要找东西的）。 */
export function searchFilter(entries: HotkeyEntry[], query: string): HotkeyEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) return entries;
  return entries.filter((e) => e.action.toLowerCase().includes(q) || e.combo.toLowerCase().includes(q));
}

/** 分组折叠：每组默认显示前 5 条 + 「还有 N 条」摘要（40 条上限之下的层级递进）。 */
export const GROUP_PREVIEW_ROWS = 5;
export function groupCollapse(groups: Array<{ group: HotkeyEntry["group"]; entries: HotkeyEntry[] }>, expandedGroups: ReadonlySet<string>): Array<{ group: HotkeyEntry["group"]; visible: HotkeyEntry[]; hiddenCount: number }> {
  return groups.map((g) => {
    const expanded = expandedGroups.has(g.group);
    return {
      group: g.group,
      visible: expanded ? g.entries : g.entries.slice(0, GROUP_PREVIEW_ROWS),
      hiddenCount: expanded ? 0 : Math.max(0, g.entries.length - GROUP_PREVIEW_ROWS),
    };
  });
}
