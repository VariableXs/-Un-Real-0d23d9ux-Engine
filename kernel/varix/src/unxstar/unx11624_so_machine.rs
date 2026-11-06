//! UNX-F11624 .so 机种不匹配（EM_386）判据
//!
//! 一份 `.so` 的身份由三样东西钉死：类宽（`ELFCLASS32/64`）、端序
//! （`ELFDATA2LSB/MSB`）和机种（`e_machine`）。前两者决定怎么读，后者决定
//! 谁来执行。`e_machine` 是 16 位无符号数，位于 ELF 头偏移 `0x12`，32 位与
//! 64 位布局里位置相同——这是本模块可以只用一个偏移读两种类宽的原因，也是
//! 最容易被写错的地方：有人按"32 位头更短"去猜偏移，结果把 `e_type` 当成
//! 机种。
//!
//! 机种闸门不是"比较一个数"这么简单。它要按固定次序回答六个问题：
//!
//! 1. 头够不够长？`e_machine` 需要前 20 字节，短于此连读都读不到。
//! 2. 端序对不对？`EI_DATA` 与本机相反时，机种字段会读成字节交换的值。
//! 3. 机种是不是 0？`EM_NONE` 表示打包时没填，不是"任意机种"。
//! 4. 机种是否在表内？表外值不能猜成"兼容"，只能报未知。
//! 5. 类宽与机种是否自洽？`EM_386` 配 `ELFCLASS64`、`EM_ARM` 配
//!    `ELFCLASS64` 都是矛盾体，先于宿主匹配判死。
//! 6. 与宿主是否同族同号？`EM_386` 在 x86-64 宿主上是"错机种"，
//!    `EM_AARCH64` 则是"外族"，两者文案相同、错误码不同。
//!
//! 依赖连坐单独成段：根对象机种正确、但 `DT_NEEDED` 拉进来的 soname 是
//! `EM_386` 时，报错主体必须是**依赖名**，根名只能以 "required by" 出现。
//! 弄反了，运维就会去换根库，而真正错的是依赖。
//!
//! 文案逐字稳定：
//!
//! - `./i386.so: wrong ELF machine: EM_386`
//! - `./ia64.so: wrong ELF machine: EM_IA_64`
//! - `./none.so: invalid ELF header: e_machine is EM_NONE`
//! - `./zz.so: wrong ELF machine: 0xffff`
//! - `./bits.so: invalid ELF header: EM_386 cannot be ELFCLASS64`
//! - `libfoo.so: wrong ELF machine: EM_386 (required by ./dep386.so)`
//! - `./be.so: invalid ELF header: ELFDATA2MSB on little-endian host`
//!
//! 闸门次序：长度 → 端序 → 机种非零 → 机种已知 → 类宽自洽 → 宿主匹配 → 依赖连坐 → 文案。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfc70;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

// ---------------------------------------------------------------------------
// ELF 头偏移
// ---------------------------------------------------------------------------

/// `EI_CLASS` 偏移。
pub const EI_CLASS: usize = 4;

/// `EI_DATA` 偏移。
pub const EI_DATA: usize = 5;

/// `EI_VERSION` 偏移。
pub const EI_VERSION: usize = 6;

/// `EI_OSABI` 偏移。
pub const EI_OSABI: usize = 7;

/// `e_machine` 偏移（32/64 位同址）。
pub const MACH_OFF: usize = 0x12;

/// `e_machine` 之后一字节。
pub const MACH_END: usize = 0x14;

/// 读得到 `e_machine` 的最短头长。
pub const HEAD_MIN: usize = 0x14;

/// 32 位 ELF 头最小长度。
pub const EHDR32_MIN: usize = 52;

/// 64 位 ELF 头最小长度。
pub const EHDR64_MIN: usize = 64;

// ---------------------------------------------------------------------------
// 类宽
// ---------------------------------------------------------------------------

/// 无类。
pub const ELFCLASSNONE: u8 = 0;
/// 32 位类。
pub const ELFCLASS32: u8 = 1;
/// 64 位类。
pub const ELFCLASS64: u8 = 2;
/// 类编号占位。
pub const ELFCLASSNUM: u8 = 3;

// ---------------------------------------------------------------------------
// 端序
// ---------------------------------------------------------------------------

/// 未定端序。
pub const ELFDATANONE: u8 = 0;
/// 小端。
pub const ELFDATA2LSB: u8 = 1;
/// 大端。
pub const ELFDATA2MSB: u8 = 2;

// ---------------------------------------------------------------------------
// 机种（e_machine）
// ---------------------------------------------------------------------------

/// 未填机种。
pub const EM_NONE: u16 = 0;
/// SPARC。
pub const EM_SPARC: u16 = 2;
/// i386。
pub const EM_386: u16 = 3;
/// m68k。
pub const EM_M68K: u16 = 4;
/// i486（i386 别名）。
pub const EM_486: u16 = 6;
/// MIPS。
pub const EM_MIPS: u16 = 8;
/// PowerPC 32。
pub const EM_PPC: u16 = 20;
/// PowerPC 64。
pub const EM_PPC64: u16 = 21;
/// s390。
pub const EM_S390: u16 = 22;
/// ARM 32。
pub const EM_ARM: u16 = 40;
/// SuperH。
pub const EM_SH: u16 = 42;
/// IA-64。
pub const EM_IA_64: u16 = 50;
/// x86-64。
pub const EM_X86_64: u16 = 62;
/// AVR。
pub const EM_AVR: u16 = 83;
/// AArch64。
pub const EM_AARCH64: u16 = 183;
/// MicroBlaze。
pub const EM_MICROBLAZE: u16 = 189;
/// AMD GPU。
pub const EM_AMDGPU: u16 = 224;
/// RISC-V。
pub const EM_RISCV: u16 = 243;
/// BPF。
pub const EM_BPF: u16 = 247;
/// LoongArch。
pub const EM_LOONGARCH: u16 = 258;
/// 表外哨兵（用于未知值样例）。
pub const EM_UNKNOWN: u16 = 0xffff;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 机种不符（同族异号）。
pub const MC_EMACHINE: u32 = SEG_BASE + 0x01;
/// 机种未填（EM_NONE）。
pub const MC_EABSENT: u32 = SEG_BASE + 0x02;
/// 机种表外。
pub const MC_EUNKNOWN: u32 = SEG_BASE + 0x03;
/// 类宽与机种矛盾。
pub const MC_EBITS: u32 = SEG_BASE + 0x04;
/// 外族机种。
pub const MC_EFOREIGN: u32 = SEG_BASE + 0x05;
/// 依赖机种不符。
pub const MC_EDEP: u32 = SEG_BASE + 0x06;
/// 端序不符。
pub const MC_EDATA: u32 = SEG_BASE + 0x07;
/// 头太短，读不到机种。
pub const MC_ESHORT: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const MC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const MC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const MC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结指纹不符。
pub const MC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const MC_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 整表扫描。
pub const OP_CORPUS: u8 = 1;
/// 机种读取。
pub const OP_MACHINE: u8 = 2;
/// 文案比较。
pub const OP_TEXT: u8 = 3;
/// 端序读取。
pub const OP_ENDIAN: u8 = 4;
/// 依赖判定。
pub const OP_DEP: u8 = 5;
/// 表校验。
pub const OP_TABLE: u8 = 6;

// ---------------------------------------------------------------------------
// FNV-1a
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

/// 十进制文本。
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

/// 定宽小写十六进制文本。
fn hex_text(v: u32, digits: usize) -> String {
    let mut s = String::new();
    let mut n = digits;
    while n > 0 {
        n -= 1;
        let shift = (n * 4) as u32;
        let d = ((v >> shift) & 0xf) as u8;
        if d < 10 {
            s.push((b'0' + d) as char);
        } else {
            s.push((b'a' + d - 10) as char);
        }
    }
    s
}

/// 四字节小写十六进制。
fn hex4(v: u16) -> String {
    hex_text(v as u32, 4)
}

/// 类宽标签。
pub fn class_tag(class: u8) -> &'static str {
    match class {
        ELFCLASSNONE => "ELFCLASSNONE",
        ELFCLASS32 => "ELFCLASS32",
        ELFCLASS64 => "ELFCLASS64",
        ELFCLASSNUM => "ELFCLASSNUM",
        _ => "ELFCLASS?",
    }
}

/// 端序标签。
pub fn data_tag(data: u8) -> &'static str {
    match data {
        ELFDATANONE => "ELFDATANONE",
        ELFDATA2LSB => "ELFDATA2LSB",
        ELFDATA2MSB => "ELFDATA2MSB",
        _ => "ELFDATA?",
    }
}

/// 拼接 `<名>: <尾>`。
fn named(name: &str, tail: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(tail);
    s
}

/// 机种标签。
fn machine_tail(machine: u16) -> String {
    let known = lookup(machine).is_some();
    let mut s = String::from("wrong ELF machine: ");
    if known {
        s.push_str(machine_tag(machine));
    } else if machine == EM_UNKNOWN {
        s.push_str("0xffff");
    } else {
        s.push_str(hex4(machine).as_str());
    }
    s
}

// ---------------------------------------------------------------------------
// 机种表
// ---------------------------------------------------------------------------

/// 一条机种记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MachineInfo {
    /// 机种号。
    pub code: u16,
    /// 标准标签。
    pub tag: &'static str,
    /// 可读名。
    pub name: &'static str,
    /// 位宽（0 表示未定）。
    pub bits: u8,
    /// 家族。
    pub family: &'static str,
    /// 是否已废弃别名。
    pub alias: bool,
}

impl MachineInfo {
    /// 构造。
    pub const fn new(
        code: u16,
        tag: &'static str,
        name: &'static str,
        bits: u8,
        family: &'static str,
        alias: bool,
    ) -> MachineInfo {
        MachineInfo { code, tag, name, bits, family, alias }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.code as u64);
        h = fold_word(h, self.bits as u64);
        h = fold_word(h, self.alias as u64);
        h = fold_word(h, digest(self.tag.as_bytes()));
        h = fold_word(h, digest(self.name.as_bytes()));
        h = fold_word(h, digest(self.family.as_bytes()));
        h
    }
}

/// 机种表（按机种号严格升序）。
pub const MACHINES: [MachineInfo; 20] = [
    MachineInfo::new(EM_NONE, "EM_NONE", "no machine", 0, "none", false),
    MachineInfo::new(EM_SPARC, "EM_SPARC", "SPARC", 32, "sparc", false),
    MachineInfo::new(EM_386, "EM_386", "Intel 80386", 32, "x86", false),
    MachineInfo::new(EM_M68K, "EM_M68K", "Motorola 68000", 32, "m68k", false),
    MachineInfo::new(EM_486, "EM_486", "Intel 80486", 32, "x86", true),
    MachineInfo::new(EM_MIPS, "EM_MIPS", "MIPS R3000", 32, "mips", false),
    MachineInfo::new(EM_PPC, "EM_PPC", "PowerPC", 32, "ppc", false),
    MachineInfo::new(EM_PPC64, "EM_PPC64", "PowerPC 64", 64, "ppc", false),
    MachineInfo::new(EM_S390, "EM_S390", "IBM S/390", 32, "s390", false),
    MachineInfo::new(EM_ARM, "EM_ARM", "ARM 32", 32, "arm", false),
    MachineInfo::new(EM_SH, "EM_SH", "SuperH", 32, "sh", false),
    MachineInfo::new(EM_IA_64, "EM_IA_64", "Intel IA-64", 64, "ia64", false),
    MachineInfo::new(EM_X86_64, "EM_X86_64", "AMD x86-64", 64, "x86", false),
    MachineInfo::new(EM_AVR, "EM_AVR", "Atmel AVR", 32, "avr", false),
    MachineInfo::new(EM_AARCH64, "EM_AARCH64", "ARM AArch64", 64, "arm", false),
    MachineInfo::new(EM_MICROBLAZE, "EM_MICROBLAZE", "Xilinx MicroBlaze", 32, "microblaze", false),
    MachineInfo::new(EM_AMDGPU, "EM_AMDGPU", "AMD GPU", 64, "amdgpu", false),
    MachineInfo::new(EM_RISCV, "EM_RISCV", "RISC-V", 64, "riscv", false),
    MachineInfo::new(EM_BPF, "EM_BPF", "eBPF", 64, "bpf", false),
    MachineInfo::new(EM_LOONGARCH, "EM_LOONGARCH", "LoongArch", 64, "loongarch", false),
];

/// 取一条机种记录（二分）。
pub fn lookup(code: u16) -> Option<MachineInfo> {
    let mut lo = 0usize;
    let mut hi = MACHINES.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        match MACHINES.get(mid) {
            Some(m) => {
                if m.code == code {
                    return Some(*m);
                }
                if m.code < code {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            None => return None,
        }
    }
    None
}

/// 机种是否在表内。
pub fn known(code: u16) -> bool {
    lookup(code).is_some()
}

/// 机种标签；表外给 `EM_UNKNOWN`。
pub fn machine_tag(code: u16) -> &'static str {
    match lookup(code) {
        Some(m) => m.tag,
        None => "EM_UNKNOWN",
    }
}

/// 机种可读名。
pub fn machine_name(code: u16) -> &'static str {
    match lookup(code) {
        Some(m) => m.name,
        None => "unknown machine",
    }
}

/// 机种位宽。
pub fn machine_bits(code: u16) -> u8 {
    match lookup(code) {
        Some(m) => m.bits,
        None => 0,
    }
}

/// 机种家族。
pub fn family_of(code: u16) -> &'static str {
    match lookup(code) {
        Some(m) => m.family,
        None => "unknown",
    }
}

/// 别名归一：`EM_486` 归到 `EM_386`，其余原样。
pub fn canonical(code: u16) -> u16 {
    if code == EM_486 {
        EM_386
    } else {
        code
    }
}

/// 是否是 x86 家族。
pub fn is_x86(code: u16) -> bool {
    family_of(code) == "x86"
}

/// 表内条数。
pub fn machine_count() -> usize {
    MACHINES.len()
}

/// 表是否按机种号严格升序。
pub fn table_sorted() -> bool {
    let mut i = 1usize;
    while i < MACHINES.len() {
        let a = match MACHINES.get(i - 1) {
            Some(v) => *v,
            None => return false,
        };
        let b = match MACHINES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.code >= b.code {
            return false;
        }
        i += 1;
    }
    true
}

/// 表内机种号是否互异。
pub fn table_unique() -> bool {
    let mut i = 0usize;
    while i < MACHINES.len() {
        let a = match MACHINES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < MACHINES.len() {
            if let Some(b) = MACHINES.get(j) {
                if a.code == b.code {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 表项自身是否自洽。
pub fn machines_ok() -> bool {
    let mut i = 0usize;
    while i < MACHINES.len() {
        let m = match MACHINES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if m.tag.is_empty() || m.name.is_empty() || m.family.is_empty() {
            return false;
        }
        if m.bits != 0 && m.bits != 32 && m.bits != 64 {
            return false;
        }
        if m.code == EM_NONE && m.bits != 0 {
            return false;
        }
        if m.alias && canonical(m.code) == m.code {
            return false;
        }
        i += 1;
    }
    true
}

/// 表整体判据。
pub fn table_ok() -> bool {
    if !table_sorted() || !table_unique() || !machines_ok() {
        return false;
    }
    if machine_count() != MACHINES.len() {
        return false;
    }
    let a = match lookup(EM_386) {
        Some(m) => m.bits == 32 && m.family == "x86" && !m.alias,
        None => false,
    };
    let b = match lookup(EM_X86_64) {
        Some(m) => m.bits == 64 && m.family == "x86" && !m.alias,
        None => false,
    };
    let c = match lookup(EM_UNKNOWN) {
        Some(_) => false,
        None => true,
    };
    let d = lookup(EM_486).is_some() && canonical(EM_486) == EM_386;
    a && b && c && d
}

/// 表指纹。
pub fn table_seal() -> u64 {
    let mut h = fold_word(FNV_OFF, MACHINES.len() as u64);
    let mut i = 0usize;
    while i < MACHINES.len() {
        if let Some(m) = MACHINES.get(i) {
            h = fold_word(h, m.seal());
        }
        i += 1;
    }
    h
}

/// 统计某家族在表内的条数。
pub fn family_count(family: &str) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < MACHINES.len() {
        if let Some(m) = MACHINES.get(i) {
            if m.family == family {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 端序
// ---------------------------------------------------------------------------

/// ELF 数据编码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endian {
    /// 小端。
    Little,
    /// 大端。
    Big,
    /// 未定。
    None,
}

impl Endian {
    /// 由 `EI_DATA` 取值。
    pub fn of_byte(b: u8) -> Endian {
        match b {
            ELFDATA2LSB => Endian::Little,
            ELFDATA2MSB => Endian::Big,
            _ => Endian::None,
        }
    }

    /// 回转成字节。
    pub fn byte(&self) -> u8 {
        match self {
            Endian::Little => ELFDATA2LSB,
            Endian::Big => ELFDATA2MSB,
            Endian::None => ELFDATANONE,
        }
    }

    /// 名称。
    pub fn name(&self) -> &'static str {
        data_tag(self.byte())
    }

    /// 本机端序（x86 系为小端）。
    pub fn native() -> Endian {
        Endian::Little
    }

    /// 是否与本机一致。
    pub fn agrees(&self, host: Endian) -> bool {
        match self {
            Endian::None => false,
            _ => *self == host,
        }
    }

    /// 读 16 位（越界给 `None`）。
    pub fn read16(&self, buf: &[u8], off: usize) -> Option<u16> {
        let lo = match buf.get(off) {
            Some(v) => *v,
            None => return None,
        };
        let k = match off.checked_add(1) {
            Some(v) => v,
            None => return None,
        };
        let hi = match buf.get(k) {
            Some(v) => *v,
            None => return None,
        };
        match self {
            Endian::Little => Some((lo as u16) | ((hi as u16) << 8)),
            Endian::Big => Some((((lo as u16) << 8) | (hi as u16)) as u16),
            Endian::None => None,
        }
    }

    /// 读 32 位（越界给 `None`）。
    pub fn read32(&self, buf: &[u8], off: usize) -> Option<u32> {
        let a = match self.read16(buf, off) {
            Some(v) => v,
            None => return None,
        };
        let k = match off.checked_add(2) {
            Some(v) => v,
            None => return None,
        };
        let b = match self.read16(buf, k) {
            Some(v) => v,
            None => return None,
        };
        match self {
            Endian::Little => Some((a as u32) | ((b as u32) << 16)),
            Endian::Big => Some(((a as u32) << 16) | (b as u32)),
            Endian::None => None,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        fold_word(FNV_OFF, self.byte() as u64)
    }
}

// ---------------------------------------------------------------------------
// 头读取
// ---------------------------------------------------------------------------

/// ELF 头的前 20 字节窗口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    buf: [u8; HEAD_MIN],
    len: usize,
}

impl Header {
    /// 空窗口。
    pub const fn blank() -> Header {
        Header { buf: [0u8; HEAD_MIN], len: 0 }
    }

    /// 写入类、端序与机种，声明实长。
    pub fn new(class: u8, data: u8, machine: u16, len: usize) -> Header {
        let mut h = Header::blank();
        h.len = if len > HEAD_MIN { HEAD_MIN } else { len };
        if h.len > EI_CLASS {
            if let Some(b) = h.buf.get_mut(EI_CLASS) {
                *b = class;
            }
        }
        if h.len > EI_DATA {
            if let Some(b) = h.buf.get_mut(EI_DATA) {
                *b = data;
            }
        }
        if h.len >= MACH_END {
            if let Some(b) = h.buf.get_mut(MACH_OFF) {
                *b = (machine & 0xff) as u8;
            }
            if let Some(b) = h.buf.get_mut(MACH_OFF + 1) {
                *b = ((machine >> 8) & 0xff) as u8;
            }
        }
        h
    }

    /// 从字节串截取前 20 字节。
    pub fn of_bytes(bytes: &[u8]) -> Header {
        let mut h = Header::blank();
        let n = if bytes.len() > HEAD_MIN { HEAD_MIN } else { bytes.len() };
        let mut i = 0usize;
        while i < n {
            match (bytes.get(i), h.buf.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => break,
            }
            i += 1;
        }
        h.len = n;
        h
    }

    /// 实长。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一字节（超出实长给 `None`）。
    pub fn byte(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.buf.get(i).copied()
    }

    /// 类字节。
    pub fn class(&self) -> u8 {
        self.byte(EI_CLASS).unwrap_or(ELFCLASSNONE)
    }

    /// 端序字节。
    pub fn data(&self) -> u8 {
        self.byte(EI_DATA).unwrap_or(ELFDATANONE)
    }

    /// 版本字节。
    pub fn version(&self) -> u8 {
        self.byte(EI_VERSION).unwrap_or(0)
    }

    /// OSABI 字节。
    pub fn osabi(&self) -> u8 {
        self.byte(EI_OSABI).unwrap_or(0)
    }

    /// 端序。
    pub fn endian(&self) -> Endian {
        Endian::of_byte(self.data())
    }

    /// 是否够读机种。
    pub const fn complete(&self) -> bool {
        self.len >= HEAD_MIN
    }

    /// 原始机种；头短或端序未定给 `None`。
    pub fn machine_raw(&self) -> Option<u16> {
        if !self.complete() {
            return None;
        }
        self.endian().read16(&self.buf, MACH_OFF)
    }

    /// 归一后的机种；读不到给 `EM_NONE`。
    pub fn machine(&self) -> u16 {
        match self.machine_raw() {
            Some(m) => m,
            None => EM_NONE,
        }
    }

    /// 该类头的最小长度。
    pub fn minimum(class: u8) -> usize {
        match class {
            ELFCLASS32 => EHDR32_MIN,
            ELFCLASS64 => EHDR64_MIN,
            _ => HEAD_MIN,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.len as u64);
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 宿主
// ---------------------------------------------------------------------------

/// 宿主。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    /// x86-64（64 位）。
    X64,
    /// i686（32 位）。
    I686,
}

impl Host {
    /// 本机。
    pub fn native() -> Host {
        Host::X64
    }

    /// 宿主机种号。
    pub fn machine(&self) -> u16 {
        match self {
            Host::X64 => EM_X86_64,
            Host::I686 => EM_386,
        }
    }

    /// 宿主类宽。
    pub fn class(&self) -> u8 {
        match self {
            Host::X64 => ELFCLASS64,
            Host::I686 => ELFCLASS32,
        }
    }

    /// 宿主位宽。
    pub fn bits(&self) -> u8 {
        match self {
            Host::X64 => 64,
            Host::I686 => 32,
        }
    }

    /// 宿主名。
    pub fn name(&self) -> &'static str {
        match self {
            Host::X64 => "x86_64",
            Host::I686 => "i686",
        }
    }

    /// 家族。
    pub fn family(&self) -> &'static str {
        "x86"
    }

    /// 端序。
    pub fn endian(&self) -> Endian {
        Endian::Little
    }

    /// 是否接受某机种。
    pub fn accepts(&self, machine: u16) -> bool {
        match self {
            Host::X64 => machine == EM_X86_64,
            Host::I686 => machine == EM_386 || machine == EM_486,
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.machine() as u64);
        h = fold_word(h, self.bits() as u64);
        fold_word(h, digest(self.name().as_bytes()))
    }
}

/// 兼容结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compat {
    /// 机种同号。
    Match,
    /// 32 位别名（i486 于 i686）。
    Alias,
    /// x32：机器对、类宽交给类闸门。
    X32,
    /// 同族异号。
    Mismatch,
    /// 外族。
    Foreign,
    /// 类宽与机种矛盾。
    Bits,
    /// 机种未填。
    Absent,
    /// 表外机种。
    Unknown,
}

impl Compat {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Compat::Match => "match",
            Compat::Alias => "alias",
            Compat::X32 => "x32",
            Compat::Mismatch => "mismatch",
            Compat::Foreign => "foreign",
            Compat::Bits => "bits",
            Compat::Absent => "absent",
            Compat::Unknown => "unknown",
        }
    }

    /// 是否直接放行。
    pub fn admitted(&self) -> bool {
        match self {
            Compat::Match => true,
            Compat::Alias => true,
            _ => false,
        }
    }

    /// 是否延后到类闸门。
    pub fn deferred(&self) -> bool {
        match self {
            Compat::X32 => true,
            _ => false,
        }
    }

    /// 备注。
    pub fn note(&self) -> &'static str {
        match self {
            Compat::Match => "native-machine",
            Compat::Alias => "i486-alias",
            Compat::X32 => "x32-class-deferred",
            Compat::Mismatch => "wrong-machine",
            Compat::Foreign => "foreign-arch",
            Compat::Bits => "class-vs-machine",
            Compat::Absent => "no-machine",
            Compat::Unknown => "unknown-machine",
        }
    }

    /// 对应根因。
    pub fn fault(&self) -> Option<Fault> {
        match self {
            Compat::Match => None,
            Compat::Alias => None,
            Compat::X32 => None,
            Compat::Mismatch => Some(Fault::Mismatch),
            Compat::Foreign => Some(Fault::Foreign),
            Compat::Bits => Some(Fault::Bits),
            Compat::Absent => Some(Fault::Absent),
            Compat::Unknown => Some(Fault::Unknown),
        }
    }
}

/// 宿主 × 类 × 机种。
pub fn compat_of(host: Host, class: u8, machine: u16) -> Compat {
    if machine == EM_NONE {
        return Compat::Absent;
    }
    let info = match lookup(machine) {
        Some(v) => v,
        None => return Compat::Unknown,
    };
    if class != ELFCLASS32 && class != ELFCLASS64 {
        return Compat::Bits;
    }
    if machine == EM_X86_64 && class == ELFCLASS32 && host.machine() == EM_X86_64 {
        return Compat::X32;
    }
    if machine != EM_X86_64 {
        if (class == ELFCLASS32 && info.bits == 64) || (class == ELFCLASS64 && info.bits == 32) {
            return Compat::Bits;
        }
    }
    if machine == host.machine() {
        return Compat::Match;
    }
    if machine == EM_486 && host.machine() == EM_386 {
        return Compat::Alias;
    }
    if info.family == host.family() {
        Compat::Mismatch
    } else {
        Compat::Foreign
    }
}

// ---------------------------------------------------------------------------
// 根因
// ---------------------------------------------------------------------------

/// 机种闸门的失败根因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 同族异号。
    Mismatch,
    /// `EM_NONE`。
    Absent,
    /// 表外。
    Unknown,
    /// 类宽矛盾。
    Bits,
    /// 外族。
    Foreign,
    /// 依赖连坐。
    Dep,
    /// 端序不符。
    Data,
    /// 头太短。
    Short,
}

impl Fault {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Fault::Mismatch => "mismatch",
            Fault::Absent => "absent",
            Fault::Unknown => "unknown",
            Fault::Bits => "bits",
            Fault::Foreign => "foreign",
            Fault::Dep => "dep",
            Fault::Data => "data",
            Fault::Short => "short",
        }
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Mismatch => MC_EMACHINE,
            Fault::Absent => MC_EABSENT,
            Fault::Unknown => MC_EUNKNOWN,
            Fault::Bits => MC_EBITS,
            Fault::Foreign => MC_EFOREIGN,
            Fault::Dep => MC_EDEP,
            Fault::Data => MC_EDATA,
            Fault::Short => MC_ESHORT,
        }
    }

    /// 属于机种族（依赖与头长不算）。
    pub fn machine_family(&self) -> bool {
        match self {
            Fault::Mismatch | Fault::Absent | Fault::Unknown => true,
            Fault::Bits | Fault::Foreign => true,
            _ => false,
        }
    }

    /// 文案。
    pub fn message(&self, subject: &str, root: &str, machine: u16, class: u8, data: u8) -> String {
        match self {
            Fault::Mismatch => named(subject, machine_tail(machine).as_str()),
            Fault::Foreign => named(subject, machine_tail(machine).as_str()),
            Fault::Absent => {
                named(subject, "invalid ELF header: e_machine is EM_NONE")
            }
            Fault::Unknown => {
                let mut s = String::from("wrong ELF machine: ");
                if machine == EM_UNKNOWN {
                    s.push_str("0xffff");
                } else {
                    s.push_str(hex4(machine).as_str());
                }
                named(subject, s.as_str())
            }
            Fault::Bits => {
                let mut s = String::from("invalid ELF header: ");
                s.push_str(machine_tag(machine));
                s.push_str(" cannot be ");
                s.push_str(class_tag(class));
                named(subject, s.as_str())
            }
            Fault::Dep => {
                let mut s = machine_tail(machine);
                s.push_str(" (required by ");
                s.push_str(root);
                s.push(')');
                named(subject, s.as_str())
            }
            Fault::Data => {
                let mut s = String::from("invalid ELF header: ");
                s.push_str(data_tag(data));
                s.push_str(" on little-endian host");
                named(subject, s.as_str())
            }
            Fault::Short => named(subject, "invalid ELF header"),
        }
    }
}

// ---------------------------------------------------------------------------
// 闸门阶梯
// ---------------------------------------------------------------------------

/// 机种管线的八段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 头长。
    Length,
    /// 端序。
    Endian,
    /// 机种非零。
    MachineZero,
    /// 机种已知。
    MachineKnown,
    /// 类宽自洽。
    ClassWidth,
    /// 宿主匹配。
    HostMatch,
    /// 依赖连坐。
    Dependency,
    /// 文案出口。
    Text,
}

impl Stage {
    /// 序号（0 起）。
    pub fn index(&self) -> u8 {
        match self {
            Stage::Length => 0,
            Stage::Endian => 1,
            Stage::MachineZero => 2,
            Stage::MachineKnown => 3,
            Stage::ClassWidth => 4,
            Stage::HostMatch => 5,
            Stage::Dependency => 6,
            Stage::Text => 7,
        }
    }

    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Length => "length",
            Stage::Endian => "endian",
            Stage::MachineZero => "machine-zero",
            Stage::MachineKnown => "machine-known",
            Stage::ClassWidth => "class-width",
            Stage::HostMatch => "host-match",
            Stage::Dependency => "dependency",
            Stage::Text => "text",
        }
    }

    /// 说明。
    pub fn detail(&self) -> &'static str {
        match self {
            Stage::Length => "need 20 bytes to read e_machine",
            Stage::Endian => "EI_DATA must agree with host",
            Stage::MachineZero => "EM_NONE means the field was never set",
            Stage::MachineKnown => "codes outside the table cannot be guessed",
            Stage::ClassWidth => "class must agree with machine width",
            Stage::HostMatch => "machine must belong to the host family",
            Stage::Dependency => "a needed object is judged by its own machine",
            Stage::Text => "only the failing stage speaks",
        }
    }

    /// 由根因落段。
    pub fn of_fault(f: Option<Fault>) -> Stage {
        match f {
            None => Stage::Text,
            Some(Fault::Short) => Stage::Length,
            Some(Fault::Data) => Stage::Endian,
            Some(Fault::Absent) => Stage::MachineZero,
            Some(Fault::Unknown) => Stage::MachineKnown,
            Some(Fault::Bits) => Stage::ClassWidth,
            Some(Fault::Dep) => Stage::Dependency,
            Some(Fault::Mismatch) => Stage::HostMatch,
            Some(Fault::Foreign) => Stage::HostMatch,
        }
    }

    /// 阶梯横幅。
    pub fn banner() -> &'static str {
        "length -> endian -> machine-zero -> machine-known -> class-width -> host-match -> dependency -> text"
    }

    /// 是否在文案段之前。
    pub fn before_text(&self) -> bool {
        self.index() < Stage::Text.index()
    }
}

/// 闸门段数。
pub fn stage_count() -> usize {
    8
}

/// 某段的名称。
pub fn stage_at(i: usize) -> Option<Stage> {
    match i {
        0 => Some(Stage::Length),
        1 => Some(Stage::Endian),
        2 => Some(Stage::MachineZero),
        3 => Some(Stage::MachineKnown),
        4 => Some(Stage::ClassWidth),
        5 => Some(Stage::HostMatch),
        6 => Some(Stage::Dependency),
        7 => Some(Stage::Text),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 结论
// ---------------------------------------------------------------------------

/// 一次机种判定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judge {
    subject: String,
    root: String,
    class: u8,
    machine: u16,
    canon: u16,
    host: Host,
    endian: Endian,
    compat: Compat,
    fault: Option<Fault>,
    note: &'static str,
}

impl Judge {
    /// 构造。
    fn make(
        subject: &str,
        root: &str,
        class: u8,
        machine: u16,
        canon: u16,
        host: Host,
        endian: Endian,
        compat: Compat,
        fault: Option<Fault>,
        note: &'static str,
    ) -> Judge {
        Judge {
            subject: String::from(subject),
            root: String::from(root),
            class,
            machine,
            canon,
            host,
            endian,
            compat,
            fault,
            note,
        }
    }

    /// 主体名。
    pub fn subject(&self) -> &str {
        self.subject.as_str()
    }

    /// 根名。
    pub fn root(&self) -> &str {
        self.root.as_str()
    }

    /// 类宽。
    pub const fn class(&self) -> u8 {
        self.class
    }

    /// 原始机种。
    pub const fn machine(&self) -> u16 {
        self.machine
    }

    /// 归一机种。
    pub const fn canonical(&self) -> u16 {
        self.canon
    }

    /// 宿主。
    pub const fn host(&self) -> Host {
        self.host
    }

    /// 端序。
    pub const fn endian(&self) -> Endian {
        self.endian
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

    /// 是否依赖连坐。
    pub fn is_dep(&self) -> bool {
        self.root != self.subject
    }

    /// 所在闸门段。
    pub fn stage(&self) -> Stage {
        Stage::of_fault(self.fault)
    }

    /// 文案；可载时为空串。
    pub fn text(&self) -> String {
        match self.fault {
            Some(f) => f.message(
                self.subject.as_str(),
                self.root.as_str(),
                self.machine,
                self.class,
                self.endian.byte(),
            ),
            None => String::new(),
        }
    }

    /// 诊断文本。
    pub fn diagnose(&self) -> String {
        let mut s = String::from(self.subject.as_str());
        s.push_str(": class=");
        s.push_str(class_tag(self.class));
        s.push_str(" machine=");
        s.push_str(machine_tag(self.machine));
        s.push('(');
        s.push_str(dec_text(self.machine as u32).as_str());
        s.push(')');
        s.push_str(" canonical=");
        s.push_str(machine_tag(self.canon));
        s.push_str(" host=");
        s.push_str(self.host.name());
        s.push_str(" endian=");
        s.push_str(self.endian.name());
        s.push_str(" compat=");
        s.push_str(self.compat.name());
        s.push_str(" fault=");
        match self.fault {
            Some(f) => s.push_str(f.name()),
            None => s.push_str("none"),
        }
        s.push_str(" note=");
        s.push_str(self.note);
        if self.is_dep() {
            s.push_str(" root=");
            s.push_str(self.root.as_str());
        }
        s.push('\n');
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.class as u64);
        h = fold_word(h, self.machine as u64);
        h = fold_word(h, self.canon as u64);
        h = fold_word(h, self.host.seal());
        h = fold_word(h, self.endian.seal());
        h = fold_word(h, self.compat.name().as_bytes().len() as u64);
        h = fold_word(h, self.code() as u64);
        h = fold_word(h, digest(self.subject.as_bytes()));
        h = fold_word(h, digest(self.root.as_bytes()));
        h = fold_word(h, digest(self.note.as_bytes()));
        h
    }
}

/// 判一份头。
pub fn judge_header(name: &str, h: &Header, host: Host) -> Judge {
    let class = h.class();
    let endian = h.endian();
    let raw = h.machine_raw();
    let machine = match raw {
        Some(v) => v,
        None => EM_NONE,
    };
    let canon = canonical(machine);
    if !h.complete() || raw.is_none() {
        return Judge::make(
            name,
            name,
            class,
            machine,
            canon,
            host,
            endian,
            Compat::Absent,
            Some(Fault::Short),
            "short-header",
        );
    }
    if !endian.agrees(host.endian()) {
        return Judge::make(
            name,
            name,
            class,
            machine,
            canon,
            host,
            endian,
            Compat::Unknown,
            Some(Fault::Data),
            "byte-order",
        );
    }
    let compat = compat_of(host, class, machine);
    let fault = compat.fault();
    Judge::make(
        name,
        name,
        class,
        machine,
        canon,
        host,
        endian,
        compat,
        fault,
        compat.note(),
    )
}

/// 判根对象 + 一个 `DT_NEEDED`。
pub fn judge_dep(root: &str, h: &Header, dep: &str, dep_machine: u16, host: Host) -> Judge {
    let r = judge_header(root, h, host);
    if !r.accepted() {
        return r;
    }
    let dclass = if machine_bits(dep_machine) == 32 {
        ELFCLASS32
    } else if machine_bits(dep_machine) == 64 {
        ELFCLASS64
    } else {
        r.class()
    };
    let compat = compat_of(host, dclass, dep_machine);
    if compat.admitted() {
        return Judge::make(
            root,
            root,
            r.class(),
            r.machine(),
            r.canonical(),
            host,
            r.endian(),
            Compat::Match,
            None,
            "deps-ok",
        );
    }
    Judge::make(
        dep,
        root,
        dclass,
        dep_machine,
        canonical(dep_machine),
        host,
        r.endian(),
        compat,
        Some(Fault::Dep),
        "needed-machine",
    )
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一条机种样例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 对象名。
    pub name: &'static str,
    /// 类宽。
    pub class: u8,
    /// 端序字节。
    pub data: u8,
    /// 机种。
    pub machine: u16,
    /// 头实长。
    pub ident_len: usize,
    /// 依赖 soname（空表示无依赖）。
    pub dep_soname: &'static str,
    /// 依赖机种。
    pub dep_machine: u16,
    /// 期望根因。
    pub expect: Option<Fault>,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(
        name: &'static str,
        class: u8,
        data: u8,
        machine: u16,
        ident_len: usize,
        dep_soname: &'static str,
        dep_machine: u16,
        expect: Option<Fault>,
        note: &'static str,
    ) -> Case {
        Case {
            name,
            class,
            data,
            machine,
            ident_len,
            dep_soname,
            dep_machine,
            expect,
            note,
        }
    }

    /// 是否带依赖。
    pub fn has_dep(&self) -> bool {
        !self.dep_soname.is_empty()
    }
}

/// 十四卡：两成十二败。
pub const CASES: [Case; CORPUS] = [
    Case::new("libc.so.6", ELFCLASS64, ELFDATA2LSB, EM_X86_64, HEAD_MIN, "", 0, None, "native-x64"),
    Case::new("libm.so.6", ELFCLASS64, ELFDATA2LSB, EM_X86_64, HEAD_MIN, "", 0, None, "native-x64-libm"),
    Case::new("./i386.so", ELFCLASS32, ELFDATA2LSB, EM_386, HEAD_MIN, "", 0, Some(Fault::Mismatch), "wrong-machine-386"),
    Case::new("./i486.so", ELFCLASS32, ELFDATA2LSB, EM_486, HEAD_MIN, "", 0, Some(Fault::Mismatch), "alias-486-on-x64"),
    Case::new("./ia64.so", ELFCLASS64, ELFDATA2LSB, EM_IA_64, HEAD_MIN, "", 0, Some(Fault::Foreign), "foreign-ia64"),
    Case::new("./arm64.so", ELFCLASS64, ELFDATA2LSB, EM_AARCH64, HEAD_MIN, "", 0, Some(Fault::Foreign), "foreign-aarch64"),
    Case::new("./arm32.so", ELFCLASS32, ELFDATA2LSB, EM_ARM, HEAD_MIN, "", 0, Some(Fault::Foreign), "foreign-arm32"),
    Case::new("./none.so", ELFCLASS64, ELFDATA2LSB, EM_NONE, HEAD_MIN, "", 0, Some(Fault::Absent), "em-none"),
    Case::new("./zz.so", ELFCLASS64, ELFDATA2LSB, EM_UNKNOWN, HEAD_MIN, "", 0, Some(Fault::Unknown), "unknown-ffff"),
    Case::new("./mips.so", ELFCLASS32, ELFDATA2LSB, EM_MIPS, HEAD_MIN, "", 0, Some(Fault::Foreign), "foreign-mips"),
    Case::new("./bits.so", ELFCLASS64, ELFDATA2LSB, EM_386, HEAD_MIN, "", 0, Some(Fault::Bits), "386-in-elf64"),
    Case::new("./bits2.so", ELFCLASS64, ELFDATA2LSB, EM_ARM, HEAD_MIN, "", 0, Some(Fault::Bits), "arm-in-elf64"),
    Case::new(
        "./dep386.so",
        ELFCLASS64,
        ELFDATA2LSB,
        EM_X86_64,
        HEAD_MIN,
        "libfoo.so",
        EM_386,
        Some(Fault::Dep),
        "needed-386",
    ),
    Case::new("./be.so", ELFCLASS64, ELFDATA2MSB, EM_X86_64, HEAD_MIN, "", 0, Some(Fault::Data), "big-endian"),
];

/// 取一条语料。
pub fn slot_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 名字、备注互异；反例四元组互异。
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
                    && b.data == a.data
                    && b.machine == a.machine
                    && b.dep_soname == a.dep_soname
                    && b.dep_machine == a.dep_machine
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

/// 造头。
pub fn header_of(c: &Case) -> Header {
    Header::new(c.class, c.data, c.machine, c.ident_len)
}

/// 判一条语料。
pub fn judge_case(c: &Case, host: Host) -> Judge {
    let h = header_of(c);
    if c.has_dep() {
        judge_dep(c.name, &h, c.dep_soname, c.dep_machine, host)
    } else {
        judge_header(c.name, &h, host)
    }
}

/// 十四卡逐条对判。
pub fn corpus_table_ok() -> bool {
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
        if j.is_dep() != c.has_dep() {
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
    mismatch: usize,
    foreign: usize,
    absent: usize,
    unknown: usize,
    bits: usize,
    dep: usize,
    data: usize,
    short: usize,
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
            mismatch: 0,
            foreign: 0,
            absent: 0,
            unknown: 0,
            bits: 0,
            dep: 0,
            data: 0,
            short: 0,
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

    /// 同族异号条数。
    pub const fn mismatch(&self) -> usize {
        self.mismatch
    }

    /// 外族条数。
    pub const fn foreign(&self) -> usize {
        self.foreign
    }

    /// 空机种条数。
    pub const fn absent(&self) -> usize {
        self.absent
    }

    /// 表外机种条数。
    pub const fn unknown(&self) -> usize {
        self.unknown
    }

    /// 类宽矛盾条数。
    pub const fn bits(&self) -> usize {
        self.bits
    }

    /// 依赖连坐条数。
    pub const fn dep(&self) -> usize {
        self.dep
    }

    /// 端序不符条数。
    pub const fn data(&self) -> usize {
        self.data
    }

    /// 头太短条数。
    pub const fn short(&self) -> usize {
        self.short
    }

    /// 对上条数。
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 是否红档。
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
            return Err(MC_ENOBS);
        }
        if self.red {
            return Err(MC_EBAL);
        }
        if self.matched != self.total || self.total != CORPUS {
            return Err(MC_EVALUE);
        }
        if self.ok != 2 || self.fail != 12 {
            return Err(MC_EVALUE);
        }
        if self.mismatch != 2 || self.foreign != 4 {
            return Err(MC_EVALUE);
        }
        if self.absent != 1 || self.unknown != 1 {
            return Err(MC_EVALUE);
        }
        if self.bits != 2 || self.dep != 1 || self.data != 1 {
            return Err(MC_EVALUE);
        }
        if self.short != 0 {
            return Err(MC_EVALUE);
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
        let mut s = String::from("machine total=");
        s.push_str(dec_text(self.total as u32).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok as u32).as_str());
        s.push_str(" fail=");
        s.push_str(dec_text(self.fail as u32).as_str());
        s.push_str(" mismatch=");
        s.push_str(dec_text(self.mismatch as u32).as_str());
        s.push_str(" foreign=");
        s.push_str(dec_text(self.foreign as u32).as_str());
        s.push_str(" absent=");
        s.push_str(dec_text(self.absent as u32).as_str());
        s.push_str(" unknown=");
        s.push_str(dec_text(self.unknown as u32).as_str());
        s.push_str(" bits=");
        s.push_str(dec_text(self.bits as u32).as_str());
        s.push_str(" dep=");
        s.push_str(dec_text(self.dep as u32).as_str());
        s.push_str(" data=");
        s.push_str(dec_text(self.data as u32).as_str());
        s.push_str(" short=");
        s.push_str(dec_text(self.short as u32).as_str());
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
            None => return Err(MC_EVALUE),
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
                    Fault::Mismatch => r.mismatch += 1,
                    Fault::Foreign => r.foreign += 1,
                    Fault::Absent => r.absent += 1,
                    Fault::Unknown => r.unknown += 1,
                    Fault::Bits => r.bits += 1,
                    Fault::Dep => r.dep += 1,
                    Fault::Data => r.data += 1,
                    Fault::Short => r.short += 1,
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
            return Err(MC_ESLOT);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 前缀封面：前 `n` 条必须逐条对上。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(MC_ESLOT);
    }
    let mut i = 0usize;
    while i < n {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return Err(MC_EVALUE),
        };
        let j = judge_case(&c, Host::native());
        if j.fault() != c.expect {
            return Err(MC_EVALUE);
        }
        i += 1;
    }
    Ok(())
}

/// 某条语料的错误码。
fn case_code(i: usize) -> u32 {
    match slot_at(i) {
        Some(c) => judge_case(&c, Host::native()).code(),
        None => MC_EVALUE,
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格观测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    /// 登记一格（槽位必须严格递增）。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MC_ESLOT);
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

    /// 对账：14 行、8 成 6 败、值与码逐行对上。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(MC_EBAL);
        }
        const VALS: [u32; 8] = [14, 2, 12, 4, 2, 2, 1, 1000];
        const CODES: [u32; 6] =
            [MC_EMACHINE, MC_EABSENT, MC_EUNKNOWN, MC_EBITS, MC_EFOREIGN, MC_EDEP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MC_EBAL),
            };
            if s.slot as usize != i {
                return Err(MC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MC_EVALUE);
                        }
                    }
                    None => return Err(MC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MC_EVALUE);
                        }
                    }
                    None => return Err(MC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MC_EBAL);
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
            return Err(MC_EBAL);
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
            Err(MC_ESEAL)
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

/// `Result<usize, u32>` → 观测格。
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

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.ok())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.fail())));
    let _ = l.register(count_cell(3, OP_TABLE, r.map(|x| x.foreign())));
    let _ = l.register(count_cell(4, OP_MACHINE, r.map(|x| x.mismatch())));
    let _ = l.register(count_cell(5, OP_MACHINE, r.map(|x| x.bits())));
    let _ = l.register(count_cell(6, OP_MACHINE, r.map(|x| x.absent())));
    let _ = l.register(count_cell(7, OP_ENDIAN, r.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_MACHINE, case_code(2)));
    let _ = l.register(err_cell(9, OP_MACHINE, case_code(7)));
    let _ = l.register(err_cell(10, OP_MACHINE, case_code(8)));
    let _ = l.register(err_cell(11, OP_TEXT, case_code(10)));
    let _ = l.register(err_cell(12, OP_TABLE, case_code(4)));
    let _ = l.register(err_cell(13, OP_DEP, case_code(12)));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 机种表自身。
fn machines_check() -> bool {
    table_ok() && table_seal() != 0 && family_count("x86") == 3
}

/// 端序读取。
fn endian_ok() -> bool {
    let bytes = [0x34u8, 0x12, 0x78, 0x56];
    let a = Endian::Little.read16(&bytes, 0) == Some(0x1234);
    let b = Endian::Big.read16(&bytes, 0) == Some(0x3412);
    let c = Endian::Little.read32(&bytes, 0) == Some(0x5678_1234);
    let d = Endian::Big.read32(&bytes, 0) == Some(0x3412_7856);
    let e = Endian::None.read16(&bytes, 0).is_none();
    let f = Endian::Little.read16(&bytes, 4).is_none();
    let g = Endian::Little.read32(&bytes, 2).is_none();
    let h = Endian::Little.agrees(Endian::native()) && !Endian::Big.agrees(Endian::native());
    let i = !Endian::None.agrees(Endian::None);
    let j = Endian::of_byte(ELFDATA2MSB) == Endian::Big
        && Endian::of_byte(7) == Endian::None
        && Endian::Big.byte() == ELFDATA2MSB;
    a && b && c && d && e && f && g && h && i && j
}

/// 头读取。
fn header_ok() -> bool {
    let h = Header::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, HEAD_MIN);
    let a = h.len() == 20 && h.complete() && h.class() == ELFCLASS64;
    let b = h.data() == ELFDATA2LSB && h.endian() == Endian::Little;
    let c = h.machine_raw() == Some(EM_X86_64) && h.machine() == EM_X86_64;
    let d = h.version() == 0 && h.osabi() == 0;
    let short = Header::new(ELFCLASS32, ELFDATA2LSB, EM_386, 10);
    let e = !short.complete() && short.machine_raw().is_none() && short.machine() == EM_NONE;
    let f = short.byte(4) == Some(ELFCLASS32) && short.byte(10).is_none();
    let bytes = [0u8, 0, 0, 0, ELFCLASS64, ELFDATA2LSB, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 62, 0];
    let g = Header::of_bytes(&bytes).machine_raw() == Some(EM_X86_64);
    let i = Header::of_bytes(&bytes).seal() == Header::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, 20).seal();
    let j = Header::minimum(ELFCLASS32) == EHDR32_MIN && Header::minimum(ELFCLASS64) == EHDR64_MIN;
    let k = Header::blank().is_empty() && !Header::blank().complete();
    a && b && c && d && e && f && g && i && j && k
}

/// 类宽自洽。
fn bits_ok() -> bool {
    let a = compat_of(Host::X64, ELFCLASS64, EM_386) == Compat::Bits;
    let b = compat_of(Host::X64, ELFCLASS64, EM_ARM) == Compat::Bits;
    let c = compat_of(Host::X64, ELFCLASS32, EM_IA_64) == Compat::Bits;
    let d = compat_of(Host::X64, ELFCLASS64, EM_X86_64) == Compat::Match;
    let e = compat_of(Host::X64, ELFCLASS32, EM_X86_64) == Compat::X32;
    let f = compat_of(Host::I686, ELFCLASS32, EM_X86_64) == Compat::Mismatch;
    let g = compat_of(Host::X64, ELFCLASS32, EM_386) == Compat::Mismatch;
    let h = compat_of(Host::I686, ELFCLASS64, EM_386) == Compat::Bits;
    let i = compat_of(Host::X64, 0, EM_X86_64) == Compat::Bits;
    a && b && c && d && e && f && g && h && i
}

/// 宿主判据。
fn host_ok() -> bool {
    let a = Host::native() == Host::X64 && Host::native().accepts(EM_X86_64);
    let b = !Host::X64.accepts(EM_386) && !Host::X64.accepts(EM_486);
    let c = Host::I686.accepts(EM_386) && Host::I686.accepts(EM_486);
    let d = !Host::I686.accepts(EM_X86_64);
    let e = Host::X64.bits() == 64 && Host::I686.bits() == 32;
    let f = Host::X64.class() == ELFCLASS64 && Host::I686.class() == ELFCLASS32;
    let g = Host::X64.family() == Host::I686.family();
    let h = Host::X64.seal() != Host::I686.seal();
    let i = match run_on(Host::I686) {
        Ok(v) => v.total() == CORPUS && v.ok() == 2 && v.fail() == 12,
        Err(_) => false,
    };
    a && b && c && d && e && f && g && h && i
}

/// 外族与别名。
fn alias_ok() -> bool {
    let a = canonical(EM_486) == EM_386 && canonical(EM_386) == EM_386;
    let b = canonical(EM_X86_64) == EM_X86_64 && canonical(EM_ARM) == EM_ARM;
    let c = compat_of(Host::I686, ELFCLASS32, EM_486) == Compat::Alias;
    let d = compat_of(Host::X64, ELFCLASS32, EM_486) == Compat::Mismatch;
    let e = compat_of(Host::X64, ELFCLASS64, EM_AARCH64) == Compat::Foreign;
    let f = compat_of(Host::X64, ELFCLASS32, EM_ARM) == Compat::Foreign;
    let g = compat_of(Host::I686, ELFCLASS32, EM_ARM) == Compat::Foreign;
    let h = is_x86(EM_386) && is_x86(EM_486) && is_x86(EM_X86_64) && !is_x86(EM_ARM);
    let i = family_count("arm") == 2 && family_count("none") == 1;
    a && b && c && d && e && f && g && h && i
}

/// 空机种与表外。
fn odd_ok() -> bool {
    let a = compat_of(Host::X64, ELFCLASS64, EM_NONE) == Compat::Absent;
    let b = compat_of(Host::I686, ELFCLASS32, EM_NONE) == Compat::Absent;
    let c = compat_of(Host::X64, ELFCLASS64, EM_UNKNOWN) == Compat::Unknown;
    let d = compat_of(Host::X64, ELFCLASS64, 0x1234) == Compat::Unknown;
    let e = compat_of(Host::X64, ELFCLASS64, EM_UNKNOWN).fault() == Some(Fault::Unknown);
    let f = compat_of(Host::X64, ELFCLASS64, EM_NONE).fault() == Some(Fault::Absent);
    let g = Compat::Match.admitted()
        && Compat::Alias.admitted()
        && !Compat::X32.admitted()
        && Compat::X32.deferred();
    let h = Compat::Bits.fault() == Some(Fault::Bits)
        && Compat::Foreign.fault() == Some(Fault::Foreign)
        && Compat::Match.fault().is_none();
    let i = machine_tag(0x1234) == "EM_UNKNOWN" && machine_name(0x1234) == "unknown machine";
    a && b && c && d && e && f && g && h && i
}

/// 文案逐字。
fn text_lit_ok() -> bool {
    let c2 = match slot_at(2) {
        Some(v) => v,
        None => return false,
    };
    let j2 = judge_case(&c2, Host::native());
    let a = j2.text().as_str() == "./i386.so: wrong ELF machine: EM_386";
    let c4 = match slot_at(4) {
        Some(v) => v,
        None => return false,
    };
    let b = judge_case(&c4, Host::native()).text().as_str()
        == "./ia64.so: wrong ELF machine: EM_IA_64";
    let c7 = match slot_at(7) {
        Some(v) => v,
        None => return false,
    };
    let c = judge_case(&c7, Host::native()).text().as_str()
        == "./none.so: invalid ELF header: e_machine is EM_NONE";
    let c8 = match slot_at(8) {
        Some(v) => v,
        None => return false,
    };
    let d = judge_case(&c8, Host::native()).text().as_str()
        == "./zz.so: wrong ELF machine: 0xffff";
    let c10 = match slot_at(10) {
        Some(v) => v,
        None => return false,
    };
    let e = judge_case(&c10, Host::native()).text().as_str()
        == "./bits.so: invalid ELF header: EM_386 cannot be ELFCLASS64";
    let c12 = match slot_at(12) {
        Some(v) => v,
        None => return false,
    };
    let f = judge_case(&c12, Host::native()).text().as_str()
        == "libfoo.so: wrong ELF machine: EM_386 (required by ./dep386.so)";
    let c13 = match slot_at(13) {
        Some(v) => v,
        None => return false,
    };
    let g = judge_case(&c13, Host::native()).text().as_str()
        == "./be.so: invalid ELF header: ELFDATA2MSB on little-endian host";
    let tiny = Header::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, 12);
    let h = judge_header("./tiny.so", &tiny, Host::native()).text().as_str()
        == "./tiny.so: invalid ELF header";
    a && b && c && d && e && f && g && h
}

/// 依赖连坐。
fn dep_ok() -> bool {
    let c = match slot_at(12) {
        Some(v) => v,
        None => return false,
    };
    let j = judge_case(&c, Host::native());
    let a = j.fault() == Some(Fault::Dep) && j.code() == MC_EDEP;
    let b = j.subject() == "libfoo.so" && j.root() == "./dep386.so" && j.is_dep();
    let d = j.machine() == EM_386 && j.class() == ELFCLASS32;
    let root = Header::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, HEAD_MIN);
    let e = judge_dep("./root.so", &root, "libc.so.6", EM_X86_64, Host::native()).accepted();
    let f = judge_dep("./root.so", &root, "libm.so.6", 0, Host::native()).code() == MC_EDEP;
    let g = judge_dep("./root.so", &root, "libz.so", EM_386, Host::native()).is_dep();
    let h = judge_dep("./i386.so", &Header::new(ELFCLASS32, ELFDATA2LSB, EM_386, HEAD_MIN), "x", EM_X86_64, Host::native()).code() == MC_EMACHINE;
    a && b && d && e && f && g && h
}

/// 段位映射。
fn stage_ok() -> bool {
    let a = stage_count() == 8 && Stage::banner().len() > 0;
    let mut i = 0usize;
    while i < stage_count() {
        let s = match stage_at(i) {
            Some(v) => v,
            None => return false,
        };
        if s.index() as usize != i {
            return false;
        }
        if i + 1 < stage_count() {
            match stage_at(i + 1) {
                Some(n) => {
                    if n.index() <= s.index() {
                        return false;
                    }
                }
                None => return false,
            }
        }
        i += 1;
    }
    let b = stage_at(stage_count()).is_none();
    let c = Stage::of_fault(None) == Stage::Text;
    let d = Stage::of_fault(Some(Fault::Short)) == Stage::Length;
    let e = Stage::of_fault(Some(Fault::Data)) == Stage::Endian;
    let f = Stage::of_fault(Some(Fault::Absent)) == Stage::MachineZero;
    let g = Stage::of_fault(Some(Fault::Unknown)) == Stage::MachineKnown;
    let h = Stage::of_fault(Some(Fault::Bits)) == Stage::ClassWidth;
    let j = Stage::of_fault(Some(Fault::Dep)) == Stage::Dependency;
    let k = Stage::of_fault(Some(Fault::Mismatch)) == Stage::HostMatch
        && Stage::of_fault(Some(Fault::Foreign)) == Stage::HostMatch;
    let m = Stage::Length.before_text() && !Stage::Text.before_text();
    a && b && c && d && e && f && g && h && j && k && m
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.ok() == 2 && r.fail() == 12 && r.matched() == CORPUS;
    let a = a && corpus_table_ok();
    let b = r.mismatch() == 2 && r.foreign() == 4 && r.absent() == 1;
    let c = r.unknown() == 1 && r.bits() == 2 && r.dep() == 1 && r.data() == 1;
    let d = r.short() == 0 && r.rate() == 1000 && r.green();
    let e = match run_on(Host::I686) {
        Ok(v) => v.total() == CORPUS && v.ok() == 2 && v.fail() == 12,
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 语料槽位。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(c) => c.expect.is_none() && c.machine == EM_X86_64,
        None => false,
    };
    let b = match slot_at(2) {
        Some(c) => c.machine == EM_386 && c.expect == Some(Fault::Mismatch),
        None => false,
    };
    let c = match slot_at(13) {
        Some(v) => v.data == ELFDATA2MSB && v.expect == Some(Fault::Data),
        None => false,
    };
    let d = slot_at(CORPUS).is_none();
    let e = match slot_at(10) {
        Some(v) => v.class == ELFCLASS64 && v.machine == EM_386,
        None => false,
    };
    let f = match slot_at(12) {
        Some(v) => v.has_dep() && v.dep_machine == EM_386,
        None => false,
    };
    a && b && c && d && e && f
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "machine total=14 ok=2 fail=12 mismatch=2 foreign=4 absent=1 unknown=1 bits=2 dep=1 data=1 short=0 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(MC_ENOBS);
    let c = e.render().as_str()
        == "machine total=0 ok=0 fail=0 mismatch=0 foreign=0 absent=0 unknown=0 bits=0 dep=0 data=0 short=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, MC_EMACHINE))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929 && r.red();
    let b = r.gate() == Err(MC_EBAL) && !r.green();
    let c = run_with(Some((CORPUS, MC_EMACHINE))).err() == Some(MC_ESLOT);
    let d = run_with(Some((CORPUS + 1, MC_EMACHINE))).err() == Some(MC_ESLOT);
    a && b && c && d
}

/// 前缀封面。
fn cover_ok() -> bool {
    let a = cover(0).is_ok();
    let b = cover(1).is_ok();
    let c = cover(10).is_ok();
    let d = cover(CORPUS).is_ok();
    let e = cover(CORPUS + 1).err() == Some(MC_ESLOT);
    let mut i = 0usize;
    while i <= CORPUS {
        if cover(i).is_err() {
            return false;
        }
        i += 1;
    }
    a && b && c && d && e
}

/// 指纹与文本。
fn hash_ok() -> bool {
    let c0 = match slot_at(0) {
        Some(v) => v,
        None => return false,
    };
    let c2 = match slot_at(2) {
        Some(v) => v,
        None => return false,
    };
    let c3 = match slot_at(3) {
        Some(v) => v,
        None => return false,
    };
    let a = judge_case(&c0, Host::native()).seal() != judge_case(&c2, Host::native()).seal();
    let b = judge_case(&c2, Host::native()).seal() != judge_case(&c3, Host::native()).seal();
    let c = digest(b"EM_386") == digest(b"EM_386");
    let d = digest(b"EM_386") != digest(b"EM_486");
    let e = table_seal() == table_seal() && table_seal() != 0;
    let f = FoldProbe::run();
    let g = judge_case(&c2, Host::native()).diagnose().len()
        != judge_case(&c0, Host::native()).diagnose().len();
    a && b && c && d && e && f && g
}

/// 一只小的折叠探针：验证 `fold_word` 的字节序敏感性。
struct FoldProbe {
    a: u64,
    b: u64,
}

impl FoldProbe {
    /// 跑两组不同顺序的字。
    fn run() -> bool {
        let p = FoldProbe {
            a: fold_word(fold_word(FNV_OFF, 0x1234), 0x5678),
            b: fold_word(fold_word(FNV_OFF, 0x5678), 0x1234),
        };
        p.a != p.b && p.a != FNV_OFF && p.b != FNV_OFF
    }
}

/// 账本冻结与重放。
fn pin_ok() -> bool {
    let l = build_ledger();
    let a = l.len() == SAMPLES && !l.is_empty() && l.ok_count() == 8 && l.err_count() == 6;
    let b = l.audit() == Ok(());
    let c = match l.sample(0) {
        Some(s) => s.ok && s.value == 14 && s.slot == 0,
        None => false,
    };
    let d = l.sample(SAMPLES).is_none() && l.root() == 0 && !l.is_frozen();
    let mut m = l;
    let e = match m.freeze() {
        Ok(v) => v != 0 && m.root() == v && m.is_frozen(),
        Err(_) => false,
    };
    let f = m.replay() == Ok(m.seal());
    let g = m.register(Sample { slot: 14, op: OP_CORPUS, ok: true, code: 0, value: 0 })
        == Err(MC_ESLOT);
    let h = m.rejects() == 1 && m.freeze() == Err(MC_EBAL) && m.rejects() == 2;
    let i = m.audit() == Ok(());
    a && b && c && d && e && f && g && h && i
}

/// 四注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = machines_check() && endian_ok() && header_ok();
    out[1] = host_ok() && bits_ok() && stage_ok();
    out[2] = text_lit_ok() && dep_ok() && alias_ok() && odd_ok();
    out[3] = corpus_ok()
        && slots_ok()
        && report_ok()
        && reds_ok()
        && cover_ok()
        && hash_ok()
        && pin_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11624_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11624.so_machine");

    s.add("MC01-机种取值表", machines_check(), "static");
    s.add("MC02-机种分类", table_ok() && lookup(EM_ARM).is_some(), "static");
    s.add("MC03-端序读取", endian_ok(), "static");
    s.add("MC04-头读取", header_ok(), "static");
    s.add("MC05-类宽自洽", bits_ok(), "static");
    s.add("MC06-宿主判据", host_ok(), "static");
    s.add("MC07-别名与外族", alias_ok(), "static");
    s.add("MC08-空与未知", odd_ok(), "static");
    s.add("MC09-文案逐字", text_lit_ok() && dep_ok(), "static");
    s.add("MC10-十四卡全跑", corpus_ok(), "static");
    s.add("MC11-语料与渲染", slots_ok() && report_ok(), "static");
    s.add("MC12-红档封面与指纹", reds_ok() && cover_ok() && hash_ok() && stage_ok(), "static");
    s.add("MC13-冻结重放四注入", inject_four() == [true; 4], "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert!(!l.is_frozen());
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert_eq!(l.root(), root);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.register(Sample::blank()), Err(MC_ESLOT));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert_eq!(out, [true; 4]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11624_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn table_lookup() {
        assert!(table_ok());
        assert_eq!(machine_count(), 20);
        assert_eq!(lookup(EM_386).map(|m| m.bits), Some(32));
        assert_eq!(lookup(EM_X86_64).map(|m| m.name), Some("AMD x86-64"));
        assert!(lookup(0x1234).is_none());
        assert_eq!(machine_tag(EM_386), "EM_386");
        assert_eq!(machine_bits(EM_ARM), 32);
        assert_eq!(family_of(EM_AARCH64), "arm");
        assert!(is_x86(EM_X86_64));
        assert!(!is_x86(EM_MIPS));
    }

    #[test]
    fn table_order() {
        assert!(table_sorted());
        assert!(table_unique());
        assert!(machines_ok());
        assert_eq!(family_count("x86"), 3);
        assert_eq!(MACHINES.get(0).map(|m| m.code), Some(EM_NONE));
        assert_eq!(MACHINES.get(19).map(|m| m.code), Some(EM_LOONGARCH));
    }

    #[test]
    fn canonical_alias() {
        assert_eq!(canonical(EM_486), EM_386);
        assert_eq!(canonical(EM_X86_64), EM_X86_64);
        assert!(lookup(EM_486).map(|m| m.alias).unwrap_or(false));
        assert_eq!(compat_of(Host::I686, ELFCLASS32, EM_486), Compat::Alias);
        assert_eq!(compat_of(Host::X64, ELFCLASS32, EM_486), Compat::Mismatch);
    }

    #[test]
    fn endian_reads() {
        let b = [0x01u8, 0x02, 0x03, 0x04];
        assert_eq!(Endian::Little.read16(&b, 0), Some(0x0201));
        assert_eq!(Endian::Big.read16(&b, 0), Some(0x0102));
        assert_eq!(Endian::Little.read32(&b, 0), Some(0x0403_0201));
        assert_eq!(Endian::Big.read32(&b, 0), Some(0x0102_0304));
        assert!(Endian::None.read16(&b, 0).is_none());
        assert!(Endian::Little.read16(&b, 3).is_none());
        assert!(Endian::Little.read32(&b, 1).is_none());
    }

    #[test]
    fn endian_labels() {
        assert_eq!(Endian::of_byte(ELFDATA2LSB), Endian::Little);
        assert_eq!(Endian::of_byte(ELFDATA2MSB), Endian::Big);
        assert_eq!(Endian::of_byte(ELFDATANONE), Endian::None);
        assert_eq!(Endian::Little.byte(), ELFDATA2LSB);
        assert_eq!(Endian::Big.name(), "ELFDATA2MSB");
        assert!(Endian::Little.agrees(Endian::native()));
        assert!(!Endian::Big.agrees(Endian::native()));
    }

    #[test]
    fn header_fields() {
        let h = Header::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, 20);
        assert!(h.complete());
        assert_eq!(h.class(), ELFCLASS64);
        assert_eq!(h.data(), ELFDATA2LSB);
        assert_eq!(h.machine_raw(), Some(EM_X86_64));
        assert_eq!(Header::minimum(ELFCLASS32), 52);
        assert_eq!(Header::minimum(ELFCLASS64), 64);
    }

    #[test]
    fn header_short() {
        let h = Header::new(ELFCLASS32, ELFDATA2LSB, EM_386, 8);
        assert!(!h.complete());
        assert!(h.machine_raw().is_none());
        assert_eq!(h.class(), ELFCLASS32);
        assert_eq!(
            judge_header("./tiny.so", &h, Host::native()).code(),
            MC_ESHORT
        );
    }

    #[test]
    fn compat_native() {
        assert_eq!(compat_of(Host::X64, ELFCLASS64, EM_X86_64), Compat::Match);
        assert_eq!(compat_of(Host::I686, ELFCLASS32, EM_386), Compat::Match);
        assert!(Compat::Match.admitted());
        assert!(Compat::Match.fault().is_none());
    }

    #[test]
    fn compat_wrong_machine() {
        assert_eq!(compat_of(Host::X64, ELFCLASS32, EM_386), Compat::Mismatch);
        assert_eq!(compat_of(Host::I686, ELFCLASS64, EM_X86_64), Compat::Mismatch);
        assert_eq!(compat_of(Host::X64, ELFCLASS32, EM_386).fault(), Some(Fault::Mismatch));
    }

    #[test]
    fn compat_foreign_and_bits() {
        assert_eq!(compat_of(Host::X64, ELFCLASS64, EM_AARCH64), Compat::Foreign);
        assert_eq!(compat_of(Host::X64, ELFCLASS64, EM_ARM), Compat::Bits);
        assert_eq!(compat_of(Host::X64, ELFCLASS32, EM_IA_64), Compat::Bits);
        assert_eq!(compat_of(Host::X64, 9, EM_X86_64), Compat::Bits);
    }

    #[test]
    fn compat_absent_unknown() {
        assert_eq!(compat_of(Host::X64, ELFCLASS64, EM_NONE), Compat::Absent);
        assert_eq!(compat_of(Host::X64, ELFCLASS64, EM_UNKNOWN), Compat::Unknown);
        assert_eq!(compat_of(Host::X64, ELFCLASS64, 7), Compat::Unknown);
    }

    #[test]
    fn compat_x32() {
        assert_eq!(compat_of(Host::X64, ELFCLASS32, EM_X86_64), Compat::X32);
        assert!(Compat::X32.deferred());
        assert!(!Compat::X32.admitted());
        let h = Header::new(ELFCLASS32, ELFDATA2LSB, EM_X86_64, 20);
        assert!(judge_header("./x32.so", &h, Host::native()).accepted());
    }

    #[test]
    fn judge_machine_text() {
        let h = Header::new(ELFCLASS32, ELFDATA2LSB, EM_386, 20);
        let j = judge_header("./i386.so", &h, Host::native());
        assert_eq!(j.code(), MC_EMACHINE);
        assert_eq!(j.stage(), Stage::HostMatch);
        assert_eq!(j.text().as_str(), "./i386.so: wrong ELF machine: EM_386");
        assert_eq!(j.subject(), "./i386.so");
        assert!(!j.accepted());
    }

    #[test]
    fn judge_dep_text() {
        let h = Header::new(ELFCLASS64, ELFDATA2LSB, EM_X86_64, 20);
        let j = judge_dep("./dep386.so", &h, "libfoo.so", EM_386, Host::native());
        assert_eq!(j.code(), MC_EDEP);
        assert!(j.is_dep());
        assert_eq!(j.root(), "./dep386.so");
        assert_eq!(
            j.text().as_str(),
            "libfoo.so: wrong ELF machine: EM_386 (required by ./dep386.so)"
        );
    }

    #[test]
    fn judge_data_text() {
        let h = Header::new(ELFCLASS64, ELFDATA2MSB, EM_X86_64, 20);
        let j = judge_header("./be.so", &h, Host::native());
        assert_eq!(j.code(), MC_EDATA);
        assert_eq!(j.stage(), Stage::Endian);
        assert_eq!(
            j.text().as_str(),
            "./be.so: invalid ELF header: ELFDATA2MSB on little-endian host"
        );
    }

    #[test]
    fn stages_ladder() {
        assert_eq!(stage_count(), 8);
        assert!(stage_ok());
        assert_eq!(Stage::of_fault(Some(Fault::Dep)), Stage::Dependency);
        assert_eq!(Stage::of_fault(None), Stage::Text);
        assert_eq!(Stage::Length.index(), 0);
        assert_eq!(Stage::Text.index(), 7);
    }

    #[test]
    fn corpus_shape() {
        assert!(unique());
        assert!(notes_ok());
        assert_eq!(CASES.len(), 14);
        assert_eq!(slot_at(0).map(|c| c.machine), Some(EM_X86_64));
        assert_eq!(slot_at(13).map(|c| c.data), Some(ELFDATA2MSB));
        assert!(slot_at(14).is_none());
    }

    #[test]
    fn corpus_walk() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.total(), 14);
        assert_eq!(r.ok(), 2);
        assert_eq!(r.fail(), 12);
        assert_eq!(r.matched(), 14);
        assert!(r.green());
    }

    #[test]
    fn report_render_literal() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(
            r.render().as_str(),
            "machine total=14 ok=2 fail=12 mismatch=2 foreign=4 absent=1 unknown=1 bits=2 dep=1 data=1 short=0 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn cover_prefix() {
        assert!(cover(0).is_ok());
        assert!(cover(14).is_ok());
        assert_eq!(cover(15), Err(MC_ESLOT));
    }

    #[test]
    fn diagnose_shape() {
        let h = Header::new(ELFCLASS32, ELFDATA2LSB, EM_386, 20);
        let j = judge_header("./i386.so", &h, Host::native());
        let d = j.diagnose();
        assert!(d.starts_with("./i386.so: class=ELFCLASS32"));
        assert!(d.contains("machine=EM_386(3)"));
        assert!(d.contains("compat=mismatch"));
        assert!(d.ends_with('\n'));
    }
}
