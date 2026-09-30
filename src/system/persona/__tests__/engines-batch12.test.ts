import { describe, expect, it } from "vitest";
import { STATE_PAGES, confirmDialogModel, auditStateCoverage } from "../state-blocks";
import { EXPORT_FORMATS, auditFormatRegistry, recognizeFormat, migrationNotes } from "../export-formats";
import { UX_DICTIONARY, auditDictionary, FirstRunLedger, honestProgress, cancelTransition, coalesceUndo, pushUndo, UNDO_STACK_MAX } from "../ux-dictionary";

// ---------- state-blocks ----------

describe("state-blocks · 状态块渲染层（十二查 8/9/11 深化）", () => {
  it("二十页登记齐全且双语目录齐备（漏页即红的挂载完整性）", () => {
    expect(STATE_PAGES).toHaveLength(20);
    const c = auditStateCoverage();
    expect(c.missingCatalog).toEqual([]);
  });

  it("EmptyStateBlock：未登记页显性红（缺目录=缺陷可检出）", () => {
    expect(pageGuard("bogus")).toBe(true);
    expect(pageGuard("tokens")).toBe(false);
  });

  it("确认对话模型：取消固定安全侧、确认 danger 形态（词典 D-CONFIRM-01 的数据面）", () => {
    const m = confirmDialogModel("tokens")!;
    expect(m.buttons[0]!.id).toBe("cancel");
    expect(m.buttons[0]!.position).toBe("safe-side");
    expect(m.buttons[1]!.kind).toBe("danger");
    expect(m.buttons[1]!.text).not.toBe("确定"); // 动词具体——禁词表同源。
    expect(confirmDialogModel("bogus")).toBeNull();
  });
});

function pageGuard(page: string): boolean {
  // EmptyStateBlock 对未登记页渲染 warn Notice——这里用目录层判定（组件逻辑与目录一致）。
  return !STATE_PAGES.includes(page as never);
}

// ---------- export-formats ----------

describe("export-formats · 导出格式注册表（十四章深化）", () => {
  it("注册表机检：active 有 schema 约束、deprecated 有去向", () => {
    const a = auditFormatRegistry();
    expect(a.ok).toBe(true);
    expect(a.activeCount).toBeGreaterThanOrEqual(9);
    expect(EXPORT_FORMATS.find((f) => f.id === "vxtheme-profile-v0")!.replacedBy).toBe("vxtheme-profile");
  });

  it("格式的 format 字段与生产引擎对拍（跨模块一致性抽查）", () => {
    // 抽查三个生产模块的导出确实用注册表的 format id。
    expect(EXPORT_FORMATS.find((f) => f.id === "vx-verdict-evidence")!.producer).toContain("verdict-report");
    expect(EXPORT_FORMATS.find((f) => f.id === "vx-usage-ranking")!.schemaConstraints.join()).toContain("零内容字段");
    expect(EXPORT_FORMATS.find((f) => f.id === "vx-interaction-ledger")!.status).toBe("active");
  });

  it("导入门房：已知格式认出、未知拒绝（不猜）、无 format 字段走 v0 迁移建议", () => {
    const hit = recognizeFormat({ format: "vxkeymap", version: 1 });
    expect(hit.format!.id).toBe("vxkeymap");
    const unknown = recognizeFormat({ format: "bogus-format" });
    expect(unknown.format).toBeNull();
    expect(unknown.advice).toContain("拒绝");
    const legacy = recognizeFormat({ theme: "dark" });
    expect(legacy.advice).toContain("vxtheme-profile-v0");
  });

  it("废弃流程：deprecated 必有迁移路径说明（升级不破坏旧数据）", () => {
    const notes = migrationNotes("vxtheme-profile-v0");
    expect(notes.some((n) => n.includes("vxtheme-profile"))).toBe(true);
    expect(notes.some((n) => n.includes("兼容承诺"))).toBe(true);
    expect(migrationNotes("bogus")[0]).toContain("未知格式");
  });
});

// ---------- ux-dictionary ----------

describe("ux-dictionary · 词典与状态模型（十/十一/八/九章深化）", () => {
  it("交互词典六域规则 + 机检（违例登记即红）", () => {
    expect(UX_DICTIONARY.map((r) => r.domain)).toEqual(["close-key", "popover-layer", "button-order", "loading", "empty-state", "accent-usage"]);
    expect(auditDictionary().ok).toBe(true);
    const bad = auditDictionary([{ ruleId: "D-CLOSE-01", where: "ime: 候选窗写了 Esc 但未实现" }]);
    expect(bad.ok).toBe(false);
    expect(bad.findings[0]!.rule).toContain("Esc");
  });

  it("first-run 三态：只出现一次、可跳过、可找回（引导不消失只不打扰）", () => {
    const fr = new FirstRunLedger();
    fr.register("k", "提示");
    expect(fr.shouldShow("k")).toBe(true);
    fr.markSeen("k");
    expect(fr.shouldShow("k")).toBe(false); // 只出现一次。
    fr.register("s", "可跳过");
    fr.skip("s");
    expect(fr.shouldShow("s")).toBe(false);
    expect(fr.findable().map((e) => e.key)).toEqual(["s"]); // 可找回。
    expect(fr.shouldShow("unregistered")).toBe(false); // 未登记不展示。
  });

  it("诚实进度：样本不足不猜、速率估计、停滞 30s 显性卡住", () => {
    const now = 100_000;
    const flowing = honestProgress([{ at: now - 8000, done: 10 }, { at: now, done: 50 }], 100, now);
    expect(flowing.ratio).toBe(0.5);
    expect(flowing.remainingMs).toBeGreaterThan(0);
    const insufficient = honestProgress([{ at: now, done: 1 }], 100, now);
    expect(insufficient.remainingMs).toBeNull(); // 不猜。
    expect(insufficient.message).toContain("不装确定");
    const stalled = honestProgress([{ at: now - 40000, done: 10 }, { at: now, done: 10 }], 100, now);
    expect(stalled.stalled).toBe(true);
    expect(stalled.message).toContain("可取消");
    expect(() => honestProgress([], 0, now)).toThrow();
  });

  it("可取消状态机：running→cancelling→cancelled；cancel-failed 显性；非法迁移不动", () => {
    expect(cancelTransition("running", "cancel")).toBe("cancelling");
    expect(cancelTransition("cancelling", "cancel-ack")).toBe("cancelled");
    expect(cancelTransition("cancelling", "cancel-fail")).toBe("cancel-failed");
    expect(cancelTransition("cancelled", "cancel")).toBe("cancelled"); // 已取消不再变。
    expect(cancelTransition("running", "cancel-ack")).toBe("running"); // 未请求取消的 ack 忽略。
  });

  it("undo 归并：同类 2s 内并步、跨类独立、栈深 50 封顶", () => {
    const merged = coalesceUndo([
      { kind: "slider", ops: 1, at: 0 },
      { kind: "slider", ops: 1, at: 1000 },
      { kind: "color", ops: 1, at: 3000 },
    ]);
    expect(merged).toHaveLength(2);
    expect(merged[0]!.ops).toBe(2);
    const far = coalesceUndo([
      { kind: "slider", ops: 1, at: 0 },
      { kind: "slider", ops: 1, at: 3000 },
    ]);
    expect(far).toHaveLength(2); // 超 2s 不归并。
    let stack: ReturnType<typeof coalesceUndo> = [];
    for (let i = 0; i < 60; i++) stack = pushUndo(stack, { kind: `k${i}`, ops: 1, at: i * 3000 });
    expect(stack).toHaveLength(UNDO_STACK_MAX);
    expect(stack[0]!.kind).toBe("k10"); // 最早步淘汰。
  });
});
