/**
 * AI-07 · V-47 搜索/命令面板历史隐私开关（三态）：
 * - local（默认）：本地保存历史
 * - off：彻底不记录——即时执行、零持久化，进程内存也不留，会话结束即焚
 * - clear：一键清空（有确认且不可撤销，由 UI 层做确认门）
 * 红线：不做历史加密（不记录>加密）；不做按条删除（全有或全无）。
 */

export type HistoryPolicy = "local" | "off";

export interface SearchHistoryState {
  policy: HistoryPolicy;
  /** 「off」模式下恒为 0（连内存都不留）。 */
  entries: string[];
}

/** 纯逻辑历史管道：面板/搜索框共用；「off」模式零记忆。 */
export class SearchHistory {
  private policy: HistoryPolicy;
  private entries: string[] = [];

  constructor(policy: HistoryPolicy = "local") {
    this.policy = policy;
  }

  /** 三态切换（切到 off 立即焚毁内存与已存条目）。 */
  setPolicy(p: HistoryPolicy): void {
    this.policy = p;
    if (p === "off") this.burn();
  }

  getPolicy(): HistoryPolicy {
    return this.policy;
  }

  /** 记录一次查询（off 模式零残留）。返回是否实际记录。 */
  record(query: string): boolean {
    const q = query.trim();
    if (!q || this.policy === "off") return false;
    this.entries = [q, ...this.entries.filter((x) => x !== q)].slice(0, 200);
    return true;
  }

  /** 历史条目（off 模式恒空）。 */
  list(): string[] {
    return this.policy === "off" ? [] : [...this.entries];
  }

  count(): number {
    return this.policy === "off" ? 0 : this.entries.length;
  }

  /** 一键清空（不可撤销——调用方 UI 必须先确认）。 */
  clear(): void {
    this.entries = [];
  }

  /** 会话即焚（「off」切换 /「关闭即焚」模式共用）。 */
  burn(): void {
    this.entries = [];
  }
}

/** 设置页展示用：当前策略的可读描述 key（zh/en 词典）。 */
export function policyLabelKey(p: HistoryPolicy): string {
  return p === "local" ? "privacyHistoryLocal" : "privacyHistoryOff";
}
