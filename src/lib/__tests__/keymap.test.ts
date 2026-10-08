/**
 * AI-05 键位纪律组 — 核心单测（Z-08/Z-09/Z-14/M-28/M-29/M-36/V-94）。
 */

import { describe, it, expect, beforeEach, vi } from "vitest";

import {
  register,
  unregister,
  snapshot,
  conflicts,
  activeByScope,
  __resetForTests,
} from "../keymap/registry";
import {
  pushContext,
  popContext,
  contextTop,
  arbitrate,
  isEditableTarget,
} from "../keymap/arbitrate";
import { SYSTEM_COMBOS, SYSTEM_RESERVED, shouldYield, setFullscreenActive } from "../keymap/system-combos";
import { KEYMAP_PROFILES, validateProfilePatch, applyProfile, type KeymapProfile } from "../keymap/profiles";
import { previewChanges, suggestFreeSlots } from "../keymap/preview";
import { runKeymapDoctor } from "../keymap/doctor";
import { recordAction, topActions, recordOccupancy } from "../keymap/telemetry";
import { KEY_REPEAT_CURVE, keyRepeatScale, eventToCombo } from "../keymap/hooks";
import { pushOverlay, popOverlay, consumeOverlayOnEsc } from "../../state/uiStore";
import { normalizeAccel, SHORTCUT_ACTIONS, effectiveBinds, findConflicts } from "../shortcuts";

// ---------- Z-08 注册表 ----------

describe("Z-08 registry", () => {
  beforeEach(() => __resetForTests());

  it("registers and unregisters bindings", () => {
    expect(register({ id: "a1", combo: "ctrl+alt+t", scope: "global", priority: 0, source: "test", descKey: "d" }).ok).toBe(true);
    expect(snapshot()).toHaveLength(1);
    expect(unregister("a1")).toBe(true);
    expect(snapshot()).toHaveLength(0);
  });

  it("rejects invalid combos", () => {
    const r = register({ id: "bad", combo: "not a combo!!", scope: "global", priority: 0, source: "test", descKey: "d" });
    expect(r.ok).toBe(false);
  });

  it("same-scope same-combo = error; higher priority may displace", () => {
    register({ id: "low", combo: "ctrl+alt+t", scope: "global", priority: 1, source: "test", descKey: "d" });
    const blocked = register({ id: "hi", combo: "ctrl+alt+t", scope: "global", priority: 0, source: "test", descKey: "d" });
    expect(blocked.ok).toBe(false);
    const displaced = register({ id: "hi2", combo: "ctrl+alt+t", scope: "global", priority: 9, source: "test", descKey: "d" });
    expect(displaced.ok).toBe(true);
    expect(conflicts().some((c) => c.kind === "error")).toBe(true);
  });

  it("cross-scope same-combo = warn (allowed)", () => {
    register({ id: "g", combo: "ctrl+alt+y", scope: "global", priority: 0, source: "test", descKey: "d" });
    const r = register({ id: "w", combo: "ctrl+alt+y", scope: "window", priority: 0, source: "test", descKey: "d" });
    expect(r.ok).toBe(true);
    expect(conflicts().some((c) => c.kind === "warn")).toBe(true);
  });

  it("snapshot diff empty after unmount-style cleanup", () => {
    register({ id: "p1", combo: "ctrl+alt+p", scope: "context", priority: 0, source: "panel:x", descKey: "d" });
    unregister("p1");
    expect(snapshot()).toHaveLength(0);
  });

  it("activeByScope filters by scope", () => {
    register({ id: "g1", combo: "ctrl+alt+a", scope: "global", priority: 0, source: "test", descKey: "d" });
    register({ id: "c1", combo: "ctrl+alt+c", scope: "context", priority: 0, source: "test", descKey: "d" });
    expect(activeByScope("global").map((b) => b.id)).toEqual(["g1"]);
    expect(activeByScope("context").map((b) => b.id)).toEqual(["c1"]);
  });
});

// ---------- Z-08/Z-10 作用域裁决 ----------

const hasDOM = typeof document !== "undefined";

describe("Z-10 arbitrate", () => {
  beforeEach(() => __resetForTests());

  const fakeEditable = (): HTMLElement => {
    const el = document.createElement("input");
    return el;
  };

  it("no binding → null with reason", () => {
    expect(arbitrate({ combo: "ctrl+alt+z", target: null }).yielded).toBe("no-binding");
  });

  it.runIf(hasDOM)("global binding yields inside editable elements", () => {
    register({ id: "g", combo: "ctrl+alt+k", scope: "global", priority: 0, source: "test", descKey: "d" });
    expect(arbitrate({ combo: "ctrl+alt+k", target: fakeEditable() }).yielded).toBe("editable");
    expect(arbitrate({ combo: "ctrl+alt+k", target: document.createElement("div") }).binding?.id).toBe("g");
  });

  it("context stack LIFO", () => {
    pushContext("ctx:a");
    pushContext("ctx:b");
    expect(contextTop()).toBe("ctx:b");
    popContext("ctx:a");
    expect(contextTop()).toBe("ctx:b");
    popContext("ctx:b");
    expect(contextTop()).toBeNull();
  });

  it.runIf(hasDOM)("isEditableTarget", () => {
    expect(isEditableTarget(document.createElement("textarea"))).toBe(true);
    expect(isEditableTarget(document.createElement("button"))).toBe(false);
  });
});

// ---------- Z-09 系统组合让位 ----------

describe("Z-09 system combos", () => {
  it("has 40+ entries and shared reserved set", () => {
    expect(SYSTEM_COMBOS.length).toBeGreaterThanOrEqual(40);
    expect(SYSTEM_RESERVED.size).toBeGreaterThanOrEqual(40);
  });

  it("passthrough never consumed", () => {
    expect(shouldYield("alt+tab").yield).toBe(true);
    expect(shouldYield("super+e").yield).toBe(true);
  });

  it("yield combos yield in fullscreen, consume otherwise", () => {
    setFullscreenActive(false);
    expect(shouldYield("super+tab").yield).toBe(false);
    setFullscreenActive(true);
    expect(shouldYield("super+tab").yield).toBe(true);
    setFullscreenActive(false);
  });

  it("non-system combos never yield", () => {
    expect(shouldYield("ctrl+alt+q").yield).toBe(false);
  });
});

// ---------- Z-14 方案管理 ----------

describe("Z-14 profiles", () => {
  it("three presets, all valid", () => {
    expect(KEYMAP_PROFILES).toHaveLength(3);
    for (const p of KEYMAP_PROFILES) {
      expect(validateProfilePatch(p.patch).ok).toBe(true);
    }
  });

  it("rejects unknown action ids (schema)", () => {
    const v = validateProfilePatch({ notAnAction: "ctrl+alt+1" });
    expect(v.ok).toBe(false);
    expect(v.errors.some((e) => e.startsWith("schema:unknown-action"))).toBe(true);
  });

  it("applyProfile merges patches", () => {
    const p = KEYMAP_PROFILES.find((x) => x.id === "leftHand")!;
    const next = applyProfile({ explorer: "ctrl+alt+e" }, p);
    expect(next.explorer).toBe("ctrl+alt+q"); // 补丁覆盖
    expect(next.quickAudio).toBe("ctrl+alt+x"); // 补丁生效
    expect(next.dnd).toBeUndefined(); // 未覆盖项不引入
  });

  it("preset round-trip converges to same terminal state (Z-14 gate)", () => {
    let overrides: Record<string, string> = {};
    for (const p of [...KEYMAP_PROFILES, KEYMAP_PROFILES[0] as KeymapProfile]) {
      overrides = applyProfile(overrides, p);
    }
    // 应用全部方案后再应用 default（空补丁）→ 与直接应用 default 终态一致
    const direct = applyProfile({}, KEYMAP_PROFILES[0] as KeymapProfile);
    expect(overrides).toEqual(direct);
  });
});

// ---------- M-36 预览 ----------

describe("M-36 preview", () => {
  it("detects changes and reserved hits", () => {
    const r = previewChanges({ explorer: "super+e" }); // super+e 系统保留
    expect(r.changes.some((c) => c.action === "explorer" && c.reservedHit)).toBe(true);
  });

  it("detects displaced actions on conflicts", () => {
    const r = previewChanges({ notifyCenter: "ctrl+alt+v", clipboardHistory: "ctrl+alt+v" });
    expect(r.conflicts).toContain("ctrl+alt+v");
    expect(r.displaced.length).toBeGreaterThanOrEqual(1);
  });

  it("suggestFreeSlots skips taken and reserved", () => {
    const taken = new Set(["ctrl+alt+a", "ctrl+alt+b", "ctrl+alt+c"]);
    const slots = suggestFreeSlots(taken, 3);
    expect(slots).toHaveLength(3);
    expect(slots.every((s) => !taken.has(s) && !SYSTEM_RESERVED.has(s))).toBe(true);
  });
});

// ---------- M-28 遥测 ----------

describe("M-28 telemetry", () => {
  it("counts actions (id + count only)", () => {
    let s: Record<string, number> = {};
    s = recordAction(s, "explorer");
    s = recordAction(s, "explorer");
    s = recordAction(s, "dnd");
    expect(topActions(s, 10)).toEqual([
      { id: "explorer", count: 2 },
      { id: "dnd", count: 1 },
    ]);
  });

  it("occupancy board counts failures", () => {
    let o: Record<string, number> = {};
    o = recordOccupancy(o, "alt+tab");
    expect(o["alt+tab"]).toBe(1);
  });
});

// ---------- M-29 长按曲线 ----------

describe("M-29 key repeat curve", () => {
  it("1x before 500ms then accelerates", () => {
    expect(keyRepeatScale(0)).toBe(1);
    expect(keyRepeatScale(499)).toBe(1);
    expect(keyRepeatScale(600)).toBe(1.5);
    expect(keyRepeatScale(1200)).toBe(2.5);
    expect(keyRepeatScale(2000)).toBe(4);
  });

  it("curve segments are monotonic", () => {
    for (let i = 1; i < KEY_REPEAT_CURVE.length; i++) {
      expect(KEY_REPEAT_CURVE[i]!.scale).toBeGreaterThan(KEY_REPEAT_CURVE[i - 1]!.scale);
    }
  });
});

// ---------- 事件 → combo ----------

describe("eventToCombo", () => {
  it("normalizes modifiers in order", () => {
    const e = { ctrlKey: true, altKey: true, shiftKey: false, metaKey: false, key: "t" } as KeyboardEvent;
    expect(eventToCombo(e)).toBe("ctrl+alt+t");
  });
});

// ---------- M-34 Esc 浮层栈 ----------

describe("M-34 esc overlay stack", () => {
  beforeEach(() => {
    let top = consumeOverlayOnEsc();
    while (top !== null) top = consumeOverlayOnEsc();
  });

  it("push/pop LIFO with idempotent push", () => {
    pushOverlay("ov:menu");
    pushOverlay("ov:modal");
    pushOverlay("ov:menu"); // 幂等：刷新到栈顶
    expect(consumeOverlayOnEsc()).toBe("ov:menu");
    expect(consumeOverlayOnEsc()).toBe("ov:modal");
    expect(consumeOverlayOnEsc()).toBeNull();
  });

  it("pop removes specific id", () => {
    pushOverlay("a");
    pushOverlay("b");
    popOverlay("a");
    expect(consumeOverlayOnEsc()).toBe("b");
    expect(consumeOverlayOnEsc()).toBeNull();
  });
});

// ---------- V-94 体检医生 ----------

describe("V-94 doctor", () => {
  beforeEach(() => __resetForTests());

  it("healthy with default binds", () => {
    const r = runKeymapDoctor({ overrides: {} });
    expect(r.healthy).toBe(true);
    expect(r.freeSlots.length).toBeGreaterThan(0);
  });

  it("reports reserved hits as errors", () => {
    const r = runKeymapDoctor({ overrides: { explorer: "super+e" } });
    expect(r.healthy).toBe(false);
    expect(r.findings.some((f) => f.code === "reserved:hit")).toBe(true);
  });

  it("reports conflicts as errors", () => {
    const r = runKeymapDoctor({
      overrides: { notifyCenter: "ctrl+alt+v", clipboardHistory: "ctrl+alt+v" },
    });
    expect(r.findings.some((f) => f.code === "binds:conflict")).toBe(true);
  });

  it("reports bare keydown audit count as warn", () => {
    const r = runKeymapDoctor({ overrides: {}, bareKeydownCount: 7 });
    expect(r.findings.some((f) => f.code === "audit:bare-keydown" && f.severity === "warn")).toBe(true);
  });
});

// ---------- 既有 shortcuts 模块回归（Z-08 收编基线） ----------

describe("shortcuts baseline", () => {
  it("default binds have no conflicts", () => {
    expect(findConflicts(effectiveBinds({})).size).toBe(0);
  });

  it("default binds avoid system reserved", () => {
    for (const b of effectiveBinds({})) {
      if (b.accel === "") continue;
      expect(SYSTEM_RESERVED.has(b.accel)).toBe(false);
    }
  });

  it("normalizeAccel orders modifiers", () => {
    expect(normalizeAccel("ALT+CTRL+T")).toBe("ctrl+alt+t");
    expect(normalizeAccel("x")).toBeNull();
    expect(normalizeAccel("ctrl+e")).toBe("ctrl+e");
  });

  it("action count stable (9 launch slots)", () => {
    expect(SHORTCUT_ACTIONS.filter((a) => a.group === "launch")).toHaveLength(9);
  });

  it("registry accepts every default action accel (Z-08 收编自举)", () => {
    __resetForTests();
    for (const a of SHORTCUT_ACTIONS) {
      const r = register({ id: `act:${a.id}`, combo: a.accel, scope: "global", priority: 5, source: "bootstrap", descKey: a.labelKey });
      if (a.accel !== "") expect(r.ok).toBe(true);
    }
    expect(snapshot().length).toBe(SHORTCUT_ACTIONS.filter((a) => a.accel !== "").length);
    __resetForTests();
  });
});

// 保持 vi 引用（供后续异步计时测试扩展）
void vi;
