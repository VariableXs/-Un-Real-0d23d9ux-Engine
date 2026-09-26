/**
 * F095 终端应用 2.0 · 前端逻辑（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据（主册 G-C-25）：10 万行 cat 大文件回看滚动 80fps；CJK 混排对齐
 * 抽查（中英混排列不错位）；分屏拖拽实时重排 80fps。
 *
 * 移植声明：`kernel/varix/src/stard/term2.rs` 的 TS 同构移植——CJK 宽度
 * 表（组合字符零宽）、环形回看双上限 + 截断提示行钉头（D12 同源修法）、
 * 虚拟滚动切片 + 渲染帧账、分屏二叉树（拖拽重排面积守恒），逐项对齐。
 */

/** 回看缓冲行数上限。 */
export const SCROLLBACK_LINES = 100_000;
/** 回看缓冲内存上限（字节，~80MB 自适应行宽）。 */
export const SCROLLBACK_BYTES = 80 * 1024 * 1024;
/** 分屏分隔条拖拽热区（px）。 */
export const DIVIDER_HOTZONE_PX = 6;
/** 字号档（Ctrl+滚轮 8 档）。 */
export const FONT_SIZE_STEPS: readonly number[] = [10, 12, 14, 16, 18, 20, 24, 28];
/** 滚动条触区宽（px）。 */
export const SCROLLBAR_TOUCH_PX = 60;
/** 80fps 帧预算（μs）——P95 ≤12.5ms 判线的帧账载体。 */
export const FRAME_BUDGET_US = 12_500;
/** 单格渲染成本（μs，K1 口径：2000 可视格恰在 12.5ms 预算内）。 */
export const COST_PER_CELL_US = 6;
/** 头部截断提示行文本（静默丢字是红线）。 */
export const TRUNCATION_NOTICE = "…（回看缓冲已满，最早输出已丢弃）";

// ---------------------------------------------------------------------------
// Unicode 宽度（CJK 全宽 / 组合字符零宽——混排列不错位的算法本体）
// ---------------------------------------------------------------------------

/** 单字符显示列宽：2 = CJK 全宽，0 = 组合字符（零宽），1 = 其余。 */
export function charWidth(c: string): number {
  const cp = c.codePointAt(0) ?? 0;
  if (
    (cp >= 0x0300 && cp <= 0x036f) || (cp >= 0x0483 && cp <= 0x0489) ||
    (cp >= 0x0591 && cp <= 0x05bd) || (cp >= 0x0610 && cp <= 0x061a) ||
    (cp >= 0x064b && cp <= 0x065f) || (cp >= 0x0e31 && cp <= 0x0e3a) ||
    (cp >= 0x200b && cp <= 0x200f) || (cp >= 0xfe00 && cp <= 0xfe0f) ||
    (cp >= 0xfe20 && cp <= 0xfe2f)
  ) return 0;
  if (
    (cp >= 0x1100 && cp <= 0x115f) || (cp >= 0x2e80 && cp <= 0x303e) ||
    (cp >= 0x3041 && cp <= 0x33ff) || (cp >= 0x3400 && cp <= 0x4dbf) ||
    (cp >= 0x4e00 && cp <= 0x9fff) || (cp >= 0xa000 && cp <= 0xa4cf) ||
    (cp >= 0xac00 && cp <= 0xd7a3) || (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0xfe30 && cp <= 0xfe4f) || (cp >= 0xff00 && cp <= 0xff60) ||
    (cp >= 0xffe0 && cp <= 0xffe6) || (cp >= 0x1f300 && cp <= 0x1faff) ||
    (cp >= 0x20000 && cp <= 0x3fffd)
  ) return 2;
  return 1;
}

/** 行显示列宽（混排对齐的行宽度量——唯一口径）。 */
export function lineWidth(text: string): number {
  let w = 0;
  for (const ch of text) w += charWidth(ch);
  return w;
}

// ---------------------------------------------------------------------------
// 回看缓冲（环形 + 字节账 + 截断提示行钉头）
// ---------------------------------------------------------------------------

export interface TermLine { text: string; stampMs: number }

function lineBytes(text: string): number {
  return text.length + lineWidth(text) * 2 + 16;
}

/** 环形回看缓冲：行数与字节数双上限，满时丢最早并插入截断提示行。
 *  实现注：物理数组 + 头游标（head）做逻辑环形——逐出 O(超额行数)，
 *  不做 splice 搬移（狂刷输出场景的 O(n²) 是性能红线）；提示行为虚拟行，
 *  驻守逻辑头永不丢失（D12 修法）。 */
export class Scrollback {
  private lines: TermLine[] = [];
  /** 与 lines 平行的每行字节账（push 时算一次——enforce 不重算 lineWidth）。 */
  private lineWidths: number[] = [];
  private head = 0;
  private bytes = 0;
  /** 头部截断提示行文本（null = 无截断发生）。 */
  private notice: string | null = null;
  truncations = 0;

  get length(): number {
    return this.lines.length - this.head + (this.notice !== null ? 1 : 0);
  }

  get bytesUsed(): number {
    return this.bytes;
  }

  /** 追加一行输出。超出任一上限 → 连带最早行丢弃至合规（提示行去重）。 */
  push(text: string, stampMs: number): void {
    const b = lineBytes(text);
    this.lines.push({ text, stampMs });
    this.lineWidths.push(b);
    this.bytes += b;
    this.enforce();
  }

  /** 批量追加（灌入大文件用——逐行复用同一 enforce 语义）。 */
  pushMany(lines: string[], stampMs: number): void {
    for (const l of lines) this.push(l, stampMs);
  }

  private enforce(): void {
    let guard = 0;
    while (guard++ < 64) {
      const overLines = Math.max(0, this.length - SCROLLBACK_LINES);
      const overBytes = Math.max(0, this.bytes - SCROLLBACK_BYTES);
      if (overLines === 0 && overBytes === 0) return;
      // 提示行驻守逻辑头——逐出从它身后开始，提示永不丢失（D12 修法）。
      const start = this.notice !== null && this.lines.length - this.head > 1 ? this.head + 1 : this.head;
      let cut = 0;
      let cutBytes = 0;
      while (start + cut < this.lines.length) {
        cutBytes += this.lineWidths[start + cut]!;
        cut += 1;
        if (cut >= overLines && cutBytes >= overBytes) break;
      }
      if (cut === 0) break;
      this.head = start + cut;
      this.bytes -= cutBytes;
      if (this.notice === null) {
        this.notice = TRUNCATION_NOTICE;
        this.bytes += lineBytes(TRUNCATION_NOTICE);
        this.truncations += 1;
      }
    }
    // guard 触顶 = 单行体量超出缓冲预算的异常——显性化（十三·补：零静默）。
    console.error("[term2] 回看缓冲 enforce 未收敛：单行体量异常");
  }

  /** 虚拟滚动切片：可视行号区间 → 行内容（只取可视——虚拟化本体）。
   *  `fromRow` 从 0（逻辑最旧，含提示行）到 length-1；越界钳制。 */
  slice(fromRow: number, rows: number): TermLine[] {
    const total = this.length;
    const from = Math.max(0, Math.min(fromRow, total));
    const to = Math.min(from + Math.max(0, rows), total);
    const out: TermLine[] = [];
    const hasNotice = this.notice !== null;
    for (let i = from; i < to; i++) {
      if (hasNotice) {
        if (i === 0) {
          out.push({ text: this.notice!, stampMs: 0 });
          continue;
        }
        out.push(this.lines[this.head + i - 1]!);
      } else {
        out.push(this.lines[this.head + i]!);
      }
    }
    return out;
  }

  /** 底部行号（跟随模式锚）。 */
  bottomRow(): number {
    return Math.max(0, this.length - 1);
  }

  /** 导出会话文本（含时间戳与退出码元数据——F096 面共享格式）。 */
  export(exitCode: number): string {
    let out = "# VARIX session export\n";
    const all = this.slice(0, this.length);
    for (const l of all) {
      if (l.text !== "") out += `[${String(l.stampMs).padStart(12, "0")}] ${l.text}\n`;
    }
    out += `# exit=${exitCode}\n`;
    return out;
  }
}

// ---------------------------------------------------------------------------
// 渲染帧账（80fps 判线的机制面——可视格数 → 预算核算）
// ---------------------------------------------------------------------------

export interface FrameAccount { cells: number; costUs: number; withinBudget: boolean }

/** 帧账：可视格数 × 单格成本 ≤ 12.5ms 预算（80×25 满屏达标口径）。 */
export function frameAccount(cols: number, rows: number): FrameAccount {
  const cells = cols * rows;
  const costUs = cells * COST_PER_CELL_US;
  return { cells, costUs, withinBudget: costUs <= FRAME_BUDGET_US };
}

// ---------------------------------------------------------------------------
// 分屏二叉树（拖拽重排——面积守恒）
// ---------------------------------------------------------------------------

export type PaneTree =
  | { kind: "leaf"; id: number; ratio: number } // ratio 仅作叶权重（重排后重算）
  | { kind: "split"; dir: "h" | "v"; a: PaneTree; b: PaneTree; ratio: number };

let nextPaneId = 1;

export function newPane(): PaneTree {
  return { kind: "leaf", id: nextPaneId++, ratio: 1 };
}

export function resetPaneIds(): void {
  nextPaneId = 1;
}

export function leafIds(t: PaneTree): number[] {
  return t.kind === "leaf" ? [t.id] : [...leafIds(t.a), ...leafIds(t.b)];
}

export function leafCount(t: PaneTree): number {
  return t.kind === "leaf" ? 1 : leafCount(t.a) + leafCount(t.b);
}

/** 在指定叶旁分裂（dir=新分隔方向）。 */
export function splitPane(t: PaneTree, leafId: number, dir: "h" | "v"): PaneTree {
  if (t.kind === "leaf") {
    if (t.id !== leafId) return t;
    return { kind: "split", dir, a: t, b: newPane(), ratio: 0.5 };
  }
  return { ...t, a: splitPane(t.a, leafId, dir), b: splitPane(t.b, leafId, dir) };
}

/** 关闭叶（根叶不可关——返回 null 表示只剩一叶）。 */
export function closePane(t: PaneTree, leafId: number): PaneTree | null {
  if (t.kind === "leaf") return null;
  const aOnly = t.a.kind === "leaf" && t.a.id === leafId;
  const bOnly = t.b.kind === "leaf" && t.b.id === leafId;
  if (aOnly) return t.b;
  if (bOnly) return t.a;
  const a2 = closePane(t.a, leafId);
  if (a2) return { ...t, a: a2 };
  const b2 = closePane(t.b, leafId);
  if (b2) return { ...t, b: b2 };
  return null;
}

export interface PaneRect { id: number; x: number; y: number; w: number; h: number }

/** 布局计算：树 → 每叶矩形（px，面积守恒——重排不丢面积）。
 *  传入边界取整误差由最后一叶吃满（无缝隙）。 */
export function layoutTree(t: PaneTree, x: number, y: number, w: number, h: number): PaneRect[] {
  if (t.kind === "leaf") return [{ id: t.id, x, y, w, h }];
  const r = Math.max(0.1, Math.min(0.9, t.ratio));
  if (t.dir === "h") {
    const wl = Math.round(w * r);
    return [...layoutTree(t.a, x, y, wl, h), ...layoutTree(t.b, x + wl, y, w - wl, h)];
  }
  const hl = Math.round(h * r);
  return [...layoutTree(t.a, x, y, w, hl), ...layoutTree(t.b, x, y + hl, w, h - hl)];
}

/** 分隔条位置（拖拽命中检测——热区 6px）。 */
export function dividerHit(t: PaneTree, px: number, py: number, x: number, y: number, w: number, h: number): { dir: "h" | "v"; at: number } | null {
  if (t.kind === "leaf") return null;
  const r = Math.max(0.1, Math.min(0.9, t.ratio));
  if (t.dir === "h") {
    const wl = Math.round(w * r);
    const cx = x + wl;
    if (py >= y && py <= y + h && Math.abs(px - cx) <= DIVIDER_HOTZONE_PX) return { dir: "h", at: wl };
    return dividerHit(t.a, px, py, x, y, wl, h) ?? dividerHit(t.b, px, py, x + wl, y, w - wl, h);
  }
  const hl = Math.round(h * r);
  const cy = y + hl;
  if (px >= x && px <= x + w && Math.abs(py - cy) <= DIVIDER_HOTZONE_PX) return { dir: "v", at: hl };
  return dividerHit(t.a, px, py, x, y, w, hl) ?? dividerHit(t.b, px, py, x, y + hl, w, h - hl);
}

/** 拖拽分隔条 → 更新 ratio（实时重排——80fps 判据的交互面）。 */
export function dragDivider(t: PaneTree, dir: "h" | "v", deltaPx: number, span: number): PaneTree {
  const set = (node: PaneTree): PaneTree => {
    if (node.kind === "split" && node.dir === dir) {
      // 取第一个匹配的分隔（拖拽 UI 一次只拖一条）。
      return { ...node, ratio: Math.max(0.1, Math.min(0.9, node.ratio + deltaPx / span)) };
    }
    if (node.kind === "leaf") return node;
    return { ...node, a: set(node.a), b: set(node.b) };
  };
  return set(t);
}
