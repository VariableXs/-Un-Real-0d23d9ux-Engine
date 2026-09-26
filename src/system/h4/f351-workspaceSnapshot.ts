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
import { checksumOf } from "./internal/hash";

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

/* ================= v4 深化批次四：完整性校验 / 快照差异 / 导出导入 / 恢复演练 ================= */

/** 快照完整性校验和（规范化结构指纹）：导出/导入防篡改与损坏检测的锚。 */
export function snapshotChecksum(s: WorkspaceSnapshot): string {
  return checksumOf({ name: s.name, createdAt: s.createdAt, displays: s.displays, windows: s.windows });
}

/** 校验和核对：expected 不符 = 快照被改过或损坏（导入面据此拒收）。 */
export function verifySnapshotIntegrity(s: WorkspaceSnapshot, expected: string): { ok: boolean; actual: string } {
  const actual = snapshotChecksum(s);
  return { ok: actual === expected, actual };
}

export type SnapshotDiffKind = "added" | "removed" | "moved" | "resized" | "state" | "unchanged";

export interface SnapshotDiffRow {
  /** 窗口身份键（appId|title）。 */
  key: string;
  kind: SnapshotDiffKind;
  from: SnapshotWindow | null;
  to: SnapshotWindow | null;
}

/**
 * 快照差异（管理面的「这个快照和那个差在哪」）：按窗口身份对齐，
 * moved/resized/state 三类细分让差异一眼可读（不糊成一锅「有变化」）。
 */
export function diffSnapshots(a: WorkspaceSnapshot, b: WorkspaceSnapshot): SnapshotDiffRow[] {
  const keyOf = (w: SnapshotWindow) => `${w.appId}|${w.title ?? ""}`;
  const mapA = new Map(a.windows.map((w) => [keyOf(w), w]));
  const mapB = new Map(b.windows.map((w) => [keyOf(w), w]));
  const rows: SnapshotDiffRow[] = [];
  for (const [key, wa] of mapA) {
    const wb = mapB.get(key);
    if (!wb) {
      rows.push({ key, kind: "removed", from: wa, to: null });
      continue;
    }
    if (wa.x !== wb.x || wa.y !== wb.y) rows.push({ key, kind: "moved", from: wa, to: wb });
    else if (wa.w !== wb.w || wa.h !== wb.h) rows.push({ key, kind: "resized", from: wa, to: wb });
    else if (wa.minimized !== wb.minimized || wa.vdesk !== wb.vdesk) rows.push({ key, kind: "state", from: wa, to: wb });
    else rows.push({ key, kind: "unchanged", from: wa, to: wb });
  }
  for (const [key, wb] of mapB) if (!mapA.has(key)) rows.push({ key, kind: "added", from: null, to: wb });
  return rows;
}

/** 启动队列口径（判据「未开应用自动启动」）：按快照 Z 序去重——前排先起、同应用只起一次。 */
export function launchOrder(snapshot: WorkspaceSnapshot): string[] {
  const seen = new Set<string>();
  const order: string[] = [];
  for (const w of [...snapshot.windows].sort((a, b) => b.z - a.z)) {
    if (seen.has(w.appId)) continue;
    seen.add(w.appId);
    order.push(w.appId);
  }
  return order;
}

/** 快照重命名：同名语义与 saveSnapshot 一致（改到已存在名 = 覆盖合并并删旧条目）。 */
export function renameSnapshot(oldName: string, newName: string, store: KvStore = defaultStore()): { ok: boolean; reason: "ok" | "missing" | "empty-name" | "persist-failed"; mergedOver: string | null } {
  if (!newName.trim()) return { ok: false, reason: "empty-name", mergedOver: null };
  const all = loadAll(store);
  const idx = all.findIndex((s) => s.name === oldName);
  if (idx < 0) return { ok: false, reason: "missing", mergedOver: null };
  const snap = all[idx]!;
  const targetIdx = newName === oldName ? idx : all.findIndex((s) => s.name === newName);
  const renamed = { ...snap, name: newName };
  if (targetIdx >= 0 && targetIdx !== idx) {
    all[targetIdx] = renamed;
    all.splice(idx, 1);
    return { ok: saveAll(store, all), reason: "ok", mergedOver: newName };
  }
  all[idx] = renamed;
  return { ok: saveAll(store, all), reason: "ok", mergedOver: null };
}

/* ---------- 导出 / 导入（跨机迁移面：规范信封 + 校验和防篡改） ---------- */

const EXPORT_KIND = "varix-h4-snapshot";

export interface SnapshotEnvelope {
  kind: typeof EXPORT_KIND;
  v: 1;
  checksum: string;
  payload: WorkspaceSnapshot;
}

/** 导出：规范化信封 + 校验和（接收方据此验完整性）。 */
export function exportSnapshot(s: WorkspaceSnapshot): string {
  const envelope: SnapshotEnvelope = { kind: EXPORT_KIND, v: 1, checksum: snapshotChecksum(s), payload: s };
  return JSON.stringify(envelope, null, 2);
}

/** 导入：三道闸——可解析、信封种类/版本、校验和与快照自检全过才收（防损坏防篡改）。 */
export function importSnapshot(text: string): { ok: boolean; snapshot: WorkspaceSnapshot | null; problems: string[] } {
  let envelope: unknown;
  try {
    envelope = JSON.parse(text);
  } catch {
    return { ok: false, snapshot: null, problems: ["不是合法 JSON——导入被拒收"] };
  }
  const e = envelope as Partial<SnapshotEnvelope> | null;
  if (!e || e.kind !== EXPORT_KIND || e.v !== 1 || typeof e.checksum !== "string" || !e.payload) {
    return { ok: false, snapshot: null, problems: ["信封种类或版本不符——不是 Varix 工作区快照"] };
  }
  const integrity = verifySnapshotIntegrity(e.payload, e.checksum);
  if (!integrity.ok) {
    return { ok: false, snapshot: null, problems: [`校验和不符（期待 ${e.checksum}，实得 ${integrity.actual}）——快照已损坏或被篡改`] };
  }
  const structural = validateSnapshot(e.payload);
  if (structural.length > 0) {
    return { ok: false, snapshot: null, problems: structural };
  }
  return { ok: true, snapshot: e.payload, problems: [] };
}

/* ---------- 恢复演练（Dry-Run：不真的动窗口，先把恢复计划核一遍） ---------- */

export interface DryRunReport {
  mode: "exact" | "scaled";
  /** 逐窗动作摘要：改几何 / 启动 / 改几何且保持最小化。 */
  actions: Array<{ appId: string; action: "reposition" | "launch" | "reposition-minimized" }>;
  maxDriftPx: number;
  /** 越界警告：映射后落点超出当前显示器的窗（恢复前就该知道）。 */
  outOfBounds: string[];
  pass: boolean;
}

/** 恢复演练：精确模式必须零漂移；比例模式给最大漂移；越界窗提前点名（<1px 判据的预检面）。 */
export function dryRunRestore(snapshot: WorkspaceSnapshot, openWindows: OpenWindowRef[], currentDisplays: { w: number; h: number }[]): DryRunReport {
  const plan = planRestore(snapshot, openWindows, currentDisplays);
  const actions = plan.entries.map((e) => ({
    appId: e.appId,
    action: (e.matched ? (e.minimized ? "reposition-minimized" : "reposition") : "launch") as DryRunReport["actions"][number]["action"],
  }));
  const outOfBounds: string[] = [];
  for (const e of plan.entries) {
    const snapWin = snapshot.windows.find((w) => w.appId === e.appId);
    const dispIdx = Math.min(snapWin?.display ?? 0, Math.max(0, currentDisplays.length - 1));
    const disp = currentDisplays[dispIdx] ?? { w: 1920, h: 1080 };
    if (e.rect.x < 0 || e.rect.y < 0 || e.rect.x + e.rect.w > disp.w || e.rect.y + e.rect.h > disp.h) {
      outOfBounds.push(`${e.appId} → (${e.rect.x},${e.rect.y} ${e.rect.w}x${e.rect.h}) 超出 ${disp.w}x${disp.h}`);
    }
  }
  return {
    mode: plan.mode,
    actions,
    maxDriftPx: plan.maxDriftPx,
    outOfBounds,
    pass: plan.mode === "exact" ? plan.maxDriftPx === 0 && outOfBounds.length === 0 : outOfBounds.length === 0,
  };
}
