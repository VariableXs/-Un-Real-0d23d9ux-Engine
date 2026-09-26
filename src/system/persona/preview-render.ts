/**
 * F152 预览深化 · 场景几何模型 + 令牌→样式解析 + 脏区 diff。
 *
 * 主册判据延伸：
 * - F152「改-预览延迟 ≤100ms」的几何面：预览不是截图——是同一棵场景
 *   描述树按令牌重算（dirty-rect 只重画变化的矩形，F056 同源纪律）；
 * - 「放弃-零残留」：预览渲染是纯函数（场景+令牌表 → 帧描述），
 *   令牌表还原则输出逐位还原（对拍可验证）。
 */

import type { TokenTable } from "./tokens";

// ---------- 场景描述树（三场景的几何真源） ----------

export type SceneKind = "desktop" | "explorer" | "settings";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface SceneNode {
  id: string;
  role: "wall" | "window" | "titlebar" | "taskbar" | "panel" | "icon" | "text-line" | "button";
  rect: Rect;
  /** 消费的令牌键（dirty 判定的依赖面）。 */
  tokens: string[];
}

export const SCENE_SIZE = { width: 400, height: 240 }; // 预览固定逻辑分辨率（0.5x 缩放的真渲染）。

/** 场景树（确定性几何——同场景同树，dirty diff 的基线）。 */
export function sceneTree(kind: SceneKind): SceneNode[] {
  const W = SCENE_SIZE.width;
  const H = SCENE_SIZE.height;
  const nodes: SceneNode[] = [
    { id: "wall", role: "wall", rect: { x: 0, y: 0, w: W, h: H }, tokens: ["--p-bg-canvas", "--p-accent-soft"] },
  ];
  if (kind === "desktop") {
    nodes.push(
      { id: "icon-1", role: "icon", rect: { x: 16, y: 16, w: 48, h: 48 }, tokens: ["--p-accent"] },
      { id: "icon-2", role: "icon", rect: { x: 16, y: 76, w: 48, h: 48 }, tokens: ["--p-accent"] },
      { id: "win", role: "window", rect: { x: 110, y: 28, w: 200, h: 130 }, tokens: ["--p-bg-raised", "--p-r-window", "--p-border-regular"] },
      { id: "win-title", role: "titlebar", rect: { x: 110, y: 28, w: 200, h: 24 }, tokens: ["--p-bg-surface", "--p-fg-primary"] },
      { id: "taskbar", role: "taskbar", rect: { x: 0, y: H - 32, w: W, h: 32 }, tokens: ["--p-bg-surface", "--p-border-subtle"] },
    );
  } else if (kind === "explorer") {
    nodes.push(
      { id: "side", role: "panel", rect: { x: 0, y: 0, w: 96, h: H - 32 }, tokens: ["--p-bg-surface"] },
      { id: "list", role: "window", rect: { x: 96, y: 0, w: W - 96, h: H - 32 }, tokens: ["--p-bg-canvas"] },
      { id: "line-1", role: "text-line", rect: { x: 108, y: 16, w: 240, h: 8 }, tokens: ["--p-fg-primary"] },
      { id: "line-2", role: "text-line", rect: { x: 108, y: 36, w: 180, h: 8 }, tokens: ["--p-fg-secondary"] },
      { id: "taskbar", role: "taskbar", rect: { x: 0, y: H - 32, w: W, h: 32 }, tokens: ["--p-bg-surface"] },
    );
  } else {
    nodes.push(
      { id: "panel", role: "panel", rect: { x: 24, y: 20, w: W - 48, h: H - 60 }, tokens: ["--p-bg-raised", "--p-r-card"] },
      { id: "row-text", role: "text-line", rect: { x: 40, y: 40, w: 200, h: 10 }, tokens: ["--p-fg-primary"] },
      { id: "row-btn", role: "button", rect: { x: 40, y: 64, w: 88, h: 26 }, tokens: ["--p-accent", "--p-r-control", "--p-on-accent"] },
    );
  }
  return nodes;
}

// ---------- 令牌 → 解析样式（回退链：--p-* → 默认物理值） ----------

export interface ResolvedStyle {
  background?: string;
  color?: string;
  borderRadius?: string;
  borderColor?: string;
}

const TOKEN_FALLBACK: Record<string, string> = {
  "--p-bg-canvas": "#14141c",
  "--p-bg-surface": "#1c1c26",
  "--p-bg-raised": "#22222e",
  "--p-fg-primary": "#e8e8f0",
  "--p-fg-secondary": "#a0a0b4",
  "--p-accent": "#6e7fd4",
  "--p-accent-soft": "rgba(110,127,212,0.2)",
  "--p-on-accent": "#ffffff",
  "--p-border-regular": "rgba(140,140,160,0.24)",
  "--p-border-subtle": "rgba(140,140,160,0.12)",
  "--p-r-control": "8px",
  "--p-r-card": "12px",
  "--p-r-window": "16px",
};

/** 令牌解析（表缺失键走出厂物理值——预览永不因缺键白屏）。 */
export function resolveTokens(table: TokenTable | null, keys: string[]): ResolvedStyle {
  const style: ResolvedStyle = {};
  const pick = (k: string): string => table?.colors[k] ?? TOKEN_FALLBACK[k] ?? "transparent";
  for (const k of keys) {
    if (k.startsWith("--p-bg")) style.background = pick(k);
    else if (k === "--p-fg-primary" || k === "--p-fg-secondary" || k === "--p-on-accent") style.color = pick(k);
    else if (k.startsWith("--p-r-")) style.borderRadius = pick(k);
    else if (k.startsWith("--p-border")) style.borderColor = pick(k);
    else if (k === "--p-accent") {
      style.background = pick(k);
      style.color = table?.colors["--p-on-accent"] ?? TOKEN_FALLBACK["--p-on-accent"]!;
    } else {
      // 未知键：显性给透明（预览不白屏不猜色——缺失可见）。
      style.background = pick(k);
    }
  }
  return style;
}

// ---------- 脏区 diff（两帧间变化矩形——只重画该重画的） ----------

export interface FrameNode {
  id: string;
  rect: Rect;
  style: ResolvedStyle;
}

export function renderFrame(kind: SceneKind, table: TokenTable | null): FrameNode[] {
  return sceneTree(kind).map((n) => ({ id: n.id, rect: n.rect, style: resolveTokens(table, n.tokens) }));
}

function rectEqual(a: Rect, b: Rect): boolean {
  return a.x === b.x && a.y === b.y && a.w === b.w && a.h === b.h;
}

function styleEqual(a: ResolvedStyle, b: ResolvedStyle): boolean {
  return a.background === b.background && a.color === b.color && a.borderRadius === b.borderRadius && a.borderColor === b.borderColor;
}

/** 脏区计算：样式或几何变化的节点矩形（F056 合成器脏区口径同源）。 */
export function dirtyRects(prev: FrameNode[], next: FrameNode[]): { dirty: Rect[]; changedIds: string[]; cleanRatio: number } {
  const prevMap = new Map(prev.map((n) => [n.id, n]));
  const dirty: Rect[] = [];
  const changedIds: string[] = [];
  for (const n of next) {
    const p = prevMap.get(n.id);
    if (!p || !rectEqual(p.rect, n.rect) || !styleEqual(p.style, n.style)) {
      dirty.push(n.rect);
      changedIds.push(n.id);
    }
  }
  const totalArea = SCENE_SIZE.width * SCENE_SIZE.height;
  const dirtyArea = dirty.reduce((s, r) => s + r.w * r.h, 0);
  return { dirty, changedIds, cleanRatio: dirty.length === 0 ? 1 : Math.round((1 - dirtyArea / totalArea) * 1000) / 1000 };
}
