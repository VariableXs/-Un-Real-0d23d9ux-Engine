//! F021 深化批次二 · 堆句柄族与内存状态面（compatstar2/deep · G-A-21）。
//!
//! 批次一深化覆盖 VirtualProtect/DECOMMIT/重提交/守护页；本批补齐主册
//! 【功能定义】「全语义对齐」的堆侧出口：HeapAlloc 族（16 字节对齐/重分配/
//! 双重释放检出）、GlobalMemoryStatusEx 内存状态四元组（诊断中心内存面板
//! 的数据源）、VirtualLock/VirtualUnlock 工作集锁定、守护页异常的 NTSTATUS
//! 分类码（F020 异常流程的分类入口）。
//!
//! 判据对账：深化以主册【设计细节】/【状态与异常】未落地面为源，一处一事实
//! （MS HeapAlloc/GlobalMemoryStatusEx/VirtualLock 文档语义对拍）。
//!
//! 零堆纪律：定长块表，无 alloc。

use crate::checks::CheckSet;

/// 堆分配对齐粒度 16 字节（MS HeapAlloc 语义：默认对齐）。
pub const HEAP_GRAIN: usize = 16;
/// 堆块表容量（域内模型口径）。
pub const MAX_HEAP_BLOCKS: usize = 32;
/// 守护页异常 NTSTATUS 码（MS STATUS_GUARD_PAGE_VIOLATION）。
pub const STATUS_GUARD_PAGE_VIOLATION: u32 = 0x8000_0001;
/// 访问违例 NTSTATUS 码（MS STATUS_ACCESS_VIOLATION）。
pub const STATUS_ACCESS_VIOLATION: u32 = 0xC000_0005;

/// 一个堆块。
#[derive(Clone, Copy)]
pub struct HeapBlock {
    pub offset: usize,
    pub size: usize,
    pub in_use: bool,
}

/// HeapAlloc 族模型：分配/释放/重分配 + 双重释放检出 + 校验。
pub struct HeapModel {
    blocks: [Option<HeapBlock>; MAX_HEAP_BLOCKS],
    next_offset: usize,
    /// 已提交字节（记账面）。
    pub used_bytes: usize,
    /// 双重释放/野句柄释放检出计数。
    pub free_violations: u32,
    /// 分配计数。
    pub alloc_count: u32,
}

impl HeapModel {
    pub const fn new() -> Self {
        HeapModel { blocks: [None; MAX_HEAP_BLOCKS], next_offset: 0, used_bytes: 0, free_violations: 0, alloc_count: 0 }
    }

    fn round_up(size: usize) -> usize {
        size.div_ceil(HEAP_GRAIN) * HEAP_GRAIN
    }

    /// HeapAlloc：对齐取整；容量不足 → Err（HEAP_NOMEM 语义如实返回）。
    pub fn alloc(&mut self, size: usize) -> Result<usize, &'static str> {
        if size == 0 {
            return Err("invalid-size");
        }
        let need = Self::round_up(size);
        for slot in self.blocks.iter_mut() {
            if slot.is_none() {
                *slot = Some(HeapBlock { offset: self.next_offset, size: need, in_use: true });
                self.next_offset += need;
                self.used_bytes += need;
                self.alloc_count += 1;
                return Ok(self.next_offset - need); // 返回块基址
            }
        }
        Err("heap-nomem")
    }

    /// HeapFree：双重释放检出（不静默——主册异常显性化纪律）。
    pub fn free(&mut self, base: usize) -> bool {
        for slot in self.blocks.iter_mut() {
            if let Some(b) = slot {
                if b.offset == base && b.in_use {
                    b.in_use = false;
                    self.used_bytes -= b.size;
                    return true;
                }
                if b.offset == base && !b.in_use {
                    self.free_violations += 1;
                    return false;
                }
            }
        }
        self.free_violations += 1;
        false
    }

    /// HeapReAlloc：缩小原块即可（记账回落）；扩容走新块（搬迁语义，
    /// 返回新基址供程序搬迁数据——MS HeapReAlloc 不保证原址；
    /// 旧块退账后释放——账面与块表始终自洽）。
    pub fn realloc(&mut self, base: usize, new_size: usize) -> Result<usize, &'static str> {
        let need = Self::round_up(new_size);
        for i in 0..MAX_HEAP_BLOCKS {
            let hit = matches!(&self.blocks[i], Some(b) if b.offset == base && b.in_use);
            if hit {
                let old_size = self.blocks[i].unwrap().size;
                if need <= old_size {
                    if let Some(b) = self.blocks[i].as_mut() {
                        self.used_bytes -= b.size - need;
                        b.size = need;
                    }
                    return Ok(base);
                }
                let moved = self.alloc(new_size)?;
                if let Some(b) = self.blocks[i].as_mut() {
                    b.in_use = false;
                    self.used_bytes -= old_size; // 旧块退账（validate 自洽的前提）
                }
                return Ok(moved);
            }
        }
        Err("invalid-handle")
    }

    /// HeapValidate：块账一致（used == Σ in-use size）。
    pub fn validate(&self) -> bool {
        let sum: usize = self.blocks.iter().flatten().filter(|b| b.in_use).map(|b| b.size).sum();
        sum == self.used_bytes
    }
}

/// GlobalMemoryStatusEx 四元组（MS 结构语义：物理/页面文件/负载）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemoryStatusEx {
    pub total_phys: u64,
    pub avail_phys: u64,
    pub total_page_file: u64,
    pub avail_page_file: u64,
}

impl MemoryStatusEx {
    /// dwMemoryLoad（permille 口径；MS 为 0-100 百分比，此处 ×10 精度）。
    pub fn memory_load_permille(&self) -> u32 {
        if self.total_phys == 0 {
            return 0;
        }
        ((self.total_phys - self.avail_phys) * 1000 / self.total_phys) as u32
    }
}

/// 工作集锁定面（VirtualLock/VirtualUnlock；MS 语义：锁定计数页上限）。
pub struct WorkingSetLocks {
    locked: [bool; 16],
    /// 当前锁定页数。
    pub locked_count: u32,
    /// 未锁先解检出计数。
    pub unlock_violations: u32,
}

impl WorkingSetLocks {
    pub const fn new() -> Self {
        WorkingSetLocks { locked: [false; 16], locked_count: 0, unlock_violations: 0 }
    }
    pub fn lock(&mut self, page: usize) -> bool {
        if page >= 16 || self.locked[page] {
            return false;
        }
        self.locked[page] = true;
        self.locked_count += 1;
        true
    }
    pub fn unlock(&mut self, page: usize) -> bool {
        if page >= 16 || !self.locked[page] {
            self.unlock_violations += 1;
            return false;
        }
        self.locked[page] = false;
        self.locked_count -= 1;
        true
    }
}

/// 守护页/访问违例 → NTSTATUS 分类（F020 流程的分类入口）。
pub fn classify_fault(guard: bool, addr_in_region: bool) -> u32 {
    match (guard, addr_in_region) {
        (true, _) => STATUS_GUARD_PAGE_VIOLATION,
        (false, true) => STATUS_ACCESS_VIOLATION,
        (false, false) => 0xC000_000E, // STATUS_INVALID_ADDRESS
    }
}

/// 域自检（深化批次二）。
pub fn run_f021d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F021-memalign-d2");
    // 1) 堆分配 16 字节对齐（MS HeapAlloc 语义）。
    let mut h = HeapModel::new();
    let a = h.alloc(1).expect("首个分配必成");
    let b = h.alloc(17).expect("次个分配必成");
    cs.add("heap_grain_align", a % HEAP_GRAIN == 0 && b % HEAP_GRAIN == 0 && b - a == HEAP_GRAIN, "");
    // 2) 释放/重分配记账：缩容原址、扩容搬迁、校验账一致。
    let _ = h.free(a);
    let c = h.alloc(8).unwrap();
    let shrunk = h.realloc(c, 4).unwrap();
    let grown = h.realloc(shrunk, 64).unwrap();
    cs.add("heap_realloc_ledger", h.validate() && grown != shrunk && grown % HEAP_GRAIN == 0, "");
    // 3) 双重释放检出（不静默）。
    let d = h.alloc(16).unwrap();
    cs.add("heap_double_free", h.free(d) && !h.free(d) && h.free_violations == 1, "");
    // 4) 内存负载四元组（25% 占用 → 250‰）。
    let ms = MemoryStatusEx { total_phys: 8 << 30, avail_phys: 6 << 30, total_page_file: 16 << 30, avail_page_file: 12 << 30 };
    cs.add("memstatus_load", ms.memory_load_permille() == 250, "");
    // 5) 工作集锁：锁定/解锁对称；未锁先解检出。
    let mut w = WorkingSetLocks::new();
    let lock_ok = w.lock(3) && !w.lock(3) && w.locked_count == 1;
    cs.add("ws_lock_symmetric", lock_ok && w.unlock(3) && !w.unlock(3) && w.unlock_violations == 1 && w.locked_count == 0, "");
    // 6) 守护页/访问违例 NTSTATUS 分类。
    cs.add(
        "fault_status_classify",
        classify_fault(true, true) == STATUS_GUARD_PAGE_VIOLATION
            && classify_fault(false, true) == STATUS_ACCESS_VIOLATION
            && classify_fault(false, false) == 0xC000_000E,
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_full_reports_nomem() {
        let mut h = HeapModel::new();
        let mut last = 0usize;
        for _ in 0..MAX_HEAP_BLOCKS {
            last = h.alloc(16).expect("容量内必成");
        }
        let _ = last;
        assert_eq!(h.alloc(16), Err("heap-nomem"), "块表满如实返回 HEAP_NOMEM 语义");
    }

    #[test]
    fn invalid_handle_realloc_rejected() {
        let mut h = HeapModel::new();
        assert_eq!(h.realloc(0xdead, 16), Err("invalid-handle"));
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f021d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
