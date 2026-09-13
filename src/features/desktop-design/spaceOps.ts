/**
 * UNREAL-X-15000 · AI-06 空间管理（族0051~0060 · X01251~X01500 · 领域02 · Variable 桌面线）。
 * 落点：src/features/desktop-design/（新建）+ src/features/vision/。
 * 红线：全部模型纯逻辑可断言；复用 AI-05 windowGeo 的几何原语，不重复造轮子。
 */
import type { Rect } from '../../system/windows/windowGeo';

export type { Rect };

/* ===================== 族0051 虚拟桌面工作流（X01251~X01275） ===================== */

/** 虚拟桌面模型：创建/切换/指派 + 每桌窗口清单 + 持久化。 */
export class VirtualDesktops {
  private desks: string[] = ['桌面 1'];
  private current = 0;
  private assign = new Map<string, number>();

  get count(): number { return this.desks.length; }
  get active(): number { return this.current; }
  name(i: number): string { return this.desks[i] ?? ''; }

  create(name?: string): number {
    this.desks.push(name ?? `桌面 ${this.desks.length + 1}`);
    return this.desks.length - 1;
  }

  switchTo(i: number): boolean {
    if (i < 0 || i >= this.desks.length) return false;
    this.current = i;
    return true;
  }

  assignWindow(winId: string, desk = this.current): boolean {
    if (desk < 0 || desk >= this.desks.length) return false;
    this.assign.set(winId, desk);
    return true;
  }

  windowsOn(desk: number): string[] {
    return [...this.assign.entries()].filter(([, d]) => d === desk).map(([id]) => id);
  }

  removeLast(): boolean {
    if (this.desks.length <= 1) return false;
    const idx = this.desks.length - 1;
    for (const [id, d] of [...this.assign]) if (d === idx) this.assign.set(id, 0);
    this.desks.pop();
    if (this.current >= this.desks.length) this.current = this.desks.length - 1;
    return true;
  }

  serialize(): { desks: string[]; current: number; assign: Record<string, number> } {
    return { desks: [...this.desks], current: this.current, assign: Object.fromEntries(this.assign) };
  }

  static deserialize(raw: ReturnType<VirtualDesktops['serialize']>): VirtualDesktops {
    const v = new VirtualDesktops();
    v.desks = [...raw.desks];
    v.current = Math.min(Math.max(0, raw.current), v.desks.length - 1);
    for (const [id, d] of Object.entries(raw.assign)) if (d >= 0 && d < v.desks.length) v.assign.set(id, d);
    return v;
  }
}

/* ===================== 族0052 整理助手 2.0（X01276~X01300） ===================== */

export interface WinInfo { id: string; app: string; rect: Rect }

/** 按应用聚类 + 自动铺排（网格化）+ 撤销栈。 */
export class OrganizeAssistant {
  private undoStack: Array<Array<{ id: string; rect: Rect }>> = [];

  cluster(wins: WinInfo[]): Map<string, string[]> {
    const m = new Map<string, string[]>();
    for (const w of wins) {
      const list = m.get(w.app) ?? [];
      list.push(w.id);
      m.set(w.app, list);
    }
    return m;
  }

  /** 网格化铺排：n 窗 → 最接近正方形的 cols×rows。 */
  autoArrange(wins: WinInfo[], area: Rect): Array<{ id: string; rect: Rect }> {
    this.undoStack.push(wins.map((w) => ({ id: w.id, rect: { ...w.rect } })));
    const n = wins.length;
    if (n === 0) return [];
    const cols = Math.ceil(Math.sqrt(n));
    const rows = Math.ceil(n / cols);
    const cw = Math.floor(area.w / cols);
    const ch = Math.floor(area.h / rows);
    return wins.map((w, i) => ({
      id: w.id,
      rect: { x: area.x + (i % cols) * cw, y: area.y + Math.floor(i / cols) * ch, w: cw, h: ch },
    }));
  }

  undo(): Array<{ id: string; rect: Rect }> | null {
    return this.undoStack.pop() ?? null;
  }

  get undoDepth(): number { return this.undoStack.length; }

  /** 整理建议：重叠 >30% 视为混乱，建议铺排。 */
  suggest(wins: WinInfo[]): 'arrange' | 'ok' {
    for (let i = 0; i < wins.length; i++) {
      for (let j = i + 1; j < wins.length; j++) {
        const a = wins[i]!.rect;
        const b = wins[j]!.rect;
        const ox = Math.max(0, Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x));
        const oy = Math.max(0, Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y));
        const inter = ox * oy;
        const smaller = Math.min(a.w * a.h, b.w * b.h) || 1;
        if (inter / smaller > 0.3) return 'arrange';
      }
    }
    return 'ok';
  }
}

/* ===================== 族0053 无限画布 2.0（X01301~X01325） ===================== */

export interface Transform { x: number; y: number; scale: number }

/** 无限画布变换：平移/缩放（0.1~5 钳制）+ 屏幕↔画布换算。 */
export class CanvasTransform {
  constructor(public t: Transform = { x: 0, y: 0, scale: 1 }) {}

  static readonly MIN_SCALE = 0.1;
  static readonly MAX_SCALE = 5;

  pan(dx: number, dy: number): void { this.t = { ...this.t, x: this.t.x + dx, y: this.t.y + dy }; }

  zoom(cx: number, cy: number, factor: number): void {
    const next = Math.min(CanvasTransform.MAX_SCALE, Math.max(CanvasTransform.MIN_SCALE, this.t.scale * factor));
    const applied = next / this.t.scale;
    this.t = { scale: next, x: cx - (cx - this.t.x) * applied, y: cy - (cy - this.t.y) * applied };
  }

  toCanvas(sx: number, sy: number): { x: number; y: number } {
    return { x: (sx - this.t.x) / this.t.scale, y: (sy - this.t.y) / this.t.scale };
  }

  toScreen(cx: number, cy: number): { x: number; y: number } {
    return { x: cx * this.t.scale + this.t.x, y: cy * this.t.scale + this.t.y };
  }

  reset(): void { this.t = { x: 0, y: 0, scale: 1 }; }
}

/* ===================== 族0054 小地图 2.0（X01326~X01350） ===================== */

/** 小地图：桌面矩形等比缩到缩略区 + 视口指示 + 点击跳转。 */
export class Minimap {
  constructor(private bounds: Rect, private viewW: number, private viewH: number) {}

  scaleRect(r: Rect): Rect {
    const k = Math.min(this.viewW / this.bounds.w, this.viewH / this.bounds.h);
    return {
      x: Math.round(r.x * k) + Math.round((this.viewW - this.bounds.w * k) / 2),
      y: Math.round(r.y * k) + Math.round((this.viewH - this.bounds.h * k) / 2),
      w: Math.max(1, Math.round(r.w * k)),
      h: Math.max(1, Math.round(r.h * k)),
    };
  }

  /** 点击跳转：缩略坐标 → 桌面坐标（钳回边界）。 */
  toDesktop(mx: number, my: number): { x: number; y: number } {
    const k = Math.min(this.viewW / this.bounds.w, this.viewH / this.bounds.h);
    const ox = (this.viewW - this.bounds.w * k) / 2;
    const oy = (this.viewH - this.bounds.h * k) / 2;
    const x = Math.min(Math.max(0, (mx - ox) / k), this.bounds.w);
    const y = Math.min(Math.max(0, (my - oy) / k), this.bounds.h);
    return { x: Math.round(x), y: Math.round(y) };
  }

  viewportIndicator(vw: number, vh: number): Rect {
    return this.scaleRect({ x: 0, y: 0, w: vw, h: vh });
  }
}

/* ===================== 族0055 放映模式 2.0（X01351~X01375） ===================== */

/** 放映模式：窗口逐台全屏轮播，退出即恢复原布局。 */
export class Presentation {
  private queue: string[] = [];
  private idx = -1;
  private saved: Array<{ id: string; rect: Rect }> = [];

  start(ids: string[], rects: Array<{ id: string; rect: Rect }>): boolean {
    if (ids.length === 0) return false;
    this.queue = [...ids];
    this.saved = rects.map((r) => ({ id: r.id, rect: { ...r.rect } }));
    this.idx = 0;
    return true;
  }

  get current(): string | null { return this.queue[this.idx] ?? null; }
  next(): string | null { this.idx = (this.idx + 1) % this.queue.length; return this.current; }
  prev(): string | null { this.idx = (this.idx - 1 + this.queue.length) % this.queue.length; return this.current; }

  /** 退出恢复：按进场快照还原（找不到的窗口静默跳过）。 */
  exit(): Array<{ id: string; rect: Rect }> {
    this.idx = -1;
    const out = this.saved;
    this.queue = [];
    this.saved = [];
    return out;
  }

  get running(): boolean { return this.idx >= 0; }
}

/* ===================== 族0056 空间记忆（X01376~X01400） ===================== */

/** 空间记忆：按上下文标签记忆窗口位置，召回最近一次，容量上限 LRU 逐出。 */
export class SpaceMemory {
  private map = new Map<string, Rect>();
  private order: string[] = [];
  constructor(private capacity = 50) {}

  remember(tag: string, rect: Rect): void {
    if (!this.map.has(tag)) this.order.push(tag);
    this.map.set(tag, rect);
    while (this.order.length > this.capacity) {
      const evict = this.order.shift()!;
      this.map.delete(evict);
    }
  }

  recall(tag: string): Rect | null { return this.map.get(tag) ?? null; }

  forget(tag: string): boolean {
    const ok = this.map.delete(tag);
    this.order = this.order.filter((t) => t !== tag);
    return ok;
  }

  /** 召回最近：返回任意给定标签中最相近坐标的记忆。 */
  nearest(x: number, y: number): { tag: string; dist: number } | null {
    let best: { tag: string; dist: number } | null = null;
    for (const [tag, r] of this.map) {
      const d = Math.hypot(r.x - x, r.y - y);
      if (best === null || d < best.dist) best = { tag, dist: d };
    }
    return best;
  }

  get size(): number { return this.map.size; }
}

/* ===================== 族0057 嗅探 2.0（X01401~X01425） ===================== */

export interface Sniffed { id: string; title: string; app: string; rect: Rect }

/** 窗口嗅探：元数据提取 + 按应用分组 + 去重。 */
export function sniffWindow(id: string, title: string, app: string, rect: Rect): Sniffed {
  return { id, title: title.trim() || '(无标题)', app: app.trim() || '(未知应用)', rect };
}

export function groupByApp(wins: Sniffed[]): Map<string, Sniffed[]> {
  const m = new Map<string, Sniffed[]>();
  for (const w of wins) {
    const list = m.get(w.app) ?? [];
    if (!list.some((x) => x.id === w.id)) list.push(w);
    m.set(w.app, list);
  }
  return m;
}

export function dedupeSniffs(wins: Sniffed[]): Sniffed[] {
  const seen = new Set<string>();
  return wins.filter((w) => (seen.has(w.id) ? false : (seen.add(w.id), true)));
}

/* ===================== 族0058 焦点引力（X01426~X01450） ===================== */

export interface FocusCandidate { id: string; dist: number; recencyMs: number }

/** 焦点引力：按距离+新近度评分选目标，含滞回（当前目标领先时不易被夺走）。 */
export function focusGravity(candidates: FocusCandidate[], thresholdDist = 120): string | null {
  let best: FocusCandidate | null = null;
  for (const c of candidates) {
    if (c.dist > thresholdDist) continue;
    if (best === null || c.dist < best.dist || (c.dist === best.dist && c.recencyMs < best.recencyMs)) best = c;
  }
  return best?.id ?? null;
}

/** 滞回：现任目标距离劣势 ≤20% 时保持现任。 */
export function gravityHysteresis(currentDist: number, challengerDist: number): 'keep' | 'switch' {
  return challengerDist < currentDist * 0.8 ? 'switch' : 'keep';
}

/* ===================== 族0059 收纳坞 2.0（X01451~X01475） ===================== */

/** 收纳坞：固定槽 + 溢出抽屉 + 钉选/解钉。 */
export class Dock {
  private pinned: string[] = [];
  private overflow: string[] = [];
  constructor(private capacity = 10) {}

  pin(id: string): boolean {
    if (this.pinned.includes(id)) return false;
    if (this.pinned.length >= this.capacity) return false;
    this.pinned.push(id);
    this.overflow = this.overflow.filter((x) => x !== id);
    return true;
  }

  unpin(id: string): boolean {
    const ok = this.pinned.includes(id);
    this.pinned = this.pinned.filter((x) => x !== id);
    return ok;
  }

  stash(id: string): boolean {
    if (this.pinned.includes(id) || this.overflow.includes(id)) return false;
    this.overflow.push(id);
    return true;
  }

  /** 收纳降容：超出容量时最旧溢出项逐出。 */
  trim(): string[] {
    const evicted: string[] = [];
    while (this.overflow.length + this.pinned.length > this.capacity && this.overflow.length > 0) {
      evicted.push(this.overflow.shift()!);
    }
    return evicted;
  }

  get slots(): string[] { return [...this.pinned]; }
  get drawer(): string[] { return [...this.overflow]; }
  get size(): number { return this.pinned.length + this.overflow.length; }
}

/* ===================== 族0060 性能降级（X01476~X01500） ===================== */

export type DegradeTier = 'full' | 'medium' | 'low';

/** 降级链：按负载分档，带滞回（升档需连续两个周期好转），省电档强制 low。 */
export class DegradeChain {
  tier: DegradeTier = 'full';
  private goodStreak = 0;
  private readonly thresholds = { medium: 0.8, low: 0.95 };

  feed(load: number, powerSaver = false): DegradeTier {
    if (powerSaver) { this.tier = 'low'; this.goodStreak = 0; return this.tier; }
    if (load >= this.thresholds.low) { this.tier = 'low'; this.goodStreak = 0; }
    else if (load >= this.thresholds.medium) { this.tier = this.tier === 'low' ? 'low' : 'medium'; this.goodStreak = 0; }
    else {
      this.goodStreak++;
      if (this.tier === 'low' && this.goodStreak >= 2) { this.tier = 'medium'; this.goodStreak = 0; }
      else if (this.tier === 'medium' && this.goodStreak >= 2) { this.tier = 'full'; this.goodStreak = 0; }
    }
    return this.tier;
  }

  /** 效果开关矩阵：低档关动效/玻璃/缩略图。 */
  effects(): { motion: boolean; glass: boolean; thumbnails: boolean } {
    return {
      motion: this.tier !== 'low',
      glass: this.tier === 'full',
      thumbnails: this.tier !== 'low',
    };
  }
}
