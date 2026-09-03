import { describe, expect, it } from "vitest";
import {
  generateStars,
  makeBandMask,
  composeMask,
  makeDensityMask,
  makeFbm2,
  makeSimplex2,
  mulberry32,
  poissonDisk,
  smoothstep,
  TIER_SPECS,
} from "./math";

describe("mulberry32 determinism", () => {
  it("same seed -> same sequence; different seed differs", () => {
    const a = mulberry32(42);
    const b = mulberry32(42);
    const c = mulberry32(43);
    const seqA = [a(), a(), a()];
    const seqB = [b(), b(), b()];
    expect(seqA).toEqual(seqB);
    expect(seqA).not.toEqual([c(), c(), c()]);
    for (const v of seqA) expect(v).toBeGreaterThanOrEqual(0), expect(v).toBeLessThan(1);
  });
});

describe("simplex / fbm noise", () => {
  it("stays within [-1,1] and is deterministic", () => {
    const n = makeSimplex2(7);
    for (let i = 0; i < 500; i++) {
      const v = n(i * 0.137, i * 0.291);
      expect(Math.abs(v)).toBeLessThanOrEqual(1.001);
    }
    expect(n(3.2, -4.5)).toBe(n(3.2, -4.5));
  });
  it("fbm stays within [-1,1]", () => {
    const f = makeFbm2(9);
    for (let i = 0; i < 200; i++) {
      expect(Math.abs(f(i * 0.71, i * 0.33))).toBeLessThanOrEqual(1.001);
    }
  });
  it("density mask lands in [0,1] with voids and clusters present", () => {
    const m = makeDensityMask(11);
    let lo = 1;
    let hi = 0;
    for (let i = 0; i < 4000; i++) {
      const v = m((i % 64) * 61.7, Math.floor(i / 64) * 47.3);
      lo = Math.min(lo, v);
      hi = Math.max(hi, v);
      expect(v).toBeGreaterThanOrEqual(-0.001);
      expect(v).toBeLessThanOrEqual(1.001);
    }
    expect(lo).toBeLessThan(0.35); // void rifts exist
    expect(hi).toBeGreaterThan(0.65); // clusters exist
  });
  it("smoothstep clamps outside the edges", () => {
    expect(smoothstep(0.35, 0.65, 0)).toBe(0);
    expect(smoothstep(0.35, 0.65, 1)).toBe(1);
  });
});

describe("milky-way band mask", () => {
  it("peaks on the centerline and fades far from it", () => {
    const w = 1600;
    const h = 900;
    const band = makeBandMask(5, w, h);
    let peak = 0;
    for (let y = 0; y < h; y += 4) {
      peak = Math.max(peak, band(w / 2, y));
    }
    expect(peak).toBeGreaterThan(0.8); // the river passes through mid-screen
    expect(band(0, 0)).toBeLessThan(0.4); // corners stay outside the band
    for (let i = 0; i < 200; i++) {
      const v = band(i * 13.7, i * 29.3);
      expect(v).toBeGreaterThanOrEqual(-0.001);
      expect(v).toBeLessThanOrEqual(1.001);
    }
  });
  it("compose takes the max of rifts and river", () => {
    const a = makeDensityMask(3);
    const b = makeBandMask(3, 1200, 800);
    const c = composeMask(a, b);
    for (let i = 0; i < 100; i++) {
      const x = i * 17.3;
      const y = i * 11.1;
      expect(c(x, y)).toBe(Math.max(a(x, y), b(x, y)));
    }
  });
});

describe("variable-radius poisson disk", () => {
  const mask = makeDensityMask(21);
  it("never packs two stars closer than ~rMin/2 in dense zones", () => {
    const pts = poissonDisk({
      x0: 0, y0: 0, width: 600, height: 600,
      rMin: 8, rMax: 20, seed: 5, mask, attempts: 16, maxPoints: 4000,
    });
    expect(pts.length).toBeGreaterThan(150);
    // Hard floor: acceptance rule is dist >= (ri+rj)/2, and ri >= rMin, so
    // no pair may be closer than rMin/2.
    let minD = Infinity;
    for (let i = 0; i < pts.length; i++) {
      for (let j = i + 1; j < pts.length; j++) {
        const dx = pts[i]!.x - pts[j]!.x;
        const dy = pts[i]!.y - pts[j]!.y;
        minD = Math.min(minD, Math.hypot(dx, dy));
      }
    }
    expect(minD).toBeGreaterThanOrEqual(8 * 0.5 - 1e-6);
  }, 20000);
  it("rejects points inside void rifts entirely", () => {
    const pts = poissonDisk({
      x0: 0, y0: 0, width: 800, height: 800,
      rMin: 10, rMax: 22, seed: 6, mask, attempts: 14, maxPoints: 3000,
    });
    for (const p of pts) expect(mask(p.x, p.y)).toBeGreaterThanOrEqual(0.28);
  }, 20000);
});

describe("generateStars packing & class mix", () => {
  it("emits stride-6 floats with valid ranges", () => {
    const { data, count } = generateStars({ viewWidth: 1280, viewHeight: 800, seed: 3, targetCount: 900 });
    expect(count).toBeGreaterThan(300);
    expect(data.length).toBe(count * 6);
    for (let i = 0; i < count; i++) {
      const size = data[i * 6 + 2]!;
      const depth = data[i * 6 + 3]!;
      const phase = data[i * 6 + 4]!;
      const freq = data[i * 6 + 5]!;
      expect(depth).toBeGreaterThanOrEqual(0.0999);
      expect(depth).toBeLessThanOrEqual(1.0001);
      expect(phase).toBeGreaterThanOrEqual(0);
      expect(phase).toBeLessThan(Math.PI * 2 + 0.001);
      expect(freq).toBeGreaterThanOrEqual(0.5);
      expect(freq).toBeLessThanOrEqual(2.0001);
      if (size <= 1.0) continue; // far dust
      if (size <= 1.8) continue; // mid
      expect(size).toBeLessThanOrEqual(3.2001); // near flare
    }
  });
  it("near flare stars keep >= 26px separation from each other", () => {
    const { data, count } = generateStars({ viewWidth: 1400, viewHeight: 900, seed: 77, targetCount: 1200 });
    const near: [number, number][] = [];
    for (let i = 0; i < count; i++) {
      if (data[i * 6 + 2]! > 1.9) near.push([data[i * 6]!, data[i * 6 + 1]!]);
    }
    expect(near.length).toBeGreaterThan(3);
    for (let i = 0; i < near.length; i++) {
      for (let j = i + 1; j < near.length; j++) {
        const d = Math.hypot(near[i]![0] - near[j]![0], near[i]![1] - near[j]![1]);
        expect(d).toBeGreaterThanOrEqual(26 - 1e-6);
      }
    }
  });
  it("is deterministic per seed", () => {
    const a = generateStars({ viewWidth: 800, viewHeight: 600, seed: 99, targetCount: 400 });
    const b = generateStars({ viewWidth: 800, viewHeight: 600, seed: 99, targetCount: 400 });
    expect(Array.from(a.data)).toEqual(Array.from(b.data));
  });
});

describe("ten-tier matrix integrity", () => {
  it("has exactly 10 monotonic levels with cumulative features", () => {
    expect(TIER_SPECS).toHaveLength(10);
    for (let i = 1; i < TIER_SPECS.length; i++) {
      const prev = TIER_SPECS[i - 1]!;
      const cur = TIER_SPECS[i]!;
      expect(cur.level).toBe(prev.level + 1);
      expect(cur.fpsTarget).toBeGreaterThanOrEqual(prev.fpsTarget);
      expect(cur.starBudget).toBeGreaterThan(prev.starBudget); // 200 → 200k ladder
      if (prev.flare) expect(cur.flare).toBe(true);
      if (prev.screentone) expect(cur.screentone).toBe(true);
      if (prev.auroraFlow) expect(cur.auroraFlow).toBe(true);
    }
    expect(TIER_SPECS[0]!.continuous).toBe(false); // L1 baked static
    expect(TIER_SPECS[0]!.starBudget).toBe(200);
    expect(TIER_SPECS[9]!.starBudget).toBe(200000); // L10 ultra dust
    expect(TIER_SPECS[9]!.detailMaster).toBe(true); // L10 ray-marched silk
    expect(TIER_SPECS[7]!.auroraFlow).toBe(true); // L8 flowing aurora
    expect(TIER_SPECS[6]!.bloom).toBe(true); // L7 screen-space soft bloom
    expect(TIER_SPECS[5]!.flare).toBe(true); // L6 anime diamond flares
    for (const spec of TIER_SPECS) {
      expect(spec.aurora).toBeLessThanOrEqual(3);
      expect(Object.keys(spec)).not.toContain("meteors"); // no meteors, ever
    }
  });

  it("power-law sizes skew small within every class", () => {
    const { data, count } = generateStars({ viewWidth: 1280, viewHeight: 800, seed: 31, targetCount: 1500 });
    const far: number[] = [];
    for (let i = 0; i < count; i++) {
      const s = data[i * 6 + 2]!;
      if (s <= 1.0) far.push(s);
    }
    expect(far.length).toBeGreaterThan(200);
    far.sort((a, b) => a - b);
    const median = far[Math.floor(far.length / 2)]!;
    // pow(random, 2.6) skew: median of 0.5..1.0 must sit well below midpoint.
    expect(median).toBeLessThan(0.5 + 0.5 * 0.35);
  });

  it("overflow budget becomes sub-pixel dust without breaking packing", () => {
    const { data, count } = generateStars({
      viewWidth: 1280, viewHeight: 800, seed: 8, targetCount: 45000,
    });
    expect(count).toBeGreaterThan(30000); // dust overflow filled the ladder
    expect(data.length).toBe(count * 6);
    let dust = 0;
    for (let i = 0; i < count; i++) {
      const s = data[i * 6 + 2]!;
      if (s < 0.9) dust++;
      expect(s).toBeLessThanOrEqual(3.2001);
    }
    expect(dust).toBeGreaterThan(20000);
  });
});
