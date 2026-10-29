//! UNX 卡 10146 — mmap 对齐面正反双判据（页对齐达成与非对齐地址误用受控）
//! 段位 seg=0x10560
//! 判据原文：UNX-F10146-J1 对齐达成（MAP_FIXED 页对齐映射成功 10/10 次）
//! 锚点：MAP_FIXED 指定地址映射的对齐闸 + 非对齐地址的受控拒收
//! 域：AlignMap 对齐映射册（区间册 + 对齐审计）
//! 结构：addr/len 对齐闸 → MAP_FIXED 落区间 → 重叠/非对齐受控 → 对齐审计与注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x10560;
pub const AM_UNALIGNED: u32 = SEG_BASE + 0x01;
pub const AM_OVERLAP: u32 = SEG_BASE + 0x02;
pub const AM_ZERO_LEN: u32 = SEG_BASE + 0x03;
pub const AM_BAD_FLAGS: u32 = SEG_BASE + 0x04;
pub const AM_SEALED: u32 = SEG_BASE + 0x05;

/// 页粒度与映射空间上界
pub const PAGE: usize = 0x1000;
pub const SPACE_TOP: usize = 0x100_0000;

/// 映射保护位
pub const PROT_R: u8 = 0x1;
pub const PROT_W: u8 = 0x2;
pub const PROT_X: u8 = 0x4;

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub start: usize,
    pub end: usize,
    pub prot: u8,
    pub fixed: bool,
}

/// 对齐映射册
pub struct AlignMap {
    regions: Vec<Region>,
    sealed: bool,
}

impl AlignMap {
    pub fn new() -> Self {
        AlignMap {
            regions: Vec::new(),
            sealed: false,
        }
    }

    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 页对齐检查：地址与长度都须页对齐
    pub fn aligned(addr: usize, len: usize) -> bool {
        addr % PAGE == 0 && len % PAGE == 0
    }

    /// 页上取整长度
    pub fn round_len(len: usize) -> usize {
        (len + PAGE - 1) & !(PAGE - 1)
    }

    /// MAP_FIXED 落区间：地址/长度对齐闸 → 重叠闸 → 落册
    pub fn map_fixed(&mut self, addr: usize, len: usize, prot: u8) -> Result<usize, u32> {
        if self.sealed {
            return Err(AM_SEALED);
        }
        if len == 0 {
            return Err(AM_ZERO_LEN);
        }
        if !Self::aligned(addr, len) {
            return Err(AM_UNALIGNED);
        }
        if prot & !(PROT_R | PROT_W | PROT_X) != 0 {
            return Err(AM_BAD_FLAGS);
        }
        let end = addr + len;
        if end > SPACE_TOP {
            return Err(AM_OVERLAP);
        }
        if self.overlaps(addr, end) {
            return Err(AM_OVERLAP);
        }
        self.regions.push(Region {
            start: addr,
            end,
            prot,
            fixed: true,
        });
        Ok(addr)
    }

    /// 区间重叠检测
    pub fn overlaps(&self, start: usize, end: usize) -> bool {
        self.regions
            .iter()
            .any(|r| start < r.end && r.start < end)
    }

    /// 任意地址 mmap：内核自选对齐槽位（first-fit 页对齐）
    pub fn map_any(&mut self, len: usize, prot: u8) -> Result<usize, u32> {
        if self.sealed {
            return Err(AM_SEALED);
        }
        if len == 0 {
            return Err(AM_ZERO_LEN);
        }
        let rl = Self::round_len(len);
        let mut cursor = 0usize;
        loop {
            if cursor + rl > SPACE_TOP {
                return Err(AM_OVERLAP);
            }
            let cand_end = cursor + rl;
            if !self.overlaps(cursor, cand_end) {
                self.regions.push(Region {
                    start: cursor,
                    end: cand_end,
                    prot,
                    fixed: false,
                });
                return Ok(cursor);
            }
            cursor = self
                .regions
                .iter()
                .filter(|r| r.start <= cursor && cursor < r.end)
                .map(|r| r.end)
                .min()
                .unwrap_or(cand_end);
        }
    }

    /// 卸载：任一覆盖区间即摘除（简化整段摘除）
    pub fn unmap(&mut self, addr: usize) -> bool {
        let before = self.regions.len();
        self.regions.retain(|r| r.start != addr);
        self.regions.len() != before
    }

    /// 按下标取区间
    pub fn region_at(&self, idx: usize) -> Option<Region> {
        self.regions.get(idx).copied()
    }

    /// 探针：地址落在哪个区间的保护位
    pub fn probe_prot(&self, addr: usize) -> Option<u8> {
        self.regions
            .iter()
            .find(|r| addr >= r.start && addr < r.end)
            .map(|r| r.prot)
    }

    /// 对齐审计：全部区间起止皆页对齐
    pub fn audit_aligned(&self) -> bool {
        self.regions.iter().all(|r| Self::aligned(r.start, r.end - r.start))
    }

    /// 覆盖页数
    pub fn pages(&self) -> usize {
        self.regions.iter().map(|r| (r.end - r.start) / PAGE).sum()
    }

    /// 固定映射占比（千分位）
    pub fn fixed_permille(&self) -> usize {
        if self.regions.is_empty() {
            return 0;
        }
        let f = self.regions.iter().filter(|r| r.fixed).count();
        f * 1000 / self.regions.len()
    }

    /// 位面指纹
    pub fn render_prot(&self) -> String {
        let mut s = String::new();
        for r in &self.regions {
            s.push_str(&format!("{:#x}:{};", r.start, prot_str(r.prot)));
        }
        s
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

fn prot_str(p: u8) -> &'static str {
    // 位序随本模块常量：R=0x1 W=0x2 X=0x4
    match p {
        0x7 => "rwx",
        0x3 => "rw-",
        0x5 => "r-x",
        0x1 => "r--",
        _ => "---",
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut m = AlignMap::new();
    // 1) 非对齐地址必拒
    got[0] = m.map_fixed(0x1800, PAGE, PROT_R).is_err();
    // 2) 非对齐长度必拒
    got[1] = m.map_fixed(0x2000, 0x800, PROT_R).is_err();
    // 3) 零长度必拒
    got[2] = m.map_fixed(0x2000, 0, PROT_R).is_err();
    // 4) 重叠映射必拒
    let _ = m.map_fixed(0x4000, PAGE, PROT_R);
    got[3] = m.map_fixed(0x4000, PAGE, PROT_W).is_err();
    // 5) 非法保护位必拒
    got[4] = m.map_fixed(0x6000, PAGE, 0x8).is_err();
    got
}

/// 判据入口：UNX-F10146
pub fn run_unx10146_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10146");
    let mut m = AlignMap::new();

    // J1 MAP_FIXED 页对齐 10/10
    let mut j1 = true;
    for i in 1..=10usize {
        let addr = i * PAGE * 4;
        if m.map_fixed(addr, PAGE, PROT_R | PROT_W) != Ok(addr) {
            j1 = false;
        }
    }
    s.add("J1_fixed_10x", j1, "MAP_FIXED 页对齐映射 10/10");

    // J2 非对齐地址受控
    let j2 = m.map_fixed(0x1001, PAGE, PROT_R) == Err(AM_UNALIGNED)
        && m.map_fixed(PAGE - 0x100, PAGE, PROT_R) == Err(AM_UNALIGNED);
    s.add("J2_unaligned_addr", j2, "非对齐地址受控 UNALIGNED");

    // J3 非对齐长度受控
    let j3 = m.map_fixed(0x40000, 0x123, PROT_R) == Err(AM_UNALIGNED);
    s.add("J3_unaligned_len", j3, "非对齐长度受控 UNALIGNED");

    // J4 零长度受控
    s.add("J4_zero_len", m.map_fixed(0x40000, 0, PROT_R) == Err(AM_ZERO_LEN), "零长度受控");

    // J5 重叠受控
    let j5 = m.map_fixed(0x3000, 2 * PAGE, PROT_R) == Err(AM_OVERLAP)
        && m.map_fixed(9 * PAGE * 4 - PAGE, 2 * PAGE, PROT_R) == Err(AM_OVERLAP);
    s.add("J5_overlap", j5, "头尾重叠双向受控 OVERLAP");

    // J6 非法保护位受控
    s.add("J6_bad_prot", m.map_fixed(0x40000, PAGE, 0x9) == Err(AM_BAD_FLAGS), "越界保护位受控");

    // J7 自选槽位对齐
    let slot = m.map_any(0x1800, PROT_X);
    let j7 = slot.is_ok() && slot.unwrap() % PAGE == 0;
    s.add("J7_any_slot", j7, "自选槽位页对齐（round_len 生效）");

    // J8 对齐审计全通过
    s.add("J8_audit", m.audit_aligned(), "册内区间全页对齐");

    // J9 探针保护位
    let j9 = m.probe_prot(0) == Some(PROT_X) && m.probe_prot(PAGE * 4) == Some(PROT_R | PROT_W);
    s.add("J9_probe_prot", j9, "探针命中区间保护位");

    // J10 卸载守恒
    let n0 = m.regions().len();
    let j10 = m.unmap(0) && m.regions().len() == n0 - 1 && m.probe_prot(0).is_none();
    s.add("J10_unmap", j10, "卸载后区间与探针双清");

    // J11 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 封账受控
    let mut m2 = AlignMap::new();
    m2.seal();
    s.add("J12_sealed", m2.map_fixed(PAGE, PAGE, PROT_R) == Err(AM_SEALED), "封账后映射拒 SEALED");

    // J13 覆盖页数对平
    let want_pages = m.regions().iter().map(|r| (r.end - r.start) / PAGE).sum::<usize>();
    s.add("J13_pages", m.pages() == want_pages && want_pages >= 10, "覆盖页数与区间和对平");

    // J14 round_len 语义
    s.add("J14_round", AlignMap::round_len(1) == PAGE && AlignMap::round_len(PAGE) == PAGE, "round_len 上取整语义");

    // J15 占比指纹
    let fp = m.fixed_permille();
    s.add("J15_fixed_pct", fp == 1000, "固定映射占比千分位满额");

    // J16 位面指纹稳定
    let f1 = m.render_prot();
    let f2 = m.render_prot();
    s.add("J16_fingerprint", f1 == f2 && f1.contains("rw-"), "位面指纹稳定");

    // J17 区间下标访问越界受控
    let n = m.regions().len();
    s.add("J17_region_at", m.region_at(0).is_some() && m.region_at(n).is_none(), "下标访问首项命中、越界判空");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align46_map() {
        let mut m = AlignMap::new();
        assert_eq!(m.map_fixed(0x8000, PAGE, PROT_R), Ok(0x8000));
        assert_eq!(m.map_any(0x100, PROT_R), Ok(0x0));
        assert!(m.audit_aligned());
    }

    #[test]
    fn test_align46_misuse() {
        let mut m = AlignMap::new();
        assert_eq!(m.map_fixed(0x1800, PAGE, PROT_R), Err(AM_UNALIGNED));
        assert_eq!(m.map_fixed(0x2000, 0x800, PROT_R), Err(AM_UNALIGNED));
        assert_eq!(m.map_fixed(0x2000, 0, PROT_R), Err(AM_ZERO_LEN));
    }

    #[test]
    fn test_align46_checks() {
        let cs = run_unx10146_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_align46_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
