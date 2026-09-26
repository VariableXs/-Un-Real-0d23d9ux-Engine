import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import { closeClick, declaredBehavior, overflowCompat, quitFromTray, setBehavior, taskbarButtons, updateActivity, type TrayApp, type TrayRuntime } from "../f368-minimizeToTray";

function rt(): TrayRuntime {
  return { resident: [], gone: [] };
}

const DOWNLOADER: TrayApp = { appId: "downloader", behavior: "closeToTray", activityPct: 30 };
const EDITOR: TrayApp = { appId: "editor", behavior: "normal", activityPct: null };

describe("F368 最小化到托盘", () => {
  it("声明行为与悬停提示：声明者 ×→托盘并提示「最小化到托盘」；未声明者真关", () => {
    const r = closeClick(rt(), DOWNLOADER);
    expect(r.windowGoesTo).toBe("tray");
    expect(r.tooltip).toBe("最小化到托盘");
    expect(r.runtime.resident.map((x) => x.appId)).toEqual(["downloader"]);
    const r2 = closeClick(rt(), EDITOR);
    expect(r2.windowGoesTo).toBe("taskbar");
    expect(r2.tooltip).toBeNull();
    expect(r2.runtime.gone).toEqual(["editor"]);
  });

  it("重复点 × 幂等：已在托盘不重复入册", () => {
    let r = closeClick(rt(), DOWNLOADER).runtime;
    r = closeClick({ ...r, gone: [] }, DOWNLOADER).runtime;
    expect(r.resident).toHaveLength(1);
  });

  it("徽标实时性：进度钳制 0-100、整数化；完成置 null 清徽标", () => {
    let r = closeClick(rt(), DOWNLOADER).runtime;
    r = updateActivity(r, "downloader", 55.6);
    expect(r.resident[0]!.activityPct).toBe(56);
    r = updateActivity(r, "downloader", 150);
    expect(r.resident[0]!.activityPct).toBe(100);
    r = updateActivity(r, "downloader", null);
    expect(r.resident[0]!.activityPct).toBeNull();
  });

  it("真退出路径：托盘右键「退出」出清且记 gone；对不在托盘的应用无害", () => {
    let r = closeClick(rt(), DOWNLOADER).runtime;
    r = quitFromTray(r, "downloader");
    expect(r.resident).toHaveLength(0);
    expect(r.gone).toContain("downloader");
    r = quitFromTray(r, "ghost");
    expect(r.gone.filter((g) => g === "ghost")).toHaveLength(1);
  });

  it("任务栏不显示判据：托盘化应用从任务栏按钮表消失", () => {
    const r = closeClick(rt(), DOWNLOADER).runtime;
    expect(taskbarButtons(r, ["downloader", "editor", "explorer"])).toEqual(["editor", "explorer"]);
    expect(taskbarButtons(rt(), ["downloader"])).toEqual(["downloader"]);
  });

  it("托盘溢出收纳兼容（F075）：容量外进溢出抽屉不消失", () => {
    let r = rt();
    for (let i = 0; i < 4; i++) r = closeClick(r, { appId: `app${i}`, behavior: "closeToTray", activityPct: null }).runtime;
    const split = overflowCompat(r, 2);
    expect(split.visible).toEqual(["app0", "app1"]);
    expect(split.overflowed).toEqual(["app2", "app3"]);
  });

  it("行为声明持久化：默认 normal、声明后 round-trip", () => {
    __clearMem();
    const s = memStore();
    expect(declaredBehavior("downloader", s)).toBe("normal");
    setBehavior("downloader", "closeToTray", s);
    expect(declaredBehavior("downloader", s)).toBe("closeToTray");
  });
});
