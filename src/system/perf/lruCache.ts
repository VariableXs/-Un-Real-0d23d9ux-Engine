/**
 * AI-13 M-49 图标缓存 LRU 治理：容量/字节双上限 + 分批淘汰（rAF 每批 ≤32）。
 * 淘汰顺序 = last_used 最旧优先；单测覆盖顺序与批量不阻塞约束。
 */

export interface LruEntry<V> {
  value: V;
  bytes: number;
  lastUsed: number;
}

export interface LruStats {
  hits: number;
  misses: number;
  evicted: number;
  bytes: number;
  count: number;
}

export class LruByteCache<V> {
  private map = new Map<string, LruEntry<V>>();
  private hits = 0;
  private misses = 0;
  private evicted = 0;
  private bytes = 0;
  constructor(private maxEntries: number, private maxBytes: number) {}

  get(key: string): V | undefined {
    const e = this.map.get(key);
    if (!e) {
      this.misses += 1;
      return undefined;
    }
    this.hits += 1;
    e.lastUsed = ++LruByteCache.clock;
    return e.value;
  }

  has(key: string): boolean {
    return this.map.has(key);
  }

  set(key: string, value: V, bytes: number): void {
    if (bytes > this.maxBytes) return; // 单条超预算直接拒收
    const old = this.map.get(key);
    if (old) {
      this.bytes -= old.bytes;
      this.map.delete(key);
    }
    this.map.set(key, { value, bytes, lastUsed: ++LruByteCache.clock });
    this.bytes += bytes;
    // 即时软收缩到预算内（数量与字节双限），大批量分摊由 evictBatch 驱动
    while (this.map.size > this.maxEntries || this.bytes > this.maxBytes) {
      if (!this.evictOldest()) break;
    }
  }

  /** rAF 分批淘汰：每批 ≤32 张，主线程单帧 ≤8ms 约束由调用方驱动。 */
  evictBatch(max = 32): number {
    let n = 0;
    while (n < max && this.map.size > this.maxEntries) {
      if (!this.evictOldest()) break;
      n += 1;
    }
    return n;
  }

  private evictOldest(): boolean {
    let oldestKey: string | null = null;
    let oldest = Infinity;
    for (const [k, e] of this.map) {
      if (e.lastUsed < oldest) {
        oldest = e.lastUsed;
        oldestKey = k;
      }
    }
    if (oldestKey === null) return false;
    const e = this.map.get(oldestKey)!;
    this.bytes -= e.bytes;
    this.map.delete(oldestKey);
    this.evicted += 1;
    return true;
  }

  stats(): LruStats {
    return { hits: this.hits, misses: this.misses, evicted: this.evicted, bytes: this.bytes, count: this.map.size };
  }

  clear(): void {
    this.map.clear();
    this.bytes = 0;
  }

  private static clock = 0;
}

/** 命中率（设置页展示用）。 */
export function hitRate(s: LruStats): number {
  const total = s.hits + s.misses;
  return total === 0 ? 0 : s.hits / total;
}
