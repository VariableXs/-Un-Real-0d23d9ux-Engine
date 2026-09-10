//! F008 ACPI 表解析 — RSDP discovery + RSDT/XSDT traversal with checksum
//! verification. F009 MADT 设备枚举 — CPU local APIC / IO APIC / interrupt
//! source override lists for the SMP and IRQ layers (consumed by AI-02).
//!
//! All parsing is pure functions over byte slices; the target-side `init`
//! only feeds the Limine RSDP address in. Host tests build table images in
//! memory and walk them. Collections are fixed-capacity (no allocator here).

// ---------------------------------------------------------------------------
// SDT header — common to every ACPI table
// ---------------------------------------------------------------------------

pub const SDT_HEADER_LEN: usize = 36;

/// System Description Table header (first 36 bytes of every table).
#[derive(Clone, Copy, Debug)]
pub struct SdtHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
}

impl SdtHeader {
    pub fn parse(bytes: &[u8]) -> Option<SdtHeader> {
        if bytes.len() < SDT_HEADER_LEN {
            return None;
        }
        let mut signature = [0u8; 4];
        signature.copy_from_slice(&bytes[0..4]);
        let mut oem_id = [0u8; 6];
        oem_id.copy_from_slice(&bytes[10..16]);
        let mut oem_table_id = [0u8; 8];
        oem_table_id.copy_from_slice(&bytes[16..24]);
        Some(SdtHeader {
            signature,
            length: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            revision: bytes[8],
            checksum: bytes[9],
            oem_id,
            oem_table_id,
        })
    }

    pub fn signature_str(&self) -> &'static str {
        sig_str(&self.signature)
    }
}

/// Map a 4-byte signature to a str (for logging / matching).
pub fn sig_str(sig: &[u8; 4]) -> &'static str {
    match sig {
        b"APIC" => "APIC",
        b"RSDT" => "RSDT",
        b"XSDT" => "XSDT",
        b"FACP" => "FACP",
        b"HPET" => "HPET",
        b"MCFG" => "MCFG",
        _ => "????",
    }
}

/// Verify the ACPI checksum rule: all bytes of the table sum to 0 (mod 256).
pub fn checksum_ok(bytes: &[u8], length: usize) -> bool {
    if length == 0 || length > bytes.len() || length > 1 << 20 {
        return false;
    }
    let sum = bytes[..length].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    sum == 0
}

// ---------------------------------------------------------------------------
// F008 — RSDP (Root System Description Pointer)
// ---------------------------------------------------------------------------

pub const RSDP_V1_LEN: usize = 20;
pub const RSDP_V2_LEN: usize = 36;

/// Parsed Root System Description Pointer.
#[derive(Clone, Copy, Debug)]
pub struct Rsdp {
    pub revision: u8,
    /// RSDT address (32-bit field; 0 when XSDT is used).
    pub rsdt_address: u64,
    /// XSDT address for revision >= 2, else 0.
    pub xsdt_address: u64,
}

impl Rsdp {
    /// Parse + validate an RSDP from raw bytes.
    pub fn parse(bytes: &[u8]) -> Option<Rsdp> {
        if bytes.len() < RSDP_V1_LEN {
            return None;
        }
        if &bytes[0..8] != b"RSD PTR " {
            return None;
        }
        // Revision 0: 20-byte structure checksummed.
        if !checksum_ok(bytes, RSDP_V1_LEN) {
            return None;
        }
        let revision = bytes[15];
        let rsdt_address = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as u64;
        if revision == 0 {
            return Some(Rsdp { revision, rsdt_address, xsdt_address: 0 });
        }
        // Revision >= 2: extended (36-byte) checksum applies.
        if bytes.len() < RSDP_V2_LEN || !checksum_ok(bytes, RSDP_V2_LEN) {
            return None;
        }
        let xs = [
            bytes[24], bytes[25], bytes[26], bytes[27],
            bytes[28], bytes[29], bytes[30], bytes[31],
        ];
        let xsdt_address = u64::from_le_bytes(xs);
        let len = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) as usize;
        if len < RSDP_V2_LEN {
            return None;
        }
        Some(Rsdp {
            revision,
            rsdt_address,
            xsdt_address,
        })
    }

    /// Prefer XSDT when present (revision >= 2 and non-zero address).
    pub fn root_table(&self) -> Option<(u64, bool)> {
        if self.revision >= 2 && self.xsdt_address != 0 {
            Some((self.xsdt_address, true))
        } else if self.rsdt_address != 0 {
            Some((self.rsdt_address, false))
        } else {
            None
        }
    }
}

/// Maximum root-table children we register.
pub const MAX_TABLES: usize = 64;

/// Walk an RSDT/XSDT body and store child pointers into `out`.
/// `entry_bytes` is the table body AFTER the 36-byte header. `is_xsdt`
/// selects 64-bit vs 32-bit child pointers. Returns the count stored.
///
/// Pure — host tests call it directly; the target reader maps addresses.
pub fn root_table_entries(entry_bytes: &[u8], is_xsdt: bool, out: &mut [u64]) -> usize {
    let ptr_size = if is_xsdt { 8 } else { 4 };
    let mut count = 0usize;
    let mut off = 0usize;
    while off + ptr_size <= entry_bytes.len() && count < out.len() {
        let addr = if is_xsdt {
            let mut b = [0u8; 8];
            b.copy_from_slice(&entry_bytes[off..off + 8]);
            u64::from_le_bytes(b)
        } else {
            let mut b = [0u8; 4];
            b.copy_from_slice(&entry_bytes[off..off + 4]);
            u32::from_le_bytes(b) as u64
        };
        out[count] = addr;
        count += 1;
        off += ptr_size;
    }
    count
}

/// Read a child table's signature + full body from its containing image.
/// `image` is the memory the table lives in, `addr` an offset inside it
/// (tests) — the target layer computes the same offset via HHDM.
pub fn table_at<'a>(image: &'a [u8], addr: u64) -> Option<(&'a [u8], SdtHeader)> {
    let off = addr as usize;
    let header = SdtHeader::parse(image.get(off..)?)?;
    let end = off.checked_add(header.length as usize)?;
    let body = image.get(off..end)?;
    Some((body, header))
}

// ---------------------------------------------------------------------------
// F009 — MADT (Multiple APIC Description Table)
// ---------------------------------------------------------------------------

pub mod madt_entry {
    pub const PROCESSOR_LOCAL_APIC: u8 = 0;
    pub const IO_APIC: u8 = 1;
    pub const INTERRUPT_SOURCE_OVERRIDE: u8 = 2;
    pub const NMI: u8 = 3;
    pub const LOCAL_APIC_ADDRESS_OVERRIDE: u8 = 5;
    pub const PROCESSOR_LOCAL_X2APIC: u8 = 9;
}

/// Maximum cores recorded from the MADT (x2APIC class machines).
pub const MAX_CPUS: usize = 256;
pub const MAX_IOAPICS: usize = 8;
pub const MAX_ISOS: usize = 24;

/// One processor core as described by the MADT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuEntry {
    pub acpi_id: u32,
    pub apic_id: u32,
    /// Enabled flag from the MADT entry (online-able).
    pub enabled: bool,
}

/// One IO APIC as described by the MADT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoApicEntry {
    pub id: u8,
    pub address: u32,
    /// First GSI served by this IO APIC.
    pub gsi_base: u32,
}

/// Interrupt Source Override (bus IRQ → GSI remap).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsoEntry {
    pub bus_source_irq: u8,
    pub gsi: u32,
    /// Flags: polarity bits 0-1, trigger mode bits 2-3.
    pub flags: u16,
}

/// Result of the full MADT walk (F009) — fixed capacity, no allocator.
#[derive(Clone, Debug)]
pub struct MadtInfo {
    pub cpus: [CpuEntry; MAX_CPUS],
    pub cpu_count: usize,
    pub ioapics: [IoApicEntry; MAX_IOAPICS],
    pub ioapic_count: usize,
    pub isos: [IsoEntry; MAX_ISOS],
    pub iso_count: usize,
    /// Local APIC MMIO base (possibly overridden by entry type 5).
    pub local_apic_address: u32,
    /// PC-AT-compatible dual-8259 PIC present.
    pub has_8259: bool,
}

impl Default for MadtInfo {
    fn default() -> MadtInfo {
        MadtInfo {
            cpus: [CpuEntry { acpi_id: 0, apic_id: 0, enabled: false }; MAX_CPUS],
            cpu_count: 0,
            ioapics: [IoApicEntry { id: 0, address: 0, gsi_base: 0 }; MAX_IOAPICS],
            ioapic_count: 0,
            isos: [IsoEntry { bus_source_irq: 0, gsi: 0, flags: 0 }; MAX_ISOS],
            iso_count: 0,
            local_apic_address: 0,
            has_8259: false,
        }
    }
}

impl MadtInfo {
    pub fn cpus(&self) -> &[CpuEntry] {
        &self.cpus[..self.cpu_count]
    }

    pub fn ioapics(&self) -> &[IoApicEntry] {
        &self.ioapics[..self.ioapic_count]
    }

    pub fn isos(&self) -> &[IsoEntry] {
        &self.isos[..self.iso_count]
    }

    /// Enabled CPU count (what AI-02's SMP bring-up will start).
    pub fn enabled_cpus(&self) -> usize {
        self.cpus().iter().filter(|c| c.enabled).count()
    }

    /// Check the classic QEMU IRQ0 timer override (IRQ0 → GSI 2).
    pub fn timer_iso_present(&self) -> bool {
        self.isos().iter().any(|i| i.bus_source_irq == 0 && i.gsi == 2)
    }
}

/// Parse a full MADT table body (header included).
pub fn parse_madt(table: &[u8]) -> Option<MadtInfo> {
    let header = SdtHeader::parse(table)?;
    if &header.signature != b"APIC" {
        return None;
    }
    let len = header.length as usize;
    if len < SDT_HEADER_LEN + 8 || len > table.len() {
        return None;
    }
    let mut info = MadtInfo {
        local_apic_address: u32::from_le_bytes([
            table[36], table[37], table[38], table[39],
        ]),
        // MADT flags field: offset 40, bit 0 = PC-AT-compatible dual 8259.
        has_8259: table[40] & 1 == 1,
        ..MadtInfo::default()
    };

    // Entry stream starts at byte 44.
    let mut off = 44usize;
    while off + 2 <= len {
        let entry_type = table[off];
        let entry_len = table[off + 1] as usize;
        if entry_len < 2 || off + entry_len > len {
            break; // malformed stream — keep what we have
        }
        let e = &table[off..off + entry_len];
        match entry_type {
            madt_entry::PROCESSOR_LOCAL_APIC if entry_len >= 8 => {
                if info.cpu_count < MAX_CPUS {
                    info.cpus[info.cpu_count] = CpuEntry {
                        acpi_id: e[2] as u32,
                        apic_id: e[3] as u32,
                        enabled: e[4] & 1 == 1,
                    };
                    info.cpu_count += 1;
                }
            }
            madt_entry::IO_APIC if entry_len >= 12 => {
                if info.ioapic_count < MAX_IOAPICS {
                    let mut addr = [0u8; 4];
                    addr.copy_from_slice(&e[4..8]);
                    let mut gsi = [0u8; 4];
                    gsi.copy_from_slice(&e[8..12]);
                    info.ioapics[info.ioapic_count] = IoApicEntry {
                        id: e[2],
                        address: u32::from_le_bytes(addr),
                        gsi_base: u32::from_le_bytes(gsi),
                    };
                    info.ioapic_count += 1;
                }
            }
            madt_entry::INTERRUPT_SOURCE_OVERRIDE if entry_len >= 10 => {
                if info.iso_count < MAX_ISOS {
                    let mut gsi = [0u8; 4];
                    gsi.copy_from_slice(&e[4..8]);
                    let mut flags = [0u8; 2];
                    flags.copy_from_slice(&e[8..10]);
                    info.isos[info.iso_count] = IsoEntry {
                        bus_source_irq: e[3],
                        gsi: u32::from_le_bytes(gsi),
                        flags: u16::from_le_bytes(flags),
                    };
                    info.iso_count += 1;
                }
            }
            madt_entry::LOCAL_APIC_ADDRESS_OVERRIDE if entry_len >= 12 => {
                let mut b = [0u8; 8];
                b.copy_from_slice(&e[4..12]);
                let addr = u64::from_le_bytes(b);
                if addr <= u32::MAX as u64 {
                    info.local_apic_address = addr as u32;
                }
            }
            madt_entry::PROCESSOR_LOCAL_X2APIC if entry_len >= 16 => {
                if info.cpu_count < MAX_CPUS {
                    let mut uid = [0u8; 4];
                    uid.copy_from_slice(&e[4..8]);
                    let mut apic = [0u8; 4];
                    apic.copy_from_slice(&e[12..16]);
                    info.cpus[info.cpu_count] = CpuEntry {
                        acpi_id: u32::from_le_bytes(uid),
                        apic_id: u32::from_le_bytes(apic),
                        enabled: e[8] & 1 == 1,
                    };
                    info.cpu_count += 1;
                }
            }
            _ => {} // NMI / others: recorded by later domains
        }
        off += entry_len;
    }
    Some(info)
}

// ---------------------------------------------------------------------------
// Target-side init — reads the real tables through the Limine HHDM map
// ---------------------------------------------------------------------------

/// Discovered ACPI state (target boot path).
#[derive(Clone, Copy, Debug)]
pub struct AcpiState {
    pub revision: u8,
    /// Number of tables registered from the root table.
    pub table_count: usize,
    pub cpus: usize,
    pub enabled_cpus: usize,
    pub ioapics: usize,
    pub has_madt: bool,
}

use crate::once::OnceLock;

/// MADT discovery result for the IRQ/SMP domains (set by `init`).
static MADT: OnceLock<MadtInfo> = OnceLock::new();

pub fn madt() -> Option<&'static MadtInfo> {
    MADT.get()
}

/// Map a firmware (physical) address through the HHDM when needed.
fn hhdm_map(addr: u64, hhdm: u64) -> u64 {
    if addr >= hhdm && hhdm != 0 {
        addr
    } else {
        addr + hhdm
    }
}

pub fn init() -> Option<AcpiState> {
    let rsdp_addr = crate::limine::rsdp_address()?;
    let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
    unsafe {
        let virt = hhdm_map(rsdp_addr, hhdm);
        let bytes = core::slice::from_raw_parts(virt as *const u8, RSDP_V2_LEN.max(RSDP_V1_LEN));
        let rsdp = Rsdp::parse(bytes)?;
        let (root_addr, is_xsdt) = rsdp.root_table()?;
        let root_virt = hhdm_map(root_addr, hhdm);
        let root_hdr_bytes = core::slice::from_raw_parts(root_virt as *const u8, SDT_HEADER_LEN);
        let header = SdtHeader::parse(root_hdr_bytes)?;
        let root_len = (header.length as usize).min(1 << 18);
        let root_full = core::slice::from_raw_parts(root_virt as *const u8, root_len);
        if !checksum_ok(root_full, root_len) {
            return None;
        }
        let mut addrs = [0u64; MAX_TABLES];
        let count = root_table_entries(&root_full[SDT_HEADER_LEN..], is_xsdt, &mut addrs);

        let mut state = AcpiState {
            revision: rsdp.revision,
            table_count: 0,
            cpus: 0,
            enabled_cpus: 0,
            ioapics: 0,
            has_madt: false,
        };
        for &addr in addrs[..count].iter() {
            if addr == 0 {
                continue;
            }
            let t = hhdm_map(addr, hhdm);
            let hdr = SdtHeader::parse(core::slice::from_raw_parts(t as *const u8, SDT_HEADER_LEN))?;
            let tlen = (hdr.length as usize).min(1 << 18);
            let body = core::slice::from_raw_parts(t as *const u8, tlen);
            if !checksum_ok(body, tlen) {
                continue; // skip corrupt tables, keep walking
            }
            state.table_count += 1;
            if &hdr.signature == b"APIC" {
                if let Some(madt) = parse_madt(body) {
                    state.cpus = madt.cpu_count;
                    state.enabled_cpus = madt.enabled_cpus();
                    state.ioapics = madt.ioapic_count;
                    state.has_madt = true;
                    let _ = MADT.set(madt);
                }
            }
        }
        Some(state)
    }
}

// ---------------------------------------------------------------------------
// Tests — synthetic QEMU-style ACPI image
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn push_u16(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_le_bytes());
    }

    /// Build an SDT with header + body and fix the checksum.
    fn sdt(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(sig);
        let len = (SDT_HEADER_LEN + body.len()) as u32;
        t.extend_from_slice(&len.to_le_bytes());
        t.push(1); // revision
        t.push(0); // checksum placeholder
        t.extend_from_slice(b"VARIX "); // oem id (6)
        t.extend_from_slice(b"VARIXTBL"); // oem table id (8)
        t.extend_from_slice(&[0u8; 12]); // creator id/rev + reserved tail
        t.extend_from_slice(body);
        let sum: u8 = t.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        t[9] = 0u8.wrapping_sub(sum);
        t
    }

    fn rsdp(rsdt_addr: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"RSD PTR ");
        v.push(0); // checksum placeholder
        v.extend_from_slice(b"VARIX "); // oem id
        v.push(0); // revision 0
        push_u32(&mut v, rsdt_addr);
        // revision 0 RSDP = 20 bytes; extend to v2 size for the parse call
        v.extend_from_slice(&[0u8; 4]); // length (v2 field, unused at rev 0)
        v.extend_from_slice(&[0u8; 8]); // xsdt = 0
        v.extend_from_slice(&[0u8; 3]); // ext checksum + reserved
        // recompute 20-byte checksum for revision 0
        let sum: u8 = v[..RSDP_V1_LEN].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        v[8] = 0u8.wrapping_sub(sum);
        v
    }

    fn madt_body() -> Vec<u8> {
        let mut b = Vec::new();
        push_u32(&mut b, 0xFEE0_0000); // local APIC address
        b.push(1); // PC-AT compatible
        b.extend_from_slice(&[0u8; 3]);
        // CPU 0: enabled
        b.extend_from_slice(&[0, 8, 0, 0, 1, 0, 0, 0]);
        // CPU 1: enabled
        b.extend_from_slice(&[0, 8, 1, 1, 1, 0, 0, 0]);
        // CPU 2: disabled
        b.extend_from_slice(&[0, 8, 2, 2, 0, 0, 0, 0]);
        // IO APIC 0
        b.extend_from_slice(&[1, 12, 0, 0]);
        push_u32(&mut b, 0xFEC0_0000);
        push_u32(&mut b, 0);
        // ISO: IRQ0 → GSI 2 (QEMU classic)
        b.extend_from_slice(&[2, 10, 0, 0]); // type, len, bus, source IRQ
        push_u32(&mut b, 2); // GSI
        push_u16(&mut b, 0); // flags
        // x2APIC CPU — type 9, len 16: reserved u16, uid u32, flags u32,
        // x2APIC id u32 (ACPI spec §5.2.12.12).
        b.extend_from_slice(&[9, 16, 0, 0]);
        push_u32(&mut b, 16); // ACPI processor UID
        push_u32(&mut b, 1); // flags: enabled
        push_u32(&mut b, 16); // x2APIC ID
        b
    }

    fn build_qemu_image() -> Vec<u8> {
        // Layout: [0..0x100) RSDP, MADT at 0x100, RSDT at 0x400.
        let madt = sdt(b"APIC", &madt_body());
        let mut image = vec![0u8; 0x1000];
        image[0x100..0x100 + madt.len()].copy_from_slice(&madt);
        let mut body = Vec::new();
        push_u32(&mut body, 0x100);
        let rsdt = sdt(b"RSDT", &body);
        image[0x400..0x400 + rsdt.len()].copy_from_slice(&rsdt);
        let r = rsdp(0x400);
        image[..r.len()].copy_from_slice(&r);
        image
    }

    #[test]
    fn rsdp_parses_rev0() {
        let r = rsdp(0x400);
        let parsed = Rsdp::parse(&r).expect("rsdp");
        assert_eq!(parsed.revision, 0);
        assert_eq!(parsed.rsdt_address, 0x400);
        assert_eq!(parsed.xsdt_address, 0);
        assert_eq!(parsed.root_table(), Some((0x400, false)));
    }

    #[test]
    fn rsdp_rejects_bad_signature_and_checksum() {
        let mut r = rsdp(0x400);
        r[0] = b'X';
        assert!(Rsdp::parse(&r).is_none());
        let mut r2 = rsdp(0x400);
        r2[10] ^= 0xFF; // corrupt after checksum computed
        assert!(Rsdp::parse(&r2).is_none());
        assert!(Rsdp::parse(&r2[..10]).is_none()); // too short
    }

    #[test]
    fn root_table_entry_widths() {
        let mut xsdt_body = Vec::new();
        xsdt_body.extend_from_slice(&0xDEAD_BEEFu64.to_le_bytes());
        let mut out = [0u64; MAX_TABLES];
        let n = root_table_entries(&xsdt_body, true, &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0], 0xDEAD_BEEF);
        let mut rsdt_body = Vec::new();
        push_u32(&mut rsdt_body, 0x1234_5678);
        let mut out2 = [0u64; MAX_TABLES];
        let n2 = root_table_entries(&rsdt_body, false, &mut out2);
        assert_eq!(n2, 1);
        assert_eq!(out2[0], 0x1234_5678);
        // trailing partial pointer ignored
        rsdt_body.push(0);
        let mut out3 = [0u64; MAX_TABLES];
        assert_eq!(root_table_entries(&rsdt_body, false, &mut out3), 1);
    }

    #[test]
    fn checksum_rule() {
        let t = sdt(b"APIC", &[1, 2, 3]);
        assert!(checksum_ok(&t, t.len()));
        let mut bad = t.clone();
        bad[0] ^= 0x55;
        assert!(!checksum_ok(&bad, bad.len()));
    }

    #[test]
    fn madt_walk_full() {
        let madt = sdt(b"APIC", &madt_body());
        let info = parse_madt(&madt).expect("madt");
        assert_eq!(info.local_apic_address, 0xFEE0_0000);
        assert!(info.has_8259);
        assert_eq!(info.cpu_count, 4); // 3 legacy + 1 x2APIC
        assert_eq!(info.enabled_cpus(), 3);
        assert_eq!(info.cpus()[1], CpuEntry { acpi_id: 1, apic_id: 1, enabled: true });
        assert!(!info.cpus()[2].enabled);
        // x2APIC entry parsed
        let x2 = info.cpus()[3];
        assert_eq!((x2.acpi_id, x2.apic_id), (16, 16));
        assert!(x2.enabled);
        assert_eq!(info.ioapic_count, 1);
        assert_eq!(
            info.ioapics()[0],
            IoApicEntry { id: 0, address: 0xFEC0_0000, gsi_base: 0 }
        );
        assert!(info.timer_iso_present());
        assert_eq!(info.isos()[0].bus_source_irq, 0);
        assert_eq!(info.isos()[0].gsi, 2);
    }

    #[test]
    fn madt_capacity_is_bounded() {
        // 300 CPUs: only MAX_CPUS recorded, no overflow.
        let mut b = Vec::new();
        push_u32(&mut b, 0xFEE0_0000);
        b.extend_from_slice(&[1, 0, 0, 0]);
        for i in 0..300u32 {
            b.extend_from_slice(&[0, 8, i as u8, i as u8, 1, 0, 0, 0]);
        }
        let t = sdt(b"APIC", &b);
        let info = parse_madt(&t).expect("madt");
        assert_eq!(info.cpu_count, MAX_CPUS);
        assert_eq!(info.enabled_cpus(), MAX_CPUS);
    }

    #[test]
    fn madt_address_override_applied() {
        let mut body = Vec::new();
        push_u32(&mut body, 0xFEE0_0000);
        body.extend_from_slice(&[1, 0, 0, 0]);
        body.push(5);
        body.push(12);
        body.extend_from_slice(&[0, 0]);
        // 64-bit address 0x0000_0000_FEE1_0000, low 32 bits first (LE).
        push_u32(&mut body, 0xFEE1_0000);
        push_u32(&mut body, 0);
        let t = sdt(b"APIC", &body);
        let info = parse_madt(&t).expect("madt");
        assert_eq!(info.local_apic_address, 0xFEE1_0000);
    }

    #[test]
    fn madt_rejects_wrong_signature_and_short() {
        let t = sdt(b"XXXX", &[0u8; 16]);
        assert!(parse_madt(&t).is_none());
        assert!(parse_madt(&[0u8; 8]).is_none());
    }

    #[test]
    fn full_image_walk() {
        let image = build_qemu_image();
        let rsdp = Rsdp::parse(&image).expect("rsdp");
        let (root_addr, is_xsdt) = rsdp.root_table().unwrap();
        assert!(!is_xsdt);
        let (body, header) = table_at(&image, root_addr).expect("rsdt");
        assert_eq!(header.signature_str(), "RSDT");
        assert!(checksum_ok(body, header.length as usize));
        let mut addrs = [0u64; MAX_TABLES];
        let n = root_table_entries(&body[SDT_HEADER_LEN..], is_xsdt, &mut addrs);
        assert_eq!(n, 1);
        let (madt_body, madt_hdr) = table_at(&image, addrs[0]).expect("madt");
        assert_eq!(madt_hdr.signature_str(), "APIC");
        assert!(checksum_ok(madt_body, madt_hdr.length as usize));
        let info = parse_madt(madt_body).expect("madt");
        assert_eq!(info.enabled_cpus(), 3);
    }

    #[test]
    fn sdt_header_fields() {
        let t = sdt(b"APIC", &[0xAB]);
        let h = SdtHeader::parse(&t).unwrap();
        assert_eq!(&h.signature, b"APIC");
        assert_eq!(h.length as usize, t.len());
        assert_eq!(&h.oem_id, b"VARIX ");
        assert_eq!(&h.oem_table_id, b"VARIXTBL");
        assert_eq!(h.signature_str(), "APIC");
        assert!(SdtHeader::parse(&t[..20]).is_none());
    }
}
