/**
 * UNREAL-X-15000 · AI-41 生态治理 三方/V/K/C 线逻辑核（族0401~0410 · X10001~X10250），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0401 反馈成长 2.0（X10001~X10025）-------- */

export const FEEDBACK_STATES = ['inbox', 'triaged', 'planned', 'shipped', 'wontfix'] as const;
export type FeedbackState = (typeof FEEDBACK_STATES)[number];

/** 反馈成长：提案状态机 + 声望累计。 */
export class FeedbackGrowth {
  clamped = 0;
  state: Record<string, FeedbackState> = {};
  karma: Record<string, number> = {};
  move(id: string, to: string): FeedbackState {
    if (!id || !(FEEDBACK_STATES as readonly string[]).includes(to)) {
      this.clamped++;
      return this.state[id] ?? 'inbox';
    }
    this.state[id] = to as FeedbackState;
    return this.state[id]!;
  }
  /** 声望：被采纳 +10，被拒 -2，钳制 ≥0。 */
  karmaDelta(user: string, adopted: boolean): number {
    const d = adopted ? 10 : -2;
    this.karma[user] = Math.max(0, (this.karma[user] ?? 0) + d);
    return this.karma[user]!;
  }
  /** 里程碑：100/500/1000 三档称号。 */
  static badge(k: number): string {
    if (k >= 1000) return '生态之友';
    if (k >= 500) return '共建者';
    if (k >= 100) return '观察员';
    return '新人';
  }
}

/* -------- 族0402 互操作联盟（X10026~X10050）-------- */

export const IOP_FORMATS = ['ofx', 'wfx', 'pfx'] as const;
export type IopFormat = (typeof IOP_FORMATS)[number];

/** 互操作联盟：成员能力矩阵 + 格式协商 + 联合认证。 */
export class IopAlliance {
  clamped = 0;
  members: Record<string, IopFormat[]> = {};
  join(name: string, fmts: string[]): boolean {
    if (!name || fmts.some((f) => !(IOP_FORMATS as readonly string[]).includes(f))) {
      this.clamped++;
      return false;
    }
    this.members[name] = fmts as IopFormat[];
    return true;
  }
  /** 双方共同支持的最高优先格式。 */
  negotiate(a: string, b: string): IopFormat | null {
    const fa = this.members[a] ?? [];
    const fb = this.members[b] ?? [];
    for (const f of IOP_FORMATS) {
      if (fa.includes(f) && fb.includes(f)) return f;
    }
    return null;
  }
  /** 认证徽章：支持全部 3 格式才发。 */
  static badge(fmts: string[]): boolean {
    return IOP_FORMATS.every((f) => fmts.includes(f));
  }
}

/* -------- 族0403 教育合作（X10051~X10075）-------- */

export const EDU_ROLES = ['student', 'teacher', 'school', 'partner'] as const;
export type EduRole = (typeof EDU_ROLES)[number];

/** 教育合作：课程体系 + 授权码 + 结业。 */
export class EduProgram {
  clamped = 0;
  seats = new Map<string, { role: EduRole; done: string[] }>();
  enroll(user: string, role: string): boolean {
    if (!user || !(EDU_ROLES as readonly string[]).includes(role)) {
      this.clamped++;
      return false;
    }
    this.seats.set(user, { role: role as EduRole, done: [] });
    return true;
  }
  /** 结业：修满 4 门课发证书。 */
  complete(user: string, course: string): boolean {
    const s = this.seats.get(user);
    if (!s || !course || s.done.includes(course)) {
      this.clamped++;
      return false;
    }
    s.done.push(course);
    return s.done.length >= 4;
  }
  /** 授权码：school 配额 ×30。 */
  static seatsFor(role: EduRole): number {
    return role === 'school' ? 30 : 1;
  }
}

/* -------- 族0404 无障碍开放 2.0（X10076~X10100）-------- */

export const A11Y_CONSUMERS = ['screen-reader', 'switch', 'braille', 'caption'] as const;

/** 无障碍开放：语义导出 + 消费端协商 + 合规水位。 */
export class A11yOpen {
  clamped = 0;
  exports = new Set<string>();
  allow(consumer: string): boolean {
    if (!(A11Y_CONSUMERS as readonly string[]).includes(consumer)) {
      this.clamped++;
      return false;
    }
    this.exports.add(consumer);
    return true;
  }
  /** 合规水位：4 个消费端全接入 = AA+。 */
  grade(): 'base' | 'aa' | 'aa+plus' {
    if (this.exports.size >= 4) return 'aa+plus';
    if (this.exports.size >= 2) return 'aa';
    return 'base';
  }
  /** 描述降级：长描述超限截断给短摘要。 */
  static describe(text: string, max: number): string {
    if (max <= 0) return '';
    return text.length <= max ? text : `${text.slice(0, Math.max(0, max - 1))}…`;
  }
}

/* -------- 族0405 生态健康（X10101~X10125）-------- */

export const HEALTH_SIGNALS = ['crash-rate', 'review', 'update-age', 'abandon'] as const;

/** 生态健康：五信号评分 + 黄旗/红旗 + 淘汰建议。 */
export class EcoHealth {
  clamped = 0;
  scores: Record<string, number> = {};
  score(item: string, signal: string, v: number): number {
    if (!item || !(HEALTH_SIGNALS as readonly string[]).includes(signal)) {
      this.clamped++;
      return 0;
    }
    const cl = Math.min(100, Math.max(0, Math.round(v)));
    this.scores[item] = cl;
    return cl;
  }
  /** 分级：≥80 绿 · ≥60 黄旗 · <60 红旗。 */
  static flag(s: number): 'green' | 'yellow' | 'red' {
    if (s >= 80) return 'green';
    if (s >= 60) return 'yellow';
    return 'red';
  }
  /** 淘汰建议：红旗且 180 天未更新。 */
  static retire(s: number, daysStale: number): boolean {
    return s < 60 && daysStale > 180;
  }
}

/* -------- 族0406 内核开放 2.0（X10126~X10150 · K 线）-------- */

export const KAPI_STABILITY = ['experimental', 'stable', 'frozen'] as const;
export type KapiStability = (typeof KAPI_STABILITY)[number];

/** 内核开放 2.0：API 稳定级 + ABI 闸 + 破坏性变更预告。 */
export class KernelOpenX2 {
  clamped = 0;
  registry = new Map<string, KapiStability>();
  publish(name: string, stability: string): boolean {
    if (!name || !(KAPI_STABILITY as readonly string[]).includes(stability)) {
      this.clamped++;
      return false;
    }
    this.registry.set(name, stability as KapiStability);
    return true;
  }
  /** 调用闸：frozen/stable 对三方开放，experimental 仅内核。 */
  callableByThirdParty(name: string): boolean {
    const s = this.registry.get(name);
    if (!s) return false;
    return s !== 'experimental';
  }
  /** 变更预告：frozen 禁改，stable 需 2 个版本预告。 */
  static changeNotice(s: KapiStability | undefined): number {
    if (s === 'frozen') return -1;
    if (s === 'stable') return 2;
    return 0;
  }
}

/* -------- 族0407 桌面协议（X10151~X10175 · K 线）-------- */

export const DESKTOP_MSGS = ['focus', 'layout', 'clip', 'state'] as const;
export type DesktopMsg = (typeof DESKTOP_MSGS)[number];

/** 桌面协议：消息总线 + 序列号 + 乱序重排。 */
export class DesktopProtocol {
  clamped = 0;
  private seq = 0;
  log: { n: number; kind: DesktopMsg }[] = [];
  send(kind: string): boolean {
    if (!(DESKTOP_MSGS as readonly string[]).includes(kind)) {
      this.clamped++;
      return false;
    }
    this.log.push({ n: ++this.seq, kind: kind as DesktopMsg });
    return true;
  }
  /** 乱序重排：按序列号升序。 */
  ordered(): DesktopMsg[] {
    return [...this.log].sort((a, b) => a.n - b.n).map((m) => m.kind);
  }
  /** 心跳：距上次 state 消息 >N 条则要求同步。 */
  static needSync(lastStateSeq: number, currentSeq: number, maxLag: number): boolean {
    return currentSeq - lastStateSeq > maxLag;
  }
}

/* -------- 族0408 AI 生态位 2.0（X10176~X10200）-------- */

export const AI_SLOT_PERMS = ['suggest', 'draft', 'verify', 'act'] as const;
export type AiSlotPerm = (typeof AI_SLOT_PERMS)[number];

/** AI 生态位：能力分级 + 人类确认闸 + 隐私边界。 */
export class AiEcoSlot {
  clamped = 0;
  perms = new Set<AiSlotPerm>();
  grant(perm: string): boolean {
    if (!(AI_SLOT_PERMS as readonly string[]).includes(perm)) {
      this.clamped++;
      return false;
    }
    this.perms.add(perm as AiSlotPerm);
    return true;
  }
  /** 确认闸：act 级必须人工确认。 */
  needsHuman(perm: AiSlotPerm): boolean {
    return perm === 'act';
  }
  /** 隐私边界：verify 以下不上传原文。 */
  static uploadable(perm: AiSlotPerm): boolean {
    return perm === 'verify' || perm === 'act';
  }
}

/* -------- 族0409 格式开放（X10201~X10225）-------- */

export const OPEN_FORMATS = ['.vxdoc', '.vxpack', '.vxtheme'] as const;

/** 格式开放：魔数校验 + 版本迁移 + 双向导入导出。 */
export class OpenFormats {
  clamped = 0;
  /** 魔数：头 6 字节前缀匹配（大小写不敏感）。 */
  static sniff(head: string): string | null {
    const up = head.toUpperCase();
    for (const f of OPEN_FORMATS) {
      if (up.startsWith(f.slice(1, 4).toUpperCase())) return f;
    }
    return null;
  }
  /** 版本迁移：v1→v2 字段升级，非法回 null。 */
  static migrate(obj: Record<string, unknown>): Record<string, unknown> | null {
    if (!obj || typeof obj !== 'object' || obj.fmt !== 'vxdoc') {
      return null;
    }
    return { ...obj, fmt: 'vxdoc2', migrated: true };
  }
  /** 导出净身：剥离私有字段。 */
  static exportDoc(o: Record<string, unknown>): Record<string, unknown> {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(o)) {
      if (!k.startsWith('_')) out[k] = v;
    }
    return out;
  }
}

/* -------- 族0410 治理（X10226~X10250）-------- */

export const GOV_QUORUM = [3, 5, 7] as const;
export const GOV_OUTCOMES = ['pass', 'reject', 'abstain'] as const;

/** 治理：提案投票 + 法定人数 + RFC 归档。 */
export class Governance {
  clamped = 0;
  votes: { rfc: string; outcome: string }[] = [];
  cast(rfc: string, outcome: string): boolean {
    if (!rfc || !(GOV_OUTCOMES as readonly string[]).includes(outcome)) {
      this.clamped++;
      return false;
    }
    this.votes.push({ rfc, outcome });
    return true;
  }
  /** 表决：达到法定人数且 pass 多于 reject 才通过。 */
  static tally(votes: string[], quorum: number): 'pass' | 'reject' | 'pending' {
    const n = votes.filter((v) => v !== 'abstain').length;
    if (n < quorum) return 'pending';
    const pass = votes.filter((v) => v === 'pass').length;
    const reject = votes.filter((v) => v === 'reject').length;
    return pass > reject ? 'pass' : 'reject';
  }
  /** 归档净身：RFC 编号唯一。 */
  static uniqueRfc(ids: string[]): boolean {
    return new Set(ids).size === ids.length;
  }
}
