/**
 * F096 终端命令面板 · 前端逻辑（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据（主册 G-C-26）：内置命令 12 条全可达；收藏-搜索-执行全链；
 * 面板弹出 <100ms。
 *
 * 移植声明：`kernel/varix/src/stard/termpalette.rs` 的 TS 同构移植——
 * 内置 12 枚举即唯一源、子序列+首字母双路模糊打分、收藏 frecency +
 * 定容逐出、模板变量 {host} 提取回填、危险字符原样传递（不代管），逐项对齐。
 */

/** 面板宽（px）。 */
export const PANEL_W_PX = 560;
/** 搜索框高（px）。 */
export const PANEL_SEARCH_H_PX = 56;
/** 列表最大高（px）。 */
export const PANEL_LIST_MAX_H_PX = 320;
/** 弹出预算（ms）。 */
export const OPEN_BUDGET_MS = 100;
/** 内置命令数。 */
export const BUILTIN_COUNT = 12;
/** 收藏上限。 */
export const FAVORITE_CAP = 64;

export type BuiltinId =
  | "copyMode" | "clearScreen" | "splitH" | "splitV" | "exportSession"
  | "fontUp" | "fontDown" | "searchBuffer" | "newTab" | "closeTab"
  | "closeOtherPanes" | "toggleFollow";

export type Effect = "immediate" | "run";

export interface BuiltinDef { id: BuiltinId; name: string; initials: string; effect: Effect }

/** 内置命令 12 条（判据锚——本表即唯一源，与 Rust Builtin::all() 同序同语义）。 */
export const BUILTINS: readonly BuiltinDef[] = [
  { id: "copyMode", name: "复制模式", initials: "fzms", effect: "immediate" },
  { id: "clearScreen", name: "清屏", initials: "qp", effect: "immediate" },
  { id: "splitH", name: "左右分屏", initials: "zyfp", effect: "immediate" },
  { id: "splitV", name: "上下分屏", initials: "sxfp", effect: "immediate" },
  { id: "exportSession", name: "导出会话", initials: "dchh", effect: "run" },
  { id: "fontUp", name: "字号增大", initials: "zhzd", effect: "immediate" },
  { id: "fontDown", name: "字号减小", initials: "zhzx", effect: "immediate" },
  { id: "searchBuffer", name: "搜索回看缓冲", initials: "sshk", effect: "immediate" },
  { id: "newTab", name: "新建标签页", initials: "xjbq", effect: "run" },
  { id: "closeTab", name: "关闭标签页", initials: "gbbq", effect: "immediate" },
  { id: "closeOtherPanes", name: "关闭其他分屏", initials: "gbqtfp", effect: "immediate" },
  { id: "toggleFollow", name: "跟随滚动开关", initials: "gsgd", effect: "immediate" },
];

export function builtinCount(): number {
  return BUILTINS.length;
}

/** 命令条目（内置或收藏的自定义命令）。 */
export interface Command {
  id: number;
  name: string;
  /** 执行体：内置命令的语义 id 或原样传递的命令行（危险字符不代管）。 */
  cmdline: string;
  builtin: BuiltinId | null;
  /** 使用频次（F072 分档语义）。 */
  freq: number;
  lastUsedMs: number;
}

/** 模糊匹配得分：null = 不匹配。子序列命中 60 起、首字母命中 80 起，
 *  连续前缀 +20，频次每 10 次 +1（封顶 +10）。分数越高越前。 */
export function fuzzyScore(name: string, initials: string, query: string, freq: number): number | null {
  const q = [...query.replace(/\s+/g, "")];
  if (q.length === 0) return 0; // 空查询 = 全量列表（面板刚开）。
  const nameChars = [...name];
  // 子序列匹配（不要求连续）。
  let sub = true;
  let hi = 0;
  for (const qc of q) {
    const p = nameChars.slice(hi).indexOf(qc);
    if (p < 0) { sub = false; break; }
    hi += p + 1;
  }
  // 首字母匹配（拼音首字母串——内置命令注入口）。
  const iniChars = [...initials];
  let ini = iniChars.length >= q.length;
  if (ini) {
    for (let i = 0; i < q.length; i++) {
      if (iniChars[i] !== q[i]) { ini = false; break; }
    }
  }
  if (!sub && !ini) return null;
  let score = ini ? 80 : 60;
  if (name.startsWith(query.replace(/\s+/g, ""))) score += 20;
  score += Math.min(freq, 100) / 10;
  return score;
}

/** 匹配段高亮（子序列命中的字符下标——UI 高亮用；返回字符下标对）。 */
export function highlightIndices(name: string, query: string): Array<[number, number]> {
  const q = [...query.replace(/\s+/g, "")];
  const out: Array<[number, number]> = [];
  if (q.length === 0) return out;
  const nameChars = [...name];
  let hi = 0;
  for (let i = 0; i < nameChars.length && hi < q.length; i++) {
    if (nameChars[i] === q[hi]) {
      out.push([i, i + 1]);
      hi += 1;
    }
  }
  return out;
}

/** 收藏（frecency：频次 + 最近使用；定容 64 逐出最冷）。 */
export class Favorites {
  private items: Command[] = [];
  private nextId = 1;

  get size(): number { return this.items.length; }
  list(): Command[] { return [...this.items]; }

  /** 注册收藏（同名同命令幂等）。返回是否新增。 */
  register(name: string, cmdline: string, nowMs: number): boolean {
    const hit = this.items.find((c) => c.cmdline === cmdline && c.name === name);
    if (hit) {
      hit.freq += 1;
      hit.lastUsedMs = nowMs;
      return false;
    }
    this.items.push({ id: this.nextId++, name, cmdline, builtin: null, freq: 1, lastUsedMs: nowMs });
    this.evict();
    return true;
  }

  /** 使用计数（bump frecency）。 */
  bump(cmdline: string, nowMs: number): void {
    const hit = this.items.find((c) => c.cmdline === cmdline);
    if (hit) {
      hit.freq += 1;
      hit.lastUsedMs = nowMs;
    }
  }

  remove(cmdline: string): boolean {
    const before = this.items.length;
    this.items = this.items.filter((c) => c.cmdline !== cmdline);
    return this.items.length < before;
  }

  /** 定容逐出：超 64 → 逐出（频次最低，同频更久未用）。 */
  private evict(): void {
    while (this.items.length > FAVORITE_CAP) {
      let coldest = 0;
      for (let i = 1; i < this.items.length; i++) {
        const a = this.items[i]!;
        const c = this.items[coldest]!;
        if (a.freq < c.freq || (a.freq === c.freq && a.lastUsedMs < c.lastUsedMs)) coldest = i;
      }
      this.items.splice(coldest, 1);
    }
  }

  /** 导出（round-trip 载体）。 */
  export(): Array<[string, string, number]> {
    return this.items.map((c) => [c.name, c.cmdline, c.freq] as [string, string, number]);
  }

  /** 导入（频次合并同名同命令；返回导入条数）。 */
  import(data: Array<[string, string, number]>, nowMs: number): number {
    let n = 0;
    for (const [name, cmdline, freq] of data) {
      const hit = this.items.find((c) => c.cmdline === cmdline && c.name === name);
      if (hit) {
        hit.freq += freq;
        hit.lastUsedMs = nowMs;
      } else {
        this.items.push({ id: this.nextId++, name, cmdline, builtin: null, freq, lastUsedMs: nowMs });
        n += 1;
      }
    }
    this.evict();
    return n;
  }
}

export type PanelState = { kind: "closed" } | { kind: "open"; openedAtMs: number; openCostMs: number };

export interface Hit { command: Command; score: number }

/** 面板（打开/关闭/查询/执行 + 弹出预算账）。 */
export class Palette {
  state: PanelState = { kind: "closed" };
  query = "";
  favorites = new Favorites();
  /** 弹出超预算计数（<100ms 判线的账）。 */
  overBudget = 0;
  openCount = 0;

  open(openCostMs: number): void {
    this.state = { kind: "open", openedAtMs: openCostMs, openCostMs };
    this.openCount += 1;
    if (openCostMs > OPEN_BUDGET_MS) this.overBudget += 1; // 诚实记账，不静默
    this.query = "";
  }

  close(): void {
    this.state = { kind: "closed" };
  }

  get isOpen(): boolean {
    return this.state.kind === "open";
  }

  /** 搜索：内置 12 + 收藏库合并打分排序（分数同 → 内置优先）。 */
  search(q: string): Hit[] {
    this.query = q;
    const hits: Hit[] = [];
    for (const b of BUILTINS) {
      const s = fuzzyScore(b.name, b.initials, q, 0);
      if (s !== null) {
        hits.push({ command: { id: -1, name: b.name, cmdline: b.id, builtin: b.id, freq: 0, lastUsedMs: 0 }, score: s + 1 }); // 内置优先微加
      }
    }
    for (const c of this.favorites.list()) {
      const s = fuzzyScore(c.name, "", q, c.freq);
      if (s !== null) hits.push({ command: c, score: s });
    }
    return hits.sort((a, b) => b.score - a.score || a.command.name.localeCompare(b.command.name, "zh"));
  }

  /** 执行：返回语义（内置 id 或命令行）；收藏命令自动 bump。 */
  execute(hit: Hit, nowMs: number): { cmdline: string; effect: Effect } {
    const c = hit.command;
    if (c.builtin) {
      const def = BUILTINS.find((b) => b.id === c.builtin)!;
      return { cmdline: c.cmdline, effect: def.effect };
    }
    this.favorites.bump(c.cmdline, nowMs);
    return { cmdline: c.cmdline, effect: "run" };
  }

  /** 内置 12 全可达自检（判据「内置命令 12 条全可达」——空查询全量列出）。 */
  allBuiltinsReachable(): boolean {
    const hits = this.search("");
    const ids = new Set(hits.filter((h) => h.command.builtin).map((h) => h.command.builtin));
    return ids.size === BUILTIN_COUNT;
  }
}

/** 模板变量提取（{host} 系——白名单族）。 */
export const TEMPLATE_VARS = ["host", "user", "dir", "file"] as const;

export function templateVars(cmdline: string): string[] {
  const out: string[] = [];
  const re = /\{([a-z]+)\}/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(cmdline)) !== null) {
    const name = m[1]!;
    if ((TEMPLATE_VARS as readonly string[]).includes(name) && !out.includes(name)) out.push(name);
  }
  return out;
}

/** 模板回填（变量表逐个替换——缺失变量诚实保留占位符）。 */
export function fillTemplate(cmdline: string, vars: Array<[string, string]>): string {
  let out = cmdline;
  for (const [k, v] of vars) out = out.split(`{${k}}`).join(v);
  return out;
}
