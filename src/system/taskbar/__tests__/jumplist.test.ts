import { beforeEach, describe, expect, it } from "vitest";
import { aggregateRecentForApp, windowsForApp, type VwmWinBrief } from "../jumplist";
import { pushRecent, clearRecent } from "../../startmenu/recent";

/** M-10 / U-15：跳转列表数据层（按 appKey 聚合最近记录 + 活动实例窗口）。 */
describe("jumplist (M-10 / U-15)", () => {
  beforeEach(() => {
    clearRecent();
  });

  it("按 appKey 过滤最近记录（同 key 只保留最新一条）并截断到上限", () => {
    pushRecent("app", "write", "写作空间");
    pushRecent("app", "mind", "思维导图");
    pushRecent("app", "write", "写作空间·新");
    const items = aggregateRecentForApp("write");
    expect(items.length).toBe(1);
    expect(items[0]!.name).toBe("写作空间·新");
    expect(aggregateRecentForApp("mind").length).toBe(1);
    expect(aggregateRecentForApp("tp:nowhere")).toEqual([]);
  });

  it("无记录返回空列表", () => {
    expect(aggregateRecentForApp("tp:nowhere")).toEqual([]);
  });

  it("windowsForApp 只取该应用窗口且 z 降序（点击聚焦最上层）", () => {
    const wins: VwmWinBrief[] = [
      { id: "w1", app: "write", title: "w1", z: 1 },
      { id: "w3", app: "write", title: "w3", z: 3 },
      { id: "m1", app: "mind", title: "m1", z: 5 },
      { id: "w2", app: "write", title: "w2", z: 2 },
    ];
    const mine = windowsForApp(wins, "write");
    expect(mine.map((w) => w.id)).toEqual(["w3", "w2", "w1"]);
  });
});
