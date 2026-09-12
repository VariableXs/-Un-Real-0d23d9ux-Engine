/**
 * AURORA-10000 领域04 · 族0081 开始菜单结构 + 族0085 开始菜单行为（AI-17 批次，勿删）。
 * 结构模式/入口区/字母拼音索引/行为决策，纯模型。
 */
import { getD4 } from "./prefs";

export type StartLayout = "single" | "double" | "tiles3" | "fullscreen" | "compact" | "palette";

export interface StartEntry {
  id: string;
  kind: "avatar" | "power" | "settings" | "documents" | "downloads" | "pictures" | "recent";
}

/** 固定入口区（F02017~F02022）。 */
export const START_ENTRIES: readonly StartEntry[] = [
  { id: "avatar", kind: "avatar" },
  { id: "power", kind: "power" },
  { id: "settings", kind: "settings" },
  { id: "documents", kind: "documents" },
  { id: "downloads", kind: "downloads" },
  { id: "pictures", kind: "pictures" },
  { id: "recent", kind: "recent" },
];

/** 当前结构（F02001~F02008）。 */
export function startLayout(): StartLayout {
  return getD4<StartLayout>("F02001") ?? "double";
}

/** 索引条（F02013 字母 / F02014 拼音首字母）：返回可用索引键。 */
export function indexKeys(items: readonly { label: string; pinyin?: string }[]): string[] {
  const alpha = getD4<boolean>("F02013") ?? true;
  const py = getD4<boolean>("F02014") ?? true;
  const keys = new Set<string>();
  for (const it of items) {
    const c = it.label.trim().charAt(0).toUpperCase();
    if (alpha && /[A-Z]/.test(c)) keys.add(c);
    if (py && it.pinyin) keys.add(it.pinyin.charAt(0).toUpperCase());
  }
  return [...keys].sort();
}

/** 分页（F02024）：按每页容量切页并返回页数。 */
export function paginate<T>(items: readonly T[], perPage: number): T[][] {
  const pages: T[][] = [];
  for (let i = 0; i < items.length; i += perPage) pages.push(items.slice(i, i + perPage));
  return pages;
}

/** 行为决策：打开时应聚焦搜索吗（F02107）；Esc 可关吗（F02104）。 */
export function openBehavior(): { focusSearch: boolean; escClose: boolean; blurClose: boolean; enterFirst: boolean } {
  return {
    focusSearch: getD4<boolean>("F02107") ?? true,
    escClose: getD4<boolean>("F02104") ?? true,
    blurClose: getD4<boolean>("F02105") ?? true,
    enterFirst: getD4<boolean>("F02108") ?? true,
  };
}

/** 开始键等价热键（F02103）：meta / ctrl+esc / alt+f1。 */
export function startHotkey(): string { return getD4<string>("F02103") ?? "meta"; }

/** 状态记忆（F02111）：页码与滚动位置容器。 */
export interface StartMemory { page: number; scrollTop: number; }
const memory = new Map<string, StartMemory>();
export function rememberState(key: string, m: StartMemory): void { memory.set(key, m); }
export function recallState(key: string): StartMemory | undefined { return memory.get(key); }
