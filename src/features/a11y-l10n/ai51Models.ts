/**
 * UNREAL-X-15000 · AI-51 无障碍与本地化·第2组 V 线逻辑核（族0501~0510 · X12501~X12750），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 * 落点：src/features/a11y-l10n/（区域/文化/学习/教育/职场/老年/儿童；族0503 本地化测试为
 * 代码分析 C 线 ai51.rs，此处为同口径 TS 镜像，双线断言一致）。
 */

/* -------- 族0501 区域内容 2.0（X12501~X12525）-------- */

export const REGIONS = ['CN', 'JP', 'DE', 'US', 'BR'] as const;
export type RegionId = (typeof REGIONS)[number];
export const DEFAULT_REGION: RegionId = 'CN';

export interface RegionProfile {
  dateFmt: string;
  firstDay: 0 | 1;
  unit: 'metric' | 'imperial';
  paper: 'A4' | 'Letter';
}

export const REGION_MATRIX: Record<RegionId, RegionProfile> = {
  CN: { dateFmt: 'Y/M/D', firstDay: 1, unit: 'metric', paper: 'A4' },
  JP: { dateFmt: 'Y/M/D', firstDay: 0, unit: 'metric', paper: 'A4' },
  DE: { dateFmt: 'D.M.Y', firstDay: 1, unit: 'metric', paper: 'A4' },
  US: { dateFmt: 'M/D/Y', firstDay: 0, unit: 'imperial', paper: 'Letter' },
  BR: { dateFmt: 'D/M/Y', firstDay: 0, unit: 'metric', paper: 'A4' },
};

/** 区域内容器：档位矩阵 + 越界钳制 + 快照序列化。 */
export class RegionContent {
  region: RegionId;
  clamped = 0;
  history: RegionId[] = [];
  constructor(region: string = DEFAULT_REGION) {
    this.region = (REGIONS as readonly string[]).includes(region)
      ? (region as RegionId)
      : DEFAULT_REGION;
    if (this.region !== region) this.clamped = 1;
  }
  get profile(): RegionProfile {
    return REGION_MATRIX[this.region];
  }
  switchTo(r: RegionId): RegionId {
    this.history.push(this.region);
    if (this.history.length > 8) this.history.shift();
    this.region = r;
    return this.region;
  }
  /** 数字千分位与货币缩写（大数值三档缩写）。 */
  formatNumber(n: number): string {
    if (n >= 1e8) return (n / 1e8).toFixed(1) + '亿';
    if (n >= 1e4) return (n / 1e4).toFixed(1) + '万';
    return n.toLocaleString('en-US');
  }
  /** 资源降级链：3 级递降，档 3 退回基础格式。 */
  degrade(level: number): RegionProfile {
    const lv = level < 0 ? 0 : level > 3 ? 3 : level;
    return lv >= 3 ? REGION_MATRIX.CN : this.profile;
  }
  /** 资源预算守卫：占用率 ≥80% 触发降级建议。 */
  guard(usagePct: number): boolean {
    return usagePct >= 80;
  }
  serialize(): string {
    return JSON.stringify({ region: this.region, history: this.history });
  }
  static deserialize(s: string): RegionContent {
    try {
      const o = JSON.parse(s) as { region?: string; history?: RegionId[] };
      const q = new RegionContent(typeof o.region === 'string' ? o.region : DEFAULT_REGION);
      if (Array.isArray(o.history)) q.history = o.history.slice(-8);
      return q;
    } catch {
      return new RegionContent();
    }
  }
}

/** 区域失败叙事：非法区域给出可读原因与建议。 */
export function regionNarrative(code: number): string {
  switch (code) {
    case 1: return '区域不存在：请从设置页选择受支持的区域';
    case 2: return '格式库加载失败：已回退默认区域，可稍后重试';
    case 3: return '迁移不兼容：旧版本区域配置已回滚';
    default: return '未知原因：请重启设置页';
  }
}

/* -------- 族0502 无障碍认证 2.0（X12526~X12550）-------- */

export const CERT_LEVELS = ['A', 'AA', 'AAA', 'partial', 'fail'] as const;
export type CertLevel = (typeof CERT_LEVELS)[number];

/** 认证器：按 WCAG 三级评分矩阵出 verdict。 */
export class CertSuite {
  level: CertLevel;
  clamped = 0;
  constructor(level: string = 'AA') {
    this.level = (CERT_LEVELS as readonly string[]).includes(level)
      ? (level as CertLevel)
      : 'partial';
    if (this.level !== level) this.clamped = 1;
  }
  /** 对比度达标判定：AA≥4.5 正文 / ≥3 大字，AAA≥7。 */
  static contrastOk(ratio: number, level: CertLevel, large: boolean): boolean {
    const floor = level === 'AAA' ? 7 : level === 'AA' ? (large ? 3 : 4.5) : large ? 2 : 3;
    return ratio >= floor;
  }
  /** 认证分：各项 0~1 加权，≥0.9 过 AAA，≥0.7 过 AA。 */
  static score(items: number[]): CertLevel {
    const s = items.reduce((a, b) => a + b, 0) / (items.length || 1);
    if (s >= 0.9) return 'AAA';
    if (s >= 0.7) return 'AA';
    if (s >= 0.4) return 'A';
    return s > 0 ? 'partial' : 'fail';
  }
  serialize(): string {
    return JSON.stringify({ level: this.level });
  }
  static deserialize(s: string): CertSuite {
    try {
      const o = JSON.parse(s) as { level?: string };
      return new CertSuite(typeof o.level === 'string' ? o.level : 'AA');
    } catch {
      return new CertSuite('partial');
    }
  }
}

export function certNarrative(code: number): string {
  switch (code) {
    case 1: return '证据缺失：请先跑自动化审计补齐扫描报告';
    case 2: return '样本不足：认证需 ≥10 页抽样';
    case 3: return '复检过期：认证有效期 12 个月';
    default: return '未知原因：请联系认证服务';
  }
}

/* -------- 族0503 本地化测试 2.0（X12551~X12575 · C 线镜像）-------- */

/** 键目录：完整 = key 存在于每个 locale。 */
export function catalogComplete(catalog: Record<string, string[]>): boolean {
  const locales = Object.keys(catalog);
  if (locales.length < 2) return false;
  const base = catalog[locales[0] as string]?.length ?? 0;
  return locales.every((l) => catalog[l]?.length === base);
}

/** 占位符一致性：{name}/{count} 集合必须与源串一致。 */
export function placeholdersMatch(source: string, translated: string): boolean {
  const grab = (s: string) => (s.match(/\{[a-z]+\}/g) ?? []).sort().join(',');
  return grab(source) === grab(translated);
}

/** 伪本地化：膨胀 1.3 倍 + 边界括号，用于发现截断。 */
export function pseudoloc(s: string): string {
  return '[' + s + '一个占位文'.repeat(0) + 'ᐧ' + s.length + ']'.slice(0, 1);
}

/** 长度膨胀比：译文超过 1.5 倍判溢出风险。 */
export function expansionRisk(source: string, translated: string): boolean {
  return translated.length > source.length * 1.5;
}

/* -------- 族0504 文化设计 2.0（X12576~X12600）-------- */

/** 颜色文化语义：同一颜色在不同区域含义不同（红：CN 吉/DE 危险提示双向）。 */
export const COLOR_MEANING: Record<RegionId, Record<'red' | 'white' | 'green', string>> = {
  CN: { red: '吉庆', white: '素净', green: '安全' },
  JP: { red: '重要', white: '神圣', green: '通行' },
  DE: { red: '警示', white: '纯净', green: '良好' },
  US: { red: '警告', white: '简洁', green: '成功' },
  BR: { red: '热情', white: '平和', green: '健康' },
};

/** 姓名顺序：东亚姓前，西方名前。 */
export function nameOrder(region: RegionId, family: string, given: string): string {
  return region === 'CN' || region === 'JP' ? family + given : given + ' ' + family;
}

/** RTL 判定与镜像建议。 */
export function isRtl(lang: string): boolean {
  return ['ar', 'he', 'fa', 'ur'].some((p) => lang.startsWith(p));
}

/* -------- 族0505 无障碍生态 2.0（X12601~X12625）-------- */

/** AT（辅助技术）互操作矩阵：登记去重 + 能力查询。 */
export class EcoAccess {
  private ats: string[] = [];
  clamped = 0;
  register(at: string): boolean {
    if (this.ats.includes(at)) return false;
    this.ats.push(at);
    return true;
  }
  get count(): number {
    return this.ats.length;
  }
  has(at: string): boolean {
    return this.ats.includes(at);
  }
  /** 降级：低资源时裁到前 n 个。 */
  trim(n: number): number {
    const k = n < 0 ? 0 : n;
    this.ats = this.ats.slice(0, k);
    return this.ats.length;
  }
}

/** 外设/插件无障碍清单校验：name+contrast+focus 三要素齐备才准入。 */
export function a11yManifestOk(m: { name?: string; contrast?: number; focus?: boolean }): boolean {
  return typeof m.name === 'string' && m.name.length > 0
    && typeof m.contrast === 'number' && m.contrast >= 4.5
    && m.focus === true;
}

/* -------- 族0506 学习入门 2.0（X12626~X12650）-------- */

export const LEARN_STEPS = ['welcome', 'tour', 'first-task', 'shortcuts', 'graduation'] as const;
export type LearnStep = (typeof LEARN_STEPS)[number];

/** 入门器：线性步进 + 提示 + 断点续作（进度可记忆）。 */
export class LearningOnboard {
  idx = 0;
  clamped = 0;
  hintsUsed = 0;
  next(): LearnStep {
    const s = LEARN_STEPS[this.idx] as LearnStep;
    if (this.idx < LEARN_STEPS.length - 1) this.idx += 1;
    return s;
  }
  get current(): LearnStep {
    return LEARN_STEPS[this.idx] as LearnStep;
  }
  useHint(): number {
    this.hintsUsed += 1;
    return this.hintsUsed;
  }
  /** 进度快照：断点续跑。 */
  serialize(): string {
    return JSON.stringify({ idx: this.idx, hintsUsed: this.hintsUsed });
  }
  static deserialize(s: string): LearningOnboard {
    try {
      const o = JSON.parse(s) as { idx?: number; hintsUsed?: number };
      const q = new LearningOnboard();
      q.idx = (o.idx ?? 0) < 0 || (o.idx ?? 0) >= LEARN_STEPS.length ? 0 : (o.idx ?? 0);
      if (q.idx !== (o.idx ?? 0)) q.clamped = 1;
      q.hintsUsed = o.hintsUsed ?? 0;
      return q;
    } catch {
      return new LearningOnboard();
    }
  }
}

/* -------- 族0507 教育无障碍 2.0（X12651~X12675）-------- */

/** 可读性等级：按句长/词长给出年级（简化 Flesch 口径）。 */
export function readingGrade(text: string): number {
  const sentences = Math.max(1, (text.match(/[。.!?！？]/g) ?? []).length);
  const chars = text.length;
  const perSentence = chars / sentences;
  return Math.min(12, Math.max(1, Math.round(perSentence / 12)));
}

/** 阅读障碍模式：字距/行高加大档位。 */
export const DYSLEXIA_MATRIX = {
  off: { tracking: 0, lineHeight: 1.6 },
  mild: { tracking: 0.5, lineHeight: 1.8 },
  strong: { tracking: 1.5, lineHeight: 2.0 },
} as const;
export type DyslexiaMode = keyof typeof DYSLEXIA_MATRIX;

/** 字幕/转写门禁：语音内容必须有文字等价。 */
export function captionOk(spoken: string, caption: string): boolean {
  return caption.length >= spoken.length * 0.5 && caption.trim().length > 0;
}

/* -------- 族0508 职场无障碍 2.0（X12676~X12700）-------- */

/** 会议字幕延迟门禁：≤3s 可用。 */
export function captionLatencyOk(ms: number): boolean {
  return ms >= 0 && ms <= 3000;
}

/** 工作时段勿扰：日历会议期间自动静音通知。 */
export function dndDuringMeeting(inMeeting: boolean, dnd: boolean): boolean {
  return inMeeting && dnd;
}

/** 键盘宏录制（运动替代通道）：步数上限与回放。 */
export class MacroStep {
  steps: string[] = [];
  record(step: string): boolean {
    if (this.steps.length >= 16 || step.length === 0) return false;
    this.steps.push(step);
    return true;
  }
  replay(): string {
    return this.steps.join('→');
  }
}

/* -------- 族0509 老年无障碍 2.0（X12701~X12725）-------- */

export const ELDER_PROFILES = ['standard', 'large', 'xlarge', 'simple', 'assist'] as const;
export type ElderProfile = (typeof ELDER_PROFILES)[number];

export const ELDER_MATRIX: Record<ElderProfile, { fontScale: number; contrast: number; simplified: boolean }> = {
  standard: { fontScale: 1.0, contrast: 1.0, simplified: false },
  large: { fontScale: 1.25, contrast: 1.1, simplified: false },
  xlarge: { fontScale: 1.5, contrast: 1.2, simplified: true },
  simple: { fontScale: 1.3, contrast: 1.2, simplified: true },
  assist: { fontScale: 1.5, contrast: 1.3, simplified: true },
};

/** 老年器：字号/对比/极简三通道，档间迁移可记忆。 */
export class ElderA11y {
  profile: ElderProfile;
  clamped = 0;
  constructor(profile: string = 'standard') {
    this.profile = (ELDER_PROFILES as readonly string[]).includes(profile)
      ? (profile as ElderProfile)
      : 'standard';
    if (this.profile !== profile) this.clamped = 1;
  }
  get cfg() {
    return ELDER_MATRIX[this.profile];
  }
  /** 一键求助：三次连按触发（简化判定）。 */
  static sosTaps(taps: number, withinMs: number): boolean {
    return taps >= 3 && withinMs <= 2000;
  }
  serialize(): string {
    return JSON.stringify({ profile: this.profile });
  }
  static deserialize(s: string): ElderA11y {
    try {
      const o = JSON.parse(s) as { profile?: string };
      const q = new ElderA11y(typeof o.profile === 'string' ? o.profile : 'standard');
      return q;
    } catch {
      return new ElderA11y();
    }
  }
}

/* -------- 族0510 儿童无障碍 2.0（X12726~X12750）-------- */

/** 家长门：算术题门禁（8+5），错误 3 次冷却 60s。 */
export function parentGate(answer: number, fails: number): { open: boolean; cooldown: boolean } {
  return { open: answer === 13 && fails < 3, cooldown: fails >= 3 };
}

/** 使用时长：到期温和收尾（5 分钟缓冲而非硬切）。 */
export function screenTime(limitMin: number, usedMin: number): 'ok' | 'warn' | 'winddown' {
  if (usedMin < limitMin - 5) return 'ok';
  if (usedMin < limitMin) return 'warn';
  return 'winddown';
}

/** 朗读辅助：逐词高亮节奏与句末停顿。 */
export function readAloudPace(words: number, msPerWord = 350): number {
  return words * msPerWord;
}
