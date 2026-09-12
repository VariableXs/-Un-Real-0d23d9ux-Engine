/**
 * AURORA-10000 · AI-11~AI-15 车道 · 本地词典（车道约定：不改 i18n/dictionaries.ts）。
 * 与 widgets/labels.ts 同构：useLaneLang 监听 <html lang> 切换。
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
    title: "设计中心 · 领域03",
    lane: "AURORA-10000 · AI-11~AI-15 · F01251~F01875",
    close: "关闭",
    groupIcons: "图标系统（AI-11）",
    groupWallpaper: "壁纸系统（AI-12）",
    groupWidgets: "桌面微件（AI-13）",
    groupVisual: "视觉一致性（AI-14）",
    groupPersonality: "桌面个性（AI-15）",
    preset: "参数档",
    switch: "开关",
    reserved: "预留位",
    on: "已开启",
    off: "已关闭",
    selected: "已选",
    resetFamily: "重置本族",
    resetAll: "全部重置",
    exportProfile: "导出档案",
    importProfile: "导入档案",
    profileOk: "档案已导入",
    profileBad: "档案校验失败",
    count: (n: number, total: number): string => `${n}/${total}`,
    theaterOpen: "进入剧场模式",
    theaterStop: "退出剧场",
    ritualTest: "试播仪式卡",
    tokenDebug: "令牌调试（F01646）",
    tokenName: "令牌名",
    tokenValue: "值",
    tokenSet: "写入",
    tokenClear: "清除",
    engineApplied: "已发送引擎切换请求（ai04:wallpaper-apply）",
    entryApplied: "已生效",
    healthRunning: "健康提醒运行中（本地，不出本机）",
    lunarNote: "农历/节气表覆盖 2024–2029，其余年份诚实跳过",
  },
  en: {
    title: "Design Center · Domain 03",
    lane: "AURORA-10000 · AI-11~AI-15 · F01251~F01875",
    close: "Close",
    groupIcons: "Icons (AI-11)",
    groupWallpaper: "Wallpaper (AI-12)",
    groupWidgets: "Widgets (AI-13)",
    groupVisual: "Visual consistency (AI-14)",
    groupPersonality: "Personality (AI-15)",
    preset: "Preset",
    switch: "Switch",
    reserved: "Reserved",
    on: "On",
    off: "Off",
    selected: "Selected",
    resetFamily: "Reset family",
    resetAll: "Reset all",
    exportProfile: "Export profile",
    importProfile: "Import profile",
    profileOk: "Profile imported",
    profileBad: "Profile rejected",
    count: (n: number, total: number): string => `${n}/${total}`,
    theaterOpen: "Enter theater",
    theaterStop: "Exit theater",
    ritualTest: "Preview ritual card",
    tokenDebug: "Token debugger (F01646)",
    tokenName: "Token",
    tokenValue: "Value",
    tokenSet: "Set",
    tokenClear: "Clear",
    engineApplied: "Engine switch sent (ai04:wallpaper-apply)",
    entryApplied: "Active",
    healthRunning: "Health reminders running (local only)",
    lunarNote: "Lunar/solar-term table covers 2024–2029; later years skipped honestly",
  },
} as const;
