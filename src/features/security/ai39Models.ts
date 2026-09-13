/**
 * UNREAL-X-15000 · AI-39 安全深水区与收官 V/三方线逻辑核（族0381~0390 · X09501~X09750），勿删。
 * 本文件承载 V/三方线五族逻辑模型（奖励/儿童/合规/安全无障碍/收官）；
 * C 线四族（模糊/审查/脱敏/档案）见 code-analysis/core/src/ai39.rs，
 * K 线族0388（安全恢复）见 kernel/varix/src/sec/secover2.rs。
 * 五层 × 五档模板，全部确定性算法，零 AI。
 */

/* -------- 族0382 漏洞奖励计划（X09526~X09550）-------- */

/** 报告严重级：0 低危 / 1 中危 / 2 高危 / 3 严重。 */
export const BOUNTY_SEVERITY = ['low', 'medium', 'high', 'critical'] as const;
export type BountySeverity = (typeof BOUNTY_SEVERITY)[number];

export const BOUNTY_YUAN: Record<BountySeverity, number> = { low: 200, medium: 1000, high: 5000, critical: 20000 };

export interface BountyReport {
  id: number;
  severity: BountySeverity;
  /** 是否首发（重复报告不重复计奖）。 */
  first: boolean;
}

/** 漏洞奖励计划：提交 → 去重 → 定级 → 发奖 → 榜单。 */
export class BountyProgram {
  seenSeverity = new Map<string, BountySeverity>();
  paidYuan = 0;
  hall: number[] = [];
  clamped = 0;
  /** 受理报告：重复（同指纹）不发奖；未知严重级钳制回 low。 */
  accept(fp: string, r: BountyReport): number {
    if (this.seenSeverity.has(fp)) {
      return 0;
    }
    let sev = r.severity;
    if (!(BOUNTY_SEVERITY as readonly string[]).includes(sev)) {
      this.clamped++;
      sev = 'low';
    }
    this.seenSeverity.set(fp, sev);
    const yuan = BOUNTY_YUAN[sev];
    if (r.first) this.paidYuan += yuan;
    this.hall.push(r.id);
    return yuan;
  }
  /** 榜单：按受理顺序的已首发报告数。 */
  leaderboard(): number {
    return this.hall.length;
  }
  static label(s: BountySeverity): string {
    return { low: '低危', medium: '中危', high: '高危', critical: '严重' }[s];
  }
}

/* -------- 族0385 儿童防护（X09601~X09625）-------- */

/** 儿童档位：0 自由 / 1 引导 / 2 严格 / 3 睡眠优先 / 4 考试模式。 */
export const KID_LEVELS = ['free', 'guided', 'strict', 'sleep-first', 'exam'] as const;
export type KidLevel = (typeof KID_LEVELS)[number];

export interface KidPolicy {
  /** 每日可用分钟。 */
  dailyMinutes: number;
  /** 内容分级上限（0 全部 ~ 4 仅适合）。 */
  maxRating: number;
  /** 支付禁用。 */
  payBlocked: boolean;
}

export const KID_PRESETS: Record<KidLevel, KidPolicy> = {
  free: { dailyMinutes: 0, maxRating: 0, payBlocked: false },
  guided: { dailyMinutes: 120, maxRating: 2, payBlocked: true },
  strict: { dailyMinutes: 60, maxRating: 1, payBlocked: true },
  'sleep-first': { dailyMinutes: 30, maxRating: 1, payBlocked: true },
  exam: { dailyMinutes: 0, maxRating: 0, payBlocked: true },
};

/** 儿童防护：档位策略 + 使用时长裁决 + 家长解锁码。 */
export class KidShield {
  level: KidLevel = 'guided';
  usedMinutes = 0;
  clamped = 0;
  unlockLog: string[] = [];
  apply(level: string): KidLevel {
    if (!(KID_LEVELS as readonly string[]).includes(level)) {
      this.clamped++;
      this.level = 'guided';
    } else {
      this.level = level as KidLevel;
    }
    return this.level;
  }
  get policy(): KidPolicy {
    return KID_PRESETS[this.level];
  }
  /** 使用裁决：超过每日额度 → deny（0 额度 = 全天 deny）。 */
  usagePolicy(min: number): 'allow' | 'deny' {
    const cap = this.policy.dailyMinutes;
    return cap === 0 || this.usedMinutes + min > cap ? 'deny' : 'allow';
  }
  /** 内容裁决：评级超过上限 → deny。 */
  contentPolicy(rating: number): 'allow' | 'deny' {
    return rating > this.policy.maxRating ? 'deny' : 'allow';
  }
  /** 家长解锁：6 位数字码，错 3 次锁定。 */
  unlock(code: string, truth: string, attempts: number): boolean {
    if (attempts >= 3) return false;
    const ok = code === truth && /^\d{6}$/.test(code);
    if (ok) this.unlockLog.push('ok');
    return ok;
  }
  static label(lv: KidLevel): string {
    return { free: '自由', guided: '引导', strict: '严格', 'sleep-first': '睡眠优先', exam: '考试模式' }[lv];
  }
}

/* -------- 族0386 安全合规地图（X09626~X09650）-------- */

/** 合规域：0 数据本地 / 1 知情同意 / 2 最小收集 / 3 可删除 / 4 审计留痕 / 5 跨境评估。 */
export const COMPLIANCE_DOMAINS = ['local-data', 'consent', 'min-collect', 'deletable', 'audit-trail', 'cross-border'] as const;
export type ComplianceDomain = (typeof COMPLIANCE_DOMAINS)[number];

/** 合规地图：域 → 就位状态；覆盖度 = 就位数 / 总域数。 */
export class ComplianceMap {
  status = new Map<ComplianceDomain, boolean>();
  clamped = 0;
  /** 登记域状态：未知域钳制拒绝。 */
  mark(domain: string, ok: boolean): boolean {
    if (!(COMPLIANCE_DOMAINS as readonly string[]).includes(domain)) {
      this.clamped++;
      return false;
    }
    this.status.set(domain as ComplianceDomain, ok);
    return true;
  }
  /** 覆盖度百分比（就位数 / 6，下取整）。 */
  coverage(): number {
    let ok = 0;
    for (const d of COMPLIANCE_DOMAINS) if (this.status.get(d) === true) ok++;
    return Math.floor((ok * 100) / COMPLIANCE_DOMAINS.length);
  }
  /** 缺口清单：未就位域列表。 */
  gaps(): ComplianceDomain[] {
    return COMPLIANCE_DOMAINS.filter((d) => this.status.get(d) !== true);
  }
  /** 等级：100 完全合规 / ≥67 基本 / ≥34 部分 / 其余 缺失。 */
  grade(): string {
    const c = this.coverage();
    return c === 100 ? '完全合规' : c >= 67 ? '基本合规' : c >= 34 ? '部分合规' : '合规缺失';
  }
  static label(d: ComplianceDomain): string {
    const m: Record<ComplianceDomain, string> = {
      'local-data': '数据本地',
      consent: '知情同意',
      'min-collect': '最小收集',
      deletable: '可删除',
      'audit-trail': '审计留痕',
      'cross-border': '跨境评估',
    };
    return m[d];
  }
}

/* -------- 族0387 安全无障碍（X09651~X09675）-------- */

/** 安全提示的可等价通道：视觉 / 听觉 / 触觉 / 读屏。 */
export const SEC_A11Y_CHANNELS = ['visual', 'audio', 'haptic', 'screen-reader'] as const;
export type SecA11yChannel = (typeof SEC_A11Y_CHANNELS)[number];

/** 安全无障碍：告警必须至少两通道等价 + 对比度 + reduce-motion。 */
export class SecA11yAlarm {
  channels: Set<SecA11yChannel> = new Set(['visual']);
  reduceMotion = false;
  contrast = 4.5;
  clamped = 0;
  /** 启用通道：未知通道钳制拒绝。 */
  enable(ch: string): boolean {
    if (!(SEC_A11Y_CHANNELS as readonly string[]).includes(ch)) {
      this.clamped++;
      return false;
    }
    this.channels.add(ch as SecA11yChannel);
    return true;
  }
  /** 等价达标：≥2 通道且对比度 ≥4.5。 */
  compliant(): boolean {
    return this.channels.size >= 2 && this.contrast >= 4.5;
  }
  /** reduce-motion：动效降级为纯淡入淡出（此处为标记位一致性）。 */
  setReduceMotion(on: boolean): boolean {
    this.reduceMotion = on;
    return this.reduceMotion;
  }
  /** 通道标签。 */
  static label(ch: SecA11yChannel): string {
    return { visual: '视觉', audio: '听觉', haptic: '触觉', 'screen-reader': '读屏' }[ch];
  }
}

/* -------- 族0390 安全收官（X09726~X09750）-------- */

/** 收官门禁：G1~G5 顺序门（K 线 4 族 / C 线 4 族 / V 线 5 族 / 三方 3 族 / 双副本对齐）。 */
export const SEC_FINALE_GATES = ['G1-kernel', 'G2-analysis', 'G3-desktop', 'G4-tripartite', 'G5-dual-copy'] as const;
export type SecFinaleGate = (typeof SEC_FINALE_GATES)[number];

/** 安全收官：顺序门禁 + 累计口径 + ID 连续审计。 */
export class SecFinale {
  passed: SecFinaleGate[] = [];
  /** 过门：必须按 G1→G5 顺序，跳门即拒绝。 */
  pass(gate: SecFinaleGate): boolean {
    const idx = SEC_FINALE_GATES.indexOf(gate);
    if (idx !== this.passed.length) return false;
    this.passed.push(gate);
    return true;
  }
  /** 全门通过。 */
  allDone(): boolean {
    return this.passed.length === SEC_FINALE_GATES.length;
  }
  /** 累计口径：本域 1000 项（AI-36~39 · 40 族 × 25）。 */
  static totalScope(): number {
    return 1000;
  }
  /** ID 连续审计：entries 覆盖 [from,to] 且无重复。 */
  static auditIds(ids: string[], from: number, to: number): boolean {
    if (ids.length !== to - from + 1) return false;
    const set = new Set(ids);
    if (set.size !== ids.length) return false;
    for (let x = from; x <= to; x++) {
      if (!set.has(`X${String(x).padStart(5, '0')}`)) return false;
    }
    return true;
  }
  /** 收官纪要。 */
  static memo(gates: number): string {
    return `安全与隐私收官 ${gates}/5 门通过`;
  }
}
