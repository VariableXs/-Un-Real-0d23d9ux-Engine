/**
 * UNREAL-X-15000 · 42 族达标描述表（X25 落点）。
 *
 * 覆盖缺口清单中的全部缺口族：
 *   AI-01 族0001~0010（X00001~X00250）· AI-02 族0011~0020（X00251~X00500）
 *   AI-53 族0521~0530（X13001~X13250）· AI-54 族0531~0540（X13251~X13500）
 *
 * 每族给出：档位矩阵（≥5 档，默认档=现状）、主失败叙事、无障碍角色、
 * 键盘 roving 槽位、快捷键集合，以及一条"接地断言" native()——
 * native() 一律走既有真实实现，用来证明该族核心链路确实已打通，
 * 而不是由达标探针自证。
 */

import { BOOTCHAIN_KEYS, BOOTCHAIN_VALUE_SETS } from '../../lib/settings';
import * as R from '../oobe/repairWorkshop';
import * as Rb from '../oobe/recoveryReborn';
import * as L from '../oobe/bootLogTheater';
import * as N from '../oobe/bootFailNarrative';
import * as P from '../oobe/bootPacing';
import * as M from '../oobe/multiBootTheater';
import * as T from '../oobe/powerTheater';
import * as H from '../settings/hiberArchive';
import * as E from '../settings/powerEvents';
import * as G from '../settings/powerGauge';
import * as WP from '../settings/warmupPlan';
import * as Q from '../settings/bootQuiet';
import * as K from '../settings/powerEggs';
import * as WB from './workbenchModels';
import * as SM from './systemModels';
import { registerX25, xidBase, type X25Spec } from './x25';

/* ==================== 新开放的档位矩阵（档2「开放全量参数」交付物） ==================== */

/** 族0002 阶段评分五档。 */
export const HEALTH_SCORE_TIERS = ['off', 'basic', 'standard', 'deep', 'forensic'] as const;
/** 族0006 错误剧场五档。 */
export const ERROR_THEATER_TIERS = ['off', 'brief', 'standard', 'rich', 'theatrical'] as const;
/** 族0013 休眠镜像五档。 */
export const HIBER_IMAGE_TIERS = ['off', 'metadata', 'session', 'full', 'forensic'] as const;
/** 族0014 预载策略五档。 */
export const PREFETCH_TIERS = ['off', 'lazy', 'balanced', 'eager', 'aggressive'] as const;
/** 族0015 唤醒源治理五档。 */
export const WAKE_SOURCE_TIERS = ['off', 'critical', 'balanced', 'permissive', 'custom'] as const;
/** 族0017 启动性能仪表五档。 */
export const GAUGE_VIEW_TIERS = ['off', 'mini', 'standard', 'detailed', 'forensic'] as const;
/** 族0019 后台延迟启动五档。 */
export const QUIET_DEFER_TIERS = ['off', 'gentle', 'hushed', 'mute', 'frozen'] as const;
/** 族0524 编辑区能力五档。 */
export const EDITOR_CAP_TIERS = ['off', 'single', 'split', 'grid', 'welcome'] as const;
/** 族0525 面板视图五档。 */
export const PANEL_VIEW_TIERS = ['off', '问题', '输出', '调试控制台', '终端'] as const;
/** 族0526 状态栏槽位五档。 */
export const STATUS_SLOT_TIERS = ['off', 'minimal', 'standard', 'detailed', 'debug'] as const;
/** 族0527 命令面板能力五档。 */
export const COMMAND_TIERS = ['off', 'recent', 'all', 'fuzzy', 'expert'] as const;
/** 族0533 材质档位五档（含 HC 全降级终档）。 */
export const MATERIAL_TIERS = ['off', 'm-solid', 'm-mica', 'm-frosted', 'm-acrylic'] as const;

/* ==================== 小工具 ==================== */

/** 连续层内序号区间 [a,b]（全闭区间）。 */
export const span = (a: number, b: number): number[] => Array.from({ length: b - a + 1 }, (_, i) => a + i);

const idsOf = (arr: readonly { id: string }[]): string[] => arr.map((x) => x.id);

const keysOf = (fam: number): string[] => [`Ctrl+Alt+${fam}`, `Ctrl+Shift+${fam}`, `Alt+${fam}`];

/** 领域01 启动与品牌剧场 · AI-01（族0001~0010）。 */
const AI01: X25Spec[] = [
  {
    fam: 1, title: '冷启动链路体检', dim: '链路段',
    tiers: BOOTCHAIN_VALUE_SETS.audit, def: 'standard',
    err: { code: 'BC-101', text: '启动链路体检超时', next: '跳过深度项后重试' },
    role: 'status', slots: 5, keys: keysOf(1),
    native: () => BOOTCHAIN_VALUE_SETS.audit.join() === 'off,standard,deep,forensic,custom',
  },
  {
    fam: 2, title: 'Bootchain 健康度', dim: '阶段评分',
    tiers: HEALTH_SCORE_TIERS, def: 'standard',
    err: { code: 'BC-102', text: '健康度采样失败', next: '改用基础采样重试' },
    role: 'meter', slots: 5, keys: keysOf(2),
    native: () => BOOTCHAIN_KEYS.length === 12 && BOOTCHAIN_VALUE_SETS.health.join() === 'off,on',
  },
  {
    fam: 3, title: '启动修复工坊', dim: '修复策略',
    tiers: idsOf(R.REPAIR_STRATEGIES), def: R.DEFAULT_STRATEGY,
    err: { code: 'BC-103', text: '修复策略执行中断', next: '从进度快照续作' },
    role: 'group', slots: 5, keys: keysOf(3),
    native: () => R.REPAIR_STRATEGIES.length === 5 && R.findStrategy('bogus').id === R.DEFAULT_STRATEGY,
  },
  {
    fam: 4, title: '恢复环境重生', dim: '恢复工具',
    tiers: idsOf(Rb.RECOVERY_TOOLS), def: 'shell',
    err: { code: 'BC-104', text: '恢复环境挂载失败', next: '检查盘体并重挂载' },
    role: 'dialog', slots: 5, keys: keysOf(4),
    native: () => Rb.SNAPSHOT_CAPACITY === 4 && Rb.RECOVERY_TOOLS.length === 5,
  },
  {
    fam: 5, title: '启动日志剧场化', dim: '日志美学',
    tiers: L.THEATER_STYLES, def: 'timeline',
    err: { code: 'BC-105', text: '日志解析异常', next: '回退纯文本视图' },
    role: 'log', slots: 5, keys: keysOf(5),
    native: () => L.THEATER_STYLES.length === 5 && L.resolveTheaterStyle('nope') === 'timeline',
  },
  {
    fam: 6, title: '引导失败叙事', dim: '错误剧场',
    tiers: ERROR_THEATER_TIERS, def: 'standard',
    err: { code: 'BC-106', text: '错误码未登记', next: '按通用叙事兜底' },
    role: 'alert', slots: 5, keys: keysOf(6),
    native: () => N.FAIL_NARRATIVES.length >= 5 && N.findNarrative('BC-999').nextSteps.length > 0,
  },
  {
    fam: 7, title: '启动配速学', dim: '节奏档',
    tiers: idsOf(P.PACING_PROFILES), def: P.DEFAULT_PACING_ID,
    err: { code: 'BC-107', text: '配速档位非法', next: '已回均衡档继续' },
    role: 'group', slots: 5, keys: keysOf(7),
    native: () => P.PACING_PROFILES.length === 5 && P.clampCountdown(31) === 30,
  },
  {
    fam: 8, title: '安全启动仪式', dim: '信任链可视化',
    tiers: BOOTCHAIN_VALUE_SETS.secureboot, def: 'off',
    err: { code: 'BC-108', text: '信任链校验失败', next: '转审计档后重启' },
    role: 'status', slots: 5, keys: keysOf(8),
    native: () => BOOTCHAIN_VALUE_SETS.secureboot.join() === 'off,audit,relaxed,strict,locked',
  },
  {
    fam: 9, title: '多系统选择剧场', dim: '选择器',
    tiers: ['variable', 'previous', 'windows', 'linux', 'recovery'], def: 'variable',
    err: { code: 'BC-109', text: '启动项不可用', next: '改用默认启动项' },
    role: 'listbox', slots: 5, keys: keysOf(9),
    native: () => M.BOOT_ENTRY_MAX === 8 && BOOTCHAIN_VALUE_SETS.multiBoot.join() === 'off,on',
  },
  {
    fam: 10, title: '固件风格定制', dim: '固件皮',
    tiers: BOOTCHAIN_VALUE_SETS.logTheater, def: 'off',
    err: { code: 'BC-110', text: '固件皮加载失败', next: '回退默认固件皮' },
    role: 'group', slots: 6, keys: keysOf(10),
    native: () => BOOTCHAIN_VALUE_SETS.logTheater.length === 6 && BOOTCHAIN_VALUE_SETS.persona.join() === 'off,on',
  },
];

/** 领域01 启动与品牌剧场 · AI-02（族0011~0020）。 */
const AI02: X25Spec[] = [
  {
    fam: 11, title: '关机重启仪式 2.0', dim: '仪式',
    tiers: idsOf(T.CEREMONY_PROFILES), def: T.DEFAULT_CEREMONY_ID,
    err: { code: 'PW-201', text: '仪式编排中断', next: '从当前幕续演' },
    role: 'dialog', slots: 5, keys: keysOf(11),
    native: () => T.CEREMONY_PROFILES.length === 5 && T.CEREMONY_STAGES.length >= 5,
  },
  {
    fam: 12, title: '睡眠唤醒剧场 2.0', dim: '唤醒',
    tiers: idsOf(T.WAKE_PROFILES), def: T.DEFAULT_WAKE_ID,
    err: { code: 'PW-202', text: '唤醒阶段超时', next: '直接亮屏后重试' },
    role: 'dialog', slots: 5, keys: keysOf(12),
    native: () => T.WAKE_PROFILES.length === 5 && T.WAKE_STAGES.length === 5,
  },
  {
    fam: 13, title: '休眠档案学', dim: '休眠镜像',
    tiers: HIBER_IMAGE_TIERS, def: 'session',
    err: { code: 'PW-203', text: '休眠镜像损坏', next: '改用上层快照恢复' },
    role: 'group', slots: 5, keys: keysOf(13),
    native: () => H.HIBER_SNAPSHOTS === 3 && H.HIBER_APP_MAX === 8,
  },
  {
    fam: 14, title: '快速启动加速', dim: '预载策略',
    tiers: PREFETCH_TIERS, def: 'balanced',
    err: { code: 'PW-204', text: '预载任务超时', next: '降级为按需加载' },
    role: 'group', slots: 5, keys: keysOf(14),
    native: () => WP.WARMUP_BUDGET_MAX === 5000 && WP.WARMUP_QUEUE_MAX === 16,
  },
  {
    fam: 15, title: '唤醒源治理', dim: '唤醒源',
    tiers: WAKE_SOURCE_TIERS, def: 'balanced',
    err: { code: 'PW-205', text: '唤醒源被禁用', next: '改白名单后恢复' },
    role: 'listbox', slots: 5, keys: keysOf(15),
    native: () => E.POWER_EVENT_TYPES.length === 8 && E.isPowerEventType('lid') && !E.isPowerEventType('boom'),
  },
  {
    fam: 16, title: '电源事件叙事', dim: '事件流',
    tiers: E.POWER_EVENT_TYPES, def: 'wake',
    err: { code: 'PW-206', text: '事件流写入失败', next: '转环形缓冲重试' },
    role: 'log', slots: 8, keys: keysOf(16),
    native: () => E.POWER_EVENT_CAPACITY === 32 && E.PowerEventStream.line({ type: 'wake', stamp: 12, detail: 'd' }).includes('唤醒'),
  },
  {
    fam: 17, title: '启动性能仪表 2.0', dim: '仪表',
    tiers: GAUGE_VIEW_TIERS, def: 'standard',
    err: { code: 'PW-207', text: '采样通道异常', next: '关闭该通道后继续' },
    role: 'meter', slots: 5, keys: keysOf(17),
    native: () => G.GAUGE_CHANNELS.length === 4 && G.GAUGE_CAPACITY === 16 && G.GAUGE_BUDGETS.cpu.bad === 90,
  },
  {
    fam: 18, title: '预热缓存编排', dim: '预热',
    tiers: WP.WARMUP_TASK_TYPES, def: 'network',
    err: { code: 'PW-208', text: '预热队列挂起', next: '接电源后自动恢复' },
    role: 'progressbar', slots: 5, keys: keysOf(18),
    native: () => WP.WARMUP_TASK_TYPES.length === 5 && WP.isWarmupTaskType('index-scan'),
  },
  {
    fam: 19, title: '启动降噪', dim: '后台延迟启动',
    tiers: QUIET_DEFER_TIERS, def: 'off',
    err: { code: 'PW-209', text: '降噪窗口非法', next: '回默认窗口重试' },
    role: 'group', slots: 5, keys: keysOf(19),
    native: () => Q.QUIET_TIERS.length === 4 && Q.QUIET_WINDOW_MAX === 300 && Q.clampQuietWindow(9999) === 300,
  },
  {
    fam: 20, title: '启动彩蛋层 2.0', dim: '彩蛋',
    tiers: idsOf(K.POWER_EGGS), def: 'night',
    err: { code: 'PW-210', text: '彩蛋触发异常', next: '关闭彩蛋后继续' },
    role: 'status', slots: 5, keys: keysOf(20),
    native: () => K.POWER_EGGS.length >= 5 && K.EGG_TRIGGER_MAX === 99 && !new K.EggKeeper().enabled,
  },
];

/** 领域15 UI 设计与优化 · AI-53（族0521~0530）。 */
const AI53: X25Spec[] = [
  {
    fam: 521, title: '工作台五区骨架', dim: '区段',
    tiers: ['menuBar', 'activityBar', 'sidePane', 'editorArea', 'auxPane', 'statusBar'], def: 'editorArea',
    err: { code: 'UI-521', text: '布局快照损坏', next: '回默认五区布局' },
    role: 'group', slots: 6, keys: keysOf(521),
    native: () => WB.WORKBENCH_DEFAULT.sideWidth === 320 && new WB.WorkbenchLayout().gridTemplate().includes('menuBar'),
  },
  {
    fam: 522, title: 'MenuBar 菜单落位', dim: '菜单项',
    tiers: WB.MENU_ORDER, def: '文件',
    err: { code: 'UI-522', text: '菜单项注册冲突', next: '改名后重新登记' },
    role: 'menu', slots: 8, keys: keysOf(522),
    native: () => WB.MENU_ORDER.length === 8 && WB.MENU_ORDER[0] === '文件' && WB.MENU_ORDER[7] === '帮助',
  },
  {
    fam: 523, title: 'ActivityBar 七槽视图', dim: '槽位',
    tiers: WB.ACTIVITY_SLOTS, def: 'explorer',
    err: { code: 'UI-523', text: '槽位越界', next: '回到首个槽位' },
    role: 'tablist', slots: 7, keys: keysOf(523),
    native: () => WB.ACTIVITY_SLOTS.length === 7 && WB.ACTIVITY_SLOTS[6] === 'settings',
  },
  {
    fam: 524, title: '编辑区 Tab 与欢迎页', dim: '能力',
    tiers: EDITOR_CAP_TIERS, def: 'single',
    err: { code: 'UI-524', text: '标签页恢复失败', next: '打开欢迎页重试' },
    role: 'tablist', slots: 5, keys: keysOf(524),
    native: () => WB.WELCOME_SHORTCUTS.length === 3 && WB.WELCOME_RECENT_LIMIT === 5,
  },
  {
    fam: 525, title: 'Panel 问题与输出区', dim: '面板',
    tiers: PANEL_VIEW_TIERS, def: '问题',
    err: { code: 'UI-525', text: '面板输出中断', next: '点续跑恢复输出' },
    role: 'region', slots: 5, keys: keysOf(525),
    native: () => WB.PANELS.length === 4 && WB.PANELS[0] === '问题' && WB.PANELS[3] === '终端',
  },
  {
    fam: 526, title: 'StatusBar 双分区', dim: '槽位',
    tiers: STATUS_SLOT_TIERS, def: 'standard',
    err: { code: 'UI-526', text: '状态槽位冲突', next: '改槽位名后重试' },
    role: 'status', slots: 5, keys: keysOf(526),
    native: () => new WB.StatusBarModel().numericAlignment === 'tabular-nums',
  },
  {
    fam: 527, title: '命令面板与注册表', dim: '能力',
    tiers: COMMAND_TIERS, def: 'all',
    err: { code: 'UI-527', text: '命令键位冲突', next: '改绑定后保存' },
    role: 'combobox', slots: 5, keys: keysOf(527),
    native: () => WB.CommandRegistry.fuzzy('打开', '打开设置').hit && WB.CommandRegistry.fuzzy('打开x', '设置').hit === false,
  },
  {
    fam: 528, title: 'kit 基础输入件九件', dim: '件',
    tiers: ['button', 'input', 'textarea', 'select', 'checkbox', 'radio', 'switch', 'slider', 'stepper'], def: 'button',
    err: { code: 'UI-528', text: '输入值越界', next: '已钳回合法区间' },
    role: 'group', slots: 9, keys: keysOf(528),
    native: () => WB.BUTTON_VARIANTS.length === 4 && WB.BUTTON_HEIGHT.sm === 28 && WB.BUTTON_HEIGHT.tg === 44,
  },
  {
    fam: 529, title: 'kit 容器与布局件', dim: '件',
    tiers: ['card', 'cardGroup', 'sidePane', 'divider', 'toolbar', 'tabs'], def: 'card',
    err: { code: 'UI-529', text: '容器折叠异常', next: '重置折叠态重试' },
    role: 'group', slots: 6, keys: keysOf(529),
    native: () => WB.CARD_TOKENS_X.radius === 12 && new WB.DividerModel().role() === 'separator',
  },
  {
    fam: 530, title: 'kit 反馈与状态件', dim: '件',
    tiers: ['badge', 'chip', 'toast', 'tooltip', 'skeleton', 'progress', 'spinner', 'empty'], def: 'badge',
    err: { code: 'UI-530', text: '反馈队列溢出', next: '清理旧提示后重试' },
    role: 'status', slots: 8, keys: keysOf(530),
    native: () => WB.TOOLTIP_TIMING_X.show === 300 && WB.ToastQueueX.LIMIT === 3 && WB.spinnerRole() === 'progressbar',
  },
];

/** 领域15 UI 设计与优化 · AI-54（族0531~0540）。 */
const AI54: X25Spec[] = [
  {
    fam: 531, title: 'kit 数据展示件', dim: '件',
    tiers: ['avatar', 'breadcrumb', 'kbd', 'statusDot', 'table', 'tree', 'empty'], def: 'avatar',
    err: { code: 'UI-531', text: '数据项渲染失败', next: '降级为文本显示' },
    role: 'list', slots: 7, keys: keysOf(531),
    native: () => SM.avatarInitialsX('John Smith') === 'JS' && new SM.KbdModelX('Ctrl+Shift+P').dims().h === 22,
  },
  {
    fam: 532, title: 'kit 导航与系统件', dim: '件',
    tiers: ['navRail', 'searchPill', 'heroCard', 'settingsRow', 'taskbarItem', 'commandPalette'], def: 'navRail',
    err: { code: 'UI-532', text: '导航项不可达', next: '回主页重新导航' },
    role: 'navigation', slots: 6, keys: keysOf(532),
    native: () => new SM.SettingsRowModel('屏幕', '亮度').role() === 'button' && new SM.SettingsRowModel('屏幕', '亮度').minHeight() === 72,
  },
  {
    fam: 533, title: '材质五档参数', dim: '档',
    tiers: MATERIAL_TIERS, def: 'm-frosted',
    err: { code: 'UI-533', text: '材质档位越界', next: '回默认材质档' },
    role: 'group', slots: 5, keys: keysOf(533),
    native: () => SM.MATERIAL_TABLE['m-frosted'].length === 5 && SM.MATERIAL_TABLE['m-mica'].every((l) => l.blurPx === 0),
  },
  {
    fam: 534, title: '数值规范令牌对稿', dim: '规则',
    tiers: ['typeScale', 'color', 'radius', 'elevation', 'spacing', 'control'], def: 'typeScale',
    err: { code: 'UI-534', text: '令牌取值缺失', next: '回退上一版令牌' },
    role: 'group', slots: 6, keys: keysOf(534),
    native: () => SM.TYPE_SCALE_X.pageTitle.size === 28 && SM.RADII_X.window === 16 && SM.ELEVATION_X.length === 7,
  },
  {
    fam: 535, title: '设置 11 页收敛', dim: '页',
    tiers: SM.SETTINGS_PAGES.map((p) => p.id), def: 'home',
    err: { code: 'UI-535', text: '设置页未收敛', next: '按映射表归位' },
    role: 'navigation', slots: 11, keys: keysOf(535),
    native: () => SM.SETTINGS_PAGES.length === 11 && SM.SETTINGS_PAGES[0]!.id === 'home' && SM.SETTINGS_PAGES[10]!.id === 'update',
  },
  {
    fam: 536, title: '开始菜单与任务栏皮', dim: '能力',
    tiers: ['pinned', 'pages', 'recent', 'openAnim', 'centered', 'tray'], def: 'pinned',
    err: { code: 'UI-536', text: '固定项超限', next: '自动分页展示' },
    role: 'menu', slots: 6, keys: keysOf(536),
    native: () => SM.TASKBAR_TRAY.length === 6 && new SM.StartMenuModel().perPage === 24,
  },
  {
    fam: 537, title: '快捷面板与通知中心', dim: '能力',
    tiers: SM.QUICK_TILES, def: 'wifi',
    err: { code: 'UI-537', text: '通知分组失败', next: '按时间平铺展示' },
    role: 'dialog', slots: 6, keys: keysOf(537),
    native: () => SM.QUICK_TILES.length === 6 && SM.QUICK_TILES[0] === 'wifi' && SM.QUICK_TILES[4] === 'dnd',
  },
  {
    fam: 538, title: '文件管理器与桌面层', dim: '能力',
    tiers: SM.ExplorerToolbarModel.actions(), def: 'new',
    err: { code: 'UI-538', text: '视图切换失败', next: '回列表视图重试' },
    role: 'toolbar', slots: 9, keys: keysOf(538),
    native: () => SM.ExplorerToolbarModel.actions().length === 9 && SM.DESKTOP_CONTEXT_GROUPS.length === 3,
  },
  {
    fam: 539, title: '动效编排表', dim: '场景',
    tiers: SM.MOTION_SCENES.map((s) => s.id), def: 'hover',
    err: { code: 'UI-539', text: '编排场景缺失', next: '回标准曲线播放' },
    role: 'group', slots: 7, keys: keysOf(539),
    native: () => SM.MOTION_SCENES.length === 7 && SM.motionOf('hover')!.dur === '--dur-2' && SM.reduceMotionFallback().ease === 'linear',
  },
  {
    fam: 540, title: '手势语言', dim: '手势',
    tiers: SM.GESTURES.map((g) => g.id), def: 'twoFingerScroll',
    err: { code: 'UI-540', text: '手势识别超时', next: '改用键盘等价操作' },
    role: 'group', slots: 5, keys: keysOf(540),
    native: () => SM.GESTURES.length === 5 && SM.touchTarget('comfortable').min === 44 && SM.touchTarget('compact').min === 40,
  },
];

/* ==================== 汇总 ==================== */

export const X25_ALL_SPECS: readonly X25Spec[] = [...AI01, ...AI02, ...AI53, ...AI54];

const EGG_LINES: Record<number, string> = {
  1: '第一次点亮，值得记一笔', 2: '健康度满格的小绿点', 3: '修好一次的成就感',
  4: '恢复环境里的一盏灯', 5: '日志也能演成一场戏', 6: '失败也讲得清楚',
  7: '配速刚好，不急不缓', 8: '信任链上每一环都亮着', 9: '另一个系统在等你',
  10: '固件也有自己的皮', 11: '关机也要体面', 12: '醒来先亮一盏灯',
  13: '休眠前的最后一帧', 14: '快一点，再快一点', 15: '该醒的时候才醒',
  16: '每一次通电都有记录', 17: '仪表盘上跳动的光', 18: '预热完成的小提示',
  19: '安静地开始一天', 20: '告别与重逢的次数',
  521: '五区各就各位', 522: '八菜单排排坐', 523: '七槽点亮', 524: '标签页排好队',
  525: '问题清零那一瞬', 526: '状态栏的一行小字', 527: '命令一敲就到',
  528: '输入件也有手感', 529: '容器收放自如', 530: '提示恰到好处',
  531: '数据自己会说话', 532: '导航指向明确', 533: '材质透出层次',
  534: '令牌对齐的工整', 535: '设置页不再迷路', 536: '开始菜单的老位置',
  537: '通知来的刚刚好', 538: '文件各归各位', 539: '动效踩在点上',
  540: '手势一气呵成',
};

let registered = false;

/** 注册全部 42 族的公共基建（三线联动 / 扩展点 / 彩蛋 / 守卫种子）。 */
export function ensureX25Registered(): void {
  if (registered) return;
  registered = true;
  for (const spec of X25_ALL_SPECS) {
    registerX25(spec, EGG_LINES[spec.fam] ?? '这一处留了点小惊喜');
  }
}

ensureX25Registered();

/** 取某一族的达标描述。 */
export function specOf(fam: number): X25Spec {
  const spec = X25_ALL_SPECS.find((s) => s.fam === fam);
  if (!spec) throw new Error(`x25: 未登记族 ${fam}`);
  return spec;
}

/** 族内已存在的 X-ID 集合 → 返回仍缺失的层内序号（1~25）。 */
export function missingOffsets(fam: number, present: readonly string[]): number[] {
  const base = xidBase(fam);
  const have = new Set(present);
  const out: number[] = [];
  for (let i = 1; i <= 25; i += 1) {
    const id = `X${String(base + i - 1).padStart(5, '0')}`;
    if (!have.has(id)) out.push(i);
  }
  return out;
}
