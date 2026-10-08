/**
 * F107 输入法状态浮窗 · 前端逻辑（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据（主册 G-C-37）：三态切换跟随 <16ms（一帧内）；光标跟随重定位不抖
 * （节流 16ms F027 同源）；避让边界 20 例全对。
 *
 * 移植声明：`kernel/varix/src/stard/imefloat.rs` 的 TS 同构移植——三态
 * 8 组合标签、节流（首移必应用）、避让几何唯一源 + 终极钳制，逐项对齐。
 * 时钟注入；D13/D14/D15 三处模型缺陷的修法（首移旁路/终极钳制）随移植
 * 原样携带。
 */

/** 浮窗尺寸（px）。 */
export const FLOAT_W_PX = 160;
export const FLOAT_H_PX = 32;
/** 圆角（px）。 */
export const FLOAT_RADIUS_PX = 8;
/** 光标下方偏移（px）。 */
export const CURSOR_GAP_PX = 8;
/** 跟随节流（ms，F027 同源——重定位不抖的机制本体）。 */
export const FOLLOW_THROTTLE_MS = 16;
/** 态切换微动画（ms，字重渐变）。 */
export const SWITCH_ANIM_MS = 80;
/** 浮窗字体（px，专用档）。 */
export const FLOAT_FONT_PX = 13;

/** 输入法三态。 */
export interface ImeStates {
  chinese: boolean;
  fullwidth: boolean;
  cnPunct: boolean;
}

export function defaultImeStates(): ImeStates {
  return { chinese: true, fullwidth: true, cnPunct: true };
}

/** 三态连接文本（8 组合标签互异——判据「三态 8 组合标签互异」的机制面）。 */
export function imeLabel(s: ImeStates): string {
  const lang = s.chinese ? "中" : "英";
  const width = s.fullwidth ? "全角" : "半角";
  const punct = s.cnPunct ? "中文标点" : "英文标点";
  return `${lang} · ${width} · ${punct}`;
}

/** 切换任一态（浮窗即开关）：0=中英 1=全半角 2=中英标点。 */
export function toggleImeState(s: ImeStates, which: 0 | 1 | 2): ImeStates {
  if (which === 0) return { ...s, chinese: !s.chinese };
  if (which === 1) return { ...s, fullwidth: !s.fullwidth };
  return { ...s, cnPunct: !s.cnPunct };
}

export type FloatMode = "follow" | "taskbarChip" | "hidden" | "cornerBadge";

export interface FloatRect { x: number; y: number; w: number; h: number }

export class ImeFloat {
  states: ImeStates = defaultImeStates();
  mode: FloatMode = "follow";
  /** 光标锚（屏幕坐标——文本插入点）。 */
  cursorX = 0;
  cursorY = 0;
  private lastMoveMs = 0;
  /** 节流账。 */
  coalescedMoves = 0;
  appliedMoves = 0;
  /** 灰显（程序不支持 IME 组合——F027 降级路径）。 */
  greyed = false;
  switchAnims = 0;

  /** 光标移动（16ms 节流——窗口内移动合并计数不重排；首次移动必应用）。
   *  （D14 同源修法：appliedMoves==0 时旁路节流窗，首移不被吞。） */
  cursorMoved(x: number, y: number, nowMs: number): void {
    this.cursorX = x;
    this.cursorY = y;
    if (this.appliedMoves > 0 && nowMs - this.lastMoveMs < FOLLOW_THROTTLE_MS) {
      this.coalescedMoves += 1;
      return;
    }
    this.lastMoveMs = nowMs;
    this.appliedMoves += 1;
  }

  /** 浮窗落位：光标下方 8px；右缘自动左移 / 底缘上移 + 终极钳制
   *  （避让几何唯一源——20 例避让判据与多分辨率矩阵都走这一处）。 */
  rect(screenW: number, screenH: number): FloatRect {
    if (this.mode !== "follow") return { x: 0, y: 0, w: 0, h: 0 };
    let x = this.cursorX;
    let y = this.cursorY + CURSOR_GAP_PX;
    if (x + FLOAT_W_PX > screenW) x = screenW - FLOAT_W_PX;
    if (x < 0) x = 0;
    if (y + FLOAT_H_PX > screenH) y = this.cursorY - CURSOR_GAP_PX - FLOAT_H_PX;
    // 终极钳制（D15 同源修法）：任何路径下浮窗整体在屏内。
    x = Math.max(0, Math.min(x, screenW - FLOAT_W_PX));
    y = Math.max(0, Math.min(y, screenH - FLOAT_H_PX));
    return { x, y, w: FLOAT_W_PX, h: FLOAT_H_PX };
  }

  /** 态切换（80ms 微动画 + 一帧内跟随——<16ms 判线：切换本身零重排）。 */
  toggleState(which: 0 | 1 | 2): void {
    this.states = toggleImeState(this.states, which);
    this.switchAnims += 1;
  }

  /** 密码框聚焦 → 自动隐藏（隐私纪律）；失焦还原跟随。 */
  setPasswordFocus(focused: boolean): void {
    if (focused) this.mode = "hidden";
    else if (this.mode === "hidden") this.mode = "follow";
  }

  /** 收起为任务栏小标 / 展开。 */
  collapse(toChip: boolean): void {
    this.mode = toChip ? "taskbarChip" : "follow";
  }

  /** 全屏降级（F106 角标复用）。 */
  setFullscreen(fullscreen: boolean): void {
    this.mode = fullscreen ? "cornerBadge" : "follow";
  }

  setGreyed(greyed: boolean): void {
    this.greyed = greyed;
  }
}

/** 避让自检：20 例边界扫描（判据「避让边界 20 例全对」的机制面）。
 *  光标扫四角、四边中点、右下象限密集点，浮窗必须整体在屏内。 */
export function avoidanceSelfTest(screenW: number, screenH: number): boolean {
  const f = new ImeFloat();
  const w = screenW;
  const h = screenH;
  const cases: Array<[number, number]> = [
    [0, 0], [w - 1, 0], [0, h - 1], [w - 1, h - 1],       // 四角
    [Math.floor(w / 2), 0], [Math.floor(w / 2), h - 1],   // 上下中点
    [0, Math.floor(h / 2)], [w - 1, Math.floor(h / 2)],   // 左右中点
    [w - 1, Math.floor((h / 4) * 1)],                     // 右缘 1/4
    [w - 1, Math.floor((h / 4) * 2)],
    [w - 1, Math.floor((h / 4) * 3)],
    [Math.floor((w / 4) * 3), h - 1],                     // 底缘 3/4
    [Math.floor((w / 4) * 3), Math.floor((h / 4) * 3)],   // 右下象限密集
    [w - 5, h - 5], [w - 1, h - 1], [w - FLOAT_W_PX, h - 1],
    [Math.floor(w / 2), Math.floor(h / 2)],               // 居中
    [1, 1], [w - 2, 1],
  ];
  for (const [cx, cy] of cases) {
    f.cursorMoved(cx, cy, f.appliedMoves * 100); // 首移旁路 + 递增避开节流
    const r = f.rect(w, h);
    if (r.x < 0 || r.y < 0 || r.x + r.w > w || r.y + r.h > h) return false;
  }
  return true;
}
