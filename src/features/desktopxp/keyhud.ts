/**
 * F106 键盘提示 HUD · 前端逻辑（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据（主册 G-C-36）：三键切换 HUD 三态全对；淡出时机 1s±50ms；
 * 全屏降级路径实测。
 *
 * 移植声明：本模块是 `kernel/varix/src/stard/keyhud.rs` 的 TS 同构移植——
 * 状态机/常量/语义逐项对齐（一处一事实），时钟由调用方注入（宿主测试
 * 确定复现）。边界：KeycastOverlay（M-33）显示快捷键组合回显，与本件
 * （锁定键三态提示）职责互异，互不替代。
 */

/** HUD 横条宽（px）——同 Rust HUD_W_PX。 */
export const HUD_W_PX = 640;
/** 距屏底（px）——同 Rust BOTTOM_OFFSET_PX。 */
export const BOTTOM_OFFSET_PX = 80;
/** 出现动画（ms，上浮淡入）。 */
export const SHOW_ANIM_MS = 120;
/** 淡出前驻留（ms，判线 1s±50ms）。 */
export const HOLD_MS = 1_000;
/** 淡出动画（ms）。 */
export const FADE_OUT_MS = 300;
/** 驻留判线容差（±ms）。 */
export const HOLD_TOLERANCE_MS = 50;
/** 角标模式图标边（px，全屏降级）。 */
export const CORNER_ICON_PX = 24;
/** 风暴合并窗口（ms）——同 Rust toggle 的 200ms 连发合并。 */
export const STORM_WINDOW_MS = 200;

export type LockKey = "caps" | "num" | "scroll";

/** 状态字（三态文案——「大写锁定 开/关」）。 */
export function lockLabel(key: LockKey, on: boolean): string {
  switch (key) {
    case "caps": return on ? "大写锁定 开" : "大写锁定 关";
    case "num": return on ? "数字锁定 开" : "数字锁定 关";
    case "scroll": return on ? "滚动锁定 开" : "滚动锁定 关";
  }
}

/** 图标形状冗余（色弱可辨——圆/方/三角三形互异，无障碍 B-39xx 联动）。 */
export function lockIconShape(key: LockKey): "circle" | "square" | "triangle" {
  return key === "caps" ? "circle" : key === "num" ? "square" : "triangle";
}

export type HudState =
  | { kind: "hidden" }
  | { kind: "showing"; t0: number }
  | { kind: "holding"; t0: number }
  | { kind: "fading"; t0: number };

export type HudContent =
  | { kind: "lock"; key: LockKey; on: boolean }
  | { kind: "ime"; chinese: boolean };

/** HUD 显示几何（px）：居下中横条 / 全屏角标右下小图标。 */
export interface HudRect { x: number; y: number; w: number; h: number }

export class KeyHud {
  state: HudState = { kind: "hidden" };
  content: HudContent | null = null;
  enabled = true;
  /** 全屏降级角标模式。 */
  cornerMode = false;
  /** 切换风暴账（同窗 200ms 内连发计数——合并展示）。 */
  stormMerged = 0;
  /** 实际展示次数（合并不重展 → 风暴下保持 1）。 */
  showCount = 0;
  private lastToggleMs = 0;

  /** 锁定键切换（键盘层喂入）。
   *  风暴合并：200ms 内连发只换内容计数不重展（不闪屏——连发不重置
   *  驻留钟，HUD 保持稳定）；超窗的新切换才重新展示（重置计时）。 */
  toggle(key: LockKey, on: boolean, nowMs: number): void {
    if (!this.enabled) return;
    const showing = this.state.kind === "showing" || this.state.kind === "holding";
    const merging = showing && nowMs - this.lastToggleMs < STORM_WINDOW_MS;
    this.lastToggleMs = nowMs;
    this.content = { kind: "lock", key, on };
    if (merging) this.stormMerged += 1;
    else this.enterShow(nowMs);
  }

  /** 输入法切换复用（F107 联动——图标换「中/英」）。 */
  imeSwitch(chinese: boolean, nowMs: number): void {
    if (!this.enabled) return;
    this.content = { kind: "ime", chinese };
    this.enterShow(nowMs);
  }

  private enterShow(nowMs: number): void {
    this.state = { kind: "showing", t0: nowMs };
    this.showCount += 1;
  }

  /** tick 推进（调用方按帧喂——淡出时机 1s±50ms 判线）。 */
  tick(nowMs: number): void {
    if (this.state.kind === "showing") {
      if (nowMs - this.state.t0 >= SHOW_ANIM_MS) {
        this.state = { kind: "holding", t0: this.state.t0 + SHOW_ANIM_MS };
      }
    } else if (this.state.kind === "holding") {
      if (nowMs - this.state.t0 >= HOLD_MS) {
        this.state = { kind: "fading", t0: this.state.t0 + HOLD_MS };
      }
    } else if (this.state.kind === "fading") {
      if (nowMs - this.state.t0 >= FADE_OUT_MS) {
        this.state = { kind: "hidden" };
        this.content = null;
      }
    }
  }

  /** 淡出实际时刻核算（Holding 起点 + HOLD——±50ms 容差外即缺陷）。 */
  fadeoutDueAt(): number | null {
    return this.state.kind === "holding" ? this.state.t0 + HOLD_MS : null;
  }

  /** 几何：居下中横条（角标模式转右下小图标）。 */
  rect(screenW: number, screenH: number): HudRect {
    if (this.cornerMode) {
      return { x: screenW - CORNER_ICON_PX - 16, y: screenH - CORNER_ICON_PX - 16, w: CORNER_ICON_PX, h: CORNER_ICON_PX };
    }
    return { x: (screenW - HUD_W_PX) / 2, y: screenH - BOTTOM_OFFSET_PX - 48, w: HUD_W_PX, h: 48 };
  }

  /** 全屏降级（放映/游戏——转角标）。 */
  setFullscreen(fullscreen: boolean): void {
    this.cornerMode = fullscreen;
  }
}
