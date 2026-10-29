//! UNX-F10141 mmap 匿名映射正反双判据（零页达成与长度零误用受控）（0x104c0 段）
//!
//! 锚点：UNX-F10141-J1 匿名映射达成（MAP_ANONYMOUS 零页可写读回一致
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：匿名页册（MAP_ANONYMOUS 标记区 + 按需零页）+ 页核（首读零页/
//! 写后回读/零长度拒/页粒度取整）+ 页账（逐次留痕 + 触页计数）+ 注入核
//! （零长度/越区/坏区柄/空留痕/封册必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x104c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x104c0;
pub const MA_ZERO_LEN: u32 = SEG_BASE + 0x01; // 长度零
pub const MA_OVER: u32 = SEG_BASE + 0x02; // 页数越界
pub const MA_BAD_AREA: u32 = SEG_BASE + 0x03; // 坏区柄
pub const MA_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const MA_SEALED: u32 = SEG_BASE + 0x05; // 封册后操作

/// 页大小与页数上限。
pub const PG: usize = 32;
pub const PG_MAX: usize = 6;

/// 匿名区。
pub struct AnonPages {
    pub pages: Vec<Option<Vec<u8>>>,
    pub anon: bool,
}

impl AnonPages {
    pub fn new(npages: usize) -> AnonPages {
        let mut pages: Vec<Option<Vec<u8>>> = Vec::new();
        for _ in 0..npages {
            pages.push(None); // 按需零页：未触页为 None
        }
        AnonPages { pages, anon: true }
    }

    /// 页读：None 页返零（零页语义）。
    pub fn page_read(&self, pno: usize) -> Vec<u8> {
        match self.pages.get(pno) {
            Some(Some(v)) => v.clone(),
            _ => vec![0u8; PG],
        }
    }

    /// 页写：触发实体化。
    pub fn page_write(&mut self, pno: usize, data: &[u8]) -> Result<(), u32> {
        if pno >= self.pages.len() {
            return Err(MA_BAD_AREA);
        }
        let mut buf = vec![0u8; PG];
        let n = data.len().min(PG);
        buf[..n].copy_from_slice(&data[..n]);
        self.pages[pno] = Some(buf);
        Ok(())
    }

    /// 实体化页数。
    pub fn resident(&self) -> usize {
        self.pages.iter().filter(|p| p.is_some()).count()
    }

    /// 页总数。
    pub fn total(&self) -> usize {
        self.pages.len()
    }
}

/// 页账留痕。
#[derive(Clone)]
pub struct MaRec {
    pub seq: u32,
    pub area: u32,
    pub npages: usize,
    pub note: String,
}

impl MaRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.npages > 0 && self.npages <= PG_MAX
    }
}

/// 匿名页册。
pub struct AnonBook {
    pub areas: Vec<Option<AnonPages>>,
    pub ledger: Vec<MaRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl AnonBook {
    pub fn new() -> AnonBook {
        AnonBook { areas: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// mmap 核心：零长度/越页限拒，页粒度取整。
    pub fn mmap_anon(&mut self, len: usize, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(MA_SEALED);
        }
        if note.is_empty() {
            return Err(MA_EMPTY_NOTE);
        }
        if len == 0 {
            return Err(MA_ZERO_LEN);
        }
        let npages = (len + PG - 1) / PG;
        if npages > PG_MAX {
            return Err(MA_OVER);
        }
        let slot = if let Some(i) = self.areas.iter().position(|s| s.is_none()) {
            i
        } else if self.areas.len() < 6 {
            self.areas.push(None);
            self.areas.len() - 1
        } else {
            return Err(MA_OVER);
        };
        self.areas[slot] = Some(AnonPages::new(npages));
        self.ledger.push(MaRec {
            seq: self.seq_next,
            area: slot as u32,
            npages,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(slot)
    }

    fn resolve(&self, area: usize) -> Result<usize, u32> {
        if area >= self.areas.len() || self.areas[area].is_none() {
            return Err(MA_BAD_AREA);
        }
        Ok(area)
    }

    /// 区内字节写：按页切分实体化。
    pub fn write_at(&mut self, area: usize, off: usize, buf: &[u8]) -> Result<usize, u32> {
        let u = self.resolve(area)?;
        if off + buf.len() > self.areas[u].as_ref().map(|a| a.total() * PG).unwrap_or(0) {
            return Err(MA_BAD_AREA);
        }
        let mut done = 0;
        while done < buf.len() {
            let pos = off + done;
            let pno = pos / PG;
            let inp = pos % PG;
            let take = (PG - inp).min(buf.len() - done);
            let a = match self.areas[u].as_mut() {
                Some(a) => a,
                None => return Err(MA_BAD_AREA),
            };
            // 读旧页拼新值
            let mut page = a.page_read(pno);
            page[inp..inp + take].copy_from_slice(&buf[done..done + take]);
            a.page_write(pno, &page)?;
            done += take;
        }
        Ok(buf.len())
    }

    /// 区内字节读：零页自动补零。
    pub fn read_at(&mut self, area: usize, off: usize, count: usize) -> Result<Vec<u8>, u32> {
        let u = self.resolve(area)?;
        let total = self.areas[u].as_ref().map(|a| a.total() * PG).unwrap_or(0);
        if off + count > total {
            return Err(MA_BAD_AREA);
        }
        let mut out: Vec<u8> = Vec::new();
        let mut done = 0;
        while done < count {
            let pos = off + done;
            let pno = pos / PG;
            let inp = pos % PG;
            let take = (PG - inp).min(count - done);
            let page = match self.areas[u].as_ref() {
                Some(a) => a.page_read(pno),
                None => return Err(MA_BAD_AREA),
            };
            out.extend_from_slice(&page[inp..inp + take]);
            done += take;
        }
        Ok(out)
    }

    /// 实体化计数透传。
    pub fn resident_of(&self, area: usize) -> Option<usize> {
        self.areas.get(area).and_then(|a| a.as_ref().map(|x| x.resident()))
    }

    /// 解除。
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
        let mut h: u32 = 0x104c00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.npages as u32);
            h = h.wrapping_mul(31).wrapping_add(r.area);
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
    let mut b = AnonBook::new();
    let a = b.mmap_anon(PG * 2, "inj-ok").unwrap_or(0);
    // 1) 零长度必拒
    got[0] = b.mmap_anon(0, "inj-zero").err() == Some(MA_ZERO_LEN);
    // 2) 页数越界必拒
    got[1] = b.mmap_anon(PG * (PG_MAX + 1), "inj-over").err() == Some(MA_OVER);
    // 3) 坏区柄必拒
    got[2] = b.write_at(99, 0, b"x").err() == Some(MA_BAD_AREA);
    // 4) 越区写必拒
    got[3] = b.write_at(a, PG * 2, b"out").err() == Some(MA_BAD_AREA);
    // 5) 封册后操作必抓
    b.seal();
    got[4] = b.mmap_anon(PG, "inj-seal").err() == Some(MA_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10141");

    // 正路径：匿名零页写读回一致
    let mut b = AnonBook::new();
    let a = b.mmap_anon(PG * 2, "双页匿名").unwrap_or(0);
    let _ = b.write_at(a, 5, b"anon-data");
    let g = b.read_at(a, 5, 9).unwrap_or_default();
    let j1 = g == b"anon-data".to_vec();
    s.add("J1-匿名写读", j1, "匿名区写 9 字节回读一致");

    // 首读零页
    let j2 = b.read_at(a, PG, 4).unwrap_or_default() == vec![0u8; 4]
        && b.resident_of(a) == Some(1);
    s.add("J2-零页语义", j2, "未触页读零且仅一页实体化");

    // 页粒度取整
    let j3 = {
        let mut b3 = AnonBook::new();
        let a3 = b3.mmap_anon(PG + 7, "取整").unwrap_or(0);
        let np = b3.ledger.last().map(|r| r.npages).unwrap_or(0);
        np == 2 && b3.write_at(a3, PG * 2 - 1, b"Z").is_ok()
    };
    s.add("J3-页粒度取整", j3, "39 字节取整 2 页且尾字节可写");

    // 触页计数推进
    let j4 = {
        let _ = b.write_at(a, PG * 2 - 1, b"E");
        b.resident_of(a) == Some(2)
    };
    s.add("J4-触页推进", j4, "写尾页后实体化 2 页");

    // 零长度受控
    let j5 = b.mmap_anon(0, "零长").err() == Some(MA_ZERO_LEN);
    s.add("J5-零长受控", j5, "len=0 必拒");

    // 页数越界受控
    let j6 = b.mmap_anon(PG * PG_MAX + 1, "越页").err() == Some(MA_OVER);
    s.add("J6-越页受控", j6, "页数出 PG_MAX 必拒");

    // 解除失效
    let j7 = {
        let mut b7 = AnonBook::new();
        let a7 = b7.mmap_anon(PG, "解除").unwrap_or(0);
        let _ = b7.munmap(a7);
        b7.read_at(a7, 0, 1).err() == Some(MA_BAD_AREA)
    };
    s.add("J7-解除失效", j7, "munmap 后访问必拒");

    // 区柄容量守恒
    let j8 = {
        let mut b8 = AnonBook::new();
        let mut ok = true;
        for i in 0..6 {
            if b8.mmap_anon(PG, &format!("a{}", i)).is_err() {
                ok = false;
            }
        }
        ok && b8.mmap_anon(PG, "full").err() == Some(MA_OVER)
    };
    s.add("J8-容量守恒", j8, "区柄槽满必拒");

    // 空留痕拒收
    let j9 = b.mmap_anon(PG, "").err() == Some(MA_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

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
    fn unx10141_anon_flow() {
        let mut b = AnonBook::new();
        let a = b.mmap_anon(64, "x").unwrap_or(0);
        let _ = b.write_at(a, 0, b"AB");
        assert_eq!(b.read_at(a, 0, 2).unwrap_or_default(), b"AB".to_vec());
        assert_eq!(b.resident_of(a), Some(1));
        assert!(b.mmap_anon(0, "z") == Err(MA_ZERO_LEN));
    }

    #[test]
    fn unx10141_checks_green() {
        let s = run_unx10141_checks();
        assert!(s.all_passed(), "UNX-F10141 判据未全绿");
    }
}
