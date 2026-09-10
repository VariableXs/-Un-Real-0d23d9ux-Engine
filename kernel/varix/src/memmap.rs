//! F015 内存图解析 — Limine memory map classification and totals.
//! F016 保留内存管理 — boot-time reservation registry (kernel image,
//! framebuffer, ACPI tables, log ring) with overlap protection.
//!
//! Pure classification over entry slices + a target adapter that reads the
//! Limine memory map. The reservation registry is the single source of truth
//! for "which regions must survive into the runtime page allocator" (AI-03).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Memory region kinds, aligned with the Limine memmap entry kinds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Usable,
    Reserved,
    AcpiReclaimable,
    AcpiNvs,
    BadMemory,
    BootloaderReclaimable,
    ExecutableAndModules,
    Framebuffer,
}

impl Kind {
    pub fn from_limine(kind: u64) -> Option<Kind> {
        match kind {
            0 => Some(Kind::Usable),
            1 => Some(Kind::Reserved),
            2 => Some(Kind::AcpiReclaimable),
            3 => Some(Kind::AcpiNvs),
            4 => Some(Kind::BadMemory),
            5 => Some(Kind::BootloaderReclaimable),
            6 => Some(Kind::ExecutableAndModules),
            7 => Some(Kind::Framebuffer),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Usable => "usable",
            Kind::Reserved => "reserved",
            Kind::AcpiReclaimable => "acpi-reclaim",
            Kind::AcpiNvs => "acpi-nvs",
            Kind::BadMemory => "bad",
            Kind::BootloaderReclaimable => "bootloader-reclaim",
            Kind::ExecutableAndModules => "kernel/modules",
            Kind::Framebuffer => "framebuffer",
        }
    }

    /// May the page allocator ever hand this region out?
    pub fn is_allocatable(self) -> bool {
        matches!(self, Kind::Usable)
    }
}

/// One classified memory region.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub base: u64,
    pub length: u64,
    pub kind: Kind,
}

impl Region {
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.length)
    }

    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.end()
    }

    /// Does this region overlap another?
    pub fn overlaps(&self, other: &Region) -> bool {
        self.base < other.end() && other.base < self.end()
    }
}

/// Aggregate view of the whole memory map (F015).
#[derive(Clone, Copy, Debug, Default)]
pub struct Summary {
    pub entries: usize,
    pub usable_bytes: u64,
    pub reserved_bytes: u64,
    pub acpi_bytes: u64,
    pub bootloader_bytes: u64,
    pub bad_bytes: u64,
    pub largest_usable: u64,
    pub physical_max: u64,
}

/// Classify a slice of (base, length, kind) regions.
pub fn summarize(regions: &[Region]) -> Summary {
    let mut s = Summary::default();
    for r in regions {
        s.entries += 1;
        if r.end() > s.physical_max {
            s.physical_max = r.end();
        }
        match r.kind {
            Kind::Usable => {
                s.usable_bytes += r.length;
                if r.length > s.largest_usable {
                    s.largest_usable = r.length;
                }
            }
            Kind::Reserved | Kind::ExecutableAndModules | Kind::Framebuffer => {
                s.reserved_bytes += r.length;
            }
            Kind::AcpiReclaimable | Kind::AcpiNvs => {
                s.acpi_bytes += r.length;
            }
            Kind::BootloaderReclaimable => {
                s.bootloader_bytes += r.length;
            }
            Kind::BadMemory => {
                s.bad_bytes += r.length;
            }
        }
    }
    s
}

/// Sanity rules every usable memory map must satisfy (F015 validation,
/// consumed by the boot self-test F025).
pub fn validate(regions: &[Region]) -> Result<(), &'static str> {
    if regions.is_empty() {
        return Err("memory map is empty");
    }
    let s = summarize(regions);
    if s.usable_bytes == 0 {
        return Err("no usable memory");
    }
    if s.largest_usable < 4 * 1024 * 1024 {
        return Err("largest usable region below 4 MiB");
    }
    // Region integrity: no zero-length entries, no overflowed ends.
    for r in regions {
        if r.length == 0 {
            return Err("zero-length region");
        }
        if r.end() == u64::MAX {
            return Err("region end overflows");
        }
    }
    Ok(())
}

/// Human-readable size (bytes → KiB/MiB/GiB) for the console.
pub fn format_size(bytes: u64, out: &mut [u8]) -> usize {
    let (div, unit) = if bytes >= 1024 * 1024 * 1024 {
        (1024u64 * 1024 * 1024, b'G')
    } else if bytes >= 1024 * 1024 {
        (1024u64 * 1024, b'M')
    } else if bytes >= 1024 {
        (1024u64, b'K')
    } else {
        (1u64, b'B')
    };
    let whole = bytes / div;
    let frac = if div > 1 { (bytes % div) * 10 / div } else { 0 };
    let mut n = 0usize;
    let mut buf = [0u8; 24];
    // whole part
    let mut digits = [0u8; 24];
    let mut w = 0usize;
    let mut v = whole;
    if v == 0 {
        digits[0] = b'0';
        w = 1;
    } else {
        while v > 0 && w < digits.len() {
            digits[w] = b'0' + (v % 10) as u8;
            v /= 10;
            w += 1;
        }
    }
    while w > 0 && n < out.len() {
        w -= 1;
        buf[n] = digits[w];
        n += 1;
    }
    if div > 1 && n + 2 < out.len() {
        buf[n] = b'.';
        buf[n + 1] = b'0' + frac as u8;
        n += 2;
    }
    if n < out.len() {
        buf[n] = unit;
        n += 1;
    }
    out[..n].copy_from_slice(&buf[..n]);
    n
}

// ---------------------------------------------------------------------------
// F016 — reservation registry
// ---------------------------------------------------------------------------

/// Maximum boot-time reservations tracked.
pub const MAX_RESERVATIONS: usize = 32;

/// Why a region is reserved — reported by the self-test / `info` log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Purpose {
    KernelImage,
    Framebuffer,
    AcpiTables,
    LogRing,
    Module,
    Other,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::KernelImage => "kernel-image",
            Purpose::Framebuffer => "framebuffer",
            Purpose::AcpiTables => "acpi-tables",
            Purpose::LogRing => "log-ring",
            Purpose::Module => "module",
            Purpose::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Reservation {
    pub base: u64,
    pub length: u64,
    pub purpose: Purpose,
    /// Denied duplicate/overlap claims remember why they failed.
    pub comment: &'static str,
}

impl Reservation {
    /// View the reservation as a (reserved) region for geometry checks.
    pub fn as_region(&self) -> Region {
        Region { base: self.base, length: self.length, kind: Kind::Reserved }
    }
}

/// Fixed-capacity reservation registry with overlap + duplicate protection.
pub struct Reservations {
    items: UnsafeCell<[Option<Reservation>; MAX_RESERVATIONS]>,
    len: AtomicUsize,
}

// SAFETY: written only from the single-threaded boot path.
unsafe impl Sync for Reservations {}

impl Reservations {
    pub const fn new() -> Reservations {
        Reservations {
            items: UnsafeCell::new([None; MAX_RESERVATIONS]),
            len: AtomicUsize::new(0),
        }
    }

    pub fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Claim a region. Fails (with the reason) on: registry full,
    /// duplicate base, or overlap with an existing reservation.
    /// Zero-length claims always fail (`empty`).
    pub fn claim(
        &self,
        base: u64,
        length: u64,
        purpose: Purpose,
        comment: &'static str,
    ) -> Result<(), &'static str> {
        if length == 0 {
            return Err("empty");
        }
        let len = self.len.load(Ordering::Relaxed);
        if len >= MAX_RESERVATIONS {
            return Err("full");
        }
        let new_region = Region { base, length, kind: Kind::Reserved };
        unsafe {
            let items = &mut *self.items.get();
            for i in 0..len {
                if let Some(existing) = items[i] {
                    if existing.base == base {
                        return Err("duplicate");
                    }
                    if existing.as_region().overlaps(&new_region) {
                        return Err("overlap");
                    }
                }
            }
            items[len] = Some(Reservation { base, length, purpose, comment });
        }
        self.len.store(len + 1, Ordering::Relaxed);
        Ok(())
    }

    /// Is an address inside any reservation?
    pub fn contains(&self, addr: u64) -> bool {
        let len = self.len.load(Ordering::Relaxed);
        unsafe {
            let items = &*self.items.get();
            (0..len).any(|i| {
                items[i]
                    .map(|r| addr >= r.base && addr < r.base.saturating_add(r.length))
                    .unwrap_or(false)
            })
        }
    }

    /// Total reserved bytes.
    pub fn reserved_bytes(&self) -> u64 {
        let len = self.len.load(Ordering::Relaxed);
        unsafe {
            let items = &*self.items.get();
            (0..len)
                .map(|i| items[i].map(|r| r.length).unwrap_or(0))
                .sum()
        }
    }
}

// Per-index accessor is the allocation-free API surface.
impl Reservations {
    pub fn get(&self, index: usize) -> Option<Reservation> {
        let len = self.len.load(Ordering::Relaxed);
        if index >= len {
            return None;
        }
        unsafe { (*self.items.get())[index] }
    }
}

// ---------------------------------------------------------------------------
// Target adapter — read the Limine memory map
// ---------------------------------------------------------------------------

static RESERVATIONS: Reservations = Reservations::new();

/// Registry of boot-time reservations (F016).
pub fn reservations() -> &'static Reservations {
    &RESERVATIONS
}

/// Maximum entries we accept from the bootloader (defensive bound).
pub const MAX_ENTRIES: usize = 512;

/// Snapshot of the Limine memory map as classified regions.
#[derive(Clone, Copy, Debug)]
pub struct MemMapState {
    pub entry_count: usize,
    pub usable_bytes: u64,
    pub reserved_bytes: u64,
    pub largest_usable: u64,
}

/// Read + classify the Limine memory map (F015 target path).
pub fn init() -> Option<MemMapState> {
    let entries = crate::limine::memmap()?;
    let mut regions = [Region { base: 0, length: 0, kind: Kind::Reserved }; MAX_ENTRIES];
    let mut count = 0usize;
    for e in entries.iter() {
        if count >= MAX_ENTRIES {
            break;
        }
        if let Some(kind) = Kind::from_limine(e.kind) {
            regions[count] = Region { base: e.base, length: e.length, kind };
            count += 1;
        }
    }
    let s = summarize(&regions[..count]);
    let state = MemMapState {
        entry_count: count,
        usable_bytes: s.usable_bytes,
        reserved_bytes: s.reserved_bytes + s.acpi_bytes,
        largest_usable: s.largest_usable,
    };
    MEMMAP.set(state).ok();
    Some(state)
}

static MEMMAP: crate::once::OnceLock<MemMapState> = crate::once::OnceLock::new();

pub fn state() -> Option<&'static MemMapState> {
    MEMMAP.get()
}

/// Register the standard boot reservations (F016): kernel image, framebuffer,
/// ACPI table range, log ring. Best-effort — each claim is logged on failure.
pub fn register_boot_reservations() {
    // Kernel image: executable address + size from the bootloader.
    if let (Some((phys, _virt)), Some(file)) =
        (crate::limine::executable_address(), crate::limine::executable_file())
    {
        if phys != 0 && file.size > 0 {
            if let Err(e) = RESERVATIONS.claim(phys, file.size, Purpose::KernelImage, "varix") {
                crate::kwarn!("reserve kernel-image failed: {}", e);
            }
        }
    }
    // Framebuffer: its backing memory must never be allocated.
    if let Some(fb) = crate::limine::framebuffer() {
        let bytes = (fb.pitch as u64) * (fb.height as u64);
        if bytes > 0 {
            if let Err(e) = RESERVATIONS.claim(
                fb.address as u64,
                bytes,
                Purpose::Framebuffer,
                "gop",
            ) {
                crate::kwarn!("reserve framebuffer failed: {}", e);
            }
        }
    }
    // ACPI tables: reserve the ACPI reclaimable + NVS ranges verbatim.
    if let Some(entries) = crate::limine::memmap() {
        for e in entries.iter() {
            if Kind::from_limine(e.kind) == Some(Kind::AcpiReclaimable)
                || Kind::from_limine(e.kind) == Some(Kind::AcpiNvs)
            {
                if RESERVATIONS.len() >= MAX_RESERVATIONS {
                    break;
                }
                let _ = RESERVATIONS.claim(
                    e.base,
                    e.length,
                    Purpose::AcpiTables,
                    "firmware",
                );
            }
        }
    }
    // Log ring (F007): static in the kernel image, no extra claim needed —
    // it is already inside the kernel image reservation.
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn r(base: u64, length: u64, kind: Kind) -> Region {
        Region { base, length, kind }
    }

    #[test]
    fn kind_mapping_roundtrip() {
        for k in 0..8u64 {
            let parsed = Kind::from_limine(k).expect("0..8 all mapped");
            assert!(parsed.as_str().len() > 0);
        }
        assert!(Kind::from_limine(8).is_none());
        assert!(Kind::from_limine(u64::MAX).is_none());
        assert!(Kind::Usable.is_allocatable());
        assert!(!Kind::Framebuffer.is_allocatable());
        assert!(!Kind::AcpiReclaimable.is_allocatable());
    }

    #[test]
    fn region_geometry() {
        let a = r(0x1000, 0x100, Kind::Usable);
        assert_eq!(a.end(), 0x1100);
        assert!(a.contains(0x1000));
        assert!(a.contains(0x10FF));
        assert!(!a.contains(0x1100));
        assert!(!a.contains(0xFFF));
        let b = r(0x2000, 0x100, Kind::Reserved);
        assert!(!a.overlaps(&b));
        let c = r(0x10F0, 0x20, Kind::Reserved);
        assert!(a.overlaps(&c));
        assert!(c.overlaps(&a)); // symmetric
    }

    #[test]
    fn summarize_mixed_map() {
        let regions = [
            r(0x0, 0x1000, Kind::Reserved),          // IVT/BIOS
            r(0x1000, 0x9F000, Kind::Usable),         // ~640K low memory
            r(0x9F000, 0x1000, Kind::AcpiReclaimable),
            r(0x100000, 0x3F00000, Kind::Usable),    // 63M
            r(0x4000000, 0x10000, Kind::Framebuffer),
            r(0xFEC00000, 0x1000, Kind::Reserved),    // IO APIC
        ];
        let s = summarize(&regions);
        assert_eq!(s.entries, 6);
        assert_eq!(s.usable_bytes, 0x9F000 + 0x3F00000);
        assert_eq!(s.largest_usable, 0x3F00000);
        assert_eq!(s.acpi_bytes, 0x1000);
        assert_eq!(s.reserved_bytes, 0x1000 + 0x10000 + 0x1000);
        assert_eq!(s.physical_max, 0xFEC01000);
        assert_eq!(s.bootloader_bytes, 0);
        assert_eq!(s.bad_bytes, 0);
    }

    #[test]
    fn validation_rules() {
        let good = [r(0x1000, 0x2000000, Kind::Usable)];
        assert!(validate(&good).is_ok());
        assert!(validate(&[]).is_err());
        let no_usable = [r(0x1000, 0x100, Kind::Reserved)];
        assert_eq!(validate(&no_usable), Err("no usable memory"));
        let tiny = [r(0x1000, 0x100000, Kind::Usable)];
        assert_eq!(validate(&tiny), Err("largest usable region below 4 MiB"));
        let zero_len = [r(0x1000, 0x2000000, Kind::Usable), r(0x9000, 0, Kind::Reserved)];
        assert_eq!(validate(&zero_len), Err("zero-length region"));
    }

    #[test]
    fn format_size_units() {
        let mut out = [0u8; 24];
        let n = format_size(0, &mut out);
        assert_eq!(&out[..n], b"0B");
        let n = format_size(1234, &mut out);
        assert_eq!(&out[..n], b"1.2K");
        let n = format_size(5 * 1024 * 1024, &mut out);
        assert_eq!(&out[..n], b"5.0M");
        let n = format_size(0x1_0000_0000, &mut out);
        assert_eq!(&out[..n], b"4.0G");
        let n = format_size(999, &mut out);
        assert_eq!(&out[..n], b"999B");
    }

    #[test]
    fn reservations_lifecycle() {
        let reg = Reservations::new();
        assert!(reg.is_empty());
        assert!(reg.claim(0x1000, 0x100, Purpose::KernelImage, "k").is_ok());
        assert!(reg.claim(0x2000, 0x100, Purpose::Framebuffer, "fb").is_ok());
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.reserved_bytes(), 0x200);
        // duplicate base rejected
        assert_eq!(reg.claim(0x1000, 0x50, Purpose::Module, "d"), Err("duplicate"));
        // overlap rejected
        assert_eq!(reg.claim(0x1080, 0x80, Purpose::Other, "o"), Err("overlap"));
        // zero length rejected
        assert_eq!(reg.claim(0x3000, 0, Purpose::Other, "z"), Err("empty"));
        assert!(reg.contains(0x1000));
        assert!(reg.contains(0x10FF));
        assert!(!reg.contains(0x1100));
        assert!(!reg.contains(0x1FFF));
        assert!(reg.contains(0x2050));
        assert_eq!(reg.get(0).unwrap().purpose, Purpose::KernelImage);
        assert!(reg.get(2).is_none());
    }

    #[test]
    fn reservations_fill_to_capacity() {
        let reg = Reservations::new();
        for i in 0..MAX_RESERVATIONS {
            assert!(reg.claim(0x1000 + (i as u64) * 0x1000, 0x800, Purpose::Module, "m").is_ok());
        }
        assert_eq!(
            reg.claim(0xFF_F000, 0x800, Purpose::Other, "x"),
            Err("full")
        );
        assert_eq!(reg.len(), MAX_RESERVATIONS);
        assert_eq!(reg.reserved_bytes(), MAX_RESERVATIONS as u64 * 0x800);
    }
}
