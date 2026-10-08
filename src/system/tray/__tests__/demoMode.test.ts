/**
 * AI-20 V-91 演示模式红线测试（实施步骤 P6 验收门禁原文）：
 * - 进出 20 次状态零残留；
 * - 进程被杀场景的钩子恢复测试（beforeunload + 启动自愈兜底）；
 * - 用户既有偏好不被吞（快照恢复原样）；
 * - 与 V-54 唤醒定时正交（退出只解除演示开启的唤醒，display=false）。
 * DOM/IPC 真链路由 dogfood/实机验收覆盖；此处 mock settings/ipc 锁死恢复语义。
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const h = vi.hoisted(() => {
  const keepawakeCalls: Array<[boolean, boolean]> = [];
  const savedInputFeel: Array<{ trailEnabled: boolean; rippleEnabled: boolean }> = [];
  const settingsState: { inputFeel: { trailEnabled: boolean; rippleEnabled: boolean } } = {
    inputFeel: { trailEnabled: false, rippleEnabled: false },
  };
  return { keepawakeCalls, savedInputFeel, settingsState };
});

vi.mock("../../../lib/settings", () => ({
  loadSettings: vi.fn(async () => h.settingsState),
  saveSetting: vi.fn(async (_key: string, value: { trailEnabled: boolean; rippleEnabled: boolean }) => {
    h.savedInputFeel.push({ trailEnabled: value.trailEnabled, rippleEnabled: value.rippleEnabled });
    h.settingsState.inputFeel = { trailEnabled: value.trailEnabled, rippleEnabled: value.rippleEnabled };
  }),
}));

vi.mock("../../../lib/ipc11", () => ({
  ipc11: {
    keepawakeSet: vi.fn(async (on: boolean, display: boolean) => {
      h.keepawakeCalls.push([on, display]);
    }),
  },
}));

vi.mock("../../../state/uiStore", () => ({
  pushToast: vi.fn(),
}));

import { notifyStore } from "../../../state/notifyStore";
import {
  toggleDemoMode, exitDemoMode, demoModeActive, installDemoModeExitHook,
  recoverDemoModeOnBoot, DEMO_MODE_KEY,
} from "../DemoMode";

// node 测试环境无真实 window（setup.ts 只兜底定时器）：本文件只用到
// beforeunload 事件总线，stub 最小实现（addEventListener/dispatchEvent）。
type Handler = (ev: Event) => void;
const listeners = new Map<string, Set<Handler>>();
vi.stubGlobal("window", {
  addEventListener: (type: string, fn: Handler) => {
    if (!listeners.has(type)) listeners.set(type, new Set());
    listeners.get(type)?.add(fn);
  },
  removeEventListener: (type: string, fn: Handler) => {
    listeners.get(type)?.delete(fn);
  },
  dispatchEvent: (ev: Event) => {
    for (const fn of [...(listeners.get(ev.type) ?? [])]) fn(ev);
    return true;
  },
});

function resetState(dnd: boolean, trail: boolean, ripple: boolean): void {
  notifyStore.setState({ dnd });
  h.settingsState.inputFeel = { trailEnabled: trail, rippleEnabled: ripple };
  h.keepawakeCalls.length = 0;
  h.savedInputFeel.length = 0;
  localStorage.clear();
}

// 模块级 snapshot 会跨用例泄漏：每个用例先退出残留演示态（幂等），再重置。
beforeEach(async () => {
  await exitDemoMode();
  resetState(false, false, false);
});

describe("AI-20 V-91：演示模式进入/退出语义", () => {
  beforeEach(() => resetState(false, false, false));

  it("进入：免打扰 + 唤醒（display=true）+ 指针增强 + 持久快照落盘", async () => {
    await toggleDemoMode();
    expect(demoModeActive()).toBe(true);
    expect(notifyStore.getState().dnd).toBe(true);
    expect(h.keepawakeCalls).toEqual([[true, true]]);
    expect(h.savedInputFeel).toEqual([{ trailEnabled: true, rippleEnabled: true }]);
    const raw = localStorage.getItem(DEMO_MODE_KEY);
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw as string)).toEqual({ dnd: false, trailEnabled: false, rippleEnabled: false });
  });

  it("再次 toggle = 退出：全部原样恢复、零残留", async () => {
    await toggleDemoMode();
    await toggleDemoMode();
    expect(demoModeActive()).toBe(false);
    expect(notifyStore.getState().dnd).toBe(false);
    expect(h.keepawakeCalls).toEqual([[true, true], [false, false]]);
    expect(h.settingsState.inputFeel).toEqual({ trailEnabled: false, rippleEnabled: false });
    expect(localStorage.getItem(DEMO_MODE_KEY)).toBeNull();
  });

  it("用户既有偏好不被吞：全开进入 → 零重复保存、退出原样", async () => {
    resetState(true, true, true);
    await toggleDemoMode();
    expect(h.savedInputFeel).toEqual([]); // 已开启则不重复保存
    expect(notifyStore.getState().dnd).toBe(true); // 本就勿扰，不吞不改
    await exitDemoMode();
    expect(h.savedInputFeel).toEqual([]);
    expect(notifyStore.getState().dnd).toBe(true);
    expect(demoModeActive()).toBe(false);
  });

  it("混合偏好：只还原被临时改动的项（trail 开、ripple 保持用户已开）", async () => {
    resetState(false, false, true);
    await toggleDemoMode();
    expect(h.savedInputFeel).toEqual([{ trailEnabled: true, rippleEnabled: true }]);
    await exitDemoMode();
    expect(h.settingsState.inputFeel).toEqual({ trailEnabled: false, rippleEnabled: true });
    expect(localStorage.getItem(DEMO_MODE_KEY)).toBeNull();
  });

  it("无快照退出 no-op：不解除唤醒、不写设置", async () => {
    await exitDemoMode();
    expect(h.keepawakeCalls).toEqual([]);
    expect(h.savedInputFeel).toEqual([]);
  });
});

describe("AI-20 V-91 红线：进出 20 次状态零残留", () => {
  beforeEach(() => resetState(false, false, false));

  it("20 轮进/出：dnd/设置/标记/keepawake 全部归位且对称", async () => {
    for (let i = 0; i < 20; i++) {
      await toggleDemoMode();
      expect(demoModeActive()).toBe(true);
      await toggleDemoMode();
      expect(demoModeActive()).toBe(false);
    }
    expect(notifyStore.getState().dnd).toBe(false);
    expect(h.settingsState.inputFeel).toEqual({ trailEnabled: false, rippleEnabled: false });
    expect(localStorage.getItem(DEMO_MODE_KEY)).toBeNull();
    // keepawake 20 对 [true,true] / [false,false] 严格对称
    expect(h.keepawakeCalls).toHaveLength(40);
    for (let i = 0; i < 20; i++) {
      expect(h.keepawakeCalls[i * 2]).toEqual([true, true]);
      expect(h.keepawakeCalls[i * 2 + 1]).toEqual([false, false]);
    }
    // 设置保存 20 对（开→关），最终值回到初始
    expect(h.savedInputFeel).toHaveLength(40);
    expect(h.savedInputFeel[h.savedInputFeel.length - 1]).toEqual({ trailEnabled: false, rippleEnabled: false });
  });
});

describe("AI-20 V-91 红线：进程被杀场景的钩子恢复", () => {
  beforeEach(() => {
    resetState(false, false, false);
    installDemoModeExitHook();
  });
  afterEach(() => {
    window.dispatchEvent(new Event("beforeunload")); // 清理可能的剩余演示态
  });

  it("beforeunload：dnd/设置/标记恢复，快照置空，唤醒解除与 V-54 正交（display=false）", async () => {
    await toggleDemoMode();
    expect(notifyStore.getState().dnd).toBe(true);
    window.dispatchEvent(new Event("beforeunload"));
    // restoreFromSnapshot 为异步 fire-and-forget，等微任务队列清空
    await vi.waitFor(() => {
      expect(notifyStore.getState().dnd).toBe(false);
      expect(h.settingsState.inputFeel).toEqual({ trailEnabled: false, rippleEnabled: false });
    });
    expect(demoModeActive()).toBe(false);
    expect(localStorage.getItem(DEMO_MODE_KEY)).toBeNull();
    expect(h.keepawakeCalls.some((c) => c[0] === false && c[1] === false)).toBe(true);
    // 正交：退出解除只关唤醒（display=false），绝不再碰 display=true 的用户定时
    expect(h.keepawakeCalls).not.toContainEqual([false, true]);
  });

  it("钩子幂等：重复安装不叠加（一次 beforeunload 只恢复一次）", async () => {
    installDemoModeExitHook();
    installDemoModeExitHook();
    await toggleDemoMode();
    const before = h.keepawakeCalls.length;
    window.dispatchEvent(new Event("beforeunload"));
    await vi.waitFor(() => expect(h.keepawakeCalls.length).toBeGreaterThan(before));
    expect(h.keepawakeCalls.filter((c) => c[0] === false)).toHaveLength(1);
  });
});

describe("AI-20 V-91 红线：进程被强杀 → 下次会话启动自愈", () => {
  beforeEach(() => resetState(false, false, false));

  it("标记留存（强杀未触发 beforeunload）→ recoverDemoModeOnBoot 恢复原状并清标记", async () => {
    // 会话 A：进入演示后被强杀（磁盘上 trail/ripple=true + 标记留存）
    await toggleDemoMode();
    expect(h.settingsState.inputFeel).toEqual({ trailEnabled: true, rippleEnabled: true });
    // —— 模拟新会话：重置模块注册表（snapshot 归零），仅持久层（标记 + settings 磁盘态）留存
    vi.resetModules();
    h.keepawakeCalls.length = 0;
    h.savedInputFeel.length = 0;
    const fresh = await import("../DemoMode");
    const freshNotify = await import("../../../state/notifyStore");
    expect(fresh.demoModeActive()).toBe(false); // 新会话模块态为空
    await fresh.recoverDemoModeOnBoot();
    expect(freshNotify.notifyStore.getState().dnd).toBe(false);
    expect(h.settingsState.inputFeel).toEqual({ trailEnabled: false, rippleEnabled: false });
    expect(localStorage.getItem(DEMO_MODE_KEY)).toBeNull();
    expect(h.keepawakeCalls.some((c) => c[0] === false && c[1] === false)).toBe(true);
  });

  it("无标记启动：no-op（不解除唤醒、不写设置）", async () => {
    await recoverDemoModeOnBoot();
    expect(h.keepawakeCalls).toEqual([]);
    expect(h.savedInputFeel).toEqual([]);
  });

  it("损坏标记：如实清除、不误恢复", async () => {
    localStorage.setItem(DEMO_MODE_KEY, "{corrupted");
    await recoverDemoModeOnBoot();
    expect(localStorage.getItem(DEMO_MODE_KEY)).toBeNull();
    expect(h.savedInputFeel).toEqual([]);
  });

  it("本会话已在演示中：启动自愈让位（不抢正在进行的演示）", async () => {
    await toggleDemoMode();
    const savesBefore = h.savedInputFeel.length;
    await recoverDemoModeOnBoot();
    expect(demoModeActive()).toBe(true);
    expect(h.savedInputFeel.length).toBe(savesBefore); // 未额外写设置
  });
});
