//! UNX-F12024 等待族语义档：STATUS_WAIT_0/TIMEOUT/ALERTED/APC（0x19520 段）
//!
//! 判据原文：等待族四态闭集——STATUS_WAIT_0（基码+索引，索引 0..63）、
//! STATUS_TIMEOUT（0x102）、STATUS_ALERTED（0x101）、STATUS_USER_APC
//! （0x100）；WAIT 索引越 63 即拒；TIMEOUT 只从等待面产生（不作为操作
//! 结果登记）；等待结果逐笔对账（结果码与态一一对应）。
//!
//! 结构：等待结果闭集（WaitOutcome 枚举 + 编解码双向）+ 等待账
//! （WaitBook：结果登记 + 对账核）+ 登记核（四码守卫）+ 折叠指纹 +
//! 印账冻结 + 注入核 + 判据聚合（run_unx12024_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19520 独占 0x20）。
pub const SEG_BASE: u32 = 0x19520;
pub const WT_INDEX_RANGE: u32 = SEG_BASE + 0x01; // WAIT 索引越 63
pub const WT_UNKNOWN_CODE: u32 = SEG_BASE + 0x02; // 非等待族码混入
pub const WT_DUP: u32 = SEG_BASE + 0x03; // 同签重号登记
pub const WT_TIMEOUT_AS_RESULT: u32 = SEG_BASE + 0x04; // TIMEOUT 冒充操作结果
pub const WT_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍登记

/// 等待族语义钉值。
pub const STATUS_WAIT_0: u32 = 0x0000_0000;
pub const STATUS_ABANDONED_WAIT_0: u32 = 0x0000_0080;
pub const STATUS_USER_APC: u32 = 0x0000_00C0;
pub const STATUS_ALERTED: u32 = 0x0000_0101;
pub const STATUS_TIMEOUT: u32 = 0x0000_0102;

/// 等待结果闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitOutcome {
    Wait(u16),       // 索引 0..63
    Abandoned(u16),  // 索引 0..63
    UserApc,
    Alerted,
    Timeout,
}

/// 索引上限（WAIT 族索引域 6 位）。
pub const WAIT_INDEX_MAX: u16 = 63;

/// 编码核：结果 → 状态码。
pub fn encode_outcome(o: WaitOutcome) -> Result<u32, u32> {
    match o {
        WaitOutcome::Wait(i) => {
            if i > WAIT_INDEX_MAX {
                return Err(WT_INDEX_RANGE);
            }
            Ok(STATUS_WAIT_0 + i as u32)
        }
        WaitOutcome::Abandoned(i) => {
            if i > WAIT_INDEX_MAX {
                return Err(WT_INDEX_RANGE);
            }
            Ok(STATUS_ABANDONED_WAIT_0 + i as u32)
        }
        WaitOutcome::UserApc => Ok(STATUS_USER_APC),
        WaitOutcome::Alerted => Ok(STATUS_ALERTED),
        WaitOutcome::Timeout => Ok(STATUS_TIMEOUT),
    }
}

/// 解码核：状态码 → 结果（非等待族码判 None）。
pub fn decode_outcome(v: u32) -> Option<WaitOutcome> {
    if v == STATUS_TIMEOUT {
        return Some(WaitOutcome::Timeout);
    }
    if v == STATUS_ALERTED {
        return Some(WaitOutcome::Alerted);
    }
    if v == STATUS_USER_APC {
        return Some(WaitOutcome::UserApc);
    }
    if v >= STATUS_ABANDONED_WAIT_0 && v <= STATUS_ABANDONED_WAIT_0 + WAIT_INDEX_MAX as u32 {
        return Some(WaitOutcome::Abandoned((v - STATUS_ABANDONED_WAIT_0) as u16));
    }
    if v <= WAIT_INDEX_MAX as u32 {
        return Some(WaitOutcome::Wait(v as u16));
    }
    None
}

/// 等待族成员判定：可被 decode_outcome 归类即族内。
pub fn is_wait_family(v: u32) -> bool {
    decode_outcome(v).is_some()
}

/// 一笔等待登记。
#[derive(Clone)]
pub struct WtRec {
    pub seq: u32,
    pub code: u32,
    pub note: String,
}

impl WtRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 等待账模型。
pub struct WaitBook {
    pub recs: Vec<WtRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl WaitBook {
    pub fn new() -> WaitBook {
        WaitBook { recs: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记核：解码合法 + TIMEOUT 不作结果登记 + 非空留痕。
    pub fn record(&mut self, code: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(WT_FROZEN);
        }
        if note.is_empty() {
            return Err(WT_UNKNOWN_CODE);
        }
        match decode_outcome(code) {
            None => return Err(WT_UNKNOWN_CODE),
            Some(WaitOutcome::Timeout) => return Err(WT_TIMEOUT_AS_RESULT),
            Some(_) => {}
        }
        let r = WtRec { seq: self.seq_next, code, note: note.to_string() };
        if !r.ok() {
            return Err(WT_UNKNOWN_CODE);
        }
        self.seq_next += 1;
        self.recs.push(r);
        Ok(())
    }

    /// 结果清点。
    pub fn count_outcome(&self, o: WaitOutcome) -> usize {
        match encode_outcome(o) {
            Ok(c) => self.recs.iter().filter(|r| r.code == c).count(),
            Err(_) => 0,
        }
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.recs.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
            h = h.wrapping_mul(211).wrapping_add(r.code);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = WaitBook::new();
    // 1) WAIT 索引越 63 → WT_INDEX_RANGE。
    got[0] = encode_outcome(WaitOutcome::Wait(64)).err() == Some(WT_INDEX_RANGE);
    // 2) 非等待族码混入（0xC0000005）→ WT_UNKNOWN_CODE。
    got[1] = b.record(0xC000_0005, "err").err() == Some(WT_UNKNOWN_CODE);
    // 3) TIMEOUT 冒充结果 → WT_TIMEOUT_AS_RESULT。
    got[2] = b.record(STATUS_TIMEOUT, "to").err() == Some(WT_TIMEOUT_AS_RESULT);
    // 4) 空留痕 → WT_UNKNOWN_CODE。
    got[3] = b.record(STATUS_WAIT_0, "").err() == Some(WT_UNKNOWN_CODE);
    // 5) 印账后登记 → WT_FROZEN。
    let _ = b.record(STATUS_WAIT_0, "w");
    b.seal();
    got[4] = b.record(STATUS_ALERTED, "post").err() == Some(WT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12024_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12024");

    // J1 编解码往返：WAIT_0=0。
    let j1 = {
        let v = encode_outcome(WaitOutcome::Wait(0)).unwrap_or(0xFFFF_FFFF);
        v == STATUS_WAIT_0
            && matches!(decode_outcome(v), Some(WaitOutcome::Wait(0)))
    };
    s.add("J1-WAIT0 往返", j1, "0 编解码一致");

    // J2 WAIT 索引边界：63 收 64 拒。
    let j2 = encode_outcome(WaitOutcome::Wait(63)).is_ok()
        && encode_outcome(WaitOutcome::Wait(64)).err() == Some(WT_INDEX_RANGE);
    s.add("J2-索引边界", j2, "63/64 切界");

    // J3 ABANDONED 族偏移：基码+索引。
    let j3 = {
        let v = encode_outcome(WaitOutcome::Abandoned(2)).unwrap_or(0xFFFF_FFFF);
        v == STATUS_ABANDONED_WAIT_0 + 2
            && matches!(decode_outcome(v), Some(WaitOutcome::Abandoned(2)))
    };
    s.add("J3-ABANDONED 偏移", j3, "0x80+2 对账");

    // J4 三单值码编解码。
    let j4 = decode_outcome(STATUS_USER_APC) == Some(WaitOutcome::UserApc)
        && decode_outcome(STATUS_ALERTED) == Some(WaitOutcome::Alerted)
        && decode_outcome(STATUS_TIMEOUT) == Some(WaitOutcome::Timeout);
    s.add("J4-三单值码", j4, "APC/ALERTED/TIMEOUT 全对");

    // J5 非等待族码判 None。
    let j5 = decode_outcome(0xC000_0005).is_none()
        && decode_outcome(0x8000_0001).is_none()
        && decode_outcome(0x4000_0000).is_none();
    s.add("J5-族外判空", j5, "Error/Warning/Info 全拒");

    // J6 族内空隙判空：0x40..0x7F 非法段（WAIT 上沿 0x3F 与 ABANDONED 下沿 0x80 之间）。
    let j6 = decode_outcome(0x0000_0041).is_none() && decode_outcome(0x0000_007F).is_none();
    s.add("J6-族内空隙", j6, "0x41 段不入族");

    // J7 等待账登记与清点。
    let j7 = {
        let mut b = WaitBook::new();
        let _ = b.record(STATUS_WAIT_0, "w0");
        let _ = b.record(STATUS_ALERTED, "al");
        let _ = b.record(STATUS_USER_APC, "apc");
        b.count_outcome(WaitOutcome::Wait(0)) == 1
            && b.count_outcome(WaitOutcome::Alerted) == 1
            && b.count_outcome(WaitOutcome::UserApc) == 1
            && b.ledger_ok()
    };
    s.add("J7-三态登记", j7, "三结果逐一对账");

    // J8 TIMEOUT 不作结果登记。
    let j8 = {
        let mut b = WaitBook::new();
        b.record(STATUS_TIMEOUT, "to").err() == Some(WT_TIMEOUT_AS_RESULT)
    };
    s.add("J8-TIMEOUT 拒登", j8, "TIMEOUT 属等待面非结果面");

    // J9 族外码拒。
    let j9 = {
        let mut b = WaitBook::new();
        b.record(0xC000_0005, "e").err() == Some(WT_UNKNOWN_CODE)
    };
    s.add("J9-族外拒", j9, "Error 码不入等待账");

    // J10 印账冻结。
    let j10 = {
        let mut b = WaitBook::new();
        let _ = b.record(STATUS_WAIT_0, "a");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.record(STATUS_ALERTED, "post").err() == Some(WT_FROZEN)
    };
    s.add("J10-指纹封账", j10, "指纹幂等且印后必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：多签等待面。
    let mut b = WaitBook::new();
    let _ = b.record(STATUS_WAIT_0 + 3, "sig3");
    let _ = b.record(STATUS_ABANDONED_WAIT_0 + 1, "abn1");
    s.add("样例双签清点", b.recs.len() == 2, "WAIT_3+ABN_1");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");

    // J12 边界复核：最高合法多签与最低废弃码。
    let j12 = {
        let hi_wait = STATUS_WAIT_0 + 0x3F; // 63 位全签
        let lo_abn = STATUS_ABANDONED_WAIT_0; // 废弃族下界
        is_wait_family(hi_wait) && is_wait_family(lo_abn)
    };
    s.add("J12-边界复核", j12, "63 签与废弃下界皆在族内");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12024_codec_flow() {
        assert_eq!(encode_outcome(WaitOutcome::Wait(5)), Ok(5));
        assert_eq!(encode_outcome(WaitOutcome::Wait(64)), Err(WT_INDEX_RANGE));
        assert_eq!(decode_outcome(STATUS_TIMEOUT), Some(WaitOutcome::Timeout));
        assert_eq!(decode_outcome(0xC000_0005), None);
    }

    #[test]
    fn unx12024_checks_green() {
        let s = run_unx12024_checks();
        assert!(s.all_passed(), "UNX-F12024 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12024_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
