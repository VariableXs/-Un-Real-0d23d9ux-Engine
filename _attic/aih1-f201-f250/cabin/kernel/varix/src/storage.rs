//! AI-06 · 存储与文件系统域（F126~F150）.
//!
//! From the AHCI register FIS up to the mount table. Everything on-disk is
//! parsed from plain byte slices, so a filesystem that lies about its geometry
//! is caught by a unit test rather than by corrupting a user's data. The
//! writeable surface is deliberately narrow: the three read-only filesystems
//! (ext2, exFAT, NTFS) can never be opened for writing, and the journal is the
//! only thing Varix trusts to be mid-update after a crash.

use core::cmp::Reverse;

pub const MAX_DEVICES: usize = 16;
pub const MAX_MOUNTS: usize = 8;
pub const MAX_FDS: usize = 64;
pub const SECTOR: usize = 512;

// ---------------------------------------------------------------------------
// F128 — block layer
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DeviceKind {
    #[default]
    None,
    /// SATA through an AHCI controller (F126).
    Ahci,
    /// NVMe namespace (F127).
    Nvme,
    VirtioBlk,
    /// USB mass storage.
    Usb,
    /// A partition view over another device.
    Partition,
    /// An image file or a differential disk (F146).
    Image,
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceKind::None => "none",
            DeviceKind::Ahci => "ahci",
            DeviceKind::Nvme => "nvme",
            DeviceKind::VirtioBlk => "virtio-blk",
            DeviceKind::Usb => "usb",
            DeviceKind::Partition => "part",
            DeviceKind::Image => "image",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockOp {
    Read,
    Write,
    Flush,
    Trim,
}

impl BlockOp {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockOp::Read => "read",
            BlockOp::Write => "write",
            BlockOp::Flush => "flush",
            BlockOp::Trim => "trim",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlockRequest {
    pub device: u8,
    pub op: BlockOp,
    pub lba: u64,
    pub blocks: u16,
    /// Submission order, used for fairness and for latency accounting.
    pub seq: u64,
}

impl BlockRequest {
    pub fn spans(&self, lba: u64, blocks: u64) -> bool {
        self.lba < lba + blocks && lba < self.lba + self.blocks as u64
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BlockDevice {
    pub slot: u8,
    pub kind: DeviceKind,
    pub block_size: u32,
    pub blocks: u64,
    pub read_only: bool,
    pub removable: bool,
    /// Queue depth the device can keep busy — what the IO scheduler has to
    /// respect when it decides how many requests to have in flight.
    pub queue_depth: u16,
    pub name: &'static str,
}

impl BlockDevice {
    pub fn capacity_bytes(&self) -> u64 {
        self.blocks * self.block_size as u64
    }

    pub fn contains(&self, lba: u64, blocks: u64) -> bool {
        lba + blocks <= self.blocks
    }

    /// Sector count in 512-byte units, which is how the AHCI/NVMe commands and
    /// every boot record express geometry.
    pub fn sectors_512(&self) -> u64 {
        self.capacity_bytes() / SECTOR as u64
    }
}

pub struct BlockLayer {
    devices: [Option<BlockDevice>; MAX_DEVICES],
    count: usize,
    reads: u64,
    writes: u64,
    rejected: u64,
}

impl BlockLayer {
    pub const fn new() -> BlockLayer {
        BlockLayer {
            devices: [None; MAX_DEVICES],
            count: 0,
            reads: 0,
            writes: 0,
            rejected: 0,
        }
    }

    pub fn register(&mut self, mut dev: BlockDevice) -> Option<u8> {
        let slot = self.devices.iter().position(|d| d.is_none())?;
        dev.slot = slot as u8;
        self.devices[slot] = Some(dev);
        self.count += 1;
        Some(slot as u8)
    }

    pub fn get(&self, slot: u8) -> Option<&BlockDevice> {
        self.devices.get(slot as usize).and_then(|d| d.as_ref())
    }

    pub fn devices(&self) -> impl Iterator<Item = &BlockDevice> {
        self.devices.iter().filter_map(|d| d.as_ref())
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn reads(&self) -> u64 {
        self.reads
    }

    pub fn writes(&self) -> u64 {
        self.writes
    }

    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// F128: the single gate every request passes through. Range and
    /// write-protection are checked here so no backend has to re-do it.
    pub fn validate(&mut self, req: &BlockRequest) -> Result<(), &'static str> {
        let dev = match self.get(req.device) {
            Some(d) => d,
            None => {
                self.rejected += 1;
                return Err("no such block device");
            }
        };
        if req.blocks == 0 {
            self.rejected += 1;
            return Err("zero-length request");
        }
        if !dev.contains(req.lba, req.blocks as u64) {
            self.rejected += 1;
            return Err("request runs past the end of the device");
        }
        if dev.read_only && matches!(req.op, BlockOp::Write | BlockOp::Trim) {
            self.rejected += 1;
            return Err("device is read-only");
        }
        match req.op {
            BlockOp::Read => self.reads += 1,
            BlockOp::Write => self.writes += 1,
            _ => {}
        }
        Ok(())
    }
}

impl Default for BlockLayer {
    fn default() -> BlockLayer {
        BlockLayer::new()
    }
}

// ---------------------------------------------------------------------------
// F126 — AHCI
// ---------------------------------------------------------------------------

pub const AHCI_SIG_ATA: u32 = 0x0000_0101;
pub const AHCI_SIG_ATAPI: u32 = 0xEB14_0101;
pub const AHCI_SIG_SEMB: u32 = 0xC33C_0101;
pub const AHCI_SIG_PM: u32 = 0x9669_0101;

pub const FIS_TYPE_REG_H2D: u8 = 0x27;
pub const ATA_CMD_READ_DMA_EX: u8 = 0x25;
pub const ATA_CMD_WRITE_DMA_EX: u8 = 0x35;
pub const ATA_CMD_IDENTIFY: u8 = 0xEC;
pub const ATA_CMD_FLUSH: u8 = 0xE7;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AhciPort {
    pub index: u8,
    pub signature: u32,
    pub present: bool,
    /// Raw sector count from IDENTIFY (48-bit where supported).
    pub lba_capacity: u64,
}

impl AhciPort {
    pub fn is_ata(&self) -> bool {
        self.present && self.signature == AHCI_SIG_ATA
    }

    pub fn is_atapi(&self) -> bool {
        self.present && self.signature == AHCI_SIG_ATAPI
    }

    /// Ports that advertise a signature Varix has no driver for.
    pub fn is_unknown(&self) -> bool {
        self.present && !self.is_ata() && !self.is_atapi()
    }
}

/// A Host-to-Device register FIS — the 20-byte structure the controller reads
/// out of memory to start a command. Encoded byte by byte because a field
/// offset error here silently writes to the wrong sector.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RegisterH2dFis {
    pub command: u8,
    pub feature_low: u8,
    pub lba: [u8; 6],
    pub device: u8,
    pub count: u16,
    /// 1 = DMA, 0 = PIO.
    pub dma: bool,
}

impl RegisterH2dFis {
    pub const LEN: usize = 20;

    pub fn for_dma(cmd: u8, lba: u64, count: u16) -> RegisterH2dFis {
        let b = lba.to_le_bytes();
        RegisterH2dFis {
            command: cmd,
            feature_low: 0,
            lba: [b[0], b[1], b[2], b[3], b[4], b[5]],
            // LBA mode + LBA bits 24..27 in the old device register.
            device: 0x40 | (((lba >> 24) & 0x0F) as u8),
            count,
            dma: true,
        }
    }

    /// Serialize into the 20-byte wire image.
    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut f = [0u8; Self::LEN];
        f[0] = FIS_TYPE_REG_H2D;
        // bit 7 = command, not control.
        f[1] = 0x80;
        f[2] = self.command;
        f[3] = self.feature_low;
        f[4..10].copy_from_slice(&self.lba);
        f[10] = self.device;
        f[11] = self.lba[3];
        f[12] = self.lba[4];
        f[13] = self.lba[5];
        f[14] = self.count as u8;
        f[15] = (self.count >> 8) as u8;
        f
    }

    /// Decode back, so the encoder is provably lossless.
    pub fn decode(f: &[u8; Self::LEN]) -> Option<RegisterH2dFis> {
        if f[0] != FIS_TYPE_REG_H2D || f[1] & 0x80 == 0 {
            return None;
        }
        Some(RegisterH2dFis {
            command: f[2],
            feature_low: f[3],
            lba: [f[4], f[5], f[6], f[7], f[8], f[9]],
            device: f[10],
            count: u16::from_le_bytes([f[14], f[15]]),
            dma: true,
        })
    }

    /// The 48-bit LBA this FIS addresses.
    pub fn lba48(&self) -> u64 {
        u64::from_le_bytes([
            self.lba[0], self.lba[1], self.lba[2], self.lba[3], self.lba[4], self.lba[5], 0, 0,
        ])
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AhciController {
    pub ports: [AhciPort; 32],
    pub port_count: usize,
    pub version: u32,
    /// Capabilities register: 64-bit addressing + native command queueing.
    pub cap: u32,
}

impl AhciController {
    pub fn supports_64bit(&self) -> bool {
        self.cap & (1 << 31) != 0
    }

    pub fn supports_ncq(&self) -> bool {
        self.cap & (1 << 30) != 0
    }

    pub fn attached(&self) -> usize {
        self.ports[..self.port_count].iter().filter(|p| p.present).count()
    }

    /// F126: the identify plan for one port — which command and how much data.
    pub fn identify_request(&self, port: u8) -> Option<RegisterH2dFis> {
        let p = self.ports.get(port as usize)?;
        if !p.present {
            return None;
        }
        Some(RegisterH2dFis {
            command: ATA_CMD_IDENTIFY,
            feature_low: 0,
            lba: [0; 6],
            device: 0,
            count: 1,
            dma: false,
        })
    }

    /// Word 60/61 of the IDENTIFY response: 28-bit LBA capacity; words 100..103
    /// carry the 48-bit value.
    pub fn capacity_from_identify(identify: &[u8; 512]) -> u64 {
        let w = |i: usize| u16::from_le_bytes([identify[i * 2], identify[i * 2 + 1]]) as u64;
        let lba48 = w(100) | (w(101) << 16) | (w(102) << 32) | (w(103) << 48);
        if lba48 != 0 {
            lba48
        } else {
            w(60) | (w(61) << 16)
        }
    }
}

// ---------------------------------------------------------------------------
// F127 — NVMe
// ---------------------------------------------------------------------------

pub const NVME_OP_WRITE: u8 = 0x01;
pub const NVME_OP_READ: u8 = 0x02;
pub const NVME_OP_FLUSH: u8 = 0x00;
pub const NVME_OP_WRITE_ZEROES: u8 = 0x08;
pub const NVME_OP_DSM: u8 = 0x09;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NvmeQueue {
    pub id: u16,
    pub depth: u16,
    pub head: u16,
    pub tail: u16,
    /// Phase bit: flips each time the ring wraps. Getting it wrong makes the
    /// controller stop completing commands.
    pub phase: bool,
    pub in_flight: u16,
}

impl NvmeQueue {
    pub fn with_depth(id: u16, depth: u16) -> NvmeQueue {
        NvmeQueue {
            id,
            depth,
            head: 0,
            tail: 0,
            phase: true,
            in_flight: 0,
        }
    }

    pub fn full(&self) -> bool {
        self.in_flight >= self.depth
    }

    /// Reserve a slot; `None` when the queue is full.
    pub fn submit(&mut self) -> Option<u16> {
        if self.full() {
            return None;
        }
        let slot = self.tail;
        self.tail = (self.tail + 1) % self.depth;
        self.in_flight += 1;
        Some(slot)
    }

    /// Complete the head of the queue, flipping the phase on wrap.
    pub fn complete(&mut self) -> Option<u16> {
        if self.in_flight == 0 {
            return None;
        }
        let slot = self.head;
        self.head = (self.head + 1) % self.depth;
        if self.head == 0 {
            self.phase = !self.phase;
        }
        self.in_flight -= 1;
        Some(slot)
    }

    pub fn free_slots(&self) -> u16 {
        self.depth.saturating_sub(self.in_flight)
    }
}

/// A 64-byte NVMe submission command, encoded field by field.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NvmeCommand {
    pub opcode: u8,
    pub nsid: u32,
    pub lba: u64,
    /// Number of logical blocks minus one.
    pub nlb: u16,
    pub cid: u16,
}

impl NvmeCommand {
    /// Read/write commands carry the 64-bit LBA across dwords 10 and 11.
    pub fn encode(&self) -> [u8; 64] {
        let mut c = [0u8; 64];
        c[0] = self.opcode;
        c[4..8].copy_from_slice(&self.nsid.to_le_bytes());
        c[40..48].copy_from_slice(&self.lba.to_le_bytes());
        c[48..50].copy_from_slice(&self.nlb.to_le_bytes());
        c[2..4].copy_from_slice(&self.cid.to_le_bytes());
        c
    }

    pub fn decode(c: &[u8; 64]) -> NvmeCommand {
        NvmeCommand {
            opcode: c[0],
            nsid: u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            lba: u64::from_le_bytes([
                c[40], c[41], c[42], c[43], c[44], c[45], c[46], c[47],
            ]),
            nlb: u16::from_le_bytes([c[48], c[49]]),
            cid: u16::from_le_bytes([c[2], c[3]]),
        }
    }

    pub fn blocks(&self) -> u32 {
        self.nlb as u32 + 1
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NvmeCompletion {
    pub result: u32,
    pub sq_head: u16,
    pub sq_id: u16,
    pub cid: u16,
    pub status: u16,
    pub phase: bool,
}

impl NvmeCompletion {
    /// The completion entry packs command id (15:0), phase (16) and status
    /// (31:17) into dword 3 — three fields in one word, and getting the shift
    /// wrong makes every command look like a device error.
    pub fn from_bytes(b: &[u8; 16]) -> NvmeCompletion {
        let dw3 = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
        NvmeCompletion {
            result: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            sq_head: u16::from_le_bytes([b[6], b[7]]),
            sq_id: u16::from_le_bytes([b[8], b[9]]),
            cid: (dw3 & 0xFFFF) as u16,
            status: (dw3 >> 17) as u16,
            phase: (dw3 >> 16) & 1 != 0,
        }
    }

    pub fn ok(&self) -> bool {
        self.status == 0
    }

    pub fn status_name(&self) -> &'static str {
        match self.status {
            0 => "ok",
            1 => "invalid opcode",
            2 => "invalid field",
            4 => "data transfer error",
            6 => "internal error",
            0x87 => "namespace not ready",
            _ => "device error",
        }
    }
}

// ---------------------------------------------------------------------------
// F129 — IO scheduler
// ---------------------------------------------------------------------------

pub const MAX_IO_QUEUE: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IoRequest {
    pub id: u64,
    pub device: u8,
    pub op: BlockOp,
    pub lba: u64,
    pub blocks: u16,
    /// Higher runs first (0 = background prefetch, 2 = foreground).
    pub priority: u8,
    pub submit_tick: u64,
    /// 0 = no deadline.
    pub deadline_tick: u64,
}

/// Mergeable? Adjacent requests of the same operation and device coalesce into
/// one larger transfer, which is the cheapest performance win a block layer
/// has.
pub fn mergeable(a: &IoRequest, b: &IoRequest) -> bool {
    a.device == b.device
        && a.op == b.op
        && matches!(a.op, BlockOp::Read | BlockOp::Write)
        && (a.lba + a.blocks as u64 == b.lba || b.lba + b.blocks as u64 == a.lba)
        && (a.blocks as u32 + b.blocks as u32) <= u16::MAX as u32
}

pub struct IoQueue {
    items: [Option<IoRequest>; MAX_IO_QUEUE],
    len: usize,
    merged: u64,
    dispatched: u64,
    expired: u64,
}

impl IoQueue {
    pub const fn new() -> IoQueue {
        IoQueue {
            items: [None; MAX_IO_QUEUE],
            len: 0,
            merged: 0,
            dispatched: 0,
            expired: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn merged(&self) -> u64 {
        self.merged
    }

    pub fn dispatched(&self) -> u64 {
        self.dispatched
    }

    /// Requests whose deadline passed before dispatch — the number that matters
    /// when a real-time path is being starved by bulk IO.
    pub fn expired(&self) -> u64 {
        self.expired
    }

    /// F129: enqueue with coalescing. Returns the slot used, or `None` when the
    /// queue is full (which is a refusal, never an overwrite).
    pub fn submit(&mut self, req: IoRequest) -> Option<usize> {
        for i in 0..MAX_IO_QUEUE {
            if let Some(existing) = self.items[i] {
                if mergeable(&existing, &req) {
                    let mut merged = existing;
                    merged.lba = merged.lba.min(req.lba);
                    merged.blocks += req.blocks;
                    merged.deadline_tick = if merged.deadline_tick == 0 {
                        req.deadline_tick
                    } else if req.deadline_tick == 0 {
                        merged.deadline_tick
                    } else {
                        merged.deadline_tick.min(req.deadline_tick)
                    };
                    self.items[i] = Some(merged);
                    self.merged += 1;
                    return Some(i);
                }
            }
        }
        let slot = self.items.iter().position(|x| x.is_none())?;
        self.items[slot] = Some(req);
        self.len += 1;
        Some(slot)
    }

    /// Pick the next request: deadline first, then priority, then age.
    pub fn peek(&self, now: u64) -> Option<IoRequest> {
        let mut best: Option<(usize, IoRequest)> = None;
        for (i, item) in self.items.iter().enumerate() {
            let r = match item {
                Some(r) => *r,
                None => continue,
            };
            let better = match best {
                None => true,
                Some((_, b)) => {
                    // 0 means "no deadline": it must sort *after* any real one.
                    let key = |x: &IoRequest| {
                        (
                            if x.deadline_tick == 0 {
                                u64::MAX
                            } else {
                                x.deadline_tick
                            },
                            Reverse(x.priority),
                            x.submit_tick,
                        )
                    };
                    key(&r) < key(&b)
                }
            };
            if better {
                best = Some((i, r));
            }
        }
        let _ = now;
        best.map(|(_, r)| r)
    }

    pub fn take_next(&mut self, now: u64) -> Option<IoRequest> {
        let next = self.peek(now)?;
        for i in 0..MAX_IO_QUEUE {
            if self.items[i].map(|r| r.id == next.id).unwrap_or(false) {
                self.items[i] = None;
                self.len -= 1;
                self.dispatched += 1;
                if next.deadline_tick != 0 && now > next.deadline_tick {
                    self.expired += 1;
                }
                return Some(next);
            }
        }
        None
    }

    /// How many requests are ready to be handed to hardware — the number the
    /// scheduler reports to the device queue depth.
    pub fn ready(&self) -> usize {
        self.len
    }
}

impl Default for IoQueue {
    fn default() -> IoQueue {
        IoQueue::new()
    }
}

// ---------------------------------------------------------------------------
// F130 — block cache
// ---------------------------------------------------------------------------

pub const MAX_CACHE_ENTRIES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CacheEntry {
    pub device: u8,
    pub lba: u64,
    pub blocks: u16,
    pub valid: bool,
    pub dirty: bool,
    /// Monotonic use counter — the LRU order.
    pub last_used: u64,
}

pub struct BlockCache {
    entries: [CacheEntry; MAX_CACHE_ENTRIES],
    clock: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
    dirty_evictions: u64,
}

impl BlockCache {
    pub const fn new() -> BlockCache {
        BlockCache {
            entries: [CacheEntry {
                device: 0,
                lba: 0,
                blocks: 0,
                valid: false,
                dirty: false,
                last_used: 0,
            }; MAX_CACHE_ENTRIES],
            clock: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
            dirty_evictions: 0,
        }
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    /// Dirty entries pushed out by pressure — each one was a write that had to
    /// reach the device before the new data could land.
    pub fn writebacks(&self) -> u64 {
        self.dirty_evictions
    }

    pub fn hit_percent(&self) -> u64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0
        } else {
            self.hits * 100 / total
        }
    }

    pub fn dirty_count(&self) -> usize {
        self.entries.iter().filter(|e| e.valid && e.dirty).count()
    }

    pub fn lookup(&mut self, device: u8, lba: u64) -> Option<usize> {
        self.clock += 1;
        for i in 0..MAX_CACHE_ENTRIES {
            let e = &mut self.entries[i];
            if e.valid && e.device == device && lba >= e.lba && lba < e.lba + e.blocks as u64 {
                e.last_used = self.clock;
                self.hits += 1;
                return Some(i);
            }
        }
        self.misses += 1;
        None
    }

    pub fn insert(&mut self, device: u8, lba: u64, blocks: u16, dirty: bool) -> usize {
        self.clock += 1;
        // Prefer a free slot, else evict the least recently used.
        let mut victim = 0usize;
        let mut oldest = u64::MAX;
        for i in 0..MAX_CACHE_ENTRIES {
            if !self.entries[i].valid {
                self.entries[i] = CacheEntry {
                    device,
                    lba,
                    blocks,
                    valid: true,
                    dirty,
                    last_used: self.clock,
                };
                return i;
            }
            if self.entries[i].last_used < oldest {
                oldest = self.entries[i].last_used;
                victim = i;
            }
        }
        if self.entries[victim].dirty {
            self.dirty_evictions += 1;
        }
        self.evictions += 1;
        self.entries[victim] = CacheEntry {
            device,
            lba,
            blocks,
            valid: true,
            dirty,
            last_used: self.clock,
        };
        victim
    }

    /// Mark an entry clean after it reached the device.
    pub fn mark_clean(&mut self, index: usize) -> bool {
        match self.entries.get_mut(index) {
            Some(e) if e.valid => {
                e.dirty = false;
                true
            }
            _ => false,
        }
    }

    pub fn invalidate_device(&mut self, device: u8) -> usize {
        let mut n = 0;
        for e in self.entries.iter_mut() {
            if e.valid && e.device == device {
                *e = CacheEntry::default();
                n += 1;
            }
        }
        n
    }
}

impl Default for BlockCache {
    fn default() -> BlockCache {
        BlockCache::new()
    }
}

// ---------------------------------------------------------------------------
// F131/F132 — VFS and mount table
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FsKind {
    #[default]
    None,
    Fat32,
    ExFat,
    Ext2,
    Ntfs,
    /// Varix's own journalled filesystem (F137).
    Varix,
    /// A ramdisk / initramfs.
    Ram,
}

impl FsKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FsKind::None => "none",
            FsKind::Fat32 => "fat32",
            FsKind::ExFat => "exfat",
            FsKind::Ext2 => "ext2",
            FsKind::Ntfs => "ntfs",
            FsKind::Varix => "varix",
            FsKind::Ram => "ram",
        }
    }

    /// F135/F136: the read-only filesystems, in one place so no call site has
    /// to remember which of them refuses writes.
    pub fn read_only_by_design(self) -> bool {
        matches!(self, FsKind::Ext2 | FsKind::Ntfs)
    }

    pub fn writable(self) -> bool {
        !matches!(self, FsKind::None | FsKind::Ext2 | FsKind::Ntfs)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NodeKind {
    #[default]
    File,
    Dir,
    Device,
    Symlink,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VfsNode {
    pub fs: u8,
    pub inode: u64,
    pub kind: NodeKind,
    pub size: u64,
    pub mode: crate::proc::Mode,
}

pub const MOUNTPOINT_BYTES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mount {
    pub mountpoint: [u8; MOUNTPOINT_BYTES],
    pub mountpoint_len: usize,
    pub fs: FsKind,
    pub device: u8,
    pub read_only: bool,
    /// The filesystem persists across reboots (as opposed to a ramdisk).
    pub persistent: bool,
}

impl Mount {
    pub fn path(&self) -> &str {
        core::str::from_utf8(&self.mountpoint[..self.mountpoint_len]).unwrap_or("?")
    }

    pub fn writable(&self) -> bool {
        !self.read_only && self.fs.writable()
    }
}

impl Default for Mount {
    fn default() -> Mount {
        Mount {
            mountpoint: [0; MOUNTPOINT_BYTES],
            mountpoint_len: 0,
            fs: FsKind::None,
            device: 0,
            read_only: false,
            persistent: false,
        }
    }
}

pub struct MountTable {
    mounts: [Mount; MAX_MOUNTS],
    len: usize,
    rejected: u64,
}

impl MountTable {
    pub const fn new() -> MountTable {
        MountTable {
            mounts: [Mount {
                mountpoint: [0; MOUNTPOINT_BYTES],
                mountpoint_len: 0,
                fs: FsKind::None,
                device: 0,
                read_only: false,
                persistent: false,
            }; MAX_MOUNTS],
            len: 0,
            rejected: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    pub fn get(&self, i: usize) -> Option<Mount> {
        if i < self.len {
            Some(self.mounts[i])
        } else {
            None
        }
    }

    /// F132: mount at `path`. Nested mountpoints are allowed (that is how
    /// `/mnt/u` ends up on a different device), but a duplicate is not.
    pub fn mount(
        &mut self,
        path: &str,
        fs: FsKind,
        device: u8,
        read_only: bool,
        persistent: bool,
    ) -> Result<usize, &'static str> {
        if self.len >= MAX_MOUNTS {
            self.rejected += 1;
            return Err("mount table is full");
        }
        if !path.starts_with('/') || path.len() > MOUNTPOINT_BYTES || path.len() < 1 {
            self.rejected += 1;
            return Err("mountpoint must be an absolute path");
        }
        if path.len() > 1 && path.ends_with('/') {
            self.rejected += 1;
            return Err("mountpoint must not end with a slash");
        }
        if self.find(path).is_some() {
            self.rejected += 1;
            return Err("already mounted");
        }
        let mut m = Mount {
            fs,
            device,
            // A filesystem that is read-only by design cannot be mounted
            // writable, however the caller asks.
            read_only: read_only || fs.read_only_by_design(),
            persistent,
            ..Mount::default()
        };
        m.mountpoint[..path.len()].copy_from_slice(path.as_bytes());
        m.mountpoint_len = path.len();
        self.mounts[self.len] = m;
        self.len += 1;
        Ok(self.len - 1)
    }

    pub fn umount(&mut self, path: &str) -> bool {
        match self.find(path) {
            Some(i) => {
                let last = self.len - 1;
                self.mounts[i] = self.mounts[last];
                self.mounts[last] = Mount::default();
                self.len -= 1;
                true
            }
            None => false,
        }
    }

    pub fn find(&self, path: &str) -> Option<usize> {
        (0..self.len).find(|i| self.mounts[*i].path() == path)
    }

    /// F132/F139: longest-prefix resolution. `/mnt/u/data` resolves through the
    /// `/mnt/u` mount, not through `/`.
    /// The returned relative path borrows from `path`, so the lifetimes are
    /// tied together explicitly.
    pub fn resolve<'a>(&self, path: &'a str) -> Option<(usize, &'a str)> {
        let mut best: Option<(usize, usize)> = None;
        for i in 0..self.len {
            let mp = self.mounts[i].path();
            let matches = path == mp
                || (path.starts_with(mp)
                    && (mp == "/" || path.as_bytes().get(mp.len()) == Some(&b'/')));
            if matches {
                let len = mp.len();
                if best.map(|(_, l)| len > l).unwrap_or(true) {
                    best = Some((i, len));
                }
            }
        }
        let (i, len) = best?;
        let rel = if path.len() == len {
            "/"
        } else if len == 1 {
            // Mounted at the root: the whole path is already relative to it.
            path
        } else {
            &path[len..]
        };
        Some((i, rel))
    }

    /// Every mount that is writable, for the deployment tooling (F433).
    pub fn writable_count(&self) -> usize {
        (0..self.len).filter(|i| self.mounts[*i].writable()).count()
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        for m in self.mounts[..self.len].iter() {
            w.str(m.path());
            w.str(" on ");
            w.str(m.fs.as_str());
            w.str(if m.read_only { " ro" } else { " rw" });
            w.str("\n");
        }
        w.used()
    }
}

impl Default for MountTable {
    fn default() -> MountTable {
        MountTable::new()
    }
}

// ---------------------------------------------------------------------------
// F133~F136 — on-disk superblocks
// ---------------------------------------------------------------------------

pub fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
pub fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
pub fn u64_at(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}

/// F133: FAT32 BPB. Validated, not merely parsed — a cluster size of zero or an
/// overlapping FAT would turn every subsequent read into a wild pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Fat32Info {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub fat_count: u8,
    pub root_entries: u16,
    pub total_sectors: u32,
    pub fat_size_sectors: u32,
    pub root_cluster: u32,
    pub fs_info_sector: u16,
}

impl Fat32Info {
    pub fn parse(boot: &[u8]) -> Result<Fat32Info, &'static str> {
        if boot.len() < 512 {
            return Err("boot sector is too short");
        }
        if boot[510] != 0x55 || boot[511] != 0xAA {
            return Err("missing boot signature");
        }
        let bytes_per_sector = u16_at(boot, 11);
        let sectors_per_cluster = boot[13];
        let reserved = u16_at(boot, 14);
        let fat_count = boot[16];
        let root_entries = u16_at(boot, 17);
        let total16 = u16_at(boot, 19) as u32;
        let fat16 = u16_at(boot, 22) as u32;
        let total32 = u32_at(boot, 32);
        let fat32 = u32_at(boot, 36);

        if bytes_per_sector != 512 && bytes_per_sector != 4096 {
            return Err("unsupported sector size");
        }
        if !sectors_per_cluster.is_power_of_two() || sectors_per_cluster == 0 {
            return Err("cluster size is not a power of two");
        }
        if reserved == 0 || fat_count == 0 {
            return Err("reserved sectors or FAT count is zero");
        }
        // A FAT32 volume has no root directory entries; if it does, this is a
        // FAT12/16 volume and must not be mounted by this driver.
        if root_entries != 0 {
            return Err("this is not a FAT32 volume");
        }
        let fat_size = if fat32 != 0 { fat32 } else { fat16 };
        if fat_size == 0 {
            return Err("FAT is empty");
        }
        let info = Fat32Info {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors: reserved,
            fat_count,
            root_entries,
            total_sectors: if total32 != 0 { total32 } else { total16 },
            fat_size_sectors: fat_size,
            root_cluster: u32_at(boot, 44) & 0x0FFF_FFFF,
            fs_info_sector: u16_at(boot, 48),
        };
        if info.root_cluster < 2 {
            return Err("root cluster is not a data cluster");
        }
        if info.cluster_bytes() == 0 {
            return Err("cluster is empty");
        }
        Ok(info)
    }

    pub fn cluster_bytes(&self) -> u32 {
        self.bytes_per_sector as u32 * self.sectors_per_cluster as u32
    }

    pub fn fat_start_lba(&self) -> u64 {
        self.reserved_sectors as u64
    }

    pub fn data_start_lba(&self) -> u64 {
        self.reserved_sectors as u64
            + self.fat_count as u64 * self.fat_size_sectors as u64
    }

    /// First sector of cluster `n` (clusters are numbered from 2).
    pub fn cluster_lba(&self, cluster: u32) -> Option<u64> {
        if cluster < 2 {
            return None;
        }
        Some(self.data_start_lba() + (cluster as u64 - 2) * self.sectors_per_cluster as u64)
    }

    pub fn cluster_count(&self) -> u64 {
        let data_sectors = self.total_sectors as u64 - self.data_start_lba();
        data_sectors / self.sectors_per_cluster as u64
    }

    /// Total capacity in bytes — cross-checked against the partition size when
    /// the volume is mounted.
    pub fn capacity_bytes(&self) -> u64 {
        self.total_sectors as u64 * self.bytes_per_sector as u64
    }

    /// A FAT32 end-of-chain marker.
    pub fn is_eoc(marker: u32) -> bool {
        marker >= 0x0FFF_FFF8
    }
}

/// F134: exFAT boot sector.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ExfatInfo {
    pub bytes_per_sector_shift: u8,
    pub sectors_per_cluster_shift: u8,
    pub fat_offset: u32,
    pub fat_length: u32,
    pub cluster_heap_offset: u32,
    pub cluster_count: u32,
    pub root_cluster: u32,
    pub volume_serial: u32,
}

impl ExfatInfo {
    pub const OEM: [u8; 8] = *b"EXFAT   ";

    pub fn parse(boot: &[u8]) -> Result<ExfatInfo, &'static str> {
        if boot.len() < 512 {
            return Err("boot sector is too short");
        }
        // The exFAT boot signature lives at offset 3, not 510.
        if boot[3..11] != Self::OEM {
            return Err("not an exFAT volume");
        }
        if boot[510] != 0x55 || boot[511] != 0xAA {
            return Err("missing boot signature");
        }
        let info = ExfatInfo {
            bytes_per_sector_shift: boot[108],
            sectors_per_cluster_shift: boot[109],
            fat_offset: u32_at(boot, 80),
            fat_length: u32_at(boot, 84),
            cluster_heap_offset: u32_at(boot, 88),
            cluster_count: u32_at(boot, 92),
            root_cluster: u32_at(boot, 96),
            volume_serial: u32_at(boot, 100),
        };
        // 2^9..=2^12 bytes per sector, and a cluster no larger than 32 MiB.
        if info.bytes_per_sector_shift < 9 || info.bytes_per_sector_shift > 12 {
            return Err("invalid sector size shift");
        }
        if info.sectors_per_cluster_shift > 25 - info.bytes_per_sector_shift {
            return Err("cluster size is out of range");
        }
        if info.fat_offset == 0 || info.cluster_heap_offset == 0 || info.cluster_count == 0 {
            return Err("volume geometry is empty");
        }
        if info.root_cluster < 2 {
            return Err("root cluster is not a data cluster");
        }
        Ok(info)
    }

    pub fn cluster_bytes(&self) -> u64 {
        1u64 << (self.bytes_per_sector_shift as u32 + self.sectors_per_cluster_shift as u32)
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.cluster_count as u64 * self.cluster_bytes()
    }

    /// exFAT needs a larger cluster than 4 KiB on volumes above 32 GiB; this is
    /// the interoperability rule that keeps Windows happy.
    pub fn cluster_size_ok_for_size(&self) -> bool {
        let cap = self.capacity_bytes();
        if cap > 32 * 1024 * 1024 * 1024 {
            self.cluster_bytes() > 4096
        } else {
            true
        }
    }
}

/// F135: ext2 superblock (read-only driver).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ext2Info {
    pub inode_count: u32,
    pub block_count: u32,
    pub first_data_block: u32,
    pub block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub inode_size: u16,
    pub magic: u16,
}

impl Ext2Info {
    pub const SUPERBLOCK_OFFSET: usize = 1024;
    pub const MAGIC: u16 = 0xEF53;

    pub fn parse(image: &[u8]) -> Result<Ext2Info, &'static str> {
        let at = Self::SUPERBLOCK_OFFSET;
        if image.len() < at + 128 {
            return Err("image is too small to hold a superblock");
        }
        let sb = &image[at..];
        let magic = u16_at(sb, 56);
        if magic != Self::MAGIC {
            return Err("not an ext2 filesystem");
        }
        let log_block_size = u32_at(sb, 24);
        if log_block_size > 6 {
            return Err("unsupported block size");
        }
        let info = Ext2Info {
            inode_count: u32_at(sb, 0),
            block_count: u32_at(sb, 4),
            first_data_block: u32_at(sb, 20),
            block_size: 1024u32 << log_block_size,
            blocks_per_group: u32_at(sb, 32),
            inodes_per_group: u32_at(sb, 40),
            inode_size: u16_at(sb, 88),
            magic,
        };
        if info.inode_count == 0 || info.block_count == 0 || info.inodes_per_group == 0 {
            return Err("empty filesystem geometry");
        }
        Ok(info)
    }

    pub fn usable_blocks(&self) -> u32 {
        self.block_count.saturating_sub(self.first_data_block)
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.block_count as u64 * self.block_size as u64
    }

    /// inode size defaults to 128 when the field is absent (ext2 rev 1).
    pub fn effective_inode_size(&self) -> u16 {
        if self.inode_size == 0 {
            128
        } else {
            self.inode_size
        }
    }
}

/// F136: NTFS boot sector. Read-only, by policy *and* by construction: this
/// structure is only ever used to compute offsets for reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NtfsInfo {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub total_sectors: u64,
    pub mft_cluster: u64,
    pub mft_mirror_cluster: u64,
    /// Signed in the on-disk format: a negative value means "2^|n| bytes".
    pub clusters_per_mft_record: i8,
}

impl NtfsInfo {
    pub const OEM: [u8; 8] = *b"NTFS    ";

    pub fn parse(boot: &[u8]) -> Result<NtfsInfo, &'static str> {
        if boot.len() < 512 {
            return Err("boot sector is too short");
        }
        if boot[3..11] != Self::OEM {
            return Err("not an NTFS volume");
        }
        if boot[510] != 0x55 || boot[511] != 0xAA {
            return Err("missing boot signature");
        }
        let bytes_per_sector = u16_at(boot, 11);
        let sectors_per_cluster = boot[13];
        if bytes_per_sector != 512 && bytes_per_sector != 4096 {
            return Err("unsupported sector size");
        }
        if !sectors_per_cluster.is_power_of_two() || sectors_per_cluster == 0 {
            return Err("cluster size is not a power of two");
        }
        let info = NtfsInfo {
            bytes_per_sector,
            sectors_per_cluster,
            total_sectors: u64_at(boot, 40),
            mft_cluster: u64_at(boot, 48),
            mft_mirror_cluster: u64_at(boot, 56),
            clusters_per_mft_record: boot[64] as i8,
        };
        if info.total_sectors == 0 || info.mft_cluster == 0 {
            return Err("volume geometry is empty");
        }
        Ok(info)
    }

    pub fn cluster_bytes(&self) -> u32 {
        self.bytes_per_sector as u32 * self.sectors_per_cluster as u32
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.total_sectors * self.bytes_per_sector as u64
    }

    /// Size of one MFT record. A negative exponent is the modern layout.
    pub fn mft_record_size(&self) -> u32 {
        let v = self.clusters_per_mft_record;
        if v > 0 {
            v as u32 * self.cluster_bytes()
        } else {
            1u32 << (v.unsigned_abs() as u32)
        }
    }

    /// The MFT never moves, and a writeable NTFS driver is out of scope: the
    /// only honest answer to "can I write this" is no (F231's read-only rule).
    pub fn writable(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// F137 — journalled writable filesystem
// ---------------------------------------------------------------------------

pub const MAX_JOURNAL_ENTRIES: usize = 32;
pub const JOURNAL_PAYLOAD: usize = 48;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JournalEntry {
    pub seq: u64,
    /// Target block, or `u64::MAX` for a metadata-only record.
    pub lba: u64,
    pub payload: [u8; JOURNAL_PAYLOAD],
    pub payload_len: usize,
    /// Simple additive checksum: cheap, and enough to catch a torn write.
    pub checksum: u32,
    pub committed: bool,
}

impl Default for JournalEntry {
    /// Hand-written because `[u8; 48]` has no `Default` impl — the payload is
    /// simply 48 zero bytes until a record is written into it.
    fn default() -> JournalEntry {
        JournalEntry {
            seq: 0,
            lba: 0,
            payload: [0u8; JOURNAL_PAYLOAD],
            payload_len: 0,
            checksum: 0,
            committed: false,
        }
    }
}

impl JournalEntry {
    pub fn checksum_of(seq: u64, lba: u64, payload: &[u8]) -> u32 {
        let mut sum = 0x811C_9DC5u32;
        for b in seq.to_le_bytes().iter().chain(lba.to_le_bytes().iter()).chain(payload.iter()) {
            sum ^= *b as u32;
            sum = sum.wrapping_mul(0x0100_0193);
        }
        sum
    }

    pub fn verify(&self) -> bool {
        self.checksum == Self::checksum_of(self.seq, self.lba, &self.payload[..self.payload_len])
    }
}

/// F137: write-ahead journal. The contract is the usual one: a transaction is
/// durable only after `commit` returns, and `replay` brings the device back to
/// the last committed state after a crash.
pub struct Journal {
    entries: [JournalEntry; MAX_JOURNAL_ENTRIES],
    len: usize,
    next_seq: u64,
    commits: u64,
    replays: u64,
    corrupt: u64,
}

impl Journal {
    pub const fn new() -> Journal {
        Journal {
            entries: [JournalEntry {
                seq: 0,
                lba: 0,
                payload: [0; JOURNAL_PAYLOAD],
                payload_len: 0,
                checksum: 0,
                committed: false,
            }; MAX_JOURNAL_ENTRIES],
            len: 0,
            next_seq: 1,
            commits: 0,
            replays: 0,
            corrupt: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn commits(&self) -> u64 {
        self.commits
    }

    pub fn replays(&self) -> u64 {
        self.replays
    }

    /// Entries rejected because their checksum did not verify.
    pub fn corrupt(&self) -> u64 {
        self.corrupt
    }

    pub fn append(&mut self, lba: u64, payload: &[u8]) -> Option<u64> {
        if self.len >= MAX_JOURNAL_ENTRIES || payload.len() > JOURNAL_PAYLOAD {
            return None;
        }
        let mut e = JournalEntry {
            seq: self.next_seq,
            lba,
            payload_len: payload.len(),
            ..JournalEntry::default()
        };
        e.payload[..payload.len()].copy_from_slice(payload);
        e.checksum = JournalEntry::checksum_of(e.seq, e.lba, payload);
        self.entries[self.len] = e;
        self.len += 1;
        self.next_seq += 1;
        Some(e.seq)
    }

    /// Commit everything up to and including `seq`. Until this returns, nothing
    /// in the journal is allowed to reach the device.
    pub fn commit(&mut self, seq: u64) -> usize {
        let mut n = 0;
        for e in self.entries[..self.len].iter_mut() {
            if e.seq <= seq && !e.committed {
                e.committed = true;
                n += 1;
            }
        }
        if n > 0 {
            self.commits += 1;
        }
        n
    }

    /// F137 recovery: return the committed entries in order, skipping anything
    /// whose checksum fails. Corrupt records are counted, not applied.
    pub fn replay(&mut self, out: &mut [JournalEntry]) -> usize {
        let mut n = 0usize;
        for i in 0..self.len {
            let e = self.entries[i];
            if !e.committed {
                continue;
            }
            if !e.verify() {
                self.corrupt += 1;
                continue;
            }
            if n < out.len() {
                out[n] = e;
                n += 1;
            }
        }
        self.replays += 1;
        n
    }

    pub fn reset(&mut self) {
        self.len = 0;
    }

    /// Highest committed sequence — what the on-disk superblock should record.
    pub fn committed_watermark(&self) -> u64 {
        self.entries[..self.len]
            .iter()
            .filter(|e| e.committed)
            .map(|e| e.seq)
            .max()
            .unwrap_or(0)
    }
}

impl Default for Journal {
    fn default() -> Journal {
        Journal::new()
    }
}

// ---------------------------------------------------------------------------
// F138 — file descriptor table
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FdKind {
    #[default]
    Unused,
    File,
    Dir,
    Device,
    Pipe,
}

pub const FD_CLOEXEC: u16 = 1;
pub const FD_APPEND: u16 = 2;

/// A descriptor: what the process sees. It names a file object rather than
/// carrying the offset itself, which is what makes `dup` share the position.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FdEntry {
    pub kind: FdKind,
    /// Index into the file-object table, or `NO_FILE`.
    pub file: u8,
    pub flags: u16,
}

pub const NO_FILE: u8 = u8::MAX;
pub const MAX_FILE_OBJECTS: usize = 32;

/// The open file itself: node, offset and a reference count.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FileObject {
    pub node: u64,
    pub offset: u64,
    pub kind: FdKind,
    pub refs: u16,
    pub used: bool,
}

pub struct FdTable {
    entries: [FdEntry; MAX_FDS],
    files: [FileObject; MAX_FILE_OBJECTS],
    limit: u32,
    next_hint: u32,
}

impl FdTable {
    pub const fn new(limit: u32) -> FdTable {
        FdTable {
            entries: [FdEntry {
                kind: FdKind::Unused,
                file: NO_FILE,
                flags: 0,
            }; MAX_FDS],
            files: [FileObject {
                node: 0,
                offset: 0,
                kind: FdKind::Unused,
                refs: 0,
                used: false,
            }; MAX_FILE_OBJECTS],
            limit,
            next_hint: 3, // 0/1/2 are stdin/stdout/stderr
        }
    }

    fn alloc_file(&mut self, kind: FdKind, node: u64) -> Option<u8> {
        let slot = self.files.iter().position(|f| !f.used)?;
        self.files[slot] = FileObject {
            node,
            offset: 0,
            kind,
            refs: 1,
            used: true,
        };
        Some(slot as u8)
    }

    fn alloc_descriptor(&mut self, entry: FdEntry) -> Option<u32> {
        let used = self.entries.iter().filter(|e| e.kind != FdKind::Unused).count() as u32;
        if used >= self.limit {
            return None;
        }
        let mut fd = self.next_hint;
        for _ in 0..MAX_FDS {
            let idx = (fd as usize) % MAX_FDS;
            if self.entries[idx].kind == FdKind::Unused {
                self.entries[idx] = entry;
                self.next_hint = (idx as u32 + 1) % MAX_FDS as u32;
                return Some(idx as u32);
            }
            fd += 1;
        }
        None
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }

    pub fn set_limit(&mut self, limit: u32) {
        self.limit = limit.min(MAX_FDS as u32);
    }

    /// Open a node: a fresh file object plus a descriptor pointing at it.
    pub fn open(&mut self, kind: FdKind, node: u64, flags: u16) -> Option<u32> {
        if self.entries.iter().filter(|e| e.kind != FdKind::Unused).count() as u32 >= self.limit {
            return None;
        }
        let file = self.alloc_file(kind, node)?;
        match self.alloc_descriptor(FdEntry {
            kind,
            file,
            flags,
        }) {
            Some(fd) => Some(fd),
            None => {
                // Never leave an orphaned file object behind.
                self.files[file as usize].used = false;
                None
            }
        }
    }

    pub fn get(&self, fd: u32) -> Option<FdEntry> {
        self.entries
            .get(fd as usize)
            .copied()
            .filter(|e| e.kind != FdKind::Unused && e.file != NO_FILE)
    }

    /// The file object behind a descriptor.
    pub fn file_of(&self, fd: u32) -> Option<FileObject> {
        let e = self.get(fd)?;
        let f = self.files.get(e.file as usize)?;
        if f.used {
            Some(*f)
        } else {
            None
        }
    }

    pub fn close(&mut self, fd: u32) -> bool {
        let entry = match self.get(fd) {
            Some(e) => e,
            None => return false,
        };
        let file = entry.file as usize;
        if let Some(f) = self.files.get_mut(file) {
            if f.refs > 1 {
                f.refs -= 1;
            } else {
                // Last reference: the file object is gone too.
                *f = FileObject::default();
            }
        }
        self.entries[fd as usize] = FdEntry::default();
        true
    }

    /// `dup`: a second descriptor onto the *same* file object, so the two
    /// share one offset. This is the whole reason descriptors and open files
    /// are separate tables.
    pub fn dup(&mut self, fd: u32) -> Option<u32> {
        let entry = self.get(fd)?;
        let allowed = entry.file != NO_FILE;
        if !allowed {
            return None;
        }
        if self.entries.iter().filter(|e| e.kind != FdKind::Unused).count() as u32 >= self.limit {
            return None;
        }
        let new_fd = self.alloc_descriptor(FdEntry {
            kind: entry.kind,
            file: entry.file,
            flags: entry.flags,
        })?;
        if let Some(f) = self.files.get_mut(entry.file as usize) {
            f.refs += 1;
        }
        Some(new_fd)
    }

    /// `exec` closes everything marked CLOEXEC.
    pub fn close_on_exec(&mut self) -> usize {
        let mut n = 0;
        for i in 0..MAX_FDS {
            if self.entries[i].kind != FdKind::Unused && self.entries[i].flags & FD_CLOEXEC != 0 {
                self.close(i as u32);
                n += 1;
            }
        }
        n
    }

    pub fn open_count(&self) -> usize {
        self.entries.iter().filter(|e| e.kind != FdKind::Unused).count()
    }

    pub fn file_count(&self) -> usize {
        self.files.iter().filter(|f| f.used).count()
    }

    pub fn seek(&mut self, fd: u32, offset: u64) -> bool {
        // O_APPEND only matters at write time; the seek itself is the same.
        let file = match self.get(fd) {
            Some(e) => e.file as usize,
            None => return false,
        };
        match self.files.get_mut(file) {
            Some(f) if f.used => {
                f.offset = offset;
                true
            }
            _ => false,
        }
    }

    pub fn advance(&mut self, fd: u32, bytes: u64) -> bool {
        let file = match self.get(fd) {
            Some(e) => e.file as usize,
            None => return false,
        };
        match self.files.get_mut(file) {
            Some(f) if f.used => {
                f.offset += bytes;
                true
            }
            _ => false,
        }
    }
}

impl Default for FdTable {
    fn default() -> FdTable {
        FdTable::new(MAX_FDS as u32)
    }
}

// ---------------------------------------------------------------------------
// F139 — directory service
// ---------------------------------------------------------------------------

pub const PATH_BYTES: usize = 256;

/// Normalize a path in place: collapse `//`, resolve `.` and `..`, drop a
/// trailing slash. Returns `None` when the path is not absolute or would climb
/// above the root — the two cases a naive implementation gets wrong.
pub fn normalize(path: &str, out: &mut [u8]) -> Option<usize> {
    if !path.starts_with('/') {
        return None;
    }
    let mut n = 0usize;
    // `components[i]` is the length *before* component i was written, so `..`
    // truncates to the end of the parent rather than to the start of the child.
    let mut components: [usize; 32] = [0; 32];
    let mut depth = 0usize;
    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                n = components[depth];
            }
            _ => {
                if n + 1 + part.len() > out.len() {
                    return None;
                }
                out[n] = b'/';
                n += 1;
                out[n..n + part.len()].copy_from_slice(part.as_bytes());
                if depth < 32 {
                    // The length before this component, i.e. the end of the
                    // parent — where `..` will truncate back to.
                    components[depth] = n - 1;
                    depth += 1;
                }
                n += part.len();
            }
        }
    }
    if n == 0 {
        if out.is_empty() {
            return None;
        }
        out[0] = b'/';
        n = 1;
    }
    Some(n)
}

/// `dirname`/`basename` pair used by the shell and by path resolution.
pub fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(0) => ("/", &path[1..]),
        Some(i) => (&path[..i], &path[i + 1..]),
        None => (".", path),
    }
}

pub fn extension(path: &str) -> Option<&str> {
    let (_, base) = split_path(path);
    match base.rfind('.') {
        Some(0) | None => None,
        Some(i) => Some(&base[i + 1..]),
    }
}

// ---------------------------------------------------------------------------
// F140 — file locks
// ---------------------------------------------------------------------------

pub const MAX_LOCKS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockKind {
    Shared,
    Exclusive,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FileLock {
    pub node: u64,
    pub owner: u32,
    pub kind: Option<LockKind>,
    pub waiters: u32,
}

pub struct LockTable {
    locks: [FileLock; MAX_LOCKS],
    len: usize,
    denied: u64,
    granted: u64,
}

impl LockTable {
    pub const fn new() -> LockTable {
        LockTable {
            locks: [FileLock {
                node: 0,
                owner: 0,
                kind: None,
                waiters: 0,
            }; MAX_LOCKS],
            len: 0,
            denied: 0,
            granted: 0,
        }
    }

    pub fn granted(&self) -> u64 {
        self.granted
    }

    /// F140: a refusal is reported with the current holder, so the caller can
    /// print "held by pid 42" instead of "busy".
    pub fn acquire(&mut self, node: u64, owner: u32, kind: LockKind) -> Result<(), (u32, LockKind)> {
        // Find the existing entry for this node.
        let mut slot = self.len;
        for i in 0..self.len {
            if self.locks[i].node == node {
                slot = i;
                break;
            }
        }
        if slot == self.len {
            if self.len >= MAX_LOCKS {
                self.denied += 1;
                return Err((0, kind));
            }
            self.locks[slot] = FileLock {
                node,
                owner,
                kind: Some(kind),
                waiters: 0,
            };
            self.len += 1;
            self.granted += 1;
            return Ok(());
        }
        let existing = self.locks[slot];
        if existing.owner == owner {
            // Re-acquiring (or upgrading) as the owner always succeeds.
            self.locks[slot].kind = Some(kind);
            self.granted += 1;
            return Ok(());
        }
        match (existing.kind, kind) {
            // Two readers are fine.
            (Some(LockKind::Shared), LockKind::Shared) => {
                self.granted += 1;
                Ok(())
            }
            (None, _) => {
                self.locks[slot].owner = owner;
                self.locks[slot].kind = Some(kind);
                self.granted += 1;
                Ok(())
            }
            (Some(k), _) => {
                self.locks[slot].waiters += 1;
                self.denied += 1;
                Err((existing.owner, k))
            }
        }
    }

    pub fn release(&mut self, node: u64, owner: u32) -> bool {
        for i in 0..self.len {
            if self.locks[i].node == node && self.locks[i].owner == owner {
                self.locks[i].kind = None;
                self.locks[i].owner = 0;
                return true;
            }
        }
        false
    }

    pub fn holder(&self, node: u64) -> Option<(u32, LockKind)> {
        self.locks[..self.len]
            .iter()
            .find(|l| l.node == node && l.kind.is_some())
            .map(|l| (l.owner, l.kind.unwrap_or(LockKind::Shared)))
    }

    pub fn denied(&self) -> u64 {
        self.denied
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for LockTable {
    fn default() -> LockTable {
        LockTable::new()
    }
}

// ---------------------------------------------------------------------------
// F141/F142 — async IO and resumable transfers
// ---------------------------------------------------------------------------

pub const MAX_AIO: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AioState {
    #[default]
    Free,
    Submitted,
    Completed,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AioSlot {
    pub id: u64,
    pub state: AioState,
    pub result_bytes: u64,
    pub error: Option<&'static str>,
}

pub struct AioQueue {
    slots: [AioSlot; MAX_AIO],
    submitted: u64,
    completed: u64,
}

impl AioQueue {
    pub const fn new() -> AioQueue {
        AioQueue {
            slots: [AioSlot {
                id: 0,
                state: AioState::Free,
                result_bytes: 0,
                error: None,
            }; MAX_AIO],
            submitted: 0,
            completed: 0,
        }
    }

    pub fn submit(&mut self, id: u64) -> Option<usize> {
        let slot = self.slots.iter().position(|s| s.state == AioState::Free)?;
        self.slots[slot] = AioSlot {
            id,
            state: AioState::Submitted,
            result_bytes: 0,
            error: None,
        };
        self.submitted += 1;
        Some(slot)
    }

    pub fn complete(&mut self, id: u64, bytes: u64) -> bool {
        for s in self.slots.iter_mut() {
            if s.id == id && s.state == AioState::Submitted {
                s.state = AioState::Completed;
                s.result_bytes = bytes;
                self.completed += 1;
                return true;
            }
        }
        false
    }

    pub fn fail(&mut self, id: u64, why: &'static str) -> bool {
        for s in self.slots.iter_mut() {
            if s.id == id && s.state == AioState::Submitted {
                s.state = AioState::Failed;
                s.error = Some(why);
                return true;
            }
        }
        false
    }

    /// F141: reap finished operations; the caller owns the memory behind them.
    pub fn polls(&mut self, out: &mut [AioSlot]) -> usize {
        let mut n = 0usize;
        for s in self.slots.iter_mut() {
            if matches!(s.state, AioState::Completed | AioState::Failed) {
                if n < out.len() {
                    out[n] = *s;
                    n += 1;
                }
                *s = AioSlot::default();
            }
        }
        n
    }

    pub fn in_flight(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.state == AioState::Submitted)
            .count()
    }

    pub fn submitted(&self) -> u64 {
        self.submitted
    }

    pub fn completed(&self) -> u64 {
        self.completed
    }
}

impl Default for AioQueue {
    fn default() -> AioQueue {
        AioQueue::new()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TransferResume {
    pub key: u64,
    pub offset: u64,
    pub total: u64,
    pub checksum: u32,
    pub retries: u32,
}

impl TransferResume {
    pub fn new(key: u64, total: u64) -> TransferResume {
        TransferResume {
            key,
            offset: 0,
            total,
            checksum: 0,
            retries: 0,
        }
    }

    pub fn progress_percent(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        self.offset * 100 / self.total
    }

    /// F142: where to resume from. A transfer that got further than the
    /// recorded offset must not be trusted, so the record wins.
    pub fn resume_at(&self) -> u64 {
        self.offset.min(self.total)
    }

    pub fn advance(&mut self, bytes: u64) {
        self.offset = (self.offset + bytes).min(self.total);
    }

    pub fn complete(&self) -> bool {
        self.offset >= self.total && self.total > 0
    }

    /// Feed a byte into the running checksum so a partial transfer can be
    /// verified without re-reading the whole source.
    pub fn update_checksum(&mut self, bytes: &[u8]) {
        let mut c = self.checksum;
        for b in bytes {
            c ^= *b as u32;
            c = c.wrapping_mul(0x0100_0193);
        }
        self.checksum = c;
    }
}

pub struct ResumeTable {
    entries: [TransferResume; 8],
    len: usize,
}

impl ResumeTable {
    pub const fn new() -> ResumeTable {
        ResumeTable {
            entries: [TransferResume {
                key: 0,
                offset: 0,
                total: 0,
                checksum: 0,
                retries: 0,
            }; 8],
            len: 0,
        }
    }

    pub fn begin(&mut self, key: u64, total: u64) -> Option<usize> {
        if self.len >= 8 {
            return None;
        }
        self.entries[self.len] = TransferResume::new(key, total);
        self.len += 1;
        Some(self.len - 1)
    }

    pub fn get(&self, key: u64) -> Option<TransferResume> {
        self.entries[..self.len].iter().copied().find(|e| e.key == key)
    }

    pub fn get_mut(&mut self, key: u64) -> Option<&mut TransferResume> {
        self.entries[..self.len].iter_mut().find(|e| e.key == key)
    }

    /// Resume after an interruption: the offset survives, the retry counter
    /// tells the caller how many attempts this file has already cost.
    pub fn resume(&mut self, key: u64) -> Option<u64> {
        let e = self.get_mut(key)?;
        e.retries += 1;
        Some(e.resume_at())
    }

    pub fn finish(&mut self, key: u64) -> bool {
        match self.entries[..self.len].iter().position(|e| e.key == key) {
            Some(i) => {
                let last = self.len - 1;
                self.entries[i] = self.entries[last];
                self.len = last;
                true
            }
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for ResumeTable {
    fn default() -> ResumeTable {
        ResumeTable::new()
    }
}

// ---------------------------------------------------------------------------
// F143~F145 — device health, TRIM, bad blocks
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Health {
    #[default]
    Unknown,
    Pass,
    Warn,
    Fail,
}

impl Health {
    pub fn as_str(self) -> &'static str {
        match self {
            Health::Unknown => "unknown",
            Health::Pass => "pass",
            Health::Warn => "warn",
            Health::Fail => "fail",
        }
    }
}

/// F143: SMART-derived health. Thresholds are deliberately conservative: a disk
/// that is merely "warm" should be reported as warm, not as an emergency.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SmartData {
    pub temperature_c: u8,
    pub power_on_hours: u32,
    pub reallocated_sectors: u32,
    pub pending_sectors: u32,
    pub wear_percent: u8,
    pub power_cycles: u32,
}

pub const SMART_TEMP_WARN_C: u8 = 55;
pub const SMART_WEAR_WARN_PERCENT: u8 = 80;
pub const SMART_REALLOC_WARN: u32 = 16;

impl SmartData {
    pub fn health(&self) -> Health {
        if self.wear_percent >= 100 || self.pending_sectors > 0 || self.reallocated_sectors > SMART_REALLOC_WARN * 4 {
            return Health::Fail;
        }
        if self.temperature_c >= SMART_TEMP_WARN_C
            || self.wear_percent >= SMART_WEAR_WARN_PERCENT
            || self.reallocated_sectors > SMART_REALLOC_WARN
        {
            return Health::Warn;
        }
        Health::Pass
    }

    /// Whether the drive is a solid-state device with finite write endurance —
    /// what decides if TRIM (F144) and wear (F272) are worth tracking.
    pub fn endurance_limited(&self) -> bool {
        self.wear_percent > 0
    }
}

/// F144: batched TRIM. One command per range wastes the device's command
/// budget, so ranges are collected and flushed in one go.
pub const MAX_TRIM_RANGES: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TrimRange {
    pub lba: u64,
    pub blocks: u32,
}

pub struct TrimQueue {
    ranges: [TrimRange; MAX_TRIM_RANGES],
    len: usize,
    flushed: u64,
    trimmed_blocks: u64,
}

impl TrimQueue {
    pub const fn new() -> TrimQueue {
        TrimQueue {
            ranges: [TrimRange { lba: 0, blocks: 0 }; MAX_TRIM_RANGES],
            len: 0,
            flushed: 0,
            trimmed_blocks: 0,
        }
    }

    /// Queue a range. Adjacent ranges merge, so a directory delete that frees
    /// hundreds of consecutive blocks becomes one command.
    pub fn add(&mut self, lba: u64, blocks: u32) -> bool {
        if blocks == 0 {
            return false;
        }
        for r in self.ranges[..self.len].iter_mut() {
            if r.lba + r.blocks as u64 == lba {
                r.blocks += blocks;
                return true;
            }
            if lba + blocks as u64 == r.lba {
                r.lba = lba;
                r.blocks += blocks;
                return true;
            }
        }
        if self.len >= MAX_TRIM_RANGES {
            self.flush();
        }
        self.ranges[self.len] = TrimRange { lba, blocks };
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Hand the batch to the device; returns how many ranges went out.
    pub fn flush(&mut self) -> usize {
        let n = self.len;
        for r in self.ranges[..n].iter() {
            self.trimmed_blocks += r.blocks as u64;
        }
        self.len = 0;
        if n > 0 {
            self.flushed += 1;
        }
        n
    }

    pub fn flushed_batches(&self) -> u64 {
        self.flushed
    }

    pub fn trimmed_blocks(&self) -> u64 {
        self.trimmed_blocks
    }

    pub fn has_capacity(&self) -> bool {
        self.len < MAX_TRIM_RANGES
    }
}

impl Default for TrimQueue {
    fn default() -> TrimQueue {
        TrimQueue::new()
    }
}

pub const MAX_BAD_BLOCKS: usize = 64;

/// F145: bad-block remapping. A failed LBA is retired and every future access
/// is redirected to a spare, so a single dying sector degrades into slightly
/// less capacity instead of an I/O error the filesystem cannot survive.
pub struct BadBlockMap {
    bad: [u64; MAX_BAD_BLOCKS],
    remap: [u64; MAX_BAD_BLOCKS],
    len: usize,
    remapped: u64,
    unmapped: u64,
}

impl BadBlockMap {
    pub const fn new() -> BadBlockMap {
        BadBlockMap {
            bad: [0; MAX_BAD_BLOCKS],
            remap: [0; MAX_BAD_BLOCKS],
            len: 0,
            remapped: 0,
            unmapped: 0,
        }
    }

    /// Retire `lba` and point it at `spare`.
    pub fn mark_bad(&mut self, lba: u64, spare: u64) -> bool {
        if self.is_bad(lba) || self.len >= MAX_BAD_BLOCKS {
            return false;
        }
        self.bad[self.len] = lba;
        self.remap[self.len] = spare;
        self.len += 1;
        true
    }

    pub fn is_bad(&self, lba: u64) -> bool {
        self.bad[..self.len].contains(&lba)
    }

    /// Where a read of `lba` should actually go.
    pub fn resolve(&mut self, lba: u64) -> u64 {
        for i in 0..self.len {
            if self.bad[i] == lba {
                self.remapped += 1;
                return self.remap[i];
            }
        }
        lba
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn remapped(&self) -> u64 {
        self.remapped
    }

    /// An access with nowhere to go — the filesystem must be told, not lied to.
    pub fn unmapped(&self) -> u64 {
        self.unmapped
    }

    pub fn note_unmapped(&mut self) {
        self.unmapped += 1;
    }

    /// Spare capacity is finite; this is what the health report shows (F143).
    pub fn spares_exhausted(&self) -> bool {
        self.len >= MAX_BAD_BLOCKS
    }
}

impl Default for BadBlockMap {
    fn default() -> BadBlockMap {
        BadBlockMap::new()
    }
}

// ---------------------------------------------------------------------------
// F146/F147 — images and shared data partitions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageKind {
    /// Flat image: LBAs map one to one.
    Raw,
    /// Copy-on-write image: reads come from the diff when it has the block.
    Differential,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DiffExtent {
    pub lba: u64,
    pub blocks: u32,
    pub backing_lba: u64,
}

pub struct DiskImage {
    pub device: u8,
    pub kind: ImageKind,
    pub base_blocks: u64,
    diff: [DiffExtent; 64],
    diff_len: usize,
    reads_from_diff: u64,
    reads_from_base: u64,
}

impl DiskImage {
    pub const fn new(device: u8, kind: ImageKind) -> DiskImage {
        DiskImage {
            device,
            kind,
            base_blocks: 0,
            diff: [DiffExtent {
                lba: 0,
                blocks: 0,
                backing_lba: 0,
            }; 64],
            diff_len: 0,
            reads_from_diff: 0,
            reads_from_base: 0,
        }
    }

    pub fn add_extent(&mut self, lba: u64, blocks: u32, backing_lba: u64) -> bool {
        if self.diff_len >= 64 || blocks == 0 {
            return false;
        }
        self.diff[self.diff_len] = DiffExtent {
            lba,
            blocks,
            backing_lba,
        };
        self.diff_len += 1;
        true
    }

    /// F146: where a read of `lba` actually comes from.
    pub fn resolve(&mut self, lba: u64) -> (u64, bool) {
        for e in self.diff[..self.diff_len].iter() {
            if lba >= e.lba && lba < e.lba + e.blocks as u64 {
                self.reads_from_diff += 1;
                return (e.backing_lba + (lba - e.lba), true);
            }
        }
        self.reads_from_base += 1;
        (lba, false)
    }

    pub fn extents(&self) -> usize {
        self.diff_len
    }

    pub fn reads_from_diff(&self) -> u64 {
        self.reads_from_diff
    }

    pub fn reads_from_base(&self) -> u64 {
        self.reads_from_base
    }

    /// Diff coverage as a percentage — how much of the image has been written
    /// since the snapshot was taken.
    pub fn coverage_percent(&self) -> u64 {
        if self.base_blocks == 0 {
            return 0;
        }
        let covered: u64 = self.diff[..self.diff_len].iter().map(|e| e.blocks as u64).sum();
        (covered * 100 / self.base_blocks).min(100)
    }
}

/// F147: the shared data partition. This is the U-disk area all three systems
/// mount read-write, so its geometry has to be expressible without any
/// filesystem-specific assumption.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DataPartition {
    pub lba_start: u64,
    pub lba_end: u64,
    pub label: [u8; 16],
    pub label_len: usize,
    /// Filesystem every system can read (exFAT by policy).
    pub fs: FsKind,
}

impl DataPartition {
    pub fn with_label(mut self, label: &str) -> DataPartition {
        let n = label.len().min(16);
        self.label[..n].copy_from_slice(&label.as_bytes()[..n]);
        self.label_len = n;
        self
    }

    pub fn label_str(&self) -> &str {
        core::str::from_utf8(&self.label[..self.label_len]).unwrap_or("?")
    }

    pub fn blocks(&self) -> u64 {
        self.lba_end.saturating_sub(self.lba_start)
    }

    pub fn contains(&self, lba: u64) -> bool {
        lba >= self.lba_start && lba < self.lba_end
    }

    /// F147: every system that boots with Varix must be able to read this.
    pub fn shared_compatible(&self) -> bool {
        matches!(self.fs, FsKind::ExFat | FsKind::Fat32)
    }
}

// ---------------------------------------------------------------------------
// F148/F149 — events and fragmentation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StorageEventKind {
    Inserted,
    Removed,
    Error,
    Trimmed,
    HealthChanged,
}

impl StorageEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StorageEventKind::Inserted => "inserted",
            StorageEventKind::Removed => "removed",
            StorageEventKind::Error => "error",
            StorageEventKind::Trimmed => "trimmed",
            StorageEventKind::HealthChanged => "health",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StorageEvent {
    pub tick: u64,
    pub device: u8,
    pub kind: Option<StorageEventKind>,
    pub detail: i64,
}

pub const MAX_STORAGE_EVENTS: usize = 32;

pub struct StorageEvents {
    events: [StorageEvent; MAX_STORAGE_EVENTS],
    head: usize,
    total: u64,
}

impl StorageEvents {
    pub const fn new() -> StorageEvents {
        StorageEvents {
            events: [StorageEvent {
                tick: 0,
                device: 0,
                kind: None,
                detail: 0,
            }; MAX_STORAGE_EVENTS],
            head: 0,
            total: 0,
        }
    }

    pub fn push(&mut self, e: StorageEvent) {
        self.events[self.head] = e;
        self.head = (self.head + 1) % MAX_STORAGE_EVENTS;
        self.total += 1;
    }

    pub fn len(&self) -> usize {
        (self.total as usize).min(MAX_STORAGE_EVENTS)
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn get(&self, offset: usize) -> Option<StorageEvent> {
        if offset >= self.len() {
            return None;
        }
        Some(self.events[(self.head + MAX_STORAGE_EVENTS - 1 - offset) % MAX_STORAGE_EVENTS])
    }

    pub fn count(&self, kind: StorageEventKind) -> u64 {
        (0..self.len())
            .filter(|i| self.get(*i).map(|e| e.kind == Some(kind)).unwrap_or(false))
            .count() as u64
    }
}

impl Default for StorageEvents {
    fn default() -> StorageEvents {
        StorageEvents::new()
    }
}

/// F149: fragmentation as a number a user can act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Fragmentation {
    pub used_extents: u64,
    pub used_blocks: u64,
    pub free_extents: u64,
    pub free_blocks: u64,
}

impl Fragmentation {
    /// 100% = perfectly contiguous; lower means more extents per byte.
    pub fn contiguity_percent(&self) -> u64 {
        if self.used_blocks == 0 || self.used_extents == 0 {
            return 100;
        }
        let ideal = self.used_blocks;
        let actual = self.used_extents;
        (ideal.min(actual) * 100 / actual.max(1)).min(100)
    }

    pub fn free_percent(&self) -> u64 {
        let total = self.used_blocks + self.free_blocks;
        if total == 0 {
            return 0;
        }
        self.free_blocks * 100 / total
    }

    /// Only worth defragmenting when it is both fragmented and half empty.
    pub fn advise_defrag(&self) -> bool {
        self.contiguity_percent() < 60 && self.free_percent() > 25
    }

    pub fn as_str(&self) -> &'static str {
        if self.advise_defrag() {
            "defragment"
        } else if self.contiguity_percent() < 80 {
            "moderate"
        } else {
            "healthy"
        }
    }
}

// ---------------------------------------------------------------------------
// Domain bring-up and self-test
// ---------------------------------------------------------------------------

pub struct StorageDomainState {
    pub devices: usize,
    pub mounts: usize,
    pub writable_mounts: usize,
    pub cache_hit_percent: u64,
    pub io_merged: u64,
    pub journal_entries: usize,
    pub self_test: (usize, usize),
}

impl StorageDomainState {
    pub fn ok(&self) -> bool {
        self.self_test.1 == 0
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("storage dev=");
        w.num(self.devices as u64);
        w.str(" mount=");
        w.num(self.mounts as u64);
        w.str("/");
        w.num(self.writable_mounts as u64);
        w.str("rw cache=");
        w.num(self.cache_hit_percent);
        w.str("% merged=");
        w.num(self.io_merged);
        w.str(" journal=");
        w.num(self.journal_entries as u64);
        w.str(" [");
        w.num(self.self_test.0 as u64);
        w.str("/");
        w.num((self.self_test.0 + self.self_test.1) as u64);
        w.str("]\n");
        w.used()
    }
}

/// The live storage state. One instance, because there is one block layer.
pub struct Storage {
    pub blocks: BlockLayer,
    pub cache: BlockCache,
    pub io: IoQueue,
    pub mounts: MountTable,
    pub events: StorageEvents,
    pub journal: Journal,
    pub bad: BadBlockMap,
    pub trim: TrimQueue,
    pub smart: SmartData,
    pub data_partition: Option<DataPartition>,
    pub seq: u64,
}

impl Storage {
    pub const fn new() -> Storage {
        Storage {
            blocks: BlockLayer::new(),
            cache: BlockCache::new(),
            io: IoQueue::new(),
            mounts: MountTable::new(),
            events: StorageEvents::new(),
            journal: Journal::new(),
            bad: BadBlockMap::new(),
            trim: TrimQueue::new(),
            smart: SmartData {
                temperature_c: 0,
                power_on_hours: 0,
                reallocated_sectors: 0,
                pending_sectors: 0,
                wear_percent: 0,
                power_cycles: 0,
            },
            data_partition: None,
            seq: 0,
        }
    }

    pub fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// F126/F128: register a device and notify (F148).
    pub fn attach(&mut self, dev: BlockDevice) -> Option<u8> {
        let slot = self.blocks.register(dev)?;
        self.events.push(StorageEvent {
            tick: crate::cpu::clock::ticks(),
            device: slot,
            kind: Some(StorageEventKind::Inserted),
            detail: self.blocks.get(slot).map(|d| d.blocks as i64).unwrap_or(0),
        });
        Some(slot)
    }

    pub fn detach(&mut self, slot: u8) -> bool {
        if self.blocks.get(slot).is_none() {
            return false;
        }
        self.blocks.devices[slot as usize] = None;
        self.cache.invalidate_device(slot);
        self.events.push(StorageEvent {
            tick: crate::cpu::clock::ticks(),
            device: slot,
            kind: Some(StorageEventKind::Removed),
            detail: 0,
        });
        true
    }

    /// The standard Varix layout: the three-systems-visible data area, a
    /// read-only NTFS view of the Windows data区, and the journalled system
    /// volume. All of it is refused if the geometry does not add up.
    pub fn apply_standard_layout(&mut self) -> usize {
        let mut mounted = 0usize;
        if self.mounts.mount("/", FsKind::Varix, 0, false, true).is_ok() {
            mounted += 1;
        }
        if self.mounts.mount("/mnt/data", FsKind::ExFat, 1, false, true).is_ok() {
            mounted += 1;
        }
        if self.mounts.mount("/mnt/windows", FsKind::Ntfs, 2, true, true).is_ok() {
            mounted += 1;
        }
        if self.mounts.mount("/mnt/rescue", FsKind::Fat32, 3, true, true).is_ok() {
            mounted += 1;
        }
        mounted
    }
}

impl Default for Storage {
    fn default() -> Storage {
        Storage::new()
    }
}

static STORAGE: crate::cpu::sync::SpinProtected<Storage> =
    crate::cpu::sync::SpinProtected::new(Storage::new());

pub fn storage() -> &'static crate::cpu::sync::SpinProtected<Storage> {
    &STORAGE
}

/// F126~F149 bring-up: nothing to probe yet, so this arms the layers and
/// reports what a real boot would have found.
pub fn init() -> StorageDomainState {
    let mut s = STORAGE.lock();
    let mounted = s.apply_standard_layout();
    let (passed, failed) = run_storage_checks(&mut s);
    let state = StorageDomainState {
        devices: s.blocks.len(),
        mounts: s.mounts.len(),
        writable_mounts: s.mounts.writable_count(),
        cache_hit_percent: s.cache.hit_percent(),
        io_merged: s.io.merged(),
        journal_entries: s.journal.len(),
        self_test: (passed, failed),
    };
    let _ = mounted;
    drop(s);

    crate::kinfo!(
        "storage: {} devices, {} mounts ({} writable), self-test {}/{}",
        state.devices,
        state.mounts,
        state.writable_mounts,
        state.self_test.0,
        state.self_test.0 + state.self_test.1
    );
    state
}

pub fn render_to_console(st: &StorageDomainState) {
    let mut buf = [0u8; 256];
    let n = st.render(&mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
    let mut detail = [0u8; 512];
    let m = storage_selftest().render(&mut detail);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &detail[..m] {
            c.put_byte(b);
        }
    }
}

static STORAGE_SELFTEST: crate::selftest::SelfTest = crate::selftest::SelfTest::new();

pub fn storage_selftest() -> &'static crate::selftest::SelfTest {
    &STORAGE_SELFTEST
}

/// F150: the storage-chain checks.
pub fn run_storage_checks(s: &mut Storage) -> (usize, usize) {
    let r = &STORAGE_SELFTEST;

    // F128 — the block layer refuses out-of-range and read-only writes.
    let dev = BlockDevice {
        slot: 0,
        kind: DeviceKind::Ahci,
        block_size: SECTOR as u32,
        blocks: 1024,
        read_only: true,
        removable: false,
        queue_depth: 32,
        name: "selftest",
    };
    let slot = s.blocks.register(dev);
    let mut range_ok = true;
    let mut ro_ok = true;
    if let Some(slot) = slot {
        let bad = BlockRequest {
            device: slot,
            op: BlockOp::Read,
            lba: 1020,
            blocks: 8,
            seq: 0,
        };
        range_ok = s.blocks.validate(&bad).is_err();
        let write = BlockRequest {
            device: slot,
            op: BlockOp::Write,
            lba: 0,
            blocks: 1,
            seq: 0,
        };
        ro_ok = s.blocks.validate(&write).is_err();
    }
    r.check("block-range", range_ok, "out-of-range request accepted");
    r.check("block-readonly", ro_ok, "write to a read-only device accepted");

    // F132 — a read-only filesystem cannot be mounted writable.
    let mut m = MountTable::new();
    let _ = m.mount("/mnt/ntfs", FsKind::Ntfs, 0, false, true);
    let ntfs_ro = m.get(0).map(|x| x.read_only).unwrap_or(false);
    r.check("ntfs-read-only", ntfs_ro, "NTFS mounted writable (F136)");

    // F137 — the journal is durable only after commit, and replay skips
    // uncommitted and corrupt records.
    let mut j = Journal::new();
    let seq = j.append(100, b"data").unwrap_or(0);
    let before = {
        let mut out = [JournalEntry::default(); 4];
        j.replay(&mut out)
    };
    j.commit(seq);
    let after = {
        let mut out = [JournalEntry::default(); 4];
        j.replay(&mut out)
    };
    r.check(
        "journal-durability",
        before == 0 && after == 1,
        "an uncommitted record was replayed, or a committed one was lost",
    );

    // F133/F134/F135/F136 — the parsers reject a zeroed sector rather than
    // inventing geometry from it.
    let zero = [0u8; 512];
    r.check(
        "fs-parsers",
        Fat32Info::parse(&zero).is_err()
            && ExfatInfo::parse(&zero).is_err()
            && NtfsInfo::parse(&zero).is_err()
            && Ext2Info::parse(&[0u8; 2048]).is_err(),
        "a zeroed superblock was accepted",
    );

    // F138 — descriptors are bounded by the process limit.
    let mut fds = FdTable::new(4);
    let a = fds.open(FdKind::File, 1, 0);
    let b = fds.open(FdKind::File, 2, FD_CLOEXEC);
    let c = fds.open(FdKind::File, 3, 0);
    let d = fds.open(FdKind::File, 4, 0);
    let e = fds.open(FdKind::File, 5, 0);
    r.check(
        "fd-limit",
        a.is_some() && b.is_some() && c.is_some() && d.is_some() && e.is_none(),
        "fd limit not enforced",
    );
    r.check(
        "fd-cloexec",
        fds.close_on_exec() == 1,
        "CLOEXEC descriptors survived exec",
    );

    // F139 — path normalization cannot climb above the root.
    let mut buf = [0u8; PATH_BYTES];
    let n = normalize("/a/./b/../c//", &mut buf).unwrap_or(0);
    let ok = core::str::from_utf8(&buf[..n]).map(|p| p == "/a/c").unwrap_or(false);
    r.check("path-normalize", ok, "path normalization produced the wrong result");
    r.check(
        "path-escape",
        normalize("/../etc", &mut buf).is_none() && normalize("relative", &mut buf).is_none(),
        "a relative or escaping path was accepted",
    );

    // F145 — a retired block never reaches the device again.
    let mut bad = BadBlockMap::new();
    let marked = bad.mark_bad(7, 900);
    r.check(
        "bad-block-remap",
        marked && bad.resolve(7) == 900 && bad.resolve(8) == 8,
        "bad-block remapping is wrong",
    );

    let (passed, failed) = r.tally();
    if failed == 0 {
        crate::kinfo!("storage self-test: {}/{} pass", passed, passed);
    } else {
        crate::kwarn!(
            "storage self-test: {}/{} pass ({} FAIL)",
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

    fn ata_device(blocks: u64, read_only: bool) -> BlockDevice {
        BlockDevice {
            slot: 0,
            kind: DeviceKind::Ahci,
            block_size: SECTOR as u32,
            blocks,
            read_only,
            removable: false,
            queue_depth: 32,
            name: "sda",
        }
    }

    #[test]
    fn block_layer_enforces_range_and_protection() {
        let mut b = BlockLayer::new();
        let slot = b.register(ata_device(2048, false)).unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b.get(slot).unwrap().capacity_bytes(), 2048 * 512);
        assert_eq!(b.get(slot).unwrap().sectors_512(), 2048);

        let ok = BlockRequest { device: slot, op: BlockOp::Read, lba: 0, blocks: 8, seq: 1 };
        assert!(b.validate(&ok).is_ok());
        assert_eq!(b.reads(), 1);
        let straddle = BlockRequest { device: slot, op: BlockOp::Read, lba: 2044, blocks: 8, seq: 2 };
        assert_eq!(b.validate(&straddle).unwrap_err(), "request runs past the end of the device");
        let zero = BlockRequest { device: slot, op: BlockOp::Read, lba: 0, blocks: 0, seq: 3 };
        assert_eq!(b.validate(&zero).unwrap_err(), "zero-length request");
        let ghost = BlockRequest { device: 9, op: BlockOp::Read, lba: 0, blocks: 1, seq: 4 };
        assert_eq!(b.validate(&ghost).unwrap_err(), "no such block device");
        assert_eq!(b.rejected(), 3);

        let mut ro = BlockLayer::new();
        let s = ro.register(ata_device(64, true)).unwrap();
        let w = BlockRequest { device: s, op: BlockOp::Write, lba: 0, blocks: 1, seq: 0 };
        assert_eq!(ro.validate(&w).unwrap_err(), "device is read-only");
        let t = BlockRequest { device: s, op: BlockOp::Trim, lba: 0, blocks: 1, seq: 0 };
        assert!(ro.validate(&t).is_err());
        let r = BlockRequest { device: s, op: BlockOp::Read, lba: 0, blocks: 1, seq: 0 };
        assert!(ro.validate(&r).is_ok(), "reads still work on a read-only device");
    }

    #[test]
    fn ahci_fis_round_trips_and_carries_the_full_lba() {
        let fis = RegisterH2dFis::for_dma(ATA_CMD_WRITE_DMA_EX, 0x0_1234_5678_9ABC, 256);
        let wire = fis.encode();
        assert_eq!(wire.len(), RegisterH2dFis::LEN);
        assert_eq!(wire[0], FIS_TYPE_REG_H2D);
        assert_eq!(wire[1] & 0x80, 0x80, "the command bit must be set");
        assert_eq!(wire[2], ATA_CMD_WRITE_DMA_EX);
        let back = RegisterH2dFis::decode(&wire).expect("decodes");
        assert_eq!(back.command, fis.command);
        assert_eq!(back.count, 256);
        assert_eq!(back.lba48(), 0x0_1234_5678_9ABC);

        let mut bad = wire;
        bad[0] = 0x34;
        assert!(RegisterH2dFis::decode(&bad).is_none());

        // IDENTIFY capacity: words 60/61 when the 48-bit field is empty.
        let mut identify = [0u8; 512];
        identify[120] = 0x00;
        identify[121] = 0x02; // word 60 = 0x0200 → 512
        identify[122] = 0x01;
        identify[123] = 0x00; // word 61 = 0x0001
        assert_eq!(AhciController::capacity_from_identify(&identify), (1 << 16) | 512);

        let c = AhciController {
            ports: [AhciPort::default(); 32],
            port_count: 4,
            version: 0x0001_0100,
            cap: (1 << 31) | (1 << 30),
        };
        assert!(c.supports_64bit());
        assert!(c.supports_ncq());
        assert_eq!(c.attached(), 0);
        assert!(c.identify_request(0).is_none(), "empty port has nothing to identify");
    }

    #[test]
    fn nvme_queue_wraps_and_flips_the_phase() {
        let mut q = NvmeQueue::with_depth(1, 4);
        for _ in 0..4 {
            assert!(q.submit().is_some());
        }
        assert!(q.full());
        assert!(q.submit().is_none(), "a full queue refuses rather than overwrites");
        assert_eq!(q.in_flight, 4);
        let start_phase = q.phase;
        for _ in 0..4 {
            assert!(q.complete().is_some());
        }
        assert!(!q.full());
        assert_ne!(q.phase, start_phase, "the phase bit flipped on wrap");
        assert!(q.complete().is_none(), "nothing left to complete");

        let cmd = NvmeCommand {
            opcode: NVME_OP_READ,
            nsid: 1,
            lba: 0x1_0000_0000,
            nlb: 7,
            cid: 0x1234,
        };
        let wire = cmd.encode();
        let back = NvmeCommand::decode(&wire);
        assert_eq!(back, cmd);
        assert_eq!(back.blocks(), 8, "nlb is zero-based");
        assert_eq!(back.opcode, NVME_OP_READ);

        // dword 3: cid = 0x1234, phase = 1, status = 0.
        let mut cbytes = [0u8; 16];
        let dw3: u32 = 0x1234 | (1 << 16);
        cbytes[12..16].copy_from_slice(&dw3.to_le_bytes());
        let c = NvmeCompletion::from_bytes(&cbytes);
        assert!(c.ok());
        assert!(c.phase);
        assert_eq!(c.cid, 0x1234);
        assert_eq!(c.status_name(), "ok");
        // status = 4 (data transfer error), phase = 1.
        let mut err = [0u8; 16];
        let dw3: u32 = (4 << 17) | (1 << 16);
        err[12..16].copy_from_slice(&dw3.to_le_bytes());
        assert!(!NvmeCompletion::from_bytes(&err).ok());
        assert_eq!(
            NvmeCompletion::from_bytes(&err).status_name(),
            "data transfer error"
        );
    }

    #[test]
    fn io_scheduler_merges_and_orders_by_deadline() {
        let mut q = IoQueue::new();
        assert!(q.is_empty());
        let a = IoRequest {
            id: 1,
            device: 0,
            op: BlockOp::Read,
            lba: 100,
            blocks: 8,
            priority: 1,
            submit_tick: 1,
            deadline_tick: 0,
        };
        assert_eq!(q.submit(a), Some(0));
        // Adjacent read coalesces into the same slot.
        let b = IoRequest { id: 2, lba: 108, blocks: 8, ..a };
        assert_eq!(q.submit(b), Some(0));
        assert_eq!(q.len(), 1);
        assert_eq!(q.merged(), 1);
        // A write is a different operation and never merges with a read.
        let w = IoRequest { id: 3, op: BlockOp::Write, lba: 116, blocks: 8, ..a };
        assert!(q.submit(w).is_some());
        assert_eq!(q.len(), 2);

        // Deadlines win over priority.
        let urgent = IoRequest { id: 4, lba: 500, blocks: 1, priority: 0, deadline_tick: 5, ..a };
        q.submit(urgent);
        let next = q.peek(0).unwrap();
        assert_eq!(next.id, 4, "the earliest deadline runs first");
        let taken = q.take_next(0).unwrap();
        assert_eq!(taken.id, 4);
        assert_eq!(q.dispatched(), 1);
        assert_eq!(q.expired(), 0);
        q.submit(IoRequest { id: 5, lba: 600, blocks: 1, deadline_tick: 2, ..a });
        let _ = q.take_next(99);
        assert_eq!(q.expired(), 1, "a missed deadline is counted");

        assert!(mergeable(
            &IoRequest { id: 0, device: 0, op: BlockOp::Read, lba: 0, blocks: 4, priority: 0, submit_tick: 0, deadline_tick: 0 },
            &IoRequest { id: 0, device: 0, op: BlockOp::Read, lba: 4, blocks: 4, priority: 0, submit_tick: 0, deadline_tick: 0 }
        ));
        assert!(!mergeable(
            &IoRequest { id: 0, device: 0, op: BlockOp::Read, lba: 0, blocks: 4, priority: 0, submit_tick: 0, deadline_tick: 0 },
            &IoRequest { id: 0, device: 0, op: BlockOp::Read, lba: 9, blocks: 4, priority: 0, submit_tick: 0, deadline_tick: 0 }
        ), "a gap is not adjacent");
    }

    #[test]
    fn cache_is_lru_and_tracks_dirty_writes() {
        let mut c = BlockCache::new();
        assert_eq!(c.lookup(0, 0), None);
        assert_eq!(c.misses(), 1);
        let slot = c.insert(0, 100, 8, false);
        assert_eq!(c.lookup(0, 104), Some(slot), "a hit inside the extent");
        assert_eq!(c.hits(), 1);
        assert_eq!(c.hit_percent(), 50);
        assert_eq!(c.lookup(0, 200), None, "outside the extent is a miss");
        // Dirty entries are counted, and cleaned when written back.
        let d = c.insert(0, 300, 8, true);
        assert_eq!(c.dirty_count(), 1);
        assert!(c.mark_clean(d));
        assert_eq!(c.dirty_count(), 0);

        // Fill past capacity: the LRU entry is evicted, dirty ones counted.
        let mut c2 = BlockCache::new();
        for i in 0..MAX_CACHE_ENTRIES as u64 {
            c2.insert(0, i * 8, 8, false);
        }
        c2.insert(0, 4096, 8, true);
        assert_eq!(c2.evictions(), 1);
        assert!(c2.invalidate_device(0) > 0);
        assert_eq!(c2.lookup(0, 4096), None);
    }

    #[test]
    fn vfs_mounts_resolve_by_longest_prefix() {
        let mut m = MountTable::new();
        assert!(m.mount("/", FsKind::Varix, 0, false, true).is_ok());
        assert!(m.mount("/mnt/data", FsKind::ExFat, 1, false, true).is_ok());
        assert!(m.mount("/mnt/win", FsKind::Ntfs, 2, false, true).is_ok());
        assert_eq!(m.len(), 3);
        assert!(m.rejected() == 0, "the read-only-by-design flag is applied, not rejected");

        // Mounting a read-only filesystem without asking for read-only still
        // yields a read-only mount.
        let win = m.find("/mnt/win").unwrap();
        assert!(m.get(win).unwrap().read_only);
        assert!(!m.get(win).unwrap().writable());
        assert_eq!(m.writable_count(), 2);

        assert_eq!(m.resolve("/etc/hosts").map(|(i, r)| (i, r)), Some((0, "/etc/hosts")));
        assert_eq!(m.resolve("/mnt/data/DCIM/a.jpg").map(|(i, r)| (i, r)), Some((1, "/DCIM/a.jpg")));
        assert_eq!(m.resolve("/mnt").map(|(i, r)| (i, r)), Some((0, "/mnt")));
        assert_eq!(m.resolve("/").map(|(i, r)| (i, r)), Some((0, "/")));

        // Duplicates and bad paths are refused.
        assert_eq!(m.mount("/", FsKind::Ram, 0, false, false).unwrap_err(), "already mounted");
        assert_eq!(m.mount("relative", FsKind::Ram, 0, false, false).unwrap_err(), "mountpoint must be an absolute path");
        assert_eq!(m.mount("/trailing/", FsKind::Ram, 0, false, false).unwrap_err(), "mountpoint must not end with a slash");
        assert!(m.umount("/mnt/data"));
        assert!(!m.umount("/mnt/data"));
        assert_eq!(m.len(), 2);
        assert!(m.render(&mut [0u8; 128]) > 0);
        assert_eq!(FsKind::Ext2.read_only_by_design(), true);
        assert_eq!(FsKind::Ntfs.writable(), false);
        assert!(FsKind::Varix.writable());
    }

    #[test]
    fn fat32_bpb_is_validated_not_just_read() {
        let mut boot = [0u8; 512];
        boot[510] = 0x55;
        boot[511] = 0xAA;
        boot[11..13].copy_from_slice(&512u16.to_le_bytes());
        boot[13] = 8; // 4 KiB clusters
        boot[14..16].copy_from_slice(&32u16.to_le_bytes());
        boot[16] = 2;
        boot[17..19].copy_from_slice(&0u16.to_le_bytes());
        boot[32..36].copy_from_slice(&1_048_576u32.to_le_bytes());
        boot[36..40].copy_from_slice(&2048u32.to_le_bytes());
        boot[44..48].copy_from_slice(&2u32.to_le_bytes());
        let info = Fat32Info::parse(&boot).expect("valid FAT32");
        assert_eq!(info.bytes_per_sector, 512);
        assert_eq!(info.cluster_bytes(), 4096);
        assert_eq!(info.fat_start_lba(), 32);
        assert_eq!(info.data_start_lba(), 32 + 2 * 2048);
        assert_eq!(info.cluster_lba(2), Some(4128));
        assert_eq!(info.cluster_lba(1), None, "clusters start at 2");
        assert_eq!(info.capacity_bytes(), 1_048_576 * 512);
        assert!(info.cluster_count() > 0);
        assert!(Fat32Info::is_eoc(0x0FFF_FFFF));
        assert!(!Fat32Info::is_eoc(0x100));

        // Hostile geometries.
        let mut bad = boot;
        bad[13] = 3; // not a power of two
        assert_eq!(Fat32Info::parse(&bad).unwrap_err(), "cluster size is not a power of two");
        let mut bad = boot;
        bad[17..19].copy_from_slice(&512u16.to_le_bytes());
        assert_eq!(Fat32Info::parse(&bad).unwrap_err(), "this is not a FAT32 volume");
        let mut bad = boot;
        bad[44..48].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(Fat32Info::parse(&bad).unwrap_err(), "root cluster is not a data cluster");
        let mut bad = boot;
        bad[36..40].copy_from_slice(&0u32.to_le_bytes());
        bad[22..24].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(Fat32Info::parse(&bad).unwrap_err(), "FAT is empty");
        let mut bad = boot;
        bad[510] = 0;
        assert_eq!(Fat32Info::parse(&bad).unwrap_err(), "missing boot signature");
    }

    #[test]
    fn exfat_and_ntfs_and_ext2_geometry() {
        let mut ex = [0u8; 512];
        ex[3..11].copy_from_slice(&ExfatInfo::OEM);
        ex[510] = 0x55;
        ex[511] = 0xAA;
        ex[108] = 9; // 512-byte sectors
        ex[109] = 3; // 8 sectors per cluster → 4 KiB
        ex[80..84].copy_from_slice(&24u32.to_le_bytes());
        ex[84..88].copy_from_slice(&1024u32.to_le_bytes());
        ex[88..92].copy_from_slice(&2048u32.to_le_bytes());
        ex[92..96].copy_from_slice(&1_000_000u32.to_le_bytes());
        ex[96..100].copy_from_slice(&4u32.to_le_bytes());
        let info = ExfatInfo::parse(&ex).expect("valid exFAT");
        assert_eq!(info.cluster_bytes(), 4096);
        assert_eq!(info.capacity_bytes(), 4_096_000_000);
        assert!(info.cluster_size_ok_for_size());
        let mut bad = ex;
        bad[108] = 8;
        assert_eq!(ExfatInfo::parse(&bad).unwrap_err(), "invalid sector size shift");
        let mut bad = ex;
        bad[96..100].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(ExfatInfo::parse(&bad).unwrap_err(), "root cluster is not a data cluster");
        let mut bad = ex;
        bad[3] = b'X';
        assert_eq!(ExfatInfo::parse(&bad).unwrap_err(), "not an exFAT volume");

        let mut nt = [0u8; 512];
        nt[3..11].copy_from_slice(&NtfsInfo::OEM);
        nt[510] = 0x55;
        nt[511] = 0xAA;
        nt[11..13].copy_from_slice(&512u16.to_le_bytes());
        nt[13] = 8;
        nt[40..48].copy_from_slice(&2_000_000u64.to_le_bytes());
        nt[48..56].copy_from_slice(&4u64.to_le_bytes());
        nt[56..64].copy_from_slice(&5u64.to_le_bytes());
        nt[64] = (-10i8) as u8; // 1024-byte MFT records
        let info = NtfsInfo::parse(&nt).expect("valid NTFS");
        assert_eq!(info.cluster_bytes(), 4096);
        assert_eq!(info.mft_record_size(), 1024);
        assert_eq!(info.capacity_bytes(), 2_000_000 * 512);
        assert!(!info.writable(), "NTFS is read-only by construction (F136)");
        let mut bad = nt;
        bad[64] = 2i8 as u8;
        assert_eq!(NtfsInfo::parse(&bad).unwrap().mft_record_size(), 8192);

        let mut img = vec![0u8; 4096];
        img[Ext2Info::SUPERBLOCK_OFFSET + 56] = 0x53;
        img[Ext2Info::SUPERBLOCK_OFFSET + 57] = 0xEF;
        img[Ext2Info::SUPERBLOCK_OFFSET] = 100;
        img[Ext2Info::SUPERBLOCK_OFFSET + 4] = 20;
        img[Ext2Info::SUPERBLOCK_OFFSET + 24] = 0; // 1 KiB blocks
        img[Ext2Info::SUPERBLOCK_OFFSET + 32] = 8;
        img[Ext2Info::SUPERBLOCK_OFFSET + 40] = 4;
        let info = Ext2Info::parse(&img).expect("valid ext2");
        assert_eq!(info.block_size, 1024);
        assert_eq!(info.magic, Ext2Info::MAGIC);
        assert_eq!(info.effective_inode_size(), 128, "defaults when absent");
        assert_eq!(info.usable_blocks(), 20);
        assert_eq!(info.capacity_bytes(), 20 * 1024);
        img[Ext2Info::SUPERBLOCK_OFFSET + 56] = 0;
        assert_eq!(Ext2Info::parse(&img).unwrap_err(), "not an ext2 filesystem");
    }

    #[test]
    fn journal_only_replays_committed_intact_records() {
        let mut j = Journal::new();
        assert!(j.is_empty());
        let s1 = j.append(10, b"first").unwrap();
        let _s2 = j.append(11, b"second").unwrap();
        assert_eq!(j.len(), 2);
        assert_eq!(j.commit(s1), 1, "only the first sequence was committed");
        assert_eq!(j.committed_watermark(), s1);

        // Corrupt the committed record: replay must skip and count it.
        j.entries[0].payload[0] ^= 0xFF;
        let mut out = [JournalEntry::default(); 4];
        assert_eq!(j.replay(&mut out), 0);
        assert_eq!(j.corrupt(), 1);

        // A fresh journal replays exactly what was committed, in order.
        let mut j2 = Journal::new();
        let a = j2.append(0, b"a").unwrap();
        let _b = j2.append(1, b"b").unwrap();
        j2.commit(a);
        let mut out2 = [JournalEntry::default(); 4];
        assert_eq!(j2.replay(&mut out2), 1);
        assert_eq!(out2[0].seq, a);
        assert!(out2[0].verify());
        assert_eq!(out2[0].lba, 0);
        assert_eq!(&out2[0].payload[..1], b"a");
        assert_eq!(j2.commits(), 1);
        assert_eq!(j2.replays(), 1);
        // Oversized payloads and a full journal are refused.
        assert!(j2.append(2, &[0u8; JOURNAL_PAYLOAD + 1]).is_none());
        for i in 0..MAX_JOURNAL_ENTRIES {
            let _ = j2.append(i as u64, b"x");
        }
        assert!(j2.append(999, b"y").is_none());
    }

    #[test]
    fn fd_table_handles_open_dup_cloexec_and_limits() {
        let mut fds = FdTable::new(8);
        assert_eq!(fds.limit(), 8);
        let a = fds.open(FdKind::File, 100, 0).unwrap();
        assert_eq!(a, 3, "0/1/2 are reserved for stdio");
        let b = fds.open(FdKind::Dir, 200, FD_CLOEXEC).unwrap();
        assert_eq!(fds.open_count(), 2);
        assert!(fds.get(a).is_some());
        assert!(fds.get(99).is_none());

        // dup shares the file object, so the offset is shared too.
        let d = fds.dup(a).unwrap();
        assert_ne!(d, a);
        assert_eq!(fds.file_of(d).unwrap().node, 100);
        assert!(fds.advance(a, 512));
        assert_eq!(fds.file_of(d).unwrap().offset, 512, "dup shares one file object");
        assert_eq!(fds.open_count(), 3);
        assert_eq!(fds.file_count(), 2, "two descriptors, two open files");

        // Closing one reference keeps the file object alive.
        assert!(fds.close(d));
        assert!(fds.get(a).is_some());
        assert_eq!(fds.file_of(a).unwrap().refs, 1);
        assert!(!fds.close(99));
        assert!(!fds.close(d), "closing twice is refused");

        // CLOEXEC closes exactly one descriptor.
        assert_eq!(fds.close_on_exec(), 1);
        assert!(fds.get(b).is_none());
        assert!(fds.get(a).is_some());

        // The limit is enforced against open descriptors.
        let mut small = FdTable::new(2);
        assert!(small.open(FdKind::File, 1, 0).is_some());
        assert!(small.open(FdKind::File, 2, 0).is_some());
        assert!(small.open(FdKind::File, 3, 0).is_none());
        small.set_limit(1);
        assert_eq!(small.limit(), 1);
    }

    #[test]
    fn paths_are_normalized_without_escaping_the_root() {
        let mut buf = [0u8; PATH_BYTES];
        let cases = [
            ("/", "/"),
            ("/a", "/a"),
            ("/a/b", "/a/b"),
            ("/a/b/", "/a/b"),
            ("/a//b", "/a/b"),
            ("/a/./b", "/a/b"),
            ("/a/b/../c", "/a/c"),
            ("/a/b/..", "/a"),
            ("/../a", ""),
            ("relative", ""),
        ];
        for (input, want) in cases {
            match normalize(input, &mut buf) {
                Some(n) => assert_eq!(core::str::from_utf8(&buf[..n]).unwrap(), want, "{input}"),
                None => assert_eq!(want, "", "{input} should have been refused"),
            }
        }
        assert_eq!(split_path("/a/b/c"), ("/a/b", "c"));
        assert_eq!(split_path("/a"), ("/", "a"));
        assert_eq!(split_path("a"), (".", "a"));
        assert_eq!(extension("/a/b.txt"), Some("txt"));
        assert_eq!(extension("/a/.hidden"), None);
        assert_eq!(extension("/a/b"), None);
    }

    #[test]
    fn locks_allow_readers_deny_conflicting_writers() {
        let mut t = LockTable::new();
        assert!(t.is_empty());
        assert!(t.acquire(1, 10, LockKind::Shared).is_ok());
        assert!(t.acquire(1, 11, LockKind::Shared).is_ok(), "two readers coexist");
        assert_eq!(t.holder(1), Some((10, LockKind::Shared)));

        let denied = t.acquire(1, 12, LockKind::Exclusive).unwrap_err();
        assert_eq!(denied, (10, LockKind::Shared), "the caller learns who holds it");
        assert_eq!(t.denied(), 1);

        // The owner may upgrade.
        assert!(t.acquire(1, 10, LockKind::Exclusive).is_ok());
        // A different owner cannot take an exclusive lock.
        assert!(t.acquire(1, 13, LockKind::Exclusive).is_err());
        assert!(t.release(1, 10));
        assert_eq!(t.holder(1), None);
        assert!(!t.release(1, 99), "only the owner releases");
        assert!(t.acquire(1, 13, LockKind::Exclusive).is_ok());
    }

    #[test]
    fn async_io_and_resumable_transfers() {
        let mut q = AioQueue::new();
        let a = q.submit(1).unwrap();
        let b = q.submit(2).unwrap();
        assert_ne!(a, b);
        assert_eq!(q.in_flight(), 2);
        assert!(q.complete(1, 4096));
        assert!(q.fail(2, "device gone"));
        assert!(!q.complete(99, 0), "unknown id");
        let mut done = [AioSlot::default(); 4];
        assert_eq!(q.polls(&mut done), 2);
        assert_eq!(done.iter().filter(|s| s.state == AioState::Completed).count(), 1);
        assert_eq!(q.in_flight(), 0);
        assert_eq!(q.submitted(), 2);
        assert_eq!(q.completed(), 1);
        // Slots are reusable after being reaped.
        assert!(q.submit(3).is_some());

        let mut rt = ResumeTable::new();
        let idx = rt.begin(0xABCD, 1000).unwrap();
        let _ = idx;
        let mut e = TransferResume::new(0xABCD, 1000);
        e.advance(400);
        e.update_checksum(b"chunk");
        assert_eq!(e.progress_percent(), 40);
        assert!(!e.complete());
        assert_eq!(e.resume_at(), 400);
        e.advance(10_000);
        assert_eq!(e.offset, 1000, "progress is clamped to the total");
        assert!(e.complete());
        assert!(e.checksum != 0);
        assert_eq!(rt.resume(0xABCD), Some(0));
        assert_eq!(rt.get(0xABCD).unwrap().retries, 1);
        assert!(rt.finish(0xABCD));
        assert!(!rt.finish(0xABCD));
        assert!(rt.is_empty());
        assert!(rt.begin(1, 1).is_some());
    }

    #[test]
    fn smart_trim_and_bad_blocks() {
        let healthy = SmartData {
            temperature_c: 35,
            power_on_hours: 1000,
            reallocated_sectors: 0,
            pending_sectors: 0,
            wear_percent: 5,
            power_cycles: 100,
        };
        assert_eq!(healthy.health(), Health::Pass);
        assert!(healthy.endurance_limited());
        let warm = SmartData {
            temperature_c: 60,
            ..healthy
        };
        assert_eq!(warm.health(), Health::Warn);
        let dying = SmartData {
            pending_sectors: 1,
            ..healthy
        };
        assert_eq!(dying.health(), Health::Fail);
        let worn = SmartData {
            wear_percent: 100,
            ..healthy
        };
        assert_eq!(worn.health(), Health::Fail);
        let mut s = SmartData::default();
        assert_eq!(s.health(), Health::Pass, "zeros are not a failure signal");
        assert!(!s.endurance_limited());
        s.temperature_c = 99;
        assert_eq!(s.health().as_str(), "warn");

        let mut tq = TrimQueue::new();
        assert!(tq.add(100, 8));
        assert!(tq.add(108, 8), "an adjacent range coalesces");
        assert_eq!(tq.len(), 1);
        assert!(tq.add(200, 8));
        assert_eq!(tq.len(), 2);
        assert!(!tq.add(300, 0), "empty ranges are ignored");
        assert_eq!(tq.flush(), 2);
        assert_eq!(tq.flushed_batches(), 1);
        assert_eq!(tq.trimmed_blocks(), 24);
        assert!(tq.is_empty());
        assert_eq!(TrimQueue::new().flush(), 0, "an empty batch sends nothing");
        assert!(tq.has_capacity());

        let mut bad = BadBlockMap::new();
        assert!(bad.mark_bad(42, 8000));
        assert!(!bad.mark_bad(42, 8001), "a block is retired once");
        assert!(bad.is_bad(42));
        assert_eq!(bad.resolve(42), 8000);
        assert_eq!(bad.resolve(43), 43);
        assert_eq!(bad.remapped(), 1);
        bad.note_unmapped();
        assert_eq!(bad.unmapped(), 1);
        for i in 0..MAX_BAD_BLOCKS {
            let _ = bad.mark_bad(1000 + i as u64, 9000 + i as u64);
        }
        assert!(bad.spares_exhausted());
    }

    #[test]
    fn images_data_partition_events_and_fragmentation() {
        let mut img = DiskImage::new(1, ImageKind::Differential);
        img.base_blocks = 1000;
        assert!(img.add_extent(100, 50, 5000));
        assert!(img.add_extent(400, 50, 5100));
        assert_eq!(img.extents(), 2);
        assert_eq!(img.resolve(120), (5020, true));
        assert_eq!(img.resolve(700), (700, false));
        assert_eq!(img.reads_from_diff(), 1);
        assert_eq!(img.reads_from_base(), 1);
        assert_eq!(img.coverage_percent(), 10);
        assert!(!img.add_extent(0, 0, 0), "empty extents are refused");

        let data = DataPartition {
            lba_start: 2048,
            lba_end: 4096,
            label: [0; 16],
            label_len: 0,
            fs: FsKind::ExFat,
        }
        .with_label("VARIX-DATA");
        assert_eq!(data.label_str(), "VARIX-DATA");
        assert_eq!(data.blocks(), 2048);
        assert!(data.contains(2048));
        assert!(!data.contains(4096), "the end is exclusive");
        assert!(data.shared_compatible(), "exFAT is what all three systems read");
        let ntfs_area = DataPartition {
            fs: FsKind::Ntfs,
            ..data
        };
        assert!(!ntfs_area.shared_compatible());

        let mut ev = StorageEvents::new();
        for i in 0..(MAX_STORAGE_EVENTS + 3) {
            ev.push(StorageEvent {
                tick: i as u64,
                device: 0,
                kind: Some(StorageEventKind::Error),
                detail: 0,
            });
        }
        assert_eq!(ev.len(), MAX_STORAGE_EVENTS);
        assert_eq!(ev.total(), (MAX_STORAGE_EVENTS + 3) as u64);
        assert_eq!(ev.count(StorageEventKind::Error), MAX_STORAGE_EVENTS as u64);
        assert!(ev.get(MAX_STORAGE_EVENTS).is_none());
        assert!(!StorageEventKind::Trimmed.as_str().is_empty());

        let frag = Fragmentation {
            used_extents: 100,
            used_blocks: 200,
            free_extents: 10,
            free_blocks: 800,
        };
        assert_eq!(frag.contiguity_percent(), 100, "200 ideal in 100 extents is perfect");
        let bad_frag = Fragmentation {
            used_extents: 400,
            used_blocks: 100,
            free_extents: 10,
            free_blocks: 900,
        };
        assert_eq!(bad_frag.contiguity_percent(), 25);
        assert!(bad_frag.advise_defrag());
        assert_eq!(bad_frag.as_str(), "defragment");
        assert_eq!(bad_frag.free_percent(), 90);
        assert!(Fragmentation::default().advise_defrag() == false);
    }

    #[test]
    fn bring_up_reports_a_consistent_state() {
        let mut s = Storage::new();
        let mounted = s.apply_standard_layout();
        assert_eq!(mounted, 4);
        assert_eq!(s.mounts.len(), 4);
        // root + data are writable; the Windows and rescue views are not.
        assert_eq!(s.mounts.writable_count(), 2);
        assert_eq!(s.events.count(StorageEventKind::Inserted), 0);

        let slot = s.attach(ata_device(4096, false)).unwrap();
        assert_eq!(s.events.count(StorageEventKind::Inserted), 1);
        assert!(s.detach(slot));
        assert_eq!(s.events.count(StorageEventKind::Removed), 1);
        assert!(!s.detach(slot), "detaching twice is refused");

        let mut check = Storage::new();
        let (passed, failed) = run_storage_checks(&mut check);
        assert_eq!(failed, 0, "{passed} passed, {failed} failed");
        assert!(passed >= 9);

        let mut st = Storage::new();
        st.apply_standard_layout();
        let state = StorageDomainState {
            devices: st.blocks.len(),
            mounts: st.mounts.len(),
            writable_mounts: st.mounts.writable_count(),
            cache_hit_percent: st.cache.hit_percent(),
            io_merged: st.io.merged(),
            journal_entries: st.journal.len(),
            self_test: run_storage_checks(&mut st),
        };
        assert!(state.ok());
        let mut out = [0u8; 256];
        let n = state.render(&mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.starts_with("storage dev=0 mount=4/2rw"), "got {text}");
        assert!(text.ends_with("]\n"));
        let mut tiny = [0u8; 8];
        assert_eq!(state.render(&mut tiny), 8);
    }
}