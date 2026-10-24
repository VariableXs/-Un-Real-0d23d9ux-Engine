#![allow(dead_code)]
//! VE-F3947 · 无障碍用户测试
//!
//! 全域用户测试制度全域化。参与制：每域至少两画像真实参与（参与度全域
//! 断言）。测试方法：任务制走查 + 参与式设计。补偿红线：参与者劳动必须
//! 有偿致谢（补偿断言）。反馈闭环：测试发现 → 改进 → 知会参与者（闭环
//! 全域化）。
//! 错误路径：参与形式化→P0（参与度审计）；无补偿→P1；发现悬空→闭环；
//! 画像缺→补。
//! 性能：参与 O(全域断言)；任务 O(制)；补偿 O(断言)；闭环 O(复述)。
//! 跨批对接：F3156/F3755 模式全域复用声明；五画像对端；社区对端。
//! 无隐私面（自身即隐私条）。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0D40）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0D40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8GCode(pub u16);

impl S8GCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const FORMAL_PARTICIPATION: Self = Self(SEG_BASE + 1);
    pub const NO_COMPENSATION: Self = Self(SEG_BASE + 2);
    pub const FINDING_HANGING: Self = Self(SEG_BASE + 3);
    pub const PORTRAIT_GAP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::FORMAL_PARTICIPATION => "参与形式化→P0",
            Self::NO_COMPENSATION => "无补偿→P1",
            Self::FINDING_HANGING => "发现悬空→闭环",
            Self::PORTRAIT_GAP => "画像缺→补",
            _ => "未知",
        }
    }
}

// ── 画像与参与制 ─────────────────────────────────────────────────────

/// 用户画像。
#[derive(Debug, Clone)]
pub struct Portrait {
    pub id: u8,
    /// 画像名（视障/听障/运动/认知/老年等）。
    pub name: &'static str,
}

/// 参与记录：画像 × 域 × 是否真实参与（有任务产出才算真实）。
#[derive(Debug, Clone)]
pub struct Participation {
    pub portrait: u8,
    pub domain: u8,
    /// 真实参与 = 完成了至少一个任务制走查任务。
    pub real: bool,
    /// 完成任务数。
    pub tasks_done: u16,
}

/// 参与度全域断言器：每域至少两画像真实参与。
#[derive(Debug, Clone)]
pub struct ParticipationAudit {
    pub recs: Vec<Participation>,
    /// 全域域数。
    pub domains: u8,
}

pub const MIN_PORTRAITS_PER_DOMAIN: usize = 2;

impl ParticipationAudit {
    pub fn new(domains: u8) -> Self {
        Self { recs: Vec::new(), domains }
    }

    pub fn record(&mut self, p: Participation) -> S8GCode {
        if p.domain == 0 || p.domain > self.domains {
            return S8GCode::PORTRAIT_GAP;
        }
        self.recs.push(p);
        S8GCode::OK
    }

    /// 域 d 的真实参与画像数（去重）。
    pub fn real_portraits_of(&self, domain: u8) -> Vec<u8> {
        let mut seen: Vec<u8> = Vec::new();
        for r in self.recs.iter() {
            if r.domain == domain && r.real && !seen.contains(&r.portrait) {
                seen.push(r.portrait);
            }
        }
        seen
    }

    /// 参与形式化检出：真实参与不足两画像的域。
    pub fn formal_gaps(&self) -> Vec<u8> {
        (1..=self.domains)
            .filter(|d| self.real_portraits_of(*d).len() < MIN_PORTRAITS_PER_DOMAIN)
            .collect()
    }

    pub fn all_real(&self) -> bool {
        self.formal_gaps().is_empty()
    }

    /// 画像缺→补：返回尚未参与的画像 id（相对某域）。
    pub fn missing_portraits(&self, domain: u8, all_portraits: &[u8]) -> Vec<u8> {
        let have = self.real_portraits_of(domain);
        all_portraits.iter().filter(|p| !have.contains(p)).map(|p| *p).collect()
    }
}

impl Default for ParticipationAudit {
    fn default() -> Self {
        Self::new(8)
    }
}

// ── 任务制走查 + 参与式设计 ──────────────────────────────────────────

/// 走查任务：真实任务制（形式化检出锚）。
#[derive(Debug, Clone)]
pub struct WalkTask {
    pub name: &'static str,
    pub domain: u8,
    /// 参与式设计会话标记（不只是执行，还有共创环节）。
    pub co_design: bool,
}

// ── 补偿与致谢（补偿断言）────────────────────────────────────────────

/// 补偿登记：参与者劳动必须有偿致谢。
#[derive(Debug, Clone)]
pub struct Compensation {
    pub portrait: u8,
    /// 有偿金额（0 = 无偿 = 违反补偿断言）。
    pub paid: u32,
    /// 致谢是否发出。
    pub thanked: bool,
}

impl Compensation {
    /// 补偿断言：有偿 + 致谢缺一不可。
    pub fn satisfied(&self) -> bool {
        self.paid > 0 && self.thanked
    }

    pub fn violation(&self) -> Option<S8GCode> {
        if self.paid == 0 {
            Some(S8GCode::NO_COMPENSATION)
        } else if !self.thanked {
            Some(S8GCode::NO_COMPENSATION)
        } else {
            None
        }
    }
}

/// 补偿台账：全部参与者补偿齐备。
#[derive(Debug, Clone)]
pub struct CompBook {
    pub items: Vec<Compensation>,
}

impl CompBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, c: Compensation) {
        self.items.push(c);
    }

    /// 无补偿→P1 名单。
    pub fn violators(&self) -> Vec<u8> {
        self.items.iter().filter(|c| c.violation().is_some()).map(|c| c.portrait).collect()
    }

    pub fn all_satisfied(&self) -> bool {
        self.violators().is_empty() && !self.items.is_empty()
    }
}

impl Default for CompBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 反馈闭环（发现 → 改进 → 知会）────────────────────────────────────

/// 测试发现三态：登记 → 改进 → 知会。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingStage {
    /// 已登记未改进。
    Logged,
    /// 已改进未知会。
    Improved,
    /// 已知会参与者（闭环）。
    Notified,
}

/// 发现条目。
#[derive(Debug, Clone)]
pub struct Finding {
    pub name: &'static str,
    pub stage: FindingStage,
}

/// 闭环账。
#[derive(Debug, Clone)]
pub struct FindingLoop {
    pub items: Vec<Finding>,
}

impl FindingLoop {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn log(&mut self, name: &'static str) {
        self.items.push(Finding { name, stage: FindingStage::Logged });
    }

    /// 推进阶段。
    pub fn advance(&mut self, name: &str) -> S8GCode {
        match self.items.iter_mut().find(|f| f.name == name) {
            Some(f) => {
                f.stage = match f.stage {
                    FindingStage::Logged => FindingStage::Improved,
                    FindingStage::Improved => FindingStage::Notified,
                    FindingStage::Notified => FindingStage::Notified,
                };
                S8GCode::OK
            }
            None => S8GCode::FINDING_HANGING,
        }
    }

    /// 发现悬空检出：未到闭环态的发现。
    pub fn hanging(&self) -> Vec<&'static str> {
        self.items.iter().filter(|f| f.stage != FindingStage::Notified).map(|f| f.name).collect()
    }

    pub fn closed(&self) -> bool {
        !self.items.is_empty() && self.hanging().is_empty()
    }
}

impl Default for FindingLoop {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves201_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves201_s08usertest");

    // 01 参与度全域断言：八域各两画像真实参与
    {
        let mut a = ParticipationAudit::new(8);
        for d in 1..=8u8 {
            a.record(Participation { portrait: 1, domain: d, real: true, tasks_done: 2 });
            a.record(Participation { portrait: 2, domain: d, real: true, tasks_done: 1 });
        }
        chk(&mut set, "C3947-01", a.all_real(), "err");
    }
    // 02 参与形式化→P0 实测（形式参与不算）
    {
        let mut a = ParticipationAudit::new(2);
        a.record(Participation { portrait: 1, domain: 1, real: true, tasks_done: 1 });
        a.record(Participation { portrait: 2, domain: 1, real: false, tasks_done: 0 });
        chk(&mut set, "C3947-02", a.formal_gaps() == vec![1, 2] && !a.all_real(), "err");
    }
    // 03 同画像重复参与去重
    {
        let mut a = ParticipationAudit::new(1);
        a.record(Participation { portrait: 3, domain: 1, real: true, tasks_done: 5 });
        a.record(Participation { portrait: 3, domain: 1, real: true, tasks_done: 9 });
        chk(&mut set, "C3947-03", a.real_portraits_of(1).len() == 1, "err");
    }
    // 04 画像缺→补实测
    {
        let mut a = ParticipationAudit::new(1);
        a.record(Participation { portrait: 1, domain: 1, real: true, tasks_done: 1 });
        let missing = a.missing_portraits(1, &[1, 2, 3, 4, 5]);
        chk(&mut set, "C3947-04", missing == vec![2, 3, 4, 5], "err");
    }
    // 05 域号越界拒收
    {
        let mut a = ParticipationAudit::new(3);
        chk(&mut set, "C3947-05", a.record(Participation { portrait: 1, domain: 4, real: true, tasks_done: 1 }) == S8GCode::PORTRAIT_GAP, "err");
    }
    // 06 补偿断言：有偿+致谢齐备
    {
        let mut b = CompBook::new();
        b.add(Compensation { portrait: 1, paid: 200, thanked: true });
        b.add(Compensation { portrait: 2, paid: 150, thanked: true });
        chk(&mut set, "C3947-06", b.all_satisfied() && b.violators().is_empty(), "err");
    }
    // 07 无补偿→P1 名单实测
    {
        let mut b = CompBook::new();
        b.add(Compensation { portrait: 1, paid: 0, thanked: false });
        b.add(Compensation { portrait: 2, paid: 100, thanked: false });
        chk(&mut set, "C3947-07", b.violators() == vec![1, 2] && !b.all_satisfied(), "err");
    }
    // 08 反馈闭环三段推进
    {
        let mut f = FindingLoop::new();
        f.log("对比度不足");
        f.advance("对比度不足");
        f.advance("对比度不足");
        chk(&mut set, "C3947-08", f.hanging().is_empty() && f.closed(), "err");
    }
    // 09 发现悬空检出
    {
        let mut f = FindingLoop::new();
        f.log("焦点丢失");
        f.advance("焦点丢失");
        chk(&mut set, "C3947-09", f.hanging() == vec!["焦点丢失"] && !f.closed(), "err");
    }
    // 10 未知发现推进拒收
    {
        let mut f = FindingLoop::new();
        chk(&mut set, "C3947-10", f.advance("幽灵") == S8GCode::FINDING_HANGING, "err");
    }
    // 11 闭环态不回退
    {
        let mut f = FindingLoop::new();
        f.log("a");
        f.advance("a");
        f.advance("a");
        f.advance("a");
        chk(&mut set, "C3947-11", f.items[0].stage == FindingStage::Notified, "err");
    }
    // 12 任务制走查参与式设计标记
    {
        let t = WalkTask { name: "结账流程", domain: 2, co_design: true };
        chk(&mut set, "C3947-12", t.co_design && t.domain == 2, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3947-13", S8GCode::PORTRAIT_GAP.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8GCode::OK, S8GCode::FORMAL_PARTICIPATION, S8GCode::NO_COMPENSATION, S8GCode::FINDING_HANGING, S8GCode::PORTRAIT_GAP];
        chk(&mut set, "C3947-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_usertest_all_green() {
        let s = run_ves201_checks();
        assert!(s.all_passed(), "ves201 red: {}", s.red_items_text());
    }

    #[test]
    fn test_participation_p0_full_domain() {
        let mut a = ParticipationAudit::new(1);
        for p in 1..=5u8 {
            a.record(Participation { portrait: p, domain: 1, real: true, tasks_done: 3 });
        }
        assert!(a.all_real());
        assert!(a.missing_portraits(1, &[1, 2, 3, 4, 5]).is_empty());
    }

    #[test]
    fn test_compensation_p1_redline() {
        let c = Compensation { portrait: 9, paid: 0, thanked: true };
        assert_eq!(c.violation(), Some(S8GCode::NO_COMPENSATION));
        assert!(!c.satisfied());
    }

    #[test]
    fn test_finding_loop_three_stage() {
        let mut f = FindingLoop::new();
        f.log("x");
        assert_eq!(f.advance("x"), S8GCode::OK);
        assert_eq!(f.advance("x"), S8GCode::OK);
        assert!(f.closed());
    }
}
