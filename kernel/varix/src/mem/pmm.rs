//! F051 物理页帧分配器（Bitmap 起步）/ F052 Buddy 分配器 / F070 坏页隔离.
//!
//! Two allocators over one address space, on purpose: the bitmap allocator is
//! what the kernel can afford *before* any allocator exists (a single bitset,
//! no metadata beyond it), and the buddy allocator is what the kernel runs on
//! afterwards because it can hand out 2 MiB and 1 GiB blocks without
//! fragmenting them. Bad frames are quarantined in a third structure so a
//! failing DIMM degrades into "one less page" instead of a mystery crash.

use crate::cpu::sync::SpinProtected;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: usize = 12;
/// Frames the allocator can address: 2 GiB of 4 KiB pages.
pub const MAX_FRAMES: usize = 1 << 19;
pub const MAX_ORDER: usize = 10; // 4 MiB blocks
pub const ORDER_COUNT: usize = MAX_ORDER + 1;
pub const NO_FRAME: u32 = u32::MAX;
/// Largest physical address the allocator tracks (2 GiB).
pub const MAX_PHYS: u64 = (MAX_FRAMES as u64) << PAGE_SHIFT;

/// Generic bitmap over frames — the whole of F051.
#[derive(Clone)]
pub struct FrameBitmap {
    words: [u64; MAX_FRAMES / 64],
    /// Total frames represented (bits beyond this are never touched).
    len: usize,
}

impl FrameBitmap {
    pub const fn new() -> FrameBitmap {
        FrameBitmap {
            words: [0u64; MAX_FRAMES / 64],
            len: 0,
        }
    }

    /// Reset to `len` frames, all clear.
    pub fn reset(&mut self, len: usize) {
        self.len = len.min(MAX_FRAMES);
        for w in self.words.iter_mut() {
            *w = 0;
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, frame: usize) -> bool {
        if frame >= self.len {
            return false;
        }
        self.words[frame / 64] & (1u64 << (frame % 64)) != 0
    }

    pub fn set(&mut self, frame: usize, value: bool) {
        if frame >= self.len {
            return;
        }
        let bit = 1u64 << (frame % 64);
        if value {
            self.words[frame / 64] |= bit;
        } else {
            self.words[frame / 64] &= !bit;
        }
    }

    pub fn count_set(&self) -> usize {
        self.words
            .iter()
            .take(self.len.div_ceil(64))
            .map(|w| w.count_ones() as usize)
            .sum()
    }

    /// First clear bit — the bootstrap "give me any page" operation.
    pub fn first_clear(&self) -> Option<usize> {
        for (i, w) in self.words.iter().enumerate() {
            if *w != u64::MAX {
                let bit = (!*w).trailing_zeros() as usize;
                let idx = i * 64 + bit;
                if idx < self.len {
                    return Some(idx);
                }
            }
        }
        None
    }

    /// First run of `n` consecutive clear bits, at or above `from`.
    pub fn find_run(&self, n: usize, from: usize) -> Option<usize> {
        if n == 0 || n > self.len {
            return None;
        }
        let mut start = from;
        let mut run = 0usize;
        while start + run < self.len {
            if self.get(start + run) {
                start += run + 1;
                run = 0;
            } else {
                run += 1;
                if run == n {
                    return Some(start);
                }
            }
        }
        None
    }
}

impl Default for FrameBitmap {
    fn default() -> FrameBitmap {
        FrameBitmap::new()
    }
}

// ---------------------------------------------------------------------------
// F070 — bad-frame quarantine
// ---------------------------------------------------------------------------

pub const MAX_BAD_FRAMES: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BadFrame {
    pub frame: u32,
    /// Short reason, e.g. "ecc-ue", "boot-self-test", "dma-desync".
    pub reason: &'static str,
}

pub struct BadFrames {
    entries: [BadFrame; MAX_BAD_FRAMES],
    len: usize,
}

impl BadFrames {
    pub const fn new() -> BadFrames {
        BadFrames {
            entries: [BadFrame {
                frame: 0,
                reason: "",
            }; MAX_BAD_FRAMES],
            len: 0,
        }
    }

    pub fn mark(&mut self, frame: u32, reason: &'static str) -> bool {
        if self.contains(frame) || self.len >= MAX_BAD_FRAMES {
            return false;
        }
        self.entries[self.len] = BadFrame { frame, reason };
        self.len += 1;
        true
    }

    pub fn contains(&self, frame: u32) -> bool {
        self.entries[..self.len].iter().any(|e| e.frame == frame)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> Option<BadFrame> {
        if i < self.len {
            Some(self.entries[i])
        } else {
            None
        }
    }
}

impl Default for BadFrames {
    fn default() -> BadFrames {
        BadFrames::new()
    }
}

// ---------------------------------------------------------------------------
// F052 — buddy allocator
// ---------------------------------------------------------------------------

/// The buddy zone. Free blocks are threaded through `free_next`, which is
/// indexed by frame number — the same trick real kernels use (storing the link
/// inside the free page) without needing the page to be mapped first.
pub struct BuddyAllocator {
    /// Head frame per order, `NO_FRAME` when the order is exhausted.
    free_head: [u32; ORDER_COUNT],
    /// Intrusive next pointer for a block's head frame.
    free_next: [u32; MAX_FRAMES],
    /// Order of a free block whose head is this frame; `0xFF` otherwise.
    block_order: [u8; MAX_FRAMES],
    /// Managed-by-this-zone bitset (0 = reserved / not ours).
    managed: FrameBitmap,
    /// First managed frame and how many.
    base: u32,
    count: u32,
    free_frames: u32,
    splits: u32,
    coalesces: u32,
}

impl BuddyAllocator {
    pub const fn new() -> BuddyAllocator {
        BuddyAllocator {
            free_head: [NO_FRAME; ORDER_COUNT],
            free_next: [NO_FRAME; MAX_FRAMES],
            block_order: [0xFF; MAX_FRAMES],
            managed: FrameBitmap::new(),
            base: 0,
            count: 0,
            free_frames: 0,
            splits: 0,
            coalesces: 0,
        }
    }

    /// Adopt `[base, base + count)` as managed free frames. Frames already
    /// marked unusable (reserved, or quarantined in `bad`) are skipped.
    pub fn init(&mut self, base: u32, count: u32, usable: &FrameBitmap, bad: &BadFrames) -> u32 {
        self.free_head = [NO_FRAME; ORDER_COUNT];
        self.block_order.fill(0xFF);
        self.base = base;
        self.count = 0;
        self.free_frames = 0;
        self.splits = 0;
        self.coalesces = 0;
        // Span the whole addressable range: a bitmap with `len == 0` would
        // silently refuse every `set`, leaving the zone with nothing to seed.
        self.managed.reset(MAX_FRAMES);

        let end = (base as usize + count as usize).min(MAX_FRAMES);
        for f in base as usize..end {
            if !usable.get(f) || bad.contains(f as u32) {
                continue;
            }
            self.managed.set(f, true);
            self.count += 1;
        }

        // Seed the free lists by decomposing each *contiguous managed run* into
        // the largest aligned power-of-two blocks that fit inside it. A block
        // that straddled an unmanaged frame would be a latent corruption bug,
        // so the run boundary is respected before any order is chosen.
        let mut cursor = base as usize;
        while cursor < end {
            if !self.managed.get(cursor) {
                cursor += 1;
                continue;
            }
            let mut run = 0usize;
            while cursor + run < end && self.managed.get(cursor + run) {
                run += 1;
            }
            let mut off = 0usize;
            while off < run {
                let mut order = MAX_ORDER;
                while order > 0 {
                    let size = 1usize << order;
                    if size > run - off || (cursor + off) % size != 0 {
                        order -= 1;
                    } else {
                        break;
                    }
                }
                let size = 1usize << order;
                self.push_free((cursor + off) as u32, order);
                self.free_frames += size as u32;
                off += size;
            }
            cursor += run.max(1);
        }
        self.free_frames
    }

    fn push_free(&mut self, frame: u32, order: usize) {
        self.free_next[frame as usize] = self.free_head[order];
        self.free_head[order] = frame;
        self.block_order[frame as usize] = order as u8;
    }

    fn pop_free(&mut self, order: usize) -> Option<u32> {
        let head = self.free_head[order];
        if head == NO_FRAME {
            return None;
        }
        self.free_head[order] = self.free_next[head as usize];
        self.free_next[head as usize] = NO_FRAME;
        self.block_order[head as usize] = 0xFF;
        Some(head)
    }

    /// True when `frame` is the head of a free block of `order`.
    pub fn is_free_block(&self, frame: u32, order: usize) -> bool {
        frame < MAX_FRAMES as u32 && self.block_order[frame as usize] == order as u8
    }

    pub fn is_managed(&self, frame: u32) -> bool {
        self.managed.get(frame as usize)
    }

    pub fn base(&self) -> u32 {
        self.base
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn free_frames(&self) -> u32 {
        self.free_frames
    }

    pub fn free_bytes(&self) -> u64 {
        self.free_frames as u64 * PAGE_SIZE as u64
    }

    pub fn splits(&self) -> u32 {
        self.splits
    }

    pub fn coalesces(&self) -> u32 {
        self.coalesces
    }

    /// Buddy frame index for a block of `order`.
    pub fn buddy_of(frame: u32, order: usize) -> u32 {
        frame ^ (1u32 << order)
    }

    /// Largest order currently available (a cheap fragmentation read-out).
    pub fn largest_free_order(&self) -> Option<usize> {
        (0..ORDER_COUNT).rev().find(|o| self.free_head[*o] != NO_FRAME)
    }

    /// Allocate `2^order` contiguous frames.
    pub fn alloc(&mut self, order: usize) -> Option<u32> {
        if order > MAX_ORDER {
            return None;
        }
        // 1. Exact fit.
        if let Some(f) = self.pop_free(order) {
            self.free_frames -= 1 << order;
            return Some(f);
        }
        // 2. Split a larger block down to the requested order.
        let source = (order + 1..ORDER_COUNT).find(|o| self.free_head[*o] != NO_FRAME)?;
        let block = self.pop_free(source)?;
        let mut cur_order = source;
        while cur_order > order {
            cur_order -= 1;
            let half = block + (1u32 << cur_order);
            self.push_free(half, cur_order);
            self.splits += 1;
        }
        self.free_frames -= 1 << order;
        Some(block)
    }

    /// Return a block, merging with its buddy as far up as it can go.
    pub fn free(&mut self, frame: u32, order: usize) -> bool {
        if order > MAX_ORDER || !self.managed.get(frame as usize) {
            return false;
        }
        let mut cur = frame;
        let mut cur_order = order;
        while cur_order < MAX_ORDER {
            let buddy = Self::buddy_of(cur, cur_order);
            if buddy < self.base || buddy >= self.base + self.count {
                break;
            }
            if !self.is_free_block(buddy, cur_order) {
                break;
            }
            // Unlink the buddy and merge.
            self.unlink(buddy, cur_order);
            self.coalesces += 1;
            cur = cur.min(buddy);
            cur_order += 1;
        }
        self.push_free(cur, cur_order);
        self.free_frames += 1 << order;
        true
    }

    fn unlink(&mut self, frame: u32, order: usize) -> bool {
        let mut prev = NO_FRAME;
        let mut cur = self.free_head[order];
        while cur != NO_FRAME {
            if cur == frame {
                let next = self.free_next[cur as usize];
                if prev == NO_FRAME {
                    self.free_head[order] = next;
                } else {
                    self.free_next[prev as usize] = next;
                }
                self.free_next[cur as usize] = NO_FRAME;
                self.block_order[cur as usize] = 0xFF;
                return true;
            }
            prev = cur;
            cur = self.free_next[cur as usize];
        }
        false
    }

    /// Frames lost to fragmentation: what is free, minus the largest single
    /// block that could be carved out of it.
    pub fn fragmented_frames(&self) -> u32 {
        match self.largest_free_order() {
            Some(order) => self.free_frames.saturating_sub(1 << order),
            None => 0,
        }
    }
}

impl Default for BuddyAllocator {
    fn default() -> BuddyAllocator {
        BuddyAllocator::new()
    }
}

/// F051: the bootstrap bitmap allocator. Single frames only, no metadata.
pub struct FrameBitmapAllocator {
    bitmap: FrameBitmap,
    next_hint: usize,
}

impl FrameBitmapAllocator {
    pub const fn new() -> FrameBitmapAllocator {
        FrameBitmapAllocator {
            bitmap: FrameBitmap::new(),
            next_hint: 0,
        }
    }

    pub fn init(&mut self, frames: usize) {
        self.bitmap.reset(frames);
        self.next_hint = 0;
    }

    pub fn alloc(&mut self) -> Option<u32> {
        let f = self.bitmap.find_run(1, self.next_hint).or_else(|| {
            self.next_hint = 0;
            self.bitmap.first_clear()
        })?;
        self.bitmap.set(f, true);
        self.next_hint = f + 1;
        Some(f as u32)
    }

    pub fn free(&mut self, frame: u32) {
        self.bitmap.set(frame as usize, false);
        self.next_hint = self.next_hint.min(frame as usize);
    }

    pub fn used(&self) -> usize {
        self.bitmap.count_set()
    }

    pub fn frames(&self) -> usize {
        self.bitmap.len()
    }
}

impl Default for FrameBitmapAllocator {
    fn default() -> FrameBitmapAllocator {
        FrameBitmapAllocator::new()
    }
}

// ---------------------------------------------------------------------------
// Global zone
// ---------------------------------------------------------------------------

static ZONE: SpinProtected<BuddyAllocator> = SpinProtected::new(BuddyAllocator::new());
static BAD: SpinProtected<BadFrames> = SpinProtected::new(BadFrames::new());
static BOOTSTRAP: SpinProtected<FrameBitmapAllocator> =
    SpinProtected::new(FrameBitmapAllocator::new());

static HANDED_OUT: AtomicU64 = AtomicU64::new(0);
static RECLAIMED: AtomicU64 = AtomicU64::new(0);
static QUARANTINE_EVENTS: AtomicU32 = AtomicU32::new(0);

pub fn zone() -> &'static SpinProtected<BuddyAllocator> {
    &ZONE
}

pub fn bad_frames() -> &'static SpinProtected<BadFrames> {
    &BAD
}

pub fn bootstrap() -> &'static SpinProtected<FrameBitmapAllocator> {
    &BOOTSTRAP
}

// Boot bitmaps. These MUST live in static storage: a `FrameBitmap` is 64 KiB
// (MAX_FRAMES/64 u64 words), and two of them + a clone on the boot stack blew
// past the bootloader's 64 KiB stack, wiping the boot page tables and
// triple-faulting (QEMU, 2026-09-12).
static USABLE_MAP: SpinProtected<FrameBitmap> = SpinProtected::new(FrameBitmap::new());
static RESERVED_MAP: SpinProtected<FrameBitmap> = SpinProtected::new(FrameBitmap::new());
static MERGED_MAP: SpinProtected<FrameBitmap> = SpinProtected::new(FrameBitmap::new());

pub fn usable_map() -> &'static SpinProtected<FrameBitmap> {
    &USABLE_MAP
}

pub fn reserved_map() -> &'static SpinProtected<FrameBitmap> {
    &RESERVED_MAP
}

/// F051+F052+F070 bring-up from the boot memory map.
///
/// `USABLE_MAP` marks the frames the firmware reported as available;
/// `RESERVED_MAP` marks frames the kernel already owns (image, log ring, boot
/// structures). Both are filled by [`crate::mem::boot_bitmaps`] before this
/// runs.
pub fn init() -> u32 {
    let total = USABLE_MAP.lock().len().min(MAX_FRAMES);

    // 1. The bitmap allocator is armed first so anything that needs a page
    //    during bring-up has somewhere to come from.
    {
        let usable = USABLE_MAP.lock();
        let reserved = RESERVED_MAP.lock();
        let mut boot = BOOTSTRAP.lock();
        boot.init(total);
        for f in 0..total {
            if !usable.get(f) || reserved.get(f) {
                boot.mark_unusable(f);
            }
        }
    }

    // 2. The buddy zone takes ownership of everything usable and unreserved.
    //    The merged view is built in static storage — never on the boot stack.
    {
        let mut merged = MERGED_MAP.lock();
        let usable = USABLE_MAP.lock();
        let reserved = RESERVED_MAP.lock();
        merged.reset(total);
        for f in 0..total {
            merged.set(f, usable.get(f) && !reserved.get(f));
        }
    }
    let free = {
        let merged = MERGED_MAP.lock();
        let bad = BAD.lock();
        ZONE.lock().init(0, total as u32, &merged, &bad)
    };

    // Read the resulting state through locals: holding a lock guard inside a
    // multi-argument expression would try to re-lock the same ticket lock.
    let (managed, free_bytes) = {
        let z = ZONE.lock();
        (z.count(), z.free_bytes())
    };
    crate::kinfo!(
        "pmm: {} frames managed, {} free ({} MiB), bitmap+buddy armed",
        managed,
        free,
        free_bytes / (1024 * 1024)
    );
    free
}

/// Allocate one page (order 0) — the common case.
pub fn alloc_page() -> Option<u64> {
    alloc_order(0)
}

/// Allocate `2^order` frames; returns the physical base address.
pub fn alloc_order(order: usize) -> Option<u64> {
    let frame = ZONE.lock().alloc(order)?;
    HANDED_OUT.fetch_add(1 << order, Ordering::Relaxed);
    Some((frame as u64) << PAGE_SHIFT)
}

/// Return frames to the zone.
pub fn free_order(phys: u64, order: usize) -> bool {
    let frame = (phys >> PAGE_SHIFT) as u32;
    if ZONE.lock().free(frame, order) {
        RECLAIMED.fetch_add(1 << order, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// F070: retire a frame from service. Never fails silently — a duplicate mark
/// is reported by the return value.
pub fn quarantine(frame: u32, reason: &'static str) -> bool {
    let ok = BAD.lock().mark(frame, reason);
    if ok {
        QUARANTINE_EVENTS.fetch_add(1, Ordering::Relaxed);
        crate::kwarn!("pmm: frame {} quarantined ({})", frame, reason);
    }
    ok
}

pub fn is_quarantined(frame: u32) -> bool {
    BAD.lock().contains(frame)
}

pub fn quarantined() -> usize {
    BAD.lock().len()
}

pub fn quarantine_events() -> u32 {
    QUARANTINE_EVENTS.load(Ordering::Relaxed)
}

pub fn frames_handed_out() -> u64 {
    HANDED_OUT.load(Ordering::Relaxed)
}

pub fn frames_reclaimed() -> u64 {
    RECLAIMED.load(Ordering::Relaxed)
}

/// Bytes of physical memory currently owned by someone.
pub fn used_bytes() -> u64 {
    let managed = ZONE.lock().count() as u64;
    let free = ZONE.lock().free_frames() as u64;
    managed.saturating_sub(free) * PAGE_SIZE as u64
}

pub fn free_bytes() -> u64 {
    ZONE.lock().free_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A "usable" bitmap: a set bit means the firmware reported the frame as
    /// RAM. `unusable` lists frames to carve back out (holes, reserved areas).
    fn bitmap_of(len: usize, unusable: &[usize]) -> FrameBitmap {
        let mut b = FrameBitmap::new();
        b.reset(len);
        for i in 0..len {
            b.set(i, true);
        }
        for &i in unusable {
            b.set(i, false);
        }
        b
    }

    #[test]
    fn bitmap_set_get_run() {
        let mut b = FrameBitmap::new();
        b.reset(256);
        assert_eq!(b.len(), 256);
        assert!(!b.get(0));
        b.set(0, true);
        assert!(b.get(0));
        assert_eq!(b.first_clear(), Some(1));
        b.set(1, true);
        b.set(2, true);
        assert_eq!(b.find_run(1, 0), Some(3));
        assert_eq!(b.find_run(4, 0), Some(3));
        b.set(3, true);
        assert_eq!(b.find_run(4, 0), Some(4));
        assert_eq!(b.count_set(), 4);
        // Out-of-range access is inert, not UB.
        b.set(999, true);
        assert!(!b.get(999));
        b.reset(0);
        assert!(b.is_empty());
    }

    #[test]
    fn bootstrap_allocator_reuses_freed_frames() {
        let mut a = FrameBitmapAllocator::new();
        a.init(64);
        assert_eq!(a.alloc(), Some(0));
        assert_eq!(a.alloc(), Some(1));
        a.free(0);
        assert_eq!(a.alloc(), Some(0));
        assert_eq!(a.used(), 2);
        assert_eq!(a.frames(), 64);
    }

    #[test]
    fn buddy_splits_and_coalesces() {
        static Z: SpinProtected<BuddyAllocator> = SpinProtected::new(BuddyAllocator::new());
        let mut z = Z.lock();
        let usable = bitmap_of(1024, &[]);
        let bad = BadFrames::new();
        let free = z.init(0, 1024, &usable, &bad);
        assert_eq!(free, 1024);
        assert_eq!(z.largest_free_order(), Some(10));

        // Order 0 comes out of a split, leaving the rest fragmented.
        let a = z.alloc(0).unwrap();
        assert_eq!(a, 0);
        assert_eq!(z.splits(), 10);
        assert_eq!(z.free_frames(), 1023);
        assert!(z.largest_free_order().is_some());

        // Order 3 = 8 contiguous frames, aligned.
        let b = z.alloc(3).unwrap();
        assert_eq!(b % 8, 0);
        assert_eq!(z.free_frames(), 1015);

        // Handing both back restores the single big block.
        assert!(z.free(a, 0));
        assert!(z.free(b, 3));
        assert_eq!(z.free_frames(), 1024);
        assert_eq!(z.largest_free_order(), Some(10));
        assert_eq!(z.coalesces(), 10);
        assert_eq!(z.fragmented_frames(), 0);
    }

    #[test]
    fn buddy_skips_quarantined_and_reserved_frames() {
        static Z2: SpinProtected<BuddyAllocator> = SpinProtected::new(BuddyAllocator::new());
        let mut z = Z2.lock();
        let usable = bitmap_of(512, &[]);
        let mut bad = BadFrames::new();
        assert!(bad.mark(0, "ecc-ue"));
        assert!(!bad.mark(0, "duplicate"));
        assert_eq!(bad.len(), 1);
        let free = z.init(0, 512, &usable, &bad);
        // Frame 0 is inside the first order-9 block, so it is skipped entirely
        // rather than partially: the zone starts one block later.
        assert!(free < 512);
        assert!(!z.is_managed(0));
        assert!(z.is_managed(1));
        assert!(bad.contains(0));
        assert_eq!(bad.get(0).unwrap().reason, "ecc-ue");
    }

    #[test]
    fn buddy_rejects_unmanaged_frames() {
        static Z3: SpinProtected<BuddyAllocator> = SpinProtected::new(BuddyAllocator::new());
        let mut z = Z3.lock();
        let usable = bitmap_of(256, &[5, 6, 7]);
        let bad = BadFrames::new();
        z.init(0, 256, &usable, &bad);
        assert!(!z.free(5, 0), "reserved frame must not be freed");
        assert!(z.alloc(0).is_some());
        // Order beyond MAX_ORDER is refused, not clamped.
        assert!(z.alloc(MAX_ORDER + 1).is_none());
    }

    #[test]
    fn buddy_allocates_high_orders_without_fragmenting() {
        static Z4: SpinProtected<BuddyAllocator> = SpinProtected::new(BuddyAllocator::new());
        let mut z = Z4.lock();
        let usable = bitmap_of(2048, &[]);
        let bad = BadFrames::new();
        z.init(0, 2048, &usable, &bad);
        // One 2 MiB block (order 9) per allocation, all of them.
        let blocks: usize = 2048 / (1 << 9);
        let mut got = [0u32; 8];
        for i in 0..blocks {
            let f = z.alloc(9).expect("aligned 2 MiB block");
            // Free lists are LIFO, so the order addresses come out is not the
            // contract — alignment and non-overlap are.
            assert_eq!(f as usize % (1 << 9), 0, "2 MiB blocks are aligned");
            assert!(!got[..i].contains(&f), "block handed out twice");
            got[i] = f;
        }
        assert!(z.alloc(9).is_none(), "nothing left in one piece");
        assert_eq!(z.free_frames(), 0);
        for slot in got.iter().take(blocks) {
            assert!(z.free(*slot, 9));
        }
        assert_eq!(z.free_frames(), 2048);
    }

    #[test]
    fn quarantine_is_bounded_and_deduplicated() {
        let mut b = BadFrames::new();
        assert!(b.is_empty());
        for i in 0..(MAX_BAD_FRAMES + 10) {
            let _ = b.mark(i as u32, "test");
        }
        assert_eq!(b.len(), MAX_BAD_FRAMES);
        assert!(b.contains(0));
        assert!(!b.contains(MAX_BAD_FRAMES as u32 + 100));
        assert!(b.get(MAX_BAD_FRAMES).is_none());
    }

    #[test]
    fn buddy_buddy_math() {
        assert_eq!(BuddyAllocator::buddy_of(0, 0), 1);
        assert_eq!(BuddyAllocator::buddy_of(1, 0), 0);
        assert_eq!(BuddyAllocator::buddy_of(4, 2), 0);
        assert_eq!(BuddyAllocator::buddy_of(0, 2), 4);
        assert_eq!(BuddyAllocator::buddy_of(8, 3), 0);
    }
}

impl FrameBitmapAllocator {
    /// Mark a frame as permanently unavailable to the bootstrap allocator.
    pub fn mark_unusable(&mut self, frame: usize) {
        self.bitmap.set(frame, true);
    }
}
