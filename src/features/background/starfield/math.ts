/**
 * Pale-Blue / Pure-White Aurora Sky — pure math core.
 *
 * Everything here is deterministic given a seed and free of DOM/WebGL so it
 * is unit-testable and reusable from a Web Worker. The renderer consumes the
 * packed Float32Array produced by `generateStars`.
 *
 * Palette law: LIGHT BLUE and PURE WHITE only. No concrete celestial bodies
 * (moon / planets / meteors) are ever simulated — light itself is the subject.
 */

// ---------- chapter 1: light-blue / pure-white palette ----------

/** Daylight sky gradient scheme (linear 0-1 RGB), high-key & low-saturation. */
export const BG_APEX = Object.freeze([0xa6 / 255, 0xc8 / 255, 0xec / 255]); // pale zenith blue
export const BG_MID = Object.freeze([0xc3 / 255, 0xdc / 255, 0xf4 / 255]); // clear horizon wash
export const BG_CORE = Object.freeze([0xea / 255, 0xf3 / 255, 0xfc / 255]); // near-white floor

/** Aurora ribbon tones: pure-white core → light-blue skirt. */
export const AURORA_CORE = Object.freeze([1.0, 1.0, 1.0]); // pure white highlight
export const AURORA_EDGE = Object.freeze([0x8f / 255, 0xb8 / 255, 0xe4 / 255]); // light blue edge

/** Star palette, cumulative weights sum to 1 (white-dominant, blue accents). */
export const STAR_PALETTE: ReadonlyArray<{ rgb: [number, number, number]; weight: number }> = Object.freeze([
  { rgb: [1.0, 1.0, 1.0], weight: 0.42 },                       // pure white main
  { rgb: [0xe4 / 255, 0xf0 / 255, 0xfd / 255], weight: 0.28 },  // faint ice white
  { rgb: [0xd8 / 255, 0xe8 / 255, 0xfa / 255], weight: 0.18 },  // pale blue
  { rgb: [0xb0 / 255, 0xcd / 255, 0xf0 / 255], weight: 0.10 },  // light blue
  { rgb: [0x8f / 255, 0xb4 / 255, 0xde / 255], weight: 0.02 },  // deep light-blue accent
]);

// ---------- chapter 2: high-entropy organic distribution ----------

/** Deterministic 32-bit PRNG (mulberry32). */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ----- 2D simplex noise (Gustavson-style, compact) -----

const GRAD2: ReadonlyArray<[number, number]> = [
  [1, 1], [-1, 1], [1, -1], [-1, -1],
  [1, 0], [-1, 0], [0, 1], [0, -1],
];

const F2 = 0.5 * (Math.sqrt(3) - 1);
const G2 = (3 - Math.sqrt(3)) / 6;

/** Seeded 2D simplex noise, output range ≈ [-1, 1]. */
export function makeSimplex2(seed: number): (x: number, y: number) => number {
  const rnd = mulberry32(seed);
  const perm = new Uint8Array(512);
  const p = new Uint8Array(256);
  for (let i = 0; i < 256; i++) p[i] = i;
  for (let i = 255; i > 0; i--) {
    const j = Math.floor(rnd() * (i + 1));
    const t = p[i]!;
    p[i] = p[j]!;
    p[j] = t;
  }
  for (let i = 0; i < 512; i++) perm[i] = p[i & 255]!;

  return (xin: number, yin: number): number => {
    const s = (xin + yin) * F2;
    const i = Math.floor(xin + s);
    const j = Math.floor(yin + s);
    const t = (i + j) * G2;
    const x0 = xin - (i - t);
    const y0 = yin - (j - t);
    const i1 = x0 > y0 ? 1 : 0;
    const j1 = x0 > y0 ? 0 : 1;
    const x1 = x0 - i1 + G2;
    const y1 = y0 - j1 + G2;
    const x2 = x0 - 1 + 2 * G2;
    const y2 = y0 - 1 + 2 * G2;
    const ii = i & 255;
    const jj = j & 255;
    let n = 0;
    let t0 = 0.5 - x0 * x0 - y0 * y0;
    if (t0 > 0) {
      const gi0 = perm[ii + perm[jj]!]! & 7;
      const g = GRAD2[gi0]!;
      t0 *= t0;
      n += t0 * t0 * (g[0] * x0 + g[1] * y0);
    }
    let t1 = 0.5 - x1 * x1 - y1 * y1;
    if (t1 > 0) {
      const gi1 = perm[ii + i1 + perm[jj + j1]!]! & 7;
      const g = GRAD2[gi1]!;
      t1 *= t1;
      n += t1 * t1 * (g[0] * x1 + g[1] * y1);
    }
    let t2 = 0.5 - x2 * x2 - y2 * y2;
    if (t2 > 0) {
      const gi2 = perm[ii + 1 + perm[jj + 1]!]! & 7;
      const g = GRAD2[gi2]!;
      t2 *= t2;
      n += t2 * t2 * (g[0] * x2 + g[1] * y2);
    }
    return 70 * n;
  };
}

/** Fractal simplex (3 octaves), output ≈ [-1, 1]. */
export function makeFbm2(seed: number): (x: number, y: number) => number {
  const n = makeSimplex2(seed);
  return (x: number, y: number): number => {
    let v = 0;
    let amp = 0.55;
    let f = 1;
    for (let o = 0; o < 3; o++) {
      v += amp * n(x * f, y * f);
      f *= 2.15;
      amp *= 0.5;
    }
    return v;
  };
}

export function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = Math.min(1, Math.max(0, (x - edge0) / (edge1 - edge0)));
  return t * t * (3 - 2 * t);
}

/**
 * Chapter 2.2 density mask N(x,y) normalized to 0..1:
 *  < 0.35 → void rift (probability 0), > 0.65 → star cluster (+200%).
 */
export function makeDensityMask(seed: number, scale = 0.0016): (x: number, y: number) => number {
  const fbm = makeFbm2(seed);
  return (x: number, y: number): number => (fbm(x * scale, y * scale) + 1) / 2;
}

/**
 * Milky-way river mask: a soft diagonal band across the sampled world whose
 * centerline wobbles sinusoidally. Combined with the simplex mask via max(),
 * it lays a winding river of dense stars across the voids (the anime night
 * sky's signature). Deterministic per seed.
 */
export function makeBandMask(
  seed: number,
  width: number,
  height: number,
  swap = false,
): (x: number, y: number) => number {
  const rnd = mulberry32(seed ^ 0x51ed270b);
  const tilt = 0.34 + rnd() * 0.30;          // vertical span fraction
  const wobbleAmp = (0.10 + rnd() * 0.08) * height;
  const wobbleFreq = (1.6 + rnd() * 1.2) * Math.PI / Math.max(1, width);
  const phase = rnd() * Math.PI * 2;
  const halfWidth = height * (0.085 + rnd() * 0.035);
  const cx = width / 2;
  const cy = height / 2;
  return (x: number, y: number): number => {
    if (swap) { const s = x; x = y; y = s; } // crossing river: run in swapped space
    const t = (x - cx) / Math.max(1, width);
    const center = cy + height * (0.5 - tilt) * Math.sin(t * Math.PI - phase * 0.5)
      + wobbleAmp * Math.sin(x * wobbleFreq + phase);
    const d = Math.abs(y - center);
    const g = Math.exp(-(d * d) / (2 * halfWidth * halfWidth));
    return g * 0.94;
  };
}

/** Compose: simplex rifts/clusters ∪ milky-way river (max). */
export function composeMask(
  base: (x: number, y: number) => number,
  band: (x: number, y: number) => number,
): (x: number, y: number) => number {
  return (x: number, y: number): number => Math.max(base(x, y), band(x, y));
}

/**
 * Full starfield density field: simplex rift/cluster base ∪ TWO crossing
 * galactic rivers (one tilted with the screen, one against it) — dense
 * luminous settlements winding through clean empty voids. Deterministic.
 */
export function makeStarfieldMask(seed: number, width: number, height: number): (x: number, y: number) => number {
  const base = makeDensityMask(seed);
  const bandA = makeBandMask(seed ^ 0x51ed270b, width, height);
  const bandB = makeBandMask(seed ^ 0x1b873c7f, height, width, true);
  return (x: number, y: number): number => Math.max(base(x, y), bandA(x, y), bandB(x, y));
}

export interface PoissonOptions {
  /** Sampled world rectangle (parallax margin included). */
  x0: number;
  y0: number;
  width: number;
  height: number;
  /** Hard physical minimum distance between any two stars (px, spec 2.1). */
  rMin: number;
  /** Upper bound used in sparse regions. */
  rMax: number;
  seed: number;
  mask?: (x: number, y: number) => number;
  /** Candidate rejections per active point (Bridson k). */
  attempts?: number;
  maxPoints?: number;
}

export interface PoissonPoint {
  x: number;
  y: number;
  /** Local spacing radius chosen for this star (cluster ⇒ small r). */
  r: number;
}

/**
 * Variable-radius Poisson-disk sampling (Bridson dart throwing on a uniform
 * background grid). In dense mask regions the local radius shrinks toward
 * rMin (clusters), in voids it grows toward rMax; candidates inside voids are
 * rejected outright, giving natural dark rifts without lattice artifacts.
 */
export function poissonDisk(opts: PoissonOptions): PoissonPoint[] {
  const { x0, y0, width, height, rMin, rMax, seed } = opts;
  const mask = opts.mask;
  const attempts = opts.attempts ?? 18;
  const maxPoints = opts.maxPoints ?? 8000;
  const rnd = mulberry32(seed);

  const cell = rMax / Math.SQRT2;
  const gw = Math.max(1, Math.ceil(width / cell));
  const gh = Math.max(1, Math.ceil(height / cell));
  // Spatial hash with per-cell linked lists — several stars may share one
  // cell (their pairwise distance rule is smaller than the cell diagonal),
  // so single-slot grids would silently drop neighbors and allow overlaps.
  const heads: Int32Array = new Int32Array(gw * gh).fill(-1);
  const next: number[] = [];
  const pts: PoissonPoint[] = [];
  const px: number[] = [];
  const py: number[] = [];
  const pr: number[] = [];

  const fits = (cx: number, cy: number, r: number): boolean => {
    const gx = Math.floor((cx - x0) / cell);
    const gy = Math.floor((cy - y0) / cell);
    for (let oy = -2; oy <= 2; oy++) {
      const yy = gy + oy;
      if (yy < 0 || yy >= gh) continue;
      for (let ox = -2; ox <= 2; ox++) {
        const xx = gx + ox;
        if (xx < 0 || xx >= gw) continue;
        for (let idx = heads[yy * gw + xx]!; idx !== -1; idx = next[idx]!) {
          const dx = cx - px[idx]!;
          const dy = cy - py[idx]!;
          if (dx * dx + dy * dy < ((r + pr[idx]!) * 0.5) ** 2) return false;
        }
      }
    }
    return true;
  };

  const tryPoint = (sx: number, sy: number): boolean => {
    const nx = mask ? mask(sx, sy) : 1;
    // Density zones: ~10% void rifts (no stars), ~60% sparse field, ~30%
    // soft "star river" settlements — smooth, never banded.
    if (nx < 0.28) return false; // dark void rift
    const dens = smoothstep(0.28, 0.62, nx);
    const r = rMax - (rMax - rMin) * dens;
    if (!fits(sx, sy, r)) return false;
    const idx = pts.length;
    pts.push({ x: sx, y: sy, r });
    px.push(sx);
    py.push(sy);
    pr.push(r);
    const cellIdx = Math.floor((sy - y0) / cell) * gw + Math.floor((sx - x0) / cell);
    next[idx] = heads[cellIdx]!;
    heads[cellIdx] = idx;
    return true;
  };

  // Active-list Bridson loop seeded from one accepted origin point.
  let guard = 0;
  let origin: PoissonPoint | null = null;
  while (!origin && guard++ < 500) {
    const sx = x0 + rnd() * width;
    const sy = y0 + rnd() * height;
    if (tryPoint(sx, sy)) origin = pts[pts.length - 1]!;
  }
  if (!origin) return pts;
  const active: number[] = [0];
  while (active.length > 0 && pts.length < maxPoints) {
    const ai = Math.floor(rnd() * active.length)!;
    const base = active[ai]!;
    let placed = false;
    for (let k = 0; k < attempts; k++) {
      const ang = rnd() * Math.PI * 2;
      const rad = pr[base]! * (1 + rnd());
      const sx = px[base]! + Math.cos(ang) * rad;
      const sy = py[base]! + Math.sin(ang) * rad;
      if (sx < x0 || sx >= x0 + width || sy < y0 || sy >= y0 + height) continue;
      if (tryPoint(sx, sy)) {
        active.push(pts.length - 1);
        placed = true;
        break;
      }
    }
    if (!placed) active.splice(ai, 1);
  }
  return pts;
}

// ---------- star generation (chapters 1.3 + 2.3) ----------

export type StarClass = 0 | 1 | 2; // far dust / mid main / near flare

export interface GenerateOptions {
  /** Visible design area (CSS px) the camera may travel across. */
  viewWidth: number;
  viewHeight: number;
  /** Extra world margin around the viewport for parallax travel (px). */
  margin?: number;
  seed: number;
  mask?: (x: number, y: number) => number;
  /** Approximate star budget before void rejection. */
  targetCount?: number;
}

/**
 * Packed star buffer, exactly the spec's 24-byte-per-star layout:
 *   [0..1] vec2 world position · [2] size px · [3] depth Z 0.1..1.0
 *   [4] twinkle phase [0,2π) · [5] frequency [0.5,2.0] Hz
 * Class distribution: 75% far dust (0.5-1.0px), 20% mid (1.2-1.8px),
 * 5% near flare stars (2.0-3.2px). Near stars get a larger personal-space
 * radius so flares never overlap into blobs.
 */
export function generateStars(opts: GenerateOptions): { data: Float32Array; count: number } {
  const margin = opts.margin ?? 320;
  const w = opts.viewWidth + margin * 2;
  const h = opts.viewHeight + margin * 2;
  const requested = opts.targetCount ?? 2100;
  // Poisson disk handles the structured foreground (min-distance guarantee);
  // budgets above the cap overflow into uniform sub-pixel DUST — overlapping
  // is invisible at that size and generation stays O(1) per star, so the
  // L9/L10 hundred-thousand-star ladders stay cheap.
  const poissonCap = 30000;
  const target = Math.min(requested, poissonCap);
  // Solve rMin so the poisson pass lands near the requested budget:
  // packing density ≈ 0.7 → rMin ≈ sqrt(0.7·area/target).
  const area = w * h;
  // Hard 8px minimum spacing — two stars can never merge into an ugly blob.
  const rMin = Math.max(8, Math.sqrt((area * 0.55) / Math.max(64, target)));
  const pts = poissonDisk({
    x0: -margin,
    y0: -margin,
    width: w,
    height: h,
    rMin,
    rMax: rMin * 2.4,
    seed: opts.seed,
    mask: opts.mask,
  });
  const rnd = mulberry32(opts.seed ^ 0x9e3779b9);
  const dustCount = Math.max(0, requested - pts.length);
  const data = new Float32Array((pts.length + dustCount) * 6);
  let n = 0;
  for (const p of pts) {
    const cr = rnd();
    // Spec: ~78% ultra-faint dust filling the void, ~18% main-sequence
    // stars, ~4% near flare stars that carry the visible bright dots.
    let cls: StarClass = cr < 0.78 ? 0 : cr < 0.96 ? 1 : 2;
    // Soft clusters (N > 0.62) gently shift dust toward mid stars — never a
    // hard-edged blob, so the clustering reads as a faint density tide.
    if (opts.mask && opts.mask(p.x, p.y) > 0.62) {
      if (cls === 0 && rnd() < 0.10) cls = 1;
    }
    let size: number;
    let depth: number;
    if (cls === 0) {
      // Far dust: 0.6-1.0 px faint specks filling the deep void.
      size = 0.6 + Math.pow(rnd(), 2.6) * 0.4;
      depth = 0.1 + rnd() * 0.35;
    } else if (cls === 1) {
      // Mid main sequence: crisp 1.15-1.8 px dots.
      size = 1.15 + Math.pow(rnd(), 2.2) * 0.65;
      depth = 0.45 + rnd() * 0.35;
    } else {
      // Near feature stars: 2.3-3.2 px, the only flare carriers.
      size = 2.3 + Math.pow(rnd(), 2.0) * 0.9;
      depth = 0.8 + rnd() * 0.2;
    }
    // Personal space scales with visual footprint (flare stars stay apart).
    if (size > 1.9 && !fitsClass(data, n, p.x, p.y, 26)) continue;
    const o = n * 6;
    data[o] = p.x;
    data[o + 1] = p.y;
    data[o + 2] = size;
    data[o + 3] = depth;
    data[o + 4] = rnd() * Math.PI * 2;
    data[o + 5] = 0.5 + rnd() * 1.5;
    n++;
  }
  // Sub-pixel cosmic dust: uniform scatter, no min-distance needed.
  for (let i = 0; i < dustCount; i++) {
    const o = n * 6;
    data[o] = -margin + rnd() * w;
    data[o + 1] = -margin + rnd() * h;
    data[o + 2] = 0.35 + Math.pow(rnd(), 2.0) * 0.5;
    data[o + 3] = 0.12 + rnd() * 0.26;
    data[o + 4] = rnd() * Math.PI * 2;
    data[o + 5] = 0.5 + rnd() * 1.5;
    n++;
  }
  return { data: data.subarray(0, n * 6), count: n };
}

/** Linear scan kept O(n²)-free by early exit; near stars are rare (≤5%). */
function fitsClass(data: Float32Array, n: number, x: number, y: number, minDist: number): boolean {
  const md2 = minDist * minDist;
  for (let i = 0; i < n; i++) {
    if (data[i * 6 + 2]! <= 1.9) continue;
    const dx = x - data[i * 6]!;
    const dy = y - data[i * 6 + 1]!;
    if (dx * dx + dy * dy < md2) return false;
  }
  return true;
}

// ---------- chapter 5: ten-tier performance matrix ----------

export interface TierSpec {
  level: number;
  fpsTarget: number;
  dprScale: number;
  /** Total star budget (poisson foreground + sub-pixel dust) for the tier. */
  starBudget: number;
  /** Feature gates consumed by the shaders/GPU pipeline. */
  globalPulse: boolean;
  twoLayerDepth: boolean;
  squarePoints: boolean;
  independentTwinkle: boolean;
  screentone: boolean;
  flare: boolean;
  bokehDispersion: boolean;
  /** L7+ screen-space soft-focus bloom (bright-pass → blur → composite). */
  bloom: boolean;
  /** L8+ FBM-driven flowing ribbons. */
  auroraFlow: boolean;
  /** L10 ray-marched ribbon detail (disabled while flinging the canvas). */
  detailMaster: boolean;
  /** Aurora ladder: 0 none · 1 basic · 2 dual+glow · 3 triple silk. */
  aurora: number;
  /** Continuous rendering; false = draw once per viewport change (L1). */
  continuous: boolean;
}

const T = (level: number, fpsTarget: number, dprScale: number, feats: Partial<TierSpec>): TierSpec => ({
  level,
  fpsTarget,
  dprScale,
  starBudget: 2400,
  globalPulse: false,
  twoLayerDepth: false,
  squarePoints: false,
  independentTwinkle: false,
  screentone: false,
  flare: false,
  bokehDispersion: false,
  bloom: false,
  auroraFlow: false,
  detailMaster: false,
  aurora: 0,
  continuous: true,
  ...feats,
});

/** Level 1 → Level 10, index 0-based externally via TIER_SPECS[level-1]. */
export const TIER_SPECS: readonly TierSpec[] = Object.freeze([
  T(1, 30, 0.6, { continuous: false, starBudget: 200 }),
  T(2, 60, 0.66, { globalPulse: true, starBudget: 500 }),
  T(3, 75, 0.75, { twoLayerDepth: true, squarePoints: true, aurora: 1, starBudget: 1000 }),
  T(4, 80, 0.85, { twoLayerDepth: true, squarePoints: true, independentTwinkle: true, aurora: 1, starBudget: 2500 }),
  T(5, 90, 0.9, { twoLayerDepth: true, independentTwinkle: true, screentone: true, aurora: 2, starBudget: 5000 }),
  T(6, 100, 1.0, { independentTwinkle: true, screentone: true, flare: true, aurora: 2, starBudget: 10000 }),
  T(7, 120, 1.0, {
    independentTwinkle: true, screentone: true, flare: true, bokehDispersion: true,
    bloom: true, aurora: 2, starBudget: 25000,
  }),
  T(8, 130, 1.0, {
    independentTwinkle: true, screentone: true, flare: true, bokehDispersion: true,
    bloom: true, auroraFlow: true, aurora: 3, starBudget: 50000,
  }),
  T(9, 144, 1.0, {
    independentTwinkle: true, screentone: true, flare: true, bokehDispersion: true,
    bloom: true, auroraFlow: true, aurora: 3, starBudget: 100000,
  }),
  T(10, 144, 1.0, {
    independentTwinkle: true, screentone: true, flare: true, bokehDispersion: true,
    bloom: true, auroraFlow: true, detailMaster: true, aurora: 3,
    starBudget: 200000,
  }),
]);
