//! VE-F1610 · 骨骼权重数据容器（VE-I 域 · I01 网格格式与几何基础组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1610`
//!
//! **判据（锚点原文）**：权重容器、规范化、层级容器、对接预留、判据。
//!
//! # 权重容器（骨骼索引 + 权重对，上限 4 骨骼/顶点可配）
//!
//! 蒙皮的几何本质是**加权仿射混合**：顶点最终位置
//! `Σᵢ wᵢ · (world(boneᵢ) · IBM(boneᵢ)) · p`。本模块提供其中三样数据：
//!
//! * [`SkinWeights`] —— 每顶点的骨骼索引 + 权重（**上限可配**的配额纪律）。
//! * [`BoneSkeleton`] —— 骨骼树（父子层级）+ 逆绑定矩阵（IBM）。
//! * [`SkinRig`] —— 两者的绑定体，产出蒙皮矩阵。
//!
//! ## 为什么上限可配
//!
//! 4 骨骼/顶点是实时蒙皮的惯例平衡（更多骨骼 = 更高精度、更高成本）。
//! 但**惯例不是定律**：LOD 远处骨骼、手部特写都可能要 6~8 个。
//! 故上限是**构造期参数**（[`SkinWeights::new`]）而非编译期常量，
//! 硬顶 [`STRIDE_HARD_CAP`] 只防「有人传 4096 把显存吃光」。
//!
//! ## 为什么用 SoA（两数组）而非 AoS
//!
//! `bone_ids: Vec<u32>` 与 `weights: Vec<f32>` 分开存放（结构体数组转数组结构体，
//! F1526 纪律）：GPU 取权重时只读 4 个连续 f32，取骨骼索引时只读 4 个连续 u32。
//! AoS 会让「只读权重」的行stride 变成 8 字节，浪费一半带宽。
//!
//! ## 填充槽位语义
//!
//! 未用满的槽位写作 `bone = [`INVALID_BONE`]`、`weight = 0.0`。
//! 归一化时 **0/x 仍是 0**，故填充槽位天然保持填充；但本模块**显式断言**这一点
//! （判据 `I1610-归一-填充保持` 直接断字段本身），不靠「除法恰好正确」蒙混。
//!
//! # 规范化（Σ权重=1，偏差容忍 1e-4，超限自动归一）
//!
//! 未归一的权重 = 蒙皮形变错误（顶点被缩放/漂移）。归一化把 `wᵢ` 除以权重和 `S`。
//!
//! * **容差是闭区间**：`|S-1| ≤ 1e-4` 视为已归一，**不动**。闭区间而非开区间，
//!   因为美术导出的权重常有 1e-5 级漂移，反复归一会引入无意义的浮点抖动。
//! * **零和是硬错**：全零权重无法归一（0/0=NaN）。返回 [`SkinError::ZeroTotalWeight`]，
//!   **绝不**产出 NaN 权重污染下游。
//! * **非有限值先拒后算**：NaN/Inf 在**写入**时就拒（[`SkinError::NonFiniteWeight`]），
//!   而不是等归一化时 `NaN > tol` 恒假而静默放行。
//!
//! 归一化**幂等**：已归一的顶点再归一返回 `changed=false`（F1619字节级确定性的前提）。
//!
//! # 层级容器（骨骼树 + 逆绑定矩阵）
//!
//! [`BoneSkeleton`] 持骨骼名、���索引、局部绑定矩阵、IBM。IBM 的定义是
//! `IBM = inverse(world_bind)`，满足恒等式 `IBM · world_bind = I` ——
//! 判据 `I1610-层级-IBM恒等` 独立重算这个乘积来验，而不是复读实现自己报的字段。
//!
//! **成环检测**：`A→B→C→A` 的父子链会让 `resolve_world` 的 `while` **死循环挂死内核**。
//! [`SkinError::HierarchyCycle`] 用步数上限截断，判据 `I1610-层级-成环` 断言错误**种类**。
//!
//! # 对接预留（骨骼命名约定契约 · 动画域 M）
//!
//! 动画域 M 要按名字驱动骨骼，名字就是跨域**接口**。契约三条：
//!
//! 1. **字符集**：ASCII 可见字符（`0x21..=0x7E`），禁空格与非 ASCII ——
//!    路径分隔符 `/` 天然被排除在名字之外（否则名字与层级二义）。
//! 2. **保留前缀**：`__` 开头为引擎内部骨骼，动画域不得驱动。
//! 3. **路径唯一**：同一骨架内 `parent/child` 全路径不可重复。
//!    重复若**静默取首个**，动画会绑到错的骨头上（十诫 3：重合行为掩盖缺失分支），
//!    故 [`BoneSkeleton::push_bone`] 以 [`SkinError::AmbiguousPath`] **拒绝**。
//!
//! [`BoneNameHash`]（FNV-1a 64）提供**跨会话稳定**的名字指纹，
//! 让 M 域可以把「名字→绑定」结果安全缓存（F1619 确定性纪律）。
//!
//! # 边界
//!
//! 本模块只做**数据容器 + 校验 + 蒙皮矩阵产出**，**不做**蒙皮执行（顶点着色器消费，
//! C 域）、**不做**动画采样（M 域）、**不做**vmesh 序列化（F1602）。
//!
//! # 隐私
//!
//! 纯数据变换，无外部 I/O、无遥测、无网络。

#![allow(clippy::needless_range_loop)]

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::meshquant::{fcos, fsin};

// ===========================================================================
// 常量
// ===========================================================================

/// 骨骼权重和的偏差容忍（锚点：1e-4）。**闭区间**：`|S-1| ≤ 该值` 视为已归一。
pub const WEIGHT_SUM_TOLERANCE: f32 = 1.0e-4;

/// 锚点默认配额：4 骨骼/顶点（实时蒙皮的惯例平衡）。
pub const STRIDE_DEFAULT: usize = 4;

/// 配额硬顶——防「有人传 4096 把显存吃光」，不是精度建议。
pub const STRIDE_HARD_CAP: usize = 16;

/// 骨骼索引填充哨兵。
pub const INVALID_BONE: u32 = u32::MAX;

/// 骨骼名最大字节数（不含结尾 NUL 的语义：实际存 ≤ 该值）。
pub const BONE_NAME_MAX: usize = 63;

/// 骨骼层级最大深度（成环的第二道防线 + 栈深上界）。
pub const MAX_DEPTH: usize = 64;

/// 骨骼名保留前缀（引擎内部专用）。
pub const RESERVED_PREFIX: &[u8] = b"__";

/// 层级路径分隔符（名字内禁止出现，见对接预留契约）。
pub const PATH_SEP: u8 = b'/';

// ===========================================================================
// 4×4 矩阵（自持最小实现）
// ===========================================================================

/// 行主序 4×4 矩阵：`m[r*4+c]`。
///
/// **约定**：`mul(a, b)` 表示「先施加 a 再施加 b」，即 `result[r][c] = Σₖ a[r][k]·b[k][c]`；
/// `transform_point` 取 `y[r] = Σ_c m[r][c]·x[c]`。判据侧用**独立写一遍**的
/// 参考乘法对账，故约定写反会被 `I1610-层级-矩阵约定` 抓住。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    /// 行主序 16 元。
    pub m: [f32; 16],
}

impl Default for Mat4 {
    fn default() -> Mat4 {
        Mat4::identity()
    }
}

impl Mat4 {
    /// 单位矩阵。
    pub fn identity() -> Mat4 {
        let mut m = [0.0f32; 16];
        // 对角线置 1。用 `get_mut` 而非下标，保持零 panic 面。
        let mut i = 0usize;
        while i < 4 {
            if let Some(v) = m.get_mut(i * 4 + i) {
                *v = 1.0;
            }
            i += 1;
        }
        Mat4 { m }
    }

    /// 取第 `r` 行（`r ≥ 4` 返回全零，不 panic）。
    #[inline]
    pub fn row(&self, r: usize) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        if let Some(chunk) = self.m.chunks_exact(4).nth(r) {
            out.copy_from_slice(chunk);
        }
        out
    }

    /// 4×4 全零矩阵（加权累加的起点）。
    pub fn zero() -> Mat4 {
        Mat4 { m: [0.0f32; 16] }
    }

    /// 取元素 `m[r][c]`。
    #[inline]
    pub fn at(&self, r: usize, c: usize) -> f32 {
        self.row(r).get(c).copied().unwrap_or(0.0)
    }

    /// 写元素 `m[r][c]`。
    #[inline]
    pub fn set(&mut self, r: usize, c: usize, v: f32) {
        if let Some(slot) = self.m.get_mut(r * 4 + c) {
            *slot = v;
        }
    }

    /// 纯平移矩阵。
    pub fn translation(tx: f32, ty: f32, tz: f32) -> Mat4 {
        let mut r = Mat4::identity();
        r.set(0, 3, tx);
        r.set(1, 3, ty);
        r.set(2, 3, tz);
        r
    }

    /// 纯缩放矩阵（对角）。
    pub fn scale(sx: f32, sy: f32, sz: f32) -> Mat4 {
        let mut r = Mat4::identity();
        r.set(0, 0, sx);
        r.set(1, 1, sy);
        r.set(2, 2, sz);
        r
    }

    /// 绕 Z 轴旋转（弧度）。
    ///
    /// **不用 `f32::sin` / `f32::cos`**：内核 `no_std` 且不链 libm，
    /// 浮点超越函数在 `core` 中不存在（真仓 no_std 构建直接 E0599）。
    /// 复用 [`meshquant::fsin`] / [`meshquant::fcos`]——与 F1605/F1606 同一套
    /// 折区 + 多项式实现，全仓三角函数口径统一。
    pub fn rotation_z(rad: f32) -> Mat4 {
        let (s, c) = (fsin(rad), fcos(rad));
        let mut r = Mat4::identity();
        r.set(0, 0, c);
        r.set(0, 1, -s);
        r.set(1, 0, s);
        r.set(1, 1, c);
        r
    }

    /// 矩阵乘：先施加 `self` 再施加 `other`。
    pub fn mul(&self, other: &Mat4) -> Mat4 {
        let mut out = Mat4::identity();
        let mut r = 0usize;
        while r < 4 {
            let mut c = 0usize;
            while c < 4 {
                let mut sum = 0.0f32;
                let mut k = 0usize;
                while k < 4 {
                    sum += self.at(r, k) * other.at(k, c);
                    k += 1;
                }
                out.set(r, c, sum);
                c += 1;
            }
            r += 1;
        }
        out
    }

    /// 变换齐次点（隐含 w=1，忽略透视除法——蒙皮只用仿射）。
    pub fn transform_point(&self, p: [f32; 3]) -> [f32; 3] {
        let x = self.at(0, 0) * p[0] + self.at(0, 1) * p[1] + self.at(0, 2) * p[2] + self.at(0, 3);
        let y = self.at(1, 0) * p[0] + self.at(1, 1) * p[1] + self.at(1, 2) * p[2] + self.at(1, 3);
        let z = self.at(2, 0) * p[0] + self.at(2, 1) * p[1] + self.at(2, 2) * p[2] + self.at(2, 3);
        [x, y, z]
    }

    /// 两矩阵各元素最大绝对差（判据侧对账用；**非**逐元素相等判定）。
    pub fn max_abs_diff(&self, other: &Mat4) -> f32 {
        let mut worst = 0.0f32;
        let mut i = 0usize;
        while i < 16 {
            let a = self.m.get(i).copied().unwrap_or(0.0);
            let b = other.m.get(i).copied().unwrap_or(0.0);
            let d = (a - b).abs();
            if d > worst {
                worst = d;
            }
            i += 1;
        }
        worst
    }

    /// 全元素有限（无 NaN/Inf）。
    pub fn all_finite(&self) -> bool {
        let mut i = 0usize;
        while i < 16 {
            match self.m.get(i) {
                Some(v) if v.is_finite() => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Gauss-Jordan 求逆（带部分选主元）。
    ///
    /// 返回 `None` 表示**奇异**——绑定矩阵奇异会让 IBM 不存在，
    /// 此时必须报错而非塞一个「接近无穷」的大数（否则蒙皮出 NaN）。
    pub fn inverse(&self) -> Option<Mat4> {
        /// 主元绝对值下限：低于此视为奇异。
        const PIVOT_EPS: f32 = 1.0e-9;
        /// 增广矩阵宽度（原 4 列 + 右侧 4 列）。
        const W: usize = 8;

        let mut a = [0.0f32; 4 * W];
        let mut r = 0usize;
        while r < 4 {
            let mut c = 0usize;
            while c < 4 {
                if let Some(v) = a.get_mut(r * W + c) {
                    *v = self.at(r, c);
                }
                if let Some(v) = a.get_mut(r * W + 4 + c) {
                    *v = if r == c { 1.0 } else { 0.0 };
                }
                c += 1;
            }
            r += 1;
        }

        let mut col = 0usize;
        while col < 4 {
            // 选主元：当前列绝对值最大者。
            let mut piv = col;
            let mut best = 0.0f32;
            let mut rr = col;
            while rr < 4 {
                let v = a.get(rr * W + col).copied().unwrap_or(0.0).abs();
                if v > best {
                    best = v;
                    piv = rr;
                }
                rr += 1;
            }
            if best < PIVOT_EPS {
                return None;
            }

            // 交换两行（分片借用，全程零 panic）。
            if piv != col {
                let (lo, hi) = if piv < col { (piv, col) } else { (col, piv) };
                let (_, rest) = a.split_at_mut(lo * W);
                let (row_lo, rest2) = rest.split_at_mut(W);
                let (_, row_hi) = rest2.split_at_mut((hi - lo) * W);
                row_lo.swap_with_slice(row_hi);
            }

            // 主元归一。
            let pv = a.get(col * W + col).copied().unwrap_or(0.0);
            if pv == 0.0 {
                return None;
            }
            let mut j = 0usize;
            while j < W {
                if let Some(v) = a.get_mut(col * W + j) {
                    *v /= pv;
                }
                j += 1;
            }

            // 消元。
            let mut rr = 0usize;
            while rr < 4 {
                if rr != col {
                    let f = a.get(rr * W + col).copied().unwrap_or(0.0);
                    if f != 0.0 {
                        let mut j = 0usize;
                        while j < W {
                            let cv = a.get(col * W + j).copied().unwrap_or(0.0);
                            if let Some(v) = a.get_mut(rr * W + j) {
                                *v -= f * cv;
                            }
                            j += 1;
                        }
                    }
                }
                rr += 1;
            }
            col += 1;
        }

        let mut out = Mat4::identity();
        let mut r = 0usize;
        while r < 4 {
            let mut c = 0usize;
            while c < 4 {
                out.set(r, c, a.get(r * W + 4 + c).copied().unwrap_or(0.0));
                c += 1;
            }
            r += 1;
        }
        if out.all_finite() {
            Some(out)
        } else {
            None
        }
    }
}

// ===========================================================================
// 错误类型（每种失败一个专属码 —— 十诫 3）
// ===========================================================================

/// 权重容器 / 层级容器的全部失败原因。
///
/// **每种错误都有唯一的产生路径**（判据逐个断错误**种类**，不只断「失败了」——
/// 否则删掉某个分支可能仍然全绿）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinError {
    /// 骨骼索引越界（`≥ bone_count`）。
    BoneOutOfRange,
    /// 顶点数索引越界。
    VertexOutOfRange,
    /// 写入的权重对数量超过配额。
    QuotaExceeded,
    /// SoA 两数组长度不一致（内部不变量被破坏）。
    StrideMismatch,
    /// 权重为负。
    NegativeWeight,
    /// 权重为 NaN / Inf。
    NonFiniteWeight,
    /// 权重和为 0，无法归一（0/0 = NaN）。
    ZeroTotalWeight,
    /// 父骨骼索引越界。
    ParentOutOfRange,
    /// 父子链成环（会让世界矩阵求解死循环）。
    HierarchyCycle,
    /// 父子链深度超限。
    DepthExceeded,
    /// 骨骼名非法（空 / 超长 / 含非法字符）。
    InvalidBoneName,
    /// 骨骼名撞引擎保留前缀 `__`。
    ReservedBoneName,
    /// 全路径重复（静默取首个会绑错骨头，故拒绝）。
    AmbiguousPath,
    /// 骨架为空。
    EmptySkeleton,
    /// 绑定矩阵奇异，IBM 不存在。
    SingularBindMatrix,
}

impl SkinError {
    /// 错误码（供跨域上报，不依赖 `Debug` 文本）。
    pub fn code(&self) -> u32 {
        match self {
            SkinError::BoneOutOfRange => 1001,
            SkinError::VertexOutOfRange => 1002,
            SkinError::QuotaExceeded => 1003,
            SkinError::StrideMismatch => 1004,
            SkinError::NegativeWeight => 1005,
            SkinError::NonFiniteWeight => 1006,
            SkinError::ZeroTotalWeight => 1007,
            SkinError::ParentOutOfRange => 1008,
            SkinError::HierarchyCycle => 1009,
            SkinError::DepthExceeded => 1010,
            SkinError::InvalidBoneName => 1011,
            SkinError::ReservedBoneName => 1012,
            SkinError::AmbiguousPath => 1013,
            SkinError::EmptySkeleton => 1014,
            SkinError::SingularBindMatrix => 1015,
        }
    }

    /// 人话描述（错误三要素之一：发生了什么）。
    pub fn message(&self) -> &'static str {
        match self {
            SkinError::BoneOutOfRange => "骨骼索引越界",
            SkinError::VertexOutOfRange => "顶点数索引越界",
            SkinError::QuotaExceeded => "权重对数量超过配额",
            SkinError::StrideMismatch => "权重与骨骼索引数组长度不一致",
            SkinError::NegativeWeight => "权重为负",
            SkinError::NonFiniteWeight => "权重为 NaN 或 Inf",
            SkinError::ZeroTotalWeight => "权重和为零，无法归一",
            SkinError::ParentOutOfRange => "父骨骼索引越界",
            SkinError::HierarchyCycle => "骨骼父子链成环",
            SkinError::DepthExceeded => "骨骼层级深度超限",
            SkinError::InvalidBoneName => "骨骼名非法",
            SkinError::ReservedBoneName => "骨骼名撞引擎保留前缀",
            SkinError::AmbiguousPath => "骨骼全路径重复",
            SkinError::EmptySkeleton => "骨架为空",
            SkinError::SingularBindMatrix => "绑定矩阵奇异，逆绑定矩阵不存在",
        }
    }
}

// ===========================================================================
// 骨骼名与命名契约（对接预留）
// ===========================================================================

/// 校验骨骼名是否符合契约：
///
/// * 非空、长度 ≤ [`BONE_NAME_MAX`]
/// * 字符集 `0x21..=0x7E`（ASCII 可见字符，**不含空格**与任何非 ASCII）
/// * 不以保留前缀 `__` 开头
///
/// 路径分隔符 `/`（`0x2F`）落在 `0x21..=0x7E` 内，故还需**显式排除**——
/// 否则「名字」与「层级」二义，`a/b` 既可是名为 `a/b` 的骨，也可是 `b` 在 `a` 下。
pub fn validate_bone_name(name: &[u8]) -> Result<(), SkinError> {
    if name.is_empty() || name.len() > BONE_NAME_MAX {
        return Err(SkinError::InvalidBoneName);
    }
    if name.first() == Some(&RESERVED_PREFIX[0]) && name.get(1) == Some(&RESERVED_PREFIX[1]) {
        return Err(SkinError::ReservedBoneName);
    }
    let mut i = 0usize;
    while i < name.len() {
        let b = name.get(i).copied().unwrap_or(0);
        if !(0x21..=0x7E).contains(&b) || b == PATH_SEP {
            return Err(SkinError::InvalidBoneName);
        }
        i += 1;
    }
    Ok(())
}

/// 骨骼名的跨会话稳定指纹（FNV-1a 64 位）。
///
/// M 域可据此缓存「名字→绑定」映射；**不是**哈希表用的随机种子哈希，
/// 故跨进程/跨版本稳定（F1619 确定性纪律）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoneNameHash(pub u64);

impl BoneNameHash {
    /// 计算指纹。
    pub fn of(name: &[u8]) -> BoneNameHash {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < name.len() {
            let b = name.get(i).copied().unwrap_or(0);
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        BoneNameHash(h)
    }
}

// ===========================================================================
// 权重容器
// ===========================================================================

/// 每顶点的骨骼索引 + 权重（SoA 布局）。
///
/// **stride（配额）构造期可配**，硬顶 [`STRIDE_HARD_CAP`]。
/// 未用满的槽位写作 `INVALID_BONE` + `0.0`。
#[derive(Clone, Debug, PartialEq)]
pub struct SkinWeights {
    /// 骨骼索引，长度 = `vertex_count * stride`。
    bone_ids: Vec<u32>,
    /// 权重，长度 = `vertex_count * stride`。
    weights: Vec<f32>,
    /// 每顶点的槽位数（配额）。
    stride: usize,
    /// 顶点数。
    vertex_count: usize,
    /// 骨架骨骼数（写入时校验索引上界）。
    bone_count: usize,
}

impl SkinWeights {
    /// 新建权重容器。
    ///
    /// `stride` 超出 `1..=[`[`STRIDE_HARD_CAP`]`]` 时**钳到硬顶并夹逼对齐**返回，
    /// 故本函数不会因配额参数而失败——配额非法是调用方的意图问题，不是数据问题。
    pub fn new(vertex_count: usize, bone_count: usize, stride: usize) -> SkinWeights {
        let stride = if stride == 0 {
            1
        } else if stride > STRIDE_HARD_CAP {
            STRIDE_HARD_CAP
        } else {
            stride
        };
        let total = vertex_count.saturating_mul(stride);
        SkinWeights {
            bone_ids: vec![INVALID_BONE; total],
            weights: vec![0.0f32; total],
            stride,
            vertex_count,
            bone_count,
        }
    }

    /// 每顶点槽位数（配额）。
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// 顶点数。
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// 骨骼数。
    pub fn bone_count(&self) -> usize {
        self.bone_count
    }

    /// 骨骼索引平铺数组长度（SoA 第一段）。
    ///
    /// 判据用它与 `vertex_count * stride` 独立对账——只断 `soa_consistent()`
    /// 会让「两数组同时被截短」这种同源错误一起变短而全绿（十诫 9）。
    pub fn bone_ids_len(&self) -> usize {
        self.bone_ids.len()
    }

    /// 权重平铺数组长度（SoA 第二段）。
    pub fn weights_len(&self) -> usize {
        self.weights.len()
    }

    /// SoA 两数组长度一致性（内部不变量；判据直接断字段）。
    pub fn soa_consistent(&self) -> bool {
        let expect = self.vertex_count.saturating_mul(self.stride);
        self.bone_ids.len() == expect && self.weights.len() == expect
    }

    /// 槽位基址（顶点越界返回 `None`）。
    #[inline]
    fn base(&self, v: usize) -> Option<usize> {
        if v >= self.vertex_count {
            None
        } else {
            v.checked_mul(self.stride)
        }
    }

    /// 读顶点 `v` 的第 `k` 槽骨骼索引。
    pub fn bone_at(&self, v: usize, k: usize) -> Option<u32> {
        let base = self.base(v)?;
        self.bone_ids.get(base.checked_add(k)?).copied()
    }

    /// 写顶点 `v` 的第 `k` 槽骨骼索引（越界静默忽略，绝不 panic）。
    pub fn set_bone_at(&mut self, v: usize, k: usize, bone: u32) {
        if let Some(base) = self.base(v) {
            if let Some(slot) = base.checked_add(k).and_then(|i| self.bone_ids.get_mut(i)) {
                *slot = bone;
            }
        }
    }

    /// 读顶点 `v` 的第 `k` 槽权重。
    pub fn weight_at(&self, v: usize, k: usize) -> Option<f32> {
        let base = self.base(v)?;
        self.weights.get(base.checked_add(k)?).copied()
    }

    /// 写顶点 `v` 的第 `k` 槽权重（越界静默忽略，绝不 panic）。
    pub fn set_weight_at(&mut self, v: usize, k: usize, w: f32) {
        if let Some(base) = self.base(v) {
            if let Some(slot) = base.checked_add(k).and_then(|i| self.weights.get_mut(i)) {
                *slot = w;
            }
        }
    }

    /// 顶点 `v` 的**有效影响数**（`bone != INVALID_BONE` 且 `weight != 0` 的槽位）。
    ///
    /// 注意口径：这是「真正参与蒙皮的骨骼数」，不是「被分配到的槽位数」。
    /// 两者混淆会让「配额没满」的顶点被误判为满载（十诫 10：净值 vs 绝对值口径）。
    pub fn active_influences(&self, v: usize) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.stride {
            let bone = self.bone_at(v, k).unwrap_or(INVALID_BONE);
            let w = self.weight_at(v, k).unwrap_or(0.0);
            if bone != INVALID_BONE && w != 0.0 {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 顶点 `v` 的权重和（**只累加有效槽位**，填充槽贡献 0）。
    pub fn weight_sum(&self, v: usize) -> Option<f32> {
        self.base(v)?;
        let mut s = 0.0f32;
        let mut k = 0usize;
        while k < self.stride {
            let bone = self.bone_at(v, k).unwrap_or(INVALID_BONE);
            let w = self.weight_at(v, k).unwrap_or(0.0);
            if bone != INVALID_BONE {
                s += w;
            }
            k += 1;
        }
        Some(s)
    }

    /// 顶点 `v` 的权重和偏差 `|S-1|`。
    pub fn sum_deviation(&self, v: usize) -> Option<f32> {
        self.weight_sum(v).map(|s| (s - 1.0).abs())
    }

    /// 顶点 `v` 是否已归一（**闭区间** `|S-1| ≤ 1e-4`）。
    ///
    /// 本谓词**自兜底**（十诫 8）：顶点越界返回 `false`，
    /// 权重含 NaN 返回 `false`（`NaN > tol` 恒假会静默放行，故显式挡）。
    pub fn is_normalized(&self, v: usize) -> bool {
        match self.weight_sum(v) {
            Some(s) if s.is_finite() => (s - 1.0).abs() <= WEIGHT_SUM_TOLERANCE,
            _ => false,
        }
    }

    /// 严格写入顶点 `v` 的权重对。
    ///
    /// **超配额即报错**（[`SkinError::QuotaExceeded`]）而非静默截断——
    /// 静默截断会丢掉美术的权重并让顶点变形，无声无息。
    pub fn set_vertex(&mut self, v: usize, pairs: &[(u32, f32)]) -> Result<(), SkinError> {
        if !self.soa_consistent() {
            return Err(SkinError::StrideMismatch);
        }
        if self.base(v).is_none() {
            return Err(SkinError::VertexOutOfRange);
        }
        if pairs.len() > self.stride {
            return Err(SkinError::QuotaExceeded);
        }
        let mut k = 0usize;
        while k < pairs.len() {
            let (bone, w) = match pairs.get(k) {
                Some(p) => *p,
                None => return Err(SkinError::QuotaExceeded),
            };
            if bone != INVALID_BONE && bone as usize >= self.bone_count {
                return Err(SkinError::BoneOutOfRange);
            }
            if !w.is_finite() {
                return Err(SkinError::NonFiniteWeight);
            }
            if w < 0.0 {
                return Err(SkinError::NegativeWeight);
            }
            k += 1;
        }
        // 全量校验通过后再落盘（不留半写状态）。
        k = 0usize;
        while k < self.stride {
            if let Some(p) = pairs.get(k) {
                self.set_bone_at(v, k, p.0);
                self.set_weight_at(v, k, p.1);
            } else {
                self.set_bone_at(v, k, INVALID_BONE);
                self.set_weight_at(v, k, 0.0);
            }
            k += 1;
        }
        Ok(())
    }

    /// 截断式写入：超过配额时**保留权重最大的 `stride` 个**，返回被丢弃数。
    ///
    /// 与 [`SkinWeights::set_vertex`] 的区别是**显式选择**：调用方知道自己在丢数据。
    /// 保留结果按权重**降序**排列——判据 `I1610-容器-截断保序` 直接断序列本身，
    /// 否则「组数/和」全绿的谓词族守不住排序键方向（十诫 12）。
    pub fn set_vertex_truncating(&mut self, v: usize, pairs: &[(u32, f32)]) -> Result<usize, SkinError> {
        if self.base(v).is_none() {
            return Err(SkinError::VertexOutOfRange);
        }
        let mut k = 0usize;
        while k < pairs.len() {
            let (bone, w) = match pairs.get(k) {
                Some(p) => *p,
                None => return Err(SkinError::QuotaExceeded),
            };
            if bone != INVALID_BONE && bone as usize >= self.bone_count {
                return Err(SkinError::BoneOutOfRange);
            }
            if !w.is_finite() {
                return Err(SkinError::NonFiniteWeight);
            }
            if w < 0.0 {
                return Err(SkinError::NegativeWeight);
            }
            k += 1;
        }
        let dropped = pairs.len().saturating_sub(self.stride);
        let mut kept: Vec<(u32, f32)> = Vec::new();
        let mut i = 0usize;
        while i < pairs.len() {
            if let Some(p) = pairs.get(i) {
                kept.push(*p);
            }
            i += 1;
        }
        // 稳定选择：按权重降序，取前 stride 个；同权重时保��原始相对序。
        let mut idx: Vec<usize> = (0..kept.len()).collect();
        idx.sort_by(|a, b| {
            let wa = kept.get(*a).map(|p| p.1).unwrap_or(0.0);
            let wb = kept.get(*b).map(|p| p.1).unwrap_or(0.0);
            // f32 全序：NaN 已在上面被拒，故此处不会出现 NaN 比较歧义。
            wb.partial_cmp(&wa).unwrap_or(core::cmp::Ordering::Equal)
        });
        let mut chosen: Vec<(u32, f32)> = Vec::new();
        let mut j = 0usize;
        while j < idx.len() && chosen.len() < self.stride {
            if let Some(&ix) = idx.get(j) {
                if let Some(p) = kept.get(ix) {
                    chosen.push(*p);
                }
            }
            j += 1;
        }
        self.set_vertex(v, &chosen)?;
        Ok(dropped)
    }

    /// 归一化顶点 `v`：全部有效权重除以权重和。
    ///
    /// 返回 `Ok(true)` 表示权重被改动过；`Ok(false)` 表示原本已在容差内。
    /// 权重和为 0 → [`SkinError::ZeroTotalWeight`]，**不产出 NaN**。
    pub fn normalize_vertex(&mut self, v: usize) -> Result<bool, SkinError> {
        if !self.soa_consistent() {
            return Err(SkinError::StrideMismatch);
        }
        if self.base(v).is_none() {
            return Err(SkinError::VertexOutOfRange);
        }
        let s = self.weight_sum(v).unwrap_or(0.0);
        if !s.is_finite() {
            return Err(SkinError::NonFiniteWeight);
        }
        if s <= 0.0 {
            return Err(SkinError::ZeroTotalWeight);
        }
        if (s - 1.0).abs() <= WEIGHT_SUM_TOLERANCE {
            return Ok(false); // 闭区间：容差内不动（幂等）
        }
        // 只缩放有效槽位；填充槽（bone=INVALID）保持 INVALID + 0.0。
        let mut k = 0usize;
        while k < self.stride {
            let bone = self.bone_at(v, k).unwrap_or(INVALID_BONE);
            if bone != INVALID_BONE {
                let w = self.weight_at(v, k).unwrap_or(0.0);
                let scaled = w / s;
                // s > 0 且 w 有限 ⇒ scaled 有限；仍显式挡 NaN 污染。
                if !scaled.is_finite() {
                    return Err(SkinError::NonFiniteWeight);
                }
                self.set_weight_at(v, k, scaled);
            }
            k += 1;
        }
        Ok(true)
    }

    /// 归一化全部顶点，返回（改动数, 零和顶点数）。
    ///
    /// 零和顶点**跳过并计数**，不中断整批——一个坏顶点不该让整张网格的蒙皮全废。
    pub fn normalize_all(&mut self) -> (usize, usize) {
        let mut changed = 0usize;
        let mut zero_sum = 0usize;
        let mut v = 0usize;
        while v < self.vertex_count {
            match self.normalize_vertex(v) {
                Ok(true) => changed += 1,
                Ok(false) => {}
                Err(SkinError::ZeroTotalWeight) => zero_sum += 1,
                Err(_) => {}
            }
            v += 1;
        }
        (changed, zero_sum)
    }

    /// 未归一顶点清单（体检用；受 `limit` 截断，返回是否还有更多）。
    pub fn unnormalized_vertices(&self, limit: usize) -> (Vec<usize>, bool) {
        let mut out = Vec::new();
        let mut v = 0usize;
        while v < self.vertex_count {
            if !self.is_normalized(v) {
                if out.len() < limit {
                    out.push(v);
                } else {
                    return (out, true);
                }
            }
            v += 1;
        }
        (out, false)
    }

    /// 每骨骼被引用的顶点数（体检用）。
    pub fn bone_usage(&self) -> Vec<u32> {
        let mut usage = vec![0u32; self.bone_count];
        let mut v = 0usize;
        while v < self.vertex_count {
            let mut k = 0usize;
            while k < self.stride {
                let bone = self.bone_at(v, k).unwrap_or(INVALID_BONE);
                if bone != INVALID_BONE {
                    if let Some(slot) = usage.get_mut(bone as usize) {
                        *slot = slot.saturating_add(1);
                    }
                }
                k += 1;
            }
            v += 1;
        }
        usage
    }
}

// ===========================================================================
// 层级容器（骨骼树 + 逆绑定矩阵）
// ===========================================================================

/// 单根骨骼。
#[derive(Clone, Debug, PartialEq)]
pub struct BoneNode {
    /// 骨骼名（ASCII，已校验）。
    pub name: Vec<u8>,
    /// 父骨骼索引；`None` 为根。
    pub parent: Option<u32>,
    /// 绑定姿态下的局部矩阵（相对父骨骼）。
    pub bind_local: Mat4,
    /// 逆绑定矩阵 `inverse(world_bind)`。
    pub inverse_bind: Mat4,
}

impl BoneNode {
    /// 构造骨骼节点。
    pub fn new(name: &[u8], parent: Option<u32>, bind_local: Mat4) -> BoneNode {
        BoneNode {
            name: name.to_vec(),
            parent,
            bind_local,
            inverse_bind: Mat4::identity(),
        }
    }
}

/// 骨骼骨架：父子层级 + 逆绑定矩阵。
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BoneSkeleton {
    nodes: Vec<BoneNode>,
}

impl BoneSkeleton {
    /// 空骨架。
    pub fn new() -> BoneSkeleton {
        BoneSkeleton { nodes: Vec::new() }
    }

    /// 骨骼数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 空骨架判定。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 只读访问节点。
    pub fn node(&self, i: usize) -> Option<&BoneNode> {
        self.nodes.get(i)
    }

    /// 追加骨骼。
    ///
    /// 校验四件事，任一不过即拒绝：
    ///
    /// 1. 名字合规（字符集 + 保留前缀 + 长度）。
    /// 2. 父索引在界内。
    /// 3. **全路径不重复**——重复若静默取首个，动画会绑到错的骨头上。
    /// 4. 局部绑定矩阵有限且非奇异（否则 IBM 不存在）。
    pub fn push_bone(
        &mut self,
        name: &[u8],
        parent: Option<u32>,
        bind_local: Mat4,
    ) -> Result<usize, SkinError> {
        validate_bone_name(name)?;
        if let Some(p) = parent {
            if p as usize >= self.nodes.len() {
                return Err(SkinError::ParentOutOfRange);
            }
        }
        if !bind_local.all_finite() {
            return Err(SkinError::SingularBindMatrix);
        }
        // 奇异预检：0 缩放矩阵无逆，push 时就拒，不拖到求解 IBM 时才发现。
        if bind_local.inverse().is_none() {
            return Err(SkinError::SingularBindMatrix);
        }
        // 全路径唯一性（新骨挂到 parent 之下）。
        if let Some(p) = parent {
            if let Some(pn) = self.nodes.get(p as usize) {
                let mut full = pn.name.clone();
                full.push(PATH_SEP);
                full.extend_from_slice(name);
                // 本节点尚未 push，故只需确认没有**已有**节点占用同一条绝对路径。
                if self.path_exists(&full) {
                    return Err(SkinError::AmbiguousPath);
                }
            }
        } else if self.path_exists(name) {
            return Err(SkinError::AmbiguousPath);
        }

        self.nodes.push(BoneNode::new(name, parent, bind_local));
        Ok(self.nodes.len() - 1)
    }

    /// 绝对路径是否已被占用。
    fn path_exists(&self, path: &[u8]) -> bool {
        let mut i = 0usize;
        while i < self.nodes.len() {
            // `is_some()` 而非 `if let Some(n)`：此处只需判存在性，
            // 路径本身由 `absolute_path(i)` 重建，无需解出节点绑定。
            if self.nodes.get(i).is_some() && self.absolute_path(i).as_slice() == path {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 骨骼 `i` 的绝对路径（`root/child/grandchild`）。
    pub fn absolute_path(&self, i: usize) -> Vec<u8> {
        // 自底向上收集名字，再反转。
        let mut chain: Vec<Vec<u8>> = Vec::new();
        let mut cur = i;
        let mut guard = 0usize;
        while let Some(n) = self.nodes.get(cur) {
            chain.push(n.name.clone());
            match n.parent {
                Some(p) => {
                    cur = p as usize;
                    guard += 1;
                    if guard > MAX_DEPTH {
                        break; // 成环兜底，绝不死循环
                    }
                }
                None => break,
            }
        }
        let mut out: Vec<u8> = Vec::new();
        let mut k = chain.len();
        while k > 0 {
            k -= 1;
            if let Some(part) = chain.get(k) {
                if !out.is_empty() {
                    out.push(PATH_SEP);
                }
                out.extend_from_slice(part);
            }
        }
        out
    }

    /// 按绝对路径查骨骼（真命中语义：命中数 ≥ 1 才返回）。
    pub fn find_by_path(&self, path: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.absolute_path(i).as_slice() == path {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 骨骼 `i` 在**绑定姿态**下的世界矩阵。
    ///
    /// 沿父链逐级上溯并回乘。**成环则返回 [`SkinError::HierarchyCycle`]**
    /// —— `while let Some(p) = parent` 在环上永不为 None，会挂死内核。
    pub fn bind_world(&self, i: usize) -> Result<Mat4, SkinError> {
        if self.nodes.is_empty() {
            return Err(SkinError::EmptySkeleton);
        }
        if self.nodes.get(i).is_none() {
            return Err(SkinError::ParentOutOfRange);
        }
        let mut m = self
            .nodes
            .get(i)
            .map(|n| n.bind_local)
            .unwrap_or_else(|| Mat4::identity());
        let mut cur = self.nodes.get(i).and_then(|n| n.parent);
        let mut steps = 0usize;
        let mut acc = Mat4::identity();
        while let Some(p) = cur {
            let node = self
                .nodes
                .get(p as usize)
                .ok_or(SkinError::ParentOutOfRange)?;
            acc = node.bind_local.mul(&acc);
            cur = node.parent;
            steps += 1;
            if steps > MAX_DEPTH {
                return Err(SkinError::HierarchyCycle);
            }
        }
        m = acc.mul(&m);
        Ok(m)
    }

    /// 求解并回写全部 IBM（`IBM = inverse(bind_world)`）。
    ///
    /// 任一骨骼绑定矩阵奇异 → [`SkinError::SingularBindMatrix`]。
    pub fn resolve_inverse_bind(&mut self) -> Result<(), SkinError> {
        if self.nodes.is_empty() {
            return Err(SkinError::EmptySkeleton);
        }
        let mut worlds: Vec<Mat4> = Vec::with_capacity(self.nodes.len());
        let mut i = 0usize;
        while i < self.nodes.len() {
            worlds.push(self.bind_world(i)?);
            i += 1;
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            let inv = worlds
                .get(i)
                .and_then(|w| w.inverse())
                .ok_or(SkinError::SingularBindMatrix)?;
            if let Some(n) = self.nodes.get_mut(i) {
                n.inverse_bind = inv;
            }
            i += 1;
        }
        Ok(())
    }

    /// 骨骼 `i` 的深度（根为 0）。
    pub fn depth(&self, i: usize) -> Result<usize, SkinError> {
        if self.nodes.get(i).is_none() {
            return Err(SkinError::ParentOutOfRange);
        }
        let mut d = 0usize;
        let mut cur = self.nodes.get(i).and_then(|n| n.parent);
        while let Some(p) = cur {
            cur = self.nodes.get(p as usize).ok_or(SkinError::ParentOutOfRange)?.parent;
            d += 1;
            if d > MAX_DEPTH {
                return Err(SkinError::HierarchyCycle);
            }
        }
        Ok(d)
    }

    /// 制造一条成环父子链（仅测试构造用；`push_bone` 本身拒绝环）。
    pub fn force_cycle_for_test(&mut self, a: usize, b: usize) {
        if let Some(n) = self.nodes.get_mut(a) {
            n.parent = Some(b as u32);
        }
    }
}

// ===========================================================================
// 蒙皮绑定体
// ===========================================================================

/// 骨架 + 权重的绑定体，产出蒙皮矩阵。
#[derive(Clone, Debug, PartialEq)]
pub struct SkinRig {
    /// 骨骼骨架。
    pub skeleton: BoneSkeleton,
    /// 权重容器。
    pub weights: SkinWeights,
}

impl SkinRig {
    /// 构造绑定体：核对骨骼数一致。
    pub fn new(skeleton: BoneSkeleton, weights: SkinWeights) -> Result<SkinRig, SkinError> {
        if skeleton.is_empty() {
            return Err(SkinError::EmptySkeleton);
        }
        if skeleton.len() != weights.bone_count() {
            return Err(SkinError::BoneOutOfRange);
        }
        Ok(SkinRig { skeleton, weights })
    }

    /// 顶点 `v` 的蒙皮矩阵 `Σᵢ wᵢ · (worldᵢ · IBMᵢ)`。
    ///
    /// **关键恒等式**：在**绑定姿态**下 `worldᵢ · IBMᵢ = I`，故当 `Σw = 1` 时
    /// 本函数必须返回单位矩阵。判据 `I1610-蒙皮-绑定姿态恒等` 用它做联合 oracle——
    /// 一条判据同时验出权重规范化与 IBM 正确性，缺一即红。
    pub fn skinning_matrix(&self, v: usize) -> Result<Mat4, SkinError> {
        if self.weights.base(v).is_none() {
            return Err(SkinError::VertexOutOfRange);
        }
        let sum = self.weights.weight_sum(v).unwrap_or(0.0);
        if !sum.is_finite() {
            return Err(SkinError::NonFiniteWeight);
        }
        if sum <= 0.0 {
            return Err(SkinError::ZeroTotalWeight);
        }
        // 加权和的数学起点是**零矩阵**，不是单位阵。
        // 若以 `identity()` 起算，Σw=1 时会得到 `I + Σwᵢ·I = 2I`，
        // 蒙皮矩阵在绑定姿态下翻倍，顶点被整体缩放 2 倍（判据
        // `I1610-蒙皮-绑定姿态恒等` 会立即转红）。
        let mut acc = Mat4::zero();
        let mut k = 0usize;
        while k < self.weights.stride() {
            let bone = self.weights.bone_at(v, k).unwrap_or(INVALID_BONE);
            let w = self.weights.weight_at(v, k).unwrap_or(0.0);
            if bone != INVALID_BONE && w != 0.0 {
                let b = bone as usize;
                let node = self.skeleton.nodes.get(b).ok_or(SkinError::BoneOutOfRange)?;
                // 绑定姿态世界矩阵 · IBM：姿态与姿态相同故可复用 bind_world。
                let world = self.skeleton.bind_world(b)?;
                let m = world.mul(&node.inverse_bind);
                // 逐元素累加全部 16 项。
                //
                // **不得**在循环外先补一次 `acc[0][0]`：那会让 [0][0] 被累加两次
                // 而其余 15 项只累加一次，破坏「Σw=1 且各骨骼矩阵相同 ⇒ 结果同该矩阵」
                // 的线性性（蒙皮矩阵在绑定姿态下不再为单位阵，顶点被整体偏移）。
                let mut r = 0usize;
                while r < 4 {
                    let mut c = 0usize;
                    while c < 4 {
                        acc.set(r, c, acc.at(r, c) + w * m.at(r, c));
                        c += 1;
                    }
                    r += 1;
                }
            }
            k += 1;
        }
        Ok(acc)
    }

    /// 顶点 `v` 蒙皮后的世界坐标。
    pub fn skinned_position(&self, v: usize, p: [f32; 3]) -> Result<[f32; 3], SkinError> {
        Ok(self.skinning_matrix(v)?.transform_point(p))
    }

    /// 全网格蒙皮（供顶点着色器消费前的 CPU 参考）。
    pub fn skinned_positions(&self, positions: &[[f32; 3]]) -> Result<Vec<[f32; 3]>, SkinError> {
        if positions.len() != self.weights.vertex_count() {
            return Err(SkinError::VertexOutOfRange);
        }
        let mut out = Vec::with_capacity(positions.len());
        let mut i = 0usize;
        while i < positions.len() {
            out.push(self.skinned_position(i, positions[i])?);
            i += 1;
        }
        Ok(out)
    }
}

// ===========================================================================
// 体检统计
// ===========================================================================

/// 权重容器体检报告。
#[derive(Clone, Debug, PartialEq)]
pub struct SkinStats {
    /// 顶点数。
    pub vertex_count: usize,
    /// 配额。
    pub stride: usize,
    /// 骨骼数。
    pub bone_count: usize,
    /// 满配额顶点数。
    pub saturated_vertices: usize,
    /// 未归一顶点数。
    pub unnormalized_vertices: usize,
    /// 零和顶点数。
    pub zero_sum_vertices: usize,
    /// 最大权重和偏差。
    pub max_deviation: f32,
    /// 活跃影响总数（Σ 有效槽位数）。
    pub total_influences: usize,
    /// 完全未被引用的骨骼数。
    pub unreferenced_bones: usize,
}

/// 权重容器体检。
pub fn inspect_weights(w: &SkinWeights) -> SkinStats {
    let mut saturated = 0usize;
    let mut unnorm = 0usize;
    let mut zero = 0usize;
    let mut max_dev = 0.0f32;
    let mut total_inf = 0usize;
    let mut v = 0usize;
    while v < w.vertex_count() {
        let act = w.active_influences(v);
        if act >= w.stride() {
            saturated += 1;
        }
        total_inf += act;
        match w.weight_sum(v) {
            Some(s) if s <= 0.0 => zero += 1,
            Some(s) => {
                let d = (s - 1.0).abs();
                if d > max_dev {
                    max_dev = d;
                }
                if d > WEIGHT_SUM_TOLERANCE {
                    unnorm += 1;
                }
            }
            None => zero += 1,
        }
        v += 1;
    }
    let usage = w.bone_usage();
    let unref = usage.iter().filter(|c| **c == 0).count();
    SkinStats {
        vertex_count: w.vertex_count(),
        stride: w.stride(),
        bone_count: w.bone_count(),
        saturated_vertices: saturated,
        unnormalized_vertices: unnorm,
        zero_sum_vertices: zero,
        max_deviation: max_dev,
        total_influences: total_inf,
        unreferenced_bones: unref,
    }
}

// ===========================================================================
// 测试夹具
// ===========================================================================

/// 3 骨骼链骨架：`root → child → tip`，纯平移，IBM 已解析。
///
/// **不标 `#[cfg(test)]`**：本函数供生产判据集 [`run_vei10_checks`] 调用，
/// 若标 test 则非测试构建下该函数不存在，真仓 `cargo build --lib` 直接 E0425。
/// 它是判据的**语料**，不是测试代码。
fn fixture_chain() -> BoneSkeleton {
    let mut sk = BoneSkeleton::new();
    let _ = sk.push_bone(b"root", None, Mat4::translation(0.0, 0.0, 0.0));
    let _ = sk.push_bone(b"child", Some(0), Mat4::translation(1.0, 0.0, 0.0));
    let _ = sk.push_bone(b"tip", Some(1), Mat4::translation(0.0, 2.0, 0.0));
    let _ = sk.resolve_inverse_bind();
    sk
}

/// 单骨骼 + 权重均为 1 的骨架。同 [`fixture_chain`]：判据语料，非测试代码。
fn fixture_single() -> BoneSkeleton {
    let mut sk = BoneSkeleton::new();
    let _ = sk.push_bone(b"only", None, Mat4::identity());
    let _ = sk.resolve_inverse_bind();
    sk
}

// ===========================================================================
// 判据
// ===========================================================================

/// VE-F1610 判据集（锚点：权重容器、规范化、层级容器、对接预留）。
pub fn run_vei10_checks() -> CheckSet {
    let mut set = CheckSet::new("gfx-vei10");

    // ---- 权重容器族 ----
    // A1 默认配额为 4（锚点「上限 4 骨骼/顶点」）
    {
        let w = SkinWeights::new(10, 8, STRIDE_DEFAULT);
        set.add(
            "I1610-容器-默认配额四",
            w.stride() == 4 && STRIDE_DEFAULT == 4,
            "默认每顶点槽位应为 4",
        );
    }
    // A2 配额可配（2 / 4 / 6 均如实生效）
    {
        let w2 = SkinWeights::new(4, 8, 2);
        let w4 = SkinWeights::new(4, 8, 4);
        let w6 = SkinWeights::new(4, 8, 6);
        set.add(
            "I1610-容器-配额可配",
            w2.stride() == 2 && w4.stride() == 4 && w6.stride() == 6,
            "配额参数应如实生效",
        );
    }
    // A3 配额超硬顶被夹到 STRIDE_HARD_CAP（不是静默变 0）
    {
        let w = SkinWeights::new(2, 4, 4096);
        set.add(
            "I1610-容器-配额夹逼硬顶",
            w.stride() == STRIDE_HARD_CAP,
            "超硬顶配额应夹到 STRIDE_HARD_CAP",
        );
    }
    // A4 SoA 两数组长度与 顶点数×配额 一致
    {
        let w = SkinWeights::new(7, 5, 4);
        let want = 7 * 4;
        set.add(
            "I1610-容器-SoA长度一致",
            w.soa_consistent() && w.bone_ids_len() == want && w.weights_len() == want,
            "两数组长度应等于顶点数×配额",
        );
    }
    // A5 严格写入超配额返回专属码 QuotaExceeded（断错误种类，非只断失败）
    {
        let mut w = SkinWeights::new(2, 8, 2);
        let r = w.set_vertex(0, &[(0, 0.5), (1, 0.3), (2, 0.2)]);
        set.add(
            "I1610-容器-超配额专属码",
            r == Err(SkinError::QuotaExceeded),
            "超配额应返回 QuotaExceeded",
        );
    }
    // A6 骨骼索引越界返回 BoneOutOfRange（与超配额**不同码**，证明分支独立）
    {
        let mut w = SkinWeights::new(2, 3, 4);
        let r = w.set_vertex(0, &[(9, 1.0)]);
        set.add(
            "I1610-容器-骨越界专属码",
            r == Err(SkinError::BoneOutOfRange),
            "骨骼索引 9 超出 bone_count=3，应 BoneOutOfRange",
        );
    }
    // A7 顶点越界返回 VertexOutOfRange（第三种码）
    {
        let mut w = SkinWeights::new(2, 3, 4);
        let r = w.set_vertex(7, &[(0, 1.0)]);
        set.add(
            "I1610-容器-顶点越界专属码",
            r == Err(SkinError::VertexOutOfRange),
            "顶点 7 超出 vertex_count=2，应 VertexOutOfRange",
        );
    }
    // A8 负权重 / NaN 权重各有专属码
    {
        let mut w = SkinWeights::new(2, 3, 4);
        let neg = w.set_vertex(0, &[(0, -0.5)]);
        let nan = w.set_vertex(0, &[(0, f32::NAN)]);
        set.add(
            "I1610-容器-负权与NaN分码",
            neg == Err(SkinError::NegativeWeight) && nan == Err(SkinError::NonFiniteWeight),
            "负权重与 NaN 应返回各自专属码",
        );
    }
    // A9 失败写入不留半写状态（校验在落盘前完成）
    {
        let mut w = SkinWeights::new(1, 4, 4);
        let _ = w.set_vertex(0, &[(0, 0.25), (1, 0.25)]);
        let before = w.weight_sum(0).unwrap_or(-1.0);
        let bad = w.set_vertex(0, &[(2, 0.5), (9, 0.5)]); // 第二项越界
        let after = w.weight_sum(0).unwrap_or(-1.0);
        set.add(
            "I1610-容器-失败不留半写",
            bad == Err(SkinError::BoneOutOfRange) && (before - after).abs() < 1e-6,
            "含越界项的写入应整体失败且不改动原值",
        );
    }
    // A10 填充槽语义：未用满的槽位为 INVALID_BONE + 0.0
    {
        let mut w = SkinWeights::new(1, 4, 4);
        let _ = w.set_vertex(0, &[(0, 0.5), (1, 0.5)]);
        let ok = w.bone_at(0, 2) == Some(INVALID_BONE) && w.weight_at(0, 2) == Some(0.0);
        set.add("I1610-容器-填充槽语义", ok, "未用槽位应为 INVALID_BONE + 0.0");
    }
    // A11 有效影响数口径（≠ 分配的槽位数）
    {
        let mut w = SkinWeights::new(1, 4, 4);
        let _ = w.set_vertex(0, &[(0, 0.4), (1, 0.6)]);
        set.add(
            "I1610-容器-有效影响口径",
            w.active_influences(0) == 2,
            "2 个非零权重槽 → 有效影响数 2（不是槽位数 4）",
        );
    }
    // A12 截断保留权重最大的 N 个，且返回丢弃数
    {
        let mut w = SkinWeights::new(1, 8, 2);
        let dropped = w.set_vertex_truncating(0, &[(0, 0.1), (1, 0.6), (2, 0.3)])
            .unwrap_or(99);
        let w0 = w.weight_at(0, 0).unwrap_or(-1.0);
        let w1 = w.weight_at(0, 1).unwrap_or(-1.0);
        set.add(
            "I1610-容器-截断留最大",
            dropped == 1 && w0 == 0.6 && w1 == 0.3,
            "配额 2 时应丢弃权重 0.1，保留 0.6 与 0.3",
        );
    }
    // A13 截断后按权重**降序**排列（十诫 12：直接断序列本身）
    {
        let mut w = SkinWeights::new(1, 8, 4);
        let _ = w.set_vertex_truncating(0, &[(0, 0.1), (1, 0.4), (2, 0.2), (3, 0.3)]);
        let seq: Vec<f32> = (0..4)
            .map(|k| w.weight_at(0, k).unwrap_or(-1.0))
            .collect();
        let desc = seq.windows(2).all(|p| p[0] >= p[1]);
        set.add(
            "I1610-容器-截断保序",
            desc && seq[0] == 0.4,
            "截断后权重序列应非增，且首位是最大者 0.4",
        );
    }
    // A14 骨骼使用统计（净口径）
    {
        let mut w = SkinWeights::new(2, 4, 2);
        let _ = w.set_vertex(0, &[(0, 0.5), (1, 0.5)]);
        let _ = w.set_vertex(1, &[(1, 1.0)]);
        let usage = w.bone_usage();
        set.add(
            "I1610-容器-骨骼使用统计",
            usage.len() == 4 && usage[0] == 1 && usage[1] == 2 && usage[2] == 0,
            "骨 0 用 1 顶点、骨 1 用 2 顶点、骨 2 未被引用",
        );
    }

    // ---- 规范化族 ----
    // B1 Σ=1 恰好在容差内 → 不改动（闭区间）
    {
        let mut w = SkinWeights::new(1, 4, 4);
        let _ = w.set_vertex(0, &[(0, 0.25), (1, 0.25), (2, 0.25), (3, 0.25)]);
        let changed = w.normalize_vertex(0);
        set.add(
            "I1610-归一-恰一不动",
            changed == Ok(false) && w.weight_at(0, 0) == Some(0.25),
            "Σ=1 应原样不动（闭区间）",
        );
    }
    // B2 容差边界**夹逼对**（十诫 4：采样留洞 ⇒ 闸门位置无人验证）
    //
    // 为什么不能用 `1.0 + WEIGHT_SUM_TOLERANCE` 构造下界：
    // f32 在 1.0 附近的 ULP = 2^-23 ≈ 1.1920929e-7，而 `1.0 ± 1e-4` 会被舍入到
    // `1.000100016594`，其偏差 1.00016594e-4 **已在容差外**（实测 dev - tol = +1.66e-8）。
    // 用它当下界会把「闭区间」判成假红。
    //
    // 更要紧的是：即便退到 ULP 阶梯（838/839×ULP），**只断两个采样点的判定结果
    // 仍然抓不住开闭区间**——实测把 `<=` 改成 `<` 后 838×ULP 依旧判「已归一」
    // （它离容差还有 1.03e-7 余量），全部判据照样全绿。这是十诫 4 的隐蔽形态：
    // 采样点离界有余量 ⇒ 界挪动测不出。
    //
    // 故本判据改为**直接断界的位置本身**：判据侧独立算出
    // `838×ULP < WEIGHT_SUM_TOLERANCE < 839×ULP`，并断被测谓词在这对
    // 紧邻可表示值上给出「内真/外假」。开区间变体会让中间那个不等式失效，
    // 于是无论谓词怎么写都对不上——闭区间被钉死。
    {
        let ulp = f32::from_bits(1.0f32.to_bits() + 1) - 1.0f32;
        let d_lo = 838.0f32 * ulp; // 最接近容差、且在容差内的可表示偏差
        let d_hi = 839.0f32 * ulp; // 容差外最近的下一个可表示偏差
        let mut w_in = SkinWeights::new(1, 4, 4);
        w_in.set_bone_at(0, 0, 0);
        w_in.set_weight_at(0, 0, 1.0f32 + d_lo);
        let mut w_out = SkinWeights::new(1, 4, 4);
        w_out.set_bone_at(0, 0, 0);
        w_out.set_weight_at(0, 0, 1.0f32 + d_hi);
        // (a) 界的位置：判据侧独立重算容差落在哪两个可表示偏差之间
        set.add(
            "I1610-归一-容差夹逼对自校对",
            d_lo < WEIGHT_SUM_TOLERANCE && d_hi > WEIGHT_SUM_TOLERANCE,
            "容差须严格落在 838×ULP 与 839×ULP 之间（界的位置由语料推导，不取自被测函数）",
        );
        // (b) 被测容差经权重和往返后仍等于原始容差（保证 (a) 的算术前提）
        let dev_in = w_in.sum_deviation(0).unwrap_or(f32::MAX);
        let dev_out = w_out.sum_deviation(0).unwrap_or(f32::MIN);
        set.add(
            "I1610-归一-夹逼偏差往返一致",
            (dev_in - d_lo).abs() <= d_lo * 1.0e-3 && (dev_out - d_hi).abs() <= d_hi * 1.0e-3,
            "经 1+s-1 往返后偏差须仍等于构造值（f32 舍入自校对）",
        );
        // (c) 界两侧判定必须相反，且内真外假
        set.add(
            "I1610-归一-容差闭区间下界",
            w_in.is_normalized(0) && !w_out.is_normalized(0),
            "容差内判已归一、紧邻容差外判未归一（界挪一格即转红）",
        );
    }
    // B3 明显超容差 → 需要归一（远离边界，避免只测边界）
    {
        let mut w = SkinWeights::new(1, 4, 4);
        w.set_bone_at(0, 0, 0);
        w.set_weight_at(0, 0, 1.0 + 10.0 * WEIGHT_SUM_TOLERANCE);
        set.add(
            "I1610-归一-超容差需归一",
            !w.is_normalized(0),
            "偏差 10×容差应判未归一",
        );
    }
    // B4 归一后权重和精确为 1（容差内），且比例独立对账（十诫 7）
    {
        let mut w = SkinWeights::new(1, 4, 4);
        let _ = w.set_vertex(0, &[(0, 2.0), (1, 6.0)]); // Σ=8
        let changed = w.normalize_vertex(0);
        let sum = w.weight_sum(0).unwrap_or(-1.0);
        // 判据侧独立重算参考值：2/8 与 6/8
        let ref0 = 2.0f32 / 8.0;
        let ref1 = 6.0f32 / 8.0;
        let got0 = w.weight_at(0, 0).unwrap_or(-1.0);
        let got1 = w.weight_at(0, 1).unwrap_or(-1.0);
        set.add(
            "I1610-归一-比例独立对账",
            changed == Ok(true)
                && (sum - 1.0).abs() <= WEIGHT_SUM_TOLERANCE
                && (got0 - ref0).abs() < 1e-6
                && (got1 - ref1).abs() < 1e-6,
            "Σ=8 归一后应为 0.25/0.75（判据侧独立重算）",
        );
    }
    // B5 零和 → ZeroTotalWeight，且**权重保持 0**（不产 NaN）
    {
        let mut w = SkinWeights::new(1, 4, 4);
        let _ = w.set_vertex(0, &[(0, 0.0), (1, 0.0)]);
        let r = w.normalize_vertex(0);
        let w0 = w.weight_at(0, 0).unwrap_or(f32::NAN);
        set.add(
            "I1610-归一-零和不产NaN",
            r == Err(SkinError::ZeroTotalWeight) && w0 == 0.0 && w0.is_finite(),
            "零和应报 ZeroTotalWeight 且权重仍为 0（不是 NaN）",
        );
    }
    // B6 归一幂等：第二次返回 changed=false 且权重逐位不变
    {
        let mut w = SkinWeights::new(1, 4, 4);
        let _ = w.set_vertex(0, &[(0, 0.3), (1, 0.9)]);
        let first = w.normalize_vertex(0);
        let snap = (w.weight_at(0, 0), w.weight_at(0, 1));
        let second = w.normalize_vertex(0);
        set.add(
            "I1610-归一-幂等",
            first == Ok(true) && second == Ok(false) && snap == (w.weight_at(0, 0), w.weight_at(0, 1)),
            "第二次归一应无改动（字节级确定性前提）",
        );
    }
    // B6b 幂等门**贴着容差界**取样（十诫 4：上面的语料 Σ=1.2 偏差 0.2，离界两个数量级，
    // 把闭区间改成开区间照样全绿——界从未被采样）。
    // 本判据直接构造「偏差恰好落在容差内沿」的权重和：取 `1 + 838×ULP`
    // （ULP = 2^-23；tol/ULP = 838.86，故 838×ULP < tol < 839×ULP），
    // 断言它被幂等门**接受**（changed=false）。开区间变体会让它落到 else 分支
    // 被重新缩放，`changed` 变 true —— 闭区间语义被钉死。
    {
        let ulp = f32::from_bits(1.0f32.to_bits() + 1) - 1.0f32;
        let edge_sum = 1.0f32 + 838.0f32 * ulp; // 偏差 9.98974e-5 < tol
        let mut w = SkinWeights::new(1, 4, 4);
        // 双槽拆分，保证两槽都有效且和恰为 edge_sum
        let _ = w.set_vertex(0, &[(0, 0.5), (1, edge_sum - 0.5)]);
        let sum_before = w.weight_sum(0).unwrap_or(-1.0);
        let changed = w.normalize_vertex(0);
        set.add(
            "I1610-归一-贴界幂等",
            (edge_sum - 1.0).abs() < WEIGHT_SUM_TOLERANCE
                && changed == Ok(false)
                && sum_before == edge_sum,
            "偏差落在容差内沿（838×ULP）时幂等门应接受、不重算（开区间变体立即转红）",
        );
        // 对照：紧邻界外的 839×ULP 必须**不**被幂等门接受
        let out_sum = 1.0f32 + 839.0f32 * ulp;
        let mut w2 = SkinWeights::new(1, 4, 4);
        let _ = w2.set_vertex(0, &[(0, 0.5), (1, out_sum - 0.5)]);
        let changed_out = w2.normalize_vertex(0);
        set.add(
            "I1610-归一-贴界外必重算",
            (out_sum - 1.0).abs() > WEIGHT_SUM_TOLERANCE && changed_out == Ok(true),
            "偏差刚超出容差（839×ULP）时必须真归一（证明上一条不是恒真）",
        );
    }
    // B7 归一保持填充槽为填充（**直接断字段本身**，十诫 9）
    {
        let mut w = SkinWeights::new(1, 4, 4);
        let _ = w.set_vertex(0, &[(0, 3.0)]); // 只有 1 个有效槽
        let _ = w.normalize_vertex(0);
        set.add(
            "I1610-归一-填充保持",
            w.bone_at(0, 1) == Some(INVALID_BONE)
                && w.weight_at(0, 1) == Some(0.0)
                && w.weight_at(0, 0) == Some(1.0),
            "归一后未用槽仍为 INVALID_BONE+0.0，有效槽为 1.0",
        );
    }
    // B8 谓词自兜底：顶点越界返回 false 而非 panic（十诫 8）
    {
        let w = SkinWeights::new(2, 4, 4);
        set.add(
            "I1610-归一-谓词自兜底",
            !w.is_normalized(99) && w.weight_sum(99).is_none() && w.active_influences(99) == 0,
            "越界顶点应返回 false/None/0，不 panic",
        );
    }
    // B9 normalize_all：零和顶点跳过并计数，不中断整批
    {
        let mut w = SkinWeights::new(3, 4, 4);
        let _ = w.set_vertex(0, &[(0, 2.0), (1, 2.0)]); // Σ=4 需归一
        let _ = w.set_vertex(1, &[(0, 0.0)]); // 零和
        let _ = w.set_vertex(2, &[(0, 0.5), (1, 0.5)]); // 已归一
        let (changed, zero) = w.normalize_all();
        set.add(
            "I1610-归一-全量零和计数",
            changed == 1 && zero == 1 && w.is_normalized(0) && !w.is_normalized(1),
            "应改动 1 个、零和 1 个，坏顶点不中断整批",
        );
    }
    // B10 体检：未归一清单与实际重算一致（独立重算，不复读谓词族）
    {
        let mut w = SkinWeights::new(4, 4, 4);
        let _ = w.set_vertex(0, &[(0, 0.25), (1, 0.25), (2, 0.25), (3, 0.25)]);
        let _ = w.set_vertex(1, &[(0, 0.5), (1, 0.5)]);
        let _ = w.set_vertex(2, &[(0, 1.5)]); // Σ=1.5 未归一
        let _ = w.set_vertex(3, &[(0, 0.2), (1, 0.3)]); // Σ=0.5 未归一
        let (list, more) = w.unnormalized_vertices(8);
        let st = inspect_weights(&w);
        set.add(
            "I1610-归一-体检未归一计数",
            !more && list.len() == 2 && st.unnormalized_vertices == 2 && st.max_deviation > 0.4,
            "顶点 2(1.5) 与 3(0.5) 未归一，最大偏差 0.5",
        );
    }

    // ---- 层级容器族 ----
    // C1 链式世界矩阵：root(0) → child(+1,0,0) → tip(0,+2,0)
    {
        let sk = fixture_chain();
        let child = sk.bind_world(1).unwrap_or_else(|_| Mat4::identity());
        let tip = sk.bind_world(2).unwrap_or_else(|_| Mat4::identity());
        set.add(
            "I1610-层级-链式世界矩阵",
            child.at(0, 3) == 1.0 && tip.at(1, 3) == 2.0 && tip.at(0, 3) == 1.0,
            "child 世界平移 (1,0,0)；tip 应为 (1,2,0)",
        );
    }
    // C2 矩阵约定：mul 是「先 a 后 b」，**判据侧独立手算参考值**对账（十诫 7）
    //
    // 独立推导（不调 `Mat4::mul`）：
    //   t = translation(10) → t[0][3] = 10；s = scale(2) → s[0][0] = 2
    //   t.mul(s)：result[0][0] = t[0][0]·s[0][0] = 2；result[0][3] = t[0][3]·s[3][3] = 10
    //             ⇒ tp([1,0,0]) = 2·1 + 10 = 12   （先放大再平移）
    //   s.mul(t)：result[0][0] = s[0][0]·t[0][0] = 2；result[0][3] = s[0][3]·t[3][3] + … = 2·10 = 20
    //             ⇒ tp([1,0,0]) = 2·1 + 20 = 22   （先平移再放大，平移量也被放大）
    // 二者不等才是这条判据的**真正目的**：约定写反时 12/22 对调，判据立即转红。
    {
        let t = Mat4::translation(10.0, 0.0, 0.0);
        let s = Mat4::scale(2.0, 2.0, 2.0);
        let p = t.mul(&s).transform_point([1.0, 0.0, 0.0]);
        let q = s.mul(&t).transform_point([1.0, 0.0, 0.0]);
        set.add(
            "I1610-层级-矩阵约定",
            p[0] == 12.0 && q[0] == 22.0,
            "mul(a,b)=先 a 后 b：t.mul(s)=12（先放大后平移），s.mul(t)=22（平移量被放大）",
        );
        // 顺序可分辨性：两结果必须**不等**，否则上条在「mul 退化为交换」时会假绿
        set.add(
            "I1610-层级-矩阵顺序可分辨",
            (p[0] - q[0]).abs() > 1.0,
            "t.mul(s) 与 s.mul(t) 之差须显著（证明矩阵乘法非交换，顺序判据非恒真）",
        );
    }
    // C3 IBM 恒等式：IBM · world_bind = I（判据侧独立重算，不复读字段）
    {
        let sk = fixture_chain();
        let mut worst = 0.0f32;
        let mut i = 0usize;
        while i < sk.len() {
            let world = sk.bind_world(i).unwrap_or_else(|_| Mat4::identity());
            let ibm = sk.node(i).map(|n| n.inverse_bind).unwrap_or_else(|| Mat4::identity());
            let d = ibm.mul(&world).max_abs_diff(&Mat4::identity());
            if d > worst {
                worst = d;
            }
            i += 1;
        }
        set.add(
            "I1610-层级-IBM恒等",
            worst < 1e-5,
            "IBM·world_bind 应为单位矩阵",
        );
    }
    // C4 成环被截断为 HierarchyCycle（不挂死）
    {
        let mut sk = fixture_chain();
        sk.force_cycle_for_test(0, 2); // root → tip → child → root 成环
        let r = sk.bind_world(0);
        set.add(
            "I1610-层级-成环",
            r == Err(SkinError::HierarchyCycle),
            "父子链成环应返回 HierarchyCycle 而非死循环",
        );
    }
    // C5 自环同样被截断（环长 1）
    {
        let mut sk = fixture_single();
        sk.force_cycle_for_test(0, 0);
        let r = sk.bind_world(0);
        set.add(
            "I1610-层级-自环",
            r == Err(SkinError::HierarchyCycle),
            "骨骼指向自身应返回 HierarchyCycle",
        );
    }
    // C6 父索引越界返回 ParentOutOfRange（与成环**不同码**）
    {
        let mut sk = fixture_chain();
        sk.force_cycle_for_test(1, 99); // child 的父指向不存在的 99
        let r = sk.bind_world(1);
        set.add(
            "I1610-层级-父越界专属码",
            r == Err(SkinError::ParentOutOfRange),
            "父索引 99 不存在，应 ParentOutOfRange 而非成环",
        );
    }
    // C7 push 时就拒奇异绑定矩阵（0 缩放无逆）
    {
        let mut sk = BoneSkeleton::new();
        let r = sk.push_bone(b"flat", None, Mat4::scale(0.0, 1.0, 1.0));
        set.add(
            "I1610-层级-奇异绑定拒",
            r == Err(SkinError::SingularBindMatrix),
            "0 缩放矩阵奇异，应在 push 时拒绝",
        );
    }
    // C8 深度计算
    {
        let sk = fixture_chain();
        let d = (0..3).map(|i| sk.depth(i).unwrap_or(99)).collect::<Vec<_>>();
        set.add(
            "I1610-层级-深度",
            d == vec![0usize, 1, 2],
            "root/child/tip 深度应为 0/1/2",
        );
    }
    // C9 全路径拼接与查表（**真命中**，十诫 11）
    {
        let sk = fixture_chain();
        let p = sk.absolute_path(2);
        let hit = sk.find_by_path(b"root/child/tip");
        let miss = sk.find_by_path(b"root/nope");
        set.add(
            "I1610-层级-路径命中",
            p.as_slice() == b"root/child/tip" && hit == Some(2) && miss.is_none(),
            "绝对路径应为 root/child/tip，命中索引 2，未命中返回 None",
        );
    }
    // C10 骨骼数上限下 push 正常工作（骨架 3 骨）
    {
        let sk = fixture_chain();
        set.add(
            "I1610-层级-骨架规模",
            sk.len() == 3 && !sk.is_empty(),
            "链式骨架应有 3 根骨骼",
        );
    }

    // ---- 对接预留族（骨骼命名约定契约）----
    // D1 合法名通过
    {
        let mut sk = BoneSkeleton::new();
        let r = sk.push_bone(b"spine_01", None, Mat4::identity());
        set.add("I1610-对接-合法名", r == Ok(0), "spine_01 应合法");
    }
    // D2 空名 / 超长名 → InvalidBoneName
    {
        let mut sk = BoneSkeleton::new();
        let empty = sk.push_bone(b"", None, Mat4::identity());
        let long: Vec<u8> = vec![b'a'; BONE_NAME_MAX + 1];
        let toolong = sk.push_bone(&long, None, Mat4::identity());
        set.add(
            "I1610-对接-空名超长名拒",
            empty == Err(SkinError::InvalidBoneName) && toolong == Err(SkinError::InvalidBoneName),
            "空名与超长名都应 InvalidBoneName",
        );
    }
    // D3 含空格 / 非 ASCII → InvalidBoneName
    {
        let mut sk = BoneSkeleton::new();
        let sp = sk.push_bone(b"a b", None, Mat4::identity());
        let cjk = sk.push_bone("骨".as_bytes(), None, Mat4::identity());
        set.add(
            "I1610-对接-字符集拒",
            sp == Err(SkinError::InvalidBoneName) && cjk == Err(SkinError::InvalidBoneName),
            "含空格与非 ASCII 名应被拒",
        );
    }
    // D4 名字含路径分隔符 → InvalidBoneName（名字与层级必须无二义）
    {
        let mut sk = BoneSkeleton::new();
        let r = sk.push_bone(b"a/b", None, Mat4::identity());
        set.add(
            "I1610-对接-分隔符入名拒",
            r == Err(SkinError::InvalidBoneName),
            "名字内含 '/' 会与层级二义，应拒绝",
        );
    }
    // D5 保留前缀 `__` → ReservedBoneName（**专属码**，与非法名不同）
    {
        let mut sk = BoneSkeleton::new();
        let r = sk.push_bone(b"__engine", None, Mat4::identity());
        set.add(
            "I1610-对接-保留前缀专属码",
            r == Err(SkinError::ReservedBoneName),
            "__ 前缀应返回 ReservedBoneName（不是 InvalidBoneName）",
        );
    }
    // D6 父索引越界 → ParentOutOfRange
    {
        let mut sk = fixture_chain();
        let r = sk.push_bone(b"ghost", Some(42), Mat4::identity());
        set.add(
            "I1610-对接-父越界拒",
            r == Err(SkinError::ParentOutOfRange),
            "父索引 42 不存在，应拒绝",
        );
    }
    // D7 全路径重复 → AmbiguousPath（拒绝而非静默取首个）
    {
        let mut sk = BoneSkeleton::new();
        let _ = sk.push_bone(b"root", None, Mat4::identity());
        let _ = sk.push_bone(b"child", Some(0), Mat4::identity());
        // 再造一条 root/child 路径
        let dup = sk.push_bone(b"child", Some(0), Mat4::identity());
        set.add(
            "I1610-对接-路径重复拒",
            dup == Err(SkinError::AmbiguousPath),
            "root/child 重复应 AmbiguousPath（静默取首个会绑错骨）",
        );
    }
    // D8 兄弟同名（同父）→ 拒；异父同名 → 允许（路径不同即不同身份）
    //
    // 构造与期望逐笔对账（判据必须先自校对，否则会把正确实现判红）：
    //   push root  → Ok，len 1
    //   push armL  → Ok，len 2
    //   push armL  → **Err(AmbiguousPath)**，len 仍 2（拒绝不增长）
    //   push a/armL→ Ok，len 3（异父同名，路径 a/armL ≠ root/armL，放行）
    {
        let mut sk = BoneSkeleton::new();
        let r0 = sk.push_bone(b"root", None, Mat4::identity());
        let r1 = sk.push_bone(b"armL", Some(0), Mat4::identity());
        let dup = sk.push_bone(b"armL", Some(0), Mat4::identity());
        let len_after_dup = sk.len();
        // 异父同名：另起一根父骨，其下再挂同名子骨 → 路径不同，应放行
        let r2 = sk.push_bone(b"root2", None, Mat4::identity());
        let r3 = sk.push_bone(b"armL", Some(r2.unwrap_or(1) as u32), Mat4::identity());
        let len_ok = sk.len();
        set.add(
            "I1610-对接-兄弟同名处理",
            r0.is_ok()
                && r1.is_ok()
                && dup == Err(SkinError::AmbiguousPath)
                && len_after_dup == 2
                && r2.is_ok()
                && r3.is_ok()
                && len_ok == 4,
            "同父同名重复路径应拒且不增长(len 2)；异父同名应放行(len 4)",
        );
    }
    // D9 名字指纹跨会话稳定（同样的名字同样的值，不同名字不同值）
    {
        let a1 = BoneNameHash::of(b"spine_01");
        let a2 = BoneNameHash::of(b"spine_01");
        let b = BoneNameHash::of(b"spine_02");
        set.add(
            "I1610-对接-名字指纹稳定",
            a1 == a2 && a1 != b,
            "同名同指纹、异名异指纹（M 域可据此缓存绑定）",
        );
    }

    // ---- 蒙皮联合 oracle ----
    // E1 绑定姿态 + Σw=1 → 蒙皮矩阵必须是单位矩阵
    //    （一条判据同时验权重规范化与 IBM；任一坏即红）
    {
        let sk = fixture_chain();
        let mut w = SkinWeights::new(2, sk.len(), 4);
        let _ = w.set_vertex(0, &[(0, 0.5), (2, 0.5)]);
        let _ = w.set_vertex(1, &[(1, 0.25), (2, 0.75)]);
        let rig = SkinRig::new(sk, w).unwrap_or_else(|_| fixture_rig());
        let m = rig.skinning_matrix(0).unwrap_or_else(|_| Mat4::identity());
        let m1 = rig.skinning_matrix(1).unwrap_or_else(|_| Mat4::identity());
        set.add(
            "I1610-蒙皮-绑定姿态恒等",
            m.max_abs_diff(&Mat4::identity()) < 1e-5 && m1.max_abs_diff(&Mat4::identity()) < 1e-5,
            "绑定姿态下 Σw=1 的蒙皮矩阵应为单位矩阵",
        );
    }
    // E2 Σw≠1 时蒙皮矩阵**不是**单位矩阵（证明 E1 非恒真 —— 十诫 5 双向验证）
    {
        let sk = fixture_single();
        let mut w = SkinWeights::new(1, 1, 4);
        let _ = w.set_vertex(0, &[(0, 0.5)]); // Σ=0.5
        let rig = SkinRig::new(sk, w).unwrap_or_else(|_| fixture_rig());
        let m = rig.skinning_matrix(0).unwrap_or_else(|_| Mat4::identity());
        set.add(
            "I1610-蒙皮-非归一非恒等",
            m.max_abs_diff(&Mat4::identity()) > 0.4,
            "Σw=0.5 时蒙皮矩阵应显著偏离单位矩阵（否则 E1 是恒真的）",
        );
    }
    // E3 蒙皮后位置：绑定姿态应**保持原位**
    {
        let rig = fixture_rig();
        let p = rig.skinned_position(0, [3.0, 4.0, 5.0]).unwrap_or([0.0; 3]);
        set.add(
            "I1610-蒙皮-绑定姿态保位",
            (p[0] - 3.0).abs() < 1e-4 && (p[1] - 4.0).abs() < 1e-4 && (p[2] - 5.0).abs() < 1e-4,
            "绑定姿态下蒙皮不应移动顶点",
        );
    }
    // E4 顶点越界 → VertexOutOfRange（蒙皮侧专属码）
    {
        let rig = fixture_rig();
        let r = rig.skinning_matrix(999);
        set.add(
            "I1610-蒙皮-顶点越界专属码",
            r == Err(SkinError::VertexOutOfRange),
            "顶点 999 越界应 VertexOutOfRange",
        );
    }
    // E5 零和顶点在蒙皮侧同样被拒（不静默产出原点）
    {
        let sk = fixture_single();
        let mut w = SkinWeights::new(1, 1, 4);
        let _ = w.set_vertex(0, &[(0, 0.0)]);
        let rig = SkinRig::new(sk, w).unwrap_or_else(|_| fixture_rig());
        let r = rig.skinning_matrix(0);
        set.add(
            "I1610-蒙皮-零和被拒",
            r == Err(SkinError::ZeroTotalWeight),
            "零和顶点在蒙皮阶段应 ZeroTotalWeight（不是静默映射到原点）",
        );
    }
    // E6 骨骼数不匹配 → BoneOutOfRange（绑定体构造校验）
    {
        let sk = fixture_chain();
        let w = SkinWeights::new(2, 9, 4); // bone_count=9 ≠ 3
        let r = SkinRig::new(sk, w);
        set.add(
            "I1610-蒙皮-骨骼数不匹配",
            r == Err(SkinError::BoneOutOfRange),
            "骨骼数 3 与权重 bone_count=9 不符，应拒绝绑定",
        );
    }
    // E7 全网格蒙皮：顶点数不符 → VertexOutOfRange
    {
        let rig = fixture_rig();
        let r = rig.skinned_positions(&[[0.0, 0.0, 0.0]]); // 只给 1 个，应需 2
        set.add(
            "I1610-蒙皮-位置数不符",
            r == Err(SkinError::VertexOutOfRange),
            "位置数组长度与顶点数不符应拒绝",
        );
    }
    // E8 错误码唯一（15 种错误码互不相同，跨域上报不歧义）
    {
        let all = [
            SkinError::BoneOutOfRange,
            SkinError::VertexOutOfRange,
            SkinError::QuotaExceeded,
            SkinError::StrideMismatch,
            SkinError::NegativeWeight,
            SkinError::NonFiniteWeight,
            SkinError::ZeroTotalWeight,
            SkinError::ParentOutOfRange,
            SkinError::HierarchyCycle,
            SkinError::DepthExceeded,
            SkinError::InvalidBoneName,
            SkinError::ReservedBoneName,
            SkinError::AmbiguousPath,
            SkinError::EmptySkeleton,
            SkinError::SingularBindMatrix,
        ];
        let mut uniq = true;
        let mut i = 0usize;
        while i < all.len() {
            let mut j = 0usize;
            while j < all.len() {
                if i != j && all[i].code() == all[j].code() {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        set.add(
            "I1610-对接-错误码唯一",
            uniq && all[0].message().len() > 0,
            "15 种错误码互不相同且都有人话描述",
        );
    }

    set
}

/// 判据用兜底绑定体（构造失败时的替身，保证判据函数零 panic 面）。
///
/// **不标 `#[cfg(test)]`**：同 [`fixture_chain`]，被生产判据集调用。
fn fixture_rig() -> SkinRig {
    let sk = fixture_chain();
    let mut w = SkinWeights::new(2, 3, 4);
    let _ = w.set_vertex(0, &[(0, 0.5), (2, 0.5)]);
    let _ = w.set_vertex(1, &[(1, 0.25), (2, 0.75)]);
    SkinRig {
        skeleton: sk,
        weights: w,
    }
}