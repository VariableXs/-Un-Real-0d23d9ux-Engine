//! CGPU-F0495 · 计时跨硬件/跨版本一致性（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 跨硬件：不同硬件档计时结论一致（口径统一，绝对值随硬件标定）；
//! - 跨版本：口径变更走 ADR（决策+理由+影响面）；
//! - 与 G06/H09 口径统一。判据：跨硬件、跨版本、口径、判据。
//!
//! 设计纪律：
//! - 口径与读数分离：口径（四段划分/单位 us/命名）对所有硬件相同；
//!   读数（各段时长）由硬件标定换算——同一帧在快慢两机上换算到
//!   标准时钟后结论一致（±带宽内）。
//! - 跨版本：口径变更必须登记 ADR（版本对+变更摘要+理由+影响面
//!   清单）；无 ADR 的口径变更判红；ADR 链单调递增。
//! - 跨域统一：G06（调度域）/H09（呈现域）与本域同概念异名，
//!   统一表逐条登记；三方名非空且互不冲突。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x11C0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x11C0，全仓独占）。
pub const SEG_BASE: u16 = 0x11C0;

pub const E_TC_HW: u16 = SEG_BASE + 1; // 跨硬件结论漂移
pub const E_TC_ADR: u16 = SEG_BASE + 2; // 无 ADR 的口径变更
pub const E_TC_UNIFY: u16 = SEG_BASE + 3; // 跨域口径未统一
pub const E_TC_VER: u16 = SEG_BASE + 4; // ADR 版本序非法

/// 标准时钟口径（us）：所有硬件读数换算到此口径再比较。
pub const STD_FRAME_BUDGET_US: u32 = 12_500;

// ---------------------------------------------------------------------------
// 一、跨硬件：标定换算后结论一致
// ---------------------------------------------------------------------------

/// 硬件档：tick 率（逻辑 tick / 标准微秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HwProfile {
    pub hw_id: u8,
    /// 换算系数（×1000 整数，如 1200 = 1.2 tick/us）。
    pub k_pm: u32,
}

/// 快慢两档样例：慢机 800‰、标准 1000‰、快机 1200‰。
pub const HW_SLOW: HwProfile = HwProfile { hw_id: 1, k_pm: 800 };
pub const HW_STD: HwProfile = HwProfile { hw_id: 2, k_pm: 1000 };
pub const HW_FAST: HwProfile = HwProfile { hw_id: 3, k_pm: 1200 };
pub const HW_FLEET: [HwProfile; 3] = [HW_SLOW, HW_STD, HW_FAST];

/// 硬件读数（本机 tick）→ 标准微秒。
pub fn to_std_us(hw_ticks: u32, hw: &HwProfile) -> u32 {
    // ticks * 1000 / k_pm（快机读数多，除回来；慢机读数少，补回来）。
    (hw_ticks.saturating_mul(1000)) / hw.k_pm.max(1)
}

/// 跨硬件一致性闸：同一帧在全部档位换算后互相一致（±2% 带）。
pub fn cross_hw_gate(frame_us_std: u32, band_pm: u32) -> Result<usize, u16> {
    if HW_FLEET.is_empty() {
        return Err(E_TC_HW);
    }
    let mut prev: Option<u32> = None;
    for hw in HW_FLEET.iter() {
        // 各档本机读数 = 标准值 × k_pm / 1000，再换算回来。
        let local = (frame_us_std.saturating_mul(hw.k_pm)) / 1000;
        let back = to_std_us(local, hw);
        // 换算往返误差：整数化损耗 ≤ 1%+band。
        let lo = frame_us_std.saturating_sub(frame_us_std * band_pm / 1000).saturating_sub(1);
        let hi = frame_us_std.saturating_add(frame_us_std * band_pm / 1000);
        if back < lo || back > hi {
            return Err(E_TC_HW);
        }
        if let Some(p) = prev {
            // 档间差 ≤ band。
            let d = if back > p { back - p } else { p - back };
            if d > frame_us_std * band_pm / 1000 {
                return Err(E_TC_HW);
            }
        }
        prev = Some(back);
    }
    Ok(HW_FLEET.len())
}

// ---------------------------------------------------------------------------
// 二、跨版本：ADR 登记
// ---------------------------------------------------------------------------

/// 口径 ADR。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdrRec {
    pub id: u32,
    /// 变更前版本（0 = 初版登记）。
    pub from_ver: u32,
    /// 变更后版本。
    pub to_ver: u32,
    /// 变更摘要。
    pub change: &'static str,
    /// 理由。
    pub rationale: &'static str,
    /// 影响面（受影响模块清单）。
    pub impact: Vec<&'static str>,
}

/// ADR 链：单调递增 + 版本衔接。
#[derive(Debug)]
pub struct AdrChain {
    pub recs: Vec<AdrRec>,
    /// 当前口径版本。
    pub cur_ver: u32,
}

impl AdrChain {
    pub fn new(init_ver: u32) -> Self {
        Self { recs: Vec::new(), cur_ver: init_ver }
    }
    /// 登记：版本必须从当前版本向前走，且四要素齐全。
    pub fn record(
        &mut self,
        from_ver: u32,
        to_ver: u32,
        change: &'static str,
        rationale: &'static str,
        impact: Vec<&'static str>,
    ) -> Result<u32, u16> {
        if from_ver != self.cur_ver || to_ver <= from_ver {
            return Err(E_TC_VER);
        }
        if change.is_empty() || rationale.is_empty() || impact.is_empty() {
            return Err(E_TC_ADR);
        }
        let id = self.recs.len() as u32 + 1;
        self.recs.push(AdrRec { id, from_ver, to_ver, change, rationale, impact });
        self.cur_ver = to_ver;
        Ok(id)
    }
    /// 口径变更合法性：变更已被 ADR 覆盖。
    pub fn covered(&self, from_ver: u32, to_ver: u32) -> Result<bool, u16> {
        if to_ver < from_ver {
            return Err(E_TC_VER);
        }
        Ok(self.recs.iter().any(|r| r.from_ver == from_ver && r.to_ver == to_ver))
    }
}

// ---------------------------------------------------------------------------
// 三、跨域口径统一：G06 / H09
// ---------------------------------------------------------------------------

/// 统一表：本域概念 → (G06 名, H09 名)。三方同义，异名即违约。
pub const UNIFY_TABLE: [(&str, &str, &str); 6] = [
    ("frame", "slot", "presented_frame"),
    ("segment", "phase_span", "present_interval"),
    ("ledger", "sched_book", "present_book"),
    ("budget_us", "slot_budget_us", "present_budget_us"),
    ("on_time", "in_slot", "within_present"),
    ("jitter", "sched_jitter", "present_jitter"),
];

/// 统一闸：三方名非空、本域概念唯一。
pub fn unify_gate() -> Result<usize, u16> {
    if UNIFY_TABLE.is_empty() {
        return Err(E_TC_UNIFY);
    }
    for (mine, g, h) in UNIFY_TABLE.iter() {
        if mine.is_empty() || g.is_empty() || h.is_empty() {
            return Err(E_TC_UNIFY);
        }
    }
    for i in 0..UNIFY_TABLE.len() {
        for j in (i + 1)..UNIFY_TABLE.len() {
            if UNIFY_TABLE[i].0 == UNIFY_TABLE[j].0 {
                return Err(E_TC_UNIFY);
            }
        }
    }
    Ok(UNIFY_TABLE.len())
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、跨硬件：三档换算一致。
    {
        v.push((
            "F0495-跨硬件-三档一致",
            cross_hw_gate(12_500, 20) == Ok(3),
        ));
        v.push(("F0495-跨硬件-换算往返", {
            // 慢机读 10_000 tick（800‰）→ 12_500us。
            to_std_us(10_000, &HW_SLOW) == 12_500
        }));
        v.push(("F0495-跨硬件-快机换算", {
            // 快机读 15_000 tick（1200‰）→ 12_500us。
            to_std_us(15_000, &HW_FAST) == 12_500
        }));
        v.push(("F0495-跨硬件-标准机恒等", to_std_us(12_500, &HW_STD) == 12_500));
        v.push(("F0495-跨硬件-机队入册", HW_FLEET.len() == 3));
    }
    // 二、跨版本 ADR。
    {
        let mut chain = AdrChain::new(1);
        v.push((
            "F0495-ADR-初版登记",
            chain.record(1, 2, "段序由三段改四段", "与 B08 相位对齐", vec!["cgb492", "cgb494"]).is_ok(),
        ));
        v.push(("F0495-ADR-版本推进", chain.cur_ver == 2));
        v.push((
            "F0495-ADR-连续登记",
            chain.record(2, 3, "单位改微秒整数", "禁浮点记账", vec!["cgb489"]).is_ok(),
        ));
        // 版本倒挂拒：from 不等于当前。
        v.push((
            "F0495-ADR-断链拒",
            chain.record(1, 5, "跳变", "不合法", vec!["x"]) == Err(E_TC_VER),
        ));
        // 版本回退拒。
        v.push((
            "F0495-ADR-回退拒",
            chain.record(3, 2, "回退", "不合法", vec!["x"]) == Err(E_TC_VER),
        ));
        // 理由缺失拒。
        v.push((
            "F0495-ADR-无理由拒",
            chain.record(3, 4, "改口径", "", vec!["x"]) == Err(E_TC_ADR),
        ));
        // 影响面缺失拒。
        v.push((
            "F0495-ADR-无影响面拒",
            chain.record(3, 4, "改口径", "对齐", Vec::new()) == Err(E_TC_ADR),
        ));
        // 覆盖查询。
        v.push(("F0495-ADR-覆盖命中", chain.covered(1, 2) == Ok(true)));
        v.push(("F0495-ADR-未覆盖", chain.covered(2, 9) == Ok(false)));
        v.push(("F0495-ADR-倒查拒", chain.covered(5, 2) == Err(E_TC_VER)));
    }
    // 三、跨域统一。
    {
        v.push(("F0495-口径-六条统一", unify_gate() == Ok(6)));
        v.push(("F0495-口径-G06帧名", UNIFY_TABLE[0].1 == "slot"));
        v.push(("F0495-口径-H09帧名", UNIFY_TABLE[0].2 == "presented_frame"));
        v.push(("F0495-口径-预算三方同名后缀", {
            let (_, g, h) = UNIFY_TABLE[3];
            g.ends_with("budget_us") && h.ends_with("budget_us")
        }));
    }
    // 四、判据自检。
    {
        v.push(("F0495-判据-标准预算入册", STD_FRAME_BUDGET_US == 12_500));
        v.push(("F0495-判据-档位系数有序", HW_SLOW.k_pm < HW_STD.k_pm && HW_STD.k_pm < HW_FAST.k_pm));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0495_timeconsist_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
