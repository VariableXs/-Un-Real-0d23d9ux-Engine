//! F001 UEFI 引导接入 — Limine boot protocol requests (v8.x protocol,
//! 4×u64 request IDs) and kernel image loading contract.
//!
//! Requests are statics in the `.limine_requests` section; the bootloader
//! scans the whole executable image for the magic ID pairs (requests may be
//! located anywhere, 8-byte aligned). Start/end delimiters are intentionally
//! NOT used because Rust gives no ordering guarantee for statics inside a
//! link section — scanning the full image is always correct.
//!
//! F002 boot info 解析 — extracts framebuffer / memory map / RSDP / SMBIOS /
//! HHDM / kernel address / cmdline / boot time from the request responses
//! into a safe `BootInfo` snapshot for the rest of the boot sequence.

use core::ffi::c_void;

pub const COMMON_MAGIC: [u64; 2] = [0xc7b1dd30df4c8b88, 0x0a82e883a194f07b];

/// Base revision 1: entry point is called as a plain C function.
const BASE_REVISION_MAGIC: [u64; 2] = [0xf9562b2d5c95a6c8, 0x6a7b384944536bdc];

// ---------------------------------------------------------------------------
// Protocol structures (repr(C) mirrors of the Limine v8.x ABI)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct Request<T: ?Sized> {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *mut T,
}

#[repr(C)]
pub struct StackSizeRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *mut StackSizeResponse,
    pub stack_size: u64,
}

#[repr(C)]
pub struct BootloaderInfoResponse {
    pub revision: u64,
    pub name: *mut u8,
    pub version: *mut u8,
}

#[repr(C)]
pub struct FirmwareTypeResponse {
    pub revision: u64,
    pub firmware_type: u64,
}

#[repr(C)]
pub struct StackSizeResponse {
    pub revision: u64,
}

#[repr(C)]
pub struct HhdmResponse {
    pub revision: u64,
    pub offset: u64,
}

#[repr(C)]
pub struct FramebufferResponse {
    pub revision: u64,
    pub framebuffer_count: u64,
    pub framebuffers: *mut *mut Framebuffer,
}

#[repr(C)]
pub struct Framebuffer {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
    pub unused: [u8; 7],
    pub edid_size: u64,
    pub edid: *mut u8,
    pub mode_count: u64,
    pub modes: *mut *mut VideoMode,
}

#[repr(C)]
pub struct VideoMode {
    pub pitch: u64,
    pub width: u64,
    pub height: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

#[repr(C)]
pub struct MemmapResponse {
    pub revision: u64,
    pub entry_count: u64,
    pub entries: *mut *mut MemmapEntry,
}

#[repr(C)]
pub struct MemmapEntry {
    pub base: u64,
    pub length: u64,
    pub kind: u64,
}

/// Limine memory map entry kinds.
pub mod memmap_kind {
    pub const USABLE: u64 = 0;
    pub const RESERVED: u64 = 1;
    pub const ACPI_RECLAIMABLE: u64 = 2;
    pub const ACPI_NVS: u64 = 3;
    pub const BAD_MEMORY: u64 = 4;
    pub const BOOTLOADER_RECLAIMABLE: u64 = 5;
    pub const EXECUTABLE_AND_MODULES: u64 = 6;
    pub const FRAMEBUFFER: u64 = 7;
}

#[repr(C)]
pub struct ExecutableFileResponse {
    pub revision: u64,
    pub executable_file: *mut File,
}

#[repr(C)]
pub struct File {
    pub revision: u64,
    pub address: *mut u8,
    pub size: u64,
    pub path: *mut u8,
    pub cmdline: *mut u8,
    pub media_type: u32,
    pub unused: u32,
    pub tftp_ip: u32,
    pub tftp_port: u32,
    pub partition_index: u32,
    pub mbr_disk_id: u32,
    pub gpt_disk_uuid: Uuid,
    pub gpt_part_uuid: Uuid,
    pub part_uuid: Uuid,
}

#[repr(C)]
pub struct Uuid {
    pub a: u32,
    pub b: u16,
    pub c: u16,
    pub d: [u8; 8],
}

#[repr(C)]
pub struct RsdpResponse {
    pub revision: u64,
    pub address: u64,
}

#[repr(C)]
pub struct SmbiosResponse {
    pub revision: u64,
    pub entry_32: u64,
    pub entry_64: u64,
}

#[repr(C)]
pub struct BootTimeResponse {
    pub revision: u64,
    pub boot_time: i64,
}

#[repr(C)]
pub struct ExecutableAddressResponse {
    pub revision: u64,
    pub physical_base: u64,
    pub virtual_base: u64,
}

// ---------------------------------------------------------------------------
// Requests (F001) — consumed by the bootloader before kernel entry
// ---------------------------------------------------------------------------

#[used]
#[link_section = ".limine_requests"]
pub static mut BASE_REVISION: [u64; 3] =
    [BASE_REVISION_MAGIC[0], BASE_REVISION_MAGIC[1], 1];

#[used]
#[link_section = ".limine_requests"]
pub static mut BOOTLOADER_INFO_REQUEST: Request<BootloaderInfoResponse> = Request {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0xf55038d8e2a1202f, 0x279426fcf5f59740],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
pub static mut FIRMWARE_TYPE_REQUEST: Request<FirmwareTypeResponse> = Request {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0x8c2f75d90bef28a8, 0x7045a4688eac00c3],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
pub static mut STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0x224ef0460a8e8926, 0xe1cb0fc25f46ea3d],
    revision: 0,
    response: core::ptr::null_mut(),
    stack_size: 128 * 1024,
};

#[used]
#[link_section = ".limine_requests"]
pub static mut HHDM_REQUEST: Request<HhdmResponse> = Request {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0x48dcf1cb8ad2b852, 0x63984e959a98244b],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
pub static mut FRAMEBUFFER_REQUEST: Request<FramebufferResponse> = Request {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0x9d5827dcd881dd75, 0xa3148604f6fab11b],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
pub static mut MEMMAP_REQUEST: Request<MemmapResponse> = Request {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0x67cf3d9d378a806f, 0xe304acdfc50c3c62],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
pub static mut EXECUTABLE_FILE_REQUEST: Request<ExecutableFileResponse> = Request {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0xad97e90e83f1ed67, 0x31eb5d1c5ff23b69],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
pub static mut RSDP_REQUEST: Request<RsdpResponse> = Request {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0xc5e77b6b397e7b43, 0x27637845accdcf3c],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
pub static mut SMBIOS_REQUEST: Request<SmbiosResponse> = Request {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0x9e9046f11e095391, 0xaa4a520fefbde5ee],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
pub static mut BOOT_TIME_REQUEST: Request<BootTimeResponse> = Request {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0x502746e184c088aa, 0xfbc5ec83e6327893],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
pub static mut EXECUTABLE_ADDRESS_REQUEST: Request<ExecutableAddressResponse> = Request {
    id: [COMMON_MAGIC[0], COMMON_MAGIC[1], 0x71ba76863cc55f63, 0xb2644a48c516a487],
    revision: 0,
    response: core::ptr::null_mut(),
};

// ---------------------------------------------------------------------------
// Safe accessors (bootloader fills the responses before jumping to _start)
// ---------------------------------------------------------------------------

/// Raw request-access helper: none of the requests is mutable through the
/// kernel itself; responses are written once by the bootloader before the
/// kernel entry point runs. The read is volatile so the link-time initial
/// value (null) can never be constant-folded over the bootloader's write.
unsafe fn response_of<T>(req: *const Request<T>) -> *mut T {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*req).response)) }
}

/// SAFETY contract for every accessor below: must only be called after the
/// bootloader handed control to the kernel (`_start`), never re-entrant with
/// a concurrent writer — boot is single threaded.

pub fn bootloader_info() -> Option<(&'static str, &'static str)> {
    unsafe {
        let resp = response_of(&raw const BOOTLOADER_INFO_REQUEST);
        if resp.is_null() {
            return None;
        }
        let r = &*resp;
        Some((cstr(r.name), cstr(r.version)))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Firmware {
    X86Bios,
    Uefi32,
    Uefi64,
    Sbi,
    Unknown(u64),
}

impl Firmware {
    pub fn as_str(self) -> &'static str {
        match self {
            Firmware::X86Bios => "X86 BIOS",
            Firmware::Uefi32 => "UEFI 32",
            Firmware::Uefi64 => "UEFI 64",
            Firmware::Sbi => "RISC-V SBI",
            Firmware::Unknown(_) => "UNKNOWN",
        }
    }
}

pub fn firmware_type() -> Firmware {
    unsafe {
        let resp = response_of(&raw const FIRMWARE_TYPE_REQUEST);
        if resp.is_null() {
            return Firmware::Unknown(u64::MAX);
        }
        match (*resp).firmware_type {
            0 => Firmware::X86Bios,
            1 => Firmware::Uefi32,
            2 => Firmware::Uefi64,
            3 => Firmware::Sbi,
            other => Firmware::Unknown(other),
        }
    }
}

pub fn hhdm_offset() -> Option<u64> {
    unsafe {
        let resp = response_of(&raw const HHDM_REQUEST);
        if resp.is_null() {
            None
        } else {
            Some((*resp).offset)
        }
    }
}

/// First framebuffer (Limine pointers already include the HHDM offset).
pub fn framebuffer() -> Option<&'static Framebuffer> {
    unsafe {
        let resp = response_of(&raw const FRAMEBUFFER_REQUEST);
        if resp.is_null() || (*resp).framebuffer_count == 0 {
            return None;
        }
        let fb = *(*resp).framebuffers;
        if fb.is_null() {
            None
        } else {
            Some(&*fb)
        }
    }
}

pub fn memmap() -> Option<&'static [&'static MemmapEntry]> {
    unsafe {
        let resp = response_of(&raw const MEMMAP_REQUEST);
        if resp.is_null() {
            return None;
        }
        let count = (*resp).entry_count as usize;
        let entries = (*resp).entries;
        if entries.is_null() || count > 512 {
            return None;
        }
        Some(core::slice::from_raw_parts(entries as *const &MemmapEntry, count))
    }
}

pub fn executable_file() -> Option<&'static File> {
    unsafe {
        let resp = response_of(&raw const EXECUTABLE_FILE_REQUEST);
        if resp.is_null() {
            return None;
        }
        let f = (*resp).executable_file;
        if f.is_null() {
            None
        } else {
            Some(&*f)
        }
    }
}

pub fn rsdp_address() -> Option<u64> {
    unsafe {
        let resp = response_of(&raw const RSDP_REQUEST);
        if resp.is_null() || (*resp).address == 0 {
            None
        } else {
            Some((*resp).address)
        }
    }
}

pub fn smbios_entries() -> Option<(u64, u64)> {
    unsafe {
        let resp = response_of(&raw const SMBIOS_REQUEST);
        if resp.is_null() {
            return None;
        }
        let e64 = (*resp).entry_64;
        if e64 != 0 {
            return Some((e64, 3));
        }
        let e32 = (*resp).entry_32;
        if e32 != 0 {
            return Some((e32, 2));
        }
        None
    }
}

pub fn boot_time() -> Option<i64> {
    unsafe {
        let resp = response_of(&raw const BOOT_TIME_REQUEST);
        if resp.is_null() || (*resp).boot_time == 0 {
            None
        } else {
            Some((*resp).boot_time)
        }
    }
}

pub fn executable_address() -> Option<(u64, u64)> {
    unsafe {
        let resp = response_of(&raw const EXECUTABLE_ADDRESS_REQUEST);
        if resp.is_null() {
            None
        } else {
            Some(((*resp).physical_base, (*resp).virtual_base))
        }
    }
}

pub fn cmdline() -> &'static str {
    unsafe {
        let resp = response_of(&raw const EXECUTABLE_FILE_REQUEST);
        if resp.is_null() {
            return "";
        }
        let f = (*resp).executable_file;
        if f.is_null() {
            return "";
        }
        cstr((*f).cmdline)
    }
}

/// C-string to `&str` (Limine strings are ASCII/UTF-8, NUL terminated).
pub fn cstr(p: *const u8) -> &'static str {
    unsafe {
        if p.is_null() {
            return "";
        }
        let mut len = 0usize;
        while *p.add(len) != 0 {
            len += 1;
            if len > 4096 {
                break;
            }
        }
        let bytes = core::slice::from_raw_parts(p, len);
        match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => "",
        }
    }
}

// ---------------------------------------------------------------------------
// F002 — boot info snapshot
// ---------------------------------------------------------------------------

/// Aggregated, safe view of everything the bootloader handed over.
#[derive(Clone, Copy, Debug)]
pub struct BootInfo {
    pub bootloader_name: &'static str,
    pub bootloader_version: &'static str,
    pub firmware: Firmware,
    pub boot_time_epoch: Option<i64>,
    pub hhdm_offset: Option<u64>,
    pub kernel_physical_base: Option<u64>,
    pub kernel_virtual_base: Option<u64>,
    pub kernel_size: Option<u64>,
    pub cmdline: &'static str,
    pub rsdp: Option<u64>,
    pub smbios: Option<(u64, u64)>,
}

impl BootInfo {
    /// Collect the request responses into one snapshot.
    ///
    /// # Safety (in tests)
    /// On target this reads bootloader-written statics after handoff (safe in
    /// the single-threaded boot phase). Host unit tests build the same
    /// struct literally.
    pub fn collect() -> BootInfo {
        let bl = bootloader_info().unwrap_or(("unknown", "unknown"));
        let (kpb, kvb) = executable_address().map(|(p, v)| (p, v)).unwrap_or((0, 0));
        BootInfo {
            bootloader_name: bl.0,
            bootloader_version: bl.1,
            firmware: firmware_type(),
            boot_time_epoch: boot_time(),
            hhdm_offset: hhdm_offset(),
            kernel_physical_base: if kpb == 0 { None } else { Some(kpb) },
            kernel_virtual_base: if kvb == 0 { None } else { Some(kvb) },
            kernel_size: executable_file().map(|f| f.size),
            cmdline: cmdline(),
            rsdp: rsdp_address(),
            smbios: smbios_entries(),
        }
    }
}

/// Marker mirroring the C `void *` contract without importing it.
pub type RawPtr = *mut c_void;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_layouts_match_protocol() {
        // id[4] + revision + response: the ABI every Limine request starts with.
        assert_eq!(core::mem::size_of::<Request<FramebufferResponse>>(), 4 * 8 + 8 + 8);
        assert_eq!(core::mem::size_of::<MemmapEntry>(), 24);
        assert_eq!(core::mem::size_of::<Framebuffer>(), 80);
        // Framebuffer struct: address..modes must stay at fixed offsets.
        let f: Framebuffer = unsafe { core::mem::zeroed() };
        let base = &f as *const _ as usize;
        assert_eq!(&f.width as *const _ as usize - base, 8);
        assert_eq!(&f.height as *const _ as usize - base, 16);
        assert_eq!(&f.pitch as *const _ as usize - base, 24);
        assert_eq!(&f.bpp as *const _ as usize - base, 32);
        assert_eq!(&f.mode_count as *const _ as usize - base, 64);
        assert_eq!(&f.modes as *const _ as usize - base, 72);
    }

    #[test]
    fn base_revision_marker_bytes() {
        // The marker triple must carry the protocol magics and revision 1.
        unsafe {
            let base = &raw const BASE_REVISION;
            assert_eq!((*base)[0], 0xf9562b2d5c95a6c8);
            assert_eq!((*base)[1], 0x6a7b384944536bdc);
            assert_eq!((*base)[2], 1);
        }
    }

    #[test]
    fn all_requests_carry_common_magic() {
        unsafe {
            let ids = [
                (&raw const BOOTLOADER_INFO_REQUEST).read().id,
                (&raw const FIRMWARE_TYPE_REQUEST).read().id,
                (&raw const STACK_SIZE_REQUEST).read().id,
                (&raw const HHDM_REQUEST).read().id,
                (&raw const FRAMEBUFFER_REQUEST).read().id,
                (&raw const MEMMAP_REQUEST).read().id,
                (&raw const EXECUTABLE_FILE_REQUEST).read().id,
                (&raw const RSDP_REQUEST).read().id,
                (&raw const SMBIOS_REQUEST).read().id,
                (&raw const BOOT_TIME_REQUEST).read().id,
                (&raw const EXECUTABLE_ADDRESS_REQUEST).read().id,
            ];
            for id in ids {
                assert_eq!(id[0], COMMON_MAGIC[0]);
                assert_eq!(id[1], COMMON_MAGIC[1]);
                assert!(id[2] != 0 && id[3] != 0);
            }
        }
    }

    #[test]
    fn firmware_type_names() {
        assert_eq!(Firmware::Uefi64.as_str(), "UEFI 64");
        assert_eq!(Firmware::X86Bios.as_str(), "X86 BIOS");
        assert_eq!(Firmware::Unknown(9).as_str(), "UNKNOWN");
    }
}
