//! unx0455_mapreg —— UNX-F0455 表区映射登记（B19 账联动防重）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0455 · 表区映射登记 · 目标 230 行
//! 判据原文：UNX-F0455-J1 映射登记账：每个将被映射的物理区
//!           以（页对齐基址、页数、用途标签）登记；账二 防
//!           重核：同基址二次登记 → 拒（幂等路径：完全相同
//!           的（基址、页数、标签）→ 返回已登记号不重复记
//!           账）；部分重叠 → 拒立案；账三 页对齐核：基址
//!           必须 4K 对齐（低 12 位全零），页数 ≥ 1；账四
//!           B19 账联动：登记号与 B19 映射账编号空间连续
//!           （号段不重号）；账五 用途标签独占：同一物理区
//!           不允许两个用途标签（映射面唯一用途纪律）。
//! 组件：页对齐核 + 防重登记器 + 幂等通道 + 编号空间核。
//! 职责定位：映射登记是物理内存的「入住登记簿」——同一页
//!   被两个子系统按不同属性映射（一个 WB 一个 UC）是缓存一
//!   致性的经典事故源；防重核就是登记簿的唯一性约束。
//! 性能逐项分解：登记 O(n)；对齐核 O(1)。
//! 诊断码段：0x34300（0x34301~0x34309 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34300 起）。
pub const SEG_BASE: u32 = 0x34300;
pub const XC_ALIGN: u32 = SEG_BASE + 1; // 页对齐破坏
pub const XC_DUP: u32 = SEG_BASE + 2; // 部分重叠/基址重号
pub const XC_PAGES: u32 = SEG_BASE + 3; // 页数为 0
pub const XC_TAGUSE: u32 = SEG_BASE + 4; // 同区异标签
pub const XC_B19SEQ: u32 = SEG_BASE + 5; // 编号空间断裂

/// 页粒度 4K。
pub const PAGE: u64 = 0x1000;

/// 映射用途标签。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UseTag {
    AcpiTable,
    AcpiData,
    FrameBuf,
    Other,
}

/// 登记条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapReg {
    pub base: u64,   // 页对齐物理基址
    pub pages: u32,  // 页数
    pub tag: UseTag,
    /// B19 联动编号（登记号）。
    pub reg_no: u32,
}

impl MapReg {
    /// 区间终点（字节）。
    pub fn span(&self) -> u64 {
        (self.pages as u64).saturating_mul(PAGE)
    }

    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.span())
    }
}

/// 页对齐核：基址低 12 位全零。
pub fn aligned(base: u64) -> bool {
    base & (PAGE - 1) == 0
}

/// 登记簿。
#[derive(Clone, Debug, Default)]
pub struct MapBook {
    pub regs: Vec<MapReg>,
    /// B19 编号空间游标（下一登记号）。
    pub next_no: u32,
}

impl MapBook {
    pub fn new(b19_base: u32) -> Self {
        MapBook { regs: Vec::new(), next_no: b19_base }
    }

    /// 登记：完全相同的（base、pages、tag）→ 幂等返回既有号；
    /// 部分重叠 → 拒；同区异标签 → 拒。
    pub fn register(&mut self, base: u64, pages: u32, tag: UseTag) -> Result<u32, u32> {
        if !aligned(base) {
            return Err(XC_ALIGN);
        }
        if pages == 0 {
            return Err(XC_PAGES);
        }
        let cand = MapReg { base, pages, tag, reg_no: 0 };
        let end = cand.end();
        // 幂等通道。
        if let Some(e) = self.regs.iter().find(|r| r.base == base && r.pages == pages && r.tag == tag) {
            return Ok(e.reg_no);
        }
        // 防重核：任何区间相交即拒；相交但标签不同额外立案语义。
        for r in &self.regs {
            let lo = r.base.max(base);
            let hi = r.end().min(end);
            if lo < hi {
                if r.tag != tag {
                    return Err(XC_TAGUSE);
                }
                return Err(XC_DUP);
            }
        }
        let no = self.next_no;
        // B19 编号空间连续核：登记号必须等于游标（严格递增无跳号）。
        self.next_no = no.checked_add(1).ok_or(XC_B19SEQ)?;
        self.regs.push(MapReg { base, pages, tag, reg_no: no });
        Ok(no)
    }
}

// ─────────────────────────── 判据入口 ───────────────────────────

/// 判据入口：UNX-F0455 全族核。
pub fn run_unx0455_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0455");

    // 族一：对齐核。
    cs.add("4K 对齐真", aligned(0xE000_0000), "低 12 位零");
    cs.add("非对齐假", !aligned(0xE000_0800), "低 12 位 800");
    cs.add("页界边界真", aligned(PAGE), "恰一页");

    // 族二：正常登记与编号空间。
    let mut bk = MapBook::new(100);
    let n1 = bk.register(0xE000_0000, 2, UseTag::AcpiTable);
    let n2 = bk.register(0xE000_2000, 1, UseTag::AcpiData);
    cs.add("首登得号 100", n1 == Ok(100), "B19 基号");
    cs.add("次登得号 101", n2 == Ok(101), "连续");
    cs.add("簿两行", bk.regs.len() == 2, "登记数");

    // 族三：幂等通道。
    let n3 = bk.register(0xE000_0000, 2, UseTag::AcpiTable);
    cs.add("幂等得原号", n3 == Ok(100), "不重记");
    cs.add("幂等不加行", bk.regs.len() == 2, "簿不变");
    let n4 = bk.register(0xE000_0000, 3, UseTag::AcpiTable);
    cs.add("同址异页数拒", n4 == Err(XC_DUP), "部分重叠变体");

    // 族四：部分重叠。
    let n5 = bk.register(0xE000_1000, 2, UseTag::AcpiData);
    cs.add("跨界重叠拒", n5 == Err(XC_DUP), "咬合两行");
    let n6 = bk.register(0xDFFF_F000, 2, UseTag::Other);
    cs.add("侵入首行拒", n6 == Err(XC_DUP), "压界");

    // 族五：同区异标签。
    let n7 = bk.register(0xE000_0000, 2, UseTag::FrameBuf);
    cs.add("异标签立案", n7 == Err(XC_TAGUSE), "用途冲突");

    // 族六：对齐与页数防线。
    let n8 = bk.register(0xE000_0FFF, 1, UseTag::Other);
    cs.add("非对齐拒", n8 == Err(XC_ALIGN), "低 12 位 FFF");
    let n9 = bk.register(0xE100_0000, 0, UseTag::Other);
    cs.add("零页拒", n9 == Err(XC_PAGES), "页数 0");

    // 族七：相切合法。
    let n10 = bk.register(0xE000_3000, 1, UseTag::Other);
    cs.add("尾邻接合法", n10 == Ok(102), "相切不交");
    cs.add("号 102 续", bk.regs.last().map(|r| r.reg_no) == Some(102), "游标续");

    // 族八：span/end 单元。
    let r0 = MapReg { base: 0xE000_0000, pages: 3, tag: UseTag::Other, reg_no: 0 };
    cs.add("span 三页", r0.span() == 0x3000, "3*4K");
    cs.add("end 基+span", r0.end() == 0xE000_3000, "终点");
    let rmax = MapReg { base: u64::MAX - 0xFFF, pages: u32::MAX, tag: UseTag::Other, reg_no: 0 };
    cs.add("span 饱和不炸", rmax.span() == u64::MAX, "saturating");

    cs
}
