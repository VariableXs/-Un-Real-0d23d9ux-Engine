import { useSyncExternalStore } from "react";

/**
 * 化境 V-13（车道 S）：固定应用文件夹。
 * - Folder { id, name, items[] }，存 localStorage（variable:start:folders:v1）
 * - 上限 24 项（规格 V-13：如实提示，绝不静默丢弃）
 * - order 数组中文件夹以 fd-<id> 作为一个条目参与手动排序
 * - 纯合并/上限判定抽成 mergeFolderItems / folderAddItem 等纯函数便于测试；
 *   localStorage 不可用时退化为内存态（测试环境与 storage 满场景）。
 */

export interface Folder {
  id: string;
  name: string;
  /** 成员 = 开始菜单网格项 id（app-* / tp-* / sys-* / tool-* / fd-*）。 */
  items: string[];
}

/** 文件夹内最多 24 项（规格 V-13）。 */
export const FOLDER_MAX_ITEMS = 24;

const KEY = "variable:start:folders:v1";
const GRID_PREFIX = "fd-";

/** 网格项 id → 文件夹 id。 */
export function folderGridId(folderId: string): string {
  return `${GRID_PREFIX}${folderId}`;
}

/** 网格项 id 是否是文件夹条目。 */
export function isFolderGridId(id: string): boolean {
  return id.startsWith(GRID_PREFIX);
}

/** 网格项 id → 文件夹 id（非文件夹 id 返回 null）。 */
export function folderIdOfGrid(id: string): string | null {
  return isFolderGridId(id) ? id.slice(GRID_PREFIX.length) : null;
}

let cache: Folder[] | null = null;
const listeners = new Set<() => void>();

function load(): Folder[] {
  if (cache) return cache;
  try {
    const raw = localStorage.getItem(KEY);
    const parsed = raw ? (JSON.parse(raw) as unknown) : [];
    cache = Array.isArray(parsed)
      ? parsed.filter(
          (f): f is Folder =>
            typeof f === "object" && f !== null &&
            typeof (f as Folder).id === "string" &&
            typeof (f as Folder).name === "string" &&
            Array.isArray((f as Folder).items),
        )
      : [];
  } catch {
    cache = [];
  }
  return cache;
}

function persist(list: Folder[]): void {
  cache = list;
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    /* storage full — 内存态继续可用 */
  }
  for (const l of listeners) l();
}

export function getFolders(): Folder[] {
  return load();
}

export function saveFolders(list: Folder[]): void {
  persist(list);
}

export function useFolders(): Folder[] {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => {
        listeners.delete(cb);
      };
    },
    () => load(),
    () => [] as Folder[],
  );
}

function newId(): string {
  return `f${Date.now().toString(36)}${Math.floor(Math.random() * 1e4).toString(36)}`;
}

/** 新建文件夹（调用方保证 items 非空；超过上限由 mergeFolderItems 拦截）。 */
export function createFolder(name: string, items: string[]): Folder {
  const f: Folder = { id: newId(), name, items: [...new Set(items)] };
  persist([...load(), f]);
  return f;
}

export type AddResult = "ok" | "full" | "missing" | "duplicate";

/** 向文件夹追加一个网格项（24 上限如实返回 "full"，不静默丢弃）。 */
export function folderAddItem(folderId: string, itemId: string): AddResult {
  const folders = load();
  const f = folders.find((x) => x.id === folderId);
  if (!f) return "missing";
  if (f.items.includes(itemId)) return "duplicate";
  if (f.items.length >= FOLDER_MAX_ITEMS) return "full";
  persist(folders.map((x) => (x.id === folderId ? { ...x, items: [...x.items, itemId] } : x)));
  return "ok";
}

/** 从文件夹移出一个成员；清空即自动解散（不留空壳）。 */
export function folderRemoveItem(folderId: string, itemId: string): Folder[] {
  const folders = load();
  const f = folders.find((x) => x.id === folderId);
  if (!f) return folders;
  const rest = f.items.filter((i) => i !== itemId);
  if (rest.length === 0) {
    disbandFolder(folderId); // 清空即解散（成员释放回网格）
    return load();
  }
  persist(folders.map((x) => (x.id === folderId ? { ...x, items: rest } : x)));
  return load();
}

/** 重命名。 */
export function renameFolder(folderId: string, name: string): void {
  const trimmed = name.trim();
  if (!trimmed) return;
  persist(load().map((f) => (f.id === folderId ? { ...f, name: trimmed } : f)));
}

/** 解散：返回释放回网格的成员 id（移除 fd- 前缀的成员也原样返回）。 */
export function disbandFolder(folderId: string): string[] {
  const folders = load();
  const f = folders.find((x) => x.id === folderId);
  if (!f) return [];
  persist(folders.filter((x) => x.id !== folderId));
  return [...f.items];
}

/**
 * 合并判定（纯函数，规格 V-13 与 VWM 贴靠同语义的 60% 重叠由 UI 层换算成
 * 「落点在目标图标中央 60% 区域」）：合并后超上限 → ok:false，绝不静默丢弃。
 * 去重保持先序（a 的成员在前）。
 */
export function mergeFolderItems(a: string[], b: string[]): { items: string[]; ok: boolean } {
  const merged = [...new Set([...a, ...b])];
  return { items: merged, ok: merged.length <= FOLDER_MAX_ITEMS };
}
