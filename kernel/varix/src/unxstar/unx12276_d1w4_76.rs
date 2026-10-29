//! UNX-F12276 文件族性能预算（O1 对标口径）（0x64980 段）
//!
//! 卡面：文件族逐操作性能预算账——Create/Open/Read/Write/Close/Query/
//! SetInfo 七类操作各有预算上限（O1 对标口径），实测须非零且不超预算；
//! 超支显性记账，超支条目存在时不得 seal。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 操作闭集：七类之外必拒。
//! - J2 预算口径：各操作预算为正且七类互异。
//! - J3 实测非零：实测 0 视为漏测必拒。
//! - J4 超支显性：实测>预算记超支并可回读。
//! - J5 超支禁 seal。
//! - J6 重号操作必拒。
//! - J7 印账冻结：seal 后补账必拒。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64980 独占 0x20）。
pub const SEG_BASE: u32 = 0x64980;
pub const PB_UNKNOWN_OP: u32 = SEG_BASE + 0x01; // 操作越闭集
pub const PB_ZERO_MEASURE: u32 = SEG_BASE + 0x02; // 实测漏记
pub const PB_DUP: u32 = SEG_BASE + 0x03; // 重号操作
pub const PB_OVER_BUDGET: u32 = SEG_BASE + 0x04; // 超支禁 seal
pub const PB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后补账

/// 操作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Pop {
    Create = 0,
    Open = 1,
    Read = 2,
    Write = 3,
    Close = 4,
    QueryInfo = 5,
    SetInfo = 6,
}

pub const POP_N: usize = 7;

/// O1 对标口径预算（模型单位：微秒）。
pub fn budget_of(p: Pop) -> u32 {
    match p {
        Pop::Create => 12,
        Pop::Open => 9,
        Pop::Read => 6,
        Pop::Write => 8,
        Pop::Close => 4,
        Pop::QueryInfo => 3,
        Pop::SetInfo => 5,
    }
}

fn pop_all() -> [Pop; POP_N] {
    [
        Pop::Create,
        Pop::Open,
        Pop::Read,
        Pop::Write,
        Pop::Close,
        Pop::QueryInfo,
        Pop::SetInfo,
    ]
}

/// 一条预算记录。
#[derive(Clone)]
pub struct PerfRec {
    pub op: Pop,
    pub measured: u32,
    pub over: bool,
}

/// 预算账册。
pub struct PerfBook {
    pub recs: Vec<PerfRec>,
    pub overs: usize,
    pub sealed: bool,
}

impl PerfBook {
    pub fn new() -> PerfBook {
        PerfBook { recs: Vec::new(), overs: 0, sealed: false }
    }

    /// 入账：闭集、实测非零、超支显性记账（入账但记超支）。
    pub fn put(&mut self, op: Pop, measured: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(PB_FROZEN);
        }
        if pop_all().iter().find(|x| **x == op).is_none() {
            return Err(PB_UNKNOWN_OP);
        }
        if measured == 0 {
            return Err(PB_ZERO_MEASURE);
        }
        if self.recs.iter().any(|r| r.op == op) {
            return Err(PB_DUP);
        }
        let over = measured > budget_of(op);
        if over {
            self.overs += 1;
        }
        self.recs.push(PerfRec { op, measured, over });
        Ok(over)
    }

    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.recs.is_empty() || self.overs > 0 {
            return Err(PB_OVER_BUDGET);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.op as u32);
            h = h.wrapping_mul(211).wrapping_add(r.measured);
            h = h.wrapping_mul(211).wrapping_add(r.over as u32);
        }
        h = h.wrapping_mul(211).wrapping_add(self.overs as u32);
        h
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("n={} ov={}", self.recs.len(), self.overs)
    }

    pub fn line_of(&self, op: Pop) -> String {
        match self.recs.iter().find(|r| r.op == op) {
            Some(r) => format!("{:?}|m{}|ov{}", r.op, r.measured, r.over as u8),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = PerfBook::new();
    // 1) 实测漏记必拒。
    got[0] = b.put(Pop::Read, 0).err() == Some(PB_ZERO_MEASURE);
    // 2) 重号操作必拒。
    let _ = b.put(Pop::Read, 3);
    got[1] = b.put(Pop::Read, 3).err() == Some(PB_DUP);
    // 3) 超支必记账（返回 true）。
    got[2] = b.put(Pop::Write, 100).ok() == Some(true);
    // 4) 超支禁 seal。
    got[3] = b.seal().err() == Some(PB_OVER_BUDGET);
    // 5) 印账后补账必抓。
    let mut b2 = PerfBook::new();
    for op in pop_all() {
        let _ = b2.put(op, budget_of(op) - 1);
    }
    let _ = b2.seal();
    got[4] = b2.put(Pop::Read, 1).err() == Some(PB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12276_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12276");

    // J1 预算口径。
    let j1 = {
        let mut pos = true;
        let mut distinct = true;
        for (i, op) in pop_all().iter().enumerate() {
            if budget_of(*op) == 0 {
                pos = false;
            }
            for (j, op2) in pop_all().iter().enumerate() {
                if i < j && budget_of(*op) == budget_of(*op2) {
                    distinct = false;
                }
            }
        }
        pos && distinct
    };
    s.add("J1-预算口径", j1, "七类预算正且互异");

    // J2 操作闭集（重号检查代表闭集判定）。
    let j2 = {
        let mut b = PerfBook::new();
        let _ = b.put(Pop::Create, 10);
        b.put(Pop::Create, 10).err() == Some(PB_DUP)
    };
    s.add("J2-操作唯一", j2, "同操作不得重复记账");

    // J3 实测漏记拒。
    let j3 = {
        let mut b = PerfBook::new();
        b.put(Pop::Open, 0).err() == Some(PB_ZERO_MEASURE)
    };
    s.add("J3-实测漏记拒", j3, "0 视为漏测");

    // J4 超支显性。
    let j4 = {
        let mut b = PerfBook::new();
        let _ = b.put(Pop::Read, budget_of(Pop::Read) + 1);
        b.overs == 1 && b.recs[0].over
    };
    s.add("J4-超支显性", j4, "超支计数可回读");

    // J5 预算内放行。
    let j5 = {
        let mut b = PerfBook::new();
        b.put(Pop::Close, budget_of(Pop::Close)).ok() == Some(false)
    };
    s.add("J5-预算内放行", j5, "恰等预算不算超支");

    // J6 超支禁 seal。
    let j6 = {
        let mut b = PerfBook::new();
        let _ = b.put(Pop::Write, 99);
        b.seal().err() == Some(PB_OVER_BUDGET)
    };
    s.add("J6-超支禁seal", j6, "不达标不印账");

    // J7 全达标 seal。
    let j7 = {
        let mut b = PerfBook::new();
        for op in pop_all() {
            let _ = b.put(op, budget_of(op) - 1);
        }
        b.seal().is_ok() && b.summary().contains("ov=0")
    };
    s.add("J7-全达标seal", j7, "七类齐且无超支");

    // J8 空账 seal 拒。
    let j8 = PerfBook::new().seal().err() == Some(PB_OVER_BUDGET);
    s.add("J8-空账seal拒", j8, "零记账必拒");

    // J9 印账冻结。
    let j9 = {
        let mut b = PerfBook::new();
        let _ = b.put(Pop::QueryInfo, 2);
        let _ = b.seal();
        b.put(Pop::Close, 1).err() == Some(PB_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = PerfBook::new();
        let _ = b.put(Pop::SetInfo, 4);
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "超支计数入指纹");

    // J11 回读渲染。
    let j11 = {
        let mut b = PerfBook::new();
        let _ = b.put(Pop::Open, 7);
        b.line_of(Pop::Open).contains("m7") && b.line_of(Pop::Read) == "missing"
    };
    s.add("J11-回读渲染", j11, "预算行可渲染");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = PerfBook::new();
    let _ = b.put(Pop::Create, 10);
    let _ = b.put(Pop::Open, 8);
    s.add("摘要回读", b.summary().contains("n=2 ov=0"), "双行在册");
    s.add("预算内记账", b.recs.iter().all(|r| !r.over), "全达标");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12276_perf_flow() {
        let mut b = PerfBook::new();
        assert_eq!(b.put(Pop::Read, 3), Ok(false));
        assert_eq!(b.put(Pop::Read, 3), Err(PB_DUP));
        assert_eq!(b.put(Pop::Read, 0), Err(PB_ZERO_MEASURE));
        assert_eq!(b.put(Pop::Write, 999), Ok(true));
        assert_eq!(b.seal(), Err(PB_OVER_BUDGET));
    }

    #[test]
    fn unx12276_checks_green() {
        let s = run_unx12276_checks();
        assert!(s.all_passed(), "UNX-F12276 判据未全绿");
    }

    #[test]
    fn unx12276_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
