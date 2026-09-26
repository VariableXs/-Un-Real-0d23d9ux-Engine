/**
 * F382 对话框位置记忆（H 域 · AI-H4）：
 * 用户拖动过的对话框记住位置（同类对话框下次出现在上次位置——「保存对话框在右下」
 * 是个人习惯）；未拖动过则屏幕居中偏上 1/3 处（视觉重心）；对话框不出现跨屏劈叉
 * （多屏时整体落在焦点屏）。
 * 判据（主册 F382）：居中偏上基准；拖动记忆（同类归组规则入册）；跨屏完整落屏判据；
 * 记忆容量上限与淘汰。
 * 存储键：variable:h4:f382:positions
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** 记忆容量上限（判据「记忆容量上限与淘汰」）——LRU 淘汰最久未用的组。 */
export const MEMORY_CAP = 20;

/** 同类归组规则（入册）：组 = 对话框种类（save/open/color/prompt…），同类共享位置记忆。 */
export type DialogGroup = "save" | "open" | "color" | "prompt" | string;

export interface StoredPosition {
  x: number;
  y: number;
  /** 记忆更新时刻（LRU 依据）。 */
  at: number;
}

const KEY = h4Key("f382", "positions");

type PositionBook = Record<string, StoredPosition>;

function isBook(v: unknown): v is PositionBook {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function load(store: KvStore): PositionBook {
  return readJson<PositionBook>(store, KEY, {}, isBook);
}

function save(store: KvStore, book: PositionBook): boolean {
  return writeJson(store, KEY, book);
}

/** 居中偏上 1/3 基准（判据）：y = 屏高 1/3 − 对话框半高（不低于 0）。 */
export function defaultPosition(screen: { w: number; h: number }, dlg: { w: number; h: number }): { x: number; y: number } {
  return {
    x: Math.max(0, Math.round((screen.w - dlg.w) / 2)),
    y: Math.max(0, Math.round(screen.h / 3 - dlg.h / 2)),
  };
}

/** 拖动落位记账（同类归组）：同组覆盖更新并刷新 LRU 时刻。 */
export function rememberPosition(group: DialogGroup, x: number, y: number, now: number, store: KvStore = defaultStore()): boolean {
  const book = load(store);
  book[group] = { x: Math.round(x), y: Math.round(y), at: now };
  const entries = Object.entries(book);
  if (entries.length > MEMORY_CAP) {
    entries.sort((a, b) => a[1].at - b[1].at);
    for (const [k] of entries.slice(0, entries.length - MEMORY_CAP)) delete book[k];
  }
  return save(store, book);
}

export interface PlacementResult {
  x: number;
  y: number;
  /** true = 来自记忆（用户拖过）；false = 居中偏上基准。 */
  fromMemory: boolean;
}

/** 下次出现位置：有记忆用记忆（同组归组规则），无记忆走基准。 */
export function placementFor(group: DialogGroup, screen: { w: number; h: number }, dlg: { w: number; h: number }, store: KvStore = defaultStore()): PlacementResult {
  const memo = load(store)[group];
  if (!memo) {
    const p = defaultPosition(screen, dlg);
    return { ...p, fromMemory: false };
  }
  return { x: memo.x, y: memo.y, fromMemory: true };
}

/** 跨屏完整落屏判据：位置钳进焦点屏工作区（整体落屏，不劈叉、不遮边）。 */
export function fitOnScreen(pos: { x: number; y: number }, dlg: { w: number; h: number }, screen: { x: number; y: number; w: number; h: number }): { x: number; y: number; clamped: boolean } {
  const x = Math.min(Math.max(pos.x, screen.x), Math.max(screen.x, screen.x + screen.w - dlg.w));
  const y = Math.min(Math.max(pos.y, screen.y), Math.max(screen.y, screen.y + screen.h - dlg.h));
  return { x, y, clamped: x !== pos.x || y !== pos.y };
}

/** 记忆容量淘汰审计：超 20 组时最久未用的被淘汰（判据）。 */
export function auditEviction(store: KvStore = defaultStore()): { size: number; oldestSurvivor: string | null } {
  const book = load(store);
  const entries = Object.entries(book).sort((a, b) => a[1].at - b[1].at);
  return { size: entries.length, oldestSurvivor: entries[0]?.[0] ?? null };
}
