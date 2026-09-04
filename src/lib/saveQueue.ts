/**
 * Serialized save coordinator.
 *
 * - Tasks for the same key never run concurrently (no interleaved DB writes
 *   that could resurrect old content over newer content).
 * - If several saves pile up for one key while a task runs, only the latest
 *   payload survives (latest-wins), preventing stale overwrites.
 */
export type SaveTask<P> = (payload: P) => Promise<void>;

export interface SaveCoordinatorOptions {
  /** Total attempts per entry, including the first (default 3 = 2 retries). */
  attempts?: number;
  /** Base delay for exponential backoff between attempts in ms (default 120). */
  backoffMs?: number;
}

interface QueueEntry {
  payload: unknown;
  resolve: () => void;
  reject: (e: unknown) => void;
  dropped?: boolean;
}

const delay = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

export class SaveCoordinator {
  private queues = new Map<string, QueueEntry[]>();
  private draining = new Set<string>();
  // Accepts any concrete task fn thanks to `never` parameter contravariance.
  private task: SaveTask<never>;
  private attempts: number;
  private backoffMs: number;

  constructor(task: SaveTask<never>, options?: SaveCoordinatorOptions) {
    this.task = task;
    this.attempts = Math.max(1, options?.attempts ?? 3);
    this.backoffMs = Math.max(0, options?.backoffMs ?? 120);
  }

  submit(key: string, payload: unknown): Promise<void> {
    return new Promise((resolve, reject) => {
      let q = this.queues.get(key);
      if (!q) {
        q = [];
        this.queues.set(key, q);
      }
      // Latest-wins: drop every entry that has not started yet.
      for (const e of q) {
        e.dropped = true;
        e.resolve();
      }
      q.length = 0;
      q.push({ payload, resolve, reject });
      void this.drain(key);
    });
  }

  private async runWithRetry(payload: unknown): Promise<void> {
    let lastError: unknown;
    for (let attempt = 0; attempt < this.attempts; attempt++) {
      if (attempt > 0) await delay(this.backoffMs * Math.pow(2, attempt - 1));
      try {
        await this.task(payload as never);
        return;
      } catch (e) {
        lastError = e;
      }
    }
    throw lastError;
  }

  private async drain(key: string): Promise<void> {
    if (this.draining.has(key)) return;
    this.draining.add(key);
    try {
      for (;;) {
        const q = this.queues.get(key);
        if (!q || q.length === 0) break;
        const entry = q.shift();
        if (!entry || entry.dropped) continue;
        try {
          await this.runWithRetry(entry.payload);
          entry.resolve();
        } catch (e) {
          entry.reject(e);
        }
      }
    } finally {
      this.draining.delete(key);
    }
  }

  /** True while any task for this key is executing or queued. */
  isBusy(key: string): boolean {
    return this.draining.has(key) || (this.queues.get(key)?.length ?? 0) > 0;
  }
}
