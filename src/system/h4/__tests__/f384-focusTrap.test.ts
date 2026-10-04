import { describe, expect, it } from "vitest";
import { auditNoEscape, closeTrap, immediateArming, nextFocus, openTrap, trapOnlyForModal, type TrapTarget } from "../f384-focusTrap";

const TARGETS: TrapTarget[] = [
  { id: "close-x", focusable: true },
  { id: "name-input", focusable: true },
  { id: "divider", focusable: false },
  { id: "ok-btn", focusable: true },
  { id: "cancel-btn", focusable: true },
];

function session() {
  return openTrap("dlg-1", TARGETS, "open-dialog-btn");
}

describe("F384 焦点陷阱（模态完整性）", () => {
  it("开陷阱：只收可聚焦控件成环（不可聚焦元素被剔除）", () => {
    const s = session();
    expect(s.ring).toEqual(["close-x", "name-input", "ok-btn", "cancel-btn"]);
    expect(s.invokerId).toBe("open-dialog-btn");
    expect(immediateArming(s)).toBe(true); // 开启即时性判据
  });

  it("Tab 循环：末尾回第一个；Shift 反向；未知落点从环首进", () => {
    const s = session();
    expect(nextFocus(s, "cancel-btn", false)).toBe("close-x");
    expect(nextFocus(s, "close-x", true)).toBe("cancel-btn");
    expect(nextFocus(s, null, false)).toBe("close-x");
    expect(nextFocus(s, "外部元素", false)).toBe("close-x");
  });

  it("陷阱完整性：模态内 Tab 50 次零逃逸（判据逐字）", () => {
    const a = auditNoEscape(session());
    expect(a.pass).toBe(true);
    expect(a.escapes).toBe(0);
    expect(a.visited).toHaveLength(50);
  });

  it("空环如实降级：返回 null 不假装有焦点（不困死）", () => {
    const s = openTrap("dlg-2", [{ id: "ghost", focusable: false }], "inv");
    expect(s.ring).toEqual([]);
    expect(nextFocus(s, null, false)).toBeNull();
    expect(auditNoEscape(s).pass).toBe(false); // 无环即不满足完整性——如实判红
  });

  it("关闭归还唤起者（F206 联动判据）；关闭即时", () => {
    const r = closeTrap(session());
    expect(r).toEqual({ returnedTo: "open-dialog-btn", active: false });
  });

  it("非模态不困判据：非模态浮层不建陷阱", () => {
    expect(trapOnlyForModal("modal", true)).toBe(true);
    expect(trapOnlyForModal("non-modal", false)).toBe(true);
    expect(trapOnlyForModal("non-modal", true)).toBe(false);
  });
});
