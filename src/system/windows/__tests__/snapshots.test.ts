import { describe, expect, it, beforeAll, beforeEach, afterEach } from "vitest";
import {
  saveSnapshot,
  listSnapshots,
  restoreSnapshot,
  deleteSnapshot,
  renameSnapshot,
  exportSnapshot,
  importSnapshot,
  displaySignature,
} from "../snapshots";
import { vwmStore, setVwmWorkArea, type VwmWin } from "../vwm";

/**
 * 批次W-4：布局快照 —— 保存/恢复回环、缺失项判定、按比例映射、导出导入。
 * node 环境：window / localStorage 均为最小 shim（与 vwm.test.ts 同策略）。
 */

const wa = { x: 0, y: 0, w: 1920, h: 1046 };

beforeAll(() => {
  const g = globalThis as {
    window?: unknown;
    localStorage?: Storage;
  };
  g.window ??= { setTimeout, screen: { width: 1920, height: 1080 }, devicePixelRatio: 1 };
  if (!g.localStorage) {
    const mem = new Map<string, string>();
    g.localStorage = {
      getItem: (k: string) => mem.get(k) ?? null,
      setItem: (k: string, v: string) => void mem.set(k, String(v)),
      removeItem: (k: string) => void mem.delete(k),
      clear: () => void mem.clear(),
      key: (i: number) => [...mem.keys()][i] ?? null,
      get length() {
        return mem.size;
      },
    } as Storage;
  }
});

afterEach(() => {
  // 恢复默认屏幕签名（比例映射用例会改）
  (globalThis as { window: { screen: { width: number; height: number } } }).window.screen = {
    width: 1920,
    height: 1080,
  };
});

function seedWins(wins: Partial<VwmWin>[]): void {
  vwmStore.setState((s) => ({
    wins: wins.map((w, i) => ({
      id: w.id ?? `w${i}`,
      app: w.app ?? "notes",
      path: w.path ?? null,
      x: w.x ?? 10,
      y: w.y ?? 20,
      w: w.w ?? 800,
      h: w.h ?? 600,
      state: w.state ?? "normal",
      minimized: w.minimized ?? false,
      z: w.z ?? i + 1,
      restore: null,
      group: null,
      groupActive: false,
    })),
    closing: [],
    flying: [],
    seq: s.seq + 1,
  }));
}

describe("W-4 布局快照", () => {
  beforeEach(() => {
    localStorage.clear();
    setVwmWorkArea(wa);
    seedWins([{ app: "notes", x: 100, y: 80, w: 700, h: 500, z: 3 }]);
  });

  it("保存 → 恢复几何一致", () => {
    saveSnapshot("work");
    seedWins([]);
    const r = restoreSnapshot("work");
    expect(r.restored).toBe(1);
    expect(r.missing).toEqual([]);
    const w = vwmStore.getState().wins[0]!;
    expect(w.app).toBe("notes");
    expect(w.x).toBe(100);
    expect(w.y).toBe(80);
    expect(w.w).toBe(700);
    expect(w.h).toBe(500);
  });

  it("第三方应用缺失 → 不自动启动并列入缺失项", () => {
    seedWins([{ app: "tp:nonexistent-app", x: 0, y: 0, w: 600, h: 400 }, { app: "calc" }]);
    saveSnapshot("with-tp");
    seedWins([]);
    const r = restoreSnapshot("with-tp");
    expect(r.missing).toContain("nonexistent-app");
    expect(r.restored).toBe(1); // calc 正常恢复
  });

  it("显示器签名不匹配 → 按比例映射（不丢出屏外）", () => {
    seedWins([{ app: "notes", x: 1600, y: 900, w: 700, h: 500 }]);
    saveSnapshot("big");
    // 模拟换到小屏：签名换成 960x540
    (globalThis as { window: { screen: { width: number; height: number } } }).window.screen = {
      width: 960,
      height: 540,
    };
    seedWins([]);
    const r = restoreSnapshot("big");
    expect(r.restored).toBe(1);
    const w = vwmStore.getState().wins[0]!;
    expect(w.x).toBeLessThanOrEqual(wa.w - 120);
    expect(w.y).toBeLessThanOrEqual(wa.y + wa.h - 48);
  });

  it("重命名 / 删除 / 导出导入", () => {
    saveSnapshot("a");
    expect(renameSnapshot("a", "b")).toBe(true);
    expect(listSnapshots().map((s) => s.name)).toEqual(["b"]);
    const json = exportSnapshot("b")!;
    expect(importSnapshot(json).ok).toBe(false); // 重名
    deleteSnapshot("b");
    expect(listSnapshots()).toEqual([]);
    expect(importSnapshot(json).ok).toBe(true);
    expect(listSnapshots()[0]!.name).toBe("b");
  });

  it("显示器签名可解析", () => {
    expect(displaySignature()[0]).toMatch(/^\d+x\d+@[\d.]+$/);
  });
});
