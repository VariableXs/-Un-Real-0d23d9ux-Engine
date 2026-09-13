// UNREAL-X：AI-12 族0115「桌面导出」（X02851~X02875）。
// 桌面配置/图标布局/壁纸清单的导出序列化、路径净化、校验与批处理。

export type ExportFormat = 'json' | 'yaml' | 'csv' | 'svg' | 'zip';
export const EXPORT_FORMATS: ExportFormat[] = ['json', 'yaml', 'csv', 'svg', 'zip'];

export interface ExportOptions {
  format: ExportFormat;
  pretty: boolean;
  includeMeta: boolean;
  includeAssets: boolean;
  maxItems: number;
  pathSafe: boolean;
}

export const DEFAULT_EXPORT: ExportOptions = {
  format: 'json',
  pretty: true,
  includeMeta: true,
  includeAssets: false,
  maxItems: 500,
  pathSafe: true,
};

export const EXPORT_LEVELS: Array<{ level: number; format: ExportFormat; includeAssets: boolean }> = [
  { level: 0, format: 'csv', includeAssets: false },
  { level: 1, format: 'json', includeAssets: false },
  { level: 2, format: 'yaml', includeAssets: false },
  { level: 3, format: 'svg', includeAssets: true },
  { level: 4, format: 'zip', includeAssets: true },
];

const clamp = (v: number, lo: number, hi: number): number => (v < lo ? lo : v > hi ? hi : v);

/** FNV-1a 32 位校验和（本地确定性，无第三方依赖）。 */
export function checksum(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, '0');
}

/** 路径净化：去掉盘符/上级引用/控制字符，防目录穿越。 */
export function sanitizePath(p: string): string {
  const cleaned = p
    .replace(/[<>:"|?*\u0000-\u001f]/g, '')
    .replace(/^[a-zA-Z]:/, '')
    .replace(/\.\./g, '')
    .replace(/\\/g, '/')
    .replace(/\/{2,}/g, '/')
    .replace(/^\/+/, '');
  return cleaned.slice(0, 120);
}

export interface DesktopItem {
  name: string;
  x: number;
  y: number;
  kind: 'app' | 'file' | 'folder' | 'widget';
}

export class ExportStudio {
  private opts: ExportOptions = { ...DEFAULT_EXPORT };
  private items: DesktopItem[] = [];

  constructor(patch?: Partial<ExportOptions>) {
    if (patch) this.configure(patch);
  }

  get options(): ExportOptions {
    return { ...this.opts };
  }

  configure(patch: Partial<ExportOptions>): boolean {
    let ok = true;
    if (patch.format !== undefined) {
      if (EXPORT_FORMATS.includes(patch.format)) this.opts.format = patch.format;
      else ok = false;
    }
    if (patch.maxItems !== undefined) {
      if (patch.maxItems >= 1 && patch.maxItems <= 10_000) this.opts.maxItems = Math.round(patch.maxItems);
      else {
        this.opts.maxItems = DEFAULT_EXPORT.maxItems;
        ok = false;
      }
    }
    if (patch.pretty !== undefined) this.opts.pretty = !!patch.pretty;
    if (patch.includeMeta !== undefined) this.opts.includeMeta = !!patch.includeMeta;
    if (patch.includeAssets !== undefined) this.opts.includeAssets = !!patch.includeAssets;
    if (patch.pathSafe !== undefined) this.opts.pathSafe = !!patch.pathSafe;
    return ok;
  }

  applyLevel(level: number): ExportOptions {
    const idx = clamp(Math.round(level), 0, EXPORT_LEVELS.length - 1);
    const l = EXPORT_LEVELS[idx]!;
    this.configure({ format: l.format, includeAssets: l.includeAssets });
    return this.options;
  }

  /** 登记去重：同名同坐标不重复入册。 */
  add(item: DesktopItem): boolean {
    if (this.items.length >= this.opts.maxItems) return false;
    if (this.items.some((i) => i.name === item.name)) return false;
    this.items.push(item);
    return true;
  }

  remove(name: string): boolean {
    const before = this.items.length;
    this.items = this.items.filter((i) => i.name !== name);
    return this.items.length !== before;
  }

  get count(): number {
    return this.items.length;
  }

  /** JSON 序列化（pretty 缩进两空格）。 */
  toJson(): string {
    const body = this.opts.includeMeta
      ? { meta: { version: 1, count: this.items.length, format: this.opts.format }, items: this.items }
      : { items: this.items };
    return this.opts.pretty ? JSON.stringify(body, null, 2) : JSON.stringify(body);
  }

  /** YAML（极简子集，仅键值对与列表）。 */
  toYaml(): string {
    const head = this.opts.includeMeta ? `version: 1\ncount: ${this.items.length}\n` : '';
    const body = this.items.map((i) => `- name: ${i.name}\n  x: ${i.x}\n  y: ${i.y}\n  kind: ${i.kind}`).join('\n');
    return `${head}items:\n${body}${this.items.length ? '\n' : ''}`;
  }

  toCsv(): string {
    const head = 'name,x,y,kind\n';
    return head + this.items.map((i) => `${i.name},${i.x},${i.y},${i.kind}`).join('\n');
  }

  toSvg(): string {
    const rects = this.items
      .map((i) => `<rect x="${i.x}" y="${i.y}" width="64" height="64" data-name="${i.name}" fill="none"/>`)
      .join('');
    return `<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600">${rects}</svg>`;
  }

  serialize(): { text: string; bytes: number; sum: string } {
    const text =
      this.opts.format === 'json'
        ? this.toJson()
        : this.opts.format === 'yaml'
          ? this.toYaml()
          : this.opts.format === 'csv'
            ? this.toCsv()
            : this.opts.format === 'svg'
              ? this.toSvg()
              : `PK\x03\x04${this.toJson()}`;
    return { text, bytes: new TextEncoder().encode(text).length, sum: checksum(text) };
  }

  /** 往返导入：只吃自己吐出的格式。 */
  importJson(text: string): number {
    try {
      const parsed = JSON.parse(text) as { items?: DesktopItem[] };
      const list = parsed.items ?? [];
      let n = 0;
      for (const it of list) if (this.add(it)) n += 1;
      return n;
    } catch {
      return 0;
    }
  }

  /** 批处理进度（按页切片）。 */
  batchProgress(pageSize: number): number {
    if (this.items.length === 0) return 100;
    const size = clamp(Math.round(pageSize), 1, Math.max(1, this.items.length));
    const pages = Math.ceil(this.items.length / size);
    return Math.round(100 / pages);
  }

  hint(): string {
    return `已选 ${this.items.length} 项，导出为 ${this.opts.format.toUpperCase()}`;
  }

  a11yAdjust(): ExportOptions {
    this.opts.pretty = true;
    this.opts.includeMeta = true;
    return this.options;
  }

  uninstall(): boolean {
    this.items = [];
    this.opts = { ...DEFAULT_EXPORT };
    return this.items.length === 0;
  }
}
