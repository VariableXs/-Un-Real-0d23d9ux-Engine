/**
 * AI-18 M-72 环境氛围会话恢复 — 纯逻辑：快照结构 + 校验 + 本地 KV 存取。
 *
 * 口径：
 * - 快照 = 壁纸状态 + 音量 + DND + 开着的窗口集合与几何（登记应用 + 几何；环境内窗口记路由参数）；
 * - 恢复是提示而非自动（绝不偷跑）；
 * - 快照可关：关闭后零残留（清键）。
 * - 与 Z-40 布局快照互补：那是用户主动存档，这是自动氛围级。
 */

export interface SessionWindowEntry {
  /** 登记应用 id（或环境内窗口路由名）。 */
  appId: string;
  /** 几何（虚拟桌面坐标；环境内窗口可为 null）。 */
  rect: { x: number; y: number; w: number; h: number } | null;
  /** 环境内四空间窗口的路由参数（外部窗口为 null）。 */
  route?: string | null;
}

export interface AmbientSnapshot {
  format: "ai18-session";
  version: 1;
  savedAt: number;
  wallpaper: string;
  volume: number;
  dnd: boolean;
  windows: SessionWindowEntry[];
}

export const SNAPSHOT_KEY = "ai18.ambientSnapshot";

function isRect(v: unknown): v is SessionWindowEntry["rect"] {
  if (typeof v !== "object" || v === null) return false;
  const r = v as Record<string, unknown>;
  return [r.x, r.y, r.w, r.h].every((n) => typeof n === "number" && Number.isFinite(n));
}

/** 校验快照（坏值/null 一律拒绝 —— 绝不恢复脏数据）。 */
export function parseSnapshot(raw: unknown): AmbientSnapshot | null {
  if (typeof raw !== "object" || raw === null) return null;
  const s = raw as Record<string, unknown>;
  if (s.format !== "ai18-session" || s.version !== 1) return null;
  if (typeof s.wallpaper !== "string" || typeof s.volume !== "number" || typeof s.dnd !== "boolean") return null;
  if (!Array.isArray(s.windows)) return null;
  const windows: SessionWindowEntry[] = [];
  for (const w of s.windows) {
    if (typeof w !== "object" || w === null) continue;
    const e = w as Record<string, unknown>;
    if (typeof e.appId !== "string") continue;
    windows.push({
      appId: e.appId,
      rect: isRect(e.rect) ? e.rect : null,
      route: typeof e.route === "string" ? e.route : null,
    });
  }
  return {
    format: "ai18-session",
    version: 1,
    savedAt: typeof s.savedAt === "number" ? s.savedAt : 0,
    wallpaper: s.wallpaper,
    volume: Math.max(0, Math.min(1, s.volume)),
    dnd: s.dnd,
    windows: windows.slice(0, 64),
  };
}

/** 保存（localStorage KV；异常静默 —— 快照失败不打扰）。 */
export function saveSnapshot(snap: AmbientSnapshot): void {
  try {
    localStorage.setItem(SNAPSHOT_KEY, JSON.stringify(snap));
  } catch { /* quota / privacy mode：忽略 */ }
}

/** 读取。 */
export function loadSnapshot(): AmbientSnapshot | null {
  try {
    const raw = localStorage.getItem(SNAPSHOT_KEY);
    if (!raw) return null;
    return parseSnapshot(JSON.parse(raw));
  } catch {
    return null;
  }
}

/** 零残留：快照关闭 / 用户拒绝恢复时清键。 */
export function clearSnapshot(): void {
  try {
    localStorage.removeItem(SNAPSHOT_KEY);
  } catch { /* noop */ }
}

/** 快照是否过旧（>7 天不再提示恢复，氛围快照不是档案）。 */
export function snapshotFresh(snap: AmbientSnapshot, now = Date.now()): boolean {
  return now - snap.savedAt < 7 * 86_400_000;
}
