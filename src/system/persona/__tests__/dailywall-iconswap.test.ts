import { beforeEach, describe, expect, it } from "vitest";
import {
  defaultDailyWallConfig, pickDailyWallpaper, commitDailyPick, pinToday,
  shouldRotateNow, nextRotateAt, compositionCrop, RESOLUTION_MATRIX, HISTORY_LIMIT_DAYS,
} from "../dailywall";
import {
  defaultIconPackState, validateIconPack, switchIconPack, rollbackIconPack,
  coverageStats, resolveIcon, ICON_CATEGORIES,
} from "../iconswap";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
});

describe("F154 壁纸每日一换", () => {
  it("今日已固定 → 不更换", () => {
    const cfg = defaultDailyWallConfig();
    const now = new Date("2026-09-26T09:00:00").getTime();
    const pinned = pinToday({ ...cfg, current: "wall-x" }, now);
    const r = pickDailyWallpaper(pinned, { now, poolItems: { p: ["wall-y"] } });
    expect(r.wallpaperId).toBe("wall-x");
    expect(r.reason).toContain("固定");
  });

  it("池空 → 回退默认路径（fellBack=true，三要素原因）", () => {
    const r = pickDailyWallpaper(defaultDailyWallConfig(), { now: 0, poolItems: {} });
    expect(r.fellBack).toBe(true);
    expect(r.reason).toContain("池空");
  });

  it("近 7 天排除表：排除后仍可抽到未用项", () => {
    const cfg = defaultDailyWallConfig();
    const poolId = cfg.pools[0]!.id;
    cfg.recent = ["a", "b", "c", "d", "e", "f", "g"];
    const r = pickDailyWallpaper(cfg, { now: 0, poolItems: { [poolId]: ["a", "b", "h"] }, rand: () => 0.5 });
    expect(r.wallpaperId).toBe("h");
  });

  it("排除表满（7 天全用尽）→ 允许重复抽取", () => {
    const cfg = defaultDailyWallConfig();
    const poolId = cfg.pools[0]!.id;
    cfg.recent = ["a", "b"];
    const r = pickDailyWallpaper(cfg, { now: 0, poolItems: { [poolId]: ["a", "b"] }, rand: () => 0 });
    expect(r.wallpaperId).toBe("a");
    expect(r.reason).toContain("允许重复");
  });

  it("commit：历史环形 30 天 + recent 排除表更新 + pinned 解除", () => {
    let cfg = defaultDailyWallConfig();
    cfg = pinToday(cfg, 0);
    const base = new Date("2026-09-01T00:00:00").getTime();
    for (let i = 0; i < HISTORY_LIMIT_DAYS + 5; i++) {
      cfg = commitDailyPick(cfg, `w${i}`, base + i * 86400000);
    }
    expect(Object.keys(cfg.history).length).toBeLessThanOrEqual(HISTORY_LIMIT_DAYS);
    expect(cfg.recent.length).toBeLessThanOrEqual(7);
    expect(cfg.pinnedDate).toBeNull();
  });

  it("到点判定与下次触发时刻", () => {
    const cfg = defaultDailyWallConfig();
    cfg.changeAt = "00:00";
    const morning = new Date("2026-09-26T09:00:00").getTime();
    expect(shouldRotateNow(cfg, morning)).toBe(true);
    // 换过后当天不再触发
    const done = commitDailyPick(cfg, "w1", morning);
    expect(shouldRotateNow(done, morning + 3600000)).toBe(false);
    expect(nextRotateAt(cfg, morning)).toBe(new Date("2026-09-27T00:00:00").getTime());
  });

  it("构图保护：焦点偏上（人像主体不裁头）", () => {
    // 竖图 1000×2000 裁到 16:9 → 裁上下，焦点 0.42 偏上
    const crop = compositionCrop({ srcW: 1000, srcH: 2000, dstW: 1920, dstH: 1080 });
    expect(crop.y).toBeLessThan((2000 - crop.h) / 2);
    // 横图裁左右：焦点 x=0.1 时窗口偏左
    const crop2 = compositionCrop({ srcW: 2000, srcH: 1000, dstW: 1080, dstH: 1920, focusX: 0.1 });
    expect(crop2.x).toBeLessThan((2000 - crop2.w) / 2);
  });

  it("20 档分辨率矩阵在位", () => {
    expect(RESOLUTION_MATRIX).toHaveLength(20);
  });
});

describe("F155 图标包热更换", () => {
  it("校验三查：结构/覆盖表键/checksum", () => {
    expect(validateIconPack(null).ok).toBe(false);
    expect(validateIconPack({ id: "x", name: "n", version: "1", checksum: "c", coverage: { "bogus.category": "i" } }).ok).toBe(false);
    expect(validateIconPack({ id: "x", name: "n", version: "1", checksum: "", coverage: {} }).ok).toBe(false);
    expect(validateIconPack({ id: "x", name: "n", version: "1", checksum: "c", coverage: { "system.folder": "i" } }).ok).toBe(true);
  });

  it("原子切换：校验失败整体拒绝维持原包", () => {
    const state = defaultIconPackState();
    const r = switchIconPack(state, null, { ok: false, reason: "包损坏" }, 5);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.kept).toBeNull();
  });

  it("换包 → 回退栈深 3 → 连退三次后栈空", () => {
    const categories = ICON_CATEGORIES.slice(0, 3);
    let state = defaultIconPackState();
    for (let i = 1; i <= 4; i++) {
      const pack = { id: `p${i}`, name: `包${i}`, version: "1", checksum: "c", coverage: Object.fromEntries(categories.map((c) => [c, `i${i}`])) };
      const r = switchIconPack(state, pack, validateIconPack(pack), 10);
      expect(r.ok).toBe(true);
      state = { current: pack, rollback: [state.current, ...state.rollback].slice(0, 3), previous: state.current };
    }
    expect(state.current?.id).toBe("p4");
    expect(state.rollback).toHaveLength(3);
    const r1 = rollbackIconPack(state);
    expect(r1?.restored?.id).toBe("p3");
    const r2 = rollbackIconPack(r1!.next);
    expect(r2?.restored?.id).toBe("p2");
    const r3 = rollbackIconPack(r2!.next);
    expect(r3?.restored?.id).toBe("p1");
    expect(rollbackIconPack(r3!.next)).toBeNull();
  });

  it("诚实覆盖度：缺失类别如实列出（187/200 形态）", () => {
    const pack = { id: "p", name: "p", version: "1", checksum: "c", coverage: { "system.folder": "i", "type.pdf": "i" } };
    const s = coverageStats(pack);
    expect(s.ratioLabel).toBe(`2/${ICON_CATEGORIES.length}`);
    expect(s.missing).toContain("system.start");
    expect(coverageStats(null).covered).toBe(ICON_CATEGORIES.length);
  });

  it("解析优先级：当前包 > 官方 > 默认占位", () => {
    const pack = { id: "p", name: "p", version: "1", checksum: "c", coverage: { "system.folder": "pack-icon" } };
    const official = { "system.start": "official-start" };
    expect(resolveIcon(pack, official, "system.folder")).toEqual({ icon: "pack-icon", source: "pack" });
    expect(resolveIcon(pack, official, "system.start")).toEqual({ icon: "official-start", source: "official" });
    expect(resolveIcon(pack, official, "system.tray").source).toBe("default");
  });
});
