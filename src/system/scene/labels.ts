/**
 * 车道 W 实况场景本地词典（不改 i18n/dictionaries.ts）。
 */

export const SCENE_LABELS = {
  zh: {
    title: "实况场景",
    enable: "启用实况场景",
    weather: "天气（手工模式 · 零网络）",
    wOff: "无",
    wSunny: "晴 · 光斑",
    wRain: "雨",
    wSnow: "雪",
    wLeaves: "落叶",
    interactive: "粒子层接收点击（关 = 点击穿透到桌面）",
    daynight: "昼夜色温（晨蓝→正午白→暮金→夜靛）",
    season: "季节 accent 偏移（仅装饰）",
    boundary: "天气为手工模式，无任何网络请求；HTTP 气象源需 netconsent 同意框架（后续车道）",
    particleStatic: "粒子已静止（打字/全屏降级/减动效）",
    close: "关闭",
  },
  en: {
    title: "Living Scene",
    enable: "Enable living scene",
    weather: "Weather (manual · offline)",
    wOff: "Off",
    wSunny: "Sunny glints",
    wRain: "Rain",
    wSnow: "Snow",
    wLeaves: "Fall leaves",
    interactive: "Particles catch clicks (off = click-through to desktop)",
    daynight: "Day-night tint (dawn→noon→dusk→night)",
    season: "Seasonal accent shift (decorative)",
    boundary: "Manual weather only, zero network; HTTP sources require the netconsent framework (later lane)",
    particleStatic: "Particles paused (typing/fullscreen/reduced motion)",
    close: "Close",
  },
} as const;

export type SceneLabelKey = keyof (typeof SCENE_LABELS)["zh"];

export function sceneT(lang?: string): (k: SceneLabelKey) => string {
  const l = lang ?? (typeof navigator !== "undefined" ? navigator.language : "zh");
  const dict = SCENE_LABELS[l && l.toLowerCase().startsWith("zh") ? "zh" : "en"];
  return (k) => dict[k] ?? k;
}

/** 季节中文名（daynight.ts 的 season id → 显示名）。 */
export const SEASON_NAMES: Record<string, { zh: string; en: string }> = {
  spring: { zh: "春芽", en: "Spring bud" },
  summer: { zh: "夏碧", en: "Summer verdure" },
  autumn: { zh: "秋赭", en: "Autumn ochre" },
  winter: { zh: "冬霜", en: "Winter frost" },
};