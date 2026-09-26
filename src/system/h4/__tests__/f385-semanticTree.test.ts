import { describe, expect, it } from "vitest";
import { ROLES, announceDialog, auditNameCoverage, auditRoleState, createTree, readableProgress, readableSwitchState, readerInterfaceDoc, speakHierarchy, type SemanticNode } from "../f385-semanticTree";

function goodTree(): SemanticNode[] {
  return [
    { id: "dlg", role: "dialog", name: "保存文档", nameSource: "text", state: null, parentIds: [] },
    { id: "btn-save", role: "button", name: "保存", nameSource: "text", state: null, parentIds: ["dlg"] },
    { id: "sw-auto", role: "switch", name: "自动保存", nameSource: "tooltip", state: "开", parentIds: ["dlg"] },
    { id: "bar", role: "progressbar", name: "导出进度", nameSource: "label", state: "进度 40%", parentIds: ["dlg"] },
  ];
}

describe("F385 无障碍语义树", () => {
  it("角色词表对齐开放标准（F141 纪律）", () => {
    expect(ROLES).toContain("dialog");
    expect(ROLES).toContain("switch");
    expect(ROLES).toHaveLength(14);
  });

  it("覆盖率审计：无名可交互元素=0 才合格（判据）", () => {
    expect(auditNameCoverage(createTree(goodTree()))).toEqual({ pass: true, unnamed: [] });
    const bad = createTree([{ id: "icon-btn", role: "button", name: null, nameSource: null, state: null, parentIds: [] }]);
    const r = auditNameCoverage(bad);
    expect(r.pass).toBe(false);
    expect(r.unnamed).toEqual(["icon-btn"]);
  });

  it("角色/状态完整性抽查 50 控件（判据）", () => {
    const many: SemanticNode[] = Array.from({ length: 60 }, (_, i) => ({
      id: `n${i}`,
      role: i % 3 === 0 ? "switch" : "button",
      name: `控件${i}`,
      nameSource: "text",
      state: i % 3 === 0 ? "关" : null,
      parentIds: [],
    }));
    const r = auditRoleState(createTree(many));
    expect(r.sampled).toBe(50);
    expect(r.pass).toBe(true);
    const noState = createTree([{ id: "sw", role: "switch", name: "开关", nameSource: "text", state: null, parentIds: [] }]);
    expect(auditRoleState(noState).unreadableStates).toEqual(["sw"]);
    const badRole = createTree([{ id: "x", role: "hacker" as never, name: "x", nameSource: "text", state: null, parentIds: [] }]);
    expect(auditRoleState(badRole).badRoles).toEqual(["x"]);
  });

  it("状态可读化：开关读开/关（不靠视觉）、进度带单位", () => {
    expect(readableSwitchState(true)).toBe("开");
    expect(readableSwitchState(false)).toBe("关");
    expect(readableProgress(66.6)).toBe("进度 67%");
    expect(readableProgress(150)).toBe("进度 100%");
  });

  it("弹窗打开即播报（判据）", () => {
    expect(announceDialog("保存文档")).toEqual({ text: "对话框：保存文档", priority: "polite" });
  });

  it("层级朗读：路径链完整（判据「层级」语义）", () => {
    const tree = createTree(goodTree());
    expect(speakHierarchy(tree, "btn-save")).toBe("位于 保存文档 的 保存");
    expect(speakHierarchy(tree, "dlg")).toBe("保存文档");
    expect(speakHierarchy(tree, "ghost")).toBe("");
  });

  it("第三方读屏接口文档公开（F135 联动判据）", () => {
    const doc = readerInterfaceDoc();
    expect(doc.standard).toContain("F141");
    expect(doc.endpoints.length).toBeGreaterThanOrEqual(3);
  });
});
