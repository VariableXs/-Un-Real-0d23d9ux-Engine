/**
 * F156/F165 素材管线地基 · 纯 TS PNG 编码器 + 确定性程序化 sprite 光栅。
 *
 * 主册判据延伸：
 * - F156「双倍率 sprite 生成」「4K 放大验证」、F165「三档密度实际帧资产」——
 *   素材管线的最后一公里是把像素变成**真文件**：本模块零依赖实现 PNG
 *   编码（RFC 2083：CRC32 分块、zlib stored 块、Adler-32、扫描线过滤），
 *   产出可直接落盘的字节——素材不再依赖外部生产环境；
 * - 确定性程序化光栅：同一 seed 同一像素（mulberry32 同族纪律）——
 *   预览即真话，烘帧产物与运行时渲染逐位可对拍；
 * - 数据开放（十四章）：产物是标准 PNG，任何工具可读，无私有格式锁定。
 */

// ---------- CRC-32（ISO 3309 · PNG 分块校验） ----------

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[n] = c >>> 0;
  }
  return table;
})();

export function crc32(data: Uint8Array): number {
  let c = 0xffffffff;
  for (let i = 0; i < data.length; i++) {
    c = CRC_TABLE[(c ^ data[i]!) & 0xff]! ^ (c >>> 8);
  }
  return (c ^ 0xffffffff) >>> 0;
}

/** Adler-32（RFC 1950 · zlib 数据块校验）。 */
export function adler32(data: Uint8Array): number {
  let a = 1;
  let b = 0;
  for (let i = 0; i < data.length; i++) {
    a = (a + data[i]!) % 65521;
    b = (b + a) % 65521;
  }
  return ((b << 16) | a) >>> 0;
}

// ---------- zlib stored 块（无损不压缩——字节诚实，产物可预算） ----------

/** zlib 流（stored 模式）：每块 ≤65535 字节，BTYPE=00，长度/反长度双记。 */
export function zlibStore(data: Uint8Array): Uint8Array {
  const blocks = Math.max(1, Math.ceil(data.length / 65535));
  const out = new Uint8Array(2 + data.length + blocks * 5 + 4);
  const v = new DataView(out.buffer);
  out[0] = 0x78; // CMF：deflate/32K 窗口
  out[1] = 0x01; // FLG：最快（stored 允许）
  let src = 0;
  let dst = 2;
  for (let i = 0; i < blocks; i++) {
    const len = Math.min(65535, data.length - src);
    out[dst] = i === blocks - 1 ? 1 : 0; // BFINAL
    v.setUint16(dst + 1, len, true); // LEN（小端）
    v.setUint16(dst + 3, (~len) & 0xffff, true); // NLEN
    out.set(data.subarray(src, src + len), dst + 5);
    src += len;
    dst += 5 + len;
  }
  v.setUint32(dst, adler32(data), false);
  return out;
}

// ---------- PNG 编码 ----------

export interface RgbaBitmap {
  width: number;
  height: number;
  /** RGBA8，行优先，length = width*height*4。 */
  data: Uint8Array;
}

function chunk(type: string, payload: Uint8Array): Uint8Array {
  const out = new Uint8Array(12 + payload.length);
  const v = new DataView(out.buffer);
  v.setUint32(0, payload.length, false);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  out.set(payload, 8);
  v.setUint32(8 + payload.length, crc32(out.subarray(4, 8 + payload.length)), false);
  return out;
}

/**
 * RGBA8 → PNG 字节（真彩色 + alpha，filter 0 = None——编码诚实不追尺寸，
 * 字节数即可预算数：素材包大小在生成前可精确算出）。
 */
export function encodePng(bmp: RgbaBitmap): Uint8Array {
  const { width, height, data } = bmp;
  if (data.length !== width * height * 4) throw new Error("像素缓冲长度与尺寸不符");
  const ihdr = new Uint8Array(13);
  const v = new DataView(ihdr.buffer);
  v.setUint32(0, width, false);
  v.setUint32(4, height, false);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // color type RGBA
  ihdr[10] = 0; // compression
  ihdr[11] = 0; // filter
  ihdr[12] = 0; // interlace
  // 扫描线：每行前缀 filter byte 0。
  const raw = new Uint8Array(height * (1 + width * 4));
  for (let y = 0; y < height; y++) {
    raw[y * (1 + width * 4)] = 0;
    raw.set(data.subarray(y * width * 4, (y + 1) * width * 4), y * (1 + width * 4) + 1);
  }
  const sig = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const parts = [sig, chunk("IHDR", ihdr), chunk("IDAT", zlibStore(raw)), chunk("IEND", new Uint8Array(0))];
  const total = parts.reduce((s, p) => s + p.length, 0);
  const out = new Uint8Array(total);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

/** PNG 字节数预估（生成前预算——素材包大小先知道再生成）。 */
export function pngBytesEstimate(width: number, height: number): number {
  return 8 + 25 + 12 + (2 + 5 * Math.ceil((height * (1 + width * 4)) / 65535) + 4) + height * (1 + width * 4) + 12;
}

// ---------- 确定性程序化 sprite 光栅（F156 指针资产） ----------

export type PointerShape = "arrow" | "ibeam" | "cross" | "hand" | "wait-segment" | "arrow-watch";

/** SDF 有符号距离：形状的解析定义（像素级锐利——4K 放大验证的数学底）。 */
function sdf(shape: PointerShape, x: number, y: number, size: number): number {
  const u = x / size;
  const w = y / size;
  switch (shape) {
    case "arrow":
      // 经典箭头多边形：内部为负。
      const inBody = w < 0.15 + u * 1.7 && u < 0.55 - w * 0.25 && w < 0.85 - u * 1.4;
      return inBody ? -Math.min(u, w, 1 - w) * size : Math.min(Math.abs(u - 0.2), Math.abs(w - 0.4)) * size;
    case "ibeam":
      return (Math.abs(u - 0.5) < 0.07 && w > 0.1 && w < 0.9 ? -1 : 1) * Math.min(u, w) * size;
    case "cross": {
      const arm = 0.08 * size;
      const d1 = Math.max(Math.abs(u - 0.5) - 0.05, Math.abs(w - 0.5) - 0.42);
      const d2 = Math.max(Math.abs(w - 0.5) - 0.05, Math.abs(u - 0.5) - 0.42);
      return Math.min(Math.abs(d1), Math.abs(d2)) < arm / size ? -arm : arm;
    }
    case "hand": {
      // 简化手掌：掌 + 三指圆头（圆 SDF 并集）。
      const circles: [number, number, number][] = [[0.5, 0.68, 0.24], [0.32, 0.4, 0.09], [0.5, 0.34, 0.09], [0.68, 0.4, 0.09]];
      let d = Infinity;
      for (const [cx, cy, r] of circles) {
        d = Math.min(d, Math.hypot(u - cx, w - cy) - r);
      }
      return d * size;
    }
    case "wait-segment": {
      const r = Math.hypot(u - 0.5, w - 0.5);
      const a = Math.atan2(w - 0.5, u - 0.5);
      const seg = Math.floor(((a + Math.PI) / (2 * Math.PI)) * 8);
      const on = seg % 2 === 0;
      return r < 0.42 && r > 0.12 && on ? -0.05 * size : 0.05 * size;
    }
    case "arrow-watch":
      return sdf("arrow", x, y, size) < 0 ? sdf("wait-segment", x - size * 0.35, y - size * 0.45, size * 0.6) : sdf("arrow", x, y, size);
  }
}

export interface RasterOptions {
  size: number;
  color: string;
  outline: string;
  /** 1px 抗锯齿边缘宽度（物理像素——DPR 选档后不再糊）。 */
  aaPx: number;
}

/** SDF → RGBA 光栅（确定性：同参数同像素）。 */
export function rasterPointer(shape: PointerShape, opts: RasterOptions): RgbaBitmap {
  const { size, color, outline, aaPx } = opts;
  const rgb = parseHex(color);
  const line = parseHex(outline);
  const data = new Uint8Array(size * size * 4);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      // 半像素中心采样（像素中心即采样点——1px 级对拍口径）。
      const d = sdf(shape, x + 0.5, y + 0.5, size);
      const o = (y * size + x) * 4;
      if (d < 0) {
        data[o] = rgb[0];
        data[o + 1] = rgb[1];
        data[o + 2] = rgb[2];
        data[o + 3] = 255;
      } else if (d < aaPx) {
        // 边缘：白描边混合（指针在深浅底都可见的轮廓纪律）。
        const t = d / aaPx;
        const inner = 1 - t;
        data[o] = Math.round(line[0] * inner + rgb[0] * t);
        data[o + 1] = Math.round(line[1] * inner + rgb[1] * t);
        data[o + 2] = Math.round(line[2] * inner + rgb[2] * t);
        data[o + 3] = 255;
      }
    }
  }
  return { width: size, height: size, data };
}

function parseHex(hex: string): [number, number, number] {
  const m = /^#([0-9a-fA-F]{6})/.exec(hex);
  if (!m) throw new Error(`非法颜色 ${hex}`);
  const n = parseInt(m[1]!, 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

/** 双倍率 sprite 产物对（1x/2x——4K 管线标准输出）。 */
export function pointerSpritePair(shape: PointerShape, basePx: number, color: string, outline: string): { png1x: Uint8Array; png2x: Uint8Array; bytes1x: number; bytes2x: number } {
  const one = rasterPointer(shape, { size: basePx, color, outline, aaPx: 1 });
  const two = rasterPointer(shape, { size: basePx * 2, color, outline, aaPx: 2 });
  const png1x = encodePng(one);
  const png2x = encodePng(two);
  return { png1x, png2x, bytes1x: png1x.length, bytes2x: png2x.length };
}

// ---------- 素材包预算（生成前知道总字节——诚实进度条的数据源） ----------

export interface AssetBudgetItem {
  name: string;
  bytes: number;
}

export interface AssetBudget {
  items: AssetBudgetItem[];
  totalBytes: number;
  /** 超过预算上限 → 分批生成（诚实进度而不是卡死）。 */
  withinBudget: boolean;
}

export function assetBudget(items: AssetBudgetItem[], limitBytes: number): AssetBudget {
  const totalBytes = items.reduce((s, i) => s + i.bytes, 0);
  return { items, totalBytes, withinBudget: totalBytes <= limitBytes };
}
