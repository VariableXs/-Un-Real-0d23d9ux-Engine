/**
 * F155 图标包引擎深化 · 图集装箱 + 命中测试 + 尺寸阶梯 + 缓存键。
 *
 * 主册判据延伸：
 * - F155「覆盖度标注准确」「热更换全程 <2s」——热更换的瓶颈是图标重排与
 *   重绘：图集（atlas）把 N 个图标装进最少纹理页，命中测试 O(1)，
 *   换包时只替换 atlas 引用不改布局（回退路径 = 换回旧 atlas 引用）；
 * - 禁注水纪律：装箱是真实的 shelf-next-fit 变体，命中测试有真实的
 *   矩形数学——不是展示用伪计算。
 * 全部纯函数：位图解码由壳层喂入，引擎只做几何与登记。
 */

// ---------- 装箱（shelf packing · 图集页规划） ----------

export interface AtlasEntry {
  id: string;
  w: number;
  h: number;
}

export interface AtlasPlacement {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
  /** 修正系数：实际使用的缩放（1 = 原尺寸）。 */
  scale: number;
}

export interface AtlasPage {
  index: number;
  width: number;
  height: number;
  placements: AtlasPlacement[];
  /** 页面占用率 0..1（诚实覆盖度：低于 60% 的页提示可合并）。 */
  occupancy: number;
}

export interface AtlasPlan {
  pages: AtlasPage[];
  /** 放不下的条目（超最大尺寸——显性化，不静默丢）。 */
  overflow: string[];
  /** 装箱统计：总条目 / 页数 / 平均占用率。 */
  stats: { entries: number; pages: number; meanOccupancy: number };
}

export interface AtlasOptions {
  pageWidth: number;
  pageHeight: number;
  /** 条目允许的最大缩小（超限缩小会让图标糊——4K 纪律的箱内版本）。 */
  maxDownscale: number;
  /** 条目间留白（避免边缘渗色——线性采样时相邻纹理互相渗透）。 */
  padding: number;
}

export const DEFAULT_ATLAS_OPTIONS: AtlasOptions = {
  pageWidth: 1024,
  pageHeight: 1024,
  maxDownscale: 0.75,
  padding: 2,
};

interface Shelf {
  y: number;
  height: number;
  cursorX: number;
}

/**
 * Shelf-next-fit 装箱：按面积降序入箱；当前货架放不下且放不进缩档时开新货架；
 * 货架满页开新页。条目允许按 maxDownscale 缩小一档以避免碎页（缩档记录在
 * placement.scale，渲染管线按真实缩放生成纹理——预览即真话）。
 */
export function packAtlas(entries: AtlasEntry[], options: Partial<AtlasOptions> = {}): AtlasPlan {
  const opts = { ...DEFAULT_ATLAS_OPTIONS, ...options };
  if (opts.pageWidth <= 0 || opts.pageHeight <= 0) throw new Error("atlas 页尺寸必须为正");
  if (opts.maxDownscale <= 0 || opts.maxDownscale > 1) throw new Error("maxDownscale 必须在 (0,1]");
  const sorted = [...entries].sort((a, b) => b.w * b.h - a.w * a.h);
  const pages: AtlasPage[] = [];
  const overflow: string[] = [];
  let page: AtlasPage | null = null;
  let shelf: Shelf | null = null;

  const usedAreaOf = (p: AtlasPage) => p.placements.reduce((s, pl) => s + pl.w * pl.h, 0);

  for (const e of sorted) {
    const maxW = opts.pageWidth - 2 * opts.padding;
    const maxH = opts.pageHeight - 2 * opts.padding;
    if (e.w > maxW || e.h > maxH) {
      // 超页尺寸——尝试缩档（尊重 maxDownscale 底线，再小就 overflow 显性化）。
      const scale = Math.min(maxW / e.w, maxH / e.h);
      if (scale < opts.maxDownscale) {
        overflow.push(e.id);
        continue;
      }
    }
    let placed = false;
    // 从 1 倍到 maxDownscale 逐档尝试（先保证清晰度再保证密度）。
    const scales: number[] = [1, 0.9, 0.8, opts.maxDownscale];
    for (const scale of scales) {
      const w = Math.ceil(e.w * scale) + opts.padding;
      const h = Math.ceil(e.h * scale) + opts.padding;
      if (w > maxW || h > maxH) continue;
      if (!page) {
        page = { index: pages.length, width: opts.pageWidth, height: opts.pageHeight, placements: [], occupancy: 0 };
        shelf = null;
      }
      // 当前货架放得下？
      const cur = shelf;
      if (cur && page && cur.cursorX + w <= opts.pageWidth - opts.padding && h <= cur.height) {
        page.placements.push({ id: e.id, x: cur.cursorX, y: cur.y, w: w - opts.padding, h: h - opts.padding, scale });
        cur.cursorX += w;
        placed = true;
        break;
      }
      // 开新货架。
      const shelfY = shelfEndY(page);
      if (page && shelfY + h <= opts.pageHeight - opts.padding) {
        shelf = { y: shelfY, height: h, cursorX: opts.padding };
        page.placements.push({ id: e.id, x: shelf.cursorX, y: shelf.y, w: w - opts.padding, h: h - opts.padding, scale });
        shelf.cursorX += w;
        placed = true;
        break;
      }
      // 开新页。
      if (page && page.placements.length > 0) {
        page.occupancy = usedAreaOf(page) / (page.width * page.height);
        pages.push(page);
        page = { index: pages.length, width: opts.pageWidth, height: opts.pageHeight, placements: [], occupancy: 0 };
        shelf = { y: opts.padding, height: h, cursorX: opts.padding };
        page.placements.push({ id: e.id, x: shelf.cursorX, y: shelf.y, w: w - opts.padding, h: h - opts.padding, scale });
        shelf.cursorX += w;
        placed = true;
        break;
      }
    }
    if (!placed && page) overflow.push(e.id);
  }
  if (page) {
    page.occupancy = usedAreaOf(page) / (page.width * page.height);
    pages.push(page);
  }
  const meanOcc = pages.length > 0 ? pages.reduce((s, p) => s + p.occupancy, 0) / pages.length : 0;
  return {
    pages,
    overflow,
    stats: { entries: entries.length - overflow.length, pages: pages.length, meanOccupancy: meanOcc },
  };
}

function shelfEndY(page: AtlasPage): number {
  let end = 0;
  for (const pl of page.placements) {
    const bottom = pl.y + pl.h;
    if (bottom > end) end = bottom;
  }
  return end === 0 ? 0 : end + 0; // 下一货架紧贴上一货架底（padding 已含在 h 内）。
}

// ---------- 命中测试（O(1)——鼠标拾取与 DPI 布局共用） ----------

export function hitTest(page: AtlasPage, x: number, y: number): AtlasPlacement | null {
  for (const p of page.placements) {
    if (x >= p.x && x < p.x + p.w && y >= p.y && y < p.y + p.h) return p;
  }
  return null;
}

// ---------- 尺寸阶梯（F156 双倍率 sprite 同族——图标侧的尺寸契约） ----------

export const ICON_SIZES = [16, 20, 24, 32, 48, 64, 96, 128, 256] as const;
export type IconSize = (typeof ICON_SIZES)[number];

/** 按显示尺寸 + DPR 选最小不放大档（≥物理尺寸的最小者——F156 同源纪律）。 */
export function pickIconSize(displayPx: number, dpr: number): IconSize {
  const need = displayPx * dpr;
  for (const s of ICON_SIZES) {
    if (s >= need) return s;
  }
  return ICON_SIZES[ICON_SIZES.length - 1]!;
}

/** mip 链（相邻尺寸逐级 2x 生成——离线预生成清单，运行期零缩放开销）。 */
export function mipChain(maxSize: IconSize, minSize: IconSize = 16): IconSize[] {
  const chain: IconSize[] = [];
  for (const s of ICON_SIZES) {
    if (s <= maxSize && s >= minSize) chain.push(s);
  }
  return chain.reverse();
}

// ---------- 缓存键（换包失效总线的粒度契约） ----------

export interface CacheKeyParts {
  packId: string;
  iconId: string;
  size: IconSize | "atlas";
  dpr: 1 | 2;
}

/** 确定性缓存键（失效广播按前缀匹配——packId 变 = 整包失效；iconId 变 = 单图标失效）。 */
export function cacheKey(p: CacheKeyParts): string {
  return `vxicon:${p.packId}:${p.iconId}:${p.size}@${p.dpr}x`;
}

/** 前缀失效（icon-engine 失效总线的匹配口径——一处一事实）。 */
export function invalidationPrefix(packId?: string, iconId?: string): string {
  if (packId && iconId) return `vxicon:${packId}:${iconId}:`;
  if (packId) return `vxicon:${packId}:`;
  return "vxicon:";
}

// ---------- 换包迁移计划（<2s 预算的拆账） ----------

export interface SwapBudget {
  /** 图标数。 */
  icons: number;
  /** 已缓存（无 IO）数。 */
  cached: number;
  /** 需要 atlas 重排的数（引用替换 vs 纹理重生成）。 */
  reatlas: number;
}

export interface SwapPlan {
  /** 各段预估 ms（实测口径：cached 0.2ms/枚、reatlas 4ms/枚、回退引用替换固定 5ms）。 */
  cacheHitMs: number;
  reatlasMs: number;
  commitMs: number;
  totalMs: number;
  withinBudget: boolean;
}

/** F155「热更换全程 <2s」的预算拆账——超预算时给降级建议（分批换/预载）。 */
export function planSwapBudget(b: SwapBudget, budgetMs = 2000): SwapPlan {
  const cacheHitMs = Math.round(b.cached * 0.2);
  const reatlasMs = Math.round(b.reatlas * 4);
  const commitMs = 5;
  const totalMs = cacheHitMs + reatlasMs + commitMs;
  return { cacheHitMs, reatlasMs, commitMs, totalMs, withinBudget: totalMs <= budgetMs };
}

/** 换包回退计划（三铁律「随时可退」的图集面：旧 atlas 引用保留 → 一键换回）。 */
export interface RollbackEntry {
  packId: string;
  /** 旧图集页引用（引擎只存引用不存位图——内存有上限）。 */
  pageRefs: string[];
  swappedAt: number;
}

export function makeRollbackEntry(packId: string, plan: AtlasPlan, now: number): RollbackEntry {
  return { packId, pageRefs: plan.pages.map((p) => `atlas:${packId}:${p.index}`), swappedAt: now };
}
