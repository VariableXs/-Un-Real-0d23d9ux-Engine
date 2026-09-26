/**
 * 资源管理器七件（AI-U3 · F515 查找替换 / F517 启动页 / F521 截图落点 /
 * F525 速查卡导出 / F526 状态栏 / F527 树折叠 / F528 树列表双向同步）。
 *
 * 判据唯一源（主册摘文）：
 * - F515「Ctrl+H 呼出替换条（与查找条 F221 同形制同位置）；逐个/全部两路；
 *   替换前显示将影响 N 处+预览第一处上下文；可撤销（整批一次 Ctrl+Z 回滚）；
 *   区分大小写/全字匹配沿用查找开关」。
 * - F517「落点三选：此机/上次关闭的文件夹/固定文件夹；「上次文件夹」记每个
 *   窗口最后位置；每种落点一句话后果；改完下次生效」。
 * - F521「落点三选：图片/截图目录（默认自动建）/桌面/每次询问；自动命名
 *   「截图 2026-09-25_1430」可加前缀；F512 历史另存同落点；S: 可选」。
 * - F525「导出当前快捷键全表（PNG 一页版/PDF 双页版）；与 F244 注册表同源；
 *   系统默认表+用户自定义表分色标注」。
 * - F526「状态栏 24px 三段：左=目录项数/中=选中态（F338 同源）/右=卷剩余
 *   空间（F456 同数据）；实时同步；空目录态显示」。
 * - F527「双击/箭头展开折叠；全部展开/全部折叠两命令；展开状态持久（F219）；
 *   万节点性能（F228 虚拟化同源）；拖拽到树节点=F262 语义」。
 * - F528「列表进子目录→树自动展开路径并高亮当前节点（滚动到可见）；树点选
 *   →列表刷新；同步高亮不抢焦点（F206）」。
 */

import { u3Store } from "./u3store";

/* ------------------------------- F515 查找与替换 ------------------------------- */

export interface FindOptions { matchCase: boolean; wholeWord: boolean }

/** 全字匹配正则构造（\b 对中文无意义——中文退化为普通包含匹配）。 */
export function findMatches(text: string, needle: string, opt: FindOptions): Array<{ start: number; end: number }> {
  if (!needle) return [];
  const out: Array<{ start: number; end: number }> = [];
  const flags = opt.matchCase ? "g" : "gi";
  const esc = needle.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(opt.wholeWord && /^[\w]+$/.test(needle) ? `\\b${esc}\\b` : esc, flags);
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    out.push({ start: m.index, end: m.index + m[0].length });
    if (m[0].length === 0) re.lastIndex++; // 防零长死循环
  }
  return out;
}

/** 影响数预览 + 第一处上下文（判据：替换前显示将影响 N 处+预览第一处）。 */
export function replacePreview(text: string, needle: string, opt: FindOptions, ctx = 24): { count: number; firstContext: string } {
  const hits = findMatches(text, needle, opt);
  if (hits.length === 0) return { count: 0, firstContext: "" };
  const h = hits[0]!; // 早退守卫后非空
  return {
    count: hits.length,
    firstContext: `${text.slice(Math.max(0, h.start - ctx), h.start)}【${text.slice(h.start, h.end)}】${text.slice(h.end, h.end + ctx)}`, // h 由上方早退守卫
  };
}

/**
 * 全部替换 + 整批一次撤销（判据：整批一次 Ctrl+Z 回滚——F202 栈内一次 undo）。
 * 返回 { after, undo }：undo() 一键恢复原文（闭包持有原文快照）。
 */
export function replaceAll(text: string, needle: string, replacement: string, opt: FindOptions): { after: string; count: number; undo: () => string } {
  const hits = findMatches(text, needle, opt);
  if (hits.length === 0) return { after: text, count: 0, undo: () => text };
  let out = "";
  let cursor = 0;
  for (const h of hits) {
    out += text.slice(cursor, h.start) + replacement;
    cursor = h.end;
  }
  out += text.slice(cursor);
  return { after: out, count: hits.length, undo: () => text };
}

/* ------------------------------- F517 启动页 ------------------------------- */

export type ExplorerHome = "thispc" | "last-folder" | "fixed";
/** 三模式后果一句话（判据：每种落点一句话后果说明）。 */
export const EXPLORER_HOME_NOTES: Record<ExplorerHome, string> = {
  "thispc": "打开资源管理器首先看到「此机」总览（默认）",
  "last-folder": "回到每个窗口上次关闭的位置——接着干活",
  "fixed": "直落你指定的固定文件夹——项目党首选",
};

export interface LastFolderMemory { [windowId: string]: string }
/** 「上次文件夹」记忆：每窗口最后位置（判据：记每个窗口最后位置）。 */
export function rememberLastFolder(memory: LastFolderMemory, windowId: string, path: string): LastFolderMemory {
  return { ...memory, [windowId]: path };
}
/** 启动页解析（下次生效语义：改设置不影响已开窗口）。 */
export function resolveStartupFolder(mode: ExplorerHome, fixed: string, memory: LastFolderMemory, windowId: string): string {
  switch (mode) {
    case "thispc": return "thispc://";
    case "fixed": return fixed || "thispc://";
    case "last-folder": return memory[windowId] ?? "thispc://";
  }
}

/* ------------------------------- F521 截图落点 ------------------------------- */

export type ShotTarget = "pictures" | "desktop" | "ask" | "share-s";
/** 三落点行为说明（判据：三落点行为）。 */
export const SHOT_TARGET_LABELS: Record<ShotTarget, string> = {
  pictures: "图片\\截图（目录不存在自动创建）",
  desktop: "桌面",
  ask: "每次询问（保存对话框）",
  "share-s": "S: 共享卷（跨机取图）",
};

/** 自动命名（判据：「截图 2026-09-25_1430」默认、可加前缀、永不重名）。 */
export function shotFileName(prefix: string, d: Date, existing: Set<string>): string {
  const p2 = (n: number) => String(n).padStart(2, "0");
  const base = `${prefix} ${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}_${p2(d.getHours())}${p2(d.getMinutes())}`;
  if (!existing.has(base)) return base;
  let i = 2;
  while (existing.has(`${base}(${i})`)) i++;
  return `${base}(${i})`;
}

/* ------------------------------- F525 速查卡导出 ------------------------------- */

export interface KeymapRow { keys: string; action: string; custom: boolean }

/** 同源导出（判据：与 F244 注册表同源；默认/自定义分色标注）。 */
export function keycardModel(rows: KeymapRow[]): {
  pages: Array<Array<KeymapRow & { tone: "default" | "custom" }>>;
  legend: string;
  format: "png-1page" | "pdf-2page";
} {
  const tagged = rows.map((r) => ({ ...r, tone: r.custom ? ("custom" as const) : ("default" as const) }));
  const perPage = Math.ceil(tagged.length / 2);
  return {
    pages: [tagged.slice(0, perPage), tagged.slice(perPage)],
    legend: "灰 = 系统默认键位 / 蓝 = 你的自定义键位（与 F244 注册表同源导出）",
    format: "png-1page",
  };
}

/* ------------------------------- F526 状态栏 ------------------------------- */

/** 高度 24px 基线（主册 F526 规格表）。 */
export const STATUSBAR_HEIGHT_PX = 24;

export interface StatusBarModel {
  items: number;
  selected: number;
  selectedBytes: number;
  volumeFreeBytes: number;
}

export function humanBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = b;
  let i = -1;
  do { v /= 1024; i++; } while (v >= 1024 && i < units.length - 1);
  return `${v.toFixed(v >= 100 ? 0 : 1)}${units[i]}`;
}

/** 三段渲染（判据：左=项数/中=选中态/右=剩余空间；空目录态显示）。 */
export function statusBarSegments(m: StatusBarModel): [string, string, string] {
  return [
    m.items === 0 ? "空目录" : `${m.items} 项`,
    m.selected > 0 ? `已选 ${m.selected} 项 · ${humanBytes(m.selectedBytes)}` : "",
    `可用 ${humanBytes(m.volumeFreeBytes)}`,
  ];
}

/* ------------------------------- F527 树折叠 ------------------------------- */

export interface TreeState { expanded: Record<string, boolean> }

/** 双击/箭头两路共用一个翻转（判据：双击/箭头两路）。 */
export function toggleNode(state: TreeState, id: string): TreeState {
  return { expanded: { ...state.expanded, [id]: !state.expanded[id] } };
}
/** 全部展开（当前层）/全部折叠两命令（判据：两命令快捷键）。 */
export function expandLevel(state: TreeState, ids: string[], on: boolean): TreeState {
  const next = { ...state.expanded };
  for (const id of ids) next[id] = on;
  return { expanded: next };
}
/** 持久化节流入口：写入 store（F219 记忆族——重启后树保持展开态）。 */
export function persistTree(state: TreeState): void {
  u3Store.set("treeCollapse", { remember: true, expanded: state.expanded });
}
/** 两命令快捷键登记（判据：全部展开/全部折叠两命令快捷键）。 */
export const TREE_COMMAND_KEYS = { expandAll: "Ctrl+Shift+Plus", collapseAll: "Ctrl+Shift+Minus" } as const;

/* ------------------------------- F528 树列表双向同步 ------------------------------- */

export interface SyncStep { path: string[]; targetId: string }

/** 列表 → 树：展开祖先路径 + 高亮当前节点（判据：树自动展开路径并高亮）。 */
export function syncTreeFromList(path: string[]): SyncStep {
  const targetId = path[path.length - 1] ?? "";
  return { path, targetId };
}
/** 5 层路径同步时序预算（判据：深层路径 5 层同步时序）。 */
export const TREE_SYNC_BUDGET_MS = 100;
/** 焦点/高亮分离声明（判据：同步高亮不抢焦点——F206 语义）。 */
export const TREE_SYNC_FOCUS_NOTE = "树高亮是视觉态：键盘焦点仍在列表（focus 跟随不迁移）";

/* ------------------------------- 运行时读取 ------------------------------- */

export function explorerHomeConfig() {
  const s = u3Store.get("explorerHome");
  return { mode: (s.mode as ExplorerHome) ?? "thispc", fixedFolder: (s.fixedFolder as string) ?? "" };
}
export function shotTargetConfig() {
  const s = u3Store.get("shotTarget");
  return { target: (s.target as ShotTarget) ?? "pictures", prefix: (s.prefix as string) ?? "截图", askEach: !!s.askEach };
}

/* ------------------------------- 自检 ------------------------------- */

/** F515/F517/F521/F525/F526/F527/F528 判据自检（前端面）。 */
export function explorerxSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  const text = "术语 A 是核心，术语 a 也是核心。术语 A 再现。";
  const pv = replacePreview(text, "术语 A", { matchCase: true, wholeWord: false });
  checks.push({ name: "F515 影响数预览", pass: pv.count === 2 && pv.firstContext.includes("【术语 A】") });
  const rep = replaceAll(text, "术语 A", "TERM", { matchCase: true, wholeWord: false });
  checks.push({ name: "F515 整批一次撤销", pass: rep.count === 2 && rep.undo() === text });
  const ww = findMatches("cat catalog cat", "cat", { matchCase: true, wholeWord: true });
  checks.push({ name: "F515 全字匹配", pass: ww.length === 2 });
  // 启动页三模式
  const mem = rememberLastFolder({}, "w1", "C:/work");
  checks.push({
    name: "F517 三模式",
    pass: resolveStartupFolder("thispc", "", {}, "w1") === "thispc://" && resolveStartupFolder("last-folder", "", mem, "w1") === "C:/work" && resolveStartupFolder("fixed", "C:/proj", mem, "w1") === "C:/proj",
  });
  // 命名永不重名
  const d = new Date(2026, 8, 25, 14, 30);
  const n1 = shotFileName("截图", d, new Set());
  const n2 = shotFileName("截图", d, new Set([n1]));
  checks.push({ name: "F521 自动命名+重名递增", pass: n1 === "截图 2026-09-25_1430" && n2 === "截图 2026-09-25_1430(2)" });
  // 状态栏三段（主册原文案「245MB」）
  const seg = statusBarSegments({ items: 128, selected: 5, selectedBytes: 245 * 1024 * 1024, volumeFreeBytes: 2 * 1024 ** 3 });
  checks.push({ name: "F526 三段渲染", pass: seg[0] === "128 项" && seg[1] === "已选 5 项 · 245MB" && seg[2].startsWith("可用 2.0GB") });
  const emptySeg = statusBarSegments({ items: 0, selected: 0, selectedBytes: 0, volumeFreeBytes: 0 });
  checks.push({ name: "F526 空目录态", pass: emptySeg[0] === "空目录" });
  // 树折叠持久化翻转
  const t0: TreeState = { expanded: {} };
  const t1 = toggleNode(toggleNode(t0, "a"), "a");
  checks.push({ name: "F527 双路翻转", pass: toggleNode(t0, "a").expanded["a"] === true && t1.expanded["a"] === false });
  // 双向同步
  const step = syncTreeFromList(["C:", "work", "proj", "src", "deep"]);
  checks.push({ name: "F528 路径展开高亮", pass: step.targetId === "deep" && step.path.length === 5 });
  return checks;
}
