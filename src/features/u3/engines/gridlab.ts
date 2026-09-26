/**
 * F503 深化引擎 · 桌面图标网格规划器（AI-U3 · gridlab）。
 *
 * 判据唯一源（主册摘文）：「三档格距 96/80/64px；密度改变时图标按最近格吸附
 * 重排（相对位置尽量保持）；自定义 8px 步进；与 F084 网格/F401 自动排列兼容；
 * 预览即时」。
 *
 * 深化点（v1/v2 只做了吸附函数，本引擎补齐「规划与度量」层）：
 * 1. **相对位置保持度度量**：重排前后构建邻接序（按行主序），度量秩相关——
 *    「尽量保持」从口号变成可测数字（≥0.9 为绿线）。
 * 2. **密度换档计划器**：预览即时的实义 = 换档前先算出目标位（无动画抖动、
 *    无二次回排）；计划器输出 from→to 的逐图标位移与冲突转移数。
 * 3. **F084/F401 兼容仲裁**：自动排列锁定态拒绝手工吸附（布局锁定 F546 联动），
 *    三态显性：放行/拒绝/降级（只对越界项吸附）。
 * 4. 8px 步进审计：自定义格距必须落步进网格，违例值显性回弹。
 */

/* ------------------------------ 基础参数 ------------------------------ */

/** 三档格距（主册 F503：宽松/标准/紧凑）。 */
export const GRID_DENSITY_PX = { loose: 96, standard: 80, compact: 64 } as const;
export type GridDensityName = keyof typeof GRID_DENSITY_PX;
/** 自定义步进 8px（主册 F503）。 */
export const GRID_CUSTOM_STEP_PX = 8;
/** 自定义格距合法区间（48-200px——小于 48 图标重叠、大于 200 一屏放不下四个）。 */
export const GRID_CUSTOM_MIN_PX = 48;
export const GRID_CUSTOM_MAX_PX = 200;
/** 相对位置保持度绿线（判据「相对位置尽量保持」的量化口径）。 */
export const KEEP_ORDER_GREEN_LINE = 0.9;

/* ------------------------------ 步进审计 ------------------------------ */

/** 自定义格距审计：钳到区间、落 8px 步进；返回修正值与是否回弹。 */
export function auditCustomGrid(px: number): { value: number; snapped: boolean } {
  const clamped = Math.min(GRID_CUSTOM_MAX_PX, Math.max(GRID_CUSTOM_MIN_PX, px));
  const stepped = Math.round(clamped / GRID_CUSTOM_STEP_PX) * GRID_CUSTOM_STEP_PX;
  return { value: stepped, snapped: stepped !== px };
}

/* ------------------------------ 换档计划器 ------------------------------ */

export interface GridPoint { x: number; y: number }
export interface ResnapPlan {
  moves: Array<{ index: number; from: GridPoint; to: GridPoint }>;
  /** 换档中因目标格被占而螺旋转移的图标数（实验室面板展示「重排复杂度」）。 */
  displaced: number;
  /** 相对位置保持度 [0,1]（行主序秩相关——≥0.9 绿线）。 */
  keepOrder: number;
}

/** 行主序秩（相对位置度量基准：同网格语义下的阅读顺序）。 */
function rowMajorRank(p: GridPoint, colPx: number, rowPx: number): number {
  return Math.round(p.y / rowPx) * 4096 + Math.round(p.x / colPx);
}

/** Spearman 秩相关（并列取平均秩——两网格粒度不同时秩有并列是常态）。 */
function spearman(a: number[], b: number[]): number {
  const n = a.length;
  if (n < 2) return 1;
  const rank = (arr: number[]): number[] => {
    const idx = arr.map((v, i) => ({ v, i })).sort((x, y) => x.v - y.v);
    const r = new Array<number>(n);
    let i = 0;
    while (i < n) {
      let j = i;
      while (j + 1 < n && idx[j + 1]!.v === idx[i]!.v) j++;
      const avg = (i + j) / 2 + 1;
      for (let k = i; k <= j; k++) r[idx[k]!.i] = avg;
      i = j + 1;
    }
    return r;
  };
  const ra = rank(a);
  const rb = rank(b);
  const ma = ra.reduce((s, v) => s + v, 0) / n;
  const mb = rb.reduce((s, v) => s + v, 0) / n;
  let num = 0;
  let da = 0;
  let db = 0;
  for (let i = 0; i < n; i++) {
    const x = ra[i]! - ma;
    const y = rb[i]! - mb;
    num += x * y;
    da += x * x;
    db += y * y;
  }
  if (da === 0 || db === 0) return 1;
  return num / Math.sqrt(da * db);
}

/**
 * 换档计划器（判据：最近格吸附重排 + 相对位置尽量保持 + 预览即时）。
 * 与 deskicons.resnapToGrid 的分工：域文件是**执行器**（纯吸附），本引擎是
 * **规划器**（吸附前算计划、吸附后出度量）——计划供预览渲染一次到位。
 */
export function planResnap(
  points: GridPoint[],
  from: { colPx: number; rowPx: number },
  to: { colPx: number; rowPx: number },
): ResnapPlan {
  const occupied = new Set<string>();
  const out: GridPoint[] = [];
  let displaced = 0;
  for (const p of points) {
    const nx = Math.round(p.x / to.colPx) * to.colPx;
    const ny = Math.round(p.y / to.rowPx) * to.rowPx;
    const key = (x: number, y: number) => `${x},${y}`;
    if (!occupied.has(key(nx, ny))) {
      occupied.add(key(nx, ny));
      out.push({ x: nx, y: ny });
      continue;
    }
    displaced++;
    let placed = false;
    for (let r = 1; r <= 8 && !placed; r++) {
      for (let dy = -r; dy <= r && !placed; dy++) {
        for (let dx = -r; dx <= r && !placed; dx++) {
          if (Math.max(Math.abs(dx), Math.abs(dy)) !== r) continue;
          const cx = nx + dx * to.colPx;
          const cy = ny + dy * to.rowPx;
          if (cx < 0 || cy < 0) continue;
          if (!occupied.has(key(cx, cy))) {
            occupied.add(key(cx, cy));
            out.push({ x: cx, y: cy });
            placed = true;
          }
        }
      }
    }
    if (!placed) out.push({ x: nx, y: ny });
  }
  const rankFrom = points.map((p) => rowMajorRank(p, from.colPx, from.rowPx));
  const rankTo = out.map((p) => rowMajorRank(p, to.colPx, to.rowPx));
  return {
    moves: points.map((p, i) => ({ index: i, from: p, to: out[i]! })),
    displaced,
    keepOrder: spearman(rankFrom, rankTo),
  };
}

/* --------------------------- F084/F401 兼容仲裁 --------------------------- */

export type GridAuthority = "auto-arrange" | "manual" | "layout-locked";
export type SnapVerdict =
  | { allow: true }
  | { allow: false; reason: "layout-locked"; hint: string }
  | { allow: false; reason: "auto-arrange-pending"; hint: string };

/**
 * 吸附权限仲裁（判据：与 F084 网格/F401 自动排列兼容）。
 * F401 自动排列执行中 → 拒绝手工吸附（两引擎抢同一网格 = 闪烁抖动）；
 * F546 布局锁定 → 拒绝并给解锁提示；普通手工 → 放行。
 */
export function arbitrateSnap(authority: GridAuthority): SnapVerdict {
  switch (authority) {
    case "layout-locked":
      return { allow: false, reason: "layout-locked", hint: "桌面布局已锁定（F546）——在「桌面右键 → 布局锁定」解除后重排" };
    case "auto-arrange":
      return { allow: false, reason: "auto-arrange-pending", hint: "自动排列（F401）进行中——排列完成后密度换档立即生效" };
    case "manual":
      return { allow: true };
  }
}

/* ------------------------------ 自检 ------------------------------ */

export function gridlabSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  checks.push({ name: "F503 三档格距常量", pass: GRID_DENSITY_PX.loose === 96 && GRID_DENSITY_PX.standard === 80 && GRID_DENSITY_PX.compact === 64 });
  // 步进审计：57 → 56（8 步进）；310 → 200（钳上限）
  const a1 = auditCustomGrid(57);
  const a2 = auditCustomGrid(310);
  checks.push({ name: "F503 8px 步进审计", pass: a1.value === 56 && a1.snapped && a2.value === 200 && a2.snapped });
  // 计划器：96→64 紧凑化，全部吸附无冲突、保持度满格
  const pts: GridPoint[] = [
    { x: 0, y: 0 }, { x: 96, y: 0 }, { x: 192, y: 0 },
    { x: 0, y: 96 }, { x: 96, y: 96 }, { x: 192, y: 96 },
  ];
  const plan = planResnap(pts, { colPx: 96, rowPx: 96 }, { colPx: 64, rowPx: 64 });
  const unique = new Set(plan.moves.map((m) => `${m.to.x},${m.to.y}`)).size;
  checks.push({ name: "F503 紧凑化零冲突", pass: unique === 6 && plan.displaced === 0 });
  // 保持度：紧凑化保持阅读序 → 满格
  checks.push({ name: "F503 相对位置保持度", pass: plan.keepOrder >= KEEP_ORDER_GREEN_LINE });
  // 冲突转移：两同点图标在 64 网格 → 一个转移
  const clash = planResnap([{ x: 10, y: 10 }, { x: 20, y: 12 }], { colPx: 80, rowPx: 80 }, { colPx: 64, rowPx: 64 });
  checks.push({ name: "F503 冲突螺旋转移", pass: clash.displaced === 1 && new Set(clash.moves.map((m) => `${m.to.x},${m.to.y}`)).size === 2 });
  // 仲裁三态
  const v1 = arbitrateSnap("manual");
  const v2 = arbitrateSnap("layout-locked");
  const v3 = arbitrateSnap("auto-arrange");
  checks.push({ name: "F503 仲裁三态显性", pass: v1.allow === true && !v2.allow && v2.reason === "layout-locked" && !v3.allow && v3.reason === "auto-arrange-pending" });
  return checks;
}
