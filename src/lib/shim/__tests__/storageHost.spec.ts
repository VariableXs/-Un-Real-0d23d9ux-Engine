import { afterEach, describe, expect, it, vi } from "vitest";
import { installKvStorageHost, uninstallKvStorageHostForTest } from "../storageHost";

/** 桩内核 KV（与 kvBridge.spec 同口径：ns 隔离 + kv_* 命令面）。 */
function makeKernelKv() {
  const store = new Map<string, Map<string, string>>();
  const invoke = vi.fn(async <T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
    const ns = String(args.ns);
    if (!store.has(ns)) store.set(ns, new Map());
    const bucket = store.get(ns)!;
    switch (cmd) {
      case "kv_keys":
        return [...bucket.keys()] as T;
      case "kv_get":
        return (bucket.get(String(args.key)) ?? null) as T;
      case "kv_set":
        bucket.set(String(args.key), String(args.value));
        return undefined as T;
      case "kv_remove":
        bucket.delete(String(args.key));
        return undefined as T;
      default:
        throw new Error(`未知命令 ${cmd}`);
    }
  });
  return { invoke, store };
}

afterEach(() => {
  uninstallKvStorageHostForTest();
});

describe("S2.04 KV 存储宿主安装器", () => {
  it("能力位可用：建桥并安装为全局 localStorage，此后全局读写落内核 KV", async () => {
    const kernel = makeKernelKv();
    const res = await installKvStorageHost({ origin: "varix-desktop", invoke: kernel.invoke, kvAvailable: () => true });
    expect(res.installed).toBe(true);
    if (!res.installed) return;
    expect(res.namespace).toBe("varix-desktop");
    // 全局裸 localStorage = 桥（vwm.ts 等既有代码的访问形态）。
    expect(globalThis.localStorage).toBe(res.storage);
    // 经全局面写入 → 内核 KV 落盘。
    globalThis.localStorage.setItem("k", "v");
    await res.storage.flush();
    expect(kernel.store.get("varix-desktop")!.get("k")).toBe("v");
  });

  it("能力位缺失（Windows 侧）：不安装、不动原生 localStorage（回退路径）", async () => {
    const native = new Map<string, string>([["theme", "dark"]]);
    Object.defineProperty(globalThis, "localStorage", {
      value: {
        getItem: (k: string) => native.get(k) ?? null,
        setItem: (k: string, v: string) => void native.set(k, v),
        removeItem: (k: string) => void native.delete(k),
        key: (i: number) => [...native.keys()][i] ?? null,
        get length() {
          return native.size;
        },
        clear: () => native.clear(),
      },
      configurable: true,
    });
    const res = await installKvStorageHost({ origin: "app", kvAvailable: () => false });
    expect(res).toEqual({ installed: false, reason: "kv-unavailable", storage: null });
    // 原生存储原样保留。
    expect(globalThis.localStorage.getItem("theme")).toBe("dark");
  });

  it("KV 预热失败：如实报 kv-init-failed，不伪造安装成功", async () => {
    const res = await installKvStorageHost({
      origin: "app",
      kvAvailable: () => true,
      invoke: async () => {
        throw new Error("内核通道断开");
      },
    });
    expect(res.installed).toBe(false);
    if (res.installed) return;
    expect(res.reason).toBe("kv-init-failed");
    expect(res.storage).toBeNull();
  });

  it("宿主 localStorage 不可配置（真浏览器语义）：如实报 native-unforgeable，bridge 照常交付", async () => {
    // 该测试把 localStorage 变成不可配置属性且无法撤销——必须放在文件最后一个用例。
    const frozen = new Map<string, string>();
    Object.defineProperty(globalThis, "localStorage", {
      value: {
        getItem: (k: string) => frozen.get(k) ?? null,
        setItem: (k: string, v: string) => void frozen.set(k, v),
        removeItem: (k: string) => void frozen.delete(k),
        key: (i: number) => [...frozen.keys()][i] ?? null,
        get length() {
          return frozen.size;
        },
        clear: () => frozen.clear(),
      },
      configurable: false,
    });
    const kernel = makeKernelKv();
    const res = await installKvStorageHost({ origin: "app", invoke: kernel.invoke, kvAvailable: () => true });
    expect(res.installed).toBe(false);
    if (res.installed) return;
    expect(res.reason).toBe("native-unforgeable");
    // bridge 仍在：嵌入层可另行注入（Servo 存储后端接线）。
    expect(res.storage).not.toBeNull();
    res.storage!.setItem("k", "v");
    await res.storage!.flush();
    expect(kernel.store.get("app")!.get("k")).toBe("v");
    // 原生属性未被顶替。
    expect(Object.getOwnPropertyDescriptor(globalThis, "localStorage")!.configurable).toBe(false);
  });
});
