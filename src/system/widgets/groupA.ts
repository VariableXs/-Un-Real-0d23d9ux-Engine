// UNREAL-X-15000: AI-10（领域03 桌面与图标 · 族0096~0098 · X02376~X02450），勿删。
// 族0096 微件框架 2.0 / 族0097 微件集 2.0 / 族0098 微件互动层 2.0。落点：src/system/widgets/。

/* ===================== 族0096 微件框架 2.0 ===================== */

export const WIDGET_SIZES = ['small', 'medium', 'large'] as const;
export type WidgetSize = (typeof WIDGET_SIZES)[number];

export interface WidgetDescriptor { id: string; name: string; size: WidgetSize; refreshMs: number }

export type WidgetLifecycle = 'registered' | 'mounted' | 'paused' | 'unmounted';

/** 微件框架：注册表 + 生命周期 + 资源配额 + 去重登记。 */
export class WidgetFramework {
  private registry = new Map<string, { desc: WidgetDescriptor; state: WidgetLifecycle; mounts: number }>();
  private maxMounted = 8;

  register(desc: WidgetDescriptor): 'new' | 'dup' {
    if (this.registry.has(desc.id)) return 'dup';
    this.registry.set(desc.id, { desc: { ...desc }, state: 'registered', mounts: 0 });
    return 'new';
  }

  mount(id: string): boolean {
    const r = this.registry.get(id);
    if (!r || r.state === 'mounted') return false;
    if (this.mountedCount() >= this.maxMounted) return false;
    r.state = 'mounted';
    r.mounts++;
    return true;
  }

  pause(id: string): boolean {
    const r = this.registry.get(id);
    if (!r || r.state !== 'mounted') return false;
    r.state = 'paused';
    return true;
  }

  unmount(id: string): boolean {
    const r = this.registry.get(id);
    if (!r) return false;
    r.state = 'unmounted';
    return true;
  }

  stateOf(id: string): WidgetLifecycle | undefined { return this.registry.get(id)?.state; }
  mountedCount(): number { return [...this.registry.values()].filter((r) => r.state === 'mounted').length; }
  count(): number { return this.registry.size; }

  setMaxMounted(n: number): void { this.maxMounted = Math.max(1, Math.min(32, n)); }

  /** 非法描述护栏（X02381）。 */
  static sanitize(d: Partial<WidgetDescriptor>): WidgetDescriptor {
    const sizes: WidgetSize[] = ['small', 'medium', 'large'];
    return {
      id: typeof d.id === 'string' && d.id ? d.id : 'widget-unknown',
      name: typeof d.name === 'string' && d.name ? d.name : '未命名微件',
      size: d.size && sizes.includes(d.size) ? d.size : 'medium',
      refreshMs: typeof d.refreshMs === 'number' && Number.isFinite(d.refreshMs) && d.refreshMs >= 250 ? d.refreshMs : 1000,
    };
  }
}

/* ===================== 族0097 微件集 2.0 ===================== */

export const WIDGET_CATALOG = ['clock', 'weather', 'notes', 'calendar', 'system', 'battery', 'music', 'todo'] as const;
export type WidgetKind = (typeof WIDGET_CATALOG)[number];

export interface WidgetDatum { kind: WidgetKind; payload: string; at: number }

/** 微件集：八件套目录 + 数据快照 + 刷新节流。 */
export class WidgetCollection {
  private data = new Map<WidgetKind, WidgetDatum>();
  private lastRefresh = new Map<WidgetKind, number>();

  update(kind: WidgetKind, payload: string, at: number): 'fresh' | 'throttled' {
    const last = this.lastRefresh.get(kind) ?? -Infinity;
    if (at - last < 250) return 'throttled';
    this.data.set(kind, { kind, payload, at });
    this.lastRefresh.set(kind, at);
    return 'fresh';
  }

  read(kind: WidgetKind): WidgetDatum | undefined {
    const d = this.data.get(kind);
    return d ? { ...d } : undefined;
  }
  filled(): number { return this.data.size; }
  catalogCount(): number { return WIDGET_CATALOG.length; }

  /** 批量快照（X02422）。 */
  snapshot(): WidgetDatum[] { return [...this.data.values()].map((d) => ({ ...d })); }

  /** 目录守卫（X02426）。 */
  static isKnown(kind: string): kind is WidgetKind { return (WIDGET_CATALOG as readonly string[]).includes(kind); }
}

/* ===================== 族0098 微件互动层 2.0 ===================== */

export type WidgetInteraction = 'click' | 'hover' | 'expand' | 'collapse' | 'drag';

export interface HitRect { x: number; y: number; w: number; h: number; widgetId: string }

/** 互动层：命中测试 + 交互路由 + 展开态管理。 */
export class WidgetInteractionLayer {
  private rects: HitRect[] = [];
  private expandedId: string | null = null;
  private log: Array<{ widgetId: string; type: WidgetInteraction }> = [];

  layout(rects: HitRect[]): void { this.rects = rects.map((r) => ({ ...r })); }

  /** 命中测试（X02376 区域口径）：自上而下取最后绘制者。 */
  hitTest(x: number, y: number): string | null {
    for (let i = this.rects.length - 1; i >= 0; i--) {
      const r = this.rects[i]!;
      if (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) return r.widgetId;
    }
    return null;
  }

  interact(type: WidgetInteraction, widgetId: string): boolean {
    if (type === 'expand') {
      this.expandedId = widgetId;
    } else if (type === 'collapse') {
      if (this.expandedId !== widgetId) return false;
      this.expandedId = null;
    }
    this.log.push({ widgetId, type });
    return true;
  }

  expanded(): string | null { return this.expandedId; }
  logCount(): number { return this.log.length; }

  /** 越界钳制（X02381 边界口径）。 */
  static clampRect(r: HitRect, maxX: number, maxY: number): HitRect {
    return {
      ...r,
      x: Math.min(Math.max(0, r.x), Math.max(0, maxX - r.w)),
      y: Math.min(Math.max(0, r.y), Math.max(0, maxY - r.h)),
      w: Math.max(0, r.w), h: Math.max(0, r.h),
    };
  }
}
