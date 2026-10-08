/**
 * VE-F0611 · 图层命中测试（D 域 · 2D 合成引擎图层树组 · 批次 D01）
 * ---------------------------------------------------------------------------
 * 职责定位：图层命中测试——屏幕点到图层的**逆映射**。屏幕上一点要回答
 * 「用户点到了哪一层」，做法是把屏幕点反投回每一层的局部坐标系，交给该层的
 * 形状自己判定。本条是 UI 输入与图层树之间唯一的语义出入口。
 *
 * 上游 F0602 逆矩阵缓存（反投的数学前提）、F0604 裁剪系统（裁剪外不可命中）、
 *       F0606 Z 序与重排（命中序的依据）、F0607 可见性传播（隐藏层不可命中）。
 * 下游 F0654 蒙版级命中细化（镂空区的精确蒙版判定不在本条，见穿透规则表）、F0618 调试可视化。
 *
 * 锚点契约（五条，逐条对应判据）：
 *   1. 逆序遍历：按 Z 序**逆序**遍历，最上层优先。次序依据 F0606 三序同源契约——
 *      遍历序 = 绘制序 = 命中序，三者共用同一份次序，本条不得另立一套排序。
 *   2. 逐层逆变换：屏幕点用该层的**逆矩阵缓存**（F0602）反投到局部坐标，
 *      逆矩阵缺失或退化时降级为包围盒判定并登记诊断，绝不静默丢弃该层。
 *   3. 两级判定：O(1) 包围盒预筛 + O(形状) 精确判定。预筛是为了让绝大多数层
 *      在一次比较内出局，精确判定只对预筛通过的少数层执行。
 *   4. 命中穿透语义：透明区域与蒙版镂空区域**穿透**到下层。穿透不是「判定失败就
 *      继续」这么简单——穿透与「跳过」不同：跳过的层不构成遮挡，穿透的层要被记录。
 *   5. 三一致性：命中结果与裁剪一致（裁剪外不可命中，F0604）、与可见性一致
 *      （隐藏层不可命中，F0607）、与不透明度一致（alpha 为 0 的层既不绘制也不遮挡，
 *      F0603「保留布局与命中豁免语义」的直译）。三一致性是本条最容易被绕过的一条——
 *      只要命中管线绕开裁剪与可见性，就会出现「看不见却点得到」的层。
 *
 * 四条容易做错、故显式记录的设计立场：
 *
 *   一、逆序遍历必须「稳定」，否则重叠层的命中会随机漂移。
 *     同 z 序键的层按插入序稳定排序（F0606 契约），命中逆序时反转的是**整个
 *     次序**而非任意重排。反转一个已稳定排序的数组仍是有定义的（后插入者在上），
 *     但若实现时用了不稳定排序做「取最上」，重叠区的命中目标会在两次相同点击间
 *     跳变——这类 bug 极难复现，因为只在特定重叠布局下出现。
 *
 *   二、两级判定的预筛必须用「世界包围盒」而不是「局部包围盒」。
 *     局部包围盒要先反投才知道位置，等于把精确判定的成本前置了，白白多一次矩阵变换。
 *     世界包围盒在树的变换阶段就已算好（F0601/F0602 的缓存），预筛退化为一次
 *     四比较的矩形包含判定，这才是 O(1) 的本意。
 *
 *   三、穿透与跳过必须区分记录，否则调试面板无法解释「为什么点空」。
 *     蒙版镂空、整层 pass-through、alpha 为 0 三者在最终结果上都表现为
 *     「这层没被点到」，但处置方向完全不同：镂空要交给 F0654 细化、alpha 为 0
 *     是 F0603 的既定语义、pass-through 是显式声明。诊断码必须拆开——
 *     处置相反的状态共用一个码，会让走查时无法定位。
 *
 *   四、逆矩阵退化时降级为包围盒判定，而不是「跳过该层」。
 *     退化意味着变换链已出奇异（F0602 的行列式近零），此时局部坐标不可信，
 *     但**世界包围盒仍然可信**（它由变换前的局部盒推得）。用世界盒判定给出
 *     一个宽松命中，比整层跳过更接近用户直觉（用户点在图形附近就期望有反馈），
 *     同时登记诊断让上层知道这次命中是降级产物。若选择跳过，用户会遇到
 *     「点得到但有时点不到」的间歇性失效，且无从排查。
 *
 * 零静默纪律：逆矩阵退化、层数超阈、非有限坐标、穿透歧义、可见性/裁剪不一致
 * 全部产出 Diagnostic（code + message + hint），降级一律显性不做暗转。
 *
 * 判据：逆序遍历、两级判定、三一致性、穿透规则、判据。
 * 依赖锚点：F0602 逆矩阵缓存、F0604 裁剪系统、F0606 Z 序与重排、F0607 可见性传播、
 *          F0603 不透明度边界语义；下游 F0654 蒙版级命中细化。
 * 交接说明：本条只做**图层级**命中，不做像素级命中。形状判定是解析式的
 *          （矩形/圆角矩形/多边形/椭圆），不读帧缓冲像素——像素级读回需要
 *          GPU 回读通道且延迟不可控，属于 F0654 蒙版细化的范围。
 *          本条的命中结果结构刻意保留了 `degraded` 标记，下游可据此决定
 *          是否触发像素级复核。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型
// ════════════════════════════════════════════════════════════════════════════

/**
 * 命中专属诊断码。
 * 处置方向相反的状态一律不共用码（立场三）：
 * 镂空穿透要交给 F0654、alpha 为 0 是既定语义、pass-through 是显式声明，
 * 三者若共用一个 PASS_THROUGH 码，走查时无法定位是哪一类。
 */
export type HitDiagCode =
  /** 逆矩阵缺失或退化（行列式近零），已降级为世界包围盒判定。 */
  | "INVERSE_DEGENERATE"
  /** 屏幕点含非有限坐标（NaN / Infinity），该点被拒绝。 */
  | "NON_FINITE_POINT"
  /** 层注册信息缺失（登记表中查不到该节点）。 */
  | "LAYER_NOT_REGISTERED"
  /** 层注册信息非法（包围盒为负、透明度越界、z 键非有限）。 */
  | "LAYER_REGISTRATION_INVALID"
  /** 层级数超出统计阈值——不截断遍历，仅登记（截断会丢命中，是错的）。 */
  | "LAYER_COUNT_EXCEEDS_THRESHOLD"
  /** 遍历序与绘制序不一致，违反 F0606 三序同源契约。 */
  | "Z_ORDER_INCONSISTENT"
  /** 命中落在裁剪域外却被判为命中，违反 F0604 一致性。 */
  | "CLIP_INCONSISTENT"
  /** 不可见层被判为命中，违反 F0607 一致性。 */
  | "VISIBILITY_INCONSISTENT"
  /** 穿透规则命中歧义分支，已由规则表裁决并留痕。 */
  | "PENETRATION_AMBIGUOUS"
  /** 图层树结构非法（多父、环、根缺失），命中遍历拒绝启动。 */
  | "TREE_INVARIANT_VIOLATED";

/** 一条诊断：发生了什么、影响什么、下一步怎么办。 */
export interface HitDiagnostic {
  readonly code: HitDiagCode;
  readonly message: string;
  readonly hint: string;
}

/** 结果判别联合。 */
export type HitOutcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly HitDiagnostic[] }
  | {
      readonly ok: false;
      readonly code: HitDiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly HitDiagnostic[];
    };

/** 成功构造。 */
export function hitOk<T>(value: T, diagnostics: readonly HitDiagnostic[] = []): HitOutcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造。 */
export function hitFail<T>(
  code: HitDiagCode,
  message: string,
  hint: string,
): HitOutcome<T> {
  const d: HitDiagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [d] };
}

/** 诊断聚合器。 */
export class HitDiagBag {
  private readonly items: HitDiagnostic[] = [];

  push(code: HitDiagCode, message: string, hint: string): void {
    this.items.push({
      code,
      message: message || "（未提供描述）",
      hint: hint || "（未提供处置建议）",
    });
  }

  get size(): number {
    return this.items.length;
  }

  all(): readonly HitDiagnostic[] {
    return this.items.slice();
  }

  byCode(code: HitDiagCode): readonly HitDiagnostic[] {
    return this.items.filter((d) => d.code === code);
  }

  has(code: HitDiagCode): boolean {
    return this.items.some((d) => d.code === code);
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 命中管线的输入契约（上游锚点在本条的投影形态）
// ════════════════════════════════════════════════════════════════════════════

/** 几何基元：2D 仿射矩阵 [a c e; b d f; 0 0 1]，与 F0602 同构。 */
export interface Mat2D {
  readonly a: number;
  readonly b: number;
  readonly c: number;
  readonly d: number;
  readonly e: number;
  readonly f: number;
}

/** 点。 */
export interface Pt {
  readonly x: number;
  readonly y: number;
}

/** 轴对齐矩形（世界或局部坐标系由上下文决定，语义在字段名后缀体现）。 */
export interface Rect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** 可见性三态（F0607）：隐藏保留布局、彻底移除是两种不同的不可见。 */
export type VisibilityTri = "visible" | "hidden" | "removed";

/** 图层形状（解析式判定，不读像素）。 */
export type LayerShape =
  | { readonly kind: "rect"; readonly rect: Rect }
  | {
      readonly kind: "roundRect";
      readonly rect: Rect;
      /** 四角半径顺序：左上、右上、右下、左下。 */
      readonly radii: readonly [number, number, number, number];
    }
  | { readonly kind: "polygon"; readonly points: readonly Pt[] }
  | { readonly kind: "ellipse"; readonly cx: number; readonly cy: number; readonly rx: number; readonly ry: number };

/**
 * 单层的命中登记信息（上游各锚点在本条的汇合点）。
 *
 * 字段刻意做平而非嵌套上游对象：命中管线是每帧可能被调用上万次的热路径，
 * 一次属性访问链比一次扁平字段读取贵得多，且 F0602/F0604/F0606/F0607
 * 各家的对象形态会随各自演进，命中条只依赖它真正用到的字段。
 */
export interface HitRegistration {
  readonly nodeId: string;
  readonly parentId: string | null;
  /** Z 序键（F0606）：同值按 insertOrder 稳定排序。 */
  readonly zKey: number;
  /** 插入序（F0606 稳定性契约的裁决依据）。 */
  readonly insertOrder: number;
  /** 本层声明的可见性（F0607 三态）。 */
  readonly visibility: VisibilityTri;
  /** 本层声明的不透明度（F0603），已解析为 0..1。 */
  readonly opacity: number;
  /** 世界包围盒：由变换后的局部盒推得，预筛用（立场二）。 */
  readonly worldBounds: Rect;
  /** 有效裁剪域（F0604 沿祖先链求交后，位于**本层局部坐标系**）；null 表示无裁剪。 */
  readonly effectiveClipLocal: Rect | null;
  /** 世界矩阵（F0602 级联结果）。 */
  readonly worldMatrix: Mat2D;
  /**
   * 逆矩阵缓存（F0602）。null 表示未缓存或缓存失效——
   * 命中管线不自己算逆矩阵，只消费缓存（逆矩阵的构造与失效归 F0602）。
   */
  readonly inverseMatrix: Mat2D | null;
  /** 形状；null 表示组节点或根节点——组自身不参与命中，只作为下钻容器。 */
  readonly shape: LayerShape | null;
  /** 蒙版镂空标记：本层的形状区域属于镂空（不遮挡、穿透），细化归 F0654。 */
  readonly maskHole: boolean;
  /** 整层显式穿透声明：本层完全不参与命中（装饰性覆盖层常用）。 */
  readonly passThrough: boolean;
}

/** 图层树的最小结构视图（命中只需结构，不需属性表）。 */
export interface HitTree {
  readonly nodes: ReadonlyMap<string, HitRegistration>;
  readonly rootId: string;
  /** 树全局版本（F0601/F0607 与之同源递增）；缺省 0 由调用方补齐。 */
  readonly treeVersion?: number;
}

/**
 * 展平后的可命中层序（先序 = 绘制序 = 遍历序 = Z 序，F0606 三序同源）。
 * 命中时对其**逆序**遍历，最上层优先（判据一）。
 */
export interface HitOrder {
  /** 先序排列的层 id，最上（Z 序最大）在**末尾**——逆序遍历即从末尾起。 */
  readonly ids: readonly string[];
  /** 逐层的有效可见性（父隐藏则子树全隐藏，F0607 传播结果）。 */
  readonly effectiveVisible: ReadonlyMap<string, boolean>;
  /** 逐层的有效不透明度（祖先连乘，F0603 分组合成语义）。 */
  readonly effectiveOpacity: ReadonlyMap<string, number>;
  /** 图层树全局版本（F0601/F0607 与之同源递增）。 */
  readonly treeVersion: number;
}

// ════════════════════════════════════════════════════════════════════════════
// §3 命中结果
// ════════════════════════════════════════════════════════════════════════════

/** 命中结果的一条记录。 */
export interface HitRecord {
  readonly nodeId: string;
  /** 判定阶段：包围盒降级 or 形状精确判定。 */
  readonly stage: "precise" | "bbox-degraded";
  /** 该层在命中序中的深度（根为 0），供调用方做层级语义。 */
  readonly depth: number;
}

/** 命中查询结果。 */
export interface HitResult {
  /** 屏幕点原样回显，便于调用方对账。 */
  readonly point: Pt;
  /**
   * 命中链：自最上层向下直到命中为止，穿透而过的层也在链内（立场三）。
   * 空数组表示该点命中了背景（穿透到底）。
   */
  readonly chain: readonly HitRecord[];
  /** 命中统计。 */
  readonly stats: HitStats;
  /** 该点查询过程中产生的诊断。 */
  readonly diagnostics: readonly HitDiagnostic[];
}

/** 命中统计（性能分解的可观测面，进 F0649 度量）。 */
export interface HitStats {
  /** 参与遍历的层数。 */
  readonly visited: number;
  /** 包围盒预筛通过的层数。 */
  readonly bboxPassed: number;
  /** 进入精确形状判定的层数。 */
  readonly preciseTested: number;
  /** 因逆矩阵退化而降级判定的层数。 */
  readonly degraded: number;
  /** 因不可见/裁剪外/穿透而跳过的层数。 */
  readonly skipped: number;
  /** 是否至少发生一次降级判定。 */
  readonly anyDegraded: boolean;
}

// ════════════════════════════════════════════════════════════════════════════
// §4 几何基元（矩阵、包围盒、形状判定）
// ════════════════════════════════════════════════════════════════════════════

/** 行列式近零阈值（F0602 同值契约）：低于此值判奇异。 */
export const DEGENERATE_EPS = 1e-9;

/** 形状边界容差：边界上的点算命中（避免描边处漏命中）。 */
export const SHAPE_BOUNDARY_EPS = 1e-9;

/** 层数统计阈值：超过只登记诊断，绝不截断遍历。 */
export const LAYER_COUNT_THRESHOLD = 4096;

/** 判断是否为有限数。 */
export function isFiniteNum(v: number): boolean {
  return Number.isFinite(v);
}

/** 判断点是否有限。 */
export function isFinitePt(p: Pt): boolean {
  return isFiniteNum(p.x) && isFiniteNum(p.y);
}

/** 矩阵是否有限。 */
export function isFiniteMat(m: Mat2D): boolean {
  return (
    isFiniteNum(m.a) && isFiniteNum(m.b) && isFiniteNum(m.c) &&
    isFiniteNum(m.d) && isFiniteNum(m.e) && isFiniteNum(m.f)
  );
}

/** 矩阵行列式。 */
export function det2d(m: Mat2D): number {
  return m.a * m.d - m.b * m.c;
}

/**
 * 判断矩阵是否退化（行列式近零，F0602 契约）。
 * 退化矩阵不可反投——反投公式要除以行列式，近零时结果发散成天文数字，
 * 后续形状判定会因坐标过大而全部失真。故必须在反投**之前**判定。
 */
export function isDegenerate(m: Mat2D): boolean {
  return !isFiniteMat(m) || Math.abs(det2d(m)) < DEGENERATE_EPS;
}

/** 正向变换一个点。 */
export function applyMat(m: Mat2D, p: Pt): Pt {
  return { x: m.a * p.x + m.c * p.y + m.e, y: m.b * p.x + m.d * p.y + m.f };
}

/** 逆变换一个点（**要求** det2d 非近零；调用方须先经 isDegenerate 拦截）。 */
export function applyInverseMat(m: Mat2D, p: Pt): Pt {
  const dt = det2d(m);
  const inv = 1 / dt;
  const px = p.x - m.e;
  const py = p.y - m.f;
  return { x: (m.d * px - m.c * py) * inv, y: (m.a * py - m.b * px) * inv };
}

/**
 * 反投屏幕点到层局部坐标。
 *
 * 语义澄清（易错点）：`inverseMatrix` 是 F0602 **已缓存的逆矩阵**，
 * 反投就是「用逆矩阵做一次正向变换」，因此此处调 applyMat 而非 applyInverseMat。
 * 若误调 applyInverseMat，等于对逆矩阵再取一次逆，旋转层会整体判错且
 * 表现为「平移正常、旋转全错」——这类半失效现象极难归因。
 * 用 applyInverseMat 的前提是手上只有正向矩阵（此时才现场取逆）。
 */
export function projectToLocal(inverse: Mat2D, p: Pt): Pt {
  return applyMat(inverse, p);
}

/** 矩形是否合法（有限且宽高非负）。 */
export function isValidRect(r: Rect): boolean {
  return isFiniteNum(r.x) && isFiniteNum(r.y) && isFiniteNum(r.w) && isFiniteNum(r.h) && r.w >= 0 && r.h >= 0;
}

/**
 * 点是否落在矩形内（含边界容差）。
 * 与 rectContainsAxisAligned 同义，保留本名作为矩形含点的语义入口。
 */
export function rectContains(r: Rect, p: Pt, eps = SHAPE_BOUNDARY_EPS): boolean {
  return (
    p.x >= r.x - eps && p.x <= r.x + r.w + eps &&
    p.y >= r.y - eps && p.y <= r.y + r.h + eps
  );
}

/** 两个矩形求交；不相交返回 null（相交域为空）。 */
export function intersectRect(a: Rect, b: Rect): Rect | null {
  const x0 = Math.max(a.x, b.x);
  const y0 = Math.max(a.y, b.y);
  const x1 = Math.min(a.x + a.w, b.x + b.w);
  const y1 = Math.min(a.y + a.h, b.y + b.h);
  if (x1 <= x0 || y1 <= y0) return null;
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/**
 * 射线法判定点是否在多边形内（含边界）。
 * 边界判定单列：用户点在线描边上算命中是交互惯例，
 * 纯射线法在顶点与水平边附近会给出抖动结果（跨过边界 ±eps 即翻转）。
 */
export function polygonContains(poly: readonly Pt[], p: Pt): boolean {
  const n = poly.length;
  if (n < 3) return false;
  // 索引必落在 [0, n) 内（n<3 已提前返回），此处用解断言收窄严格模式的
  // noUncheckedIndexedAccess——逐个下标判空会把热路径拖成两倍分支。
  // 严格模式（noUncheckedIndexedAccess）下标访问会带入 undefined，
  // 逐点判空既啰嗦又拖慢热路径。n<3 已提前返回，索引必在 [0,n) 内，
  // 故此处统一经非空取值收窄——判空后跳过该顶点，语义不变且不会漏判边界。
  const at = (k: number): Pt => {
    const q = poly[k < 0 ? k + n : k];
    return q === undefined ? { x: 0, y: 0 } : q;
  };
  // 边界优先：对每条边做点—线段距离判定
  for (let i = 0, j = n - 1; i < n; j = i, i += 1) {
    if (pointOnSegment(at(j), at(i), p)) return true;
  }
  let inside = false;
  for (let i = 0, j = n - 1; i < n; j = i, i += 1) {
    const vi = at(i);
    const vj = at(j);
    const yi = vi.y;
    const yj = vj.y;
    if (yi > p.y !== yj > p.y) {
      const t = (p.y - yi) / (yj - yi);
      const x = vi.x + t * (vj.x - vi.x);
      if (p.x < x) inside = !inside;
    }
  }
  return inside;
}

/** 点是否落在线段上（含容差）。 */
export function pointOnSegment(a: Pt, b: Pt, p: Pt, eps = SHAPE_BOUNDARY_EPS): boolean {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len2 = dx * dx + dy * dy;
  let t = 0;
  if (len2 > 0) {
    t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2;
  }
  const cx = a.x + t * dx;
  const cy = a.y + t * dy;
  const dist = Math.hypot(p.x - cx, p.y - cy);
  const scale = Math.max(1, Math.hypot(dx, dy));
  return dist <= eps * scale;
}

/**
 * 圆角矩形判定。
 *
 * 判定次序（次序错就会漏判缺角，故显式固定）：
 *   1. 外接矩形外 → 直接不命中。这一步是**必要**的前置闸门：
 *      缺角区域（如 100×100 圆角 30 时的 (1,1)）落在外接矩形内但不在形状内，
 *      若缺少本步而只靠「内矩形 + 四角圆」判否，后续任何兜底放行都会把缺角判成命中。
 *   2. 内矩形（外矩形向内收缩四个半径）内 → 命中。
 *   3. 四角圆：到该角圆心的距离 ≤ 该角半径 → 命中。四半径不一致时逐角独立判定。
 *   4. 剩余情形是四条边带（半径各异的过渡区），带内即命中。
 * 半径为 0 时本函数严格等价于轴对齐矩形判定（内矩形即外矩形）。
 */
export function roundRectContains(
  rect: Rect,
  radii: readonly [number, number, number, number],
  p: Pt,
): boolean {
  const [tl, tr, br, bl] = radii;
  // 前置闸门：外接矩形之外一律不命中（缺角误判的根因防线）
  if (!rectContainsAxisAligned(rect, p)) return false;
  // 半径钳制：不得超过对应边长的一半，超出时几何上等价于「被相邻半径合并的胶囊」
  const cap = (r: number, w: number, h: number): number =>
    Math.max(0, Math.min(r, Math.min(w, h) / 2));
  const rtl = cap(tl, rect.w, rect.h);
  const rtr = cap(tr, rect.w, rect.h);
  const rbr = cap(br, rect.w, rect.h);
  const rbl = cap(bl, rect.w, rect.h);
  const x1 = rect.x + rect.w;
  const y1 = rect.y + rect.h;
  const innerX0 = rect.x + rtl;
  const innerY0 = rect.y + rtl;
  const innerX1 = x1 - rtr;
  const innerY1 = y1 - rbr;
  // 步骤 2：内矩形（含边界）直接命中
  if (p.x >= innerX0 - SHAPE_BOUNDARY_EPS && p.x <= innerX1 + SHAPE_BOUNDARY_EPS &&
      p.y >= innerY0 - SHAPE_BOUNDARY_EPS && p.y <= innerY1 + SHAPE_BOUNDARY_EPS) {
    return true;
  }
  // 步骤 3：四角圆逐角独立判定
  const inCorner = (cxc: number, cyc: number, r: number): boolean => {
    if (r <= 0) return false;
    return Math.hypot(p.x - cxc, p.y - cyc) <= r + SHAPE_BOUNDARY_EPS;
  };
  if (inCorner(innerX0, innerY0, rtl)) return true;
  if (inCorner(innerX1, innerY0, rtr)) return true;
  if (inCorner(innerX1, innerY1, rbr)) return true;
  if (inCorner(innerX0, innerY1, rbl)) return true;
  // 步骤 4：边带。边带是「外接矩形减去四角方块」后的剩余区域，
  // 因此**两个方向都要约束**：上/下带须x 落在内矩形横向区间内，
  // 左/右带须 y 落在内矩形纵向区间内。
  // 只约束单向是缺角漏判的经典成因——上带判据只比y 时，
  // 缺角点 (1,1)（y=1 落在上带内）会被误判命中。
  const inInnerX = p.x >= innerX0 - SHAPE_BOUNDARY_EPS && p.x <= innerX1 + SHAPE_BOUNDARY_EPS;
  const inInnerY = p.y >= innerY0 - SHAPE_BOUNDARY_EPS && p.y <= innerY1 + SHAPE_BOUNDARY_EPS;
  if (inInnerX && p.y >= rect.y - SHAPE_BOUNDARY_EPS) return true;
  if (inInnerX && p.y <= y1 + SHAPE_BOUNDARY_EPS) return true;
  if (inInnerY && p.x >= rect.x - SHAPE_BOUNDARY_EPS) return true;
  if (inInnerY && p.x <= x1 + SHAPE_BOUNDARY_EPS) return true;
  // 走到此处说明落在某个缺角区域内且不在对应圆内 → 不命中
  return false;
}

/** 纯轴对齐矩形带判定（圆角矩形四边区域的兜底）。 */
export function rectContainsAxisAligned(rect: Rect, p: Pt): boolean {
  return (
    p.x >= rect.x - SHAPE_BOUNDARY_EPS && p.x <= rect.x + rect.w + SHAPE_BOUNDARY_EPS &&
    p.y >= rect.y - SHAPE_BOUNDARY_EPS && p.y <= rect.y + rect.h + SHAPE_BOUNDARY_EPS
  );
}

/** 椭圆判定（归一化半径比较，避免除零）。 */
export function ellipseContains(cx: number, cy: number, rx: number, ry: number, p: Pt): boolean {
  if (rx <= 0 || ry <= 0) return false;
  const nx = (p.x - cx) / rx;
  const ny = (p.y - cy) / ry;
  return nx * nx + ny * ny <= 1 + SHAPE_BOUNDARY_EPS;
}

/** 形状的局部包围盒（供登记时构建 worldBounds 的前置校验）。 */
export function shapeLocalBounds(shape: LayerShape): Rect | null {
  switch (shape.kind) {
    case "rect":
      return shape.rect;
    case "roundRect":
      return shape.rect;
    case "ellipse":
      return { x: shape.cx - shape.rx, y: shape.cy - shape.ry, w: shape.rx * 2, h: shape.ry * 2 };
    case "polygon": {
      if (shape.points.length === 0) return null;
      let minX = Infinity;
      let minY = Infinity;
      let maxX = -Infinity;
      let maxY = -Infinity;
      for (const p of shape.points) {
        if (!isFinitePt(p)) return null;
        if (p.x < minX) minX = p.x;
        if (p.y < minY) minY = p.y;
        if (p.x > maxX) maxX = p.x;
        if (p.y > maxY) maxY = p.y;
      }
      return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
    }
    default:
      return null;
  }
}

/** 局部点是否落在形状内（精确判定的第二步，O(形状)）。 */
export function shapeContainsLocal(shape: LayerShape, p: Pt): boolean {
  switch (shape.kind) {
    case "rect":
      return rectContainsAxisAligned(shape.rect, p);
    case "roundRect":
      return roundRectContains(shape.rect, shape.radii, p);
    case "polygon":
      return polygonContains(shape.points, p);
    case "ellipse":
      return ellipseContains(shape.cx, shape.cy, shape.rx, shape.ry, p);
    default:
      return false;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §5 穿透规则表（判据四）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 穿透规则的裁决结果。
 * 关键区分：`continue`（穿透——记录进链，继续向下找）与
 * `abort`（终止——命中栈顶，不再向下）。跳过类（不可见、裁剪外）
 * 既不记录也不终止，因为它们不构成遮挡。
 */
export type PenetrationVerdict = "hit" | "continue" | "skip" | "abort";

/** 穿透规则表：给定层与判定阶段，裁决下一步走向。 */
export interface PenetrationRule {
  readonly code: HitDiagCode | "CLEAR";
  readonly verdict: PenetrationVerdict;
  /** 记录进命中链（仅 verdict 为 continue/hit 时有意义）。 */
  readonly record: boolean;
  readonly reason: string;
}

/**
 * 裁决函数（纯函数，无共享态——多点触控并发靠此保证逐点独立）。
 *
 * 裁决次序即语义优先级，从上到下：
 *   1. 树结构非法 → abort（不在本函数，管线入口已拦）
 *   2. 不可见 / alpha 为 0 → skip（不构成遮挡）
 *   3. 显式 pass-through → continue + record
 *   4. 蒙版镂空 → continue + record（细化交 F0654）
 *   5. 组节点（无形状）→ skip（只做容器，不下钻判定）
 *   6. 预筛未过 → skip
 *   7. 裁剪外 → skip（F0604 一致性：裁剪外的像素不存在）
 *   8. 精确判定未过 → continue + record（落在形状外=透明区域，穿透）
 *   9. 精确判定命中 → hit + record
 */
export function decidePenetration(input: {
  readonly effectiveVisible: boolean;
  readonly effectiveOpacity: number;
  readonly passThrough: boolean;
  readonly maskHole: boolean;
  readonly isGroup: boolean;
  readonly bboxPassed: boolean;
  readonly withinClip: boolean;
  readonly shapeHit: boolean;
}): PenetrationRule {
  if (!input.effectiveVisible || input.effectiveOpacity <= 0) {
    return {
      code: "CLEAR",
      verdict: "skip",
      record: false,
      reason: "不可见或有效不透明度为 0——该层不绘制亦不遮挡（F0603/F0607 一致性）",
    };
  }
  if (input.passThrough) {
    return {
      code: "CLEAR",
      verdict: "continue",
      record: true,
      reason: "显式 pass-through 声明：本层完全穿透，记录后继续下钻",
    };
  }
  if (input.maskHole) {
    return {
      code: "CLEAR",
      verdict: "continue",
      record: true,
      reason: "蒙版镂空区域：图层级穿透，像素级细化归 F0654",
    };
  }
  if (input.isGroup) {
    return {
      code: "CLEAR",
      verdict: "skip",
      record: false,
      reason: "组节点自身无形状，仅作下钻容器",
    };
  }
  if (!input.bboxPassed) {
    return {
      code: "CLEAR",
      verdict: "skip",
      record: false,
      reason: "包围盒预筛未通过：一次比较即出局（O(1)）",
    };
  }
  if (!input.withinClip) {
    return {
      code: "CLEAR",
      verdict: "skip",
      record: false,
      reason: "落在有效裁剪域外：裁剪外像素不存在，与 F0604 保持一致",
    };
  }
  if (!input.shapeHit) {
    return {
      code: "CLEAR",
      verdict: "continue",
      record: true,
      reason: "落在形状外的透明区域：穿透到下层",
    };
  }
  return {
    code: "CLEAR",
    verdict: "hit",
    record: true,
    reason: "形状精确判定命中",
  };
}

// ════════════════════════════════════════════════════════════════════════════
// §6 层序构建（F0606 三序同源 + F0607 有效可见性传播）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 兄弟间稳定排序（F0606 契约）：z 序键升序，同值按插入序升序。
 * 排序稳定性是契约而非实现细节——同 z 键的层必须保持插入先后。
 */
export function sortSiblingsStable(
  regs: readonly HitRegistration[],
  diagnostics: HitDiagBag,
): HitRegistration[] {
  const valid = regs.filter((r) => {
    if (!isFiniteNum(r.zKey) || !isFiniteNum(r.insertOrder)) {
      diagnostics.push(
        "LAYER_REGISTRATION_INVALID",
        `层 ${r.nodeId} 的 z 序键或插入序非有限`,
        "z 序键与插入序须为有限数；越界键按 F0606 钳制并告警，此处剔除该层参与排序",
      );
      return false;
    }
    return true;
  });
  return valid.slice().sort((a, b) => {
    if (a.zKey !== b.zKey) return a.zKey - b.zKey;
    return a.insertOrder - b.insertOrder;
  });
}

/**
 * 构建可命中层序（先序，三序同源）。
 * 同时计算有效可见性（父隐藏 → 子树全隐藏，F0607）与有效不透明度（祖先连乘，F0603）。
 *
 * 结构不变式校验：无环、单父、根存在。违规即拒绝启动——在脏树上做命中遍历，
 * 结果不可预测且无从归因（死循环或静默丢层）。
 */
export function buildHitOrder(tree: HitTree, diagnostics: HitDiagBag): HitOrder | null {
  const root = tree.nodes.get(tree.rootId);
  if (root === undefined) {
    diagnostics.push(
      "TREE_INVARIANT_VIOLATED",
      `根节点 ${tree.rootId} 不存在于节点表`,
      "根节点必须先注册再构建层序；检查图层树的构建顺序",
    );
    return null;
  }
  // 单父校验
  const seenParents = new Set<string>();
  for (const reg of tree.nodes.values()) {
    if (reg.parentId === null) continue;
    if (seenParents.has(reg.nodeId)) {
      diagnostics.push(
        "TREE_INVARIANT_VIOLATED",
        `层 ${reg.nodeId} 出现多个父节点`,
        "F0601 单父无环不变式被破坏；命中遍历拒绝启动",
      );
      return null;
    }
    seenParents.add(reg.nodeId);
    if (!tree.nodes.has(reg.parentId)) {
      diagnostics.push(
        "LAYER_NOT_REGISTERED",
        `层 ${reg.nodeId} 的父节点 ${reg.parentId} 未注册`,
        "父子引用必须双向一致；缺失父节点会导致子树不可下钻",
      );
      return null;
    }
  }
  const ids: string[] = [];
  const effectiveVisible = new Map<string, boolean>();
  const effectiveOpacity = new Map<string, number>();
  const emitted = new Set<string>();
  // 子节点索引：一次建表O(节点数)，供遍历 O(1) 取子节点
  // 不建索引而每层重扫全表会让万级深树退化为 O(节点数×深度)，实测不可接受
  const childrenOf = new Map<string, HitRegistration[]>();
  for (const creg of tree.nodes.values()) {
    if (creg.parentId === null) continue;
    const list = childrenOf.get(creg.parentId);
    if (list === undefined) childrenOf.set(creg.parentId, [creg]);
    else list.push(creg);
  }
  const treeVersion = tree.treeVersion ?? 0;
  // 显式栈迭代（不用递归：深树下递归栈溢出，且层级可达万级 F0616）
  type Frame = { readonly id: string; readonly visible: boolean; readonly opacity: number; readonly depth: number };
  const stack: Frame[] = [{ id: tree.rootId, visible: true, opacity: 1, depth: 0 }];
  while (stack.length > 0) {
    const frame = stack.pop() as Frame;
    // 已产出层序的节点再次被访问 = 成环（或同一节点多父可达）。
    // 判据用「全局已产出集合」而非「当前祖先路径」：显式栈迭代下，
    // 祖先路径标记会在 push 子节点后立即被清除，环 a→b→a 反复弹出时
    // 路径上永远只有单个节点，环检测会失效并导致无限 push（实测崩栈）。
    // 图层树的不变式是单父无环，故「每节点恰访问一次」与「无环」等价。
    if (emitted.has(frame.id)) {
      diagnostics.push(
        "TREE_INVARIANT_VIOLATED",
        `层 ${frame.id} 被重复访问（成环或单父不变式被破坏）`,
        "F0601 单父无环不变式被破坏；命中遍历拒绝启动",
      );
      return null;
    }
    emitted.add(frame.id);
    const reg = tree.nodes.get(frame.id);
    if (reg === undefined) {
      diagnostics.push(
        "LAYER_NOT_REGISTERED",
        `层 ${frame.id} 在节点表中缺失`,
        "构建层序时所有被引用节点都必须已注册",
      );
      return null;
    }
    const selfVisible = reg.visibility === "visible";
    const visible = frame.visible && selfVisible;
    const opacity = clamp01(frame.opacity * clamp01(reg.opacity));
    effectiveVisible.set(frame.id, visible);
    effectiveOpacity.set(frame.id, opacity);
    ids.push(frame.id);
    // 子节点逆序入栈：出栈顺序即升序，保持与绘制序一致
    // 子节点索引一次建好（O(节点数)），避免每层重扫全表退化为 O(节点数×深度)
    const children = childrenOf.get(frame.id);
    if (children !== undefined) {
      const sorted = sortSiblingsStable(children, diagnostics);
      for (let i = sorted.length - 1; i >= 0; i -= 1) {
        const child = sorted[i];
        if (child !== undefined) {
          stack.push({ id: child.nodeId, visible, opacity, depth: frame.depth + 1 });
        }
      }
    }
  }
  return { ids, effectiveVisible, effectiveOpacity, treeVersion };
}

/** 值域钳制到 0..1。 */
export function clamp01(v: number): number {
  if (!isFiniteNum(v)) return 0;
  if (v < 0) return 0;
  if (v > 1) return 1;
  return v;
}

// ════════════════════════════════════════════════════════════════════════════
// §7 命中管线主流程（判据一、二、三、五）
// ════════════════════════════════════════════════════════════════════════════

/** 深度索引（由 buildHitOrder 的先序隐含，此处由调用方按 ids 位置推）。 */
function depthOfIndex(index: number): number {
  return index;
}

/**
 * 单点命中查询（主流程）。
 *
 * 流程（逐层逆序 = 判据一）：
 *   1. 校验点有限 → 非有限直接拒绝（判据五的输入闸门）
 *   2. 从 Z 序最上层（ids 末尾）起逆序遍历
 *   3. 每层：可见性与 alpha → 世界包围盒预筛（O(1)）→ 逆矩阵反投 →
 *      裁剪域判定（F0604）→ 形状精确判定（O(形状)）→ 穿透规则表裁决
 *   4. 命中即终止（abort）；穿透则记录进链继续；跳过则不进链
 *
 * 逆矩阵缺失/退化 → 降级为世界包围盒判定 + 登记诊断 + 记 degraded（立场四）。
 */
export function hitTestPoint(
  tree: HitTree,
  order: HitOrder,
  point: Pt,
  diagnostics: HitDiagBag,
): HitOutcome<HitResult> {
  const bag = diagnostics;
  // 闸门：非有限点拒绝（NaN 会让所有比较为 false，静默穿透到底层）
  if (!isFinitePt(point)) {
    bag.push(
      "NON_FINITE_POINT",
      `屏幕点 (${String(point.x)}, ${String(point.y)}) 含非有限坐标`,
      "命中管线不吞掉非法点；请上游检查指针事件的坐标来源",
    );
    return hitFail<HitResult>(
      "NON_FINITE_POINT",
      "屏幕点含非有限坐标",
      "修正坐标来源后重试；该点未被判定",
    );
  }
  // 层数超阈只登记不截断（截断会丢命中）
  if (order.ids.length > LAYER_COUNT_THRESHOLD) {
    bag.push(
      "LAYER_COUNT_EXCEEDS_THRESHOLD",
      `可命中层数 ${order.ids.length} 超过阈值 ${LAYER_COUNT_THRESHOLD}`,
      "遍历继续（截断会丢命中）；层数治理归 F0616 大层数虚拟化",
    );
  }
  const chain: HitRecord[] = [];
  let visited = 0;
  let bboxPassed = 0;
  let preciseTested = 0;
  let degraded = 0;
  let skipped = 0;
  const depthById = new Map<string, number>();
  const idAt = (i: number): string => order.ids[i] ?? "";
  for (let i = 0; i < order.ids.length; i += 1) {
    depthById.set(idAt(i), depthOfIndex(i));
  }
  let aborted = false;
  // 逆序遍历：最上层优先
  for (let idx = order.ids.length - 1; idx >= 0 && !aborted; idx -= 1) {
    const id = idAt(idx);
    const reg = tree.nodes.get(id);
    visited += 1;
    if (reg === undefined) {
      skipped += 1;
      bag.push(
        "LAYER_NOT_REGISTERED",
        `层序中的层 ${id} 在节点表中缺失`,
        "层序与节点表必须同源构建；检查是否分别构造",
      );
      continue;
    }
    const visible = order.effectiveVisible.get(id) ?? false;
    const opacity = order.effectiveOpacity.get(id) ?? 0;
    const isGroup = reg.shape === null;
    const useBboxOnly = reg.inverseMatrix === null || isDegenerate(reg.inverseMatrix);
    // 预筛（O(1)）：用世界包围盒（立场二）
    const bboxHit = rectContainsAxisAligned(reg.worldBounds, point);
    // 快速早退：不可见 / alpha 为 0 / 组 / 预筛未过，一律不进精确判定
    if (!visible || opacity <= 0 || isGroup || !bboxHit) {
      const verdict = decidePenetration({
        effectiveVisible: visible,
        effectiveOpacity: opacity,
        passThrough: reg.passThrough,
        maskHole: reg.maskHole,
        isGroup,
        bboxPassed: bboxHit,
        withinClip: false,
        shapeHit: false,
      });
      if (verdict.verdict === "skip") skipped += 1;
      else if (verdict.verdict === "continue" && verdict.record) {
        chain.push({ nodeId: id, stage: "precise", depth: depthById.get(id) ?? idx });
      }
      continue;
    }
    bboxPassed += 1;
    if (reg.passThrough || reg.maskHole) {
      const verdict = decidePenetration({
        effectiveVisible: visible,
        effectiveOpacity: opacity,
        passThrough: reg.passThrough,
        maskHole: reg.maskHole,
        isGroup,
        bboxPassed: bboxHit,
        withinClip: true,
        shapeHit: false,
      });
      if (verdict.verdict === "continue" && verdict.record) {
        chain.push({ nodeId: id, stage: "precise", depth: depthById.get(id) ?? idx });
      } else if (verdict.verdict === "skip") skipped += 1;
      continue;
    }
    // 降级路径（判据二 + 立场四）：逆矩阵不可信 → 世界包围盒判定
    if (useBboxOnly) {
      degraded += 1;
      // 登记必须发生在两条分支之前：降级本身是异常事实，
      // 只在「裁剪外」这条分支登记会让「降级且命中」完全静默（实测踩过）
      bag.push(
        "INVERSE_DEGENERATE",
        reg.inverseMatrix === null
          ? `层 ${id} 逆矩阵缓存缺失，本次以世界包围盒降级判定`
          : `层 ${id} 逆矩阵退化（行列式近零），本次以世界包围盒降级判定`,
        "该层命中结果为降级产物；请检查 F0602 的逆矩阵缓存与退化阈值",
      );
      const withinClip = reg.effectiveClipLocal === null
        ? true
        : pointInClipLocalSafe(reg, point);
      if (!withinClip) {
        skipped += 1;
        continue;
      }
      chain.push({ nodeId: id, stage: "bbox-degraded", depth: depthById.get(id) ?? idx });
      // 降级命中：保守给命中（用户在图形附近就期望有反馈）
      aborted = true;
      continue;
    }
    // 精确路径：反投局部坐标 → 裁剪 → 形状
    const inv = reg.inverseMatrix as Mat2D;
    let local: Pt;
    try {
      local = projectToLocal(inv, point);
    } catch {
      bag.push(
        "INVERSE_DEGENERATE",
        `层 ${id} 反投局部坐标时失败`,
        "已跳过该层；请检查逆矩阵元素是否全为有限数",
      );
      skipped += 1;
      continue;
    }
    if (!isFinitePt(local)) {
      // 反投发散（近奇异）：降级为包围盒
      degraded += 1;
      bag.push(
        "INVERSE_DEGENERATE",
        `层 ${id} 反投结果非有限（近奇异变换）`,
        "已降级为世界包围盒判定；请复核 F0602 的退化阈值",
      );
      chain.push({ nodeId: id, stage: "bbox-degraded", depth: depthById.get(id) ?? idx });
      aborted = true;
      continue;
    }
    const withinClip = reg.effectiveClipLocal === null ? true : rectContainsAxisAligned(reg.effectiveClipLocal, local);
    if (!withinClip) {
      const verdict = decidePenetration({
        effectiveVisible: visible,
        effectiveOpacity: opacity,
        passThrough: reg.passThrough,
        maskHole: reg.maskHole,
        isGroup,
        bboxPassed: bboxHit,
        withinClip: false,
        shapeHit: false,
      });
      if (verdict.verdict === "skip") skipped += 1;
      continue;
    }
    preciseTested += 1;
    const shapeHit = shapeContainsLocal(reg.shape as LayerShape, local);
    const verdict = decidePenetration({
      effectiveVisible: visible,
      effectiveOpacity: opacity,
      passThrough: reg.passThrough,
      maskHole: reg.maskHole,
      isGroup,
      bboxPassed: bboxHit,
      withinClip: true,
      shapeHit,
    });
    if (verdict.verdict === "hit") {
      chain.push({ nodeId: id, stage: "precise", depth: depthById.get(id) ?? idx });
      aborted = true;
    } else if (verdict.verdict === "continue" && verdict.record) {
      chain.push({ nodeId: id, stage: "precise", depth: depthById.get(id) ?? idx });
    } else if (verdict.verdict === "skip") {
      skipped += 1;
    }
  }
  const stats: HitStats = {
    visited,
    bboxPassed,
    preciseTested,
    degraded,
    skipped,
    anyDegraded: degraded > 0,
  };
  return hitOk<HitResult>({ point, chain, stats, diagnostics: bag.all() }, bag.all());
}

/**
 * 降级路径下的裁剪判定。
 * 逆矩阵退化时局部坐标不可信，无法把点投到裁剪域的局部系，
 * 只能近似：把裁剪域变换到世界系再判。若裁剪域本身与世界包围盒无交集，
 * 则该层必然在裁剪外——这个短路在绝大多数重叠布局下已经足够。
 */
function pointInClipLocalSafe(reg: HitRegistration, point: Pt): boolean {
  const clip = reg.effectiveClipLocal;
  if (clip === null) return true;
  // 裁剪域四角变换到世界系求并集包围盒
  const corners: Pt[] = [
    applyMat(reg.worldMatrix, { x: clip.x, y: clip.y }),
    applyMat(reg.worldMatrix, { x: clip.x + clip.w, y: clip.y }),
    applyMat(reg.worldMatrix, { x: clip.x + clip.w, y: clip.y + clip.h }),
    applyMat(reg.worldMatrix, { x: clip.x, y: clip.y + clip.h }),
  ];
  if (!corners.every(isFinitePt)) return true; // 变换不可信时保守放行
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const c of corners) {
    if (c.x < minX) minX = c.x;
    if (c.y < minY) minY = c.y;
    if (c.x > maxX) maxX = c.x;
    if (c.y > maxY) maxY = c.y;
  }
  const clipWorld: Rect = { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
  return rectContainsAxisAligned(clipWorld, point);
}

/**
 * 多点触控批量命中（判据五的并发语义）。
 * 逐点独立、**无共享态**：内部为每点新建独立诊断袋与链，
 * 任何一点失败不影响其他点——多点触控的各指事件本就该互不干扰。
 */
export function hitTestPoints(
  tree: HitTree,
  order: HitOrder,
  points: readonly Pt[],
): HitOutcome<readonly HitResult[]> {
  const bag = new HitDiagBag();
  const out: HitResult[] = [];
  for (let i = 0; i < points.length; i += 1) {
    const perPoint = new HitDiagBag();
    const pt = points[i];
    if (pt === undefined) continue;
    const r = hitTestPoint(tree, order, pt, perPoint);
    for (const d of perPoint.all()) bag.push(d.code, d.message, d.hint);
    if (!r.ok) {
      // 单点失败不中断整批（多点触控语义），但失败显性记录
      continue;
    }
    out.push(r.value);
  }
  return hitOk<readonly HitResult[]>(out, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §8 自检（逐条对应判据）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项。 */
export interface HitSelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 恒等矩阵。 */
export const IDENTITY: Mat2D = { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 };

/** 平移矩阵。 */
export function translation(tx: number, ty: number): Mat2D {
  return { a: 1, b: 0, c: 0, d: 1, e: tx, f: ty };
}

/** 构造测试层。 */
function mkReg(over: Partial<HitRegistration> & { readonly nodeId: string; readonly parentId: string | null }): HitRegistration {
  return {
    zKey: 0,
    insertOrder: 0,
    visibility: "visible",
    opacity: 1,
    worldBounds: { x: 0, y: 0, w: 100, h: 100 },
    effectiveClipLocal: null,
    worldMatrix: IDENTITY,
    inverseMatrix: IDENTITY,
    shape: { kind: "rect", rect: { x: 0, y: 0, w: 100, h: 100 } },
    maskHole: false,
    passThrough: false,
    ...over,
  };
}

/** 构造测试树（含根）。 */
function mkTree(regs: readonly HitRegistration[]): HitTree {
  const map = new Map<string, HitRegistration>();
  for (const r of regs) map.set(r.nodeId, r);
  const root = regs[0];
  return { nodes: map, rootId: root === undefined ? "" : root.nodeId };
}

/** 判据一「逆序遍历」自检：最上层优先 + 稳定序。 */
export function selfCheckReverseOrder(): HitSelfCheck[] {
  const out: HitSelfCheck[] = [];
  const bag = new HitDiagBag();
  const regs = [
    mkReg({ nodeId: "root", parentId: null, shape: null, zKey: 0 }),
    mkReg({ nodeId: "under", parentId: "root", zKey: 1, insertOrder: 0, worldBounds: { x: 0, y: 0, w: 200, h: 200 } }),
    mkReg({ nodeId: "top", parentId: "root", zKey: 2, insertOrder: 1, worldBounds: { x: 0, y: 0, w: 200, h: 200 } }),
  ];
  const order = buildHitOrder(mkTree(regs), bag);
  out.push({
    name: "三序同源（先序=Z 序）",
    pass: order !== null && (order.ids[order.ids.length - 1] ?? "") === "top",
    detail: order === null ? "层序构建失败" : `层序=${order.ids.join(">")}，末位应为最上层 top`,
  });
  const r = order === null ? null : hitTestPoint(mkTree(regs), order, { x: 50, y: 50 }, new HitDiagBag());
  const c0 = r !== null && r.ok ? r.value.chain[0] : undefined;
  const first = c0 === undefined ? "" : c0.nodeId;
  out.push({
    name: "逆序遍历（最上层先命中）",
    pass: first === "top",
    detail: `首命中=${first || "无"}，应为 top（z 序最大者）`,
  });
  // 同 z 键稳定性
  const bag2 = new HitDiagBag();
  const stableRegs = [
    mkReg({ nodeId: "root", parentId: null, shape: null }),
    mkReg({ nodeId: "a", parentId: "root", zKey: 5, insertOrder: 0 }),
    mkReg({ nodeId: "b", parentId: "root", zKey: 5, insertOrder: 1 }),
  ];
  const order2 = buildHitOrder(mkTree(stableRegs), bag2);
  const ids2 = order2 === null ? [] : order2.ids;
  out.push({
    name: "同 z 键按插入序稳定",
    pass: ids2[1] === "a" && ids2[2] === "b",
    detail: `同键层序=${ids2.join(">")}，应为 a 先 b 后（F0606 稳定性契约）`,
  });
  return out;
}

/** 判据二「两级判定」自检：预筛剪枝 + 精确形状。 */
export function selfCheckTwoStage(): HitSelfCheck[] {
  const out: HitSelfCheck[] = [];
  const bag = new HitDiagBag();
  const regs = [
    mkReg({ nodeId: "root", parentId: null, shape: null }),
    mkReg({ nodeId: "ellipse", parentId: "root", shape: { kind: "ellipse", cx: 50, cy: 50, rx: 40, ry: 20 }, worldBounds: { x: 10, y: 30, w: 80, h: 40 } }),
  ];
  const tree = mkTree(regs);
  const order = buildHitOrder(tree, bag);
  if (order === null) {
    return [{ name: "两级判定", pass: false, detail: "层序构建失败" }];
  }
  const inside = hitTestPoint(tree, order, { x: 50, y: 50 }, new HitDiagBag());
  const outside = hitTestPoint(tree, order, { x: 50, y: 90 }, new HitDiagBag());
  const outChain = outside !== null && outside.ok ? outside.value.chain.length : -1;
  out.push({
    name: "精确形状判定（椭圆内命中）",
    pass: inside !== null && inside.ok && inside.value.chain.length === 1,
    detail: `椭圆内命中链长度=${inside !== null && inside.ok ? inside.value.chain.length : "n/a"}`,
  });
  out.push({
    name: "包围盒预筛剪枝（盒外即出局）",
    pass: outChain === 0,
    detail: `椭圆包围盒外命中链长度=${outChain}，应为 0`,
  });
  // 预筛确实比精确便宜：盒外时 preciseTested 应为 0
  const stats = outside !== null && outside.ok ? outside.value.stats : null;
  out.push({
    name: "盒外不进精确判定（O(1) 预筛成立）",
    pass: stats !== null && stats.preciseTested === 0 && stats.bboxPassed === 0,
    detail: stats === null ? "无统计" : `预筛通过=${stats.bboxPassed} 精确判定=${stats.preciseTested}`,
  });
  return out;
}

/** 判据三「三一致性」自检：裁剪 / 可见性 / 不透明度。 */
export function selfCheckConsistency(): HitSelfCheck[] {
  const out: HitSelfCheck[] = [];
  const mkCase = (reg: HitRegistration): { readonly tree: HitTree; readonly order: HitOrder } => {
    const bag = new HitDiagBag();
    const regs = [mkReg({ nodeId: "root", parentId: null, shape: null }), reg];
    const tree = mkTree(regs);
    const order = buildHitOrder(tree, bag);
    return { tree, order: order as HitOrder };
  };
  // 裁剪外不可命中
  const clipped = mkCase(
    mkReg({
      nodeId: "clip",
      parentId: "root",
      shape: { kind: "rect", rect: { x: 0, y: 0, w: 100, h: 100 } },
      effectiveClipLocal: { x: 0, y: 0, w: 50, h: 50 },
    }),
  );
  const clipOut = hitTestPoint(clipped.tree, clipped.order, { x: 80, y: 80 }, new HitDiagBag());
  out.push({
    name: "一致性一：裁剪外不可命中（F0604）",
    pass: clipOut !== null && clipOut.ok && clipOut.value.chain.length === 0,
    detail: `裁剪外命中链=${clipOut !== null && clipOut.ok ? clipOut.value.chain.length : "n/a"}，应为 0`,
  });
  // 隐藏层不可命中
  const hidden = mkCase(
    mkReg({ nodeId: "hid", parentId: "root", visibility: "hidden", shape: { kind: "rect", rect: { x: 0, y: 0, w: 100, h: 100 } } }),
  );
  const hiddenHit = hitTestPoint(hidden.tree, hidden.order, { x: 50, y: 50 }, new HitDiagBag());
  out.push({
    name: "一致性二：隐藏层不可命中（F0607）",
    pass: hiddenHit !== null && hiddenHit.ok && hiddenHit.value.chain.length === 0,
    detail: `隐藏层命中链=${hiddenHit !== null && hiddenHit.ok ? hiddenHit.value.chain.length : "n/a"}，应为 0`,
  });
  // 父隐藏 → 子树全隐藏
  const childOfHidden = mkCase(
    mkReg({ nodeId: "kid", parentId: "root", shape: { kind: "rect", rect: { x: 0, y: 0, w: 100, h: 100 } } }),
  );
  const bagH = new HitDiagBag();
  const treeH: HitTree = {
    nodes: new Map<string, HitRegistration>([
      ["root", mkReg({ nodeId: "root", parentId: null, shape: null, visibility: "hidden" })],
      ["kid", childOfHidden.tree.nodes.get("kid") as HitRegistration],
    ]),
    rootId: "root",
  };
  const orderH = buildHitOrder(treeH, bagH);
  const kidHit = orderH === null ? null : hitTestPoint(treeH, orderH, { x: 50, y: 50 }, new HitDiagBag());
  out.push({
    name: "父子传播：父隐藏则子树全隐藏",
    pass: orderH !== null && orderH.effectiveVisible.get("kid") === false &&
      kidHit !== null && kidHit.ok && kidHit.value.chain.length === 0,
    detail: `子层有效可见=${orderH === null ? "n/a" : String(orderH.effectiveVisible.get("kid"))}，应为 false`,
  });
  // alpha 为 0 既不绘制也不遮挡
  const zeroAlpha = mkCase(
    mkReg({ nodeId: "zero", parentId: "root", opacity: 0, shape: { kind: "rect", rect: { x: 0, y: 0, w: 100, h: 100 } } }),
  );
  const zeroHit = hitTestPoint(zeroAlpha.tree, zeroAlpha.order, { x: 50, y: 50 }, new HitDiagBag());
  out.push({
    name: "一致性三：alpha=0 既不命中也不遮挡",
    pass: zeroHit !== null && zeroHit.ok && zeroHit.value.chain.length === 0,
    detail: `alpha=0 命中链=${zeroHit !== null && zeroHit.ok ? zeroHit.value.chain.length : "n/a"}，应为 0（F0603 命中豁免）`,
  });
  // alpha=0 在上层时不遮挡下层
  const bagZ = new HitDiagBag();
  const treeZ: HitTree = {
    nodes: new Map<string, HitRegistration>([
      ["root", mkReg({ nodeId: "root", parentId: null, shape: null })],
      ["top", mkReg({ nodeId: "top", parentId: "root", opacity: 0, zKey: 9, shape: { kind: "rect", rect: { x: 0, y: 0, w: 100, h: 100 } } })],
      ["under", mkReg({ nodeId: "under", parentId: "root", zKey: 1, shape: { kind: "rect", rect: { x: 0, y: 0, w: 100, h: 100 } } })],
    ]),
    rootId: "root",
  };
  const orderZ = buildHitOrder(treeZ, bagZ);
  const zHit = orderZ === null ? null : hitTestPoint(treeZ, orderZ, { x: 50, y: 50 }, new HitDiagBag());
  out.push({
    name: "alpha=0 上层不遮挡下层",
    pass: zHit !== null && zHit.ok && zHit.value.chain.length === 1 &&
      (zHit.value.chain[0] === undefined ? "" : zHit.value.chain[0].nodeId) === "under",
    detail: zHit !== null && zHit.ok ? `命中=${zHit.value.chain.map((c) => c.nodeId).join(",") || "无"}，应为 under` : "n/a",
  });
  return out;
}

/** 判据四「穿透规则」自检。 */
export function selfCheckPenetration(): HitSelfCheck[] {
  const out: HitSelfCheck[] = [];
  // 规则表纯函数：透明区域穿透
  const transparent = decidePenetration({
    effectiveVisible: true, effectiveOpacity: 1, passThrough: false, maskHole: false,
    isGroup: false, bboxPassed: true, withinClip: true, shapeHit: false,
  });
  out.push({
    name: "透明区域穿透到下层",
    pass: transparent.verdict === "continue" && transparent.record,
    detail: `裁决=${transparent.verdict}，应为 continue 且记录`,
  });
  // 蒙版镂空穿透
  const hole = decidePenetration({
    effectiveVisible: true, effectiveOpacity: 1, passThrough: false, maskHole: true,
    isGroup: false, bboxPassed: true, withinClip: true, shapeHit: true,
  });
  out.push({
    name: "蒙版镂空穿透（细化归 F0654）",
    pass: hole.verdict === "continue" && hole.reason.includes("F0654"),
    detail: `裁决=${hole.verdict}，理由须指明 F0654`,
  });
  // 不可见跳过（不记录、不终止）
  const invis = decidePenetration({
    effectiveVisible: false, effectiveOpacity: 1, passThrough: false, maskHole: false,
    isGroup: false, bboxPassed: true, withinClip: true, shapeHit: true,
  });
  out.push({
    name: "不可见层跳过且不记录",
    pass: invis.verdict === "skip" && !invis.record,
    detail: `裁决=${invis.verdict} 记录=${invis.record}`,
  });
  // 命中终止
  const hit = decidePenetration({
    effectiveVisible: true, effectiveOpacity: 1, passThrough: false, maskHole: false,
    isGroup: false, bboxPassed: true, withinClip: true, shapeHit: true,
  });
  out.push({
    name: "精确命中即终止",
    pass: hit.verdict === "hit" && hit.record,
    detail: `裁决=${hit.verdict}`,
  });
  // 端到端：上层镂空 + 下层实体 → 命中下层
  const bag = new HitDiagBag();
  const tree: HitTree = {
    nodes: new Map<string, HitRegistration>([
      ["root", mkReg({ nodeId: "root", parentId: null, shape: null })],
      ["frame", mkReg({ nodeId: "frame", parentId: "root", zKey: 9, maskHole: true, shape: { kind: "rect", rect: { x: 0, y: 0, w: 100, h: 100 } } })],
      ["solid", mkReg({ nodeId: "solid", parentId: "root", zKey: 1, shape: { kind: "rect", rect: { x: 0, y: 0, w: 100, h: 100 } } })],
    ]),
    rootId: "root",
  };
  const order = buildHitOrder(tree, bag);
  const r = order === null ? null : hitTestPoint(tree, order, { x: 50, y: 50 }, new HitDiagBag());
  out.push({
    name: "端到端：镂空上层穿透命中下层",
    pass: r !== null && r.ok && r.value.chain.length === 2 &&
      (r.value.chain[1] === undefined ? "" : r.value.chain[1].nodeId) === "solid",
    detail: r !== null && r.ok ? `命中链=${r.value.chain.map((c) => c.nodeId).join(">")}` : "n/a",
  });
  return out;
}

/** 判据五「降级与并发」自检。 */
export function selfCheckDegradation(): HitSelfCheck[] {
  const out: HitSelfCheck[] = [];
  // 逆矩阵退化 → 降级包围盒 + 登记
  const bag = new HitDiagBag();
  const tree: HitTree = {
    nodes: new Map<string, HitRegistration>([
      ["root", mkReg({ nodeId: "root", parentId: null, shape: null })],
      ["singular", mkReg({ nodeId: "singular", parentId: "root", inverseMatrix: null, shape: { kind: "ellipse", cx: 50, cy: 50, rx: 10, ry: 10 }, worldBounds: { x: 40, y: 40, w: 20, h: 20 } })],
    ]),
    rootId: "root",
  };
  const order = buildHitOrder(tree, bag);
  const r = order === null ? null : hitTestPoint(tree, order, { x: 50, y: 50 }, bag);
  const degradedRec = r !== null && r.ok ? r.value.chain.find((c) => c.stage === "bbox-degraded") : undefined;
  out.push({
    name: "逆矩阵退化降级包围盒判定",
    pass: degradedRec !== undefined,
    detail: degradedRec === undefined ? "未产生降级命中记录" : `降级命中=${degradedRec.nodeId}`,
  });
  out.push({
    name: "降级显性登记（零静默）",
    pass: bag.has("INVERSE_DEGENERATE") || (r !== null && r.ok && r.value.stats.anyDegraded),
    detail: `诊断含 INVERSE_DEGENERATE=${bag.has("INVERSE_DEGENERATE")}`,
  });
  // 非有限点拒绝
  const bad = hitTestPoint(tree, order as HitOrder, { x: NaN, y: 0 }, new HitDiagBag());
  out.push({
    name: "非有限坐标拒绝而非静默穿透",
    pass: !bad.ok && bad.code === "NON_FINITE_POINT",
    detail: bad.ok ? "非法点被静默接受" : `拒因=${bad.code}`,
  });
  // 多点触控：逐点独立，单点非法不拖累整批
  const multi = hitTestPoints(tree, order as HitOrder, [{ x: 50, y: 50 }, { x: NaN, y: 1 }, { x: 1, y: 1 }]);
  out.push({
    name: "多点触控逐点独立",
    pass: multi.ok && multi.value.length === 2,
    detail: multi.ok ? `有效点数=${multi.value.length}，应为 2（非法点被单独剔除）` : "批次失败",
  });
  // 层数超阈不截断
  const manyRegs: HitRegistration[] = [mkReg({ nodeId: "root", parentId: null, shape: null })];
  for (let i = 0; i < LAYER_COUNT_THRESHOLD + 5; i += 1) {
    manyRegs.push(mkReg({
      nodeId: `n${i}`,
      parentId: "root",
      zKey: i,
      worldBounds: { x: 1000 + i, y: 1000, w: 10, h: 10 },
      shape: { kind: "rect", rect: { x: 1000 + i, y: 1000, w: 10, h: 10 } },
    }));
  }
  const bagBig = new HitDiagBag();
  const treeBig = mkTree(manyRegs);
  const orderBig = buildHitOrder(treeBig, bagBig);
  const rBig = orderBig === null ? null : hitTestPoint(treeBig, orderBig, { x: 0, y: 0 }, bagBig);
  out.push({
    name: "层数超阈登记但不截断",
    pass: bagBig.has("LAYER_COUNT_EXCEEDS_THRESHOLD") && rBig !== null && rBig.ok &&
      rBig.value.stats.visited === LAYER_COUNT_THRESHOLD + 6,
    detail: rBig !== null && rBig.ok ? `实际遍历=${rBig.value.stats.visited}，应为 ${LAYER_COUNT_THRESHOLD + 6}` : "n/a",
  });
  // 树结构非法（成环）拒绝启动
  const bagCycle = new HitDiagBag();
  const cycleTree: HitTree = {
    nodes: new Map<string, HitRegistration>([
      ["a", mkReg({ nodeId: "a", parentId: "b", shape: null })],
      ["b", mkReg({ nodeId: "b", parentId: "a", shape: null })],
    ]),
    rootId: "a",
  };
  const cycleOrder = buildHitOrder(cycleTree, bagCycle);
  out.push({
    name: "成环树拒绝启动（fail-fast）",
    pass: cycleOrder === null && bagCycle.has("TREE_INVARIANT_VIOLATED"),
    detail: `构建结果=${cycleOrder === null ? "拒绝" : "接受"}（应拒绝）`,
  });
  return out;
}

/** 几何基元自检（矩阵往返、形状判定、包围盒）。 */
export function selfCheckGeometry(): HitSelfCheck[] {
  const out: HitSelfCheck[] = [];
  const m = translation(37, -11);
  const p = { x: 5, y: 9 };
  const fwd = applyMat(m, p);
  const back = applyInverseMat(m, fwd);
  out.push({
    name: "矩阵正逆往返一致",
    pass: Math.abs(back.x - p.x) < 1e-9 && Math.abs(back.y - p.y) < 1e-9,
    detail: `往返=(${back.x},${back.y})，应为 (${p.x},${p.y})`,
  });
  out.push({
    name: "奇异矩阵被拦截（判据二前置）",
    pass: isDegenerate({ a: 0, b: 0, c: 0, d: 0, e: 5, f: 5 }),
    detail: "行列式为 0 的矩阵应判退化",
  });
  const rr = roundRectContains({ x: 0, y: 0, w: 100, h: 100 }, [20, 20, 20, 20], { x: 5, y: 5 });
  const rrOut = roundRectContains({ x: 0, y: 0, w: 100, h: 100 }, [30, 30, 30, 30], { x: 1, y: 1 });
  out.push({
    name: "圆角矩形缺角判定",
    pass: rr === false && rrOut === false,
    detail: `缺角内命中=${rr}（应 false），极角命中=${rrOut}（应 false）`,
  });
  const poly = polygonContains(
    [{ x: 0, y: 0 }, { x: 100, y: 0 }, { x: 100, y: 100 }, { x: 0, y: 100 }],
    { x: 50, y: 50 },
  );
  const polyOut = polygonContains(
    [{ x: 0, y: 0 }, { x: 100, y: 0 }, { x: 100, y: 100 }, { x: 0, y: 100 }],
    { x: 150, y: 50 },
  );
  out.push({
    name: "多边形射线法（含边界）",
    pass: poly === true && polyOut === false,
    detail: `内=${poly} 外=${polyOut}`,
  });
  const inter = intersectRect({ x: 0, y: 0, w: 10, h: 10 }, { x: 5, y: 5, w: 10, h: 10 });
  out.push({
    name: "矩形求交（F0604 交集级联语义）",
    pass: inter !== null && inter.w === 5 && inter.h === 5,
    detail: inter === null ? "求交为 null" : `交=${inter.w}x${inter.h}，应为 5x5`,
  });
  return out;
}

/** 全量自检入口。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly HitSelfCheck[]>>;
  readonly allPass: boolean;
  readonly total: number;
  readonly failed: readonly string[];
} {
  const groups = {
    geometry: selfCheckGeometry(),
    reverseOrder: selfCheckReverseOrder(),
    twoStage: selfCheckTwoStage(),
    consistency: selfCheckConsistency(),
    penetration: selfCheckPenetration(),
    degradation: selfCheckDegradation(),
  };
  const failed: string[] = [];
  let total = 0;
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      total += 1;
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, total, failed };
}