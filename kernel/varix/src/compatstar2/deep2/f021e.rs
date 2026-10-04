//! F021 深化批次三 · VirtualAlloc 四态状态机与保护位边界面（compatstar2/deep2 · G-A-21）。
//!
//! 批次一覆盖 VirtualProtect/DECOMMIT/重提交，批次二覆盖堆句柄族；本批补齐
//! 主册【功能定义】「全语义对齐」的执行/边界/注入面：VirtualAlloc 四态转换
//! 矩阵（free→reserve→commit→decommit→release，非法转换如实拒绝）、PAGE_*
//! 保护位三维判定（读/写/取指，GUARD 陷阱先行）、区域扫描器（64KB 分配
//! 粒度定长区域表，VirtualQuery 遍历模型：返回下一段基址/大小/状态/保护）、
//! commit 超越 reserve 边界拒绝。
//!
//! 判据对账：主册 G-A-21【设计细节】区域粒度/状态机段 + MS VirtualAlloc/
//! VirtualQuery/PAGE_* 文档语义对拍（保护位取 MS 真实值）。零堆纪律：定长
//! 区域表（16 槽），无 Vec/String/Box/format!，错误一律 Err 或计数账面。

use crate::checks::CheckSet;

/// 分配粒度 64KB（主册 G-A-21；MS dwAllocationGranularity）。
pub const ALLOC_GRANULARITY: usize = 65536;
/// 区域表容量（域内模型口径）。
pub const MAX_REGIONS: usize = 16;
/// PAGE_* 保护位（MS winntr.h 真实值）。
pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_EXECUTE: u32 = 0x10;
pub const PAGE_GUARD: u32 = 0x100;
/// 基础保护位掩码（低 8 位；GUARD 为修饰位不参与基础判定）。
pub const PAGE_BASE_MASK: u32 = 0x0FF;

/// 区域四态（MS MEM_FREE=0x1_0000/MEM_RESERVE=0x2000/MEM_COMMIT=0x1000）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionState {
    Free,
    Reserved,
    Committed,
}

/// 四态转换矩阵：合法路径放行；同态/非法转换如实拒绝（主册【状态与异常】）。
pub fn transition(from: RegionState, to: RegionState) -> Result<RegionState, &'static str> {
    match (from, to) {
        (RegionState::Free, RegionState::Reserved)
        | (RegionState::Free, RegionState::Committed)
        | (RegionState::Reserved, RegionState::Committed) => Ok(to),
        (RegionState::Committed, RegionState::Reserved) => Ok(RegionState::Reserved),
        (RegionState::Reserved, RegionState::Free)
        | (RegionState::Committed, RegionState::Free) => Ok(RegionState::Free),
        _ => Err("invalid-transition"),
    }
}

/// 访问维度（读/写/取指三维判定模型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Read,
    Write,
    Exec,
}

/// 基础保护位三维判定（MS PAGE_* 文档：NOACCESS 全拒；EXECUTE 仅取指）。
pub fn access_allowed(protect: u32, want: Access) -> bool {
    match (protect & PAGE_BASE_MASK, want) {
        (PAGE_READONLY, Access::Read) | (PAGE_READWRITE, Access::Read) | (PAGE_READWRITE, Access::Write)
        | (PAGE_EXECUTE, Access::Exec) => true,
        _ => false,
    }
}

/// GUARD 修饰位检出。
pub fn is_guard(protect: u32) -> bool {
    protect & PAGE_GUARD != 0
}

/// 访问裁决：GUARD 陷阱先行（MS 语义：先触发一次异常再摘牌），其后基础判定。
pub fn access_verdict(protect: u32, want: Access) -> Result<(), &'static str> {
    if is_guard(protect) { Err("guard-violation") } else if access_allowed(protect, want) { Ok(()) } else { Err("access-denied") }
}

/// 一个定长区域段（VirtualQuery 返回面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub base: usize,
    pub size: usize,
    pub state: RegionState,
    pub protect: u32,
    /// 自基址起已提交字节数（前缀扩展模型，64KB 对齐）。
    pub commit_extent: usize,
}

/// 定长区域表：reserve/commit/decommit/release + VirtualQuery 遍历模型。
pub struct RegionTable {
    regions: [Option<Region>; MAX_REGIONS],
    pub count: usize,
    /// commit 超越 reserve 边界拒绝计数（不静默）。
    pub commit_rejects: u32,
    pub overlap_rejects: u32,
    pub decommit_count: u32,
    pub release_count: u32,
}

fn align_up(x: usize) -> usize {
    x.div_ceil(ALLOC_GRANULARITY) * ALLOC_GRANULARITY
}

fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.0 + b.1 && b.0 < a.0 + a.1
}

impl RegionTable {
    pub const fn new() -> Self {
        RegionTable {
            regions: [None; MAX_REGIONS],
            count: 0,
            commit_rejects: 0,
            overlap_rejects: 0,
            decommit_count: 0,
            release_count: 0,
        }
    }

    /// VirtualAlloc(MEM_RESERVE)：基址/大小按 64KB 粒度对齐；重叠拒绝。
    pub fn reserve(&mut self, base_in: usize, size_in: usize) -> Result<usize, &'static str> {
        let base = align_up(base_in);
        let size = align_up(size_in);
        for slot in self.regions.iter().flatten() {
            if overlaps((base, size), (slot.base, slot.size)) { self.overlap_rejects += 1; return Err("region-overlap"); }
        }
        let Some(i) = (0..MAX_REGIONS).find(|&i| self.regions[i].is_none()) else {
            return Err("region-table-full");
        };
        self.regions[i] = Some(Region { base, size, state: RegionState::Reserved, protect: PAGE_NOACCESS, commit_extent: 0 });
        self.count += 1;
        Ok(base)
    }

    /// VirtualAlloc(MEM_COMMIT)：保留区内前缀扩展；超越 reserve 边界如实
    /// 拒绝；自由区上 commit = reserve+commit 合并（MS 语义）。
    pub fn commit(&mut self, base: usize, size: usize) -> Result<(), &'static str> {
        for i in 0..MAX_REGIONS {
            if let Some(r) = self.regions[i] {
                if r.base == base {
                    let need = align_up(size);
                    if r.commit_extent + need > r.size { self.commit_rejects += 1; return Err("commit-beyond-reserve"); }
                    if let Some(slot) = self.regions[i].as_mut() {
                        slot.commit_extent += need;
                        slot.state = RegionState::Committed;
                        slot.protect = PAGE_READWRITE;
                    }
                    return Ok(());
                }
            }
        }
        let b = self.reserve(base, size)?;
        if let Some(slot) = self.regions.iter_mut().flatten().find(|r| r.base == b) {
            slot.state = RegionState::Committed;
            slot.protect = PAGE_READWRITE;
            slot.commit_extent = slot.size;
        }
        Ok(())
    }

    /// VirtualFree(MEM_DECOMMIT)：整区退提交、保留保留态。
    pub fn decommit(&mut self, base: usize) -> bool {
        for slot in self.regions.iter_mut().flatten() {
            if slot.base == base && slot.state == RegionState::Committed {
                slot.state = RegionState::Reserved;
                slot.protect = PAGE_NOACCESS;
                slot.commit_extent = 0;
                self.decommit_count += 1;
                return true;
            }
        }
        false
    }

    /// VirtualFree(MEM_RELEASE)：整区释放（槽位回收）。
    pub fn release(&mut self, base: usize) -> bool {
        for i in 0..MAX_REGIONS {
            if let Some(r) = self.regions[i] {
                if r.base == base {
                    self.regions[i] = None;
                    self.count -= 1;
                    self.release_count += 1;
                    return true;
                }
            }
        }
        false
    }

    /// VirtualQuery：返回包含 addr 的区域段（基址/大小/状态/保护）。
    pub fn query(&self, addr: usize) -> Result<Region, &'static str> {
        for slot in self.regions.iter().flatten() {
            if addr >= slot.base && addr < slot.base + slot.size { return Ok(*slot); }
        }
        Err("query-no-region")
    }

    /// VirtualQuery 遍历模型：返回基址大于 after 的最低段（逐段走表）。
    pub fn walk(&self, after: usize) -> Option<Region> {
        let mut best: Option<Region> = None;
        for slot in self.regions.iter().flatten() {
            if slot.base > after && best.map_or(true, |b| slot.base < b.base) {
                best = Some(*slot);
            }
        }
        best
    }

    /// 账面自洽：count 与非空槽一致。
    pub fn invariant(&self) -> bool {
        self.regions.iter().flatten().count() == self.count
    }
}

/// 域自检（深化批次三）。
pub fn run_f021e_checks() -> crate::checks::CheckSet {
    let mut cs = CheckSet::new("F021-memalign-d3");
    // 1) 四态矩阵合法链：reserve→commit→decommit→release 与自由区直连 commit。
    cs.add(
        "state_matrix_legal",
        transition(RegionState::Free, RegionState::Reserved).is_ok() && transition(RegionState::Reserved, RegionState::Committed).is_ok()
            && transition(RegionState::Committed, RegionState::Reserved).is_ok() && transition(RegionState::Reserved, RegionState::Free).is_ok()
            && transition(RegionState::Free, RegionState::Committed).is_ok(),
        "",
    );
    // 2) 同态/非法转换如实拒绝。
    cs.add(
        "state_matrix_reject",
        transition(RegionState::Committed, RegionState::Committed) == Err("invalid-transition")
            && transition(RegionState::Free, RegionState::Free) == Err("invalid-transition")
            && transition(RegionState::Reserved, RegionState::Reserved) == Err("invalid-transition"),
        "",
    );
    // 3) PAGE_* 真实值 + 三维判定（MS 值 0x01/0x02/0x04/0x10；MEM_* 值见枚举注释）。
    cs.add(
        "protect_dims",
        (PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, PAGE_EXECUTE) == (0x01, 0x02, 0x04, 0x10)
            && access_allowed(PAGE_READONLY, Access::Read) && !access_allowed(PAGE_READONLY, Access::Write)
            && !access_allowed(PAGE_READONLY, Access::Exec) && access_allowed(PAGE_READWRITE, Access::Read)
            && access_allowed(PAGE_READWRITE, Access::Write) && !access_allowed(PAGE_READWRITE, Access::Exec)
            && !access_allowed(PAGE_NOACCESS, Access::Read) && access_allowed(PAGE_EXECUTE, Access::Exec)
            && !access_allowed(PAGE_EXECUTE, Access::Read),
        "",
    );
    // 4) GUARD 修饰位：0x100 检出、基础位保留、陷阱先行。
    cs.add(
        "guard_trap_first",
        is_guard(PAGE_READWRITE | PAGE_GUARD)
            && (PAGE_READWRITE | PAGE_GUARD) & PAGE_BASE_MASK == PAGE_READWRITE
            && access_verdict(PAGE_READWRITE | PAGE_GUARD, Access::Read) == Err("guard-violation"),
        "",
    );
    // 5) 64KB 粒度对齐（MS 分配粒度）。
    let mut t = RegionTable::new();
    let base = t.reserve(1000, 1000).expect("空表首保留必成");
    cs.add("granularity_align", base == ALLOC_GRANULARITY && t.query(base).is_ok(), "");
    // 6) commit 前缀扩展 + 越界拒绝（超越 reserve 边界）。
    let _ = t.commit(base, ALLOC_GRANULARITY);
    cs.add(
        "commit_beyond_reserve",
        t.commit(base, ALLOC_GRANULARITY * 2) == Err("commit-beyond-reserve") && t.commit_rejects == 1, "",
    );
    // 7) VirtualQuery 返回段四元组；表外地址显性拒绝。
    let q = t.query(base).expect("区内查询必中");
    cs.add(
        "virtual_query_region",
        q.base == base && q.state == RegionState::Committed && q.protect == PAGE_READWRITE
            && t.query(base + ALLOC_GRANULARITY * 16) == Err("query-no-region"),
        "",
    );
    // 8) VirtualQuery 遍历：walk 返回下一最低段，走完为 None。
    let b2 = t.reserve(base + ALLOC_GRANULARITY * 8, ALLOC_GRANULARITY).expect("次保留必成");
    let nxt = t.walk(base).expect("存在后续段");
    cs.add("virtual_query_walk", nxt.base == b2 && t.walk(b2).is_none(), "");
    // 9) decommit/release 账面：退提交回保留态、释放清槽、重复释放拒绝。
    let d_ok = t.decommit(base) && t.decommit_count == 1;
    let q2 = t.query(base).expect("退提交后仍在表");
    let r_ok = t.release(base) && !t.release(base) && t.release_count == 1;
    cs.add(
        "decommit_release_ledger",
        d_ok && q2.state == RegionState::Reserved && q2.commit_extent == 0 && r_ok && t.invariant(),
        "",
    );
    // 10) 重叠保留拒绝 + 区域表满如实拒绝（均不静默）。
    let mut t2 = RegionTable::new();
    let _ = t2.reserve(0, ALLOC_GRANULARITY);
    let mut ok = true;
    for k in 1..MAX_REGIONS {
        ok = ok && t2.reserve(k * ALLOC_GRANULARITY, ALLOC_GRANULARITY).is_ok();
    }
    cs.add(
        "reserve_rejects",
        t2.reserve(0, ALLOC_GRANULARITY) == Err("region-overlap") && t2.overlap_rejects == 1
            && t2.reserve(MAX_REGIONS * ALLOC_GRANULARITY, ALLOC_GRANULARITY) == Err("region-table-full")
            && ok && t2.invariant(),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_matrix_rejects_self_loops() {
        let pairs = [
            (RegionState::Free, RegionState::Free),
            (RegionState::Reserved, RegionState::Reserved),
            (RegionState::Committed, RegionState::Committed),
        ];
        for (f, t) in pairs {
            assert_eq!(transition(f, t), Err("invalid-transition"), "同态转换必须拒绝");
        }
    }

    #[test]
    fn commit_beyond_reserve_is_explicit() {
        let mut t = RegionTable::new();
        let b = t.reserve(0, ALLOC_GRANULARITY).unwrap();
        assert!(t.commit(b, ALLOC_GRANULARITY).is_ok());
        assert_eq!(t.commit(b, 1), Err("commit-beyond-reserve"));
        assert_eq!(t.commit_rejects, 1);
    }

    #[test]
    fn guard_raises_before_base_protect() {
        // READWRITE 基础位本可读，但 GUARD 陷阱必须先行。
        assert_eq!(access_verdict(PAGE_READWRITE | PAGE_GUARD, Access::Read), Err("guard-violation"));
        assert_eq!(access_verdict(PAGE_READWRITE, Access::Read), Ok(()));
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f021e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
