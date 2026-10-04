// UNREAL-X：AI-12 族0114「桌面打印」（X02826~X02850）。
// 桌面内容（壁纸/图标布局/清单）输出到纸张的页面编排、缩放适配与耗材估算。

export type PaperSize = 'A4' | 'A5' | 'B5' | 'Letter' | 'Legal';
export type Orientation = 'portrait' | 'landscape';
export type ColorMode = 'color' | 'grayscale' | 'mono';
export type Duplex = 'off' | 'long-edge' | 'short-edge';

/** 纸张尺寸（mm）。 */
export const PAPER_MM: Record<PaperSize, { w: number; h: number }> = {
  A4: { w: 210, h: 297 },
  A5: { w: 148, h: 210 },
  B5: { w: 176, h: 250 },
  Letter: { w: 216, h: 279 },
  Legal: { w: 216, h: 356 },
};

export const MAX_COPIES = 999;
export const DEFAULT_DPI = 300;

export interface PrintOptions {
  paper: PaperSize;
  orientation: Orientation;
  marginMm: number;
  dpi: number;
  color: ColorMode;
  duplex: Duplex;
  copies: number;
  collate: boolean;
  header: boolean;
  footer: boolean;
  watermark: string;
  fit: 'fit' | 'fill' | 'actual';
}

export const DEFAULT_PRINT: PrintOptions = {
  paper: 'A4',
  orientation: 'portrait',
  marginMm: 10,
  dpi: DEFAULT_DPI,
  color: 'color',
  duplex: 'off',
  copies: 1,
  collate: true,
  header: true,
  footer: true,
  watermark: '',
  fit: 'fit',
};

/** 五档打印质量矩阵。 */
export const PRINT_LEVELS: Array<{ level: number; dpi: number; color: ColorMode }> = [
  { level: 0, dpi: 150, color: 'mono' },
  { level: 1, dpi: 200, color: 'grayscale' },
  { level: 2, dpi: 300, color: 'color' },
  { level: 3, dpi: 600, color: 'color' },
  { level: 4, dpi: 1200, color: 'color' },
];

const clamp = (v: number, lo: number, hi: number): number => (v < lo ? lo : v > hi ? hi : v);

/** 可印区域（mm）：纸张减去四周页边距。 */
export function printableArea(o: PrintOptions): { w: number; h: number } {
  const p = PAPER_MM[o.paper];
  const base = o.orientation === 'landscape' ? { w: p.h, h: p.w } : { w: p.w, h: p.h };
  const m = clamp(o.marginMm, 0, Math.floor(Math.min(base.w, base.h) / 2 - 1));
  return { w: base.w - m * 2, h: base.h - m * 2 };
}

/** 适配缩放：内容等比装入可印区（fit 不放大）。 */
export function fitScale(o: PrintOptions, contentPx: { w: number; h: number }): number {
  const area = printableArea(o);
  const mmPerPx = 25.4 / Math.max(1, o.dpi);
  const cw = contentPx.w * mmPerPx;
  const ch = contentPx.h * mmPerPx;
  if (cw <= 0 || ch <= 0) return 1;
  const raw = Math.min(area.w / cw, area.h / ch);
  if (o.fit === 'actual') return 1;
  if (o.fit === 'fill') return Number(raw.toFixed(4));
  return Number(Math.min(1, raw).toFixed(4));
}

/** N-up 排版：每页放几版。 */
export function nUpPages(total: number, perPage: number): number {
  if (total <= 0) return 0;
  const n = clamp(Math.round(perPage), 1, 16);
  return Math.ceil(total / n);
}

/** 双面用纸张数。 */
export function sheets(pages: number, duplex: Duplex, copies: number): number {
  const perCopy = duplex === 'off' ? pages : Math.ceil(pages / 2);
  return perCopy * clamp(Math.round(copies), 1, MAX_COPIES);
}

export interface PrintJob {
  id: string;
  pages: number;
  sheets: number;
  dpi: number;
  color: ColorMode;
  createdAt: number;
  status: 'queued' | 'printing' | 'done' | 'failed';
}

export class PrintStudio {
  private opts: PrintOptions = { ...DEFAULT_PRINT };
  private jobs: PrintJob[] = [];

  constructor(patch?: Partial<PrintOptions>) {
    if (patch) this.configure(patch);
  }

  get options(): PrintOptions {
    return { ...this.opts };
  }

  configure(patch: Partial<PrintOptions>): boolean {
    let ok = true;
    if (patch.paper !== undefined) {
      if (patch.paper in PAPER_MM) this.opts.paper = patch.paper;
      else ok = false;
    }
    if (patch.orientation !== undefined) {
      if (patch.orientation === 'portrait' || patch.orientation === 'landscape') this.opts.orientation = patch.orientation;
      else ok = false;
    }
    if (patch.marginMm !== undefined) {
      if (patch.marginMm >= 0 && patch.marginMm <= 50) this.opts.marginMm = patch.marginMm;
      else {
        this.opts.marginMm = DEFAULT_PRINT.marginMm;
        ok = false;
      }
    }
    if (patch.dpi !== undefined) {
      if (patch.dpi >= 72 && patch.dpi <= 2400) this.opts.dpi = Math.round(patch.dpi);
      else {
        this.opts.dpi = DEFAULT_DPI;
        ok = false;
      }
    }
    if (patch.copies !== undefined) {
      if (patch.copies >= 1 && patch.copies <= MAX_COPIES) this.opts.copies = Math.round(patch.copies);
      else {
        this.opts.copies = 1;
        ok = false;
      }
    }
    if (patch.color !== undefined) this.opts.color = patch.color;
    if (patch.duplex !== undefined) this.opts.duplex = patch.duplex;
    if (patch.fit !== undefined) this.opts.fit = patch.fit;
    if (patch.collate !== undefined) this.opts.collate = !!patch.collate;
    if (patch.header !== undefined) this.opts.header = !!patch.header;
    if (patch.footer !== undefined) this.opts.footer = !!patch.footer;
    if (patch.watermark !== undefined) this.opts.watermark = patch.watermark.slice(0, 24);
    return ok;
  }

  applyLevel(level: number): PrintOptions {
    const idx = clamp(Math.round(level), 0, PRINT_LEVELS.length - 1);
    const l = PRINT_LEVELS[idx]!;
    this.configure({ dpi: l.dpi, color: l.color });
    return this.options;
  }

  /** 耗材估算（permille 覆盖率墨量，纯本地经验模型）。 */
  inkEstimate(pages: number, coveragePermille: number): number {
    const cov = clamp(coveragePermille, 0, 1000) / 1000;
    const factor = this.opts.color === 'color' ? 3 : this.opts.color === 'grayscale' ? 1.4 : 1;
    return Math.round(pages * cov * factor * 100) / 100;
  }

  /** 提交作业：去重（同 id 不重复入队）。 */
  submit(id: string, pages: number, at: number): { ok: boolean; reason?: string } {
    if (pages <= 0) return { ok: false, reason: 'E01 · 页数非法，请检查打印范围' };
    if (this.jobs.some((j) => j.id === id)) return { ok: false, reason: 'E06 · 同名作业已在队列中，请等待完成' };
    this.jobs.push({
      id,
      pages,
      sheets: sheets(pages, this.opts.duplex, this.opts.copies),
      dpi: this.opts.dpi,
      color: this.opts.color,
      createdAt: at,
      status: 'queued',
    });
    return { ok: true };
  }

  advance(id: string): boolean {
    const j = this.jobs.find((x) => x.id === id);
    if (!j || j.status === 'done') return false;
    j.status = j.status === 'queued' ? 'printing' : 'done';
    return true;
  }

  list(): PrintJob[] {
    return this.jobs.map((j) => ({ ...j }));
  }

  get count(): number {
    return this.jobs.length;
  }

  /** 微文案。 */
  hint(): string {
    return this.opts.duplex === 'off' ? '单面打印' : '双面打印，注意装订边';
  }

  /** 无障碍等价通道：单色模式 + 页眉页脚可关闭。 */
  a11yAdjust(): PrintOptions {
    this.opts.color = 'mono';
    this.opts.watermark = '';
    return this.options;
  }

  uninstall(): boolean {
    this.jobs = [];
    this.opts = { ...DEFAULT_PRINT };
    return this.jobs.length === 0;
  }
}
