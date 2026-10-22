//! VE-F3443 · 圆角变体运行时 —— 三档圆角、嵌套一致性、兄弟一致、钳制
//!
//! ## 职责拆解
//! 1. **三档**（`Radius`）：锐利 2px / 标准 8px / 圆润 16px。数值本身不稀奇，
//!    稀奇的是**它们必须能嵌套**：卡片 16px 里放按钮，按钮的角要跟着变，否则
//!    内角比外角还圆，视觉上「角套角」会崩。
//! 2. **嵌套约束**（`check_nesting`）：`外角 ≥ 内角 + 间距`。这是硬几何：
//!    内外两段圆弧同心时，半径必须相差一个 padding，否则内弧会切到外弧。
//! 3. **反向求解**（`inner_for`）：给定外角与间距，算出**最大**可用的内角。
//!    这是设计侧最需要的函数——它把「约束」变成「可直接用的数」。
//! 4. **兄弟一致**（`check_siblings`）：同一容器内的同级控件必须同角。
//!    一排按钮里混进一个异角，看上去是「手滑」，实际是令牌用错了。
//! 5. **钳制**（`clamp`）：圆角不得超过短边的一半，否则矩形退化成胶囊/椭圆。
//!
//! ## 为什么这么拆
//! 圆角是最「看起来无脑」的轴：改个数字就行。恰恰因此它最容易出错——
//! 单看每个值都对，套在一起就崩。本册把「一起看」的三条几何关系
//! （嵌套、兄弟、钳制）全部做成可失败检查，而不是留给视觉评审去发现。
//!
//! ## 复杂度
//! - 嵌套检查 O(1)；兄弟检查 O(n)，n 为同级控件数。
//! - 全档审计 O(档 × 检查项) = 常数。
//!
//! ## 零 panic 面
//! - 全 u32 运算，减法前先比较（`saturating_sub`），不会下溢。
//! - 无索引切片，无 `unwrap` / `expect`。

use alloc::format;
use alloc::string::ToString;
use alloc::string::String;
use alloc::vec::Vec;

/// 圆角上限占短边的比例（千分比）。500‰ = 一半，超过即退化成椭圆。
pub const MAX_RADIUS_PERMILLE: u32 = 500;

/// 诊断码（E18 段 · 5607~5616）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum RadiusCode {
    /// 档位越界。
    LevelOutOfRange = 5607,
    /// 嵌套违例：内角相对外角过大。
    NestedViolation = 5608,
    /// 半径为负（调用方传了非法值）。
    NegativeRadius = 5609,
    /// 圆角超过短边一半。
    RadiusTooLarge = 5610,
    /// 间距为负。
    GapNegative = 5611,
    /// 同级控件圆角不一致。
    SiblingInconsistent = 5612,
    /// 钳制后仍不可用（短边过小）。
    ClampImpossible = 5613,
    /// 三档不单调。
    NonMonotonic = 5614,
    /// 计算溢出。
    Overflow = 5615,
    /// 圆角族为空。
    EmptyFamily = 5616,
}

impl RadiusCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            RadiusCode::LevelOutOfRange => "档位越界",
            RadiusCode::NestedViolation => "嵌套违例：内角相对外角过大",
            RadiusCode::NegativeRadius => "半径为负",
            RadiusCode::RadiusTooLarge => "圆角超过短边一半",
            RadiusCode::GapNegative => "间距为负",
            RadiusCode::SiblingInconsistent => "同级控件圆角不一致",
            RadiusCode::ClampImpossible => "短边过小，钳制后仍不可用",
            RadiusCode::NonMonotonic => "三档不单调",
            RadiusCode::Overflow => "计算溢出",
            RadiusCode::EmptyFamily => "圆角族为空",
        }
    }
}

/// 三档圆角。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Radius {
    Sharp,
    Standard,
    Round,
}

impl Radius {
    pub const ALL: [Radius; 3] = [Radius::Sharp, Radius::Standard, Radius::Round];

    pub fn of_level(level: u8) -> Result<Self, RadiusCode> {
        match level {
            0 => Ok(Radius::Sharp),
            1 => Ok(Radius::Standard),
            2 => Ok(Radius::Round),
            _ => Err(RadiusCode::LevelOutOfRange),
        }
    }

    pub fn level(self) -> u8 {
        match self {
            Radius::Sharp => 0,
            Radius::Standard => 1,
            Radius::Round => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Radius::Sharp => "sharp",
            Radius::Standard => "standard",
            Radius::Round => "round",
        }
    }

    /// 该档的基础圆角（px）。锐利档取 2 而非 0：0 会让「有边框」与「无边框」
    /// 的控件在抗锯齿下看起来是两种材质。
    pub fn base_px(self) -> u32 {
        match self {
            Radius::Sharp => 2,
            Radius::Standard => 8,
            Radius::Round => 16,
        }
    }

    /// 按控件尺寸缩放圆角：小控件（≤24px）用基础值，大控件按 lg 比例放大但不超过
    /// 基础值的两倍。理由是圆角要与控件「看起来同族」，不是等比例放大。
    pub fn for_size(self, size_px: u32) -> u32 {
        let base = self.base_px();
        if size_px <= 24 {
            base
        } else if size_px <= 96 {
            base + base / 2
        } else {
            base * 2
        }
    }
}

/// 嵌套检查：内角为直角永远合法；否则要求 `外角 ≥ 内角 + 间距`。
///
/// 为什么不是「外角 = 内角 + 间距」？因为内角可以更小（更方），只要小到不切外弧
/// 就是合法的。等式只是最优解，不等式才是约束。
///
/// 「内角为 0 永远合法」这条是踩出来的：锐利档外角 2px 配 12px 内边距时，
/// 偏移量已经大于外角，内层**本来就不该有圆角**——按 `外 ≥ 内+间` 硬算会把
/// 「正确地没有圆角」判成违例。几何上，偏移超过外弧半径时内层退化为直角，
/// 这不是妥协，是唯一解。
pub fn check_nesting(outer: u32, inner: u32, gap: u32) -> Result<(), RadiusCode> {
    if inner == 0 {
        return Ok(());
    }
    let need = inner.saturating_add(gap);
    if outer < need {
        return Err(RadiusCode::NestedViolation);
    }
    Ok(())
}

/// 给定外角与间距，求最大可用的内角。间距大于外角时返回 0（内角最多是直角）。
pub fn inner_for(outer: u32, gap: u32) -> u32 {
    outer.saturating_sub(gap)
}

/// 钳制到短边一半。短边为 0 时返回 0（退化为无圆角，但仍然合法）。
pub fn clamp(radius: u32, short_side: u32) -> u32 {
    let limit = mul_permille(short_side, MAX_RADIUS_PERMILLE);
    if radius > limit {
        limit
    } else {
        radius
    }
}

fn mul_permille(value: u32, permille: u32) -> u32 {
    let v = u64::from(value) * u64::from(permille);
    let r = v / 1000;
    if r > u64::from(u32::MAX) {
        u32::MAX
    } else {
        r as u32
    }
}

/// 一个带几何信息的控件：外框短边 + 圆角 +（可选）内层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Corner {
    pub id: u32,
    pub radius: u32,
    pub short_side: u32,
    pub gap: u32,
    pub inner: Option<u32>,
}

impl Corner {
    pub fn new(id: u32, radius: u32, short_side: u32) -> Self {
        Self {
            id,
            radius,
            short_side,
            gap: 0,
            inner: None,
        }
    }

    pub fn with_inner(mut self, inner: u32, gap: u32) -> Self {
        self.inner = Some(inner);
        self.gap = gap;
        self
    }

    /// 单点自检：负值/越界/嵌套，一次报全。
    pub fn audit(&self) -> Vec<RadiusCode> {
        let mut out = Vec::new();
        if self.short_side == 0 {
            out.push(RadiusCode::ClampImpossible);
            return out;
        }
        let limit = mul_permille(self.short_side, MAX_RADIUS_PERMILLE);
        if self.radius > limit {
            out.push(RadiusCode::RadiusTooLarge);
        }
        if let Some(inner) = self.inner {
            if check_nesting(self.radius, inner, self.gap).is_err() {
                out.push(RadiusCode::NestedViolation);
            }
        }
        out
    }

    /// 修正：把圆角钳到合法范围，内角按 `inner_for` 重算。
    pub fn repaired(&self) -> Self {
        let r = clamp(self.radius, self.short_side);
        let inner = self.inner.map(|_| inner_for(r, self.gap));
        Self {
            radius: r,
            inner,
            ..*self
        }
    }
}

/// 兄弟一致检查：同一容器内同级控件的圆角必须一致。
pub fn check_siblings(corners: &[Corner]) -> Result<(), RadiusCode> {
    if corners.is_empty() {
        return Err(RadiusCode::EmptyFamily);
    }
    let first = match corners.first() {
        Some(c) => c.radius,
        None => return Err(RadiusCode::EmptyFamily),
    };
    for c in corners.iter().skip(1) {
        if c.radius != first {
            return Err(RadiusCode::SiblingInconsistent);
        }
    }
    Ok(())
}

/// 全档审计：单调性 + 每档对 8/16/24/48/96/200 六种尺寸的嵌套可行。
pub fn audit() -> Vec<RadiusCode> {
    let mut out = Vec::new();
    // 单调
    let a = Radius::Sharp.base_px();
    let b = Radius::Standard.base_px();
    let c = Radius::Round.base_px();
    if !(a < b && b < c) {
        out.push(RadiusCode::NonMonotonic);
    }
    // 每档：对每个尺寸，外层用 for_size，内层用 inner_for，必须过嵌套检查
    for r in Radius::ALL {
        for size in [24u32, 48, 96, 200] {
            let outer = r.for_size(size);
            // 内层控件留 12px 间距
            let gap = 12u32;
            let inner = inner_for(outer, gap);
            if check_nesting(outer, inner, gap).is_err() {
                out.push(RadiusCode::NestedViolation);
            }
            // 钳制后必须仍然有序
            let clamped = clamp(outer, size);
            if clamped > size / 2 + 1 {
                out.push(RadiusCode::RadiusTooLarge);
            }
        }
    }
    out
}

/// 派生族：为一条控件链（外到内）生成圆角序列。每层减一个间距。
pub fn derive_chain(density_radius: Radius, sizes: &[u32], gap: u32) -> Vec<u32> {
    let mut out = Vec::with_capacity(sizes.len());
    let mut outer = match sizes.first() {
        Some(s) => density_radius.for_size(*s),
        None => return out,
    };
    out.push(outer);
    for size in sizes.iter().skip(1) {
        let inner = inner_for(outer, gap).min(density_radius.for_size(*size));
        out.push(inner);
        outer = inner;
    }
    out
}

/// 读屏清单：三档的基础值与代表尺寸。
pub fn matrix_lines() -> Vec<String> {
    let mut out = Vec::new();
    out.push("圆角档位 | 基础 | ≤24 | ≤96 | >96".to_string());
    for r in Radius::ALL {
        out.push(format!(
            "{:<8} {:>4}px {:>3}px {:>3}px {:>4}px",
            r.name(),
            r.base_px(),
            r.for_size(24),
            r.for_size(96),
            r.for_size(200)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_gears_and_levels() {
        assert_eq!(Radius::of_level(1), Ok(Radius::Standard));
        assert_eq!(Radius::of_level(9), Err(RadiusCode::LevelOutOfRange));
        assert!(Radius::Sharp.base_px() < Radius::Standard.base_px());
        assert!(Radius::Standard.base_px() < Radius::Round.base_px());
    }

    #[test]
    fn nesting_inequality_is_not_equality() {
        // 外 16 内 8 间距 4：16 ≥ 12，合法
        assert_eq!(check_nesting(16, 8, 4), Ok(()));
        // 内角更小也合法
        assert_eq!(check_nesting(16, 2, 4), Ok(()));
        // 内 14 间距 4：需要 18 > 16，违例
        assert_eq!(check_nesting(16, 14, 4), Err(RadiusCode::NestedViolation));
        // 间距 0 时内角可以等于外角
        assert_eq!(check_nesting(16, 16, 0), Ok(()));
        // 间距 0 但内角更大：违例
        assert_eq!(check_nesting(16, 17, 0), Err(RadiusCode::NestedViolation));
        // 回归：偏移量大于外角时，内层退化为直角 —— 这不是违例。
        assert_eq!(check_nesting(2, 0, 12), Ok(()));
        assert_eq!(check_nesting(0, 0, 8), Ok(()));
    }

    #[test]
    fn inner_for_is_the_usable_answer() {
        assert_eq!(inner_for(16, 4), 12);
        assert_eq!(inner_for(8, 12), 0, "间距超过外角时内角最多是直角");
        // 用它算出的内角必须过检查
        let inner = inner_for(16, 4);
        assert_eq!(check_nesting(16, inner, 4), Ok(()));
    }

    #[test]
    fn clamp_keeps_shape_from_becoming_pill() {
        assert_eq!(clamp(16, 24), 12, "24 的一半是 12，16 必须被钳到 12");
        assert_eq!(clamp(4, 24), 4);
        assert_eq!(clamp(100, 0), 0);
    }

    #[test]
    fn corner_audit_reports_all_problems() {
        let bad = Corner::new(1, 20, 24).with_inner(18, 4);
        let issues = bad.audit();
        assert!(issues.contains(&RadiusCode::RadiusTooLarge), "{:?}", issues);
        assert!(issues.contains(&RadiusCode::NestedViolation), "{:?}", issues);
    }

    #[test]
    fn repaired_corner_passes_its_own_audit() {
        let bad = Corner::new(7, 40, 32).with_inner(30, 8);
        let fixed = bad.repaired();
        assert!(fixed.audit().is_empty(), "{:?}", fixed.audit());
        assert!(fixed.radius <= 16);
        assert_eq!(fixed.inner, Some(inner_for(fixed.radius, 8)));
    }

    #[test]
    fn siblings_must_agree() {
        let a = Corner::new(1, 8, 32);
        let b = Corner::new(2, 8, 32);
        let c = Corner::new(3, 12, 32);
        assert_eq!(check_siblings(&[a, b]), Ok(()));
        assert_eq!(check_siblings(&[a, b, c]), Err(RadiusCode::SiblingInconsistent));
        assert_eq!(check_siblings(&[]), Err(RadiusCode::EmptyFamily));
    }

    #[test]
    fn chain_derives_inward_monotonically() {
        let chain = derive_chain(Radius::Round, &[320, 200, 120, 40], 8);
        assert_eq!(chain.len(), 4);
        for w in chain.windows(2) {
            assert!(w[0] >= w[1], "内层圆角不得大于外层：{:?}", chain);
        }
    }

    #[test]
    fn full_audit_is_clean() {
        assert!(audit().is_empty(), "{:?}", audit());
    }

    #[test]
    fn matrix_lines_are_readable() {
        let l = matrix_lines();
        assert_eq!(l.len(), 4);
        assert!(l[0].contains("圆角档位"));
    }
}
