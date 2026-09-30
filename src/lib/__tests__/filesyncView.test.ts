/**
 * 跨域文件级同步 · 视图侧纯逻辑测试（双域 ③-a）。
 *
 * 这些用例钉住的都是**用户能感知的行为**，不是实现细节：
 *   - 最新改动排在列表最前（不是字典序）
 *   - 超过上限只显示最新的 N 项，并如实报告总数
 *   - 删除项被补成完整条目（视图层不必写第二套分支）
 *   - 未接盘 / 已接盘 / 被截断 三态文案各不相同
 *   - 事件名与后端常量逐字一致（改名必须两边同步，否则界面永不刷新）
 */
import { describe, expect, it } from "vitest";
import {
  EMPTY_STATUS,
  FALLBACK_POLL_MS,
  RECENT_LIMIT,
  SYNC_EVENT,
  changeLine,
  idleHint,
  mergeDiff,
  statusHint,
  statusTitle,
} from "../filesyncView";
import type { Shell } from "../ipc";

function ent(rel: string, size = 0, dir = false): Shell.SyncEntry {
  return { rel, dir, size, mtimeMs: 1_700_000_000_000 };
}

describe("filesyncView · 契约常量", () => {
  it("事件名与后端逐字一致（改名必须两边同步）", () => {
    // 后端：src-tauri/src/shell/filesync.rs → pub const SYNC_EVENT
    expect(SYNC_EVENT).toBe("filesync://changed");
  });

  it("兜底轮询比后端 1 秒主路慢，且不至于让界面看起来卡死", () => {
    expect(FALLBACK_POLL_MS).toBeGreaterThan(1000);
    expect(FALLBACK_POLL_MS).toBeLessThanOrEqual(10_000);
  });

  it("展示上限是个能装下几次连续操作的合理值", () => {
    expect(RECENT_LIMIT).toBeGreaterThanOrEqual(3);
    expect(RECENT_LIMIT).toBeLessThanOrEqual(20);
  });
});

describe("filesyncView · mergeDiff", () => {
  it("空差异得到空列表与 0 总数", () => {
    const r = mergeDiff({ changed: [], removed: [] });
    expect(r.rows).toEqual([]);
    expect(r.total).toBe(0);
  });

  it("只算改动时，列表与总数同步", () => {
    const r = mergeDiff({ changed: [ent("a.txt", 3)], removed: [] });
    expect(r.total).toBe(1);
    expect(r.rows).toHaveLength(1);
    expect(r.rows[0]!.kind).toBe("changed");
    expect(r.rows[0]!.entry.rel).toBe("a.txt");
  });

  it("删除项被补成完整条目（rel 保留，其余字段补零）", () => {
    const r = mergeDiff({ changed: [], removed: ["gone.txt"] });
    expect(r.total).toBe(1);
    const row = r.rows[0]!;
    expect(row.kind).toBe("removed");
    expect(row.entry.rel).toBe("gone.txt");
    // 视图层不该被迫处理 undefined 字段。
    expect(row.entry.dir).toBe(false);
    expect(row.entry.size).toBe(0);
    expect(typeof row.entry.mtimeMs).toBe("number");
  });

  it("最新变动排最前（改动在删除之后入列，反转为最新优先）", () => {
    const r = mergeDiff({
      changed: [ent("first.txt", 1), ent("second.txt", 2)],
      removed: ["third.txt"],
    });
    // 输入顺序 = 改动在前、删除在后 → 反转后删除最前，改动倒序跟随。
    expect(r.rows.map((x) => x.entry.rel)).toEqual(["third.txt", "second.txt", "first.txt"]);
  });

  it("超出上限只保留最新的 N 项，但总数如实报告", () => {
    const changed = Array.from({ length: RECENT_LIMIT + 4 }, (_, i) => ent(`f${i}.txt`, i));
    const r = mergeDiff({ changed, removed: [] });
    expect(r.total).toBe(RECENT_LIMIT + 4);
    expect(r.rows).toHaveLength(RECENT_LIMIT);
    // 保留的是「最后入列的」那批（即最新的），丢弃最早入列的。
    expect(r.rows[0]!.entry.rel).toBe(`f${RECENT_LIMIT + 3}.txt`);
    expect(r.rows.map((x) => x.entry.rel)).not.toContain("f0.txt");
  });

  it("自定义上限生效（面板可在小窗口里收窄展示）", () => {
    const changed = [ent("a"), ent("b"), ent("c")];
    expect(mergeDiff({ changed, removed: [] }, 2).rows).toHaveLength(2);
    expect(mergeDiff({ changed, removed: [] }, 2).total).toBe(3);
  });

  it("不修改传入的 diff 数组（调用方可能复用同一对象）", () => {
    const changed = [ent("a"), ent("b")];
    const snapshot = changed.map((e) => e.rel);
    mergeDiff({ changed, removed: [] });
    expect(changed.map((e) => e.rel)).toEqual(snapshot);
  });
});

describe("filesyncView · changeLine", () => {
  it("文件显示体积，格式与全仓统一格式化器一致", () => {
    expect(changeLine({ entry: ent("a.txt", 2048), kind: "changed" })).toBe("更新 a.txt · 2.0 KB");
  });

  it("目录不显示 0 字节（会误导成空目录）", () => {
    expect(changeLine({ entry: ent("docs", 0, true), kind: "changed" })).toBe("更新 docs");
  });

  it("删除行以「移除」开头且不带体积", () => {
    expect(changeLine({ entry: ent("old.txt", 4096), kind: "removed" })).toBe("移除 old.txt");
  });
});

describe("filesyncView · 三态文案", () => {
  it("未接盘：标题说未接、提示告诉用户怎么接上", () => {
    expect(statusTitle(EMPTY_STATUS)).toBe("未接共享盘");
    expect(statusHint(EMPTY_STATUS)).toContain("U 盘");
    // 未接盘时说「N 个条目正在受管」是撒谎。
    expect(statusHint(EMPTY_STATUS)).not.toContain("正在受管");
  });

  it("已接盘：标题报告同步中、副标题给出受管条目数", () => {
    const st: Shell.SyncStatus = { available: true, root: "W:\\", entries: 42, truncated: false };
    expect(statusTitle(st)).toContain("实时同步中");
    expect(statusHint(st)).toContain("42");
  });

  it("被截断：标题显性标注，不让用户以为看到的是全量", () => {
    const st: Shell.SyncStatus = { available: true, root: "W:\\", entries: 20_000, truncated: true };
    expect(statusTitle(st)).toContain("上限");
    expect(statusTitle(st)).toContain("实时同步中");
  });

  it("空闲提示告诉用户怎么触发一次同步，而不是干等", () => {
    expect(idleHint()).toContain("Windows");
    expect(idleHint().length).toBeGreaterThan(8);
  });
});
