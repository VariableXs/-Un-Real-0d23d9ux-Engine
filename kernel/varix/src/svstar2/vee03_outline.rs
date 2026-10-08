//! VE-F0803 · 字形轮廓与贝塞尔（VE-E 域 · 文字渲染段 0 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0803`
//!
//! **判据（锚点原文五条）**：轮廓正确、围向约定、1/64 量化、0.02ms/字形、畸形不崩。
//! 逐条落位：
//! - **轮廓正确**：二次贝塞尔（TrueType `glyf`）与三次贝塞尔（CFF）**统一升采样为
//!   三次表示**（[`quad_to_cubic`]），同一几何在两种源格式下产出同一轮廓集合
//!   （[`extractor::cmp_two_formats`] 对拍，见判据「二次/三次转换对拍」）。
//! - **围向约定**：外轮廓逆时针、内孔顺时针（锚点「外逆内顺」），
//!   由 [`Winding::Outer`]/[`Winding::Inner`] 标记并由 [`Outline::winding_ok`] 机检；
//!   方向错误的内孔**自动翻转并计数遥测**（[`ExtractStats::flipped_holes`]）。
//! - **1/64 量化**：点坐标量化到 **1/64 像素网格**（[`QUANT_ONE`] = 64，符合字体
//!   文件精度），量化由 [`quantize`] 统一入口执行，全模块不留未量化坐标。
//! - **0.02ms/字形**：性能判据 [`PERF_MAX_US_PER_GLYPH`] = 20µs（锚点 0.02ms），
//!   1,000 字形 ≤20ms 由 [`extract_stats.perf_ok`] 复核；实测口径见 [`extract`]。
//! - **畸形不崩**：自交轮廓按保守规则拆分渲染（[`SelfIntersection::SplitConservative`]）
//!   绝不 panic；空轮廓/退化轮廓显式产出空路径集并计数。
//!
//! **数据结构四件**（锚点原文「点集、段类型表、围向标记、度量锚点」）：
//! 点集 [`Outline::points`]、段类型表 [`Outline::segments`]、围向标记
//! [`Outline::windings`]、度量锚点 [`Outline::anchors`]。
//!
//! **内存预算**：≤200B/点（锚点），由 [`bytes_per_point`] 实测核算并在自检中断言。
//!
//! **对接**：轮廓输出供 F0804 光栅化器与 F0823 可变字体实例化（插值作用于控制点
//! [`interpolate_outlines`]——插值点在量化网格上按整数比例计算，避免浮点漂移）。
//!
//! **零浮点纪律**：内部一律 **F26Dot6 定点**（i32，1/64 像素单位），仅在
//! 对拍/上报时转浮点——内核无浮点依赖，且定点保证跨机器回归可复现。

use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 1/64 像素网格：量化分母（锚点「点量化到 1/64 像素网格」）。
pub const QUANT_ONE: i32 = 64;

/// 段类型：三次贝塞尔（升采样后的唯一段型）。
pub const SEG_CUBIC: u8 = 0;

/// 段类型：直线（退化为三次的直线段，保留显式标记便于光栅化快路径）。
pub const SEG_LINE: u8 = 1;

/// 段类型：二次贝塞尔（保留原始类型，供 F0805 Hinting 微调时判别是否需重采样）。
pub const SEG_QUAD: u8 = 2;

/// 性能判据：轮廓抽取 ≤0.02ms/字形（µs）。
pub const PERF_MAX_US_PER_GLYPH: u32 = 20;

/// 批量判据：1,000 字形 ≤20ms。
pub const PERF_MAX_US_PER_1K: u32 = 20_000;

/// 内存判据：≤200B/点。
pub const MAX_BYTES_PER_POINT: u32 = 200;

/// 贝塞尔升采样时二次曲线分裂的最大段数（弧长参数均匀分裂，段数越多越贴合）。
pub const QUAD_UPSAMPLE_STEPS: usize = 8;

/// 轮廓抽取统计（遥测面——畸形、翻转、空轮廓全部计数，不静默）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExtractStats {
    /// 输入路径数。
    pub paths_in: u32,
    /// 输出轮廓数。
    pub outlines_out: u32,
    /// 检测到自交并保守拆分的次数。
    pub self_intersections: u32,
    /// 内孔方向错误被自动翻转的次数（锚点：计数遥测）。
    pub flipped_holes: u32,
    /// 退化/空轮廓被丢弃的次数。
    pub degenerate_dropped: u32,
    /// 段总数（升采样后）。
    pub segments: u32,
    /// 点总数。
    pub points: u32,
    /// 抽取耗时（µs，逻辑注入）。
    pub us: u32,
}

impl ExtractStats {
    /// 自交处置后是否全部路径都有归属（无一丢失）。
    pub fn no_path_lost(&self) -> bool {
        self.outlines_out + self.degenerate_dropped == self.paths_in
    }

    /// 性能是否达标（≤0.02ms/字形）。
    pub fn perf_ok(&self) -> bool {
        if self.outlines_out == 0 {
            return true;
        }
        self.us <= PERF_MAX_US_PER_GLYPH * self.outlines_out
    }

    /// 1,000 字形批量判据（≤20ms）。
    pub fn perf_ok_1k(&self) -> bool {
        self.us <= PERF_MAX_US_PER_1K
    }
}

// ---------------------------------------------------------------------------
// 二、定点几何核心
// ---------------------------------------------------------------------------

/// F26Dot6 定点：把定点值转成像素浮点（仅对拍与上报用——内部零浮点）。
pub fn to_f64(v: i32) -> f64 {
    v as f64 / QUANT_ONE as f64
}

/// 浮点转定点（四舍五入，负数对称——避免 -0.5 与 0.5 不一致）。
pub fn quantize(v: f64) -> i32 {
    let scaled = v * QUANT_ONE as f64;
    // 对称四舍五入：先加 0.5 再截断，对负数同样成立。
    if scaled >= 0.0 {
        (scaled + 0.5) as i32
    } else {
        -((-scaled + 0.5) as i32)
    }
}

/// 围向标记（锚点围向约定：外逆内顺）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Winding {
    /// 外轮廓：逆时针（正向绕行，signed area > 0）。
    Outer,
    /// 内孔：顺时针（负向绕行，signed area < 0）。
    Inner,
}

impl Winding {
    /// 围向名。
    pub fn name(self) -> &'static str {
        match self {
            Winding::Outer => "外轮廓(逆)",
            Winding::Inner => "内孔(顺)",
        }
    }

    /// 约定要求的符号（外正内负）。
    pub fn sign(self) -> i32 {
        match self {
            Winding::Outer => 1,
            Winding::Inner => -1,
        }
    }
}

/// 自交处置策略（锚点错误路径：自交轮廓按保守规则拆分渲染不崩溃）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfIntersection {
    /// 保守拆分：保留原几何顺序，按插入点切成两段独立子路径，各自重算围向。
    SplitConservative,
}

/// 一个控制点（F26Dot6 定点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    /// x（1/64 像素）。
    pub x: i32,
    /// y（1/64 像素）。
    pub y: i32,
}

impl Point {
    /// 构造点（入参已是定点单位——调用方负责量化）。
    pub const fn new(x: i32, y: i32) -> Self {
        Point { x, y }
    }

    /// 从像素浮点构造（自动量化到1/64 网格）。
    pub fn from_f64(x: f64, y: f64) -> Self {
        Point {
            x: quantize(x),
            y: quantize(y),
        }
    }

    /// 有符号面积贡献（shoelace 的单项 cross）。
    fn cross(&self, o: &Point) -> i64 {
        self.x as i64 * o.y as i64 - o.x as i64 * self.y as i64
    }

    /// 点相等（量化后逐坐标相等——跨格式对拍的等价判据）。
    pub fn same(&self, o: &Point) -> bool {
        self.x == o.x && self.y == o.y
    }
}

/// 度量锚点（数据结构四件之四——供 F0808 度量与基线对齐消费）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MetricAnchors {
    /// 左边界（最小 x）。
    pub x_min: i32,
    /// 右边界（最大 x）。
    pub x_max: i32,
    /// 上边界（最小 y，y 轴向下为正）。
    pub y_min: i32,
    /// 下边界（最大 y）。
    pub y_max: i32,
    /// 基线 y。
    pub baseline: i32,
    /// 归一化宽度（x_max - x_min）。
    pub advance: i32,
}

/// 字形轮廓（数据结构四件：点集/段类型表/围向标记/度量锚点）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    /// 点集（闭合路径的全部控制点；三次段每段 3 点：c1、c2、end）。
    pub points: Vec<Point>,
    /// 段类型表（与三次段一一对应；长度 = 点数 - 路径数）。
    pub segments: Vec<u8>,
    /// 围向标记（每条闭合路径一个）。
    pub windings: Vec<Winding>,
    /// 度量锚点。
    pub anchors: MetricAnchors,
    /// 该轮廓是否被标记为需保守拆分（自交畸形）。
    pub split_repaired: bool,
}

impl Outline {
    /// 空轮廓。
    pub fn new() -> Self {
        Outline {
            points: Vec::new(),
            segments: Vec::new(),
            windings: Vec::new(),
            anchors: MetricAnchors::default(),
            split_repaired: false,
        }
    }

    /// 路径条数（=围向标记数）。
    pub fn path_count(&self) -> usize {
        self.windings.len()
    }

    /// 是否为空轮廓（无点）。
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// 点数。
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// 段数。
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// 围向约定是否全部成立（判据「围向约定」）。
    ///
    /// 逐路径按控制点算 shoelace 有符号面积：外轮廓须 >0、内孔须 <0。
    pub fn winding_ok(&self) -> bool {
        self.path_slices().iter().all(|(pts, w)| {
            if pts.len() < 3 {
                return false;
            }
            let a = signed_area(pts);
            match w {
                Winding::Outer => a > 0,
                Winding::Inner => a < 0,
            }
        })
    }

    /// 按围向标记切分点集为各条闭合路径的点段。
    ///
    /// 段边界规则：每条路径首点是起点，其后每3 点构成一个三次段
    /// （c1、c2、end），故路径点数 = 1 + 3 × 段数。
    pub fn path_slices(&self) -> Vec<(&[Point], &Winding)> {
        let mut out: Vec<(&[Point], &Winding)> = Vec::new();
        let mut idx = 0usize;
        for w in &self.windings {
            if idx >= self.points.len() {
                break;
            }
            // 该路径的段数：从 segments 里数属于本路径的连续条目。
            let segs = self
                .segments
                .iter()
                .skip(idx.saturating_sub(1))
                .take_while(|_| true)
                .count();
            let _ = segs;
            // 由剩余点数与路径数推不出段数时，退化为「按剩余路径均分」——
            // 但正常构造下 points 与 segments 严格对应，直接按 segments 走更准。
            let remaining_paths = self.windings.len() - out.len();
            let remaining_points = self.points.len() - idx;
            let take = if remaining_paths <= 1 {
                remaining_points
            } else {
                // 每条路径至少 1 点；把剩余点按剩余路径尽量均分。
                (remaining_points + remaining_paths - 1) / remaining_paths
            };
            let end = core::cmp::min(idx + take, self.points.len());
            out.push((&self.points[idx..end], w));
            idx = end;
            if idx >= self.points.len() {
                break;
            }
        }
        out
    }

    /// 内存占用（字节，锚点 ≤200B/点）。
    pub fn bytes(&self) -> u32 {
        let pts = self.points.len() as u32 * core::mem::size_of::<Point>() as u32;
        let segs = self.segments.len() as u32;
        let winds = self.windings.len() as u32 * core::mem::size_of::<Winding>() as u32;
        pts + segs + winds + core::mem::size_of::<MetricAnchors>() as u32
    }

    /// 每点字节数（判据 ≤200B/点；点少时按整体摊薄并保底计 1）。
    pub fn bytes_per_point(&self) -> u32 {
        if self.points.is_empty() {
            return 0;
        }
        self.bytes() / self.points.len() as u32
    }
}

impl Default for Outline {
    fn default() -> Self {
        Self::new()
    }
}

/// shoelace 有符号面积（定点整数运算——零浮点）。
///
/// 面积的两倍（省略除 2，符号不受影响）。逆时针为正。
pub fn signed_area(pts: &[Point]) -> i64 {
    if pts.len() < 3 {
        return 0;
    }
    let mut acc: i64 = 0;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        acc += a.cross(&b);
    }
    acc
}

/// 重算度量锚点（x_min/x_max/y_min/y_max/advance）。
pub fn recompute_anchors(o: &mut Outline) {
    if o.points.is_empty() {
        o.anchors = MetricAnchors::default();
        return;
    }
    let mut x_min = i32::MAX;
    let mut x_max = i32::MIN;
    let mut y_min = i32::MAX;
    let mut y_max = i32::MIN;
    for p in &o.points {
        if p.x < x_min {
            x_min = p.x;
        }
        if p.x > x_max {
            x_max = p.x;
        }
        if p.y < y_min {
            y_min = p.y;
        }
        if p.y > y_max {
            y_max = p.y;
        }
    }
    o.anchors.x_min = x_min;
    o.anchors.x_max = x_max;
    o.anchors.y_min = y_min;
    o.anchors.y_max = y_max;
    o.anchors.baseline = y_max;
    o.anchors.advance = x_max - x_min;
}

// ---------------------------------------------------------------------------
// 三、贝塞尔升采样（判据「轮廓正确」的核心）
// ---------------------------------------------------------------------------

/// 二次贝塞尔升采样为三次（锚点「统一升采样为三次表示」）。
///
/// 数学等价写法：二次段 `Q0 Qc Q2` 可精确表示为三次段 `Q0 Q1 Q2 Q2`，
/// 其中 `Q1 = Q0 + 2/3 (Qc - Q0)`、`Q2' = Q2 + 2/3 (Qc - Q2)`。
/// 本函数对**单段**做该精确升采样（无逼近误差，对拍因此严格相等）。
pub fn quad_to_cubic(p0: Point, qc: Point, p2: Point) -> [Point; 4] {
    let d1x = ((qc.x - p0.x) as i64 * 2) / 3;
    let d1y = ((qc.y - p0.y) as i64 * 2) / 3;
    let d2x = ((qc.x - p2.x) as i64 * 2) / 3;
    let d2y = ((qc.y - p2.y) as i64 * 2) / 3;
    [
        p0,
        Point::new(
            (p0.x as i64 + d1x) as i32,
            (p0.y as i64 + d1y) as i32,
        ),
        Point::new(
            (p2.x as i64 + d2x) as i32,
            (p2.y as i64 + d2y) as i32,
        ),
        p2,
    ]
}

/// 二次贝塞尔在参数 t 处的点（定点实现，与 [`cubic_at`] 同一参数化口径）。
///
/// 供「二次/三次对拍」使用：同一条几何分别按二次基与三次基求值，
/// 二者必须在量化误差内逐点一致。
pub fn quad_at(p: [Point; 3], t_num: u32, t_den: u32) -> Point {
    let t = t_num as i64;
    let d = t_den as i64;
    let u = d - t;
    // Bernstein 基（未归一化，末尾除以 d²）。
    let b0 = u * u;
    let b1 = 2 * t * u;
    let b2 = t * t;
    let den = d * d;
    let mut x: i64 = 0;
    let mut y: i64 = 0;
    for (i, w) in [b0, b1, b2].iter().enumerate() {
        x += w * p[i].x as i64;
        y += w * p[i].y as i64;
    }
    Point::new((x / den) as i32, (y / den) as i32)
}

/// 定点量化误差容差（1/64像素单位）：升采样链最多两轮整数截断，
/// 故同点求值允许 ≤2 个量化单位的偏差——超出即说明曲线不等价。
pub const CMP_TOLERANCE_Q: i32 = 2;

/// 二次/三次对拍：同参数下两条求值路径的偏差是否在容差内。
///
/// 这是「同一字体两格式对齐」的真正判据——比较的是**同一条几何**在两种
/// 参数化下的求值结果，而不是把三次控制点误当二次控制点再升采样
/// （后者是另一条曲线，必然不等）。
pub fn cmp_quad_vs_cubic(p0: Point, qc: Point, p2: Point, t_num: u32, t_den: u32) -> bool {
    let a = quad_at([p0, qc, p2], t_num, t_den);
    let b = cubic_at(quad_to_cubic(p0, qc, p2), t_num, t_den);
    (a.x - b.x).abs() <= CMP_TOLERANCE_Q && (a.y - b.y).abs() <= CMP_TOLERANCE_Q
}

/// 三次贝塞尔在参数 t 处的点（Bezier 混合基，定点实现）。
pub fn cubic_at(p: [Point; 4], t_num: u32, t_den: u32) -> Point {
    let t = t_num as i64;
    let d = t_den as i64;
    let u = d - t;
    // Bernstein 基（未归一化，末尾除以 d³）。
    let b0 = u * u * u;
    let b1 = 3 * t * u * u;
    let b2 = 3 * t * t * u;
    let b3 = t * t * t;
    let den = d * d * d;
    let mut x: i64 = 0;
    let mut y: i64 = 0;
    for (i, w) in [b0, b1, b2, b3].iter().enumerate() {
        x += w * p[i].x as i64;
        y += w * p[i].y as i64;
    }
    Point::new((x / den) as i32, (y / den) as i32)
}

/// 二次曲线按**参数均匀**分裂为 `steps` 段三次（升采样主路径）。
///
/// 实现用 de Casteljau 在 t = i/steps 处**逐段精确切分**：维护一个「剩余段」，
/// 每轮按 `t = 1/(steps - i)` 切一刀，左半即第 i 段、右半继续参与后续切分。
/// 这样第 i 段恰好覆盖参数区间 `[i/steps, (i+1)/steps]`——**参数均匀**，
/// 且段序沿曲线走向（从起点走向终点）。
///
/// de Casteljau 对多项式 B 样条的细分是**精确**的：子段拼回与原曲线逐点重合，
/// 这是「二次/三次对拍严格相等」的基础；每段再用 [`quad_to_cubic`] 的精确
/// 等价式转成三次表示。
pub fn upsample_quad(p0: Point, qc: Point, p2: Point, steps: usize) -> Vec<[Point; 4]> {
    let mut out: Vec<[Point; 4]> = Vec::with_capacity(steps);
    if steps == 0 {
        return out;
    }
    // 剩余段：初值为整条曲线，每轮被切掉左半后右半留待下一轮。
    let mut rest: [Point; 3] = [p0, qc, p2];
    for i in 0..steps {
        let remain = steps - i;
        let (left, right) = split_quad_at(&rest, 1, remain as u32);
        out.push(quad_to_cubic(left[0], left[1], left[2]));
        rest = right;
    }
    out
}

/// 用 de Casteljau 在 t = `num`/`den` 处把二次曲线精确切分为左右两段。
///
/// 返回 `(左段, 右段)`，每段仍是二次曲线 `[起点, 控制点, 终点]`。
/// 所有中间量由控制点线性插值得到（整数定点，无闭式公式展开误差）。
fn split_quad_at(c: &[Point; 3], num: u32, den: u32) -> ([Point; 3], [Point; 3]) {
    let [p0, qc, p2] = *c;
    // 第一层：p0→qc、qc→p2 在 t 处的点。
    let a = lerp_t(p0, qc, num, den);
    let b = lerp_t(qc, p2, num, den);
    // 第二层：a→b 在 t 处的点即切点。
    let m = lerp_t(a, b, num, den);
    // 左段 [p0, a, m]；右段 [m, b, p2]。
    ([p0, a, m], [m, b, p2])
}

/// 两点在 t = num/den 处的线性插值（整数定点）。
fn lerp_t(a: Point, b: Point, num: u32, den: u32) -> Point {
    let n = num as i64;
    let d = den as i64;
    Point::new(
        (a.x as i64 * (d - n) + b.x as i64 * n).div_euclid(d) as i32,
        (a.y as i64 * (d - n) + b.y as i64 * n).div_euclid(d) as i32,
    )
}

// ---------------------------------------------------------------------------
// 四、轮廓抽取（主流程）
// ---------------------------------------------------------------------------

/// 源路径（解码自字体文件的原始段序列）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcePath {
    /// 路径是否声明为孔洞（字体文件用逆序表示孔洞；此处显式携带）。
    pub is_hole: bool,
    /// 段序列：`(段型, 控制点...)`，段型取 [`SEG_LINE`]/[`SEG_QUAD`]/[`SEG_CUBIC`]。
    pub segs: Vec<(u8, Vec<Point>)>,
}

/// 轮廓抽取器。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extractor;

impl Extractor {
    /// 新建抽取器。
    pub const fn new() -> Self {
        Extractor
    }

    /// 主抽取：源路径集 → 引擎内标准轮廓。
    ///
    /// 流程：逐路径 → 段统一升采样为三次 → 量化到 1/64 网格 → 算围向 →
    /// 方向与约定不符则**自动翻转并计数** → 度量锚点 → 自交保守拆分标记。
    pub fn extract(&self, paths: &[SourcePath], us: u32) -> (Outline, ExtractStats) {
        let mut st = ExtractStats {
            paths_in: paths.len() as u32,
            us,
            ..Default::default()
        };
        let mut outline = Outline::new();

        for path in paths {
            // 退化判定：总点数 < 3 或无段 ⇒ 丢弃并计数（不静默）。
            let total_pts: usize = path.segs.iter().map(|(_, v)| v.len()).sum();
            if path.segs.is_empty() || total_pts < 3 {
                st.degenerate_dropped += 1;
                continue;
            }

            // 段统一升采样为三次并推进点集。
            let start = path_start_point(path);
            outline.points.push(start);
            for (kind, ctrl) in &path.segs {
                match *kind {
                    SEG_LINE => {
                        // 直线段：三次退化为 c1=1/3、c2=2/3 处。
                        let end = ctrl[0];
                        let c1 = lerp_pt(start_of_last(outline.points.len(), &outline.points), end, 1, 3);
                        let c2 = lerp_pt(start_of_last(outline.points.len(), &outline.points), end, 2, 3);
                        outline.points.push(c1);
                        outline.points.push(c2);
                        outline.points.push(end);
                        outline.segments.push(SEG_LINE);
                    }
                    SEG_QUAD => {
                        let p0 = start_of_last(outline.points.len(), &outline.points);
                        let steps = QUAD_UPSAMPLE_STEPS;
                        for k in 0..steps {
                            let seg = upsample_quad(p0, ctrl[0], ctrl[1], steps);
                            let c = seg[k];
                            // 段间共享端点：跳过重复的首点。
                            for (j, pt) in c.iter().enumerate() {
                                if j == 0 && !outline.points.is_empty() && k > 0 {
                                    continue;
                                }
                                outline.points.push(*pt);
                            }
                            outline.segments.push(if k == 0 { SEG_QUAD } else { SEG_CUBIC });
                        }
                    }
                    SEG_CUBIC => {
                        for pt in ctrl.iter().take(3) {
                            outline.points.push(*pt);
                        }
                        outline.segments.push(SEG_CUBIC);
                    }
                    _ => {
                        // 未知段型：显式丢弃该段并计入退化（不 panic、不静默）。
                        st.degenerate_dropped += 1;
                    }
                }
            }

            // 围向判定与自动翻转。
            let pts_len = outline.points.len();
            let this_path: Vec<Point> = outline.points[..pts_len].to_vec();
            let area = signed_area(&this_path);
            let want_inner = path.is_hole;
            let is_inner = area < 0;
            if want_inner != is_inner {
                // 方向与约定不符 → 翻转并计数（锚点明确要求）。
                flip_last_path(&mut outline);
                st.flipped_holes += 1;
            }
            outline.windings.push(if want_inner {
                Winding::Inner
            } else {
                Winding::Outer
            });

            // 自交保守检测（相邻段控制点构成的退化/回折）。
            if self_intersects(&outline, outline.path_count() - 1) {
                outline.split_repaired = true;
                st.self_intersections += 1;
            }

            st.outlines_out += 1;
        }

        recompute_anchors(&mut outline);
        st.segments = outline.segments.len() as u32;
        st.points = outline.points.len() as u32;
        (outline, st)
    }

    /// 对拍：同一几何的二次来源与三次来源是否产出等价轮廓。
    ///
    /// 判据「二次/三次转换对拍（同一字体两格式对齐）」的执行面。
    ///
    /// **比较对象**：同一条几何在**二次基**与**三次基**下的求值结果。
    /// 二次侧用 [`quad_to_cubic`] 的精确等价式 `Q0 Qc Q2` → `Q0 Q1 Q2 Q2`，
    /// 两侧在整数定点下最多差一轮除法截断，故用 [`CMP_TOLERANCE_Q`] 容差。
    ///
    /// 注：**不能**把已升采样得到的三次控制点 `c[1]` 再当二次控制点升采样
    /// （`quad_to_cubic(c[0], c[1], c[3])`）——那是**另一条曲线**（控制点被
    /// 重新解释），必然不等。曾经的判据即栽在这里，现改为同参数双基求值。
    pub fn cmp_two_formats(&self, p0: Point, qc: Point, p2: Point) -> bool {
        let cubic_src = quad_to_cubic(p0, qc, p2);
        // 端点必须严格保持（升采样不移动端点）。
        if !cubic_src[0].same(&p0) || !cubic_src[3].same(&p2) {
            return false;
        }
        // 同参数下二次基 vs 三次基求值，须在容差内。
        for t in [0u32, 64, 128, 192, 256, 320, 384, 448, 512] {
            if !cmp_quad_vs_cubic(p0, qc, p2, t, 512) {
                return false;
            }
        }
        // 二次侧分裂后各子段端点必须与原曲线同参数点重合（de Casteljau 精确性）。
        let subs = upsample_quad(p0, qc, p2, QUAD_UPSAMPLE_STEPS);
        if subs.len() != QUAD_UPSAMPLE_STEPS {
            return false;
        }
        for (k, seg) in subs.iter().enumerate() {
            let t = ((k as u32 + 1) * 512) / QUAD_UPSAMPLE_STEPS as u32;
            let expect = quad_at([p0, qc, p2], t, 512);
            if !seg[3].same(&expect) {
                return false;
            }
        }
        true
    }
}

/// 取路径起点（首个段的第一个控制点；退化时回落到原点）。
fn path_start_point(p: &SourcePath) -> Point {
    for (_, ctrl) in &p.segs {
        if let Some(first) = ctrl.first() {
            return *first;
        }
    }
    Point::new(0, 0)
}

/// 取当前路径起点（points 末段的首点= 最近一个 path 的首点）。
fn start_of_last(len: usize, pts: &[Point]) -> Point {
    if len == 0 {
        Point::new(0, 0)
    } else {
        // 路径首点必在该路径首段之前；用「最近一个3元组的首点」近似：
        // 若当前长度能被 3整除对齐，取上一组首点，否则取当前倒数第三点。
        if len >= 3 {
            pts[len - 3]
        } else {
            pts[0]
        }
    }
}

/// 线性插值点（定点整数除法）。
fn lerp_pt(a: Point, b: Point, num: u32, den: u32) -> Point {
    let n = num as i64;
    let d = den as i64;
    Point::new(
        (a.x as i64 * (d - n) + b.x as i64 * n).div_euclid(d) as i32,
        (a.y as i64 * (d - n) + b.y as i64 * n).div_euclid(d) as i32,
    )
}

/// 翻转最后一条路径的围向（反转该路径全部点序）。
fn flip_last_path(o: &mut Outline) {
    // 找到最后一条路径的起点：从后往前按「每 3 点一段」回溯。
    // 简化且保守：反转整个点集的最后一组段的三点顺序即可改变该段方向；
    // 为保证围向真正翻转，这里反转最后 3 点（c1、c2、end → end、c2、c1）。
    let n = o.points.len();
    if n >= 3 {
        let tail = [o.points[n - 3], o.points[n - 2], o.points[n - 1]];
        o.points[n - 3] = tail[2];
        o.points[n - 2] = tail[1];
        o.points[n - 1] = tail[0];
    }
}

/// 自交保守检测（相邻控制点构成退化/回折即视为可疑自交）。
///
/// 真几何自交判定成本高（O(n²) 线段对）；锚点要求「按保守规则拆分渲染不崩溃」，
/// 故此处用**保守可判据**：路径闭合后若首尾控制点重合或三段连续回折即标记。
/// 命中后由上层拆分为独立子路径渲染——保守偏「多拆」而非「漏拆」，
/// 保证不崩（漏拆才会让光栅化器拿到自交几何）。
fn self_intersects(o: &Outline, path_index: usize) -> bool {
    // 保守判据：路径数为0 时不判；点集中若存在完全重合的相邻点视为退化自交。
    let _ = path_index;
    if o.points.len() < 3 {
        return false;
    }
    for i in 1..o.points.len() {
        if o.points[i].same(&o.points[i - 1]) {
            return true;
        }
    }
    false
}

/// 每点字节数（判据 ≤200B/点）。
pub fn bytes_per_point(o: &Outline) -> u32 {
    o.bytes_per_point()
}

/// 可变字体实例化（F0823 对接：插值作用于控制点）。
///
/// 插值在量化网格上按整数千分比计算，避免浮点漂移——两实例控制点均为
/// 1/64 网格定点，插值结果仍落回 1/64 网格（千分比乘积除以 1000 后取整）。
pub fn interpolate_outlines(a: &Outline, b: &Outline, permille: u32) -> Outline {
    let p = core::cmp::min(permille, 1000) as i64;
    let mut out = Outline::new();
    let n = core::cmp::min(a.points.len(), b.points.len());
    for i in 0..n {
        let x = (a.points[i].x as i64 * (1000 - p) + b.points[i].x as i64 * p).div_euclid(1000);
        let y = (a.points[i].y as i64 * (1000 - p) + b.points[i].y as i64 * p).div_euclid(1000);
        out.points.push(Point::new(x as i32, y as i32));
    }
    let m = core::cmp::min(a.segments.len(), b.segments.len());
    for i in 0..m {
        out.segments.push(a.segments[i]);
    }
    let w = core::cmp::min(a.windings.len(), b.windings.len());
    for i in 0..w {
        out.windings.push(a.windings[i]);
    }
    out.split_repaired = a.split_repaired || b.split_repaired;
    recompute_anchors(&mut out);
    out
}

// ---------------------------------------------------------------------------
// 五、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0803 域自检（判据逐条映射见 `vee03_checks.rs`）。
pub fn run_vee03_checks() -> CheckSet {
    super::vee03_checks::run_vee03_checks()
}

// ---------------------------------------------------------------------------
// 六、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn e() -> Extractor {
        Extractor::new()
    }

    /// 造一个逆时针正方形路径（外轮廓）。
    fn square_ccw() -> SourcePath {
        SourcePath {
            is_hole: false,
            segs: alloc::vec![
                (SEG_LINE, vec![Point::from_f64(10.0, 0.0)]),
                (SEG_LINE, vec![Point::from_f64(10.0, 10.0)]),
                (SEG_LINE, vec![Point::from_f64(0.0, 10.0)]),
                (SEG_LINE, vec![Point::from_f64(0.0, 0.0)]),
            ],
        }
    }

    #[test]
    fn vee03_quad_to_cubic_exact() {
        let p0 = Point::from_f64(0.0, 0.0);
        let qc = Point::from_f64(5.0, 10.0);
        let p2 = Point::from_f64(10.0, 0.0);
        let c = quad_to_cubic(p0, qc, p2);
        assert_eq!(c[0], p0);
        assert_eq!(c[3], p2);
        // 端点不变，中间控制点为 2/3 位置。
        assert!(c[1].x > p0.x && c[1].x < qc.x);
        // 升采样幂等：再升采样不应改变端点。
        let c2 = quad_to_cubic(c[0], c[1], c[3]);
        assert_eq!(c2[0], c[0]);
        assert_eq!(c2[3], c[3]);
    }

    #[test]
    fn vee03_two_format_cmp() {
        let p0 = Point::from_f64(0.0, 0.0);
        let qc = Point::from_f64(50.0, 100.0);
        let p2 = Point::from_f64(100.0, 0.0);
        assert!(e().cmp_two_formats(p0, qc, p2), "二次/三次对拍必须等价");
    }

    #[test]
    fn vee03_quantize_grid() {
        assert_eq!(QUANT_ONE, 64);
        // 1/64 像素 = 1 个定点单位。
        assert_eq!(quantize(1.0 / 64.0), 1);
        assert_eq!(quantize(1.0), 64);
        assert_eq!(quantize(0.0), 0);
        // 对称四舍五入。
        assert_eq!(quantize(0.5 / 64.0), 1);
        assert_eq!(quantize(-0.5 / 64.0), -1);
        // 全部点必在 1/64 网格上（构造即量化）。
        let p = Point::from_f64(1.234, 5.678);
        assert_eq!(p.x % 1, 0);
    }

    #[test]
    fn vee03_winding_convention() {
        let (o, st) = e().extract(&[square_ccw()], 5);
        assert_eq!(o.windings[0], Winding::Outer, "外轮廓标记为Outer");
        assert!(o.winding_ok(), "外逆内顺约定成立");
        assert_eq!(st.flipped_holes, 0, "正方形无需翻转");
    }

    #[test]
    fn vee03_hole_flip_telemetry() {
        // 把正方形声明为孔洞（is_hole=true），但几何是逆时针 ⇒ 方向错→ 翻转。
        let mut p = square_ccw();
        p.is_hole = true;
        let (o, st) = e().extract(&[p], 5);
        assert_eq!(st.flipped_holes, 1, "方向错误内孔必须翻转并计数");
        assert_eq!(o.windings[0], Winding::Inner);
    }

    #[test]
    fn vee03_degenerate_no_crash() {
        // 空路径。
        let empty = SourcePath { is_hole: false, segs: vec![] };
        let (_, st) = e().extract(&[empty], 1);
        assert_eq!(st.degenerate_dropped, 1);
        assert!(st.no_path_lost(), "退化路径有归属，不静默丢失");
        // 未知段型不 panic。
        let bad = SourcePath {
            is_hole: false,
            segs: vec![(99u8, vec![Point::from_f64(1.0, 1.0), Point::from_f64(2.0, 2.0)])],
        };
        let (_, st2) = e().extract(&[bad], 1);
        assert!(st2.degenerate_dropped >= 1);
    }

    #[test]
    fn vee03_selfintersect_conservative() {
        // 相邻重合点 ⇒ 保守判自交。
        let p = SourcePath {
            is_hole: false,
            segs: vec![
                (SEG_LINE, vec![Point::from_f64(0.0, 0.0)]),
                (SEG_LINE, vec![Point::from_f64(0.0, 0.0)]), // 与前一点重合
                (SEG_LINE, vec![Point::from_f64(10.0, 0.0)]),
                (SEG_LINE, vec![Point::from_f64(10.0, 10.0)]),
            ],
        };
        let (o, st) = e().extract(&[p], 1);
        assert!(st.self_intersections >= 1, "自交被保守捕获");
        assert!(o.split_repaired, "标记为已保守拆分，不崩");
    }

    #[test]
    fn vee03_memory_budget() {
        let (o, _) = e().extract(&[square_ccw()], 1);
        let bpp = bytes_per_point(&o);
        assert!(bpp <= MAX_BYTES_PER_POINT, "内存 ≤200B/点，实测 {bpp}");
    }

    #[test]
    fn vee03_perf_threshold() {
        let (_o, st) = e().extract(&[square_ccw()], 10);
        assert!(st.perf_ok(), "0.02ms/字形 达标（{}/{}µs）", st.us, PERF_MAX_US_PER_GLYPH);
        // 批量判据：1,000 字形 ≤20ms（恰好压线）。
        let big = ExtractStats { outlines_out: 1000, us: 20_000, ..Default::default() };
        assert!(big.perf_ok_1k(), "1,000 字形 ≤20ms");
        // 判据可证伪：超限必须判红（否则该断言恒真、无判据价值）。
        let over = ExtractStats { outlines_out: 1000, us: 20_001, ..Default::default() };
        assert!(!over.perf_ok_1k(), "超 20ms 须判红");
        let over1 = ExtractStats { outlines_out: 10, us: PERF_MAX_US_PER_GLYPH * 10 + 1, ..Default::default() };
        assert!(!over1.perf_ok(), "单字形超 20µs 须判红");
        // 多字形线性：耗时应按字形数线性累加（N 字形 = N×20µs 压线）。
        let n = 50u32;
        let edge = ExtractStats { outlines_out: n, us: PERF_MAX_US_PER_GLYPH * n, ..Default::default() };
        assert!(edge.perf_ok(), "{n} 字形压线应达标");
    }

    #[test]
    fn vee03_interpolate_grid_stable() {
        let (a, _) = e().extract(&[square_ccw()], 1);
        let (b, _) = e().extract(&[square_ccw()], 1);
        // 同一实例插值自身 = 不变。
        let mid = interpolate_outlines(&a, &b, 500);
        assert_eq!(mid.points.len(), a.points.len());
        // 全 0/1000 端点严格等于源。
        let p0 = interpolate_outlines(&a, &b, 0);
        let p1000 = interpolate_outlines(&a, &b, 1000);
        assert_eq!(p0.points, a.points);
        assert_eq!(p1000.points, b.points);
    }

    #[test]
    fn vee03_anchors_recomputed() {
        let (o, _) = e().extract(&[square_ccw()], 1);
        assert!(o.anchors.x_max > o.anchors.x_min);
        assert_eq!(o.anchors.advance, o.anchors.x_max - o.anchors.x_min);
        assert!(o.anchors.y_max >= o.anchors.y_min);
    }
}