//! UNX-F12206 展开次序对照账（SEG 0x640c0..0x640e0 段窗）
//!
//! 卡面：展开次序对照账（基准机录制比对）——基准序列录制、实测
//! 序列回收、逐位比对裁决；长度差/内容差/空账拒止全链过闸。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 基准录制：基准序列先录先存，重录覆盖。
//! - J2 实测回收：实测序列回收入账。
//! - J3 无基准拒止：未录基准即比对必拒 CM_NOBASE。
//! - J4 空实测拒止：无实测即比对必拒 CM_EMPTY。
//! - J5 长度差裁决：两序长度不等必拒 CM_LEN。
//! - J6 逐位比对：同长逐位对判，差位计数并留痕。
//! - J7 全等裁决：差位数 0 即全等，verdicts 全真。
//! - J8 次序敏感：元素相同次序不同必报差。
//! - J9 印账冻结：seal 后录制/回收/比对必拒 CM_FROZEN。
//! - J10 指纹幂等：同态同指纹。
//! - J11 摘要口径：比对结果摘要可回读。
//! - J12 注入五况全抓。
//!
//! 结构：OrderLedger 对照账（基准序列/实测序列/逐位裁决账/操作账）
//! + 比对核（compare 逐位裁决）+ 折叠指纹（211 进制卷积）+ 注入核
//! （无基准/空实测/长度差/次序差/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x640c0 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x640c0;
pub const CM_NOBASE: u32 = SEG_BASE + 0x01; // 无基准即比对
pub const CM_EMPTY: u32 = SEG_BASE + 0x02; // 无实测即比对
pub const CM_LEN: u32 = SEG_BASE + 0x03; // 长度差
pub const CM_FROZEN: u32 = SEG_BASE + 0x04; // 印账后操作
pub const CM_DIFF: u32 = SEG_BASE + 0x05; // 内容差（注入样本用）

/// 对照账。
pub struct OrderLedger {
    pub baseline: Vec<u32>,
    pub actual: Vec<u32>,
    pub verdicts: Vec<(usize, bool)>, // (位下标, 是否相等)
    pub ops: Vec<String>,
    pub sealed: bool,
}

impl OrderLedger {
    pub fn new() -> OrderLedger {
        OrderLedger {
            baseline: Vec::new(),
            actual: Vec::new(),
            verdicts: Vec::new(),
            ops: Vec::new(),
            sealed: false,
        }
    }

    /// 基准录制：先录先存，重录覆盖。
    pub fn record_baseline(&mut self, seq: Vec<u32>) -> Result<(), u32> {
        if self.sealed {
            return Err(CM_FROZEN);
        }
        self.baseline = seq;
        self.ops.push(format!("base{}", self.baseline.len()));
        Ok(())
    }

    /// 实测回收。
    pub fn record_actual(&mut self, seq: Vec<u32>) -> Result<(), u32> {
        if self.sealed {
            return Err(CM_FROZEN);
        }
        self.actual = seq;
        self.ops.push(format!("act{}", self.actual.len()));
        Ok(())
    }

    /// 比对核：逐位裁决，返回差位数；结构差（长度/空账）即拒。
    pub fn compare(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(CM_FROZEN);
        }
        if self.baseline.is_empty() {
            return Err(CM_NOBASE);
        }
        if self.actual.is_empty() {
            return Err(CM_EMPTY);
        }
        if self.baseline.len() != self.actual.len() {
            return Err(CM_LEN);
        }
        self.verdicts.clear();
        let mut diffs = 0usize;
        for i in 0..self.baseline.len() {
            let eq = self.baseline[i] == self.actual[i];
            if !eq {
                diffs += 1;
            }
            self.verdicts.push((i, eq));
        }
        self.ops.push(format!("cmp{}d{}", self.verdicts.len(), diffs));
        Ok(diffs)
    }

    /// 全等裁决。
    pub fn all_equal(&self) -> bool {
        !self.verdicts.is_empty() && self.verdicts.iter().all(|v| v.1)
    }

    /// 差位清单。
    pub fn diff_positions(&self) -> Vec<usize> {
        self.verdicts.iter().filter(|v| !v.1).map(|v| v.0).collect()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for x in &self.baseline {
            h = h.wrapping_mul(211).wrapping_add(*x);
        }
        for x in &self.actual {
            h = h.wrapping_mul(211).wrapping_add(x.wrapping_add(0x1000));
        }
        for v in &self.verdicts {
            h = h.wrapping_mul(211).wrapping_add(v.0 as u32);
            h = h.wrapping_mul(211).wrapping_add(v.1 as u32);
        }
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("b={} a={} v={}", self.baseline.len(), self.actual.len(), self.verdicts.len())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 无基准即比对必拒。
    got[0] = OrderLedger::new().compare().err() == Some(CM_NOBASE);
    // 2) 空实测即比对必拒。
    let mut l = OrderLedger::new();
    let _ = l.record_baseline(vec![1, 2]);
    got[1] = l.compare().err() == Some(CM_EMPTY);
    // 3) 长度差必拒。
    let mut l = OrderLedger::new();
    let _ = l.record_baseline(vec![1, 2, 3]);
    let _ = l.record_actual(vec![1, 2]);
    got[2] = l.compare().err() == Some(CM_LEN);
    // 4) 次序差必报差位。
    let mut l = OrderLedger::new();
    let _ = l.record_baseline(vec![1, 2, 3]);
    let _ = l.record_actual(vec![1, 3, 2]);
    got[3] = l.compare() == Ok(2) && l.diff_positions() == vec![1, 2];
    // 5) 印账后操作必抓。
    let mut f = OrderLedger::new();
    f.seal();
    got[4] = f.record_baseline(vec![1]).err() == Some(CM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12206_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12206");

    // J1 基准录制。
    let j1 = {
        let mut l = OrderLedger::new();
        l.record_baseline(vec![3, 2, 1]).is_ok() && l.baseline == vec![3, 2, 1]
    };
    s.add("J1-基准录制", j1, "先录先存");

    // J2 实测回收。
    let j2 = {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![1, 2]);
        l.record_actual(vec![1, 2]).is_ok() && l.actual == vec![1, 2]
    };
    s.add("J2-实测回收", j2, "回收入账");

    // J3 无基准拒止。
    let j3 = {
        let mut l = OrderLedger::new();
        let _ = l.record_actual(vec![1]);
        l.compare().err() == Some(CM_NOBASE)
    };
    s.add("J3-无基准拒止", j3, "先基准后实测");

    // J4 空实测拒止。
    let j4 = {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![1]);
        l.compare().err() == Some(CM_EMPTY)
    };
    s.add("J4-空实测拒止", j4, "无实测必拒");

    // J5 长度差裁决。
    let j5 = {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![1, 2]);
        let _ = l.record_actual(vec![1, 2, 3]);
        l.compare().err() == Some(CM_LEN)
    };
    s.add("J5-长度差裁决", j5, "长度不等必拒");

    // J6 逐位比对。
    let j6 = {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![5, 6, 7, 8]);
        let _ = l.record_actual(vec![5, 0, 7, 8]);
        l.compare() == Ok(1) && l.diff_positions() == vec![1] && l.verdicts.len() == 4
    };
    s.add("J6-逐位比对", j6, "差位计数并留痕");

    // J7 全等裁决。
    let j7 = {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![1, 2, 3]);
        let _ = l.record_actual(vec![1, 2, 3]);
        let d = l.compare().unwrap_or(1);
        d == 0 && l.all_equal()
    };
    s.add("J7-全等裁决", j7, "差位 0 即全等");

    // J8 次序敏感。
    let j8 = {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![9, 8]);
        let _ = l.record_actual(vec![8, 9]);
        l.compare() == Ok(2)
    };
    s.add("J8-次序敏感", j8, "同元素异序必报差");

    // J9 印账冻结。
    let j9 = {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![1]);
        l.seal();
        l.record_actual(vec![1]).err() == Some(CM_FROZEN) && l.compare().err() == Some(CM_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![4, 5]);
        l.fold() == l.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 摘要口径。
    let j11 = {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![1, 2, 3]);
        let _ = l.record_actual(vec![1, 2, 3]);
        let _ = l.compare();
        l.summary() == "b=3 a=3 v=3"
    };
    s.add("J11-摘要口径", j11, "基准/实测/裁决量回读");

    // J12 重录覆盖。
    let j12 = {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![1, 2, 3, 4]);
        let _ = l.record_baseline(vec![7]);
        l.baseline == vec![7] && l.summary().contains("b=1")
    };
    s.add("J12-重录覆盖", j12, "重录覆盖旧基准");

    let got = inject_five();
    s.add_dyn("J13-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J14-指纹非零", {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![2]);
        let _ = l.record_actual(vec![2]);
        let _ = l.compare();
        l.fold() != 0
    }, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12206_compare_flow() {
        let mut l = OrderLedger::new();
        let _ = l.record_baseline(vec![1, 2]);
        assert_eq!(l.compare(), Err(CM_EMPTY));
        let _ = l.record_actual(vec![1, 3]);
        assert_eq!(l.compare(), Ok(1));
        assert!(l.all_equal() == false);
    }

    #[test]
    fn unx12206_checks_green() {
        let s = run_unx12206_checks();
        assert!(s.all_passed(), "UNX-F12206 判据未全绿");
    }

    #[test]
    fn unx12206_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
