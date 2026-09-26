import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import {
  SYSTEM_TREATMENT_CHECKLIST,
  auditTreatment,
  classifyDrop,
  downloadNotifyPlan,
  listCompatIssues,
  recordCompatIssue,
  uaPolicy,
} from "../f355-edgeSynergy";

describe("F355 Edge 深度协同", () => {
  it("下载管：完成通知带双动作并联动 F357（校验失败不联动收口）", () => {
    const ok = downloadNotifyPlan({ id: "d1", fileName: "a.pdf", sizeBytes: 2 * 1024 * 1024, targetPath: "S:/Downloads/a.pdf", hashOk: true });
    expect(ok.actions.map((a) => a.id)).toEqual(["open", "reveal"]);
    expect(ok.handoffToF357).toBe(true);
    expect(ok.body).toContain("S:/Downloads/a.pdf");
    const bad = downloadNotifyPlan({ id: "d2", fileName: "b.zip", sizeBytes: 512, targetPath: "x", hashOk: false });
    expect(bad.handoffToF357).toBe(false); // 哈希失败交给 F357 重下路径，不走普通收口
  });

  it("拖拽管：F255 四落点语义逐格对齐", () => {
    const text = { kind: "text" as const, text: "hi" };
    expect(classifyDrop("editor", text).accepted).toBe(true);
    expect(classifyDrop("desktop", text).accepted).toBe(false);
    expect(classifyDrop("explorer", text).accepted).toBe(false);
    expect(classifyDrop("taskbar", text).accepted).toBe(false);
    expect(classifyDrop("desktop", { kind: "link", text: "u" }).accepted).toBe(true);
    expect(classifyDrop("taskbar", { kind: "file", text: "" }).accepted).toBe(false);
  });

  it("待遇管：清单五项齐备；任一失格 → 整体不合格且原因入册", () => {
    expect(SYSTEM_TREATMENT_CHECKLIST).toHaveLength(5);
    const all = auditTreatment({});
    expect(all.pass).toBe(true);
    const some = auditTreatment({ snap: "贴靠手势被页面拦截" });
    expect(some.pass).toBe(false);
    expect(some.items.find((i) => i.id === "snap")!.entitled).toBe(false);
    expect(some.items.find((i) => i.id === "snap")!.reason).toContain("贴靠");
    expect(some.items.find((i) => i.id === "altTab")!.entitled).toBe(true);
  });

  it("诚实边界：UA 永不伪装（类型级 + 值级双保证）", () => {
    const ua = uaPolicy();
    expect(ua.spoofingAllowed).toBe(false);
    expect(ua.productToken).toBe("VARIX");
  });

  it("兼容问题登记：回退到 A2 判例工厂，留痕可查（上限 50 滚动）", () => {
    __clearMem();
    const s = memStore();
    for (let i = 0; i < 55; i++) recordCompatIssue(`https://x/${i}`, "布局错乱", 1000 + i, s);
    const all = listCompatIssues(s);
    expect(all).toHaveLength(50);
    expect(all[0]!.url).toBe("https://x/5");
    expect(all[49]!.caseFactoryRef).toBe("A2/case-factory");
  });
});
