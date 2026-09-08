/**
 * AI-07 · 内置命令注册（命令注册纪律：AI-07 全部用户可执行动作入表）。
 * 动作处理器由 UI 层安装（setCommandHandler）——库层保持纯净可测，
 * 面板/搜索/宏引擎共用同一事实源。
 */

import { registerCommands } from "./registry";
import type { Command } from "./types";

/** UI 层动作处理器表（id → 执行）。 */
const handlers = new Map<string, () => void>();

export function setCommandHandler(id: string, fn: () => void): void {
  handlers.set(id, fn);
}

function run(id: string): void {
  handlers.get(id)?.();
}

/** AI-07 域内置命令（效率中枢 + 既有动作收编）。 */
export function builtinCommands(): Command[] {
  const c = (
    id: string,
    titleKey: string,
    category: Command["category"],
    keywords: string[] = [],
  ): Command => ({ id, titleKey, category, keywords, action: () => run(id), source: "builtin" });

  return [
    // ---- N-13 面板自身 ----
    c("palette.open", "cmdPalette", "tool", ["command palette", "ctrl+k", "mlmb"]),
    // ---- 应用（VWM 工具 + 系统窗口 + 官方软件）----
    c("app.notes", "cmdAppNotes", "app", ["notes", "bianzheng", "bj"]),
    c("app.calc", "cmdAppCalc", "app", ["calc", "jisuanqi", "jsq"]),
    c("app.calendar", "cmdAppCalendar", "app", ["calendar", "rili", "rl"]),
    c("app.snapshot", "cmdAppSnapshot", "app", ["snapshot", "快照", "kz"]),
    c("app.clipboard", "cmdAppClipboard", "app", ["clipboard", "剪贴板", "jtb"]),
    c("app.explorer", "cmdAppExplorer", "app", ["explorer", "文件", "wj"]),
    c("app.recycle", "cmdAppRecycle", "app", ["recycle", "回收站", "hsz"]),
    c("app.taskman", "cmdAppTaskman", "app", ["task manager", "任务管理", "rwgl"]),
    c("app.write", "cmdAppWrite", "app", ["write", "记录", "jl"]),
    c("app.mind", "cmdAppMind", "app", ["mindmap", "思维导图", "swdt"]),
    c("app.code", "cmdAppCode", "app", ["code", "代码", "dm"]),
    c("app.fate", "cmdAppFate", "app", ["fate", "回顾", "hg"]),
    // ---- 环境动作 ----
    c("act.snapLeft", "cmdSnapLeft", "action", ["snap left", "贴靠左", "tkz"]),
    c("act.snapRight", "cmdSnapRight", "action", ["snap right", "贴靠右", "tky"]),
    c("act.snapUp", "cmdSnapUp", "action", ["snap up", "贴靠上", "tks"]),
    c("act.snapDown", "cmdSnapDown", "action", ["snap down", "贴靠下", "tkx"]),
    c("act.showDesktop", "cmdShowDesktop", "action", ["show desktop", "显示桌面", "xszm"]),
    c("act.minimizeAll", "cmdMinimizeAll", "action", ["minimize all", "最小化全部", "zxhqb"]),
    c("act.dnd", "cmdDnd", "action", ["do not disturb", "勿扰", "wr"]),
    c("act.quickPanel", "cmdQuickPanel", "action", ["quick panel", "快捷面板", "kjmb"]),
    c("act.settings", "cmdSettings", "action", ["settings", "设置", "sz"]),
    // ---- AI-07 效率中枢动作（N-14…N-18 / V-41…V-50）----
    c("eff.searchAll", "cmdSearchAll", "search", ["search everything", "搜索一切", "ssyq"]),
    c("eff.purePaste", "cmdPurePaste", "tool", ["pure paste", "净化粘贴", "jhzs"]),
    c("eff.qrCode", "cmdQrCode", "tool", ["qrcode", "二维码", "ewm"]),
    c("eff.quickNote", "cmdQuickNote", "tool", ["quick note", "速记", "sj"]),
    c("eff.snipRegion", "cmdSnip", "tool", ["snip", "截图", "jt"]),
    c("eff.ocrScreen", "cmdOcr", "tool", ["ocr", "取字", "qz", "屏慕取字"]),
    c("eff.shortcutsHub", "cmdShortcutsHub", "tool", ["shortcuts hub", "快捷键中心", "kjzzx"]),
    c("eff.macroEngine", "cmdMacroEngine", "tool", ["macro", "宏引擎", "hyq"]),
    c("eff.cheatsheetMd", "cmdCheatsheetMd", "tool", ["cheatsheet", "速查表", "scb"]),
    c("eff.cheatsheetHtml", "cmdCheatsheetHtml", "tool", ["cheatsheet html", "速查表HTML"]),
    c("eff.historyPrivacy", "cmdHistoryPrivacy", "settings", ["search history privacy", "搜索历史隐私", "ssl"]),
    c("eff.inlineCalc", "cmdInlineCalc", "tool", ["calculator", "内联计算", "nljs"]),
    c("eff.timestamp", "cmdTimestamp", "tool", ["timestamp", "时间戳", "sjc", "now", "ts"]),
    c("eff.clipboardHistory", "cmdClipboardHistory", "tool", ["clipboard history", "剪贴板历史", "jtb"]),
  ];
}

let registered = false;

/** 幂等注册（DesktopShell 挂载时调用一次）。 */
export function ensureBuiltinRegistered(): void {
  if (registered) return;
  registerCommands(builtinCommands());
  registered = true;
}
