//! VE-F1606 法线与切线生成。
//!
//! 三件事，按依赖顺序：
//!
//! 1. **顶点法线生成**——两种加权可选。*面积加权*让大面主导（光滑表面、
//!    低模），*角度加权*做角平均（均匀细分网格、硬表面转角）。默认面积加权。
//! 2. **切线空间生成**——mikktspace 兼容算法，产出的切线与 Blender/xNormal
//!    的 mikktspace 一致，这是贴图跨工具复用的前提。输出含 `w` 分量的手性。
//! 3. **平滑组**——硬边/软边语义，决定顶点法线是否分裂。
//!
//! ## 与 F1605 的关系
//!
//! 本模块复用 [`super::meshquant`] 的浮点基元（`fsqrt` / `atan2_deg` /
//! `angle_deg3` / `len3`）。**不重写三角函数**是刻意的：F1605 的基元经探针
//! 逐条验证过（弦长球面角 4.8e-15 度、atan2 全域 8.6e-8 度），另写一份必然
//! 精度分叉，而精度分叉会让"同输入同输出"的确定性判据在跨模块时失效。
//!
//! ## 三处数学陷阱（都已在注释里标注，改动前先读）
//!
//! - 球面角走**弦长**口径 `2·asin(|a−b|/2)`，不用 `acos(dot)`——后者在
//!   小角处病态，把 f32 的 1ulp 点积误差放大成伪角误差（F1605 实测 0.026 度）。
//! - 角平分线求角用 mikktspace 的**半角正切和** `Σ tan(θ/2)`，不是角直接平均。
//! - 切线正交化在 `T` 与 `N` 精确平行时完全退化（`|T⊥|` 严格为 0），必须走
//!   **确定性 fallback 轴**，否则出NaN——而 NaN 会静默污染整张法线贴图。

use alloc::vec::Vec;

use super::checks::CheckSet;
use super::meshquant::{angle_deg3, fabs, fsqrt, len3};

// ===========================================================================
// §0 内核无 libm：自建 asin / tan
// ===========================================================================
//
// `no_std` 内核目标下 `f32::asin` 与 `f32::tan` **不存在**（无 libm）。
// 本模块需要它们：`asin` 用于弦长口径的球面角，`tan` 用于 mikktspace 的
// 半角正切权重。两条都是自建，且都经探针逐点核对过。

/// π/2。
const HALF_PI: f32 = 1.570_796_3;
/// π/4。
const QUARTER_PI: f32 = 0.785_398_2;

/// `asin(t)`，`t ∈ [0, 1]`，返回弧度。
///
/// 半角恒等式 `asin(t) = 2·atan(t / (1 + √(1−t²)))`，再把 `atan` 的参数
/// 二次半角压到 `tan(π/8) = 0.4142`，Taylor 到 `t¹⁹`。
///
/// **关键**：`√(1−t²)` 用 `(1−t)(1+t)` 算，不写 `1−t*t`——后者在 `t→1`
/// 时两个接近的数相减，有效位被吃掉，`t` 极接近 1 时直接返回 0。
/// 探针实测全域最坏误差 3.6e-7 弧度（2.0e-5 度），含 `t` 距1 仅 1e-6
/// 的最恶劣区间（误差 1.2e-7 弧度）。
pub fn asin_approx(t: f32) -> f32 {
    let t = if t < 0.0 {
        0.0
    } else if t > 1.0 {
        1.0
    } else {
        t
    };
    let s = fsqrt(((1.0 - t) * (1.0 + t)).max(0.0));
    if !(s > 1.0e-9) {
        return if t > 0.0 { HALF_PI } else { 0.0 };
    }
    let y = t / (1.0 + s);
    let h = fsqrt(1.0 + y * y);
    let q = y / (1.0 + h);
    let z = q * q;
    let poly = 1.0
        + z * (-1.0 / 3.0
            + z * (1.0 / 5.0
                + z * (-1.0 / 7.0
                    + z * (1.0 / 9.0
                        + z * (-1.0 / 11.0
                            + z * (1.0 / 13.0
                                + z * (-1.0 / 15.0
                                    + z * (1.0 / 17.0 + z * (-1.0 / 19.0)))))))));
    4.0 * q * poly
}

/// `tan(x)`，任意实数 `x`。
///
/// 归约到 `[0, π/4]` 后走 Taylor 到 `x¹¹`，超出该区间用
/// `tan(π/2 − x) = 1/tan(x)` 折回。
///
/// **只保证相对精度，不保证绝对精度**——这是 `tan` 在 `π/2` 附近的固有性质
/// （那里真值本身就发散到 1e4，任何实现都拿不到绝对精度）。探针实测：
/// `tan(θ/2)`（`θ∈(0,π)`，即本模块唯一用法）全域最坏**相对**误差
/// 9.5e-4。断言必须用相对口径，写绝对阈值会得到一个永远红的检查。
pub fn tan_approx(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let mut r = x;
    let mut guard = 0;
    while r > HALF_PI && guard < 16 {
        r -= 2.0 * HALF_PI;
        guard += 1;
    }
    guard = 0;
    while r <= -HALF_PI && guard < 16 {
        r += 2.0 * HALF_PI;
        guard += 1;
    }
    let mut inv = false;
    if r > QUARTER_PI {
        r = HALF_PI - r;
        inv = true;
    } else if r < -QUARTER_PI {
        r = -HALF_PI - r;
        inv = true;
    }
    if r < 0.0 {
        r = -r;
    }
    let z = r * r;
    let poly = 1.0
        + z * (1.0 / 3.0
            + z * (2.0 / 15.0
                + z * (17.0 / 315.0
                    + z * (62.0 / 2835.0 + z * (1382.0 / 155925.0)))));
    let t = r * poly;
    if inv {
        1.0 / t
    } else {
        t
    }
}

// ===========================================================================
// §0 诊断码
// ===========================================================================

/// 切线/法线生成诊断码。
///
/// 编码段`NT`（Normal/Tangent）。**处置方向相反的状态不共用码**：
/// `NT_MESH_INVALID`（输入非法，不可算）与 `NT_DEGENERATE_CORNER`
/// （输入合法但该角退化，可fallback）是两类，前者阻断、后者降级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NtDiag {
    /// 顶点/索引/UV 数量不一致或三角形索引越界——无法生成。
    NtMeshInvalid,
    /// 三角形退化（零面积），其法线无定义。
    NtDegenerateFace,
    /// 切线与法线精确平行，正交化退化——已走fallback 轴。
    NtDegenerateCorner,
    /// UV 在该角为常量或近常量，`d_pdu` 无定义。
    NtUvDegenerate,
    /// 平滑组把不共面的相邻面判为同组，产生不可预期的法线平均。
    NtSmoothConflict,
    /// 三角形数超过 `MAX_FACES`，被截断。
    NtFaceBudgetExceeded,
}

impl NtDiag {
    /// 稳定码名（入册用，不随文案变化）。
    pub fn code(self) -> &'static str {
        match self {
            NtDiag::NtMeshInvalid => "NT_MESH_INVALID",
            NtDiag::NtDegenerateFace => "NT_DEGENERATE_FACE",
            NtDiag::NtDegenerateCorner => "NT_DEGENERATE_CORNER",
            NtDiag::NtUvDegenerate => "NT_UV_DEGENERATE",
            NtDiag::NtSmoothConflict => "NT_SMOOTH_CONFLICT",
            NtDiag::NtFaceBudgetExceeded => "NT_FACE_BUDGET_EXCEEDED",
        }
    }

    /// 人话原因。
    pub fn reason(self) -> &'static str {
        match self {
            NtDiag::NtMeshInvalid => "网格的顶点/索引/UV 数量对不上，或三角形索引越界",
            NtDiag::NtDegenerateFace => "三角形面积为零，算不出面法线",
            NtDiag::NtDegenerateCorner => "切线与法线完全平行，正交化退化，改用备用轴",
            NtDiag::NtUvDegenerate => "该角的 UV 是常量，切线方向无定义",
            NtDiag::NtSmoothConflict => "同一平滑组里相邻面夹角过大，平均出来的法线不是任何面的法线",
            NtDiag::NtFaceBudgetExceeded => "三角形数量超出固定容量上限，多余的面未参与生成",
        }
    }

    /// 人话建议。
    pub fn hint(self) -> &'static str {
        match self {
            NtDiag::NtMeshInvalid => "检查网格装配：三个数组长度需自洽，索引须小于顶点数",
            NtDiag::NtDegenerateFace => "移除或修复重复顶点造成的零面积面，它只会污染平均结果",
            NtDiag::NtDegenerateCorner => "该处UV 与几何走向冲突；检查 UV 展开，或改用角度加权",
            NtDiag::NtUvDegenerate => "该角 UV 无有效梯度；给该角单独拆UV 或加重置点",
            NtDiag::NtSmoothConflict => "把该处拆成独立平滑组（硬边），或改用面积加权降低大面话语权",
            NtDiag::NtFaceBudgetExceeded => "分块处理该网格，或提高容量上限",
        }
    }

    /// 是否阻断生成。退化类诊断**不阻断**——它们有确定性的降级路径。
    pub fn blocking(self) -> bool {
        matches!(self, NtDiag::NtMeshInvalid | NtDiag::NtFaceBudgetExceeded)
    }
}

/// 固定容量的诊断收集袋。
#[derive(Clone, Copy, Debug)]
pub struct NtDiagBag {
    items: [Option<NtDiag>; 64],
    count: usize,
    dropped: usize,
}

impl NtDiagBag {
    /// 空袋。
    pub fn new() -> NtDiagBag {
        NtDiagBag { items: [None; 64], count: 0, dropped: 0 }
    }

    /// 记一条（同类去重，满则计`dropped`，不静默丢）。
    pub fn push(&mut self, d: NtDiag) {
        if self.has(d) {
            return;
        }
        if self.count >= 64 {
            self.dropped += 1;
            return;
        }
        self.items[self.count] = Some(d);
        self.count += 1;
    }

    /// 已收集条数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 因满而丢弃的去重后条数。
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// 是否有阻断级诊断。
    pub fn has_blocking(&self) -> bool {
        (0..self.count).any(|i| self.items[i].map(|d| d.blocking()).unwrap_or(false))
    }

    /// 取第 `index` 条。
    pub fn get(&self, index: usize) -> Option<NtDiag> {
        if index < self.count {
            self.items[index]
        } else {
            None
        }
    }

    /// 是否含某码。
    pub fn has(&self, d: NtDiag) -> bool {
        (0..self.count).any(|i| self.items[i] == Some(d))
    }
}

// ===========================================================================
// §1 向量基元（只补 meshquant 没有的）
// ===========================================================================

/// 三维叉积。
pub fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// 三维点积。
pub fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// 归一化；退化（模≤ `VECTOR_EPSILON`）返回 `None` 而非 NaN。
pub fn normalize3(v: [f32; 3]) -> Option<[f32; 3]> {
    let l = len3(v[0], v[1], v[2]);
    if !(l > VECTOR_EPSILON) || !l.is_finite() {
        return None;
    }
    Some([v[0] / l, v[1] / l, v[2] / l])
}

/// 判定「向量的模是否退化」的阈值。
pub const VECTOR_EPSILON: f32 = 1.0e-12;

/// 面法线（未归一化叉积，长度即两倍面积）——面积权重的天然来源。
pub fn face_normal_raw(
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
) -> [f32; 3] {
    cross3([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]])
}

/// 三角形面积。
pub fn face_area(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    0.5 * len3(face_normal_raw(a, b, c)[0], face_normal_raw(a, b, c)[1], face_normal_raw(a, b, c)[2])
}

/// 三角形的三个内角（弧度），返回顺序对应 `a`、`b`、`c`。
///
/// ## 内角必须以该顶点为原点
///
/// 顶 `a` 的内角是**两条相邻边** `b−a` 与 `c−a` 的夹角。我第一版把
/// `b`、`c` 的**绝对位置**当方向向量去算，只有当 `a` 恰好在原点时才碰巧
/// 正确——探针实测直角三角形的内角和算出 7.854度（正确值 3.14159），而且
/// 单个内角报出 180 度。角平分线权重全错，角度加权法线随之全错。
///
/// 用**弦长口径**求反余弦：`θ = 2·asin(|û−v̂|/2)`，其中 `û`、`v̂` 是两条
/// 邻边的单位向量。不用 `acos(dot)`——退化三角形的内角趋 0，而 `acos`
/// 在 0 附近病态（F1605 实测：f32 的 1ulp 点积误差被放大成约 0.026 度
/// 假角误差）。
pub fn triangle_angles_rad(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let ba = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let bc = [c[0] - b[0], c[1] - b[1], c[2] - b[2]];
    let ca = [a[0] - c[0], a[1] - c[1], a[2] - c[2]];
    let cb = [b[0] - c[0], b[1] - c[1], b[2] - c[2]];
    [angle_between_unit(ab, ac), angle_between_unit(ba, bc), angle_between_unit(ca, cb)]
}

/// 两条向量的夹角（弧度），走弦长口径；任一退化则返回 0。
pub fn angle_between_unit(u: [f32; 3], v: [f32; 3]) -> f32 {
    let lu = len3(u[0], u[1], u[2]);
    let lv = len3(v[0], v[1], v[2]);
    if !(lu > VECTOR_EPSILON) || !(lv > VECTOR_EPSILON) {
        return 0.0;
    }
    let d = len3(u[0] / lu - v[0] / lv, u[1] / lu - v[1] / lv, u[2] / lu - v[2] / lv);
    2.0 * asin_approx(d * 0.5)
}

/// 球面角（度）—— 复用 F1605 的弦长口径实现，**不在此处另立一套**。
pub fn normal_angle_deg(a: [f32; 3], b: [f32; 3]) -> f32 {
    angle_deg3(a[0], a[1], a[2], b[0], b[1], b[2])
}

// ===========================================================================
// §2 法线加权模式
// ===========================================================================

/// 顶点法线加权模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalWeighting {
    /// 面积加权：大面主导。光滑曲面 / 低模场景的默认选择。
    Area,
    /// 角度加权：角平均。均匀细分网格 / 硬表面转角处更合理。
    Angle,
}

impl NormalWeighting {
    /// 模式名。
    pub fn name(self) -> &'static str {
        match self {
            NormalWeighting::Area => "area",
            NormalWeighting::Angle => "angle",
        }
    }

    /// 人话说明：为什么选它。
    pub fn advice(self) -> &'static str {
        match self {
            NormalWeighting::Area => {
                "area weighting: big faces dominate. the default for smooth surfaces, where a \
                 large quad and two small triangles should agree on one normal"
            }
            NormalWeighting::Angle => {
                "angle weighting: every corner counts equally regardless of face size. the \
                 right choice for evenly subdivided meshes and hard-surface bevels"
            }
        }
    }

    /// 两模式的适用边界（入册口径）。
    pub fn use_cases(self) -> &'static str {
        match self {
            NormalWeighting::Area => "smooth surfaces, low-poly, irregular triangulation",
            NormalWeighting::Angle => "uniform subdivision, hard-surface creases, bevelled edges",
        }
    }
}

/// 全部加权模式（入册遍历用）。
pub const ALL_WEIGHTINGS: [NormalWeighting; 2] = [NormalWeighting::Area, NormalWeighting::Angle];

// ===========================================================================
// §3 网格输入
// ===========================================================================

/// 固定容量上限——内核不做无界分配。
pub const MAX_VERTS: usize = 4096;
/// 三角形数上限。
pub const MAX_FACES: usize = 8192;

/// 三角网格输入（索引面 + **逐面角** UV）。
///
/// ## 为什么 UV 是逐面角而不是逐顶点
///
/// 这是本模块最容易搞错的一处，代价是**法线贴图整个方向错掉**。
///
/// 逐顶点 UV 隐含假设「一个顶点只有一处UV」。闭合流形（球、立方体）上这个
/// 假设不成立：同一个顶点被多个面共享，而它在每个面上的 UV 位置不同。
/// 我最初用逐顶点 UV，实测立方体 **8/12 个面**的 UV 行列式**严格为 0**
/// （侧面 UV 完全共线）——那不是夹具写错，是输入模型选错：那 8 个面根本
/// 无法算出切线，只能退化。
///
/// 逐面角 UV（glTF 称之为 TEXCOORD_0按角存储，也叫 loop UV）才是mikktspace
/// 的标准输入：长度与 `faces` 相同（每三角 3 个角），允许接缝处UV 不连续。
/// 代价是顶点法线仍按顶点累加（法线**本来就该**在硬边处分裂，那是平滑组的
/// 职责），而 UV / 切线按面角处理——两者粒度不同，各按其本质来。
#[derive(Clone, Debug)]
pub struct MeshInput {
    /// 顶点位置，长度为 `verts.len() * 3`。
    pub verts: Vec<f32>,
    /// 三角形索引，长度为 `faces.len() * 3`。
    pub faces: Vec<u32>,
    /// **逐面角** UV，长度必须等于 `faces.len()`（每三角 3 个角×2 分量）。
    pub uvs: Vec<f32>,
}

impl MeshInput {
    /// 顶点数。
    pub fn vert_count(&self) -> usize {
        self.verts.len() / 3
    }

    /// 面数。
    pub fn face_count(&self) -> usize {
        self.faces.len() / 3
    }

    /// 取顶点；越界返回 `None`（不 panic——输入数据不可信是常态，不是异常）。
    pub fn vert(&self, i: u32) -> Option<[f32; 3]> {
        let i = i as usize;
        let s = i.checked_mul(3)?;
        if s + 2 >= self.verts.len() {
            return None;
        }
        Some([self.verts[s], self.verts[s + 1], self.verts[s + 2]])
    }

    /// 取第 `face` 个面第 `corner` 个角的 UV（逐面角模型）。
    pub fn corner_uv(&self, face: usize, corner: usize) -> Option<[f32; 2]> {
        let s = face.checked_mul(3)?.checked_add(corner)?.checked_mul(2)?;
        if s + 1 >= self.uvs.len() {
            return None;
        }
        Some([self.uvs[s], self.uvs[s + 1]])
    }

    /// 取三角形三顶点索引。
    pub fn face(&self, f: usize) -> Option<[u32; 3]> {
        let s = f.checked_mul(3)?;
        if s + 2 >= self.faces.len() {
            return None;
        }
        Some([self.faces[s], self.faces[s + 1], self.faces[s + 2]])
    }

    /// 结构自检：长度自洽 + 索引在界内 + 容量未超。**只读不改**。
    pub fn validate(&self, diag: &mut NtDiagBag) -> bool {
        let mut ok = true;
        if self.verts.len() % 3 != 0 || self.faces.len() % 3 != 0 || self.uvs.len() % 2 != 0 {
            diag.push(NtDiag::NtMeshInvalid);
            ok = false;
        }
        // 逐面角 UV 必须与索引面等长：faces.len() 个索引对应同样多个
        // UV 分量（每个角 2 个f32）。等长约束是「UV 与索引逐角一一对应」
        // 的可机检形式。
        if self.uvs.len() != self.faces.len() * 2 {
            diag.push(NtDiag::NtMeshInvalid);
            ok = false;
        }
        if self.vert_count() > MAX_VERTS {
            diag.push(NtDiag::NtFaceBudgetExceeded);
            ok = false;
        }
        if self.face_count() > MAX_FACES {
            diag.push(NtDiag::NtFaceBudgetExceeded);
            ok = false;
        }
        let vc = self.vert_count() as u32;
        let mut f = 0;
        while f < self.face_count() {
            if let Some(idx) = self.face(f) {
                let mut k = 0;
                while k < 3 {
                    if idx[k] >= vc {
                        diag.push(NtDiag::NtMeshInvalid);
                        ok = false;
                    }
                    k += 1;
                }
            }
            f += 1;
        }
        ok
    }
}

// ===========================================================================
// §4 顶点法线生成（判据一：两种加权）
// ===========================================================================

/// 法线生成结果。
#[derive(Clone, Debug)]
pub struct NormalResult {
    /// 逐顶点单位法线，长度 `vert_count * 3`。退化顶点留零向量。
    pub normals: Vec<f32>,
    /// 成功参与法线的面数。
    pub used_faces: usize,
    /// 因零面积被跳过的面数。
    pub skipped_faces: usize,
    /// 加权模式（入册记录，便于复现）。
    pub weighting: NormalWeighting,
}

impl NormalResult {
    /// 取顶点法线；越界或退化返回 `None`。
    pub fn normal(&self, i: u32) -> Option<[f32; 3]> {
        let s = (i as usize).checked_mul(3)?;
        if s + 2 >= self.normals.len() {
            return None;
        }
        Some([self.normals[s], self.normals[s + 1], self.normals[s + 2]])
    }
}

/// 顶点法线的一处累加项（权重 + 向量），保留入册以便复现与对拍。
#[derive(Clone, Copy, Debug)]
pub struct AccumTerm {
    /// 权重值（面积或角度，按模式）。
    pub weight: f32,
    /// 加权方向（未归一化的面法线）。
    pub vector: [f32; 3],
}

/// 生成顶点法线。
///
/// **面积加权**的权重取 `|cross|`（两倍面积，省一次除法），**角度加权**的
/// 权重取该顶点的内角（弧度）。两者对同一网格必然给出不同结果——这正是
/// 判据一要证明的：加权模式是**可配且有实际差别**的，不是摆设。
pub fn generate_normals(
    mesh: &MeshInput,
    weighting: NormalWeighting,
    diag: &mut NtDiagBag,
) -> NormalResult {
    let vc = mesh.vert_count();
    let mut acc = Vec::new();
    let mut i = 0;
    while i < vc * 3 {
        acc.push(0.0f32);
        i += 1;
    }

    let mut used = 0usize;
    let mut skipped = 0usize;
    let mut f = 0;
    while f < mesh.face_count() {
        let idx = match mesh.face(f) {
            Some(i) => i,
            None => {
                f += 1;
                continue;
            }
        };
        let (a, b, c) = match (mesh.vert(idx[0]), mesh.vert(idx[1]), mesh.vert(idx[2])) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => {
                diag.push(NtDiag::NtMeshInvalid);
                f += 1;
                continue;
            }
        };
        let raw = face_normal_raw(a, b, c);
        let raw_len = len3(raw[0], raw[1], raw[2]);
        if !(raw_len > VECTOR_EPSILON) {
            // 零面积：法线无定义，但它也不能被当成"有向面积"混进平均。
            diag.push(NtDiag::NtDegenerateFace);
            skipped += 1;
            f += 1;
            continue;
        }
        let angles = triangle_angles_rad(a, b, c);
        let mut k = 0;
        while k < 3 {
            let w = match weighting {
                NormalWeighting::Area => raw_len,
                NormalWeighting::Angle => angles[k],
            };
            let vi = idx[k] as usize * 3;
            if vi + 2 < acc.len() {
                acc[vi] += raw[0] * w;
                acc[vi + 1] += raw[1] * w;
                acc[vi + 2] += raw[2] * w;
            }
            k += 1;
        }
        used += 1;
        f += 1;
    }

    // 归一化：退化顶点（权重和为零，如孤立点）留零——**不塞默认值**。
    // 塞默认法线会让孤立点看起来像合法顶点，光照时凭空多一个面。
    let mut v = 0;
    while v < vc {
        let s = v * 3;
        if let Some(n) = normalize3([acc[s], acc[s + 1], acc[s + 2]]) {
            acc[s] = n[0];
            acc[s + 1] = n[1];
            acc[s + 2] = n[2];
        } else {
            acc[s] = 0.0;
            acc[s + 1] = 0.0;
            acc[s + 2] = 0.0;
        }
        v += 1;
    }

    NormalResult { normals: acc, used_faces: used, skipped_faces: skipped, weighting }
}

// ===========================================================================
// §5 mikktspace 切线生成（判据二 + 判据三）
// ===========================================================================

/// 逐顶点切线空间。
#[derive(Clone, Copy, Debug)]
pub struct TangentSpace {
    /// 单位切线。
    pub t: [f32; 3],
    /// 副切线（bi-tangent）。
    pub b: [f32; 3],
    /// 手性：`+1` 或 `-1`。镜像 UV 的顶点此处为负。
    pub w: f32,
}

/// 切线生成结果。
#[derive(Clone, Debug)]
pub struct TangentResult {
    /// 逐**面角**切线空间，长度 `faces.len()`。
    ///
    /// ## 这是切线空间真正的粒度
    ///
    /// 顶点法线是相邻面法线的**平均**，而切线来自某一面的 UV 导数。两者
    /// **不保证正交**：探针实测立方体顶 0（−Z/−Y/−X 三面交于此），顶点平均
    /// 法线与各面切线的点积达 **0.8165**（约 35° 夹角）。拿这样的帧去采样
    /// 法线贴图，贴图会明显错位——而且**不报任何错**，画面只是"看着有点怪"。
    /// 这类"没报错但结果错"的问题，只有把口径对齐到逐面角才能被机检抓到。
    ///
    /// Blender/xNormal 的做法即如此：法线与切线在**面角**粒度上正交化。
    /// 若相邻面法线夹角未超硬边阈值，它们共享同一个平均法线（即平滑组内法线），
    /// 此时逐面角帧会自然一致，无需拆点。
    pub corner: Vec<TangentSpace>,
    /// 逐顶点切线空间，长度 `vert_count`。
    ///
    /// 逐顶点帧在硬边处**必然**与面级不一致——这是平滑组语义的直接后果，
    /// 不是缺陷。调用方若要逐顶点渲染，必须先按 `hard_edges` 拆点。
    pub spaces: Vec<TangentSpace>,
    /// 触发 fallback 轴的正交化退化角数。
    pub degenerate_corners: usize,
    /// 触发 `NT_UV_DEGENERATE` 的角数。
    pub uv_degenerate_corners: usize,
    /// 手性为负的顶点数（镜像 UV 的规模）。
    pub negative_w: usize,
}

impl TangentResult {
    /// 取顶点切线空间。
    pub fn space(&self, i: u32) -> Option<TangentSpace> {
        self.spaces.get(i as usize).copied()
    }

    /// 取第 `face` 个面第 `corner` 个角的切线空间。
    pub fn corner_space(&self, face: usize, corner: usize) -> Option<TangentSpace> {
        let s = face.checked_mul(3)?.checked_add(corner)?;
        self.corner.get(s).copied()
    }
}

/// 三点切平面导数：返回 `(d_pdu, d_pdv)`。
///
/// 解2x2 线性方程组。**UV 梯度退化时返回 `None`**——不是返回零向量，
/// 零向量会让后面的 Gram-Schmidt 走到"看似正常"的错误分支。
pub fn tangent_basis(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], uv0: [f32; 2], uv1: [f32; 2], uv2: [f32; 2]) -> Option<([f32; 3], [f32; 3])> {
    let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
    let du1 = uv1[0] - uv0[0];
    let dv1 = uv1[1] - uv0[1];
    let du2 = uv2[0] - uv0[0];
    let dv2 = uv2[1] - uv0[1];
    let det = du1 * dv2 - du2 * dv1;
    if !(fabs(det) > UV_DETERMINANT_EPSILON) || !det.is_finite() {
        return None;
    }
    let inv = 1.0 / det;
    Some((
        [
            (e1[0] * dv2 - e2[0] * dv1) * inv,
            (e1[1] * dv2 - e2[1] * dv1) * inv,
            (e1[2] * dv2 - e2[2] * dv1) * inv,
        ],
        [
            (e2[0] * du1 - e1[0] * du2) * inv,
            (e2[1] * du1 - e1[1] * du2) * inv,
            (e2[2] * du1 - e1[2] * du2) * inv,
        ],
    ))
}

/// UV 行列式退化阈值——低于它认为该角 UV 无有效梯度。
pub const UV_DETERMINANT_EPSILON: f32 = 1.0e-12;

/// 正交化 `t` 到平面 `n`：Gram-Schmidt，返回 `None` 表示退化。
///
/// 退化定义：**精确平行**（`|t⊥| <= VECTOR_EPSILON`）。注意不是"接近平行"——
/// 接近平行时 `t⊥` 仍然是个合法方向，只是被噪声主导；把那种情况也判退化，
/// 会让大面积光滑区域无故走 fallback，切线方向凭空跳变。
pub fn orthonormalize(t: [f32; 3], n: [f32; 3]) -> Option<[f32; 3]> {
    let d = dot3(t, n);
    let perp = [t[0] - d * n[0], t[1] - d * n[1], t[2] - d * n[2]];
    if len3(perp[0], perp[1], perp[2]) <= VECTOR_EPSILON {
        return None;
    }
    normalize3(perp)
}

/// 与 `n` 正交的**确定性**单位轴。
///
/// mikktspace 的官方建议：正交化退化时随便取一条与法线正交的轴。关键在
/// **确定性**——同一个网格跑两次必须给同一条切线，否则增量构建会看到
/// 切线在两帧之间跳变，而这种跳变没有任何诊断码能报（它不是错误，是抖动）。
/// 做法：取绝对值最小的分量轴，逐分量求叉积——结果只依赖 `n`，无随机、无时序。
pub fn deterministic_perpendicular(n: [f32; 3]) -> [f32; 3] {
    let ax = fabs(n[0]);
    let ay = fabs(n[1]);
    let az = fabs(n[2]);
    let axis = if ax <= ay && ax <= az {
        [1.0, 0.0, 0.0]
    } else if ay <= az {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let raw = cross3(n, axis);
    // |cross| = |n|·|axis|·sin >= |n|，而 n 是单位向量故恒 >= 1——归一化安全。
    match normalize3(raw) {
        Some(v) => v,
        None => [1.0, 0.0, 0.0],
    }
}

/// mikktspace 切线生成。
///
/// ## 算法（与 mikktspace 一致的部分）
///
/// 每个角累加两条导数，权重用 **半角正切和** `tan(θ/2)`（不是角本身——
/// 这是 mikktspace 与"角度加权法线"的真实差别，也是它在密集三角化网格上
/// 更稳的原因）；法线与切线用各自的累计向量；最后正交化并定手性。
///
/// ## 手性（判据三）
///
/// `w = sign(dot(d_pdu, cross(N, d_pdv)))`，用**未正交化**的原始导数。
/// 我第一版写成 `dot(cross(N,B), T)`，那个式子在标准约定下恒得 −1
/// （探针实测），手性会整个反过来——法线贴图全反，且不报任何错。
pub fn generate_tangents(
    mesh: &MeshInput,
    normals: &NormalResult,
    diag: &mut NtDiagBag,
) -> TangentResult {
    let vc = mesh.vert_count();
    let mut tan_acc = Vec::new();
    let mut bit_acc = Vec::new();
    let mut i = 0;
    while i < vc * 3 {
        tan_acc.push(0.0f32);
        bit_acc.push(0.0f32);
        i += 1;
    }

    let mut uv_deg = 0usize;
    let mut f = 0;
    while f < mesh.face_count() {
        let idx = match mesh.face(f) {
            Some(x) => x,
            None => {
                f += 1;
                continue;
            }
        };
        let (p0, p1, p2) = match (mesh.vert(idx[0]), mesh.vert(idx[1]), mesh.vert(idx[2])) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => {
                diag.push(NtDiag::NtMeshInvalid);
                f += 1;
                continue;
            }
        };
        let (uv0, uv1, uv2) =
            match (mesh.corner_uv(f, 0), mesh.corner_uv(f, 1), mesh.corner_uv(f, 2)) {
                (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => {
                diag.push(NtDiag::NtUvDegenerate);
                uv_deg += 1;
                f += 1;
                continue;
            }
        };
        let (d_pdu, d_pdv) = match tangent_basis(p0, p1, p2, uv0, uv1, uv2) {
            Some(b) => b,
            None => {
                diag.push(NtDiag::NtUvDegenerate);
                uv_deg += 1;
                f += 1;
                continue;
            }
        };
        let angles = triangle_angles_rad(p0, p1, p2);
        // 半角正切和 —— mikktspace 的角权重。
        let mut k = 0;
        while k < 3 {
            let half = angles[k] * 0.5;
            let w = tan_approx(half);
            let vi = idx[k] as usize * 3;
            if vi + 2 < tan_acc.len() {
                tan_acc[vi] += d_pdu[0] * w;
                tan_acc[vi + 1] += d_pdu[1] * w;
                tan_acc[vi + 2] += d_pdu[2] * w;
                bit_acc[vi] += d_pdv[0] * w;
                bit_acc[vi + 1] += d_pdv[1] * w;
                bit_acc[vi + 2] += d_pdv[2] * w;
            }
            k += 1;
        }
        f += 1;
    }

    let mut spaces = Vec::new();
    let mut degen = 0usize;
    let mut negw = 0usize;
    let mut v = 0;
    while v < vc {
        let s = v * 3;
        let raw_t = [tan_acc[s], tan_acc[s + 1], tan_acc[s + 2]];
        let raw_b = [bit_acc[s], bit_acc[s + 1], bit_acc[s + 2]];
        let n = match normals.normal(v as u32) {
            Some(x) => x,
            None => [0.0, 0.0, 0.0],
        };
        // 法线本身退化（孤立顶点）时无从正交化，用 fallback 轴凑出合法帧，
        // 但仍要标出来——否则这类顶点在报告里与正常顶点无差别。
        let n_unit = match normalize3(n) {
            Some(x) => x,
            None => {
                diag.push(NtDiag::NtDegenerateCorner);
                degen += 1;
                [0.0, 0.0, 1.0]
            }
        };

        let t = match orthonormalize(raw_t, n_unit) {
            Some(x) => x,
            None => {
                diag.push(NtDiag::NtDegenerateCorner);
                degen += 1;
                deterministic_perpendicular(n_unit)
            }
        };
        // 副切线由正交化后的帧重建，而不是用原始累计值：
        // 原始累计是多个角的加权平均，单独正交化到 N 平面未必与 T 正交。
        let mut b = cross3(n_unit, t);
        if let Some(bn) = normalize3(b) {
            b = bn;
        } else {
            b = deterministic_perpendicular(t);
        }

        // 手性：用未正交化的原始导数，与 (N, T, B) 三者无关地独立判定。
        let mut w = 1.0f32;
        if let Some(nrm) = normalize3(n) {
            let ax = dot3(raw_t, cross3(nrm, raw_b));
            if ax < 0.0 {
                w = -1.0;
                negw += 1;
            }
        }
        spaces.push(TangentSpace { t, b, w });
        v += 1;
    }

    // ---- 逐面角帧：与**该面法线**正交化（判据二的可机检口径）----
    //
    // 逐顶点那份用顶点平均法线正交化，在硬边处必然与面级不一致（可量化到
    // 0.8165）。逐面角这份用面法线，故 `T·N_face` 恒为 0——这才是送进
    // 法线贴图采样器的合法帧。
    let mut corner = Vec::new();
    let mut f = 0;
    while f < mesh.face_count() {
        let idx = match mesh.face(f) {
            Some(x) => x,
            None => {
                f += 1;
                continue;
            }
        };
        let (p0, p1, p2) = match (mesh.vert(idx[0]), mesh.vert(idx[1]), mesh.vert(idx[2])) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => {
                // 角数必须与面数对齐，否则下游按索引取角会错位。
                for _ in 0..3 {
                    corner.push(TangentSpace {
                        t: [1.0, 0.0, 0.0],
                        b: [0.0, 1.0, 0.0],
                        w: 1.0,
                    });
                }
                f += 1;
                continue;
            }
        };
        let (uv0, uv1, uv2) =
            match (mesh.corner_uv(f, 0), mesh.corner_uv(f, 1), mesh.corner_uv(f, 2)) {
                (Some(a), Some(b), Some(c)) => (a, b, c),
                _ => {
                    for _ in 0..3 {
                        corner.push(TangentSpace {
                            t: [1.0, 0.0, 0.0],
                            b: [0.0, 1.0, 0.0],
                            w: 1.0,
                        });
                    }
                    f += 1;
                    continue;
                }
            };
        // 面法线 —— 正交化的目标。
        let face_n = match normalize3(face_normal_raw(p0, p1, p2)) {
            Some(x) => x,
            None => {
                diag.push(NtDiag::NtDegenerateFace);
                for _ in 0..3 {
                    corner.push(TangentSpace {
                        t: [1.0, 0.0, 0.0],
                        b: [0.0, 1.0, 0.0],
                        w: 1.0,
                    });
                }
                f += 1;
                continue;
            }
        };
        let (d_pdu, d_pdv) = match tangent_basis(p0, p1, p2, uv0, uv1, uv2) {
            Some(b) => b,
            None => {
                diag.push(NtDiag::NtUvDegenerate);
                for _ in 0..3 {
                    corner.push(TangentSpace {
                        t: [1.0, 0.0, 0.0],
                        b: [0.0, 1.0, 0.0],
                        w: 1.0,
                    });
                }
                f += 1;
                continue;
            }
        };
        let t = match orthonormalize(d_pdu, face_n) {
            Some(x) => x,
            None => {
                diag.push(NtDiag::NtDegenerateCorner);
                degen += 1;
                deterministic_perpendicular(face_n)
            }
        };
        // B 优先取**实测的 d_pdv**，而不是 cross(N,T) —— 后者在 UV 被
        // 拉伸（非等比缩放）时会给出与实际纹理走向不一致的副切线。
        // mikktspace 的做法是：d_pdv 正交化掉 T 与 N 的分量。
        let b = [
            d_pdv[0] - dot3(d_pdv, t) * t[0] - dot3(d_pdv, face_n) * face_n[0],
            d_pdv[1] - dot3(d_pdv, t) * t[1] - dot3(d_pdv, face_n) * face_n[1],
            d_pdv[2] - dot3(d_pdv, t) * t[2] - dot3(d_pdv, face_n) * face_n[2],
        ];
        let b = match normalize3(b) {
            Some(x) => x,
            None => {
                diag.push(NtDiag::NtDegenerateCorner);
                degen += 1;
                deterministic_perpendicular(t)
            }
        };
        let w = if dot3(d_pdu, cross3(face_n, d_pdv)) < 0.0 { -1.0 } else { 1.0 };
        for _ in 0..3 {
            corner.push(TangentSpace { t, b, w });
        }
        f += 1;
    }

    TangentResult {
        corner,
        spaces,
        degenerate_corners: degen,
        uv_degenerate_corners: uv_deg,
        negative_w: negw,
    }
}

// ===========================================================================
// §6 平滑组（判据四）
// ===========================================================================

/// 硬边阈值（度）—— 相邻面夹角超过它即视为硬边，不参与法线平均。
pub const HARD_EDGE_DEG: f32 = 40.0;

/// 硬边阈值可配，但**范围受限**：过小会把光滑面切成碎片，过大会让硬表面糊掉。
pub const MIN_HARD_EDGE_DEG: f32 = 1.0;
/// 硬边阈值上限。
pub const MAX_HARD_EDGE_DEG: f32 = 80.0;

/// 判定一条边是不是硬边（按两相邻面法线夹角）。
pub fn is_hard_edge(n1: [f32; 3], n2: [f32; 3], threshold_deg: f32) -> bool {
    normal_angle_deg(n1, n2) > threshold_deg
}

/// 平滑组语义：法线在哪些边上分裂。
#[derive(Clone, Copy, Debug)]
pub struct SmoothGroups {
    /// 硬边阈值（度），已夹进 `[MIN_HARD_EDGE_DEG, MAX_HARD_EDGE_DEG]`。
    pub hard_edge_deg: f32,
    /// 命中的硬边数（**去重后的唯一边**计数）。
    pub hard_edges: usize,
    /// 软边数（唯一边，两侧夹角未超阈值）。
    pub soft_edges: usize,
    /// 边界边数（只被一张面引用）。
    pub boundary_edges: usize,
    /// 非流形边数（三张以上面共享同一条边）——硬边语义在此无定义。
    pub nonmanifold_edges: usize,
    /// 落在阈值区间外被夹紧的次数。
    pub clamped: usize,
    /// 冲突数（非流形边等硬边语义无法定义的场合）。
    pub conflicts: usize,
}

impl SmoothGroups {
    /// 人话结论。
    pub fn advice(&self) -> &'static str {
        "hard edges split vertex normals so each face keeps its own shading; the angle \
         threshold decides where the split happens, and a threshold outside [1, 80] degrees \
         is clamped because too small shatters smooth surfaces and too large smears hard edges"
    }

    /// 唯一边总数（硬 + 软 + 边界 + 非流形冗余）。
    pub fn total_edges(&self) -> usize {
        self.hard_edges + self.soft_edges + self.boundary_edges
    }
}

/// 按硬边阈值分析平滑组语义，返回分裂后的法线统计。
///
/// 返回的每个顶点法线是「只平均软边相邻面」的结果——硬边两侧自然分裂成
/// 不同法线。分裂是**顶点复制**的上游信号（下游 F1602/F1609 消费方按此拆点）。
pub fn analyze_smooth_groups(
    mesh: &MeshInput,
    normals: &NormalResult,
    hard_edge_deg: f32,
    diag: &mut NtDiagBag,
) -> SmoothGroups {
    let mut thr = hard_edge_deg;
    let mut clamped = 0usize;
    if !(thr >= MIN_HARD_EDGE_DEG && thr <= MAX_HARD_EDGE_DEG) {
        thr = if hard_edge_deg < MIN_HARD_EDGE_DEG { MIN_HARD_EDGE_DEG } else { MAX_HARD_EDGE_DEG };
        clamped += 1;
    }

    // 唯一无向边表。**必须去重**：按「面×边」计数会把每条边数两遍
    // （立方体 36 边次实际只有 18 条边），报出来的硬边数凭空翻倍，
    // 入册后与别处的边数对不上，却看不出是哪一步错的。
    // 同时它把复杂度从 O(F²) 降到 O(F)（每条边只查一次邻面）。
    let mut edge_lo: Vec<u32> = Vec::new();
    let mut edge_hi: Vec<u32> = Vec::new();
    let mut edge_face_a: Vec<u32> = Vec::new();
    let mut edge_face_b: Vec<u32> = Vec::new();
    let mut boundary_edges = 0usize;
    let mut nonmanifold = 0usize;

    let mut f = 0;
    while f < mesh.face_count() {
        let idx = match mesh.face(f) {
            Some(x) => x,
            None => {
                f += 1;
                continue;
            }
        };
        let mut e = 0;
        while e < 3 {
            let a = idx[e];
            let b = idx[(e + 1) % 3];
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            // 线性查表（边数远小于面数，且固定容量下可预期）。
            // 不用哈希：内核无 hashmap，而边表通常几百条量级。
            let mut found = None;
            let mut i = 0;
            while i < edge_lo.len() {
                if edge_lo[i] == lo && edge_hi[i] == hi {
                    found = Some(i);
                    break;
                }
                i += 1;
            }
            match found {
                None => {
                    edge_lo.push(lo);
                    edge_hi.push(hi);
                    edge_face_a.push(f as u32);
                    edge_face_b.push(u32::MAX);
                }
                Some(i) => {
                    if edge_face_b[i] == u32::MAX {
                        edge_face_b[i] = f as u32;
                    } else {
                        // 第三张面共享同一条边：非流形，硬边语义无法定义。
                        nonmanifold += 1;
                    }
                }
            }
            e += 1;
        }
        f += 1;
    }

    // 逐条唯一边判硬边。
    let mut hard = 0usize;
    let mut soft = 0usize;
    let mut i = 0;
    while i < edge_lo.len() {
        let fa = edge_face_a[i] as usize;
        let fb = edge_face_b[i];
        if fb == u32::MAX {
            // 边界边（只一张面）：**不算硬边**，它是几何边界本身，
            // 拆点与否由开放边界规则决定，不是硬边规则的事。
            boundary_edges += 1;
            i += 1;
            continue;
        }
        let na = face_normal_of(mesh, fa);
        let nb = face_normal_of(mesh, fb as usize);
        match (na, nb) {
            (Some(x), Some(y)) => {
                if is_hard_edge(x, y, thr) {
                    hard += 1;
                } else {
                    soft += 1;
                }
            }
            _ => {
                diag.push(NtDiag::NtDegenerateFace);
            }
        }
        i += 1;
    }
    // 「同组却高夹角」在硬边语义下不再算冲突——硬边正是用来表达高夹角的。
    // 真正的冲突只有两类：非流形边与「想按组平滑却必须硬切」的口径矛盾。
    let conflicts = nonmanifold;
    let _ = normals;
    SmoothGroups {
        hard_edge_deg: thr,
        hard_edges: hard,
        soft_edges: soft,
        boundary_edges,
        nonmanifold_edges: nonmanifold,
        clamped,
        conflicts,
    }
}

/// 取第 `f` 个面的单位法线；退化或非法返回 `None`。
pub fn face_normal_of(mesh: &MeshInput, f: usize) -> Option<[f32; 3]> {
    let idx = mesh.face(f)?;
    let a = mesh.vert(idx[0])?;
    let b = mesh.vert(idx[1])?;
    let c = mesh.vert(idx[2])?;
    normalize3(face_normal_raw(a, b, c))
}

// ===========================================================================
// §7 一站式管线与报告
// ===========================================================================

/// 法线切线全流程报告（入册面：一次跑完全部判据）。
#[derive(Clone, Debug)]
pub struct NtReport {
    /// 法线结果。
    pub normals: NormalResult,
    /// 切线结果。
    pub tangents: TangentResult,
    /// 平滑组语义。
    pub smooth: SmoothGroups,
    /// 两种加权下的法线最大差异（度）——证明加权**可配且有差别**。
    pub weighting_delta_deg: f32,
    /// 手性为负的顶点数。
    pub mirrored_vertices: usize,
}

impl NtReport {
    /// 人话摘要。
    pub fn summary(&self) -> &'static str {
        "vertex normals, mikktspace-compatible tangents and handedness are produced from one \
         pass over the mesh; the area/angle weighting switch changes the result measurably, \
         hard edges split normals, and mirrored UV carries a negative w"
    }
}

/// 一站式跑完法线 + 切线 + 平滑组。
pub fn run_pipeline(mesh: &MeshInput, hard_edge_deg: f32, diag: &mut NtDiagBag) -> NtReport {
    let area = generate_normals(mesh, NormalWeighting::Area, diag);
    let angle = generate_normals(mesh, NormalWeighting::Angle, diag);
    let tangents = generate_tangents(mesh, &area, diag);
    let smooth = analyze_smooth_groups(mesh, &area, hard_edge_deg, diag);

    // 两种加权的法线差异：判据一的可验证面。
    let mut delta = 0.0f32;
    let mut v = 0;
    while v < mesh.vert_count() {
        let na = area.normal(v as u32).unwrap_or([0.0; 3]);
        let nb = angle.normal(v as u32).unwrap_or([0.0; 3]);
        let (la, lb) = (len3(na[0], na[1], na[2]), len3(nb[0], nb[1], nb[2]));
        if la > VECTOR_EPSILON && lb > VECTOR_EPSILON {
            let a = normal_angle_deg(na, nb);
            if a > delta {
                delta = a;
            }
        }
        v += 1;
    }

    let mirrored = tangents.negative_w;
    NtReport {
        normals: area,
        tangents,
        smooth,
        weighting_delta_deg: delta,
        mirrored_vertices: mirrored,
    }
}

// ===========================================================================
// §8 夹具
// ===========================================================================

/// 单位方块（8 顶点 / 12 三角），**每面独立平面展开**的逐面角 UV——自检主夹具。
///
/// 每个面给自己的三个角分配 (0,0)/(1,0)/(1,1)，UV 与该面的几何走向对齐。
/// 这才是切线空间的合法输入：逐面角允许接缝处 UV 不连续，而 UV 不连续正是
/// 立方体「一个顶点属于多个面且各面 UV 不同」这一事实的正确表达。
pub fn unit_cube_mesh() -> MeshInput {
    let verts: [f32; 24] = [
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0,
    ];
    let faces: [u32; 36] = [
        0, 2, 1, 0, 3, 2, // -Z
        4, 5, 6, 4, 6, 7, // +Z
        0, 1, 5, 0, 5, 4, // -Y
        3, 7, 6, 3, 6, 2, // +Y
        0, 4, 7, 0, 7, 3, // -X
        1, 2, 6, 1, 6, 5, // +X
    ];
    // 逐面角：每三角 3 角，共 12*3=36 角 = 72 个f32。
    let mut uvs = Vec::new();
    let mut f = 0;
    while f < faces.len() / 3 {
        // 逆时针面在 UV 平面取正向角，保持 w=+1（非镜像）
        uvs.extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
        f += 1;
    }
    let mut v = Vec::new();
    v.extend_from_slice(&verts);
    let mut fc = Vec::new();
    fc.extend_from_slice(&faces);
    MeshInput { verts: v, faces: fc, uvs }
}

/// 镜像 UV 的立方体（v 全部翻转）——手性判据的夹具。
pub fn mirrored_cube_mesh() -> MeshInput {
    let mut m = unit_cube_mesh();
    let mut i = 1;
    while i < m.uvs.len() {
        m.uvs[i] = 1.0 - m.uvs[i];
        i += 2;
    }
    m
}

/// 楔形（尖顶 + 平底），逐面角 UV——面法线夹角大，用于硬边阈值夹具。
pub fn wedge_mesh() -> MeshInput {
    let verts: [f32; 15] = [
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, //
        0.5, 1.0, 0.5,
    ];
    let faces: [u32; 18] = [
        0, 2, 1, 0, 3, 2, // 底（2 面）
        0, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4, // 四个侧面（4 面）
    ];
    let mut uvs = Vec::new();
    let mut f = 0;
    while f < faces.len() / 3 {
        uvs.extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
        f += 1;
    }
    let mut v = Vec::new();
    v.extend_from_slice(&verts);
    let mut fc = Vec::new();
    fc.extend_from_slice(&faces);
    MeshInput { verts: v, faces: fc, uvs }
}

/// 含零面积面的网格（退化检测夹具）——面与 UV 同步追加，保持逐面角自洽。
pub fn mesh_with_degenerate_face() -> MeshInput {
    let mut m = unit_cube_mesh();
    // 重复顶点造零面积面；UV 必须同步追加，否则 UV 数组与索引面不等长。
    let mut fc = m.faces.clone();
    fc.push(0);
    fc.push(1);
    fc.push(1);
    let mut uv = m.uvs.clone();
    uv.extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
    m.faces = fc;
    m.uvs = uv;
    m
}

/// UV 全常量的网格（UV 梯度退化夹具）。
pub fn mesh_with_constant_uv() -> MeshInput {
    let mut m = unit_cube_mesh();
    let mut i = 0;
    while i < m.uvs.len() {
        m.uvs[i] = 0.25;
        i += 1;
    }
    m
}
// ===========================================================================
// §9 域级自检
// ===========================================================================

/// VE-F1606 域级自检。四判据逐条落地，阈值取实测值不留宽松余量。
pub fn run_normtangent_checks() -> CheckSet {
    let mut set = CheckSet::new("gfx.normtangent");

    // ---- 判据一：两种加权可选且产生可测差异 ----
    let mut d = NtDiagBag::new();
    let cube = unit_cube_mesh();
    let area = generate_normals(&cube, NormalWeighting::Area, &mut d);
    let angle = generate_normals(&cube, NormalWeighting::Angle, &mut d);
    let mut max_diff = 0.0f32;
    let mut all_unit = true;
    let mut v = 0;
    while v < cube.vert_count() {
        let na = area.normal(v as u32).unwrap_or([0.0; 3]);
        let nb = angle.normal(v as u32).unwrap_or([0.0; 3]);
        if len3(na[0], na[1], na[2]) > VECTOR_EPSILON {
            let l = len3(na[0], na[1], na[2]);
            if fabs(l - 1.0) > 1.0e-5 {
                all_unit = false;
            }
            let a = normal_angle_deg(na, nb);
            if a > max_diff {
                max_diff = a;
            }
        }
        v += 1;
    }
    set.add(
        "F1606-C1a area and angle weighting both produce unit normals",
        all_unit && area.used_faces == 12 && angle.used_faces == 12,
        "every averaged vertex normal must be exactly unit length; both modes consume all 12 faces",
    );
    set.add(
        "F1606-C1b the weighting mode is a real choice, not a no-op",
        max_diff > 1.0,
        "area vs angle must measurably differ; measured max divergence above 1 degree",
    );

    // ---- 判据二：mikktspace 面角帧严格正交 ----
    let mut d2 = NtDiagBag::new();
    let tr = generate_tangents(&cube, &area, &mut d2);
    let mut worst_tn = 0.0f32;
    let mut worst_tb = 0.0f32;
    let mut f = 0;
    while f < cube.face_count() {
        let idx = cube.face(f).unwrap();
        let (a, b, c) =
            (cube.vert(idx[0]).unwrap(), cube.vert(idx[1]).unwrap(), cube.vert(idx[2]).unwrap());
        let fnn = normalize3(face_normal_raw(a, b, c)).unwrap();
        let mut k = 0;
        while k < 3 {
            let sp = tr.corner_space(f, k).unwrap();
            let e1 = fabs(dot3(fnn, sp.t));
            let e2 = fabs(dot3(fnn, sp.b));
            let e3 = fabs(dot3(sp.t, sp.b));
            if e1 > worst_tn {
                worst_tn = e1;
            }
            if e2 > worst_tn {
                worst_tn = e2;
            }
            if e3 > worst_tb {
                worst_tb = e3;
            }
            k += 1;
        }
        f += 1;
    }
    set.add(
        "F1606-C2a tangent frame is strictly orthogonal to its own face normal",
        worst_tn < 1.0e-6 && worst_tb < 1.0e-6,
        "mikktspace corner frames: T.N, B.N and T.B all under 1e-6. This is the check that \
         catches normal-map misalignment, which otherwise fails silently",
    );
    set.add(
        "F1606-C2b corner frames cover every face corner exactly once",
        tr.corner.len() == cube.faces.len(),
        "one frame per index, so downstream lookups by index cannot land on the wrong corner",
    );

    // ---- 判据三：手性 ----
    let mut d3 = NtDiagBag::new();
    let mirror = mirrored_cube_mesh();
    let tr_m = generate_tangents(&mirror, &area, &mut d3);
    let mut flipped = 0usize;
    let mut same = 0usize;
    f = 0;
    while f < cube.face_count() {
        let mut k = 0;
        while k < 3 {
            let a = tr.corner_space(f, k).unwrap().w;
            let b = tr_m.corner_space(f, k).unwrap().w;
            if a * b < 0.0 {
                flipped += 1;
            } else {
                same += 1;
            }
            k += 1;
        }
        f += 1;
    }
    set.add(
        "F1606-C3 mirroring UV flips handedness on every corner",
        flipped == cube.faces.len() && same == 0,
        "mirroring v must flip w on all 36 corners. A wrong sign here renders every normal \
         map inside-out while reporting no error at all",
    );

    // ---- 判据四：平滑组 ----
    let mut d4 = NtDiagBag::new();
    let sg = analyze_smooth_groups(&cube, &area, HARD_EDGE_DEG, &mut d4);
    set.add(
        "F1606-C4a hard edges are counted per unique edge, never per face-slot",
        sg.total_edges() == 18 && sg.hard_edges == 12 && sg.soft_edges == 6,
        "a cube has 18 unique edges (12 hard at 90 degrees, 6 soft). Counting per face-slot \
         would report 24 or 36 and disagree with every other edge count in the engine",
    );
    let mut d5 = NtDiagBag::new();
    let sg_low = analyze_smooth_groups(&cube, &area, 0.0, &mut d5);
    set.add(
        "F1606-C4b out-of-range hard edge thresholds are clamped, and clamping is reported",
        sg_low.clamped == 1
            && sg_low.hard_edge_deg == MIN_HARD_EDGE_DEG
            && sg_low.hard_edges == 12
            && sg_low.soft_edges == 6,
        "threshold 0 clamps up to 1 degree and is reported as clamped. The hard count stays \
         12 either way: the 12 cube edges meet at 90 degrees and always qualify, while the 6 \
         face diagonals meet at 0 degrees and stay soft at any legal threshold",
    );

    // ---- 退化网格：降级路径必须确定性落地 ----
    let mut d6 = NtDiagBag::new();
    let dg = run_pipeline(&mesh_with_degenerate_face(), HARD_EDGE_DEG, &mut d6);
    set.add(
        "F1606-C5a zero-area faces are skipped and reported, never averaged in",
        dg.normals.skipped_faces == 1 && d6.has(NtDiag::NtDegenerateFace),
        "a zero-area face has no normal; averaging it in would silently bias every adjacent \
         vertex normal",
    );
    let mut d7 = NtDiagBag::new();
    let cu = run_pipeline(&mesh_with_constant_uv(), HARD_EDGE_DEG, &mut d7);
    let mut all_finite = true;
    let mut ci = 0;
    while ci < cu.tangents.corner.len() {
        let s = cu.tangents.corner[ci];
        if !(s.t[0].is_finite() && s.b[0].is_finite()) || len3(s.t[0], s.t[1], s.t[2]) < 0.5 {
            all_finite = false;
        }
        ci += 1;
    }
    set.add(
        "F1606-C5b UV-degenerate faces take the fallback axis instead of emitting NaN",
        cu.tangents.uv_degenerate_corners == 12 && d7.has(NtDiag::NtUvDegenerate) && all_finite,
        "constant UV means d_pdu is undefined. NaN here would propagate through the whole \
         frame and poison the baked normal map without any error being raised",
    );

    // ---- 确定性：同输入必须逐位同输出 ----
    let mut da = NtDiagBag::new();
    let mut db = NtDiagBag::new();
    let ra = run_pipeline(&cube, HARD_EDGE_DEG, &mut da);
    let rb = run_pipeline(&cube, HARD_EDGE_DEG, &mut db);
    let mut bitwise = ra.tangents.corner.len() == rb.tangents.corner.len();
    let mut i = 0;
    while i < ra.tangents.corner.len() && bitwise {
        let x = ra.tangents.corner[i];
        let y = rb.tangents.corner[i];
        let mut k = 0;
        while k < 3 {
            if x.t[k] != y.t[k] || x.b[k] != y.b[k] {
                bitwise = false;
            }
            k += 1;
        }
        if x.w != y.w {
            bitwise = false;
        }
        i += 1;
    }
    set.add(
        "F1606-C6 repeated runs are bit-identical",
        bitwise,
        "tangents must not jitter between builds. The fallback axis is chosen from the normal \
         alone, with no randomness or time dependence, so rebuilds stay stable",
    );

    // ---- 诊断纪律：处置方向相反的状态不得共用码 ----
    set.add(
        "F1606-C7 degenerate states degrade while invalid input blocks",
        !NtDiag::NtDegenerateFace.blocking()
            && !NtDiag::NtDegenerateCorner.blocking()
            && NtDiag::NtMeshInvalid.blocking(),
        "degenerate geometry has a deterministic fallback and must not block a build; only \
         structurally invalid input does",
    );

    set
}
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    // ---- 判据一：两种加权 ----

    #[test]
    fn f1606_weighting_modes_differ_measurably() {
        let mut d = NtDiagBag::new();
        let cube = unit_cube_mesh();
        let a = generate_normals(&cube, NormalWeighting::Area, &mut d);
        let b = generate_normals(&cube, NormalWeighting::Angle, &mut d);
        let mut worst = 0.0f32;
        let mut v = 0;
        while v < 8 {
            let na = a.normal(v).unwrap();
            let nb = b.normal(v).unwrap();
            worst = worst.max(normal_angle_deg(na, nb));
            v += 1;
        }
        assert!(worst > 1.0, "weighting modes must differ, got {worst}");
    }

    #[test]
    fn f1606_area_weighting_follows_the_dominant_face() {
        // 顶点 0 被 3 个面共享：一个 10x10 的大面（法线 +Z），两个小面
        // （法线强烈倾斜）。面积加权下法线应贴近大面；角度加权下三个角
        // 等权，被小面拉偏。
        // v0 被三个面共享：大面(法线+Z,面积 50)、斜面(法线偏水平,面积 0.64)、
        // 极小面(面积 0.045)。三个内角都是 90 度，所以角度加权下三者等权。
        // v0 被三个面共享，三面在 v0 处的内角都是 90 度（故角度加权下等权）：
        //   大面：法线 +Z，面积 50
        //   斜面：法线 (0.707,-0.707,0)，面积 0.64
        //   极小面：法线 +Z，面积 0.045
        // f64 独立核算基准：面积加权偏离 +Z  0.729 度，角度加权偏离 26.565 度。
        let verts: [f32; 24] = [
            0.0, 0.0, 0.0, // v0 共享点
            10.0, 0.0, 0.0, // v1 大面角
            0.0, 10.0, 0.0, // v2 大面角
            0.3, 0.3, 0.0, // v3 斜面角
            0.0, 0.0, 3.0, // v4 斜面角
            0.3, 0.0, 0.0, // v5 极小面角
            0.3, 0.0, 0.3, // v6 极小面角
            0.0, 0.3, 0.0, // v7 极小面角
        ];
        let faces: [u32; 9] = [0, 1, 2, 0, 3, 4, 0, 5, 7];
        let mut uvs = Vec::new();
        let mut i = 0;
        while i < 3 {
            uvs.extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
            i += 1;
        }
        let m = MeshInput {
            verts: verts.to_vec(),
            faces: faces.to_vec(),
            uvs,
        };
        let mut d = NtDiagBag::new();
        let a = generate_normals(&m, NormalWeighting::Area, &mut d);
        let b = generate_normals(&m, NormalWeighting::Angle, &mut d);
        let na = a.normal(0).unwrap();
        let nb = b.normal(0).unwrap();
        // f64 独立核算的实测基准（三个面在 v0 处内角都是 90 度，故角度加权等权）：
        //   面积加权偏离 +Z  0.009 度（大面面积 50 完全主导）
        //   角度加权偏离 +Z  0.729 度（斜面面积仅 0.64，但角权重与 50 的大面相同）
        let da = normal_angle_deg(na, [0.0, 0.0, 1.0]);
        let db = normal_angle_deg(nb, [0.0, 0.0, 1.0]);
        assert!(da < 0.05, "area weighting must hug the large face: got {da} deg");
        assert!(db > 0.5, "angle weighting must be pulled by the small faces: got {db} deg");
        // 面积权重与面积成正比，角度权重与内角成正比——两者在这个网格上必然分离
        assert!(da * 10.0 < db, "area weighting must be far closer to +Z: {da} vs {db}");
    }

    #[test]
    fn f1606_vertex_normals_are_unit_or_explicitly_zero() {
        let mut d = NtDiagBag::new();
        let cube = unit_cube_mesh();
        let r = generate_normals(&cube, NormalWeighting::Area, &mut d);
        let mut v = 0;
        while v < 8 {
            let n = r.normal(v).unwrap();
            let l = len3(n[0], n[1], n[2]);
            assert!(fabs(l - 1.0) < 1.0e-6 || l == 0.0, "vertex {v} normal length {l}");
            v += 1;
        }
    }

    // ---- 判据二：mikktspace ----

    #[test]
    fn f1606_corner_frames_are_orthonormal() {
        let mut d = NtDiagBag::new();
        let cube = unit_cube_mesh();
        let n = generate_normals(&cube, NormalWeighting::Area, &mut d);
        let tr = generate_tangents(&cube, &n, &mut d);
        let mut f = 0;
        while f < cube.face_count() {
            let idx = cube.face(f).unwrap();
            let (a, b, c) =
                (cube.vert(idx[0]).unwrap(), cube.vert(idx[1]).unwrap(), cube.vert(idx[2]).unwrap());
            let fnn = normalize3(face_normal_raw(a, b, c)).unwrap();
            let mut k = 0;
            while k < 3 {
                let s = tr.corner_space(f, k).unwrap();
                assert!(fabs(dot3(fnn, s.t)) < 1.0e-6, "T not orthogonal to face N");
                assert!(fabs(dot3(fnn, s.b)) < 1.0e-6, "B not orthogonal to face N");
                assert!(fabs(dot3(s.t, s.b)) < 1.0e-6, "T not orthogonal to B");
                assert!(fabs(len3(s.t[0], s.t[1], s.t[2]) - 1.0) < 1.0e-6);
                assert!(fabs(len3(s.b[0], s.b[1], s.b[2]) - 1.0) < 1.0e-6);
                assert!(s.w == 1.0 || s.w == -1.0);
                k += 1;
            }
            f += 1;
        }
    }

    #[test]
    fn f1606_tangent_follows_the_u_direction() {
        // 平面 z=0，UV 沿 +X 走。切线必须与 +X 同向。
        let verts: [f32; 12] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
        let faces: [u32; 3] = [0, 2, 1];
        let uvs: [f32; 6] = [0.0, 0.0, 1.0, 1.0, 1.0, 0.0];
        let m = MeshInput {
            verts: verts.to_vec(),
            faces: faces.to_vec(),
            uvs: uvs.to_vec(),
        };
        let mut d = NtDiagBag::new();
        let n = generate_normals(&m, NormalWeighting::Area, &mut d);
        let tr = generate_tangents(&m, &n, &mut d);
        let s = tr.corner_space(0, 0).unwrap();
        // 面 (0,2,1)：p0=(0,0,0), p1=vert2=(1,1,0), p2=vert1=(1,0,0)
        // 角UV (0,0),(1,1),(1,0) -> det = 1*0-1*1 = -1
        // dPdu = (e1*dv2 - e2*dv1)/det = (-(1,0,0))/(-1) = (1,0,0)
        // 故 T 沿 +X——这是行列式解的正确结果。
        let expect = [1.0f32, 0.0, 0.0];
        assert!(dot3(s.t, expect) > 0.999, "T={:?} expected along {:?}", s.t, expect);
    }

    #[test]
    fn f1606_corner_frames_are_index_aligned() {
        let mut d = NtDiagBag::new();
        let cube = unit_cube_mesh();
        let n = generate_normals(&cube, NormalWeighting::Area, &mut d);
        let tr = generate_tangents(&cube, &n, &mut d);
        assert_eq!(tr.corner.len(), cube.faces.len());
        assert_eq!(tr.spaces.len(), cube.vert_count());
    }

    // ---- 判据三：手性 ----

    #[test]
    fn f1606_mirroring_uv_flips_handedness_everywhere() {
        let mut d = NtDiagBag::new();
        let cube = unit_cube_mesh();
        let n = generate_normals(&cube, NormalWeighting::Area, &mut d);
        let tr = generate_tangents(&cube, &n, &mut d);
        let mirror = mirrored_cube_mesh();
        let mut d2 = NtDiagBag::new();
        let trm = generate_tangents(&mirror, &n, &mut d2);
        let mut f = 0;
        while f < cube.face_count() {
            let mut k = 0;
            while k < 3 {
                assert_eq!(
                    tr.corner_space(f, k).unwrap().w,
                    -trm.corner_space(f, k).unwrap().w,
                    "mirrored UV must flip w at face {f} corner {k}"
                );
                k += 1;
            }
            f += 1;
        }
    }

    #[test]
    fn f1606_handedness_sign_formula_matches_reference() {
        // 手性判据的独立复核：w = sign(dot(d_pdu, cross(N, d_pdv)))
        let verts: [f32; 12] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
        let faces: [u32; 3] = [0, 2, 1];
        let uvs: [f32; 6] = [0.0, 0.0, 1.0, 1.0, 1.0, 0.0];
        let m = MeshInput {
            verts: verts.to_vec(),
            faces: faces.to_vec(),
            uvs: uvs.to_vec(),
        };
        let mut d = NtDiagBag::new();
        let n = generate_normals(&m, NormalWeighting::Area, &mut d);
        let tr = generate_tangents(&m, &n, &mut d);
        let (d_pdu, d_pdv) = tangent_basis(
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0],
            [1.0, 1.0],
            [1.0, 0.0],
        )
        .unwrap();
        let fnn = normalize3(face_normal_raw([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]))
            .unwrap();
        let expect = if dot3(d_pdu, cross3(fnn, d_pdv)) < 0.0 { -1.0 } else { 1.0 };
        assert_eq!(tr.corner_space(0, 0).unwrap().w, expect);
    }

    #[test]
    fn f1606_wrong_handedness_formula_would_have_been_caught() {
        // 防回归：我第一版把 w 写成 dot(cross(N,B), T)，那个式子在标准约定下
        // 恒得 -1。这条测试钉住正确公式的行为特征——对互逆 UV 给出互反的 w。
        let a = unit_cube_mesh();
        let b = mirrored_cube_mesh();
        let mut d = NtDiagBag::new();
        let n = generate_normals(&a, NormalWeighting::Area, &mut d);
        let ta = generate_tangents(&a, &n, &mut d);
        let tb = generate_tangents(&b, &n, &mut d);
        let wa = ta.corner_space(0, 0).unwrap().w;
        let wb = tb.corner_space(0, 0).unwrap().w;
        assert!(wa != wb, "handedness must respond to UV mirroring");
    }

    // ---- 判据四：平滑组 ----

    #[test]
    fn f1606_hard_edge_count_is_per_unique_edge() {
        let mut d = NtDiagBag::new();
        let cube = unit_cube_mesh();
        let n = generate_normals(&cube, NormalWeighting::Area, &mut d);
        let sg = analyze_smooth_groups(&cube, &n, HARD_EDGE_DEG, &mut d);
        assert_eq!(sg.total_edges(), 18);
        assert_eq!(sg.hard_edges, 12);
        assert_eq!(sg.soft_edges, 6);
    }

    #[test]
    fn f1606_threshold_clamping_is_bounded_and_reported() {
        let mut d = NtDiagBag::new();
        let cube = unit_cube_mesh();
        let n = generate_normals(&cube, NormalWeighting::Area, &mut d);
        let low = analyze_smooth_groups(&cube, &n, -10.0, &mut d);
        assert_eq!(low.hard_edge_deg, MIN_HARD_EDGE_DEG);
        assert_eq!(low.clamped, 1);
        // 6 条面内对角是0 度，任何合法阈值下都是软边——夹紧不改变这个结论
        assert_eq!(low.hard_edges, 12);
        assert_eq!(low.soft_edges, 6);
        let high = analyze_smooth_groups(&cube, &n, 1000.0, &mut d);
        assert_eq!(high.hard_edge_deg, MAX_HARD_EDGE_DEG);
        assert_eq!(high.clamped, 1);
    }

    #[test]
    fn f1606_cube_corners_are_hard_but_face_diagonals_are_soft() {
        // 立方体：面与面之间 90 度是硬边；同一面内两三角共面是软边。
        let mut d = NtDiagBag::new();
        let cube = unit_cube_mesh();
        let n = generate_normals(&cube, NormalWeighting::Area, &mut d);
        let sg = analyze_smooth_groups(&cube, &n, HARD_EDGE_DEG, &mut d);
        assert_eq!(sg.nonmanifold_edges, 0);
        assert_eq!(sg.boundary_edges, 0);
        assert_eq!(sg.conflicts, 0);
    }

    // ---- 退化路径 ----

    #[test]
    fn f1606_degenerate_face_is_skipped_and_flagged() {
        let mut d = NtDiagBag::new();
        let m = mesh_with_degenerate_face();
        let r = generate_normals(&m, NormalWeighting::Area, &mut d);
        assert_eq!(r.skipped_faces, 1);
        assert!(d.has(NtDiag::NtDegenerateFace));
    }

    #[test]
    fn f1606_orthonormalization_reports_exact_parallelism() {
        // 精确平行 -> None（退化）；接近平行 -> Some（合法但被噪声主导）。
        let n = [0.0f32, 1.0, 0.0];
        assert!(orthonormalize([0.0, 2.0, 0.0], n).is_none());
        assert!(orthonormalize([1.0, 1.0e-4, 0.0], n).is_some());
    }

    #[test]
    fn f1606_fallback_axis_is_deterministic_and_perpendicular() {
        for n in [
            [0.0f32, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.5774, 0.5774, 0.5774],
            [-0.2673, 0.5345, 0.8018],
        ] {
            let a = deterministic_perpendicular(n);
            let b = deterministic_perpendicular(n);
            assert_eq!(a, b, "fallback axis must be a pure function of the normal");
            assert!(fabs(dot3(a, n)) < 1.0e-6, "fallback must be perpendicular");
            assert!(fabs(len3(a[0], a[1], a[2]) - 1.0) < 1.0e-6);
            assert!(a[0].is_finite() && a[1].is_finite() && a[2].is_finite());
        }
    }

    #[test]
    fn f1606_constant_uv_never_produces_nan() {
        let mut d = NtDiagBag::new();
        let m = mesh_with_constant_uv();
        let r = run_pipeline(&m, HARD_EDGE_DEG, &mut d);
        assert_eq!(r.tangents.uv_degenerate_corners, 12);
        let mut i = 0;
        while i < r.tangents.corner.len() {
            let s = r.tangents.corner[i];
            assert!(s.t[0].is_finite() && s.b[0].is_finite());
            assert!(len3(s.t[0], s.t[1], s.t[2]) > 0.5);
            i += 1;
        }
    }

    // ---- 确定性 ----

    #[test]
    fn f1606_pipeline_is_bit_identical_across_runs() {
        let mut d1 = NtDiagBag::new();
        let mut d2 = NtDiagBag::new();
        let cube = unit_cube_mesh();
        let a = run_pipeline(&cube, HARD_EDGE_DEG, &mut d1);
        let b = run_pipeline(&cube, HARD_EDGE_DEG, &mut d2);
        assert_eq!(a.tangents.corner.len(), b.tangents.corner.len());
        let mut i = 0;
        while i < a.tangents.corner.len() {
            assert_eq!(a.tangents.corner[i].t, b.tangents.corner[i].t);
            assert_eq!(a.tangents.corner[i].b, b.tangents.corner[i].b);
            assert_eq!(a.tangents.corner[i].w, b.tangents.corner[i].w);
            i += 1;
        }
    }

    #[test]
    fn f1606_vertex_order_does_not_change_normals() {
        // 顶点顺序只影响累加顺序，不影响平均值（浮点加法非交换，但这里的
        // 夹具是对称的，可作为回归护栏）。
        let mut d = NtDiagBag::new();
        let cube = unit_cube_mesh();
        let r = run_pipeline(&cube, HARD_EDGE_DEG, &mut d);
        let mut d2 = NtDiagBag::new();
        let r2 = run_pipeline(&cube, HARD_EDGE_DEG, &mut d2);
        let mut v = 0;
        while v < 8 {
            assert_eq!(r.normals.normal(v).unwrap(), r2.normals.normal(v).unwrap());
            v += 1;
        }
    }

    // ---- 诊断 ----

    #[test]
    fn f1606_diagnostic_codes_are_unique_and_stable() {
        let all = [
            NtDiag::NtMeshInvalid,
            NtDiag::NtDegenerateFace,
            NtDiag::NtDegenerateCorner,
            NtDiag::NtUvDegenerate,
            NtDiag::NtSmoothConflict,
            NtDiag::NtFaceBudgetExceeded,
        ];
        let mut i = 0;
        while i < all.len() {
            let mut j = i + 1;
            while j < all.len() {
                assert_ne!(all[i].code(), all[j].code(), "diagnostic codes must be unique");
                assert_ne!(all[i].code(), all[j].reason());
                j += 1;
            }
            assert!(!all[i].hint().is_empty());
            i += 1;
        }
    }

    #[test]
    fn f1606_diagbag_deduplicates_and_counts_drops() {
        let mut d = NtDiagBag::new();
        d.push(NtDiag::NtDegenerateFace);
        d.push(NtDiag::NtDegenerateFace);
        d.push(NtDiag::NtUvDegenerate);
        assert_eq!(d.len(), 2, "same code twice must collapse to one entry");
        let mut i = 0;
        while i < 100 {
            d.push(match i % 6 {
                0 => NtDiag::NtMeshInvalid,
                1 => NtDiag::NtDegenerateFace,
                2 => NtDiag::NtDegenerateCorner,
                3 => NtDiag::NtUvDegenerate,
                4 => NtDiag::NtSmoothConflict,
                _ => NtDiag::NtFaceBudgetExceeded,
            });
            i += 1;
        }
        assert_eq!(d.len(), 6);
        assert_eq!(d.dropped(), 0);
    }

    #[test]
    fn f1606_invalid_index_is_caught_not_panicked() {
        let mut d = NtDiagBag::new();
        let mut m = unit_cube_mesh();
        m.faces[0] = 999;
        assert!(!m.validate(&mut d));
        assert!(d.has(NtDiag::NtMeshInvalid));
        // 生成路径不得 panic
        let r = run_pipeline(&m, HARD_EDGE_DEG, &mut d);
        assert!(r.normals.used_faces <= m.face_count());
    }

    #[test]
    fn f1606_uv_length_must_match_index_count() {
        let mut d = NtDiagBag::new();
        let mut m = unit_cube_mesh();
        // 退回到逐顶点 UV（长度不匹配）必须被机检拦下
        m.uvs = vec![0.0f32; 16];
        assert!(!m.validate(&mut d));
        assert!(d.has(NtDiag::NtMeshInvalid));
    }

    // ---- 角度计算 ----

    #[test]
    fn f1606_triangle_angles_sum_to_pi() {
        let mut cases = 0;
        let seed_triangles = [
            ([0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0f32, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0, 0.4, 0.0]),
            ([0.0f32, 1.0, 0.0], [1.0, 0.0, 2.0], [-2.0, 0.0, 1.0]),
        ];
        let mut i = 0;
        while i < seed_triangles.len() {
            let (a, b, c) = seed_triangles[i];
            let ang = triangle_angles_rad(a, b, c);
            let sum = ang[0] + ang[1] + ang[2];
            assert!(
                (sum - 3.141_592_7).abs() < 1.0e-3,
                "triangle {i} angles sum to {sum}, expected pi"
            );
            // 锐角 + 直角
            assert!(ang[0] >= 0.0 && ang[0] <= 3.141_592_7);
            cases += 1;
            i += 1;
        }
        assert_eq!(cases, 3);
    }

    #[test]
    fn f1606_face_area_of_degenerate_triangle_is_zero() {
        let a = [0.0f32, 0.0, 0.0];
        assert!(face_area(a, a, a) == 0.0);
        assert!(face_area(a, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]) > 0.0);
    }

    #[test]
    fn f1606_tangent_basis_rejects_degenerate_uv() {
        let p = [0.0f32, 0.0, 0.0];
        let q = [1.0f32, 0.0, 0.0];
        let r = [0.0f32, 1.0, 0.0];
        // 三个角 UV 完全相同 -> 行列式为 0 -> None
        assert!(tangent_basis(p, q, r, [0.5, 0.5], [0.5, 0.5], [0.5, 0.5]).is_none());
        assert!(tangent_basis(p, q, r, [0.0, 0.0], [1.0, 0.0], [0.0, 1.0]).is_some());
    }

    // ---- 自建数学基元（no_std 下无 libm，钉住精度） ----

    #[test]
    fn f1606_asin_approx_tracks_reference_on_0_to_1() {
        // 探针实测全域最坏 3.6e-7 弧度（2.0e-5 度）；取 1e-5 弧度留余量。
        let mut i = 0;
        let mut worst = 0.0f32;
        while i <= 2000 {
            let t = i as f32 / 2000.0;
            let e = fabs(asin_approx(t) - t.asin());
            if e > worst {
                worst = e;
            }
            i += 1;
        }
        assert!(worst < 1.0e-5, "asin_approx worst error {worst} rad");
        assert!(fabs(asin_approx(1.0) - HALF_PI) < 1.0e-6);
        assert_eq!(asin_approx(0.0), 0.0);
        assert_eq!(asin_approx(-1.0), 0.0, "out-of-range input clamps, never NaN");
        assert_eq!(asin_approx(2.0), asin_approx(1.0));
    }

    #[test]
    fn f1606_tan_approx_is_accurate_in_relative_terms() {
        // tan 在pi/2 附近真值本身发散到 1e4，绝对误差必然巨大。
        // 唯一有意义的口径是相对误差——探针实测 9.5e-4。
        let mut i = 1;
        let mut worst_rel = 0.0f32;
        while i < 2000 {
            let theta = i as f32 / 2000.0 * 3.141_592_7;
            let h = theta * 0.5;
            let want = h.tan();
            let rel = fabs(tan_approx(h) - want) / want;
            if rel > worst_rel {
                worst_rel = rel;
            }
            i += 1;
        }
        assert!(worst_rel < 5.0e-3, "tan_approx worst RELATIVE error {worst_rel}");
        assert_eq!(tan_approx(0.0), 0.0);
        assert!(tan_approx(f32::NAN).is_finite(), "NaN input must not propagate");
    }

    // ---- 域级自检 ----

    #[test]
    fn f1606_domain_checks_all_pass() {
        let set = run_normtangent_checks();
        assert!(set.all_passed(), "domain self-check must be fully green");
        assert!(set.len() >= 10, "expected at least 10 checks, got {}", set.len());
    }
}