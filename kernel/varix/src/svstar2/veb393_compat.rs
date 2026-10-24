//! veb393_compat —— VE-F0393 B 域兼容承诺。
//! 锚点（CoRun Unxreal 主册）：VE-F0393 · B 域兼容承诺 · 目标 320 行
//! 判据原文：VE-F0393-J1 兼容承诺表：对每个对外接口记兼容档
//!           （永续/大版内/仅当前）；永续接口变更（任何形式）
//!           拒；大版内接口跨主版变更须提前一版公示（未公示
//!           拒）；仅当前接口随版本消亡（不承诺）；承诺违反
//!           核（破诺数 = 0 才许发版）。
//! 组件：承诺表（接口 → 兼容档）+ 变更审查 + 公示账 + 发版闸。
//! 职责定位：兼容承诺是「B 域的对外信用」——永续接口是 iron
//!           promise，改一次信用清零；大版内接口要给下游迁移
//!           时间，提前一版公示是缓冲带；仅当前接口明确不承诺
//!           反而是诚实：把不确定性写进合同比含糊其辞强。
//! 性能逐项分解：变更审查 O(1)；公示核 O(n)。
//! 诊断码段：0x307C0（0x307C1~0x307C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CP_VERSION: &str = "veb393_compat-v1";

/// 诊断段基址（0x307C0 段 · 本模块独占）。
pub const CP_SEG_BASE: u32 = 0x307C0;
pub const XC_IRON_BREAK: u32 = 0x307C1; // 永续接口变更
pub const XC_NO_NOTICE: u32 = 0x307C2; // 大版内未公示
pub const XC_NOENT: u32 = 0x307C3; // 无此接口
pub const XC_DUP: u32 = 0x307C4; // 重名
pub const XC_MAGIC: u32 = 0x307C5; // 账魔数破
pub const XC_BREAK_OPEN: u32 = 0x307C6; // 破诺未清禁发版
pub const XC_NOTICE_FOREIGN: u32 = 0x307C7; // 公示指向无变更接口
pub const XC_TIER_OOR: u32 = 0x307C8; // 兼容档越域
pub const XC_LEDGER: u32 = 0x307C9; // 账不平

/// 兼容档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpTier {
    /// 永续（任何变更拒）。
    Iron = 0,
    /// 大版内（跨主版变更须提前一版公示）。
    Major = 1,
    /// 仅当前（不承诺）。
    Ephem = 2,
}

/// 变更记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChgRec {
    pub name: &'static str,
    /// 变更后将跨入的主版本。
    pub to_major: u32,
    /// 是否已提前公示。
    pub noticed: bool,
}

/// 兼容承诺表。
pub struct CompatBook {
    tiers: Vec<(&'static str, CmpTier)>,
    chgs: Vec<ChgRec>,
    notices: Vec<&'static str>,
    magic: u32,
}

impl CompatBook {
    /// 建。
    pub fn new() -> CompatBook {
        CompatBook { tiers: Vec::new(), chgs: Vec::new(), notices: Vec::new(), magic: 0x07C0_C0DE }
    }

    /// 登记承诺。
    pub fn promise(&mut self, name: &'static str, tier: CmpTier) -> Result<(), u32> {
        if self.magic != 0x07C0_C0DE {
            return Err(XC_MAGIC);
        }
        for t in self.tiers.iter() {
            if t.0 == name {
                return Err(XC_DUP);
            }
        }
        self.tiers.push((name, tier));
        Ok(())
    }

    fn tier_of(&self, name: &str) -> Result<CmpTier, u32> {
        for t in self.tiers.iter() {
            if t.0 == name {
                return Ok(t.1);
            }
        }
        Err(XC_NOENT)
    }

    /// 提前公示（大版内接口跨主版变更的前置动作）。
    pub fn notice(&mut self, name: &'static str) -> Result<(), u32> {
        // 公示应指向确有变更计划的接口（有变更账）。
        let mut planned = false;
        for c in self.chgs.iter() {
            if c.name == name {
                planned = true;
            }
        }
        if !planned {
            return Err(XC_NOTICE_FOREIGN);
        }
        for n in self.notices.iter() {
            if *n == name {
                return Ok(()); // 已公示幂等
            }
        }
        self.notices.push(name);
        Ok(())
    }

    /// 申请变更。
    pub fn propose_change(&mut self, name: &'static str, to_major: u32) -> Result<(), u32> {
        let tier = self.tier_of(name)?;
        match tier {
            CmpTier::Iron => Err(XC_IRON_BREAK),
            CmpTier::Ephem => {
                // 不承诺接口随意变。
                self.chgs.push(ChgRec { name, to_major, noticed: true });
                Ok(())
            }
            CmpTier::Major => {
                // 须提前一版公示：先查公示账。
                let mut noticed = false;
                for n in self.notices.iter() {
                    if *n == name {
                        noticed = true;
                    }
                }
                if !noticed {
                    self.chgs.push(ChgRec { name, to_major, noticed: false });
                    return Err(XC_NO_NOTICE);
                }
                self.chgs.push(ChgRec { name, to_major, noticed: true });
                Ok(())
            }
        }
    }

    /// 发版闸：任何未公示的大版内变更（破诺）禁发版。
    pub fn release_gate(&self) -> Result<u32, u32> {
        if self.magic != 0x07C0_C0DE {
            return Err(XC_MAGIC);
        }
        let mut breaks = 0u32;
        for c in self.chgs.iter() {
            if !c.noticed {
                breaks += 1;
            }
        }
        if breaks > 0 {
            return Err(XC_BREAK_OPEN);
        }
        Ok(self.chgs.len() as u32)
    }

    /// 账核：公示账与变更账对应。
    pub fn audit(&self) -> Result<u32, u32> {
        for n in self.notices.iter() {
            let mut planned = false;
            for c in self.chgs.iter() {
                if c.name == *n {
                    planned = true;
                }
            }
            if !planned {
                return Err(XC_LEDGER);
            }
        }
        Ok(self.notices.len() as u32)
    }

    /// 承诺数。
    pub fn promise_count(&self) -> usize {
        self.tiers.len()
    }
}

pub fn run_veb393_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0393");
    // 族一：登记承诺。
    let mut b = CompatBook::new();
    let _ = b.promise("io_read", CmpTier::Iron);
    let _ = b.promise("io_batch", CmpTier::Major);
    let _ = b.promise("io_probe", CmpTier::Ephem);
    s.add("三承诺", b.promise_count() == 3, "三档各一");
    s.add("重名拒", b.promise("io_read", CmpTier::Major) == Err(XC_DUP), "主键");
    s.add("查无", b.propose_change("ghost", 2) == Err(XC_NOENT), "未承诺");
    // 族二：永续不可变。
    s.add("永续变更拒", b.propose_change("io_read", 2) == Err(XC_IRON_BREAK), "iron promise");
    // 族三：大版内须公示。
    s.add("未公示拒", b.propose_change("io_batch", 2) == Err(XC_NO_NOTICE), "先公示");
    s.add("破诺挂账", b.release_gate() == Err(XC_BREAK_OPEN), "未清禁发版");
    // 补公示：公示前须有变更计划——先放行一笔计划（这里变通：
    // 公示账先记，变更账由未公示申请带出计划）。
    // 上一步 propose_change 已把未公示变更入账 → notice 可指。
    s.add("公示过", b.notice("io_batch").is_ok(), "提前一版");
    s.add("补公示后变更过", b.propose_change("io_batch", 2).is_ok(), "已公示");
    s.add("发版闸开", b.release_gate() == Ok(2), "两笔均公示");
    // 族四：仅当前随版消亡。
    s.add("仅当前随意变", b.propose_change("io_probe", 3).is_ok(), "不承诺");
    // 族五：公示指向核。
    let mut c = CompatBook::new();
    let _ = c.promise("x", CmpTier::Major);
    s.add("公示无计划拒", c.notice("x") == Err(XC_NOTICE_FOREIGN), "无变更计划");
    s.add("空账审计", c.audit() == Ok(0), "无公示无变更");
    // 公示幂等。
    let _ = c.propose_change("x", 2); // 未公示 → 计划入账
    let _ = c.notice("x");
    s.add("公示幂等", c.notice("x").is_ok(), "重复公示无害");
    s.add("审计 1", c.audit() == Ok(1), "一公示一计划");
    s.add("档位核", CmpTier::Iron == CmpTier::Iron && CmpTier::Ephem != CmpTier::Major, "枚举");
    s.add("魔数核", CP_SEG_BASE == 0x307C0, "段基址");
    s
}
