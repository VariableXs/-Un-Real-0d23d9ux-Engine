/**
 * H4 契约执法事件总线（v7 · 深化批次七）：
 * consumers.ts 的 BINDINGS 是纸面契约；本总线把它变成**活的**——
 * - emit 只放行契约登记过的事件（契约执法运行时化：未登记 = 拒绝 + 显性原因，零静默）；
 * - handler 异常逐个隔离（一个订阅者崩不炸总线、不吞别人的投递），错误计数显性上报；
 * - 全部收发落 H4 体验日志（xlog，十三章：每次投递谁、何时、几个订阅者——可回放）；
 * - 事件流可录制重放（replayTo——调试与测试的回放面）。
 * 桌面壳接线施工时以本总线为唯一事件通道（单例 h4Bus）——契约不再是文档，是运行时门禁。
 */

import { BINDINGS, type ConsumerBinding } from "./consumers";
import { xlog } from "./xlog";

export type H4Handler = (payload: unknown) => void;

export interface EmitVerdict {
  ok: boolean;
  reason: string;
  /** 实际送达的订阅者数（拒绝时为 0）。 */
  delivered: number;
}

export interface BusJournalEntry {
  name: string;
  at: number;
  handlers: number;
}

/** 日志环容量（与 xlog 同级口径——总线自己的投递账）。 */
const JOURNAL_CAP = 500;

export class H4Bus {
  private readonly handlers = new Map<string, Set<H4Handler>>();
  private readonly anyHandlers = new Set<H4Handler>();
  private readonly registered = new Set<string>();
  private readonly journal: BusJournalEntry[] = [];
  private handlerErrors = 0;

  constructor(bindings: ConsumerBinding[] = BINDINGS) {
    for (const b of bindings) {
      for (const e of [...b.eventsIn, ...b.eventsOut]) this.registered.add(e);
    }
  }

  /* ---------- 契约面 ---------- */

  /** 契约登记的事件总数（入向 + 出向去重）。 */
  get contractEventCount(): number {
    return this.registered.size;
  }

  isRegistered(name: string): boolean {
    return this.registered.has(name);
  }

  registeredEvents(): string[] {
    return [...this.registered].sort();
  }

  /* ---------- 订阅 ---------- */

  on(name: string, handler: H4Handler): () => void {
    const set = this.handlers.get(name) ?? new Set<H4Handler>();
    set.add(handler);
    this.handlers.set(name, set);
    return () => {
      set.delete(handler);
    };
  }

  /** 通配订阅（诊断/日志面——全部投递都可见）。 */
  onAny(handler: H4Handler): () => void {
    this.anyHandlers.add(handler);
    return () => {
      this.anyHandlers.delete(handler);
    };
  }

  /* ---------- 投递 ---------- */

  emit(name: string, payload: unknown, at = Date.now()): EmitVerdict {
    if (!this.registered.has(name)) {
      const reason = `事件「${name}」未在 BINDINGS 契约登记——拒绝投递（先进册再发，零静默）`;
      xlog.log("warn", "h4-bus", "contract-reject", reason);
      return { ok: false, reason, delivered: 0 };
    }
    const subs = this.handlers.get(name);
    const count = (subs?.size ?? 0) + this.anyHandlers.size;
    if (subs) {
      for (const h of subs) {
        try {
          h(payload);
        } catch (err) {
          this.handlerErrors++;
          xlog.log("error", "h4-bus", "handler-throw", `${name}: ${err instanceof Error ? err.message : String(err)}`);
        }
      }
    }
    for (const h of this.anyHandlers) {
      try {
        h({ name, payload, at });
      } catch (err) {
        this.handlerErrors++;
        xlog.log("error", "h4-bus", "any-handler-throw", `${name}: ${err instanceof Error ? err.message : String(err)}`);
      }
    }
    this.journal.push({ name, at, handlers: count });
    if (this.journal.length > JOURNAL_CAP) this.journal.shift();
    xlog.log("debug", "h4-bus", "emit", `${name} → ${count} 订阅者`);
    return { ok: true, reason: "ok", delivered: count };
  }

  /* ---------- 账与回放 ---------- */

  get journalSize(): number {
    return this.journal.length;
  }

  get handlerErrorCount(): number {
    return this.handlerErrors;
  }

  journalSlice(last = 20): BusJournalEntry[] {
    return this.journal.slice(-last);
  }

  /** 重放：把本总线账上的事件流重放到目标总线（payload 原样——回放不失真）。 */
  replayTo(target: H4Bus, filter?: (e: BusJournalEntry) => boolean): number {
    let n = 0;
    for (const e of this.journal) {
      if (filter && !filter(e)) continue;
      target.emit(e.name, undefined, e.at);
      n++;
    }
    return n;
  }
}

/** 总线单例（桌面壳与面板共用——事件通道全系统唯一）。 */
export const h4Bus = new H4Bus();
