//! UNX 卡 10158 — mmap 选址正反双判据（内核选址达成与 MAP_FIXED 冲突误用受控）
//! 段位 seg=0x106e0
//! 判据原文：UNX-F10158-J1 内核选址达成（addr=NULL 内核返回合法页对齐地址 10/10 次）
//! 锚点：addr=NULL 内核自选槽位 + MAP_FIXED 冲突受控 + 选址不撞已占区
//! 域：PickBook 选址册（空洞表 + first-fit 选核 + 冲突账）
//! 结构：挂已占区 → 选址探核 → MAP_FIXED 冲突拒 → 空洞遍历 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x106e0;
pub const PK_FIXED_CONFLICT: u32 = SEG_BASE + 0x01;
pub const PK_SEALED: u32 = SEG_BASE + 0x02;
pub const PK_BAD_LEN: u32 = SEG_BASE + 0x03;
pub const PK_SPACE_FULL: u32 = SEG_BASE + 0x04;
pub const PK_UNALIGNED: u32 = SEG_BASE + 0x05;

/// 地址空间顶与页粒度
pub const SPACE_TOP: usize = 0x100_0000;
pub const PAGE: usize = 0x1000;
pub const REGION_CAP: usize = 0x40;

/// 已占区
#[derive(Clone, Copy, Debug)]
pub struct Occupied {
    pub start: usize,
    pub npages: usize,
}

impl Occupied {
    pub fn end(&self) -> usize {
        self.start + self.npages * PAGE
    }
}

/// 选址册
pub struct PickBook {
    pub regions: Vec<Occupied>,
    pub picks: usize,
    pub conflicts: usize,
    sealed: bool,
}

impl PickBook {
    pub fn new() -> Self {
        PickBook {
            regions: Vec::new(),
            picks: 0,
            conflicts: 0,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn regions(&self) -> &[Occupied] {
        &self.regions
    }

    pub fn conflicts(&self) -> usize {
        self.conflicts
    }

    /// 挂已占区
    pub fn occupy(&mut self, start: usize, npages: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(PK_SEALED);
        }
        if npages == 0 {
            return Err(PK_BAD_LEN);
        }
        if start % PAGE != 0 {
            return Err(PK_UNALIGNED);
        }
        if start + npages * PAGE > SPACE_TOP {
            return Err(PK_SPACE_FULL);
        }
        if self.regions.len() >= REGION_CAP {
            return Err(PK_SPACE_FULL);
        }
        self.regions.push(Occupied { start, npages });
        Ok(())
    }

    fn overlaps(&self, start: usize, end: usize) -> bool {
        self.regions.iter().any(|r| start < r.end() && r.start < end)
    }

    /// mmap(addr=NULL)：first-fit 选空洞，返回页对齐地址
    pub fn pick_any(&mut self, npages: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(PK_SEALED);
        }
        if npages == 0 {
            return Err(PK_BAD_LEN);
        }
        let need = npages * PAGE;
        let mut cursor = PAGE; // 0 页保留（零页哨兵）
        loop {
            if cursor + need > SPACE_TOP {
                return Err(PK_SPACE_FULL);
            }
            if !self.overlaps(cursor, cursor + need) {
                self.regions.push(Occupied {
                    start: cursor,
                    npages,
                });
                self.picks += 1;
                return Ok(cursor);
            }
            // 跳到覆盖 cursor 的区尾
            let next = self
                .regions
                .iter()
                .filter(|r| r.start <= cursor && cursor < r.end())
                .map(|r| r.end())
                .min()
                .unwrap_or(cursor + PAGE);
            cursor = next;
        }
    }

    /// mmap(MAP_FIXED)：指定地址，冲突受控
    pub fn pick_fixed(&mut self, addr: usize, npages: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(PK_SEALED);
        }
        if npages == 0 {
            return Err(PK_BAD_LEN);
        }
        if addr % PAGE != 0 {
            return Err(PK_UNALIGNED);
        }
        if addr < PAGE {
            return Err(PK_FIXED_CONFLICT); // 零页保护区
        }
        if addr + npages * PAGE > SPACE_TOP {
            return Err(PK_SPACE_FULL);
        }
        if self.overlaps(addr, addr + npages * PAGE) {
            self.conflicts += 1;
            return Err(PK_FIXED_CONFLICT);
        }
        self.regions.push(Occupied { start: addr, npages });
        Ok(addr)
    }

    /// 选址对齐审计：全部区页对齐
    pub fn audit_aligned(&self) -> bool {
        self.regions.iter().all(|r| r.start % PAGE == 0)
    }

    /// 空洞计数（相邻区之间的空闲段，粗探）
    pub fn hole_probe(&self, probe_addr: usize, npages: usize) -> bool {
        !self.overlaps(probe_addr, probe_addr + npages * PAGE)
    }

    /// 选址指纹
    pub fn render(&self) -> String {
        let mut s = String::new();
        for r in &self.regions {
            s.push_str(&format!("{:#x}:{};", r.start, r.npages));
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
    let mut b = PickBook::new();
    let _ = b.occupy(0x8000, 2);
    // 1) MAP_FIXED 撞区必拒
    got[0] = b.pick_fixed(0x9000, 1).is_err();
    // 2) 零页保护区必拒
    got[1] = b.pick_fixed(0, 1).is_err();
    // 3) 非对齐必拒
    got[2] = b.pick_fixed(0x1800, 1).is_err() && b.occupy(0x1800, 1).is_err();
    // 4) 零页数必拒
    got[3] = b.pick_any(0).is_err();
    // 5) 封册后必拒
    b.seal();
    got[4] = b.pick_any(1).is_err();
    got
}

/// 判据入口：UNX-F10158
pub fn run_unx10158_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10158");
    let mut b = PickBook::new();

    // J1 内核选址 10/10：逐次自选皆页对齐且不撞
    let mut j1 = true;
    let mut prev = 0usize;
    for _ in 0..10 {
        match b.pick_any(1) {
            Ok(addr) => {
                if addr % PAGE != 0 || addr < PAGE {
                    j1 = false;
                }
                if addr <= prev {
                    j1 = false;
                }
                prev = addr;
            }
            Err(_) => {
                j1 = false;
                break;
            }
        }
    }
    s.add("J1_pick_10x", j1 && b.picks == 10, "addr=NULL 自选页对齐地址 10/10");

    // J2 选址避让已占区
    let mut b2 = PickBook::new();
    let _ = b2.occupy(0x1000, 4);
    let a = b2.pick_any(2);
    let j2 = a == Ok(0x5000) && b2.hole_probe(0x1000, 4) == false;
    s.add("J2_avoid", j2, "自选跳过已占区落 0x5000");

    // J3 MAP_FIXED 空位成功
    let f = b2.pick_fixed(0x9000, 2);
    let j3 = f == Ok(0x9000);
    s.add("J3_fixed_ok", j3, "空位固定映射成功");

    // J4 MAP_FIXED 冲突受控
    let c0 = b2.conflicts();
    let j4 = b2.pick_fixed(0x9000, 1) == Err(PK_FIXED_CONFLICT)
        && b2.pick_fixed(0x5000, 2) == Err(PK_FIXED_CONFLICT)
        && b2.conflicts() == c0 + 2;
    s.add("J4_fixed_conflict", j4, "撞区固定映射双拒且冲突账 +2");

    // J5 零页保护区受控
    let j5 = b2.pick_fixed(0, 1) == Err(PK_FIXED_CONFLICT)
        && b2.pick_fixed(PAGE / 2, 1) == Err(PK_UNALIGNED);
    s.add("J5_zero_guard", j5, "零页区拒固定映射、半页拒非对齐");

    // J6 非对齐受控
    let j6 = b2.occupy(0x1800, 1) == Err(PK_UNALIGNED)
        && b2.pick_fixed(0x1FFF, 1) == Err(PK_UNALIGNED);
    s.add("J6_unaligned", j6, "挂区与固定映射非对齐双拒");

    // J7 零页数受控
    let j7 = b2.pick_any(0) == Err(PK_BAD_LEN) && b2.pick_fixed(0x20000, 0) == Err(PK_BAD_LEN);
    s.add("J7_zero_len", j7, "零页数双路受控");

    // J8 对齐审计
    s.add("J8_aligned", b2.audit_aligned(), "全部区页对齐");

    // J9 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J9_inject_all", caught == 5, "5 注入点全检出");

    // J10 封册受控
    let mut b3 = PickBook::new();
    b3.seal();
    let j10 = b3.pick_any(1) == Err(PK_SEALED)
        && b3.pick_fixed(0x1000, 1) == Err(PK_SEALED)
        && b3.occupy(0x1000, 1) == Err(PK_SEALED);
    s.add("J10_sealed", j10, "封册后三入口皆拒 SEALED");

    // J11 空间耗尽受控
    let mut b4 = PickBook::new();
    let r = b4.occupy(SPACE_TOP - PAGE, 2);
    let j11 = r == Err(PK_SPACE_FULL) && b4.pick_any(0x1000) == Err(PK_SPACE_FULL);
    s.add("J11_space_full", j11, "越顶挂区与超大自选同拒");

    // J12 指纹稳定
    let f1 = b2.render();
    let f2 = b2.render();
    s.add("J12_fingerprint", f1 == f2 && f1.starts_with("0x1000:4;"), "占区指纹稳定");

    // J13 选址计数
    s.add("J13_picks", b.picks == 10 && b.conflicts() == 0, "自选 10 次、冲突零");

    // J14 空洞探核
    let j14 = b2.hole_probe(0x20000, 2) && !b2.hole_probe(0x1000, 1);
    s.add("J14_hole", j14, "空位探真、占位探假");

    // J15 首地址哨兵
    let a0 = b.pick_any(1);
    let j15 = a0 == Ok(11 * PAGE) && a0.unwrap_or(0) >= PAGE;
    s.add("J15_sentinel", j15, "续选避开十连选落 0xB000");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pick58_any() {
        let mut b = PickBook::new();
        b.occupy(0x1000, 4).unwrap();
        assert_eq!(b.pick_any(2), Ok(0x5000));
        assert!(b.audit_aligned());
    }

    #[test]
    fn test_pick58_conflict() {
        let mut b = PickBook::new();
        b.occupy(0x8000, 2).unwrap();
        assert_eq!(b.pick_fixed(0x9000, 1), Err(PK_FIXED_CONFLICT));
        assert_eq!(b.pick_fixed(0, 1), Err(PK_FIXED_CONFLICT));
        assert_eq!(b.pick_fixed(0x1800, 1), Err(PK_UNALIGNED));
    }

    #[test]
    fn test_pick58_checks() {
        let cs = run_unx10158_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_pick58_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
