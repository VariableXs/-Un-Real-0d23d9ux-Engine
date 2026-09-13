/**
 * UNREAL-X AI-01 · 族0009 多系统选择剧场（X00201~X00225）。
 *
 * 启动项列表 + 选择记忆：固定容量启动项注册表、默认项标记、
 * 选择记忆（记住上次选择）、不可用项过滤、越界钳制。
 * 与内核侧 bootchain.rs 的 MenuEntry 语义对齐但独立演进。
 */

export const BOOT_ENTRY_MAX = 8;
export const ENTRY_NAME_MAX = 24;

export type BootEntryKind = "variable" | "previous" | "windows" | "linux" | "recovery";

export interface BootEntry {
  id: string;
  name: string;
  kind: BootEntryKind;
  available: boolean;
  /** 上次引导时间戳（0 = 从未引导）。 */
  lastBootMs: number;
}

/** 多系统选择剧场状态。 */
export class MultiBootTheater {
  entries: BootEntry[] = [];
  /** 选择记忆：上次选中项 id（未选择 = null）。 */
  remembered: string | null = null;
  /** 默认项 id（倒计时结束进入哪一项）。 */
  defaultId: string | null = null;
  /** 是否隐藏不可用项。 */
  hideUnavailable = false;
  clamped = 0;

  /** 登记启动项：容量 8、重名拒绝、名字钳到 24 字节显示宽。 */
  add(id: string, name: string, kind: BootEntryKind, available = true): boolean {
    const cleanId = id.trim();
    if (!cleanId || this.entries.length >= BOOT_ENTRY_MAX || this.entries.some((e) => e.id === cleanId)) {
      this.clamped += 1;
      return false;
    }
    this.entries.push({
      id: cleanId,
      name: name.slice(0, ENTRY_NAME_MAX),
      kind,
      available,
      lastBootMs: 0,
    });
    return true;
  }

  /** 删除项（若为默认/记忆项则一并清除，不留残档）。 */
  remove(id: string): boolean {
    const before = this.entries.length;
    this.entries = this.entries.filter((e) => e.id !== id);
    if (this.entries.length === before) {
      this.clamped += 1;
      return false;
    }
    if (this.defaultId === id) this.defaultId = null;
    if (this.remembered === id) this.remembered = null;
    return true;
  }

  /** 选择启动项：记录选择记忆；不可用项拒绝并记钳制。 */
  select(id: string, nowMs: number): BootEntry | null {
    const e = this.entries.find((x) => x.id === id);
    if (!e || !e.available) {
      this.clamped += 1;
      return null;
    }
    e.lastBootMs = nowMs;
    this.remembered = id;
    return e;
  }

  /** 标记默认项：必须存在。 */
  setDefault(id: string): boolean {
    if (!this.entries.some((e) => e.id === id)) {
      this.clamped += 1;
      return false;
    }
    this.defaultId = id;
    return true;
  }

  /** 剧场渲染清单：按可用性过滤 + 记忆项置顶。 */
  renderList(): BootEntry[] {
    const list = this.hideUnavailable ? this.entries.filter((e) => e.available) : [...this.entries];
    if (!this.remembered) return list;
    const mem = list.find((e) => e.id === this.remembered);
    if (!mem) return list;
    return [mem, ...list.filter((e) => e.id !== this.remembered)];
  }

  /** 倒计时结束后实际进入的项：默认项，默认项不可用则回落记忆项，再回落第一个可用项。 */
  resolveTarget(): string | null {
    const byId = (id: string | null) => (id ? this.entries.find((e) => e.id === id && e.available) : undefined);
    const hit = byId(this.defaultId) ?? byId(this.remembered) ?? this.entries.find((e) => e.available);
    return hit?.id ?? null;
  }

  /** 回滚净身：清空注册表与记忆。 */
  reset(): void {
    this.entries = [];
    this.remembered = null;
    this.defaultId = null;
    this.clamped = 0;
  }
}

/** 条目摘要行（UI 直读）。 */
export function entryCaption(e: BootEntry): string {
  const tag = e.available ? "" : "（不可用）";
  return `${e.name} · ${e.kind}${tag}`;
}
