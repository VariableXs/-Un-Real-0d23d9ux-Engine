/**
 * N-11 天气状态（纯逻辑 + localStorage 键位）。
 *
 * 诚实边界：天气默认「手工模式」，用户自选当日天气，零网络
 * （功能全景 L899）；HTTP 气象源完全不在本车道实现，netconsent.rs
 * 网络同意框架接入属后续车道 —— 代码里没有任何 fetch/XHR。
 */

export type Weather = "off" | "sunny" | "rain" | "snow" | "fall-leaves";

/** 循环顺序（工坊/面板 UI 的切换顺序 = 状态机转移表）。 */
export const WEATHER_CYCLE: readonly Weather[] = ["off", "sunny", "rain", "snow", "fall-leaves"];

export const SCENE_ENABLED = "variable:scene:enabled";
export const SCENE_WEATHER = "variable:scene:weather";
export const SCENE_DAYNIGHT = "variable:scene:daynight";
export const SCENE_SEASON = "variable:scene:season";
export const SCENE_INTERACTIVE = "variable:scene:interactive";
/** 场景配置变化事件（面板 → 渲染层即时刷新）。 */
export const SCENE_CHANGED_EVENT = "variable:scene-changed";

/** 脏值兜底：未知/空值一律 "off"（默认行为 = 现状）。 */
export function normalizeWeather(raw: string | null | undefined): Weather {
  const v = (raw ?? "").trim().toLowerCase();
  return (WEATHER_CYCLE as readonly string[]).includes(v) ? (v as Weather) : "off";
}

/** 状态机转移：沿 WEATHER_CYCLE 循环步进（dir=1 下一档 / -1 上一档）。 */
export function cycleWeather(current: Weather, dir: 1 | -1): Weather {
  const idx = WEATHER_CYCLE.indexOf(current);
  const next = (idx + dir + WEATHER_CYCLE.length) % WEATHER_CYCLE.length;
  return WEATHER_CYCLE[next];
}

/** 场景总开关（默认关闭，规格口径）。 */
export function sceneEnabled(): boolean {
  try {
    return localStorage.getItem(SCENE_ENABLED) === "1";
  } catch {
    return false;
  }
}

export function loadWeather(): Weather {
  try {
    return normalizeWeather(localStorage.getItem(SCENE_WEATHER));
  } catch {
    return "off";
  }
}

export function flagOn(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}