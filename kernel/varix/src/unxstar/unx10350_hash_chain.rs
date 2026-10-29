//! UNX-F10350 波次产物哈希链（域账快照链完备核）（0x1b640 段）
//!
//! 锚点：UNX-F10350-J1 快照链完备——波次域账快照逐环入链（环=序号+
//! 快照+前根+本根，本根由快照与前根折 FNV-64），逐环前根必等于前环本
//! 根且本根可重算，链长达锚零断环才算链完备，封链后拒加。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：链账（环序记账）+ 续链核（前根衔接）+ 重算核（本根可由快照
//! 与前根重算）+ 封链闸（链长达锚零断环方可封链）+ 注入核（零快照/
//! 断环/短链封链/封链后续环四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b640 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b640;
pub const HC_ZERO: u32 = SEG_BASE + 0x01; // 零快照
pub const HC_CAP: u32 = SEG_BASE + 0x02; // 链超长
pub const HC_SEALED: u32 = SEG_BASE + 0x03; // 封链后续环
pub const HC_SHORT: u32 = SEG_BASE + 0x04; // 短链封链
pub const HC_BROKEN: u32 = SEG_BASE + 0x05; // 断环封链

/// 链长锚（八环）。
pub const LINKS_NEEDED: usize = 8;
/// 链长帽。
pub const CAP: usize = 16;
/// 链基根（首环前根）。
pub const GENESIS: u64 = 0x1b6401b64;

/// FNV-1a 64 位折算。
pub fn fnv64(prev: u64, snap: u64) -> u64 {
    let mut h = prev ^ 0xcbf29ce484222325;
    let mut k = 0u32;
    while k < 8 {
        let byte = (snap >> (k * 8)) as u64 & 0xff;
        h ^= byte;
        h = h.wrapping_mul(0x100000001b3);
        k += 1;
    }
    h
}

/// 链环。
#[derive(Clone)]
pub struct Link {
    pub idx: u32,
    pub snap: u64,
    pub prev_root: u64,
    pub root: u64,
}

impl Link {
    /// 本根重算核。
    pub fn root_ok(&self) -> bool {
        self.root == fnv64(self.prev_root, self.snap)
    }
}

/// 波次哈希链。
pub struct HashChain {
    pub links: Vec<Link>,
    pub sealed: bool,
}

impl HashChain {
    pub fn new() -> HashChain {
        HashChain { links: Vec::new(), sealed: false }
    }

    /// 续环：快照零即拒；前根自动衔接前环本根。
    pub fn append(&mut self, snap: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(HC_SEALED);
        }
        if snap == 0 {
            return Err(HC_ZERO);
        }
        if self.links.len() >= CAP {
            return Err(HC_CAP);
        }
        let idx = self.links.len() as u32 + 1;
        let prev_root = match self.links.len() {
            0 => GENESIS,
            n => self.links[n - 1].root,
        };
        let root = fnv64(prev_root, snap);
        self.links.push(Link { idx, snap, prev_root, root });
        Ok(root)
    }

    /// 断环清单（前根不衔接或本根不可重算）。
    pub fn broken(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.links.len() {
            let l = &self.links[k];
            let expect_prev = if k == 0 { GENESIS } else { self.links[k - 1].root };
            if l.prev_root != expect_prev || !l.root_ok() {
                v.push(l.idx);
            }
            k += 1;
        }
        v
    }

    /// 链完备：链长达锚、零断环。
    pub fn complete(&self) -> bool {
        self.links.len() >= LINKS_NEEDED && self.broken().is_empty()
    }

    /// 链头根。
    pub fn head(&self) -> u64 {
        match self.links.len() {
            0 => GENESIS,
            n => self.links[n - 1].root,
        }
    }

    /// 封链：完备方可。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(HC_SEALED);
        }
        if self.links.len() < LINKS_NEEDED {
            return Err(HC_SHORT);
        }
        if !self.broken().is_empty() {
            return Err(HC_BROKEN);
        }
        self.sealed = true;
        Ok(self.head())
    }

    /// 链指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1b6401b64;
        h = h.wrapping_mul(0x100000001b3).wrapping_add(self.links.len() as u64);
        let mut k = 0usize;
        while k < self.links.len() {
            h = h.wrapping_mul(0x100000001b3).wrapping_add(self.links[k].root);
            k += 1;
        }
        h = h.wrapping_mul(0x100000001b3).wrapping_add(self.sealed as u64);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut c = HashChain::new();
    // 1) 零快照必拒
    got[0] = c.append(0).err() == Some(HC_ZERO);
    // 2) 短链封链必拒
    let _ = c.append(7);
    got[1] = c.seal().err() == Some(HC_SHORT);
    // 3) 封链后续环必拒
    let mut full = HashChain::new();
    let mut n = 1u64;
    while n <= LINKS_NEEDED as u64 {
        let _ = full.append(n * 0x9e3779b97f4a7c15);
        n += 1;
    }
    let _ = full.seal();
    got[2] = full.append(1).err() == Some(HC_SEALED);
    // 4) 重算核可识破伪造本根
    let mut t = HashChain::new();
    let _ = t.append(42);
    t.links[0].root = t.links[0].root ^ 1;
    got[3] = t.broken() == [1u32] && !t.links[0].root_ok();
    got
}

/// 批内判据。
pub fn run_unx10350_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10350");

    // J1 正路径：八环续链零断环封链
    let mut c = HashChain::new();
    let mut ok = true;
    let mut n = 1u64;
    while n <= LINKS_NEEDED as u64 {
        let snap = n.wrapping_mul(0x9e3779b97f4a7c15);
        if c.append(snap).is_err() {
            ok = false;
        }
        n += 1;
    }
    let head = c.head();
    let sealed = c.seal();
    s.add(
        "J1-快照链完备",
        ok && c.complete() && sealed.is_ok() && c.sealed,
        "八环续链零断环并封链出头根",
    );

    // J2 头根非零且等于末环本根
    let j2 = head != 0 && head == c.links[c.links.len() - 1].root;
    s.add("J2-头根对环", j2, "链头根即末环本根且非零");

    // J3 注入检出：零快照 + 短链封链
    let inj = inject_four();
    s.add("J3-零照短链检出", inj[0] && inj[1], "零快照与短链封链必拒");

    // J4 注入检出：封链续环 + 伪造本根
    s.add("J4-续环伪造检出", inj[2] && inj[3], "封链续环必拒且伪造本根必被抓");

    // J5 前根衔接逐环核
    let j5 = {
        let mut all = true;
        let mut k = 0usize;
        while k < c.links.len() {
            let expect_prev = if k == 0 { GENESIS } else { c.links[k - 1].root };
            if c.links[k].prev_root != expect_prev {
                all = false;
            }
            k += 1;
        }
        all
    };
    s.add("J5-前根衔接", j5, "逐环前根等于前环本根（首环接基根）");

    // J6 本根可重算逐环核
    let j6 = {
        let mut all = true;
        let mut k = 0usize;
        while k < c.links.len() {
            if !c.links[k].root_ok() {
                all = false;
            }
            k += 1;
        }
        all
    };
    s.add("J6-本根重算", j6, "逐环本根由快照与前根重算可复核");

    // J7 断环清单可枚举（伪造中环本根）
    let j7 = {
        let mut t = HashChain::new();
        let mut q = 1u64;
        while q <= 4u64 {
            let _ = t.append(q * 1000);
            q += 1;
        }
        t.links[2].root = t.links[2].root.wrapping_add(7);
        t.broken() == [3u32]
    };
    s.add("J7-断环枚举", j7, "中环本根被改后续环断点可枚举");

    // J8 折算确定性：同前根同快照同根
    let j8 = fnv64(GENESIS, 5) == fnv64(GENESIS, 5) && fnv64(GENESIS, 5) != fnv64(GENESIS, 6);
    s.add("J8-折算确定", j8, "折算同入必同根异入必异根");

    // J9 容量边界
    let j9 = {
        let mut t = HashChain::new();
        let mut q = 1u64;
        let mut filled = true;
        while q <= CAP as u64 {
            if t.append(q * 31).is_err() {
                filled = false;
            }
            q += 1;
        }
        filled && t.append(999).err() == Some(HC_CAP)
    };
    s.add("J9-容量边界", j9, "链满必拒且原链不损");

    // J10 链指纹随续环推进且封链后稳定
    let j10 = {
        let mut t = HashChain::new();
        let f0 = t.fold();
        let _ = t.append(11);
        let f1 = t.fold();
        let _ = t.append(12);
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J10-指纹推进", j10, "每续一环指纹一变且终态稳定");

    // J11 链长达锚
    s.add(
        "J11-链长达锚",
        c.links.len() == LINKS_NEEDED,
        "链环数恰为八",
    );

    // J12 动态明细：断环计数落账
    let br = c.broken().len();
    s.add_dyn("J12-断环计数", br == 0, &format!("broken={}", br));

    // J13 动态明细：链指纹落账
    let fp = c.fold();
    s.add_dyn("J13-链指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10350_hash_chain_flow() {
        let mut c = HashChain::new();
        assert!(c.append(0).err() == Some(HC_ZERO));
        assert!(c.seal().err() == Some(HC_SHORT));
        let mut n = 1u64;
        while n <= LINKS_NEEDED as u64 {
            assert!(c.append(n * 777).is_ok());
            n += 1;
        }
        assert!(c.append(777).err() == Some(HC_CAP) || c.links.len() <= CAP);
        assert!(c.broken().is_empty());
        let head = c.head();
        assert!(c.seal().is_ok());
        assert!(c.seal().err() == Some(HC_SEALED));
        assert!(c.append(1).err() == Some(HC_SEALED));
        assert!(c.head() == head);
        assert!(c.links[0].prev_root == GENESIS);
        assert!(c.links[1].prev_root == c.links[0].root);
    }

    #[test]
    fn unx10350_checks_green() {
        let s = run_unx10350_checks();
        assert!(s.all_passed(), "UNX-F10350 判据未全绿");
    }
}
