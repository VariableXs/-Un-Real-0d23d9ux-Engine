// UNREAL-X：AI-12 族0120「桌面设计收官」（X02976~X03000）。
// 领域03（桌面设计·桌面与图标）收官编排：里程碑清单、去重叠加、时间线与致谢。

export type MilestoneKind = 'delivery' | 'teaching' | 'celebration' | 'handover';

export interface Milestone {
  id: string;
  title: string;
  kind: MilestoneKind;
  done: boolean;
  /** 权重（用于加权完成度）。 */
  weight: number;
}

export interface FinaleOptions {
  strict: boolean;
  includeOptional: boolean;
  freezeBaseline: boolean;
}

export const DEFAULT_FINALE: FinaleOptions = {
  strict: true,
  includeOptional: false,
  freezeBaseline: false,
};

/** 收官里程碑骨架：AI-09/AI-10/AI-11/AI-12 四组各一交付位 + 教学/庆典/交接。 */
export const FINALE_ITEMS: Milestone[] = [
  { id: 'F-09', title: '图标体系交付', kind: 'delivery', done: false, weight: 3 },
  { id: 'F-10', title: '壁纸与微件交付', kind: 'delivery', done: false, weight: 3 },
  { id: 'F-11', title: '内核桌面服务交付', kind: 'delivery', done: false, weight: 3 },
  { id: 'F-12', title: '桌面设计与分析交付', kind: 'delivery', done: false, weight: 3 },
  { id: 'T-01', title: '使用教学与示例', kind: 'teaching', done: false, weight: 1 },
  { id: 'C-01', title: '收官庆典与彩蛋', kind: 'celebration', done: false, weight: 1 },
  { id: 'H-01', title: '交接与运维文档', kind: 'handover', done: false, weight: 2 },
];

export class DesignFinale {
  private opts: FinaleOptions = { ...DEFAULT_FINALE };
  private items: Milestone[] = FINALE_ITEMS.map((m) => ({ ...m }));

  constructor(patch?: Partial<FinaleOptions>) {
    if (patch) this.configure(patch);
  }

  get options(): FinaleOptions {
    return { ...this.opts };
  }

  configure(patch: Partial<FinaleOptions>): boolean {
    let ok = true;
    if (typeof patch.strict === 'boolean') this.opts.strict = patch.strict;
    if (typeof patch.includeOptional === 'boolean') this.opts.includeOptional = patch.includeOptional;
    if (typeof patch.freezeBaseline === 'boolean') this.opts.freezeBaseline = patch.freezeBaseline;
    return ok;
  }

  /** 登记去重：同 id 不重复入册。 */
  add(item: Milestone): boolean {
    if (this.items.some((m) => m.id === item.id)) return false;
    if (!this.opts.includeOptional && item.kind === 'celebration') return false;
    this.items.push({ ...item });
    return true;
  }

  remove(id: string): boolean {
    const before = this.items.length;
    this.items = this.items.filter((m) => m.id !== id);
    return this.items.length !== before;
  }

  all(): Milestone[] {
    return this.items.map((m) => ({ ...m }));
  }

  /** 勾选里程碑（重复勾选返回 false）。 */
  reach(id: string): boolean {
    const m = this.items.find((x) => x.id === id);
    if (!m || m.done) return false;
    m.done = true;
    return true;
  }

  unmark(id: string): boolean {
    const m = this.items.find((x) => x.id === id);
    if (!m || !m.done) return false;
    m.done = false;
    return true;
  }

  doneCount(): number {
    return this.items.filter((m) => m.done).length;
  }

  /** 加权完成度 0~100。 */
  progress(): number {
    const total = this.items.reduce((a, m) => a + m.weight, 0);
    if (total === 0) return 0;
    const done = this.items.filter((m) => m.done).reduce((a, m) => a + m.weight, 0);
    return Math.round((done * 100) / total);
  }

  /** 是否收官：strict 下必须全勾。 */
  isDone(): boolean {
    if (this.items.length === 0) return false;
    if (this.opts.strict) return this.items.every((m) => m.done);
    return this.progress() >= 80;
  }

  /** 时间线（按 kind 分组的完成数）。 */
  timeline(): Array<{ kind: MilestoneKind; done: number; total: number }> {
    const kinds: MilestoneKind[] = ['delivery', 'teaching', 'celebration', 'handover'];
    return kinds.map((k) => {
      const list = this.items.filter((m) => m.kind === k);
      return { kind: k, done: list.filter((m) => m.done).length, total: list.length };
    });
  }

  /** 待办清单。 */
  pending(): string[] {
    return this.items.filter((m) => !m.done).map((m) => m.id);
  }

  /** 致谢名单（按贡献权重）。 */
  credits(): string[] {
    return this.items
      .filter((m) => m.done)
      .sort((a, b) => b.weight - a.weight)
      .map((m) => `${m.id} ${m.title}`);
  }

  /** 基线冻结后禁止改动。 */
  freeze(): boolean {
    this.opts.freezeBaseline = true;
    return this.opts.freezeBaseline;
  }

  canEdit(): boolean {
    return !this.opts.freezeBaseline;
  }

  hint(): string {
    return this.isDone() ? '桌面设计已收官' : `还有 ${this.pending().length} 项待完成`;
  }

  uninstall(): boolean {
    this.items = FINALE_ITEMS.map((m) => ({ ...m }));
    this.opts = { ...DEFAULT_FINALE };
    return this.doneCount() === 0;
  }
}
