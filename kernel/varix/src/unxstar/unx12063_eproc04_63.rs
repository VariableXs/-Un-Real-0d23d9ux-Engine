//! UNX-F12063 ClientId 进程线程对编码（0x19A00 段）
//!
//! 判据原文：ClientId 是 { UniqueProcess, UniqueThread } 对；
//! 语义规则：进程号与线程号线程位约定（线程号必为偶数槽编码；
//! 进程号任意非零）；空对拒；对账簿按 (pid, tid) 唯一登记；
//! 解码按位拆装；封存后拒登记。
//!
//! 结构：对编码核（CidPair 编解码）+ 对账簿（CidBook 唯一登记）
//! + 四码守卫 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12063_checks）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19A00 独占 0x20）。
pub const SEG_BASE: u32 = 0x19A00;
pub const CI_ZERO_PAIR: u32 = SEG_BASE + 0x01; // 零对拒
pub const CI_TID_ODD: u32 = SEG_BASE + 0x02; // 线程号奇数槽
pub const CI_DUP: u32 = SEG_BASE + 0x03; // 重复登记
pub const CI_FROZEN: u32 = SEG_BASE + 0x04; // 印账后仍登记
pub const CI_BAD_ENC: u32 = SEG_BASE + 0x05; // 伪编码

/// 进程线程对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CidPair {
    pub pid: u32,
    pub tid: u32,
}

impl CidPair {
    /// 语义校验：pid/tid 非零；tid 必须偶数（低位槽约定）。
    pub fn ok(&self) -> Result<(), u32> {
        if self.pid == 0 || self.tid == 0 {
            return Err(CI_ZERO_PAIR);
        }
        if self.tid % 2 != 0 {
            return Err(CI_TID_ODD);
        }
        Ok(())
    }

    /// 64 位打包：高 32 pid，低 32 tid。
    pub fn pack(&self) -> u64 {
        ((self.pid as u64) << 32) | (self.tid as u64)
    }

    /// 64 位解包。
    pub fn unpack(v: u64) -> CidPair {
        CidPair { pid: (v >> 32) as u32, tid: v as u32 }
    }
}

/// 对账簿。
pub struct CidBook {
    pub entries: Vec<CidPair>,
    pub sealed: bool,
}

impl CidBook {
    pub fn new() -> CidBook {
        CidBook { entries: Vec::new(), sealed: false }
    }

    /// 登记核：语义校验 + 唯一性。
    pub fn enroll(&mut self, p: CidPair) -> Result<usize, u32> {
        if self.sealed {
            return Err(CI_FROZEN);
        }
        p.ok()?;
        if self.entries.iter().any(|e| e.pid == p.pid && e.tid == p.tid) {
            return Err(CI_DUP);
        }
        self.entries.push(p);
        Ok(self.entries.len() - 1)
    }

    /// 按进程清点线程。
    pub fn threads_of(&self, pid: u32) -> usize {
        self.entries.iter().filter(|e| e.pid == pid).count()
    }

    /// 进程清单（去重）。
    pub fn distinct_pids(&self) -> u32 {
        let mut pids: Vec<u32> = Vec::new();
        for e in &self.entries {
            if !pids.iter().any(|p| *p == e.pid) {
                pids.push(e.pid);
            }
        }
        pids.len() as u32
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.pid & 0xFF);
            h = h.wrapping_mul(211).wrapping_add(e.tid & 0xFF);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CidBook::new();
    // 1) 零对（pid=0）→ CI_ZERO_PAIR。
    got[0] = b.enroll(CidPair { pid: 0, tid: 4 }).err() == Some(CI_ZERO_PAIR);
    // 2) 零对（tid=0）→ CI_ZERO_PAIR。
    got[1] = b.enroll(CidPair { pid: 1, tid: 0 }).err() == Some(CI_ZERO_PAIR);
    // 3) 奇数线程号 → CI_TID_ODD。
    got[2] = b.enroll(CidPair { pid: 100, tid: 7 }).err() == Some(CI_TID_ODD);
    // 4) 重复登记 → CI_DUP。
    let _ = b.enroll(CidPair { pid: 100, tid: 8 });
    got[3] = b.enroll(CidPair { pid: 100, tid: 8 }).err() == Some(CI_DUP);
    // 5) 印账后登记 → CI_FROZEN。
    b.seal();
    got[4] = b.enroll(CidPair { pid: 200, tid: 10 }).err() == Some(CI_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12063_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12063");

    // J1 打包解包往返。
    let j1 = {
        let p = CidPair { pid: 0x1234, tid: 0x5678 };
        CidPair::unpack(p.pack()) == p
    };
    s.add("J1-编解码往返", j1, "64 位拆装一致");

    // J2 零对双面拒。
    let j2 = {
        let a = CidPair { pid: 0, tid: 4 }.ok().err() == Some(CI_ZERO_PAIR);
        let b = CidPair { pid: 1, tid: 0 }.ok().err() == Some(CI_ZERO_PAIR);
        a && b
    };
    s.add("J2-零对拒", j2, "pid/tid 非零");

    // J3 奇数线程号拒。
    let j3 = CidPair { pid: 100, tid: 9 }.ok().err() == Some(CI_TID_ODD);
    s.add("J3-奇槽拒", j3, "tid 偶数约定");

    // J4 登记与清点。
    let j4 = {
        let mut b = CidBook::new();
        let _ = b.enroll(CidPair { pid: 100, tid: 2 });
        let _ = b.enroll(CidPair { pid: 100, tid: 4 });
        let _ = b.enroll(CidPair { pid: 200, tid: 6 });
        b.threads_of(100) == 2 && b.distinct_pids() == 2
    };
    s.add("J4-对账清点", j4, "双进程三线程");

    // J5 重复登记拒。
    let j5 = {
        let mut b = CidBook::new();
        let _ = b.enroll(CidPair { pid: 1, tid: 2 });
        b.enroll(CidPair { pid: 1, tid: 2 }).err() == Some(CI_DUP)
    };
    s.add("J5-重复拒", j5, "对唯一");

    // J6 印账冻结。
    let j6 = {
        let mut b = CidBook::new();
        b.seal();
        b.enroll(CidPair { pid: 9, tid: 2 }).err() == Some(CI_FROZEN)
    };
    s.add("J6-印账冻结", j6, "印后登记拒");

    // J7 指纹区分度与幂等。
    let j7 = {
        let mut a = CidBook::new();
        let _ = a.enroll(CidPair { pid: 1, tid: 2 });
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = CidBook::new();
        let _ = c.enroll(CidPair { pid: 1, tid: 4 });
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J7-指纹性质", j7, "幂等且异簿异纹");

    // J8 pack 钉值核对。
    let j8 = {
        let p = CidPair { pid: 0x0000_00A1, tid: 0x0000_00BC };
        p.pack() == 0x0000_00A1_0000_00BC
    };
    s.add("J8-钉值打包", j8, "高低半区钉值");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // 汇总样例：典型 4-4 对。
    let mut b = CidBook::new();
    let _ = b.enroll(CidPair { pid: 4, tid: 4 });
    s.add("样例系统对", b.threads_of(4) == 1, "System 4-4");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12063_pack_roundtrip() {
        let p = CidPair { pid: 0xDEAD, tid: 0xBEE0 };
        assert_eq!(CidPair::unpack(p.pack()), p);
    }

    #[test]
    fn unx12063_pack_edge() {
        // 钉值面：System 4-4 与边界偶号线程号。
        let sys = CidPair { pid: 4, tid: 4 };
        assert!(sys.ok().is_ok());
        assert_eq!(sys.pack(), 0x0000_0004_0000_0004);
        let even = CidPair { pid: 0xFFFF_FFFF, tid: 0xFFFF_FFFE };
        assert!(even.ok().is_ok());
        assert_eq!(CidPair::unpack(even.pack()), even);
        // 奇槽 / 零对双拒。
        assert_eq!(CidPair { pid: 4, tid: 5 }.ok().err(), Some(CI_TID_ODD));
        assert_eq!(CidPair { pid: 0, tid: 2 }.ok().err(), Some(CI_ZERO_PAIR));
    }

    #[test]
    fn unx12063_checks_green() {
        let s = run_unx12063_checks();
        assert!(s.all_passed(), "UNX-F12063 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12063_book_tally() {
        let mut b = CidBook::new();
        let _ = b.enroll(CidPair { pid: 100, tid: 2 });
        let _ = b.enroll(CidPair { pid: 100, tid: 4 });
        let _ = b.enroll(CidPair { pid: 200, tid: 6 });
        assert_eq!(b.threads_of(100), 2);
        assert_eq!(b.threads_of(200), 1);
        assert_eq!(b.distinct_pids(), 2);
        assert_eq!(b.enroll(CidPair { pid: 100, tid: 4 }).err(), Some(CI_DUP));
        let f1 = b.fold();
        let f2 = b.fold();
        assert_eq!(f1, f2);
        assert_ne!(f1, SEG_BASE);
    }

    #[test]
    fn unx12063_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
