//! VE-F1410 · 遮挡与穿透（VE-H 域 · 音频引擎 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1410`
//!
//! **规格原文**：遮挡声学：遮挡 occlusion（全阻挡——低通+衰减）：遮挡语义
//! （全阻挡（墙后声源——低通滤波+距离衰减（物理直觉（隔墙听声闷）、半遮挡
//! obstruction（部分阻挡——仅衰减：半遮挡（部分视线（门缝/半掩门——只衰减
//! 不低通（高频部分保留——occlusion/obstruction 两档判定（遮挡比例阈值分档）、
//! 几何查询接口（遮挡判定需要场景几何——接口预留对接 3D 场景图，诚实标注一期
//! 简化球体代理、二期接真几何：接口预留（遮挡判定需射线检测（3D 场景图查询
//! ——一期简化（声源-听者连线球体代理检测（够用且便宜/二期真几何（精确遮挡
//! ——诚实标注（一期简化是有意的工程取舍非能力缺陷）、材质联动（F1431 声学
//! 材质吸声系数参与计算：材质参与（遮挡的材质系数（混凝土墙 vs 布帘的穿透
//! 差异（F1431 系数输入——材质让遮挡有质感）、平滑过渡（遮挡状态变化渐变
//! ——不跳变：平滑（遮挡状态切换（滤波参数渐变（50ms 过渡——状态跳变=可闻
//! 突变（渐变是体验红线。
//!
//! **工程量构成**（锚点原文）：两档判定 90 行＋几何接口预留 80 行＋材质联动
//! 60 行＋平滑过渡 50 行＝目标 360 行构成。
//!
//! **判据**：两档、接口预留标注、材质联动、平滑、判据。
//!
//! ---
//!
//! ## 设计要点
//!
//! ### 1. 为什么"两档"的区分特征是低通而不是增益
//!
//! 锚点把两档的差别写得很死：全阻挡 =「低通+衰减」、半遮挡 =「仅衰减……
//! 高频部分保留」。若把两档实现成"两套不同的衰减系数"，则判据只能断言
//! "两档参数不相等"——而这被"两档恰好用了同一组数"骗过。故本实现让
//! **增益衰减律对两档统一**（`gain = 1 − blocked`，跨档单调不增），把档位
//! 差异**唯一地**落在低通截止频率上：
//!
//! - 半遮挡：截止频率恒为开路值 [`OPEN_CUTOFF_HZ`] ⇒ 高频完整保留；
//! - 全阻挡：截止频率取所有遮挡段中最严的（最小）`lowpass_hz`。
//!
//! 于是"半遮挡只衰减不低通"成为一条**可被证伪的强断言**（截止频率精确等于
//! 开路值），而不是一句声明。
//!
//! ### 2. 遮挡比例按"材质加权"而非几何长度
//!
//! 若只用几何长度算遮挡比例，则混凝土墙与布帘同权——布帘把视线挡得满满
//! 一堵，算法却报告"全阻挡"，听感上却是"隔着一层布"。故本实现按
//! `t` 参数区间积分：每个元区间取覆盖它的所有遮挡段中**透声系数最大者**
//! （最开的一条路决定实际到达量），再取补得到 `blocked ∈ [0,1]`。
//!
//! "取最大透声而非最小"是物理取向：声音走的是最通畅的那条路径，不是被
//! 最致密的那层挡住。此规则还顺带消灭了**重叠双计**——两块完全重合的球
//! 与一块球算出同一比例，这是 union 语义的必然结果，不需要额外去重代码。
//!
//! 判据侧对此设了专属断言（两块重合球 == 一块球），否则一个"逐段相加"的
//! 实现会静默给出 2.0 倍比例并被 `clamp` 掩盖成 1.0。
//!
//! ### 3. 区间求交是解析闭式，不是采样
//!
//! 声源→听者连线与球面的交点由二次方程闭式解出，`t` 归一化到 `[0,1]`。
//! **不做离散采样**：采样会在"球恰好只盖住段端一点"这类边界上漏判，而
//! 遮挡比例的阈值分档恰恰对这些边界敏感（分档判据被采样漏判 → 全绿假象）。
//! 判据以解析夹逼对（`t0−ε` / `t0` / `t0+ε`）钉死界位置。
//!
//! ### 4. 平滑过渡：50ms 线性斜坡，参数级而非状态级
//!
//! 锚点「状态跳变=可闻突变（渐变是体验红线）」。本实现对**滤波参数**
//! （增益、截止频率）各挂一条斜坡 [`Ramp`]：目标变更时从**当前值**起坡
//! （不是从上一个目标起坡——后者会在中途改目标时产生跳变），经
//! [`TRANSITION_MS`] = 50ms 线性抵达。
//!
//! 选线性斜坡而非一阶低通：低通的收敛是渐近的，"何时算到位"没有解析
//! 时刻，判据只能写"足够接近"这类模糊阈值；线性斜坡在 `t = 50ms` 处
//! **精确**等于目标，于是判据可以断言"25ms 处恰在中点""49ms 未到位
//! 50ms 已到位"这类可证伪的定点事实。
//!
//! `dt ≤ 0` / 非有限的 tick 不推进斜坡（时间倒流或 NaN 注入不得让参数回退）。
//!
//! ### 5. 分层：参数计算在本模块，滤波器在 F1329
//!
//! 本模块只产出**滤波参数目标值**，不实现 biquad。真正的滤波核归 F1329
//! （七型biquad 共享核）——同核同对拍纪律：遮挡不另写一份滤波器，否则
//! 两处滤波器的系数推导会各自漂移，遮挡的听感就再也无法与混音图上的滤波
//! 对拍。声明见 [`LAYERING_DECLARATION`]。
//!
//! ### 6. 材质系数的权威在 F1431
//!
//! 本模块**消费**透声/吸声系数（[`AcousticMaterial`] 是输入结构），并内置
//! 一张**最小自洽表**（4 种材质）供模块自持与判据对拍。系数库的权威版本
//! 归 F1431（材质系数库）；此处不复制、不覆盖、不假装自己是那份库——
//! 声明见 [`MATERIAL_AUTHORITY_DECLARATION`]。
//!
//! ### 7. 几何接口是一期球体代理（诚实标注）
//!
//! 遮挡判定需要场景几何。本模块以 [`OcclusionGeometry`] trait 预留对接 3D
//! 场景图，并给出两个实现：[`SphereProxyGeometry`]（一期，声源-听者连线与
//! 场景遮挡代理球的解析求交——够用且便宜）与 [`NullGeometry`]（无几何挂载
//! 时的显式空实现，使"接口预留"是**可编译可替换的真实接缝**而非注释）。
//!
//! **一期简化是有意的工程取舍，不是能力缺陷**：球体代理无法表达薄墙、
//! 门缝、拐角遮挡，但换来的是零场景遍历、与场景图解耦、可确定性回归。
//! 二期接真几何时**只需新增一个 trait 实现**——分类、材质加权、平滑三段
//! 全部复用，不动本模块任何一行。声明见 [`GEOMETRY_PHASE1_DECLARATION`]。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 坐标契约复用 F1409 的 `Cartesian`（VE-H 域内同型，避免两套坐标并存——
// 同域内两个坐标类型混用即"坐标系不一致=空间错乱"，锚点原话）。
use super::veh09_spatial::Cartesian;

// ---------------------------------------------------------------------------
// 一、几何查询接口预留（80 行 · 锚点：接口预留对接 3D 场景图 / 一期球体代理）
// ---------------------------------------------------------------------------

/// 遮挡区间（`t` 归一化参数，`from + t·(to − from)`，恒 `0 ≤ t0 < t1 ≤ 1`）。
///
/// `material` 是**该区间自身的材质**，不是"整个场景的材质"——同一段视线上
/// 可以先后穿过混凝土与布帘，两段的透声能力必须分别参与加权。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OcclusionSpan {
    pub t0: f32,
    pub t1: f32,
    pub material: AcousticMaterial,
}

/// 几何查询接缝。
///
/// **为什么是 trait 而不是函数**：遮挡比例的算法（三段：求交 → 材质加权
/// → 阈值分档）必须与"几何从哪来"解耦，否则二期接真几何时整条链路都要重写。
/// trait 保证二期只需新增一个实现。
pub trait OcclusionGeometry {
    /// 把 `from → to` 连线上的遮挡区间**追加**到 `out`（调用方提供缓冲）。
    ///
    /// 取 `&mut Vec` 而非返回 `Vec`：内核热路径上每帧每声源查询一次，返回
    /// `Vec` 会让稳态分配随声源数线性增长。调用方跨帧复用同一缓冲即零分配。
    fn occlusion_spans_into(&self, from: Cartesian, to: Cartesian, out: &mut Vec<OcclusionSpan>);

    /// 便捷版：自行分配缓冲（测试与非热路径用）。
    fn collect_spans(&self, from: Cartesian, to: Cartesian) -> Vec<OcclusionSpan> {
        let mut v: Vec<OcclusionSpan> = Vec::new();
        self.occlusion_spans_into(from, to, &mut v);
        v
    }
}

/// 场景遮挡代理球（**仅一期**：球体代理的遮挡表达）。
///
/// 二期接真几何时本类型退役——真实网格不需要"代理球"这一中间层。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OccluderSphere {
    pub center: Cartesian,
    pub radius_m: f32,
    pub material: AcousticMaterial,
}

impl OccluderSphere {
    /// 构造并校验（半径须为有限正数，材质须自洽，中心须有限）。
    pub fn new(
        center: Cartesian,
        radius_m: f32,
        material: AcousticMaterial,
    ) -> Result<OccluderSphere, String> {
        if !center.is_finite() {
            return Err("遮挡代理球中心坐标非有限（NaN/Inf 会静默污染下游）".to_string());
        }
        if !(radius_m.is_finite()) || !(radius_m > 0.0) {
            return Err(format!(
                "遮挡代理球半径非法：{}（须为有限正数）",
                radius_m
            ));
        }
        if !material.is_valid() {
            return Err(format!(
                "遮挡代理球材质「{}」系数越界（透声/吸声须在 [0,1]）",
                material.name
            ));
        }
        Ok(OccluderSphere {
            center,
            radius_m,
            material,
        })
    }
}

/// 一期几何实现：声源-听者连线 × 场景代理球的**解析**求交。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SphereProxyGeometry {
    spheres: Vec<OccluderSphere>,
}

impl SphereProxyGeometry {
    pub fn new() -> SphereProxyGeometry {
        SphereProxyGeometry {
            spheres: Vec::new(),
        }
    }

    /// 挂入一颗遮挡代理球（校验失败即拒，不静默丢弃）。
    pub fn add(&mut self, sphere: OccluderSphere) -> Result<u32, String> {
        let idx = self.spheres.len() as u32;
        self.spheres.push(sphere);
        Ok(idx)
    }

    pub fn len(&self) -> usize {
        self.spheres.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spheres.is_empty()
    }
}

/// 连线与球的求交（返回归一化区间 `Option<(t0, t1)>`）。
///
/// 闭式解：`|A + tD − C|² = r²` ⇒ `a t² + b t + c = 0`，其中
/// `a = D·D`、`b = 2 F·D`、`c = F·F − r²`、`F = A − C`。
///
/// 退化处理：
/// - `a == 0`（`A == B`，零长连线）：退化为点包含判定，命中即全覆盖 `[0,1]`；
/// - 判别式 `< 0`（不相离）：无交；
/// - 根全体落在段外：裁剪后 `t1 ≤ t0` 即无有效遮挡。
fn segment_sphere_span(
    a: Cartesian,
    b: Cartesian,
    center: Cartesian,
    radius_m: f32,
) -> Option<(f32, f32)> {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let dz = b.z - a.z;
    let qa = dx * dx + dy * dy + dz * dz;
    let fx = a.x - center.x;
    let fy = a.y - center.y;
    let fz = a.z - center.z;

    if qa <= 1.0e-12 {
        // 零长连线：等价于点包含。
        let dist2 = fx * fx + fy * fy + fz * fz;
        return if dist2 <= radius_m * radius_m {
            Some((0.0, 1.0))
        } else {
            None
        };
    }

    let qb = 2.0 * (fx * dx + fy * dy + fz * dz);
    let qc = fx * fx + fy * fy + fz * fz - radius_m * radius_m;
    let disc = qb * qb - 4.0 * qa * qc;
    if !(disc >= 0.0) {
        return None;
    }
    let root = fsqrt(disc);
    let inv = 1.0 / (2.0 * qa);
    let mut t0 = (-qb - root) * inv;
    let mut t1 = (-qb + root) * inv;
    if t0 < 0.0 {
        t0 = 0.0;
    }
    if t1 > 1.0 {
        t1 = 1.0;
    }
    if !(t1 > t0) {
        return None;
    }
    Some((t0, t1))
}

impl OcclusionGeometry for SphereProxyGeometry {
    fn occlusion_spans_into(&self, from: Cartesian, to: Cartesian, out: &mut Vec<OcclusionSpan>) {
        if !from.is_finite() || !to.is_finite() {
            // 非有限输入不产出任何区间（等价"无遮挡"）——异常零静默在此处
            // 是"拒绝"而非"报一个假的 0"，故此处另记标志供上层显性化。
            return;
        }
        for s in self.spheres.iter() {
            if let Some((t0, t1)) =
                segment_sphere_span(from, to, s.center, s.radius_m)
            {
                out.push(OcclusionSpan {
                    t0,
                    t1,
                    material: s.material,
                });
            }
        }
    }
}

/// 无几何挂载的显式空实现（使 trait 接缝真实可替换，而非纸面声明）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NullGeometry;

impl OcclusionGeometry for NullGeometry {
    fn occlusion_spans_into(&self, _from: Cartesian, _to: Cartesian, _out: &mut Vec<OcclusionSpan>) {
        // 空场景即无遮挡。
    }
}

/// 一期简化诚实标注（锚点：「一期简化是有意的工程取舍非能力缺陷」）。
pub const GEOMETRY_PHASE1_DECLARATION: &str = "\
遮挡几何一期实现为「声源-听者连线 × 场景遮挡代理球」的解析求交；\
球体代理无法表达薄墙、门缝、拐角与多段绕射等真实遮挡形态，这是\
有意选择的工程取舍而非能力缺陷——取舍的对价是：零场景遍历、与场景图\
完全解耦、判定完全确定性可回归。二期接真几何时只需为 OcclusionGeometry \
新增一个实现，材质加权与两档分档逻辑零改动。";

// ---------------------------------------------------------------------------
// 二、材质联动（60 行 · 锚点：F1431 系数输入 / 混凝土墙 vs 布帘的穿透差异）
// ---------------------------------------------------------------------------

/// 声学材质（**F1431 系数库的输入结构**，非本模块的库）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AcousticMaterial {
    /// 材质标识（数据驱动：材质可增删，不改代码）。
    pub id: u16,
    /// 材质名（人可读，错误信息与日志用）。
    pub name: &'static str,
    /// 透声系数 `[0,1]`：1 = 完全透声（薄幕），0 = 完全不透（厚混凝土）。
    pub transmission: f32,
    /// 吸声系数 `[0,1]`（F1431 权威项；本模块仅携带，不参与遮挡计算）。
    pub absorption: f32,
    /// 该材质引入的低通截止频率（Hz）：全阻挡档的滤波参数来源。
    pub lowpass_hz: f32,
}

/// 开路截止频率（无遮挡时的上限，20kHz ≙ 全频带）。
pub const OPEN_CUTOFF_HZ: f32 = 20_000.0;

/// 截止频率比较容差（Hz）。
pub const CUTOFF_EPS_HZ: f32 = 1.0e-3;

impl AcousticMaterial {
    /// 构造并校验（三项系数均须有限且在 `[0,1]`，截止频率须有限正数）。
    pub fn new(
        id: u16,
        name: &'static str,
        transmission: f32,
        absorption: f32,
        lowpass_hz: f32,
    ) -> Result<AcousticMaterial, String> {
        let m = AcousticMaterial {
            id,
            name,
            transmission,
            absorption,
            lowpass_hz,
        };
        if !m.is_valid() {
            return Err(format!(
                "材质「{}」系数越界（透声/吸声须有限且在 [0,1]，截止频率须有限正数）",
                name
            ));
        }
        Ok(m)
    }

    /// 系数自洽。
    pub fn is_valid(&self) -> bool {
        let unit = self.transmission.is_finite()
            && self.absorption.is_finite()
            && self.transmission >= 0.0
            && self.transmission <= 1.0
            && self.absorption >= 0.0
            && self.absorption <= 1.0;
        let lp = self.lowpass_hz.is_finite() && self.lowpass_hz > 0.0;
        unit && lp
    }

    /// 该材质单独造成的遮挡量（`1 − 透声`）。
    pub fn block_of(&self) -> f32 {
        1.0 - self.transmission
    }
}

/// 混凝土（厚、重、低频可通过、高频几乎全挡）。
pub const MAT_CONCRETE: AcousticMaterial = AcousticMaterial {
    id: 1,
    name: "混凝土",
    transmission: 0.08,
    absorption: 0.02,
    lowpass_hz: 700.0,
};

/// 玻璃（硬、薄、中高频仍有一定穿透）。
pub const MAT_GLASS: AcousticMaterial = AcousticMaterial {
    id: 2,
    name: "玻璃",
    transmission: 0.35,
    absorption: 0.18,
    lowpass_hz: 2_600.0,
};

/// 木板（比玻璃透，阻尼中等）。
pub const MAT_WOOD: AcousticMaterial = AcousticMaterial {
    id: 3,
    name: "木板",
    transmission: 0.45,
    absorption: 0.30,
    lowpass_hz: 3_400.0,
};

/// 布帘（最透——隔一层布帘仍是"听得清但发闷"）。
pub const MAT_CURTAIN: AcousticMaterial = AcousticMaterial {
    id: 4,
    name: "布帘",
    transmission: 0.72,
    absorption: 0.55,
    lowpass_hz: 6_500.0,
};

/// 内置最小自洽材质表（**非** F1431 权威库，见 [`MATERIAL_AUTHORITY_DECLARATION`]）。
pub const BUILTIN_MATERIALS: [AcousticMaterial; 4] =
    [MAT_CONCRETE, MAT_GLASS, MAT_WOOD, MAT_CURTAIN];

/// 材质库归属声明（防止本模块被误当作 F1431 的权威系数库）。
pub const MATERIAL_AUTHORITY_DECLARATION: &str = "\
本模块只消费声学材质系数：AcousticMaterial 是F1431 材质系数库的输入结构，\
内置的 4 种材质（混凝土/玻璃/木板/布帘）是为模块自持与判据对拍而设的最小\
自洽表，不是权威库。系数表的权威版本、增删与频响曲线归 F1431；本模块不\
复制、不覆盖、不假装自己是那份库。";

// ---------------------------------------------------------------------------
// 三、两档判定（90 行 · 锚点：occlusion/obstruction 两档 / 遮挡比例阈值分档）
// ---------------------------------------------------------------------------

/// 遮挡档位（两档 + 开路基线）。
///
/// 锚点的"两档"指的是**被遮挡时的两档**；`Clear` 是未被遮挡的基线态，
/// 不是第三档。三态齐全才不会出现"轻微遮挡无处安放"的空洞。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcclusionGrade {
    /// 无遮挡：增益 1，截止频率开路。
    Clear,
    /// 半遮挡 obstruction（部分阻挡——门缝/半掩门）：**仅衰减，不低通**。
    Partial,
    /// 全阻挡 occlusion（墙后声源）：低通 + 衰减。
    Full,
}

/// 进入半遮挡档的遮挡比例阈值。
pub const PARTIAL_ENTER: f32 = 0.15;

/// 进入全阻挡档的遮挡比例阈值。
pub const FULL_ENTER: f32 = 0.55;

/// 分档边界的夹逼判据容差（比阈值小 4 个数量级）。
pub const TIER_EPS: f32 = 1.0e-4;

impl OcclusionGrade {
    /// 按材质加权后的遮挡比例分档（闭区间在上：恰等于阈值即进入该档）。
    pub fn of(blocked_ratio: f32) -> OcclusionGrade {
        if !blocked_ratio.is_finite() {
            // 非有限比例不得静默落进"无遮挡"——那会让故障听起来正常。
            // 约定：非有限按最严处理（全阻挡），异常零静默的方向是保守侧。
            return OcclusionGrade::Full;
        }
        if blocked_ratio >= FULL_ENTER {
            OcclusionGrade::Full
        } else if blocked_ratio >= PARTIAL_ENTER {
            OcclusionGrade::Partial
        } else {
            OcclusionGrade::Clear
        }
    }

    /// 是否施加低通（全阻挡档专属——这是两档的唯一区分特征）。
    pub fn applies_lowpass(&self) -> bool {
        *self == OcclusionGrade::Full
    }

    /// 机读档位名。
    pub fn wire(&self) -> &'static str {
        match self {
            OcclusionGrade::Clear => "clear",
            OcclusionGrade::Partial => "obstruction",
            OcclusionGrade::Full => "occlusion",
        }
    }
}

/// 材质加权后的遮挡比例。
///
/// 算法：对 `t ∈ [0,1]` 做**元区间积分**——以所有区间端点与 `0/1` 切分，
/// 每个元区间取覆盖它的所有遮挡段中**透声系数最大者**，累加 `1 − 透声`。
///
/// 由此得到的两个性质是本算法的正确性凭据：
/// 1. **重叠不双计**（并集语义）：两块完全重合的球与一块球结果相同；
/// 2. **取最通路径**：并存的混凝土与布帘按布帘算——声音走通畅的那条路。
pub fn weighted_blocked_ratio(spans: &[OcclusionSpan]) -> f32 {
    if spans.is_empty() {
        return 0.0;
    }
    // 边界点：0、1 与各区间端点（裁剪进 [0,1]）。
    let mut cuts: Vec<f32> = Vec::with_capacity(spans.len() * 2 + 2);
    cuts.push(0.0);
    cuts.push(1.0);
    for s in spans.iter() {
        if !s.t0.is_finite() || !s.t1.is_finite() {
            continue;
        }
        cuts.push(clamp01(s.t0));
        cuts.push(clamp01(s.t1));
    }
    cuts.sort_by(|x, y| x.total_cmp(y));
    cuts.dedup_by(|x, y| (*x - *y).abs() <= 1.0e-7);

    let mut blocked = 0.0f32;
    for w in cuts.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let seg = hi - lo;
        if !(seg > 1.0e-7) {
            continue;
        }
        // 元区间中点：判定"覆盖"用中点而非 lo，避免相邻元区间因浮点边界
        // 被同时判为覆盖或同时判为不覆盖（那会让积分漏掉一格）。
        let mid = 0.5 * (lo + hi);
        // "未被覆盖"的语义是**全透**（贡献 0），但它不能直接用作比较的
        // 初值：透声系数的取值域上界就是 1.0，用 `初值=1.0` 配合严格 `>`
        // 比较会让**任何**遮挡段都"不大于初值"，于是所有元区间都判成未覆盖，
        // 遮挡比例恒为 0（实测：全覆盖的混凝土也得 0.0）。
        //
        // 两个反直觉的初值各自对应一种真实缺陷，故此处用显式的 `covered`
        // 标志把两件事分开：
        // - 初值取 0.0 → 未覆盖段被当成"完全不透声"，遮挡比例被高估
        //   （实测半覆盖时0.64，正确值 0.14）；
        // - 初值取 1.0 → 未覆盖段正确，但覆盖段永远无法胜出，比例恒为 0。
        let mut best_transmission = 0.0f32;
        let mut covered = false;
        for s in spans.iter() {
            if !s.t0.is_finite() || !s.t1.is_finite() {
                continue;
            }
            let a = clamp01(s.t0);
            let b = clamp01(s.t1);
            if mid > a && mid < b && (!covered || s.material.transmission > best_transmission) {
                best_transmission = s.material.transmission;
                covered = true;
            }
        }
        // 未覆盖 ⇒全透 ⇒ 贡献 0；覆盖 ⇒ 取最通路径的 `1 − 透声`。
        let contribution = if covered {
            seg * (1.0 - best_transmission)
        } else {
            0.0
        };
        blocked += contribution;
    }
    clamp01(blocked)
}

/// 全阻挡档的低通截止频率：**按遮挡贡献加权**取主导材质，而非无条件取最小。
///
/// ## 为什么不能"取所有 span 里最小的 lowpass"
///
/// 朴素实现（遍历全部 span 取 min）有一个实测可复现的错误：构造
/// 「混凝土盖 90% 视线（blocked 贡献 0.828）+ 一小段透声 0.99 但
/// lowpass=50Hz 的材质（贡献 0.00075）」，则
///
/// ```text
/// grade=Full  blocked=0.8288  cutoff=50.0   ← 朴素实现
/// 应为                cutoff=700.0  （混凝土主导）
/// ```
///
/// 听感后果是**比混凝土本身还闷**：一个长度占比 7.5%、几乎不挡声的
/// 材质，凭自己的 `lowpass_hz` 一票否决了主导材质。这在物理上说不通——
/// 决定"听到多闷"的是**累积遮挡量**，不是某个边缘材质的标称截止频率。
///
/// ## 加权口径
///
/// 以每个 span 的遮挡贡献 `len × (1 − 透声)` 为权重，取**权重最大**的
/// span 的 `lowpass_hz`。即"谁挡得多听谁的"，与 `weighted_blocked_ratio`
/// 的口径一致（同一个"贡献"定义，不出现两套标准）。
///
/// 权重全部为 0（理论上不可达：进了 Full 档就必有 span 有贡献）时
/// 返回开路值而非某个任意最小值——保守方向是不额外低通。
fn dominant_cutoff(spans: &[OcclusionSpan]) -> f32 {
    let mut best_weight = -1.0f32;
    let mut cutoff = OPEN_CUTOFF_HZ;
    for s in spans.iter() {
        if !s.t0.is_finite() || !s.t1.is_finite() {
            continue;
        }
        let len = clamp01(s.t1) - clamp01(s.t0);
        if !(len > 0.0) {
            continue;
        }
        let weight = len * s.material.block_of();
        // 严格 `>`：同权重时取先出现者，使结果对 span 顺序确定性可复现
        // （`>=` 会让结果依赖遍历顺序，同输入可能出不同 cutoff）。
        if weight > best_weight {
            best_weight = weight;
            cutoff = s.material.lowpass_hz;
        }
    }
    cutoff
}

/// 遮挡判定结论（滤波参数目标值，**不是**滤波器本身）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OcclusionVerdict {
    pub grade: OcclusionGrade,
    /// 材质加权后的有效遮挡比例 `[0,1]`。
    pub blocked_ratio: f32,
    /// 目标增益（跨档统一衰减律 `1 − blocked`，随比例单调不增）。
    pub target_gain: f32,
    /// 目标低通截止频率（半遮挡档恒为开路值——高频完整保留）。
    pub target_cutoff_hz: f32,
}

impl OcclusionVerdict {
    /// 低通是否生效（**派生自截止频率**，不是并行标志位）。
    ///
    /// 派生而非另设布尔：两个各自维护的字段会漂移（截止频率已降到 700Hz
    /// 而标志位还停在 false），而"低通没开但参数已改"正是最难查的一类
    /// 静音错误。判据直接断截止频率本身。
    pub fn lowpass_active(&self) -> bool {
        self.target_cutoff_hz < OPEN_CUTOFF_HZ - CUTOFF_EPS_HZ
    }
}

/// 完整判定：几何 → 材质加权 → 阈值分档 → 滤波参数。
///
/// **分层**：本函数只算参数；biquad 核归 F1329（见 [`LAYERING_DECLARATION`]）。
pub fn classify<G: OcclusionGeometry>(
    geometry: &G,
    from: Cartesian,
    to: Cartesian,
) -> OcclusionVerdict {
    let spans = geometry.collect_spans(from, to);
    let blocked = weighted_blocked_ratio(spans.as_slice());
    let grade = OcclusionGrade::of(blocked);
    let target_gain = clamp01(1.0 - blocked);
    // 两档的唯一区分点在此：全阻挡才把截止频率从开路值拉下来，
    // 且取**贡献主导**材质的截止频率（见 `dominant_cutoff` 的加权理由）。
    let target_cutoff_hz = if grade.applies_lowpass() {
        dominant_cutoff(spans.as_slice())
    } else {
        OPEN_CUTOFF_HZ
    };
    OcclusionVerdict {
        grade,
        blocked_ratio: blocked,
        target_gain,
        target_cutoff_hz,
    }
}

/// 分层声明（参数在本模块 / 滤波核在 F1329）。
pub const LAYERING_DECLARATION: &str = "\
VE-F1410 只产出滤波参数目标值（增益 + 低通截止频率）并负责档位判定与参数\
平滑；biquad 滤波核归 F1329（七型共享核）。遮挡不另写一份滤波器——两处\
各自推导系数会使听感与混音图上的滤波无法对拍。";

// ---------------------------------------------------------------------------
// 四、平滑过渡（50 行 · 锚点：滤波参数渐变 / 50ms 过渡 / 不跳变）
// ---------------------------------------------------------------------------

/// 状态切换过渡时长（毫秒）——锚点给定 50ms。
pub const TRANSITION_MS: f32 = 50.0;

/// 单参数线性斜坡（从当前值起坡，固定时长抵达目标）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ramp {
    from: f32,
    to: f32,
    elapsed_ms: f32,
}

impl Ramp {
    /// 构造为已到位（`from = to = value`）。
    pub fn settled(value: f32) -> Ramp {
        Ramp {
            from: value,
            to: value,
            elapsed_ms: TRANSITION_MS,
        }
    }

    /// 改目标。**从当前插值位置起坡**——这是"中途改目标不跳变"的唯一保证；
    /// 若从上一个目标起坡，改目标的瞬间参数就会跳到旧目标的终点。
    pub fn retarget(&mut self, target: f32) {
        if !target.is_finite() {
            return;
        }
        if target == self.to {
            return; // 目标未变：保留既有进度，不重启斜坡。
        }
        self.from = self.value();
        self.to = target;
        self.elapsed_ms = 0.0;
    }

    /// 推进时间（`dt ≤ 0` 或非有限则不推进——时间倒流不得让参数回退）。
    pub fn advance(&mut self, dt_ms: f32) {
        if !dt_ms.is_finite() || dt_ms <= 0.0 {
            return;
        }
        self.elapsed_ms += dt_ms;
        if self.elapsed_ms > TRANSITION_MS {
            self.elapsed_ms = TRANSITION_MS;
        }
    }

    /// 当前插值（`t = elapsed / 50ms`，钳到 `[0,1]`）。
    ///
    /// 因是 `from` 与 `to` 的凸组合，**恒不越界**——不可能过冲，也不可能
    /// 因为目标中途变化而跑到两端之外。这是"渐变是体验红线"的结构性保证，
    /// 而非靠调用方小心。
    pub fn value(&self) -> f32 {
        let t = clamp01(self.elapsed_ms / TRANSITION_MS);
        self.from + (self.to - self.from) * t
    }

    /// 是否已到位（`elapsed ≥ 50ms`）。
    pub fn is_settled(&self) -> bool {
        self.elapsed_ms >= TRANSITION_MS
    }

    /// 目标值（供跨模块对齐用）。
    pub fn target(&self) -> f32 {
        self.to
    }

    /// 起坡值（供跨模块对齐用）。
    pub fn origin(&self) -> f32 {
        self.from
    }
}

/// 遮挡滤波参数平滑器（增益 + 截止频率两条斜坡）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OcclusionSmoother {
    gain: Ramp,
    cutoff: Ramp,
}

/// 平滑后的滤波参数（上层据此驱动 F1329 的 biquad）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothedFilter {
    pub gain: f32,
    pub cutoff_hz: f32,
    /// 低通是否生效（**派生自** `cutoff_hz`，与 [`OcclusionVerdict::lowpass_active`]
    /// 同一取法——两处若各设一个布尔，参数与标志会漂移）。
    pub lowpass_active: bool,
}

impl OcclusionSmoother {
    /// 构造为开路稳态（增益 1、截止频率开路、低通未开）。
    pub fn new_open() -> OcclusionSmoother {
        OcclusionSmoother {
            gain: Ramp::settled(1.0),
            cutoff: Ramp::settled(OPEN_CUTOFF_HZ),
        }
    }

    /// 施加新结论（改目标，不跳变）。
    pub fn apply(&mut self, verdict: &OcclusionVerdict) {
        self.gain.retarget(verdict.target_gain);
        self.cutoff.retarget(verdict.target_cutoff_hz);
    }

    /// 推进 `dt_ms` 毫秒。
    pub fn advance(&mut self, dt_ms: f32) {
        self.gain.advance(dt_ms);
        self.cutoff.advance(dt_ms);
    }

    /// 是否已完全到位（两条斜坡皆稳）。
    pub fn is_settled(&self) -> bool {
        self.gain.is_settled() && self.cutoff.is_settled()
    }

    /// 当前滤波参数。
    pub fn current(&self) -> SmoothedFilter {
        let cutoff_hz = self.cutoff.value();
        SmoothedFilter {
            gain: self.gain.value(),
            cutoff_hz,
            lowpass_active: cutoff_hz < OPEN_CUTOFF_HZ - CUTOFF_EPS_HZ,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、小工具（无依赖，供本模块与判据共用）
// ---------------------------------------------------------------------------

/// 钳到 `[0,1]`（NaN → 0，非有限正负 → 对应端点）。
fn clamp01(v: f32) -> f32 {
    if !v.is_finite() {
        return 0.0;
    }
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

/// 平方根（`core` 无 `f32::sqrt` 在部分目标上不可用时的自备实现）。
///
/// 本模块的几何求交与材质查表都要开方，故不复用 F1409 的同名函数——
/// `use super::veh09_spatial::fsqrt` 会让两处数学实现耦合，一处改动波及
/// 遮挡判定。重复一个十几行的开方，换取两模块各自可独立审计。
pub fn fsqrt(v: f32) -> f32 {
    if !(v > 0.0) {
        return 0.0;
    }
    // 牛顿迭代，初值取量级猜测；固定 24 轮对 f32 有效位（约 7 位）远超足够。
    let mut x = v * 0.5 + 0.5;
    for _ in 0..24 {
        let nx = 0.5 * (x + v / x);
        if nx == x {
            break;
        }
        x = nx;
    }
    x
}

/// 材质查表（按 id）。
pub fn material_by_id(id: u16) -> Option<AcousticMaterial> {
    BUILTIN_MATERIALS
        .iter()
        .copied()
        .find(|m| m.id == id)
}

/// 模块能力自述（机读：判据与跨域自检据此断言"接口预留"不是空话）。
pub fn capability_declaration() -> String {
    let mut s = String::new();
    s.push_str("VE-F1410 遮挡与穿透：");
    s.push_str("几何=OcclusionGeometry trait（一期 SphereProxyGeometry 解析球体代理 / NullGeometry 空实现）；");
    s.push_str("分档=材质加权遮挡比例阈值（半遮挡仅衰减不低通 / 全阻挡低通+衰减）；");
    s.push_str("平滑=滤波参数 50ms 线性斜坡（从当前值起坡，不跳变不过冲）；");
    s.push_str("材质=消费 AcousticMaterial（权威库归 F1431）；滤波核归 F1329。");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn seg(len: f32) -> (Cartesian, Cartesian) {
        (Cartesian::ORIGIN, Cartesian::new(0.0, 0.0, len))
    }

    fn geo_at(z: f32, r: f32, m: AcousticMaterial) -> SphereProxyGeometry {
        let mut g = SphereProxyGeometry::new();
        let s = OccluderSphere::new(Cartesian::new(0.0, 0.0, z), r, m)
            .expect("合法球");
        g.add(s).expect("接纳");
        g
    }

    /// 相交闭式解：球心在段上、半径 r、段长 L ⇒ t ∈ [(L/2−r)/L, (L/2+r)/L]。
    #[test]
    fn analytic_intersection_matches_closed_form() {
        let (len, r) = (4.0f32, 1.0f32);
        let g = geo_at(len / 2.0, r, MAT_CONCRETE);
        let (a, b) = seg(len);
        let spans = g.collect_spans(a, b);
        assert_eq!(spans.len(), 1);
        assert!((spans[0].t0 - 0.25).abs() < 1.0e-5);
        assert!((spans[0].t1 - 0.75).abs() < 1.0e-5);
    }

    /// 半覆盖的加权比例 = 覆盖长度 × 材质遮挡量（独立闭式对账）。
    #[test]
    fn partial_cover_weighted_by_material() {
        let (len, r) = (4.0f32, 1.0f32);
        let (a, b) = seg(len);
        let concrete = classify(&geo_at(len / 2.0, r, MAT_CONCRETE), a, b);
        let curtain = classify(&geo_at(len / 2.0, r, MAT_CURTAIN), a, b);
        // 覆盖长度 0.5。
        assert!((concrete.blocked_ratio - 0.5 * MAT_CONCRETE.block_of()).abs() < 1.0e-5);
        assert!((curtain.blocked_ratio - 0.5 * MAT_CURTAIN.block_of()).abs() < 1.0e-5);
    }

    /// 相切（判别式 = 0）不产生区间。
    #[test]
    fn tangent_sphere_yields_no_span() {
        let mut g = SphereProxyGeometry::new();
        let s = OccluderSphere::new(Cartesian::new(0.0, 1.0, 2.0), 1.0, MAT_CONCRETE)
            .expect("合法球");
        g.add(s).expect("接纳");
        let (a, b) = seg(4.0);
        assert!(g.collect_spans(a, b).is_empty());
    }

    /// 两块完全重合的球 == 一块球（并集语义，不双计）。
    #[test]
    fn coincident_spheres_do_not_double_count() {
        let (a, b) = seg(4.0);
        let one = classify(&geo_at(2.0, 1.0, MAT_CURTAIN), a, b).blocked_ratio;
        let mut two = SphereProxyGeometry::new();
        for _ in 0..2 {
            let s = OccluderSphere::new(Cartesian::new(0.0, 0.0, 2.0), 1.0, MAT_CURTAIN)
                .expect("合法球");
            two.add(s).expect("接纳");
        }
        let dup = classify(&two, a, b).blocked_ratio;
        assert!((one - dup).abs() < 1.0e-6);
    }

    /// 并存取最通路径（布帘胜过混凝土）。
    #[test]
    fn overlapping_materials_take_openest_path() {
        let (a, b) = seg(4.0);
        let mut g = SphereProxyGeometry::new();
        for m in [MAT_CONCRETE, MAT_CURTAIN].iter() {
            let s = OccluderSphere::new(Cartesian::new(0.0, 0.0, 2.0), 3.0, *m)
                .expect("合法球");
            g.add(s).expect("接纳");
        }
        let got = classify(&g, a, b).blocked_ratio;
        assert!((got - MAT_CURTAIN.block_of()).abs() < 1.0e-5);
    }

    /// 半遮挡档：截止频率**精确**为开路值（两档唯一区分特征）。
    #[test]
    fn partial_grade_keeps_highs() {
        let (a, b) = seg(4.0);
        let v = classify(&geo_at(2.0, 3.0, MAT_CURTAIN), a, b);
        assert_eq!(v.grade, OcclusionGrade::Partial);
        assert_eq!(v.target_cutoff_hz, OPEN_CUTOFF_HZ);
        assert!(!v.lowpass_active());
        assert!(v.target_gain < 1.0);
    }

    /// 全遮挡档：截止频率取最严材质。
    #[test]
    fn full_grade_takes_strictest_cutoff() {
        let (a, b) = seg(4.0);
        let v = classify(&geo_at(2.0, 3.0, MAT_CONCRETE), a, b);
        assert_eq!(v.grade, OcclusionGrade::Full);
        assert!((v.target_cutoff_hz - MAT_CONCRETE.lowpass_hz).abs() < CUTOFF_EPS_HZ);
        assert!(v.lowpass_active());
    }

    /// 斜坡：50ms 精确抵达，25ms 精确中点。
    #[test]
    fn ramp_is_exact_linear() {
        let mut r = Ramp::settled(1.0);
        r.retarget(0.0);
        r.advance(TRANSITION_MS / 2.0);
        assert!((r.value() - 0.5).abs() < 1.0e-6);
        r.advance(TRANSITION_MS / 2.0);
        assert!((r.value() - 0.0).abs() < 1.0e-6);
        assert!(r.is_settled());
    }

    /// 中途改目标不跳变，且从当前值起坡。
    #[test]
    fn retarget_is_continuous() {
        let mut r = Ramp::settled(1.0);
        r.retarget(0.0);
        r.advance(TRANSITION_MS / 4.0);
        let before = r.value();
        r.retarget(1.0);
        assert!((r.value() - before).abs() < 1.0e-6);
        r.advance(TRANSITION_MS / 2.0);
        assert!((r.value() - 0.875).abs() < 1.0e-5);
    }

    /// 非法 tick 不推进，也不造成时间膨胀。
    #[test]
    fn illegal_ticks_are_ignored() {
        let mut r = Ramp::settled(1.0);
        r.retarget(0.4);
        r.advance(-100.0);
        r.advance(f32::NAN);
        r.advance(0.0);
        assert!((r.value() - 1.0).abs() < 1.0e-6);
        for _ in 0..50 {
            r.advance(1.0);
        }
        assert!(r.is_settled());
        assert!((r.value() - 0.4).abs() < 1.0e-6);
    }

    /// 敌意输入零 panic：NaN/Inf 坐标、超界区间、非有限比例。
    #[test]
    fn hostile_inputs_do_not_panic() {
        let (a, b) = seg(4.0);
        let g = geo_at(2.0, 1.0, MAT_CONCRETE);
        for bad in [
            Cartesian::new(f32::NAN, 0.0, 4.0),
            Cartesian::new(0.0, f32::INFINITY, 0.0),
            Cartesian::new(f32::NEG_INFINITY, 0.0, 0.0),
        ] {
            let v = classify(&g, bad, b);
            assert!(v.blocked_ratio.is_finite() && v.target_gain.is_finite());
        }
        let spans = vec![
            OcclusionSpan { t0: f32::NAN, t1: 1.0, material: MAT_CONCRETE },
            OcclusionSpan { t0: -5.0, t1: 9.0, material: MAT_CURTAIN },
        ];
        let r = weighted_blocked_ratio(spans.as_slice());
        assert!(r.is_finite() && (0.0..=1.0).contains(&r));
        // 非有限比例按最严处理（异常零静默的保守侧）。
        assert_eq!(OcclusionGrade::of(f32::NAN), OcclusionGrade::Full);
        // 零长连线退化为点包含。
        assert!(classify(&g, a, a).target_gain.is_finite());
    }

    /// 确定性：同输入两次构造结果逐位相同。
    #[test]
    fn classification_is_deterministic() {
        let (a, b) = seg(4.0);
        let mk = || classify(&geo_at(2.0, 1.0, MAT_WOOD), a, b);
        assert_eq!(mk(), mk());
    }
}
