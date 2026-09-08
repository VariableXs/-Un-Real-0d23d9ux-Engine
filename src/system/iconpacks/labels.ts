/**
 * 车道 E 本地词典（N-12 图标包）。同构 lane 词典，不改 i18n/dictionaries.ts。
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
    title: "图标包工坊",
    close: "关闭",
    installed: "已安装包",
    noPack: "未安装任何包（全部走原生图标）",
    install: "安装",
    uninstall: "卸载",
    uninstalled: "已卸载，图标全部还原",
    pick: "选择 .vicon 文件",
    preview: "安装前预览",
    confirm: "确认安装",
    cancel: "取消",
    imported: "校验通过",
    bad: "校验失败",
    fallbackCount: "缺键回退计数（本会话）",
    fallbackNote: "缺键逐图标回退原生，绝不空洞；计数含预览查询",
    grid: "12 宫格对比预览（左 原 · 右 新）",
    original: "原",
    replaced: "新",
    fallbackRow: "回退",
    exportSkeleton: "导出包骨架",
    zipNote: ".vicon 为 JSON manifest（内联 SVG / PNG dataURL）；未用 zip 容器以免新增依赖",
    nativeNote: "「原」列当前为占位首字母——真实原生图需在 DesktopIcons/StartMenu 接线 useIcon 后显示",
    keys: "个图标键",
    version: "版本",
  },
  en: {
    title: "Icon Pack Studio",
    close: "Close",
    installed: "Installed pack",
    noPack: "No pack installed (all native icons)",
    install: "Install",
    uninstall: "Uninstall",
    uninstalled: "Uninstalled, icons restored",
    pick: "Pick a .vicon file",
    preview: "Preview before install",
    confirm: "Confirm install",
    cancel: "Cancel",
    imported: "Validated",
    bad: "Validation failed",
    fallbackCount: "Fallback count (session)",
    fallbackNote: "Missing keys fall back per-icon; count includes preview queries",
    grid: "12-grid compare (left native · right pack)",
    original: "Orig",
    replaced: "New",
    fallbackRow: "fallback",
    exportSkeleton: "Export skeleton",
    zipNote: ".vicon is a JSON manifest (inline SVG / PNG dataURL); no zip container to avoid new deps",
    nativeNote: "「Orig」column uses placeholder initials until DesktopIcons/StartMenu wire useIcon",
    keys: "icon keys",
    version: "Version",
  },
} as const;