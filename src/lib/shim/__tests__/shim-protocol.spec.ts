import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  installShimTransport,
  onShimEvent,
  resetShimTransportForTest,
  shimCapability,
  shimHello,
  shimInvoke,
  ShimMappedError,
  ShimMissingError,
  dispatchShimEvent,
  type ShimTransport,
} from "../shimInvoke";

/** 传输形态三段式（协议规范 §3）：载荷直达 | __shim_error | __shim_missing。 */
function transportOf(responder: (cmd: string) => unknown): ShimTransport {
  return vi.fn(async (cmd: string) => responder(cmd));
}

beforeEach(() => resetShimTransportForTest());
afterEach(() => resetShimTransportForTest());

describe("垫片协议三段式", () => {
  it("OK 路径：载荷直达原样返回", async () => {
    installShimTransport(transportOf(() => ({ n: 42 })));
    await expect(shimInvoke("doc_list")).resolves.toEqual({ n: 42 });
  });

  it("MAPPED_ERR 路径：__shim_error 解出 code/message/retryable 抛 ShimMappedError", async () => {
    installShimTransport(
      transportOf(() => ({ __shim_error: { code: "SHIM_PERM_DENIED", message: "白名单拒绝" } })),
    );
    let err: unknown;
    try {
      await shimInvoke("doc_save");
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(ShimMappedError);
    expect((err as ShimMappedError).code).toBe("SHIM_PERM_DENIED");
    expect((err as ShimMappedError).retryable).toBe(false);
    expect((err as ShimMappedError).message).toBe("白名单拒绝");
  });

  it("MISSING 路径：__shim_missing 解出 cmd 抛 ShimMissingError，降级不产生副作用", async () => {
    let sideEffect = 0;
    installShimTransport(
      transportOf((cmd) => {
        sideEffect += cmd === "wine_launch" ? 0 : 1;
        return { __shim_missing: cmd };
      }),
    );
    const err = await shimInvoke("wine_launch").catch((e) => e);
    expect(err).toBeInstanceOf(ShimMissingError);
    expect((err as ShimMissingError).cmd).toBe("wine_launch");
    expect(sideEffect).toBe(0);
  });
});

describe("版本协商 shim_hello", () => {
  it("兼容应答：记录能力位并返回 hello", async () => {
    const t = transportOf(() => ({
      protocolVersion: 1,
      backendVersion: 1,
      capabilities: ["inputBus", "kvStore"],
    }));
    installShimTransport(t);
    const hello = await shimHello();
    expect(hello.capabilities).toContain("inputBus");
    expect(shimCapability("inputBus")).toBe(true);
    expect(shimCapability("wineChannel")).toBe(false);
    const calls = (t as unknown as { mock: { calls: string[][] } }).mock.calls;
    expect(calls[0]?.[0]).toBe("shim_hello");
  });

  it("版本不兼容：抛 SHIM_VERSION_MISMATCH 且不记录能力位", async () => {
    installShimTransport(
      transportOf(() => ({ protocolVersion: 2, backendVersion: 1, capabilities: [] })),
    );
    await expect(shimHello()).rejects.toMatchObject({ code: "SHIM_VERSION_MISMATCH" });
    expect(shimCapability("inputBus")).toBe(false);
  });
});

describe("传输层与事件反向通道", () => {
  it("未安装传输层时 invoke 报 SHIM_BACKEND_DOWN（不静默）", async () => {
    await expect(shimInvoke("doc_list")).rejects.toMatchObject({ code: "SHIM_BACKEND_DOWN" });
  });

  it("重复安装传输层报 SHIM_INVALID_ARGS", () => {
    installShimTransport(transportOf(() => null));
    expect(() => installShimTransport(transportOf(() => null))).toThrow(ShimMappedError);
  });

  it("事件分发：订阅通道收到载荷，未订阅通道不触发，退订生效", () => {
    const a = vi.fn();
    const b = vi.fn();
    const offA = onShimEvent("boot://event", a);
    onShimEvent("settings://changed", b);
    dispatchShimEvent("boot://event", { phase: "loading" });
    expect(a).toHaveBeenCalledWith({ phase: "loading" });
    expect(b).not.toHaveBeenCalled();
    offA();
    dispatchShimEvent("boot://event", { phase: "done" });
    expect(a).toHaveBeenCalledTimes(1);
  });
});
