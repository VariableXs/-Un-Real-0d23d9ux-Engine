import { beforeEach, describe, expect, it } from "vitest";
import {
  canLaunch, getPendingItems, LAUNCH_DEBOUNCE_MS, LAUNCH_TIMEOUT_MS,
  markLaunch, pendingPhase, settleLaunch,
} from "../pending";

/** M-13：等待态（800ms 防抖绝不重复 ShellExecute；8s 如实转「仍在启动」）。 */
describe("taskbar pending (M-13)", () => {
  beforeEach(() => {
    for (const k of getPendingItems(0)) settleLaunch(k.key);
  });

  it("首次启动允许；800ms 内重复被拦截", () => {
    expect(canLaunch("app:write", 1000)).toBe(true);
    markLaunch("app:write", "写作空间", 1000);
    expect(canLaunch("app:write", 1000 + LAUNCH_DEBOUNCE_MS - 1)).toBe(false);
    expect(canLaunch("app:write", 1000 + LAUNCH_DEBOUNCE_MS)).toBe(true);
  });

  it("settleLaunch 撤占位后立即可再启动", () => {
    markLaunch("tp:wx", "微信", 0);
    settleLaunch("tp:wx");
    expect(canLaunch("tp:wx", 1)).toBe(true);
  });

  it("pendingPhase：<8s = pending；≥8s = slow（如实转文案）", () => {
    expect(pendingPhase(0, LAUNCH_TIMEOUT_MS - 1)).toBe("pending");
    expect(pendingPhase(0, LAUNCH_TIMEOUT_MS)).toBe("slow");
  });

  it("不同 key 互不影响", () => {
    markLaunch("app:write", "写作", 0);
    expect(canLaunch("app:mind", 0)).toBe(true);
  });
});
