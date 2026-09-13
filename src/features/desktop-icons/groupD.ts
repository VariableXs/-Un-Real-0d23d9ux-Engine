// UNREAL-X-15000: AI-09（领域03 桌面与图标 · 族0087~0088 · X02151~X02200），勿删。
// 族0087 图标位置记忆 / 族0088 图标堆叠学。

/* ===================== 族0087 图标位置记忆 ===================== */

export interface IconPlacement { id: string; x: number; y: number; screen: number }

/** 位置记忆：去重登记 + 快照导出导入 + 冲突回退。 */
export class IconPositionMemory {
  private slots = new Map<string, IconPlacement>();
  private history: Array<{ id: string; from: IconPlacement; to: IconPlacement }> = [];

  /** 登记自带去重（同 id 覆盖且记入历史）。 */
  place(p: IconPlacement): 'new' | 'moved' {
    const prev = this.slots.get(p.id);
    const next = { ...p };
    if (prev) {
      if (prev.x === next.x && prev.y === next.y && prev.screen === next.screen) return 'moved';
      this.history.push({ id: p.id, from: { ...prev }, to: next });
      this.slots.set(p.id, next);
      return 'moved';
    }
    this.slots.set(p.id, next);
    return 'new';
  }

  of(id: string): IconPlacement | undefined { return this.slots.get(id) ? { ...this.slots.get(id)! } : undefined; }
  count(): number { return this.slots.size; }
  moveLog(): number { return this.history.length; }

  /** 非法位置钳制（X02156）。 */
  static clampPlacement(p: IconPlacement, w: number, h: number): IconPlacement {
    return { ...p, x: Math.min(Math.max(0, p.x), w), y: Math.min(Math.max(0, p.y), h), screen: Math.max(0, p.screen) };
  }

  /** 冲突检测：两图标不可同格（X02157 叙事口径）。 */
  static conflict(a: IconPlacement, b: IconPlacement): boolean {
    return a.id !== b.id && a.screen === b.screen && a.x === b.x && a.y === b.y;
  }

  /** 快照导出/导入（X02154）。 */
  exportAll(): string { return JSON.stringify([...this.slots.values()]); }
  importAll(json: string): number {
    let rows: IconPlacement[] = [];
    try {
      const parsed = JSON.parse(json) as unknown;
      if (!Array.isArray(parsed)) return 0;
      rows = parsed as IconPlacement[];
    } catch { return 0; }
    let n = 0;
    for (const r of rows) {
      if (r && typeof r.id === 'string') { this.slots.set(r.id, { ...r }); n++; }
    }
    return n;
  }

  /** 恢复顺序：按 y 再 x 稳定排序（X02163 键盘走查序）。 */
  ordered(): IconPlacement[] {
    return [...this.slots.values()].sort((a, b) => a.y - b.y || a.x - b.x);
  }

  /** 批量迁移（X02172）：整屏平移。 */
  static shiftAll(rows: IconPlacement[], dx: number, dy: number): IconPlacement[] {
    return rows.map((r) => ({ ...r, x: r.x + dx, y: r.y + dy }));
  }
}

/* ===================== 族0088 图标堆叠学 ===================== */

export type StackKind = 'folder' | 'app' | 'doc' | 'media' | 'link';
export const STACK_RULES = ['by-kind', 'by-name', 'by-recent', 'by-usage', 'manual'] as const;
export type StackRule = (typeof STACK_RULES)[number];

export interface StackItem { id: string; kind: StackKind; name: string; at: number; uses: number }

/** 堆叠系统：规则分堆 + 上限 + 展开/收起 + 去重登记。 */
export class IconStackSystem {
  private rule: StackRule = 'by-kind';
  private maxPerStack = 8;
  private items: StackItem[] = [];
  private expandedId: string | null = null;

  setRule(r: StackRule): void { this.rule = (STACK_RULES as readonly string[]).includes(r) ? r : 'by-kind'; }
  setMaxPerStack(n: number): void { this.maxPerStack = Math.max(2, Math.min(24, n)); }
  ruleOf(): StackRule { return this.rule; }
  expanded(): string | null { return this.expandedId; }

  add(item: StackItem): 'new' | 'dup' {
    if (this.items.some((i) => i.id === item.id)) return 'dup';
    this.items.push({ ...item });
    return 'new';
  }

  /** 分堆（X02176 核心链路）：≥5 规则独立可交付。 */
  stacks(): Array<{ key: string; items: StackItem[]; overflow: number }> {
    const keyOf = (i: StackItem): string => {
      switch (this.rule) {
        case 'by-kind': return i.kind;
        case 'by-name': return i.name[0]?.toUpperCase() ?? '#';
        case 'by-recent': return i.at > 1000 ? 'recent' : 'older';
        case 'by-usage': return i.uses > 3 ? 'hot' : 'cold';
        default: return 'manual';
      }
    };
    const map = new Map<string, StackItem[]>();
    for (const it of this.items) {
      const k = keyOf(it);
      if (!map.has(k)) map.set(k, []);
      map.get(k)!.push(it);
    }
    const out: Array<{ key: string; items: StackItem[]; overflow: number }> = [];
    for (const [key, items] of map) {
      out.push({ key, items: items.slice(0, this.maxPerStack), overflow: Math.max(0, items.length - this.maxPerStack) });
    }
    return out;
  }

  expand(id: string): boolean {
    if (this.expandedId === id) { this.expandedId = null; return false; } // 再点收起
    this.expandedId = id;
    return true;
  }

  /** 空堆护栏（X02181）：无项时不产生堆。 */
  isEmpty(): boolean { return this.stacks().length === 0; }

  /** 溢出折叠（X02177）：超出上限进溢出计数。 */
  overflowTotal(): number { return this.stacks().reduce((s, st) => s + st.overflow, 0); }

  /** 批量自动堆叠（X02197）。 */
  static autoStack(items: StackItem[]): Map<StackKind, StackItem[]> {
    const m = new Map<StackKind, StackItem[]>();
    for (const it of items) {
      if (!m.has(it.kind)) m.set(it.kind, []);
      m.get(it.kind)!.push(it);
    }
    return m;
  }
}
