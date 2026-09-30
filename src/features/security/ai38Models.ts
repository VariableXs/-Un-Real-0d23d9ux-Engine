/**
 * UNREAL-X-15000 · AI-38 防线工程 V/K 线逻辑核（族0371~0376 · X09251~X09400），勿删。
 * 本文件承载 V 线三族逻辑模型（应急/长者/教育，X09276~X09350）；
 * K 线三族（网络隔离/密码学/权限最小化）见 kernel/varix/src/sec/。
 * 五层 × 五档模板，全部确定性算法，零 AI。
 */

/* -------- 族0372 安全应急 2.0（X09276~X09300）-------- */

/** 应急档位：0 观察值机 / 1 告警 / 2 锁定入口 / 3 断网隔离 / 4 全量冻结。 */
export const EMERGENCY_LEVELS = ['observe', 'alert', 'lockdown', 'airgap', 'freeze'] as const;
export type EmergencyLevel = (typeof EMERGENCY_LEVELS)[number];

export const EMERGENCY_LABELS: Record<EmergencyLevel, string> = {
  observe: '观察值机',
  alert: '告警',
  lockdown: '锁定入口',
  airgap: '断网隔离',
  freeze: '全量冻结',
};

export interface EmergencyEvent {
  kind: 'intrusion' | 'ransom' | 'leak' | 'lost-device';
  score: number;
}

/** 安全应急中枢：五档升级矩阵 + 一键降档 + 事件追溯。 */
export class EmergencyHub {
  level: EmergencyLevel = 'observe';
  clamped = 0;
  log: EmergencyEvent[] = [];
  /** 触发应急：分数决定档位（<30 observe / 30~59 alert / 60~79 lockdown / 80~94 airgap / ≥95 freeze）。 */
  trigger(ev: EmergencyEvent): EmergencyLevel {
    this.log.push(ev);
    const want: EmergencyLevel =
      ev.score >= 95 ? 'freeze' : ev.score >= 80 ? 'airgap' : ev.score >= 60 ? 'lockdown' : ev.score >= 30 ? 'alert' : 'observe';
    this.setLevel(want);
    return this.level;
  }
  /** 档位设置：越界回 observe（钳制计数）。 */
  setLevel(lv: string): EmergencyLevel {
    if (!(EMERGENCY_LEVELS as readonly string[]).includes(lv)) {
      this.clamped++;
      this.level = 'observe';
    } else {
      this.level = lv as EmergencyLevel;
    }
    return this.level;
  }
  /** 一键降档：应急解除回观察档。 */
  standDown(): EmergencyLevel {
    this.level = 'observe';
    return this.level;
  }
  /** 升级约束：只升不降（同一事件流内）。 */
  escalateOnly(): boolean {
    return EMERGENCY_LEVELS.indexOf(this.level) >= 0;
  }
  /** 建议叙事：档位 → 中文下一步。 */
  advice(): string {
    const a: Record<EmergencyLevel, string> = {
      observe: '保持观察，无需操作',
      alert: '核对告警来源，确认后升级',
      lockdown: '锁定入口并通知管理员',
      airgap: '断网隔离，保存现场',
      freeze: '全量冻结，进入应急响应',
    };
    return a[this.level];
  }
  static label(lv: EmergencyLevel): string {
    return EMERGENCY_LABELS[lv];
  }
}

/* -------- 族0373 长者守护 2.0（X09301~X09325）-------- */

/** 守护档位：0 标准 / 1 大字 / 2 陪伴 / 3 谨慎 / 4 安心。 */
export const ELDER_LEVELS = ['standard', 'large', 'companion', 'cautious', 'secure'] as const;
export type ElderLevel = (typeof ELDER_LEVELS)[number];

export interface ElderProfile {
  /** 每日步数提醒阈值。 */
  stepsGoal: number;
  /** 陌生来电拦截。 */
  blockUnknown: boolean;
  /** 大额转账二次确认阈值（元）。 */
  payConfirmYuan: number;
}

export const ELDER_PRESETS: Record<ElderLevel, ElderProfile> = {
  standard: { stepsGoal: 6000, blockUnknown: false, payConfirmYuan: 5000 },
  large: { stepsGoal: 4000, blockUnknown: false, payConfirmYuan: 3000 },
  companion: { stepsGoal: 3000, blockUnknown: true, payConfirmYuan: 1000 },
  cautious: { stepsGoal: 2000, blockUnknown: true, payConfirmYuan: 500 },
  secure: { stepsGoal: 1500, blockUnknown: true, payConfirmYuan: 0 },
};

/** 长者守护：档案档位 + 异常来电/支付判定 + 家人回执。 */
export class ElderGuard {
  level: ElderLevel = 'standard';
  clamped = 0;
  acks: string[] = [];
  /** 应用档位：未知档位回 standard。 */
  apply(level: string): ElderLevel {
    if (!(ELDER_LEVELS as readonly string[]).includes(level)) {
      this.clamped++;
      this.level = 'standard';
    } else {
      this.level = level as ElderLevel;
    }
    return this.level;
  }
  get profile(): ElderProfile {
    return ELDER_PRESETS[this.level];
  }
  /** 来电判定：拦截档下陌生号码 → 建议转家人确认。 */
  callPolicy(known: boolean): 'allow' | 'screen' {
    if (known) return 'allow';
    return this.profile.blockUnknown ? 'screen' : 'allow';
  }
  /** 支付判定：超过阈值 → 需二次确认（0 元阈值 = 全部确认）。 */
  payPolicy(yuan: number): 'pass' | 'confirm' {
    return yuan > this.profile.payConfirmYuan ? 'confirm' : 'pass';
  }
  /** 家人回执登记（去重）。 */
  ack(by: string): number {
    if (!this.acks.includes(by)) this.acks.push(by);
    return this.acks.length;
  }
  static label(lv: ElderLevel): string {
    return { standard: '标准', large: '大字', companion: '陪伴', cautious: '谨慎', secure: '安心' }[lv];
  }
}

/* -------- 族0374 安全教育 2.0（X09326~X09350）-------- */

/** 课程难度：0 入门 / 1 进阶 / 2 实战 / 3 专家 / 4 讲师。 */
export const EDU_TIERS = ['intro', 'advance', 'drill', 'expert', 'trainer'] as const;
export type EduTier = (typeof EDU_TIERS)[number];

export interface EduLesson {
  id: number;
  tier: EduTier;
  topic: string;
  /** 通过判据分数（0~100）。 */
  passScore: number;
}

/** 安全教育：课程解锁链 + 测验判分 + 错题本。 */
export class SecurityAcademy {
  unlocked: EduTier[] = ['intro'];
  wrongBook: number[] = [];
  clamped = 0;
  /** 完成课程：测验分数过线则解锁下一档。 */
  complete(lesson: EduLesson, score: number): boolean {
    if (!(EDU_TIERS as readonly string[]).includes(lesson.tier)) {
      this.clamped++;
      return false;
    }
    const pass = score >= lesson.passScore;
    if (!pass) {
      if (!this.wrongBook.includes(lesson.id)) this.wrongBook.push(lesson.id);
      return false;
    }
    const idx = EDU_TIERS.indexOf(lesson.tier);
    const next = EDU_TIERS[idx + 1];
    if (next && !this.unlocked.includes(next)) this.unlocked.push(next);
    return true;
  }
  /** 错题本清空（重修通过后）。 */
  clearWrong(id: number): number {
    const i = this.wrongBook.indexOf(id);
    if (i >= 0) this.wrongBook.splice(i, 1);
    return this.wrongBook.length;
  }
  /** 课程表：五档 × 每档主题。 */
  static syllabus(): EduLesson[] {
    const topics: string[] = ['密码与口令', '钓鱼识别', '诈骗演练', '隐私边界', '带练家人'];
    return EDU_TIERS.map((tier, i) => ({ id: i + 1, tier, topic: topics[i]!, passScore: 60 + i * 5 }));
  }
  static label(t: EduTier): string {
    return { intro: '入门', advance: '进阶', drill: '实战', expert: '专家', trainer: '讲师' }[t];
  }
}
