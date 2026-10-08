import { describe, expect, it } from "vitest";
import { auditFalsePositive, findDuplicates, idleAdmission, restoreFromTrash, trashChecked, type ScannedFile } from "../f365-dupeFinder";

function f(path: string, size: number, hash: string, mtime = 0): ScannedFile {
  return { path, sizeBytes: size, hash, mtimeMs: mtime };
}

describe("F365 重复文件查找", () => {
  it("内容哈希命中：改名重复现形；大小不同不误并组", () => {
    const report = findDuplicates([
      f("S:/Downloads/a.zip", 100, "h1", 1),
      f("S:/Docs/副本.zip", 100, "h1", 5), // 改名+挪目录的重复
      f("S:/x/other.zip", 100, "h2", 0), // 同大小不同内容
      f("S:/y/unique.bin", 999, "h3", 0),
    ]);
    expect(report.groups).toHaveLength(1);
    expect(report.groups[0]!.files.map((x) => x.path)).toEqual(["S:/Docs/副本.zip", "S:/Downloads/a.zip"]);
    expect(report.reclaimableBytes).toBe(100);
  });

  it("组排序：浪费最大的在前；保留推荐=最新修改（同刻取字典序）", () => {
    const report = findDuplicates([
      f("a", 10, "h-small", 0), f("b", 10, "h-small", 0),
      f("c", 500, "h-big", 0), f("d", 500, "h-big", 0), f("e", 500, "h-big", 0),
    ]);
    expect(report.groups.map((g) => g.hash)).toEqual(["h-big", "h-small"]);
    expect(report.groups[0]!.wastedBytes).toBe(1000);
    const tie = findDuplicates([f("b", 1, "h", 7), f("a", 1, "h", 7)]);
    expect(tie.groups[0]!.keepPath).toBe("a"); // 同刻字典序
  });

  it("空闲扫描准入：前台活跃让路、静默窗满放行（F050 纪律）", () => {
    expect(idleAdmission(5000, 4000).admitted).toBe(false);
    expect(idleAdmission(5000, 4000).reason).toContain("让路");
    expect(idleAdmission(5000, 1000).admitted).toBe(true);
    expect(idleAdmission(0, -1).admitted).toBe(true); // 无前台 IO 记录 = 空闲
  });

  it("删除走回收站：推荐项受保护（勾了也不删）、其余入账可还原", () => {
    const report = findDuplicates([f("keep", 10, "h", 9), f("kill1", 10, "h", 1), f("kill2", 10, "h", 2)]);
    const g = report.groups[0]!;
    const r = trashChecked(g, ["kill1", "keep", "不存在"], 100);
    expect(r.trashed.map((t) => t.path)).toEqual(["kill1"]);
    expect(r.protectedKept).toEqual(["keep"]);
    const restore = restoreFromTrash(r.trashed, "kill1");
    expect(restore).toEqual({ ok: true, originalDir: "/" }); // 裸文件名无目录 → 根
    expect(restoreFromTrash(r.trashed, "kill2").ok).toBe(false);
  });

  it("误判率审计：抽查 20 组全哈希全等 → 零误判", () => {
    const report = findDuplicates(Array.from({ length: 60 }, (_, i) => f(`f${i}`, 5, `h${i % 12}`, i)));
    const a = auditFalsePositive(report);
    expect(a.checkedGroups).toBe(12); // 12 个真实组
    expect(a.misjudged).toBe(0);
  });
});
