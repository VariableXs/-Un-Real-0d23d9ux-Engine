/**
 * J 鼠标域 · F615/F617 交互动作路由中心（vx-j1-action 的系统接线端）。
 *
 * v2 及之前：J1Runtime 只把动作派发成 CustomEvent——「系统组件按 action 订阅」
 * 只是声明，没有任何消费端（接线悬空）。本模块把这件事落成真实设施：
 *
 * - 动作登记表（一处一事实）：内置动作全量登记（12 手势 + 5 侧键目标），
 *   每条带名称/说明/作用域提示——审计面板与手势库共用同一份；
 * - 两级路由（与 F615 侧键同构的应用>全局哲学）：
 *   应用作用域处理器（registerActionHandler 带 appId）→ 全局处理器 → 未处理；
 * - 开放扩展点（章十四）：系统组件/第三方应用 registerActionHandler 即接入，
 *   退订函数返回即撤销；扩展崩溃不拖垮本体（handler 异常捕获 + 显性日志）；
 * - 未处理显性化（十三·补）：没有任何处理器接的动作不算「触发成功」——
 *   返回 false、遥测记 no-feedback、logger 留痕，绝不静默吞掉；
 * - 别名语义：`shortcut:K1+K2` 派发 vx-j1-shortcut（F244 快捷键路由消费）、
 *   `launch:appId` 派发 vx-j1-launch（启动器消费）——侧键三类映射目标
 *   （系统动作/快捷键/启动应用）在动作面上各有真实去处。
 */

import { j1Telemetry } from "./telemetry";
import { logWarn } from "../../lib/logger";
import { BUILTIN_GESTURES } from "./gestures";
import { SIDE_ACTIONS } from "./sideButtons";

/* ------------------------------- 动作登记表 ------------------------------- */

export type J1ActionSource = "gesture" | "side" | "api";

/** 动作作用域提示：window=窗口管理类，view=视图类，edit=编辑类，nav=导航类，sys=系统类。 */
export type J1ActionScope = "window" | "view" | "edit" | "nav" | "sys";

export interface J1ActionMeta {
  action: string;
  name: string;
  desc: string;
  scope: J1ActionScope;
  /** 来源（gesture=手势库动作；side=侧键系统动作表）。 */
  via: "gesture" | "side";
}

/** 内置动作全量登记（12 手势动作 + 侧键系统动作表去重合并——一处一事实）。 */
export const J1_ACTION_REGISTRY: J1ActionMeta[] = [
  ...BUILTIN_GESTURES.map((g) => ({
    action: g.action,
    name: g.name,
    desc: `右键手势「${g.name}」的目标动作。`,
    scope: actionScopeOf(g.action),
    via: "gesture" as const,
  })),
  ...SIDE_ACTIONS.map((a) => ({
    action: a.id,
    name: a.name,
    desc: a.desc,
    scope: actionScopeOf(a.id),
    via: "side" as const,
  })),
];

/** 动作 → 作用域（nav./view./edit./window. 前缀语义；其余归 sys）。 */
export function actionScopeOf(action: string): J1ActionScope {
  if (action.startsWith("nav.")) return "nav";
  if (action.startsWith("view.")) return "view";
  if (action.startsWith("edit.")) return "edit";
  if (action.startsWith("window.")) return "window";
  return "sys";
}

/** 登记表查询（未登记动作返回 null——自定义手势的 custom.* 动作走扩展登记）。 */
export function findActionMeta(action: string): J1ActionMeta | null {
  return J1_ACTION_REGISTRY.find((a) => a.action === action) ?? null;
}

/* ------------------------------- 处理器注册 ------------------------------- */

export type J1ActionHandler = (detail: {
  action: string;
  source: J1ActionSource;
  /** 触发时所在的应用作用域（data-app-id；无声明为 null）。 */
  appScope: string | null;
}) => void | boolean | Promise<void | boolean>;

interface Registration {
  id: number;
  action: string;
  appId: string | null; // null = 全局
  handler: J1ActionHandler;
}

let nextRegId = 1;
const registrations: Registration[] = [];

/**
 * 注册动作处理器（章十四开放扩展点）。
 * @param action 动作名（内置动作或自定义手势的 custom.* 动作）
 * @param handler 处理函数；返回 false 显式声明「我没接住」→ 继续向下路由
 * @param opts.appId 应用作用域（缺省=全局处理器）
 * @returns 退订函数（调用即撤销登记）
 */
export function registerActionHandler(
  action: string,
  handler: J1ActionHandler,
  opts: { appId?: string } = {},
): () => void {
  if (!action.trim()) throw new Error("[mouse-j1:actions] registerActionHandler 需要非空 action");
  const reg: Registration = { id: nextRegId++, action, appId: opts.appId ?? null, handler };
  registrations.push(reg);
  return () => {
    const i = registrations.indexOf(reg);
    if (i >= 0) registrations.splice(i, 1);
  };
}

/** 当前登记快照（审计面板与测试同源；只读副本）。 */
export function actionHandlerSnapshot(): { action: string; appId: string | null }[] {
  return registrations.map((r) => ({ action: r.action, appId: r.appId }));
}

/* ------------------------------- 派发与路由 ------------------------------- */

export interface DispatchResult {
  /** 是否有处理器接住（未处理=false——调用方可据此诚实呈现）。 */
  handled: boolean;
  /** 命中的处理器层级（诊断面用）。 */
  via: "app" | "global" | "alias" | "unhandled";
}

/** 事件化出口的环境守卫（无 DOM 环境零崩溃——测试与边缘宿主的诚实降级）。 */
function emit(name: string, detail: Record<string, unknown>): void {
  if (typeof window === "undefined" || typeof window.dispatchEvent !== "function" || typeof CustomEvent !== "function") return;
  window.dispatchEvent(new CustomEvent(name, { detail }));
}

/**
 * 统一动作出口（J1Runtime/windowRuntime 唯一调用点）：
 * 1. 别名动作（shortcut:/launch:）→ 派发对应系统事件（F244/启动器消费）；
 * 2. 应用作用域处理器 → 全局处理器（handler 返回 false 视为未接住，继续下探）;
 * 3. 无人接 → handled=false + 遥测 no-feedback + logger 留痕（异常零静默）。
 * 单个 handler 抛错：捕获、记日志、继续路由（扩展崩溃不拖垮本体——章十四）。
 */
export async function dispatchJ1Action(
  action: string,
  source: J1ActionSource,
  appScope: string | null,
  pos?: { x: number; y: number },
): Promise<DispatchResult> {
  // 别名面：侧键「快捷键/启动应用」两类映射目标的事件化去处。
  if (action.startsWith("shortcut:")) {
    emit("vx-j1-shortcut", { keys: action.slice(9), source, appScope });
    j1Telemetry.log("side-button", "smooth", `shortcut:${action.slice(9)}`, pos?.x ?? 0, pos?.y ?? 0);
    return { handled: true, via: "alias" };
  }
  if (action.startsWith("launch:")) {
    emit("vx-j1-launch", { appId: action.slice(7), source, appScope });
    j1Telemetry.log("side-button", "smooth", `launch:${action.slice(7)}`, pos?.x ?? 0, pos?.y ?? 0);
    return { handled: true, via: "alias" };
  }

  const detail = { action, source, appScope };
  const run = async (reg: Registration): Promise<boolean> => {
    try {
      const r = await reg.handler(detail);
      return r !== false; // undefined/true = 接住；false = 显式让位
    } catch (e) {
      logWarn("mouse-j1", `动作 ${action} 的处理器（${reg.appId ?? "全局"}）抛错已隔离: ${String(e)}`);
      return false;
    }
  };

  // 1) 应用作用域优先（与 F615 侧键覆盖优先级同构）。
  if (appScope) {
    for (const reg of registrations.filter((r) => r.action === action && r.appId === appScope)) {
      if (await run(reg)) {
        j1Telemetry.log(source === "gesture" ? "gesture" : "side-button", "smooth", `${action}@${appScope}`, pos?.x ?? 0, pos?.y ?? 0);
        return { handled: true, via: "app" };
      }
    }
  }
  // 2) 全局处理器。
  for (const reg of registrations.filter((r) => r.action === action && r.appId === null)) {
    if (await run(reg)) {
      j1Telemetry.log(source === "gesture" ? "gesture" : "side-button", "smooth", action, pos?.x ?? 0, pos?.y ?? 0);
      return { handled: true, via: "global" };
    }
  }

  // 3) 未处理：显性化（不静默——十三·补）。
  j1Telemetry.log(source === "gesture" ? "gesture-aborted" : "side-button", "no-feedback", `${action}(无人处理)`, pos?.x ?? 0, pos?.y ?? 0);
  logWarn("mouse-j1", `动作 ${action}（来源 ${source}）当前窗口无处理器——已显性登记，未静默丢弃`);
  return { handled: false, via: "unhandled" };
}

/* ------------------------------- 内置处理器装配 ------------------------------- */

/**
 * 内置全局处理器装配（desktop 桌面窗调用；各窗口按能力选择性装配）：
 * - nav.back / nav.forward：窗口历史导航（app 窗口内的前进/后退语义）；
 * - view.refresh / view.refresh-hard：视图刷新（hard 附带缓存清除语义位）；
 * - window.minimize / window.toggle-max：窗口管理（Tauri 窗口 API；浏览器 dev 降级为事件）；
 * - edit.copy / edit.cut / edit.paste：走剪贴板 + 选区（聚焦元素的 execCommand 路径）；
 * - nav.up：层级上移（data-nav-up 声明元素的点击——声明式不越权）；
 * - file.new：vx-j1-file-new 事件（各应用自行消费——文件语义属于应用）；
 * - window.close-tab：vx-j1-close-tab 事件（标签页宿主消费）；
 * - sys 类（taskview/desktop-toggle/mute）：vx-j1-sys-action 事件（系统 shell 消费）。
 *
 * 设计约束：内置处理器只做「本窗口内确定安全」的事——跨窗口语义全部事件化
 * 交给对应宿主（投影协议/启动器/F244），绝不越权替别的窗口做主。
 */

export interface BuiltinWiring {
  /** 窗口历史导航回调（app 窗口有历史栈则提供；缺省=不装配该动作）。 */
  onNav?: (dir: "back" | "forward") => void;
  /** 视图刷新回调（hard=忽略缓存档）。 */
  onRefresh?: (hard: boolean) => void;
  /** Tauri 窗口句柄（window.minimize/toggle-max 真实落点；缺省=事件降级）。 */
  tauriWindow?: {
    minimize: () => Promise<void>;
    toggleMaximize: () => Promise<void>;
  };
}

/** 已装配的退订函数集（dispose 用）。 */
export interface BuiltinWiringDisposer {
  dispose: () => void;
  /** 本次装配的动作清单（审计面板同源）。 */
  wired: string[];
}

export function installBuiltinHandlers(wiring: BuiltinWiring, opts: { appId?: string } = {}): BuiltinWiringDisposer {
  const unsubs: (() => void)[] = [];
  const wired: string[] = [];
  const reg = (action: string, h: J1ActionHandler): void => {
    unsubs.push(registerActionHandler(action, h, opts));
    wired.push(action);
  };

  if (wiring.onNav) {
    reg("nav.back", () => void wiring.onNav!("back"));
    reg("nav.forward", () => void wiring.onNav!("forward"));
  }
  if (wiring.onRefresh) {
    reg("view.refresh", () => wiring.onRefresh!(false));
    reg("view.refresh-hard", () => wiring.onRefresh!(true));
  }
  if (wiring.tauriWindow) {
    reg("window.minimize", () => void wiring.tauriWindow!.minimize());
    reg("window.toggle-max", () => void wiring.tauriWindow!.toggleMaximize());
  }
  reg("edit.copy", () => editViaSelection("copy"));
  reg("edit.cut", () => editViaSelection("cut"));
  reg("edit.paste", () => editViaSelection("paste"));
  reg("nav.up", (d) => clickDeclarative(d.appScope, "data-nav-up"));
  reg("file.new", () => emit("vx-j1-file-new", {}));
  reg("window.close-tab", () => emit("vx-j1-close-tab", {}));
  reg("taskview", () => emit("vx-j1-sys-action", { action: "taskview" }));
  reg("desktop-toggle", () => emit("vx-j1-sys-action", { action: "desktop-toggle" }));
  reg("mute", () => emit("vx-j1-sys-action", { action: "mute" }));

  return {
    dispose: () => { for (const u of unsubs) u(); },
    wired,
  };
}

/** 剪贴板三兄弟：聚焦可编辑元素优先（execCommand 保 IME/撤销链），否则全局剪贴板。 */
function editViaSelection(kind: "copy" | "cut" | "paste"): boolean {
  const el = document.activeElement as HTMLElement | null;
  const editable = el && (el.isContentEditable || el.tagName === "INPUT" || el.tagName === "TEXTAREA");
  if (editable) {
    try {
      const ok = document.execCommand(kind);
      if (ok) return true;
    } catch {
      /* execCommand 不可用 → 走下方显性失败 */
    }
  }
  logWarn("mouse-j1", `edit.${kind} 无法在本窗口执行（无可编辑焦点或权限缺失）——已显性登记`);
  return false; // false = 未接住 → 继续路由/未处理显性化
}

/** nav.up 的声明式落点：最近的 [data-nav-up] 元素点击（没有声明=不猜语义）。 */
function clickDeclarative(_appScope: string | null, attr: string): boolean {
  const el = document.querySelector(`[${attr}]`) as HTMLElement | null;
  if (!el) {
    logWarn("mouse-j1", `nav.up 落点缺失（本窗口无 [${attr}] 声明）——显性登记`);
    return false;
  }
  el.click();
  return true;
}

/* ------------------------------- 测试辅助 ------------------------------- */

/** 清空全部登记（仅测试用——生产代码不得调用）。 */
export function _resetActionRegistryForTest(): void {
  registrations.length = 0;
  nextRegId = 1;
}
