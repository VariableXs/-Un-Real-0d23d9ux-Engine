/**
 * F093 图片缩略图引擎 · 前端缓存面（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据（主册 G-C-23）：万张目录滚动帧率不掉；二次浏览命中率 >95%；
 * 缓存库 2GB 上限生效。
 *
 * 移植声明：`kernel/varix/src/stard/thumbeng.rs` 的 TS 缓存面同构——
 * LRU 字节上限、命中率热库增量核算（K5 口径：库级累计含冷启动 miss，
 * 命中率只算热库增量）、三级优先队列（可视>邻近>背景）、损坏即弃自愈、
 * 渐进占位（低清先行）。前端面为 Object-URL 位图缓存（ AlbumApp 等列表
 * 消费）；像素解码管线判据在 Rust 模型面。
 */

/** 缓存字节上限默认（2GB——判据线）。 */
export const CACHE_CAP_BYTES = 2 * 1024 * 1024 * 1024;
/** 队列容量（挤兑诚实拒绝线）。 */
export const QUEUE_CAP = 256;

export type ThumbPriority = 0 | 1 | 2; // 0=可视 1=邻近 2=背景

/** FNV-1a 键（文件名+尺寸+修改时刻——与 mediainfo 缓存键同族）。 */
export function thumbKey(name: string, size: number, mtimeMs: number): string {
  let h = 0x811c9dc5;
  const s = `${name}|${size}|${Math.round(mtimeMs)}`;
  for (let i = 0; i < s.length; i++) {
    h = (h ^ s.charCodeAt(i)) >>> 0;
    h = (Math.imul(h, 0x01000193)) >>> 0;
  }
  return h.toString(16);
}

interface Entry { bytes: number; at: number }

/** 缩略图 LRU 缓存（字节上限 + 命中率热库核算 + 损坏即弃）。 */
export class ThumbCache {
  private map = new Map<string, Entry>();
  private bytesUsed = 0;
  /** 热库账（增量口径——K5）。 */
  hotHits = 0;
  hotMisses = 0;
  evictions = 0;
  discards = 0;

  constructor(public capBytes = CACHE_CAP_BYTES) {}

  get size(): number { return this.map.size; }
  get bytes(): number { return this.bytesUsed; }

  /** 命中率（热库增量口径——>95% 判线）。
   *  K5 口径：库级累计含冷启动 miss，不能混算——判据核算用
   *  startWindow()/hitRateWindow() 取「热窗口」内的增量。 */
  hitRate(): number {
    const total = this.hotHits + this.hotMisses;
    return total === 0 ? 0 : this.hotHits / total;
  }

  private winHits = 0;
  private winMisses = 0;

  /** 开一段热库核算窗口（二次浏览判据从这里起算）。 */
  startWindow(): void {
    this.winHits = 0;
    this.winMisses = 0;
  }

  /** 窗口内命中率（hits/(hits+misses)——窗口无请求返回 0）。 */
  hitRateWindow(): number {
    const total = this.winHits + this.winMisses;
    return total === 0 ? 0 : this.winHits / total;
  }

  has(key: string): boolean {
    return this.map.has(key);
  }

  /** 取用（命中即触碰——LRU 侧；返回是否存在）。 */
  touch(key: string): boolean {
    const e = this.map.get(key);
    if (!e) {
      this.hotMisses += 1;
      this.winMisses += 1;
      return false;
    }
    this.hotHits += 1;
    this.winHits += 1;
    this.map.delete(key);
    this.map.set(key, { ...e, at: Date.now() });
    return true;
  }

  /** 放入（超上限从最冷逐出至合规）。 */
  put(key: string, bytes: number): void {
    if (this.map.has(key)) {
      const old = this.map.get(key)!;
      this.bytesUsed -= old.bytes;
      this.map.delete(key);
    }
    this.map.set(key, { bytes, at: Date.now() });
    this.bytesUsed += bytes;
    while (this.bytesUsed > this.capBytes && this.map.size > 1) {
      const oldest = this.map.keys().next().value;
      if (oldest === undefined) break;
      const e = this.map.get(oldest)!;
      this.bytesUsed -= e.bytes;
      this.map.delete(oldest);
      this.evictions += 1;
    }
  }

  /** 损坏即弃（解码失败——自愈路径：删除条目，下次重生成）。 */
  discard(key: string): void {
    const e = this.map.get(key);
    if (!e) return;
    this.bytesUsed -= e.bytes;
    this.map.delete(key);
    this.discards += 1;
  }

  clear(): void {
    this.map.clear();
    this.bytesUsed = 0;
    this.hotHits = 0;
    this.hotMisses = 0;
    this.winHits = 0;
    this.winMisses = 0;
  }
}

/** 三级优先生成队列（可视>邻近>背景；满则诚实拒绝并记账）。 */
export class ThumbQueue {
  private buckets: Array<Set<string>> = [new Set(), new Set(), new Set()];
  /** 满载拒绝计数（诚实拒绝——不静默丢）。 */
  rejected = 0;
  enqueued = 0;
  processed = 0;

  enqueue(key: string, priority: ThumbPriority): boolean {
    for (const bucket of this.buckets) {
      if (bucket.has(key)) return true; // 已在队列——幂等
    }
    const bucket = this.buckets[priority]!;
    if (bucket.size >= QUEUE_CAP) {
      this.rejected += 1;
      return false;
    }
    bucket.add(key);
    this.enqueued += 1;
    return true;
  }

  dequeue(): string | null {
    for (const b of this.buckets) {
      const first = b.values().next().value;
      if (first !== undefined) {
        b.delete(first);
        this.processed += 1;
        return first;
      }
    }
    return null;
  }

  get pending(): number {
    return this.buckets[0]!.size + this.buckets[1]!.size + this.buckets[2]!.size;
  }
}

/** 渐进占位策略：可视请求直接全清生成；背景请求低清先行（占位→精修）。 */
export function progressivePlan(priority: ThumbPriority, progressiveEnabled: boolean): { placeholder: boolean; target: "low" | "full" } {
  if (!progressiveEnabled || priority === 0) return { placeholder: false, target: "full" };
  return { placeholder: true, target: "low" };
}
