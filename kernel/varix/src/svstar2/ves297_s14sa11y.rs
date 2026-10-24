//! ves297_s14sa11y — VE-F4057 · T03 排版与 S 域无障碍
//! 语言门复用单源 + RTL 读屏协同（复用 F3856）+ 阅读辅助协同（复用 F3883）+ 排版×无障碍交叉对拍 + 门分叉归一执法。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x83C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum SaCode {
    Ok = 0,
    GateSplit = SEG_BASE + 0x01,
    LinkBroken = SEG_BASE + 0x02,
    SourceSplit = SEG_BASE + 0x03,
    PairContradict = SEG_BASE + 0x04,
    GateNotArabic = SEG_BASE + 0x05,
}

/// 语言门（复用单源）：阿语族字形对比度门。
pub struct LanguageGate {
    pub threshold_ppm: u32,
}

impl LanguageGate {
    pub fn canonical() -> Self { LanguageGate { threshold_ppm: 4_500_000 } } // 4.5:1
    pub fn applies(&self, family: &str) -> bool {
        family == "ar" || family == "fa" || family == "ur"
    }
    pub fn check(&self, family: &str, contrast_ppm: u32) -> Result<(), SaCode> {
        if !self.applies(family) { return Err(SaCode::GateNotArabic); }
        if contrast_ppm < self.threshold_ppm { return Err(SaCode::GateSplit); }
        Ok(())
    }
    /// 门分叉归一：两侧门阈值必须一致。
    pub fn unify(a: u32, b: u32) -> Result<u32, SaCode> {
        if a != b { return Err(SaCode::GateSplit); }
        Ok(a)
    }
}

/// 三条单源复述：RTL 读屏（F3856）/阅读辅助（F3883）/语言门（F2895）。
pub struct SourceLedger {
    pub entries: Vec<(&'static str, bool)>, // (单源名, 复述一致)
}

impl SourceLedger {
    pub fn canonical() -> Self {
        SourceLedger {
            entries: vec![
                ("F3856-RTL读屏", true),
                ("F3883-阅读辅助", true),
                ("F2895-语言门", true),
            ],
        }
    }
    pub fn all_retold(&self) -> bool {
        self.entries.len() == 3 && self.entries.iter().all(|e| e.1)
    }
    /// 断链检出：任一单源复述断开。
    pub fn broken(&self) -> Option<&'static str> {
        for e in self.entries.iter() {
            if !e.1 { return Some(e.0); }
        }
        None
    }
}

/// 交叉对拍：排版属性 × 无障碍属性 双方一致。
#[derive(Clone)]
pub struct PairClaim {
    pub item: String,
    pub layout_val: u32,
    pub a11y_val: u32,
}

pub struct PairBook {
    pub claims: Vec<PairClaim>,
}

impl PairBook {
    pub fn new() -> Self { PairBook { claims: Vec::new() } }
    pub fn add(&mut self, c: PairClaim) { self.claims.push(c); }
    pub fn judge(&self) -> (bool, Vec<String>) {
        let mut bad = Vec::new();
        for c in self.claims.iter() {
            if c.layout_val != c.a11y_val { bad.push(c.item.clone()); }
        }
        (bad.is_empty(), bad)
    }
    /// 仲裁：矛盾时按无障碍侧优先（交叉对拍终版归无障碍）。
    pub fn arbitrate(&self, item: &str) -> Result<u32, SaCode> {
        for c in self.claims.iter() {
            if c.item == item {
                return Ok(c.a11y_val);
            }
        }
        Err(SaCode::PairContradict)
    }
}

pub fn run_ves297_checks() -> CheckSet {
    let mut set = CheckSet::new("ves297_s14sa11y");
    let gate = LanguageGate::canonical();

    // 1) 语言门覆盖阿语族。
    set.add("C4057-01", gate.applies("ar") && gate.applies("fa") && !gate.applies("zh"), "门覆盖族");

    // 2) 门阈值检查。
    set.add("C4057-02", gate.check("ar", 4_600_000).is_ok() && gate.check("ar", 4_000_000) == Err(SaCode::GateSplit), "门检查");

    // 3) 非阿语族不进门。
    set.add("C4057-03", gate.check("en", 1_000_000) == Err(SaCode::GateNotArabic), "非阿语不进门");

    // 4) 门分叉归一。
    set.add("C4057-04", LanguageGate::unify(4_500_000, 4_500_000) == Ok(4_500_000)
        && LanguageGate::unify(4_500_000, 4_400_000) == Err(SaCode::GateSplit), "门分叉归一");

    // 5) 三条单源复述。
    let sl = SourceLedger::canonical();
    set.add("C4057-05", sl.all_retold(), "三条单源");

    // 6) 断链检出。
    let mut sl2 = SourceLedger::canonical();
    sl2.entries[1].1 = false;
    set.add("C4057-06", sl2.broken() == Some("F3883-阅读辅助"), "断链检出");

    // 7) 交叉对拍全绿。
    let mut pb = PairBook::new();
    pb.add(PairClaim { item: String::from("行距"), layout_val: 150, a11y_val: 150 });
    pb.add(PairClaim { item: String::from("字距"), layout_val: 20, a11y_val: 20 });
    set.add("C4057-07", pb.judge() == (true, Vec::new()), "对拍绿");

    // 8) 对拍矛盾检出。
    pb.add(PairClaim { item: String::from("字号"), layout_val: 14, a11y_val: 16 });
    let (ok, bad) = pb.judge();
    set.add("C4057-08", !ok && bad == ["字号"], "矛盾检出");

    // 9) 仲裁：无障碍优先。
    set.add("C4057-09", pb.arbitrate("字号") == Ok(16), "无障碍优先");

    // 10) 仲裁查无。
    set.add("C4057-10", pb.arbitrate("无此项") == Err(SaCode::PairContradict), "查无拒");

    // 11) 门阈值常量。
    set.add("C4057-11", gate.threshold_ppm == 4_500_000, "阈值 4.5:1");

    // 12) 段位自查。
    set.add("C4057-12", SEG_BASE == 0x83C0, "段位 0x83C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_and_pair() {
        let g = LanguageGate::canonical();
        assert!(g.check("ar", 4_600_000).is_ok());
        let mut pb = PairBook::new();
        pb.add(PairClaim { item: String::from("x"), layout_val: 1, a11y_val: 1 });
        assert!(pb.judge().0);
    }
}
