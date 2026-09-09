/**
 * AI-18 AmbienceRuntime — 氛围组运行时编排器（无 UI；全部副作用集中于此）。
 *
 * 职责：
 * - V-74/75/76/77/78、M-71 档位 → documentElement data 属性 / CSS 变量注入；
 * - V-72 壁纸滤镜 → --wp-sat / --wp-bright；
 * - N-33 情绪引擎：每分钟计算情绪 → 保守偏移 → --mood-bright/--mood-sat（120s 缓动由 CSS transition 兜底）；
 * - U-54 节律助手：30s 轮询 computeDueRhythms → badge/count/notify 三种提醒；
 * - M-65 昼夜壁纸组：每分钟 slot 判定 → 目录壁纸切换（onPatchSettings）；
 * - V-71 精选轮换：本地日界换一张内置 SVG 精选（与昼夜组互斥）；
 * - M-67 纯净模式：Ctrl+Alt+P 三层隐藏 + 边缘小点角标；
 * - U-49 音景睡眠定时；
 * - M-72 退出快照（persistAmbientSnapshot 由 DesktopShell 退出路径调用）。
 *
 * 红线：全部功能默认关；任何异常静默回落（氛围绝不让用户看到错误）。
 */

import { useEffect, useRef } from "react";
import type { Settings } from "../../lib/settings";
import { pushToast } from "../../state/uiStore";
import { pushNotify } from "../../state/notifyStore";
import { useI18n } from "../../i18n";
import { computeMood, moodToOffsets, offsetsInEnvelope } from "../../lib/moodEngine";
import { computeDueRhythms, type RhythmStateMap } from "./rhythm";
import { slotOfHour, slotConfigured } from "./dayAround";
import { CURATED_WALLPAPERS, dailyIndex } from "./curated";
import { stopScene, activeScenes } from "../../lib/soundscape";
import { createStore } from "../../lib/store";

/** 节律状态共享（RhythmCenter 只读渲染）。 */
export const rhythmStore = createStore<{ state: RhythmStateMap; tick: number }>({
  state: {},
  tick: 0,
});

/** 纯净模式共享（Taskbar/DesktopIcons/CSS 读取）。 */
export const pureStore = createStore<{ active: boolean }>({ active: false });

export function AmbienceRuntime(props: {
  settings: Settings;
  onPatchSettings: (patch: Partial<Settings>) => void;
}): null {
  const { t } = useI18n();
  const amb = props.settings.ambience;
  const ambRef = useRef(amb);
  ambRef.current = amb;
  const tRef = useRef(t);
  tRef.current = t;

  // ---- 档位注入（V-74/75/76/77/78、M-71、V-72、M-64 accent）----
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.hover = amb.hoverLatency;
    root.dataset.density = amb.uiDensity;
    root.dataset.radius = amb.uiRadius;
    root.dataset.focusRing = amb.focusRing;
    root.dataset.iconTier = String(amb.iconTier);
    root.dataset.pureMode = pureStore.getState().active ? "on" : "off";
    root.style.setProperty("--ui-font", amb.uiFont || "");
    root.style.setProperty("--wp-sat", String(amb.wallpaperFilter.saturation / 100));
    root.style.setProperty("--wp-bright", String(amb.wallpaperFilter.brightness / 100));
  }, [amb.hoverLatency, amb.uiDensity, amb.uiRadius, amb.focusRing, amb.iconTier, amb.uiFont, amb.wallpaperFilter]);

  // ---- N-33 情绪引擎：每分钟计算 + 注入保守偏移 ----
  useEffect(() => {
    const apply = (): void => {
      const a = ambRef.current;
      const root = document.documentElement;
      if (!a.mood.enabled) {
        root.style.removeProperty("--mood-bright");
        root.style.removeProperty("--mood-sat");
        return;
      }
      const mood = computeMood({
        hour: new Date().getHours(),
        scene: "neutral",
        app: "neutral",
        manualArousal: a.mood.manualArousal,
        manualFocus: a.mood.manualFocus,
      });
      const off = moodToOffsets(mood, root.dataset.theme === "high-contrast");
      if (!offsetsInEnvelope(off)) return; // 安全阀：越界整体停用本轮
      root.style.setProperty("--mood-bright", String(1 + off.brightness));
      root.style.setProperty("--mood-sat", String(1 + off.saturation));
    };
    apply();
    const id = window.setInterval(apply, 60_000);
    return () => window.clearInterval(id);
  }, [amb.mood.enabled, amb.mood.manualArousal, amb.mood.manualFocus]);

  // ---- U-54 节律助手：30s 轮询 ----
  useEffect(() => {
    const poll = (): void => {
      const a = ambRef.current;
      const nowMono = performance.now();
      const hour = new Date().getHours();
      const { due, next } = computeDueRhythms(a, nowMono, hour, false, rhythmStore.getState().state);
      rhythmStore.setState({ state: next, tick: rhythmStore.getState().tick + 1 });
      for (const d of due) {
        const label = tRef.current(`amb18Rhythm_${d.kind}`);
        if (d.notify === "notify") {
          pushNotify("reminder", label, tRef.current("amb18RhythmBody"));
        } else if (d.notify === "badge") {
          pushToast("info", `${label} · ${tRef.current("amb18RhythmDue")}`);
        }
        // count：静默计数，不打扰
      }
    };
    const id = window.setInterval(poll, 30_000);
    return () => window.clearInterval(id);
  }, []);

  // ---- M-65 昼夜壁纸组 + V-71 精选轮换（互斥；每分钟检查）----
  const lastSlot = useRef<string>("");
  useEffect(() => {
    const tick = (): void => {
      const a = ambRef.current;
      const now = new Date();
      if (a.dayAround.enabled) {
        const slot = slotOfHour(now.getHours(), a.dayAround.boundaries);
        if (slot !== lastSlot.current) {
          lastSlot.current = slot;
          const dir = a.dayAround.dirs[slot];
          if (slotConfigured(a.dayAround.dirs, slot) && dir) {
            props.onPatchSettings({
              wallpaperMode: "living",
              customBg: { ...props.settings.customBg, imagePath: dir },
            });
          }
        }
        return;
      }
      if (a.curated.on) {
        const key = `curated:${dailyIndex(now, CURATED_WALLPAPERS.length)}`;
        if (key !== lastSlot.current) {
          lastSlot.current = key;
          const wp = CURATED_WALLPAPERS[dailyIndex(now, CURATED_WALLPAPERS.length)];
          if (wp) {
            props.onPatchSettings({
              wallpaperMode: "living",
              customBg: { ...props.settings.customBg, imagePath: wp.svg },
            });
          }
        }
      }
    };
    tick();
    const id = window.setInterval(tick, 60_000);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [amb.dayAround.enabled, amb.curated.on]);

  // ---- M-67 纯净模式：Ctrl+Alt+P ----
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.ctrlKey && e.altKey && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        const active = !pureStore.getState().active;
        pureStore.setState({ active });
        document.documentElement.dataset.pureMode = active ? "on" : "off";
        pushToast("info", t(active ? "amb18PureOn" : "amb18PureOff"));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [t]);

  // ---- U-49 音景睡眠定时 ----
  useEffect(() => {
    if (amb.soundscapeSleepMin <= 0) return undefined;
    const id = window.setTimeout(() => {
      const a = ambRef.current;
      for (const sc of activeScenes()) stopScene(sc, a.soundscapeVolumes[sc] ?? 0.5);
    }, amb.soundscapeSleepMin * 60_000);
    return () => window.clearTimeout(id);
  }, [amb.soundscapeSleepMin]);

  return null;
}
