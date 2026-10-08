//! AI-13 驱动框架域（F301~F325）。
//!
//! One driver model shared by every bus, a device tree, the hot-plug queue,
//! lifecycle/liveness rules, PCI/PCIe enumeration, DMA ownership (the
//! history lesson from the frame-pool bug), MMIO/port-IO wrappers, the
//! loader, ABI negotiation and the driver self-test.
//!
//! 红线 (F306): a driver fault must never take the kernel down — every
//! entry point returns `Result` and the framework quarantines instead of
//! propagating.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F301 — 驱动模型
// ---------------------------------------------------------------------------

pub type DriverFn = fn(&mut DriverCtx) -> Result<(), i32>;

/// Minimal context handed to every driver entry point.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriverCtx {
    pub device_id: u16,
    pub irq: u8,
    pub bar0: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DriverOps {
    pub probe: Option<DriverFn>,
    pub start: Option<DriverFn>,
    pub stop: Option<DriverFn>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverKind {
    Block,
    Net,
    Audio,
    Input,
    Display,
    Bus,
    Other,
}

#[derive(Clone, Copy, Debug)]
pub struct DriverDesc {
    pub name: &'static str,
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub kind: DriverKind,
    /// Driver-facing ABI revision (F320).
    pub abi: u16,
}

impl DriverDesc {
    /// Does this driver claim the device? `0xFFFF/0xFFFF` = class-only rule.
    pub fn matches(&self, vendor: u16, device: u16, class: u8) -> bool {
        if self.vendor == 0xFFFF && self.device == 0xFFFF {
            return self.class == class;
        }
        self.vendor == vendor && self.device == device
    }
}

// ---------------------------------------------------------------------------
// F302 — 总线抽象
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusKind {
    Pci,
    Usb,
    Virtio,
    Platform,
    Acpi,
}

pub const MAX_BUSES: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct BusEntry {
    pub kind: BusKind,
    pub devices: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct BusRegistry {
    entries: [Option<BusEntry>; MAX_BUSES],
    count: usize,
}

impl BusRegistry {
    pub const fn new() -> BusRegistry {
        BusRegistry { entries: [None; MAX_BUSES], count: 0 }
    }

    pub fn attach(&mut self, entry: BusEntry) -> bool {
        if self.count >= MAX_BUSES {
            return false;
        }
        self.entries[self.count] = Some(entry);
        self.count += 1;
        true
    }

    pub fn get(&self, kind: BusKind) -> Option<BusEntry> {
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.kind == kind {
                    return Some(e);
                }
            }
        }
        None
    }

    pub fn total_devices(&self) -> u32 {
        let mut n = 0u32;
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                n += e.devices;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F303 — 设备树
// ---------------------------------------------------------------------------

pub const MAX_NODES: usize = 24;

#[derive(Clone, Copy, Debug)]
pub struct DeviceNode {
    pub id: u16,
    /// `None` = root.
    pub parent: Option<u16>,
    pub kind: BusKind,
}

#[derive(Clone, Copy, Debug)]
pub struct DeviceTree {
    nodes: [Option<DeviceNode>; MAX_NODES],
    count: usize,
}

impl DeviceTree {
    pub const fn new() -> DeviceTree {
        DeviceTree { nodes: [None; MAX_NODES], count: 0 }
    }

    pub fn add(&mut self, node: DeviceNode) -> bool {
        if self.count >= MAX_NODES {
            return false;
        }
        self.nodes[self.count] = Some(node);
        self.count += 1;
        true
    }

    pub fn find(&self, id: u16) -> Option<DeviceNode> {
        for i in 0..self.count {
            if let Some(n) = self.nodes[i] {
                if n.id == id {
                    return Some(n);
                }
            }
        }
        None
    }

    /// Depth from the root (root = 0); `None` on a malformed parent cycle.
    pub fn depth(&self, id: u16) -> Option<usize> {
        let mut d = 0usize;
        let mut cur = self.find(id)?;
        while let Some(p) = cur.parent {
            d += 1;
            if d > MAX_NODES {
                return None; // cycle guard
            }
            cur = self.find(p)?;
        }
        Some(d)
    }

    pub fn is_ancestor(&self, ancestor: u16, node: u16) -> bool {
        let mut cur = match self.find(node) {
            Some(n) => n,
            None => return false,
        };
        let mut guard = 0usize;
        while let Some(p) = cur.parent {
            if p == ancestor {
                return true;
            }
            guard += 1;
            if guard > MAX_NODES {
                return false;
            }
            cur = match self.find(p) {
                Some(n) => n,
                None => return false,
            };
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F304 — 热插拔框架
// ---------------------------------------------------------------------------

pub const MAX_HOTPLUG_EVENTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotplugAction {
    Insert,
    Remove,
    Change,
}

#[derive(Clone, Copy, Debug)]
pub struct HotplugEvent {
    pub action: HotplugAction,
    pub device_id: u16,
    pub bus: BusKind,
}

#[derive(Clone, Copy, Debug)]
pub struct HotplugQueue {
    events: [Option<HotplugEvent>; MAX_HOTPLUG_EVENTS],
    head: usize,
    count: usize,
    subscribers: u32,
}

impl HotplugQueue {
    pub const fn new() -> HotplugQueue {
        HotplugQueue {
            events: [None; MAX_HOTPLUG_EVENTS],
            head: 0,
            count: 0,
            subscribers: 0,
        }
    }

    pub fn subscribe(&mut self) -> bool {
        if self.subscribers >= 32 {
            return false;
        }
        self.subscribers += 1;
        true
    }

    /// Never blocks: a full queue reports failure so the USB poller can
    /// coalesce instead of stalling inside an interrupt.
    pub fn push(&mut self, event: HotplugEvent) -> bool {
        if self.count >= MAX_HOTPLUG_EVENTS {
            return false;
        }
        self.events[self.head] = Some(event);
        self.head = (self.head + 1) % MAX_HOTPLUG_EVENTS;
        self.count += 1;
        true
    }

    pub fn pop(&mut self) -> Option<HotplugEvent> {
        if self.count == 0 {
            return None;
        }
        let pos = (self.head + MAX_HOTPLUG_EVENTS - self.count) % MAX_HOTPLUG_EVENTS;
        let out = self.events[pos];
        self.events[pos] = None;
        self.count -= 1;
        out
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F305 — 驱动生命周期
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverState {
    Registered,
    Probed,
    Bound,
    Running,
    Suspending,
    Stopped,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverEvent {
    Probe,
    Bind,
    Start,
    Suspend,
    Resume,
    Stop,
    Fault,
    Unbind,
}

/// Legal transition table — `None` = the event is illegal in this state.
pub fn driver_transition(state: DriverState, event: DriverEvent) -> Option<DriverState> {
    use DriverEvent::*;
    use DriverState::*;
    match (state, event) {
        (Registered, Probe) => Some(Probed),
        (Registered, Bind) => Some(Bound),
        (Probed, Bind) => Some(Bound),
        (Bound, Start) => Some(Running),
        (Running, Suspend) => Some(Suspending),
        (Suspending, Resume) => Some(Running),
        (Suspending, Stop) => Some(Stopped),
        (Running, Stop) => Some(Stopped),
        (Stopped, Start) => Some(Running),
        (Stopped, Unbind) | (Bound, Unbind) | (Probed, Unbind) => Some(Registered),
        // A fault is always accepted and always quarantines (F306).
        (_, Fault) => Some(Failed),
        (Failed, Unbind) => Some(Registered),
        _ => None,
    }
}

pub fn is_live(state: DriverState) -> bool {
    matches!(state, DriverState::Running)
}

// ---------------------------------------------------------------------------
// F306 — 驱动隔离（红线）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultPolicy {
    /// Faults allowed inside the rolling window before quarantine.
    pub max_faults: u32,
    pub window_ms: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverHealth {
    pub faults: u32,
    pub quarantined: bool,
    pub last_fault_ms: u32,
}

impl DriverHealth {
    pub const fn new() -> DriverHealth {
        DriverHealth { faults: 0, quarantined: false, last_fault_ms: 0 }
    }

    /// Record a fault. Returns `true` when the driver is now quarantined —
    /// the caller skips it and the rest of the system keeps running.
    pub fn record_fault(&mut self, now_ms: u32, policy: FaultPolicy) -> bool {
        let expired = now_ms.saturating_sub(self.last_fault_ms) > policy.window_ms;
        if expired {
            self.faults = 0;
        }
        self.faults += 1;
        self.last_fault_ms = now_ms;
        if self.faults >= policy.max_faults {
            self.quarantined = true;
        }
        self.quarantined
    }

    pub fn clear(&mut self) {
        self.faults = 0;
        self.quarantined = false;
    }
}

// ---------------------------------------------------------------------------
// F307 — 兼容档案库
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quirk {
    None,
    NoMsi,
    DelayAfterReset(u32),
    ForcePolling,
    SuppressAspm,
}

#[derive(Clone, Copy, Debug)]
pub struct CompatEntry {
    pub vendor: u16,
    pub device: u16,
    pub driver: &'static str,
    pub quirk: Quirk,
    /// 0..100 — higher wins when several entries match.
    pub confidence: u8,
}

pub const MAX_COMPAT: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct CompatDb {
    entries: [Option<CompatEntry>; MAX_COMPAT],
    count: usize,
}

impl CompatDb {
    pub const fn new() -> CompatDb {
        CompatDb { entries: [None; MAX_COMPAT], count: 0 }
    }

    pub fn add(&mut self, entry: CompatEntry) -> bool {
        if self.count >= MAX_COMPAT {
            return false;
        }
        self.entries[self.count] = Some(entry);
        self.count += 1;
        true
    }

    /// Best-matching entry for a device (highest confidence wins).
    pub fn lookup(&self, vendor: u16, device: u16) -> Option<CompatEntry> {
        let mut best: Option<CompatEntry> = None;
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.vendor == vendor && e.device == device {
                    let better = match best {
                        Some(b) => e.confidence > b.confidence,
                        None => true,
                    };
                    if better {
                        best = Some(e);
                    }
                }
            }
        }
        best
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F308 — 双轨测试（QEMU / 真机）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackResult {
    pub qemu: Option<bool>,
    pub real: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DualTrackVerdict {
    /// Both tracks ran and passed.
    Verified,
    /// QEMU passed, real hardware not available yet (W1/W2 阶段正常).
    QemuOnly,
    Failed,
    NotRun,
}

pub fn dual_track_verdict(r: TrackResult) -> DualTrackVerdict {
    match (r.qemu, r.real) {
        (Some(false), _) | (_, Some(false)) => DualTrackVerdict::Failed,
        (Some(true), Some(true)) => DualTrackVerdict::Verified,
        (Some(true), None) => DualTrackVerdict::QemuOnly,
        _ => DualTrackVerdict::NotRun,
    }
}

// ---------------------------------------------------------------------------
// F309 — 驱动签名
// ---------------------------------------------------------------------------

/// FNV-1a 64 — the kernel's cheap integrity digest for driver payloads.
pub fn hash64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Constant-time compare (the F235 discipline applied to driver loading).
pub fn constant_time_eq(a: &[u8; 8], b: &[u8; 8]) -> bool {
    let mut diff = 0u8;
    for i in 0..8 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureVerdict {
    Trusted,
    Unsigned,
    Mismatch,
}

/// Verify a driver payload against its signature digest.
pub fn verify_driver(payload: &[u8], sig: Option<[u8; 8]>, allow_unsigned: bool) -> SignatureVerdict {
    let digest = hash64(payload).to_le_bytes();
    match sig {
        Some(s) => {
            if constant_time_eq(&s, &digest) {
                SignatureVerdict::Trusted
            } else {
                SignatureVerdict::Mismatch
            }
        }
        None => {
            if allow_unsigned {
                SignatureVerdict::Trusted
            } else {
                SignatureVerdict::Unsigned
            }
        }
    }
}

// ---------------------------------------------------------------------------
// F310/F311 — 驱动版本管理与回滚
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DriverVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl DriverVersion {
    pub const fn new(major: u16, minor: u16, patch: u16) -> DriverVersion {
        DriverVersion { major, minor, patch }
    }

    /// Parse "1.2.3" — a trailing suffix (e.g. "-rc1") is ignored.
    pub fn parse(text: &str) -> Option<DriverVersion> {
        let mut it = text.split('.');
        let major = parse_u16(it.next()?)?;
        let minor = parse_u16(it.next().unwrap_or("0"))?;
        let patch_txt = it.next().unwrap_or("0");
        let end = patch_txt
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(patch_txt.len());
        let patch = parse_u16(&patch_txt[..end])?;
        Some(DriverVersion { major, minor, patch })
    }

    /// Same major version == API-compatible.
    pub fn compatible_with(&self, other: DriverVersion) -> bool {
        self.major == other.major
    }
}

fn parse_u16(text: &str) -> Option<u16> {
    if text.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    for c in text.bytes() {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (c - b'0') as u32;
        if v > u16::MAX as u32 {
            return None;
        }
    }
    Some(v as u16)
}

#[derive(Clone, Copy, Debug)]
pub struct RollbackSlot {
    pub current: DriverVersion,
    pub previous: Option<DriverVersion>,
}

impl RollbackSlot {
    pub const fn new(current: DriverVersion) -> RollbackSlot {
        RollbackSlot { current, previous: None }
    }

    pub fn install(&mut self, next: DriverVersion) {
        self.previous = Some(self.current);
        self.current = next;
    }

    /// One-key rollback to the previous version (可回滚军规).
    pub fn rollback(&mut self) -> Option<DriverVersion> {
        let prev = self.previous?;
        self.previous = Some(self.current);
        self.current = prev;
        Some(prev)
    }
}

// ---------------------------------------------------------------------------
// F312 — PCI 枚举
// ---------------------------------------------------------------------------

/// Build the 0xCF8 configuration address for (bus, device, function, reg).
pub fn pci_config_address(bus: u8, device: u8, function: u8, reg: u8) -> u32 {
    (1 << 31)
        | ((bus as u32) << 16)
        | (((device as u32) & 0x1F) << 11)
        | (((function as u32) & 0x7) << 8)
        | ((reg as u32) & 0xFC)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciDevice {
    pub vendor: u16,
    pub device: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub header_type: u8,
}

impl PciDevice {
    pub fn class(&self) -> u16 {
        ((self.class_code as u16) << 8) | self.subclass as u16
    }

    pub fn is_multifunction(&self) -> bool {
        self.header_type & 0x80 != 0
    }
}

/// Parse the head of PCI config space.
pub fn parse_pci_device(config: &[u8]) -> Option<PciDevice> {
    if config.len() < 16 {
        return None;
    }
    let vendor = u16::from_le_bytes([config[0], config[1]]);
    if vendor == 0xFFFF {
        return None; // no device in this slot
    }
    Some(PciDevice {
        vendor,
        device: u16::from_le_bytes([config[2], config[3]]),
        class_code: config[11],
        subclass: config[10],
        header_type: config[14],
    })
}

/// Decode a BAR size from its mask (writable bits set to 1).
/// PCI BARs are aligned to at least 16 bytes, so the smallest reported size
/// is 16 — a mask of all ones means "unimplemented" and yields `None`.
pub fn bar_size(mask: u32) -> Option<u64> {
    if mask == 0 || mask == 0xFFFF_FFFF {
        return None;
    }
    let size = (!(mask & !0xF)) as u64 + 1;
    if size <= 16 {
        Some(16)
    } else {
        Some(size)
    }
}

// ---------------------------------------------------------------------------
// F313 — PCIe 能力
// ---------------------------------------------------------------------------

/// Walk the PCI capability list for `cap_id`, returning the config offset.
pub fn find_capability(config: &[u8], cap_id: u8) -> Option<usize> {
    if config.len() < 0x40 {
        return None;
    }
    let status = u16::from_le_bytes([config[6], config[7]]);
    if status & (1 << 4) == 0 {
        return None; // capability list not supported
    }
    let mut ptr = (config[0x34] & 0xFC) as usize;
    let mut guard = 0usize;
    while ptr + 2 <= config.len() && ptr >= 0x40 && guard < 48 {
        if config[ptr] == cap_id {
            return Some(ptr);
        }
        if config[ptr] == 0 {
            return None;
        }
        ptr = (config[ptr + 1] & 0xFC) as usize;
        guard += 1;
    }
    None
}

/// PCIe link speed from the Link Capabilities register (bits 0..3).
pub fn pcie_link_speed(link_cap: u32) -> &'static str {
    match link_cap & 0xF {
        1 => "2.5GT/s",
        2 => "5.0GT/s",
        3 => "8.0GT/s",
        4 => "16.0GT/s",
        5 => "32.0GT/s",
        _ => "unknown",
    }
}

/// Negotiated link width from Link Status (bits 4..9).
pub fn pcie_link_width(link_status: u16) -> u8 {
    ((link_status >> 4) & 0x3F) as u8
}

// ---------------------------------------------------------------------------
// F314 — DMA 框架
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaOwner {
    Free,
    Cpu,
    Device,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaBuffer {
    pub phys: u64,
    pub len: usize,
    pub owner: DmaOwner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaError {
    NotCpuOwned,
    NotDeviceOwned,
    AlreadyFree,
    Overflow,
}

impl DmaBuffer {
    pub fn new(phys: u64, len: usize) -> DmaBuffer {
        DmaBuffer { phys, len, owner: DmaOwner::Free }
    }

    /// Hand the buffer to the device (the CPU must own it first).
    pub fn submit(&mut self, len: usize) -> Result<(), DmaError> {
        if self.owner != DmaOwner::Cpu {
            return Err(DmaError::NotCpuOwned);
        }
        if len > self.len {
            return Err(DmaError::Overflow);
        }
        self.owner = DmaOwner::Device;
        Ok(())
    }

    /// Reclaim ownership after the completion interrupt.
    pub fn complete(&mut self) -> Result<(), DmaError> {
        if self.owner != DmaOwner::Device {
            return Err(DmaError::NotDeviceOwned);
        }
        self.owner = DmaOwner::Cpu;
        Ok(())
    }

    pub fn free(&mut self) -> Result<(), DmaError> {
        if self.owner == DmaOwner::Free {
            return Err(DmaError::AlreadyFree);
        }
        self.owner = DmaOwner::Free;
        Ok(())
    }
}

/// 32-bit-only devices need a bounce buffer for memory above the mask.
pub fn needs_bounce(phys: u64, dma_mask: u64) -> bool {
    dma_mask != 0 && (phys | phys.saturating_add(4095)) > dma_mask
}

// ---------------------------------------------------------------------------
// F315 — IRQ 共享
// ---------------------------------------------------------------------------

pub const MAX_SHARED_HANDLERS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct SharedIrq {
    pub line: u8,
    handlers: [Option<(&'static str, fn() -> bool)>; MAX_SHARED_HANDLERS],
    count: usize,
}

impl SharedIrq {
    pub const fn new(line: u8) -> SharedIrq {
        SharedIrq { line, handlers: [None; MAX_SHARED_HANDLERS], count: 0 }
    }

    pub fn add_handler(&mut self, name: &'static str, handler: fn() -> bool) -> bool {
        if self.count >= MAX_SHARED_HANDLERS {
            return false;
        }
        self.handlers[self.count] = Some((name, handler));
        self.count += 1;
        true
    }

    /// Dispatch to every handler; returns how many claimed the interrupt.
    /// A shared line must be polled in full — stopping at the first handler
    /// would lose the other devices' events.
    pub fn dispatch(&self) -> usize {
        let mut claimed = 0usize;
        for i in 0..self.count {
            if let Some((_, h)) = self.handlers[i] {
                if h() {
                    claimed += 1;
                }
            }
        }
        claimed
    }

    pub fn handler_count(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F316 — MMIO 封装
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MmioRegion {
    pub base: u64,
    pub len: usize,
}

impl MmioRegion {
    pub const fn new(base: u64, len: usize) -> MmioRegion {
        MmioRegion { base, len }
    }

    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.base + self.len as u64
    }

    /// Bounds-checked view of the region over a backing slice.
    pub fn view<'a>(&self, backing: &'a [u8], offset: usize, len: usize) -> Option<&'a [u8]> {
        if offset.checked_add(len)? > self.len {
            return None;
        }
        backing.get(offset..offset + len)
    }

    /// 32-bit little-endian register read.
    pub fn read_u32(backing: &[u8], offset: usize) -> Option<u32> {
        let b = backing.get(offset..offset + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn write_u32(backing: &mut [u8], offset: usize, value: u32) -> bool {
        if offset + 4 > backing.len() {
            return false;
        }
        backing[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        true
    }
}

// ---------------------------------------------------------------------------
// F317 — 端口 IO 封装
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortRange {
    pub base: u16,
    pub len: u16,
}

impl PortRange {
    pub const fn new(base: u16, len: u16) -> PortRange {
        PortRange { base, len }
    }

    pub fn contains(&self, port: u16) -> bool {
        port >= self.base && (port as u32) < self.base as u32 + self.len as u32
    }
}

pub const MAX_PORT_RANGES: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct PortSpace {
    ranges: [Option<PortRange>; MAX_PORT_RANGES],
    count: usize,
}

impl PortSpace {
    pub const fn new() -> PortSpace {
        PortSpace { ranges: [None; MAX_PORT_RANGES], count: 0 }
    }

    pub fn allow(&mut self, range: PortRange) -> bool {
        if self.count >= MAX_PORT_RANGES {
            return false;
        }
        self.ranges[self.count] = Some(range);
        self.count += 1;
        true
    }

    pub fn permitted(&self, port: u16) -> bool {
        (0..self.count).any(|i| self.ranges[i].map(|r| r.contains(port)).unwrap_or(false))
    }
}

// ---------------------------------------------------------------------------
// F318 — 固件接口避让
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReservedRange {
    pub start: u64,
    pub len: u64,
    pub reason: &'static str,
}

pub const MAX_RESERVED: usize = 12;

#[derive(Clone, Copy, Debug)]
pub struct ReservedMap {
    ranges: [Option<ReservedRange>; MAX_RESERVED],
    count: usize,
}

impl ReservedMap {
    pub const fn new() -> ReservedMap {
        ReservedMap { ranges: [None; MAX_RESERVED], count: 0 }
    }

    pub fn add(&mut self, range: ReservedRange) -> bool {
        if self.count >= MAX_RESERVED {
            return false;
        }
        self.ranges[self.count] = Some(range);
        self.count += 1;
        true
    }

    pub fn overlaps(&self, start: u64, len: u64) -> Option<ReservedRange> {
        let end = start.saturating_add(len);
        for i in 0..self.count {
            if let Some(r) = self.ranges[i] {
                let r_end = r.start.saturating_add(r.len);
                if start < r_end && r.start < end {
                    return Some(r);
                }
            }
        }
        None
    }

    /// True when a mapping request is safe (SMM avoidance, F318).
    pub fn mapping_allowed(&self, start: u64, len: u64) -> bool {
        self.overlaps(start, len).is_none()
    }
}

// ---------------------------------------------------------------------------
// F319 — 驱动加载器
// ---------------------------------------------------------------------------

pub const MAX_LOAD_STEPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    TooMany,
    DependencyMissing,
    Cycle,
}

/// Resolve a load order with dependency ordering.
/// `deps[i]` lists the driver indices `i` depends on (loaded first).
pub fn resolve_load_order(deps: &[&[usize]], out: &mut [usize]) -> Result<usize, LoadError> {
    let n = deps.len();
    if n > out.len() || n > MAX_LOAD_STEPS {
        return Err(LoadError::TooMany);
    }
    let mut emitted = [false; MAX_LOAD_STEPS];
    let mut count = 0usize;
    while count < n {
        let mut progressed = false;
        for i in 0..n {
            if emitted[i] {
                continue;
            }
            if deps[i].iter().any(|d| *d >= n) {
                return Err(LoadError::DependencyMissing);
            }
            let ready = deps[i].iter().all(|d| emitted[*d]);
            if ready {
                emitted[i] = true;
                out[count] = i;
                count += 1;
                progressed = true;
            }
        }
        if !progressed {
            return Err(LoadError::Cycle);
        }
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// F320 — 驱动 API 稳定承诺
// ---------------------------------------------------------------------------

/// Negotiate the highest ABI both sides support.
pub fn negotiate_abi(
    kernel_min: u16,
    kernel_max: u16,
    driver_min: u16,
    driver_max: u16,
) -> Option<u16> {
    let lo = kernel_min.max(driver_min);
    let hi = kernel_max.min(driver_max);
    if lo <= hi {
        Some(hi)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// F321 — 驱动文档规范
// ---------------------------------------------------------------------------

pub const REQUIRED_SECTIONS: [&'static str; 5] =
    ["Model", "Registers", "Interrupts", "Power", "Testing"];

/// Lint a driver doc for the mandatory sections.
/// Returns `(present, missing_count)`; missing names land in `missing`.
pub fn doc_lint(text: &str, missing: &mut [Option<&'static str>]) -> (usize, usize) {
    let mut found = 0usize;
    let mut miss = 0usize;
    for section in REQUIRED_SECTIONS.iter() {
        if text.contains(section) {
            found += 1;
        } else if miss < missing.len() {
            missing[miss] = Some(*section);
            miss += 1;
        }
    }
    (found, miss)
}

// ---------------------------------------------------------------------------
// F322 — 驱动性能预算
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverBudget {
    pub probe_us_max: u32,
    /// Longest ISR a driver may run without breaking the latency red line.
    pub isr_us_max: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetVerdict {
    Within,
    ProbeOver,
    IsrOver,
    BothOver,
}

pub fn budget_verdict(probe_us: u32, isr_us: u32, budget: DriverBudget) -> BudgetVerdict {
    let p = probe_us > budget.probe_us_max;
    let i = isr_us > budget.isr_us_max;
    match (p, i) {
        (false, false) => BudgetVerdict::Within,
        (true, false) => BudgetVerdict::ProbeOver,
        (false, true) => BudgetVerdict::IsrOver,
        (true, true) => BudgetVerdict::BothOver,
    }
}

// ---------------------------------------------------------------------------
// F323 — 驱动事件日志
// ---------------------------------------------------------------------------

const DRIVER_EVENT_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverEventKind {
    Load,
    Unload,
    Bind,
    Fault,
    Quarantine,
    Rollback,
    RejectSignature,
    Hotplug,
}

#[derive(Clone, Copy, Debug)]
pub struct DriverEventRecord {
    pub kind: DriverEventKind,
    pub driver: &'static str,
    pub arg: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct DriverEventLog {
    events: [DriverEventRecord; DRIVER_EVENT_CAP],
    head: usize,
    count: usize,
}

impl DriverEventLog {
    pub const fn new() -> DriverEventLog {
        DriverEventLog {
            events: [DriverEventRecord {
                kind: DriverEventKind::Load,
                driver: "",
                arg: 0,
            }; DRIVER_EVENT_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, record: DriverEventRecord) {
        self.events[self.head] = record;
        self.head = (self.head + 1) % DRIVER_EVENT_CAP;
        if self.count < DRIVER_EVENT_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, index: usize) -> Option<DriverEventRecord> {
        if index >= self.count {
            return None;
        }
        let pos = (self.head + DRIVER_EVENT_CAP - 1 - index) % DRIVER_EVENT_CAP;
        Some(self.events[pos])
    }

    pub fn count_of(&self, kind: DriverEventKind) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|e| e.kind == kind).unwrap_or(false)).count()
    }
}

// ---------------------------------------------------------------------------
// F324 — 第三方驱动 SDK
// ---------------------------------------------------------------------------

pub const MAX_ENTRY_POINTS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdkManifest {
    pub abi: u16,
    pub license: &'static str,
    pub entry_points: [&'static str; MAX_ENTRY_POINTS],
    pub entry_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestVerdict {
    Ok,
    MissingProbe,
    NoEntryPoints,
}

/// The SDK contract: every third-party driver must expose `probe`, `start`
/// and `stop`, and declare a licence.
pub fn manifest_verdict(manifest: SdkManifest) -> ManifestVerdict {
    if manifest.entry_count == 0 {
        return ManifestVerdict::NoEntryPoints;
    }
    let mut has_probe = false;
    let mut has_start = false;
    let mut has_stop = false;
    for i in 0..manifest.entry_count.min(MAX_ENTRY_POINTS) {
        match manifest.entry_points[i] {
            "probe" => has_probe = true,
            "start" => has_start = true,
            "stop" => has_stop = true,
            _ => {}
        }
    }
    if !(has_probe && has_start && has_stop) {
        return ManifestVerdict::MissingProbe;
    }
    if manifest.license.is_empty() {
        return ManifestVerdict::MissingProbe;
    }
    ManifestVerdict::Ok
}

// ---------------------------------------------------------------------------
// F325 — 驱动自检
// ---------------------------------------------------------------------------

pub fn run_driver_checks() -> CheckSet {
    let mut set = CheckSet::new("driver");

    let desc = DriverDesc {
        name: "ahci",
        vendor: 0x8086,
        device: 0x2922,
        class: 0x01,
        kind: DriverKind::Block,
        abi: 3,
    };
    set.add(
        "F301 driver model",
        desc.matches(0x8086, 0x2922, 0x01)
            && !desc.matches(0x8086, 0x1111, 0x01)
            && DriverDesc { vendor: 0xFFFF, device: 0xFFFF, ..desc }.matches(0, 0, 0x01),
        "match rules",
    );

    let mut buses = BusRegistry::new();
    buses.attach(BusEntry { kind: BusKind::Pci, devices: 12 });
    buses.attach(BusEntry { kind: BusKind::Usb, devices: 3 });
    set.add(
        "F302 bus registry",
        buses.get(BusKind::Pci).map(|b| b.devices) == Some(12)
            && buses.get(BusKind::Acpi).is_none()
            && buses.total_devices() == 15,
        "bus attach",
    );

    let mut tree = DeviceTree::new();
    tree.add(DeviceNode { id: 0, parent: None, kind: BusKind::Pci });
    tree.add(DeviceNode { id: 1, parent: Some(0), kind: BusKind::Pci });
    tree.add(DeviceNode { id: 2, parent: Some(1), kind: BusKind::Usb });
    set.add(
        "F303 device tree",
        tree.depth(0) == Some(0)
            && tree.depth(2) == Some(2)
            && tree.is_ancestor(0, 2)
            && !tree.is_ancestor(2, 1)
            && tree.depth(99).is_none(),
        "tree walk",
    );

    let mut queue = HotplugQueue::new();
    queue.subscribe();
    let pushed = queue.push(HotplugEvent {
        action: HotplugAction::Insert,
        device_id: 5,
        bus: BusKind::Usb,
    });
    let popped = queue.pop();
    set.add(
        "F304 hotplug",
        pushed && popped.map(|e| e.device_id) == Some(5) && queue.len() == 0 && queue.pop().is_none(),
        "fifo",
    );

    let mut state = DriverState::Registered;
    for event in [DriverEvent::Probe, DriverEvent::Bind, DriverEvent::Start] {
        state = driver_transition(state, event).unwrap_or(state);
    }
    let suspended = driver_transition(state, DriverEvent::Suspend);
    set.add(
        "F305 lifecycle",
        is_live(state)
            && suspended == Some(DriverState::Suspending)
            && driver_transition(DriverState::Registered, DriverEvent::Start).is_none()
            && driver_transition(state, DriverEvent::Fault) == Some(DriverState::Failed),
        "state machine",
    );

    let mut health = DriverHealth::new();
    let policy = FaultPolicy { max_faults: 3, window_ms: 1000 };
    let _ = health.record_fault(10, policy);
    let _ = health.record_fault(20, policy);
    let quarantined = health.record_fault(30, policy);
    set.add(
        "F306 isolation",
        quarantined && health.quarantined && health.faults == 3,
        "quarantine",
    );
    health.clear();
    let _ = health.record_fault(5000, policy);
    set.add("F306 window reset", !health.quarantined && health.faults == 1, "window expiry");

    let mut db = CompatDb::new();
    db.add(CompatEntry {
        vendor: 0x8086,
        device: 0x2922,
        driver: "ahci",
        quirk: Quirk::None,
        confidence: 90,
    });
    db.add(CompatEntry {
        vendor: 0x8086,
        device: 0x2922,
        driver: "ahci-quirk",
        quirk: Quirk::SuppressAspm,
        confidence: 70,
    });
    set.add(
        "F307 compat db",
        db.lookup(0x8086, 0x2922).map(|e| e.driver) == Some("ahci")
            && db.lookup(1, 2).is_none()
            && db.len() == 2,
        "best match",
    );

    set.add(
        "F308 dual track",
        dual_track_verdict(TrackResult { qemu: Some(true), real: Some(true) })
            == DualTrackVerdict::Verified
            && dual_track_verdict(TrackResult { qemu: Some(true), real: None })
                == DualTrackVerdict::QemuOnly
            && dual_track_verdict(TrackResult { qemu: Some(true), real: Some(false) })
                == DualTrackVerdict::Failed
            && dual_track_verdict(TrackResult { qemu: None, real: None })
                == DualTrackVerdict::NotRun,
        "verdict",
    );

    let payload = b"varix-ahci-driver-image";
    let digest = hash64(payload).to_le_bytes();
    set.add(
        "F309 signature",
        verify_driver(payload, Some(digest), false) == SignatureVerdict::Trusted
            && verify_driver(b"tampered", Some(digest), false) == SignatureVerdict::Mismatch
            && verify_driver(payload, None, false) == SignatureVerdict::Unsigned
            && verify_driver(payload, None, true) == SignatureVerdict::Trusted,
        "verify",
    );

    let v = DriverVersion::parse("2.13.7").expect("version");
    set.add(
        "F310 version",
        v == DriverVersion::new(2, 13, 7)
            && DriverVersion::parse("1.2").is_some()
            && DriverVersion::parse("x.y").is_none()
            && DriverVersion::new(2, 0, 0) > DriverVersion::new(1, 9, 9)
            && v.compatible_with(DriverVersion::new(2, 1, 0)),
        "parse + compare",
    );

    let mut slot = RollbackSlot::new(DriverVersion::new(1, 0, 0));
    slot.install(DriverVersion::new(2, 0, 0));
    let rolled = slot.rollback();
    set.add(
        "F311 rollback",
        rolled == Some(DriverVersion::new(1, 0, 0))
            && slot.current == DriverVersion::new(1, 0, 0)
            && RollbackSlot::new(DriverVersion::new(1, 0, 0)).rollback().is_none(),
        "rollback",
    );

    let mut cfg = [0u8; 64];
    cfg[0..2].copy_from_slice(&0x8086u16.to_le_bytes());
    cfg[2..4].copy_from_slice(&0x2922u16.to_le_bytes());
    cfg[10] = 0x01;
    cfg[11] = 0x01;
    cfg[14] = 0x80;
    let pci = parse_pci_device(&cfg).expect("pci");
    set.add(
        "F312 pci enum",
        pci.vendor == 0x8086
            && pci.device == 0x2922
            && pci.class() == 0x0101
            && pci.is_multifunction()
            && pci_config_address(0, 1, 0, 0x10) == 0x8000_0810
            && bar_size(0xFFFF_F000) == Some(4096)
            && bar_size(0).is_none(),
        "config + bars",
    );
    set.add("F312 empty slot", parse_pci_device(&[0xFF; 64]).is_none(), "0xFFFF vendor");

    let mut pcie = [0u8; 68];
    pcie[6] = 0x10; // status: capability list present
    pcie[0x34] = 0x40;
    pcie[0x40] = 0x10; // PCIe capability id
    set.add(
        "F313 pcie",
        find_capability(&pcie, 0x10) == Some(0x40)
            && find_capability(&pcie, 0x05).is_none()
            && pcie_link_speed(3) == "8.0GT/s"
            && pcie_link_width(0x0040) == 4,
        "capability walk",
    );

    let mut dma = DmaBuffer::new(0x1000_0000, 4096);
    dma.owner = DmaOwner::Cpu;
    let submitted = dma.submit(4096).is_ok();
    let refused = dma.submit(4096).is_err();
    let completed = dma.complete().is_ok();
    set.add(
        "F314 dma ownership",
        submitted
            && refused
            && completed
            && dma.free().is_ok()
            && dma.free().is_err()
            && needs_bounce(0x1_0000_0000, 0xFFFF_FFFF)
            && !needs_bounce(0x1000, 0xFFFF_FFFF),
        "ownership",
    );

    fn claim() -> bool {
        true
    }
    fn ignore() -> bool {
        false
    }
    let mut irq = SharedIrq::new(16);
    irq.add_handler("ahci", claim);
    irq.add_handler("nvme", ignore);
    set.add(
        "F315 shared irq",
        irq.dispatch() == 1 && irq.handler_count() == 2 && SharedIrq::new(3).dispatch() == 0,
        "dispatch all",
    );

    let region = MmioRegion::new(0xFEB0_0000, 0x1000);
    let mut backing = [0u8; 0x1000];
    set.add(
        "F316 mmio",
        region.contains(0xFEB0_0000)
            && !region.contains(0xFEC0_0000)
            && region.view(&backing, 0, 16).is_some()
            && region.view(&backing, 0xFF8, 16).is_none()
            && MmioRegion::write_u32(&mut backing, 4, 0xDEAD_BEEF)
            && MmioRegion::read_u32(&backing, 4) == Some(0xDEAD_BEEF)
            && !MmioRegion::write_u32(&mut backing, 0x1000, 1),
        "bounds",
    );

    let mut ports = PortSpace::new();
    ports.allow(PortRange::new(0x1F0, 8));
    set.add(
        "F317 port io",
        ports.permitted(0x1F0) && ports.permitted(0x1F7) && !ports.permitted(0x1F8)
            && !ports.permitted(0x3F8),
        "allowlist",
    );

    let mut reserved = ReservedMap::new();
    reserved.add(ReservedRange { start: 0xE000_0000, len: 0x1000, reason: "SMM" });
    set.add(
        "F318 firmware avoidance",
        !reserved.mapping_allowed(0xE000_0000, 0x100)
            && reserved.mapping_allowed(0xE000_2000, 0x100)
            && reserved.overlaps(0xDFFF_F000, 0x2000).is_some(),
        "reserved map",
    );

    let mut order = [0usize; MAX_LOAD_STEPS];
    let resolved = resolve_load_order(&[&[], &[0], &[1]], &mut order);
    let cyclic = resolve_load_order(&[&[1], &[0]], &mut order);
    let missing = resolve_load_order(&[&[9]], &mut order);
    set.add(
        "F319 loader",
        resolved == Ok(3)
            && order[0] == 0
            && order[2] == 2
            && cyclic == Err(LoadError::Cycle)
            && missing == Err(LoadError::DependencyMissing),
        "topological",
    );

    set.add(
        "F320 abi negotiate",
        negotiate_abi(1, 3, 2, 5) == Some(3)
            && negotiate_abi(3, 4, 1, 2).is_none()
            && negotiate_abi(1, 1, 1, 1) == Some(1),
        "negotiation",
    );

    let good_doc = "## Model\n## Registers\n## Interrupts\n## Power\n## Testing\n";
    let bad_doc = "## Model\n";
    let mut missing_sections = [None; 5];
    let (found_ok, miss_ok) = doc_lint(good_doc, &mut missing_sections);
    let (found_bad, miss_bad) = doc_lint(bad_doc, &mut missing_sections);
    set.add(
        "F321 doc lint",
        found_ok == 5 && miss_ok == 0 && found_bad == 1 && miss_bad == 4,
        "sections",
    );

    let budget = DriverBudget { probe_us_max: 50_000, isr_us_max: 20 };
    set.add(
        "F322 perf budget",
        budget_verdict(1_000, 10, budget) == BudgetVerdict::Within
            && budget_verdict(60_000, 10, budget) == BudgetVerdict::ProbeOver
            && budget_verdict(1_000, 40, budget) == BudgetVerdict::IsrOver
            && budget_verdict(60_000, 40, budget) == BudgetVerdict::BothOver,
        "verdicts",
    );

    let mut log = DriverEventLog::new();
    log.push(DriverEventRecord { kind: DriverEventKind::Load, driver: "ahci", arg: 0 });
    log.push(DriverEventRecord { kind: DriverEventKind::Fault, driver: "wifi", arg: 9 });
    set.add(
        "F323 driver log",
        log.len() == 2
            && log.get(0).unwrap().kind == DriverEventKind::Fault
            && log.count_of(DriverEventKind::Fault) == 1,
        "ring",
    );

    let manifest = SdkManifest {
        abi: 3,
        license: "MIT",
        entry_points: ["probe", "start", "stop", "", "", ""],
        entry_count: 3,
    };
    set.add(
        "F324 third party sdk",
        manifest_verdict(manifest) == ManifestVerdict::Ok
            && manifest_verdict(SdkManifest { entry_count: 0, ..manifest })
                == ManifestVerdict::NoEntryPoints
            && manifest_verdict(SdkManifest { entry_count: 1, ..manifest })
                == ManifestVerdict::MissingProbe,
        "manifest",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f302_bus_capacity() {
        let mut b = BusRegistry::new();
        for i in 0..MAX_BUSES + 3 {
            let ok = b.attach(BusEntry { kind: BusKind::Platform, devices: i as u32 });
            if i >= MAX_BUSES {
                assert!(!ok);
            }
        }
        assert_eq!(b.total_devices(), (0..MAX_BUSES as u32).sum::<u32>());
    }

    #[test]
    fn f303_tree_parent_cycles_terminate() {
        let mut t = DeviceTree::new();
        t.add(DeviceNode { id: 0, parent: Some(1), kind: BusKind::Pci });
        t.add(DeviceNode { id: 1, parent: Some(0), kind: BusKind::Pci });
        assert_eq!(t.depth(0), None); // cycle guard
        assert!(!t.is_ancestor(5, 0));
    }

    #[test]
    fn f304_hotplug_queue_full() {
        let mut q = HotplugQueue::new();
        for i in 0..MAX_HOTPLUG_EVENTS {
            assert!(q.push(HotplugEvent {
                action: HotplugAction::Change,
                device_id: i as u16,
                bus: BusKind::Pci,
            }));
        }
        assert!(!q.push(HotplugEvent {
            action: HotplugAction::Insert,
            device_id: 99,
            bus: BusKind::Pci,
        }));
        assert_eq!(q.len(), MAX_HOTPLUG_EVENTS);
        assert_eq!(q.pop().map(|e| e.device_id), Some(0));
        assert!(q.push(HotplugEvent {
            action: HotplugAction::Remove,
            device_id: 100,
            bus: BusKind::Usb,
        }));
    }

    #[test]
    fn f305_illegal_transitions_rejected() {
        assert!(driver_transition(DriverState::Registered, DriverEvent::Stop).is_none());
        assert!(driver_transition(DriverState::Running, DriverEvent::Probe).is_none());
        assert_eq!(
            driver_transition(DriverState::Failed, DriverEvent::Unbind),
            Some(DriverState::Registered)
        );
        assert!(!is_live(DriverState::Stopped));
    }

    #[test]
    fn f306_fault_window_forgets_old_faults() {
        let policy = FaultPolicy { max_faults: 2, window_ms: 100 };
        let mut h = DriverHealth::new();
        let _ = h.record_fault(0, policy);
        assert!(!h.quarantined);
        let q = h.record_fault(5000, policy);
        assert!(!q, "fault outside the window resets the counter");
        let q2 = h.record_fault(5100, policy);
        assert!(q2);
    }

    #[test]
    fn f307_compat_capacity() {
        let mut db = CompatDb::new();
        for i in 0..MAX_COMPAT + 2 {
            let ok = db.add(CompatEntry {
                vendor: 1,
                device: i as u16,
                driver: "d",
                quirk: Quirk::None,
                confidence: 10,
            });
            if i >= MAX_COMPAT {
                assert!(!ok);
            }
        }
        assert_eq!(db.len(), MAX_COMPAT);
        assert!(db.lookup(1, 99).is_none());
    }

    #[test]
    fn f309_hash_is_stable() {
        assert_eq!(hash64(b"varix"), hash64(b"varix"));
        assert_ne!(hash64(b"varix"), hash64(b"variy"));
        assert!(constant_time_eq(&[1, 2, 3, 4, 5, 6, 7, 8], &[1, 2, 3, 4, 5, 6, 7, 8]));
        assert!(!constant_time_eq(&[1, 2, 3, 4, 5, 6, 7, 8], &[1, 2, 3, 4, 5, 6, 7, 9]));
    }

    #[test]
    fn f310_version_parsing_edges() {
        assert!(DriverVersion::parse("").is_none());
        assert_eq!(DriverVersion::parse("1.x.0"), None);
        assert_eq!(DriverVersion::parse("1.2.3-rc1"), Some(DriverVersion::new(1, 2, 3)));
        assert_eq!(DriverVersion::parse("65535.0.0"), Some(DriverVersion::new(65535, 0, 0)));
        assert_eq!(DriverVersion::parse("70000.0.0"), None);
    }

    #[test]
    fn f312_bar_sizes() {
        assert_eq!(bar_size(0xFFFF_0000), Some(65536));
        assert_eq!(bar_size(0xFFFF_FFFC), Some(16));
        assert_eq!(bar_size(0xFFFF_FFFF), None);
        assert_eq!(pci_config_address(0xFF, 0x1F, 0x7, 0xFC), 0x80FF_FFFC);
    }

    #[test]
    fn f313_capability_loop_is_bounded() {
        let mut cfg = [0u8; 68];
        cfg[6] = 0x10;
        cfg[0x34] = 0x40;
        cfg[0x40] = 0x01; // some other cap
        cfg[0x41] = 0x40; // points at itself -> loop
        assert_eq!(find_capability(&cfg, 0x10), None);
        cfg[6] = 0; // no capability list
        assert_eq!(find_capability(&cfg, 0x01), None);
        assert_eq!(find_capability(&[0u8; 8], 0x01), None);
    }

    #[test]
    fn f314_dma_ownership_errors() {
        let mut b = DmaBuffer::new(0x2000, 64);
        assert_eq!(b.submit(8), Err(DmaError::NotCpuOwned));
        b.owner = DmaOwner::Cpu;
        assert_eq!(b.submit(65), Err(DmaError::Overflow));
        assert!(b.submit(64).is_ok());
        assert_eq!(b.complete(), Ok(()));
        assert_eq!(b.complete(), Err(DmaError::NotDeviceOwned));
    }

    #[test]
    fn f315_shared_irq_capacity() {
        fn t() -> bool {
            true
        }
        let mut irq = SharedIrq::new(1);
        for _ in 0..MAX_SHARED_HANDLERS {
            assert!(irq.add_handler("h", t));
        }
        assert!(!irq.add_handler("h", t));
        assert_eq!(irq.dispatch(), MAX_SHARED_HANDLERS);
    }

    #[test]
    fn f317_port_range_math() {
        let r = PortRange::new(0x3F8, 8);
        assert!(r.contains(0x3F8));
        assert!(r.contains(0x3FF));
        assert!(!r.contains(0x400));
        let mut p = PortSpace::new();
        assert!(!p.permitted(0x3F8));
        assert!(p.allow(r));
    }

    #[test]
    fn f318_reserved_map_capacity() {
        let mut m = ReservedMap::new();
        for i in 0..MAX_RESERVED + 2 {
            let ok = m.add(ReservedRange { start: i as u64 * 0x1000, len: 0x1000, reason: "r" });
            if i >= MAX_RESERVED {
                assert!(!ok);
            }
        }
        assert!(m.overlaps(0x100_0000, 0x10).is_none());
        assert!(m.mapping_allowed(0x100_0000, 0x10));
    }

    #[test]
    fn f319_loader_order_and_errors() {
        let mut out = [0usize; MAX_LOAD_STEPS];
        // a <- b <- c : c must come last.
        let deps: [&[usize]; 3] = [&[], &[0], &[1]];
        assert_eq!(resolve_load_order(&deps, &mut out), Ok(3));
        assert_eq!(&out[..3], &[0usize, 1, 2]);
        let too_many: [&[usize]; 1] = [&[]];
        let mut small = [0usize; 0];
        assert_eq!(resolve_load_order(&too_many, &mut small), Err(LoadError::TooMany));
    }

    #[test]
    fn f320_abi_boundaries() {
        assert_eq!(negotiate_abi(1, 2, 3, 4), None);
        assert_eq!(negotiate_abi(4, 6, 1, 4), Some(4));
        assert_eq!(negotiate_abi(0, 0, 0, 0), Some(0));
    }

    #[test]
    fn f323_driver_log_ring() {
        let mut l = DriverEventLog::new();
        for i in 0..DRIVER_EVENT_CAP + 3 {
            l.push(DriverEventRecord {
                kind: DriverEventKind::Load,
                driver: "d",
                arg: i as u32,
            });
        }
        assert_eq!(l.len(), DRIVER_EVENT_CAP);
        assert_eq!(l.get(0).unwrap().arg, (DRIVER_EVENT_CAP + 2) as u32);
        assert!(l.get(DRIVER_EVENT_CAP).is_none());
    }

    #[test]
    fn f324_manifest_requires_entries() {
        let m = SdkManifest {
            abi: 1,
            license: "Apache-2.0",
            entry_points: ["probe", "start", "stop", "x", "y", "z"],
            entry_count: 6,
        };
        assert_eq!(manifest_verdict(m), ManifestVerdict::Ok);
        assert_eq!(
            manifest_verdict(SdkManifest { entry_count: 2, ..m }),
            ManifestVerdict::MissingProbe
        );
        assert_eq!(
            manifest_verdict(SdkManifest { license: "", ..m }),
            ManifestVerdict::MissingProbe
        );
    }

    #[test]
    fn f325_self_test_passes() {
        let set = run_driver_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("driver self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
