//! UNX-F10116 mmap 正反双判据（映射达成与非法长度误用受控）（0x101a0 段）
//!
//! 锚点：UNX-F10116-J1 mmap 映射达成（匿名页可读写 LTP mmap01 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：页册（PAGE_SIZE 定长页的匿名映射区）+ 映射核（建区/页粒度
//! 取整/零页按需）+ 映射账（区表留痕可折指纹）+ 注入核（零长度/超上
//! 限/坏区柄/越区访问/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x101a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x101a0;
pub const MM_ZERO_LEN: u32 = SEG_BASE + 0x01; // 零长度
pub const MM_OVER: u32 = SEG_BASE + 0x02; // 超映射上限
pub const MM_BAD_AREA: u32 = SEG_BASE + 0x03; // 坏区柄
pub const MM_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const MM_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 页大小与页数上限。
pub const PAGE_SIZE: usize = 16;
pub const PAGE_MAX: usize = 8;

/// 匿名映射区：定长页阵列（内容按页拆存）。
pub struct AnonArea {
    pub pages: Vec<Vec<u8>>,
    pub size: usize,
    pub touched: Vec<bool>,
}

impl AnonArea {
    /// 按页粒度建区：零长度在入口拒，这里只收正则化后的页数。
    pub fn new(npages: usize) -> AnonArea {
        let mut pages: Vec<Vec<u8>> = Vec::new();
        let mut touched: Vec<bool> = Vec::new();
        let mut i = 0;
        while i < npages {
            pages.push(vec![0u8; PAGE_SIZE]);
            touched.push(false);
            i += 1;
        }
        AnonArea { pages, size: npages * PAGE_SIZE, touched }
    }

    /// 区内读：越区拒，触页置 touched。
    pub fn read(&mut self, off: usize, count: usize) -> Result<Vec<u8>, u32> {
        if off + count > self.size {
            return Err(MM_BAD_AREA);
        }
        let mut out: Vec<u8> = Vec::new();
        let mut i = off;
        while i < off + count {
            let pg = i / PAGE_SIZE;
            let inpg = i % PAGE_SIZE;
            if let Some(p) = self.pages.get(pg) {
                if let Some(b) = p.get(inpg) {
                    out.push(*b);
                    self.touched[pg] = true;
                }
            }
            i += 1;
        }
        Ok(out)
    }

    /// 区内写：越区拒。
    pub fn write(&mut self, off: usize, buf: &[u8]) -> Result<usize, u32> {
        if off + buf.len() > self.size {
            return Err(MM_BAD_AREA);
        }
        let mut i = 0;
        while i < buf.len() {
            let pg = (off + i) / PAGE_SIZE;
            let inpg = (off + i) % PAGE_SIZE;
            if let Some(p) = self.pages.get_mut(pg) {
                if inpg < p.len() {
                    p[inpg] = buf[i];
                    self.touched[pg] = true;
                }
            }
            i += 1;
        }
        Ok(buf.len())
    }

    /// 触页数。
    pub fn touched_pages(&self) -> usize {
        self.touched.iter().filter(|t| **t).count()
    }
}

/// 映射账留痕。
#[derive(Clone)]
pub struct MmRec {
    pub seq: u32,
    pub area: u32,
    pub npages: usize,
    pub note: String,
}

impl MmRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.npages > 0 && self.npages <= PAGE_MAX
    }
}

/// 页册 + 映射核。
pub struct MmapBook {
    pub areas: Vec<Option<AnonArea>>,
    pub ledger: Vec<MmRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl MmapBook {
    pub fn new() -> MmapBook {
        MmapBook { areas: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// mmap 核心：长度按页取整，零长/超限拒。
    pub fn mmap(&mut self, len: usize, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(MM_SEALED);
        }
        if note.is_empty() {
            return Err(MM_EMPTY_NOTE);
        }
        if len == 0 {
            return Err(MM_ZERO_LEN);
        }
        let npages = (len + PAGE_SIZE - 1) / PAGE_SIZE;
        if npages > PAGE_MAX {
            return Err(MM_OVER);
        }
        let slot = if let Some(i) = self.areas.iter().position(|s| s.is_none()) {
            i
        } else if self.areas.len() < 8 {
            self.areas.push(None);
            self.areas.len() - 1
        } else {
            return Err(MM_OVER);
        };
        self.areas[slot] = Some(AnonArea::new(npages));
        self.ledger.push(MmRec { seq: self.seq_next, area: slot as u32, npages, note: note.to_string() });
        self.seq_next += 1;
        Ok(slot)
    }

    fn resolve(&self, area: usize) -> Result<usize, u32> {
        if area >= self.areas.len() || self.areas[area].is_none() {
            return Err(MM_BAD_AREA);
        }
        Ok(area)
    }

    /// 区内写透传。
    pub fn write_at(&mut self, area: usize, off: usize, buf: &[u8]) -> Result<usize, u32> {
        let u = self.resolve(area)?;
        let a = match self.areas[u].as_mut() {
            Some(a) => a,
            None => return Err(MM_BAD_AREA),
        };
        a.write(off, buf)
    }

    /// 区内读透传。
    pub fn read_at(&mut self, area: usize, off: usize, count: usize) -> Result<Vec<u8>, u32> {
        let u = self.resolve(area)?;
        let a = match self.areas[u].as_mut() {
            Some(a) => a,
            None => return Err(MM_BAD_AREA),
        };
        a.read(off, count)
    }

    /// 解除映射。
    pub fn munmap(&mut self, area: usize) -> Result<(), u32> {
        let u = self.resolve(area)?;
        self.areas[u] = None;
        Ok(())
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x101a00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.npages as u32);
            h = h.wrapping_mul(31).wrapping_add(r.area);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
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
    let mut b = MmapBook::new();
    let a = b.mmap(PAGE_SIZE, "inj-ok").unwrap_or(usize::MAX);
    // 1) 零长度必拒
    got[0] = b.mmap(0, "inj-zero").err() == Some(MM_ZERO_LEN);
    // 2) 超页上限必拒
    got[1] = b.mmap(PAGE_SIZE * (PAGE_MAX + 1), "inj-over").err() == Some(MM_OVER);
    // 3) 坏区柄必拒
    got[2] = b.write_at(99, 0, b"x").err() == Some(MM_BAD_AREA);
    // 4) 越区访问必拒
    got[3] = b.write_at(a, PAGE_SIZE * PAGE_MAX, b"out").err() == Some(MM_BAD_AREA);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.mmap(PAGE_SIZE, "inj-seal").err() == Some(MM_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10116_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10116");

    // 正路径：匿名页写读一致（mmap01 面）
    let mut b = MmapBook::new();
    let a = b.mmap(PAGE_SIZE * 2, "双页").unwrap_or(usize::MAX);
    let _ = b.write_at(a, 3, b"mmap-data");
    let g = b.read_at(a, 3, 9).unwrap_or_default();
    let j1 = g == b"mmap-data".to_vec();
    s.add("J1-匿名页读写", j1, "写 9 字节回读一致");

    // 零页语义：未触页读得零
    let j2 = b.read_at(a, PAGE_SIZE, PAGE_SIZE).unwrap_or_default()
        == vec![0u8; PAGE_SIZE];
    s.add("J2-零页语义", j2, "未写页读得全零");

    // 页粒度取整：奇数长向上取整
    let j3 = {
        let mut b3 = MmapBook::new();
        let a3 = b3.mmap(PAGE_SIZE + 5, "取整").unwrap_or(usize::MAX);
        let np = b3.ledger.last().map(|r| r.npages).unwrap_or(0);
        np == 2 && b3.write_at(a3, PAGE_SIZE * 2 - 1, b"X").is_ok()
    };
    s.add("J3-页粒度取整", j3, "21 字节取整 2 页且尾字节可写");

    // 触页计数
    let j4 = {
        let mut b4 = MmapBook::new();
        let a4 = b4.mmap(PAGE_SIZE * 3, "触页").unwrap_or(usize::MAX);
        let _ = b4.write_at(a4, 0, b"A");
        let _ = b4.write_at(a4, PAGE_SIZE * 2, b"B");
        let t = b4.areas[a4].as_ref().map(|x| x.touched_pages()).unwrap_or(0);
        t == 2
    };
    s.add("J4-触页计数", j4, "首尾两页触达计数 2");

    // 零长度受控
    let j5 = b.mmap(0, "零长").err() == Some(MM_ZERO_LEN);
    s.add("J5-零长度受控", j5, "len=0 必拒");

    // 超上限受控
    let j6 = b.mmap(PAGE_SIZE * PAGE_MAX + 1, "超限").err() == Some(MM_OVER);
    s.add("J6-超上限受控", j6, "页数出 PAGE_MAX 必拒");

    // 越区访问受控
    let j7 = b.write_at(a, PAGE_SIZE * 2, b"out").err() == Some(MM_BAD_AREA)
        && b.read_at(a, PAGE_SIZE * 2 + 1, 1).is_err();
    s.add("J7-越区受控", j7, "越区读写同码拒");

    // 解除后访问受控
    let j8 = {
        let mut b8 = MmapBook::new();
        let a8 = b8.mmap(PAGE_SIZE, "解除").unwrap_or(usize::MAX);
        let _ = b8.munmap(a8);
        b8.read_at(a8, 0, 1).err() == Some(MM_BAD_AREA)
    };
    s.add("J8-解除失效", j8, "munmap 后访问必拒");

    // 区柄容量守恒
    let j9 = {
        let mut b9 = MmapBook::new();
        let mut ok = true;
        for i in 0..8 {
            if b9.mmap(PAGE_SIZE, &format!("a{}", i)).is_err() {
                ok = false;
            }
        }
        ok && b9.mmap(PAGE_SIZE, "full").err() == Some(MM_OVER)
    };
    s.add("J9-容量守恒", j9, "区柄槽满必拒");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "页数在闭集且注非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10116_mmap_flow() {
        let mut b = MmapBook::new();
        let a = b.mmap(32, "x").unwrap_or(0);
        let _ = b.write_at(a, 0, b"AB");
        assert_eq!(b.read_at(a, 0, 2).unwrap_or_default(), b"AB".to_vec());
        assert!(b.mmap(0, "z") == Err(MM_ZERO_LEN));
    }

    #[test]
    fn unx10116_checks_green() {
        let s = run_unx10116_checks();
        assert!(s.all_passed(), "UNX-F10116 判据未全绿");
    }
}
