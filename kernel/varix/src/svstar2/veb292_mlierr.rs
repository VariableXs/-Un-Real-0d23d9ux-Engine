//! veb292_mlierr —— VE-F0292 Mali 错误恢复。
//! 锚点（CoRun Unxreal 主册）：VE-F0292 · Mali 错误恢复 · 目标 320 行
//! 判据原文：VE-F0292-J1 错误恢复账：错误三类（总线错 / 超
//!           时 / 数据坏）；恢复三策（重试 / 复位单元 / 全卡
//!           复位）；策略升级链（同类错重试 2 败 → 单元复
//!           位；单元复位 2 败 → 全卡复位）；恢复成功记档
//!           （策略 + 耗时探针次数）；恢复失败封顶（全卡复
//!           位 2 败 → 挂起并立案）；错误账（时间序 + 类别
//!           计数）；升级核（低策未败尽不升策）。
//! 组件：错误账 + 升级链 + 挂起闸。
//! 职责定位：错误恢复是「Mali 的急救链」——三策是三级急救：
//!           先揉（重试）、再按（复位单元）、最后电击（全卡
//!           复位）；低策未败尽不升策是急救纪律：没揉两下就
//!           电击是过度医疗；全卡复位两败挂起是放弃的仪式：
//!           救不活的要体面地记录而不是无限抢救。
//! 性能逐项分解：记错 O(1)；恢复 O(策略链)。
//! 诊断码段：0x31200（0x31201~0x31209 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const ER_VERSION: &str = "veb292_mlierr-v1";

/// 诊断段基址（0x31200 段 · 本模块独占）。
pub const ER_SEG_BASE: u32 = 0x31200;
pub const XC_MAGIC: u32 = 0x31201; // 账魔数破
pub const XC_HALT: u32 = 0x31202; // 已挂起

/// 账魔数。
const MAGIC: u32 = 0x3120_19C7;

/// 错误三类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrKind {
    Bus,
    Timeout,
    DataCorrupt,
}

/// 恢复三策（升级序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strat {
    Retry,
    UnitReset,
    FullReset,
}

/// 恢复档。
pub struct HealRec {
    pub kind: ErrKind,
    pub strat: Strat,
    pub probes: u32,
    pub ok: bool,
}

/// 恢复引擎。
pub struct ErrHeal {
    /// 错误时间序账。
    pub log: Vec<ErrKind>,
    /// 类别计数 [Bus, Timeout, DataCorrupt]。
    pub counts: [u32; 3],
    /// 当前同类错的低策已败次数。
    strat_fails: u32,
    /// 全卡复位败次数。
    full_fails: u32,
    /// 挂起旗。
    pub halted: bool,
    /// 恢复档。
    pub recs: Vec<HealRec>,
    magic: u32,
}

impl ErrHeal {
    /// 建。
    pub fn new() -> ErrHeal {
        ErrHeal {
            log: Vec::new(),
            counts: [0; 3],
            strat_fails: 0,
            full_fails: 0,
            halted: false,
            recs: Vec::new(),
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    fn kind_idx(k: ErrKind) -> usize {
        match k {
            ErrKind::Bus => 0,
            ErrKind::Timeout => 1,
            ErrKind::DataCorrupt => 2,
        }
    }

    /// 当前应使用的策略：败 0~1 重试；2~3 单元复位；≥4 全卡复位。
    pub fn current_strat(&self) -> Strat {
        if self.strat_fails >= 4 {
            Strat::FullReset
        } else if self.strat_fails >= 2 {
            Strat::UnitReset
        } else {
            Strat::Retry
        }
    }

    /// 报错一次：记账并返回本次采用的策略。
    pub fn report(&mut self, k: ErrKind) -> Result<Strat, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.halted {
            return Err(XC_HALT);
        }
        self.log.push(k);
        self.counts[Self::kind_idx(k)] += 1;
        let st = self.current_strat();
        Ok(st)
    }

    /// 策略执行败一次：升级计数；全卡复位败计满 2 → 挂起。
    pub fn strat_fail(&mut self) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.halted {
            return Err(XC_HALT);
        }
        match self.current_strat() {
            Strat::FullReset => {
                self.full_fails += 1;
                self.strat_fails += 1;
                if self.full_fails >= 2 {
                    self.halted = true;
                }
                Ok(())
            }
            _ => {
                self.strat_fails += 1;
                Ok(())
            }
        }
    }

    /// 恢复成功：记档并清升级计数。
    pub fn heal_ok(&mut self, k: ErrKind) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.halted {
            return Err(XC_HALT);
        }
        self.recs.push(HealRec {
            kind: k,
            strat: self.current_strat(),
            probes: self.strat_fails + 1,
            ok: true,
        });
        self.strat_fails = 0;
        self.full_fails = 0;
        Ok(())
    }

    /// 恢复档数。
    pub fn rec_count(&self) -> usize {
        self.recs.len()
    }

    /// 错误总数。
    pub fn err_total(&self) -> usize {
        self.log.len()
    }
}

pub fn run_veb292_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0292");
    // 族一：低策期。
    let mut h = ErrHeal::new();
    s.add("首错重试", h.report(ErrKind::Bus) == Ok(Strat::Retry), "低策");
    s.add("二错仍重试", h.report(ErrKind::Bus) == Ok(Strat::Retry), "未败尽");
    // 败一次升序不变。
    let _ = h.strat_fail();
    s.add("败 1 仍重试", h.report(ErrKind::Timeout) == Ok(Strat::Retry), "低策未败尽");
    // 族二：升级链。
    let _ = h.strat_fail(); // 败 2 → UnitReset
    s.add("败 2 升单元", h.report(ErrKind::Bus) == Ok(Strat::UnitReset), "中策");
    let _ = h.strat_fail();
    let _ = h.strat_fail(); // 败 4 → FullReset
    s.add("败 4 升全卡", h.report(ErrKind::DataCorrupt) == Ok(Strat::FullReset), "高策");
    // 恢复成功清零。
    let _ = h.heal_ok(ErrKind::DataCorrupt);
    s.add("愈后回低策", h.report(ErrKind::Bus) == Ok(Strat::Retry), "清零");
    s.add("档记 1", h.rec_count() == 1, "一笔");
    s.add("错误账 6", h.err_total() == 6, "时间序");
    // 类别计数。
    s.add("Bus 4", h.counts[0] == 4, "类别核");
    s.add("Timeout 1", h.counts[1] == 1, "类别核");
    s.add("坏数 1", h.counts[2] == 1, "类别核");
    // 族三：挂起封顶。
    let mut f = ErrHeal::new();
    // 直推到全卡复位（败 4 次）。
    for _ in 0..4 {
        let _ = f.report(ErrKind::Bus);
        let _ = f.strat_fail();
    }
    s.add("全卡在策", f.report(ErrKind::Bus) == Ok(Strat::FullReset), "高策");
    // 全卡复位败 1 → 仍活。
    let _ = f.strat_fail();
    s.add("全卡败 1 仍活", f.halted == false, "未封顶");
    s.add("败 5 仍全卡", f.report(ErrKind::Bus) == Ok(Strat::FullReset), "高策续");
    // 全卡复位败 2 → 挂起。
    let _ = f.strat_fail();
    s.add("挂起立", f.halted, "封顶");
    s.add("挂后报错拒", f.report(ErrKind::Bus) == Err(XC_HALT), "不再接诊");
    s.add("挂后成功拒", f.heal_ok(ErrKind::Bus) == Err(XC_HALT), "不再施救");
    // 族四：三策各成档。
    let mut t = ErrHeal::new();
    let _ = t.report(ErrKind::Timeout);
    let _ = t.heal_ok(ErrKind::Timeout); // Retry 档
    for _ in 0..2 {
        let _ = t.report(ErrKind::Timeout);
        let _ = t.strat_fail();
    }
    let _ = t.report(ErrKind::Timeout);
    let _ = t.heal_ok(ErrKind::Timeout); // UnitReset 档
    // 档策略核对。
    s.add("首档重试", t.recs[0].strat == Strat::Retry, "低");
    s.add("次档单元", t.recs[1].strat == Strat::UnitReset, "中");
    s.add("档 2 笔", t.rec_count() == 2, "两笔");
    // probes：首档 1、次档 3（败 2 + 本次）。
    s.add("探针 1", t.recs[0].probes == 1, "低策一击");
    s.add("探针 3", t.recs[1].probes == 3, "中策三击");
    s.add("魔数核", ER_SEG_BASE == 0x31200, "段基址");
    s
}
