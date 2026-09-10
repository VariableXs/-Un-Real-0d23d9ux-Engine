//! F056 页表管理 / F057 高半区映射 / F058 按需调页 / F059 写时复制 CoW /
//! F060 大页支持 / F068 栈溢出守卫 / F071 页级 KASLR.
//!
//! The page table is implemented as real four-level table math over an arena of
//! 4 KiB frames, so the walk, the huge-page alignment rules and the CoW
//! decision are all exercised on the host instead of being asserted only in
//! QEMU. A wrong `PMask` is the kind of bug that shows up as random corruption
//! hours later; it is worth being able to unit-test it.

use crate::mem::pmm::PAGE_SIZE;

pub const ENTRIES: usize = 512;
pub const ENTRY_BYTES: usize = 8;
/// Table frames the in-memory walker can address (256 KiB).
pub const MAX_TABLE_FRAMES: usize = 64;

/// Bits that mean something on x86_64.
pub const P_PRESENT: u64 = 1 << 0;
pub const P_WRITE: u64 = 1 << 1;
pub const P_USER: u64 = 1 << 2;
pub const P_PWT: u64 = 1 << 3;
pub const P_PCD: u64 = 1 << 4;
pub const P_ACCESSED: u64 = 1 << 5;
pub const P_DIRTY: u64 = 1 << 6;
pub const P_HUGE: u64 = 1 << 7;
pub const P_GLOBAL: u64 = 1 << 8;
/// Software bit: page is shared and must be copied on write (F059).
pub const P_COW: u64 = 1 << 9;
pub const P_NX: u64 = 1 << 63;
/// Physical address bits, page-aligned.
pub const P_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// Higher-half base. Everything at or above this is kernel space (F057).
pub const KERNEL_BASE: u64 = 0xFFFF_8000_0000_0000;
/// Highest canonical user address.
pub const USER_TOP: u64 = 0x0000_7FFF_FFFF_FFFF;
/// HHDM offset used by the kernel's direct map.
pub const HHDM_OFFSET: u64 = 0xFFFF_8000_0000_0000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Pml4 = 4,
    Pdpt = 3,
    Pd = 2,
    Pt = 1,
}

impl Level {
    /// Shift applied to the virtual address for this level's index.
    pub fn shift(self) -> u32 {
        match self {
            Level::Pml4 => 39,
            Level::Pdpt => 30,
            Level::Pd => 21,
            Level::Pt => 12,
        }
    }

    pub fn index(self, virt: u64) -> usize {
        ((virt >> self.shift()) & 0x1FF) as usize
    }

    /// All levels, top down.
    pub const ALL: [Level; 4] = [Level::Pml4, Level::Pdpt, Level::Pd, Level::Pt];
}

/// The four indices of a virtual address, plus its page offset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AddressParts {
    pub pml4: usize,
    pub pdpt: usize,
    pub pd: usize,
    pub pt: usize,
    pub offset: usize,
}

impl AddressParts {
    pub fn of(virt: u64) -> AddressParts {
        AddressParts {
            pml4: Level::Pml4.index(virt),
            pdpt: Level::Pdpt.index(virt),
            pd: Level::Pd.index(virt),
            pt: Level::Pt.index(virt),
            offset: (virt & 0xFFF) as usize,
        }
    }
}

/// A canonical x86_64 address: bits 63..48 must all equal bit 47.
pub fn is_canonical(virt: u64) -> bool {
    let sign = virt >> 47;
    sign == 0 || sign == 0x1_FFFF
}

pub fn is_kernel(virt: u64) -> bool {
    virt >= KERNEL_BASE
}

pub fn is_user(virt: u64) -> bool {
    virt <= USER_TOP
}

/// Direct-map translation (F057). The HHDM maps all of physical memory at a
/// fixed offset, so this is pure arithmetic — no walk required.
pub fn phys_to_virt(phys: u64) -> u64 {
    phys + HHDM_OFFSET
}

pub fn virt_to_phys(virt: u64) -> Option<u64> {
    if virt >= HHDM_OFFSET {
        Some(virt - HHDM_OFFSET)
    } else {
        None
    }
}

pub fn page_align_down(v: u64) -> u64 {
    v & !(PAGE_SIZE as u64 - 1)
}

pub fn page_align_up(v: u64) -> u64 {
    page_align_down(v + PAGE_SIZE as u64 - 1)
}

// ---------------------------------------------------------------------------
// F060 — huge pages
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HugeSize {
    /// 4 KiB
    Small,
    /// 2 MiB — set at PD level.
    Huge2M,
    /// 1 GiB — set at PDPT level.
    Huge1G,
}

impl HugeSize {
    pub const fn bytes(self) -> u64 {
        match self {
            HugeSize::Small => PAGE_SIZE as u64,
            HugeSize::Huge2M => 2 * 1024 * 1024,
            HugeSize::Huge1G => 1024 * 1024 * 1024,
        }
    }

    /// The level whose entry carries the huge bit for this size.
    pub const fn level(self) -> Option<Level> {
        match self {
            HugeSize::Small => None,
            HugeSize::Huge2M => Some(Level::Pd),
            HugeSize::Huge1G => Some(Level::Pdpt),
        }
    }

    pub fn alignment_ok(self, phys: u64) -> bool {
        phys % self.bytes() == 0
    }
}

// ---------------------------------------------------------------------------
// F056 — the four-level table
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapError {
    /// No table frame left in the arena.
    OutOfFrames,
    /// Address is not canonical.
    NotCanonical,
    /// Physical address does not satisfy the huge-page alignment rule.
    Misaligned,
    /// A huge mapping already covers this address range.
    HugeConflict,
}

pub struct TableArena {
    frames: [[u64; ENTRIES]; MAX_TABLE_FRAMES],
    used: [bool; MAX_TABLE_FRAMES],
}

impl TableArena {
    pub const fn new() -> TableArena {
        TableArena {
            frames: [[0u64; ENTRIES]; MAX_TABLE_FRAMES],
            used: [false; MAX_TABLE_FRAMES],
        }
    }

    /// Allocate a zeroed table frame. Frame 0 is reserved as the root.
    pub fn alloc_table(&mut self) -> Option<u16> {
        for i in 1..MAX_TABLE_FRAMES {
            if !self.used[i] {
                self.used[i] = true;
                self.frames[i] = [0u64; ENTRIES];
                return Some(i as u16);
            }
        }
        None
    }

    /// Claim frame 0 as the PML4 and return it.
    pub fn create_root(&mut self) -> u16 {
        self.used[0] = true;
        self.frames[0] = [0u64; ENTRIES];
        0
    }

    pub fn entry(&self, table: u16, index: usize) -> u64 {
        self.frames[table as usize][index]
    }

    pub fn set_entry(&mut self, table: u16, index: usize, value: u64) {
        self.frames[table as usize][index] = value;
    }

    pub fn used_frames(&self) -> usize {
        self.used.iter().filter(|u| **u).count()
    }

    /// Map one page (or one huge page) at `virt` to `phys`.
    pub fn map(
        &mut self,
        root: u16,
        virt: u64,
        phys: u64,
        flags: u64,
        size: HugeSize,
    ) -> Result<(), MapError> {
        if !is_canonical(virt) {
            return Err(MapError::NotCanonical);
        }
        if size != HugeSize::Small && !size.alignment_ok(phys) {
            return Err(MapError::Misaligned);
        }
        // Which level carries the leaf for this size, and does it need the
        // huge bit? Deciding this up front means a 1 GiB mapping never allocates
        // the page tables it will not use.
        let (target, leaf_flags) = match size {
            HugeSize::Small => (Level::Pt, flags),
            HugeSize::Huge2M => (Level::Pd, flags | P_HUGE),
            HugeSize::Huge1G => (Level::Pdpt, flags | P_HUGE),
        };

        let mut table = root;
        for level in [Level::Pml4, Level::Pdpt, Level::Pd] {
            if level == target {
                break;
            }
            let idx = level.index(virt);
            let e = self.entry(table, idx);
            if e & P_PRESENT == 0 {
                let next = self.alloc_table().ok_or(MapError::OutOfFrames)?;
                self.set_entry(table, idx, (next as u64) << 12 | P_PRESENT | P_WRITE);
                table = next;
            } else if e & P_HUGE != 0 {
                return Err(MapError::HugeConflict);
            } else {
                table = ((e & P_ADDR_MASK) >> 12) as u16;
            }
        }

        let value = (phys & P_ADDR_MASK) | leaf_flags | P_PRESENT | P_USER;
        self.set_entry(table, target.index(virt), value);
        Ok(())
    }

    /// Remove a mapping; returns the physical address that was mapped.
    pub fn unmap(&mut self, root: u16, virt: u64) -> Option<u64> {
        let mut table = root;
        for level in [Level::Pml4, Level::Pdpt, Level::Pd] {
            let e = self.entry(table, level.index(virt));
            if e & P_PRESENT == 0 {
                return None;
            }
            if e & P_HUGE != 0 {
                // A huge mapping is unmapped at its own level.
                self.set_entry(table, level.index(virt), 0);
                return Some(e & P_ADDR_MASK);
            }
            table = ((e & P_ADDR_MASK) >> 12) as u16;
        }
        let idx = Level::Pt.index(virt);
        let e = self.entry(table, idx);
        if e & P_PRESENT == 0 {
            return None;
        }
        self.set_entry(table, idx, 0);
        Some(e & P_ADDR_MASK)
    }

    /// Walk to the entry covering `virt`; returns (entry, level) — the level
    /// matters because a huge page terminates the walk early.
    pub fn walk(&self, root: u16, virt: u64) -> Option<(u64, Level)> {
        let mut table = root;
        for level in [Level::Pml4, Level::Pdpt, Level::Pd] {
            let e = self.entry(table, level.index(virt));
            if e & P_PRESENT == 0 {
                return None;
            }
            if e & P_HUGE != 0 {
                return Some((e, level));
            }
            table = ((e & P_ADDR_MASK) >> 12) as u16;
        }
        let e = self.entry(table, Level::Pt.index(virt));
        if e & P_PRESENT == 0 {
            None
        } else {
            Some((e, Level::Pt))
        }
    }

    /// Translate to a physical address, honouring huge mappings.
    pub fn translate(&self, root: u16, virt: u64) -> Option<u64> {
        let (e, level) = self.walk(root, virt)?;
        let base = e & P_ADDR_MASK;
        let offset = match level {
            Level::Pt => virt & 0xFFF,
            Level::Pd => virt & 0x1F_FFFF,   // 2 MiB
            Level::Pdpt => virt & 0x3FFF_FFFF, // 1 GiB
            Level::Pml4 => return None,
        };
        Some(base + offset)
    }

    /// Rewrite the flags of an existing entry, at whatever level terminates the
    /// walk for `virt` (a huge page terminates early).
    pub fn set_flags(&mut self, root: u16, virt: u64, add: u64, remove: u64) -> bool {
        let (entry, level) = match self.walk(root, virt) {
            Some(v) => v,
            None => return false,
        };
        // Walk down to the table that holds the entry.
        let mut table = root;
        for l in [Level::Pml4, Level::Pdpt, Level::Pd] {
            if l == level {
                break;
            }
            let e = self.entry(table, l.index(virt));
            table = ((e & P_ADDR_MASK) >> 12) as u16;
        }
        self.set_entry(table, level.index(virt), (entry | add) & !remove);
        true
    }
}

impl Default for TableArena {
    fn default() -> TableArena {
        TableArena::new()
    }
}

// ---------------------------------------------------------------------------
// F058 — demand paging
// ---------------------------------------------------------------------------

/// Decoded page-fault error code (the same bits F030 explains in words).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FaultError {
    pub present: bool,
    pub write: bool,
    pub user: bool,
    pub reserved_write: bool,
    pub instruction_fetch: bool,
}

impl FaultError {
    pub const fn from_bits(bits: u64) -> FaultError {
        FaultError {
            present: bits & 1 != 0,
            write: bits & 2 != 0,
            user: bits & 4 != 0,
            reserved_write: bits & 8 != 0,
            instruction_fetch: bits & 16 != 0,
        }
    }
}

/// What the fault handler should do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DemandAction {
    /// Map a fresh zero page (anonymous memory touched for the first time).
    MapZero,
    /// Copy the shared page, then grant write access (F059).
    CopyOnWrite,
    /// Pull the page back from swap (F062).
    SwapIn,
    /// Grow the stack by one page (F068).
    GrowStack,
    /// Not ours: hand off to the signal path (F109).
    Fault,
}

/// What the policy is allowed to do for a given region.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub lazy: bool,
    pub cow: bool,
    pub swap_backed: bool,
    pub guard: bool,
    pub stack: bool,
}

impl Region {
    pub const fn anonymous() -> Region {
        Region {
            lazy: true,
            cow: false,
            swap_backed: false,
            guard: false,
            stack: false,
        }
    }
    pub const fn shared() -> Region {
        Region {
            lazy: false,
            cow: true,
            swap_backed: false,
            guard: false,
            stack: false,
        }
    }
    pub const fn stack() -> Region {
        Region {
            lazy: true,
            cow: false,
            swap_backed: false,
            guard: false,
            stack: true,
        }
    }
    pub const fn guard() -> Region {
        Region {
            lazy: false,
            cow: false,
            swap_backed: false,
            guard: true,
            stack: false,
        }
    }
}

/// F058: the whole decision, as a pure function.
pub fn decide(region: Region, err: FaultError, touched: bool) -> DemandAction {
    // A guard page is never resolved — that is the point of a guard page.
    if region.guard {
        return DemandAction::Fault;
    }
    if err.reserved_write {
        return DemandAction::Fault;
    }
    if region.stack && !touched {
        return DemandAction::GrowStack;
    }
    if err.present && err.write && region.cow {
        return DemandAction::CopyOnWrite;
    }
    if !err.present {
        if region.swap_backed {
            return DemandAction::SwapIn;
        }
        if region.lazy {
            return DemandAction::MapZero;
        }
    }
    DemandAction::Fault
}

// ---------------------------------------------------------------------------
// F059 — copy on write
// ---------------------------------------------------------------------------

pub const MAX_COW_FRAMES: usize = 256;

/// Reference counts for frames that are currently shared.
pub struct CowTable {
    frames: [u32; MAX_COW_FRAMES],
    counts: [u32; MAX_COW_FRAMES],
    len: usize,
    copies: u64,
    shared_hits: u64,
}

impl CowTable {
    pub const fn new() -> CowTable {
        CowTable {
            frames: [0; MAX_COW_FRAMES],
            counts: [0; MAX_COW_FRAMES],
            len: 0,
            copies: 0,
            shared_hits: 0,
        }
    }

    /// Mark a frame as shared; returns the new reference count.
    pub fn share(&mut self, phys: u64) -> u32 {
        let key = (phys >> 12) as u32;
        for i in 0..self.len {
            if self.frames[i] == key {
                self.counts[i] += 1;
                return self.counts[i];
            }
        }
        if self.len < MAX_COW_FRAMES {
            self.frames[self.len] = key;
            self.counts[self.len] = 2; // the original owner + the new sharer
            self.len += 1;
        }
        2
    }

    pub fn refcount(&self, phys: u64) -> u32 {
        let key = (phys >> 12) as u32;
        self.frames[..self.len]
            .iter()
            .position(|f| *f == key)
            .map(|i| self.counts[i])
            .unwrap_or(1)
    }

    /// One owner goes away. Returns the remaining count.
    pub fn release(&mut self, phys: u64) -> u32 {
        let key = (phys >> 12) as u32;
        for i in 0..self.len {
            if self.frames[i] == key {
                self.counts[i] = self.counts[i].saturating_sub(1);
                return self.counts[i];
            }
        }
        0
    }

    /// Decide what a write fault on a shared page requires.
    pub fn write_fault(&mut self, phys: u64) -> CowOutcome {
        let rc = self.refcount(phys);
        if rc <= 1 {
            // Sole owner: just flip the writable bit, no copy.
            self.shared_hits += 1;
            CowOutcome::GrantWrite
        } else {
            self.copies += 1;
            CowOutcome::CopyThenWrite
        }
    }

    pub fn copies(&self) -> u64 {
        self.copies
    }

    pub fn sole_owner_grants(&self) -> u64 {
        self.shared_hits
    }

    pub fn tracked(&self) -> usize {
        self.len
    }
}

impl Default for CowTable {
    fn default() -> CowTable {
        CowTable::new()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CowOutcome {
    /// A copy is required before the write may proceed.
    CopyThenWrite,
    /// No copy needed — clear P_COW and set P_WRITE.
    GrantWrite,
}

/// The flag transition a CoW resolution performs on the *new* copy.
pub fn cow_flags_for_new_copy(entry: u64) -> u64 {
    (entry & !P_COW) | P_WRITE
}

/// The flag transition for the *original* page when a copy is made: it stays
/// read-only but is no longer marked CoW (it now has a single owner).
pub fn cow_flags_for_original(entry: u64) -> u64 {
    (entry & !P_WRITE) & !P_COW
}

pub fn mark_cow(entry: u64) -> u64 {
    (entry & !P_WRITE) | P_COW
}

pub fn is_cow(entry: u64) -> bool {
    entry & P_COW != 0
}

// ---------------------------------------------------------------------------
// F068 — stack overflow guard
// ---------------------------------------------------------------------------

pub const MAX_GUARDS: usize = 32;

/// A guard page. Touching it is a bug, and the fault handler must say so
/// instead of quietly mapping a page and hiding the overflow.
pub struct GuardPages {
    starts: [u64; MAX_GUARDS],
    len: usize,
    hits: u64,
}

impl GuardPages {
    pub const fn new() -> GuardPages {
        GuardPages {
            starts: [0; MAX_GUARDS],
            len: 0,
            hits: 0,
        }
    }

    pub fn add(&mut self, addr: u64) -> bool {
        if self.len >= MAX_GUARDS || self.contains(addr) {
            return false;
        }
        self.starts[self.len] = page_align_down(addr);
        self.len += 1;
        true
    }

    pub fn contains(&self, addr: u64) -> bool {
        let page = page_align_down(addr);
        self.starts[..self.len].iter().any(|s| *s == page)
    }

    pub fn note_hit(&mut self) {
        self.hits += 1;
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Lay out a kernel stack: one guard page directly below `[base, base+size)`.
    /// Refuses a region smaller than four pages — that is a typo, not a stack.
    pub fn install_stack(&mut self, base: u64, size: u64) -> Option<u64> {
        if size < 4 * PAGE_SIZE as u64 {
            return None;
        }
        let guard = page_align_down(base.saturating_sub(PAGE_SIZE as u64));
        if self.add(guard) {
            Some(guard)
        } else {
            None
        }
    }
}

impl Default for GuardPages {
    fn default() -> GuardPages {
        GuardPages::new()
    }
}

// ---------------------------------------------------------------------------
// F071 — page-level KASLR
// ---------------------------------------------------------------------------

/// Page-granular randomization window. 64 KiB of jitter is enough to break
/// "the image always starts at 0x…0000" assumptions without making the kernel
/// image's address space layout unpredictable to itself.
pub const PAGE_KASLR_WINDOW: u64 = 64 * 1024;

/// Derive a page-aligned jitter offset from entropy, confined to the window.
pub fn page_jitter(entropy: u64) -> u64 {
    (entropy % (PAGE_KASLR_WINDOW / PAGE_SIZE as u64)) * PAGE_SIZE as u64
}

/// Apply per-region jitter to a base address. Each region gets an independent
/// draw, so two regions never move together (which would give the game away).
pub fn jitter_base(base: u64, entropy: u64, region_index: u64) -> u64 {
    let mixed = entropy
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(17)
        .wrapping_add(region_index.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    base + page_jitter(mixed)
}

/// A randomization plan: one offset per region, all page-aligned.
#[derive(Clone, Copy, Debug, Default)]
pub struct KaslrPlan {
    pub regions: u32,
    pub first: u64,
    pub last: u64,
}

impl KaslrPlan {
    pub fn build(entropy: u64, regions: u32) -> KaslrPlan {
        if regions == 0 {
            return KaslrPlan::default();
        }
        let first = page_jitter(entropy);
        let last = page_jitter(
            entropy
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .rotate_left(31)
                .wrapping_add(regions as u64),
        );
        KaslrPlan {
            regions,
            first,
            last,
        }
    }

    /// All offsets are page-aligned and inside the window — the invariant the
    /// boot self-test checks.
    pub fn valid(&self) -> bool {
        self.regions == 0
            || (self.first % PAGE_SIZE as u64 == 0
                && self.last % PAGE_SIZE as u64 == 0
                && self.first < PAGE_KASLR_WINDOW
                && self.last < PAGE_KASLR_WINDOW)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::sync::SpinProtected;

    #[test]
    fn address_index_math() {
        let v = 0xFFFF_8000_1234_5678u64;
        let p = AddressParts::of(v);
        assert_eq!(p.pml4, ((v >> 39) & 0x1FF) as usize);
        assert_eq!(p.pdpt, ((v >> 30) & 0x1FF) as usize);
        assert_eq!(p.pd, ((v >> 21) & 0x1FF) as usize);
        assert_eq!(p.pt, ((v >> 12) & 0x1FF) as usize);
        assert_eq!(p.offset, 0x678);
        assert!(is_canonical(v));
        assert!(!is_canonical(0x0001_0000_0000_0000));
        assert!(is_canonical(0x0000_7FFF_FFFF_FFFF));
    }

    #[test]
    fn high_half_helpers() {
        assert!(is_kernel(KERNEL_BASE));
        assert!(!is_kernel(KERNEL_BASE - 1));
        assert!(is_user(USER_TOP));
        assert!(!is_user(USER_TOP + 1));
        assert_eq!(phys_to_virt(0x1000), HHDM_OFFSET + 0x1000);
        assert_eq!(virt_to_phys(HHDM_OFFSET + 0x1000), Some(0x1000));
        assert_eq!(virt_to_phys(0x1000), None);
        assert_eq!(page_align_down(0x1FFF), 0x1000);
        assert_eq!(page_align_up(0x1000), 0x1000);
        assert_eq!(page_align_up(0x1001), 0x2000);
    }

    #[test]
    fn walk_allocates_tables_and_translates() {
        static T: SpinProtected<TableArena> = SpinProtected::new(TableArena::new());
        let mut a = T.lock();
        let root = a.create_root();
        let virt = 0xFFFF_8000_0000_1000u64;
        let phys = 0x20_0000u64;
        assert_eq!(
            a.map(root, virt, phys, P_WRITE, HugeSize::Small),
            Ok(())
        );
        assert_eq!(a.translate(root, virt), Some(phys));
        assert_eq!(a.translate(root, virt + 0xFFF), Some(phys + 0xFFF));
        assert_eq!(a.translate(root, 0xFFFF_8000_0000_2000), None);
        assert_eq!(a.used_frames(), 4, "root + pdpt + pd + pt");
        assert_eq!(a.unmap(root, virt), Some(phys));
        assert_eq!(a.translate(root, virt), None);
        // Unmapping something that is not mapped is a no-op, not a panic.
        assert_eq!(a.unmap(root, virt), None);
    }

    #[test]
    fn map_rejects_non_canonical_and_misaligned_huge_pages() {
        static T: SpinProtected<TableArena> = SpinProtected::new(TableArena::new());
        let mut a = T.lock();
        let root = a.create_root();
        assert_eq!(
            a.map(root, 0x0001_0000_0000_0000, 0x1000, P_WRITE, HugeSize::Small),
            Err(MapError::NotCanonical)
        );
        assert_eq!(
            a.map(root, 0xFFFF_8000_0000_0000, 0x10_0000, P_WRITE, HugeSize::Huge2M),
            Err(MapError::Misaligned),
            "2 MiB pages must be 2 MiB aligned"
        );
    }

    #[test]
    fn huge_pages_report_their_level_and_offset() {
        assert_eq!(HugeSize::Small.level(), None);
        assert_eq!(HugeSize::Huge2M.level(), Some(Level::Pd));
        assert_eq!(HugeSize::Huge1G.level(), Some(Level::Pdpt));
        assert_eq!(HugeSize::Huge2M.bytes(), 2 * 1024 * 1024);
        assert!(HugeSize::Huge1G.alignment_ok(0x4000_0000));
        assert!(!HugeSize::Huge1G.alignment_ok(0x2000_0000));

        static T: SpinProtected<TableArena> = SpinProtected::new(TableArena::new());
        let mut a = T.lock();
        let root = a.create_root();
        let virt = 0xFFFF_9000_0000_0000u64;
        assert_eq!(a.map(root, virt, 0x4000_0000, P_WRITE, HugeSize::Huge1G), Ok(()));
        assert_eq!(a.translate(root, virt + 0x1234), Some(0x4000_0000 + 0x1234));
        let (_, level) = a.walk(root, virt).unwrap();
        assert_eq!(level, Level::Pdpt);
        // Flags can be rewritten on a huge mapping too.
        assert!(a.set_flags(root, virt, 0, P_WRITE));
        let (e, _) = a.walk(root, virt).unwrap();
        assert_eq!(e & P_WRITE, 0);
    }

    #[test]
    fn huge_conflict_is_detected() {
        static T: SpinProtected<TableArena> = SpinProtected::new(TableArena::new());
        let mut a = T.lock();
        let root = a.create_root();
        let virt = 0xFFFF_9000_0000_0000u64;
        assert_eq!(a.map(root, virt, 0x4000_0000, P_WRITE, HugeSize::Huge1G), Ok(()));
        assert_eq!(
            a.map(root, virt + 0x1000, 0x1000, P_WRITE, HugeSize::Small),
            Err(MapError::HugeConflict)
        );
    }

    #[test]
    fn demand_paging_decisions() {
        let none = FaultError::default();
        assert_eq!(
            decide(Region::anonymous(), none, false),
            DemandAction::MapZero
        );
        assert_eq!(
            decide(Region::stack(), none, false),
            DemandAction::GrowStack
        );
        // A shared page that is written to needs a CoW copy.
        let wr = FaultError::from_bits(0x3);
        assert!(wr.present && wr.write);
        assert_eq!(
            decide(Region::shared(), wr, true),
            DemandAction::CopyOnWrite
        );
        // A swap-backed anonymous page comes back from swap.
        let mut r = Region::anonymous();
        r.swap_backed = true;
        assert_eq!(decide(r, none, true), DemandAction::SwapIn);
        // The guard page never resolves.
        assert_eq!(
            decide(Region::guard(), FaultError::from_bits(0x3), true),
            DemandAction::Fault
        );
        // A reserved-bit write violation is always a real fault.
        assert_eq!(
            decide(Region::anonymous(), FaultError::from_bits(0x8), false),
            DemandAction::Fault
        );
        // A read fault on a present page is not a demand-paging case.
        assert_eq!(
            decide(Region::anonymous(), FaultError::from_bits(0x5), true),
            DemandAction::Fault
        );
    }

    #[test]
    fn cow_reference_counting_and_flags() {
        let mut t = CowTable::new();
        let phys = 0x30_0000u64;
        assert_eq!(t.refcount(phys), 1);
        assert_eq!(t.share(phys), 2);
        assert_eq!(t.share(phys), 3);
        assert_eq!(t.refcount(phys), 3);
        assert_eq!(t.write_fault(phys), CowOutcome::CopyThenWrite);
        assert_eq!(t.release(phys), 2);
        assert_eq!(t.release(phys), 1);
        assert_eq!(t.write_fault(phys), CowOutcome::GrantWrite);
        assert_eq!(t.copies(), 1);
        assert_eq!(t.sole_owner_grants(), 1);
        assert_eq!(t.tracked(), 1);

        let entry = P_PRESENT | P_USER | 0x30_0000;
        let cow = mark_cow(entry);
        assert!(is_cow(cow));
        assert_eq!(cow & P_WRITE, 0, "shared pages are read-only");
        let fresh = cow_flags_for_new_copy(cow);
        assert_eq!(fresh & P_WRITE, P_WRITE);
        assert!(!is_cow(fresh));
        let original = cow_flags_for_original(cow);
        assert_eq!(original & P_WRITE, 0);
        assert!(!is_cow(original));
    }

    #[test]
    fn guard_pages_bound_a_stack() {
        let mut g = GuardPages::new();
        assert!(g.is_empty());
        let base = 0xFFFF_8000_0010_0000u64;
        let guard = g.install_stack(base, 64 * 1024).unwrap();
        assert_eq!(guard, base - PAGE_SIZE as u64);
        assert!(g.contains(guard));
        assert!(!g.contains(base));
        assert!(!g.add(guard), "the same guard is not installed twice");
        g.note_hit();
        assert_eq!(g.hits(), 1);
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn page_kaslr_stays_page_aligned_and_bounded() {
        for entropy in [0u64, 1, 0xDEAD_BEEF, u64::MAX] {
            let j = page_jitter(entropy);
            assert_eq!(j % PAGE_SIZE as u64, 0);
            assert!(j < PAGE_KASLR_WINDOW);
        }
        let plan = KaslrPlan::build(0x1234_5678_9ABC_DEF0, 8);
        assert_eq!(plan.regions, 8);
        assert!(plan.valid());
        // Two regions must not move in lockstep.
        let a = jitter_base(KERNEL_BASE, 0xABCD, 0);
        let b = jitter_base(KERNEL_BASE, 0xABCD, 1);
        assert_ne!(a, b);
        assert_eq!(a % PAGE_SIZE as u64, 0);
        assert!(KaslrPlan::build(1, 0).valid());
    }
}
