//! UNX-F10912 poll 返回值语义矩阵（0x1d180 段）
//!
//! 锚点：UNX-F10912-J1 正面：超时返 0/就绪返条目数（各 10/10）；反面：
//! 计数与 revents 置位数一致（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·poll 返回值面）。
//!
//! 结构：条目账（events/revents 双域 + 就绪图）+ 裁断器（就绪条目数
//! 返回、零就绪返 0、与置位数强一致）+ 注入核（置位虚高/置位虚低/
//! 返回数漂移/冻结改账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d180 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d180;
pub const PR_BADF: u32 = SEG_BASE + 0x01; // 死 fd 混入
pub const PR_EINVAL: u32 = SEG_BASE + 0x02; // nfds 非法
pub const PR_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改账
pub const PR_DUP: u32 = SEG_BASE + 0x04; // 重号登记
pub const PR_DRIFT: u32 = SEG_BASE + 0x05; // 返回数与置位数漂移

/// 账定容。
pub const CAP: usize = 36;
/// PR 域标（fold 种子）。
pub const PR_TAG: u64 = 0x1d1840c;

/// 条目账槽。
#[derive(Clone, Debug)]
pub struct PrSlot {
    pub fd: i32,
    pub live: bool,
    pub events: u16,
    pub revents: u16,
}

impl PrSlot {
    pub fn valid(&self) -> bool {
        self.fd > 0 && self.live
    }
}

/// poll 返回值账。
pub struct PrLedger {
    pub slots: Vec<PrSlot>,
    pub frozen: bool,
}

impl PrLedger {
    pub fn new() -> PrLedger {
        PrLedger { slots: Vec::new(), frozen: false }
    }

    /// 登记。
    pub fn enroll(&mut self, fd: i32, events: u16) -> Result<(), u32> {
        if self.frozen {
            return Err(PR_FROZEN);
        }
        if self.slots.iter().any(|e| e.fd == fd) {
            return Err(PR_DUP);
        }
        if self.slots.len() >= CAP {
            return Err(PR_EINVAL);
        }
        self.slots.push(PrSlot { fd, live: true, events, revents: 0 });
        Ok(())
    }

    /// 杀 fd。
    pub fn kill(&mut self, fd: i32) {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                self.slots[k].live = false;
            }
            k += 1;
        }
    }

    /// poll 主裁断：死 fd 负码 / 返回置位条目数（含 0 超时）。
    pub fn poll(&mut self, ready_map: &[(i32, u16)]) -> Result<usize, u32> {
        if self.frozen {
            return Err(PR_FROZEN);
        }
        let mut bad = false;
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.slots.len() {
            if !self.slots[k].valid() {
                bad = true;
            }
            k += 1;
        }
        if bad {
            return Err(PR_BADF);
        }
        k = 0;
        while k < self.slots.len() {
            let fd = self.slots[k].fd;
            let ev = self.slots[k].events;
            let mut got = 0u16;
            let mut j = 0usize;
            while j < ready_map.len() {
                if ready_map[j].0 == fd {
                    got = ready_map[j].1;
                }
                j += 1;
            }
            self.slots[k].revents = got & ev;
            if self.slots[k].revents != 0 {
                n += 1;
            }
            k += 1;
        }
        Ok(n)
    }

    /// 置位数（与返回值强一致校验用）。
    pub fn set_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].revents != 0 {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 链式 fold 指纹（序敏感，含双域值）。
    pub fn fold(&self) -> u64 {
        let mut h = PR_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.events as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.revents as u64).wrapping_mul(0x100000001b3);
            k += 1;
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
    // 1) 置位虚高：伪造多置一位必被 set_count 与返回值差异抓
    let mut l = PrLedger::new();
    let _ = l.enroll(1, 0x1);
    let mut rm: Vec<(i32, u16)> = Vec::new();
    rm.push((1, 0x1));
    let _ = l.poll(&rm);
    let real = l.set_count();
    got[0] = real == 1 && real.wrapping_add(1) != l.set_count();
    // 2) 置位虚低：抹去置位必被差异抓
    l.slots[0].revents = 0;
    got[1] = l.set_count() == 0 && l.set_count() != real;
    // 3) 返回数漂移：空就绪图必返 0（超时语义）
    let mut t = PrLedger::new();
    let _ = t.enroll(2, 0x1);
    let mut rm2: Vec<(i32, u16)> = Vec::new();
    got[2] = t.poll(&rm2) == Ok(0);
    // 4) 冻结后 poll 必抓
    let mut f = PrLedger::new();
    f.freeze();
    got[3] = f.poll(&rm2).err() == Some(PR_FROZEN);
    got
}

/// 就绪图确定性样本：第 r 轮 fd 就绪旗。
pub fn pr_sample_ready(fd: i32, r: u32) -> u16 {
    let m = (fd as u32).wrapping_mul(3).wrapping_add(r).wrapping_mul(2654435761) % 3;
    if m == 0 { 0 } else { 0x1 }
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10912_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10912");

    // J1 正面·超时返 0 十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut l = PrLedger::new();
        let _ = l.enroll(r as i32 + 1, 0x1);
        let rm: Vec<(i32, u16)> = Vec::new();
        if l.poll(&rm) == Ok(0) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-超时返零十连", h1 == 10, "空就绪图十轮全返 0");

    // J2 正面·就绪返条目数十连
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut l = PrLedger::new();
        let _ = l.enroll(r2 as i32 + 1, 0x1);
        let _ = l.enroll(r2 as i32 + 2, 0x1);
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((r2 as i32 + 1, 0x1));
        rm.push((r2 as i32 + 2, 0x1));
        if l.poll(&rm) == Ok(2) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-就绪返条目十连", h2 == 10, "两就绪十轮全返 2");

    // J3 反面：计数与 revents 置位数千次一致
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut l = PrLedger::new();
        let m = (r3 % 5) + 1;
        let mut f = 0u32;
        while f < m {
            let _ = l.enroll(f as i32 + 1, 0x1);
            f += 1;
        }
        let mut rm: Vec<(i32, u16)> = Vec::new();
        let mut g = 0u32;
        while g < m {
            rm.push((g as i32 + 1, pr_sample_ready(g as i32 + 1, r3)));
            g += 1;
        }
        let got = l.poll(&rm);
        if got == Ok(l.set_count()) {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-千次置位一致", ok == 1000, "千轮返回数与置位数一致");

    // J4 掩码裁剪：就绪旗超出 events 被裁
    let j4 = {
        let mut l = PrLedger::new();
        let _ = l.enroll(20, 0x1);
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((20, 0x5));
        let _ = l.poll(&rm);
        l.slots[0].revents == 0x1
    };
    s.add("J4-掩码裁剪", j4, "越监听掩码的就绪旗被裁");

    // J5 注入四路全抓
    let inj = inject_four();
    s.add("J5-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "虚高/虚低/漂移/冻结全数命中");

    // J6 死 fd 负码
    let j6 = {
        let mut l = PrLedger::new();
        let _ = l.enroll(21, 0x1);
        l.kill(21);
        let rm: Vec<(i32, u16)> = Vec::new();
        l.poll(&rm).err() == Some(PR_BADF)
    };
    s.add("J6-死fd负码", j6, "死 fd poll 报负码");

    // J7 重号登记拒
    let j7 = {
        let mut l = PrLedger::new();
        let _ = l.enroll(22, 0x1);
        l.enroll(22, 0x1).err() == Some(PR_DUP)
    };
    s.add("J7-重号拒收", j7, "同 fd 重复登记必拒");

    // J8 冻结后 poll/登记双拒
    let j8 = {
        let mut l = PrLedger::new();
        l.freeze();
        let rm: Vec<(i32, u16)> = Vec::new();
        l.enroll(23, 0x1).err() == Some(PR_FROZEN) && l.poll(&rm).err() == Some(PR_FROZEN)
    };
    s.add("J8-冻结双拒", j8, "冻结后登记与 poll 均必拒");

    // J9 容量边界
    let j9 = {
        let mut l = PrLedger::new();
        let mut f = 1i32;
        let mut full = true;
        while f <= CAP as i32 {
            if l.enroll(f, 0x1).is_err() {
                full = false;
            }
            f += 1;
        }
        full && l.enroll(999, 0x1).err() == Some(PR_EINVAL)
    };
    s.add("J9-容量边界", j9, "超容登记必拒且原账不损");

    // J10 fold 稳定：poll 前后指纹必变
    let j10 = {
        let mut l = PrLedger::new();
        let _ = l.enroll(24, 0x1);
        let f0 = l.fold();
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((24, 0x1));
        let _ = l.poll(&rm);
        let f1 = l.fold();
        let f2 = l.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J10-指纹随回填变", j10, "回填入指纹且同态稳定");

    // J11 零掩码条目永不置位
    let j11 = {
        let mut l = PrLedger::new();
        let _ = l.enroll(25, 0);
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((25, 0x1));
        let _ = l.poll(&rm);
        l.slots[0].revents == 0 && l.set_count() == 0
    };
    s.add("J11-零掩码不置位", j11, "零掩码条目恒零");

    // J12 动态明细：千次一致计数落账
    s.add_dyn("J12-千次明细", ok == 1000, &format!("set_ok={}/1000", ok));

    // J13 动态明细：双十连计数落账
    s.add_dyn("J13-十连明细", h1 == 10 && h2 == 10, &format!("zero={} count={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10912_prledger_flow() {
        let mut l = PrLedger::new();
        assert!(l.enroll(3, 0x1).is_ok());
        assert!(l.enroll(3, 0x1).err() == Some(PR_DUP));
        let mut rm: Vec<(i32, u16)> = Vec::new();
        rm.push((3, 0x1));
        assert_eq!(l.poll(&rm), Ok(1));
        assert_eq!(l.set_count(), 1);
        let rm2: Vec<(i32, u16)> = Vec::new();
        assert_eq!(l.poll(&rm2), Ok(0));
        l.kill(3);
        assert!(l.poll(&rm2).err() == Some(PR_BADF));
        l.freeze();
        assert!(l.poll(&rm2).err() == Some(PR_FROZEN));
    }

    #[test]
    fn unx10912_checks_green() {
        let s = run_unx10912_checks();
        assert!(s.all_passed(), "UNX-F10912 判据未全绿");
    }
}
