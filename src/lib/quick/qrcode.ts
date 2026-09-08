/**
 * AI-07 · V-43 二维码速递（零出站红线：全部本地生成）：
 * - 本地生成库：qrcode-generator（Kazuhiko Arase，MIT，纯本地计算零网络）
 * - UTF-8 字节模式；容量评估（超限如实提示，绝不截断）
 * - 输出 SVG（浮窗矢量放大不失真）+ PNG dataURL（复制为图片）
 * 红线：不做扫码识别；不做二维码美化（保持标准可扫性）。
 */

import qrcode from "qrcode-generator";

// UTF-8 编码注入（默认 stringToBytes 为单字节）
const qrcodeFactory = qrcode as unknown as {
  (typeNumber: number, ecl: "L" | "M" | "Q" | "H"): {
    addData(data: string, mode?: string): void;
    make(): void;
    getModuleCount(): number;
    isDark(row: number, col: number): boolean;
    createDataURL(cellSize?: number, margin?: number): string;
  };
  stringToBytes: (s: string) => number[];
};
qrcodeFactory.stringToBytes = (s: string) => Array.from(new TextEncoder().encode(s));

export interface QrResult {
  /** 模块数（尺寸 = count × count）。 */
  moduleCount: number;
  /** SVG 字符串（scalable，浮窗放大用）。 */
  svg: string;
  /** PNG dataURL（复制为图片）。 */
  dataUrl: string;
}

/** 字节容量评估（version 40 + ECC L 上限 2953 字节；超限如实报错）。 */
export function qrByteCapacity(): number {
  return 2953;
}

/** UTF-8 字节数（容量评估口径）。 */
export function utf8Bytes(s: string): number {
  return new TextEncoder().encode(s).length;
}

/**
 * 生成二维码（本地、零出站）。超容量抛错（UI 层如实提示「内容超长」，
 * 绝不静默截断）。
 */
export function makeQr(text: string): QrResult {
  if (utf8Bytes(text) > qrByteCapacity()) {
    throw new Error(`内容超出二维码容量（${utf8Bytes(text)}/${qrByteCapacity()} 字节）`);
  }
  const qr = qrcodeFactory(0, "M"); // 0 = 自动选最小版本；M 级纠错
  qr.addData(text, "Byte");
  qr.make();
  const n = qr.getModuleCount();

  // SVG：白底黑模块，scalable
  let path = "";
  for (let r = 0; r < n; r++) {
    for (let c = 0; c < n; c++) {
      if (qr.isDark(r, c)) path += `M${c},${r}h1v1h-1z`;
    }
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${n + 8} ${n + 8}" shape-rendering="crispEdges"><rect width="${n + 8}" height="${n + 8}" fill="#fff"/><path transform="translate(4,4)" d="${path}" fill="#000"/></svg>`;

  return { moduleCount: n, svg, dataUrl: qr.createDataURL(4, 4) };
}

/** 是否可生成（容量评估，UI 预检用）。 */
export function canMakeQr(text: string): boolean {
  return utf8Bytes(text) <= qrByteCapacity();
}
