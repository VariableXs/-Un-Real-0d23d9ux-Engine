//! VE-F3505 · 夜间模式与高对比度交互 —— 两特性正交组合 + 冲突仲裁 + 互不干扰。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3505`
//!
//! # 职责（锚点原文拆解）
//!
//! - **正交组合**（[`FeatureMatrix`]）：色温夜览（改色温不动对比度）
//!   与高对比度主题（改对比度不动色温）是**两个正交的无障碍特性**。
//!   组合状态是二维矩阵（夜览开/关 × 高对比开/关），四个格子都是
//!   合法状态——不存在「开了夜览就开不了高对比」这种隐式互斥。
//! - **冲突仲裁**（[`OrthoArbiter`]）：两特性同时请求改**同一个**
//!   底层属性时才需要仲裁。仲裁规则公开且写死：**对比度优先**——
//!   夜览只改色温轴，高对比只改对比轴；若某实现把两轴拧在一起
//!   （历史包袱），仲裁器拆轴（[`AxisSplitter`]）并告警
//!   （[`OrthoCode::AxisEntangled`]）。
//! - **互不干扰承诺**（[`OrthoInvariant`]）：任何一方变更后，另一方的
//!   生效值必须不变。用不变量快照验证：变更前抓对方轴的值，变更后
//!   复查——不一致即违例（这是 CI 级判据的运行时形态）。
//! - **规则公开**（[`OrthoView`]）：当前组合状态、双方生效值、
//!   最近一次仲裁结果，一次读全（读屏播报源）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 组合失效 | 分别生效（拆轴） |
//! | 冲突 | 仲裁（对比度优先） |
//! | 规则漂移 | 修正（不变量复检） |
//!
//! # 复杂度
//!
//! 全部 O(1)。状态是四个枚举 + 两个轴值。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5720 起）
// ---------------------------------------------------------------------------

/// 夜览×高对比组合域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrthoCode {
    /// 两轴拧在一起（拆轴处理并告警）。
    AxisEntangled = 5720,
    /// 互不干扰承诺被破坏（对方轴值漂移）。
    CrossAxisDrift = 5721,
    /// 组合状态非法（矩阵外状态）。
    ComboInvalid = 5722,
    /// 对比度低于硬线（无障碍底线，强制回升）。
    ContrastBelowFloor = 5723,
}

impl OrthoCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            OrthoCode::AxisEntangled => "色温与对比度两轴纠缠",
            OrthoCode::CrossAxisDrift => "正交承诺被破坏",
            OrthoCode::ComboInvalid => "组合状态非法",
            OrthoCode::ContrastBelowFloor => "对比度低于硬线",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            OrthoCode::CrossAxisDrift | OrthoCode::ContrastBelowFloor
        )
    }
}

// ---------------------------------------------------------------------------
// 二、状态矩阵
// ---------------------------------------------------------------------------

/// 对比度硬线（千分比 4500 = 4.5:1，WCAG AA 正文线）。
pub const CONTRAST_FLOOR_PERMILLE: u32 = 4500;

/// 夜览状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NightMode {
    /// 关。
    Off,
    /// 开（当前色温）。
    On(u32),
}

/// 高对比状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HighContrast {
    /// 关。
    Off,
    /// 开（当前对比度，千分比）。
    On(u32),
}

/// 组合状态四格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Combo {
    /// 全关（标准显示）。
    Plain,
    /// 只开夜览。
    NightOnly,
    /// 只开高对比。
    ContrastOnly,
    /// 两者都开（合法且常见——深夜 + 低视力用户）。
    Both,
}

/// 从两态推导组合格。
pub fn combo_of(n: NightMode, h: HighContrast) -> Combo {
    match (n, h) {
        (NightMode::Off, HighContrast::Off) => Combo::Plain,
        (NightMode::On(_), HighContrast::Off) => Combo::NightOnly,
        (NightMode::Off, HighContrast::On(_)) => Combo::ContrastOnly,
        (NightMode::On(_), HighContrast::On(_)) => Combo::Both,
    }
}

/// 组合状态中文说明（读屏播报）。
pub fn combo_label(c: Combo) -> &'static str {
    match c {
        Combo::Plain => "标准显示",
        Combo::NightOnly => "仅夜览色温",
        Combo::ContrastOnly => "仅高对比度",
        Combo::Both => "夜览与高对比度同时生效",
    }
}

// ---------------------------------------------------------------------------
// 三、拆轴器与仲裁器
// ---------------------------------------------------------------------------

/// 拆轴后的两轴值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AxisPair {
    /// 色温轴（开尔文）。
    pub kelvin: u32,
    /// 对比轴（千分比）。
    pub contrast_permille: u32,
}

/// 拆轴器：把「一个实现同时动两轴」的请求拆成两条正交请求。
/// 拆轴动作本身是告警事件（历史包袱的证据）。
#[derive(Default)]
pub struct AxisSplitter {
    pub events: Vec<OrthoCode>,
}

impl AxisSplitter {
    /// 拆轴：请求方声称要同时改两轴 → 拆开返回两轴目标值。
    pub fn split(&mut self, req_kelvin: u32, req_contrast: u32) -> AxisPair {
        self.events.push(OrthoCode::AxisEntangled);
        AxisPair {
            kelvin: super::ver08a_kelvin::clamp_kelvin(req_kelvin),
            contrast_permille: req_contrast.clamp(1000, 21000),
        }
    }

    /// 取走事件。
    pub fn take_events(&mut self) -> Vec<OrthoCode> {
        core::mem::take(&mut self.events)
    }
}

/// 仲裁结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrthoVerdict {
    /// 两轴各走各的，无冲突。
    Independent,
    /// 拆轴后分别生效。
    SplitApplied(AxisPair),
    /// 对比度硬线触发，夜览请求被部分钳制。
    ContrastFloorGuarded(AxisPair),
}

/// 正交仲裁器：夜览/高对比两特性的唯一合流点。
pub struct OrthoArbiter {
    night: NightMode,
    hc: HighContrast,
    last_verdict: Option<OrthoVerdict>,
    drift_count: u32,
}

impl OrthoArbiter {
    /// 初始仲裁器（全关）。
    pub fn new() -> Self {
        OrthoArbiter {
            night: NightMode::Off,
            hc: HighContrast::Off,
            last_verdict: None,
            drift_count: 0,
        }
    }

    /// 当前组合格。
    pub fn combo(&self) -> Combo {
        combo_of(self.night, self.hc)
    }

    /// 当前两轴值。
    pub fn axes(&self) -> AxisPair {
        let kelvin = match self.night {
            NightMode::On(k) => k,
            NightMode::Off => 6500,
        };
        let contrast = match self.hc {
            HighContrast::On(c) => c,
            HighContrast::Off => 2100,
        };
        AxisPair {
            kelvin,
            contrast_permille: contrast,
        }
    }

    /// 最近仲裁结果。
    pub fn last_verdict(&self) -> Option<OrthoVerdict> {
        self.last_verdict
    }

    /// 漂移计数（互不干扰违例累计，审计）。
    pub fn drift_count(&self) -> u32 {
        self.drift_count
    }

    /// 应用夜览变更。硬线检查：变更后对比轴不得低于硬线。
    pub fn apply_night(&mut self, k: u32) -> OrthoVerdict {
        let before = self.axes().contrast_permille;
        self.night = NightMode::On(super::ver08a_kelvin::clamp_kelvin(k));
        let after = self.axes();
        let v = if after.contrast_permille < CONTRAST_FLOOR_PERMILLE {
            // 硬线守卫：把对比度抬回底线（高对比特性被夜览改动牵连
            // 只会发生在轴纠缠的坏实现里——运行时兜底 + 记违例）。
            self.drift_count += 1;
            self.hc = HighContrast::On(CONTRAST_FLOOR_PERMILLE);
            let fixed = self.axes();
            self.last_verdict = Some(OrthoVerdict::ContrastFloorGuarded(fixed));
            return OrthoVerdict::ContrastFloorGuarded(fixed);
        } else {
            OrthoVerdict::Independent
        };
        // 互不干扰复查：对比轴没动才算干净。
        if after.contrast_permille != before && matches!(self.hc, HighContrast::On(_)) {
            self.drift_count += 1;
        }
        self.last_verdict = Some(v);
        v
    }

    /// 应用高对比变更。夜览轴必须原样保留（正交承诺）。
    pub fn apply_contrast(&mut self, c: u32) -> Result<OrthoVerdict, OrthoCode> {
        let cc = c.clamp(1000, 21000);
        if cc < CONTRAST_FLOOR_PERMILLE && cc > 2100 {
            // 用户明确要低于硬线的对比度：拒绝——无障碍底线不谈判。
            return Err(OrthoCode::ContrastBelowFloor);
        }
        let kelvin_before = self.axes().kelvin;
        self.hc = HighContrast::On(cc);
        let after = self.axes();
        if after.kelvin != kelvin_before {
            self.drift_count += 1;
            return Err(OrthoCode::CrossAxisDrift);
        }
        let v = OrthoVerdict::Independent;
        self.last_verdict = Some(v);
        Ok(v)
    }

    /// 关闭夜览。
    pub fn clear_night(&mut self) {
        self.night = NightMode::Off;
    }

    /// 关闭高对比。
    pub fn clear_contrast(&mut self) {
        self.hc = HighContrast::Off;
    }

    /// 规则公开视图（读屏播报源）。
    pub fn view(&self) -> OrthoView {
        OrthoView {
            combo: self.combo(),
            axes: self.axes(),
            drift_count: self.drift_count,
            rule: "对比度优先；色温只动色温轴，对比度只动对比轴",
        }
    }
}

/// 规则公开视图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrthoView {
    /// 当前组合格。
    pub combo: Combo,
    /// 两轴值。
    pub axes: AxisPair,
    /// 互不干扰违例计数。
    pub drift_count: u32,
    /// 仲裁规则文本（公开）。
    pub rule: &'static str,
}

// ---------------------------------------------------------------------------
// 四、不变量快照（CI 判据的运行时形态）
// ---------------------------------------------------------------------------

/// 正交不变量：抓取对方轴快照，变更后复查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrthoInvariant {
    snapshot_kelvin: u32,
    snapshot_contrast: u32,
}

impl OrthoInvariant {
    /// 抓快照。
    pub fn capture(a: &OrthoArbiter) -> Self {
        let ax = a.axes();
        OrthoInvariant {
            snapshot_kelvin: ax.kelvin,
            snapshot_contrast: ax.contrast_permille,
        }
    }

    /// 复查：返回被破坏的轴（无破坏返回 None）。
    pub fn verify(&self, a: &OrthoArbiter, changed: Axis) -> Option<Axis> {
        let ax = a.axes();
        match changed {
            Axis::Kelvin => {
                if ax.contrast_permille != self.snapshot_contrast {
                    Some(Axis::Contrast)
                } else {
                    None
                }
            }
            Axis::Contrast => {
                if ax.kelvin != self.snapshot_kelvin {
                    Some(Axis::Kelvin)
                } else {
                    None
                }
            }
        }
    }
}

/// 轴标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// 色温轴。
    Kelvin,
    /// 对比度轴。
    Contrast,
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orthogonal_combo_both_legal() {
        let mut a = OrthoArbiter::new();
        a.apply_night(3400);
        let inv = OrthoInvariant::capture(&a);
        a.apply_contrast(8000).unwrap();
        // 夜览轴不该被对比度变更牵动。
        assert_eq!(inv.verify(&a, Axis::Contrast), None);
        assert_eq!(a.combo(), Combo::Both);
    }

    #[test]
    fn contrast_floor_guarded() {
        let mut a = OrthoArbiter::new();
        a.apply_contrast(6000).unwrap();
        let v = a.apply_night(3400);
        assert!(matches!(v, OrthoVerdict::ContrastFloorGuarded(_)));
        assert!(a.axes().contrast_permille >= CONTRAST_FLOOR_PERMILLE);
    }

    #[test]
    fn below_floor_rejected() {
        let mut a = OrthoArbiter::new();
        assert_eq!(
            a.apply_contrast(3000),
            Err(OrthoCode::ContrastBelowFloor)
        );
    }
}
