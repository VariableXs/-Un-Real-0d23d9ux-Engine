//! VE-F4302 · 一致性×新功能开发（U06 场景应用组）
//! 职责：新功能四点嵌入（设计期令牌预检/开发期 IDE 实时查/联调期跨域对拍
//! /收口期闸门）+ 清单随需求模板下发 + 首犯辅导制（首犯转辅导不记过，
//! 重复首犯转正式记录）+ 嵌入点缺失探针补位 + 清单无障碍条目强制。
//! 段位：0x2FE0（续表 idx536）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2FE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NfCode {
    /// 嵌入点缺失。
    EmbedMissing = SEG_BASE,
    /// 清单漏项。
    ChecklistHole = SEG_BASE + 0x01,
    /// 辅导滥用（重复首犯转正式）。
    TutorAbuse = SEG_BASE + 0x02,
}

/// 四点嵌入。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Embed {
    Design = 0,
    Coding = 1,
    Integration = 2,
    Close = 3,
}
pub const EMBED_N: usize = 4;

/// 首犯账。
#[derive(Clone, Copy, Debug)]
pub struct FirstOff {
    pub who: u8,
    pub count: u8,
    pub tutoring: bool,
    pub formal: bool,
}

/// 新功能场景器。
pub struct NewFeature {
    pub embeds: [bool; EMBED_N],
    /// 清单模板：(条目, 强制)。
    pub checklist: Vec<(u16, bool)>,
    pub firsts: Vec<FirstOff>,
    pub probes: Vec<u8>,
}

impl NewFeature {
    pub fn new() -> Self {
        NewFeature { embeds: [false; EMBED_N], checklist: Vec::new(), firsts: Vec::new(), probes: Vec::new() }
    }

    /// 嵌入点登记。
    pub fn embed(&mut self, e: Embed) {
        self.embeds[e as usize] = true;
    }

    /// 探针补位：缺哪点补哪点。
    pub fn probe_fill(&mut self) -> usize {
        let mut filled = 0;
        for i in 0..EMBED_N {
            if !self.embeds[i] {
                self.embeds[i] = true;
                self.probes.push(i as u8);
                filled += 1;
            }
        }
        filled
    }

    /// 清单下发（无障碍条目强制——域本色）。
    pub fn issue_checklist(&mut self, items: Vec<(u16, bool)>) -> Result<(), NfCode> {
        // 强制条目须含 a11y 前缀（编号 < 100 视为无障碍条）
        let has_a11y = items.iter().any(|(id, forced)| *forced && *id < 100);
        if !has_a11y {
            return Err(NfCode::ChecklistHole);
        }
        self.checklist = items;
        Ok(())
    }

    /// 违例处置：首犯辅导不记过；重复首犯转正式。
    pub fn violate(&mut self, who: u8) -> Result<(), NfCode> {
        if let Some(f) = self.firsts.iter_mut().find(|f| f.who == who) {
            f.count += 1;
            if f.count >= 2 {
                f.tutoring = false;
                f.formal = true; // 重复首犯转正式记录
                return Err(NfCode::TutorAbuse);
            }
            return Ok(());
        }
        self.firsts.push(FirstOff { who, count: 1, tutoring: true, formal: false });
        Ok(())
    }

    pub fn checklist_forced(&self) -> usize {
        self.checklist.iter().filter(|(_, f)| *f).count()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C536-01..12。
pub fn run_ves536_checks() -> CheckSet {
    let mut set = CheckSet::new("ves536");

    let mut nf = NewFeature::new();
    nf.embed(Embed::Design);
    nf.embed(Embed::Coding);
    chk(&mut set, "C536-01", nf.probe_fill() == 2, "探针补位两点（联调/收口）");
    chk(&mut set, "C536-02", nf.embeds.iter().all(|e| *e) && nf.probes == alloc::vec![2, 3], "四点嵌入齐+探针账");
    chk(&mut set, "C536-03", nf.probe_fill() == 0, "补齐后探针零补");

    // 清单
    chk(&mut set, "C536-04", nf.issue_checklist(alloc::vec![(200, true), (201, false)]) == Err(NfCode::ChecklistHole), "清单缺无障碍强制条拒绝（域色）");
    let _ = nf.issue_checklist(alloc::vec![(50, true), (200, true), (201, false)]);
    chk(&mut set, "C536-05", nf.checklist.len() == 3 && nf.checklist_forced() == 2, "清单含无障碍条下发");
    chk(&mut set, "C536-06", nf.checklist[0].0 == 50 && nf.checklist[0].1, "无障碍条目强制在册");

    // 首犯辅导
    chk(&mut set, "C536-07", nf.violate(1).is_ok(), "首犯转辅导不记过");
    chk(&mut set, "C536-08", nf.firsts[0].tutoring && !nf.firsts[0].formal, "辅导标记在册");
    chk(&mut set, "C536-09", nf.violate(1) == Err(NfCode::TutorAbuse), "重复首犯转正式记录");
    chk(&mut set, "C536-10", nf.firsts[0].formal && !nf.firsts[0].tutoring, "正式记录替换辅导");
    chk(&mut set, "C536-11", nf.violate(2).is_ok() && nf.firsts.len() == 2, "新人独立首犯账");

    chk(&mut set, "C536-12", EMBED_N == 4 && nf.firsts.iter().filter(|f| f.formal).count() == 1, "四点与首犯账自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_feature() {
        let mut nf = NewFeature::new();
        assert_eq!(nf.probe_fill(), 4);
        assert!(nf.issue_checklist(alloc::vec![(1, true)]).is_ok());
        assert!(nf.violate(1).is_ok());
        assert!(nf.violate(1).is_err());
    }
}
