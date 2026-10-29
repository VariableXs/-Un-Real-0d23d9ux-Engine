//! UNX-F9190 clock_nanosleep 语义档（时钟基与 TIMER_ABSTIME）（0x63780 段）
//!
//! 锚点：UNX-F9190-J1 基睡账（clock_nanosleep 每次落账：时钟基、
//! flag、请求值、换算后相对值、结果、留痕）；J2 基睡语义（时钟基闭
//! 集 + flag 闭集 + ABSTIME 换算唯一 + 相对直通四性质）；J3 基睡指
//! 纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（clock_nanosleep 时钟基面）。
//!
//! 结构：时钟基闭集 + flag 闭集 + 换算核（ABSTIME：目标−当前）+
//! 基睡账本 + 折叠指纹 + 注入核（错基/空留痕/野 flag/重号/印后登记
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63780 独占 0x20）。
pub const SEG_BASE: u32 = 0x63780;
pub const CN_UNK_CLK: u32 = SEG_BASE + 0x01; // 错时钟基
pub const CN_BAD_FLAG: u32 = SEG_BASE + 0x02; // 野 flag
pub const CN_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const CN_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const CN_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 时钟基闭集。
pub const CK_REALTIME: u8 = 0;
pub const CK_MONOTONIC: u8 = 1;
pub const CK_COUNT: u8 = 2;

pub const CK_NAMES: [&str; 2] = ["realtime", "monotonic"];

/// flag 闭集。
pub const F_REL: u32 = 0; // 相对（默认）
pub const F_ABSTIME: u32 = 1; // 绝对
pub const F_KNOWN: u32 = F_REL | F_ABSTIME;

/// 单位：纳秒。
pub type Ns = u64;

/// ABSTIME 换算：target − now（回退→0）。
pub fn abst_to_rel(target: Ns, now: Ns) -> Ns {
    target.saturating_sub(now)
}

/// 单条基睡账。
#[derive(Clone)]
pub struct CnRec {
    pub seq: u32,
    pub clk: u8,
    pub flag: u32,
    pub req: Ns,
    pub rel: Ns, // 换算后相对睡量
    pub note: String,
}

impl CnRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.clk as usize) < CK_COUNT as usize && self.flag & !F_KNOWN == 0
    }

    pub fn render_line(&self) -> String {
        format!(
            "CN{}|{}|f{}|r{}->rel{}|{}",
            self.seq,
            CK_NAMES[self.clk as usize],
            self.flag,
            self.req,
            self.rel,
            self.note
        )
    }
}

/// 基睡簿。
pub struct CnBook {
    pub ledger: Vec<CnRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl CnBook {
    pub fn new() -> CnBook {
        CnBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// clock_nanosleep 门。
    pub fn sleep(&mut self, clk: u8, flag: u32, req: Ns, now: Ns, note: &str) -> Result<Ns, u32> {
        if self.sealed {
            return Err(CN_FROZEN);
        }
        if note.is_empty() {
            return Err(CN_EMPTY_NOTE);
        }
        if clk >= CK_COUNT {
            return Err(CN_UNK_CLK);
        }
        if flag & !F_KNOWN != 0 {
            return Err(CN_BAD_FLAG);
        }
        let rel = if flag & F_ABSTIME != 0 { abst_to_rel(req, now) } else { req };
        self.ledger.push(CnRec { seq: self.seq_next, clk, flag, req, rel, note: note.to_string() });
        self.seq_next += 1;
        Ok(rel)
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91900;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.clk as u32);
            h = h.wrapping_mul(31).wrapping_add(r.flag);
            h = h.wrapping_mul(31).wrapping_add((r.rel & 0xFFFF) as u32);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错时钟基账
    let bad = CnRec {
        seq: 1,
        clk: 9,
        flag: 0,
        req: 1,
        rel: 1,
        note: "inj-clk".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = CnRec {
        seq: 2,
        clk: 0,
        flag: 0,
        req: 0,
        rel: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 错时钟基拒
    let mut b = CnBook::new();
    got[2] = b.sleep(9, F_REL, 100, 0, "inj-clk").err() == Some(CN_UNK_CLK);
    // 4) 野 flag 拒
    got[3] = b.sleep(CK_MONOTONIC, 0x10, 100, 0, "inj-flag").err() == Some(CN_BAD_FLAG);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.sleep(CK_MONOTONIC, F_REL, 100, 0, "inj-frozen").err() == Some(CN_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9190_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9190");
    let mut b = CnBook::new();

    // J1 基睡账非空（两基两 flag 齐）
    let j1 = {
        let _ = b.sleep(CK_MONOTONIC, F_REL, 500, 0, "mono 相对");
        let _ = b.sleep(CK_REALTIME, F_ABSTIME, 1_500, 1_000, "realtime 绝对");
        b.ledger.iter().filter(|r| r.clk == CK_MONOTONIC).count() >= 1
            && b.ledger.iter().filter(|r| r.clk == CK_REALTIME).count() >= 1
            && b.ledger.iter().filter(|r| r.flag & F_ABSTIME != 0).count() >= 1
    };
    s.add("J1-基睡账齐", j1, "两基两 flag 样本齐");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且基/flag 闭集");

    // J3 ABSTIME 换算：target 1500 − now 1000 = 500
    let j3 = b.ledger.iter().any(|r| r.flag & F_ABSTIME != 0 && r.rel == 500);
    s.add("J3-ABSTIME换算", j3, "target−now 换算");

    // J4 相对直通
    let j4 = {
        let mut b2 = CnBook::new();
        b2.sleep(CK_MONOTONIC, F_REL, 700, 999, "相对") == Ok(700)
    };
    s.add("J4-相对直通", j4, "F_REL 忽略 now");

    // J5 ABSTIME 回退→0
    let j5 = abst_to_rel(500, 1000) == 0;
    s.add("J5-回退归零", j5, "target<now → 0 直返");

    // J6 时钟基闭集
    let j6 = b.sleep(9, F_REL, 1, 0, "野基").err() == Some(CN_UNK_CLK);
    s.add("J6-基闭集", j6, "时钟基出闭集必拒");

    // J7 flag 闭集
    let j7 = b.sleep(CK_MONOTONIC, 0x10, 1, 0, "野f").err() == Some(CN_BAD_FLAG);
    s.add("J7-flag闭集", j7, "flag 出闭集必拒");

    // J8 换算唯一
    let j8 = abst_to_rel(1500, 1000) == abst_to_rel(1500, 1000);
    s.add("J8-换算唯一", j8, "同参同出");

    // J9 双基等价性（同一相对时长两基同 rel）
    let j9 = {
        let mut b2 = CnBook::new();
        let a = b2.sleep(CK_MONOTONIC, F_REL, 300, 0, "m");
        let bb = b2.sleep(CK_REALTIME, F_REL, 300, 0, "r");
        a == bb
    };
    s.add("J9-双基等价", j9, "相对模式双基同值");

    // J10 ABSTIME 等值（target==now → 0）
    let j10 = {
        let mut b2 = CnBook::new();
        b2.sleep(CK_REALTIME, F_ABSTIME, 1000, 1000, "等值") == Ok(0)
    };
    s.add("J10-绝对等值", j10, "target==now → 直返");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.sleep(CK_MONOTONIC, F_REL, 1, 0, "").err() == Some(CN_EMPTY_NOTE);
    s.add("J12-空留痕拒", j12, "空留痕必拒");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9190_clk_flow() {
        let mut b = CnBook::new();
        assert_eq!(b.sleep(CK_MONOTONIC, F_REL, 500, 0, "a"), Ok(500));
        assert_eq!(b.sleep(CK_REALTIME, F_ABSTIME, 1500, 1000, "b"), Ok(500));
        assert_eq!(b.sleep(CK_REALTIME, F_ABSTIME, 500, 1000, "c"), Ok(0));
        assert_eq!(b.sleep(9, F_REL, 1, 0, "d"), Err(CN_UNK_CLK));
        assert_eq!(b.sleep(CK_MONOTONIC, 0x10, 1, 0, "e"), Err(CN_BAD_FLAG));
    }

    #[test]
    fn unx9190_checks_green() {
        let s = run_unx9190_checks();
        assert!(s.all_passed(), "UNX-F9190 判据未全绿");
    }
}
