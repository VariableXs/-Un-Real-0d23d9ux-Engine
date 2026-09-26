import { describe, expect, it } from "vitest";
import { buildTour, TourRunner } from "../tour-preset-parity";
import { PresetLibrary } from "../tour-preset-parity";
import { auditParity, E_ACTION_PAIRS } from "../tour-preset-parity";

// ---------- tour-engine ----------

describe("tour-engine · 渐进披露导览（十一章深化）", () => {
  const tour = buildTour("t1", "五分钟上手", [
    ["tokens", "a", "第一步", "click-target"],
    ["preview", "b", "第二步", "next-button"],
    ["archive", "c", "第三步", "next-button"],
  ]);

  it("五分钟契约：步数 ≤10（每步 ≤30s → 全程 ≤5min）", () => {
    expect(tour.withinFiveMin).toBe(true);
    expect(tour.steps).toHaveLength(3);
    expect(buildTour("t2", "超长", Array.from({ length: 11 }, (_, i) => ["p", `e${i}`, "s", "next-button"] as const)).withinFiveMin).toBe(false);
  });

  it("推进器：只进不退、进度百分比、走完 done", () => {
    const r = new TourRunner(tour);
    expect(r.current!.targetId).toBe("a");
    expect(r.progress()).toBe(0);
    r.next();
    expect(r.progress()).toBe(33);
    r.next();
    r.next();
    expect(r.done).toBe(true);
    expect(r.current).toBeNull();
    expect(r.progress()).toBe(100);
  });
});

// ---------- preset-library ----------

describe("preset-library · 预设库（三铁律「预设+微调」深化）", () => {
  it("官方预设只读：覆盖显性拒绝、删除显性拒绝（出厂保证）", () => {
    const lib = new PresetLibrary();
    lib.registerOfficial("official", "官方", { a: 1 });
    expect(lib.saveUser("official", "冒名", { a: 2 }).ok).toBe(false);
    expect(lib.deleteUser("official").ok).toBe(false);
    expect(lib.deleteUser("nonexistent").reason).toBe("不存在");
  });

  it("用户预设可存可删（官方保护之外的自由）", () => {
    const lib = new PresetLibrary();
    lib.registerOfficial("o", "官方", {});
    expect(lib.saveUser("u1", "我的", { x: 1 }).ok).toBe(true);
    expect(lib.saveUser("u1", "我的改", { x: 2 })).toEqual({ ok: true }); // 用户预设可覆盖更新。
    expect(lib.deleteUser("u1").ok).toBe(true);
    expect(lib.all.filter((p) => !p.official)).toHaveLength(0);
  });

  it("预设 diff 预览：应用前逐键差异（不是盲应用）", () => {
    const lib = new PresetLibrary();
    const p = { id: "p", name: "n", official: false, payload: { accent: "#111", density: "standard", keep: 1 } };
    const d = lib.diff({ ...p, payload: { accent: "#222", density: "standard", keep: 1 } }, { accent: "#333", density: "standard", keep: 1 });
    expect(d).toEqual([{ path: "accent", from: "#333", to: "#222" }]); // 同值不列——差异面板只摆真正要改的。
  });
});

// ---------- keyboard-map ----------

describe("keyboard-map · 键鼠对等审计（四章深化）", () => {
  it("E 域标准对偶表：核心动作全有键盘等价（红线达标）", () => {
    const a = auditParity(E_ACTION_PAIRS);
    expect(a.ok).toBe(true);
    expect(a.total).toBeGreaterThanOrEqual(6);
  });

  it("缺等价逐条暴露（keyboard: null = 红线）", () => {
    const a = auditParity([...E_ACTION_PAIRS, { action: "拖拽组件", mouse: "拖", keyboard: null }]);
    expect(a.ok).toBe(false);
    expect(a.missing).toEqual([{ action: "拖拽组件", mouse: "拖" }]);
  });
});
