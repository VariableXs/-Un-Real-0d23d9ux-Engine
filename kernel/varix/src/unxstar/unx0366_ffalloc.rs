//! unx0366_ffalloc —— UNX-F0366 早年页帧分配器（first-fit 位图扫描）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0366 · first-fit 分配器 · 目标 260 行
//! 判据原文：UNX-F0366-J1 早年分配器（页帧池上第一层）：first-fit
//!           位图扫描分配连续 n 帧；分配即记账（请求方 + 帧区间）；
//!           释放即销账；账目不平（分配 > 容量）当场立案；对齐
//!           类分配（2/4/8 帧对齐窗）支持。
//! 组件：分配器（位图 + 账行）+ first-fit 扫描（对齐窗可选）+
//!       分配账（借行/还行 + 平账核）+ 统计（峰值/存量）。
//! 职责定位：早年分配器是「内存管理的学步期」——没有伙伴系统、
//!   没有slab，只有位图和 first-fit；但记账纪律不能少：每笔
//!   分配谁借的、还了没有必须可对账，否则早年泄漏会在内存
//!   管理成熟后变成永远查不清的历史悬案。
//! 性能逐项分解：分配 O(帧数)；释放 O(1)；平账 O(账行数)。
//! 诊断码段：0x20800（0x20801~0x20809 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FA_VERSION: &str = "unx0366_ffalloc-v1";

/// 诊断段基址（0x20800 段 · 本模块独占）。
pub const FA_SEG_BASE: u32 = 0x20800;
pub const XC_EXHAUST: u32 = 0x20801; // 无连续区
pub const XC_ALIGN_BAD: u32 = 0x20802; // 对齐类越域
pub const XC_ZERO_REQ: u32 = 0x20803; // 零帧请求
pub const XC_OWNER_EMPTY: u32 = 0x20804; // 空请求方
pub const XC_NOT_OWNER: u32 = 0x20805; // 非持有方释放
pub const XC_LEDGER_UNBAL: u32 = 0x20806; // 账目不平
pub const XC_OVERALLOC: u32 = 0x20807; // 分配超容量
pub const XC_FRAME_FREE: u32 = 0x20808; // 释放未持有帧
pub const XC_DUP_OWNER: u32 = 0x20809; // 同方同区间双记

/// 页大小（字节）。
pub const PAGE: u32 = 4096;

/// 分配账行。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AllocRow {
    pub owner: u32,
    pub start: u32,
    pub frames: u32,
}

/// 早年分配器。
pub struct EarlyAlloc {
    /// 帧占用位图。
    used: Vec<bool>,
    total: u32,
    ledger: Vec<AllocRow>,
    peak: u32,
}

impl EarlyAlloc {
    pub fn new(total: u32) -> Result<EarlyAlloc, u32> {
        if total == 0 || total > 4096 {
            return Err(XC_OVERALLOC);
        }
        Ok(EarlyAlloc { used: vec![false; total as usize], total, ledger: Vec::new(), peak: 0 })
    }

    /// 已用帧数。
    pub fn used_frames(&self) -> u32 {
        self.used.iter().filter(|&&b| b).count() as u32
    }

    /// 分配账深度。
    pub fn ledger_len(&self) -> u32 {
        self.ledger.len() as u32
    }

    /// 峰值。
    pub fn peak(&self) -> u32 {
        self.peak
    }

    /// first-fit 分配：align_class ∈ {1,2,4,8}（帧对齐类）。
    pub fn alloc(&mut self, owner: u32, frames: u32, align_class: u32) -> Result<u32, u32> {
        if frames == 0 {
            return Err(XC_ZERO_REQ);
        }
        if !matches!(align_class, 1 | 2 | 4 | 8) {
            return Err(XC_ALIGN_BAD);
        }
        if owner == 0 {
            return Err(XC_OWNER_EMPTY);
        }
        // 容量闸：分配总量不许超池。
        if self.used_frames() + frames > self.total {
            return Err(XC_OVERALLOC);
        }
        // first-fit 扫描：起始帧对齐 align_class，区间整体空闲即取。
        let mut i: u32 = 0;
        while i + frames <= self.total {
            if i % align_class == 0 {
                let lo = i as usize;
                let hi = (i + frames) as usize;
                if self.used[lo..hi].iter().all(|&b| !b) {
                    for slot in self.used.iter_mut().take(hi).skip(lo) {
                        *slot = true;
                    }
                    let start = i;
                    // 账行（同方同区间双记拒）。
                    if self.ledger.iter().any(|r| r.owner == owner && r.start == start && r.frames == frames) {
                        return Err(XC_DUP_OWNER);
                    }
                    self.ledger.push(AllocRow { owner, start, frames });
                    let used = self.used_frames();
                    if used > self.peak {
                        self.peak = used;
                    }
                    return Ok(start * PAGE);
                }
            }
            i += 1;
        }
        Err(XC_EXHAUST)
    }

    /// 释放：按（owner, start）销账；未持有帧拒。
    pub fn free(&mut self, owner: u32, addr: u32) -> Result<u32, u32> {
        if owner == 0 {
            return Err(XC_OWNER_EMPTY);
        }
        if addr % PAGE != 0 {
            return Err(XC_NOT_OWNER);
        }
        let start = addr / PAGE;
        let row = self
            .ledger
            .iter()
            .position(|r| r.owner == owner && r.start == start)
            .ok_or(XC_NOT_OWNER)?;
        let row_v = self.ledger.remove(row);
        for slot in self.used.iter_mut().take((row_v.start + row_v.frames) as usize).skip(row_v.start as usize) {
            *slot = false;
        }
        Ok(row_v.frames)
    }

    /// 平账核：账行帧数和 == 位图占用数。
    pub fn reconcile(&self) -> Result<(), u32> {
        let ledger_sum: u32 = self.ledger.iter().map(|r| r.frames).sum();
        if ledger_sum != self.used_frames() {
            return Err(XC_LEDGER_UNBAL);
        }
        Ok(())
    }
}

pub fn run_unx0366_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0366_ffalloc");
    // 族一：first-fit 基本分配。
    let mut al = match EarlyAlloc::new(64) {
        Ok(a) => a,
        Err(_) => {
            cs.add("分配器构建", false, "被拒");
            return cs;
        }
    };
    let a1 = al.alloc(1, 2, 1);
    cs.add("2 帧分配", matches!(a1, Ok(0)), "首位帧 0");
    let a2 = al.alloc(2, 4, 1);
    cs.add("4 帧分配", matches!(a2, Ok(0x2000)), "帧 2 起");
    cs.add("账深 2", al.ledger_len() == 2, "两笔");
    cs.add("平账", al.reconcile().is_ok(), "6 帧=6 帧");
    // 族二：对齐类。
    let mut al2 = EarlyAlloc::new(32).unwrap_or(EarlyAlloc { used: vec![false; 32], total: 32, ledger: Vec::new(), peak: 0 });
    cs.add("4 对齐窗", matches!(al2.alloc(5, 2, 4), Ok(0)), "帧 0 对齐 4");
    let _ = al2.alloc(5, 1, 1);
    let a3 = al2.alloc(6, 2, 4);
    cs.add("跳到帧 4", matches!(a3, Ok(0x4000)), "对齐窗跳空");
    cs.add("对齐类越域拒", al2.alloc(6, 1, 3).is_err(), "ALIGN_BAD");
    cs.add("对齐类 16 拒", al2.alloc(6, 1, 16).is_err(), "ALIGN_BAD");
    // 族三：容量与耗尽。
    let mut al3 = EarlyAlloc::new(8).unwrap_or(EarlyAlloc { used: vec![false; 8], total: 8, ledger: Vec::new(), peak: 0 });
    let _ = al3.alloc(7, 8, 1);
    cs.add("满池分配拒", al3.alloc(7, 1, 1) == Err(XC_OVERALLOC), "OVERALLOC");
    cs.add("超容拒", al3.alloc(7, 9, 1) == Err(XC_OVERALLOC), "9>8");
    cs.add("零帧拒", al3.alloc(7, 0, 1).is_err(), "ZERO");
    // 无连续区（碎片态）。
    let mut al4 = EarlyAlloc::new(8).unwrap_or(EarlyAlloc { used: vec![false; 8], total: 8, ledger: Vec::new(), peak: 0 });
    let _ = al4.alloc(1, 1, 1); // 0
    let _ = al4.alloc(1, 1, 1); // 1
    let _ = al4.free(1, 0);
    let _ = al4.alloc(1, 1, 1); // 0
    let _ = al4.alloc(2, 1, 1); // 2
    let _ = al4.alloc(2, 1, 1); // 3
    let _ = al4.alloc(2, 1, 1); // 4
    let _ = al4.alloc(2, 1, 1); // 5
    let _ = al4.alloc(2, 1, 1); // 6
    let _ = al4.alloc(2, 1, 1); // 7
    let _ = al4.free(2, 2 * PAGE);
    let _ = al4.free(2, 4 * PAGE);
    cs.add("碎片无连续", al4.alloc(9, 3, 1) == Err(XC_EXHAUST), "2 个单帧空穴");
    // 族四：释放与账。
    let f1 = al.free(2, 0x2000);
    cs.add("释放 4 帧", f1 == Ok(4), "销账");
    cs.add("释放后平账", al.reconcile().is_ok(), "2 帧=2 帧");
    cs.add("非持有方释放拒", al.free(9, 0) == Err(XC_NOT_OWNER), "NOT_OWNER");
    cs.add("未分配地址释放拒", al.free(1, 0x8000).is_err(), "NOT_OWNER");
    cs.add("未对齐地址拒", al.free(1, 0x1004).is_err(), "NOT_OWNER");
    cs.add("空方拒", al.alloc(0, 1, 1).is_err(), "OWNER_EMPTY");
    // 族五：峰值统计。
    let mut al5 = EarlyAlloc::new(16).unwrap_or(EarlyAlloc { used: vec![false; 16], total: 16, ledger: Vec::new(), peak: 0 });
    let _ = al5.alloc(3, 8, 1);
    let _ = al5.alloc(3, 4, 1);
    cs.add("峰值 12", al5.peak() == 12, "8+4");
    let _ = al5.free(3, 8 * PAGE);
    cs.add("峰值保持", al5.peak() == 12, "峰值不减");
    cs.add("存量 8", al5.used_frames() == 8, "释放后");
    cs
}
