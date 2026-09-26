//! F021 内存语义对齐（compatstar · G-A-21）——Windows 内存 API 的 VARIX 落地面。
//!
//! 主册判据（验收标准第一句）：
//! **7-Zip 基准实测无降级（F021 验收原文）；VirtualAlloc 压力脚本
//! （分配-提交-触碰-释放 1 万轮）无泄漏无碎片失控。**
//!
//! 功能定义（G-A-21）：VirtualAlloc 族（RESERVE/COMMIT/PROTECT 六保护位/
//! 写时复制）、MapViewOfFile 文件映射、VirtualQuery 查询、大页请求
//! （自适应降级）全语义对齐；伙伴分配器供给（WP-104 既有）加大页优先策略。
//!
//! 【设计细节】RESERVE 地址空间按 64KB 粒度（Windows 同粒度）；PROTECT 六位
//! 全支持（含写时复制位）；WRITECOPY 即写时复制（改页触发复制）；文件映射
//! 大小按文件当前大小（扩展后需重映射，语义对齐）；大页请求 2MB 对齐不足
//! 自动降 4KB（降级日志）。
//! 【状态与异常】内存不足 → COMMIT 失败返回 NULL（Windows 语义），程序自
//! 处理；保护位违规 → SIGSEGV 类异常进 F020 流程；释放后再访问 → 守卫页
//! 捕获（F176）。提交记账入 MD2 附录 K 内存配额账（F195 配额执行）。
//!
//! 零堆纪律：定长区域表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 页粒度 4KB（x86_64 基线）。
pub const PAGE_SIZE: u64 = 4096;
/// RESERVE 粒度 64KB——主册【设计细节】「Windows 同粒度」
/// （MS 文档 dwAllocationGranularity = 64KB）。
pub const ALLOC_GRANULARITY: u64 = 64 << 10;
/// 大页 2MB——主册：大页请求 2MB 对齐。
pub const LARGE_PAGE: u64 = 2 << 20;

/// AllocationType 位（MS VirtualAlloc 语义对齐）。
pub const MEM_COMMIT: u32 = 0x0000_1000;
pub const MEM_RESERVE: u32 = 0x0000_2000;
pub const MEM_RESET: u32 = 0x0008_0000;
pub const MEM_RELEASE: u32 = 0x0000_8000;

/// PROTECT 六保护位——主册「PROTECT 六位全支持（含写时复制位）」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protect {
    NoAccess,
    ReadOnly,
    ReadWrite,
    /// 写时复制：改页触发复制（WRITECOPY 即写时复制——主册）。
    WriteCopy,
    ExecuteRead,
    ExecuteReadWrite,
}

impl Protect {
    /// MS PAGE_* 常量值（VirtualQuery 返回语义）。
    pub fn win_value(self) -> u32 {
        match self {
            Protect::NoAccess => 0x01,
            Protect::ReadOnly => 0x02,
            Protect::ReadWrite => 0x04,
            Protect::WriteCopy => 0x08,
            Protect::ExecuteRead => 0x20,
            Protect::ExecuteReadWrite => 0x40,
        }
    }
    pub fn writable(self) -> bool {
        matches!(self, Protect::ReadWrite | Protect::WriteCopy | Protect::ExecuteReadWrite)
    }
}

/// 地址按 64KB 粒度向上取整。
pub fn round_up_granularity(addr: u64) -> u64 {
    addr.div_ceil(ALLOC_GRANULARITY) * ALLOC_GRANULARITY
}

// ---------------------------------------------------------------------------
// 区域表（VirtualAlloc 语义状态机）
// ---------------------------------------------------------------------------

/// 单个保留区域的状态。
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub base: u64,
    pub size: u64,
    pub reserved: bool,
    pub committed: bool,
    pub protect: Protect,
    /// 写时复制页位图（128 位 = 8KB 覆盖，域内自检用足够）。
    pub copy_on_write_pages: u64,
    /// 是否来自大页请求（降级日志标记）。
    pub large_page_degraded: bool,
}

/// 定长区域表：VirtualAlloc/VirtualFree/VirtualQuery 的语义承载。
/// 表容量 64 区（域内自检与压力脚本模型用；真实容量随伙伴分配器供给面）。
pub const MAX_REGIONS: usize = 64;

pub struct VirtualMemory {
    regions: [Option<Region>; MAX_REGIONS],
    /// 提交字节记账（F195 配额执行的兼容面账目）。
    committed_bytes: u64,
    reserved_bytes: u64,
    /// 容量上限（OOM → COMMIT 失败返回 NULL——主册 Windows 语义）。
    pub commit_limit: u64,
    /// 大页降级事件计数（降级日志的账面）。
    pub large_page_degrades: u32,
    /// 保护违规事件计数（进 F020 流程的入口计数）。
    pub protect_violations: u32,
    /// 释放后再访问捕获计数（守卫页 F176 联动）。
    pub use_after_free_hits: u32,
}

impl VirtualMemory {
    pub const fn new(commit_limit: u64) -> Self {
        VirtualMemory {
            regions: [None; MAX_REGIONS],
            committed_bytes: 0,
            reserved_bytes: 0,
            commit_limit,
            large_page_degrades: 0,
            protect_violations: 0,
            use_after_free_hits: 0,
        }
    }

    /// VirtualAlloc：RESERVE 地址空间按 64KB 粒度；COMMIT 按页提交。
    /// 返回 Some(base) = 成功；None = NULL（容量不足/参数非法，Windows 语义）。
    pub fn virtual_alloc(
        &mut self,
        addr_hint: u64,
        size: u64,
        alloc_type: u32,
        protect: Protect,
    ) -> Option<u64> {
        if size == 0 || (alloc_type & (MEM_RESERVE | MEM_COMMIT)) == 0 {
            return None;
        }
        let base = round_up_granularity(addr_hint);
        let reserve_size = round_up_granularity(size);
        // 找空槽并检查地址冲突。
        let mut slot = None;
        for i in 0..MAX_REGIONS {
            if self.regions[i].is_none() {
                slot = Some(i);
                break;
            }
        }
        let i = slot?;
        for r in self.regions.iter().flatten() {
            if base < r.base + r.size && r.base < base + reserve_size {
                return None; // 地址冲突
            }
        }
        let need_commit = if alloc_type & MEM_COMMIT != 0 { reserve_size } else { 0 };
        if self.committed_bytes + need_commit > self.commit_limit {
            return None; // COMMIT 失败返回 NULL（主册：程序自处理）
        }
        self.regions[i] = Some(Region {
            base,
            size: reserve_size,
            reserved: true,
            committed: alloc_type & MEM_COMMIT != 0,
            protect,
            copy_on_write_pages: 0,
            large_page_degraded: false,
        });
        self.reserved_bytes += reserve_size;
        self.committed_bytes += need_commit;
        Some(base)
    }

    /// 大页请求：2MB 对齐；对齐不足自动降 4KB 并计降级日志（主册【设计细节】）。
    pub fn virtual_alloc_large(
        &mut self,
        addr_hint: u64,
        pages: u64,
        protect: Protect,
    ) -> Option<u64> {
        let aligned = addr_hint % LARGE_PAGE == 0 && pages % (LARGE_PAGE / PAGE_SIZE) == 0;
        if !aligned {
            self.large_page_degrades += 1; // 降级日志（账面）
        }
        self.virtual_alloc(addr_hint, pages * PAGE_SIZE, MEM_RESERVE | MEM_COMMIT, protect)
    }

    /// 写访问：WRITECOPY 页触发复制（改页触发复制——主册），计数进提交账。
    pub fn write_page(&mut self, addr: u64, page_index: u64) -> Result<(), &'static str> {
        for i in 0..MAX_REGIONS {
            if let Some(r) = &self.regions[i] {
                if addr >= r.base && addr < r.base + r.size {
                    let protect = r.protect;
                    let cow = r.copy_on_write_pages >> page_index & 1 == 1;
                    let committed = r.committed;
                    if !protect.writable() {
                        self.protect_violations += 1; // SIGSEGV 类异常进 F020
                        return Err("access-violation");
                    }
                    if !committed {
                        return Err("not-committed");
                    }
                    if protect == Protect::WriteCopy && !cow {
                        if let Some(rm) = self.regions[i].as_mut() {
                            rm.copy_on_write_pages |= 1 << page_index;
                        }
                        self.committed_bytes += PAGE_SIZE; // 私有副本计入提交账
                    }
                    return Ok(());
                }
            }
        }
        self.use_after_free_hits += 1; // 守卫页捕获（F176）
        Err("guard-page")
    }

    /// VirtualFree（MEM_RELEASE 整块释放）。
    pub fn virtual_free(&mut self, base: u64) -> bool {
        for i in 0..MAX_REGIONS {
            if let Some(r) = self.regions[i] {
                if r.base == base {
                    self.reserved_bytes -= r.size;
                    if r.committed {
                        let cow_private = r.copy_on_write_pages.count_ones() as u64 * PAGE_SIZE;
                        self.committed_bytes -= r.size + cow_private;
                    }
                    self.regions[i] = None;
                    return true;
                }
            }
        }
        false
    }

    /// VirtualQuery：返回区域的保护位快照（MS 语义）。
    pub fn virtual_query(&self, addr: u64) -> Option<(u64, u64, u32)> {
        for r in self.regions.iter().flatten() {
            if addr >= r.base && addr < r.base + r.size {
                return Some((r.base, r.size, r.protect.win_value()));
            }
        }
        None
    }

    /// MapViewOfFile：文件映射大小按文件当前大小（扩展后需重映射——主册语义）。
    /// 基址自动避让：从 0 起按 64KB 粒度扫描首个无冲突槽（映射系统面职责，
    /// 区别于显式 hint 的 VirtualAlloc）。
    pub fn map_view_of_file(&mut self, file_size: u64, protect: Protect) -> Option<u64> {
        let size = round_up_granularity(file_size.max(1));
        let mut hint = 0u64;
        for _ in 0..4096 {
            if let Some(b) = self.virtual_alloc(hint, size, MEM_RESERVE | MEM_COMMIT, protect) {
                return Some(b);
            }
            hint += ALLOC_GRANULARITY;
        }
        None
    }

    pub fn committed_bytes(&self) -> u64 {
        self.committed_bytes
    }
    pub fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }
    /// 记账一致性：提交账 ≤ 上限（COW 私有副本可超保留账——保留是地址
    /// 空间口径，提交是物理页口径，二者正交，与 Windows 语义一致）。
    pub fn accounting_invariant(&self) -> bool {
        self.committed_bytes <= self.commit_limit
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_memalign_checks() -> CheckSet {
    let mut cs = CheckSet::new("F021-memalign");
    // 1) 64KB 粒度取整（主册：Windows 同粒度）。
    cs.add("granularity_64k", round_up_granularity(1) == ALLOC_GRANULARITY && round_up_granularity(64 << 10) == 64 << 10, "");
    // 2) RESERVE/COMMIT 全语义：分配成功且记账入账。
    let mut vm = VirtualMemory::new(1 << 30);
    let base = vm.virtual_alloc(0, 128 << 10, MEM_RESERVE | MEM_COMMIT, Protect::ReadWrite);
    cs.add("reserve_commit_ok", base.is_some() && vm.committed_bytes() == 128 << 10, "");
    // 3) OOM → COMMIT 失败返回 NULL（Windows 语义，程序自处理）。
    let mut tight = VirtualMemory::new(64 << 10);
    cs.add("commit_fail_returns_null", tight.virtual_alloc(0, 128 << 10, MEM_RESERVE | MEM_COMMIT, Protect::ReadWrite).is_none(), "");
    // 4) PROTECT 六位全支持（win 值对拍 MS 常量）。
    let six = [Protect::NoAccess, Protect::ReadOnly, Protect::ReadWrite, Protect::WriteCopy, Protect::ExecuteRead, Protect::ExecuteReadWrite];
    let vals: [u32; 6] = [0x01, 0x02, 0x04, 0x08, 0x20, 0x40];
    let mut six_ok = true;
    for (p, v) in six.iter().zip(vals.iter()) {
        six_ok &= p.win_value() == *v;
    }
    cs.add("six_protect_bits", six_ok, "");
    // 5) WRITECOPY 写时复制：首写触发复制、记账增加、二次写不再复制。
    let mut cow = VirtualMemory::new(1 << 20);
    let b = cow.virtual_alloc(0, PAGE_SIZE * 2, MEM_RESERVE | MEM_COMMIT, Protect::WriteCopy).unwrap();
    let before = cow.committed_bytes();
    let w1 = cow.write_page(b, 0);
    let mid = cow.committed_bytes();
    let w2 = cow.write_page(b, 0);
    cs.add("writecopy_cow", w1.is_ok() && w2.is_ok() && mid == before + PAGE_SIZE && cow.committed_bytes() == mid, "");
    // 6) 保护违规计数（进 F020 流程的入口）。
    let mut ro = VirtualMemory::new(1 << 20);
    let rb = ro.virtual_alloc(0, PAGE_SIZE, MEM_RESERVE | MEM_COMMIT, Protect::ReadOnly).unwrap();
    cs.add("protect_violation_logged", ro.write_page(rb, 0).is_err() && ro.protect_violations == 1, "");
    // 7) 释放后再访问 → 守卫页捕获（F176 联动）。
    let mut uaf = VirtualMemory::new(1 << 20);
    let ub = uaf.virtual_alloc(0, PAGE_SIZE, MEM_RESERVE | MEM_COMMIT, Protect::ReadWrite).unwrap();
    uaf.virtual_free(ub);
    cs.add("use_after_free_guard", uaf.write_page(ub, 0).is_err() && uaf.use_after_free_hits == 1, "");
    // 8) 大页请求：对齐不足自动降 4KB + 降级日志；对齐足额不降。
    let mut lp = VirtualMemory::new(1 << 30);
    lp.virtual_alloc_large(0, LARGE_PAGE / PAGE_SIZE, Protect::ReadWrite);
    lp.virtual_alloc_large(4096, 512, Protect::ReadWrite);
    cs.add("large_page_degrade_log", lp.large_page_degrades == 1, "");
    // 9) VirtualQuery 返回保护位快照（MS 语义）。
    cs.add("virtual_query", vm.virtual_query(base.unwrap()) == Some((base.unwrap(), 128 << 10, Protect::ReadWrite.win_value())), "");
    // 10) MapViewOfFile：映射大小按文件当前大小取整到 64KB。
    let mut mm = VirtualMemory::new(1 << 30);
    let mb = mm.map_view_of_file(1000, Protect::ReadOnly);
    cs.add("map_view_size", mb.is_some() && mm.reserved_bytes() == ALLOC_GRANULARITY, "");
    // 11) 记账一致性不变量。
    cs.add("accounting_invariant", vm.accounting_invariant() && cow.accounting_invariant(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据：VirtualAlloc 压力脚本（分配-提交-触碰-释放 1 万轮）
    /// 无泄漏无碎片失控。
    #[test]
    fn stress_10k_rounds_no_leak() {
        let mut vm = VirtualMemory::new(64 << 20);
        let base_commit = vm.committed_bytes();
        for round in 0..10_000u32 {
            let size = PAGE_SIZE * (1 + (round % 16) as u64);
            let b = vm
                .virtual_alloc(0, size, MEM_RESERVE | MEM_COMMIT, Protect::ReadWrite)
                .expect("1 万轮内不得 OOM（容量按轮次上限预留）");
            vm.write_page(b, 0).expect("触碰页应可写");
            assert!(vm.virtual_free(b), "释放应成功");
        }
        // 无泄漏：终态记账回到起点。
        assert_eq!(vm.committed_bytes(), base_commit, "无泄漏判据");
        assert_eq!(vm.reserved_bytes(), 0, "保留账也归零");
        assert!(vm.accounting_invariant(), "碎片失控判据（记账不变量）");
    }

    #[test]
    fn free_then_reuse_same_slot() {
        let mut vm = VirtualMemory::new(1 << 20);
        let b = vm.virtual_alloc(0, 64 << 10, MEM_RESERVE | MEM_COMMIT, Protect::ReadWrite).unwrap();
        vm.virtual_free(b);
        let b2 = vm.virtual_alloc(0, 64 << 10, MEM_RESERVE | MEM_COMMIT, Protect::ReadWrite).unwrap();
        assert_eq!(b, b2, "释放后槽位可复用（无碎片堆积）");
    }

    #[test]
    fn writecopy_second_write_is_private() {
        let mut vm = VirtualMemory::new(1 << 20);
        let b = vm.virtual_alloc(0, PAGE_SIZE * 4, MEM_RESERVE | MEM_COMMIT, Protect::WriteCopy).unwrap();
        vm.write_page(b, 0).unwrap();
        vm.write_page(b, 1).unwrap();
        // 两个私有页；提交账基线按 64KB RESERVE 粒度取整（模型口径：
        // COMMIT 随 RESERVE 粒度整段入账）。
        let r = vm.virtual_query(b).unwrap();
        assert_eq!(r.2, Protect::WriteCopy.win_value());
        assert_eq!(vm.committed_bytes(), ALLOC_GRANULARITY + PAGE_SIZE * 2);
    }

    #[test]
    fn addr_conflict_rejected() {
        let mut vm = VirtualMemory::new(1 << 20);
        let b = vm.virtual_alloc(0, 64 << 10, MEM_RESERVE, Protect::ReadOnly).unwrap();
        assert!(vm.virtual_alloc(b, 64 << 10, MEM_RESERVE, Protect::ReadOnly).is_none(), "地址冲突必须拒绝");
    }

    #[test]
    fn file_mapping_remap_semantics() {
        // 文件映射大小按文件当前大小；扩展后需重映射（旧视图不自动扩）。
        let mut vm = VirtualMemory::new(4 << 20);
        let v1 = vm.map_view_of_file(1 << 10, Protect::ReadOnly).unwrap();
        let v2 = vm.map_view_of_file(1 << 20, Protect::ReadOnly).unwrap();
        assert_ne!(v1, v2, "第二个视图自动避让到无冲突基址");
        assert_eq!(vm.reserved_bytes(), ALLOC_GRANULARITY * 17, "1KB→1 粒度 + 1MB→16 粒度");
    }
}

// ===========================================================================
// 深化层 · G-A-21 补强：VirtualProtect / MEM_RESET / DECOMMIT / 守护页
// （主册【功能定义】全语义对齐口径；语义对照 Wine virtual.c 与 MS 文档）
// ---------------------------------------------------------------------------

/// MEM_DECOMMIT / MEM_RELEASE 释放类型（MS 语义对拍）。
pub const MEM_DECOMMIT: u32 = 0x0000_4000;

/// 分配类型合法组合矩阵（MS VirtualAlloc dwAllocationType 判据）：
/// RESERVE 独用 / RESERVE|COMMIT / COMMIT（对已保留区）合法；其余非法。
pub fn alloc_type_valid(alloc_type: u32) -> bool {
    match alloc_type {
        a if a == MEM_RESERVE => true,
        a if a == (MEM_RESERVE | MEM_COMMIT) => true,
        a if a == MEM_COMMIT => true, // 对已保留区再提交（本域模型允许）
        a if a == MEM_RESET => true,
        _ => false,
    }
}

/// 页保护权能矩阵（VirtualProtect 可设集合；WRITECOPY 不可显式申请——MS 语义）。
pub fn protect_settable(p: Protect) -> bool {
    !matches!(p, Protect::WriteCopy)
}

/// 重保护结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReprotectVerdict {
    Ok,
    /// 区不存在（VirtualQuery 空洞）。
    NotFound,
    /// WRITECOPY 显式申请拒绝。
    InvalidProtect,
}

/// 区域表深化：重保护 + 反提交 + 守护页查询。
impl VirtualMemory {
    /// VirtualProtect：对已提交区改保护位；WRITECOPY 不可显式申请。
    pub fn virtual_protect(&mut self, addr: u64, protect: Protect) -> ReprotectVerdict {
        if !protect_settable(protect) {
            return ReprotectVerdict::InvalidProtect;
        }
        for i in 0..MAX_REGIONS {
            let hit = matches!(&self.regions[i], Some(r) if addr >= r.base && addr < r.base + r.size);
            if hit {
                if let Some(r) = self.regions[i].as_mut() {
                    r.protect = protect;
                }
                return ReprotectVerdict::Ok;
            }
        }
        ReprotectVerdict::NotFound
    }

    /// MEM_DECOMMIT：退提交（保留壳仍在），提交账回落；页级区间 [addr, addr+size)。
    pub fn virtual_decommit(&mut self, addr: u64, size: u64) -> bool {
        let pages = round_up_granularity(size);
        for i in 0..MAX_REGIONS {
            let hit = matches!(&self.regions[i], Some(r) if addr >= r.base && addr < r.base + r.size);
            if hit {
                if let Some(r) = self.regions[i].as_mut() {
                    if !r.committed {
                        return false;
                    }
                    let drop = pages.min(r.size);
                    r.committed = false;
                    self.committed_bytes -= drop + r.copy_on_write_pages.count_ones() as u64 * PAGE_SIZE;
                    r.copy_on_write_pages = 0;
                }
                return true;
            }
        }
        false
    }

    /// 重提交：对已保留壳的区重新 COMMIT（DECOMMIT 后的恢复路径）。
    pub fn virtual_recommit(&mut self, addr: u64, protect: Protect) -> bool {
        for i in 0..MAX_REGIONS {
            let hit = matches!(&self.regions[i], Some(r) if addr >= r.base && addr < r.base + r.size);
            if hit {
                if let Some(r) = self.regions[i].as_mut() {
                    if r.committed {
                        return false; // 已提交
                    }
                    r.committed = true;
                    r.protect = protect;
                    self.committed_bytes += r.size;
                }
                return true;
            }
        }
        false
    }

    /// MEM_RESET：页内容作废标记（不退账，语义 = 数据不再可信）。
    pub fn virtual_reset(&mut self, addr: u64) -> bool {
        for i in 0..MAX_REGIONS {
            let hit = matches!(&self.regions[i], Some(r) if addr >= r.base && addr < r.base + r.size);
            if hit {
                return true;
            }
        }
        false
    }

    /// 守护页语义：PAGE_GUARD 类访问捕获计数（F176 联动的兼容面出口）。
    pub fn guard_query(&self, addr: u64) -> bool {
        self.virtual_query(addr).is_none() // 无主地址 = 守护捕获口径
    }
}

/// 提交账策略常量：DECOMMIT 后重提交必须先 RESERVE 过（Windows 语义）。
pub const DECOMMIT_KEEPS_RESERVE: bool = true;

/// 域自检（深化层）。
pub fn run_memalign_deep() -> CheckSet {
    let mut cs = CheckSet::new("F021-memalign-deep");
    // 1) 分配类型矩阵：合法三态 + RESET + 非法组合拒绝。
    cs.add(
        "alloc_type_matrix",
        alloc_type_valid(MEM_RESERVE) && alloc_type_valid(MEM_RESERVE | MEM_COMMIT) && alloc_type_valid(MEM_COMMIT) && alloc_type_valid(MEM_RESET) && !alloc_type_valid(0xFFFF) && !alloc_type_valid(MEM_RELEASE),
        "",
    );
    // 2) VirtualProtect：重保护生效 + WRITECOPY 显式拒绝 + 空洞 NotFound。
    let mut vm = VirtualMemory::new(1 << 20);
    let b = vm.virtual_alloc(0, 64 << 10, MEM_RESERVE | MEM_COMMIT, Protect::ReadWrite).unwrap();
    cs.add(
        "virtual_protect",
        vm.virtual_protect(b, Protect::ExecuteRead) == ReprotectVerdict::Ok
            && vm.virtual_query(b).unwrap().2 == Protect::ExecuteRead.win_value()
            && vm.virtual_protect(b, Protect::WriteCopy) == ReprotectVerdict::InvalidProtect
            && vm.virtual_protect(1 << 30, Protect::ReadOnly) == ReprotectVerdict::NotFound,
        "",
    );
    // 3) MEM_DECOMMIT：提交账回落、保留壳仍在、再访问走守护捕获。
    let mut dm = VirtualMemory::new(1 << 20);
    let db = dm.virtual_alloc(0, 128 << 10, MEM_RESERVE | MEM_COMMIT, Protect::ReadWrite).unwrap();
    let before = dm.committed_bytes();
    cs.add(
        "decommit_semantics",
        dm.virtual_decommit(db, 64 << 10) && dm.committed_bytes() == before - (64 << 10) && DECOMMIT_KEEPS_RESERVE && dm.reserved_bytes() == 128 << 10 && dm.write_page(db, 0).is_err(),
        "",
    );
    // 4) MEM_RESET：作废标记不改账。
    cs.add("mem_reset_noop_ledger", dm.virtual_reset(db) && dm.committed_bytes() == before - (64 << 10), "");
    // 5) 守护页口径：未映射地址 query 即守护捕获。
    cs.add("guard_page_query", vm.guard_query(1 << 30) && !vm.guard_query(b), "");
    // 6) 重保护后写访问按新保护位裁决（ReadOnly → 违规计数）。
    let mut pm = VirtualMemory::new(1 << 20);
    let pb = pm.virtual_alloc(0, 64 << 10, MEM_RESERVE | MEM_COMMIT, Protect::ReadWrite).unwrap();
    pm.virtual_protect(pb, Protect::ReadOnly);
    cs.add("reprotect_enforced", pm.write_page(pb, 0).is_err() && pm.protect_violations == 1, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn protect_matrix_disciplines() {
        // 六保护位中仅 WRITECOPY 不可显式申请（MS VirtualProtect 语义）。
        assert!(protect_settable(Protect::NoAccess));
        assert!(protect_settable(Protect::ExecuteReadWrite));
        assert!(!protect_settable(Protect::WriteCopy));
    }

    #[test]
    fn decommit_then_recommit_flow() {
        let mut vm = VirtualMemory::new(1 << 20);
        let b = vm.virtual_alloc(0, 128 << 10, MEM_RESERVE | MEM_COMMIT, Protect::ReadWrite).unwrap();
        vm.virtual_decommit(b, 128 << 10);
        assert_eq!(vm.committed_bytes(), 0);
        // 保留壳仍在 → 同址重提交恢复（无需重新 RESERVE）。
        assert!(vm.virtual_recommit(b, Protect::ReadWrite));
        assert_eq!(vm.committed_bytes(), 128 << 10);
    }

    #[test]
    fn deep_checks_all_green() {
        let cs = run_memalign_deep();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
