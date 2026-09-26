/**
 * H4 批次共用持久化底座（F351-F400 · AI-H4）：
 * - 存储通道可注入（单测用内存实现），默认走 localStorage（真机）；
 * - localStorage 缺失（node 测试环境）自动落内存兜底，绝不抛出；
 * - readJson 对损坏数据零容错：解析失败/类型不符一律回退默认值——
 *   「静默吞错是我的红线」在这里的落实是**显式回退**而非异常上抛，
 *   持久化失败不影响任何交互路径（体验完整性 · 十二 · 数据安全）。
 */

export interface KvStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem?(key: string): void;
}

const MEM = new Map<string, string>();

/** 内存实现（测试与降级共用；显式注入时保证用例间可 clear）。 */
export function memStore(): KvStore {
  return {
    getItem: (k) => (MEM.has(k) ? (MEM.get(k) as string) : null),
    setItem: (k, v) => {
      MEM.set(k, String(v));
    },
    removeItem: (k) => {
      MEM.delete(k);
    },
  };
}

/** 仅测试用：清空内存兜底存储。 */
export function __clearMem(): void {
  MEM.clear();
}

/** 默认通道：真机 localStorage；不可用（node/隐私模式）则内存兜底。 */
export function defaultStore(): KvStore {
  try {
    if (typeof localStorage !== "undefined") return localStorage;
  } catch {
    /* 访问被策略拒绝 → 内存兜底 */
  }
  return memStore();
}

/** 读 JSON：损坏/类型不符显式回退 fallback（不抛、不留半态）。 */
export function readJson<T>(store: KvStore, key: string, fallback: T, validate?: (v: unknown) => v is T): T {
  const raw = store.getItem(key);
  if (raw === null) return fallback;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (validate && !validate(parsed)) return fallback;
    return parsed as T;
  } catch {
    return fallback;
  }
}

/** 写 JSON：写失败如实上报 false（配额满/被禁），调用方决定降级路径。 */
export function writeJson(store: KvStore, key: string, value: unknown): boolean {
  try {
    store.setItem(key, JSON.stringify(value));
    return true;
  } catch {
    return false;
  }
}

/** 删除键：失败不抛（键可能不存在）。 */
export function removeKey(store: KvStore, key: string): void {
  try {
    store.removeItem?.(key);
  } catch {
    /* 容忍 */
  }
}

/** 键名统一前缀：H4 批次全部持久化都挂在 variable:h4: 命名域下（一处一事实）。 */
export function h4Key(item: string, slot = ""): string {
  return slot ? `variable:h4:${item}:${slot}` : `variable:h4:${item}`;
}
