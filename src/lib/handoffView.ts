/**
 * 需求 2 的界面纯逻辑（与 `filesyncView.ts` 同一范式：把视图侧判断抽出来，
 * 让 node 环境能钉住行为——项目测试无 jsdom，组件本身不可测）。
 *
 * 这里只做一件事：把「共享盘 / 配置文件 / 两个开关」的状态翻译成**不会撒谎**
 * 的文案与可用性判断。诚实是硬要求——「没读到配置」绝不能说成「默认正常」，
 * 那是把不确定性伪装成结论。
 */
import type { Shell } from "./ipc";

/**
 * 配置读不到时的占位。`handoff: true` 与内核 `DEFAULT_HANDOFF_TO_VARIABLE`
 * 对齐：内核在缺这个键时按 true 处理，界面必须显示同一结论，否则用户会以为
 * 「关着」而实际「开着」。
 */
export const HANDOFF_FALLBACK: Shell.BootCfgView = {
  found: false,
  sharedRoot: "",
  path: "",
  handoff: true,
  handoffExplicit: false,
  timeoutSec: null,
  defaultEntry: "variable",
  showMenu: true,
};

/** 共享盘状态四态。 */
export type SharedState = "loading" | "no-shared" | "no-config" | "ready";

export function sharedState(cfg: Shell.BootCfgView | null): SharedState {
  if (cfg === null) {
    return "loading";
  }
  if (!cfg.sharedRoot) {
    return "no-shared";
  }
  return cfg.found ? "ready" : "no-config";
}

/** 共享盘状态一句话（用于卡片底部说明行）。 */
export function sharedLabel(cfg: Shell.BootCfgView | null): string {
  const st = sharedState(cfg);
  if (st === "loading") {
    return "正在读取共享盘配置…";
  }
  if (cfg === null) {
    return "正在读取共享盘配置…";
  }
  if (st === "no-shared") {
    return "未找到共享盘（SHARED）—— 请插入系统 U 盘后再改这些设置";
  }
  if (st === "no-config") {
    return `共享盘已就绪，但还没有 boot-select.json：${cfg.path}`;
  }
  // 显式写没写这个键，对用户是有效信息：没写就是跟着内核默认走。
  return cfg.handoffExplicit
    ? `共享盘配置：${cfg.path}`
    : `共享盘配置：${cfg.path}（handoff 用的是内核默认值）`;
}

/** 交接开关能否点击。 */
export function handoffDisabled(cfg: Shell.BootCfgView | null, busy: boolean): boolean {
  // 配置没读到就禁用：盲写一份配置可能把用户原有的设置覆盖掉。
  return busy || cfg === null;
}

/** 自启开关能否点击（自启状态读不到时 `auto` 为 null → 禁用）。 */
export function autostartDisabled(auto: { on: boolean } | null, busy: boolean): boolean {
  return busy || auto === null;
}

/**
 * 两个开关是否都到位（决定卡片顶部的"链路是否完整"提示）。
 *
 * 只有**两半都开**时，A 卡加载完才真能落到 Variable 桌面：
 * 内核侧不开 → 停在 ushell；Windows 侧不开 → 落到普通 Windows 桌面。
 */
export function chainComplete(cfg: Shell.BootCfgView | null, auto: { on: boolean } | null): boolean {
  return (cfg?.handoff ?? false) && (auto?.on ?? false);
}

/** 链路缺口提示；完整时返回空串（不画多余提示）。 */
export function chainHint(cfg: Shell.BootCfgView | null, auto: { on: boolean } | null): string {
  if (cfg === null || auto === null) {
    return "";
  }
  const noHandoff = !cfg.handoff;
  const noAuto = !auto.on;
  if (noHandoff && noAuto) {
    return "两半都没开：A 卡会停在 VARIX 自绘 ushell，交接链不会启动。";
  }
  if (noHandoff) {
    return "内核侧交接关着：A 卡会停在 VARIX 自绘 ushell。";
  }
  if (noAuto) {
    return "Windows 侧自启关着：交接过去只会看到普通 Windows 桌面，不会自动进 Variable。";
  }
  return "";
}
