import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  INPUT_EVENT_CHANNEL,
  currentFocus,
  dispatchInputEvent,
  installInputBus,
  registerTarget,
  requestClipboard,
  resetInputBusForTest,
  setClipboardArbiter,
  setFocus,
  setFocusFallback,
  type KernelInputEvent,
} from "../inputBus";

const key = (k: string, down = true): KernelInputEvent => ({
  kind: "key",
  code: `Key${k}`,
  key: k,
  down,
  ctrl: false,
  alt: false,
  shift: false,
});

beforeEach(() => {
  resetInputBusForTest();
  installInputBus();
});
afterEach(() => resetInputBusForTest());

describe("焦点模型", () => {
  it("焦点栈：栈顶收键，置顶幂等，退订自动出栈", () => {
    const a = { id: "win-a", focusable: true, onKeyDown: vi.fn() };
    const b = { id: "win-b", focusable: true, onKeyDown: vi.fn() };
    const offA = registerTarget(a);
    registerTarget(b);
    expect(setFocus("win-a")).toBe(true);
    expect(setFocus("win-a")).toBe(true);
    expect(currentFocus()).toBe("win-a");
    setFocus("win-b");
    dispatchInputEvent(key("X"));
    expect(b.onKeyDown).toHaveBeenCalledTimes(1);
    expect(a.onKeyDown).not.toHaveBeenCalled();
    offA();
    expect(currentFocus()).toBe("win-b");
  });

  it("不可收键目标被跳过（占位卡/幕帘），焦点落到下一个可收键层", () => {
    const curtain = { id: "curtain", focusable: true, onKeyDown: vi.fn() };
    const app = { id: "app", focusable: true, onKeyDown: vi.fn() };
    registerTarget(curtain);
    registerTarget(app);
    setFocus("app");
    // 幕帘置顶但不可收键 → 按键穿透给 app（等价幕帘语义：屏蔽交互但不吞键记录）
    curtain.focusable = false;
    setFocus("curtain");
    dispatchInputEvent(key("Q"));
    expect(app.onKeyDown).toHaveBeenCalledTimes(1);
    expect(curtain.onKeyDown).not.toHaveBeenCalled();
  });

  it("焦点切换 1000 次循环无串键：每次只有栈顶目标收到自己的键", () => {
    const a = { id: "a", focusable: true, onKeyDown: vi.fn() };
    const b = { id: "b", focusable: true, onKeyDown: vi.fn() };
    registerTarget(a);
    registerTarget(b);
    for (let i = 0; i < 1000; i++) {
      const top = i % 2 === 0 ? "a" : "b";
      expect(setFocus(top)).toBe(true);
      dispatchInputEvent(key(i % 2 === 0 ? "A" : "B"));
    }
    expect(a.onKeyDown).toHaveBeenCalledTimes(500);
    expect(b.onKeyDown).toHaveBeenCalledTimes(500);
    // 无串键：a 只在 a 置顶轮收到
    expect(a.onKeyDown.mock.calls.every(([e]) => e.key === "A")).toBe(true);
    expect(b.onKeyDown.mock.calls.every(([e]) => e.key === "B")).toBe(true);
  });

  it("兜底目标：无焦点时桌面壳收键；兜底失效则事件无副作用丢弃", () => {
    const shell = { id: "shell", focusable: true, onKeyDown: vi.fn() };
    registerTarget(shell);
    setFocusFallback("shell");
    dispatchInputEvent(key("S"));
    expect(shell.onKeyDown).toHaveBeenCalledTimes(1);
    setFocusFallback("gone");
    expect(dispatchInputEvent(key("S"))).toBe(false);
    expect(shell.onKeyDown).toHaveBeenCalledTimes(1);
  });
});

describe("事件路由与守卫", () => {
  it("未安装总线时事件被丢弃（内核未接线时的安全默认）", () => {
    resetInputBusForTest();
    const t = { id: "t", focusable: true, onKeyDown: vi.fn() };
    registerTarget(t);
    expect(dispatchInputEvent(key("K"))).toBe(false);
    expect(t.onKeyDown).not.toHaveBeenCalled();
  });

  it("pointer/text 事件按 action 分发到对应回调", () => {
    const t = {
      id: "t",
      focusable: true,
      onPointer: vi.fn(),
      onText: vi.fn(),
    };
    registerTarget(t);
    setFocus("t");
    dispatchInputEvent({ kind: "pointer", action: "move", x: 1, y: 2, button: 0 });
    dispatchInputEvent({ kind: "text", text: "中" });
    expect(t.onPointer).toHaveBeenCalledWith(expect.objectContaining({ action: "move", x: 1 }));
    expect(t.onText).toHaveBeenCalledWith(expect.objectContaining({ text: "中" }));
  });

  it("剪贴板白名单预留点：默认拒绝，仲裁器只对授权目标放行", () => {
    const t = { id: "editor", focusable: true };
    registerTarget(t);
    expect(requestClipboard("editor")).toBe(false);
    setClipboardArbiter((id) => id === "editor");
    expect(requestClipboard("editor")).toBe(true);
    expect(requestClipboard("other")).toBe(false);
  });

  it("输出频道名是协议事件表登记的 shim://input", () => {
    expect(INPUT_EVENT_CHANNEL).toBe("shim://input");
  });
});
