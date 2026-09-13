// AURORA-10000: AI-69 批次（族0341~0345）自检条目，勿删。

import { CheckEntry, memoized } from './types';
import * as D from './groupD';

export function checkF0341(): CheckEntry[] {
  const wiz = new D.FirstRunWizard();
  wiz.next();
  const cl = new D.SetupChecklist();
  cl.add('语言'); cl.add('视觉'); cl.tick('语言');
  const logs = ['字体大小', '音量', '字体大小', '对比度', '字体大小'];
  return [
    memoized({ id: 'F08501', name: '读屏自动检测', check: () => D.autoDetect({ screenReaderRunning: true, systemLocale: 'zh-CN' }).a11yOn && !D.autoDetect({ screenReaderRunning: false, systemLocale: 'zh-CN' }).a11yOn }),
    memoized({ id: 'F08502', name: '语言自动跟随', check: () => D.autoDetect({ screenReaderRunning: false, systemLocale: 'en' }).locale === 'en' }),
    memoized({ id: 'F08503', name: '首次配置推进', check: () => !wiz.done && wiz.next() === '视觉' && wiz.progressPct === 33 }),
    memoized({ id: 'F08504', name: '快速设置完成', check: () => { while (!wiz.done) wiz.next(); return wiz.done && wiz.progressPct === 100; } }),
    memoized({ id: 'F08505', name: '一键预设四类', check: () => Object.keys(D.ONE_CLICK_PRESETS).length === 4 && D.ONE_CLICK_PRESETS.visual!.largeText === 150 }),
    memoized({ id: 'F08506', name: '预设组合合并', check: () => { const c = D.combinedPreset(['visual', 'hearing']); return c.magnifier === true && c.captions === true; } }),
    memoized({ id: 'F08507', name: '预设分享导入', check: () => { const code = D.presetExport(['visual', 'motor']); return D.presetImport(code)!.length === 2 && D.presetImport('vp2:x') === null && D.presetImport('vp1:bogus') === null; } }),
    memoized({ id: 'F08508', name: '检查清单进度', check: () => { const p1 = cl.progress; return p1 === 50 && !cl.tick('不存在') && cl.tick('视觉') && (cl.progress as number) === 100; } }),
    memoized({ id: 'F08509', name: '自评问卷评分', check: () => { const r = D.questionnaireScore([{ id: 'vision1', answerYes: true, weight: 2 }, { id: 'hearing1', answerYes: true, weight: 1 }, { id: 'motor1', answerYes: false, weight: 3 }]); return r.score === 3 && r.recommends.join() === 'visual,hearing'; } }),
    memoized({ id: 'F08510', name: '推荐配置映射', check: () => D.recommendedConfig(4).intensity === 'full' && D.recommendedConfig(1).intensity === 'standard' && D.recommendedConfig(0).preset.length === 0 }),
    memoized({ id: 'F08511', name: '渐进增强解锁', check: () => D.progressiveUnlock(2).length === 2 && D.progressiveUnlock(9).length === 4 && D.progressiveUnlock(0).length === 0 }),
    memoized({ id: 'F08512', name: '视频位预留', check: () => D.VIDEO_SLOT.reserved && !D.VIDEO_SLOT.enabled }),
    memoized({ id: 'F08513', name: '图文指南对', check: () => { const g = D.illustratedGuide('开启字幕', '💬'); return g.icon === '💬'; } }),
    memoized({ id: 'F08514', name: 'FAQ 问答表', check: () => D.ONBOARDING_FAQ.length === 3 && D.ONBOARDING_FAQ[0]!.a.includes('放大镜') }),
    memoized({ id: 'F08515', name: '社区入口', check: () => D.ONBOARDING_COMMUNITY.mentors === true }),
    memoized({ id: 'F08516', name: '热线位预留', check: () => D.HOTLINE_SLOT.reserved && !D.HOTLINE_SLOT.enabled }),
    memoized({ id: 'F08517', name: '家庭协助授权', check: () => { const f = new D.FamilyAssist(); return f.grant(30) === 30 && f.granted && f.grant(999) === 240 && f.revoke() && !f.granted; } }),
    memoized({ id: 'F08518', name: '常改设置统计', check: () => D.frequentSettings(logs)[0]![0] === '字体大小' && D.frequentSettings(logs)[0]![1] === 3 }),
    memoized({ id: 'F08519', name: '入门回归清单', check: () => D.ONBOARDING_REGRESSION.length === 3 }),
    memoized({ id: 'F08520', name: '入门教学五步', check: () => D.ONBOARDING_TUTORIAL.length === 5 }),
    memoized({ id: 'F08521', name: '入门彩蛋低频', check: () => D.onboardingEgg(false, 67) === null && D.onboardingEgg(true, 134) !== null && D.onboardingEgg(true, 1) === null }),
    memoized({ id: 'F08522', name: '入门 API 冻结', check: () => D.ONBOARDING_API_VERSION === '1.0' }),
    memoized({ id: 'F08523', name: '入门文档页', check: () => D.ONBOARDING_DOCS.length === 3 }),
    memoized({ id: 'F08524', name: '入门收官清单', check: () => D.ONBOARDING_FINALE.length === 5 }),
    memoized({ id: 'F08525', name: '入门致谢', check: () => D.ONBOARDING_THANKS.length === 3 }),
  ];
}

export function checkF0342(): CheckEntry[] {
  const room = new D.ClassroomMode();
  return [
    memoized({ id: 'F08526', name: '课堂模式进入', check: () => room.enter(false, ['游戏']).length === 0 && room.active && !room.exam }),
    memoized({ id: 'F08527', name: '字幕广播位预留', check: () => D.BROADCAST_SLOT.reserved && !D.BROADCAST_SLOT.enabled }),
    memoized({ id: 'F08528', name: '朗读课件序列', check: () => D.readableCourseware(['第一课', '', '第二课']).length === 2 }),
    memoized({ id: 'F08529', name: '适配输入策略', check: () => D.adaptedInput('switch') === '扫描输入' && D.adaptedInput('voice') === '语音输入' && D.adaptedInput('keyboard') === '标准键盘' }),
    memoized({ id: 'F08530', name: '简化课堂界面', check: () => D.simplifiedClassroom(['课本', '游戏', '测验'], ['课本', '测验']).length === 2 }),
    memoized({ id: 'F08531', name: '考试模式封锁', check: () => { const r = new D.ClassroomMode(); return r.enter(true, ['聊天', '搜索']).length === 2 && r.exam && r.exit() && !r.active; } }),
    memoized({ id: 'F08532', name: '作业模板注册', check: () => D.HOMEWORK_TEMPLATES.length === 3 }),
    memoized({ id: 'F08533', name: '课件检查器', check: () => { const bad = D.coursewareCheck({ hasTitle: false, imagesHaveAlt: false, contrastOk: false }); const good = D.coursewareCheck({ hasTitle: true, imagesHaveAlt: true, contrastOk: true }); return bad.issues.length === 3 && !bad.ok && good.ok; } }),
    memoized({ id: 'F08534', name: '学生档案模板', check: () => { const s = D.studentTemplate('小明', ['visual']); return s.name === '小明' && s.notes === '' && s.needs.length === 1; } }),
    memoized({ id: 'F08535', name: '教师指南', check: () => D.TEACHER_GUIDE.length === 4 }),
    memoized({ id: 'F08536', name: '家长指南', check: () => D.PARENT_GUIDE.length === 3 }),
    memoized({ id: 'F08537', name: '教a11y 教学五步', check: () => D.EDU_TUTORIAL.length === 5 }),
    memoized({ id: 'F08538', name: '教a11y 审计五项', check: () => D.EDU_AUDIT.length === 5 }),
    memoized({ id: 'F08539', name: '教a11y 测试用例', check: () => D.EDU_TESTS.length === 5 }),
    memoized({ id: 'F08540', name: '教a11y 回归清单', check: () => D.EDU_REGRESSION.length === 3 }),
    memoized({ id: 'F08541', name: '教a11y 文档页', check: () => D.EDU_DOCS.length === 3 }),
    memoized({ id: 'F08542', name: '教a11y 彩蛋低频', check: () => D.eduEgg(false, 61) === null && D.eduEgg(true, 122) !== null && D.eduEgg(true, 1) === null }),
    memoized({ id: 'F08543', name: '教a11y API 冻结', check: () => D.EDU_API_VERSION === '1.0' }),
    memoized({ id: 'F08544', name: '教a11y 案例', check: () => D.EDU_CASES.length === 3 }),
    memoized({ id: 'F08545', name: '教a11y 收官清单', check: () => D.EDU_FINALE.length === 5 }),
    memoized({ id: 'F08546', name: '教a11y 致谢', check: () => D.EDU_THANKS.length === 3 }),
    memoized({ id: 'F08547', name: '教育合作位预留', check: () => { const s = D.EDU_SLOTS.find((x) => x.id === 'F08547')!; return s.reserved && !s.enabled; } }),
    memoized({ id: 'F08548', name: '教育研究位预留', check: () => { const s = D.EDU_SLOTS.find((x) => x.id === 'F08548')!; return s.reserved && !s.enabled; } }),
    memoized({ id: 'F08549', name: '教a11y 日历', check: () => D.EDU_CALENDAR.length === 3 && D.EDU_CALENDAR[0]!.includes('开学') }),
    memoized({ id: 'F08550', name: '教a11y 二期收官', check: () => D.EDU_FINALE_2.length === 3 }),
  ];
}

export function checkF0343(): CheckEntry[] {
  const pc = new D.PresentationCaptions();
  pc.add('开场', 5); pc.add('要点', 10);
  return [
    memoized({ id: 'F08551', name: '演讲字幕排程', check: () => { const s = pc.schedule(); return s[0]!.from === 0 && s[1]!.from === 5 && pc.totalSeconds === 15; } }),
    memoized({ id: 'F08552', name: '会议字幕位预留', check: () => D.MEETING_CAPTION_SLOT.reserved && !D.MEETING_CAPTION_SLOT.enabled }),
    memoized({ id: 'F08553', name: '简历模板校验', check: () => { const bad = D.resumeValidate(['基本信息']); const good = D.resumeValidate([...D.RESUME_TEMPLATE]); return !bad.ok && bad.missing.length === 4 && good.ok; } }),
    memoized({ id: 'F08554', name: '文档检查器', check: () => { const r = D.docA11yCheck({ hasHeadingStyles: false, tablesHaveHeader: true, linksHaveText: false }); return !r.ok && r.issues.length === 2 && D.docA11yCheck({ hasHeadingStyles: true, tablesHaveHeader: true, linksHaveText: true }).ok; } }),
    memoized({ id: 'F08555', name: '纯文本邮件剥离', check: () => D.plainTextEmail('<p>你好</p> <b>世界</b>') === '你好 世界' }),
    memoized({ id: 'F08556', name: '三重提醒钳位', check: () => { const r = D.reinforcedReminders(30); return r.join() === '0,15,25' && D.reinforcedReminders(120).join() === '60,105,115'; } }),
    memoized({ id: 'F08557', name: '员工档案模板', check: () => { const e = D.employeeTemplate('小李'); return e.needs.length === 0 && e.accommodations.length === 0; } }),
    memoized({ id: 'F08558', name: '雇主指南', check: () => D.EMPLOYER_GUIDE.length === 4 }),
    memoized({ id: 'F08559', name: '员工指南', check: () => D.EMPLOYEE_GUIDE.length === 4 }),
    memoized({ id: 'F08560', name: '合理便利映射', check: () => D.accommodationSuggest(['hearing', 'motor']).length === 2 && D.accommodationSuggest(['cognitive'])[0]!.includes('任务拆分') }),
    memoized({ id: 'F08561', name: '职a11y 教学五步', check: () => D.WORK_TUTORIAL.length === 5 }),
    memoized({ id: 'F08562', name: '职a11y 审计五项', check: () => D.EDU_WORK_AUDIT.length === 5 }),
    memoized({ id: 'F08563', name: '职a11y 测试用例', check: () => D.WORK_TESTS.length === 5 }),
    memoized({ id: 'F08564', name: '职a11y 回归清单', check: () => D.WORK_REGRESSION.length === 3 }),
    memoized({ id: 'F08565', name: '职a11y 文档页', check: () => D.WORK_DOCS.length === 3 }),
    memoized({ id: 'F08566', name: '职a11y 彩蛋低频', check: () => D.workEgg(false, 59) === null && D.workEgg(true, 118) !== null && D.workEgg(true, 1) === null }),
    memoized({ id: 'F08567', name: '职a11y API 冻结', check: () => D.WORK_API_VERSION === '1.0' }),
    memoized({ id: 'F08568', name: '职a11y 案例', check: () => D.WORK_CASES.length === 3 }),
    memoized({ id: 'F08569', name: '职a11y 收官清单', check: () => D.WORK_FINALE.length === 5 }),
    memoized({ id: 'F08570', name: '职a11y 致谢', check: () => D.WORK_THANKS.length === 3 }),
    memoized({ id: 'F08571', name: '企业合作位预留', check: () => { const s = D.WORK_SLOTS.find((x) => x.id === 'F08571')!; return s.reserved && !s.enabled; } }),
    memoized({ id: 'F08572', name: '职场研究位预留', check: () => { const s = D.WORK_SLOTS.find((x) => x.id === 'F08572')!; return s.reserved && !s.enabled; } }),
    memoized({ id: 'F08573', name: '职a11y 日历', check: () => D.WORK_CALENDAR.length === 3 }),
    memoized({ id: 'F08574', name: '职a11y 二期收官', check: () => D.WORK_FINALE_2.length === 3 }),
    memoized({ id: 'F08575', name: '职场实验位预留', check: () => D.WORK_EXP_SLOT.reserved && !D.WORK_EXP_SLOT.enabled }),
  ];
}

export function checkF0344(): CheckEntry[] {
  const fm = new D.FamilyManager();
  return [
    memoized({ id: 'F08576', name: '一键大字预设', check: () => { const s = D.oneClickLargeText(); return s.fontScale === 150 && s.fontWeight === 600; } }),
    memoized({ id: 'F08577', name: '一键高对比预设', check: () => D.oneClickHighContrast().highContrast === true && D.oneClickHighContrast().focusRing === true }),
    memoized({ id: 'F08578', name: '简化桌面白名单', check: () => { const r = D.simplifiedDesktop(['电话', '商店', '时钟', '设置']); return r.join() === '电话,时钟' && r.length === 2; } }),
    memoized({ id: 'F08579', name: '常用功能聚合', check: () => D.simplifiedDesktop(['家人']).join() === '家人' }),
    memoized({ id: 'F08580', name: '大按钮 64px', check: () => D.elderButtonSize() === 64 }),
    memoized({ id: 'F08581', name: '语音入口置顶', check: () => D.voiceFirstOrder(['相机', '语音助手'])[0] === '语音助手' && D.voiceFirstOrder(['a', 'b']).length === 3 }),
    memoized({ id: 'F08582', name: '防误触双确认', check: () => D.elderConfirmPolicy().doubleConfirm && D.elderConfirmPolicy().minGapMs === 500 }),
    memoized({ id: 'F08583', name: '慢动画 0.6x', check: () => D.elderMotionScale() === 0.6 }),
    memoized({ id: 'F08584', name: '老年减弹窗', check: () => D.elderDialogPolicy(false, 3) === 'batch' && D.elderDialogPolicy(false, 1) === 'show' && D.elderDialogPolicy(true, 9) === 'show' }),
    memoized({ id: 'F08585', name: '大音量上限 0.9', check: () => D.elderVolumeBoost(0.6) === 0.8 && D.elderVolumeBoost(0.85) === 0.9 }),
    memoized({ id: 'F08586', name: '点读命中', check: () => D.tapToRead(['药盒', '水杯'], 1) === '水杯' && D.tapToRead(['药盒'], 5) === null }),
    memoized({ id: 'F08587', name: '远程协助位预留', check: () => D.ELDER_REMOTE_SLOT.reserved && !D.ELDER_REMOTE_SLOT.enabled }),
    memoized({ id: 'F08588', name: '家人管理授权', check: () => fm.add('女儿') && !fm.add('女儿') && fm.canManage('女儿') && !fm.canManage('儿子') }),
    memoized({ id: 'F08589', name: '用药三重提醒', check: () => D.medicationReminder(600, 590).due && D.medicationReminder(600, 590).prompts === 3 && !D.medicationReminder(500, 590).due }),
    memoized({ id: 'F08590', name: '防诈骗评分', check: () => { const r = D.scamGuard({ asksMoney: true, urgentTone: true, unknownSender: false, hasLink: false }); return r.risk === 2 && r.warn && !D.scamGuard({ asksMoney: false, urgentTone: false, unknownSender: false, hasLink: false }).warn; } }),
    memoized({ id: 'F08591', name: '长按确认删除', check: () => D.elderDeleteConfirm(1500) && !D.elderDeleteConfirm(800) }),
    memoized({ id: 'F08592', name: '简化设置', check: () => D.simplifiedSettings(['音量', '网络', '字体大小']).join() === '音量,字体大小' }),
    memoized({ id: 'F08593', name: '老年教学五步', check: () => D.ELDER_TUTORIAL.length === 5 }),
    memoized({ id: 'F08594', name: '老年审计五项', check: () => D.EDU_ELDER_AUDIT.length === 5 }),
    memoized({ id: 'F08595', name: '老年测试用例', check: () => D.ELDER_TESTS.length === 5 }),
    memoized({ id: 'F08596', name: '老年回归清单', check: () => D.ELDER_REGRESSION.length === 3 }),
    memoized({ id: 'F08597', name: '老年文档页', check: () => D.ELDER_DOCS.length === 3 }),
    memoized({ id: 'F08598', name: '老年彩蛋低频', check: () => D.elderEgg(false, 53) === null && D.elderEgg(true, 106) !== null && D.elderEgg(true, 1) === null }),
    memoized({ id: 'F08599', name: '老年收官清单', check: () => D.ELDER_FINALE.length === 5 }),
    memoized({ id: 'F08600', name: '老年致谢', check: () => D.ELDER_THANKS.length === 3 }),
  ];
}

export function checkF0345(): CheckEntry[] {
  const st = new D.ScreenTime(60);
  const badges = new D.KidBadges();
  const parental = new D.ParentalControl('1234');
  return [
    memoized({ id: 'F08601', name: '儿童大字 140%', check: () => D.kidLargeText() === 140 }),
    memoized({ id: 'F08602', name: '儿童语音预设', check: () => D.kidVoicePreset().voice === 'child' && D.kidVoicePreset().rate === 0.9 }),
    memoized({ id: 'F08603', name: '儿童简化入口', check: () => D.kidSimplified(['学习', '商店', '故事']).join() === '学习,故事' }),
    memoized({ id: 'F08604', name: '儿童防误触', check: () => D.kidMistouchPolicy().minGapMs === 600 && D.kidMistouchPolicy().bigTargets }),
    memoized({ id: 'F08605', name: '时长到期锁定', check: () => { st.use(50); const r1 = st.remaining; return r1 === 10 && !st.locked && (st.use(10), (st.remaining as number) === 0 && (st.use(5), st.remaining === 0)); } }),
    memoized({ id: 'F08606', name: '分级过滤', check: () => D.kidContentFilter('kid', 'kid') && !D.kidContentFilter('kid', 'teen') && D.kidContentFilter('teen', 'teen') }),
    memoized({ id: 'F08607', name: '跟读编辑距离', check: () => D.readAloudScore('你好', '你好') === 100 && D.readAloudScore('你号', '你好') === 50 && D.readAloudScore('', '你好') === 0 }),
    memoized({ id: 'F08608', name: '楷体学习字体', check: () => D.KID_FONT === '楷体' }),
    memoized({ id: 'F08609', name: '护眼参数', check: () => D.kidEyeCare().colorTemp === 4000 && D.kidEyeCare().breakEveryMin === 20 }),
    memoized({ id: 'F08610', name: '专注屏蔽', check: () => { const f = D.kidFocus(['弹窗', '通知']); return f.active && f.blocked.length === 2 && !D.kidFocus([]).active; } }),
    memoized({ id: 'F08611', name: '徽章去重', check: () => badges.earn('阅读之星') && !badges.earn('阅读之星') && badges.earn('早睡') && badges.count === 2 }),
    memoized({ id: 'F08612', name: '家长 PIN 与周报', check: () => parental.unlock('1234') && !parental.unlock('0000') && parental.weeklyReport(200).level === 'low' && parental.weeklyReport(500).level === 'ok' && parental.weeklyReport(900).level === 'high' }),
    memoized({ id: 'F08613', name: '儿童教学五步', check: () => D.KID_TUTORIAL.length === 5 }),
    memoized({ id: 'F08614', name: '儿童审计五项', check: () => D.EDU_KID_AUDIT.length === 5 }),
    memoized({ id: 'F08615', name: '儿童测试用例', check: () => D.KID_TESTS.length === 5 }),
    memoized({ id: 'F08616', name: '儿童回归清单', check: () => D.KID_REGRESSION.length === 3 }),
    memoized({ id: 'F08617', name: '儿童文档页', check: () => D.KID_DOCS.length === 3 }),
    memoized({ id: 'F08618', name: '儿童彩蛋低频', check: () => D.kidEgg(false, 47) === null && D.kidEgg(true, 94) !== null && D.kidEgg(true, 1) === null }),
    memoized({ id: 'F08619', name: '儿童 API 冻结', check: () => D.KID_API_VERSION === '1.0' }),
    memoized({ id: 'F08620', name: '儿童案例', check: () => D.KID_CASES.length === 3 }),
    memoized({ id: 'F08621', name: '儿童收官清单', check: () => D.KID_FINALE.length === 5 }),
    memoized({ id: 'F08622', name: '儿童致谢', check: () => D.KID_THANKS.length === 3 }),
    memoized({ id: 'F08623', name: '儿童研究位预留', check: () => D.KID_RESEARCH_SLOT.reserved && !D.KID_RESEARCH_SLOT.enabled }),
    memoized({ id: 'F08624', name: '儿童日历', check: () => D.KID_CALENDAR.length === 3 && D.KID_CALENDAR[1]!.includes('儿童节') }),
    memoized({ id: 'F08625', name: '儿童二期收官', check: () => D.KID_FINALE_2.length === 3 }),
  ];
}
