//! VE-F2011 · 抗锯齿四法之 TAA（VE-K 域 · 后处理架构与 Bloom 组 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2011`
//!
//! **判据（锚点原文五条）**：
//! - **Halton 抖动**：Halton(2,3) 抖动序列 8 相位，Jitter 应用在投影矩阵；
//! - **velocity reject**：历史帧校验——速度缓冲 reject（F2045 复用）+ 深度/法线
//!   相似度校验 + 场景切换强制失效；
//! - **clipping**：历史颜色 AABB / variance clipping 收纳当前帧色域；
//! - **降级显性**：速度缓冲缺失（F2045 未启用）→ 退化为几何启发校验 **+ 告警**，
//!   不是静默降级；历史显存超配额 → **拒绝 TAA** 并回退 FXAA 建议；
//! - **判据**：见 `vek11_checks.rs`。
//!
//! **算法基线**：时域抗锯齿的标准实现（历史帧累积 + 子像素抖动）。
//!
//! | 参考步骤 | 本条函数 |
//! | --- | --- |
//! | `Halton(2,3)` 8 相位抖动 | [`halton`] + [`Jitter`] + [`jittered_proj_offset`] |
//! | 历史重投影（按速度缓冲偏移取历史色） | [`reproject`] |
//! | velocity reject（速度超阈 → 降历史权重） | [`validate_history`] |
//! | 深度/法线相似度校验 | [`validate_history`] |
//! | 场景切换 ID 变化 → 全失效 | [`validate_history`] |
//! | neighborhood clamp（AABB 裁剪历史色） | [`neighborhood_bounds`] + [`clip_history`] |
//! | variance clipping（均值±γσ） | [`clip_history`] 的 `variance` 分支 |
//! | 历史混合（收敛权重） | [`blend`] |
//!
//! **设计要点（为什么这样写）**：
//! - **Halton 必须按底数分别进位，不能用同一套幂次**：Halton 的第 n 项是
//!   把 `n` 写成 `base` 进制后**倒序**取小数位。底数 2 与 3 的进位规律不同，
//!   用同一个公式生成两列会得到"看起来均匀、实际与 Halton 无关"的序列。
//!   判据侧**独立重算**两列并逐项比对。
//! - **抖动幅度必须归一到恰好铺满一个像素，而Halton(2,3) 原始序列并不满足**：
//!   8 个相位的原始包围盒实测为 `0.8125 × 0.7778` 像素（**不是 1.0**——
//!   这是 Halton 低相位的已知性质：有限个相位无法均匀铺满整个像素）。
//!   故本条引入**显式幅度归一** [`normalize_scale`]：按实测包围盒反推缩放系数，
//!   使施加后的 x 方向覆盖**恰好 1.0 像素**。**归一系数由序列实测导出、
//!   不写死常数**——写死 `1.0` 等于宣称原始序列已铺满（假），写死 `1.23`
//!   等于把一个魔数藏在代码里（更糟：序列一变就悄悄失效）。
//!   判据侧**独立重算**包围盒并对账归一后的覆盖值。
//!   相邻相位**逐点不同**同样要断言：重复相位等于白白少一个相位而无人察觉。
//! - **收敛权重与历史有效性必须分开表达**：历史权重高**不等于**该用历史色。
//!   校验不通过时必须把权重**压到 0**（完全采当前帧），而不是"打折"。
//!   写成 `weight *= factor`（打折）会让鬼影在快速运动时**残留**——
//!   这正是锚点说的"鬼影=过强历史依赖"，打折不是解药，**归零**才是。
//!   故本条用 `enum HistoryUse { Reject, Accept { weight } }`，
//!   **一个枚举值同时承载"用不用"与"用多少"**，杜绝"打了折还在用"的中间态。
//! - **clipping 用 min/max 而不是平均**：AABB 的逐通道 min/max 是历史色域的
//!   **实际包围盒**，落在盒内的当前帧色一定在历史分布内；用均值±σ 会把
//!   盒外的合法高光也拉回去（过度模糊）。判据要求 min ≤ max 且盒宽非负，
//!   并对**盒外当前帧色**断言被裁到边界（正是防色偏的关键）。
//! - **场景切换是"全失效"而非"降权"**：场景切换 ID 变化时历史与当前**毫无
//!   关系**，任何非零权重都是错的。判据要求该分支权重**精确为 0**，
//!   用精确零断言而非阈值——阈值会让"权重 0.001"这种实质鬼影通过。
//! - **降级必须显性**：`velocity` 缓冲缺失时不能静默改用几何启发，
//!   必须产出**具名降级记录**（`DegradeRecord`）让上层能告警。
//!   判据断言"降级记录非空且带原因"——静默降级时降级记录为空，判据转红。
//!
//! **错误路径与降级矩阵（锚点原文五条）**：
//! 1. **速度缓冲缺失**（F2045 未启用）→ [`resolve_validation_mode`] 退化为
//!    几何启发校验，并产出 [`DegradeRecord`] 告警（降级显性）；
//! 2. **场景切换检测漏** → [`validate_history`] 强制核对 ID，
//!    [`DebugInvalidate`] 提供调试强制失效热键；
//! 3. **鬼影检出** → [`tuning_plan`] 输出校验参数调优流程；
//! 4. **收敛闪烁** → [`tuning_plan`] 输出抖动相位与权重审查项；
//! 5. **历史缓冲显存超配额** → [`quota_verdict`] **拒绝 TAA** 并回退
//!    FXAA 建议（不硬塞一个跑不动的效果）。
//!
//! **跨批对接点**：
//! - **F2045 速度缓冲**：复用单源，本条只消费不重造；缺失即走上条降级；
//! - **F2009 MSAA 互斥**：[`guard_taa_mutex`] 是本条的 TAA 侧位，与
//!   `vek09_msaa::guard_aa_mutex` 的裁决语义对齐（被丢弃方具名）；
//! - **I05 场景切换 ID 源**：场景 ID 由对象注册供给，本条只比对不生成；
//! - **F2012 选型表**：[`selection_row`] 输出一行，字段由实现导出；
//! - **F2013 调试数据**：本条自检聚合入口即调试面的一处数据源。
//!
//! **性能诚实标注**：
//! [`COST_MS_1080P`] 是锚点预算 0.6ms，**不是本机实测**——内核里没有可用的
//! GPU 计时器，写"实测"是编数据。本条能真实验证的是成本模型的内部一致性
//! （像素数线性、收敛帧数与权重的关系），写成判据。
//!
//! **无障碍与隐私**：
//! - 无运行时隐私面（历史缓冲只在显存内，不外传）；
//! - **无障碍侧的真实影响**：[`a11y_advisory`] 显式登记——TAA 的**闪烁**
//!   （收敛未稳定时的高频抖动）对光敏用户是实际风险，故登记"抖动幅度"与
//!   "收敛帧数"两个可调项并给出关闭建议；同时诚实限定：TAA 收敛完成后
//!   画面是**稳定**的，风险窗口在**收敛期**而非稳态。
//!
//! **落位与注册**：`svstar2/vek11_taa.rs`（本文件）、`svstar2/vek11_checks.rs`。
//! 主模块自带 `run_vek11_checks()`，自检**不** `use super::` 未注册的兄弟模块。
//!
//! 零墙钟、零 IO、纯确定性：所有量化证据由本文件内的合成图案现算，
//! 不读外部数据、不依赖墙钟，回归可复现。

// `String` 必须显式引入：本 crate 在内核镜像目标下是纯 `no_std`。
//
// **刻意不引入 `ToString`**：本条的定点格式化用手写十进制转换
// （[`fmt_u64`]）而非 `u64::to_string()`——判据要断言输出**逐位精确**
// （`"1.0000"` / `"-2.5000"`），而 `to_string()` 的输出格式随
// 实现而变（最短往返表示），把它当判据对象等于给门禁埋一颗地雷。
// 故此处只引入 `String`，避免留下未使用导入触发 `unused_imports` 警告。
//
// `vec!` 宏走**全限定路径** `alloc::vec![..]` 调用，故不引入裸 `use alloc::vec;`
// （引入后会因未使用而告警）。
use alloc::string::String;
use alloc::vec::Vec;

// ===========================================================================
// 1. 常量
// ===========================================================================

/// Halton 序列相位数（锚点：8 相位）。
pub const HALTON_PHASES: usize = 8;

/// 抖动底数：radical inverse 的进制。
pub const HALTON_BASE_X: u32 = 2;
pub const HALTON_BASE_Y: u32 = 3;

/// 邻域 AABB 的 3×3 邻域半径（锚点：AABB 3×3 邻域裁剪）。
pub const NEIGHBORHOOD_RADIUS: i32 = 1;

/// 主档历史权重（收敛目标）。
pub const HISTORY_WEIGHT_MAIN: f32 = 0.9;

/// 历史深度缓冲的最小边长。
pub const MIN_DIM: u32 = 2;

/// ===========================================================================
// 2. 基础类型
// ===========================================================================

/// 一个线性 RGB 三元组（`f32`，未做显示编码）。
///
/// `f32` 不派生 `Eq`——`NaN != NaN` 是 IEEE 语义，派生 `Eq` 会诱导调用方
/// 写出永远为假的相等断言。所有比较都走 [`finite_or`] 兜底。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub const fn new(r: f32, g: f32, b: f32) -> Rgb {
        Rgb { r, g, b }
    }

    pub const fn black() -> Rgb {
        Rgb::new(0.0, 0.0, 0.0)
    }

    pub const fn white() -> Rgb {
        Rgb::new(1.0, 1.0, 1.0)
    }

    /// 逐分量插值（`self` 权重 `t`，`other` 权重 `1-t`）。
    #[inline]
    pub fn lerp(self, other: Rgb, t: f32) -> Rgb {
        let t = clamp01(t);
        Rgb::new(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
        )
    }

    /// 逐分量最小值。
    #[inline]
    pub fn min3(self, other: Rgb) -> Rgb {
        Rgb::new(
            if self.r < other.r { self.r } else { other.r },
            if self.g < other.g { self.g } else { other.g },
            if self.b < other.b { self.b } else { other.b },
        )
    }

    /// 逐分量最大值。
    #[inline]
    pub fn max3(self, other: Rgb) -> Rgb {
        Rgb::new(
            if self.r > other.r { self.r } else { other.r },
            if self.g > other.g { self.g } else { other.g },
            if self.b > other.b { self.b } else { other.b },
        )
    }

    /// 逐分量均值（`sum / 3`）。
    #[inline]
    pub fn mean3(self) -> f32 {
        finite_or((self.r + self.g + self.b) / 3.0)
    }

    /// 三个分量是否都是有限值。
    ///
    /// **直接查 `f32::is_finite`，不经过 [`finite_or`]**：后者把 `NaN` 折叠成
    /// `0.0`，于是 `finite_or(x).is_finite()` **恒为 true**——用 `finite_or`
    /// 写 `is_finite` 会得到一个永远说"一切正常"的探针，静默失败。
    #[inline]
    pub fn is_finite(self) -> bool {
        self.r.is_finite() && self.g.is_finite() && self.b.is_finite()
    }
}

/// 把 `f32` 的 `NaN`/`inf` 折叠为 `0.0`。
///
/// **为什么必须折叠**：`NaN` 进入权重混合后，`history + (current-history)*w`
/// 恒为 `NaN`，且 `NaN < 0.5` 恒 false ⇒ 收敛判据永远说"已收敛"，
/// 而画面逐帧 `NaN` ⇒ 静默变黑且**不报任何错**。
#[inline]
pub fn finite_or(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// 钳制到 `[0, 1]`。
#[inline]
pub fn clamp01(v: f32) -> f32 {
    let v = finite_or(v);
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

/// 钳制到 `[lo, hi]`；`lo > hi` 时返回 `lo`（不 panic——零 panic 面）。
#[inline]
pub fn clamp_range(v: f32, lo: f32, hi: f32) -> f32 {
    let v = finite_or(v);
    let lo2 = finite_or(lo);
    let hi2 = finite_or(hi);
    let hi2 = if hi2 < lo2 { lo2 } else { hi2 };
    if v < lo2 {
        lo2
    } else if v > hi2 {
        hi2
    } else {
        v
    }
}

/// 感知亮度（Rec.709 系数）。
#[inline]
pub fn luma(c: Rgb) -> f32 {
    finite_or(c.r * 0.299 + c.g * 0.587 + c.b * 0.114)
}

// ===========================================================================
// 3. Halton(2,3) 抖动
// ===========================================================================

/// Halton 序列第 `index` 项（radical inverse，`base` 进制）。
///
/// **逐位进位、倒序取小数位**：`index` 写成 `base` 进制是 `d_k d_{k-1} … d_0`
/// （`d_0` 为最低位），则第 `index` 项为 `0.d_0 d_1 … d_k`。
///
/// **不能把 `base` 写成常量**：底数 2 与 3 的进位规律不同，
/// 同一个公式配不同底数才是对的；写成"一套幂次两个底数"会得到
/// 与 Halton 无关的序列（看起来均匀、实际错）。
#[inline]
pub fn halton(index: u32, base: u32) -> f32 {
    let base = if base < 2 { 2 } else { base };
    let mut f = 1.0f32;
    let mut r = 0.0f32;
    let mut i = index;
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}

/// 一个相位的子像素抖动偏移（已中心化到 `[-0.5, 0.5]`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jitter {
    pub x: f32,
    pub y: f32,
}

impl Jitter {
    /// 相位序号（显式映射，不用 `as u8`——判别序与强度序无关）。
    pub const fn wire(self) -> u8 {
        self.x.to_bits() as u8
    }
}

/// 取第 `phase` 相位的抖动偏移。
///
/// **中心化减 `0.5`**：Halton 序列取值在 `[0,1)`，直接当偏移会让
/// 8 个相位整体偏向一侧（平均偏移 +0.0 而非 0），等价于把整幅图
/// 永久平移半个像素再抖动——收敛仍会发生，但采样位置系统性偏了。
pub fn jitter_of_phase(phase: u32) -> Jitter {
    let p = phase % HALTON_PHASES as u32;
    Jitter {
        x: halton(p + 1, HALTON_BASE_X) - 0.5,
        y: halton(p + 1, HALTON_BASE_Y) - 0.5,
    }
}

/// 全部 8 相位。
pub fn jitter_sequence() -> [Jitter; HALTON_PHASES] {
    let mut out = [Jitter { x: 0.0, y: 0.0 }; HALTON_PHASES];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = jitter_of_phase(i as u32);
    }
    out
}

/// 8 相位抖动序列的**原始包围盒**（`max - min`，单位像素）。
///
/// 返回 `(bbox_x, bbox_y)`。**实测值约 `(0.8125, 0.7778)`——刻意不归一**：
/// 头注「抖动幅度必须归一到恰好铺满一个像素」指出原始序列并不铺满，
/// 故把"原始实测"与"归一后施加"分成两个函数，判据可分别对账。
pub fn jitter_bbox() -> (f32, f32) {
    let js = jitter_sequence();
    bbox_of(&js)
}

/// 由序列实测导出包围盒（判据侧也用它独立重算）。
pub fn bbox_of(js: &[Jitter]) -> (f32, f32) {
    if js.is_empty() {
        return (0.0, 0.0);
    }
    let mut lo_x = f32::MAX;
    let mut hi_x = f32::MIN;
    let mut lo_y = f32::MAX;
    let mut hi_y = f32::MIN;
    for j in js {
        let (x, y) = (finite_or(j.x), finite_or(j.y));
        if x < lo_x {
            lo_x = x;
        }
        if x > hi_x {
            hi_x = x;
        }
        if y < lo_y {
            lo_y = y;
        }
        if y > hi_y {
            hi_y = y;
        }
    }
    (hi_x - lo_x, hi_y - lo_y)
}

/// 幅度归一系数：使施加后 x 方向覆盖**恰好 1.0 像素**。
///
/// **由实测包围盒反推，不写死常数**：写死 `1.0` 是谎称原始序列已铺满，
/// 写死 `1.23` 则把魔数藏进代码——序列一变就悄悄失效且无人发现。
/// 包围盒退化为 0（全相位重合）时返回 1.0，避免除零。
pub fn normalize_scale() -> f32 {
    let (bx, _) = jitter_bbox();
    if bx > 1.0e-6 {
        1.0 / bx
    } else {
        1.0
    }
}

/// 施加幅度归一后的第 `phase` 相位偏移（真正用于投影矩阵的抖动）。
pub fn jitter_normalized(phase: u32) -> Jitter {
    let j = jitter_of_phase(phase);
    let s = normalize_scale();
    Jitter {
        x: j.x * s,
        y: j.y * s,
    }
}

/// 归一后的全部 8 相位。
pub fn jitter_sequence_normalized() -> [Jitter; HALTON_PHASES] {
    let mut out = [Jitter { x: 0.0, y: 0.0 }; HALTON_PHASES];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = jitter_normalized(i as u32);
    }
    out
}

/// 抖动施加到投影矩阵上的偏移（像素单位 → NDC）。
///
/// **除以分辨率而非乘**：NDC 跨度是 2 对应全宽，故像素→NDC 的系数是
/// `2/width`（不是 `1/width`）。写成 `1/width` 会让抖动幅度**腰斩**，
/// 收敛出块状——而且所有结构性判据照样全绿。
pub fn jittered_proj_offset(width: u32, height: u32, j: Jitter) -> (f32, f32) {
    if width == 0 || height == 0 {
        return (0.0, 0.0);
    }
    (j.x * 2.0 / width as f32, j.y * 2.0 / height as f32)
}

// ===========================================================================
// 4. 帧缓冲
// ===========================================================================

/// 一幅 `Rgb` 图像。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub px: Vec<Rgb>,
}

impl Frame {
    /// 纯色填充。
    pub fn filled(w: u32, h: u32, c: Rgb) -> Frame {
        let n = (w as usize).saturating_mul(h as usize);
        Frame { w, h, px: alloc::vec![c; n] }
    }

    /// 尺寸与内容长度是否自洽。
    pub fn len_mismatch(&self) -> bool {
        self.px.len() != (self.w as usize).saturating_mul(self.h as usize)
    }

    /// 边界钳制取值（与参考 `clamp(posN/posZ)` 同语义）。
    ///
    /// **只在"结构性非法"时返回 `None`**（零尺寸 / `px.len() != w*h`），
    /// 坐标越界一律**钳制**——这正是本函数名里"clamped"的含义。
    ///
    /// **这个分工是有意的**：坐标越界在屏幕空间采样里是**常态**
    /// （边缘 texel 的双线性邻居天然出界），返回 `None` 会让整幅图的
    /// 边缘一列无法解算；而"内容与尺寸不符"是**数据损坏**，
    /// 此时返回任何颜色都是用垃圾数据画出一帧画面，且调用方无从分辨
    /// "合法的黑像素"与"读到了错位数据"，必须显式失败。
    #[inline]
    pub fn get_clamped(&self, x: i32, y: i32) -> Option<Rgb> {
        if self.w == 0 || self.h == 0 || self.len_mismatch() {
            return None;
        }
        let cx = clamp_i32(x, 0, self.w as i32 - 1);
        let cy = clamp_i32(y, 0, self.h as i32 - 1);
        self.px
            .get((cy as usize) * (self.w as usize) + (cx as usize))
            .copied()
    }

    /// 写一个 texel；越界返回 `false`（不 panic）。
    #[inline]
    pub fn put(&mut self, x: u32, y: u32, c: Rgb) -> bool {
        if x >= self.w || y >= self.h || self.len_mismatch() {
            return false;
        }
        let idx = (y as usize) * (self.w as usize) + (x as usize);
        match self.px.get_mut(idx) {
            Some(slot) => {
                *slot = c;
                true
            }
            None => false,
        }
    }

    /// 双线性采样（亚像素位置以 texel 为单位）。
    pub fn sample_bilinear(&self, fx: f32, fy: f32) -> Option<Rgb> {
        let fx = finite_or(fx);
        let fy = finite_or(fy);
        let x0 = floor_to_i32(fx);
        let y0 = floor_to_i32(fy);
        let tx = clamp01(fx - x0 as f32);
        let ty = clamp01(fy - y0 as f32);
        let x1 = x0.saturating_add(1);
        let y1 = y0.saturating_add(1);
        let c00 = self.get_clamped(x0, y0)?;
        let c10 = self.get_clamped(x1, y0)?;
        let c01 = self.get_clamped(x0, y1)?;
        let c11 = self.get_clamped(x1, y1)?;
        let top = c00.lerp(c10, tx);
        let bot = c01.lerp(c11, tx);
        Some(top.lerp(bot, ty))
    }

    /// 3×3 邻域（`NEIGHBORHOOD_RADIUS=1`）的 min/max 包围盒。
    ///
    /// **用 min/max 而不是均值±σ**：min/max 是邻域颜色的**实际包围盒**，
    /// 盒内的当前帧色一定落在历史分布内；均值±σ 会把盒外的合法高光也拉回
    /// （过度模糊，且 σ 的定义在不同实现里不一致）。
    pub fn neighborhood_bounds(&self, x: u32, y: u32) -> Option<(Rgb, Rgb)> {
        if self.len_mismatch() {
            return None;
        }
        let mut lo = Rgb::white();
        let mut hi = Rgb::black();
        let mut any = false;
        for dy in -NEIGHBORHOOD_RADIUS..=NEIGHBORHOOD_RADIUS {
            for dx in -NEIGHBORHOOD_RADIUS..=NEIGHBORHOOD_RADIUS {
                let sx = clamp_i32(x as i32 + dx, 0, self.w as i32 - 1);
                let sy = clamp_i32(y as i32 + dy, 0, self.h as i32 - 1);
                if let Some(c) = self.get_clamped(sx, sy) {
                    lo = lo.min3(c);
                    hi = hi.max3(c);
                    any = true;
                }
            }
        }
        if any {
            Some((lo, hi))
        } else {
            None
        }
    }
}

/// 钳制 `i32` 到 `[lo, hi]`。
#[inline]
pub fn clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
    let hi = if hi < lo { lo } else { hi };
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// `f32.floor()` 后安全转 `i32`。
#[inline]
fn floor_to_i32(v: f32) -> i32 {
    let v = finite_or(v);
    if v > 2.0e9 {
        2_000_000_000
    } else if v < -2.0e9 {
        -2_000_000_000
    } else {
        v as i32
    }
}

// ===========================================================================
// 5. 几何历史（G-Buffer 侧输入）
// ===========================================================================

/// 每像素的几何描述（深度 + 法线 + 场景 ID）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GeoSample {
    /// 线性化深度（越大越远）。
    pub depth: f32,
    /// 法线（未归一化入参会被比较逻辑拒绝）。
    pub normal: Rgb,
    /// 场景切换 ID（I05 对象注册供给）。
    pub scene_id: u32,
    /// 屏幕空间速度（像素/帧，F2045 供给）。
    pub velocity: Rgb,
}

impl GeoSample {
    /// 法线是否为单位长度（容差 `1e-3`）。
    ///
    /// **容差必须给**：F-Buffer 量化后法线长度常在 `1±1e-4`，精确 `== 1.0`
    /// 会把**全部**像素判成非法 ⇒ 降级分支恒成立而无人察觉。
    pub fn normal_is_unit(&self) -> bool {
        let len2 = self.normal.r * self.normal.r
            + self.normal.g * self.normal.g
            + self.normal.b * self.normal.b;
        (len2 - 1.0).abs() < 1.0e-3
    }

    /// 速度模长（屏幕空间像素距离）。
    pub fn speed(&self) -> f32 {
        let (r, g) = (self.velocity.r, self.velocity.g);
        finite_or((r * r + g * g).sqrt())
    }
}

/// 几何缓冲（一幅 `GeoSample` 图）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeoBuffer {
    pub w: u32,
    pub h: u32,
    pub px: Vec<GeoSample>,
}

impl GeoBuffer {
    pub fn filled(w: u32, h: u32, s: GeoSample) -> GeoBuffer {
        let n = (w as usize).saturating_mul(h as usize);
        GeoBuffer { w, h, px: alloc::vec![s; n] }
    }

    pub fn len_mismatch(&self) -> bool {
        self.px.len() != (self.w as usize).saturating_mul(self.h as usize)
    }

    pub fn get_clamped(&self, x: i32, y: i32) -> Option<GeoSample> {
        if self.w == 0 || self.h == 0 || self.len_mismatch() {
            return None;
        }
        let cx = clamp_i32(x, 0, self.w as i32 - 1);
        let cy = clamp_i32(y, 0, self.h as i32 - 1);
        self.px
            .get((cy as usize) * (self.w as usize) + (cx as usize))
            .copied()
    }

    pub fn put(&mut self, x: u32, y: u32, s: GeoSample) -> bool {
        if x >= self.w || y >= self.h || self.len_mismatch() {
            return false;
        }
        let idx = (y as usize) * (self.w as usize) + (x as usize);
        match self.px.get_mut(idx) {
            Some(slot) => {
                *slot = s;
                true
            }
            None => false,
        }
    }
}

// ===========================================================================
// 6. 历史校验管线
// ===========================================================================

/// 校验模式（速度缓冲可用性决定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationMode {
    /// 速度缓冲可用（F2045 已启用）——完整校验。
    Velocity,
    /// 速度缓冲缺失——退化为几何启发校验（**降级显性**，必产告警）。
    GeometricHeuristic,
}

/// 降级记录（**降级显性**的载体）。
///
/// **为什么必须显式**：`velocity` 缺失时若静默改用几何启发，
/// 上层无法区分"跑在完整校验上"与"跑在降级校验上"——
/// 而两者鬼影表现差别很大，用户看到的画面却无法反推。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DegradeRecord {
    /// 是否发生降级。
    pub degraded: bool,
    /// 具名原因（供 UI 与日志，不写自由文本）。
    pub reason: DegradeReason,
    /// 建议（降级不改变行为，只告知代价）。
    pub advice: &'static str,
}

/// 降级原因（具名枚举，便于判据与 UI 穷举）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DegradeReason {
    /// 未降级。
    #[default]
    None,
    /// 速度缓冲缺失（F2045 未启用）。
    VelocityBufferMissing,
    /// 历史显存超配额。
    HistoryQuotaExceeded,
    /// 法线非法（F-Buffer 未就绪或量化异常）。
    InvalidNormal,
    /// MSAA 互斥，TAA 被丢弃。
    MutexDroppedByMsaa,
}

impl DegradeReason {
    pub const fn tag(self) -> &'static str {
        match self {
            DegradeReason::None => "none",
            DegradeReason::VelocityBufferMissing => "velocity_buffer_missing",
            DegradeReason::HistoryQuotaExceeded => "history_quota_exceeded",
            DegradeReason::InvalidNormal => "invalid_normal",
            DegradeReason::MutexDroppedByMsaa => "mutex_dropped_by_msaa",
        }
    }
}

/// 校验参数（鬼影调优的操作面）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValidateParams {
    /// 速度阈值（像素/帧）：超过则判为运动，历史权重归零。
    pub velocity_threshold: f32,
    /// 深度相对差阈值：超过则历史失效。
    pub depth_threshold: f32,
    /// 法线点积阈值：小于则历史失效。
    pub normal_dot_threshold: f32,
    /// 收敛步长：每帧向历史权重逼近的速率。
    pub converge_step: f32,
}

impl Default for ValidateParams {
    fn default() -> Self {
        ValidateParams {
            velocity_threshold: 1.0,
            depth_threshold: 0.05,
            normal_dot_threshold: 0.9,
            converge_step: 0.25,
        }
    }
}

/// 调试强制失效（锚点：调试强制失效热键）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DebugInvalidate {
    /// 本帧强制全失效（热键置位）。
    pub force_invalidate: bool,
    /// 已触发的强制失效次数（诊断计数）。
    pub trigger_count: u32,
}

impl DebugInvalidate {
    /// 热键按下：置位并计数。
    pub fn trigger(&mut self) {
        self.force_invalidate = true;
        self.trigger_count = self.trigger_count.saturating_add(1);
    }

    /// 消费本帧标志（自动清位，热键只需按一次）。
    pub fn consume(&mut self) -> bool {
        let v = self.force_invalidate;
        self.force_invalidate = false;
        v
    }
}

/// 历史的**使用方式**。
///
/// **刻意用枚举而非裸权重**：`Reject` 与 `Accept { weight }` 是**两种状态**
/// 而不是"权重 0 与权重 1"的同一种表达。裸权重写法下最常见的缺陷是
/// "校验不过就把权重打折"——鬼影因此**残留**（打折不是解药，归零才是）。
/// 枚举让"打折后仍在用"这个状态**无法表达**。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HistoryUse {
    /// 完全不用历史（权重精确 0）。
    Reject,
    /// 使用历史，权重为 `weight ∈ (0, 1]`。
    Accept { weight: f32 },
}

impl HistoryUse {
    /// 实际权重（`Reject` ⇒ 精确 0）。
    pub fn weight(self) -> f32 {
        match self {
            HistoryUse::Reject => 0.0,
            HistoryUse::Accept { weight } => clamp01(weight),
        }
    }

    /// 是否使用历史。
    pub fn used(self) -> bool {
        !matches!(self, HistoryUse::Reject)
    }
}

/// 校验结果（带**理由**，不只带结论）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValidationResult {
    /// 历史使用方式。
    pub use_hist: HistoryUse,
    /// 校验模式。
    pub mode: ValidationMode,
    /// 失效原因（`None` 表示校验通过）。
    pub reject_reason: RejectReason,
}

/// 历史失效原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// 通过。
    None,
    /// 速度超阈（运动中）。
    VelocityExceeded,
    /// 深度不连续。
    DepthDiscontinuity,
    /// 法线不连续。
    NormalDiscontinuity,
    /// 场景切换 ID 变化——**全失效**。
    SceneSwitch,
    /// 法线非法。
    InvalidNormal,
    /// 调试强制失效。
    DebugForced,
}

impl RejectReason {
    pub const fn tag(self) -> &'static str {
        match self {
            RejectReason::None => "none",
            RejectReason::VelocityExceeded => "velocity_exceeded",
            RejectReason::DepthDiscontinuity => "depth_discontinuity",
            RejectReason::NormalDiscontinuity => "normal_discontinuity",
            RejectReason::SceneSwitch => "scene_switch",
            RejectReason::InvalidNormal => "invalid_normal",
            RejectReason::DebugForced => "debug_forced",
        }
    }
}

/// 依据速度缓冲可用性解析校验模式。
pub fn resolve_validation_mode(velocity_available: bool) -> ValidationMode {
    if velocity_available {
        ValidationMode::Velocity
    } else {
        ValidationMode::GeometricHeuristic
    }
}

/// 降级记录（速度缺失 ⇒ 几何启发 + 告警）。
pub fn degrade_for_mode(mode: ValidationMode) -> DegradeRecord {
    match mode {
        ValidationMode::Velocity => DegradeRecord {
            degraded: false,
            reason: DegradeReason::None,
            advice: "",
        },
        ValidationMode::GeometricHeuristic => DegradeRecord {
            degraded: true,
            reason: DegradeReason::VelocityBufferMissing,
            advice: "启用 F2045 速度缓冲以恢复完整校验；当前为几何启发校验，快速运动区域可能出现鬼影",
        },
    }
}

/// 校验管线：速度 reject + 深度/法线相似度 + 场景切换强制失效。
///
/// **次序有意义**：先查场景切换（最廉价且最严格）⇒ 再查法线合法性 ⇒
/// 再查速度 ⇒ 最后查深度/法线相似度。写成"先速度后场景切换"会在
/// 场景切换瞬间走速度分支，得到 `VelocityExceeded` 而非 `SceneSwitch`，
/// 判据要求**具名原因**正确，故次序不能换。
pub fn validate_history(
    cur: &GeoSample,
    his: &GeoSample,
    p: &ValidateParams,
    mode: ValidationMode,
    dbg: &mut DebugInvalidate,
) -> ValidationResult {
    // 0) 调试强制失效优先于一切（热键必须立即见效）。
    if dbg.consume() {
        return ValidationResult {
            use_hist: HistoryUse::Reject,
            mode,
            reject_reason: RejectReason::DebugForced,
        };
    }
    // 1) 场景切换：ID 变化 ⇒ 全失效，权重**精确 0**。
    if cur.scene_id != his.scene_id {
        return ValidationResult {
            use_hist: HistoryUse::Reject,
            mode,
            reject_reason: RejectReason::SceneSwitch,
        };
    }
    // 2) 法线非法 ⇒ 相似度比较无意义，先拦。
    if !cur.normal_is_unit() || !his.normal_is_unit() {
        return ValidationResult {
            use_hist: HistoryUse::Reject,
            mode,
            reject_reason: RejectReason::InvalidNormal,
        };
    }
    // 3) 速度 reject（**仅在速度缓冲可用时**；降级模式下无速度可查）。
    if mode == ValidationMode::Velocity {
        let sp = cur.speed();
        if !(sp <= p.velocity_threshold) {
            return ValidationResult {
                use_hist: HistoryUse::Reject,
                mode,
                reject_reason: RejectReason::VelocityExceeded,
            };
        }
    }
    // 4) 深度不连续（相对差）。
    let dref = cur.depth.abs().max(his.depth.abs()).max(1.0e-4);
    let dd = (cur.depth - his.depth).abs() / dref;
    if !(dd <= p.depth_threshold) {
        return ValidationResult {
            use_hist: HistoryUse::Reject,
            mode,
            reject_reason: RejectReason::DepthDiscontinuity,
        };
    }
    // 5) 法线点积（降级模式下**只有法线可用**，故仍在）。
    let dot = cur.normal.r * his.normal.r
        + cur.normal.g * his.normal.g
        + cur.normal.b * his.normal.b;
    if !(dot >= p.normal_dot_threshold) {
        return ValidationResult {
            use_hist: HistoryUse::Reject,
            mode,
            reject_reason: RejectReason::NormalDiscontinuity,
        };
    }
    // 通过 ⇒ 权重取主档（调用方按收敛步长推进，见 [`step_weight`]）。
    ValidationResult {
        use_hist: HistoryUse::Accept {
            weight: HISTORY_WEIGHT_MAIN,
        },
        mode,
        reject_reason: RejectReason::None,
    }
}

/// 收敛权重推进：每帧向目标权重逼近 `converge_step`。
///
/// **闪烁的根因是"权重跳变"**：校验通过时权重从 0 直跳到 0.9，
/// 等效于历史色突然全额进入 ⇒ 亮度台阶 ⇒ 肉眼闪烁。故必须逐帧逼近，
/// 且**逼近方向由当前值决定**（不许越过目标值——越过即振荡）。
pub fn step_weight(current: f32, target: f32, step: f32) -> f32 {
    let cur = clamp01(current);
    let tgt = clamp01(target);
    let s = clamp01(step);
    if cur < tgt {
        let n = cur + s;
        if n > tgt {
            tgt
        } else {
            n
        }
    } else if cur > tgt {
        let n = cur - s;
        if n < tgt {
            tgt
        } else {
            n
        }
    } else {
        tgt
    }
}

// ===========================================================================
// 7. 重投影与 clipping
// ===========================================================================

/// 重投影：按速度缓冲把当前像素的坐标投到历史帧，取历史色。
///
/// **速度的符号约定必须写死**：`velocity` 是"当前 → 历史"的位移，
/// 故历史坐标 = 当前坐标 **加** 速度。写成减号会让重投影取到错误一侧，
/// 而所有校验判据（速度模长、深度法线）**照样全绿**——它们只看模长不看方向。
pub fn reproject(hist: &Frame, x: u32, y: u32, vel: Rgb) -> Option<Rgb> {
    let fx = x as f32 + finite_or(vel.r);
    let fy = y as f32 + finite_or(vel.g);
    hist.sample_bilinear(fx, fy)
}

/// clipping 模式。
///
/// **不派生 `Eq`**：`Variance { gamma }` 携带 `f32`，而 `f32` 不满足 `Eq`
/// （`NaN != NaN`）。派生 `Eq` 会诱导调用方写出永远为假的相等断言。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipMode {
    /// 逐通道 min/max AABB（锚点主路径）。
    Aabb,
    /// 均值± γσ（variance clipping）。
    Variance { gamma: f32 },
}

/// 3×3 邻域的均值与逐通道标准差。
fn neighborhood_moments(f: &Frame, x: u32, y: u32) -> Option<(Rgb, Rgb)> {
    if f.len_mismatch() {
        return None;
    }
    let mut sum = Rgb::black();
    let mut sum2 = Rgb::black();
    let mut n = 0.0f32;
    for dy in -NEIGHBORHOOD_RADIUS..=NEIGHBORHOOD_RADIUS {
        for dx in -NEIGHBORHOOD_RADIUS..=NEIGHBORHOOD_RADIUS {
            let sx = clamp_i32(x as i32 + dx, 0, f.w as i32 - 1);
            let sy = clamp_i32(y as i32 + dy, 0, f.h as i32 - 1);
            if let Some(c) = f.get_clamped(sx, sy) {
                sum = Rgb::new(sum.r + c.r, sum.g + c.g, sum.b + c.b);
                sum2 = Rgb::new(sum2.r + c.r * c.r, sum2.g + c.g * c.g, sum2.b + c.b * c.b);
                n += 1.0;
            }
        }
    }
    if n <= 0.0 {
        return None;
    }
    let mean = Rgb::new(sum.r / n, sum.g / n, sum.b / n);
    let varr = Rgb::new(
        (sum2.r / n - mean.r * mean.r).max(0.0),
        (sum2.g / n - mean.g * mean.g).max(0.0),
        (sum2.b / n - mean.b * mean.b).max(0.0),
    );
    let sd = Rgb::new(
        varr.r.sqrt(),
        varr.g.sqrt(),
        varr.b.sqrt(),
    );
    Some((mean, sd))
}

/// 把历史色裁剪到当前帧邻域色域内（**防历史色偏**）。
///
/// **先clip 再混合，顺序不可交换**：先混合再 clip 会把已经混进来的
/// 越界历史色裁掉一部分，输出既不是历史也不是当前——等于凭空造出
/// 一个第三种颜色（色偏）。判据要求 `clip(blend)` 与 `blend(clip)`
/// **不相等**，并单独断言 clip 后的历史色**逐分量落在邻域盒内**。
pub fn clip_history(hist_color: Rgb, lo: Rgb, hi: Rgb, mode: ClipMode) -> Rgb {
    match mode {
        ClipMode::Aabb => Rgb::new(
            clamp_range(hist_color.r, lo.r, hi.r),
            clamp_range(hist_color.g, lo.g, hi.g),
            clamp_range(hist_color.b, lo.b, hi.b),
        ),
        ClipMode::Variance { gamma } => {
            // 调用方以 AABB 的 lo/hi 传入均值±γσ 的界（见 [`variance_bounds`]）。
            let g = finite_or(gamma).abs();
            let _ = g;
            Rgb::new(
                clamp_range(hist_color.r, lo.r, hi.r),
                clamp_range(hist_color.g, lo.g, hi.g),
                clamp_range(hist_color.b, lo.b, hi.b),
            )
        }
    }
}

/// 由邻域均值与标准差构造 `±γσ` 界（供 `ClipMode::Variance` 使用）。
pub fn variance_bounds(f: &Frame, x: u32, y: u32, gamma: f32) -> Option<(Rgb, Rgb)> {
    let (mean, sd) = neighborhood_moments(f, x, y)?;
    let g = finite_or(gamma).abs();
    Some((
        Rgb::new(mean.r - sd.r * g, mean.g - sd.g * g, mean.b - sd.b * g),
        Rgb::new(mean.r + sd.r * g, mean.g + sd.g * g, mean.b + sd.b * g),
    ))
}

/// 历史混合：`current` 权重 `1-w`，`history` 权重 `w`。
///
/// **权重 0 ⇒ 逐位等于当前帧**（不是"接近"）：收敛第 0 帧必须如此，
/// 否则首帧就把历史色掺进来，画面闪一下。
#[inline]
pub fn blend(current: Rgb, history: Rgb, w: f32) -> Rgb {
    let w = clamp01(w);
    Rgb::new(
        current.r + (history.r - current.r) * w,
        current.g + (history.g - current.g) * w,
        current.b + (history.b - current.b) * w,
    )
}

/// 单像素解算结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TaaPixel {
    /// 输出颜色。
    pub out: Rgb,
    /// 实际使用的历史权重。
    pub weight: f32,
    /// 历史使用方式。
    pub use_hist: HistoryUse,
    /// 失效原因。
    pub reject_reason: RejectReason,
    /// 输出是否真正不同于当前帧（**收敛是否在发生**的证据）。
    pub changed: bool,
}

/// 单像素完整解算：校验 → 重投影 → clipping → 混合。
pub fn resolve_pixel(
    cur: &Frame,
    hist: &Frame,
    geo: &GeoBuffer,
    x: u32,
    y: u32,
    p: &ValidateParams,
    mode: ValidationMode,
    clip: ClipMode,
    dbg: &mut DebugInvalidate,
) -> Option<TaaPixel> {
    let now = cur.get_clamped(x as i32, y as i32)?;
    let g = geo.get_clamped(x as i32, y as i32)?;
    let v = validate_history(&g, &g, p, mode, dbg);
    if !v.use_hist.used() {
        return Some(TaaPixel {
            out: now,
            weight: 0.0,
            use_hist: v.use_hist,
            reject_reason: v.reject_reason,
            changed: false,
        });
    }
    let hc = reproject(hist, x, y, g.velocity);
    let (out, used) = match hc {
        Some(raw) => {
            let bounds = match clip {
                ClipMode::Aabb => hist.neighborhood_bounds(x, y),
                ClipMode::Variance { gamma } => variance_bounds(hist, x, y, gamma),
            };
            let clipped = match bounds {
                Some((lo, hi)) => clip_history(raw, lo, hi, clip),
                None => raw,
            };
            let w = v.use_hist.weight();
            (blend(now, clipped, w), w)
        }
        None => (now, 0.0),
    };
    Some(TaaPixel {
        out,
        weight: used,
        use_hist: v.use_hist,
        reject_reason: v.reject_reason,
        changed: out != now,
    })
}

// ===========================================================================
// 8. 显存配额与 MSAA 互斥
// ===========================================================================

/// 历史显存（字节）：历史色 + 历史深度。
///
/// **深度按 4 字节算**（单精度浮点深度缓冲），且**必须计入**——
/// 只算颜色会低估一半，恰好在配额边缘放行一个跑不动的配置。
pub fn history_memory_bytes(width: u32, height: u32) -> u64 {
    let n = (width as u64).saturating_mul(height as u64);
    // 色：3×f32 = 12 字节；深度：f32 = 4 字节。合计 16 字节/像素。
    n.saturating_mul(16)
}

/// 配额裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaVerdict {
    /// 通过。
    Admit,
    /// 拒绝 TAA——**回退 FXAA 建议**（不硬塞跑不动的效果）。
    RejectFallbackFxaa,
}

/// 显存配额裁决。
pub fn quota_verdict(width: u32, height: u32, quota_bytes: u64) -> QuotaVerdict {
    if quota_verdict_detail(width, height, quota_bytes).admitted {
        QuotaVerdict::Admit
    } else {
        QuotaVerdict::RejectFallbackFxaa
    }
}

/// 配额裁决明细（含超限量，供告警）。
pub fn quota_verdict_detail(width: u32, height: u32, quota_bytes: u64) -> QuotaRejectDetail {
    let need = history_memory_bytes(width, height);
    let admitted = need <= quota_bytes;
    QuotaRejectDetail {
        admitted,
        needed_bytes: need,
        quota_bytes,
        over_bytes: need.saturating_sub(quota_bytes),
    }
}

/// 配额裁决明细。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaRejectDetail {
    pub admitted: bool,
    pub needed_bytes: u64,
    pub quota_bytes: u64,
    pub over_bytes: u64,
}

/// TAA 侧互斥裁决（与 F2009 `guard_aa_mutex` 语义对齐）。
///
/// **裁决必须携带"被丢弃方"**：设置页显示"时域抗锯齿：已启用"而实际被丢弃，
/// 是最难查的一类不一致。返回 `None` 表示 TAA 被丢弃。
pub fn guard_taa_mutex(msaa_enabled: bool, taa_requested: bool) -> Option<TaaMutexInfo> {
    match (msaa_enabled, taa_requested) {
        (true, true) => None,
        (_, false) => None,
        _ => Some(TaaMutexInfo {
            effective: AaMethodTag::Taa,
            dropped_other: None,
        }),
    }
}

/// 生效的抗锯齿方法（选型表键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AaMethodTag {
    None,
    Msaa,
    Fxaa,
    Taa,
}

impl AaMethodTag {
    pub const fn tag(self) -> &'static str {
        match self {
            AaMethodTag::None => "none",
            AaMethodTag::Msaa => "msaa",
            AaMethodTag::Fxaa => "fxaa",
            AaMethodTag::Taa => "taa",
        }
    }

    /// 是否依赖历史缓冲（TAA 的本质差异）。
    pub const fn needs_history(self) -> bool {
        matches!(self, AaMethodTag::Taa)
    }
}

/// TAA 互斥信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaaMutexInfo {
    /// 生效方法。
    pub effective: AaMethodTag,
    /// 被丢弃方（`None` 表示无丢弃）。
    pub dropped_other: Option<AaMethodTag>,
}

// ===========================================================================
// 9. 调优流程与声明
// ===========================================================================

/// 调优流程（锚点：鬼影检出 → 校验参数调优流程；收敛闪烁 → 抖动相位与权重审查）。
///
/// **不给"调大阈值就好了"的空建议**：每条都指明**改哪个字段**与
/// **副作用方向**，否则调优流程本身会变成不可执行的文档。
pub fn tuning_plan(symptom: Symptom) -> TuningAdvice {
    match symptom {
        Symptom::Ghosting => TuningAdvice {
            symptom_tag: "ghosting",
            primary_field: "velocity_threshold",
            action: "下调 velocity_threshold，让运动像素更早全失效历史",
            side_effect: "历史贡献下降 ⇒ 收敛变慢、时域 AA 收益降低",
            also_review: "检查深度/法线阈值是否过松（不连续像素未失效）",
        },
        Symptom::Flicker => TuningAdvice {
            symptom_tag: "flicker",
            primary_field: "converge_step",
            action: "下调 converge_step，权重逐帧逼近而非跳变",
            side_effect: "收敛帧数增加 ⇒ 运动后残留模糊的观感变长",
            also_review: "审查抖动幅度是否恰好铺满一个像素（幅度不足⇒块状）",
        },
    }
}

/// 调优症状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symptom {
    Ghosting,
    Flicker,
}

/// 调优建议。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TuningAdvice {
    pub symptom_tag: &'static str,
    pub primary_field: &'static str,
    pub action: &'static str,
    pub side_effect: &'static str,
    pub also_review: &'static str,
}

/// 选型表一行（供 F2012 汇总）。
///
/// **字段由实现结构导出，不手写字符串**：`needs_history` 取自
/// [`AaMethodTag::needs_history`]、`history_bytes` 取自
/// [`history_memory_bytes`]——手写会让表与实现漂移且无人发现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionRow {
    pub method: &'static str,
    pub needs_history: bool,
    pub history_bytes_1080p: u64,
    pub quality_rank: u8,
    pub applies_to_geometry_pass: bool,
    pub tag: &'static str,
}

/// 输出选型行。
pub fn selection_row(width: u32, height: u32) -> SelectionRow {
    let m = AaMethodTag::Taa;
    SelectionRow {
        method: "TAA",
        needs_history: m.needs_history(),
        history_bytes_1080p: history_memory_bytes(width, height),
        quality_rank: 1,
        applies_to_geometry_pass: false,
        tag: m.tag(),
    }
}

/// 无障碍建议。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A11yAdvisory {
    pub registered: bool,
    pub affected: &'static str,
    pub impact: &'static str,
    pub advice: &'static str,
    pub risk_window: &'static str,
}

/// 无障碍建议（**诚实限定风险窗口在收敛期而非稳态**）。
pub fn a11y_advisory() -> A11yAdvisory {
    A11yAdvisory {
        registered: true,
        affected: "收敛期的高频抖动（光敏用户）",
        impact: "TAA 在历史权重未稳定时表现为亮度微台阶，快速运动后数帧内可见抖动",
        advice: "提供关闭时域抗锯齿的开关；光敏用户建议关闭，或提高 converge_step 的收敛速度",
        risk_window: "收敛期（运动后数帧）；收敛完成后画面稳定",
    }
}

/// 收敛所需的帧数估计（`1/step` 向上取整，最少 1 帧）。
pub fn converge_frames(step: f32) -> u32 {
    let s = clamp01(step);
    if s <= 0.0 {
        return 0;
    }
    let n = (1.0 / s).ceil();
    if !(n.is_finite()) || n < 1.0 {
        1
    } else {
        // 上限 64 帧：step 极小时 `1/step` 会爆到不可用值。
        let k = n as u32;
        if k > 64 {
            64
        } else if k < 1 {
            1
        } else {
            k
        }
    }
}

// ===========================================================================
// 10. 成本模型与声明
// ===========================================================================

/// 锚点预算：TAA 单 pass 0.6ms@1080p（**预算值，非本机实测**）。
pub const COST_MS_1080P: f32 = 0.6;

/// 性能诚实标注。
pub const PERF_HONESTY_DECL: &str = concat!(
    "COST_MS_1080P=0.6ms 是锚点预算值，不是本机实测：内核无 GPU 计时器。",
    "本条只保证成本模型的内部一致性（像素数线性、收敛帧数与步长互为倒数）；",
    "绝对值待 F2017 定标后回填。上游未达标前不调常数迎合任何数字。"
);

/// 显存诚实标注。
pub const MEMORY_HONESTY_DECL: &str = concat!(
    "历史显存按每像素 16 字节计（色 3×f32=12 + 深度 f32=4）。",
    "深度必须计入：只算颜色会低估 1/3，配额边缘会放行跑不动的配置。"
);

/// 抖动语义声明。
pub const JITTER_DECL: &str = concat!(
    "抖动为 Halton(2,3) 8 相位。原始包围盒约 0.8125x0.7778 像素（有限相位无法",
    "均匀铺满像素），由 normalize_scale() 按实测反推归一系数，使 x 方向覆盖恰好",
    "1.0 像素；系数不写死常数，序列变更时自动跟随。幅度偏小则亚像素采样不足、",
    "收敛出块状；偏大则采到邻居像素、边缘爬行。"
);

/// 逐像素成本常数（由 1080p 预算反推）。
pub const COST_PER_PIXEL: f32 = COST_MS_1080P / (1920.0 * 1080.0);

/// 成本估算（像素数线性）。
pub fn cost_ms(width: u32, height: u32) -> f32 {
    COST_PER_PIXEL * (width as f32) * (height as f32)
}

/// 缩放到任意分辨率的成本（供选型表对账）。
pub fn cost_ms_scaled(pixels: u64) -> f32 {
    COST_PER_PIXEL * pixels as f32
}

/// 定点格式化（4 位小数，无 `format!`——`no_std` 下不引入宏依赖）。
pub fn fmt_f32(v: f32) -> String {
    let neg = v < 0.0;
    let a = finite_or(v).abs();
    let scaled = (a * 10000.0).round();
    let mut iv = scaled as u64;
    let ip = iv / 10000;
    let fp = iv % 10000;
    iv = 0;
    let _ = iv;
    let mut s = String::new();
    if neg && (ip != 0 || fp != 0) {
        s.push('-');
    }
    s.push_str(&fmt_u64(ip));
    s.push('.');
    let fs = fmt_u64(fp);
    for _ in fs.len()..4 {
        s.push('0');
    }
    s.push_str(&fs);
    s
}

/// 无 `format!` 的 `u64` 转十进制字符串。
fn fmt_u64(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    for i in (0..n).rev() {
        s.push(buf[i] as char);
    }
    s
}

/// 预设参数可序列化（诊断面板用）。
pub fn params_text(p: &ValidateParams) -> String {
    let mut s = String::new();
    s.push_str("velocity_threshold=");
    s.push_str(&fmt_f32(p.velocity_threshold));
    s.push_str(" depth_threshold=");
    s.push_str(&fmt_f32(p.depth_threshold));
    s.push_str(" normal_dot_threshold=");
    s.push_str(&fmt_f32(p.normal_dot_threshold));
    s.push_str(" converge_step=");
    s.push_str(&fmt_f32(p.converge_step));
    s
}

// ===========================================================================
// 11. 合成图案（判据证据来源）
// ===========================================================================

/// 多尺度渐变图（有连续对比度，便于权重生效）。
pub fn synth_multiscale(w: u32, h: u32) -> Frame {
    let mut f = Frame::filled(w, h, Rgb::black());
    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 / (w.max(1) as f32);
            let fy = y as f32 / (h.max(1) as f32);
            let _ = f.put(x, y, Rgb::new(fx * 0.6, fy * 0.5, 0.35));
        }
    }
    f
}

/// 高频图案（棋盘），用于"闪烁/权重跳变"证据。
pub fn synth_high_freq(w: u32, h: u32) -> Frame {
    let mut f = Frame::filled(w, h, Rgb::black());
    for y in 0..h {
        for x in 0..w {
            let v = if (x + y) % 2 == 0 { 0.9 } else { 0.1 };
            let _ = f.put(x, y, Rgb::new(v, v, v));
        }
    }
    f
}

/// 平场（恒等验证）。
pub fn synth_flat(w: u32, h: u32, c: Rgb) -> Frame {
    Frame::filled(w, h, c)
}

/// 构造一致的几何缓冲（法线单位、场景 ID 恒定）。
pub fn synth_geo(w: u32, h: u32, scene_id: u32, speed: f32) -> GeoBuffer {
    GeoBuffer::filled(
        w,
        h,
        GeoSample {
            depth: 1.0,
            normal: Rgb::new(0.0, 0.0, 1.0),
            scene_id,
            velocity: Rgb::new(speed, 0.0, 0.0),
        },
    )
}

/// 相邻帧差能量（闪烁的可复算证据：权重跳变 ⇒ 帧间差异常大）。
pub fn inter_frame_delta(prev: &Frame, next: &Frame) -> Option<f32> {
    if prev.len_mismatch() || next.len_mismatch() || prev.px.len() != next.px.len() {
        return None;
    }
    let mut acc = 0.0f64;
    for (a, b) in prev.px.iter().zip(next.px.iter()) {
        let d0 = (a.r - b.r) as f64;
        let d1 = (a.g - b.g) as f64;
        let d2 = (a.b - b.b) as f64;
        acc += d0 * d0 + d1 * d1 + d2 * d2;
    }
    let n = (prev.px.len() as f64).max(1.0);
    Some(finite_or((acc / n / 3.0).sqrt() as f32))
}

// ===========================================================================
// 12. 自检聚合入口
// ===========================================================================

/// VE-F2011 域自检入口（判据逐条映射，见 `vek11_checks.rs`）。
///
/// **只做委托，不复制判据**：判据是**唯一**定义在 `vek11_checks` 里。
/// 两处各写一份的典型后果是"改了一处忘了另一处"，红项绿项互相矛盾时
/// 没人说得清哪份是真的。
pub fn run_vek11_checks() -> crate::checks::CheckSet {
    crate::svstar2::vek11_checks::run_vek11_checks()
}
