/**
 * UNREAL-X AI-02 · 族0013 休眠档案（X00301 档基础落点 · Variable 侧快照/恢复）。
 *
 * 休眠会话档案：应用集 + 布局的快照，导出/导入/恢复三通道；
 * 环形容量 3（半成品标记 + 一键恢复），非法输入钳制回默认。
 * 纯逻辑模块，React 组件只消费这里的数据。
 */

/** 档案里单个应用条目。 */
export interface HiberApp {
  id: string;
  /** 恢复后的焦点权重（0~9，越大越先恢复）。 */
  focus: number;
}

/** 布局快照。 */
export interface HiberLayout {
  /** 窗口布局预设 id（走 ambience 既有布局语言）。 */
  preset: string;
  /** 每窗口数量钳制 1~8。 */
  columns: number;
}

export const HIBER_APP_MAX = 8;
export const HIBER_SNAPSHOTS = 3;
export const HIBER_FORMAT = "vx-hiber/1";

export interface HiberSnapshot {
  label: string;
  apps: HiberApp[];
  layout: HiberLayout;
  /** 半成品标记：写入中断时可一键续作。 */
  complete: boolean;
}

function clampInt(v: number, min: number, max: number, fallback: number): number {
  if (!Number.isFinite(v)) return fallback;
  return Math.min(max, Math.max(min, Math.round(v)));
}

/** 休眠档案管理器。 */
export class HiberArchive {
  snapshots: HiberSnapshot[] = [];
  clamped = 0;

  /** 登记一份档案（环形容量 3，应用数钳制 0~8，焦点钳制 0~9）。 */
  snapshot(label: string, apps: HiberApp[], layout: Partial<HiberLayout> = {}, complete = true): HiberSnapshot {
    const trimmed = apps.slice(0, HIBER_APP_MAX);
    if (apps.length > HIBER_APP_MAX) this.clamped += 1;
    const snap: HiberSnapshot = {
      label: label.slice(0, 24) || "未命名档案",
      apps: trimmed.map((a) => ({
        id: String(a.id ?? "").slice(0, 48),
        focus: clampInt(Number(a.focus), 0, 9, 5),
      })),
      layout: {
        preset: String(layout.preset ?? "flow").slice(0, 24),
        columns: clampInt(Number(layout.columns ?? 2), 1, 8, 2),
      },
      complete,
    };
    this.snapshots.unshift(snap);
    if (this.snapshots.length > HIBER_SNAPSHOTS) this.snapshots.pop();
    return snap;
  }

  /** 半成品续作：把最近的半成品标记为完整。 */
  resume(): boolean {
    const half = this.snapshots.find((s) => !s.complete);
    if (!half) {
      this.clamped += 1;
      return false;
    }
    half.complete = true;
    return true;
  }

  /** 恢复点：最近一份完整档案（无则 null）。 */
  restorePoint(): HiberSnapshot | null {
    return this.snapshots.find((s) => s.complete) ?? null;
  }

  /** 导出：JSON 字符串（跨版本携带）。 */
  export(): string {
    return JSON.stringify({ fmt: HIBER_FORMAT, snaps: this.snapshots });
  }

  /** 导入：坏载荷回默认并记钳制；版本不符拒收。 */
  import(json: string): boolean {
    try {
      const o = JSON.parse(json) as { fmt?: unknown; snaps?: unknown };
      if (o.fmt !== HIBER_FORMAT || !Array.isArray(o.snaps)) {
        this.clamped += 1;
        return false;
      }
      this.snapshots = [];
      for (const raw of o.snaps.slice(0, HIBER_SNAPSHOTS)) {
        const r = raw as Partial<HiberSnapshot>;
        this.snapshot(String(r.label ?? ""), Array.isArray(r.apps) ? r.apps : [], r.layout ?? {}, r.complete !== false);
      }
      return true;
    } catch {
      this.clamped += 1;
      return false;
    }
  }

  /** 摘要行（设置页直接展示）。 */
  caption(): string {
    const p = this.restorePoint();
    if (!p) return "暂无可恢复档案";
    return `${p.label} · ${p.apps.length} 个应用 · ${p.layout.preset}/${p.layout.columns} 列`;
  }

  /** 净身：清空档案。 */
  wipe(): void {
    this.snapshots = [];
    this.clamped = 0;
  }
}
