//! VE-F4343 · 一致性审查器汇总（U08 测试与资产组）
//! 职责：静态审查器汇总——审查器五族（词典符合度 / 令牌合规 / 契约完备 /
//! 隐私标注 / 无障碍标注——无障碍为一等族）+ 审查器框架（统一注册 + 按需
//! 启停 + 自定义规则插槽）+ 误报治理（抑制标记 + 误报率月度量）+ 崩溃隔离
//! 不断扫描流 + 抑制滥用复审抽查 + 规则冲突版本仲裁。
//! 段位：0x3CC0（续表 idx577）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3CC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RvCode {
    /// 审查器崩溃（隔离不断流）。
    Crashed = SEG_BASE,
    /// 抑制滥用（复审抽查）。
    SuppressAbuse = SEG_BASE + 0x01,
    /// 规则冲突（版本仲裁）。
    RuleClash = SEG_BASE + 0x02,
    /// 族号越界。
    FamilyRange = SEG_BASE + 0x03,
}

/// 五族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    DictMatch,
    TokenComply,
    ContractFull,
    PrivacyMark,
    /// 无障碍标注——一等族。
    A11yMark,
}

/// 一等族断言（域色）：无障碍族恒在一等位。
pub fn is_first_class(f: Family) -> bool {
    matches!(f, Family::A11yMark)
}

/// 审查器实例。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Linter {
    pub id: u32,
    pub family: Family,
    pub enabled: bool,
    /// 自定义插槽规则（None = 内置规则）。
    pub custom: Option<u32>,
    /// 崩溃隔离标记。
    pub quarantined: bool,
    /// 抑制计数。
    pub suppressed: u32,
}

/// 规则版本冲突记录。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clash {
    pub rule_a: u32,
    pub rule_b: u32,
    /// 仲裁版本（取大）。
    pub resolved: u32,
}

/// 审查器台。
#[derive(Default)]
pub struct LinterDesk {
    pub linters: Vec<Linter>,
    pub clashes: Vec<Clash>,
    pub month_suppress: u32,
    pub scanned_files: u32,
}

/// 抑制滥用线（月度：单器抑制 > 50 = 抽查）。
pub const SUPPRESS_AUDIT: u32 = 50;

impl LinterDesk {
    pub fn new() -> Self {
        LinterDesk { ..Default::default() }
    }

    /// 审查器注册（五族校验在类型层；内置/自定义插槽双形态）。
    pub fn register(&mut self, id: u32, family: Family, custom: Option<u32>) {
        if !self.linters.iter().any(|l| l.id == id) {
            self.linters.push(Linter { id, family, enabled: true, custom, quarantined: false, suppressed: 0 });
        }
    }

    /// 按需启停。
    pub fn toggle(&mut self, id: u32, on: bool) -> Result<(), RvCode> {
        let l = self.linters.iter_mut().find(|l| l.id == id).ok_or(RvCode::Crashed)?;
        l.enabled = on;
        Ok(())
    }

    /// 扫描流：启用且未隔离的器才参与；有器崩溃 → 隔离但流不断（返回参与数）。
    pub fn scan(&mut self, crash_id: Option<u32>) -> usize {
        if let Some(cid) = crash_id {
            if let Some(l) = self.linters.iter_mut().find(|l| l.id == cid) {
                l.quarantined = true;
                l.enabled = false;
            }
        }
        self.scanned_files += 1;
        self.linters.iter().filter(|l| l.enabled && !l.quarantined).count()
    }

    /// 崩溃器恢复（重验后复位）。
    pub fn revive(&mut self, id: u32) -> Result<(), RvCode> {
        let l = self.linters.iter_mut().find(|l| l.id == id).ok_or(RvCode::Crashed)?;
        if l.quarantined {
            l.quarantined = false;
            l.enabled = true;
        }
        Ok(())
    }

    /// 抑制标记登记（误报治理输入）。
    pub fn suppress(&mut self, id: u32) -> Result<(), RvCode> {
        let l = self.linters.iter_mut().find(|l| l.id == id).ok_or(RvCode::Crashed)?;
        l.suppressed += 1;
        self.month_suppress += 1;
        if l.suppressed > SUPPRESS_AUDIT {
            return Err(RvCode::SuppressAbuse);
        }
        Ok(())
    }

    /// 月度量：抑制滥用器名单（抽查对象）。
    pub fn abuse_list(&self) -> Vec<u32> {
        self.linters.iter().filter(|l| l.suppressed > SUPPRESS_AUDIT).map(|l| l.id).collect()
    }

    /// 规则冲突版本仲裁：同槽规则双注册冲突 → 取版本大者（留痕）。
    pub fn clash_arbitrate(&mut self, rule_a: u32, rule_b: u32) -> u32 {
        let resolved = rule_a.max(rule_b);
        self.clashes.push(Clash { rule_a, rule_b, resolved });
        resolved
    }

    /// 一等族在册断言（域色）：无障碍族至少一器启用。
    pub fn a11y_first_class(&self) -> bool {
        self.linters.iter().any(|l| l.family == Family::A11yMark && l.enabled && !l.quarantined)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves577_checks() -> CheckSet {
    let mut set = CheckSet::new("ves577");
    let mut ld = LinterDesk::new();

    // C577-01 五族注册（含自定义插槽）。
    ld.register(1, Family::DictMatch, None);
    ld.register(2, Family::TokenComply, None);
    ld.register(3, Family::ContractFull, None);
    ld.register(4, Family::PrivacyMark, Some(0x51));
    ld.register(5, Family::A11yMark, None);
    chk(&mut set, "C577-01", ld.linters.len() == 5 && ld.linters[3].custom == Some(0x51), "五族注册含插槽");

    // C577-02 一等族断言（无障碍恒一等——域色）。
    chk(&mut set, "C577-02", is_first_class(Family::A11yMark) && !is_first_class(Family::DictMatch)
        && ld.a11y_first_class(), "一等族在册启用");

    // C577-03 按需启停。
    chk(&mut set, "C577-03", ld.toggle(3, false).is_ok() && ld.scan(None) == 4, "启停后扫描 4 器");
    let _ = ld.toggle(3, true);

    // C577-04 崩溃隔离不断流。
    chk(&mut set, "C577-04", ld.scan(Some(2)) == 4 && ld.linters[1].quarantined, "崩溃隔离流不断");

    // C577-05 隔离器恢复。
    chk(&mut set, "C577-05", ld.revive(2).is_ok() && ld.scan(None) == 5, "恢复重验");

    // C577-06 抑制标记登记。
    chk(&mut set, "C577-06", ld.suppress(1).is_ok() && ld.linters[0].suppressed == 1, "抑制登记");

    // C577-07 抑制滥用检出（>50 抽查）。
    let mut ld2 = LinterDesk::new();
    ld2.register(9, Family::TokenComply, None);
    let mut last = Ok(());
    for _ in 0..51 {
        last = ld2.suppress(9);
    }
    chk(&mut set, "C577-07", last == Err(RvCode::SuppressAbuse) && ld2.abuse_list() == [9], "抑制滥用抽查");

    // C577-08 无滥用器名单为空。
    chk(&mut set, "C577-08", ld.abuse_list().is_empty(), "无滥用名单空");

    // C577-09 规则冲突版本仲裁取大。
    chk(&mut set, "C577-09", ld.clash_arbitrate(12, 30) == 30 && ld.clashes.len() == 1, "冲突取大留痕");

    // C577-10 无器扫描零参与（不臆测）。
    let mut ld3 = LinterDesk::new();
    chk(&mut set, "C577-10", ld3.scan(None) == 0, "空台零参与");

    // C577-11 全员隔离扫描不断流（零参与但流计数）。
    let mut ld4 = LinterDesk::new();
    ld4.register(1, Family::DictMatch, None);
    let _ = ld4.scan(Some(1));
    chk(&mut set, "C577-11", ld4.scan(None) == 0 && ld4.scanned_files == 2, "全隔离流计数");

    // C577-12 注册幂等。
    ld.register(1, Family::DictMatch, None);
    chk(&mut set, "C577-12", ld.linters.len() == 5, "注册幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_isolate_revive() {
        let mut ld = LinterDesk::new();
        for i in 1..=3u32 {
            ld.register(i, Family::DictMatch, None);
        }
        assert_eq!(ld.scan(None), 3);
        assert_eq!(ld.scan(Some(2)), 2);
        assert!(ld.linters[1].quarantined);
        assert!(ld.revive(2).is_ok());
        assert_eq!(ld.scan(None), 3);
    }

    #[test]
    fn suppress_abuse() {
        let mut ld = LinterDesk::new();
        ld.register(1, Family::A11yMark, None);
        for _ in 0..50 {
            assert!(ld.suppress(1).is_ok());
        }
        assert_eq!(ld.suppress(1), Err(RvCode::SuppressAbuse));
        assert_eq!(ld.abuse_list(), [1]);
    }
}
