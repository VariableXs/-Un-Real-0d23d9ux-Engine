import { describe, expect, it } from "vitest";
import { SHEET_MAX_VISIBLE_ROWS, TRIGGER_MS, auditSameSource, initialSheet, overlayClickThrough, sheetContent, sheetRenderBudget, tick, winDown, winUp, type HotkeyEntry } from "../f374-hotkeySheet";

const REG: HotkeyEntry[] = [
  { combo: "Win+←", action: "贴靠左半", group: "window" },
  { combo: "Win+D", action: "显示桌面", group: "system" },
  { combo: "Ctrl+F", action: "窗口内查找", group: "appGeneric" },
  { combo: "Ctrl+K", action: "插入链接", group: "appSpecific", app: "write" },
];

describe("F374 快捷键速查浮层", () => {
  it("600ms 触发：未满不显示、满即显示（判据）", () => {
    let s = winDown(initialSheet(), 1000);
    expect(tick(s, 1599).visible).toBe(false);
    expect(tick(s, 1600).visible).toBe(true);
    expect(TRIGGER_MS).toBe(600);
  });

  it("松开即隐；再按重新计时", () => {
    let s = winDown(initialSheet(), 0);
    s = tick(s, 600);
    expect(s.visible).toBe(true);
    s = winUp(s);
    expect(s.visible).toBe(false);
    expect(winUp(s).winDownAt).toBeNull();
    const again = tick(winDown(s, 5000), 5500);
    expect(again.visible).toBe(false);
  });

  it("未按 Win 时 tick 无害幂等", () => {
    const s = initialSheet();
    expect(tick(s, 99999)).toEqual(initialSheet());
  });

  it("覆盖层点击穿透（判据）", () => {
    expect(overlayClickThrough(initialSheet())).toBe(true);
    expect(overlayClickThrough({ winDownAt: 0, visible: true })).toBe(true);
  });

  it("分组列出 + 应用专属组叠加：聚焦 write 时出现 Ctrl+K；聚焦其他不出现", () => {
    const content = sheetContent(() => REG, "write");
    expect(content.map((g) => g.group)).toEqual(["window", "system", "appGeneric", "appSpecific"]);
    expect(content.find((g) => g.group === "appSpecific")!.entries[0]!.combo).toBe("Ctrl+K");
    const none = sheetContent(() => REG, "calc");
    expect(none.find((g) => g.group === "appSpecific")).toBeUndefined();
  });

  it("同源实时性：改键后视图即变（无缓存）", () => {
    const before = REG;
    const after = REG.map((e) => (e.combo === "Win+D" ? { ...e, combo: "Win+Shift+D" } : e));
    expect(auditSameSource(before, after)).toBe(true);
    expect(auditSameSource(before, before)).toBe(false);
  });

  it("渲染预算：超 40 条折叠", () => {
    expect(sheetRenderBudget(SHEET_MAX_VISIBLE_ROWS)).toEqual({ rows: SHEET_MAX_VISIBLE_ROWS, collapsed: false });
    expect(sheetRenderBudget(55)).toEqual({ rows: SHEET_MAX_VISIBLE_ROWS, collapsed: true });
  });
});
