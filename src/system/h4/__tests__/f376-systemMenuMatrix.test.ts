import { describe, expect, it } from "vitest";
import { MENU_LABELS, MENU_OFFSET_PX, MENU_ORDER, auditMatrix, buildMenu, disabledMatrix, keyboardModeFor, menuOrigin } from "../f376-systemMenuMatrix";

describe("F376 标题栏系统菜单", () => {
  it("六项顺序与 Windows 逐字对齐（交互词典判据）", () => {
    expect(MENU_ORDER).toEqual(["restore", "move", "size", "minimize", "maximize", "close"]);
    expect(MENU_LABELS.restore).toBe("还原");
    expect(MENU_LABELS.close).toBe("关闭");
    expect(buildMenu("normal").map((m) => m.label)).toEqual(Object.values(MENU_LABELS));
  });

  it("置灰状态机：四状态×六项矩阵逐格（判据）", () => {
    expect(disabledMatrix("normal")).toEqual({ restore: true, move: false, size: false, minimize: false, maximize: false, close: false });
    expect(disabledMatrix("maximized")).toEqual({ restore: false, move: true, size: true, minimize: false, maximize: true, close: false });
    expect(disabledMatrix("minimized")).toEqual({ restore: false, move: true, size: true, minimize: true, maximize: false, close: false });
    expect(disabledMatrix("snapped")).toEqual({ restore: false, move: false, size: false, minimize: false, maximize: true, close: false });
  });

  it("状态机完整性审计：四状态全矩阵可生成、关闭永不置灰", () => {
    expect(auditMatrix()).toEqual({ pass: true, missing: [] });
  });

  it("菜单几何：鼠标点下方 2px（判据）", () => {
    expect(MENU_OFFSET_PX).toBe(2);
    expect(menuOrigin({ x: 120, y: 40 })).toEqual({ x: 120, y: 42 });
  });

  it("键盘模式衔接：移动/大小 → F377 两模式；其余项 null", () => {
    expect(keyboardModeFor("move")).toBe("move");
    expect(keyboardModeFor("size")).toBe("size");
    expect(keyboardModeFor("close")).toBeNull();
    expect(keyboardModeFor("restore")).toBeNull();
  });
});
