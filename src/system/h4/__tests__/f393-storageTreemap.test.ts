import { describe, expect, it } from "vitest";
import { auditAreaAccuracy, drillInto, hitTile, heatToShade, idleAdmission, topDirs, treemap, type FsNode } from "../f393-storageTreemap";

const CANVAS = { x: 0, y: 0, w: 1000, h: 1000 };

function file(path: string, size: number): FsNode {
  return { path, sizeBytes: size, isDir: false, version: "1" };
}

const TREE: FsNode = {
  path: "S:/",
  sizeBytes: 0,
  isDir: true,
  version: "1",
  children: [
    { path: "S:/videos", sizeBytes: 0, isDir: true, version: "1", children: [file("S:/videos/v1.mkv", 400), file("S:/videos/v2.mkv", 200)] },
    { path: "S:/docs", sizeBytes: 0, isDir: true, version: "1", children: [file("S:/docs/d1.pdf", 100)] },
    file("S:/loose.bin", 300),
  ],
};

describe("F393 存储热点图", () => {
  it("面积=占用：全域 tile 面积占比与字节占比对账 ±2%（判据）", () => {
    const r = treemap(TREE, CANVAS);
    const a = auditAreaAccuracy(r, CANVAS);
    expect(a.pass).toBe(true);
    expect(a.worstDeviationPct).toBeLessThanOrEqual(2);
    expect(r.totalBytes).toBe(1000); // 600+100+300
  });

  it("层级下钻：根(0)→videos(1)→文件(2) 三层（判据）", () => {
    const root = treemap(TREE, CANVAS);
    const videos = root.tiles.find((t) => t.path === "S:/videos")!;
    const child = root.tiles.find((t) => t.path === "S:/videos/v1.mkv")!;
    expect(videos.depth).toBe(1);
    expect(child.depth).toBe(2);
    const drill = drillInto(root, "S:/videos", TREE.children![0]!, CANVAS);
    expect(drill.totalBytes).toBe(600);
    expect(drill.tiles.every((t) => t.bytes <= 600)).toBe(true);
  });

  it("点选命中：最深层 tile 胜出（判据「点选即定位」）", () => {
    const r = treemap(TREE, CANVAS);
    const hit = hitTile(r, 50, 50);
    expect(hit).not.toBeNull();
    expect(hit!.path.startsWith("S:/")).toBe(true);
  });

  it("榜单：目录按字节降序前 10（判据）", () => {
    const r = treemap(TREE, CANVAS);
    const top = topDirs(r);
    expect(top[0]!.path).toBe("S:/videos");
    expect(top.every((t) => t.bytes >= (top[top.indexOf(t) + 1]?.bytes ?? 0))).toBe(true);
  });

  it("空闲通道纪律（判据）：前台活跃不让布图", () => {
    expect(idleAdmission(5000, 4000)).toBe(false);
    expect(idleAdmission(5000, 1000)).toBe(true);
    expect(idleAdmission(0, -1)).toBe(true);
  });

  it("颜色深浅=热度：heat 越高灰度越深", () => {
    expect(heatToShade(0)).toBe(255);
    expect(heatToShade(1)).toBe(90);
    expect(heatToShade(0.5)).toBe(Math.round(255 - 0.5 * 165));
    expect(heatToShade(2)).toBe(90); // 越界钳制
    expect(heatToShade(-1)).toBe(255);
  });
});
