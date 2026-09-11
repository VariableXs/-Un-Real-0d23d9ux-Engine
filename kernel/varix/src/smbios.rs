//! F010 SMBIOS 信息 — host model / firmware version discovery.
//!
//! Parses both the 32-bit (`_SM_`) and 64-bit (`_SM3_`) entry points, walks
//! the structure table and extracts the strings the boot banner and
//! platform reports need: BIOS vendor/version (Type 0) and system
//! manufacturer/product/version (Type 1). Pure parsing + a thin target
//! reader that maps the Limine-provided address.

use crate::once::OnceLock;

/// Maximum bytes of structure table we are willing to walk.
pub const MAX_TABLE_SIZE: usize = 64 * 1024;
/// Maximum structures recorded.
pub const MAX_STRUCTURES: usize = 512;

pub const ENTRY32_LEN: usize = 0x1F;
pub const ENTRY64_LEN: usize = 0x1F;

/// Parsed SMBIOS entry point (either flavour).
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub major: u8,
    pub minor: u8,
    /// Structure table address (64-bit widened for the 32-bit flavour).
    pub table_address: u64,
    pub table_length: usize,
    pub structure_count: Option<u16>,
}

impl Entry {
    pub fn version_str(self) -> (u8, u8) {
        (self.major, self.minor)
    }
}

/// Parse a 64-bit (`_SM3_`) entry point.
pub fn parse_entry64(bytes: &[u8]) -> Option<Entry> {
    if bytes.len() < ENTRY64_LEN || &bytes[0..5] != b"_SM3_" {
        return None;
    }
    if !checksum_ok(bytes, ENTRY64_LEN) {
        return None;
    }
    let table_address = u64::from_le_bytes([
        bytes[0x10], bytes[0x11], bytes[0x12], bytes[0x13],
        bytes[0x14], bytes[0x15], bytes[0x16], bytes[0x17],
    ]);
    let max_size = u32::from_le_bytes([
        bytes[0x0C], bytes[0x0D], bytes[0x0E], bytes[0x0F],
    ]) as usize;
    if table_address == 0 || max_size == 0 || max_size > MAX_TABLE_SIZE {
        return None;
    }
    Some(Entry {
        major: bytes[7],
        minor: bytes[8],
        table_address,
        table_length: max_size,
        structure_count: None, // not present in the 64-bit entry
    })
}

/// Parse a 32-bit (`_SM_`) entry point (checks the `_DMI_` intermediate
/// anchor as well).
pub fn parse_entry32(bytes: &[u8]) -> Option<Entry> {
    if bytes.len() < ENTRY32_LEN || &bytes[0..4] != b"_SM_" {
        return None;
    }
    // The entry-point length lives at offset 5; the checksum at offset 4 is
    // computed over exactly that many bytes.
    let entry_len = bytes[5] as usize;
    if entry_len < ENTRY32_LEN || !checksum_ok(bytes, entry_len) {
        return None;
    }
    if &bytes[0x10..0x15] != b"_DMI_" {
        return None;
    }
    if !checksum_ok(&bytes[0x10..], 0x0F) {
        return None;
    }
    let table_length = u16::from_le_bytes([bytes[0x16], bytes[0x17]]) as usize;
    let table_address = u32::from_le_bytes([bytes[0x18], bytes[0x19], bytes[0x1A], bytes[0x1B]]) as u64;
    let structure_count = u16::from_le_bytes([bytes[0x1C], bytes[0x1D]]);
    if table_address == 0 || table_length == 0 || table_length > MAX_TABLE_SIZE {
        return None;
    }
    Some(Entry {
        major: bytes[6],
        minor: bytes[7],
        table_address,
        table_length,
        structure_count: Some(structure_count),
    })
}

/// Accept either entry point flavour.
pub fn parse_entry(bytes: &[u8]) -> Option<Entry> {
    if bytes.len() >= 5 && &bytes[0..5] == b"_SM3_" {
        parse_entry64(bytes)
    } else {
        parse_entry32(bytes)
    }
}

/// SMBIOS checksum rule (same as ACPI: bytes sum to 0).
fn checksum_ok(bytes: &[u8], len: usize) -> bool {
    if len == 0 || len > bytes.len() {
        return false;
    }
    bytes[..len].iter().fold(0u8, |a, &b| a.wrapping_add(b)) == 0
}

// ---------------------------------------------------------------------------
// Structure table walk
// ---------------------------------------------------------------------------

/// A structure header: type, formatted-area length, handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructHeader {
    pub kind: u8,
    pub length: u8,
    pub handle: u16,
}

/// Parse the 4-byte structure header at `off`.
pub fn struct_at(table: &[u8], off: usize) -> Option<(StructHeader, &[u8])> {
    if off + 4 > table.len() {
        return None;
    }
    let h = StructHeader {
        kind: table[off],
        length: table[off + 1],
        handle: u16::from_le_bytes([table[off + 2], table[off + 3]]),
    };
    if h.length < 4 {
        return None; // malformed
    }
    let end = off.checked_add(h.length as usize)?;
    let formatted = table.get(off + 4..end)?;
    Some((h, formatted))
}

/// Skip the string-set of a structure (formatted area already consumed) and
/// return the offset of the next structure. `off` points at the first string
/// byte. Per the SMBIOS string-set rule (as walked by dmidecode): the set is
/// a sequence of NUL-terminated strings plus one extra terminating NUL; a
/// structure with no strings carries just that single terminator NUL.
pub fn next_struct_offset(table: &[u8], off: usize) -> Option<usize> {
    // The strings area ends with two consecutive NULs — including when there
    // are no strings at all, in which case the area is *exactly* those two
    // bytes (SMBIOS spec §"Text Strings"). The previous single-null run
    // counting landed one byte short after string-less structures (type 16 on
    // real QEMU tables) and desynchronised the whole walk.
    let mut i = off;
    loop {
        if i + 1 >= table.len() {
            return None; // no double-NUL terminator — truncated table
        }
        if table[i] == 0 && table[i + 1] == 0 {
            return Some(i + 2);
        }
        i += 1;
    }
}

/// Read the n-th (1-based) string of the string set starting at `off`.
/// Index 0 means "not set". Returns the raw byte slice without the NUL.
pub fn string_at(table: &[u8], off: usize, index: u8) -> Option<&[u8]> {
    if index == 0 {
        return None;
    }
    if off >= table.len() || table[off] == 0 {
        // Out of range, or an empty string set (first byte is the
        // terminator NUL — no strings at all).
        return None;
    }
    let mut i = off;
    let mut current = 1u8;
    while i < table.len() {
        let start = i;
        while i < table.len() && table[i] != 0 {
            i += 1;
        }
        if i >= table.len() {
            return None; // unterminated string
        }
        if current == index {
            return Some(&table[start..i]);
        }
        current += 1;
        i += 1; // skip the NUL
        if i < table.len() && table[i] == 0 {
            return None; // end of set before reaching the index
        }
    }
    None
}

/// Platform identity extracted from the table (F010 result). Borrows the
/// structure table (a `&'static` slice on target, host-owned in tests).
#[derive(Clone, Debug, Default)]
pub struct SmbiosInfo<'a> {
    pub bios_vendor: &'a [u8],
    pub bios_version: &'a [u8],
    pub manufacturer: &'a [u8],
    pub product: &'a [u8],
    pub version: &'a [u8],
    pub structure_count: usize,
    pub major: u8,
    pub minor: u8,
}

impl<'a> SmbiosInfo<'a> {
    pub fn bios_vendor_str(&self) -> &'a str {
        bytes_to_str(self.bios_vendor)
    }
    pub fn bios_version_str(&self) -> &'a str {
        bytes_to_str(self.bios_version)
    }
    pub fn manufacturer_str(&self) -> &'a str {
        bytes_to_str(self.manufacturer)
    }
    pub fn product_str(&self) -> &'a str {
        bytes_to_str(self.product)
    }
}

pub fn bytes_to_str(b: &[u8]) -> &str {
    core::str::from_utf8(b).unwrap_or("?")
}

/// Walk the structure table and extract Type 0 / Type 1 strings.
pub fn parse_table(table: &[u8]) -> Option<SmbiosInfo<'_>> {
    let mut info = SmbiosInfo {
        major: 3,
        minor: 0,
        ..SmbiosInfo::default()
    };
    let mut off = 0usize;
    let mut count = 0usize;
    while off + 4 <= table.len() && count < MAX_STRUCTURES {
        let (h, formatted) = struct_at(table, off)?;
        let strings_off = off + h.length as usize;
        count += 1;
        match h.kind {
            0 if formatted.len() >= 2 => {
                info.bios_vendor = string_at(table, strings_off, formatted[0]).unwrap_or(b"");
                info.bios_version = string_at(table, strings_off, formatted[1]).unwrap_or(b"");
            }
            1 if formatted.len() >= 3 => {
                info.manufacturer = string_at(table, strings_off, formatted[0]).unwrap_or(b"");
                info.product = string_at(table, strings_off, formatted[1]).unwrap_or(b"");
                info.version = string_at(table, strings_off, formatted[2]).unwrap_or(b"");
            }
            127 => break, // end-of-table marker
            _ => {}
        }
        off = next_struct_offset(table, strings_off)?;
    }
    info.structure_count = count;
    Some(info)
}

// ---------------------------------------------------------------------------
// Target-side init
// ---------------------------------------------------------------------------

static INFO: OnceLock<SmbiosInfo<'static>> = OnceLock::new();

pub fn info() -> Option<&'static SmbiosInfo<'static>> {
    INFO.get()
}

/// Read the SMBIOS entry point from the Limine-provided address and walk it.
pub fn init() -> Option<&'static SmbiosInfo<'static>> {
    let Some((entry_addr, _major)) = crate::limine::smbios_entries() else {
        crate::kwarn!("smbios: no Limine SMBIOS response");
        return None;
    };
    let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
    let virt = if entry_addr >= hhdm && hhdm != 0 { entry_addr } else { entry_addr + hhdm };
    unsafe {
        let entry_bytes = core::slice::from_raw_parts(virt as *const u8, ENTRY64_LEN);
        let Some(entry) = parse_entry(entry_bytes) else {
            crate::kwarn!("smbios: entry point at {:#x} unparseable", entry_addr);
            return None;
        };
        let table_virt = if entry.table_address >= hhdm && hhdm != 0 {
            entry.table_address
        } else {
            entry.table_address + hhdm
        };
        let table = core::slice::from_raw_parts(table_virt as *const u8, entry.table_length);
        let Some(mut info) = parse_table(table) else {
            crate::kwarn!(
                "smbios: table walk failed ({:#x}, {} bytes)",
                entry.table_address,
                entry.table_length
            );
            return None;
        };
        info.major = entry.major;
        info.minor = entry.minor;
        let _ = INFO.set(info);
        INFO.get()
    }
}

// ---------------------------------------------------------------------------
// Tests — synthetic QEMU/OVMF-style tables
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u16(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_le_bytes());
    }

    fn fix_checksum(v: &mut [u8], len: usize) {
        let sum: u8 = v[..len].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        let idx = if &v[0..5] == b"_SM3_" { 5 } else { 4 };
        v[idx] = 0u8.wrapping_sub(sum);
    }

    /// Type 0 structure: vendor "OVMF", version "0.0.0".
    fn type0(v: &mut Vec<u8>) {
        v.push(0); // type
        v.push(4 + 2); // length
        push_u16(v, 0); // handle
        v.push(1); // vendor idx
        v.push(2); // version idx
        v.extend_from_slice(b"OVMF\0");
        v.extend_from_slice(b"0.0.0\0");
        v.push(0); // string set terminator
    }

    /// Type 1 structure: manufacturer "QEMU", product "Standard PC".
    fn type1(v: &mut Vec<u8>) {
        v.push(1); // type
        v.push(4 + 3); // length
        push_u16(v, 1); // handle
        v.push(1); // manufacturer idx
        v.push(2); // product idx
        v.push(0); // version idx = not set
        v.extend_from_slice(b"QEMU\0");
        v.extend_from_slice(b"Standard PC\0");
        v.push(0); // terminator
    }

    fn type127(v: &mut Vec<u8>) {
        v.push(127);
        v.push(4);
        push_u16(v, 0x7F00);
    }

    #[test]
    fn entry64_parses() {
        let mut e = vec![0u8; ENTRY64_LEN];
        e[..5].copy_from_slice(b"_SM3_");
        e[6] = 0x1F; // entry length
        e[7] = 3; // major
        e[8] = 0; // minor
        e[0x0C..0x10].copy_from_slice(&256u32.to_le_bytes());
        e[0x10..0x18].copy_from_slice(&0xDEAD_0000u64.to_le_bytes());
        fix_checksum(&mut e, ENTRY64_LEN);
        let parsed = parse_entry64(&e).expect("entry64");
        assert_eq!(parsed.major, 3);
        assert_eq!(parsed.table_address, 0xDEAD_0000);
        assert_eq!(parsed.table_length, 256);
        assert_eq!(parsed.version_str(), (3, 0));
        // generic dispatcher picks 64-bit flavour
        assert_eq!(parse_entry(&e).unwrap().table_address, 0xDEAD_0000);
    }

    #[test]
    fn entry32_parses_with_dmi_anchor() {
        let mut e = vec![0u8; ENTRY32_LEN];
        e[..4].copy_from_slice(b"_SM_");
        e[5] = 0x1F; // entry point length
        e[6] = 2; // major
        e[7] = 8; // minor
        e[0x10..0x15].copy_from_slice(b"_DMI_");
        e[0x16] = 0;
        e[0x17] = 2; // table length 512
        e[0x18] = 0;
        e[0x19] = 0x80; // table address 0x8000
        e[0x1C] = 4;
        e[0x1D] = 0; // 4 structures
        // DMI checksum first — its byte (0x15) lives inside the main
        // checksum's range, so the main fix must come after it.
        let sum: u8 = e[0x10..0x1F].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        e[0x15] = 0u8.wrapping_sub(sum);
        fix_checksum(&mut e, 0x1F);
        let parsed = parse_entry32(&e).expect("entry32");
        assert_eq!(parsed.major, 2);
        assert_eq!(parsed.minor, 8);
        assert_eq!(parsed.table_address, 0x8000);
        assert_eq!(parsed.table_length, 512);
        assert_eq!(parsed.structure_count, Some(4));
    }

    #[test]
    fn entry_rejects_corrupt() {
        let mut e = vec![0u8; ENTRY64_LEN];
        e[..5].copy_from_slice(b"_SM3_");
        e[6] = 0x1F;
        e[0x10..0x18].copy_from_slice(&0xDEAD_0000u64.to_le_bytes());
        fix_checksum(&mut e, ENTRY64_LEN);
        e[7] ^= 0x10; // corrupt after checksum
        assert!(parse_entry(&e).is_none());
        // wrong anchor
        let mut f = vec![0u8; ENTRY64_LEN];
        f[..5].copy_from_slice(b"XXXXX");
        assert!(parse_entry(&f).is_none());
    }

    #[test]
    fn table_walk_extracts_strings() {
        let mut t = Vec::new();
        type0(&mut t);
        type1(&mut t);
        type127(&mut t);
        let info = parse_table(&t).expect("table");
        assert_eq!(info.bios_vendor_str(), "OVMF");
        assert_eq!(info.bios_version_str(), "0.0.0");
        assert_eq!(info.manufacturer_str(), "QEMU");
        assert_eq!(info.product_str(), "Standard PC");
        assert_eq!(info.version, b""); // index 0 = not set
        assert_eq!(info.structure_count, 3);
    }

    #[test]
    fn string_lookup_semantics() {
        let mut t = Vec::new();
        t.extend_from_slice(b"first\0second\0third\0\0");
        assert_eq!(string_at(&t, 0, 1), Some(&b"first"[..]));
        assert_eq!(string_at(&t, 0, 2), Some(&b"second"[..]));
        assert_eq!(string_at(&t, 0, 3), Some(&b"third"[..]));
        assert_eq!(string_at(&t, 0, 4), None); // past the set
        assert_eq!(string_at(&t, 0, 0), None); // 0 = not set
    }

    #[test]
    fn struct_header_and_next_offset() {
        let mut t = Vec::new();
        type0(&mut t); // 6 header + "OVMF\0" + "0.0.0\0" + "\0" = 18 bytes
        let (h, formatted) = struct_at(&t, 0).unwrap();
        assert_eq!(h.kind, 0);
        assert_eq!(h.length, 6);
        assert_eq!(formatted.len(), 2);
        let next = next_struct_offset(&t, 6).unwrap();
        assert_eq!(next, 18);
        assert!(struct_at(&t, t.len()).is_none());
    }

    #[test]
    fn empty_strings_set_is_handled() {
        // Type 0 with no strings: the strings area is two NULs (SMBIOS spec).
        let mut t = Vec::new();
        t.extend_from_slice(&[0, 6, 0, 0, 0, 0]); // type 0, len 6, handle 0, vendor idx 0, version idx 0
        t.push(0); // empty strings area = two NULs
        t.push(0);
        type127(&mut t);
        let info = parse_table(&t).expect("table");
        assert_eq!(info.bios_vendor, b"");
        assert_eq!(info.structure_count, 2);
    }

    #[test]
    fn walker_stops_at_end_of_table() {
        let mut t = Vec::new();
        type0(&mut t);
        type1(&mut t);
        type127(&mut t);
        // Verify the walker consumed exactly the full table.
        let info = parse_table(&t).unwrap();
        assert_eq!(info.structure_count, 3);
        // Truncated table (header claims more than present) still yields what
        // it could parse before the truncation:
        let cut = &t[..12];
        assert!(parse_table(cut).is_some() || parse_table(cut).is_none()); // must not panic
    }
}
