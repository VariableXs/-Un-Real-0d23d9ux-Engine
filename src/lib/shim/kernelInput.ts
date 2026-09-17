/**
 * 内核输入事件解码器（任务 26 · 前端侧闭环件）。
 *
 * 对接 AI-K 任务 19 的 `shim://input` 16B 定长契约（kernel/varix/src/inputsvc.rs，
 * 实机逐字节核对基准）：
 * ```text
 * [0..8)   u64 seq      事件序号（服务级单调，自 1 起，LE）
 * [8]      u8  kind     0=Key 1=Mouse
 * [9]      u8  key      kind=Key：0=Up 1=Down 2=Enter；kind=Mouse：恒 0
 * [10..12) i16 dx       kind=Mouse：X 位移（有符号，LE）；kind=Key：恒 0
 * [12..14) i16 dy       kind=Mouse：Y 位移（有符号，LE）；kind=Key：恒 0
 * [14]     u8  buttons  kind=Mouse：bit0 左 bit1 右 bit2 中；kind=Key：恒 0
 * [15]     u8  pad      保留，恒 0
 * ```
 * 内核只发布 make（按下）事件（断码忽略，任务 19 实测定案）；
 * 按键抬起由前端按按钮位边沿推导。开放性：新输入设备只加 kind/key 取值，
 * 未知取值拒绝并计数（不猜测、不静默改语义）。
 */
import { dispatchInputEvent } from "./inputBus";
import type { KernelInputEvent, KernelKeyEvent } from "./inputBus";

export const SHIM_INPUT_EVENT_SIZE = 16;

export const KIND_KEY = 0;
export const KIND_MOUSE = 1;
/** 键名取值（契约固定声明序）。 */
export const KEY_UP = 0;
export const KEY_DOWN = 1;
export const KEY_ENTER = 2;
/** 鼠标按钮位（契约：bit0 左 bit1 右 bit2 中）。 */
export const BTN_LEFT = 1;
export const BTN_RIGHT = 2;
export const BTN_MIDDLE = 4;

/** 键名 → 前端 KeyboardEvent.code 对齐值（openTools/keymap 既有命名）。 */
const KEY_NAMES: Record<number, { code: string; key: string }> = {
  [KEY_UP]: { code: "ArrowUp", key: "ArrowUp" },
  [KEY_DOWN]: { code: "ArrowDown", key: "ArrowDown" },
  [KEY_ENTER]: { code: "Enter", key: "Enter" },
};

export type DecodeFailReason = "len" | "pad" | "kind" | "key" | "seq";

export type DecodeResult =
  | { ok: true; event: KernelInputEvent; seq: number }
  | { ok: false; reason: DecodeFailReason; seq: number };

export interface KernelInputStats {
  /** 成功解码并交付的事件数。 */
  decoded: number;
  /** 契约违规/未知取值被拒的事件数。 */
  rejected: number;
  /** 序号缺口：内核报告 dropped_oldest/丢包时前端可见的丢失数。 */
  missed: number;
  /** 重复/回放（seq ≤ lastSeq）被去重的事件数。 */
  duplicates: number;
  lastSeq: number;
}

export class KernelInputDecoder {
  private lastSeq = 0;
  private prevButtons = 0;
  private cursorX = 0;
  private cursorY = 0;
  private viewportW: number | null = null;
  private viewportH: number | null = null;
  private readonly stats: KernelInputStats = {
    decoded: 0,
    rejected: 0,
    missed: 0,
    duplicates: 0,
    lastSeq: 0,
  };

  constructor(private readonly sink: (event: KernelInputEvent) => void) {}

  /** 视口钳制（嵌入方随分辨率变化调用；未设置时不钳制）。 */
  setViewport(w: number, h: number): void {
    this.viewportW = w;
    this.viewportH = h;
  }

  getStats(): Readonly<KernelInputStats> {
    return { ...this.stats, lastSeq: this.lastSeq };
  }

  getCursor(): { x: number; y: number } {
    return { x: this.cursorX, y: this.cursorY };
  }

  /** 解码一条 16B 事件；交付走 sink。返回逐字段核对结果。 */
  feed(bytes: Uint8Array): DecodeResult {
    if (bytes.length !== SHIM_INPUT_EVENT_SIZE) return this.reject("len", 0);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const seq = Number(view.getBigUint64(0, true));
    if (view.getUint8(15) !== 0) return this.reject("pad", seq);
    if (seq === 0) return this.reject("seq", seq);
    if (seq <= this.lastSeq) {
      this.stats.duplicates += 1;
      return { ok: false, reason: "seq", seq };
    }
    if (this.lastSeq === 0) {
      // 首事件建立基线：订阅前历史不计缺口（对齐任务 19 订阅游标 write_seq+1 语义）
    } else if (seq > this.lastSeq + 1) {
      this.stats.missed += seq - this.lastSeq - 1;
    }
    this.lastSeq = seq;

    const kind = view.getUint8(8);
    if (kind === KIND_KEY) {
      const keyByte = view.getUint8(9);
      const dx = view.getInt16(10, true);
      const dy = view.getInt16(12, true);
      const buttons = view.getUint8(14);
      if (KEY_NAMES[keyByte] === undefined || dx !== 0 || dy !== 0 || buttons !== 0) {
        return this.reject("key", seq);
      }
      const named = KEY_NAMES[keyByte];
      const event: KernelKeyEvent = {
        kind: "key",
        code: named.code,
        key: named.key,
        down: true, // 内核只发布 make 事件（任务 19 断码忽略）
        ctrl: false,
        alt: false,
        shift: false,
      };
      this.stats.decoded += 1;
      this.sink(event);
      return { ok: true, event, seq };
    }
    if (kind === KIND_MOUSE) {
      if (view.getUint8(9) !== 0) return this.reject("kind", seq);
      const dx = view.getInt16(10, true);
      const dy = view.getInt16(12, true);
      const buttons = view.getUint8(14) & 0x07;
      this.cursorX = this.clampX(this.cursorX + dx);
      this.cursorY = this.clampY(this.cursorY + dy);
      this.emitPointerFrame(dx, dy, buttons);
      this.stats.decoded += 1;
      return { ok: true, event: { kind: "pointer", action: "move", x: this.cursorX, y: this.cursorY, button: buttons }, seq };
    }
    return this.reject("kind", seq);
  }

  /** 喂入一帧鼠标状态：位移帧 + 按钮位边沿帧（down/up 由前端推导）。 */
  private emitPointerFrame(dx: number, dy: number, buttons: number): void {
    if (dx !== 0 || dy !== 0) {
      this.sink({
        kind: "pointer",
        action: "move",
        x: this.cursorX,
        y: this.cursorY,
        button: buttons,
      });
    }
    const pressed = buttons & ~this.prevButtons;
    const released = this.prevButtons & ~buttons;
    for (const bit of [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE]) {
      if (pressed & bit) {
        this.sink({ kind: "pointer", action: "down", x: this.cursorX, y: this.cursorY, button: bit });
      }
      if (released & bit) {
        this.sink({ kind: "pointer", action: "up", x: this.cursorX, y: this.cursorY, button: bit });
      }
    }
    this.prevButtons = buttons;
  }

  private clampX(x: number): number {
    return this.viewportW === null ? x : Math.max(0, Math.min(this.viewportW - 1, x));
  }
  private clampY(y: number): number {
    return this.viewportH === null ? y : Math.max(0, Math.min(this.viewportH - 1, y));
  }

  private reject(reason: DecodeFailReason, seq: number): DecodeResult {
    this.stats.rejected += 1;
    return { ok: false, reason, seq };
  }
}

/** 组装：解码器 → 输入总线（垫片传输层把内核字节流喂给返回的 feed）。 */
export function connectKernelInput(): {
  feed: (bytes: Uint8Array) => DecodeResult;
  decoder: KernelInputDecoder;
} {
  const decoder = new KernelInputDecoder((e) => dispatchInputEvent(e));
  return { feed: (b) => decoder.feed(b), decoder };
}
