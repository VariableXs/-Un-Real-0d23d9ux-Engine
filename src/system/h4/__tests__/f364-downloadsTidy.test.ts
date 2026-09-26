import { describe, expect, it } from "vitest";
import { auditNoAutoMove, buildProposal, classifyFile, executeTidy, idempotencyCheck, undoBatch, type DownloadFile } from "../f364-downloadsTidy";

const FILES: DownloadFile[] = [
  { name: "报告.docx", sizeBytes: 100 },
  { name: "photo.JPG", sizeBytes: 200 },
  { name: "setup.exe", sizeBytes: 300 },
  { name: "backup.7z", sizeBytes: 400 },
  { name: "noext", sizeBytes: 10 },
  { name: "readme.PDF", sizeBytes: 20 },
];

describe("F364 下载文件夹一键整理", () => {
  it("分类零错放：扩展名映射全对（大小写不敏感、无扩展名→其他）", () => {
    expect(classifyFile(FILES[0]!)).toBe("document");
    expect(classifyFile(FILES[1]!)).toBe("image");
    expect(classifyFile(FILES[2]!)).toBe("installer");
    expect(classifyFile(FILES[3]!)).toBe("archive");
    expect(classifyFile(FILES[4]!)).toBe("other");
    expect(classifyFile(FILES[5]!)).toBe("document");
  });

  it("方案预览：每类「多少项去哪」、目标目录正确、合计字节数对账", () => {
    const proposal = buildProposal(FILES);
    const doc = proposal.find((g) => g.category === "document")!;
    expect(doc.files).toHaveLength(2);
    expect(doc.targetDir).toContain("文档");
    expect(doc.totalBytes).toBe(120);
    expect(proposal.reduce((s, g) => s + g.files.length, 0)).toBe(FILES.length);
  });

  it("勾选执行：只动被勾选类目", () => {
    const proposal = buildProposal(FILES);
    const exec = executeTidy(proposal, ["document"], new Set());
    expect(exec.moves).toHaveLength(2);
    expect(exec.moves.every((m) => m.category === "document")).toBe(true);
  });

  it("整批撤销：逆向移动计划一一对应（F202 整批语义）", () => {
    const proposal = buildProposal(FILES);
    const exec = executeTidy(proposal, ["image", "installer"], new Set());
    const undo = undoBatch(exec);
    expect(undo).toHaveLength(exec.moves.length);
    expect(undo.every((m) => m.to === "S:/Downloads")).toBe(true);
  });

  it("自动动手=0：模块无任何自动触发面（结构性审计）", () => {
    const a = auditNoAutoMove();
    expect(a.autoTriggers).toBe(0);
    expect(a.requiresExplicitCall).toBe(true);
  });

  it("重复执行幂等：第二轮全部跳过、零移动", () => {
    const r = idempotencyCheck(FILES);
    expect(r.firstRun.moves.length).toBeGreaterThan(0);
    expect(r.secondRun.moves).toHaveLength(0);
    expect(r.idempotent).toBe(true);
  });
});
