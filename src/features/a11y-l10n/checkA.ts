// AURORA-10000: AI-66 批次（族0326~0330）自检条目，勿删。

import { CheckEntry, memoized } from './types';
import * as A from './groupA';

export function checkF0326(): CheckEntry[] {
  const mag = new A.Magnifier();
  return [
    memoized({ id: 'F08126', name: '放大镜三形态', check: () => ['fullscreen', 'lens', 'docked'].includes(mag.setMode('docked').mode) }),
    memoized({ id: 'F08127', name: '倍率 2~16 钳位', check: () => mag.setZoom(1) === 2 && mag.setZoom(32) === 16 && mag.setZoom(8) === 8 }),
    memoized({ id: 'F08128', name: '跟随光标/键盘/焦点', check: () => mag.setFollow('focus').follow === 'focus' && mag.setFollow('keyboard').follow === 'keyboard' }),
    memoized({ id: 'F08129', name: '色盲滤镜三矩阵', check: () => { const p = A.applyCvdFilter('protanopia', [255, 0, 0]); return A.CVD_MATRICES.deuteranopia.length === 9 && p[0]! < 255 && p[1]! > 0; } }),
    memoized({ id: 'F08130', name: '高对比主题令牌', check: () => A.HIGH_CONTRAST_TOKENS.length === 5 }),
    memoized({ id: 'F08131', name: '焦点环强化', check: () => { const r = A.focusRing(1); return r.width === 2 && r.minContrast === 4.5; } }),
    memoized({ id: 'F08132', name: '光标加粗 1~6', check: () => A.caretWidth(0) === 1 && A.caretWidth(9) === 6 && A.caretWidth(3) === 3 }),
    memoized({ id: 'F08133', name: '闪烁限制 ≤3/s', check: () => A.flashAllowed(3) && !A.flashAllowed(4) }),
    memoized({ id: 'F08134', name: '减动效归零', check: () => { const r = A.reduceMotion(true, 200, 12); return r.durMs === 0 && r.distPx === 0 && A.reduceMotion(false, 200, 12).durMs === 200; } }),
    memoized({ id: 'F08135', name: '光敏保护拦截', check: () => A.photosensitivityGuard([0, 1, 0, 1, 0]) === false && A.photosensitivityGuard([0, 0.1, 0.2, 0.3, 0.4]) === true }),
    memoized({ id: 'F08136', name: '指针加大 16~96', check: () => A.pointerSize(8) === 16 && A.pointerSize(200) === 96 && A.pointerSize(48) === 48 }),
    memoized({ id: 'F08137', name: '指针轨迹截取', check: () => A.pointerTrail([[1, 1], [2, 2], [3, 3]], 2).length === 2 }),
    memoized({ id: 'F08138', name: '点击涟漪半径', check: () => A.clickRipple(0) === 8 && A.clickRipple(1) === 32 }),
    memoized({ id: 'F08139', name: '焦点高亮令牌', check: () => A.FOCUS_HIGHLIGHT_TOKEN.startsWith('--a11y-') }),
    memoized({ id: 'F08140', name: '朗读位置九宫描述', check: () => A.announcePosition({ x: 10, y: 10 }, { w: 300, h: 300 }) === '上方左侧' && A.announcePosition({ x: 150, y: 150 }, { w: 300, h: 300 }) === '中间中部' }),
    memoized({ id: 'F08141', name: '放大热键表', check: () => A.MAGNIFIER_HOTKEYS['win+esc'] === 'exit' && Object.keys(A.MAGNIFIER_HOTKEYS).length === 3 }),
    memoized({ id: 'F08142', name: '放大配置文件存取', check: () => { const p = new A.MagnifierProfiles(); p.save('a', 'lens', 4); return p.load('a')!.zoom === 4 && p.load('b') === null && p.size === 1; } }),
    memoized({ id: 'F08143', name: '读屏联动跟随', check: () => A.linkScreenReader(true, { x: 1, y: 2 }) !== null && A.linkScreenReader(false, { x: 1, y: 2 }) === null }),
    memoized({ id: 'F08144', name: '低视力指南五步', check: () => A.LOW_VISION_GUIDE.length === 5 }),
    memoized({ id: 'F08145', name: '全局大字 100~200%', check: () => A.globalFontScale(50) === 100 && A.globalFontScale(250) === 200 }),
    memoized({ id: 'F08146', name: '字重 400~900', check: () => A.fontWeight(100) === 400 && A.fontWeight(1000) === 900 }),
    memoized({ id: 'F08147', name: '行距 1.0~2.5', check: () => A.lineHeight(0.5) === 1 && A.lineHeight(3) === 2.5 && A.lineHeight(1.5) === 1.5 }),
    memoized({ id: 'F08148', name: '字距 0~0.3em', check: () => A.letterSpacing(-1) === 0 && A.letterSpacing(1) === 0.3 }),
    memoized({ id: 'F08149', name: '视障教学五步', check: () => A.VISION_TUTORIAL.length === 5 }),
    memoized({ id: 'F08150', name: '视障收官清单', check: () => A.VISION_FINALE.length === 5 }),
  ];
}

export function checkF0327(): CheckEntry[] {
  const ce = new A.CaptionEngine();
  ce.toggle(true);
  ce.push(0, 2, '叮咚'); ce.push(2.5, 4, '门铃');
  const ts = new A.TranscriptionSession();
  ts.add('甲', '你好');
  return [
    memoized({ id: 'F08151', name: '系统级环境字幕', check: () => ce.enabled && ce.at(1)!.text === '叮咚' && ce.at(3)!.text === '门铃' && ce.at(4.5) === null }),
    memoized({ id: 'F08152', name: '字幕样式可调', check: () => { ce.fontSize = 24; ce.background = 'rgba(0,0,0,0.9)'; return ce.fontSize === 24 && ce.background.includes('0.9'); } }),
    memoized({ id: 'F08153', name: '字幕位置上/下', check: () => { ce.position = 'top'; return ce.position === 'top'; } }),
    memoized({ id: 'F08154', name: '本地实时转写', check: () => ts.local && ts.add('乙', '你好吗') === 2 }),
    memoized({ id: 'F08155', name: '对话双角色转写', check: () => ts.text.includes('甲:你好') && ts.text.includes('乙:你好吗') }),
    memoized({ id: 'F08156', name: 'SRT 导出格式', check: () => { const s = A.exportSrt([{ start: 0, end: 1.5, text: 'hi' }]); return s.includes('1\n') && s.includes('00:00:00,000 --> 00:00:01,500') && s.includes('hi'); } }),
    memoized({ id: 'F08157', name: '视觉铃声脉冲', check: () => A.visualBell().length === 3 && A.visualBell()[2] === 240 }),
    memoized({ id: 'F08158', name: '声音转视觉标签', check: () => A.soundToVisual('error').includes('‼') && A.soundToVisual('alert').includes('⚠') && A.soundToVisual('chime').includes('♪') }),
    memoized({ id: 'F08159', name: '振动模式两档', check: () => A.vibrationPattern('short').length === 3 && A.vibrationPattern('long').length === 1 }),
    memoized({ id: 'F08160', name: '单声道合并', check: () => A.monoDownmix(0.2, 0.6) === 0.4 }),
    memoized({ id: 'F08161', name: '听力辅助增益 0~12dB', check: () => A.hearingBoost(20) === 12 && A.hearingBoost(6) === 6 }),
    memoized({ id: 'F08162', name: '助听兼容位预留', check: () => { const s = A.HEARING_SLOTS.find((x) => x.id === 'F08162')!; return s.reserved && !s.enabled; } }),
    memoized({ id: 'F08163', name: '耳蜗映射位预留', check: () => { const s = A.HEARING_SLOTS.find((x) => x.id === 'F08163')!; return s.reserved && !s.enabled; } }),
    memoized({ id: 'F08164', name: '音量可视化声源', check: () => { const m = A.volumeMeters({ mic: 0.5, media: 2 }); return m.length === 2 && m[1]![1] === 1; } }),
    memoized({ id: 'F08165', name: '声源方向三态', check: () => A.soundDirection(0.9, 0.1) === '左' && A.soundDirection(0.1, 0.9) === '右' && A.soundDirection(0.5, 0.5) === '正前' }),
    memoized({ id: 'F08166', name: '音量突发平滑', check: () => A.volumeSmooth(0.5, 1) === 0.7 && A.volumeSmooth(0.5, 0) === 0.3 && A.volumeSmooth(0.5, 0.6) === 0.6 }),
    memoized({ id: 'F08167', name: '语音消息转文字', check: () => A.speechToText(ts).includes('你好吗') }),
    memoized({ id: 'F08168', name: '会议字幕位预留', check: () => { const s = A.HEARING_SLOTS.find((x) => x.id === 'F08168')!; return s.reserved && !s.enabled; } }),
    memoized({ id: 'F08169', name: '听障教学五步', check: () => A.HEARING_TUTORIAL.length === 5 }),
    memoized({ id: 'F08170', name: '听障审计五项', check: () => A.HEARING_AUDIT.length === 5 }),
    memoized({ id: 'F08171', name: '听障测试用例', check: () => A.HEARING_TESTS.length === 5 }),
    memoized({ id: 'F08172', name: '听障文档页', check: () => A.HEARING_DOCS.length === 3 }),
    memoized({ id: 'F08173', name: '听障回归清单', check: () => A.HEARING_REGRESSION.length === 3 }),
    memoized({ id: 'F08174', name: '听障彩蛋低频', check: () => A.hearingEgg(false, 0) === null && A.hearingEgg(true, 97 * 2) !== null && A.hearingEgg(true, 1) === null }),
    memoized({ id: 'F08175', name: '听障收官清单', check: () => A.HEARING_FINALE.length === 5 }),
  ];
}

export function checkF0328(): CheckEntry[] {
  const sc = new A.SwitchScanner();
  sc.load(['A', 'B', 'C']);
  const sticky = new A.StickyKeys();
  const guard = new A.MistouchGuard();
  const cmds = new A.VoiceCommandSet();
  return [
    memoized({ id: 'F08176', name: '开关扫描全系统', check: () => sc.items.length === 3 && sc.step() === 'A' }),
    memoized({ id: 'F08177', name: '单开关模式', check: () => { sc.mode = 'single'; return sc.press() === 'B'; } }),
    memoized({ id: 'F08178', name: '双开关模式', check: () => { const s2 = new A.SwitchScanner(); s2.load(['x', 'y', 'z']); s2.mode = 'dual'; return s2.press() === 'x' && s2.press() === 'y'; } }),
    memoized({ id: 'F08179', name: '扫描速度可调', check: () => { sc.intervalMs = 400; return sc.intervalMs === 400; } }),
    memoized({ id: 'F08180', name: '自动扫描推进', check: () => { const s = new A.SwitchScanner(); s.load(['x', 'y']); s.step(); s.step(); return s.select() === 'x'; } }),
    memoized({ id: 'F08181', name: '驻留触发 dwell', check: () => { const d = new A.DwellClick(800); return d.move(0) === 'holding' && d.move(400) === 'holding' && d.move(800) === 'fired'; } }),
    memoized({ id: 'F08182', name: '眼动点击位预留', check: () => { const s = A.MOTION_SLOTS.find((x) => x.id === 'F08182')!; return s.reserved && !s.enabled; } }),
    memoized({ id: 'F08183', name: '语音控制全集', check: () => cmds.match('打开设置') === 'settings.open' }),
    memoized({ id: 'F08184', name: '自定义命令', check: () => cmds.add('弹钢琴', 'piano.play') && cmds.match('弹钢琴') === 'piano.play' && !cmds.add('弹钢琴', 'x') }),
    memoized({ id: 'F08185', name: '编号覆盖', check: () => { const o = A.numberOverlay(['保存', '取消']); return o[0]![0] === 1 && o[1]![0] === 2 && o[1]![1] === '取消'; } }),
    memoized({ id: 'F08186', name: '头部控制位预留', check: () => { const s = A.MOTION_SLOTS.find((x) => x.id === 'F08186')!; return s.reserved && !s.enabled; } }),
    memoized({ id: 'F08187', name: '摇杆死区', check: () => A.joystickAxis(0.1) === 0 && A.joystickAxis(1) === 1 && Math.abs(A.joystickAxis(0.6) - 0.529) < 0.01 }),
    memoized({ id: 'F08188', name: '脚踏映射', check: () => A.FOOT_PEDAL_MAP.left === 'step' && A.FOOT_PEDAL_MAP.right === 'cancel' }),
    memoized({ id: 'F08189', name: '粘滞键状态机', check: () => sticky.tap('shift') === 'stuck' && sticky.isStuck('shift') && sticky.tap('shift') === 'locked' && sticky.isLocked('shift') && sticky.tap('shift') === 'released' && !sticky.isStuck('shift') }),
    memoized({ id: 'F08190', name: '慢速键停留生效', check: () => { const s = new A.SlowKeys(500); return !s.accept(0, 300) && s.accept(0, 600); } }),
    memoized({ id: 'F08191', name: '重复键钳位', check: () => { const r = A.keyRepeat(100, 99); return r.delayMs === 250 && r.ratePerSec === 30; } }),
    memoized({ id: 'F08192', name: '双击间隔 200~1000', check: () => A.doubleClickInterval(50) === 200 && A.doubleClickInterval(2000) === 1000 }),
    memoized({ id: 'F08193', name: '键盘拖拽位移', check: () => { const p = A.keyboardDrag({ x: 10, y: 10 }, { dx: -5, dy: 20 }); return p.x === 5 && p.y === 30; } }),
    memoized({ id: 'F08194', name: '长按替代动作', check: () => A.LONG_PRESS_ALT === 'menu.longPressAlternative' }),
    memoized({ id: 'F08195', name: '误触保护抑制', check: () => { guard.tap(1000); guard.tap(1020); return guard.count === 2 && guard.suppressed; } }),
    memoized({ id: 'F08196', name: '自动重复禁用', check: () => A.disableAutoRepeat(true).autoRepeat === false && A.disableAutoRepeat(false).autoRepeat === true }),
    memoized({ id: 'F08197', name: '运动教学五步', check: () => A.MOTION_TUTORIAL.length === 5 }),
    memoized({ id: 'F08198', name: '运动审计五项', check: () => A.MOTION_AUDIT.length === 5 }),
    memoized({ id: 'F08199', name: '运动测试用例', check: () => A.MOTION_TESTS.length === 5 }),
    memoized({ id: 'F08200', name: '运动收官清单', check: () => A.MOTION_FINALE.length === 5 }),
  ];
}

export function checkF0329(): CheckEntry[] {
  const cp = new A.CognitiveProfile();
  const guide = new A.StepGuide(['一', '二']);
  const aid = new A.MemoryAid();
  return [
    memoized({ id: 'F08201', name: '简化模式选项减少', check: () => { cp.setSimplified(true, ['保存']); return cp.visibleOptions(['保存', '导出', '高级']).length === 1 && cp.simplified; } }),
    memoized({ id: 'F08202', name: '专注隐藏非关键', check: () => cp.visibleOptions(['高级']).length === 0 }),
    memoized({ id: 'F08203', name: '分步引导推进', check: () => guide.next() === '一' && guide.next() === '二' && guide.done }),
    memoized({ id: 'F08204', name: '重复提示计数', check: () => A.repeatHint(1).startsWith('提示') && A.repeatHint(2).includes('第 2 次') }),
    memoized({ id: 'F08205', name: '记忆辅助回放', check: () => { aid.record('打开了设置'); aid.record('改了字号'); return aid.recap(1)[0] === '改了字号' && aid.recap(5).length === 2; } }),
    memoized({ id: 'F08206', name: '阅读行聚焦', check: () => A.readingLineFocus(['a', 'b'], 1) === 'b' && A.readingLineFocus(['a'], 5) === null }),
    memoized({ id: 'F08207', name: '阅读标尺范围', check: () => { const r = A.readingRuler(1000, 200); return r.top === 176 && r.bottom === 224; } }),
    memoized({ id: 'F08208', name: '行高亮令牌', check: () => A.READING_HIGHLIGHT_TOKEN.startsWith('--a11y-') }),
    memoized({ id: 'F08209', name: '音节划分位预留', check: () => A.SYLLABLE_SLOT.reserved && !A.SYLLABLE_SLOT.enabled }),
    memoized({ id: 'F08210', name: '易读字体档', check: () => A.EASY_READ_FONTS.length === 3 }),
    memoized({ id: 'F08211', name: '大间隔 1.5x', check: () => A.relaxedSpacing(8) === 12 }),
    memoized({ id: 'F08212', name: '减少弹窗聚合', check: () => A.dialogPolicy(false, 5) === 'batch' && A.dialogPolicy(true, 5) === 'show' && A.dialogPolicy(false, 1) === 'show' }),
    memoized({ id: 'F08213', name: '图文双语对', check: () => { const p = A.pictureText('保存', '💾'); return p.icon === '💾' && p.text === '保存'; } }),
    memoized({ id: 'F08214', name: '平静模式降饱和', check: () => { const c = A.calmMode(true); return c.saturation === 0.6 && c.motionScale === 0.3 && A.calmMode(false).saturation === 1; } }),
    memoized({ id: 'F08215', name: '可预测导航一致', check: () => A.predictableNav(['bar', 'bar']) && !A.predictableNav(['bar', 'tabs']) }),
    memoized({ id: 'F08216', name: '撤销友好标记', check: () => { const u = A.undoFriendly('删除文件'); return u.confirm && u.undoable; } }),
    memoized({ id: 'F08217', name: '时间放宽 3x', check: () => cp.timeoutMs(30000) === 90000 && cp.setSimplified(false, []).length === 0 && cp.timeoutMs(30000) === 30000 }),
    memoized({ id: 'F08218', name: '认知教学五步', check: () => A.COGNITIVE_TUTORIAL.length === 5 }),
    memoized({ id: 'F08219', name: '认知审计五项', check: () => A.COGNITIVE_AUDIT.length === 5 }),
    memoized({ id: 'F08220', name: '认知测试用例', check: () => A.COGNITIVE_TESTS.length === 5 }),
    memoized({ id: 'F08221', name: '认知回归清单', check: () => A.COGNITIVE_REGRESSION.length === 3 }),
    memoized({ id: 'F08222', name: '认知文档页', check: () => A.COGNITIVE_DOCS.length === 3 }),
    memoized({ id: 'F08223', name: '认知彩蛋低频', check: () => A.cognitiveEgg(false, 89) === null && A.cognitiveEgg(true, 89 * 3) !== null && A.cognitiveEgg(true, 2) === null }),
    memoized({ id: 'F08224', name: '认知收官清单', check: () => A.COGNITIVE_FINALE.length === 5 }),
    memoized({ id: 'F08225', name: '认知致谢名单', check: () => A.COGNITIVE_THANKS.length === 3 }),
  ];
}

export function checkF0330(): CheckEntry[] {
  const vc = new A.VoiceControl();
  vc.enabled = true;
  vc.setOverlay(['文件', '编辑', '视图']);
  return [
    memoized({ id: 'F08226', name: '语音控制全集', check: () => vc.enabled && vc.local && Object.keys(A.BASE_VOICE_COMMANDS).length === 4 }),
    memoized({ id: 'F08227', name: '语音导航跳到', check: () => vc.command('跳到 文件', ['文件', '编辑']) === 'focus:文件' && vc.command('跳到 帮助', ['文件']) === null }),
    memoized({ id: 'F08228', name: '语音听写', check: () => vc.dictate('今天天气好') === 1 && vc.dictate('心情好') === 2 }),
    memoized({ id: 'F08229', name: '语音编辑选中', check: () => vc.command('选中 该段', []) === 'select:该段' }),
    memoized({ id: 'F08230', name: '语音编号覆盖', check: () => vc.overlay.length === 3 && vc.overlay[2]![0] === 3 }),
    memoized({ id: 'F08231', name: '命令自定义注册', check: () => A.addVoiceCommand(vc.custom, '深呼吸', 'focus.breathe') && vc.command('深呼吸', []) === 'focus.breathe' }),
    memoized({ id: 'F08232', name: '操作反馈确认', check: () => vc.feedback('已保存').includes('已保存') }),
    memoized({ id: 'F08233', name: '免手模式', check: () => { vc.handsFree = true; return vc.handsFree; } }),
    memoized({ id: 'F08234', name: '语速 0.5~2.0', check: () => { vc.speed = A.voiceSpeed(5); return vc.speed === 2 && A.voiceSpeed(0.1) === 0.5; } }),
    memoized({ id: 'F08235', name: '口音适应加权', check: () => { const p = A.accentAdapt({}, '四', '十'); return p['四'] === 1 && p['十'] === 0.5; } }),
    memoized({ id: 'F08236', name: '离线模型过滤', check: () => A.voiceOffline([{ name: 'zh-small', onDevice: true }, { name: 'cloud', onDevice: false }]).join() === 'zh-small' }),
    memoized({ id: 'F08237', name: '本地隐私承诺', check: () => A.VOICE_PRIVACY.upload === false && A.VOICE_PRIVACY.telemetry === false && A.VOICE_PRIVACY.retention === 'memory-only' }),
    memoized({ id: 'F08238', name: '语音教学五步', check: () => A.VOICE_TUTORIAL.length === 5 }),
    memoized({ id: 'F08239', name: '语音审计五项', check: () => A.VOICE_AUDIT.length === 5 }),
    memoized({ id: 'F08240', name: '语音测试用例', check: () => A.VOICE_TESTS.length === 5 }),
    memoized({ id: 'F08241', name: '语音回归清单', check: () => A.VOICE_REGRESSION.length === 3 }),
    memoized({ id: 'F08242', name: '语音文档页', check: () => A.VOICE_DOCS.length === 3 }),
    memoized({ id: 'F08243', name: '语音彩蛋口令', check: () => A.voiceEgg(false, '芝麻开门') === null && A.voiceEgg(true, '芝麻开门') !== null && A.voiceEgg(true, '你好') === null }),
    memoized({ id: 'F08244', name: '眼动组合位预留', check: () => { const s = A.VOICE_SLOTS.find((x) => x.id === 'F08244')!; return s.reserved && !s.enabled; } }),
    memoized({ id: 'F08245', name: '语音+扫描组合', check: () => A.voiceScanCombo(true, true) && !A.voiceScanCombo(true, false) }),
    memoized({ id: 'F08246', name: '语音 API 冻结', check: () => A.VOICE_API_VERSION === '1.0' }),
    memoized({ id: 'F08247', name: '反馈音两态', check: () => A.voiceFeedbackTone(true) === 'tick' && A.voiceFeedbackTone(false) === 'thud' }),
    memoized({ id: 'F08248', name: '语音收官清单', check: () => A.VOICE_FINALE.length === 5 }),
    memoized({ id: 'F08249', name: '语音致谢名单', check: () => A.VOICE_THANKS.length === 3 }),
    memoized({ id: 'F08250', name: '语音实验位预留', check: () => { const s = A.VOICE_SLOTS.find((x) => x.id === 'F08250')!; return s.reserved && !s.enabled; } }),
  ];
}
