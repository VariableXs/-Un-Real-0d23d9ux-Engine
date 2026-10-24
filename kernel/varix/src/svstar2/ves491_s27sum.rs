//! VE-F4257 · 验证与全域审查汇总（U03 一致性验证平台）
//! 职责：U03 对全 U 域验证结果汇总（U01 架构+U02 契约+U03 平台三段一致率合成）
//! + 对家族审查体系的输出位确认 + 终版口径（无障碍一致率）+ 衔接点挂指针。
//! 段位：0x1E20（续表 idx491）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1E20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmCode {
    /// 合成口径漂移。
    CaliberDrift = SEG_BASE,
    /// 输出位缺。
    OutMissing = SEG_BASE + 0x01,
    /// 衔接断。
    LinkBroken = SEG_BASE + 0x02,
    /// 越界（家族审查体系外的输出）。
    OutOfScope = SEG_BASE + 0x03,
}

/// 三段。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seg {
    U01Arch = 0,
    U02Contract = 1,
    U03Platform = 2,
}
pub const SEG_N: usize = 3;

/// 家族审查体系输出位（五站：S/T/U/审查/总账）。
pub const OUTPUT_SLOTS: [&str; 5] = ["S", "T", "U", "Review", "Ledger"];

/// 汇总册。
pub struct SummaryBook {
    /// 各段一致率（bp）与样本数。
    pub rates: [(u32, u32); SEG_N],
    /// 口径指纹（单源对版）。
    pub caliber_fp: u32,
    /// 输出位登记表。
    pub outputs: Vec<&'static str>,
    /// 衔接指针：(段, 上游 F 号)。
    pub links: Vec<(u8, u32)>,
}

impl SummaryBook {
    pub fn new(caliber_fp: u32) -> Self {
        SummaryBook { rates: [(0, 0); SEG_N], caliber_fp, outputs: Vec::new(), links: Vec::new() }
    }

    /// 段数据录入。
    pub fn feed(&mut self, s: Seg, rate_bp: u32, samples: u32) {
        self.rates[s as usize] = (rate_bp, samples);
    }

    /// 三段合成一致率（加权=样本数加权；零样本段不计权）。
    pub fn synthesize(&self) -> u32 {
        let mut num: u64 = 0;
        let mut den: u64 = 0;
        for (r, n) in self.rates.iter() {
            if *n > 0 {
                num += (*r as u64) * (*n as u64);
                den += *n as u64;
            }
        }
        if den == 0 {
            return 0;
        }
        (num / den) as u32
    }

    /// 终版口径确认（无障碍一致率单列终版）。
    pub fn final_caliber(&self, fp: u32) -> Result<u32, SmCode> {
        if fp != self.caliber_fp {
            return Err(SmCode::CaliberDrift);
        }
        Ok(self.synthesize())
    }

    /// 输出位挂载（不越界：只允许五站）。
    pub fn mount_output(&mut self, slot: &'static str) -> Result<(), SmCode> {
        if !OUTPUT_SLOTS.contains(&slot) {
            return Err(SmCode::OutOfScope);
        }
        if !self.outputs.contains(&slot) {
            self.outputs.push(slot);
        }
        Ok(())
    }

    /// 衔接指针挂载。
    pub fn link(&mut self, s: Seg, f: u32) {
        if !self.links.iter().any(|(a, b)| *a == s as u8 && *b == f) {
            self.links.push((s as u8, f));
        }
    }

    /// 衔接完备：三段各有指针。
    pub fn links_ok(&self) -> bool {
        (0..SEG_N).all(|s| self.links.iter().any(|(a, _)| *a as usize == s))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C491-01..12。
pub fn run_ves491_checks() -> CheckSet {
    let mut set = CheckSet::new("ves491");

    let mut sb = SummaryBook::new(0xCA11);
    chk(&mut set, "C491-01", sb.synthesize() == 0, "零样本合成=0（保守）");

    sb.feed(Seg::U01Arch, 9800, 40);
    sb.feed(Seg::U02Contract, 9500, 10);
    sb.feed(Seg::U03Platform, 9900, 50);
    // (9800*40 + 9500*10 + 9900*50)/100 = (392000+95000+495000)/100 = 9820
    chk(&mut set, "C491-02", sb.synthesize() == 9820, "样本数加权合成 9820bp");

    chk(&mut set, "C491-03", sb.final_caliber(0xCA11) == Ok(9820), "终版口径一致→合成值");
    chk(&mut set, "C491-04", sb.final_caliber(0xBEEF) == Err(SmCode::CaliberDrift), "口径漂移拒绝出终版");

    // 输出位
    chk(&mut set, "C491-05", sb.mount_output("Ledger").is_ok(), "总账输出位挂载");
    chk(&mut set, "C491-06", sb.mount_output("Rogue") == Err(SmCode::OutOfScope), "五站外输出越界退回");
    let _ = sb.mount_output("Ledger");
    chk(&mut set, "C491-07", sb.outputs == alloc::vec!["Ledger"], "输出位去重");

    // 衔接指针
    sb.link(Seg::U01Arch, 4207);
    sb.link(Seg::U02Contract, 4227);
    sb.link(Seg::U03Platform, 4241);
    sb.link(Seg::U03Platform, 4241);
    chk(&mut set, "C491-08", sb.links_ok(), "三段衔接指针齐");
    chk(&mut set, "C491-09", sb.links.len() == 3, "指针去重");

    // 部分录入合成
    let mut sb2 = SummaryBook::new(1);
    sb2.feed(Seg::U01Arch, 9000, 1);
    chk(&mut set, "C491-10", sb2.synthesize() == 9000, "单段合成=段值");
    let mut sb3 = SummaryBook::new(1);
    sb3.feed(Seg::U01Arch, 9000, 0);
    chk(&mut set, "C491-11", sb3.synthesize() == 0, "零样本段不计权");

    chk(&mut set, "C491-12", sb.links.iter().all(|(a, _)| (*a as usize) < SEG_N), "指针段位不越界");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_synthesize() {
        let mut sb = SummaryBook::new(1);
        sb.feed(Seg::U01Arch, 10000, 1);
        sb.feed(Seg::U02Contract, 5000, 1);
        assert_eq!(sb.synthesize(), 7500);
        assert!(sb.mount_output("S").is_ok());
        assert!(sb.mount_output("X").is_err());
    }
}
