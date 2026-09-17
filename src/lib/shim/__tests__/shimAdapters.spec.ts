import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BootEventBuffer, type BootLoadEvent } from "../bootEvents";
import {
  VARIX_SHIM_GLOBAL,
  findEmbedderBridge,
  installEmbedderTransportIfPresent,
} from "../embedderTransport";
import { resetShimTransportForTest, shimInvoke, ShimMissingError } from "../shimInvoke";

beforeEach(() => {
  resetShimTransportForTest();
  delete (globalThis as Record<string, unknown>)[VARIX_SHIM_GLOBAL];
});
afterEach(() => {
  resetShimTransportForTest();
  delete (globalThis as Record<string, unknown>)[VARIX_SHIM_GLOBAL];
});

const ev = (seq: number, progress: number, currentTask = "加载"): BootLoadEvent => ({
  seq,
  progress,
  currentTask,
  icon: "📦",
  level: 0,
});

describe("boot://event 容错（总案阶段 3 步骤 8 验收项）", () => {
  it("重复事件去重：同 seq 回灌只计一次", () => {
    const buf = new BootEventBuffer();
    expect(buf.feed(ev(1, 0.1))).toBe(true);
    expect(buf.feed(ev(1, 0.1))).toBe(false);
    expect(buf.count).toBe(1);
  });

  it("乱序安全：按 seq 有序通知，消费者看到单调序列", () => {
    const buf = new BootEventBuffer();
    const seen: number[] = [];
    buf.onEvent((e) => seen.push(e.seq));
    buf.feedAll([ev(3, 0.3), ev(1, 0.1), ev(2, 0.2), ev(5, 0.5)]);
    expect(seen).toEqual([1, 2, 3]); // 4 未到，5 暂扣
    buf.feed(ev(4, 0.4));
    expect(seen).toEqual([1, 2, 3, 4, 5]);
  });

  it("进度单调夹取：展示值永不回退，事件真实值保留", () => {
    const buf = new BootEventBuffer();
    buf.feedAll([ev(1, 0.8), ev(2, 0.3), ev(3, 0.5)]);
    expect(buf.getProgress()).toBe(0.8);
    expect(buf.getLatest()).toMatchObject({ seq: 3, progress: 0.5 });
  });

  it("非法载荷拒绝：seq 非正/非有限", () => {
    const buf = new BootEventBuffer();
    expect(buf.feed(ev(0, 0))).toBe(false);
    expect(buf.feed({ seq: Number.NaN, progress: 0, currentTask: "x" })).toBe(false);
    expect(buf.count).toBe(0);
  });

  it("boot_replay 回灌语义：先到先喂，乱序安全，后续实时事件无缝续播", () => {
    const buf = new BootEventBuffer();
    const seen: number[] = [];
    buf.onEvent((e) => seen.push(e.seq));
    // 回放缓冲先到（乱序），再补实时事件
    expect(buf.feedAll([ev(2, 0.2), ev(1, 0.1)])).toBe(2);
    expect(buf.feed(ev(3, 0.4))).toBe(true);
    expect(seen).toEqual([1, 2, 3]);
    expect(buf.getProgress()).toBe(0.4);
  });
});

describe("embedder 传输适配（任务 27 前端侧）", () => {
  it("桥未注入 → findEmbedderBridge 为 null 且不安装传输层", () => {
    expect(findEmbedderBridge()).toBeNull();
    expect(installEmbedderTransportIfPresent()).toBe(false);
    expect(shimInvoke("doc_list")).rejects.toMatchObject({ code: "SHIM_BACKEND_DOWN" });
  });

  it("桥存在 → 安装成功，invoke 走桥且三段式解码生效", async () => {
    const bridge = vi.fn(async (cmd: string, args: Record<string, unknown>) => {
      if (cmd === "doc_list") return { docs: [], args };
      if (cmd === "wine_launch") return { __shim_missing: cmd };
      return { __shim_error: { code: "SHIM_TIMEOUT", message: "内核服务超时" } };
    });
    (globalThis as Record<string, unknown>)[VARIX_SHIM_GLOBAL] = bridge;
    expect(installEmbedderTransportIfPresent()).toBe(true);
    await expect(shimInvoke<{ docs: unknown[] }>("doc_list", { x: 1 })).resolves.toEqual({
      docs: [],
      args: { x: 1 },
    });
    await expect(shimInvoke("wine_launch")).rejects.toBeInstanceOf(ShimMissingError);
    await expect(shimInvoke("other")).rejects.toMatchObject({ code: "SHIM_TIMEOUT" });
    expect(bridge).toHaveBeenCalledWith("doc_list", { x: 1 });
  });
});
