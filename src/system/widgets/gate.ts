/**
 * N-09 刷新 gate：全屏（Tauri 事件 sys://fullscreen，联动 N-03/C-5）与低电
 * （navigator.getBattery，level<0.2 且未充电；浏览器无该 API 时视为不暂停）时
 * 暂停所有组件刷新回调。reduce-motion 单独暴露（A-2.3 动效降级）。
 */
import { useEffect, useState } from "react";

export interface WidgetGate {
  /** true = 全屏或低电，组件应暂停刷新回调（数据通道照常，仅省渲染/轮询）。 */
  paused: boolean;
  reason: "fullscreen" | "battery" | null;
  reduceMotion: boolean;
}

export function useWidgetGate(): WidgetGate {
  const [fs, setFs] = useState(false);
  const [low, setLow] = useState(false);
  const [reduceMotion, setReduceMotion] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let alive = true;
    // Tauri 全屏信号（动态 import，纯浏览器 dev 环境优雅缺席）
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<boolean>("sys://fullscreen", (e) => {
          if (alive) setFs(e.payload === true);
        }),
      )
      .then((un) => {
        if (un) unlisten = () => { void Promise.resolve(un()).catch(() => {}); };
      })
      .catch(() => {});

    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onMq = (): void => setReduceMotion(mq.matches);
    onMq();
    mq.addEventListener?.("change", onMq);

    // 低电轮询（Battery API 缺席 = 桌面机，不暂停）
    let timer = 0;
    const checkBattery = (): void => {
      const nav = navigator as Navigator & { getBattery?: () => Promise<{ level: number; charging: boolean }> };
      if (typeof nav.getBattery !== "function") return;
      void nav
        .getBattery()
        .then((b) => setLow(b.level < 0.2 && !b.charging))
        .catch(() => {});
    };
    checkBattery();
    timer = window.setInterval(checkBattery, 60_000);

    return () => {
      alive = false;
      unlisten?.();
      mq.removeEventListener?.("change", onMq);
      window.clearInterval(timer);
    };
  }, []);

  return { paused: fs || low, reason: fs ? "fullscreen" : low ? "battery" : null, reduceMotion };
}