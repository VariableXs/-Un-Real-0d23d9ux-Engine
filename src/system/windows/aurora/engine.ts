import { AURORA_WINDOW_SPACE_FAMILIES, type AuroraFamily } from "./catalog";
import { VISUAL_FAMS, applyVisual } from "./visual";

/**
 * AURORA-10000 · 领域02 总控（AI-06~AI-10 · W1）。
 * 选择存储：族号 → 条目 ID，localStorage 持久化；缺失时回退族内首项。
 * 视觉族（28/46/47/48/49/50）产出 CSS 变量；其余族选中态以
 * `--aurora-ws-f<族号>` 变量登记，供对应运行时模块读取。
 */

const STORE_KEY = "aurora.ws.selections.v1";

/** 读取已保存选择（隐私模式返回空表）。 */
export function loadSelections(): Record<number, string> {
  try {
    const raw = localStorage.getItem(STORE_KEY);
    if (!raw) return {};
    const obj = JSON.parse(raw) as Record<string, string>;
    const out: Record<number, string> = {};
    for (const [k, v] of Object.entries(obj)) {
      const fam = Number(k);
      if (Number.isInteger(fam) && typeof v === "string") out[fam] = v;
    }
    return out;
  } catch {
    return {};
  }
}

/** 保存选择（静默失败 = 隐私模式）。 */
export function saveSelections(sel: Record<number, string>): void {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(sel));
  } catch {
    /* 隐私模式：静默 */
  }
}

/** 族内合法条目 ID；非法回退 null。 */
export function validItem(fam: AuroraFamily, itemId: string): string | null {
  return fam.items.some((i) => i.id === itemId) ? itemId : null;
}

/** 族默认选择：首项。 */
export function defaultSelection(fam: AuroraFamily): string {
  return fam.items[0]!.id;
}

/** 合成最终选择表：保存值优先（非法回退默认）。 */
export function effectiveSelections(saved?: Record<number, string>): Record<number, string> {
  const sel: Record<number, string> = {};
  for (const fam of AURORA_WINDOW_SPACE_FAMILIES) {
    const want = saved?.[fam.fam];
    sel[fam.fam] = (want && validItem(fam, want)) || defaultSelection(fam);
  }
  return sel;
}

/** 应用一族选择：返回 CSS 变量映射（全部族都有登记变量，视觉族额外有真实参数）。 */
export function applyFamilySelection(fam: number, itemId: string): Record<string, string> {
  const vars = applyVisual(fam, itemId);
  vars[`--aurora-ws-f${fam}`] = itemId;
  return vars;
}

/** 把变量表写到元素上。 */
export function setCssVars(el: HTMLElement, vars: Record<string, string>): void {
  for (const [k, v] of Object.entries(vars)) el.style.setProperty(k, v);
}

/** 全量应用：按选择表把 25 族全部应用到 documentElement。 */
export function applyAllSelections(sel: Record<number, string>, root: HTMLElement): void {
  for (const fam of AURORA_WINDOW_SPACE_FAMILIES) {
    const itemId = sel[fam.fam] ?? defaultSelection(fam);
    setCssVars(root, applyFamilySelection(fam.fam, itemId));
  }
  root.dataset.auroraWs = "1";
}

/** 视觉族号（透出给调用方判断）。 */
export { VISUAL_FAMS };
