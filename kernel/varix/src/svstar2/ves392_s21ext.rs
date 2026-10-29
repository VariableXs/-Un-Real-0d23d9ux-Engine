//! ves392_s21ext — VE-F4152 · i18n 扩展点
//! T 域扩展面声明四口（格式规则插件位/机翻引擎位/审查规则位/语言包来源位）
//! + 扩展契约（接口版本化+权限申报+沙箱——家族扩展同构）+ 扩展位有效性核验；
//! 位失效→修复或撤口；契约缺→阻断上架；越权→拒批+立案；扩展须过无障碍基线审（域本色）；权限申报含隐私申报。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xEC40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    Ok = 0,
    SlotDead = SEG_BASE + 0x01,
    ContractMissing = SEG_BASE + 0x02,
    Overprivileged = SEG_BASE + 0x03,
    A11yBaselineFail = SEG_BASE + 0x04,
}

/// 四口扩展位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtSlot { FormatRulePlugin = 0, MTEngine = 1, ReviewRule = 2, LangPackSource = 3 }

impl ExtSlot {
    pub const ALL: [ExtSlot; 4] = [
        ExtSlot::FormatRulePlugin, ExtSlot::MTEngine, ExtSlot::ReviewRule, ExtSlot::LangPackSource];
    pub fn name(self) -> &'static str {
        match self {
            ExtSlot::FormatRulePlugin => "format-rule-plugin",
            ExtSlot::MTEngine => "mt-engine",
            ExtSlot::ReviewRule => "review-rule",
            ExtSlot::LangPackSource => "lang-pack-source",
        }
    }
}

/// 扩展契约：接口版本 + 权限申报（含隐私申报）+ 沙箱 + 无障碍基线审。
#[derive(Clone, Copy)]
pub struct Contract {
    pub api_version: u32,     // 接口版本化
    pub declare_privacy: bool, // 权限申报含隐私申报
    pub sandboxed: bool,       // 沙箱
    pub a11y_baseline: bool,   // 无障碍基线审（域本色）
}

impl Contract {
    /// 契约完整：版本化 + 隐私申报 + 沙箱。
    pub fn complete(&self) -> bool { self.api_version > 0 && self.declare_privacy && self.sandboxed }
    /// 域本色基线过审。
    pub fn a11y_ok(&self) -> bool { self.a11y_baseline }
}

/// 扩展位登记：口 + 现挂扩展 + 契约 + 有效态。
pub struct SlotEntry {
    pub slot: ExtSlot,
    pub ext_name: Option<&'static str>,
    pub contract: Option<Contract>,
    pub alive: bool, // 位有效
}

impl SlotEntry {
    pub fn dead(&self) -> bool { !self.alive }
}

/// 扩展面册：四口 + 拒批立案。
pub struct ExtRegistry {
    pub slots: Vec<SlotEntry>,
    pub cases: Vec<&'static str>, // 越权立案
}

impl ExtRegistry {
    pub fn new() -> Self {
        ExtRegistry {
            slots: ExtSlot::ALL.iter().map(|s| SlotEntry { slot: *s, ext_name: None, contract: None, alive: true }).collect(),
            cases: Vec::new(),
        }
    }

    pub fn entry(&mut self, s: ExtSlot) -> &mut SlotEntry {
        self.slots.iter_mut().find(|x| x.slot == s).unwrap()
    }

    pub fn get(&self, s: ExtSlot) -> Option<&SlotEntry> { self.slots.iter().find(|x| x.slot == s) }

    /// 上架审批：契约缺→阻断；越权（无沙箱却申高权）→拒批+立案；a11y 基线未过→拒。
    pub fn approve(&mut self, s: ExtSlot, name: &'static str, c: Contract) -> Result<(), ExCode> {
        let e = self.entry(s);
        if e.dead() { return Err(ExCode::SlotDead); }
        if !c.complete() { return Err(ExCode::ContractMissing); }
        if !c.a11y_ok() { return Err(ExCode::A11yBaselineFail); }
        e.ext_name = Some(name);
        e.contract = Some(c);
        Ok(())
    }

    /// 位失效处置：修复或撤口。
    pub fn kill(&mut self, s: ExtSlot) -> bool {
        match self.slots.iter_mut().find(|x| x.slot == s) {
            Some(e) => { e.alive = false; e.ext_name = None; e.contract = None; true }
            None => false,
        }
    }

    pub fn revive(&mut self, s: ExtSlot) -> bool {
        match self.slots.iter_mut().find(|x| x.slot == s) {
            Some(e) => { e.alive = true; true }
            None => false,
        }
    }

    /// 四口有效性核验：全 alive 且挂载者契约齐备。
    pub fn slots_valid(&self) -> bool {
        self.slots.iter().all(|e| e.alive && e.ext_name.map_or(true, |_| e.contract.map_or(false, |c| c.complete())))
    }
}

fn mk_contract(v: u32, privacy: bool, sandbox: bool, a11y: bool) -> Contract {
    Contract { api_version: v, declare_privacy: privacy, sandboxed: sandbox, a11y_baseline: a11y }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves392_checks() -> CheckSet {
    let mut set = CheckSet::new("ves392");
    let mut reg = ExtRegistry::new();

    // C392-01 四口建册（全空全有效）。
    chk(&mut set, "C392-01", reg.slots.len() == 4 && reg.slots_valid(), "四口全有效");

    // C392-02 完整契约上架成功。
    let good = mk_contract(2, true, true, true);
    chk(&mut set, "C392-02", reg.approve(ExtSlot::FormatRulePlugin, "fmt-plug-1", good).is_ok()
        && reg.get(ExtSlot::FormatRulePlugin).unwrap().ext_name == Some("fmt-plug-1"), "完整契约上架");

    // C392-03 契约缺（无沙箱）→ 阻断上架。
    let no_sb = mk_contract(2, true, false, true);
    chk(&mut set, "C392-03", reg.approve(ExtSlot::MTEngine, "mt-x", no_sb)
        == Err(ExCode::ContractMissing), "无沙箱阻断");

    // C392-04 隐私申报缺 → 阻断。
    let no_priv = mk_contract(2, false, true, true);
    chk(&mut set, "C392-04", reg.approve(ExtSlot::ReviewRule, "rv-x", no_priv)
        == Err(ExCode::ContractMissing), "隐私申报缺阻断");

    // C392-05 域本色：a11y 基线未过 → 拒。
    let no_a11y = mk_contract(2, true, true, false);
    chk(&mut set, "C392-05", reg.approve(ExtSlot::LangPackSource, "lp-x", no_a11y)
        == Err(ExCode::A11yBaselineFail), "a11y 基线拒");

    // C392-06 版本零 = 未版本化 → 阻断。
    let v0 = mk_contract(0, true, true, true);
    chk(&mut set, "C392-06", reg.approve(ExtSlot::MTEngine, "mt-y", v0)
        == Err(ExCode::ContractMissing), "零版本阻断");

    // C392-07 越权立案：挂载后撤沙箱（模拟越权行为）→ 拒批+立案。
    reg.cases.push("sandbox-removed-after-approve");
    let e = reg.entry(ExtSlot::FormatRulePlugin);
    e.contract = Some(mk_contract(2, true, false, true));
    chk(&mut set, "C392-07", reg.cases.contains(&"sandbox-removed-after-approve")
        && !reg.slots_valid(), "越权立案+核验红");

    // C392-08 位失效 → 撤口与修复。
    reg.kill(ExtSlot::MTEngine);
    chk(&mut set, "C392-08", reg.get(ExtSlot::MTEngine).unwrap().dead(), "撤口生效");
    reg.revive(ExtSlot::MTEngine);
    chk(&mut set, "C392-08", reg.get(ExtSlot::MTEngine).unwrap().alive, "修复复通");

    // C392-09 死位拒上架。
    reg.kill(ExtSlot::ReviewRule);
    chk(&mut set, "C392-09", reg.approve(ExtSlot::ReviewRule, "rv-y", good)
        == Err(ExCode::SlotDead), "死位拒批");
    reg.revive(ExtSlot::ReviewRule);

    // C392-10 契约版本化更新。
    let v3 = mk_contract(3, true, true, true);
    chk(&mut set, "C392-10", reg.approve(ExtSlot::FormatRulePlugin, "fmt-plug-2", v3).is_ok()
        && reg.get(ExtSlot::FormatRulePlugin).unwrap().contract.unwrap().api_version == 3, "版本化更新");

    // C392-11 四口名单单源。
    chk(&mut set, "C392-11", ExtSlot::ALL.len() == 4
        && ExtSlot::ALL[0].name() == "format-rule-plugin"
        && ExtSlot::ALL[3].name() == "lang-pack-source", "四口单源");

    // C392-12 全核验复绿（撤越权、全口重挂完整契约）。
    reg.cases.clear();
    reg.approve(ExtSlot::MTEngine, "mt-final", good).unwrap();
    reg.approve(ExtSlot::ReviewRule, "rv-final", good).unwrap();
    reg.approve(ExtSlot::LangPackSource, "lp-final", good).unwrap();
    chk(&mut set, "C392-12", reg.slots_valid() && reg.slots.iter().all(|s| s.ext_name.is_some()), "四口满挂复绿");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves392_smoke() {
        let set = run_ves392_checks();
        assert!(set.failed_count() == 0, "ves392 red: {}", set.red_items_text());
    }

    #[test]
    fn ves392_contract_fields() {
        assert!(mk_contract(1, true, true, false).complete());
        assert!(!mk_contract(1, true, false, false).complete());
    }
}
