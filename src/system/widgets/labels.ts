/**
 * 车道 E 本地词典（N-09 小组件）。与 theme-studio/labels.ts 同构——
 * 车道本地词典，不改 i18n/dictionaries.ts。
 */
import { useEffect, useState } from "react";

export type LaneLang = "zh" | "en";

export function detectLang(): LaneLang {
  if (typeof document === "undefined") return "zh";
  return document.documentElement.lang === "en" ? "en" : "zh";
}

export function useLaneLang(): LaneLang {
  const [lang, setLang] = useState<LaneLang>(detectLang);
  useEffect(() => {
    const ob = new MutationObserver(() => setLang(detectLang()));
    ob.observe(document.documentElement, { attributes: true, attributeFilter: ["lang"] });
    return () => ob.disconnect();
  }, []);
  return lang;
}

export const LABELS = {
  zh: {
    board: "小组件",
    desktopMode: "桌面常驻",
    boardMode: "组件板",
    editLayout: "编辑布局",
    done: "完成",
    add: "添加组件",
    remove: "移除",
    restore: "恢复",
    collapsed: "已收起（连续失败 3 次）",
    paused: "已暂停刷新",
    pausedFs: "全屏运行中 · 刷新已暂停",
    pausedBattery: "低电量 · 刷新已暂停",
    thirdImport: "导入第三方（manifest + html）",
    thirdPickManifest: "选择 manifest.json",
    thirdPickHtml: "选择 widget.html",
    thirdAdded: "第三方组件已加入",
    thirdBad: "manifest 校验失败",
    thirdTooBig: "html 超过 256KB 上限",
    sdkNote: "第三方组件运行于 iframe 沙箱（allow-scripts，无 same-origin）；禁网依赖宿主 CSP",
    clock: "世界时钟",
    calendar: "本日历",
    todos: "待办",
    quicklaunch: "快捷启动",
    focus: "倒计时专注",
    net: "网络状态",
    inspire: "灵感卡",
    perf: "性能迷你条",
    perfNoData: "性能数据源不可用（需 N-19 数据通道）",
    online: "在线",
    offline: "离线",
    start: "开始",
    pause: "暂停",
    reset: "重置",
    minutes: "分钟",
    timeUp: "时间到",
    addTodo: "添加待办…",
    noTodos: "暂无待办",
    addClock: "添加时区（IANA 名）",
    openApp: "打开",
    shuffle: "换一条",
    mutedNote: "提示音尊重系统静音设置",
  },
  en: {
    board: "Widgets",
    desktopMode: "On desktop",
    boardMode: "Widget board",
    editLayout: "Edit layout",
    done: "Done",
    add: "Add widget",
    remove: "Remove",
    restore: "Restore",
    collapsed: "Collapsed (3 consecutive failures)",
    paused: "Refresh paused",
    pausedFs: "Fullscreen · refresh paused",
    pausedBattery: "Low battery · refresh paused",
    thirdImport: "Import 3rd-party (manifest + html)",
    thirdPickManifest: "Pick manifest.json",
    thirdPickHtml: "Pick widget.html",
    thirdAdded: "3rd-party widget added",
    thirdBad: "manifest invalid",
    thirdTooBig: "html exceeds 256KB limit",
    sdkNote: "3rd-party widgets run in an iframe sandbox (allow-scripts, no same-origin); network denial relies on host CSP",
    clock: "World clock",
    calendar: "Calendar",
    todos: "Todos",
    quicklaunch: "Quick launch",
    focus: "Focus timer",
    net: "Network",
    inspire: "Inspire",
    perf: "Perf mini",
    perfNoData: "Perf data source unavailable (needs N-19 channel)",
    online: "Online",
    offline: "Offline",
    start: "Start",
    pause: "Pause",
    reset: "Reset",
    minutes: "min",
    timeUp: "Time is up",
    addTodo: "Add a todo…",
    noTodos: "No todos",
    addClock: "Add timezone (IANA)",
    openApp: "Open",
    shuffle: "Shuffle",
    mutedNote: "Chime respects system mute setting",
  },
} as const;