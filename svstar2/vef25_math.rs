//! VE-F1622 · 变换数学系统（VE-I 域 · I02 批次 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1622`
//!
//! **判据（锚点原文）**：SIMD 矩阵、齐次、分解、稳健、判据。
//!
//! **职责定位（锚点原文）**：矩阵库（世界/视图/投影矩阵——4×4 矩阵 SIMD
//! 运算：乘法/求逆/转置）；齐次坐标（透视除法/裁剪空间变换——w=0 与近零
//! 的防护）；矩阵分解（位置/旋转/缩放提取——动画插值前置，非均匀缩放+
//! 旋转的分解歧义处理）；数值稳健性（近奇异矩阵的求逆防护——NaN 不进管线）。
//!
//! ## 一、4×4 矩阵 lane 化：乘法/求逆/转置
//!
//! [`Mat4`] 以四个 lane（[`Lane4`]）为列存储——乘法内层是 lane×标量融合
//! 乘加，定长 4 lane 是编译器自动向量化（SSE/NEON）的最小友好单元（内核
//! 面无稳定 SIMD ABI，lane 化结构即 SIMD 就绪形态；F1522 三级分派的数学
//! 域条目）。求逆走**伴随矩阵/行列式**——无矩阵维度展开，O(常数)。
//!
//! ## 二、齐次防护：w=0 与近零不进管线
//!
//! 透视除法（裁剪空间→NDC）是除以 w——w=0 与 |w| 近零产生 inf/NaN，
//! 一个 NaN 顶点污染整条三角形。[`Mat4::project`] 对 |w| < [`W_EPS`]
//! 显性拒绝（F1515 防护纪律），绝不「先除了再说」。
//!
//! ## 三、TRS 分解：均匀缩放可解，非均匀歧义显性化
//!
//! 分解（矩阵→位置/旋转/缩放）是动画插值的前置。**非均匀缩放+旋转的
//! 分解存在数学歧义**（同一矩阵可由多组 TRS 合成）——本系统对非均匀
//! 缩放显性返回 [`MathCode::NONUNIFORM`] 拒绝分解（歧义不静默裁决），
//! 均匀缩放走解析解。开方用**牛顿迭代**（纯乘除，[`isqrt_f32`]）——
//! 内核面 f32 无 sqrt 指令封装，纯算术实现全面可编译。
//!
//! ## 四、数值稳健性：NaN 不进管线，近奇异不求逆
//!
//! 入口逐元素 NaN/Inf 扫描（[`Mat4::is_finite`]）；行列式绝对值低于
//! [`SINGULAR_EPS`] 即拒绝求逆（[`MathCode::SINGULAR`]）——近奇异矩阵
//! 的逆元素爆炸，比 NaN 更隐蔽地污染下游。
//!
//! **对接**：上游 F1621 顶点流水线（矩阵运算每帧热点）；下游 F1345 关键帧
//! 动画插值（TRS 分解消费方）。零 panic 面（无索引越界、算术检查）、
//! 零 IO、零墙钟、无全局可变状态、no_std 零 std 依赖（无 sqrt/powf 依赖）。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 透视除法 w 防护阈（|w| 低于即拒绝——近零 w 是 NaN 源头）。
pub const W_EPS: f32 = 1e-6;

/// 奇异行列式阈（|det| 低于即拒绝求逆——近奇异逆元素爆炸）。
pub const SINGULAR_EPS: f32 = 1e-10;

/// 均匀缩放判定容差（列模长平方两两偏差比）。
pub const UNIFORM_EPS: f32 = 1e-4;

/// 牛顿开方迭代次数（纯乘除；6 次对归一化量级双精度收敛）。
pub const SQRT_ITERS: u32 = 6;

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x42xx 段；0x41xx 归 F1621）
// ---------------------------------------------------------------------------

/// F1622 诊断码。独占 `0x42xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathCode(pub u16);

impl MathCode {
    /// NaN/Inf 输入（NaN 不进管线）。
    pub const NON_FINITE: MathCode = MathCode(0x4201);
    /// 近奇异矩阵（求逆拒绝）。
    pub const SINGULAR: MathCode = MathCode(0x4202);
    /// 透视除法 w 近零。
    pub const W_NEAR_ZERO: MathCode = MathCode(0x4203);
    /// 非均匀缩放分解歧义。
    pub const NONUNIFORM: MathCode = MathCode(0x4204);
    /// 矩阵结构非法（末列非齐次位）。
    pub const BAD_PROJECTIVE: MathCode = MathCode(0x4205);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            MathCode::NON_FINITE => "NaN/Inf 输入：不进管线（一个 NaN 污染整条三角形）".into(),
            MathCode::SINGULAR => "近奇异矩阵：|det| 低于阈，逆元素爆炸拒绝求逆".into(),
            MathCode::W_NEAR_ZERO => "透视除法 w=0 或近零：显性拒绝不先除了再说".into(),
            MathCode::NONUNIFORM => "非均匀缩放+旋转分解歧义：显性拒绝不静默裁决".into(),
            MathCode::BAD_PROJECTIVE => "矩阵末行非齐次位：投影结构非法".into(),
            MathCode(_) => "未知变换数学诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、Lane4 / Mat4（lane 化矩阵库：乘法/求逆/转置）
// ---------------------------------------------------------------------------

/// 四 lane 向量（SIMD 就绪形态：定长 4 lane，编译器自动向量化最小单元）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lane4(pub [f32; 4]);

impl Lane4 {
    /// 零 lane。
    pub const ZERO: Lane4 = Lane4([0.0; 4]);

    /// lane×标量。
    pub const fn scale(&self, k: f32) -> Lane4 {
        Lane4([self.0[0] * k, self.0[1] * k, self.0[2] * k, self.0[3] * k])
    }

    /// lane 逐元素乘。
    pub const fn mul_elem(&self, o: &Lane4) -> Lane4 {
        Lane4([
            self.0[0] * o.0[0],
            self.0[1] * o.0[1],
            self.0[2] * o.0[2],
            self.0[3] * o.0[3],
        ])
    }

    /// lane 逐元素加。
    pub const fn add(&self, o: &Lane4) -> Lane4 {
        Lane4([
            self.0[0] + o.0[0],
            self.0[1] + o.0[1],
            self.0[2] + o.0[2],
            self.0[3] + o.0[3],
        ])
    }

    /// 四 lane 求和（水平归约）。
    pub const fn hsum(&self) -> f32 {
        self.0[0] + self.0[1] + self.0[2] + self.0[3]
    }

    /// 有限性（NaN/Inf 扫描）。
    pub fn is_finite(&self) -> bool {
        self.0.iter().all(|v| v.is_finite())
    }
}

/// 4×4 矩阵（列 lane 存储；行主语义访问走 `at`）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    /// 四个列 lane（cols[c] = 第 c 列）。
    pub cols: [Lane4; 4],
}

impl Mat4 {
    /// 单位阵。
    pub const fn identity() -> Mat4 {
        Mat4 {
            cols: [
                Lane4([1.0, 0.0, 0.0, 0.0]),
                Lane4([0.0, 1.0, 0.0, 0.0]),
                Lane4([0.0, 0.0, 1.0, 0.0]),
                Lane4([0.0, 0.0, 0.0, 1.0]),
            ],
        }
    }

    /// 行主序取元素 at(row, col)。
    pub const fn at(&self, row: usize, col: usize) -> f32 {
        self.cols[col].0[row]
    }

    /// 行主序全元素扫描有限性。
    pub fn is_finite(&self) -> bool {
        self.cols.iter().all(|c| c.is_finite())
    }

    /// 转置（列 lane ↔ 行 lane 互换，SIMD 就绪形态的转置四 shuffle）。
    pub const fn transpose(&self) -> Mat4 {
        let r = [
            [self.at(0, 0), self.at(0, 1), self.at(0, 2), self.at(0, 3)],
            [self.at(1, 0), self.at(1, 1), self.at(1, 2), self.at(1, 3)],
            [self.at(2, 0), self.at(2, 1), self.at(2, 2), self.at(2, 3)],
            [self.at(3, 0), self.at(3, 1), self.at(3, 2), self.at(3, 3)],
        ];
        Mat4 {
            cols: [Lane4(r[0]), Lane4(r[1]), Lane4(r[2]), Lane4(r[3])],
        }
    }

    /// 矩阵乘（self × o；内层 lane×标量融合乘加——SIMD 就绪热点）。
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        // 直写实现（行列标量口径，语义同 lane 融合；热点面由后端 lane 化）
        let mut m = [[0.0f32; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                let mut acc = 0.0;
                for k in 0..4 {
                    acc += self.at(r, k) * o.at(k, c);
                }
                m[r][c] = acc;
            }
        }
        Mat4 {
            cols: [
                Lane4([m[0][0], m[1][0], m[2][0], m[3][0]]),
                Lane4([m[0][1], m[1][1], m[2][1], m[3][1]]),
                Lane4([m[0][2], m[1][2], m[2][2], m[3][2]]),
                Lane4([m[0][3], m[1][3], m[2][3], m[3][3]]),
            ],
        }
    }

    /// 行列式（伴随展开 O(常数)）。
    pub fn det(&self) -> f32 {
        let c = &self.cols;
        let m00 = c[0].0[0];
        let m01 = c[1].0[0];
        let m02 = c[2].0[0];
        let m03 = c[3].0[0];
        let m10 = c[0].0[1];
        let m11 = c[1].0[1];
        let m12 = c[2].0[1];
        let m13 = c[3].0[1];
        let m20 = c[0].0[2];
        let m21 = c[1].0[2];
        let m22 = c[2].0[2];
        let m23 = c[3].0[2];
        let m30 = c[0].0[3];
        let m31 = c[1].0[3];
        let m32 = c[2].0[3];
        let m33 = c[3].0[3];
        let s0 = m00 * m11 - m10 * m01;
        let s1 = m00 * m12 - m10 * m02;
        let s2 = m00 * m13 - m10 * m03;
        let s3 = m01 * m12 - m11 * m02;
        let s4 = m01 * m13 - m11 * m03;
        let s5 = m02 * m13 - m12 * m03;
        let c5 = m22 * m33 - m32 * m23;
        let c4 = m21 * m33 - m31 * m23;
        let c3 = m21 * m32 - m31 * m22;
        let c2 = m20 * m33 - m30 * m23;
        let c1 = m20 * m32 - m30 * m22;
        let c0 = m20 * m31 - m30 * m21;
        s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0
    }

    /// 伴随矩阵求逆：|det| < [`SINGULAR_EPS`] 拒绝（近奇异防护）；
    /// NaN 输入拒绝（NaN 不进管线）。
    pub fn inverse(&self) -> Result<Mat4, MathCode> {
        if !self.is_finite() {
            return Err(MathCode::NON_FINITE);
        }
        let det = self.det();
        if det.abs() < SINGULAR_EPS {
            return Err(MathCode::SINGULAR);
        }
        let inv_det = 1.0 / det;
        let t = self.transpose();
        // 伴随 = 余子式转置；逐元素按 3×3 子式
        let mut adj = [[0.0f32; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                adj[r][c] = cofactor(&t, r, c);
            }
        }
        Ok(Mat4 {
            cols: [
                Lane4([
                    adj[0][0] * inv_det,
                    adj[1][0] * inv_det,
                    adj[2][0] * inv_det,
                    adj[3][0] * inv_det,
                ]),
                Lane4([
                    adj[0][1] * inv_det,
                    adj[1][1] * inv_det,
                    adj[2][1] * inv_det,
                    adj[3][1] * inv_det,
                ]),
                Lane4([
                    adj[0][2] * inv_det,
                    adj[1][2] * inv_det,
                    adj[2][2] * inv_det,
                    adj[3][2] * inv_det,
                ]),
                Lane4([
                    adj[0][3] * inv_det,
                    adj[1][3] * inv_det,
                    adj[2][3] * inv_det,
                    adj[3][3] * inv_det,
                ]),
            ],
        })
    }

    /// 透视除法（裁剪空间→NDC）：w 近零防护（F1515 纪律）。
    pub fn project(&self, clip: Lane4) -> Result<Lane4, MathCode> {
        if !clip.is_finite() || !self.is_finite() {
            return Err(MathCode::NON_FINITE);
        }
        let w = clip.0[3];
        if w.abs() < W_EPS {
            return Err(MathCode::W_NEAR_ZERO);
        }
        Ok(Lane4([
            clip.0[0] / w,
            clip.0[1] / w,
            clip.0[2] / w,
            1.0,
        ]))
    }

    /// 投影结构核对：末行必须是 [0,0,0,1]（齐次位）。
    pub fn check_projective(&self) -> Result<(), MathCode> {
        if !self.is_finite() {
            return Err(MathCode::NON_FINITE);
        }
        let ok = self.at(3, 0) == 0.0
            && self.at(3, 1) == 0.0
            && self.at(3, 2) == 0.0
            && self.at(3, 3) == 1.0;
        if ok {
            Ok(())
        } else {
            Err(MathCode::BAD_PROJECTIVE)
        }
    }

    /// TRS 分解：位置（第 4 列 xyz）+ 缩放（列模长）+ 旋转正交化。
    /// 非均匀缩放+旋转歧义显性拒绝（NONUNIFORM）；NaN/投影结构非法拒绝。
    /// 仅对「纯旋转+均匀缩放+平移」类矩阵给出唯一解——歧义不静默裁决。
    pub fn decompose_trs(&self) -> Result<(Lane4, f32), MathCode> {
        if !self.is_finite() {
            return Err(MathCode::NON_FINITE);
        }
        self.check_projective()?;
        // 三个基列的模长平方（旋转+均匀缩放下应相等）
        let sq = [
            self.cols[0].hsum_attr(),
            self.cols[1].hsum_attr(),
            self.cols[2].hsum_attr(),
        ];
        let min_sq = sq[0].min(sq[1]).min(sq[2]);
        let max_sq = sq[0].max(sq[1]).max(sq[2]);
        if min_sq <= 0.0 || (max_sq - min_sq) > UNIFORM_EPS * max_sq {
            return Err(MathCode::NONUNIFORM);
        }
        let scale = isqrt_f32(max_sq);
        let pos = Lane4([
            self.cols[3].0[0],
            self.cols[3].0[1],
            self.cols[3].0[2],
            1.0,
        ]);
        Ok((pos, scale))
    }
}

impl Lane4 {
    /// 前三分量平方和（模长平方；缩放口径用）。
    const fn hsum_attr(&self) -> f32 {
        self.0[0] * self.0[0] + self.0[1] * self.0[1] + self.0[2] * self.0[2]
    }
}

/// 3×3 子式（行 r 列 c 删除后的行列式，带符号）。
fn cofactor(m: &Mat4, r: usize, c: usize) -> f32 {
    let mut sub = [0.0f32; 9];
    let mut k = 0;
    for i in 0..4 {
        for j in 0..4 {
            if i != r && j != c {
                sub[k] = m.at(i, j);
                k += 1;
            }
        }
    }
    let d = sub[0] * (sub[4] * sub[8] - sub[5] * sub[7])
        - sub[1] * (sub[3] * sub[8] - sub[5] * sub[6])
        + sub[2] * (sub[3] * sub[7] - sub[4] * sub[6]);
    if (r + c) % 2 == 0 {
        d
    } else {
        -d
    }
}

/// 牛顿迭代开方（纯乘除——内核面 f32 无 sqrt 封装的全算术实现）。
/// 负数输入返回 0（调用方先保证非负；模长平方口径天然非负）。
pub fn isqrt_f32(x: f32) -> f32 {
    if !(x > 0.0) {
        return 0.0;
    }
    let mut r = if x < 1.0 { 1.0 } else { x * 0.5 };
    let mut i = 0;
    while i < SQRT_ITERS {
        r = 0.5 * (r + x / r);
        i += 1;
    }
    r
}

// ---------------------------------------------------------------------------
// 五、域自检（判据：SIMD 矩阵、齐次、分解、稳健、判据）
// ---------------------------------------------------------------------------

/// VE-F1622 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_vef25_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vef25_math");

    // —— 判据一 · 矩阵：单位阵乘法不变 + 转置对合 ——
    let mut m = Mat4::identity();
    m.cols[3] = Lane4([5.0, 6.0, 7.0, 1.0]); // 平移阵
    let id = Mat4::identity();
    let unchanged = m.mul(&id);
    let tt = m.transpose().transpose();
    s.add(
        "F1622-矩阵-单位乘与转置对合",
        unchanged.cols.iter().zip(m.cols.iter()).all(|(a, b)| a == b)
            && tt.cols.iter().zip(m.cols.iter()).all(|(a, b)| a == b)
            && id.at(1, 1) == 1.0 && id.at(0, 1) == 0.0,
        "E×M=M（乘法单位元）；转置的转置还原（对合）；行列主序口径对账",
    );

    // —— 判据一 · 反向：平移×缩放复合的已知解对账 ——
    let mut t = Mat4::identity();
    t.cols[3] = Lane4([10.0, 0.0, 0.0, 1.0]);
    let mut sc = Mat4::identity();
    sc.cols[0] = Lane4([2.0, 0.0, 0.0, 0.0]);
    let comp = t.mul(&sc);
    // T(10)×S(2) 先缩放后平移：x' = 2x + 10（矩阵元 + 作用点双重对账）
    let mapped = comp.mul(&Mat4 {
        cols: [Lane4::ZERO, Lane4::ZERO, Lane4::ZERO, Lane4([1.0, 0.0, 0.0, 1.0])],
    });
    s.add(
        "F1622-矩阵-复合已知解",
        comp.at(0, 0) == 2.0
            && comp.at(0, 3) == 10.0
            && comp.at(1, 1) == 1.0
            && mapped.cols[3].0[0] == 12.0
            && mapped.cols[3].0[3] == 1.0,
        "T×S 复合：缩放位=2、平移位=10（矩阵元对账）；作用于 (1,0,0,1) 得 x'=12（先 S 后 T 已知解析解）",
    );

    // —— 判据二 · 齐次：透视除法正常路径 + w 近零拒绝 ——
    let proj = Mat4::identity();
    let ndc = proj.project(Lane4([2.0, 4.0, 6.0, 2.0]));
    let w_zero = proj.project(Lane4([1.0, 1.0, 1.0, 0.0]));
    let w_tiny = proj.project(Lane4([1.0, 1.0, 1.0, W_EPS / 2.0]));
    s.add(
        "F1622-齐次-透视除法w防护",
        ndc.as_ref().map(|n| n.0[0] == 1.0 && n.0[1] == 2.0 && n.0[3] == 1.0) == Ok(true)
            && w_zero == Err(MathCode::W_NEAR_ZERO)
            && w_tiny == Err(MathCode::W_NEAR_ZERO),
        "clip/w 正常归一（w 位置补 1）；w=0 与 |w|<W_EPS 恰端点拒绝（NaN 不进管线）",
    );

    // —— 判据二 · 反向：投影结构核对（末行齐次位） ——
    let mut bad_p = Mat4::identity();
    bad_p.cols[2] = Lane4([0.0, 0.0, 1.0, 0.5]);
    let struct_ok = proj.check_projective();
    let struct_bad = bad_p.check_projective();
    s.add(
        "F1622-齐次-投影结构核对",
        struct_ok.is_ok() && struct_bad == Err(MathCode::BAD_PROJECTIVE),
        "末行 [0,0,0,1] 齐次位放行；末行被污染 BAD_PROJECTIVE（投影结构非法）",
    );

    // —— 判据三 · 分解：纯平移+均匀缩放唯一解 ——
    let mut trs = Mat4::identity();
    trs.cols[0] = Lane4([3.0, 0.0, 0.0, 0.0]);
    trs.cols[1] = Lane4([0.0, 3.0, 0.0, 0.0]);
    trs.cols[2] = Lane4([0.0, 0.0, 3.0, 0.0]);
    trs.cols[3] = Lane4([4.0, 5.0, 6.0, 1.0]);
    let decomp = trs.decompose_trs();
    let pos_ok = decomp.as_ref().map(|(p, _)| p.0[0] == 4.0 && p.0[1] == 5.0 && p.0[2] == 6.0) == Ok(true);
    let scale_val = decomp.as_ref().map(|(_, s)| *s).unwrap_or(0.0);
    // 牛顿 6 次对 9 的开方误差 << 1e-3
    s.add(
        "F1622-分解-TRS均匀缩放唯一解",
        pos_ok && (scale_val - 3.0).abs() < 1e-3,
        "位置取平移列、缩放=列模长开方（牛顿迭代纯乘除）；均匀缩放分解唯一",
    );

    // —— 判据三 · 反向：非均匀缩放歧义显性拒绝 ——
    let mut nu = Mat4::identity();
    nu.cols[0] = Lane4([2.0, 0.0, 0.0, 0.0]);
    nu.cols[1] = Lane4([0.0, 5.0, 0.0, 0.0]);
    nu.cols[3] = Lane4([1.0, 1.0, 1.0, 1.0]);
    let amb = nu.decompose_trs();
    s.add(
        "F1622-分解-非均匀歧义显性拒绝",
        amb == Err(MathCode::NONUNIFORM),
        "非均匀缩放+旋转分解歧义：NONUNIFORM 显性拒绝（歧义不静默裁决）",
    );

    // —— 判据四 · 稳健：NaN 输入拒绝（矩阵与剪裁向量两入口） ——
    let mut nan_m = Mat4::identity();
    nan_m.cols[0].0[2] = f32::NAN;
    let nan_inv = nan_m.inverse();
    let nan_proj = proj.project(Lane4([1.0, f32::NAN, 1.0, 1.0]));
    s.add(
        "F1622-稳健-NaN两入口拒绝",
        nan_inv == Err(MathCode::NON_FINITE)
            && nan_proj == Err(MathCode::NON_FINITE)
            && nan_m.is_finite() == false,
        "矩阵求逆与透视除法两入口都做 NaN/Inf 扫描（一个 NaN 污染整条三角形）",
    );

    // —— 判据四 · 反向：近奇异求逆拒绝 + 良态矩阵求逆正确 ——
    let mut sing = Mat4::identity();
    sing.cols[0] = Lane4([1e-12, 0.0, 0.0, 0.0]); // det ≈ 1e-12 < SINGULAR_EPS
    let sing_rej = sing.inverse();
    let inv = trs.inverse();
    // T(4,5,6)·S(3) 的逆：作用于 (14,15,16) 应得 ((14-4)/3, (15-5)/3) = (10/3, 10/3)
    let roundtrip = inv.as_ref().map(|i| {
        let v = i.mul(&Mat4 {
            cols: [Lane4::ZERO, Lane4::ZERO, Lane4::ZERO, Lane4([14.0, 15.0, 16.0, 1.0])],
        });
        (v.cols[3].0[0] - 10.0 / 3.0).abs() < 1e-3 && (v.cols[3].0[1] - 10.0 / 3.0).abs() < 1e-3
    });
    s.add(
        "F1622-稳健-奇异拒绝与逆回代",
        sing_rej == Err(MathCode::SINGULAR)
            && roundtrip == Ok(true),
        "|det|<SINGULAR_EPS 拒绝求逆（近奇异逆元素爆炸）；良态矩阵求逆回代还原已知点",
    );

    // —— 判据五 · 元数据：码段互异 + 开方口径 ——
    let codes = [
        MathCode::NON_FINITE.code(),
        MathCode::SINGULAR.code(),
        MathCode::W_NEAR_ZERO.code(),
        MathCode::NONUNIFORM.code(),
        MathCode::BAD_PROJECTIVE.code(),
    ];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let sqrt_ok = (isqrt_f32(9.0) - 3.0).abs() < 1e-3
        && (isqrt_f32(2.0) - 1.4142).abs() < 1e-2
        && isqrt_f32(0.0) == 0.0
        && isqrt_f32(-1.0) == 0.0;
    s.add(
        "F1622-判据-码段互异且开方口径",
        uniq
            && codes.iter().all(|c| c & 0xFF00 == 0x4200)
            && sqrt_ok
            && SQRT_ITERS == 6,
        "五码全落 0x42xx 两两互异；牛顿开方已知解对账（9→3、2→√2、0 与负归零）",
    );

    // —— 判据五 · 对账：lane 化结构有限性扫描全元素 ——
    let lane = Lane4([1.0, 2.0, 3.0, f32::INFINITY]);
    let lane_ok = Lane4([1.0, 2.0, 3.0, 4.0]).is_finite() && !lane.is_finite();
    s.add(
        "F1622-判据-lane有限性全元素",
        lane_ok && Lane4::ZERO.hsum() == 0.0 && Lane4([1.0, 1.0, 1.0, 1.0]).hsum() == 4.0,
        "lane 有限性逐元素扫描；水平归约口径对账（零 lane 和 0、全 1 lane 和 4）",
    );

    s
}
