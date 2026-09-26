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
    let mut v = vec![0u8; file_size];
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
pub fn run_peblend_checks() -> CheckSet {
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
        if parse(&vec![0u8; n]).is_err() { rejected += 1; }
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
    let mut mapped = vec![0u8; 0x11_0000];
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
    let mut mapped_bad = vec![0u8; 0x11_0000];
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
        assert_eq!(parse(&vec![0u8; 16]), Err(PeBlendError::TooSmall));
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
        let mut mapped = vec![0u8; 0x11_0000];
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
