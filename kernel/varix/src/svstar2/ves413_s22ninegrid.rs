//! ves413_s22ninegrid — VE-F4173 · 九维汇总表与风险登记
//! T 域九维汇总表（API/测试/性能/安全/无障碍/文档/预算/扩展/观测九维各一页汇总）
//! + 风险登记册（每风险挂维度/概率/影响/缓解四列）；风险未登记→扫描补登；缓解缺→补案；
//! 汇总与资产不符→对账修正；九维含无障碍维主场位（域本色）；风险册不含用户身份信息。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0EA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NgCode {
    Ok = 0,
    RiskUnregistered = SEG_BASE + 0x01, // 0x0EA1
    MitigationMissing = SEG_BASE + 0x02, // 0x0EA2
    SheetMismatch = SEG_BASE + 0x03,    // 0x0EA3
}

/// 九维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NineDim { Api = 0, Test = 1, Perf = 2, Security = 3, A11y = 4, Doc = 5, Budget = 6, Ext = 7, Observe = 8 }

impl NineDim {
    pub const ALL: [NineDim; 9] = [
        NineDim::Api, NineDim::Test, NineDim::Perf, NineDim::Security, NineDim::A11y,
        NineDim::Doc, NineDim::Budget, NineDim::Ext, NineDim::Observe];
    pub fn name(self) -> &'static str {
        match self {
            NineDim::Api => "api", NineDim::Test => "test", NineDim::Perf => "perf",
            NineDim::Security => "security", NineDim::A11y => "a11y", NineDim::Doc => "doc",
            NineDim::Budget => "budget", NineDim::Ext => "ext", NineDim::Observe => "observe",
        }
    }
}

/// 概率 × 影响。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level { Low = 0, Mid = 1, High = 2 }

/// 风险四列（维度/概率/影响/缓解）。
#[derive(Clone, Copy)]
pub struct Risk {
    pub dim: NineDim,
    pub prob: Level,
    pub impact: Level,
    pub mitigation: Option<&'static str>, // 缓解（None = 缺）
    pub has_user_identity: bool,          // 红线：风险册不含用户身份
}

impl Risk {
    pub fn severity(&self) -> Level {
        let p = self.prob as u8;
        let i = self.impact as u8;
        match p.saturating_add(i) {
            0..=1 => Level::Low,
            2..=3 => Level::Mid,
            _ => Level::High,
        }
    }
    pub fn mitigation_ok(&self) -> bool { self.mitigation.is_some() }
    pub fn clean(&self) -> bool { !self.has_user_identity }
}

/// 九维汇总表 + 风险登记册。
pub struct NineGrid {
    pub sheets: Vec<(NineDim, bool)>, // 每维一页（bool = 与资产对账一致）
    pub risks: Vec<Risk>,
}

impl NineGrid {
    pub fn new() -> Self { NineGrid { sheets: Vec::new(), risks: Vec::new() } }

    pub fn put_sheet(&mut self, d: NineDim, consistent: bool) {
        match self.sheets.iter_mut().find(|x| x.0 == d) {
            Some(x) => x.1 = consistent,
            None => self.sheets.push((d, consistent)),
        }
    }

    /// 九页齐备且全部与资产一致。
    pub fn sheets_ok(&self) -> bool {
        NineDim::ALL.iter().all(|d| self.sheets.iter().any(|x| x.0 == *d && x.1))
    }

    /// 对账修正。
    pub fn fix_sheet(&mut self, d: NineDim) -> bool {
        match self.sheets.iter_mut().find(|x| x.0 == d) {
            Some(x) => { x.1 = true; true }
            None => false,
        }
    }

    /// 风险登记（幂等按维+描述无法区分——按序登记，扫描补登）。
    pub fn file_risk(&mut self, r: Risk) { self.risks.push(r); }

    /// 缓解缺补案。
    pub fn missing_mitigations(&self) -> Vec<usize> {
        self.risks.iter().enumerate().filter(|(_, r)| !r.mitigation_ok()).map(|(i, _)| i).collect()
    }

    pub fn add_mitigation(&mut self, idx: usize, m: &'static str) -> bool {
        match self.risks.get_mut(idx) {
            Some(r) => { r.mitigation = Some(m); true }
            None => false,
        }
    }

    /// 隐私核验：风险册不含用户身份。
    pub fn privacy_ok(&self) -> bool { self.risks.iter().all(|r| r.clean()) }

    /// 高危风险定位（severity High）。
    pub fn high_risks(&self) -> Vec<usize> {
        self.risks.iter().enumerate().filter(|(_, r)| r.severity() == Level::High).map(|(i, _)| i).collect()
    }

    /// 无障碍维主场位（域本色）：a11y 页存在。
    pub fn a11y_sheet_present(&self) -> bool {
        self.sheets.iter().any(|x| x.0 == NineDim::A11y)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves413_checks() -> CheckSet {
    let mut set = CheckSet::new("ves413");
    let mut ng = NineGrid::new();

    // C413-01 九页汇总（含 a11y 主场位）。
    for d in NineDim::ALL.iter() { ng.put_sheet(*d, true); }
    chk(&mut set, "C413-01", ng.sheets_ok() && ng.a11y_sheet_present(), "九页全一致");

    // C413-02 汇总与资产不符检出与修正。
    ng.put_sheet(NineDim::Perf, false);
    chk(&mut set, "C413-02", !ng.sheets_ok() && ng.fix_sheet(NineDim::Perf) && ng.sheets_ok(), "perf 页修正");

    // C413-03 风险登记（四列）。
    ng.file_risk(Risk { dim: NineDim::Perf, prob: Level::Mid, impact: Level::High, mitigation: Some("基准门加严"), has_user_identity: false });
    ng.file_risk(Risk { dim: NineDim::Security, prob: Level::Low, impact: Level::High, mitigation: None, has_user_identity: false });
    chk(&mut set, "C413-03", ng.risks.len() == 2, "两风险登记");

    // C413-04 缓解缺补案。
    chk(&mut set, "C413-04", ng.missing_mitigations() == vec![1]
        && ng.add_mitigation(1, "出站白名单审") && ng.missing_mitigations().is_empty(), "补案复绿");

    // C413-05 严重度核算：Mid+High → High。
    chk(&mut set, "C413-05", ng.risks[0].severity() == Level::High
        && ng.high_risks().contains(&0), "高危定位");

    // C413-06 严重度低档。
    let lo = Risk { dim: NineDim::Doc, prob: Level::Low, impact: Level::Low, mitigation: Some("留痕"), has_user_identity: false };
    chk(&mut set, "C413-06", lo.severity() == Level::Low, "Low+Low=Low");

    // C413-07 隐私红线：含用户身份风险册违规。
    ng.file_risk(Risk { dim: NineDim::Api, prob: Level::Low, impact: Level::Low, mitigation: Some("x"), has_user_identity: true });
    chk(&mut set, "C413-07", !ng.privacy_ok(), "身份信息违规");

    // C413-08 清理后合规。
    ng.risks.pop();
    chk(&mut set, "C413-08", ng.privacy_ok(), "清理合规");

    // C413-09 九维名单单源。
    chk(&mut set, "C413-09", NineDim::ALL.len() == 9 && NineDim::ALL[0].name() == "api"
        && NineDim::ALL[8].name() == "observe", "九维单源");

    // C413-10 域本色主场位：a11y 维名。
    chk(&mut set, "C413-10", NineDim::A11y.name() == "a11y", "a11y 维名");

    // C413-11 风险缺登记扫描补登（册空 + 九页齐 = 无风险态合法）。
    let mut ng2 = NineGrid::new();
    for d in NineDim::ALL.iter() { ng2.put_sheet(*d, true); }
    chk(&mut set, "C413-11", ng2.sheets_ok() && ng2.risks.is_empty() && ng2.privacy_ok(), "零风险态合法");

    // C413-12 空册红。
    let empty = NineGrid::new();
    chk(&mut set, "C413-12", !empty.sheets_ok() && !empty.a11y_sheet_present(), "空册红");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves413_smoke() {
        let set = run_ves413_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves413 red: {:?}", red);
    }
}
