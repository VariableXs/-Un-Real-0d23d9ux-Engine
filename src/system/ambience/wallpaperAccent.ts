/**
 * AI-18 M-64 壁纸主色主题采样 — OKLCH 聚类取主色（本地计算，绝不自动生效）。
 *
 * 口径：
 * - canvas 降采样（64px）→ 像素 → OKLCH 空间 k-means（k=3，亮度排序初始化）；
 * - 过暗/过曝主色亮度钳制 0.25..0.82（纯白壁纸不产生刺眼过曝）；
 * - 采样 → 预览 → 应用三步分离（applyAccent 只在用户点击候选色后调用）。
 */

export function rgbToOklch(r: number, g: number, b: number): [number, number, number] {
  const lin = (v: number): number => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  const [lr, lg, lb] = [lin(r), lin(g), lin(b)];
  const l = 0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb;
  const m = 0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb;
  const s = 0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb;
  const l_ = Math.cbrt(l), m_ = Math.cbrt(m), s_ = Math.cbrt(s);
  const L = 0.2104542553 * l_ + 0.793617785 * m_ - 0.0040720468 * s_;
  const A = 1.9779984951 * l_ - 2.428592205 * m_ + 0.4505937099 * s_;
  const B = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.808675766 * s_;
  const C = Math.sqrt(A * A + B * B);
  let H = (Math.atan2(B, A) * 180) / Math.PI;
  if (H < 0) H += 360;
  return [L, C, H];
}

/** OKLCH → CSS 颜色（现代浏览器 OKLCH 直出，色彩空间无损）。 */
export function oklchToCss(L: number, C: number, H: number): string {
  return `oklch(${(Math.round(L * 1000) / 1000).toString()} ${(Math.round(C * 1000) / 1000).toString()} ${Math.round(H)}deg)`;
}

/** OKLCH → sRGB hex（V-80 对比度守护需要 hex 输入；够用的近似转换）。 */
export function oklchToHex(L: number, C: number, H: number): string {
  const hRad = (H * Math.PI) / 180;
  const a = C * Math.cos(hRad);
  const b = C * Math.sin(hRad);
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
  const s_ = L - 0.0894841775 * a - 1.291485548 * b;
  const l = l_ * l_ * l_;
  const m = m_ * m_ * m_;
  const s = s_ * s_ * s_;
  const lr = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
  const lg = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
  const lb = -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s;
  const gamma = (v: number): number => {
    const c = v <= 0.0031308 ? 12.92 * v : 1.055 * Math.pow(v, 1 / 2.4) - 0.055;
    return Math.round(Math.min(255, Math.max(0, c * 255)));
  };
  const hx = (n: number): string => n.toString(16).padStart(2, "0");
  return `#${hx(gamma(lr))}${hx(gamma(lg))}${hx(gamma(lb))}`;
}

export interface SamplePixel {
  rgb: [number, number, number];
  oklch: [number, number, number];
}

/** 图片降采样采样（失败/跨域 = null）。 */
export async function sampleImagePixels(url: string, size = 64): Promise<SamplePixel[] | null> {
  if (typeof document === "undefined") return null;
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = (): void => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = size;
        canvas.height = size;
        const ctx = canvas.getContext("2d", { willReadFrequently: true });
        if (!ctx) {
          resolve(null);
          return;
        }
        ctx.drawImage(img, 0, 0, size, size);
        const data = ctx.getImageData(0, 0, size, size).data;
        const out: SamplePixel[] = [];
        for (let i = 0; i + 3 < data.length; i += 4) {
          const r = data[i] ?? 0, g = data[i + 1] ?? 0, b = data[i + 2] ?? 0;
          if ((data[i + 3] ?? 255) < 128) continue; // 透明跳过
          out.push({ rgb: [r, g, b], oklch: rgbToOklch(r, g, b) });
        }
        resolve(out.length > 0 ? out : null);
      } catch {
        resolve(null); // 跨域污染 canvas：如实返回 null
      }
    };
    img.onerror = (): void => resolve(null);
    img.src = url;
  });
}

/** OKLCH k-means 聚类取 k 个主色（亮度分位点初始化，比随机稳定）。 */
export function clusterDominant(pixels: SamplePixel[], k = 3, iterations = 12): [number, number, number][] {
  if (pixels.length === 0) return [];
  const kk = Math.min(k, pixels.length);
  // 初始化：按明度排序取分位点（比随机更稳定）
  const sorted = [...pixels].sort((a, b) => a.oklch[0] - b.oklch[0]);
  let centers: [number, number, number][] = Array.from({ length: kk }, (_, i) => {
    const p = sorted[Math.floor(((i + 0.5) / kk) * sorted.length)];
    return [...(p?.oklch ?? [0.5, 0.1, 0])] as [number, number, number];
  });
  for (let it = 0; it < iterations; it++) {
    const sums: number[][] = centers.map(() => [0, 0, 0, 0]);
    for (const p of pixels) {
      const [l2, c2, h2] = p.oklch;
      let best = 0, bestD = Infinity;
      for (let c = 0; c < centers.length; c++) {
        const ct = centers[c];
        if (!ct) continue;
        const [l1, c1, h1] = ct;
        // 色相环差
        let dh = Math.abs(h1 - h2);
        if (dh > 180) dh = 360 - dh;
        const d = Math.pow((l1 - l2) * 0.8, 2) + Math.pow(c1 - c2, 2) + Math.pow(dh / 3, 2);
        if (d < bestD) { bestD = d; best = c; }
      }
      const s = sums[best];
      if (!s) break;
      s[0] = (s[0] ?? 0) + l2; s[1] = (s[1] ?? 0) + c2; s[2] = (s[2] ?? 0) + h2; s[3] = (s[3] ?? 0) + 1;
    }
    centers = sums.map((s, i) => {
      const n = s[3] ?? 0;
      return n > 0
        ? ([(s[0] ?? 0) / n, (s[1] ?? 0) / n, (s[2] ?? 0) / n] as [number, number, number])
        : (centers[i] ?? [0.5, 0.1, 0]);
    });
  }
  // 过滤过暗/过曝主色（纯白壁纸不产生刺眼过曝：亮度钳制 0.25..0.82）
  return centers
    .map(([l, c, h]) => [Math.min(0.82, Math.max(0.25, l)), c, h] as [number, number, number])
    .slice(0, k);
}

/** 采样 → 三候选主色（OKLCH）。 */
export async function sampleAccentCandidates(url: string): Promise<[number, number, number][]> {
  const px = await sampleImagePixels(url);
  if (!px) return [];
  return clusterDominant(px, 3);
}

/** 应用候选色（仅用户点击候选色后调用 —— 采样→预览→应用三步分离，绝不偷跑）。 */
export function applyAccent(candidate: [number, number, number]): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  const css = oklchToCss(candidate[0], candidate[1], candidate[2]);
  root.style.setProperty("--accent", css);
  root.style.setProperty("--accent-soft", css.replace(")", " / 0.16)"));
}
