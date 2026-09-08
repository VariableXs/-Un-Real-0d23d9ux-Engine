/**
 * AI-17 · 视觉语言组运行时（VisionRuntime）
 * 挂载：响应式上下文（U-12）/ 动效速度档（Z-67）/ 边缘热区（Z-69）/
 *       帮助中心（Z-70）/ 首次导览（U-57）。
 * 仅桌面窗口由 App.tsx 挂载一次。
 */
import { useEffect, useState } from "react";
import { initResponsiveContext } from "../../lib/breakpoints";
import { needsTour } from "../onboarding/onboarding";
import { SpotlightTour } from "../onboarding/SpotlightTour";
import { HelpCenter } from "../help/HelpCenter";
import { EdgeHotspots } from "./EdgeHotspots";

export const MOTION_SPEED_KEY = "vision.motionSpeed.v1"; // "0.5" | "1" | "1.5"

export function loadMotionSpeed(): "0.5" | "1" | "1.5" {
  try {
    const v = localStorage.getItem(MOTION_SPEED_KEY);
    return v === "0.5" || v === "1.5" ? v : "1";
  } catch {
    return "1";
  }
}

export function saveMotionSpeed(v: "0.5" | "1" | "1.5"): void {
  try {
    localStorage.setItem(MOTION_SPEED_KEY, v);
  } catch {
    /* 隐私模式：静默 */
  }
  applyMotionSpeed(v);
}

export function applyMotionSpeed(v: "0.5" | "1" | "1.5"): void {
  document.documentElement.dataset.motionSpeed = v;
}

export function VisionRuntime(): React.ReactElement {
  const [tourOpen, setTourOpen] = useState<boolean>(() => needsTour());

  useEffect(() => {
    const dispose = initResponsiveContext();
    applyMotionSpeed(loadMotionSpeed());
    return dispose;
  }, []);

  return (
    <>
      <EdgeHotspots />
      <HelpCenter />
      {tourOpen ? <SpotlightTour onDone={() => setTourOpen(false)} /> : null}
    </>
  );
}
