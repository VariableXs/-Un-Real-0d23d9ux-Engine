/**
 * AI-17 · U-57 引导体系（Onboarding System）
 * ① 首次导览：5 站 spotlight 巡回（任务栏→开始菜单→VWM→文件管理器→设置），
 *    每站三行说明，随时 Esc 跳过，可在设置重看。
 * ② 上下文提示（coach marks）：功能首次触达一次性气泡，可在设置统一清除重置。
 * ③ 技巧卡片：本地轮换 30 条，无推荐算法、无遥测。
 * 状态入 localStorage（onboarding.v1），全部提示文案走 i18n（VisionRuntime 消费 t()）。
 */

export interface TourStep {
  id: string;
  /** 目标元素选择器（找不到时自动跳到下一站） */
  selector: string;
  titleKey: string;
  bodyKey: string;
}

export const TOUR_STEPS: TourStep[] = [
  { id: "taskbar", selector: ".taskbar", titleKey: "obTourTaskbarTitle", bodyKey: "obTourTaskbarBody" },
  { id: "startmenu", selector: ".start-menu-btn", titleKey: "obTourStartTitle", bodyKey: "obTourStartBody" },
  { id: "vwm", selector: ".vwm-root, .vwm-window", titleKey: "obTourVwmTitle", bodyKey: "obTourVwmBody" },
  { id: "files", selector: ".files-root, .explorer", titleKey: "obTourFilesTitle", bodyKey: "obTourFilesBody" },
  { id: "settings", selector: ".settings-modal, .settings-panel", titleKey: "obTourSettingsTitle", bodyKey: "obTourSettingsBody" },
];

export interface Tip {
  id: string;
}

/** 30 条技巧（本地轮换，按日序数取模，无算法无遥测）。i18n 键名 tipN。 */
export const TIPS: Tip[] = Array.from({ length: 30 }, (_, i) => ({ id: `tip${i + 1}` }));

interface OnboardingState {
  tourDone: boolean;
  coachSeen: string[];
}

const STORAGE_KEY = "vision.onboarding.v1";
const EMPTY: OnboardingState = { tourDone: false, coachSeen: [] };

function loadState(): OnboardingState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...EMPTY, coachSeen: [] };
    const p = JSON.parse(raw) as Partial<OnboardingState>;
    return { tourDone: Boolean(p.tourDone), coachSeen: Array.isArray(p.coachSeen) ? p.coachSeen : [] };
  } catch {
    return { ...EMPTY, coachSeen: [] };
  }
}

function saveState(s: OnboardingState): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    /* 隐私模式：静默 */
  }
}

/** 首次导览是否尚未完成（OobeWizard 之后触发一次）。 */
export function needsTour(): boolean {
  return !loadState().tourDone;
}

export function markTourDone(): void {
  const s = loadState();
  s.tourDone = true;
  saveState(s);
}

/** 重看导览：清掉完成标记（下次 needsTour() = true）。 */
export function resetTour(): void {
  const s = loadState();
  s.tourDone = false;
  saveState(s);
}

/** coach mark：每提示只出现一次；出现即登记。 */
export function shouldShowCoach(id: string): boolean {
  return !loadState().coachSeen.includes(id);
}

export function markCoachSeen(id: string): void {
  const s = loadState();
  if (!s.coachSeen.includes(id)) {
    s.coachSeen.push(id);
    saveState(s);
  }
}

/** 设置页「清除并重置全部提示」：重置后全部重新出现。 */
export function resetAllCoaches(): void {
  const s = loadState();
  s.coachSeen = [];
  saveState(s);
}

/** 今日技巧：按日期序数轮换（跨天自然轮转）。 */
export function tipOfDay(now: Date = new Date()): Tip {
  const dayIndex = Math.floor(now.getTime() / 86_400_000);
  const idx = ((dayIndex % TIPS.length) + TIPS.length) % TIPS.length;
  return TIPS[idx]!;
}
