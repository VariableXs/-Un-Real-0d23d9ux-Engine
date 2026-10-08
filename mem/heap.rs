//! F053 slab 缓存 / F054 内核堆 GlobalAlloc / F055 泄漏检测器.
//!
//! The kernel heap is a slab: a fixed arena carved into 4 KiB blocks, each
//! block owned by exactly one size class, with the free list threaded through
//! the free slots themselves. That gives O(1) allocation with no metadata per
//! object and — crucially — a completely predictable memory footprint, which is
//! what a kernel needs and a general-purpose allocator cannot promise.
//!
//! Every allocation is also journaled (F055) with its call site, so "where did
//! this 64 bytes come from" is answerable without a debugger.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::cpu::sync::SpinProtected;

/// Slab granularity. Every block handed out to a class is exactly this big.
pub const BLOCK_SIZE: usize = 4096;
/// Arena size — 256 KiB, i.e. 64 blocks. Small on purpose: a kernel heap that
/// can grow unboundedly hides leaks instead of surfacing them.
pub const ARENA_BYTES: usize = 256 * 1024;
pub const MAX_BLOCKS: usize = ARENA_BYTES / BLOCK_SIZE;
/// Largest single allocation the slab serves. Larger requests go to the page
/// allocator (F051/F052), which is the correct owner for page-sized objects.
pub const MAX_ALLOC: usize = 4096;

/// Size classes, in bytes. Powers of two keep the mapping cheap.
pub const CLASS_SIZES: [usize; 9] = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096];
pub const CLASSES: usize = CLASS_SIZES.len();
pub const NO_SLOT: u16 = u16::MAX;
pub const NO_BLOCK: u8 = 0xFF;

/// Size class for a request, or `None` when it must go to the page allocator.
pub fn class_for(size: usize) -> Option<usize> {
    if size == 0 || size > MAX_ALLOC {
        return None;
    }
    CLASS_SIZES.iter().position(|s| *s >= size)
}

/// Slots in a block of `class`.
pub fn slots_in(class: usize) -> usize {
    BLOCK_SIZE / CLASS_SIZES[class]
}

/// Bytes actually consumed by a `size` request (slot rounding).
pub fn slot_size(size: usize) -> Option<usize> {
    class_for(size).map(|c| CLASS_SIZES[c])
}

// ---------------------------------------------------------------------------
// F053 — the slab
// ---------------------------------------------------------------------------

/// Slots in the largest (16-byte) class → 256, i.e. four `u64` words.
pub const SLOT_WORDS: usize = 4;

pub struct SlabHeap {
    arena: [u8; ARENA_BYTES],
    /// Class owning each block (`NO_BLOCK` = still in the arena's free pool).
    block_class: [u8; MAX_BLOCKS],
    /// Head slot index of each block's free list.
    block_head: [u16; MAX_BLOCKS],
    /// Free slot count per block (0..=slots_in(class)).
    block_free: [u16; MAX_BLOCKS],
    /// Which slots are currently handed out — the double-free guard.
    slot_used: [[u64; SLOT_WORDS]; MAX_BLOCKS],
    /// Free-list head for blocks that belong to no class yet.
    arena_head: u8,
    /// Per-class block list heads.
    class_head: [u8; CLASSES],
    /// Intrusive next-block pointer, indexed by block id.
    block_next: [u8; MAX_BLOCKS],
    /// Live slot count per class.
    live_slots: [u32; CLASSES],
    /// Blocks currently assigned to a class.
    class_blocks: [u16; CLASSES],
    /// Refused frees: a pointer that was not allocated, or was freed twice.
    double_frees: u32,
    bad_frees: u32,
}

impl SlabHeap {
    /// Construct a fully armed heap: every arena block is already in the free
    /// pool, so the slab works even if `init` is never called (which is what
    /// the unit tests rely on, and what saves the boot path from a chicken-and
    /// -egg problem if something allocates early).
    pub const fn new() -> SlabHeap {
        let mut heap = SlabHeap {
            arena: [0u8; ARENA_BYTES],
            block_class: [NO_BLOCK; MAX_BLOCKS],
            block_head: [NO_SLOT; MAX_BLOCKS],
            block_free: [0; MAX_BLOCKS],
            slot_used: [[0u64; SLOT_WORDS]; MAX_BLOCKS],
            arena_head: NO_BLOCK,
            class_head: [NO_BLOCK; CLASSES],
            block_next: [NO_BLOCK; MAX_BLOCKS],
            live_slots: [0; CLASSES],
            class_blocks: [0; CLASSES],
            double_frees: 0,
            bad_frees: 0,
        };
        let mut i = 0usize;
        while i < MAX_BLOCKS {
            heap.block_next[i] = if i + 1 < MAX_BLOCKS {
                (i + 1) as u8
            } else {
                NO_BLOCK
            };
            i += 1;
        }
        heap.arena_head = 0;
        heap
    }

    fn slot_is_used(&self, block: u8, slot: usize) -> bool {
        self.slot_used[block as usize][slot / 64] & (1u64 << (slot % 64)) != 0
    }

    fn set_slot_used(&mut self, block: u8, slot: usize, used: bool) {
        let bit = 1u64 << (slot % 64);
        if used {
            self.slot_used[block as usize][slot / 64] |= bit;
        } else {
            self.slot_used[block as usize][slot / 64] &= !bit;
        }
    }

    /// Link every arena block into the free pool. Idempotent enough to be safe
    /// to call twice (the heap is stateless until the first allocation).
    pub fn init(&mut self) {
        // A block with no class owns no slots: the free list head only becomes
        // meaningful once a class claims the block.
        for i in 0..MAX_BLOCKS {
            self.block_class[i] = NO_BLOCK;
            self.block_head[i] = NO_SLOT;
            self.block_free[i] = 0;
            self.slot_used[i] = [0u64; SLOT_WORDS];
            self.block_next[i] = if i + 1 < MAX_BLOCKS {
                (i + 1) as u8
            } else {
                NO_BLOCK
            };
        }
        self.arena_head = 0;
        self.class_head = [NO_BLOCK; CLASSES];
        self.live_slots = [0; CLASSES];
        self.class_blocks = [0; CLASSES];
        self.double_frees = 0;
        self.bad_frees = 0;
    }

    /// Frees the slab refused (bad pointer/size) and double frees it caught.
    pub fn double_frees(&self) -> u32 {
        self.double_frees
    }

    pub fn bad_frees(&self) -> u32 {
        self.bad_frees
    }

    fn block_carve(&mut self) -> Option<u8> {
        let b = self.arena_head;
        if b == NO_BLOCK {
            return None;
        }
        self.arena_head = self.block_next[b as usize];
        self.block_next[b as usize] = NO_BLOCK;
        Some(b)
    }

    fn block_release(&mut self, block: u8) {
        self.block_class[block as usize] = NO_BLOCK;
        self.block_head[block as usize] = NO_SLOT;
        self.block_free[block as usize] = 0;
        self.block_next[block as usize] = self.arena_head;
        self.arena_head = block;
    }

    /// Claim a block for `class` and thread its slots into a free list.
    fn block_assign(&mut self, class: usize) -> Option<u8> {
        let b = self.block_carve()?;
        let n = slots_in(class);
        self.block_class[b as usize] = class as u8;
        self.block_free[b as usize] = n as u16;
        self.block_head[b as usize] = 0;
        // A recycled block must start with no slot marked live.
        self.slot_used[b as usize] = [0u64; SLOT_WORDS];
        // Thread the slots: slot i's first two bytes hold i+1 (or NO_SLOT).
        for i in 0..n {
            let next = if i + 1 < n { (i + 1) as u16 } else { NO_SLOT };
            self.write_slot_link(b, class, i, next);
        }
        self.block_next[b as usize] = self.class_head[class];
        self.class_head[class] = b;
        self.class_blocks[class] += 1;
        Some(b)
    }

    fn slot_offset(&self, block: u8, class: usize, slot: usize) -> usize {
        block as usize * BLOCK_SIZE + slot * CLASS_SIZES[class]
    }

    /// Write the free-list link into the slot's own bytes.
    fn write_slot_link(&mut self, block: u8, class: usize, slot: usize, next: u16) {
        let off = self.slot_offset(block, class, slot);
        self.arena[off..off + 2].copy_from_slice(&next.to_le_bytes());
    }

    fn read_slot_link(&self, block: u8, class: usize, slot: usize) -> u16 {
        let off = self.slot_offset(block, class, slot);
        u16::from_le_bytes([self.arena[off], self.arena[off + 1]])
    }

    /// F053: allocate `size` bytes satisfying `align` (≤ 16).
    pub fn alloc(&mut self, size: usize, align: usize) -> Option<(*mut u8, usize)> {
        let class = class_for(size)?;
        // Slots are 16-byte aligned and sized in multiples of 16, so any
        // alignment up to 16 comes for free.
        if align > 16 {
            return None;
        }
        let mut block = self.class_head[class];
        while block != NO_BLOCK && self.block_head[block as usize] == NO_SLOT {
            block = self.block_next[block as usize];
        }
        let block = match block {
            NO_BLOCK => self.block_assign(class)?,
            b => b,
        };
        let slot = self.block_head[block as usize];
        if slot == NO_SLOT {
            return None;
        }
        let next = self.read_slot_link(block, class, slot as usize);
        self.block_head[block as usize] = next;
        self.block_free[block as usize] -= 1;
        self.live_slots[class] += 1;
        self.set_slot_used(block, slot as usize, true);
        let off = self.slot_offset(block, class, slot as usize);
        // SAFETY: `off` is inside `arena`, and the block is exclusively owned
        // by `class`, whose slots are handed out one at a time.
        let ptr = unsafe { self.arena.as_mut_ptr().add(off) };
        Some((ptr, CLASS_SIZES[class]))
    }

    /// F053: return a slot. `size` must match the allocation.
    pub fn dealloc(&mut self, ptr: *mut u8, size: usize) -> bool {
        let class = match class_for(size) {
            Some(c) => c,
            None => {
                self.bad_frees += 1;
                return false;
            }
        };
        let base = self.arena.as_ptr() as usize;
        let addr = ptr as usize;
        if addr < base || addr >= base + ARENA_BYTES {
            self.bad_frees += 1;
            return false;
        }
        let off = addr - base;
        if off % CLASS_SIZES[class] != 0 {
            self.bad_frees += 1;
            return false;
        }
        let block = (off / BLOCK_SIZE) as u8;
        if self.block_class[block as usize] as usize != class {
            self.bad_frees += 1;
            return false;
        }
        let slot = (off % BLOCK_SIZE) / CLASS_SIZES[class];
        // F457 territory belongs to the robustness domain, but a slab that
        // silently re-links an already-free slot corrupts the free list, so the
        // cheap per-slot guard lives here.
        if !self.slot_is_used(block, slot) {
            self.double_frees += 1;
            return false;
        }
        self.set_slot_used(block, slot, false);
        let head = self.block_head[block as usize];
        self.write_slot_link(block, class, slot, head);
        self.block_head[block as usize] = slot as u16;
        self.block_free[block as usize] += 1;
        self.live_slots[class] = self.live_slots[class].saturating_sub(1);

        // Fully free block: hand it back so the arena does not fragment.
        if self.block_free[block as usize] as usize == slots_in(class) {
            self.unlink_block(block, class);
            self.block_release(block);
        }
        true
    }

    fn unlink_block(&mut self, block: u8, class: usize) {
        let mut prev = NO_BLOCK;
        let mut cur = self.class_head[class];
        while cur != NO_BLOCK {
            if cur == block {
                let next = self.block_next[cur as usize];
                if prev == NO_BLOCK {
                    self.class_head[class] = next;
                } else {
                    self.block_next[prev as usize] = next;
                }
                self.class_blocks[class] = self.class_blocks[class].saturating_sub(1);
                return;
            }
            prev = cur;
            cur = self.block_next[cur as usize];
        }
    }

    pub fn live_slots(&self, class: usize) -> u32 {
        self.live_slots[class]
    }

    pub fn blocks_for(&self, class: usize) -> u16 {
        self.class_blocks[class]
    }

    pub fn arena_free_blocks(&self) -> u16 {
        let mut n = 0u16;
        let mut b = self.arena_head;
        while b != NO_BLOCK {
            n += 1;
            b = self.block_next[b as usize];
        }
        n
    }

    /// Effectively used bytes (live slots × their slot size).
    pub fn live_bytes(&self) -> usize {
        let mut total = 0usize;
        for c in 0..CLASSES {
            total += self.live_slots[c] as usize * CLASS_SIZES[c];
        }
        total
    }

    /// Bytes held by classes but not currently handed out — the fragmentation
    /// the caller can see. Zero when every block is perfectly packed or free.
    pub fn internal_waste(&self) -> usize {
        let mut total = 0usize;
        for c in 0..CLASSES {
            total += self.class_blocks[c] as usize * BLOCK_SIZE
                - self.live_slots[c] as usize * CLASS_SIZES[c];
        }
        total
    }
}

impl Default for SlabHeap {
    fn default() -> SlabHeap {
        SlabHeap::new()
    }
}

static HEAP: SpinProtected<SlabHeap> = SpinProtected::new(SlabHeap::new());

pub fn heap() -> &'static SpinProtected<SlabHeap> {
    &HEAP
}

// ---------------------------------------------------------------------------
// F055 — leak detector
// ---------------------------------------------------------------------------

pub const MAX_TRACKED: usize = 256;

#[derive(Clone, Copy, Default, Debug)]
pub struct Tracked {
    pub ptr: usize,
    pub size: usize,
    /// Call site label (`"boot"`, `"driver:pci"`, …), `""` when untracked.
    pub site: &'static str,
}

pub struct LeakDetector {
    live: [Tracked; MAX_TRACKED],
    len: usize,
    allocations: u64,
    frees: u64,
    bytes_live: u64,
    peak_bytes: u64,
    peak_slots: usize,
    orphan_frees: u64,
}

impl LeakDetector {
    pub const fn new() -> LeakDetector {
        LeakDetector {
            live: [Tracked {
                ptr: 0,
                size: 0,
                site: "",
            }; MAX_TRACKED],
            len: 0,
            allocations: 0,
            frees: 0,
            bytes_live: 0,
            peak_bytes: 0,
            peak_slots: 0,
            orphan_frees: 0,
        }
    }

    /// Journal an allocation.
    pub fn on_alloc(&mut self, ptr: usize, size: usize, site: &'static str) {
        self.allocations += 1;
        self.bytes_live += size as u64;
        self.peak_bytes = self.peak_bytes.max(self.bytes_live);
        if self.len >= MAX_TRACKED {
            return;
        }
        self.live[self.len] = Tracked { ptr, size, site };
        self.len += 1;
        self.peak_slots = self.peak_slots.max(self.len);
    }

    /// Journal a free. Returns the size that was tracked, or `None` when the
    /// pointer was never seen (a double free, or a heap the detector missed).
    pub fn on_free(&mut self, ptr: usize) -> Option<usize> {
        self.frees += 1;
        for i in 0..self.len {
            if self.live[i].ptr == ptr {
                let size = self.live[i].size;
                let last = self.len - 1;
                self.live[i] = self.live[last];
                self.live[last] = Tracked::default();
                self.len -= 1;
                self.bytes_live = self.bytes_live.saturating_sub(size as u64);
                return Some(size);
            }
        }
        self.orphan_frees += 1;
        None
    }

    pub fn live_slots(&self) -> usize {
        self.len
    }

    pub fn live_bytes(&self) -> u64 {
        self.bytes_live
    }

    pub fn peak_bytes(&self) -> u64 {
        self.peak_bytes
    }

    pub fn peak_slots(&self) -> usize {
        self.peak_slots
    }

    pub fn allocations(&self) -> u64 {
        self.allocations
    }

    pub fn frees(&self) -> u64 {
        self.frees
    }

    /// Frees that had no matching allocation — a double free, most likely.
    pub fn orphan_frees(&self) -> u64 {
        self.orphan_frees
    }

    /// Is anything still outstanding?
    pub fn leaked(&self) -> bool {
        self.len > 0
    }

    pub fn entry(&self, i: usize) -> Option<Tracked> {
        if i < self.len {
            Some(self.live[i])
        } else {
            None
        }
    }

    /// Render the outstanding allocations, newest first, capped at `max`.
    pub fn render_leaks(&self, out: &mut [u8], max: usize) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("heap leaks live=");
        w.num(self.len as u64);
        w.str(" bytes=");
        w.num(self.bytes_live);
        w.str("\n");
        let shown = max.min(self.len);
        for i in 0..shown {
            let e = self.live[self.len - 1 - i];
            w.str("  ");
            w.str(if e.site.is_empty() { "?" } else { e.site });
            w.str(" +");
            w.num(e.size as u64);
            w.str("B\n");
        }
        w.used()
    }
}

impl Default for LeakDetector {
    fn default() -> LeakDetector {
        LeakDetector::new()
    }
}

static TRACKER: SpinProtected<LeakDetector> = SpinProtected::new(LeakDetector::new());
static TOTAL_ALLOCS: AtomicU64 = AtomicU64::new(0);
static TOTAL_FREES: AtomicU64 = AtomicU64::new(0);

pub fn tracker() -> &'static SpinProtected<LeakDetector> {
    &TRACKER
}

// ---------------------------------------------------------------------------
// Public heap API
// ---------------------------------------------------------------------------

/// Allocate `size` bytes with `align` (≤ 16), journaling the call site.
pub fn kmalloc(size: usize, align: usize, site: &'static str) -> Option<*mut u8> {
    let (ptr, _slot) = HEAP.lock().alloc(size, align)?;
    TRACKER.lock().on_alloc(ptr as usize, size, site);
    TOTAL_ALLOCS.fetch_add(1, Ordering::Relaxed);
    Some(ptr)
}

/// Zeroed allocation — the common case for kernel objects.
pub fn kzalloc(size: usize, align: usize, site: &'static str) -> Option<*mut u8> {
    let p = kmalloc(size, align, site)?;
    // SAFETY: `kmalloc` returned a live, uniquely owned slice of `size` bytes.
    unsafe { core::ptr::write_bytes(p, 0, size) };
    Some(p)
}

/// Release an allocation. Returns `false` for a pointer the slab does not own.
pub fn kfree(ptr: *mut u8, size: usize) -> bool {
    let ok = HEAP.lock().dealloc(ptr, size);
    if ok {
        TRACKER.lock().on_free(ptr as usize);
        TOTAL_FREES.fetch_add(1, Ordering::Relaxed);
    }
    ok
}

pub fn total_allocations() -> u64 {
    TOTAL_ALLOCS.load(Ordering::Relaxed)
}

pub fn total_frees() -> u64 {
    TOTAL_FREES.load(Ordering::Relaxed)
}

/// F053/F055 bring-up. Arm the arena and prove the accounting is at zero.
pub fn init() {
    HEAP.lock().init();
    let (blocks, waste) = {
        let h = HEAP.lock();
        (h.arena_free_blocks(), h.internal_waste())
    };
    crate::kinfo!(
        "heap: slab {} KiB, {} blocks free, {} classes, leak-tracker armed (waste={}B)",
        ARENA_BYTES / 1024,
        blocks,
        CLASSES,
        waste
    );
}

// ---------------------------------------------------------------------------
// F054 — GlobalAlloc
// ---------------------------------------------------------------------------

/// The kernel's `GlobalAlloc`. Registering it with `#[global_allocator]` is a
/// one-line change once a consumer needs `alloc::`; the contract (fallible,
/// alignment-aware, leak-journaled) is what this implements and tests.
pub struct VarixAllocator;

// SAFETY: every method either returns null on failure or hands out a uniquely
// owned region from the slab; `dealloc` is the exact inverse of `alloc`.
unsafe impl GlobalAlloc for VarixAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match kmalloc(layout.size(), layout.align(), "global") {
            Some(p) => p,
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        match kzalloc(layout.size(), layout.align(), "global-zeroed") {
            Some(p) => p,
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = kfree(ptr, layout.size());
    }
}

/// The single instance the `#[global_allocator]` attribute would name.
pub static ALLOCATOR: VarixAllocator = VarixAllocator;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_classes_are_monotonic_and_bounded() {
        assert_eq!(class_for(1), Some(0));
        assert_eq!(class_for(16), Some(0));
        assert_eq!(class_for(17), Some(1));
        assert_eq!(class_for(4096), Some(CLASSES - 1));
        assert_eq!(class_for(0), None);
        assert_eq!(class_for(4097), None);
        assert_eq!(slot_size(20), Some(32));
        assert_eq!(slots_in(0), 256);
        assert_eq!(slots_in(CLASSES - 1), 1);
    }

    #[test]
    fn slab_allocates_and_reuses_slots() {
        static H: SpinProtected<SlabHeap> = SpinProtected::new(SlabHeap::new());
        let mut h = H.lock();
        h.init();
        assert_eq!(h.arena_free_blocks(), MAX_BLOCKS as u16);
        let (a, slot) = h.alloc(32, 8).unwrap();
        assert_eq!(slot, 32);
        let (b, _) = h.alloc(32, 8).unwrap();
        assert_ne!(a, b);
        assert_eq!(h.live_slots(1), 2);
        assert_eq!(h.blocks_for(1), 1);
        // Freeing returns the slot to the same block's free list.
        assert!(h.dealloc(a, 32));
        assert_eq!(h.live_slots(1), 1);
        let (c, _) = h.alloc(32, 8).unwrap();
        assert_eq!(c, a, "the freed slot is reused before growing the slab");
        assert_eq!(h.blocks_for(1), 1);
        assert!(h.dealloc(b, 32));
        assert!(h.dealloc(c, 32));
        // A fully free block goes back to the arena.
        assert_eq!(h.blocks_for(1), 0);
        assert_eq!(h.arena_free_blocks(), MAX_BLOCKS as u16);
    }

    #[test]
    fn slab_grows_one_block_at_a_time_then_exhausts() {
        static H: SpinProtected<SlabHeap> = SpinProtected::new(SlabHeap::new());
        let mut h = H.lock();
        h.init();
        // 300 slots of the 16-byte class need two 4 KiB blocks (256 slots each).
        let mut live = [core::ptr::null_mut(); 300];
        for p in live.iter_mut() {
            *p = h.alloc(16, 1).expect("arena has room").0;
        }
        assert_eq!(h.blocks_for(0), 2);
        assert_eq!(h.arena_free_blocks(), MAX_BLOCKS as u16 - 2);
        for p in live.iter() {
            assert!(h.dealloc(*p, 16));
        }
        assert_eq!(h.live_bytes(), 0);
        assert_eq!(h.internal_waste(), 0);

        // Exhaust the whole arena, then prove the refusal path.
        let mut n = 0usize;
        while h.alloc(16, 1).is_some() {
            n += 1;
            assert!(n <= MAX_BLOCKS * 256, "allocator over-delivered");
        }
        assert_eq!(n, MAX_BLOCKS * 256);
        assert!(h.alloc(16, 1).is_none());
        assert_eq!(h.arena_free_blocks(), 0);
    }

    #[test]
    fn slab_detects_double_free() {
        static H: SpinProtected<SlabHeap> = SpinProtected::new(SlabHeap::new());
        let mut h = H.lock();
        h.init();
        let (a, _) = h.alloc(64, 8).unwrap();
        let (b, _) = h.alloc(64, 8).unwrap();
        assert!(h.dealloc(a, 64));
        assert!(!h.dealloc(a, 64), "second free of the same slot is refused");
        assert_eq!(h.double_frees(), 1);
        // The free list is intact: the next allocation still works.
        let (c, _) = h.alloc(64, 8).unwrap();
        assert_eq!(c, a);
        assert!(h.dealloc(b, 64));
        assert!(h.dealloc(c, 64));
        assert_eq!(h.live_slots(2), 0);
    }

    #[test]
    fn slab_rejects_bad_deallocations() {
        static H: SpinProtected<SlabHeap> = SpinProtected::new(SlabHeap::new());
        let mut h = H.lock();
        h.init();
        let (p, _) = h.alloc(64, 8).unwrap();
        assert!(!h.dealloc(p.wrapping_add(8), 64), "unaligned pointer");
        assert!(!h.dealloc(p, 4096), "wrong size class");
        assert!(!h.dealloc(0x1000 as *mut u8, 64), "not from this arena");
        assert!(h.dealloc(p, 64));
        // Alignment beyond 16 belongs to the page allocator.
        assert!(h.alloc(64, 4096).is_none());
    }

    #[test]
    fn slab_waste_is_zero_when_fully_free() {
        static H: SpinProtected<SlabHeap> = SpinProtected::new(SlabHeap::new());
        let mut h = H.lock();
        h.init();
        assert_eq!(h.internal_waste(), 0);
        let (a, _) = h.alloc(100, 4).unwrap();
        let (b, _) = h.alloc(100, 4).unwrap();
        assert_eq!(h.live_bytes(), 256);
        assert!(h.internal_waste() > 0);
        let _ = h.dealloc(a, 100);
        let _ = h.dealloc(b, 100);
        assert_eq!(h.internal_waste(), 0);
    }

    #[test]
    fn leak_detector_tracks_and_reports() {
        let mut d = LeakDetector::new();
        assert!(!d.leaked());
        d.on_alloc(0x1000, 32, "boot");
        d.on_alloc(0x2000, 64, "driver:pci");
        assert_eq!(d.live_slots(), 2);
        assert_eq!(d.live_bytes(), 96);
        assert_eq!(d.peak_bytes(), 96);
        assert!(d.leaked());
        assert_eq!(d.on_free(0x1000), Some(32));
        assert_eq!(d.live_slots(), 1);
        assert_eq!(d.live_bytes(), 64);
        // Double free is counted, not silently accepted.
        assert_eq!(d.on_free(0x1000), None);
        assert_eq!(d.orphan_frees(), 1);
        assert_eq!(d.allocations(), 2);
        assert_eq!(d.frees(), 2);

        let mut out = [0u8; 128];
        let n = d.render_leaks(&mut out, 4);
        let s = core::str::from_utf8(&out[..n]).unwrap();
        assert!(s.starts_with("heap leaks live=1 bytes=64"), "got {s}");
        assert!(s.contains("driver:pci +64B"), "got {s}");
    }

    #[test]
    fn leak_detector_bounds_its_table() {
        let mut d = LeakDetector::new();
        for i in 0..(MAX_TRACKED + 20) {
            d.on_alloc(0x1000 + i, 8, "x");
        }
        assert_eq!(d.live_slots(), MAX_TRACKED);
        // Bytes still account for the untracked ones.
        assert_eq!(d.live_bytes(), (MAX_TRACKED as u64 + 20) * 8);
        assert_eq!(d.peak_slots(), MAX_TRACKED);
    }

    #[test]
    fn kmalloc_and_kfree_account_globally() {
        let before_a = total_allocations();
        let before_f = total_frees();
        let p = kmalloc(48, 8, "test").expect("heap available");
        assert!(!p.is_null());
        // Other tests share the global heap and counters, so only the direction
        // can be asserted here.
        assert!(total_allocations() > before_a);
        assert!(kfree(p, 48));
        assert!(total_frees() > before_f);
    }

    #[test]
    fn kzalloc_zeroes_the_region() {
        let p = kzalloc(64, 8, "test-zero").expect("heap available");
        // SAFETY: 64 bytes were just handed out exclusively.
        let bytes = unsafe { core::slice::from_raw_parts(p, 64) };
        assert!(bytes.iter().all(|b| *b == 0));
        assert!(kfree(p, 64));
    }

    #[test]
    fn global_allocator_contract() {
        let layout = Layout::from_size_align(24, 8).unwrap();
        // SAFETY: exercising the trait the way `alloc::` would.
        let p = unsafe { ALLOCATOR.alloc(layout) };
        assert!(!p.is_null());
        unsafe { ALLOCATOR.dealloc(p, layout) };
        let zeroed = Layout::from_size_align(16, 16).unwrap();
        let q = unsafe { ALLOCATOR.alloc_zeroed(zeroed) };
        assert!(!q.is_null());
        assert_eq!(unsafe { *q }, 0);
        unsafe { ALLOCATOR.dealloc(q, zeroed) };
        // An impossible request returns null instead of panicking.
        let huge = Layout::from_size_align(1 << 20, 8).unwrap();
        assert!(unsafe { ALLOCATOR.alloc(huge) }.is_null());
    }
}
