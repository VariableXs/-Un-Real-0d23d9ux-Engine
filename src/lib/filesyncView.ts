/**
 * 跨域文件级同步（双域 ③-a）的**视图侧纯逻辑**。
 *
 * 为什么单独放一个模块：`DualBootTab` 的同步面板要跑在浏览器/Tauri 窗口里，
 * 而本仓测试跑在 node 环境（无 jsdom）。把「事件怎么合并进列表」「哪些行该显示」
 * 这类判断从 JSX 里剥出来，就能被单元测试钉住——面板的**行为正确性**不依赖
 * 渲染环境，也不该依赖肉眼验收。
 *
 * 与 Rust `shell::filesync` 的契约：`SyncStatus` / `SyncDiff` / `SyncEntry`
 * 字段名与语义逐条对齐（见 `src/lib/ipc.ts` 的 `Shell` 命名空间）。
 */
import type { Shell } from "./ipc";
import { formatBytes } from "./format";

/** 事件名必须与后端 `shell::filesync::SYNC_EVENT` 逐字一致。 */
export const SYNC_EVENT = "filesync://changed";

/** 兜底轮询间隔（事件通道为主路，这条只防「事件静默失效导致界面静止」）。 */
export const FALLBACK_POLL_MS = 4000;

/** 最近变动列表的展示上限（超出折叠成计数，不让面板被刷爆）。 */
export const RECENT_LIMIT = 6;

/** 一行变动：条目 + 变化类型。 */
export interface RecentChange {
  entry: Shell.SyncEntry;
  kind: "changed" | "removed";
}

/** 未接共享盘时的状态（首帧/降级用；`root` 空串而非编造路径）。 */
export const EMPTY_STATUS: Shell.SyncStatus = {
  available: false,
  root: "",
  entries: 0,
  truncated: false,
};

/**
 * 把一次后端回包整理成面板要的三样东西：状态、展示行、本轮总数。
 *
 * 排序规则：**最新变动排最前**。后端给的是「扫描顺序」（BTreeMap 键序，
 * 即路径字典序），直接展示会让用户看到一堆按文件名排的无关行，而刚刚那笔
 * 改动沉在中间——所以这里统一反转为「后到者在前」。
 *
 * `removed` 只带相对路径，必须补成完整的 `SyncEntry` 骨架再进列表，否则
 * 视图层要写两套分支（易漏、易错）。
 */
export function mergeDiff(diff: Shell.SyncDiff, limit = RECENT_LIMIT): {
  rows: RecentChange[];
  total: number;
} {
  const items: RecentChange[] = [
    ...diff.changed.map((e) => ({ entry: e, kind: "changed" as const })),
    ...diff.removed.map((rel) => ({
      entry: { rel, dir: false, size: 0, mtimeMs: 0 } as Shell.SyncEntry,
      kind: "removed" as const,
    })),
  ];
  const total = items.length;
  // 反转后截断：保留「最近发生的」，丢弃更早的（而不是保留字典序靠前的）。
  const rows = items.reverse().slice(0, limit);
  return { rows, total };
}

/** 单行的主文案（目录不显体积——目录的 0 字节会误导成「空目录」）。 */
export function changeLine(c: RecentChange): string {
  if (c.kind === "removed") return `移除 ${c.entry.rel}`;
  return c.entry.dir ? `更新 ${c.entry.rel}` : `更新 ${c.entry.rel} · ${formatBytes(c.entry.size)}`;
}

/** 状态行的主标题（三态：未接盘 / 已同步 / 已同步但被截断）。 */
export function statusTitle(st: Shell.SyncStatus): string {
  if (!st.available) return "未接共享盘";
  return st.truncated ? "共享盘已接上 · 实时同步中（已达扫描上限）" : "共享盘已接上 · 实时同步中";
}

/** 状态行的副标题（解释「接下来该做什么」，不是只报数字）。 */
export function statusHint(st: Shell.SyncStatus): string {
  if (!st.available) return "插入系统 U 盘后自动接上，无需重启";
  return `${st.entries} 个条目正在受管`;
}

/** 空变动时的提示（告诉用户怎么触发一次同步，而不是干等）。 */
export function idleHint(): string {
  return "暂无变动 · 在 Windows 侧改文件，这里会在 1 秒内出现";
}
