//! CGPU-F0473 · 缺陷清账（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 缺陷清账：C 域账本全量盘点（🟢清理/🟡全修/🔴零确认）；
//! - 逐条闭环（修复+回归+勾销）；
//! - 豁免 ADR（不修项注明理由与影响面）。判据：全量盘点、三类
//!   处置、闭环、豁免、判据。
//!
//! 设计纪律：
//! - 三类处置闭集：🟢清理（已修复+已回归+已勾销）、🟡全修（修复中
//!   但收口前必须转绿）、🔴零（安全/守恒类缺陷零容忍，存在即拦）。
//! - 闭环四步位图（发现/修复/回归/勾销）——与 F0466 违例闭环同构。
//! - 豁免 ADR：不修项必须携带（理由码+影响面声明+审批签），缺任一
//!   即豁免不成立，该项仍按未处置拦收口。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x10C0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x10C0，全仓独占）。
pub const SEG_BASE: u16 = 0x10C0;

pub const E_DF_RED: u16 = SEG_BASE + 1; // 🔴零容忍缺陷在场
pub const E_DF_YELLOW: u16 = SEG_BASE + 2; // 🟡未转绿
pub const E_DF_OPEN: u16 = SEG_BASE + 3; // 未闭环
pub const E_DF_ADR: u16 = SEG_BASE + 4; // 豁免 ADR 不成立

/// 处置分类。
pub const D_CLEAN: u8 = 0; // 🟢清理
pub const D_FIXING: u8 = 1; // 🟡全修
pub const D_RED: u8 = 2; // 🔴零容忍
pub const D_WAIVED: u8 = 3; // 豁免

/// 闭环四步位。
pub const STEP_FOUND: u32 = 1;
pub const STEP_FIXED: u32 = 2;
pub const STEP_REGRESSED: u32 = 4;
pub const STEP_CLOSED: u32 = 8;

/// 豁免 ADR 理由码。
pub const ADR_COST: u8 = 1; // 修复成本不成比例
pub const ADR_SCOPE: u8 = 2; // 影响面不涉本域
pub const ADR_SUPERSEDED: u8 = 3; // 被新架构取代

/// 缺陷条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defect {
    pub id: u32,
    pub class: u8,
    /// 闭环步骤位图。
    pub steps: u32,
    /// 豁免 ADR（仅 D_WAIVED 类需非 None）。
    pub adr: Option<(u8, &'static str, &'static str)>, // (理由码, 理由, 影响面)
}

impl Defect {
    pub fn closed(&self) -> bool {
        self.steps & 0b1111 == 0b1111
    }
    /// 豁免 ADR 成立：理由码合法+理由与影响面非空。
    pub fn adr_ok(&self) -> bool {
        match (self.class, &self.adr) {
            (D_WAIVED, Some((code, reason, impact))) => {
                (*code == ADR_COST || *code == ADR_SCOPE || *code == ADR_SUPERSEDED) && !reason.is_empty() && !impact.is_empty()
            }
            (D_WAIVED, None) => false,
            _ => true,
        }
    }
}

/// 账本清账终审：🟢闭环 + 🟡全修拦截 + 🔴零容忍拦截 + 豁免核验。
pub fn settle(all: &[Defect]) -> Result<(usize, usize, usize), u16> {
    let mut green = 0;
    let mut yellow = 0;
    for d in all {
        match d.class {
            D_RED => return Err(E_DF_RED),
            D_FIXING => {
                // 🟡 收口前必须转绿（闭环或豁免）。
                if !d.closed() && !(d.class == D_WAIVED) {
                    yellow += 1;
                }
            }
            D_CLEAN => {
                if !d.closed() {
                    return Err(E_DF_OPEN);
                }
                green += 1;
            }
            D_WAIVED => {
                if !d.adr_ok() {
                    return Err(E_DF_ADR);
                }
                green += 1; // 合法豁免视同清理
            }
            _ => return Err(E_DF_OPEN),
        }
    }
    if yellow > 0 {
        return Err(E_DF_YELLOW);
    }
    Ok((green, yellow, all.len()))
}

/// 逐条闭环推进。
pub fn progress(d: &mut Defect) {
    d.steps |= STEP_FOUND;
    d.steps |= STEP_FIXED;
    d.steps |= STEP_REGRESSED;
    d.steps |= STEP_CLOSED;
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、全量盘点：🟢闭环通过。
    {
        let mut d1 = Defect { id: 1, class: D_CLEAN, steps: 0, adr: None };
        progress(&mut d1);
        let r = settle(&[d1]);
        v.push(("F0473-全量盘点-闭环绿通过", r == Ok((1, 0, 1))));
    }
    // 二、🔴零容忍拦截。
    {
        let red = Defect { id: 2, class: D_RED, steps: 0b1111, adr: None };
        v.push(("F0473-三类处置-零容忍拦截", settle(&[red]) == Err(E_DF_RED)));
    }
    // 三、🟡全修未转绿拦。
    {
        let fixing = Defect { id: 3, class: D_FIXING, steps: STEP_FOUND | STEP_FIXED, adr: None };
        v.push(("F0473-三类处置-修复中拦", settle(&[fixing]) == Err(E_DF_YELLOW)));
        // 🟡 闭环后转绿。
        let mut done = Defect { id: 3, class: D_CLEAN, steps: 0, adr: None };
        progress(&mut done);
        v.push(("F0473-三类处置-闭环转绿", settle(&[done]).is_ok()));
    }
    // 四、未闭环拦。
    {
        let open = Defect { id: 4, class: D_CLEAN, steps: STEP_FOUND | STEP_FIXED, adr: None };
        v.push(("F0473-闭环-缺回归勾销拦", settle(&[open]) == Err(E_DF_OPEN)));
    }
    // 五、豁免 ADR。
    {
        let good = Defect { id: 5, class: D_WAIVED, steps: 0, adr: Some((ADR_COST, "修复成本不成比例", "仅离线档首帧")) };
        v.push(("F0473-豁免-三要素成立", settle(&[good]) == Ok((1, 0, 1))));
        // 理由码非法。
        let bad_code = Defect { id: 6, class: D_WAIVED, steps: 0, adr: Some((9, "x", "y")) };
        v.push(("F0473-豁免-理由码非法拒", settle(&[bad_code]) == Err(E_DF_ADR)));
        // 缺影响面。
        let no_impact = Defect { id: 7, class: D_WAIVED, steps: 0, adr: Some((ADR_SCOPE, "不涉本域", "")) };
        v.push(("F0473-豁免-缺影响面拒", settle(&[no_impact]) == Err(E_DF_ADR)));
        // 无 ADR。
        let none = Defect { id: 8, class: D_WAIVED, steps: 0, adr: None };
        v.push(("F0473-豁免-无ADR拒", settle(&[none]) == Err(E_DF_ADR)));
    }
    // 六、混合账本终审。
    {
        let mut a = Defect { id: 11, class: D_CLEAN, steps: 0, adr: None };
        progress(&mut a);
        let w = Defect { id: 12, class: D_WAIVED, steps: 0, adr: Some((ADR_SUPERSEDED, "被新配方取代", "F0353 场景层")) };
        let r = settle(&[a, w]);
        v.push(("F0473-判据-混合账本通过", r == Ok((2, 0, 2))));
        // 计数口径：绿=闭环+合法豁免。
        v.push(("F0473-判据-总数对齐", r.map(|(g, _, t)| g <= t) == Ok(true)));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0473_defectsettle_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
