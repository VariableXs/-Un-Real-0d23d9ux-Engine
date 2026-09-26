/**
 * F158 开始菜单布局预设 · 完整设计。
 *
 * 主册判据：三预设切换结果与定义一致（逐项对拍）；自定义保存-切换-还原闭环；
 * 长辈模式实测（大图标可点面积达标）。
 *
 * 【功能定义】开始菜单三官方预设（效率=固定常用+最近多用/简洁=仅固定六枚/
 * 全屏=全屏模式默认）+ 用户自定义布局存第四套；一键切换即时生效。
 *
 * 【状态与异常】预设引用的应用已卸载 → 该项跳过+清单标注；保存自定义与官方同名
 * → 自动加「（自定义）」；切换到全屏预设 → 分辨率不适配时警告。
 *
 * 【设计细节】预设=「差异集」存储（只记与默认的差异——官方更新不冲掉用户改动）；
 * 切换不丢最近使用数据（预设只改布局不改数据）；全屏预设与 F071 搜索共存验证。
 */

import { personaStore } from "./store";

export const SECTION = "startmenu";
export const REARRANGE_MS = 200; // F124 曲线
export const ELDER_MIN_TOUCH_PX = 56; // 长辈模式可点面积达标线
export const SIMPLE_FIXED_COUNT = 6; // 「简洁=仅固定六枚」

export type StartMenuRegion = "pinned" | "recent" | "recommended" | "allApps";

export interface StartMenuLayout {
  /** 固定区应用 id 顺序（简洁预设=六枚）。 */
  pinned: string[];
  /** 区开关。 */
  regions: Record<StartMenuRegion, boolean>;
  /** 全屏态。 */
  fullscreen: boolean;
  /** 大图标（长辈模式）。 */
  largeIcons: boolean;
}

/** 差异集：只记与默认的差异（undefined = 与默认一致）。 */
export interface LayoutDiff {
  pinned?: string[];
  regions?: Partial<Record<StartMenuRegion, boolean>>;
  fullscreen?: boolean;
  largeIcons?: boolean;
}

export interface StartPreset {
  id: string;
  name: string;
  official: boolean;
  diff: LayoutDiff;
}

export const START_REGION_DEFAULTS: Record<StartMenuRegion, boolean> = {
  pinned: true,
  recent: true,
  recommended: true,
  allApps: true,
};

/** 默认布局（全开、固定区常用八枚占位——实际项由 F072 最近使用引擎供给）。 */
export function defaultStartLayout(): StartMenuLayout {
  return { pinned: [], regions: { ...START_REGION_DEFAULTS }, fullscreen: false, largeIcons: false };
}

export function applyDiff(base: StartMenuLayout, diff: LayoutDiff): StartMenuLayout {
  return {
    pinned: diff.pinned ? [...diff.pinned] : [...base.pinned],
    regions: { ...base.regions, ...(diff.regions ?? {}) },
    fullscreen: diff.fullscreen ?? base.fullscreen,
    largeIcons: diff.largeIcons ?? base.largeIcons,
  };
}

// ---------- 三官方预设（主册【功能定义】） ----------

export function officialPresets(): StartPreset[] {
  return [
    {
      id: "preset-efficiency",
      name: "效率",
      official: true,
      diff: {
        pinned: ["common-1", "common-2", "common-3", "common-4", "common-5", "common-6", "common-7", "common-8"],
        regions: { pinned: true, recent: true, recommended: true, allApps: true },
        fullscreen: false,
        largeIcons: false,
      },
    },
    {
      id: "preset-simple",
      name: "简洁",
      official: true,
      diff: {
        pinned: ["common-1", "common-2", "common-3", "common-4", "common-5", "common-6"],
        regions: { pinned: true, recent: false, recommended: false, allApps: true },
        fullscreen: false,
        largeIcons: true,
      },
    },
    {
      id: "preset-fullscreen",
      name: "全屏",
      official: true,
      diff: {
        regions: { pinned: true, recent: true, recommended: true, allApps: true },
        fullscreen: true,
        largeIcons: false,
      },
    },
  ];
}

// ---------- 配置持久化 ----------

export interface StartPresetConfig {
  presets: StartPreset[]; // 官方三 + 自定义（第四套起）
  activeId: string;
}

export function defaultStartPresetConfig(): StartPresetConfig {
  return { presets: officialPresets(), activeId: "preset-efficiency" };
}

export function loadStartPresetConfig(): StartPresetConfig {
  const stored = personaStore.getWith(SECTION, "presets", undefined) as Partial<StartPresetConfig> | undefined;
  if (!stored) return defaultStartPresetConfig();
  const d = defaultStartPresetConfig();
  return {
    presets: Array.isArray(stored.presets) && stored.presets.length > 0 ? stored.presets : d.presets,
    activeId: typeof stored.activeId === "string" ? stored.activeId : d.activeId,
  };
}

export function saveStartPresetConfig(c: StartPresetConfig): void {
  personaStore.set(SECTION, { presets: c });
}

/** 保存自定义：与官方同名 → 自动加「（自定义）」。 */
export function saveAsCustom(config: StartPresetConfig, name: string, layout: StartMenuLayout): { config: StartPresetConfig; preset: StartPreset } {
  const officialNames = new Set(officialPresets().map((p) => p.name));
  const finalName = officialNames.has(name) ? `${name}（自定义）` : name;
  const preset: StartPreset = {
    id: `preset-custom-${Date.now().toString(36)}`,
    name: finalName,
    official: false,
    diff: diffFromLayout(defaultStartLayout(), layout),
  };
  return { config: { ...config, presets: [...config.presets, preset] }, preset };
}

/** 从布局反推差异集（与默认布局逐字段比较）。 */
export function diffFromLayout(base: StartMenuLayout, layout: StartMenuLayout): LayoutDiff {
  const diff: LayoutDiff = {};
  if (JSON.stringify(base.pinned) !== JSON.stringify(layout.pinned)) diff.pinned = [...layout.pinned];
  const regionDiff: Partial<Record<StartMenuRegion, boolean>> = {};
  for (const k of Object.keys(base.regions) as StartMenuRegion[]) {
    if (base.regions[k] !== layout.regions[k]) regionDiff[k] = layout.regions[k];
  }
  if (Object.keys(regionDiff).length > 0) diff.regions = regionDiff;
  if (base.fullscreen !== layout.fullscreen) diff.fullscreen = layout.fullscreen;
  if (base.largeIcons !== layout.largeIcons) diff.largeIcons = layout.largeIcons;
  return diff;
}

/** 切换预设：返回新布局；卸载应用项跳过+标注（installedApps 由调用方供给）。 */
export function applyPreset(preset: StartPreset, installedApps: Set<string>): { layout: StartMenuLayout; skipped: string[] } {
  const base = defaultStartLayout();
  const layout = applyDiff(base, preset.diff);
  const skipped: string[] = [];
  if (preset.diff.pinned) {
    layout.pinned = preset.diff.pinned.filter((id) => {
      if (installedApps.has(id)) return true;
      skipped.push(id);
      return false;
    });
  }
  return { layout, skipped };
}

/** 全屏预设分辨率适配警告：高度 <768px 不适合全屏模式。 */
export function fullscreenFitWarning(screenH: number): string | null {
  return screenH < 768 ? "当前分辨率高度不足 768px，全屏开始菜单可用空间较小" : null;
}

/** 长辈模式可点面积达标判定（大图标档格子 ≥56px 判据）。 */
export function elderTouchOk(tilePx: number): boolean {
  return tilePx >= ELDER_MIN_TOUCH_PX;
}
