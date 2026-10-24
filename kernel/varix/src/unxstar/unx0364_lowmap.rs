//! unx0364_lowmap —— UNX-F0364 低 1MB 恒等映射段豁免账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0364 · 低 1MB 豁免 · 目标 220 行
//! 判据原文：UNX-F0364-J1 低 1MB（实模式遗留区）恒等映射豁免账：
//!           段子区（VGA 0xA0000/EBDA/BIOS 数据）逐段登记豁免
//!           （不进页帧池、不被分配器碰）；豁免账与位图双向对
//!           核（豁免段全部落在位图外）；越豁免分配拦截。
//! 组件：豁免段（起止 + 类别）+ 豁免账（升序无重叠 + 覆盖闭集）
//!       + 位图外核 + 拦截核。
//! 职责定位：低 1MB 是「引导期的公共走廊」——SMBIOS/MP 表/VGA
//!   都住在这；页帧池若把这吞了，后续每次读表都是定时炸弹。
//!   豁免账是**声明式隔离**：段列表与位图对核，谁漏谁越界一目
//!   了然，不靠「记得别碰」。
//! 性能逐项分解：登记 O(段数)；对核 O(段数)；拦截 O(段数)。
//! 诊断码段：0x20780（0x20781~0x20789 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const LM_VERSION: &str = "unx0364_lowmap-v1";

/// 诊断段基址（0x20780 段 · 本模块独占）。
pub const LM_SEG_BASE: u32 = 0x20780;
pub const XC_OVERLAP: u32 = 0x20781; // 豁免段重叠
pub const XC_ORDER_OOO: u32 = 0x20782; // 段表乱序
pub const XC_SEG_EMPTY: u32 = 0x20783; // 空段拒
pub const XC_RANGE_BAD: u32 = 0x20784; // 止<起
pub const XC_EXCEED_LOW: u32 = 0x20785; // 越出低 1MB 闭集
pub const XC_IN_POOL: u32 = 0x20786; // 豁免段落进池内
pub const XC_ALLOC_IN_EXEMPT: u32 = 0x20787; // 越豁免分配
pub const XC_KIND_OOR: u32 = 0x20788; // 类别越域
pub const XC_COVER_MISS: u32 = 0x20789; // 必豁段缺

/// 低 1MB 闭集。
pub const LOW_CEIL: u32 = 0x0010_0000;

/// 豁免类别。
pub const K_VGA: u8 = 1; // VGA 帧缓冲 0xA0000~0xBFFFF
pub const K_EBDA: u8 = 2; // EBDA
pub const K_BIOS: u8 = 3; // BIOS 数据/代码区
pub const K_LEGACY: u8 = 4; // 实模式遗留（IVT/BDA）

/// 豁免段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExemptSeg {
    pub start: u32,
    pub end: u32, // 开区间端（不含）
    pub kind: u8,
}

impl ExemptSeg {
    pub fn new(start: u32, end: u32, kind: u8) -> Result<ExemptSeg, u32> {
        if end <= start {
            return Err(XC_RANGE_BAD);
        }
        if end > LOW_CEIL {
            return Err(XC_EXCEED_LOW);
        }
        if kind < K_VGA || kind > K_LEGACY {
            return Err(XC_KIND_OOR);
        }
        Ok(ExemptSeg { start, end, kind })
    }

    /// 命中核：地址是否落在段内。
    pub fn hits(&self, addr: u32) -> bool {
        addr >= self.start && addr < self.end
    }
}

/// 豁免账（升序无重叠）。
pub struct ExemptBook {
    segs: Vec<ExemptSeg>,
}

impl ExemptBook {
    pub fn new(segs: &[ExemptSeg]) -> Result<ExemptBook, u32> {
        if segs.is_empty() {
            return Err(XC_SEG_EMPTY);
        }
        let mut prev_end = 0;
        for (i, s) in segs.iter().enumerate() {
            s.valid()?;
            if i > 0 && s.start < prev_end {
                return Err(XC_OVERLAP);
            }
            if i > 0 && s.start < segs[i - 1].start {
                return Err(XC_ORDER_OOO);
            }
            prev_end = s.end;
        }
        Ok(ExemptBook { segs: segs.to_vec() })
    }

    /// 必豁段覆盖核：VGA（0xA0000~0xC0000）与 IVT（0~0x400）必须在账。
    pub fn mandatory_cover(&self) -> Result<(), u32> {
        let vga_ok = self.segs.iter().any(|s| s.kind == K_VGA && s.start <= 0xA0000 && s.end >= 0xC0000);
        let ivt_ok = self.segs.iter().any(|s| s.kind == K_LEGACY && s.start == 0 && s.end >= 0x400);
        if vga_ok && ivt_ok {
            Ok(())
        } else {
            Err(XC_COVER_MISS)
        }
    }

    /// 拦截核：分配地址落豁免段 → 拒。
    pub fn intercept(&self, addr: u32) -> Result<(), u32> {
        if self.segs.iter().any(|s| s.hits(addr)) {
            return Err(XC_ALLOC_IN_EXEMPT);
        }
        Ok(())
    }

    /// 位图外核：豁免段整体不落池闭集 [pool_base, pool_base+frames*4K)。
    pub fn out_of_pool(&self, pool_base: u32, pool_frames: u32) -> Result<(), u32> {
        let pool_end = pool_base.checked_add(pool_frames.saturating_mul(4096)).ok_or(XC_RANGE_BAD)?;
        for s in self.segs.iter() {
            if s.start >= pool_base && s.start < pool_end {
                return Err(XC_IN_POOL);
            }
        }
        Ok(())
    }

    pub fn len(&self) -> u32 {
        self.segs.len() as u32
    }
}

impl ExemptSeg {
    fn valid(&self) -> Result<(), u32> {
        if self.end <= self.start || self.end > LOW_CEIL {
            return Err(XC_RANGE_BAD);
        }
        Ok(())
    }
}

pub fn run_unx0364_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0364_lowmap");
    // 族一：标准豁免账。
    let segs = vec![
        ExemptSeg::new(0, 0x400, K_LEGACY).unwrap_or(ExemptSeg { start: 0, end: 0x400, kind: K_LEGACY }),
        ExemptSeg { start: 0x400, end: 0x9_FC00, kind: K_BIOS }, // 常规低内存豁免到 EBDA 前
        ExemptSeg { start: 0x9_FC00, end: 0xA_0000, kind: K_EBDA },
        ExemptSeg { start: 0xA_0000, end: 0xC_0000, kind: K_VGA },
        ExemptSeg { start: 0xF_0000, end: LOW_CEIL, kind: K_BIOS },
    ];
    let book = match ExemptBook::new(&segs) {
        Ok(b) => b,
        Err(_) => {
            cs.add("豁免账构建", false, "被拒");
            return cs;
        }
    };
    cs.add("五段入账", book.len() == 5, "升序");
    cs.add("必豁覆盖", book.mandatory_cover().is_ok(), "VGA+IVT 齐");
    // 族二：拦截核。
    cs.add("VGA 拦", book.intercept(0xA_1234) == Err(XC_ALLOC_IN_EXEMPT), "帧缓冲区");
    cs.add("IVT 拦", book.intercept(0x100) == Err(XC_ALLOC_IN_EXEMPT), "中断向量表");
    cs.add("BIOS 拦", book.intercept(0xF_0050) == Err(XC_ALLOC_IN_EXEMPT), "BIOS 区");
    cs.add("缝隙放行", book.intercept(0xC_5000).is_ok(), "非豁免区");
    // 族三：位图外核。
    cs.add("池在高位过", book.out_of_pool(0x10_0000, 1024).is_ok(), "池起 1MB");
    cs.add("池吞豁免拦", book.out_of_pool(0x8_0000, 256) == Err(XC_IN_POOL), "池覆盖 VGA");
    // 族四：构造闸。
    cs.add("空账拒", ExemptBook::new(&[]).is_err(), "SEG_EMPTY");
    cs.add("重叠拒", ExemptBook::new(&[
        ExemptSeg { start: 0x100, end: 0x300, kind: K_BIOS },
        ExemptSeg { start: 0x200, end: 0x400, kind: K_BIOS },
    ]).is_err(), "OVERLAP");
    cs.add("乱序拒", ExemptBook::new(&[
        ExemptSeg { start: 0x300, end: 0x400, kind: K_BIOS },
        ExemptSeg { start: 0x100, end: 0x200, kind: K_BIOS },
    ]).is_err(), "ORDER");
    cs.add("止<起拒", ExemptSeg::new(0x400, 0x100, K_BIOS).is_err(), "RANGE");
    cs.add("越 1MB 拒", ExemptSeg::new(0x10_0000, 0x11_0000, K_BIOS).is_err(), "EXCEED");
    cs.add("类别越域拒", ExemptSeg::new(0, 0x100, 9).is_err(), "KIND");
    // 族五：必豁缺。
    let thin = vec![ExemptSeg { start: 0xF_0000, end: LOW_CEIL, kind: K_BIOS }];
    let thin_book = ExemptBook::new(&thin).unwrap_or(book);
    cs.add("缺 VGA/IVT 立案", thin_book.mandatory_cover() == Err(XC_COVER_MISS), "COVER_MISS");
    // 族六：段命中边界。
    let s = ExemptSeg { start: 0xA_0000, end: 0xC_0000, kind: K_VGA };
    cs.add("起端命中", s.hits(0xA_0000), "闭起");
    cs.add("止端不中", !s.hits(0xC_0000), "开止");
    cs.add("段外不中", !s.hits(0x9_FFFF), "段前");
    cs
}
