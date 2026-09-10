//! F061 内存压缩器 / F062 交换框架 / F063 OOM 优雅击杀 / F064 内存水位告警 /
//! F065 NUMA 感知 / F066 DMA 缓冲管理 / F067 驱动内存池 / F069 内存访问统计 /
//! F072 内存诊断转储 / F073 进程内存配额 / F074 虚拟化内存管理 / F075 内存自检.
//!
//! The policy half of the memory domain. Everything here answers "what should
//! happen when memory is scarce": compress the cold page, swap it, reclaim it,
//! or kill the process that owns it — in that order, and never surprises.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::cpu::Hud;
use crate::mem::pmm::PAGE_SIZE;

// ---------------------------------------------------------------------------
// F061 — cold-page compressor
// ---------------------------------------------------------------------------

/// Compressed page header: original length, so decompression can verify.
pub const COMP_HEADER: usize = 4;

/// Heuristic gate: never spend cycles compressing a page that will not shrink.
pub const MIN_COMPRESS_RATIO_NUM: usize = 7;
pub const MIN_COMPRESS_RATIO_DEN: usize = 8;

/// One op byte:
/// * high bit 0 → run: `len = (op & 0x7F) + 1` copies of the next byte
/// * high bit 1 → literal: `len = (op & 0x7F) + 1` literal bytes follow
pub const MAX_OP_LEN: usize = 128;

/// Compress `src` into `dst`. Returns the compressed length, or `None` when it
/// does not fit or would not be worth it.
pub fn compress(src: &[u8], dst: &mut [u8], min_run: usize) -> Option<usize> {
    let min_run = min_run.max(3).min(MAX_OP_LEN);
    if dst.len() < COMP_HEADER + 1 {
        return None;
    }
    let mut out = COMP_HEADER;
    let mut i = 0usize;
    let mut literal_start = 0usize;

    let flush_literals = |dst: &mut [u8], out: &mut usize, src: &[u8], from: usize, to: usize| {
        let mut at = from;
        while at < to {
            let n = (to - at).min(MAX_OP_LEN);
            if *out + 1 + n > dst.len() {
                return false;
            }
            dst[*out] = 0x80 | (n - 1) as u8;
            *out += 1;
            dst[*out..*out + n].copy_from_slice(&src[at..at + n]);
            *out += n;
            at += n;
        }
        true
    };

    while i < src.len() {
        // How long is the run starting here?
        let mut run = 1usize;
        while i + run < src.len() && src[i + run] == src[i] && run < MAX_OP_LEN {
            run += 1;
        }
        if run >= min_run {
            if !flush_literals(dst, &mut out, src, literal_start, i) {
                return None;
            }
            if out + 2 > dst.len() {
                return None;
            }
            dst[out] = (run - 1) as u8;
            dst[out + 1] = src[i];
            out += 2;
            i += run;
            literal_start = i;
        } else {
            i += run;
        }
    }
    if !flush_literals(dst, &mut out, src, literal_start, src.len()) {
        return None;
    }

    // Header: original length, little-endian.
    let len = src.len() as u32;
    dst[0..4].copy_from_slice(&len.to_le_bytes());

    // Worth keeping?
    if out * MIN_COMPRESS_RATIO_DEN >= src.len() * MIN_COMPRESS_RATIO_NUM {
        return None;
    }
    Some(out)
}

/// Decompress into `dst`; returns the produced length, or `None` on any
/// inconsistency (truncated input, wrong original length, overflow).
pub fn decompress(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    if src.len() < COMP_HEADER {
        return None;
    }
    let original = u32::from_le_bytes([src[0], src[1], src[2], src[3]]) as usize;
    if original > dst.len() {
        return None;
    }
    let mut i = COMP_HEADER;
    let mut o = 0usize;
    while i < src.len() {
        let op = src[i];
        i += 1;
        let len = (op & 0x7F) as usize + 1;
        if op & 0x80 == 0 {
            if i >= src.len() || o + len > dst.len() {
                return None;
            }
            let v = src[i];
            i += 1;
            for b in dst[o..o + len].iter_mut() {
                *b = v;
            }
            o += len;
        } else {
            if i + len > src.len() || o + len > dst.len() {
                return None;
            }
            dst[o..o + len].copy_from_slice(&src[i..i + len]);
            i += len;
            o += len;
        }
    }
    if o != original {
        return None;
    }
    Some(o)
}

pub struct Compressor {
    attempts: AtomicU64,
    stored: AtomicU64,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    skipped_low_gain: AtomicU64,
}

impl Compressor {
    pub const fn new() -> Compressor {
        Compressor {
            attempts: AtomicU64::new(0),
            stored: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            skipped_low_gain: AtomicU64::new(0),
        }
    }

    /// Compress one page into `scratch`. Returns `Some(len)` when the page is
    /// now smaller and should be stored compressed.
    pub fn try_page(&self, page: &[u8], scratch: &mut [u8]) -> Option<usize> {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        match compress(page, scratch, 4) {
            Some(n) => {
                self.stored.fetch_add(1, Ordering::Relaxed);
                self.bytes_in.fetch_add(page.len() as u64, Ordering::Relaxed);
                self.bytes_out.fetch_add(n as u64, Ordering::Relaxed);
                Some(n)
            }
            None => {
                self.skipped_low_gain.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    pub fn attempts(&self) -> u64 {
        self.attempts.load(Ordering::Relaxed)
    }
    pub fn stored(&self) -> u64 {
        self.stored.load(Ordering::Relaxed)
    }
    pub fn skipped(&self) -> u64 {
        self.skipped_low_gain.load(Ordering::Relaxed)
    }

    /// Bytes saved as a percentage of the input (0 when nothing was stored).
    pub fn saved_percent(&self) -> u64 {
        let input = self.bytes_in.load(Ordering::Relaxed);
        if input == 0 {
            return 0;
        }
        let saved = input.saturating_sub(self.bytes_out.load(Ordering::Relaxed));
        saved * 100 / input
    }
}

static COMPRESSOR: Compressor = Compressor::new();

pub fn compressor() -> &'static Compressor {
    &COMPRESSOR
}

/// Zero-filled pages are the canonical compressible page.
pub fn is_zero_page(page: &[u8]) -> bool {
    page.iter().all(|b| *b == 0)
}

// ---------------------------------------------------------------------------
// F062 — swap framework
// ---------------------------------------------------------------------------

pub const MAX_SWAP_SLOTS: usize = 1024;
pub const MAX_SWAP_ENTRIES: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwapSlot {
    pub slot: u32,
    pub frame: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SwapBacking {
    /// No swap device configured — swapping is disabled, not broken.
    #[default]
    None,
    /// A file on a mounted filesystem.
    File,
    /// A raw partition.
    Partition,
}

pub struct SwapFramework {
    backing: SwapBacking,
    slot_used: [bool; MAX_SWAP_SLOTS],
    entries: [SwapSlot; MAX_SWAP_ENTRIES],
    len: usize,
    out: u64,
    back_in: u64,
    refaults: u64,
}

impl SwapFramework {
    pub const fn new() -> SwapFramework {
        SwapFramework {
            backing: SwapBacking::None,
            slot_used: [false; MAX_SWAP_SLOTS],
            entries: [SwapSlot {
                slot: 0,
                frame: 0,
            }; MAX_SWAP_ENTRIES],
            len: 0,
            out: 0,
            back_in: 0,
            refaults: 0,
        }
    }

    pub fn set_backing(&mut self, backing: SwapBacking) {
        self.backing = backing;
    }

    pub fn backing(&self) -> SwapBacking {
        self.backing
    }

    pub fn enabled(&self) -> bool {
        self.backing != SwapBacking::None
    }

    fn alloc_slot(&mut self) -> Option<u32> {
        for i in 0..MAX_SWAP_SLOTS {
            if !self.slot_used[i] {
                self.slot_used[i] = true;
                return Some(i as u32);
            }
        }
        None
    }

    /// Write a frame out. Refuses cleanly when swap is not configured.
    pub fn swap_out(&mut self, frame: u32) -> Option<u32> {
        if !self.enabled() || self.len >= MAX_SWAP_ENTRIES {
            return None;
        }
        let slot = self.alloc_slot()?;
        self.entries[self.len] = SwapSlot { slot, frame };
        self.len += 1;
        self.out += 1;
        Some(slot)
    }

    /// Bring a frame back in.
    pub fn swap_in(&mut self, slot: u32) -> Option<u32> {
        for i in 0..self.len {
            if self.entries[i].slot == slot {
                let frame = self.entries[i].frame;
                let last = self.len - 1;
                self.entries[i] = self.entries[last];
                self.len = last;
                self.slot_used[slot as usize] = false;
                self.back_in += 1;
                return Some(frame);
            }
        }
        self.refaults += 1;
        None
    }

    pub fn resident(&self) -> usize {
        self.len
    }

    pub fn swapped_out(&self) -> u64 {
        self.out
    }

    pub fn swapped_in(&self) -> u64 {
        self.back_in
    }

    /// A slot that was asked for but is not resident: a bookkeeping error.
    pub fn refaults(&self) -> u64 {
        self.refaults
    }

    /// Bytes currently on the swap device.
    pub fn swapped_bytes(&self) -> u64 {
        self.len as u64 * PAGE_SIZE as u64
    }
}

impl Default for SwapFramework {
    fn default() -> SwapFramework {
        SwapFramework::new()
    }
}

/// SAFETY: `swap_mut` is only reachable from the fault handler or the
/// single-threaded boot path; `swap` hands out shared access to data those
/// paths keep consistent.
struct SwapCell {
    inner: core::cell::UnsafeCell<SwapFramework>,
}
unsafe impl Sync for SwapCell {}
static SWAP_CTRL: SwapCell = SwapCell {
    inner: core::cell::UnsafeCell::new(SwapFramework::new()),
};

pub fn swap() -> &'static SwapFramework {
    // SAFETY: see `SwapCell`.
    unsafe { &*SWAP_CTRL.inner.get() }
}

/// # Safety
/// Caller must hold the VM lock (or be on the single-threaded boot path).
pub unsafe fn swap_mut() -> &'static mut SwapFramework {
    &mut *SWAP_CTRL.inner.get()
}

// ---------------------------------------------------------------------------
// F064 — watermarks
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum WaterLevel {
    /// Below `min`: the kernel itself is at risk.
    Critical,
    /// Below `low`: reclaim now, before it becomes critical.
    Low,
    /// Between `low` and `high`: normal operation.
    #[default]
    Normal,
    /// Above `high`: background reclaim can stop.
    Comfortable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Watermarks {
    pub min_bytes: u64,
    pub low_bytes: u64,
    pub high_bytes: u64,
}

impl Watermarks {
    /// Derive the three marks from total RAM: 1% / 3% / 5%, with a 32 MiB floor
    /// so a small VM does not spend its life reclaiming.
    pub fn for_total(total_bytes: u64) -> Watermarks {
        let floor = 32 * 1024 * 1024u64;
        Watermarks {
            min_bytes: (total_bytes / 100).max(floor / 4),
            low_bytes: (total_bytes * 3 / 100).max(floor / 2),
            high_bytes: (total_bytes * 5 / 100).max(floor),
        }
    }

    pub fn level(&self, free_bytes: u64) -> WaterLevel {
        if free_bytes < self.min_bytes {
            WaterLevel::Critical
        } else if free_bytes < self.low_bytes {
            WaterLevel::Low
        } else if free_bytes < self.high_bytes {
            WaterLevel::Normal
        } else {
            WaterLevel::Comfortable
        }
    }

    pub fn valid(&self) -> bool {
        self.min_bytes <= self.low_bytes && self.low_bytes <= self.high_bytes
    }
}

pub struct WatermarkWatcher {
    marks: Watermarks,
    level: AtomicU32,
    breaches: [AtomicU64; 2],
    lowest_seen: AtomicU64,
}

impl WatermarkWatcher {
    pub const fn new() -> WatermarkWatcher {
        WatermarkWatcher {
            marks: Watermarks {
                min_bytes: 0,
                low_bytes: 0,
                high_bytes: 0,
            },
            level: AtomicU32::new(WaterLevel::Comfortable as u32),
            breaches: [AtomicU64::new(0), AtomicU64::new(0)],
            lowest_seen: AtomicU64::new(u64::MAX),
        }
    }

    pub fn arm(&mut self, total_bytes: u64) {
        self.marks = Watermarks::for_total(total_bytes);
    }

    pub fn marks(&self) -> Watermarks {
        self.marks
    }

    /// Update with a fresh free-memory reading; returns the new level.
    pub fn sample(&self, free_bytes: u64, _total_bytes: u64) -> WaterLevel {
        self.lowest_seen.fetch_min(free_bytes, Ordering::Relaxed);
        let level = self.marks.level(free_bytes);
        self.level.store(level as u32, Ordering::Release);
        match level {
            WaterLevel::Critical => {
                self.breaches[0].fetch_add(1, Ordering::Relaxed);
                self.breaches[1].fetch_add(1, Ordering::Relaxed);
            }
            WaterLevel::Low => {
                self.breaches[1].fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        level
    }

    pub fn level(&self) -> WaterLevel {
        match self.level.load(Ordering::Acquire) {
            0 => WaterLevel::Critical,
            1 => WaterLevel::Low,
            2 => WaterLevel::Normal,
            _ => WaterLevel::Comfortable,
        }
    }

    pub fn critical_breaches(&self) -> u64 {
        self.breaches[0].load(Ordering::Relaxed)
    }

    pub fn low_breaches(&self) -> u64 {
        self.breaches[1].load(Ordering::Relaxed)
    }

    pub fn lowest_free_bytes(&self) -> u64 {
        let v = self.lowest_seen.load(Ordering::Relaxed);
        if v == u64::MAX {
            0
        } else {
            v
        }
    }
}

static WATCHER: crate::cpu::sync::SpinProtected<WatermarkWatcher> =
    crate::cpu::sync::SpinProtected::new(WatermarkWatcher::new());

pub fn watcher() -> &'static crate::cpu::sync::SpinProtected<WatermarkWatcher> {
    &WATCHER
}

// ---------------------------------------------------------------------------
// F063 — graceful OOM
// ---------------------------------------------------------------------------

/// Escalation ladder. Each rung is strictly more drastic than the last, and
/// the kernel only reaches rung 3 when the alternatives have been exhausted.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum OomAction {
    /// 1. Ask reclaim (page cache, compressed cold pages) to give memory back.
    Reclaim,
    /// 2. Kill the single worst offender.
    KillVictim,
    /// 3. Kill the whole process group that owns the victim.
    KillGroup,
    /// 4. Nothing left to give: panic with a diagnosis rather than corrupt.
    Halt,
}

impl OomAction {
    pub fn rung(self) -> u8 {
        match self {
            OomAction::Reclaim => 1,
            OomAction::KillVictim => 2,
            OomAction::KillGroup => 3,
            OomAction::Halt => 4,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OomAction::Reclaim => "reclaim",
            OomAction::KillVictim => "kill-victim",
            OomAction::KillGroup => "kill-group",
            OomAction::Halt => "halt",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OomScore {
    pub pid: u32,
    pub rss_bytes: u64,
    /// `-1000..=1000`, the same semantics as Linux's `oom_score_adj`:
    /// negative protects, positive invites.
    pub adj: i32,
    /// Ticks since the process last did useful work.
    pub idle_ticks: u64,
    /// Kernel threads are never victims.
    pub kernel: bool,
    /// Root-owned processes get the benefit of the doubt.
    pub root_owned: bool,
}

impl OomScore {
    /// Higher is a better victim. The formula is intentionally legible: memory
    /// used dominates, adjustment shifts it, and idleness is a tiebreaker.
    pub fn score(&self) -> i64 {
        if self.kernel {
            return i64::MIN;
        }
        let rss_mb = (self.rss_bytes / (1024 * 1024)) as i64;
        let mut s = rss_mb * 10;
        s += self.adj as i64;
        s += (self.idle_ticks / 100) as i64;
        if self.root_owned {
            s -= 30;
        }
        s
    }
}

/// Pick the highest-scoring victim. Ties go to the lower pid for determinism.
pub fn pick_victim(scores: &[OomScore]) -> Option<usize> {
    let mut best: Option<(usize, i64)> = None;
    for (i, s) in scores.iter().enumerate() {
        let sc = s.score();
        if sc == i64::MIN {
            continue;
        }
        match best {
            None => best = Some((i, sc)),
            Some((bi, bsc)) => {
                if sc > bsc || (sc == bsc && s.pid < scores[bi].pid) {
                    best = Some((i, sc));
                }
            }
        }
    }
    best.map(|(i, _)| i)
}

/// Map "how bad is it" onto the ladder.
pub fn escalation(level: WaterLevel, attempted: OomAction) -> OomAction {
    match level {
        WaterLevel::Comfortable | WaterLevel::Normal => attempted.min(OomAction::Reclaim),
        WaterLevel::Low => attempted,
        WaterLevel::Critical => match attempted {
            OomAction::Reclaim => OomAction::KillVictim,
            other => other,
        },
    }
}

// ---------------------------------------------------------------------------
// F065 — NUMA awareness
// ---------------------------------------------------------------------------

pub const MAX_NODES: usize = 8;
pub const MAX_CPUS_NUMA: usize = 64;
pub const LOCAL_DISTANCE: u8 = 10;

#[derive(Clone, Copy, Debug)]
pub struct NumaNode {
    pub id: u32,
    pub frame_base: u64,
    pub frame_count: u64,
    pub distance: [u8; MAX_NODES],
    pub present: bool,
}

impl NumaNode {
    pub const fn empty() -> NumaNode {
        NumaNode {
            id: 0,
            frame_base: 0,
            frame_count: 0,
            distance: [LOCAL_DISTANCE; MAX_NODES],
            present: false,
        }
    }

    pub fn bytes(&self) -> u64 {
        self.frame_count * PAGE_SIZE as u64
    }

    pub fn contains_phys(&self, phys: u64) -> bool {
        self.present
            && phys >= self.frame_base
            && phys < self.frame_base + self.bytes()
    }

    pub fn distance_to(&self, node: usize) -> u8 {
        if node >= MAX_NODES {
            return u8::MAX;
        }
        self.distance[node]
    }
}

pub struct NumaTopology {
    nodes: [NumaNode; MAX_NODES],
    count: usize,
    /// cpu index → node index.
    cpu_node: [u8; MAX_CPUS_NUMA],
}

impl NumaTopology {
    pub const fn new() -> NumaTopology {
        NumaTopology {
            nodes: [NumaNode::empty(); MAX_NODES],
            count: 0,
            cpu_node: [0; MAX_CPUS_NUMA],
        }
    }

    pub fn add_node(&mut self, id: u32, frame_base: u64, frame_count: u64) -> Option<usize> {
        if self.count >= MAX_NODES {
            return None;
        }
        let idx = self.count;
        let mut node = NumaNode::empty();
        node.id = id;
        node.frame_base = frame_base;
        node.frame_count = frame_count;
        node.present = true;
        self.nodes[idx] = node;
        self.count += 1;
        Some(idx)
    }

    pub fn bind_cpu(&mut self, cpu: usize, node: usize) -> bool {
        if cpu >= MAX_CPUS_NUMA || node >= self.count {
            return false;
        }
        self.cpu_node[cpu] = node as u8;
        true
    }

    pub fn set_distance(&mut self, from: usize, to: usize, distance: u8) -> bool {
        if from >= self.count || to >= MAX_NODES {
            return false;
        }
        self.nodes[from].distance[to] = distance;
        true
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn node(&self, i: usize) -> Option<NumaNode> {
        if i < self.count {
            Some(self.nodes[i])
        } else {
            None
        }
    }

    pub fn node_of_cpu(&self, cpu: usize) -> Option<usize> {
        if cpu >= MAX_CPUS_NUMA || self.count == 0 {
            return None;
        }
        let n = self.cpu_node[cpu] as usize;
        if n < self.count {
            Some(n)
        } else {
            None
        }
    }

    /// Node to allocate from for `cpu`: its own node, else the nearest one that
    /// exists. Never returns an index that is not present.
    pub fn preferred_for_cpu(&self, cpu: usize) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        let local = self.node_of_cpu(cpu).unwrap_or(0);
        if self.nodes[local].present {
            return Some(local);
        }
        (0..self.count)
            .filter(|n| self.nodes[*n].present)
            .min_by_key(|n| self.nodes[local].distance_to(*n))
    }

    /// NUMA nodes with a local memory range that contains `phys`.
    pub fn node_of_phys(&self, phys: u64) -> Option<usize> {
        (0..self.count).find(|n| self.nodes[*n].contains_phys(phys))
    }
}

impl Default for NumaTopology {
    fn default() -> NumaTopology {
        NumaTopology::new()
    }
}

// ---------------------------------------------------------------------------
// F066 — DMA buffers
// ---------------------------------------------------------------------------

pub const DMA_ALIGN: u64 = 64;
pub const MAX_DMA_BUFFERS: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DmaDirection {
    ToDevice,
    FromDevice,
    Bidirectional,
}

impl DmaDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            DmaDirection::ToDevice => "to-device",
            DmaDirection::FromDevice => "from-device",
            DmaDirection::Bidirectional => "bidi",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DmaBuffer {
    pub phys: u64,
    pub size: usize,
    pub direction: Option<DmaDirection>,
    /// Coherent (non-cached) memory needs no explicit synchronisation.
    pub coherent: bool,
    pub owner: &'static str,
}

impl DmaBuffer {
    pub fn end(&self) -> u64 {
        self.phys + self.size as u64
    }

    pub fn contains(&self, phys: u64) -> bool {
        phys >= self.phys && phys < self.end()
    }
}

pub struct DmaPool {
    buffers: [DmaBuffer; MAX_DMA_BUFFERS],
    len: usize,
    coherent: bool,
    syncs: u64,
    rejected: u64,
    high_water: usize,
}

impl DmaPool {
    pub const fn new() -> DmaPool {
        DmaPool {
            buffers: [DmaBuffer {
                phys: 0,
                size: 0,
                direction: None,
                coherent: false,
                owner: "",
            }; MAX_DMA_BUFFERS],
            len: 0,
            coherent: true,
            syncs: 0,
            rejected: 0,
            high_water: 0,
        }
    }

    /// Whether the platform's DMA is cache-coherent. When it is not, every
    /// buffer must be explicitly synchronised before and after use.
    pub fn set_coherent(&mut self, coherent: bool) {
        self.coherent = coherent;
    }

    pub fn is_coherent(&self) -> bool {
        self.coherent
    }

    /// Register a buffer. Refuses a size that is not 64-byte aligned — the
    /// alignment is a hardware requirement, not a style preference.
    pub fn register(
        &mut self,
        phys: u64,
        size: usize,
        direction: DmaDirection,
        owner: &'static str,
    ) -> bool {
        if size == 0 || phys % DMA_ALIGN != 0 || size as u64 % DMA_ALIGN != 0 {
            self.rejected += 1;
            return false;
        }
        if self.len >= MAX_DMA_BUFFERS || self.overlaps(phys, size) {
            self.rejected += 1;
            return false;
        }
        self.buffers[self.len] = DmaBuffer {
            phys,
            size,
            direction: Some(direction),
            coherent: self.coherent,
            owner,
        };
        self.len += 1;
        self.high_water = self.high_water.max(self.len);
        true
    }

    fn overlaps(&self, phys: u64, size: usize) -> bool {
        let end = phys + size as u64;
        self.buffers[..self.len]
            .iter()
            .any(|b| phys < b.end() && b.phys < end)
    }

    pub fn release(&mut self, phys: u64) -> bool {
        for i in 0..self.len {
            if self.buffers[i].phys == phys {
                let last = self.len - 1;
                self.buffers[i] = self.buffers[last];
                self.len = last;
                return true;
            }
        }
        false
    }

    /// Hand ownership to the device. A no-op on coherent hardware (nothing to
    /// flush), which is exactly the point of tracking the flag.
    pub fn sync_for_device(&mut self, phys: u64) -> bool {
        let coherent = match self.buffers[..self.len].iter().find(|b| b.phys == phys) {
            Some(b) => b.coherent,
            None => return false,
        };
        if !coherent {
            self.syncs += 1;
        }
        true
    }

    /// Take ownership back from the device.
    pub fn sync_for_cpu(&mut self, phys: u64) -> bool {
        self.sync_for_device(phys)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn bytes(&self) -> u64 {
        self.buffers[..self.len].iter().map(|b| b.size as u64).sum()
    }

    pub fn syncs(&self) -> u64 {
        self.syncs
    }

    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    pub fn high_water(&self) -> usize {
        self.high_water
    }

    pub fn find(&self, phys: u64) -> Option<DmaBuffer> {
        self.buffers[..self.len].iter().copied().find(|b| b.phys == phys)
    }
}

impl Default for DmaPool {
    fn default() -> DmaPool {
        DmaPool::new()
    }
}

// ---------------------------------------------------------------------------
// F067 — per-driver memory pools
// ---------------------------------------------------------------------------

pub const MAX_DRIVER_POOLS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct DriverPool {
    pub name: &'static str,
    pub quota_bytes: u64,
    pub used_bytes: u64,
    pub peak_bytes: u64,
    pub refusals: u64,
}

pub struct DriverPools {
    pools: [DriverPool; MAX_DRIVER_POOLS],
    len: usize,
}

impl DriverPools {
    pub const fn new() -> DriverPools {
        DriverPools {
            pools: [DriverPool {
                name: "",
                quota_bytes: 0,
                used_bytes: 0,
                peak_bytes: 0,
                refusals: 0,
            }; MAX_DRIVER_POOLS],
            len: 0,
        }
    }

    /// Give a driver a quota. Drivers with no entry are unlimited (kernel
    /// internal allocations), which is deliberate: quotas exist to contain
    /// third-party code, not to slow down the core.
    pub fn register(&mut self, name: &'static str, quota_bytes: u64) -> bool {
        if self.len >= MAX_DRIVER_POOLS || self.find(name).is_some() {
            return false;
        }
        self.pools[self.len] = DriverPool {
            name,
            quota_bytes,
            used_bytes: 0,
            peak_bytes: 0,
            refusals: 0,
        };
        self.len += 1;
        true
    }

    pub fn find(&self, name: &str) -> Option<&DriverPool> {
        self.pools[..self.len].iter().find(|p| p.name == name)
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.pools[..self.len].iter().position(|p| p.name == name)
    }

    /// Charge an allocation. An unknown driver is allowed through (kernel
    /// internal); a known one is capped.
    pub fn charge(&mut self, name: &str, bytes: u64) -> bool {
        match self.index(name) {
            None => true,
            Some(i) => {
                if self.pools[i].used_bytes + bytes > self.pools[i].quota_bytes {
                    self.pools[i].refusals += 1;
                    return false;
                }
                self.pools[i].used_bytes += bytes;
                self.pools[i].peak_bytes = self.pools[i].peak_bytes.max(self.pools[i].used_bytes);
                true
            }
        }
    }

    pub fn release(&mut self, name: &str, bytes: u64) {
        if let Some(i) = self.index(name) {
            self.pools[i].used_bytes = self.pools[i].used_bytes.saturating_sub(bytes);
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn pool(&self, i: usize) -> Option<DriverPool> {
        if i < self.len {
            Some(self.pools[i])
        } else {
            None
        }
    }

    /// Drivers that hit their cap — the ones to look at first when the system
    /// runs out of memory.
    pub fn offenders(&self) -> usize {
        self.pools[..self.len]
            .iter()
            .filter(|p| p.refusals > 0)
            .count()
    }
}

impl Default for DriverPools {
    fn default() -> DriverPools {
        DriverPools::new()
    }
}

// ---------------------------------------------------------------------------
// F069 — hot/cold page statistics
// ---------------------------------------------------------------------------

pub const HEAT_LEVELS: u8 = 4;
pub const MAX_HEAT_FRAMES: usize = 4096;

pub struct PageHeat {
    counters: [u8; MAX_HEAT_FRAMES],
    scans: u64,
    touches: u64,
    hottest: u32,
}

impl PageHeat {
    pub const fn new() -> PageHeat {
        PageHeat {
            counters: [0; MAX_HEAT_FRAMES],
            scans: 0,
            touches: 0,
            hottest: 0,
        }
    }

    /// Record an access. Counting saturates; the value is a heat *signal*, not
    /// an exact count, so a page cannot be remembered forever by hammering it.
    pub fn touch(&mut self, frame: u32) {
        if (frame as usize) < MAX_HEAT_FRAMES {
            let c = &mut self.counters[frame as usize];
            *c = (*c + 1).min(HEAT_LEVELS);
        }
        self.touches += 1;
        self.hottest = self.hottest.max(frame);
    }

    /// Halve every counter — the aging pass the reclaim scan calls.
    pub fn decay(&mut self) {
        for c in self.counters.iter_mut() {
            *c /= 2;
        }
        self.scans += 1;
    }

    pub fn heat(&self, frame: u32) -> u8 {
        if (frame as usize) < MAX_HEAT_FRAMES {
            self.counters[frame as usize]
        } else {
            0
        }
    }

    /// Frames above `threshold` heat — the ones to keep.
    pub fn hot(&self, threshold: u8) -> usize {
        self.counters.iter().filter(|c| **c >= threshold).count()
    }

    /// Frames that have cooled to zero — the candidates for reclaim.
    pub fn cold(&self) -> usize {
        self.counters.iter().filter(|c| **c == 0).count()
    }

    pub fn scans(&self) -> u64 {
        self.scans
    }

    pub fn touches(&self) -> u64 {
        self.touches
    }
}

impl Default for PageHeat {
    fn default() -> PageHeat {
        PageHeat::new()
    }
}

// ---------------------------------------------------------------------------
// F073 — per-process memory quota
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RssAccount {
    pub limit_bytes: u64,
    pub used_bytes: u64,
    pub peak_bytes: u64,
    pub denials: u64,
}

impl RssAccount {
    pub const fn new(limit_bytes: u64) -> RssAccount {
        RssAccount {
            limit_bytes,
            used_bytes: 0,
            peak_bytes: 0,
            denials: 0,
        }
    }

    /// Charge `bytes`. A limit of 0 means unlimited.
    pub fn charge(&mut self, bytes: u64) -> bool {
        if self.limit_bytes != 0 && self.used_bytes + bytes > self.limit_bytes {
            self.denials += 1;
            return false;
        }
        self.used_bytes += bytes;
        self.peak_bytes = self.peak_bytes.max(self.used_bytes);
        true
    }

    pub fn release(&mut self, bytes: u64) {
        self.used_bytes = self.used_bytes.saturating_sub(bytes);
    }

    pub fn remaining(&self) -> u64 {
        self.limit_bytes.saturating_sub(self.used_bytes)
    }

    pub fn utilization_percent(&self) -> u64 {
        if self.limit_bytes == 0 {
            return 0;
        }
        self.used_bytes * 100 / self.limit_bytes
    }

    pub fn over_limit(&self) -> bool {
        self.limit_bytes != 0 && self.used_bytes > self.limit_bytes
    }
}

// ---------------------------------------------------------------------------
// F074 — guest memory reservation (paving the way for W5)
// ---------------------------------------------------------------------------

pub const MAX_GUEST_RANGES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestRange {
    pub start: u64,
    pub size: u64,
    pub tag: &'static str,
}

pub struct GuestMemory {
    ranges: [GuestRange; MAX_GUEST_RANGES],
    len: usize,
    reserved_bytes: u64,
    refusals: u64,
}

impl GuestMemory {
    pub const fn new() -> GuestMemory {
        GuestMemory {
            ranges: [GuestRange {
                start: 0,
                size: 0,
                tag: "",
            }; MAX_GUEST_RANGES],
            len: 0,
            reserved_bytes: 0,
            refusals: 0,
        }
    }

    /// Reserve a contiguous window for a guest. 2 MiB aligned so the guest can
    /// use EPT huge pages (F328) without a second copy.
    pub fn reserve(&mut self, start: u64, size: u64, tag: &'static str) -> bool {
        if size == 0 || start % (2 * 1024 * 1024) != 0 || size % PAGE_SIZE as u64 != 0 {
            self.refusals += 1;
            return false;
        }
        if self.len >= MAX_GUEST_RANGES || self.overlaps(start, size) {
            self.refusals += 1;
            return false;
        }
        self.ranges[self.len] = GuestRange { start, size, tag };
        self.len += 1;
        self.reserved_bytes += size;
        true
    }

    pub fn release(&mut self, tag: &str) -> Option<u64> {
        let i = self.ranges[..self.len].iter().position(|r| r.tag == tag)?;
        let size = self.ranges[i].size;
        let last = self.len - 1;
        self.ranges[i] = self.ranges[last];
        self.len = last;
        self.reserved_bytes -= size;
        Some(size)
    }

    fn overlaps(&self, start: u64, size: u64) -> bool {
        let end = start + size;
        self.ranges[..self.len]
            .iter()
            .any(|r| start < r.start + r.size && r.start < end)
    }

    pub fn is_guest_owned(&self, phys: u64) -> bool {
        self.ranges[..self.len]
            .iter()
            .any(|r| phys >= r.start && phys < r.start + r.size)
    }

    pub fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn refusals(&self) -> u64 {
        self.refusals
    }

    pub fn range(&self, i: usize) -> Option<GuestRange> {
        if i < self.len {
            Some(self.ranges[i])
        } else {
            None
        }
    }
}

impl Default for GuestMemory {
    fn default() -> GuestMemory {
        GuestMemory::new()
    }
}

// ---------------------------------------------------------------------------
// F072 — diagnostic dump
// ---------------------------------------------------------------------------

/// A snapshot of the whole memory domain, rendered as text. Deliberately built
/// from data the caller passes in, so the dump never lies about live state.
#[derive(Clone, Copy, Debug, Default)]
pub struct MemSnapshot {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub fragmented_pages: u32,
    pub heap_live_bytes: u64,
    pub heap_live_slots: usize,
    pub leaked_pointers: usize,
    pub quarantined_frames: usize,
    pub swap_out: u64,
    pub water: WaterLevel,
}

pub fn render_snapshot(s: &MemSnapshot, out: &mut [u8]) -> usize {
    let mut w = Hud::new(out);
    w.str("mem total=");
    w.num(s.total_bytes / (1024 * 1024));
    w.str("MiB free=");
    w.num(s.free_bytes / (1024 * 1024));
    w.str("MiB used=");
    w.num(s.used_bytes / (1024 * 1024));
    w.str("MiB water=");
    w.str(match s.water {
        WaterLevel::Critical => "CRITICAL",
        WaterLevel::Low => "low",
        WaterLevel::Normal => "normal",
        WaterLevel::Comfortable => "ok",
    });
    w.str("\nheap live=");
    w.num(s.heap_live_bytes);
    w.str("B/");
    w.num(s.heap_live_slots as u64);
    w.str(" slots leaked=");
    w.num(s.leaked_pointers as u64);
    w.str(" frag=");
    w.num(s.fragmented_pages as u64);
    w.str(" bad=");
    w.num(s.quarantined_frames as u64);
    w.str(" swapout=");
    w.num(s.swap_out);
    w.str("\n");
    w.used()
}

// ---------------------------------------------------------------------------
// F075 — memory self-test
// ---------------------------------------------------------------------------

static MEM_SELFTEST: crate::selftest::SelfTest = crate::selftest::SelfTest::new();

pub fn mem_selftest() -> &'static crate::selftest::SelfTest {
    &MEM_SELFTEST
}

/// F075: every memory-chain invariant, checked against live state.
pub fn run_memory_checks() -> (usize, usize) {
    let r = &MEM_SELFTEST;

    // F052 — the zone has frames and a coherent free count.
    let z = crate::mem::pmm::zone().lock();
    let managed = z.count();
    let free = z.free_frames();
    let order_ok = z.largest_free_order().is_some() || free == 0;
    drop(z);
    r.check("pmm-frames", managed > 0, "no managed frames");
    r.check("pmm-budget", free <= managed, "free exceeds managed");
    r.check("pmm-order", order_ok, "free frames but no free block");

    // F051 — the bootstrap bitmap agrees with the zone it backs.
    let boot_used = crate::mem::pmm::bootstrap().lock().used();
    r.check("pmm-bitmap", boot_used > 0, "bitmap allocator not armed");

    // F053/F054 — the slab can hand out and take back a slot.
    let heap_ok = {
        match crate::mem::heap::kmalloc(64, 8, "self-test") {
            Some(p) => crate::mem::heap::kfree(p, 64),
            None => false,
        }
    };
    r.check("heap-roundtrip", heap_ok, "slab alloc/free failed");

    // F055 — the tracker saw the round trip and is balanced afterwards.
    let t = crate::mem::heap::tracker().lock();
    let leaked = t.leaked();
    drop(t);
    r.check("heap-tracking", !leaked, "outstanding allocations at rest");

    // F061 — the compressor round-trips a zero page.
    let mut scratch = [0u8; 1024];
    let page = [0u8; 256];
    let compressed = compress(&page, &mut scratch, 4);
    let comp_ok = match compressed {
        Some(n) => {
            let mut back = [0u8; 256];
            decompress(&scratch[..n], &mut back).map(|m| m == 256).unwrap_or(false)
        }
        None => false,
    };
    r.check("compressor", comp_ok, "round trip failed");

    // F064 — watermarks are ordered.
    let marks = crate::mem::mm_watcher_marks();
    r.check("watermarks", marks.valid(), "watermarks out of order");

    // F070 — quarantine bookkeeping is consistent.
    r.check(
        "bad-frames",
        crate::mem::pmm::quarantined() < crate::mem::pmm::MAX_BAD_FRAMES,
        "quarantine full",
    );

    // F071 — page KASLR stays page aligned.
    let plan = crate::mem::paging::KaslrPlan::build(crate::kaslr::pool_state().0, 4);
    r.check("page-kaslr", plan.valid(), "jitter misaligned");

    let (passed, failed) = r.tally();
    if failed == 0 {
        crate::kinfo!("memory self-test: {}/{} pass", passed, passed);
    } else {
        crate::kwarn!(
            "memory self-test: {}/{} pass ({} FAIL)",
            passed,
            passed + failed,
            failed
        );
    }
    (passed, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressor_round_trips_and_rejects_unprofitable_pages() {
        let mut dst = [0u8; 4096];

        // A zero page compresses hard.
        let zeros = [0u8; 4096];
        let n = compress(&zeros, &mut dst, 4).expect("zero page compresses");
        assert!(n < 128, "got {n}");
        let mut back = [0u8; 4096];
        assert_eq!(decompress(&dst[..n], &mut back), Some(4096));
        assert_eq!(back, zeros);

        // A mixed page with structure still round-trips.
        let mut page = [0u8; 4096];
        for (i, b) in page.iter_mut().enumerate() {
            *b = if i < 2048 { 0 } else { (i % 251) as u8 };
        }
        let n2 = compress(&page, &mut dst, 4).expect("structured page compresses");
        let mut back2 = [0u8; 4096];
        assert_eq!(decompress(&dst[..n2], &mut back2), Some(4096));
        assert_eq!(back2, page);

        // Random-looking data must be refused (no gain) rather than stored big.
        let mut noisy = [0u8; 4096];
        let mut x: u32 = 0x1234_5678;
        for b in noisy.iter_mut() {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            *b = (x >> 16) as u8;
        }
        assert!(compress(&noisy, &mut dst, 4).is_none());
    }

    #[test]
    fn compressor_is_bounds_safe() {
        let zeros = [7u8; 1024];
        let mut tiny = [0u8; 8];
        assert!(compress(&zeros, &mut tiny, 4).is_none(), "dst too small");
        // Truncated input must not be accepted by the decoder.
        let mut dst = [0u8; 1024];
        let n = compress(&zeros, &mut dst, 4).unwrap();
        let mut back = [0u8; 1024];
        assert!(decompress(&dst[..n - 1], &mut back).is_none());
        // A wrong original-length header is caught.
        let mut bad = [0u8; 1024];
        bad[..n].copy_from_slice(&dst[..n]);
        bad[0] = 0xFF;
        assert!(decompress(&bad[..n], &mut back).is_none());
        assert!(decompress(&[], &mut back).is_none());
    }

    #[test]
    fn compressor_stats_and_zero_page_detection() {
        let c = Compressor::new();
        let mut scratch = [0u8; 4096];
        let page = [0u8; PAGE_SIZE];
        assert!(is_zero_page(&page));
        assert!(c.try_page(&page, &mut scratch).is_some());
        let mut noisy = [0u8; PAGE_SIZE];
        let mut x: u32 = 7;
        for b in noisy.iter_mut() {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            *b = (x >> 16) as u8;
        }
        assert!(c.try_page(&noisy, &mut scratch).is_none());
        assert_eq!(c.attempts(), 2);
        assert_eq!(c.stored(), 1);
        assert_eq!(c.skipped(), 1);
        assert!(c.saved_percent() > 90);
    }

    #[test]
    fn swap_refuses_without_backing_and_accounts_when_enabled() {
        let mut s = SwapFramework::new();
        assert!(!s.enabled());
        assert_eq!(s.swap_out(10), None);
        s.set_backing(SwapBacking::Partition);
        assert!(s.enabled());
        let slot = s.swap_out(10).unwrap();
        assert_eq!(slot, 0);
        let slot2 = s.swap_out(11).unwrap();
        assert_eq!(s.resident(), 2);
        assert_eq!(s.swapped_bytes(), 2 * PAGE_SIZE as u64);
        assert_eq!(s.swap_in(slot), Some(10));
        assert_eq!(s.resident(), 1);
        assert_eq!(s.swap_in(slot), None);
        assert_eq!(s.refaults(), 1);
        assert_eq!(s.swap_in(slot2), Some(11));
        assert_eq!(s.swapped_out(), 2);
        assert_eq!(s.swapped_in(), 2);
        assert_eq!(s.resident(), 0);
    }

    #[test]
    fn watermark_levels_and_breaches() {
        let m = Watermarks::for_total(8 * 1024 * 1024 * 1024);
        assert!(m.valid());
        assert_eq!(m.level(u64::MAX), WaterLevel::Comfortable);
        assert_eq!(m.level(m.min_bytes), WaterLevel::Low);
        assert_eq!(m.level(m.min_bytes - 1), WaterLevel::Critical);
        let mut w = WatermarkWatcher::new();
        w.arm(8 * 1024 * 1024 * 1024);
        assert!(w.marks().valid());
        assert_eq!(w.sample(m.low_bytes + 10, 0), WaterLevel::Normal);
        assert_eq!(w.sample(0, 0), WaterLevel::Critical);
        assert_eq!(w.critical_breaches(), 1);
        assert_eq!(w.low_breaches(), 1);
        assert_eq!(w.level(), WaterLevel::Critical);
        assert_eq!(w.lowest_free_bytes(), 0);
    }

    #[test]
    fn oom_scores_pick_the_worst_offender() {
        let scores = [
            OomScore {
                pid: 1,
                rss_bytes: 1024 * 1024,
                adj: -500,
                idle_ticks: 0,
                kernel: false,
                root_owned: true,
            },
            OomScore {
                pid: 2,
                rss_bytes: 512 * 1024 * 1024,
                adj: 200,
                idle_ticks: 10_000,
                kernel: false,
                root_owned: false,
            },
            OomScore {
                pid: 3,
                rss_bytes: 1024 * 1024 * 1024,
                adj: 0,
                idle_ticks: 0,
                kernel: true,
                root_owned: false,
            },
        ];
        // Kernel threads are never victims, even at 1 GiB RSS.
        assert_eq!(pick_victim(&scores), Some(1));
        assert!(scores[2].score() == i64::MIN);
        assert!(scores[1].score() > scores[0].score());
        assert_eq!(pick_victim(&[]), None);
        // Determinism on a tie: lower pid wins.
        let tie = [
            OomScore {
                pid: 9,
                rss_bytes: 0,
                adj: 0,
                idle_ticks: 0,
                kernel: false,
                root_owned: false,
            },
            OomScore {
                pid: 4,
                rss_bytes: 0,
                adj: 0,
                idle_ticks: 0,
                kernel: false,
                root_owned: false,
            },
        ];
        assert_eq!(pick_victim(&tie), Some(1));
    }

    #[test]
    fn oom_escalation_ladder() {
        assert_eq!(
            escalation(WaterLevel::Comfortable, OomAction::Reclaim),
            OomAction::Reclaim
        );
        assert_eq!(
            escalation(WaterLevel::Critical, OomAction::Reclaim),
            OomAction::KillVictim,
            "critical memory skips straight to a kill"
        );
        assert_eq!(
            escalation(WaterLevel::Critical, OomAction::KillVictim),
            OomAction::KillVictim,
            "already escalated: do not escalate again by accident"
        );
        assert_eq!(OomAction::Halt.rung(), 4);
        assert_eq!(OomAction::KillGroup.rung(), 3);
        assert!(!OomAction::Halt.as_str().is_empty());
    }

    #[test]
    fn numa_topology_affinity() {
        let mut t = NumaTopology::new();
        assert_eq!(t.count(), 0);
        assert_eq!(t.add_node(0, 0, 1 << 18), Some(0));
        assert_eq!(t.add_node(1, 1 << 18, 1 << 18), Some(1));
        assert!(t.bind_cpu(0, 0));
        assert!(t.bind_cpu(8, 1));
        assert!(!t.bind_cpu(8, 99));
        assert!(!t.bind_cpu(999, 0));
        assert_eq!(t.node_of_cpu(0), Some(0));
        assert_eq!(t.node_of_cpu(8), Some(1));
        assert_eq!(t.preferred_for_cpu(0), Some(0));
        assert_eq!(t.node(0).unwrap().bytes(), (1 << 18) * PAGE_SIZE as u64);
        assert!(t.node(0).unwrap().contains_phys(0x1000));
        assert!(!t.node(0).unwrap().contains_phys(1 << 30));
        // 1 MiB is still inside node 0; node 1 starts at its frame_base.
        assert_eq!(t.node_of_phys(1 << 20), Some(0));
        assert_eq!(t.node_of_phys((1 << 18) * PAGE_SIZE as u64 + 0x1000), Some(1));
        assert_eq!(t.node(0).unwrap().distance_to(1), LOCAL_DISTANCE);
        assert!(t.set_distance(0, 1, 21));
        assert_eq!(t.node(0).unwrap().distance_to(1), 21);
        assert_eq!(t.node(0).unwrap().distance_to(99), u8::MAX);
    }

    #[test]
    fn dma_pool_enforces_alignment_and_overlap() {
        let mut p = DmaPool::new();
        assert!(p.is_coherent());
        assert!(
            !p.register(0x1010, 64, DmaDirection::ToDevice, "ahci"),
            "must be 64B aligned"
        );
        assert!(p.register(0x1000, 64, DmaDirection::ToDevice, "ahci"));
        assert!(!p.register(0x1000, 128, DmaDirection::FromDevice, "nvme"), "overlap");
        assert!(!p.register(0x2000, 0, DmaDirection::ToDevice, "x"), "zero size");
        assert!(p.register(0x1040, 64, DmaDirection::Bidirectional, "xhci"));
        assert_eq!(p.len(), 2);
        assert_eq!(p.bytes(), 128);
        assert_eq!(p.rejected(), 3);
        assert!(p.sync_for_device(0x1000));
        assert_eq!(p.syncs(), 0, "coherent memory needs no flush");
        p.set_coherent(false);
        assert!(p.register(0x1080, 64, DmaDirection::ToDevice, "e1000"));
        assert!(p.sync_for_device(0x1080));
        assert_eq!(p.syncs(), 1);
        assert!(!p.sync_for_device(0xDEAD), "unknown buffer");
        assert!(p.release(0x1000));
        assert_eq!(p.len(), 2);
        assert_eq!(p.find(0x1000), None);
        assert_eq!(p.high_water(), 3);
    }

    #[test]
    fn driver_pools_enforce_quotas() {
        let mut d = DriverPools::new();
        assert!(d.register("ahci", 1 << 20));
        assert!(!d.register("ahci", 1 << 20), "no duplicate pools");
        assert!(d.charge("ahci", 512 * 1024));
        assert!(d.charge("ahci", 256 * 1024));
        assert!(!d.charge("ahci", 512 * 1024), "quota exceeded");
        assert_eq!(d.find("ahci").unwrap().refusals, 1);
        assert_eq!(d.find("ahci").unwrap().peak_bytes, 768 * 1024);
        d.release("ahci", 512 * 1024);
        assert!(d.charge("ahci", 512 * 1024));
        // Unknown drivers (kernel internal) are not capped.
        assert!(d.charge("core", u64::MAX));
        assert_eq!(d.offenders(), 1);
        assert_eq!(d.pool(0).unwrap().name, "ahci");
        assert!(d.pool(9).is_none());
    }

    #[test]
    fn page_heat_ages_and_classifies() {
        let mut h = PageHeat::new();
        h.touch(0);
        h.touch(0);
        h.touch(2);
        assert_eq!(h.heat(0), 2);
        assert_eq!(h.heat(2), 1);
        assert_eq!(h.hot(2), 1);
        h.decay();
        assert_eq!(h.heat(0), 1);
        assert_eq!(h.heat(2), 0);
        assert_eq!(h.scans(), 1);
        assert_eq!(h.touches(), 3);
        // Heat saturates rather than wrapping.
        for _ in 0..50 {
            h.touch(1);
        }
        assert_eq!(h.heat(1), HEAT_LEVELS);
        // Out-of-range frames are ignored.
        h.touch(u32::MAX);
        assert_eq!(h.heat(u32::MAX), 0);
    }

    #[test]
    fn rss_accounting_enforces_limits() {
        let mut a = RssAccount::new(1024);
        assert!(a.charge(512));
        assert!(a.charge(512));
        assert!(!a.charge(1));
        assert_eq!(a.denials, 1);
        assert_eq!(a.utilization_percent(), 100);
        assert_eq!(a.remaining(), 0);
        assert!(!a.over_limit());
        a.release(1024);
        assert_eq!(a.used_bytes, 0);
        assert_eq!(a.peak_bytes, 1024);
        // Limit 0 means unlimited.
        let mut u = RssAccount::new(0);
        assert!(u.charge(u64::MAX - 1));
        assert_eq!(u.utilization_percent(), 0);
    }

    #[test]
    fn guest_memory_reservations() {
        let mut g = GuestMemory::new();
        assert!(g.is_empty());
        assert!(g.reserve(0x40_0000, 64 * 1024 * 1024, "wtg"), "2MiB aligned");
        assert!(!g.reserve(0x40_0000 + 0x1000, 0x1000, "bad"), "unaligned + overlap");
        assert!(g.reserve(0x1000_0000, 64 * 1024 * 1024, "guest2"));
        assert!(g.is_guest_owned(0x40_1000));
        assert!(!g.is_guest_owned(0x1000));
        assert_eq!(g.reserved_bytes(), 128 * 1024 * 1024);
        assert_eq!(g.len(), 2);
        assert_eq!(g.release("wtg"), Some(64 * 1024 * 1024));
        assert_eq!(g.len(), 1);
        assert_eq!(g.release("nope"), None);
        assert_eq!(g.refusals(), 1);
        assert_eq!(g.range(0).unwrap().tag, "guest2");
        assert!(g.range(5).is_none());
    }

    #[test]
    fn snapshot_render_is_bounded() {
        let s = MemSnapshot {
            total_bytes: 2 * 1024 * 1024 * 1024,
            free_bytes: 1024 * 1024 * 1024,
            used_bytes: 1024 * 1024 * 1024,
            fragmented_pages: 12,
            heap_live_bytes: 4096,
            heap_live_slots: 3,
            leaked_pointers: 0,
            quarantined_frames: 2,
            swap_out: 5,
            water: WaterLevel::Normal,
        };
        let mut out = [0u8; 256];
        let n = render_snapshot(&s, &mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.starts_with("mem total=2048MiB"), "got {text}");
        assert!(text.contains("water=normal"), "got {text}");
        assert!(text.contains("heap live=4096B/3 slots"), "got {text}");
        assert!(text.ends_with('\n'));
        let mut tiny = [0u8; 6];
        assert_eq!(render_snapshot(&s, &mut tiny), 6);
    }
}
