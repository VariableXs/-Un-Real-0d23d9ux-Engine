//! VE-F4358 · 演进预留（U08 测试与资产组）
//! 职责：U08 十年演进预留——三方向（跨项目模板 / 即服务化 / 规则
//! 自动合成）只预留接口不写死实现（十年口径：接口稳定 + 实现空白）
//! + 私自实现回退拦截（任何方向提前写实实现即回退）+ 接口冻结评审
//! （冻结期内接口签名不得改动）+ 接口过期废弃流程（预留接口超过
//! 时效未启用须走废弃评审下架，不留僵尸接口）+ 预留清单与总账对拍。
//! 段位：0x45E0（续表 idx592）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x45E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvCode {
    /// 私自实现（回退拦截）。
    RogueImpl = SEG_BASE,
    /// 冻结期内改签名。
    FrozenTampered = SEG_BASE + 0x01,
    /// 接口过期未走废弃评审。
    StaleNoRetire = SEG_BASE + 0x02,
}

/// 三方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvDir {
    /// 跨项目模板。
    Template,
    /// 即服务化。
    AsService,
    /// 规则自动合成。
    AutoSynth,
}

/// 一个预留接口。
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub name: &'static str,
    pub dir: EvDir,
    /// 签名指纹（冻结评审后不得改动）。
    pub sig: u64,
    /// 冻结评审通过（冻结期内签名不可改）。
    pub frozen: bool,
    /// 私自实现标记（提前写实 = 回退拦截）。
    pub implemented: bool,
    /// 预留起始周期号（过期判定用）。
    pub since_cycle: u32,
    /// 已走废弃评审下架。
    pub retired: bool,
}

/// 预留台账。
#[derive(Default)]
pub struct Reserve {
    pub slots: Vec<Slot>,
    /// 接口时效（周期数）：超期未启用须废弃评审。
    pub ttl_cycles: u32,
    pub retires: u32,
}

impl Reserve {
    pub fn new(ttl: u32) -> Self {
        Reserve { slots: Vec::new(), ttl_cycles: ttl, retires: 0 }
    }

    /// 预留登记。
    pub fn add(&mut self, s: Slot) -> Result<(), EvCode> {
        if s.implemented {
            return Err(EvCode::RogueImpl);
        }
        if self.slots.iter().any(|x| x.name == s.name) {
            return Err(EvCode::RogueImpl);
        }
        self.slots.push(s);
        Ok(())
    }

    /// 冻结评审：通过后签名指纹锁定。
    pub fn freeze(&mut self, name: &'static str) -> Result<(), EvCode> {
        let s = self.slots.iter_mut().find(|x| x.name == name).ok_or(EvCode::FrozenTampered)?;
        s.frozen = true;
        Ok(())
    }

    /// 冻结期内改签名 = 拦截（未冻结则允许修订）。
    pub fn revise_sig(&mut self, name: &'static str, sig: u64) -> Result<(), EvCode> {
        let s = self.slots.iter_mut().find(|x| x.name == name).ok_or(EvCode::FrozenTampered)?;
        if s.frozen {
            return Err(EvCode::FrozenTampered);
        }
        s.sig = sig;
        Ok(())
    }

    /// 私自实现登记：写实即回退拦截（返回错误要求回退）。
    pub fn try_implement(&self, name: &'static str) -> Result<(), EvCode> {
        let s = self.slots.iter().find(|x| x.name == name).ok_or(EvCode::RogueImpl)?;
        if s.implemented {
            return Err(EvCode::RogueImpl);
        }
        // 十年口径：实现一律拒——方向只留接口。
        let _ = s;
        Err(EvCode::RogueImpl)
    }

    /// 过期扫描：超 ttl 且未启用未废弃的接口。
    pub fn stale_slots(&self, now: u32) -> Vec<&'static str> {
        self.slots
            .iter()
            .filter(|s| !s.retired && !s.implemented && now.saturating_sub(s.since_cycle) > self.ttl_cycles)
            .map(|s| s.name)
            .collect()
    }

    /// 废弃评审下架。
    pub fn retire(&mut self, name: &'static str, now: u32) -> Result<(), EvCode> {
        let stale = self.stale_slots(now);
        let s = self.slots.iter_mut().find(|x| x.name == name).ok_or(EvCode::StaleNoRetire)?;
        if !stale.contains(&name) {
            return Err(EvCode::StaleNoRetire);
        }
        s.retired = true;
        self.retires += 1;
        Ok(())
    }

    /// 三方向覆盖断言。
    pub fn dirs_covered(&self) -> bool {
        [EvDir::Template, EvDir::AsService, EvDir::AutoSynth]
            .iter()
            .all(|d| self.slots.iter().any(|s| s.dir == *d && !s.retired))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves592_checks() -> CheckSet {
    let mut set = CheckSet::new("ves592");
    let mut rv = Reserve::new(20);

    // C592-01 三方向预留登记。
    let _ = rv.add(Slot { name: "tpl_if", dir: EvDir::Template, sig: 0xA1, frozen: false, implemented: false, since_cycle: 1, retired: false });
    let _ = rv.add(Slot { name: "svc_if", dir: EvDir::AsService, sig: 0xB2, frozen: false, implemented: false, since_cycle: 1, retired: false });
    let _ = rv.add(Slot { name: "syn_if", dir: EvDir::AutoSynth, sig: 0xC3, frozen: false, implemented: false, since_cycle: 1, retired: false });
    chk(&mut set, "C592-01", rv.dirs_covered() && rv.slots.len() == 3, "三方向预留");

    // C592-02 私自实现入账拦截（登记即拒）。
    chk(&mut set, "C592-02", rv.add(Slot { name: "rogue", dir: EvDir::Template, sig: 1, frozen: false, implemented: true, since_cycle: 1, retired: false })
        == Err(EvCode::RogueImpl), "私有实现入账拒");

    // C592-03 实现申请一律拒（十年口径：只留接口）。
    chk(&mut set, "C592-03", rv.try_implement("tpl_if") == Err(EvCode::RogueImpl), "实现申请拒");

    // C592-04 未冻结可修订签名。
    chk(&mut set, "C592-04", rv.revise_sig("tpl_if", 0xA9).is_ok(), "未冻结可修订");

    // C592-05 冻结评审后改签名拦截。
    let _ = rv.freeze("tpl_if");
    chk(&mut set, "C592-05", rv.revise_sig("tpl_if", 0xFF) == Err(EvCode::FrozenTampered), "冻结改签拦");

    // C592-06 未冻结接口可正常修订（冻结不影响他人）。
    chk(&mut set, "C592-06", rv.revise_sig("svc_if", 0xB9).is_ok(), "他口可修订");

    // C592-07 未过期不列过期清单。
    chk(&mut set, "C592-07", rv.stale_slots(15).is_empty(), "期内不过期");

    // C592-08 超期列出过期清单。
    chk(&mut set, "C592-08", rv.stale_slots(30) == alloc::vec!["svc_if", "syn_if"], "超期列出");

    // C592-09 未过期废弃评审拒。
    chk(&mut set, "C592-09", rv.retire("syn_if", 15).is_err(), "未过期废退拒");

    // C592-10 过期废弃评审通过。
    chk(&mut set, "C592-10", rv.retire("svc_if", 30).is_ok() && rv.retires == 1, "过期废退过");

    // C592-11 废弃后不再列过期（僵尸清理闭环）。
    chk(&mut set, "C592-11", rv.stale_slots(100) == alloc::vec!["syn_if"], "废退后清");

    // C592-12 重名预留拒 + 剩余方向覆盖仍真。
    chk(&mut set, "C592-12", rv.add(Slot { name: "tpl_if", dir: EvDir::Template, sig: 1, frozen: false, implemented: false, since_cycle: 2, retired: false }).is_err()
        && rv.dirs_covered(), "重名拒+覆盖");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_gate() {
        let mut rv = Reserve::new(10);
        let _ = rv.add(Slot { name: "a", dir: EvDir::Template, sig: 1, frozen: false, implemented: false, since_cycle: 0, retired: false });
        let _ = rv.freeze("a");
        assert_eq!(rv.revise_sig("a", 2), Err(EvCode::FrozenTampered));
    }

    #[test]
    fn stale_retire() {
        let mut rv = Reserve::new(5);
        let _ = rv.add(Slot { name: "a", dir: EvDir::AsService, sig: 1, frozen: false, implemented: false, since_cycle: 0, retired: false });
        assert!(rv.stale_slots(6) == alloc::vec!["a"]);
        assert!(rv.retire("a", 6).is_ok());
        assert!(rv.stale_slots(6).is_empty());
    }
}
