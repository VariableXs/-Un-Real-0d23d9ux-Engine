//! UNX 卡 10148 — msync 正反双判据（同步达成与未映射区间误用受控）
//! 段位 seg=0x105a0
//! 判据原文：UNX-F10148-J1 同步达成（MS_SYNC 返回后重读一致 10/10 次）
//! 锚点：MS_SYNC 落盘后重读一致 + MS_ASYNC 延迟刷 + 未映射区间受控
//! 域：SyncMap 脏页同步册（页缓存 + 后备帧 + 脏位面）
//! 结构：脏页累积 → MS_SYNC 即刷 → MS_ASYNC 延迟 → flush 收口 → 未映射受控 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x105a0;
pub const MS_UNMAPPED: u32 = SEG_BASE + 0x01;
pub const MS_BAD_FLAGS: u32 = SEG_BASE + 0x02;
pub const MS_SEALED: u32 = SEG_BASE + 0x03;
pub const MS_BAD_PAGE: u32 = SEG_BASE + 0x04;
pub const MS_NO_DIRTY: u32 = SEG_BASE + 0x05;

/// 页数与页粒度
pub const NPAGES: usize = 0x10;
pub const PAGE: usize = 0x1000;

/// 同步标志
pub const MS_SYNC: u8 = 0x1;
pub const MS_ASYNC: u8 = 0x2;
pub const MS_INVALIDATE: u8 = 0x4;

/// 单页：缓存帧 + 后备帧 + 脏位
#[derive(Clone, Copy)]
pub struct SyncPage {
    pub cache: u16,
    pub back: u16,
    pub dirty: bool,
}

impl SyncPage {
    pub const fn fresh() -> SyncPage {
        SyncPage {
            cache: 0,
            back: 0,
            dirty: false,
        }
    }
}

/// msync 同步册
pub struct SyncMap {
    pages: [SyncPage; NPAGES],
    /// 异步待刷集合（位图）
    pending: u16,
    flushes: usize,
    sealed: bool,
}

impl SyncMap {
    pub fn new() -> Self {
        SyncMap {
            pages: [SyncPage::fresh(); NPAGES],
            pending: 0,
            flushes: 0,
            sealed: false,
        }
    }

    pub fn flushes(&self) -> usize {
        self.flushes
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn pending(&self) -> u16 {
        self.pending
    }

    /// 写页缓存并置脏
    pub fn poke(&mut self, page: usize, val: u16) -> Result<(), u32> {
        if self.sealed {
            return Err(MS_SEALED);
        }
        if page >= NPAGES {
            return Err(MS_BAD_PAGE);
        }
        self.pages[page].cache = val;
        self.pages[page].dirty = true;
        self.pending |= 1 << page;
        Ok(())
    }

    /// msync：addr 为字节地址，须页内起点；MS_SYNC 即刷、MS_ASYNC 入队
    pub fn msync(&mut self, addr: usize, flags: u8) -> Result<usize, u32> {
        if self.sealed {
            return Err(MS_SEALED);
        }
        if flags & !(MS_SYNC | MS_ASYNC | MS_INVALIDATE) != 0 {
            return Err(MS_BAD_FLAGS);
        }
        if flags & MS_SYNC != 0 && flags & MS_ASYNC != 0 {
            return Err(MS_BAD_FLAGS);
        }
        if addr % PAGE != 0 {
            return Err(MS_UNMAPPED);
        }
        let page = addr / PAGE;
        if page >= NPAGES {
            return Err(MS_UNMAPPED);
        }
        if !self.pages[page].dirty {
            return Err(MS_NO_DIRTY);
        }
        if flags & MS_SYNC != 0 {
            self.flush_page(page);
            Ok(0)
        } else {
            self.pending |= 1 << page;
            Ok(1)
        }
    }

    /// 单页落后备帧
    fn flush_page(&mut self, page: usize) {
        self.pages[page].back = self.pages[page].cache;
        self.pages[page].dirty = false;
        self.pending &= !(1 << page);
        self.flushes += 1;
    }

    /// 异步收口：刷全部 pending
    pub fn flush_pending(&mut self) -> usize {
        let mut n = 0;
        for p in 0..NPAGES {
            if self.pending & (1 << p) != 0 {
                self.flush_page(p);
                n += 1;
            }
        }
        n
    }

    /// 重读：后备帧内容
    pub fn reread(&self, page: usize) -> Option<u16> {
        if page >= NPAGES {
            return None;
        }
        Some(self.pages[page].back)
    }

    /// 缓存内容
    pub fn cache_of(&self, page: usize) -> Option<u16> {
        if page >= NPAGES {
            return None;
        }
        Some(self.pages[page].cache)
    }

    /// MS_INVALIDATE 语义：刷后缓存重载自后备帧（此处为读回一致）
    pub fn invalidate_probe(&self, page: usize) -> bool {
        if page >= NPAGES {
            return false;
        }
        !self.pages[page].dirty && self.pages[page].cache == self.pages[page].back
    }

    /// 脏页计数
    pub fn dirty_count(&self) -> usize {
        self.pages.iter().filter(|p| p.dirty).count()
    }

    /// 一致性审计：全部页 cache==back 且无脏
    pub fn audit_clean(&self) -> bool {
        self.dirty_count() == 0
            && self
                .pages
                .iter()
                .all(|p| p.cache == p.back)
    }

    /// 同步指纹
    pub fn render_backs(&self) -> String {
        let mut s = String::new();
        for (i, p) in self.pages.iter().enumerate() {
            if p.back != 0 {
                s.push_str(&format!("{}:{};", i, p.back));
            }
        }
        s
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut m = SyncMap::new();
    // 1) 未映射区间（越界页地址）必拒
    got[0] = m.msync(NPAGES as usize * PAGE, MS_SYNC).is_err();
    // 2) 非对齐地址必拒
    got[1] = m.msync(0x1800, MS_SYNC).is_err();
    // 3) 非法标志位必拒
    got[2] = m.msync(0, 0x8).is_err();
    // 4) SYNC|ASYNC 同给必拒
    got[3] = m.msync(0, MS_SYNC | MS_ASYNC).is_err();
    // 5) 干净页同步必拒
    let _ = m.poke(3, 7);
    let _ = m.msync(3 * PAGE, MS_SYNC);
    got[4] = m.msync(3 * PAGE, MS_SYNC).is_err();
    got
}

/// 判据入口：UNX-F10148
pub fn run_unx10148_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10148");
    let mut m = SyncMap::new();

    // J1 MS_SYNC 同步达成 10/10：写→同步→重读一致
    let mut j1 = true;
    for i in 0..10usize {
        let v = (i as u16) * 3 + 1;
        if m.poke(i, v).is_err() {
            j1 = false;
        }
        if m.msync(i * PAGE, MS_SYNC) != Ok(0) {
            j1 = false;
        }
        if m.reread(i) != Some(v) {
            j1 = false;
        }
    }
    s.add("J1_sync_10x", j1, "MS_SYNC 返回后重读一致 10/10");

    // J2 未映射区间受控
    let j2 = m.msync(NPAGES as usize * PAGE, MS_SYNC) == Err(MS_UNMAPPED)
        && m.msync(0x1800, MS_SYNC) == Err(MS_UNMAPPED);
    s.add("J2_unmapped", j2, "越界与非对齐地址受控 UNMAPPED");

    // J3 非法标志受控
    let j3 = m.msync(0, 0x8) == Err(MS_BAD_FLAGS)
        && m.msync(0, MS_SYNC | MS_ASYNC) == Err(MS_BAD_FLAGS);
    s.add("J3_bad_flags", j3, "未知位与 SYNC|ASYNC 互斥受控");

    // J4 MS_ASYNC 延迟刷
    let mut m2 = SyncMap::new();
    let _ = m2.poke(2, 0xAB);
    let _ = m2.poke(5, 0xCD);
    let r = m2.msync(2 * PAGE, MS_ASYNC);
    let j4 = r == Ok(1) && m2.pending() & 0b100100 != 0 && m2.reread(2) == Some(0);
    s.add("J4_async_defer", j4, "MS_ASYNC 入队未落盘");

    // J5 异步收口
    let n = m2.flush_pending();
    let j5 = n == 2 && m2.reread(2) == Some(0xAB) && m2.reread(5) == Some(0xCD);
    s.add("J5_flush_pending", j5, "收口刷 2 页后备帧一致");

    // J6 干净页同步受控
    let mut m3 = SyncMap::new();
    let _ = m3.poke(4, 9);
    let _ = m3.msync(4 * PAGE, MS_SYNC);
    s.add("J6_no_dirty", m3.msync(4 * PAGE, MS_SYNC) == Err(MS_NO_DIRTY), "干净页二次同步受控");

    // J7 flush 计数守恒
    let fl = m.flushes() + m2.flushes() + m3.flushes();
    s.add("J7_flushes", fl == 13, "总刷次 10+2+1 守恒");

    // J8 脏位面清零
    let j8 = m.dirty_count() == 0 && m2.dirty_count() == 0;
    s.add("J8_clean", j8, "同步后脏页归零");

    // J9 INVALIDATE 探针
    let j9 = m.invalidate_probe(0) && m.invalidate_probe(9);
    s.add("J9_invalidate", j9, "INVALIDATE 探针 cache==back 无脏");

    // J10 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 封账受控
    let mut m4 = SyncMap::new();
    m4.seal();
    let j11 = m4.poke(1, 1) == Err(MS_SEALED) && m4.msync(PAGE, MS_SYNC) == Err(MS_SEALED);
    s.add("J11_sealed", j11, "封账后 poke/msync 皆拒 SEALED");

    // J12 坏页号受控
    let j12 = m.poke(NPAGES, 1) == Err(MS_BAD_PAGE) && m.reread(NPAGES).is_none();
    s.add("J12_bad_page", j12, "越界页号 poke/reread 双受控");

    // J13 审计干净
    s.add("J13_audit", m.audit_clean(), "全册 cache==back 且无脏");

    // J14 同步指纹稳定
    let fp = m.render_backs();
    let fp2 = m.render_backs();
    s.add("J14_fingerprint", fp == fp2 && fp.contains("0:1;"), "后备帧指纹稳定");

    // J15 缓存重载一致（INVALIDATE 全页）
    let j15 = (0..10usize).all(|i| m.invalidate_probe(i));
    s.add("J15_reload", j15, "10 页缓存重载一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msync48_sync() {
        let mut m = SyncMap::new();
        m.poke(1, 0x42).unwrap();
        assert_eq!(m.msync(PAGE, MS_SYNC), Ok(0));
        assert_eq!(m.reread(1), Some(0x42));
    }

    #[test]
    fn test_msync48_async() {
        let mut m = SyncMap::new();
        m.poke(0, 5).unwrap();
        assert_eq!(m.msync(0, MS_ASYNC), Ok(1));
        assert_eq!(m.reread(0), Some(0));
        assert_eq!(m.flush_pending(), 1);
        assert_eq!(m.reread(0), Some(5));
    }

    #[test]
    fn test_msync48_misuse() {
        let mut m = SyncMap::new();
        assert_eq!(m.msync(0x800, MS_SYNC), Err(MS_UNMAPPED));
        assert_eq!(m.msync(0, 0x8), Err(MS_BAD_FLAGS));
        assert_eq!(m.msync(0, MS_SYNC | MS_ASYNC), Err(MS_BAD_FLAGS));
        assert_eq!(m.msync(0, MS_SYNC), Err(MS_NO_DIRTY));
    }

    #[test]
    fn test_msync48_checks() {
        let cs = run_unx10148_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_msync48_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
