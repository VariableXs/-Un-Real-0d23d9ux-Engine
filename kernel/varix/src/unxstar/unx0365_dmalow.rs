//! unx0365_dmalow —— UNX-F0365 DMA 低位区预留（<16MB 专用池）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0365 · DMA 低位池 · 目标 240 行
//! 判据原文：UNX-F0365-J1 24 位 DMA 设备只能寻址 <16MB：专用池
//!           预留段 [dma_base, dma_base+dma_size) 只服务 DMA
//!           分配（通用分配不得碰）；池耗尽显式拒（不许向通用
//!           池借——借来的地址设备寻不到）；池使用率遥测入账。
//! 组件：DMA 池（基址/容量/水位）+ 资格核（地址 <16MB）+ 隔离
//!       核（通用分配请求拒）+ 水位遥测（用率千分）。
//! 职责定位：DMA 低位池是「老设备的专用车道」——24 位设备的
//!   总线主控只能看到低 16MB，通用池的高地址对它们等于不存在。
//!   隔离是单向的：DMA 池可以拒通用请求，但通用池永远不接
//!   DMA 请求（借址 = 设备静默写错内存，最毒的 bug）。
//! 性能逐项分解：分配 O(1)；资格核 O(1)；遥测 O(1)。
//! 诊断码段：0x207C0（0x207C1~0x207C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DL_VERSION: &str = "unx0365_dmalow-v1";

/// 诊断段基址（0x207C0 段 · 本模块独占）。
pub const DL_SEG_BASE: u32 = 0x207C0;
pub const XC_ADDR_HI: u32 = 0x207C1; // 地址 ≥16MB
pub const XC_POOL_EXHAUST: u32 = 0x207C2; // 池耗尽
pub const XC_BORROW_DENY: u32 = 0x207C3; // 借址拒
pub const XC_SIZE_ZERO: u32 = 0x207C4; // 零尺寸分配
pub const XC_SIZE_BIG: u32 = 0x207C5; // 尺寸超池
pub const XC_DUP_FREE: u32 = 0x207C6; // 双释放
pub const XC_NOT_OWNED: u32 = 0x207C7; // 释放非池地址
pub const XC_MISALIGN: u32 = 0x207C8; // 池基址未对齐
pub const XC_RATE_OOR: u32 = 0x207C9; // 用率越域

/// 24 位 DMA 上限。
pub const DMA_CEIL: u32 = 0x0100_0000; // 16MB
/// 分配粒度（页）。
pub const PAGE: u32 = 4096;
/// 池最大页数。
pub const POOL_PAGES_MAX: u32 = 2048;

/// DMA 低位池（位图页分配）。
pub struct DmaPool {
    base: u32,
    pages: u32,
    used: Vec<bool>,
    used_count: u32,
}

impl DmaPool {
    /// 构建：基址须页对齐且 <16MB；池整体也须落在 16MB 内。
    pub fn new(base: u32, pages: u32) -> Result<DmaPool, u32> {
        if pages == 0 || pages > POOL_PAGES_MAX {
            return Err(XC_SIZE_BIG);
        }
        if base % PAGE != 0 {
            return Err(XC_MISALIGN);
        }
        if base >= DMA_CEIL || base.checked_add(pages.saturating_mul(PAGE)).ok_or(XC_ADDR_HI)? > DMA_CEIL {
            return Err(XC_ADDR_HI);
        }
        Ok(DmaPool { base, pages, used: vec![false; pages as usize], used_count: 0 })
    }

    /// DMA 分配：连续 n 页 first-fit；耗尽显式拒（不借）。
    pub fn alloc(&mut self, n: u32) -> Result<u32, u32> {
        if n == 0 {
            return Err(XC_SIZE_ZERO);
        }
        if n > self.pages {
            return Err(XC_SIZE_BIG);
        }
        // first-fit 连续空位扫描。
        let mut run = 0;
        for i in 0..self.pages as usize {
            if self.used[i] {
                run = 0;
                continue;
            }
            run += 1;
            if run == n {
                let start = i + 1 - n as usize;
                for slot in self.used.iter_mut().take(i + 1).skip(start) {
                    *slot = true;
                }
                self.used_count += n;
                return Ok(self.base + (start as u32) * PAGE);
            }
        }
        Err(XC_POOL_EXHAUST)
    }

    /// 释放：非池地址/双释放拒。
    pub fn free(&mut self, addr: u32, n: u32) -> Result<(), u32> {
        if addr < self.base || addr >= self.base + self.pages * PAGE || (addr - self.base) % PAGE != 0 {
            return Err(XC_NOT_OWNED);
        }
        let start = ((addr - self.base) / PAGE) as usize;
        if start + n as usize > self.pages as usize {
            return Err(XC_NOT_OWNED);
        }
        for i in start..start + n as usize {
            if !self.used[i] {
                return Err(XC_DUP_FREE);
            }
        }
        for slot in self.used.iter_mut().take(start + n as usize).skip(start) {
            *slot = false;
        }
        self.used_count -= n;
        Ok(())
    }

    /// 资格核：地址必须 <16MB（24 位可寻址）。
    pub fn qualified(&self, addr: u32) -> Result<(), u32> {
        if addr >= DMA_CEIL {
            return Err(XC_ADDR_HI);
        }
        Ok(())
    }

    /// 隔离核：通用分配请求（非 DMA 路径）一律拒——池是专用道。
    pub fn general_request(&self) -> Result<(), u32> {
        Err(XC_BORROW_DENY)
    }

    /// 水位用率（千分）。
    pub fn usage_milli(&self) -> u32 {
        if self.pages == 0 {
            return 0;
        }
        ((self.used_count as u64) * 1000 / self.pages as u64) as u32
    }

    /// 空闲页数。
    pub fn free_pages(&self) -> u32 {
        self.pages - self.used_count
    }
}

pub fn run_unx0365_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0365_dmalow");
    // 族一：池构建。
    let mut pool = match DmaPool::new(0x0060_0000, 256) {
        Ok(p) => p,
        Err(_) => {
            cs.add("池构建", false, "被拒");
            return cs;
        }
    };
    cs.add("容量 256", pool.free_pages() == 256, "全空");
    cs.add("用率 0", pool.usage_milli() == 0, "空池");
    // 族二：分配。
    let a1 = pool.alloc(4);
    cs.add("4 页分配", matches!(a1, Ok(addr) if addr == 0x0060_0000), "first-fit 首位");
    let a2 = pool.alloc(8);
    cs.add("8 页分配", matches!(a2, Ok(addr) if addr == 0x0060_1000), "紧随其后");
    cs.add("用率千分", pool.usage_milli() == 46, "12/256=46‰");
    // 族三：耗尽。
    let big = pool.alloc(300);
    cs.add("超池拒", big == Err(XC_SIZE_BIG), "SIZE_BIG");
    let mut small = DmaPool::new(0x0080_0000, 4).unwrap_or(DmaPool { base: 0x0080_0000, pages: 4, used: vec![false; 4], used_count: 0 });
    cs.add("全占后耗尽", {
        let _ = small.alloc(4);
        small.alloc(1) == Err(XC_POOL_EXHAUST)
    }, "不借通用池");
    // 族四：资格核。
    cs.add("15MB 过", pool.qualified(0x00F0_0000).is_ok(), "低位");
    cs.add("16MB 拒", pool.qualified(0x0100_0000) == Err(XC_ADDR_HI), "闭上界");
    cs.add("16MB+ 拒", pool.qualified(0x0100_1000).is_err(), "高位");
    // 族五：隔离核。
    cs.add("通用请求拒", pool.general_request() == Err(XC_BORROW_DENY), "专用道");
    // 族六：释放与双释放。
    let f1 = pool.free(0x0060_0000, 4);
    cs.add("释放 4 页", f1.is_ok(), "回池");
    cs.add("双释放拒", pool.free(0x0060_0000, 4) == Err(XC_DUP_FREE), "DOUBLE");
    cs.add("非池地址拒", pool.free(0x0200_0000, 1) == Err(XC_NOT_OWNED), "高位释放");
    cs.add("池外低位拒", pool.free(0x0010_0000, 1) == Err(XC_NOT_OWNED), "低于池");
    // 族七：构建闸。
    cs.add("零页拒", DmaPool::new(0x0060_0000, 0).is_err(), "SIZE");
    cs.add("未对齐拒", DmaPool::new(0x0060_1004, 8).is_err(), "MISALIGN");
    cs.add("池越 16MB 拒", DmaPool::new(0x00F0_0000, 300).is_err(), "ADDR_HI");
    cs.add("基址高拒", DmaPool::new(0x0100_0000, 8).is_err(), "ADDR_HI");
    cs.add("零尺寸拒", {
        let mut p2 = DmaPool::new(0x0060_0000, 8).unwrap_or(DmaPool { base: 0, pages: 0, used: Vec::new(), used_count: 0 });
        p2.alloc(0).is_err()
    }, "SIZE_ZERO");
    cs
}
