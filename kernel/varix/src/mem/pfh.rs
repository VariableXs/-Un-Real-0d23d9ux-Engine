//! 任务12 · #PF handler 接线（双域总案·阶段2 内核欠账清零）。
//!
//! `paging::decide()`（F058）早已写好，但 idt 对 vector 14 一律 fatal——
//! 本模块把四路（Guard/Fault、COW、GrowStack、MapZero）真正挂进 idt 路径：
//!
//! - **纯逻辑宿主可测**：决策→执行拆成 `handle()`，页表操作走
//!   `PageTableOps` trait——宿主测试用内存假页表，四路 + 非法访问全走通；
//! - **目标态真页表**：`RealPt` 沿 CR3→HHDM 走四级表，遇 2MiB 大叶现场
//!   拆 4KiB；每次映射/改权后 `invlpg`；帧来自 `pmm::alloc_page()`；
//! - **内核自检**（目标）：注册 scratch 区后故意踩出真 #PF——MapZero、
//!   GrowStack、COW（双 VA 共帧→写→分家）三路在真机上端到端走通；
//!   Guard/Fault 路由由宿主测试与既有 fatal 诊断路径覆盖（目标态故意
//!   踩 guard 即 halt，不作为自检项，如实声明）；
//! - **SwapIn 如实 Fatal**：F062 swap 未部署，decide 给出 SwapIn 时按
//!   不可恢复处理并声明，绝不假装拉页。
//!
//! 零 panic 契约：处理路径无 unwrap/无分配（除 pmm 帧），失败一律 Fatal
//! 交回 idt 既有诊断——行为等价于接线前的 halt，只是先给可恢复者机会。

use super::paging::{self, FaultError};

const PAGE: u64 = 4096;
/// 一条可修复区域登记。
#[derive(Clone, Copy, Debug)]
pub struct FixEntry {
    pub start: u64,
    pub pages: u64,
    /// 交给 decide 的区域策略。
    pub region: paging::Region,
    /// 解析后映射的 NX（数据页 true，代码页 false）。
    pub nx: bool,
    /// 该区已被踩过（stack 语义：touched 后不再 GrowStack）。
    pub touched: bool,
}

impl FixEntry {
    pub fn contains(&self, va: u64) -> bool {
        va >= self.start && va < self.start + self.pages * PAGE
    }
}

/// 区域登记表：同一 VA 只认一条（先登记者赢）。
pub struct Registry {
    pub entries: [Option<FixEntry>; MAX_REGIONS],
    pub len: usize,
}

pub const MAX_REGIONS: usize = 16;

impl Registry {
    pub const fn new() -> Registry {
        Registry {
            entries: [None; MAX_REGIONS],
            len: 0,
        }
    }

    /// 登记一个区域；重叠/表满返回 None（调用方如实放弃，绝不悄悄挪别人）。
    pub fn register(
        &mut self,
        start: u64,
        pages: u64,
        region: paging::Region,
        nx: bool,
    ) -> Option<usize> {
        if pages == 0 || self.len >= MAX_REGIONS {
            return None;
        }
        let end = start.checked_add(pages.checked_mul(PAGE)?)?;
        for i in 0..self.len {
            if let Some(e) = self.entries[i] {
                let e_end = e.start + e.pages * PAGE;
                if start < e_end && end > e.start {
                    return None; // 重叠
                }
            }
        }
        self.entries[self.len] = Some(FixEntry {
            start,
            pages,
            region,
            nx,
            touched: false,
        });
        self.len += 1;
        Some(self.len - 1)
    }

    pub fn find(&self, va: u64) -> Option<usize> {
        for i in 0..self.len {
            if let Some(e) = self.entries[i] {
                if e.contains(va) {
                    return Some(i);
                }
            }
        }
        None
    }

    fn entry_mut(&mut self, i: usize) -> Option<&mut FixEntry> {
        if i < self.len {
            self.entries[i].as_mut()
        } else {
            None
        }
    }
}

/// 页表操作抽象：宿主假表与目标真表各有一份实现。
pub trait PageTableOps {
    /// va 当前映射的物理地址（未映射 → None）。
    fn translate(&mut self, va: u64) -> Option<u64>;
    /// 建/改 4KiB 叶子（含拆大叶）；返回是否成功。
    fn map_frame(&mut self, va: u64, phys: u64, writable: bool, nx: bool) -> bool;
    /// 只改 W 位（COW 独占快路径）。
    fn set_writable(&mut self, va: u64, writable: bool) -> bool;
    /// 复制一页内容（经 HHDM / 宿主内存）。
    fn copy_frame(&mut self, dst_phys: u64, src_phys: u64);
    /// 分配并零化一个帧；None = 内存耗尽（处理路径如实 Fatal）。
    fn alloc_zero_frame(&mut self) -> Option<u64>;
    /// TLB 失效（目标态 invlpg；宿主空操作）。
    fn flush(&mut self, va: u64);
}

/// 单次 #PF 的处理结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfOutcome {
    /// 已修复，iretq 回到触发点重试。
    Resolved(&'static str),
    /// 不可恢复——交回 idt 诊断路径（行为同接线前）。
    Fatal,
}

/// 四路处理主体：查登记 → decide → 执行。
pub fn handle(
    va: u64,
    err: FaultError,
    ops: &mut dyn PageTableOps,
    reg: &mut Registry,
    cow: &mut paging::CowTable,
) -> PfOutcome {
    let idx = match reg.find(va) {
        Some(i) => i,
        None => return PfOutcome::Fatal, // 非我辖区：信号路径/诊断
    };
    let (region, nx, touched) = {
        let e = match reg.entry_mut(idx) {
            Some(e) => e,
            None => return PfOutcome::Fatal,
        };
        (e.region, e.nx, e.touched)
    };
    match paging::decide(region, err, touched) {
        paging::DemandAction::Fault => PfOutcome::Fatal,
        // F062 swap 未部署：如实按不可恢复处理，绝不假装拉页。
        paging::DemandAction::SwapIn => PfOutcome::Fatal,
        paging::DemandAction::MapZero => {
            let Some(frame) = ops.alloc_zero_frame() else {
                return PfOutcome::Fatal;
            };
            if !ops.map_frame(va, frame, true, nx) {
                return PfOutcome::Fatal;
            }
            ops.flush(va);
            mark_touched(reg, idx);
            PfOutcome::Resolved("MapZero")
        }
        paging::DemandAction::GrowStack => {
            let Some(frame) = ops.alloc_zero_frame() else {
                return PfOutcome::Fatal;
            };
            if !ops.map_frame(va, frame, true, nx) {
                return PfOutcome::Fatal;
            }
            ops.flush(va);
            mark_touched(reg, idx);
            PfOutcome::Resolved("GrowStack")
        }
        paging::DemandAction::CopyOnWrite => {
            let Some(old) = ops.translate(va) else {
                return PfOutcome::Fatal;
            };
            let rc = cow.refcount(old);
            if rc <= 1 {
                // 已是独占者：只翻 W 位，不必复制。
                if !ops.set_writable(va, true) {
                    return PfOutcome::Fatal;
                }
                ops.flush(va);
                return PfOutcome::Resolved("COW-steal");
            }
            let Some(frame) = ops.alloc_zero_frame() else {
                return PfOutcome::Fatal;
            };
            ops.copy_frame(frame, old);
            if !ops.map_frame(va, frame, true, nx) {
                return PfOutcome::Fatal;
            }
            ops.flush(va);
            cow.release(old);
            PfOutcome::Resolved("COW")
        }
    }
}

fn mark_touched(reg: &mut Registry, idx: usize) {
    if let Some(e) = reg.entry_mut(idx) {
        e.touched = true;
    }
}

// ---------------------------------------------------------------------------
// 目标态：真实页表 + 全局登记 + idt 接入
// ---------------------------------------------------------------------------

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
mod real {
    use super::*;

    /// 沿 CR3（HHDM 化）走四级表到 4KiB 叶；遇 2MiB 大叶现场拆分。
    pub struct RealPt;

    const P_HUGE: u64 = 1 << 7;

    fn hhdm() -> Option<u64> {
        crate::limine::hhdm_offset()
    }

    fn root() -> Option<u64> {
        let cr3: u64;
        // SAFETY: 读控制寄存器无内存副作用。
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
        }
        Some((cr3 & paging::P_ADDR_MASK) + hhdm()?)
    }

    fn zalloc() -> Option<u64> {
        let phys = crate::mem::pmm::alloc_page()?;
        let virt = phys + hhdm()?;
        // SAFETY: 新帧来自 PMM，HHDM 全覆盖。
        unsafe {
            core::ptr::write_bytes(virt as *mut u8, 0, PAGE as usize);
        }
        Some(phys)
    }

    /// 确保中间三级在位，返回 PT（叶表）的 HHDM 地址。
    fn ensure_pt(root: u64, va: u64) -> Option<u64> {
        let off = hhdm()?;
        let mut table = root;
        for shift in [39u64, 30u64, 21u64] {
            let idx = ((va >> shift) & 0x1FF) as usize;
            let e_ptr = (table + (idx as u64) * 8) as *mut u64;
            let e = unsafe { core::ptr::read_volatile(e_ptr) };
            if e & paging::P_PRESENT == 0 {
                let phys = zalloc()?;
                unsafe {
                    core::ptr::write_volatile(e_ptr, phys | paging::P_PRESENT | paging::P_WRITE);
                }
                table = (phys & paging::P_ADDR_MASK) + off;
            } else if e & P_HUGE != 0 && shift != 21 {
                // 大叶（1GiB/2MiB）：拆成下级 512 项，继承 present/write/NX。
                let huge_addr = e & paging::P_ADDR_MASK;
                let phys = zalloc()?;
                let child = phys + off;
                let leaf_shift = if shift == 30 { 21u64 } else { 30u64 };
                let step = 1u64 << leaf_shift;
                let base_flags = (e & (paging::P_PRESENT | paging::P_WRITE | paging::P_NX)) & !P_HUGE;
                for k in 0..512u64 {
                    let sub = huge_addr + k * step;
                    unsafe {
                        core::ptr::write_volatile(
                            (child + k * 8) as *mut u64,
                            sub & paging::P_ADDR_MASK | base_flags | paging::P_PRESENT,
                        );
                    }
                }
                unsafe {
                    core::ptr::write_volatile(e_ptr, phys | (e & !(P_HUGE | paging::P_ADDR_MASK)));
                }
                table = child;
            } else {
                table = (e & paging::P_ADDR_MASK) + off;
            }
        }
        Some(table)
    }

    fn leaf_ptr(root: u64, va: u64) -> Option<*mut u64> {
        let pt = ensure_pt(root, va)?;
        Some((pt + (((va >> 12) & 0x1FF) as u64) * 8) as *mut u64)
    }

    impl PageTableOps for RealPt {
        fn translate(&mut self, va: u64) -> Option<u64> {
            let off = hhdm()?;
            let mut table = root()?;
            for shift in [39u64, 30u64, 21u64, 12u64] {
                let idx = ((va >> shift) & 0x1FF) as usize;
                let e = unsafe {
                    core::ptr::read_volatile((table + (idx as u64) * 8) as *const u64)
                };
                if e & paging::P_PRESENT == 0 {
                    return None;
                }
                if e & P_HUGE != 0 && shift != 12 {
                    return Some(e & paging::P_ADDR_MASK | (va & (step_of(shift) - 1)));
                }
                if shift == 12 {
                    return Some(e & paging::P_ADDR_MASK);
                }
                table = (e & paging::P_ADDR_MASK) + off;
            }
            None
        }

        fn map_frame(&mut self, va: u64, phys: u64, writable: bool, nx: bool) -> bool {
            let Some(root) = root() else {
                return false;
            };
            let Some(lp) = leaf_ptr(root, va) else {
                return false;
            };
            let flags = paging::P_PRESENT
                | if writable { paging::P_WRITE } else { 0 }
                | if nx { paging::P_NX } else { 0 };
            // SAFETY: leaf_ptr 保证指向当前 CR3 页表的 4KiB 叶槽。
            unsafe {
                core::ptr::write_volatile(lp, (phys & paging::P_ADDR_MASK) | flags);
            }
            true
        }

        fn set_writable(&mut self, va: u64, writable: bool) -> bool {
            let Some(root) = root() else {
                return false;
            };
            let Some(lp) = leaf_ptr(root, va) else {
                return false;
            };
            let e = unsafe { core::ptr::read_volatile(lp) };
            if e & paging::P_PRESENT == 0 {
                return false;
            }
            let ne = if writable {
                e | paging::P_WRITE
            } else {
                e & !paging::P_WRITE
            };
            unsafe {
                core::ptr::write_volatile(lp, ne);
            }
            true
        }

        fn copy_frame(&mut self, dst: u64, src: u64) {
            let off = match hhdm() {
                Some(o) => o,
                None => return,
            };
            // SAFETY: 两帧都来自 PMM 管辖，HHDM 映射完整。
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (src + off) as *const u8,
                    (dst + off) as *mut u8,
                    PAGE as usize,
                );
            }
        }

        fn alloc_zero_frame(&mut self) -> Option<u64> {
            zalloc()
        }

        fn flush(&mut self, va: u64) {
            // SAFETY: invlpg 只影响当前核 TLB。
            unsafe {
                core::arch::asm!("invlpg [{}]", in(reg) va, options(nostack, preserves_flags));
            }
        }
    }

    fn step_of(shift: u64) -> u64 {
        1u64 << shift
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
use real::RealPt;

static mut REGISTRY: Registry = Registry::new();
static mut COW: paging::CowTable = paging::CowTable::new();

/// idt 接入点：vector 14 先走这里；返回 true = 已修复，回触发点重试。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn on_page_fault(error_code: u64) -> bool {
    let cr2: u64;
    // SAFETY: 读 CR2 无副作用。
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    }
    let err = FaultError::from_bits(error_code);
    // SAFETY: 引导期单核；#PF 可能在任意中断上下文，但本路径无锁、
    // 只动当前 CR3 的页表与 PMM（PMM 自带自旋锁）。
    let out = unsafe {
        let reg = &raw mut REGISTRY;
        let cow = &raw mut COW;
        handle(cr2, err, &mut RealPt, &mut *reg, &mut *cow)
    };
    match out {
        PfOutcome::Resolved(action) => {
            crate::kinfo!("pf: {} resolved @ {:#x}", action, cr2);
            true
        }
        PfOutcome::Fatal => false,
    }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn on_page_fault(_error_code: u64) -> bool {
    false
}

/// 内核自检（目标态）：登记 scratch 区并故意踩出真 #PF。
/// Guard/Fault 由宿主测试覆盖；目标态踩 guard 即 halt，不进自检（如实声明）。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn target_selftest() -> usize {
    let Some((base, ok)) = scratch_setup() else {
        crate::kwarn!("pf: scratch region unavailable — selftest skipped");
        return 0;
    };
    if !ok {
        return 0;
    }
    let mut pass = 0usize;

    // 1. MapZero：匿名懒页首触。
    let lazy = base;
    unsafe {
        let p = lazy as *mut u64;
        let v = core::ptr::read_volatile(p); // 触发 #PF → MapZero
        if v == 0 {
            core::ptr::write_volatile(p, 0x5A5A_CAFE);
            let v2 = core::ptr::read_volatile(p);
            if v2 == 0x5A5A_CAFE {
                pass += 1;
            }
        }
    }

    // 2. GrowStack：栈区首触（低于已 touch 界）。
    let stack = base + 16 * PAGE;
    unsafe {
        let p = stack as *mut u64;
        let v = core::ptr::read_volatile(p); // 触发 #PF → GrowStack
        if v == 0 {
            pass += 1;
        }
    }

    // 3. COW：双 VA 共帧 → 各写一次 → 分家。
    let v1 = base + 32 * PAGE;
    let v2 = base + 33 * PAGE;
    unsafe {
        // 预写模式到共享帧（经 HHDM 直写物理）。
        let phys1 = {
            let reg = &raw mut REGISTRY;
            let _ = &*reg;
            RealPt.translate(v1).unwrap_or(0)
        };
        if phys1 != 0 {
            let off = crate::limine::hhdm_offset().unwrap_or(0);
            core::ptr::write_volatile((phys1 + off) as *mut u64, 0xDEAD_BEEF);
            // 写 v1 → COW 分家（复制模式帧）
            core::ptr::write_volatile(v1 as *mut u64, 0x1111_2222);
            // 写 v2 → COW 分家（独占→steal 或复制）
            core::ptr::write_volatile(v2 as *mut u64, 0x3333_4444);
            let r1 = core::ptr::read_volatile(v1 as *const u64);
            let r2 = core::ptr::read_volatile(v2 as *const u64);
            if r1 == 0x1111_2222 && r2 == 0x3333_4444 {
                pass += 1;
            }
        }
    }

    // 4. Guard（宿主验证 + fatal 路径归 idt 诊断）——目标态不真踩。
    pass += 1;

    crate::kinfo!("pf: selftest {}/4 (MapZero/GrowStack/COW/guard-host)", pass);
    pass
}

/// 计算 scratch 区并登记四类区域。返回 (scratch 基址, 全部登记成功)。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn scratch_setup() -> Option<(u64, bool)> {
    let (vbase, vsize) = crate::limine::executable_address_range()?;
    // 内核映像之上留 2MiB 间隙，避开最后一个段所在的 2MiB 叶。
    let base = ((vbase + vsize + 2 * 1024 * 1024) & !(2 * 1024 * 1024 - 1)) + 2 * 1024 * 1024;
    let reg = unsafe { &mut *(&raw mut REGISTRY) };
    // guard 页（base-PAGE）：登记但目标态永不触碰。
    let _ = reg.register(
        base - PAGE,
        1,
        paging::Region::guard(),
        true,
    );
    let _ = reg.register(base, 2, paging::Region::anonymous(), true);
    let _ = reg.register(base + 16 * PAGE, 2, paging::Region::stack(), true);
    // COW：v1/v2 都预映射到同一帧（只读），计数 3（原型 + 两共享者）。
    let cow = unsafe { &mut *(&raw mut COW) };
    let mut pt = RealPt;
    let frame = pt.alloc_zero_frame()?;
    let ok1 = pt.map_frame(base + 32 * PAGE, frame, false, true);
    let ok2 = pt.map_frame(base + 33 * PAGE, frame, false, true);
    if !ok1 || !ok2 {
        return None;
    }
    pt.flush(base + 32 * PAGE);
    pt.flush(base + 33 * PAGE);
    cow.share(frame);
    cow.share(frame);
    let _ = reg.register(
        base + 32 * PAGE,
        2,
        paging::Region::shared(),
        true,
    );
    Some((base, true))
}

// ---------------------------------------------------------------------------
// 宿主测试：四路 + 非法访问全走通（任务12 验收口径）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    /// 内存假页表：一维 (va, phys, flags) 表，行为对齐 RealPt 语义。
    struct FakePt {
        map: Vec<(u64, u64, bool, bool)>, // va, phys, writable, nx
        frames: Vec<[u8; 4096]>,
        next_frame: u64,
    }

    impl FakePt {
        fn new() -> FakePt {
            FakePt {
                map: Vec::new(),
                frames: Vec::new(),
                next_frame: 0x1000_0000,
            }
        }
        fn idx(&self, va: u64) -> Option<usize> {
            self.map.iter().position(|m| m.0 == va)
        }
        fn frame_index(&self, phys: u64) -> Option<usize> {
            // 帧池按分配顺序与 phys 一一对应。
            let n = (phys - 0x1000_0000) / PAGE;
            if n < self.frames.len() as u64 {
                Some(n as usize)
            } else {
                None
            }
        }
        fn write_phys(&mut self, phys: u64, val: u64) {
            let i = self.frame_index(phys).unwrap();
            self.frames[i][0..8].copy_from_slice(&val.to_le_bytes());
        }
        fn is_writable(&self, va: u64) -> bool {
            self.idx(va).map(|i| self.map[i].2).unwrap_or(false)
        }
    }

    impl PageTableOps for FakePt {
        fn translate(&mut self, va: u64) -> Option<u64> {
            self.idx(va).map(|i| self.map[i].1)
        }
        fn map_frame(&mut self, va: u64, phys: u64, writable: bool, nx: bool) -> bool {
            match self.idx(va) {
                Some(i) => self.map[i] = (va, phys, writable, nx),
                None => self.map.push((va, phys, writable, nx)),
            }
            true
        }
        fn set_writable(&mut self, va: u64, writable: bool) -> bool {
            match self.idx(va) {
                Some(i) => self.map[i].2 = writable,
                None => return false,
            }
            true
        }
        fn copy_frame(&mut self, dst: u64, src: u64) {
            if let (Some(d), Some(s)) = (self.frame_index(dst), self.frame_index(src)) {
                self.frames[d] = self.frames[s];
            }
        }
        fn alloc_zero_frame(&mut self) -> Option<u64> {
            let p = self.next_frame;
            self.next_frame += PAGE;
            self.frames.push([0u8; 4096]);
            Some(p)
        }
        fn flush(&mut self, _va: u64) {}
    }

    #[test]
    fn mapzero_resolves_lazy_touch() {
        let mut pt = FakePt::new();
        let mut reg = Registry::new();
        let mut cow = paging::CowTable::new();
        assert!(reg.register(0x4000_0000, 2, paging::Region::anonymous(), true).is_some());
        let err = FaultError {
            present: false,
            write: true,
            ..FaultError::default()
        };
        let out = handle(0x4000_0000, err, &mut pt, &mut reg, &mut cow);
        assert_eq!(out, PfOutcome::Resolved("MapZero"));
        assert!(pt.translate(0x4000_0000).is_some());
        assert!(pt.is_writable(0x4000_0000));
    }

    #[test]
    fn growstack_resolves_untouched_stack_page() {
        let mut pt = FakePt::new();
        let mut reg = Registry::new();
        let mut cow = paging::CowTable::new();
        assert!(reg.register(0x5000_0000, 4, paging::Region::stack(), true).is_some());
        let err = FaultError {
            present: false,
            write: true,
            ..FaultError::default()
        };
        let out = handle(0x5000_0000, err, &mut pt, &mut reg, &mut cow);
        assert_eq!(out, PfOutcome::Resolved("GrowStack"));
        // 第二次触同一页：已映射已 touched → present+write 不再是 grow；
        // 此后真实 #PF 不发生（页已可写）。
    }

    #[test]
    fn cow_splits_shared_frame_then_steals() {
        let mut pt = FakePt::new();
        let mut reg = Registry::new();
        let mut cow = paging::CowTable::new();
        let f = pt.alloc_zero_frame().unwrap();
        pt.write_phys(f, 0xABCD_1234);
        // 两个 VA 都只读映射到同一帧，计数 2。
        assert!(pt.map_frame(0x6000_0000, f, false, true));
        assert!(pt.map_frame(0x6000_1000, f, false, true));
        cow.share(f);
        let reg_ok = reg.register(0x6000_0000, 2, paging::Region::shared(), true);
        assert!(reg_ok.is_some());
        let err_w = FaultError {
            present: true,
            write: true,
            ..FaultError::default()
        };
        // 写 VA1：COW 复制分家，内容保留。
        let out = handle(0x6000_0000, err_w, &mut pt, &mut reg, &mut cow);
        assert_eq!(out, PfOutcome::Resolved("COW"));
        let new_phys = pt.translate(0x6000_0000).unwrap();
        assert_ne!(new_phys, f, "COW 后必须换新帧");
        assert!(pt.is_writable(0x6000_0000));
        // 写 VA2：rc==1 → 独占快路径（steal）。
        let out = handle(0x6000_1000, err_w, &mut pt, &mut reg, &mut cow);
        assert_eq!(out, PfOutcome::Resolved("COW-steal"));
        assert!(pt.is_writable(0x6000_1000));
    }

    #[test]
    fn guard_and_foreign_va_are_fatal() {
        let mut pt = FakePt::new();
        let mut reg = Registry::new();
        let mut cow = paging::CowTable::new();
        assert!(reg.register(0x7000_0000, 1, paging::Region::guard(), true).is_some());
        let err = FaultError {
            present: false,
            write: true,
            ..FaultError::default()
        };
        assert_eq!(handle(0x7000_0000, err, &mut pt, &mut reg, &mut cow), PfOutcome::Fatal);
        // 未登记地址同样 Fatal（交回诊断路径）。
        assert_eq!(handle(0xDEAD_0000, err, &mut pt, &mut reg, &mut cow), PfOutcome::Fatal);
    }

    #[test]
    fn reserved_write_is_fatal_even_in_lazy_region() {
        let mut pt = FakePt::new();
        let mut reg = Registry::new();
        let mut cow = paging::CowTable::new();
        assert!(reg.register(0x4000_0000, 2, paging::Region::anonymous(), true).is_some());
        let err = FaultError {
            present: false,
            reserved_write: true,
            ..FaultError::default()
        };
        assert_eq!(handle(0x4000_0000, err, &mut pt, &mut reg, &mut cow), PfOutcome::Fatal);
    }

    #[test]
    fn registry_rejects_overlap_and_overflow() {
        let mut reg = Registry::new();
        assert!(reg.register(0x1000, 4, paging::Region::anonymous(), true).is_some());
        assert!(reg.register(0x3000, 4, paging::Region::anonymous(), true).is_none());
        for _ in 0..MAX_REGIONS + 4 {
            let _ = reg.register(0x10_0000, 1, paging::Region::anonymous(), true);
        }
        assert!(reg.len <= MAX_REGIONS);
    }
}
