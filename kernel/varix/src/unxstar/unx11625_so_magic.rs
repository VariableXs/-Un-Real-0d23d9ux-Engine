//! UNX-F11625 .so 缺 ELF 魔数判据
//!
//! 装载器打开一份共享对象后做的第一件事不是看类宽、也不是看机种，而是读
//! `e_ident` 的前四个字节：`7f 45 4c 46`（`\x7fELF`）。这四个字节是整条
//! 管线的入场券，任何一个不对，后面所有字段的解释都失去意义。glibc 在
//! `_dl_map_object_from_fd` 里先读 16 字节 `e_ident`，`memcmp` 不过就直接
//! 给 `invalid ELF header`，连 `e_type` 都不看。本模块把这条"入场券"单独
//! 拆出来，做成可逐字节解释的判据。
//!
//! 魔数错误不是一种，而是一族。运维最常遇到的几种：
//!
//! - 文件根本不是 ELF：首字节不是 `0x7f`，整段是文本或空文件；
//! - 最后一个字母写错：`\x7fELG`，常见于手工拼接头或截断传输；
//! - 全零：打包工具写了一个 16 字节的零块，看起来"有头"其实没头；
//! - 魔数整体后移：文件前面粘了 shebang、签名或对齐填充，`0x7fELF`
//!   出现在偏移 1、4、8……装载器只认偏移 0，于是同样报 `invalid ELF header`，
//!   但诊断必须指出真实偏移，否则运维会以为文件彻底坏了；
//! - 魔数对了但 `e_ident` 里其它字节不对：版本号 0、数据编码 0、
//!   OSABI 未知、ABIVERSION 超界。
//!
//! 因此本模块先做**签名分类**（`Signature`），再做**扫描诊断**
//! （`Scan`，只用于解释错位，不改变"偏移 0 才算数"的裁决），最后才看
//! `e_ident` 其余字段。次序不能倒：版本号只有在魔数正确时才有意义；错位
//! 魔数必须报"错位"而不是"版本 0"，否则根因就被后一段噪声覆盖。
//!
//! 文案逐字稳定：
//!
//! - `./elg.so: invalid ELF header`
//! - `./empty.so: file too short`
//! - `./shift.so: invalid ELF header: ELF magic at offset 1`
//! - `./ver0.so: invalid ELF header: e_ident[EI_VERSION]=0`
//! - `./data0.so: invalid ELF header: e_ident[EI_DATA]=0`
//! - `./osabi.so: invalid ELF header: e_ident[EI_OSABI]=66`
//! - `./abi7.so: invalid ELF header: e_ident[EI_ABIVERSION]=7`
//! - `libfoo.so: invalid ELF header (required by ./dep.so)`
//!
//! 闸门次序：长度 → 魔数 → 错位诊断 → 版本 → 数据编码 → OSABI → ABIVERSION → 依赖连坐。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfc80;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// `e_ident` 长度。
pub const EI_NIDENT: usize = 16;

/// `EI_MAG0` 偏移。
pub const EI_MAG0: usize = 0;

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

/// 扫描窗口上限。
pub const SCAN_MAX: usize = 64;

// ---------------------------------------------------------------------------
// e_ident 取值
// ---------------------------------------------------------------------------

/// 无类（仅占位）。
pub const ELFCLASSNONE: u8 = 0;
/// 64 位类。
pub const ELFCLASS64: u8 = 2;

/// 未定数据编码。
pub const ELFDATANONE: u8 = 0;
/// 小端。
pub const ELFDATA2LSB: u8 = 1;
/// 大端。
pub const ELFDATA2MSB: u8 = 2;

/// 无效版本。
pub const EV_NONE: u8 = 0;
/// 当前版本。
pub const EV_CURRENT: u8 = 1;
/// 版本编号占位。
pub const EV_NUM: u8 = 2;

/// System V ABI。
pub const ELFOSABI_SYSV: u8 = 0;
/// HP-UX。
pub const ELFOSABI_HPUX: u8 = 1;
/// NetBSD。
pub const ELFOSABI_NETBSD: u8 = 2;
/// GNU/Linux。
pub const ELFOSABI_GNU: u8 = 3;
/// Solaris。
pub const ELFOSABI_SOLARIS: u8 = 6;
/// AIX。
pub const ELFOSABI_AIX: u8 = 7;
/// IRIX。
pub const ELFOSABI_IRIX: u8 = 8;
/// FreeBSD。
pub const ELFOSABI_FREEBSD: u8 = 9;
/// Tru64。
pub const ELFOSABI_TRU64: u8 = 10;
/// ARM EABI。
pub const ELFOSABI_ARM: u8 = 97;
/// 独立环境。
pub const ELFOSABI_STANDALONE: u8 = 255;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 魔数不符。
pub const MG_EBAD: u32 = SEG_BASE + 0x01;
/// 太短，读不到完整 ident。
pub const MG_ESHORT: u32 = SEG_BASE + 0x02;
/// 魔数错位。
pub const MG_EDISP: u32 = SEG_BASE + 0x03;
/// `EI_VERSION` 不符。
pub const MG_EVERSION: u32 = SEG_BASE + 0x04;
/// `EI_DATA` 不符。
pub const MG_EDATA: u32 = SEG_BASE + 0x05;
/// OSABI 不受理。
pub const MG_EOSABI: u32 = SEG_BASE + 0x06;
/// ABIVERSION 超界。
pub const MG_EABI: u32 = SEG_BASE + 0x07;
/// 依赖魔数不符。
pub const MG_EDEP: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const MG_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const MG_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const MG_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结指纹不符。
pub const MG_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const MG_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 语料扫描。
pub const OP_CORPUS: u8 = 1;
/// 签名比较。
pub const OP_MAGIC: u8 = 2;
/// 扫描诊断。
pub const OP_SCAN: u8 = 3;
/// 版本判据。
pub const OP_VERSION: u8 = 4;
/// OSABI 判据。
pub const OP_OSABI: u8 = 5;
/// 依赖判定。
pub const OP_DEP: u8 = 6;

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

/// 两字节十六进制。
fn hex2(v: u8) -> String {
    hex_text(v as u32, 2)
}

/// 拼接 `<名>: <尾>`。
fn named(name: &str, tail: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(tail);
    s
}

// ---------------------------------------------------------------------------
// 魔数
// ---------------------------------------------------------------------------

/// 四字节 ELF 魔数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Magic {
    bytes: [u8; 4],
}

impl Magic {
    /// 正确魔数。
    pub const fn good() -> Magic {
        Magic { bytes: [0x7f, b'E', b'L', b'F'] }
    }

    /// 全零魔数。
    pub const fn blank() -> Magic {
        Magic { bytes: [0, 0, 0, 0] }
    }

    /// 由四字节构造。
    pub const fn new(bytes: [u8; 4]) -> Magic {
        Magic { bytes }
    }

    /// 取字节组。
    pub const fn bytes(&self) -> [u8; 4] {
        self.bytes
    }

    /// 取一字节。
    pub fn byte(&self, i: usize) -> Option<u8> {
        self.bytes.get(i).copied()
    }

    /// 是否等于正确魔数。
    pub fn is_good(&self) -> bool {
        self.bytes == Magic::good().bytes()
    }

    /// 与正确魔数相同的前缀长度（0..4）。
    pub fn prefix_len(&self) -> u8 {
        let good = Magic::good().bytes;
        let mut n = 0usize;
        while n < 4 {
            match (self.bytes.get(n), good.get(n)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        break;
                    }
                }
                _ => break,
            }
            n += 1;
        }
        n as u8
    }

    /// 第一个不同的位置。
    pub fn mismatch(&self) -> Option<usize> {
        let good = Magic::good().bytes;
        let mut i = 0usize;
        while i < 4 {
            match (self.bytes.get(i), good.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return Some(i);
                    }
                }
                _ => return Some(i),
            }
            i += 1;
        }
        None
    }

    /// 逐位匹配掩码（bit i 表示第 i 字节相同）。
    pub fn mask(&self) -> u8 {
        let good = Magic::good().bytes;
        let mut m = 0u8;
        let mut i = 0usize;
        while i < 4 {
            match (self.bytes.get(i), good.get(i)) {
                (Some(a), Some(b)) => {
                    if a == b {
                        m |= 1u8 << i;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        m
    }

    /// 十六进制展示，如 `7f 45 4c 46`。
    pub fn hex(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < 4 {
            if i > 0 {
                s.push(' ');
            }
            if let Some(b) = self.bytes.get(i) {
                s.push_str(hex2(*b).as_str());
            }
            i += 1;
        }
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < 4 {
            if let Some(b) = self.bytes.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        h
    }
}

/// 在 `bytes[start..]` 里找正确魔数，返回绝对偏移。
pub fn find_magic(bytes: &[u8], start: usize) -> Option<usize> {
    let good = Magic::good().bytes();
    let mut i = start;
    while i < bytes.len() {
        let end = match i.checked_add(4) {
            Some(v) => v,
            None => return None,
        };
        if end > bytes.len() {
            return None;
        }
        let mut ok = true;
        let mut k = 0usize;
        while k < 4 {
            match (bytes.get(i + k), good.get(k)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        ok = false;
                        break;
                    }
                }
                _ => {
                    ok = false;
                    break;
                }
            }
            k += 1;
        }
        if ok {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 签名分类
// ---------------------------------------------------------------------------

/// 首四字节的形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signature {
    /// 正确魔数。
    Good,
    /// 不足四字节。
    Short(usize),
    /// 四个零。
    Blank,
    /// 有 1..3 字节前缀正确。
    Prefix(u8),
    /// 正确魔数出现在偏移 `> 0`。
    Displaced(usize),
    /// 完全无关。
    Other,
}

impl Signature {
    /// 分类。
    pub fn of(bytes: &[u8]) -> Signature {
        if bytes.len() < 4 {
            return Signature::Short(bytes.len());
        }
        let m = match (bytes.get(0), bytes.get(1), bytes.get(2), bytes.get(3)) {
            (Some(a), Some(b), Some(c), Some(d)) => Magic::new([*a, *b, *c, *d]),
            _ => return Signature::Short(bytes.len()),
        };
        if m.is_good() {
            return Signature::Good;
        }
        if m.bytes() == Magic::blank().bytes() {
            return Signature::Blank;
        }
        if let Some(off) = find_magic(bytes, 1) {
            return Signature::Displaced(off);
        }
        let p = m.prefix_len();
        if p > 0 {
            Signature::Prefix(p)
        } else {
            Signature::Other
        }
    }

    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Signature::Good => "good",
            Signature::Short(_) => "short",
            Signature::Blank => "blank",
            Signature::Prefix(_) => "prefix",
            Signature::Displaced(_) => "displaced",
            Signature::Other => "other",
        }
    }

    /// 是否入场。
    pub fn admitted(&self) -> bool {
        match self {
            Signature::Good => true,
            _ => false,
        }
    }

    /// 前缀长度。
    pub fn prefix(&self) -> u8 {
        match self {
            Signature::Good => 4,
            Signature::Prefix(n) => *n,
            _ => 0,
        }
    }

    /// 错位偏移。
    pub fn offset(&self) -> Option<usize> {
        match self {
            Signature::Displaced(o) => Some(*o),
            _ => None,
        }
    }

    /// 说明。
    pub fn detail(&self) -> &'static str {
        match self {
            Signature::Good => "elf magic at offset zero",
            Signature::Short(_) => "fewer than four bytes",
            Signature::Blank => "four zero bytes",
            Signature::Prefix(_) => "partial elf magic",
            Signature::Displaced(_) => "elf magic not at offset zero",
            Signature::Other => "not an elf magic",
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.prefix() as u64);
        h = fold_word(h, self.offset().unwrap_or(0) as u64);
        fold_word(h, digest(self.name().as_bytes()))
    }
}

/// 魔数扫描诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scan {
    width: usize,
    first: Option<usize>,
    at_zero: bool,
}

impl Scan {
    /// 扫一遍窗口。
    pub fn of(bytes: &[u8]) -> Scan {
        let width = if bytes.len() > SCAN_MAX {
            SCAN_MAX
        } else {
            bytes.len()
        };
        let slice = match bytes.get(..width) {
            Some(v) => v,
            None => bytes,
        };
        let first = find_magic(slice, 0);
        let at_zero = first == Some(0);
        Scan { width, first, at_zero }
    }

    /// 窗口宽度。
    pub const fn width(&self) -> usize {
        self.width
    }

    /// 首次出现位置。
    pub const fn first(&self) -> Option<usize> {
        self.first
    }

    /// 是否落在零位。
    pub const fn at_zero(&self) -> bool {
        self.at_zero
    }

    /// 是否错位。
    pub fn displaced(&self) -> bool {
        match self.first {
            Some(0) => false,
            Some(_) => true,
            None => false,
        }
    }

    /// 首次位置文本。
    pub fn first_text(&self) -> String {
        match self.first {
            Some(v) => dec_text(v as u32),
            None => String::from("none"),
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("scan width=");
        s.push_str(dec_text(self.width as u32).as_str());
        s.push_str(" first=");
        s.push_str(self.first_text().as_str());
        s.push_str(" zero=");
        s.push_str(dec_text(self.at_zero as u32).as_str());
        s.push('\n');
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.width as u64);
        h = fold_word(h, self.first.unwrap_or(usize::MAX) as u64);
        fold_word(h, self.at_zero as u64)
    }
}

// ---------------------------------------------------------------------------
// OSABI / ABI
// ---------------------------------------------------------------------------

/// OSABI 标签。
pub fn osabi_name(v: u8) -> &'static str {
    match v {
        ELFOSABI_SYSV => "ELFOSABI_SYSV",
        ELFOSABI_HPUX => "ELFOSABI_HPUX",
        ELFOSABI_NETBSD => "ELFOSABI_NETBSD",
        ELFOSABI_GNU => "ELFOSABI_GNU",
        ELFOSABI_SOLARIS => "ELFOSABI_SOLARIS",
        ELFOSABI_AIX => "ELFOSABI_AIX",
        ELFOSABI_IRIX => "ELFOSABI_IRIX",
        ELFOSABI_FREEBSD => "ELFOSABI_FREEBSD",
        ELFOSABI_TRU64 => "ELFOSABI_TRU64",
        ELFOSABI_ARM => "ELFOSABI_ARM",
        ELFOSABI_STANDALONE => "ELFOSABI_STANDALONE",
        _ => "ELFOSABI_UNKNOWN",
    }
}

/// OSABI 是否受理。
pub fn osabi_ok(v: u8) -> bool {
    match v {
        ELFOSABI_SYSV => true,
        ELFOSABI_GNU => true,
        ELFOSABI_FREEBSD => true,
        _ => false,
    }
}

/// ABIVERSION 是否与 OSABI 配套。
pub fn abi_ok(osabi: u8, abi: u8) -> bool {
    match osabi {
        ELFOSABI_SYSV => abi == 0,
        ELFOSABI_GNU => abi == 0,
        ELFOSABI_FREEBSD => abi <= 2,
        _ => false,
    }
}

/// 数据编码是否是可读的一种。
pub fn data_ok(v: u8) -> bool {
    match v {
        ELFDATA2LSB => true,
        ELFDATA2MSB => true,
        _ => false,
    }
}

/// 数据编码标签。
pub fn data_name(v: u8) -> &'static str {
    match v {
        ELFDATANONE => "ELFDATANONE",
        ELFDATA2LSB => "ELFDATA2LSB",
        ELFDATA2MSB => "ELFDATA2MSB",
        _ => "ELFDATA?",
    }
}

/// 版本是否当前版本。
pub fn version_ok(v: u8) -> bool {
    v == EV_CURRENT
}

// ---------------------------------------------------------------------------
// e_ident
// ---------------------------------------------------------------------------

/// 十六字节 `e_ident` 窗口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ident {
    buf: [u8; EI_NIDENT],
    len: usize,
}

impl Ident {
    /// 空窗口。
    pub const fn blank() -> Ident {
        Ident { buf: [0u8; EI_NIDENT], len: 0 }
    }

    /// 造一份完整 ident。
    pub fn build(
        magic: Magic,
        class: u8,
        data: u8,
        version: u8,
        osabi: u8,
        abi: u8,
        len: usize,
    ) -> Ident {
        let mut id = Ident::blank();
        id.len = if len > EI_NIDENT { EI_NIDENT } else { len };
        let m = magic.bytes();
        let mut i = 0usize;
        while i < 4 {
            if id.len > i {
                if let (Some(src), Some(dst)) = (m.get(i), id.buf.get_mut(i)) {
                    *dst = *src;
                }
            }
            i += 1;
        }
        if id.len > EI_CLASS {
            if let Some(dst) = id.buf.get_mut(EI_CLASS) {
                *dst = class;
            }
        }
        if id.len > EI_DATA {
            if let Some(dst) = id.buf.get_mut(EI_DATA) {
                *dst = data;
            }
        }
        if id.len > EI_VERSION {
            if let Some(dst) = id.buf.get_mut(EI_VERSION) {
                *dst = version;
            }
        }
        if id.len > EI_OSABI {
            if let Some(dst) = id.buf.get_mut(EI_OSABI) {
                *dst = osabi;
            }
        }
        if id.len > EI_ABIVERSION {
            if let Some(dst) = id.buf.get_mut(EI_ABIVERSION) {
                *dst = abi;
            }
        }
        id
    }

    /// 从字节串截取。
    pub fn parse(bytes: &[u8]) -> Ident {
        let mut id = Ident::blank();
        let n = if bytes.len() > EI_NIDENT { EI_NIDENT } else { bytes.len() };
        let mut i = 0usize;
        while i < n {
            match (bytes.get(i), id.buf.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => break,
            }
            i += 1;
        }
        id.len = n;
        id
    }

    /// 整体右移 `k` 字节，前面补零（模拟被前缀粘住的文件）。
    pub fn displace(&mut self, k: usize) -> bool {
        if k == 0 || k >= EI_NIDENT {
            return false;
        }
        let mut out = [0u8; EI_NIDENT];
        let mut i = 0usize;
        while i + k < EI_NIDENT {
            match self.buf.get(i) {
                Some(b) => {
                    if let Some(dst) = out.get_mut(i + k) {
                        *dst = *b;
                    }
                }
                None => break,
            }
            i += 1;
        }
        self.buf = out;
        self.len = EI_NIDENT;
        true
    }

    /// 实长。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否完整。
    pub const fn complete(&self) -> bool {
        self.len >= EI_NIDENT
    }

    /// 取一字节。
    pub fn byte(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.buf.get(i).copied()
    }

    /// 当前窗口。
    pub fn window(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    /// 魔数（不足四字节时缺失部分按零）。
    pub fn magic(&self) -> Magic {
        Magic::new([
            self.byte(0).unwrap_or(0),
            self.byte(1).unwrap_or(0),
            self.byte(2).unwrap_or(0),
            self.byte(3).unwrap_or(0),
        ])
    }

    /// 签名。
    pub fn signature(&self) -> Signature {
        Signature::of(self.window())
    }

    /// 扫描。
    pub fn scan(&self) -> Scan {
        Scan::of(self.window())
    }

    /// 魔数实际偏移。
    pub fn magic_offset(&self) -> Option<usize> {
        self.scan().first()
    }

    /// 类宽字节。
    pub fn class(&self) -> u8 {
        self.byte(EI_CLASS).unwrap_or(ELFCLASSNONE)
    }

    /// 数据编码。
    pub fn data(&self) -> u8 {
        self.byte(EI_DATA).unwrap_or(ELFDATANONE)
    }

    /// 版本。
    pub fn version(&self) -> u8 {
        self.byte(EI_VERSION).unwrap_or(EV_NONE)
    }

    /// OSABI。
    pub fn osabi(&self) -> u8 {
        self.byte(EI_OSABI).unwrap_or(ELFOSABI_SYSV)
    }

    /// ABIVERSION。
    pub fn abi(&self) -> u8 {
        self.byte(EI_ABIVERSION).unwrap_or(0)
    }

    /// 十六进制窗口（每字节两位，空格分隔）。
    pub fn hex(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if i > 0 {
                s.push(' ');
            }
            if let Some(b) = self.buf.get(i) {
                s.push_str(hex2(*b).as_str());
            }
            i += 1;
        }
        s
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
// 根因
// ---------------------------------------------------------------------------

/// 魔数闸门的失败根因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 魔数不符。
    Bad,
    /// 太短。
    Short,
    /// 魔数错位。
    Displaced,
    /// 版本不符。
    Version,
    /// 数据编码不符。
    Data,
    /// OSABI 不受理。
    Osabi,
    /// ABIVERSION 超界。
    Abi,
    /// 依赖连坐。
    Dep,
}

impl Fault {
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            Fault::Bad => "bad-magic",
            Fault::Short => "short",
            Fault::Displaced => "displaced",
            Fault::Version => "version",
            Fault::Data => "data",
            Fault::Osabi => "osabi",
            Fault::Abi => "abi",
            Fault::Dep => "dep",
        }
    }

    /// 错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Bad => MG_EBAD,
            Fault::Short => MG_ESHORT,
            Fault::Displaced => MG_EDISP,
            Fault::Version => MG_EVERSION,
            Fault::Data => MG_EDATA,
            Fault::Osabi => MG_EOSABI,
            Fault::Abi => MG_EABI,
            Fault::Dep => MG_EDEP,
        }
    }

    /// 是否属于魔数族。
    pub fn magic_family(&self) -> bool {
        match self {
            Fault::Bad | Fault::Displaced => true,
            Fault::Short => true,
            _ => false,
        }
    }

    /// 文案。
    pub fn message(&self, subject: &str, root: &str, id: &Ident) -> String {
        match self {
            Fault::Bad => named(subject, "invalid ELF header"),
            Fault::Short => named(subject, "file too short"),
            Fault::Displaced => {
                let mut s = String::from("invalid ELF header: ELF magic at offset ");
                s.push_str(dec_text(id.magic_offset().unwrap_or(0) as u32).as_str());
                named(subject, s.as_str())
            }
            Fault::Version => {
                let mut s = String::from("invalid ELF header: e_ident[EI_VERSION]=");
                s.push_str(dec_text(id.version() as u32).as_str());
                named(subject, s.as_str())
            }
            Fault::Data => {
                let mut s = String::from("invalid ELF header: e_ident[EI_DATA]=");
                s.push_str(dec_text(id.data() as u32).as_str());
                named(subject, s.as_str())
            }
            Fault::Osabi => {
                let mut s = String::from("invalid ELF header: e_ident[EI_OSABI]=");
                s.push_str(dec_text(id.osabi() as u32).as_str());
                named(subject, s.as_str())
            }
            Fault::Abi => {
                let mut s = String::from("invalid ELF header: e_ident[EI_ABIVERSION]=");
                s.push_str(dec_text(id.abi() as u32).as_str());
                named(subject, s.as_str())
            }
            Fault::Dep => {
                let mut s = String::from("invalid ELF header (required by ");
                s.push_str(root);
                s.push(')');
                named(subject, s.as_str())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 结论
// ---------------------------------------------------------------------------

/// 一次魔数判定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judge {
    subject: String,
    root: String,
    sig: Signature,
    version: u8,
    data: u8,
    osabi: u8,
    abi: u8,
    fault: Option<Fault>,
    note: &'static str,
    ident: Ident,
}

impl Judge {
    /// 构造。
    fn make(
        subject: &str,
        root: &str,
        id: &Ident,
        fault: Option<Fault>,
        note: &'static str,
    ) -> Judge {
        Judge {
            subject: String::from(subject),
            root: String::from(root),
            sig: id.signature(),
            version: id.version(),
            data: id.data(),
            osabi: id.osabi(),
            abi: id.abi(),
            fault,
            note,
            ident: *id,
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

    /// 签名。
    pub const fn signature(&self) -> Signature {
        self.sig
    }

    /// 版本。
    pub const fn version(&self) -> u8 {
        self.version
    }

    /// 数据编码。
    pub const fn data(&self) -> u8 {
        self.data
    }

    /// OSABI。
    pub const fn osabi(&self) -> u8 {
        self.osabi
    }

    /// ABIVERSION。
    pub const fn abi(&self) -> u8 {
        self.abi
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

    /// 魔数偏移。
    pub fn magic_offset(&self) -> Option<usize> {
        self.ident.magic_offset()
    }

    /// 文案；可载时为空串。
    pub fn text(&self) -> String {
        match self.fault {
            Some(f) => f.message(self.subject.as_str(), self.root.as_str(), &self.ident),
            None => String::new(),
        }
    }

    /// 诊断。
    pub fn diagnose(&self) -> String {
        let mut s = String::from(self.subject.as_str());
        s.push_str(": magic=");
        s.push_str(self.ident.magic().hex().as_str());
        s.push_str(" sig=");
        s.push_str(self.sig.name());
        s.push_str(" prefix=");
        s.push_str(dec_text(self.sig.prefix() as u32).as_str());
        s.push_str(" offset=");
        match self.magic_offset() {
            Some(v) => s.push_str(dec_text(v as u32).as_str()),
            None => s.push_str("none"),
        }
        s.push_str(" version=");
        s.push_str(dec_text(self.version as u32).as_str());
        s.push_str(" data=");
        s.push_str(data_name(self.data));
        s.push_str(" osabi=");
        s.push_str(osabi_name(self.osabi));
        s.push_str(" abi=");
        s.push_str(dec_text(self.abi as u32).as_str());
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
        let mut h = fold_word(FNV_OFF, self.sig.seal());
        h = fold_word(h, self.version as u64);
        h = fold_word(h, self.data as u64);
        h = fold_word(h, self.osabi as u64);
        h = fold_word(h, self.abi as u64);
        h = fold_word(h, self.code() as u64);
        h = fold_word(h, digest(self.subject.as_bytes()));
        h = fold_word(h, digest(self.root.as_bytes()));
        h = fold_word(h, digest(self.note.as_bytes()));
        h = fold_word(h, self.ident.seal());
        h
    }
}

/// 判一份 ident。
pub fn judge_ident(name: &str, id: &Ident) -> Judge {
    if id.len() < 4 {
        return Judge::make(name, name, id, Some(Fault::Short), "short-magic");
    }
    let sig = id.signature();
    if !sig.admitted() {
        if let Signature::Displaced(_) = sig {
            return Judge::make(name, name, id, Some(Fault::Displaced), "magic-offset");
        }
        return Judge::make(name, name, id, Some(Fault::Bad), "bad-magic");
    }
    if id.len() < EI_NIDENT {
        return Judge::make(name, name, id, Some(Fault::Short), "short-ident");
    }
    if !version_ok(id.version()) {
        return Judge::make(name, name, id, Some(Fault::Version), "ident-version");
    }
    if !data_ok(id.data()) {
        return Judge::make(name, name, id, Some(Fault::Data), "ident-data");
    }
    if !osabi_ok(id.osabi()) {
        return Judge::make(name, name, id, Some(Fault::Osabi), "ident-osabi");
    }
    if !abi_ok(id.osabi(), id.abi()) {
        return Judge::make(name, name, id, Some(Fault::Abi), "ident-abi");
    }
    Judge::make(name, name, id, None, "ident-ok")
}

/// 判根对象 + 一个依赖的 ident。
pub fn judge_dep(root: &str, rid: &Ident, dep: &str, did: &Ident) -> Judge {
    let r = judge_ident(root, rid);
    if !r.accepted() {
        return r;
    }
    let d = judge_ident(dep, did);
    if d.accepted() {
        return Judge::make(root, root, rid, None, "deps-ok");
    }
    Judge::make(dep, root, did, Some(Fault::Dep), "needed-bad-magic")
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一条魔数样例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 对象名。
    pub name: &'static str,
    /// 魔数。
    pub magic: Magic,
    /// 类宽。
    pub class: u8,
    /// 数据编码。
    pub data: u8,
    /// 版本。
    pub version: u8,
    /// OSABI。
    pub osabi: u8,
    /// ABIVERSION。
    pub abi: u8,
    /// ident 实长。
    pub len: usize,
    /// 前置错位字节数。
    pub displace: usize,
    /// 依赖 soname。
    pub dep: &'static str,
    /// 依赖魔数。
    pub dep_magic: Magic,
    /// 期望根因。
    pub expect: Option<Fault>,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(
        name: &'static str,
        magic: Magic,
        class: u8,
        data: u8,
        version: u8,
        osabi: u8,
        abi: u8,
        len: usize,
        displace: usize,
        dep: &'static str,
        dep_magic: Magic,
        expect: Option<Fault>,
        note: &'static str,
    ) -> Case {
        Case {
            name,
            magic,
            class,
            data,
            version,
            osabi,
            abi,
            len,
            displace,
            dep,
            dep_magic,
            expect,
            note,
        }
    }

    /// 是否带依赖。
    pub fn has_dep(&self) -> bool {
        !self.dep.is_empty()
    }
}

/// 十四卡：四成十败。
pub const CASES: [Case; CORPUS] = [
    Case::new(
        "libc.so.6",
        Magic::good(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_SYSV,
        0,
        EI_NIDENT,
        0,
        "",
        Magic::blank(),
        None,
        "sysv-magic",
    ),
    Case::new(
        "libm.so.6",
        Magic::good(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_GNU,
        0,
        EI_NIDENT,
        0,
        "",
        Magic::blank(),
        None,
        "gnu-magic",
    ),
    Case::new(
        "./freebsd.so",
        Magic::good(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_FREEBSD,
        0,
        EI_NIDENT,
        0,
        "",
        Magic::blank(),
        None,
        "freebsd-magic",
    ),
    Case::new(
        "./be.so",
        Magic::good(),
        ELFCLASS64,
        ELFDATA2MSB,
        EV_CURRENT,
        ELFOSABI_SYSV,
        0,
        EI_NIDENT,
        0,
        "",
        Magic::blank(),
        None,
        "big-endian-ident",
    ),
    Case::new(
        "./elg.so",
        Magic::new([0x7f, b'E', b'L', b'G']),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_SYSV,
        0,
        EI_NIDENT,
        0,
        "",
        Magic::blank(),
        Some(Fault::Bad),
        "last-letter-g",
    ),
    Case::new(
        "./empty.so",
        Magic::blank(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_SYSV,
        0,
        0,
        0,
        "",
        Magic::blank(),
        Some(Fault::Short),
        "empty-file",
    ),
    Case::new(
        "./short.so",
        Magic::new([0x7f, b'E', 0, 0]),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_SYSV,
        0,
        2,
        0,
        "",
        Magic::blank(),
        Some(Fault::Short),
        "two-bytes",
    ),
    Case::new(
        "./shift.so",
        Magic::good(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_SYSV,
        0,
        EI_NIDENT,
        1,
        "",
        Magic::blank(),
        Some(Fault::Displaced),
        "magic-at-one",
    ),
    Case::new(
        "./zero.so",
        Magic::blank(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_SYSV,
        0,
        EI_NIDENT,
        0,
        "",
        Magic::blank(),
        Some(Fault::Bad),
        "zero-ident",
    ),
    Case::new(
        "./ver0.so",
        Magic::good(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_NONE,
        ELFOSABI_SYSV,
        0,
        EI_NIDENT,
        0,
        "",
        Magic::blank(),
        Some(Fault::Version),
        "version-zero",
    ),
    Case::new(
        "./data0.so",
        Magic::good(),
        ELFCLASS64,
        ELFDATANONE,
        EV_CURRENT,
        ELFOSABI_SYSV,
        0,
        EI_NIDENT,
        0,
        "",
        Magic::blank(),
        Some(Fault::Data),
        "data-zero",
    ),
    Case::new(
        "./abi7.so",
        Magic::good(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_SYSV,
        7,
        EI_NIDENT,
        0,
        "",
        Magic::blank(),
        Some(Fault::Abi),
        "abi-seven",
    ),
    Case::new(
        "./osabi.so",
        Magic::good(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        0x42,
        0,
        EI_NIDENT,
        0,
        "",
        Magic::blank(),
        Some(Fault::Osabi),
        "osabi-unknown",
    ),
    Case::new(
        "./dep.so",
        Magic::good(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_SYSV,
        0,
        EI_NIDENT,
        0,
        "libfoo.so",
        Magic::new([0x7f, b'E', b'L', b'G']),
        Some(Fault::Dep),
        "needed-bad-magic",
    ),
];

/// 取一条语料。
pub fn slot_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 造根 ident。
pub fn ident_of(c: &Case) -> Ident {
    let mut id = Ident::build(c.magic, c.class, c.data, c.version, c.osabi, c.abi, c.len);
    if c.displace > 0 {
        let _ = id.displace(c.displace);
    }
    id
}

/// 造依赖 ident。
pub fn dep_ident_of(c: &Case) -> Ident {
    Ident::build(c.dep_magic, c.class, c.data, c.version, c.osabi, c.abi, c.len)
}

/// 判一条语料。
pub fn judge_case(c: &Case, root_id: &Ident, dep_id: &Ident) -> Judge {
    if c.has_dep() {
        judge_dep(c.name, root_id, c.dep, dep_id)
    } else {
        judge_ident(c.name, root_id)
    }
}

/// 名字、备注互异；反例规格互异。
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
                let same = b.magic == a.magic
                    && b.class == a.class
                    && b.data == a.data
                    && b.version == a.version
                    && b.osabi == a.osabi
                    && b.abi == a.abi
                    && b.dep == a.dep
                    && b.dep_magic == a.dep_magic
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
        let rid = ident_of(&c);
        let did = dep_ident_of(&c);
        let j = judge_case(&c, &rid, &did);
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
    bad: usize,
    short: usize,
    displaced: usize,
    version: usize,
    data: usize,
    osabi: usize,
    abi: usize,
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
            bad: 0,
            short: 0,
            displaced: 0,
            version: 0,
            data: 0,
            osabi: 0,
            abi: 0,
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

    /// 魔数不符条数。
    pub const fn bad(&self) -> usize {
        self.bad
    }

    /// 太短条数。
    pub const fn short(&self) -> usize {
        self.short
    }

    /// 错位条数。
    pub const fn displaced(&self) -> usize {
        self.displaced
    }

    /// 版本不符条数。
    pub const fn version(&self) -> usize {
        self.version
    }

    /// 数据编码不符条数。
    pub const fn data(&self) -> usize {
        self.data
    }

    /// OSABI 不受理条数。
    pub const fn osabi(&self) -> usize {
        self.osabi
    }

    /// ABI 超界条数。
    pub const fn abi(&self) -> usize {
        self.abi
    }

    /// 依赖条数。
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
            return Err(MG_ENOBS);
        }
        if self.red {
            return Err(MG_EBAL);
        }
        if self.matched != self.total || self.total != CORPUS {
            return Err(MG_EVALUE);
        }
        if self.ok != 4 || self.fail != 10 {
            return Err(MG_EVALUE);
        }
        if self.bad != 2 || self.short != 2 || self.displaced != 1 {
            return Err(MG_EVALUE);
        }
        if self.version != 1 || self.data != 1 || self.osabi != 1 || self.abi != 1 {
            return Err(MG_EVALUE);
        }
        if self.dep != 1 {
            return Err(MG_EVALUE);
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
        let mut s = String::from("magic total=");
        s.push_str(dec_text(self.total as u32).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok as u32).as_str());
        s.push_str(" fail=");
        s.push_str(dec_text(self.fail as u32).as_str());
        s.push_str(" bad=");
        s.push_str(dec_text(self.bad as u32).as_str());
        s.push_str(" short=");
        s.push_str(dec_text(self.short as u32).as_str());
        s.push_str(" displaced=");
        s.push_str(dec_text(self.displaced as u32).as_str());
        s.push_str(" version=");
        s.push_str(dec_text(self.version as u32).as_str());
        s.push_str(" data=");
        s.push_str(dec_text(self.data as u32).as_str());
        s.push_str(" osabi=");
        s.push_str(dec_text(self.osabi as u32).as_str());
        s.push_str(" abi=");
        s.push_str(dec_text(self.abi as u32).as_str());
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
    walk(None)
}

/// 跑一遍并注入红档。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 语料行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(MG_EVALUE),
        };
        let rid = ident_of(&c);
        let did = dep_ident_of(&c);
        let j = judge_case(&c, &rid, &did);
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
                    Fault::Bad => r.bad += 1,
                    Fault::Short => r.short += 1,
                    Fault::Displaced => r.displaced += 1,
                    Fault::Version => r.version += 1,
                    Fault::Data => r.data += 1,
                    Fault::Osabi => r.osabi += 1,
                    Fault::Abi => r.abi += 1,
                    Fault::Dep => r.dep += 1,
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
            return Err(MG_ESLOT);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 前缀封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(MG_ESLOT);
    }
    let mut i = 0usize;
    while i < n {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return Err(MG_EVALUE),
        };
        let rid = ident_of(&c);
        let did = dep_ident_of(&c);
        let j = judge_case(&c, &rid, &did);
        if j.fault() != c.expect {
            return Err(MG_EVALUE);
        }
        i += 1;
    }
    Ok(())
}

/// 某条语料的错误码。
fn case_code(i: usize) -> u32 {
    match slot_at(i) {
        Some(c) => {
            let rid = ident_of(&c);
            let did = dep_ident_of(&c);
            judge_case(&c, &rid, &did).code()
        }
        None => MG_EVALUE,
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MG_ESLOT);
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
            return Err(MG_EBAL);
        }
        const VALS: [u32; 8] = [14, 4, 10, 2, 2, 1, 1, 1000];
        const CODES: [u32; 6] =
            [MG_EBAD, MG_ESHORT, MG_EDISP, MG_EVERSION, MG_EDATA, MG_EDEP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MG_EBAL),
            };
            if s.slot as usize != i {
                return Err(MG_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MG_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MG_EVALUE);
                        }
                    }
                    None => return Err(MG_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MG_EVALUE);
                        }
                    }
                    None => return Err(MG_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MG_EBAL);
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
            return Err(MG_EBAL);
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
            Err(MG_ESEAL)
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
    let _ = l.register(count_cell(3, OP_MAGIC, r.map(|x| x.bad())));
    let _ = l.register(count_cell(4, OP_MAGIC, r.map(|x| x.short())));
    let _ = l.register(count_cell(5, OP_SCAN, r.map(|x| x.displaced())));
    let _ = l.register(count_cell(6, OP_VERSION, r.map(|x| x.version())));
    let _ = l.register(count_cell(7, OP_CORPUS, r.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_MAGIC, case_code(4)));
    let _ = l.register(err_cell(9, OP_MAGIC, case_code(5)));
    let _ = l.register(err_cell(10, OP_SCAN, case_code(7)));
    let _ = l.register(err_cell(11, OP_VERSION, case_code(9)));
    let _ = l.register(err_cell(12, OP_OSABI, case_code(10)));
    let _ = l.register(err_cell(13, OP_DEP, case_code(13)));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 魔数常量与比较。
fn magic_ok() -> bool {
    let g = Magic::good();
    let a = g.is_good() && g.prefix_len() == 4 && g.mismatch().is_none() && g.mask() == 0xf;
    let b = g.hex().as_str() == "7f 45 4c 46" && g.byte(0) == Some(0x7f);
    let c = g.byte(4).is_none() && g.seal() != 0;
    let elg = Magic::new([0x7f, b'E', b'L', b'G']);
    let d = !elg.is_good() && elg.prefix_len() == 3 && elg.mismatch() == Some(3);
    let e = elg.mask() == 0x7 && elg.hex().as_str() == "7f 45 4c 47";
    let z = Magic::blank();
    let f = !z.is_good() && z.prefix_len() == 0 && z.mismatch() == Some(0) && z.mask() == 0;
    let g2 = Magic::new([0x7f, 0, 0, 0]);
    let h = g2.prefix_len() == 1 && g2.mask() == 1 && g2.mismatch() == Some(1);
    a && b && c && d && e && f && h
}

/// 签名分类。
fn classify_ok() -> bool {
    let good = [0x7f, 0x45, 0x4c, 0x46, 0, 0, 0, 0];
    let a = Signature::of(&good) == Signature::Good && Signature::of(&good).admitted();
    let elg = [0x7f, 0x45, 0x4c, 0x47];
    let b = Signature::of(&elg) == Signature::Prefix(3)
        && Signature::of(&elg).prefix() == 3
        && !Signature::of(&elg).admitted();
    let c = Signature::of(&[0x7f, 0x45]) == Signature::Short(2);
    let d = Signature::of(&[]) == Signature::Short(0);
    let e = Signature::of(&[0, 0, 0, 0, 0]) == Signature::Blank;
    let f = Signature::of(&[1, 2, 3, 4]) == Signature::Other;
    let shifted = [0u8, 0x7f, 0x45, 0x4c, 0x46, 0];
    let g = Signature::of(&shifted) == Signature::Displaced(1)
        && Signature::of(&shifted).offset() == Some(1);
    let h = Signature::Good.detail().len() > 0
        && Signature::Blank.detail().len() > 0
        && Signature::Other.detail().len() > 0;
    let i = Signature::Good.prefix() == 4 && Signature::Good.offset().is_none();
    a && b && c && d && e && f && g && h && i
}

/// 扫描诊断。
fn scan_ok() -> bool {
    let good = [0x7f, 0x45, 0x4c, 0x46, 0, 0, 0, 0];
    let a = Scan::of(&good).first() == Some(0) && Scan::of(&good).at_zero();
    let shifted = [0u8, 0x7f, 0x45, 0x4c, 0x46];
    let b = Scan::of(&shifted).first() == Some(1) && Scan::of(&shifted).displaced();
    let c = Scan::of(&[1, 2, 3]).first().is_none() && !Scan::of(&[1, 2, 3]).displaced();
    let d = Scan::of(&[]).width() == 0 && Scan::of(&[]).first_text().as_str() == "none";
    let e = Scan::of(&shifted).render().as_str() == "scan width=5 first=1 zero=0\n";
    let long = [0u8; 100];
    let f = Scan::of(&long).width() == SCAN_MAX;
    let g = Scan::of(&good).seal() != Scan::of(&shifted).seal();
    a && b && c && d && e && f && g
}

/// ident 读取。
fn ident_ok() -> bool {
    let id = Ident::build(
        Magic::good(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_CURRENT,
        ELFOSABI_SYSV,
        0,
        EI_NIDENT,
    );
    let a = id.complete() && id.len() == 16 && !id.is_empty();
    let b = id.magic().is_good() && id.signature() == Signature::Good;
    let c = id.class() == ELFCLASS64 && id.data() == ELFDATA2LSB && id.version() == EV_CURRENT;
    let d = id.osabi() == ELFOSABI_SYSV && id.abi() == 0 && id.magic_offset() == Some(0);
    let parsed = Ident::parse(id.window());
    let e = parsed == id && parsed.seal() == id.seal();
    let mut shifted = id;
    let f = shifted.displace(1) && shifted.len() == 16 && shifted.byte(0) == Some(0);
    let g = shifted.magic_offset() == Some(1) && !shifted.displace(0);
    let h = id.hex().as_str() == "7f 45 4c 46 02 01 01 00 00 00 00 00 00 00 00 00";
    let i = {
        let mut z = id;
        z.displace(1) && z.magic() == Magic::new([0, 0x7f, 0x45, 0x4c])
    };
    let short = Ident::build(Magic::good(), ELFCLASS64, ELFDATA2LSB, EV_CURRENT, 0, 0, 6);
    let j = short.len() == 6 && !short.complete() && short.window().len() == 6;
    a && b && c && d && e && f && g && h && i && j
}

/// 版本闸门。
fn version_ok_check() -> bool {
    let a = version_ok(EV_CURRENT) && !version_ok(EV_NONE) && !version_ok(EV_NUM);
    let id = Ident::build(
        Magic::good(),
        ELFCLASS64,
        ELFDATA2LSB,
        EV_NONE,
        ELFOSABI_SYSV,
        0,
        EI_NIDENT,
    );
    let b = judge_ident("./ver.so", &id).code() == MG_EVERSION;
    let c = judge_ident("./ver.so", &id).fault() == Some(Fault::Version);
    let d = Fault::Version.magic_family() == false;
    let e = Fault::Version.name().len() > 0;
    a && b && c && d && e
}

/// OSABI 与 ABI。
fn osabi_ok_check() -> bool {
    let a = osabi_ok(ELFOSABI_SYSV) && osabi_ok(ELFOSABI_GNU) && osabi_ok(ELFOSABI_FREEBSD);
    let b = !osabi_ok(ELFOSABI_HPUX) && !osabi_ok(ELFOSABI_ARM) && !osabi_ok(0x42);
    let c = abi_ok(ELFOSABI_SYSV, 0) && !abi_ok(ELFOSABI_SYSV, 1);
    let d = abi_ok(ELFOSABI_GNU, 0) && !abi_ok(ELFOSABI_GNU, 2);
    let e = abi_ok(ELFOSABI_FREEBSD, 0) && abi_ok(ELFOSABI_FREEBSD, 2);
    let f = !abi_ok(ELFOSABI_FREEBSD, 3) && !abi_ok(0x42, 0);
    let g = osabi_name(ELFOSABI_GNU) == "ELFOSABI_GNU"
        && osabi_name(ELFOSABI_FREEBSD) == "ELFOSABI_FREEBSD"
        && osabi_name(0x42) == "ELFOSABI_UNKNOWN";
    let h = data_ok(ELFDATA2LSB) && data_ok(ELFDATA2MSB) && !data_ok(ELFDATANONE);
    let i = data_name(ELFDATA2LSB) == "ELFDATA2LSB" && data_name(ELFDATANONE) == "ELFDATANONE";
    a && b && c && d && e && f && g && h && i
}

/// 文案逐字。
fn text_lit_ok() -> bool {
    let t = |i: usize| -> String {
        match slot_at(i) {
            Some(c) => {
                let rid = ident_of(&c);
                let did = dep_ident_of(&c);
                judge_case(&c, &rid, &did).text()
            }
            None => String::new(),
        }
    };
    let a = t(4).as_str() == "./elg.so: invalid ELF header";
    let b = t(5).as_str() == "./empty.so: file too short";
    let c = t(7).as_str() == "./shift.so: invalid ELF header: ELF magic at offset 1";
    let d = t(9).as_str() == "./ver0.so: invalid ELF header: e_ident[EI_VERSION]=0";
    let e = t(10).as_str() == "./data0.so: invalid ELF header: e_ident[EI_DATA]=0";
    let f = t(11).as_str() == "./abi7.so: invalid ELF header: e_ident[EI_ABIVERSION]=7";
    let g = t(12).as_str() == "./osabi.so: invalid ELF header: e_ident[EI_OSABI]=66";
    let h = t(13).as_str() == "libfoo.so: invalid ELF header (required by ./dep.so)";
    let i = t(0).is_empty() && t(1).is_empty() && t(2).is_empty() && t(3).is_empty();
    a && b && c && d && e && f && g && h && i
}

/// 错位魔数。
fn displaced_ok() -> bool {
    let c = match slot_at(7) {
        Some(v) => v,
        None => return false,
    };
    let id = ident_of(&c);
    let j = judge_case(&c, &id, &dep_ident_of(&c));
    let a = j.code() == MG_EDISP && j.fault() == Some(Fault::Displaced);
    let b = j.magic_offset() == Some(1) && j.signature() == Signature::Displaced(1);
    let c2 = match slot_at(0) {
        Some(v) => v,
        None => return false,
    };
    let i2 = ident_of(&c2);
    let d = judge_case(&c2, &i2, &dep_ident_of(&c2)).accepted();
    let e = judge_ident("./x.so", &i2).magic_offset() == Some(0);
    let f = Fault::Displaced.magic_family() && !Fault::Version.magic_family();
    a && b && d && e && f
}

/// 依赖连坐。
fn dep_ok() -> bool {
    let c = match slot_at(13) {
        Some(v) => v,
        None => return false,
    };
    let rid = ident_of(&c);
    let did = dep_ident_of(&c);
    let j = judge_case(&c, &rid, &did);
    let a = j.code() == MG_EDEP && j.is_dep();
    let b = j.subject() == "libfoo.so" && j.root() == "./dep.so";
    let c2 = match slot_at(0) {
        Some(v) => v,
        None => return false,
    };
    let r2 = ident_of(&c2);
    let e = judge_dep("./root.so", &r2, "libc.so.6", &r2).accepted();
    let f = judge_dep("./root.so", &r2, "libfoo.so", &did).code() == MG_EDEP;
    a && b && e && f
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.ok() == 4 && r.fail() == 10 && r.matched() == CORPUS;
    let b = r.bad() == 2 && r.short() == 2 && r.displaced() == 1;
    let c = r.version() == 1 && r.data() == 1 && r.osabi() == 1 && r.abi() == 1;
    let d = r.dep() == 1 && r.rate() == 1000 && r.green() && corpus_table_ok();
    a && b && c && d
}

/// 语料槽位。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(c) => c.expect.is_none() && c.magic.is_good(),
        None => false,
    };
    let b = match slot_at(4) {
        Some(c) => c.magic.prefix_len() == 3 && c.expect == Some(Fault::Bad),
        None => false,
    };
    let c = match slot_at(7) {
        Some(v) => v.displace == 1 && v.expect == Some(Fault::Displaced),
        None => false,
    };
    let d = match slot_at(13) {
        Some(v) => v.has_dep() && !v.dep_magic.is_good(),
        None => false,
    };
    let e = slot_at(CORPUS).is_none();
    let f = match slot_at(3) {
        Some(v) => v.data == ELFDATA2MSB && v.expect.is_none(),
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
        == "magic total=14 ok=4 fail=10 bad=2 short=2 displaced=1 version=1 data=1 osabi=1 abi=1 dep=1 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(MG_ENOBS);
    let c = e.render().as_str()
        == "magic total=0 ok=0 fail=0 bad=0 short=0 displaced=0 version=0 data=0 osabi=0 abi=0 dep=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, MG_EBAD))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929 && r.red();
    let b = r.gate() == Err(MG_EBAL) && !r.green();
    let c = run_with(Some((CORPUS, MG_EBAD))).err() == Some(MG_ESLOT);
    let d = run_with(Some((CORPUS + 1, MG_EBAD))).err() == Some(MG_ESLOT);
    a && b && c && d
}

/// 前缀封面。
fn cover_ok() -> bool {
    let a = cover(0).is_ok() && cover(1).is_ok() && cover(10).is_ok();
    let b = cover(CORPUS).is_ok() && cover(CORPUS + 1).err() == Some(MG_ESLOT);
    let mut i = 0usize;
    while i <= CORPUS {
        if cover(i).is_err() {
            return false;
        }
        i += 1;
    }
    a && b
}

/// 指纹与文本。
fn hash_ok() -> bool {
    let good = Magic::good();
    let elg = Magic::new([0x7f, b'E', b'L', b'G']);
    let a = good.seal() != elg.seal();
    let b = digest(b"\x7fELF") != digest(b"\x7fELG");
    let c = Signature::Good.seal() != Signature::Displaced(1).seal();
    let id = Ident::build(good, ELFCLASS64, ELFDATA2LSB, EV_CURRENT, ELFOSABI_SYSV, 0, 16);
    let mut sh = id;
    let d = sh.displace(1) && sh.seal() != id.seal();
    let e = Scan::of(id.window()).seal() != Scan::of(sh.window()).seal();
    let f = judge_ident("./id.so", &id).seal() != judge_ident("./id.so", &sh).seal();
    let g = judge_ident("./id.so", &id).diagnose().as_str()
        == "./id.so: magic=7f 45 4c 46 sig=good prefix=4 offset=0 version=1 data=ELFDATA2LSB osabi=ELFOSABI_SYSV abi=0 fault=none note=ident-ok\n";
    let h = judge_ident("./id.so", &id).diagnose().starts_with("./id.so: magic=7f 45 4c 46");
    a && b && c && d && e && f && g && h
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
        == Err(MG_ESLOT);
    let h = m.rejects() == 1 && m.freeze() == Err(MG_EBAL) && m.rejects() == 2;
    let i = m.audit() == Ok(());
    a && b && c && d && e && f && g && h && i
}

/// 四注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = magic_ok() && classify_ok() && scan_ok();
    out[1] = ident_ok() && version_ok_check() && osabi_ok_check();
    out[2] = text_lit_ok() && displaced_ok() && dep_ok();
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
pub fn run_unx11625_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11625.so_magic");

    s.add("MG01-魔数常量与比较", magic_ok(), "static");
    s.add("MG02-签名分类", classify_ok(), "static");
    s.add("MG03-扫描诊断", scan_ok(), "static");
    s.add("MG04-ident字段", ident_ok(), "static");
    s.add("MG05-版本闸门", version_ok_check(), "static");
    s.add("MG06-OSABI与ABI", osabi_ok_check(), "static");
    s.add("MG07-文案逐字", text_lit_ok(), "static");
    s.add("MG08-错位魔数", displaced_ok(), "static");
    s.add("MG09-依赖连坐", dep_ok(), "static");
    s.add("MG10-十四卡全跑", corpus_ok(), "static");
    s.add("MG11-语料与渲染", slots_ok() && report_ok(), "static");
    s.add("MG12-红档封面与指纹", reds_ok() && cover_ok() && hash_ok(), "static");
    s.add("MG13-冻结重放四注入", inject_four() == [true; 4], "static");

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
        assert_eq!(l.register(Sample::blank()), Err(MG_ESLOT));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true; 4]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11625_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn magic_compare() {
        let g = Magic::good();
        assert!(g.is_good());
        assert_eq!(g.prefix_len(), 4);
        assert_eq!(g.mismatch(), None);
        assert_eq!(g.mask(), 0xf);
        assert_eq!(g.hex().as_str(), "7f 45 4c 46");
        let elg = Magic::new([0x7f, b'E', b'L', b'G']);
        assert_eq!(elg.prefix_len(), 3);
        assert_eq!(elg.mismatch(), Some(3));
        assert_eq!(elg.mask(), 0x7);
        assert_eq!(elg.hex().as_str(), "7f 45 4c 47");
        assert!(!elg.is_good());
    }

    #[test]
    fn magic_blank() {
        let z = Magic::blank();
        assert!(!z.is_good());
        assert_eq!(z.prefix_len(), 0);
        assert_eq!(z.mismatch(), Some(0));
        assert_eq!(z.mask(), 0);
        assert_eq!(z.byte(0), Some(0));
        assert_eq!(z.byte(4), None);
    }

    #[test]
    fn signatures() {
        assert_eq!(Signature::of(&[0x7f, 0x45, 0x4c, 0x46]), Signature::Good);
        assert_eq!(Signature::of(&[0x7f, 0x45]), Signature::Short(2));
        assert_eq!(Signature::of(&[0, 0, 0, 0]), Signature::Blank);
        assert_eq!(Signature::of(&[0x7f, 0x45, 0x4c, 0x47]), Signature::Prefix(3));
        assert_eq!(Signature::of(&[1, 2, 3, 4]), Signature::Other);
        assert_eq!(Signature::of(&[0, 0x7f, 0x45, 0x4c, 0x46]), Signature::Displaced(1));
    }

    #[test]
    fn signature_meta() {
        assert_eq!(Signature::Good.name(), "good");
        assert!(Signature::Good.admitted());
        assert!(!Signature::Prefix(2).admitted());
        assert_eq!(Signature::Displaced(4).offset(), Some(4));
        assert_eq!(Signature::Good.prefix(), 4);
        assert!(Signature::Blank.detail().len() > 0);
    }

    #[test]
    fn magic_scan() {
        let good = [0x7f, 0x45, 0x4c, 0x46, 0, 0];
        let s = Scan::of(&good);
        assert_eq!(s.first(), Some(0));
        assert!(s.at_zero());
        assert!(!s.displaced());
        assert_eq!(s.render().as_str(), "scan width=6 first=0 zero=1\n");
        let shifted = [0u8, 0x7f, 0x45, 0x4c, 0x46];
        assert_eq!(Scan::of(&shifted).first(), Some(1));
        assert!(Scan::of(&shifted).displaced());
        assert_eq!(Scan::of(&[1, 2, 3]).first_text().as_str(), "none");
    }

    #[test]
    fn scan_width_limit() {
        let long = [0u8; 100];
        assert_eq!(Scan::of(&long).width(), SCAN_MAX);
        assert_eq!(Scan::of(&[]).width(), 0);
    }

    #[test]
    fn ident_build() {
        let id = Ident::build(
            Magic::good(),
            ELFCLASS64,
            ELFDATA2LSB,
            EV_CURRENT,
            ELFOSABI_GNU,
            0,
            EI_NIDENT,
        );
        assert!(id.complete());
        assert_eq!(id.len(), 16);
        assert_eq!(id.magic().bytes(), Magic::good().bytes());
        assert_eq!(id.version(), EV_CURRENT);
        assert_eq!(id.osabi(), ELFOSABI_GNU);
        assert_eq!(id.abi(), 0);
        assert_eq!(id.magic_offset(), Some(0));
    }

    #[test]
    fn ident_parse_and_displace() {
        let id = Ident::build(Magic::good(), ELFCLASS64, ELFDATA2LSB, 1, 0, 0, 16);
        let parsed = Ident::parse(id.window());
        assert_eq!(parsed.seal(), id.seal());
        let mut sh = id;
        assert!(sh.displace(1));
        assert_eq!(sh.byte(0), Some(0));
        assert_eq!(sh.magic_offset(), Some(1));
        assert_eq!(sh.magic(), Magic::new([0, 0x7f, 0x45, 0x4c]));
        assert!(!sh.displace(0));
    }

    #[test]
    fn ident_short_and_hex() {
        let id = Ident::build(Magic::good(), ELFCLASS64, ELFDATA2LSB, 1, 0, 0, 6);
        assert_eq!(id.len(), 6);
        assert!(!id.complete());
        assert!(id.window().len() == 6);
        let full = Ident::build(Magic::good(), ELFCLASS64, ELFDATA2LSB, 1, 0, 0, 16);
        assert_eq!(
            full.hex().as_str(),
            "7f 45 4c 46 02 01 01 00 00 00 00 00 00 00 00 00"
        );
    }

    #[test]
    fn version_gate() {
        assert!(version_ok(EV_CURRENT));
        assert!(!version_ok(EV_NONE));
        let id = Ident::build(Magic::good(), ELFCLASS64, ELFDATA2LSB, EV_NONE, 0, 0, 16);
        assert_eq!(judge_ident("./v.so", &id).code(), MG_EVERSION);
    }

    #[test]
    fn osabi_and_abi() {
        assert!(osabi_ok(ELFOSABI_SYSV));
        assert!(osabi_ok(ELFOSABI_GNU));
        assert!(osabi_ok(ELFOSABI_FREEBSD));
        assert!(!osabi_ok(0x42));
        assert!(abi_ok(ELFOSABI_SYSV, 0));
        assert!(!abi_ok(ELFOSABI_SYSV, 7));
        assert!(abi_ok(ELFOSABI_FREEBSD, 2));
        assert!(!abi_ok(ELFOSABI_FREEBSD, 3));
        assert_eq!(osabi_name(ELFOSABI_GNU), "ELFOSABI_GNU");
    }

    #[test]
    fn data_gate() {
        assert!(data_ok(ELFDATA2LSB));
        assert!(data_ok(ELFDATA2MSB));
        assert!(!data_ok(ELFDATANONE));
        let id = Ident::build(Magic::good(), ELFCLASS64, ELFDATANONE, 1, 0, 0, 16);
        assert_eq!(judge_ident("./d.so", &id).code(), MG_EDATA);
    }

    #[test]
    fn bad_magic_text() {
        let id = Ident::build(
            Magic::new([0x7f, b'E', b'L', b'G']),
            ELFCLASS64,
            ELFDATA2LSB,
            1,
            0,
            0,
            16,
        );
        let j = judge_ident("./elg.so", &id);
        assert_eq!(j.code(), MG_EBAD);
        assert_eq!(j.text().as_str(), "./elg.so: invalid ELF header");
        assert!(!j.accepted());
    }

    #[test]
    fn short_magic_text() {
        let id = Ident::build(Magic::blank(), ELFCLASS64, 1, 1, 0, 0, 2);
        let j = judge_ident("./short.so", &id);
        assert_eq!(j.code(), MG_ESHORT);
        assert_eq!(j.text().as_str(), "./short.so: file too short");
    }

    #[test]
    fn displaced_text() {
        let mut id = Ident::build(Magic::good(), ELFCLASS64, 1, 1, 0, 0, 16);
        assert!(id.displace(1));
        let j = judge_ident("./shift.so", &id);
        assert_eq!(j.code(), MG_EDISP);
        assert_eq!(
            j.text().as_str(),
            "./shift.so: invalid ELF header: ELF magic at offset 1"
        );
        assert_eq!(j.magic_offset(), Some(1));
    }

    #[test]
    fn osabi_text() {
        let id = Ident::build(Magic::good(), ELFCLASS64, 1, 1, 0x42, 0, 16);
        let j = judge_ident("./osabi.so", &id);
        assert_eq!(j.code(), MG_EOSABI);
        assert_eq!(
            j.text().as_str(),
            "./osabi.so: invalid ELF header: e_ident[EI_OSABI]=66"
        );
    }

    #[test]
    fn abi_text() {
        let id = Ident::build(Magic::good(), ELFCLASS64, 1, 1, ELFOSABI_SYSV, 7, 16);
        let j = judge_ident("./abi7.so", &id);
        assert_eq!(j.code(), MG_EABI);
        assert_eq!(
            j.text().as_str(),
            "./abi7.so: invalid ELF header: e_ident[EI_ABIVERSION]=7"
        );
    }

    #[test]
    fn dep_text() {
        let rid = Ident::build(Magic::good(), ELFCLASS64, 1, 1, 0, 0, 16);
        let did = Ident::build(Magic::new([0x7f, b'E', b'L', b'G']), ELFCLASS64, 1, 1, 0, 0, 16);
        let j = judge_dep("./dep.so", &rid, "libfoo.so", &did);
        assert_eq!(j.code(), MG_EDEP);
        assert!(j.is_dep());
        assert_eq!(
            j.text().as_str(),
            "libfoo.so: invalid ELF header (required by ./dep.so)"
        );
    }

    #[test]
    fn corpus_shape() {
        assert!(unique());
        assert!(notes_ok());
        assert_eq!(CASES.len(), 14);
        assert_eq!(slot_at(0).map(|c| c.magic), Some(Magic::good()));
        assert_eq!(slot_at(7).map(|c| c.displace), Some(1));
        assert!(slot_at(14).is_none());
    }

    #[test]
    fn corpus_walk() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.total(), 14);
        assert_eq!(r.ok(), 4);
        assert_eq!(r.fail(), 10);
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
            "magic total=14 ok=4 fail=10 bad=2 short=2 displaced=1 version=1 data=1 osabi=1 abi=1 dep=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn cover_prefix() {
        assert!(cover(0).is_ok());
        assert!(cover(14).is_ok());
        assert_eq!(cover(15), Err(MG_ESLOT));
    }

    #[test]
    fn diagnose_shape() {
        let id = Ident::build(Magic::good(), ELFCLASS64, 1, 1, 0, 0, 16);
        let d = judge_ident("id.so", &id).diagnose();
        assert!(d.starts_with("id.so: magic=7f 45 4c 46"));
        assert!(d.contains("sig=good"));
        assert!(d.ends_with('\n'));
    }
}
