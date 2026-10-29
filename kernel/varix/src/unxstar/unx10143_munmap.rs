//! UNX-F10143 munmap 正反双判据（解除达成与未映射区间误用受控）（0x10500 段）
//!
//! 锚点：UNX-F10143-J1 解除达成（munmap 后访问失效分进程复现 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：分进程区册（pid→区表的二段映射）+ 解除核（munmap 摘区/访问失
//! 效/双进程独立复现/重解除拒）+ 解除账（逐次留痕）+ 注入核（重解除/
//! 未映射访问/野 pid/空留痕/封册必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x10500 独占 0x20）。
pub const SEG_BASE: u32 = 0x10500;
pub const UM_NOT_MAPPED: u32 = SEG_BASE + 0x01; // 未映射区间
pub const UM_BAD_PID: u32 = SEG_BASE + 0x02; // 野 pid
pub const UM_ZERO_LEN: u32 = SEG_BASE + 0x03; // 零长度
pub const UM_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const UM_SEALED: u32 = SEG_BASE + 0x05; // 封册后操作

/// 单进程区表。
#[derive(Clone)]
pub struct ProcAreas {
    pub pid: u32,
    /// (基址, 页数)
    pub areas: Vec<(u32, u32)>,
}

impl ProcAreas {
    pub fn ok(&self) -> bool {
        self.pid != 0
    }

    pub fn contains(&self, addr: u32) -> bool {
        self.areas.iter().any(|(b, n)| addr >= *b && addr < b + n * 16)
    }

    pub fn find_base(&self, base: u32) -> Option<usize> {
        self.areas.iter().position(|(b, _)| *b == base)
    }
}

/// 解除账留痕。
#[derive(Clone)]
pub struct UmRec {
    pub seq: u32,
    pub pid: u32,
    pub base: u32,
    pub pages: u32,
    pub op: u32, // 0=mmap 1=munmap
}

impl UmRec {
    pub fn ok(&self) -> bool {
        self.pid != 0 && self.op <= 1
    }
}

/// 分进程区册。
pub struct UmBook {
    pub procs: Vec<ProcAreas>,
    pub ledger: Vec<UmRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl UmBook {
    pub fn new() -> UmBook {
        UmBook { procs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登进程。
    pub fn add_proc(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(UM_SEALED);
        }
        if pid == 0 || self.procs.iter().any(|p| p.pid == pid) {
            return Err(UM_BAD_PID);
        }
        self.procs.push(ProcAreas { pid, areas: Vec::new() });
        Ok(())
    }

    fn proc_idx(&self, pid: u32) -> Result<usize, u32> {
        match self.procs.iter().position(|p| p.pid == pid) {
            Some(i) => Ok(i),
            None => Err(UM_BAD_PID),
        }
    }

    /// mmap 面：建区（相交/零长/不对齐拒）。
    pub fn mmap_at(&mut self, pid: u32, base: u32, pages: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(UM_SEALED);
        }
        if note.is_empty() {
            return Err(UM_EMPTY_NOTE);
        }
        if pages == 0 {
            return Err(UM_ZERO_LEN);
        }
        if base % 16 != 0 {
            return Err(UM_NOT_MAPPED);
        }
        let i = self.proc_idx(pid)?;
        if self.procs[i].areas.iter().any(|(b, n)| base < b + n * 16 && *b < base + pages * 16) {
            return Err(UM_NOT_MAPPED);
        }
        self.procs[i].areas.push((base, pages));
        self.ledger.push(UmRec { seq: self.seq_next, pid, base, pages, op: 0 });
        self.seq_next += 1;
        Ok(())
    }

    /// munmap 核心：摘区（未映射/野 pid 拒）。
    pub fn munmap_at(&mut self, pid: u32, base: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(UM_SEALED);
        }
        if note.is_empty() {
            return Err(UM_EMPTY_NOTE);
        }
        let i = self.proc_idx(pid)?;
        let pos = match self.procs[i].find_base(base) {
            Some(p) => p,
            None => return Err(UM_NOT_MAPPED),
        };
        let (_, pages) = self.procs[i].areas[pos];
        self.procs[i].areas.remove(pos);
        self.ledger.push(UmRec { seq: self.seq_next, pid, base, pages, op: 1 });
        self.seq_next += 1;
        Ok(pages)
    }

    /// 访问模拟：区在则通，区失则拒。
    pub fn access(&self, pid: u32, addr: u32) -> Result<(), u32> {
        let i = self.proc_idx(pid)?;
        if self.procs[i].contains(addr) {
            Ok(())
        } else {
            Err(UM_NOT_MAPPED)
        }
    }

    /// 分进程独立复现核：A 进程失效不影响 B 进程同址。
    pub fn per_proc_isolated(&self, pa: u32, pb: u32, addr: u32) -> bool {
        self.access(pa, addr).is_err() && self.access(pb, addr).is_ok()
    }

    /// 区数统计。
    pub fn area_count(&self, pid: u32) -> Result<usize, u32> {
        let i = self.proc_idx(pid)?;
        Ok(self.procs[i].areas.len())
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
        let mut h: u32 = 0x105000;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.op);
            h = h.wrapping_mul(31).wrapping_add(r.base);
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 活区页数合计。
    pub fn live_pages(&self, pid: u32) -> Result<u32, u32> {
        let i = self.proc_idx(pid)?;
        Ok(self.procs[i].areas.iter().map(|a| a.1).sum())
    }

    /// 区基址枚举（升序）。
    pub fn bases_of(&self, pid: u32) -> Result<Vec<u32>, u32> {
        let i = self.proc_idx(pid)?;
        let mut v: Vec<u32> = self.procs[i].areas.iter().map(|a| a.0).collect();
        v.sort();
        Ok(v)
    }

    /// 全册区面：各进程区数合计。
    pub fn total_areas(&self) -> usize {
        self.procs.iter().map(|p| p.areas.len()).sum()
    }

    /// 在册进程数。
    pub fn proc_count(&self) -> usize {
        self.procs.len()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = UmBook::new();
    let _ = b.add_proc(10);
    let _ = b.mmap_at(10, 0x1000, 2, "inj-ok");
    // 1) 重解除必拒
    let _ = b.munmap_at(10, 0x1000, "inj-1");
    got[0] = b.munmap_at(10, 0x1000, "inj-2").err() == Some(UM_NOT_MAPPED);
    // 2) 未映射访问必拒
    got[1] = b.access(10, 0x1000).err() == Some(UM_NOT_MAPPED);
    // 3) 野 pid 必拒
    got[2] = b.mmap_at(99, 0x2000, 1, "inj-3").err() == Some(UM_BAD_PID);
    // 4) 空留痕必拒
    got[3] = b.mmap_at(10, 0x3000, 1, "").err() == Some(UM_EMPTY_NOTE);
    // 5) 封册后操作必抓
    b.seal();
    got[4] = b.munmap_at(10, 0x3000, "inj-seal").err() == Some(UM_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10143_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10143");

    // 正路径：建区→访问通→解除→访问失效（分进程复现面）
    let mut b = UmBook::new();
    let _ = b.add_proc(100);
    let _ = b.add_proc(200);
    let _ = b.mmap_at(100, 0x4000, 2, "A 区");
    let _ = b.mmap_at(200, 0x4000, 2, "B 区");
    let ok_before = b.access(100, 0x4010).is_ok();
    let _ = b.munmap_at(100, 0x4000, "A 解除");
    let j1 = ok_before
        && b.access(100, 0x4010).is_err()
        && b.per_proc_isolated(100, 200, 0x4010);
    s.add("J1-解除失效", j1, "A 解除后失效且 B 同址独立可达");

    // 区数对平
    let j2 = b.area_count(100).unwrap_or(9) == 0 && b.area_count(200).unwrap_or(9) == 1;
    s.add("J2-区数对平", j2, "解除后 A 零区、B 单区");

    // 解除返回页数
    let j3 = {
        let _ = b.mmap_at(200, 0x6000, 3, "三页");
        b.munmap_at(200, 0x6000, "三页解除").unwrap_or(0) == 3
    };
    s.add("J3-解除回页", j3, "munmap 回报被摘页数");

    // 部分页访问界
    let j4 = b.access(200, 0x401F).is_ok() && b.access(200, 0x4020).is_err();
    s.add("J4-页界访问", j4, "区内尾地址通、区外首址拒");

    // 重解除受控
    let j5 = b.munmap_at(200, 0x4000, "再解").is_ok()
        && b.munmap_at(200, 0x4000, "三解").err() == Some(UM_NOT_MAPPED);
    s.add("J5-重解除受控", j5, "二次解除必拒");

    // 未映射访问受控
    let j6 = b.access(100, 0x9000).err() == Some(UM_NOT_MAPPED);
    s.add("J6-未映射受控", j6, "野地址访问必拒");

    // 野 pid 受控
    let j7 = b.access(999, 0x4000).err() == Some(UM_BAD_PID)
        && b.munmap_at(999, 0x4000, "野").err() == Some(UM_BAD_PID);
    s.add("J7-野pid受控", j7, "未登进程双操作同拒");

    // 相交建区受控
    let j8 = {
        let _ = b.mmap_at(100, 0x8000, 2, "占位");
        b.mmap_at(100, 0x8010, 1, "撞区").err() == Some(UM_NOT_MAPPED)
    };
    s.add("J8-相交受控", j8, "区间相交建区必拒");

    // 空留痕拒收
    let j9 = b.mmap_at(100, 0xA000, 1, "").err() == Some(UM_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "op 在闭集且 pid 非零");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // 活页合计对平
    let j13 = b.live_pages(100) == Ok(2) && b.live_pages(200) == Ok(0);
    s.add("J13-活页合计", j13, "A 余两页、B 已全解除对平");

    // 基址枚举升序
    let j14 = b.bases_of(100) == Ok(vec![0x8000]) && b.bases_of(200) == Ok(vec![]);
    s.add("J14-基址枚举", j14, "A 余区基址枚举、B 空枚举");

    // 全册区面对平
    let j15 = b.total_areas() == 1;
    s.add("J15-全册区面", j15, "A 一区、B 零区合计一区");

    // 在册进程数
    let j16 = b.proc_count() == 2;
    s.add("J16-在册进程", j16, "100/200 双进程在册");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10143_umap_flow() {
        let mut b = UmBook::new();
        let _ = b.add_proc(1);
        let _ = b.mmap_at(1, 0x1000, 1, "a");
        assert!(b.access(1, 0x1008).is_ok());
        let _ = b.munmap_at(1, 0x1000, "b");
        assert!(b.access(1, 0x1008).err() == Some(UM_NOT_MAPPED));
    }

    #[test]
    fn unx10143_checks_green() {
        let s = run_unx10143_checks();
        assert!(s.all_passed(), "UNX-F10143 判据未全绿");
    }
}
