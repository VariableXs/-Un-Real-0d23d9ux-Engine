/**
 * AURORA-10000 领域04 · 族0095 快速操作（AI-19 批次，勿删）。
 * 快操登记/手势表/计时器与番茄钟模型。
 */

export type QuickOpId =
  | "edge-fan" | "corner-dial" | "longpress-fan" | "snap-top" | "snap-left" | "snap-right"
  | "shake" | "dblclick-pin" | "middle-close" | "side-nav" | "snip-region" | "record"
  | "color-picker" | "ruler" | "magnifier" | "note" | "translate" | "speak"
  | "timer" | "countdown" | "pomodoro" | "whitenoise";

export interface QuickOp {
  id: QuickOpId;
  label: string;
  /** 触发器描述。 */
  trigger: string;
  enabled: boolean;
}

/** 默认快操表（F02351~F02375）。 */
export const QUICK_OPS: readonly QuickOp[] = [
  { id: "edge-fan", label: "边缘扇形菜单", trigger: "屏幕右缘左滑", enabled: true },
  { id: "corner-dial", label: "角落拨盘", trigger: "右下角悬停", enabled: false },
  { id: "longpress-fan", label: "长按扇形菜单", trigger: "右键长按", enabled: false },
  { id: "snap-top", label: "拖到顶全屏", trigger: "拖拽", enabled: true },
  { id: "snap-left", label: "左半屏", trigger: "拖拽", enabled: true },
  { id: "snap-right", label: "右半屏", trigger: "拖拽", enabled: true },
  { id: "shake", label: "晃窗最小化其他", trigger: "标题栏摇动", enabled: false },
  { id: "snip-region", label: "区域截屏", trigger: "Win+Shift+S", enabled: true },
  { id: "color-picker", label: "屏幕取色", trigger: "快捷面板", enabled: true },
  { id: "ruler", label: "屏幕标尺", trigger: "快捷面板", enabled: false },
  { id: "magnifier", label: "临时放大镜", trigger: "Win++", enabled: false },
  { id: "timer", label: "快速计时", trigger: "启动器", enabled: true },
  { id: "pomodoro", label: "番茄钟", trigger: "启动器", enabled: false },
  { id: "whitenoise", label: "白噪音", trigger: "快捷面板", enabled: false },
];

/** 自定义快操（F02375）：动作 → 绑定登记。 */
const custom = new Map<string, string>();
export function bindCustomOp(id: string, action: string): void { custom.set(id, action); }
export function resolveCustomOp(id: string): string | undefined { return custom.get(id); }

/** 计时器（F02371/72/73）：返回到期时间戳。 */
export function startTimer(seconds: number, now = Date.now()): number { return now + seconds * 1000; }

/** 番茄钟节奏（F02373）：25 分钟专注 + 5 分钟休息。 */
export const POMODORO = { focusMin: 25, breakMin: 5 } as const;
export function pomodoroPhase(elapsedMs: number): "focus" | "break" {
  const cycle = (POMODORO.focusMin + POMODORO.breakMin) * 60_000;
  const t = elapsedMs % cycle;
  return t < POMODORO.focusMin * 60_000 ? "focus" : "break";
}
