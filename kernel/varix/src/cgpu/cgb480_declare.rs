//! CGPU-F0480 · 收官宣告（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 收官宣告：C 域 160 项完整对账（F0321-F0480 逐项入册 grep 实测
//!   160/160）；九组收口汇总（各组双签+遗留清零或豁免）；域级承诺
//!   声明（五不变量/v1.0 接口十年承诺/公平指数达标/80 帧合同预算
//!   侧就绪）；向 D 域移交就绪宣告。判据：160/160 对账、九组汇总、
//!   四承诺、移交宣告、判据。
//!
//! 设计纪律：
//! - 160/160 对账：任务号区间 [F0321, F0480] 逐号在册（缺号即对账
//!   失败），对账表可重放。
//! - 九组收口汇总：每组（双签在场，遗留=0 或全豁免）→ 组绿。
//! - 四承诺结构化：五不变量/v1.0 十年/公平指数/80 帧就绪——四枚举
//!   各自带承诺文本与状态位，缺一即宣告不成立。
//! - 移交宣告：D 域侧要件（契约义务齐备+证据三件套）核验通过才可
//!   宣告就绪。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1130 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1130，全仓独占）。
pub const SEG_BASE: u16 = 0x1130;

pub const E_DC_GAP: u16 = SEG_BASE + 1; // 对账缺号
pub const E_DC_GROUP: u16 = SEG_BASE + 2; // 组收口未齐
pub const E_DC_PLEDGE: u16 = SEG_BASE + 3; // 承诺缺项
pub const E_DC_HANDOFF: u16 = SEG_BASE + 4; // 移交未就绪

/// 对账区间。
pub const RECON_LO: u32 = 321;
pub const RECON_HI: u32 = 480;
/// 160 项对账：区间内全部任务号在册。
pub fn recon_ok(registered: &[u32]) -> Result<(), u16> {
    for id in RECON_LO..=RECON_HI {
        if !registered.contains(&id) {
            return Err(E_DC_GAP);
        }
    }
    Ok(())
}

/// 组收口状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupCloseout {
    pub group: u8,
    /// 组签在场。
    pub signed: bool,
    /// 遗留缺陷数（0 或全豁免）。
    pub leftover: u32,
    /// 豁免数（leftover ≤ waived 即清零或豁免）。
    pub waived: u32,
}

impl GroupCloseout {
    pub fn green(&self) -> bool {
        self.signed && self.leftover <= self.waived
    }
}

/// 九组汇总：全绿才收官。
pub fn groups_summary(gs: &[GroupCloseout]) -> Result<(), u16> {
    if gs.len() != 9 {
        return Err(E_DC_GROUP);
    }
    if gs.iter().any(|g| !g.green()) {
        return Err(E_DC_GROUP);
    }
    Ok(())
}

/// 四承诺枚举。
pub const P_INVARIANTS: u8 = 1; // 五不变量
pub const P_TENYEAR: u8 = 2; // v1.0 接口十年承诺
pub const P_FAIRNESS: u8 = 3; // 公平指数达标
pub const P_80FPS: u8 = 4; // 80 帧合同预算侧就绪
pub const PLEDGES: [u8; 4] = [P_INVARIANTS, P_TENYEAR, P_FAIRNESS, P_80FPS];

/// 域级承诺声明。
#[derive(Debug)]
pub struct DomainPledge {
    pub status: [bool; 4],
}

impl DomainPledge {
    pub fn new() -> Self {
        Self { status: [false; 4] }
    }
    pub fn make(&mut self, p: u8) -> Result<(), u16> {
        let i = PLEDGES.iter().position(|&x| x == p).ok_or(E_DC_PLEDGE)?;
        self.status[i] = true;
        Ok(())
    }
    /// 宣告成立：四承诺全置位。
    pub fn declared(&self) -> Result<(), u16> {
        if self.status.iter().all(|&b| b) {
            Ok(())
        } else {
            Err(E_DC_PLEDGE)
        }
    }
}

/// 移交就绪宣告：要件（契约义务齐备+证据三件套）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffReady {
    pub obligations: u32, // 三对接点义务条数
    pub evid_tree: u32,
    pub evid_drills: u32,
    pub evid_baseline: u32,
}

impl HandoffReady {
    pub fn ready(&self) -> Result<(), u16> {
        if self.obligations < 3 {
            return Err(E_DC_HANDOFF);
        }
        if self.evid_tree < 16 || self.evid_drills != 5 || self.evid_baseline != 6 {
            return Err(E_DC_HANDOFF);
        }
        Ok(())
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、160/160 对账。
    {
        let full: Vec<u32> = (RECON_LO..=RECON_HI).collect();
        v.push(("F0480-对账-160项全在册", recon_ok(&full).is_ok() && full.len() == 160));
        // 缺号拦截。
        let gap: Vec<u32> = (RECON_LO..=RECON_HI).filter(|x| *x != 400).collect();
        v.push(("F0480-对账-缺号拦截", recon_ok(&gap) == Err(E_DC_GAP)));
    }
    // 二、九组收口汇总。
    {
        let all_green: Vec<GroupCloseout> = (1..=9u8)
            .map(|g| GroupCloseout { group: g, signed: true, leftover: 0, waived: 0 })
            .collect();
        v.push(("F0480-九组汇总-全绿通过", groups_summary(&all_green).is_ok()));
        // 遗留未豁免拦。
        let leftover = vec![GroupCloseout { group: 3, signed: true, leftover: 2, waived: 1 }];
        v.push(("F0480-九组汇总-遗留未豁免拦", groups_summary(&leftover) == Err(E_DC_GROUP)));
        // 无签拦。
        let unsigned = vec![GroupCloseout { group: 5, signed: false, leftover: 0, waived: 0 }];
        v.push(("F0480-九组汇总-无签拦", groups_summary(&unsigned) == Err(E_DC_GROUP)));
        // 遗留全豁免=绿（九组成套，组 7 遗留 3 全豁免）。
        let waived_all: Vec<GroupCloseout> = (1..=9u8)
            .map(|g| GroupCloseout {
                group: g,
                signed: true,
                leftover: if g == 7 { 3 } else { 0 },
                waived: if g == 7 { 3 } else { 0 },
            })
            .collect();
        v.push(("F0480-九组汇总-全豁免视绿", groups_summary(&waived_all).is_ok()));
    }
    // 三、四承诺。
    {
        let mut d = DomainPledge::new();
        d.make(P_INVARIANTS).ok();
        d.make(P_TENYEAR).ok();
        d.make(P_FAIRNESS).ok();
        v.push(("F0480-四承诺-缺一不宣告", d.declared() == Err(E_DC_PLEDGE)));
        d.make(P_80FPS).ok();
        v.push(("F0480-四承诺-齐备宣告", d.declared().is_ok()));
        // 非法承诺码。
        v.push(("F0480-四承诺-非法码拒", d.make(99) == Err(E_DC_PLEDGE)));
    }
    // 四、移交宣告。
    {
        let ready = HandoffReady { obligations: 3, evid_tree: 16, evid_drills: 5, evid_baseline: 6 };
        v.push(("F0480-移交-就绪宣告", ready.ready().is_ok()));
        let no_ob = HandoffReady { obligations: 2, evid_tree: 16, evid_drills: 5, evid_baseline: 6 };
        v.push(("F0480-移交-义务缺失拒", no_ob.ready() == Err(E_DC_HANDOFF)));
        let no_drill = HandoffReady { obligations: 3, evid_tree: 16, evid_drills: 4, evid_baseline: 6 };
        v.push(("F0480-移交-演练证据缺失拒", no_drill.ready() == Err(E_DC_HANDOFF)));
    }
    // 五、收官全流程编排：对账+九组+四承诺+移交四闸串联。
    {
        let full: Vec<u32> = (RECON_LO..=RECON_HI).collect();
        let gs: Vec<GroupCloseout> = (1..=9u8)
            .map(|g| GroupCloseout { group: g, signed: true, leftover: 0, waived: 0 })
            .collect();
        let mut d = DomainPledge::new();
        for p in PLEDGES.iter() {
            d.make(*p).ok();
        }
        let ready = HandoffReady { obligations: 3, evid_tree: 16, evid_drills: 5, evid_baseline: 6 };
        let final_ok = recon_ok(&full).is_ok()
            && groups_summary(&gs).is_ok()
            && d.declared().is_ok()
            && ready.ready().is_ok();
        v.push(("F0480-判据-四闸串联收官", final_ok));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0480_declare_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
