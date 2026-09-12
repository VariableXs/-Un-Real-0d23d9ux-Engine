/**
 * AURORA-10000 · AI-11~AI-15 车道 · 目录聚合（领域03 全 25 族 / 625 项）。
 * ID 校验：F01251~F01875 唯一、连续、无跳号（__tests__ 兜底，
 * 波次出口另跑 tools/check-aurora 校验全景图本体）。
 */
import { AI11_FAMILIES } from "./ai11-icons";
import { AI12_FAMILIES } from "./ai12-wallpaper";
import { AI13_FAMILIES } from "./ai13-widgets";
import { AI14_FAMILIES } from "./ai14-visual";
import { AI15_FAMILIES } from "./ai15-personality";
import type { DesignEntry, DesignFamily } from "./types";

export const LANE = {
  ai: "AI-11~AI-15",
  wave: "W2",
  domain: "领域03 桌面设计·桌面与图标",
  range: [1251, 1875] as [number, number],
};

export const FAMILIES: DesignFamily[] = [
  ...AI11_FAMILIES,
  ...AI12_FAMILIES,
  ...AI13_FAMILIES,
  ...AI14_FAMILIES,
  ...AI15_FAMILIES,
];

export const ALL_ENTRIES: DesignEntry[] = FAMILIES.flatMap((f) => f.entries);

export function findEntry(id: string): DesignEntry | null {
  return ALL_ENTRIES.find((e) => e.id === id) ?? null;
}

export function familyOf(entryId: string): DesignFamily | null {
  const ent = findEntry(entryId);
  return ent ? FAMILIES.find((f) => f.id === ent.family) ?? null : null;
}
