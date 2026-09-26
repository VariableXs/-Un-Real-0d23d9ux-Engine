/**
 * F158/F168 布局引擎深化 · 开始菜单布局计算器 + 任务栏布局计算器。
 *
 * 主册判据延伸：
 * - F158【设计细节】预设卡缩略图实时渲染（真缩小版）——布局计算器为缩略渲染
 *   提供确定性几何。
 * - F168【设计细节】大图标档任务栏增高 56px、左对齐锚点切换、托盘折叠——
 *   布局计算器输出完整几何（图标位/托盘折叠线/开始钮位），即时重排 200ms 内完成。
 */

import { defaultStartLayout, type StartMenuLayout, type StartPreset } from "./startpresets";
import { ICON_SIZE_PX, HEIGHT_PX, type TaskbarPrefs } from "./taskbarprefs";

// ---------- 开始菜单布局计算器 ----------

export interface StartMenuGeometry {
  width: number;
  height: number;
  tilePx: number;
  tileColumns: number;
  pinnedRows: number;
  /** 最近/推荐区是否渲染。 */
  showRecent: boolean;
  showRecommended: boolean;
  /** 长辈模式达标（tile ≥56px）。 */
  elderOk: boolean;
}

const START_W = 560;
const START_H_STANDARD = 640;
const START_H_FULLSCREEN_RATIO = 0.86;
const TILE_STANDARD = 44;
const TILE_ELDER = 60;
const TILE_GAP = 8;

/** 布局 → 几何（纯函数；fullscreen 用屏高比率，长辈档放大格子）。 */
export function startMenuGeometry(layout: StartMenuLayout, screenW: number, screenH: number): StartMenuGeometry {
  const tilePx = layout.largeIcons ? TILE_ELDER : TILE_STANDARD;
  if (layout.fullscreen) {
    const w = Math.round(screenW * 0.62);
    const h = Math.round(screenH * START_H_FULLSCREEN_RATIO);
    const columns = Math.max(4, Math.floor((w - 48) / (tilePx + TILE_GAP)));
    return {
      width: w,
      height: h,
      tilePx,
      tileColumns: columns,
      pinnedRows: Math.ceil(Math.max(1, layout.pinned.length) / columns),
      showRecent: layout.regions.recent ?? true,
      showRecommended: layout.regions.recommended ?? true,
      elderOk: tilePx >= 56,
    };
  }
  const columns = Math.max(4, Math.floor((START_W - 48) / (tilePx + TILE_GAP)));
  return {
    width: START_W,
    height: START_H_STANDARD,
    tilePx,
    tileColumns: columns,
    pinnedRows: Math.ceil(Math.max(1, layout.pinned.length) / columns),
    showRecent: layout.regions.recent ?? true,
    showRecommended: layout.regions.recommended ?? true,
    elderOk: tilePx >= 56,
  };
}

/** 预设缩略渲染规格（真缩小版——渲染器按几何 × 缩放系数出图，非贴图）。 */
export interface PresetThumbSpec {
  scale: number;
  geometry: StartMenuGeometry;
}

export function presetThumbSpec(preset: StartPreset, screenW: number, screenH: number, thumbWidthPx = 120): PresetThumbSpec {
  const layout = applyPresetDiff(defaultStartLayout(), preset);
  const geometry = startMenuGeometry(layout, screenW, screenH);
  return { scale: Math.min(1, thumbWidthPx / geometry.width), geometry };
}

function applyPresetDiff(base: StartMenuLayout, preset: StartPreset): StartMenuLayout {
  return {
    pinned: preset.diff.pinned ?? base.pinned,
    regions: { ...base.regions, ...(preset.diff.regions ?? {}) },
    fullscreen: preset.diff.fullscreen ?? base.fullscreen,
    largeIcons: preset.diff.largeIcons ?? base.largeIcons,
  };
}

// ---------- 任务栏布局计算器 ----------

export interface TaskbarItem {
  id: string;
  /** 是否固定（固定项不折叠）。 */
  pinned: boolean;
}

export interface TaskbarGeometry {
  heightPx: number;
  iconPx: number;
  /** 开始钮宽（图标 + 内边距）。 */
  startButtonPx: number;
  /** 图标区起点 x（左对齐=紧随开始钮；居中=计算居中偏移）。 */
  iconsOriginX: number;
  /** 溢出而折叠进「^」展开器的项（pinned 永不折叠）。 */
  foldedIds: string[];
  /** 可见图标位（x 坐标逐项）。 */
  visibleX: Record<string, number>;
  /** 托盘区起点 x（从右缘向左排）。 */
  trayOriginX: number;
}

const START_BUTTON_PX = 48;
const ICON_GAP = 6;
const TRAY_WIDTH = 192;

/** 任务栏完整几何（纯函数——重排 200ms 预算内的确定性计算）。 */
export function taskbarGeometry(prefs: TaskbarPrefs, items: TaskbarItem[], screenW: number, trayIconCount: number): TaskbarGeometry {
  const heightPx = HEIGHT_PX[prefs.iconSize];
  const iconPx = ICON_SIZE_PX[prefs.iconSize];
  const maxIcons = Math.max(1, Math.floor((screenW - START_BUTTON_PX - TRAY_WIDTH) / (iconPx + ICON_GAP)));

  // 折叠： pinned 永不折叠；超出容量的非固定项进展开器。
  const pinned = items.filter((i) => i.pinned);
  const unpinned = items.filter((i) => !i.pinned);
  const visible = [...pinned, ...unpinned].slice(0, maxIcons);
  const visibleIds = new Set(visible.map((i) => i.id));
  const foldedIds = items.filter((i) => !visibleIds.has(i.id)).map((i) => i.id);

  const usedWidth = visible.length * (iconPx + ICON_GAP);
  let iconsOriginX: number;
  if (prefs.align === "left") {
    iconsOriginX = START_BUTTON_PX;
  } else {
    const contentStart = START_BUTTON_PX;
    const contentEnd = screenW - TRAY_WIDTH;
    iconsOriginX = Math.round(contentStart + (contentEnd - contentStart - usedWidth) / 2);
  }
  const visibleX: Record<string, number> = {};
  visible.forEach((item, idx) => {
    visibleX[item.id] = iconsOriginX + idx * (iconPx + ICON_GAP);
  });

  // 托盘折叠联动（F075）：数量超阈值折叠为展开器，托盘宽度按折叠态收缩。
  const trayFolded = trayIconCount > trayFoldThreshold(prefs);
  const trayWidth = trayFolded ? TRAY_WIDTH * 0.6 : TRAY_WIDTH;
  return {
    heightPx,
    iconPx,
    startButtonPx: START_BUTTON_PX,
    iconsOriginX,
    foldedIds,
    visibleX,
    trayOriginX: screenW - Math.round(trayWidth),
  };
}

function trayFoldThreshold(prefs: TaskbarPrefs): number {
  return prefs.iconSize === "large" ? Math.max(3, Math.floor(prefs.trayFoldThreshold * 0.75)) : prefs.trayFoldThreshold;
}

/** 重排动画预算校验（200ms 内完成——计算耗时由调用方实测入账）。 */
export const REARRANGE_BUDGET_MS = 200;

// ---------- 自动隐藏状态机（F168 深化——五态） ----------

export type AutoHideState = "shown" | "hiding" | "hidden" | "revealing" | "pinned-by-focus";

export interface AutoHideInput {
  state: AutoHideState;
  /** 光标距屏幕底缘像素。 */
  cursorYFromBottom: number;
  /** 光标在热区停留毫秒。 */
  dwellMs: number;
  /** 有窗口获得任务栏相关焦点（开始菜单/托盘弹层）→ 钉住。 */
  focusPinned: boolean;
  fullscreenActive: boolean;
  autoHidePref: boolean;
  /** 距离上次状态变更毫秒（迟滞去抖）。 */
  sinceTransitionMs: number;
}

export const HIDE_AFTER_MS = 400; // 光标离开后延迟隐藏（防手滑）
export const REVEAL_HOLD_MS = 120; // 唤出保持迟滞

/** 自动隐藏下一态判定（纯函数状态机——每帧调用）。 */
export function autoHideNext(input: AutoHideState, i: Omit<AutoHideInput, "state">): AutoHideState {
  if (!i.autoHidePref || i.fullscreenActive) return "shown";
  if (i.focusPinned) return "pinned-by-focus";
  const inHotzone = i.cursorYFromBottom <= 6 && i.dwellMs >= 200;
  const nearEdge = i.cursorYFromBottom <= 24;
  switch (input) {
    case "shown":
      return nearEdge ? input : (i.sinceTransitionMs >= HIDE_AFTER_MS ? "hiding" : input);
    case "hiding":
      return nearEdge ? "shown" : "hidden";
    case "hidden":
      return inHotzone ? "revealing" : input;
    case "revealing":
      return nearEdge || i.dwellMs < REVEAL_HOLD_MS ? input : "hidden";
    case "pinned-by-focus":
      return "shown";
    default:
      return input;
  }
}
