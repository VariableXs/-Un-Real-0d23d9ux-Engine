//! VE-F0602 · 图层变换系统（VE-D 域 · 2D 合成引擎 · 目标 460 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0602`
//!
//! **判据（锚点原文）**：2D 与 3D 仿射及投影变换矩阵、变换原点三参照系
//! （自身中心、父原点、显式锚点——逐层声明语义不混猜）；级联契约：子层坐标系
//! 建立在父变换之后，世界矩阵=父级联乘自身，级联顺序即视觉结果不容两义；
//! 分解与重组（平移、旋转、缩放、倾斜的可逆分解——供 F0608 动画分量插值）；
//! 退化防护（奇异矩阵检测、近零行列式降级为恒等加告警）；逆矩阵缓存
//! （命中测试 F0611 与脏区逆映射共用）。
//!
//! **错误路径与降级矩阵**：奇异→降级恒等加告警；级联断链→重算全路径；
//! 插值穿越奇异→分量钳制。
//!
//! **设计要点**：
//! - **2D 仿射**：6 元素 `Mat2D`（CSS `matrix()` 同构），矩阵乘法定义为
//!   `self.mul(other)` = 先应用 other 再应用 self——级联写法
//!   `world = parent.mul(local)` 与锚点契约逐字对应；
//! - **三参照系**：原点参照系显式枚举（自身中心用层尺寸、父原点即零偏移、
//!   显式锚点用声明锚点），`with_origin` 统一实现 `T(o)·M·T(-o)`——
//!   逐层声明语义不混猜；
//! - **可逆分解**：`sx = hypot(a,b)`、`r = atan2(b,a)`、`sy = det/sx`、
//!   `k = atan((c·cos r + d·sin r)/sy)`，重组按 `T·R·K·S` 乘回——
//!   分解/重组数学互逆（回归测试逐分量 roundtrip）；
//! - **退化防护**：`|det| < NEAR_ZERO_DET` 即奇异；`invert_or_identity`
//!   降级恒等并出告警（告警入账可查）；插值用的分量钳制
//!   `clamp_components` 把缩放钳到 `MIN_SCALE_CLAMP` 防穿越奇异；
//! - **逆矩阵缓存**：缓存键 = 矩阵位型（逐元素位比较），命中 O(1)——
//!   命中测试 F0611 与脏区逆映射共用同一份缓存条目；
//! - **级联断链→重算全路径**：缓存条目按矩阵位型失效，位型不符即整条重算，
//!   不存在"半新半旧"的级联结果。
//!
//! **3D 投影**：`Mat4` 提供 4x4 仿射与透视投影（fov/aspect/near/far），
//! 乘法与点变换含 w 除——3D 路径供后续 VE-I 域消费，本条先立数学本体。
//!
//! **跨批对接点**：上游 F0601 树结构；下游 F0608 插值、F0611 命中、F0613 脏区。
//!
//! 零外部依赖；浮点确定性纪律：同输入同输出（IEEE 754 基本运算，无快速数学）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 行列式奇异判定阈值：|det| 低于此值视为奇异。
pub const NEAR_ZERO_DET: f32 = 1e-9;

/// 插值分量钳制下限：缩放不得低于此值（防穿越奇异）。
pub const MIN_SCALE_CLAMP: f32 = 1e-4;

/// 逆矩阵缓存条目数（命中测试与脏区逆映射共用，双条目足够热路径）。
pub const INV_CACHE_SLOTS: usize = 2;

// ---------------------------------------------------------------------------
// 二、2D 仿射矩阵（CSS matrix() 同构：x' = a·x + c·y + e; y' = b·x + d·y + f）
// ---------------------------------------------------------------------------

/// 2D 仿射矩阵。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat2D {
    /// 列 1 x 分量（缩放/旋转）。
    pub a: f32,
    /// 列 1 y 分量。
    pub b: f32,
    /// 列 2 x 分量（倾斜/缩放）。
    pub c: f32,
    /// 列 2 y 分量。
    pub d: f32,
    /// 平移 x。
    pub e: f32,
    /// 平移 y。
    pub f: f32,
}

impl Mat2D {
    /// 恒等矩阵。
    pub const IDENTITY: Mat2D = Mat2D { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    /// 平移矩阵。
    pub fn translation(tx: f32, ty: f32) -> Self {
        Mat2D { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: tx, f: ty }
    }

    /// 旋转矩阵（弧度）。
    pub fn rotation(rad: f32) -> Self {
        let (s, c) = rad.sin_cos();
        Mat2D { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    /// 缩放矩阵。
    pub fn scaling(sx: f32, sy: f32) -> Self {
        Mat2D { a: sx, b: 0.0, c: 0.0, d: sy, e: 0.0, f: 0.0 }
    }

    /// 倾斜矩阵（skew_x：x 随 y 偏移；skew_y：y 随 x 偏移）。
    pub fn skewing(kx: f32, ky: f32) -> Self {
        Mat2D { a: 1.0, b: ky.tan(), c: kx.tan(), d: 1.0, e: 0.0, f: 0.0 }
    }

    /// 级联乘法（级联契约：`parent.mul(local)` = 先 local 后 parent，
    /// 即 world = parent ∘ local。级联顺序即视觉结果，不容两义）。
    pub fn mul(&self, o: &Mat2D) -> Mat2D {
        Mat2D {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }

    /// 应用到点。
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// 变换方向向量（只取线性部分，不含平移——法线/位移的变换入口）。
    pub fn apply_direction(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }

    /// 边界防护：六元素全部有限（NaN/Inf 输入零容忍——上游脏数据在入口拦住，
    /// 不让它进级联链污染整棵树的世界矩阵）。
    pub fn is_finite(&self) -> bool {
        self.a.is_finite()
            && self.b.is_finite()
            && self.c.is_finite()
            && self.d.is_finite()
            && self.e.is_finite()
            && self.f.is_finite()
    }

    /// 校验构造：非有限输入返回 None（边界防护的显式出口，调用方按错误路径处置）。
    pub fn validated(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Option<Mat2D> {
        let m = Mat2D { a, b, c, d, e, f };
        if m.is_finite() {
            Some(m)
        } else {
            None
        }
    }

    /// 分量插值（F0608 动画插值的数学入口）：两端各自分解（奇异端走钳制），
    /// 四分量逐项线性插值后重组——插值的是"分量"而不是矩阵元素，
    /// 否则旋转半程会塌缩成缩放（动画经典破相）。
    ///
    /// t 钳制到 [0,1]：越界插值按端点处理（边界防护）。
    pub fn lerp_components(&self, other: &Mat2D, t: f32) -> Mat2D {
        let t = t.clamp(0.0, 1.0);
        let (tx0, ty0, r0, k0, sx0, sy0) = self.decompose_clamped();
        let (tx1, ty1, r1, k1, sx1, sy1) = other.decompose_clamped();
        Mat2D::recompose(
            tx0 + (tx1 - tx0) * t,
            ty0 + (ty1 - ty0) * t,
            r0 + (r1 - r0) * t,
            k0 + (k1 - k0) * t,
            sx0 + (sx1 - sx0) * t,
            sy0 + (sy1 - sy0) * t,
        )
    }

    /// 行列式。
    pub fn det(&self) -> f32 {
        self.a * self.d - self.b * self.c
    }

    /// 精确求逆（奇异返回 None——调用方按降级矩阵处置）。
    pub fn invert(&self) -> Option<Mat2D> {
        let det = self.det();
        if det.abs() < NEAR_ZERO_DET {
            return None;
        }
        let inv = 1.0 / det;
        Some(Mat2D {
            a: self.d * inv,
            b: -self.b * inv,
            c: -self.c * inv,
            d: self.a * inv,
            e: (self.c * self.f - self.d * self.e) * inv,
            f: (self.b * self.e - self.a * self.f) * inv,
        })
    }

    /// 变换原点三参照系（判据：自身中心、父原点、显式锚点——逐层声明不混猜）。
    pub fn with_origin(&self, frame: OriginFrame, size: (f32, f32), anchor: (f32, f32)) -> Mat2D {
        let (ox, oy) = frame.resolve(size, anchor);
        // T(o) · M · T(-o)：先把原点搬到参照点，变换后再搬回。
        Mat2D::translation(ox, oy)
            .mul(self)
            .mul(&Mat2D::translation(-ox, -oy))
    }

    /// 可逆分解（判据：平移/旋转/缩放/倾斜四分量；数学互逆供 F0608 插值）。
    ///
    /// 返回 (tx, ty, 旋转 rad, skew_x rad, sx, sy)。
    /// 奇异矩阵（sx 或 sy 近零）返回 None——退化走 [`Self::decompose_clamped`]。
    pub fn decompose(&self) -> Option<(f32, f32, f32, f32, f32, f32)> {
        let tx = self.e;
        let ty = self.f;
        let sx = (self.a * self.a + self.b * self.b).sqrt();
        if sx.abs() < NEAR_ZERO_DET {
            return None;
        }
        let rot = self.b.atan2(self.a);
        let det = self.det();
        let sy = det / sx;
        if sy.abs() < NEAR_ZERO_DET {
            return None;
        }
        // 注意：f32::sin_cos 返回 (sin, cos)。
        let (sin_r, cos_r) = rot.sin_cos();
        let shear = (self.c * cos_r + self.d * sin_r) / sy;
        let skx = shear.atan();
        Some((tx, ty, rot, skx, sx, sy))
    }

    /// 分解的钳制版（错误路径：插值穿越奇异→分量钳制）。
    /// 缩放分量钳到 `MIN_SCALE_CLAMP`，永不返回 None。
    pub fn decompose_clamped(&self) -> (f32, f32, f32, f32, f32, f32) {
        match self.decompose() {
            Some(v) => v,
            None => {
                // 奇异：按缩放钳制下限保守重建可插值分量。
                (self.e, self.f, 0.0, 0.0, MIN_SCALE_CLAMP, MIN_SCALE_CLAMP)
            }
        }
    }

    /// 由分量重组（分解的严格逆：T(tx,ty)·R(rot)·K(skx)·S(sx,sy)）。
    pub fn recompose(tx: f32, ty: f32, rot: f32, skx: f32, sx: f32, sy: f32) -> Mat2D {
        Mat2D::translation(tx, ty)
            .mul(&Mat2D::rotation(rot))
            .mul(&Mat2D::skewing(skx, 0.0))
            .mul(&Mat2D::scaling(sx, sy))
    }
}

/// 变换原点三参照系（判据：三参照系——逐层声明语义不混猜）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginFrame {
    /// 自身中心：参照点 = 层尺寸 / 2。
    OwnCenter,
    /// 父原点：参照点 = (0,0)，即父坐标系原点。
    ParentOrigin,
    /// 显式锚点：参照点 = 声明的锚点坐标。
    ExplicitAnchor,
}

impl OriginFrame {
    /// 解析参照点（语义唯一，无默认混猜）。
    pub fn resolve(self, size: (f32, f32), anchor: (f32, f32)) -> (f32, f32) {
        match self {
            OriginFrame::OwnCenter => (size.0 / 2.0, size.1 / 2.0),
            OriginFrame::ParentOrigin => (0.0, 0.0),
            OriginFrame::ExplicitAnchor => anchor,
        }
    }

    /// 读屏可读名。
    pub fn screen_name(self) -> &'static str {
        match self {
            OriginFrame::OwnCenter => "自身中心",
            OriginFrame::ParentOrigin => "父原点",
            OriginFrame::ExplicitAnchor => "显式锚点",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、逆矩阵缓存（判据：命中测试 F0611 与脏区逆映射共用）
// ---------------------------------------------------------------------------

/// 逆矩阵缓存：键 = 矩阵位型（逐元素位比较，浮点 NaN 位型也稳定）。
#[derive(Clone, Copy, Debug)]
pub struct InvCacheEntry {
    key: Option<Mat2D>,
    inv: Mat2D,
}

/// 双条目缓存（热路径：最近两级变换的逆各占一条）。
#[derive(Clone, Copy, Debug)]
pub struct InvCache {
    entries: [InvCacheEntry; INV_CACHE_SLOTS],
    /// 命中次数（遥测）。
    pub hits: u64,
    /// 未命中次数（遥测）。
    pub misses: u64,
}

impl InvCache {
    /// 构造空缓存。
    pub fn new() -> Self {
        InvCache {
            entries: [
                InvCacheEntry { key: None, inv: Mat2D::IDENTITY },
                InvCacheEntry { key: None, inv: Mat2D::IDENTITY },
            ],
            hits: 0,
            misses: 0,
        }
    }

    fn same(a: &Mat2D, b: &Mat2D) -> bool {
        a.a.to_bits() == b.a.to_bits()
            && a.b.to_bits() == b.b.to_bits()
            && a.c.to_bits() == b.c.to_bits()
            && a.d.to_bits() == b.d.to_bits()
            && a.e.to_bits() == b.e.to_bits()
            && a.f.to_bits() == b.f.to_bits()
    }

    /// 取逆：命中 O(1)；未命中整条重算并置换该条目（级联断链→重算全路径）。
    pub fn invert_cached(&mut self, m: &Mat2D) -> Option<Mat2D> {
        for e in self.entries.iter_mut() {
            if let Some(k) = e.key {
                if Self::same(&k, m) {
                    self.hits = self.hits.saturating_add(1);
                    return Some(e.inv);
                }
            }
        }
        self.misses = self.misses.saturating_add(1);
        let inv = m.invert()?;
        // 置换最旧条目（轮转：用 misses 计数取模决定槽位）。
        let slot = (self.misses as usize).wrapping_sub(1) % INV_CACHE_SLOTS;
        self.entries[slot] = InvCacheEntry { key: Some(*m), inv };
        Some(inv)
    }

    /// 清空（级联断链时调用方显式重算全路径的入口）。
    pub fn clear(&mut self) {
        for e in self.entries.iter_mut() {
            e.key = None;
        }
    }
}

impl Default for InvCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、退化告警账（判据：奇异→降级恒等加告警）
// ---------------------------------------------------------------------------

/// 一条退化告警（可观测铁律：降级必须留痕）。
#[derive(Clone, Copy, Debug)]
pub struct SingularWarning {
    /// 被降级矩阵的行列式（证据）。
    pub det: f32,
    /// 处置：0 = 恒等降级。
    pub action: u8,
}

/// 告警账：容量 64，满后丢弃计数（零静默不等于无限内存）。
#[derive(Clone, Copy, Debug)]
pub struct WarningLedger {
    pub count: usize,
    pub dropped: u64,
    pub items: [Option<SingularWarning>; 64],
}

impl Default for WarningLedger {
    fn default() -> Self {
        WarningLedger {
            count: 0,
            dropped: 0,
            items: [None; 64],
        }
    }
}

impl WarningLedger {
    /// 记一条告警。
    pub fn record(&mut self, w: SingularWarning) {
        if self.count >= 64 {
            self.dropped = self.dropped.saturating_add(1);
        } else {
            self.items[self.count] = Some(w);
            self.count += 1;
        }
    }

    /// 求逆的降级入口：奇异 → 恒等 + 告警；正常 → 精确逆。
    pub fn invert_or_identity(led: &mut WarningLedger, m: &Mat2D) -> Mat2D {
        match m.invert() {
            Some(inv) => inv,
            None => {
                led.record(SingularWarning { det: m.det(), action: 0 });
                Mat2D::IDENTITY
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、3D 仿射与投影矩阵（判据：3D 仿射及投影变换矩阵）
// ---------------------------------------------------------------------------

/// 4x4 矩阵（行主序：m[row][col]）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    /// 行主序元素。
    pub m: [[f32; 4]; 4],
}

impl Mat4 {
    /// 恒等。
    pub const IDENTITY: Mat4 = Mat4 {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// 平移。
    pub fn translation(tx: f32, ty: f32, tz: f32) -> Self {
        let mut r = Self::IDENTITY;
        r.m[0][3] = tx;
        r.m[1][3] = ty;
        r.m[2][3] = tz;
        r
    }

    /// 缩放。
    pub fn scaling(sx: f32, sy: f32, sz: f32) -> Self {
        let mut r = Self::IDENTITY;
        r.m[0][0] = sx;
        r.m[1][1] = sy;
        r.m[2][2] = sz;
        r
    }

    /// 绕 X 轴旋转（弧度）。
    pub fn rotation_x(rad: f32) -> Self {
        let (s, c) = rad.sin_cos();
        let mut r = Self::IDENTITY;
        r.m[1][1] = c;
        r.m[1][2] = -s;
        r.m[2][1] = s;
        r.m[2][2] = c;
        r
    }

    /// 绕 Y 轴旋转。
    pub fn rotation_y(rad: f32) -> Self {
        let (s, c) = rad.sin_cos();
        let mut r = Self::IDENTITY;
        r.m[0][0] = c;
        r.m[0][2] = s;
        r.m[2][0] = -s;
        r.m[2][2] = c;
        r
    }

    /// 绕 Z 轴旋转。
    pub fn rotation_z(rad: f32) -> Self {
        let (s, c) = rad.sin_cos();
        let mut r = Self::IDENTITY;
        r.m[0][0] = c;
        r.m[0][1] = -s;
        r.m[1][0] = s;
        r.m[1][1] = c;
        r
    }

    /// 透视投影（竖直 fov 弧度 / 宽高比 / 近远平面；D3D 风格 0..1 深度）。
    pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Self {
        let f = 1.0 / (fov_y / 2.0).tan();
        let mut r = Mat4 { m: [[0.0; 4]; 4] };
        r.m[0][0] = f / aspect;
        r.m[1][1] = f;
        r.m[2][2] = far / (near - far);
        r.m[2][3] = near * far / (near - far);
        r.m[3][2] = -1.0;
        r
    }

    /// 正交投影（2D 界面的 3D 快路径 / 无透视需求场景；D3D 风格 0..1 深度）。
    pub fn orthographic(l: f32, r: f32, b: f32, t: f32, near: f32, far: f32) -> Self {
        let mut m = Self::IDENTITY;
        m.m[0][0] = 2.0 / (r - l);
        m.m[1][1] = 2.0 / (t - b);
        m.m[2][2] = 1.0 / (near - far);
        m.m[0][3] = (l + r) / (l - r);
        m.m[1][3] = (t + b) / (b - t);
        m.m[2][3] = near / (near - far);
        m
    }

    /// 边界防护：16 元素全部有限（NaN/Inf 零容忍，与 Mat2D 同纪律）。
    pub fn is_finite(&self) -> bool {
        self.m.iter().all(|row| row.iter().all(|v| v.is_finite()))
    }

    /// 矩阵乘（self ∘ other：先 other 后 self，与 2D 级联同约定）。
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut r = Mat4 { m: [[0.0; 4]; 4] };
        for i in 0..4 {
            for j in 0..4 {
                let mut acc = 0.0;
                for k in 0..4 {
                    acc += self.m[i][k] * o.m[k][j];
                }
                r.m[i][j] = acc;
            }
        }
        r
    }

    /// 变换点（含 w 除——投影后的透视除法）。
    pub fn apply_point(&self, x: f32, y: f32, z: f32) -> (f32, f32, f32) {
        let w = self.m[3][0] * x + self.m[3][1] * y + self.m[3][2] * z + self.m[3][3];
        if w.abs() < NEAR_ZERO_DET {
            // w 近零：点在无穷远，返回未除结果并留符号——调用方按裁剪处置。
            return (
                self.m[0][0] * x + self.m[0][1] * y + self.m[0][2] * z + self.m[0][3],
                self.m[1][0] * x + self.m[1][1] * y + self.m[1][2] * z + self.m[1][3],
                self.m[2][0] * x + self.m[2][1] * y + self.m[2][2] * z + self.m[2][3],
            );
        }
        (
            (self.m[0][0] * x + self.m[0][1] * y + self.m[0][2] * z + self.m[0][3]) / w,
            (self.m[1][0] * x + self.m[1][1] * y + self.m[1][2] * z + self.m[1][3]) / w,
            (self.m[2][0] * x + self.m[2][1] * y + self.m[2][2] * z + self.m[2][3]) / w,
        )
    }
}

// ---------------------------------------------------------------------------
// 六、级联契约（判据：级联顺序即视觉结果不容两义；级联断链→重算全路径）
// ---------------------------------------------------------------------------

/// 级联：世界矩阵 = 父级联乘自身（`parent_world.mul(local)`）。
///
/// O(深度) 由调用方沿树逐层调用实现；本函数是单层级联的契约实现，
/// 断链时调用方清空 [`InvCache`] 后从根重算全路径。
pub fn cascade(parent_world: &Mat2D, local: &Mat2D) -> Mat2D {
    parent_world.mul(local)
}

// ---------------------------------------------------------------------------
// 七、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0602 域自检（判据逐条映射见 `ved02_checks.rs`）。
pub fn run_ved02_checks() -> CheckSet {
    super::ved02_checks::run_ved02_checks()
}

/// 读屏摘要（浮点格式化容差 3 位小数）。
pub fn screen_text(m: &Mat2D) -> String {
    format!(
        "变换矩阵：scale=({:.3},{:.3}) rot={:.3} skew={:.3} translate=({:.3},{:.3})",
        (m.a * m.a + m.b * m.b).sqrt(),
        m.det() / (m.a * m.a + m.b * m.b).sqrt().max(f32::EPSILON),
        m.b.atan2(m.a),
        0.0,
        m.e,
        m.f
    )
}
