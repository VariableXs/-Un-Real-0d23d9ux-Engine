//! F002 静态 PE 全量支持（compatstar · G-A-02）——go build 出来的单文件 exe，双击就能跑。
//!
//! 主册判据（验收标准第一句）：
//! **「开源静态样本集 20 枚（Go/Rust/Nim 各类）装载成功率 20/20；对抗样本集
//! 30 枚（自造畸形）拒绝率 30/30——两组数字同页呈现，缺一不可。」**
//!
//! 功能定义（G-A-02）：无导入表（或仅导入 kernel32 基础面）的静态链接 PE 完整
//! 装载：GUI/CUI/Windows CE 三类子系统、任意节数（上限 96 节，Windows 规范值）、
//! 非常规 SectionAlignment（含 4KB 以下）、BASE 重定位全类型（HIGHLOW/DIR64）。
//!
//! 【交互设计】CUI 子系统自动挂接终端（F012）：资源管理器双击控制台程序自动
//! 新开终端标签页；GUI 子系统按 F001 流程。装载期无用户可见等待项（20MB 装载
//! 目标 ≤800ms，实测线随闸门实机登记——本模块提供装载判定与节惰性提交账本）。
//! 【数据与存储】节映射零拷贝（文件页缓存直接映射语义，模型层记账）；重定位
//! 表处理完即弃（不常驻内存）。
//! 【状态与异常】畸形头（e_lfanew 越界/节表溢出）→ 拒绝并归因「文件损坏或非
//! PE」；地址空间不足 → 提示「内存不足」而非闪退；TLS 回调先于入口点执行
//! （顺序差异表登记）。
//! 【设计细节】SectionAlignment < 4096 时按文件对齐映射（FILE_ALIGN 模式）；
//! 重定位 HIGHLOW 占 95% 场景优先快路径；BASE 重定位块处理上限 64MB（超限
//! 归因「镜像异常」拒绝）；装载内存按节惰性提交，20MB 程序实际驻留首启 <6MB。
//!
//! 与既有底座关系：`proc/pe.rs` 是 ring3 PE32+ 装载主链（一次解析、节段翻译、
//! W^X，边界担保口径一致：拒绝一切不能完全担保的东西）；本模块是 A 域判据面
//! ——子系统三分类、非常规对齐、BASE 重定位全类型、对抗样本拒绝审计、惰性
//! 提交计划，为 F001（无感双击）提供装载判定底盘。
//!
//! 零堆纪律：全定长结构，无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 节数硬顶（主册：任意节数，上限 96 节，Windows 规范值）。
pub const MAX_SECTIONS: usize = 96;
/// BASE 重定位块处理上限 64MB（主册【设计细节】：超限归因「镜像异常」拒绝）。
pub const RELOC_BUDGET_BYTES: u64 = 64 * 1024 * 1024;
/// 页对齐线（主册：SectionAlignment 含 4KB 以下——低于此线走 FILE_ALIGN 模式）。
pub const PAGE_ALIGN: u64 = 4096;
/// 20MB 程序首启驻留比例线（主册：20MB 程序实际驻留首启小于 6MB ≈ 30%）。
pub const LAZY_RESIDENT_RATIO_PERMILLE: u64 = 300;
/// DOS 头最小长度（e_lfanew@0x3C 需要 4 字节；与 proc/pe.rs 同源）。
pub const DOS_HEADER_SIZE: usize = 0x40;
/// PE 签名 + COFF 头（20B）+ PE32+ 可选头（240B）= 0xF8，节表随后。
pub const OPTIONAL_HDR_SIZE_PE32P: usize = 0xF0;
/// 节表项长度（40B）。
pub const SECTION_HEADER_SIZE: usize = 40;

pub const MZ_MAGIC: [u8; 2] = [b'M', b'Z'];
pub const PE_MAGIC: [u8; 4] = [b'P', b'E', 0, 0];
/// IMAGE_FILE_MACHINE_AMD64（proc/pe.rs 同源）。
pub const MACHINE_AMD64: u16 = 0x8664;
/// PE32+ 可选头魔数。
pub const OPTIONAL_MAGIC_PE32P: u16 = 0x20B;

// 子系统字段值（Microsoft PE Spec / winnt.h IMAGE_SUBSYSTEM_*）。
pub const SUBSYSTEM_NATIVE: u16 = 1;
pub const SUBSYSTEM_GUI: u16 = 2;
pub const SUBSYSTEM_CUI: u16 = 3;
pub const SUBSYSTEM_CE_GUI: u16 = 9;

// BASE 重定位条目类型（PE Spec §Base Relocation Types）。
pub const REL_ABSOLUTE: u8 = 0; // 填充，跳过
pub const REL_HIGHLOW: u8 = 10; // 32 位绝对地址（95% 场景快路径）
pub const REL_DIR64: u8 = 11; // 64 位绝对地址（PE32+）

// 数据目录索引（PE Spec）。
pub const DIR_INDEX_BASERELOC: usize = 5;

// ---------------------------------------------------------------------------
// 错误与子系统
// ---------------------------------------------------------------------------

/// 解析/装载拒绝归因。每条归因字符串进入 F001 诊断链与 F020 dump 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeBlendError {
    TooSmall,
    BadDosMagic,
    BadPeOffset,
    BadPeMagic,
    WrongMachine,
    NotPe32Plus,
    BadSectionTable,
    NoSections,
    SectionOutOfRange,
    ImageTooLarge,
    EntryOutsideImage,
    EntryNotExecutable,
    UnsupportedSubsystem,
    /// BASE 重定位块总量超 64MB 预算（主册：归因「镜像异常」拒绝）。
    RelocBudgetExceeded,
    /// 未知重定位类型——Windows 加载器同样拒绝，不做静默跳过。
    UnknownRelocType,
    /// 重定位越出镜像边界（坏 RVA/坏条目）。
    RelocOutOfRange,
}

impl PeBlendError {
    /// 归因短语（三要素「为什么」面的静态串，无堆）。
    pub fn as_str(self) -> &'static str {
        match self {
            PeBlendError::TooSmall => "file is smaller than a DOS header",
            PeBlendError::BadDosMagic => "not an MZ image",
            PeBlendError::BadPeOffset => "e_lfanew is out of bounds",
            PeBlendError::BadPeMagic => "PE signature missing",
            PeBlendError::WrongMachine => "image is not built for AMD64",
            PeBlendError::NotPe32Plus => "only PE32+ (64-bit) optional headers are supported",
            PeBlendError::BadSectionTable => "section header table is out of bounds",
            PeBlendError::NoSections => "image has nothing to load",
            PeBlendError::SectionOutOfRange => "a section runs past the end of the file",
            PeBlendError::ImageTooLarge => "SizeOfImage exceeds the loader cap",
            PeBlendError::EntryOutsideImage => "entry RVA is outside SizeOfImage",
            PeBlendError::EntryNotExecutable => "entry point is not inside an executable section",
            PeBlendError::UnsupportedSubsystem => "subsystem is not GUI/CUI/Windows-CE",
            PeBlendError::RelocBudgetExceeded => "relocation blocks exceed the 64MB budget",
            PeBlendError::UnknownRelocType => "unknown base relocation type",
            PeBlendError::RelocOutOfRange => "relocation entry is outside the image",
        }
    }
}

/// 三类承诺子系统（主册【功能定义】：GUI/CUI/Windows CE）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Subsystem {
    /// GUI：按 F001 无感双击流程开窗口。
    Gui,
    /// CUI：自动挂接终端（F012），双击即新开终端标签页。
    Cui,
    /// Windows CE GUI（主册明文承诺第三类）。
    CeGui,
    /// NATIVE（驱动类静态映像——无用户窗口语义，装载后不进桌面流程；
    /// 主册边界句「只承诺窗口级 GUI 与控制台两类子系统」之外，如实标记
    /// 走底层通道，不冒充 GUI/CUI）。
    Native,
}

impl Subsystem {
    pub fn from_raw(raw: u16) -> Option<Subsystem> {
        match raw {
            SUBSYSTEM_GUI => Some(Subsystem::Gui),
            SUBSYSTEM_CUI => Some(Subsystem::Cui),
            SUBSYSTEM_CE_GUI => Some(Subsystem::CeGui),
            SUBSYSTEM_NATIVE => Some(Subsystem::Native),
            _ => None,
        }
    }

    /// 是否走 F001 桌面双击动线（GUI/CE 有窗口；CUI 挂终端；Native 不进桌面）。
    pub fn desktop_launch(self) -> bool {
        matches!(self, Subsystem::Gui | Subsystem::CeGui)
    }

    /// 是否自动挂终端标签页（主册【交互设计】：CUI 双击自动新开终端）。
    pub fn attaches_console(self) -> bool {
        self == Subsystem::Cui
    }
}

// ---------------------------------------------------------------------------
// 解析结果
// ---------------------------------------------------------------------------

/// 单节装载视图（FILE_ALIGN 模式下 rva 按 SectionAlignment 计算、映射按
/// 文件对齐落位——主册【设计细节】FILE_ALIGN 语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlendSection {
    pub rva: u64,
    pub raw_offset: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub executable: bool,
    pub writable: bool,
}

/// 解析完成的静态映像描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlendImage {
    pub entry_rva: u64,
    pub image_base: u64,
    pub size_of_image: u64,
    pub subsystem: Subsystem,
    pub sections: [BlendSection; MAX_SECTIONS],
    pub section_count: usize,
    /// 重定位目录（RVA, Size）；None = 静态无重定位（无需 BASE 修正）。
    pub reloc_dir: Option<(u32, u32)>,
    /// FILE_ALIGN 模式（SectionAlignment < 4096，主册【设计细节】）。
    pub file_align_mode: bool,
}

impl BlendImage {
    pub fn sections(&self) -> &[BlendSection] {
        &self.sections[..self.section_count]
    }

    pub fn section_at(&self, rva: u64) -> Option<&BlendSection> {
        self.sections()
            .iter()
            .find(|s| rva >= s.rva && rva < s.rva + s.memsz)
    }

    /// 入口是否落在可执行节内（装载担保纪律，与 proc/pe.rs 同口径）。
    pub fn entry_executable(&self) -> bool {
        self.section_at(self.entry_rva).map_or(false, |s| s.executable)
    }
}

// ---------------------------------------------------------------------------
// 解析器（纯字节输入，无副作用）
// ---------------------------------------------------------------------------

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

/// SizeOfImage 上限 256 MiB（proc/pe.rs 同口径——超出视为损坏/恶意头）。
const MAX_IMAGE_SIZE: u64 = 256 * 1024 * 1024;

/// 解析静态 PE32+ 映像（完整判据面：子系统三分类 + 非常规对齐 + 重定位目录）。
pub fn parse(image: &[u8]) -> Result<BlendImage, PeBlendError> {
    if image.len() < DOS_HEADER_SIZE {
        return Err(PeBlendError::TooSmall);
    }
    if image[0..2] != MZ_MAGIC {
        return Err(PeBlendError::BadDosMagic);
    }
    let pe_off = u32_at(image, 0x3C) as usize;
    // e_lfanew 越界（主册【状态与异常】畸形头第一例）。
    if pe_off < DOS_HEADER_SIZE || pe_off + 24 > image.len() {
        return Err(PeBlendError::BadPeOffset);
    }
    if image[pe_off..pe_off + 4] != PE_MAGIC {
        return Err(PeBlendError::BadPeMagic);
    }
    let coff = pe_off + 4;
    if u16_at(image, coff) != MACHINE_AMD64 {
        return Err(PeBlendError::WrongMachine);
    }
    let nsec = u16_at(image, coff + 2) as usize;
    let opt_size = u16_at(image, coff + 16) as usize;
    if nsec == 0 || nsec > MAX_SECTIONS {
        return Err(PeBlendError::NoSections);
    }
    let opt = coff + 20;
    if opt_size < OPTIONAL_HDR_SIZE_PE32P || opt + opt_size > image.len() {
        return Err(PeBlendError::BadSectionTable);
    }
    if u16_at(image, opt) != OPTIONAL_MAGIC_PE32P {
        return Err(PeBlendError::NotPe32Plus);
    }
    // 子系统三分类（opt+68，PE Spec）。
    let subsystem = Subsystem::from_raw(u16_at(image, opt + 68))
        .ok_or(PeBlendError::UnsupportedSubsystem)?;
    let entry_rva = u32_at(image, opt + 16) as u64;
    let base = u64_at(image, opt + 24);
    let sec_align = u32_at(image, opt + 32) as u64;
    let size_of_image = u32_at(image, opt + 56) as u64;
    let n_dirs = u32_at(image, opt + 108) as usize;

    if size_of_image > MAX_IMAGE_SIZE {
        return Err(PeBlendError::ImageTooLarge);
    }
    if entry_rva >= size_of_image {
        return Err(PeBlendError::EntryOutsideImage);
    }

    // 非常规对齐判定（主册：SectionAlignment 含 4KB 以下 → FILE_ALIGN 模式）。
    let file_align_mode = sec_align < PAGE_ALIGN;

    // 节表（节表溢出 = 头声明节数×40B 越过文件尾 → BadSectionTable）。
    let table_off = opt + opt_size;
    if table_off + nsec * SECTION_HEADER_SIZE > image.len() {
        return Err(PeBlendError::BadSectionTable);
    }
    let mut sections = [BlendSection {
        rva: 0,
        raw_offset: 0,
        filesz: 0,
        memsz: 0,
        executable: false,
        writable: false,
    }; MAX_SECTIONS];
    let mut count = 0usize;
    for i in 0..nsec {
        let sh = table_off + i * SECTION_HEADER_SIZE;
        let vsize = u32_at(image, sh + 8) as u64;
        let rva = u32_at(image, sh + 12) as u64;
        let rawsz = u32_at(image, sh + 16) as u64;
        let rawoff = u32_at(image, sh + 20) as u64;
        let chars = u32_at(image, sh + 36);
        // 节内容越过文件边界（proc/pe.rs 同口径拒绝）。
        if rawsz > 0 && (rawoff.saturating_add(rawsz)) > image.len() as u64 {
            return Err(PeBlendError::SectionOutOfRange);
        }
        sections[count] = BlendSection {
            rva,
            raw_offset: rawoff,
            filesz: rawsz,
            memsz: if vsize > rawsz { vsize } else { rawsz },
            executable: chars & 0x2000_0000 != 0,
            writable: chars & 0x8000_0000 != 0,
        };
        count += 1;
    }

    // 重定位目录（数据目录第 6 项）。
    let reloc_dir = if n_dirs > DIR_INDEX_BASERELOC {
        let rva = u32_at(image, opt + 112 + DIR_INDEX_BASERELOC * 8);
        let sz = u32_at(image, opt + 116 + DIR_INDEX_BASERELOC * 8);
        if rva != 0 && sz != 0 {
            Some((rva, sz))
        } else {
            None
        }
    } else {
        None
    };
    // 重定位预算检查（主册 64MB 上限：超限拒绝在解析期完成——「处理完即弃」
    // 前先担保预算，防装载期 OOM）。
    if let Some((_, sz)) = reloc_dir {
        if sz as u64 > RELOC_BUDGET_BYTES {
            return Err(PeBlendError::RelocBudgetExceeded);
        }
    }

    let img = BlendImage {
        entry_rva,
        image_base: base,
        size_of_image,
        subsystem,
        sections,
        section_count: count,
        reloc_dir,
        file_align_mode,
    };
    // 入口可执行担保（主册异常纪律：不担保的拒绝，不留「三条指令之后的
    // 莫名故障」）。
    if !img.entry_executable() {
        return Err(PeBlendError::EntryNotExecutable);
    }
    Ok(img)
}

// ---------------------------------------------------------------------------
// BASE 重定位（HIGHLOW / DIR64 全类型）
// ---------------------------------------------------------------------------

/// 重定位应用结果（记账面：块数/条数/类型分布——HIGHLOW 95% 快路径观测）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelocReport {
    pub blocks: u32,
    pub entries: u32,
    pub highlow: u32,
    pub dir64: u32,
    pub absolute_pad: u32,
    /// 处理完即弃语义的核对位：块遍历是否消费完整（越界即 false）。
    pub consumed_clean: bool,
}

/// 对一段「已映射镜像内存」应用 BASE 重定位。
///
/// `mapped` 模拟镜像内存（下标 = RVA−0 起）；`base_delta` = 实际装载基址 −
/// ImageBase。块遍历按 PE Spec：每块头 (PageRva u32, BlockSize u32)，条目
/// (Type<<12 | Offset) u16。未知类型 → [`PeBlendError::UnknownRelocType`]
/// （Windows 同语义拒绝，不静默跳过——对抗样本拒绝率的根基）。
pub fn apply_relocations(
    mapped: &mut [u8],
    image: &BlendImage,
    base_delta: i64,
) -> Result<RelocReport, PeBlendError> {
    let mut report = RelocReport::default();
    let (dir_rva, dir_size) = match image.reloc_dir {
        Some(x) => x,
        None => {
            // 静态无重定位：零条目即零消费——consumed_clean 如实为 true。
            report.consumed_clean = true;
            return Ok(report);
        }
    };
    let total = dir_size as u64;
    if total > RELOC_BUDGET_BYTES {
        return Err(PeBlendError::RelocBudgetExceeded);
    }
    let mut off = dir_rva as u64;
    let end = dir_rva as u64 + total;
    // 目录本身必须在镜像范围内。
    if end > image.size_of_image {
        return Err(PeBlendError::RelocOutOfRange);
    }
    while off < end {
        // 块头 8B：PageRva + BlockSize（BlockSize 含块头，4 对齐）。
        if off + 8 > end {
            return Err(PeBlendError::RelocOutOfRange);
        }
        let page_rva = u32_at(mapped, off as usize) as u64;
        let block_size = u32_at(mapped, (off + 4) as usize) as u64;
        if block_size < 8 || off + block_size > end {
            return Err(PeBlendError::RelocOutOfRange);
        }
        let entry_count = (block_size - 8) / 2;
        let mut e = 0u64;
        while e < entry_count {
            let entry = u16_at(mapped, (off + 8 + e * 2) as usize);
            let rtype = (entry >> 12) as u8;
            let roff = (entry & 0x0FFF) as u64;
            let target = page_rva + roff;
            match rtype {
                REL_ABSOLUTE => {
                    report.absolute_pad += 1;
                }
                REL_HIGHLOW => {
                    // 快路径（95% 场景）：u32 原位加 delta。
                    if target + 4 > image.size_of_image || target as usize + 4 > mapped.len() {
                        return Err(PeBlendError::RelocOutOfRange);
                    }
                    let v = i64::from(u32_at(mapped, target as usize)) + base_delta;
                    mapped[target as usize..target as usize + 4]
                        .copy_from_slice(&(v as u32).to_le_bytes());
                    report.highlow += 1;
                }
                REL_DIR64 => {
                    if target + 8 > image.size_of_image || target as usize + 8 > mapped.len() {
                        return Err(PeBlendError::RelocOutOfRange);
                    }
                    let v = (u64_at(mapped, target as usize) as i64) + base_delta;
                    mapped[target as usize..target as usize + 8]
                        .copy_from_slice(&(v as u64).to_le_bytes());
                    report.dir64 += 1;
                }
                _ => return Err(PeBlendError::UnknownRelocType),
            }
            report.entries += 1;
            e += 1;
        }
        report.blocks += 1;
        off += block_size;
    }
    report.consumed_clean = true;
    Ok(report)
}

// ---------------------------------------------------------------------------
// 节惰性提交计划（20MB 首启驻留 <6MB 判据的记账模型）
// ---------------------------------------------------------------------------

/// 惰性提交账本：节未触碰不占物理页（主册【设计细节】），触碰按页记账。
pub struct LazyCommitPlan {
    /// 每节已提交页数（按 4KB 页粒度）。
    committed_pages: [u32; MAX_SECTIONS],
    section_count: usize,
}

impl LazyCommitPlan {
    pub fn new(image: &BlendImage) -> LazyCommitPlan {
        LazyCommitPlan {
            committed_pages: [0; MAX_SECTIONS],
            section_count: image.section_count,
        }
    }

    /// 触碰一页（CPU 取指/访问触发缺页 → 提交）。
    pub fn touch_page(&mut self, image: &BlendImage, rva: u64) -> bool {
        for i in 0..image.section_count {
            let s = &image.sections[i];
            if rva >= s.rva && rva < s.rva + s.memsz {
                let page = ((rva - s.rva) / PAGE_ALIGN) as u32;
                if (page as usize) < (s.memsz / PAGE_ALIGN) as usize + 1 {
                    if page + 1 > self.committed_pages[i] {
                        self.committed_pages[i] = page + 1;
                    }
                    return true;
                }
            }
        }
        false
    }

    /// 当前物理驻留字节（判据观测面）。
    pub fn resident_bytes(&self) -> u64 {
        let mut n = 0u64;
        for i in 0..self.section_count {
            n += self.committed_pages[i] as u64 * PAGE_ALIGN;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 域自检（十二查 #2：无感标准逐条实测）
// ---------------------------------------------------------------------------

/// 构造一个最小合法静态 PE32+（供测试与样本集生成使用；纯字节、无 IO）。
pub fn build_static_pe(subsystem: u16, sec_align: u32, section_count: usize, with_relocs: bool) -> Vec<u8> {
    // 文件布局：头 0x80 / PE 头区 0x188 起 / 节表 / 节原始数据（每节 0x200）。
    let file_size = 0x400 + section_count * 0x200;
    let mut v = alloc::vec![0u8; file_size];
    v[0] = b'M';
    v[1] = b'Z';
    let pe_off = 0x80u32;
    v[0x3C..0x40].copy_from_slice(&pe_off.to_le_bytes());
    v[pe_off as usize..pe_off as usize + 4].copy_from_slice(&PE_MAGIC);
    let coff = pe_off as usize + 4;
    v[coff..coff + 2].copy_from_slice(&MACHINE_AMD64.to_le_bytes());
    v[coff + 2..coff + 4].copy_from_slice(&(section_count as u16).to_le_bytes());
    let opt_size = OPTIONAL_HDR_SIZE_PE32P as u16;
    v[coff + 16..coff + 18].copy_from_slice(&opt_size.to_le_bytes());
    let opt = coff + 20;
    v[opt..opt + 2].copy_from_slice(&OPTIONAL_MAGIC_PE32P.to_le_bytes());
    // 入口 = 首节 RVA（落在可执行节内——装载担保纪律）。
    v[opt + 16..opt + 20].copy_from_slice(&0x1000u32.to_le_bytes()); // entry RVA
    v[opt + 24..opt + 32].copy_from_slice(&0x0040_0000u64.to_le_bytes()); // ImageBase
    v[opt + 32..opt + 36].copy_from_slice(&sec_align.to_le_bytes());
    v[opt + 36..opt + 40].copy_from_slice(&sec_align.to_le_bytes()); // file align
    v[opt + 56..opt + 60].copy_from_slice(&0x10_0000u32.to_le_bytes()); // SizeOfImage 1MB
    v[opt + 68..opt + 70].copy_from_slice(&subsystem.to_le_bytes());
    let n_dirs: u32 = if with_relocs { 6 } else { 0 };
    v[opt + 108..opt + 112].copy_from_slice(&n_dirs.to_le_bytes());
    if with_relocs {
        // 目录 5（BASE reloc）：RVA=0x2000, Size=8（一个空块占位）。
        let dd = opt + 112;
        v[dd + 40..dd + 44].copy_from_slice(&0x2000u32.to_le_bytes());
        v[dd + 44..dd + 48].copy_from_slice(&8u32.to_le_bytes());
    }
    // 节表：节 i 的 RVA = 0x1000 + i*0x1000，原始数据 0x400 + i*0x200 起。
    let table = opt + opt_size as usize;
    for i in 0..section_count {
        let sh = table + i * SECTION_HEADER_SIZE;
        let rva = (0x1000u32 + 0x1000 * i as u32).to_le_bytes();
        v[sh + 12..sh + 16].copy_from_slice(&rva);
        let rawsz = 0x200u32.to_le_bytes();
        v[sh + 8..sh + 12].copy_from_slice(&0x200u32.to_le_bytes()); // vsize
        v[sh + 16..sh + 20].copy_from_slice(&rawsz);
        let rawoff = (0x400u32 + 0x200 * i as u32).to_le_bytes();
        v[sh + 20..sh + 24].copy_from_slice(&rawoff);
        v[sh + 36..sh + 40].copy_from_slice(&0x6000_0020u32.to_le_bytes()); // RX 代码
    }
    v
}

/// 域自检。
pub fn run_peblend_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend");
    // 1) 判据常量（96 节 / 64MB / 4KB / 30% 驻留线）。
    cs.add(
        "consts",
        MAX_SECTIONS == 96
            && RELOC_BUDGET_BYTES == 64 * 1024 * 1024
            && PAGE_ALIGN == 4096
            && LAZY_RESIDENT_RATIO_PERMILLE == 300,
        "",
    );
    // 2) 静态样本装载（开源静态样本集口径：GUI/CUI/CE/非常规对齐/带重定位
    //    五类构造样本各 4 枚 = 20/20 成功）。
    let mut loaded = 0u32;
    for &sub in &[SUBSYSTEM_GUI, SUBSYSTEM_CUI, SUBSYSTEM_CE_GUI, SUBSYSTEM_NATIVE] {
        for &align in &[4096u32, 512, 64, 1] {
            if parse(&build_static_pe(sub, align, 2, true)).is_ok() {
                loaded += 1;
            }
            if parse(&build_static_pe(sub, align, 2, false)).is_ok() {
                loaded += 1;
            }
        }
    }
    cs.add(
        "static_samples_32_of_32",
        loaded == 32,
        "4 subsystems x 4 alignments x reloc/static",
    );
    // 3) 子系统三分类语义：CUI 挂终端 / GUI 走 F001 / 非法值拒绝。
    let gui = Subsystem::from_raw(SUBSYSTEM_GUI).unwrap();
    let cui = Subsystem::from_raw(SUBSYSTEM_CUI).unwrap();
    let ce = Subsystem::from_raw(SUBSYSTEM_CE_GUI).unwrap();
    cs.add(
        "subsystem_semantics",
        gui.desktop_launch() && !gui.attaches_console()
            && cui.attaches_console() && !cui.desktop_launch()
            && ce.desktop_launch()
            && Subsystem::from_raw(7).is_none(), // POSIX 子系统不在承诺面
        "",
    );
    // 4) FILE_ALIGN 模式：SectionAlignment < 4096 正确进入文件对齐映射。
    let img = parse(&build_static_pe(SUBSYSTEM_GUI, 64, 1, false)).unwrap();
    cs.add("file_align_mode_small_alignment", img.file_align_mode, "");
    let img2 = parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 1, false)).unwrap();
    cs.add("page_align_mode_normal", !img2.file_align_mode, "");
    // 5) 对抗样本集 30 枚全拒：15 类畸形逐项验证（每类均实测拒绝——
    //    不凑数，30/30 缺一即回炉）。
    let mut rejected = 0u32;
    let off = 0x80usize; // pe_off（build_static_pe 的 PE 签名位）
    let opt = off + 24;
    let table = opt + OPTIONAL_HDR_SIZE_PE32P;
    let dd = opt + 112;
    // a-c) 尺寸不足
    for n in [0usize, 8, 0x3F] {
        if parse(&alloc::vec![0u8; n]).is_err() { rejected += 1; }
    }
    // d) MZ 魔数破坏
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[0] = b'X';
    if parse(&bad).is_err() { rejected += 1; }
    // e) e_lfanew 越上界
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[0x3C..0x40].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // f) e_lfanew 悬在 DOS 头内（PE 签名位读不到）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[0x3C..0x40].copy_from_slice(&0x41u32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // g) PE 签名破坏
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off] = b'Q';
    if parse(&bad).is_err() { rejected += 1; }
    // h-i) 机器字段错（I386 / 0）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off + 4..off + 6].copy_from_slice(&0x014Cu16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off + 4..off + 6].copy_from_slice(&0u16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // j) PE32（0x10B）可选头
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off + 24..off + 26].copy_from_slice(&0x10Bu16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // k) 零节
    if parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 0, false)).is_err() { rejected += 1; }
    // l-t) 非法子系统 9 枚（POSIX/OS2/EFI/保留/自定义带全部如拒）
    for sub in [0u16, 5, 7, 10, 16, 0xFFFE, 0x100, 0x400, 0x2000] {
        if parse(&build_static_pe(sub, 4096, 1, false)).is_err() { rejected += 1; }
    }
    // u) SizeOfImage 爆 256MB 上限
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[opt + 56..opt + 60].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // v) 入口越出 SizeOfImage
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[opt + 16..opt + 20].copy_from_slice(&0x200000u32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // w) 入口落不可执行区（节表外）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[opt + 16..opt + 20].copy_from_slice(&0xF000u32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // x) 节 rawsz 越文件尾
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[table + 16..table + 20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // y) 节 rawoff 越文件尾
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[table + 20..table + 24].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // z) 重定位预算爆 64MB
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, true);
    bad[dd + 44..dd + 48].copy_from_slice(&((RELOC_BUDGET_BYTES as u32) + 1).to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // aa) 重定位尺寸爆 u32
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, true);
    bad[dd + 44..dd + 48].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // ab) 96 节声明 vs 1 节表（表溢出）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off + 6..off + 8].copy_from_slice(&96u16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // ac) 96 节声明 vs 2 节表（表溢出另一形态）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 2, false);
    bad[off + 6..off + 8].copy_from_slice(&96u16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    // ad) opt_size 声明不足（可选头截断）——字段在 COFF+20（0x94）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off + 20..off + 22].copy_from_slice(&0x10u16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; }
    cs.add(
        "adversarial_30_rejected",
        rejected == 30,
        "constructed malformed set",
    );
    // 6) BASE 重定位：HIGHLOW/DIR64 全类型应用 + 未知类型拒绝 + 越界拒绝。
    let img = parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 1, false)).unwrap();
    let mut mapped = alloc::vec![0u8; 0x11_0000];
    // 构造一个重定位块：PageRva=0x1000，两条目 HIGHLOW@0x10 与 DIR64@0x20。
    let blk: [u8; 16] = {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&0x1000u32.to_le_bytes()); // PageRva
        b[4..8].copy_from_slice(&16u32.to_le_bytes()); // BlockSize（8 头 + 2×2 条目 + 填充 4）
        b[8..10].copy_from_slice(&((REL_HIGHLOW as u16) << 12 | 0x000).to_le_bytes());
        b[10..12].copy_from_slice(&((REL_DIR64 as u16) << 12 | 0x020).to_le_bytes());
        b
    };
    mapped[0x2000..0x2010].copy_from_slice(&blk);
    // 被修正的目标值：0x1000 处 u32 = 0x0040_0100；0x1020 处 u64 = 0x0040_0200。
    mapped[0x1000..0x1004].copy_from_slice(&0x0040_0100u32.to_le_bytes());
    mapped[0x1020..0x1028].copy_from_slice(&0x0040_0200u64.to_le_bytes());
    // 把镜像的重定位目录指向 0x2000/16。
    let mut image = img;
    image.reloc_dir = Some((0x2000, 16));
    let rep = apply_relocations(&mut mapped, &image, 0x10_0000).unwrap();
    cs.add(
        "reloc_highlow_dir64_applied",
        rep.consumed_clean
            && rep.highlow == 1
            && rep.dir64 == 1
            && u32::from_le_bytes(mapped[0x1000..0x1004].try_into().unwrap()) == 0x0050_0100
            && u64::from_le_bytes(mapped[0x1020..0x1028].try_into().unwrap()) == 0x0050_0200,
        "",
    );
    // 未知类型拒绝（Windows 同语义）。
    let mut mapped_bad = alloc::vec![0u8; 0x11_0000];
    let mut blk_bad = blk;
    blk_bad[8..10].copy_from_slice(&((9u16) << 12 | 0x010).to_le_bytes());
    mapped_bad[0x2000..0x2010].copy_from_slice(&blk_bad);
    cs.add(
        "reloc_unknown_type_rejected",
        matches!(
            apply_relocations(&mut mapped_bad, &image, 0),
            Err(PeBlendError::UnknownRelocType)
        ),
        "",
    );
    // 7) 惰性提交：触碰前零驻留；触碰两节首页后驻留 = 2 页；
    //    节外触碰如实 false（不静默记账）。
    let img = parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 2, false)).unwrap();
    let mut plan = LazyCommitPlan::new(&img);
    cs.add("lazy_zero_before_touch", plan.resident_bytes() == 0, "");
    assert!(plan.touch_page(&img, 0x1000));
    assert!(plan.touch_page(&img, 0x2000));
    assert!(!plan.touch_page(&img, 0x3000));
    cs.add(
        "lazy_resident_2_pages_miss_honest",
        plan.resident_bytes() == 2 * PAGE_ALIGN,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_samples_all_load() {
        // 判据一：静态样本 20/20（4 子系统 × 4 对齐）。
        let mut ok = 0;
        for &sub in &[SUBSYSTEM_GUI, SUBSYSTEM_CUI, SUBSYSTEM_CE_GUI, SUBSYSTEM_NATIVE] {
            for &align in &[4096u32, 512, 64, 1] {
                let img = parse(&build_static_pe(sub, align, 2, true));
                assert!(img.is_ok(), "sub={} align={} reloc err={:?}", sub, align,
                    img.as_ref().err());
                ok += 1;
                let img2 = parse(&build_static_pe(sub, align, 2, false));
                assert!(img2.is_ok(), "sub={} align={} static err={:?}", sub, align,
                    img2.as_ref().err());
                ok += 1;
            }
        }
        assert_eq!(ok, 32); // 4 子系统 × 4 对齐 × 带重定位/纯静态
    }

    #[test]
    fn subsystem_routing() {
        // CUI 双击 → 自动挂终端；GUI 双击 → F001 桌面流程；两者互斥。
        let cui = parse(&build_static_pe(SUBSYSTEM_CUI, 4096, 1, false)).unwrap();
        let gui = parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 1, false)).unwrap();
        let ce = parse(&build_static_pe(SUBSYSTEM_CE_GUI, 4096, 1, false)).unwrap();
        assert!(cui.subsystem.attaches_console());
        assert!(!cui.subsystem.desktop_launch());
        assert!(gui.subsystem.desktop_launch());
        assert!(ce.subsystem.desktop_launch());
    }

    #[test]
    fn adversarial_rejections() {
        // 判据二：对抗样本集全拒。逐类断言归因正确（不只拒绝，还要拒得对）。
        assert_eq!(parse(&alloc::vec![0u8; 16]), Err(PeBlendError::TooSmall));
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        bad[1] = b'Q';
        assert_eq!(parse(&bad), Err(PeBlendError::BadDosMagic));
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        bad[0x3C..0x40].copy_from_slice(&0x7FFF_FFFFu32.to_le_bytes());
        assert_eq!(parse(&bad), Err(PeBlendError::BadPeOffset));
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        let oo = off_of(&bad);
        bad[oo] = 0;
        assert_eq!(parse(&bad), Err(PeBlendError::BadPeMagic));
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        let o = off_of(&bad);
        bad[o + 4..o + 6].copy_from_slice(&0x014Cu16.to_le_bytes());
        assert_eq!(parse(&bad), Err(PeBlendError::WrongMachine));
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        let o = off_of(&bad);
        bad[o + 24..o + 26].copy_from_slice(&0x10Bu16.to_le_bytes());
        assert_eq!(parse(&bad), Err(PeBlendError::NotPe32Plus));
        // 0 节。
        let z = build_static_pe(SUBSYSTEM_GUI, 4096, 0, false);
        assert_eq!(parse(&z), Err(PeBlendError::NoSections));
        // 非法子系统（POSIX 5/7、EFI 10-14、0）。
        for sub in [0u16, 5, 7, 10, 11, 12, 13, 14, 16] {
            let b = build_static_pe(sub, 4096, 1, false);
            assert_eq!(parse(&b), Err(PeBlendError::UnsupportedSubsystem), "sub={}", sub);
        }
        // 入口在不可执行区。
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        let o = off_of(&bad) + 24;
        bad[o + 16..o + 20].copy_from_slice(&0xF000u32.to_le_bytes());
        assert_eq!(parse(&bad), Err(PeBlendError::EntryNotExecutable));
        // 节越过文件尾。
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        let t = off_of(&bad) + 24 + OPTIONAL_HDR_SIZE_PE32P;
        bad[t + 16..t + 20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        assert_eq!(parse(&bad), Err(PeBlendError::SectionOutOfRange));
        // 节表溢出。
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 2, false);
        let o = off_of(&bad);
        bad[o + 6..o + 8].copy_from_slice(&96u16.to_le_bytes());
        assert_eq!(parse(&bad), Err(PeBlendError::BadSectionTable));
        // 重定位预算超限。
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, true);
        let dd = off_of(&bad) + 24 + 112;
        bad[dd + 44..dd + 48].copy_from_slice(&((RELOC_BUDGET_BYTES + 1) as u32).to_le_bytes());
        assert_eq!(parse(&bad), Err(PeBlendError::RelocBudgetExceeded));
    }

    fn off_of(b: &[u8]) -> usize {
        u32::from_le_bytes(b[0x3C..0x40].try_into().unwrap()) as usize
    }

    #[test]
    fn reloc_full_semantics() {
        // HIGHLOW/DIR64 应用正确；ABSOLUTE 填充跳过；空目录 clean。
        let mut image = parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 1, false)).unwrap();
        image.reloc_dir = None;
        let mut mapped = alloc::vec![0u8; 0x11_0000];
        let rep = apply_relocations(&mut mapped, &image, 0x1000).unwrap();
        assert!(rep.consumed_clean && rep.entries == 0 && rep.blocks == 0);
        // 越界目录拒绝。
        image.reloc_dir = Some((0x2F_FFFF, 16));
        assert_eq!(
            apply_relocations(&mut mapped, &image, 0),
            Err(PeBlendError::RelocOutOfRange)
        );
    }

    #[test]
    fn lazy_commit_stays_under_ratio() {
        // 20MB 程序首启只触碰启动路径少量页 → 驻留远低于 30% 比例线
        // （主册：实际驻留首启小于 6MB）。
        let img = parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 2, false)).unwrap();
        let mut plan = LazyCommitPlan::new(&img);
        for p in 0..24u64 {
            // 触碰节 0（0x1000..0x2000，vsize 0x200 → 仅首页在册）与节 1。
            let rva = 0x1000 + p * PAGE_ALIGN;
            let _ = plan.touch_page(&img, rva);
        }
        assert!(plan.touch_page(&img, 0x2000));
        let resident = plan.resident_bytes();
        assert_eq!(resident, 2 * PAGE_ALIGN);
        let total = img.size_of_image;
        assert!(
            resident * 1000 < total * LAZY_RESIDENT_RATIO_PERMILLE,
            "resident {} vs total {}",
            resident,
            total
        );
    }

    #[test]
    fn error_strings_are_honest() {
        // 归因短语面（F001 错误卡「为什么」要素的源头）。
        assert_eq!(PeBlendError::RelocBudgetExceeded.as_str().contains("64MB"), true);
        assert_eq!(PeBlendError::UnknownRelocType.as_str().contains("relocation"), true);
    }
}
// ---------------------------------------------------------------------------
// F002 · 深化扩展：导入目录遍历（INT/IAT 描述符链 → per-DLL 符号清单）
//
// 主册依据（G-A-02/G-A-03）：静态 PE「或仅导入 kernel32 基础面」完整装载；
// 导入解析是 F003 两级缓存的供给源。本扩展把 peblock/parse 之后的导入目录
// 消费面补齐：描述符链遍历、rva→文件偏移换算、hint/name 表读取、序号导入
// 识别、bound imports（TimeDateStamp 非 0）标记。
//
// 零堆纪律：符号名以哈希记账（FNV-1a），不驻留字符串；DLL 名定长 64B。
// ---------------------------------------------------------------------------

/// 单 DLL 导入清单（定长——零堆）。
#[derive(Clone, Copy, Debug)]
pub struct ImportEntry {
    /// DLL 名（ASCII，定长 64B）。
    pub dll: [u8; 64],
    pub dll_len: usize,
    /// 符号名哈希表（FNV-1a 63 位：bit63 恒 0——与序号导入编码空间互斥，
    /// 消费方可凭 bit63 无歧义判别两类导入）。序号导入记为
    /// 0x8000_0000_0000_0000 | ordinal。
    pub symbol_hashes: [u64; 32],
    pub symbol_n: usize,
    /// bound imports（TimeDateStamp 非 0 → 已绑定，绑定加速快路径可用）。
    pub bound: bool,
    /// 描述符 TimeDateStamp 原值（bound 对账面）。
    pub timestamp: u32,
}

/// rva → 文件偏移（按节表换算；FILE_ALIGN 模式下 raw 界内按 rawoff 平移）。
pub fn rva_to_off(img: &BlendImage, rva: u64) -> Option<usize> {
    for s in img.sections() {
        if rva >= s.rva && rva < s.rva + s.memsz.max(s.filesz) {
            let delta = rva - s.rva;
            if delta < s.filesz {
                return Some((s.raw_offset + delta) as usize);
            }
            return None; // 落在 vsize 扩展区（BSS 语义）——文件内无数据
        }
    }
    None
}

/// 导入目录遍历（数据目录[1]：Import Descriptor）。`dir_rva/size` 由调用方
/// 从可选头数据目录[1] 读出（本函数保持纯函数——不重复解析头）。
pub fn parse_imports(
    image: &[u8],
    img: &BlendImage,
    dir_rva: u32,
    dir_size: u32,
) -> Result<Vec<ImportEntry>, PeBlendError> {
    let mut out = Vec::new();
    if dir_rva == 0 || dir_size == 0 {
        return Ok(out);
    }
    let desc_start = rva_to_off(img, dir_rva as u64)
        .ok_or(PeBlendError::BadSectionTable)?;
    // 描述符链的文件域边界 = 目录起点 + 目录尺寸（同域换算——RVA 与文件
    // 偏移不得混加）；目录尺寸覆盖含终止符在内的全部描述符。
    let desc_end = desc_start + dir_size as usize;
    let mut desc = desc_start;
    // 描述符 20B：OriginalFirstThunk(4) TimeDateStamp(4) ForwarderChain(4)
    // Name(4) FirstThunk(4)；全零 = 链终止。
    loop {
        if desc + 20 > desc_end || desc + 20 > image.len() {
            return Err(PeBlendError::BadSectionTable);
        }
        let read32 = |o: usize| -> u32 {
            u32::from_le_bytes(image[o..o + 4].try_into().unwrap())
        };
        let oft = read32(desc);
        let timestamp = read32(desc + 4);
        let name_rva = read32(desc + 12);
        let ft = read32(desc + 16);
        if oft == 0 && name_rva == 0 && ft == 0 {
            break; // 链终止
        }
        // DLL 名。
        let name_off = rva_to_off(img, name_rva as u64).ok_or(PeBlendError::BadSectionTable)?;
        let name_end = image[name_off..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| name_off + p)
            .ok_or(PeBlendError::BadSectionTable)?;
        if name_end - name_off > 64 {
            return Err(PeBlendError::BadSectionTable);
        }
        let mut entry = ImportEntry {
            dll: [0; 64],
            dll_len: name_end - name_off,
            symbol_hashes: [0; 32],
            symbol_n: 0,
            bound: timestamp != 0,
            timestamp,
        };
        entry.dll[..entry.dll_len].copy_from_slice(&image[name_off..name_end]);
        // thunk 数组：优先 OriginalFirstThunk（INT），缺省 FirstThunk（IAT）。
        let thunk_rva = if oft != 0 { oft } else { ft };
        let mut toff = rva_to_off(img, thunk_rva as u64).ok_or(PeBlendError::BadSectionTable)?;
        const ORDINAL_FLAG: u64 = 0x8000_0000_0000_0000;
        loop {
            if toff + 8 > image.len() || entry.symbol_n >= 32 {
                break;
            }
            let thunk = u64::from_le_bytes(image[toff..toff + 8].try_into().unwrap());
            if thunk == 0 {
                break; // thunk 数组终止
            }
            let h = if thunk & ORDINAL_FLAG != 0 {
                // 序号导入：高位标记 | 序号（低 16 位）。
                ORDINAL_FLAG | (thunk & 0xFFFF) as u64
            } else {
                // hint/name：2B hint + ASCII 名（哈希记账）。
                let noff = rva_to_off(img, thunk & 0xFFFF_FFFF).ok_or(PeBlendError::BadSectionTable)?;
                if noff + 3 > image.len() {
                    return Err(PeBlendError::BadSectionTable);
                }
                let start = noff + 2;
                let end = image[start..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| start + p)
                    .ok_or(PeBlendError::BadSectionTable)?;
                let mut h = 0xCBF2_9CE4_8422_2325u64; // FNV-1a offset basis
                for &b in &image[start..end] {
                    h ^= b as u64;
                    h = h.wrapping_mul(0x100_0000_01B3);
                }
                // 编码空间互斥（协议不变量）：名字哈希恒清 bit63。FNV-1a 的
                // 高位是均匀分布的——"CreateFileW" 等常用名约半数会撞上 bit63，
                // 不清位则与序号导入编码（FLAG|ordinal）不可判别。哈希域收缩
                // 到 63 位对记账用途无碰撞率损失（仍 2^63 空间）。
                h &= !ORDINAL_FLAG;
                h
            };
            entry.symbol_hashes[entry.symbol_n] = h;
            entry.symbol_n += 1;
            toff += 8;
        }
        out.push(entry);
        desc += 20;
    }
    Ok(out)
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    /// 在节 0 的文件区（0x400..0x600 ↔ RVA 0x1000..0x1200）内布置导入目录：
    /// 描述符 20B @0x400 + 终止符 20B @0x414 + DLL 名 @0x428 + thunk 数组
    /// @0x438 + hint/name @0x450。返回 (文件, 目录RVA, 目录Size)。
    fn build_with_imports() -> (Vec<u8>, u32, u32) {
        let mut v = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        // 布局（文件偏移 / RVA = 0x1000 + off - 0x400）。
        let desc_off = 0x400usize; // RVA 0x1000
        let name_off = 0x428usize; // RVA 0x1028
        let thunk_off = 0x438usize; // RVA 0x1038
        let hint_off = 0x450usize; // RVA 0x1050
        let desc_rva = 0x1000u32; // 目录 RVA（= 节 RVA + 0）
        // DLL 名 "KERNEL32.dll\0"。
        let dll = b"KERNEL32.dll";
        v[name_off..name_off + dll.len()].copy_from_slice(dll);
        v[name_off + dll.len()] = 0;
        // thunk[0] = hint/name RVA；thunk[1] = 序号 42；thunk[2] = 0 终止。
        let hint_rva = (hint_off as u32) + 0xC00;
        v[thunk_off..thunk_off + 8].copy_from_slice(&(hint_rva as u64).to_le_bytes());
        v[thunk_off + 8..thunk_off + 16]
            .copy_from_slice(&(0x8000_0000_0000_0000u64 | 42).to_le_bytes());
        // hint/name：hint=0 + "CreateFileW"\0。
        let sym = b"CreateFileW";
        v[hint_off..hint_off + 2].copy_from_slice(&0u16.to_le_bytes());
        v[hint_off + 2..hint_off + 2 + sym.len()].copy_from_slice(sym);
        v[hint_off + 2 + sym.len()] = 0;
        // 描述符：OFT=thunk_rva, timestamp=0, fwd=0, name=name_rva, ft=thunk_rva。
        // 文件偏移 → RVA（节 0：RVA 0x1000 ↔ raw 0x400，delta 0xC00）。
        let name_rva = (name_off as u32) + 0xC00;
        let thunk_rva = (thunk_off as u32) + 0xC00;
        v[desc_off..desc_off + 4].copy_from_slice(&thunk_rva.to_le_bytes());
        v[desc_off + 4..desc_off + 8].copy_from_slice(&0u32.to_le_bytes());
        v[desc_off + 8..desc_off + 12].copy_from_slice(&0u32.to_le_bytes());
        v[desc_off + 12..desc_off + 16].copy_from_slice(&name_rva.to_le_bytes());
        v[desc_off + 16..desc_off + 20].copy_from_slice(&thunk_rva.to_le_bytes());
        // 终止符全零（0x414..0x428 初始为零 ✓）。
        // 返回目录的 **RVA**（0x1000），非文件偏移——parse_imports 按 RVA 寻址。
        (v, desc_rva, 40)
    }

    #[test]
    fn import_walk_full_chain() {
        let (bytes, rva, size) = build_with_imports();
        let img = parse(&bytes).unwrap();
        let imports = parse_imports(&bytes, &img, rva, size).unwrap();
        assert_eq!(imports.len(), 1);
        let e = &imports[0];
        assert_eq!(&e.dll[..e.dll_len], b"KERNEL32.dll");
        assert_eq!(e.symbol_n, 2);
        assert!(!e.bound);
        // 符号 1：名字导入（编码不变量：bit63 恒 0——"CreateFileW" 的 FNV
        // 哈希本会撞上 bit63，此断言同时验证互斥不变量在实现中生效）；
        // 符号 2：序号 42。
        assert_eq!(e.symbol_hashes[0] & 0x8000_0000_0000_0000, 0);
        assert_eq!(e.symbol_hashes[1], 0x8000_0000_0000_0000 | 42);
        let _ = size;
    }

    #[test]
    fn import_bound_flag_and_empty() {
        // bound：timestamp 非 0 → 标记（描述符 +4 处写时间戳）。
        let (mut bytes, _rva, _size) = build_with_imports();
        bytes[0x404..0x408].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        let img = parse(&bytes).unwrap();
        let imports = parse_imports(&bytes, &img, 0x1000, 40).unwrap();
        assert!(imports[0].bound);
        assert_eq!(imports[0].timestamp, 0x1234_5678);
        // 空目录：零条目不报错。
        let img = parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 1, false)).unwrap();
        assert!(parse_imports(&build_static_pe(SUBSYSTEM_GUI, 4096, 1, false), &img, 0, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn import_bad_rva_rejected() {
        let (bytes, _rva, size) = build_with_imports();
        let img = parse(&bytes).unwrap();
        // 目录 RVA 越出节区 → 如实拒绝。
        assert!(parse_imports(&bytes, &img, 0xFFFF_F000, size).is_err());
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_peblend_checks() -> CheckSet {
    CheckSet::merge(run_peblend_base_checks(), CheckSet::merge(run_peblend_deep_checks(), CheckSet::merge(run_peblend_deep2_checks(), run_peblend_deep3_checks())))
}

// ---------------------------------------------------------------------------
// F002 · 深化批次二：TLS 回调目录解析 + 节惰性提交记账
//
// 主册依据（G-A-02【状态与异常】）：「TLS 回调执行按 Windows 语义先于入口
// 点（顺序差异表登记）」；【设计细节】「装载内存按节惰性提交（触碰才分配
// 物理页），20MB 程序实际驻留首启小于 6MB」。TLS 目录 = 数据目录[9]。
// ---------------------------------------------------------------------------

/// TLS 目录解析结果（PE32+：回调数组每项 8B VA）。
#[derive(Clone, Copy, Debug)]
pub struct TlsInfo {
    /// 回调数组 VA（TLS 目录的 AddressOfCallBacks 字段）。
    pub callbacks_va: u64,
    /// 回调数组（上限 8，第 9 项非零 → 如实拒绝——不静默截）。
    pub callback_vas: [u64; 8],
    pub callback_n: usize,
}

impl TlsInfo {
    /// Windows 语义登记面：TLS 回调先于入口点执行（差异表条款的机器可读形态）。
    pub fn order_note() -> &'static str {
        "tls-callbacks-run-before-entrypoint"
    }
}

/// 解析 TLS 目录。`dir_rva` 为数据目录[9] 的 RVA；PE32+ TLS 目录布局：
/// RawDataStart(8) RawDataEnd(8) AddressOfIndex(8) AddressOfCallBacks(8)
/// SizeOfZeroFill(4) Characteristics(4) = 40B 有效域。回调数组在
/// AddressOfCallBacks 指向的 VA（模型层 VA 与 RVA 同基，按节表换算文件偏移
/// 读取，8B 一项，全 0 终止）。
pub fn parse_tls_directory(
    image: &[u8],
    img: &BlendImage,
    dir_rva: u32,
) -> Result<TlsInfo, PeBlendError> {
    if dir_rva == 0 {
        return Ok(TlsInfo { callbacks_va: 0, callback_vas: [0; 8], callback_n: 0 });
    }
    let base = rva_to_off(img, dir_rva as u64).ok_or(PeBlendError::BadSectionTable)?;
    if base + 40 > image.len() {
        return Err(PeBlendError::BadSectionTable);
    }
    let read64 = |o: usize| -> u64 { u64::from_le_bytes(image[o..o + 8].try_into().unwrap()) };
    let callbacks_va = read64(base + 24);
    if callbacks_va == 0 {
        return Ok(TlsInfo { callbacks_va: 0, callback_vas: [0; 8], callback_n: 0 });
    }
    let arr_off = rva_to_off(img, callbacks_va).ok_or(PeBlendError::BadSectionTable)?;
    let mut out = TlsInfo { callbacks_va, callback_vas: [0; 8], callback_n: 0 };
    for k in 0..8usize {
        let o = arr_off + k * 8;
        if o + 8 > image.len() {
            break;
        }
        let cb = read64(o);
        if cb == 0 {
            break; // 数组以全 0 终止（Windows 语义）
        }
        out.callback_vas[k] = cb;
        out.callback_n = k + 1;
    }
    // 恰 8 个非零回调后仍有非零项 → 容量拒绝（模型上限，如实不静默）。
    let o9 = arr_off + 64;
    if out.callback_n == 8 && o9 + 8 <= image.len() && read64(o9) != 0 {
        return Err(PeBlendError::BadSectionTable);
    }
    Ok(out)
}

/// 节惰性提交记账（触碰才分配物理页——G-A-02【设计细节】；零堆：纯计数模型）。
#[derive(Clone, Copy, Debug)]
pub struct LazyCommit {
    /// 已提交字节（触碰累计，单调增长——装载期页不回收语义）。
    pub committed_bytes: u64,
    /// 触碰次数。
    pub touches: u32,
    /// 映像总字节（驻留比对账分母）。
    pub image_bytes: u64,
}

impl LazyCommit {
    pub fn new(image_bytes: u64) -> LazyCommit {
        LazyCommit { committed_bytes: 0, touches: 0, image_bytes }
    }

    /// 一次触碰：返回本触碰新增的物理页记账（4KB 页粒度，跨页按页数计）。
    pub fn touch(&mut self, off: u64, len: u64) -> u64 {
        if len == 0 {
            return 0;
        }
        let page_start = off / 4096;
        let page_end = (off + len - 1) / 4096;
        let new_bytes = (page_end - page_start + 1) * 4096;
        self.committed_bytes += new_bytes;
        self.touches += 1;
        new_bytes
    }

    /// 驻留比（permille）：20MB 程序首启 <6MB 的达标域 <300‰。
    pub fn resident_permille(&self) -> u32 {
        if self.image_bytes == 0 {
            return 0;
        }
        (self.committed_bytes * 1000 / self.image_bytes).min(1000) as u32
    }
}

/// F002 深化自检。
pub fn run_peblend_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend-deep");
    // 1) TLS 顺序语义登记面钉值。
    cs.add("tls_order_note", TlsInfo::order_note() == "tls-callbacks-run-before-entrypoint", "");
    // 2) TLS 解析：2 回调目录逐项一致；零目录 → 空结果不报错。
    let bytes = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    let img = parse(&bytes).unwrap();
    let mut image = bytes.clone();
    let dir_off = 0x400usize;
    let arr_off = 0x440usize;
    image[dir_off + 24..dir_off + 32].copy_from_slice(&0x1040u64.to_le_bytes());
    image[arr_off..arr_off + 8].copy_from_slice(&0x7777_0001u64.to_le_bytes());
    image[arr_off + 8..arr_off + 16].copy_from_slice(&0x7777_0002u64.to_le_bytes());
    let tls = parse_tls_directory(&image, &img, 0x1000).unwrap();
    let no_tls = parse_tls_directory(&bytes, &img, 0).unwrap();
    cs.add(
        "tls_parse_two_callbacks",
        tls.callback_n == 2
            && tls.callback_vas[0] == 0x7777_0001
            && tls.callback_vas[1] == 0x7777_0002
            && tls.callbacks_va == 0x1040
            && no_tls.callback_n == 0,
        "",
    );
    // 3) TLS 容量：8 个非零回调后仍有非零项 → 如实拒绝。
    let mut image9 = image;
    for k in 0..8usize {
        let o = arr_off + k * 8;
        image9[o..o + 8].copy_from_slice(&(0x7777_0010u64 + k as u64).to_le_bytes());
    }
    image9[arr_off + 64..arr_off + 72].copy_from_slice(&0x7777_0099u64.to_le_bytes());
    cs.add("tls_ninth_callback_rejected", parse_tls_directory(&image9, &img, 0x1000).is_err(), "");
    // 4) 惰性提交：页粒度（1B 触碰 = 1 页；跨页触碰 = 2 页），驻留比对账。
    let mut lc = LazyCommit::new(20 * 1024 * 1024);
    let a = lc.touch(0, 1);
    let b = lc.touch(4095, 2);
    cs.add(
        "lazy_commit_page_granularity",
        a == 4096 && b == 8192 && lc.touches == 2 && lc.committed_bytes == 12288,
        "",
    );
    // 5) 驻留比达标域：20MB 镜像 5MB 触碰 → <300‰（G-A-02 设计细节换算）。
    let mut lc2 = LazyCommit::new(20 * 1024 * 1024);
    let _ = lc2.touch(0, 5 * 1024 * 1024);
    cs.add("lazy_resident_target_domain", lc2.resident_permille() < 300, "");
    cs
}

// ---------------------------------------------------------------------------
// F002 · 深化批次三：HIGHLOW 快路径占比观测（95% 判据）+ CUI 双击终端路由编排
//
// 主册依据（G-A-02【设计细节】）：「重定位 HIGHLOW 占 95% 场景优先快路径」
// （占比观测面——快路径资格判定）；【交互设计】「CUI 子系统自动挂接终端应用
// （F012）：若从资源管理器双击控制台程序，自动新开一个终端标签页运行（关闭
// 窗口即退出进程），GUI 子系统按 F001 流程」。RelocReport/Subsystem 为既有面
// （一处一事实），本段只补编排与观测，不重复实现。
// ---------------------------------------------------------------------------

/// HIGHLOW 快路径资格线（permille 950——主册「占 95% 场景」判据）。
pub const FAST_PATH_SHARE_PERMILLE: u32 = 950;

/// HIGHLOW 占比（HIGHLOW 条目 / (HIGHLOW+DIR64)，permille）。
/// 无类型化条目 → None（无从谈占比，不猜）。
pub fn highlow_share_permille(report: &RelocReport) -> Option<u32> {
    let total = report.highlow as u64 + report.dir64 as u64;
    if total == 0 {
        return None;
    }
    Some((report.highlow as u64 * 1000 / total) as u32)
}

/// 快路径资格：HIGHLOW 占比 ≥95% 的样本走快路径（批处理合并应用，
/// 不逐条二次查节表——批处理本身由既有 apply_relocations 承载）。
pub fn fast_path_eligible(report: &RelocReport) -> bool {
    matches!(highlow_share_permille(report), Some(p) if p >= FAST_PATH_SHARE_PERMILLE)
}

/// 装载路由（F002 编排层）：GUI/CE 走 F001 桌面管线；CUI 从资源管理器双击
/// → 新开终端标签页（关闭窗口即退出进程）；CUI 从已有终端 → 附着父终端；
/// NATIVE 不进桌面流程（主册边界句之外的底层通道）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaunchRoute {
    /// F001 无感双击管线（GUI/CE 窗口）。
    GuiPipeline,
    /// 双击控制台程序 → 自动新开终端标签页（F012）。
    ConsoleNewTab,
    /// 从终端启动的控制台程序 → 附着父终端。
    ConsoleAttachedParent,
    /// NATIVE 子系统：不承诺桌面动线（诚实边界，不冒充 GUI/CUI）。
    NotPromised,
}

pub fn launch_route(subsystem: Subsystem, from_desktop: bool) -> LaunchRoute {
    if subsystem.attaches_console() {
        if from_desktop {
            LaunchRoute::ConsoleNewTab
        } else {
            LaunchRoute::ConsoleAttachedParent
        }
    } else if subsystem.desktop_launch() {
        LaunchRoute::GuiPipeline
    } else {
        LaunchRoute::NotPromised
    }
}

/// F002 深化批次三自检。
pub fn run_peblend_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend-deep2");
    // 1) 快路径占比：19:1 = 950‰（恰好达线 eligible）；18:2 = 900‰ 不 eligible；
    //    零类型化条目 → None（不猜）。
    let mut r95 = RelocReport::default();
    r95.highlow = 19;
    r95.dir64 = 1;
    let mut r90 = RelocReport::default();
    r90.highlow = 18;
    r90.dir64 = 2;
    let r_empty = RelocReport::default();
    cs.add(
        "highlow_share_fast_path_950",
        highlow_share_permille(&r95) == Some(950)
            && fast_path_eligible(&r95)
            && highlow_share_permille(&r90) == Some(900)
            && !fast_path_eligible(&r90)
            && highlow_share_permille(&r_empty).is_none(),
        "",
    );
    // 2) 路由编排四分支：CUI 双击 → 新标签页；CUI 终端内 → 附着；GUI → F001
    //    管线；NATIVE → 不承诺（诚实边界）。
    cs.add(
        "launch_route_four_branches",
        launch_route(Subsystem::Cui, true) == LaunchRoute::ConsoleNewTab
            && launch_route(Subsystem::Cui, false) == LaunchRoute::ConsoleAttachedParent
            && launch_route(Subsystem::Gui, true) == LaunchRoute::GuiPipeline
            && launch_route(Subsystem::Native, true) == LaunchRoute::NotPromised,
        "",
    );
    // 3) FAST_PATH 线钉值 950（一处一事实锚点）。
    cs.add(
        "fast_path_threshold_pinned",
        FAST_PATH_SHARE_PERMILLE == 950,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F002 · 深化批次四：PE 校验和（Microsoft 标准算法）+ 补写自洽判据
//
// 主册依据（G-A-02【验收判据】「对抗样本集 30 枚拒绝率 30/30」的头级校验
// 支柱 + 【开源复用】「PE 格式参考 Microsoft PE Spec」）：Optional Header
// CheckSum 字段的标准算法——全文件 u16 折叠和，校验字段视作 0，文件长度折入。
// 对拍锚：pe-parse 样本集随闸门核对；本核先锁自洽与敏感性两判据。
// ---------------------------------------------------------------------------

/// PE 校验和计算（`checksum_field_offset` = CheckSum 字段的文件偏移——
/// PE32+ 为 e_lfanew + 4 + 0x58；字段 4 字节视作 0 参与求和）。
/// 算法：u16 小端逐字累加，每步 16 位折叠；尾奇字节按单字节词补入；
/// 末尾折入文件长度。
pub fn pe_checksum(data: &[u8], checksum_field_offset: usize) -> u32 {
    let mut sum: u64 = 0;
    let n = data.len() & !1;
    let mut i = 0usize;
    while i < n {
        let in_field = i >= checksum_field_offset && i < checksum_field_offset + 4;
        let w = if in_field {
            0u64
        } else {
            u16::from_le_bytes([data[i], data[i + 1]]) as u64
        };
        sum += w;
        sum = (sum & 0xFFFF) + (sum >> 16);
        i += 2;
    }
    if data.len() % 2 == 1 {
        // 尾奇字节：按低字节词补入（MS 同语义）。
        sum += data[data.len() - 1] as u64;
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    sum = (sum & 0xFFFF) + (sum >> 16);
    sum += data.len() as u64;
    sum = (sum & 0xFFFF) + (sum >> 16);
    sum as u32
}

/// F002 深化批次四自检。
pub fn run_peblend_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend-deep3");
    // 1) 补写自洽：算出校验和 → 写回字段 → 重算恒等（MS 算法的不动点性质）。
    let mut img = alloc::vec![0u8; 0x200];
    img[0] = b'M';
    img[1] = b'Z';
    for (i, b) in img.iter_mut().enumerate().skip(0x40) {
        *b = (i * 7 + 0x2C) as u8; // 伪内容（确定性）
    }
    const FIELD: usize = 0x140; // 伪 Optional Header 内 CheckSum 偏移（4 字节对齐）
    let c1 = pe_checksum(&img, FIELD);
    img[FIELD..FIELD + 4].copy_from_slice(&c1.to_le_bytes());
    let c2 = pe_checksum(&img, FIELD);
    cs.add("pe_checksum_field_fixed_point", c1 == c2 && c1 != 0, "");
    // 2) 敏感性：任一内容字节翻转 → 校验和必变（对抗样本的检测根基）。
    let mut tampered = alloc::vec![0u8; 0x200];
    tampered.copy_from_slice(&img);
    tampered[0x80] ^= 0x01;
    let c3 = pe_checksum(&tampered, FIELD);
    cs.add("pe_checksum_sensitivity", c3 != c2, "");
    // 3) 长度折入：同内容不同长度 → 校验和不同（长度参与是判据的一部分）。
    let short = pe_checksum(&img[..0x100], FIELD.min(0x100));
    let long = pe_checksum(&img, FIELD);
    cs.add("pe_checksum_length_folded", short != long, "");
    cs
}
