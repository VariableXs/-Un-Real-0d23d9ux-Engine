/**
 * AURORA-10000 领域04 · 族0077 任务栏交互 + 族0091 窗口切换器（AI-16/AI-19 批次，勿删）。
 * 图标排序/角标/进度模型 + 切换器排序与过滤模型，纯函数可测。
 */
import { getD4 } from "./prefs";

/** 任务栏图标运行时数据。 */
export interface TaskIcon {
  appId: string;
  label: string;
  pinned: boolean;
  /** 最近使用时间戳（LRU）。 */
  lastUsed: number;
  useCount: number;
  /** 未读数字角标（F01918）。 */
  badge: number;
  /** 内嵌进度 0~1（F01919），null=无进度。 */
  progress: number | null;
  instances: number;
}

/** 图标排序（F01925 频率自排 / F01904 手动重排基础）：固定优先，其余按频率。 */
export function sortIcons(icons: readonly TaskIcon[]): TaskIcon[] {
  const byFreq = getD4<boolean>("F01925") ?? true;
  return [...icons].sort((a, b) => {
    if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
    if (!byFreq) return a.appId.localeCompare(b.appId);
    if (b.useCount !== a.useCount) return b.useCount - a.useCount;
    return b.lastUsed - a.lastUsed;
  });
}

/** Ctrl+数字直达（F01924）：返回序号 → 图标映射（1 起）。 */
export function numberDirect(icons: readonly TaskIcon[]): Map<number, TaskIcon> {
  const m = new Map<number, TaskIcon>();
  [...icons].sort((a, b) => a.appId.localeCompare(b.appId)).forEach((ic, i) => m.set(i + 1, ic));
  return m;
}

/** 点击行为（F01908）：activate / minimize-restore / new-instance。 */
export function clickAction(icon: TaskIcon): "activate" | "minimize-restore" | "new-instance" {
  const mode = getD4<string>("F01908") ?? "activate";
  if (mode === "new-instance") return "new-instance";
  if (mode === "minimize-restore") return icon.instances > 0 ? "minimize-restore" : "activate";
  return icon.instances > 0 ? "activate" : "activate";
}

/* ---------------- 族0091 窗口切换器 ---------------- */

export interface SwitchWindow {
  id: string;
  title: string;
  appId: string;
  desktop: number;
  minimized: boolean;
  lastFocused: number;
}

export type SwitcherLayout = "classic" | "thumbs" | "list" | "ring" | "text";

export interface SwitcherQuery {
  windows: readonly SwitchWindow[];
  currentDesktop: number;
  search?: string;
}

export interface SwitcherResult {
  ordered: SwitchWindow[];
  layout: SwitcherLayout;
  /** 数字键直达（F02259）。 */
  numbered: Map<number, SwitchWindow>;
}

/** 切换器排序与过滤（F02251/57/66/67/68/54/59）。 */
export function buildSwitcher(q: SwitcherQuery): SwitcherResult {
  const layout = getD4<SwitcherLayout>("F02251") ?? "classic";
  const order = getD4<"lru" | "fixed">("F02257") ?? "lru";
  const minMode = getD4<"include" | "exclude">("F02266") ?? "exclude";
  const scope = getD4<"all" | "current">("F02267") ?? "all";
  const currentFirst = getD4<boolean>("F02268") ?? true;
  let list = [...q.windows];
  if (minMode === "exclude") list = list.filter((w) => !w.minimized);
  if (scope === "current") list = list.filter((w) => w.desktop === q.currentDesktop);
  if (q.search) {
    const s = q.search.toLowerCase();
    list = list.filter((w) => w.title.toLowerCase().includes(s) || w.appId.toLowerCase().includes(s));
  }
  list.sort((a, b) => {
    if (currentFirst && (a.desktop === q.currentDesktop) !== (b.desktop === q.currentDesktop)) {
      return a.desktop === q.currentDesktop ? -1 : 1;
    }
    return order === "lru" ? b.lastFocused - a.lastFocused : a.appId.localeCompare(b.appId) || a.title.localeCompare(b.title);
  });
  const numbered = new Map<number, SwitchWindow>();
  list.forEach((w, i) => numbered.set(i + 1, w));
  return { ordered: list, layout, numbered };
}

/** 切换动效时长（F02272）→ CSS ms，令牌外仅作倍率。 */
export function switcherDurationMs(): number { return getD4<number>("F02272") ?? 120; }
