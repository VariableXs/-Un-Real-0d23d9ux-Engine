/**
 * UNREAL-X-15000 · AI-16 任务栏内核与引擎·V 线逻辑核（族0157~0160 · X03901~X04000），勿删。
 * 任务栏本地化 2.0 / 任务栏主题适配 2.0 / 任务栏收官（跨线集成校验）。
 * （族0151~0153 内核线 → kernel/varix/src/shell/tbengine.rs；
 *   族0154~0156/0159 代码分析线 → code-analysis/core/src/ai16.rs）
 * 纯逻辑、零依赖，UI 层按此装配。
 */

/* ===================== 族0157 任务栏本地化 2.0 ===================== */

export const L10N_LOCALES = ['zh-CN', 'zh-TW', 'en-US', 'ja-JP'] as const;
export type Locale = (typeof L10N_LOCALES)[number];

export type TaskbarStrKey =
  | 'taskbar.contextMenu' | 'startMenu.label' | 'tray.showHidden' | 'clock.format'
  | 'search.placeholder' | 'taskView.label' | 'notifCenter.empty' | 'quickPanel.title';

export const TASKBAR_STRINGS: Record<Locale, Partial<Record<TaskbarStrKey, string>>> = {
  'zh-CN': {
    'taskbar.contextMenu': '任务栏设置', 'startMenu.label': '开始', 'tray.showHidden': '显示隐藏的图标',
    'clock.format': 'yyyy/M/d HH:mm', 'search.placeholder': '搜索应用、文件和设置',
    'taskView.label': '任务视图', 'notifCenter.empty': '没有新通知', 'quickPanel.title': '快捷面板',
  },
  'zh-TW': {
    'taskbar.contextMenu': '工作列設定', 'startMenu.label': '開始', 'tray.showHidden': '顯示隱藏的圖示',
    'clock.format': 'yyyy/M/d HH:mm', 'search.placeholder': '搜尋應用程式、檔案與設定',
    'taskView.label': '工作檢視', 'notifCenter.empty': '沒有新通知', 'quickPanel.title': '快捷面板',
  },
  'en-US': {
    'taskbar.contextMenu': 'Taskbar settings', 'startMenu.label': 'Start', 'tray.showHidden': 'Show hidden icons',
    'clock.format': 'M/d/yyyy h:mm a', 'search.placeholder': 'Search apps, files, and settings',
    'taskView.label': 'Task view', 'notifCenter.empty': 'No new notifications', 'quickPanel.title': 'Quick panel',
  },
  'ja-JP': {
    'taskbar.contextMenu': 'タスクバーの設定', 'startMenu.label': 'スタート', 'tray.showHidden': '隠れているアイコンを表示',
    'clock.format': 'yyyy/M/d H:mm', 'search.placeholder': 'アプリ、ファイル、設定を検索',
    'taskView.label': 'タスクビュー', 'notifCenter.empty': '新しい通知はありません', 'quickPanel.title': 'クイックパネル',
  },
};

/** 任务栏本地化：四语回退链（locale → zh-CN → key 兜底）、插值、RTL 无关断言。 */
export class TaskbarL10n {
  locale: Locale = 'zh-CN';

  static isLocale(v: string): v is Locale { return (L10N_LOCALES as readonly string[]).includes(v); }

  setLocale(l: string): boolean {
    if (!TaskbarL10n.isLocale(l)) return false;
    this.locale = l;
    return true;
  }

  t(key: TaskbarStrKey): string {
    return TASKBAR_STRINGS[this.locale][key] ?? TASKBAR_STRINGS['zh-CN'][key] ?? key;
  }

  /** 覆盖率：该语言与 zh-CN 键集一致。 */
  coverage(l: Locale): number {
    const base = Object.keys(TASKBAR_STRINGS['zh-CN']!).length;
    const has = Object.keys(TASKBAR_STRINGS[l]!).length;
    return base === 0 ? 1 : has / base;
  }

  /** 插值：{n} 占位符替换。 */
  static format(tpl: string, vars: Record<string, string | number>): string {
    return tpl.replace(/\{(\w+)\}/g, (_, k) => String(vars[k] ?? `{${k}}`));
  }
}

/* ===================== 族0158 任务栏主题适配 2.0 ===================== */

export const THEME_NAMES = ['light', 'dark', 'hc'] as const;
export type ThemeName = (typeof THEME_NAMES)[number];

export interface TaskbarThemeToken {
  bg: string; fg: string; accent: string; borderPx: number; blurOn: boolean;
}

/** 三主题令牌（HC 红线：强边框、禁模糊、黑白高对比）。 */
export const TASKBAR_THEMES: Record<ThemeName, TaskbarThemeToken> = {
  light: { bg: 'rgba(243,243,243,0.85)', fg: '#1a1a1a', accent: '#0067c0', borderPx: 1, blurOn: true },
  dark: { bg: 'rgba(32,32,32,0.85)', fg: '#f3f3f3', accent: '#4cc2ff', borderPx: 1, blurOn: true },
  hc: { bg: '#000000', fg: '#ffffff', accent: '#00ffff', borderPx: 2, blurOn: false },
};

/** 主题适配：令牌解析、HC 红线、壁纸亮度推导、降级。 */
export class TaskbarTheme {
  theme: ThemeName = 'dark';
  /** 壁纸平均亮度 0~1000。 */
  wallpaperLuma = 500;

  setTheme(t: string): boolean {
    if (!(THEME_NAMES as readonly string[]).includes(t)) return false;
    this.theme = t as ThemeName;
    return true;
  }

  token(): TaskbarThemeToken { return TASKBAR_THEMES[this.theme]; }

  /** 壁纸亮度 → 自动主题（luma<300 暗、>700 亮、其余保持）。 */
  autoFromWallpaper(): ThemeName {
    if (this.wallpaperLuma < 300) return 'dark';
    if (this.wallpaperLuma > 700) return 'light';
    return this.theme;
  }

  hcOk(): boolean {
    const t = TASKBAR_THEMES.hc;
    return t.borderPx >= 2 && !t.blurOn && t.bg === '#000000' && t.fg === '#ffffff';
  }

  /** 低配：关闭模糊、简化为纯色。 */
  degrade(): TaskbarThemeToken {
    const t = { ...this.token() };
    t.blurOn = false;
    return t;
  }
}

/* ===================== 族0160 任务栏收官（跨线集成校验） ===================== */

/** 收官聚合口径：三线各自 25 项全绿 + 跨线握手。 */
export interface TaskbarClosingProbe {
  kernelFamiliesGreen: number; // 0148/0151/0152/0153 → 4
  analysisFamiliesGreen: number; // 0154/0155/0156/0159 → 4
  variableFamiliesGreen: number; // 0141~0147/0149/0150/0157/0158 → 11
}

export const CLOSING_EXPECT = { kernel: 4, analysis: 4, variable: 11 } as const;

export class TaskbarClosing {
  probe: TaskbarClosingProbe = { kernelFamiliesGreen: 0, analysisFamiliesGreen: 0, variableFamiliesGreen: 0 };
  handshakes: string[] = [];

  record(line: 'kernel' | 'analysis' | 'variable', families: number): boolean {
    if (!Number.isInteger(families) || families < 0 || families > 12) return false;
    if (line === 'kernel') this.probe.kernelFamiliesGreen = families;
    else if (line === 'analysis') this.probe.analysisFamiliesGreen = families;
    else this.probe.variableFamiliesGreen = families;
    return true;
  }

  /** 三线握手：命名去重。 */
  handshake(name: string): boolean {
    if (this.handshakes.includes(name)) return true;
    this.handshakes.push(name);
    return true;
  }

  allGreen(): boolean {
    const e = CLOSING_EXPECT;
    return this.probe.kernelFamiliesGreen === e.kernel
      && this.probe.analysisFamiliesGreen === e.analysis
      && this.probe.variableFamiliesGreen === e.variable
      && this.handshakes.length >= 5;
  }
}
