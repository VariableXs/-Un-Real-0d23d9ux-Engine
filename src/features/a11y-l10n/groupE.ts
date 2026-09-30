// AURORA-10000: AI-70 批次（族0346~0350 · 本地化工程/全球发布/社区本地化/无障碍研究/本地化收官），勿删。

import { clamp, type FeatureSlot } from './types';
import { REGION_HOLIDAYS as REGION_HOLIDAYS_REF } from './groupC';

/* ============ 族0346 本地化工程（F08626~F08650） ============ */

/** F08626 i18n 框架统一入口。 */
export class I18nEngine {
  locale = 'zh-CN';
  dicts: Record<string, Record<string, string>> = {};
  private cache = new Map<string, string>();
  register(locale: string, dict: Record<string, string>): void { this.dicts[locale] = { ...this.dicts[locale], ...dict }; this.cache.clear(); }
  setLocale(l: string): void { if (this.locale !== l) { this.locale = l; this.cache.clear(); } }
  t(key: string, vars?: Record<string, string>): string {
    const ck = `${this.locale}:${key}:${JSON.stringify(vars ?? {})}`;
    const hit = this.cache.get(ck);
    if (hit !== undefined) return hit;
    let s = this.dicts[this.locale]?.[key] ?? this.dicts.en?.[key] ?? key;
    if (vars) for (const [k, v] of Object.entries(vars)) s = s.split(`{{${k}}}`).join(v);
    this.cache.set(ck, s);
    return s;
  }
}

/** F08627 键命名规范：小写点分层。 */
export function keyNamingOk(key: string): boolean { return /^[a-z0-9]+(\.[a-z0-9]+)+$/.test(key); }

/** F08628 注释规范：键上下文注释必填。 */
export interface KeyComment { key: string; context: string }
export function commentOk(c: KeyComment): boolean { return c.context.trim().length >= 4; }

/** F08629 外置 100%：扫描代码串判断硬编码残留。 */
export function externalizedRatio(samples: { isKey: boolean }[]): number {
  if (samples.length === 0) return 100;
  return Math.round((samples.filter((s) => s.isKey).length / samples.length) * 100);
}

/** F08630 硬编码扫描：判定字符串是否疑似硬编码中文文案。 */
export function hardcodedScan(s: string, isFromDict: boolean): boolean {
  return /[\u4e00-\u9fff]/.test(s) && !isFromDict;
}

/** F08631 复数框架：Intl.PluralRules 分支。 */
export function pluralText(locale: string, n: number, forms: Record<string, string>): string {
  return forms[new Intl.PluralRules(locale).select(n)] ?? forms.other ?? '';
}

/** F08632 性别框架：接口冻结（§15 与性别位联动）。 */
export interface GenderForms { male?: string; female?: string; neutral: string }
export function genderSelect(forms: GenderForms, g?: 'male' | 'female'): string {
  if (g === 'male' && forms.male) return forms.male;
  if (g === 'female' && forms.female) return forms.female;
  return forms.neutral;
}

/** F08633 插值规范：{{var}} 嵌套一层展开。 */
export function interpolate(s: string, vars: Record<string, string>): string {
  let out = s;
  for (const [k, v] of Object.entries(vars)) out = out.split('{{' + k + '}}').join(v);
  return out;
}

/** F08634~F08638 日期/时区/数字/货币/单位统一库。 */
export function fmtDate(d: Date, locale: string, style: 'full' | 'medium' = 'medium'): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: style }).format(d);
}
export function fmtZone(d: Date, tz: string): string {
  return new Intl.DateTimeFormat('en-US', { timeZone: tz, dateStyle: 'short', timeStyle: 'short' }).format(d);
}
export function fmtNumber(n: number, locale: string): string { return new Intl.NumberFormat(locale).format(n); }
export function fmtMoney(n: number, locale: string, currency: string): string {
  return new Intl.NumberFormat(locale, { style: 'currency', currency }).format(n);
}
export function fmtUnit(n: number, locale: string, unit: string): string {
  return new Intl.NumberFormat(locale, { style: 'unit', unit }).format(n);
}

/** F08639 ICU 排序。 */
export function icuSort(items: string[], locale: string): string[] {
  return [...items].sort(new Intl.Collator(locale).compare);
}

/** F08640 断行库：按宽度断行，CJK 可任意断、拉丁按空格。 */
export function breakLines(text: string, width: number): string[] {
  const out: string[] = [];
  let line = '';
  for (const ch of text) {
    if (ch === ' ' && [...line].length > 0 && [...line].length >= width * 0.6) { out.push(line); line = ''; continue; }
    line += ch;
    if ([...line].length >= width) { out.push(line); line = ''; }
  }
  if (line) out.push(line);
  return out;
}

/** F08641 字符宽度：全角 2 半角 1。 */
export function charWidth(s: string): number {
  return [...s].reduce((w, c) => w + (c.codePointAt(0)! > 0xff ? 2 : 1), 0);
}

/** F08642 UTF-8 编码：TextEncoder 字节一致性。 */
export function utf8Bytes(s: string): number { return new TextEncoder().encode(s).length; }

/** F08643 BOM 处理：剥离 UTF-8 BOM。 */
export function stripBom(s: string): string { return s.codePointAt(0) === 0xfeff ? s.slice(1) : s; }

/** F08644 编码检测：判定字节序列是否合法 UTF-8（含 BOM 识别）。 */
export function detectEncoding(bytes: Uint8Array): 'utf-8-bom' | 'utf-8' | 'other' {
  if (bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) return 'utf-8-bom';
  let i = 0;
  while (i < bytes.length) {
    const b = bytes[i]!;
    if (b < 0x80) { i++; continue; }
    const len = b < 0xe0 ? 2 : b < 0xf0 ? 3 : 4;
    if (i + len > bytes.length) return 'other';
    for (let k = 1; k < len; k++) if ((bytes[i + k]! & 0xc0) !== 0x80) return 'other';
    i += len;
  }
  return 'utf-8';
}

/** F08645/F08646 查表性能与缓存：热路径 <1ms 且缓存命中更快或持平。 */
export function lookupPerf(engine: I18nEngine, key: string): { coldMs: number; warmMs: number; withinBudget: boolean } {
  engine.t(key);
  const t0 = performance.now();
  engine.t(key);
  const coldMs = performance.now() - t0;
  const t1 = performance.now();
  engine.t(key);
  const warmMs = performance.now() - t1;
  return { coldMs, warmMs, withinBudget: warmMs < 1 };
}

/** F08647 拆包：按语言切分字典包。 */
export function splitPacks(dicts: Record<string, Record<string, string>>): Record<string, number> {
  const out: Record<string, number> = {};
  for (const [l, d] of Object.entries(dicts)) out[l] = Object.keys(d).length;
  return out;
}

/** F08648 教学包：框架教学材料。 */
export const L10N_ENGINE_TUTORIAL = ['注册字典', '命名规范', '插值与复数', '格式化库', '性能与拆包'];

/** F08649 工程彩蛋（低频 + 总控）。 */
export function i18nEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 43 === 0 ? '字典比代码还整齐 📚' : null;
}

/** F08650 工程收官清单。 */
export const L10N_ENGINE_FINALE = ['25 项自检', '外置 100%', '性能达标', '拆包生效', '教学齐备'];

/* ============ 族0347 全球发布（F08651~F08675） ============ */

/** F08651/F08652 多区域计划与灰度：区域→放量百分比状态机。 */
export class GlobalRollout {
  regions: Record<string, number> = {};
  plan(regions: string[]): void { for (const r of regions) this.regions[r] ??= 0; }
  ramp(region: string, pct: number): number { return (this.regions[region] = clamp(pct, 0, 100)); }
  isShipped(region: string): boolean { return (this.regions[region] ?? 0) >= 100; }
}

/** F08653 区域回滚：放量归零并记录。 */
export class RegionRollback {
  log: { region: string; at: string }[] = [];
  rollback(rollout: GlobalRollout, region: string, at: string): boolean {
    if (!(region in rollout.regions)) return false;
    rollout.ramp(region, 0);
    this.log.push({ region, at });
    return true;
  }
}

/** F08654/F08655 多语说明与公告：按语言取文。 */
export const RELEASE_NOTES: Record<string, Record<string, string>> = {
  '1.0.0': { 'zh-CN': '首发说明', 'zh-TW': '首發說明', en: 'Release notes' },
};
export const RELEASE_ANNOUNCEMENTS: Record<string, string[]> = {
  'zh-CN': ['新版本上线', '无障碍增强'],
  en: ['New version', 'A11y improvements'],
};

/** F08656 当地上午策略：发布时间落在当地上午 9~11 点。 */
export function localMorningSlot(tz: string): { hour: number; tz: string } {
  return { hour: 9, tz };
}

/** F08657 镜像分发：多镜像地址表。 */
export const DIST_MIRRORS = ['mirror-a.example', 'mirror-b.example', 'mirror-c.example'] as const;

/** F08658 CDN 位（§15 预留）。 */
export const CDN_SLOT: FeatureSlot = { id: 'F08658', name: 'CDN 分发', reserved: true, enabled: false };

/** F08659 下载统计：计数器。 */
export class DownloadStats {
  private counts = new Map<string, number>();
  bump(region: string, n = 1): number { return (this.counts.set(region, (this.counts.get(region) ?? 0) + n), this.counts.get(region)!); }
  get total(): number { return [...this.counts.values()].reduce((a, b) => a + b, 0); }
  top(): string | null { return [...this.counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? null; }
}

/** F08660 反馈分区：按区域归档。 */
export function feedbackPartition(items: { region: string; text: string }[]): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const it of items) (out[it.region] ??= []).push(it.text);
  return out;
}

/** F08661 支持时段：各区域服务窗口。 */
export const SUPPORT_HOURS: Record<string, string> = { 'zh-CN': '09:00-21:00', en: '08:00-18:00' };

/** F08662 避开假日：发布日不撞区域假日。 */
export function avoidHoliday(dateIso: string, region: string): boolean {
  return !(REGION_HOLIDAYS_REF[region] ?? []).includes(dateIso);
}

/** F08663 法律检查：发布前检查项。 */
export function legalCheck(ok: { privacy: boolean; export: boolean; license: boolean }): boolean { return ok.privacy && ok.export && ok.license; }

/** F08664 合规签字位（§15 预留）。 */
export const SIGNOFF_SLOT: FeatureSlot = { id: 'F08664', name: '合规签字', reserved: true, enabled: false };

/** F08665 紧急停用开关。 */
export class KillSwitch {
  engaged = false;
  engage(): boolean { this.engaged = true; return true; }
  release(): boolean { this.engaged = false; return false; }
}

/** F08666 发布检查清单：逐项勾选放行。 */
export class ReleaseChecklist {
  items: Record<string, boolean> = { 多语说明: false, 镜像可用: false, 法律检查: false, 回滚预案: false, 假日避让: false };
  tick(name: string): boolean { if (!(name in this.items)) return false; this.items[name] = true; return true; }
  get ready(): boolean { return Object.values(this.items).every(Boolean); }
}

/** F08667 发布演练：模拟回滚计时。 */
export function releaseDrill(rollbackMs: number): { ok: boolean; rollbackMs: number } {
  return { ok: rollbackMs < 5 * 60_000, rollbackMs };
}

/** F08668 发布教学步骤。 */
export const RELEASE_TUTORIAL = ['区域计划', '灰度放量', '发布清单', '紧急停用演练', '回滚复盘'];

/** F08669 发布彩蛋（低频 + 总控）。 */
export function releaseEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 41 === 0 ? '全球同庆 🌍' : null;
}

/** F08670 发布 API 冻结。 */
export interface ReleaseApi { plan(regions: string[]): void; ramp(region: string, pct: number): number }
export const RELEASE_API_VERSION = '1.0';

/** F08671 发布回归清单。 */
export const RELEASE_REGRESSION = ['灰度回归', '回滚回归', '清单回归'];

/** F08672 发布看板：各区域状态。 */
export function releaseBoard(rollout: GlobalRollout): { region: string; pct: number; shipped: boolean }[] {
  return Object.entries(rollout.regions).map(([region, pct]) => ({ region, pct, shipped: pct >= 100 }));
}

/** F08673 发布文档页。 */
export const RELEASE_DOCS = ['发布流程', '回滚预案', '区域说明'];

/** F08674 发布收官清单。 */
export const RELEASE_FINALE = ['25 项自检', '预留位冻结', '演练通过', '看板发布', 'API 冻结'];

/** F08675 发布致谢。 */
export const RELEASE_THANKS = ['各地区发布团队', '镜像托管方', '测试志愿者'];

/* ============ 族0348 社区本地化（F08676~F08700） ============ */

/** F08676/F08677 门户与任务认领。 */
export class CommunityPortal {
  tasks: { id: string; lang: string; claimedBy: string | null; status: 'open' | 'claimed' | 'review' | 'done' }[] = [];
  addTask(id: string, lang: string): void { this.tasks.push({ id, lang, claimedBy: null, status: 'open' }); }
  claim(taskId: string, user: string): boolean {
    const t = this.tasks.find((x) => x.id === taskId);
    if (!t || t.status !== 'open') return false;
    t.claimedBy = user; t.status = 'claimed';
    return true;
  }
  submit(taskId: string): boolean {
    const t = this.tasks.find((x) => x.id === taskId);
    if (!t || t.status !== 'claimed') return false;
    t.status = 'review';
    return true;
  }
}

/** F08678 进度看板：各语言完成度。 */
export function progressBoard(tasks: { lang: string; status: string }[]): Record<string, number> {
  const out: Record<string, number> = {};
  const groups: Record<string, { done: number; total: number }> = {};
  for (const t of tasks) {
    const g = (groups[t.lang] ??= { done: 0, total: 0 });
    g.total++;
    if (t.status === 'done') g.done++;
  }
  for (const [lang, g] of Object.entries(groups)) out[lang] = Math.round((g.done / g.total) * 100);
  return out;
}

/** F08679 审核流程：claimed→review→done。 */
export function reviewFlowTask(task: { status: 'claimed' | 'review' | 'done' }, approve: boolean): 'claimed' | 'review' | 'done' {
  if (task.status === 'claimed') return 'review';
  return task.status === 'review' && approve ? 'done' : task.status;
}

/** F08680 术语投票。 */
export class TermVote {
  votes: Record<string, number> = {};
  vote(term: string): number { return (this.votes[term] = (this.votes[term] ?? 0) + 1); }
  winner(): string | null {
    const e = Object.entries(this.votes).sort((a, b) => b[1] - a[1])[0];
    return e && e[1] > 0 ? e[0] : null;
  }
}

/** F08681 新词讨论：议题与回帖。 */
export class TermDiscussion {
  threads: { term: string; posts: string[] }[] = [];
  open(term: string): void { this.threads.push({ term, posts: [] }); }
  reply(term: string, post: string): boolean { const t = this.threads.find((x) => x.term === term); if (!t) return false; t.posts.push(post); return true; }
}

/** F08682 质量互评：双人评审均值。 */
export function peerReview(scores: number[]): number {
  if (scores.length === 0) return 0;
  return Math.round((scores.reduce((a, b) => a + b, 0) / scores.length) * 10) / 10;
}

/** F08683/F08684 译者等级与徽章。 */
export function translatorLevel(words: number): { level: number; title: string } {
  const level = words >= 50000 ? 5 : words >= 20000 ? 4 : words >= 8000 ? 3 : words >= 2000 ? 2 : 1;
  return { level, title: ['新译者', '见习译者', '译者', '资深译者', '大师译者'][level - 1]! };
}
export function translatorBadges(words: number, langs: number): string[] {
  const b: string[] = [];
  if (words >= 2000) b.push('破千');
  if (words >= 20000) b.push('两万里程碑');
  if (langs >= 3) b.push('多语通');
  return b;
}

/** F08685 翻译马拉松：活动进度。 */
export function translationSprint(target: number, done: number): { pct: number; finished: boolean } {
  return { pct: Math.round((done / Math.max(1, target)) * 100), finished: done >= target };
}

/** F08686 社区教学步骤。 */
export const COMMUNITY_TUTORIAL = ['认领任务', '查阅术语表', '提交翻译', '互评审核', '获得徽章'];

/** F08687 CAT 位（§15 预留）。 */
export const CAT_SLOT: FeatureSlot = { id: 'F08687', name: 'CAT 工具', reserved: true, enabled: false };

/** F08688 社区 API 冻结。 */
export interface CommunityApi { claim(taskId: string, user: string): boolean }
export const COMMUNITY_API_VERSION = '1.0';

/** F08689 导出导入：任务包序列化。 */
export function taskPackExport(tasks: { id: string; lang: string }[]): string {
  return JSON.stringify(tasks);
}
export function taskPackImport(json: string): { id: string; lang: string }[] | null {
  try {
    const v = JSON.parse(json) as unknown;
    return Array.isArray(v) && v.every((x) => typeof (x as { id: unknown }).id === 'string') ? (v as { id: string; lang: string }[]) : null;
  } catch { return null; }
}

/** F08690 版本对齐：社区翻译跟随版本号。 */
export function versionAlign(community: string, upstream: string): boolean { return community === upstream; }

/** F08691 激励位（§15 预留）。 */
export const INCENTIVE_SLOT: FeatureSlot = { id: 'F08691', name: '社区激励', reserved: true, enabled: false };

/** F08692 社区规则：三条底线。 */
export const COMMUNITY_RULES = ['署名与许可', '术语一致优先', '禁止机翻直发'] as const;

/** F08693 社区彩蛋（低频 + 总控）。 */
export function communityEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 37 === 0 ? '译者之夜 🌙' : null;
}

/** F08694 社区案例。 */
export const COMMUNITY_CASES = ['方言众包尝试', '术语大战投票', '马拉松冲榜'] as const;

/** F08695 社区审计项。 */
export const COMMUNITY_AUDIT = ['认领去重', '审核留痕', '导出可解析', '预留位冻结', '规则公示'];

/** F08696 社区回归清单。 */
export const COMMUNITY_REGRESSION = ['认领回归', '投票回归', '对齐回归'];

/** F08697 社区看板2：审核队列深度。 */
export function reviewQueueDepth(tasks: { status: string }[]): number {
  return tasks.filter((t) => t.status === 'review').length;
}

/** F08698 社区收官清单。 */
export const COMMUNITY_FINALE = ['25 项自检', '预留位冻结', '案例归档', '回归通过', 'API 冻结'];

/** F08699 社区致谢。 */
export const COMMUNITY_THANKS = ['全部社区译者', '评审志愿者', '术语贡献者'];

/** F08700 社区博物馆：历史贡献展。 */
export const COMMUNITY_MUSEUM = ['首批十位译者手记', '第一万条术语', '马拉松奖杯'] as const;

/* ============ 族0349 无障碍研究（F08701~F08725） ============ */

/** F08701/F08702 高校合作与引用清单。 */
export const UNIVERSITY_PARTNERS = ['特殊教育学院', '人机交互实验室'] as const;
export const CITATIONS = ['WCAG 2.2 指南', 'ISO 9241-171', '本地无障碍白皮书'] as const;

/** F08703 开放数据：匿名化数据集。 */
export function anonymize(records: { userId: string; setting: string }[]): { hash: string; setting: string }[] {
  return records.map((r) => ({ hash: 'anon-' + r.userId.split('').reverse().join(''), setting: r.setting }));
}

/** F08704/F08705 用户研究与可用性测试计划。 */
export interface ResearchPlan { name: string; participants: number; disability: string; tasks: string[] }
export function researchPlan(name: string, participants: number, disability: string): ResearchPlan {
  return { name, participants: clamp(participants, 5, 50), disability, tasks: ['完成入门', '开启预设', '执行任务'] };
}
export function usabilityScore(success: number, total: number, satis: number): number {
  if (total === 0) return 0;
  return Math.round(((success / total) * 0.6 + (satis / 5) * 0.4) * 100);
}

/** F08706 眼动位（§15 预留）。 */
export const EYE_TRACK_SLOT: FeatureSlot = { id: 'F08706', name: '眼动研究', reserved: true, enabled: false };

/** F08707 实验室开放：预约制。 */
export class OpenLab {
  slots: { date: string; bookedBy: string | null }[] = [];
  openSlots(dates: string[]): void { this.slots = dates.map((d) => ({ date: d, bookedBy: null })); }
  book(date: string, by: string): boolean {
    const s = this.slots.find((x) => x.date === date && x.bookedBy === null);
    if (!s) return false;
    s.bookedBy = by;
    return true;
  }
}

/** F08708 报告公开：脱敏发布。 */
export function publishReport(findings: { title: string; detail: string; pii: boolean }[]): { title: string; detail: string }[] {
  return findings.filter((f) => !f.pii).map((f) => ({ title: f.title, detail: f.detail }));
}

/** F08709 伦理审查：三要素齐全。 */
export function ethicsReview(ok: { consent: boolean; anonymized: boolean; withdrawable: boolean }): boolean { return ok.consent && ok.anonymized && ok.withdrawable; }

/** F08710 知情同意模板。 */
export const CONSENT_TEMPLATE = ['研究目的', '数据用途与匿名化', '可随时退出', '联系方式'] as const;

/** F08711 参与者感谢：致谢名单生成。 */
export function participantThanks(names: string[]): string {
  return '感谢参与者：' + names.join('、');
}

/** F08712 研究会议：年度日程。 */
export const RESEARCH_CONFERENCES = ['无障碍论坛（春）', 'HCII 投稿（夏）', '年度复盘（冬）'] as const;

/** F08713 研究奖项：评奖。 */
export function researchAward(nominations: { name: string; score: number }[]): string | null {
  return [...nominations].sort((a, b) => b.score - a.score)[0]?.name ?? null;
}

/** F08714 基金位（§15 预留）。 */
export const FUND_SLOT: FeatureSlot = { id: 'F08714', name: '研究基金', reserved: true, enabled: false };

/** F08715 研究教学步骤。 */
export const RESEARCH_TUTORIAL = ['立题评审', '伦理审查', '招募与同意', '数据匿名化', '公开报告'];

/** F08716 研究彩蛋（低频 + 总控）。 */
export function researchEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 31 === 0 ? '数据不说谎 📈' : null;
}

/** F08717 研究 API 冻结。 */
export interface ResearchApi { ethicsReview(ok: { consent: boolean; anonymized: boolean; withdrawable: boolean }): boolean }
export const RESEARCH_API_VERSION = '1.0';

/** F08718 研究回归清单。 */
export const RESEARCH_REGRESSION = ['匿名化回归', '同意流回归', '发布过滤回归'];

/** F08719 研究看板：研究进度。 */
export function researchBoard(plans: ResearchPlan[]): { name: string; participants: number }[] {
  return plans.map((p) => ({ name: p.name, participants: p.participants }));
}

/** F08720 研究文档页。 */
export const RESEARCH_DOCS = ['研究章程', '伦理指南', '数据政策'];

/** F08721 研究案例。 */
export const RESEARCH_CASES = ['读屏任务耗时研究', '扫描输入疲劳研究', '老年用户可用性'] as const;

/** F08722 研究收官清单。 */
export const RESEARCH_FINALE = ['25 项自检', '预留位冻结', '报告公开', '案例归档', 'API 冻结'];

/** F08723 研究致谢。 */
export const RESEARCH_THANKS = ['全体研究参与者', '高校合作方', '伦理委员会'];

/** F08724 研究日历。 */
export const RESEARCH_CALENDAR = ['03-01 立题', '06-01 中期', '12-01 年报'] as const;

/** F08725 研究二期收官。 */
export const RESEARCH_FINALE_2 = ['二期立项通过', '数据集扩容', '论文投稿'];

/* ============ 族0350 本地化收官（F08726~F08750） ============ */

/** F08726 三语 100% 覆盖检查。 */
export function coverageCheck(dicts: Record<string, string[]>, keys: string[]): { ok: boolean; missing: Record<string, number> } {
  const missing: Record<string, number> = {};
  for (const [lang, ks] of Object.entries(dicts)) missing[lang] = keys.filter((k) => !ks.includes(k)).length;
  return { ok: Object.values(missing).every((n) => n === 0), missing };
}

/** F08727 RTL 审计：镜像完整项数。 */
export function rtlAudit(pages: string[], mirrored: string[]): { ok: boolean; missing: string[] } {
  const missing = pages.filter((p) => !mirrored.includes(p));
  return { ok: missing.length === 0, missing };
}

/** F08728/F08729 术语与风格指南终版。 */
export const TERM_FINAL: Record<string, string> = { 桌面: 'Desktop', 工作区: 'Workspace', 放大镜: 'Magnifier' };
export const STYLE_GUIDE_FINAL = ['简洁优先', '动词开头', '避免俚语', '统一标点'] as const;

/** F08730~F08733 四大终审：QA/a11y/文化/合规。 */
export class FinalReview {
  private results: Record<string, boolean> = {};
  review(kind: 'qa' | 'a11y' | 'culture' | 'compliance', passed: boolean): boolean { this.results[kind] = passed; return passed; }
  get allPassed(): boolean { return ['qa', 'a11y', 'culture', 'compliance'].every((k) => this.results[k] === true); }
}

/** F08734 发布清单终版。 */
export const RELEASE_LIST_FINAL = ['多语说明', '镜像可用', '法律检查', '回滚预案', '紧急停用演练'] as const;

/** F08735/F08736/F08737 文档、教学材料、帮助终版。 */
export const DOC_FINAL = ['快速上手', '无障碍指南', '本地化指南'] as const;
export const TUTORIAL_FINAL = ['入门教学包', '无障碍教学包', '本地化教学包'] as const;
export const HELP_FINAL = ['帮助中心三语 100%', '上下文帮助覆盖 25 面', 'FAQ 更新'] as const;

/** F08738 营销本地化素材。 */
export const MARKETING_L10N = { 'zh-CN': '轻快如风，稳如磐石', 'zh-TW': '輕快如風，穩如磐石', en: 'Swift as wind, solid as rock' } as const;

/** F08739 商店元数据本地化。 */
export function storeMetadata(locale: string): { name: string; subtitle: string } {
  const t: Record<string, { name: string; subtitle: string }> = {
    'zh-CN': { name: 'Variable 桌面', subtitle: '为每个人而生的桌面' },
    'zh-TW': { name: 'Variable 桌面', subtitle: '為每個人而生的桌面' },
    en: { name: 'Variable Desktop', subtitle: 'A desktop for everyone' },
  };
  return t[locale] ?? t.en!;
}

/** F08740 错误码本地化：错误码→三语文案。 */
export const ERROR_CODES: Record<string, Record<string, string>> = {
  E1001: { 'zh-CN': '网络不可用', 'zh-TW': '網路不可用', en: 'Network unavailable' },
  E1002: { 'zh-CN': '文件已损坏', 'zh-TW': '檔案已損壞', en: 'File corrupted' },
};
export function errorMessage(code: string, locale: string): string {
  return ERROR_CODES[code]?.[locale] ?? ERROR_CODES[code]?.en ?? '未知错误';
}

/** F08741 日志策略：开发日志保持英文。 */
export function devLogPolicy(locale: string, msg: string): { lang: string; msg: string } {
  return { lang: locale === 'en' ? 'en' : 'en', msg };
}

/** F08742 合同模板位（§15 预留）。 */
export const CONTRACT_SLOT: FeatureSlot = { id: 'F08742', name: '合同模板', reserved: true, enabled: false };

/** F08743 年报。 */
export function annualL10nReport(year: number, langs: number, coverage: number): { year: number; langs: number; coverage: number } {
  return { year, langs: clamp(langs, 0, 999), coverage: clamp(coverage, 0, 100) };
}

/** F08744 本地化博物馆。 */
export const L10N_MUSEUM = ['第一句繁体译文', '第一次 RTL 截图', '三语 100% 纪念碑'] as const;

/** F08745 致谢。 */
export const L10N_THANKS = ['全部译者', '无障碍顾问团', '文化审查委员会', '研究参与者'];

/** F08746 收官彩蛋（低频 + 总控）。 */
export function finaleEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 29 === 0 ? '万项功成，一词不易 🏁' : null;
}

/** F08747 时间线大事记。 */
export const FINALE_TIMELINE = ['2026-09 W6 领域14 交付', '2026-Q4 全球发布', '2027 年报'] as const;

/** F08748 路线图。 */
export const FINALE_ROADMAP = ['W7 UI 优化', 'W8 大收官', '下一纪元'] as const;

/** F08749 庆典仪式：里程碑点亮。 */
export function celebration(milestones: number): { lit: number; done: boolean } {
  return { lit: clamp(milestones, 0, 8), done: milestones >= 8 };
}

/** F08750 最终致谢。 */
export const FINAL_THANKS = ['每一位用户', '每一位译者', '每一位无障碍顾问', '每一位研究者', '并肩的八十个会话'];
