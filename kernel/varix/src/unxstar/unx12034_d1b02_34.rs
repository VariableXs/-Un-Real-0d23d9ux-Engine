//! UNX-F12034 缓冲族：BUFFER_OVERFLOW/BUFFER_TOO_SMALL/PARTIAL_COPY（0x19660 段）
//!
//! 判据原文：缓冲三态判定——需量超容量按差值分档：差值 ≥ 单元尺寸判
//! BUFFER_OVERFLOW（0xC0000023），差值 < 单元尺寸判 BUFFER_TOO_SMALL
//! （0xC0000020）；可截断面按 PARTIAL_COPY 语义给出已拷字节数；恰等
//! 容量不判不足（边界收）。
//!
//! 结构：缓冲判定器（BufJudge：单元尺寸 × 容量）+ 判定核（四码守卫）+
//! 判定账（BfRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12034_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19660 独占 0x20）。
pub const SEG_BASE: u32 = 0x19660;
pub const BF_ZERO_UNIT: u32 = SEG_BASE + 0x01; // 零单元
pub const BF_BAD_REC: u32 = SEG_BASE + 0x02; // 伪造判定账
pub const BF_OVERFLOW: u32 = SEG_BASE + 0x03; // 溢出档
pub const BF_TOO_SMALL: u32 = SEG_BASE + 0x04; // 不足档
pub const BF_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍判定

/// 语义钉值。
pub const STATUS_BUFFER_OVERFLOW: u32 = 0xC000_0023;
pub const STATUS_BUFFER_TOO_SMALL: u32 = 0xC000_0020;

/// 判定三态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BufVerdict {
    Fit = 1,
    Partial = 2,
    Overflow = 3,
    TooSmall = 4,
}

/// 一笔判定的账目。
#[derive(Clone)]
pub struct BfRec {
    pub seq: u32,
    pub need: u32,
    pub cap: u32,
    pub verdict: BufVerdict,
    /// PARTIAL_COPY 已拷字节数。
    pub copied: u32,
    pub note: String,
}

impl BfRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
    /// 账目自洽：Partial 态必须带正拷贝数。
    pub fn verdict_ok(&self) -> bool {
        match self.verdict {
            BufVerdict::Partial => self.copied > 0,
            _ => true,
        }
    }
}

/// 缓冲判定器。
pub struct BufJudge {
    pub unit: u32,
    pub cap: u32,
    pub ledger: Vec<BfRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl BufJudge {
    pub fn new(unit: u32, cap: u32) -> BufJudge {
        BufJudge { unit, cap, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 判定核：need ≤ cap 放行；溢出差 ≥ unit 判 Overflow；差 < unit 判
    /// TooSmall（本档无 PARTIAL 面，Partial 由显式截断入口产生）。
    pub fn judge(&mut self, need: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(BF_FROZEN);
        }
        if note.is_empty() || self.unit == 0 {
            return Err(BF_ZERO_UNIT);
        }
        let verdict = if need <= self.cap {
            BufVerdict::Fit
        } else if need - self.cap >= self.unit {
            BufVerdict::Overflow
        } else {
            BufVerdict::TooSmall
        };
        let rec = BfRec {
            seq: self.seq_next,
            need,
            cap: self.cap,
            verdict,
            copied: 0,
            note: note.to_string(),
        };
        if !rec.ok() {
            return Err(BF_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        match verdict {
            BufVerdict::Fit => Ok(need),
            BufVerdict::Overflow => Err(BF_OVERFLOW),
            _ => Err(BF_TOO_SMALL),
        }
    }

    /// PARTIAL_COPY 显式入口：可截断面拷到满容为止。
    pub fn partial_copy(&mut self, need: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(BF_FROZEN);
        }
        if note.is_empty() || self.unit == 0 {
            return Err(BF_ZERO_UNIT);
        }
        let copied = if need <= self.cap { need } else { self.cap };
        let verdict = if copied == need { BufVerdict::Fit } else { BufVerdict::Partial };
        let rec = BfRec {
            seq: self.seq_next,
            need,
            cap: self.cap,
            verdict,
            copied,
            note: note.to_string(),
        };
        if !rec.ok() || !rec.verdict_ok() {
            return Err(BF_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        if copied == need {
            Ok(copied)
        } else {
            Err(BF_TOO_SMALL)
        }
    }

    /// 档清点。
    pub fn count_verdict(&self, v: BufVerdict) -> usize {
        self.ledger.iter().filter(|r| r.verdict == v).count()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok() && r.verdict_ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
            h = h.wrapping_mul(211).wrapping_add(r.verdict as u32);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut j = BufJudge::new(16, 0x100);
    // 1) 溢出档：差 16 ≥ 单元 → BF_OVERFLOW。
    got[0] = j.judge(0x110, "ovf").err() == Some(BF_OVERFLOW);
    // 2) 不足档：差 8 < 单元 → BF_TOO_SMALL。
    got[1] = j.judge(0x108, "small").err() == Some(BF_TOO_SMALL);
    // 3) 零单元 → BF_ZERO_UNIT。
    got[2] = BufJudge::new(0, 0x100).judge(8, "z").err() == Some(BF_ZERO_UNIT);
    // 4) 空留痕 → BF_ZERO_UNIT（无痕判定守卫）。
    got[3] = j.judge(0x40, "").err() == Some(BF_ZERO_UNIT);
    // 5) 印账后判定 → BF_FROZEN。
    let _ = j.judge(0x40, "fit");
    j.seal();
    got[4] = j.judge(0x40, "post").err() == Some(BF_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12034_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12034");

    // J1 恰等容量边界收。
    let j1 = {
        let mut j = BufJudge::new(16, 0x100);
        j.judge(0x100, "eq") == Ok(0x100)
    };
    s.add("J1-恰等边界", j1, "need==cap 放行");

    // J2 溢出档：差恰等单元。
    let j2 = {
        let mut j = BufJudge::new(16, 0x100);
        j.judge(0x110, "o").err() == Some(BF_OVERFLOW)
    };
    s.add("J2-溢出档", j2, "差 16 判溢出");

    // J3 不足档：差小于单元。
    let j3 = {
        let mut j = BufJudge::new(16, 0x100);
        j.judge(0x10F, "u").err() == Some(BF_TOO_SMALL)
    };
    s.add("J3-不足档", j3, "差 15 判不足");

    // J4 档差值分界：差 15/16 恰好分档。
    let j4 = {
        let mut a = BufJudge::new(16, 0x100);
        let small = a.judge(0x10F, "x").err() == Some(BF_TOO_SMALL);
        let mut b = BufJudge::new(16, 0x100);
        let ovf = b.judge(0x110, "y").err() == Some(BF_OVERFLOW);
        small && ovf
    };
    s.add("J4-档差分界", j4, "15/16 切界");

    // J5 PARTIAL_COPY：截到满容。
    let j5 = {
        let mut j = BufJudge::new(16, 0x100);
        match j.partial_copy(0x180, "p") {
            Ok(_) => false,
            Err(e) => {
                e == BF_TOO_SMALL
                    && j.ledger[0].copied == 0x100
                    && j.ledger[0].verdict == BufVerdict::Partial
            }
        }
    };
    s.add("J5-部分拷贝", j5, "拷 0x100 需 0x180");

    // J6 PARTIAL_COPY 全量放行。
    let j6 = {
        let mut j = BufJudge::new(16, 0x100);
        j.partial_copy(0x40, "f") == Ok(0x40)
    };
    s.add("J6-部分全量", j6, "need<cap 全拷");

    // J7 零单元拒。
    let j7 = { BufJudge::new(0, 0x100).judge(8, "z").err() == Some(BF_ZERO_UNIT) };
    s.add("J7-零单元拒", j7, "无单元无判定");

    // J8 语义钉值核对。
    let j8 = STATUS_BUFFER_OVERFLOW == 0xC000_0023 && STATUS_BUFFER_TOO_SMALL == 0xC000_0020;
    s.add("J8-钉值核对", j8, "0x23/0x20");

    // J9 档清点对平。
    let j9 = {
        let mut j = BufJudge::new(16, 0x100);
        let _ = j.judge(0x40, "fit");
        let _ = j.judge(0x200, "ovf");
        let _ = j.judge(0x104, "small");
        let _ = j.partial_copy(0x180, "p");
        j.count_verdict(BufVerdict::Fit) == 1
            && j.count_verdict(BufVerdict::Overflow) == 1
            && j.count_verdict(BufVerdict::TooSmall) == 1
            && j.count_verdict(BufVerdict::Partial) == 1
            && j.ledger_ok()
    };
    s.add("J9-四档对平", j9, "各一档");

    // J10 印账冻结。
    let j10 = {
        let mut j = BufJudge::new(16, 0x100);
        let f1 = j.fold();
        let f2 = j.fold();
        j.seal();
        f1 == f2
            && j.judge(0x40, "post").err() == Some(BF_FROZEN)
            && j.partial_copy(0x40, "post").err() == Some(BF_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：判定面。
    let mut j = BufJudge::new(16, 0x200);
    let _ = j.judge(0x100, "sum-fit");
    let _ = j.judge(0x300, "sum-ovf");
    s.add("样例两档", j.ledger.len() == 2, "1 放 1 溢");
    s.add("样例账面合法", j.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12034_judge_flow() {
        let mut j = BufJudge::new(16, 0x100);
        assert_eq!(j.judge(0x100, "a"), Ok(0x100));
        assert_eq!(j.judge(0x110, "b"), Err(BF_OVERFLOW));
        assert_eq!(j.judge(0x101, "c"), Err(BF_TOO_SMALL));
        assert_eq!(j.partial_copy(0x140, "d"), Err(BF_TOO_SMALL));
        assert_eq!(j.ledger[3].copied, 0x100);
    }

    #[test]
    fn unx12034_checks_green() {
        let s = run_unx12034_checks();
        assert!(s.all_passed(), "UNX-F12034 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12034_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
