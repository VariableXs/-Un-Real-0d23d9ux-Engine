/**
 * AI-18 M-68 屏保时钟 — 空闲 N 分钟后的极简时钟屏保。
 *
 * 口径：
 * - 空闲检测：输入监听（pointer/key/wheel/touch，被动）重置计时；
 * - 时钟分钟级跳动（不渲染秒针，省电省眼）；
 * - 任意输入退出（≤100ms —— 事件即退）；视频壁纸播放中不触发；
 * - 模式 off / clock / black；默认 off（现状）。
 */

import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import type { Settings } from "../../lib/settings";

export function ScreensaverClock(props: { settings: Settings }): React.ReactElement | null {
  const { t, lang } = useI18n();
  const mode = props.settings.ambience.screensaver;
  const idleMin = props.settings.ambience.screensaverIdleMin;
  const videoWallpaper = props.settings.wallpaperMode === "video";
  const [active, setActive] = useState(false);
  const [now, setNow] = useState(() => new Date());
  const lastInput = useRef<number>(Date.now());
  const activeRef = useRef(false);

  // 输入监听（复用打字降级的被动监听思路；不阻断不读取内容）
  useEffect(() => {
    if (mode === "off") return undefined;
    const touch = (): void => {
      lastInput.current = Date.now();
      if (activeRef.current) {
        activeRef.current = false;
        setActive(false);
      }
    };
    const events = ["pointerdown", "pointermove", "keydown", "wheel", "touchstart"] as const;
    for (const e of events) window.addEventListener(e, touch, { passive: true, capture: true });
    return () => {
      for (const e of events) window.removeEventListener(e, touch, true);
    };
  }, [mode]);

  // 空闲轮询（10s 粒度足够；进入后时钟每 30s 对齐分钟跳动）
  useEffect(() => {
    if (mode === "off") return undefined;
    const id = window.setInterval(() => {
      const idleMs = Date.now() - lastInput.current;
      // 视频壁纸播放中不触发（全景书红线）
      if (!activeRef.current && !videoWallpaper && idleMs >= idleMin * 60_000) {
        activeRef.current = true;
        setActive(true);
      }
      if (activeRef.current) setNow(new Date());
    }, 10_000);
    return () => window.clearInterval(id);
  }, [mode, idleMin, videoWallpaper]);

  if (mode === "off" || !active) return null;

  const locale = lang === "en" ? "en-US" : "zh-CN";
  const time = now.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hour12: false });
  const date = now.toLocaleDateString(locale, { weekday: "long", year: "numeric", month: "long", day: "numeric" });

  return (
    <div className={`ai18-screensaver${mode === "black" ? " black" : ""}`} data-testid="ai18-screensaver" role="presentation">
      {mode === "clock" && (
        <>
          <div className="ai18-ss-time">{time}</div>
          <div className="ai18-ss-date">{date}</div>
        </>
      )}
      <div className="ai18-ss-hint">{t("amb18SsHint")}</div>
    </div>
  );
}
