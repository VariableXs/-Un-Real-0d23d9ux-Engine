//! F026 GDT 装载 / F027 TSS 管理 — the descriptor tables the CPU needs before
//! anything else can run: a flat 64-bit GDT plus one TSS holding ring-0 stacks
//! and the IST slots that keep #DF / #MC / #NMI survivable.
//!
//! Design notes:
//! * The GDT is built as plain `u64`s (the exact wire format) so the encoding
//!   is unit-testable on the host with no `asm!` involved.
//! * The TSS is a byte image with explicit offsets — `#[repr(packed)]` structs
//!   make every field access unaligned, and the kernel wants zero unaligned
//!   reads in the double-fault path.

/// Number of 8-byte descriptors in the GDT (TSS occupies two slots).
pub const GDT_ENTRIES: usize = 7;

pub const SEL_NULL: u16 = 0x00;
pub const SEL_KERNEL_CODE: u16 = 0x08;
pub const SEL_KERNEL_DATA: u16 = 0x10;
pub const SEL_USER_DATA: u16 = 0x18;
pub const SEL_USER_CODE: u16 = 0x20;
pub const SEL_TSS: u16 = 0x28;

/// Access byte bits.
const ACC_PRESENT: u8 = 0x80;
const ACC_USER_DPL: u8 = 0x60; // DPL 3
const ACC_SYSTEM: u8 = 0x00; // S=0 → system descriptor (TSS/LDT)
const ACC_SEGMENT: u8 = 0x10; // S=1 → code/data
const ACC_EXEC: u8 = 0x08;
const ACC_RW: u8 = 0x02;
const ACC_ACCESSED: u8 = 0x01;

/// Flag nibble (limit 19:16 | AVL | L | D/B | G).
const FLAG_GRAN: u8 = 0x08;
const FLAG_LONG: u8 = 0x02;
const FLAG_DB: u8 = 0x04;

/// TSS byte image size (x86_64 TSS, without an IO port bitmap).
pub const TSS_SIZE: usize = 104;
/// Byte offsets inside the TSS image.
pub const TSS_OFF_RSP0: usize = 4;
pub const TSS_OFF_RSP1: usize = 12;
pub const TSS_OFF_RSP2: usize = 20;
pub const TSS_OFF_IST1: usize = 36;
/// IST slots are 1..=7 (0 means "unused").
pub const IST_MAX: usize = 7;

/// IST index reserved for the double-fault handler (F027).
pub const IST_DOUBLE_FAULT: usize = 1;
/// IST index reserved for the machine-check handler.
pub const IST_MACHINE_CHECK: usize = 2;
/// IST index reserved for NMI.
pub const IST_NMI: usize = 3;

/// Size of each IST stack. 16 KiB survives a nested dump without blowing the
/// reserved-memory budget.
pub const IST_STACK_SIZE: usize = 16 * 1024;

/// Build a legacy 8-byte descriptor from (base, limit, access, flags).
const fn descriptor(base: u32, limit: u32, access: u8, flags: u8) -> u64 {
    let mut v = 0u64;
    v |= (limit as u64) & 0xFFFF;
    v |= ((base as u64) & 0xFFFF) << 16;
    v |= (((base >> 16) as u64) & 0xFF) << 32;
    v |= (access as u64) << 40;
    v |= (((limit >> 16) as u64) & 0xF) << 48;
    v |= ((flags as u64) & 0xF) << 52;
    v |= (((base >> 24) as u64) & 0xFF) << 56;
    v
}

/// Ring-0 64-bit code descriptor.
pub const fn kernel_code() -> u64 {
    descriptor(
        0,
        0xFFFF,
        ACC_PRESENT | ACC_SEGMENT | ACC_EXEC | ACC_RW,
        FLAG_GRAN | FLAG_LONG,
    )
}

/// Ring-0 data descriptor (64-bit mode ignores most of it).
pub const fn kernel_data() -> u64 {
    descriptor(
        0,
        0xFFFF,
        ACC_PRESENT | ACC_SEGMENT | ACC_RW,
        FLAG_GRAN | FLAG_DB,
    )
}

/// Ring-3 data descriptor (stack segment for user threads).
pub const fn user_data() -> u64 {
    descriptor(
        0,
        0xFFFF,
        ACC_PRESENT | ACC_USER_DPL | ACC_SEGMENT | ACC_RW,
        FLAG_GRAN | FLAG_DB,
    )
}

/// Ring-3 64-bit code descriptor (F101 needs it; declared here, owned here).
pub const fn user_code() -> u64 {
    descriptor(
        0,
        0xFFFF,
        ACC_PRESENT | ACC_USER_DPL | ACC_SEGMENT | ACC_EXEC | ACC_RW,
        FLAG_GRAN | FLAG_LONG,
    )
}

/// The low half of the 16-byte TSS system descriptor.
pub fn tss_low(base: u64, limit: u32) -> u64 {
    descriptor(
        base as u32,
        limit,
        ACC_PRESENT | ACC_SYSTEM | ACC_EXEC | ACC_ACCESSED, // type 0x9: avail 64-bit TSS
        0,
    )
}

/// The high half of the 16-byte TSS system descriptor (base 63:32).
pub fn tss_high(base: u64) -> u64 {
    (base >> 32) & 0xFFFF_FFFF
}

/// The GDT image plus its LGDT descriptor.
pub struct Gdt {
    entries: [u64; GDT_ENTRIES],
    installed: bool,
}

impl Gdt {
    pub const fn new() -> Gdt {
        Gdt {
            entries: [
                0, // null (architecturally required)
                kernel_code(),
                kernel_data(),
                user_data(),
                user_code(),
                0, // TSS low, filled by `set_tss`
                0, // TSS high
            ],
            installed: false,
        }
    }

    /// Install the TSS descriptor at the fixed `SEL_TSS` slot.
    pub fn set_tss(&mut self, base: u64, limit: u32) {
        self.entries[5] = tss_low(base, limit);
        self.entries[6] = tss_high(base);
    }

    pub fn entries(&self) -> &[u64; GDT_ENTRIES] {
        &self.entries
    }

    pub fn installed(&self) -> bool {
        self.installed
    }

    /// LGDT pseudo-descriptor: (limit, base).
    pub fn descriptor(&self) -> (u16, u64) {
        let limit = (GDT_ENTRIES * 8 - 1) as u16;
        let base = self.entries.as_ptr() as u64;
        (limit, base)
    }

    /// Load the table and reload the segment registers (F026).
    ///
    /// SAFETY: caller must guarantee a valid stack and that no other core is
    /// running this code path.
    pub unsafe fn install(&mut self) {
        let (limit, base) = self.descriptor();
        load_gdt(limit, base);
        reload_segments();
        self.installed = true;
        crate::kinfo!("gdt: {} entries loaded", GDT_ENTRIES);
    }

    /// Load the task register with `SEL_TSS` (F027).
    ///
    /// SAFETY: the TSS descriptor must already be present in the GDT.
    pub unsafe fn load_tr(&self) {
        load_tr(SEL_TSS);
        crate::kinfo!("tss: TR=0x{:x}", SEL_TSS);
    }
}

impl Default for Gdt {
    fn default() -> Gdt {
        Gdt::new()
    }
}

// ---------------------------------------------------------------------------
// TSS (F027)
// ---------------------------------------------------------------------------

/// A byte-exact x86_64 TSS image: three ring-0 stacks, seven IST slots.
pub struct Tss {
    raw: [u8; TSS_SIZE],
}

impl Tss {
    pub const fn new() -> Tss {
        Tss {
            raw: [0u8; TSS_SIZE],
        }
    }

    fn write_u64(&mut self, off: usize, v: u64) {
        self.raw[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }

    fn read_u64(&self, off: usize) -> u64 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&self.raw[off..off + 8]);
        u64::from_le_bytes(b)
    }

    fn write_u16(&mut self, off: usize, v: u16) {
        self.raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }

    pub fn raw(&self) -> &[u8; TSS_SIZE] {
        &self.raw
    }

    pub fn set_rsp0(&mut self, v: u64) {
        self.write_u64(TSS_OFF_RSP0, v);
    }
    pub fn rsp0(&self) -> u64 {
        self.read_u64(TSS_OFF_RSP0)
    }
    pub fn set_rsp1(&mut self, v: u64) {
        self.write_u64(TSS_OFF_RSP1, v);
    }
    pub fn set_rsp2(&mut self, v: u64) {
        self.write_u64(TSS_OFF_RSP2, v);
    }

    /// Byte offset of IST slot `i` (1..=7), or `None` when out of range.
    pub const fn ist_offset(i: usize) -> Option<usize> {
        if i == 0 || i > IST_MAX {
            None
        } else {
            Some(TSS_OFF_IST1 + (i - 1) * 8)
        }
    }

    /// Point IST slot `i` at the *top* of its stack. Returns `Err(())` for an
    /// invalid slot so a bad constant can never silently disable the guard.
    pub fn set_ist(&mut self, i: usize, top: u64) -> Result<(), ()> {
        match Self::ist_offset(i) {
            Some(off) => {
                self.write_u64(off, top);
                Ok(())
            }
            None => Err(()),
        }
    }

    pub fn ist(&self, i: usize) -> Option<u64> {
        Self::ist_offset(i).map(|off| self.read_u64(off))
    }

    /// No IO port bitmap: an offset past the TSS size disables it.
    pub fn disable_iopb(&mut self) {
        self.write_u16(TSS_SIZE - 2, TSS_SIZE as u16);
    }

    pub fn iopb(&self) -> u16 {
        u16::from_le_bytes([self.raw[TSS_SIZE - 2], self.raw[TSS_SIZE - 1]])
    }
}

impl Default for Tss {
    fn default() -> Tss {
        Tss::new()
    }
}

// ---------------------------------------------------------------------------
// Globals + bring-up
// ---------------------------------------------------------------------------

use core::cell::UnsafeCell;

/// SAFETY: written once from the single-threaded boot path before any other
/// core is started; afterwards read-only.
unsafe impl Sync for GdtTables {}

/// The live GDT + TSS + IST stacks, all in one cache-friendly object.
pub struct GdtTables {
    gdt: UnsafeCell<Gdt>,
    tss: UnsafeCell<Tss>,
    stacks: UnsafeCell<[[u8; IST_STACK_SIZE]; IST_MAX]>,
    ready: core::sync::atomic::AtomicBool,
}

impl GdtTables {
    const ZERO_STACK: [u8; IST_STACK_SIZE] = [0u8; IST_STACK_SIZE];

    pub const fn new() -> GdtTables {
        GdtTables {
            gdt: UnsafeCell::new(Gdt::new()),
            tss: UnsafeCell::new(Tss::new()),
            stacks: UnsafeCell::new([Self::ZERO_STACK; IST_MAX]),
            ready: core::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(core::sync::atomic::Ordering::Acquire)
    }

    /// LGDT pseudo-descriptor of the live table (F041 needs it for the APs).
    pub fn descriptor(&self) -> (u16, u64) {
        // SAFETY: read-only view of a table that is never replaced.
        unsafe { (*self.gdt.get()).descriptor() }
    }

    pub fn is_installed(&self) -> bool {
        // SAFETY: see `descriptor`.
        unsafe { (*self.gdt.get()).installed() }
    }

    /// # Safety
    /// Single-threaded boot path only; hands out `&mut` to a `static`.
    unsafe fn gdt_mut(&self) -> &mut Gdt {
        &mut *self.gdt.get()
    }
    unsafe fn tss_mut(&self) -> &mut Tss {
        &mut *self.tss.get()
    }
    unsafe fn stacks_ref(&self) -> &[[u8; IST_STACK_SIZE]; IST_MAX] {
        &*self.stacks.get()
    }
}

static TABLES: GdtTables = GdtTables::new();

pub fn tables() -> &'static GdtTables {
    &TABLES
}

/// F026 + F027 bring-up: build the TSS, wire the IST stacks, install.
///
/// Returns `true` when both the GDT and the task register are live. On the host
/// (test) target the `asm!`-backed steps are no-ops, so the function still
/// exercises every pure computation.
pub fn init() -> bool {
    if TABLES.is_ready() {
        return true;
    }
    let mut ok = true;

    let tss_base = unsafe {
        let tss = TABLES.tss_mut();
        // IST stacks grow down: hand the TSS the *high* address.
        let stacks = TABLES.stacks_ref();
        let _ = tss.set_ist(IST_DOUBLE_FAULT, stack_top(&stacks[IST_DOUBLE_FAULT - 1]));
        let _ = tss.set_ist(IST_MACHINE_CHECK, stack_top(&stacks[IST_MACHINE_CHECK - 1]));
        let _ = tss.set_ist(IST_NMI, stack_top(&stacks[IST_NMI - 1]));
        tss.disable_iopb();
        ok &= tss.ist(IST_DOUBLE_FAULT).is_some();
        tss.raw().as_ptr() as u64
    };

    let gdt_ok = unsafe {
        let gdt = TABLES.gdt_mut();
        gdt.set_tss(tss_base, (TSS_SIZE - 1) as u32);
        gdt.install();
        gdt.load_tr();
        gdt.installed()
    };
    ok &= gdt_ok;

    TABLES.ready.store(true, core::sync::atomic::Ordering::Release);
    ok
}

/// Top address (exclusive) of a stack image.
fn stack_top(s: &[u8; IST_STACK_SIZE]) -> u64 {
    s.as_ptr() as u64 + IST_STACK_SIZE as u64
}

// ---------------------------------------------------------------------------
// Architecture glue — real instructions on the kernel target, inert elsewhere.
// ---------------------------------------------------------------------------

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
unsafe fn load_gdt(limit: u16, base: u64) {
    let ptr: [u16; 5] = [limit, base as u16, (base >> 16) as u16, (base >> 32) as u16, (base >> 48) as u16];
    core::arch::asm!("lgdt [{0}]", in(reg) ptr.as_ptr(), options(readonly, nostack, preserves_flags));
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
unsafe fn load_gdt(_limit: u16, _base: u64) {}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
unsafe fn reload_segments() {
    core::arch::asm!(
        "mov ds, {0:x}",
        "mov es, {0:x}",
        "mov fs, {0:x}",
        "mov gs, {0:x}",
        "mov ss, {0:x}",
        in(reg) SEL_KERNEL_DATA,
        options(nostack, preserves_flags)
    );
    // Far return reloads CS with the ring-0 code selector.
    core::arch::asm!(
        "push {sel:x}",
        "lea {tmp}, [rip + 2f]",
        "push {tmp}",
        "retfq",
        "2:",
        sel = in(reg) SEL_KERNEL_CODE,
        tmp = lateout(reg) _,
        options(preserves_flags)
    );
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
unsafe fn reload_segments() {}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
unsafe fn load_tr(sel: u16) {
    core::arch::asm!("ltr {0:x}", in(reg) sel, options(nostack, preserves_flags));
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
unsafe fn load_tr(_sel: u16) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_descriptor_is_64bit_and_ring0() {
        let e = kernel_code();
        assert_eq!(e >> 40 & 0xFF, 0x9A, "access byte");
        assert_eq!(e >> 52 & 0xF, 0xA, "flags: G+L");
        assert_eq!(e & 0xFFFF, 0xFFFF, "limit low");
    }

    #[test]
    fn user_descriptors_are_dpl3() {
        assert_eq!(user_code() >> 40 & 0xFF, 0xFA);
        assert_eq!(user_data() >> 40 & 0xFF, 0xF2);
        assert_eq!(kernel_data() >> 40 & 0xFF, 0x92);
    }

    #[test]
    fn selectors_are_eight_bytes_apart() {
        assert_eq!(SEL_KERNEL_CODE, 0x08);
        assert_eq!(SEL_KERNEL_DATA, 0x10);
        assert_eq!(SEL_USER_DATA, 0x18);
        assert_eq!(SEL_USER_CODE, 0x20);
        assert_eq!(SEL_TSS, 0x28);
    }

    #[test]
    fn tss_descriptor_round_trips_base() {
        let base: u64 = 0xFFFF_8000_1234_5000;
        assert_eq!(tss_high(base), 0xFFFF_8000);
        let low = tss_low(base, 103);
        assert_eq!(low >> 40 & 0xFF, 0x89, "type 0x9 + present");
        assert_eq!(low & 0xFFFF, 103, "limit");
    }

    #[test]
    fn tss_image_offsets_and_stacks() {
        let mut t = Tss::new();
        assert_eq!(TSS_SIZE, 104);
        t.set_rsp0(0xDEAD_BEEF);
        assert_eq!(t.rsp0(), 0xDEAD_BEEF);
        assert!(t.set_ist(0, 1).is_err(), "IST0 is reserved");
        assert!(t.set_ist(8, 1).is_err(), "IST8 does not exist");
        assert!(t.set_ist(IST_DOUBLE_FAULT, 0x1234_5678).is_ok());
        assert_eq!(t.ist(IST_DOUBLE_FAULT), Some(0x1234_5678));
        assert_eq!(Tss::ist_offset(7), Some(36 + 6 * 8));
        t.disable_iopb();
        assert_eq!(t.iopb(), 104);
    }

    #[test]
    fn gdt_descriptor_limit_covers_all_entries() {
        let g = Gdt::new();
        let (limit, _base) = g.descriptor();
        assert_eq!(limit as usize, GDT_ENTRIES * 8 - 1);
        assert_eq!(g.entries().len(), GDT_ENTRIES);
        assert_eq!(g.entries()[1], kernel_code());
    }
}
