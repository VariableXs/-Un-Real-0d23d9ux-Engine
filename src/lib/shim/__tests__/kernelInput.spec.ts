import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BTN_LEFT,
  BTN_MIDDLE,
  BTN_RIGHT,
  KEY_A,
  KEY_D0,
  KEY_D1,
  KEY_Z,
  KernelInputDecoder,
  SHIM_INPUT_EVENT_SIZE,
  connectKernelInput,
} from "../kernelInput";
import {
  dispatchInputEvent,
  installInputBus,
  registerTarget,
  resetInputBusForTest,
  setFocus,
} from "../inputBus";

/** 任务 19 实机逐字节核对向量（docs/acceptance/2026-09-17-任务19-输入服务化/实机串口-确认轮verdict-true.log seq71..76）。 */
const REAL_MACHINE: Array<{ seq: number; hex: string }> = [
  { seq: 71, hex: "47000000000000000001000000000000" }, // Key Down(方向键↓) 按下
  { seq: 72, hex: "48000000000000000000000000000000" }, // Key Up(方向键↑) 按下
  { seq: 73, hex: "49000000000000000002000000000000" }, // Key Enter 按下
  { seq: 74, hex: "4a0000000000000001002800e2ff0000" }, // Mouse dx=40 dy=-30
  { seq: 75, hex: "4b000000000000000100e2ff14000000" }, // Mouse dx=-30 dy=20
  { seq: 76, hex: "4c000000000000000100000000000100" }, // Mouse 左键按下（位移零）
];

function bytesOf(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

function makeEvent(seq: number, kind: number, key = 0, dx = 0, dy = 0, buttons = 0): Uint8Array {
  const b = new Uint8Array(16);
  const view = new DataView(b.buffer);
  view.setBigUint64(0, BigInt(seq), true);
  b[8] = kind;
  b[9] = key;
  view.setInt16(10, dx, true);
  view.setInt16(12, dy, true);
  b[14] = buttons;
  return b;
}

beforeEach(() => {
  resetInputBusForTest();
  installInputBus();
});
afterEach(() => resetInputBusForTest());

describe("shim://input 16B 契约解码（实机向量逐字段核对）", () => {
  it("实机 seq71..76 六向量：解码产物与任务 19 实机记录逐字段一致", () => {
    const got: Array<Record<string, unknown>> = [];
    const d = new KernelInputDecoder((e) => got.push({ ...e } as Record<string, unknown>));
    for (const v of REAL_MACHINE) {
      const r = d.feed(bytesOf(v.hex));
      expect(r.ok).toBe(true);
      expect((r as { seq: number }).seq).toBe(v.seq);
    }
    expect(got[0]).toMatchObject({ kind: "key", code: "ArrowDown", key: "ArrowDown", down: true });
    expect(got[1]).toMatchObject({ kind: "key", code: "ArrowUp", key: "ArrowUp", down: true });
    expect(got[2]).toMatchObject({ kind: "key", code: "Enter", key: "Enter", down: true });
    // seq74: dx=40 dy=-30 → 光标 (40,-30)，位移帧
    expect(got[3]).toMatchObject({ kind: "pointer", action: "move", x: 40, y: -30, button: 0 });
    // seq75: dx=-30 dy=20 → 光标 (10,-10)
    expect(got[4]).toMatchObject({ kind: "pointer", action: "move", x: 10, y: -10 });
    // seq76: 位移零 + buttons bit0 → 不发位移帧，只发左键 down 边沿
    expect(got[5]).toMatchObject({ kind: "pointer", action: "down", x: 10, y: -10, button: BTN_LEFT });
    const s = d.getStats();
    expect(s.decoded).toBe(6);
    expect(s.rejected).toBe(0);
    expect(s.missed).toBe(0);
    expect(s.duplicates).toBe(0);
    expect(s.lastSeq).toBe(76);
  });

  it("序号缺口：seq 跳号计入 missed（内核丢包前端可见）", () => {
    const d = new KernelInputDecoder(vi.fn());
    expect(d.feed(makeEvent(1, 0, 0)).ok).toBe(true);
    expect(d.feed(makeEvent(4, 0, 0)).ok).toBe(true);
    expect(d.getStats().missed).toBe(2);
    expect(d.getStats().lastSeq).toBe(4);
  });

  it("重复/回放去重：同 seq 重复喂入拒收且计数", () => {
    const d = new KernelInputDecoder(vi.fn());
    d.feed(makeEvent(1, 0, 0));
    const r = d.feed(makeEvent(1, 0, 0));
    expect(r).toMatchObject({ ok: false, reason: "seq" });
    expect(d.getStats().duplicates).toBe(1);
  });

  it("契约违规拒绝：截断/序号0/pad非0/未知kind/键帧携带脏字段", () => {
    const d = new KernelInputDecoder(vi.fn());
    expect(d.feed(new Uint8Array(15))).toMatchObject({ ok: false, reason: "len" });
    const zero = makeEvent(0, 0, 0);
    expect(d.feed(zero)).toMatchObject({ ok: false, reason: "seq" });
    const pad = makeEvent(1, 0, 0);
    pad[15] = 1;
    expect(d.feed(pad)).toMatchObject({ ok: false, reason: "pad" });
    expect(d.feed(makeEvent(2, 2))).toMatchObject({ ok: false, reason: "kind" });
    const dirtyKey = makeEvent(3, 0, 2, 5); // 键帧 dx 必须 0
    expect(d.feed(dirtyKey)).toMatchObject({ ok: false, reason: "key" });
    const unknownKey = makeEvent(4, 0, 99); // 任务 55 扩表后 0..57 全量合法；99 仍未知 → 拒绝计数
    expect(d.feed(unknownKey)).toMatchObject({ ok: false, reason: "key" });
    expect(d.getStats().rejected).toBe(6);
    expect(d.getStats().decoded).toBe(0);
  });

  it("任务 55 键表同源扩表：0..57 全量可解码，字母/数字/IME 依赖键逐段核对（与内核 key_byte 同序）", () => {
    const got: Array<{ code: string; key: string }> = [];
    const d = new KernelInputDecoder((e) => {
      if (e.kind === "key") got.push({ code: e.code, key: e.key });
    });
    let seq = 0;
    for (let kb = 0; kb <= 57; kb += 1) {
      seq += 1;
      const r = d.feed(makeEvent(seq, 0, kb));
      expect(r.ok, `key byte ${kb} 应合法`).toBe(true);
    }
    // 逐段抽核（生成式注册防手抄错位，但表本身仍需抽核锚点）
    expect(got[0]).toEqual({ code: "ArrowUp", key: "ArrowUp" });
    expect(got[2]).toEqual({ code: "Enter", key: "Enter" });
    expect(got[4]).toEqual({ code: "Space", key: " " });
    expect(got[KEY_A]).toEqual({ code: "KeyA", key: "a" });
    expect(got[KEY_Z]).toEqual({ code: "KeyZ", key: "z" });
    expect(got[KEY_D1]).toEqual({ code: "Digit1", key: "1" });
    expect(got[KEY_D0]).toEqual({ code: "Digit0", key: "0" });
    expect(got[47]).toEqual({ code: "Minus", key: "-" });
    expect(got[57]).toEqual({ code: "Backquote", key: "`" });
    // IME 专项锚点：拼音组合依赖字母 + 数字标调（ni3 → "n" "i" "3"）全可达
    const byCode = new Map(got.map((g) => [g.code, g.key]));
    expect(byCode.get("KeyN")).toBe("n");
    expect(byCode.get("KeyI")).toBe("i");
    expect(byCode.get("Digit3")).toBe("3");
    expect(d.getStats().decoded).toBe(58);
    expect(d.getStats().rejected).toBe(0);
  });

  it("按钮边沿：按下/抬起由前端推导（内核只报状态位），三位全覆盖", () => {
    const got: Array<{ action: string; button: number }> = [];
    const d = new KernelInputDecoder((e) => {
      if (e.kind === "pointer" && e.action !== "move") got.push({ action: e.action, button: e.button });
    });
    d.feed(makeEvent(1, 1, 0, 0, 0, BTN_LEFT | BTN_RIGHT));
    d.feed(makeEvent(2, 1, 0, 0, 0, BTN_MIDDLE));
    d.feed(makeEvent(3, 1, 0, 0, 0, 0));
    expect(got).toEqual([
      { action: "down", button: BTN_LEFT },
      { action: "down", button: BTN_RIGHT },
      { action: "up", button: BTN_LEFT },
      { action: "up", button: BTN_RIGHT },
      { action: "down", button: BTN_MIDDLE },
      { action: "up", button: BTN_MIDDLE },
    ]);
  });

  it("光标累计与视口钳制：未设置不钳制，设置后夹到 [0, size-1]", () => {
    const d = new KernelInputDecoder(vi.fn());
    d.feed(makeEvent(1, 1, 0, 5000, 5000));
    expect(d.getCursor()).toEqual({ x: 5000, y: 5000 });
    d.setViewport(1920, 1080);
    d.feed(makeEvent(2, 1, 0, 100, 100));
    expect(d.getCursor()).toEqual({ x: 1919, y: 1079 });
    d.feed(makeEvent(3, 1, 0, -9999, -9999));
    expect(d.getCursor()).toEqual({ x: 0, y: 0 });
  });

  it("常量与契约尺寸：16B 定长", () => {
    expect(SHIM_INPUT_EVENT_SIZE).toBe(16);
    expect(BTN_LEFT).toBe(1);
    expect(BTN_RIGHT).toBe(2);
    expect(BTN_MIDDLE).toBe(4);
  });
});

describe("解码器 → 输入总线端到端（内核字节流驱动焦点路由）", () => {
  it("键事件按焦点路由到栈顶目标；鼠标位移/点击同样到达", () => {
    const { feed } = connectKernelInput();
    const keyLog: string[] = [];
    const ptrLog: string[] = [];
    const shell = {
      id: "shell",
      focusable: true,
      onKeyDown: vi.fn((e) => keyLog.push(`shell:${e.code}`)),
      onPointer: vi.fn((e) => ptrLog.push(`shell:${e.action}`)),
    };
    const app = {
      id: "app",
      focusable: true,
      onKeyDown: vi.fn((e) => keyLog.push(`app:${e.code}`)),
      onPointer: vi.fn((e) => ptrLog.push(`app:${e.action}`)),
    };
    registerTarget(shell);
    registerTarget(app);
    setFocus("app");
    feed(bytesOf(REAL_MACHINE[0]!.hex)); // ArrowDown → app
    feed(bytesOf(REAL_MACHINE[2]!.hex)); // Enter → app
    feed(bytesOf(REAL_MACHINE[3]!.hex)); // mouse move → app
    feed(bytesOf(REAL_MACHINE[5]!.hex)); // left press → app
    expect(keyLog).toEqual(["app:ArrowDown", "app:Enter"]);
    expect(shell.onKeyDown).not.toHaveBeenCalled();
    expect(ptrLog).toEqual(["app:move", "app:down"]);
  });

  it("总线未安装时解码仍成功但事件无副作用（内核未接线的安全默认）", () => {
    resetInputBusForTest(); // 不 installInputBus
    const { feed, decoder } = connectKernelInput();
    const r = feed(bytesOf(REAL_MACHINE[0]!.hex));
    expect(r.ok).toBe(true);
    expect(decoder.getStats().decoded).toBe(1);
    expect(dispatchInputEvent({ kind: "key", code: "X", key: "X", down: true, ctrl: false, alt: false, shift: false })).toBe(false);
  });
});
