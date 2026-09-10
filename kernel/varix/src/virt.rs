//! AI-14 虚拟化并存域（F326~F350）。
//!
//! Hardware-virtualisation bring-up arithmetic, VMCS/EPT encodings, the
//! guest lifecycle, the host-vs-guest arbitration rules that keep Varix
//! always in front (F340), the coexistence channels (clipboard, shares,
//! audio, input, capture) and the no-VT-x fallback.
//!
//! Discipline: everything here is pure logic plus policy — the domain is
//! allowed to degrade to boot-level coexistence (F349) on machines without
//! VT-x, and must never block the other 475 features.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F326 — VT-x 探测
// ---------------------------------------------------------------------------

/// CPUID.1:ECX bit 5 — VMX support.
pub const CPUID_ECX_VMX: u32 = 1 << 5;

/// IA32_VMX_PROCBASED_CTLS2 "enable EPT" (bit 1).
pub const VMX_CTLS2_EPT: u64 = 1 << 1;
/// "enable VPID" (bit 5).
pub const VMX_CTLS2_VPID: u64 = 1 << 5;
/// "unrestricted guest" (bit 7).
pub const VMX_CTLS2_UNRESTRICTED: u64 = 1 << 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VtxCapability {
    pub vmx: bool,
    pub ept: bool,
    pub vpid: bool,
    pub unrestricted_guest: bool,
}

impl VtxCapability {
    /// Enough for the WTG guest: VMX + EPT + unrestricted guest.
    pub fn can_run_guest(&self) -> bool {
        self.vmx && self.ept && self.unrestricted_guest
    }
}

/// Decode VMX capabilities from CPUID.1:ECX and the secondary-control MSR.
pub fn vmx_capability(cpuid_ecx: u32, proc_based_ctls2: u64) -> VtxCapability {
    let vmx = cpuid_ecx & CPUID_ECX_VMX != 0;
    VtxCapability {
        vmx,
        ept: vmx && proc_based_ctls2 & VMX_CTLS2_EPT != 0,
        vpid: vmx && proc_based_ctls2 & VMX_CTLS2_VPID != 0,
        unrestricted_guest: vmx && proc_based_ctls2 & VMX_CTLS2_UNRESTRICTED != 0,
    }
}

/// Locked (must-be-1) bits: VMX control MSRs set bit 31 of the high dword.
pub fn apply_vmx_fixed_bits(want: u64, allowed0: u64, allowed1: u64) -> u64 {
    // Bits set in allowed0 must be 1; bits clear in allowed1 must be 0.
    (want | allowed0) & allowed1
}

// ---------------------------------------------------------------------------
// F327 — VMX 管理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmcsWidth {
    /// 16-bit field.
    W16,
    /// 64-bit field.
    W64,
    /// Natural width (32/64-bit).
    Natural,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmcsKind {
    Control,
    ReadOnly,
    Guest,
    Host,
}

/// Encode a VMCS field: bits 0..9 index, 10..11 kind, 12..13 width.
pub fn vmcs_field_encoding(kind: VmcsKind, width: VmcsWidth, index: u16) -> u16 {
    let k = match kind {
        VmcsKind::Control => 0,
        VmcsKind::ReadOnly => 1,
        VmcsKind::Guest => 2,
        VmcsKind::Host => 3,
    };
    let w = match width {
        VmcsWidth::W16 => 0,
        VmcsWidth::W64 => 1,
        VmcsWidth::Natural => 2,
    };
    ((k as u16) << 10) | ((w as u16) << 12) | (index & 0x1FF)
}

/// VMXON / VMCS regions always fit one page (4 KiB) on x86-64.
pub const VMX_REGION_BYTES: usize = 4096;

/// The VM-instruction error number → plain-language cause (F465 discipline).
pub fn vm_instruction_error(number: u32) -> &'static str {
    match number {
        1 => "VMCALL executed outside VMX operation",
        2 => "invalid control-field value",
        3 => "VM-entry failed with invalid guest state",
        4 => "VM-entry failed while loading MSRs",
        5 => "VMLAUNCH called on an already-launched VMCS",
        6 => "VMRESUME called on a not-yet-launched VMCS",
        7 => "VM-exit with invalid host state",
        8 => "VM-entry failed with invalid executive-VMCS pointer",
        12 => "VMXON executed with a non-4KiB-aligned region",
        13 => "VMXOFF with a corrupted VMXON pointer",
        _ => "unknown VMX error",
    }
}

// ---------------------------------------------------------------------------
// F328 — EPT 页表
// ---------------------------------------------------------------------------

pub const EPT_R: u64 = 1 << 0;
pub const EPT_W: u64 = 1 << 1;
pub const EPT_X: u64 = 1 << 2;
/// Memory type: bits 3..5 (0 = write-back, 6 = write-back override).
pub const EPT_MEMTYPE_SHIFT: u64 = 3;
pub const EPT_MEMTYPE_WB: u64 = 0;
pub const EPT_MEMTYPE_UC: u64 = 6 << EPT_MEMTYPE_SHIFT;
/// Physical-address field starts at bit 12.
pub const EPT_ADDR_SHIFT: u64 = 12;

/// Build a leaf EPT entry for a 4 KiB-aligned physical address.
pub fn ept_entry(phys: u64, read: bool, write: bool, execute: bool, mem_type: u64) -> u64 {
    let mut e = (phys & !0xFFFu64) | (mem_type & 0x38);
    if read {
        e |= EPT_R;
    }
    if write {
        e |= EPT_W;
    }
    if execute {
        e |= EPT_X;
    }
    e
}

pub fn ept_permissions(entry: u64) -> (bool, bool, bool) {
    (entry & EPT_R != 0, entry & EPT_W != 0, entry & EPT_X != 0)
}

pub fn ept_phys(entry: u64) -> u64 {
    entry >> EPT_ADDR_SHIFT << EPT_ADDR_SHIFT
}

/// The four page-table indices for a guest-virtual address.
pub fn ept_walk_indices(guest_addr: u64) -> [usize; 4] {
    [
        ((guest_addr >> 39) & 0x1FF) as usize,
        ((guest_addr >> 30) & 0x1FF) as usize,
        ((guest_addr >> 21) & 0x1FF) as usize,
        ((guest_addr >> 12) & 0x1FF) as usize,
    ]
}

/// EPT misconfiguration when the page is writable but not readable.
pub fn ept_misconfigured(read: bool, write: bool, execute: bool) -> bool {
    (write && !read) || (execute && !read)
}

// ---------------------------------------------------------------------------
// F329 — 最小 hypervisor
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VcpuState {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub cr0: u64,
    pub cr3: u64,
    pub cr4: u64,
}

impl Default for VcpuState {
    fn default() -> VcpuState {
        // Sensible reset state: protected mode, paging off.
        VcpuState {
            rip: 0xFFF0,
            rsp: 0,
            rflags: 0x2,
            cr0: 0x6000_0010,
            cr3: 0,
            cr4: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmxExit {
    Exception,
    InterruptWindow,
    Cpuid,
    Hlt,
    IoPort,
    ControlRegister,
    MsrAccess,
    EptViolation,
    EptMisconfig,
    ExternalInterrupt,
    TripleFault,
    Vmcall,
    Unknown(u32),
}

/// Decode the basic exit-reason field (bits 0..15 of VM_EXIT_REASON).
pub fn decode_exit(reason: u32) -> VmxExit {
    let basic = reason & 0xFFFF;
    match basic {
        0 => VmxExit::Exception,
        7 => VmxExit::InterruptWindow,
        10 => VmxExit::Cpuid,
        12 => VmxExit::Hlt,
        28 => VmxExit::ControlRegister,
        30 => VmxExit::IoPort,
        31 => VmxExit::MsrAccess,
        48 => VmxExit::EptViolation,
        49 => VmxExit::EptMisconfig,
        1 => VmxExit::ExternalInterrupt,
        2 => VmxExit::TripleFault,
        18 => VmxExit::Vmcall,
        other => VmxExit::Unknown(other),
    }
}

/// Exit qualification bits 0..2 for EPT violations (read/write/execute).
pub fn ept_violation_access(qualification: u64) -> (bool, bool, bool) {
    (qualification & 1 != 0, qualification & 2 != 0, qualification & 4 != 0)
}

/// Guest instruction pointer advance after `cpuid`/`rdmsr` (VMX already
/// advances it; this is the book-keeping used by the emulator stub).
pub fn decode_instruction_len(instruction_info: u64) -> usize {
    (instruction_info & 0x7FFF) as usize
}

// ---------------------------------------------------------------------------
// F330 — Windows 客机引导
// ---------------------------------------------------------------------------

/// EFI system partition type GUID (first 8 bytes, mixed-endian).
pub const ESP_TYPE_PREFIX: u32 = 0xEF00_0000;
/// Microsoft basic-data partition type GUID prefix (mixed-endian).
pub const MSFT_BASIC_DATA_PREFIX: u32 = 0xEBD0_A0C0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partition {
    pub index: u8,
    pub type_prefix: u32,
    pub size_bytes: u64,
    /// Set when a `\Windows\System32\ntoskrnl.exe` probe succeeded.
    pub has_windows: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestOs {
    Windows,
    Other,
    None,
}

/// Pick the WTG (Windows-to-go) partition: a Windows-flagged partition with
/// the largest size. `None` when the machine has no Windows install.
pub fn detect_wtg(partitions: &[Partition]) -> Option<Partition> {
    let mut best: Option<Partition> = None;
    for p in partitions {
        if !p.has_windows {
            continue;
        }
        let better = match best {
            Some(b) => p.size_bytes > b.size_bytes,
            None => true,
        };
        if better {
            best = Some(*p);
        }
    }
    best
}

pub fn classify_guest(p: Option<Partition>) -> GuestOs {
    match p {
        Some(p) if p.has_windows => GuestOs::Windows,
        Some(_) => GuestOs::Other,
        None => GuestOs::None,
    }
}

// ---------------------------------------------------------------------------
// F331 — 客机生命周期
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestState {
    Created,
    Running,
    Paused,
    Saved,
    Stopped,
    Crashed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestEvent {
    Launch,
    Pause,
    Resume,
    Save,
    Restore,
    Stop,
    Fault,
}

pub fn guest_transition(state: GuestState, event: GuestEvent) -> Option<GuestState> {
    use GuestEvent::*;
    use GuestState::*;
    match (state, event) {
        (Created, Launch) => Some(Running),
        (Paused, Resume) => Some(Running),
        (Running, Pause) => Some(Paused),
        (Running, Save) | (Paused, Save) => Some(Saved),
        (Saved, Restore) => Some(Paused),
        (Running, Stop) | (Paused, Stop) | (Saved, Stop) | (Crashed, Stop) => Some(Stopped),
        (Stopped, Launch) => Some(Running),
        // A guest fault never leaves the guest running (F343).
        (_, Fault) if state != Stopped => Some(Crashed),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// F332/F346 — 快照与差分管理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotMeta {
    pub id: u32,
    pub parent: u32,
    pub bytes: u64,
    pub crc: u32,
    pub stamp: u64,
}

pub const MAX_SNAPSHOTS: usize = 8;
/// Refuse to chain deeper than this — restore time grows with depth.
pub const MAX_DIFF_DEPTH: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct SnapshotTree {
    nodes: [Option<SnapshotMeta>; MAX_SNAPSHOTS],
    count: usize,
}

impl SnapshotTree {
    pub const fn new() -> SnapshotTree {
        SnapshotTree { nodes: [None; MAX_SNAPSHOTS], count: 0 }
    }

    pub fn add(&mut self, meta: SnapshotMeta) -> Result<(), &'static str> {
        if self.count >= MAX_SNAPSHOTS {
            return Err("snapshot table full");
        }
        if meta.parent != 0 && self.find(meta.parent).is_none() {
            return Err("parent snapshot missing");
        }
        if self.depth_of(meta.parent) + 1 >= MAX_DIFF_DEPTH {
            return Err("diff chain too deep");
        }
        self.nodes[self.count] = Some(meta);
        self.count += 1;
        Ok(())
    }

    pub fn find(&self, id: u32) -> Option<SnapshotMeta> {
        for i in 0..self.count {
            if let Some(n) = self.nodes[i] {
                if n.id == id {
                    return Some(n);
                }
            }
        }
        None
    }

    /// Chain depth below the root (root itself = 0).
    pub fn depth_of(&self, id: u32) -> usize {
        let mut d = 0usize;
        let mut cur = match self.find(id) {
            Some(n) => n,
            None => return 0,
        };
        while cur.parent != 0 && d < MAX_SNAPSHOTS {
            cur = match self.find(cur.parent) {
                Some(n) => n,
                None => break,
            };
            d += 1;
        }
        d
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

/// Bytes needed for a hibernation image of `pages` guest pages.
pub fn hibernation_bytes(pages: u64, page_size: u64) -> u64 {
    pages.saturating_mul(page_size).saturating_add(1 << 20)
}

// ---------------------------------------------------------------------------
// F333 — 窗口收养
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdoptedWindow {
    pub guest_id: u16,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub z: u16,
    /// `false` = hidden into the tray (F417 semantics).
    pub visible: bool,
}

impl AdoptedWindow {
    /// Host-space rectangle after the guest's DPI scaling.
    pub fn host_rect(&self, scale_permille: u32) -> (i32, i32, u32, u32) {
        let s = scale_permille.max(1) as i64;
        (
            (self.x as i64 * s / 1000) as i32,
            (self.y as i64 * s / 1000) as i32,
            (self.width as i64 * s / 1000) as u32,
            (self.height as i64 * s / 1000) as u32,
        )
    }

    /// Hidden windows stay out of Alt-Tab and the taskbar (F417).
    pub fn in_alt_tab(&self) -> bool {
        self.visible
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x + self.width as i32
            && py < self.y + self.height as i32
    }
}

/// Topmost visible adopted window under the pointer (hit test, F186 link).
pub fn pick_window<'a>(windows: &'a [AdoptedWindow], px: i32, py: i32) -> Option<&'a AdoptedWindow> {
    let mut best: Option<&AdoptedWindow> = None;
    for w in windows.iter() {
        if !w.visible || !w.contains(px, py) {
            continue;
        }
        let better = match best {
            Some(b) => w.z >= b.z,
            None => true,
        };
        if better {
            best = Some(w);
        }
    }
    best
}

// ---------------------------------------------------------------------------
// F334 — 客机画面捕获
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureFormat {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    /// Bytes per pixel.
    pub bpp: u8,
}

pub const MAX_DIRTY_RECTS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl CaptureFormat {
    pub fn frame_bytes(&self) -> usize {
        self.stride as usize * self.height as usize
    }
}

pub fn capture_bandwidth_mbps(fmt: CaptureFormat, fps: u32) -> u32 {
    ((fmt.frame_bytes() as u64 * fps as u64 * 8) / 1_000_000) as u32
}

/// Merge-capable dirty list: adding a rect that overlaps an existing one
/// replaces it instead of growing the list (keeps damage tracking honest).
pub fn add_dirty(rects: &mut [Option<Rect>], count: &mut usize, rect: Rect) -> bool {
    for i in 0..*count {
        if let Some(r) = rects[i] {
            let overlaps = rect.x < r.x + r.w
                && r.x < rect.x + rect.w
                && rect.y < r.y + r.h
                && r.y < rect.y + rect.h;
            if overlaps {
                rects[i] = Some(merge_rect(r, rect));
                return true;
            }
        }
    }
    if *count >= rects.len() {
        return false;
    }
    rects[*count] = Some(rect);
    *count += 1;
    true
}

pub fn merge_rect(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = (a.x + a.w).max(b.x + b.w);
    let bottom = (a.y + a.h).max(b.y + b.h);
    Rect { x, y, w: right - x, h: bottom - y }
}

// ---------------------------------------------------------------------------
// F335 — 输入注入
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectKind {
    KeyDown,
    KeyUp,
    MouseMove,
    MouseButton,
    Wheel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectEvent {
    pub kind: InjectKind,
    /// PS/2 set-1 scancode for keys, button index for buttons.
    pub code: u16,
    pub x: i32,
    pub y: i32,
}

/// Map host coordinates into guest coordinates for the adopted window.
pub fn scale_to_guest(win: &AdoptedWindow, guest_w: u32, guest_h: u32, px: i32, py: i32) -> (u32, u32) {
    if win.width == 0 || win.height == 0 {
        return (0, 0);
    }
    let rx = ((px - win.x).max(0) as i64 * guest_w as i64 / win.width as i64) as u32;
    let ry = ((py - win.y).max(0) as i64 * guest_h as i64 / win.height as i64) as u32;
    (rx.min(guest_w.saturating_sub(1)), ry.min(guest_h.saturating_sub(1)))
}

/// USB HID usage (usage page 0x07) → PS/2 set-1 scancode, the two planes
/// the guest expects. Returns `None` for keys the guest has no scancode for.
pub fn hid_usage_to_ps2(usage: u8) -> Option<u16> {
    match usage {
        0x04 => Some(0x1E), // a
        0x05 => Some(0x30), // b
        0x06 => Some(0x2E), // c
        0x07 => Some(0x20), // d
        0x08 => Some(0x12), // e
        0x28 => Some(0x1C), // enter
        0x29 => Some(0x01), // escape
        0x2C => Some(0x39), // space
        0xE0 | 0xE1 | 0xE2 | 0xE3 => Some(0x1D), // ctrl
        0xE4 => Some(0x0E), // left shift (single-byte)
        _ => None,
    }
}

/// Trailing `0xE0` prefix required for the extended (arrow/ctrl) keys.
pub fn needs_e0_prefix(ps2: u16) -> bool {
    ps2 == 0x1D || (0x38..=0x48).contains(&ps2) && ps2 >= 0x47
}

// ---------------------------------------------------------------------------
// F336 — 设备直通
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassthroughDevice {
    pub bdf: u16,
    pub iommu_group: u16,
    pub kind: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassthroughVerdict {
    Allowed,
    /// Another function in the same IOMMU group is in use by the host.
    GroupBusy,
    /// Never hand the host storage or the root bridge to a guest.
    Forbidden,
}

pub const FORBIDDEN_KINDS: [&'static str; 3] = ["host-storage", "root-bridge", "host-gpu"];

pub fn passthrough_verdict(dev: PassthroughDevice, other_functions_in_group: usize) -> PassthroughVerdict {
    if FORBIDDEN_KINDS.contains(&dev.kind) {
        return PassthroughVerdict::Forbidden;
    }
    if other_functions_in_group > 1 {
        return PassthroughVerdict::GroupBusy;
    }
    PassthroughVerdict::Allowed
}

// ---------------------------------------------------------------------------
// F337 — 虚拟磁盘
// ---------------------------------------------------------------------------

pub const VDISK_MAGIC: [u8; 4] = *b"VRXD";
pub const VDISK_HEADER_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtDiskHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub cluster_bits: u8,
    pub l1_entries: u32,
    pub size_bytes: u64,
}

impl VirtDiskHeader {
    pub fn encode(self) -> [u8; VDISK_HEADER_BYTES] {
        let mut out = [0u8; VDISK_HEADER_BYTES];
        out[0..4].copy_from_slice(&self.magic);
        out[4..6].copy_from_slice(&self.version.to_le_bytes());
        out[6] = self.cluster_bits;
        out[8..12].copy_from_slice(&self.l1_entries.to_le_bytes());
        out[12..20].copy_from_slice(&self.size_bytes.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<VirtDiskHeader> {
        if bytes.len() < VDISK_HEADER_BYTES || &bytes[0..4] != &VDISK_MAGIC {
            return None;
        }
        let mut l1 = [0u8; 4];
        l1.copy_from_slice(&bytes[8..12]);
        let mut size = [0u8; 8];
        size.copy_from_slice(&bytes[12..20]);
        Some(VirtDiskHeader {
            magic: VDISK_MAGIC,
            version: u16::from_le_bytes([bytes[4], bytes[5]]),
            cluster_bits: bytes[6],
            l1_entries: u32::from_le_bytes(l1),
            size_bytes: u64::from_le_bytes(size),
        })
    }

    pub fn valid(&self) -> bool {
        self.magic == VDISK_MAGIC
            && self.version >= 1
            && (self.cluster_bits as u32) >= 12
            && self.cluster_bits <= 21
            && self.size_bytes > 0
    }
}

/// Byte offset of the cluster-level-2 table entry for a logical address.
pub fn vdisk_cluster_index(lba_bytes: u64, cluster_bits: u8) -> u64 {
    lba_bytes >> cluster_bits
}

pub fn vdisk_cluster_size(cluster_bits: u8) -> u64 {
    1u64 << cluster_bits
}

/// How many L1 entries a disk of `size_bytes` needs.
pub fn vdisk_l1_entries(size_bytes: u64, cluster_bits: u8) -> u32 {
    let per_l2 = 512u64; // 4 KiB L2 table / 8-byte entries
    let clusters = (size_bytes >> cluster_bits).max(1);
    ((clusters / per_l2) + 1).min(u32::MAX as u64) as u32
}

// ---------------------------------------------------------------------------
// F338 — 虚拟网络
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtNetMode {
    Nat,
    Bridge,
    Isolated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaseTable {
    pub base_last_octet: u8,
    pub size: u8,
    used: u32,
}

impl LeaseTable {
    pub const fn new(base_last_octet: u8, size: u8) -> LeaseTable {
        LeaseTable { base_last_octet, size, used: 0 }
    }

    /// Bitmap of granted leases — the guest gets the first free address.
    pub fn allocate(&mut self, index: u8) -> Option<u8> {
        if index >= self.size {
            return None;
        }
        let bit = 1u32 << index;
        if self.used & bit != 0 {
            return None;
        }
        self.used |= bit;
        Some(self.base_last_octet.saturating_add(index))
    }

    pub fn release(&mut self, index: u8) -> bool {
        if index >= self.size {
            return false;
        }
        let bit = 1u32 << index;
        let had = self.used & bit != 0;
        self.used &= !bit;
        had
    }

    pub fn used_count(&self) -> u32 {
        self.used.count_ones()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortForward {
    pub host_port: u16,
    pub guest_port: u16,
    pub proto: &'static str,
}

pub const MAX_FORWARDS: usize = 8;

/// NAT needs at least one host→guest forward to be useful; bridge mode
/// must not have forwards (the guest is on the LAN directly).
pub fn forwards_ok(mode: VirtNetMode, count: usize) -> bool {
    match mode {
        VirtNetMode::Nat => count <= MAX_FORWARDS,
        VirtNetMode::Bridge => count == 0,
        VirtNetMode::Isolated => count == 0,
    }
}

// ---------------------------------------------------------------------------
// F339/F340 — 客机算力限额与宿主优先
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestQuota {
    /// Guest CPU share cap, permille of total cycles.
    pub cpu_permille: u16,
    pub mem_bytes: u64,
    pub io_mbps: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaAction {
    Ok,
    ThrottleCpu,
    RefuseStart,
}

pub fn quota_action(quota: GuestQuota, want_mem: u64, host_free_mem: u64) -> QuotaAction {
    if quota.cpu_permille == 0 || quota.mem_bytes == 0 {
        return QuotaAction::RefuseStart;
    }
    if want_mem > host_free_mem / 2 || want_mem > quota.mem_bytes {
        return QuotaAction::RefuseStart;
    }
    if quota.cpu_permille > 900 {
        return QuotaAction::ThrottleCpu; // host must keep a slice
    }
    QuotaAction::Ok
}

/// Varix is always in front: guest share shrinks as host load grows, and is
/// hard-capped so a busy guest can never starve the desktop (F340).
pub fn arbiter_guest_share(host_load_permille: u16, guest_want_permille: u16) -> u16 {
    let headroom = 1000u16.saturating_sub(host_load_permille);
    guest_want_permille.min(headroom).min(500)
}

// ---------------------------------------------------------------------------
// F341/F342 — 剪贴板通道与共享文件夹
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipFormat {
    Text,
    Html,
    Image,
    Files,
}

pub const CLIP_MAX_BYTES: usize = 4 << 20;

/// Sanitised clipboard packet size: oversized payloads are dropped rather
/// than truncated (a half-copied file list is worse than none).
pub fn clip_payload_ok(format: ClipFormat, bytes: usize) -> bool {
    match format {
        ClipFormat::Text | ClipFormat::Html => bytes > 0 && bytes <= CLIP_MAX_BYTES / 4,
        ClipFormat::Image => bytes <= CLIP_MAX_BYTES,
        ClipFormat::Files => bytes <= CLIP_MAX_BYTES * 4,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShareMount {
    pub host_path: &'static str,
    pub guest_path: &'static str,
    pub read_only: bool,
}

/// Reject share names that would escape the mount root.
pub fn share_name_safe(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name.contains('/')
        && !name.contains('\\')
        && name.len() <= 64
}

pub fn share_allowed(mount: ShareMount, write: bool) -> bool {
    if write && mount.read_only {
        return false;
    }
    share_name_safe(mount.guest_path) || mount.guest_path == "/media/host"
}

// ---------------------------------------------------------------------------
// F343 — 客机崩溃隔离
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestCrash {
    pub exit_reason: u32,
    /// Triple fault = guest unrecoverable.
    pub triple_fault: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostAction {
    /// Guest is gone; the host continues, unsaved guest state is lost.
    IsolateGuest,
    /// Recoverable exit: log and keep the guest running.
    LogOnly,
}

pub fn host_action(crash: GuestCrash) -> HostAction {
    if crash.triple_fault || (crash.exit_reason & 0xFFFF) == 2 {
        HostAction::IsolateGuest
    } else {
        HostAction::LogOnly
    }
}

// ---------------------------------------------------------------------------
// F344 — 虚拟化性能仪表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtPerf {
    pub exits_per_sec: u32,
    pub avg_exit_us: u32,
    /// Host time consumed per wall second, permille.
    pub host_time_permille: u16,
    pub redline_permille: u16,
}

impl VirtPerf {
    /// Virtualisation overhead in percent of wall time.
    pub fn overhead_percent(&self) -> u32 {
        (self.exits_per_sec as u64 * self.avg_exit_us as u64 / 10_000) as u32
    }

    pub fn over_redline(&self) -> bool {
        self.host_time_permille > self.redline_permille
    }
}

// ---------------------------------------------------------------------------
// F345 — 客机音频通道
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestAudioChannel {
    pub rate_hz: u32,
    pub channels: u8,
    pub buffer_ms: u32,
    pub host_output_ms: u32,
}

impl GuestAudioChannel {
    /// Host-side delay needed so guest audio lines up with the mixer clock.
    pub fn latency_compensation_ms(&self) -> u32 {
        self.host_output_ms.saturating_sub(self.buffer_ms)
    }

    /// Mix one guest frame into the host mix bus (saturating add).
    pub fn mix_into_host(&self, host: i16, guest: i16) -> i16 {
        let sum = host as i32 + guest as i32;
        sum.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }
}

// ---------------------------------------------------------------------------
// F347 — 客机分辨率协商
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub preferred: bool,
}

/// Choose the mode closest to (but not exceeding) the adopted window, so the
/// guest never renders pixels the desktop cannot show.
pub fn best_mode(modes: &[DisplayMode], want_w: u32, want_h: u32) -> Option<DisplayMode> {
    let mut best: Option<DisplayMode> = None;
    for m in modes {
        let fits = m.width <= want_w && m.height <= want_h;
        if !fits {
            continue;
        }
        let better = match best {
            Some(b) => {
                m.width as u64 * m.height as u64 > b.width as u64 * b.height as u64
                    || (m.width == b.width && m.preferred && !b.preferred)
            }
            None => true,
        };
        if better {
            best = Some(*m);
        }
    }
    best
}

// ---------------------------------------------------------------------------
// F348 — 并存健康检查
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoexistHealth {
    pub host_varix: bool,
    pub wtg_guest: bool,
    pub host_windows: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoexistVerdict {
    AllSystems,
    Degraded,
    HostOnly,
}

pub fn coexist_verdict(h: CoexistHealth) -> CoexistVerdict {
    if !h.host_varix {
        return CoexistVerdict::HostOnly;
    }
    if h.wtg_guest && h.host_windows {
        CoexistVerdict::AllSystems
    } else {
        CoexistVerdict::Degraded
    }
}

// ---------------------------------------------------------------------------
// F349/F350 — 无 VT-x 降级与迁移占位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoVtxPlan {
    /// Boot-level coexistence (menu picks Varix / WTG / host Windows).
    pub boot_level_only: bool,
    pub live_coexistence: bool,
    pub note: &'static str,
}

pub fn no_vtx_plan(cap: VtxCapability) -> NoVtxPlan {
    if cap.can_run_guest() {
        NoVtxPlan {
            boot_level_only: false,
            live_coexistence: true,
            note: "full VMX coexistence",
        }
    } else {
        NoVtxPlan {
            boot_level_only: true,
            live_coexistence: false,
            note: "no VT-x/EPT: boot-level coexistence only",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationPlan {
    pub capable: bool,
    pub steps: u8,
    pub chunk_bytes: u64,
}

/// Placeholder capability probe — real migration lands after S7.
pub fn migration_plan(total_bytes: u64, shared_storage: bool) -> MigrationPlan {
    MigrationPlan {
        capable: shared_storage && total_bytes > 0,
        steps: if shared_storage { 4 } else { 0 },
        chunk_bytes: if shared_storage { 16 << 20 } else { 0 },
    }
}

// ---------------------------------------------------------------------------
// F350 — 虚拟化并存自检
// ---------------------------------------------------------------------------

pub fn run_virt_checks() -> CheckSet {
    let mut set = CheckSet::new("virt");

    let cap = vmx_capability(CPUID_ECX_VMX, VMX_CTLS2_EPT | VMX_CTLS2_VPID | VMX_CTLS2_UNRESTRICTED);
    set.add(
        "F326 vtx probe",
        cap.vmx && cap.ept && cap.vpid && cap.unrestricted_guest && cap.can_run_guest(),
        "cpuid + msr",
    );
    set.add(
        "F326 no vtx",
        !vmx_capability(0, u64::MAX).vmx && !vmx_capability(0, u64::MAX).can_run_guest(),
        "absent vmx",
    );
    set.add(
        "F326 fixed bits",
        apply_vmx_fixed_bits(0, 0b01, 0b11) == 0b01 && apply_vmx_fixed_bits(0b10, 0, 0b10) == 0b10,
        "locked bits",
    );

    set.add(
        "F327 vmcs encoding",
        vmcs_field_encoding(VmcsKind::Guest, VmcsWidth::W64, 0x2800)
            == (2 << 10) | (1 << 12) | 0x000
            && vmcs_field_encoding(VmcsKind::Host, VmcsWidth::Natural, 0x0C)
                == (3 << 10) | (2 << 12) | 0x0C
            && VMX_REGION_BYTES == 4096,
        "field encode",
    );
    set.add(
        "F327 vmx errors",
        vm_instruction_error(3).contains("guest state")
            && vm_instruction_error(9999) == "unknown VMX error",
        "error table",
    );

    let leaf = ept_entry(0x1_0000_0000, true, true, false, EPT_MEMTYPE_WB);
    set.add(
        "F328 ept entry",
        ept_permissions(leaf) == (true, true, false)
            && ept_phys(leaf) == 0x1_0000_0000
            && ept_walk_indices(0x1_0000_0000)[3] == 0
            && ept_walk_indices(0x1234_5000)[3] == 0x145
            && ept_misconfigured(true, true, false) == false,
        "ept encode",
    );
    set.add(
        "F328 ept misconfig",
        ept_misconfigured(false, true, false) && ept_misconfigured(false, false, true),
        "misconfig rules",
    );
    let uc = ept_entry(0x2000, true, false, false, EPT_MEMTYPE_UC);
    set.add("F328 ept memtype", uc & 0x38 == EPT_MEMTYPE_UC, "uncached");

    let st = VcpuState::default();
    set.add(
        "F329 vcpu state",
        st.rip == 0xFFF0 && st.cr0 & 1 == 0,
        "reset state",
    );
    set.add(
        "F329 exit decode",
        decode_exit(48) == VmxExit::EptViolation
            && decode_exit(10) == VmxExit::Cpuid
            && decode_exit(0x8000_0000) == VmxExit::Exception
            && matches!(decode_exit(999), VmxExit::Unknown(999))
            && ept_violation_access(0b11) == (true, true, false)
            && decode_instruction_len(0x7FFF) == 0x7FFF,
        "exit reasons",
    );

    let partitions = [
        Partition { index: 1, type_prefix: ESP_TYPE_PREFIX, size_bytes: 512 << 20, has_windows: false },
        Partition { index: 2, type_prefix: MSFT_BASIC_DATA_PREFIX, size_bytes: 200 << 30, has_windows: true },
        Partition { index: 3, type_prefix: MSFT_BASIC_DATA_PREFIX, size_bytes: 400 << 30, has_windows: true },
        Partition { index: 4, type_prefix: 0, size_bytes: 100 << 30, has_windows: false },
    ];
    let wtg = detect_wtg(&partitions);
    set.add(
        "F330 wtg detect",
        wtg.map(|p| p.index) == Some(3)
            && classify_guest(wtg) == GuestOs::Windows
            && classify_guest(None) == GuestOs::None,
        "partition probe",
    );

    let mut gstate = GuestState::Created;
    for event in [GuestEvent::Launch, GuestEvent::Pause, GuestEvent::Save, GuestEvent::Restore] {
        gstate = guest_transition(gstate, event).unwrap_or(gstate);
    }
    set.add(
        "F331 guest lifecycle",
        gstate == GuestState::Paused
            && guest_transition(gstate, GuestEvent::Stop) == Some(GuestState::Stopped)
            && guest_transition(GuestState::Running, GuestEvent::Fault) == Some(GuestState::Crashed)
            && guest_transition(GuestState::Created, GuestEvent::Stop).is_none(),
        "guest states",
    );

    let mut tree = SnapshotTree::new();
    let root = SnapshotMeta { id: 1, parent: 0, bytes: 1 << 30, crc: 1, stamp: 10 };
    let child = SnapshotMeta { id: 2, parent: 1, bytes: 1 << 20, crc: 2, stamp: 20 };
    let ok_root = tree.add(root).is_ok();
    let ok_child = tree.add(child).is_ok();
    let orphan = tree.add(SnapshotMeta { id: 3, parent: 99, bytes: 0, crc: 0, stamp: 30 });
    set.add(
        "F332/F346 snapshots",
        ok_root
            && ok_child
            && orphan.is_err()
            && tree.depth_of(2) == 1
            && tree.depth_of(1) == 0
            && hibernation_bytes(4, 4096) == 4 * 4096 + (1 << 20),
        "snapshot tree",
    );

    let win = AdoptedWindow {
        guest_id: 7,
        x: 100,
        y: 50,
        width: 800,
        height: 600,
        z: 3,
        visible: true,
    };
    let hidden = AdoptedWindow { visible: false, z: 9, ..win };
    let low = AdoptedWindow { z: 1, ..win };
    set.add(
        "F333 window adoption",
        win.host_rect(2000) == (200, 100, 1600, 1200)
            && win.contains(200, 100)
            && !win.contains(99, 100)
            && pick_window(&[low, hidden, win], 200, 200).map(|w| w.z) == Some(3)
            && !hidden.in_alt_tab(),
        "adoption + hit test",
    );

    let fmt = CaptureFormat { width: 1920, height: 1080, stride: 1920 * 4, bpp: 32 };
    let mut rects = [None; MAX_DIRTY_RECTS];
    let mut count = 0usize;
    let a = Rect { x: 0, y: 0, w: 100, h: 100 };
    let b = Rect { x: 50, y: 50, w: 100, h: 100 };
    let added_a = add_dirty(&mut rects, &mut count, a);
    let added_b = add_dirty(&mut rects, &mut count, b);
    set.add(
        "F334 capture",
        fmt.frame_bytes() == 1920 * 4 * 1080
            && capture_bandwidth_mbps(fmt, 60) > 0
            && added_a
            && added_b
            && count == 1
            && rects[0].unwrap().w == 150
            && rects[0].unwrap().h == 150,
        "dirty merge",
    );

    set.add(
        "F335 input injection",
        scale_to_guest(&win, 1600, 1200, 500, 350) == (800, 600)
            && scale_to_guest(&win, 1600, 1200, 0, 0) == (0, 0)
            && hid_usage_to_ps2(0x04) == Some(0x1E)
            && hid_usage_to_ps2(0x99).is_none()
            && needs_e0_prefix(0x1D),
        "coordinate map",
    );

    let dev = PassthroughDevice { bdf: 0x0100, iommu_group: 3, kind: "usb-controller" };
    set.add(
        "F336 passthrough",
        passthrough_verdict(dev, 1) == PassthroughVerdict::Allowed
            && passthrough_verdict(dev, 4) == PassthroughVerdict::GroupBusy
            && passthrough_verdict(PassthroughDevice { kind: "host-storage", ..dev }, 1)
                == PassthroughVerdict::Forbidden,
        "eligibility",
    );

    let header = VirtDiskHeader {
        magic: VDISK_MAGIC,
        version: 1,
        cluster_bits: 16,
        l1_entries: vdisk_l1_entries(64 << 30, 16),
        size_bytes: 64 << 30,
    };
    let encoded = header.encode();
    let decoded = VirtDiskHeader::decode(&encoded).expect("vdisk");
    set.add(
        "F337 virtual disk",
        decoded.valid()
            && decoded.size_bytes == 64 << 30
            && vdisk_cluster_size(16) == 65536
            && vdisk_cluster_index(65536, 16) == 1
            && VirtDiskHeader::decode(b"XXXX").is_none(),
        "header + cluster math",
    );

    let mut leases = LeaseTable::new(100, 8);
    let first = leases.allocate(1);
    let dup = leases.allocate(1);
    let released = leases.release(1);
    set.add(
        "F338 virtual network",
        first == Some(101)
            && dup.is_none()
            && released
            && leases.used_count() == 0
            && forwards_ok(VirtNetMode::Bridge, 0)
            && !forwards_ok(VirtNetMode::Bridge, 3),
        "leases + forwards",
    );

    let quota = GuestQuota { cpu_permille: 400, mem_bytes: 8 << 30, io_mbps: 500 };
    set.add(
        "F339 guest quota",
        quota_action(quota, 4 << 30, 16 << 30) == QuotaAction::Ok
            && quota_action(quota, 4 << 30, 4 << 30) == QuotaAction::RefuseStart
            && quota_action(GuestQuota { cpu_permille: 950, ..quota }, 1 << 30, 16 << 30)
                == QuotaAction::ThrottleCpu,
        "quota policy",
    );
    set.add(
        "F340 host first",
        arbiter_guest_share(100, 800) == 500
            && arbiter_guest_share(900, 800) == 100
            && arbiter_guest_share(1000, 800) == 0,
        "arbiter",
    );

    set.add(
        "F341 clipboard",
        clip_payload_ok(ClipFormat::Text, 1024)
            && !clip_payload_ok(ClipFormat::Text, 0)
            && !clip_payload_ok(ClipFormat::Image, CLIP_MAX_BYTES + 1),
        "payload limits",
    );
    set.add(
        "F342 shares",
        share_name_safe("downloads")
            && !share_name_safe("../etc")
            && !share_name_safe("a/b")
            && !share_allowed(
                ShareMount { host_path: "C:\\", guest_path: "downloads", read_only: true },
                true,
            ),
        "path safety",
    );

    set.add(
        "F343 crash isolation",
        host_action(GuestCrash { exit_reason: 2, triple_fault: true }) == HostAction::IsolateGuest
            && host_action(GuestCrash { exit_reason: 48, triple_fault: false }) == HostAction::LogOnly,
        "host survives",
    );

    let perf = VirtPerf {
        exits_per_sec: 20_000,
        avg_exit_us: 2,
        host_time_permille: 80,
        redline_permille: 150,
    };
    set.add(
        "F344 virt perf",
        perf.overhead_percent() == 4 && !perf.over_redline(),
        "overhead",
    );

    let audio = GuestAudioChannel { rate_hz: 48_000, channels: 2, buffer_ms: 20, host_output_ms: 30 };
    set.add(
        "F345 guest audio",
        audio.latency_compensation_ms() == 10
            && audio.mix_into_host(20_000, 20_000) == 32_767
            && audio.rate_hz == 48_000,
        "audio bridge",
    );

    let modes = [
        DisplayMode { width: 1280, height: 720, preferred: false },
        DisplayMode { width: 1920, height: 1080, preferred: true },
        DisplayMode { width: 2560, height: 1440, preferred: true },
    ];
    set.add(
        "F347 resolution",
        best_mode(&modes, 2000, 1200).map(|m| m.width) == Some(1920)
            && best_mode(&modes, 100, 100).is_none()
            && best_mode(&modes, 3840, 2160).map(|m| m.width) == Some(2560),
        "mode pick",
    );

    set.add(
        "F348 coexist health",
        coexist_verdict(CoexistHealth { host_varix: true, wtg_guest: true, host_windows: true })
            == CoexistVerdict::AllSystems
            && coexist_verdict(CoexistHealth { host_varix: true, wtg_guest: false, host_windows: true })
                == CoexistVerdict::Degraded
            && coexist_verdict(CoexistHealth { host_varix: false, wtg_guest: true, host_windows: true })
                == CoexistVerdict::HostOnly,
        "health",
    );

    let no_vtx = no_vtx_plan(VtxCapability { vmx: false, ept: false, vpid: false, unrestricted_guest: false });
    set.add(
        "F349 no-vtx fallback",
        no_vtx.boot_level_only && !no_vtx.live_coexistence && !no_vtx.note.is_empty()
            && !no_vtx_plan(cap).boot_level_only,
        "degrade",
    );
    set.add(
        "F350 migration placeholder",
        migration_plan(64 << 30, true).capable
            && migration_plan(64 << 30, true).steps == 4
            && !migration_plan(64 << 30, false).capable,
        "placeholder",
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
    fn f326_partial_capability() {
        let cap = vmx_capability(CPUID_ECX_VMX, VMX_CTLS2_EPT);
        assert!(cap.vmx && cap.ept);
        assert!(!cap.unrestricted_guest);
        assert!(!cap.can_run_guest(), "EPT alone is not enough for WTG");
    }

    #[test]
    fn f327_field_encoding_is_injective() {
        let a = vmcs_field_encoding(VmcsKind::Guest, VmcsWidth::W64, 0x10);
        let b = vmcs_field_encoding(VmcsKind::Host, VmcsWidth::W64, 0x10);
        let c = vmcs_field_encoding(VmcsKind::Guest, VmcsWidth::W16, 0x10);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a & 0x1FF, 0x10);
    }

    #[test]
    fn f328_ept_permission_round_trip() {
        let e = ept_entry(0xFFFF_F000, false, false, true, EPT_MEMTYPE_WB);
        assert_eq!(ept_permissions(e), (false, false, true));
        assert_eq!(ept_phys(e), 0xFFFF_F000);
        assert!(ept_misconfigured(false, false, true)); // execute without read
    }

    #[test]
    fn f330_no_windows_means_no_guest() {
        let parts = [Partition {
            index: 1,
            type_prefix: MSFT_BASIC_DATA_PREFIX,
            size_bytes: 1 << 40,
            has_windows: false,
        }];
        assert!(detect_wtg(&parts).is_none());
        assert_eq!(classify_guest(detect_wtg(&parts)), GuestOs::None);
        assert_eq!(classify_guest(Some(parts[0])), GuestOs::Other);
    }

    #[test]
    fn f331_guest_state_illegal_events() {
        assert!(guest_transition(GuestState::Created, GuestEvent::Pause).is_none());
        assert!(guest_transition(GuestState::Stopped, GuestEvent::Fault).is_none());
        assert_eq!(
            guest_transition(GuestState::Stopped, GuestEvent::Launch),
            Some(GuestState::Running)
        );
    }

    #[test]
    fn f332_snapshot_depth_limited() {
        let mut t = SnapshotTree::new();
        assert!(t.add(SnapshotMeta { id: 1, parent: 0, bytes: 1, crc: 0, stamp: 0 }).is_ok());
        for i in 2..=MAX_DIFF_DEPTH as u32 + 1 {
            let r = t.add(SnapshotMeta {
                id: i,
                parent: i - 1,
                bytes: 1,
                crc: 0,
                stamp: 0,
            });
            if i as usize > MAX_DIFF_DEPTH {
                assert!(r.is_err(), "chain deeper than {} must be refused", MAX_DIFF_DEPTH);
            }
        }
    }

    #[test]
    fn f333_pick_ignores_hidden() {
        let base = AdoptedWindow {
            guest_id: 1,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            z: 1,
            visible: true,
        };
        let hidden = AdoptedWindow { z: 9, visible: false, ..base };
        assert!(pick_window(&[hidden], 10, 10).is_none());
        assert!(pick_window(&[], 10, 10).is_none());
        assert_eq!(pick_window(&[base], 99, 99).map(|w| w.guest_id), Some(1));
        assert!(pick_window(&[base], 100, 100).is_none());
    }

    #[test]
    fn f334_dirty_rect_capacity() {
        let mut rects = [None; MAX_DIRTY_RECTS];
        let mut count = 0usize;
        for i in 0..MAX_DIRTY_RECTS + 4 {
            let ok = add_dirty(
                &mut rects,
                &mut count,
                Rect { x: i as u32 * 1000, y: 0, w: 10, h: 10 },
            );
            if i >= MAX_DIRTY_RECTS {
                assert!(!ok);
            }
        }
        assert_eq!(count, MAX_DIRTY_RECTS);
        assert_eq!(merge_rect(Rect { x: 0, y: 0, w: 10, h: 10 }, Rect { x: 5, y: 5, w: 10, h: 10 }).w, 15);
    }

    #[test]
    fn f335_scaling_clamps() {
        let win = AdoptedWindow {
            guest_id: 1,
            x: 0,
            y: 0,
            width: 100,
            height: 50,
            z: 0,
            visible: true,
        };
        assert_eq!(scale_to_guest(&win, 200, 100, 50, 25), (100, 50));
        assert_eq!(scale_to_guest(&win, 200, 100, 500, 500), (199, 99));
        let zero = AdoptedWindow { width: 0, height: 0, ..win };
        assert_eq!(scale_to_guest(&zero, 100, 100, 1, 1), (0, 0));
    }

    #[test]
    fn f337_vdisk_round_trip() {
        let h = VirtDiskHeader {
            magic: VDISK_MAGIC,
            version: 2,
            cluster_bits: 12,
            l1_entries: vdisk_l1_entries(1 << 30, 12),
            size_bytes: 1 << 30,
        };
        let back = VirtDiskHeader::decode(&h.encode()).unwrap();
        assert!(back.valid());
        assert_eq!(back.cluster_bits, 12);
        let bad = VirtDiskHeader { cluster_bits: 40, ..h };
        assert!(!bad.valid());
        assert_eq!(vdisk_cluster_index(1 << 20, 12), 256);
    }

    #[test]
    fn f338_lease_table_bounds() {
        let mut t = LeaseTable::new(10, 4);
        assert_eq!(t.allocate(0), Some(10));
        assert_eq!(t.allocate(3), Some(13));
        assert_eq!(t.allocate(4), None);
        assert!(t.release(4) == false);
        assert!(t.release(0));
        assert_eq!(t.used_count(), 1);
    }

    #[test]
    fn f339_quota_refusals() {
        let q = GuestQuota { cpu_permille: 300, mem_bytes: 4 << 30, io_mbps: 100 };
        assert_eq!(quota_action(q, 2 << 30, 16 << 30), QuotaAction::Ok);
        assert_eq!(quota_action(q, 9 << 30, 16 << 30), QuotaAction::RefuseStart);
        assert_eq!(
            quota_action(GuestQuota { cpu_permille: 0, ..q }, 1, 1 << 40),
            QuotaAction::RefuseStart
        );
    }

    #[test]
    fn f340_host_always_wins() {
        for load in [0u16, 200, 500, 800, 999, 1000] {
            let share = arbiter_guest_share(load, 1000);
            assert!(share <= 500, "guest share capped");
            assert!(share as u32 + load as u32 <= 1000, "host keeps its share");
        }
    }

    #[test]
    fn f344_overhead_math() {
        let p = VirtPerf {
            exits_per_sec: 100_000,
            avg_exit_us: 5,
            host_time_permille: 400,
            redline_permille: 150,
        };
        assert_eq!(p.overhead_percent(), 50);
        assert!(p.over_redline());
    }

    #[test]
    fn f347_best_mode_prefers_largest_fitting() {
        let modes = [
            DisplayMode { width: 800, height: 600, preferred: false },
            DisplayMode { width: 1024, height: 768, preferred: false },
        ];
        assert_eq!(best_mode(&modes, 1024, 768).map(|m| m.width), Some(1024));
        assert_eq!(best_mode(&modes, 1000, 768).map(|m| m.width), Some(800));
        assert!(best_mode(&[], 1024, 768).is_none());
    }

    #[test]
    fn f350_self_test_passes() {
        let set = run_virt_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("virt self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
