import type { NodeShape, PathStyle } from "../../lib/types";

export interface Pt {
  x: number;
  y: number;
}

export const SHAPES: NodeShape[] = [
  "rect", "rounded", "circle", "triangle", "diamond", "pentagon", "hexagon", "heptagon",
];

export const SHAPE_SIDES: Record<NodeShape, number> = {
  rect: 4,
  rounded: 4,
  circle: 8,
  triangle: 3,
  diamond: 4,
  pentagon: 5,
  hexagon: 6,
  heptagon: 7,
};

/** Enforce a true circle: width === height === diameter. */
export function circleDiameter(w: number, h: number): number {
  return Math.min(w, h);
}

// ---------- text containment geometry (module-2) ----------

/**
 * Area-weighted polygon centroid (true geometric center of mass). For regular
 * polygons and the diamond this is the box center; for the triangle it sits at
 * (w/2, 2h/3) — the visually correct anchor for apex-top triangles.
 */
export function centroidOf(shape: NodeShape, w: number, h: number): Pt {
  if (shape === "rect" || shape === "rounded" || shape === "circle") return { x: w / 2, y: h / 2 };
  const pts = shapePoints(shape, w, h);
  let area2 = 0;
  let cx = 0;
  let cy = 0;
  for (let i = 0; i < pts.length; i++) {
    const p = pts[i]!;
    const q = pts[(i + 1) % pts.length]!;
    const cross = p.x * q.y - q.x * p.y;
    area2 += cross;
    cx += (p.x + q.x) * cross;
    cy += (p.y + q.y) * cross;
  }
  if (Math.abs(area2) < 1e-9) return { x: w / 2, y: h / 2 };
  return { x: cx / (3 * area2), y: cy / (3 * area2) };
}

export interface InscRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

const INSCRIBED_CACHE = new Map<string, InscRect>();

/**
 * Largest axis-aligned rectangle CENTERED AT THE CENTROID whose four corners
 * lie strictly inside the shape. Box shapes keep the full box; circles get the
 * inscribed square; polygons are solved numerically (nested binary search:
 * outer ternary search on half-height maximizing area, inner search on the
 * feasible half-width). Memoized per shape+size — drag-resize cost is nil.
 */
export function inscribedRect(shape: NodeShape, w: number, h: number): InscRect {
  if (shape === "rect" || shape === "rounded") return { x: 0, y: 0, w, h };
  const key = `${shape}|${Math.round(w)}x${Math.round(h)}`;
  const cached = INSCRIBED_CACHE.get(key);
  if (cached) return cached;

  let out: InscRect;
  if (shape === "circle") {
    const r = Math.min(w, h) / 2;
    const side = r * Math.SQRT2 * 0.94;
    out = { x: w / 2 - side / 2, y: h / 2 - side / 2, w: side, h: side };
  } else {
    const c = centroidOf(shape, w, h);
    const cornersInside = (a: number, b: number): boolean =>
      pointInShape(shape, w, h, c.x + a, c.y - b) &&
      pointInShape(shape, w, h, c.x + a, c.y + b) &&
      pointInShape(shape, w, h, c.x - a, c.y - b) &&
      pointInShape(shape, w, h, c.x - a, c.y + b);
    const maxHalfW = (b: number): number => {
      let lo = 0;
      let hi = w;
      for (let i = 0; i < 18; i++) {
        const a = (lo + hi) / 2;
        if (cornersInside(a, b)) lo = a;
        else hi = a;
      }
      return lo;
    };
    let lo = h * 1e-3;
    // Half-height is bounded by BOTH box halves around the centroid.
    let hi = Math.max(lo, Math.min(h / 2, c.y, h - c.y));
    for (let i = 0; i < 42; i++) {
      const m1 = lo + (hi - lo) / 3;
      const m2 = hi - (hi - lo) / 3;
      if (maxHalfW(m1) * m1 < maxHalfW(m2) * m2) lo = m1;
      else hi = m2;
    }
    const b = Math.max(h * 1e-3, Math.min((lo + hi) / 2, h - c.y, c.y));
    const a = maxHalfW(b);
    out = { x: c.x - a, y: c.y - b, w: 2 * a, h: 2 * b };
  }

  if (INSCRIBED_CACHE.size > 800) INSCRIBED_CACHE.clear();
  INSCRIBED_CACHE.set(key, out);
  return out;
}

// ---------- dimension guards (module-1/2/3/4 rev 3) ----------

/** Hard minimum frame size — no polygon may ever be squeezed below this. */
export const MIN_NODE_W = 120;
export const MIN_NODE_H = 80;
/** Extreme-elongation guard: either axis may never exceed 3× the other. */
export const MAX_ASPECT = 3;
/** Safety padding the drawn edge keeps beyond the text box (module-3). */
export const TEXT_PAD = 16;
/** Text-first typography: default preferred wrap width in px (module-2). */
export const PREFERRED_TEXT_W = 280;

export interface Dim {
  width: number;
  height: number;
}

/**
 * Free-form writing frames: rect/rounded boxes are plain text containers and
 * may take ANY aspect ratio — a tall narrow writing column is a legitimate
 * layout, so the MAX_ASPECT elongation guard never applies to them. The guard
 * exists to keep polygons/circles from degenerating into needles or slivers.
 */
export function boxShapeExempt(shape: NodeShape | undefined): boolean {
  return shape === "rect" || shape === "rounded";
}

/**
 * Hard dimension clamps applied on EVERY resize path (handles, vertex drags,
 * imports, autogrow): absolute minimums first, then the aspect-ratio guard —
 * when one axis exceeds MAX_ASPECT × the other, the LONG side is pulled back
 * to exactly the limit (equivalent to force-expanding the short side), so a
 * polygon can never be stretched into a needle or a line. Box frames
 * (rect/rounded) are exempt via `shape` — their text column may grow
 * arbitrarily tall without the frame silently clamping and clipping the
 * content (the "text overflows the border" regression).
 * Non-finite input falls back to the matching minimum (NaN/∞ poison guard).
 */
export function clampDims(width: number, height: number, shape?: NodeShape): Dim {
  let w = Number.isFinite(width) ? width : MIN_NODE_W;
  let h = Number.isFinite(height) ? height : MIN_NODE_H;
  w = Math.max(MIN_NODE_W, Math.min(1400, w));
  h = Math.max(MIN_NODE_H, Math.min(20000, h));
  if (!boxShapeExempt(shape)) {
    if (w > h * MAX_ASPECT) w = Math.round(h * MAX_ASPECT);
    if (h > w * MAX_ASPECT) h = Math.round(w * MAX_ASPECT);
  }
  return { width: Math.round(w), height: Math.round(h) };
}

/** Shoelace area of a polygon (absolute value). */
export function polygonArea(pts: Pt[]): number {
  let a = 0;
  for (let i = 0; i < pts.length; i++) {
    const p = pts[i]!;
    const q = pts[(i + 1) % pts.length]!;
    a += p.x * q.y - q.x * p.y;
  }
  return Math.abs(a / 2);
}

/**
 * Module-1 free-vertex safety lock: true when the shape has degenerated into
 * a sliver — below the hard minimums, area under ~1% of its bounding box, or
 * any two adjacent vertices closer than 6px. Callers reject the pending
 * resize (collision damping keeps the previous dims).
 */
export function shapeCollapsed(shape: NodeShape, w: number, h: number): boolean {
  if (!Number.isFinite(w) || !Number.isFinite(h)) return true;
  if (w < MIN_NODE_W || h < MIN_NODE_H) return true;
  const pts = shapePoints(shape, w, h);
  if (polygonArea(pts) < Math.max(1600, 0.01 * w * h)) return true;
  for (let i = 0; i < pts.length; i++) {
    const p = pts[i]!;
    const q = pts[(i + 1) % pts.length]!;
    if (Math.hypot(p.x - q.x, p.y - q.y) < 6) return true;
  }
  return false;
}

/**
 * Module-4 sanity normalization run before render AND before persist:
 * non-finite or undersized dims are repaired to the standard ratio and
 * flagged so callers log/heal exactly once. clampDims also silently repairs
 * aspect violations on load.
 */
export function sanitizeDims(
  width: number,
  height: number,
  shape?: NodeShape,
): { dim: Dim; repaired: boolean } {
  const bad =
    !Number.isFinite(width) || !Number.isFinite(height) ||
    width < MIN_NODE_W || height < MIN_NODE_H;
  const dim = clampDims(
    Number.isFinite(width) ? width : 230,
    Number.isFinite(height) ? height : 88,
    shape,
  );
  return { dim, repaired: bad };
}

/**
 * Module-3 reverse dilation: smallest node size whose centroid-centered
 * inscribed rectangle contains textW × textH plus a TEXT_PAD margin on every
 * side. The node grows proportionally around its current size and iterates on
 * the cached inscribed solve until the text fits (or caps are hit, where the
 * content falls back to internal scrolling).
 */
export function growDimsForText(
  shape: NodeShape,
  width: number,
  height: number,
  textW: number,
  textH: number,
): Dim {
  let cur = clampDims(width, height, shape);
  const needW = Math.max(0, Number.isFinite(textW) ? textW : 0) + TEXT_PAD * 2;
  const needH = Math.max(0, Number.isFinite(textH) ? textH : 0) + TEXT_PAD * 2;
  for (let i = 0; i < 12; i++) {
    const insc = inscribedRect(shape, cur.width, cur.height);
    const dw = needW - insc.w;
    const dh = needH - insc.h;
    if (dw <= 0 && dh <= 0) break;
    const kw = dw > 0 && insc.w > 1 ? dw / insc.w : 0;
    const kh = dh > 0 && insc.h > 1 ? dh / insc.h : 0;
    const k = Math.max(kw, kh) + 0.02; // small overshoot → converge faster
    cur = clampDims(cur.width * (1 + k), cur.height * (1 + k), shape);
  }
  return cur;
}

export interface VertexDragSigns {
  /** -1 = west side, 0 = centered, +1 = east side (relative to box center). */
  sx: -1 | 0 | 1;
  /** -1 = north side, 0 = centered, +1 = south side. */
  sy: -1 | 0 | 1;
}
/**
 * Axis signs of polygon vertex `index` for a shape fitted into (w, h).
 * Dragging that vertex maps onto the matching edge-handle behavior: e.g. the
 * triangle apex has {sx:0, sy:-1} → pure north-style height resize.
 */
export function vertexDragSigns(shape: NodeShape, w: number, h: number, index: number): VertexDragSigns {
  const pts = shapePoints(shape, w, h);
  const p = pts[index] ?? pts[0] ?? { x: w / 2, y: h / 2 };
  const dx = p.x - w / 2;
  const dy = p.y - h / 2;
  return {
    sx: Math.abs(dx) < 1e-6 ? 0 : dx > 0 ? 1 : -1,
    sy: Math.abs(dy) < 1e-6 ? 0 : dy > 0 ? 1 : -1,
  };
}

/** Polygon vertices for a shape fitted into box (0,0,w,h), first vertex at top. */
export function shapePoints(shape: NodeShape, w: number, h: number): Pt[] {
  const cx = w / 2;
  const cy = h / 2;
  switch (shape) {
    case "circle": {
      // Regular octagon approximation is NOT used; callers render circles via
      // border-radius. Points describe the circumference anchor ring.
      return anchorsFor(shape, w, h).map((a) => a.p);
    }
    case "triangle":
      return [
        { x: cx, y: 0 },
        { x: w, y: h },
        { x: 0, y: h },
      ];
    case "diamond":
      return [
        { x: cx, y: 0 },
        { x: w, y: cy },
        { x: cx, y: h },
        { x: 0, y: cy },
      ];
    case "pentagon":
    case "hexagon":
    case "heptagon": {
      const n = SHAPE_SIDES[shape];
      const rx = (w / 2) * 0.98;
      const ry = (h / 2) * 0.98;
      const pts: Pt[] = [];
      // Circumradius fit: use min(rx, ry) so polygons stay regular-ish inside box.
      const r = Math.min(rx, ry);
      const scx = cx;
      const scy = cy;
      for (let i = 0; i < n; i++) {
        const angle = -Math.PI / 2 + (i * 2 * Math.PI) / n;
        pts.push({ x: scx + r * Math.cos(angle), y: scy + r * Math.sin(angle) });
      }
      return pts;
    }
    default:
      return [
        { x: 0, y: 0 }, { x: w, y: 0 }, { x: w, y: h }, { x: 0, y: h },
      ];
  }
}

export interface Anchor {
  p: Pt;
  /** Index of the edge this anchor belongs to (vertex pair index). */
  edgeIndex: number;
}

/**
 * Anchor points sit at the MIDDLE of each edge — between corners, never on
 * them — so multiple connections never pile up on one corner.
 */
export function anchorsFor(shape: NodeShape, w: number, h: number): Anchor[] {
  if (shape === "circle") {
    const r = Math.min(w, h) / 2;
    const cx = w / 2;
    const cy = h / 2;
    const out: Anchor[] = [];
    for (let i = 0; i < 8; i++) {
      const a = (i * Math.PI) / 4;
      out.push({ p: { x: cx + r * Math.cos(a), y: cy + r * Math.sin(a) }, edgeIndex: i });
    }
    return out;
  }
  const verts = shapePoints(shape, w, h);
  const out: Anchor[] = [];
  for (let i = 0; i < verts.length; i++) {
    const a = verts[i]!;
    const b = verts[(i + 1) % verts.length]!;
    out.push({ p: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }, edgeIndex: i });
  }
  return out;
}

/** Nearest edge-midpoint anchor toward an outside point. */
export function nearestAnchor(shape: NodeShape, w: number, h: number, from: Pt): Anchor {
  const list = anchorsFor(shape, w, h);
  let best = list[0]!;
  let bestD = Infinity;
  for (const a of list) {
    const dx = a.p.x - from.x;
    const dy = a.p.y - from.y;
    const d = dx * dx + dy * dy;
    if (d < bestD) {
      bestD = d;
      best = a;
    }
  }
  return best;
}

/**
 * When several edges share one anchor edge, spread their attach points along
 * that edge so lines do not overlap.
 */
export function distributedAnchor(
  node: { width: number; height: number; shape: NodeShape },
  from: Pt,
  usageIndex: number,
  usageCount: number,
): Pt {
  const anchor = nearestAnchor(node.shape, node.width, node.height, from);
  if (node.shape === "circle" || usageCount <= 1) return anchor.p;
  const verts = shapePoints(node.shape, node.width, node.height);
  const a = verts[anchor.edgeIndex % verts.length]!;
  const b = verts[(anchor.edgeIndex + 1) % verts.length]!;
  // Keep away from the corners: usable span is the middle 60% of the edge.
  const t = 0.2 + (0.6 * (usageIndex + 1)) / (usageCount + 1);
  return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
}

// ---------- exact boundary anchors (module-6: edges hug the drawn shape) ----------

/**
 * Intersection of the ray (center → local direction) with the SHAPE boundary
 * itself — never the bounding box. For box shapes the boundary is the rect
 * outline; for polygons the true edges; for circles the circumference.
 * Multiple connections spread along the hit edge via the usage band, matching
 * the historical distribution semantics.
 */
export function boundaryAnchor(
  shape: NodeShape,
  w: number,
  h: number,
  dir: Pt,
  usage?: { index: number; count: number },
): Pt {
  const cx = w / 2;
  const cy = h / 2;
  const len = Math.hypot(dir.x, dir.y);
  if (!Number.isFinite(len) || len < 1e-9) {
    return nearestAnchor(shape, w, h, { x: cx + (dir.x || 1), y: cy + (dir.y || 0) }).p;
  }
  const dx = dir.x / len;
  const dy = dir.y / len;

  if (shape === "circle") {
    const r = Math.min(w, h) / 2 - 1;
    return { x: cx + dx * r, y: cy + dy * r };
  }

  // Box shapes fall through shapePoints' default branch → the rect outline.
  const verts = shapePoints(shape, w, h);
  let bestT = Infinity;
  let bestS = 0.5;
  let bestI = 0;
  for (let i = 0; i < verts.length; i++) {
    const A = verts[i]!;
    const B = verts[(i + 1) % verts.length]!;
    const ex = B.x - A.x;
    const ey = B.y - A.y;
    const den = dx * ey - dy * ex;
    if (Math.abs(den) < 1e-9) continue;
    const rx = A.x - cx;
    const ry = A.y - cy;
    const t = (rx * ey - ry * ex) / den;
    const s = (rx * dy - ry * dx) / den;
    if (t > 1e-6 && s >= -1e-6 && s <= 1 + 1e-6 && t < bestT) {
      bestT = t;
      bestS = s;
      bestI = i;
    }
  }
  if (!Number.isFinite(bestT)) {
    return nearestAnchor(shape, w, h, { x: cx + dx, y: cy + dy }).p;
  }
  const A = verts[bestI]!;
  const B = verts[(bestI + 1) % verts.length]!;
  // Single connection: keep the exact hit point (kept off the corners).
  // Multiple connections: redistribute across the hit edge's middle band.
  const s = usage && usage.count > 1
    ? 0.2 + (0.6 * (usage.index + 1)) / (usage.count + 1)
    : Math.min(0.85, Math.max(0.15, bestS));
  return { x: A.x + (B.x - A.x) * s, y: A.y + (B.y - A.y) * s };
}

/** Minimal structural node info needed for edge geometry. */
export interface EdgeNodePart {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  shape: NodeShape;
  rotation?: number;
}

/** Rotate point p around center c by deg degrees. */
export function rotatePt(p: Pt, c: Pt, deg: number): Pt {
  if (!deg) return p;
  const a = (deg * Math.PI) / 180;
  const cos = Math.cos(a);
  const sin = Math.sin(a);
  const dx = p.x - c.x;
  const dy = p.y - c.y;
  return { x: c.x + dx * cos - dy * sin, y: c.y + dx * sin + dy * cos };
}

export interface EdgeGeomInput {
  from: EdgeNodePart;
  to: EdgeNodePart;
  pathStyle: PathStyle;
  fromUsage?: { index: number; count: number };
  toUsage?: { index: number; count: number };
}

/**
 * Compute world-space endpoints and svg path for one edge (rotation-aware).
 * Endpoints are EXACT ray/shape intersections in each node's local space
 * (inverse-rotate the direction, intersect, rotate the hit back), so lines
 * always land on the drawn polygon/circle outline instead of an AABB.
 */
export function computeEdge(input: EdgeGeomInput): { a: Pt; b: Pt; d: string } {
  const fu = input.fromUsage ?? { index: 0, count: 1 };
  const tu = input.toUsage ?? { index: 0, count: 1 };
  const fc = { x: input.from.width / 2, y: input.from.height / 2 };
  const tc = { x: input.to.width / 2, y: input.to.height / 2 };
  const gcx = input.from.x + fc.x;
  const gcy = input.from.y + fc.y;
  const gtx = input.to.x + tc.x;
  const gty = input.to.y + tc.y;
  const dirX = gtx - gcx;
  const dirY = gty - gcy;
  const fromDir = rotatePt({ x: dirX, y: dirY }, { x: 0, y: 0 }, -(input.from.rotation ?? 0));
  const toDir = rotatePt({ x: -dirX, y: -dirY }, { x: 0, y: 0 }, -(input.to.rotation ?? 0));
  const aLocal = boundaryAnchor(input.from.shape, input.from.width, input.from.height, fromDir, fu);
  const bLocal = boundaryAnchor(input.to.shape, input.to.width, input.to.height, toDir, tu);
  const aRot = rotatePt(aLocal, fc, input.from.rotation ?? 0);
  const bRot = rotatePt(bLocal, tc, input.to.rotation ?? 0);
  const a = { x: input.from.x + aRot.x, y: input.from.y + aRot.y };
  const b = { x: input.to.x + bRot.x, y: input.to.y + bRot.y };
  return { a, b, d: pathD(a, b, input.pathStyle) };
}

// ---------- alignment guides ----------

export interface GuideLine {
  axis: "v" | "h";
  /** World coordinate of the guide line on the perpendicular axis. */
  at: number;
  /** Gap in px between the two nearest facing edges, when meaningful. */
  gap?: number;
  /** World span for drawing the line. */
  from: number;
  to: number;
}

export interface Box {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

interface EdgeVals {
  left: number; centerX: number; right: number;
  top: number; centerY: number; bottom: number;
}

function valsOf(b: Box): EdgeVals {
  return {
    left: b.x, centerX: b.x + b.width / 2, right: b.x + b.width,
    top: b.y, centerY: b.y + b.height / 2, bottom: b.y + b.height,
  };
}

/**
 * Detect vertical/horizontal alignment relations between the moving box and
 * static boxes. Returns at most one strongest guide per axis with the gap
 * annotation between the closest facing edges.
 */
export function computeGuides(moving: Box, others: Box[], threshold: number): GuideLine[] {
  const m = valsOf(moving);
  let bestV: GuideLine | null = null;
  let bestVS = -1;
  let bestH: GuideLine | null = null;
  let bestHS = -1;

  for (const o of others) {
    if (o.id === moving.id) continue;
    const t = valsOf(o);
    const vxPairs: [number, number][] = [
      [m.left, t.left], [m.left, t.centerX], [m.left, t.right],
      [m.centerX, t.left], [m.centerX, t.centerX], [m.centerX, t.right],
      [m.right, t.left], [m.right, t.centerX], [m.right, t.right],
    ];
    for (const [mv, tv] of vxPairs) {
      const d = Math.abs(mv - tv);
      if (d > threshold) continue;
      const s = threshold - d;
      if (s > bestVS) {
        bestVS = s;
        const gapRaw = Math.min(Math.abs(m.left - t.right), Math.abs(t.left - m.right));
        bestV = {
          axis: "v",
          at: tv,
          from: Math.min(moving.y, o.y),
          to: Math.max(moving.y + moving.height, o.y + o.height),
          gap: gapRaw > 0.5 ? Math.round(gapRaw) : undefined,
        };
      }
    }
    const hyPairs: [number, number][] = [
      [m.top, t.top], [m.top, t.centerY], [m.top, t.bottom],
      [m.centerY, t.top], [m.centerY, t.centerY], [m.centerY, t.bottom],
      [m.bottom, t.top], [m.bottom, t.centerY], [m.bottom, t.bottom],
    ];
    for (const [mv, tv] of hyPairs) {
      const d = Math.abs(mv - tv);
      if (d > threshold) continue;
      const s = threshold - d;
      if (s > bestHS) {
        bestHS = s;
        const gapRaw = Math.min(Math.abs(m.top - t.bottom), Math.abs(t.top - m.bottom));
        bestH = {
          axis: "h",
          at: tv,
          from: Math.min(moving.x, o.x),
          to: Math.max(moving.x + moving.width, o.x + o.width),
          gap: gapRaw > 0.5 ? Math.round(gapRaw) : undefined,
        };
      }
    }
  }
  const out: GuideLine[] = [];
  if (bestV) out.push(bestV);
  if (bestH) out.push(bestH);
  return out;
}

/** Viewport-world AABB intersection used for render culling. */
export function boxIntersectsRect(
  b: { x: number; y: number; width: number; height: number },
  r: { x0: number; y0: number; x1: number; y1: number },
): boolean {
  return b.x < r.x1 && b.x + b.width > r.x0 && b.y < r.y1 && b.y + b.height > r.y0;
}

export function pathD(a: Pt, b: Pt, style: PathStyle): string {
  if (style === "straight") {
    return `M ${a.x} ${a.y} L ${b.x} ${b.y}`;
  }
  if (style === "ortho") {
    const dx = Math.abs(b.x - a.x);
    const dy = Math.abs(b.y - a.y);
    const r = Math.min(14, dx / 2, dy / 2);
    if (dx > dy) {
      const mx = (a.x + b.x) / 2;
      return `M ${a.x} ${a.y} L ${mx - Math.sign(b.x - a.x || 1) * r} ${a.y} Q ${mx} ${a.y} ${mx} ${a.y + Math.sign(b.y - a.y || 1) * r} L ${mx} ${b.y - Math.sign(b.y - a.y || 1) * r} Q ${mx} ${b.y} ${mx + Math.sign(b.x - a.x || 1) * r} ${b.y} L ${b.x} ${b.y}`;
    }
    const my = (a.y + b.y) / 2;
    return `M ${a.x} ${a.y} L ${a.x} ${my - Math.sign(b.y - a.y || 1) * r} Q ${a.x} ${my} ${a.x + Math.sign(b.x - a.x || 1) * r} ${my} L ${b.x - Math.sign(b.x - a.x || 1) * r} ${my} Q ${b.x} ${my} ${b.x} ${my + Math.sign(b.y - a.y || 1) * r} L ${b.x} ${b.y}`;
  }
  // curve: horizontal-tangent cubic
  const dist = Math.max(40, Math.abs(b.x - a.x) * 0.45);
  return `M ${a.x} ${a.y} C ${a.x + dist} ${a.y}, ${b.x - dist} ${b.y}, ${b.x} ${b.y}`;
}

/** Point-in-shape hit test in local coordinates. */
export function pointInShape(shape: NodeShape, w: number, h: number, x: number, y: number): boolean {
  if (x < 0 || y < 0 || x > w || y > h) return false;
  if (shape === "rect" || shape === "rounded") return true;
  if (shape === "circle") {
    const r = Math.min(w, h) / 2;
    const dx = x - w / 2;
    const dy = y - h / 2;
    return dx * dx + dy * dy <= r * r;
  }
  const poly = shapePoints(shape, w, h);
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const pi = poly[i]!;
    const pj = poly[j]!;
    const xi = pi.x;
    const yi = pi.y;
    const xj = pj.x;
    const yj = pj.y;
    const intersect = yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi + Number.EPSILON) + xi;
    if (intersect) inside = !inside;
  }
  return inside;
}
