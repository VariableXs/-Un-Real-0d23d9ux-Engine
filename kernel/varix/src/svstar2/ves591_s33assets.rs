//! VE-F4357 · 资产总账（U08 测试与资产组）
//! 职责：U08 组产出资产总账——八类登记造册（规则库 / 词典 / 判例 /
//! 语料 / 基准 / 示例 / 文档 / 扩展点）+ 每类资产含版本、健康度、
//! 归属组三字段（缺任一字段断言拦截入账）+ 失效标记补制（总账里
//! 挂"失效"而无接替资产的必须补制或降级下架）+ 移交失配对拍（移出
//! 组与接收组双账比对）+ 无障碍资产类（读屏词条 / 高对比样张）单列
//! 域色。
//! 段位：0x4560（续表 idx591）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4560;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsCode {
    /// 三字段缺一（入账拦截）。
    FieldMissing = SEG_BASE,
    /// 失效无接替（补制拦截）。
    DeadNoHeir = SEG_BASE + 0x01,
    /// 移交失配（对拍拦截）。
    HandoffMismatch = SEG_BASE + 0x02,
}

/// 八类资产。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsKind {
    RuleLib,
    Dict,
    Precedent,
    Corpus,
    Bench,
    Example,
    Doc,
    /// 扩展点——含无障碍扩展点子类。
    ExtPoint,
}

/// 健康度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health {
    /// 在役。
    Alive,
    /// 失效——必须有接替资产。
    Dead,
}

/// 一项资产登记。
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: &'static str,
    pub kind: AsKind,
    /// 字段一：版本号。
    pub ver: u32,
    /// 字段二：健康度。
    pub health: Health,
    /// 字段三：归属组号（0 = 未归属，视为缺字段）。
    pub group: u32,
    /// 接替资产名（失效资产必须指认）。
    pub heir: Option<&'static str>,
    /// 无障碍专项标记（域色单列）。
    pub a11y: bool,
    /// 已移交出组。
    pub handed_off: bool,
}

/// 资产总账。
#[derive(Default)]
pub struct Ledger {
    pub items: Vec<Asset>,
    /// 接收组侧账（移交对拍用）：资产名 -> 组号。
    pub recv: Vec<(&'static str, u32)>,
    pub audited: u32,
}

impl Ledger {
    pub fn new() -> Self {
        Default::default()
    }

    /// 入账：三字段（版本 / 健康度 / 归属组）缺任一即拦截。
    pub fn enroll(&mut self, a: Asset) -> Result<(), AsCode> {
        if a.ver == 0 || a.group == 0 {
            return Err(AsCode::FieldMissing);
        }
        if a.health == Health::Dead && a.heir.is_none() {
            return Err(AsCode::DeadNoHeir);
        }
        self.items.push(a);
        Ok(())
    }

    /// 失效标记：标记失效必须同时指认接替资产，否则拒（补制拦截）。
    pub fn mark_dead(&mut self, name: &'static str, heir: &'static str) -> Result<(), AsCode> {
        let it = self.items.iter_mut().find(|x| x.name == name).ok_or(AsCode::DeadNoHeir)?;
        if heir.is_empty() {
            return Err(AsCode::DeadNoHeir);
        }
        it.health = Health::Dead;
        it.heir = Some(heir);
        Ok(())
    }

    /// 失效无接替扫描：账内任何 Dead 且无 heir 的残留。
    pub fn dead_unheired(&self) -> usize {
        self.items.iter().filter(|x| x.health == Health::Dead && x.heir.is_none()).count()
    }

    /// 移交登记：出组侧记录。
    pub fn handoff(&mut self, name: &'static str, to_group: u32) -> Result<(), AsCode> {
        let it = self.items.iter_mut().find(|x| x.name == name).ok_or(AsCode::HandoffMismatch)?;
        if to_group == 0 {
            return Err(AsCode::HandoffMismatch);
        }
        it.handed_off = true;
        it.group = to_group;
        self.audited += 1;
        Ok(())
    }

    /// 接收组侧入账（对拍另一侧）。
    pub fn recv_enroll(&mut self, name: &'static str, group: u32) {
        self.recv.push((name, group));
    }

    /// 移交失配对拍：出组侧与接收侧逐名比对（组号一致且两侧都在）。
    pub fn handoff_mismatches(&self) -> Vec<&'static str> {
        let mut bad = Vec::new();
        for it in self.items.iter().filter(|x| x.handed_off) {
            match self.recv.iter().find(|(n, _)| *n == it.name) {
                Some((_, g)) if *g == it.group => {}
                _ => bad.push(it.name),
            }
        }
        bad
    }

    /// 无障碍资产单列清单（域色核心）。
    pub fn a11y_assets(&self) -> Vec<&'static str> {
        self.items.iter().filter(|x| x.a11y).map(|x| x.name).collect()
    }

    /// 八类覆盖断言：每类至少一项（总账造册完整）。
    pub fn kinds_covered(&self) -> bool {
        use AsKind::*;
        [RuleLib, Dict, Precedent, Corpus, Bench, Example, Doc, ExtPoint]
            .iter()
            .all(|k| self.items.iter().any(|x| x.kind == *k))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves591_checks() -> CheckSet {
    let mut set = CheckSet::new("ves591");
    let mut lg = Ledger::new();

    // C591-01 三字段齐备入账。
    let ok = lg.enroll(Asset { name: "规则库A", kind: AsKind::RuleLib, ver: 3, health: Health::Alive, group: 8, heir: None, a11y: false, handed_off: false });
    chk(&mut set, "C591-01", ok.is_ok(), "齐备入账");

    // C591-02 缺版本拦截。
    chk(&mut set, "C591-02", lg.enroll(Asset { name: "X", kind: AsKind::Dict, ver: 0, health: Health::Alive, group: 8, heir: None, a11y: false, handed_off: false })
        == Err(AsCode::FieldMissing), "缺版本拦截");

    // C591-03 缺归属组拦截（group=0 视为缺字段）。
    chk(&mut set, "C591-03", lg.enroll(Asset { name: "Y", kind: AsKind::Dict, ver: 1, health: Health::Alive, group: 0, heir: None, a11y: false, handed_off: false })
        == Err(AsCode::FieldMissing), "缺归属拦截");

    // C591-04 失效无接替入账拦截。
    chk(&mut set, "C591-04", lg.enroll(Asset { name: "Z", kind: AsKind::Corpus, ver: 1, health: Health::Dead, group: 8, heir: None, a11y: false, handed_off: false })
        == Err(AsCode::DeadNoHeir), "失效无接替拦");

    // C591-05 失效标记须指认接替。
    let r = lg.mark_dead("规则库A", "规则库B");
    chk(&mut set, "C591-05", r.is_ok() && lg.items[0].heir == Some("规则库B"), "失效指认接替");

    // C591-06 空接替名拒。
    chk(&mut set, "C591-06", lg.mark_dead("规则库A", "").is_err(), "空接替拒");

    // C591-07 造册八类覆盖。
    let _ = lg.enroll(Asset { name: "词典A", kind: AsKind::Dict, ver: 2, health: Health::Alive, group: 8, heir: None, a11y: true, handed_off: false });
    let _ = lg.enroll(Asset { name: "判例A", kind: AsKind::Precedent, ver: 1, health: Health::Alive, group: 8, heir: None, a11y: false, handed_off: false });
    let _ = lg.enroll(Asset { name: "语料A", kind: AsKind::Corpus, ver: 4, health: Health::Alive, group: 8, heir: None, a11y: false, handed_off: false });
    let _ = lg.enroll(Asset { name: "基准A", kind: AsKind::Bench, ver: 1, health: Health::Alive, group: 8, heir: None, a11y: false, handed_off: false });
    let _ = lg.enroll(Asset { name: "示例A", kind: AsKind::Example, ver: 2, health: Health::Alive, group: 8, heir: None, a11y: false, handed_off: false });
    let _ = lg.enroll(Asset { name: "文档A", kind: AsKind::Doc, ver: 5, health: Health::Alive, group: 8, heir: None, a11y: false, handed_off: false });
    let _ = lg.enroll(Asset { name: "扩展点A", kind: AsKind::ExtPoint, ver: 1, health: Health::Alive, group: 8, heir: None, a11y: true, handed_off: false });
    chk(&mut set, "C591-07", lg.kinds_covered(), "八类全覆盖");

    // C591-08 无障碍资产单列（域色）。
    chk(&mut set, "C591-08", lg.a11y_assets() == alloc::vec!["词典A", "扩展点A"], "无障碍单列");

    // C591-09 移交登记与对拍一致。
    let _ = lg.handoff("语料A", 9);
    lg.recv_enroll("语料A", 9);
    chk(&mut set, "C591-09", lg.handoff_mismatches().is_empty(), "对拍一致");

    // C591-10 接收侧缺账 = 失配。
    let _ = lg.handoff("示例A", 7);
    chk(&mut set, "C591-10", lg.handoff_mismatches() == alloc::vec!["示例A"], "缺账失配");

    // C591-11 接收组号不一致 = 失配。
    lg.recv_enroll("示例A", 8);
    chk(&mut set, "C591-11", lg.handoff_mismatches() == alloc::vec!["示例A"], "组号失配");
    lg.recv.pop();
    lg.recv_enroll("示例A", 7);
    chk(&mut set, "C591-11b", lg.handoff_mismatches().is_empty(), "更正后一致");

    // C591-12 非法组号移交拒 + 审计计数。
    chk(&mut set, "C591-12", lg.handoff("基准A", 0).is_err() && lg.audited == 2, "非法拒+计数");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_gate() {
        let mut lg = Ledger::new();
        assert_eq!(lg.enroll(Asset { name: "A", kind: AsKind::Dict, ver: 0, health: Health::Alive, group: 8, heir: None, a11y: false, handed_off: false }),
            Err(AsCode::FieldMissing));
    }

    #[test]
    fn dead_gate() {
        let mut lg = Ledger::new();
        let _ = lg.enroll(Asset { name: "A", kind: AsKind::Doc, ver: 1, health: Health::Alive, group: 8, heir: None, a11y: false, handed_off: false });
        assert!(lg.mark_dead("A", "B").is_ok());
        assert!(lg.dead_unheired() == 0);
    }
}
