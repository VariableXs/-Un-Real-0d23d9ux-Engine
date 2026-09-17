//! 任务39（AI-B）· PE32+（x86-64）装载器 —— 静态链接 exe 加载运行。
//!
//! 与 elf.rs 同一职责口径：装载器是决定"这个文件能不能跑"的人，所以
//! 它拒绝一切不能完全担保的东西——非 AMD64 机器、非 PE32+ 可选头、
//! 节内容越过文件边界、入口不在任何可执行节内、ImageBase 落进内核
//! 半区。exec 时一次清楚拒绝，远好过三条指令之后一场莫名故障。
//!
//! **任务39 边界 → 任务40 兑现**：导入目录 RVA/Size 解析后暴露在
//! [`PeImage::import_dir`]；任务40 的 [`parse_imports_into`] 把描述符数组、
//! INT/IAT thunk 链与名字记录解析成结构化 [`ImportTable`]，绑定（registry
//! 查找 + IAT 补钉 + thunk 页）在 `winapi.rs`。带导入的映像经绑定后照常
//! 进入 ring3（spawn_pe_imports 链）。
//!
//! 装载执行走任务15 引擎：[`PeSource`] 实现 [`super::loader::ImageFormat`]，
//! 节翻译成 [`SegmentView`]，页账本/同页共享/回滚语义全部复用，引擎
//! 对格式零知识的结构保证在此兑现（loader.rs 测试里 `PeLike` 镜像的
//! 路径就是本模块走的路）。
//!
//! W^X 口径与 ElfSource 一致：可写节强制 NX（映射层 executable=false）。

use super::loader::SegmentView;

/// DOS 头最小长度（e_lfanew@0x3C 需要 4 字节）。
pub const DOS_HEADER_SIZE: usize = 0x40;
/// PE 签名 + COFF 头（20B）+ PE32+ 可选头（240B）= 0xF8，节表随后。
pub const OPTIONAL_HDR_SIZE_PE32P: usize = 0xF0;
/// 节表项长度。
pub const SECTION_HEADER_SIZE: usize = 40;
/// 节数上限（Windows 加载器同为 96）。
pub const MAX_SECTIONS: usize = 96;
/// SizeOfImage 上限 256 MiB——超出视为损坏/恶意头。
pub const MAX_IMAGE_SIZE: u64 = 256 * 1024 * 1024;

pub const PE_MAGIC: [u8; 4] = [b'P', b'E', 0, 0];
pub const MZ_MAGIC: [u8; 2] = [b'M', b'Z'];
/// IMAGE_FILE_MACHINE_AMD64
pub const MACHINE_AMD64: u16 = 0x8664;
/// PE32+ 可选头魔数（PE32 = 0x10B，明确拒绝）。
pub const OPTIONAL_MAGIC_PE32P: u16 = 0x20B;

/// 节特征位（用到的三个）。
pub const SCN_MEM_EXECUTE: u32 = 0x2000_0000;
pub const SCN_MEM_READ: u32 = 0x4000_0000;
pub const SCN_MEM_WRITE: u32 = 0x8000_0000;
/// 代码节（与 EXECUTE 独立出现于数据节时不算可执行）。
pub const SCN_CNT_CODE: u32 = 0x0000_0020;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeError {
    TooSmall,
    BadDosMagic,
    BadPeOffset,
    BadPeMagic,
    WrongMachine,
    NotPe32Plus,
    BadSectionTable,
    NoSections,
    SectionOutOfRange,
    SectionMisaligned,
    ImageTooLarge,
    EntryOutsideImage,
    EntryNotExecutable,
    ImageBaseNotUser,
}

impl PeError {
    pub fn as_str(self) -> &'static str {
        match self {
            PeError::TooSmall => "file is smaller than a DOS header",
            PeError::BadDosMagic => "not an MZ image",
            PeError::BadPeOffset => "e_lfanew is out of bounds",
            PeError::BadPeMagic => "PE signature missing",
            PeError::WrongMachine => "image is not built for AMD64",
            PeError::NotPe32Plus => "only PE32+ (64-bit) optional headers are supported",
            PeError::BadSectionTable => "section header table is out of bounds",
            PeError::NoSections => "image has nothing to load",
            PeError::SectionOutOfRange => "a section runs past the end of the file",
            PeError::SectionMisaligned => "section alignment violates page alignment",
            PeError::ImageTooLarge => "SizeOfImage exceeds the loader cap",
            PeError::EntryOutsideImage => "entry RVA is outside SizeOfImage",
            PeError::EntryNotExecutable => "entry point is not inside an executable section",
            PeError::ImageBaseNotUser => "ImageBase is not a user address",
        }
    }
}

/// 导入目录（任务40 的消费点；任务39 只解析不行动）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ImportDir {
    pub rva: u32,
    pub size: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct PeSection {
    pub rva: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub raw_offset: u64,
    pub writable: bool,
    pub executable: bool,
}

impl PeSection {
    pub fn end(&self) -> u64 {
        self.rva + self.memsz
    }

    pub fn contains(&self, rva: u64) -> bool {
        rva >= self.rva && rva < self.end()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PeImage {
    /// 入口绝对虚拟地址（ImageBase + AddressOfEntryPoint）。
    pub entry: u64,
    pub image_base: u64,
    pub size_of_image: u64,
    pub sections: [PeSection; MAX_SECTIONS],
    pub count: usize,
    /// 导入目录；`None` = 数据目录缺失或全零（静态映像）。
    pub import_dir: Option<ImportDir>,
}

impl Default for PeImage {
    fn default() -> Self {
        PeImage {
            entry: 0,
            image_base: 0,
            size_of_image: 0,
            sections: [PeSection {
                rva: 0,
                filesz: 0,
                memsz: 0,
                raw_offset: 0,
                writable: false,
                executable: false,
            }; MAX_SECTIONS],
            count: 0,
            import_dir: None,
        }
    }
}

impl PeImage {
    pub fn sections(&self) -> &[PeSection] {
        &self.sections[..self.count]
    }

    /// 是否带导入（任务40 的分流依据；任务39 仅记录）。
    pub fn has_imports(&self) -> bool {
        self.import_dir.is_some()
    }
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[off..off + 4]);
    u32::from_le_bytes(a)
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}

/// 解析 PE32+ 映像。`image` 必须包含头、节表与全部节原始内容。
pub fn parse(image: &[u8]) -> Result<PeImage, PeError> {
    if image.len() < DOS_HEADER_SIZE {
        return Err(PeError::TooSmall);
    }
    if image[0..2] != MZ_MAGIC {
        return Err(PeError::BadDosMagic);
    }
    let pe_off = u32_at(image, 0x3C) as usize;
    // e_lfanew 至少要让 4B 签名 + 20B COFF 头放下。
    if pe_off < DOS_HEADER_SIZE || pe_off + 24 > image.len() {
        return Err(PeError::BadPeOffset);
    }
    if image[pe_off..pe_off + 4] != PE_MAGIC {
        return Err(PeError::BadPeMagic);
    }
    let coff = pe_off + 4;
    if u16_at(image, coff) != MACHINE_AMD64 {
        return Err(PeError::WrongMachine);
    }
    let nsec = u16_at(image, coff + 2) as usize;
    let opt_size = u16_at(image, coff + 16) as usize;
    if nsec == 0 || nsec > MAX_SECTIONS {
        return Err(PeError::NoSections);
    }
    let opt = coff + 20;
    if opt_size < OPTIONAL_HDR_SIZE_PE32P || opt + opt_size > image.len() {
        return Err(PeError::BadSectionTable);
    }
    if u16_at(image, opt) != OPTIONAL_MAGIC_PE32P {
        return Err(PeError::NotPe32Plus);
    }
    let entry_rva = u32_at(image, opt + 16) as u64;
    let base = u64_at(image, opt + 24);
    let sec_align = u32_at(image, opt + 32) as u64;
    let size_of_image = u32_at(image, opt + 56) as u64;

    if !crate::mem::paging::is_user(base) || base == 0 {
        return Err(PeError::ImageBaseNotUser);
    }
    if !crate::mem::paging::is_user(base + size_of_image.saturating_sub(1)) {
        return Err(PeError::ImageBaseNotUser);
    }
    if size_of_image > MAX_IMAGE_SIZE {
        return Err(PeError::ImageTooLarge);
    }
    if entry_rva >= size_of_image {
        return Err(PeError::EntryOutsideImage);
    }

    // 数据目录：PE32+ 第 0 目录 = 导出，第 1 目录 = 导入。
    let dirs = opt + 112;
    let imp_rva = u32_at(image, dirs + 8);
    let imp_size = u32_at(image, dirs + 12);
    let import_dir = if imp_rva != 0 && imp_size != 0 {
        Some(ImportDir {
            rva: imp_rva,
            size: imp_size,
        })
    } else {
        None
    };

    let table = opt + opt_size;
    let table_end = table
        .checked_add(nsec * SECTION_HEADER_SIZE)
        .ok_or(PeError::BadSectionTable)?;
    if table_end > image.len() {
        return Err(PeError::BadSectionTable);
    }

    let mut out = PeImage {
        entry: base + entry_rva,
        image_base: base,
        size_of_image,
        import_dir,
        ..PeImage::default()
    };

    for i in 0..nsec {
        let h = table + i * SECTION_HEADER_SIZE;
        let vsize = u32_at(image, h + 8) as u64;
        let rva = u32_at(image, h + 12) as u64;
        let rawsz = u32_at(image, h + 16) as u64;
        let rawoff = u32_at(image, h + 20) as u64;
        let chars = u32_at(image, h + 36);
        if rva >= size_of_image {
            return Err(PeError::SectionOutOfRange);
        }
        // 节 VA 必须按 SectionAlignment（≥页）对齐——引擎按页落位，
        // 页中任意起始的段内偏移已支持，但节表本身必须页对齐。
        if sec_align >= 4096 && rva & (sec_align - 1) != 0 {
            return Err(PeError::SectionMisaligned);
        }
        // 文件内容必须整体落在映像内；VirtualSize 超过原始尺寸的部分是
        // 零填充（引擎的 BSS 零语义天然覆盖）。
        let file_end = rawoff.checked_add(rawsz).ok_or(PeError::SectionOutOfRange)?;
        if file_end > image.len() as u64 {
            return Err(PeError::SectionOutOfRange);
        }
        if rva + vsize > size_of_image {
            return Err(PeError::SectionOutOfRange);
        }
        out.sections[out.count] = PeSection {
            rva,
            filesz: rawsz.min(vsize),
            memsz: vsize,
            raw_offset: rawoff,
            writable: chars & SCN_MEM_WRITE != 0,
            executable: chars & SCN_CNT_CODE != 0 || chars & SCN_MEM_EXECUTE != 0,
        };
        out.count += 1;
    }
    if out.count == 0 {
        return Err(PeError::NoSections);
    }

    // 入口必须落在某个可执行节内（与 elf.rs 的 validate 同一口径）。
    let entry_rva_in = image
        .len()
        .checked_add(0)
        .map(|_| entry_rva)
        .unwrap_or(entry_rva);
    let _ = entry_rva_in;
    let covered = out
        .sections()
        .iter()
        .any(|s| s.executable && s.contains(entry_rva));
    if !covered {
        return Err(PeError::EntryNotExecutable);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 任务40（AI-B）· 导入表解析
//
// 任务39 把导入目录如实解析出来暴露在 [`PeImage::import_dir`]，本节兑现
// 承诺：把 IMAGE_IMPORT_DESCRIPTOR 数组、INT/IAT thunk 链与 IMAGE_BY_NAME
// 记录逐字段解析成结构化 [`ImportTable`]，绑定（registry 查找 + IAT 补钉）
// 在 `winapi.rs`。口径与 parse() 一致——解析器是决定"这批导入能不能绑"
// 的人，拒绝一切不能完全担保的形态；每个拒绝都带具名原因。
//
// 首层边界（如实声明，后续任务放宽）：
// * TimeDateStamp != 0（旧式 bound import）→ 具名拒绝（BoundImports），
//   我们永远fresh 解析，不信外部预绑定数据；
// * 序号导入照常解析（ImportFunc::ordinal），但首层注册表无序号项，
//   绑定期如实 MissingApi；
// * ForwarderChain 是老装载器的转发数据，本层不消费（照常解析不拒绝）。
// ---------------------------------------------------------------------------

/// 导入 DLL 名上限（"KERNEL32.DLL" 12 字节，32 足够且防恶意长名）。
pub const MAX_DLL_NAME: usize = 32;
/// 导入函数名上限。
pub const MAX_FUNC_NAME: usize = 64;
/// 单 DLL 导入函数数上限。
pub const MAX_IMPORT_FUNCS: usize = 64;
/// 导入 DLL 数上限。
pub const MAX_IMPORT_DLLS: usize = 8;
/// IMAGE_IMPORT_DESCRIPTOR 长度。
pub const IMPORT_DESCRIPTOR_SIZE: usize = 20;
/// thunk 序号导入标志（bit63）。
pub const THUNK_ORDINAL_FLAG: u64 = 0x8000_0000_0000_0000;
/// thunk 序号掩码（低 16 位）。
pub const THUNK_ORDINAL_MASK: u64 = 0xFFFF;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportError {
    NoImportDir,
    DescriptorRvaBad,
    DescriptorUnbounded,
    DllNameRvaBad,
    DllNameTooLong,
    DllNameNotTerminated,
    ThunkRvaBad,
    ThunkUnbounded,
    FuncNameRvaBad,
    FuncNameTooLong,
    FuncNameNotTerminated,
    IntIatMismatch,
    IatNotWritable,
    BoundImports,
    TooManyDlls,
    TooManyFuncs,
}

impl ImportError {
    pub fn as_str(self) -> &'static str {
        match self {
            ImportError::NoImportDir => "image declares no import directory",
            ImportError::DescriptorRvaBad => "import descriptor RVA does not map to file content",
            ImportError::DescriptorUnbounded => "import descriptor table runs past the file",
            ImportError::DllNameRvaBad => "DLL name RVA does not map to file content",
            ImportError::DllNameTooLong => "DLL name exceeds the length cap",
            ImportError::DllNameNotTerminated => "DLL name is not NUL-terminated within the file",
            ImportError::ThunkRvaBad => "thunk array RVA does not map to file content",
            ImportError::ThunkUnbounded => "thunk array runs past the file without a terminator",
            ImportError::FuncNameRvaBad => "function name RVA does not map to file content",
            ImportError::FuncNameTooLong => "function name exceeds the length cap",
            ImportError::FuncNameNotTerminated => "function name is not NUL-terminated within the file",
            ImportError::IntIatMismatch => "INT and IAT thunk chains disagree in length",
            ImportError::IatNotWritable => "IAT lives in a non-writable section (cannot patch)",
            ImportError::BoundImports => "old-style bound imports are refused (fresh resolution only)",
            ImportError::TooManyDlls => "import DLL count exceeds the cap",
            ImportError::TooManyFuncs => "import function count exceeds the cap",
        }
    }
}

/// 单个导入函数：名字导入（hint + name）或序号导入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImportFunc {
    pub name_len: usize,
    pub name: [u8; MAX_FUNC_NAME],
    pub ordinal: Option<u16>,
}

impl ImportFunc {
    const fn empty() -> ImportFunc {
        ImportFunc { name_len: 0, name: [0; MAX_FUNC_NAME], ordinal: None }
    }
}

/// 单个导入 DLL：名字 + INT/IAT thunk 链展开后的函数清单。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImportDll {
    pub name_len: usize,
    pub name: [u8; MAX_DLL_NAME],
    /// FirstThunk（IAT）RVA——补钉落点。
    pub iat_rva: u64,
    /// IAT 所在节可写（补钉前提）。
    pub iat_writable: bool,
    pub funcs: [ImportFunc; MAX_IMPORT_FUNCS],
    pub func_count: usize,
}

impl ImportDll {
    const fn empty() -> ImportDll {
        ImportDll {
            name_len: 0,
            name: [0; MAX_DLL_NAME],
            iat_rva: 0,
            iat_writable: false,
            funcs: [ImportFunc::empty(); MAX_IMPORT_FUNCS],
            func_count: 0,
        }
    }
}

/// 解析后的导入表（借用自由：名字拷入定长缓冲，无生命周期牵连）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImportTable {
    pub dlls: [ImportDll; MAX_IMPORT_DLLS],
    pub count: usize,
}

impl ImportTable {
    pub const fn empty() -> ImportTable {
        ImportTable { dlls: [ImportDll::empty(); MAX_IMPORT_DLLS], count: 0 }
    }

    pub fn dlls(&self) -> &[ImportDll] {
        &self.dlls[..self.count]
    }
}

/// RVA → 文件偏移。只认有文件内容的区间（VirtualSize 大于原始数据的
/// 零填充部分不是文件内容，映射 None）。
fn rva_to_offset(img: &PeImage, rva: u64) -> Option<usize> {
    let s = img.sections().iter().find(|s| rva >= s.rva && rva < s.rva + s.memsz)?;
    let delta = rva - s.rva;
    if delta >= s.filesz {
        return None;
    }
    Some((s.raw_offset + delta) as usize)
}

/// 节可写查询（IAT 补钉前提校验用）。
fn section_writable(img: &PeImage, rva: u64) -> bool {
    img.sections()
        .iter()
        .find(|s| rva >= s.rva && rva < s.rva + s.memsz)
        .map(|s| s.writable)
        .unwrap_or(false)
}

/// 读 NUL 结尾 ASCII 串（≤cap）：越文件界或超长都具名拒绝。
fn read_nul_name(image: &[u8], off: usize, cap: usize, long: ImportError, unterminated: ImportError) -> Result<([u8; MAX_FUNC_NAME], usize), ImportError> {
    // MAX_FUNC_NAME 是唯一调用方缓冲尺寸；cap 只能更小。
    let mut out = [0u8; MAX_FUNC_NAME];
    let mut i = 0usize;
    loop {
        let p = off + i;
        if p >= image.len() {
            return Err(unterminated);
        }
        let b = image[p];
        if b == 0 {
            return Ok((out, i));
        }
        if i >= cap {
            return Err(long);
        }
        out[i] = b;
        i += 1;
    }
}

/// 解析导入表到调用方提供的暂存（目标态安全路径：调用方给 .bss 静态，
/// 本函数栈上零大物化——函数清单位置写，最大局部量是单个 ImportFunc）。
/// `out` 必须先 `*out = ImportTable::empty()` 清底（调用方负责或由本函数
/// 清——本函数清，语义自洽）。
pub fn parse_imports_into(image: &[u8], img: &PeImage, out: &mut ImportTable) -> Result<(), ImportError> {
    *out = ImportTable::empty();
    let dir = img.import_dir.ok_or(ImportError::NoImportDir)?;

    // 描述符数组起点。
    let mut desc_off = rva_to_offset(img, dir.rva as u64).ok_or(ImportError::DescriptorRvaBad)?;
    loop {
        if desc_off + IMPORT_DESCRIPTOR_SIZE > image.len() {
            return Err(ImportError::DescriptorUnbounded);
        }
        let ofthunk = u32_at(image, desc_off) as u64;
        let timestamp = u32_at(image, desc_off + 4);
        let _forwarder = u32_at(image, desc_off + 8);
        let name_rva = u32_at(image, desc_off + 12) as u64;
        let first_thunk = u32_at(image, desc_off + 16) as u64;
        if ofthunk == 0 && timestamp == 0 && name_rva == 0 && first_thunk == 0 {
            break; // 全零终止描述符。
        }
        if timestamp != 0 {
            return Err(ImportError::BoundImports);
        }
        if out.count >= MAX_IMPORT_DLLS {
            return Err(ImportError::TooManyDlls);
        }

        // DLL 名。
        let name_off = rva_to_offset(img, name_rva).ok_or(ImportError::DllNameRvaBad)?;
        let (name, name_len) =
            read_nul_name(image, name_off, MAX_DLL_NAME, ImportError::DllNameTooLong, ImportError::DllNameNotTerminated)?;

        // thunk 链：bound 之外的映像 INT 与 IAT 内容一致（未解析 RVA）。
        let int_rva = if ofthunk != 0 { ofthunk } else { first_thunk };

        // INT 展开：函数清单位置直写，不设 64×80B 局部数组。
        out.dlls[out.count] = ImportDll::empty();
        let dll = &mut out.dlls[out.count];
        let mut nfuncs = 0usize;
        let mut toff = rva_to_offset(img, int_rva).ok_or(ImportError::ThunkRvaBad)?;
        loop {
            if toff + 8 > image.len() {
                return Err(ImportError::ThunkUnbounded);
            }
            let val = u64_at(image, toff);
            if val == 0 {
                break;
            }
            if nfuncs >= MAX_IMPORT_FUNCS {
                return Err(ImportError::TooManyFuncs);
            }
            if val & THUNK_ORDINAL_FLAG != 0 {
                dll.funcs[nfuncs] = ImportFunc {
                    name_len: 0,
                    name: [0; MAX_FUNC_NAME],
                    ordinal: Some((val & THUNK_ORDINAL_MASK) as u16),
                };
                nfuncs += 1;
            } else {
                let nb_off = rva_to_offset(img, val).ok_or(ImportError::FuncNameRvaBad)?;
                if nb_off + 2 > image.len() {
                    return Err(ImportError::FuncNameRvaBad);
                }
                let (fname, flen) = read_nul_name(
                    image,
                    nb_off + 2,
                    MAX_FUNC_NAME,
                    ImportError::FuncNameTooLong,
                    ImportError::FuncNameNotTerminated,
                )?;
                dll.funcs[nfuncs] = ImportFunc { name_len: flen, name: fname, ordinal: None };
                nfuncs += 1;
            }
            toff += 8;
        }
        if nfuncs == 0 {
            return Err(ImportError::IntIatMismatch);
        }

        // IAT：必须在有文件内容的位置，且与 INT 等长。
        let iat_off = rva_to_offset(img, first_thunk).ok_or(ImportError::ThunkRvaBad)?;
        let mut iat_len = 0usize;
        let mut ioff = iat_off;
        loop {
            if ioff + 8 > image.len() {
                return Err(ImportError::ThunkUnbounded);
            }
            if u64_at(image, ioff) == 0 {
                break;
            }
            iat_len += 1;
            ioff += 8;
            if iat_len > MAX_IMPORT_FUNCS {
                return Err(ImportError::TooManyFuncs);
            }
        }
        if iat_len != nfuncs {
            return Err(ImportError::IntIatMismatch);
        }
        if !section_writable(img, first_thunk) {
            return Err(ImportError::IatNotWritable);
        }

        dll.name[..name_len].copy_from_slice(&name[..name_len]);
        dll.name_len = name_len;
        dll.iat_rva = first_thunk;
        dll.iat_writable = true;
        dll.func_count = nfuncs;
        out.count += 1;
        desc_off += IMPORT_DESCRIPTOR_SIZE;
    }
    Ok(())
}

/// 解析导入表（返回值版）。**宿主/测试专用**——返回值在调用方栈上物化
/// ≈41KB，目标态 64KB 内核栈装不下两层（见 [`ImportTable`] 栈纪律）。
pub fn parse_imports(image: &[u8], img: &PeImage) -> Result<ImportTable, ImportError> {
    let mut out = ImportTable::empty();
    parse_imports_into(image, img, &mut out)?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// 装载适配器：PeSource → 任务15 引擎
// ---------------------------------------------------------------------------

/// PE → [`super::loader::ImageFormat`]。offset 以 ImageBase 归零后的
/// 文件偏移直接给出（节的 raw_offset 就是 blob 内偏移）。
pub struct PeSource<'a> {
    pub img: &'a PeImage,
    pub blob: &'a [u8],
}

impl super::loader::ImageFormat for PeSource<'_> {
    fn entry(&self) -> u64 {
        self.img.entry
    }
    fn blob(&self) -> &[u8] {
        self.blob
    }
    fn for_each_segment(&self, f: &mut dyn FnMut(SegmentView)) {
        for s in self.img.sections() {
            f(SegmentView {
                // 节映射在 ImageBase + RVA（pe.rs 不做重定位：静态映像
                // 按首选基址装载；任务40 引入动态映像时再上 reloc）。
                vaddr: self.img.image_base + s.rva,
                offset: s.raw_offset,
                filesz: s.filesz,
                memsz: s.memsz,
                writable: s.writable,
                // W^X：可写节强制 NX（与 ElfSource 同口径）。
                executable: s.executable && !s.writable,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// 宿主测试：真实样例解析 + 敌意头 + 引擎端到端装载
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::loader::{load_into, release_pages, ImageFormat, UserMapper, PAGE};

    /// 静态 PE 样例（tools/make-pe.py 生成，随源入库）：
    /// write(1,"hello from PE\n",14) 后 exit(0)。
    static HELLO_PE: &[u8] = include_bytes!("hello.pe");

    const BASE: u64 = 0x1_4000_0000;
    const TEXT_RVA: u64 = 0x1000;
    const MSG_RVA: u64 = TEXT_RVA + 0x40;
    const ENTRY: u64 = BASE + TEXT_RVA;
    const MSG_VA: u64 = BASE + MSG_RVA;

    /// 宿主假内存（与 loader.rs 测试同一模型，独立成套免跨模块依赖）。
    struct HostMapper {
        map: Vec<(u64, u64, bool, bool)>,
        frames: Vec<[u8; PAGE as usize]>,
        freed: Vec<u64>,
    }
    impl HostMapper {
        fn new() -> Self {
            HostMapper {
                map: Vec::new(),
                frames: Vec::new(),
                freed: Vec::new(),
            }
        }
        fn read_va(&self, va: u64, n: usize) -> Vec<u8> {
            let (_, phys, _, _) = *self.map.iter().find(|e| e.0 == va & !0xFFF).unwrap();
            let in_page = (va & 0xFFF) as usize;
            let idx = ((phys - 0x1000_0000) / PAGE) as usize;
            self.frames[idx][in_page..in_page + n].to_vec()
        }
        fn flags_of(&self, va: u64) -> Option<(bool, bool)> {
            self.map
                .iter()
                .find(|e| e.0 == va & !0xFFF)
                .map(|e| (e.2, e.3))
        }
    }
    impl UserMapper for HostMapper {
        fn alloc_zero_frame(&mut self) -> Option<u64> {
            let phys = 0x1000_0000 + (self.frames.len() as u64) * PAGE;
            self.frames.push([0u8; PAGE as usize]);
            Some(phys)
        }
        fn map_user_frame(&mut self, va: u64, phys: u64, w: bool, nx: bool) -> bool {
            if va >> 47 != 0 {
                return false;
            }
            match self.map.iter_mut().find(|e| e.0 == va) {
                Some(e) => *e = (va, phys, w, nx),
                None => self.map.push((va, phys, w, nx)),
            }
            true
        }
        fn write_frame_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            let idx = ((phys - 0x1000_0000) / PAGE) as usize;
            self.frames[idx][off as usize..off as usize + data.len()].copy_from_slice(data);
        }
        fn flush(&mut self, _va: u64) {}
        fn unmap_user(&mut self, va: u64) -> Option<u64> {
            let i = self.map.iter().position(|e| e.0 == va)?;
            Some(self.map.remove(i).1)
        }
        fn free_frame(&mut self, phys: u64) {
            self.freed.push(phys);
        }
    }

    /// 直接构造单节 PE 的头（测试敌意变体的底版）。
    /// layout: DOS(0x40) e_lfanew=0x40, PE sig@0x40, COFF, opt(0xF0), 1 节。
    fn synth_pe(entry_rva: u32, base: u64, rawsz: u32, chars: u32) -> Vec<u8> {
        let mut img = vec![0u8; 0x400];
        img[0..2].copy_from_slice(&MZ_MAGIC);
        img[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        img[0x40..0x44].copy_from_slice(&PE_MAGIC);
        img[0x44..0x46].copy_from_slice(&MACHINE_AMD64.to_le_bytes());
        img[0x46..0x48].copy_from_slice(&1u16.to_le_bytes());
        img[0x54..0x56].copy_from_slice(&0xF0u16.to_le_bytes());
        img[0x56..0x58].copy_from_slice(&0x22u16.to_le_bytes());
        let opt = 0x58;
        img[opt..opt + 2].copy_from_slice(&OPTIONAL_MAGIC_PE32P.to_le_bytes());
        img[opt + 16..opt + 20].copy_from_slice(&entry_rva.to_le_bytes());
        img[opt + 24..opt + 32].copy_from_slice(&base.to_le_bytes());
        img[opt + 32..opt + 36].copy_from_slice(&0x1000u32.to_le_bytes()); // SecAlign
        img[opt + 36..opt + 40].copy_from_slice(&0x200u32.to_le_bytes()); // FileAlign
        img[opt + 56..opt + 60].copy_from_slice(&0x2000u32.to_le_bytes()); // SizeOfImage
        img[opt + 60..opt + 64].copy_from_slice(&0x200u32.to_le_bytes()); // SizeOfHeaders
        let sh = opt + 0xF0;
        let name = b".text\0\0\0";
        img[sh..sh + 8].copy_from_slice(name);
        img[sh + 8..sh + 12].copy_from_slice(&0x200u32.to_le_bytes()); // VirtualSize
        img[sh + 12..sh + 16].copy_from_slice(&0x1000u32.to_le_bytes()); // VirtualAddress
        img[sh + 16..sh + 20].copy_from_slice(&rawsz.to_le_bytes()); // SizeOfRawData
        img[sh + 20..sh + 24].copy_from_slice(&0x200u32.to_le_bytes()); // PointerToRawData
        img[sh + 36..sh + 40].copy_from_slice(&chars.to_le_bytes());
        // 代码与消息照真实样例放（敌意测试只改头，不改内容）。
        let code: [u8; 36] = [
            0x48, 0xC7, 0xC0, 0x02, 0, 0, 0, 0x48, 0xC7, 0xC7, 0x01, 0, 0, 0, 0x48, 0x8D, 0x35,
            0x2B, 0, 0, 0, 0x48, 0xC7, 0xC2, 0x0E, 0, 0, 0, 0x0F, 0x05, 0x31, 0xC0, 0x31, 0xFF,
            0x0F, 0x05,
        ];
        img[0x200..0x200 + 36].copy_from_slice(&code);
        img[0x240..0x240 + 14].copy_from_slice(b"hello from PE\n");
        img
    }

    #[test]
    fn hello_pe_parses_and_validates() {
        let img = parse(HELLO_PE).expect("hello.pe 必须可解析");
        assert_eq!(img.entry, ENTRY);
        assert_eq!(img.image_base, BASE);
        assert_eq!(img.count, 1);
        let s = img.sections()[0];
        assert_eq!(s.rva, TEXT_RVA);
        // filesz = min(SizeOfRawData=0x200, VirtualSize=114) = 114。
        assert_eq!(s.filesz, 114);
        assert!(s.executable);
        assert!(!s.writable);
        assert!(!img.has_imports(), "静态样例必须无导入");
        assert!(s.contains(TEXT_RVA));
        assert!(s.contains(MSG_RVA));
    }

    #[test]
    fn hello_pe_loads_through_the_task15_engine() {
        let img = parse(HELLO_PE).unwrap();
        let src = PeSource {
            img: &img,
            blob: HELLO_PE,
        };
        let mut mp = HostMapper::new();
        let loaded =
            load_into(&src, &mut mp, 2, 0x7FFF_F000).expect("静态 PE 必须能走任务15 引擎");
        assert_eq!(loaded.entry, ENTRY);

        // 入口首条指令 = mov rax,2（SYS_WRITE）——逐字节核对。
        let code = mp.read_va(ENTRY, 8);
        assert_eq!(&code[..], &[0x48, 0xC7, 0xC0, 0x02, 0, 0, 0, 0x48]);

        // 消息串精确落位在 0x140001040。
        let msg = mp.read_va(MSG_VA, 14);
        assert_eq!(&msg[..], b"hello from PE\n");

        // flags：R|X 节 → W=0 NX=0；栈页 W=1 NX=1。
        assert_eq!(mp.flags_of(ENTRY), Some((false, false)));
        assert_eq!(mp.flags_of(0x7FFF_E000), Some((true, true)));

        // 1 节页 + 2 栈页 = 3 帧。
        assert_eq!(loaded.frames_used, 3);
        // 全量回收：摘叶归还一页不漏，重复回收为 0。
        let n = loaded.pages.len();
        assert_eq!(release_pages(&loaded.pages, &mut mp), n);
        assert_eq!(mp.freed.len(), n);
        assert_eq!(release_pages(&loaded.pages, &mut mp), 0);
    }

    #[test]
    fn import_dir_is_parsed_but_not_resolved() {
        // 复刻带导入目录的映像：数据目录[1] 非零 → has_imports()=true，
        // 但任务39 照常装载（导入是数据，不是装载期依赖）。
        let mut img = synth_pe(0x1000, 0x140000000, 0x200, 0x6000_0020);
        let dirs = 0x58 + 112;
        img[dirs + 8..dirs + 12].copy_from_slice(&0x2000u32.to_le_bytes()); // 导入 RVA
        img[dirs + 12..dirs + 16].copy_from_slice(&0x28u32.to_le_bytes()); // 导入 size
        let p = parse(&img).unwrap();
        assert_eq!(p.import_dir, Some(ImportDir { rva: 0x2000, size: 0x28 }));
        assert!(p.has_imports());
        // 装载照常通过（边界声明在模块头：运行期 thunk 未解析属任务40）。
        let src = PeSource { img: &p, blob: &img };
        let mut mp = HostMapper::new();
        assert!(load_into(&src, &mut mp, 1, 0x7FFF_F000).is_ok());
    }

    #[test]
    fn hostile_headers_are_refused_by_name() {
        let good = synth_pe(0x1000, 0x140000000, 0x200, 0x6000_0020);

        let mut bad = good.clone();
        bad[0] = b'X';
        assert_eq!(parse(&bad).unwrap_err(), PeError::BadDosMagic);

        let mut bad = good.clone();
        bad[0x3C..0x40].copy_from_slice(&0x10_0000u32.to_le_bytes());
        assert_eq!(parse(&bad).unwrap_err(), PeError::BadPeOffset);

        let mut bad = good.clone();
        bad[0x40] = b'X';
        assert_eq!(parse(&bad).unwrap_err(), PeError::BadPeMagic);

        let mut bad = good.clone();
        bad[0x44..0x46].copy_from_slice(&0x14Cu16.to_le_bytes()); // i386
        assert_eq!(parse(&bad).unwrap_err(), PeError::WrongMachine);

        let mut bad = good.clone();
        bad[0x58..0x5A].copy_from_slice(&0x10Bu16.to_le_bytes()); // PE32（可选头@0x58）
        assert_eq!(parse(&bad).unwrap_err(), PeError::NotPe32Plus);

        let mut bad = good.clone();
        bad[0x46..0x48].copy_from_slice(&0u16.to_le_bytes()); // 0 节
        assert_eq!(parse(&bad).unwrap_err(), PeError::NoSections);

        let mut bad = good.clone();
        bad[0x3C..0x40].copy_from_slice(&0x3Fu32.to_le_bytes()); // < DOS 头
        assert_eq!(parse(&bad).unwrap_err(), PeError::BadPeOffset);

        // 内核半区 ImageBase → 拒。
        let kern = synth_pe(0x1000, 0xFFFF_8000_0000_0000, 0x200, 0x6000_0020);
        assert_eq!(parse(&kern).unwrap_err(), PeError::ImageBaseNotUser);

        // 入口不在可执行节内（落在节外 RVA）→ 拒。
        let stray = synth_pe(0x1800, 0x140000000, 0x200, 0x6000_0020);
        assert_eq!(parse(&stray).unwrap_err(), PeError::EntryNotExecutable);

        // 入口在非执行节（数据节）→ 拒。
        let data = synth_pe(0x1000, 0x140000000, 0x200, 0xC000_0040);
        assert_eq!(parse(&data).unwrap_err(), PeError::EntryNotExecutable);

        // 节原始内容越过 blob → 拒。
        let big = synth_pe(0x1000, 0x140000000, 0x1000, 0x6000_0020);
        assert_eq!(parse(&big).unwrap_err(), PeError::SectionOutOfRange);

        // 节 VA 未按 SectionAlignment 对齐 → 拒。
        let mut unaligned = synth_pe(0x1000, 0x140000000, 0x200, 0x6000_0020);
        let sh = 0x58 + 0xF0;
        unaligned[sh + 12..sh + 16].copy_from_slice(&0x1001u32.to_le_bytes());
        assert_eq!(parse(&unaligned).unwrap_err(), PeError::SectionMisaligned);

        assert_eq!(parse(&[]).unwrap_err(), PeError::TooSmall);
        assert_eq!(parse(&[0u8; 8]).unwrap_err(), PeError::TooSmall);
    }

    #[test]
    fn writable_section_is_mapped_writable_and_nx() {
        // W^X：可写数据节 → writable=true 且映射层 executable=false。
        let img = synth_pe(0x1000, 0x140000000, 0x200, 0x6000_0020);
        let mut p = parse(&img).unwrap();
        p.sections[0].writable = true;
        let segs: Vec<SegmentView> = {
            let src = PeSource { img: &p, blob: &img };
            let mut v = Vec::new();
            src.for_each_segment(&mut |s| v.push(s));
            v
        };
        assert_eq!(segs.len(), 1);
        assert!(segs[0].writable);
        assert!(!segs[0].executable, "可写节必须强制 NX");
    }
}
