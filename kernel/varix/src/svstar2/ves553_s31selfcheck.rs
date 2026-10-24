//! VE-F4319 · U06 预备自查（U06 场景应用组）
//! 职责：U06 收口前自查——四维自查（覆盖/正确/一致/无障碍同前组口径）
//! + 本组特查：七景触达与判定权单源不越权——逐景复核无改判行为 + 回修
//! 重扫（不达标回修后复扫）+ 特查异议升级复核 + 自查清单与扫描报告。
//! 段位：0x32E0（续表 idx553）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x32E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsCode {
    /// 自查不达标（回修重扫）。
    NotReady = SEG_BASE,
    /// 越权改判（判定权单源破坏——特查红线）。
    Overreach = SEG_BASE + 0x01,
    /// 特查异议（升级复核）。
    Dispute = SEG_BASE + 0x02,
    /// 景号越界。
    SceneRange = SEG_BASE + 0x03,
}

/// 四维自查项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    Coverage,
    Correctness,
    Consistency,
    A11y,
}

/// 逐景特查记录：触达方式 + 是否发生改判。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneAudit {
    pub scene: u8,
    /// 触达方式（提示/引导/闸门——均不得改判）。
    pub touch: &'static str,
    /// 本景发生改判行为 = 越权红线。
    pub rejudged: bool,
    pub cleared: bool,
}

/// 自查台。
#[derive(Default)]
pub struct U06SelfCheck {
    /// 四维得分（bp）。
    pub dims: [(u32, u32); 4], // (得分, 满分)
    pub audits: Vec<SceneAudit>,
    pub disputes: Vec<u8>,
    pub rescans: u32,
    pub escalated: u32,
}

/// 四维通过线（bp）。
pub const DIM_PASS_BP: u32 = 9000;

impl U06SelfCheck {
    pub fn new() -> Self {
        U06SelfCheck {
            dims: [(0, 10000); 4],
            ..Default::default()
        }
    }

    /// 四维记分。
    pub fn score(&mut self, dim: Dim, got: u32, total: u32) -> Result<(), UsCode> {
        let i = match dim {
            Dim::Coverage => 0,
            Dim::Correctness => 1,
            Dim::Consistency => 2,
            Dim::A11y => 3,
        };
        if total == 0 {
            return Err(UsCode::NotReady);
        }
        self.dims[i] = (got.min(total), total);
        Ok(())
    }

    /// 四维达标断言（全维 >= 9000bp）。
    pub fn dims_ready(&self) -> bool {
        self.dims.iter().all(|(g, t)| *t > 0 && (*g as u64 * 10000 / *t as u64) >= DIM_PASS_BP as u64)
    }

    /// 逐景特查登记：改判行为即时红线（Overreach）。
    pub fn audit(&mut self, scene: u8, touch: &'static str, rejudged: bool) -> Result<(), UsCode> {
        if scene == 0 || scene > 7 {
            return Err(UsCode::SceneRange);
        }
        if rejudged {
            return Err(UsCode::Overreach);
        }
        if let Some(a) = self.audits.iter_mut().find(|a| a.scene == scene) {
            a.touch = touch;
            a.rejudged = false;
            a.cleared = true;
        } else {
            self.audits.push(SceneAudit { scene, touch, rejudged: false, cleared: true });
        }
        Ok(())
    }

    /// 特查异议登记（升级复核——计数留痕）。
    pub fn dispute(&mut self, scene: u8) {
        if !self.disputes.contains(&scene) {
            self.disputes.push(scene);
            self.escalated += 1;
        }
    }

    /// 复核裁决：维持原判（清异议）或翻案（景重审）。
    pub fn adjudicate(&mut self, scene: u8, uphold: bool) {
        if uphold {
            self.disputes.retain(|s| *s != scene);
        } else if let Some(a) = self.audits.iter_mut().find(|a| a.scene == scene) {
            a.cleared = false; // 翻案：重审
        }
    }

    /// 回修重扫：不达标项回修后复扫（计数留痕）。
    pub fn rescan(&mut self) {
        self.rescans += 1;
    }

    /// 预备报告就绪：四维达标 + 七景特查全清 + 无未结异议。
    pub fn ready(&self) -> bool {
        self.dims_ready()
            && (1..=7u8).all(|s| self.audits.iter().any(|a| a.scene == s && a.cleared))
            && self.disputes.is_empty()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves553_checks() -> CheckSet {
    let mut set = CheckSet::new("ves553");
    let mut sc = U06SelfCheck::new();

    // C553-01 四维记分（全维达标线）。
    let _ = sc.score(Dim::Coverage, 9200, 10000);
    let _ = sc.score(Dim::Correctness, 9500, 10000);
    let _ = sc.score(Dim::Consistency, 9000, 10000);
    let _ = sc.score(Dim::A11y, 9800, 10000);
    chk(&mut set, "C553-01", sc.dims_ready(), "四维全达标");

    // C553-02 维度不足检出。
    let _ = sc.score(Dim::Consistency, 8900, 10000);
    chk(&mut set, "C553-02", !sc.dims_ready(), "不足维检出");

    // C553-03 零满分防护（total=0 拒绝）。
    let mut sc2 = U06SelfCheck::new();
    chk(&mut set, "C553-03", sc2.score(Dim::Coverage, 1, 0) == Err(UsCode::NotReady), "零满分拒");

    // C553-04 逐景特查：无改判全清。
    let mut ok = true;
    for s in 1..=7u8 {
        if sc.audit(s, "提示", false).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C553-04", ok && sc.audits.len() == 7, "七景特查全清");

    // C553-05 越权改判红线（判定权单源破坏）。
    chk(&mut set, "C553-05", sc.audit(3, "闸门", true) == Err(UsCode::Overreach), "改判红线");

    // C553-06 特查异议升级复核（计数）。
    sc.dispute(5);
    sc.dispute(5); // 幂等
    chk(&mut set, "C553-06", sc.disputes == [5] && sc.escalated == 1, "异议升级留痕");

    // C553-07 复核维持原判清异议。
    sc.adjudicate(5, true);
    chk(&mut set, "C553-07", sc.disputes.is_empty(), "维持原判清异");

    // C553-08 复核翻案重审（清 cleared）。
    sc.dispute(2);
    sc.adjudicate(2, false);
    chk(&mut set, "C553-08", !sc.audits.iter().find(|a| a.scene == 2).map(|a| a.cleared).unwrap_or(true), "翻案重审");

    // C553-09 就绪总闸：翻案未清不放行。
    chk(&mut set, "C553-09", !sc.ready(), "翻案未清不放行");

    // C553-10 回修重扫留痕后恢复。
    let _ = sc.audit(2, "提示", false);
    sc.rescan();
    chk(&mut set, "C553-10", sc.rescans == 1 && sc.ready(), "回修复扫就绪");

    // C553-11 景号越界拒。
    chk(&mut set, "C553-11", sc.audit(0, "x", false).is_err() && sc.audit(8, "x", false).is_err(), "景号越界拒");

    // C553-12 记分钳制（超满分钳到满分——不虚报）。
    let _ = sc.score(Dim::A11y, 20000, 10000);
    chk(&mut set, "C553-12", sc.dims[3].0 == 10000 && sc.ready(), "记分钳制不虚报");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dims_gate() {
        let mut sc = U06SelfCheck::new();
        let _ = sc.score(Dim::Coverage, 10000, 10000);
        assert!(!sc.dims_ready()); // 其余三维零
        for d in [Dim::Correctness, Dim::Consistency, Dim::A11y] {
            let _ = sc.score(d, 10000, 10000);
        }
        assert!(sc.dims_ready());
    }

    #[test]
    fn overreach_redline() {
        let mut sc = U06SelfCheck::new();
        assert_eq!(sc.audit(1, "提示", false), Ok(()));
        assert_eq!(sc.audit(1, "闸门", true), Err(UsCode::Overreach));
        assert_eq!(sc.audit(8, "提示", false), Err(UsCode::SceneRange));
    }

    #[test]
    fn dispute_cycle() {
        let mut sc = U06SelfCheck::new();
        let _ = sc.audit(3, "提示", false);
        sc.dispute(3);
        sc.adjudicate(3, false);
        assert!(!sc.audits[0].cleared);
        let _ = sc.audit(3, "提示", false);
        assert!(sc.audits[0].cleared);
    }
}
