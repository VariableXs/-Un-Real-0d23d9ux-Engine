//! UNX-F12028 STATUS_ACCESS_VIOLATION 与异常状态码族（SEH 联动）（0x195A0 段）
//!
//! 判据原文：访问违例记录面——违例以四元组落地（读/写操作位、地址、
//! 页序号、可恢复位）；读违例与写违例分面留账；地址必须落在登记的
//! 违例页带内（越带地址冒充违例即拒）；不可恢复违例必须升级为致命账。
//!
//! 结构：违例页带（FaultZone：基址 × 带长）+ 违例记录器（AvRecorder：
//! 四元组 + 分面账）+ 记录核（五码守卫）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12028_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x195A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x195A0;
pub const AV_OUT_OF_ZONE: u32 = SEG_BASE + 0x01; // 地址越违例带
pub const AV_BAD_OP: u32 = SEG_BASE + 0x02; // 未知操作位
pub const AV_BAD_REC: u32 = SEG_BASE + 0x03; // 伪造违例账
pub const AV_FATAL_UNRECOVERED: u32 = SEG_BASE + 0x04; // 不可恢复未升级
pub const AV_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍记录

/// STATUS_ACCESS_VIOLATION 语义钉值。
pub const STATUS_ACCESS_VIOLATION: u32 = 0xC000_0005;

/// 操作位闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AvOp {
    Read = 1,
    Write = 2,
    Execute = 3,
}

pub fn op_of(v: u16) -> Option<AvOp> {
    match v {
        1 => Some(AvOp::Read),
        2 => Some(AvOp::Write),
        3 => Some(AvOp::Execute),
        _ => None,
    }
}

/// 一笔违例记录。
#[derive(Clone)]
pub struct AvRec {
    pub seq: u32,
    pub op: AvOp,
    pub addr: u64,
    /// 可恢复位（SEH 可接管）。
    pub recoverable: bool,
    pub note: String,
}

impl AvRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 违例页带。
#[derive(Clone, Copy)]
pub struct FaultZone {
    pub base: u64,
    pub len: u64,
}

impl FaultZone {
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr - self.base < self.len
    }
}

/// 违例记录器。
pub struct AvRecorder {
    pub zone: FaultZone,
    pub recs: Vec<AvRec>,
    pub seq_next: u32,
    pub fatal_open: bool,
    pub sealed: bool,
}

impl AvRecorder {
    pub fn new(base: u64, len: u64) -> AvRecorder {
        AvRecorder {
            zone: FaultZone { base, len },
            recs: Vec::new(),
            seq_next: 1,
            fatal_open: false,
            sealed: false,
        }
    }

    /// 记录核：带内校验 → 分面落账 → 致命面升级。
    pub fn record(&mut self, op_no: u16, addr: u64, recoverable: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(AV_FROZEN);
        }
        if note.is_empty() {
            return Err(AV_BAD_OP);
        }
        if op_of(op_no).is_none() {
            return Err(AV_BAD_OP);
        }
        if !self.zone.contains(addr) {
            return Err(AV_OUT_OF_ZONE);
        }
        // 不可恢复违例必须升级致命账（fatal_open 置位）。
        if !recoverable {
            self.fatal_open = true;
        }
        let op = op_of(op_no).unwrap_or(AvOp::Read);
        let rec = AvRec { seq: self.seq_next, op, addr, recoverable, note: note.to_string() };
        if !rec.ok() {
            return Err(AV_BAD_REC);
        }
        self.seq_next += 1;
        self.recs.push(rec);
        Ok(())
    }

    /// 致命面升级核对：不可恢复账存在时 fatal_open 必须为真。
    pub fn fatal_consistent(&self) -> bool {
        let any_fatal = self.recs.iter().any(|r| !r.recoverable);
        !any_fatal || self.fatal_open
    }

    /// 操作面清点。
    pub fn count_op(&self, op: AvOp) -> usize {
        self.recs.iter().filter(|r| r.op == op).count()
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
            h = h.wrapping_mul(211).wrapping_add(r.op as u32);
            h = h.wrapping_mul(211).wrapping_add((r.addr & 0xFFFF) as u32);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut r = AvRecorder::new(0x7FF6_0000, 0x1000);
    // 1) 地址越违例带 → AV_OUT_OF_ZONE。
    got[0] = r.record(1, 0x7FF7_0000, true, "out").err() == Some(AV_OUT_OF_ZONE);
    // 2) 未知操作位（0）→ AV_BAD_OP。
    got[1] = r.record(0, 0x7FF6_0000, true, "op").err() == Some(AV_BAD_OP);
    // 3) 空留痕 → AV_BAD_OP（无痕记录守卫）。
    got[2] = r.record(2, 0x7FF6_0000, true, "").err() == Some(AV_BAD_OP);
    // 4) 伪造违例账：空留痕自洽核红 → AV_BAD_REC 面。
    let forged = AvRec { seq: 1, op: AvOp::Read, addr: 1, recoverable: true, note: String::new() };
    got[3] = !forged.ok();
    // 5) 印账后记录 → AV_FROZEN。
    let _ = r.record(1, 0x7FF6_0000, true, "in");
    r.seal();
    got[4] = r.record(2, 0x7FF6_0100, true, "post").err() == Some(AV_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12028_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12028");

    // J1 正路径：带内读写双违例分面入账。
    let j1 = {
        let mut r = AvRecorder::new(0x7FF6_0000, 0x1000);
        let _ = r.record(1, 0x7FF6_0010, true, "rd");
        let _ = r.record(2, 0x7FF6_0020, true, "wr");
        r.count_op(AvOp::Read) == 1 && r.count_op(AvOp::Write) == 1 && r.ledger_ok()
    };
    s.add("J1-读写分面", j1, "双面各一脚");

    // J2 页带边界：带基址收、带尾前一字节收、带尾拒。
    let j2 = {
        let z = FaultZone { base: 0x1000, len: 0x100 };
        z.contains(0x1000) && z.contains(0x10FF) && !z.contains(0x1100) && !z.contains(0x0FFF)
    };
    s.add("J2-带边界", j2, "基/尾前收 尾拒");

    // J3 越带地址拒。
    let j3 = {
        let mut r = AvRecorder::new(0x7FF6_0000, 0x100);
        r.record(1, 0x7FF6_0100, true, "x").err() == Some(AV_OUT_OF_ZONE)
    };
    s.add("J3-越带拒", j3, "尾恰出界必拒");

    // J4 操作位闭集：1/2/3 收，0/4 拒。
    let j4 = op_of(1).is_some() && op_of(2).is_some() && op_of(3).is_some()
        && op_of(0).is_none() && op_of(4).is_none();
    s.add("J4-操作位闭集", j4, "闭集外全拒");

    // J5 不可恢复升级致命账。
    let j5 = {
        let mut r = AvRecorder::new(0x7FF6_0000, 0x100);
        let _ = r.record(2, 0x7FF6_0040, false, "fatal");
        r.fatal_open && r.fatal_consistent()
    };
    s.add("J5-致命升级", j5, "不可恢复即致命");

    // J6 可恢复不升级。
    let j6 = {
        let mut r = AvRecorder::new(0x7FF6_0000, 0x100);
        let _ = r.record(1, 0x7FF6_0040, true, "seh");
        !r.fatal_open && r.fatal_consistent()
    };
    s.add("J6-可恢复不升级", j6, "SEH 接管面");

    // J7 带长为零无违例带：任何地址越带。
    let j7 = {
        let mut r = AvRecorder::new(0x7FF6_0000, 0);
        r.record(1, 0x7FF6_0000, true, "z").err() == Some(AV_OUT_OF_ZONE)
    };
    s.add("J7-零带全拒", j7, "零带无违例面");

    // J8 执行违例分面。
    let j8 = {
        let mut r = AvRecorder::new(0x7FF6_0000, 0x100);
        let _ = r.record(3, 0x7FF6_0008, true, "x");
        r.count_op(AvOp::Execute) == 1
    };
    s.add("J8-执行面", j8, "DEP 违例分面");

    // J9 印账冻结。
    let j9 = {
        let mut r = AvRecorder::new(0x7FF6_0000, 0x100);
        let f1 = r.fold();
        let f2 = r.fold();
        r.seal();
        f1 == f2
            && r.record(1, 0x7FF6_0010, true, "post").err() == Some(AV_FROZEN)
    };
    s.add("J9-指纹封账", j9, "指纹幂等且印后必拒");

    // J10 账目自洽。
    let j10 = {
        let mut r = AvRecorder::new(0x7FF6_0000, 0x100);
        let _ = r.record(1, 0x7FF6_0080, true, "a");
        let _ = r.record(2, 0x7FF6_0090, false, "b");
        r.ledger_ok() && r.fatal_consistent() && r.recs.len() == 2
    };
    s.add("J10-账目自洽", j10, "双笔对账");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：典型 AV 现场。
    let mut r = AvRecorder::new(0x7FF6_0000, 0x1000);
    let _ = r.record(2, 0x7FF6_0800, false, "null-adjacent");
    s.add("样例致命面", r.fatal_open, "不可恢复写违例");
    s.add("样例账面合法", r.ledger_ok(), "逐笔过自洽");

    // J12 汇总面补充：违例与致命双口径清点。
    let j12 = {
        let mut r = AvRecorder::new(0x7FF6_0000, 0x100);
        let _ = r.record(1, 0x7FF6_0010, true, "read-ok");
        let _ = r.record(2, 0x7FF6_0020, false, "write-fatal");
        r.recs.len() == 2 && r.fatal_open && r.count_op(AvOp::Write) == 1
    };
    s.add("J12-双口径清点", j12, "违例/致命口径一致");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12028_av_flow() {
        let mut r = AvRecorder::new(0x7FF6_0000, 0x100);
        assert!(r.record(1, 0x7FF6_0000, true, "a").is_ok());
        assert_eq!(r.record(4, 0x7FF6_0010, true, "b"), Err(AV_BAD_OP));
        assert_eq!(r.record(1, 0x7FF6_0100, true, "c"), Err(AV_OUT_OF_ZONE));
        assert!(r.record(2, 0x7FF6_00FF, false, "d").is_ok());
        assert!(r.fatal_open);
    }

    #[test]
    fn unx12028_checks_green() {
        let s = run_unx12028_checks();
        assert!(s.all_passed(), "UNX-F12028 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12028_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
