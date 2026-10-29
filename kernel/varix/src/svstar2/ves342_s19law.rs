//! ves342_s19law — VE-F4102 · T06 地区法规合规
//! 合规规则库×地区矩阵（数据法/消费者保护/无障碍法案三类条目化）+ 合规检查器（功能上线前跑地区规则——不过阻断上线）+ 三戳留痕（规则版本+检查时间+结论）+ 规则缺保守处理（按最严地区）+ 版本漂移重检 + ADA/EN301549 等法案条目登记（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9F40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LwCode {
    Ok = 0,
    BlockShip = SEG_BASE + 0x01,
    RuleMissing = SEG_BASE + 0x02,
    VersionDrift = SEG_BASE + 0x03,
}

/// 三类规则。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleClass { DataLaw = 0, Consumer = 1, A11yAct = 2 }

impl RuleClass {
    pub const ALL: [RuleClass; 3] = [RuleClass::DataLaw, RuleClass::Consumer, RuleClass::A11yAct];
    pub fn name(self) -> &'static str {
        match self { RuleClass::DataLaw => "data-law", RuleClass::Consumer => "consumer", RuleClass::A11yAct => "a11y-act" }
    }
}

/// 一条规则：地区 × 类 × 规则键 × 最严级。
#[derive(Clone)]
pub struct Rule { pub region: &'static str, pub class: RuleClass, pub key: &'static str, pub severity: u8 }

/// 无障碍法案条目登记（域本色）。
pub const A11Y_ACTS: [&'static str; 4] = ["ADA", "EN301549", "-section508", "WCAG-binding"];

pub struct RuleBook { pub rules: Vec<Rule>, pub acts: Vec<&'static str> }

impl RuleBook {
    pub fn new() -> Self { RuleBook { rules: Vec::new(), acts: Vec::new() } }

    pub fn add(&mut self, r: Rule) { self.rules.push(r); }

    pub fn register_act(&mut self, act: &'static str) {
        if !self.acts.contains(&act) { self.acts.push(act); }
    }

    /// 地区某类规则集。
    pub fn of(&self, region: &str, class: RuleClass) -> Vec<&Rule> {
        self.rules.iter().filter(|r| r.region == region && r.class == class).collect()
    }

    /// 最严地区兜底：任一功能在规则缺地区按最严地区（severity 最大集）检查。
    pub fn strictest_region(&self, class: RuleClass) -> &'static str {
        let mut best: Option<(&'static str, usize)> = None;
        for r in self.rules.iter() {
            if r.class != class { continue; }
            let n = self.of(r.region, class).len();
            let sev = self.rules.iter().filter(|x| x.region == r.region && x.class == class)
                .map(|x| x.severity).max().unwrap_or(0);
            let score = n * 10 + sev as usize;
            if best.map_or(true, |(_, bs)| score > bs) { best = Some((r.region, score)); }
        }
        best.map(|(s, _)| s).unwrap_or("GLOBAL")
    }

    pub fn act_count(&self) -> usize { self.acts.len() }
}

/// 合规检查器：功能声明满足的规则键集 vs 地区要求 → 缺即阻断。
pub struct ComplianceCheck { pub region: &'static str, pub feature: &'static str, pub satisfied: Vec<&'static str> }

/// 三戳留痕：规则版本 + 检查时间 + 结论。
#[derive(Clone)]
pub struct Stamp { pub rule_version: u32, pub checked_at: u64, pub passed: bool }

pub struct StampBook { pub stamps: Vec<Stamp> }

impl StampBook {
    pub fn new() -> Self { StampBook { stamps: Vec::new() } }

    pub fn stamp(&mut self, rule_version: u32, checked_at: u64, passed: bool) {
        self.stamps.push(Stamp { rule_version, checked_at, passed });
    }

    /// 版本漂移重检：最新戳版本 != 当前库版本 → 需重检。
    pub fn needs_recheck(&self, current_version: u32) -> bool {
        match self.stamps.last() {
            Some(s) => s.rule_version != current_version,
            None => true,
        }
    }

    pub fn latest_passed(&self) -> bool { self.stamps.last().map_or(false, |s| s.passed) }
}

/// 检查执行：satisfied 必须覆盖 required 键集。
pub fn run_check(required: &[&'static str], satisfied: &[&'static str]) -> Result<(), LwCode> {
    for r in required.iter() {
        if !satisfied.contains(r) { return Err(LwCode::BlockShip); }
    }
    Ok(())
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves342_checks() -> CheckSet {
    let mut set = CheckSet::new("ves342");
    let mut rb = RuleBook::new();

    // 夹具：EU 数据法严、US 消保严。
    rb.add(Rule { region: "EU", class: RuleClass::DataLaw, key: "gdpr-consent", severity: 3 });
    rb.add(Rule { region: "EU", class: RuleClass::DataLaw, key: "gdpr-dsr", severity: 3 });
    rb.add(Rule { region: "US", class: RuleClass::Consumer, key: "refund-14d", severity: 3 });
    rb.add(Rule { region: "US", class: RuleClass::A11yAct, key: "ada-web", severity: 2 });
    rb.add(Rule { region: "EU", class: RuleClass::A11yAct, key: "en301549", severity: 3 });

    // C342-01 三类规则条目化齐备。
    let three = RuleClass::ALL.iter().all(|c| rb.rules.iter().any(|r| r.class == *c));
    chk(&mut set, "C342-01", three, "三类规则均有条目");

    // C342-02 地区×类查询。
    chk(&mut set, "C342-02", rb.of("EU", RuleClass::DataLaw).len() == 2, "EU 数据法两条");

    // C342-03 合规检查通过。
    let req = ["gdpr-consent", "gdpr-dsr"];
    let ok = run_check(&req, &["gdpr-consent", "gdpr-dsr", "extra"]);
    chk(&mut set, "C342-03", ok.is_ok(), "满足集覆盖要求集");

    // C342-04 检查不过 → 阻断上线。
    chk(&mut set, "C342-04", run_check(&req, &["gdpr-consent"]) == Err(LwCode::BlockShip), "缺 gdpr-dsr => 阻断");

    // C342-05 最严地区兜底：数据法最严是 EU。
    chk(&mut set, "C342-05", rb.strictest_region(RuleClass::DataLaw) == "EU", "数据法最严 EU");

    // C342-06 规则缺地区按最严处理。
    let strictest = rb.strictest_region(RuleClass::Consumer);
    let req_c = rb.of(strictest, RuleClass::Consumer).iter().map(|r| r.key).collect::<Vec<_>>();
    let blocked = run_check(&req_c, &[]) == Err(LwCode::BlockShip);
    chk(&mut set, "C342-06", blocked, "规则缺地区按最严 => 阻断（保守）");

    // C342-07 无障碍法案条目登记（域本色）。
    for a in A11Y_ACTS.iter() { rb.register_act(a); }
    chk(&mut set, "C342-07", rb.act_count() == 4 && rb.acts.contains(&"EN301549"), "四法案登记");

    // C342-08 三戳留痕。
    let mut st = StampBook::new();
    st.stamp(3, 1_700_000_000, true);
    chk(&mut set, "C342-08", st.stamps.len() == 1 && st.latest_passed(), "三戳入库且过");

    // C342-09 版本漂移重检。
    chk(&mut set, "C342-09", st.needs_recheck(4), "库版本 4 ≠ 戳版本 3 => 重检");

    // C342-10 同版本不重检。
    chk(&mut set, "C342-10", !st.needs_recheck(3), "同版本免检");

    // C342-11 无戳即需检。
    chk(&mut set, "C342-11", StampBook::new().needs_recheck(1), "无戳 => 必检");

    // C342-12 失败戳留痕不影响最新结论口径。
    st.stamp(4, 1_700_000_100, false);
    chk(&mut set, "C342-12", !st.latest_passed() && st.stamps.len() == 2, "最新戳红 => 未过");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves342_smoke() {
        let set = run_ves342_checks();
        assert!(set.failed_count() == 0, "ves342 red: {}", set.red_items_text());
    }

    #[test]
    fn ves342_block() {
        assert_eq!(run_check(&["a"], &["b"]), Err(LwCode::BlockShip));
        assert!(run_check(&["a"], &["a", "b"]).is_ok());
    }
}
