//! VE-F1626 · 裁剪空间约定（VE-F 域 · 几何批 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1626`
//!
//! **判据（锚点原文）**：NDC 封装、深度精度、裁剪声明、判据。
//!
//! **职责定位（锚点原文）**：NDC 约定统一（深度范围 Z 0-1/-1-1 的后端
//! 差异封装——跨后端一致语义：投影矩阵适配——后端差异在投影矩阵吸收
//! ——上层零感知）；深度精度（非线性深度——近处精度高远处低——精度
//! 优化建议：反转 Z（F1522 时代主流实践——建议而非强制）/对数深度）；
//! 裁剪行为声明（图元裁剪/顶点钳制的统一语义）。
//!
//! # 一、NDC 深度范围差异为什么封装在投影矩阵而不在上层
//!
//! D3D 系把 NDC 深度映射到 [0,1]，OpenGL 系映射到 [-1,1]——差异是
//! 后端的历史包袱，不该让每个上层调用者写 if。本模块给出两个深度
//! 约定的封闭枚举与**深度行适配向量**：换后端=换投影矩阵的深度行，
//! 上层语义深度零感知。适配是仿射的（z' = 2z − 1 或其逆），整数
//! 万分位可精确表达，零浮点。
//!
//! # 二、深度精度为什么「建议而非强制」
//!
//! 非线性深度分布（1/z）意味着近处分辨率高、远处指数级劣化——这是
//! 投影数学的固有属性。反转 Z 是主流实践但依赖后端支持（深度比较
//! 方向与清除值都要跟着换），对数深度牺牲线性换均匀。本模块给出
//! **建议结构体**（含理由与代价），绝不替用户改状态——建议不是强制，
//! 强制就越权（域边界纪律）。
//!
//! # 三、裁剪行为为什么是声明不是探测
//!
//! 图元裁剪（NDC 外的图元被裁剪管线处理）与顶点钳制（越界深度值被
//! 钳到范围）是两种后端行为，运行期探测既慢又不可靠——行为是
//! **编译期/初始化期声明**：后端在接入时声明自己的行为，上层按
//! 声明语义适配。声明封闭两态，表外拒绝。
//!
//! # 四、对接
//!
//! 上游 F1625 双侧渲染（同一管线约定）、F1522 渲染架构（反转 Z 的
//! 时代语境）；下游 F1627 顶点流输出、F1629 批处理（同一裁剪语义）。
//! 整数口径：万分位（1.0 = 10000），零浮点零 panic 面。

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、NDC 封装（判据一：深度范围差异在投影矩阵吸收）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const CLIP_CONV_VERSION: &str = "F29-clip-v1";

/// NDC 深度范围约定（封闭二态——表外拒绝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthRange {
    /// D3D 系：NDC 深度 ∈ [0,1]。
    ZeroToOne,
    /// OpenGL 系：NDC 深度 ∈ [-1,1]。
    NegOneToOne,
}

impl DepthRange {
    /// 人话名（读屏）。
    pub fn say(self) -> &'static str {
        match self {
            DepthRange::ZeroToOne => "D3D 系 [0,1]",
            DepthRange::NegOneToOne => "OpenGL 系 [-1,1]",
        }
    }
}

/// 万分位标度（1.0 = 10000）。
pub const DEPTH_SCALE: i64 = 10_000;

/// NDC→NDC 深度仿射（z' = k·z + b，万分位整数；两约定互逆）。
///
/// ZeroToOne→NegOneToOne：z' = 2z − 1（k=20000, b=−10000）；
/// NegOneToOne→ZeroToOne：z = (z'+1)/2（k=5000, b=5000）。
pub fn depth_affine(from: DepthRange, to: DepthRange) -> (i64, i64) {
    match (from, to) {
        (DepthRange::ZeroToOne, DepthRange::NegOneToOne) => (2 * DEPTH_SCALE, -DEPTH_SCALE),
        (DepthRange::NegOneToOne, DepthRange::ZeroToOne) => (DEPTH_SCALE / 2, DEPTH_SCALE / 2),
        _ => (DEPTH_SCALE, 0), // 同约定恒等
    }
}

/// 单点深度换算（定点：z 以万分位给出，越域输入原样钳回本约定域）。
pub fn convert_ndc_depth(z_per10k: i64, from: DepthRange, to: DepthRange) -> i64 {
    let (k, b) = depth_affine(from, to);
    let out = (z_per10k * k + b * DEPTH_SCALE) / DEPTH_SCALE;
    clamp_to_range(out, to)
}

/// 按约定域钳制（[0,1] 或 [-1,1]，万分位）。
pub fn clamp_to_range(z_per10k: i64, r: DepthRange) -> i64 {
    match r {
        DepthRange::ZeroToOne => z_per10k.clamp(0, DEPTH_SCALE),
        DepthRange::NegOneToOne => z_per10k.clamp(-DEPTH_SCALE, DEPTH_SCALE),
    }
}

/// 投影矩阵深度行适配（判据核心：后端差异在投影矩阵吸收——上层零感知）。
///
/// 输入为「当前矩阵深度行」四元素（万分位，行作用方式 row·[x,y,z,w]），
/// 输出为「目标约定下的深度行」。深度行只与 z/w 有关：新行 = 仿射作用于
/// 旧行的输出，即 k·(row) + b·w 行 → row' = k·row + b·[0,0,0,1]。
pub fn adapt_depth_row(
    row: [i64; 4],
    from: DepthRange,
    to: DepthRange,
) -> [i64; 4] {
    let (k, b) = depth_affine(from, to);
    [
        (row[0] * k) / DEPTH_SCALE,
        (row[1] * k) / DEPTH_SCALE,
        (row[2] * k) / DEPTH_SCALE,
        (row[3] * k + b * DEPTH_SCALE) / DEPTH_SCALE,
    ]
}

/// 上层零感知声明（字面量冻结——判据独立对拍）。
pub const PROJECTION_ADAPTS_NOTICE: &str =
    "后端深度范围差异在投影矩阵深度行吸收——上层语义深度零感知";

// ---------------------------------------------------------------------------
// 二、深度精度（判据二：非线性分布与优化建议）
// ---------------------------------------------------------------------------

/// 近平面距离（视空间单位，万分位）。
pub const DEFAULT_NEAR_PER10K: i64 = 100;
/// 远平面距离（万分位）。
pub const DEFAULT_FAR_PER10K: i64 = 1_000_000;

/// 给定 NDC 深度的眼空间深度分辨率（万分位整数近似）。
///
/// 非线性深度 z_ndc ∝ 1/z_eye：分辨率 ≈ z_eye² / (a·2^bits)，其中
/// a = f/(f−n)（ZeroToOne 系数）。整数近似：z_eye = n·f/(f − z_ndc·(f−n))，
/// 全程 u128 防溢出。返回该深度处「最小可分辨的眼空间步长」。
pub fn depth_resolution_at(
    z_ndc_per10k: i64,
    near: i64,
    far: i64,
    depth_bits: u32,
) -> i64 {
    let bits: i128 = 1i128 << depth_bits.min(24);
    // z_eye = n*f / (f - z*(f-n))，z 为 NDC（万分位 → 归一处理全程万分位缩放）
    let denom = far * DEPTH_SCALE - z_ndc_per10k * (far - near);
    if denom <= 0 {
        return i64::MAX; // 超远平面：分辨率无意义
    }
    let z_eye = (near as i128 * far as i128 * DEPTH_SCALE as i128) / denom as i128;
    // 分辨率 ≈ z_eye² / (a·2^bits)，a = f/(f−n) ≈ 万分位直接除
    let a_num = far as i128;
    let a_den = (far - near).max(1) as i128;
    let res = z_eye * z_eye * a_den / (a_num * bits * DEPTH_SCALE as i128);
    res.min(i64::MAX as i128) as i64
}

/// 精度分布表（近/中/远三点——分布非线性的可机检证据）。
pub fn precision_profile(near: i64, far: i64, bits: u32) -> [(String, i64); 3] {
    let z01 = near * DEPTH_SCALE / (near + far);
    let zmid = (z01 + DEPTH_SCALE) / 2; // NDC 线性中点
    [
        ("近".to_string(), depth_resolution_at(z01, near, far, bits)),
        ("中".to_string(), depth_resolution_at(zmid, near, far, bits)),
        ("远".to_string(), depth_resolution_at(DEPTH_SCALE, near, far, bits)),
    ]
}

/// 深度行适配往返验证（工程自检：from→to→from 须回到原行）。
pub fn depth_row_roundtrip(row: [i64; 4], a: DepthRange, b: DepthRange) -> bool {
    let ab = adapt_depth_row(row, a, b);
    let ba = adapt_depth_row(ab, b, a);
    // 整数除法容差：每元素 ≤1（万分位下 1 = 0.0001）
    row.iter()
        .zip(ba.iter())
        .all(|(x, y)| (x - y).abs() <= 1)
}

/// 优化建议（**建议而非强制**——is_forced 恒 false 是域纪律）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepthAdvice {
    /// 建议名（反转 Z / 对数深度）。
    pub name: &'static str,
    /// 理由（人话）。
    pub reason: String,
    /// 代价（人话）。
    pub cost: String,
    /// 恒 false——本模块只建议不强制。
    pub is_forced: bool,
}

/// 反转 Z 建议（F1522 时代主流实践——远处精度高近处精度低翻转后近处优）。
pub fn reversed_z_advice() -> DepthAdvice {
    DepthAdvice {
        name: "反转 Z",
        reason: "浮点深度在近处精度天然高、远处指数劣化；反转后远处分精度大增，配合 [0,1] 约定最常用".to_string(),
        cost: "需后端支持深度比较方向反转与清除值=1.0，切换有一次性成本".to_string(),
        is_forced: false,
    }
}

/// 对数深度建议（均匀分布换非线性代价）。
pub fn log_depth_advice() -> DepthAdvice {
    DepthAdvice {
        name: "对数深度",
        reason: "近远平面跨距极大（如行星尺度）时均匀分桶避免远处全糊".to_string(),
        cost: "着色器需改写深度输出，牺牲部分早期深度测试效率".to_string(),
        is_forced: false,
    }
}

// ---------------------------------------------------------------------------
// 三、裁剪行为声明（判据三：图元裁剪/顶点钳制统一语义）
// ---------------------------------------------------------------------------

/// 裁剪行为（封闭二态——表外拒绝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipBehavior {
    /// 图元裁剪：NDC 外图元被裁剪管线裁成域内片段。
    PrimitiveClipping,
    /// 顶点钳制：越界深度值被钳到深度范围（guard-band 语义）。
    VertexClamping,
}

impl ClipBehavior {
    pub fn say(self) -> &'static str {
        match self {
            ClipBehavior::PrimitiveClipping => "图元裁剪：NDC 外图元被裁剪为域内片段",
            ClipBehavior::VertexClamping => "顶点钳制：越界深度钳入深度范围（guard-band）",
        }
    }
}

/// 裁剪语义声明表（后端接入时登记——声明不是探测）。
pub fn declare_clip_behavior(backend: &str, b: ClipBehavior) -> String {
    alloc::format!("后端 {} 裁剪行为声明：{}", backend, b.say())
}

/// 统一语义（跨后端一致——上层只依赖这一句，字面量冻结）。
pub const UNIFIED_CLIP_SEMANTICS: &str =
    "统一语义：NDC 域外几何不产生可见片段——图元被裁或深度被钳，二者对上层等价";

/// 声明合法性（二态封闭；后端名非空）。
pub fn clip_declare_legal(backend: &str, b: Option<ClipBehavior>) -> bool {
    !backend.is_empty() && b.is_some()
}

// ---------------------------------------------------------------------------
// 三·五、后端接入声明（三节粘合：NDC 约定 + 裁剪行为一体登记）
// ---------------------------------------------------------------------------

/// 后端接入描述（渲染后端接入本约定时的**一体化声明**——名/深度范围/
/// 裁剪行为三件齐，缺一即未完成接入）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NdcBackend {
    /// 后端名（人话，如 "D3D12" / "OpenGL"）。
    pub name: String,
    /// 深度范围约定。
    pub range: DepthRange,
    /// 裁剪行为声明。
    pub behavior: ClipBehavior,
}

impl NdcBackend {
    /// 接入合法性（名非空即合法——二态枚举天然封闭）。
    pub fn legal(&self) -> bool {
        !self.name.is_empty()
    }

    /// 接入声明单行（读屏可播报：名+范围+裁剪三事实）。
    pub fn declare(&self) -> String {
        alloc::format!(
            "后端 {} 接入：深度 {}；{}",
            self.name,
            self.range.say(),
            self.behavior.say()
        )
    }

    /// 语义深度换算到另一后端约定（上层零感知的调用面）。
    pub fn convert_to(&self, other: &NdcBackend, z_per10k: i64) -> i64 {
        convert_ndc_depth(z_per10k, self.range, other.range)
    }
}

// ---------------------------------------------------------------------------
// 四、单元测试（锚点判据承载）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndc_仿射互逆() {
        let z = 3000i64; // 0.3
        let gl = convert_ndc_depth(z, DepthRange::ZeroToOne, DepthRange::NegOneToOne);
        assert_eq!(gl, -4000); // 2*0.3-1 = -0.4
        let back = convert_ndc_depth(gl, DepthRange::NegOneToOne, DepthRange::ZeroToOne);
        assert_eq!(back, 3000);
    }

    #[test]
    fn 深度行_端点吸收() {
        let row = [0, 0, 10000, -5000];
        let r = adapt_depth_row(row, DepthRange::ZeroToOne, DepthRange::NegOneToOne);
        assert_eq!(r, [0, 0, 20000, -25000]);
    }

    #[test]
    fn 精度_近优于远() {
        let p = precision_profile(DEFAULT_NEAR_PER10K, DEFAULT_FAR_PER10K, 24);
        assert!(p[0].1 < p[2].1); // 近处分辨率数值更小=更精细
    }

    #[test]
    fn 建议_不强制() {
        assert!(!reversed_z_advice().is_forced);
        assert!(!log_depth_advice().is_forced);
    }
}
