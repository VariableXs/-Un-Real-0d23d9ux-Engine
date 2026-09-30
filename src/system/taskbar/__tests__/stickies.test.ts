import { beforeEach, describe, expect, it } from "vitest";
import {
  addSticky, clearUnpinned, getStickiesSnapshot, pinSticky, removeSticky,
  restoreStickies, STICKY_FADE_MS, STICKY_MAX, tickStickies, type Sticky,
} from "../stickies";

/** M-17：便签速贴（10 分钟淡隐 500ms 动画；钉住恢复；≤20 上限）。 */
function snap(): Sticky[] {
  return getStickiesSnapshot().items;
}

describe("stickies (M-17)", () => {
  beforeEach(() => {
    for (const s of snap()) removeSticky(s.id);
  });

  it("新增便签：默认未钉、分配四角、10 分钟后开始淡隐", () => {
    const s = addSticky("记点事", 1000);
    expect(s).not.toBeNull();
    expect(s!.pinned).toBe(false);
    expect(s!.fadeAt).toBe(1000 + 10 * 60 * 1000);
    expect(s!.corner).toBe("br");
  });

  it("空白文本拒绝", () => {
    expect(addSticky("   ", 0)).toBeNull();
  });

  it("≤20 条上限：超限如实拒绝", () => {
    for (let i = 0; i < STICKY_MAX; i++) addSticky(`n${i}`, i + 1);
    expect(snap().length).toBe(STICKY_MAX);
    expect(addSticky("overflow", 99999)).toBeNull();
  });

  it("淡隐推进：到时 fading，动画结束后移除；钉住项永不淡隐", () => {
    const s = addSticky("temp", 0)!;
    const fadeAt = s.fadeAt!;
    tickStickies(fadeAt - 1);
    expect(snap()[0]!.fading).toBe(false);
    tickStickies(fadeAt + 1);
    expect(snap()[0]!.fading).toBe(true);
    expect(snap().length).toBe(1);
    tickStickies(fadeAt + STICKY_FADE_MS + 1);
    expect(snap().length).toBe(0);
  });

  it("钉住后 fadeAt 清空且不淡隐", () => {
    const s = addSticky("keep", 0)!;
    pinSticky(s.id);
    const cur = snap()[0]!;
    expect(cur.pinned).toBe(true);
    expect(cur.fadeAt).toBeNull();
    tickStickies(Date.now() + 999_999_999);
    expect(snap().length).toBe(1);
  });

  it("restoreStickies 只恢复钉住项", () => {
    const a = addSticky("pinned", 0)!;
    pinSticky(a.id);
    addSticky("unpinned", 1);
    const raw = snap().map((s) => ({ id: s.id, text: s.text, pinned: s.pinned, corner: s.corner, createdAt: s.createdAt }));
    const pinnedRaw = raw.filter((r) => r.pinned);
    // 先清空运行态（removeSticky 会 persist 覆盖），再写入持久化样本后恢复
    for (const s of snap()) removeSticky(s.id);
    try {
      localStorage.setItem("variable:stickies:v1", JSON.stringify(pinnedRaw));
    } catch { /* node env ok */ }
    restoreStickies(0);
    const restored = snap();
    expect(restored.length).toBe(1);
    expect(restored[0]!.text).toBe("pinned");
    expect(restored[0]!.pinned).toBe(true);
  });

  it("clearUnpinned 清未钉住、留钉住", () => {
    const a = addSticky("a", 0)!;
    addSticky("b", 1);
    pinSticky(a.id);
    clearUnpinned();
    expect(snap().length).toBe(1);
    expect(snap()[0]!.text).toBe("a");
  });
});
