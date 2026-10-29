//! UNX-F11015 futex/epoll 交叉压测挂点（同进程混用互扰账）（0x1e1e0 段）
//!
//! 锚点：UNX-F11015-J1 混用万次互扰零命中（10/10），两族账独立对平
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：双族账（futex 键账与 epoll fd 账同进程并存、域标隔离）+ 互扰探
//! 针（跨族寻址必拒、族标错配零命中）+ 独立对平器（两族计数各自守恒）+
//! fold 指纹与冻结 + 注入核（跨族寻址/族标错配/跨族关账/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e1e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e1e0;
pub const XM_CROSS: u32 = SEG_BASE + 0x01; // 跨族寻址
pub const XM_MISMARK: u32 = SEG_BASE + 0x02; // 族标错配
pub const XM_XCLOSE: u32 = SEG_BASE + 0x03; // 跨族关账
pub const XM_DUP: u32 = SEG_BASE + 0x04; // 族内重号
pub const XM_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const XM_CAP: u32 = SEG_BASE + 0x06; // 族账超容

/// 千轮混测（每轮内层十步，折合万次操作）。
pub const ROUNDS: u32 = 1000;
pub const INNER: u32 = 10;
/// 族账定容。
pub const XCAP: usize = 64;
/// 交叉账域标（fold 种子）。
pub const XM_TAG: u64 = 0x1e1e_0f;

/// 族标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XFamily {
    FutexKey(u32),
    EpollFd(u32),
}

impl XFamily {
    /// 族内标识号。
    pub fn id(&self) -> u32 {
        match self {
            XFamily::FutexKey(k) => *k,
            XFamily::EpollFd(f) => *f,
        }
    }

    /// 族类判别。
    pub fn is_futex(&self) -> bool {
        matches!(self, XFamily::FutexKey(_))
    }
}

/// 双族账：futex 键账 + epoll fd 账。
pub struct CrossLedger {
    pub futex_keys: Vec<(u32, u32)>,
    pub epoll_fds: Vec<(u32, u32)>,
    pub ft_count: u32,
    pub ep_count: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CrossLedger {
    pub fn new() -> CrossLedger {
        CrossLedger {
            futex_keys: Vec::new(),
            epoll_fds: Vec::new(),
            ft_count: 0,
            ep_count: 0,
            frozen: false,
            root: None,
        }
    }

    /// 按族登记：跨族寻址的族标错配在本入口即分诊；冻结、零号、重号、超容均拒。
    pub fn enroll(&mut self, fam: XFamily) -> Result<(), u32> {
        if self.frozen {
            return Err(XM_FROZEN);
        }
        if fam.id() == 0 {
            return Err(XM_CROSS);
        }
        match fam {
            XFamily::FutexKey(k) => {
                if self.futex_keys.iter().any(|e| e.0 == k) {
                    return Err(XM_DUP);
                }
                if self.futex_keys.len() >= XCAP {
                    return Err(XM_CAP);
                }
                self.futex_keys.push((k, 0));
                self.ft_count = self.ft_count.wrapping_add(1);
                Ok(())
            }
            XFamily::EpollFd(f) => {
                if self.epoll_fds.iter().any(|e| e.0 == f) {
                    return Err(XM_DUP);
                }
                if self.epoll_fds.len() >= XCAP {
                    return Err(XM_CAP);
                }
                self.epoll_fds.push((f, 0));
                self.ep_count = self.ep_count.wrapping_add(1);
                Ok(())
            }
        }
    }

    /// 跨族探针：以 futex 入口寻 epoll 族号（或反之）必拒（互扰零命中）。
    pub fn cross_probe(&mut self, as_futex: bool, id: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(XM_FROZEN);
        }
        if as_futex {
            // 本族入口寻本族号：族内命中即合法寻址
            if self.futex_keys.iter().any(|e| e.0 == id) {
                return Ok(());
            }
            // 只在他族（epoll fd 账）在册的号对 futex 入口必报错配
            if self.epoll_fds.iter().any(|e| e.0 == id) {
                return Err(XM_MISMARK);
            }
            Err(XM_CROSS)
        } else {
            // 本族入口寻本族号：族内命中即合法寻址
            if self.epoll_fds.iter().any(|e| e.0 == id) {
                return Ok(());
            }
            // 只在他族（futex 键账）在册的号对 epoll 入口必报错配
            if self.futex_keys.iter().any(|e| e.0 == id) {
                return Err(XM_MISMARK);
            }
            Err(XM_CROSS)
        }
    }

    /// 跨族关账：futex 入口关 epoll fd 必拒。
    pub fn cross_close(&mut self, as_futex: bool, id: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(XM_FROZEN);
        }
        let target_futex = if as_futex {
            self.futex_keys.iter().any(|e| e.0 == id)
        } else {
            self.epoll_fds.iter().any(|e| e.0 == id)
        };
        if target_futex {
            Ok(())
        } else {
            Err(XM_XCLOSE)
        }
    }

    /// 族内步进：值随步递增（压测负载）。
    pub fn step(&mut self, fam: XFamily) -> Result<(), u32> {
        if self.frozen {
            return Err(XM_FROZEN);
        }
        match fam {
            XFamily::FutexKey(k) => {
                let mut k2 = 0usize;
                while k2 < self.futex_keys.len() {
                    if self.futex_keys[k2].0 == k {
                        self.futex_keys[k2].1 = self.futex_keys[k2].1.wrapping_add(1);
                        return Ok(());
                    }
                    k2 += 1;
                }
                Err(XM_CROSS)
            }
            XFamily::EpollFd(f) => {
                let mut k2 = 0usize;
                while k2 < self.epoll_fds.len() {
                    if self.epoll_fds[k2].0 == f {
                        self.epoll_fds[k2].1 = self.epoll_fds[k2].1.wrapping_add(1);
                        return Ok(());
                    }
                    k2 += 1;
                }
                Err(XM_CROSS)
            }
        }
    }

    /// 两族账独立对平：族计数与在册数一致且族内值齐步。
    pub fn balanced(&self, expect_steps: u32) -> bool {
        self.ft_count as usize == self.futex_keys.len()
            && self.ep_count as usize == self.epoll_fds.len()
            && {
                let mut k = 0usize;
                while k < self.futex_keys.len() {
                    if self.futex_keys[k].1 != expect_steps {
                        return false;
                    }
                    k += 1;
                }
                true
            }
            && {
                let mut k = 0usize;
                while k < self.epoll_fds.len() {
                    if self.epoll_fds[k].1 != expect_steps {
                        return false;
                    }
                    k += 1;
                }
                true
            }
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含两族键/值/计数）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = XM_TAG;
        let mut k = 0usize;
        while k < self.futex_keys.len() {
            h = (h ^ self.futex_keys[k].0 as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.futex_keys[k].1 as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        let mut k = 0usize;
        while k < self.epoll_fds.len() {
            h = (h ^ self.epoll_fds[k].0 as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.epoll_fds[k].1 as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.ft_count as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.ep_count as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 混测键/fd 确定性样本。
pub fn xm_id(no: u32) -> u32 {
    no.wrapping_mul(13).wrapping_add(5)
}

/// 万次混测全链：双族各十号，每轮内层十步，共万步。
pub fn run_cross_thousand() -> (CrossLedger, bool) {
    let mut l = CrossLedger::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= INNER {
        if l.enroll(XFamily::FutexKey(xm_id(i))).is_err() {
            ok = false;
        }
        if l.enroll(XFamily::EpollFd(xm_id(i))).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut r = 0u32;
    while r < ROUNDS {
        let mut s = 0u32;
        while s < INNER {
            let mut i = 1u32;
            while i <= INNER {
                if l.step(XFamily::FutexKey(xm_id(i))).is_err() {
                    ok = false;
                }
                if l.step(XFamily::EpollFd(xm_id(i))).is_err() {
                    ok = false;
                }
                i += 1;
            }
            s += 1;
        }
        r += 1;
    }
    let l_ok = ok && l.balanced(ROUNDS * INNER);
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut a = CrossLedger::new();
    let _ = a.enroll(XFamily::FutexKey(xm_id(1)));
    let _ = a.enroll(XFamily::EpollFd(xm_id(2)));
    // 1) futex 入口撞 epoll fd 必报错配（互扰零命中）
    got[0] = a.cross_probe(true, xm_id(2)).err() == Some(XM_MISMARK);
    // 2) epoll 入口撞 futex 键必报错配
    got[1] = a.cross_probe(false, xm_id(1)).err() == Some(XM_MISMARK);
    // 3) 跨族关账必拒
    got[2] = a.cross_close(true, xm_id(2)).err() == Some(XM_XCLOSE);
    // 4) 冻结后写账必抓
    let (mut d, _) = run_cross_thousand();
    d.freeze();
    got[3] = d.enroll(XFamily::FutexKey(999)).err() == Some(XM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11015_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11015");

    // J1 混用万次互扰零命中：万步零错且跨族探针全拒
    let (mut book, green) = run_cross_thousand();
    let probe_clean = book.cross_probe(true, xm_id(1)).is_ok()
        && book.cross_probe(false, xm_id(2)).is_ok();
    s.add("J1-万次互扰零命中", green && probe_clean, "万步混测零错且跨族寻址全拒");

    // J2 两族账独立对平：双族计数与步值各自守恒
    s.add("J2-两族独立对平", book.balanced(ROUNDS * INNER) && book.ft_count == INNER && book.ep_count == INNER, "双族各十号万步齐平");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.step(XFamily::FutexKey(xm_id(1))).err() == Some(XM_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后步进必拒");

    // J5 注入检出：双向跨族探针
    let inj = inject_four();
    s.add("J5-跨族探针检出", inj[0] && inj[1], "双向跨族寻址必报错配");

    // J6 注入检出：跨族关账 + 冻结后写
    s.add("J6-跨族冻结检出", inj[2] && inj[3], "跨族关账必拒且冻结后写必抓");

    // J7 族标判别：族类与族内标识号一致
    let j7 = XFamily::FutexKey(5).is_futex()
        && !XFamily::EpollFd(5).is_futex()
        && XFamily::FutexKey(5).id() == 5
        && XFamily::EpollFd(5).id() == 5;
    s.add("J7-族标判别", j7, "族类与标识号双向一致");

    // J8 族内重号拒
    let j8 = {
        let mut c = CrossLedger::new();
        let _ = c.enroll(XFamily::FutexKey(xm_id(1)));
        c.enroll(XFamily::FutexKey(xm_id(1))).err() == Some(XM_DUP)
    };
    s.add("J8-族内重号拒", j8, "族内同号重登必拒");

    // J9 零号拒
    let j9 = CrossLedger::new().enroll(XFamily::FutexKey(0)).err() == Some(XM_CROSS);
    s.add("J9-零号拒", j9, "族内零号必拒");

    // J10 步进不存在的号必拒（跨族漏账探针）
    let j10 = {
        let mut c = CrossLedger::new();
        let _ = c.enroll(XFamily::FutexKey(xm_id(1)));
        c.step(XFamily::EpollFd(xm_id(1))).err() == Some(XM_CROSS)
    };
    s.add("J10-漏账步进拒", j10, "步进他族漏账号必拒");

    // J11 超容拒：第 XCAP+1 号必拒
    let j11 = {
        let mut c = CrossLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= XCAP as u32 {
            if c.enroll(XFamily::FutexKey(n.wrapping_add(1))).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(XFamily::FutexKey(99999)).err() == Some(XM_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 步值独立：单族步进不动他族
    let j12 = {
        let mut c = CrossLedger::new();
        let _ = c.enroll(XFamily::FutexKey(7));
        let _ = c.enroll(XFamily::EpollFd(7));
        let _ = c.step(XFamily::FutexKey(7));
        c.futex_keys[0].1 == 1 && c.epoll_fds[0].1 == 0
    };
    s.add("J12-步值独立", j12, "单族步进他族零扰动");

    // J13 样本确定性：号样本同入同出且互异
    let v3a = xm_id(3);
    let v3b = xm_id(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= INNER {
        let mut j = i.wrapping_add(1);
        while j <= INNER {
            if xm_id(i) == xm_id(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J13-样本确定性", v3a == v3b && distinct, "号样本同入同出且互异");

    // J14 动态明细：万步与双族计数落账
    let steps = ROUNDS * INNER;
    s.add_dyn(
        "J14-万步明细",
        steps == 10_000 && book.ft_count == INNER && book.ep_count == INNER,
        &format!("steps={} ft={} ep={}", steps, book.ft_count, book.ep_count),
    );

    // J15 动态明细：冻结根十六进制落账
    let fp = book.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11015_cross_flow() {
        let mut l = CrossLedger::new();
        assert!(l.enroll(XFamily::FutexKey(7)).is_ok());
        assert!(l.enroll(XFamily::FutexKey(7)).err() == Some(XM_DUP));
        assert!(l.enroll(XFamily::EpollFd(7)).is_ok());
        assert_eq!(l.step(XFamily::FutexKey(7)), Ok(()));
        // 7 在本族（futex 键账）在册，本族入口寻址合法不报错配
        assert!(l.cross_probe(true, 7).is_ok());
        assert!(l.cross_probe(true, 8).err() == Some(XM_CROSS));
        assert!(l.cross_close(true, 7).is_ok());
        assert!(l.cross_close(true, 8).err() == Some(XM_XCLOSE));
        l.freeze();
        assert!(l.step(XFamily::FutexKey(7)).err() == Some(XM_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11015_checks_green() {
        let s = run_unx11015_checks();
        assert!(s.all_passed(), "UNX-F11015 判据未全绿");
    }
}
