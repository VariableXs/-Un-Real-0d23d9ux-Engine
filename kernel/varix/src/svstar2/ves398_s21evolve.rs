//! ves398_s21evolve — VE-F4158 · i18n 演进预留
//! T 域预留面声明四口（新语言族接入位/新地区规则源位/新机翻引擎位/新方向书写系统位）；
//! 预留纪律（预留不预写——只留接口与断言位）；域级增口登记；
//! 预写越界→回退；口被占→迁移评审；登记缺→补登；四口均含无障碍适配位说明（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0060;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvCode {
    Ok = 0,
    PreWriteBreach = SEG_BASE + 0x01,  // 0x0061 预写越界
    SlotOccupied = SEG_BASE + 0x02,    // 0x0062 口被占
    RegisterMissing = SEG_BASE + 0x03, // 0x0063 登记缺
}

/// 四口预留位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReserveSlot {
    NewLangFamily = 0,     // 新语言族接入位
    NewRegionRuleSrc = 1,  // 新地区规则源位
    NewMtEngine = 2,       // 新机翻引擎位
    NewScriptSystem = 3,   // 新方向书写系统位
}

impl ReserveSlot {
    pub const ALL: [ReserveSlot; 4] = [
        ReserveSlot::NewLangFamily, ReserveSlot::NewRegionRuleSrc,
        ReserveSlot::NewMtEngine, ReserveSlot::NewScriptSystem];
    pub fn name(self) -> &'static str {
        match self {
            ReserveSlot::NewLangFamily => "new-lang-family",
            ReserveSlot::NewRegionRuleSrc => "new-region-rule-src",
            ReserveSlot::NewMtEngine => "new-mt-engine",
            ReserveSlot::NewScriptSystem => "new-script-system",
        }
    }
}

/// 接口位：只留签名与断言位，不得预写实现（红线）。
pub struct IfaceStub {
    pub slot: ReserveSlot,
    pub signature: &'static str,     // 接口签名（空 = 未留）
    pub assertion_site: bool,        // 断言位就位
    pub a11y_adapter_note: bool,     // 无障碍适配位说明（域本色——必须 true）
    pub occupied_by: Option<&'static str>, // 口被占时的占用者
}

impl IfaceStub {
    /// 预留合规：留接口 + 留断言位 + 无障碍适配说明，且无实现预写。
    pub fn compliant(&self) -> bool {
        !self.signature.is_empty() && self.assertion_site && self.a11y_adapter_note
            && self.occupied_by.is_none()
    }
}

/// 预留登记册。
pub struct ReserveBook {
    pub stubs: Vec<IfaceStub>,
    pub registrations: Vec<&'static str>, // 域级增口登记
    pub rollbacks: Vec<&'static str>,     // 预写越界回退留痕
    pub reviews: Vec<&'static str>,       // 迁移评审留痕
}

impl ReserveBook {
    pub fn new() -> Self {
        ReserveBook { stubs: Vec::new(), registrations: Vec::new(), rollbacks: Vec::new(), reviews: Vec::new() }
    }

    /// 建四口（空口登记——登记缺→补登）。
    pub fn init_slots(&mut self) {
        for s in ReserveSlot::ALL.iter() {
            if !self.stubs.iter().any(|x| x.slot == *s) {
                self.stubs.push(IfaceStub {
                    slot: *s, signature: "", assertion_site: false,
                    a11y_adapter_note: true, occupied_by: None,
                });
            }
        }
    }

    pub fn entry(&mut self, s: ReserveSlot) -> Option<&mut IfaceStub> {
        self.stubs.iter_mut().find(|x| x.slot == s)
    }
    pub fn get(&self, s: ReserveSlot) -> Option<&IfaceStub> {
        self.stubs.iter().find(|x| x.slot == s)
    }

    /// 留接口位（只签名与断言位）。
    pub fn stub_iface(&mut self, s: ReserveSlot, sig: &'static str, assertion: bool) -> bool {
        match self.entry(s) {
            Some(e) if e.occupied_by.is_none() => {
                e.signature = sig;
                e.assertion_site = assertion;
                true
            }
            _ => false,
        }
    }

    /// 预写越界检测：口上出现实现体（signature 含 "impl-body"）→ 强制回退。
    pub fn audit_prewrite(&mut self) -> usize {
        let mut n = 0;
        for e in self.stubs.iter_mut() {
            if e.signature.contains("impl-body") {
                e.signature = "";
                e.assertion_site = false;
                self.rollbacks.push(e.slot.name());
                n += 1;
            }
        }
        n
    }

    /// 口被占 → 迁移评审。
    pub fn occupy(&mut self, s: ReserveSlot, who: &'static str) -> Result<(), EvCode> {
        match self.entry(s) {
            Some(e) => {
                if e.occupied_by.is_some() {
                    self.reviews.push("slot-migration-review");
                    Err(EvCode::SlotOccupied)
                } else {
                    e.occupied_by = Some(who);
                    Ok(())
                }
            }
            None => Err(EvCode::RegisterMissing),
        }
    }

    /// 域级增口登记（幂等）。
    pub fn register_extra(&mut self, name: &'static str) -> bool {
        if self.registrations.contains(&name) { return false; }
        self.registrations.push(name);
        true
    }

    /// 登记完备：四口全登记 + 均合规（空口允许——预留不预写）。
    pub fn audit(&self) -> Vec<&'static str> {
        ReserveSlot::ALL.iter()
            .filter(|s| match self.get(**s) {
                None => true,
                Some(e) => !e.compliant() && e.occupied_by.is_none(),
            })
            .map(|s| s.name()).collect()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves398_checks() -> CheckSet {
    let mut set = CheckSet::new("ves398");
    let mut bk = ReserveBook::new();

    // C398-01 四口建册（空口登记）。
    bk.init_slots();
    chk(&mut set, "C398-01", bk.stubs.len() == 4, "四口登记");

    // C398-02 留接口位（签名 + 断言位 + a11y 适配说明）。
    chk(&mut set, "C398-02", bk.stub_iface(ReserveSlot::NewLangFamily, "fn attach_lang_family(sig)", true)
        && bk.get(ReserveSlot::NewLangFamily).unwrap().compliant(), "接口位合规");

    // C398-03 预留不预写：空口合法（不留实现）。
    chk(&mut set, "C398-03", bk.get(ReserveSlot::NewMtEngine).unwrap().signature.is_empty()
        && bk.audit().len() == 3, "空口合法余三口待留");

    // C398-04 预写越界 → 回退留痕。
    bk.stub_iface(ReserveSlot::NewScriptSystem, "impl-body: heavy logic", false);
    let n = bk.audit_prewrite();
    chk(&mut set, "C398-04", n == 1 && bk.rollbacks == vec!["new-script-system"]
        && bk.get(ReserveSlot::NewScriptSystem).unwrap().signature.is_empty(), "预写回退留痕");

    // C398-05 域级增口登记（幂等）。
    chk(&mut set, "C398-05", bk.register_extra("new-ime-family") && !bk.register_extra("new-ime-family")
        && bk.registrations.len() == 1, "增口登记幂等");

    // C398-06 口被占 → 迁移评审。
    bk.occupy(ReserveSlot::NewLangFamily, "lang-fam-v2").unwrap();
    chk(&mut set, "C398-06", bk.occupy(ReserveSlot::NewLangFamily, "lang-fam-v3")
        == Err(EvCode::SlotOccupied) && bk.reviews.len() == 1, "重占触发迁移评审");

    // C398-07 未登记口 → 登记缺。
    let mut bk2 = ReserveBook::new();
    chk(&mut set, "C398-07", bk2.occupy(ReserveSlot::NewMtEngine, "mt-x") == Err(EvCode::RegisterMissing), "未建册占用拒");

    // C398-08 四口全留接口后审计归零。
    bk.stub_iface(ReserveSlot::NewRegionRuleSrc, "fn region_rule_src(id)", true);
    bk.stub_iface(ReserveSlot::NewMtEngine, "fn mt_engine_if()", true);
    bk.stub_iface(ReserveSlot::NewScriptSystem, "fn script_dir_if()", true);
    chk(&mut set, "C398-08", bk.audit().is_empty(), "四口全合规");

    // C398-09 a11y 适配位说明（域本色）四口必备。
    chk(&mut set, "C398-09", bk.stubs.iter().all(|e| e.a11y_adapter_note), "四口 a11y 适配位");

    // C398-10 四口名单单源。
    chk(&mut set, "C398-10", ReserveSlot::ALL.len() == 4
        && ReserveSlot::ALL[0].name() == "new-lang-family"
        && ReserveSlot::ALL[3].name() == "new-script-system", "四口单源");

    // C398-11 断言位缺 → 审计红。
    bk.entry(ReserveSlot::NewMtEngine).unwrap().assertion_site = false;
    chk(&mut set, "C398-11", bk.audit() == vec!["new-mt-engine"], "缺断言位检出");

    // C398-12 建册幂等（重复 init 不增口）。
    bk.init_slots();
    chk(&mut set, "C398-12", bk.stubs.len() == 4, "建册幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves398_smoke() {
        let set = run_ves398_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves398 red: {:?}", red);
    }

    #[test]
    fn ves398_no_a11y_note_fails() {
        let s = IfaceStub { slot: ReserveSlot::NewMtEngine, signature: "f", assertion_site: true, a11y_adapter_note: false, occupied_by: None };
        assert!(!s.compliant());
    }
}
