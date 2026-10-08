/**
 * AI-18 M-69 今日简报卡 — 纯逻辑：每日首启判定 + 小贴士池轮换。
 *
 * 口径：
 * - 触发 = 每日首次进入环境（本地日期键比对；跨午夜会话由日期键变化捕获）；
 * - 贴士池 30 条轮换（已读集合持久化，池尽重置）；
 * - 全部数据本地只读。
 */

/** 本地日期键（YYYY-MM-DD，本地时区）。 */
export function localDayKey(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** ISO 周数（简报卡「第 N 周」）。 */
export function isoWeek(d: Date): number {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const day = t.getUTCDay() || 7;
  t.setUTCDate(t.getUTCDate() + 4 - day);
  const yearStart = new Date(Date.UTC(t.getUTCFullYear(), 0, 1));
  return Math.ceil(((t.getTime() - yearStart.getTime()) / 86_400_000 + 1) / 7);
}

export interface BriefingTip {
  id: number;
  zh: string;
  en: string;
}

/** 30 条快捷键/功能小贴士（含新功能引导；zh/en 双语）。 */
export const TIPS: readonly BriefingTip[] = [
  { id: 1, zh: "Ctrl+Alt+O 打开窗口编排中心，一次摆好所有窗口", en: "Ctrl+Alt+O opens the Window Orchestrator to arrange all windows" },
  { id: 2, zh: "Ctrl+/ 呼出快捷键速查表，随时查按键", en: "Ctrl+/ brings up the keyboard quick reference" },
  { id: 3, zh: "双击 Esc 快速回到桌面，再按一次恢复", en: "Double-press Esc to peek at the desktop" },
  { id: 4, zh: "Ctrl+Alt+P 一键纯净模式：图标、任务栏、横幅全部隐去", en: "Ctrl+Alt+P toggles Pure Mode: icons, taskbar and banners all fade away" },
  { id: 5, zh: "任务栏时钟悬停可见农历与节气（V-73）", en: "Hover the taskbar clock for lunar date and solar terms" },
  { id: 6, zh: "音景引擎（U-49）：雨、林、白噪、粉噪、深夜，可叠加播放", en: "Soundscape Engine: rain, forest, white, pink and night, stackable" },
  { id: 7, zh: "壁纸空白处右键可收藏当前壁纸 / 换一张", en: "Right-click empty desktop to favorite or shuffle the wallpaper" },
  { id: 8, zh: "设置 → 氛围：界面密度、圆角、图标尺寸都有档位", en: "Settings → Ambience: density, radius and icon size are tiered" },
  { id: 9, zh: "焦点舱（U-50）：全屏专注，通知自动排队", en: "Focus Cabin: fullscreen focus with queued notifications" },
  { id: 10, zh: "节律助手（U-54）：久坐/用眼/喝水/站立温和提醒", en: "Rhythm Assistant: gentle sitting/eye/drink/stretch reminders" },
  { id: 11, zh: "Alt+Tab 之外，Ctrl+Alt+方向键也能切窗口", en: "Beyond Alt+Tab, Ctrl+Alt+Arrows also switch windows" },
  { id: 12, zh: "剪贴板历史 Ctrl+Shift+V，跨窗口粘贴不再来回切", en: "Clipboard history Ctrl+Shift+V ends copy-paste window juggling" },
  { id: 13, zh: "壁纸太艳看不清图标？设置里可调饱和度/明度（V-72）", en: "Wallpaper too vivid? Adjust saturation/brightness in Settings" },
  { id: 14, zh: "主题工坊选强调色时有对比度守护（V-80），选错会提醒", en: "The theme studio guards accent-color contrast (V-80)" },
  { id: 15, zh: "昼夜壁纸组（M-65）：早/日/暮/夜按时段自动切换", en: "Day-Around wallpapers switch automatically by time of day" },
  { id: 16, zh: "屏保时钟（M-68）：空闲 10 分钟进入极简时钟", en: "Screensaver Clock: 10 idle minutes bring a minimal clock" },
  { id: 17, zh: "环境辉光（U-53）：屏幕边缘随壁纸主色轻呼吸", en: "Ambient Glow: screen edges breathe with the wallpaper accent" },
  { id: 18, zh: "情绪引擎（N-33）：按时间段与场景微调界面氛围", en: "Mood Engine subtly tunes ambience by time and scene" },
  { id: 19, zh: "文件拖到任务栏图标上即可用该应用打开", en: "Drop a file on a taskbar icon to open it with that app" },
  { id: 20, zh: "标签智能文件夹：一条规则自动归档", en: "Smart folders: one rule, auto-filed" },
  { id: 21, zh: "版本时光机：文件的历史版本随时找回", en: "Version Time Machine brings back any file version" },
  { id: 22, zh: "回收站悬停可预览内容再决定恢复", en: "Hover the Recycle Bin to preview before restoring" },
  { id: 23, zh: "窗口排列：拖到屏幕边缘自动吸附半屏", en: "Drag a window to a screen edge to snap it to half" },
  { id: 24, zh: "通知中心支持免打扰时段，深夜不被打扰", en: "Notification Center supports quiet hours" },
  { id: 25, zh: "设置 → 视觉：动效速度可三档缩放", en: "Settings → Vision: motion speed has three tiers" },
  { id: 26, zh: "搜索框直接输入算式即可计算", en: "Type an expression in Search to calculate it" },
  { id: 27, zh: "便签支持多色与置顶，拖放即可传文本", en: "Notes support colors, pinning and text drag-drop" },
  { id: 28, zh: "会话恢复（M-72）：退出前自动存氛围快照，回来一键还原", en: "Session Restore saves an ambient snapshot on exit" },
  { id: 29, zh: "精选壁纸（V-71）可每日轮换，附本地小故事", en: "Curated wallpapers rotate daily with local stories" },
  { id: 30, zh: "全部数据本地存储，零出站网络", en: "All data stays local — zero outbound network" },
];

/** 是否应显示今日简报（每日首启；dismissedToday = 用户点了「今日不再显示」）。 */
export function shouldShowBriefing(lastShownDay: string, now: Date): boolean {
  return lastShownDay !== localDayKey(now);
}

/**
 * 轮换选下一条贴士：优先未读（id 升序首个未读），池尽后重置已读集合。
 * @returns [tip, nextShownIds]
 */
export function nextTip(shownIds: readonly number[]): [BriefingTip, number[]] {
  const shown = new Set(shownIds);
  const unread = TIPS.filter((t) => !shown.has(t.id));
  if (unread.length === 0) {
    // 池尽重置：从第 1 条重新开始（隔日不重复 → 整池轮完才重置）
    const first = TIPS[0];
    if (!first) return [{ id: 0, zh: "", en: "" }, [0]];
    return [first, [first.id]];
  }
  const tip = unread[0];
  if (!tip) return [TIPS[0] ?? { id: 0, zh: "", en: "" }, [TIPS[0]?.id ?? 0]];
  return [tip, [...shownIds, tip.id]];
}
