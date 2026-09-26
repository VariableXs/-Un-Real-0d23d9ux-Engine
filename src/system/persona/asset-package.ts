/**
 * F156/F165/F127 素材包深化 · vxtheme 资产包格式（清单 + PNG 字节 + 完整性）。
 *
 * 主册判据延伸：
 * - F127「打包全绿」的资产面：引擎产出的 PNG 字节进标准包——清单
 *   （manifest）逐条登记名称/尺寸/用途/CRC，解包逐条校验（篡改即拒）；
 * - 十四章「数据开放」：包格式公开、无锁——清单即文档；
 * - 「包体积预估（导入前可见）」：打包前先算总字节（persistence 同源纪律）。
 */

import { crc32 } from "./png-encode";

export type AssetUse = "pointer-sprite" | "boot-frame" | "icon" | "wallpaper" | "preview";

export interface AssetEntry {
  /** 包内路径（如 "pointer/arrow@2x.png"）。 */
  path: string;
  use: AssetUse;
  width: number;
  height: number;
  /** PNG 字节（真产物——来自 png-encode）。 */
  png: Uint8Array;
}

export interface AssetManifestItem {
  path: string;
  use: AssetUse;
  width: number;
  height: number;
  bytes: number;
  checksum: number;
}

export interface AssetPackage {
  format: "vxtheme-assets";
  version: 1;
  createdAt: number;
  manifest: AssetManifestItem[];
  /** 条目字节（与 manifest 平行——解包按序取）。 */
  blobs: Uint8Array[];
}

export interface PackResult {
  pkg: AssetPackage;
  totalBytes: number;
  /** 超预算 → 分批（诚实进度）。 */
  withinBudget: boolean;
}

/** 打包：逐条登记 CRC（打包前先校验 PNG 签名——垃圾进不来）。 */
export function packAssets(entries: AssetEntry[], createdAt: number, budgetBytes = 80 * 1024 * 1024): PackResult {
  const manifest: AssetManifestItem[] = [];
  const blobs: Uint8Array[] = [];
  for (const e of entries) {
    const sig = [...e.png.slice(0, 4)].map((b) => b.toString(16).padStart(2, "0")).join("");
    if (sig !== "89504e47") throw new Error(`${e.path}: 不是 PNG（签名 ${sig}）——素材管线产物必须经 png-encode`);
    manifest.push({ path: e.path, use: e.use, width: e.width, height: e.height, bytes: e.png.length, checksum: crc32(e.png) });
    blobs.push(e.png);
  }
  const totalBytes = manifest.reduce((s, m) => s + m.bytes, 0);
  return {
    pkg: { format: "vxtheme-assets", version: 1, createdAt, manifest, blobs },
    totalBytes,
    withinBudget: totalBytes <= budgetBytes,
  };
}

export type UnpackResult = { ok: true; entries: AssetEntry[] } | { ok: false; reason: string; badPaths: string[] };

/** 解包：格式/逐条 CRC/尺寸一致性三重校验（篡改即拒——F194 同源口径）。 */
export function unpackAssets(raw: unknown): UnpackResult {
  if (typeof raw !== "object" || raw === null) return { ok: false, reason: "包不是对象", badPaths: [] };
  const pkg = raw as Partial<AssetPackage>;
  if (pkg.format !== "vxtheme-assets" || pkg.version !== 1) return { ok: false, reason: `格式不符：${pkg.format}@${pkg.version}`, badPaths: [] };
  if (!Array.isArray(pkg.manifest) || !Array.isArray(pkg.blobs) || pkg.manifest.length !== pkg.blobs.length) {
    return { ok: false, reason: "清单与数据体不一致", badPaths: [] };
  }
  const entries: AssetEntry[] = [];
  const badPaths: string[] = [];
  pkg.manifest.forEach((m, i) => {
    const png = pkg.blobs![i]!;
    const sig = [...png.slice(0, 4)].map((b) => b.toString(16).padStart(2, "0")).join("");
    if (sig !== "89504e47" || crc32(png) !== m.checksum || png.length !== m.bytes) {
      badPaths.push(m.path);
      return;
    }
    entries.push({ path: m.path, use: m.use, width: m.width, height: m.height, png });
  });
  if (badPaths.length > 0) return { ok: false, reason: `${badPaths.length} 条校验失败（CRC/签名/长度）`, badPaths };
  return { ok: true, entries };
}

/** 路径契约：use → 目录前缀（整理不乱——第三方包也按此校验）。 */
export const USE_PREFIX: Record<AssetUse, string> = {
  "pointer-sprite": "pointer",
  "boot-frame": "boot",
  icon: "icons",
  wallpaper: "wallpaper",
  preview: "preview",
};

export function validatePath(use: AssetUse, path: string): boolean {
  return path.startsWith(`${USE_PREFIX[use]}/`) && path.endsWith(".png");
}

/** 双倍率 sprite 条目对（F156 4K 管线的打包习惯件）。 */
export function spriteEntryPair(shape: string, basePx: number, make: (size: number) => Uint8Array): AssetEntry[] {
  return [
    { path: `pointer/${shape}@1x.png`, use: "pointer-sprite", width: basePx, height: basePx, png: make(basePx) },
    { path: `pointer/${shape}@2x.png`, use: "pointer-sprite", width: basePx * 2, height: basePx * 2, png: make(basePx * 2) },
  ];
}
