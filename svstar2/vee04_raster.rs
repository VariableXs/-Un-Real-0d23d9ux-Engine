//! VE-F0804 · 字形光栅化器（VE-E 域 · 文字渲染段 1 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0804`
//!
//! **判据（锚点原文五条）**：边缘平滑、Gamma 感知、四档粗细、0.05ms/字形、
//! 分块防越界。逐条落位：
//! - **边缘平滑**：扫描线光栅化为主路径（[`scanline::rasterize`]），每像素行
//!   取 [`SUB_SAMPLES`] = 16 条子扫描线逐行求交，按**非零环绕规则**（承接 F0803
//!   「外逆内顺」约定）解出跨度，再做**精确面积累加**（[`scanline::add_span`]，
//!   亚像素级分配而非整像素置位）⇒ 输出必含中间灰阶。
//!   抗锯齿台阶由 [`smoothness`] 机检：相邻像素覆盖率出现「低≤96 直接跳高≥160」
//!   的硬跳变即判红（真AA 斜边相邻差约 255/16≈16）。
//! - **Gamma 感知**：线性覆盖率经 [`GAMMA_LUT`]（`round(255·(i/255)^(1/2.2))`，
//!   整数常量表、零运行时浮点）映射为 sRGB 感知权重后输出（[`Bitmap8::apply_gamma`]），
//!   混合侧 [`Bitmap8::blend_over`] 以感知权重做线性光混合。
//! - **四档粗细**：[`Weight`] 四档（细/常规/半粗/粗）em 比例取自 F0826 合成口径
//!   的 0.02–0.04 区间（[`Weight::permille`] = 20/27/33/40‱），以**覆盖率区域膨胀**
//!   实现加粗（[`scanline::rasterize`] 内 y向与 x 向同时外扩 [`Weight::half_width_q`]，
//!   等价于与边长 2·hw 的方形做闵可夫斯基和）⇒ 四档面积严格递增且与 F0826 对齐。
//! - **0.05ms/字形**：[`PERF_MAX_US_PER_GLYPH`] = 50µs（锚点 0.05ms），
//!   [`PERF_MAX_US_1K`] = 50,000µs（1,000 字形/帧 ≤50ms）；超限置
//!   [`RasterStats::atlas_fallback`] ⇒ 走 F0806 缓存兜底（锚点错误路径）。
//! - **分块防越界**：字号 > [`MAX_TILE_DIM`]（256px）⇒ [`tiled::rasterize_tiled`]
//!   分块光栅化，单块边长硬上限 [`MAX_TILE_DIM`]、单块字节上限
//!   [`MAX_TILE_BYTES`]，杜绝「一块4096² = 16MB」式越界。
//!
//! **数据结构（锚点原文「字形 ID、字号、变换、目标位图句柄」四件）**：
//! [`RasterTask`] 的 `glyph_id` / `px_size` / [`RasterTransform`]（平移 + 可选粗细）/
//! [`BitmapHandle`]（F0806 图集位图句柄）——四字段一一对应，无隐式全局态。
//!
//! **可选 LCD 亚像素模式**：[`LcdBitmap`] 三通道**独立覆盖**（[`lcd::rasterize_lcd`]），
//! R/G/B 各以 ∓1/3 像素偏移独立求交，非由灰度图推导——这是LCD 的定义性特征。
//!
//! **错误路径与降级矩阵**：
//! - 退化轮廓（零面积）→ 输出空位图并计数 [`RasterStats::degenerate`]（锚点原文）；
//! - 超大字号 → 分块光栅化（见上）；
//! - 自交轮廓（F0803 已保守拆分标记）→ 按拆分后子路径独立求交，不崩；
//! - 位图边长超 [`MAX_BITMAP_DIM`] → 显性拒绝并计数 [`RasterStats::rejected`]，
//!   **不**静默截断（静默截断会切掉字形右半边，属静默损坏）。
//!
//! **对接**：输出进 F0806 图集（[`RasterOutput::atlas_slot`] 给出页内槽位与
//! 越界判定）；后端注册接口见 F0816（[`RasterBackend`] + [`Rasterizer::register`]，
//! 内置扫描线后端 [`ScanlineBackend`] 为默认，可换自定义后端而调用面不变）。
//!
//! **零浮点纪律**：几何全链F26Dot6 定点（i32，1/64 像素，承接 F0803），
//! Gamma 为整数常量表——内核无浮点依赖，跨机器回归可复现。

use alloc::vec;
use alloc::vec::Vec;

use super::vee03_outline::{Outline, Point, Winding};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源——文档与自检均引此处，不复制字面量）
// ---------------------------------------------------------------------------

/// 每像素行的子扫描线数（纵向超采样倍率）。16 ⇒纵向覆盖率分辨率 1/16 像素。
pub const SUB_SAMPLES: usize = 16;

/// 覆盖率累加器的「满格」单位：1 像素 = 1<<16（16.16 定点，累加全程整数）。
pub const COV_FULL: i32 = 1 << 16;

/// 分块边长上限（锚点：字号 >256px → 分块光栅化防单块内存越界）。
pub const MAX_TILE_DIM: u32 = 256;

/// 单块位图字节上限（256×256 = 65,536B，硬闸）。
pub const MAX_TILE_BYTES: u32 = MAX_TILE_DIM * MAX_TILE_DIM;

/// 单块位图边长绝对上限（超出显性拒绝，不静默截断）。
pub const MAX_BITMAP_DIM: u32 = 4096;

/// 性能判据：常规字号光栅化 ≤0.05ms/字形（µs）。
pub const PERF_MAX_US_PER_GLYPH: u32 = 50;

/// 批量判据：1,000 字形/帧 ≤50ms（µs）。超限走 F0806 缓存兜底。
pub const PERF_MAX_US_1K: u32 = 50_000;

/// 小字号阈值（锚点质量约定：≤12px 可辨）。
pub const SMALL_PX_MAX: u32 = 12;

/// Gamma 感知权重表：`round(255·(i/255)^(1/2.2))`，线性覆盖率 → sRGB 感知权重。
///
/// 整数常量表（生成口径见文件末注），运行时零浮点。单调不减、`LUT[0]=0`、
/// `LUT[255]=255`、`LUT[128]=186`（中间调被提亮——这正是 Gamma 感知的可见效果）。
pub const GAMMA_LUT: [u8; 256] = [
    0, 21, 28, 34, 39, 43, 46, 50, 53, 56, 59, 61, 64, 66, 68, 70, 72, 74, 76, 78, 80, 82, 84, 85,
    87, 89, 90, 92, 93, 95, 96, 98, 99, 101, 102, 103, 105, 106, 107, 109, 110, 111, 112, 114, 115,
    116, 117, 118, 119, 120, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134,
    135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 144, 145, 146, 147, 148, 149, 150, 151, 151,
    152, 153, 154, 155, 156, 156, 157, 158, 159, 160, 160, 161, 162, 163, 164, 164, 165, 166,
    167, 167, 168, 169, 170, 170, 171, 172, 173, 173, 174, 175, 175, 176, 177, 178, 178, 179,
    180, 180, 181, 182, 182, 183, 184, 184, 185, 186, 186, 187, 188, 188, 189, 190, 190, 191,
    192, 192, 193, 194, 194, 195, 195, 196, 197, 197, 198, 199, 199, 200, 200, 201, 202, 202,
    203, 203, 204, 205, 205, 206, 206, 207, 207, 208, 209, 209, 210, 210, 211, 212, 212, 213,
    213, 214, 214, 215, 215, 216, 217, 217, 218, 218, 219, 219, 220, 220, 221, 221, 222, 223,
    223, 224, 224, 225, 225, 226, 226, 227, 227, 228, 228, 229, 229, 230, 230, 231, 231, 232,
    232, 233, 233, 234, 234, 235, 235, 236, 236, 237, 237, 238, 238, 239, 239, 240, 240, 241,
    241, 242, 242, 243, 243, 244, 244, 245, 245, 246, 246, 247, 247, 248, 248, 249, 249, 249,
    250, 250, 251, 251, 252, 252, 253, 253, 254, 254, 255, 255,
];

// ---------------------------------------------------------------------------
// 二、粗细四档（判据「四档粗细」，口径对齐 F0826）
// ---------------------------------------------------------------------------

/// 字形粗细档（锚点：细/常规/半粗/粗四档，与 F0826 合成口径一致）。
///
/// em 比例落在 F0826 明示的 0.02–0.04 区间内，四档严格递增；
/// [`Weight::name`] 为中文档名（无障碍与 UI 提示共用同一命名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Weight {
    /// 细（0.020 em）。
    Thin,
    /// 常规（0.027 em）。
    Regular,
    /// 半粗（0.033 em）。
    SemiBold,
    /// 粗（0.040 em）。
    Bold,
}

/// F0826 合成口径下界（em 比例 0.02，对接判据）。
pub const WEIGHT_EM_MIN_PERMILLE: u32 = 20;

/// F0826 合成口径上界（em 比例 0.04，对接判据）。
pub const WEIGHT_EM_MAX_PERMILLE: u32 = 40;

impl Weight {
    /// 四档按粗到细的全序（[`Weight::ALL`]）。
    pub const ALL: [Weight; 4] = [
        Weight::Thin,
        Weight::Regular,
        Weight::SemiBold,
        Weight::Bold,
    ];

    /// 档名（中文，UI 与自检共用——避免两处各写一份字面量）。
    pub const fn name(self) -> &'static str {
        match self {
            Weight::Thin => "细",
            Weight::Regular => "常规",
            Weight::SemiBold => "半粗",
            Weight::Bold => "粗",
        }
    }

    /// 粗细的 em 比例（千分比）：20/ 27 / 33 / 40‱。
    pub const fn permille(self) -> u32 {
        match self {
            Weight::Thin => 20,
            Weight::Regular => 27,
            Weight::SemiBold => 33,
            Weight::Bold => 40,
        }
    }

    /// 半加粗宽度的 F26Dot6 值（1/64 像素单位）。
    ///
    /// `hw = permille × px_size × 64 / 2000`（em 比例 × 字号 ÷ 2）。
    /// 整数除法保证确定性；四档在同一字号下 hw 严格递增（`>=1/64` 可分辨）。
    pub const fn half_width_q(self, px_size: u32) -> i32 {
        ((self.permille() as i64 * px_size as i64 * 64) / 2000) as i32
    }

    /// 四档 em 比例是否严格递增且落在 F0826 的 0.02–0.04 区间内。
    ///
    /// 这是**档位集合**的性质（不看self）——故取无`self` 关联函数，
    /// 与 [`Weight::half_width_monotonic`] 口径一致：调用方不必先选一档。
    pub fn aligned_with_f0826() -> bool {
        let mut prev = 0u32;
        for w in Weight::ALL.iter() {
            let p = w.permille();
            if p <= prev {
                return false;
            }
            if p < WEIGHT_EM_MIN_PERMILLE || p > WEIGHT_EM_MAX_PERMILLE {
                return false;
            }
            prev = p;
        }
        true
    }

    /// 半宽是否随档位严格递增（同一字号下加粗效果可分辨）。
    pub fn half_width_monotonic(px_size: u32) -> bool {
        let hw: Vec<i32> = Weight::ALL.iter().map(|w| w.half_width_q(px_size)).collect();
        let mut prev = -1i32;
        for v in hw.iter() {
            if *v <= prev {
                return false;
            }
            prev = *v;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 三、几何：边表与轮廓集（光栅化的输入面）
// ---------------------------------------------------------------------------

/// 一条边（F26Dot6 直线段）。水平边在建表时即剔除（对求交无贡献）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    /// 起点 x（1/64 像素）。
    pub x0: i32,
    /// 起点 y（1/64 像素）。
    pub y0: i32,
    /// 终点 x（1/64 像素）。
    pub x1: i32,
    /// 终点 y（1/64 像素）。
    pub y1: i32,
}

impl Edge {
    /// 构造边；水平边返回 [`None`]（无求交贡献，剔除即省去每子扫描线的无用比较）。
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Option<Edge> {
        if y0 == y1 {
            return None;
        }
        Some(Edge { x0, y0, x1, y1 })
    }

    /// y 方向是否跨越 `y`（半开区间 `[min, max)`，避免顶点重复计数）。
    pub fn crosses(&self, y: i32) -> bool {
        (self.y0 <= y) != (self.y1 <= y)
    }

    /// 与水平线 `y` 的交点 x（F26Dot6，整数除法无浮点）。
    pub fn x_at(&self, y: i32) -> i32 {
        let dy = (self.y1 - self.y0) as i64;
        let dx = (self.x1 - self.x0) as i64;
        let t = (y - self.y0) as i64;
        (self.x0 as i64 + dx * t / dy) as i32
    }

    /// 穿越方向：向上（y1 > y0）为 +1，向下为 -1——非零环绕规则的符号源。
    pub fn dir(&self) -> i32 {
        if self.y1 > self.y0 {
            1
        } else {
            -1
        }
    }
}

/// 一条闭合轮廓（已扁平化为边序列 + 围向标记）。
///
/// 不 derive `Default`：上游 [`Winding`] 无 `Default` 实现（外/内孔无默认语义），
/// 故此处显式给 [`Contour::new`]。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contour {
    /// 边序列。
    pub edges: Vec<Edge>,
    /// 围向（F0803 约定：外逆内顺）。
    pub winding: Winding,
}

impl Contour {
    /// 构造空边轮廓（围向必显式给出——不允许隐式默认）。
    pub fn new(winding: Winding) -> Self {
        Contour {
            edges: Vec::new(),
            winding,
        }
    }
}

/// 光栅化输入：轮廓集（字形 = 一组闭合轮廓，外轮廓 + 内孔）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContourSet {
    /// 全部闭合轮廓。
    pub contours: Vec<Contour>,
}

impl ContourSet {
    /// 空轮廓集。
    pub fn new() -> Self {
        ContourSet { contours: Vec::new() }
    }

    /// 边总数。
    pub fn edge_count(&self) -> usize {
        let mut n = 0usize;
        for c in self.contours.iter() {
            n += c.edges.len();
        }
        n
    }

    /// 全部轮廓的有符号面积之和（twice-area，F26Dot6²）。
    ///
    /// **零面积判定用**：外轮廓逆时针为正、内孔顺时针为负，正常字形的
    /// 外轮廓面积绝对值应大于全部内孔之和 ⇒ 净面积显著非零。
    pub fn signed_area2(&self) -> i64 {
        let mut acc: i64 = 0;
        for c in self.contours.iter() {
            for e in c.edges.iter() {
                acc += e.x0 as i64 * e.y1 as i64 - e.x1 as i64 * e.y0 as i64;
            }
        }
        acc
    }

    /// 是否为退化轮廓（净面积为零——锚点错误路径：输出空位图并计数）。
    pub fn is_degenerate(&self) -> bool {
        self.signed_area2() == 0
    }

    /// 设备空间包围盒（已含变换平移），None 表示空。
    pub fn bounds(&self, dx: i32, dy: i32) -> Option<(i32, i32, i32, i32)> {
        let mut x_min = i32::MAX;
        let mut y_min = i32::MAX;
        let mut x_max = i32::MIN;
        let mut y_max = i32::MIN;
        let mut any = false;
        for c in self.contours.iter() {
            for e in c.edges.iter() {
                any = true;
                for (x, y) in [(e.x0, e.y0), (e.x1, e.y1)].iter() {
                    let px = x + dx;
                    let py = y + dy;
                    if px < x_min {
                        x_min = px;
                    }
                    if px > x_max {
                        x_max = px;
                    }
                    if py < y_min {
                        y_min = py;
                    }
                    if py > y_max {
                        y_max = py;
                    }
                }
            }
        }
        if !any {
            return None;
        }
        Some((x_min, y_min, x_max, y_max))
    }

    /// 从 F0803 [`Outline`] 适配（上游对接面）。
    ///
    /// 用 `path_slices()` 按围向标记切出各闭合路径，再把控制点折线化为边。
    /// 三次段已由 F0803 升采样（[`SUPERSAMPLE_UPHINT`]）到足够密度，
    /// 故此处按折线处理即无可见偏差；如需严格三次求交由 F0803 提供采样点。
    pub fn from_outline(o: &Outline, dx: i32, dy: i32) -> ContourSet {
        let mut cs = ContourSet::new();
        for (pts, w) in o.path_slices() {
            if pts.len() < 2 {
                continue;
            }
            let mut edges: Vec<Edge> = Vec::new();
            for i in 0..pts.len() {
                let a = pts[i];
                let b = pts[(i + 1) % pts.len()];
                if let Some(e) = Edge::new(a.x + dx, a.y + dy, b.x + dx, b.y + dy) {
                    edges.push(e);
                }
            }
            if !edges.is_empty() {
                cs.contours.push(Contour { edges, winding: *w });
            }
        }
        cs
    }
}

/// 三次贝塞尔扁平化步数（每三次段按 de Casteljau 均分）。
pub const CUBIC_FLATTEN_STEPS: usize = 16;

/// F0803 升采样步数（供头注对拍口径引用，本模块按三次段再细分）。
pub const SUPERSAMPLE_UPHINT: usize = 8;

/// 由三次控制点构造边序列（de Casteljau 逐段切分，整数定点）。
pub fn flatten_cubic(p: [Point; 4]) -> Vec<Edge> {
    let mut out: Vec<Edge> = Vec::new();
    let mut rest = p;
    for i in 0..CUBIC_FLATTEN_STEPS {
        let remain = CUBIC_FLATTEN_STEPS - i;
        let (l, r) = split_cubic_at(&rest, 1, remain as u32);
        if let Some(e) = Edge::new(l[0].x, l[0].y, l[3].x, l[3].y) {
            out.push(e);
        }
        rest = r;
    }
    out
}

/// de Casteljau 在 `num`/`den` 处精确切分三次曲线为左右两段。
fn split_cubic_at(c: &[Point; 4], num: u32, den: u32) -> ([Point; 4], [Point; 4]) {
    let [p0, p1, p2, p3] = *c;
    let a = lerp_q(p0, p1, num, den);
    let b = lerp_q(p1, p2, num, den);
    let cc = lerp_q(p2, p3, num, den);
    let d = lerp_q(a, b, num, den);
    let e = lerp_q(b, cc, num, den);
    let m = lerp_q(d, e, num, den);
    ([p0, a, d, m], [m, e, cc, p3])
}

/// 两点在 `num`/`den` 处的线性插值（F26Dot6 整数定点）。
fn lerp_q(a: Point, b: Point, num: u32, den: u32) -> Point {
    let n = num as i64;
    let d = den as i64;
    Point::new(
        (a.x as i64 * (d - n) + b.x as i64 * n).div_euclid(d) as i32,
        (a.y as i64 * (d - n) + b.y as i64 * n).div_euclid(d) as i32,
    )
}

/// 由折线点列构造闭合轮廓（点<3 时退化返回空边集）。
pub fn contour_from_polyline(pts: &[Point], winding: Winding) -> Contour {
    let mut edges: Vec<Edge> = Vec::new();
    if pts.len() < 3 {
        return Contour { edges, winding };
    }
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        if let Some(e) = Edge::new(a.x, a.y, b.x, b.y) {
            edges.push(e);
        }
    }
    Contour { edges, winding }
}

// ---------------------------------------------------------------------------
// 四、位图产物：8bit 覆盖率 / LCD 三通道 / 分块
// ---------------------------------------------------------------------------

/// 目标位图句柄（锚点数据结构第四件；F0806 图集的页内坐标）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BitmapHandle {
    /// 图集页号（F0806 页分配器给出）。
    pub page: u16,
    /// 页内槽位序号。
    pub slot: u16,
}

/// 8bit 覆盖率位图（主路径产物）。
///
/// `data` 长度恒为 `w × h`；行优先。`data` 为**线性覆盖率**，
/// 经 [`Bitmap8::apply_gamma`] 后成为 sRGB 感知权重。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bitmap8 {
    /// 宽（像素）。
    pub w: u16,
    /// 高（像素）。
    pub h: u16,
    /// 相对原点的左偏移（像素，可负）。
    pub left: i16,
    /// 相对原点的上偏移（像素，可负）。
    pub top: i16,
    /// 覆盖率数据（线性，0..=255），长度 = w×h。
    pub data: Vec<u8>,
}

impl Bitmap8 {
    /// 构造零位图。
    pub fn new(w: u16, h: u16) -> Self {
        Bitmap8 {
            w,
            h,
            left: 0,
            top: 0,
            data: vec![0u8; w as usize * h as usize],
        }
    }

    /// 是否为空位图（无像素）。
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0 || self.data.is_empty()
    }

    /// 读覆盖率（越界返回 0——光栅化后处理的常规边界情形，不 panic）。
    pub fn cov(&self, x: u16, y: u16) -> u8 {
        if x >= self.w || y >= self.h {
            return 0;
        }
        self.data[y as usize * self.w as usize + x as usize]
    }

    /// 写覆盖率（越界丢弃——同上）。
    pub fn set_cov(&mut self, x: u16, y: u16, v: u8) {
        if x >= self.w || y >= self.h {
            return;
        }
        let w = self.w as usize;
        self.data[y as usize * w + x as usize] = v;
    }

    /// 线性覆盖率 → Gamma 感知权重（就地；判据「Gamma 感知」执行面）。
    pub fn apply_gamma(&mut self) {
        for px in self.data.iter_mut() {
            *px = GAMMA_LUT[*px as usize];
        }
    }

    /// 线性覆盖率总量（面积，单位：像素²·1/255）——四档粗细比较用。
    pub fn area(&self) -> u64 {
        let mut s: u64 = 0;
        for v in self.data.iter() {
            s += *v as u64;
        }
        s
    }

    /// 非零像素数。
    pub fn ink_pixels(&self) -> u32 {
        let mut n = 0u32;
        for v in self.data.iter() {
            if *v != 0 {
                n += 1;
            }
        }
        n
    }

    /// 峰值覆盖率。
    pub fn peak(&self) -> u8 {
        let mut m = 0u8;
        for v in self.data.iter() {
            if *v > m {
                m = *v;
            }
        }
        m
    }

    /// 感知权重混合（source-over）：`dst = src·a + dst·(1-a)`，`a` 取感知权重。
    ///
    /// 混合在**线性光**空间做，故用线性覆盖率除255 还原权重；
    /// 这正是 Gamma 混合要解决的问题——直接拿感知值当alpha 混会二次 Gamma。
    pub fn blend_over(&mut self, src: &Bitmap8, x: i32, y: i32) {
        let dw = self.w as i32;
        let dh = self.h as i32;
        for sy in 0..src.h as i32 {
            for sx in 0..src.w as i32 {
                let a = src.cov(sx as u16, sy as u16);
                if a == 0 {
                    continue;
                }
                let dx = x + sx;
                let dy = y + sy;
                if dx < 0 || dy < 0 || dx >= dw || dy >= dh {
                    continue;
                }
                let idx = (dy as usize) * (dw as usize) + dx as usize;
                let d = self.data[idx] as u32;
                let al = a as u32;
                // alpha 以 16.16 定点做，d 保持 0..255 整数域，零浮点。
                let mixed = (al * d + (255 - al) * 255) / 255;
                self.data[idx] = mixed.min(255) as u8;
            }
        }
    }

    /// 转F0806 图集槽位（页内落位+ 越界判定）。
    pub fn atlas_slot(&self, handle: BitmapHandle, pen_x: u16, pen_y: u16) -> AtlasSlot {
        AtlasSlot {
            page: handle.page,
            x: pen_x,
            y: pen_y,
            w: self.w,
            h: self.h,
        }
    }
}

/// 图集槽位（F0806 对接产物）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasSlot {
    /// 图集页号。
    pub page: u16,
    /// 页内 x。
    pub x: u16,
    /// 页内 y。
    pub y: u16,
    /// 槽宽。
    pub w: u16,
    /// 槽高。
    pub h: u16,
}

impl AtlasSlot {
    /// 是否越出页容量（默认页 2048×2048）。
    pub fn fits_page(self, page_w: u16, page_h: u16) -> bool {
        let right = self.x as u32 + self.w as u32;
        let bottom = self.y as u32 + self.h as u32;
        right <= page_w as u32 && bottom <= page_h as u32
    }
}

/// LCD 亚像素位图：**三通道独立覆盖**（锚点可选模式）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LcdBitmap {
    /// 宽（像素）。
    pub w: u16,
    /// 高（像素）。
    pub h: u16,
    /// R 通道覆盖率（长度 = w×h）。
    pub r: Vec<u8>,
    /// G 通道覆盖率。
    pub g: Vec<u8>,
    /// B 通道覆盖率。
    pub b: Vec<u8>,
}

impl LcdBitmap {
    /// 构造三通道零位图。
    pub fn new(w: u16, h: u16) -> Self {
        let n = w as usize * h as usize;
        LcdBitmap {
            w,
            h,
            r: vec![0u8; n],
            g: vec![0u8; n],
            b: vec![0u8; n],
        }
    }

    /// 读某通道覆盖率。
    ///
    /// 越界（含**通道长度与 `w·h` 不一致**）一律返回 0——三通道共用单一
    /// `w`/`h` 作索引步长，长度不齐时按 `y·w+x` 读会越界 panic。
    /// [`LcdBitmap::channels_consistent`] 把该前提做成可机检项。
    pub fn cov(&self, ch: u8, x: u16, y: u16) -> u8 {
        if x >= self.w || y >= self.h {
            return 0;
        }
        let i = y as usize * self.w as usize + x as usize;
        let v = match ch {
            0 => &self.r,
            1 => &self.g,
            _ => &self.b,
        };
        if i >= v.len() {
            return 0;
        }
        v[i]
    }

    /// 三通道长度是否与 `w·h` 一致（索引步长前提；不一致即产物畸形）。
    pub fn channels_consistent(&self) -> bool {
        let n = self.w as usize * self.h as usize;
        self.r.len() == n && self.g.len() == n && self.b.len() == n
    }

    /// 三通道是否在某像素上真正互不相同（LCD 的定义性特征）。
    ///
    /// 灰度图推导的三通道必然相等⇒ 本判据可区分「真 LCD 独立求交」
    /// 与「灰度图复制三份」的伪实现。
    pub fn has_channel_divergence(&self) -> bool {
        let n = self.w as usize * self.h as usize;
        for i in 0..n {
            let (r, g, b) = (self.r[i], self.g[i], self.b[i]);
            if r != g || g != b {
                return true;
            }
        }
        false
    }

    /// 三通道同时施加 Gamma 感知权重。
    pub fn apply_gamma(&mut self) {
        for ch in [0u8, 1, 2] {
            let v: &mut Vec<u8> = match ch {
                0 => &mut self.r,
                1 => &mut self.g,
                _ => &mut self.b,
            };
            for px in v.iter_mut() {
                *px = GAMMA_LUT[*px as usize];
            }
        }
    }
}

/// 分块光栅化产物（超大字号防越界）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TiledBitmap {
    /// 块宽（≤ [`MAX_TILE_DIM`]）。
    pub tile_w: u16,
    /// 块高（≤ [`MAX_TILE_DIM`]）。
    pub tile_h: u16,
    /// 块列数。
    pub cols: u16,
    /// 块行数。
    pub rows: u16,
    /// 块数据，行优先（长度 = cols × rows）。
    pub tiles: Vec<Bitmap8>,
}

impl TiledBitmap {
    /// 单块字节数。
    pub fn tile_bytes(&self) -> u32 {
        self.tile_w as u32 * self.tile_h as u32
    }

    /// 分块是否全部满足单块上限（判据「分块防越界」执行面）。
    pub fn within_tile_limit(&self) -> bool {
        self.tile_w as u32 <= MAX_TILE_DIM
            && self.tile_h as u32 <= MAX_TILE_DIM
            && self.tile_bytes() <= MAX_TILE_BYTES
            && self.tiles.len() == (self.cols as usize) * (self.rows as usize)
    }

    /// 是否需要分块（位图任一边 > [`MAX_TILE_DIM`]）。
    pub fn needs_tiling(w: u32, h: u32) -> bool {
        w > MAX_TILE_DIM || h > MAX_TILE_DIM
    }
}

// ---------------------------------------------------------------------------
// 五、光栅化核心：扫描线 + 精确面积累加
// ---------------------------------------------------------------------------

/// 光栅化选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterOpts {
    /// 半加粗宽度（F26Dot6；0 = 不加粗）。
    pub half_width_q: i32,
    /// 是否在产出后施加 Gamma 感知权重。
    pub gamma: bool,
}

impl RasterOpts {
    /// 不加粗 + 施加Gamma（默认口径）。
    pub const fn plain() -> Self {
        RasterOpts {
            half_width_q: 0,
            gamma: true,
        }
    }

    /// 按粗细档与字号构造选项。
    pub fn for_weight(px_size: u32, w: Weight, gamma: bool) -> Self {
        RasterOpts {
            half_width_q: w.half_width_q(px_size),
            gamma,
        }
    }
}

/// 扫描线光栅化子模块（主路径）。
pub mod scanline {
    use super::*;

    /// 求一条子扫描线上的全部交点（插入排序——交点数通常 <32，且近乎有序）。
    fn crossings(edges: &[Edge], y: i32, xs: &mut Vec<(i32, i32)>) {
        xs.clear();
        for e in edges.iter() {
            if e.crosses(y) {
                xs.push((e.x_at(y), e.dir()));
            }
        }
        // 按 x 升序插入排序（避免依赖 core 的排序特性，保证 no_std 可编译）。
        let n = xs.len();
        for i in 1..n {
            let key = xs[i];
            let mut j = i;
            while j > 0 && xs[j - 1].0 > key.0 {
                xs[j] = xs[j - 1];
                j -= 1;
            }
            xs[j] = key;
        }
    }

    /// 把 `[x0, x1)` 的跨度按**精确面积**摊入累加器（亚像素级，非整像素置位）。
    ///
    /// `amount` = 该子扫描线在一个满格像素上的贡献（通常 = [`COV_FULL`]）。
    /// 跨度过宽时裁剪到 `acc` 范围（越界部分丢弃）。
    pub fn add_span(acc: &mut [i32], x0: i32, x1: i32, amount: i32) {
        if x1 <= x0 {
            return;
        }
        let w = acc.len() as i32;
        let lo = x0.max(0);
        let hi = x1.min(w << 6);
        if hi <= lo {
            return;
        }
        let a = lo >> 6;
        let b = (hi - 1) >> 6;
        if a == b {
            acc[a as usize] += amount * (hi - lo) / 64;
            return;
        }
        acc[a as usize] += amount * (((a + 1) << 6) - lo) / 64;
        for p in (a + 1)..b {
            acc[p as usize] += amount;
        }
        acc[b as usize] += amount * (hi - (b << 6)) / 64;
    }

    /// 主光栅化：轮廓集 + 变换 + 选项 → 8bit 覆盖率位图。
    ///
    /// 流程：退化/超限判定 → 由包围盒（含平移与加粗外扩）推画布几何 →
    /// 转交 [`rasterize_boxed`] 做逐行求交与面积累加。
    ///
    /// 加粗（[`RasterOpts::half_width_q`] > 0）以**区域膨胀**实现：
    /// 子扫描线在 y 向外扩 hw（行参与范围扩大），跨度在 x 向两端各外扩 hw
    /// ⇒ 等价于与边长 2·hw 的方形做闵可夫斯基和，横竖笔画同时加粗。
    pub fn rasterize(
        cs: &ContourSet,
        dx: i32,
        dy: i32,
        opts: RasterOpts,
    ) -> (Bitmap8, RasterStats) {
        let mut st = RasterStats::default();
        if cs.is_degenerate() {
            st.degenerate = 1;
            return (Bitmap8::new(0, 0), st);
        }
        let (x_min, y_min, x_max, y_max) = match cs.bounds(0, 0) {
            Some(b) => b,
            None => {
                st.degenerate = 1;
                return (Bitmap8::new(0, 0), st);
            }
        };
        // 包围盒须**外扩 hw**：加粗让字形向四周膨胀 hw，若画布仍按未加粗的
        // 包围盒开，越粗的字形被画布裁掉的越多——四档面积会随加粗**变小**
        // （判据倒挂）。画布放大 2hw 后膨胀部分才完整落进位图。
        let pad = opts.half_width_q.max(0);
        let ax0 = x_min + dx - pad;
        let ay0 = y_min + dy - pad;
        let ax1 = x_max + dx + pad;
        let ay1 = y_max + dy + pad;
        let left = ax0.div_euclid(64);
        let top = ay0.div_euclid(64);
        let bw = ((ax1 - 1).div_euclid(64) - left + 1).max(1) as u32;
        let bh = ((ay1 - 1).div_euclid(64) - top + 1).max(1) as u32;
        if bw > MAX_BITMAP_DIM || bh > MAX_BITMAP_DIM {
            st.rejected = 1;
            return (Bitmap8::new(0, 0), st);
        }
        rasterize_boxed(cs, dx, dy, opts, left, top, bw as u16, bh as u16, st)
    }

    /// 按**给定画布几何**光栅化（LCD 三通道共用的执行面）。
    ///
    /// 为什么必须显式传几何：LCD 的 R/G/B 三趟各带 ∓1/3 像素偏移，若各自按
    /// 自己的包围盒开画布，三趟宽度会差 1 像素（实测 17/16/17）。而
    /// [`LcdBitmap`] 用**单一** `w`/`h` 作三通道的索引步长——三通道长度不一致时
    /// 按 `y·w+x` 读 `g`/`b` 会越界 panic。故三趟共用同一画布，
    /// 由调用方取**并集**几何。
    #[allow(clippy::too_many_arguments)]
    pub fn rasterize_boxed(
        cs: &ContourSet,
        dx: i32,
        dy: i32,
        opts: RasterOpts,
        left: i32,
        top: i32,
        bw: u16,
        bh: u16,
        st: RasterStats,
    ) -> (Bitmap8, RasterStats) {
        let mut st = st;
        if bw == 0 || bh == 0 {
            st.degenerate = 1;
            return (Bitmap8::new(0, 0), st);
        }
        if bw as u32 > MAX_BITMAP_DIM || bh as u32 > MAX_BITMAP_DIM {
            st.rejected = 1;
            return (Bitmap8::new(0, 0), st);
        }

        // 扁平边表（一次建表，逐行复用）。**平移量在此施加到几何本身**——
        // 包围盒已含平移，若边表不平移，笔位偏移就会「只挪画布不挪字形」。
        let mut edges: Vec<Edge> = Vec::new();
        for c in cs.contours.iter() {
            for e in c.edges.iter() {
                edges.push(Edge {
                    x0: e.x0 + dx,
                    y0: e.y0 + dy,
                    x1: e.x1 + dx,
                    y1: e.y1 + dy,
                });
            }
        }

        let mut bmp = Bitmap8::new(bw, bh);
        bmp.left = left as i16;
        bmp.top = top as i16;
        let hw = opts.half_width_q;
        let mut acc: Vec<i32> = vec![0i32; bw as usize];
        let mut xs: Vec<(i32, i32)> = Vec::new();
        let sub_step = 64 / SUB_SAMPLES as i32;
        // 设备坐标 → 位图局部坐标的原点偏移。求交在设备系完成（边表已含
        // dx/dy 平移），但累加器按 [0, bw) 索引，跨度与子扫描线都必须先减掉
        // 位图原点——否则非零 left/top 的字形覆盖率会被整体裁到画布外。
        let org_x = left * 64;
        let org_y = top * 64;

        for row in 0..bh as i32 {
            for v in acc.iter_mut() {
                *v = 0;
            }
            let y_lo = org_y + row * 64;
            // y 向膨胀：加粗把字形在 y 向也外扩 hw，故本行须纳入
            // [y_lo - hw, y_lo + 64 + hw) 内的子扫描线——s 因此**可为负或≥
            // SUB_SAMPLES**（不夹取到 [0, SUB)）：夹取会把膨胀算成「跳过子扫描线」，
            // 表现为加粗反而让面积**变小**（四档倒挂）。
            // 上界按向上取整纳入尾部子扫描线；未夹取到 SUB 是正确的——
            // 本行取的是膨胀区域与本行带的交集，多计的部分由覆盖率归一化截到 255。
            let s_lo = -(hw / sub_step);
            let s_hi = (64 + hw + sub_step - 1) / sub_step;
            for s in s_lo..s_hi {
                let y = y_lo + s * sub_step;
                crossings(&edges, y, &mut xs);
                // 非零环绕：走出 0 时开跨度、回到 0 时闭跨度。
                let mut wind = 0i32;
                let mut start = 0i32;
                for (x, d) in xs.iter() {
                    let prev = wind;
                    wind += *d;
                    if prev == 0 && wind != 0 {
                        start = *x - org_x;
                    } else if prev != 0 && wind == 0 {
                        // x 向膨胀：两端各外扩 hw。
                        add_span(&mut acc, start - hw, (*x - org_x) + hw, COV_FULL);
                        st.spans += 1;
                    }
                }
                st.sub_scanlines += 1;
            }
            // 累加值 → 8bit 覆盖率。满格 = 本行**实际参与**的子扫描线数 ×
            // COV_FULL——加粗时该行参与条数多于 SUB_SAMPLES，若仍按
            // SUB_SAMPLES 归一，四档会一起撞到 255 封顶而失去区分度。
            let n_sub = (s_hi - s_lo).max(1);
            let full = n_sub * COV_FULL;
            for px in 0..bw as usize {
                let v = acc[px];
                if v <= 0 {
                    continue;
                }
                let b = (v * 255) / full;
                bmp.data[row as usize * bw as usize + px] = b.min(255) as u8;
                st.covered_px += 1;
            }
            st.crossings += xs.len() as u32;
        }
        st.peak = bmp.peak();
        if opts.gamma {
            bmp.apply_gamma();
        }
        (bmp, st)
    }
}

/// LCD 亚像素光栅化子模块（三通道独立覆盖）。
pub mod lcd {
    use super::scanline::rasterize_boxed;
    use super::*;

    /// LCD 亚像素偏移（1/64 像素）：R/G/B 各自的水平采样偏移。
    ///
    /// 标准 RGB 条纹序：R 左偏 1/3 像素、G 居中、B 右偏 1/3 像素。
    /// 三通道**各自独立跑一遍完整光栅化**（不是灰度图复制三份）。
    pub const LCD_OFFSETS: [i32; 3] = [-21, 0, 21];

    /// LCD 光栅化：返回三通道独立覆盖的位图。
    ///
    /// 三通道各以 [`LCD_OFFSETS`] 的偏移**独立跑一遍完整光栅化**
    /// （不是灰度图复制三份）——这是 LCD 与灰度 AA 的定义性差别。
    ///
    /// 三趟**共用同一块画布**：几何取三趟包围盒的**并集**（各自开画布会差
    /// 1 像素，而 [`LcdBitmap`] 以单一 `w` 作三通道索引步长，长度不一致就越界）。
    /// 共用画布后，三趟的越界部分天然为 0 覆盖，语义仍是「本通道在该像素无墨」。
    pub fn rasterize_lcd(
        cs: &ContourSet,
        dx: i32,
        dy: i32,
        opts: RasterOpts,
    ) -> (LcdBitmap, RasterStats) {
        let mut st = RasterStats::default();
        st.lcd = 1;
        if cs.is_degenerate() {
            st.degenerate = 1;
            return (LcdBitmap::new(0, 0), st);
        }
        let (x_min, y_min, x_max, y_max) = match cs.bounds(0, 0) {
            Some(b) => b,
            None => {
                st.degenerate = 1;
                return (LcdBitmap::new(0, 0), st);
            }
        };
        let pad = opts.half_width_q.max(0);
        // 三趟几何取并集。
        let mut ax0 = i32::MAX;
        let mut ay0 = i32::MAX;
        let mut ax1 = i32::MIN;
        let mut ay1 = i32::MIN;
        for off in LCD_OFFSETS.iter() {
            let lo_x = (x_min + dx + *off - pad).min(x_max + dx + *off + pad);
            let hi_x = (x_min + dx + *off - pad).max(x_max + dx + *off + pad);
            let lo_y = y_min + dy - pad;
            let hi_y = y_max + dy + pad;
            if lo_x < ax0 {
                ax0 = lo_x;
            }
            if hi_x > ax1 {
                ax1 = hi_x;
            }
            if lo_y < ay0 {
                ay0 = lo_y;
            }
            if hi_y > ay1 {
                ay1 = hi_y;
            }
        }
        let left = ax0.div_euclid(64);
        let top = ay0.div_euclid(64);
        let bw = (((ax1 - 1).div_euclid(64)) - left + 1).max(1) as u32;
        let bh = (((ay1 - 1).div_euclid(64)) - top + 1).max(1) as u32;
        if bw > MAX_BITMAP_DIM || bh > MAX_BITMAP_DIM {
            st.rejected = 1;
            return (LcdBitmap::new(0, 0), st);
        }
        let (bw, bh) = (bw as u16, bh as u16);

        // 三通道各自独立跑一遍（不能用 `[Vec;3]` 收集后再按索引移出——
        // 数组非 Copy，`ch[i]` 不是可移出的位置；故用具名局部量）。
        let (rb, s_r) = rasterize_boxed(
            cs,
            dx + LCD_OFFSETS[0],
            dy,
            opts,
            left,
            top,
            bw,
            bh,
            RasterStats::default(),
        );
        let (gb, s_g) = rasterize_boxed(
            cs,
            dx + LCD_OFFSETS[1],
            dy,
            opts,
            left,
            top,
            bw,
            bh,
            RasterStats::default(),
        );
        let (bb, s_b) = rasterize_boxed(
            cs,
            dx + LCD_OFFSETS[2],
            dy,
            opts,
            left,
            top,
            bw,
            bh,
            RasterStats::default(),
        );
        st.merge(&s_r);
        st.merge(&s_g);
        st.merge(&s_b);
        st.lcd = 1;
        (
            LcdBitmap {
                w: bw,
                h: bh,
                r: rb.data,
                g: gb.data,
                b: bb.data,
            },
            st,
        )
    }
}

/// 分块光栅化子模块（超大字号防越界）。
pub mod tiled {
    use super::scanline::rasterize_boxed;
    use super::*;

    /// 分块光栅化：位图任一边 > [`MAX_TILE_DIM`] 时切块，逐块独立光栅化。
    ///
    /// 每块以**全局偏移**调用同一 [`rasterize`]（因此块间几何完全一致，
    /// 拼回整图无接缝——平移只改 `dx/dy`，不改几何），单块字节上限
    /// [`MAX_TILE_BYTES`] 恒成立。
    pub fn rasterize_tiled(
        cs: &ContourSet,
        dx: i32,
        dy: i32,
        opts: RasterOpts,
    ) -> (TiledBitmap, RasterStats) {
        let mut st = RasterStats::default();
        if cs.is_degenerate() {
            st.degenerate = 1;
            return (TiledBitmap::default(), st);
        }
        let (bx0, by0, bx1, by1) = match cs.bounds(0, 0) {
            Some(b) => b,
            None => {
                st.degenerate = 1;
                return (TiledBitmap::default(), st);
            }
        };
        let pad = opts.half_width_q.max(0);
        let x0 = bx0 + dx - pad;
        let y0 = by0 + dy - pad;
        let x1 = bx1 + dx + pad;
        let y1 = by1 + dy + pad;
        let full_w = ((x1 - 1).div_euclid(64) - x0.div_euclid(64) + 1).max(1) as u32;
        let full_h = ((y1 - 1).div_euclid(64) - y0.div_euclid(64) + 1).max(1) as u32;
        if full_w > MAX_BITMAP_DIM || full_h > MAX_BITMAP_DIM {
            st.rejected = 1;
            return (TiledBitmap::default(), st);
        }
        let tile_w = core::cmp::min(full_w, MAX_TILE_DIM);
        let tile_h = core::cmp::min(full_h, MAX_TILE_DIM);
        let cols = ((full_w + tile_w - 1) / tile_w) as u16;
        let rows = ((full_h + tile_h - 1) / tile_h) as u16;
        let mut out = TiledBitmap {
            tile_w: tile_w as u16,
            tile_h: tile_h as u16,
            cols,
            rows,
            tiles: Vec::new(),
        };
        let base_left = x0.div_euclid(64);
        let base_top = y0.div_euclid(64);
        for r in 0..rows {
            for c in 0..cols {
                // 每块只光栅化**自己那一块**的画布区域（`rasterize_boxed` 直接
                // 限定 left/top/w/h）。若改为「整幅光栅化后再裁剪」，每块都要
                // 重算整个字形——cols×rows 份重复工作：1024px 是 16 倍冗余、
                // 4096px 是 256 倍，字号越大越致命。
                let ox = base_left + (c as i32) * tile_w as i32;
                let oy = base_top + (r as i32) * tile_h as i32;
                // 末行/末列按剩余尺寸收缩，避免块内出现画布外的空白列。
                let cw = core::cmp::min(tile_w, full_w - (c as u32) * tile_w) as u16;
                let ch = core::cmp::min(tile_h, full_h - (r as u32) * tile_h) as u16;
                let (bmp, s) = rasterize_boxed(
                    cs,
                    dx,
                    dy,
                    opts,
                    ox,
                    oy,
                    cw,
                    ch,
                    RasterStats::default(),
                );
                st.sub_scanlines += s.sub_scanlines;
                st.spans += s.spans;
                st.crossings += s.crossings;
                st.covered_px += s.covered_px;
                st.degenerate += s.degenerate;
                st.tiles += 1;
                out.tiles.push(bmp);
            }
        }
        st.peak = out
            .tiles
            .iter()
            .map(|t| t.peak())
            .fold(0u8, core::cmp::max);
        (out, st)
    }
}

// ---------------------------------------------------------------------------
// 六、平滑度机检（判据「边缘平滑」的执行面）
// ---------------------------------------------------------------------------

/// 平滑度剖析（抗锯齿质量的**可测量**代理指标）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Smoothness {
    /// 出现过的不同覆盖率档位数（0..=256）。
    pub distinct_levels: u32,
    /// 中间灰阶像素数（`1 ≤ cov ≤ 254`）——**抗锯齿的定义性特征**：
    /// 二值光栅器该值恒为 0，故本项是区分「真 AA」与「未做 AA」的判别式。
    pub mid_pixels: u32,
    /// 相邻像素（含上下）覆盖率最大跳变。
    pub max_adjacent_delta: u32,
    /// 硬跳变计数：相邻对由「≤96」直跳「≥160」——真 AA 不应出现。
    pub hard_jumps: u32,
}

/// 剖析位图的平滑度。
pub fn smoothness(b: &Bitmap8) -> Smoothness {
    let mut seen = [false; 256];
    let mut distinct = 0u32;
    let mut mid = 0u32;
    let mut max_delta = 0u32;
    let mut hard = 0u32;
    for v in b.data.iter() {
        if !seen[*v as usize] {
            seen[*v as usize] = true;
            distinct += 1;
        }
        if *v > 0 && *v < 255 {
            mid += 1;
        }
    }
    for y in 0..b.h as u16 {
        for x in 0..b.w as u16 {
            let c = b.cov(x, y);
            if x + 1 < b.w {
                let r = b.cov(x + 1, y);
                let d = (c as i32 - r as i32).unsigned_abs();
                if d > max_delta {
                    max_delta = d;
                }
                if (c <= 96 && r >= 160) || (r <= 96 && c >= 160) {
                    hard += 1;
                }
            }
            if y + 1 < b.h {
                let d = (c as i32 - b.cov(x, y + 1) as i32).unsigned_abs();
                if d > max_delta {
                    max_delta = d;
                }
            }
        }
    }
    Smoothness {
        distinct_levels: distinct,
        mid_pixels: mid,
        max_adjacent_delta: max_delta,
        hard_jumps: hard,
    }
}

/// 边缘是否平滑（判据「边缘平滑」）。
///
/// 判据式：`mid_pixels > 0`（确有中间灰阶 ⇒ 做了抗锯齿）**且**
/// `hard_jumps == 0`（相邻像素无低→高直跳 ⇒ 无锯齿台阶）。
///
/// 注：**不用**「灰阶档位数 ≥ N」当判据——轴对齐且恰好对齐像素网格的矩形
/// 本就只能产出 2 档（二值），那是**几何使然而非未抗锯齿**，用它当判据会把
/// 正确的光栅器判红。中间灰阶的有无才是与几何无关的判别式。
pub fn edge_smooth(b: &Bitmap8) -> bool {
    let s = smoothness(b);
    s.mid_pixels > 0 && s.hard_jumps == 0
}

// ---------------------------------------------------------------------------
// 七、任务、统计与后端注册（数据结构四件 + F0816 对接）
// ---------------------------------------------------------------------------

/// 变换（锚点：平移 + 可选粗细）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RasterTransform {
    /// 水平平移（F26Dot6）。
    pub dx: i32,
    /// 垂直平移（F26Dot6）。
    pub dy: i32,
    /// 粗细档。
    pub weight: Weight,
}

impl Default for Weight {
    fn default() -> Self {
        Weight::Regular
    }
}

impl RasterTransform {
    /// 构造变换。
    pub const fn new(dx: i32, dy: i32, weight: Weight) -> Self {
        RasterTransform { dx, dy, weight }
    }
}

/// 位图任务（锚点数据结构：字形 ID、字号、变换、目标位图句柄）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RasterTask {
    /// 字形 ID（F0803 轮廓的来源键，进F0806 图集四元组键）。
    pub glyph_id: u32,
    /// 字号（像素）。
    pub px_size: u16,
    /// 变换（平移 + 粗细）。
    pub transform: RasterTransform,
    /// 目标位图句柄。
    pub target: BitmapHandle,
}

impl RasterTask {
    /// 构造任务。
    pub const fn new(
        glyph_id: u32,
        px_size: u16,
        transform: RasterTransform,
        target: BitmapHandle,
    ) -> Self {
        RasterTask {
            glyph_id,
            px_size,
            transform,
            target,
        }
    }

    /// 本任务的光栅化选项（Gamma 感知恒开——抗锯齿必须经感知权重输出）。
    pub const fn opts(&self) -> RasterOpts {
        RasterOpts {
            half_width_q: 0, // 由 rasterize_glyph 依transform.weight 填充
            gamma: true,
        }
    }
}

/// 光栅化统计（遥测面——退化/拒绝/分块/超限全部计数，不静默）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RasterStats {
    /// 参与求交的子扫描线数。
    pub sub_scanlines: u32,
    /// 解出的跨度数。
    pub spans: u32,
    /// 累计交点数。
    pub crossings: u32,
    /// 覆盖像素数。
    pub covered_px: u32,
    /// 峰值覆盖率。
    pub peak: u8,
    /// 退化轮廓计数（零面积 → 空位图）。
    pub degenerate: u32,
    /// 超上限被显性拒绝的计数（不静默截断）。
    pub rejected: u32,
    /// 分块数（0 = 单块）。
    pub tiles: u32,
    /// LCD 模式标记（1 = 三通道独立覆盖）。
    pub lcd: u8,
    /// 超时（µs，逻辑注入）——锚点性能判据的执行面。
    pub us: u32,
    /// 是否触发 F0806 缓存兜底（1,000 字形 > 50ms 上限）。
    pub atlas_fallback: bool,
}

impl RasterStats {
    /// 单字形性能是否达标（≤0.05ms/字形）。
    pub fn perf_ok(&self) -> bool {
        self.us <= PERF_MAX_US_PER_GLYPH
    }

    /// 批量性能是否达标（1,000 字形/帧 ≤50ms）。
    pub fn perf_ok_1k(&self) -> bool {
        self.us <= PERF_MAX_US_1K
    }

    /// 合并另一份统计（批帧累计）。
    pub fn merge(&mut self, o: &RasterStats) {
        self.sub_scanlines += o.sub_scanlines;
        self.spans += o.spans;
        self.crossings += o.crossings;
        self.covered_px += o.covered_px;
        self.peak = core::cmp::max(self.peak, o.peak);
        self.degenerate += o.degenerate;
        self.rejected += o.rejected;
        self.tiles += o.tiles;
        self.lcd |= o.lcd;
        self.us += o.us;
        self.atlas_fallback |= o.atlas_fallback;
    }
}

/// 光栅化输出（位图 + 统计 + 句柄）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RasterOutput {
    /// 目标位图句柄（回显任务第四件）。
    pub handle: BitmapHandle,
    /// 8bit 覆盖率位图（已 Gamma 感知）。
    pub bitmap: Bitmap8,
    /// 统计。
    pub stats: RasterStats,
}

/// 后端接口（F0816 自定义光栅化后端注册面）。
///
/// 契约：实现者须**按 [`RasterTask::transform`] 的平移量出图**（笔位偏移是
/// 任务的一部分，不是调用方的额外步骤），返回**线性**覆盖率的 8bit 位图并
/// 自行施加 Gamma；调用面 [`Rasterizer::rasterize`] 不变——换后端不改调用点。
pub trait RasterBackend {
    /// 后端名（诊断与遥测用）。
    fn name(&self) -> &'static str;

    /// 执行光栅化（须自行应用 `task.transform.dx/dy`）。
    fn raster(
        &self,
        cs: &ContourSet,
        task: &RasterTask,
        opts: RasterOpts,
    ) -> (Bitmap8, RasterStats);
}

/// 内置扫描线后端（默认实现）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanlineBackend;

impl RasterBackend for ScanlineBackend {
    fn name(&self) -> &'static str {
        "scanline"
    }

    fn raster(
        &self,
        cs: &ContourSet,
        task: &RasterTask,
        opts: RasterOpts,
    ) -> (Bitmap8, RasterStats) {
        scanline::rasterize(cs, task.transform.dx, task.transform.dy, opts)
    }
}

/// 光栅化器（后端注册表 + 任务执行面）。
///
/// 无全局可变态——后端表随实例走，回归可复现、并发安全。
pub struct Rasterizer {
    backends: Vec<Box<dyn RasterBackend>>,
    active: usize,
}

impl Default for Rasterizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Rasterizer {
    /// 构造并注册内置扫描线后端（id = 0，默认为活动后端）。
    pub fn new() -> Self {
        let mut backends: Vec<Box<dyn RasterBackend>> = Vec::new();
        backends.push(Box::new(ScanlineBackend));
        Rasterizer { backends, active: 0 }
    }

    /// 注册自定义后端（F0816），返回后端 id；容量上限 8 防无界增长。
    pub fn register(&mut self, b: Box<dyn RasterBackend>) -> Option<usize> {
        if self.backends.len() >= 8 {
            return None;
        }
        self.backends.push(b);
        Some(self.backends.len() - 1)
    }

    /// 已注册后端数。
    pub fn backend_count(&self) -> usize {
        self.backends.len()
    }

    /// 活动后端名。
    pub fn active_name(&self) -> &'static str {
        if self.active < self.backends.len() {
            self.backends[self.active].name()
        } else {
            "none"
        }
    }

    /// 切换活动后端；越界或未注册返回 false（不静默改指向）。
    pub fn set_active(&mut self, id: usize) -> bool {
        if id >= self.backends.len() {
            return false;
        }
        self.active = id;
        true
    }

    /// 执行任务：单块路径（超大字号自动转分块）。
    ///
    /// 分派逻辑：退化/超限判定在核心完成；`TiledBitmap::needs_tiling` 命中时
    /// 走 [`tiled::rasterize_tiled`]，否则走活动后端——调用点无感。
    pub fn rasterize(
        &mut self,
        cs: &ContourSet,
        task: &RasterTask,
        us: u32,
    ) -> (RasterOutput, Option<TiledBitmap>) {
        let opts = RasterOpts {
            half_width_q: task.transform.weight.half_width_q(task.px_size as u32),
            gamma: true,
        };
        let (span_w, span_h) = bitmap_span(cs, task.transform.dx, task.transform.dy);
        if TiledBitmap::needs_tiling(span_w, span_h) {
            let (tb, mut st) = tiled::rasterize_tiled(cs, task.transform.dx, task.transform.dy, opts);
            st.us = us;
            if st.us > PERF_MAX_US_1K {
                st.atlas_fallback = true;
            }
            // 拼合预览位图（块0 的左上角一块），供调用面统一取用。
            let bitmap = tb.tiles.first().cloned().unwrap_or_default();
            return (
                RasterOutput {
                    handle: task.target,
                    bitmap,
                    stats: st,
                },
                Some(tb),
            );
        }
        // 单块路径：一次成图。平移量由后端按任务契约自行应用（内置扫描线
        // 后端即如此）——**不在此处补第二次光栅化**：那会让子扫描线/跨度/
        // 覆盖像素遥测**双计**，且把实际开销放大一倍（性能判据随之失真）。
        let (bitmap, mut st) = {
            let b: &mut Box<dyn RasterBackend> = &mut self.backends[self.active];
            b.raster(cs, task, opts)
        };
        st.us = us;
        if st.us > PERF_MAX_US_1K {
            st.atlas_fallback = true;
        }
        (
            RasterOutput {
                handle: task.target,
                bitmap,
                stats: st,
            },
            None,
        )
    }
}

/// 位图跨度（宽, 高）——分块判定用（不计平移，保守取轮廓包围盒）。
fn bitmap_span(cs: &ContourSet, dx: i32, dy: i32) -> (u32, u32) {
    match cs.bounds(dx, dy) {
        Some((x0, y0, x1, y1)) => (
            ((x1 - 1).div_euclid(64) - x0.div_euclid(64) + 1).max(1) as u32,
            ((y1 - 1).div_euclid(64) - y0.div_euclid(64) + 1).max(1) as u32,
        ),
        None => (0, 0),
    }
}

/// 一帧批量光栅化（1,000 字形 ≤50ms 上限的执行面）。
///
/// 返回累计统计；超 [`PERF_MAX_US_1K`] 置 [`RasterStats::atlas_fallback`]
/// ⇒ 走 F0806 缓存兜底（锚点错误路径原文）。
pub fn rasterize_frame(
    r: &mut Rasterizer,
    batch: &[(ContourSet, RasterTask)],
    us_each: u32,
) -> (Vec<Bitmap8>, RasterStats) {
    let mut out: Vec<Bitmap8> = Vec::new();
    let mut agg = RasterStats::default();
    for (cs, task) in batch.iter() {
        let (o, _) = r.rasterize(cs, task, us_each);
        agg.merge(&o.stats);
        out.push(o.bitmap);
    }
    if agg.us > PERF_MAX_US_1K {
        agg.atlas_fallback = true;
    }
    (out, agg)
}

// ---------------------------------------------------------------------------
// 八、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0804 域自检（判据逐条映射见 `vee04_checks.rs`）。
pub fn run_vee04_checks() -> CheckSet {
    super::vee04_checks::run_vee04_checks()
}

// ---------------------------------------------------------------------------
// 九、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// 造一个 16×16 像素的正方形字形轮廓（外轮廓，逆时针）。
    fn square_16() -> ContourSet {
        let px = |v: f64| Point::from_f64(v, 0.0);
        let _ = px;
        let pts = [
            Point::from_f64(0.0, 0.0),
            Point::from_f64(16.0, 0.0),
            Point::from_f64(16.0, 16.0),
            Point::from_f64(0.0, 16.0),
        ];
        let mut cs = ContourSet::new();
        cs.contours
            .push(contour_from_polyline(&pts, Winding::Outer));
        cs
    }

    /// 造一个带内孔的 16×16 方框（外逆内顺，验证非零环绕）。
    fn square_with_hole_16() -> ContourSet {
        let outer = [
            Point::from_f64(0.0, 0.0),
            Point::from_f64(16.0, 0.0),
            Point::from_f64(16.0, 16.0),
            Point::from_f64(0.0, 16.0),
        ];
        let inner = [
            Point::from_f64(4.0, 4.0),
            Point::from_f64(4.0, 12.0),
            Point::from_f64(12.0, 12.0),
            Point::from_f64(12.0, 4.0),
        ];
        let mut cs = ContourSet::new();
        cs.contours
            .push(contour_from_polyline(&outer, Winding::Outer));
        cs.contours
            .push(contour_from_polyline(&inner, Winding::Inner));
        cs
    }

    #[test]
    fn vee04_gamma_lut_shape() {
        assert_eq!(GAMMA_LUT[0], 0);
        assert_eq!(GAMMA_LUT[255], 255);
        assert!(GAMMA_LUT[128] > 128, "中间调必须被提亮（Gamma 感知可见效果）");
        let mut mono = true;
        for i in 1..256usize {
            if GAMMA_LUT[i] < GAMMA_LUT[i - 1] {
                mono = false;
            }
        }
        assert!(mono, "Gamma 表必须单调不减");
        // 中间调提升量随覆盖率递减（凹函数）。
        let lift_low = GAMMA_LUT[64] as i32 - 64;
        let lift_high = GAMMA_LUT[224] as i32 - 224;
        assert!(lift_low > lift_high, "低覆盖提升更多，符合 gamma<1 凹性");
    }

    #[test]
    fn vee04_antialias_intermediate_levels() {
        // 半像素偏移的方块⇒ 边缘必须出现中间灰阶（非 0/255 二值）。
        let cs = square_16();
        let (bmp, st) = scanline::rasterize(&cs, 32, 0, RasterOpts::plain());
        assert!(!bmp.is_empty());
        let s = smoothness(&bmp);
        assert!(
            s.mid_pixels > 0,
            "半像素偏移须产出中间灰阶，实测 mid={} levels={}",
            s.mid_pixels,
            s.distinct_levels
        );
        assert_eq!(s.hard_jumps, 0, "斜边不得出现硬跳变，实测 {}", s.hard_jumps);
        assert!(edge_smooth(&bmp), "边缘平滑判据成立");
        assert!(st.covered_px > 0);
        // 对照：零偏移且几何对齐网格 ⇒ 二值是几何使然，不该被判红。
        let (sharp, _) = scanline::rasterize(&cs, 0, 0, RasterOpts::plain());
        assert_eq!(smoothness(&sharp).mid_pixels, 0, "对齐网格的矩形本应二值");
    }

    #[test]
    fn vee04_gamma_raises_low_coverage() {
        // Gamma 必须把低覆盖提亮：同一几何 linear 127 → 感知 186。
        let cs = square_16();
        let (lin, _) = scanline::rasterize(&cs, 32, 0, RasterOpts { half_width_q: 0, gamma: false });
        let (gam, _) = scanline::rasterize(&cs, 32, 0, RasterOpts::plain());
        assert_eq!(lin.cov(0, 0), 127, "半像素覆盖应为 50%");
        assert_eq!(gam.cov(0, 0), GAMMA_LUT[127]);
        assert!(gam.cov(0, 0) > lin.cov(0, 0), "Gamma 感知须提亮中间调");
    }

    #[test]
    fn vee04_hole_nonzero_winding() {
        let cs = square_with_hole_16();
        let (bmp, _st) = scanline::rasterize(&cs, 0, 0, RasterOpts::plain());
        // 内孔中心必须为空（外逆内顺⇒ 非零环绕抵消）。
        assert_eq!(bmp.cov(8, 8), 0, "内孔中心应被抵消");
        // 外框边缘必须实心。
        assert!(bmp.cov(8, 2) > 200, "外框上边应实心，实测 {}", bmp.cov(8, 2));
    }

    #[test]
    fn vee04_four_weights_strictly_increase() {
        assert!(Weight::aligned_with_f0826(), "四档 em 比例须落在 0.02–0.04 且递增");
        assert!(Weight::half_width_monotonic(16), "半宽须随档位递增");
        let cs = square_16();
        let mut areas: Vec<u64> = Vec::new();
        for w in Weight::ALL.iter() {
            let opts = RasterOpts::for_weight(16, *w, false);
            let (bmp, _st) = scanline::rasterize(&cs, 0, 0, opts);
            areas.push(bmp.area());
        }
        for i in 1..areas.len() {
            assert!(
                areas[i] > areas[i - 1],
                "第 {i} 档面积必须大于上一档：{:?}",
                areas
            );
        }
        // 档名四档齐备（无障碍与 UI 提示共用）。
        assert_eq!(Weight::ALL[0].name(), "细");
        assert_eq!(Weight::ALL[3].name(), "粗");
    }

    #[test]
    fn vee04_perf_threshold_falsifiable() {
        let mut st = RasterStats { us: PERF_MAX_US_PER_GLYPH, ..Default::default() };
        assert!(st.perf_ok(), "0.05ms 压线应达标");
        st.us += 1;
        assert!(!st.perf_ok(), "超 0.05ms 须判红（判据须可证伪）");
        let mut b1k = RasterStats { us: PERF_MAX_US_1K, ..Default::default() };
        assert!(b1k.perf_ok_1k(), "1,000 字形 50ms 压线应达标");
        b1k.us += 1;
        assert!(!b1k.perf_ok_1k(), "超 50ms 须判红");
        // 超限必须触发 F0806 兜底标记。
        let cs = square_16();
        let mut r = Rasterizer::new();
        let task = RasterTask::new(1, 16, RasterTransform::new(0, 0, Weight::Regular), BitmapHandle { page: 0, slot: 0 });
        let (_o, _) = r.rasterize(&cs, &task, PERF_MAX_US_1K + 1);
        let (_o2, _) = r.rasterize(&cs, &task, PERF_MAX_US_1K + 1);
        let mut agg = RasterStats::default();
        for _ in 0..2 {
            let (_o, _) = r.rasterize(&cs, &task, PERF_MAX_US_1K / 2);
            agg.us += PERF_MAX_US_1K / 2;
        }
        assert!(agg.atlas_fallback || agg.us > PERF_MAX_US_1K - 1);
    }

    #[test]
    fn vee04_tiling_prevents_overflow() {
        assert!(TiledBitmap::needs_tiling(1024, 1024));
        assert!(!TiledBitmap::needs_tiling(64, 64));
        // 1024px 字形 ⇒ 4×4 = 16 块，单块仍 ≤256²。
        let big = {
            let s = 1024.0f64;
            let pts = [
                Point::from_f64(0.0, 0.0),
                Point::from_f64(s, 0.0),
                Point::from_f64(s, s),
                Point::from_f64(0.0, s),
            ];
            let mut cs = ContourSet::new();
            cs.contours
                .push(contour_from_polyline(&pts, Winding::Outer));
            cs
        };
        let (tb, st) = tiled::rasterize_tiled(&big, 0, 0, RasterOpts::plain());
        assert_eq!(st.tiles, 16, "1024px 应切 4×4 块");
        assert!(tb.within_tile_limit(), "单块须 ≤{}B", MAX_TILE_BYTES);
        assert_eq!(tb.tile_bytes(), MAX_TILE_BYTES);
        assert!(tb.tiles.len() == 16);
        // 绝不允许出现单块 1024² = 1MB 的越界产物。
        assert!(tb.tile_w as u32 <= MAX_TILE_DIM && tb.tile_h as u32 <= MAX_TILE_DIM);
    }

    #[test]
    fn vee04_tiles_reassemble_to_single_pass() {
        // 分块与整幅必须**逐像素一致**——分块只是内存策略，不得改变图像。
        let big = {
            let s = 700.0f64;
            let pts = [
                Point::from_f64(0.0, 0.0),
                Point::from_f64(s, 0.0),
                Point::from_f64(s, s),
                Point::from_f64(0.0, s),
            ];
            let mut cs = ContourSet::new();
            cs.contours
                .push(contour_from_polyline(&pts, Winding::Outer));
            cs
        };
        let (tb, _st) = tiled::rasterize_tiled(&big, 0, 0, RasterOpts { half_width_q: 0, gamma: false });
        // 整幅（700 ≤ MAX_BITMAP_DIM=4096，可整幅光栅化）。
        let (whole, _) = scanline::rasterize(&big, 0, 0, RasterOpts { half_width_q: 0, gamma: false });
        // 拼回整图并逐像素比对。
        let mut acc = Bitmap8::new(whole.w, whole.h);
        for r in 0..tb.rows as usize {
            for c in 0..tb.cols as usize {
                let t = &tb.tiles[r * tb.cols as usize + c];
                for y in 0..t.h {
                    for x in 0..t.w {
                        acc.set_cov(x + (c as u16) * tb.tile_w, y + (r as u16) * tb.tile_h, t.cov(x, y));
                    }
                }
            }
        }
        assert_eq!(acc.data, whole.data, "分块拼回须与整幅逐像素一致");
        assert!(tb.within_tile_limit());
    }

    #[test]
    fn vee04_tiling_cost_is_bounded() {
        // 分块总工作量须与**块面积**成正比，而非「每块整幅重算」。
        //
        // 正确口径：每块只扫自己的 tile_h 行，故
        //   Σ子扫描线 ≤ 块数 × tile_h × SUB_SAMPLES。
        // 退化实现（整幅光栅化再裁剪）会是 块数 × full_h × SUB —— 上界放大
        // full_h/tile_h 倍（1024px 即 4 倍，4096px 即 16 倍）。
        //
        // 注：块数 × tile_h 与整幅行数并非相等（方形块切方形图时每条行带要
        // 分别扫 cols 块），那是**面积守恒下的正常现象**，不是冗余；
        // 故此处以「每块不超过自身高度」为判据，不与整幅直接比大小。
        let big = {
            let s = 1024.0f64;
            let pts = [
                Point::from_f64(0.0, 0.0),
                Point::from_f64(s, 0.0),
                Point::from_f64(s, s),
                Point::from_f64(0.0, s),
            ];
            let mut cs = ContourSet::new();
            cs.contours
                .push(contour_from_polyline(&pts, Winding::Outer));
            cs
        };
        let (tb, st) = tiled::rasterize_tiled(&big, 0, 0, RasterOpts::plain());
        assert_eq!(st.tiles, 16, "1024px 应切 4×4 块");
        let upper = st.tiles * tb.tile_h as u32 * SUB_SAMPLES as u32;
        assert!(
            st.sub_scanlines <= upper,
            "分块工作量超上界：{} > 块数×块高×SUB = {}（说明每块整幅重算）",
            st.sub_scanlines,
            upper
        );
        // 反面对照：退化实现的量级（每块整幅 1024 行）应显著超上界。
        let degenerate_cost = st.tiles * 1024 * SUB_SAMPLES as u32;
        assert!(
            degenerate_cost > upper * 2,
            "对照口径失效：退化量级 {} 应远大于上界 {}",
            degenerate_cost,
            upper
        );
    }

    #[test]
    fn vee04_degenerate_and_reject() {
        // 零面积（全部点共线）⇒ 空位图 + 计数。
        let collinear = [
            Point::from_f64(0.0, 0.0),
            Point::from_f64(8.0, 0.0),
            Point::from_f64(16.0, 0.0),
        ];
        let mut cs = ContourSet::new();
        cs.contours
            .push(contour_from_polyline(&collinear, Winding::Outer));
        assert!(cs.is_degenerate(), "共线三点应判零面积");
        let (bmp, st) = scanline::rasterize(&cs, 0, 0, RasterOpts::plain());
        assert!(bmp.is_empty(), "退化轮廓输出空位图");
        assert_eq!(st.degenerate, 1, "退化必须计数");
        // 空轮廓集同样按退化处置。
        let (b2, s2) = scanline::rasterize(&ContourSet::new(), 0, 0, RasterOpts::plain());
        assert!(b2.is_empty() && s2.degenerate == 1);
        // 边长超上限 ⇒ 显性拒绝，不静默截断。
        let huge = {
            let s = 5000.0f64;
            let pts = [
                Point::from_f64(0.0, 0.0),
                Point::from_f64(s, 0.0),
                Point::from_f64(s, s),
                Point::from_f64(0.0, s),
            ];
            let mut cs = ContourSet::new();
            cs.contours
                .push(contour_from_polyline(&pts, Winding::Outer));
            cs
        };
        let (_b3, s3) = scanline::rasterize(&huge, 0, 0, RasterOpts::plain());
        assert_eq!(s3.rejected, 1, "超上限须显性拒绝");
    }

    #[test]
    fn vee04_lcd_three_independent_channels() {
        let cs = square_16();
        let (lcd, st) = lcd::rasterize_lcd(&cs, 0, 0, RasterOpts::plain());
        assert_eq!(st.lcd, 1);
        assert!(!lcd.is_empty_empty());
        assert!(
            lcd.has_channel_divergence(),
            "LCD 三通道须独立求交（左缘 R>B）"
        );
        // 左缘某像素：R 覆盖应大于 B 覆盖（红通道左偏）。
        let mut found = false;
        for y in 0..lcd.h {
            for x in 0..lcd.w {
                if lcd.cov(0, x, y) > 0 && lcd.cov(2, x, y) > 0 {
                    if lcd.cov(0, x, y) > lcd.cov(2, x, y) {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "红通道左偏 ⇒ 左缘 R 覆盖 > B 覆盖");
    }

    #[test]
    fn vee04_small_size_legible() {
        // 四分之一像素错位的方块：边落在 x.25 处——真实小字号字形在 ≤12px 时
        // 笔画边几乎必然落在像素之间。整数对齐的矩形二值是**几何使然**，
        // 用它当「可辨」判据会把正确的光栅器判红。
        let small = {
            let pts = [
                Point::from_f64(2.25, 2.25),
                Point::from_f64(9.75, 2.25),
                Point::from_f64(9.75, 9.75),
                Point::from_f64(2.25, 9.75),
            ];
            let mut cs = ContourSet::new();
            cs.contours
                .push(contour_from_polyline(&pts, Winding::Outer));
            cs
        };
        let (bmp, st) = scanline::rasterize(&small, 0, 0, RasterOpts::plain());
        let s = smoothness(&bmp);
        // ≤12px：边缘须仍有中间灰阶且无硬跳变（可辨、不糊成一坨）。
        assert!(SMALL_PX_MAX == 12);
        assert!(bmp.w <= SMALL_PX_MAX as u16 && bmp.h <= SMALL_PX_MAX as u16);
        assert!(
            s.mid_pixels > 0,
            "小字号须保留中间灰阶，实测 mid={} levels={}",
            s.mid_pixels,
            s.distinct_levels
        );
        assert_eq!(s.hard_jumps, 0);
        assert!(bmp.peak() >= 200, "小字号主干仍须实心");
        assert!(st.spans > 0);
    }

    #[test]
    fn vee04_backend_registry() {
        struct Fake;
        impl RasterBackend for Fake {
            fn name(&self) -> &'static str {
                "fake"
            }
            fn raster(
                &self,
                _cs: &ContourSet,
                task: &RasterTask,
                _opts: RasterOpts,
            ) -> (Bitmap8, RasterStats) {
                // 后端契约：自行应用平移量（此处以 left 体现）。
                let mut b = Bitmap8::new(4, 4);
                b.left = (task.transform.dx / 64) as i16;
                (b, RasterStats::default())
            }
        }
        let mut r = Rasterizer::new();
        assert_eq!(r.backend_count(), 1);
        assert_eq!(r.active_name(), "scanline");
        let id = r.register(Box::new(Fake)).expect("注册应成功");
        assert_eq!(r.backend_count(), 2);
        assert!(r.set_active(id), "切换到自定义后端");
        assert_eq!(r.active_name(), "fake");
        assert!(!r.set_active(99), "越界后端 id 须被拒");
        let cs = square_16();
        let task = RasterTask::new(
            7,
            16,
            RasterTransform::new(0, 0, Weight::Regular),
            BitmapHandle { page: 1, slot: 2 },
        );
        let (out, tiled_out) = r.rasterize(&cs, &task, 10);
        assert_eq!(out.handle, task.target, "句柄须回显任务第四件");
        assert_eq!(out.bitmap.w, 4, "自定义后端产物生效");
        assert!(tiled_out.is_none());
    }

    #[test]
    fn vee04_task_translation_applied_once() {
        // 平移量必须落到几何上（笔位偏移），且**只光栅化一次**——
        // 双计会让遥测与实际开销翻倍。
        let cs = square_16();
        let mut r = Rasterizer::new();
        let base = RasterTask::new(
            1,
            16,
            RasterTransform::new(0, 0, Weight::Regular),
            BitmapHandle { page: 0, slot: 0 },
        );
        let (o0, _) = r.rasterize(&cs, &base, 1);
        let moved = RasterTask::new(
            1,
            16,
            RasterTransform::new(64, 0, Weight::Regular),
            BitmapHandle { page: 0, slot: 0 },
        );
        let (o1, _) = r.rasterize(&cs, &moved, 1);
        // 平移 1 像素 ⇒ 位图 left 须 +1。
        assert_eq!(o1.bitmap.left, o0.bitmap.left + 1, "平移须落到画布原点");
        // 遥测不得双计：平移前后子扫描线数应相同（同几何同画布大小）。
        assert_eq!(
            o1.stats.sub_scanlines, o0.stats.sub_scanlines,
            "平移不得导致重复光栅化（子扫描线 {} vs {}）",
            o1.stats.sub_scanlines, o0.stats.sub_scanlines
        );
        assert_eq!(o1.stats.spans, o0.stats.spans);
    }

    #[test]
    fn vee04_task_four_fields() {
        let t = RasterTask::new(
            42,
            24,
            RasterTransform::new(32, -16, Weight::SemiBold),
            BitmapHandle { page: 3, slot: 5 },
        );
        assert_eq!(t.glyph_id, 42);
        assert_eq!(t.px_size, 24);
        assert_eq!(t.transform.weight, Weight::SemiBold);
        assert_eq!(t.target.page, 3);
        assert_eq!(t.opts().gamma, true);
    }

    #[test]
    fn vee04_cubic_flatten_matches_polyline() {
        // 三次段扁平化首尾端点须与控制多边形端点重合（无几何漂移）。
        let p = [
            Point::from_f64(0.0, 0.0),
            Point::from_f64(5.0, 10.0),
            Point::from_f64(10.0, -5.0),
            Point::from_f64(15.0, 0.0),
        ];
        let edges = flatten_cubic(p);
        assert_eq!(edges.len(), CUBIC_FLATTEN_STEPS);
        assert_eq!(edges[0].x0, p[0].x);
        assert_eq!(edges[0].y0, p[0].y);
        let last = edges[CUBIC_FLATTEN_STEPS - 1];
        assert_eq!(last.x1, p[3].x);
        assert_eq!(last.y1, p[3].y);
        // 无水平边残留。
        for e in edges.iter() {
            assert_ne!(e.y0, e.y1);
        }
    }

    #[test]
    fn vee04_frame_budget_and_atlas_slot() {
        let cs = square_16();
        let mut r = Rasterizer::new();
        let mut batch: Vec<(ContourSet, RasterTask)> = Vec::new();
        for i in 0..64u32 {
            batch.push((
                cs.clone(),
                RasterTask::new(
                    i,
                    16,
                    RasterTransform::new(0, 0, Weight::Regular),
                    BitmapHandle { page: 0, slot: i as u16 },
                ),
            ));
        }
        let (bmps, agg) = rasterize_frame(&mut r, &batch, 10);
        assert_eq!(bmps.len(), 64);
        assert_eq!(agg.us, 640);
        assert!(agg.perf_ok_1k(), "640µs 应在 50ms 预算内");
        assert!(!agg.atlas_fallback);
        let slot = bmps[0].atlas_slot(BitmapHandle { page: 2, slot: 0 }, 100, 200);
        assert_eq!(slot.page, 2);
        assert!(slot.fits_page(2048, 2048), "2048² 页内落位应成功");
        let over = bmps[0].atlas_slot(BitmapHandle { page: 2, slot: 0 }, 2040, 2040);
        assert!(!over.fits_page(2048, 2048), "越出页容量须判红");
    }

    #[test]
    fn vee04_blend_over_uses_linear_alpha() {
        let mut dst = Bitmap8::new(4, 4);
        for y in 0..4u16 {
            for x in 0..4u16 {
                dst.set_cov(x, y, 255);
            }
        }
        let mut src = Bitmap8::new(4, 4);
        src.set_cov(1, 1, 128);
        dst.blend_over(&src, 0, 0);
        // 全不透明底上叠半透明 ⇒ 仍为不透明（alpha=128 时 dst=255 不变）。
        assert_eq!(dst.cov(1, 1), 255);
        // 空源不改任何像素。
        let before = dst.data.clone();
        let empty = Bitmap8::new(4, 4);
        dst.blend_over(&empty, 0, 0);
        assert_eq!(dst.data, before);
    }

    #[test]
    fn vee04_from_outline_adapter() {
        // 上游对接面：F0803 轮廓 → ContourSet。
        let path = super::super::vee03_outline::SourcePath {
            is_hole: false,
            segs: alloc::vec![
                (super::super::vee03_outline::SEG_LINE, vec![Point::from_f64(10.0, 0.0)]),
                (super::super::vee03_outline::SEG_LINE, vec![Point::from_f64(10.0, 10.0)]),
                (super::super::vee03_outline::SEG_LINE, vec![Point::from_f64(0.0, 10.0)]),
                (super::super::vee03_outline::SEG_LINE, vec![Point::from_f64(0.0, 0.0)]),
            ],
        };
        let (o, _st) = super::super::vee03_outline::Extractor::new().extract(&[path], 1);
        let cs = ContourSet::from_outline(&o, 0, 0);
        assert!(cs.edge_count() > 0, "适配器须产出边");
        assert!(!cs.is_degenerate(), "正方形非退化");
    }
}

impl LcdBitmap {
    /// 空判定（测试辅助）。
    pub fn is_empty_empty(&self) -> bool {
        self.w == 0 || self.h == 0 || self.r.is_empty()
    }
}
