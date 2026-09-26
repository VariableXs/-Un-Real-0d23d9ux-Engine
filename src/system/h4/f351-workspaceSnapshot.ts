/**
 * F351 工作区快照（H 域 · AI-H4）：
 * 把当前所有窗口的位置/尺寸/虚拟桌面归属/最小化态存成命名快照，一键恢复整组布局。
 * 判据（主册 F351）：
 * - 快照/恢复精度 <1px（含多屏——同签名显示器走精确还原，异签名走比例映射）；
 * - 未开应用自动启动（按快照内 Z 序排队，启动三拍子 F283 由调用方执行排队）；
 * - 快照数量上限 10 与管理（超出按最早创建淘汰，命名冲突=覆盖）；
 * - 恢复时最小化窗状态还原（最小化的窗按快照位置记账但不抢前台）。
 * 依赖锚点：F081 任务视图 / F237 窗口记忆（快照优先级更高——恢复时按快照走）/ F283 启动排队。
 * 存储键：variable:h4:f351（本批命名域）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 快照中单个窗口的完整几何与归属。 */
export interface SnapshotWindow {
  /** 应用标识（恢复时用于匹配已开窗口 / 缺失时进入启动队列）。 */
  appId: string;
  /** 恢复匹配用的窗口标题（可选；同应用多窗时辅助消歧）。 */
  title: string | null;
  x: number;
  y: number;
  w: number;
  h: number;
  /** 虚拟桌面归属（F235 语义，0 起）。 */
  vdesk: number;
  /** 物理显示器索引（0 起，对应 displays 数组下标）。 */
  display: number;
  /** 快照时刻的最小化态（恢复时原样还原，不展开）。 */
  minimized: boolean;
  /** Z 序（大者在上；启动队列按此序排队——前排先起）。 */
  z: number;
}

export interface WorkspaceSnapshot {
  name: string;
  createdAt: number;
  /** 快照时刻显示器签名（宽x高@缩放），恢复时用于精确/比例判定。 */
  displays: string[];
  windows: SnapshotWindow[];
}

/** 快照数量上限（判据：上限 10）。 */
export const SNAPSHOT_CAP = 10;

const KEY = h4Key("f351");

function isSnapshotArray(v: unknown): v is WorkspaceSnapshot[] {
  return (
    Array.isArray(v) &&
    v.every((s) => s && typeof (s as WorkspaceSnapshot).name === "string" && Array.isArray((s as WorkspaceSnapshot).windows))
  );
}

function loadAll(store: KvStore): WorkspaceSnapshot[] {
  return readJson(store, KEY, [] as WorkspaceSnapshot[], isSnapshotArray);
}

function saveAll(store: KvStore, all: WorkspaceSnapshot[]): boolean {
  return writeJson(store, KEY, all);
}

/** 列出全部快照（创建时间升序——最老的在前，先淘汰）。 */
export function listSnapshots(store: KvStore = defaultStore()): WorkspaceSnapshot[] {
  return loadAll(store).sort((a, b) => a.createdAt - b.createdAt);
}

/**
 * 保存/覆盖命名快照。数量超上限时淘汰最早的（判据：上限 10 与管理）。
 * 同名 = 覆盖（「我的桌面布置方案」更新语义），且覆盖不新增条目。
 */
export function saveSnapshot(
  name: string,
  windows: SnapshotWindow[],
  displays: string[],
  now: number,
  store: KvStore = defaultStore(),
): { ok: boolean; evicted: string | null; reason: "ok" | "empty-name" | "persist-failed" } {
  if (!name.trim()) return { ok: false, evicted: null, reason: "empty-name" };
  const all = loadAll(store);
  const existingIdx = all.findIndex((s) => s.name === name);
  let evicted: string | null = null;
  if (existingIdx === -1 && all.length >= SNAPSHOT_CAP) {
    const oldest = all.reduce((a, b) => (a.createdAt <= b.createdAt ? a : b));
    all.splice(all.indexOf(oldest), 1);
    evicted = oldest.name;
  }
  const snap: WorkspaceSnapshot = { name, createdAt: now, displays: [...displays], windows: windows.map((w) => ({ ...w })) };
  if (existingIdx >= 0) all[existingIdx] = snap;
  else all.push(snap);
  const ok = saveAll(store, all);
  return { ok, evicted, reason: ok ? "ok" : "persist-failed" };
}

/** 删除命名快照；返回是否确有删除。 */
export function deleteSnapshot(name: string, store: KvStore = defaultStore()): boolean {
  const all = loadAll(store);
  const next = all.filter((s) => s.name !== name);
  if (next.length === all.length) return false;
  return saveAll(store, next);
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** 显示器签名匹配：归一化后比较（剥掉 @缩放 后缀只比 分辨率 串——快照签名带缩放、
 * 恢复面常只给分辨率；归一化是精确/比例判定的唯一闸口）。 */
export function displaysMatch(a: string[], b: string[]): boolean {
  const norm = (arr: string[]) => arr.map((s) => s.split("@")[0] ?? s);
  const na = norm(a);
  const nb = norm(b);
  return na.length === nb.length && na.every((v, i) => v === nb[i]);
}

/**
 * 单窗几何按显示器比例映射（异签名屏）。映射精度：整数像素取整；
 * 同签名时恒等（逐字段不变——恢复精度 <1px 的实现在于恒等路径）。
 */
export function mapRect(rect: Rect, from: { w: number; h: number }, to: { w: number; h: number }): Rect {
  if (from.w === to.w && from.h === to.h) return { ...rect };
  const sx = to.w / from.w;
  const sy = to.h / from.h;
  return {
    x: Math.round(rect.x * sx),
    y: Math.round(rect.y * sy),
    w: Math.max(1, Math.round(rect.w * sx)),
    h: Math.max(1, Math.round(rect.h * sy)),
  };
}

export interface OpenWindowRef {
  appId: string;
  title: string | null;
  currentRect: Rect;
  vdesk: number;
  minimized: boolean;
}

export interface RestoreEntry {
  appId: string;
  title: string | null;
  /** 目标几何（已按显示器映射）。 */
  rect: Rect;
  vdesk: number;
  minimized: boolean;
  /** 是否命中已开窗口（true=改几何；false=进启动队列）。 */
  matched: boolean;
}

export interface RestorePlan {
  /** 逐窗恢复计划（含已匹配与待启动；调用方按 matched 分流）。 */
  entries: RestoreEntry[];
  /** 启动队列：快照里有、当前未开的 appId（按快照 Z 序降序——前排先起，F283 排队）。 */
  launchQueue: string[];
  /** 恢复时采用的模式：精确（<1px）/ 比例映射。 */
  mode: "exact" | "scaled";
  /** 精确模式下误差恒 0；比例模式下给出最大单轴偏差（诊断用）。 */
  maxDriftPx: number;
}

/**
 * 生成恢复计划：快照优先级高于 F237 窗口记忆——恢复时按快照走。
 * 匹配规则：同 appId 且同 title（title 为 null 时只看 appId，取该应用第一个未匹配窗）。
 */
export function planRestore(
  snapshot: WorkspaceSnapshot,
  openWindows: OpenWindowRef[],
  currentDisplays: { w: number; h: number }[],
): RestorePlan {
  const exact = displaysMatch(snapshot.displays, currentDisplays.map((d) => `${d.w}x${d.h}`));
  const snapDisp = snapshot.displays.map((s) => {
    const m = /(\d+)x(\d+)/.exec(s);
    return m ? { w: Number(m[1]), h: Number(m[2]) } : { w: currentDisplays[0]?.w ?? 1920, h: currentDisplays[0]?.h ?? 1080 };
  });
  const used = new Set<number>();
  const entries: RestoreEntry[] = [];
  let maxDrift = 0;
  const zOrdered = [...snapshot.windows].sort((a, b) => b.z - a.z);

  for (const w of zOrdered) {
    const dispIdx = Math.min(w.display, Math.max(0, currentDisplays.length - 1));
    const from = snapDisp[Math.min(dispIdx, snapDisp.length - 1)] ?? { w: 1920, h: 1080 };
    const to = currentDisplays[dispIdx] ?? currentDisplays[0] ?? { w: 1920, h: 1080 };
    const rect = mapRect({ x: w.x, y: w.y, w: w.w, h: w.h }, from, to);
    if (exact) {
      maxDrift = Math.max(maxDrift, Math.abs(rect.x - w.x), Math.abs(rect.y - w.y));
    } else {
      maxDrift = Math.max(maxDrift, Math.abs(rect.x - w.x), Math.abs(rect.y - w.y));
    }
    const idx = openWindows.findIndex(
      (o, i) =>
        !used.has(i) &&
        o.appId === w.appId &&
        (w.title === null || o.title === w.title),
    );
    if (idx >= 0) {
      used.add(idx);
      entries.push({ appId: w.appId, title: w.title, rect, vdesk: w.vdesk, minimized: w.minimized, matched: true });
    } else {
      entries.push({ appId: w.appId, title: w.title, rect, vdesk: w.vdesk, minimized: w.minimized, matched: false });
    }
  }
  return {
    entries,
    launchQueue: entries.filter((e) => !e.matched).map((e) => e.appId),
    mode: exact ? "exact" : "scaled",
    maxDriftPx: maxDrift,
  };
}

/** 快照合法性自检（F375 一致性走查用）：空快照/越界几何判不合格。 */
export function validateSnapshot(s: WorkspaceSnapshot): string[] {
  const problems: string[] = [];
  if (!s.name.trim()) problems.push("名称为空");
  if (s.windows.length === 0) problems.push("无窗口记录");
  for (const w of s.windows) {
    if (w.w <= 0 || w.h <= 0) problems.push(`窗口 ${w.appId} 尺寸非法`);
    if (!Number.isFinite(w.x) || !Number.isFinite(w.y)) problems.push(`窗口 ${w.appId} 坐标非法`);
  }
  if (new Set(s.windows.map((w) => `${w.appId}|${w.title ?? ""}`)).size !== s.windows.length) {
    problems.push("存在重复窗口记录");
  }
  return problems;
}
