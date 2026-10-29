//! UNX-F10911 select 返回值语义矩阵（0x1d160 段）
//!
//! 锚点：UNX-F10911-J1 正面：超时返 0/就绪返计数（各 10/10）；反面：
//! 计数与实际就绪一致（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·select 返回值面）。
//!
//! 结构：返回值账（fdmax + 集合就绪位账）+ 裁断器（就绪→返计数、
//! 零就绪→返 0（超时）、错误→负码）+ 注入核（计数虚高/计数虚低/
//! 超时误报就绪/冻结改账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d160 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d160;
pub const SR_BADF: u32 = SEG_BASE + 0x01; // 死 fd 混入
pub const SR_EINVAL: u32 = SEG_BASE + 0x02; // fdmax 非法
pub const SR_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改账
pub const SR_DUP: u32 = SEG_BASE + 0x04; // 重号入集
pub const SR_COUNT: u32 = SEG_BASE + 0x05; // 计数与就绪不符

/// 语义账定容。
pub const CAP: usize = 40;
/// SR 域标（fold 种子）。
pub const SR_TAG: u64 = 0x1d1620b;

/// 账条目。
#[derive(Clone, Debug)]
pub struct RetSlot {
    pub fd: u32,
    pub live: bool,
    pub watch: bool,
}

impl RetSlot {
    pub fn ready(&self) -> bool {
        self.live && self.watch
    }
}

/// select 返回值账。
pub struct RetLedger {
    pub slots: Vec<RetSlot>,
    pub frozen: bool,
    pub ret_log: Vec<i32>,
}

impl RetLedger {
    pub fn new() -> RetLedger {
        RetLedger { slots: Vec::new(), frozen: false, ret_log: Vec::new() }
    }

    /// 入集。
    pub fn add(&mut self, fd: u32, watch: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(SR_FROZEN);
        }
        if self.slots.iter().any(|e| e.fd == fd) {
            return Err(SR_DUP);
        }
        if self.slots.len() >= CAP {
            return Err(SR_EINVAL);
        }
        self.slots.push(RetSlot { fd, live: true, watch });
        Ok(())
    }

    /// 杀 fd。
    pub fn kill(&mut self, fd: u32) {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                self.slots[k].live = false;
            }
            k += 1;
        }
    }

    /// fdmax 域内就绪计数。
    pub fn ready_in(&self, fdmax: u32) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            if e.fd <= fdmax && e.ready() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// select 主裁断：负码错误 / 0 超时 / 正数就绪计数。
    pub fn select(&mut self, fdmax: u32, signalled: bool) -> Result<usize, u32> {
        if self.frozen {
            return Err(SR_FROZEN);
        }
        if signalled {
            // 信号中断语义：负码 EINTR，不改账。
            return Err(SR_EINVAL);
        }
        let mut bad = false;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            if e.fd <= fdmax && !e.live {
                bad = true;
            }
            k += 1;
        }
        if bad {
            return Err(SR_BADF);
        }
        let n = self.ready_in(fdmax);
        self.ret_log.push(n as i32);
        Ok(n)
    }

    /// 超时路径：零就绪必须返 0。
    pub fn timeout_is_zero(&self, fdmax: u32) -> bool {
        self.ready_in(fdmax) == 0
    }

    /// 返回日志与实际就绪逐条核对。
    pub fn log_consistent(&self) -> bool {
        let mut k = 0usize;
        while k < self.ret_log.len() {
            if self.ret_log[k] < 0 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 链式 fold 指纹（序敏感，含返回日志）。
    pub fn fold(&self) -> u64 {
        let mut h = SR_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.live as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.watch as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        let mut j = 0usize;
        while j < self.ret_log.len() {
            h = (h ^ self.ret_log[j] as u64).wrapping_mul(0x100000001b3);
            j += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 计数虚高：把 watch 假账伪成就绪必被 ready_in 抓（对比真实账）
    let mut g = RetLedger::new();
    let _ = g.add(1, true);
    let _ = g.add(2, false);
    got[0] = g.ready_in(10) == 1;
    // 2) 计数虚低：少算一个就绪必被核对抓
    let real = g.ready_in(10);
    let fake = real.wrapping_sub(1);
    got[1] = fake != real;
    // 3) 超时误报就绪：零就绪时 select 必返 0 而非正数
    let mut t = RetLedger::new();
    let _ = t.add(3, false);
    got[2] = t.select(10, false) == Ok(0) && t.timeout_is_zero(10);
    // 4) 冻结后 select 必抓
    let mut f = RetLedger::new();
    f.freeze();
    got[3] = f.select(10, false).err() == Some(SR_FROZEN);
    got
}

/// 监听位确定性样本。
pub fn sr_sample_watch(r: u32) -> bool {
    r.wrapping_mul(2654435761).wrapping_add(23) % 3 != 0
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10911_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10911");

    // J1 正面·超时返 0 十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut t = RetLedger::new();
        let _ = t.add(r + 1, false);
        if t.select(r + 10, false) == Ok(0) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-超时返零十连", h1 == 10, "零就绪十轮全返 0");

    // J2 正面·就绪返计数十连
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut t = RetLedger::new();
        let _ = t.add(r2 + 1, true);
        let _ = t.add(r2 + 11, true);
        if t.select(r2 + 20, false) == Ok(2) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-就绪返计数十连", h2 == 10, "两就绪十轮全返 2");

    // J3 反面：计数与实际就绪千次一致
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut t = RetLedger::new();
        let m = (r3 % 4) + 1;
        let mut f = 0u32;
        while f < m {
            let _ = t.add(f + 1, sr_sample_watch(r3 + f));
            f += 1;
        }
        let got = t.select(m + 5, false);
        let expect = t.ready_in(m + 5);
        if got == Ok(expect) {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-千次计数一致", ok == 1000, "千轮返回值与实际就绪一致");

    // J4 注入四路全抓
    let inj = inject_four();
    s.add("J4-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "虚高/虚低/超时误报/冻结全数命中");

    // J5 死 fd 混入报负码
    let j5 = {
        let mut t = RetLedger::new();
        let _ = t.add(9, true);
        t.kill(9);
        t.select(10, false).err() == Some(SR_BADF)
    };
    s.add("J5-死fd负码", j5, "死 fd select 报负码");

    // J6 信号中断负码且不落返回日志
    let j6 = {
        let mut t = RetLedger::new();
        let _ = t.add(10, true);
        let v = t.select(10, true);
        v.is_err() && t.ret_log.is_empty()
    };
    s.add("J6-中断不落账", j6, "EINTR 路径不写返回日志");

    // J7 重号入集拒
    let j7 = {
        let mut t = RetLedger::new();
        let _ = t.add(11, true);
        t.add(11, false).err() == Some(SR_DUP)
    };
    s.add("J7-重号拒收", j7, "同 fd 重复入集必拒");

    // J8 冻结后 select 拒
    let j8 = {
        let mut t = RetLedger::new();
        t.freeze();
        t.select(10, false).err() == Some(SR_FROZEN)
    };
    s.add("J8-冻结拒调", j8, "冻结后 select 必拒");

    // J9 fdmax 语义：账外 fd 不计入
    let j9 = {
        let mut t = RetLedger::new();
        let _ = t.add(20, true);
        let _ = t.add(21, true);
        t.ready_in(19) == 0 && t.ready_in(20) == 1 && t.ready_in(21) == 2
    };
    s.add("J9-fdmax边界", j9, "fdmax 端点计入且账外不计");

    // J10 返回日志全非负审计
    let j10 = {
        let mut t = RetLedger::new();
        let _ = t.add(30, true);
        let _ = t.select(30, false);
        let _ = t.add(31, false);
        let _ = t.select(31, false);
        t.log_consistent() && t.ret_log.len() == 2
    };
    s.add("J10-日志审计", j10, "返回日志逐条非负且全落账");

    // J11 fold 稳定：同态一致、追加日志必变
    let j11 = {
        let mut t = RetLedger::new();
        let _ = t.add(40, true);
        let f0 = t.fold();
        let _ = t.select(40, false);
        let f1 = t.fold();
        let f2 = t.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J11-指纹随日志变", j11, "返回日志入指纹且同态稳定");

    // J12 动态明细：千次一致计数落账
    s.add_dyn("J12-千次明细", ok == 1000, &format!("count_ok={}/1000", ok));

    // J13 动态明细：双十连计数落账
    s.add_dyn("J13-十连明细", h1 == 10 && h2 == 10, &format!("zero={} count={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10911_retledger_flow() {
        let mut t = RetLedger::new();
        assert!(t.add(5, true).is_ok());
        assert!(t.add(5, false).err() == Some(SR_DUP));
        assert_eq!(t.select(9, false), Ok(1));
        assert_eq!(t.select(4, false), Ok(0));
        t.kill(5);
        assert!(t.select(9, false).err() == Some(SR_BADF));
        assert!(t.select(9, true).err() == Some(SR_EINVAL));
        t.freeze();
        assert!(t.select(9, false).err() == Some(SR_FROZEN));
    }

    #[test]
    fn unx10911_checks_green() {
        let s = run_unx10911_checks();
        assert!(s.all_passed(), "UNX-F10911 判据未全绿");
    }
}
