/**
 * 十一章 渐进披露导览 + 三铁律 预设库 + 四章 键鼠对等审计（收官三合一深化）。
 *
 * - tour-engine：新手五分钟路线 → 分步 spotlight（高亮元素+说明+下一步）
 *   → 可跳过/可找回（first-run 接线）→ 渐进披露（进阶功能不在首屏轰炸）；
 * - preset-library：官方预设只读 + 用户预设可存可删 + 预设 diff 预览 +
 *   一键应用带撤销（三铁律"预设+微调"的数据面）；
 * - keyboard-map：每个鼠标动作登记键盘等价物——对等审计（缺等价 = 红线）。
 */

// ---------- tour-engine（分步导览） ----------

export interface TourStep {
  id: string;
  page: string;
  /** 高亮目标元素（spotlight 的锚点）。 */
  targetId: string;
  /** 这一步说什么（一句话——不是教程）。 */
  note: string;
  /** 下一步触发（点击目标/点击下一步/自动 3s）。 */
  advance: "click-target" | "next-button" | "auto-3s";
}

export interface Tour {
  id: string;
  title: string;
  steps: TourStep[];
  /** 五分钟内可完成（步数 × 平均 30s ≤ 300s 的契约）。 */
  withinFiveMin: boolean;
}

export function buildTour(id: string, title: string, steps: Array<[string, string, string, TourStep["advance"]]>): Tour {
  const built: TourStep[] = steps.map(([page, targetId, note, advance]) => ({ id: `${id}-${targetId}`, page, targetId, note, advance }));
  return { id, title, steps: built, withinFiveMin: built.length <= 10 };
}

/** 导览推进器（spotlight 游标——只进不退，Esc 整体跳过）。 */
export class TourRunner {
  private idx = 0;
  constructor(private tour: Tour) {}

  get current(): TourStep | null {
    return this.tour.steps[this.idx] ?? null;
  }

  get done(): boolean {
    return this.idx >= this.tour.steps.length;
  }

  next(): TourStep | null {
    this.idx++;
    return this.current;
  }

  /** 进度百分比（诚实进度条——导览也有进度语义）。 */
  progress(): number {
    return Math.round((this.idx / this.tour.steps.length) * 100);
  }
}

// ---------- preset-library（预设库） ----------

export interface Preset {
  id: string;
  name: string;
  /** 官方预设只读（不可删改——出厂保证）。 */
  official: boolean;
  /** 预设载荷（分节配置——与 store 分节同构）。 */
  payload: Record<string, unknown>;
}

export class PresetLibrary {
  private presets = new Map<string, Preset>();

  registerOfficial(id: string, name: string, payload: Record<string, unknown>): void {
    this.presets.set(id, { id, name, official: true, payload });
  }

  /** 用户预设可存可删（官方保护——删除官方 = 显性拒绝）。 */
  saveUser(id: string, name: string, payload: Record<string, unknown>): { ok: boolean; reason?: string } {
    const existing = this.presets.get(id);
    if (existing?.official) return { ok: false, reason: `${id} 是官方预设——不可覆盖（出厂保证）` };
    this.presets.set(id, { id, name, official: false, payload });
    return { ok: true };
  }

  deleteUser(id: string): { ok: boolean; reason?: string } {
    const p = this.presets.get(id);
    if (!p) return { ok: false, reason: "不存在" };
    if (p.official) return { ok: false, reason: "官方预设不可删（可随时回退的出厂保证）" };
    this.presets.delete(id);
    return { ok: true };
  }

  get all(): Preset[] {
    return [...this.presets.values()];
  }

  get(id: string): Preset | null {
    return this.presets.get(id) ?? null;
  }

  /** 预设 diff 预览（应用前先看差异——不是盲应用）。 */
  diff(preset: Preset, current: Record<string, unknown>): Array<{ path: string; from: unknown; to: unknown }> {
    const out: Array<{ path: string; from: unknown; to: unknown }> = [];
    for (const [k, v] of Object.entries(preset.payload)) {
      if (JSON.stringify(current[k]) !== JSON.stringify(v)) {
        out.push({ path: k, from: current[k], to: v });
      }
    }
    return out;
  }
}

// ---------- keyboard-map（键鼠对等审计） ----------

export interface ActionPair {
  action: string;
  /** 鼠标触发（方式描述）。 */
  mouse: string;
  /** 键盘等价物（null = 缺失 = 红线）。 */
  keyboard: string | null;
}

/** 对等审计：键盘用户与鼠标用户能力必须对等（缺失逐条列——红线机检）。 */
export function auditParity(pairs: ActionPair[]): { ok: boolean; missing: Array<{ action: string; mouse: string }>; total: number } {
  const missing = pairs.filter((p) => p.keyboard === null).map((p) => ({ action: p.action, mouse: p.mouse }));
  return { ok: missing.length === 0, missing, total: pairs.length };
}

/** E 域标准对偶表（二十页核心动作——新动作登记即入审计）。 */
export const E_ACTION_PAIRS: ActionPair[] = [
  { action: "打开设置页", mouse: "点击侧栏项", keyboard: "Ctrl+, 全局" },
  { action: "应用预览更改", mouse: "点击应用钮", keyboard: "Ctrl+Enter 页内" },
  { action: "放弃预览更改", mouse: "点击放弃钮", keyboard: "Ctrl+Backspace 页内" },
  { action: "切换深浅主题", mouse: "点击切换开关", keyboard: "Ctrl+Shift+D 全局" },
  { action: "重录快捷键", mouse: "点击条目+按下新组合", keyboard: "Tab 到条目 + Enter 进入录制 + 组合键" },
  { action: "导入档案", mouse: "点击导入+粘贴", keyboard: "Ctrl+V 焦点在文本框" },
];
