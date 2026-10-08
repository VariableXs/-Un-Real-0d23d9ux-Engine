/**
 * F154 壁纸引擎深化 · 官方池下载队列 + 缓存 LRU + 预载调度 + 多屏池 + EXIF 方向。
 *
 * 主册判据延伸：
 * - 【设计细节】「官方池下载失败 → 静默用本地池」「预加载次日壁纸（空闲时段
 *   F049 窗口解码缓存——换的时刻零等待）」「多屏前瞻接口预留（各屏独立池）」。
 * - 数据安全红线：下载只写白名单缓存目录；校验失败文件即弃（外部输入全清洗）。
 */

import { pickExcluding } from "./store";

// ---------- 下载队列（官方池，F126/F134 管线） ----------

export type DownloadState = "queued" | "downloading" | "done" | "failed" | "skipped";

export interface DownloadTask {
  wallpaperId: string;
  url: string;
  /** 预期字节数上限（超限即弃——防炸弹）。 */
  maxBytes: number;
  /** 完整性校验（下载后核对；不匹配 → failed）。 */
  checksum: string;
  state: DownloadState;
  attempts: number;
  bytes: number;
  /** 三要素失败原因（人话）。 */
  reason?: string;
}

export const MAX_ATTEMPTS = 3;

export interface DownloadQueueConfig {
  /** 空闲时段窗口（F049）：仅允许在该窗口内消耗带宽。 */
  idleWindow: { fromHour: number; toHour: number };
  /** 并发上限。 */
  concurrency: number;
}

export const DEFAULT_QUEUE_CONFIG: DownloadQueueConfig = { idleWindow: { fromHour: 2, toHour: 5 }, concurrency: 2 };

/** 队列调度器：每次 tick 取出至多 concurrency 个 queued 任务置为 downloading。 */
export function scheduleDownloads(tasks: DownloadTask[], config: DownloadQueueConfig, now: number): DownloadTask[] {
  const inWindow = inIdleWindow(now, config.idleWindow);
  let slots = inWindow ? config.concurrency : 0;
  return tasks.map((t) => {
    if (t.state === "queued" && slots > 0) {
      slots--;
      return { ...t, state: "downloading" as const };
    }
    return t;
  });
}

export function inIdleWindow(now: number, window: { fromHour: number; toHour: number }): boolean {
  const h = new Date(now).getHours();
  if (window.fromHour <= window.toHour) return h >= window.fromHour && h < window.toHour;
  return h >= window.fromHour || h < window.toHour; // 跨零点窗口
}

/** 下载完成判定：字节数在 (0, maxBytes] 且 checksum 匹配 → done；否则失败计入重试。 */
export function completeDownload(task: DownloadTask, actualBytes: number, actualChecksum: string): DownloadTask {
  if (actualBytes <= 0 || actualBytes > task.maxBytes) {
    return failTask(task, `下载体积异常（${actualBytes} 字节，上限 ${task.maxBytes}）`);
  }
  if (actualChecksum !== task.checksum) {
    return failTask(task, "完整性校验不匹配——文件已弃用");
  }
  return { ...task, state: "done", bytes: actualBytes, reason: undefined };
}

export function failTask(task: DownloadTask, reason: string): DownloadTask {
  const attempts = task.attempts + 1;
  if (attempts >= MAX_ATTEMPTS) {
    return { ...task, state: "failed", attempts, reason: `${reason}（已重试 ${MAX_ATTEMPTS} 次，静默回退本地池）` };
  }
  return { ...task, state: "queued", attempts, reason };
}

// ---------- 缓存 LRU（容量上限 + 命中记账） ----------

export interface CacheEntryMeta {
  id: string;
  bytes: number;
  lastUsedAt: number;
  pinned: boolean; // 当前壁纸与预载位不驱逐
}

export class WallpaperCache {
  private entries = new Map<string, CacheEntryMeta>();

  constructor(private maxBytes: number) {}

  get capacityBytes(): number {
    return this.maxBytes;
  }

  get usedBytes(): number {
    let s = 0;
    for (const e of this.entries.values()) s += e.bytes;
    return s;
  }

  touch(id: string, bytes: number, now: number): void {
    const existing = this.entries.get(id);
    this.entries.set(id, { id, bytes: existing?.bytes ?? bytes, lastUsedAt: now, pinned: existing?.pinned ?? false });
  }

  pin(id: string, pinned: boolean): void {
    const e = this.entries.get(id);
    if (e) e.pinned = pinned;
  }

  /** 是否需要驱逐以容纳 incoming 字节；返回被逐清单（LRU 序，pinned 豁免）。 */
  planEviction(incomingBytes: number): string[] {
    const evict: string[] = [];
    let projected = this.usedBytes + incomingBytes;
    if (projected <= this.maxBytes) return evict;
    const candidates = [...this.entries.values()]
      .filter((e) => !e.pinned)
      .sort((a, b) => a.lastUsedAt - b.lastUsedAt);
    for (const e of candidates) {
      if (projected <= this.maxBytes) break;
      evict.push(e.id);
      projected -= e.bytes;
    }
    return projected > this.maxBytes && evict.length === candidates.length ? evict : evict;
  }

  applyEviction(ids: string[]): void {
    for (const id of ids) this.entries.delete(id);
  }

  /** 命中率（F093 同族口径——二次浏览命中率 >95% 的记账面）。 */
  stats(): { count: number; usedBytes: number; capacityBytes: number } {
    return { count: this.entries.size, usedBytes: this.usedBytes, capacityBytes: this.maxBytes };
  }
}

// ---------- 预载调度（换的时刻零等待） ----------

export interface PreloadPlan {
  wallpaperId: string;
  /** 预载时刻（空闲窗口内、换壁纸时刻之前）。 */
  preloadAt: number;
  reason: string;
}

/** 预载计划：空闲窗口起点与换壁纸时刻取交集，提前 30 分钟解码缓存。 */
export function planPreload(nextRotateAt: number, idleWindow: { fromHour: number; toHour: number }, now: number, candidateId: string | null): PreloadPlan | null {
  if (!candidateId) return null;
  // 空闲窗口与「换壁纸前 30 分钟」的交集起点。
  const leadMs = 30 * 60 * 1000;
  const target = nextRotateAt - leadMs;
  if (target <= now) return { wallpaperId: candidateId, preloadAt: now, reason: "距更换不足 30 分钟，立即预载" };
  const inWindow = inIdleWindow(target, idleWindow);
  if (inWindow) return { wallpaperId: candidateId, preloadAt: target, reason: "空闲窗口内提前 30 分钟预载" };
  // 不在窗口：顺延到窗口起点（换之前最晚的窗口时刻）。
  const windowStart = new Date(target);
  windowStart.setHours(idleWindow.fromHour, 0, 0, 0);
  if (windowStart.getTime() < now) windowStart.setDate(windowStart.getDate() + 1);
  if (windowStart.getTime() >= nextRotateAt) return null; // 窗口在更换之后——放弃预载
  return { wallpaperId: candidateId, preloadAt: windowStart.getTime(), reason: "空闲窗口起点预载" };
}

// ---------- 多屏池（前瞻接口——各屏独立池） ----------

export interface MonitorPool {
  monitorId: string;
  poolIds: string[];
  /** 主屏跟随全局轮换；副屏独立抽取。 */
  followGlobal: boolean;
}

export function pickForMonitors(pools: MonitorPool[], globalPick: string | null, randByMonitor: Record<string, () => number>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of pools) {
    if (m.followGlobal && globalPick) {
      out[m.monitorId] = globalPick;
      continue;
    }
    const pick = pickExcluding(m.poolIds, () => false, randByMonitor[m.monitorId] ?? Math.random);
    if (pick) out[m.monitorId] = pick;
  }
  return out;
}

// ---------- EXIF 方向（JPEG Orientation 1-8 → 裁切前旋转） ----------

export type ExifOrientation = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;

/** EXIF 方向 → 渲染前变换描述（swapWH = 需交换宽高）。 */
export function orientationTransform(o: ExifOrientation): { swapWH: boolean; flipX: boolean; flipY: boolean; rotateDeg: 0 | 90 | 180 | 270 } {
  switch (o) {
    case 1: return { swapWH: false, flipX: false, flipY: false, rotateDeg: 0 };
    case 2: return { swapWH: false, flipX: true, flipY: false, rotateDeg: 0 };
    case 3: return { swapWH: false, flipX: false, flipY: false, rotateDeg: 180 };
    case 4: return { swapWH: false, flipX: false, flipY: true, rotateDeg: 0 };
    case 5: return { swapWH: true, flipX: false, flipY: false, rotateDeg: 90 };
    case 6: return { swapWH: true, flipX: false, flipY: false, rotateDeg: 90 };
    case 7: return { swapWH: true, flipX: false, flipY: false, rotateDeg: 270 };
    case 8: return { swapWH: true, flipX: false, flipY: false, rotateDeg: 270 };
  }
}

/** 从 JPEG 头解析 EXIF Orientation（无/非法 → 1）。 */
export function parseExifOrientation(jpeg: ArrayBuffer): ExifOrientation {
  const v = new DataView(jpeg);
  try {
    if (v.getUint16(0) !== 0xffd8) return 1; // 非 JPEG
    let off = 2;
    for (;;) {
      const marker = v.getUint16(off);
      const size = v.getUint16(off + 2);
      if (marker === 0xffe1) {
        // APP1: "Exif\0\0" + TIFF header
        const tiff = off + 10;
        const little = v.getUint16(tiff) === 0x4949;
        const u32 = (o: number) => v.getUint32(tiff + o, little);
        const ifd0 = tiff + u32(4);
        const entries = v.getUint16(ifd0, little);
        for (let i = 0; i < entries; i++) {
          const e = ifd0 + 2 + i * 12;
          if (v.getUint16(e, little) === 0x0112) {
            const val = v.getUint16(e + 8, little);
            return (val >= 1 && val <= 8 ? val : 1) as ExifOrientation;
          }
        }
        return 1;
      }
      if (marker === 0xffda || marker === 0xd9) return 1; // 扫描开始/文件结束——无 EXIF
      off += 2 + size;
      if (off + 4 > v.byteLength) return 1;
    }
  } catch {
    return 1;
  }
}
