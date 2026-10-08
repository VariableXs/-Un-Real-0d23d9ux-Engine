//! VE-F1805 · 聚光（VE-J 域 · 光照与阴影 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1805`
//!
//! **判据（锚点原文）**：实现锥形光（位置/方向/内锥角/外锥角四参数）的半影
//! 平滑过渡（内外锥间 smoothstep 衰减）与衰减合成（距离衰减×锥形衰减两级
//! 相乘），并预留 IES 光域网文件支持接口（一期接口位+解析器占位，诚实标注
//! 未实现）。判据四条：**内外锥、衰减合成、IES 预留、显性报错**。
//!
//! **错误路径与降级矩阵**：内角≥外角→参数校验拒绝（导入期+运行时双检）；
//! 方向零向量→归一化告警；锥角超 180°→钳制；IES 文件被引用但接口未实现→
//! 显性报错（错误三要素：预留接口不静默吞）并提示当前版本边界；光域网未来
//! 实现不得改变现有参数块布局（前向兼容约束写入接口注释）。
//!
//! **设计要点**：
//! - **内外锥**：角度弧度存储，`inner < outer` 硬约束——导入期构造拒绝 +
//!   运行期 [`SpotLight::validate`] 双检；外锥超 180° 钳制到 π 并告警；
//!   **判定顺序为「先钳制、后校序」**：内锥 3.2 / 外锥 7.0 时外锥被钳到 π，
//!   校序在内会把自相矛盾的参数块（内>外、半影分母为负）当作合法放行；
//! - **非有限角显式拒绝**：NaN 与任何值比较恒为 false，仅靠 `>=` 校序会
//!   **静默放行 NaN** 并让它传播进着色结果，故有限性检查是双检的共同前置；
//! - **半影平滑**：内锥全亮、外锥归零，之间 `1 - smoothstep(t)`（t 归一化
//!   锥角），三次多项式确定性实现（无浮点查表漂移）；
//! - **衰减合成**：锥形衰减 × 距离衰减（复用 F1804 的 [`curve_attenuation`]
//!   单一事实源）两级相乘，O(1)/像素；
//! - **IES 预留**：[`IesSlot`] 接口位（文件路径 + `Reserved` 状态枚举）——
//!   引用未实现的解析器即显性报错（[`SpotError::IesNotImplemented`]，三要素
//!   齐发并提示当前版本边界）；**前向兼容**：光域网未来实现不得改变现有
//!   参数块布局（约束写入 [`IES_FORWARD_COMPAT_DOC`]）；
//! - **LUT 可选**：[`SpotLut`] 角度→衰减一维预计算（线性插值采样），
//!   实时 smoothstep 之外的优化路径，精度对拍入自检；构建前复用 `validate`
//!   校验灯，坏灯不参与建表（否则步长为负、采样错位）。
//!
//! **跨批对接点**：F1828 聚光阴影视锥贴合直接消费本条目锥参数（J02 契约
//! 兑现点）；强度语义 F1802；管理 F1807；fuzz 进 F1813；调试可视 F1810。
//!
//! 逻辑 tick 注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use super::vej04_pointlight::{curve_attenuation, FalloffCurve};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 外锥角上限（180°，弧度）——超限钳制。
pub const MAX_CONE_ANGLE: f32 = core::f32::consts::PI;

/// 零向量回退方向（方向零向量告警后的确定性回退；-Z 前向惯例）。
pub const FALLBACK_DIR: (f32, f32, f32) = (0.0, 0.0, -1.0);

/// IES 前向兼容约束（接口注释判据的文本载体）。
pub const IES_FORWARD_COMPAT_DOC: &str = "\
IES 光域网前向兼容约束（VE-F1805 · v1）：光域网解析器未来实现时，\
不得改变现有聚光参数块布局（位置/方向/内外锥角/距离衰减曲线/强度/颜色\
字段序与语义冻结）；IES 采样结果只允许经由 IesSlot 状态位接入锥形衰减\
的乘法链，不允许新增必填参数。当前版本解析器未实现（RESERVED）。";

// ---------------------------------------------------------------------------
// 二、错误（三要素显性）
// ---------------------------------------------------------------------------

/// 聚光错误（构造拒绝与预留接口显性报错；describe 三要素齐发）。
///
/// **不derive `Eq`**：变体 `ConeOrderInvalid` 携带 `f32` 角度值，而 `f32`
/// 只实现 `PartialEq` 不实现 `Eq`（含 NaN 不自反）。派生 `Eq` 会直接编译失败
/// E0277——半成品即踩此坑，故此处只 `PartialEq`。
#[derive(Clone, Debug, PartialEq)]
pub enum SpotError {
    /// 内角 ≥ 外角（导入期或运行期检出）。
    ConeOrderInvalid {
        /// 内角值。
        inner: f32,
        /// 外角值。
        outer: f32,
        /// 检出阶段（导入期/运行期）。
        phase: &'static str,
    },
    /// 锥角非有限（NaN/±∞）——比较法放行 NaN，故须显式有限性检查。
    ConeNotFinite {
        /// 出问题的字段名。
        field: &'static str,
        /// 原始值。
        value: f32,
        /// 检出阶段。
        phase: &'static str,
    },
    /// LUT 采样点数不足（<2 无法覆盖区间两端，步长将除零/错位）。
    LutSamplesTooFew {
        /// 请求的采样点数。
        requested: usize,
        /// 最小合法点数。
        minimum: usize,
    },
    /// IES 光域网被引用但解析器未实现（预留接口显性报错，不静默吞）。
    IesNotImplemented { path: String },
}

impl SpotError {
    /// 人话描述（发生了什么/为什么/下一步 + 版本边界）。
    pub fn describe(&self) -> String {
        match self {
            SpotError::ConeOrderInvalid {
                inner,
                outer,
                phase,
            } => format!(
                "聚光参数被拒（{phase}）：内锥角 {inner} ≥ 外锥角 {outer}——半影区间必须为正，\
                 否则 smoothstep 分母为零。下一步：调小内锥角或调大外锥角",
            ),
            SpotError::ConeNotFinite {
                field,
                value,
                phase,
            } => format!(
                "聚光参数被拒（{phase}）：{field} = {value} 非有限（NaN/±∞）——\
                 NaN 与任何值比较恒为 false，靠大小比较无法拦住它，会一路NaN 传播进\
                 着色结果（黑屏/花屏）。下一步：改用有限的角度值重新导入",
            ),
            SpotError::IesNotImplemented { path } => format!(
                "IES 光域网被引用（{path}）但当前版本解析器未实现（RESERVED 预留接口）——\
                 预留接口不静默吞。下一步：移除 IES 引用改用内外锥角参数；\
                 光域网支持属后续版本边界（见 IES_FORWARD_COMPAT_DOC）",
            ),
            SpotError::LutSamplesTooFew { requested, minimum } => format!(
                "半影 LUT 构建被拒：采样点数 {requested} < {minimum}——\
                 少于2 点无法同时覆盖内外锥两端，步长除以 (n-1) 会除零或采样错位。\
                 下一步：把采样点数提到 {minimum} 以上",
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、IES 接口位（预留 + 显性报错）
// ---------------------------------------------------------------------------

/// IES 光域网接口状态（一期只实现 RESERVED）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IesStatus {
    /// 未引用光域网（纯内外锥角控制）。
    NotUsed,
    /// 已预留：文件路径已登记，解析器未实现（当前版本边界）。
    Reserved {
        /// 光域网文件路径。
        path: String,
    },
}

/// IES 接口位（参数块布局冻结——见 [`IES_FORWARD_COMPAT_DOC`]）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IesSlot {
    /// 状态位。
    pub status: IesStatus,
}

impl IesSlot {
    /// 未引用。
    pub const fn none() -> Self {
        IesSlot {
            status: IesStatus::NotUsed,
        }
    }

    /// 登记光域网引用：一期只登记路径（RESERVED），解析显性报错。
    pub fn reserve(path: &str) -> Self {
        IesSlot {
            status: IesStatus::Reserved {
                path: path.to_string(),
            },
        }
    }

    /// 请求解析光域网：未实现即显性报错（三要素判据）。
    pub fn request_parse(&self) -> Result<(), SpotError> {
        match &self.status {
            IesStatus::NotUsed => Ok(()),
            IesStatus::Reserved { path } => {
                Err(SpotError::IesNotImplemented { path: path.clone() })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 四、聚光参数块（构造即校验 + 运行期双检）
// ---------------------------------------------------------------------------

/// 聚光参数块（位置/方向/内角/外角 + F1804 距离衰减 + IES 接口位）。
#[derive(Clone, Debug, PartialEq)]
pub struct SpotLight {
    /// 位置（场景空间）。
    pub pos: (f32, f32, f32),
    /// 方向（单位向量；零向量已回退并告警）。
    pub dir: (f32, f32, f32),
    /// 内锥角（弧度，全亮边界）。
    pub inner_cone: f32,
    /// 外锥角（弧度，零光边界；超 180° 已钳制）。
    pub outer_cone: f32,
    /// 范围半径（距离衰减钳制边界）。
    pub range: f32,
    /// 强度（cd，非负——F1802 量纲纪律）。
    pub intensity_cd: f32,
    /// 距离衰减曲线（沿用 F1804 枚举）。
    pub distance_curve: FalloffCurve,
    /// IES 接口位。
    pub ies: IesSlot,
}

/// 聚光构造告警（方向回退/锥角钳制等显性入账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotWarning {
    /// 告警码。
    pub code: &'static str,
    /// 人话说明。
    pub detail: String,
}

impl SpotLight {
    /// 构造（导入期检查）：非有限角拒绝；外角超 180° 钳制后校内锥序；
    /// 零方向回退告警。
    ///
    /// 参数为 8 个：聚光四参数（位置/方向/内锥/外锥）+ 沿用 F1804 的距离
    /// 语义（范围/强度/曲线）+ IES 接口位。这里刻意保持平铺——四参数各自
    /// 同层可读，比塞进builder 更利于导入期逐项校验与告警归因；聚光参数块
    /// 本身即可校验来源，不另立来源类型。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pos: (f32, f32, f32),
        dir: (f32, f32, f32),
        inner_cone: f32,
        outer_cone: f32,
        range: f32,
        intensity_cd: f32,
        distance_curve: FalloffCurve,
        ies: IesSlot,
    ) -> Result<(Self, Vec<SpotWarning>), SpotError> {
        let mut warn = Vec::new();
        // 有限性先行（双检的共同前置）：NaN 与任何值比较恒为 false，靠
        // `>=` 判内锥序会**静默放行 NaN**，随后 NaN 一路传播进着色结果。
        // 故 NaN/±∞ 必须显式拒绝，而不是指望大小比较能拦住。
        for (name, v) in [("inner_cone", inner_cone), ("outer_cone", outer_cone)] {
            if !v.is_finite() {
                return Err(SpotError::ConeNotFinite {
                    field: name,
                    value: v,
                    phase: "导入期",
                });
            }
        }
        // 外锥超 180° → 钳制（先钳制，再校内锥序——顺序反了会自相矛盾）。
        let outer_cone = if outer_cone > MAX_CONE_ANGLE {
            warn.push(SpotWarning {
                code: "W_CONE_CLAMPED",
                detail: format!("外锥角 {outer_cone} 超 180° → 钳制到 π"),
            });
            MAX_CONE_ANGLE
        } else {
            outer_cone
        };
        // 内锥序双检之一（导入期）。
        // **必须在钳制之后判**：内锥 3.2 / 外锥 7.0 时外锥被钳到 π=3.14159，
        // 于是内锥(3.2) > 外锥(3.14159)。若先判内锥序再钳制，这个自相矛盾的
        // 参数块会被当作合法放行（半成品即此顺序错），运行期才发现，且半影
        // 分母为负 → smoothstep 参数越界。
        if inner_cone >= outer_cone {
            return Err(SpotError::ConeOrderInvalid {
                inner: inner_cone,
                outer: outer_cone,
                phase: "导入期",
            });
        }
        // 方向零向量 → 回退 + 告警；否则归一化。
        let len2 = dir.0 * dir.0 + dir.1 * dir.1 + dir.2 * dir.2;
        let dir = if !len2.is_finite() || len2 <= 0.0 {
            warn.push(SpotWarning {
                code: "W_DIR_ZERO_FALLBACK",
                detail: format!("方向 {dir:?} 为零/非法向量 → 回退 -Z 前向"),
            });
            FALLBACK_DIR
        } else {
            let len = sqrt_approx(len2);
            (dir.0 / len, dir.1 / len, dir.2 / len)
        };
        Ok((
            SpotLight {
                pos,
                dir,
                inner_cone,
                outer_cone,
                range,
                intensity_cd,
                distance_curve,
                ies,
            },
            warn,
        ))
    }

    /// 运行期双检（导入期之外的第二道闸；F1807 管理层每帧调用）。
    ///
    /// 与导入期同序：先有限性、再内锥序。字段是 `pub`，运行期可被外部改坏，
    /// 故此闸必须能独立拦住 NaN 与内≥外两种坏值。
    pub fn validate(&self) -> Result<(), SpotError> {
        for (name, v) in [
            ("inner_cone", self.inner_cone),
            ("outer_cone", self.outer_cone),
        ] {
            if !v.is_finite() {
                return Err(SpotError::ConeNotFinite {
                    field: name,
                    value: v,
                    phase: "运行期",
                });
            }
        }
        if self.inner_cone >= self.outer_cone {
            return Err(SpotError::ConeOrderInvalid {
                inner: self.inner_cone,
                outer: self.outer_cone,
                phase: "运行期",
            });
        }
        Ok(())
    }
}

/// 平方根（无 libm 的确定性牛顿迭代——内核树既有自建惯例）。
fn sqrt_approx(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    let mut x = v;
    for _ in 0..24 {
        // 上一轮迭代值即收敛判据，直接在循环内比较，无需额外中间变量。
        let next = 0.5 * (x + v / x);
        if (next - x).abs() < 1e-7 {
            return next;
        }
        x = next;
    }
    x
}

// ---------------------------------------------------------------------------
// 五、锥形衰减与合成（O(1)/像素）
// ---------------------------------------------------------------------------

/// 半影平滑：`1 - smoothstep(t)`，t ∈ [0,1]（三次多项式 3t²-2t³，确定性）。
pub fn penumbra_factor(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// 锥形衰减：与光轴夹角 → [0,1] 系数（内锥 1 / 半影过渡 / 外锥 0）。
pub fn cone_attenuation(light: &SpotLight, dx: f32, dy: f32, dz: f32) -> f32 {
    let dlen = sqrt_approx(dx * dx + dy * dy + dz * dz);
    if dlen <= 0.0 {
        return 1.0; // 点在光源处：沿轴判定不适用，按全亮（距离衰减另有 0）。
    }
    let cos_theta = (dx * light.dir.0 + dy * light.dir.1 + dz * light.dir.2) / dlen;
    let cos_theta = cos_theta.clamp(-1.0, 1.0);
    let theta = acos_approx(cos_theta);
    if theta <= light.inner_cone {
        1.0
    } else if theta >= light.outer_cone {
        0.0
    } else {
        penumbra_factor((theta - light.inner_cone) / (light.outer_cone - light.inner_cone))
    }
}

/// 衰减合成：锥形衰减 × 距离衰减（两级相乘判据；O(1)/像素）。
pub fn combined_attenuation(light: &SpotLight, px: (f32, f32, f32)) -> f32 {
    let dx = px.0 - light.pos.0;
    let dy = px.1 - light.pos.1;
    let dz = px.2 - light.pos.2;
    let cone = cone_attenuation(light, dx, dy, dz);
    let dist = sqrt_approx(dx * dx + dy * dy + dz * dz);
    cone * curve_attenuation(light.distance_curve, light.range, dist)
}

/// 反余弦（无 libm 的确定性实现：半角恒等式 + 自建 atan2）。
fn acos_approx(c: f32) -> f32 {
    let c = c.clamp(-1.0, 1.0);
    let sin_half = ((1.0 - c) * 0.5).max(0.0).sqrt();
    let cos_half = ((1.0 + c) * 0.5).max(1e-12).sqrt();
    2.0 * atan2_approx(sin_half, cos_half)
}

/// atan2（无 libm 的确定性实现：半角化简三次 + 幂级数，误差 <1e-6 rad）。
///
/// 化简恒等式：atan(z) = 2·atan(z / (1+√(1+z²)))，逐次把自变量压到
/// tan(π/16)≈0.2 以内后用五项交错级数（确定性，无查表）。
fn atan2_approx(y: f32, x: f32) -> f32 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let mut a = atan_pos(y.abs() / x.abs());
    if x < 0.0 {
        a = core::f32::consts::PI - a;
    }
    if y < 0.0 {
        a = -a;
    }
    a
}

/// atan(z)，z ≥ 0（主值 [0, π/2)）。
fn atan_pos(z: f32) -> f32 {
    if z > 1.0 {
        return core::f32::consts::FRAC_PI_2 - atan_pos(1.0 / z);
    }
    // 三次半角化简：每次 z ← z/(1+√(1+z²))，atan(z) 翻倍。
    let z1 = z / (1.0 + sqrt_approx(1.0 + z * z));
    let z2 = z1 / (1.0 + sqrt_approx(1.0 + z1 * z1));
    let z3 = z2 / (1.0 + sqrt_approx(1.0 + z2 * z2));
    let s = z3 * z3;
    let series = z3
        * (1.0 - s / 3.0 + s * s / 5.0 - s * s * s / 7.0 + s * s * s * s / 9.0
            - s * s * s * s * s / 11.0);
    8.0 * series
}

// ---------------------------------------------------------------------------
// 六、LUT 可选优化（角度→衰减一维预计算）
// ---------------------------------------------------------------------------

/// 半影 LUT：角度等分预计算（线性插值采样；实时 smoothstep 的可选替代）。
pub struct SpotLut {
    /// 内/外锥角（采样区间）。
    inner: f32,
    outer: f32,
    /// 等分样本（含两端；samples[i] 对应 inner + i·step）。
    samples: Vec<f32>,
}

impl SpotLut {
    /// 构建（n≥2 等分；n<2 显性拒绝不猜）。
    ///
    /// **先校验灯再算步长**：半影步长是 `(outer - inner)/(n-1)`，若灯的内外锥
    /// 序已坏（内>外，运行期字段被改坏），步长为负 → 采样索引整体错位、
    /// 采样值越界。半成品直接拿灯算步长不校验（实测：内0.9/外0.8 的灯竟
    /// 构建成功），此处改为显性拒绝。
    ///
    /// 复用 [`SpotLight::validate`] 作同一套有限性 + 锥序判定，避免两处
    /// 判据各自漂移。
    pub fn build(light: &SpotLight, n: usize) -> Result<Self, SpotError> {
        if n < 2 {
            return Err(SpotError::LutSamplesTooFew {
                requested: n,
                minimum: 2,
            });
        }
        light.validate().map_err(|e| match e {
            // 阶段名改写为LUT 构建期，如实说明是哪个环节拦下的。
            SpotError::ConeOrderInvalid { inner, outer, .. } => SpotError::ConeOrderInvalid {
                inner,
                outer,
                phase: "LUT构建",
            },
            SpotError::ConeNotFinite { field, value, .. } => SpotError::ConeNotFinite {
                field,
                value,
                phase: "LUT构建",
            },
            other => other,
        })?;
        let step = (light.outer_cone - light.inner_cone) / (n - 1) as f32;
        let mut samples = Vec::with_capacity(n);
        for i in 0..n {
            let theta = light.inner_cone + step * i as f32;
            let t = (theta - light.inner_cone) / (light.outer_cone - light.inner_cone);
            samples.push(penumbra_factor(t));
        }
        Ok(SpotLut {
            inner: light.inner_cone,
            outer: light.outer_cone,
            samples,
        })
    }

    /// 采样（角度→系数；线性插值；区间外按锥语义裁剪）。
    pub fn sample(&self, theta: f32) -> f32 {
        if theta <= self.inner {
            return 1.0;
        }
        if theta >= self.outer {
            return 0.0;
        }
        let n = self.samples.len();
        let step = (self.outer - self.inner) / (n - 1) as f32;
        let idx = ((theta - self.inner) / step) as usize;
        let idx = idx.min(n - 2);
        let frac = (theta - self.inner) / step - idx as f32;
        let a = self.samples[idx];
        let b = self.samples[idx + 1];
        a + (b - a) * frac
    }
}

// ---------------------------------------------------------------------------
// 七、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F1805 域自检（判据逐条映射见 `vej05_checks.rs`）。
pub fn run_vej05_checks() -> CheckSet {
    super::vej05_checks::run_vej05_checks()
}

// ---------------------------------------------------------------------------
// 八、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(inner: f32, outer: f32) -> SpotLight {
        SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            inner,
            outer,
            100.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap()
        .0
    }

    #[test]
    fn vej05_cone_order_rejected_twice() {
        // 导入期拒绝。
        let e = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            1.0,
            0.5,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap_err();
        assert!(matches!(
            e,
            SpotError::ConeOrderInvalid {
                phase: "导入期",
                ..
            }
        ));
        assert!(e.describe().contains("下一步"), "三要素齐发");
        // 运行期双检。
        let s = spot(0.3, 0.8);
        assert!(s.validate().is_ok());
        let bad = SpotLight {
            inner_cone: 0.9,
            outer_cone: 0.8,
            ..spot(0.3, 0.8)
        };
        assert!(matches!(
            bad.validate(),
            Err(SpotError::ConeOrderInvalid {
                phase: "运行期",
                ..
            })
        ));
    }

    #[test]
    fn vej05_penumbra_smoothstep() {
        // 半影对拍：t=0 → 1；t=0.5 → 0.5；t=1 → 0（三次多项式）。
        assert!((penumbra_factor(0.0) - 1.0).abs() < 1e-6);
        assert!((penumbra_factor(0.5) - 0.5).abs() < 1e-6);
        assert!(penumbra_factor(1.0).abs() < 1e-6);
        // 锥形衰减：内锥内全亮，外锥外归零，半影中点约 0.5。
        let s = spot(0.2, 0.8); // 半影 0.2..0.8，中点 0.5
        assert_eq!(cone_attenuation(&s, 0.0, 0.0, -1.0), 1.0, "轴上全亮");
        assert_eq!(cone_attenuation(&s, 10.0, 0.0, 0.0), 0.0, "90° 在外锥外");
        let mid = cone_attenuation(&s, 0.0, -0.4794255, -0.8775826); // θ=0.5
        assert!((mid - 0.5).abs() < 0.01, "半影中点 ≈ 0.5，实际 {mid}");
    }

    #[test]
    fn vej05_distance_cone_composed() {
        // 衰减合成 = 锥形 × 距离（物理平方反比）。
        let s = spot(0.2, 0.8); // range=100, intensity=1
                                // 轴上 d=2：锥形=1，距离=0.25 → 合成 0.25。
        let a = combined_attenuation(&s, (0.0, 0.0, -2.0));
        assert!((a - 0.25).abs() < 1e-5, "两级相乘，实际 {a}");
        // 外锥外：距离再近合成也是 0。
        let b = combined_attenuation(&s, (10.0, 0.0, -0.0));
        assert_eq!(b, 0.0);
    }

    #[test]
    fn vej05_ies_reserved_explicit_error() {
        // 未引用：无动作。
        assert!(IesSlot::none().request_parse().is_ok());
        // 引用未实现解析器：显性报错（三要素 + 版本边界）。
        let slot = IesSlot::reserve("lights/stage.ies");
        let e = slot.request_parse().unwrap_err();
        let d = e.describe();
        assert!(d.contains("未实现") && d.contains("下一步") && d.contains("版本边界"));
        assert!(
            matches!(e, SpotError::IesNotImplemented { ref path } if path == "lights/stage.ies")
        );
        assert!(
            IES_FORWARD_COMPAT_DOC.contains("不得改变现有聚光参数块布局"),
            "前向兼容约束在册"
        );
    }

    #[test]
    fn vej05_dir_zero_fallback_and_clamp() {
        // 零方向 → 回退 + 告警。
        let (s, warn) = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 0.0),
            0.2,
            0.8,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap();
        assert_eq!(s.dir, FALLBACK_DIR);
        assert!(warn.iter().any(|w| w.code == "W_DIR_ZERO_FALLBACK"));
        // 外锥超 180° → 钳制 π。
        let (s2, warn2) = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            0.2,
            7.0,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap();
        assert!((s2.outer_cone - MAX_CONE_ANGLE).abs() < 1e-6);
        assert!(warn2.iter().any(|w| w.code == "W_CONE_CLAMPED"));
        // 单位向量归一化：非零方向入参后长度为 1。
        let (s3, _) = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 3.0, 4.0),
            0.2,
            0.8,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap();
        let l2 = s3.dir.0 * s3.dir.0 + s3.dir.1 * s3.dir.1 + s3.dir.2 * s3.dir.2;
        assert!((l2 - 1.0).abs() < 1e-5);
    }

    #[test]
    fn vej05_lut_matches_live_smoothstep() {
        let s = spot(0.3, 0.9);
        let lut = SpotLut::build(&s, 64).unwrap();
        // 多个采样角：LUT 与实时 smoothstep 一致（插值精度内）。
        for i in 1..16 {
            let theta = s.inner_cone + (s.outer_cone - s.inner_cone) * (i as f32 / 16.0);
            let live = penumbra_factor((theta - s.inner_cone) / (s.outer_cone - s.inner_cone));
            let via_lut = lut.sample(theta);
            assert!(
                (live - via_lut).abs() < 0.01,
                "θ={theta} live={live} lut={via_lut}"
            );
        }
        // 区间外按锥语义。
        assert_eq!(lut.sample(s.inner_cone - 0.01), 1.0);
        assert_eq!(lut.sample(s.outer_cone + 0.01), 0.0);
        // n<2 显性拒绝（错误类型如实区分于锥角非法）。
        assert!(matches!(
            SpotLut::build(&s, 1),
            Err(SpotError::LutSamplesTooFew {
                requested: 1,
                minimum: 2
            })
        ));
        // 坏灯（内>外）不参与建表——否则步长为负、采样整体错位。
        let bad = SpotLight {
            inner_cone: 0.9,
            outer_cone: 0.8,
            ..s.clone()
        };
        assert!(matches!(
            SpotLut::build(&bad, 8),
            Err(SpotError::ConeOrderInvalid {
                phase: "LUT构建",
                ..
            })
        ));
    }

    #[test]
    fn vej05_clamp_before_order_check() {
        // 内锥 3.2 / 外锥 7.0：外锥钳到 π=3.14159 后内锥反而更大。
        // 顺序若错（先校序后钳制），这个自相矛盾的参数块会被当合法放行，
        // 半影分母为负 → smoothstep 参数越界。
        let e = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            3.2,
            7.0,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap_err();
        assert!(
            matches!(
                e,
                SpotError::ConeOrderInvalid {
                    phase: "导入期",
                    ..
                }
            ),
            "{e:?}"
        );
        // 对照：内锥 3.0 < π 钳制后仍合法。
        let (ok, w) = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            3.0,
            7.0,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap();
        assert!(ok.validate().is_ok());
        assert!((ok.outer_cone - MAX_CONE_ANGLE).abs() < 1e-6);
        assert!(w.iter().any(|x| x.code == "W_CONE_CLAMPED"));
    }

    #[test]
    fn vej05_nonfinite_cone_rejected() {
        // NaN 与任何值比较恒 false：只靠 `>=` 校序会静默放行 NaN，
        // 随后 NaN 传播进 cone_attenuation 的着色结果。
        let e = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            f32::NAN,
            0.8,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap_err();
        assert!(
            matches!(
                e,
                SpotError::ConeNotFinite {
                    phase: "导入期",
                    ..
                }
            ),
            "{e:?}"
        );
        // ±∞ 同拒。
        assert!(SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            0.2,
            f32::INFINITY,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .is_err());
        // 运行期双检同样拦得住（字段 pub，可被外部改坏）。
        let s = spot(0.2, 0.8);
        let nan_light = SpotLight {
            inner_cone: f32::NAN,
            ..s.clone()
        };
        assert!(matches!(
            nan_light.validate(),
            Err(SpotError::ConeNotFinite {
                phase: "运行期",
                ..
            })
        ));
        // 错误三要素齐发。
        let d = e.describe();
        assert!(d.contains("下一步") && d.contains("NaN"));
    }

    #[test]
    fn vej05_checks_all_green() {
        let set = run_vej05_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F1805 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}
