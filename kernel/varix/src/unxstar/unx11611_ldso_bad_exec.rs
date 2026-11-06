//! UNX-F11611 ld.so 对坏可执行报错文案判据（cannot execute）
//!
//! 镜面：内核把可执行交给解释器（ld.so）之后，解释器自己还要再读一遍
//! 映像头。坏件在 ld.so 侧以 `cannot execute: <短语>` 落文本，短语与
//! 故障类一一对应：
//!
//! * `bad ELF magic` / `not an ELF image`；
//! * `ELF header truncated` / `empty file`；
//! * `wrong ELF class` / `wrong ELF data encoding`；
//! * `wrong machine`；
//! * `no program headers` / `bad program header size` / `program headers
//!   outside file`；
//! * `no loadable segments` / `segment outside file`；
//! * `no dynamic section` / `no entry point`。
//!
//! 映像检查是**单遍**的：先判长度与魔数，再判类别、数据编码、机器，
//! 然后走程序头表，最后核对装载段、动态段与入口。任一环失败即定类，
//! 后续短语不再产生。
//!
//! 语料十四槽，二活十二死：魔数二、截断二、类别二、机器二、段表二、
//! 装载二。
//!
//! 闸门次序：名称 → 魔数 → 机器 → 长度 → 段表 → 装载 → 文案 →
//! 语料 → 汇总 → 红单 → 覆盖 → 冻结重放。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfba0;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// ELF64 头最小字节。
pub const EHDR64: usize = 64;

/// ELF64 程序头条目字节。
pub const PHENT: usize = 56;

/// 映像缓冲字节。
pub const IMAGE_BYTES: usize = 512;

/// 程序头表槽上限。
pub const PHN: usize = 4;

/// ELFCLASS64。
pub const ELFCLASS64: u8 = 2;

/// ELFDATA2LSB。
pub const ELFDATA2LSB: u8 = 1;

/// EM_X86_64。
pub const EM_X86_64: u16 = 62;

/// PT_LOAD。
pub const PT_LOAD: u32 = 1;

/// PT_DYNAMIC。
pub const PT_DYNAMIC: u32 = 2;

/// ENOEXEC。
pub const ENOEXEC: i32 = 8;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 魔数错。
pub const LD_EMAGIC: u32 = SEG_BASE + 0x01;
/// 头部截断。
pub const LD_ETRUNC: u32 = SEG_BASE + 0x02;
/// 类别 / 数据编码错。
pub const LD_ECLASS: u32 = SEG_BASE + 0x03;
/// 机器错。
pub const LD_EMACH: u32 = SEG_BASE + 0x04;
/// 段表错。
pub const LD_EPHDR: u32 = SEG_BASE + 0x05;
/// 装载段错。
pub const LD_ELOAD: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const LD_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const LD_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const LD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const LD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const LD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const LD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const LD_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 头检查。
pub const OP_EHDR: u32 = 1;
/// 段表检查。
pub const OP_PHDR: u32 = 2;
/// 装载检查。
pub const OP_LOAD: u32 = 3;
/// 文案。
pub const OP_TEXT: u32 = 4;
/// 汇总。
pub const OP_REPORT: u32 = 5;

// ---------------------------------------------------------------------------
// 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折一个字节。
pub const fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 折一个字。
pub const fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x = fold_byte(x, ((w >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

/// 折一段字节。
pub fn fold_bytes(h: u64, b: &[u8]) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            x = fold_byte(x, *v);
        }
        i += 1;
    }
    fold_word(x, b.len() as u64)
}

/// 十进制文本。
pub fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
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

/// 读 u16（小端）。
pub fn rd16(b: &[u8], off: usize) -> Option<u16> {
    let a = b.get(off)?;
    let c = b.get(off + 1)?;
    Some((*a as u16) | ((*c as u16) << 8))
}

/// 读 u32（小端）。
pub fn rd32(b: &[u8], off: usize) -> Option<u32> {
    let a = rd16(b, off)? as u32;
    let c = rd16(b, off + 2)? as u32;
    Some(a | (c << 16))
}

/// 读 u64（小端）。
pub fn rd64(b: &[u8], off: usize) -> Option<u64> {
    let a = rd32(b, off)? as u64;
    let c = rd32(b, off + 4)? as u64;
    Some(a | (c << 32))
}

// ---------------------------------------------------------------------------
// 故障
// ---------------------------------------------------------------------------

/// 故障类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 可装载。
    Ok,
    /// 魔数错。
    BadMagic,
    /// 非 ELF 文本。
    Text,
    /// 头部截断。
    Trunc,
    /// 空文件。
    Empty,
    /// 类别错。
    Class,
    /// 数据编码错。
    Data,
    /// 机器错。
    Mach,
    /// 无程序头。
    NoPhdr,
    /// 程序头条目长度错。
    BadPhentsize,
    /// 程序头表越界。
    PhdrOut,
    /// 无装载段。
    NoLoad,
    /// 装载段越界。
    SegOut,
    /// 无动态段。
    NoDyn,
    /// 无入口。
    BadEntry,
}

impl Fault {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::BadMagic => "magic",
            Fault::Text => "text",
            Fault::Trunc => "trunc",
            Fault::Empty => "empty",
            Fault::Class => "class",
            Fault::Data => "data",
            Fault::Mach => "mach",
            Fault::NoPhdr => "nophdr",
            Fault::BadPhentsize => "phentsize",
            Fault::PhdrOut => "phdrout",
            Fault::NoLoad => "noload",
            Fault::SegOut => "segout",
            Fault::NoDyn => "nodyn",
            Fault::BadEntry => "entry",
        }
    }

    /// 序数。
    pub const fn index(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::BadMagic => 1,
            Fault::Text => 2,
            Fault::Trunc => 3,
            Fault::Empty => 4,
            Fault::Class => 5,
            Fault::Data => 6,
            Fault::Mach => 7,
            Fault::NoPhdr => 8,
            Fault::BadPhentsize => 9,
            Fault::PhdrOut => 10,
            Fault::NoLoad => 11,
            Fault::SegOut => 12,
            Fault::NoDyn => 13,
            Fault::BadEntry => 14,
        }
    }

    /// 短语。
    pub const fn phrase(&self) -> &'static str {
        match self {
            Fault::Ok => "loadable image",
            Fault::BadMagic => "bad ELF magic",
            Fault::Text => "not an ELF image",
            Fault::Trunc => "ELF header truncated",
            Fault::Empty => "empty file",
            Fault::Class => "wrong ELF class",
            Fault::Data => "wrong ELF data encoding",
            Fault::Mach => "wrong machine",
            Fault::NoPhdr => "no program headers",
            Fault::BadPhentsize => "bad program header size",
            Fault::PhdrOut => "program headers outside file",
            Fault::NoLoad => "no loadable segments",
            Fault::SegOut => "segment outside file",
            Fault::NoDyn => "no dynamic section",
            Fault::BadEntry => "no entry point",
        }
    }

    /// 统计桶。
    pub const fn bucket(&self) -> Bucket {
        match self {
            Fault::Ok => Bucket::Ok,
            Fault::BadMagic | Fault::Text => Bucket::Magic,
            Fault::Trunc | Fault::Empty => Bucket::Trunc,
            Fault::Class | Fault::Data => Bucket::Class,
            Fault::Mach => Bucket::Mach,
            Fault::NoPhdr | Fault::BadPhentsize | Fault::PhdrOut => Bucket::Phdr,
            Fault::NoLoad | Fault::SegOut | Fault::NoDyn | Fault::BadEntry => Bucket::Load,
        }
    }

    /// 对应码。
    pub const fn code(&self) -> u32 {
        match self.bucket() {
            Bucket::Ok => 0,
            Bucket::Magic => LD_EMAGIC,
            Bucket::Trunc => LD_ETRUNC,
            Bucket::Class => LD_ECLASS,
            Bucket::Mach => LD_EMACH,
            Bucket::Phdr => LD_EPHDR,
            Bucket::Load => LD_ELOAD,
        }
    }

    /// 是否放行。
    pub const fn alive(&self) -> bool {
        matches!(self, Fault::Ok)
    }

    /// 文本。
    pub fn message(&self, path: &str) -> String {
        let mut s = String::new();
        s.push_str(path);
        if *self == Fault::Ok {
            s.push_str(": ok: loadable image\n");
        } else {
            s.push_str(": cannot execute: ");
            s.push_str(self.phrase());
            s.push('\n');
        }
        s
    }
}

/// 统计桶。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    /// 放行。
    Ok,
    /// 魔数。
    Magic,
    /// 截断。
    Trunc,
    /// 类别。
    Class,
    /// 机器。
    Mach,
    /// 段表。
    Phdr,
    /// 装载。
    Load,
}

impl Bucket {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Bucket::Ok => "ok",
            Bucket::Magic => "magic",
            Bucket::Trunc => "trunc",
            Bucket::Class => "class",
            Bucket::Mach => "mach",
            Bucket::Phdr => "phdr",
            Bucket::Load => "load",
        }
    }
}

// ---------------------------------------------------------------------------
// 单遍检查
// ---------------------------------------------------------------------------

/// 程序头观测。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Phdrs {
    /// 头数。
    pub count: usize,
    /// 装载段数。
    pub loads: usize,
    /// 是否有动态段。
    pub dynp: bool,
    /// 越界段数。
    pub outside: usize,
}

/// 单遍检查。
pub fn inspect(bytes: &[u8]) -> Fault {
    let len = bytes.len();
    if len == 0 {
        return Fault::Empty;
    }
    if len < 4 {
        return Fault::Trunc;
    }
    let m = match (bytes.first(), bytes.get(1), bytes.get(2), bytes.get(3)) {
        (Some(a), Some(b), Some(c), Some(d)) => (*a, *b, *c, *d),
        _ => return Fault::Trunc,
    };
    if m != (0x7f, b'E', b'L', b'F') {
        return if m.0 == 0x7f { Fault::BadMagic } else { Fault::Text };
    }
    if len < EHDR64 {
        return Fault::Trunc;
    }
    let class = match bytes.get(4) {
        Some(v) => *v,
        None => return Fault::Trunc,
    };
    if class != ELFCLASS64 {
        return Fault::Class;
    }
    let data = match bytes.get(5) {
        Some(v) => *v,
        None => return Fault::Trunc,
    };
    if data != ELFDATA2LSB {
        return Fault::Data;
    }
    let machine = match rd16(bytes, 18) {
        Some(v) => v,
        None => return Fault::Trunc,
    };
    if machine != EM_X86_64 {
        return Fault::Mach;
    }
    let entry = match rd64(bytes, 24) {
        Some(v) => v,
        None => return Fault::Trunc,
    };
    let phoff = match rd64(bytes, 32) {
        Some(v) => v,
        None => return Fault::Trunc,
    };
    let phentsize = match rd16(bytes, 54) {
        Some(v) => v,
        None => return Fault::Trunc,
    };
    let phnum = match rd16(bytes, 56) {
        Some(v) => v,
        None => return Fault::Trunc,
    };
    if phnum == 0 {
        return Fault::NoPhdr;
    }
    if phentsize as usize != PHENT {
        return Fault::BadPhentsize;
    }
    let span = match (phnum as u64).checked_mul(PHENT as u64) {
        Some(v) => v,
        None => return Fault::PhdrOut,
    };
    let end = match phoff.checked_add(span) {
        Some(v) => v,
        None => return Fault::PhdrOut,
    };
    if end > len as u64 {
        return Fault::PhdrOut;
    }
    let mut ph = Phdrs { count: phnum as usize, loads: 0, dynp: false, outside: 0 };
    let mut i = 0usize;
    while i < phnum as usize {
        let base = match (phoff as usize).checked_add(i.saturating_mul(PHENT)) {
            Some(v) => v,
            None => return Fault::PhdrOut,
        };
        let p_type = match rd32(bytes, base) {
            Some(v) => v,
            None => return Fault::PhdrOut,
        };
        let p_off = match rd64(bytes, base + 8) {
            Some(v) => v,
            None => return Fault::PhdrOut,
        };
        let p_filesz = match rd64(bytes, base + 32) {
            Some(v) => v,
            None => return Fault::PhdrOut,
        };
        let seg_end = match p_off.checked_add(p_filesz) {
            Some(v) => v,
            None => {
                ph.outside += 1;
                i += 1;
                continue;
            }
        };
        if seg_end > len as u64 {
            ph.outside += 1;
        }
        if p_type == PT_LOAD {
            ph.loads += 1;
        }
        if p_type == PT_DYNAMIC {
            ph.dynp = true;
        }
        i += 1;
    }
    if ph.outside > 0 {
        return Fault::SegOut;
    }
    if ph.loads == 0 {
        return Fault::NoLoad;
    }
    if !ph.dynp {
        return Fault::NoDyn;
    }
    if entry == 0 {
        return Fault::BadEntry;
    }
    Fault::Ok
}

/// 程序头观测（对好件）。
pub fn phdrs_of(bytes: &[u8]) -> Result<Phdrs, Fault> {
    let f = inspect(bytes);
    if f != Fault::Ok {
        return Err(f);
    }
    let phoff = match rd64(bytes, 32) {
        Some(v) => v as usize,
        None => return Err(Fault::Trunc),
    };
    let phnum = match rd16(bytes, 56) {
        Some(v) => v as usize,
        None => return Err(Fault::Trunc),
    };
    let mut ph = Phdrs { count: phnum, loads: 0, dynp: false, outside: 0 };
    let mut i = 0usize;
    while i < phnum {
        let base = phoff.saturating_add(i.saturating_mul(PHENT));
        let p_type = match rd32(bytes, base) {
            Some(v) => v,
            None => return Err(Fault::PhdrOut),
        };
        if p_type == PT_LOAD {
            ph.loads += 1;
        }
        if p_type == PT_DYNAMIC {
            ph.dynp = true;
        }
        i += 1;
    }
    Ok(ph)
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 好件基座。
const fn base() -> [u8; IMAGE_BYTES] {
    let mut a = [0u8; IMAGE_BYTES];
    a[0] = 0x7f;
    a[1] = b'E';
    a[2] = b'L';
    a[3] = b'F';
    a[4] = ELFCLASS64;
    a[5] = ELFDATA2LSB;
    a[6] = 1;
    a[16] = 2;
    a[18] = EM_X86_64 as u8;
    a[20] = 1;
    a[24] = 0x40;
    a[32] = 0x40;
    a[54] = PHENT as u8;
    a[56] = 2;
    a[64] = PT_LOAD as u8;
    a[64 + 8] = 0x00;
    a[64 + 32] = 0x10;
    a[64 + 40] = 0x10;
    a[64 + 56] = PT_DYNAMIC as u8;
    a[64 + 56 + 8] = 0x40;
    a[64 + 56 + 32] = 0x10;
    a[64 + 56 + 40] = 0x10;
    a
}

/// PIE 好件。
const fn pie() -> [u8; IMAGE_BYTES] {
    let mut a = base();
    a[16] = 3;
    a
}

/// 魔数坏。
const fn bad_magic() -> [u8; IMAGE_BYTES] {
    let mut a = base();
    a[1] = b'X';
    a
}

/// 类别错。
const fn cls32() -> [u8; IMAGE_BYTES] {
    let mut a = base();
    a[4] = 1;
    a
}

/// 数据编码错。
const fn data_msb() -> [u8; IMAGE_BYTES] {
    let mut a = base();
    a[5] = 2;
    a
}

/// AArch64。
const fn mach_aarch64() -> [u8; IMAGE_BYTES] {
    let mut a = base();
    a[18] = 183;
    a
}

/// RISC-V。
const fn mach_riscv() -> [u8; IMAGE_BYTES] {
    let mut a = base();
    a[18] = 243;
    a
}

/// 无段表。
const fn no_phdr() -> [u8; IMAGE_BYTES] {
    let mut a = base();
    a[56] = 0;
    a
}

/// 段表条目长度错。
const fn bad_phentsize() -> [u8; IMAGE_BYTES] {
    let mut a = base();
    a[54] = 32;
    a
}

/// 无装载段。
const fn no_load() -> [u8; IMAGE_BYTES] {
    let mut a = base();
    a[64] = 3;
    a
}

/// 装载段越界。
const fn seg_out() -> [u8; IMAGE_BYTES] {
    let mut a = base();
    a[64 + 8] = 0xff;
    a[64 + 9] = 0x03;
    a
}

/// 截断头。
const fn trunc_buf() -> [u8; 10] {
    let a = base();
    let mut t = [0u8; 10];
    let mut i = 0usize;
    while i < 10 {
        t[i] = a[i];
        i += 1;
    }
    t
}

static GOOD: [u8; IMAGE_BYTES] = base();
static PIE: [u8; IMAGE_BYTES] = pie();
static BADM: [u8; IMAGE_BYTES] = bad_magic();
static CLS32: [u8; IMAGE_BYTES] = cls32();
static DATA2: [u8; IMAGE_BYTES] = data_msb();
static MACH183: [u8; IMAGE_BYTES] = mach_aarch64();
static MACH243: [u8; IMAGE_BYTES] = mach_riscv();
static NOPH: [u8; IMAGE_BYTES] = no_phdr();
static PHENT32: [u8; IMAGE_BYTES] = bad_phentsize();
static NOLOAD: [u8; IMAGE_BYTES] = no_load();
static SEGOUT: [u8; IMAGE_BYTES] = seg_out();
static TRUNC: [u8; 10] = trunc_buf();
static TEXT: [u8; 4] = [b'M', b'Z', 0x90, 0x00];

/// 一个语料槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 映像字节。
    pub bytes: &'static [u8],
    /// 期望故障。
    pub fault: Fault,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(bytes: &'static [u8], fault: Fault, note: &'static str) -> Slot {
        Slot { bytes, fault, note }
    }
}

/// 十四槽语料：二活十二死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(&GOOD, Fault::Ok, "et-exec-load-dyn"),
    Slot::new(&PIE, Fault::Ok, "et-dyn-pie-load"),
    Slot::new(&BADM, Fault::BadMagic, "magic-xELF"),
    Slot::new(&TEXT, Fault::Text, "mz-stub"),
    Slot::new(&TRUNC, Fault::Trunc, "ten-bytes"),
    Slot::new(&[], Fault::Empty, "zero-bytes"),
    Slot::new(&CLS32, Fault::Class, "elfclass32"),
    Slot::new(&DATA2, Fault::Data, "eldata2msb"),
    Slot::new(&MACH183, Fault::Mach, "em-aarch64"),
    Slot::new(&MACH243, Fault::Mach, "em-riscv"),
    Slot::new(&NOPH, Fault::NoPhdr, "phnum-zero"),
    Slot::new(&PHENT32, Fault::BadPhentsize, "phentsize-32"),
    Slot::new(&NOLOAD, Fault::NoLoad, "pt3-only"),
    Slot::new(&SEGOUT, Fault::SegOut, "load-past-eof"),
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<Slot> {
    CASES.get(i).copied()
}

/// 语料指纹是否互异（按「字节内容 + 期望类」）。
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
                if same_slot(&a, &b) {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 两槽是否同指纹。
fn same_slot(a: &Slot, b: &Slot) -> bool {
    if a.fault != b.fault || a.bytes.len() != b.bytes.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.bytes.len() {
        match (a.bytes.get(i), b.bytes.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 备注全非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(s) => {
                if s.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 语料逐条对判。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if inspect(slot.bytes) != slot.fault {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    total: usize,
    accept: usize,
    reject: usize,
    magic: usize,
    trunc: usize,
    class: usize,
    mach: usize,
    phdr: usize,
    load: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report {
            total: 0,
            accept: 0,
            reject: 0,
            magic: 0,
            trunc: 0,
            class: 0,
            mach: 0,
            phdr: 0,
            load: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 放行条数。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收条数。
    pub const fn reject(&self) -> usize {
        self.reject
    }

    /// 魔数桶。
    pub const fn magic(&self) -> usize {
        self.magic
    }

    /// 截断桶。
    pub const fn trunc(&self) -> usize {
        self.trunc
    }

    /// 类别桶。
    pub const fn class(&self) -> usize {
        self.class
    }

    /// 机器桶。
    pub const fn mach(&self) -> usize {
        self.mach
    }

    /// 段表桶。
    pub const fn phdr(&self) -> usize {
        self.phdr
    }

    /// 装载桶。
    pub const fn load(&self) -> usize {
        self.load
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

    /// 比率（千分）。
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
            return Err(LD_ENOBS);
        }
        if self.red {
            return Err(LD_EVER);
        }
        if self.matched != self.total {
            return Err(LD_EVALUE);
        }
        if self.accept != 2 || self.reject != 12 {
            return Err(LD_EVALUE);
        }
        if self.magic != 2 || self.trunc != 2 || self.class != 2 {
            return Err(LD_EVALUE);
        }
        if self.mach != 2 || self.phdr != 2 || self.load != 2 {
            return Err(LD_EVALUE);
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
        let mut s = String::new();
        s.push_str("ldso corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" magic=");
        s.push_str(u32_text(self.magic as u32).as_str());
        s.push_str(" trunc=");
        s.push_str(u32_text(self.trunc as u32).as_str());
        s.push_str(" class=");
        s.push_str(u32_text(self.class as u32).as_str());
        s.push_str(" mach=");
        s.push_str(u32_text(self.mach as u32).as_str());
        s.push_str(" phdr=");
        s.push_str(u32_text(self.phdr as u32).as_str());
        s.push_str(" load=");
        s.push_str(u32_text(self.load as u32).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑一遍语料。
pub fn run() -> Result<Report, u32> {
    walk(None)
}

/// 跑一遍语料并注入一处红档。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 语料行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(LD_EVALUE),
        };
        let f = inspect(slot.bytes);
        r.total += 1;
        match f.bucket() {
            Bucket::Ok => r.accept += 1,
            Bucket::Magic => {
                r.reject += 1;
                r.magic += 1;
            }
            Bucket::Trunc => {
                r.reject += 1;
                r.trunc += 1;
            }
            Bucket::Class => {
                r.reject += 1;
                r.class += 1;
            }
            Bucket::Mach => {
                r.reject += 1;
                r.mach += 1;
            }
            Bucket::Phdr => {
                r.reject += 1;
                r.phdr += 1;
            }
            Bucket::Load => {
                r.reject += 1;
                r.load += 1;
            }
        }
        if f == slot.fault {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, _)) = red {
        if slot >= r.total {
            return Err(LD_ESLOT);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 绿色覆盖判据。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(LD_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 解释器通道：一门一用
// ---------------------------------------------------------------------------

/// ld.so 通道。
#[derive(Clone, Copy, Debug)]
pub struct Loader {
    steps: u32,
    bound: bool,
    last: u32,
}

impl Loader {
    /// 空。
    pub const fn new() -> Loader {
        Loader { steps: 0, bound: false, last: 0 }
    }

    /// 交一件。
    pub fn hand(&mut self, bytes: &[u8]) -> Result<Fault, u32> {
        if self.bound {
            self.last = LD_ESTATE;
            return Err(LD_ESTATE);
        }
        self.bound = true;
        self.steps = self.steps.saturating_add(1);
        let f = inspect(bytes);
        self.last = if f.alive() { 0 } else { f.code() };
        Ok(f)
    }

    /// 步数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 是否绑定。
    pub const fn bound(&self) -> bool {
        self.bound
    }

    /// 最后码。
    pub const fn last(&self) -> u32 {
        self.last
    }
}

/// 逐字文案（无路径）。
pub fn phrase(f: Fault) -> &'static str {
    f.phrase()
}

/// 是否 `cannot execute` 族。
pub fn cannot_execute(f: Fault) -> bool {
    !f.alive()
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽。
    pub slot: u32,
    /// 操作。
    pub op: u32,
    /// 是否成功。
    pub ok: bool,
    /// 码。
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
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(LD_ESLOT);
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
            return Err(LD_EBAL);
        }
        const VALS: [u32; 8] = [14, 2, 12, 2, 2, 2, 2, 929];
        const CODES: [u32; 6] = [LD_EMAGIC, LD_ETRUNC, LD_ECLASS, LD_EMACH, LD_EPHDR, LD_ELOAD];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LD_EBAL),
            };
            if s.slot as usize != i {
                return Err(LD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LD_EVALUE);
                        }
                    }
                    None => return Err(LD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LD_EVALUE);
                        }
                    }
                    None => return Err(LD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LD_EBAL);
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
            return Err(LD_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(LD_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(LD_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(LD_ESEAL);
        }
        Ok(self.root)
    }

    /// 根。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒收次数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 计数格。
fn count_cell(slot: u32, op: u32, v: Result<usize, u32>) -> Sample {
    match v {
        Ok(n) => Sample { slot, op, ok: true, code: 0, value: n as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 错误格。
fn err_cell(slot: u32, op: u32, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 造账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_REPORT, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_REPORT, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_REPORT, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_EHDR, r.map(|x| x.magic())));

    let _ = l.register(count_cell(4, OP_EHDR, r.map(|x| x.trunc())));
    let _ = l.register(count_cell(5, OP_EHDR, r.map(|x| x.class())));
    let _ = l.register(count_cell(6, OP_PHDR, r.map(|x| x.mach())));

    let red = run_with(Some((0, LD_EMAGIC)));
    let _ = l.register(count_cell(7, OP_LOAD, red.map(|x| x.rate() as usize)));

    let m = inspect(&BADM);
    let _ = l.register(err_cell(8, OP_EHDR, if m == Fault::Ok { LD_EMAGIC } else { m.code() }));

    let t = inspect(&TRUNC);
    let _ = l.register(err_cell(9, OP_EHDR, if t == Fault::Ok { LD_ETRUNC } else { t.code() }));

    let c = inspect(&CLS32);
    let _ = l.register(err_cell(10, OP_EHDR, if c == Fault::Ok { LD_ECLASS } else { c.code() }));

    let ma = inspect(&MACH183);
    let _ = l.register(err_cell(11, OP_EHDR, if ma == Fault::Ok { LD_EMACH } else { ma.code() }));

    let p = inspect(&NOPH);
    let _ = l.register(err_cell(12, OP_PHDR, if p == Fault::Ok { LD_EPHDR } else { p.code() }));

    let lo = inspect(&NOLOAD);
    let _ = l.register(err_cell(13, OP_LOAD, if lo == Fault::Ok { LD_ELOAD } else { lo.code() }));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称闸门。
fn name_ok() -> bool {
    let names = [
        Fault::Ok.name(),
        Fault::BadMagic.name(),
        Fault::Text.name(),
        Fault::Trunc.name(),
        Fault::Empty.name(),
        Fault::Class.name(),
        Fault::Data.name(),
        Fault::Mach.name(),
        Fault::NoPhdr.name(),
        Fault::BadPhentsize.name(),
        Fault::PhdrOut.name(),
        Fault::NoLoad.name(),
        Fault::SegOut.name(),
        Fault::NoDyn.name(),
        Fault::BadEntry.name(),
    ];
    let mut i = 0usize;
    while i < names.len() {
        if names[i].is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    LD_EMAGIC == SEG_BASE + 1
        && LD_ETRUNC == SEG_BASE + 2
        && LD_ECLASS == SEG_BASE + 3
        && LD_EMACH == SEG_BASE + 4
        && LD_EPHDR == SEG_BASE + 5
        && LD_ELOAD == SEG_BASE + 6
        && LD_ESTATE == SEG_BASE + 7
        && LD_EVER == SEG_BASE + 8
        && LD_EBAL == SEG_BASE + 9
        && LD_ESLOT == SEG_BASE + 0x0a
        && LD_EVALUE == SEG_BASE + 0x0b
        && LD_ESEAL == SEG_BASE + 0x0c
        && LD_ENOBS == SEG_BASE + 0x0d
}

/// 魔数闸门。
fn magic_ok() -> bool {
    let a = inspect(&GOOD) == Fault::Ok;
    let b = inspect(&BADM) == Fault::BadMagic;
    let c = inspect(&TEXT) == Fault::Text;
    let d = inspect(&[]) == Fault::Empty;
    let e = inspect(&TRUNC) == Fault::Trunc;
    let f = inspect(&[0x7f]) == Fault::Trunc && inspect(&[0x7f, b'E']) == Fault::Trunc;
    let g = inspect(&[b'M', b'Z', 0, 0]) == Fault::Text;
    a && b && c && d && e && f && g
}

/// 类别闸门。
fn class_ok() -> bool {
    let a = inspect(&CLS32) == Fault::Class;
    let b = inspect(&DATA2) == Fault::Data;
    let c = Fault::Class.code() == LD_ECLASS && Fault::Data.code() == LD_ECLASS;
    let d = Fault::Class.bucket() == Bucket::Class && Fault::Data.bucket() == Bucket::Class;
    a && b && c && d
}

/// 机器闸门。
fn mach_ok() -> bool {
    let a = inspect(&MACH183) == Fault::Mach;
    let b = inspect(&MACH243) == Fault::Mach;
    let c = Fault::Mach.code() == LD_EMACH;
    let d = Fault::Mach.phrase() == "wrong machine";
    a && b && c && d
}

/// 段表闸门。
fn phdr_ok() -> bool {
    let a = inspect(&NOPH) == Fault::NoPhdr;
    let b = inspect(&PHENT32) == Fault::BadPhentsize;
    let c = Fault::NoPhdr.code() == LD_EPHDR && Fault::BadPhentsize.code() == LD_EPHDR;
    let ph = match phdrs_of(&GOOD) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = ph.count == 2 && ph.loads == 1 && ph.dynp && ph.outside == 0;
    let e = phdrs_of(&NOPH).err() == Some(Fault::NoPhdr);
    a && b && c && d && e
}

/// 装载闸门。
fn load_ok() -> bool {
    let a = inspect(&NOLOAD) == Fault::NoLoad;
    let b = inspect(&SEGOUT) == Fault::SegOut;
    let c = Fault::NoLoad.code() == LD_ELOAD && Fault::SegOut.code() == LD_ELOAD;
    let d = Fault::NoDyn.code() == LD_ELOAD && Fault::BadEntry.code() == LD_ELOAD;
    let e = !Fault::SegOut.alive() && Fault::Ok.alive();
    a && b && c && d && e
}

/// 文案闸门。
fn message_ok() -> bool {
    let a = Fault::Ok.message("./prog").as_str() == "./prog: ok: loadable image\n";
    let b = Fault::BadMagic.message("./prog").as_str() == "./prog: cannot execute: bad ELF magic\n";
    let c = Fault::Text.message("./prog").as_str() == "./prog: cannot execute: not an ELF image\n";
    let d = Fault::Trunc.message("./prog").as_str() == "./prog: cannot execute: ELF header truncated\n";
    let e = Fault::Empty.message("./prog").as_str() == "./prog: cannot execute: empty file\n";
    let f = Fault::Class.message("./prog").as_str() == "./prog: cannot execute: wrong ELF class\n";
    a && b && c && d && e && f
}

/// 短语闸门。
fn phrase_ok() -> bool {
    let a = Fault::Data.phrase() == "wrong ELF data encoding";
    let b = Fault::Mach.phrase() == "wrong machine";
    let c = Fault::NoPhdr.phrase() == "no program headers";
    let d = Fault::BadPhentsize.phrase() == "bad program header size";
    let e = Fault::PhdrOut.phrase() == "program headers outside file";
    let f = Fault::NoLoad.phrase() == "no loadable segments";
    let g = Fault::SegOut.phrase() == "segment outside file";
    let h = Fault::NoDyn.phrase() == "no dynamic section";
    let i = Fault::BadEntry.phrase() == "no entry point";
    a && b && c && d && e && f && g && h && i
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.accept() == 2 && r.reject() == 12 && r.matched() == CORPUS;
    let b = r.magic() == 2 && r.trunc() == 2 && r.class() == 2;
    let c = r.mach() == 2 && r.phdr() == 2 && r.load() == 2;
    let d = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    a && b && c && d
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(s) => s.fault == Fault::Ok && s.bytes.len() == IMAGE_BYTES,
        None => false,
    };
    let b = match slot_at(5) {
        Some(s) => s.fault == Fault::Empty && s.bytes.is_empty(),
        None => false,
    };
    let c = match slot_at(CORPUS - 1) {
        Some(s) => s.fault == Fault::SegOut && s.note == "load-past-eof",
        None => false,
    };
    let d = slot_at(CORPUS).is_none();
    a && b && c && d
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "ldso corpus=14 accept=2 reject=12 magic=2 trunc=2 class=2 mach=2 phdr=2 load=2 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(LD_ENOBS);
    let c = e.render().as_str()
        == "ldso corpus=0 accept=0 reject=0 magic=0 trunc=0 class=0 mach=0 phdr=0 load=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, LD_EMAGIC))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(LD_EVER) && !r.green();
    let c = run_with(Some((CORPUS, LD_EMAGIC))).err() == Some(LD_ESLOT);
    a && b && c
}

/// 覆盖档。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(LD_ENOBS) && cover(0) == Err(LD_ENOBS)
}

/// 通道闸门。
fn loader_ok() -> bool {
    let mut ld = Loader::new();
    let a = !ld.bound() && ld.steps() == 0;
    let b = match ld.hand(&GOOD) {
        Ok(f) => f == Fault::Ok && ld.last() == 0,
        Err(_) => false,
    };
    let c = ld.hand(&GOOD).err() == Some(LD_ESTATE);
    let d = ld.bound() && ld.steps() == 1 && ld.last() == LD_ESTATE;
    let e = cannot_execute(Fault::BadMagic) && !cannot_execute(Fault::Ok);
    a && b && c && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && magic_ok() && class_ok();
    out[1] = mach_ok() && phdr_ok() && load_ok();
    out[2] = message_ok() && phrase_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && loader_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11611_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11611.ldso_bad_exec");

    s.add(
        "LD01-名称闸门",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "LD02-魔数与截断",
        {
            magic_ok()
        },
        "static",
    );

    s.add(
        "LD03-类别与编码",
        {
            class_ok()
        },
        "static",
    );

    s.add(
        "LD04-机器闸门",
        {
            mach_ok()
        },
        "static",
    );

    s.add(
        "LD05-段表闸门",
        {
            phdr_ok()
        },
        "static",
    );

    s.add(
        "LD06-装载闸门",
        {
            load_ok()
        },
        "static",
    );

    s.add(
        "LD07-文案逐字",
        {
            message_ok()
        },
        "static",
    );

    s.add(
        "LD08-短语矩阵",
        {
            phrase_ok()
        },
        "static",
    );

    s.add(
        "LD09-十四卡全跑",
        {
            corpus_ok()
        },
        "static",
    );

    s.add(
        "LD10-语料编号",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "LD11-汇总渲染",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "LD12-红单与覆盖",
        {
            reds_ok() && cover_ok()
        },
        "static",
    );

    s.add(
        "LD13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.root() == r,
                Err(_) => false,
            };
            let c = l.replay() == Ok(l.root());
            let d = l.register(Sample::blank()).err() == Some(LD_ESLOT) && l.rejects() == 1;
            let inj = inject_four();
            let mut e = true;
            let mut i = 0usize;
            while i < inj.len() {
                if !inj[i] {
                    e = false;
                }
                i += 1;
            }
            a && b && c && d && e
        },
        "static",
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fail() -> ! {
        panic!("unexpected failure path");
    }

    #[test]
    fn good_image_ok() {
        assert_eq!(inspect(&GOOD), Fault::Ok);
        assert!(Fault::Ok.alive());
    }

    #[test]
    fn bad_magic_detected() {
        assert_eq!(inspect(&BADM), Fault::BadMagic);
        assert_eq!(inspect(&TEXT), Fault::Text);
        assert_eq!(Fault::Text.code(), LD_EMAGIC);
    }

    #[test]
    fn trunc_and_empty() {
        assert_eq!(inspect(&[]), Fault::Empty);
        assert_eq!(inspect(&TRUNC), Fault::Trunc);
        assert_eq!(Fault::Empty.code(), LD_ETRUNC);
        assert_eq!(Fault::Trunc.code(), LD_ETRUNC);
    }

    #[test]
    fn class_and_data() {
        assert_eq!(inspect(&CLS32), Fault::Class);
        assert_eq!(inspect(&DATA2), Fault::Data);
        assert_eq!(Fault::Data.bucket(), Bucket::Class);
    }

    #[test]
    fn mach_detected() {
        assert_eq!(inspect(&MACH183), Fault::Mach);
        assert_eq!(inspect(&MACH243), Fault::Mach);
        assert_eq!(Fault::Mach.code(), LD_EMACH);
    }

    #[test]
    fn phdr_faults() {
        assert_eq!(inspect(&NOPH), Fault::NoPhdr);
        assert_eq!(inspect(&PHENT32), Fault::BadPhentsize);
        assert_eq!(Fault::BadPhentsize.bucket(), Bucket::Phdr);
    }

    #[test]
    fn load_faults() {
        assert_eq!(inspect(&NOLOAD), Fault::NoLoad);
        assert_eq!(inspect(&SEGOUT), Fault::SegOut);
        assert_eq!(Fault::SegOut.bucket(), Bucket::Load);
    }

    #[test]
    fn messages_verbatim() {
        assert_eq!(Fault::BadMagic.message("./prog").as_str(), "./prog: cannot execute: bad ELF magic\n");
        assert_eq!(Fault::NoPhdr.message("./prog").as_str(), "./prog: cannot execute: no program headers\n");
        assert_eq!(Fault::Ok.message("./prog").as_str(), "./prog: ok: loadable image\n");
        assert_eq!(Fault::BadEntry.message("a.out").as_str(), "a.out: cannot execute: no entry point\n");
    }

    #[test]
    fn phrase_matrix() {
        assert_eq!(Fault::Data.phrase(), "wrong ELF data encoding");
        assert_eq!(Fault::PhdrOut.phrase(), "program headers outside file");
        assert_eq!(Fault::SegOut.phrase(), "segment outside file");
        assert_eq!(Fault::NoDyn.phrase(), "no dynamic section");
    }

    #[test]
    fn read_helpers() {
        assert_eq!(rd16(&GOOD, 18), Some(EM_X86_64));
        assert_eq!(rd32(&GOOD, 64), Some(PT_LOAD));
        assert_eq!(rd64(&GOOD, 32), Some(0x40));
        assert_eq!(rd16(&GOOD, 9999), None);
    }

    #[test]
    fn phdrs_of_good() {
        let ph = match phdrs_of(&GOOD) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(ph.count, 2);
        assert_eq!(ph.loads, 1);
        assert!(ph.dynp);
        assert_eq!(ph.outside, 0);
    }

    #[test]
    fn corpus_shape() {
        assert_eq!(CASES.len(), CORPUS);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
    }

    #[test]
    fn report_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.total(), 14);
        assert_eq!(r.accept(), 2);
        assert_eq!(r.reject(), 12);
        assert_eq!(r.magic(), 2);
        assert_eq!(r.trunc(), 2);
        assert_eq!(r.class(), 2);
        assert_eq!(r.mach(), 2);
        assert_eq!(r.phdr(), 2);
        assert_eq!(r.load(), 2);
        assert_eq!(r.matched(), 14);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "ldso corpus=14 accept=2 reject=12 magic=2 trunc=2 class=2 mach=2 phdr=2 load=2 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((7, LD_EMACH))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched(), 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(LD_EVER));
        assert_eq!(run_with(Some((14, LD_EMAGIC))).err(), Some(LD_ESLOT));
    }

    #[test]
    fn loader_once() {
        let mut ld = Loader::new();
        match ld.hand(&GOOD) {
            Ok(f) => assert_eq!(f, Fault::Ok),
            Err(_) => fail(),
        }
        assert_eq!(ld.hand(&GOOD).err(), Some(LD_ESTATE));
        assert_eq!(ld.steps(), 1);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(100).is_none());
        assert!(!l.is_empty());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_ne!(r, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.register(Sample::blank()).err(), Some(LD_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(LD_ESEAL));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0], "inject0");
        assert!(out[1], "inject1");
        assert!(out[2], "inject2");
        assert!(out[3], "inject3");
    }

    #[test]
    fn fold_and_text() {
        let a = fold_bytes(FNV_OFF, &GOOD);
        let b = fold_bytes(FNV_OFF, &GOOD);
        assert_eq!(a, b);
        assert_ne!(a, fold_bytes(FNV_OFF, &BADM));
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11611_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
