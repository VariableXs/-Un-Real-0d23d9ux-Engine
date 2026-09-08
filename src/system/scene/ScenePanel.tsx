/**
 * N-11 实况场景设置面板（ScenePanel）。
 *
 * 开启方式：壁纸工坊页脚入口，或事件 ai04:open-feature {feature:"scene-settings"}。
 * 只写 localStorage（variable:scene:*）并派发 variable:scene-changed，
 * 渲染层（WeatherLayer）即时响应 —— 不写 settings.ts（禁改）。
 *
 * 诚实边界：天气为手工模式（用户自选），零网络；HTTP 气象源必须在
 * netconsent.rs 网络同意框架内接入，属后续车道 —— 本面板不做任何请求。
 *
 * 自挂载：模块加载即监听 ai04:open-feature；集成阶段动态 import 本模块即激活。
 */

import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { mountOnEvent, installCloseHandler, dispatchClose } from "../wallpaper/mount";
import { seasonAccent } from "./daynight";
import { SEASON_NAMES, sceneT } from "./labels";
import {
  cycleWeather,
  flagOn,
  loadWeather,
  SCENE_CHANGED_EVENT,
  SCENE_DAYNIGHT,
  SCENE_ENABLED,
  SCENE_INTERACTIVE,
  SCENE_SEASON,
  sceneEnabled,
} from "./weatherState";

function setFlag(key: string, on: boolean): void {
  try {
    localStorage.setItem(key, on ? "1" : "0");
  } catch {
    /* 隐私模式：设置仅会话内生效 */
  }
  window.dispatchEvent(new CustomEvent(SCENE_CHANGED_EVENT));
}

export function ScenePanel(): React.ReactElement {
  const t = useMemo(() => sceneT(), []);
  const [on, setOn] = useState(() => sceneEnabled());
  const [weather, setWeather] = useState(() => loadWeather());
  const [interactive, setInteractive] = useState(() => flagOn(SCENE_INTERACTIVE));
  const [daynight, setDaynight] = useState(() => flagOn(SCENE_DAYNIGHT));
  const [season, setSeason] = useState(() => flagOn(SCENE_SEASON));
  const accent = useMemo(() => seasonAccent(new Date()), []);

  // Esc 关闭（ overlay 自身职责，不经全局监听）
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") dispatchClose("scene-settings");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const weatherNames: Record<string, string> = {
    off: t("wOff"),
    sunny: t("wSunny"),
    rain: t("wRain"),
    snow: t("wSnow"),
    "fall-leaves": t("wLeaves"),
  };

  return createPortal(
    <div className="scene-panel-root" role="dialog" aria-label={t("title")}>
      <div className="scene-panel">
        <header className="scene-panel-head">
          <h3>{t("title")}</h3>
          <button className="wp-studio-close" onClick={() => dispatchClose("scene-settings")}>✕</button>
        </header>

        <label className="scene-row">
          <input
            type="checkbox"
            checked={on}
            onChange={(e) => {
              setOn(e.target.checked);
              setFlag(SCENE_ENABLED, e.target.checked);
            }}
          />
          <span>{t("enable")}</span>
        </label>

        <div className="scene-row">
          <span>{t("weather")}</span>
          <button
            disabled={!on}
            onClick={() => {
              const next = cycleWeather(weather, 1);
              setWeather(next);
              try {
                localStorage.setItem("variable:scene:weather", next);
              } catch {
                /* 忽略 */
              }
              window.dispatchEvent(new CustomEvent(SCENE_CHANGED_EVENT));
            }}
          >
            {weatherNames[weather] ?? weather} ›
          </button>
        </div>

        <label className="scene-row">
          <input
            type="checkbox"
            checked={interactive}
            onChange={(e) => {
              setInteractive(e.target.checked);
              setFlag(SCENE_INTERACTIVE, e.target.checked);
            }}
          />
          <span>{t("interactive")}</span>
        </label>

        <label className="scene-row">
          <input
            type="checkbox"
            checked={daynight}
            onChange={(e) => {
              setDaynight(e.target.checked);
              setFlag(SCENE_DAYNIGHT, e.target.checked);
            }}
          />
          <span>{t("daynight")}</span>
        </label>

        <label className="scene-row">
          <input
            type="checkbox"
            checked={season}
            onChange={(e) => {
              setSeason(e.target.checked);
              setFlag(SCENE_SEASON, e.target.checked);
            }}
          />
          <span>
            {t("season")}（{SEASON_NAMES[accent.season]?.zh ?? accent.season} {accent.hueShift > 0 ? "+" : ""}
            {accent.hueShift}°）
          </span>
        </label>

        <p className="scene-note">{t("boundary")}</p>
      </div>
    </div>,
    document.body,
  );
}

/* 模块加载即监听（主控集成阶段动态 import 本模块即激活）。 */
if (typeof window !== "undefined") {
  mountOnEvent(window, "scene-settings", async () => ({ Overlay: ScenePanel }));
  installCloseHandler(window, "scene-settings");
}