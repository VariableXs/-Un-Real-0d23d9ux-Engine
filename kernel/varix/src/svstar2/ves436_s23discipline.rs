//! ves436_s23discipline — VE-F4196 · T 域变更纪律与承诺条款
//! 纪律：T 域冻结后变更纪律（变更申请→影响面评估→双签→哈希更新→通告五步——步步留痕）
//! +承诺条款（对外双承诺状态终审+违诺处置）；
//! 越步变更→作废+回退；违诺→处置+通告；评估缺→拒批；
//! 变更评估含多语言无障碍影响面专列（域本色）；承诺不含用户数据越界条款。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1340;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    Ok = 0,
    StepSkipped = SEG_BASE + 0x01, // 0x1341 越步
    AssessMissing = SEG_BASE + 0x02, // 0x1342 评估缺
    Breach = SEG_BASE + 0x03,      // 0x1343 违诺
    Rejected = SEG_BASE + 0x04,    // 0x1344 拒批
}

/// 变更五步。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeStep { Apply = 0, Assess = 1, DualSign = 2, HashUpdate = 3, Notify = 4 }

impl ChangeStep {
    pub const ALL: [ChangeStep; 5] = [
        ChangeStep::Apply, ChangeStep::Assess, ChangeStep::DualSign, ChangeStep::HashUpdate, ChangeStep::Notify];
}

/// 影响面评估（含多语言无障碍专列——域本色）。
pub struct ImpactAssess { pub scope: u32, pub a11y_impact: bool, pub a11y_note: bool }

/// 变更纪律册。
pub struct DisciplineBook {
    pub steps_done: Vec<ChangeStep>,
    pub assess: Option<ImpactAssess>,
    pub signers: [&'static str; 2],
    pub hash_updated: bool,
    pub notified: bool,
    pub voided: u32,
    pub reverted: u32,
    pub filed: u32,
}

impl DisciplineBook {
    pub fn new() -> Self {
        DisciplineBook { steps_done: Vec::new(), assess: None, signers: ["", ""], hash_updated: false,
            notified: false, voided: 0, reverted: 0, filed: 0 }
    }
    pub fn set_assess(&mut self, scope: u32, a11y_impact: bool, a11y_note: bool) {
        self.assess = Some(ImpactAssess { scope, a11y_impact, a11y_note });
    }
    pub fn do_step(&mut self, s: ChangeStep) -> Result<(), DcCode> {
        // 步序强制：只允许做紧邻下一步（越步→作废+回退）。
        let idx = s as usize;
        if idx != self.steps_done.len() {
            self.voided += 1;
            self.reverted += 1;
            self.steps_done.clear();
            return Err(DcCode::StepSkipped);
        }
        // 评估步前置：评估未做→拒批（Apply 后必须先评估再走后续步）。
        if idx >= 2 && self.assess.is_none() {
            self.filed += 1;
            return Err(DcCode::AssessMissing);
        }
        // 评估缺无障碍专列说明→拒批（域本色）。
        if idx >= 2 {
            if let Some(a) = &self.assess {
                if a.a11y_impact && !a.a11y_note { self.filed += 1; return Err(DcCode::AssessMissing); }
            }
        }
        if s == ChangeStep::DualSign && (self.signers[0].is_empty() || self.signers[1].is_empty()) {
            return Err(DcCode::Rejected);
        }
        self.steps_done.push(s);
        if s == ChangeStep::HashUpdate { self.hash_updated = true; }
        if s == ChangeStep::Notify { self.notified = true; }
        Ok(())
    }
    pub fn complete(&self) -> bool { self.steps_done.len() == 5 && self.hash_updated && self.notified }
}

/// 双承诺台账（对外双承诺状态终审+违诺处置）。
pub struct PromiseBook {
    pub promises: Vec<(&'static str, bool)>, // (承诺名, 守诺)
    pub breaches: u32,
    pub notices: u32,
}

impl PromiseBook {
    pub fn new() -> Self { PromiseBook { promises: Vec::new(), breaches: 0, notices: 0 } }
    pub fn put(&mut self, name: &'static str, kept: bool) {
        for p in self.promises.iter_mut() { if p.0 == name { p.1 = kept; return; } }
        self.promises.push((name, kept));
    }
    /// 双承诺终审：两条承诺全在册且全守诺。
    pub fn dual_ok(&self) -> bool {
        self.promises.len() == 2 && self.promises.iter().all(|(_, k)| *k)
    }
    /// 违诺处置：违诺→处置+通告（无用户数据越界条款复述）。
    pub fn audit(&mut self) -> Result<(), DcCode> {
        let broken: Vec<&'static str> = self.promises.iter().filter(|(_, k)| !k).map(|(n, _)| *n).collect();
        if broken.is_empty() { return Ok(()); }
        self.breaches += broken.len() as u32;
        self.notices += broken.len() as u32;
        Err(DcCode::Breach)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves436_checks() -> CheckSet {
    let mut set = CheckSet::new("ves436");
    let mut bk = DisciplineBook::new();

    // C436-01 越步（直接做第三步）→作废+回退。
    chk(&mut set, "C436-01", bk.do_step(ChangeStep::DualSign) == Err(DcCode::StepSkipped)
        && bk.voided == 1 && bk.reverted == 1 && bk.steps_done.is_empty(), "越步作废回退");

    // C436-02 步序归零后按序走第一步。
    chk(&mut set, "C436-02", bk.do_step(ChangeStep::Apply).is_ok(), "申请步");

    // C436-03 评估缺→拒批（跳过评估直接双签）。
    chk(&mut set, "C436-03", bk.do_step(ChangeStep::DualSign) == Err(DcCode::AssessMissing)
        && bk.filed == 1, "评估缺拒批");

    // C436-04 评估有 a11y 影响但缺专列说明→拒（域本色）。
    bk.set_assess(3, true, false);
    chk(&mut set, "C436-04", bk.do_step(ChangeStep::Assess).is_ok()
        && bk.do_step(ChangeStep::DualSign) == Err(DcCode::AssessMissing), "a11y 专列缺拒");

    // C436-05 补专列说明+双签齐→五步走通。
    bk.set_assess(3, true, true);
    bk.signers = ["wb-a", "wb-b"];
    for s in [ChangeStep::DualSign, ChangeStep::HashUpdate, ChangeStep::Notify] {
        bk.do_step(s).unwrap();
    }
    chk(&mut set, "C436-05", bk.complete(), "五步走通");

    // C436-06 哈希更新与通告留痕断言。
    chk(&mut set, "C436-06", bk.hash_updated && bk.notified, "哈希通告留痕");

    // C436-07 双签缺一→拒。
    let mut bk2 = DisciplineBook::new();
    bk2.set_assess(1, false, false);
    bk2.do_step(ChangeStep::Apply).unwrap();
    bk2.do_step(ChangeStep::Assess).unwrap();
    bk2.signers = ["wb-a", ""];
    chk(&mut set, "C436-07", bk2.do_step(ChangeStep::DualSign) == Err(DcCode::Rejected), "双签缺一拒");

    // C436-08 承诺台账：双承诺全守。
    let mut pb = PromiseBook::new();
    pb.put("frozen-stability", true);
    pb.put("a11y-floor", true);
    chk(&mut set, "C436-08", pb.dual_ok() && pb.audit().is_ok(), "双守诺");

    // C436-09 违诺→处置+通告。
    pb.put("a11y-floor", false);
    chk(&mut set, "C436-09", pb.audit() == Err(DcCode::Breach)
        && pb.breaches == 1 && pb.notices == 1, "违诺处置通告");

    // C436-10 违诺整改复诺复绿。
    pb.put("a11y-floor", true);
    chk(&mut set, "C436-10", pb.audit().is_ok(), "复诺复绿");

    // C436-11 五步单源断言。
    chk(&mut set, "C436-11", ChangeStep::ALL.len() == 5
        && ChangeStep::ALL[0] == ChangeStep::Apply
        && ChangeStep::ALL[4] == ChangeStep::Notify, "五步单源");

    // C436-12 无用户数据越界条款：台账不含该条（复述断言）。
    chk(&mut set, "C436-12", !pb.promises.iter().any(|(n, _)| *n == "user-data-outbound"), "无越界条款");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves436_smoke() {
        let set = run_ves436_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves436 red: {:?}", red);
    }
}
