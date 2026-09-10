import { createStore } from "../../lib/store";
import type { VwmApp, VwmWin } from "./vwm";
import { isTpApp } from "./vwm";

/**
 * AI-01 窗口手感组：标题栏右键菜单模型 + 逐应用不透明度记忆 + 挂起登记（纯逻辑，可单测）。
 * - Z-36 标题栏右键菜单（不透明度滑杆 + 置顶开关；总开关 winFeelOpacity 在设置页，默认 off）
 * - M-02 右键「卷起 / 展开」（嵌入 L1 窗口不支持卷帘 → 置灰并说明）
 * - M-06 挂起 / 恢复（仅嵌入登记的第三方进程开放）
 * - M-04 体检徽标文案模型（不自动杀进程，只给「再等等 / 结束进程」）
 */

export type WinFeelMenuItemId =
  | "roll"
  | "unroll"
  | "topmost"
  | "untopmost"
  | "suspend"
  | "resume"
  | "hide"
  | "saveLayout"
  | "applyLayout";

export interface WinFeelMenuItem {
  id: WinFeelMenuItemId;
  labelKey: string;
  disabled: boolean;
  /** 置灰原因说明（如实告知，不静默）。 */
  hintKey?: string;
}

/** M-02：嵌入 L1 窗口重父级后高度受宿主约束，强行卷会破坏渲染 → 如置灰并说明。 */
export function winFeelMenuItems(w: Pick<VwmWin, "app" | "state" | "rolledUp" | "topmost">, suspended: boolean): WinFeelMenuItem[] {
  const isTp = isTpApp(w.app);
  const rollSupported = w.state === "normal" && !isTp;
  const items: WinFeelMenuItem[] = [
    w.rolledUp
      ? { id: "unroll", labelKey: "wfMenuUnroll", disabled: !w.rolledUp }
      : { id: "roll", labelKey: "wfMenuRoll", disabled: !rollSupported, hintKey: isTp ? "wfRollTpHint" : undefined },
    {
      id: w.topmost ? "untopmost" : "topmost",
      labelKey: w.topmost ? "wfMenuUntopmost" : "wfMenuTopmost",
      disabled: false,
    },
  ];
  // M-06：仅嵌入登记的第三方进程提供挂起/恢复（四空间等环境内窗口本就可最小化零占用）
  if (isTp) {
    items.push({
      id: suspended ? "resume" : "suspend",
      labelKey: suspended ? "wfMenuResume" : "wfMenuSuspend",
      disabled: false,
    });
  }
  // 批次F：隐藏窗口（桌面+任务栏消失，进程与状态保留；Ctrl+Alt+H / 任务栏恢复）
  items.push({ id: "hide", labelKey: "wfMenuHide", disabled: false });
  return items;
}

// ---------- Z-36 逐应用不透明度记忆（localStorage，键 = 应用名） ----------

const OPACITY_KEY = "variable:vwm:app-opacity";

function loadOpacityMap(): Record<string, number> {
  try {
    const raw = JSON.parse(localStorage.getItem(OPACITY_KEY) ?? "{}") as Record<string, unknown>;
    const out: Record<string, number> = {};
    for (const [k, v] of Object.entries(raw)) {
      if (typeof v === "number" && v >= 0.2 && v <= 1) out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

/** Z-36：读取某应用的记忆透明度（无记录 → null = 用 1）。 */
export function appOpacityOf(app: VwmApp): number | null {
  return loadOpacityMap()[app] ?? null;
}

/** Z-36：记住某应用透明度（与全局默认一致的 1 不记忆）。 */
export function rememberAppOpacity(app: VwmApp, v: number): void {
  try {
    const all = loadOpacityMap();
    if (v >= 0.999) delete all[app];
    else all[app] = Math.round(v * 100) / 100;
    localStorage.setItem(OPACITY_KEY, JSON.stringify(all));
  } catch {
    /* storage blocked → 本次会话内不持久 */
  }
}

// ---------- M-06 挂起登记（前端镜像；恢复动作与自动恢复见 VirtualWindowManager） ----------

export const suspensionStore = createStore<{ suspended: Record<string, boolean> }>({ suspended: {} });

/** 该 VWM 窗口是否处于挂起态。 */
export function isSuspended(embedId: string): boolean {
  return suspensionStore.getState().suspended[embedId] === true;
}

export function setSuspended(embedId: string, on: boolean): void {
  suspensionStore.setState((s) => {
    if (!!s.suspended[embedId] === on) return s;
    const suspended = { ...s.suspended };
    if (on) suspended[embedId] = true;
    else delete suspended[embedId];
    return { suspended };
  });
}

/** 环境退出前自动恢复：返回全部挂起中的 embedId 列表并清空登记。 */
export function takeAllSuspended(): string[] {
  const ids = Object.keys(suspensionStore.getState().suspended);
  suspensionStore.setState({ suspended: {} });
  return ids;
}
