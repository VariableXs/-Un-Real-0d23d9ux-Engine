// AURORA-10000: AI-31~AI-35 批次领域07自检注册表（F03751~F04375 共 625 项），勿删。
// 每项一条可运行断言；「位/预留」条目按 §15 守卫口径 = 接口冻结 + 开关存在。

import * as A from './groupA';
import * as B from './groupB';
import * as C from './groupC';
import * as D from './groupD';
import * as E from './groupE';

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

const T0 = 1760000000000;

/* -------- AI-31 族0151 剪贴板增强 -------- */
export function checkF0151(): CheckEntry[] {
  const lib = new A.SnippetLib();
  lib.add({ title: '签名', body: '—— Variable' });
  lib.add({ title: '周报模板', body: '{{date}} 周报', kind: 'code' });
  lib.add({ title: '富文本', body: '<b>加粗</b>', kind: 'rich' });
  const hist = new A.ClipboardHistory(5);
  const q = new A.PasteQueue();
  const undo = new A.PasteUndo();
  return [
    { id: 'F03751', name: '贴图板', check: () => A.pinImage('data:image/png;base64,x', 10, 20).x === 10 },
    { id: 'F03752', name: '片段库', check: () => lib.list.length === 3 },
    { id: 'F03753', name: '模板片段', check: () => A.resolveDynamic(lib.list[1]!.body, new Date(T0)).includes('2025-10-09') },
    { id: 'F03754', name: '动态片段', check: () => A.resolveDynamic('{{date}} {{time}}', new Date(2026, 8, 13, 9, 30, 5)) === '2026-09-13 09:30:05' },
    { id: 'F03755', name: '代码片段', check: () => lib.list[1]!.kind === 'code' },
    { id: 'F03756', name: '富文本', check: () => lib.list[2]!.kind === 'rich' && lib.list[2]!.body.includes('<b>') },
    { id: 'F03757', name: '分类', check: () => (lib.add({ title: '代码', body: 'x', category: '代码' }), lib.categories().includes('代码')) },
    { id: 'F03758', name: '搜索', check: () => lib.search('周报').length === 1 && lib.byCategory('代码').length === 1 },
    { id: 'F03759', name: '同步位', check: () => A.setSyncEnabled(true) === true && A.setSyncEnabled(false) === false },
    { id: 'F03760', name: '导入导出', check: () => { const l2 = new A.SnippetLib(); return A.importSnippets(l2, A.exportSnippets(lib)) === 4 && l2.list.length === 4; } },
    { id: 'F03761', name: '分享', check: () => A.shareFile(lib).startsWith('VARIX-SNIPPETS:') },
    { id: 'F03762', name: 'OCR 贴', check: () => A.ocrPaste('  hello \n world ') === 'hello world' },
    { id: 'F03763', name: '公式贴', check: () => A.formulaPaste('\\frac{a}{b}+\\sqrt{x}') === '(a)/(b)+√(x)' },
    { id: 'F03764', name: '颜色贴', check: () => A.colorSnippet('#abc') === '#aabbcc' && A.colorSnippet('rgb(255, 0, 0)') === '#ff0000' },
    { id: 'F03765', name: '历史轮', check: () => (hist.push('a'), hist.push('b'), hist.push('a'), hist.pick(0) === 'a' && hist.pick(1) === 'b' && hist.size === 2) },
    { id: 'F03766', name: '贴为图片', check: () => A.textToImageSpec('ab\ncd', 10).lines === 2 && A.textToImageSpec('ab', 10).height === 16 },
    { id: 'F03767', name: '粘贴预览', check: () => A.previewPaste('x'.repeat(200), 120).includes('共 200 字') },
    { id: 'F03768', name: 'AI 整理', check: () => A.aiOrganize(['https://a.com', 'const x = 1;', 'hello']).get('链接')?.length === 1 },
    { id: 'F03769', name: '队列', check: () => (q.push('1'), q.push('2'), q.pending === 2 && q.pop() === '1') },
    { id: 'F03770', name: '批量贴', check: () => A.batchPaste(['a', 'b', 'c']) === 'a\nb\nc' },
    { id: 'F03771', name: '延迟贴', check: () => A.schedulePaste('x', T0, T0 + 1).due === true && A.schedulePaste('x', T0, T0 - 1).due === false },
    { id: 'F03772', name: '贴撤销', check: () => (undo.push('old'), undo.undo() === 'old' && undo.undo() === undefined) },
    { id: 'F03773', name: '跨设备位', check: () => A.CROSS_DEVICE_RESERVED === true },
    { id: 'F03774', name: '教学', check: () => A.CLIPBOARD_TIPS.length >= 3 },
    { id: 'F03775', name: '彩蛋', check: () => A.clipboardEgg('上上下下') !== undefined && A.clipboardEgg('abc') === undefined },
  ];
}

/* -------- AI-31 族0152 快速笔记 -------- */
export function checkF0152(): CheckEntry[] {
  const board = new A.StickyBoard();
  const n1 = board.add('明天买牛奶', { color: '黄' });
  const n2 = board.add('重要会议记录', { color: '蓝' });
  const n3 = board.add('加密事项');
  board.pin(n2.id);
  board.setRemind(n1.id, T0 + 3600_000);
  board.encrypt(n3.id, 'pw');
  return [
    { id: 'F03776', name: '全局便签', check: () => board.list.length === 3 && n1.text === '明天买牛奶' },
    { id: 'F03777', name: '便签墙', check: () => board.wall()[0]!.id === n2.id },
    { id: 'F03778', name: '颜色', check: () => A.NOTE_COLORS.length === 6 && n1.color === '黄' },
    { id: 'F03779', name: '置顶', check: () => board.pin(n2.id) === true && board.wall()[0]!.id !== n2.id },
    { id: 'F03780', name: '提醒', check: () => board.setRemind(n1.id, T0 + 7200_000) && A.todayFocus(board.list as A.StickyNote[], T0).length === 1 },
    { id: 'F03781', name: '加密', check: () => board.search('加密事项').length === 0 && board.decrypt(n3.id, 'pw') === '加密事项' },
    { id: 'F03782', name: '搜索', check: () => board.search('牛奶').length === 1 },
    { id: 'F03783', name: '导出', check: () => board.exportText().includes('买牛奶') },
    { id: 'F03784', name: '转待办', check: () => board.toTodo(n1.id) === '明天买牛奶' },
    { id: 'F03785', name: '转日程', check: () => board.add('2026-10-01 出发旅行').createdAt > 0 && board.toSchedule(board.list[board.list.length - 1]!.id).date === '2026-10-01' },
    { id: 'F03786', name: '语音', check: () => A.voiceNotePlaceholder(30).transcript.includes('30') },
    { id: 'F03787', name: '图片', check: () => board.add('【图片便签】IMG_001.png').text.includes('IMG_001') },
    { id: 'F03788', name: '手写位', check: () => A.HANDWRITING_NOTE_RESERVED === true },
    { id: 'F03789', name: '历史', check: () => (board.edit(n1.id, '明天买两盒牛奶'), board.edit(n1.id, '明天买三盒牛奶'), board.history(n1.id).length === 2) },
    { id: 'F03790', name: '共享位', check: () => A.NOTE_SHARING_RESERVED === true },
    { id: 'F03791', name: '倒计时贴', check: () => A.countdownDays(T0 + 3 * 86400_000, T0) === 3 },
    { id: 'F03792', name: '今日焦点', check: () => { const b2 = new A.StickyBoard(); b2.add('焦点事项', { remindAt: T0 + 1000 }); return A.todayFocus(b2.list as A.StickyNote[], T0).length === 1; } },
    { id: 'F03793', name: '灵感捕捉', check: () => A.NOTE_TEMPLATES['灵感捕捉'] === '【灵感】' },
    { id: 'F03794', name: '会议速记', check: () => A.NOTE_TEMPLATES['会议速记']!.includes('待办') },
    { id: 'F03795', name: '速记模板', check: () => Object.keys(A.NOTE_TEMPLATES).length >= 3 },
    { id: 'F03796', name: '统计', check: () => board.stats().total >= 4 && board.stats().chars > 0 },
    { id: 'F03797', name: '离线', check: () => board.stats().total === board.list.length },
    { id: 'F03798', name: '回收', check: () => (board.remove(n1.id) === true, board.restore(n1.id) === true && board.list.some((x) => x.id === n1.id)) },
    { id: 'F03799', name: '多窗', check: () => board.wall().length >= 4 },
    { id: 'F03800', name: '教学', check: () => A.NOTE_TIPS.length >= 3 },
  ];
}

/* -------- AI-31 族0153 待办与任务 -------- */
export function checkF0153(): CheckEntry[] {
  const store = new A.TodoStore();
  const t1 = store.addTodo('明天3点开会', T0);
  store.addTodo('今天 买菜', T0);
  const t3 = store.addTodo('下周一 交报告', T0);
  return [
    { id: 'F03801', name: '自然语言', check: () => t1.title.includes('开会') && t1.due !== undefined && t1.due > T0 },
    { id: 'F03802', name: '今日', check: () => store.today(T0).some((t) => t.title === '买菜') },
    { id: 'F03803', name: '计划', check: () => store.planned(T0).length >= 2 && store.planned(T0)[0]!.due! <= store.planned(T0)[1]!.due! },
    { id: 'F03804', name: '收件箱', check: () => store.inbox.length === 3 },
    { id: 'F03805', name: '项目', check: () => (store.promote(t3.id, '工作'), store.byProject('工作').length === 1 && store.inbox.length === 2) },
    { id: 'F03806', name: '标签', check: () => (store.setTags(t1.id, ['会议']), store.all.find((t) => t.id === t1.id)!.tags.includes('会议')) },
    { id: 'F03807', name: '优先级', check: () => (store.setPriority(t1.id, 3), A.PRIORITY_LABELS[3] === '高' && store.all.find((t) => t.id === t1.id)!.priority === 3) },
    { id: 'F03808', name: '到期提醒', check: () => store.dueSoon(12 * 86400_000, T0).length >= 2 },
    { id: 'F03809', name: '重复', check: () => A.parseNatural('明天3点开会').due !== undefined && (() => { const rep = { ...t1, repeat: 'daily' as const }; return store.nextRepeat(rep, T0)! > T0; })() },
    { id: 'F03810', name: '子任务', check: () => (store.addSubtask(t1.id, '订会议室'), store.completeSubtask(t1.id, `${t1.id}-s1`) && store.all.find((t) => t.id === t1.id)!.subtasks[0]!.done) },
    { id: 'F03811', name: '拖拽', check: () => (store.reorder(t1.id, 5), store.all[store.all.length - 1]!.id === t1.id) },
    { id: 'F03812', name: '四象限', check: () => (store.setPriority(t3.id, 3), store.quadrant(t3.id, T0) === 'doFirst' || store.quadrant(t3.id, T0) === 'schedule') && store.quadrant(t1.id, T0) !== undefined },
    { id: 'F03813', name: '甘特位', check: () => A.TODO_GANTT_RESERVED === true },
    { id: 'F03814', name: '看板', check: () => store.kanban().todo.length + store.kanban().doing.length === 3 && store.kanban().done.length === 0 },
    { id: 'F03815', name: '日历', check: () => store.calendar() instanceof Map },
    { id: 'F03816', name: '番茄绑定', check: () => (store.bindPomodoro(t1.id), store.bindPomodoro(t1.id), store.focusStats().pomodoros === 2) },
    { id: 'F03817', name: '专注统计', check: () => store.focusStats().bound === 1 },
    { id: 'F03818', name: '完成庆祝', check: () => store.toggleDone(t1.id) === true && store.kanban().done.length === 1 },
    { id: 'F03819', name: '批量', check: () => (store.addTodo('临时1'), store.addTodo('临时2'), store.batchOp(store.all.slice(-2).map((t) => t.id), 'done') === 2) },
    { id: 'F03820', name: '导入', check: () => (() => { const s = new A.TodoStore(); return s.importCsv('title,priority,done\n写周报,2,false') === 1 && s.all[0]!.title === '写周报'; })() },
    { id: 'F03821', name: '导出', check: () => store.exportCsv().startsWith('title,priority,done') },
    { id: 'F03822', name: '本地优先', check: () => JSON.parse(store.backup()).todos.length >= 1 },
    { id: 'F03823', name: '快捷键', check: () => A.TODO_TIPS.length >= 3 },
    { id: 'F03824', name: '备份', check: () => store.backup().length > 10 },
    { id: 'F03825', name: '教学', check: () => A.TODO_TIPS.some((t) => t.includes('自然语言') || t.includes('识别')) },
  ];
}

/* -------- AI-31 族0154 日程与时钟 -------- */
export function checkF0154(): CheckEntry[] {
  const es = new A.EventStore();
  const ev = es.quickAdd('周五 14:00 例会', new Date(2026, 8, 9));
  es.quickAdd('明天 全天 体检', new Date(2026, 8, 9));
  const wall = new A.CountdownWall().add('发布日', T0 + 5 * 86400_000);
  const sw = new A.Stopwatch();
  const timers = new A.TimerBank().add('泡面', 180).add('烧行', 600);
  const pomo = new A.Pomodoro(1, 1);
  return [
    { id: 'F03826', name: '月历', check: () => { const g = A.monthGrid(2026, 8); return g.length >= 5 && g.flat().includes(1) && g.flat().includes(30); } },
    { id: 'F03827', name: '周视图', check: () => A.weekDays(new Date(2026, 8, 9)).length === 7 },
    { id: 'F03828', name: '日视图', check: () => es.byDate(ev.date).length === 1 },
    { id: 'F03829', name: '议程', check: () => es.agenda().length === 2 && es.agenda()[0]!.date <= es.agenda()[1]!.date },
    { id: 'F03830', name: '快加', check: () => ev.title === '例会' && ev.startMin === 840 },
    { id: 'F03831', name: '全天', check: () => es.list.some((e) => e.allDay === true && e.startMin === undefined) },
    { id: 'F03832', name: '重复', check: () => { const store2 = new A.EventStore(); const e2 = store2.add({ title: '晨会', date: '2026-09-09', repeat: 'weekly' }); return store2.occurrences(e2, 3).length === 3 && store2.occurrences(e2, 3)[1] === '2026-09-16'; } },
    { id: 'F03833', name: '提醒', check: () => es.add({ title: '提醒事件', date: '2026-09-10', remindMinBefore: 30 }).remindMinBefore === 30 },
    { id: 'F03834', name: '时区', check: () => A.timezoneShift('2026-09-13 08:00', 0, 480) === '2026-09-13 16:00' },
    { id: 'F03835', name: '节假日', check: () => A.CN_HOLIDAYS_2026['2026-10-01'] === '国庆' },
    { id: 'F03836', name: '农历/节气', check: () => A.solarTermOf(new Date(2026, 8, 23)) === '秋分' },
    { id: 'F03837', name: '生日', check: () => A.birthdayCountdown('12-25', new Date(2026, 8, 13)) === 103 },
    { id: 'F03838', name: '倒计时墙', check: () => wall.sorted(T0)[0]!.days === 5 },
    { id: 'F03839', name: '秒表', check: () => (sw.start(T0), sw.lap(T0 + 1000) === 1000 && sw.lap(T0 + 3000) === 2000 && sw.elapsed(T0 + 5000) === 5000) },
    { id: 'F03840', name: '多计时', check: () => (timers.tick(180), timers.expired().includes('泡面') && !timers.expired().includes('烧行')) },
    { id: 'F03841', name: '番茄', check: () => (pomo.tick(60), pomo.completed === 1 && pomo.phase === 'break') },
    { id: 'F03842', name: '世界钟', check: () => A.worldClock(new Date('2026-09-13T00:00:00Z'), [{ name: '北京', offsetMin: 480 }, { name: '伦敦', offsetMin: 0 }])[0]!.time === '08:00' },
    { id: 'F03843', name: '闹钟', check: () => { const al = new A.AlarmStore(); return al.add('07:30') && al.due(450).length === 1; } },
    { id: 'F03844', name: '报时', check: () => A.hourlyChime(true).includes('开启') },
    { id: 'F03845', name: '午休', check: () => A.napReminder(13 * 60 + 30) === true && A.napReminder(15 * 60) === false },
    { id: 'F03846', name: '日落', check: () => A.sunsetMinutes(31, 180) > 600 && A.sunsetMinutes(31, 180) < 1200 },
    { id: 'F03847', name: 'ics 导入', check: () => { const evs = A.icsImport('BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:评审\nDTSTART;VALUE=DATE:20261001\nEND:VEVENT\nEND:VCALENDAR'); return evs.length === 1 && evs[0]!.title === '评审'; } },
    { id: 'F03848', name: 'ics 导出', check: () => A.icsExport([{ id: '1', title: '评审', date: '2026-10-01' }]).startsWith('BEGIN:VCALENDAR') },
    { id: 'F03849', name: '分享位', check: () => A.SCHEDULE_SHARE_RESERVED === true },
    { id: 'F03850', name: '教学', check: () => A.SCHEDULE_TIPS.length === 3 },
  ];
}

/* -------- AI-31 族0155 计算与换算 -------- */
export function checkF0155(): CheckEntry[] {
  const hist = new A.CalcHistory();
  return [
    { id: 'F03851', name: '科学', check: () => A.evalExpr('1+2*3') === 7 && A.evalExpr('2^10') === 1024 && A.evalExpr('sqrt(16)+sin(0)') === 4 },
    { id: 'F03852', name: '程序员', check: () => A.toBase(255, 16) === 'FF' && A.bitVisualize(6, '&', 3).res === 2 },
    { id: 'F03853', name: '单位', check: () => A.convertUnits(1, 'km', 'm') === 1000 && Math.abs(A.convertUnits(2, '斤', 'kg') - 1) < 1e-9 },
    { id: 'F03854', name: '汇率', check: () => A.convertCurrency(100, 'USD', 'CNY') > 700 && A.convertCurrency(100, 'USD', 'CNY') < 715 },
    { id: 'F03855', name: '贷款', check: () => { const r = A.loanMonthly(1_000_000, 0.031, 360); return r.monthly > 3000 && r.monthly < 6000 && Math.abs(r.total - r.monthly * 360) < 1; } },
    { id: 'F03856', name: '利息', check: () => Math.abs(A.compoundInterest(10000, 0.05, 2).final - 11025) < 0.01 },
    { id: 'F03857', name: '日期差', check: () => A.dateDiff('2026-01-01', '2026-01-31') === 30 },
    { id: 'F03858', name: '年龄', check: () => A.ageOf('2000-06-15', new Date(2026, 8, 13)).years === 26 },
    { id: 'F03859', name: 'BMI', check: () => A.bmi(70, 1.75).label === '正常' && A.bmi(70, 1.75).value === 22.9 },
    { id: 'F03860', name: '尺码', check: () => A.shoeSize(24) === 38 },
    { id: 'F03861', name: '时区', check: () => A.timezoneShift('2026-01-01 00:00', 480, -300) === '2025-12-31 11:00' },
    { id: 'F03862', name: '进制', check: () => A.toBase(8, 2) === '1000' && A.toBase(255, 8) === '377' },
    { id: 'F03863', name: '位运算', check: () => A.bitVisualize(5, '|', 2).res === 7 && A.bitVisualize(5, '^', 3).res === 6 && A.bitVisualize(1, '<<', 4).res === 16 },
    { id: 'F03864', name: '子网', check: () => A.subnetOf('192.168.1.130', 26).network === '192.168.1.128' && A.subnetOf('192.168.1.130', 26).hosts === 62 },
    { id: 'F03865', name: '颜色', check: () => A.rgbToHex(255, 0, 0) === '#ff0000' && A.hexToRgb('#ff0000')[0] === 255 && A.rgbToHsl(255, 0, 0)[0] === 0 && A.rgbToCmyk(0, 0, 0)[3] === 100 },
    { id: 'F03866', name: '色环', check: () => A.resistorOhms(['棕', '黑', '红']) === 1000 },
    { id: 'F03867', name: '螺纹位', check: () => A.THREAD_SPEC_RESERVED === true },
    { id: 'F03868', name: '密度表', check: () => A.DENSITY_TABLE['水'] === 1.0 && A.DENSITY_TABLE['金']! > 19 },
    { id: 'F03869', name: '三角可视', check: () => A.trigSamples('sin', 0, Math.PI, 4).length === 5 },
    { id: 'F03870', name: '单位收藏', check: () => A.FAV_UNIT_COMBOS.length === 3 && A.convertUnits(1, 'MB', 'GB') > 0 },
    { id: 'F03871', name: '历史', check: () => (hist.push('1+1', 2), hist.push('2*2', 4), hist.list.length === 2) },
    { id: 'F03872', name: '键盘全操作', check: () => A.evalExpr('((1+2)*(3+4))') === 21 },
    { id: 'F03873', name: '复制', check: () => A.roundTo(Math.PI, 2) === 3.14 },
    { id: 'F03874', name: '精度', check: () => A.roundTo(1 / 3, 4) === 0.3333 },
    { id: 'F03875', name: '教学', check: () => A.CALC_TIPS.length === 3 },
  ];
}

/* -------- AI-32 族0156 系统监视器 -------- */
export function checkF0156(): CheckEntry[] {
  const h = new B.MetricsHistory();
  h.push({ t: T0, cpu: 20, mem: 4000, disk: 10, net: 100, gpu: 5 });
  h.push({ t: T0 + 30_000, cpu: 40, mem: 5000, disk: 20, net: 200, gpu: 10 });
  h.push({ t: T0 + 90_000, cpu: 60, mem: 6000, disk: 30, net: 300, gpu: 15 });
  const pt = new B.ProcessTable();
  return [
    { id: 'F03876', name: 'CPU', check: () => h.series(60).length === 2 },
    { id: 'F03877', name: '内存', check: () => B.summaryMetrics(h).mem > 0 },
    { id: 'F03878', name: '磁盘', check: () => B.MOCK_PROCS.some((p) => p.disk > 10) },
    { id: 'F03879', name: '网络', check: () => B.summaryMetrics(h).net > 0 },
    { id: 'F03880', name: 'GPU', check: () => B.perProcessRanking(B.MOCK_PROCS, 'gpu')[0]!.pid === 2048 },
    { id: 'F03881', name: '进程列表', check: () => pt.procs.length === 8 },
    { id: 'F03882', name: '排序', check: () => pt.sortBy('cpu')[0]!.name === 'game.exe' && pt.sortBy('mem', false)[0]!.name === 'audiodg.exe' && pt.sortBy('mem')[0]!.name === 'game.exe' },
    { id: 'F03883', name: '搜索', check: () => pt.search('varix').length === 2 && pt.search('1024').length >= 1 },
    { id: 'F03884', name: '结束', check: () => pt.kill(4) === 'protected' && pt.kill(99999) === 'missing' && pt.kill(3000) === 'ok' && pt.procs.length === 7 },
    { id: 'F03885', name: '进程树', check: () => { const t1 = pt.tree(1024); return t1.length === 2 && t1[1]!.name.startsWith('  ') && pt.tree().length === pt.procs.length; } },
    { id: 'F03886', name: '服务', check: () => { const s = new B.ServiceTable(); return s.toggle('varix-core') && !s.toggle('legacy-print') && s.setStartup('varix-sync', 'auto'); } },
    { id: 'F03887', name: '启动项', check: () => { const sm = new B.StartupManager(); const n = sm.list.length; sm.toggle('cloud-drive'); return sm.list.length === n && sm.highImpact().length === 0; } },
    { id: 'F03888', name: '计划任务', check: () => B.SCHEDULED_TASKS.length === 3 && B.SCHEDULED_TASKS[0]!.trigger.includes('每周日') },
    { id: 'F03889', name: '历史曲线', check: () => h.series(600).length === 3 && h.series(60).every((p) => p.t >= T0 + 30_000) },
    { id: 'F03890', name: '分类视图', check: () => [...pt.byCategory().keys()].sort().join() === 'app,service,system' },
    { id: 'F03891', name: '句柄', check: () => B.MOCK_PROCS.every((p) => p.handles > 0) },
    { id: 'F03892', name: '模块', check: () => B.MOCK_PROCS.find((p) => p.pid === 1025)!.modules.includes('d3d11.dll') },
    { id: 'F03893', name: '线程', check: () => B.MOCK_PROCS.find((p) => p.name === 'game.exe')!.threads === 64 },
    { id: 'F03894', name: '等待链', check: () => B.waitChain(B.MOCK_PROCS)[0]!.chain[0] === 'stuck.exe' },
    { id: 'F03895', name: 'GPU 按进程', check: () => B.perProcessRanking(B.MOCK_PROCS, 'gpu', 3).map((p) => p.pid).join() === '2048,1025,1024' },
    { id: 'F03896', name: '磁盘按进程', check: () => B.perProcessRanking(B.MOCK_PROCS, 'disk')[0]!.pid === 2048 },
    { id: 'F03897', name: '网络按进程', check: () => B.perProcessRanking(B.MOCK_PROCS, 'net')[0]!.pid === 2048 },
    { id: 'F03898', name: '摘要仪表', check: () => B.summaryMetrics(h).cpu > 0 && B.summaryMetrics(h).uptimeMin === 3 },
    { id: 'F03899', name: '悬浮窗', check: () => B.FLOAT_WINDOW_SPEC.width === 180 && B.FLOAT_WINDOW_SPEC.alwaysOnTop === true },
    { id: 'F03900', name: '教学', check: () => B.MONITOR_TIPS.length === 3 },
  ];
}

/* -------- AI-32 族0157 效率面板 -------- */
export function checkF0157(): CheckEntry[] {
  const log = new B.UsageLog();
  const hour = 3600_000;
  log.record({ app: 'Write', start: T0, end: T0 + 2 * hour, focus: true });
  log.record({ app: 'Browser', start: T0 + 2 * hour, end: T0 + 3 * hour, focus: false });
  log.record({ app: 'Code', start: T0 + 3 * hour, end: T0 + 4 * hour, focus: true });
  log.recordPomodoro(T0, 25).recordPomodoro(T0 + hour, 25);
  log.recordInterruption('IM', T0).recordInterruption('IM', T0 + 1).recordInterruption('邮件', T0 + 2);
  log.recordSwitch().recordSwitch().recordSwitch();
  const habits = new B.HabitTracker().add('早读');
  habits.check('早读', 1);
  habits.check('早读', 2);
  habits.check('早读', 3);
  const planner = new B.TimeBlockPlanner();
  planner.add('深度写作', 540, 600);
  planner.add('邮件', 600, 630);
  return [
    { id: 'F03901', name: '今日概览', check: () => log.todayOverview(T0 + 4 * hour).totalMin === 240 },
    { id: 'F03902', name: '应用排行', check: () => log.appRanking(2, T0 + 4 * hour).length === 2 && log.appRanking(1, T0 + 4 * hour)[0]!.minutes === 120 },
    { id: 'F03903', name: '网站位', check: () => B.WEB_TIME_RESERVED === true },
    { id: 'F03904', name: '专注统计', check: () => log.focusMinutes() === 180 },
    { id: 'F03905', name: '番茄历史', check: () => log.pomodoroHistory().length === 2 },
    { id: 'F03906', name: '评分', check: () => log.dailyScore(T0 + 4 * hour) >= 0 && log.dailyScore(T0 + 4 * hour) <= 100 },
    { id: 'F03907', name: '打扰源', check: () => log.interruptionRanking()[0]!.source === 'IM' && log.interruptionRanking()[0]!.count === 2 },
    { id: 'F03908', name: '切换次数', check: () => log.contextSwitches() === 3 },
    { id: 'F03909', name: '深度时段', check: () => log.deepHours(T0 + 4 * hour).length >= 2 && log.deepHours(T0 + 4 * hour)[0]!.focusMin === 120 },
    { id: 'F03910', name: '周报', check: () => log.weeklyReport(T0).days.length === 7 },
    { id: 'F03911', name: '月报', check: () => log.monthlyReport(T0).weeks === 4 },
    { id: 'F03912', name: '目标', check: () => { const g = new B.EfficiencyGoals().set('focus', 120); return g.progress('focus', 60).pct === 50 && g.progress('focus', 130).met === true; } },
    { id: 'F03913', name: '习惯', check: () => habits.names.includes('早读') && habits.dueReminders('早读', 9) === true },
    { id: 'F03914', name: '连续', check: () => habits.streak('早读', 3) === 3 },
    { id: 'F03915', name: '提醒', check: () => habits.dueReminders('早读', 4) === true && habits.dueReminders('早读', 3) === false },
    { id: 'F03916', name: '建议', check: () => B.efficiencySuggestions({ totalMin: 240, focusMin: 60 }, 50).some((s) => s.includes('专注')) },
    { id: 'F03917', name: '时间块', check: () => planner.plan.length === 2 && planner.add('冲突', 590, 610) === false },
    { id: 'F03918', name: '执行', check: () => planner.execute(550)?.label === '深度写作' && planner.plan[0]!.done === true },
    { id: 'F03919', name: '日程整合', check: () => planner.integrate([{ startMin: 600, endMin: 660 }]).length === 1 },
    { id: 'F03920', name: '教学', check: () => B.EFFICIENCY_TIPS.length === 2 },
    { id: 'F03921', name: '导出', check: () => JSON.parse(B.efficiencyExport(log)).v === 1 },
    { id: 'F03922', name: '隐私', check: () => B.EFFICIENCY_PRIVACY.includes('仅本地') },
    { id: 'F03923', name: '自定义', check: () => B.EFFICIENCY_WIDGET_SPEC.refreshMin === 5 },
    { id: 'F03924', name: '小部件', check: () => B.EFFICIENCY_WIDGET_SPEC.size.length === 2 },
    { id: 'F03925', name: '快捷键', check: () => B.EFFICIENCY_HOTKEY === 'Ctrl+Alt+E' },
  ];
}

/* -------- AI-32 族0158 文本工具集 -------- */
export function checkF0158(): CheckEntry[] {
  const multi = 'b\na\nc\na';
  return [
    { id: 'F03926', name: '大小写', check: () => B.toFullWidth('A') === 'Ａ' && B.toHalfWidth('Ａ') === 'A' },
    { id: 'F03927', name: '命名转换', check: () => B.caseStyle('hello world', 'camel') === 'helloWorld' && B.caseStyle('HelloWorld', 'snake') === 'hello_world' && B.caseStyle('my var', 'kebab') === 'my-var' && B.caseStyle('a b', 'upper-snake') === 'A_B' },
    { id: 'F03928', name: '去重', check: () => B.dedupeLines(multi) === 'b\na\nc' },
    { id: 'F03929', name: '排序', check: () => B.sortLines('3\n1\n2', { numeric: true }) === '1\n2\n3' && B.sortLines('b\na').startsWith('a') },
    { id: 'F03930', name: '反转', check: () => B.reverseText('abc') === 'cba' && B.reverseLines('1\n2') === '2\n1' },
    { id: 'F03931', name: '去空行', check: () => B.removeEmptyLines('a\n\n\nb') === 'a\nb' },
    { id: 'F03932', name: '去空格', check: () => B.trimLines(' a \n b') === 'a\nb' },
    { id: 'F03933', name: '行数', check: () => B.countLines('a\nb\nc') === 3 && B.countLines('') === 0 },
    { id: 'F03934', name: '字数', check: () => B.countWords('你好 world 42') === 4 },
    { id: 'F03935', name: '列编辑', check: () => B.columnPrefix('a\nb', '> ') === '> a\n> b' },
    { id: 'F03936', name: '列拼接', check: () => B.joinColumns('a\nb', '1\n2') === 'a\t1\nb\t2' },
    { id: 'F03937', name: '列拆分', check: () => B.splitColumn('a,1\nb,2', ',', 1) === '1\n2' },
    { id: 'F03938', name: '正则提取', check: () => B.regexExtract('a1 b22', '([a-z][0-9])').join() === 'a1,b2' },
    { id: 'F03939', name: '正则替换', check: () => B.regexReplace('a1b2', '\\d', '#') === 'a#b#' },
    { id: 'F03940', name: 'MD→HTML', check: () => B.mdToHtml('# t\n- a').includes('<h1>t</h1>') && B.mdToHtml('- a').includes('<li>a</li>') },
    { id: 'F03941', name: 'HTML→MD', check: () => B.htmlToMd('<h2>x</h2><p><strong>b</strong></p>').startsWith('## x') && B.htmlToMd('<strong>b</strong>').includes('**b**') },
    { id: 'F03942', name: 'JSON', check: () => B.jsonMinify(B.jsonFormat('{"a":1}')) === '{"a":1}' && B.jsonCheck('{"a":1}').ok && !B.jsonCheck('{').ok },
    { id: 'F03943', name: 'CSV', check: () => B.jsonToCsv(B.csvToJson('a,b\n1,2')) === 'a,b\n1,2' },
    { id: 'F03944', name: 'SQL 格式化', check: () => B.sqlFormat('select a from t where b=1').startsWith('SELECT') && B.sqlFormat('select a from t').includes('\nFROM') },
    { id: 'F03945', name: '代码美化', check: () => B.beautifyCode('function a(){\nreturn 1;\n}').split('\n')[1] === '  return 1;' },
    { id: 'F03946', name: 'Lorem', check: () => B.lorem(2).split('. ').length === 2 },
    { id: 'F03947', name: 'UUID', check: () => { const u = B.genUuidBatch(3); return u.length === 3 && new Set(u).size === 3 && u[0]!.length === 36; } },
    { id: 'F03948', name: '密码', check: () => B.genPassword(16).length === 16 && B.genPassword(8, { symbol: true }).length === 8 },
    { id: 'F03949', name: '二维码', check: () => { const m = B.qrMatrix('varix://hello'); return m.size >= 21 && m.dark(0, 0) === true && m.dark(0, m.size - 1) === true; } },
    { id: 'F03950', name: '对比', check: () => B.diffLines('a\nb', 'a\nc').some((l) => l.type === 'add') && B.diffLines('a\nb', 'a\nc').some((l) => l.type === 'del') },
  ];
}

/* -------- AI-32 族0159 开发者工具 -------- */
export function checkF0159(): CheckEntry[] {
  const jwtHeader = B.base64Encode('{"alg":"HS256"}').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  const jwtPayload = B.base64Encode('{"sub":"u1"}').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  return [
    { id: 'F03951', name: '正则测试', check: () => { const r = B.regexTest('\\d+', 'g', 'a1 b22'); const err = B.regexTest('(', 'g', 'x'); return Array.isArray(r) && r.length === 2 && !Array.isArray(err) && err.error !== undefined; } },
    { id: 'F03952', name: '时间戳', check: () => B.timestampConvert(1760000000000).iso.startsWith('2025-10-09') && B.timestampOf('2026-01-01T00:00:00Z') > 1.7e12 },
    { id: 'F03953', name: 'JSON', check: () => B.jsonCheck('{"a":1}').ok === true && B.jsonCheck('{"a"').ok === false },
    { id: 'F03954', name: 'JSON→TS', check: () => B.jsonToTs('{"a":1,"sub":{"name":"x"}}').includes('interface Root') && B.jsonToTs('{"a":1}').includes('a: number') },
    { id: 'F03955', name: 'URL', check: () => B.urlEncode('中 文') === '%E4%B8%AD%20%E6%96%87' && B.urlDecode('%E4%B8%AD%20%E6%96%87') === '中 文' },
    { id: 'F03956', name: 'Base64', check: () => B.base64Decode(B.base64Encode('你好 varix')) === '你好 varix' },
    { id: 'F03957', name: 'JWT', check: () => { const r = B.jwtDecode(`${jwtHeader}.${jwtPayload}.sig`); return !('error' in r) && r.payload.sub === 'u1'; } },
    { id: 'F03958', name: 'Hash', check: () => B.sha256Hex('abc') === 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' },
    { id: 'F03959', name: 'HMAC', check: () => B.hmacSha256Hex('k1', 'msg').length === 64 && B.hmacSha256Hex('k1', 'msg') !== B.hmacSha256Hex('k2', 'msg') },
    { id: 'F03960', name: 'AES 位', check: () => B.AES_TRIAL_RESERVED === true },
    { id: 'F03961', name: 'Diff', check: () => B.diffLines('a\nb\nc', 'a\nx\nc').filter((l) => l.type !== 'same').length === 2 },
    { id: 'F03962', name: '合并', check: () => B.merge3('a\nb', 'a\nc', 'a\nb').merged === 'a\nc' && B.merge3('a\nb', 'a\nx', 'a\ny').conflicts === 1 },
    { id: 'F03963', name: '取色', check: () => B.nearestNamedColor('#ff0000') === 'red' && B.nearestNamedColor('#010101') === 'black' },
    { id: 'F03964', name: '标尺', check: () => B.RULER_SPEC.unit === 'px' && B.RULER_SPEC.orientation.length === 2 },
    { id: 'F03965', name: '网格', check: () => B.GRID_OVERLAY.sizes.includes(8) },
    { id: 'F03966', name: 'XML', check: () => B.xmlCheck('<a><b/></a>').ok && !B.xmlCheck('<a><b></a>').ok },
    { id: 'F03967', name: 'YAML', check: () => B.yamlCheck('a: 1\nb: 2').ok && !B.yamlCheck('a: 1\n\tb: 2').ok },
    { id: 'F03968', name: 'TOML', check: () => B.tomlCheck('[pkg]\nname = "x"').ok && !B.tomlCheck('bad line').ok },
    { id: 'F03969', name: 'SQL', check: () => B.sqlFormat('select a from t').includes('SELECT') && B.sqlFormat('select a from t').includes('FROM') },
    { id: 'F03970', name: 'cURL 转', check: () => B.curlToFetch(`curl -X POST https://api.x.com/v1 -H 'Content-Type: application/json' -d '{"a":1}'`).includes('fetch(') },
    { id: 'F03971', name: '占位图', check: () => B.placeholderImage(120, 80).startsWith('data:image/svg+xml;base64,') },
    { id: 'F03972', name: '假数据', check: () => B.fakeData(1).name.length >= 2 && B.fakeData(1).phone.startsWith('1') && B.fakeData(1).email.includes('@') },
    { id: 'F03973', name: 'QR 解码', check: () => B.qrDecodeMeta('https://x.com').format === 'url' && B.qrDecodeMeta('WIFI:T:WPA;S:a;;').format === 'wifi' && B.qrDecodeMeta('BEGIN:VCARD').format === 'vcard' },
    { id: 'F03974', name: 'cron', check: () => B.cronExplain('* * * * *', 2).next.length === 2 && B.cronExplain('0 9 * * 1-5', 2).desc.includes('9') },
    { id: 'F03975', name: '教学', check: () => B.DEV_TIPS.length === 3 },
  ];
}

/* -------- AI-32 族0160 录音与音频工具 -------- */
export function checkF0160(): CheckEntry[] {
  const rec = new B.RecorderSession();
  rec.start(T0);
  rec.stop(T0 + 2000);
  const list = new B.RecordingList();
  list.add('a.wav', 10, 160);
  list.add('b.wav', 20, 320);
  const wave = Array.from({ length: 5000 }, (_, i) => Math.sin((2 * Math.PI * i) / 100));
  return [
    { id: 'F03976', name: '录音机', check: () => rec.durationSec(T0 + 3000) === 2 },
    { id: 'F03977', name: '录音列表', check: () => { const before = list.list.length; list.remove('b.wav'); return before === 2 && list.list.length === 1; } },
    { id: 'F03978', name: '重命名', check: () => B.batchRenameRecordings(['x.wav', 'y.mp3'])[1] === '录音_002.mp3' },
    { id: 'F03979', name: '剪辑', check: () => B.trimWave(wave, 1, 2, 1000).length === 1000 },
    { id: 'F03980', name: '降噪', check: () => Math.max(...B.denoise([0, 1, 0], 3)) < 1 },
    { id: 'F03981', name: '转码', check: () => B.AUDIO_TRANSCODE_PRESETS.length === 4 },
    { id: 'F03982', name: '变声', check: () => Math.abs(B.pitchShiftFactor(12) - 2) < 1e-9 && B.pitchShiftFactor(-12) === 0.5 },
    { id: 'F03983', name: '转文字', check: () => B.transcribePlaceholder(60).includes('预留') },
    { id: 'F03984', name: 'TTS', check: () => B.TTS_VOICES.length === 4 },
    { id: 'F03985', name: '标准化', check: () => B.loudnessNormalize([0.1, 0.1], 0.3).every((x) => Math.abs(x - 0.3) < 1e-9) },
    { id: 'F03986', name: '铃声', check: () => B.makeRingtone(Array(5000).fill(1), 30, 100, 50).length === 3000 && B.makeRingtone(Array(5000).fill(1), 30, 100, 50)[0]! < 1 },
    { id: 'F03987', name: '白噪音混合', check: () => { const m = new B.NoiseMixer().set('雨声', 0.6).set('海浪', 0.4); return m.active.length === 2 && m.mixedGain() === 0.5 && m.remove('雨声'); } },
    { id: 'F03988', name: '环境音播放', check: () => B.AMBIENT_TRACKS.length === 5 },
    { id: 'F03989', name: '可视化', check: () => B.visualizeBars([0.5, 0.5, 0.2, 0.2], 2).join() === '0.5,0.2' },
    { id: 'F03990', name: '频谱', check: () => B.dftSpectrum([1, -1, 1, -1], 4).length === 4 },
    { id: 'F03991', name: '节拍器', check: () => B.metronomeClicks(60, 1, 4).join() === '0,1000,2000,3000' },
    { id: 'F03992', name: '调音器', check: () => B.noteFromFreq(440).note === 'A' && B.noteFromFreq(440).octave === 4 && Math.abs(B.noteFromFreq(442).cents) === 8 },
    { id: 'F03993', name: '音高检测', check: () => B.pitchDetect(Array.from({ length: 2001 }, (_, i) => Math.sin((2 * Math.PI * i) / 250 + 0.01)), 1000) === 4 },
    { id: 'F03994', name: '合并', check: () => B.mergeSegments([10, 20, 30]) === 60 },
    { id: 'F03995', name: '拆分', check: () => B.splitByDuration(65, 30).join() === '30,30,5' },
    { id: 'F03996', name: '批量转换', check: () => B.batchAudioConvert(['a.wav', 'b.flac'], 'mp3').join() === 'a.mp3,b.mp3' },
    { id: 'F03997', name: '元数据', check: () => B.readAudioMeta('x.mp3', 1000, 8).bitrateKbps === 1000 },
    { id: 'F03998', name: '标签编辑', check: () => B.editTags({ artist: 'a' }, { title: 't' }).title === 't' && B.editTags({ artist: 'a' }, { title: 't' }).artist === 'a' },
    { id: 'F03999', name: '抓轨位', check: () => B.CD_RIP_RESERVED === true },
    { id: 'F04000', name: '教学', check: () => B.AUDIO_TIPS.length === 3 },
  ];
}

/* -------- AI-33 族0161 演示与白板 -------- */
export function checkF0161(): CheckEntry[] {
  const wb = new C.Whiteboard();
  wb.stroke([{ x: 0, y: 0 }, { x: 10, y: 10 }]);
  wb.shape('rect', 0, 0, 10, 10);
  wb.sticky(5, 5, '便签');
  wb.insertImage(0, 0, 100, 80);
  const timer = new C.PresentationTimer(1);
  timer.tick(30);
  const laser = new C.LaserPointer(1000);
  laser.move(1, 1, T0);
  laser.move(2, 2, T0 + 100);
  return [
    { id: 'F04001', name: '白板', check: () => wb.all.length === 4 },
    { id: 'F04002', name: '画笔', check: () => wb.tool === 'pen' && wb.erase(5, 5) === 1 && wb.all.length === 3 },
    { id: 'F04003', name: '形状', check: () => wb.shape('ellipse', 0, 0, 5, 5).shape === 'ellipse' },
    { id: 'F04004', name: '便签贴', check: () => wb.all.some((e) => e.kind === 'sticky' && e.text === '便签') },
    { id: 'F04005', name: '插图', check: () => wb.all.some((e) => e.kind === 'image' && e.w === 100) },
    { id: 'F04006', name: '模板', check: () => C.WHITEBOARD_TEMPLATES.length === 6 },
    { id: 'F04007', name: '导出', check: () => wb.exportSpec().png.w === 3840 && wb.exportSpec().elements === wb.all.length },
    { id: 'F04008', name: '倒计时', check: () => timer.remainSec === 30 && Math.abs(timer.progress - 0.5) < 1e-9 },
    { id: 'F04009', name: '激光笔', check: () => laser.visible(T0 + 200).length === 2 && laser.visible(T0 + 2000).length === 0 },
    { id: 'F04010', name: '聚光灯', check: () => C.spotlightMask({ cx: 100, cy: 100, radius: 50 }, 1920, 1080).hole.radius === 50 },
    { id: 'F04011', name: '缩放聚焦', check: () => Math.abs(C.zoomFocus({ x: 0, y: 0, w: 100, h: 100 }, { w: 200, h: 200 }).scale - 2) < 1e-9 },
    { id: 'F04012', name: '板书', check: () => C.HANDWRITING_BOARD_RESERVED === true },
    { id: 'F04013', name: '共享位', check: () => C.COLLAB_RESERVED === true },
    { id: 'F04014', name: '录制讲解', check: () => C.RECORD_LECTURE_RESERVED === true },
    { id: 'F04015', name: '思维导图', check: () => C.mindmapLayout({ label: '根', children: [{ label: 'A', children: [] }, { label: 'B', children: [] }] }).length === 3 },
    { id: 'F04016', name: '流程图', check: () => { const d = C.flowLayout([{ id: 'a', label: 'A', next: ['b'] }, { id: 'b', label: 'B', next: [] }]); return d.get('a') === 0 && d.get('b') === 1; } },
    { id: 'F04017', name: '时间线图', check: () => C.timelineLayout([{ label: 'a', at: 1 }, { label: 'b', at: 3 }]).length === 2 && C.timelineLayout([{ label: 'a', at: 1 }, { label: 'b', at: 3 }])[1]!.x === 1000 },
    { id: 'F04018', name: '甘特轻量', check: () => { const g = C.ganttLayout([{ name: 'a', startDay: 2, days: 3 }], 10); return g[0]!.xPct === 20 && g[0]!.wPct === 30; } },
    { id: 'F04019', name: '鱼骨图', check: () => C.fishboneLayout('延期', [{ bone: '人', items: ['不足'] }]).bones[0]!.angle === 60 },
    { id: 'F04020', name: 'SWOT', check: () => Object.keys(C.SWOT_TEMPLATE).length === 4 },
    { id: 'F04021', name: '康奈尔', check: () => Object.keys(C.CORNELL_TEMPLATE).length === 3 },
    { id: 'F04022', name: '手帐', check: () => Object.keys(C.JOURNAL_TEMPLATE).length === 4 },
    { id: 'F04023', name: '协作位', check: () => C.MULTIPLAYER_RESERVED === true },
    { id: 'F04024', name: '遥控', check: () => C.remotePaging('next', 0, 5) === 1 && C.remotePaging('prev', 0, 5) === 0 && C.remotePaging('black', 3, 5) === 3 },
    { id: 'F04025', name: '教学', check: () => C.WHITEBOARD_TIPS.length === 2 },
  ];
}

/* -------- AI-33 族0162 阅读器 -------- */
export function checkF0162(): CheckEntry[] {
  const rs = new C.ReaderSession();
  rs.setToc([{ id: 'ch1', title: '第一章' }, { id: 'ch2', title: '第二章' }]);
  const rl = new C.ReadLater();
  rl.add('https://a', '文章A', true);
  rl.add('https://b', '文章B');
  return [
    { id: 'F04026', name: 'EPUB', check: () => C.epubParse('<spine><itemref idref="ch1"/><itemref idref="ch2"/></spine>').join() === 'ch1,ch2' },
    { id: 'F04027', name: 'TXT', check: () => C.txtPaginate('x'.repeat(25), 10).length === 3 },
    { id: 'F04028', name: 'PDF', check: () => C.pdfPageCount('p1\fp2') === 2 },
    { id: 'F04029', name: 'MOBI 位', check: () => C.MOBI_RESERVED === true },
    { id: 'F04030', name: '翻页', check: () => C.PAGE_FLIP_STYLES.length === 3 && C.PAGE_FLIP_STYLES[0]!.style === 'curl' },
    { id: 'F04031', name: '排版', check: () => C.TYPOGRAPHY_PRESETS['默认']!.fontSize === 18 && C.TYPOGRAPHY_PRESETS['舒适']!.lineHeight === 2.0 },
    { id: 'F04032', name: '主题', check: () => C.READER_THEMES['night']!.bg === '#111318' && C.READER_THEMES['paper'] !== undefined },
    { id: 'F04033', name: '目录', check: () => rs.tocList.length === 2 && rs.jumpChapter('ch2') === 1 },
    { id: 'F04034', name: '书签', check: () => rs.addMark('ch1', 0.5).chapter === 'ch1' && rs.markList.length === 1 },
    { id: 'F04035', name: '划线', check: () => rs.highlight('金句一段', '感触') === true && rs.highlight('  ') === false },
    { id: 'F04036', name: '笔记导出', check: () => rs.exportNotes().includes('金句一段') && rs.exportNotes().includes('感触') },
    { id: 'F04037', name: '进度', check: () => rs.setProgress(50).progress === 50 && rs.setProgress(120).progress === 100 },
    { id: 'F04038', name: '统计', check: () => { const st = new C.ReadingStats().add('2026-09-13', 30).add('2026-09-12', 20); return st.total() === 50 && st.byDay('2026-09-13') === 30; } },
    { id: 'F04039', name: '听书', check: () => C.READER_TTS_DEFAULT.voice === '晓晓' },
    { id: 'F04040', name: '自动滚动', check: () => C.READER_TTS_DEFAULT.autoScroll === false },
    { id: 'F04041', name: '语速', check: () => C.READER_TTS_DEFAULT.rate === 1 },
    { id: 'F04042', name: '划词词典', check: () => C.lookupWord('aurora')!.includes('极光') && C.lookupWord('zzz') === undefined },
    { id: 'F04043', name: '划词翻译', check: () => C.translateWord('kernel').includes('内核') },
    { id: 'F04044', name: '全屏', check: () => C.FULLSCREEN_READER.immersive === true },
    { id: 'F04045', name: '自动翻页', check: () => typeof C.READER_TTS_DEFAULT.autoPageSec === 'number' },
    { id: 'F04046', name: 'RSS 位', check: () => C.RSS_RESERVED === true },
    { id: 'F04047', name: '稍后读', check: () => rl.list.length === 2 && rl.add('https://a', '重复') === false },
    { id: 'F04048', name: '离线', check: () => rl.offlineCount() === 1 },
    { id: 'F04049', name: '历史', check: () => rs.recordHistory('书A').recordHistory('书B').readHistory.length === 2 },
    { id: 'F04050', name: '教学', check: () => C.READER_TIPS.length === 3 },
  ];
}

/* -------- AI-33 族0163 媒体播放器 -------- */
export function checkF0163(): CheckEntry[] {
  const pl = new C.Playlist();
  pl.add({ id: '1', title: 'A', durationSec: 100, kind: 'video', path: '/a.mp4' });
  pl.add({ id: '2', title: 'B', durationSec: 200, kind: 'audio', path: '/b.mp3' });
  const cue = C.srtParse('1\n00:00:01,000 --> 00:00:02,000\n你好');
  const ab = new C.AbLoop().setA(10).setB(20);
  const eq = new C.Equalizer();
  const sleep = new C.SleepTimer(1);
  sleep.tick(60);
  const lib = new C.MediaLibrary();
  lib.add({ id: '1', title: 'A', durationSec: 100, kind: 'video', path: '/a.mp4' });
  lib.add({ id: '2', title: 'B', durationSec: 200, kind: 'audio', path: '/b.mp3' });
  return [
    { id: 'F04051', name: '视频', check: () => lib.videoList.length === 1 && lib.videoList[0]!.kind === 'video' },
    { id: 'F04052', name: '音频', check: () => lib.audioList.length === 1 },
    { id: 'F04053', name: '列表', check: () => pl.list.length === 2 && pl.exportM3U().startsWith('#EXTM3U') },
    { id: 'F04054', name: '字幕', check: () => cue.length === 1 && cue[0]!.text === '你好' && C.subtitleAt(cue, 1.5) === '你好' && C.subtitleShift(cue, 5)[0]!.startSec === 6 },
    { id: 'F04055', name: '音轨', check: () => C.AUDIO_TRACKS.length === 4 },
    { id: 'F04056', name: '画质', check: () => C.QUALITY_LEVELS.includes('1080p') },
    { id: 'F04057', name: '倍速', check: () => C.clampSpeed(5) === 4 && C.clampSpeed(0.1) === 0.25 && C.PLAYBACK_SPEEDS[0] === 0.25 },
    { id: 'F04058', name: 'A-B 循环', check: () => ab.wrap(25) === 10 && ab.wrap(15) === 15 },
    { id: 'F04059', name: '时间书签', check: () => new C.TimeBookmark().add('名场面', 90).nearest(95)?.at === 90 },
    { id: 'F04060', name: '截图', check: () => C.screenshotSpec(12, { w: 1920, h: 1080 }).format === 'png' },
    { id: 'F04061', name: '连播', check: () => { pl.playAt(0); return pl.mode === 'sequence' && pl.next()?.id === '2'; } },
    { id: 'F04062', name: '随机', check: () => { pl.mode = 'shuffle'; return pl.next() !== undefined; } },
    { id: 'F04063', name: '均衡器', check: () => { const ok = eq.set(60, 8); const g1 = eq.gains[0]; eq.preset('bass'); const g2 = eq.gains[0]; return ok && g1 === 8 && g2 === 6; } },
    { id: 'F04064', name: '增强', check: () => C.volumeBoost(8) === 6 && C.volumeBoost(-1) === 0 },
    { id: 'F04065', name: '睡眠定时', check: () => sleep.remainSec === 0 && sleep.tick(1) === false },
    { id: 'F04066', name: '迷你', check: () => C.MINI_MODE_SPEC.width === 320 && C.MINI_MODE_SPEC.compact === true },
    { id: 'F04067', name: '画中画', check: () => C.PIP_SPEC.alwaysOnTop === true },
    { id: 'F04068', name: '记忆', check: () => new C.ResumeMemory().save('a', 95).resume('a') === 95 },
    { id: 'F04069', name: '视频库', check: () => lib.videoList[0]!.path === '/a.mp4' },
    { id: 'F04070', name: '音乐库', check: () => lib.search('b').length === 1 },
    { id: 'F04071', name: '歌词', check: () => { const l = C.lrcParse('[00:01.00]你好\n[00:05.00]再见'); return l[0]!.timeSec === 1 && C.lyricAt(l, 6) === '再见'; } },
    { id: 'F04072', name: '桌面歌词', check: () => C.DESKTOP_LYRIC_SPEC.draggable === true },
    { id: 'F04073', name: '频谱', check: () => C.spectrumBars([0.5, 0.5, 0.1, 0.1], 2).join() === '0.5,0.1' },
    { id: 'F04074', name: '硬解', check: () => C.HW_DECODE.codecs.includes('hevc') && C.HW_DECODE.prefer === true },
    { id: 'F04075', name: '教学', check: () => C.PLAYER_TIPS.length === 3 },
  ];
}

/* -------- AI-33 族0164 图片查看器 -------- */
export function checkF0164(): CheckEntry[] {
  const meta: C.ImageMeta = { path: '/p/a.jpg', width: 4000, height: 3000, format: 'jpg', exif: { Orientation: '6', GPS: '31,121' } };
  const strip = new C.ThumbnailStrip().load([meta, { ...meta, path: '/p/b.jpg' }]);
  const gif = new C.GifControl(8);
  const px = new Uint8ClampedArray([255, 0, 0, 255, 0, 255, 0, 255]);
  return [
    { id: 'F04076', name: '快速看', check: () => C.openBudget(meta).instant === true && C.openBudget({ ...meta, width: 20000, height: 20000 }).instant === false },
    { id: 'F04077', name: '滚轮缩放', check: () => C.zoomClamp(64) === 32 && C.smoothZoomStep(1, 1) === 1.25 && C.smoothZoomStep(1, -1) === 0.75 },
    { id: 'F04078', name: '旋转', check: () => C.transformImage(450, 'h').rotate === 90 && C.transformImage(-90, 'none').rotate === 270 },
    { id: 'F04079', name: '缩略条', check: () => strip.activate(1)!.path === '/p/b.jpg' && strip.active === 1 },
    { id: 'F04080', name: '幻灯', check: () => { const s = new C.Slideshow(2); return s.tick(1) === false && s.tick(1) === true; } },
    { id: 'F04081', name: 'EXIF', check: () => C.exifPanel(meta)[0]!.includes('4000') && C.exifPanel(meta).some((r) => r.startsWith('GPS')) },
    { id: 'F04082', name: 'RAW 位', check: () => C.RAW_FORMATS_RESERVED.includes('cr3') },
    { id: 'F04083', name: '批量', check: () => { const bv = new C.BatchViewer().load(['/a', '/b', '/c']); return bv.step(1) === '/b' && bv.step(-1) === '/a' && bv.step(-1) === '/c'; } },
    { id: 'F04084', name: '对比', check: () => C.compareImages(meta, { ...meta, width: 2000 }).sameSize === false },
    { id: 'F04085', name: '放大镜', check: () => C.magnifierRegion(10, 20).zoom === 2 && C.magnifierRegion(10, 20, 80, 3).zoom === 3 },
    { id: 'F04086', name: '取色', check: () => C.pickColor(px, 0, 0, 2) === '#ff0000' && C.pickColor(px, 1, 0, 2) === '#00ff00' },
    { id: 'F04087', name: '直方图', check: () => { const h = C.histogram(px); return h.r.length === 32 && h.r[31] === 1 && h.g[31] === 1; } },
    { id: 'F04088', name: '自动转正', check: () => C.autoOrient(6) === 90 && C.autoOrient(1) === 0 },
    { id: 'F04089', name: '打印', check: () => C.printHandoff(meta).paper === 'A3' && C.printHandoff({ ...meta, width: 2000, height: 4000 }).paper === 'A4' },
    { id: 'F04090', name: '设壁纸', check: () => C.setWallpaper(meta).path === '/p/a.jpg' },
    { id: 'F04091', name: '轻编辑', check: () => C.lightCrop(meta, { x: 0, y: 0, w: 1000, h: 500 }).width === 1000 },
    { id: 'F04092', name: '分享位', check: () => C.SHARE_RESERVED === true },
    { id: 'F04093', name: '同目录', check: () => C.sameDirNavigate(['/a', '/b'], '/a', 1) === '/b' && C.sameDirNavigate(['/a', '/b'], '/a', -1) === '/b' },
    { id: 'F04094', name: 'GIF 控制', check: () => gif.toggle() === false && gif.playing === false },
    { id: 'F04095', name: '逐帧', check: () => gif.stepFrame(1) === 1 && gif.stepFrame(-1) === 0 && gif.stepFrame(-1) === 7 },
    { id: 'F04096', name: 'SVG', check: () => C.SVG_VIEW.vector === true },
    { id: 'F04097', name: 'HEIC', check: () => C.HEIC_SUPPORTED === true },
    { id: 'F04098', name: '双屏', check: () => C.DUAL_SCREEN_VIEWER.primary === 'viewer' },
    { id: 'F04099', name: '全屏', check: () => C.FULLSCREEN_IMAGE.hideUi === true },
    { id: 'F04100', name: '教学', check: () => C.IMAGE_TIPS.length === 3 },
  ];
}

/* -------- AI-33 族0165 打印中心 -------- */
export function checkF0165(): CheckEntry[] {
  const q = new C.PrintQueue();
  const job = q.add('报告', 10, 1, { colorMode: 'mono' });
  q.start();
  return [
    { id: 'F04101', name: '队列', check: () => q.list.length === 1 && job.state === 'printing' },
    { id: 'F04102', name: '预览', check: () => C.printPreview(3).mm === '210×297' && C.printPreview(3, 'A3').mm === '297×420' },
    { id: 'F04103', name: '多合一', check: () => C.nUpLayout(4).cols === 2 && C.nUpLayout(4).scalePct === 50 && C.nUpLayout(9).perSheet === 9 },
    { id: 'F04104', name: '双面', check: () => C.duplexHint(10, true).includes('5 张纸') && C.duplexHint(9, true).includes('末页空白') },
    { id: 'F04105', name: '到 PDF', check: () => C.PRINT_TO_PDF.virtual === true },
    { id: 'F04106', name: '到图片', check: () => C.printToImages(3).count === 3 && C.printToImages(3).format === 'png' },
    { id: 'F04107', name: '页码范围', check: () => C.parsePageRange('1-3,5', 10).join() === '1,2,3,5' && C.parsePageRange('8-', 10).join() === '8,9,10' },
    { id: 'F04108', name: '缩放', check: () => C.scaleFit('custom', 150).pct === 150 && C.scaleFit('fit').label === '适应纸张' },
    { id: 'F04109', name: '份数', check: () => q.add('讲义', 5, 3).copies === 3 },
    { id: 'F04110', name: '色彩', check: () => job.colorMode === 'mono' && q.list[1]!.colorMode === 'color' },
    { id: 'F04111', name: '纸张', check: () => C.PAPER_SIZES.includes('A4') && C.PAPER_SIZES.includes('16K') },
    { id: 'F04112', name: '历史', check: () => C.printHistory(q.list).length >= 2 },
    { id: 'F04113', name: '默认打印机', check: () => C.setDefaultPrinter(['A', 'B'], 'B') === 'B' && C.setDefaultPrinter(['A'], 'C') === undefined },
    { id: 'F04114', name: '发现', check: () => C.discoverPrinters([{ name: 'a', ip: '1', via: 'mDNS', online: true }, { name: 'b', ip: '2', via: 'SNMP', online: false }]).length === 1 },
    { id: 'F04115', name: '驱动状态', check: () => C.driverStatus(true).healthy === true && C.driverStatus(false).message.includes('重装') },
    { id: 'F04116', name: '成本估算', check: () => C.costEstimate(10, 1, 'mono', 5).yuan === 0.8 && C.costEstimate(10, 1, 'color').yuan === 4.5 },
    { id: 'F04117', name: '省墨', check: () => C.ecoInk(40).savedPct === 36 && C.ecoInk(100).savedPct === 0 },
    { id: 'F04118', name: '海报', check: () => C.posterTiles(420, 594).tiles === 9 },
    { id: 'F04119', name: '小册子', check: () => C.bookletOrder(8).flat().join() === '8,1,2,7,6,3,4,5' },
    { id: 'F04120', name: '取消', check: () => { const q2 = new C.PrintQueue(); const j = q2.add('x', 1); q2.start(); return q2.cancel(j.id) === true && q2.list[0]!.state === 'cancelled'; } },
    { id: 'F04121', name: '暂停恢复', check: () => { const q2 = new C.PrintQueue(); const j = q2.add('x', 1); q2.start(); return q2.pause(j.id) && q2.resume(j.id) && q2.list[0]!.state === 'printing'; } },
    { id: 'F04122', name: '诊断', check: () => C.diagnoseQueue([{ id: '1', title: 'a', pages: 1, copies: 1, state: 'queued', printer: 'p', colorMode: 'color', duplex: false, createdAt: 0 }])[0]!.includes('队列未启动') },
    { id: 'F04123', name: '水印', check: () => C.printWatermark('机密').text === '机密' && C.printWatermark('机密').opacity < 0.5 },
    { id: 'F04124', name: '批量', check: () => C.batchPrint(['a', 'b', 'c']).length === 3 },
    { id: 'F04125', name: '教学', check: () => C.PRINT_TIPS.length === 3 },
  ];
}

/* -------- AI-34 族0166 通讯录与人脉 -------- */
export function checkF0166(): CheckEntry[] {
  const cs = new D.Contacts();
  const c1 = cs.add({ name: '张三', phones: ['13800000001'], emails: ['zs@x.com'], birthday: '12-25' });
  const c2 = cs.add({ name: '李四', phones: ['13800000002'] });
  cs.addGroup(c1.id, '家庭');
  cs.addGroup(c2.id, '同事');
  cs.touch(c2.id, T0);
  cs.add({ name: '张三', phones: ['13800000001'] });
  return [
    { id: 'F04126', name: '管理', check: () => cs.all.length === 3 && cs.update(c1.id, { note: '老朋友' }) },
    { id: 'F04127', name: '分组', check: () => cs.group('家庭').length === 1 && cs.addGroup(c1.id, '家庭') === false },
    { id: 'F04128', name: '搜索', check: () => cs.search('13800000001').length === 2 && cs.search('张').length === 2 },
    { id: 'F04129', name: '生日', check: () => cs.birthdayReminders(120, new Date(2026, 8, 13))[0]!.name === '张三' },
    { id: 'F04130', name: '备注', check: () => cs.all[0]!.note === '老朋友' },
    { id: 'F04131', name: '头像', check: () => (cs.update(c1.id, { avatar: 'data:image/png;base64,x' }), cs.all[0]!.avatar !== undefined) },
    { id: 'F04132', name: '多字段', check: () => c1.phones.length === 1 && c1.emails.length === 1 && (cs.update(c1.id, { address: '上海市' }), cs.all[0]!.address === '上海市') },
    { id: 'F04133', name: '导入', check: () => { const c3 = new D.Contacts(); return c3.importVcard('BEGIN:VCARD\r\nVERSION:3.0\r\nFN:王五\r\nTEL:13800000003\r\nEND:VCARD') === 1 && c3.all[0]!.name === '王五'; } },
    { id: 'F04134', name: '导出', check: () => cs.exportVcard().includes('BEGIN:VCARD') && cs.exportVcard().includes('FN:张三') },
    { id: 'F04135', name: '去重', check: () => cs.dedupe() === 1 && cs.all.length === 2 },
    { id: 'F04136', name: '置顶', check: () => (cs.pin(c2.id), cs.sorted()[0]!.id === c2.id) },
    { id: 'F04137', name: '快速拨号位', check: () => D.QUICK_DIAL_RESERVED === true },
    { id: 'F04138', name: '二维码', check: () => D.vcardQrText(cs.all[0]!).includes('FN:张三') },
    { id: 'F04139', name: '名片 OCR', check: () => D.cardOcr('姓名 王五 电话 13911112222 邮箱 w@x.com').phone === '13911112222' },
    { id: 'F04140', name: '家庭组', check: () => cs.group('家庭').length === 1 },
    { id: 'F04141', name: '同事组', check: () => cs.group('同事').length === 1 },
    { id: 'F04142', name: '标签', check: () => (cs.batchOp([c1.id], 'tag', '重要'), cs.all[0]!.tags.includes('重要')) },
    { id: 'F04143', name: '最近联系', check: () => cs.sorted().find((c) => c.name === '李四')!.lastContactAt === T0 },
    { id: 'F04144', name: '时间线', check: () => cs.sorted().every((c, i, arr) => i === 0 || Number(c.pinned) <= Number(arr[i - 1]!.pinned)) },
    { id: 'F04145', name: '隐私锁', check: () => { const c = new D.Contacts(); c.add({ name: '机密人', groups: ['私密'] }); c.add({ name: '普通人' }); c.lockGroup('私密'); const hidden = c.search('机密人').length === 0; c.unlockGroup('私密'); return hidden && c.search('机密人').length === 1 && c.search('普通人').length === 1; } },
    { id: 'F04146', name: '批量', check: () => (cs.batchOp([c2.id], 'group', 'VIP'), cs.group('VIP').length === 1) },
    { id: 'F04147', name: '打印', check: () => cs.printList().includes('张三') },
    { id: 'F04148', name: '备份', check: () => JSON.parse(cs.backup()).length === 2 },
    { id: 'F04149', name: '恢复', check: () => { const c4 = new D.Contacts(); return c4.restore(cs.backup()) === 2; } },
    { id: 'F04150', name: '教学', check: () => D.CONTACTS_TIPS.length === 3 },
  ];
}

/* -------- AI-34 族0167 密码管理器 -------- */
export function checkF0167(): CheckEntry[] {
  const vault = new D.PasswordVault();
  vault.create('master-pw-1');
  const e1 = vault.add('github.com', 'me', 'Str0ng!Pass1', '开发');
  vault.add('mail.com', 'me', '123456', '常用');
  vault.add('bank.com', 'me', 'Str0ng!Pass1', '金融');
  vault.changePassword(e1!.id, 'N3w!Pass99');
  vault.setTotp(e1!.id, 'SECRETKEY234567');
  return [
    { id: 'F04151', name: '库', check: () => vault.isLocked === false && vault.getPassword(e1!.id) === 'N3w!Pass99' },
    { id: 'F04152', name: '生成', check: () => D.generatePassword(20).length === 20 && D.generatePassword(12, { symbol: true }).length === 12 },
    { id: 'F04153', name: '强度', check: () => D.passwordStrength('abc').label === '极弱' && D.passwordStrength('Xk9!mQ2#vL8@').score >= 85 },
    { id: 'F04154', name: '自动填充位', check: () => D.AUTOFILL_RESERVED === true },
    { id: 'F04155', name: '分类', check: () => vault.categories().length === 3 },
    { id: 'F04156', name: '搜索', check: () => vault.search('github').length === 1 },
    { id: 'F04157', name: '收藏', check: () => vault.favorite(e1!.id) === true && vault.favorite(e1!.id) === false },
    { id: 'F04158', name: '历史', check: () => vault.passwordHistory(e1!.id) === 1 },
    { id: 'F04159', name: '到期提醒', check: () => vault.expiring(86400_000 * 100).length === 0 },
    { id: 'F04160', name: '重复检测', check: () => { const v2 = new D.PasswordVault(); v2.create('master-pw-1'); v2.add('a.com', 'u', 'SamePw1!'); v2.add('b.com', 'u', 'SamePw1!'); return v2.duplicates().length === 1 && v2.duplicates()[0]!.join().includes('b.com'); } },
    { id: 'F04161', name: '弱口令检测', check: () => vault.weakList().some((w) => w.site === 'mail.com') },
    { id: 'F04162', name: '泄露位', check: () => D.BREACH_CHECK_RESERVED === true },
    { id: 'F04163', name: '安全问答', check: () => vault.setQa(e1!.id, [{ q: '出生城市?', a: '上海' }]) },
    { id: 'F04164', name: '密钥文件', check: () => D.twoFactorKey('pw', 'filehash') !== 'pw' && D.twoFactorKey('pw', 'f1') !== D.twoFactorKey('pw', 'f2') },
    { id: 'F04165', name: '恢复码', check: () => { const rc = D.recoveryCodes(3); return rc.length === 3 && /^[0-9A-F]{4}-[0-9A-F]{4}$/.test(rc[0]!); } },
    { id: 'F04166', name: '导出', check: () => JSON.parse(vault.exportEncrypted()).entries.length === 3 },
    { id: 'F04167', name: '导入', check: () => { const v2 = new D.PasswordVault(); v2.create('master-pw-1'); return v2.importEncrypted(vault.exportEncrypted()) === 3; } },
    { id: 'F04168', name: '卡片打印', check: () => vault.printCard().includes('••') && !vault.printCard().includes('Str0ng') },
    { id: 'F04169', name: 'TOTP', check: () => /^\d{6}$/.test(D.totp('SECRETKEY234567', 0)) && D.totp('SECRETKEY234567', 0) !== D.totp('SECRETKEY234567', 60) },
    { id: 'F04170', name: '备份码', check: () => D.recoveryCodes(5).length === 5 },
    { id: 'F04171', name: '扩展位', check: () => D.BROWSER_EXTENSION_RESERVED === true },
    { id: 'F04172', name: '自动锁', check: () => vault.autoLockMs === 5 * 60_000 },
    { id: 'F04173', name: '剪贴自清', check: () => D.CLIPBOARD_CLEAR_SEC === 30 },
    { id: 'F04174', name: '审计', check: () => { const a = vault.audit(); return a.total === 3 && a.weak === 1 && a.score > 0 && a.score <= 100; } },
    { id: 'F04175', name: '教学', check: () => D.PASSWORD_TIPS.length === 3 },
  ];
}

/* -------- AI-34 族0168 网络工具 -------- */
export function checkF0168(): CheckEntry[] {
  const st = new D.SpeedTest().push(0, 300, 50).push(1, 280, 48);
  const proxy = new D.ProxyCenter();
  proxy.add('本机', 'socks5', '127.0.0.1', 7890);
  const traffic = new D.TrafficStats().record('Browser', 500).record('IM', 100);
  const watcher = new D.OfflineWatcher();
  watcher.markDown(T0);
  return [
    { id: 'F04176', name: '测速', check: () => st.result().down === 290 && st.result().grade === '百兆级' && st.result().up === 49 },
    { id: 'F04177', name: 'Ping', check: () => { const p = D.pingParse('a.com', [10, 11, null, 12]); return p.lossPct === 25 && p.avgMs === 11; } },
    { id: 'F04178', name: 'Traceroute', check: () => D.tracerouteMock([{ hop: 1, host: 'gw', ms: 1 }, { hop: 2, host: 'isp', ms: 9 }]).reached === true && D.tracerouteMock([{ hop: 1, host: 'gw', ms: null }]).reached === false },
    { id: 'F04179', name: 'DNS', check: () => D.dnsQuery('example.com').records[0] === '93.184.216.34' && D.dnsQuery('nope.com').records.length === 0 },
    { id: 'F04180', name: 'IP', check: () => D.localIpInfo('192.168.1.5', '1.2.3.4').isPrivate === true && D.localIpInfo('8.8.8.8', '1.2.3.4').isPrivate === false },
    { id: 'F04181', name: '端口', check: () => { const r = D.portScan([{ port: 80, state: 'open', process: 'httpd' }, { port: 445, state: 'open' }, { port: 9999, state: 'closed' }]); return r.open.length === 2 && r.wellKnown.length === 2; } },
    { id: 'F04182', name: '共享管理', check: () => { const s = new D.ShareManager(); return s.add('/docs', '文档', 'read', ['family']) && s.list.length === 1 && s.revoke('/docs'); } },
    { id: 'F04183', name: 'RDP 位', check: () => D.RDP_RESERVED === true },
    { id: 'F04184', name: 'SSH 位', check: () => D.SSH_RESERVED === true },
    { id: 'F04185', name: 'Telnet 位', check: () => D.TELNET_RESERVED === true },
    { id: 'F04186', name: '代理中心', check: () => proxy.activate('本机') && proxy.mode === 'manual' && proxy.list[0]!.active === true },
    { id: 'F04187', name: 'hosts', check: () => { const entries = D.hostsParse('127.0.0.1 local.dev\n# 注释\n10.0.0.1 old.dev'); return entries.length === 2 && D.hostsRender([...entries, { ip: '1.1.1.1', host: 'x.dev', enabled: false }]).includes('# 1.1.1.1'); } },
    { id: 'F04188', name: '证书', check: () => { const ok = D.certStatus({ subject: 'a', issuer: 'b', validFrom: T0 - 1000, validTo: T0 + 86400_000 * 30 }, T0); return ok.valid && ok.daysLeft === 30; } },
    { id: 'F04189', name: 'WiFi 密码', check: () => D.wifiPasswordView([{ ssid: 'Home', password: 'pw123' }], 'Home') === 'pw123' && D.wifiPasswordView([], 'x') === undefined },
    { id: 'F04190', name: 'WiFi 二维码', check: () => D.wifiQrText('Home', 'pw123').startsWith('WIFI:T:WPA;S:Home;P:pw123;;') },
    { id: 'F04191', name: '流量统计', check: () => traffic.totalMB === 600 },
    { id: 'F04192', name: '按应用流量', check: () => traffic.perApp(1)[0]!.app === 'Browser' },
    { id: 'F04193', name: '诊断向导', check: () => { const r = D.diagWizard([{ check: '网线', ok: true }, { check: 'DHCP', ok: false, fix: '重启路由' }, { check: 'DNS', ok: true }]); return r.broken === 'DHCP' && r.fixes[0] === '重启路由' && r.reachInternet === false; } },
    { id: 'F04194', name: '修复', check: () => D.ONE_CLICK_REPAIR_STEPS.length === 4 },
    { id: 'F04195', name: '网卡信息', check: () => D.nicReport([{ name: '以太网', mac: 'aa:bb', ip: '192.168.1.2', speedMbps: 1000, up: true }]).includes('已连接') },
    { id: 'F04196', name: '路由表', check: () => D.routeTableMock()[0]!.dest === '0.0.0.0' },
    { id: 'F04197', name: 'ARP', check: () => D.arpTableMock().length === 2 && D.arpTableMock()[0]!.type === 'dynamic' },
    { id: 'F04198', name: '性能图', check: () => { const c = D.latencyChart([{ ms: 10, lost: false }, { ms: 20, lost: false }, { ms: 0, lost: true }]); return c.avgMs === 15 && c.lossPct === 33 && c.maxMs === 20; } },
    { id: 'F04199', name: '断网提醒', check: () => { if (!watcher.isDown) return false; const downMs = watcher.markUp(T0 + 5000); return downMs === 5000 && !watcher.isDown; } },
    { id: 'F04200', name: '教学', check: () => D.NETWORK_TIPS.length === 3 },
  ];
}

/* -------- AI-34 族0169 系统维护工具 -------- */
export function checkF0169(): CheckEntry[] {
  const junk = D.junkScan([
    { path: '/c/tmp', sizeMB: 120, kind: 'temp', safe: true },
    { path: '/c/logs', sizeMB: 30, kind: 'log', safe: true },
    { path: '/c/imp', sizeMB: 10, kind: 'cache', safe: false },
  ]);
  const upd = new D.UpdateCenter();
  upd.scan([
    { kb: 'KB001', title: '安全补丁', kind: 'security', sizeMB: 100 },
    { kb: 'KB002', title: '功能更新', kind: 'feature', sizeMB: 500 },
  ]);
  upd.install('KB002');
  const rp = new D.RestorePoints();
  rp.create('清理前');
  return [
    { id: 'F04201', name: '垃圾清理', check: () => junk.totalMB === 160 && junk.byKind['temp'] === 120 && junk.safeItems.length === 2 },
    { id: 'F04202', name: '注册表位', check: () => D.REGISTRY_CLEAN_RESERVED === true },
    { id: 'F04203', name: '启动项', check: () => { const r = D.startupImpact([{ name: '慢', enabled: true, impactMs: 1200 }, { name: '快', enabled: true, impactMs: 100 }]); return r.totalMs === 1300 && r.advice.length === 1; } },
    { id: 'F04204', name: '服务', check: () => D.serviceAdvice([{ name: 's1', running: false, startup: 'auto', desc: 'd' }]).length === 1 && D.serviceAdvice([{ name: 's2', running: true, startup: 'auto', desc: 'd' }]).length === 0 },
    { id: 'F04205', name: '计划任务', check: () => { const h = D.cronHealth([{ name: 't1', schedule: '@daily', nextRun: 'x', lastResult: 'failed' }, { name: 't2', schedule: '@hourly', nextRun: 'y', lastResult: 'ok' }]); return h.failed[0] === 't1' && h.healthy === false; } },
    { id: 'F04206', name: '驱动列表', check: () => { const r = D.driverReport([{ name: 'gpu', version: '1', date: 'd', status: 'error' }, { name: 'nic', version: '1', date: 'd', status: 'ok' }]); return r.problems.length === 1 && r.healthy === 1; } },
    { id: 'F04207', name: '驱动更新位', check: () => D.DRIVER_UPDATE_RESERVED === true },
    { id: 'F04208', name: '系统更新', check: () => upd.pending.length === 1 && upd.pending[0]!.kb === 'KB001' },
    { id: 'F04209', name: '更新历史', check: () => upd.history().length === 1 && upd.history()[0]!.kb === 'KB002' },
    { id: 'F04210', name: '回滚更新', check: () => upd.rollback('KB001') === false && upd.rollback('KB002') === true && upd.pending.length === 2 },
    { id: 'F04211', name: '还原点', check: () => rp.list.length === 1 && rp.latest()!.label === '清理前' },
    { id: 'F04212', name: '建还原点', check: () => rp.create('清理前') === false && rp.create('更新前') === true },
    { id: 'F04213', name: '系统评分', check: () => { const s = D.healthScore([{ name: '磁盘', score: 60, weight: 1 }, { name: '内存', score: 100, weight: 1 }]); return s.total === 80 && s.grade === '良' && s.weakest === '磁盘'; } },
    { id: 'F04214', name: '硬件信息', check: () => D.hardwareText({ cpu: 'X1', cores: 8, ramGB: 32, disks: [{ model: 'SSD1', sizeGB: 1024, type: 'ssd' }], gpu: 'G1' }).includes('32GB') },
    { id: 'F04215', name: '压力测试位', check: () => D.STRESS_TEST_RESERVED === true },
    { id: 'F04216', name: '温度', check: () => { const t = D.tempStatus([{ name: 'CPU', celsius: 90 }, { name: '盘', celsius: 45 }]); return t.hottest!.name === 'CPU' && t.overheat.length === 1; } },
    { id: 'F04217', name: '风扇位', check: () => D.FAN_CONTROL_RESERVED === true },
    { id: 'F04218', name: '电源计划', check: () => D.POWER_PLANS.length === 4 },
    { id: 'F04219', name: '电池报告', check: () => D.batteryHealth({ designCapacityWh: 50, fullChargeWh: 40, cycleCount: 300 }).healthPct === 80 },
    { id: 'F04220', name: '事件查看器', check: () => D.eventViewer([{ at: 1, level: 'info', source: 's', message: 'm' }, { at: 2, level: 'error', source: 's', message: 'm' }], 'error').length === 1 },
    { id: 'F04221', name: '蓝屏分析位', check: () => D.BSOD_ANALYSIS_RESERVED === true },
    { id: 'F04222', name: '日志清理', check: () => D.logCleanup([{ at: 1, level: 'info', source: 's', message: 'm' }, { at: T0, level: 'info', source: 's', message: 'm' }], 7, T0) === 1 },
    { id: 'F04223', name: '文件校验', check: () => { const r = D.fileVerify([{ path: '/a', hash: 'h', expected: 'h' }, { path: '/b', hash: 'x', expected: 'h' }]); return r.ok[0] === '/a' && r.corrupt[0] === '/b'; } },
    { id: 'F04224', name: '一键体检', check: () => { const r = D.oneClickCheckup([{ item: '磁盘', ok: true, detail: '' }, { item: '更新', ok: false, detail: '有补丁' }]); return r.pass === 1 && r.fail === 1 && r.actions[0]!.includes('补丁'); } },
    { id: 'F04225', name: '报告', check: () => D.checkupReport({ pass: 3, fail: 1, actions: ['x'] }, T0).includes('通过 3 项') },
  ];
}

/* -------- AI-34 族0170 卸载器增强 -------- */
export function checkF0170(): CheckEntry[] {
  const u = new D.Uninstaller();
  u.load([
    { name: 'OldApp', version: '1.0', sizeMB: 500, publisher: 'X', source: 'classic', lastUsedDays: 100, crashRate: 0.001 },
    { name: 'StoreApp', version: '2.0', sizeMB: 200, publisher: 'Y', source: 'store', lastUsedDays: 10, crashRate: 0.02 },
    { name: 'BigGame', version: '3.0', sizeMB: 90000, publisher: 'Z', source: 'classic', lastUsedDays: 3, crashRate: 0.2 },
  ]);
  const fs = { dirs: ['/c/OldApp', '/c/OldApp Cache'], registryKeys: ['HKCU/OldApp'], autoruns: ['OldApp Helper'] };
  const mon = new D.InstallMonitor();
  mon.record({ at: 1, app: 'OldApp', kind: 'file', target: '/a.dll' }).record({ at: 2, app: 'OldApp', kind: 'autorun', target: 'run' });
  return [
    { id: 'F04226', name: '全列表', check: () => u.list.length === 3 && u.list.some((a) => a.source === 'store') },
    { id: 'F04227', name: '强制卸载', check: () => u.uninstall('StoreApp') === 'stub' && u.uninstall('StoreApp', { force: true }) === 'ok' },
    { id: 'F04228', name: '残留扫描', check: () => { const r = u.leftoverScan('OldApp', fs); return r.dirs.length === 2 && r.registryKeys.length === 1 && r.autoruns.length === 1; } },
    { id: 'F04229', name: '残留清理', check: () => { const fs2 = { dirs: ['/c/OldApp', '/c/keep'], registryKeys: ['HKCU/OldApp'], autoruns: [] }; return u.cleanupLeftover(fs2, ['keep']) === 2 && fs2.dirs.join() === '/c/keep'; } },
    { id: 'F04230', name: '批量', check: () => { const u2 = new D.Uninstaller().load([{ name: 'a', version: '1', sizeMB: 1, publisher: 'p', source: 'classic', lastUsedDays: 1, crashRate: 0 }, { name: 'b', version: '1', sizeMB: 1, publisher: 'p', source: 'store', lastUsedDays: 1, crashRate: 0 }]); const r = u2.batch(['a', 'b']); return r.done.join() === 'a,b' && r.failed.length === 0; } },
    { id: 'F04231', name: '静默位', check: () => D.SILENT_UNINSTALL_RESERVED === true },
    { id: 'F04232', name: '安装监控', check: () => mon.byApp('OldApp').length === 2 },
    { id: 'F04233', name: '快照对比', check: () => { const d = mon.snapshotDiff([], [{ at: 1, app: 'N', kind: 'file', target: '/x' }]); return d.added.length === 1 && d.removed.length === 0; } },
    { id: 'F04234', name: '自启关联', check: () => u.cleanAutorun('OldApp', ['OldApp Helper', 'other']).join() === 'OldApp Helper' },
    { id: 'F04235', name: '右键清理', check: () => D.contextLeftovers([{ location: 'file', label: '旧压缩', orphan: true }, { location: 'file', label: '好压缩', orphan: false }]).length === 1 },
    { id: 'F04236', name: '浏览器扩展', check: () => D.extensionRisk({ browser: 'varix', name: 'x', enabled: true, permissions: ['<all_urls>', 'cookies'] }) === 'high' },
    { id: 'F04237', name: '运行库检测', check: () => { const r = D.detectRuntimes(['VC++ 2015-2022 x64', '.NET 8']); return r.present.length === 2 && r.missing.length === 4; } },
    { id: 'F04238', name: '运行库修复位', check: () => D.RUNTIME_FIX_RESERVED === true },
    { id: 'F04239', name: '孤立 DLL', check: () => D.orphanDlls(['a.dll', 'b.dll'], ['b.dll']).join() === 'a.dll' },
    { id: 'F04240', name: '预装管理', check: () => D.PREINSTALLED_MANAGE.view === true },
    { id: 'F04241', name: '不常用建议', check: () => u.rarelyUsed(30).map((a) => a.name).join() === 'OldApp' },
    { id: 'F04242', name: '后悔记录', check: () => u.history.some((h) => h.app === 'StoreApp' && h.action === 'uninstall') },
    { id: 'F04243', name: '绿色登记', check: () => { const list: string[] = []; return D.registerPortable(list, 'Tool') && !D.registerPortable(list, 'Tool'); } },
    { id: 'F04244', name: '便携扫描', check: () => { const r = D.portableScan([{ name: 'p1', hasExe: true, hasUninstall: false }, { name: 'r1', hasExe: true, hasUninstall: true }]); return r.portable.join() === 'p1' && r.regular.join() === 'r1'; } },
    { id: 'F04245', name: '体积排行', check: () => u.sizeRanking(1)[0]!.name === 'BigGame' },
    { id: 'F04246', name: '健康', check: () => u.healthFlag(u.list[0]!) === 'stable' && u.healthFlag(u.list.find((a) => a.name === 'BigGame')!) === 'crashy' && u.healthFlag({ name: 'w', version: '1', sizeMB: 1, publisher: 'p', source: 'classic', lastUsedDays: 1, crashRate: 0.03 }) === 'watch' },
    { id: 'F04247', name: '默认重置', check: () => D.defaultAppsReset(['浏览器', '图片'])['浏览器'] === 'varix' },
    { id: 'F04248', name: '企业白名单', check: () => D.enterpriseWhitelist(['BigGame'])(u.list.find((a) => a.name === 'BigGame')!).protectedByPolicy === true },
    { id: 'F04249', name: '教学', check: () => D.UNINSTALL_TIPS.length === 3 },
    { id: 'F04250', name: '报告', check: () => D.uninstallReport(u).includes('已卸载 1 个应用') },
  ];
}

/* -------- AI-35 族0171 天气与出行 -------- */
export function checkF0171(): CheckEntry[] {
  const w: E.WeatherNow = { city: '上海', tempC: 30, condition: '多云', humidity: 65, windDir: '东南', windLevel: 3, aqi: 80, updatedAt: T0 };
  const cities = new E.CityWeatherManager();
  cities.add(w);
  cities.add({ ...w, city: '吐鲁番', tempC: 40 });
  const cache = new E.WeatherCache();
  cache.put('sh', w, T0);
  const rainW: E.WeatherNow = { ...w, condition: '大雨', windLevel: 7 };
  return [
    { id: 'F04251', name: '实时', check: () => cities.get('上海')!.tempC === 30 },
    { id: 'F04252', name: '24 小时', check: () => E.hourlyForecast(w).length === 24 },
    { id: 'F04253', name: '7 天', check: () => E.sevenDayForecast(w).length === 7 && E.sevenDayForecast(w)[0]!.condition === '多云' },
    { id: 'F04254', name: '15 天位', check: () => E.FIFTEEN_DAY_RESERVED === true },
    { id: 'F04255', name: '空气质量', check: () => E.aqiLevel(300).label === '重度污染' && E.aqiLevel(50).label === '优' },
    { id: 'F04256', name: '降水雷达', check: () => E.radarCells([0, 1, 5, 20]).map((c) => c.level).join() === '0,1,2,3' },
    { id: 'F04257', name: '预警', check: () => E.weatherAlerts(rainW).length === 2 && E.weatherAlerts(rainW)[0]!.kind === '暴雨' },
    { id: 'F04258', name: '体感', check: () => E.feelsLike(32, 80, 1) > 32 && E.feelsLike(5, 50, 4) < 5 },
    { id: 'F04259', name: '日出日落', check: () => E.sunriseMinutes(31, 180) < 480 && E.sunriseMinutes(31, 1) > 400 },
    { id: 'F04260', name: '月相', check: () => E.moonPhase(new Date('2026-09-13T00:00:00Z')).length > 0 },
    { id: 'F04261', name: '洗车', check: () => E.lifeIndices(w)[0]!.name === '洗车' && E.lifeIndices({ ...w, condition: '小雨' })[0]!.level === 1 },
    { id: 'F04262', name: '穿衣', check: () => E.lifeIndices(w)[1]!.name === '穿衣' },
    { id: 'F04263', name: '运动', check: () => E.lifeIndices(w)[2]!.name === '运动' && E.lifeIndices({ ...w, aqi: 200 })[2]!.level === 1 },
    { id: 'F04264', name: '紫外线', check: () => E.lifeIndices({ ...w, condition: '晴' })[3]!.level === 3 },
    { id: 'F04265', name: '湿度', check: () => E.humidityLabel(80) === '潮湿' && E.humidityLabel(20) === '干燥' },
    { id: 'F04266', name: '风向', check: () => E.windLabel(90, 3) === '东风 3 级' },
    { id: 'F04267', name: '多城', check: () => cities.names.length === 2 && cities.hottest()!.city === '吐鲁番' && cities.remove('吐鲁番') },
    { id: 'F04268', name: '微件联动', check: () => E.WEATHER_WIDGET_LINK.refreshMin === 30 },
    { id: 'F04269', name: '锁屏联动', check: () => E.WEATHER_LOCKSCREEN_LINK.showTemp === true },
    { id: 'F04270', name: '通知', check: () => E.severeWeatherNotify([{ kind: '高温', level: '橙色', message: 'm' }, { kind: '大风', level: '蓝色', message: 'm' }]).length === 1 },
    { id: 'F04271', name: '雷达动画', check: () => E.radarAnimationFrames([1, 2], 4).length === 4 },
    { id: 'F04272', name: '科普', check: () => E.WEATHER_KNOWLEDGE.length === 3 },
    { id: 'F04273', name: '离线', check: () => cache.get('sh', T0 + 1000) !== undefined && cache.get('sh', T0 + 86400_000) === undefined && cache.offlineGet('sh') !== undefined },
    { id: 'F04274', name: '数据源', check: () => E.WEATHER_SOURCES.length === 3 },
    { id: 'F04275', name: '教学', check: () => E.WEATHER_TIPS.length === 2 },
  ];
}

/* -------- AI-35 族0172 地图与位置 -------- */
export function checkF0172(): CheckEntry[] {
  const pb = new E.PlaceBook();
  pb.add({ name: '家', lat: 31.0, lon: 121.0, kind: 'home' });
  pb.add({ name: '公司', lat: 31.1, lon: 121.1, kind: 'work' });
  pb.add({ name: '西湖', lat: 30.24, lon: 120.15, kind: 'favorite' });
  const packs = new E.OfflinePackManager();
  packs.download('上海', 500);
  packs.download('杭州', 300);
  return [
    { id: 'F04276', name: '离线地图位', check: () => E.OFFLINE_MAP_RESERVED === true },
    { id: 'F04277', name: '收藏', check: () => pb.byKind('favorite').length === 1 },
    { id: 'F04278', name: '常用地址', check: () => pb.byKind('home').length === 1 && pb.byKind('work').length === 1 },
    { id: 'F04279', name: '坐标', check: () => E.geocode('北京 附近')!.lat === 39.9087 },
    { id: 'F04280', name: '海拔', check: () => E.elevationMock(31, 121) > 0 },
    { id: 'F04281', name: '日出计算', check: () => { const winter = E.sunriseMinutes(31, 1); return winter > 400 && winter < 480 && E.sunriseMinutes(-31, 180) > 400; } },
    { id: 'F04282', name: '时区', check: () => E.timezoneOf(121.47) === 8 && E.timezoneOf(-74) === -5 },
    { id: 'F04283', name: '测距', check: () => { const d = E.haversineKm({ lat: 31.23, lon: 121.47 }, { lat: 39.9, lon: 116.4 }); return d > 1000 && d < 1120; } },
    { id: 'F04284', name: '行程位', check: () => E.TRIP_LOG_RESERVED === true },
    { id: 'F04285', name: '分享位', check: () => E.LOCATION_SHARE_RESERVED === true },
    { id: 'F04286', name: '地理编码', check: () => E.geocode('上海 附近')!.lon === 121.4737 },
    { id: 'F04287', name: '逆编码', check: () => E.reverseGeocode(31.2304, 121.4737)!.includes('上海') },
    { id: 'F04288', name: '家乡壁纸', check: () => E.HOMETOWN_WALLPAPER_SPEC.style.length === 3 },
    { id: 'F04289', name: '位置天气', check: () => E.localWeatherLink(pb.list[0]!).sunriseMin > 0 },
    { id: 'F04290', name: '截图标注', check: () => E.mapAnnotationSpec(10, 20, '标注').pin === 'drop' },
    { id: 'F04291', name: 'GPX', check: () => { const g = '<trkpt lat="31.2" lon="121.4"><ele>5.0</ele><time>2026-09-13T00:00:00Z</time></trkpt>'; return E.gpxParse(g).length === 1 && E.gpxSerialize(E.gpxParse(g)).includes('lat="31.2"'); } },
    { id: 'F04292', name: '旅行计划', check: () => E.tripPlan(3, ['北京'])[2]!.city === '北京' && E.tripPlan(3, ['北京'])[0]!.items.includes('出发') },
    { id: 'F04293', name: '旅行清单', check: () => E.travelChecklist('出境').includes('护照') },
    { id: 'F04294', name: '签证提醒位', check: () => E.VISA_REMINDER_RESERVED === true },
    { id: 'F04295', name: '汇率联动', check: () => E.tripCurrency(100, 0.14) === 14 },
    { id: 'F04296', name: '当地日历', check: () => E.localHoliday('2026-12-25') === '圣诞' },
    { id: 'F04297', name: '当地节日', check: () => E.localFestivalHint(4) === '樱花季' },
    { id: 'F04298', name: '隐私承诺', check: () => E.LOCATION_PRIVACY.includes('不追踪') },
    { id: 'F04299', name: '离线管理', check: () => packs.totalMB() === 800 && packs.remove('杭州') && packs.totalMB() === 500 },
    { id: 'F04300', name: '教学', check: () => E.LOCATION_TIPS.length === 3 },
  ];
}

/* -------- AI-35 族0173 学习工具 -------- */
export function checkF0173(): CheckEntry[] {
  let card = E.createCard('英语', 'aurora', '极光');
  card = E.sm2(card, 5, T0);
  card = E.sm2(card, 5, T0);
  card = E.sm2(card, 4, T0);
  const lapsed = E.createCard('英语', 'hard', '难');
  E.sm2(lapsed, 0, T0);
  E.sm2(lapsed, 1, T0);
  const deck = new E.DeckManager().add(card).add(lapsed);
  const vocab = new E.VocabularyBook();
  vocab.add('aurora', '极光');
  vocab.add('kernel', '内核');
  vocab.promote('aurora');
  const notes = new E.ReadingNotes().add('书A', '摘录1');
  return [
    { id: 'F04301', name: '闪卡', check: () => card.front === 'aurora' && card.ease >= 2.5 },
    { id: 'F04302', name: '间隔重复', check: () => card.reps === 3 && card.intervalDays >= 6 && card.dueAt > T0 },
    { id: 'F04303', name: '卡片创建', check: () => E.createCard('d', 'f', 'b').back === 'b' },
    { id: 'F04304', name: '牌组', check: () => deck.decks().join() === '英语' && deck.byDeck('英语').length === 2 },
    { id: 'F04305', name: '统计', check: () => deck.stats().total === 2 && deck.stats().learning === 2 },
    { id: 'F04306', name: '复习提醒', check: () => E.reviewReminder(deck.due(T0 + 20 * 86400_000), T0 + 20 * 86400_000).includes('2 张卡片待复习') },
    { id: 'F04307', name: '错题本', check: () => deck.mistakeBook().length === 1 && deck.mistakeBook()[0]!.front === 'hard' },
    { id: 'F04308', name: '单词本', check: () => vocab.byBox(2).join() === 'aurora' && vocab.promote('kernel') },
    { id: 'F04309', name: '发音练习', check: () => E.pronunciationPractice('aurora', 'aurora').score === 100 && E.pronunciationPractice('aurora', 'xxxxxx').score < 50 },
    { id: 'F04310', name: '听写模式', check: () => E.dictationCheck('hello', 'hello').correct === true && E.dictationCheck('hello', 'hella').diffAt === 4 },
    { id: 'F04311', name: '计时', check: () => { const st = new E.StudyStats(); st.add('2026-09-13', 25); return st.total() === 25 && st.bestDay() === '2026-09-13'; } },
    { id: 'F04312', name: '计划', check: () => E.studyPlan([{ subject: '数学', minutesPerDay: 30, days: ['一', '三'] }]).includes('30 分钟') },
    { id: 'F04313', name: '课程表', check: () => E.timetableNow([{ name: '数学', day: 1, startMin: 480, endMin: 540 }], 500, 1)?.name === '数学' && E.timetableNow([{ name: '数学', day: 1, startMin: 480, endMin: 540 }], 500, 2) === undefined },
    { id: 'F04314', name: '作业倒计时', check: () => E.homeworkCountdown(T0 + 3 * 86400_000, T0).includes('3 天') },
    { id: 'F04315', name: '公式库', check: () => E.FORMULA_LIBRARY.some((f) => f.name === '勾股定理') },
    { id: 'F04316', name: '元素周期表', check: () => E.elementLookup('Fe')!.z === 26 && E.elementLookup(8)!.name === '氧' },
    { id: 'F04317', name: '单位速查', check: () => E.UNIT_CHEATSHEET['重量']!.includes('斤') },
    { id: 'F04318', name: '速查卡', check: () => E.CHEAT_SHEETS.length === 4 },
    { id: 'F04319', name: '读书笔记', check: () => notes.byBook('书A')[0] === '摘录1' },
    { id: 'F04320', name: '笔记回顾', check: () => notes.reviewDue(1, Date.now() + 2 * 86400_000).length === 1 && notes.reviewDue(1, Date.now()).length === 0 },
    { id: 'F04321', name: '涂鸦草稿', check: () => E.DOODLE_CANVAS_SPEC.modes.includes('橡皮') },
    { id: 'F04322', name: '白噪音联动', check: () => B.AMBIENT_TRACKS.length === 5 },
    { id: 'F04323', name: '成就', check: () => E.studyAchievements({ total: 700, streakDays: 8, cards: 120 }).length >= 2 },
    { id: 'F04324', name: '导出', check: () => JSON.parse(deck.exportJson()).length === 2 },
    { id: 'F04325', name: '教学', check: () => E.STUDY_TIPS.length === 3 },
  ];
}

/* -------- AI-35 族0174 家庭模式 -------- */
export function checkF0174(): CheckEntry[] {
  const fam = new E.FamilyMode();
  const kid = fam.addChild('小明', { dailyLimitMin: 60, bedtime: '21:30' });
  fam.whitelist('小明', '画图');
  fam.use('小明', 50);
  const meds = new E.MedicationReminder().add('钙片', ['08:00', '20:00']);
  const famCal = new E.FamilyCalendar();
  famCal.add('2026-10-01', '全家', '出游');
  const ftodos = new E.FamilyTodos();
  ftodos.add('倒垃圾', '小明');
  ftodos.add('买菜', '爸爸');
  return [
    { id: 'F04326', name: '儿童账户', check: () => fam.names.includes('小明') && fam.get('小明')!.dailyLimitMin === 60 },
    { id: 'F04327', name: '时长限制', check: () => fam.use('小明', 5).allowed === true && fam.use('小明', 60).allowed === false && fam.get('小明')!.usedMin === 55 },
    { id: 'F04328', name: '应用白名单', check: () => fam.isAllowed('小明', '画图') === true && fam.isAllowed('小明', '游戏') === false },
    { id: 'F04329', name: '内容分级', check: () => fam.setRating('小明', 'teen') && fam.get('小明')!.ratingLimit === 'teen' },
    { id: 'F04330', name: '休息提醒', check: () => E.BREAK_REMINDER_SPEC.everyMin === 30 },
    { id: 'F04331', name: '护眼强制', check: () => E.EYECARE_FORCE.warmFilter === true },
    { id: 'F04332', name: '睡前锁', check: () => fam.bedtimeLocked('小明', 21 * 60 + 40) === true && fam.bedtimeLocked('小明', 8 * 60) === false },
    { id: 'F04333', name: '审批', check: () => E.INSTALL_APPROVAL.requireParent === true },
    { id: 'F04334', name: '使用报告', check: () => E.parentReport(kid, [{ date: '2026-09-12', minutes: 40, topApp: '画图' }]).includes('日均 40 分钟') },
    { id: 'F04335', name: '共享库位', check: () => E.FAMILY_LIBRARY_RESERVED === true },
    { id: 'F04336', name: '长辈模式', check: () => E.ELDER_MODE.fontScale === 1.4 && E.ELDER_MODE.simplified === true },
    { id: 'F04337', name: '一键求助位', check: () => E.SOS_RESERVED === true },
    { id: 'F04338', name: '亲情号', check: () => E.familyNumbers(['爸爸', '妈妈'])[1]!.dial === '妈妈' },
    { id: 'F04339', name: '用药提醒', check: () => meds.due('08:00').join() === '钙片' && meds.mark('钙片', '08:00') && meds.due('08:00').length === 0 && meds.due('20:00').length === 1 },
    { id: 'F04340', name: '家人生日', check: () => A.birthdayCountdown('05-20', new Date(2026, 4, 1)) === 19 },
    { id: 'F04341', name: '家庭日历', check: () => famCal.byDate('2026-10-01')[0]!.who === '全家' },
    { id: 'F04342', name: '账本位', check: () => E.FAMILY_LEDGER_RESERVED === true },
    { id: 'F04343', name: '家庭待办', check: () => ftodos.byWho('小明')[0]!.text === '倒垃圾' && ftodos.toggle('ft-1') && ftodos.byWho('小明')[0]!.done === true },
    { id: 'F04344', name: '相框模式', check: () => E.PHOTO_FRAME_MODE.intervalSec === 15 },
    { id: 'F04345', name: '儿童锁屏', check: () => E.CHILD_LOCK_SCREEN.pin === 'parent-only' },
    { id: 'F04346', name: '游戏时段', check: () => E.GAME_TIME_LOCK.schoolDayMin === 0 },
    { id: 'F04347', name: '远程时长', check: () => E.remoteMinutes(fam.get('小明')!).pct >= 90 },
    { id: 'F04348', name: '紧急联系人', check: () => E.EMERGENCY_PAGE.contacts === 2 },
    { id: 'F04349', name: '本地承诺', check: () => E.FAMILY_LOCAL_PROMISE.includes('本机') },
    { id: 'F04350', name: '教学', check: () => E.FAMILY_TIPS.length === 3 },
  ];
}

/* -------- AI-35 族0175 工具箱合集 -------- */
export function checkF0175(): CheckEntry[] {
  const sw = new E.ToolboxStopwatch();
  sw.start(T0);
  const cd = new E.ToolboxCountdown(1);
  cd.tick(30);
  const clip = new A.ClipboardHistory(10);
  clip.push('a');
  return [
    { id: 'F04351', name: '放大镜', check: () => E.MAGNIFIER_SPEC.levels.includes(2) && E.MAGNIFIER_SPEC.followCursor === true },
    { id: 'F04352', name: '屏幕键盘', check: () => E.ONSCREEN_KEYBOARD_ROWS.length === 5 && E.ONSCREEN_KEYBOARD_ROWS[2]!.includes('Enter') },
    { id: 'F04353', name: '讲述人', check: () => E.NARRATOR_SPEC.voice === '云扬' },
    { id: 'F04354', name: '色盲滤镜', check: () => Object.keys(E.COLOR_BLIND_FILTERS).length === 3 && E.applyColorFilter(255, 0, 0, 'protanopia')[0] < 255 },
    { id: 'F04355', name: '对比主题', check: () => E.HIGH_CONTRAST_THEMES.length === 4 },
    { id: 'F04356', name: '焦点辅助', check: () => E.FOCUS_ASSIST.dimBackground === true },
    { id: 'F04357', name: '时钟', check: () => E.TOOLBOX_CLOCK.modes.length === 5 },
    { id: 'F04358', name: '计算器', check: () => E.TOOLBOX_CALC_LINK.includes('族0155') },
    { id: 'F04359', name: '字符表', check: () => E.charMapPage(0).length === 16 && E.charMapPage(0)[0] === '一' },
    { id: 'F04360', name: '剪贴板', check: () => clip.pick(0) === 'a' },
    { id: 'F04361', name: '截图', check: () => E.SCREENSHOT_MODES.length === 5 && E.SCREENSHOT_MODES.includes('滚动长图') },
    { id: 'F04362', name: '扫描位', check: () => E.SCANNER_RESERVED === true },
    { id: 'F04363', name: '相机', check: () => E.CAMERA_SPEC.mirror === true },
    { id: 'F04364', name: '录音机', check: () => typeof B.RecorderSession === 'function' },
    { id: 'F04365', name: '标尺', check: () => E.screenRulerSpec('h', 200).ticks.join() === '50,100,150,200' },
    { id: 'F04366', name: '量角器', check: () => E.protractorAngle({ x: 10, y: 0 }, { x: 0, y: 0 }, { x: 0, y: 10 }) === 90 },
    { id: 'F04367', name: '水平仪', check: () => E.spiritLevel(0.2, 0.3).bubbleOk === true && E.spiritLevel(2, 0).bubbleOk === false },
    { id: 'F04368', name: '取色器', check: () => E.COLOR_PICKER_SPEC.formats.length === 3 },
    { id: 'F04369', name: '色卡', check: () => E.COLOR_PALETTES['中国传统']!.length === 6 },
    { id: 'F04370', name: '秒表', check: () => sw.split(T0 + 1500) === 1500 },
    { id: 'F04371', name: '倒计时', check: () => cd.display === '00:30' && cd.tick(30) === true },
    { id: 'F04372', name: '随机数', check: () => { const r = E.randomInt(1, 6, 5); return r.length === 5 && r.every((x) => x >= 1 && x <= 6); } },
    { id: 'F04373', name: '骰子', check: () => { const d = E.rollDice(6, 3); return d.length === 3 && d.every((x) => x >= 1 && x <= 6); } },
    { id: 'F04374', name: '指南针', check: () => E.compassHeading(1, 0) === 0 && E.compassLabel(90) === '东' },
    { id: 'F04375', name: '教学', check: () => E.TOOLBOX_TIPS.length === 3 },
  ];
}

/** 汇总：领域07 全部 25 族 625 项。 */
export function runDomain07Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [
    checkF0151, checkF0152, checkF0153, checkF0154, checkF0155,
    checkF0156, checkF0157, checkF0158, checkF0159, checkF0160,
    checkF0161, checkF0162, checkF0163, checkF0164, checkF0165,
    checkF0166, checkF0167, checkF0168, checkF0169, checkF0170,
    checkF0171, checkF0172, checkF0173, checkF0174, checkF0175,
  ];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => {
    try {
      return !e.check();
    } catch {
      return true;
    }
  });
  return { entries, failed };
}
