/**
 * boot://event 容错适配器（任务 28 前端侧就绪件；实机对接待任务 27）。
 *
 * 载荷与 Windows 侧 src-tauri/src/boot.rs LoadEvent 逐字段同构（camelCase）：
 * { seq, progress, currentTask, filePath?, fileCount?, totalCount?, icon, level, elapsedMs? }。
 * 容错规则（总案阶段 3 步骤 8 验收项）：
 * - 重复/回放：seq ≤ 已见最大值 → 去重不重复通知（对齐后端 REPLAY+SEQ 语义）；
 * - 乱序：按 seq 排序后有序通知，消费者永远看到单调序列；
 * - 进度回退：progress 单调夹取（展示值 = 历史最大），真实值原样保留在事件上。
 * bootPhase（loading→exit→done）迁移仍由 BootScreen onExitStart/onDone 驱动，App.tsx 零改动。
 */
export interface BootLoadEvent {
  seq: number;
  /** 真实累计进度 0.0–1.0，仅由已完成工作推进。 */
  progress: number;
  currentTask: string;
  filePath?: string | null;
  fileCount?: number | null;
  totalCount?: number | null;
  icon?: string;
  level?: number;
  elapsedMs?: number | null;
}

export class BootEventBuffer {
  private bySeq = new Map<number, BootLoadEvent>();
  private deliveredUpTo = 0;
  private maxProgress = 0;
  private listeners = new Set<(ev: BootLoadEvent) => void>();

  /** 喂入一条事件（任意顺序/重复）；返回是否为新事件（去重后）。 */
  feed(ev: BootLoadEvent): boolean {
    if (!Number.isFinite(ev.seq) || ev.seq <= 0) return false;
    if (this.bySeq.has(ev.seq)) return false;
    this.bySeq.set(ev.seq, ev);
    this.deliverInOrder();
    return true;
  }

  /** 批量回灌（boot_replay 语义：先到先喂，乱序安全）。 */
  feedAll(events: BootLoadEvent[]): number {
    let n = 0;
    for (const ev of events) if (this.feed(ev)) n += 1;
    return n;
  }

  /** 展示进度：历史最大值（永不回退）。 */
  getProgress(): number {
    return this.maxProgress;
  }

  /** 已接收的最新事件（按 seq 最大者）。 */
  getLatest(): BootLoadEvent | null {
    return this.bySeq.size ? (this.bySeq.get(this.deliveredUpTo) ?? null) : null;
  }

  /** 已去重事件总数。 */
  get count(): number {
    return this.bySeq.size;
  }

  onEvent(fn: (ev: BootLoadEvent) => void): () => void {
    this.listeners.add(fn);
    return () => {
      this.listeners.delete(fn);
    };
  }

  private deliverInOrder(): void {
    for (;;) {
      const next = this.bySeq.get(this.deliveredUpTo + 1);
      if (!next) break;
      this.deliveredUpTo += 1;
      if (Number.isFinite(next.progress)) {
        this.maxProgress = Math.max(this.maxProgress, Math.min(1, Math.max(0, next.progress)));
      }
      for (const fn of this.listeners) fn(next);
    }
  }
}
