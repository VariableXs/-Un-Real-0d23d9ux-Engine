import { describe, it, expect, beforeEach } from "vitest";
import {
  TOUR_STEPS, TIPS, needsTour, markTourDone, resetTour,
  shouldShowCoach, markCoachSeen, resetAllCoaches, tipOfDay,
} from "../onboarding";

describe("AI-17 onboarding（U-57）", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("导览 5 站固定顺序（任务栏→开始菜单→VWM→文件管理器→设置）", () => {
    expect(TOUR_STEPS.map((s) => s.id)).toEqual(["taskbar", "startmenu", "vwm", "files", "settings"]);
  });

  it("默认需要导览；完成后不再出现", () => {
    expect(needsTour()).toBe(true);
    markTourDone();
    expect(needsTour()).toBe(false);
  });

  it("重看导览：reset 后 needsTour 再次为 true", () => {
    markTourDone();
    resetTour();
    expect(needsTour()).toBe(true);
  });

  it("coach mark 每提示只出现一次", () => {
    expect(shouldShowCoach("drag-snap")).toBe(true);
    markCoachSeen("drag-snap");
    expect(shouldShowCoach("drag-snap")).toBe(false);
    expect(shouldShowCoach("other")).toBe(true);
  });

  it("重置后全部提示重新出现", () => {
    markCoachSeen("a");
    markCoachSeen("b");
    resetAllCoaches();
    expect(shouldShowCoach("a")).toBe(true);
    expect(shouldShowCoach("b")).toBe(true);
  });

  it("技巧库 30 条，tipOfDay 按日轮换且稳定", () => {
    expect(TIPS.length).toBe(30);
    const d1 = new Date("2026-09-08T10:00:00Z");
    expect(tipOfDay(d1)).toEqual(tipOfDay(d1)); // 同日稳定
    const next = new Date(d1.getTime() + 86_400_000);
    expect(tipOfDay(next)).not.toEqual(tipOfDay(d1)); // 跨天轮转
  });
});
