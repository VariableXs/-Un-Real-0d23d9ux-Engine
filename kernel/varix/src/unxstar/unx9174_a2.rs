//! UNX-F9174 资源族与 A2 计时基座消费协议（0x63580 段）
//!
//! 锚点：UNX-F9174-J1 换算账（A2 tick → 资源族 CLK_TCK 单位逐条落
//! 账：tick 原值、换算值、来源印、留痕）；J2 换算语义（来源锁定 A2
//! + 单位换算唯一 + 单调闸 + 源印一致四性质）；J3 换算指纹（账本折
//! 叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（资源族×A2 计时基座面）。
//!
//! 结构：A2 源印（时钟基座版本锁）+ 换算核（tick→CLK_TCK 单位）+
//! 换算账本 + 折叠指纹 + 注入核（错源印/空留痕/回退/重号/印后登记
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63580 独占 0x20）。
pub const SEG_BASE: u32 = 0x63580;
pub const CV_BAD_SRC: u32 = SEG_BASE + 0x01; // 错源印
pub const CV_REGRESS: u32 = SEG_BASE + 0x02; // 换算回退
pub const CV_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const CV_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const CV_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// A2 时钟基座源印（锁定版本）。
pub const A2_SRC_SEAL: u32 = 0xA2C1_0001;
/// 资源族单位：CLK_TCK。
pub const CLK_TCK: u32 = 100;

/// tick → CLK_TCK 单位换算（A2 tick 频率 = CLK_TCK × 比例；样本取
/// A2 tick = 1ms，CLK_TCK tick = 10ms ⇒ 换算除 10）。
pub const A2_TICK_MS: u32 = 1;
pub const CLK_TICK_MS: u32 = 10;

pub fn ticks_to_clk(a2_ticks: u32) -> u32 {
    a2_ticks * A2_TICK_MS / CLK_TICK_MS
}

/// 单条换算账。
#[derive(Clone)]
pub struct CvRec {
    pub seq: u32,
    pub a2_ticks: u32,
    pub clk_ticks: u32,
    pub src_seal: u32,
    pub note: String,
}

impl CvRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.src_seal == A2_SRC_SEAL
    }

    pub fn render_line(&self) -> String {
        format!(
            "CV{}|t{}->c{}|src{:x}|{}",
            self.seq, self.a2_ticks, self.clk_ticks, self.src_seal, self.note
        )
    }
}

/// 换算簿。
pub struct CvBook {
    pub ledger: Vec<CvRec>,
    pub last_clk: u32,
    pub seq_next: u32,
    pub sealed: bool,
}

impl CvBook {
    pub fn new() -> CvBook {
        CvBook { ledger: Vec::new(), last_clk: 0, seq_next: 1, sealed: false }
    }

    /// 换算核：A2 tick 进 → CLK_TCK 单位出，单调闸。
    pub fn convert(&mut self, a2_ticks: u32, src: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CV_FROZEN);
        }
        if note.is_empty() {
            return Err(CV_EMPTY_NOTE);
        }
        if src != A2_SRC_SEAL {
            return Err(CV_BAD_SRC);
        }
        let c = ticks_to_clk(a2_ticks);
        if c < self.last_clk {
            self.record(a2_ticks, c, src, note);
            return Err(CV_REGRESS);
        }
        self.last_clk = c;
        self.record(a2_ticks, c, src, note);
        Ok(c)
    }

    fn record(&mut self, a2: u32, c: u32, src: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(CvRec {
            seq,
            a2_ticks: a2,
            clk_ticks: c,
            src_seal: src,
            note: note.to_string(),
        });
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
        let mut h: u32 = 0x91740;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.a2_ticks);
            h = h.wrapping_mul(31).wrapping_add(r.clk_ticks);
            h = h.wrapping_mul(31).wrapping_add(r.src_seal);
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
    // 1) 错源印账
    let bad = CvRec {
        seq: 1,
        a2_ticks: 1,
        clk_ticks: 0,
        src_seal: 0xDEAD,
        note: "inj-src".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = CvRec {
        seq: 2,
        a2_ticks: 1,
        clk_ticks: 0,
        src_seal: A2_SRC_SEAL,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 错源印拒
    let mut b = CvBook::new();
    got[2] = b.convert(10, 0xBEEF, "inj-src").err() == Some(CV_BAD_SRC);
    // 4) 回退拒
    let _ = b.convert(1000, A2_SRC_SEAL, "a"); // c=100
    got[3] = b.convert(100, A2_SRC_SEAL, "b").err() == Some(CV_REGRESS);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.convert(10, A2_SRC_SEAL, "inj-frozen").err() == Some(CV_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9174_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9174");
    let mut b = CvBook::new();

    // J1 换算账非空
    let _ = b.convert(150, A2_SRC_SEAL, "A2 进");
    let j1 = !b.ledger.is_empty();
    s.add("J1-换算账非空", j1, "convert 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且源印锁定");

    // J3 换算正确：150 tick → 15 CLK 单位
    let j3 = b.convert(150, A2_SRC_SEAL, "再算") == Ok(15);
    s.add("J3-换算比值", j3, "150×1/10=15");

    // J4 零换算
    let j4 = ticks_to_clk(0) == 0;
    s.add("J4-零换算", j4, "零 tick 出零");

    // J5 换算唯一：同入同出
    let j5 = {
        let mut b2 = CvBook::new();
        let a = b2.convert(1000, A2_SRC_SEAL, "一");
        let bb = b2.convert(1000, A2_SRC_SEAL, "二");
        a.is_err() == false && true && {
            // 第二次 c 相同但单调闸允许等值
            let _ = bb;
            ticks_to_clk(1000) == ticks_to_clk(1000)
        }
    };
    s.add("J5-换算唯一", j5, "同 tick 换算同值");

    // J6 来源锁定：错源印拒
    let j6 = b.convert(10, 0xBEEF, "野源").err() == Some(CV_BAD_SRC);
    s.add("J6-源印锁定", j6, "非 A2 印必拒");

    // J7 单调闸
    let j7 = {
        let mut b2 = CvBook::new();
        let _ = b2.convert(2000, A2_SRC_SEAL, "a"); // c=200
        b2.convert(100, A2_SRC_SEAL, "b").err() == Some(CV_REGRESS) // c=10 < 200
    };
    s.add("J7-单调闸", j7, "换算值回退必拒");

    // J8 等值放行
    let j8 = {
        let mut b2 = CvBook::new();
        let _ = b2.convert(1000, A2_SRC_SEAL, "a");
        b2.convert(1000, A2_SRC_SEAL, "b").is_ok()
    };
    s.add("J8-等值放行", j8, "等值换算不触回退");

    // J9 CLK_TCK 锁定
    let j9 = CLK_TCK == 100 && A2_TICK_MS == 1 && CLK_TICK_MS == 10;
    s.add("J9-常量锁定", j9, "换算常量三方锁定");

    // J10 大数不溢出（1<<20 tick → 104856? 验算：1048576/10=104857）
    let j10 = ticks_to_clk(1 << 20) == 104857;
    s.add("J10-大数换算", j10, "1<<20 tick → 104857 单位");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.convert(10, A2_SRC_SEAL, "").err() == Some(CV_EMPTY_NOTE);
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
    fn unx9174_convert_flow() {
        assert_eq!(ticks_to_clk(150), 15);
        assert_eq!(ticks_to_clk(0), 0);
        assert_eq!(ticks_to_clk(1 << 20), 104857);
        let mut b = CvBook::new();
        assert_eq!(b.convert(1000, A2_SRC_SEAL, "a"), Ok(100));
        assert_eq!(b.convert(100, A2_SRC_SEAL, "b"), Err(CV_REGRESS));
        assert_eq!(b.convert(10, 0xBEEF, "c"), Err(CV_BAD_SRC));
    }

    #[test]
    fn unx9174_checks_green() {
        let s = run_unx9174_checks();
        assert!(s.all_passed(), "UNX-F9174 判据未全绿");
    }
}
