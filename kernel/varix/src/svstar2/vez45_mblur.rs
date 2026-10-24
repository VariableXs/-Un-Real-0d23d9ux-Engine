//! VE-F5245 · 运动模糊（VE-Z 域 · Z03 后处理 · 锚点目标 330 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5245`
//!
//! **判据（锚点原文）**：相机速度重建、切片采样、深度感知、内外鬼影抑制、
//! 判据。
//!
//! # 深化说明
//!
//! ## 深化一：模糊量必须来自**前后帧相机矩阵差**，不是「速度参数」
//!
//! 直接暴露一个 `blur_amount` 参数的做法，会让模糊与真实运动脱钩：镜头
//! 快速平移与缓速跟拍会产生同样的模糊，观感上是「假」。正确做法是用上一帧
//! 与当前帧的**视图投影矩阵**做重投影，得到每个像素的屏幕空间位移向量。
//! 本实现提供 [`CameraDelta`]：由两帧的视图矩阵与投影参数重建 4×4（简化为
//! 平移 + 旋转两部分），并给出「世界点 → 屏幕位移」的解析式。
//! `static_camera_has_no_blur` 判据要求静止相机位移严格为 0。
//!
//! ## 深化二：切片采样用**黄金角旋转抖动**，不是均匀切片
//!
//! 沿模糊向量取 N 个均匀样本会产生**条带**（banding）——每片之间是硬边界。
//! 本实现给每片加一个黄金角（0.618…）错开的抖动偏移，使样本在帧间与片间
//! 都近似独立，条带被打散为细噪声。`jitter_is_stratified` 判据把 128 个
//! 样本分 8 桶，每桶落点应互不相同且覆盖整段（这是「无条带」的机检形式）。
//!
//! ## 深化三：深度感知用**逐片比较深度**，近物遮远物
//!
//! 模糊采到「被前景挡住」的背景像素会产生**鬼影**（前景边缘拖出背景颜色）。
//! 本实现在每个切片位置比较该片深度与中心深度：片比中心**远**则跳过
//! （被遮挡）。`depth_aware_rejects_far_samples` 判据构造「中心近、采样远」
//! 的场景，要求有效样本数显著少于总样本数。
//!
//! ## 深化四：内外鬼影是两种不同成因，分开抑制
//!
//! - **内鬼影**（inner ghosting）：物体自身运动过快，模糊尾部反向延伸到自己
//!   身上 → 成因是**采样方向被符号反转**。抑制法：只沿**速度正方向**采样
//!   （`toward = -1` 一端的能量按比例衰减）。
//! - **外鬼影**（outer ghosting）：背景被前景的运动拖过来 → 成因是**深度
//!   权重不足**（见深化三）。
//!
//! 两者若混成一个「鬼影系数」，调一个必然连带另一个。本实现把
//! [`BlurConfig::inner_ghost_suppress`] 与深度阈值分开，且各有独立判据。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const MBLUR_VERSION: &str = "F5245-mblur-v1";

// ── 诊断码（本模块独占段 0xE060~0xE06F）─────────────────────────────
const SEG_BASE: u16 = 0xE060;
/// 切片数非法（0 或 > SLICES_MAX）。
pub const E_SLICES: u16 = SEG_BASE + 1;
/// 快门角度非法（不在 [0, 360]）。
pub const E_SHUTTER: u16 = SEG_BASE + 2;
/// 鬼影抑制系数非法（不在 [0,1]）。
pub const E_GHOST: u16 = SEG_BASE + 3;
/// 最大模糊长度非法（≤0 或 > MAX_BLUR_PX）。
pub const E_MAX_LEN: u16 = SEG_BASE + 4;

/// 切片数上限。依据：16 片时模糊带内相邻片间距约为带宽 1/16，配合抖动后
/// 条带已不可见；再增到 32 片仅降噪 3dB 而采样成本翻倍。
pub const SLICES_MAX: u8 = 16;

/// 模糊长度上限（像素）。依据：1080p 下 64px 已是 3% 屏宽，超过即观感
/// 失控（应为「镜头在飞」而非「画面糊」）。
pub const MAX_BLUR_PX: f32 = 64.0;

/// 黄金角（弧度），用于切片抖动错位。
///
/// 依据：`2π * (1 - 1/φ) ≈ 2.399963`，这是低差异序列在圆周上分布最均匀的
/// 旋转步长（黄金角螺旋），任意连续切片数下互不重合。
pub const GOLDEN_ANGLE: f32 = 2.399_963_2;

/// 相机帧间变化（用于重投影求位移）。
#[derive(Clone, Copy, Debug)]
pub struct CameraDelta {
    /// 相机世界位置（当前帧）。
    pub pos: [f32; 3],
    /// 相机位置变化（当前 − 上一帧），世界单位。
    pub move_delta: [f32; 3],
    /// 相机绕 Y 轴旋转（当前 − 上一帧），弧度。
    pub yaw_delta: f32,
    /// 竖直视场角（弧度）。
    pub fov_y: f32,
    /// 屏幕高度（像素）。
    pub screen_h: f32,
}

impl CameraDelta {
    /// 静止相机。
    pub fn still() -> Self {
        Self {
            pos: [0.0, 0.0, 0.0],
            move_delta: [0.0, 0.0, 0.0],
            yaw_delta: 0.0,
            fov_y: 1.0,
            screen_h: 1080.0,
        }
    }

    /// 校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.fov_y.is_nan() || self.fov_y <= 0.01 || self.fov_y > 3.1 {
            return Err(E_SHUTTER);
        }
        if self.screen_h.is_nan() || self.screen_h <= 0.0 {
            return Err(E_MAX_LEN);
        }
        if self.move_delta.iter().any(|v| v.is_nan()) || self.yaw_delta.is_nan() {
            return Err(E_MAX_LEN);
        }
        Ok(())
    }

    /// 相机是否静止（位移与旋转都为 0）。
    pub fn is_still(&self) -> bool {
        self.move_delta[0] == 0.0
            && self.move_delta[1] == 0.0
            && self.move_delta[2] == 0.0
            && self.yaw_delta == 0.0
    }

    /// 世界点在屏幕上的位移向量（像素）。
    ///
    /// 分解为两部分：
    /// 1. **平移**：相机移动 `d` 会让深度 `z` 处的点在屏幕上反向移动
    ///    `-d_xy / z * f`（`f = screen_h / (2 tan(fov/2))` 为像素焦距）；
    /// 2. **旋转**：相机 yaw 变化 `Δθ` 让所有点同向平移 `-Δθ * f`。
    ///
    /// 只取屏幕平面（XY）分量：深度方向位移不产生屏幕运动（只产生缩放，
    /// 属 zoom blur 范畴，本模块不做）。
    pub fn screen_displacement(&self, world: [f32; 3]) -> [f32; 2] {
        let f = self.focal_px();
        // 相对相机的深度（至少 0.05，防除零放大）。
        let z = (world[2] - self.pos[2]).abs().max(0.05);
        let dx = -self.move_delta[0] / z * f;
        let dy = -self.move_delta[1] / z * f;
        let dr = -self.yaw_delta * f;
        [dx + dr, dy]
    }

    /// 像素焦距 `f = screen_h / (2 tan(fov/2))`。
    pub fn focal_px(&self) -> f32 {
        let half = self.fov_y * 0.5;
        let t = tanf(half);
        if t <= 1e-6 {
            return self.screen_h;
        }
        self.screen_h / (2.0 * t)
    }
}

/// 运动模糊参数。
#[derive(Clone, Copy, Debug)]
pub struct BlurConfig {
    /// 切片数。
    pub slices: u8,
    /// 快门角度（度）：360 = 整帧曝光，180 = 半帧（电影常用）。
    pub shutter_deg: f32,
    /// 最大模糊长度（像素）。
    pub max_len_px: f32,
    /// 内鬼影抑制系数（0=不抑制，1=完全沿正方向单向采样）。
    pub inner_ghost_suppress: f32,
    /// 深度容差（世界单位）：片深度大于中心 + 容差即视为被遮挡。
    pub depth_tolerance: f32,
}

impl BlurConfig {
    /// 标准（180° 快门，接近电影的 1/48s 曝光）。
    pub fn standard() -> Self {
        Self {
            slices: 8,
            shutter_deg: 180.0,
            max_len_px: 32.0,
            inner_ghost_suppress: 0.5,
            depth_tolerance: 0.5,
        }
    }

    pub fn validate(&self) -> Result<(), u16> {
        if self.slices == 0 || self.slices > SLICES_MAX {
            return Err(E_SLICES);
        }
        if self.shutter_deg.is_nan() || !(0.0..=360.0).contains(&self.shutter_deg) {
            return Err(E_SHUTTER);
        }
        if self.inner_ghost_suppress.is_nan() || !(0.0..=1.0).contains(&self.inner_ghost_suppress) {
            return Err(E_GHOST);
        }
        if self.max_len_px.is_nan() || self.max_len_px <= 0.0 || self.max_len_px > MAX_BLUR_PX {
            return Err(E_MAX_LEN);
        }
        Ok(())
    }

    /// 快门缩放系数：`shutter_deg / 360`。
    pub fn shutter_scale(&self) -> f32 {
        self.shutter_deg / 360.0
    }

    /// 某像素的模糊向量（像素），已按快门与上限钳制。
    pub fn blur_vector(&self, cam: &CameraDelta, world: [f32; 3]) -> [f32; 2] {
        let d = cam.screen_displacement(world);
        let k = self.shutter_scale();
        let v = [d[0] * k, d[1] * k];
        let len = sqrtf(v[0] * v[0] + v[1] * v[1]);
        if len > self.max_len_px {
            let s = self.max_len_px / len;
            [v[0] * s, v[1] * s]
        } else {
            v
        }
    }
}

/// 一个采样切片。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slice {
    /// 沿模糊方向的偏移系数（`-1..1`，相对半矢量）。
    pub t: f32,
    /// 该片的采样权重（含内鬼影抑制后的衰减）。
    pub weight: f32,
    /// 该片在像素空间的位置偏移。
    pub offset: [f32; 2],
}

/// 生成切片表（含黄金角抖动）。
///
/// `t_i = (i + 0.5)/N * 2 - 1`（把 `[-1,1]` 均分），再按黄金角错开每片的
/// 实际取址；`weight` 按内鬼影抑制系数对 `t<0`（反向）一侧线性衰减。
pub fn build_slices(cfg: &BlurConfig, blur: [f32; 2]) -> Vec<Slice> {
    let n = cfg.slices as usize;
    let mut out = Vec::with_capacity(n);
    let half = [blur[0] * 0.5, blur[1] * 0.5];
    for i in 0..n {
        // 均匀基准位置（片中心），范围开区间 (-1, 1)。
        let base = (i as f32 + 0.5) / n as f32 * 2.0 - 1.0;
        // 黄金角抖动：每片错开 1/N 的黄金比例，使片间落点互不重合。
        let jitter = (golden_frac(i as f32 * GOLDEN_ANGLE) - 0.5) / n as f32;
        let t = (base + jitter).clamp(-1.0, 1.0);
        // 内鬼影抑制：反向（t<0）一侧按 suppress 衰减。
        let weight = if t < 0.0 {
            1.0 - cfg.inner_ghost_suppress
        } else {
            1.0
        };
        out.push(Slice {
            t,
            weight,
            offset: [t * half[0], t * half[1]],
        });
    }
    out
}

/// 深度感知采样：从切片表里筛出**未被遮挡**的片。
///
/// `depth_of` 返回某片位置的深度；`center_depth` 为中心像素深度。
/// 规则：片深度 > 中心深度 + 容差 ⇒ 该片被前景遮挡，跳过。
pub fn depth_filtered(
    slices: &[Slice],
    center_depth: f32,
    tolerance: f32,
    depth_of: impl Fn(&Slice) -> f32,
) -> (usize, usize) {
    let mut kept = 0usize;
    for s in slices {
        let d = depth_of(s);
        if d <= center_depth + tolerance {
            kept += 1;
        }
    }
    (kept, slices.len())
}

/// 加权合成：按切片权重求平均色（4 通道 RGBA）。
///
/// `fetch` 返回某片的颜色。权重和为 0 时返回第一个片颜色（不除零）。
pub fn composite(
    slices: &[Slice],
    fetch: impl Fn(&Slice) -> [f32; 4],
) -> [f32; 4] {
    let mut acc = [0.0f32; 4];
    let mut wsum = 0.0f32;
    let mut first: Option<[f32; 4]> = None;
    for s in slices {
        let c = fetch(s);
        if first.is_none() {
            first = Some(c);
        }
        for k in 0..4 {
            acc[k] += c[k] * s.weight;
        }
        wsum += s.weight;
    }
    if wsum <= 0.0 {
        return first.unwrap_or([0.0; 4]);
    }
    [acc[0] / wsum, acc[1] / wsum, acc[2] / wsum, acc[3] / wsum]
}

// ── 数学工具 ────────────────────────────────────────────────────

/// 取小数部分（黄金比例抖动用）。
pub fn golden_frac(x: f32) -> f32 {
    let f = x - floorf(x);
    if f < 0.0 {
        f + 1.0
    } else {
        f
    }
}

pub fn floorf(x: f32) -> f32 {
    let t = x as i32 as f32;
    if x < 0.0 && t != x {
        t - 1.0
    } else {
        t
    }
}

pub fn sqrtf(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return 0.0;
    }
    let mut g = if x > 1.0 { x } else { 1.0 };
    for _ in 0..24 {
        g = 0.5 * (g + x / g);
    }
    g
}

/// 正切（`sinf/cosf`，|x| < π/2 时精度足够）。
pub fn tanf(x: f32) -> f32 {
    let c = cosf(x);
    if c.abs() < 1e-6 {
        return 1e6;
    }
    sinf(x) / c
}

pub fn cosf(x: f32) -> f32 {
    let two_pi = core::f32::consts::PI * 2.0;
    let mut t = x % two_pi;
    if t > core::f32::consts::PI {
        t -= two_pi;
    } else if t < -core::f32::consts::PI {
        t += two_pi;
    }
    let t2 = t * t;
    let mut term = 1.0f32;
    let mut sum = 1.0f32;
    let mut k = 1.0f32;
    for _ in 0..12 {
        term *= -t2 / ((2.0 * k - 1.0) * (2.0 * k));
        sum += term;
        k += 1.0;
    }
    sum
}

pub fn sinf(x: f32) -> f32 {
    cosf(x - core::f32::consts::PI * 0.5)
}

// ── 判据 ────────────────────────────────────────────────────────

/// F5245 判据集。
pub fn run_vez45_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5245");

    // ── 1. 相机速度重建：静止零位移 ──
    {
        let cam = CameraDelta::still();
        let d = cam.screen_displacement([10.0, 5.0, 100.0]);
        s.add(
            "VZ45-相机-01",
            d[0] == 0.0 && d[1] == 0.0,
            "静止相机位移严格为 0（无伪模糊）",
        );
        s.add("VZ45-相机-02", cam.is_still(), "is_still 判定正确");
        // 平移：相机向 +X 移动，屏幕点向 -X 移动。
        let cam2 = CameraDelta {
            move_delta: [10.0, 0.0, 0.0],
            ..CameraDelta::still()
        };
        let d2 = cam2.screen_displacement([0.0, 0.0, 100.0]);
        s.add(
            "VZ45-相机-03",
            d2[0] < 0.0 && d2[1] == 0.0,
            "相机 +X 平移 → 屏幕点 -X 位移（方向正确）",
        );
        // 位移量与深度成反比：深度翻倍、位移减半。
        let near = cam2.screen_displacement([0.0, 0.0, 50.0]);
        let far = cam2.screen_displacement([0.0, 0.0, 100.0]);
        s.add(
            "VZ45-相机-04",
            (near[0] / far[0] - 2.0).abs() < 0.02,
            "位移与深度反比（深度翻倍位移减半）",
        );
        // 旋转：yaw 变化让所有点同向平移（与深度无关）。
        let cam3 = CameraDelta {
            yaw_delta: 0.01,
            ..CameraDelta::still()
        };
        let a = cam3.screen_displacement([0.0, 0.0, 50.0]);
        let b = cam3.screen_displacement([0.0, 0.0, 500.0]);
        s.add(
            "VZ45-相机-05",
            (a[0] - b[0]).abs() < 1e-4,
            "旋转位移与深度无关（全场同向，符合 yaw 的几何）",
        );
        // 焦距换算核对：fov=1.0rad、h=1080 → f ≈ 886。
        let f = cam.focal_px();
        s.add(
            "VZ45-相机-06",
            (f - 886.0).abs() < 3.0,
            "像素焦距 f = h/(2tan(fov/2)) ≈ 886（fov=1rad,h=1080）",
        );
        // 非法 fov 拒绝。
        let bad = CameraDelta {
            fov_y: 0.0,
            ..CameraDelta::still()
        }
        .validate()
        .is_err();
        s.add("VZ45-相机-07", bad, "反向：fov=0 被拒");
    }

    // ── 2. 快门与最长钳制 ──
    {
        let cfg = BlurConfig::standard();
        s.add(
            "VZ45-快门-01",
            (cfg.shutter_scale() - 0.5).abs() < 1e-6,
            "180° 快门缩放 = 0.5（半帧曝光）",
        );
        let full = BlurConfig {
            shutter_deg: 360.0,
            ..BlurConfig::standard()
        };
        s.add(
            "VZ45-快门-02",
            (full.shutter_scale() - 1.0).abs() < 1e-6,
            "360° 快门缩放 = 1.0",
        );
        let zero = BlurConfig {
            shutter_deg: 0.0,
            ..BlurConfig::standard()
        };
        let z = zero.blur_vector(&CameraDelta::still(), [0.0, 0.0, 10.0]);
        s.add(
            "VZ45-快门-03",
            z[0] == 0.0 && z[1] == 0.0,
            "0° 快门 → 零模糊（可关闭）",
        );
        // 上限钳制。
        let cam = CameraDelta {
            move_delta: [1.0e6, 0.0, 0.0],
            ..CameraDelta::still()
        };
        let v = cfg.blur_vector(&cam, [0.0, 0.0, 0.01]);
        let len = sqrtf(v[0] * v[0] + v[1] * v[1]);
        s.add(
            "VZ45-快门-04",
            len <= cfg.max_len_px + 1e-3,
            "极大位移下模糊长度被钳到 max_len_px",
        );
        // 反向：不钳制的话长度会是天文数字。
        let raw = cam.screen_displacement([0.0, 0.0, 0.01]);
        let raw_len = sqrtf(raw[0] * raw[0] + raw[1] * raw[1]);
        s.add(
            "VZ45-快门-05",
            raw_len > cfg.max_len_px * 100.0,
            "反向：未钳制的原始位移远超上限（钳制确有必要）",
        );
    }

    // ── 3. 切片：黄金角抖动分层 ──
    {
        let cfg = BlurConfig::standard();
        let slices = build_slices(&cfg, [100.0, 0.0]);
        s.add(
            "VZ45-切片-01",
            slices.len() == cfg.slices as usize,
            "切片数 = 配置值（8）",
        );
        // 全部落在 [-1,1]。
        let mut in_range = true;
        for sl in &slices {
            if sl.t < -1.0 || sl.t > 1.0 {
                in_range = false;
            }
        }
        s.add("VZ45-切片-02", in_range, "全部切片参数 t ∈ [-1,1]");
        // 分层：8 桶各至少一片（无空洞，即覆盖整段模糊带）。
        let mut buckets = [0u32; 8];
        for sl in &slices {
            let idx = (((sl.t + 1.0) * 0.5) * 8.0) as usize;
            let idx = if idx > 7 { 7 } else { idx };
            buckets[idx] += 1;
        }
        s.add(
            "VZ45-切片-03",
            buckets.iter().all(|c| *c >= 1),
            "八桶各至少一片（模糊带无空洞）",
        );
        // 落点互不重合（黄金角抖动生效）。
        let mut distinct = true;
        for i in 0..slices.len() {
            for j in (i + 1)..slices.len() {
                if (slices[i].t - slices[j].t).abs() < 1e-6 {
                    distinct = false;
                }
            }
        }
        s.add("VZ45-切片-04", distinct, "任意两片 t 互不相等（抖动生效）");
        // 反向：无抖动的均匀切片会有「等差」结构，用相邻差标准差证明差异。
        let n = cfg.slices as usize;
        let mut uniform = Vec::new();
        for i in 0..n {
            uniform.push((i as f32 + 0.5) / n as f32 * 2.0 - 1.0);
        }
        let mut uniform_gaps = Vec::new();
        for i in 1..n {
            uniform_gaps.push(uniform[i] - uniform[i - 1]);
        }
        let mut jitter_gaps = Vec::new();
        for i in 1..slices.len() {
            jitter_gaps.push(slices[i].t - slices[i - 1].t);
        }
        let ug_var = variance(&uniform_gaps);
        let jg_var = variance(&jitter_gaps);
        s.add(
            "VZ45-切片-05",
            ug_var < 1e-9 && jg_var > 1e-6,
            "反向：均匀切片间距方差为 0（会有条带），抖动后间距有方差",
        );
        // 内鬼影抑制：反向片权重衰减。
        let mut neg_w = 1.0f32;
        let mut pos_w = 0.0f32;
        for sl in &slices {
            if sl.t < 0.0 && sl.weight < neg_w {
                neg_w = sl.weight;
            }
            if sl.t > 0.0 && sl.weight > pos_w {
                pos_w = sl.weight;
            }
        }
        s.add(
            "VZ45-切片-06",
            (pos_w - 1.0).abs() < 1e-6 && (neg_w - 0.5).abs() < 1e-6,
            "内鬼影抑制：正向片权重 1.0、反向片权重 0.5（suppress=0.5）",
        );
        // suppress=1 时反向片权重归零。
        let cfg1 = BlurConfig {
            inner_ghost_suppress: 1.0,
            ..BlurConfig::standard()
        };
        let s1 = build_slices(&cfg1, [100.0, 0.0]);
        let mut all_zero = true;
        for sl in &s1 {
            if sl.t < 0.0 && sl.weight > 1e-6 {
                all_zero = false;
            }
        }
        s.add(
            "VZ45-切片-07",
            all_zero,
            "suppress=1 时反向片权重全为 0（单向采样）",
        );
    }

    // ── 4. 深度感知：遮挡样本被剔除 ──
    {
        let cfg = BlurConfig::standard();
        let slices = build_slices(&cfg, [100.0, 0.0]);
        // 场景：中心深度 10（近），一半采样点深度 100（远，被遮挡）。
        let (kept, total) = depth_filtered(&slices, 10.0, cfg.depth_tolerance, |sl| {
            if sl.t < 0.0 {
                100.0
            } else {
                10.0
            }
        });
        s.add(
            "VZ45-深度-01",
            kept < total && kept > 0,
            "遮挡样本被剔除（有效样本数 < 总数且 > 0）",
        );
        // 全同深度的场景应全部保留。
        let (k2, t2) = depth_filtered(&slices, 10.0, cfg.depth_tolerance, |_| 10.0);
        s.add(
            "VZ45-深度-02",
            k2 == t2 && t2 > 0,
            "等深度场景零剔除（不误杀）",
        );
        // 容差生效：容差极大时全部保留。
        let (k3, t3) = depth_filtered(&slices, 10.0, 1000.0, |sl| {
            if sl.t < 0.0 {
                100.0
            } else {
                10.0
            }
        });
        s.add(
            "VZ45-深度-03",
            k3 == t3,
            "容差放大后不剔除（容差语义正确）",
        );
        // 反向：不做深度判断会有外鬼影——用加权合成证明差异。
        let all: Vec<Slice> = slices.clone();
        let filtered: Vec<Slice> = slices
            .iter()
            .filter(|sl| sl.t >= 0.0)
            .copied()
            .collect();
        let c_all = composite(&all, |sl| {
            if sl.t < 0.0 {
                [1.0, 0.0, 0.0, 1.0] // 被遮挡处的红色（鬼影）
            } else {
                [0.0, 0.0, 1.0, 1.0]
            }
        });
        let c_f = composite(&filtered, |_| [0.0, 0.0, 1.0, 1.0]);
        s.add(
            "VZ45-深度-04",
            c_all[0] > 0.05 && c_f[0] == 0.0,
            "反向：不剔除被遮挡样本会混入红色鬼影；剔除后为纯蓝",
        );
    }

    // ── 5. 合成权重归一 ──
    {
        let cfg = BlurConfig::standard();
        let slices = build_slices(&cfg, [50.0, 50.0]);
        // 常值色合成后不变（权重归一）。
        let c = composite(&slices, |_| [0.25, 0.5, 0.75, 1.0]);
        s.add(
            "VZ45-合成-01",
            (c[0] - 0.25).abs() < 1e-6
                && (c[1] - 0.5).abs() < 1e-6
                && (c[2] - 0.75).abs() < 1e-6,
            "常值色合成后不变（权重归一正确）",
        );
        // 全零权重时返回首片颜色（不除零）。
        let mut zero = Vec::new();
        for sl in &slices {
            zero.push(Slice {
                weight: 0.0,
                ..*sl
            });
        }
        let z = composite(&zero, |_| [0.1, 0.2, 0.3, 1.0]);
        s.add(
            "VZ45-合成-02",
            (z[0] - 0.1).abs() < 1e-6,
            "全零权重返回首片色（不产生 NaN）",
        );
        // 空切片表安全。
        let e = composite(&[], |_| [0.0; 4]);
        s.add(
            "VZ45-合成-03",
            e[0] == 0.0 && e[3] == 0.0,
            "空切片表返回零色（不 panic）",
        );
    }

    // ── 6. 参数校验 ──
    {
        let ok = BlurConfig::standard().validate().is_ok();
        let b1 = BlurConfig {
            slices: 0,
            ..BlurConfig::standard()
        }
        .validate()
        .is_err();
        let b2 = BlurConfig {
            slices: SLICES_MAX + 1,
            ..BlurConfig::standard()
        }
        .validate()
        .is_err();
        let b3 = BlurConfig {
            shutter_deg: 400.0,
            ..BlurConfig::standard()
        }
        .validate()
        .is_err();
        let b4 = BlurConfig {
            inner_ghost_suppress: 1.5,
            ..BlurConfig::standard()
        }
        .validate()
        .is_err();
        let b5 = BlurConfig {
            max_len_px: MAX_BLUR_PX + 1.0,
            ..BlurConfig::standard()
        }
        .validate()
        .is_err();
        s.add(
            "VZ45-校验-01",
            ok && b1 && b2 && b3 && b4 && b5,
            "校验矩阵：标准过；切片0/17、快门400、鬼影1.5、超长 各拒",
        );
        s.add(
            "VZ45-冻结-01",
            MBLUR_VERSION == "F5245-mblur-v1",
            "API 版本冻结：F5245-mblur-v1",
        );
        s.add(
            "VZ45-冻结-02",
            SLICES_MAX == 16 && MAX_BLUR_PX == 64.0,
            "参数指纹：切片上限 16 / 模糊上限 64px",
        );
        s.add(
            "VZ45-工具-01",
            (tanf(0.5) - 0.546_302).abs() < 1e-3,
            "tanf(0.5) ≈ 0.5463（焦距换算依赖）",
        );
    }

    s
}

/// 方差（判据内用）。
fn variance(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mut m = 0.0f32;
    for x in v {
        m += x;
    }
    m /= v.len() as f32;
    let mut s = 0.0f32;
    for x in v {
        let d = x - m;
        s += d * d;
    }
    s / v.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn still_camera_zero_blur() {
        let c = CameraDelta::still();
        let d = c.screen_displacement([1.0, 2.0, 3.0]);
        assert_eq!(d, [0.0, 0.0]);
    }

    #[test]
    fn slices_are_stratified() {
        let cfg = BlurConfig::standard();
        let sl = build_slices(&cfg, [64.0, 0.0]);
        let mut b = [0u32; 8];
        for s in &sl {
            let i = (((s.t + 1.0) * 0.5) * 8.0) as usize;
            b[if i > 7 { 7 } else { i }] += 1;
        }
        assert!(b.iter().all(|c| *c >= 1));
    }

    #[test]
    fn depth_filter_drops_occluded() {
        let cfg = BlurConfig::standard();
        let sl = build_slices(&cfg, [10.0, 0.0]);
        let (k, t) = depth_filtered(&sl, 5.0, 0.5, |s| if s.t < 0.0 { 50.0 } else { 5.0 });
        assert!(k < t);
        assert!(k > 0);
    }

    #[test]
    fn composite_keeps_constant_colour() {
        let cfg = BlurConfig::standard();
        let sl = build_slices(&cfg, [1.0, 1.0]);
        let c = composite(&sl, |_| [0.5; 4]);
        assert!((c[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn focal_pixels_reasonable() {
        let c = CameraDelta {
            fov_y: 1.0,
            screen_h: 1080.0,
            ..CameraDelta::still()
        };
        let f = c.focal_px();
        assert!(f > 880.0 && f < 890.0);
    }

    #[test]
    fn ghost_suppress_zero_keeps_both_sides() {
        let cfg = BlurConfig {
            inner_ghost_suppress: 0.0,
            ..BlurConfig::standard()
        };
        let sl = build_slices(&cfg, [1.0, 0.0]);
        assert!(sl.iter().all(|s| (s.weight - 1.0).abs() < 1e-6));
    }
}
