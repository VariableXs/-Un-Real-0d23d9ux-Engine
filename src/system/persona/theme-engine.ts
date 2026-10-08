/**
 * F151/F153 主题引擎深化 · 过渡编排 + 令牌 diff + 对比度自修 + 硬编码扫描器。
 *
 * 主册判据延伸：
 * - F153 设计细节「淡入实现=双层壁纸交叉（任务栏/窗口令牌同步替换在动画
 *   中段原子完成）」——本引擎实现双缓冲过渡编排：前半段旧表层淡出、
 *   中段一次性原子替换全部令牌、后半段新表层淡入，全程无白屏帧。
 * - F151 设计细节「导出含未覆盖令牌清单」+ B-1104「30 界面零硬编码」——
 *   硬编码扫描器对源码文本做语义色字面量审计，输出违例清单（供回归门禁）。
 * - 三铁律「随时可退」：diff 引擎支撑「逐令牌回滚」与差异预览（F161 联动）。
 */

import { defaultTokenTable, type TokenTable } from "./tokens";
import { contrastRatio, relativeLuminance } from "../theme-studio/tokens";
import { SWITCH_FADE_MS } from "./autodark";

// ---------- 过渡编排（双缓冲交叉淡入） ----------

export type TransitionPhase = "idle" | "fade-out" | "fade-in" | "done";

export interface TransitionFrame {
  phase: TransitionPhase;
  /** 0..1 总进度。 */
  progress: number;
  /** 旧表不透明度（fade-out 段 1→0）。 */
  oldOpacity: number;
  /** 新表不透明度（fade-in 段 0→1）。 */
  newOpacity: number;
  /** 本帧是否应执行原子替换（只在中段边界为 true 一次）。 */
  doSwap: boolean;
}

export const TRANSITION_TOTAL_MS = SWITCH_FADE_MS; // 300ms 交叉淡入
/** 中段原子替换点：50%（前后各半，双层交叉对称）。 */
export const SWAP_POINT = 0.5;

/**
 * 过渡帧计算（纯函数——渲染循环每帧调用）：
 * - 双层交叉全程进行：旧表层 1→0、新表层 0→1（线性——双层亮度守恒，无白屏帧）；
 * - 令牌（任务栏/窗口）在中段边界（50%）原子替换：帧可能跳过精确中点，
 *   用「首次越过」语义由 TransitionDriver 保证恰好替换一次。
 */
export function transitionFrame(elapsedMs: number): TransitionFrame {
  const t = Math.min(1, Math.max(0, elapsedMs / TRANSITION_TOTAL_MS));
  const crossedSwap = t >= SWAP_POINT;
  const phase: TransitionPhase = t <= 0 ? "fade-out" : t >= 1 ? "done" : crossedSwap ? "fade-in" : "fade-out";
  return {
    phase,
    progress: t,
    oldOpacity: 1 - t,
    newOpacity: t,
    doSwap: crossedSwap,
  };
}

/**
 * 过渡驱动器：消费帧序列，保证 doSwap 恰好触发一次（跨零点无重复触发——
 * 与 F153 状态机去抖同源纪律）。
 */
export class TransitionDriver {
  private swapped = false;
  private started = false;

  start(): void {
    this.started = true;
    this.swapped = false;
  }

  get active(): boolean {
    return this.started && !this.isDone;
  }

  get isDone(): boolean {
    return this.swapped && this.lastPhase === "done";
  }

  private lastPhase: TransitionPhase = "idle";

  /** 每帧喂入 elapsed；返回帧信息。doSwap 全生命周期恰好一次。 */
  tick(elapsedMs: number): TransitionFrame {
    const f = transitionFrame(elapsedMs);
    this.lastPhase = f.phase;
    if (f.doSwap && !this.swapped) {
      this.swapped = true;
      return f;
    }
    return { ...f, doSwap: false };
  }
}

// ---------- 令牌 diff（逐令牌回滚与差异预览的数学面） ----------

export type TokenDiffKind = "color" | "radius" | "font" | "motion" | "version";

export interface TokenDiffEntry {
  kind: TokenDiffKind;
  key: string;
  from: unknown;
  to: unknown;
}

/** 两张令牌表的逐令牌差异（F161 差异预览与 F152 放弃回滚共用）。 */
export function diffTokenTables(a: TokenTable, b: TokenTable): TokenDiffEntry[] {
  const out: TokenDiffEntry[] = [];
  const keys = new Set([...Object.keys(a.colors), ...Object.keys(b.colors)]);
  for (const k of keys) {
    if (a.colors[k] !== b.colors[k]) {
      out.push({ kind: "color", key: k, from: a.colors[k] ?? null, to: b.colors[k] ?? null });
    }
  }
  for (const slot of ["control", "card", "window"] as const) {
    if (a.radius[slot] !== b.radius[slot]) {
      out.push({ kind: "radius", key: `radius.${slot}`, from: a.radius[slot], to: b.radius[slot] });
    }
  }
  for (const slot of ["caption", "body", "title", "display"] as const) {
    if (a.font[slot] !== b.font[slot]) {
      out.push({ kind: "font", key: `font.${slot}`, from: a.font[slot], to: b.font[slot] });
    }
  }
  for (const curve of Object.keys(a.motion) as (keyof TokenTable["motion"])[]) {
    const ma = a.motion[curve];
    const mb = b.motion[curve];
    if (ma && mb && (ma.curve !== mb.curve || ma.duration !== mb.duration)) {
      out.push({ kind: "motion", key: `motion.${curve}`, from: { ...ma }, to: { ...mb } });
    }
  }
  return out;
}

/** 差异计数摘要（「将更改：主题/…/12 处」形态）。 */
export function diffSummary(entries: TokenDiffEntry[]): string {
  if (entries.length === 0) return "无差异";
  return `${entries.length} 处差异`;
}

// ---------- 对比度自修（AA 达标建议） ----------

export interface ContrastFix {
  tokenKey: string;
  current: string;
  /** 达标建议色（保持色相，向目标方向调整亮度）。 */
  suggestion: string;
  ratioBefore: number;
  ratioAfter: number;
}

const AA_RATIO = 4.5;

/**
 * 文本色对画布底的 AA 自修建议：保持色相，二分调亮度直到达标
 * （不改变主题作者的颜色意图——只调明度轴）。
 */
export function suggestContrastFix(fgHex: string, bgHex: string): ContrastFix | null {
  const before = contrastRatio(fgHex, bgHex);
  if (before >= AA_RATIO) return null;
  const bgLum = relativeLuminance(bgHex);
  // 向远离底色亮度的方向调（底亮则压暗前景，底暗则提亮前景）。
  // 二分收敛到「达标的最小调整量」——不改变主题作者的颜色意图，只调明度轴。
  const darken = bgLum > 0.5;
  let lo = 0;
  let hi = 1;
  let best: string | null = null;
  for (let i = 0; i < 24; i++) {
    const mid = (lo + hi) / 2;
    const candidate = shiftLuminance(fgHex, darken ? -mid : mid);
    if (contrastRatio(candidate, bgHex) >= AA_RATIO) {
      best = candidate;
      hi = mid; // 达标——尝试更小的调整量。
    } else {
      lo = mid; // 不达标——加大调整量。
    }
  }
  if (!best) return null;
  return {
    tokenKey: "",
    current: fgHex,
    suggestion: best,
    ratioBefore: before,
    ratioAfter: contrastRatio(best, bgHex),
  };
}

/** 明度轴平移（保持 RGB 色相比例，缩放亮度分量）。 */
export function shiftLuminance(hex: string, delta: number): string {
  const rgb = hexToRgbTuple(hex);
  if (!rgb) return hex;
  const factor = Math.max(0, Math.min(2.5, 1 + delta));
  const ch = (v: number) => Math.round(Math.min(255, Math.max(0, v * factor)));
  const [r, g, b] = rgb;
  return rgbToHex(ch(r), ch(g), ch(b));
}

function hexToRgbTuple(hex: string): [number, number, number] | null {
  const m = /^#([0-9a-fA-F]{6})/.exec(hex);
  if (!m) return null;
  const n = parseInt(m[1] ?? "000000", 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

function rgbToHex(r: number, g: number, b: number): string {
  const h = (v: number) => v.toString(16).padStart(2, "0");
  return `#${h(r)}${h(g)}${h(b)}`;
}

/** 全表 AA 审计：文本/状态色对画布逐枚检查，返回不达标项的自修建议。 */
export function auditTableContrast(table: TokenTable): ContrastFix[] {
  const bg = table.colors["--p-bg-canvas"] ?? "#14141c";
  const textKeys = ["--p-fg-primary", "--p-fg-secondary", "--p-accent", "--p-success", "--p-warn", "--p-danger", "--p-on-accent"];
  const fixes: ContrastFix[] = [];
  for (const key of textKeys) {
    const fg = table.colors[key];
    if (!fg) continue;
    const fix = suggestContrastFix(fg, bg);
    if (fix) fixes.push({ ...fix, tokenKey: key });
  }
  return fixes;
}

// ---------- 硬编码扫描器（B-1104 联动 · 30 界面零硬编码的执法工具） ----------

export interface HardcodeHit {
  file: string;
  line: number;
  snippet: string;
  /** 命中的字面量。 */
  literal: string;
  /** 建议替换的令牌键。 */
  suggestion: string;
}

/** 语义色 → 典型字面量对照（扫描器知识库：出生主题的物理值）。 */
const DEFAULT_PHYSICALS: readonly { token: string; examples: string[] }[] = [
  { token: "--p-bg-canvas", examples: ["#14141c", "#14141C"] },
  { token: "--p-bg-surface", examples: ["#1c1c26"] },
  { token: "--p-bg-raised", examples: ["#22222e"] },
  { token: "--p-fg-primary", examples: ["#e8e8f0"] },
  { token: "--p-fg-secondary", examples: ["#a0a0b4"] },
  { token: "--p-fg-disabled", examples: ["#5a5a6c"] },
  { token: "--p-accent", examples: ["#6e7fd4"] },
  { token: "--p-success", examples: ["#5fbf8a"] },
  { token: "--p-warn", examples: ["#d4b45f"] },
  { token: "--p-danger", examples: ["#d4685f"] },
  { token: "--p-focus-ring", examples: ["#8ea0ff"] },
];

/**
 * 扫描一段源码文本中的硬编码语义色字面量。
 * 规则：与令牌默认物理值完全相等的 hex 字面量即违例（近似值不误报——
 * 门禁要的是确定性，不是猜测）。
 */
export function scanHardcodedColors(file: string, source: string): HardcodeHit[] {
  const hits: HardcodeHit[] = [];
  const lines = source.split("\n");
  lines.forEach((line, idx) => {
    for (const def of DEFAULT_PHYSICALS) {
      for (const lit of def.examples) {
        let from = 0;
        for (;;) {
          const at = line.indexOf(lit, from);
          if (at < 0) break;
          hits.push({
            file,
            line: idx + 1,
            snippet: line.trim().slice(0, 120),
            literal: lit,
            suggestion: `var(${def.token})`,
          });
          from = at + lit.length;
        }
      }
    }
  });
  return hits;
}

/** 批量扫描（多文件）并汇总（30 界面抽查的机械口径）。 */
export function scanProjectHardcodes(files: Record<string, string>): { hits: HardcodeHit[]; filesClean: number; filesTotal: number } {
  const hits: HardcodeHit[] = [];
  let clean = 0;
  const names = Object.keys(files);
  for (const name of names) {
    const h = scanHardcodedColors(name, files[name] ?? "");
    if (h.length === 0) clean++;
    hits.push(...h);
  }
  return { hits, filesClean: clean, filesTotal: names.length };
}

// ---------- 主题包集成（vtheme 导入 → 令牌表派生） ----------

/**
 * .vtheme 变体 → persona 令牌表派生（theme-studio 主题库与本域令牌表的桥）：
 * vtheme 的 11 变量映射进 24 色语义集（其余按默认兜底——覆盖度如实标注）。
 */
export function deriveTableFromVthemeColors(vthemeColors: Record<string, string>, base?: TokenTable): { table: TokenTable; unmapped: string[] } {
  const d = base ?? defaultTokenTable();
  const table: TokenTable = JSON.parse(JSON.stringify(d));
  const bridge: Record<string, string> = {
    "--bg-canvas": "--p-bg-canvas",
    "--bg-surface": "--p-bg-surface",
    "--bg-raised": "--p-bg-raised",
    "--text-primary": "--p-fg-primary",
    "--text-secondary": "--p-fg-secondary",
    "--accent": "--p-accent",
    "--accent-soft": "--p-accent-soft",
    "--success": "--p-success",
    "--warn": "--p-warn",
    "--danger": "--p-danger",
    "--stroke": "--p-border-regular",
  };
  const unmapped: string[] = [];
  for (const [k, v] of Object.entries(vthemeColors)) {
    const target = bridge[k];
    if (target && /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(v)) {
      table.colors[target] = v;
    } else {
      unmapped.push(k);
    }
  }
  return { table, unmapped };
}

/** 令牌表 → .vtheme 变体颜色导出（反向桥——主题工坊可消费 persona 主题）。 */
export function exportVthemeColors(table: TokenTable): Record<string, string> {
  const reverse: Record<string, string> = {
    "--p-bg-canvas": "--bg-canvas",
    "--p-bg-surface": "--bg-surface",
    "--p-bg-raised": "--bg-raised",
    "--p-fg-primary": "--text-primary",
    "--p-fg-secondary": "--text-secondary",
    "--p-accent": "--accent",
    "--p-accent-soft": "--accent-soft",
    "--p-success": "--success",
    "--p-warn": "--warn",
    "--p-danger": "--danger",
    "--p-border-regular": "--stroke",
  };
  const out: Record<string, string> = {};
  for (const [pk, vk] of Object.entries(reverse)) {
    const v = table.colors[pk];
    if (v) out[vk] = v;
  }
  return out;
}

/** diff 的哈希口径（放弃=零残留的细粒度证据）。 */
export function diffHash(a: TokenTable, b: TokenTable): number {
  return diffTokenTables(a, b).length;
}
