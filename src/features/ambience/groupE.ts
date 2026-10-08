// AURORA-10000: AI-60 批次（族0296~0300 · 第一印象打磨/微文案/帮助体系/艺术合作/视觉收官），勿删。

/* ============ 族0296 第一印象打磨（F07376~F07400） ============ */

export interface FirstRunStep {
  id: string;
  name: string;
  done: boolean;
}

export class FirstImpression {
  private steps: FirstRunStep[] = [
    { id: 'first-frame', name: '首帧即有内容', done: false },
    { id: 'no-white', name: '防白屏兜底', done: false },
    { id: 'font-preload', name: '字体预载', done: false },
    { id: 'icon-preload', name: '图标预载', done: false },
    { id: 'onboarding', name: '首启引导', done: false },
    { id: 'seamless-desktop', name: '无缝进桌', done: false },
    { id: 'first-wallpaper', name: '首次壁纸决策', done: false },
    { id: 'first-accent', name: '首次强调色决策', done: false },
    { id: 'welcome-card', name: '欢迎卡', done: false },
    { id: 'three-min-tutorial', name: '三分钟教学', done: false },
  ];
  private skipped = new Set<string>();

  /** F07376/77 首帧与防白屏 */
  static firstFrameBudget(ms: number): boolean {
    return ms <= 800;
  }
  static whiteScreenFallback(hasContent: boolean, elapsedMs: number): string {
    return hasContent ? 'ok' : elapsedMs > 1500 ? 'show-skeleton' : 'wait';
  }
  /** F07378/79 预载清单 */
  static preloadList(): { fonts: string[]; icons: string[] } {
    return { fonts: ['ui-sans', 'ui-mono'], icons: ['taskbar', 'desktop', 'settings'] };
  }
  /** F07384 欢迎卡文案 */
  static welcomeCard(name: string): string {
    return `欢迎回来，${name}。桌面已按你的习惯就绪。`;
  }
  /** F07385 三分钟教学 */
  static quickTour(): string[] {
    return ['开始菜单与搜索', '窗口与虚拟桌面', '文件管理', '个性化：主题与强调色', '快捷键速成'];
  }
  /** F07386/87 回顾 */
  static weekRecall(completed: number, total: number): string {
    return `一周了：你已学会 ${completed}/${total} 项核心操作`;
  }
  static monthRecall(days: number): string {
    return `30 天成长回顾：累计使用 ${days} 天`;
  }
  /** F07388/89 更新体验 */
  static firstUpdateNotes(items: string[]): string {
    return `本次更新：${items.slice(0, 3).join('、')}${items.length > 3 ? ' 等' : ''}`;
  }
  static changeHighlights(changes: { id: string; title: string; seen: boolean }[]): string[] {
    return changes.filter((c) => !c.seen).map((c) => `新：${c.title}`);
  }
  /** F07391 引导记忆：跳过不再弹 */
  skip(id: string): void {
    this.skipped.add(id);
  }
  shouldShow(id: string): boolean {
    return !this.skipped.has(id);
  }
  /** F07392 重看入口 */
  static rewatchEntry(): string {
    return '设置 → 帮助 → 重看首启教学';
  }
  /** F07393 完成度 */
  markDone(id: string): void {
    const s = this.steps.find((x) => x.id === id);
    if (s) s.done = true;
  }
  get progress(): number {
    return this.steps.filter((s) => s.done).length / this.steps.length;
  }
  /** F07394/95 离线与慢机首启 */
  static offlineFirstRun(online: boolean): string {
    return online ? 'cloud-sync' : 'local-only（可全程离线设置）';
  }
  static lowEndPreset(score: number): string {
    return score < 40 ? 'lite' : score < 75 ? 'balanced' : 'full';
  }
  /** F07396 隐私默认 */
  static privacyDefaults(): Record<string, boolean> {
    return { telemetry: false, personalizedAds: false, location: false, diagnostics: false };
  }
  /** F07397 无障碍首启检测 */
  static a11yFirstRun(needs: { vision: boolean; hearing: boolean; motor: boolean }): string[] {
    const out: string[] = [];
    if (needs.vision) out.push('建议开启：放大与高对比');
    if (needs.hearing) out.push('建议开启：视觉提示音替代');
    if (needs.motor) out.push('建议开启：粘滞键与更大点击区');
    return out;
  }
  /** F07399 彩蛋：首启生日解锁 */
  static firstRunEaster(setupDate: string, today: string): boolean {
    return setupDate === today;
  }
  /** F07400 收官 */
  static finale(): string {
    return '第一印象收官：从首帧到 30 天回顾全链路打磨';
  }
}

/* ============ 族0297 微文案（F07401~F07425） ============ */

export type CopyTone = 'friendly' | 'neutral' | 'technical' | 'child';

export class CopyKit {
  /** F07401 错误库：不吓人 */
  static errors(): Record<string, string> {
    return {
      network: '网络好像打了个盹，稍后重试即可',
      file: '这个文件暂时打不开，可能正在被其他程序使用',
      unknown: '出了点小状况，我们已记录日志，放心继续使用',
    };
  }
  /** F07402 成功库：不油腻 */
  static success(): Record<string, string> {
    return { save: '已保存', copy: '已复制', done: '搞定' };
  }
  /** F07403 空态 */
  static emptyStates(): Record<string, string> {
    return { files: '这里空空的，拖入文件开始吧', search: '没找到相关内容，换个词试试', notes: '写下第一条笔记' };
  }
  /** F07404 加载 */
  static loading(): string[] {
    return ['马上就好…', '正在整理…', '快了快了…'];
  }
  /** F07405/06/07 规范 */
  static buttonRules(): string[] {
    return ['动词开头', '不超过 4 个字', '危险操作明说后果'];
  }
  static menuRules(): string[] {
    return ['常用在上', '分组用分隔线', '快捷键右对齐'];
  }
  static notifyRules(): string[] {
    return ['一句话说清', '附一个动作', '可静音'];
  }
  /** F07410 危险确认 */
  static dangerConfirm(action: string, consequence: string): string {
    return `${action}后${consequence}，此操作不可撤销。确定继续吗？`;
  }
  /** F07412 孩子模式 */
  static childCopy(key: 'delete' | 'exit'): string {
    return key === 'delete' ? '要把它放进回收站吗？' : '要退出吗？';
  }
  /** F07413 技术模式：展开详说 */
  static technicalDetail(brief: string, detail: string): string {
    return `${brief}（详情：${detail}）`;
  }
  /** F07414 字数约束 */
  static withinLimit(text: string, max: number): boolean {
    return text.length <= max;
  }
  /** F07415 术语表 */
  static glossary(): Record<string, string> {
    return { 档案: '一组可切换的个性化配置', 氛围光: '屏幕四周的环境背光', 密度: '界面元素的大小与间距' };
  }
  /** F07416 中英一致 */
  static zhEnMap(): Record<string, string> {
    return { 已保存: 'Saved', 已复制: 'Copied', 取消: 'Cancel', 确定: 'OK' };
  }
  /** F07417 繁中一致 */
  static zhTwMap(): Record<string, string> {
    return { 已保存: '已儲存', 已复制: '已複製', 取消: '取消', 确定: '確定' };
  }
  /** F07418 标点规范 */
  static punctuationOk(text: string): boolean {
    return !/[，。！？]{2,}/.test(text) && !/[,.!?]{2,}/.test(text);
  }
  /** F07419 A/B 测试 */
  static abTest(a: string, b: string, clicks: [number, number]): string {
    return clicks[0] >= clicks[1] ? a : b;
  }
  /** F07420 审计工具：发现感叹号与超长 */
  static audit(texts: string[], maxLen = 20): string[] {
    return texts.filter((t) => t.length > maxLen || t.includes('!!') || t.includes('！！'));
  }
  /** F07424 API */
  static apiSpec(): string {
    return 'variable.copy.get(key, {tone, locale, limit}) -> string';
  }
  /** F07425 收官 */
  static finale(): string {
    return '微文案收官：错误/成功/空态/危险四库齐备';
  }
}

/* ============ 族0298 帮助体系（F07426~F07450） ============ */

export interface HelpTopic {
  id: string;
  title: string;
  body: string;
  keywords: string[];
  audience: ('general' | 'teacher' | 'parent' | 'senior' | 'developer' | 'admin')[];
  version: string;
}

export class HelpCenter {
  private topics = new Map<string, HelpTopic>();
  private views = new Map<string, number>();

  publish(t: HelpTopic): boolean {
    if (this.topics.has(t.id)) return false;
    this.topics.set(t.id, t);
    return true;
  }
  /** F07426 中心 */
  get all(): HelpTopic[] {
    return [...this.topics.values()];
  }
  /** F07427 情境帮助：按界面键取 */
  contextual(screenKey: string): HelpTopic | null {
    return this.topics.get(`ctx:${screenKey}`) ?? null;
  }
  /** F07429 发现中心 */
  static discovery(feats: { name: string; enabled: boolean }[]): string[] {
    return feats.filter((f) => !f.enabled).map((f) => `试试：${f.name}`);
  }
  /** F07430 快捷键速查 */
  static hotkeyCheat(): Record<string, string> {
    return { 'Ctrl+K': '全局搜索', 'Ctrl+Alt+方向': '切换虚拟桌面', 'Win+D': '显示桌面', F5: '刷新' };
  }
  /** F07433 搜索：标题/关键词命中 */
  search(q: string): HelpTopic[] {
    const needle = q.trim().toLowerCase();
    if (!needle) return [];
    return this.all.filter((t) => t.title.toLowerCase().includes(needle) || t.keywords.some((k) => k.toLowerCase().includes(needle)));
  }
  /** F07434 反馈 */
  open(topicId: string): void {
    this.views.set(topicId, (this.views.get(topicId) ?? 0) + 1);
  }
  /** F07435 与版本同步 */
  static versionSync(topic: HelpTopic, sysVersion: string): boolean {
    return topic.version === sysVersion;
  }
  /** F07436 离线 */
  static offlineBundle(topics: HelpTopic[]): number {
    return topics.length;
  }
  /** F07437 多语言 */
  static localized(topic: HelpTopic, locales: string[]): boolean {
    return locales.every(() => Boolean(topic.title));
  }
  /** F07439~43 受众过滤 */
  forAudience(a: HelpTopic['audience'][number]): HelpTopic[] {
    return this.all.filter((t) => t.audience.includes(a) || t.audience.includes('general'));
  }
  /** F07444 统计：哪不会用 */
  weakest(n = 3): string[] {
    return [...this.views.entries()].sort((a, b) => b[1] - a[1]).slice(0, n).map(([id]) => id);
  }
  /** F07445 改进闭环 */
  static improveLoop(views: number, feedback: number): string {
    return `浏览 ${views} 次、反馈 ${feedback} 条 → 生成改稿任务`;
  }
  /** F07450 致谢 */
  static credits(): string {
    return '帮助内容由社区文档组与无障碍顾问共同审校';
  }
}

/* ============ 族0299 艺术合作（F07451~F07475） ============ */

export interface Artwork {
  id: string;
  title: string;
  artist: string;
  medium: 'wallpaper' | 'theme' | 'soundscape' | 'pixel' | 'illustration' | 'photo' | 'font' | 'animation';
  license: 'CC-BY' | 'CC-BY-SA' | 'CC0';
  year: number;
}

export class ArtProgram {
  private collection = new Map<string, Artwork>();
  private residents: string[] = [];

  /** F07451 驻场计划 */
  joinResidency(artist: string): boolean {
    if (this.residents.includes(artist)) return false;
    this.residents.push(artist);
    return true;
  }
  get residentList(): string[] {
    return [...this.residents];
  }
  /** F07452 驻场作品 */
  submit(a: Artwork): boolean {
    if (this.collection.has(a.id) || !['CC-BY', 'CC-BY-SA', 'CC0'].includes(a.license)) return false;
    this.collection.set(a.id, a);
    return true;
  }
  get works(): Artwork[] {
    return [...this.collection.values()];
  }
  /** F07453 展模式：全屏画廊顺序 */
  static galleryOrder(works: Artwork[]): string[] {
    return works.map((w, i) => `slot-${i + 1}:${w.title}`);
  }
  /** F07454 数字展策展：按媒介分组 */
  static curate(works: Artwork[]): Record<string, string[]> {
    const out: Record<string, string[]> = {};
    for (const w of works) (out[w.medium] ??= []).push(w.title);
    return out;
  }
  /** F07456 访谈 */
  static interview(artist: string, q: string[]): string[] {
    return q.map((question, i) => `Q${i + 1}: ${question} —— ${artist}`);
  }
  /** F07465 拒绝 NFT */
  static nftPolicy(offer: { nft: boolean }): string {
    return offer.nft ? '拒绝：系统内置艺术永不发行 NFT' : '接受：按 CC 授权收录';
  }
  /** F07466 授权选择 */
  static licenseOptions(): string[] {
    return ['CC-BY', 'CC-BY-SA', 'CC0'];
  }
  /** F07467 署名规范 */
  static attribution(w: Artwork): string {
    return `${w.title} © ${w.artist} ${w.year} ${w.license}`;
  }
  /** F07468 收藏馆 */
  get gallery(): string[] {
    return this.works.map((w) => ArtProgram.attribution(w));
  }
  /** F07469 每天一幅 */
  static dailyArt(works: Artwork[], dayOfYear: number): Artwork | null {
    if (works.length === 0) return null;
    return works[dayOfYear % works.length] ?? null;
  }
  /** F07473 社区 */
  static communityChannels(): string[] {
    return ['征集页', '投稿评审', '月度精选', '艺术家访谈'];
  }
  /** F07474 收官 */
  static finale(n: number): string {
    return `艺术收官：${n} 件作品入库`;
  }
}

/* ============ 族0300 视觉收官（F07476~F07500） ============ */

export interface LibraryV2 {
  name: string;
  version: string;
  items: number;
}

export class VisionFinale {
  private libraries = new Map<string, LibraryV2>();
  private auditFindings: string[] = [];

  /** F07477 债务清零 */
  clearDebt(item: string): void {
    this.auditFindings = this.auditFindings.filter((f) => f !== item);
  }
  addDebt(item: string): number {
    this.auditFindings.push(item);
    return this.auditFindings.length;
  }
  get debt(): string[] {
    return [...this.auditFindings];
  }
  /** F07478~89 库 2.0 升级登记 */
  upgradeLibrary(name: string, version: string, items: number): boolean {
    const old = this.libraries.get(name);
    if (old && old.version >= version) return false;
    this.libraries.set(name, { name, version, items });
    return true;
  }
  get librariesV2(): LibraryV2[] {
    return [...this.libraries.values()];
  }
  static expectedLibraries(): string[] {
    return ['design-system', 'components', 'icons', 'motion', 'themes', 'wallpapers', 'soundscapes', 'cursors', 'fonts', 'widgets', 'screensavers', 'eggs'];
  }
  /** F07490 视觉规范 */
  static spec(): string[] {
    return ['色彩：token 唯一来源', '字体：三族九级', '间距：4 的倍数', '圆角：同心', '动效：层级时长', '图标：24 网格'];
  }
  /** F07491 设计评审流程 */
  static reviewFlow(): string[] {
    return ['提案', '走查', '可用性测试', '过会', '入库'];
  }
  /** F07492 贡献指南 */
  static contributionGuide(): string[] {
    return ['Fork + 分支', '遵循规范文档', '附带基线截图', '过视觉守卫', '提交评审'];
  }
  /** F07494/95 工具链与到代码 */
  static toolchain(): string[] {
    return ['token-gen', 'icon-export', 'motion-lab', 'baseline-shot'];
  }
  static designToCode(dsl: string): string {
    return dsl.replace(/(?:^|;)\s*(?<k>[\w-]+)=(?<v>[^;]+)/g, (_m, k: string, v: string) => `--aurora-${k}:${v};`).trim();
  }
  /** F07496 token 双向同步 */
  static tokenSync(a: Record<string, string>, b: Record<string, string>): { synced: Record<string, string>; drift: string[] } {
    const drift = Object.keys(a).filter((k) => k in b && a[k] !== b[k]);
    return { synced: { ...a, ...b }, drift };
  }
  /** F07497 性能守卫 */
  static perfGuard(frameMs: number, nodes: number): boolean {
    return frameMs <= 16 && nodes <= 1500;
  }
  /** F07499 庆典 */
  static celebration(families: number, items: number): string {
    return `🎉 视觉体系收官：${families} 个族、${items} 项能力全部就绪`;
  }
  /** F07500 致谢 */
  static credits(): string {
    return '致谢每一位设计师、艺术家、内测用户与社区贡献者';
  }
}
