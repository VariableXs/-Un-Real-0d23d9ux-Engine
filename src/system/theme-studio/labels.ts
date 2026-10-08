/**
 * 车道 E 本地词典（N-08）：不改动 i18n/dictionaries.ts（避免并行冲突），
 * 主题工坊 overlay 经 createRoot 挂载、不在 I18nProvider 上下文内，
 * 语言信号取 documentElement.lang（App.tsx 设置应用时写入）。
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
    title: "主题工坊",
    close: "关闭",
    tabLight: "亮色",
    tabDark: "暗色",
    tabHC: "高对比",
    groupBg: "背景",
    groupText: "文字",
    groupBrand: "品牌 / 描边",
    groupState: "状态色",
    shape: "形 · 圆角 / 密度 / 字阶",
    radius: "圆角",
    density: "密度",
    fontScale: "字阶",
    radiusSharp: "硬朗",
    radiusSoft: "柔和",
    radiusRound: "圆润",
    densityCompact: "紧凑",
    densityRegular: "标准",
    densityRelaxed: "宽松",
    fontSmall: "小",
    fontStandard: "标准",
    fontLarge: "大",
    preview: "实时预览（桌面缩样）",
    contrastOk: "AA 达标",
    contrastFail: "不达标",
    contrastPairs: "对比度校验（WCAG AA ≥ 4.5:1）",
    exempt: "豁免并标注",
    exemptNote: "已豁免（导出包内保留标注）",
    name: "主题名",
    save: "保存",
    saved: "已保存到本地主题库",
    apply: "应用到桌面",
    applied: "已应用（跟随亮/暗/高对比信号切换变体）",
    restore: "还原内建主题",
    restored: "已还原内建主题",
    export: "导出 .vtheme",
    import: "导入 .vtheme",
    importOk: "导入成功",
    importFail: "导入失败",
    conflicts: "冲突字段",
    library: "本地主题库",
    noLibrary: "暂无已保存主题",
    translucentNote: "半透明 token：拾色器只改 RGB，原 alpha 保留",
    storageNote: "应用主题存于本会话 + localStorage；随 data-theme 信号自动换变体",
  },
  en: {
    title: "Theme Studio",
    close: "Close",
    tabLight: "Light",
    tabDark: "Dark",
    tabHC: "High contrast",
    groupBg: "Background",
    groupText: "Text",
    groupBrand: "Brand / Stroke",
    groupState: "State",
    shape: "Shape · Radius / Density / Type scale",
    radius: "Radius",
    density: "Density",
    fontScale: "Type scale",
    radiusSharp: "Sharp",
    radiusSoft: "Soft",
    radiusRound: "Round",
    densityCompact: "Compact",
    densityRegular: "Regular",
    densityRelaxed: "Relaxed",
    fontSmall: "S",
    fontStandard: "M",
    fontLarge: "L",
    preview: "Live preview (mini desktop)",
    contrastOk: "AA pass",
    contrastFail: "fail",
    contrastPairs: "Contrast checks (WCAG AA ≥ 4.5:1)",
    exempt: "Exempt & annotate",
    exemptNote: "Exempted (annotation kept in export)",
    name: "Theme name",
    save: "Save",
    saved: "Saved to local library",
    apply: "Apply to desktop",
    applied: "Applied (variant follows light/dark/HC signal)",
    restore: "Restore built-in theme",
    restored: "Built-in theme restored",
    export: "Export .vtheme",
    import: "Import .vtheme",
    importOk: "Imported",
    importFail: "Import failed",
    conflicts: "Conflicting fields",
    library: "Local library",
    noLibrary: "No saved themes yet",
    translucentNote: "Translucent token: picker edits RGB only, alpha kept",
    storageNote: "Applied theme persists in localStorage; variant follows data-theme signal",
  },
} as const;

export type LaneT = (typeof LABELS)["zh"];