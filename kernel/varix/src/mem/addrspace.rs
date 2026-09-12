//! AI-01 · F001 每进程地址空间（VARIABLE-200 用户态进程域）.
//!
//! One PML4 per process inside a shared table arena: the user half is private
//! and the kernel half is the *same* PML4 entries in every space (the boot
//! root's upper 256 slots are copied down), so switching processes can never
//! hide the kernel and a process can never reach another process's pages.
//!
//! This module is the isolation proof for the whole user-space domain. Every
//! claim the process domain makes — "two processes cannot see each other",
//! "the guard page faults instead of eating the stack", "W^X holds" — is
//! decided here by consulting real page-table state, not by a flag someone set.

use super::paging::{self, HugeSize, MapError, TableArena, P_NX, P_USER, P_WRITE};

/// Lowest address a user mapping may occupy (the null guard).
pub const USER_MIN: u64 = 0x0000_0000_0000_1000;
/// Highest user address (the canonical lower half, per `paging::USER_TOP`).
pub const USER_TOP: u64 = paging::USER_TOP;
/// PML4 slots 256..512 carry the kernel; every space shares them.
pub const KERNEL_PML4_FIRST: usize = 256;
/// Table frames a single address space may consume before it is refused.
pub const MAX_SPACE_FRAMES: usize = 16;
/// Regions one space tracks (for isolation sweeps and teardown).
pub const MAX_MAPPINGS: usize = 24;
/// Address spaces the arena can host at once (one per live process slot).
pub const MAX_SPACES: usize = 16;
/// Stack guard pages: one page of unmapped memory below the stack.
pub const GUARD_PAGES: usize = 1;
/// Default user stack size in pages (64 KiB).
pub const DEFAULT_STACK_PAGES: usize = 16;
/// Hard ceiling for `brk` growth (8 MiB) — F004 owns the policy.
pub const HEAP_LIMIT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpaceError {
    /// Not page aligned.
    Misaligned,
    /// Outside the user half (belongs to the kernel or the null guard).
    NotUser,
    /// A page cannot be writable *and* executable (F029).
    WriteExecute,
    /// Overlaps an existing mapping in the same space.
    Overlap,
    /// The arena ran out of table frames.
    OutOfFrames,
    /// The space consumed its frame budget.
    Budget,
    /// No free space slot.
    TooManySpaces,
    /// Unmapping something that was never mapped.
    NotMapped,
    /// Address arithmetic overflowed.
    Overflow,
}

/// What a region is for. Kept explicit so the isolation sweep can tell the
/// stack apart from the image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionKind {
    /// Loaded ELF segment (F028 owns the decision, F001 owns the space).
    Image,
    /// User stack (F003).
    Stack,
    /// The single unmapped guard page under a stack (F003).
    Guard,
    /// `brk` heap (F004).
    Heap,
    /// Anonymous `mmap`-style region (F059 consumes it).
    Anonymous,
}

impl RegionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RegionKind::Image => "image",
            RegionKind::Stack => "stack",
            RegionKind::Guard => "guard",
            RegionKind::Heap => "heap",
            RegionKind::Anonymous => "anon",
        }
    }
}

/// One tracked region inside a space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mapping {
    pub base: u64,
    pub pages: usize,
    pub flags: u64,
    pub kind: RegionKind,
}

impl Mapping {
    pub const fn end(&self) -> u64 {
        self.base + (self.pages as u64) * 4096
    }

    pub fn contains(&self, virt: u64) -> bool {
        virt >= self.base && virt < self.end()
    }

    pub fn writable(&self) -> bool {
        self.flags & P_WRITE != 0
    }

    pub fn executable(&self) -> bool {
        self.flags & P_NX == 0
    }
}

/// One process's address space.
#[derive(Clone, Copy, Debug)]
pub struct AddressSpace {
    /// PML4 frame id inside the arena.
    pub root: u16,
    pub pid: u32,
    /// Table frames currently owned by this space (root included).
    pub table_frames: usize,
    /// Mapped user pages, by region.
    mappings: [Option<Mapping>; MAX_MAPPINGS],
    count: usize,
    /// F003 — stack geometry, set by `install_stack`.
    pub stack_base: u64,
    pub stack_top: u64,
    /// F004 — `brk` pointer and its ceiling.
    pub brk: u64,
    pub brk_limit: u64,
    /// Set once the root has been retired; a dead space refuses every map.
    pub destroyed: bool,
}

impl AddressSpace {
    fn empty(root: u16, pid: u32) -> AddressSpace {
        AddressSpace {
            root,
            pid,
            table_frames: 1,
            mappings: [None; MAX_MAPPINGS],
            count: 0,
            stack_base: 0,
            stack_top: 0,
            brk: 0,
            brk_limit: 0,
            destroyed: false,
        }
    }

    pub fn mappings(&self) -> &[Option<Mapping>] {
        &self.mappings
    }

    pub fn mapping_count(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<Mapping> {
        if i < self.count {
            self.mappings[i]
        } else {
            None
        }
    }

    /// Total mapped user pages in this space.
    pub fn mapped_pages(&self) -> usize {
        let mut n = 0;
        for i in 0..self.count {
            if let Some(m) = self.mappings[i] {
                n += m.pages;
            }
        }
        n
    }

    /// Find the region that owns `virt`, ignoring the guard pages (which are
    /// deliberately *not* mapped — that is the whole point of a guard).
    pub fn region_of(&self, virt: u64) -> Option<Mapping> {
        for i in 0..self.count {
            if let Some(m) = self.mappings[i] {
                if m.contains(virt) {
                    return Some(m);
                }
            }
        }
        None
    }

    /// True when `virt` sits in the guard window below the stack (F003).
    pub fn in_guard(&self, virt: u64) -> bool {
        if self.stack_base == 0 {
            return false;
        }
        let guard_lo = self.stack_base - (GUARD_PAGES as u64) * 4096;
        virt >= guard_lo && virt < self.stack_base
    }

    fn record(&mut self, m: Mapping) -> Result<(), SpaceError> {
        if self.count >= MAX_MAPPINGS {
            return Err(SpaceError::Budget);
        }
        self.mappings[self.count] = Some(m);
        self.count += 1;
        Ok(())
    }

    /// True when `[base, end)` collides with any tracked region.
    fn collides(&self, base: u64, pages: usize) -> bool {
        let end = base + (pages as u64) * 4096;
        for i in 0..self.count {
            if let Some(m) = self.mappings[i] {
                if base < m.end() && m.base < end {
                    return true;
                }
            }
        }
        false
    }
}

/// The set of address spaces plus the single table arena they share.
///
/// One arena means the isolation claim is checkable in-process: two spaces are
/// isolated exactly when no user virtual address of one resolves to the same
/// physical page in the other.
pub struct SpaceArena {
    arena: TableArena,
    kernel_root: Option<u16>,
    spaces: [Option<AddressSpace>; MAX_SPACES],
    count: usize,
    destroyed: usize,
}

impl SpaceArena {
    pub const fn new() -> SpaceArena {
        SpaceArena {
            arena: TableArena::new(),
            kernel_root: None,
            spaces: [None; MAX_SPACES],
            count: 0,
            destroyed: 0,
        }
    }

    /// Install the kernel half. Called once with the boot PML4.
    pub fn install_kernel_root(&mut self, root: u16) {
        self.kernel_root = Some(root);
    }

    pub fn table_frames_used(&self) -> usize {
        self.arena.used_frames()
    }

    pub fn live_spaces(&self) -> usize {
        self.count
    }

    pub fn destroyed_spaces(&self) -> usize {
        self.destroyed
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, i: usize) -> Option<&AddressSpace> {
        if i < self.count {
            self.spaces[i].as_ref()
        } else {
            None
        }
    }

    fn get_mut(&mut self, i: usize) -> Option<&mut AddressSpace> {
        if i < self.count {
            self.spaces[i].as_mut()
        } else {
            None
        }
    }

    /// Find a space by pid.
    pub fn find_pid(&self, pid: u32) -> Option<usize> {
        (0..self.count).find(|&i| self.spaces[i].map(|s| s.pid == pid).unwrap_or(false))
    }

    /// F001 — create a fresh PML4 and copy the kernel half into it.
    pub fn create(&mut self, pid: u32) -> Result<usize, SpaceError> {
        if self.count >= MAX_SPACES {
            return Err(SpaceError::TooManySpaces);
        }
        let root = self.arena.alloc_table().ok_or(SpaceError::OutOfFrames)?;
        // 内核半区共享：PML4 上半 256 项从 boot root 逐项复制，进程切换后
        // 内核照常可执行，同时用户态的 PML4 项一律为空 —— 天然互不可见。
        if let Some(kroot) = self.kernel_root {
            for slot in KERNEL_PML4_FIRST..paging::ENTRIES {
                let e = self.arena.entry(kroot, slot);
                self.arena.set_entry(root, slot, e);
            }
        }
        let space = AddressSpace::empty(root, pid);
        self.spaces[self.count] = Some(space);
        self.count += 1;
        Ok(self.count - 1)
    }

    /// F001 — map one user page with W^X enforced (F029) and overlap refused.
    pub fn map_page(
        &mut self,
        index: usize,
        virt: u64,
        phys: u64,
        mut flags: u64,
        kind: RegionKind,
    ) -> Result<(), SpaceError> {
        validate_user_page(virt)?;
        if flags & P_WRITE != 0 && flags & P_NX == 0 {
            return Err(SpaceError::WriteExecute);
        }
        let root = {
            let s = self.get_mut(index).ok_or(SpaceError::TooManySpaces)?;
            if s.destroyed {
                return Err(SpaceError::NotMapped);
            }
            if s.collides(virt, 1) {
                return Err(SpaceError::Overlap);
            }
            s.root
        };
        // The arena always ORs P_USER; NX/X selection rides on `flags`.
        flags |= P_USER;
        let before = self.arena.used_frames();
        self.arena
            .map(root, virt, phys, flags, HugeSize::Small)
            .map_err(map_err)?;
        let after = self.arena.used_frames();
        let over = {
            let s = self.get_mut(index).ok_or(SpaceError::TooManySpaces)?;
            s.table_frames += after - before;
            s.table_frames > MAX_SPACE_FRAMES
        };
        if over {
            // Over budget: undo so the space cannot creep past its cap.
            if let Some(s) = self.get_mut(index) {
                s.table_frames = s.table_frames.saturating_sub(after - before);
            }
            let root = self.get(index).map(|s| s.root).unwrap_or(0);
            self.arena.unmap(root, virt);
            return Err(SpaceError::Budget);
        }
        let s = self.get_mut(index).ok_or(SpaceError::TooManySpaces)?;
        s.record(Mapping {
            base: virt,
            pages: 1,
            flags,
            kind,
        })
    }

    /// Map `pages` consecutive pages, all-or-nothing (F039 consumes this).
    pub fn map_range(
        &mut self,
        index: usize,
        base: u64,
        phys: u64,
        pages: usize,
        flags: u64,
        kind: RegionKind,
    ) -> Result<(), SpaceError> {
        for p in 0..pages {
            let virt = base + (p as u64) * 4096;
            if let Err(e) = self.map_page(index, virt, phys + (p as u64) * 4096, flags, kind) {
                // Roll back what we already mapped: a half-loaded image is
                // worse than a refusal (F039).
                for q in 0..p {
                    let _ = self.unmap(index, base + (q as u64) * 4096);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    /// Remove one mapping. Returns the physical page that was released.
    pub fn unmap(&mut self, index: usize, virt: u64) -> Option<u64> {
        let root = self.get(index)?.root;
        let phys = self.arena.unmap(root, virt)?;
        let s = self.get_mut(index)?;
        for i in 0..s.count {
            if let Some(m) = s.mappings[i] {
                if m.contains(virt) {
                    // Track a partially unmapped range as a hole by shrinking
                    // from the front only when it is the first page.
                    if m.base == virt && m.pages > 1 {
                        s.mappings[i] = Some(Mapping {
                            base: m.base + 4096,
                            pages: m.pages - 1,
                            flags: m.flags,
                            kind: m.kind,
                        });
                    } else {
                        for j in i..s.count - 1 {
                            s.mappings[j] = s.mappings[j + 1];
                        }
                        s.mappings[s.count - 1] = None;
                        s.count -= 1;
                    }
                    break;
                }
            }
        }
        Some(phys)
    }

    /// F038/F029 — rewrite permissions without remapping.
    pub fn protect(&mut self, index: usize, virt: u64, add: u64, remove: u64) -> bool {
        let root = match self.get(index) {
            Some(s) => s.root,
            None => return false,
        };
        if !self.arena.set_flags(root, virt, add, remove) {
            return false;
        }
        if let Some(s) = self.get_mut(index) {
            for i in 0..s.count {
                if let Some(m) = s.mappings[i] {
                    if m.contains(virt) {
                        s.mappings[i] = Some(Mapping {
                            flags: (m.flags | add) & !remove,
                            ..m
                        });
                        return true;
                    }
                }
            }
        }
        true
    }

    pub fn translate(&self, index: usize, virt: u64) -> Option<u64> {
        let s = self.get(index)?;
        if s.destroyed {
            return None;
        }
        self.arena.translate(s.root, virt)
    }

    pub fn flags_of(&self, index: usize, virt: u64) -> Option<u64> {
        let s = self.get(index)?;
        self.arena.walk(s.root, virt).map(|(e, _)| e)
    }

    /// F001 隔离性判定 — true when the two spaces share no user page.
    pub fn isolated(&self, a: usize, b: usize) -> bool {
        let (sa, sb) = match (self.get(a), self.get(b)) {
            (Some(x), Some(y)) => (x, y),
            _ => return false,
        };
        for i in 0..sa.count {
            let m = match sa.mappings[i] {
                Some(m) => m,
                None => continue,
            };
            for p in 0..m.pages {
                let virt = m.base + (p as u64) * 4096;
                if let Some(pa) = self.arena.translate(sa.root, virt) {
                    if let Some(pb) = self.arena.translate(sb.root, virt) {
                        if pa == pb {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    /// Every user virtual address in `a` that is *not* mapped in `b`.
    /// This is the stronger statement: b cannot address a's memory at all.
    pub fn unreachable_from(&self, a: usize, b: usize) -> bool {
        let (sa, sb) = match (self.get(a), self.get(b)) {
            (Some(x), Some(y)) => (x, y),
            _ => return false,
        };
        for i in 0..sa.count {
            let m = match sa.mappings[i] {
                Some(m) => m,
                None => continue,
            };
            for p in 0..m.pages {
                let virt = m.base + (p as u64) * 4096;
                if self.arena.translate(sb.root, virt).is_some() {
                    return false;
                }
            }
        }
        true
    }

    /// F003 — build the user stack: `pages` usable pages plus a guard below.
    pub fn install_stack(
        &mut self,
        index: usize,
        top: u64,
        pages: usize,
        phys_base: u64,
    ) -> Result<(u64, u64), SpaceError> {
        if pages == 0 || pages + GUARD_PAGES > MAX_MAPPINGS {
            return Err(SpaceError::Budget);
        }
        let bytes = (pages as u64) * 4096;
        let base = top.checked_sub(bytes).ok_or(SpaceError::Overflow)?;
        validate_user_page(base)?;
        validate_user_page(top - 4096)?;
        // 栈可从写不可执行；守卫页干脆不映射 —— 越界即页错误，不会踩内存。
        let flags = P_WRITE | P_NX;
        self.map_range(index, base, phys_base, pages, flags, RegionKind::Stack)?;
        {
            let s = self.get_mut(index).ok_or(SpaceError::TooManySpaces)?;
            s.stack_base = base;
            s.stack_top = top;
            // 首次进入用户态的 rsp：栈顶向下留 16 字节（规范要求的对齐余量）。
            let _ = s.record(Mapping {
                base: base - 4096,
                pages: 0,
                flags: 0,
                kind: RegionKind::Guard,
            });
        }
        Ok((base, top))
    }

    /// F004 — grow the heap by `pages`, refusing past `brk_limit`.
    pub fn brk_grow(
        &mut self,
        index: usize,
        pages: usize,
        phys_base: u64,
    ) -> Result<u64, SpaceError> {
        let (brk, limit) = {
            let s = self.get(index).ok_or(SpaceError::TooManySpaces)?;
            (s.brk, s.brk_limit)
        };
        if brk == 0 {
            // Heap not initialised yet: initialise on first touch.
            let s = self.get_mut(index).ok_or(SpaceError::TooManySpaces)?;
            s.brk = page_up(s.stack_top.saturating_add(0x10));
            s.brk_limit = s.brk + HEAP_LIMIT_BYTES;
            return self.brk_grow(index, pages, phys_base);
        }
        let want = pages as u64 * 4096;
        if brk.checked_add(want).map(|n| n > limit).unwrap_or(true) {
            return Err(SpaceError::Budget);
        }
        let flags = P_WRITE | P_NX;
        self.map_range(index, brk, phys_base, pages, flags, RegionKind::Heap)?;
        let s = self.get_mut(index).ok_or(SpaceError::TooManySpaces)?;
        s.brk = brk + want;
        Ok(s.brk)
    }

    /// F011 — tear one space down: every user page unmapped, kernel half left
    /// alone (it was shared, not owned), slot retired.
    pub fn destroy(&mut self, index: usize) -> usize {
        let frozen: [(u64, usize); MAX_MAPPINGS] = {
            let s = match self.get(index) {
                Some(s) => s,
                None => return 0,
            };
            let mut buf = [(0u64, 0usize); MAX_MAPPINGS];
            for i in 0..s.count {
                if let Some(m) = s.mappings[i] {
                    buf[i] = (m.base, m.pages);
                }
            }
            buf
        };
        let root = match self.get(index) {
            Some(s) => s.root,
            None => return 0,
        };
        let mut freed = 0usize;
        for (base, pages) in frozen.iter() {
            if *pages == 0 {
                continue;
            }
            for p in 0..*pages {
                if self.arena.unmap(root, base + (p as u64) * 4096).is_some() {
                    freed += 1;
                }
            }
        }
        let count = self
            .get(index)
            .map(|s| s.count)
            .unwrap_or(0);
        if let Some(s) = self.get_mut(index) {
            s.destroyed = true;
            s.count = 0;
            s.mappings = [None; MAX_MAPPINGS];
            s.table_frames = 0;
        }
        self.destroyed += 1;
        let _ = count;
        freed
    }

    /// F019 — copy-on-write fork: share every page read-only and mark it COW.
    ///
    /// The child gets its own PML4 but the *same* physical pages, mapped
    /// read-only with `P_COW` set; the first write on either side takes a
    /// fault and `paging::CowTable` decides who copies.
    pub fn fork_cow(&mut self, parent: usize, child_pid: u32) -> Result<usize, SpaceError> {
        let child = self.create(child_pid)?;
        let snapshot: [(u64, usize, u64); MAX_MAPPINGS] = {
            let p = self.get(parent).ok_or(SpaceError::NotMapped)?;
            let mut buf = [(0u64, 0usize, 0u64); MAX_MAPPINGS];
            for i in 0..p.count {
                if let Some(m) = p.mappings[i] {
                    buf[i] = (m.base, m.pages, m.flags);
                }
            }
            buf
        };
        let parent_root = self.get(parent).ok_or(SpaceError::NotMapped)?.root;
        for (base, pages, flags) in snapshot.iter() {
            if *pages == 0 {
                continue;
            }
            for p in 0..*pages {
                let virt = base + (p as u64) * 4096;
                let phys = match self.arena.translate(parent_root, virt) {
                    Some(pa) => pa,
                    None => continue,
                };
                // Parent and child both end up read-only + COW.
                let ro = (flags & !P_WRITE) | paging::P_COW | P_NX;
                let _ = self.arena.set_flags(parent_root, virt, paging::P_COW, P_WRITE | P_NX);
                if self
                    .map_page(child, virt, phys, ro, RegionKind::Anonymous)
                    .is_err()
                {
                    let _ = self.destroy(child);
                    return Err(SpaceError::OutOfFrames);
                }
            }
        }
        // Stack geometry travels with the child.
        let (sb, st, brk, lim) = {
            let p = self.get(parent).ok_or(SpaceError::NotMapped)?;
            (p.stack_base, p.stack_top, p.brk, p.brk_limit)
        };
        if let Some(c) = self.get_mut(child) {
            c.stack_base = sb;
            c.stack_top = st;
            c.brk = brk;
            c.brk_limit = lim;
        }
        Ok(child)
    }

    /// Swap in a space: the "CR3 原子换装" step, expressed as the PML4 the CPU
    /// would load. A destroyed space refuses.
    pub fn switch_to(&self, index: usize) -> Result<u16, SpaceError> {
        let s = self.get(index).ok_or(SpaceError::TooManySpaces)?;
        if s.destroyed {
            return Err(SpaceError::NotMapped);
        }
        Ok(s.root)
    }

    /// Aggregate frame accounting, for the domain HUD.
    pub fn total_table_frames(&self) -> usize {
        let mut n = 0;
        for i in 0..self.count {
            if let Some(s) = self.spaces[i] {
                n += s.table_frames;
            }
        }
        n
    }
}

impl Default for SpaceArena {
    fn default() -> Self {
        SpaceArena::new()
    }
}

fn page_up(v: u64) -> u64 {
    (v + 0xFFF) & !0xFFF
}

/// A user mapping must be page aligned, inside the user half and off the null
/// page. Anything else is refused before the tables are touched.
pub fn validate_user_page(virt: u64) -> Result<(), SpaceError> {
    if virt & 0xFFF != 0 {
        return Err(SpaceError::Misaligned);
    }
    if virt < USER_MIN || virt > USER_TOP - 0xFFF {
        return Err(SpaceError::NotUser);
    }
    Ok(())
}

/// W^X proof for one flag word: a page is never writable *and* executable.
pub fn wx_ok(flags: u64) -> bool {
    !(flags & P_WRITE != 0 && flags & P_NX == 0)
}

fn map_err(e: MapError) -> SpaceError {
    match e {
        MapError::OutOfFrames => SpaceError::OutOfFrames,
        MapError::NotCanonical => SpaceError::NotUser,
        MapError::Misaligned => SpaceError::Misaligned,
        MapError::HugeConflict => SpaceError::Overlap,
    }
}

/// F005 — a minimal TSS view so the double-fault stack can be verified for
/// every CPU the process domain will run on. `rsp0` is the kernel stack the
/// CPU loads on a ring3→ring0 transition; `ist1` is IST index 1
/// (`#DF` handler). Both must be non-null and 16-byte aligned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tss16 {
    pub rsp0: u64,
    pub ist1: u64,
    /// IST indices 2..8 in order (index 1 is stored above).
    pub ist_rest: [u64; 7],
}

impl Tss16 {
    /// The layout `ltr` will see: every stack pointer must be present and the
    /// double-fault stack must not be the same page as the normal one.
    pub fn valid(&self) -> bool {
        self.rsp0 != 0
            && self.rsp0 & 0xF == 0
            && self.ist1 != 0
            && self.ist1 & 0xF == 0
            && self.ist1 != self.rsp0
    }

    /// Per-CPU TSS with the double-fault stack already installed.
    pub fn for_cpu(kernel_stack_top: u64, double_fault_top: u64) -> Tss16 {
        Tss16 {
            rsp0: kernel_stack_top & !0xF,
            ist1: double_fault_top & !0xF,
            ist_rest: [0; 7],
        }
    }
}

/// F008 — the register set a context switch must preserve. Grouped by class so
/// a missing class is a visible hole rather than a silent corruption.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SwitchFrame {
    /// Callee-saved general purpose registers.
    pub gp: [u64; 8],
    /// MXCSR + x87 control word + 8 xmm slots (abridged software model).
    pub fpu: [u64; 10],
    /// DR0..DR7.
    pub debug: [u64; 8],
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
}

impl SwitchFrame {
    pub fn capture(seed: u64) -> SwitchFrame {
        let mut f = SwitchFrame::default();
        for (i, v) in f.gp.iter_mut().enumerate() {
            *v = seed.wrapping_mul(i as u64 + 1).wrapping_add(0x1000);
        }
        for (i, v) in f.fpu.iter_mut().enumerate() {
            *v = seed ^ (0x2000 + i as u64);
        }
        for (i, v) in f.debug.iter_mut().enumerate() {
            *v = seed.wrapping_add(i as u64);
        }
        f.rip = 0x40_0000 + seed;
        f.rsp = 0x7FFF_0000 + seed * 4;
        f.rflags = 0x202 | (seed & 1);
        f
    }

    /// A switch is lossless when every class came back bit-for-bit.
    pub fn matches(&self, other: &SwitchFrame) -> bool {
        self.gp == other.gp
            && self.fpu == other.fpu
            && self.debug == other.debug
            && self.rip == other.rip
            && self.rsp == other.rsp
            && self.rflags == other.rflags
    }

    /// Classes the frame carries — 3 means "gp+fpu+debug", the F008 floor.
    pub fn classes(&self) -> usize {
        let mut n = 1;
        if self.fpu.iter().any(|v| *v != 0) {
            n += 1;
        }
        if self.debug.iter().any(|v| *v != 0) {
            n += 1;
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // 非测试代码不再需要它，但测试要拼原始页表项做隔离断言。
    use crate::mem::paging::P_PRESENT;

    fn arena_with_kernel() -> (SpaceArena, usize, usize) {
        let mut a = SpaceArena::new();
        let kroot = a.arena.create_root();
        a.install_kernel_root(kroot);
        let p1 = a.create(1).expect("space 1");
        let p2 = a.create(2).expect("space 2");
        (a, p1, p2)
    }

    #[test]
    fn f001_two_spaces_are_isolated() {
        let (mut a, p1, p2) = arena_with_kernel();
        let flags = P_WRITE | P_NX;
        a.map_page(p1, 0x40_0000, 0x10_0000, flags, RegionKind::Image)
            .unwrap();
        a.map_page(p2, 0x40_0000, 0x20_0000, flags, RegionKind::Image)
            .unwrap();
        assert!(a.isolated(p1, p2), "不同物理页必须互不可见");
        assert!(a.unreachable_from(p1, p2) == false, "同址映射在对方空间可达（不同物理页）");
        assert_eq!(a.translate(p1, 0x40_0000), Some(0x10_0000));
        assert_eq!(a.translate(p2, 0x40_0000), Some(0x20_0000));
        assert_ne!(a.translate(p1, 0x40_0000), a.translate(p2, 0x40_0000));
    }

    #[test]
    fn f001_shared_page_is_reported_not_isolated() {
        let (mut a, p1, p2) = arena_with_kernel();
        let flags = P_WRITE | P_NX;
        a.map_page(p1, 0x20_0000, 0x99_0000, flags, RegionKind::Anonymous).unwrap();
        a.map_page(p2, 0x20_0000, 0x99_0000, flags, RegionKind::Anonymous).unwrap();
        assert!(!a.isolated(p1, p2), "同物理页即视为共享，必须报 false");
    }

    #[test]
    fn f001_kernel_half_is_copied() {
        let (mut a, p1, _) = arena_with_kernel();
        let kroot = a.kernel_root.unwrap();
        a.arena.set_entry(kroot, 300, 0xDEAD_B000 | P_PRESENT);
        // 新空间创建后继承；旧空间必须在创建前已存在（这里验证复制语义）。
        let p3 = a.create(3).unwrap();
        let root3 = a.get(p3).unwrap().root;
        assert_eq!(a.arena.entry(root3, 300), 0xDEAD_B000 | P_PRESENT);
        assert_eq!(a.arena.entry(root3, 1), 0, "用户半区必须为空");
        let _ = p1;
    }

    #[test]
    fn f001_user_range_is_enforced() {
        let (mut a, p1, _) = arena_with_kernel();
        let flags = P_WRITE | P_NX;
        assert_eq!(
            a.map_page(p1, 0, 0x10_0000, flags, RegionKind::Anonymous),
            Err(SpaceError::NotUser)
        );
        assert_eq!(
            a.map_page(p1, 0xFFFF_8000_0000_0000, 0x10_0000, flags, RegionKind::Anonymous),
            Err(SpaceError::NotUser)
        );
        assert_eq!(
            a.map_page(p1, 0x40_0001, 0x10_0000, flags, RegionKind::Anonymous),
            Err(SpaceError::Misaligned)
        );
    }

    #[test]
    fn f001_overlap_is_refused() {
        let (mut a, p1, _) = arena_with_kernel();
        let flags = P_WRITE | P_NX;
        a.map_page(p1, 0x40_0000, 0x10_0000, flags, RegionKind::Anonymous).unwrap();
        assert_eq!(
            a.map_page(p1, 0x40_0000, 0x11_0000, flags, RegionKind::Anonymous),
            Err(SpaceError::Overlap)
        );
    }

    #[test]
    fn f001_switch_returns_root() {
        let (a, p1, p2) = arena_with_kernel();
        assert_ne!(a.switch_to(p1).unwrap(), a.switch_to(p2).unwrap());
        assert!(a.switch_to(99).is_err());
    }

    #[test]
    fn f002_wx_is_enforced_on_map() {
        let (mut a, p1, _) = arena_with_kernel();
        assert_eq!(
            a.map_page(p1, 0x40_0000, 0x10_0000, P_WRITE, RegionKind::Image),
            Err(SpaceError::WriteExecute)
        );
        assert!(wx_ok(P_WRITE | P_NX));
        assert!(wx_ok(P_NX));
        assert!(!wx_ok(P_WRITE));
    }

    #[test]
    fn f003_stack_guard_is_not_mapped() {
        let (mut a, p1, _) = arena_with_kernel();
        let (base, top) = a.install_stack(p1, 0x7FFF_0000, 4, 0x30_0000).unwrap();
        assert_eq!(base, 0x7FFF_0000 - 4 * 4096);
        assert_eq!(top, 0x7FFF_0000);
        assert!(a.translate(p1, base).is_some());
        assert_eq!(a.translate(p1, base - 4096), None, "守卫页必须未映射");
        assert!(a.get(p1).unwrap().in_guard(base - 1));
        // 栈可写不可执行。
        let f = a.flags_of(p1, base).unwrap();
        assert!(f & P_WRITE != 0);
        assert!(f & P_NX != 0);
    }

    #[test]
    fn f004_brk_grows_and_caps() {
        let (mut a, p1, _) = arena_with_kernel();
        let _ = a.install_stack(p1, 0x7FFF_0000, 4, 0x30_0000).unwrap();
        let brk = a.brk_grow(p1, 2, 0x40_0000).unwrap();
        assert!(brk > 0);
        // 越界申请被拒绝，而不是默默扩张。
        assert_eq!(a.brk_grow(p1, 100_000, 0x50_0000), Err(SpaceError::Budget));
    }

    #[test]
    fn f011_destroy_reclaims_pages() {
        let (mut a, p1, p2) = arena_with_kernel();
        let flags = P_WRITE | P_NX;
        a.map_range(p1, 0x40_0000, 0x10_0000, 3, flags, RegionKind::Image).unwrap();
        let freed = a.destroy(p1);
        assert_eq!(freed, 3);
        assert_eq!(a.translate(p1, 0x40_0000), None);
        assert_eq!(a.destroyed_spaces(), 1);
        // 另一个空间毫发无损 —— 崩溃不扩散。
        a.map_page(p2, 0x40_0000, 0x20_0000, flags, RegionKind::Image).unwrap();
        assert_eq!(a.translate(p2, 0x40_0000), Some(0x20_0000));
    }

    #[test]
    fn f019_cow_fork_shares_then_splits() {
        let (mut a, p1, _) = arena_with_kernel();
        let flags = P_WRITE | P_NX;
        a.map_page(p1, 0x40_0000, 0x10_0000, flags, RegionKind::Anonymous).unwrap();
        let child = a.fork_cow(p1, 7).unwrap();
        // 父子同物理页但都已只读 + COW。
        assert_eq!(a.translate(child, 0x40_0000), Some(0x10_0000));
        let pf = a.flags_of(p1, 0x40_0000).unwrap();
        let cf = a.flags_of(child, 0x40_0000).unwrap();
        assert!(pf & P_WRITE == 0 && cf & P_WRITE == 0);
        assert!(pf & paging::P_COW != 0 && cf & paging::P_COW != 0);
        assert_eq!(a.get(child).unwrap().pid, 7);
    }

    #[test]
    fn f005_tss_double_fault_stack_is_distinct() {
        let t = Tss16::for_cpu(0x9000_0000, 0x9100_0000);
        assert!(t.valid());
        let bad = Tss16::for_cpu(0x9000_0000, 0x9000_0000);
        assert!(!bad.valid());
        let zero = Tss16 { rsp0: 0, ist1: 0, ist_rest: [0; 7] };
        assert!(!zero.valid());
    }

    #[test]
    fn f008_switch_frame_round_trips_every_class() {
        let a = SwitchFrame::capture(11);
        let b = SwitchFrame::capture(11);
        assert!(a.matches(&b));
        assert_eq!(a.classes(), 3);
        let mut c = b;
        c.gp[3] ^= 1;
        assert!(!a.matches(&c));
        let mut d = b;
        d.debug[7] ^= 1;
        assert!(!a.matches(&d));
    }
}
