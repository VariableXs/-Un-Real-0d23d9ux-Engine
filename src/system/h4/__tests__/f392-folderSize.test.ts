import { describe, expect, it } from "vitest";
import { ESTIMATE_THRESHOLD_FILES, MEASURE_BUDGET_MS, auditSharedSource, displaySize, emptyCache, fillPhase, formatBytes, measureCached, measureDir, type FsNode } from "../f392-folderSize";

function dir(path: string, version: string, children: FsNode[]): FsNode {
  return { path, sizeBytes: 0, isDir: true, version, children };
}

function file(path: string, size: number): FsNode {
  return { path, sizeBytes: size, isDir: false, version: "1" };
}

describe("F392 文件夹大小列", () => {
  it("递归计量：子树字节累计准确", () => {
    const tree = dir("S:/x", "v1", [file("a", 100), file("b", 250), dir("sub", "v1", [file("c", 50)])]);
    const r = measureDir(tree, 0);
    expect(r.bytes).toBe(400);
    expect(r.estimated).toBe(false);
  });

  it("估算标注阈值：文件数超 5 万 → 估算态（判据）", () => {
    expect(ESTIMATE_THRESHOLD_FILES).toBe(50_000);
    const big = dir("S:/huge", "v1", Array.from({ length: ESTIMATE_THRESHOLD_FILES + 1 }, (_, i) => file(`f${i}`, 1)));
    const r = measureDir(big, 0);
    expect(r.estimated).toBe(true);
    expect(displaySize(r).startsWith("~")).toBe(true);
  });

  it("计算预算超时 → 估算（判据「算不完给估算」）", () => {
    expect(MEASURE_BUDGET_MS).toBe(2000);
    let calls = 0;
    const clock = (): number => (calls++ > 2 ? 3000 : 0); // 第 3 次读取时"超时"
    const tree = dir("S:/x", "v1", Array.from({ length: 10 }, (_, i) => file(`f${i}`, 1)));
    const r = measureDir(tree, 0, clock);
    expect(r.estimated).toBe(true);
  });

  it("显示格式：B/KB/MB/GB 分级与「~」前缀", () => {
    expect(displaySize({ path: "x", bytes: 512, estimated: false, tookMs: 0 })).toBe("512 B");
    expect(displaySize({ path: "x", bytes: 2048, estimated: false, tookMs: 0 })).toBe("2.0 KB");
    expect(displaySize({ path: "x", bytes: 5 * 1024 * 1024, estimated: true, tookMs: 0 })).toBe("~5.0 MB");
    expect(formatBytes(1.5 * 1024 * 1024 * 1024)).toBe("1.5 GB");
  });

  it("缓存判据：version 不变秒出（cacheHit、tookMs=0）；变了重算回填", () => {
    const tree = dir("S:/x", "v1", [file("a", 100)]);
    const cache = emptyCache();
    const r1 = measureCached(tree, cache, 0);
    expect(r1.cacheHit).toBe(false);
    expect(r1.result.bytes).toBe(100);
    const r2 = measureCached(tree, cache, 0);
    expect(r2.cacheHit).toBe(true);
    expect(r2.result.tookMs).toBe(0);
    const changed = dir("S:/x", "v2", [file("a", 100), file("b", 20)]);
    const r3 = measureCached(changed, cache, 0);
    expect(r3.cacheHit).toBe(false);
    expect(r3.result.bytes).toBe(120);
  });

  it("三功能同源对账（判据）：消费者共享同一结果", () => {
    const r: SizeResult2 = { path: "S:/x", bytes: 100, estimated: false, tookMs: 1 };
    expect(auditSharedSource(r, ["F268-space-warning", "F365-dupe-finder", "F393-treemap"]).pass).toBe(true);
  });

  it("异步淡入填入状态机（判据）：占位→计量→填入", () => {
    expect(fillPhase(false, false)).toBe("placeholder");
    expect(fillPhase(false, true)).toBe("measuring");
    expect(fillPhase(true, true)).toBe("filled");
  });
});

type SizeResult2 = ReturnType<typeof measureDir>;
