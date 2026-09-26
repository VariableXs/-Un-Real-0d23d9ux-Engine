/**
 * 文档与输入六件（AI-U1 · F413 PrintScreen / F425 Aero Shake /
 * F426 文档键位保存族 / F427 Ctrl+X/C/V / F428 Ctrl+P / F450 粘滞键筛选键）。
 *
 * 判据唯一源（主册摘文，与 kernel/varix/src/uni1/ v1 同参数）：
 * - F413「三键位语义；窗口自动框定（含阴影边界）；即存路径与通知；键位
 *   可改持久化」。
 * - F425「判定轨迹特征（频率+幅度阈值参数入册）；误触注入测试（拖文件
 *   晃动 20 次 0 触发）；最小化/恢复原位精度（F237 联动）；依次收缩动画；
 *   关闭开关（设置可禁用）」。
 * - F426「三键行为矩阵；脏标记反馈；另存命名规则；只读保存失败三问；
 *   键位注册（F244）」。
 * - F427「三域×三键矩阵用例；文件剪切延迟执行与反悔（剪切后源文件还在
 *   直到粘贴）；粘贴冲突走 F087；禁用静默判据」。
 * - F428「四常用项功能；分页预览准确性（与实际输出一致）；PDF 产物落位；
 *   无打印机引导链；对话框键位（F434）兼容」。
 * - F450「粘滞键逐键等效用例（五组合键）；五次触发与确认框；筛选键阈值
 *   参数与效果；指示器；永不再提醒」。
 */

import { u1Store } from "./u1store";

/* ------------------------------- F413 PrintScreen ------------------------------- */

export type PrintScreenMode = "fullscreen" | "window" | "region";

/** 三键位语义映射。 */
export function printScreenResolve(withWin: boolean, withAlt: boolean): PrintScreenMode {
  if (withWin) return "region";
  if (withAlt) return "window";
  return "fullscreen";
}

/** 即存路径通知（判据：即存路径与通知）。 */
export function printScreenSavedNotice(path: string): string {
  return `截图已保存：${path}`;
}

/* ------------------------------- F425 Aero Shake ------------------------------- */

export const SHAKE_WINDOW_MS = 150;
export const SHAKE_CROSSES_REQUIRED = 3;
export const SHAKE_AMPLITUDE_MIN_PX = 40;
export const CASCADE_STEP_MS = 40;

export interface TracePoint { tMs: number; x: number }

/** 晃动判定（与内核 shake 同式：时间窗内过中线 ≥3 次且幅度达标）。 */
export function shakeDetect(points: TracePoint[]): boolean {
  const t0 = points[0]?.tMs;
  const x0 = points[0]?.x;
  if (t0 === undefined || x0 === undefined) return false;
  let crossings = 0;
  let maxAmp = 0;
  let lastSide = 0;
  for (const p of points) {
    if (p.tMs - t0 > SHAKE_WINDOW_MS) break;
    const d = p.x - x0;
    maxAmp = Math.max(maxAmp, Math.abs(d));
    const side = d > 0 ? 1 : d < 0 ? -1 : 0;
    if (side !== 0 && lastSide !== 0 && side !== lastSide) crossings += 1;
    if (side !== 0) lastSide = side;
  }
  return crossings + 1 >= SHAKE_CROSSES_REQUIRED && maxAmp >= SHAKE_AMPLITUDE_MIN_PX;
}

/** 依次收缩阶梯（判据：逐窗 40ms 延迟）。 */
export function cascadeSteps(windowCount: number): number[] {
  return Array.from({ length: windowCount }, (_, i) => i * CASCADE_STEP_MS);
}

/* ------------------------------- F426 文档键位保存族 ------------------------------- */

/** 三键行为矩阵（Ctrl+S / Ctrl+Shift+S / F12——F244 注册面）。 */
export function saveKeyResolve(ctrl: boolean, shift: boolean, key: string): "save" | "save-as" | "noop" {
  if (ctrl && key.toLowerCase() === "s") return shift ? "save-as" : "save";
  if (key === "F12") return "save-as";
  return "noop";
}

/** 只读保存失败三问（判据：另存为/重试/取消——取消永远安全出路）。 */
export const READONLY_SAVE_OPTIONS = ["另存为…", "重试", "取消"] as const;

/* ------------------------------- F427 Ctrl+X/C/V 语义表 ------------------------------- */

export type ClipDomain = "text" | "files" | "image";

/** 剪切延迟执行：文件剪切后源文件还在直到粘贴（判据原文）。 */
export function cutSemantics(domain: ClipDomain): "deferred" | "immediate" {
  return domain === "files" ? "deferred" : "immediate";
}

/** 反悔窗口：剪切后粘贴前按 Esc/Ctrl+Z → 取消剪切标记（源文件无恙）。 */
export function cutRevert(pasted: boolean): boolean {
  return !pasted;
}

/* ------------------------------- F428 Ctrl+P 打印链路 ------------------------------- */

/** 四常用项（判据：打印机/页码范围/份数/单双面+方向——高级项折叠）。 */
export const PRINT_QUICK_ITEMS = ["打印机", "页码范围", "份数", "单双面/方向"] as const;

/** 分页预览数学：totalPages = ceil(pages / perSheet)。 */
export function printPreviewPages(totalJobs: number, perSheet: 1 | 2): number {
  return Math.ceil(totalJobs / perSheet);
}

/** 无打印机引导链（判据：没打印机的人有 PDF 出路）。 */
export const NO_PRINTER_GUIDE = "未检测到打印机——可选「Microsoft Print to PDF」虚拟打印，产物落文档目录";

/* ------------------------------- F450 粘滞键与筛选键 ------------------------------- */

export const STICKY_TOGGLE_PRESSES = 5;
export const FILTER_MIN_HOLD_MS = 50;

export type Modifier = "Ctrl" | "Alt" | "Shift" | "Win";

export interface StickyState {
  enabled: boolean; pendingConfirm: boolean; neverRemind: boolean;
  latched: Modifier[]; shiftPresses: number; enableNotices: number;
}

/** 连按 5 次 Shift → 确认框（防误启）；已启用 → 快捷关闭；不再提醒 → 直接启用。 */
export function stickyShiftPress(s: StickyState): StickyState {
  if (s.enabled) return { ...s, enabled: false, latched: [] };
  if (s.neverRemind) return { ...s, enabled: true, enableNotices: s.enableNotices + 1 };
  const presses = s.shiftPresses + 1;
  if (presses >= STICKY_TOGGLE_PRESSES) return { ...s, shiftPresses: 0, pendingConfirm: true };
  return { ...s, shiftPresses: presses };
}

export function stickyConfirm(s: StickyState): StickyState {
  if (!s.pendingConfirm) return s;
  return { ...s, pendingConfirm: false, enabled: true, enableNotices: s.enableNotices + 1 };
}

/** 修饰键锁存：按下松开即锁存；再按同键取消。 */
export function stickyTap(s: StickyState, m: Modifier): StickyState {
  if (!s.enabled) return s;
  if (s.latched.includes(m)) return { ...s, latched: s.latched.filter((x) => x !== m) };
  return { ...s, latched: [...s.latched, m] };
}

/** 实体键到达：合成组合键 + 锁存清空（一次合成的落点）。 */
export function stickyKey(s: StickyState, key: string): { combo: Modifier[]; key: string; next: StickyState } {
  return { combo: [...s.latched], key, next: { ...s, latched: [] } };
}

/** 逐键等效判据：粘滞路径 == 直按组合键语义。 */
export function stickyEquivalent(direct: Modifier[], latched: Modifier[], key: string, gotKey: string): boolean {
  return key === gotKey && direct.length === latched.length && direct.every((m) => latched.includes(m));
}

export interface FilterState { enabled: boolean; minHoldMs: number; ignoredTaps: number; suppressedRepeats: number }

/** 筛选键：短促误击忽略（手抖救星）；长按重复抑制。 */
export function filterPress(s: FilterState, holdMs: number): boolean {
  if (!s.enabled) return true;
  if (holdMs < s.minHoldMs) {
    s.ignoredTaps += 1;
    return false;
  }
  return true;
}

export function filterAutoRepeat(s: FilterState): boolean {
  if (s.enabled) {
    s.suppressedRepeats += 1;
    return false;
  }
  return true;
}

/** 指示器：任一开启即显示（判据原文）。 */
export function a11yIndicator(stickyOn: boolean, filterOn: boolean): boolean {
  return stickyOn || filterOn;
}

/* ------------------------------- 面板读数 ------------------------------- */

export function stickyPref(): { filterMinHoldMs: number; neverRemind: boolean } {
  const cfg = u1Store.get<{ filterMinHoldMs?: number; neverRemind?: boolean }>("a11yKeys");
  return { filterMinHoldMs: cfg.filterMinHoldMs ?? FILTER_MIN_HOLD_MS, neverRemind: cfg.neverRemind ?? false };
}
