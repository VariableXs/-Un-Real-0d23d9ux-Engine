// AURORA-10000: AI-69 批次（族0341~0345 · 学习入门/教育/职场/老年/儿童无障碍），勿删。

import { clamp, type FeatureSlot } from './types';

/* ============ 族0341 学习与入门（F08501~F08525） ============ */

/** F08501/F08502 自动检测：读屏/系统语言自动启用。 */
export function autoDetect(opts: { screenReaderRunning: boolean; systemLocale: string }): { a11yOn: boolean; locale: string } {
  return { a11yOn: opts.screenReaderRunning, locale: opts.systemLocale };
}

/** F08503/F08504 首次配置与快速设置：步骤推进。 */
export class FirstRunWizard {
  steps = ['语言', '视觉', '听觉', '运动', '认知', '完成'];
  at = 0;
  next(): string | null { return this.at < this.steps.length ? this.steps[this.at++]! : null; }
  get done(): boolean { return this.at >= this.steps.length; }
  get progressPct(): number { return Math.round((this.at / this.steps.length) * 100); }
}

/** F08505/F08506 一键预设与组合。 */
export type PresetKind = 'visual' | 'hearing' | 'motor' | 'cognitive';
export const ONE_CLICK_PRESETS: Record<PresetKind, Record<string, boolean | number | string>> = {
  visual: { magnifier: true, highContrast: true, largeText: 150 },
  hearing: { captions: true, visualBell: true, mono: false },
  motor: { stickyKeys: true, dwell: true, slowKeys: false },
  cognitive: { simplified: true, calm: true, timeouts3x: true },
};
export function combinedPreset(kinds: PresetKind[]): Record<string, boolean | number | string> {
  return kinds.reduce((acc, k) => ({ ...acc, ...ONE_CLICK_PRESETS[k] }), {});
}

/** F08507 预设分享：导出/导入编码（纯本地编码，非命令）。 */
export function presetExport(kinds: PresetKind[]): string {
  return 'vp1:' + kinds.join('+');
}
export function presetImport(code: string): PresetKind[] | null {
  if (!code.startsWith('vp1:')) return null;
  const kinds = code.slice(4).split('+') as PresetKind[];
  return kinds.length > 0 && kinds.every((k) => k in ONE_CLICK_PRESETS) ? kinds : null;
}

/** F08508 检查清单：入门完成度勾选。 */
export class SetupChecklist {
  items: { name: string; done: boolean }[] = [];
  add(name: string): void { this.items.push({ name, done: false }); }
  tick(name: string): boolean { const i = this.items.find((x) => x.name === name); if (!i) return false; i.done = true; return true; }
  get progress(): number { return this.items.length === 0 ? 100 : Math.round((this.items.filter((i) => i.done).length / this.items.length) * 100); }
}

/** F08509 自评问卷：评分引擎。 */
export interface Question { id: string; answerYes: boolean; weight: number }
export function questionnaireScore(qs: Question[]): { score: number; recommends: PresetKind[] } {
  const score = qs.reduce((s, q) => s + (q.answerYes ? q.weight : 0), 0);
  const recommends: PresetKind[] = [];
  if (qs.some((q) => q.id.startsWith('vision') && q.answerYes)) recommends.push('visual');
  if (qs.some((q) => q.id.startsWith('hearing') && q.answerYes)) recommends.push('hearing');
  if (qs.some((q) => q.id.startsWith('motor') && q.answerYes)) recommends.push('motor');
  if (qs.some((q) => q.id.startsWith('cognitive') && q.answerYes)) recommends.push('cognitive');
  return { score, recommends };
}

/** F08510 推荐配置：按评分映射预设强度。 */
export function recommendedConfig(score: number): { preset: PresetKind[]; intensity: 'light' | 'standard' | 'full' } {
  return { preset: score >= 3 ? ['visual', 'hearing', 'motor', 'cognitive'] : score >= 1 ? ['visual'] : [], intensity: score >= 3 ? 'full' : score >= 1 ? 'standard' : 'light' };
}

/** F08511 渐进增强：能力逐级解锁。 */
export function progressiveUnlock(level: number): string[] {
  const caps = ['基础读屏', '放大与对比', '开关扫描', '语音控制'];
  return caps.slice(0, clamp(level, 0, caps.length));
}

/** F08512 视频位（§15 预留）。 */
export const VIDEO_SLOT: FeatureSlot = { id: 'F08512', name: '入门视频', reserved: true, enabled: false };

/** F08513 图文指南：文本配图对。 */
export function illustratedGuide(title: string, icon: string): { title: string; icon: string } {
  return { title, icon };
}

/** F08514 FAQ 问答表。 */
export const ONBOARDING_FAQ = [
  { q: '放大镜怎么开？', a: '设置→无障碍→视觉→放大镜' },
  { q: '字幕在哪里？', a: '设置→无障碍→听觉→环境字幕' },
  { q: '如何恢复默认？', a: '设置→无障碍→重置档案' },
];

/** F08515 社区入口。 */
export const ONBOARDING_COMMUNITY = { forum: 'a11y.onboard', mentors: true } as const;

/** F08516 热线位（§15 预留）。 */
export const HOTLINE_SLOT: FeatureSlot = { id: 'F08516', name: '支持热线', reserved: true, enabled: false };

/** F08517 家庭协助模式：协助者临时授权。 */
export class FamilyAssist {
  granted = false;
  grant(minutes: number): number { this.granted = minutes > 0; return clamp(minutes, 0, 240); }
  revoke(): boolean { this.granted = false; return true; }
}

/** F08518 使用统计：常改设置排行。 */
export function frequentSettings(logs: string[]): [string, number][] {
  const m = new Map<string, number>();
  for (const s of logs) m.set(s, (m.get(s) ?? 0) + 1);
  return [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, 5);
}

/** F08519 入门回归清单。 */
export const ONBOARDING_REGRESSION = ['检测回归', '预设回归', '清单回归'];

/** F08520 入门教学步骤。 */
export const ONBOARDING_TUTORIAL = ['自动检测', '一键预设', '自评问卷', '图文 FAQ', '家庭协助'];

/** F08521 入门彩蛋（低频 + 总控）。 */
export function onboardingEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 67 === 0 ? '欢迎回家 🏠' : null;
}

/** F08522 入门 API 冻结。 */
export interface OnboardingApi { autoDetect(o: { screenReaderRunning: boolean; systemLocale: string }): { a11yOn: boolean; locale: string } }
export const ONBOARDING_API_VERSION = '1.0';

/** F08523 入门文档页。 */
export const ONBOARDING_DOCS = ['快速上手', '预设说明', '家庭协助指南'];

/** F08524 入门收官清单。 */
export const ONBOARDING_FINALE = ['25 项自检', '预留位冻结', 'FAQ 归档', '回归通过', 'API 冻结'];

/** F08525 入门致谢。 */
export const ONBOARDING_THANKS = ['障碍用户试点家庭', '特教顾问', '社区导师'];

/* ============ 族0342 教育无障碍（F08526~F08550） ============ */

/** F08526/F08531 课堂模式与考试模式：受控开关集。 */
export class ClassroomMode {
  active = false;
  exam = false;
  blocked: string[] = [];
  enter(exam: boolean, block: string[]): string[] {
    this.active = true;
    this.exam = exam;
    this.blocked = exam ? block : [];
    return this.blocked;
  }
  exit(): boolean { this.active = false; this.exam = false; this.blocked = []; return true; }
}

/** F08527 字幕广播位（§15 预留）。 */
export const BROADCAST_SLOT: FeatureSlot = { id: 'F08527', name: '课堂字幕广播', reserved: true, enabled: false };

/** F08528 朗读课件：提取标题朗读序列。 */
export function readableCourseware(lines: string[]): string[] {
  return lines.filter((l) => l.trim().length > 0);
}

/** F08529 适配输入：运动课堂的输入策略。 */
export function adaptedInput(kind: 'switch' | 'voice' | 'keyboard'): string {
  return kind === 'switch' ? '扫描输入' : kind === 'voice' ? '语音输入' : '标准键盘';
}

/** F08530 简化界面：课堂可见白名单。 */
export function simplifiedClassroom(all: string[], keep: string[]): string[] {
  return all.filter((o) => keep.includes(o));
}

/** F08532 作业模板注册。 */
export const HOMEWORK_TEMPLATES = ['阅读理解', '数学练习', '听写练习'] as const;

/** F08533 课件检查：标题层级/alt/对比规则。 */
export interface SlideRule { hasTitle: boolean; imagesHaveAlt: boolean; contrastOk: boolean }
export function coursewareCheck(r: SlideRule): { ok: boolean; issues: string[] } {
  const issues: string[] = [];
  if (!r.hasTitle) issues.push('缺标题层级');
  if (!r.imagesHaveAlt) issues.push('图片缺替代文本');
  if (!r.contrastOk) issues.push('对比度不足');
  return { ok: issues.length === 0, issues };
}

/** F08534 学生档案模板。 */
export interface StudentProfile { name: string; needs: PresetKind[]; notes: string }
export function studentTemplate(name: string, needs: PresetKind[]): StudentProfile {
  return { name, needs, notes: '' };
}

/** F08535/F08536 教师与家长指南。 */
export const TEACHER_GUIDE = ['识别需求', '选择预设', '课堂演练', '反馈调整'];
export const PARENT_GUIDE = ['家庭模式入门', '时长与内容管理', '家校同步'];

/** F08537 教a11y 教学步骤。 */
export const EDU_TUTORIAL = ['课堂模式', '课件检查', '学生档案', '适配输入', '考试演练'];

/** F08538 教a11y 审计项。 */
export const EDU_AUDIT = ['考试锁定可靠', '课件零缺 alt', '档案隐私本地', '预留位冻结', '输入全可达'];

/** F08539 教a11y 测试用例。 */
export const EDU_TESTS = ['课堂进入退出', '课件检查器', '模板取用', '简化白名单', '考试封锁'];

/** F08540 教a11y 回归清单。 */
export const EDU_REGRESSION = ['课堂模式回归', '课件回归', '档案回归'];

/** F08541 教a11y 文档页。 */
export const EDU_DOCS = ['教师指南', '家长指南', '课堂模式说明'];

/** F08542 教a11y 彩蛋（低频 + 总控）。 */
export function eduEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 61 === 0 ? '下课铃响啦 🔔' : null;
}

/** F08543 教a11y API 冻结。 */
export interface EduApi { coursewareCheck(r: SlideRule): { ok: boolean; issues: string[] } }
export const EDU_API_VERSION = '1.0';

/** F08544 教a11y 案例。 */
export const EDU_CASES = ['视障学生读屏课堂', '运动障碍扫描考试', '认知简化界面'] as const;

/** F08545 教a11y 收官清单。 */
export const EDU_FINALE = ['25 项自检', '预留位冻结', '案例归档', '回归通过', 'API 冻结'];

/** F08546 教a11y 致谢。 */
export const EDU_THANKS = ['一线特教教师', '试点班级', '家长委员会'];

/** F08547 教a11y 合作位 / F08548 研究位（§15 预留）。 */
export const EDU_SLOTS: FeatureSlot[] = [
  { id: 'F08547', name: '教育机构合作', reserved: true, enabled: false },
  { id: 'F08548', name: '教育研究', reserved: true, enabled: false },
];

/** F08549 教a11y 日历：学期关键日。 */
export const EDU_CALENDAR = ['09-01 开学', '01-15 期末', '07-01 暑假'] as const;

/** F08550 教a11y 二期收官。 */
export const EDU_FINALE_2 = ['二期测试通过', '日历对齐校历', '案例扩充'];

/* ============ 族0343 职场无障碍（F08551~F08575） ============ */

/** F08551 演讲字幕：演示文稿逐句计时。 */
export class PresentationCaptions {
  lines: { text: string; seconds: number }[] = [];
  add(text: string, seconds: number): number { this.lines.push({ text, seconds }); return this.lines.length; }
  schedule(): { text: string; from: number; to: number }[] {
    let t = 0;
    return this.lines.map((l) => { const s = { text: l.text, from: t, to: t + l.seconds }; t += l.seconds; return s; });
  }
  get totalSeconds(): number { return this.lines.reduce((s, l) => s + l.seconds, 0); }
}

/** F08552 会议字幕位（§15 预留）。 */
export const MEETING_CAPTION_SLOT: FeatureSlot = { id: 'F08552', name: '实时会议字幕', reserved: true, enabled: false };

/** F08553 简历模板：无障碍简历结构。 */
export const RESUME_TEMPLATE = ['基本信息', '可胜任岗位', '工作示例', '合理便利需求', '联系方式'] as const;
export function resumeValidate(sections: string[]): { ok: boolean; missing: string[] } {
  const missing = RESUME_TEMPLATE.filter((s) => !sections.includes(s));
  return { ok: missing.length === 0, missing };
}

/** F08554 文档检查：办公模板无障碍规则。 */
export function docA11yCheck(doc: { hasHeadingStyles: boolean; tablesHaveHeader: boolean; linksHaveText: boolean }): { ok: boolean; issues: string[] } {
  const issues: string[] = [];
  if (!doc.hasHeadingStyles) issues.push('缺标题样式');
  if (!doc.tablesHaveHeader) issues.push('表格缺表头');
  if (!doc.linksHaveText) issues.push('链接缺描述文本');
  return { ok: issues.length === 0, issues };
}

/** F08555 纯文本邮件：剥离富文本保语义。 */
export function plainTextEmail(html: string): string {
  return html.replace(/<[^>]+>/g, '').replace(/\s+/g, ' ').trim();
}

/** F08556 提醒强化：日程三重提醒。 */
export function reinforcedReminders(startMinFromNow: number): number[] {
  return [-60, -15, -5].map((m) => clamp(startMinFromNow + m, 0, startMinFromNow));
}

/** F08557 员工档案模板。 */
export interface EmployeeProfile { name: string; needs: PresetKind[]; accommodations: string[] }
export function employeeTemplate(name: string): EmployeeProfile { return { name, needs: [], accommodations: [] }; }

/** F08558/F08559 雇主与员工指南。 */
export const EMPLOYER_GUIDE = ['了解义务', '岗位评估', '提供便利', '定期回顾'];
export const EMPLOYEE_GUIDE = ['了解权利', '提出需求', '试用配置', '反馈效果'];

/** F08560 合理便利建议：按需求映射。 */
export function accommodationSuggest(needs: PresetKind[]): string[] {
  const map: Record<PresetKind, string> = {
    visual: '屏幕阅读与放大设备',
    hearing: '字幕与视觉提醒',
    motor: '替代输入设备与弹性工时',
    cognitive: '任务拆分与书面指引',
  };
  return needs.map((n) => map[n]);
}

/** F08561 职a11y 教学步骤。 */
export const WORK_TUTORIAL = ['演讲字幕', '文档检查', '简历模板', '便利建议', '提醒强化'];

/** F08562 职a11y 审计项。 */
export const EDU_WORK_AUDIT = ['模板零缺项', '检查器零漏报', '档案隐私本地', '预留位冻结', '提醒不缺漏'];

/** F08563 职a11y 测试用例。 */
export const WORK_TESTS = ['字幕排程', '纯文本剥离', '简历校验', '文档检查', '便利映射'];

/** F08564 职a11y 回归清单。 */
export const WORK_REGRESSION = ['字幕回归', '模板回归', '检查器回归'];

/** F08565 职a11y 文档页。 */
export const WORK_DOCS = ['雇主指南', '员工指南', '便利建议说明'];

/** F08566 职a11y 彩蛋（低频 + 总控）。 */
export function workEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 59 === 0 ? '带薪喝口水 💧' : null;
}

/** F08567 职a11y API 冻结。 */
export interface WorkApi { docA11yCheck(d: { hasHeadingStyles: boolean; tablesHaveHeader: boolean; linksHaveText: boolean }): { ok: boolean; issues: string[] } }
export const WORK_API_VERSION = '1.0';

/** F08568 职a11y 案例。 */
export const WORK_CASES = ['听障员工会议字幕', '视障工程师读屏开发', '认知便利任务拆分'] as const;

/** F08569 职a11y 收官清单。 */
export const WORK_FINALE = ['25 项自检', '预留位冻结', '案例归档', '回归通过', 'API 冻结'];

/** F08570 职a11y 致谢。 */
export const WORK_THANKS = ['参与试点的企业', '残障就业服务机构', '员工志愿者'];

/** F08571 职a11y 合作位 / F08572 研究位（§15 预留）。 */
export const WORK_SLOTS: FeatureSlot[] = [
  { id: 'F08571', name: '企业合作', reserved: true, enabled: false },
  { id: 'F08572', name: '职场研究', reserved: true, enabled: false },
];

/** F08573 职a11y 日历。 */
export const WORK_CALENDAR = ['03-01 招聘季', '06-30 评审', '12-31 年结'] as const;

/** F08574 职a11y 二期收官。 */
export const WORK_FINALE_2 = ['二期测试通过', '日历对齐季度', '案例扩充'];

/** F08575 职a11y 实验位（§15 预留）。 */
export const WORK_EXP_SLOT: FeatureSlot = { id: 'F08575', name: '职场实验位', reserved: true, enabled: false };

/* ============ 族0344 老年无障碍（F08576~F08600） ============ */

/** F08576/F08577 一键大字与高对比：返回设置 diff。 */
export function oneClickLargeText(): Record<string, number | boolean> {
  return { fontScale: 150, fontWeight: 600, contrast: true };
}
export function oneClickHighContrast(): Record<string, number | boolean> {
  return { highContrast: true, focusRing: true, reduceTransparency: true };
}

/** F08578/F08579 简化桌面与功能聚合：常用入口白名单。 */
export function simplifiedDesktop(all: string[]): string[] {
  const keep = ['电话', '家人', '拍照', '时钟', '提醒'];
  return all.filter((o) => keep.includes(o));
}

/** F08580 大按钮：触控目标 64px。 */
export function elderButtonSize(): number { return 64; }

/** F08581 语音优先：语音入口置顶。 */
export function voiceFirstOrder(entries: string[]): string[] {
  return ['语音助手', ...entries.filter((e) => e !== '语音助手')];
}

/** F08582 防误触强化：双击确认。 */
export function elderConfirmPolicy(): { doubleConfirm: boolean; minGapMs: number } {
  return { doubleConfirm: true, minGapMs: 500 };
}

/** F08583 慢动画：动效 0.6x。 */
export function elderMotionScale(): number { return 0.6; }

/** F08584 减少弹窗：非关键聚合。 */
export function elderDialogPolicy(critical: boolean, pending: number): 'show' | 'batch' {
  return critical || pending < 2 ? 'show' : 'batch';
}

/** F08585 大音量：上限保护下提升。 */
export function elderVolumeBoost(v: number): number { return clamp(v + 0.2, 0, 0.9); }

/** F08586 点读：点哪读哪。 */
export function tapToRead(targets: string[], idx: number): string | null { return targets[idx] ?? null; }

/** F08587 远程协助位（§15 预留）。 */
export const ELDER_REMOTE_SLOT: FeatureSlot = { id: 'F08587', name: '远程协助', reserved: true, enabled: false };

/** F08588 家人管理：授权家庭成员。 */
export class FamilyManager {
  members: string[] = [];
  add(name: string): boolean { if (this.members.includes(name)) return false; this.members.push(name); return true; }
  canManage(name: string): boolean { return this.members.includes(name); }
}

/** F08589 用药强化提醒：到点三重提示。 */
export function medicationReminder(nowMin: number, atMin: number): { due: boolean; prompts: number } {
  const due = nowMin >= atMin;
  return { due, prompts: due ? 3 : 0 };
}

/** F08590 防诈骗提示：可疑特征评分。 */
export function scamGuard(input: { asksMoney: boolean; urgentTone: boolean; unknownSender: boolean; hasLink: boolean }): { risk: number; warn: boolean } {
  const risk = [input.asksMoney, input.urgentTone, input.unknownSender, input.hasLink].filter(Boolean).length;
  return { risk, warn: risk >= 2 };
}

/** F08591 操作确认：删除类操作长按确认。 */
export function elderDeleteConfirm(holdMs: number): boolean { return holdMs >= 1200; }

/** F08592 简化设置：仅保留必要项。 */
export function simplifiedSettings(all: string[]): string[] {
  return all.filter((s) => ['音量', '亮度', '字体大小'].includes(s));
}

/** F08593 老年教学步骤。 */
export const ELDER_TUTORIAL = ['一键大字', '简化桌面', '大按钮点读', '防诈骗练习', '家人管理'];

/** F08594 老年审计项。 */
export const EDU_ELDER_AUDIT = ['大字不溢出', '确认不缺漏', '音量有上限', '预留位冻结', '诈骗提示可达'];

/** F08595 老年测试用例。 */
export const ELDER_TESTS = ['一键大字', '简化白名单', '点读命中', '防诈骗评分', '长按确认'];

/** F08596 老年回归清单。 */
export const ELDER_REGRESSION = ['大字回归', '桌面回归', '提醒回归'];

/** F08597 老年文档页。 */
export const ELDER_DOCS = ['大字模式说明', '家人管理指南', '防骗手册'];

/** F08598 老年彩蛋（低频 + 总控）。 */
export function elderEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 53 === 0 ? '慢慢来，比较快 🍵' : null;
}

/** F08599 老年收官清单。 */
export const ELDER_FINALE = ['25 项自检', '预留位冻结', '手册归档', '回归通过', '致谢发布'];

/** F08600 老年致谢。 */
export const ELDER_THANKS = ['银发试点用户', '社区养老驿站', '家属志愿者'];

/* ============ 族0345 儿童无障碍（F08601~F08625） ============ */

/** F08601/F08602 儿童大字与语音。 */
export function kidLargeText(): number { return 140; }
export function kidVoicePreset(): { rate: number; voice: 'child' } { return { rate: 0.9, voice: 'child' }; }

/** F08603/F08604 儿童简化与防误触。 */
export function kidSimplified(all: string[]): string[] { return all.filter((s) => ['学习', '画画', '故事'].includes(s)); }
export function kidMistouchPolicy(): { minGapMs: number; bigTargets: boolean } { return { minGapMs: 600, bigTargets: true }; }

/** F08605 时长管理：剩余时间与到期锁定。 */
export class ScreenTime {
  budgetMin: number;
  usedMin = 0;
  constructor(budgetMin: number) { this.budgetMin = budgetMin; }
  use(min: number): void { this.usedMin = Math.min(this.usedMin + min, this.budgetMin); }
  get remaining(): number { return this.budgetMin - this.usedMin; }
  get locked(): boolean { return this.remaining <= 0; }
}

/** F08606 分级过滤：内容适龄。 */
export function kidContentFilter(level: 'all' | 'kid' | 'teen', content: 'all' | 'kid' | 'teen' | 'adult'): boolean {
  const order = ['all', 'kid', 'teen', 'adult'];
  return order.indexOf(content) <= order.indexOf(level);
}

/** F08607 跟读评分：编辑距离相似度。 */
export function readAloudScore(said: string, expected: string): number {
  const m = said.length;
  const n = expected.length;
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array<number>(n + 1).fill(0));
  for (let i = 0; i <= m; i++) dp[i]![0] = i;
  for (let j = 0; j <= n; j++) dp[0]![j] = j;
  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      dp[i]![j] = Math.min(dp[i - 1]![j]! + 1, dp[i]![j - 1]! + 1, dp[i - 1]![j - 1]! + (said[i - 1] === expected[j - 1] ? 0 : 1));
    }
  }
  return Math.round((1 - dp[m]![n]! / Math.max(m, n, 1)) * 100);
}

/** F08608 楷体学习字体。 */
export const KID_FONT = '楷体';

/** F08609 护眼强化：护眼参数档。 */
export function kidEyeCare(): { colorTemp: number; breakEveryMin: number } {
  return { colorTemp: 4000, breakEveryMin: 20 };
}

/** F08610 专注模式：屏蔽干扰项。 */
export function kidFocus(blockers: string[]): { active: boolean; blocked: string[] } {
  return { active: blockers.length > 0, blocked: blockers };
}

/** F08611 游戏化教学：徽章奖励。 */
export class KidBadges {
  earned = new Set<string>();
  earn(name: string): boolean { if (this.earned.has(name)) return false; this.earned.add(name); return true; }
  get count(): number { return this.earned.size; }
}

/** F08612 家长监督：PIN 与报告。 */
export class ParentalControl {
  private pin: string;
  constructor(pin: string) { this.pin = pin; }
  unlock(input: string): boolean { return input === this.pin; }
  weeklyReport(minutes: number): { minutes: number; level: 'low' | 'ok' | 'high' } {
    return { minutes, level: minutes < 300 ? 'low' : minutes <= 600 ? 'ok' : 'high' };
  }
}

/** F08613 儿童教学步骤。 */
export const KID_TUTORIAL = ['大字语音', '简化入口', '时长约定', '跟读练习', '家长监督'];

/** F08614 儿童审计项。 */
export const EDU_KID_AUDIT = ['时长锁定可靠', '分级过滤生效', '档案隐私本地', '预留位冻结', '防误触生效'];

/** F08615 儿童测试用例。 */
export const KID_TESTS = ['时长到期', '跟读评分', '分级过滤', 'PIN 校验', '简化白名单'];

/** F08616 儿童回归清单。 */
export const KID_REGRESSION = ['时长回归', '语音回归', '分级回归'];

/** F08617 儿童文档页。 */
export const KID_DOCS = ['家长设置指南', '跟读说明', '时长管理说明'];

/** F08618 儿童彩蛋（低频 + 总控）。 */
export function kidEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 47 === 0 ? '奖励一颗小星星 ⭐' : null;
}

/** F08619 儿童 API 冻结。 */
export interface KidApi { readAloudScore(said: string, expected: string): number }
export const KID_API_VERSION = '1.0';

/** F08620 儿童案例。 */
export const KID_CASES = ['低龄大字跟读', '阅读障碍楷体模式', '多动儿童专注模式'] as const;

/** F08621 儿童收官清单。 */
export const KID_FINALE = ['25 项自检', '预留位冻结', '案例归档', '回归通过', 'API 冻结'];

/** F08622 儿童致谢。 */
export const KID_THANKS = ['试点学校与家庭', '儿科发育顾问', '绘本作者'];

/** F08623 儿童研究位（§15 预留）。 */
export const KID_RESEARCH_SLOT: FeatureSlot = { id: 'F08623', name: '儿童发展研究', reserved: true, enabled: false };

/** F08624 儿童日历。 */
export const KID_CALENDAR = ['02-28 学期初', '06-01 儿童节', '09-01 新学年'] as const;

/** F08625 儿童二期收官。 */
export const KID_FINALE_2 = ['二期测试通过', '日历对齐校历', '案例扩充'];
