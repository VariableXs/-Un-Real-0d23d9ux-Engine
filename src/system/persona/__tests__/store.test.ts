import { beforeEach, describe, expect, it } from "vitest";
import {
  personaStore, perceptualVolume, nextDailyFire, pickExcluding,
  withinBootDuration, BOOT_DURATION_MS, UNDO_STACK_DEPTH, PERSONA_SECTIONS,
} from "../store";

beforeEach(() => {
  personaStore.reset();
});

describe("personaStore 底座", () => {
  it("set/get 读写与持久化键一致", () => {
    personaStore.set("theme", { foo: 1 });
    expect(personaStore.get("theme").foo).toBe(1);
    const raw = localStorage.getItem("variable:persona:v1");
    expect(raw).toContain('"format":"persona-config"');
  });

  it("undoSection 回退上一态；栈深 3 静默淘汰最旧", () => {
    personaStore.set("sound", { v: 1 });
    personaStore.set("sound", { v: 2 });
    personaStore.set("sound", { v: 3 });
    personaStore.set("sound", { v: 4 }); // 栈里剩 [v2, v3, v4 前]
    expect(personaStore.canUndo("sound")).toBe(true);
    expect(personaStore.undoSection("sound")).toBe(true);
    expect(personaStore.get("sound").v).toBe(3);
    expect(personaStore.undoSection("sound")).toBe(true);
    expect(personaStore.get("sound").v).toBe(2);
    expect(personaStore.undoSection("sound")).toBe(true);
    expect(personaStore.get("sound").v).toBe(1);
    // 三层用尽后不可再退
    expect(personaStore.undoSection("sound")).toBe(false);
    expect(UNDO_STACK_DEPTH).toBe(3);
  });

  it("订阅制总线：按节广播，退订生效", () => {
    const seen: string[] = [];
    const off = personaStore.subscribe((section) => seen.push(section));
    personaStore.set("icons", { a: 1 });
    personaStore.set("ime", { b: 2 });
    off();
    personaStore.set("icons", { a: 2 });
    expect(seen).toEqual(["icons", "ime"]);
  });

  it("importAll：空包拒绝（原子性——不留半套）", () => {
    expect(() => personaStore.importAll({})).toThrow(/没有任何合法分节/);
  });

  it("importAll：合法分节覆盖 + 非法分节忽略", () => {
    personaStore.set("motion", { tier: "full" });
    personaStore.importAll({ motion: { tier: "reduced" }, bogusSection: { x: 1 } });
    expect(personaStore.get("motion").tier).toBe("reduced");
  });

  it("exportAll → importAll round-trip 逐节等值", () => {
    personaStore.set("taskbar", { iconSize: "large" });
    personaStore.set("sound", { masterMute: true });
    const snap = personaStore.exportAll();
    personaStore.reset();
    personaStore.importAll(snap as unknown as Record<string, unknown>);
    expect(personaStore.get("taskbar").iconSize).toBe("large");
    expect(personaStore.get("sound").masterMute).toBe(true);
  });

  it("PERSONA_SECTIONS 十六节完整（E 域覆盖面自证）", () => {
    expect(PERSONA_SECTIONS).toHaveLength(16);
  });
});

describe("共享纯函数", () => {
  it("感知音量：50% 滑杆 = 25% 感知响度（半响口径）", () => {
    expect(perceptualVolume(0.5)).toBeCloseTo(0.25);
    expect(perceptualVolume(0)).toBe(0);
    expect(perceptualVolume(1)).toBe(1);
    expect(perceptualVolume(-1)).toBe(0);
    expect(perceptualVolume(2)).toBe(1);
  });

  it("nextDailyFire：当日已过则排到明天", () => {
    const now = new Date("2026-09-26T10:00:00").getTime();
    expect(nextDailyFire(now, 9, 0)).toBe(new Date("2026-09-27T09:00:00").getTime());
    expect(nextDailyFire(now, 11, 0)).toBe(new Date("2026-09-26T11:00:00").getTime());
  });

  it("pickExcluding：排除表非空时只从剩余里抽；全被排除才回退全池", () => {
    const pool = ["a", "b", "c"];
    const r = pickExcluding(pool, (x) => x === "a" || x === "b", () => 0);
    expect(r).toBe("c");
    const allExcluded = pickExcluding(pool, () => true, () => 0);
    expect(pool).toContain(allExcluded as string);
    expect(pickExcluding([], () => false)).toBeNull();
  });

  it("开机结构时长 8.0s±0.2s 契约", () => {
    expect(BOOT_DURATION_MS).toBe(8000);
    expect(withinBootDuration(8000)).toBe(true);
    expect(withinBootDuration(8150)).toBe(true);
    expect(withinBootDuration(8300)).toBe(false);
    expect(withinBootDuration(7700)).toBe(false);
  });
});
