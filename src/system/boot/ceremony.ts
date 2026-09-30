/**
 * U-01/U-06 启动仪式状态机（纯函数 reducer，可测）。
 *
 * 阶段：entering(入场编排) → streaming(真实流式加载) → readyHold(真实摘要停留)
 *       → exiting(按 bootAnim 退出编排) → done。
 *
 * 硬性规则（docs/ARCHITECTURE_V2.md §5）：
 * - progress 单调不减：回放旧 seq / 乱序事件绝不回退（reducer 内 Math.max 兜底）。
 * - ready 只认真实 stats 事件，无预设时间线；快机（入场期就 ready）在入场结束后
 *   直达 readyHold，不空等 streaming；慢机在 streaming 里自然拉伸。
 * - skip 仅在真实进度 ≥30% 时被接受（SKIP_THRESHOLD），跳过的是 UI 而非后台加载。
 *
 * 定时器是副作用，由 BootScreen 按阶段 + pacingTimings 驱动；reducer 本身无时钟。
 */

import type { BootPacing } from "../../lib/settings";

export type CeremonyPhase = "entering" | "streaming" | "readyHold" | "exiting" | "done";

export type CeremonyEvent =
  | { type: "ENTER_DONE" }
  | { type: "PROGRESS"; progress: number }
  | { type: "READY" }
  | { type: "SKIP" }
  | { type: "EXIT_DONE" };

export interface CeremonyState {
  phase: CeremonyPhase;
  /** 真实进度 0..1（单调不减；显示层另有 rAF 平滑，不在此处）。 */
  progress: number;
  /** 后端 ready（携带真实 stats）已到达。 */
  ready: boolean;
  /** 用户已跳过（≥30%；后台加载继续，仅 UI 提前退出）。 */
  skipped: boolean;
}

export const INITIAL_CEREMONY: CeremonyState = {
  phase: "entering",
  progress: 0,
  ready: false,
  skipped: false,
};

/** 跳过阈值：真实进度 ≥30% 才允许跳过 UI（更早拒绝并如实提示）。 */
export const SKIP_THRESHOLD = 0.3;

export interface PacingTimings {
  /** entering 阶段时长（入场编排）。 */
  enter: number;
  /** readyHold 阶段时长（真实摘要停留）。 */
  readyHold: number;
}

/**
 * 节奏档位（U-06 bootPacing 设置）：
 * - cinematic：影院（enter 900ms / readyHold 1200ms）
 * - brisk：轻快（enter 500ms / readyHold 400ms）
 * - instant：直通 —— 两个等待都归零，BootScreen 直接走 bootAnim=none 快路径。
 */
export function pacingTimings(pacing: BootPacing): PacingTimings {
  switch (pacing) {
    case "brisk":
      return { enter: 500, readyHold: 400 };
    case "instant":
      return { enter: 0, readyHold: 0 };
    case "cinematic":
    default:
      return { enter: 900, readyHold: 1200 };
  }
}

export function ceremonyReducer(state: CeremonyState, event: CeremonyEvent): CeremonyState {
  switch (event.type) {
    case "PROGRESS": {
      // 单调不减：回放旧 seq / 乱序事件被 clamp 掉。
      const progress = Math.min(1, Math.max(state.progress, event.progress));
      if (progress === state.progress) return state;
      return { ...state, progress };
    }

    case "ENTER_DONE": {
      if (state.phase !== "entering") return state;
      // 快机：入场期间 ready 已到 → 跳过 streaming 直达 readyHold。
      return { ...state, phase: state.ready ? "readyHold" : "streaming" };
    }

    case "READY": {
      if (state.ready) return state; // 幂等：重复 ready（回放）不重触发
      const ready = true;
      if (state.phase === "streaming") return { ...state, ready, phase: "readyHold" };
      // entering：等 ENTER_DONE 再进 readyHold；exiting/done：跳过或已退出，仅记账。
      return { ...state, ready };
    }

    case "SKIP": {
      if (state.phase === "exiting" || state.phase === "done") return state;
      if (state.progress < SKIP_THRESHOLD) return state; // <30% 拒绝（UI 另行如实提示）
      return { ...state, phase: "exiting", skipped: true };
    }

    case "EXIT_DONE": {
      // 自然流：readyHold 计时到点 → 宿主走退出编排，结束时 EXIT_DONE → done。
      // 跳过流：SKIP 已进入 exiting，编排结束同样 EXIT_DONE → done。
      if (state.phase !== "exiting" && state.phase !== "readyHold") return state;
      return { ...state, phase: "done" };
    }
  }
}
