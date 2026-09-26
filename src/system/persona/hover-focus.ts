/**
 * F205/F206 悬停与焦点深化 · tooltip 状态机 + 焦点环/Tab 序/归还表。
 *
 * 主册判据延伸：
 * - F205「出现/消失 500ms/200ms±20ms」「屏幕四角锚点翻转」「全系统审计
 *   无一处自绘 tooltip」——tooltip 统一状态机 + 翻转几何（数学面）；
 * - F206「焦点环可见、Tab 顺序符合视觉顺序、焦点归还 100%」——Tab 序
 *   计算（场景树 + 角色权重）、焦点归还配对表、对话框焦点陷阱。
 */

import type { SceneNode } from "./preview-render";

// ---------- tooltip 状态机 ----------

export type TooltipPhase = "idle" | "waiting-show" | "visible" | "waiting-hide";

export const TOOLTIP_SHOW_DELAY_MS = 500;
export const TOOLTIP_HIDE_DELAY_MS = 200;

export interface TooltipMachineState {
  phase: TooltipPhase;
  /** 当前提示内容键（说明句三件套的 tooltip 面）。 */
  targetId: string | null;
}

/** tooltip 状态机（纯逻辑可回放——delay 校准 ±20ms 的判定面）。 */
export class TooltipMachine {
  private phase: TooltipPhase = "idle";
  private targetId: string | null = null;

  get state(): TooltipMachineState {
    return { phase: this.phase, targetId: this.targetId };
  }

  /** 悬停进入：idle → waiting-show；已显示则换目标（不重等 500ms）。 */
  hoverEnter(targetId: string): boolean {
    if (this.phase === "visible") {
      this.targetId = targetId;
      return true; // 邻近目标即切——等待只针对冷启动。
    }
    this.phase = "waiting-show";
    this.targetId = targetId;
    return false;
  }

  /** 计时到点（定时器回调）。 */
  showDue(): boolean {
    if (this.phase === "waiting-show") {
      this.phase = "visible";
      return true;
    }
    return false;
  }

  /** 悬停离开 → waiting-hide（200ms 宽限——扫过间隙不闪烁）。 */
  hoverLeave(): boolean {
    if (this.phase === "visible" || this.phase === "waiting-show") {
      this.phase = "waiting-hide";
      return true;
    }
    return false;
  }

  hideDue(): void {
    if (this.phase === "waiting-hide") {
      this.phase = "idle";
      this.targetId = null;
    }
  }

  /** 期间再进入 → 取消隐藏（宽限语义）。 */
  hoverReenter(targetId: string): boolean {
    if (this.phase === "waiting-hide" && this.targetId === targetId) {
      this.phase = "visible";
      return true;
    }
    return this.hoverEnter(targetId);
  }
}

// ---------- 锚点翻转几何（四角用例的数学面） ----------

export interface AnchorRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Viewport {
  width: number;
  height: number;
}

export type Placement = "top" | "bottom" | "left" | "right";

/** tooltip 放置：优先指定侧，放不下按剩余空间翻转（四角翻转 4 用例全对）。 */
export function placeTooltip(anchor: AnchorRect, tipSize: { w: number; h: number }, gap: number, viewport: Viewport, preferred: Placement = "bottom"): { placement: Placement; x: number; y: number } {
  const fits: Record<Placement, boolean> = {
    bottom: anchor.y + anchor.h + gap + tipSize.h <= viewport.height,
    top: anchor.y - gap - tipSize.h >= 0,
    right: anchor.x + anchor.w + gap + tipSize.w <= viewport.width,
    left: anchor.x - gap - tipSize.w >= 0,
  };
  const order: Placement[] = [preferred, "bottom", "top", "right", "left"];
  const placement = order.find((p) => fits[p]) ?? preferred; // 全放不下 → 保持偏好（边缘场景显性接受）。
  let x: number;
  let y: number;
  switch (placement) {
    case "bottom":
      x = anchor.x + anchor.w / 2 - tipSize.w / 2;
      y = anchor.y + anchor.h + gap;
      break;
    case "top":
      x = anchor.x + anchor.w / 2 - tipSize.w / 2;
      y = anchor.y - gap - tipSize.h;
      break;
    case "right":
      x = anchor.x + anchor.w + gap;
      y = anchor.y + anchor.h / 2 - tipSize.h / 2;
      break;
    case "left":
      x = anchor.x - gap - tipSize.w;
      y = anchor.y + anchor.h / 2 - tipSize.h / 2;
      break;
  }
  // 视口内夹紧（贴边不裁切——比翻转更细的一层）。
  x = Math.max(4, Math.min(viewport.width - tipSize.w - 4, x));
  y = Math.max(4, Math.min(viewport.height - tipSize.h - 4, y));
  return { placement, x: Math.round(x), y: Math.round(y) };
}

// ---------- 焦点模型（Tab 序 + 归还 + 陷阱） ----------

export interface FocusableNode {
  id: string;
  /** Tab 权重（DOM 序即视觉序的代理——sceneTree 深度优先序）。 */
  order: number;
  role: SceneNode["role"] | "custom";
  disabled: boolean;
}

/** Tab 序：视觉顺序 = 登记顺序（树遍历保证），跳过禁用；循环回到首。 */
export function tabSequence(nodes: FocusableNode[], currentIndex: number, back = false): number {
  const focusable = nodes.map((n, i) => ({ ...n, index: i })).filter((n) => !n.disabled);
  if (focusable.length === 0) return -1;
  const pos = focusable.findIndex((n) => n.index === currentIndex);
  const step = back ? -1 : 1;
  const next = focusable[(pos + step + focusable.length) % focusable.length]!;
  return next.index;
}

/** 焦点归还配对表（开关浮层 → 记录触发者 → 关闭时归还——100% 判定的数据面）。 */
export class FocusReturnLedger {
  private stack: Array<{ popoverId: string; triggerId: string }> = [];

  open(popoverId: string, triggerId: string): void {
    this.stack.push({ popoverId, triggerId });
  }

  close(popoverId: string): { returnTo: string } | null {
    const idx = this.stack.findIndex((e) => e.popoverId === popoverId);
    if (idx < 0) return null; // 未登记的关闭——显性 null（丢焦点 = 缺陷可检出）。
    const [entry] = this.stack.splice(idx, 1);
    return { returnTo: entry!.triggerId };
  }

  /** 泄漏审计：页面卸载时应为空——非空 = 有浮层关了没还焦点。 */
  leaks(): string[] {
    return this.stack.map((e) => e.popoverId);
  }
}

/** 对话框焦点陷阱：Tab 循环锁定在容器内可聚焦集（Esc 归还由调用方接 FocusReturnLedger）。 */
export function trapCycle(ids: string[], currentId: string, back = false): string {
  if (ids.length === 0) return currentId;
  const pos = ids.indexOf(currentId);
  const next = (pos + (back ? -1 : 1) + ids.length) % ids.length;
  return ids[next]!;
}
