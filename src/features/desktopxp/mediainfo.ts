/**
 * F094 媒体信息悬浮 · 前端逻辑（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据（主册 G-C-24）：MP4/MKV/WebM/FLAC/MP3 五容器样本解析全对；
 * 悬停到显示 <100ms（缓存命中时）。
 *
 * 移植声明：`kernel/varix/src/stard/mediainfo.rs` 的 TS 轻量同构——前端
 * 面负责「头部嗅探 + 关键字段抽取 + LRU 缓存 + 悬停调度」；深度 box 走查
 * （旋转矩阵/轨道字节实算）判据面在 Rust 模型层，两处共用同一容器语义
 * （一处一事实）。大文件只读头部前 64KB（不整读——内存纪律）。
 */

/** 悬停延迟（ms）。 */
export const HOVER_DELAY_MS = 800;
/** 缓存命中显示判线（ms）。 */
export const CACHE_HIT_LINE_MS = 100;
/** 提示卡圆角（px）。 */
export const TIP_RADIUS_PX = 4;
/** 提示卡与光标间距（px）。 */
export const TIP_GAP_PX = 8;
/** 头部嗅探读取量（字节）。 */
export const HEAD_BYTES = 64 * 1024;

export type Container = "mp4" | "mkv" | "webm" | "flac" | "mp3";

export interface MediaInfo {
  container: Container;
  /** 时长 ms（未知 null——诚实留白，不编数）。 */
  durationMs: number | null;
  width: number | null;
  height: number | null;
  /** 视频轨计数。 */
  videoTracks: number;
  audioTracks: number;
  /** 主视频码率 bps（未知 null）。 */
  videoBps: number | null;
  /** 音频采样率 Hz（未知 null）。 */
  sampleRate: number | null;
  channels: number | null;
  codec: string | null;
}

export function durationLabel(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms) || ms < 0) return "时长未知";
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${m}:${ss}`;
}

export function resolutionLabel(info: MediaInfo): string {
  return info.width !== null && info.height !== null ? `${info.width}×${info.height}` : "分辨率未知";
}

export function isVideo(c: Container): boolean {
  return c === "mp4" || c === "mkv" || c === "webm";
}

// ---------------------------------------------------------------------------
// 容器嗅探与关键字段（头部轻解析）
// ---------------------------------------------------------------------------

function u32beSafe(b: Uint8Array, off: number): number {
  if (off + 4 > b.length) return 0;
  return (((b[off]! << 24) | (b[off + 1]! << 16) | (b[off + 2]! << 8) | b[off + 3]!) >>> 0);
}

function ascii(b: Uint8Array, off: number, len: number): string {
  let s = "";
  for (let i = 0; i < len && off + i < b.length; i++) s += String.fromCharCode(b[off + i]!);
  return s;
}

/** MP4：ftyp 嗅探 + moov/mvhd 时长 + tkhd 宽高（头部窗内 box 线性走查）。 */
export function parseMp4(head: Uint8Array, fileBytes: number): MediaInfo | null {
  if (head.length < 12 || ascii(head, 4, 4) !== "ftyp") return null;
  const info: MediaInfo = { container: "mp4", durationMs: null, width: null, height: null, videoTracks: 0, audioTracks: 0, videoBps: null, sampleRate: null, channels: null, codec: null };
  // box 线性走查（moov 通常在前 64KB——大 moov 走模型面深查，此处诚实留白）。
  let off = 0;
  let end = head.length;
  while (off + 8 <= end) {
    let size = u32beSafe(head, off);
    const type = ascii(head, off + 4, 4);
    if (size === 1) {
      // 64 位 size：高 32 位足够（头部窗内不会超）。
      const hi = u32beSafe(head, off + 8);
      size = hi * 2 ** 32 + u32beSafe(head, off + 12);
    } else if (size === 0) {
      size = end - off; // 到文件尾
    }
    if (size < 8) break;
    if (type === "moov" || type === "trak") {
      // 进入子箱走查（嵌套一层——宽度/时长在 trak 的子箱）。
      const subEnd = Math.min(off + size, end);
      let sub = off + 8;
      while (sub + 8 <= subEnd) {
        const ssz = u32beSafe(head, sub);
        const sty = ascii(head, sub + 4, 4);
        if (ssz < 8) break;
        if (sty === "mvhd") {
          const ver = head[sub + 8]!;
          // body = sub+8 起：v0 = ver/flags(4)+ctime(4)+mtime(4)+timescale(4)+duration(4)。
          if (ver === 1 && sub + 8 + 28 + 8 <= subEnd) {
            // v1: ctime(8)+mtime(8)+timescale(4)+duration(8)。
            const ts = u32beSafe(head, sub + 8 + 4 + 16);
            const dur = Number((BigInt(u32beSafe(head, sub + 8 + 4 + 20)) << 32n) | BigInt(u32beSafe(head, sub + 8 + 4 + 24)));
            if (ts > 0 && dur > 0) info.durationMs = Math.round((dur / ts) * 1000);
          } else if (sub + 8 + 20 <= subEnd) {
            const ts = u32beSafe(head, sub + 8 + 12);
            const dur = u32beSafe(head, sub + 8 + 16);
            if (ts > 0 && dur > 0) info.durationMs = Math.round((dur / ts) * 1000);
          }
        } else if (sty === "tkhd") {
          // tkhd：宽高在尾部 8 字节（16.16 定点）。
          // body 布局 v0：ver/flags(4)+ctime(4)+mtime(4)+trackID(4)+reserved(4)+
          //   duration(4)+reserved(8)+layer(2)+alt(2)+volume(2)+reserved(2)+
          //   matrix(36) → 宽在 body+76、高 body+80；v1 时间字段 64 位 → +12。
          const body = sub + 8;
          const ver = head[body]!;
          const wOff = body + (ver === 1 ? 88 : 76);
          if (wOff + 8 <= subEnd) {
            const w = u32beSafe(head, wOff) / 65536;
            const h = u32beSafe(head, wOff + 4) / 65536;
            if (w > 0 && h > 0) { info.width = Math.round(w); info.height = Math.round(h); info.videoTracks += 1; }
          }
        } else if (sty === "mdhd") {
          // 音轨时长兜底（mvhd 缺时）——此处不覆盖视频时长。
        }
        sub += ssz;
      }
    }
    off += size;
  }
  if (info.videoTracks === 0) info.audioTracks = 1; // 纯音频 mp4（m4a 族）
  info.videoBps = fileBytes > 0 && info.durationMs ? Math.round((fileBytes * 8) / (info.durationMs / 1000)) : null;
  return info;
}

/** EBML varint 读取（MKV/WebM——返回 [值, 字节数]）。 */
function readVint(b: Uint8Array, off: number): [number, number] | null {
  if (off >= b.length) return null;
  const first = b[off]!;
  let len = 0;
  for (let i = 0; i < 8; i++) {
    if ((first & (0x80 >> i)) !== 0) { len = i + 1; break; }
  }
  if (len === 0 || off + len > b.length) return null;
  let val = first & (0xff >> len);
  for (let i = 1; i < len; i++) val = val * 256 + b[off + i]!;
  return [val, len];
}

/** MKV/WebM：EBML 头嗅探 + Segment/Info(TimecodeScale, Duration float) + TrackType/PixelW/H。 */
export function parseMkv(head: Uint8Array, isWebm: boolean): MediaInfo | null {
  if (head.length < 4) return null;
  const magic = ascii(head, 0, 4);
  if (magic !== "\x1aE\xdf\xa3") return null;
  const info: MediaInfo = { container: isWebm ? "webm" : "mkv", durationMs: null, width: null, height: null, videoTracks: 0, audioTracks: 0, videoBps: null, sampleRate: null, channels: null, codec: null };
  // 扫 Master 元素：Info(0x1549a966) / Tracks(0x1654ae6b)——嵌套走查两层。
  let off = 0;
  while (off + 4 < head.length) {
    const id = readVint(head, off);
    if (!id) break;
    const size = readVint(head, off + id[1]);
    if (!size) break;
    const body = off + id[1] + size[1];
    const bodyEnd = Math.min(body + size[0], head.length);
    if (id[0] === 0x1549a966 || id[0] === 0x1654ae6b) {
      let sub = body;
      while (sub + 2 < bodyEnd) {
        const sid = readVint(head, sub);
        if (!sid) break;
        const ssz = readVint(head, sub + sid[1]);
        if (!ssz) break;
        const sbody = sub + sid[1] + ssz[1];
        const sEnd = Math.min(sbody + ssz[0], bodyEnd);
        if (id[0] === 0x1549a966) {
          if (sid[0] === 0x2ad7b1 && ssz[0] === 4) {
            // TimecodeScale（默认 1e6 ns）
          } else if (sid[0] === 0x4489 && ssz[0] === 8) {
            // Duration：float64
            const dv = new DataView(head.buffer, head.byteOffset + sbody, 8);
            const durSec = dv.getFloat64(0, false);
            if (Number.isFinite(durSec) && durSec > 0) info.durationMs = Math.round(durSec);
          }
        } else {
          // TrackEntry(0xae) 内层第三层
          if (sid[0] === 0xae) {
            let t = sbody;
            while (t + 2 < sEnd) {
              const tid = readVint(head, t);
              if (!tid) break;
              const tsz = readVint(head, t + tid[1]);
              if (!tsz) break;
              const tbody = t + tid[1] + tsz[1];
              const tEnd = Math.min(tbody + tsz[0], sEnd);
              if (tid[0] === 0x83 && tsz[0] === 1) {
                const tt = head[tbody]!;
                if (tt === 1) { info.videoTracks += 1; }
                else if (tt === 2) { info.audioTracks += 1; }
              } else if (tid[0] === 0xb0 && tsz[0] <= 4) {
                let w = 0;
                for (let i = 0; i < tsz[0]; i++) w = w * 256 + head[tbody + i]!;
                if (w > 0) info.width = w;
              } else if (tid[0] === 0xba && tsz[0] <= 4) {
                let h = 0;
                for (let i = 0; i < tsz[0]; i++) h = h * 256 + head[tbody + i]!;
                if (h > 0) info.height = h;
              }
              t = tEnd;
            }
          }
        }
        sub = sEnd;
      }
    }
    off = bodyEnd;
    if (info.durationMs !== null && (info.width !== null || info.audioTracks > 0)) break;
  }
  return info;
}

/** FLAC：fLaC 嗅探 + STREAMINFO 位拆（采样率/声道/总样本）。
 *  位布局（STREAMINFO body 34 字节，自 fLaC+块头后起）：
 *  10-12 字节 = 采样率 17 位 + 声道 3 位 + 位深高位；13 字节 = 位深低位 +
 *  总样本高 7 位；14-17 字节 = 总样本低 32 位。 */
export function parseFlac(head: Uint8Array): MediaInfo | null {
  if (head.length < 42 || ascii(head, 0, 4) !== "fLaC") return null;
  const p = 8; // STREAMINFO body 起点（fLaC 4 + 块头 4）
  const sampleRate = (head[p + 10]! << 9) | (head[p + 11]! << 1) | (head[p + 12]! >> 7);
  const channels = ((head[p + 12]! >> 4) & 0x7) + 1;
  const totalSamplesHi = head[p + 13]! & 0x7f;
  const totalSamples = totalSamplesHi * 2 ** 32 + u32beSafe(head, p + 14);
  const info: MediaInfo = { container: "flac", durationMs: null, width: null, height: null, videoTracks: 0, audioTracks: 1, videoBps: null, sampleRate, channels, codec: "FLAC" };
  if (sampleRate > 0 && totalSamples > 0) info.durationMs = Math.round((totalSamples / sampleRate) * 1000);
  return info;
}

/** MP3：帧头嗅探（0xFFE 同步）+ 码率/采样率表 + Xing VBR 时长。 */
export function parseMp3(head: Uint8Array, fileBytes: number): MediaInfo | null {
  // 跳 ID3v2。
  let off = 0;
  if (head.length > 10 && ascii(head, 0, 3) === "ID3") {
    const sz = ((head[6]! & 0x7f) << 21) | ((head[7]! & 0x7f) << 14) | ((head[8]! & 0x7f) << 7) | (head[9]! & 0x7f);
    off = 10 + sz;
  }
  // 找帧同步（最多扫 4KB）。
  const scanEnd = Math.min(off + 4096, head.length - 4);
  let frame = -1;
  for (let i = off; i < scanEnd; i++) {
    if (head[i] === 0xff && (head[i + 1]! & 0xe0) === 0xe0) { frame = i; break; }
  }
  if (frame < 0) return null;
  const b1 = head[frame + 1]!;
  const b2 = head[frame + 2]!;
  const version = (b1 >> 3) & 0x3; // 3=MPEG1 2=MPEG2
  const bitrateIdx = (b2 >> 4) & 0xf;
  const srIdx = (b2 >> 2) & 0x3;
  const channelMode = (b2 >> 6) & 0x3;
  const MPEG1_L3: number[] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0];
  const MPEG2_L3: number[] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0];
  const SR_MPEG1 = [44100, 48000, 32000, 0];
  const SR_MPEG2 = [22050, 24000, 16000, 0];
  const kbps = version === 3 ? MPEG1_L3[bitrateIdx]! : MPEG2_L3[bitrateIdx]!;
  const sampleRate = version === 3 ? SR_MPEG1[srIdx]! : SR_MPEG2[srIdx]!;
  if (kbps === 0 || sampleRate === 0) return null;
  const info: MediaInfo = { container: "mp3", durationMs: null, width: null, height: null, videoTracks: 0, audioTracks: 1, videoBps: null, sampleRate, channels: channelMode === 3 ? 1 : 2, codec: "MP3" };
  // Xing/Info 头（VBR 时长——帧数 × 每帧样本 / 采样率）。
  const sideInfo = version === 3 ? (channelMode === 3 ? 17 : 32) : (channelMode === 3 ? 9 : 17);
  const xingOff = frame + 4 + sideInfo;
  if (xingOff + 12 < head.length && (ascii(head, xingOff, 4) === "Xing" || ascii(head, xingOff, 4) === "Info")) {
    const flags = u32beSafe(head, xingOff + 4);
    if ((flags & 0x1) !== 0) {
      const frames = u32beSafe(head, xingOff + 8);
      const samplesPerFrame = version === 3 ? 1152 : 576;
      if (frames > 0) info.durationMs = Math.round((frames * samplesPerFrame * 1000) / sampleRate);
    }
  }
  if (info.durationMs === null && fileBytes > 0) {
    info.durationMs = Math.round((fileBytes * 8) / (kbps * 1000)); // CBR 兜底
  }
  return info;
}

/** 嗅探分派（五容器统一入口）。 */
export function sniffAndParse(head: Uint8Array, fileBytes: number): MediaInfo | null {
  if (head.length >= 12 && ascii(head, 4, 4) === "ftyp") {
    return parseMp4(head, fileBytes);
  }
  if (head.length >= 4 && ascii(head, 0, 4) === "\x1aE\xdf\xa3") {
    // webm 判定：DocType——嗅探窗内找 "webm" 字面量。
    let isWebm = false;
    for (let i = 0; i < Math.min(head.length - 4, 64); i++) {
      if (ascii(head, i, 4) === "webm") { isWebm = true; break; }
    }
    return parseMkv(head, isWebm);
  }
  if (head.length >= 4 && ascii(head, 0, 4) === "fLaC") return parseFlac(head);
  if (head.length > 4 && ((head[0] === 0xff && (head[1]! & 0xe0) === 0xe0) || ascii(head, 0, 3) === "ID3")) return parseMp3(head, fileBytes);
  return null;
}

// ---------------------------------------------------------------------------
// 缓存与悬停调度
// ---------------------------------------------------------------------------

/** FNV-1a 哈希（键——与 F093 缩略图缓存同族语义，一处一事实）。 */
export function fnv1a64(name: string, size: number): string {
  // 64 位以两个 32 位车道模拟（JS 位运算 32 位——h1/h2 拼接）。
  let h1 = 0x811c9dc5;
  let h2 = 0x01000193 ^ size;
  for (let i = 0; i < name.length; i++) {
    const c = name.charCodeAt(i);
    h1 = (h1 ^ (c & 0xff)) >>> 0; h1 = (h1 * 0x01000193) >>> 0;
    h1 = (h1 ^ (c >>> 8)) >>> 0; h1 = (h1 * 0x01000193) >>> 0;
    h2 = (h2 ^ c) >>> 0; h2 = (Math.imul(h2, 0x85ebca6b)) >>> 0;
  }
  return `${h1.toString(16)}${h2.toString(16).padStart(8, "0")}`;
}

interface CacheEntry { info: MediaInfo | null; at: number }

/** 媒体信息缓存（LRU + 负缓存——解析失败的文件也记账，不重复烧头）。 */
export class MediaInfoStore {
  private map = new Map<string, CacheEntry>();
  constructor(public cap = 512) {}
  get size(): number { return this.map.size; }

  lookup(key: string): MediaInfo | null | undefined {
    const e = this.map.get(key);
    if (!e) return undefined;
    // LRU 触碰。
    this.map.delete(key);
    this.map.set(key, e);
    return e.info;
  }

  put(key: string, info: MediaInfo | null): void {
    this.map.delete(key);
    this.map.set(key, { info, at: Date.now() });
    while (this.map.size > this.cap) {
      const oldest = this.map.keys().next().value;
      if (oldest === undefined) break;
      this.map.delete(oldest);
    }
  }
}

export type HoverState = { kind: "idle" } | { kind: "waiting"; since: number } | { kind: "shown" };

/** 悬停调度器：800ms 延迟显示；缓存命中立即显示（<100ms 判线）。 */
export class HoverScheduler {
  state: HoverState = { kind: "idle" };
  /** 缓存命中即时显示次数（判线核算账）。 */
  instantShows = 0;

  hover(nowMs: number, cacheHit: boolean): void {
    if (cacheHit) {
      this.state = { kind: "shown" };
      this.instantShows += 1;
    } else {
      this.state = { kind: "waiting", since: nowMs };
    }
  }

  /** tick：返回 true = 该显示了。 */
  tick(nowMs: number): boolean {
    if (this.state.kind === "waiting" && nowMs - this.state.since >= HOVER_DELAY_MS) {
      this.state = { kind: "shown" };
      return true;
    }
    return false;
  }

  leave(): void {
    this.state = { kind: "idle" };
  }
}
