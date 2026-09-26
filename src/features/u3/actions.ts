/**
 * I 通用域 · AI-U3 交互动作路由中心（vx-u3-action 的系统接线端）。
 *
 * 对齐 J 鼠标域 actions.ts 哲学（一处一事实，同一套路由语义）：
 * - v2 及之前：U3Runtime 的个别出口（瞥桌面 CSS 变量/剪贴板热键）是散装
 *   直连——本模块把 U3 全部交互出口收拢为「登记表 + 两级路由 + 未处理
 *   显性化」的统一动作面（章十四开放扩展点）；
 * - 两级路由：应用作用域处理器（registerU3ActionHandler 带 appId）→ 全局
 *   处理器 → 未处理；handler 返回 false 显式声明「我没接住」继续下探；
 * - 未处理显性化（十三·补）：无人接的动作不算「触发成功」——返回
 *   handled=false、logger 留痕，绝不静默吞掉；
 * - 别名语义：`window-N:<n>` 派发 vx-u3-winnum（F535 任务栏消费）、
 *   `lockscreen:reason` 派发 vx-u3-lock-screen（F238 锁屏消费）。
 *
 * 动作登记表（一处一事实，F 编号语义）：
 * | 动作 | 来源判据 | 语义 |
 * | window.minimize-mid | F520 | 标题栏中键最小化 |
 * | window.pin-top | F548 | 任务管理器置顶切换 |
 * | window.peek | F537 | 瞥桌面按下/松开 |
 * | desktop.lock-layout | F539 | 布局锁定开关查询与拒绝反馈 |
 * | desktop.resnap | F503 | 网格密度变化重排 |
 * | deskicon.label-style | F501/F502 | 图标文字双层渲染参数 |
 * | winnum.activate | F535 | Win+数字激活任务栏第 N 位 |
 * | clicklock.drop | F542 | ClickLock 抓起态放下/放弃 |
 * | bin.undo-empty | F524 | 清空回收站后悔窗撤销 |
 * | clip.wipe | F511 | 剪贴板一键清空 |
 * | lockscreen.now | F505/F504 | 动态锁触发锁屏 |
 * | ime.toggle | F518 | 输入法切换（四方案出口） |
 */

import { logWarn } from "../../lib/logger";

export type U3ActionSource = "runtime" | "panel" | "api";

/** 动作作用域提示（与 J1 同构：window/desktop/view/sys）。 */
export type U3ActionScope = "window" | "desktop" | "view" | "sys";

export interface U3ActionMeta {
  action: string;
  fno: string;
  name: string;
  desc: string;
  scope: U3ActionScope;
}

/** 内置动作全量登记（12 条——上表的一处一事实载体）。 */
export const U3_ACTION_REGISTRY: U3ActionMeta[] = [
  { action: "window.minimize-mid", fno: "F520", name: "中键最小化", desc: "标题栏中键点击 → 窗口最小化（三义分流之一）。", scope: "window" },
  { action: "window.pin-top", fno: "F548", name: "管理器置顶切换", desc: "任务管理器形制窗口的置顶开关（F248 语义不抢焦点）。", scope: "window" },
  { action: "window.peek", fno: "F537", name: "瞥桌面", desc: "Win+逗号按住/松开 → 窗口层 15% 透明纯看。", scope: "window" },
  { action: "desktop.lock-layout", fno: "F539", name: "布局锁定", desc: "锁定开关查询与拖拽拒绝反馈（抖动+状态栏一句话）。", scope: "desktop" },
  { action: "desktop.resnap", fno: "F503", name: "网格重排", desc: "密度切换后图标按最近格吸附重排（相对位置保持）。", scope: "desktop" },
  { action: "deskicon.label-style", fno: "F501/F502", name: "图标文字样式", desc: "双层渲染参数（投影/选字色/两行封顶）下发桌面。", scope: "desktop" },
  { action: "winnum.activate", fno: "F535", name: "Win+数字激活", desc: "按序激活任务栏第 N 位（启动/切换/最小化三态）。", scope: "sys" },
  { action: "clicklock.drop", fno: "F542", name: "ClickLock 放下", desc: "抓起态的单击放下 / Esc 放弃出口。", scope: "sys" },
  { action: "bin.undo-empty", fno: "F524", name: "撤销清空回收站", desc: "后悔窗内整批还原（暂存项全回）。", scope: "sys" },
  { action: "clip.wipe", fno: "F511", name: "剪贴板清空", desc: "当前+历史全清（确认后执行）。", scope: "sys" },
  { action: "lockscreen.now", fno: "F505/F504", name: "立即锁屏", desc: "动态锁触发或手动锁屏（F238 接管）。", scope: "sys" },
  { action: "ime.toggle", fno: "F518", name: "输入法切换", desc: "四方案切换键的统一出口（与 F421 点击循环并存）。", scope: "sys" },
];

export function findU3ActionMeta(action: string): U3ActionMeta | null {
  return U3_ACTION_REGISTRY.find((a) => a.action === action) ?? null;
}

/* ------------------------------- 处理器注册 ------------------------------- */

export type U3ActionHandler = (detail: {
  action: string;
  source: U3ActionSource;
  /** 触发时所在应用作用域（data-app-id；无声明为 null）。 */
  appScope: string | null;
  /** 动作参数（winnum.activate 的 n / peek 的 pressed 等）。 */
  arg?: unknown;
}) => void | boolean | Promise<void | boolean>;

interface Registration {
  id: number;
  action: string;
  appId: string | null; // null = 全局
  handler: U3ActionHandler;
}

let nextRegId = 1;
const registrations: Registration[] = [];

/** 注册动作处理器（章十四开放扩展点）；返回退订函数。 */
export function registerU3ActionHandler(
  action: string,
  handler: U3ActionHandler,
  opts: { appId?: string } = {},
): () => void {
  if (!action.trim()) throw new Error("[u3:actions] registerU3ActionHandler 需要非空 action");
  const reg: Registration = { id: nextRegId++, action, appId: opts.appId ?? null, handler };
  registrations.push(reg);
  return () => {
    const i = registrations.indexOf(reg);
    if (i >= 0) registrations.splice(i, 1);
  };
}

/** 当前登记快照（审计面板与测试同源；只读副本）。 */
export function u3ActionHandlerSnapshot(): Array<{ action: string; appId: string | null }> {
  return registrations.map((r) => ({ action: r.action, appId: r.appId }));
}

/* ------------------------------- 派发与路由 ------------------------------- */

export interface U3DispatchResult {
  handled: boolean;
  via: "app" | "global" | "alias" | "unhandled";
}

/** 事件化出口的环境守卫（无 DOM 环境零崩溃）。 */
function emit(name: string, detail: Record<string, unknown>): void {
  if (typeof window === "undefined" || typeof window.dispatchEvent !== "function" || typeof CustomEvent !== "function") return;
  window.dispatchEvent(new CustomEvent(name, { detail }));
}

/**
 * 统一动作出口（U3Runtime/面板唯一调用点）：
 * 1. 别名动作（winnum-N:/lockscreen:）→ 派发对应系统事件；
 * 2. 应用作用域处理器 → 全局处理器（返回 false 视为未接住继续下探）；
 * 3. 无人接 → handled=false + logger 留痕（异常零静默）。
 */
export async function dispatchU3Action(
  action: string,
  source: U3ActionSource,
  arg?: unknown,
): Promise<U3DispatchResult> {
  const appScope =
    typeof document !== "undefined"
      ? (document.querySelector("[data-app-id]")?.getAttribute("data-app-id") ?? null)
      : null;

  // 别名：F535 Win+数字（winnum-N: 任务栏消费）
  if (action.startsWith("winnum-")) {
    emit("vx-u3-winnum", { n: Number(action.slice(7)), source });
    return { handled: true, via: "alias" };
  }
  // 别名：锁屏（F238 消费）
  if (action.startsWith("lockscreen:")) {
    emit("vx-u3-lock-screen", { reason: action.slice(11), source });
    return { handled: true, via: "alias" };
  }

  const run = async (level: "app" | "global"): Promise<boolean> => {
    for (const reg of [...registrations]) {
      if (reg.action !== action) continue;
      if (level === "app" && reg.appId === null) continue;
      if (level === "global" && reg.appId !== null) continue;
      try {
        const r = await reg.handler({ action, source, appScope, arg });
        if (r !== false) return true;
      } catch (e) {
        logWarn("[u3:actions]", `处理器 ${action}(${reg.appId ?? "global"}) 抛错——继续路由: ${String(e)}`);
      }
    }
    return false;
  };

  if (appScope && (await run("app"))) return { handled: true, via: "app" };
  if (await run("global")) return { handled: true, via: "global" };

  logWarn("[u3:actions]", `动作 ${action} 无消费者（no-feedback 显性化）`);
  return { handled: false, via: "unhandled" };
}
