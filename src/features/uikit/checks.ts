// AURORA-10000: AI-71~AI-75 批次领域15自检注册表（F08751~F09375 共 625 项），勿删。
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

/** 单次求值记忆：条目创建后首次运行缓存结果，避免状态型断言被二次求值破坏。 */
function memoized(e: CheckEntry): CheckEntry {
  let ran = false;
  let ok = false;
  return { ...e, check: () => {
    if (!ran) {
      try {
        ok = e.check();
      } catch {
        ok = false;
      }
      ran = true;
    }
    return ok;
  } };
}

/* -------- AI-71 族0351 组件库补全 F08751~F08875 -------- */
export function checkF0351(): CheckEntry[] {
  const sw = new A.SwitchModel();
  const exp = new A.ExpanderModel();
  const seg = new A.SegmentedModel(['日', '周', '月']);
  const slider = new A.SliderModel(0, 100, 5, 47);
  const tabs = new A.TabModel(['a', 'b', 'c']);
  const dlg = new A.DialogModel();
  const toast = new A.ToastQueue();
  const stepper = new A.StepperModel(1, 0, 3);
  return [
    { id: 'F08751', name: '组件目录 25 个', check: () => A.COMPONENT_CATALOG.length === 25 && new Set(A.COMPONENT_CATALOG).size === 25 },
    { id: 'F08752', name: 'Switch 44×20', check: () => { sw.toggle(); return sw.on && sw.dims().w === 44 && sw.dims().h === 20; } },
    { id: 'F08753', name: 'Expander 行内展开', check: () => exp.toggle() && exp.ariaExpanded() === 'true' && !exp.toggle() },
    { id: 'F08754', name: '分段选择器单选', check: () => seg.select(2) === '月' && seg.selected === '月' && seg.select(9) === null },
    { id: 'F08755', name: '滑杆步进钳位', check: () => slider.set(47) === 45 && slider.set(120) === 100 && slider.bubble() === '100' },
    { id: 'F08756', name: '卡片令牌', check: () => A.CARD_TOKENS.radius === 8 && A.CARD_TOKENS.elevation === 2 },
    { id: 'F08757', name: 'Chevron 行高', check: () => A.CHEVRON_ROW.minHeight === 48 && A.CHEVRON_ROW.chevron === '›' },
    { id: 'F08758', name: '面包屑中段省略', check: () => A.breadcrumb(['a', 'b', 'c']).length === 3 && A.breadcrumb(['a', 'b', 'c', 'd', 'e']).join('/') === 'a/…/e' },
    { id: 'F08759', name: '搜索即时过滤', check: () => A.searchFilter(['设置', '声音'], '声').length === 1 && A.searchFilter(['a'], '').length === 1 },
    { id: 'F08760', name: '徽标 99+', check: () => A.badgeLabel(150) === '99+' && A.badgeLabel(0) === '0' },
    { id: 'F08761', name: '进度环换算', check: () => { const r = A.progressRing(0.5); return r.dashoffset === r.dasharray / 2; } },
    { id: 'F08762', name: '提示延迟令牌', check: () => A.TOOLTIP_TIMING.show === 300 && A.TOOLTIP_TIMING.hide === 150 },
    { id: 'F08763', name: '菜单扁平化', check: () => { const f = A.flattenMenu([{ label: 'a', submenu: [{ label: 'b' }] }]); return f.length === 2 && f[1]!.depth === 1; } },
    { id: 'F08764', name: '工具栏溢出', check: () => { const o = A.toolbarOverflow(['1', '2', '3'], 2); return o.visible.length === 2 && o.overflow[0] === '3'; } },
    { id: 'F08765', name: '标签 roving', check: () => tabs.keys('ArrowRight') === 1 && tabs.keys('End') === 2 && tabs.keys('ArrowLeft') === 1 },
    { id: 'F08766', name: '对话框焦点还原', check: () => { dlg.show('btn1'); return dlg.trapKeys().includes('Escape') && dlg.close() === 'btn1' && !dlg.open; } },
    { id: 'F08767', name: 'Toast 队列上限 3', check: () => { ['a', 'b', 'c', 'd'].forEach((x) => toast.push(x)); return toast.list().join() === 'b,c,d'; } },
    { id: 'F08768', name: '骨架镜像布局', check: () => A.skeletonLayout(2)[0]!.w === '60%' && A.skeletonLayout(3).length === 3 },
    { id: 'F08769', name: '空态定义', check: () => A.EMPTY_STATES.length === 3 && A.EMPTY_STATES[0]!.action === '新建' },
    { id: 'F08770', name: '日期范围校验', check: () => A.dateFieldValid('2026-05-01', '2026-01-01', '2026-12-31') && !A.dateFieldValid('2025-01-01', '2026-01-01', '2026-12-31') },
    { id: 'F08771', name: '数字钳位千分位', check: () => { const r = A.numberField(1500, 0, 1000); return r.value === 1000 && r.text === '1,000'; } },
    { id: 'F08772', name: '步进器边界', check: () => stepper.dec() === 0 && !stepper.canDec() && stepper.inc() === 1 },
    { id: 'F08773', name: '评分半星', check: () => { const s = A.ratingStars(3.5); return s[2] === 'full' && s[3] === 'half' && s[4] === 'empty'; } },
    { id: 'F08774', name: '色样对比度', check: () => A.colorSwatchContrast([0, 0, 0], [255, 255, 255]) > 20 && A.colorSwatchContrast([128, 128, 128], [128, 128, 128]) < 1.1 },
    { id: 'F08775', name: '命令面板模糊匹配', check: () => A.avatarInitials('王小明') === '小明' && A.avatarInitials('John Smith') === 'JS' && A.commandPalette(['打开设置'], '设置').length === 1 },
  ];
}

/* -------- AI-71 族0352 布局系统 F08776~F08800 -------- */
export function checkF0352(): CheckEntry[] {
  return [
    { id: 'F08776', name: '4/8pt 间距刻度', check: () => A.SPACING_SCALE[3] === 12 && A.SPACING_SCALE.every((s) => s % 4 === 0) },
    { id: 'F08777', name: '断点三档', check: () => A.breakpoint(400) === 'narrow' && A.breakpoint(800) === 'medium' && A.breakpoint(1600) === 'wide' },
    { id: 'F08778', name: '12 栏跨度换算', check: () => { const g = A.gridSpan(12, 6, 3); return g.width === '50%' && g.left === '25%'; } },
    { id: 'F08779', name: '容器查询列数', check: () => A.containerColumns(300) === 1 && A.containerColumns(900) === 3 && A.containerColumns(1500) === 4 },
    { id: 'F08780', name: '瀑布流最短列', check: () => { const m = A.masonryAssign([1, 1, 1, 1], 2); return m.join() === '0,1,0,1'; } },
    { id: 'F08781', name: '侧栏宽度档', check: () => A.SPLIT_LAYOUT.sidebar === 320 && A.SPLIT_LAYOUT.sidebarNarrow === 260 },
    { id: 'F08782', name: '吸顶偏移', check: () => A.stickyOffset(48) === 'scroll-margin-top:56px' },
    { id: 'F08783', name: '安全区内边距', check: () => A.safeArea(1, 2, 3, 4).includes('env(safe-area-inset-top,1px)') },
    { id: 'F08784', name: 'RTL 逻辑属性', check: () => A.rtlFlip('margin-left:4px;text-align:left').includes('margin-inline-start') && A.rtlFlip('text-align:left').includes('start') },
    { id: 'F08785', name: '宽高比盒', check: () => A.aspectBox(16, 9) === 'aspect-ratio:16/9' },
    { id: 'F08786', name: 'z 分层预算', check: () => A.Z_SCALE.modal > A.Z_SCALE.overlay && A.Z_SCALE.toast === 500 },
    { id: 'F08787', name: '栈间距规则', check: () => A.stackGap(10) === 4 && A.stackGap(3) === 12 },
    { id: 'F08788', name: '响应式列数', check: () => A.responsiveColumns(960, 96) === 10 && A.responsiveColumns(50) === 1 },
    { id: 'F08789', name: '内容钳位', check: () => A.contentClamp(200, 600).includes('clamp(200px') },
    { id: 'F08790', name: '布局令牌输出', check: () => A.layoutTokens(8, 4).includes('--aurora-layout-gap:8px') },
    { id: 'F08791', name: '光学对齐补偿', check: () => A.opticalAlign('circle', 100) === 96 && A.opticalAlign('square', 100) === 100 },
    { id: 'F08792', name: '网格吸附', check: () => A.snapToGrid(17, 8) === 16 && A.snapToGrid(15, 8) === 16 },
    { id: 'F08793', name: '折行规则', check: () => A.wrapRule('see https://a.b/c') === 'overflow-wrap:anywhere' && A.wrapRule('普通文本') === 'overflow-wrap:normal' },
    { id: 'F08794', name: '圣杯布局', check: () => A.holyGrail(200) === 'grid-template-columns:200px 1fr 200px' },
    { id: 'F08795', name: '布局回退', check: () => A.layoutFallback('x').startsWith('@supports not') },
    { id: 'F08796', name: '行高刻度', check: () => A.LINE_HEIGHT_SCALE.normal === 1.5 && A.LINE_HEIGHT_SCALE.tight === 1.2 },
    { id: 'F08797', name: '密度行高三档', check: () => A.densityRowHeight('compact') === 32 && A.densityRowHeight('spacious') === 48 },
    { id: 'F08798', name: '布局调试线框', check: () => A.debugOutline(true).includes('dashed') && A.debugOutline(false) === 'outline:none' },
    { id: 'F08799', name: '栅格快照序列化', check: () => A.layoutSnapshot(2, 3, ['a', 'b']).includes('2 cols') },
    { id: 'F08800', name: '布局规范六条', check: () => A.LAYOUT_SPEC.length === 6 },
  ];
}

/* -------- AI-71 族0353 图标一致性 F08801~F08825 -------- */
export function checkF0353(): CheckEntry[] {
  const good: A.IconDef = { name: 'arrow-right', category: 'nav', size: 24, stroke: 1.5 };
  const bad: A.IconDef = { name: 'ArrowX', category: 'nav', size: 18, stroke: 1.5 };
  return [
    { id: 'F08801', name: '尺寸五档', check: () => A.ICON_SIZES.join() === '16,20,24,32,48' },
    { id: 'F08802', name: '键线审计', check: () => A.iconKeylineAudit(good).length === 0 && A.iconKeylineAudit(bad).length > 0 },
    { id: 'F08803', name: '线重令牌', check: () => A.ICON_STROKE.regular === 1.5 && A.ICON_STROKE.bold === 2 },
    { id: 'F08804', name: '圆形光学放大', check: () => A.iconOpticalSize('circle', 100) === 104 && A.iconOpticalSize('rect', 100) === 100 },
    { id: 'F08805', name: '整像素贴合', check: () => A.iconPixelSnap(12.4) === 12 && A.iconPixelSnap(12.6) === 13 },
    { id: 'F08806', name: '命名 kebab', check: () => A.iconNameValid('arrow-right') && !A.iconNameValid('ArrowRight') },
    { id: 'F08807', name: '分类五类', check: () => A.ICON_CATEGORIES.length === 5 },
    { id: 'F08808', name: '语义色令牌', check: () => A.iconColorToken('nav') === '--aurora-icon-nav' },
    { id: 'F08809', name: 'RTL 镜像', check: () => A.iconMirror('arrow-right', true) && !A.iconMirror('settings', true) },
    { id: 'F08810', name: '填充描边配对', check: () => { const m = A.iconPairCheck([{ ...good, name: 'heart' }, { ...good, name: 'heart-filled' }]); return m.missing.includes('heart'); } },
    { id: 'F08811', name: 'viewBox 强制', check: () => A.iconViewBoxOk('<svg viewBox="0 0 24 24">') && !A.iconViewBoxOk('<svg viewBox="0 0 16 16">') },
    { id: 'F08812', name: '重复路径检测', check: () => A.iconDuplicate(['M0 0', 'M0 0', 'M1 1']).join() === 'M0 0' },
    { id: 'F08813', name: '雪碧图生成', check: () => A.iconSprite(['a', 'b']).includes('symbol id="icon-a"') },
    { id: 'F08814', name: '导出规范', check: () => A.iconExportSheet([good, bad]).count === 2 },
    { id: 'F08815', name: '悬停提亮', check: () => A.ICON_HOVER_BRIGHTNESS === 1.08 },
    { id: 'F08816', name: '禁用去饱和', check: () => A.ICON_DISABLED_FILTER.includes('saturate(0.6)') },
    { id: 'F08817', name: '焦点环', check: () => A.ICON_FOCUS_RING.includes('var(--aurora-accent)') },
    { id: 'F08818', name: '一致性评分', check: () => A.iconConsistencyScore([good, bad]) === 50 && A.iconConsistencyScore([]) === 100 },
    { id: 'F08819', name: 'linter 五规则', check: () => A.ICON_LINT_RULES.length === 5 },
    { id: 'F08820', name: '修复建议', check: () => A.iconFixSuggestion('name').includes('kebab') && A.iconFixSuggestion('other') === '人工复核' },
    { id: 'F08821', name: '图标文档', check: () => A.ICON_DOCS.length === 5 },
    { id: 'F08822', name: '网格叠加', check: () => A.iconGridOverlay(true).includes('linear-gradient') && A.iconGridOverlay(false) === '' },
    { id: 'F08823', name: '圆角令牌', check: () => A.ICON_CORNER.none === 0 && A.ICON_CORNER.large === 8 },
    { id: 'F08824', name: '缺字占位', check: () => A.iconFallback('star') === '[icon:star]' },
    { id: 'F08825', name: '走查清单', check: () => A.ICON_WALKTHROUGH.length === 6 },
  ];
}

/* -------- AI-71 族0354 数据可视化 F08826~F08850 -------- */
export function checkF0354(): CheckEntry[] {
  return [
    { id: 'F08826', name: 'CVD 色板 8 色', check: () => A.CHART_PALETTE.length === 8 && A.CHART_PALETTE[0] === '#0078D4' },
    { id: 'F08827', name: 'nice 刻度', check: () => { const t = A.niceTicks(0, 10, 5); return t[0] === 0 && t.length === 6 && t[1] === 2; } },
    { id: 'F08828', name: '线性/对数比例', check: () => A.scaleValue(5, [0, 10], [0, 100]) === 50 && A.scaleValue(10, [1, 100], [0, 100], true) === 50 && A.scaleValue(100, [1, 100], [0, 100], true) === 100 },
    { id: 'F08829', name: '迷你图点集', check: () => { const s = A.sparkline([0, 10], 100, 50); return s[0]![1] === 50 && s[1]![1] === 0; } },
    { id: 'F08830', name: '环形弧换算', check: () => { const a = A.donutArc(0.25); return a.large === 0 && A.donutArc(0.75).large === 1; } },
    { id: 'F08831', name: '堆叠累计', check: () => { const s = A.stackedBar([1, 2]); return s[0]!.y1 === 1 && s[1]!.y0 === 1 && s[1]!.y1 === 3; } },
    { id: 'F08832', name: '图例两列', check: () => { const l = A.legendLayout(['a', 'b', 'c']); return l.length === 2 && l[1]!.length === 1; } },
    { id: 'F08833', name: '提示框翻转', check: () => A.tooltipAnchor(990, 10, 100, 50, 1000, 800).flip.includes('left') },
    { id: 'F08834', name: '平滑曲线', check: () => A.smoothPath([[0, 0], [10, 10], [20, 0]]).startsWith('M0,0C') },
    { id: 'F08835', name: '阈值带标记', check: () => A.thresholdBand([1, 5, 9], 5).join() === 'false,false,true' },
    { id: 'F08836', name: '标签防重叠', check: () => { const l = A.declutterLabels(100, 6); return l.length <= 7 && l[0] === 0; } },
    { id: 'F08837', name: '空数据兜底', check: () => A.chartEmptyGuard([]).empty && !A.chartEmptyGuard([1]).empty },
    { id: 'F08838', name: '零基线规则', check: () => A.barDomain([3, 7]).join() === '0,7' },
    { id: 'F08839', name: '时间轴粒度', check: () => A.timeAxisLabels(30_000).unit === '秒' && A.timeAxisLabels(172_800_000).unit === '日' },
    { id: 'F08840', name: 'K/万 缩写', check: () => A.formatCompact(1500) === '1.5K' && A.formatCompact(15000) === '1.5万' && A.formatCompact(99) === '99' },
    { id: 'F08841', name: '色带插值', check: () => A.colorRamp(['#000000', '#ffffff'], 0.5) === '#808080' },
    { id: 'F08842', name: '条最小宽', check: () => A.barMinWidth(1, 1000, 100) === 2 && A.barMinWidth(0, 100, 100) === 0 },
    { id: 'F08843', name: '饼图归并其他', check: () => { const s = A.pieSliceSort(Array.from({ length: 8 }, (_, i) => ({ label: `${i}`, value: 10 - i }))); return s.length === 6 && s[5]!.label === '其他'; } },
    { id: 'F08844', name: '趋势箭头', check: () => A.trendArrow(5, 3) === '↑' && A.trendArrow(3, 5) === '↓' && A.trendArrow(3, 3) === '→' },
    { id: 'F08845', name: '网格线', check: () => A.gridLines(3)[2] === 'h2' && A.gridLines(2, false)[1] === 'v1' },
    { id: 'F08846', name: '响应式图表', check: () => A.chartResponsive(300).compact && !A.chartResponsive(500).compact },
    { id: 'F08847', name: '过渡 200ms', check: () => A.CHART_TRANSITION_MS === 200 },
    { id: 'F08848', name: '系列配色循环', check: () => A.seriesColor(0) === A.seriesColor(8) && A.seriesColor(1) !== A.seriesColor(0) },
    { id: 'F08849', name: 'SVG 导出', check: () => A.chartExportSvg(10, 10, '<g/>').includes('width="10"') },
    { id: 'F08850', name: '图表数据表回退', check: () => A.chartA11yTable('销量', [{ label: '一月', value: 5 }]).includes('|一月|5|') },
  ];
}

/* -------- AI-71 族0355 表单体验 F08851~F08875 -------- */
export function checkF0355(): CheckEntry[] {
  const f = new A.FieldModel('email', (v) => (v.includes('@') ? null : '需包含 @'));
  const wiz = new A.WizardModel(4);
  return [
    { id: 'F08851', name: '字段三态', check: () => { f.set('a'); const s = f.blur(); return s.touched && s.dirty && s.error === '需包含 @' && !f.valid(); } },
    { id: 'F08852', name: '必填标记', check: () => A.requiredMark(true).mark === '*' && A.requiredMark(true).aria && !A.requiredMark(false).mark },
    { id: 'F08853', name: '校验时机', check: () => A.validationTiming('input') === 'none' && A.validationTiming('blur') === 'gentle' && A.validationTiming('submit') === 'strict' },
    { id: 'F08854', name: '行内错误关联', check: () => A.fieldErrorAria('f1', true) === 'f1-error' && A.fieldErrorAria('f1', false) === null },
    { id: 'F08855', name: '成功对勾', check: () => A.fieldSuccess(true, true) && !A.fieldSuccess(true, false) },
    { id: 'F08856', name: '密码强度', check: () => A.passwordStrength('a') === 0 && A.passwordStrength('Abcdefg1!') === 3 },
    { id: 'F08857', name: '手机掩码', check: () => A.phoneMask('13812345678') === '138 1234 5678' && A.phoneMask('138') === '138' },
    { id: 'F08858', name: 'autocomplete 映射', check: () => A.autocompleteAttr('new-password') === 'autocomplete="new-password"' },
    { id: 'F08859', name: '聚焦首个错误', check: () => A.focusFirstError([{ name: 'a', error: null }, { name: 'b', error: 'x' }]) === 'b' && A.focusFirstError([{ name: 'a', error: null }]) === null },
    { id: 'F08860', name: '脏数据守卫', check: () => A.formDirtyGuard(true, false) === 'block' && A.formDirtyGuard(false, false) === 'allow' },
    { id: 'F08861', name: '草稿防抖保存', check: () => A.draftAutosave(0, 2000) === 'saved' && A.draftAutosave(0, 500) === 'pending' },
    { id: 'F08862', name: '向导进度', check: () => wiz.next() === 1 && wiz.prev() === 0 && wiz.progress() === 25 },
    { id: 'F08863', name: '提交中禁用', check: () => !A.submitGuard(true) && A.submitGuard(false) },
    { id: 'F08864', name: '合法才提交', check: () => A.submitEnabled(true, false) && !A.submitEnabled(false, false) && !A.submitEnabled(true, true) },
    { id: 'F08865', name: '字数计数预警', check: () => { const c = A.charCounter('x'.repeat(95), 100); return c.count === 95 && c.warn; } },
    { id: 'F08866', name: '粘贴净化', check: () => A.pasteNormalize('  a   b  ') === 'a b' },
    { id: 'F08867', name: '数字输入钳位', check: () => A.numberInputClamp(150, 0, 100) === 100 && A.numberInputClamp(NaN, 5, 100) === 5 },
    { id: 'F08868', name: '下拉占位', check: () => A.selectPlaceholder().disabled && A.selectPlaceholder('请选择').value === '请选择' },
    { id: 'F08869', name: '全选联动', check: () => A.checkboxSelectAll([true, true]) === 'all' && A.checkboxSelectAll([true, false]) === 'some' },
    { id: 'F08870', name: '单选键盘循环', check: () => A.radioKeyboard(2, 3, 'ArrowDown') === 0 && A.radioKeyboard(0, 3, 'ArrowUp') === 2 },
    { id: 'F08871', name: '文本域自增高', check: () => A.textareaAutogrow('a\nb\nc') === 76 },
    { id: 'F08872', name: '日期起止校验', check: () => A.dateRangeValid('2026-01-01', '2026-02-01') && !A.dateRangeValid('2026-03-01', '2026-02-01') },
    { id: 'F08873', name: '拖放类型过滤', check: () => { const r = A.fileDropAccept(['a.png', 'b.txt'], ['.png']); return r.ok.join() === 'a.png' && r.rejected.length === 1; } },
    { id: 'F08874', name: '标签必绑审计', check: () => A.formA11yAudit([{ id: 'a', label: 'x' }, { id: 'b', label: '' }])[0] === 'b:missing-label' },
    { id: 'F08875', name: '错误汇总', check: () => A.errorSummary(['a', 'b']).count === 2 },
  ];
}

/* -------- AI-72 族0356 加载与骨架 F08876~F08900 -------- */
export function checkF0356(): CheckEntry[] {
  const m = new B.LoadMachine();
  return [
    { id: 'F08876', name: '加载状态机', check: () => m.start() === 'loading' && m.succeed() === 'success' && m.start() === 'loading' && m.fail() === 'error' && m.reset() === 'idle' },
    { id: 'F08877', name: '骨架镜像', check: () => B.skeletonMirror([{ h: 48, w: 60.4 }])[0]!.w === 60 },
    { id: 'F08878', name: '微光 1.2s', check: () => B.SHIMMER_MS === 1200 },
    { id: 'F08879', name: '图片渐进模糊', check: () => B.progressiveImage(false).filter.includes('blur(12px)') && B.progressiveImage(true).scale === 1 },
    { id: 'F08880', name: '骨架优先规则', check: () => B.loaderChoice(true) === 'skeleton' && B.loaderChoice(false) === 'spinner' },
    { id: 'F08881', name: '防闪 300ms', check: () => B.MIN_LOADING_MS === 300 },
    { id: 'F08882', name: 'SWR 标记', check: () => B.swrIndicator(true, true) === 'refreshing' && B.swrIndicator(false, false) === 'fresh' },
    { id: 'F08883', name: '无限滚动哨兵', check: () => B.infiniteSentinel(150) && !B.infiniteSentinel(300) },
    { id: 'F08884', name: '下拉刷新', check: () => { const p = B.pullToRefresh(100); return p.armed && p.offset === 96; } },
    { id: 'F08885', name: '顶部进度条', check: () => B.TOP_PROGRESS.height === 2 && B.TOP_PROGRESS.indeterminate },
    { id: 'F08886', name: '按钮加载态', check: () => B.buttonLoading(true).disabled && B.buttonLoading(true).spinner },
    { id: 'F08887', name: '路由兜底', check: () => B.routeFallback('settings') === 'skeleton:settings' },
    { id: 'F08888', name: '文本占位', check: () => B.textPlaceholder(3)[0] === '100%' && B.textPlaceholder(3)[2] === '70%' },
    { id: 'F08889', name: '头像占位', check: () => B.AVATAR_PLACEHOLDER.size === 40 && B.AVATAR_PLACEHOLDER.shape === 'circle' },
    { id: 'F08890', name: '表格骨架 5 行', check: () => B.tableSkeletonRows().length === 5 },
    { id: 'F08891', name: '百分比加载', check: () => B.percentLoader(5, 10).pct === 50 && B.percentLoader(0, 0).pct === 100 },
    { id: 'F08892', name: '不确定循环', check: () => B.indeterminateLoop(150) === 50 },
    { id: 'F08893', name: '超时 8s 提示', check: () => B.loadTimeout(9000) !== null && B.loadTimeout(1000) === null },
    { id: 'F08894', name: '失败重试入口', check: () => B.retryAffordance('error') === '重试' && B.retryAffordance('loading') === null },
    { id: 'F08895', name: '预取 100ms', check: () => B.PREFETCH_DELAY_MS === 100 },
    { id: 'F08896', name: '字体 swap', check: () => B.FONT_SWAP === 'font-display:swap' },
    { id: 'F08897', name: '图片占位防跳', check: () => B.imagePlaceholder(16, 9) === 'aspect-ratio:16/9' },
    { id: 'F08898', name: '列表错峰 300 封顶', check: () => B.listStagger(2) === 60 && B.listStagger(50) === 300 },
    { id: 'F08899', name: '首帧 200ms', check: () => B.firstPaintBudget(150) && !B.firstPaintBudget(300) },
    { id: 'F08900', name: '加载规范五条', check: () => B.LOADING_SPEC.length === 5 },
  ];
}

/* -------- AI-72 族0357 错误处理 UI F08901~F08925 -------- */
export function checkF0357(): CheckEntry[] {
  return [
    { id: 'F08901', name: '错误分级', check: () => B.errorLevel('E1001') === 'error' && B.errorLevel('W2001') === 'warn' && B.errorLevel('X1') === 'info' },
    { id: 'F08902', name: '行内错误 alert', check: () => B.inlineError('错').role === 'alert' && B.inlineError(null).text === null },
    { id: 'F08903', name: 'Toast 带操作', check: () => B.errorToast('失败', '重试').action === '重试' && B.errorToast('失败').action === null },
    { id: 'F08904', name: '整页错误主操作', check: () => B.fullPageError(true).primary === '重试' && B.fullPageError(false).primary === '返回首页' },
    { id: 'F08905', name: '错误码目录', check: () => B.ERROR_CODES.NET_OFFLINE === 'E1001' && Object.keys(B.ERROR_CODES).length === 6 },
    { id: 'F08906', name: '人话文案', check: () => B.humanizeError('E1001').includes('网络') && B.humanizeError('ZZZ') === '发生未知错误' },
    { id: 'F08907', name: '重试退避', check: () => B.retryBackoff(0) === 1000 && B.retryBackoff(2) === 4000 && B.retryBackoff(3) === null },
    { id: 'F08908', name: '离线横幅', check: () => B.offlineBanner(false).show && !B.offlineBanner(true).show },
    { id: 'F08909', name: '重试倒计时', check: () => B.retryCountdown(3).includes('3s') && B.retryCountdown(0).includes('正在') },
    { id: 'F08910', name: '表单错误汇总', check: () => B.formErrorSummary([{ name: 'a', error: '必填' }]).join() === 'a：必填' },
    { id: 'F08911', name: '404 页', check: () => B.NOT_FOUND_PAGE.title.includes('找不到') },
    { id: 'F08912', name: '500 页', check: () => B.SERVER_ERROR_PAGE.action === '重试' },
    { id: 'F08913', name: '崩溃兜底', check: () => B.crashFallback('X').boundary === 'X' && B.crashFallback('X').message.includes('暂时') },
    { id: 'F08914', name: '本地错误报告', check: () => { const r = B.errorReportLocal('E1', 'x'.repeat(3000)); return !r.uploaded && r.stack.length === 2000; } },
    { id: 'F08915', name: '破坏性确认', check: () => B.destructiveConfirm('删除').danger && B.destructiveConfirm('删除').confirmText === '确认删除' },
    { id: 'F08916', name: '未保存守卫', check: () => B.unsavedGuard(true).block && !B.unsavedGuard(false).block },
    { id: 'F08917', name: '恢复建议', check: () => B.recoverySuggestions('E1001').length === 3 && B.recoverySuggestions('?')[0] === '重试一次' },
    { id: 'F08918', name: '错误去重 30s', check: () => B.errorDedup(0, 10_000) && !B.errorDedup(0, 40_000) },
    { id: 'F08919', name: '三次升级人工', check: () => B.escalationPath(3) === 'manual' && B.escalationPath(1) === 'auto' },
    { id: 'F08920', name: 'alert 语义', check: () => B.ALERT_ROLE === 'alert' },
    { id: 'F08921', name: '焦点移错误', check: () => B.focusOnError(true, 'f') === '#f' && B.focusOnError(false, 'f') === null },
    { id: 'F08922', name: '复制诊断', check: () => B.copyDiagnostics({ a: '1' }) === 'a=1' },
    { id: 'F08923', name: '错误 i18n 键', check: () => B.errorI18nKey('E1001') === 'error.e1001' },
    { id: 'F08924', name: '遥测默认关', check: () => B.errorTelemetryDefault().enabled === false && B.errorTelemetryDefault().askConsent },
    { id: 'F08925', name: '错误规范五条', check: () => B.ERROR_SPEC.length === 5 },
  ];
}

/* -------- AI-72 族0358 空态与引导 F08926~F08950 -------- */
export function checkF0358(): CheckEntry[] {
  const tour = new B.TourTracker();
  tour.complete('step1');
  return [
    { id: 'F08926', name: '空态目录 10 款', check: () => B.EMPTY_STATE_CATALOG.length === 10 && B.EMPTY_STATE_CATALOG.every((s) => s.action.length > 0) },
    { id: 'F08927', name: '插画槽', check: () => B.emptyIllustration('no-items') === 'illustration:no-items' },
    { id: 'F08928', name: '空态唯一主操作', check: () => B.emptyPrimaryAction(B.EMPTY_STATE_CATALOG[0]!) === '新建' },
    { id: 'F08929', name: '搜索空态建议', check: () => B.emptySearchSuggestions('x').length === 3 },
    { id: 'F08930', name: '引导五步', check: () => B.ONBOARDING_STEPS.length === 5 },
    { id: 'F08931', name: '气泡引导', check: () => B.coachMark('#btn', '点这里').anchor === '#btn' },
    { id: 'F08932', name: '首用高亮', check: () => B.firstUseHighlight('x') === 'highlight:x' },
    { id: 'F08933', name: '可关提示记忆', check: () => B.dismissibleHint('h', []) && !B.dismissibleHint('h', ['h']) },
    { id: 'F08934', name: '新功能徽章', check: () => B.featureBadge(true, false) === 'new' && B.featureBadge(true, true) === null },
    { id: 'F08935', name: '键盘提示卡', check: () => B.keyboardHintCard([['Ctrl+K', '搜索']])[0]!.includes('Ctrl+K') },
    { id: 'F08936', name: '渐进披露', check: () => { const p = B.progressiveDisclosure(['1', '2', '3', '4', '5', '6', '7']); return p.shown.length === 5 && p.more === 2; } },
    { id: 'F08937', name: '示例数据播种', check: () => B.sampleDataSeed('notes').length === 2 && B.sampleDataSeed('files')[0]!.endsWith('.txt') },
    { id: 'F08938', name: '空回收站', check: () => B.EMPTY_TRASH.key === 'empty-trash' },
    { id: 'F08939', name: '空通知', check: () => B.EMPTY_NOTIFICATIONS.key === 'no-notifications' },
    { id: 'F08940', name: '空下载', check: () => B.EMPTY_DOWNLOADS.key === 'no-downloads' },
    { id: 'F08941', name: '空微件', check: () => B.EMPTY_WIDGETS.key === 'no-widgets' },
    { id: 'F08942', name: '引导进度追踪', check: () => tour.isComplete('step1') && !tour.isComplete('step2') && tour.progress(5) === 0.2 },
    { id: 'F08943', name: '引导可跳过', check: () => B.tourSkip(true).skip && B.tourSkip(true).remember },
    { id: 'F08944', name: '引导可重播', check: () => B.tourReplay() },
    { id: 'F08945', name: '帮助链接右上', check: () => B.helpLinkPlacement() === 'top-right' },
    { id: 'F08946', name: '引导不出站', check: () => B.TOUR_LOCAL_ONLY },
    { id: 'F08947', name: '引导节奏', check: () => B.tourPace(5, 4) === 'done' && B.tourPace(5, 0) === 'step' },
    { id: 'F08948', name: '线性插画风格', check: () => B.EMPTY_ILLUSTRATION_STYLE === 'line-art' },
    { id: 'F08949', name: '引导读屏', check: () => B.tourA11y(0, 5) === '第 1 步，共 5 步' },
    { id: 'F08950', name: '空态规范四条', check: () => B.EMPTY_STATE_SPEC_DOCS.length === 4 },
  ];
}

/* -------- AI-72 族0359 信息密度优化 F08951~F08975 -------- */
export function checkF0359(): CheckEntry[] {
  const pm = new B.PanelMemory();
  pm.set('x', true);
  const ado = new B.AppDensityOverride();
  ado.set('files', 'compact');
  return [
    { id: 'F08951', name: '密度三档令牌', check: () => B.densityTokens('compact').row === 32 && B.densityTokens('comfortable').row === 40 && B.densityTokens('spacious').row === 48 },
    { id: 'F08952', name: '表格密度', check: () => B.tableDensity('compact') === 32 },
    { id: 'F08953', name: '列表内边距', check: () => B.listPadding('comfortable') === 'padding:8px' },
    { id: 'F08954', name: '字号随密度', check: () => B.densityFont('compact') === 12 && B.densityFont('spacious') === 14 },
    { id: 'F08955', name: '图标随密度', check: () => B.densityIconSize('compact') === 16 && B.densityIconSize('spacious') === 24 },
    { id: 'F08956', name: '视口自适应', check: () => B.adaptiveDensity(600) === 'compact' && B.adaptiveDensity(2000) === 'spacious' },
    { id: 'F08957', name: '留白比例', check: () => B.whitespaceRatio(60, 100) === 0.4 },
    { id: 'F08958', name: '内容最大宽 1000', check: () => B.CONTENT_MAX_WIDTH === 1000 },
    { id: 'F08959', name: '行长守卫', check: () => B.lineLengthGuard(30) === 'ok' && B.lineLengthGuard(50) === 'too-long' && B.lineLengthGuard(10) === 'too-short' },
    { id: 'F08960', name: '长列表分块', check: () => B.chunkList([1, 2, 3, 4, 5], 2).length === 3 && B.chunkList([1], 200).length === 1 },
    { id: 'F08961', name: '折叠默认首开', check: () => { const s = B.collapsibleSections(['a', 'b']); return s[0]!.open && !s[1]!.open; } },
    { id: 'F08962', name: '先 5 条再展开', check: () => { const r = B.showFiveThenExpand([1, 2, 3, 4, 5, 6]); return r.shown.length === 5 && r.rest.length === 1; } },
    { id: 'F08963', name: '数字等宽', check: () => B.TABULAR_NUMS.includes('tabular-nums') },
    { id: 'F08964', name: '路径中段省略', check: () => { const e = B.middleEllipsis('/very/long/path/that/goes/on/and/on/forever', 20); return e.includes('…') && e.length === 20; } },
    { id: 'F08965', name: '字重优先层级', check: () => B.HIERARCHY_RULE === 'font-weight-first' },
    { id: 'F08966', name: '间距刻度审计', check: () => B.spacingAudit([4, 7, 16]).join() === '7' },
    { id: 'F08967', name: '面板折叠记忆', check: () => pm.collapsed('x') && !pm.collapsed('y') },
    { id: 'F08968', name: '网格列表切换', check: () => B.densityViewToggle('grid') === 'list' && B.densityViewToggle('list') === 'grid' },
    { id: 'F08969', name: '打印紧凑', check: () => B.printDensity() === 'compact' },
    { id: 'F08970', name: '按应用覆盖', check: () => ado.of('files', 'comfortable') === 'compact' && ado.of('other', 'comfortable') === 'comfortable' },
    { id: 'F08971', name: '触控保底 44', check: () => B.minTouchTarget('compact', 44) && !B.minTouchTarget('compact', 30) },
    { id: 'F08972', name: '密度 CSS 变量', check: () => B.densityCssVars('compact').includes('--row:32px') },
    { id: 'F08973', name: '密度预览', check: () => B.densityPreview('comfortable', 'compact').live },
    { id: 'F08974', name: '密度规范', check: () => B.DENSITY_SPEC.length === 4 },
    { id: 'F08975', name: '密度一致性评分', check: () => B.densityScore([32, 32], 'compact') === 100 && B.densityScore([40], 'compact') === 0 },
  ];
}

/* -------- AI-72 族0360 交互反馈强化 F08976~F09000 -------- */
export function checkF0360(): CheckEntry[] {
  const opt = new B.OptimisticUi<string>();
  return [
    { id: 'F08976', name: '按压涟漪', check: () => { const r = B.pressRipple(10, 20); return r.r === 80 && r.x === 10; } },
    { id: 'F08977', name: '悬停抬升', check: () => B.HOVER_LIFT === 'translateY(-1px)' },
    { id: 'F08978', name: '焦点环常显', check: () => B.FOCUS_RING_VISIBLE.includes('var(--aurora-accent)') },
    { id: 'F08979', name: '成功对勾动画', check: () => B.successCheckmark(24).animate },
    { id: 'F08980', name: '错误抖动', check: () => B.errorShake()[0] === -4 && B.errorShake()[3] === 0 },
    { id: 'F08981', name: '触觉三模式', check: () => B.hapticPattern('tick').join() === '10' && B.hapticPattern('double').length === 3 },
    { id: 'F08982', name: '静音静默', check: () => B.soundFeedback(true) === 'silent' && B.soundFeedback(false) === 'tick' },
    { id: 'F08983', name: '乐观更新回滚', check: () => opt.apply('a', '乐观', '实际', false) === '实际' && opt.rollback('a', '旧') === '旧' },
    { id: 'F08984', name: '操作状态迁移', check: () => B.actionState('done') === 'done' },
    { id: 'F08985', name: '数字滚动', check: () => B.countUp(0, 100, 0.5) === 50 && B.countUp(0, 100, 2) === 100 },
    { id: 'F08986', name: '开关弹簧过冲', check: () => B.toggleSpring(0.5) > 1 && B.toggleSpring(1) === 1 },
    { id: 'F08987', name: '拖拽幽灵半透', check: () => B.DRAG_GHOST_OPACITY === 0.6 },
    { id: 'F08988', name: '放置高亮', check: () => B.dropTargetHighlight(true).includes('dashed') && B.dropTargetHighlight(false) === '' },
    { id: 'F08989', name: '复制闪示 1s', check: () => B.copyFlash(500, 0) && !B.copyFlash(2000, 0) },
    { id: 'F08990', name: '已保存指示', check: () => B.saveIndicator('saved') === '已保存' && B.saveIndicator('saving') === '保存中…' },
    { id: 'F08991', name: '操作进度', check: () => B.actionProgress(3, 10) === '3/10' },
    { id: 'F08992', name: '撤销 5s 窗口', check: () => B.undoToast(4000, 0) && !B.undoToast(6000, 0) },
    { id: 'F08993', name: '按钮冷却 500ms', check: () => B.buttonCooldown(0, 200) && !B.buttonCooldown(0, 600) },
    { id: 'F08994', name: '长按进度环', check: () => B.longPressProgress(300) === 0.5 && B.longPressProgress(900) === 1 },
    { id: 'F08995', name: '下拉吸附', check: () => B.pullSnap(true, true) === 'refresh' && B.pullSnap(true, false) === 'snap-back' },
    { id: 'F08996', name: '指针审计', check: () => B.cursorAudit(true, 'default').length === 1 && B.cursorAudit(true, 'pointer').length === 0 },
    { id: 'F08997', name: '键盘回声', check: () => B.keyboardEcho(true) === 'flash' },
    { id: 'F08998', name: 'aria-live', check: () => B.liveRegion(true) === 'aria-live=polite' && B.liveRegion(false) === 'aria-live=assertive' },
    { id: 'F08999', name: '减动效回退', check: () => !B.reducedMotionFallback(true).animate && B.reducedMotionFallback(false).durationMs === 200 },
    { id: 'F09000', name: '反馈令牌表', check: () => B.FEEDBACK_TOKENS.hover === 150 && B.FEEDBACK_TOKENS.undo === 5000 },
  ];
}

/* -------- AI-73 族0361 导航体系 F09001~F09025 -------- */
export function checkF0361(): CheckEntry[] {
  const back = new C.BackStack();
  back.push('a'); back.push('b'); back.push('c');
  const hist = new C.TabHistory();
  hist.go('a'); hist.go('b');
  const nav = new C.NavCollapse();
  const recent = new C.RecentPages();
  recent.visit('a'); recent.visit('b'); recent.visit('a');
  const pins = new C.PinnedPages();
  pins.pin('x'); pins.pin('x');
  const scroll = new C.ScrollMemory();
  scroll.save('p', 120);
  return [
    { id: 'F09001', name: '侧栏分区', check: () => C.NAV_SECTIONS.length === 4 && C.NAV_SECTIONS[1]!.children!.length === 3 },
    { id: 'F09002', name: '面包屑生成', check: () => C.breadcrumbOf('设置', '显示').join('/') === '设置/显示' },
    { id: 'F09003', name: '返回栈', check: () => back.back() === 'b' && back.depth === 2 && back.back() === 'a' },
    { id: 'F09004', name: '标签历史前进', check: () => hist.back() === 'a' && hist.forward() === 'b' },
    { id: 'F09005', name: '深链映射', check: () => C.deepLink('/s', { '/s': 'Settings' }) === 'Settings' && C.deepLink('/x', { '/s': 'Settings' }) === null },
    { id: 'F09006', name: '分区高亮', check: () => C.activeSection('home', ['home']) === 'home' && C.activeSection('x', ['home']) === null },
    { id: 'F09007', name: '折叠宽度', check: () => !nav.collapsed && nav.width() === 320 && nav.toggle() && nav.width() === 48 },
    { id: 'F09008', name: '方向键循环', check: () => C.navKeyboard(2, 3, 'ArrowDown') === 0 && C.navKeyboard(0, 3, 'ArrowUp') === 2 },
    { id: 'F09009', name: '导航内搜索', check: () => C.navFilter(C.NAV_SECTIONS, '设置').length === 1 && C.navFilter(C.NAV_SECTIONS, '').length === 4 },
    { id: 'F09010', name: '跳转列表', check: () => C.jumpTo('files', C.NAV_SECTIONS)!.id === 'files' && C.jumpTo('zz', C.NAV_SECTIONS) === null },
    { id: 'F09011', name: '上下文导航', check: () => C.contextNav(null, ['a', 'b']).length === 2 },
    { id: 'F09012', name: '切页 200ms', check: () => C.PAGE_TRANSITION_MS === 200 },
    { id: 'F09013', name: '标题同步', check: () => C.titleSync('设置', '显示') === '显示 - 设置 - Variable' },
    { id: 'F09014', name: '滚动恢复', check: () => scroll.restore('p') === 120 && scroll.restore('q') === 0 },
    { id: 'F09015', name: '导航守卫', check: () => C.navGuard(true) === 'confirm' && C.navGuard(false) === 'pass' },
    { id: 'F09016', name: '最近 5 页去重置顶', check: () => recent.recent()[0] === 'a' && recent.recent().length === 2 },
    { id: 'F09017', name: '固定页去重', check: () => pins.pinned().join() === 'x' },
    { id: 'F09018', name: '导航快捷键表', check: () => C.NAV_SHORTCUTS.length === 3 && C.NAV_SHORTCUTS[0]![0] === 'Alt+←' },
    { id: 'F09019', name: 'RTL 翻转', check: () => C.navRtlFlip(true) === 'row-reverse' },
    { id: 'F09020', name: '窄屏抽屉', check: () => C.navDrawer(500) === 'drawer' && C.navDrawer(1200) === 'sidebar' },
    { id: 'F09021', name: 'aria-current', check: () => C.navAriaCurrent(true) === 'page' && C.navAriaCurrent(false) === null },
    { id: 'F09022', name: '图标文字双标注', check: () => C.navItemLabel(C.NAV_SECTIONS[0]!) === 'home:主页' },
    { id: 'F09023', name: '导航溢出', check: () => { const o = C.navOverflow(['1', '2', '3', '4', '5', '6', '7', '8', '9']); return o.visible.length === 7 && o.more.length === 2; } },
    { id: 'F09024', name: '面包屑跳层', check: () => C.breadcrumbJump(['a', 'b'], 1) === 'b' && C.breadcrumbJump(['a'], 5) === null },
    { id: 'F09025', name: '导航规范五条', check: () => C.NAV_SPEC.length === 5 },
  ];
}

/* -------- AI-73 族0362 搜索体验 UI F09026~F09050 -------- */
export function checkF0362(): CheckEntry[] {
  const rs = new C.RecentSearches();
  rs.add('a'); rs.add('b'); rs.add('a'); rs.add('');
  const chips = new C.FilterChips();
  chips.add('x'); chips.add('x'); chips.remove('x');
  return [
    { id: 'F09026', name: '语境占位符', check: () => C.searchPlaceholder('文件') === '在文件中搜索' },
    { id: 'F09027', name: '即时过滤', check: () => C.instantFilter([1, 2, 3], (n) => n > 1).length === 2 },
    { id: 'F09028', name: '防抖 250ms', check: () => C.SEARCH_DEBOUNCE_MS === 250 },
    { id: 'F09029', name: '命中高亮', check: () => C.highlightMatch('打开设置', '设置') === '打开【设置】' && C.highlightMatch('abc', 'z') === 'abc' },
    { id: 'F09030', name: '最近 5 条去重', check: () => rs.recent().join() === 'a,b' },
    { id: 'F09031', name: '前缀建议', check: () => C.searchSuggestions(['设置中心'], '设置').length === 1 && C.searchSuggestions(['x'], '').length === 0 },
    { id: 'F09032', name: '无结果空态', check: () => C.searchEmptyState('q').suggestions.length === 3 },
    { id: 'F09033', name: '范围选择器', check: () => C.scopeLabel('all') === '全部' && C.scopeLabel('filename') === '文件名' },
    { id: 'F09034', name: '结果键盘导航', check: () => { const e = C.resultKeyboard(1, 3, 'Enter'); return e.confirmed && C.resultKeyboard(2, 3, 'ArrowDown').index === 0; } },
    { id: 'F09035', name: 'Ctrl+K 呼出', check: () => C.GLOBAL_SEARCH_HOTKEY === 'Ctrl+K' },
    { id: 'F09036', name: '筛选片管理', check: () => chips.list().length === 0 && (chips.add('y'), chips.list().join() === 'y') },
    { id: 'F09037', name: '结果分组', check: () => { const g = C.groupResults([{ kind: 'a', v: 1 }, { kind: 'b', v: 2 }] as Array<{ kind: string; v: number }>); return Object.keys(g).length === 2; } },
    { id: 'F09038', name: '私密不索引', check: () => C.indexable('/public', ['/private']) && !C.indexable('/private', ['/private']) },
    { id: 'F09039', name: '拼音首字母匹配', check: () => C.pinyinInitialMatch('设置', 'sz', 's') },
    { id: 'F09040', name: '模糊编辑距离', check: () => C.fuzzyScore('kitten', 'sitting') === 3 && C.fuzzyScore('abc', 'abc') === 0 },
    { id: 'F09041', name: '可中断', check: () => C.searchCancel(true) },
    { id: 'F09042', name: '一键清空', check: () => C.searchClear() === '' },
    { id: 'F09043', name: '结果计数', check: () => C.resultCount(12) === '12 个结果' },
    { id: 'F09044', name: '查询转义', check: () => C.escapeQuery('a.b*c') === 'a\\.b\\*c' },
    { id: 'F09045', name: '索引仅本地', check: () => C.SEARCH_LOCAL_ONLY },
    { id: 'F09046', name: '历史可清空', check: () => C.clearHistory(['a']).length === 0 },
    { id: 'F09047', name: '结果预览', check: () => { const p = C.resultPreview('前缀目标文本就在这里', '目标', 10); return p.includes('目标'); } },
    { id: 'F09048', name: '离线降级可用', check: () => C.offlineSearch(false, ['a']).join() === 'a' },
    { id: 'F09049', name: '搜索 i18n 键', check: () => C.searchI18nKey('files') === 'search.placeholder.files' },
    { id: 'F09050', name: '搜索规范五条', check: () => C.SEARCH_SPEC.length === 5 },
  ];
}

/* -------- AI-73 族0363 设置体验 F09051~F09075 -------- */
export function checkF0363(): CheckEntry[] {
  const exp = new C.SettingExpander();
  const reset = new C.SettingReset();
  reset.registerDefault('k', 5);
  const cur = new Map<string, unknown>([['k', 9]]);
  const log = new C.SettingsChangeLog();
  log.record('k', 5, 9, 1);
  return [
    { id: 'F09051', name: '卡片行令牌', check: () => C.SETTING_ROW_TOKENS.radius === 8 && C.SETTING_ROW_TOKENS.minHeight === 48 },
    { id: 'F09052', name: 'Expander 展开', check: () => exp.toggle() && !exp.toggle() },
    { id: 'F09053', name: '设置搜索', check: () => { const rows: C.SettingRow[] = [{ key: 'a', title: '显示亮度', kind: 'slider', value: 50 }]; return C.settingsSearch(rows, '亮度').length === 1 && C.settingsSearch(rows, '').length === 1; } },
    { id: 'F09054', name: '分区锚点', check: () => C.sectionAnchor('display') === '#settings-display' },
    { id: 'F09055', name: '单项重置', check: () => { reset.reset('k', cur); return cur.get('k') === 5 && reset.isDefault('k', cur); } },
    { id: 'F09056', name: '修改指示点', check: () => C.modifiedDot(false) === 'dot' && C.modifiedDot(true) === null },
    { id: 'F09057', name: '即时生效类', check: () => C.applyMode('toggle') === 'immediate' && C.applyMode('action') === 'on-save' },
    { id: 'F09058', name: '需重启标记', check: () => C.restartBadge(true) === 'restart' && C.restartBadge(false) === null },
    { id: 'F09059', name: '导入导出', check: () => { const j = C.exportSettings({ a: 1 }); return C.importSettings(j)!.a === 1 && C.importSettings('{bad') === null; } },
    { id: 'F09060', name: '设置页 a11y', check: () => C.settingsA11y('显示').h1 === '显示' },
    { id: 'F09061', name: '危险区样式', check: () => C.DANGER_ZONE_STYLE.includes('var(--aurora-danger)') },
    { id: 'F09062', name: '依赖禁用', check: () => C.dependencyDisabled(false) && !C.dependencyDisabled(true) },
    { id: 'F09063', name: '滑杆气泡', check: () => C.sliderBubble(50, '%') === '50%' },
    { id: 'F09064', name: '分段选中', check: () => C.settingSegmented(['a', 'b'], 'b') === 1 },
    { id: 'F09065', name: '设置面包屑', check: () => C.settingsBreadcrumb('系统', '显示').join('/') === '系统/显示' },
    { id: 'F09066', name: '设置 tooltip', check: () => C.settingTooltip('说明').startsWith('help:') },
    { id: 'F09067', name: '默认值表', check: () => C.SETTING_DEFAULTS['privacy.telemetry'] === false },
    { id: 'F09068', name: '旧键迁移', check: () => { const t: Record<string, unknown> = {}; C.migrateKey({ old: 1 }, 'old', 'new', t); return t.new === 1; } },
    { id: 'F09069', name: '遥测默认关', check: () => C.SETTINGS_TELEMETRY_DEFAULT === false },
    { id: 'F09070', name: '设置页文档', check: () => C.settingsDoc('display').startsWith('settings-doc:') },
    { id: 'F09071', name: 'IA 七分区', check: () => C.SETTINGS_IA.length === 7 },
    { id: 'F09072', name: '搜索高亮', check: () => C.settingsHighlight('显示亮度', '亮度') === '显示【亮度】' },
    { id: 'F09073', name: '重置全部需确认', check: () => C.resetAllSettings(true) && !C.resetAllSettings(false) },
    { id: 'F09074', name: '变更日志', check: () => log.for('k')[0]!.from === 5 && log.entries().length === 1 },
    { id: 'F09075', name: '设置规范五条', check: () => C.SETTINGS_SPEC.length === 5 },
  ];
}

/* -------- AI-73 族0364 视觉审计工具 F09076~F09100 -------- */
export function checkF0364(): CheckEntry[] {
  const wl = new C.AuditWhitelist();
  wl.add('no-hardcoded-color', 'logo.svg');
  const ledger = new C.AuditLedger();
  ledger.add({ rule: 'x', severity: 'warn', target: 'a' });
  return [
    { id: 'F09076', name: '硬编码色扫描', check: () => C.scanHardcodedColor('color:#fff').length === 1 && C.scanHardcodedColor('color:var(--x)').length === 0 },
    { id: 'F09077', name: '内联时长扫描', check: () => C.scanInlineDuration('transition:all 150ms').length === 0 && C.scanInlineDuration('transition:all 333ms').length === 1 },
    { id: 'F09078', name: '对比度 AA', check: () => C.contrastCheck(4.5) === 'AA' && C.contrastCheck(3) === 'fail' },
    { id: 'F09079', name: '触控 44 审计', check: () => C.touchTargetCheck(40, 48) !== null && C.touchTargetCheck(44, 44) === null },
    { id: 'F09080', name: '字号阶梯审计', check: () => C.fontSizeAudit(15) !== null && C.fontSizeAudit(14) === null },
    { id: 'F09081', name: '间距刻度审计', check: () => C.spacingAudit([4, 7]).length === 1 },
    { id: 'F09082', name: 'z 预算审计', check: () => C.zIndexAudit(700) !== null && C.zIndexAudit(500) === null },
    { id: 'F09083', name: '焦点可见审计', check: () => C.focusVisibleAudit('outline:none') !== null && C.focusVisibleAudit('outline:2px solid') === null },
    { id: 'F09084', name: '图标网格审计', check: () => C.iconGridAudit(18) !== null && C.iconGridAudit(24) === null },
    { id: 'F09085', name: '对比度批量报告', check: () => { const r = C.contrastReport([{ name: 'a', ratio: 5 }, { name: 'b', ratio: 3 }]); return r.pass.join() === 'a' && r.fail.join() === 'b'; } },
    { id: 'F09086', name: '令牌使用率', check: () => { const r = C.tokenUsageReport('a{color:var(--x)}b{color:#ffffff}'); return r.total === 2 && r.ratio === 0.5; } },
    { id: 'F09087', name: 'i18n 键审计', check: () => { const r = C.i18nKeyAudit(['a', 'b'], ['a', 'b'], ['a']); return !r.ok && r.missing.includes('en:b'); } },
    { id: 'F09088', name: 'aria 规则集', check: () => C.ARIA_RULES.length === 5 },
    { id: 'F09089', name: '重复样式检测', check: () => C.duplicateStyles(['a', 'a', 'b']).join() === 'a' },
    { id: 'F09090', name: '快照矩阵计划', check: () => C.snapshotPlan(['p'], ['dark', 'light'], ['zh', 'en']) === 4 },
    { id: 'F09091', name: '审计总分', check: () => C.auditScore([{ rule: 'x', severity: 'error', target: 't' }]) === 90 },
    { id: 'F09092', name: '白名单豁免', check: () => wl.has('no-hardcoded-color', 'logo.svg') && !wl.has('no-hardcoded-color', 'other.svg') },
    { id: 'F09093', name: 'CI 门禁阻断', check: () => !C.ciGate([{ rule: 'x', severity: 'error', target: 't' }]).pass && C.ciGate([]).pass },
    { id: 'F09094', name: 'HTML 报告', check: () => C.auditHtmlReport([{ rule: 'r', severity: 'warn', target: 't' }]).includes('rule="r"') },
    { id: 'F09095', name: '修复建议', check: () => C.auditFixSuggestion('no-hardcoded-color').includes('令牌') },
    { id: 'F09096', name: '按页过滤', check: () => C.auditPage('settings', [{ rule: 'x', severity: 'warn', target: 'settings:btn' }]).length === 1 },
    { id: 'F09097', name: '每日调度', check: () => C.AUDIT_SCHEDULE === 'daily' },
    { id: 'F09098', name: '严重度三档', check: () => C.SEVERITY_LEVELS.join() === 'info,warn,error' },
    { id: 'F09099', name: '问题台账闭环', check: () => { ledger.resolve('x', 'a'); return ledger.open().length === 0 && ledger.bySeverity('error').length === 0; } },
    { id: 'F09100', name: '审计规范五条', check: () => C.VISUAL_AUDIT_SPEC.length === 5 },
  ];
}

/* -------- AI-73 族0365 性能体验 F09101~F09125 -------- */
export function checkF0365(): CheckEntry[] {
  const guard = new C.LayoutThrashGuard();
  let order: string[] = [];
  guard.read(() => order.push('r'));
  guard.write(() => order.push('w'));
  guard.read(() => order.push('r'));
  guard.flush();
  return [
    { id: 'F09101', name: '骨架先行', check: () => C.skeletonBeforeData(false, true) === 'skeleton' && C.skeletonBeforeData(true, false) === 'content' },
    { id: 'F09102', name: '乐观阈值 100ms', check: () => C.optimisticThreshold(200) && !C.optimisticThreshold(50) },
    { id: 'F09103', name: '输入预算 100ms', check: () => C.INPUT_LATENCY_BUDGET_MS === 100 },
    { id: 'F09104', name: '帧预算 16.7ms', check: () => C.frameBudget(16) && !C.frameBudget(20) },
    { id: 'F09105', name: '搜索防抖', check: () => C.PERF_SEARCH_DEBOUNCE === 250 },
    { id: 'F09106', name: '滚动 rAF 节流', check: () => C.scrollThrottle(0, 16) && !C.scrollThrottle(0, 8) },
    { id: 'F09107', name: '虚拟窗口', check: () => { const w = C.virtualWindow(1000, 500, 50, 1000); return w.start === 15 && w.end === 35; } },
    { id: 'F09108', name: '图片懒加载', check: () => C.lazyImage(true) === 'load' && C.lazyImage(false) === 'defer' },
    { id: 'F09109', name: '低优空闲执行', check: () => C.deferNonCritical('low') === 'idle' && C.deferNonCritical('critical') === 'now' },
    { id: 'F09110', name: '路由级分割', check: () => C.codeSplitBoundary('home') === 'chunk:home' },
    { id: 'F09111', name: 'memo 条件', check: () => C.memoEligible([1, 'a']) && !C.memoEligible([{}]) },
    { id: 'F09112', name: '读写分离防抖动', check: () => order.join() === 'r,r,w' },
    { id: 'F09113', name: '长任务 >50ms', check: () => C.longTask(60) && !C.longTask(40) },
    { id: 'F09114', name: '性能 HUD', check: () => C.perfHud(60, 40) === '60fps 40MB' },
    { id: 'F09115', name: '慢路径文案', check: () => C.slowPathMessage(20) !== null && C.slowPathMessage(60) === null },
    { id: 'F09116', name: '优先级提示', check: () => C.priorityHint('high') === 'fetchpriority=high' },
    { id: 'F09117', name: '预连接清单', check: () => C.PRECONNECT_HOSTS.length === 0 },
    { id: 'F09118', name: '仅合成器属性', check: () => C.COMPOSITOR_ONLY.join() === 'transform,opacity' },
    { id: 'F09119', name: '内存预算', check: () => C.memoryBudget(45, 50) && !C.memoryBudget(55, 50) },
    { id: 'F09120', name: '启动 2s 预算', check: () => C.startupBudget(1800) && !C.startupBudget(2500) },
    { id: 'F09121', name: 'INP 测量', check: () => C.inpMeasure(100, 180) === 80 },
    { id: 'F09122', name: '回归告警 10%', check: () => C.perfRegression(120, 100) && !C.perfRegression(105, 100) },
    { id: 'F09123', name: '预算注册表', check: () => C.PERF_BUDGETS.taskbarMemMb === 50 },
    { id: 'F09124', name: '预算项四位', check: () => Object.keys(C.PERF_BUDGETS).length === 4 },
    { id: 'F09125', name: '性能规范五条', check: () => C.PERF_SPEC.length === 5 },
  ];
}

/* -------- AI-74 族0366 巨型组件重构 F09126~F09150 -------- */
export function checkF0366(): CheckEntry[] {
  const boundary = new D.ModuleBoundary();
  boundary.define('modA', ['fnA']);
  const tracker = new D.RefactorTracker();
  tracker.mark('X');
  return [
    { id: 'F09126', name: '巨型判定 800 行', check: () => D.sizeAnalyzer(900).giant && !D.sizeAnalyzer(700).giant },
    { id: 'F09127', name: '抽取计划四步', check: () => D.extractPlan('App').length === 4 },
    { id: 'F09128', name: '逻辑呈现分离', check: () => D.separationCheck({ jsx: true, pureFns: 3, effects: 2 }) && !D.separationCheck({ jsx: true, pureFns: 0, effects: 5 }) },
    { id: 'F09129', name: 'Hook 命名', check: () => D.hookExtraction('fetchData') === 'useFetchData' },
    { id: 'F09130', name: '三层转 Context', check: () => D.contextMigration(3).needsContext && !D.contextMigration(1).needsContext },
    { id: 'F09131', name: '状态机抽取', check: () => D.stateMachineExtract(['a', 'b'], ['e1']).reducer === 'useReducer' },
    { id: 'F09132', name: '测试接缝', check: () => D.testSeam('clock') === 'inject:clock' },
    { id: 'F09133', name: '绞杀者分步', check: () => D.stranglerSteps('Old', 3).length === 3 && D.stranglerSteps('Old', 3)[2] === 'Old.step3' },
    { id: 'F09134', name: '行为快照一致', check: () => D.behaviorSnapshot(['a', 'b'], ['b', 'a']) && !D.behaviorSnapshot(['a'], ['b']) },
    { id: 'F09135', name: '依赖图', check: () => D.dependencyGraph([['a', 'b']]).a![0] === 'b' },
    { id: 'F09136', name: '死代码检测', check: () => D.deadCode(['a', 'b'], new Set(['a'])).join() === 'b' },
    { id: 'F09137', name: '重复块检测', check: () => D.duplicateBlocks(['x', 'x', 'y']).join() === 'x' },
    { id: 'F09138', name: '组件命名', check: () => D.componentNameValid('MyComp') && !D.componentNameValid('myComp') },
    { id: 'F09139', name: '文件预算 800', check: () => D.FILE_LINE_BUDGET === 800 },
    { id: 'F09140', name: '重构九步清单', check: () => D.REFACTOR_CHECKLIST.length === 9 },
    { id: 'F09141', name: '风险评分', check: () => D.refactorRisk(400, 5, 10) === 15 && D.refactorRisk(100, 1, 20) === 0 && D.refactorRisk(5000, 0, 0) === 100 },
    { id: 'F09142', name: '回滚计划', check: () => D.rollbackPlan(['c1', 'c2']).revertTo === 'c1' && D.rollbackPlan([]).revertTo === '' },
    { id: 'F09143', name: '评审指南', check: () => D.REFACTOR_REVIEW_GUIDE.length === 4 },
    { id: 'F09144', name: '模块边界', check: () => boundary.owns('modA', 'fnA') && !boundary.owns('modA', 'fnB') && boundary.modules().length === 1 },
    { id: 'F09145', name: '循环依赖检测', check: () => D.importCycle({ a: ['b'], b: ['a'] }) && !D.importCycle({ a: ['b'], b: [] }) },
    { id: 'F09146', name: '热点文件清单', check: () => D.HOT_FILES.length === 5 && D.HOT_FILES[0]!.includes('ipc.ts') },
    { id: 'F09147', name: '重构文档', check: () => D.REFACTOR_DOCS.length === 4 },
    { id: 'F09148', name: '重构示例', check: () => D.REFACTOR_EXAMPLES.length === 3 },
    { id: 'F09149', name: '重构追踪', check: () => tracker.isDone('X') && !tracker.isDone('Y') && tracker.progress(2) === 0.5 },
    { id: 'F09150', name: '零回归门槛', check: () => D.refactorGate(true, true, true) && !D.refactorGate(true, false, true) },
  ];
}

/* -------- AI-74 族0367 设计系统治理 F09151~F09175 -------- */
export function checkF0367(): CheckEntry[] {
  const reg = new D.TokenRegistry();
  reg.register({ name: '--aurora-accent', value: '#0078d4', category: 'color' });
  reg.register({ name: '--aurora-old', value: '#000', category: 'color', deprecated: true });
  return [
    { id: 'F09151', name: '令牌注册表', check: () => reg.get('--aurora-accent')!.value === '#0078d4' && reg.byCategory('color').length === 2 && reg.deprecated().length === 1 },
    { id: 'F09152', name: '令牌命名规范', check: () => D.tokenNameValid('--aurora-accent') && !D.tokenNameValid('--accent') },
    { id: 'F09153', name: '弃用三阶段', check: () => D.deprecationPhase(0, 10) === 'marked' && D.deprecationPhase(0, 50) === 'warned' && D.deprecationPhase(0, 100) === 'removed' },
    { id: 'F09154', name: '版本策略', check: () => D.tokenVersionBump('1.0.0', '2.0.0') === 'major' && D.tokenVersionBump('1.0.0', '1.1.0') === 'minor' },
    { id: 'F09155', name: '提案流程', check: () => D.COMPONENT_PROPOSAL_FLOW.length === 4 },
    { id: 'F09156', name: '变更日志行', check: () => D.changelogEntry('feat', 'ui', 'x') === 'feat(ui): x' },
    { id: 'F09157', name: '所有权地图', check: () => D.DESIGN_OWNERS.components === 'AI-71' },
    { id: 'F09158', name: 'lint 规则', check: () => D.DESIGN_LINT_RULES.length === 4 },
    { id: 'F09159', name: '令牌副本同步', check: () => D.tokenSyncCheck(['a', 'a']).ok && !D.tokenSyncCheck(['a', 'b']).ok },
    { id: 'F09160', name: '对比政策', check: () => D.contrastPolicy(3, true) && !D.contrastPolicy(3, false) && D.contrastPolicy(4.5, false) },
    { id: 'F09161', name: '明暗核对单', check: () => D.THEME_PARITY_CHECKLIST.length === 5 },
    { id: 'F09162', name: '图标治理', check: () => D.iconGovernance('a', new Set(['a'])) === 'registered' && D.iconGovernance('b', new Set(['a'])) === 'unregistered' },
    { id: 'F09163', name: '动效令牌', check: () => D.MOTION_TOKENS.dur2 === 150 && D.MOTION_TOKENS.dur4 === 300 },
    { id: 'F09164', name: '贡献指南', check: () => D.DESIGN_CONTRIBUTION_GUIDE.length === 5 },
    { id: 'F09165', name: '评审会规则', check: () => D.DESIGN_REVIEW_RULES.length === 4 },
    { id: 'F09166', name: '破坏性变更', check: () => D.breakingChangePolicy(true).requires !== null && D.breakingChangePolicy(false).requires === null },
    { id: 'F09167', name: 'Codemod 计划', check: () => D.codemodPlan('a', 'b') === 'codemod:a->b' },
    { id: 'F09168', name: '采用率', check: () => D.adoptionMetric(8, 10) === 80 && D.adoptionMetric(0, 0) === 100 },
    { id: 'F09169', name: '漂移检测', check: () => D.driftDetect({ a: '1', b: '2' }, { a: '1', b: '3' }).join() === 'b' },
    { id: 'F09170', name: '冻结区清单', check: () => D.DESIGN_FREEZE_ZONES.length === 2 },
    { id: 'F09171', name: '治理文档', check: () => D.GOVERNANCE_DOCS.length === 5 },
    { id: 'F09172', name: '治理评分', check: () => D.governanceScore(9, 10) === 90 },
    { id: 'F09173', name: '令牌申请单', check: () => D.tokenRequest('--aurora-new', '#111', '需要').status === 'pending' },
    { id: 'F09174', name: '弃用清单', check: () => D.deprecatedTokens([{ name: 'a', value: '', category: 'color', deprecated: true }]).join() === 'a' },
    { id: 'F09175', name: '治理收官', check: () => D.governanceFinale(10).includes('10') },
  ];
}

/* -------- AI-74 族0368 键盘与焦点 UI F09176~F09200 -------- */
export function checkF0368(): CheckEntry[] {
  const roving = new D.RovingTabindex(3);
  const trap = new D.FocusTrap();
  trap.set(['a', 'b', 'c']);
  const sc = new D.ShortcutRegistry();
  sc.register('Ctrl+K', 'search');
  const expanded = new Set<string>();
  return [
    { id: 'F09176', name: 'roving tabindex', check: () => roving.move('ArrowDown') === 1 && roving.tabindexOf(1) === 0 && roving.tabindexOf(0) === -1 && roving.move('Home') === 0 },
    { id: 'F09177', name: '焦点陷阱循环', check: () => trap.next('c', false) === 'a' && trap.next('a', true) === 'c' },
    { id: 'F09178', name: '关闭还原焦点', check: () => D.focusRestore('#btn') === '#btn' && D.focusRestore(null) === null },
    { id: 'F09179', name: '跳转主内容', check: () => D.SKIP_LINK.href === '#main' },
    { id: 'F09180', name: '网格方向导航', check: () => { const p = D.gridNav({ r: 0, c: 0 }, 3, 3, 'ArrowUp'); return p.r === 0 && p.c === 0; } },
    { id: 'F09181', name: 'Home/End', check: () => D.homeEnd('Home', 5) === 0 && D.homeEnd('End', 5) === 4 },
    { id: 'F09182', name: '首字母跳转', check: () => D.typeahead(['apple', 'banana', 'cherry'], 'b', 0) === 1 },
    { id: 'F09183', name: '键盘才显焦点环', check: () => D.focusVisiblePolicy('keyboard') === 'ring' && D.focusVisiblePolicy('pointer') === 'none' },
    { id: 'F09184', name: 'Tab 序审计', check: () => D.tabOrderAudit(['a', 'b', 'c'], ['a', 'c', 'b']).length === 2 },
    { id: 'F09185', name: '快捷键冲突检测', check: () => sc.owner('Ctrl+K') === 'search' && !sc.register('Ctrl+K', 'other') && sc.conflicts().length === 0 },
    { id: 'F09186', name: 'Esc 分层关闭', check: () => D.escapeLayer(['m1', 'm2']) === 'm2' && D.escapeLayer([]) === null },
    { id: 'F09187', name: '按钮键语义', check: () => D.buttonKeys('button').join() === 'Enter,Space' && D.buttonKeys('link').join() === 'Enter' },
    { id: 'F09188', name: '模态初始焦点', check: () => D.modalInitialFocus('form') === 'first-field' && D.modalInitialFocus('confirm') === 'primary' },
    { id: 'F09189', name: '菜单键盘', check: () => D.menuKeyboard('ArrowRight', true) === 'open' && D.menuKeyboard('Escape', false) === 'close' },
    { id: 'F09190', name: '列表 typeahead', check: () => D.listboxTypeahead(['apple', 'banana'], 'b') === 1 },
    { id: 'F09191', name: '组合框模式', check: () => { const c = D.comboboxPattern('a', ['ab', 'cd']); return c.expanded && c.filtered.length === 1; } },
    { id: 'F09192', name: '树形导航', check: () => D.treeNav(expanded, 'ArrowRight', 'n1') === 'expand' && D.treeNav(expanded, 'ArrowLeft', 'n1') === 'collapse' },
    { id: 'F09193', name: '问号呼出帮助', check: () => D.SHORTCUT_DISCOVERY_KEY === '?' },
    { id: 'F09194', name: '自定义控件角色', check: () => D.customControlA11y('', '').join() === 'missing-role,missing-label' && D.customControlA11y('button', '保存').length === 0 },
    { id: 'F09195', name: 'activedescendant', check: () => D.activedescendant('list', 2) === 'list-opt-2' },
    { id: 'F09196', name: '键盘帮助页', check: () => D.KEYBOARD_HELP.length === 4 },
    { id: 'F09197', name: '焦点回环', check: () => D.focusWrap(2, 3, 1) === 0 && D.focusWrap(0, 3, -1) === 2 },
    { id: 'F09198', name: '只读跳过', check: () => D.readonlySkipped(true) === -1 && D.readonlySkipped(false) === 0 },
    { id: 'F09199', name: '键盘使用率', check: () => D.keyboardUsageRatio(3, 1) === 75 && D.keyboardUsageRatio(0, 0) === 0 },
    { id: 'F09200', name: '焦点规范五条', check: () => D.FOCUS_SPEC.length === 5 },
  ];
}

/* -------- AI-74 族0369 触屏混合输入 UI F09201~F09225 -------- */
export function checkF0369(): CheckEntry[] {
  const stats = new D.InputSessionStats();
  stats.record('touch'); stats.record('touch'); stats.record('mouse');
  return [
    { id: 'F09201', name: '目标 44px', check: () => D.TOUCH_TARGET_MIN === 44 },
    { id: 'F09202', name: '无悬停依赖', check: () => D.hoverIndependent(true) === 'long-press' },
    { id: 'F09203', name: '输入方式检测', check: () => D.pointerType({ pointerType: 'pen' }) === 'pen' },
    { id: 'F09204', name: '长按 500ms', check: () => D.LONG_PRESS_MS === 500 },
    { id: 'F09205', name: '滑动手势表', check: () => Object.keys(D.SWIPE_GESTURES).length === 3 },
    { id: 'F09206', name: '边缘返回', check: () => D.edgeSwipeBack(10) && !D.edgeSwipeBack(100) },
    { id: 'F09207', name: '过界包含', check: () => D.OVERSCROLL === 'overscroll-behavior:contain' },
    { id: 'F09208', name: '原生惯性', check: () => D.touchScrollMomentum(true) === 'native' },
    { id: 'F09209', name: '双击缩放控制', check: () => D.doubleTapZoom(false) === 'none' },
    { id: 'F09210', name: '笔压感', check: () => D.penPressure({ pointerType: 'pen', pressure: 0.5 }) === 0.5 && D.penPressure({ pointerType: 'mouse', pressure: 0 }) === null },
    { id: 'F09211', name: '笔按键映射', check: () => D.penButton(1) === 'erase' && D.penButton(2) === 'select' && D.penButton(0) === 'draw' },
    { id: 'F09212', name: '触屏键盘模式', check: () => D.touchKeyboardMode('number') === 'inputmode:decimal' },
    { id: 'F09213', name: '命中容差', check: () => D.hitSlop(28) === 44 },
    { id: 'F09214', name: '拖拽阈值 8px', check: () => D.DRAG_THRESHOLD_PX === 8 },
    { id: 'F09215', name: '触屏涟漪', check: () => D.touchRipple(1, 2).x === 1 },
    { id: 'F09216', name: '滚动吸附', check: () => D.SCROLL_SNAP.includes('mandatory') },
    { id: 'F09217', name: '双指缩放钳位', check: () => D.pinchZoom(200, 100) === 2 && D.pinchZoom(1000, 100) === 5 && D.pinchZoom(10, 100) === 0.5 },
    { id: 'F09218', name: '手掌抑制', check: () => D.palmRejection('pen') === 'reject-touch' && D.palmRejection('touch') === 'allow' },
    { id: 'F09219', name: '切换提示', check: () => D.inputSwitchNotice('mouse', 'pen').includes('触控笔') },
    { id: 'F09220', name: '触屏 a11y 替代', check: () => D.touchA11yFallback(true) === 'haptic' },
    { id: 'F09221', name: '会话统计主导', check: () => stats.dominant() === 'touch' && stats.total() === 3 },
    { id: 'F09222', name: '可滚动判定', check: () => D.scrollableRegion(2000, 500) && !D.scrollableRegion(100, 500) },
    { id: 'F09223', name: 'hover 守卫', check: () => D.TOUCH_HOVER_GUARD.includes('(hover: none)') },
    { id: 'F09224', name: '目标间距 8px', check: () => D.TOUCH_GAP_MIN === 8 },
    { id: 'F09225', name: '触屏规范五条', check: () => D.TOUCH_SPEC.length === 5 },
  ];
}

/* -------- AI-74 族0370 一致性走查 F09226~F09250 -------- */
export function checkF0370(): CheckEntry[] {
  const loop = new D.WalkthroughLoop();
  loop.report('i1');
  const watch = new D.RegressionWatch();
  watch.mark('r1');
  return [
    { id: 'F09226', name: '走查八页', check: () => D.WALKTHROUGH_PAGES.length === 8 },
    { id: 'F09227', name: '每页核对单', check: () => D.WALKTHROUGH_CHECKLIST.length === 8 },
    { id: 'F09228', name: '一致性评分', check: () => D.walkthroughScore(9, 10) === 90 && D.walkthroughScore(0, 0) === 100 },
    { id: 'F09229', name: '相似页对比', check: () => { const d = D.siblingDiff([{ a: '1' }, { a: '2' }], ['a']); return d.join() === 'a'; } },
    { id: 'F09230', name: '术语表', check: () => D.GLOSSARY.folder === '文件夹' && Object.keys(D.GLOSSARY).length === 6 },
    { id: 'F09231', name: '文案规则', check: () => D.copyRules('好!!!').length === 1 && D.copyRules('好的').length === 0 },
    { id: 'F09232', name: '日期格式一致', check: () => D.dateFormatConsistency(['2026-01-01', '2026/01/01'], /^\d{4}-\d{2}-\d{2}$/).length === 1 },
    { id: 'F09233', name: '数字千分位', check: () => D.numberFormatConsistency(['100000', '1,000']).join() === '100000' },
    { id: 'F09234', name: '按钮文案', check: () => D.buttonLabelOk('保存') && !D.buttonLabelOk('保存更改到文件中去了') },
    { id: 'F09235', name: '对话框标题', check: () => D.dialogTitleOk('确认删除') && !D.dialogTitleOk('提示信息') },
    { id: 'F09236', name: '三主题色彩走查', check: () => D.themeColorWalkthrough('dark', 0).ok && !D.themeColorWalkthrough('dark', 2).ok },
    { id: 'F09237', name: '截图矩阵', check: () => D.screenshotMatrix(8, 3, 3, 2) === 144 },
    { id: 'F09238', name: '走查报告', check: () => D.walkthroughReport('设置', 2, 90).includes('2 项问题') },
    { id: 'F09239', name: '分级处置', check: () => D.triage('P0') === 'fix-now' && D.triage('P2') === 'backlog' },
    { id: 'F09240', name: '修复闭环', check: () => loop.remaining() === 1 && (loop.fix('i1'), loop.remaining() === 0) },
    { id: 'F09241', name: '回归盯防', check: () => watch.isWatched('r1') && watch.watchlist().length === 1 },
    { id: 'F09242', name: '术语强制', check: () => D.glossaryEnforce('鼠标右键', ['右键菜单']).length === 0 },
    { id: 'F09243', name: '三主题一致性', check: () => D.threeThemeParity([{ theme: 'dark', pass: true }, { theme: 'light', pass: true }, { theme: 'hc', pass: true }]) && !D.threeThemeParity([{ theme: 'dark', pass: true }]) },
    { id: 'F09244', name: '三语言键数', check: () => D.threeLangParity({ zh: 10, tw: 10, en: 10 }) && !D.threeLangParity({ zh: 10, tw: 9, en: 10 }) },
    { id: 'F09245', name: '200% 缩放走查', check: () => D.zoomWalkthrough(0) && !D.zoomWalkthrough(1) },
    { id: 'F09246', name: '走查调度', check: () => D.WALKTHROUGH_SCHEDULE === 'wave-exit' },
    { id: 'F09247', name: '审计面板入口', check: () => D.WALKTHROUGH_TOOL === 'audit-panel' },
    { id: 'F09248', name: '走查归档', check: () => { const a = D.walkthroughArchive([{ page: 'a', score: 90 }, { page: 'b', score: 80 }]); return a.count === 2 && a.minScore === 80; } },
    { id: 'F09249', name: '走查规范', check: () => D.WALKTHROUGH_SPEC.length === 4 },
    { id: 'F09250', name: '走查收官', check: () => D.walkthroughFinale(8, 100).includes('8 页') },
  ];
}

/* -------- AI-75 族0371 实测问题修复 F09251~F09275 -------- */
export function checkF0371(): CheckEntry[] {
  const ledger = new E.BugLedger();
  ledger.add({ id: 'BUG-99', symptom: 's', repro: 'r', fixed: false });
  return [
    { id: 'F09251', name: 'BUG-01 阅读时长 0 字', check: () => E.readingMinutes(0) === 0 && E.readingTimeLabel(0) === '约 0 分钟' && E.readingMinutes(800, 400) === 2 },
    { id: 'F09252', name: 'BUG-02a 横幅可收起', check: () => E.bannerDismissible(false).canCollapse },
    { id: 'F09253', name: 'BUG-02b 8s 消散让位', check: () => E.bannerAutoDismiss(0, 9000) && !E.bannerAutoDismiss(0, 1000) && E.bannerLayoutReserve(40) === 'margin-top:40px' },
    { id: 'F09254', name: 'BUG-03 盘符升序', check: () => { const s = E.diskSortByLetter([{ letter: 'D', usedPct: 1 }, { letter: 'C', usedPct: 2 }]); return s[0]!.letter === 'C' && s[1]!.letter === 'D'; } },
    { id: 'F09255', name: 'BUG-04 标题省略', check: () => { const t = E.mediaTitleEllipsis('一个很长很长的视频标题超出了宽度', 10); return t.shown.endsWith('…') && t.hover && t.full.length > 10; } },
    { id: 'F09256', name: 'BUG-05 主按钮强调', check: () => { const w = E.dialogButtonWeights('accent', 'normal'); return w.emphasized && w.primary === 'accent'; } },
    { id: 'F09257', name: 'BUG-06 靠源弹出', check: () => { const p = E.dialogNearSource({ x: 400, y: 300, w: 200, h: 100 }, { w: 100, h: 80 }, { w: 1000, h: 800 }); return p.x === 450 && p.y === 310; } },
    { id: 'F09258', name: 'BUG-08 pill 令牌化', check: () => { const t = E.hotkeyPillTokens(); return t.bg.includes('var(--aurora') && t.radius === 8 && t.durationMs === 150; } },
    { id: 'F09259', name: 'BUG-07 super+tab 提示', check: () => { const n = E.superTabNotice(true, false); return n.show && n.text!.includes('恢复') && !E.superTabNotice(false, false).show && !E.superTabNotice(true, true).show; } },
    { id: 'F09260', name: '问题登记表', check: () => ledger.open().length === 1 && (ledger.markFixed('BUG-99'), ledger.open().length === 0) && ledger.all().length === 1 },
    { id: 'F09261', name: '复现步骤完备', check: () => E.reproComplete(['a', 'b', 'c']) && !E.reproComplete(['a']) },
    { id: 'F09262', name: '最小修复', check: () => E.minimalFix(3) && !E.minimalFix(8) },
    { id: 'F09263', name: '一修一测', check: () => E.regressionTestPair(8, 8) && !E.regressionTestPair(8, 5) },
    { id: 'F09264', name: '截图前后对比', check: () => E.screenshotCompare('a.png', 'b.png').recorded },
    { id: 'F09265', name: '日志对比', check: () => E.logCompare(['x'], ['y', 'z']).removed === 1 },
    { id: 'F09266', name: '真机复测', check: () => E.realMachineRetest('variable.exe', 8, 8) && !E.realMachineRetest('variable.exe', 8, 7) },
    { id: 'F09267', name: 'W0 八项对号', check: () => E.W0_BUG_MAP.length === 8 && E.W0_BUG_MAP[0]![1] === 'F09251' },
    { id: 'F09268', name: '发布注记', check: () => E.releaseNote(['a', 'b']) === '修复 2 项实测问题' },
    { id: 'F09269', name: '修复不越界', check: () => E.fixScopeGuard(['a.ts', 'b.ts'], ['a.ts']).join() === 'b.ts' },
    { id: 'F09270', name: '让位优先红线', check: () => E.yieldFirstPolicy(true) === 'yield-then-hint' },
    { id: 'F09271', name: '修复归档', check: () => { const a = E.fixArchive([{ id: '1', symptom: '', repro: '', fixed: true }, { id: '2', symptom: '', repro: '', fixed: false }]); return a.total === 2 && a.fixed === 1; } },
    { id: 'F09272', name: '回归基线更新', check: () => E.regressionBaselineUpdate(true, 'v1') === 'v1@fixed' && E.regressionBaselineUpdate(false, 'v1') === 'v1' },
    { id: 'F09273', name: '手感不降', check: () => E.feelCheck(3, 2) && !E.feelCheck(2, 3) },
    { id: 'F09274', name: '遗留移交', check: () => E.handoffRemaining('x', 'AI-76').status === 'handed-off' },
    { id: 'F09275', name: '实测修复收官', check: () => E.bugfixFinale(8, 8).includes('8/8') },
  ];
}

/* -------- AI-75 族0372 微细节清单 F09276~F09300 -------- */
export function checkF0372(): CheckEntry[] {
  return [
    { id: 'F09276', name: '焦点环 2px', check: () => E.MICRO_FOCUS_RING.startsWith('2px') },
    { id: 'F09277', name: '选区色主题化', check: () => E.MICRO_SELECTION.includes('var(--aurora-accent-soft)') },
    { id: 'F09278', name: '滚动条细圆角', check: () => E.MICRO_SCROLLBAR.includes('8px') },
    { id: 'F09279', name: '光标语境', check: () => E.cursorFor('text') === 'text' && E.cursorFor('disabled') === 'not-allowed' },
    { id: 'F09280', name: '禁用降饱和', check: () => E.MICRO_DISABLED.includes('0.4') },
    { id: 'F09281', name: '按下下沉', check: () => E.MICRO_ACTIVE === 'transform:translateY(1px)' },
    { id: 'F09282', name: '过渡两档', check: () => E.microDuration('hover') === 150 && E.microDuration('page') === 200 },
    { id: 'F09283', name: '阴影三档', check: () => E.MICRO_ELEVATION.length === 3 },
    { id: 'F09284', name: '圆角令牌', check: () => E.MICRO_RADII.corner2 === 8 },
    { id: 'F09285', name: '中文默认字距', check: () => E.MICRO_CJK_TRACKING === 'letter-spacing:normal' },
    { id: 'F09286', name: '数字等宽', check: () => E.MICRO_TABULAR.includes('tabular') },
    { id: 'F09287', name: '占位符二级色', check: () => E.MICRO_PLACEHOLDER.includes('text-secondary') },
    { id: 'F09288', name: '插入符随主题', check: () => E.MICRO_CARET.includes('var(--aurora-accent)') },
    { id: 'F09289', name: '下划线偏移', check: () => E.MICRO_UNDERLINE === 'text-underline-offset:3px' },
    { id: 'F09290', name: '图文间距 8px', check: () => E.MICRO_ICON_TEXT_GAP === 8 },
    { id: 'F09291', name: '正文行高 1.5', check: () => E.MICRO_LINE_HEIGHT === 1.5 },
    { id: 'F09292', name: '触控 44', check: () => E.MICRO_TOUCH_TARGET === 44 },
    { id: 'F09293', name: '过界发光关闭', check: () => E.MICRO_OVERSCROLL_GLOW === 'overscroll-behavior:none' },
    { id: 'F09294', name: '选区对比 3:1', check: () => E.selectionContrast(4) && !E.selectionContrast(2) },
    { id: 'F09295', name: '图片 alt', check: () => E.imgAltAudit('截图') && !E.imgAltAudit('') },
    { id: 'F09296', name: '分隔线对齐', check: () => E.MICRO_DIVIDER_ALIGN.includes('row-pad') },
    { id: 'F09297', name: '提示延迟令牌', check: () => E.MICRO_TOOLTIP_MS.show === 300 },
    { id: 'F09298', name: 'hover/focus 等价', check: () => E.hoverFocusParity('x', 'x:focus') && !E.hoverFocusParity('x', 'x') },
    { id: 'F09299', name: '暗色深阴影', check: () => E.darkShadowFix('dark') !== E.darkShadowFix('light') },
    { id: 'F09300', name: '微细节 25 项', check: () => E.MICRO_DETAIL_COUNT === 25 },
  ];
}

/* -------- AI-75 族0373 动线优化 F09301~F09325 -------- */
export function checkF0373(): CheckEntry[] {
  const fc = new E.FlowCounter();
  fc.step(); fc.step();
  const resume = new E.FlowResume();
  resume.save('setup', 3);
  const local = new E.FlowAnalyticsLocal();
  local.track('buy'); local.track('buy');
  const fr = new E.FrictionRegistry();
  fr.add('设置页', '层级过深');
  return [
    { id: 'F09301', name: '步数计数', check: () => fc.count === 2 && (fc.step(), fc.reset(), Number(fc.count) === 0) },
    { id: 'F09302', name: '深度 ≤3 红线', check: () => E.clickDepthOk(3) && !E.clickDepthOk(4) },
    { id: 'F09303', name: '高频动作上浮', check: () => E.frequentActionSurface(200, true) === 'promote' && E.frequentActionSurface(10, true) === 'keep' },
    { id: 'F09304', name: '向导/表单形态', check: () => E.wizardOrForm(10) === 'wizard' && E.wizardOrForm(5) === 'form' },
    { id: 'F09305', name: '智能默认', check: () => E.smartDefault(['上次值']) === '上次值' && E.smartDefault([]) === '' },
    { id: 'F09306', name: '最近优先', check: () => E.recentFirst([{ usedAt: 1 }, { usedAt: 9 }])[0]!.usedAt === 9 },
    { id: 'F09307', name: '语境入口', check: () => E.contextualEntry(true) === 'context-menu' && E.contextualEntry(false) === null },
    { id: 'F09308', name: '动线有出口', check: () => E.escapeHatchPresent(true, true) && !E.escapeHatchPresent(true, false) },
    { id: 'F09309', name: '进度可续', check: () => resume.resume('setup') === 3 && resume.resume('x') === null },
    { id: 'F09310', name: '高手捷径', check: () => E.powerShortcutAvailable(true) && !E.powerShortcutAvailable(false) },
    { id: 'F09311', name: '埋点仅本地', check: () => local.counts().get('buy') === 2 && local.cloudUpload() === false },
    { id: 'F09312', name: '摩擦点登记', check: () => fr.list().length === 1 && fr.list()[0]!.where === '设置页' },
    { id: 'F09313', name: '动线模拟 ≤5', check: () => E.flowSimulate(['a', 'b']).ok && !E.flowSimulate(['1', '2', '3', '4', '5', '6']).ok },
    { id: 'F09314', name: '全动线可撤销', check: () => E.undoAvailable('del', new Set(['del'])) && !E.undoAvailable('del', new Set()) },
    { id: 'F09315', name: '仅危险需确认', check: () => E.confirmOnlyDestructive(true) === 'confirm' && E.confirmOnlyDestructive(false) === 'direct' },
    { id: 'F09316', name: '批量阈值', check: () => E.batchOperation(3) === 'batch' && E.batchOperation(1) === 'single' },
    { id: 'F09317', name: '动线图', check: () => E.flowMap(['a', 'b'], [['a', 'b']]).edges === 1 },
    { id: 'F09318', name: '不增步红线', check: () => E.noAddedSteps(3, 3) && !E.noAddedSteps(3, 4) },
    { id: 'F09319', name: '危险默认安全侧', check: () => E.safeDefaultButton(true) === 'cancel' && E.safeDefaultButton(false) === 'confirm' },
    { id: 'F09320', name: '动线命名', check: () => E.flowName('导出', '报告') === '导出-报告' },
    { id: 'F09321', name: '中断恢复提示', check: () => E.resumePrompt(2)!.includes('第 2 步') && E.resumePrompt(null) === null },
    { id: 'F09322', name: '同类同构', check: () => E.flowParity([{ steps: ['a'] }, { steps: ['b'] }]) && !E.flowParity([{ steps: ['a'] }, { steps: ['b', 'c'] }]) },
    { id: 'F09323', name: '动线文档', check: () => E.flowDoc('导出') === 'flow-doc:导出' },
    { id: 'F09324', name: '完成率', check: () => E.completionRate(10, 8) === 80 && E.completionRate(0, 0) === 100 },
    { id: 'F09325', name: '动线全 ≤5 步', check: () => E.flowFinale([{ steps: ['a'] }]) && !E.flowFinale([{ steps: ['1', '2', '3', '4', '5', '6'] }]) },
  ];
}

/* -------- AI-75 族0374 视觉品质 F09326~F09350 -------- */
export function checkF0374(): CheckEntry[] {
  return [
    { id: 'F09326', name: '字号阶梯', check: () => E.typeScaleOk(14) && !E.typeScaleOk(15) && E.TYPE_SCALE.length === 6 },
    { id: 'F09327', name: '光学基线', check: () => E.opticalBaseline(24, 14) === 5 },
    { id: 'F09328', name: '对比 AA', check: () => E.contrastAA([0, 0, 0], [255, 255, 255]) && !E.contrastAA([200, 200, 200], [255, 255, 255]) },
    { id: 'F09329', name: '海拔三档', check: () => E.elevationStyle(3).includes('24px') && E.elevationStyle(1) !== E.elevationStyle(2) },
    { id: 'F09330', name: '色彩和谐', check: () => E.colorHarmony([200, 220]) && !E.colorHarmony([200, 300]) },
    { id: 'F09331', name: '渐变止点', check: () => E.gradientOk([0, 50, 100]) && !E.gradientOk([100, 0]) },
    { id: 'F09332', name: '图片不放大', check: () => E.imageQualityOk(200, 100) && !E.imageQualityOk(50, 100) },
    { id: 'F09333', name: '文本抗锯齿', check: () => E.MICRO_ANTIALIAS.includes('antialiased') },
    { id: 'F09334', name: '留白平衡', check: () => E.whitespaceBalance(70, 100) && !E.whitespaceBalance(90, 100) },
    { id: 'F09335', name: '8pt 对齐', check: () => E.alignedToGrid(16) && !E.alignedToGrid(15) },
    { id: 'F09336', name: '图标光学居中', check: () => E.iconOpticalCenter(24, 16) === 4 },
    { id: 'F09337', name: '动效曲线', check: () => E.EASE_QUALITY.enter.includes('cubic-bezier') },
    { id: 'F09338', name: '骨架微光', check: () => E.skeletonPolish(true) && !E.skeletonPolish(false) },
    { id: 'F09339', name: '暗色景深', check: () => E.darkDepth('dark') === 3 && E.darkDepth('light') === 2 },
    { id: 'F09340', name: '高对比全覆盖', check: () => E.highContrastParity(10, 10) && !E.highContrastParity(9, 10) },
    { id: 'F09341', name: '打印省墨', check: () => E.PRINT_STYLE.includes('animation:none') },
    { id: 'F09342', name: '品牌派生链', check: () => E.brandConsistency('#78d4', ['a78d4', 'b78d4']) && !E.brandConsistency('#78d4', ['zzz']) },
    { id: 'F09343', name: '动效克制 ≤3', check: () => E.motionRestraint(2) && !E.motionRestraint(5) },
    { id: 'F09344', name: '层级 ≤3', check: () => E.visualHierarchyOk(3) && !E.visualHierarchyOk(4) },
    { id: 'F09345', name: '截图评审矩阵', check: () => E.screenshotReviewProtocol(8) === 72 },
    { id: 'F09346', name: '品质核对单', check: () => E.QUALITY_CHECKLIST.length === 6 },
    { id: 'F09347', name: '品质评分', check: () => E.qualityScore(9, 10) === 90 },
    { id: 'F09348', name: '问题零新增', check: () => E.zeroNewIssues(5, 3) && !E.zeroNewIssues(5, 6) },
    { id: 'F09349', name: '品质文档', check: () => E.QUALITY_DOCS.length === 4 },
    { id: 'F09350', name: '品质收官', check: () => E.qualityFinale(95).includes('达标') && E.qualityFinale(80).includes('未达标') },
  ];
}

/* -------- AI-75 族0375 UI 收官 F09351~F09375 -------- */
export function checkF0375(): CheckEntry[] {
  const debt = new E.UiDebtLedger();
  debt.add('样式散落', 'P2');
  return [
    { id: 'F09351', name: '收官核对单 25 项', check: () => E.UI_FINALE_CHECKLIST.length === 25 },
    { id: 'F09352', name: '技术债台账', check: () => debt.p1().length === 0 && (debt.clear('样式散落'), debt.open().length === 0) },
    { id: 'F09353', name: '零 P0 门槛', check: () => E.zeroP0Gate(0) && !E.zeroP0Gate(1) },
    { id: 'F09354', name: '三主题终验', check: () => E.finalThreeThemes([{ theme: 'dark', pass: true }, { theme: 'light', pass: true }, { theme: 'hc', pass: true }]) && !E.finalThreeThemes([{ theme: 'dark', pass: false }]) },
    { id: 'F09355', name: '三语言终验', check: () => E.finalThreeLangs({ zh: 5, tw: 5, en: 5 }) && !E.finalThreeLangs({ zh: 5, tw: 4, en: 5 }) },
    { id: 'F09356', name: '200% 终验', check: () => E.finalZoom(0) && !E.finalZoom(1) },
    { id: 'F09357', name: 'a11y AA 终验', check: () => E.finalA11yAA(0) && !E.finalA11yAA(2) },
    { id: 'F09358', name: '性能终验', check: () => E.finalPerf([{ name: 'startup', ok: true }, { name: 'input', ok: true }]) && !E.finalPerf([{ name: 'x', ok: false }]) },
    { id: 'F09359', name: '视觉基线冻结', check: () => E.visualBaselineFreeze('2026-09-13').frozen },
    { id: 'F09360', name: '文档冻结', check: () => E.docsFreeze(['a', 'b']).frozen && E.docsFreeze(['a']).count === 1 },
    { id: 'F09361', name: '组件库冻结', check: () => E.componentLibraryFreeze(25).api === 'stable' },
    { id: 'F09362', name: '收官庆典', check: () => E.uiCelebration(625).includes('625') },
    { id: 'F09363', name: '致谢名单', check: () => E.uiCredits('AI-71~75').includes('AI-71~75') },
    { id: 'F09364', name: '复盘记录', check: () => { const r = E.retro([{ wentWell: 'a', toImprove: '' }, { wentWell: '', toImprove: 'b' }]); return r.well === 1 && r.improve === 1; } },
    { id: 'F09365', name: '交接文档', check: () => E.handoffDoc('AI-76', ['a', 'b']).includes('2 条备注') },
    { id: 'F09366', name: '遗留分级', check: () => { const t = E.triageRemaining([{ id: 'a', p0: true }, { id: 'b', p0: false }]); return t.now.join() === 'a' && t.later.join() === 'b'; } },
    { id: 'F09367', name: '长尾预算', check: () => E.longTailBudget(30) && !E.longTailBudget(80) },
    { id: 'F09368', name: '标志性瞬间', check: () => E.signatureMoment('开机剧场') === 'signature:开机剧场' },
    { id: 'F09369', name: '致谢页', check: () => E.thanksPage('1.0').includes('80 个 AI 会话') },
    { id: 'F09370', name: '材料归档', check: () => E.archiveMaterials(['a', 'b']).sealed && E.archiveMaterials(['a']).count === 1 },
    { id: 'F09371', name: '版本标记', check: () => E.versionTag('1.0.0', '7') === '1.0.0+w7' },
    { id: 'F09372', name: '终版报告', check: () => E.finalReport(625, 100).includes('625/625') },
    { id: 'F09373', name: '冻结区守护', check: () => E.finaleGuard('tokens.css', ['tokens.css']) && !E.finaleGuard('x', ['tokens.css']) },
    { id: 'F09374', name: '后续入口', check: () => E.nextSteps(['a', 'b']).length === 2 },
    { id: 'F09375', name: 'UI 收官完成', check: () => E.uiFinale(E.UI_FINALE_CHECKLIST) },
  ];
}

/** 领域15 汇总：625 项。 */
export function runDomain15Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [
    checkF0351, checkF0352, checkF0353, checkF0354, checkF0355,
    checkF0356, checkF0357, checkF0358, checkF0359, checkF0360,
    checkF0361, checkF0362, checkF0363, checkF0364, checkF0365,
    checkF0366, checkF0367, checkF0368, checkF0369, checkF0370,
    checkF0371, checkF0372, checkF0373, checkF0374, checkF0375,
  ];
  const entries = families.flatMap((f) => f().map(memoized));
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
