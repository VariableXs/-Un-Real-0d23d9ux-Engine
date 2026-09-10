import { vwmStore, isTpApp, tpIdOf, type VwmApp, type VwmRect } from "./vwm";
import { getThirdApps } from "../launcher/thirdApps";

/**
 * 批次W-4：布局快照 —— 把当前 VWM 全部虚拟窗口的几何/Z 序/贴靠/最小化态
 * 存成一命名快照，恢复时精确（显示器签名匹配）或按比例映射（换屏）还原。
 * - 手动保存（设置→外观→布局快照「保存当前布局」）
 * - 退出自动保存一份 `__autosave__`
 * - 第三方应用缺失时**不自动启动**，以「快照缺失项」如实列出
 * 存储键：`variable:vwm:snapshots`（localStorage，与 vwm.ts 同域）。
 */

export interface SnapshotWin {
  app: VwmApp;
  geom: VwmRect;
  z: number;
  /** true = 快照时处于最大化（恢复时按 snapped 处理）。 */
  snapped: boolean;
  minimized: boolean;
  /** explorer 初始定位路径（仅系统窗口使用）。 */
  path: string | null;
}

export interface VwmSnapshot {
  name: string;
  created: number;
  windows: SnapshotWin[];
  /** 显示器签名（宽x高@缩放）；恢复时用于精确/按比例判定。 */
  displays: string[];
}

const KEY = "variable:vwm:snapshots";
export const AUTOSAVE_NAME = "__autosave__";

/** 当前显示器签名（webview 内可观测面：主屏尺寸 + 缩放）。 */
export function displaySignature(): string[] {
  try {
    const scale = window.devicePixelRatio > 0 ? window.devicePixelRatio : 1;
    return [`${window.screen.width}x${window.screen.height}@${scale.toFixed(2)}`];
  } catch {
    return ["unknown"];
  }
}

function loadAll(): VwmSnapshot[] {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "[]") as VwmSnapshot[];
    return Array.isArray(raw) ? raw.filter((s) => s && typeof s.name === "string") : [];
  } catch {
    return [];
  }
}

function saveAll(all: VwmSnapshot[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    /* storage full/blocked → 快照不持久化（如实降级） */
  }
}

export function listSnapshots(): VwmSnapshot[] {
  return loadAll();
}

/** 抓取当前窗口布局（不含关闭动画中的窗口）。 */
export function captureCurrent(): SnapshotWin[] {
  return vwmStore
    .getState()
    .wins.filter((w) => !vwmStore.getState().closing.includes(w.id))
    .map((w) => ({
      app: w.app,
      geom: { x: Math.round(w.x), y: Math.round(w.y), w: Math.round(w.w), h: Math.round(w.h) },
      z: w.z,
      snapped: w.state === "max",
      minimized: w.minimized,
      path: w.path,
    }));
}

/** 保存/覆盖命名快照。 */
export function saveSnapshot(name: string): void {
  const trimmed = name.trim();
  if (!trimmed) return;
  const all = loadAll().filter((s) => s.name !== trimmed);
  all.push({ name: trimmed, created: Date.now(), windows: captureCurrent(), displays: displaySignature() });
  saveAll(all);
}

/** 退出时自动保存（覆盖旧自动档）。 */
export function autosaveSnapshot(): void {
  saveSnapshot(AUTOSAVE_NAME);
}

// ---------- 批次W-5：显示器热切换 + 分屏记忆 ----------

const AUTO_PREFIX = "__auto:";

/** 当前签名记忆（initDisplayMemory 在桌面壳启动时置位）。 */
let lastDisplaySig: string | null = null;

export function initDisplayMemory(): void {
  lastDisplaySig = displaySignature()[0] ?? null;
}

/** 出屏窗口吸附回工作区最近合法位置（拔屏兜底，零丢窗）。 */
export function snapOffscreenBack(): void {
  const s = vwmStore.getState();
  const wa = s.workArea;
  if (wa.w <= 0) return;
  const clamped = s.wins.map((w) => {
    const x = Math.min(Math.max(w.x, wa.x - w.w + 120), Math.max(wa.x, wa.x + wa.w - 120));
    const y = Math.min(Math.max(w.y, wa.y), Math.max(wa.y, wa.y + wa.h - 48));
    const wd = Math.min(w.w, wa.w);
    const ht = Math.min(w.h, wa.h);
    return w.x === x && w.y === y && w.w === wd && w.h === ht
      ? w
      : { ...w, x: Math.round(x), y: Math.round(y), w: Math.round(wd), h: Math.round(ht) };
  });
  if (clamped.some((w, i) => w !== s.wins[i])) {
    vwmStore.setState({ wins: clamped, seq: s.seq + 1 });
  }
}

/**
 * 显示器签名变化（`sys://display-changed`）：
 * - 旧签名布局自动存档（`__auto:<旧签名>`）
 * - 新签名有存档 → 整体恢复（分屏记忆：单屏↔双屏切换即恢复）
 * - 无存档 → 出屏窗口吸附回主屏最近合法位置
 */
export function handleDisplayChanged(): void {
  const sig = displaySignature()[0] ?? "unknown";
  if (lastDisplaySig === null) {
    lastDisplaySig = sig;
    return;
  }
  if (sig === lastDisplaySig) return;
  const prevSig = lastDisplaySig;
  lastDisplaySig = sig;
  // 先把旧屏布局存档（含几何/Z 序/最小化态）
  const prevWins = captureCurrent();
  if (prevWins.length > 0) {
    const all = loadAll().filter((s) => s.name !== AUTO_PREFIX + prevSig);
    all.push({
      name: AUTO_PREFIX + prevSig,
      created: Date.now(),
      windows: prevWins,
      displays: [prevSig],
    });
    saveAll(all);
  }
  // 新签名有分屏记忆 → 恢复；否则出屏吸附
  const target = AUTO_PREFIX + sig;
  if (loadAll().some((s) => s.name === target)) {
    restoreSnapshot(target);
  } else {
    snapOffscreenBack();
  }
}

/** 自动分屏存档名是否隐藏于用户列表。 */
export function isAutoSnapshotName(name: string): boolean {
  return name.startsWith(AUTO_PREFIX) || name === AUTOSAVE_NAME;
}

export function deleteSnapshot(name: string): void {
  saveAll(loadAll().filter((s) => s.name !== name));
}

export function renameSnapshot(oldName: string, newName: string): boolean {
  const trimmed = newName.trim();
  if (!trimmed) return false;
  const all = loadAll();
  if (all.some((s) => s.name === trimmed && s.name !== oldName)) return false;
  const hit = all.find((s) => s.name === oldName);
  if (!hit) return false;
  hit.name = trimmed;
  saveAll(all);
  return true;
}

/** 导出 JSON 文本（下载由 UI 层完成）。 */
export function exportSnapshot(name: string): string | null {
  const hit = loadAll().find((s) => s.name === name);
  return hit ? JSON.stringify(hit, null, 2) : null;
}

/** 从 JSON 文本导入（校验结构；重名报错）。 */
export function importSnapshot(json: string): { ok: boolean; error?: string } {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return { ok: false, error: "invalid JSON" };
  }
  const s = parsed as VwmSnapshot;
  if (!s || typeof s.name !== "string" || !Array.isArray(s.windows) || !Array.isArray(s.displays)) {
    return { ok: false, error: "missing fields" };
  }
  const name = s.name === AUTOSAVE_NAME ? "imported" : s.name;
  if (loadAll().some((x) => x.name === name)) return { ok: false, error: "duplicate name" };
  const all = loadAll();
  all.push({
    name,
    created: typeof s.created === "number" ? s.created : Date.now(),
    windows: s.windows
      .filter((w) => w && w.app && w.geom)
      .map((w) => ({
        app: w.app,
        geom: w.geom,
        z: typeof w.z === "number" ? w.z : 0,
        snapped: !!w.snapped,
        minimized: !!w.minimized,
        path: typeof w.path === "string" ? w.path : null,
      })),
    displays: s.displays,
  });
  saveAll(all);
  return { ok: true };
}

/** 从签名串解析尺寸（解析失败返回 null → 走精确还原）。 */
function parseSig(sig: string): { w: number; h: number } | null {
  const m = /^(\d+)x(\d+)@/.exec(sig);
  return m ? { w: Number(m[1]), h: Number(m[2]) } : null;
}

export interface RestoreResult {
  /** 实际重建的窗口数。 */
  restored: number;
  /** 快照缺失项：第三方应用已不在登记表 → 不自动启动，仅列出。 */
  missing: string[];
}

/**
 * 恢复快照：替换当前全部虚拟窗口。
 * - 显示器签名匹配 → 精确还原
 * - 不匹配（换屏）→ 按比例映射到当前屏幕，并钳制在工作区内（无窗口丢出屏外）
 * - 第三方应用缺失 → 不启动，列入缺失项
 */
export function restoreSnapshot(name: string): RestoreResult {
  const snap = loadAll().find((s) => s.name === name);
  if (!snap) return { restored: 0, missing: [] };

  const missing: string[] = [];
  const usable = snap.windows.filter((w) => {
    if (!isTpApp(w.app)) return true;
    const id = tpIdOf(w.app);
    if (getThirdApps().some((a) => a.id === id)) return true;
    if (!missing.includes(id)) missing.push(id);
    return false;
  });

  // 比例映射：签名不匹配时按新旧屏幕尺寸缩放几何
  const curSig = displaySignature()[0] ?? "unknown";
  const oldSig = snap.displays[0] ?? curSig;
  const scale = (() => {
    const cur = parseSig(curSig);
    const old = parseSig(oldSig);
    if (!cur || !old || (cur.w === old.w && cur.h === old.h)) return { x: 1, y: 1 };
    return { x: cur.w / Math.max(1, old.w), y: cur.h / Math.max(1, old.h) };
  })();

  const wa = vwmStore.getState().workArea;
  const clamp = (g: VwmRect): VwmRect => {
    const w = Math.min(Math.max(Math.round(g.w * scale.x), 300), Math.max(300, wa.w));
    const h = Math.min(Math.max(Math.round(g.h * scale.y), 220), Math.max(220, wa.h));
    const x = Math.min(Math.max(Math.round(g.x * scale.x), wa.x - w + 120), Math.max(wa.x, wa.x + wa.w - 120));
    const y = Math.min(Math.max(Math.round(g.y * scale.y), wa.y), Math.max(wa.y, wa.y + wa.h - 48));
    return { x, y, w, h };
  };

  // 按 Z 序重建（保持快照的层叠关系）；最大化窗口恢复为占满工作区
  const ordered = [...usable].sort((a, b) => a.z - b.z);
  let topZ = vwmStore.getState().topZ;
  const wins = ordered.map((w, i) => {
    const geom = w.snapped ? { ...wa } : clamp(w.geom);
    const z = topZ + i + 1;
    return {
      id: `vwm-${w.app}-${Date.now().toString(36)}${i}`,
      app: w.app,
      path: w.path,
      ...geom,
      state: w.snapped ? ("max" as const) : ("normal" as const),
      minimized: w.minimized && !w.snapped,
      // 批次F：布局快照恢复 = 全新会话态，隐藏是运行时操作不属于快照语义
      hidden: false,
      z,
      restore: null,
      group: null,
      groupActive: false,
      rolledUp: false,
      minimizedAt: null,
      opacity: 1,
      topmost: false,
    };
  });
  if (wins.length > 0) topZ = wins[wins.length - 1]!.z;
  const focusTop = [...wins].filter((w) => !w.minimized).sort((a, b) => b.z - a.z)[0];
  vwmStore.setState({
    wins,
    topZ,
    focusedId: focusTop?.id ?? null,
    closing: [],
    flying: [],
    seq: vwmStore.getState().seq + 1,
  });
  return { restored: wins.length, missing };
}
