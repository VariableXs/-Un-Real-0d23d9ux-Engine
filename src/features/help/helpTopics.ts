/**
 * AI-17 · Z-70 帮助中心（Help Center）
 * ① 上下文帮助主题注册表：界面路由 → 帮助主题（F1 命中依据）
 * ② 离线文档（与版本同发布，随包离线）
 * ③ 全文搜索（本地，无遥测）
 * 内容双语内联（帮助文档非 UI 词条，不进 i18n 词典审计域）。
 */

export interface HelpTopic {
  id: string;
  /** 命中的界面路由/面板 id（F1 上下文映射表） */
  matches: string[];
  zh: { title: string; body: string };
  en: { title: string; body: string };
  /** 快捷键表条目（从键位注册表口径同步的单一事实源摘要） */
  keys?: string[];
}

export const HELP_TOPICS: HelpTopic[] = [
  {
    id: "desktop",
    matches: ["desktop", "桌面", "wallpaper", "壁纸"],
    zh: {
      title: "桌面与环境",
      body: "桌面支持图标自由摆放、右键菜单、框选与多选。壁纸在 设置 → 外观 中更换；环境窗口之间用 Ctrl+Alt+方向键 切换。",
    },
    en: {
      title: "Desktop & Environment",
      body: "The desktop supports free icon placement, context menu, and marquee selection. Change wallpaper in Settings → Appearance; switch environments with Ctrl+Alt+Arrow.",
    },
  },
  {
    id: "startmenu",
    matches: ["startmenu", "start-menu", "开始菜单", "start"],
    zh: {
      title: "开始菜单",
      body: "点击任务栏中央「开始」打开。支持搜索、固定应用与最近使用；右键应用可打开跳转列表。左下角屏幕边缘热区默认映射为打开开始菜单（可在 设置 → 视觉语言 关闭）。",
    },
    en: {
      title: "Start Menu",
      body: "Open with the centered taskbar button. Search, pinning, and recent apps are supported; right-click an app for its jump list. The bottom-left edge hotspot opens Start by default (configurable in Settings → Vision).",
    },
  },
  {
    id: "windows",
    matches: ["vwm", "window", "窗口", "分屏", "snap"],
    zh: {
      title: "窗口管理",
      body: "拖动窗口到屏幕边缘可吸附分屏；顶部标题栏可拖动，双击最大化；快捷键表见 设置 → 快捷键。拖动过程中系统会临时降低渲染质量以保帧率，松手即恢复全清。",
    },
    en: {
      title: "Window Management",
      body: "Drag a window to a screen edge to snap it. Title bar drag; double-click to maximize. See Settings → Shortcuts for the key table. During drags, render quality is temporarily reduced for smoothness and restored on release.",
    },
  },
  {
    id: "files",
    matches: ["files", "file", "explorer", "文件"],
    zh: {
      title: "文件管理器",
      body: "多标签页 + 双栏视图；支持批量重命名、重复文件报告、空间分析与校验工具。删除的文件进入回收站，可在其中恢复或彻底清除。",
    },
    en: {
      title: "File Manager",
      body: "Multi-tab and dual-pane views; batch rename, duplicate report, space analysis, and checksum tools. Deleted files go to the Recycle Bin for restore or permanent erase.",
    },
  },
  {
    id: "settings",
    matches: ["settings", "设置", "preference"],
    zh: {
      title: "设置",
      body: "所有设置即时生效并跨窗口同步。「视觉语言」页集中管理动效速度、边缘热区、材质档位与引导重置。",
    },
    en: {
      title: "Settings",
      body: "All settings apply instantly and sync across windows. The Vision page centralizes motion speed, edge hotspots, material tier, and onboarding reset.",
    },
  },
  {
    id: "themes",
    matches: ["theme", "主题", "appearance", "外观", "material"],
    zh: {
      title: "主题与材质",
      body: "支持深色 / 浅色 / 高对比度三态。切换时采用 170ms 交叉淡入，不会闪白闪黑。磨砂材质同屏并发超过 3 个时自动降级为实心以保流畅。",
    },
    en: {
      title: "Themes & Materials",
      body: "Dark / light / high-contrast tri-state. Switching uses a 170ms cross-fade with no white or black flash. When more than 3 frosted surfaces are on screen, they automatically degrade to solid for smoothness.",
    },
  },
  {
    id: "shortcuts",
    matches: ["shortcut", "keymap", "快捷键", "hotkey"],
    zh: {
      title: "快捷键",
      body: "Ctrl+/ 打开速查浮层，随时查看全部键位；Ctrl+Alt+O 打开编排中心。冲突检测在 设置 → 快捷键 中自动进行。",
    },
    en: {
      title: "Shortcuts",
      body: "Ctrl+/ opens the cheat-sheet overlay with every binding; Ctrl+Alt+O opens the orchestrator. Conflict detection runs in Settings → Shortcuts.",
    },
  },
  {
    id: "privacy",
    matches: ["privacy", "security", "隐私", "安全"],
    zh: {
      title: "隐私与安全",
      body: "全部数据本地存储，无遥测、无后台上报。数据保险箱在 设置 → 数据 中；网络访问默认最小化。",
    },
    en: {
      title: "Privacy & Security",
      body: "All data stays local — no telemetry, no background reporting. The data vault lives in Settings → Data; network access is minimized by default.",
    },
  },
];

/** F1 上下文命中：按当前界面上下文串（路由/面板 id/可见类名）匹配主题。 */
export function topicForContext(context: string): HelpTopic | null {
  const c = context.toLowerCase();
  for (const topic of HELP_TOPICS) {
    if (topic.matches.some((m) => c.includes(m.toLowerCase()))) return topic;
  }
  return null;
}

/** 全文搜索：标题与正文（zh/en 双语同时匹配），多词按「全部命中」计分，返回带评分的有序结果。 */
export function searchHelp(query: string, limit = 8): { topic: HelpTopic; score: number }[] {
  const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return [];
  const results: { topic: HelpTopic; score: number }[] = [];
  for (const topic of HELP_TOPICS) {
    let score = 0;
    for (const locale of [topic.zh, topic.en]) {
      const title = locale.title.toLowerCase();
      const body = locale.body.toLowerCase();
      for (const q of tokens) {
        if (title === q) score += 100;
        else if (title.startsWith(q)) score += 60;
        else if (title.includes(q)) score += 30;
        if (body.includes(q)) score += 10;
      }
    }
    for (const q of tokens) {
      if (topic.matches.some((m) => m.toLowerCase().includes(q))) score += 20;
    }
    if (score > 0) results.push({ topic, score });
  }
  return results.sort((a, b) => b.score - a.score).slice(0, limit);
}

/** 默认主题（F1 未命中时展示的兜底页）。 */
export function defaultTopic(lang: "zh" | "en"): HelpTopic {
  const zh = { title: "帮助中心", body: "在这里搜索或按 F1 查看当前界面的帮助。全部文档随版本离线发布。" };
  const en = { title: "Help Center", body: "Search here or press F1 for help on the current screen. All docs ship offline with each release." };
  return { id: "default", matches: [], zh, en: lang === "en" ? en : zh, ...(lang === "zh" ? { en } : {}) } as HelpTopic;
}
