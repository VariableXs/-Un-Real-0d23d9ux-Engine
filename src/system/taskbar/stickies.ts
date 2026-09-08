import { useSyncExternalStore } from "react";

/**
 * M-17 任务栏便签速贴（AI-03 任务栏与托盘组）：
 * - 桌面 shell 层轻组件，与四大空间无关（不是写作空间入口）
 * - 纯文本，四角堆叠、可拖动、10 分钟淡隐、点击钉住
 * - 数据 localStorage KV（轻量，不上 SQLite）；≤20 条上限
 * - 环境重启后钉住的恢复，未钉住的如实消失
 * 不做富文本、不做提醒时间、不与写作空间打通。
 */

const KEY = "variable:stickies:v1";
export const STICKY_MAX = 20;
export const STICKY_TTL_MS = 10 * 60 * 1000;
/** 淡隐动画时长（ms）。 */
export const STICKY_FADE_MS = 500;

export type StickyCorner = "tl" | "tr" | "bl" | "br";

export interface Sticky {
  id: number;
  text: string;
  pinned: boolean;
  corner: StickyCorner;
  createdAt: number;
  /** 淡隐开始时刻（null = 未开始）；钉住项无。 */
  fadeAt: number | null;
  /** 本会话正在淡出（动画期间保留渲染）。 */
  fading: boolean;
}

interface StickyState {
  items: Sticky[];
  inputOpen: boolean;
}

const listeners = new Set<() => void>();
let state: StickyState = { items: [], inputOpen: false };

function persist(): void {
  try {
    const pinned = state.items
      .filter((s) => s.pinned)
      .map((s) => ({ id: s.id, text: s.text, pinned: s.pinned, corner: s.corner, createdAt: s.createdAt }));
    localStorage.setItem(KEY, JSON.stringify(pinned));
  } catch {
    /* storage full/blocked — 内存态继续可用 */
  }
}

function emit(): void {
  state = { ...state, items: [...state.items] };
  for (const l of listeners) l();
}

export function subscribeStickies(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function useStickies(): StickyState {
  return useSyncExternalStore(subscribeStickies, () => state, () => ({ items: [], inputOpen: false }));
}

/** 重启恢复：只恢复钉住项。 */
export function restoreStickies(now: number = Date.now()): void {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "[]") as unknown;
    if (!Array.isArray(raw)) return;
    const restored: Sticky[] = raw
      .filter(
        (r): r is Omit<Sticky, "fadeAt" | "fading"> =>
          r !== null && typeof r === "object" && typeof (r as Sticky).text === "string" && (r as Sticky).pinned === true,
      )
      .slice(0, STICKY_MAX)
      .map((r) => ({ ...r, fadeAt: null, fading: false }));
    state = { ...state, items: restored };
    emit();
  } catch {
    /* corrupted → 空列表 */
  }
}

export function setInputOpen(open: boolean): void {
  state = { ...state, inputOpen: open };
  emit();
}

/** 新增便签；超上限如实拒绝（返回 null）。 */
export function addSticky(text: string, now: number = Date.now()): Sticky | null {
  const t = text.trim().slice(0, 500);
  if (!t) return null;
  if (state.items.length >= STICKY_MAX) return null;
  const s: Sticky = {
    id: now,
    text: t,
    pinned: false,
    corner: (["br", "tr", "bl", "tl"] as const)[state.items.length % 4],
    createdAt: now,
    fadeAt: now + STICKY_TTL_MS,
    fading: false,
  };
  state = { ...state, items: [...state.items, s] };
  emit();
  return s;
}

export function pinSticky(id: number): void {
  const hit = state.items.find((s) => s.id === id);
  if (!hit) return;
  patch(id, { pinned: !hit.pinned, fadeAt: !hit.pinned ? null : Date.now() + STICKY_TTL_MS, fading: false });
  persist();
}

export function patchSticky(id: number, p: Partial<Sticky>): void {
  patch(id, p);
}

function patch(id: number, p: Partial<Sticky>): void {
  state = { ...state, items: state.items.map((s) => (s.id === id ? { ...s, ...p } : s)) };
  emit();
}

export function moveSticky(id: number, corner: StickyCorner): void {
  patch(id, { corner });
  persist();
}

export function removeSticky(id: number): void {
  state = { ...state, items: state.items.filter((s) => s.id !== id) };
  emit();
  persist();
}

export function clearUnpinned(): void {
  state = { ...state, items: state.items.filter((s) => s.pinned) };
  emit();
  persist();
}

/** 过期推进：到时开始 500ms 淡隐，动画结束移除（钉住项永不淡隐）。 */
export function tickStickies(now: number = Date.now()): void {
  let changed = false;
  for (const s of state.items) {
    if (s.pinned || s.fading || s.fadeAt === null) continue;
    if (now >= s.fadeAt) {
      if (now >= s.fadeAt + STICKY_FADE_MS) {
        state = { ...state, items: state.items.filter((x) => x.id !== s.id) };
      } else {
        state = { ...state, items: state.items.map((x) => (x.id === s.id ? { ...x, fading: true } : x)) };
      }
      changed = true;
    }
  }
  if (changed) emit();
}
