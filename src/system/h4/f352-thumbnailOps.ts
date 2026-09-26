/**
 * F352 缩略图窗上直接操作（H 域 · AI-H4）：
 * 任务栏悬停缩略图（F073）不只是看——三型缩略图各带操作集：
 * - 普通：右上角关闭 ×（点掉对应窗不打断当前工作）；
 * - 媒体：播放/暂停迷你键（F251 媒体会话联动）；
 * - 传输：迷你进度条（只读，不可点）。
 * 判据（主册 F352）：
 * - 三型缩略图操作集分型正确；
 * - 关闭命中精度：命中区 = 按钮可见区外扩 2px（手抖容差），不误伤相邻键；
 * - 迷你键响应 <100ms（本层提供响应预算常量与点击时序校验）；
 * - 悬停稳定性：指针在缩略图/操作按钮上（含移动途中）时隐藏被抑制——
 *   操作进行中缩略图不消失（离开延迟 hideDelay 走 F205 消失时序）。
 */

/** 缩略图三型（判据口径）。 */
export type ThumbKind = "normal" | "media" | "transfer";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface ThumbAction {
  id: string;
  kind: "close" | "playpause" | "progress";
  /** 按钮可视区（缩略图坐标系）。 */
  rect: Rect;
  enabled: boolean;
}

export interface ThumbModel {
  winId: string;
  kind: ThumbKind;
  actions: ThumbAction[];
}

/** 关闭按钮可视边长（px）。 */
export const CLOSE_BUTTON_SIZE = 24;
/** 命中外扩容差（px）——「关闭命中精度」的实现口径。 */
export const HIT_TOLERANCE_PX = 2;
/** 迷你键响应预算（ms）——判据 <100ms。 */
export const MINI_KEY_RESPONSE_BUDGET_MS = 100;
/** 悬停抑制后的离开隐藏延迟（ms，F205 消失时序同源）。 */
export const HIDE_DELAY_MS = 200;

/** 按三型生成标准操作集（普通=关闭；媒体=关闭+播放暂停；传输=进度只读）。 */
export function buildThumbModel(winId: string, kind: ThumbKind, thumbW: number, playing = true): ThumbModel {
  const actions: ThumbAction[] = [];
  if (kind !== "transfer") {
    actions.push({
      id: "close",
      kind: "close",
      rect: { x: thumbW - CLOSE_BUTTON_SIZE - 6, y: 6, w: CLOSE_BUTTON_SIZE, h: CLOSE_BUTTON_SIZE },
      enabled: true,
    });
  }
  if (kind === "media") {
    actions.push({ id: "playpause", kind: "playpause", rect: { x: Math.round(thumbW / 2) - 16, y: 8, w: 32, h: 32 }, enabled: true });
    actions.push({ id: "mediaState", kind: "progress", rect: { x: 0, y: 0, w: 0, h: 0 }, enabled: playing });
  }
  if (kind === "transfer") {
    actions.push({ id: "progress", kind: "progress", rect: { x: 8, y: 8, w: thumbW - 16, h: 6 }, enabled: false });
  }
  return { winId, kind, actions };
}

/** 命中测试：可视区外扩 HIT_TOLERANCE_PX；禁用键不命中；命中返回动作 id。 */
export function hitTest(model: ThumbModel, px: number, py: number): string | null {
  for (const a of model.actions) {
    if (!a.enabled || a.rect.w === 0 || a.rect.h === 0) continue;
    const inX = px >= a.rect.x - HIT_TOLERANCE_PX && px <= a.rect.x + a.rect.w + HIT_TOLERANCE_PX;
    const inY = py >= a.rect.y - HIT_TOLERANCE_PX && py <= a.rect.y + a.rect.h + HIT_TOLERANCE_PX;
    if (inX && inY) return a.id;
  }
  return null;
}

/**
 * 命中精度审计：两按钮外扩后不得重叠（重叠 = 手抖容差把邻居也吃掉 → 缺陷）。
 * 返回冲突对列表（空 = 通过）。
 */
export function auditHitPrecision(model: ThumbModel): Array<[string, string]> {
  const active = model.actions.filter((a) => a.enabled && a.rect.w > 0 && a.rect.h > 0);
  const conflicts: Array<[string, string]> = [];
  for (let i = 0; i < active.length; i++) {
    for (let j = i + 1; j < active.length; j++) {
      const a = active[i]!;
      const b = active[j]!;
      const ax2 = a.rect.x + a.rect.w + HIT_TOLERANCE_PX;
      const ay2 = a.rect.y + a.rect.h + HIT_TOLERANCE_PX;
      const bx2 = b.rect.x + b.rect.w + HIT_TOLERANCE_PX;
      const by2 = b.rect.y + b.rect.h + HIT_TOLERANCE_PX;
      const overlap = a.rect.x - HIT_TOLERANCE_PX < bx2 && b.rect.x - HIT_TOLERANCE_PX < ax2 && a.rect.y - HIT_TOLERANCE_PX < by2 && b.rect.y - HIT_TOLERANCE_PX < ay2;
      if (overlap) conflicts.push([a.id, b.id]);
    }
  }
  return conflicts;
}

/** 点击响应时序校验：按下到生效耗时是否在 <100ms 预算内。 */
export function withinResponseBudget(pressedAtMs: number, effectiveAtMs: number): boolean {
  return effectiveAtMs - pressedAtMs < MINI_KEY_RESPONSE_BUDGET_MS;
}

/**
 * 悬停稳定性状态机：指针进入缩略图区 → 展示；在区内/操作按钮间移动 → 抑制隐藏；
 * 离开全部区域 → 起 HIDE_DELAY 计时后隐藏；中途回来 → 取消隐藏。
 * 返回新的「应显示」判定与是否需要重置隐藏计时。
 */
export type HoverZone = "outside" | "thumb" | "action";

export interface HoverState {
  visible: boolean;
  /** true = 需要重启隐藏倒计时（HIDE_DELAY_MS 后隐藏）。 */
  scheduleHide: boolean;
  /** true = 需要取消已排队的隐藏。 */
  cancelHide: boolean;
}

export function hoverTransition(current: { visible: boolean; hideQueued: boolean }, zone: HoverZone): HoverState {
  switch (zone) {
    case "thumb":
    case "action":
      return { visible: true, scheduleHide: false, cancelHide: current.hideQueued };
    case "outside":
      if (!current.visible) return { visible: false, scheduleHide: false, cancelHide: false };
      return { visible: true, scheduleHide: true, cancelHide: false };
  }
}

/** 传输类迷你进度条：0-100 归一化 + 渲染裁剪（超界输入钳制，零 NaN）。 */
export function transferProgress(bytesDone: number, bytesTotal: number): { ratio: number; clamped: boolean; valid: boolean } {
  if (!Number.isFinite(bytesDone) || !Number.isFinite(bytesTotal) || bytesTotal <= 0 || bytesDone < 0) {
    return { ratio: 0, clamped: false, valid: false };
  }
  const raw = bytesDone / bytesTotal;
  const clamped = raw > 1;
  return { ratio: Math.min(1, Math.max(0, raw)), clamped, valid: true };
}
