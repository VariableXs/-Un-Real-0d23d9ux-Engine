// UNREAL-X：AI-12 族0117「桌面无障碍 2.0」（X02901~X02925）。
// 桌面对比度实测、触控目标、焦点序、读屏标签与 HC 主题红线的本地判定。

export type A11yLevel = 'A' | 'AA' | 'AAA' | 'fail';

/** sRGB 相对亮度（0~1）。 */
export function luminance(rgb: [number, number, number]): number {
  const f = (c: number): number => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * f(rgb[0]) + 0.7152 * f(rgb[1]) + 0.0722 * f(rgb[2]);
}

/** WCAG 对比度（1~21）。 */
export function contrast(a: [number, number, number], b: [number, number, number]): number {
  const l1 = luminance(a);
  const l2 = luminance(b);
  const hi = Math.max(l1, l2);
  const lo = Math.min(l1, l2);
  return (hi + 0.05) / (lo + 0.05);
}

/** 等级判定：正文 AA=4.5、AAA=7，大字 AA=3。 */
export function wcagLevel(ratio: number, largeText = false): A11yLevel {
  if (largeText) return ratio >= 4.5 ? 'AAA' : ratio >= 3 ? 'AA' : ratio >= 2 ? 'A' : 'fail';
  return ratio >= 7 ? 'AAA' : ratio >= 4.5 ? 'AA' : ratio >= 3 ? 'A' : 'fail';
}

/** 触控目标最小 44px。 */
export const MIN_TARGET = 44;
export function targetOk(w: number, h: number): boolean {
  return w >= MIN_TARGET && h >= MIN_TARGET;
}

/** 焦点环：至少 2px 且与背景对比 ≥3。 */
export const MIN_FOCUS_RING = 2;
export function focusRingOk(px: number, ratio: number): boolean {
  return px >= MIN_FOCUS_RING && ratio >= 3;
}

export interface A11yNode {
  id: string;
  label: string;
  role: string;
  tabIndex: number;
  w: number;
  h: number;
  fg: [number, number, number];
  bg: [number, number, number];
}

export interface A11yOptions {
  hcTheme: boolean;
  reduceMotion: boolean;
  largeText: boolean;
  screenReaderHints: boolean;
  keyboardOnly: boolean;
}

export const DEFAULT_A11Y: A11yOptions = {
  hcTheme: false,
  reduceMotion: false,
  largeText: false,
  screenReaderHints: true,
  keyboardOnly: false,
};

export class DesktopA11y {
  private opts: A11yOptions = { ...DEFAULT_A11Y };
  private nodes: A11yNode[] = [];

  constructor(patch?: Partial<A11yOptions>) {
    if (patch) this.configure(patch);
  }

  get options(): A11yOptions {
    return { ...this.opts };
  }

  configure(patch: Partial<A11yOptions>): boolean {
    let ok = true;
    if (typeof patch.hcTheme === 'boolean') this.opts.hcTheme = patch.hcTheme;
    if (typeof patch.reduceMotion === 'boolean') this.opts.reduceMotion = patch.reduceMotion;
    if (typeof patch.largeText === 'boolean') this.opts.largeText = patch.largeText;
    if (typeof patch.screenReaderHints === 'boolean') this.opts.screenReaderHints = patch.screenReaderHints;
    if (typeof patch.keyboardOnly === 'boolean') this.opts.keyboardOnly = patch.keyboardOnly;
    return ok;
  }

  /** 登记去重：同 id 不重复入册。 */
  add(node: A11yNode): boolean {
    if (this.nodes.some((n) => n.id === node.id)) return false;
    this.nodes.push(node);
    return true;
  }

  remove(id: string): boolean {
    const before = this.nodes.length;
    this.nodes = this.nodes.filter((n) => n.id !== id);
    return this.nodes.length !== before;
  }

  get count(): number {
    return this.nodes.length;
  }

  /** 对比度不达标清单（HC 主题下按 7:1 红线）。 */
  contrastFailures(): string[] {
    const floor = this.opts.hcTheme ? 7 : this.opts.largeText ? 3 : 4.5;
    return this.nodes.filter((n) => contrast(n.fg, n.bg) < floor).map((n) => n.id);
  }

  /** 触控目标过小清单。 */
  targetFailures(): string[] {
    return this.nodes.filter((n) => !targetOk(n.w, n.h)).map((n) => n.id);
  }

  /** 缺读屏标签清单。 */
  labelFailures(): string[] {
    if (!this.opts.screenReaderHints) return [];
    return this.nodes.filter((n) => n.label.trim().length === 0 || n.role.trim().length === 0).map((n) => n.id);
  }

  /** 焦点序是否合理：tabIndex 非负且按注册顺序单调不减。 */
  focusOrderOk(): boolean {
    let prev = -1;
    for (const n of this.nodes) {
      if (n.tabIndex < 0) return false;
      if (n.tabIndex < prev) return false;
      prev = n.tabIndex;
    }
    return true;
  }

  /** 无障碍健康分 0~100。 */
  score(): number {
    if (this.nodes.length === 0) return 100;
    const bad = new Set([...this.contrastFailures(), ...this.targetFailures(), ...this.labelFailures()]);
    return Math.round(((this.nodes.length - bad.size) * 100) / this.nodes.length);
  }

  /** 动效降级：reduce-motion / HC 下时长归零。 */
  motionDuration(ms: number): number {
    return this.opts.reduceMotion || this.opts.hcTheme ? 0 : ms;
  }

  hint(): string {
    const bad = this.contrastFailures().length + this.targetFailures().length + this.labelFailures().length;
    return bad === 0 ? '无障碍检查全部通过' : `还有 ${bad} 项待修正`;
  }

  uninstall(): boolean {
    this.nodes = [];
    this.opts = { ...DEFAULT_A11Y };
    return this.nodes.length === 0;
  }
}
