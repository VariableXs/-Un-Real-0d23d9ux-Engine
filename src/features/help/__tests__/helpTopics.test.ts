import { describe, it, expect } from "vitest";
import { HELP_TOPICS, topicForContext, searchHelp } from "../helpTopics";

describe("AI-17 helpTopics（Z-70）", () => {
  it("主题双语完整（zh/en 标题与正文非空）", () => {
    for (const t of HELP_TOPICS) {
      expect(t.zh.title.trim()).toBeTruthy();
      expect(t.zh.body.trim()).toBeTruthy();
      expect(t.en.title.trim()).toBeTruthy();
      expect(t.en.body.trim()).toBeTruthy();
    }
  });

  it("F1 上下文命中正确主题", () => {
    expect(topicForContext("desktop-wallpaper")?.id).toBe("desktop");
    expect(topicForContext("vwm-window")?.id).toBe("windows");
    expect(topicForContext("files-tab")?.id).toBe("files");
    expect(topicForContext("zzz-unknown")).toBeNull();
  });

  it("搜索命中并按评分排序，空查询返回空", () => {
    expect(searchHelp("")).toEqual([]);
    const hits = searchHelp("壁纸 wallpaper");
    expect(hits.length).toBeGreaterThan(0);
    expect(hits[0]!.score).toBeGreaterThanOrEqual(hits[1]?.score ?? 0);
    const en = searchHelp("recycle");
    expect(en.length).toBeGreaterThan(0);
  });

  it("搜索响应覆盖中英双语正文", () => {
    expect(searchHelp("分屏")?.[0]?.topic.id).toBe("windows");
    expect(searchHelp("snap")?.[0]?.topic.id).toBe("windows");
  });
});
