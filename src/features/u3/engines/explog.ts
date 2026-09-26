/**
 * U3 领地体验日志引擎（AI-U3 · explog · 十三章/十三·补纪律的落地面）。
 *
 * 纪律唯一源（入格宪章十三章/十三·补）：
 * - 「记录到交互细节层：每次点击/右键/拖拽/快捷键——在哪个界面、对哪个元素、
 *   什么时刻、触发了什么、反馈是什么、耗时多少」。
 * - 「主动捕获挫败信号：狂点（rage click）、点击无反馈区（dead click）、
 *   反复开关的浮层、被打断后放弃的操作」。
 * - 「每个事件带体验结论字段：顺畅/卡顿/无反馈/被打断/报错」。
 * - 「隐私红线：只记交互行为与结果，不记用户输入的具体内容」；「写入绝不
 *   阻塞交互（异步、批量）」。
 *
 * 深化点：U3 域（锁屏/指针/横幅/剪贴板/粉碎等）此前只有判据自检，没有
 * 交互面日志——本引擎补齐统一事件层：环形缓冲（有上限，不涨内存）、
 * 挫败信号指纹器、结论推导器、脱敏闸门、批量出口。
 */

/* ------------------------------ 事件模型 ------------------------------ */

export type ExpVerdict = "smooth" | "laggy" | "no-response" | "interrupted" | "error";

export interface ExpEvent {
  /** 事件序号（单调递增——时间轴排序依据）。 */
  seq: number;
  atMs: number;
  /** U3 域标识（f501…f550 / runtime / lab）。 */
  domain: string;
  /** 交互动作（click/dblclick/contextmenu/hotkey/drag/scroll/overlay-open/overlay-close…）。 */
  action: string;
  /** 元素标识（脱敏后的稳定 id，不含用户内容）。 */
  targetId: string;
  /** 耗时 ms（触发到反馈）。 */
  latencyMs: number;
  verdict: ExpVerdict;
}

/** 环形缓冲上限（内存上限纪律——十四章性能是开放性前提）。 */
export const EXPLOG_CAP = 2000;

/* ------------------------------ 日志器 ------------------------------ */

export class ExpLog {
  private buf: ExpEvent[] = [];
  private nextSeq = 1;
  /** 同点位点击时间（环形缓冲即事实源，指纹器直接扫描）。 */
  /** 浮层开关序列（flap 指纹器用）。 */
  private overlayTurns = new Map<string, Array<number>>();

  /** 记录一条交互事件（同步入环形缓冲——入队 O(1) 不阻塞交互；导出走批量）。 */
  record(e: Omit<ExpEvent, "seq">): ExpEvent {
    const full: ExpEvent = { ...e, seq: this.nextSeq++ };
    this.buf.push(full);
    if (this.buf.length > EXPLOG_CAP) this.buf.splice(0, this.buf.length - EXPLOG_CAP);
    return full;
  }

  /**
   * 挫败指纹器（判据「糟糕体验的指纹」自动标记）：
   * - rage click：同点位 1.5s 内 ≥3 次点击
   * - dead click：latencyMs ≥ 1000 或 verdict=no-response
   * - overlay flap：同一浮层 10s 内 ≥3 次开关
   * 返回挫败事件清单（应转发到总日志中心并标体验结论）。
   */
  frustrationSignals(): Array<{ kind: "rage-click" | "dead-click" | "overlay-flap"; targetId: string; atMs: number; verdict: ExpVerdict }> {
    const out: Array<{ kind: "rage-click" | "dead-click" | "overlay-flap"; targetId: string; atMs: number; verdict: ExpVerdict }> = [];
    for (const e of this.buf) {
      if (e.action === "click") {
        const recent = this.buf.filter((x) => x.action === "click" && x.targetId === e.targetId && e.atMs - x.atMs <= 1500);
        if (recent.length >= 3) {
          out.push({ kind: "rage-click", targetId: e.targetId, atMs: e.atMs, verdict: "no-response" });
        }
      }
      if (e.action === "click" && (e.latencyMs >= 1000 || e.verdict === "no-response")) {
        out.push({ kind: "dead-click", targetId: e.targetId, atMs: e.atMs, verdict: e.verdict });
      }
      if (e.action === "overlay-open" || e.action === "overlay-close") {
        const turns = this.overlayTurns.get(e.targetId) ?? [];
        turns.push(e.atMs);
        while (turns.length > 0 && e.atMs - turns[0]! > 10000) turns.shift();
        this.overlayTurns.set(e.targetId, turns);
        if (turns.length >= 3) {
          out.push({ kind: "overlay-flap", targetId: e.targetId, atMs: e.atMs, verdict: "interrupted" });
        }
      }
    }
    return out;
  }

  /** 结论推导（判据「每个事件带体验结论」：延迟→结论的统一口径）。 */
  static verdictFor(latencyMs: number, failed: boolean, interrupted: boolean): ExpVerdict {
    if (failed) return "error";
    if (interrupted) return "interrupted";
    if (latencyMs >= 1000) return "no-response";
    if (latencyMs >= 100) return "laggy";
    return "smooth";
  }

  /** 批量导出（判据「结构化与可回放」：按时间轴有序导出副本）。 */
  exportAll(): ExpEvent[] {
    return [...this.buf].sort((a, b) => a.seq - b.seq);
  }

  /** 域过滤（总日志中心的过滤口径）。 */
  exportByDomain(domain: string): ExpEvent[] {
    return this.exportAll().filter((e) => e.domain === domain);
  }

  /** 脱敏闸门（判据「隐私红线」：目标 id 不得含疑似敏感内容——密码/正文全文）。 */
  static sanitizedTargetId(raw: string): string {
    const cleaned = raw.replace(/\s+/g, "-");
    if (/(password|passwd|secret|密码|密钥)/i.test(cleaned)) return "target:redacted";
    if (cleaned.length > 64) return `${cleaned.slice(0, 61)}...`;
    return cleaned;
  }

  size(): number {
    return this.buf.length;
  }
}

/** 全局实例（单例——全 U3 域共享一个时间轴）。 */
export const u3ExpLog = new ExpLog();

/** 挫败信号 → 体验改进清单条目（判据「最挫败的十次操作」维度）。 */
export function toImprovementItems(
  signals: Array<{ kind: string; targetId: string; atMs: number; verdict: string }>,
): Array<{ rank: number; kind: string; targetId: string; suggestion: string }> {
  const count = new Map<string, number>();
  for (const s of signals) {
    const k = `${s.kind}@${s.targetId}`;
    count.set(k, (count.get(k) ?? 0) + 1);
  }
  return [...count.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 10)
    .map(([k, n], i) => {
      const at = k.indexOf("@");
      const kind = k.slice(0, at);
      const targetId = k.slice(at + 1);
      const suggestion =
        kind === "rage-click" ? `「${targetId}」狂点 ${n} 次——检查反馈是否存在（100ms 反馈红线）`
        : kind === "dead-click" ? `「${targetId}」点击无反馈——检查事件绑定与禁用态呈现`
        : `「${targetId}」浮层 10s 内反复开关 ${n} 次——检查关闭路径与误触焦电`;
      return { rank: i + 1, kind, targetId, suggestion };
    });
}

/* ------------------------------ 自检 ------------------------------ */

export function explogSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 结论推导口径
  checks.push({ name: "十三章 结论推导", pass: ExpLog.verdictFor(50, false, false) === "smooth" && ExpLog.verdictFor(120, false, false) === "laggy" && ExpLog.verdictFor(1500, false, false) === "no-response" && ExpLog.verdictFor(10, true, false) === "error" && ExpLog.verdictFor(10, false, true) === "interrupted" });
  // 脱敏：密码字段打码、超长截断
  checks.push({ name: "十三章 脱敏闸门", pass: ExpLog.sanitizedTargetId("input-password") === "target:redacted" && ExpLog.sanitizedTargetId("输入密码框") === "target:redacted" && ExpLog.sanitizedTargetId("a".repeat(100)).length === 64 });
  // 环形缓冲上限
  const log = new ExpLog();
  for (let i = 0; i < EXPLOG_CAP + 50; i++) {
    log.record({ atMs: i, domain: "f513", action: "click", targetId: `t${i % 7}`, latencyMs: 10, verdict: "smooth" });
  }
  checks.push({ name: "十三章 环形缓冲上限", pass: log.size() === EXPLOG_CAP });
  // dead click 指纹
  const log2 = new ExpLog();
  log2.record({ atMs: 1, domain: "f509", action: "click", targetId: "shred-btn", latencyMs: 1500, verdict: "no-response" });
  checks.push({ name: "十三章 dead click 指纹", pass: log2.frustrationSignals().some((s) => s.kind === "dead-click") });
  // overlay flap：10s 内 3 次开关
  const log3 = new ExpLog();
  for (let i = 0; i < 3; i++) {
    log3.record({ atMs: i * 2000, domain: "f525", action: "overlay-open", targetId: "keycard", latencyMs: 10, verdict: "smooth" });
    log3.record({ atMs: i * 2000 + 1000, domain: "f525", action: "overlay-close", targetId: "keycard", latencyMs: 10, verdict: "smooth" });
  }
  checks.push({ name: "十三章 overlay flap 指纹", pass: log3.frustrationSignals().some((s) => s.kind === "overlay-flap") });
  // 改进清单排名
  const items = toImprovementItems(log2.frustrationSignals());
  checks.push({ name: "十三章 改进清单", pass: items.length === 1 && items[0]!.rank === 1 && items[0]!.suggestion.includes("无反馈") });
  // 域过滤与有序导出
  const all = log.exportAll();
  checks.push({ name: "十三章 时间轴有序导出", pass: all.every((e, i) => i === 0 || all[i - 1]!.seq < e.seq) && log.exportByDomain("f513").every((e) => e.domain === "f513") });
  return checks;
}
