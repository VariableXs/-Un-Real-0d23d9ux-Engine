// UNREAL-X：AI-12 族0116「图标包互操作」（X02876~X02900）。
// 三方图标包（Varix / Freedesktop index.theme / SVG sprite / ICO 目录 / ICNS 目录）
// 的导入映射、尺寸阶梯补全、命名归一与冲突消解。

export type PackFormat = 'varix' | 'freedesktop' | 'svg-sprite' | 'ico-dir' | 'icns-dir';
export const PACK_FORMATS: PackFormat[] = ['varix', 'freedesktop', 'svg-sprite', 'ico-dir', 'icns-dir'];

/** 尺寸阶梯：从 16 到 256 的七级。 */
export const SIZE_LADDER = [16, 24, 32, 48, 64, 128, 256] as const;

export interface IconEntry {
  /** 归一化后的逻辑名（小写、连字符分隔）。 */
  name: string;
  /** 原始名（导入源里的名字）。 */
  raw: string;
  sizes: number[];
  format: PackFormat;
  license?: string;
}

export interface InteropOptions {
  target: PackFormat;
  /** 缺失尺寸用最近邻放大补全。 */
  upscaleFallback: boolean;
  /** 同名冲突时保留来源优先级更高的一方。 */
  preferSource: boolean;
  normalizeNames: boolean;
  requireLicense: boolean;
}

export const DEFAULT_INTEROP: InteropOptions = {
  target: 'varix',
  upscaleFallback: true,
  preferSource: true,
  normalizeNames: true,
  requireLicense: false,
};

const clamp = (v: number, lo: number, hi: number): number => (v < lo ? lo : v > hi ? hi : v);

/** 命名归一：小写、非字母数字转连字符、合并重复连字符、去首尾连字符。 */
export function normalizeName(raw: string): string {
  return raw
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/-{2,}/g, '-')
    .replace(/^-|-$/g, '');
}

/** 尺寸阶梯就近匹配（不全则取最近的上级）。 */
export function nearestSize(want: number): number {
  let best: number = SIZE_LADDER[0]!;
  let bestD = Number.POSITIVE_INFINITY;
  for (const s of SIZE_LADDER) {
    const d = Math.abs(s - want);
    if (d < bestD) {
      bestD = d;
      best = s;
    }
  }
  return best;
}

/** 尺寸阶梯补全覆盖率 0~100。 */
export function ladderCoverage(sizes: number[]): number {
  const have = SIZE_LADDER.filter((s) => sizes.includes(s)).length;
  return Math.round((have * 100) / SIZE_LADDER.length);
}

/** Freedesktop index.theme 解析（极简子集：Name / Size / Context）。 */
export function parseIndexTheme(text: string): { name: string; sizes: number[] } {
  const name = /^\s*Name\s*=\s*(.+)$/m.exec(text)?.[1]?.trim() ?? 'unnamed';
  const sizes: number[] = [];
  for (const m of text.matchAll(/^\s*Size\s*=\s*(\d+)\s*$/gm)) {
    const v = Number(m[1]);
    if (!sizes.includes(v)) sizes.push(v);
  }
  return { name, sizes };
}

/** SVG sprite 里 <symbol id="..."> 的 id 提取。 */
export function parseSpriteIds(svg: string): string[] {
  const ids: string[] = [];
  for (const m of svg.matchAll(/<symbol[^>]*\bid\s*=\s*"([^"]+)"/g)) {
    const id = m[1]!;
    if (!ids.includes(id)) ids.push(id);
  }
  return ids;
}

export class PackInterop {
  private opts: InteropOptions = { ...DEFAULT_INTEROP };
  private entries: IconEntry[] = [];

  constructor(patch?: Partial<InteropOptions>) {
    if (patch) this.configure(patch);
  }

  get options(): InteropOptions {
    return { ...this.opts };
  }

  configure(patch: Partial<InteropOptions>): boolean {
    let ok = true;
    if (patch.target !== undefined) {
      if (PACK_FORMATS.includes(patch.target)) this.opts.target = patch.target;
      else ok = false;
    }
    if (patch.upscaleFallback !== undefined) this.opts.upscaleFallback = !!patch.upscaleFallback;
    if (patch.preferSource !== undefined) this.opts.preferSource = !!patch.preferSource;
    if (patch.normalizeNames !== undefined) this.opts.normalizeNames = !!patch.normalizeNames;
    if (patch.requireLicense !== undefined) this.opts.requireLicense = !!patch.requireLicense;
    return ok;
  }

  /** 登记去重：归一化后同名不重复入册。 */
  add(raw: string, sizes: number[], format: PackFormat, license?: string): boolean {
    const name = this.opts.normalizeNames ? normalizeName(raw) : raw;
    if (!name) return false;
    if (this.opts.requireLicense && !license) return false;
    const idx = this.entries.findIndex((e) => e.name === name);
    if (idx >= 0) {
      if (!this.opts.preferSource) return false;
      const merged = Array.from(new Set([...this.entries[idx]!.sizes, ...sizes])).sort((a, b) => a - b);
      this.entries[idx] = { ...this.entries[idx]!, sizes: merged, format, license: license ?? this.entries[idx]!.license };
      return false;
    }
    this.entries.push({ name, raw, sizes: [...sizes].sort((a, b) => a - b), format, license });
    return true;
  }

  remove(name: string): boolean {
    const before = this.entries.length;
    this.entries = this.entries.filter((e) => e.name !== name);
    return this.entries.length !== before;
  }

  get count(): number {
    return this.entries.length;
  }

  list(): IconEntry[] {
    return this.entries.map((e) => ({ ...e, sizes: [...e.sizes] }));
  }

  /** 缺失尺寸（按目标阶梯）。 */
  missingSizes(name: string): number[] {
    const e = this.entries.find((x) => x.name === name);
    if (!e) return [...SIZE_LADDER];
    return SIZE_LADDER.filter((s) => !e.sizes.includes(s));
  }

  /** 冲突检测：归一化后撞名的原始名清单。 */
  conflicts(): string[] {
    const seen = new Map<string, string[]>();
    for (const e of this.entries) {
      const list = seen.get(e.name) ?? [];
      list.push(e.raw);
      seen.set(e.name, list);
    }
    return Array.from(seen.entries())
      .filter(([, v]) => v.length > 1)
      .map(([k]) => k);
  }

  /** 导出为 freedesktop index.theme 文本。 */
  toIndexTheme(packName = 'varix-pack'): string {
    const head = `[Icon Theme]\nName=${packName}\nComment=Converted by Variable desktop\nDirectories=`;
    const dirs = SIZE_LADDER.map((s) => `${s}/apps`).join(',');
    const body = SIZE_LADDER.map((s) => `\n\n[${s}/apps]\nSize=${s}\nContext=Applications\nType=Threshold`).join('');
    return `${head}${dirs}\n${body}\n`;
  }

  /** 导出为 SVG sprite 骨架。 */
  toSprite(): string {
    const syms = this.entries
      .map((e) => `<symbol id="${e.name}" viewBox="0 0 24 24"><title>${e.name}</title></symbol>`)
      .join('');
    return `<svg xmlns="http://www.w3.org/2000/svg" style="display:none">${syms}</svg>`;
  }

  /** 互操作健康分：平均阶梯覆盖率（0~100）。 */
  health(): number {
    if (this.entries.length === 0) return 0;
    const total = this.entries.reduce((a, e) => a + ladderCoverage(e.sizes), 0);
    return Math.round(total / this.entries.length);
  }

  applyLevel(level: number): InteropOptions {
    const idx = clamp(Math.round(level), 0, 4);
    this.opts.upscaleFallback = idx >= 1;
    this.opts.requireLicense = idx >= 3;
    return this.options;
  }

  hint(): string {
    return `已导入 ${this.entries.length} 个图标，平均覆盖 ${this.health()}%`;
  }

  uninstall(): boolean {
    this.entries = [];
    this.opts = { ...DEFAULT_INTEROP };
    return this.entries.length === 0;
  }
}
