/**
 * H4 快速设置磁贴——纯模型（v9 · 深化批次九）：
 * F387 判据点名「快速设置磁贴+快捷键双入口」——本件补齐快速设置面的 H4 磁贴：
 * 灰度模式磁贴（真引擎互斥 toggle）+ 创造者工具两枚呼出钮（拾色器/标尺——v3 浮层通道复用）。
 * 磁贴状态零自研：pressed = 引擎滤镜位的投影；toggle = f387.requestFilter 原生语义。
 */

import * as grayscale from "../../system/h4/f387-grayscaleMode";
import type { KvStore } from "../../system/h4/internal/store";
import { announceFilterChanged } from "./h4ui";

export type QuickTileId = "grayscale" | "picker" | "ruler";

export interface QuickTileModel {
  id: QuickTileId;
  label: string;
  /** 按压态（灰度 = 滤镜位命中；呼出钮恒 false——按钮不是开关，不假装）。 */
  pressed: boolean;
  /** 呼出事件（v10 通道唯一化：走 h4Bus 契约事件——settings.* 域已登记）。 */
  summon: "settings.summon-picker" | "settings.summon-ruler" | null;
}

/** 磁贴模型（快速设置面板的渲染契约——渲染壳零自研逻辑）。 */
export function buildQuickTiles(filter: grayscale.ColorFilter): QuickTileModel[] {
  return [
    { id: "grayscale", label: "灰度模式", pressed: filter === "grayscale", summon: null },
    { id: "picker", label: "屏幕拾色器", pressed: false, summon: "settings.summon-picker" },
    { id: "ruler", label: "像素标尺", pressed: false, summon: "settings.summon-ruler" },
  ];
}

/**
 * 灰度 toggle（F387 互斥单点的用户动作面）：
 * 灰度开 → 关；其他滤镜在位 → 切到灰度（互斥替换）；无滤镜 → 开。
 * 变更后走 announceFilterChanged（v9 起唯一通道 = h4Bus 契约事件）。
 */
export function toggleGrayscale(store: KvStore): { now: grayscale.ColorFilter; replaced: grayscale.ColorFilter } {
  const current = grayscale.activeFilter(store);
  const want = current === "grayscale" ? "none" : "grayscale";
  const r = grayscale.requestFilter(store, want);
  announceFilterChanged(r.now);
  return r;
}