//! UNX-F12061 TEB 字段台账（线程环境块关键槽位记账）（0x199C0 段）
//!
//! 判据原文：TEB 关键字段（Self 指针、ClientId、ExceptionList、
//! StackBase/StackLimit、LastErrorValue）以字段台账建模：每槽带
//! { 偏移, 宽度, 写者域 }；写入核校验域权限与同值拒；台账逐笔
//! 留痕；封存后拒写。
//!
//! 结构：字段钉位表（field_pin）+ 台账（TebLedger）+ 写核（四码
//! 守卫）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12061_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x199C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x199C0;
pub const TL_BAD_FIELD: u32 = SEG_BASE + 0x01; // 空留痕/未知槽/越界
pub const TL_WRONG_DOMAIN: u32 = SEG_BASE + 0x02; // 写者域无权
pub const TL_NOCHANGE: u32 = SEG_BASE + 0x03; // 同值重写
pub const TL_FROZEN: u32 = SEG_BASE + 0x04; // 印账后仍写
pub const TL_BAD_REC: u32 = SEG_BASE + 0x05; // 伪造台账

/// TEB 结构总长（64 位基准 0x1800 窗口内取实际尾）。
pub const TEB_TOTAL: u32 = 0x380;

/// 写者域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TDomain {
    Kernel,
    User,
}

/// TEB 字段钉位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TebFid {
    NtTibExceptionList = 1, // +0x000
    StackBase = 2,          // +0x008
    StackLimit = 3,         // +0x010
    SelfPtr = 4,            // +0x030
    ClientId = 5,           // +0x048
    LastErrorValue = 6,     // +0x068
    Win32ThreadInfo = 7,    // +0x058
}

/// 字段钉位表：(偏移, 宽度, 写者域)。
pub fn field_pin(f: TebFid) -> (u32, u32, TDomain) {
    match f {
        TebFid::NtTibExceptionList => (0x000, 8, TDomain::Kernel),
        TebFid::StackBase => (0x008, 8, TDomain::Kernel),
        TebFid::StackLimit => (0x010, 8, TDomain::Kernel),
        TebFid::SelfPtr => (0x030, 8, TDomain::Kernel),
        TebFid::ClientId => (0x048, 8, TDomain::Kernel),
        TebFid::Win32ThreadInfo => (0x058, 8, TDomain::User),
        TebFid::LastErrorValue => (0x068, 4, TDomain::User),
    }
}

/// 一笔字段写账。
#[derive(Clone)]
pub struct TebRec {
    pub seq: u32,
    pub fid: TebFid,
    pub old: u64,
    pub new: u64,
    pub note: String,
}

impl TebRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.old != self.new
    }
}

/// TEB 字段台账。
pub struct TebLedger {
    pub vals: Vec<(TebFid, u64)>,
    pub recs: Vec<TebRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TebLedger {
    pub fn new() -> TebLedger {
        TebLedger { vals: Vec::new(), recs: Vec::new(), seq_next: 1, sealed: false }
    }

    fn slot_of(&self, f: TebFid) -> Option<usize> {
        self.vals.iter().position(|v| v.0 == f)
    }

    /// 读核：未写过的字段回零值。
    pub fn read(&self, f: TebFid) -> u64 {
        match self.slot_of(f) {
            Some(i) => self.vals[i].1,
            None => 0,
        }
    }

    /// 写核：域权 → 越界 → 同值 → 落账。
    pub fn write(&mut self, who: TDomain, f: TebFid, val: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TL_FROZEN);
        }
        if note.is_empty() {
            return Err(TL_BAD_REC);
        }
        let (off, width, dom) = field_pin(f);
        if off + width > TEB_TOTAL {
            return Err(TL_BAD_FIELD);
        }
        if who != dom {
            return Err(TL_WRONG_DOMAIN);
        }
        let old = self.read(f);
        if old == val {
            return Err(TL_NOCHANGE);
        }
        let rec = TebRec { seq: self.seq_next, fid: f, old, new: val, note: note.to_string() };
        if !rec.ok() {
            return Err(TL_BAD_REC);
        }
        self.seq_next += 1;
        self.recs.push(rec);
        match self.slot_of(f) {
            Some(i) => self.vals[i].1 = val,
            None => self.vals.push((f, val)),
        }
        Ok(())
    }

    /// 台账完整性。
    pub fn ledger_ok(&self) -> bool {
        self.recs.iter().all(|r| r.ok())
    }

    /// 写账笔数。
    pub fn rec_count(&self) -> usize {
        self.recs.len()
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for v in &self.vals {
            h = h.wrapping_mul(211).wrapping_add(v.0 as u32);
            h = h.wrapping_mul(211).wrapping_add(v.1 as u32);
        }
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut l = TebLedger::new();
    // 1) 用户域写内核槽 → TL_WRONG_DOMAIN。
    got[0] = l.write(TDomain::User, TebFid::SelfPtr, 0x1000, "u")
        .err() == Some(TL_WRONG_DOMAIN);
    // 2) 内核域写用户槽 → TL_WRONG_DOMAIN。
    got[1] = l.write(TDomain::Kernel, TebFid::LastErrorValue, 5, "k")
        .err() == Some(TL_WRONG_DOMAIN);
    // 3) 空留痕 → TL_BAD_REC。
    got[2] = l.write(TDomain::User, TebFid::LastErrorValue, 1, "")
        .err() == Some(TL_BAD_REC);
    // 4) 同值重写 → TL_NOCHANGE。
    let _ = l.write(TDomain::User, TebFid::LastErrorValue, 1, "ok");
    got[3] = l.write(TDomain::User, TebFid::LastErrorValue, 1, "dup")
        .err() == Some(TL_NOCHANGE);
    // 5) 印账后写 → TL_FROZEN。
    l.seal();
    got[4] = l.write(TDomain::User, TebFid::LastErrorValue, 2, "post")
        .err() == Some(TL_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12061_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12061");

    // J1 钉位表：七槽偏移单调且不越窗。
    let j1 = {
        let mut prev_end = 0;
        let mut ok = true;
        let mut n = 1;
        while n <= 7 {
            let f = match n {
                1 => TebFid::NtTibExceptionList,
                2 => TebFid::StackBase,
                3 => TebFid::StackLimit,
                4 => TebFid::SelfPtr,
                5 => TebFid::ClientId,
                6 => TebFid::Win32ThreadInfo,
                _ => TebFid::LastErrorValue,
            };
            let (off, w, _) = field_pin(f);
            ok = ok && off >= prev_end && off + w <= TEB_TOTAL;
            prev_end = off + w;
            n += 1;
        }
        ok
    };
    s.add("J1-钉位单调", j1, "七槽偏移单调不越窗");

    // J2 正路径：用户域写 LastErrorValue 两次。
    let j2 = {
        let mut l = TebLedger::new();
        let a = l.write(TDomain::User, TebFid::LastErrorValue, 0x13, "set").is_ok();
        let b = l.write(TDomain::User, TebFid::LastErrorValue, 0, "clr").is_ok();
        a && b && l.read(TebFid::LastErrorValue) == 0
    };
    s.add("J2-用户槽读写", j2, "置错清错两笔");

    // J3 正路径：内核域写栈界。
    let j3 = {
        let mut l = TebLedger::new();
        let _ = l.write(TDomain::Kernel, TebFid::StackLimit, 0x5_0000, "lo");
        let _ = l.write(TDomain::Kernel, TebFid::StackBase, 0x9_0000, "hi");
        l.read(TebFid::StackLimit) == 0x5_0000 && l.read(TebFid::StackBase) == 0x9_0000
    };
    s.add("J3-内核栈槽", j3, "栈界双槽落账");

    // J4 域权拒（双向）。
    let j4 = {
        let mut l = TebLedger::new();
        let a = l.write(TDomain::User, TebFid::ClientId, 1, "u").err() == Some(TL_WRONG_DOMAIN);
        let b = l.write(TDomain::Kernel, TebFid::Win32ThreadInfo, 1, "k").err() == Some(TL_WRONG_DOMAIN);
        a && b
    };
    s.add("J4-域权拒", j4, "双向越权全拒");

    // J5 未写读回零。
    let j5 = TebLedger::new().read(TebFid::SelfPtr) == 0;
    s.add("J5-零值回读", j5, "未写槽回零");

    // J6 同值重写拒。
    let j6 = {
        let mut l = TebLedger::new();
        let _ = l.write(TDomain::Kernel, TebFid::SelfPtr, 0x7EF, "a");
        l.write(TDomain::Kernel, TebFid::SelfPtr, 0x7EF, "b").err() == Some(TL_NOCHANGE)
    };
    s.add("J6-同值拒", j6, "空变更拒");

    // J7 留痕对账：两笔账序号递增。
    let j7 = {
        let mut l = TebLedger::new();
        let _ = l.write(TDomain::Kernel, TebFid::StackBase, 1, "r1");
        let _ = l.write(TDomain::User, TebFid::LastErrorValue, 9, "r2");
        l.rec_count() == 2 && l.recs[0].seq == 1 && l.recs[1].seq == 2 && l.ledger_ok()
    };
    s.add("J7-留痕对账", j7, "双笔序号递增");

    // J8 空留痕拒。
    let j8 = TebLedger::new().write(TDomain::User, TebFid::LastErrorValue, 3, "")
        .err() == Some(TL_BAD_REC);
    s.add("J8-空留痕拒", j8, "留痕必填");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = TebLedger::new();
        let _ = a.write(TDomain::User, TebFid::LastErrorValue, 4, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = TebLedger::new();
        let _ = c.write(TDomain::User, TebFid::LastErrorValue, 5, "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异账异纹");

    // J10 印账冻结。
    let j10 = {
        let mut l = TebLedger::new();
        l.seal();
        l.write(TDomain::Kernel, TebFid::SelfPtr, 1, "post").err() == Some(TL_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后写必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：自指针钉值面（TEB 自指向）。
    let mut l = TebLedger::new();
    let _ = l.write(TDomain::Kernel, TebFid::SelfPtr, 0xDEAD_0000, "self");
    s.add("样例自指针", l.read(TebFid::SelfPtr) == 0xDEAD_0000, "Self 落账");
    s.add("样例账面合法", l.ledger_ok(), "逐笔过自洽");

    // J12 汇总面补充：写账按域清点（内核/用户双域各有落账）。
    // ClientId 双写演 pid→tid 演进，Win32ThreadInfo 单写记 GUI 挂接。
    let j12 = {
        let mut l = TebLedger::new();
        let _ = l.write(TDomain::Kernel, TebFid::ClientId, 0x1A2B, "pid");
        let _ = l.write(TDomain::Kernel, TebFid::ClientId, 0x3C4D, "tid");
        let _ = l.write(TDomain::User, TebFid::Win32ThreadInfo, 0x5A00, "gui");
        l.rec_count() == 3
            && l.recs.iter().filter(|r| r.fid == TebFid::ClientId).count() == 2
            && l.recs.iter().filter(|r| r.fid == TebFid::Win32ThreadInfo).count() == 1
            && l.ledger_ok()
            && l.read(TebFid::ClientId) == 0x3C4D
    };
    s.add("J12-域分布核", j12, "双域三笔按字段清点");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12061_write_flow() {
        let mut l = TebLedger::new();
        assert!(l.write(TDomain::Kernel, TebFid::StackBase, 0x90000, "a").is_ok());
        assert_eq!(l.write(TDomain::User, TebFid::StackBase, 1, "b").err(), Some(TL_WRONG_DOMAIN));
        assert_eq!(l.write(TDomain::Kernel, TebFid::StackBase, 0x90000, "c").err(), Some(TL_NOCHANGE));
        assert_eq!(l.read(TebFid::StackBase), 0x90000);
    }

    #[test]
    fn unx12061_checks_green() {
        let s = run_unx12061_checks();
        assert!(s.all_passed(), "UNX-F12061 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12061_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
