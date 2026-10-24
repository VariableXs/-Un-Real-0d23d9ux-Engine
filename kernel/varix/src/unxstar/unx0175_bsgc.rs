//! unx0175_bsgc — UNX-F0175 EFI 内存垃圾回收（boot services 区回收）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0175-J1）：退出引导服务后 BS 区并入
//! PMM 可用账，回收量与内存图声明一致。
//!
//! 职责定位：ExitBootServices 之后，Boot Services 代码/数据区从"固件专
//! 用"转为"内核可用"——回收不及时是纯浪费，回收多了会踩固件仍在用的
//! 区。本模块三账守恒：①内存图声明账（BS 区各段的类型与页数——回收量
//! 的声明真源）；②回收账（并入 PMM 的页数——只许 ≤ 声明，逐段登记）；
//! ③守恒对账（回收量 ≡ 声明中标记为可回收的段之和——一致即判据兑现）。
//! no_std + alloc；零 unwrap 零 expect。
//! 诊断码段 0x6550（本会话独占，grep 全库零占用后领取，2026-10-22）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const GC_VERSION: &str = "B175-bsgc-v1";

// ── 诊断码（0x655 段，本会话独占）──
/// 回收超声明（回收量 > 声明可回收量——踩固件在用区的风险路径）。
pub const XC_GC_OVER_RECLAIM: u16 = 0x6550;
/// 重复回收（同段两次并入 PMM——双计）。
pub const XC_GC_DUP_RECLAIM: u16 = 0x6551;
/// 段类型非法（不在内存类型闭集）。
pub const XC_GC_BAD_TYPE: u16 = 0x6552;
/// 守恒对账失败（回收量 ≠ 声明可回收和——判据直接违约）。
pub const XC_GC_UNBALANCED: u16 = 0x6553;
/// 页数非法（0 页或超段界）。
pub const XC_GC_BAD_PAGES: u16 = 0x6554;

/// 内存段类型闭集（EFI_MEMORY_TYPE 子集，回收视角归组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemZone {
    /// BS 代码区（EBS 后可回收）。
    BsCode,
    /// BS 数据区（EBS 后可回收）。
    BsData,
    /// BS 服务区（EBS 后可回收）。
    BsSndData,
    /// 运行时区（永不可回收）。
    Runtime,
    /// 保留区（永不可回收）。
    Reserved,
}

impl MemZone {
    /// 该类型是否 EBS 后可回收。
    pub fn reclaimable(self) -> bool {
        matches!(self, MemZone::BsCode | MemZone::BsData | MemZone::BsSndData)
    }
    /// 类型名。
    pub fn name(self) -> &'static str {
        match self {
            MemZone::BsCode => "BS_CODE",
            MemZone::BsData => "BS_DATA",
            MemZone::BsSndData => "BS_SND",
            MemZone::Runtime => "RUNTIME",
            MemZone::Reserved => "RESERVED",
        }
    }
}

/// 内存图声明段。
#[derive(Clone, Copy, Debug)]
pub struct MemSegment {
    /// 段类型。
    pub zone: MemZone,
    /// 页数（4KiB 页）。
    pub pages: u64,
    /// 是否已在回收账。
    pub reclaimed: bool,
}

/// BS 区回收账。
#[derive(Debug)]
pub struct BsGcLedger {
    /// 内存图声明账。
    pub segments: Vec<MemSegment>,
    /// 回收明细（段索引 → 回收页数）。
    pub reclaimed: Vec<(usize, u64)>,
    /// EBS 是否已过（回收前置条件）。
    ebs_done: bool,
}

impl BsGcLedger {
    pub fn boot() -> Self {
        BsGcLedger { segments: Vec::new(), reclaimed: Vec::new(), ebs_done: false }
    }

    /// EBS 完成（回收的门闸——EBS 前一律拒收）。
    pub fn mark_ebs(&mut self) {
        self.ebs_done = true;
    }

    /// 登记内存图声明段。
    pub fn declare(&mut self, zone: MemZone, pages: u64) -> Result<usize, u16> {
        if pages == 0 || pages > 0x0100_0000 {
            return Err(XC_GC_BAD_PAGES);
        }
        self.segments.push(MemSegment { zone, pages, reclaimed: false });
        Ok(self.segments.len() - 1)
    }

    /// 回收一段（并入 PMM 可用账）：只许可回收类型、只许一次、
    /// EBS 后才许做。
    pub fn reclaim(&mut self, seg_idx: usize) -> Result<u64, u16> {
        if !self.ebs_done {
            return Err(XC_GC_OVER_RECLAIM);
        }
        let (reclaimable, dup, pages) = match self.segments.get(seg_idx) {
            Some(s) => (s.zone.reclaimable(), s.reclaimed, s.pages),
            None => return Err(XC_GC_BAD_PAGES),
        };
        if !reclaimable {
            return Err(XC_GC_OVER_RECLAIM);
        }
        if dup {
            return Err(XC_GC_DUP_RECLAIM);
        }
        self.segments[seg_idx].reclaimed = true;
        self.reclaimed.push((seg_idx, pages));
        Ok(pages)
    }

    /// 守恒对账（判据）：回收总量 ≡ 声明中可回收段的页数和。
    pub fn conservation(&self) -> Result<u64, u16> {
        let declared: u64 = self.segments.iter().filter(|s| s.zone.reclaimable()).map(|s| s.pages).fold(0u64, |a, b| a.saturating_add(b));
        let reclaimed: u64 = self.reclaimed.iter().map(|(_, p)| *p).fold(0u64, |a, b| a.saturating_add(b));
        if reclaimed > declared {
            return Err(XC_GC_OVER_RECLAIM);
        }
        // 全量回收语义：EBS 后账内所有可回收段都应回收完（回收量一致）。
        if self.reclaimed.len() == self.segments.iter().filter(|s| s.zone.reclaimable()).count() {
            if reclaimed != declared {
                return Err(XC_GC_UNBALANCED);
            }
        }
        Ok(reclaimed)
    }

    /// 不可回收段守住（红线：Runtime/Reserved 零回收）。
    pub fn no_forbidden_reclaim(&self) -> bool {
        self.reclaimed.iter().all(|(i, _)| self.segments[*i].zone.reclaimable())
    }

    /// 回收总量读数。
    pub fn reclaimed_total(&self) -> u64 {
        self.reclaimed.iter().map(|(_, p)| *p).fold(0u64, |a, b| a.saturating_add(b))
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_GC_OVER_RECLAIM => "回收越界：只许回收 BS 三区且 EBS 后进行——Runtime/Reserved 永不回收。".to_string(),
            XC_GC_DUP_RECLAIM => "重复回收：同段只许并入 PMM 一次。".to_string(),
            XC_GC_BAD_TYPE => "段类型非法：查内存类型闭集。".to_string(),
            XC_GC_UNBALANCED => "守恒对账失败：回收量 ≠ 声明可回收和——先对内存图再宣告回收。".to_string(),
            XC_GC_BAD_PAGES => "页数非法：1 页起、段界内。".to_string(),
            _ => "未知诊断码：查 0x655 段分配表。".to_string(),
        }
    }
}

/// 自证：回收量与声明一致 + 红线对拍。
pub fn self_audit() -> Result<(), u16> {
    let mut lg = BsGcLedger::boot();
    // 内存图：BS 三区 64+128+16 页，Runtime 32 页，Reserved 8 页。
    let i0 = lg.declare(MemZone::BsCode, 64)?;
    let i1 = lg.declare(MemZone::BsData, 128)?;
    let i2 = lg.declare(MemZone::BsSndData, 16)?;
    lg.declare(MemZone::Runtime, 32)?;
    lg.declare(MemZone::Reserved, 8)?;
    // EBS 前回收 → 拒。
    if lg.reclaim(i0).is_ok() {
        return Err(XC_GC_OVER_RECLAIM);
    }
    lg.mark_ebs();
    // 逐段回收 BS 三区。
    let r0 = lg.reclaim(i0)?;
    let r1 = lg.reclaim(i1)?;
    let r2 = lg.reclaim(i2)?;
    if r0 != 64 || r1 != 128 || r2 != 16 {
        return Err(XC_GC_BAD_PAGES);
    }
    // 重复回收拒。
    if lg.reclaim(i0).is_ok() {
        return Err(XC_GC_DUP_RECLAIM);
    }
    // 守恒：64+128+16 = 208。
    let total = lg.conservation()?;
    if total != 208 || lg.reclaimed_total() != 208 {
        return Err(XC_GC_UNBALANCED);
    }
    // 红线：Runtime/Reserved 零回收。
    if !lg.no_forbidden_reclaim() {
        return Err(XC_GC_OVER_RECLAIM);
    }
    // 部分回收：守恒不报错但总量小于声明（诚实读数）。
    let mut lg2 = BsGcLedger::boot();
    lg2.mark_ebs();
    let j0 = lg2.declare(MemZone::BsCode, 10)?;
    lg2.declare(MemZone::BsData, 20)?;
    let t2 = lg2.reclaim(j0)?;
    if t2 != 10 || lg2.conservation()? != 10 {
        return Err(XC_GC_UNBALANCED);
    }
    Ok(())
}
