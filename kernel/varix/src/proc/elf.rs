//! F104 ELF64 加载器 / F105 动态链接占位.
//!
//! The loader is what decides whether a file is runnable, so it refuses
//! anything it cannot fully vouch for: wrong class, wrong machine, a segment
//! that runs off the end of the image, an entry point outside the user half, or
//! a shared-object interpreter Varix cannot service. A clear refusal at exec
//! time is worth far more than a mysterious fault three instructions in.

pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const ELFCLASS64: u8 = 2;
pub const ELFDATA_LSB: u8 = 1;
pub const EM_X86_64: u16 = 0x3E;
pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;

pub const PT_LOAD: u32 = 1;
pub const PT_INTERP: u32 = 3;
pub const PT_GNU_STACK: u32 = 0x6474_E551;

pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

pub const EHDR_SIZE: usize = 64;
pub const PHDR_SIZE: usize = 56;
pub const MAX_LOAD_SEGMENTS: usize = 8;
pub const INTERP_PATH_BYTES: usize = 96;

/// F105: dynamic linking is *reserved*, not implemented. Every interface that
/// would carry it exists (PT_INTERP is parsed, the path is extracted), and the
/// loader refuses at the last possible moment with a name the user can read.
pub const DYNAMIC_LINKING_SUPPORTED: bool = false;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElfError {
    TooSmall,
    BadMagic,
    Not64Bit,
    NotLittleEndian,
    WrongMachine,
    NotExecutable,
    BadHeaderSize,
    BadProgramHeaders,
    NoLoadSegments,
    SegmentOutOfRange,
    SegmentMisaligned,
    BssSmallerThanData,
    EntryOutsideUserRange,
    NeedsInterpreter,
    NoImage,
}

impl ElfError {
    pub fn as_str(self) -> &'static str {
        match self {
            ElfError::TooSmall => "file is smaller than an ELF header",
            ElfError::BadMagic => "not an ELF file",
            ElfError::Not64Bit => "only 64-bit images are supported",
            ElfError::NotLittleEndian => "only little-endian images are supported",
            ElfError::WrongMachine => "image is not built for x86_64",
            ElfError::NotExecutable => "not an executable image",
            ElfError::BadHeaderSize => "ELF header size is wrong",
            ElfError::BadProgramHeaders => "program header table is out of bounds",
            ElfError::NoLoadSegments => "image has nothing to load",
            ElfError::SegmentOutOfRange => "a segment runs past the end of the file",
            ElfError::SegmentMisaligned => "a segment alignment is not a power of two",
            ElfError::BssSmallerThanData => "p_memsz is smaller than p_filesz",
            ElfError::EntryOutsideUserRange => "entry point is not a user address",
            ElfError::NeedsInterpreter => "image needs a dynamic linker that Varix does not have yet",
            ElfError::NoImage => "no such process image",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LoadSegment {
    pub vaddr: u64,
    pub offset: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
    pub flags: u32,
}

impl LoadSegment {
    pub fn end(&self) -> u64 {
        self.vaddr + self.memsz
    }

    pub fn bss_bytes(&self) -> u64 {
        self.memsz.saturating_sub(self.filesz)
    }

    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.vaddr && addr < self.end()
    }

    pub fn writable(&self) -> bool {
        self.flags & PF_W != 0
    }

    pub fn executable(&self) -> bool {
        self.flags & PF_X != 0
    }

    /// The permission bits the page tables should get for this segment.
    pub fn page_flags(&self) -> u64 {
        let mut f = crate::mem::paging::P_USER | crate::mem::paging::P_PRESENT;
        if self.flags & PF_W != 0 {
            f |= crate::mem::paging::P_WRITE;
        }
        if self.flags & PF_X == 0 {
            f |= crate::mem::paging::P_NX;
        }
        f
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElfKind {
    Executable,
    /// PIE: position independent, so the loader picks the base address.
    PositionIndependent,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ElfImage {
    pub entry: u64,
    pub kind: Option<ElfKind>,
    pub segments: [LoadSegment; MAX_LOAD_SEGMENTS],
    pub count: usize,
    /// Byte length of the `PT_INTERP` path, 0 when there is none (F105).
    pub interp_len: usize,
    pub interp_path: [u8; INTERP_PATH_BYTES],
}

impl Default for ElfImage {
    /// Written out rather than derived: `[u8; 96]` has no `Default` impl, and
    /// the interpreter path really does start as 96 zero bytes.
    fn default() -> ElfImage {
        ElfImage {
            entry: 0,
            kind: None,
            segments: [LoadSegment {
                vaddr: 0,
                offset: 0,
                filesz: 0,
                memsz: 0,
                align: 0,
                flags: 0,
            }; MAX_LOAD_SEGMENTS],
            count: 0,
            interp_len: 0,
            interp_path: [0u8; INTERP_PATH_BYTES],
        }
    }
}

impl ElfImage {
    pub fn segments(&self) -> &[LoadSegment] {
        &self.segments[..self.count]
    }

    pub fn needs_interpreter(&self) -> bool {
        self.interp_len > 0
    }

    pub fn interp_path_str(&self) -> Option<&str> {
        if self.interp_len == 0 {
            return None;
        }
        core::str::from_utf8(&self.interp_path[..self.interp_len]).ok()
    }

    /// Total virtual size, and its highest address — both needed to size the
    /// address-space reservation before a single page is mapped.
    pub fn total_memsz(&self) -> u64 {
        self.segments().iter().map(|s| s.memsz).sum()
    }

    pub fn highest_vaddr(&self) -> u64 {
        self.segments().iter().map(|s| s.end()).max().unwrap_or(0)
    }

    pub fn lowest_vaddr(&self) -> u64 {
        self.segments()
            .iter()
            .map(|s| s.vaddr)
            .min()
            .unwrap_or(u64::MAX)
    }

    /// Which segment owns `addr` (used by the fault handler to decide whether a
    /// missing page is a bug or a BSS page that has not been touched yet).
    pub fn segment_of(&self, addr: u64) -> Option<&LoadSegment> {
        self.segments().iter().find(|s| s.contains(addr))
    }

    /// F104: everything the loader must be able to promise before it hands
    /// control to `entry`.
    pub fn validate(&self) -> Result<(), ElfError> {
        if self.kind.is_none() || self.count == 0 {
            return Err(ElfError::NoImage);
        }
        if !crate::mem::paging::is_user(self.entry) {
            return Err(ElfError::EntryOutsideUserRange);
        }
        if self.needs_interpreter() && !DYNAMIC_LINKING_SUPPORTED {
            return Err(ElfError::NeedsInterpreter);
        }
        if self.interp_path_str().is_none() && self.interp_len != 0 {
            return Err(ElfError::NeedsInterpreter);
        }
        // The entry point must live in an executable segment.
        let covered = self
            .segments()
            .iter()
            .any(|s| s.executable() && s.contains(self.entry));
        if !covered {
            return Err(ElfError::EntryOutsideUserRange);
        }
        Ok(())
    }
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}

/// F104: parse an ELF64 image out of the mapped file contents.
///
/// `image` must contain the header and the program header table; the segments
/// themselves are read from the same buffer per the `p_offset` values.
pub fn parse(image: &[u8]) -> Result<ElfImage, ElfError> {
    if image.len() < EHDR_SIZE {
        return Err(ElfError::TooSmall);
    }
    if image[0..4] != ELF_MAGIC {
        return Err(ElfError::BadMagic);
    }
    if image[4] != ELFCLASS64 {
        return Err(ElfError::Not64Bit);
    }
    if image[5] != ELFDATA_LSB {
        return Err(ElfError::NotLittleEndian);
    }
    let e_type = u16_at(image, 16);
    if e_type != ET_EXEC && e_type != ET_DYN {
        return Err(ElfError::NotExecutable);
    }
    if u16_at(image, 18) != EM_X86_64 {
        return Err(ElfError::WrongMachine);
    }
    let entry = u64_at(image, 24);
    let phoff = u64_at(image, 32);
    let ehsize = u16_at(image, 52);
    let phentsize = u16_at(image, 54);
    let phnum = u16_at(image, 56);

    if ehsize as usize != EHDR_SIZE || phentsize as usize != PHDR_SIZE {
        return Err(ElfError::BadHeaderSize);
    }
    if phnum == 0 {
        return Err(ElfError::NoLoadSegments);
    }
    let table_start = phoff as usize;
    let table_end = table_start
        .checked_add(phnum as usize * PHDR_SIZE)
        .ok_or(ElfError::BadProgramHeaders)?;
    if table_end > image.len() {
        return Err(ElfError::BadProgramHeaders);
    }

    let mut out = ElfImage {
        entry,
        kind: Some(if e_type == ET_DYN {
            ElfKind::PositionIndependent
        } else {
            ElfKind::Executable
        }),
        ..ElfImage::default()
    };

    for i in 0..phnum as usize {
        let base = table_start + i * PHDR_SIZE;
        let p_type = u32_at(image, base);
        match p_type {
            PT_LOAD => {
                if out.count >= MAX_LOAD_SEGMENTS {
                    continue;
                }
                let flags = u32_at(image, base + 4);
                let offset = u64_at(image, base + 8);
                let vaddr = u64_at(image, base + 16);
                let filesz = u64_at(image, base + 32);
                let memsz = u64_at(image, base + 40);
                let align = u64_at(image, base + 48);
                if memsz < filesz {
                    return Err(ElfError::BssSmallerThanData);
                }
                // The file-backed part has to be inside the image we were given.
                let file_end = offset
                    .checked_add(filesz)
                    .ok_or(ElfError::SegmentOutOfRange)?;
                if file_end > image.len() as u64 {
                    return Err(ElfError::SegmentOutOfRange);
                }
                if align > 1 && !align.is_power_of_two() {
                    return Err(ElfError::SegmentMisaligned);
                }
                if !crate::mem::paging::is_user(vaddr) {
                    return Err(ElfError::EntryOutsideUserRange);
                }
                out.segments[out.count] = LoadSegment {
                    vaddr,
                    offset,
                    filesz,
                    memsz,
                    align,
                    flags,
                };
                out.count += 1;
            }
            PT_INTERP => {
                // F105: record the interpreter path so the refusal can name it.
                let offset = u64_at(image, base + 8) as usize;
                let filesz = u64_at(image, base + 32) as usize;
                if offset < image.len() && filesz > 0 {
                    let end = (offset + filesz).min(image.len());
                    let slice = &image[offset..end];
                    // The path is NUL-terminated inside the segment.
                    let len = slice.iter().position(|b| *b == 0).unwrap_or(slice.len());
                    let len = len.min(INTERP_PATH_BYTES);
                    out.interp_path[..len].copy_from_slice(&slice[..len]);
                    out.interp_len = len;
                }
            }
            PT_GNU_STACK => {
                // A requested executable stack is a security downgrade, but it
                // is not the loader's decision to refuse — the segment simply
                // records that the program wants one.
            }
            _ => {}
        }
    }

    if out.count == 0 {
        return Err(ElfError::NoLoadSegments);
    }
    Ok(out)
}

/// F105: what happens when a dynamic image is handed to the loader today.
pub fn reject_dynamic(image: &ElfImage) -> Result<(), ElfError> {
    if image.needs_interpreter() && !DYNAMIC_LINKING_SUPPORTED {
        return Err(ElfError::NeedsInterpreter);
    }
    Ok(())
}

/// A tiny synthetic ELF64 image, used by the tests and by the loader's own
/// self-check. Building it here rather than checking in a binary keeps the test
/// independent of any host toolchain.
pub fn synth_image(entry: u64, vaddr: u64, filesz: usize, memsz: u64, flags: u32, interp: Option<&str>) -> [u8; 1024] {
    let mut img = [0u8; 1024];
    img[0..4].copy_from_slice(&ELF_MAGIC);
    img[4] = ELFCLASS64;
    img[5] = ELFDATA_LSB;
    img[6] = 1; // EV_CURRENT
    img[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
    img[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
    img[24..32].copy_from_slice(&entry.to_le_bytes());
    let phoff = EHDR_SIZE as u64;
    let phnum: u16 = if interp.is_some() { 2 } else { 1 };
    img[32..40].copy_from_slice(&phoff.to_le_bytes());
    img[52..54].copy_from_slice(&(EHDR_SIZE as u16).to_le_bytes());
    img[54..56].copy_from_slice(&(PHDR_SIZE as u16).to_le_bytes());
    img[56..58].copy_from_slice(&phnum.to_le_bytes());

    // Segment payload lives right after the program header table.
    let data_off = EHDR_SIZE + phnum as usize * PHDR_SIZE;
    let seg = EHDR_SIZE;
    img[seg..seg + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    img[seg + 4..seg + 8].copy_from_slice(&flags.to_le_bytes());
    img[seg + 8..seg + 16].copy_from_slice(&(data_off as u64).to_le_bytes());
    img[seg + 16..seg + 24].copy_from_slice(&vaddr.to_le_bytes());
    img[seg + 32..seg + 40].copy_from_slice(&(filesz as u64).to_le_bytes());
    img[seg + 40..seg + 48].copy_from_slice(&memsz.to_le_bytes());
    img[seg + 48..seg + 56].copy_from_slice(&4096u64.to_le_bytes());

    if let Some(path) = interp {
        let p = EHDR_SIZE + PHDR_SIZE;
        let bytes = path.as_bytes();
        img[p..p + 4].copy_from_slice(&PT_INTERP.to_le_bytes());
        img[p + 8..p + 16].copy_from_slice(&(data_off as u64).to_le_bytes());
        img[p + 32..p + 40].copy_from_slice(&((bytes.len() + 1) as u64).to_le_bytes());
        img[p + 40..p + 48].copy_from_slice(&((bytes.len() + 1) as u64).to_le_bytes());
        img[data_off..data_off + bytes.len()].copy_from_slice(bytes);
        img[data_off + bytes.len()] = 0;
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_synthetic_image() {
        // Entry 0x40_0800 sits inside the single 4 KiB segment; an entry exactly
        // at a segment's end would be outside it, which the entry test covers.
        let img = synth_image(0x40_0800, 0x40_0000, 64, 4096, PF_R | PF_X, None);
        let e = parse(&img).expect("valid image");
        assert_eq!(e.entry, 0x40_0800);
        assert_eq!(e.kind, Some(ElfKind::Executable));
        assert_eq!(e.count, 1);
        let s = e.segments()[0];
        assert_eq!(s.vaddr, 0x40_0000);
        assert_eq!(s.filesz, 64);
        assert_eq!(s.memsz, 4096);
        assert_eq!(s.bss_bytes(), 4032);
        assert!(s.executable());
        assert!(!s.writable());
        // 4 KiB of memsz starting at 0x40_0000 ends at 0x40_1000.
        assert_eq!(s.end(), 0x40_1000);
        assert!(s.contains(0x40_0000));
        assert!(s.contains(0x40_0FFF));
        assert!(!s.contains(0x40_1000), "the end is exclusive");
        assert_eq!(e.total_memsz(), 4096);
        assert_eq!(e.highest_vaddr(), 0x40_1000);
        assert_eq!(e.lowest_vaddr(), 0x40_0000);
        assert!(e.segment_of(0x40_0800).is_some());
        assert!(e.segment_of(0x90_0000).is_none());
        assert!(e.validate().is_ok());
    }

    #[test]
    fn segment_page_flags_follow_the_program_header() {
        let img = synth_image(0x40_0000, 0x40_0000, 16, 16, PF_R | PF_W, None);
        let e = parse(&img).unwrap();
        let f = e.segments()[0].page_flags();
        assert_ne!(f & crate::mem::paging::P_WRITE, 0);
        assert_ne!(f & crate::mem::paging::P_USER, 0);
        assert_ne!(f & crate::mem::paging::P_NX, 0, "not executable → NX set");
        assert_ne!(f & crate::mem::paging::P_PRESENT, 0);

        let img2 = synth_image(0x40_0000, 0x40_0000, 16, 16, PF_R | PF_X, None);
        let e2 = parse(&img2).unwrap();
        assert_eq!(e2.segments()[0].page_flags() & crate::mem::paging::P_NX, 0);
    }

    #[test]
    fn hostile_headers_are_refused_by_name() {
        let good = synth_image(0x40_0000, 0x40_0000, 16, 4096, PF_R | PF_X, None);

        let mut bad = good;
        bad[0] = b'X';
        assert_eq!(parse(&bad).unwrap_err(), ElfError::BadMagic);

        let mut bad = good;
        bad[4] = 1;
        assert_eq!(parse(&bad).unwrap_err(), ElfError::Not64Bit);

        let mut bad = good;
        bad[5] = 2;
        assert_eq!(parse(&bad).unwrap_err(), ElfError::NotLittleEndian);

        let mut bad = good;
        bad[18] = 0x28; // i386
        assert_eq!(parse(&bad).unwrap_err(), ElfError::WrongMachine);

        let mut bad = good;
        bad[16] = 1; // ET_REL
        assert_eq!(parse(&bad).unwrap_err(), ElfError::NotExecutable);

        let mut bad = good;
        bad[56] = 0; // no program headers
        assert_eq!(parse(&bad).unwrap_err(), ElfError::NoLoadSegments);

        let mut bad = good;
        bad[54] = 32; // wrong phentsize
        assert_eq!(parse(&bad).unwrap_err(), ElfError::BadHeaderSize);

        let mut bad = good;
        bad[32..40].copy_from_slice(&100_000u64.to_le_bytes());
        assert_eq!(parse(&bad).unwrap_err(), ElfError::BadProgramHeaders);

        assert_eq!(parse(&[]).unwrap_err(), ElfError::TooSmall);
        assert_eq!(parse(&[0u8; 8]).unwrap_err(), ElfError::TooSmall);
    }

    #[test]
    fn oversized_segments_and_misalignment_are_caught() {
        let mut bad = synth_image(0x40_0000, 0x40_0000, 16, 4096, PF_R | PF_X, None);
        let seg = EHDR_SIZE;
        // filesz larger than the image.
        bad[seg + 32..seg + 40].copy_from_slice(&900_000u64.to_le_bytes());
        bad[seg + 40..seg + 48].copy_from_slice(&900_000u64.to_le_bytes());
        assert_eq!(parse(&bad).unwrap_err(), ElfError::SegmentOutOfRange);

        let mut bad2 = synth_image(0x40_0000, 0x40_0000, 16, 4096, PF_R | PF_X, None);
        bad2[seg + 48..seg + 56].copy_from_slice(&3000u64.to_le_bytes());
        assert_eq!(parse(&bad2).unwrap_err(), ElfError::SegmentMisaligned);

        let mut bad3 = synth_image(0x40_0000, 0x40_0000, 4096, 4096, PF_R | PF_X, None);
        // memsz < filesz: the BSS arithmetic would underflow.
        bad3[seg + 40..seg + 48].copy_from_slice(&16u64.to_le_bytes());
        assert_eq!(parse(&bad3).unwrap_err(), ElfError::BssSmallerThanData);
    }

    #[test]
    fn entry_must_be_inside_an_executable_segment() {
        // Entry in the user half but outside every segment.
        let img = synth_image(0x50_0000, 0x40_0000, 16, 4096, PF_R | PF_X, None);
        let e = parse(&img).unwrap();
        assert_eq!(e.validate().unwrap_err(), ElfError::EntryOutsideUserRange);

        // Entry inside a non-executable segment.
        let img2 = synth_image(0x40_0000, 0x40_0000, 16, 4096, PF_R | PF_W, None);
        let e2 = parse(&img2).unwrap();
        assert_eq!(e2.validate().unwrap_err(), ElfError::EntryOutsideUserRange);

        // A kernel entry point parses (the header is well-formed) but can never
        // be validated — the refusal happens exactly where it matters.
        let img3 = synth_image(0xFFFF_8000_0000_1000, 0x40_0000, 16, 4096, PF_R | PF_X, None);
        let e3 = parse(&img3).expect("well-formed header");
        assert_eq!(e3.validate().unwrap_err(), ElfError::EntryOutsideUserRange);
    }

    #[test]
    fn interp_is_parsed_and_then_refused_with_its_path() {
        let img = synth_image(0x40_0000, 0x40_0000, 16, 4096, PF_R | PF_X, Some("/lib/ld-varix.so"));
        let e = parse(&img).expect("the header itself is valid");
        assert!(e.needs_interpreter());
        assert_eq!(e.interp_path_str(), Some("/lib/ld-varix.so"));
        // The refusal names what is missing instead of faulting later.
        assert_eq!(e.validate().unwrap_err(), ElfError::NeedsInterpreter);
        assert_eq!(reject_dynamic(&e).unwrap_err(), ElfError::NeedsInterpreter);
        assert_eq!(ElfError::NeedsInterpreter.as_str().contains("dynamic"), true);
        // A static image passes the same check.
        let stat = synth_image(0x40_0000, 0x40_0000, 16, 4096, PF_R | PF_X, None);
        assert!(reject_dynamic(&parse(&stat).unwrap()).is_ok());
    }
}
