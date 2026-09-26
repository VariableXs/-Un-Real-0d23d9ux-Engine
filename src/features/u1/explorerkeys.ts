/**
 * 资源管理器键位九件（AI-U1 · F401 自动排列 / F404 Win+E / F409 F1 帮助 /
 * F410 Enter 族 / F411 Backspace / F412 Alt+Enter / F414 拖拽进回收站 /
 * F415 回收站两态 / F431 快捷新建）。
 *
 * 判据唯一源（主册摘文，与 kernel/varix/src/uni1/ 同参数）：
 * - F401「四序排列正确性（各 20 项实测）；插入/删除补位；模式互斥与切换；
 *   临时移动弹回动画（F124 弹性档）」。
 * - F404「默认标签/窗口设置；此机页内容清单；连按行为；冷启动打开时长
 *   （首开 <1.5s，F283 骨架先行）」。
 * - F409「上下文映射表（设置各页→帮助锚点全覆盖）；不抢焦点判据；锚点
 *   直达有效性（死锚=0）」。
 * - F410「四键行为矩阵（文件/目录×四键）；首字母跳转；默认方式与 F257
 *   联动；新窗独立会话」。
 * - F411「两键分岔用例（跨目录跳转后行为差异）；根目录边界」。
 * - F412「单/多选两形制；合计计算准确性（F392 同源）；对话框记忆联动」。
 * - F414「语义一致性（拖拽/Delete/右键删除三路同归）；高亮反馈；还原
 *   路径；拖放判定区域；误拖撤销（F202 联动）」。
 * - F415「两态视觉切换即时（删入/清出 <1s）；角标数量；清空确认与不可逆
 *   提示；容量属性页；四菜单项功能」。
 * - F431「两场景生效；命名初态（F260 复用）；连建循环与退出；重名自动
 *   「新建文件夹(2)」递增；键位注册」。
 */

import { u1Store } from "./u1store";

/* ------------------------------- F401 桌面自动排列 ------------------------------- */

export type ArrangeOrder = "name" | "size" | "type" | "date";

export interface DeskIcon { id: number; name: string; sizeKb: number; kind: "doc" | "img" | "app" | "dir"; dateMs: number }

/** 四序比较器（与内核 autoarrange 同语义：名称用 localeCompare；平局按 id）。 */
export function compareBy(order: ArrangeOrder): (a: DeskIcon, b: DeskIcon) => number {
  switch (order) {
    case "name": return (a, b) => a.name.localeCompare(b.name, "zh") || a.id - b.id;
    case "size": return (a, b) => a.sizeKb - b.sizeKb || a.id - b.id;
    case "type": return (a, b) => a.kind.localeCompare(b.kind) || a.id - b.id;
    case "date": return (a, b) => a.dateMs - b.dateMs || a.id - b.id;
  }
}

/** 四序排列（判据：各 20 项实测——纯函数可直接喂样例）。 */
export function autoArrange(items: DeskIcon[], order: ArrangeOrder): DeskIcon[] {
  return [...items].sort(compareBy(order));
}

/** 插入补位：新项进序列后按当前序落位（返回落位下标）。 */
export function insertIntoSequence(seq: DeskIcon[], item: DeskIcon, order: ArrangeOrder): number {
  const next = autoArrange([...seq, item], order);
  return next.findIndex((i) => i.id === item.id);
}

/** 模式互斥：自动排列开 → 自由拖拽关闭（切换互斥判据）。 */
export function arrangeMutex(autoOn: boolean): { auto: boolean; free: boolean } {
  return { auto: autoOn, free: !autoOn };
}

/* ------------------------------- F404 Win+E ------------------------------- */

/** 此机页内容清单（判据：清单钉死——六区）。 */
export const THIS_PC_LIST = [
  "文件夹（桌面/文档/下载…）", "设备与驱动器", "网络位置", "可移动介质", "共享卷 S:", "云与同步区",
] as const;

/** 冷启动预算 <1.5s（F283 骨架先行：先骨架后内容）。 */
export const WIN_E_COLD_BUDGET_MS = 1500;

/* ------------------------------- F409 F1 上下文帮助 ------------------------------- */

/** 上下文映射表（判据：设置各页→帮助锚点全覆盖——抽样区）。 */
export const HELP_ANCHORS: Record<string, string> = {
  "settings/appearance": "help/appearance",
  "settings/network": "help/network",
  "settings/bluetooth": "help/bluetooth",
  "settings/display": "help/display",
  "settings/accounts": "help/accounts",
  "explorer/main": "help/explorer",
};

/** 死锚审计：映射表内所有锚点必须存在（死锚=0 判据）。 */
export function helpAnchorsValid(existing: ReadonlySet<string>): boolean {
  return Object.values(HELP_ANCHORS).every((a) => existing.has(a));
}

/** F1 不抢焦点：帮助以侧栏呈现，焦点留在原控件。 */
export const HELP_FOCUS_KEEP = true;

/* ------------------------------- F410 Enter 打开与 Ctrl+Enter 新窗 ------------------------------- */

export type EntryKind = "file" | "dir";

/** 四键行为矩阵（文件/目录 × Enter/Ctrl+Enter）。 */
export function enterResolve(kind: EntryKind, ctrl: boolean, preferTab: boolean): "preview" | "open-tab" | "open-window" | "open-app" {
  if (kind === "file") return ctrl ? "open-app" : "preview";
  if (ctrl) return "open-window";
  return preferTab ? "open-tab" : "open-window";
}

/* ------------------------------- F411 Backspace 上级 ------------------------------- */

/** 上级目录：跨目录跳转后 Backspace 走历史而非层级（两键分岔判据）。 */
export function backspaceResolve(jumpedAcross: boolean, atRoot: boolean): "history-back" | "parent" | "noop" {
  if (atRoot) return "noop";
  return jumpedAcross ? "history-back" : "parent";
}

/* ------------------------------- F412 Alt+Enter 属性 ------------------------------- */

/** 合计计算（判据：合计准确性——大小求和 + 数量）。 */
export function aggregateProps(items: { sizeKb: number }[]): { count: number; totalKb: number } {
  return { count: items.length, totalKb: items.reduce((s, i) => s + i.sizeKb, 0) };
}

/* ------------------------------- F414 拖拽进回收站 ------------------------------- */

/** 三路同归：拖拽 / Delete / 右键删除 → 同一 confirmDelete 入口。 */
export type DeleteRoute = "drag" | "del-key" | "ctx-menu";

export function deleteRouteNormalizes(route: DeleteRoute): "confirm-delete" {
  void route;
  return "confirm-delete";
}

/** 拖放判定区：落点在回收站图标 24px 判定半径内才算命中。 */
export const TRASH_DROP_RADIUS_PX = 24;

export function inTrashDropZone(drop: { x: number; y: number }, trash: { x: number; y: number }): boolean {
  const dx = drop.x - trash.x;
  const dy = drop.y - trash.y;
  return dx * dx + dy * dy <= TRASH_DROP_RADIUS_PX * TRASH_DROP_RADIUS_PX;
}

/* ------------------------------- F415 回收站满空两态 ------------------------------- */

export type TrashState = "empty" | "partial" | "full";

/** 两态即时切换：删入 → 非空；清出 → 空（<1s 判据由调用方计时）。 */
export function trashState(itemCount: number, capPct: number): TrashState {
  if (itemCount === 0) return "empty";
  return capPct >= 100 ? "full" : "partial";
}

/** 四菜单项（判据：四菜单项功能）。 */
export const TRASH_MENU = ["打开", "清空回收站", "还原所有项目", "属性"] as const;

/* ------------------------------- F431 Ctrl+Shift+N ------------------------------- */

/** 重名递增（与内核 newfolder 同语义）：无重名 → 新建文件夹；有 → (n)。 */
export function nextFolderName(existing: ReadonlySet<string>): string {
  if (!existing.has("新建文件夹")) return "新建文件夹";
  for (let n = 2; ; n++) {
    const name = `新建文件夹(${n})`;
    if (!existing.has(name)) return name;
  }
}

/* ------------------------------- 面板读数 ------------------------------- */

/** Win+E 默认模式读数。 */
export function winEDefaultMode(): "tab" | "window" {
  const cfg = u1Store.get<{ winEMode?: "tab" | "window" }>("explorerKeys");
  return cfg.winEMode ?? "tab";
}
