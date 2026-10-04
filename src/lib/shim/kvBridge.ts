/**
 * localStorage → 内核 KV 透明桥（任务 26 · 对接任务 24 kvsrv）。
 *
 * 同步/异步阻抗解法（总案阶段 3 步骤 4：前端代码零改动）：
 * init 时一次性拉取本命名空间全量键值到内存镜像 → 之后同步读写走镜像（localStorage
 * 语义逐条对齐）→ 写操作即时异步持久化（写后直达内核 KV），失败经 onError 如实上抛，
 * 绝不静默丢写（体验红线）。flush() 供退出/切换前强同步。
 *
 * 语义对齐（任务 24 定案）：值恒字符串；remove 不存在键 = no-op；末写生效；
 * 满容量 Err(Full) 经 onError 透传；内核无 clear 命令（差异清单第 2 条）→
 * keys()+逐键 remove 等价实现。
 *
 * 命名空间规则（任务 24 约束 ns≤16B，全 API 强制）：
 * origin 字符安全且 ≤16B → 原样；否则 11 字符安全前缀 + "_" + fnv1a-32 低 4 hex，
 * 确定性可复现，碰撞空间 2^32（同机 origin 量级下视为不碰撞，规则入测试）。
 *
 * 能力位 kvStorage=false（Windows 侧/未握手）→ 回退原生 localStorage，行为等价，
 * Windows 侧零改动。
 */
import { shimInvoke } from "./shimInvoke";

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
  key(index: number): string | null;
  readonly length: number;
  clear(): void;
}

export interface KvBridge extends StorageLike {
  /** 等待全部在途持久化落盘（退出/切换前调用）。 */
  flush(): Promise<void>;
  /** 内核命名空间（诊断/审计用）。 */
  readonly namespace: string;
}

/** fnv1a-32（与内核 fs23/kvsrv 校验同族算法，仅用于命名空间摘要）。 */
function fnv1a32(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

/** origin → 内核命名空间（≤16B，字符集 [A-Za-z0-9_-]，确定性）。 */
export function originToNamespace(origin: string): string {
  const safe = origin.replace(/[^A-Za-z0-9_-]/g, "");
  if (safe === origin && origin.length >= 1 && origin.length <= 16) return origin;
  const prefix = safe.slice(0, 11) || "ns";
  const hex = fnv1a32(origin).toString(16).padStart(8, "0").slice(-4);
  return `${prefix}_${hex}`.slice(0, 16);
}

type Transport = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

export function initKvStorage(opts: {
  origin: string;
  /** 垫片 invoke（默认 shimInvoke；测试注入桩）。 */
  invoke?: Transport;
  /** 内核 KV 是否可用（能力位查询；默认恒真，测试可注入）。 */
  kvAvailable?: () => boolean;
  /** 能力位不满足时的回退存储（Windows 侧传 window.localStorage）。 */
  nativeFallback?: Storage | null;
  /** 异步持久化失败回调（不传则 console.error，绝不静默）。 */
  onError?: (key: string, err: unknown) => void;
}): Promise<KvBridge> {
  const invoke = (opts.invoke ?? shimInvoke) as Transport;
  const available = opts.kvAvailable ?? (() => true);
  const onError = opts.onError ?? ((key, err) => console.error(`[kvBridge] 持久化失败 key=${key}`, err));
  const ns = originToNamespace(opts.origin);

  // 能力位不满足：直接包原生存储（同步语义天然成立，Windows 侧零改动）
  if (!available()) {
    if (!opts.nativeFallback) return Promise.reject(new Error("KV 不可用且未提供回退存储"));
    const s = opts.nativeFallback;
    return Promise.resolve({
      getItem: (k) => s.getItem(k),
      setItem: (k, v) => s.setItem(k, v),
      removeItem: (k) => s.removeItem(k),
      key: (i) => s.key(i),
      get length() {
        return s.length;
      },
      clear: () => s.clear(),
      flush: () => Promise.resolve(),
      namespace: ns,
    });
  }

  // 预热：全量拉取本命名空间键值（几何/设置量级，逐键 get 成本可接受）
  const mirror = new Map<string, string>();
  const pending = new Set<Promise<unknown>>();

  const track = (p: Promise<unknown>, key: string): void => {
    const t = p.catch((err) => onError(key, err));
    pending.add(t);
    void t.finally(() => pending.delete(t));
  };

  return (async () => {
    const keys = (await invoke("kv_keys", { ns })) as string[];
    const pairs = await Promise.all(
      keys.map(async (k) => [k, (await invoke("kv_get", { ns, key: k })) as string | null] as const),
    );
    for (const [k, v] of pairs) if (v !== null) mirror.set(k, v);

    return {
      namespace: ns,
      flush: () => Promise.all([...pending]).then(() => undefined),

      getItem(key: string): string | null {
        return mirror.get(key) ?? null;
      },
      setItem(key: string, value: string): void {
        const v = String(value);
        mirror.set(key, v); // 末写生效：镜像先行
        track(invoke("kv_set", { ns, key, value: v }), key);
      },
      removeItem(key: string): void {
        // 内核语义：remove 不存在 = no-op（镜像 delete 天然 no-op）
        if (!mirror.delete(key)) return;
        track(invoke("kv_remove", { ns, key }), key);
      },
      key(index: number): string | null {
        return [...mirror.keys()][index] ?? null;
      },
      get length(): number {
        return mirror.size;
      },
      clear(): void {
        const keys = [...mirror.keys()];
        mirror.clear();
        for (const k of keys) track(invoke("kv_remove", { ns, key: k }), k);
      },
    };
  })();
}
