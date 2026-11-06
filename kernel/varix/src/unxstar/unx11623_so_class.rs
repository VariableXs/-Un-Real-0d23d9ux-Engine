//! UNX-F11623 .so ELF class 不匹配（ELFCLASS32）判据
//!
//! `e_ident` 十六个字节里，第 4 个（`EI_CLASS`）是整份 ELF 的"宽度契约"：
//! 0 = `ELFCLASSNONE`，1 = `ELFCLASS32`，2 = `ELFCLASS64`，3 = `ELFCLASSNUM`。
//! 它不是一个可以事后补救的提示位，而是决定**后面每个结构体多宽**的总开关：
//! `Elf32_Ehdr` 52 字节、`Elf64_Ehdr` 64 字节；程序头项 32 与 56；节头项
//! 40 与 64；地址 32 位与 64 位。读错这一字节，紧接着的 `e_phoff` 就会
//! 按错的宽度解释，最终表现为"表越界"这种离根因很远的报错。
//!
//! 所以类宽闸门必须**在 Ehdr 其余字段之前**判完，并且失败文案逐字稳定：
//!
//! - `./ilp32.so: wrong ELF class: ELFCLASS32`
//! - `./wide.so: wrong ELF class: ELFCLASS64`
//! - `./none.so: wrong ELF class: ELFCLASSNONE`
//! - `./num.so: wrong ELF class: ELFCLASSNUM`
//! - `./odd.so: wrong ELF class: ELFCLASSNN/8`
//!
//! 三个容易混淆的边界在本模块里各有独立判据：
//!
//! 1. **x32 ABI**：`EI_CLASS=1` 配 `e_machine=EM_X86_64(62)` 是合法的 x32
//!    ABI，不是"i386 的 .so"。本机 LP64 装载器仍要拒（宽度不符），但诊断里
//!    必须标出 `compat=x32`，否则运维会把它当成编译错误去重编 32 位工具链。
//! 2. **类与项宽的连带校验**：`e_phentsize` 只能是 32 或 56，且必须与
//!    `EI_CLASS` 配套；`e_ehsize` 只能是 52 或 64。类对而项宽不对，属于
//!    "头部自相矛盾"，落 `invalid ELF header` 而不是 `wrong ELF class`。
//! 3. **依赖的类**：一个 64 位 .so 的 `DT_NEEDED` 指向一个 32 位库时，报错
//!    主体是**依赖名**，不是根对象名。文案仍是 `wrong ELF class: ELFCLASS32`，
//!    但前缀换了主体，这条如果写错，排障会照着根对象白找一遍。
//!
//! 闸门次序：ident 长度 → 类字节 → 宿主兼容 → 项宽连坐 → 机种×类 → 依赖链 → 文案 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfc60;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// `e_ident` 长度。
pub const EI_NIDENT: usize = 16;

/// `EI_CLASS` 偏移。
pub const EI_CLASS: usize = 4;

/// `EI_DATA` 偏移。
pub const EI_DATA: usize = 5;

/// `EI_VERSION` 偏移。
pub const EI_VERSION: usize = 6;

/// `EI_OSABI` 偏移。
pub const EI_OSABI: usize = 7;

/// `EI_ABIVERSION` 偏移。
pub const EI_ABIVERSION: usize = 8;

/// 类字节可读的最短长度。
pub const MIN_CLASS: usize = EI_CLASS + 1;

/// `ELFCLASSNONE`。
pub const ELFCLASSNONE: u8 = 0;

/// `ELFCLASS32`。
pub const ELFCLASS32: u8 = 1;

/// `ELFCLASS64`。
pub const ELFCLASS64: u8 = 2;

/// `ELFCLASSNUM`。
pub const ELFCLASSNUM: u8 = 3;

/// Elf32_Ehdr 长度。
pub const EHDR32: u32 = 52;

/// Elf64_Ehdr 长度。
pub const EHDR64: u32 = 64;

/// Elf32_Phdr 长度。
pub const PHENT32: u32 = 32;

/// Elf64_Phdr 长度。
pub const PHENT64: u32 = 56;

/// Elf32_Shdr 长度。
pub const SHENT32: u32 = 40;

/// Elf64_Shdr 长度。
pub const SHENT64: u32 = 64;

/// EM_386。
pub const EM_386: u16 = 3;

/// EM_X86_64。
pub const EM_X86_64: u16 = 62;

/// EM_ARM（只用于类闸门的机种噪声样本）。
pub const EM_ARM: u16 = 40;

/// ELFDATA2LSB。
pub const ELFDATA2LSB: u8 = 1;

/// ELFDATA2MSB。
pub const ELFDATA2MSB: u8 = 2;

/// EV_CURRENT。
pub const EV_CURRENT: u8 = 1;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 32 位类拒收。
pub const CL_ECLASS32: u32 = SEG_BASE + 0x01;
/// 64 位类拒收。
pub const CL_ECLASS64: u32 = SEG_BASE + 0x02;
/// 无类拒收。
pub const CL_ENONE: u32 = SEG_BASE + 0x03;
/// 类编号占位拒收。
pub const CL_ENUM: u32 = SEG_BASE + 0x04;
/// 类字节未知。
pub const CL_EOTHER: u32 = SEG_BASE + 0x05;
/// ident 不足。
pub const CL_ESHORT: u32 = SEG_BASE + 0x06;
/// 依赖类不符。
pub const CL_EDEP: u32 = SEG_BASE + 0x07;
/// 类与项宽自相矛盾。
pub const CL_EIDENT: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const CL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const CL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const CL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const CL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const CL_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 读类字节。
pub const OP_CLASS: u8 = 0x01;
/// 宿主兼容。
pub const OP_HOST: u8 = 0x02;
/// 项宽连坐。
pub const OP_WIDTH: u8 = 0x03;
/// 依赖链。
pub const OP_DEP: u8 = 0x04;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x05;
/// 文案。
pub const OP_MSG: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端序逐字节）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut shift = 56u32;
    loop {
        v = fold_byte(v, (((w >> shift) & 0xff) as u32) as u8);
        if shift == 0 {
            break;
        }
        shift -= 8;
    }
    v
}

/// 字节串摘要。
fn digest(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 文本
// ---------------------------------------------------------------------------

/// 十进制。
fn dec_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + ((n % 10) as u8);
        n /= 10;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 定宽小写十六进制。
fn hex_text(v: u32, digits: usize) -> String {
    let mut s = String::new();
    let mut n = digits;
    while n > 0 {
        n -= 1;
        let shift = (n as u32) * 4;
        let nib = ((v >> shift) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
        s.push(c as char);
    }
    s
}

/// `<name>: <msg>`。
pub fn named(name: &str, msg: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(msg);
    s
}

// ---------------------------------------------------------------------------
// 类
// ---------------------------------------------------------------------------

/// `e_ident[EI_CLASS]` 的分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElfClass {
    /// 无类。
    None,
    /// 32 位。
    C32,
    /// 64 位。
    C64,
    /// 类编号（保留）。
    Num,
    /// 未知字节。
    Other(u8),
}

impl ElfClass {
    /// 由字节分类。
    pub fn of_byte(b: u8) -> ElfClass {
        match b {
            ELFCLASSNONE => ElfClass::None,
            ELFCLASS32 => ElfClass::C32,
            ELFCLASS64 => ElfClass::C64,
            ELFCLASSNUM => ElfClass::Num,
            other => ElfClass::Other(other),
        }
    }

    /// 回写字节。
    pub fn byte(&self) -> u8 {
        match self {
            ElfClass::None => ELFCLASSNONE,
            ElfClass::C32 => ELFCLASS32,
            ElfClass::C64 => ELFCLASS64,
            ElfClass::Num => ELFCLASSNUM,
            ElfClass::Other(b) => *b,
        }
    }

    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            ElfClass::None => "ELFCLASSNONE",
            ElfClass::C32 => "ELFCLASS32",
            ElfClass::C64 => "ELFCLASS64",
            ElfClass::Num => "ELFCLASSNUM",
            ElfClass::Other(_) => "ELFCLASSNN",
        }
    }

    /// 标签：`ELFCLASS32` 或 `ELFCLASSNN/8`。
    pub fn tag(&self) -> String {
        match self {
            ElfClass::Other(b) => {
                let mut s = String::from("ELFCLASSNN/");
                s.push_str(dec_text(*b as u32).as_str());
                s
            }
            other => String::from(other.name()),
        }
    }

    /// 位宽（0 表示无效）。
    pub fn bits(&self) -> u32 {
        match self {
            ElfClass::C32 => 32,
            ElfClass::C64 => 64,
            _ => 0,
        }
    }

    /// 序号（账本观测用）。
    pub fn index(&self) -> u32 {
        match self {
            ElfClass::None => 0,
            ElfClass::C32 => 1,
            ElfClass::C64 => 2,
            ElfClass::Num => 3,
            ElfClass::Other(_) => 4,
        }
    }

    /// 是否合法类字节（0/1/2/3）。
    pub fn numeric(&self) -> bool {
        match self {
            ElfClass::Other(_) => false,
            _ => true,
        }
    }

    /// 是否 32 位家族。
    pub fn is32(&self) -> bool {
        match self {
            ElfClass::C32 => true,
            _ => false,
        }
    }

    /// 是否 64 位家族。
    pub fn is64(&self) -> bool {
        match self {
            ElfClass::C64 => true,
            _ => false,
        }
    }

    /// 下游尺寸。
    pub fn sizes(&self) -> Sizes {
        match self {
            ElfClass::C32 => Sizes { ehdr: EHDR32, phent: PHENT32, shent: SHENT32, limit: 0xffff_ffff },
            ElfClass::C64 => Sizes {
                ehdr: EHDR64,
                phent: PHENT64,
                shent: SHENT64,
                limit: 0xffff_ffff_ffff_ffff,
            },
            _ => Sizes { ehdr: 0, phent: 0, shent: 0, limit: 0 },
        }
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.index() as u64);
        h = fold_word(h, self.byte() as u64);
        h
    }
}

/// 类决定的下游尺寸。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sizes {
    /// Ehdr 长度。
    pub ehdr: u32,
    /// 程序头项长度。
    pub phent: u32,
    /// 节头项长度。
    pub shent: u32,
    /// 地址上界。
    pub limit: u64,
}

/// 类宽文案。
pub fn class_text(name: &str, class: ElfClass) -> String {
    let mut s = String::from(name);
    s.push_str(": wrong ELF class: ");
    s.push_str(class.tag().as_str());
    s
}

// ---------------------------------------------------------------------------
// 数据编码与 ABI
// ---------------------------------------------------------------------------

/// `e_ident[EI_DATA]`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Data {
    /// 小端。
    Lsb,
    /// 大端。
    Msb,
    /// 未知。
    Other(u8),
}

impl Data {
    /// 分类。
    pub fn of_byte(b: u8) -> Data {
        match b {
            ELFDATA2LSB => Data::Lsb,
            ELFDATA2MSB => Data::Msb,
            other => Data::Other(other),
        }
    }

    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Data::Lsb => "ELFDATA2LSB",
            Data::Msb => "ELFDATA2MSB",
            Data::Other(_) => "ELFDATANONE",
        }
    }

    /// 是否本机编码。
    pub fn native(&self) -> bool {
        match self {
            Data::Lsb => true,
            _ => false,
        }
    }
}

/// `e_ident[EI_OSABI]`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsAbi {
    /// System V。
    SysV,
    /// Linux。
    Linux,
    /// FreeBSD。
    FreeBsd,
    /// OpenBSD。
    OpenBsd,
    /// Solaris。
    Solaris,
    /// 其它。
    Other(u8),
}

impl OsAbi {
    /// 分类。
    pub fn of_byte(b: u8) -> OsAbi {
        match b {
            0 => OsAbi::SysV,
            3 => OsAbi::Linux,
            9 => OsAbi::FreeBsd,
            12 => OsAbi::OpenBsd,
            6 => OsAbi::Solaris,
            other => OsAbi::Other(other),
        }
    }

    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            OsAbi::SysV => "ELFOSABI_SYSV",
            OsAbi::Linux => "ELFOSABI_LINUX",
            OsAbi::FreeBsd => "ELFOSABI_FREEBSD",
            OsAbi::OpenBsd => "ELFOSABI_OPENBSD",
            OsAbi::Solaris => "ELFOSABI_SOLARIS",
            OsAbi::Other(_) => "ELFOSABI_NONE",
        }
    }

    /// 是否可装载。
    pub fn admitted(&self) -> bool {
        match self {
            OsAbi::SysV | OsAbi::Linux => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// e_ident
// ---------------------------------------------------------------------------

/// 十六字节 ident（不足补零，长度按实长记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ident {
    buf: [u8; EI_NIDENT],
    len: usize,
}

impl Ident {
    /// 从字节串取（多截少补）。
    pub fn new(bytes: &[u8]) -> Ident {
        let mut buf = [0u8; EI_NIDENT];
        let mut i = 0usize;
        while i < EI_NIDENT && i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                buf[i] = *b;
            }
            i += 1;
        }
        let len = if bytes.len() > EI_NIDENT { EI_NIDENT } else { bytes.len() };
        Ident { buf, len }
    }

    /// 造一份「魔数完好、类字节给定」的 ident。
    pub fn build(class: u8, len: usize) -> Ident {
        let mut buf = [0u8; EI_NIDENT];
        buf[0] = 0x7f;
        buf[1] = b'E';
        buf[2] = b'L';
        buf[3] = b'F';
        buf[EI_DATA] = ELFDATA2LSB;
        buf[EI_VERSION] = EV_CURRENT;
        if len > EI_CLASS {
            buf[EI_CLASS] = class;
        }
        let n = if len > EI_NIDENT { EI_NIDENT } else { len };
        Ident { buf, len: n }
    }

    /// 实长。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否够读类字节。
    pub fn complete(&self) -> bool {
        self.len >= MIN_CLASS
    }

    /// 取一字节。
    pub fn byte(&self, off: usize) -> Result<u8, u32> {
        if off >= self.len {
            return Err(CL_ESHORT);
        }
        match self.buf.get(off) {
            Some(b) => Ok(*b),
            None => Err(CL_ESHORT),
        }
    }

    /// 类字节原文。
    pub fn class_byte(&self) -> Result<u8, u32> {
        self.byte(EI_CLASS)
    }

    /// 类。
    pub fn class(&self) -> Result<ElfClass, u32> {
        match self.class_byte() {
            Ok(b) => Ok(ElfClass::of_byte(b)),
            Err(c) => Err(c),
        }
    }

    /// 数据编码。
    pub fn data(&self) -> Result<Data, u32> {
        match self.byte(EI_DATA) {
            Ok(b) => Ok(Data::of_byte(b)),
            Err(c) => Err(c),
        }
    }

    /// ident 版本。
    pub fn version(&self) -> Result<u8, u32> {
        self.byte(EI_VERSION)
    }

    /// ABI 版本。
    pub fn abiversion(&self) -> Result<u8, u32> {
        self.byte(EI_ABIVERSION)
    }

    /// OS ABI。
    pub fn osabi(&self) -> Result<OsAbi, u32> {
        match self.byte(EI_OSABI) {
            Ok(b) => Ok(OsAbi::of_byte(b)),
            Err(c) => Err(c),
        }
    }

    /// 是否 `\x7fELF`。
    pub fn magic_ok(&self) -> bool {
        self.len >= 4 && self.buf.get(0) == Some(&0x7f) && self.buf.get(3) == Some(&b'F')
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        fold_word(h, self.len as u64)
    }
}

impl Default for Ident {
    fn default() -> Ident {
        Ident::new(&[])
    }
}

// ---------------------------------------------------------------------------
// 宿主
// ---------------------------------------------------------------------------

/// 装载器宿主宽度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    /// LP64（x86_64 本机）。
    Lp64,
    /// ILP32。
    Ilp32,
}

impl Host {
    /// 本机。
    pub fn native() -> Host {
        Host::Lp64
    }

    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Host::Lp64 => "lp64",
            Host::Ilp32 => "ilp32",
        }
    }

    /// 宿主宽度。
    pub fn bits(&self) -> u32 {
        match self {
            Host::Lp64 => 64,
            Host::Ilp32 => 32,
        }
    }
}

/// 兼容结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compat {
    /// 同宽，可载。
    Native,
    /// 32 位类配 EM_X86_64（x32 ABI）。
    X32,
    /// 类比宿主窄。
    TooNarrow,
    /// 类比宿主宽。
    TooWide,
    /// 无类。
    Absent,
    /// 类字节未知 / 保留。
    Unknown,
}

impl Compat {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Compat::Native => "native",
            Compat::X32 => "x32",
            Compat::TooNarrow => "too-narrow",
            Compat::TooWide => "too-wide",
            Compat::Absent => "absent",
            Compat::Unknown => "unknown",
        }
    }

    /// 是否可载。
    pub fn admitted(&self) -> bool {
        match self {
            Compat::Native => true,
            _ => false,
        }
    }
}

/// 宿主 × 类 × 机种的兼容判据。
pub fn compat_of(host: Host, class: ElfClass, machine: u16) -> Compat {
    match class {
        ElfClass::None => Compat::Absent,
        ElfClass::Num => Compat::Unknown,
        ElfClass::Other(_) => Compat::Unknown,
        ElfClass::C32 => {
            if machine == EM_X86_64 {
                return Compat::X32;
            }
            match host {
                Host::Lp64 => Compat::TooNarrow,
                Host::Ilp32 => Compat::Native,
            }
        }
        ElfClass::C64 => match host {
            Host::Lp64 => Compat::Native,
            Host::Ilp32 => Compat::TooWide,
        },
    }
}

// ---------------------------------------------------------------------------
// 项宽连坐
// ---------------------------------------------------------------------------

/// `e_ehsize` 是否与类配套（0 视为未填，容忍）。
pub fn ehsize_ok(class: ElfClass, ehsize: u16) -> bool {
    if ehsize == 0 {
        return true;
    }
    let s = class.sizes();
    if s.ehdr == 0 {
        return true;
    }
    ehsize as u32 == s.ehdr
}

/// `e_phentsize` 是否与类配套（0 视为未填，容忍）。
pub fn phentsize_ok(class: ElfClass, phentsize: u16) -> bool {
    if phentsize == 0 {
        return true;
    }
    let s = class.sizes();
    if s.phent == 0 {
        return true;
    }
    phentsize as u32 == s.phent
}

/// `e_shentsize` 是否与类配套（0 视为无节表，容忍）。
pub fn shentsize_ok(class: ElfClass, shentsize: u16) -> bool {
    if shentsize == 0 {
        return true;
    }
    let s = class.sizes();
    if s.shent == 0 {
        return true;
    }
    shentsize as u32 == s.shent
}

/// 类与机种是否自洽。
pub fn machine_ok(class: ElfClass, machine: u16) -> bool {
    match class {
        ElfClass::C32 => machine == EM_386 || machine == EM_X86_64,
        ElfClass::C64 => machine == EM_X86_64,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 故障
// ---------------------------------------------------------------------------

/// 类闸门的失败根因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 32 位类。
    Class32,
    /// 64 位类。
    Class64,
    /// 无类。
    ClassNone,
    /// 类编号占位。
    ClassNum,
    /// 类字节未知。
    ClassOther,
    /// ident 不足，读不到类字节。
    ShortIdent,
    /// 依赖类不符（主体是依赖名）。
    DepMismatch,
    /// 类与项宽自相矛盾。
    IdentMismatch,
}

impl Fault {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Fault::Class32 => "class32",
            Fault::Class64 => "class64",
            Fault::ClassNone => "classnone",
            Fault::ClassNum => "classnum",
            Fault::ClassOther => "classother",
            Fault::ShortIdent => "short-ident",
            Fault::DepMismatch => "dep-mismatch",
            Fault::IdentMismatch => "ident-mismatch",
        }
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Class32 => CL_ECLASS32,
            Fault::Class64 => CL_ECLASS64,
            Fault::ClassNone => CL_ENONE,
            Fault::ClassNum => CL_ENUM,
            Fault::ClassOther => CL_EOTHER,
            Fault::ShortIdent => CL_ESHORT,
            Fault::DepMismatch => CL_EDEP,
            Fault::IdentMismatch => CL_EIDENT,
        }
    }

    /// 是否类不符族。
    pub fn class_family(&self) -> bool {
        match self {
            Fault::Class32 | Fault::Class64 | Fault::ClassNone | Fault::ClassNum => true,
            Fault::ClassOther => true,
            _ => false,
        }
    }

    /// 文案。
    pub fn message(&self, name: &str, class: ElfClass) -> String {
        match self {
            Fault::ShortIdent => named(name, "invalid ELF header"),
            Fault::IdentMismatch => named(name, "invalid ELF header"),
            _ => class_text(name, class),
        }
    }
}

// ---------------------------------------------------------------------------
// 结论
// ---------------------------------------------------------------------------

/// 一次类判定的结论。
#[derive(Clone, Debug)]
pub struct Judge {
    subject: String,
    root: String,
    class: ElfClass,
    machine: u16,
    host: Host,
    compat: Compat,
    fault: Option<Fault>,
    note: &'static str,
    sizes: Sizes,
}

impl Judge {
    /// 造一条结论。
    fn make(
        subject: &str,
        root: &str,
        class: ElfClass,
        machine: u16,
        host: Host,
        compat: Compat,
        fault: Option<Fault>,
        note: &'static str,
    ) -> Judge {
        Judge {
            subject: String::from(subject),
            root: String::from(root),
            class,
            machine,
            host,
            compat,
            fault,
            note,
            sizes: class.sizes(),
        }
    }

    /// 主体名（失败时打印的名字）。
    pub fn subject(&self) -> &str {
        self.subject.as_str()
    }

    /// 根对象名。
    pub fn root(&self) -> &str {
        self.root.as_str()
    }

    /// 类。
    pub const fn class(&self) -> ElfClass {
        self.class
    }

    /// 机种。
    pub const fn machine(&self) -> u16 {
        self.machine
    }

    /// 宿主。
    pub const fn host(&self) -> Host {
        self.host
    }

    /// 兼容结论。
    pub const fn compat(&self) -> Compat {
        self.compat
    }

    /// 根因。
    pub const fn fault(&self) -> Option<Fault> {
        self.fault
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        match self.fault {
            Some(f) => f.code(),
            None => 0,
        }
    }

    /// 是否可载。
    pub fn accepted(&self) -> bool {
        self.fault.is_none()
    }

    /// 备注。
    pub fn note(&self) -> &'static str {
        self.note
    }

    /// 下游尺寸。
    pub const fn sizes(&self) -> Sizes {
        self.sizes
    }

    /// dlerror 文案；可载时为空串。
    pub fn text(&self) -> String {
        match self.fault {
            Some(f) => f.message(self.subject.as_str(), self.class),
            None => String::new(),
        }
    }

    /// 诊断文本。
    pub fn diagnose(&self) -> String {
        let mut s = String::from(self.subject.as_str());
        s.push_str(": class=");
        s.push_str(self.class.tag().as_str());
        s.push_str(" bits=");
        s.push_str(dec_text(self.class.bits()).as_str());
        s.push_str(" ehdr=");
        s.push_str(dec_text(self.sizes.ehdr).as_str());
        s.push_str(" phent=");
        s.push_str(dec_text(self.sizes.phent).as_str());
        s.push_str(" compat=");
        s.push_str(self.compat.name());
        s.push_str(" fault=");
        match self.fault {
            Some(f) => s.push_str(f.name()),
            None => s.push_str("none"),
        }
        s.push_str(" note=");
        s.push_str(self.note);
        if self.root != self.subject {
            s.push_str(" root=");
            s.push_str(self.root.as_str());
        }
        s.push('\n');
        s
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.class.seal());
        h = fold_word(h, self.machine as u64);
        h = fold_word(h, self.code() as u64);
        h = fold_word(h, self.host.bits() as u64);
        h = fold_word(h, digest(self.subject.as_bytes()));
        h = fold_word(h, digest(self.note.as_bytes()));
        h
    }
}

/// 判一份对象的类。
pub fn judge_ident(name: &str, ident: &Ident, machine: u16, host: Host) -> Judge {
    let class = match ident.class() {
        Ok(c) => c,
        Err(_) => {
            return Judge::make(
                name,
                name,
                ElfClass::None,
                machine,
                host,
                Compat::Absent,
                Some(Fault::ShortIdent),
                "ident-short",
            );
        }
    };
    let compat = compat_of(host, class, machine);
    let mut note = "native";
    let fault = match compat {
        Compat::Native => None,
        Compat::X32 => {
            note = "x32-abi";
            Some(Fault::Class32)
        }
        Compat::TooNarrow => {
            note = "ilp32-on-lp64";
            Some(Fault::Class32)
        }
        Compat::TooWide => {
            note = "lp64-on-ilp32";
            Some(Fault::Class64)
        }
        Compat::Absent => {
            note = "class-none";
            Some(Fault::ClassNone)
        }
        Compat::Unknown => match class {
            ElfClass::Num => {
                note = "class-num";
                Some(Fault::ClassNum)
            }
            _ => {
                note = "class-other";
                Some(Fault::ClassOther)
            }
        },
    };
    Judge::make(name, name, class, machine, host, compat, fault, note)
}

/// 判根对象 + 一个依赖。
pub fn judge_dep(
    root: &str,
    ident: &Ident,
    machine: u16,
    dep_soname: &str,
    dep_class: u8,
    host: Host,
) -> Judge {
    let root_judge = judge_ident(root, ident, machine, host);
    if !root_judge.accepted() {
        return root_judge;
    }
    let dep = ElfClass::of_byte(dep_class);
    let compat = compat_of(host, dep, 0);
    if compat.admitted() {
        return Judge::make(
            root,
            root,
            root_judge.class(),
            machine,
            host,
            Compat::Native,
            None,
            "deps-ok",
        );
    }
    let fault = match dep {
        ElfClass::C32 => Fault::DepMismatch,
        ElfClass::C64 => Fault::DepMismatch,
        ElfClass::None => Fault::DepMismatch,
        ElfClass::Num => Fault::DepMismatch,
        ElfClass::Other(_) => Fault::DepMismatch,
    };
    Judge::make(
        dep_soname,
        root,
        dep,
        machine,
        host,
        compat,
        Some(fault),
        "needed",
    )
}

/// 判一条语料。
pub fn judge_case(c: &Case, host: Host) -> Judge {
    let ident = Ident::build(c.class, c.ident_len);
    if c.dep_soname.is_empty() {
        judge_ident(c.name, &ident, c.machine, host)
    } else {
        judge_dep(c.name, &ident, c.machine, c.dep_soname, c.dep_class, host)
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一条 64 位依赖样本。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// 对象名。
    pub name: &'static str,
    /// 类字节。
    pub class: u8,
    /// 机种。
    pub machine: u16,
    /// ident 实长。
    pub ident_len: usize,
    /// 依赖 soname（空表示无依赖）。
    pub dep_soname: &'static str,
    /// 依赖类字节。
    pub dep_class: u8,
    /// 期望根因（None 表示期望可载）。
    pub expect: Option<Fault>,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(
        name: &'static str,
        class: u8,
        machine: u16,
        ident_len: usize,
        dep_soname: &'static str,
        dep_class: u8,
        expect: Option<Fault>,
        note: &'static str,
    ) -> Case {
        Case { name, class, machine, ident_len, dep_soname, dep_class, expect, note }
    }
}

/// 十四卡：两成十二败。
pub const CASES: [Case; CORPUS] = [
    Case::new("libc.so.6", ELFCLASS64, EM_X86_64, 16, "", 0, None, "native-lp64"),
    Case::new("libm.so.6", ELFCLASS64, EM_X86_64, 16, "", 0, None, "native-lp64-libm"),
    Case::new("./ilp32.so", ELFCLASS32, EM_386, 16, "", 0, Some(Fault::Class32), "i386-so"),
    Case::new("./ilp32b.so", ELFCLASS32, EM_X86_64, 16, "", 0, Some(Fault::Class32), "x32-so"),
    Case::new("./none.so", ELFCLASSNONE, EM_X86_64, 16, "", 0, Some(Fault::ClassNone), "class-zero"),
    Case::new("./none2.so", ELFCLASSNONE, EM_386, 16, "", 0, Some(Fault::ClassNone), "class-zero-i386"),
    Case::new("./num.so", ELFCLASSNUM, EM_X86_64, 16, "", 0, Some(Fault::ClassNum), "class-num"),
    Case::new("./num2.so", ELFCLASSNUM, EM_386, 16, "", 0, Some(Fault::ClassNum), "class-num-i386"),
    Case::new("./ff.so", 0xff, EM_X86_64, 16, "", 0, Some(Fault::ClassOther), "class-ff"),
    Case::new("./big8.so", 8, EM_X86_64, 16, "", 0, Some(Fault::ClassOther), "class-eight"),
    Case::new(
        "./dep32.so",
        ELFCLASS64,
        EM_X86_64,
        16,
        "libfoo.so",
        ELFCLASS32,
        Some(Fault::DepMismatch),
        "needed-ilp32",
    ),
    Case::new(
        "./depnone.so",
        ELFCLASS64,
        EM_X86_64,
        16,
        "libbar.so",
        ELFCLASSNONE,
        Some(Fault::DepMismatch),
        "needed-classnone",
    ),
    Case::new("./short.so", ELFCLASS64, EM_X86_64, 4, "", 0, Some(Fault::ShortIdent), "ident-four"),
    Case::new("./ilp32c.so", ELFCLASS32, EM_ARM, 16, "", 0, Some(Fault::Class32), "class-before-machine"),
];

/// 取一条语料。
pub fn slot_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 名字与规格互异。
///
/// 两条正向样例（libc/libm）允许同类同机种——它们代表不同的命中路径；
/// 反例之间则不许出现 (类, 机种, 依赖, 期望) 四元组重复。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            if let Some(b) = CASES.get(j) {
                if b.name == a.name || b.note == a.note {
                    return false;
                }
                let same = b.class == a.class
                    && b.machine == a.machine
                    && b.dep_soname == a.dep_soname
                    && b.expect == a.expect;
                if same && a.expect.is_some() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(c) => {
                if c.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 逐条对判。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let j = judge_case(&c, Host::native());
        if j.fault() != c.expect {
            return false;
        }
        if j.accepted() != j.text().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    total: usize,
    ok: usize,
    fail: usize,
    c32: usize,
    cnone: usize,
    cnum: usize,
    cother: usize,
    short: usize,
    dep: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report {
            total: 0,
            ok: 0,
            fail: 0,
            c32: 0,
            cnone: 0,
            cnum: 0,
            cother: 0,
            short: 0,
            dep: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 可载条数。
    pub const fn ok(&self) -> usize {
        self.ok
    }

    /// 拒收条数。
    pub const fn fail(&self) -> usize {
        self.fail
    }

    /// 32 位类条数。
    pub const fn c32(&self) -> usize {
        self.c32
    }

    /// 无类条数。
    pub const fn cnone(&self) -> usize {
        self.cnone
    }

    /// 类编号条数。
    pub const fn cnum(&self) -> usize {
        self.cnum
    }

    /// 未知类条数。
    pub const fn cother(&self) -> usize {
        self.cother
    }

    /// ident 不足条数。
    pub const fn short(&self) -> usize {
        self.short
    }

    /// 依赖不符条数。
    pub const fn dep(&self) -> usize {
        self.dep
    }

    /// 对上条数。
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 是否红单。
    pub const fn red(&self) -> bool {
        self.red
    }

    /// 千分率。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            0
        } else if self.red {
            929
        } else {
            1000
        }
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(CL_ENOBS);
        }
        if self.red {
            return Err(CL_EBAL);
        }
        if self.matched != self.total || self.total != CORPUS {
            return Err(CL_EVALUE);
        }
        if self.ok != 2 || self.fail != 12 {
            return Err(CL_EVALUE);
        }
        if self.c32 != 3 || self.cnone != 2 || self.cnum != 2 || self.cother != 2 {
            return Err(CL_EVALUE);
        }
        if self.short != 1 || self.dep != 2 {
            return Err(CL_EVALUE);
        }
        Ok(())
    }

    /// 是否绿。
    pub fn green(&self) -> bool {
        match self.gate() {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("class total=");
        s.push_str(dec_text(self.total as u32).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok as u32).as_str());
        s.push_str(" fail=");
        s.push_str(dec_text(self.fail as u32).as_str());
        s.push_str(" c32=");
        s.push_str(dec_text(self.c32 as u32).as_str());
        s.push_str(" cnone=");
        s.push_str(dec_text(self.cnone as u32).as_str());
        s.push_str(" cnum=");
        s.push_str(dec_text(self.cnum as u32).as_str());
        s.push_str(" cother=");
        s.push_str(dec_text(self.cother as u32).as_str());
        s.push_str(" short=");
        s.push_str(dec_text(self.short as u32).as_str());
        s.push_str(" dep=");
        s.push_str(dec_text(self.dep as u32).as_str());
        s.push_str(" match=");
        s.push_str(dec_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑一遍。
pub fn run() -> Result<Report, u32> {
    walk(Host::native(), None)
}

/// 在指定宿主上跑。
pub fn run_on(host: Host) -> Result<Report, u32> {
    walk(host, None)
}

/// 跑一遍并注入红档。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(Host::native(), red)
}

/// 语料行走。
fn walk(host: Host, red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(CL_EVALUE),
        };
        let j = judge_case(&c, host);
        r.total += 1;
        match j.fault() {
            None => {
                r.ok += 1;
                if c.expect.is_none() {
                    r.matched += 1;
                }
            }
            Some(f) => {
                r.fail += 1;
                match f {
                    Fault::Class32 => r.c32 += 1,
                    Fault::Class64 => r.c32 += 1,
                    Fault::ClassNone => r.cnone += 1,
                    Fault::ClassNum => r.cnum += 1,
                    Fault::ClassOther => r.cother += 1,
                    Fault::ShortIdent => r.short += 1,
                    Fault::DepMismatch => r.dep += 1,
                    Fault::IdentMismatch => {}
                }
                if c.expect == Some(f) {
                    r.matched += 1;
                }
            }
        }
        i += 1;
    }
    if let Some((slot, _code)) = red {
        if slot >= r.total {
            return Err(CL_ESLOT);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 封面判据。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(CL_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CL_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(CL_EBAL);
        }
        const VALS: [u32; 8] = [14, 2, 12, 3, 2, 2, 2, 929];
        const CODES: [u32; 6] = [CL_ECLASS32, CL_ENONE, CL_ENUM, CL_EOTHER, CL_ESHORT, CL_EDEP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CL_EBAL),
            };
            if s.slot as usize != i {
                return Err(CL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CL_EVALUE);
                        }
                    }
                    None => return Err(CL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CL_EVALUE);
                        }
                    }
                    None => return Err(CL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CL_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CL_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(CL_ESEAL)
        }
    }

    /// 冻结指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 失败格。
fn err_cell(slot: u8, op: u8, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 某条语料的判定码。
fn case_code(i: usize) -> u32 {
    match CASES.get(i) {
        Some(c) => judge_case(c, Host::native()).code(),
        None => CL_ENOBS,
    }
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.ok())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.fail())));
    let _ = l.register(count_cell(3, OP_CLASS, r.map(|x| x.c32())));
    let _ = l.register(count_cell(4, OP_CLASS, r.map(|x| x.cnone())));
    let _ = l.register(count_cell(5, OP_CLASS, r.map(|x| x.cnum())));
    let _ = l.register(count_cell(6, OP_CLASS, r.map(|x| x.cother())));

    let red = run_with(Some((0, CL_ECLASS32)));
    let _ = l.register(count_cell(7, OP_HOST, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_CLASS, case_code(2)));
    let _ = l.register(err_cell(9, OP_CLASS, case_code(4)));
    let _ = l.register(err_cell(10, OP_CLASS, case_code(6)));
    let _ = l.register(err_cell(11, OP_CLASS, case_code(8)));
    let _ = l.register(err_cell(12, OP_WIDTH, case_code(12)));
    let _ = l.register(err_cell(13, OP_DEP, case_code(10)));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 类取值表。
fn table_class_ok() -> bool {
    let a = EI_NIDENT == 16 && EI_CLASS == 4 && MIN_CLASS == 5;
    let b = ElfClass::of_byte(0) == ElfClass::None;
    let c = ElfClass::of_byte(1) == ElfClass::C32;
    let d = ElfClass::of_byte(2) == ElfClass::C64;
    let e = ElfClass::of_byte(3) == ElfClass::Num;
    let f = ElfClass::of_byte(8) == ElfClass::Other(8);
    let g = ElfClass::C32.byte() == 1 && ElfClass::C64.byte() == 2;
    let h = ElfClass::Other(0xff).byte() == 0xff;
    let i = ElfClass::C32.name() == "ELFCLASS32" && ElfClass::C64.name() == "ELFCLASS64";
    let j = ElfClass::None.name() == "ELFCLASSNONE" && ElfClass::Num.name() == "ELFCLASSNUM";
    a && b && c && d && e && f && g && h && i && j
}

/// 类分类与尺寸。
fn class_ok() -> bool {
    let a = ElfClass::None.index() == 0 && ElfClass::Other(9).index() == 4;
    let b = ElfClass::C32.bits() == 32 && ElfClass::C64.bits() == 64;
    let c = ElfClass::None.bits() == 0 && ElfClass::Num.bits() == 0;
    let d = ElfClass::Num.numeric() && !ElfClass::Other(4).numeric();
    let e = ElfClass::C32.is32() && !ElfClass::C32.is64();
    let f = ElfClass::C64.is64() && !ElfClass::C64.is32();
    let g = ElfClass::C32.sizes().ehdr == 52 && ElfClass::C64.sizes().ehdr == 64;
    let h = ElfClass::C32.sizes().phent == 32 && ElfClass::C64.sizes().phent == 56;
    let i = ElfClass::C32.sizes().shent == 40 && ElfClass::C64.sizes().shent == 64;
    let j = ElfClass::C32.sizes().limit == 0xffff_ffff;
    let k = ElfClass::None.sizes().ehdr == 0;
    let m = ElfClass::C64.tag().as_str() == "ELFCLASS64"
        && ElfClass::Other(8).tag().as_str() == "ELFCLASSNN/8";
    a && b && c && d && e && f && g && h && i && j && k && m
}

/// 宿主兼容矩阵。
fn compat_ok() -> bool {
    let a = compat_of(Host::Lp64, ElfClass::C64, EM_X86_64) == Compat::Native;
    let b = compat_of(Host::Lp64, ElfClass::C32, EM_386) == Compat::TooNarrow;
    let c = compat_of(Host::Lp64, ElfClass::C32, EM_X86_64) == Compat::X32;
    let d = compat_of(Host::Lp64, ElfClass::None, EM_X86_64) == Compat::Absent;
    let e = compat_of(Host::Lp64, ElfClass::Num, EM_X86_64) == Compat::Unknown;
    let f = compat_of(Host::Lp64, ElfClass::Other(9), EM_X86_64) == Compat::Unknown;
    let g = compat_of(Host::Ilp32, ElfClass::C32, EM_386) == Compat::Native;
    let h = compat_of(Host::Ilp32, ElfClass::C64, EM_X86_64) == Compat::TooWide;
    let i = Compat::Native.admitted() && !Compat::X32.admitted();
    let j = Compat::TooNarrow.name() == "too-narrow" && Compat::TooWide.name() == "too-wide";
    let k = Host::native() == Host::Lp64 && Host::Lp64.bits() == 64 && Host::Ilp32.bits() == 32;
    a && b && c && d && e && f && g && h && i && j && k
}

/// 文案逐字。
fn text_lit_ok() -> bool {
    let a = class_text("./ilp32.so", ElfClass::C32).as_str()
        == "./ilp32.so: wrong ELF class: ELFCLASS32";
    let b = class_text("./wide.so", ElfClass::C64).as_str()
        == "./wide.so: wrong ELF class: ELFCLASS64";
    let c = class_text("./none.so", ElfClass::None).as_str()
        == "./none.so: wrong ELF class: ELFCLASSNONE";
    let d = class_text("./num.so", ElfClass::Num).as_str()
        == "./num.so: wrong ELF class: ELFCLASSNUM";
    let e = class_text("./odd.so", ElfClass::Other(8)).as_str()
        == "./odd.so: wrong ELF class: ELFCLASSNN/8";
    let f = Fault::ShortIdent.message("./short.so", ElfClass::None).as_str()
        == "./short.so: invalid ELF header";
    let g = Fault::DepMismatch.message("libfoo.so", ElfClass::C32).as_str()
        == "libfoo.so: wrong ELF class: ELFCLASS32";
    let h = named("a.so", "b").as_str() == "a.so: b";
    a && b && c && d && e && f && g && h
}

/// ident 读取与短档。
fn ident_ok() -> bool {
    let full = Ident::build(ELFCLASS32, 16);
    let a = full.len() == 16 && full.complete() && full.magic_ok();
    let b = full.class_byte() == Ok(1) && full.class() == Ok(ElfClass::C32);
    let c = full.data() == Ok(Data::Lsb) && full.version() == Ok(EV_CURRENT);
    let d = full.osabi() == Ok(OsAbi::SysV) && full.abiversion() == Ok(0);
    let e = full.byte(EI_ABIVERSION) == Ok(0) && full.byte(EI_NIDENT) == Err(CL_ESHORT);
    let short = Ident::build(ELFCLASS64, 4);
    let f = short.len() == 4 && !short.complete() && short.class() == Err(CL_ESHORT);
    let g = short.magic_ok() && !Ident::default().magic_ok();
    let h = Ident::default().is_empty() && Ident::default().class() == Err(CL_ESHORT);
    let i = {
        let v = judge_ident("./short.so", &short, EM_X86_64, Host::native());
        v.fault() == Some(Fault::ShortIdent) && v.code() == CL_ESHORT
    };
    let j = judge_ident("./short.so", &short, EM_X86_64, Host::native()).text().as_str()
        == "./short.so: invalid ELF header";
    a && b && c && d && e && f && g && h && i && j
}

/// 项宽连坐与机种自洽。
fn width_ok() -> bool {
    let a = ehsize_ok(ElfClass::C32, 52) && ehsize_ok(ElfClass::C64, 64);
    let b = !ehsize_ok(ElfClass::C32, 64) && !ehsize_ok(ElfClass::C64, 52);
    let c = ehsize_ok(ElfClass::C32, 0) && ehsize_ok(ElfClass::None, 99);
    let d = phentsize_ok(ElfClass::C32, 32) && phentsize_ok(ElfClass::C64, 56);
    let e = !phentsize_ok(ElfClass::C32, 56) && !phentsize_ok(ElfClass::C64, 32);
    let f = shentsize_ok(ElfClass::C32, 40) && shentsize_ok(ElfClass::C64, 64);
    let g = !shentsize_ok(ElfClass::C32, 64);
    let h = machine_ok(ElfClass::C32, EM_386) && machine_ok(ElfClass::C32, EM_X86_64);
    let i = machine_ok(ElfClass::C64, EM_X86_64) && !machine_ok(ElfClass::C64, EM_386);
    let j = !machine_ok(ElfClass::None, EM_X86_64) && !machine_ok(ElfClass::Other(4), 0);
    a && b && c && d && e && f && g && h && i && j
}

/// 依赖链。
fn dep_ok() -> bool {
    let a = {
        let j = judge_case(&CASES[10], Host::native());
        j.fault() == Some(Fault::DepMismatch)
            && j.subject() == "libfoo.so"
            && j.root() == "./dep32.so"
            && j.code() == CL_EDEP
    };
    let b = judge_case(&CASES[10], Host::native()).text().as_str()
        == "libfoo.so: wrong ELF class: ELFCLASS32";
    let c = {
        let j = judge_case(&CASES[11], Host::native());
        j.subject() == "libbar.so" && j.class() == ElfClass::None
    };
    let d = judge_case(&CASES[11], Host::native()).text().as_str()
        == "libbar.so: wrong ELF class: ELFCLASSNONE";
    let e = {
        let ident = Ident::build(ELFCLASS64, 16);
        let j = judge_dep("./root.so", &ident, EM_X86_64, "libok.so", ELFCLASS64, Host::native());
        j.accepted() && j.note() == "deps-ok" && j.subject() == "./root.so"
    };
    let f = judge_case(&CASES[10], Host::native()).diagnose().as_str()
        == "libfoo.so: class=ELFCLASS32 bits=32 ehdr=52 phent=32 compat=too-narrow fault=dep-mismatch note=needed root=./dep32.so\n";
    a && b && c && d && e && f
}

/// x32 诊断。
fn x32_ok() -> bool {
    let j = judge_case(&CASES[3], Host::native());
    let a = j.compat() == Compat::X32 && j.fault() == Some(Fault::Class32);
    let b = j.note() == "x32-abi";
    let c = j.diagnose().as_str()
        == "./ilp32b.so: class=ELFCLASS32 bits=32 ehdr=52 phent=32 compat=x32 fault=class32 note=x32-abi\n";
    let d = j.machine() == EM_X86_64 && j.class() == ElfClass::C32;
    a && b && c && d
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.ok() == 2 && r.fail() == 12 && r.matched() == CORPUS;
    let b = r.c32() == 3 && r.cnone() == 2 && r.cnum() == 2 && r.cother() == 2;
    let c = r.short() == 1 && r.dep() == 2 && r.rate() == 1000 && r.green();
    let d = match run_on(Host::Ilp32) {
        Ok(v) => v.total() == CORPUS && v.ok() == 2 && v.fail() == 12,
        Err(_) => false,
    };
    a && b && c && d
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(c) => c.class == ELFCLASS64 && c.expect.is_none(),
        None => false,
    };
    let b = match slot_at(13) {
        Some(c) => c.class == ELFCLASS32 && c.expect == Some(Fault::Class32),
        None => false,
    };
    let c = slot_at(CORPUS).is_none();
    a && b && c
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "class total=14 ok=2 fail=12 c32=3 cnone=2 cnum=2 cother=2 short=1 dep=2 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(CL_ENOBS);
    let c = e.render().as_str()
        == "class total=0 ok=0 fail=0 c32=0 cnone=0 cnum=0 cother=0 short=0 dep=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((1, CL_ECLASS32))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929 && r.red();
    let b = r.gate() == Err(CL_EBAL) && !r.green();
    let c = run_with(Some((CORPUS, CL_ENONE))).err() == Some(CL_ESLOT);
    let d = run_with(Some((CORPUS + 1, CL_ENONE))).err() == Some(CL_ESLOT);
    a && b && c && d
}

/// 封面。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(CL_ENOBS) && cover(0) == Err(CL_ENOBS)
}

/// 摘要与文本。
fn hash_ok() -> bool {
    let a = ElfClass::C32.seal() == ElfClass::C32.seal();
    let b = ElfClass::C32.seal() != ElfClass::C64.seal();
    let c = digest(b"abcdef") == digest(b"abcdef") && digest(b"abc") != digest(b"abd");
    let d = hex_text(0x2a, 8).as_str() == "0000002a" && dec_text(4660).as_str() == "4660";
    let e = {
        let j = judge_case(&CASES[2], Host::native());
        j.seal() == judge_case(&CASES[2], Host::native()).seal()
    };
    let f = judge_case(&CASES[2], Host::native()).seal()
        != judge_case(&CASES[3], Host::native()).seal();
    let g = Ident::build(1, 16).seal() == Ident::build(1, 16).seal();
    let h = Ident::build(1, 16).seal() != Ident::build(2, 16).seal();
    let i = Data::of_byte(2).name() == "ELFDATA2MSB" && !Data::of_byte(2).native();
    let j = OsAbi::of_byte(3).admitted() && !OsAbi::of_byte(9).admitted();
    a && b && c && d && e && f && g && h && i && j
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = table_class_ok() && class_ok();
    out[1] = compat_ok() && width_ok();
    out[2] = text_lit_ok() && ident_ok() && x32_ok() && dep_ok();
    out[3] = corpus_ok()
        && slots_ok()
        && report_ok()
        && reds_ok()
        && cover_ok()
        && hash_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11623_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11623.so_class");

    s.add("CL01-类取值表", table_class_ok() && class_ok(), "static");
    s.add("CL02-类分类", class_ok() && compat_ok(), "static");
    s.add("CL03-宿主兼容", compat_ok(), "static");
    s.add("CL04-文案逐字", text_lit_ok(), "static");
    s.add("CL05-ident读取", ident_ok(), "static");
    s.add("CL06-项宽连坐", width_ok(), "static");
    s.add("CL07-依赖类不符", dep_ok(), "static");
    s.add("CL08-x32诊断", x32_ok(), "static");
    s.add("CL09-十四卡全跑", corpus_ok(), "static");
    s.add("CL10-语料与渲染", slots_ok() && report_ok(), "static");
    s.add("CL11-红单封面", reds_ok() && cover_ok(), "static");
    s.add("CL12-摘要与文本", hash_ok(), "static");
    s.add("CL13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_shape() {
        assert_eq!(CASES.len(), CORPUS);
        assert_eq!(CORPUS, 14);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
    }

    #[test]
    fn class_bytes() {
        assert_eq!(ElfClass::of_byte(0), ElfClass::None);
        assert_eq!(ElfClass::of_byte(1), ElfClass::C32);
        assert_eq!(ElfClass::of_byte(2), ElfClass::C64);
        assert_eq!(ElfClass::of_byte(3), ElfClass::Num);
        assert_eq!(ElfClass::of_byte(0x7f), ElfClass::Other(0x7f));
        assert_eq!(ElfClass::C32.byte(), 1);
    }

    #[test]
    fn class_sizes() {
        assert_eq!(ElfClass::C32.sizes().ehdr, EHDR32);
        assert_eq!(ElfClass::C64.sizes().ehdr, EHDR64);
        assert_eq!(ElfClass::C32.sizes().phent, PHENT32);
        assert_eq!(ElfClass::C64.sizes().phent, PHENT64);
        assert_eq!(ElfClass::None.sizes().ehdr, 0);
        assert_eq!(ElfClass::C32.sizes().limit, 0xffff_ffff);
    }

    #[test]
    fn compat_matrix() {
        assert_eq!(compat_of(Host::Lp64, ElfClass::C64, EM_X86_64), Compat::Native);
        assert_eq!(compat_of(Host::Lp64, ElfClass::C32, EM_386), Compat::TooNarrow);
        assert_eq!(compat_of(Host::Ilp32, ElfClass::C64, EM_X86_64), Compat::TooWide);
        assert_eq!(compat_of(Host::Lp64, ElfClass::None, 0), Compat::Absent);
        assert_eq!(compat_of(Host::Lp64, ElfClass::Num, 0), Compat::Unknown);
    }

    #[test]
    fn x32_detected() {
        let j = judge_case(&CASES[3], Host::native());
        assert_eq!(j.compat(), Compat::X32);
        assert_eq!(j.fault(), Some(Fault::Class32));
        assert_eq!(j.note(), "x32-abi");
    }

    #[test]
    fn class_texts() {
        assert_eq!(class_text("a.so", ElfClass::C32).as_str(), "a.so: wrong ELF class: ELFCLASS32");
        assert_eq!(class_text("a.so", ElfClass::None).as_str(), "a.so: wrong ELF class: ELFCLASSNONE");
        assert_eq!(class_text("a.so", ElfClass::Num).as_str(), "a.so: wrong ELF class: ELFCLASSNUM");
        assert_eq!(
            class_text("a.so", ElfClass::Other(8)).as_str(),
            "a.so: wrong ELF class: ELFCLASSNN/8"
        );
    }

    #[test]
    fn short_ident() {
        let id = Ident::build(ELFCLASS64, 4);
        assert_eq!(id.class(), Err(CL_ESHORT));
        let j = judge_ident("./s.so", &id, EM_X86_64, Host::native());
        assert_eq!(j.fault(), Some(Fault::ShortIdent));
        assert_eq!(j.text().as_str(), "./s.so: invalid ELF header");
    }

    #[test]
    fn phent_consistency() {
        assert!(phentsize_ok(ElfClass::C32, 32));
        assert!(phentsize_ok(ElfClass::C64, 56));
        assert!(!phentsize_ok(ElfClass::C64, 32));
        assert!(ehsize_ok(ElfClass::C32, 52));
        assert!(!ehsize_ok(ElfClass::C32, 64));
        assert!(shentsize_ok(ElfClass::C64, 64));
    }

    #[test]
    fn machine_class_agree() {
        assert!(machine_ok(ElfClass::C32, EM_386));
        assert!(machine_ok(ElfClass::C32, EM_X86_64));
        assert!(machine_ok(ElfClass::C64, EM_X86_64));
        assert!(!machine_ok(ElfClass::C64, EM_386));
    }

    #[test]
    fn dep_mismatch_named() {
        let j = judge_case(&CASES[10], Host::native());
        assert_eq!(j.subject(), "libfoo.so");
        assert_eq!(j.root(), "./dep32.so");
        assert_eq!(j.code(), CL_EDEP);
        assert_eq!(j.text().as_str(), "libfoo.so: wrong ELF class: ELFCLASS32");
    }

    #[test]
    fn diagnose_text() {
        assert_eq!(
            judge_case(&CASES[3], Host::native()).diagnose().as_str(),
            "./ilp32b.so: class=ELFCLASS32 bits=32 ehdr=52 phent=32 compat=x32 fault=class32 note=x32-abi\n"
        );
    }

    #[test]
    fn report_render() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(
            r.render().as_str(),
            "class total=14 ok=2 fail=12 c32=3 cnone=2 cnum=2 cother=2 short=1 dep=2 match=14 rate=1000ppm\n"
        );
        assert_eq!(r.gate(), Ok(()));
        assert!(r.green());
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((1, CL_ECLASS32))) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.matched(), CORPUS - 1);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(CL_EBAL));
        assert_eq!(run_with(Some((14, CL_ENONE))).err(), Some(CL_ESLOT));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(CL_EBAL));
        assert!(l.rejects() >= 1);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0]);
        assert!(out[1]);
        assert!(out[2]);
        assert!(out[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11623_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}


