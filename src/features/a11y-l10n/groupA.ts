// AURORA-10000: AI-66 批次（族0326~0330 · 视觉/听觉/运动/认知/语音无障碍），勿删。

import { clamp, type FeatureSlot } from './types';

/* ============ 族0326 视觉无障碍（F08126~F08150） ============ */

export type MagnifierMode = 'fullscreen' | 'lens' | 'docked';
export type MagnifierFollow = 'cursor' | 'keyboard' | 'focus';

/** F08126 放大镜三形态与 F08127 倍率 2~16x、F08128 跟随目标。 */
export class Magnifier {
  mode: MagnifierMode = 'lens';
  zoom = 2;
  follow: MagnifierFollow = 'cursor';
  setMode(m: MagnifierMode): this { this.mode = m; return this; }
  setZoom(z: number): number { this.zoom = clamp(Math.round(z), 2, 16); return this.zoom; }
  setFollow(f: MagnifierFollow): this { this.follow = f; return this; }
}

/** F08129 色盲滤镜：三种模式 3x3 矩阵作用于 RGB。 */
export type CvdMode = 'protanopia' | 'deuteranopia' | 'tritanopia';
export const CVD_MATRICES: Record<CvdMode, number[]> = {
  protanopia: [0.567, 0.433, 0, 0.558, 0.442, 0, 0, 0.242, 0.758],
  deuteranopia: [0.625, 0.375, 0, 0.7, 0.3, 0, 0, 0.3, 0.7],
  tritanopia: [0.95, 0.05, 0, 0, 0.433, 0.567, 0, 0.475, 0.525],
};
export function applyCvdFilter(mode: CvdMode, rgb: [number, number, number]): [number, number, number] {
  const m = CVD_MATRICES[mode];
  return [0, 1, 2].map((i) => clamp(Math.round(m[i]! * rgb[0]! + m[i + 3]! * rgb[1]! + m[i + 6]! * rgb[2]!), 0, 255)) as [number, number, number];
}

/** F08130 对比主题令牌键。 */
export const HIGH_CONTRAST_TOKENS = ['bg', 'fg', 'accent', 'focus', 'border'] as const;

/** F08131 高对比焦点环规格。 */
export function focusRing(thickness = 2, contrast = 4.5): { width: number; minContrast: number } {
  return { width: Math.max(2, thickness), minContrast: contrast };
}

/** F08132 文本光标加粗：1~6px。 */
export function caretWidth(w: number): number { return clamp(w, 1, 6); }

/** F08133 闪烁限制：1s 内闪烁不超过 3 次（光敏安全基线）。 */
export function flashAllowed(flashesPerSecond: number): boolean { return flashesPerSecond <= 3; }

/** F08134 减动效：全局动效时长与位移归零化因子。 */
export function reduceMotion(enabled: boolean, durMs: number, distPx: number): { durMs: number; distPx: number } {
  return enabled ? { durMs: 0, distPx: 0 } : { durMs, distPx };
}

/** F08135 光敏保护：检测危险闪烁序列（3 次内亮暗切换超过阈值则拦下）。 */
export function photosensitivityGuard(lumaSeq: number[], threshold = 0.35): boolean {
  let switches = 0;
  for (let i = 1; i < lumaSeq.length; i++) {
    if (Math.abs(lumaSeq[i]! - lumaSeq[i - 1]!) > threshold) switches++;
    if (switches >= 3) return false;
  }
  return true;
}

/** F08136 鼠标加大：指针尺寸 16~96。 */
export function pointerSize(px: number): number { return clamp(px, 16, 96); }

/** F08137 指针轨迹：保留最近 n 个点。 */
export function pointerTrail(history: [number, number][], n: number): [number, number][] {
  return history.slice(-clamp(n, 0, 32));
}

/** F08138 点击显示：点击位置涟漪半径。 */
export function clickRipple(strength: number): number { return 8 + clamp(strength, 0, 1) * 24; }

/** F08139 键盘焦点高亮：焦点元素描边令牌。 */
export const FOCUS_HIGHLIGHT_TOKEN = '--a11y-focus-highlight';

/** F08140 朗读位置：把元素几何转为可播报描述。 */
export function announcePosition(rect: { x: number; y: number }, viewport: { w: number; h: number }): string {
  const hz = rect.x < viewport.w / 3 ? '左侧' : rect.x > (viewport.w * 2) / 3 ? '右侧' : '中部';
  const vt = rect.y < viewport.h / 3 ? '上方' : rect.y > (viewport.h * 2) / 3 ? '下方' : '中间';
  return `${vt}${hz}`;
}

/** F08141 放大热键表。 */
export const MAGNIFIER_HOTKEYS: Record<string, string> = { 'win+plus': 'zoomIn', 'win+minus': 'zoomOut', 'win+esc': 'exit' };

/** F08142 放大配置文件：命名方案保存/切换。 */
export class MagnifierProfiles {
  private store = new Map<string, { mode: MagnifierMode; zoom: number }>();
  save(name: string, mode: MagnifierMode, zoom: number): void { this.store.set(name, { mode, zoom }); }
  load(name: string): { mode: MagnifierMode; zoom: number } | null { return this.store.get(name) ?? null; }
  get size(): number { return this.store.size; }
}

/** F08143 读屏联动：放大镜跟随读屏朗读位置。 */
export function linkScreenReader(active: boolean, rect: { x: number; y: number } | null): { x: number; y: number } | null {
  return active && rect ? rect : null;
}

/** F08144 低视力指南五步。 */
export const LOW_VISION_GUIDE = ['评估视力需求', '选择放大形态', '调整对比与色彩', '优化文本排版', '绑定热键与档案'];

/** F08145 全局大字：根字号缩放 100%~200%。 */
export function globalFontScale(pct: number): number { return clamp(pct, 100, 200); }

/** F08146 字重可调：400~900。 */
export function fontWeight(w: number): number { return clamp(w, 400, 900); }

/** F08147 行距可调：1.0~2.5。 */
export function lineHeight(v: number): number { return clamp(Math.round(v * 10) / 10, 1, 2.5); }

/** F08148 字距可调：0~0.3em。 */
export function letterSpacing(v: number): number { return clamp(Math.round(v * 100) / 100, 0, 0.3); }

/** F08149 视障教学步骤。 */
export const VISION_TUTORIAL = ['开启放大镜', '选择跟随模式', '应用色盲滤镜', '调整全局大字与行距', '保存配置文件'];

/** F08150 视障收官清单。 */
export const VISION_FINALE = ['25 项自检', '读屏语义完整', '三主题高对比过审', '热键无冲突', '教学文档齐备'];

/* ============ 族0327 听觉无障碍（F08151~F08175） ============ */

export interface Caption {
  start: number;
  end: number;
  text: string;
}

/** F08151~F08153 环境字幕引擎：字幕生成、样式、位置。 */
export class CaptionEngine {
  enabled = false;
  fontSize = 18;
  background = 'rgba(0,0,0,0.75)';
  position: 'top' | 'bottom' = 'bottom';
  lines: Caption[] = [];
  toggle(on: boolean): boolean { this.enabled = on; return this.enabled; }
  push(start: number, end: number, text: string): Caption {
    const c = { start, end, text };
    this.lines.push(c);
    return c;
  }
  at(t: number): Caption | null {
    for (const c of this.lines) if (t >= c.start && t <= c.end) return c;
    return null;
  }
}

/** F08154/F08155 本地实时转写与对话转写会话。 */
export class TranscriptionSession {
  readonly local = true;
  mode: 'live' | 'dialogue' = 'live';
  utterances: { speaker: string; text: string }[] = [];
  add(speaker: string, text: string): number {
    this.utterances.push({ speaker, text });
    return this.utterances.length;
  }
  get text(): string { return this.utterances.map((u) => `${u.speaker}:${u.text}`).join('\n'); }
}

/** F08156 转写导出 SRT。 */
export function exportSrt(captions: Caption[]): string {
  const fmt = (t: number) => {
    const ms = Math.round((t % 1) * 1000);
    const s = Math.floor(t) % 60;
    const m = Math.floor(t / 60) % 60;
    const h = Math.floor(t / 3600);
    const p = (n: number, w = 2) => String(n).padStart(w, '0');
    return `${p(h)}:${p(m)}:${p(s)},${p(ms, 3)}`;
  };
  return captions.map((c, i) => `${i + 1}\n${fmt(c.start)} --> ${fmt(c.end)}\n${c.text}\n`).join('\n');
}

/** F08157 视觉铃声：响铃时屏幕闪光脉冲次数。 */
export function visualBell(): number[] { return [120, 120, 240]; }

/** F08158 声音通知转视觉：把声音事件映射为视觉标签。 */
export function soundToVisual(kind: 'chime' | 'alert' | 'error'): string {
  return kind === 'error' ? '‼ 错误' : kind === 'alert' ? '⚠ 提醒' : '♪ 提示';
}

/** F08159 振动通知：短促/长振动模式。 */
export function vibrationPattern(kind: 'short' | 'long'): number[] {
  return kind === 'short' ? [60, 40, 60] : [300];
}

/** F08160 单声道合并：左右声道平均。 */
export function monoDownmix(l: number, r: number): number { return (l + r) / 2; }

/** F08161 听力辅助增益：可听频段提升 0~12dB。 */
export function hearingBoost(db: number): number { return clamp(db, 0, 12); }

/** F08162 助听兼容位 / F08163 耳蜗映射位（§15 预留）。 */
export const HEARING_SLOTS: FeatureSlot[] = [
  { id: 'F08162', name: '助听兼容', reserved: true, enabled: false },
  { id: 'F08163', name: '耳蜗映射', reserved: true, enabled: false },
  { id: 'F08168', name: '会议字幕', reserved: true, enabled: false },
];

/** F08164 音量可视化：各声源音量条。 */
export function volumeMeters(sources: Record<string, number>): [string, number][] {
  return Object.entries(sources).map(([k, v]) => [k, clamp(v, 0, 1)] as [string, number]);
}

/** F08165 方向指示：按双耳电平差指示声源方向。 */
export function soundDirection(l: number, r: number): '左' | '右' | '正前' {
  const d = l - r;
  if (d > 0.1) return '左';
  if (d < -0.1) return '右';
  return '正前';
}

/** F08166 音量平滑：突发声压限幅。 */
export function volumeSmooth(prev: number, next: number, maxStep = 0.2): number {
  return clamp(next, prev - maxStep, prev + maxStep);
}

/** F08167 语音消息转文字（本地转写包装）。 */
export function speechToText(session: TranscriptionSession): string { return session.text; }

/** F08169 听障教学步骤。 */
export const HEARING_TUTORIAL = ['开启环境字幕', '选择字幕样式与位置', '配置视觉铃声', '设置振动与单声道', '导出转写归档'];

/** F08170 听障审计项。 */
export const HEARING_AUDIT = ['字幕覆盖', '字幕可读对比', '视觉替代存在', '音量安全上限', '预留位未误启'];

/** F08171 听障测试用例。 */
export const HEARING_TESTS = ['字幕命中', 'SRT 导出', '振动模式', '单声道合并', '方向指示'];

/** F08172 听障文档页。 */
export const HEARING_DOCS = ['字幕指南', '转写指南', '振动与闪光指南'];

/** F08173 听障回归清单。 */
export const HEARING_REGRESSION = ['字幕回归', '铃声回归', '通知转视觉回归'];

/** F08174 听障彩蛋（低频 + 总控）。 */
export function hearingEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 97 === 0 ? '手语小课堂：你好 👋' : null;
}

/** F08175 听障收官清单。 */
export const HEARING_FINALE = ['25 项自检', '预留位冻结', '审计零违规', '回归通过', '文档齐备'];

/* ============ 族0328 运动无障碍（F08176~F08200） ============ */

/** F08176~F08180 开关扫描：全系统扫描状态机（单/双开关、速度、自动扫描）。 */
export class SwitchScanner {
  items: string[] = [];
  index = 0;
  mode: 'single' | 'dual' = 'single';
  intervalMs = 1000;
  auto = true;
  paused = false;
  load(items: string[]): void { this.items = items; this.index = 0; }
  step(): string | null {
    if (this.items.length === 0 || this.paused) return null;
    const cur = this.items[this.index]!;
    if (this.auto) this.index = (this.index + 1) % this.items.length;
    return cur;
  }
  select(): string | null { return this.items[this.index] ?? null; }
  press(): string | null { return this.mode === 'single' ? this.select() : this.step(); }
}

/** F08181 按键驻留（dwell）：悬停驻留达阈值触发。 */
export class DwellClick {
  thresholdMs: number;
  private heldSince: number | null = null;
  constructor(thresholdMs = 800) { this.thresholdMs = thresholdMs; }
  move(atMs: number): 'idle' | 'holding' | 'fired' {
    if (this.heldSince === null) { this.heldSince = atMs; return 'holding'; }
    return atMs - this.heldSince >= this.thresholdMs ? (this.heldSince = null, 'fired') : 'holding';
  }
  leave(): void { this.heldSince = null; }
}

/** F08182 眼动点击位 / F08186 头部控制位（§15 预留）。 */
export const MOTION_SLOTS: FeatureSlot[] = [
  { id: 'F08182', name: '眼动点击', reserved: true, enabled: false },
  { id: 'F08186', name: '头部控制', reserved: true, enabled: false },
];

/** F08183/F08184 语音控制全集与自定义命令。 */
export class VoiceCommandSet {
  builtin: Record<string, string> = { '打开设置': 'settings.open', '关闭窗口': 'window.close', '最小化': 'window.minimize' };
  custom: Record<string, string> = {};
  add(phrase: string, action: string): boolean {
    if (phrase in this.builtin || phrase in this.custom) return false;
    this.custom[phrase] = action;
    return true;
  }
  match(phrase: string): string | null { return this.builtin[phrase] ?? this.custom[phrase] ?? null; }
}

/** F08185 编号覆盖：为界面可点元素编号。 */
export function numberOverlay(labels: string[]): [number, string][] {
  return labels.map((l, i) => [i + 1, l] as [number, string]);
}

/** F08187 摇杆轴映射：死区处理。 */
export function joystickAxis(raw: number, deadzone = 0.15): number {
  return Math.abs(raw) < deadzone ? 0 : Math.sign(raw) * ((Math.abs(raw) - deadzone) / (1 - deadzone));
}

/** F08188 脚踏键映射：踏板→动作。 */
export const FOOT_PEDAL_MAP: Record<'left' | 'middle' | 'right', string> = { left: 'step', middle: 'select', right: 'cancel' };

/** F08189 粘滞键：修饰键粘滞状态机。 */
export class StickyKeys {
  private stuck = new Set<string>();
  tap(mod: string): 'stuck' | 'locked' | 'released' {
    if (this.stuck.has(mod + '!')) { this.stuck.delete(mod + '!'); this.stuck.delete(mod); return 'released'; }
    if (this.stuck.has(mod)) { this.stuck.delete(mod); this.stuck.add(mod + '!'); return 'locked'; }
    this.stuck.add(mod);
    return 'stuck';
  }
  isStuck(mod: string): boolean { return this.stuck.has(mod) || this.stuck.has(mod + '!'); }
  isLocked(mod: string): boolean { return this.stuck.has(mod + '!'); }
  reset(): void { this.stuck.clear(); }
}

/** F08190 慢速键：按键需停留时长生效。 */
export class SlowKeys {
  constructor(readonly holdMs: number) {}
  accept(downAt: number, upAt: number): boolean { return upAt - downAt >= this.holdMs; }
}

/** F08191 重复键控制：延迟与速率。 */
export function keyRepeat(delayMs: number, ratePerSec: number): { delayMs: number; ratePerSec: number } {
  return { delayMs: clamp(delayMs, 250, 2000), ratePerSec: clamp(ratePerSec, 2, 30) };
}

/** F08192 双击间隔加大：200~1000ms。 */
export function doubleClickInterval(ms: number): number { return clamp(ms, 200, 1000); }

/** F08193 键盘拖拽：空间键模拟拖拽会话。 */
export function keyboardDrag(start: { x: number; y: number }, delta: { dx: number; dy: number }): { x: number; y: number } {
  return { x: start.x + delta.dx, y: start.y + delta.dy };
}

/** F08194 长按替代：把长按动作替换为菜单项。 */
export const LONG_PRESS_ALT = 'menu.longPressAlternative';

/** F08195 误触保护：抖动窗口内多次点击仅计一次。 */
export class MistouchGuard {
  windowMs: number;
  private last = -Infinity;
  count = 0;
  constructor(windowMs = 250) { this.windowMs = windowMs; }
  tap(atMs: number): number {
    if (atMs - this.last < this.windowMs) { this.count = Math.min(this.count + 1, 2); }
    else { this.count = 1; }
    this.last = atMs;
    return this.count;
  }
  get suppressed(): boolean { return this.count > 1; }
}

/** F08196 重复禁用：自动重复关闭。 */
export function disableAutoRepeat(enabled: boolean): { autoRepeat: boolean } { return { autoRepeat: !enabled }; }

/** F08197 运动教学步骤。 */
export const MOTION_TUTORIAL = ['选择输入方式', '配置扫描速度', '练习粘滞与慢速', '绑定脚踏与摇杆', '保存动作档案'];

/** F08198 运动审计项。 */
export const MOTION_AUDIT = ['全部目标键盘可达', '扫描可中止', '预留位未误启', '热键无冲突', '误触保护默认开'];

/** F08199 运动测试用例。 */
export const MOTION_TESTS = ['扫描选择', '驻留触发', '粘滞锁定', '慢速拦截', '拖拽位移'];

/** F08200 运动收官清单。 */
export const MOTION_FINALE = ['25 项自检', '预留位冻结', '审计零违规', '测试通过', '教学齐备'];

/* ============ 族0329 认知无障碍（F08201~F08225） ============ */

/** F08201/F08202 简化模式与专注：白名单决定可见选项。 */
export class CognitiveProfile {
  simplified = false;
  whitelist: string[] = [];
  timeouts = { defaultMs: 30000, extendedMs: 90000 };
  setSimplified(on: boolean, keep: string[]): string[] {
    this.simplified = on;
    this.whitelist = on ? keep : [];
    return this.whitelist;
  }
  visibleOptions(all: string[]): string[] { return this.simplified ? all.filter((o) => this.whitelist.includes(o)) : all; }
  /** F08217 时间放宽：超时延长 3x。 */
  timeoutMs(ms: number): number { return this.simplified ? ms * 3 : ms; }
}

/** F08203 分步引导：一步一步推进。 */
export class StepGuide {
  steps: string[];
  at = 0;
  constructor(steps: string[]) { this.steps = steps; }
  next(): string | null { return this.at < this.steps.length ? this.steps[this.at++]! : null; }
  get done(): boolean { return this.at >= this.steps.length; }
}

/** F08204 重复提示：同一操作第 n 次仍给提示。 */
export function repeatHint(count: number): string {
  return count <= 1 ? '提示：确认后再继续' : `提醒（第 ${count} 次）：确认后再继续`;
}

/** F08205 记忆辅助：回放「刚才做了什么」。 */
export class MemoryAid {
  log: string[] = [];
  record(action: string): void { this.log.push(action); }
  recap(n = 3): string[] { return this.log.slice(-n); }
}

/** F08206~F08208 阅读辅助：行聚焦、标尺、行高亮。 */
export function readingLineFocus(lines: string[], index: number): string | null { return lines[index] ?? null; }
export function readingRuler(docHeight: number, rulerY: number, band = 48): { top: number; bottom: number } {
  return { top: clamp(rulerY - band / 2, 0, docHeight), bottom: clamp(rulerY + band / 2, 0, docHeight) };
}
export const READING_HIGHLIGHT_TOKEN = '--a11y-reading-highlight';

/** F08209 音节划分位（§15 预留）。 */
export const SYLLABLE_SLOT: FeatureSlot = { id: 'F08209', name: '音节划分', reserved: true, enabled: false };

/** F08210 易读字体档。 */
export const EASY_READ_FONTS = ['Sans 优先', '圆润体', '等距笔形'] as const;

/** F08211 大间隔：元素间距放宽因子。 */
export function relaxedSpacing(base: number): number { return base * 1.5; }

/** F08212 减少弹窗：非关键弹窗聚合为待办条。 */
export function dialogPolicy(critical: boolean, pending: number): 'show' | 'batch' {
  return critical || pending < 3 ? 'show' : 'batch';
}

/** F08213 图文双语：文本配图标签对。 */
export function pictureText(text: string, icon: string): { text: string; icon: string } { return { text, icon }; }

/** F08214 平静模式：降饱和 + 降动效。 */
export function calmMode(on: boolean): { saturation: number; motionScale: number } {
  return on ? { saturation: 0.6, motionScale: 0.3 } : { saturation: 1, motionScale: 1 };
}

/** F08215 可预测导航：页面布局签名一致性检查。 */
export function predictableNav(sigs: string[]): boolean { return new Set(sigs).size === 1; }

/** F08216 撤销友好：危险操作前生成可撤销标记。 */
export function undoFriendly(action: string): { action: string; confirm: boolean; undoable: boolean } {
  return { action, confirm: true, undoable: true };
}

/** F08218 认知教学步骤。 */
export const COGNITIVE_TUTORIAL = ['开启简化模式', '选择保留选项', '启用分步引导', '调整阅读辅助', '设置时间放宽'];

/** F08219 认知审计项。 */
export const COGNITIVE_AUDIT = ['白名单生效', '无死胡同', '超时可延长', '预留位未误启', '提示可重复'];

/** F08220 认知测试用例。 */
export const COGNITIVE_TESTS = ['简化过滤', '分步完成', '回放记忆', '标尺范围', '弹窗聚合'];

/** F08221 认知回归清单。 */
export const COGNITIVE_REGRESSION = ['简化模式回归', '引导回归', '平静模式回归'];

/** F08222 认知文档页。 */
export const COGNITIVE_DOCS = ['简化模式指南', '阅读辅助指南', '时间放宽说明'];

/** F08223 认知彩蛋（低频 + 总控）。 */
export function cognitiveEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 89 === 0 ? '今天也慢慢来 🐢' : null;
}

/** F08224 认知收官清单。 */
export const COGNITIVE_FINALE = ['25 项自检', '预留位冻结', '审计零违规', '回归通过', '文档齐备'];

/** F08225 认知致谢名单。 */
export const COGNITIVE_THANKS = ['认知障碍社群', '特教老师顾问团', '家属陪护志愿者'];

/* ============ 族0330 语音无障碍（F08226~F08250） ============ */

/** F08226~F08233 语音控制中枢：全集、导航、听写、编辑、编号、自定义、反馈、免手。 */
export class VoiceControl {
  readonly local = true;
  enabled = false;
  handsFree = false;
  speed = 1;
  dictationLog: string[] = [];
  overlay: [number, string][] = [];
  custom: Record<string, string> = {};
  command(phrase: string, targets: string[]): string | null {
    if (phrase.startsWith('跳到 ')) {
      const t = phrase.slice(3).trim();
      return targets.includes(t) ? `focus:${t}` : null;
    }
    if (phrase.startsWith('选中 ')) return `select:${phrase.slice(3).trim()}`;
    return { ...BASE_VOICE_COMMANDS, ...this.custom }[phrase] ?? null;
  }
  dictate(text: string): number { this.dictationLog.push(text); return this.dictationLog.length; }
  setOverlay(labels: string[]): void { this.overlay = labels.map((l, i) => [i + 1, l] as [number, string]); }
  feedback(action: string): string { return `已${action}`; }
}

/** F08230 编号覆盖基础命令。 */
export const BASE_VOICE_COMMANDS: Record<string, string> = {
  '打开设置': 'settings.open', '返回': 'nav.back', '撤销': 'edit.undo', '关闭窗口': 'window.close',
};

/** F08231 自定义命令注册。 */
export function addVoiceCommand(store: Record<string, string>, phrase: string, action: string): boolean {
  if (phrase in store) return false;
  store[phrase] = action;
  return true;
}

/** F08234 语速 0.5~2.0。 */
export function voiceSpeed(v: number): number { return clamp(v, 0.5, 2); }

/** F08235 口音适应：按纠错样本加权。 */
export function accentAdapt(profile: Record<string, number>, heard: string, truth: string): Record<string, number> {
  const p = { ...profile };
  p[heard] = (p[heard] ?? 0) + 1;
  if (heard !== truth) p[truth] = (p[truth] ?? 0) + 0.5;
  return p;
}

/** F08236 离线标志：仅本地模型可用。 */
export function voiceOffline(models: { name: string; onDevice: boolean }[]): string[] {
  return models.filter((m) => m.onDevice).map((m) => m.name);
}

/** F08237 本地承诺：语音数据不出站。 */
export const VOICE_PRIVACY = { upload: false, telemetry: false, retention: 'memory-only' } as const;

/** F08238 语音教学步骤。 */
export const VOICE_TUTORIAL = ['开启语音控制', '试说基础命令', '练习编号选择', '注册自定义命令', '设置免手模式'];

/** F08239 语音审计项。 */
export const VOICE_AUDIT = ['命令全部可达', '误识可撤销', '数据不出站', '预留位未误启', '反馈及时'];

/** F08240 语音测试用例。 */
export const VOICE_TESTS = ['导航命令', '听写追加', '自定义注册', '语速钳位', '免手模式'];

/** F08241 语音回归清单。 */
export const VOICE_REGRESSION = ['命令回归', '听写回归', '反馈回归'];

/** F08242 语音文档页。 */
export const VOICE_DOCS = ['命令总表', '自定义指南', '隐私说明'];

/** F08243 语音彩蛋（低频 + 总控）。 */
export function voiceEgg(unlocked: boolean, phrase: string): string | null {
  return unlocked && phrase === '芝麻开门' ? '山门缓缓打开 🚪' : null;
}

/** F08244 眼动组合位（§15 预留）/ F08245 语音+扫描组合（可用）。 */
export const VOICE_SLOTS: FeatureSlot[] = [
  { id: 'F08244', name: '眼动组合', reserved: true, enabled: false },
  { id: 'F08250', name: '语音实验位', reserved: true, enabled: false },
];
export function voiceScanCombo(voiceOk: boolean, scanOk: boolean): boolean { return voiceOk && scanOk; }

/** F08246 语音 API 冻结接口。 */
export interface VoiceControlApi {
  command(phrase: string): string | null;
  dictate(text: string): void;
  setSpeed(v: number): void;
}
export const VOICE_API_VERSION = '1.0';

/** F08247 反馈音：操作确认短音。 */
export function voiceFeedbackTone(ok: boolean): string { return ok ? 'tick' : 'thud'; }

/** F08248 语音收官清单。 */
export const VOICE_FINALE = ['25 项自检', '预留位冻结', '审计零违规', '回归通过', 'API 冻结'];

/** F08249 语音致谢名单。 */
export const VOICE_THANKS = ['语音障碍社群', '方言口音志愿者', '内测家庭'];
