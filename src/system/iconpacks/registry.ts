import { useSyncExternalStore } from "react";

/**
 * N-12 图标包注册表（主控预置的共享基础设施）：
 * - 优先级：用户包 > 内置语言 > 系统原生提取（调用方先问这里，拿不到再走原生）
 * - `.vicon` = JSON manifest（内联 SVG 字符串或 PNG data URL；未用 zip 以避免新增依赖，
 *   诚实边界：与规格的 zip 容器差异已在工坊 UI 标注）
 * - 缺键回退：getIconOverride 返回 null 即回退原生；每次回退计数（fallbackCount）
 * - 卸载即全部还原（包数据整体删除，无残留）
 */

export interface IconPackMeta {
  id: string;
  name: string;
  version: string;
  /** 四空间/区域变体：键形如 "desktop:sys-recycle"、"start:app-write"、"file:.md" */
  icons: Record<string, string>;
  installedAt: number;
}

const LS_KEY = "variable:iconpacks:v1";

let cache: IconPackMeta[] | null = null;
let fallbackCount = 0;
const listeners = new Set<() => void>();

function load(): IconPackMeta[] {
  if (cache) return cache;
  try {
    const raw = localStorage.getItem(LS_KEY);
    cache = raw ? (JSON.parse(raw) as IconPackMeta[]) : [];
  } catch {
    cache = [];
  }
  return cache;
}

function persist(packs: IconPackMeta[]): void {
  cache = packs;
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(packs));
  } catch {
    /* storage full — 内存态继续可用 */
  }
  bump(); // 修复：版本号随持久化递增，否则 useIconPackVersion 快照不变、消费方不重渲染
  for (const l of listeners) l();
}

/** 取当前生效的包（单包模型：后装覆盖先装；卸载即无包）。 */
export function activePack(): IconPackMeta | null {
  const packs = load();
  return packs.length > 0 ? packs[packs.length - 1] ?? null : null;
}

/** 图标键查询：命中返回资源（SVG 字符串或 data URL），未命中 null（调用方回退原生并计数）。 */
export function getIconOverride(key: string): string | null {
  const pack = activePack();
  if (!pack) return null;
  const hit = pack.icons[key];
  if (hit === undefined) {
    fallbackCount++;
    return null;
  }
  return hit;
}

/** 本次会话缺键回退计数（回退纪律验收口径）。 */
export function getFallbackCount(): number {
  return fallbackCount;
}

export function listPacks(): IconPackMeta[] {
  return [...load()];
}

export function installPack(meta: Omit<IconPackMeta, "installedAt">): IconPackMeta {
  const full: IconPackMeta = { ...meta, installedAt: Date.now() };
  persist([...load().filter((p) => p.id !== full.id), full]);
  return full;
}

export function uninstallPack(id: string): void {
  persist(load().filter((p) => p.id !== id));
}

const SUBSCRIBE_KEY = "variable:iconpacks:version";
function bump(): void {
  try {
    localStorage.setItem(SUBSCRIBE_KEY, String(Date.now()));
  } catch {
    /* ignore */
  }
}

export function useIconPackVersion(): number {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => {
      try {
        return Number(localStorage.getItem(SUBSCRIBE_KEY) ?? "0");
      } catch {
        return 0;
      }
    },
    () => 0,
  );
}
