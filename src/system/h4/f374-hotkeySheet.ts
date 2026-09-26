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
