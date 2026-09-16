import type { QuickSection } from "./uiStore";
import type { NotifyItem } from "./notifyStore";
import type { VwmWin } from "../system/windows/vwm";

/**
 * M4-B 跨窗投影协议（桌面窗 = 权威，任务栏独立窗 = 投影）：
 *
 * 任务栏搬出桌面 WebView 后，Taskbar/StartMenu 消费的状态（vwm 窗口表 / 通知 /
 * startOpen 等）仍由桌面窗权威持有。协议三向：
 * 1. 桌面 → 任务栏快照（taskbar://snapshot，seq 单调防乱序；vwm/notify/ui 三面）；
 * 2. 任务栏 → 桌面命令（taskbar://vwm-cmd / ui-set / notify-cmd / desktop-cmd / exit）；
 * 3. 任务栏窗 → 后端 hitmap 上报（交互矩形 + 菜单开合，驱动点击穿透状态机）。
 *
 * 本模块对 vitest / 浏览器 dev 零副作用：角色探测无 __TAURI_INTERNALS__ 时
 * 恒 false，全部 guard 退化为本地执行；emit 均为动态 import + 失败静默。
 */

export const DESKTOP_LABEL = "desktop";
export const TASKBAR_LABEL = "taskbar";

/** 当前窗口 label（无 Tauri 运行时 → null）。 */
export function currentWindowLabel(): string | null {
  try {
    const internals = (
      window as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }
    ).__TAURI_INTERNALS__;
    return internals?.metadata?.currentWindow?.label ?? null;
  } catch {
    return null;
  }
}

/** 本窗是否任务栏投影窗（vitest / 浏览器 dev 恒 false）。 */
export function isTaskbarProjection(): boolean {
  return currentWindowLabel() === TASKBAR_LABEL;
}

/** 本窗是否桌面权威窗。 */
export function isDesktopAuthority(): boolean {
  return currentWindowLabel() === DESKTOP_LABEL;
}

function fire(target: string, event: string, payload: unknown): void {
  void import("@tauri-apps/api/event")
    .then(({ emitTo }) => emitTo(target, event, payload))
    .catch(() => {});
}

// ---------------------------------------------------------------------------
// 任务栏投影窗 → 桌面权威窗（命令转发）
// ---------------------------------------------------------------------------

/** vwm.ts 中被任务栏树直接调用、需要转发到桌面执行的导出函数白名单。 */
export type VwmCmdFn =
  | "openVwmApp"
  | "openVwmSystem"
  | "focusVwmWin"
  | "closeVwmWin"
  | "taskbarClickVwm";

export function forwardVwm(fn: VwmCmdFn, args: unknown[]): void {
  fire(DESKTOP_LABEL, "taskbar://vwm-cmd", { fn, args });
}

/** 允许转发到桌面的 uiStore 键（任务栏树内会触碰的桌面状态面；其余键仅本地）。 */
const UI_FORWARD_KEYS = new Set([
  "startOpen",
  "quickOpen",
  "quickSection",
  "searchOpen",
  "searchInitialQuery",
  "launcherOpen",
  "launcherTab",
  "aiHubOpen",
  "settingsOpen",
  "settingsTab",
]);

/** 收窄到白名单键（发送/接收两侧都过一道，防未知键穿透）。 */
export function sanitizeUiPatch(patch: Record<string, unknown>): Record<string, unknown> {
  const picked: Record<string, unknown> = {};
  for (const k of Object.keys(patch)) {
    if (UI_FORWARD_KEYS.has(k)) picked[k] = patch[k];
  }
  return picked;
}

export function forwardUiPatch(patch: Partial<Record<string, unknown>>): void {
  const picked = sanitizeUiPatch(patch);
  if (Object.keys(picked).length === 0) return;
  fire(DESKTOP_LABEL, "taskbar://ui-set", picked);
}

/** notifyStore 中任务栏树会调用、需要桌面同步的导出函数白名单。 */
export type NotifyCmdFn =
  | "markAllRead"
  | "clearNotifications"
  | "toggleDnd"
  | "recordNotifyAction";

export function forwardNotify(fn: NotifyCmdFn, args: unknown[]): void {
  fire(DESKTOP_LABEL, "taskbar://notify-cmd", { fn, args });
}

/** 任务栏树内派发、由桌面窗消费的 window CustomEvent 白名单（挂载协议/通知动作）。 */
export const DESKTOP_EVENT_FORWARD = new Set(["variable:notify-action", "ai04:open-feature"]);

export function forwardDesktopEvent(name: string, detail: unknown): void {
  fire(DESKTOP_LABEL, "taskbar://desktop-cmd", { name, detail });
}

/** 退出 Variable（桌面窗执行完整保存冲刷 + 关闭编排）。 */
export function forwardExit(): void {
  fire(DESKTOP_LABEL, "taskbar://exit", {});
}

// ---------------------------------------------------------------------------
// 快照（桌面 → 任务栏）
// ---------------------------------------------------------------------------

export interface ProjectionSnapshot {
  seq: number;
  vwm: { wins: VwmWin[]; focusedId: string | null };
  notify: { items: unknown[] };
  ui: { startOpen: boolean; quickOpen: boolean; quickSection: QuickSection | null };
}

/**
 * 桌面权威窗：三 store 订阅 → 节流快照广播 + 任务栏命令接收。
 * 返回清理函数（DesktopShell 卸载时解绑）。
 */
export async function initDesktopProjection(opts: { onExit?: () => void }): Promise<() => void> {
  const [
    { vwmStore, openVwmApp, openVwmSystem, focusVwmWin, closeVwmWin, taskbarClickVwm },
    { notifyStore, markAllRead, clearNotifications, toggleDnd, recordNotifyAction },
    { uiStore },
    { listen },
  ] = await Promise.all([
    import("../system/windows/vwm"),
    import("./notifyStore"),
    import("./uiStore"),
    import("@tauri-apps/api/event"),
  ]);

  let seq = 0;
  let timer = 0;
  const broadcast = (): void => {
    const v = vwmStore.getState();
    const n = notifyStore.getState();
    const u = uiStore.getState();
    seq += 1;
    const payload: ProjectionSnapshot = {
      seq,
      vwm: { wins: v.wins, focusedId: v.focusedId },
      notify: { items: n.items },
      ui: { startOpen: u.startOpen, quickOpen: u.quickOpen, quickSection: u.quickSection },
    };
    fire(TASKBAR_LABEL, "taskbar://snapshot", payload);
  };
  // 80ms 尾节流：拖拽/轮询高频变更不刷爆事件通道
  const schedule = (): void => {
    if (timer) return;
    timer = window.setTimeout(() => {
      timer = 0;
      broadcast();
    }, 80);
  };

  const unsubs: Array<() => void> = [];
  unsubs.push(vwmStore.subscribe(schedule));
  unsubs.push(notifyStore.subscribe(schedule));
  unsubs.push(uiStore.subscribe(schedule));
  // 心跳兜底：任何一路事件丢失（对端 listen 建立前发出）最多 5s 自愈
  const hb = window.setInterval(broadcast, 5000);
  unsubs.push(() => window.clearInterval(hb));

  const listenWrap = async (event: string, handler: (payload: unknown) => void): Promise<void> => {
    try {
      const un = await listen<unknown>(event, (e) => handler(e.payload));
      unsubs.push(un);
    } catch {
      /* 浏览器 dev：无事件通道 */
    }
  };

  const VWM_FNS: Record<string, (args: unknown[]) => void> = {
    openVwmApp: (a) =>
      openVwmApp(
        a[0] as Parameters<typeof openVwmApp>[0],
        (a[1] ?? undefined) as Parameters<typeof openVwmApp>[1],
      ),
    openVwmSystem: (a) =>
      openVwmSystem(
        a[0] as Parameters<typeof openVwmSystem>[0],
        (a[1] ?? undefined) as Parameters<typeof openVwmSystem>[1],
      ),
    focusVwmWin: (a) => focusVwmWin(a[0] as string),
    closeVwmWin: (a) => closeVwmWin(a[0] as string),
    taskbarClickVwm: (a) => taskbarClickVwm(a[0] as Parameters<typeof taskbarClickVwm>[0]),
  };
  await listenWrap("taskbar://vwm-cmd", (p) => {
    const cmd = p as { fn?: string; args?: unknown[] };
    const fn = cmd?.fn ? VWM_FNS[cmd.fn] : undefined;
    if (fn && Array.isArray(cmd.args)) fn(cmd.args);
  });

  await listenWrap("taskbar://ui-set", (p) => {
    if (p && typeof p === "object") {
      uiStore.setState(sanitizeUiPatch(p as Record<string, unknown>) as Parameters<typeof uiStore.setState>[0]);
    }
  });

  const NOTIFY_FNS: Record<string, (args: unknown[]) => void> = {
    markAllRead: () => markAllRead(),
    clearNotifications: () => clearNotifications(),
    toggleDnd: () => toggleDnd(),
    recordNotifyAction: (a) =>
      recordNotifyAction(
        a[0] as Parameters<typeof recordNotifyAction>[0],
        a[1] as Parameters<typeof recordNotifyAction>[1],
      ),
  };
  await listenWrap("taskbar://notify-cmd", (p) => {
    const cmd = p as { fn?: string; args?: unknown[] };
    const fn = cmd?.fn ? NOTIFY_FNS[cmd.fn] : undefined;
    if (fn && Array.isArray(cmd.args)) fn(cmd.args);
  });

  await listenWrap("taskbar://desktop-cmd", (p) => {
    const cmd = p as { name?: string; detail?: unknown };
    if (cmd?.name && DESKTOP_EVENT_FORWARD.has(cmd.name)) {
      window.dispatchEvent(new CustomEvent(cmd.name, { detail: cmd.detail }));
    }
  });

  await listenWrap("taskbar://exit", () => opts.onExit?.());
  await listenWrap("taskbar://hello", () => broadcast());

  broadcast(); // 首包（对端未就绪时由 hello / 心跳补齐）
  return () => {
    for (const u of unsubs) u();
    if (timer) window.clearTimeout(timer);
  };
}

/**
 * 任务栏投影窗：订阅快照并落地本地 store（绕过转发包装，防回声环）。
 * 挂载后向桌面发 hello（带重试，覆盖 listen 建立时序差）。
 */
export async function initProjectionConsumer(): Promise<() => void> {
  const [
    { vwmStore },
    { notifyStore },
    { applyProjectionUiPatch },
    { listen },
  ] = await Promise.all([
    import("../system/windows/vwm"),
    import("./notifyStore"),
    import("./uiStore"),
    import("@tauri-apps/api/event"),
  ]);

  let lastSeq = 0;
  const unsubs: Array<() => void> = [];
  try {
    const un = await listen<ProjectionSnapshot>("taskbar://snapshot", (e) => {
      const p = e.payload;
      if (!p || typeof p.seq !== "number" || p.seq <= lastSeq) return; // 乱序/重复丢弃
      lastSeq = p.seq;
      vwmStore.setState({
        wins: p.vwm?.wins ?? [],
        focusedId: p.vwm?.focusedId ?? null,
      });
      notifyStore.setState({ items: (p.notify?.items as NotifyItem[] | undefined) ?? [] });
      applyProjectionUiPatch({
        startOpen: !!p.ui?.startOpen,
        quickOpen: !!p.ui?.quickOpen,
        quickSection: p.ui?.quickSection ?? null,
      });
    });
    unsubs.push(un);
  } catch {
    /* 浏览器 dev */
  }
  // hello 重试：桌面首包可能早于本窗 listen 建立
  for (let i = 0; i < 6; i += 1) {
    fire(DESKTOP_LABEL, "taskbar://hello", {});
    await new Promise((r) => window.setTimeout(r, 500));
  }
  return () => {
    for (const u of unsubs) u();
  };
}

// ---------------------------------------------------------------------------
// hitmap（任务栏窗交互矩形 → 后端点击穿透状态机）
// ---------------------------------------------------------------------------

/** 宿主容器不产生命中矩形（全屏透明窗），只下探其子树。 */
function isHostContainer(el: Element): boolean {
  return (el as HTMLElement).id === "root" || el.classList.contains("taskbar-window-root");
}

/**
 * 收集任务栏树内可交互元素矩形（逻辑 px）：
 * - 交互元素（pointer-events != none）压入自身 rect，并继续下探（弹出层可能逃出容器矩形）；
 * - 穿透层（pointer-events = none）不下压矩形但继续下探（子树可重新开启交互，
 *   如 .toast-host none + .toast auto）；
 * - display:none / 零尺寸跳过。
 */
export function collectHitRects(): Array<[number, number, number, number]> {
  const out: Array<[number, number, number, number]> = [];
  const walk = (el: Element, depth: number): void => {
    if (depth > 20) return;
    for (const child of Array.from(el.children)) {
      if (isHostContainer(child)) {
        walk(child, depth + 1);
        continue;
      }
      const cs = window.getComputedStyle(child);
      if (cs.display === "none" || cs.visibility === "hidden") continue;
      const r = child.getBoundingClientRect();
      if (r.width < 1 || r.height < 1) continue;
      if (cs.pointerEvents !== "none") {
        out.push([r.x, r.y, r.width, r.height]);
        for (const sub of Array.from(child.children)) walk(sub, depth + 1);
      } else {
        walk(child, depth + 1);
      }
    }
  };
  walk(document.body, 0);
  return out;
}

/**
 * hitmap 上报循环：250ms 兜底节流 + DOM 突变即时上报（80ms 抖动）。
 * menu_open = 开始菜单/快捷面板（React 态）或任一浮层（DOM 态）开启 ——
 * 后端钉住显示并全窗放行（外点关闭菜单的语义依赖它）。
 */
export function startHitmapReporting(isMenuPinned: () => boolean): () => void {
  let stopped = false;
  let last = 0;
  const report = (): void => {
    if (stopped || !isTaskbarProjection()) return;
    const now = Date.now();
    if (now - last < 80) return;
    last = now;
    const rects = collectHitRects();
    const menuOpen =
      isMenuPinned() ||
      !!document.querySelector(
        ".ctx-menu,.modal-overlay,.card-pop,.quick-panel,.start-menu,.tray-drawer",
      );
    void import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke("taskbar_report_hitmap_cmd", { rects, menuOpen }))
      .catch(() => {});
  };
  report();
  const id = window.setInterval(report, 250);
  const mo = new MutationObserver(() => report());
  mo.observe(document.body, { childList: true, subtree: true, attributes: true, characterData: true });
  return () => {
    stopped = true;
    window.clearInterval(id);
    mo.disconnect();
  };
}
