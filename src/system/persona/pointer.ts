/**
 * F156 指针编辑器 · 纯逻辑层（编辑器 UI 见 pages-asset）。
 *
 * 主册判据：15 枚指针全可编辑导出；热点生效精确（1px 级取放对拍）；
 * 双倍率 sprite 生成清晰（4K 放大验证）。
 *
 * 【功能定义】指针方案定制编辑器：热点校准（像素级）/尺寸缩放（0.5x-2x）/
 * 动效开关（沙漏旋转等动画指针）/导入第三方指针包（F133 规范子集）；
 * 4K 双倍率 sprite 自动生成。
 *
 * 【状态与异常】热点未设 → 默认 (0,0)+黄色提醒；尺寸过大（>128px）→ 警告遮挡；
 * 动效帧率 >60 → 降采样提示。
 *
 * 【设计细节】热点标定十字线放大 8x；sprite 生成含 1x/2x 双套；动效指针帧序列
 * 上限 16 帧；预览区背景三色切换（深浅中灰）。
 */

import { personaStore } from "./store";

export const SECTION = "pointer";

/** 15 枚标准指针（乙语义全保留——主册【交互设计】）。 */
export const POINTER_ROLES: readonly { id: string; zh: string; en: string; animated: boolean }[] = [
  { id: "arrow", zh: "箭头", en: "Arrow", animated: false },
  { id: "hand", zh: "手型", en: "Hand", animated: false },
  { id: "text", zh: "文本", en: "Text", animated: false },
  { id: "busy", zh: "忙碌（沙漏）", en: "Busy", animated: true },
  { id: "busy-arrow", zh: "后台忙碌", en: "Busy arrow", animated: true },
  { id: "cross", zh: "十字", en: "Cross", animated: false },
  { id: "move", zh: "移动", en: "Move", animated: false },
  { id: "resize-ns", zh: "纵向缩放", en: "Resize NS", animated: false },
  { id: "resize-ew", zh: "横向缩放", en: "Resize EW", animated: false },
  { id: "resize-nesw", zh: "对角缩放 ↗↙", en: "Resize NE-SW", animated: false },
  { id: "resize-nwse", zh: "对角缩放 ↖↘", en: "Resize NW-SE", animated: false },
  { id: "zoom-in", zh: "放大", en: "Zoom in", animated: false },
  { id: "zoom-out", zh: "缩小", en: "Zoom out", animated: false },
  { id: "unavailable", zh: "不可用", en: "Unavailable", animated: false },
  { id: "link", zh: "链接选择", en: "Link select", animated: false },
] as const;

export const POINTER_ROLE_COUNT = 15;

/** 编辑器画布（主册【交互设计】：编辑器 720×480px）。 */
export const EDITOR_W = 720;
export const EDITOR_H = 480;
export const HOTSPOT_ZOOM = 8;
export const SIZE_LIMIT_PX = 128;
export const ANIM_MAX_FRAMES = 16;
export const ANIM_MAX_FPS = 60;

export interface PointerRoleAsset {
  role: string;
  /** 1x 基准尺寸（px）。 */
  size: number;
  /** 热点（1x 基准坐标）；null=未设（运行时按 (0,0) 并黄条提醒）。 */
  hotspot: { x: number; y: number } | null;
  /** 动效帧（SVG dataURL 序列），静态指针为单帧。 */
  frames: string[];
  /** 帧率（动效指针）。 */
  fps: number;
  /** 动效开关（默认开——沙漏旋转等）。 */
  animated: boolean;
}

export interface PointerScheme {
  name: string;
  scale: number; // 0.5x-2x
  roles: Record<string, PointerRoleAsset>;
  version: number;
}

export function defaultPointerScheme(): PointerScheme {
  const roles: Record<string, PointerRoleAsset> = {};
  for (const r of POINTER_ROLES) {
    roles[r.id] = {
      role: r.id,
      size: r.id === "arrow" ? 24 : 32,
      hotspot: null,
      frames: [],
      fps: r.animated ? 24 : 0,
      animated: r.animated,
    };
  }
  return { name: "我的指针方案", scale: 1, roles, version: 1 };
}

export function loadPointerScheme(): PointerScheme {
  const stored = personaStore.getWith(SECTION, "scheme", undefined) as Partial<PointerScheme> | undefined;
  if (!stored) return defaultPointerScheme();
  const d = defaultPointerScheme();
  return {
    name: typeof stored.name === "string" ? stored.name : d.name,
    scale: typeof stored.scale === "number" ? Math.min(2, Math.max(0.5, stored.scale)) : 1,
    roles: { ...d.roles, ...(typeof stored.roles === "object" && stored.roles !== null ? stored.roles : {}) },
    version: 1,
  };
}

export function savePointerScheme(s: PointerScheme): void {
  personaStore.set(SECTION, { scheme: s });
}

// ---------- 校验与警告（主册【状态与异常】三则） ----------

export interface PointerIssue {
  role: string;
  kind: "hotspot-unset" | "size-overflow" | "fps-oversample" | "frames-overflow" | "empty";
  level: "warning" | "error";
  message: string;
}

export function validateScheme(s: PointerScheme): PointerIssue[] {
  const issues: PointerIssue[] = [];
  for (const r of POINTER_ROLES) {
    const asset = s.roles[r.id];
    if (!asset) continue;
    if (asset.frames.length === 0) {
      issues.push({ role: r.id, kind: "empty", level: "error", message: "该指针尚未导入或绘制任何帧" });
      continue;
    }
    if (!asset.hotspot) {
      issues.push({ role: r.id, kind: "hotspot-unset", level: "warning", message: "热点未设，运行时按 (0,0) 处理（黄色提醒）" });
    }
    const scaled = Math.round(asset.size * s.scale);
    if (scaled > SIZE_LIMIT_PX) {
      issues.push({ role: r.id, kind: "size-overflow", level: "warning", message: `缩放后 ${scaled}px 超过 ${SIZE_LIMIT_PX}px，可能遮挡内容` });
    }
    if (asset.frames.length > ANIM_MAX_FRAMES) {
      issues.push({ role: r.id, kind: "frames-overflow", level: "error", message: `动效帧 ${asset.frames.length} 超上限 ${ANIM_MAX_FRAMES}` });
    }
    if (asset.fps > ANIM_MAX_FPS) {
      issues.push({ role: r.id, kind: "fps-oversample", level: "warning", message: `帧率 ${asset.fps}fps >60，导出时已降采样至 60` });
    }
  }
  return issues;
}

// ---------- sprite 双倍率生成（纯函数） ----------

export interface SpritePlan {
  role: string;
  /** 1x 尺寸。 */
  size1x: number;
  /** 2x 尺寸（4K 基线）。 */
  size2x: number;
  /** 1x/2x 热点。 */
  hotspot1x: { x: number; y: number };
  hotspot2x: { x: number; y: number };
}

/** 双倍率 sprite 方案：热点按倍率精确放大（1px 级取放对拍的数学面）。 */
export function spritePlan(asset: PointerRoleAsset, schemeScale: number): SpritePlan {
  const hs = asset.hotspot ?? { x: 0, y: 0 };
  const size1x = Math.round(asset.size * schemeScale);
  return {
    role: asset.role,
    size1x,
    size2x: size1x * 2,
    hotspot1x: { x: Math.round(hs.x * schemeScale), y: Math.round(hs.y * schemeScale) },
    hotspot2x: { x: Math.round(hs.x * schemeScale * 2), y: Math.round(hs.y * schemeScale * 2) },
  };
}

/** 导出前的帧率降采样（>60fps → 60，主册「降采样提示」的执行面）。 */
export function clampFps(fps: number): number {
  return Math.min(ANIM_MAX_FPS, Math.max(1, Math.round(fps)));
}

/** 帧序列裁剪到 16 帧上限。 */
export function clampFrames(frames: string[]): string[] {
  return frames.slice(0, ANIM_MAX_FRAMES);
}

/** 预览底色三档（深/浅/中灰——各底色可见性检查）。 */
export const PREVIEW_BACKDROPS: readonly { id: string; color: string }[] = [
  { id: "dark", color: "#1c1c26" },
  { id: "light", color: "#f0f0f4" },
  { id: "mid", color: "#808088" },
] as const;
