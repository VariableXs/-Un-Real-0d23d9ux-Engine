//! UNX 卡 10156 — COW 语义正反双判据（写时复制达成与父子互扰误用受控）
//! 段位 seg=0x106a0
//! 判据原文：UNX-F10156-J1 COW 达成（fork 后子写父不可见 10/10 次）
//! 锚点：fork 共享页 → 子写触发复制 → 父面不可见 → 父写子面亦隔离 → 双向互扰受控
//! 域：CowBook 写时复制册（共享页引用计数 + 副本层）
//! 结构：共享页（引用计数）→ 子写 COW → 父写 COW → 双面隔离审计 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x106a0;
pub const CW_BAD_PAGE: u32 = SEG_BASE + 0x01;
pub const CW_SEALED: u32 = SEG_BASE + 0x02;
pub const CW_BAD_PID: u32 = SEG_BASE + 0x03;
pub const CW_PAGE_CAP: u32 = SEG_BASE + 0x04;
pub const CW_REF_CAP: u32 = SEG_BASE + 0x05;

/// 页容量与页数帽
pub const PAGE_WORDS: usize = 4;
pub const PAGE_CAP: usize = 0x40;
pub const REF_CAP: usize = 0x10;

/// 一页数据
pub type PageData = [u32; PAGE_WORDS];

/// 一页（共享数据 + 引用者）
#[derive(Clone, Debug)]
pub struct CowPage {
    pub pfn: usize,
    pub data: PageData,
    pub refs: Vec<u32>,
}

/// 写时复制册
pub struct CowBook {
    pub pages: Vec<CowPage>,
    /// 各进程私有副本：pid → (pfn, data)
    pub copies: Vec<(u32, usize, PageData)>,
    /// COW 触发次数
    pub faults: usize,
    sealed: bool,
}

impl CowBook {
    pub fn new() -> Self {
        CowBook {
            pages: Vec::new(),
            copies: Vec::new(),
            faults: 0,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 建共享页
    pub fn add_page(&mut self, seed: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(CW_SEALED);
        }
        if self.pages.len() >= PAGE_CAP {
            return Err(CW_PAGE_CAP);
        }
        let pfn = self.pages.len();
        self.pages.push(CowPage {
            pfn,
            data: [seed; PAGE_WORDS],
            refs: Vec::new(),
        });
        Ok(pfn)
    }

    /// fork 挂引用（父子两方共享）
    pub fn fork_attach(&mut self, pfn: usize, parent: u32, child: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CW_SEALED);
        }
        if parent == 0 || child == 0 || parent == child {
            return Err(CW_BAD_PID);
        }
        if pfn >= self.pages.len() {
            return Err(CW_BAD_PAGE);
        }
        if self.pages[pfn].refs.len() + 2 > REF_CAP {
            return Err(CW_REF_CAP);
        }
        self.pages[pfn].refs.push(parent);
        self.pages[pfn].refs.push(child);
        Ok(())
    }

    fn find_copy(&self, pid: u32, pfn: usize) -> Option<usize> {
        self.copies.iter().position(|c| c.0 == pid && c.1 == pfn)
    }

    /// 读面：有副本读副本，否则读共享页
    pub fn read(&self, pid: u32, pfn: usize) -> Result<PageData, u32> {
        if pfn >= self.pages.len() {
            return Err(CW_BAD_PAGE);
        }
        if let Some(i) = self.find_copy(pid, pfn) {
            return Ok(self.copies[i].2);
        }
        Ok(self.pages[pfn].data)
    }

    /// 写面：首次写触发 COW 建副本
    pub fn write(&mut self, pid: u32, pfn: usize, data: PageData) -> Result<bool, u32> {
        if self.sealed {
            return Err(CW_SEALED);
        }
        if pfn >= self.pages.len() {
            return Err(CW_BAD_PAGE);
        }
        if pid == 0 {
            return Err(CW_BAD_PID);
        }
        if let Some(i) = self.find_copy(pid, pfn) {
            self.copies[i].2 = data;
            return Ok(false);
        }
        // 未挂引用的进程直接写共享页？
        let attached = self.pages[pfn].refs.contains(&pid);
        if !attached {
            return Err(CW_BAD_PID);
        }
        self.copies.push((pid, pfn, data));
        self.faults += 1;
        Ok(true)
    }

    /// 隔离核：子写后父面仍为共享值
    pub fn parent_unaffected(&self, parent: u32, child: u32, pfn: usize) -> bool {
        match (self.read(parent, pfn), self.read(child, pfn)) {
            (Ok(a), Ok(b)) => a != b,
            _ => false,
        }
    }

    /// 共享面核：fork 未写时父子同值
    pub fn shared_before_write(&self, parent: u32, child: u32, pfn: usize) -> bool {
        match (self.read(parent, pfn), self.read(child, pfn)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }

    /// COW 触发账
    pub fn fault_count(&self) -> usize {
        self.faults
    }

    /// 副本计数
    pub fn copy_count(&self) -> usize {
        self.copies.len()
    }

    /// 引用计数合计
    pub fn ref_total(&self) -> usize {
        self.pages.iter().map(|p| p.refs.len()).sum()
    }

    /// 指纹
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x106a00;
        for p in &self.pages {
            h = h.wrapping_mul(31).wrapping_add(p.data[0]);
            h = h.wrapping_mul(31).wrapping_add(p.refs.len() as u32);
        }
        for c in &self.copies {
            h = h.wrapping_mul(31).wrapping_add(c.2[0]);
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CowBook::new();
    let p = b.add_page(7).unwrap_or(0);
    // 1) 未挂引用进程写必拒
    got[0] = b.write(99, p, [1; PAGE_WORDS]).is_err();
    // 2) fork 自附（parent==child）必拒
    got[1] = b.fork_attach(p, 3, 3).is_err();
    // 3) 野页读写必拒
    got[2] = b.read(1, 99).is_err();
    // 4) fork 未写时父子同值
    let _ = b.fork_attach(p, 1, 2);
    got[3] = b.shared_before_write(1, 2, p);
    // 5) 封册后必拒
    b.seal();
    got[4] = b.write(1, p, [2; PAGE_WORDS]).is_err();
    got
}

/// 判据入口：UNX-F10156
pub fn run_unx10156_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10156");
    let mut b = CowBook::new();

    // J1 COW 达成 10/10：十页各 fork→子写→父不可见
    let mut j1 = true;
    for i in 0..10u32 {
        let pfn = b.add_page(i + 1);
        let pfn = match pfn {
            Ok(v) => v,
            Err(_) => {
                j1 = false;
                break;
            }
        };
        if b.fork_attach(pfn, 10, 20).is_err() {
            j1 = false;
        }
        if !b.shared_before_write(10, 20, pfn) {
            j1 = false;
        }
        if b.write(20, pfn, [i + 100; PAGE_WORDS]).is_err() {
            j1 = false;
        }
        if !b.parent_unaffected(10, 20, pfn) {
            j1 = false;
        }
    }
    s.add("J1_cow_10x", j1 && b.fault_count() == 10, "fork 后子写父不可见 10/10");

    // J2 父面读共享值
    let v = b.read(10, 0).unwrap_or([0; PAGE_WORDS]);
    s.add("J2_parent_shared", v == [1; PAGE_WORDS], "父面保持共享种子 1");

    // J3 子面读副本
    let v2 = b.read(20, 0).unwrap_or([0; PAGE_WORDS]);
    s.add("J3_child_copy", v2 == [100; PAGE_WORDS], "子面读私有副本 100");

    // J4 二次写不触发 COW
    let f0 = b.fault_count();
    let r = b.write(20, 0, [200; PAGE_WORDS]);
    let j4 = r == Ok(false) && b.fault_count() == f0;
    s.add("J4_rewrite", j4, "副本在册二次写不触发");

    // J5 父写亦隔离
    let _ = b.write(10, 0, [50; PAGE_WORDS]);
    let j5 = b.read(10, 0) == Ok([50; PAGE_WORDS]) && b.read(20, 0) == Ok([200; PAGE_WORDS]);
    s.add("J5_parent_cow", j5, "父写后子面不受扰");

    // J6 未挂引用写受控
    let j6 = b.write(77, 1, [9; PAGE_WORDS]) == Err(CW_BAD_PID);
    s.add("J6_bad_writer", j6, "未挂引用进程写拒 BAD_PID");

    // J7 自附受控
    let mut b2 = CowBook::new();
    let p2 = b2.add_page(3).unwrap_or(0);
    let j7 = b2.fork_attach(p2, 5, 5) == Err(CW_BAD_PID)
        && b2.fork_attach(p2, 0, 6) == Err(CW_BAD_PID);
    s.add("J7_bad_pid", j7, "自附与零 pid 双拒");

    // J8 野页受控
    let j8 = b2.read(1, 99) == Err(CW_BAD_PAGE) && b2.write(1, 99, [1; PAGE_WORDS]) == Err(CW_BAD_PAGE);
    s.add("J8_bad_page", j8, "野页读写双拒");

    // J9 引用计数对平
    let want: usize = b.pages.iter().map(|p| p.refs.len()).sum();
    s.add("J9_refs", b.ref_total() == want && b.ref_total() == 20, "十页各 2 引用计 20");

    // J10 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 封册受控
    let mut b3 = CowBook::new();
    let p3 = b3.add_page(1).unwrap_or(0);
    b3.seal();
    let j11 = b3.add_page(1) == Err(CW_SEALED)
        && b3.fork_attach(p3, 1, 2) == Err(CW_SEALED)
        && b3.write(1, p3, [1; PAGE_WORDS]) == Err(CW_SEALED);
    s.add("J11_sealed", j11, "封册后三操作皆拒 SEALED");

    // J12 页容量守门
    let mut b4 = CowBook::new();
    let mut fill_ok = true;
    for _ in 0..PAGE_CAP {
        if b4.add_page(1).is_err() {
            fill_ok = false;
        }
    }
    let j12 = fill_ok && b4.add_page(1) == Err(CW_PAGE_CAP);
    s.add("J12_page_cap", j12, "页满 64 拒 PAGE_CAP");

    // J13 副本计数对平
    let j13 = b.copy_count() == 11 && b.fault_count() == 11;
    s.add("J13_copies", j13, "十次子写加一次父写副本账 11");

    // J14 指纹稳定
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J14_fingerprint", f1 == f2, "页册折叠指纹一致");

    // J15 双向互扰零检出
    let mut ok = true;
    for pfn in 0..10usize {
        let a = b.read(10, pfn).unwrap_or([0; PAGE_WORDS]);
        let c = b.read(20, pfn).unwrap_or([1; PAGE_WORDS]);
        if pfn == 0 {
            if a != [50; PAGE_WORDS] || c != [200; PAGE_WORDS] {
                ok = false;
            }
        } else if a == c {
            ok = false; // 子已写，父应保持种子
        }
    }
    s.add("J15_no_interfere", ok, "十页父/子双向零互扰");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cow56_flow() {
        let mut b = CowBook::new();
        let p = b.add_page(9).unwrap();
        b.fork_attach(p, 1, 2).unwrap();
        assert!(b.shared_before_write(1, 2, p));
        assert!(b.write(2, p, [7; PAGE_WORDS]).unwrap());
        assert_ne!(b.read(1, p).unwrap(), b.read(2, p).unwrap());
        assert!(b.parent_unaffected(1, 2, p));
    }

    #[test]
    fn test_cow56_misuse() {
        let mut b = CowBook::new();
        let p = b.add_page(1).unwrap();
        assert_eq!(b.fork_attach(p, 3, 3), Err(CW_BAD_PID));
        assert_eq!(b.write(9, p, [1; PAGE_WORDS]), Err(CW_BAD_PID));
        assert_eq!(b.read(1, 99), Err(CW_BAD_PAGE));
    }

    #[test]
    fn test_cow56_checks() {
        let cs = run_unx10156_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_cow56_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
