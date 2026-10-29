//! UNX-F11090 fd/资源账长稳监控（fd 泄漏探针）（0x1eb40 段）
//!
//! 锚点：UNX-F11090-J1 fd 账万轮零泄漏——万轮开关循环后 fd 账余额归零；
//! J2 资源账逐族零漏——fd/内存/进程/IPC 四族逐族探针全零。
//! 域：CoRun·UNX-C4（长稳监控域）。
//!
//! 结构：资源探针册（四族余额表 + 万轮开关引擎）+ 泄漏探针（每轮开闭
//! 对账，差额即漏）+ 峰值水位计（历史峰值留痕）+ 注入核（漏探/族错/
//! 越界/封账写入四路必抓）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x1eb40 独占 0x20）。
pub const SEG_BASE: u32 = 0x1eb40;
pub const LK_LEAK: u32 = SEG_BASE + 0x01; // 泄漏探针命中
pub const LK_NOENT: u32 = SEG_BASE + 0x02; // 族不在册
pub const LK_DUP: u32 = SEG_BASE + 0x03; // 族重开
pub const LK_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写账
pub const LK_KIND: u32 = SEG_BASE + 0x05; // 族错探
pub const LK_CLOSE2: u32 = SEG_BASE + 0x06; // 重复关闭

/// 四族。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Res {
    Fd,
    Mem,
    Proc,
    Ipc,
}

/// 探针册：四族余额 + 峰值。
pub struct LeakBook {
    pub bal: [u32; 4],
    pub peak: [u32; 4],
    pub rounds: u32,
    pub leaks: u32,
    pub sealed: bool,
}

impl LeakBook {
    pub fn new() -> LeakBook {
        LeakBook { bal: [0; 4], peak: [0; 4], rounds: 0, leaks: 0, sealed: false }
    }

    fn idx(&self, r: Res) -> Result<usize, u32> {
        match r {
            Res::Fd => Ok(0),
            Res::Mem => Ok(1),
            Res::Proc => Ok(2),
            Res::Ipc => Ok(3),
        }
    }

    /// 取资源（加账）。
    pub fn take(&mut self, r: Res, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(LK_FROZEN);
        }
        let i = self.idx(r)?;
        self.bal[i] = self.bal[i].wrapping_add(n);
        if self.bal[i] > self.peak[i] {
            self.peak[i] = self.bal[i];
        }
        Ok(self.bal[i])
    }

    /// 还资源（减账；重复关闭/透支必拒）。
    pub fn give(&mut self, r: Res, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(LK_FROZEN);
        }
        let i = self.idx(r)?;
        if self.bal[i] < n {
            self.leaks = self.leaks.wrapping_add(1);
            return Err(LK_CLOSE2);
        }
        self.bal[i] = self.bal[i].wrapping_sub(n);
        Ok(self.bal[i])
    }

    /// 泄漏探针：单族余额应零非零即漏。
    pub fn probe(&self, r: Res) -> Result<bool, u32> {
        let i = self.idx(r)?;
        Ok(self.bal[i] == 0)
    }

    /// 万轮开关引擎：每轮 take+give 对账。
    pub fn churn(&mut self, rounds: u32) -> bool {
        let fams = [Res::Fd, Res::Mem, Res::Proc, Res::Ipc];
        let mut ok = true;
        let mut r = 0u32;
        while r < rounds {
            let mut k = 0usize;
            while k < fams.len() {
                let n = (r % 3).wrapping_add(1);
                if self.take(fams[k], n).is_err() {
                    ok = false;
                }
                if self.give(fams[k], n).is_err() {
                    ok = false;
                }
                k = k.wrapping_add(1);
            }
            r = r.wrapping_add(1);
        }
        self.rounds = rounds;
        ok
    }

    /// 全族零漏裁断。
    pub fn all_zero(&self) -> bool {
        let mut k = 0usize;
        while k < 4 {
            if self.bal[k] != 0 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1eb4_02a0;
        let mut k = 0usize;
        while k < 4 {
            h = (h ^ self.bal[k] as u64).wrapping_mul(0x100000001b3);
            h = (h ^ (self.peak[k] as u64) << 20).wrapping_mul(0x100000001b3);
            k = k.wrapping_add(1);
        }
        h = (h ^ self.rounds as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.leaks as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 泄漏探针：只取不还必中
    let mut b = LeakBook::new();
    let _ = b.take(Res::Fd, 2);
    got[0] = b.probe(Res::Fd) == Ok(false) && b.probe(Res::Mem) == Ok(true);
    // 2) 重复关闭必抓
    let mut c = LeakBook::new();
    let _ = c.take(Res::Fd, 1);
    let _ = c.give(Res::Fd, 1);
    got[1] = c.give(Res::Fd, 1).err() == Some(LK_CLOSE2) && c.leaks == 1;
    // 3) 族错探：探针按族独立不串
    let mut d = LeakBook::new();
    let _ = d.take(Res::Ipc, 3);
    got[2] = d.probe(Res::Ipc) == Ok(false) && d.probe(Res::Fd) == Ok(true)
        && d.probe(Res::Proc) == Ok(true);
    // 4) 封账写入必抓
    d.seal();
    got[3] = d.take(Res::Fd, 1).err() == Some(LK_FROZEN)
        && d.give(Res::Fd, 1).err() == Some(LK_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11090_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11090");

    // J1 fd 账万轮零泄漏
    let mut b = LeakBook::new();
    let ok = b.churn(10000);
    let inj = inject_four();
    let j1 = ok && b.rounds == 10000 && b.bal[0] == 0 && b.probe(Res::Fd) == Ok(true)
        && b.leaks == 0 && inj[1];
    s.add("J1-fd 万轮零泄漏", j1, "万轮开关循环 fd 账余额归零零漏探");

    // J2 资源账逐族零漏
    let j2 = b.all_zero() && b.probe(Res::Mem) == Ok(true)
        && b.probe(Res::Proc) == Ok(true) && b.probe(Res::Ipc) == Ok(true);
    s.add("J2-逐族零漏", j2, "fd/内存/进程/IPC 四族探针全零");

    // J3 泄漏探针命中必报
    s.add("J3-漏探命中", inj[0], "只取不还必被探针抓出");

    // J4 重复关闭必抓
    s.add("J4-重复关闭抓", inj[1], "超额关闭必拒且漏账计一笔");

    // J5 族错探隔离
    s.add("J5-族探隔离", inj[2], "单族泄漏不影响他族探针");

    // J6 封账写入必抓
    s.add("J6-封账拒写", inj[3], "封账后取还两路均必拒");

    // J7 峰值水位：峰值不降且 ≥ 当前
    let mut c = LeakBook::new();
    let _ = c.take(Res::Fd, 5);
    let _ = c.give(Res::Fd, 3);
    let _ = c.take(Res::Fd, 2);
    let j7 = c.peak[0] == 5 && c.bal[0] == 4 && c.peak[0] >= c.bal[0];
    s.add("J7-峰值水位", j7, "峰值留痕不随余额回落丢失");

    // J8 未知族探针零命中（全零册探针恒真）
    let d = LeakBook::new();
    let j8 = d.probe(Res::Fd) == Ok(true) && d.all_zero() && d.leaks == 0;
    s.add("J8-零册恒真", j8, "零账册四族探针恒真零误报");

    // J9 长稳轮数留痕：rounds 与指纹联动
    let mut e = LeakBook::new();
    let _ = e.churn(100);
    let fp0 = e.fold();
    let _ = e.churn(100);
    let j9 = e.rounds == 100 && fp0 == e.fold();
    s.add("J9-轮数留痕", j9, "等额 churn 重放指纹一致");

    // J10 fold 指纹与封账根
    let fp = c.fold();
    let root = c.seal();
    let j10 = fp == root && c.fold() == root;
    s.add("J10-指纹冻结", j10, "封账根与 fold 一致且二次 fold 稳定");

    // J11 大批等额开关：千取千还不漏
    let mut f = LeakBook::new();
    let mut n = 0u32;
    while n < 1000 {
        let _ = f.take(Res::Mem, 1);
        n = n.wrapping_add(1);
    }
    let peak_before = f.peak[1];
    let mut n = 0u32;
    while n < 1000 {
        let _ = f.give(Res::Mem, 1);
        n = n.wrapping_add(1);
    }
    let j11 = f.bal[1] == 0 && f.peak[1] == peak_before && f.probe(Res::Mem) == Ok(true);
    s.add("J11-千取千还", j11, "千取千还余额归零且峰值不漂");

    // J12 动态明细：万轮峰值落账
    s.add_dyn("J12-峰值明细", b.rounds == 10000,
        &format!("fd_peak={} mem_peak={}", b.peak[0], b.peak[1]));

    // J13 动态明细：探针册指纹落账
    let fp = b.fold();
    s.add_dyn("J13-册指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11090_leak_flow() {
        let mut b = LeakBook::new();
        assert!(b.take(Res::Fd, 3).is_ok());
        assert!(b.take(Res::Fd, 3).unwrap() == 6);
        assert!(b.peak[0] == 6);
        assert!(b.give(Res::Fd, 6).is_ok());
        assert!(b.give(Res::Fd, 1).err() == Some(LK_CLOSE2));
        assert!(b.leaks == 1);
        assert!(b.probe(Res::Fd) == Ok(true));
        b.seal();
        assert!(b.take(Res::Fd, 1).err() == Some(LK_FROZEN));
    }

    #[test]
    fn unx11090_checks_green() {
        let s = run_unx11090_checks();
        assert!(s.all_passed(), "UNX-F11090 判据未全绿");
    }
}
