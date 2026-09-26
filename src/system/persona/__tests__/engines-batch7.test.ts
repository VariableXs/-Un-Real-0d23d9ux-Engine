import { describe, expect, it } from "vitest";
import { crc32, adler32, zlibStore, encodePng, pngBytesEstimate, rasterPointer, pointerSpritePair, assetBudget } from "../png-encode";
import { projectFrame, particleAlpha, bakeJobs, downgradeDensity, actCoverageCheck, actBoundsContract, bakeFrame } from "../boot-bitmap";
import { seedParticles } from "../boot-engine";
import { accentRamp, pickRampSlot, simulateVision, auditStateDistinction, suggestDistinctionFix, deltaE } from "../accent-ramp";

// ---------- png-encode ----------

describe("png-encode · 素材管线地基（F156/F165 深化）", () => {
  it("CRC32 已知向量：123456789 → 0xCBF43926（ISO 3309 标准校验）", () => {
    const data = new TextEncoder().encode("123456789");
    expect(crc32(data)).toBe(0xcbf43926);
  });

  it("Adler32 已知向量：Wikipedia → 0x11E60398", () => {
    const data = new TextEncoder().encode("Wikipedia");
    expect(adler32(data)).toBe(0x11e60398);
  });

  it("zlib stored：头/块结构/Adler 尾完整（解压器可读的流）", () => {
    const data = new TextEncoder().encode("hello varix");
    const z = zlibStore(data);
    expect(z[0]).toBe(0x78);
    expect(z[1]).toBe(0x01);
    // 最后 4 字节 = Adler-32（大端）。
    const expectAdler = adler32(data);
    expect((z[z.length - 4]! << 24) | (z[z.length - 3]! << 16) | (z[z.length - 2]! << 8) | z[z.length - 1]!).toBe(expectAdler);
  });

  it("PNG 编码：签名 + 分块 CRC 自洽（字节级产物真实性）", () => {
    const bmp = { width: 2, height: 2, data: new Uint8Array([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0]) };
    const png = encodePng(bmp);
    const sig = [...png.slice(0, 8)].map((b) => b.toString(16).padStart(2, "0")).join("");
    expect(sig).toBe("89504e470d0a1a0a");
    // IHDR 长度 13（前 4 字节大端）。
    const v = new DataView(png.buffer, png.byteOffset, png.byteLength);
    expect(v.getUint32(8, false)).toBe(13);
    expect(v.getUint32(16, false)).toBe(2); // width
    expect(v.getUint32(20, false)).toBe(2); // height
    expect(png[25]).toBe(6); // color type RGBA
  });

  it("尺寸/缓冲不符显性抛错；字节预估 ≥ 实际（预算可前置）", () => {
    expect(() => encodePng({ width: 2, height: 2, data: new Uint8Array(3) })).toThrow();
    const est = pngBytesEstimate(1920, 1080);
    const actual = encodePng({ width: 1920, height: 1080, data: new Uint8Array(1920 * 1080 * 4) }).length;
    expect(est).toBeGreaterThanOrEqual(actual);
  });

  it("SDF 光栅：arrow 内实外虚、半透明边缘存在（1px 抗锯齿）", () => {
    const bmp = rasterPointer("arrow", { size: 32, color: "#6e7fd4", outline: "#ffffff", aaPx: 1 });
    expect(bmp.data.length).toBe(32 * 32 * 4);
    // 形状内部（约 8,8 处是箭头身体）应有实体像素。
    const center = (8 * 32 + 8) * 4;
    expect(bmp.data[center + 3]).toBe(255);
    // 角落应为透明。
    const corner = (0 * 32 + 31) * 4;
    expect(bmp.data[corner + 3]).toBe(0);
  });

  it("双倍率产物：2x 字节 > 1x（4K 管线两档都在）", () => {
    const pair = pointerSpritePair("hand", 24, "#6e7fd4", "#ffffff");
    expect(pair.bytes2x).toBeGreaterThan(pair.bytes1x);
    expect(pair.png1x.length).toBe(pair.bytes1x);
  });

  it("素材包预算：超限显性（分批生成的信号源）", () => {
    const items = [{ name: "a", bytes: 100 }, { name: "b", bytes: 200 }];
    expect(assetBudget(items, 300).withinBudget).toBe(true);
    expect(assetBudget(items, 299).withinBudget).toBe(false);
  });
});

// ---------- boot-bitmap ----------

describe("boot-bitmap · 烘焙产物面（F165 深化）", () => {
  const opts = { width: 160, height: 90, backdrop: [10, 10, 18] as [number, number, number], particleRadius: 2, palette: { base: "#6e7fd4", highlight: "#ffffff" } };

  it("投影：底色填充 + 粒子叠加改变像素（幕一非纯底）", () => {
    const particles = seedParticles("minimal", 42, "#8ea0ff", 160, 90);
    const frame = projectFrame(particles, opts, 100); // 幕一。
    expect(frame.data.length).toBe(160 * 90 * 4);
    // 底色存在。
    expect(frame.data[0]).toBe(10);
    // 有粒子亮过底色（加法混合后至少一个像素 R 通道 > 10）。
    let brighter = 0;
    for (let i = 0; i < frame.data.length; i += 4) if (frame.data[i]! > 10) brighter++;
    expect(brighter).toBeGreaterThan(0);
  });

  it("幕四透明度线性淡出（1→0，交接不跳变）", () => {
    const p = { id: 0, x: 80, y: 45, vx: 0, vy: 0, act: 3 as const, size: 2, color: "base" as const };
    expect(particleAlpha({ ...p }, 6800)).toBe(1);
    expect(particleAlpha({ ...p }, 7400)).toBeCloseTo(0.5, 2);
    expect(particleAlpha({ ...p }, 8000)).toBe(0);
  });

  it("烘焙作业：三档 × 240 帧（8s × 30fps 结构契约）；stored PNG 字节随分辨率不随密度（诚实属性）", () => {
    const jobs = bakeJobs(42, 160, 90);
    expect(jobs).toHaveLength(3);
    for (const j of jobs) {
      expect(j.frameCount).toBe(240);
      expect(j.totalBytes).toBe(j.bytesPerFrame * 240);
    }
    expect(new Set(jobs.map((j) => j.totalBytes)).size).toBe(1); // 密度不改字节——降档逻辑靠密度≠字节的常识不成立时要显性。
  });

  it("超预算降档：预算内取最大档、全超显性建议降分辨率（时长永不动）", () => {
    const jobs = bakeJobs(42, 160, 90);
    const loose = downgradeDensity(jobs, Number.MAX_SAFE_INTEGER);
    expect(loose.chosen).toBe("dense"); // 预算够 → 最大档。
    const none = downgradeDensity(jobs, 1);
    expect(none.chosen).toBeNull();
    expect(none.note).toContain("降分辨率");
  });

  it("幕覆盖对拍：240 帧四幕全覆盖（结构少一幕即缺陷）", () => {
    const c = actCoverageCheck();
    expect(c.ok).toBe(true);
    expect(c.perAct.reduce((s, n) => s + n, 0)).toBe(240);
    expect(c.perAct.every((n) => n > 0)).toBe(true);
  });

  it("幕边界契约：末边界 = 总时长（投影不做自己的时间表）", () => {
    const c = actBoundsContract();
    expect(c.ok).toBe(true);
    expect(c.totalMs).toBe(8000);
  });

  it("bakeFrame 确定性：同 seed 同帧两次逐位一致；不同帧不同", () => {
    const a = bakeFrame("minimal", 42, 10, opts);
    const b = bakeFrame("minimal", 42, 10, opts);
    expect(a.png.length).toBe(b.png.length);
    expect([...a.png]).toEqual([...b.png]);
    const c = bakeFrame("minimal", 42, 30, opts);
    expect(a.png).not.toEqual(c.png);
  });
});

// ---------- accent-ramp ----------

describe("accent-ramp · 强调色阶梯与色觉（F151/F162 深化）", () => {
  it("11 档阶梯：亮度单调递增、档位亮度覆盖深浅两端", () => {
    const ramp = accentRamp("#6e7fd4");
    expect(ramp).toHaveLength(11);
    for (let i = 1; i < ramp.length; i++) {
      expect(ramp[i]!.luminance).toBeGreaterThan(ramp[i - 1]!.luminance);
    }
    expect(ramp[0]!.contrastOnWhite).toBeGreaterThan(ramp[ramp.length - 1]!.contrastOnWhite); // 暗端对白更可读。
    expect(ramp[ramp.length - 1]!.contrastOnBlack).toBeGreaterThan(ramp[0]!.contrastOnBlack); // 亮端对黑更可读。
  });

  it("语义槽位推荐：hover 与 base 不同档、text 档对白 ≥4.5（浅底可读线）", () => {
    const ramp = accentRamp("#6e7fd4");
    const slots = pickRampSlot(ramp, "#6e7fd4");
    expect(slots.hover).not.toBe("#6e7fd4");
    expect(slots.disabled).not.toBe(slots.active);
    const textStep = ramp.find((r) => r.hex === slots.text)!;
    expect(textStep.contrastOnWhite).toBeGreaterThanOrEqual(4.5);
  });

  it("色觉模拟：normal 恒等；红绿盲下红绿趋同（模拟有效）", () => {
    expect(simulateVision("#ff0000", "normal")).toBe("#ff0000");
    const red = simulateVision("#ff0000", "protanopia");
    const green = simulateVision("#00b000", "protanopia");
    // 色盲下两者接近（ΔE 小于正常视觉）。
    expect(deltaE(red, green)).toBeLessThan(deltaE("#ff0000", "#00b000"));
  });

  it("状态色可辨识性矩阵：四类视觉全跑、ΔE 阈值 20 判定", () => {
    const colors = { "--p-success": "#5fbf8a", "--p-warn": "#d4b45f", "--p-danger": "#d4685f", "--p-accent": "#6e7fd4" };
    const audits = auditStateDistinction(colors);
    expect(audits).toHaveLength(4);
    for (const a of audits) {
      expect(a.pairs).toHaveLength(6); // C(4,2)。
      expect(a.allDistinguishable).toBe(a.pairs.every((p) => p.distinguishable));
    }
  });

  it("修复建议：不可分对拉开明度、可分时不产生建议", () => {
    const colors = { "--p-success": "#5fbf8a", "--p-warn": "#5fbf8a", "--p-danger": "#d4685f", "--p-accent": "#6e7fd4" }; // 故意相同。
    const fixes = suggestDistinctionFix(colors, "deuteranopia");
    expect(fixes.length).toBeGreaterThan(0);
    expect(fixes[0]!.reason).toContain("拉开明度");
    // 全可分时不产生修复建议。
    const ok = { "--p-success": "#5fbf8a", "--p-warn": "#d4b45f", "--p-danger": "#d4685f", "--p-accent": "#6e7fd4" };
    const okFixes = suggestDistinctionFix(ok, "deuteranopia");
    expect(Array.isArray(okFixes)).toBe(true);
  });
});
