import { describe, expect, it } from "vitest";
import { BANNER_CAP, DEQUEUE_GAP_MS, afterGap, auditFifo, auditFocusUniqueness, auditModalUniqueness, closeModal, dismissBanner, enqueue, initialEngine, type ActiveLayer, type LayerRequest } from "../f383-modalQueue";

const req = (id: string, kind: LayerRequest["kind"], at: number): LayerRequest => ({ id, kind, at });

describe("F383 弹窗排队不叠罗汉", () => {
  it("模态唯一性：第二模态排队不叠现（判据）", () => {
    const r1 = enqueue(initialEngine(), req("m1", "modal", 0));
    expect(r1.presented).toBe(true);
    const r2 = enqueue(r1.state, req("m2", "modal", 10));
    expect(r2.presented).toBe(false);
    expect(r2.state.modalQueue).toEqual(["m2"]);
    expect(auditModalUniqueness([r1.state.active!])).toBe(true);
  });

  it("排队 200ms 间隔：提前递补被拒、到点递补淡入（判据）", () => {
    expect(DEQUEUE_GAP_MS).toBe(200);
    let s = enqueue(enqueue(initialEngine(), req("m1", "modal", 0)).state, req("m2", "modal", 10)).state;
    const closed = closeModal(s, 1000);
    s = closed.state;
    expect(closed.promoted).toBeNull();
    const early = afterGap(s, 1100);
    expect(early.promoted).toBeNull(); // 未满 200ms
    const due = afterGap(s, 1200);
    expect(due.promoted).toBe("m2");
    expect(due.state.active!.id).toBe("m2");
  });

  it("横幅 3 条上限与溢流：第 4 条进通知中心（判据）", () => {
    expect(BANNER_CAP).toBe(3);
    let s = initialEngine();
    for (let i = 0; i < 3; i++) s = enqueue(s, req(`b${i}`, "banner", i)).state;
    expect(s.banners).toHaveLength(3);
    const fourth = enqueue(s, req("b3", "banner", 10));
    expect(fourth.presented).toBe(false);
    expect(fourth.state.overflowedToCenter).toEqual(["b3"]);
    // 关掉一条后新横幅可回填
    s = dismissBanner(s, "b0");
    expect(enqueue(s, req("b4", "banner", 20)).presented).toBe(true);
  });

  it("焦点唯一判据", () => {
    const layers: ActiveLayer[] = [
      { id: "a", kind: "modal", hasFocus: true },
      { id: "b", kind: "osd", hasFocus: false },
    ];
    expect(auditFocusUniqueness(layers)).toBe(true);
    expect(auditFocusUniqueness([...layers, { id: "c", kind: "modal", hasFocus: true }])).toBe(false);
  });

  it("队列公平（先到先出）：递补顺序==入队顺序", () => {
    let s = enqueue(initialEngine(), req("m1", "modal", 0)).state;
    for (const id of ["m2", "m3", "m4"]) s = enqueue(s, req(id, "modal", 5)).state;
    const promoted: string[] = [];
    s = closeModal(s, 100).state;
    for (let now = 400; ; now += 250) {
      const r = afterGap(s, now);
      if (!r.promoted) break;
      promoted.push(r.promoted);
      s = closeModal(r.state, now).state;
    }
    expect(promoted).toEqual(["m2", "m3", "m4"]);
    expect(auditFifo(["m2", "m3", "m4"], promoted)).toBe(true);
  });

  it("OSD 非模态放行不进队", () => {
    const r = enqueue(initialEngine(), req("o1", "osd", 0));
    expect(r.presented).toBe(true);
    expect(r.state.modalQueue).toHaveLength(0);
  });
});
