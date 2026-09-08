/**
 * AI-06 输入手感组 — 纯逻辑测试（vitest）。
 * 覆盖：V-61 参数钳制与双击测试、V-62 13 态校验、V-66 重映射白名单/
 * 让位/翻译、V-67 自然滚动启发式、V-69 降速比例、V-70 拖拽阈值、
 * U-58 覆盖审计表与快捷键表同源、coerce 全默认即现状。
 */

import { describe, expect, it } from "vitest";
import {
  DEFAULT_DRAG_THRESHOLD,
  DEFAULT_INPUT_FEEL,
  KEYBOARD_COVERAGE,
  POINTER_STATES,
  coerceInputFeel,
  coerceMouseParams,
  coercePrecisionRatio,
  isDragStart,
  isRemapFromAllowed,
  isRemapToAllowed,
  isTestSquareDoubleHit,
  matchRemap,
  shouldInvertWheel,
  touchModeActive,
  validateCustomCursorPack,
  validateRemaps,
} from "../inputFeel";
import { SHORTCUT_ACTIONS, normalizeAccel } from "../shortcuts";

describe("V-61 鼠标参数", () => {
  it("钳制到合法区间", () => {
    const p = coerceMouseParams({ speed: 99, doubleClickMs: 10, wheelLines: 0, swapButtons: true });
    expect(p.speed).toBe(20);
    expect(p.doubleClickMs).toBe(200);
    expect(p.wheelLines).toBe(1);
    expect(p.swapButtons).toBe(true);
  });
  it("空值回退默认（默认即现状）", () => {
    expect(coerceMouseParams(null)).toEqual(coerceMouseParams(undefined));
    expect(coerceMouseParams(undefined).doubleClickMs).toBe(500);
  });
  it("双击测试方块：间隔内且位移小 = 双击", () => {
    const t0 = Date.now();
    expect(isTestSquareDoubleHit({ x: 10, y: 10, t: t0 }, { x: 12, y: 11, t: t0 + 400 }, 500)).toBe(true);
    expect(isTestSquareDoubleHit({ x: 10, y: 10, t: t0 }, { x: 12, y: 11, t: t0 + 600 }, 500)).toBe(false);
    expect(isTestSquareDoubleHit({ x: 10, y: 10, t: t0 }, { x: 40, y: 40, t: t0 + 100 }, 500)).toBe(false);
    expect(isTestSquareDoubleHit(null, { x: 0, y: 0, t: t0 }, 500)).toBe(false);
  });
});

describe("V-62 指针方案", () => {
  it("13 态标准齐全", () => {
    expect(POINTER_STATES.length).toBe(13);
    expect(POINTER_STATES).toContain("normalSelect");
    expect(POINTER_STATES).toContain("move");
  });
  it("缺文件的态逐条报错", () => {
    const bad = validateCustomCursorPack({ normalSelect: "a.cur" });
    expect(bad.length).toBe(12);
    expect(bad).not.toContain("normalSelect");
    const good = Object.fromEntries(POINTER_STATES.map((s) => [s, `${s}.cur`]));
    expect(validateCustomCursorPack(good)).toEqual([]);
    expect(validateCustomCursorPack({ ...good, busy: "busy.txt" })).toEqual(["busy"]);
  });
});

describe("V-66 按键重映射", () => {
  it("白名单：CapsLock 可映射，Esc/组合不可", () => {
    expect(isRemapFromAllowed("CapsLock")).toBe(true);
    expect(isRemapFromAllowed("KeyA")).toBe(true);
    expect(isRemapFromAllowed("Escape")).toBe(false);
    expect(isRemapToAllowed("ControlLeft")).toBe(true);
    expect(isRemapToAllowed("Escape")).toBe(false);
  });
  it("校验：身份映射/重复 from/非法 to 报错", () => {
    expect(validateRemaps([{ from: "CapsLock", to: "CapsLock" }])[0]?.reason).toBe("identity");
    expect(
      validateRemaps([
        { from: "CapsLock", to: "ControlLeft" },
        { from: "CapsLock", to: "AltLeft" },
      ]).some((e) => e.reason === "duplicate-from"),
    ).toBe(true);
  });
  it("翻译：命中映射返回目标键", () => {
    const remaps = [{ from: "CapsLock", to: "ControlLeft" }];
    const hit = matchRemap({ code: "CapsLock", ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, repeat: false }, remaps);
    expect(hit?.code).toBe("ControlLeft");
    expect(hit?.key).toBe("Control");
  });
  it("让位：其他修饰键按住时不翻译（系统组合键让位协议）", () => {
    const remaps = [{ from: "CapsLock", to: "ControlLeft" }];
    const hit = matchRemap({ code: "CapsLock", ctrlKey: true, altKey: false, shiftKey: false, metaKey: false, repeat: false }, remaps);
    expect(hit).toBeNull();
  });
  it("repeat 事件不重复翻译", () => {
    const remaps = [{ from: "KeyA", to: "KeyB" }];
    expect(matchRemap({ code: "KeyA", ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, repeat: true }, remaps)).toBeNull();
  });
});

describe("V-67 自然滚动", () => {
  it("关闭 = 永不反转（现状）", () => {
    expect(shouldInvertWheel(false, "touch", 0, 1000)).toBe(false);
  });
  it("触摸类指针反转", () => {
    expect(shouldInvertWheel(true, "touch", 0, 1000)).toBe(true);
  });
  it("近期有鼠标移动 → 保持鼠标滚轮语义", () => {
    const now = 10_000;
    expect(shouldInvertWheel(true, "", now - 1000, now)).toBe(false);
    expect(shouldInvertWheel(true, "", now - 3000, now)).toBe(true);
    expect(shouldInvertWheel(true, "", 0, now)).toBe(true);
  });
});

describe("V-69/V-70 精确模式与拖拽阈值", () => {
  it("降速比例钳制 20%–60%", () => {
    expect(coercePrecisionRatio(0.1)).toBe(0.2);
    expect(coercePrecisionRatio(0.9)).toBe(0.6);
    expect(coercePrecisionRatio(0.4)).toBe(0.4);
  });
  it("阈值内不算拖拽、阈值外启动", () => {
    const t = DEFAULT_DRAG_THRESHOLD; // 4
    expect(isDragStart(3, 0, t)).toBe(false);
    expect(isDragStart(0, 3.9, t)).toBe(false);
    expect(isDragStart(4, 0, t)).toBe(true);
  });
  it("触控输入阈值自动放大 ×2", () => {
    expect(isDragStart(5, 0, 4, "mouse")).toBe(true);
    expect(isDragStart(5, 0, 4, "touch")).toBe(false);
    expect(isDragStart(8, 0, 4, "touch")).toBe(true);
  });
  it("阈值本身钳制 2..10", () => {
    expect(isDragStart(50, 0, 99, "mouse")).toBe(true); // 99 → 10
    expect(isDragStart(1, 0, 0, "mouse")).toBe(false); // 0 → 2
  });
});

describe("U-58 键盘全景", () => {
  it("覆盖审计表：全部已覆盖且路径可归一化", () => {
    expect(KEYBOARD_COVERAGE.length).toBeGreaterThanOrEqual(20);
    for (const row of KEYBOARD_COVERAGE) {
      expect(row.covered, row.opKey).toBe(true);
      expect(normalizeAccel(row.path), row.opKey).not.toBeNull();
    }
  });
  it("与快捷键表同源：审计表中 SHORTCUT_ACTIONS 的路径与默认表一致", () => {
    for (const row of KEYBOARD_COVERAGE) {
      const action = SHORTCUT_ACTIONS.find((a) => a.accel === row.path);
      if (action) expect(action.accel).toBe(row.path);
    }
    // 关键操作在注册表中存在（改键即同步的前提）
    const ids = new Set(SHORTCUT_ACTIONS.map((a) => a.id));
    for (const need of ["explorer", "showDesktop", "snapLeft", "notifyCenter", "clipboardHistory", "dnd"]) {
      expect(ids.has(need), need).toBe(true);
    }
  });
});

describe("U-59 触控模式", () => {
  it("auto = 首触切换；on/off 覆盖信号", () => {
    expect(touchModeActive("auto", false)).toBe(false);
    expect(touchModeActive("auto", true)).toBe(true);
    expect(touchModeActive("on", false)).toBe(true);
    expect(touchModeActive("off", true)).toBe(false);
  });
});

describe("coerceInputFeel（默认即现状）", () => {
  it("损坏输入整体回退默认", () => {
    expect(coerceInputFeel(null)).toEqual(DEFAULT_INPUT_FEEL);
    expect(coerceInputFeel("junk")).toEqual(DEFAULT_INPUT_FEEL);
    expect(coerceInputFeel({}).trailEnabled).toBe(false);
    expect(coerceInputFeel({}).keyRemaps).toEqual([]);
    expect(coerceInputFeel({}).naturalScroll).toBe(false);
    expect(coerceInputFeel({}).typingSound).toBe("off");
  });
  it("合法字段保留、非法字段回退", () => {
    const s = coerceInputFeel({
      trailEnabled: true,
      trailLevel: 9,
      dragThreshold: 7,
      typingSound: "rain",
      precisionRatio: 0.5,
      keyRemaps: [{ from: "CapsLock", to: "ControlLeft" }, { from: 1, to: 2 }, "bad"],
      mouse: { speed: 12, doubleClickMs: 300, wheelLines: 2, swapButtons: false },
      touchMode: "on",
    });
    expect(s.trailEnabled).toBe(true);
    expect(s.trailLevel).toBe(1); // 9 非法 → 默认 1
    expect(s.dragThreshold).toBe(7);
    expect(s.typingSound).toBe("rain");
    expect(s.precisionRatio).toBe(0.5);
    expect(s.keyRemaps).toEqual([{ from: "CapsLock", to: "ControlLeft" }]); // 坏条目剔除
    expect(s.mouse.speed).toBe(12);
    expect(s.touchMode).toBe("on");
  });
  it("自定义指针文件仅接受字符串路径", () => {
    const s = coerceInputFeel({ customCursors: { normalSelect: "a.cur", busy: 42, move: "  " } });
    expect(s.customCursors.normalSelect).toBe("a.cur");
    expect(s.customCursors.busy).toBeUndefined();
    expect(s.customCursors.move).toBeUndefined();
  });
});
