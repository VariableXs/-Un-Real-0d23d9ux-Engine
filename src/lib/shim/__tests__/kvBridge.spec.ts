import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { initKvStorage, originToNamespace, type KvBridge } from "../kvBridge";

/** 桩内核 KV：命令面 kv_keys/kv_get/kv_set/kv_remove，ns 隔离与满容语义对齐任务 24。 */
function makeKernelKv(opts?: { fullKeys?: Set<string> }) {
  const store = new Map<string, Map<string, string>>();
  const calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  const invoke = vi.fn(async <T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
    calls.push({ cmd, args });
    const ns = String(args.ns);
    if (!store.has(ns)) store.set(ns, new Map());
    const bucket = store.get(ns)!;
    switch (cmd) {
      case "kv_keys":
        return [...bucket.keys()] as T;
      case "kv_get":
        return (bucket.get(String(args.key)) ?? null) as T;
      case "kv_set": {
        const key = String(args.key);
        if (opts?.fullKeys?.has(key)) throw Object.assign(new Error("KV 满"), { code: "IO" });
        bucket.set(key, String(args.value));
        return undefined as T;
      }
      case "kv_remove":
        bucket.delete(String(args.key));
        return undefined as T;
      default:
        throw new Error(`未知命令 ${cmd}`);
    }
  });
  return { invoke, calls, store };
}

beforeEach(() => {});
afterEach(() => {});

describe("origin → 命名空间规则（任务 24 约束 ns≤16B）", () => {
  it("短且字符安全的 origin 原样；结果恒 ≤16B 且字符集安全", () => {
    expect(originToNamespace("file")).toBe("file");
    expect(originToNamespace("varix")).toBe("varix");
    for (const origin of [
      "http://localhost:1420",
      "tauri://localhost",
      "https://variable.varix.os/desktop",
      "非常长的中文 origin 带空格 与符号!!",
    ]) {
      const ns = originToNamespace(origin);
      expect(ns.length).toBeLessThanOrEqual(16);
      expect(ns).toMatch(/^[A-Za-z0-9_-]+$/);
    }
  });

  it("确定性：同 origin 恒同 ns；同前缀不同 origin 因哈希后缀而区分", () => {
    const a1 = originToNamespace("http://localhost:1420");
    const a2 = originToNamespace("http://localhost:1420");
    expect(a1).toBe(a2);
    const b = originToNamespace("http://localhost:1421");
    expect(b).not.toBe(a1);
    // 手工构造同 11 字符前缀的长 origin，验证后缀区分
    const long1 = originToNamespace("workspace-alpha-user1");
    const long2 = originToNamespace("workspace-alpha-user2");
    expect(long1).not.toBe(long2);
  });
});

describe("KV 桥：localStorage 语义对齐（任务 24 定案）", () => {
  it("预热加载 → 同步读写镜像 → 异步持久化到内核（写后可达）", async () => {
    const kernel = makeKernelKv();
    kernel.store.set("app", new Map([["boot", "1"]]));
    const kv: KvBridge = await initKvStorage({ origin: "app", invoke: kernel.invoke });
    expect(kv.getItem("boot")).toBe("1");
    kv.setItem("layout", "deck-1");
    expect(kv.getItem("layout")).toBe("deck-1"); // 末写生效，镜像即时
    await kv.flush();
    const bucket = kernel.store.get("app")!;
    expect(bucket.get("layout")).toBe("deck-1");
    expect(bucket.get("boot")).toBe("1");
  });

  it("remove 不存在键 = no-op（不产生内核命令）；存在键移除后落盘", async () => {
    const kernel = makeKernelKv();
    kernel.store.set("app", new Map([["gone-soon", "x"]]));
    const kv = await initKvStorage({ origin: "app", invoke: kernel.invoke });
    const before = kernel.calls.filter((c) => c.cmd === "kv_remove").length;
    kv.removeItem("never-existed");
    expect(kernel.calls.filter((c) => c.cmd === "kv_remove").length).toBe(before);
    kv.removeItem("gone-soon");
    await kv.flush();
    expect(kernel.store.get("app")!.has("gone-soon")).toBe(false);
    expect(kv.getItem("gone-soon")).toBeNull();
  });

  it("key(i)/length/clear 与 localStorage 行为一致；clear=逐键 remove（内核无 clear 命令）", async () => {
    const kernel = makeKernelKv();
    const kv = await initKvStorage({ origin: "app", invoke: kernel.invoke });
    kv.setItem("a", "1");
    kv.setItem("b", "2");
    expect(kv.length).toBe(2);
    expect(kv.key(0)).toBe("a");
    expect(kv.key(9)).toBeNull();
    kv.clear();
    expect(kv.length).toBe(0);
    await kv.flush();
    expect(kernel.store.get("app")!.size).toBe(0);
    expect(kernel.calls.filter((c) => c.cmd === "kv_set").length).toBe(2);
    expect(kernel.calls.filter((c) => c.cmd === "kv_remove").length).toBe(2);
  });

  it("持久化失败经 onError 如实上抛（绝不静默丢写），镜像仍保持末写", async () => {
    const kernel = makeKernelKv({ fullKeys: new Set(["too-big"]) });
    const errors: Array<{ key: string; err: unknown }> = [];
    const kv = await initKvStorage({
      origin: "app",
      invoke: kernel.invoke,
      onError: (key, err) => errors.push({ key, err }),
    });
    kv.setItem("too-big", "x");
    await kv.flush();
    expect(errors).toHaveLength(1);
    expect(errors[0]!.key).toBe("too-big");
    expect(kv.getItem("too-big")).toBe("x");
  });

  it("命名空间隔离：不同 origin 互不可见互不可删（任务 24 语义）", async () => {
    const kernel = makeKernelKv();
    const a = await initKvStorage({ origin: "app", invoke: kernel.invoke });
    const b = await initKvStorage({ origin: "shell", invoke: kernel.invoke });
    a.setItem("k", "from-app");
    b.setItem("k", "from-shell");
    await Promise.all([a.flush(), b.flush()]);
    expect(a.getItem("k")).toBe("from-app");
    expect(b.getItem("k")).toBe("from-shell");
    b.removeItem("k");
    await b.flush();
    expect(a.getItem("k")).toBe("from-app");
    expect(kernel.store.get("app")!.get("k")).toBe("from-app");
  });

  it("能力位不满足 → 回退原生存储（Windows 侧零改动）", async () => {
    const native = new Map<string, string>([["theme", "dark"]]);
    const fake = {
      getItem: (k: string) => native.get(k) ?? null,
      setItem: (k: string, v: string) => void native.set(k, v),
      removeItem: (k: string) => void native.delete(k),
      key: (i: number) => [...native.keys()][i] ?? null,
      get length() {
        return native.size;
      },
      clear: () => native.clear(),
    } as Storage;
    const kv = await initKvStorage({
      origin: "app",
      kvAvailable: () => false,
      nativeFallback: fake,
    });
    expect(kv.getItem("theme")).toBe("dark");
    kv.setItem("x", "1");
    expect(native.get("x")).toBe("1");
    await expect(kv.flush()).resolves.toBeUndefined();
  });
});
