import { describe, expect, it } from "vitest";
import { STEP_FAST_PX, STEP_PX, arrowKey, auditEdgeClamp, beginEdit, cancelEdit, commit, editVisual } from "../f377-keyboardWindowEdit";

const WORK = { x: 0, y: 0, w: 1920, h: 1040 };
const RECT = { x: 100, y: 100, w: 800, h: 600 };

describe("F377 键盘移动与调整窗口", () => {
  it("移动模式：方向键步进 1px；Shift 加速 10px", () => {
    expect(STEP_PX).toBe(1);
    expect(STEP_FAST_PX).toBe(10);
    let s = beginEdit("move", RECT, WORK);
    s = arrowKey(s, "right", false);
    expect(s.current).toMatchObject({ x: 101 });
    s = arrowKey(s, "down", true);
    expect(s.current).toMatchObject({ y: 110 }); // 100 + 10（加速档）
  });

  it("大小模式：右/下增大、左/上减小；最小 40 钳制", () => {
    let s = beginEdit("size", RECT, WORK);
    s = arrowKey(s, "right", true);
    expect(s.current.w).toBe(810);
    s = arrowKey(s, "up", true);
    expect(s.current.h).toBe(590);
    for (let i = 0; i < 100; i++) s = arrowKey(s, "left", true);
    expect(s.current.w).toBe(40);
    for (let i = 0; i < 100; i++) s = arrowKey(s, "down", true);
    expect(s.current.h).toBe(WORK.h);
  });

  it("Enter 落定精度：返回值与 current 逐字段恒等、drift=0（<1px 判据）", () => {
    const s = arrowKey(beginEdit("move", RECT, WORK), "right", false);
    const { committed, driftPx } = commit(s);
    expect(committed).toEqual(s.current);
    expect(driftPx).toBe(0);
  });

  it("Esc 还原原地（判据）", () => {
    let s = beginEdit("move", RECT, WORK);
    s = arrowKey(arrowKey(arrowKey(s, "right", true), "down", true), "right", true);
    expect(s.current.x).toBe(120); // 两次 right 加速 = +20
    expect(cancelEdit(s)).toEqual(RECT);
  });

  it("半透明提示：80% 不透明 + 人话提示（判据）", () => {
    const v = editVisual(beginEdit("move", RECT, WORK));
    expect(v.opacity).toBe(0.8);
    expect(v.hint).toContain("正在编排");
    expect(v.hint).toContain("Esc");
  });

  it("边界贴边：连按加速键恒在工作区内、足够步数后精确贴边（判据自证）", () => {
    for (const dir of ["up", "down", "left", "right"] as const) {
      const a = auditEdgeClamp(beginEdit("move", RECT, WORK), dir, 300);
      expect(a.pass).toBe(true);
    }
    const right = auditEdgeClamp(beginEdit("move", RECT, WORK), "right", 300);
    expect(right.final.x).toBe(WORK.w - RECT.w);
  });
});
