//! ves418_s22reserve — VE-F4178 · T 域演进预留（组级）
//! T 域组级演进预留声明：预留面四口（新语言族接入位/新机翻引擎位/新地区规则源位/新方向书写系统位
//! ——与 F4158 域级四口呼应细化）；预留纪律（预留不预写——只留接口与断言位）；
//! 预写越界→回退；预留位被占→迁移评审；登记缺→补登；四口均含无障碍适配位说明（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0F60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsCode {
    Ok = 0,
    PreWrite = SEG_BASE + 0x01,  // 0x0F61
    SlotTaken = SEG_BASE + 0x02, // 0x0F62
    NotRegistered = SEG_BASE + 0x03, // 0x0F63
}

/// 四口（组级细化，与 F4158 域级呼应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupSlot { LangFamily = 0, MtEngine = 1, RegionRuleSrc = 2, ScriptSystem = 3 }

impl GroupSlot {
    pub const ALL: [GroupSlot; 4] = [
        GroupSlot::LangFamily, GroupSlot::MtEngine, GroupSlot::RegionRuleSrc, GroupSlot::ScriptSystem];
    pub fn name(self) -> &'static str {
        match self {
            GroupSlot::LangFamily => "g-lang-family", GroupSlot::MtEngine => "g-mt-engine",
            GroupSlot::RegionRuleSrc => "g-region-rule-src", GroupSlot::ScriptSystem => "g-script-system",
        }
    }
    /// 呼应的域级口名（F4158）。
    pub fn domain_counterpart(self) -> &'static str {
        match self {
            GroupSlot::LangFamily => "new-lang-family", GroupSlot::MtEngine => "new-mt-engine",
            GroupSlot::RegionRuleSrc => "new-region-rule-src", GroupSlot::ScriptSystem => "new-script-system",
        }
    }
}

/// 接口位。
pub struct GroupStub {
    pub slot: GroupSlot,
    pub signature: Option<&'static str>, // 接口签名（None = 空口合法）
    pub assertion: bool,                 // 断言位
    pub a11y_note: bool,                 // 无障碍适配位说明（域本色）
    pub taken_by: Option<&'static str>,
}

impl GroupStub {
    /// 留接口合规：有签名 + 断言位 + a11y 说明；空口（None 签名）也合规（预留不预写）。
    pub fn compliant(&self) -> bool {
        (self.signature.is_none() || self.assertion) && self.a11y_note
    }
    /// 预写越界：签名含实现体标记。
    pub fn prewritten(&self) -> bool {
        self.signature.map_or(false, |s| s.contains("impl-body"))
    }
}

/// 组级预留册。
pub struct GroupReserve {
    pub stubs: Vec<GroupStub>,
    pub rollbacks: Vec<&'static str>,
    pub reviews: Vec<&'static str>,
    pub extra_regs: Vec<&'static str>, // 补登
}

impl GroupReserve {
    pub fn new() -> Self {
        GroupReserve {
            stubs: GroupSlot::ALL.iter().map(|s| GroupStub {
                slot: *s, signature: None, assertion: false, a11y_note: true, taken_by: None }).collect(),
            rollbacks: Vec::new(), reviews: Vec::new(), extra_regs: Vec::new(),
        }
    }

    pub fn get_mut(&mut self, s: GroupSlot) -> Option<&mut GroupStub> {
        self.stubs.iter_mut().find(|x| x.slot == s)
    }
    pub fn get(&self, s: GroupSlot) -> Option<&GroupStub> { self.stubs.iter().find(|x| x.slot == s) }

    /// 留接口。
    pub fn stub(&mut self, s: GroupSlot, sig: &'static str, assertion: bool) -> bool {
        match self.get_mut(s) {
            Some(x) if x.taken_by.is_none() => { x.signature = Some(sig); x.assertion = assertion; true }
            _ => false,
        }
    }

    /// 预写越界回退（扫描全口）。
    pub fn audit_prewrite(&mut self) -> usize {
        let mut n = 0;
        for x in self.stubs.iter_mut() {
            if x.prewritten() {
                x.signature = None;
                x.assertion = false;
                self.rollbacks.push(x.slot.name());
                n += 1;
            }
        }
        n
    }

    /// 占口 → 迁移评审。
    pub fn occupy(&mut self, s: GroupSlot, who: &'static str) -> Result<(), RsCode> {
        match self.get_mut(s) {
            Some(x) => {
                if x.taken_by.is_some() { self.reviews.push("migration-review"); Err(RsCode::SlotTaken) }
                else { x.taken_by = Some(who); Ok(()) }
            }
            None => Err(RsCode::NotRegistered),
        }
    }

    /// 补登（域级增口呼应）。
    pub fn register_extra(&mut self, name: &'static str) -> bool {
        if self.extra_regs.contains(&name) { return false; }
        self.extra_regs.push(name);
        true
    }

    /// 全册核验。
    pub fn all_compliant(&self) -> bool { self.stubs.iter().all(|x| x.compliant() && !x.prewritten()) }

    /// 域级呼应：四口均有对应域级口名。
    pub fn domain_echo_ok(&self) -> bool {
        GroupSlot::ALL.iter().all(|s| !s.domain_counterpart().is_empty())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves418_checks() -> CheckSet {
    let mut set = CheckSet::new("ves418");
    let mut rs = GroupReserve::new();

    // C418-01 四口建册（全空口合规）。
    chk(&mut set, "C418-01", rs.stubs.len() == 4 && rs.all_compliant(), "四口空置合规");

    // C418-02 留接口 + 断言位。
    chk(&mut set, "C418-02", rs.stub(GroupSlot::LangFamily, "fn g_lang_attach()", true)
        && rs.get(GroupSlot::LangFamily).unwrap().compliant(), "接口位合规");

    // C418-03 断言位缺 → 不合规。
    rs.stub(GroupSlot::MtEngine, "fn g_mt_if()", false);
    chk(&mut set, "C418-03", !rs.get(GroupSlot::MtEngine).unwrap().compliant(), "缺断言位红");

    // C418-04 预写越界回退。
    rs.stub(GroupSlot::ScriptSystem, "impl-body: heavy", true);
    chk(&mut set, "C418-04", rs.audit_prewrite() == 1
        && rs.rollbacks == vec!["g-script-system"]
        && rs.get(GroupSlot::ScriptSystem).unwrap().signature.is_none(), "预写回退留痕");

    // C418-05 占口与迁移评审。
    rs.occupy(GroupSlot::LangFamily, "v2").unwrap();
    chk(&mut set, "C418-05", rs.occupy(GroupSlot::LangFamily, "v3") == Err(RsCode::SlotTaken)
        && rs.reviews.len() == 1, "重占迁移评审");

    // C418-06 未建册占口拒。
    let mut rs2 = GroupReserve::new();
    rs2.stubs.clear();
    chk(&mut set, "C418-06", rs2.occupy(GroupSlot::MtEngine, "x") == Err(RsCode::NotRegistered), "未建册拒");

    // C418-07 补登幂等。
    chk(&mut set, "C418-07", rs.register_extra("g-new-ime") && !rs.register_extra("g-new-ime")
        && rs.extra_regs.len() == 1, "补登幂等");

    // C418-08 修复断言位复绿。
    rs.get_mut(GroupSlot::MtEngine).unwrap().assertion = true;
    chk(&mut set, "C418-08", rs.all_compliant(), "全册复绿");

    // C418-09 域级呼应核验。
    chk(&mut set, "C418-09", rs.domain_echo_ok()
        && GroupSlot::LangFamily.domain_counterpart() == "new-lang-family", "域级呼应");

    // C418-10 a11y 适配位说明四口必备（域本色）。
    chk(&mut set, "C418-10", rs.stubs.iter().all(|x| x.a11y_note), "四口 a11y 说明");

    // C418-11 a11y 说明缺失违规。
    rs.get_mut(GroupSlot::RegionRuleSrc).unwrap().a11y_note = false;
    chk(&mut set, "C418-11", !rs.all_compliant(), "缺 a11y 说明红");

    // C418-12 四口名单单源。
    chk(&mut set, "C418-12", GroupSlot::ALL.len() == 4
        && GroupSlot::ALL[0].name() == "g-lang-family"
        && GroupSlot::ALL[3].name() == "g-script-system", "四口单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves418_smoke() {
        let set = run_ves418_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves418 red: {:?}", red);
    }
}
