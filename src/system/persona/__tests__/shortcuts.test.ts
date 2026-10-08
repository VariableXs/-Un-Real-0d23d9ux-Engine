import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_SHORTCUTS, DEFAULT_SHORTCUT_COUNT, normalizeCombo, comboSignature,
  validateRebind, detectConflict, rebind, undoRebind, resetAllToDefault,
  effectiveCombos, searchShortcuts, loadOverrides, saveOverrides,
  RESERVED_COMBOS, GROUP_NAMES,
} from "../shortcuts";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
});

describe("F169 快捷键查看器", () => {
  it("默认表 60+ 条目分 4 组（数量实查登记）", () => {
    expect(DEFAULT_SHORTCUT_COUNT).toBeGreaterThanOrEqual(60);
    expect(Object.keys(GROUP_NAMES)).toEqual(["system", "window", "text", "a11y"]);
    for (const g of Object.keys(GROUP_NAMES) as (keyof typeof GROUP_NAMES)[]) {
      expect(DEFAULT_SHORTCUTS.filter((e) => e.group === g).length).toBeGreaterThan(0);
    }
  });

  it("标准化序 Win>Ctrl>Alt>Shift>键", () => {
    expect(normalizeCombo({ ctrl: true, shift: true, key: "S" })).toBe("Ctrl+Shift+S");
    expect(normalizeCombo({ shift: true, ctrl: true, key: "S" })).toBe("Ctrl+Shift+S");
    expect(normalizeCombo({ win: true, alt: true, ctrl: true, shift: true, key: "K" })).toBe("Win+Ctrl+Alt+Shift+K");
    expect(comboSignature({ shift: true, ctrl: true, key: "s" })).toBe("ctrl+shift+s");
  });

  it("录制校验：单键/纯修饰键/系统保留组合拒绝（三要素）", () => {
    expect(validateRebind({ key: "F5" }).ok).toBe(false);
    expect(validateRebind({ key: "Shift" }).ok).toBe(false);
    expect(validateRebind({ ctrl: true, key: "Control" }).ok).toBe(false);
    const reserved = validateRebind({ win: true, key: "L" });
    expect(reserved.ok).toBe(false);
    expect(reserved.reason).toContain("保留");
    expect(RESERVED_COMBOS.has("win+l")).toBe(true);
    expect(validateRebind({ win: true, alt: true, key: "S" }).ok).toBe(true);
  });

  it("冲突检测即录即查（O(1) 查表语义）——撞了谁如实报告", () => {
    const o = loadOverrides();
    const c = detectConflict(o, "sys.clipboard-history", { win: true, shift: true, key: "S" });
    expect(c.conflictId).toBe("sys.screenshot");
    expect(c.conflictZh).toBe("截图");
    expect(detectConflict(o, "sys.settings", { win: true, alt: true, key: "X" }).conflictId).toBeNull();
  });

  it("重录-冲突-改回全链", () => {
    let o = loadOverrides();
    // 与截图热键冲突 → 拒绝
    const conflict = rebind(o, "sys.clipboard-history", { win: true, shift: true, key: "S" });
    expect(conflict.ok).toBe(false);
    expect(conflict.reason).toContain("冲突");
    // 合法重录 → 生效入 undo 栈
    const ok = rebind(o, "sys.screenshot", { win: true, alt: true, key: "S" });
    expect(ok.ok).toBe(true);
    o = ok.overrides;
    expect(o.combos["sys.screenshot"]).toEqual({ win: true, alt: true, key: "S" });
    expect(o.undo).toHaveLength(1);
    saveOverrides(o);
    // 撤销改回默认
    const undone = undoRebind(loadOverrides());
    expect(undone.combos["sys.screenshot"]).toBeUndefined();
  });

  it("系统核心语义条目不可改（可查不可改）", () => {
    const r = rebind(loadOverrides(), "sys.lock", { win: true, alt: true, key: "L" });
    expect(r.ok).toBe(false);
    expect(r.reason).toContain("不可改");
  });

  it("恢复默认一键全复原", () => {
    let o = rebind(loadOverrides(), "sys.settings", { win: true, alt: true, key: "P" });
    expect(o.ok).toBe(true);
    saveOverrides(o.overrides);
    saveOverrides(resetAllToDefault());
    expect(loadOverrides().combos).toEqual({});
    expect(effectiveCombos(loadOverrides()).get("sys.settings")).toEqual(DEFAULT_SHORTCUTS.find((e) => e.id === "sys.settings")?.default);
  });

  it("双向搜索：功能名/按键串", () => {
    expect(searchShortcuts("截图", "zh").map((e) => e.id)).toContain("sys.screenshot");
    expect(searchShortcuts("win+shift", "zh").map((e) => e.id)).toContain("sys.screenshot");
    expect(searchShortcuts("screenshot", "en").map((e) => e.id)).toContain("sys.screenshot");
    expect(searchShortcuts("", "zh")).toHaveLength(DEFAULT_SHORTCUT_COUNT);
  });
});
