//! UNX-F11616 坏 ELF 探针发生成器（探针资产面）
//!
//! 镜面：前十五单判的是「抓到坏件之后怎么拒」。本单把坏件本身做出来——
//! 一台纯函数式的探针生成器，按缺陷算子写出真实的 ELF64 字节流，再用
//! 一台独立的屏幕器把字节流读回来分类。生成与判读不复用同一份状态，
//! 写出去什么样、读回来哪一类，必须逐槽对上。
//!
//! 生成面固定为「头 + 三程序头 + 三段体」的规范件：
//!
//! 1. 标识 `\x7fELF`、EI_CLASS=2、EI_DATA=1、EI_VERSION=1；
//! 2. `e_type ∈ {ET_EXEC, ET_DYN}`、`e_machine=EM_X86_64`、`e_version=1`；
//! 3. `e_phoff=0x40`、`e_phentsize=56`、`e_phnum=3`；
//! 4. 三段 PT_LOAD 各自页对齐且同偏置 0x400000；
//! 5. 段体分别落在文件 0x0000/0x1000/0x2000，文件长 0x2100。
//!
//! 缺陷算子在规范件上做单点破坏：标识、类别、类型、程序头偏移、程序头
//! 表项大小、内存小于文件、段越界、对齐非幂、地址不同余。屏幕器按固定
//! 次序读回：字节长 → 标识 → 类别 → 类型 → 表项大小 → 表界 → 逐段。
//!
//! 语料十四槽，八活六死——标识二、越界一、表项一、段面二。
//!
//! 闸门次序：名称 → 字节读写 → 规范件 → 缺陷算子 → 屏幕判面 → 页算术 →
//! 语料 → 汇总 → 红单 → 文案 → 校验器 → 冻结重放。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfbf0;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 探针文件长。
pub const LEN: usize = 0x2100;

/// ELF 头长。
pub const EHDR: usize = 64;

/// 程序头表项长。
pub const PHENT: usize = 56;

/// 规范程序头数。
pub const PHNUM: u16 = 3;

/// 规范程序头偏移。
pub const PHOFF: u64 = 0x40;

/// 页字节。
pub const PAGE: u64 = 0x1000;

/// 页内掩码。
pub const PMASK: u64 = PAGE - 1;

/// 64 位类别。
pub const ELFCLASS64: u8 = 2;

/// 小端数据编码。
pub const ELFDATA2LSB: u8 = 1;

/// 当前版本。
pub const EV_CURRENT: u8 = 1;

/// 可执行文件。
pub const ET_EXEC: u16 = 2;

/// 共享对象 / PIE。
pub const ET_DYN: u16 = 3;

/// 可重定位目标。
pub const ET_REL: u16 = 1;

/// x86-64。
pub const EM_X86_64: u16 = 62;

/// 装载段。
pub const PT_LOAD: u32 = 1;

/// R。
pub const PF_R: u32 = 4;

/// W。
pub const PF_W: u32 = 2;

/// X。
pub const PF_X: u32 = 1;

/// RX。
pub const PF_RX: u32 = PF_R | PF_X;

/// RW。
pub const PF_RW: u32 = PF_R | PF_W;

/// ENOEXEC。
pub const ENOEXEC: i32 = 8;

/// EINVAL。
pub const EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 标识坏。
pub const BP_EMAGIC: u32 = SEG_BASE + 0x01;
/// 表越界。
pub const BP_EBOUNDS: u32 = SEG_BASE + 0x02;
/// 表项坏。
pub const BP_EPHDR: u32 = SEG_BASE + 0x03;
/// 段面坏。
pub const BP_ESEG: u32 = SEG_BASE + 0x04;
/// 同余坏。
pub const BP_ECONG: u32 = SEG_BASE + 0x05;
/// 类型坏。
pub const BP_ETYPE: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const BP_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const BP_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const BP_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const BP_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const BP_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const BP_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const BP_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 生成。
pub const OP_FORGE: u32 = 1;
/// 屏幕。
pub const OP_SCREEN: u32 = 2;
/// 页算术。
pub const OP_PAGE: u32 = 3;
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

/// 十六进制文本（带 `0x`，小写）。
pub fn hex_text(v: u64) -> String {
    let mut buf = [0u8; 16];
    let mut n = v;
    let mut i = 16usize;
    if n == 0 {
        return String::from("0x0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        let d = (n & 0xf) as u8;
        buf[i] = if d < 10 { b'0' + d } else { b'a' + (d - 10) };
        n >>= 4;
    }
    let mut s = String::from("0x");
    while i < 16 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 字节读写
// ---------------------------------------------------------------------------

/// 读一字节。
pub fn rd8(b: &[u8], o: usize) -> Option<u8> {
    b.get(o).copied()
}

/// 读两字节（小端）。
pub fn rd16(b: &[u8], o: usize) -> Option<u16> {
    let lo = rd8(b, o)? as u16;
    let hi = rd8(b, o.checked_add(1)?)? as u16;
    Some(lo | (hi << 8))
}

/// 读四字节（小端）。
pub fn rd32(b: &[u8], o: usize) -> Option<u32> {
    let mut v = 0u32;
    let mut i = 0usize;
    while i < 4 {
        let byte = rd8(b, o.checked_add(i)?)? as u32;
        v |= byte << (i * 8);
        i += 1;
    }
    Some(v)
}

/// 读八字节（小端）。
pub fn rd64(b: &[u8], o: usize) -> Option<u64> {
    let mut v = 0u64;
    let mut i = 0usize;
    while i < 8 {
        let byte = rd8(b, o.checked_add(i)?)? as u64;
        v |= byte << (i * 8);
        i += 1;
    }
    Some(v)
}

/// 写一字节。
pub fn wr8(b: &mut [u8], o: usize, v: u8) {
    if let Some(x) = b.get_mut(o) {
        *x = v;
    }
}

/// 写两字节（小端）。
pub fn wr16(b: &mut [u8], o: usize, v: u16) {
    wr8(b, o, (v & 0xff) as u8);
    if let Some(o2) = o.checked_add(1) {
        wr8(b, o2, (v >> 8) as u8);
    }
}

/// 写四字节（小端）。
pub fn wr32(b: &mut [u8], o: usize, v: u32) {
    let mut i = 0usize;
    while i < 4 {
        match o.checked_add(i) {
            Some(k) => wr8(b, k, ((v >> (i * 8)) & 0xff) as u8),
            None => return,
        }
        i += 1;
    }
}

/// 写八字节（小端）。
pub fn wr64(b: &mut [u8], o: usize, v: u64) {
    let mut i = 0usize;
    while i < 8 {
        match o.checked_add(i) {
            Some(k) => wr8(b, k, ((v >> (i * 8)) & 0xff) as u8),
            None => return,
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// 页算术与同余
// ---------------------------------------------------------------------------

/// 向下取页。
pub const fn page_floor(v: u64) -> u64 {
    v & !PMASK
}

/// 向上取页（0 仍为 0）。
pub const fn page_ceil(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    let lo = page_floor(v);
    if lo == v {
        v
    } else {
        match lo.checked_add(PAGE) {
            Some(x) => x,
            None => u64::MAX,
        }
    }
}

/// 段偏置。
pub const fn bias_of(vaddr: u64, off: u64) -> u64 {
    page_floor(vaddr).wrapping_sub(page_floor(off))
}

/// 是否 2 的幂（0 与 1 放行）。
pub const fn page_power(align: u64) -> bool {
    align <= 1 || (align & (align - 1)) == 0
}

/// 地址与偏移是否按 `align` 同余。
pub const fn congruent(off: u64, vaddr: u64, align: u64) -> bool {
    if align <= 1 {
        return true;
    }
    off % align == vaddr % align
}

// ---------------------------------------------------------------------------
// 缺陷算子
// ---------------------------------------------------------------------------

/// 生成缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defect {
    /// 规范件。
    None,
    /// 标识首字节坏。
    Magic,
    /// EI_CLASS 退为 32 位。
    Class,
    /// e_type 退为 ET_REL。
    Type,
    /// 程序头偏移越界。
    Phoff,
    /// 表项大小错。
    Phent,
    /// 文件字节大于内存字节。
    Overflow,
    /// 段越出文件。
    Trunc,
    /// 对齐非 2 的幂。
    Align,
    /// 地址与偏移不同余。
    Cong,
    /// 程序头数归零。
    Bare,
    /// 退为 PIE。
    Pie,
    /// 只留一段。
    Single,
    /// 对齐归零。
    AlignZero,
    /// 对齐归一。
    AlignOne,
    /// 对齐抬到 2 MiB。
    TwoMeg,
    /// 纯 BSS 段。
    Bss,
}

impl Defect {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Defect::None => "none",
            Defect::Magic => "magic",
            Defect::Class => "class",
            Defect::Type => "type",
            Defect::Phoff => "phoff",
            Defect::Phent => "phent",
            Defect::Overflow => "overflow",
            Defect::Trunc => "trunc",
            Defect::Align => "align",
            Defect::Cong => "cong",
            Defect::Bare => "bare",
            Defect::Pie => "pie",
            Defect::Single => "single",
            Defect::AlignZero => "align-zero",
            Defect::AlignOne => "align-one",
            Defect::TwoMeg => "two-meg",
            Defect::Bss => "bss",
        }
    }

    /// 屏幕器期望判面。
    pub const fn expect(&self) -> Fault {
        match self {
            Defect::None
            | Defect::Bare
            | Defect::Pie
            | Defect::Single
            | Defect::AlignZero
            | Defect::AlignOne
            | Defect::TwoMeg
            | Defect::Bss => Fault::Ok,
            Defect::Magic | Defect::Class => Fault::Magic,
            Defect::Type => Fault::Type,
            Defect::Phoff => Fault::Bounds,
            Defect::Phent => Fault::Phdr,
            Defect::Overflow | Defect::Trunc | Defect::Align => Fault::Seg,
            Defect::Cong => Fault::Cong,
        }
    }

    /// 是否活档。
    pub const fn alive(&self) -> bool {
        matches!(self.expect(), Fault::Ok)
    }
}

// ---------------------------------------------------------------------------
// 探针
// ---------------------------------------------------------------------------

/// 一段探针资产。
#[derive(Clone)]
pub struct Probe {
    /// 字节。
    pub bytes: [u8; LEN],
    /// 有效长度。
    pub len: usize,
}

impl Probe {
    /// 空件。
    pub const fn empty() -> Probe {
        Probe { bytes: [0u8; LEN], len: 0 }
    }

    /// 切片。
    pub fn as_slice(&self) -> &[u8] {
        match self.bytes.get(..self.len) {
            Some(s) => s,
            None => &[],
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.len as u64);
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bytes.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        h
    }
}

/// 写标识。
fn ident(b: &mut [u8], class: u8) {
    wr8(b, 0, 0x7f);
    wr8(b, 1, b'E');
    wr8(b, 2, b'L');
    wr8(b, 3, b'F');
    wr8(b, 4, class);
    wr8(b, 5, ELFDATA2LSB);
    wr8(b, 6, EV_CURRENT);
}

/// 写 ELF 头。
fn head(b: &mut [u8], etype: u16, phoff: u64, phentsize: u16, phnum: u16) {
    wr16(b, 16, etype);
    wr16(b, 18, EM_X86_64);
    wr32(b, 20, 1);
    wr64(b, 24, 0);
    wr64(b, 32, phoff);
    wr64(b, 40, 0);
    wr32(b, 48, 0);
    wr16(b, 52, EHDR as u16);
    wr16(b, 54, phentsize);
    wr16(b, 56, phnum);
    wr16(b, 58, 64);
    wr16(b, 60, 0);
    wr16(b, 62, 0);
}

/// 写一个程序头。
fn seg(
    b: &mut [u8],
    at: usize,
    ptype: u32,
    off: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    flags: u32,
    align: u64,
) {
    wr32(b, at, ptype);
    wr32(b, at + 4, flags);
    wr64(b, at + 8, off);
    wr64(b, at + 16, vaddr);
    wr64(b, at + 24, vaddr);
    wr64(b, at + 32, filesz);
    wr64(b, at + 40, memsz);
    wr64(b, at + 48, align);
}

/// 规范件骨架：头 + 三段。
fn skeleton() -> Probe {
    let mut p = Probe::empty();
    p.len = LEN;
    ident(&mut p.bytes, ELFCLASS64);
    head(&mut p.bytes, ET_EXEC, PHOFF, PHENT as u16, PHNUM);
    seg(&mut p.bytes, 0x40, PT_LOAD, 0, 0x400000, 0x40, 0x40, PF_RX, PAGE);
    seg(&mut p.bytes, 0x40 + PHENT, PT_LOAD, 0x1000, 0x401000, 0x100, 0x100, PF_R, PAGE);
    seg(&mut p.bytes, 0x40 + 2 * PHENT, PT_LOAD, 0x2000, 0x402000, 0x100, 0x100, PF_RW, PAGE);
    p
}

/// 按缺陷生成探针。
pub fn emit(d: Defect) -> Probe {
    let mut p = skeleton();
    match d {
        Defect::None => {}
        Defect::Magic => wr8(&mut p.bytes, 0, 0x7e),
        Defect::Class => wr8(&mut p.bytes, 4, 1),
        Defect::Type => wr16(&mut p.bytes, 16, ET_REL),
        Defect::Phoff => wr64(&mut p.bytes, 32, LEN as u64 + 0x100),
        Defect::Phent => wr16(&mut p.bytes, 54, (PHENT - 1) as u16),
        Defect::Overflow => {
            seg(&mut p.bytes, 0x40, PT_LOAD, 0, 0x400000, 0x80, 0x40, PF_RX, PAGE);
        }
        Defect::Trunc => {
            seg(
                &mut p.bytes,
                0x40,
                PT_LOAD,
                0x100,
                0x400100,
                LEN as u64,
                LEN as u64,
                PF_RX,
                PAGE,
            );
        }
        Defect::Align => {
            seg(&mut p.bytes, 0x40, PT_LOAD, 0, 0x400000, 0x40, 0x40, PF_RX, 0x1800);
        }
        Defect::Cong => {
            seg(&mut p.bytes, 0x40, PT_LOAD, 0, 0x400800, 0x40, 0x40, PF_RX, PAGE);
        }
        Defect::Bare => wr16(&mut p.bytes, 56, 0),
        Defect::Pie => wr16(&mut p.bytes, 16, ET_DYN),
        Defect::Single => wr16(&mut p.bytes, 56, 1),
        Defect::AlignZero => {
            let mut at = 0x40usize;
            while at < 0x40 + 3 * PHENT {
                wr64(&mut p.bytes, at + 48, 0);
                at += PHENT;
            }
        }
        Defect::AlignOne => {
            let mut at = 0x40usize;
            while at < 0x40 + 3 * PHENT {
                wr64(&mut p.bytes, at + 48, 1);
                at += PHENT;
            }
        }
        Defect::TwoMeg => {
            let mut at = 0x40usize;
            while at < 0x40 + 3 * PHENT {
                wr64(&mut p.bytes, at + 48, 0x20_0000);
                at += PHENT;
            }
        }
        Defect::Bss => {
            let at = 0x40 + PHENT;
            seg(&mut p.bytes, at, PT_LOAD, 0x1000, 0x401000, 0, 0x100, PF_RW, PAGE);
        }
    }
    p
}

// ---------------------------------------------------------------------------
// 屏幕器
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 通过。
    Ok,
    /// 标识坏。
    Magic,
    /// 类型坏。
    Type,
    /// 表越界。
    Bounds,
    /// 表项坏。
    Phdr,
    /// 段面坏。
    Seg,
    /// 同余坏。
    Cong,
}

impl Fault {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Magic => "magic",
            Fault::Type => "type",
            Fault::Bounds => "bounds",
            Fault::Phdr => "phdr",
            Fault::Seg => "seg",
            Fault::Cong => "cong",
        }
    }

    /// 错误码。
    pub const fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Magic => BP_EMAGIC,
            Fault::Type => BP_ETYPE,
            Fault::Bounds => BP_EBOUNDS,
            Fault::Phdr => BP_EPHDR,
            Fault::Seg => BP_ESEG,
            Fault::Cong => BP_ECONG,
        }
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        match self {
            Fault::Ok => 0,
            Fault::Magic | Fault::Type | Fault::Seg | Fault::Cong => ENOEXEC,
            _ => EINVAL,
        }
    }

    /// 是否拒收面。
    pub const fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 屏幕摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    /// 程序头数。
    pub phnum: u16,
    /// 装载段数。
    pub load: u32,
    /// 表末。
    pub span_end: u64,
    /// 统一偏置。
    pub bias: u64,
}

impl Summary {
    /// 空摘要。
    pub const fn blank() -> Summary {
        Summary { phnum: 0, load: 0, span_end: 0, bias: 0 }
    }
}

/// 读单个程序的字段包。
struct Fields {
    ptype: u32,
    off: u64,
    vaddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

/// 读一个程序头。
fn read_phdr(b: &[u8], at: usize) -> Option<Fields> {
    Some(Fields {
        ptype: rd32(b, at)?,
        off: rd64(b, at.checked_add(8)?)?,
        vaddr: rd64(b, at.checked_add(16)?)?,
        filesz: rd64(b, at.checked_add(32)?)?,
        memsz: rd64(b, at.checked_add(40)?)?,
        align: rd64(b, at.checked_add(48)?)?,
    })
}

/// 屏幕：把字节流读回来分类。
pub fn screen(b: &[u8]) -> Result<Summary, Fault> {
    if b.len() < EHDR {
        return Err(Fault::Bounds);
    }
    if rd8(b, 0) != Some(0x7f)
        || rd8(b, 1) != Some(b'E')
        || rd8(b, 2) != Some(b'L')
        || rd8(b, 3) != Some(b'F')
    {
        return Err(Fault::Magic);
    }
    if rd8(b, 4) != Some(ELFCLASS64) {
        return Err(Fault::Magic);
    }
    let etype = match rd16(b, 16) {
        Some(v) => v,
        None => return Err(Fault::Bounds),
    };
    if etype != ET_EXEC && etype != ET_DYN {
        return Err(Fault::Type);
    }
    let phoff = match rd64(b, 32) {
        Some(v) => v,
        None => return Err(Fault::Bounds),
    };
    let phentsize = match rd16(b, 54) {
        Some(v) => v,
        None => return Err(Fault::Bounds),
    };
    let phnum = match rd16(b, 56) {
        Some(v) => v,
        None => return Err(Fault::Bounds),
    };
    if phnum == 0 {
        return Ok(Summary { phnum: 0, load: 0, span_end: phoff, bias: 0 });
    }
    if phentsize as usize != PHENT {
        return Err(Fault::Phdr);
    }
    let table = (phnum as u64).checked_mul(PHENT as u64);
    let span_end = match table.and_then(|x| phoff.checked_add(x)) {
        Some(v) => v,
        None => return Err(Fault::Bounds),
    };
    if span_end > b.len() as u64 {
        return Err(Fault::Bounds);
    }
    let base = match usize::try_from(phoff) {
        Ok(v) => v,
        Err(_) => return Err(Fault::Bounds),
    };
    let mut load = 0u32;
    let mut bias: Option<u64> = None;
    let mut i = 0usize;
    while i < phnum as usize {
        let at = match i.checked_mul(PHENT).and_then(|x| base.checked_add(x)) {
            Some(v) => v,
            None => return Err(Fault::Bounds),
        };
        let f = match read_phdr(b, at) {
            Some(v) => v,
            None => return Err(Fault::Bounds),
        };
        i += 1;
        if f.ptype != PT_LOAD {
            continue;
        }
        if f.filesz > f.memsz {
            return Err(Fault::Seg);
        }
        if !page_power(f.align) {
            return Err(Fault::Seg);
        }
        if !congruent(f.off, f.vaddr, f.align) {
            return Err(Fault::Cong);
        }
        match f.off.checked_add(f.filesz) {
            Some(end) => {
                if end > b.len() as u64 {
                    return Err(Fault::Seg);
                }
            }
            None => return Err(Fault::Seg),
        }
        let bb = bias_of(f.vaddr, f.off);
        match bias {
            None => bias = Some(bb),
            Some(x) => {
                if x != bb {
                    return Err(Fault::Cong);
                }
            }
        }
        load += 1;
    }
    if load == 0 {
        return Err(Fault::Seg);
    }
    Ok(Summary {
        phnum,
        load,
        span_end,
        bias: bias.unwrap_or(0),
    })
}

/// 屏幕一件探针。
pub fn screen_probe(p: &Probe) -> Result<Summary, Fault> {
    screen(p.as_slice())
}

/// 单行诊断。
pub fn message(f: Fault, p: &Probe) -> String {
    match f {
        Fault::Ok => match screen_probe(p) {
            Ok(s) => {
                let mut out = String::from("ok: phnum=");
                out.push_str(&u32_text(s.phnum as u32));
                out.push_str(" load=");
                out.push_str(&u32_text(s.load));
                out.push_str(" bias=");
                out.push_str(&hex_text(s.bias));
                out.push('\n');
                out
            }
            Err(_) => String::from("ok: probe unavailable\n"),
        },
        Fault::Magic => String::from("./prog: not an ELF64 executable (bad identification)\n"),
        Fault::Type => {
            let t = rd16(p.as_slice(), 16).unwrap_or(0);
            let mut out = String::from("./prog: unsupported ELF type ");
            out.push_str(&hex_text(t as u64));
            out.push('\n');
            out
        }
        Fault::Bounds => String::from("./prog: program header table past end of file\n"),
        Fault::Phdr => {
            let s = rd16(p.as_slice(), 54).unwrap_or(0);
            let mut out = String::from("./prog: bad program header entry size ");
            out.push_str(&u32_text(s as u32));
            out.push('\n');
            out
        }
        Fault::Seg => String::from("./prog: PT_LOAD segment overruns file image\n"),
        Fault::Cong => {
            let f = read_phdr(p.as_slice(), 0x40);
            let mut out = String::from("./prog: PT_LOAD vaddr ");
            match f {
                Some(x) => {
                    out.push_str(&hex_text(x.vaddr));
                    out.push_str(" not congruent with offset ");
                    out.push_str(&hex_text(x.off));
                }
                None => out.push_str("not congruent with offset"),
            }
            out.push('\n');
            out
        }
    }
}

/// 短名。
pub fn short(f: Fault) -> &'static str {
    f.name()
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 缺陷。
    pub defect: Defect,
    /// 期望判面。
    pub fault: Fault,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(defect: Defect, fault: Fault, note: &'static str) -> Slot {
        Slot { defect, fault, note }
    }
}

/// 十四槽语料：八活六死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(Defect::None, Fault::Ok, "classic-three-load"),
    Slot::new(Defect::Bare, Fault::Ok, "bare-no-phdr"),
    Slot::new(Defect::Pie, Fault::Ok, "pie-exec"),
    Slot::new(Defect::Single, Fault::Ok, "single-load"),
    Slot::new(Defect::AlignZero, Fault::Ok, "align-zero"),
    Slot::new(Defect::AlignOne, Fault::Ok, "align-one"),
    Slot::new(Defect::TwoMeg, Fault::Ok, "align-two-meg"),
    Slot::new(Defect::Bss, Fault::Ok, "bss-tail"),
    Slot::new(Defect::Magic, Fault::Magic, "bad-magic"),
    Slot::new(Defect::Class, Fault::Magic, "elf32-class"),
    Slot::new(Defect::Phoff, Fault::Bounds, "phdr-past-eof"),
    Slot::new(Defect::Phent, Fault::Phdr, "entry-size-55"),
    Slot::new(Defect::Overflow, Fault::Seg, "filesz-over-memsz"),
    Slot::new(Defect::Trunc, Fault::Seg, "segment-past-eof"),
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<Slot> {
    CASES.get(i).copied()
}

/// 语料互异。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            let b = match CASES.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a.note == b.note || (a.defect == b.defect && a.fault == b.fault) {
                return false;
            }
            j += 1;
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
            Some(v) => {
                if v.note.is_empty() || v.defect.name().is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 逐槽对判。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let s = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let p = emit(s.defect);
        let got = match screen_probe(&p) {
            Ok(_) => Fault::Ok,
            Err(f) => f,
        };
        if got != s.fault || s.defect.expect() != s.fault {
            return false;
        }
        if p.len != LEN {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 汇总面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总数。
    pub total: u32,
    /// 通过。
    pub accept: u32,
    /// 拒收。
    pub reject: u32,
    /// 标识桶。
    pub magic: u32,
    /// 越界桶。
    pub bounds: u32,
    /// 表项桶。
    pub phdr: u32,
    /// 段面桶（含同余）。
    pub seg: u32,
    /// 命中数。
    pub matched: u32,
}

impl Report {
    /// 空面。
    pub const fn new() -> Report {
        Report { total: 0, accept: 0, reject: 0, magic: 0, bounds: 0, phdr: 0, seg: 0, matched: 0 }
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 命中率（千分比，四舍五入）。
    pub const fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched * 1000 + self.total / 2) / self.total
    }

    /// 是否全绿。
    pub const fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total
    }

    /// 闸门。
    pub const fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS as u32 {
            return Err(BP_ENOBS);
        }
        if self.matched != self.total {
            return Err(BP_EVER);
        }
        Ok(())
    }

    /// 计一槽。
    pub fn tally(&mut self, f: Fault) {
        self.total += 1;
        if f == Fault::Ok {
            self.accept += 1;
            return;
        }
        self.reject += 1;
        match f {
            Fault::Magic => self.magic += 1,
            Fault::Bounds => self.bounds += 1,
            Fault::Phdr => self.phdr += 1,
            Fault::Seg | Fault::Cong => self.seg += 1,
            Fault::Type | Fault::Ok => {}
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("probe corpus=");
        s.push_str(&u32_text(self.total));
        s.push_str(" accept=");
        s.push_str(&u32_text(self.accept));
        s.push_str(" reject=");
        s.push_str(&u32_text(self.reject));
        s.push_str(" magic=");
        s.push_str(&u32_text(self.magic));
        s.push_str(" bounds=");
        s.push_str(&u32_text(self.bounds));
        s.push_str(" phdr=");
        s.push_str(&u32_text(self.phdr));
        s.push_str(" seg=");
        s.push_str(&u32_text(self.seg));
        s.push_str(" match=");
        s.push_str(&u32_text(self.matched));
        s.push_str(" rate=");
        s.push_str(&u32_text(self.rate()));
        s.push_str("ppm\n");
        s
    }
}

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 带红档覆盖的全跑。
pub fn run_with(over: Option<(usize, u32)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(BP_ESLOT);
        }
    }
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(BP_ESLOT),
        };
        let p = emit(slot.defect);
        let got = match screen_probe(&p) {
            Ok(_) => Fault::Ok,
            Err(f) => f,
        };
        r.tally(got);
        let overridden = match over {
            Some((k, _)) => k == i,
            None => false,
        };
        if got == slot.fault && !overridden {
            r.matched += 1;
        }
        i += 1;
    }
    Ok(r)
}

/// 覆盖率闸门。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(BP_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 校验器（单次）
// ---------------------------------------------------------------------------

/// 单次校验器。
#[derive(Clone, Copy, Debug)]
pub struct Checker {
    used: bool,
    last: u32,
    steps: u32,
}

impl Checker {
    /// 新建。
    pub const fn new() -> Checker {
        Checker { used: false, last: 0, steps: 0 }
    }

    /// 是否已用。
    pub const fn bound(&self) -> bool {
        self.used
    }

    /// 步数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 末码。
    pub const fn last(&self) -> u32 {
        self.last
    }

    /// 过一件。
    pub fn check(&mut self, p: &Probe) -> Result<Summary, u32> {
        if self.used {
            self.last = BP_ESTATE;
            return Err(BP_ESTATE);
        }
        self.used = true;
        self.steps = 1;
        match screen_probe(p) {
            Ok(s) => {
                self.last = 0;
                Ok(s)
            }
            Err(f) => {
                self.last = f.code();
                Err(f.code())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u32,
    /// 操作码。
    pub op: u32,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 数值。
    pub value: u32,
}

impl Sample {
    /// 空格。
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
    /// 新账。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BP_ESEAL);
        }
        if self.len >= SAMPLES {
            return Err(BP_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
            self.len += 1;
            Ok(())
        } else {
            Err(BP_ESLOT)
        }
    }

    /// 格数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<&Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i)
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
            return Err(BP_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 2, 1, 1, 2, 929];
        const CODES: [u32; 6] = [BP_EMAGIC, BP_EMAGIC, BP_EBOUNDS, BP_EPHDR, BP_ESEG, BP_ESEG];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(BP_EBAL),
            };
            if s.slot as usize != i {
                return Err(BP_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(BP_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(BP_EVALUE);
                        }
                    }
                    None => return Err(BP_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(BP_EVALUE);
                        }
                    }
                    None => return Err(BP_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(BP_EBAL);
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
            return Err(BP_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(BP_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(BP_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(BP_ESEAL);
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

    let _ = l.register(count_cell(0, OP_REPORT, r.map(|x| x.total as usize)));
    let _ = l.register(count_cell(1, OP_REPORT, r.map(|x| x.accept as usize)));
    let _ = l.register(count_cell(2, OP_REPORT, r.map(|x| x.reject as usize)));
    let _ = l.register(count_cell(3, OP_FORGE, r.map(|x| x.magic as usize)));

    let _ = l.register(count_cell(4, OP_FORGE, r.map(|x| x.bounds as usize)));
    let _ = l.register(count_cell(5, OP_SCREEN, r.map(|x| x.phdr as usize)));
    let _ = l.register(count_cell(6, OP_PAGE, r.map(|x| x.seg as usize)));

    let red = run_with(Some((0, BP_EMAGIC)));
    let _ = l.register(count_cell(7, OP_TEXT, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_SCREEN, CASES[8].fault.code()));
    let _ = l.register(err_cell(9, OP_SCREEN, CASES[9].fault.code()));
    let _ = l.register(err_cell(10, OP_FORGE, CASES[10].fault.code()));
    let _ = l.register(err_cell(11, OP_SCREEN, CASES[11].fault.code()));
    let _ = l.register(err_cell(12, OP_FORGE, CASES[12].fault.code()));
    let _ = l.register(err_cell(13, OP_SCREEN, CASES[13].fault.code()));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称与段基址。
fn name_ok() -> bool {
    let names = [
        Fault::Ok.name(),
        Fault::Magic.name(),
        Fault::Type.name(),
        Fault::Bounds.name(),
        Fault::Phdr.name(),
        Fault::Seg.name(),
        Fault::Cong.name(),
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
    BP_EMAGIC == SEG_BASE + 0x01
        && BP_EBOUNDS == SEG_BASE + 0x02
        && BP_EPHDR == SEG_BASE + 0x03
        && BP_ESEG == SEG_BASE + 0x04
        && BP_ECONG == SEG_BASE + 0x05
        && BP_ETYPE == SEG_BASE + 0x06
        && BP_ESTATE == SEG_BASE + 0x07
        && BP_EVER == SEG_BASE + 0x08
        && BP_EBAL == SEG_BASE + 0x09
        && BP_ESLOT == SEG_BASE + 0x0a
        && BP_EVALUE == SEG_BASE + 0x0b
        && BP_ESEAL == SEG_BASE + 0x0c
        && BP_ENOBS == SEG_BASE + 0x0d
}

/// 字节读写。
fn bytes_ok() -> bool {
    let mut b = [0u8; 16];
    wr16(&mut b, 0, 0x1234);
    wr32(&mut b, 2, 0xdead_beef);
    wr64(&mut b, 8, 0x0102_0304_0506_0708);
    let a = rd16(&b, 0) == Some(0x1234);
    let c = rd32(&b, 2) == Some(0xdead_beef);
    let d = rd64(&b, 8) == Some(0x0102_0304_0506_0708);
    let e = rd8(&b, 0) == Some(0x34) && rd8(&b, 1) == Some(0x12);
    let f = rd16(&b, 16).is_none() && rd64(&b, 9).is_none();
    let mut short = [0u8; 4];
    wr32(&mut short, 0, 0xffff_ffff);
    let g = rd32(&short, 0) == Some(0xffff_ffff);
    a && c && d && e && f && g
}

/// 规范件。
fn skeleton_ok() -> bool {
    let p = emit(Defect::None);
    let b = p.as_slice();
    let a = b.len() == LEN && p.len == LEN;
    let c = rd8(b, 0) == Some(0x7f)
        && rd8(b, 1) == Some(b'E')
        && rd8(b, 2) == Some(b'L')
        && rd8(b, 3) == Some(b'F');
    let d = rd8(b, 4) == Some(ELFCLASS64)
        && rd8(b, 5) == Some(ELFDATA2LSB)
        && rd8(b, 6) == Some(EV_CURRENT);
    let e = rd16(b, 16) == Some(ET_EXEC)
        && rd16(b, 18) == Some(EM_X86_64)
        && rd32(b, 20) == Some(1);
    let f = rd64(b, 32) == Some(PHOFF)
        && rd16(b, 52) == Some(EHDR as u16)
        && rd16(b, 54) == Some(PHENT as u16)
        && rd16(b, 56) == Some(PHNUM);
    let g = rd16(b, 58) == Some(64) && rd16(b, 60) == Some(0) && rd16(b, 62) == Some(0);
    let h = read_phdr(b, 0x40)
        .map(|x| x.ptype == PT_LOAD && x.off == 0 && x.vaddr == 0x400000 && x.filesz == 0x40)
        .unwrap_or(false);
    a && c && d && e && f && g && h
}

/// 缺陷算子。
fn defect_ok() -> bool {
    let base = emit(Defect::None);
    let a = emit(Defect::Magic).bytes.get(0) == Some(&0x7e);
    let b = emit(Defect::Class).bytes.get(4) == Some(&1);
    let c = rd16(emit(Defect::Type).as_slice(), 16) == Some(ET_REL);
    let d = rd64(emit(Defect::Phoff).as_slice(), 32) == Some(LEN as u64 + 0x100);
    let e = rd16(emit(Defect::Phent).as_slice(), 54) == Some((PHENT - 1) as u16);
    let f = read_phdr(emit(Defect::Overflow).as_slice(), 0x40)
        .map(|x| x.filesz > x.memsz)
        .unwrap_or(false);
    let g = read_phdr(emit(Defect::Trunc).as_slice(), 0x40)
        .map(|x| x.off + x.filesz > LEN as u64)
        .unwrap_or(false);
    let h = read_phdr(emit(Defect::Align).as_slice(), 0x40)
        .map(|x| !page_power(x.align))
        .unwrap_or(false);
    let i = read_phdr(emit(Defect::Cong).as_slice(), 0x40)
        .map(|x| !congruent(x.off, x.vaddr, x.align))
        .unwrap_or(false);
    let j = rd16(emit(Defect::Bare).as_slice(), 56) == Some(0);
    let k = rd16(emit(Defect::Pie).as_slice(), 16) == Some(ET_DYN);
    let l = rd16(emit(Defect::Single).as_slice(), 56) == Some(1);
    let m = read_phdr(emit(Defect::AlignZero).as_slice(), 0x40)
        .map(|x| x.align == 0)
        .unwrap_or(false);
    let n = read_phdr(emit(Defect::AlignOne).as_slice(), 0x40)
        .map(|x| x.align == 1)
        .unwrap_or(false);
    let o = read_phdr(emit(Defect::TwoMeg).as_slice(), 0x40)
        .map(|x| x.align == 0x20_0000)
        .unwrap_or(false);
    let q = read_phdr(emit(Defect::Bss).as_slice(), 0x40 + PHENT)
        .map(|x| x.filesz == 0 && x.memsz == 0x100)
        .unwrap_or(false);
    let r = base.seal() == emit(Defect::None).seal() && base.seal() != emit(Defect::Bss).seal();
    a && b && c && d && e && f && g && h && i && j && k && l && m && n && o && q && r
}

/// 屏幕判面。
fn screen_ok() -> bool {
    let a = screen_probe(&emit(Defect::None)).is_ok();
    let b = screen_probe(&emit(Defect::Bare)) == Ok(Summary { phnum: 0, load: 0, span_end: PHOFF, bias: 0 });
    let c = screen_probe(&emit(Defect::Pie)) == Ok(Summary { phnum: 3, load: 3, span_end: 0xe8, bias: 0x400000 });
    let d = screen_probe(&emit(Defect::Magic)).err() == Some(Fault::Magic);
    let e = screen_probe(&emit(Defect::Class)).err() == Some(Fault::Magic);
    let f = screen_probe(&emit(Defect::Type)).err() == Some(Fault::Type);
    let g = screen_probe(&emit(Defect::Phoff)).err() == Some(Fault::Bounds);
    let h = screen_probe(&emit(Defect::Phent)).err() == Some(Fault::Phdr);
    let i = screen_probe(&emit(Defect::Overflow)).err() == Some(Fault::Seg);
    let j = screen_probe(&emit(Defect::Trunc)).err() == Some(Fault::Seg);
    let k = screen_probe(&emit(Defect::Align)).err() == Some(Fault::Seg);
    let l = screen_probe(&emit(Defect::Cong)).err() == Some(Fault::Cong);
    let m = screen(&[]).err() == Some(Fault::Bounds) && screen(&[0u8; 32]).err() == Some(Fault::Bounds);
    a && b && c && d && e && f && g && h && i && j && k && l && m
}

/// 页算术。
fn page_ok() -> bool {
    let a = page_floor(0x4000ff) == 0x400000 && page_ceil(0x400001) == 0x401000;
    let b = page_ceil(0x400000) == 0x400000 && page_ceil(0) == 0 && page_ceil(u64::MAX) == u64::MAX;
    let c = bias_of(0x401000, 0x1000) == 0x400000 && bias_of(0x400800, 0x100) == 0x400000;
    let d = page_power(0) && page_power(1) && page_power(2) && page_power(0x1000);
    let e = !page_power(3) && !page_power(0x1800) && !page_power(0x6000);
    let f = congruent(0, 0x400000, 0x1000) && congruent(0x1000, 0x401000, 0x1000);
    let g = !congruent(0, 0x400800, 0x1000) && congruent(0, 0x400800, 0) && congruent(0, 0x400800, 1);
    a && b && c && d && e && f && g
}

/// 语料互异。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.magic == 2 && r.bounds == 1 && r.phdr == 1 && r.seg == 2;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = table_ok() && unique() && notes_ok();
    a && b && c && d
}

/// 渲染。
fn render_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    r.render().as_str()
        == "probe corpus=14 accept=8 reject=6 magic=2 bounds=1 phdr=1 seg=2 match=14 rate=1000ppm\n"
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, BP_EMAGIC))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == CORPUS as u32 - 1 && r.rate() == 929;
    let b = r.gate() == Err(BP_EVER) && !r.green();
    let c = run_with(Some((CORPUS, BP_EMAGIC))).err() == Some(BP_ESLOT);
    a && b && c
}

/// 文案。
fn message_ok() -> bool {
    let a = message(Fault::Ok, &emit(Defect::None)).as_str()
        == "ok: phnum=3 load=3 bias=0x400000\n";
    let b = message(Fault::Magic, &emit(Defect::Magic)).as_str()
        == "./prog: not an ELF64 executable (bad identification)\n";
    let c = message(Fault::Type, &emit(Defect::Type)).as_str()
        == "./prog: unsupported ELF type 0x1\n";
    let d = message(Fault::Bounds, &emit(Defect::Phoff)).as_str()
        == "./prog: program header table past end of file\n";
    let e = message(Fault::Phdr, &emit(Defect::Phent)).as_str()
        == "./prog: bad program header entry size 55\n";
    let f = message(Fault::Seg, &emit(Defect::Overflow)).as_str()
        == "./prog: PT_LOAD segment overruns file image\n";
    let g = message(Fault::Cong, &emit(Defect::Cong)).as_str()
        == "./prog: PT_LOAD vaddr 0x400800 not congruent with offset 0x0\n";
    let h = message(Fault::Ok, &emit(Defect::Bare)).as_str()
        == "ok: phnum=0 load=0 bias=0x0\n";
    a && b && c && d && e && f && g && h
}

/// 槽号与覆盖。
fn slots_ok() -> bool {
    let a = match slot_at(0) {
        Some(s) => s.defect == Defect::None && s.fault == Fault::Ok,
        None => false,
    };
    let b = match slot_at(13) {
        Some(s) => s.defect == Defect::Trunc && s.fault == Fault::Seg,
        None => false,
    };
    let c = slot_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(BP_ENOBS);
    let e = short(Fault::Cong) == "cong" && short(Fault::Ok) == "ok";
    let f = Defect::Cong.expect() == Fault::Cong && !Defect::Cong.alive();
    a && b && c && d && e && f
}

/// 校验器。
fn checker_ok() -> bool {
    let mut c = Checker::new();
    let a = !c.bound() && c.steps() == 0;
    let b = match c.check(&emit(Defect::None)) {
        Ok(s) => s.phnum == 3 && s.load == 3 && c.last() == 0,
        Err(_) => false,
    };
    let d = c.check(&emit(Defect::None)).err() == Some(BP_ESTATE);
    let e = c.bound() && c.steps() == 1 && c.last() == BP_ESTATE;
    a && b && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && bytes_ok() && page_ok();
    out[1] = skeleton_ok() && defect_ok() && screen_ok();
    out[2] = corpus_ok() && render_ok() && reds_ok() && slots_ok();
    out[3] = message_ok()
        && checker_ok()
        && build_ledger().audit() == Ok(())
        && {
            let mut l = build_ledger();
            l.freeze().is_ok() && l.replay().is_ok()
        };
    out
}

/// 判据集合。
pub fn run_unx11616_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11616.probe_forge");

    s.add(
        "BF01-名称与段基址",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "BF02-字节读写",
        {
            bytes_ok() && page_ok()
        },
        "static",
    );

    s.add(
        "BF03-规范件生成",
        {
            skeleton_ok()
        },
        "static",
    );

    s.add(
        "BF04-缺陷算子",
        {
            defect_ok()
        },
        "static",
    );

    s.add(
        "BF05-屏幕判面",
        {
            screen_ok()
        },
        "static",
    );

    s.add(
        "BF06-语料互异",
        {
            unique() && notes_ok()
        },
        "static",
    );

    s.add(
        "BF07-逐槽对判",
        {
            table_ok()
        },
        "static",
    );

    s.add(
        "BF08-汇总与渲染",
        {
            corpus_ok() && render_ok()
        },
        "static",
    );

    s.add(
        "BF09-红档显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "BF10-文案逐字",
        {
            message_ok()
        },
        "static",
    );

    s.add(
        "BF11-槽号与覆盖",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "BF12-校验器与账本",
        {
            checker_ok() && build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "BF13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            a && b && inj[0] && inj[1] && inj[2] && inj[3]
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
        panic!("unx11616 test fail")
    }

    #[test]
    fn bytes_roundtrip() {
        let mut b = [0u8; 8];
        wr8(&mut b, 0, 0xaa);
        assert_eq!(rd8(&b, 0), Some(0xaa));
        wr16(&mut b, 1, 0x1234);
        assert_eq!(rd16(&b, 1), Some(0x1234));
        wr32(&mut b, 3, 0x0102_0304);
        assert_eq!(rd32(&b, 3), Some(0x0102_0304));
        wr64(&mut b, 0, 0xffff_ffff_ffff_ffff);
        assert_eq!(rd64(&b, 0), Some(u64::MAX));
        assert!(rd64(&b, 1).is_none());
        assert!(rd8(&b, 8).is_none());
        assert!(rd16(&b, 7).is_none());
    }

    #[test]
    fn skeleton_header() {
        let p = emit(Defect::None);
        let b = p.as_slice();
        assert_eq!(p.len, LEN);
        assert_eq!(rd16(b, 16), Some(ET_EXEC));
        assert_eq!(rd16(b, 18), Some(EM_X86_64));
        assert_eq!(rd64(b, 32), Some(PHOFF));
        assert_eq!(rd16(b, 54), Some(PHENT as u16));
        assert_eq!(rd16(b, 56), Some(PHNUM));
        assert_eq!(rd16(b, 52), Some(EHDR as u16));
    }

    #[test]
    fn skeleton_segments() {
        let p = emit(Defect::None);
        let b = p.as_slice();
        let s0 = match read_phdr(b, 0x40) {
            Some(v) => v,
            None => fail(),
        };
        let s1 = match read_phdr(b, 0x40 + PHENT) {
            Some(v) => v,
            None => fail(),
        };
        let s2 = match read_phdr(b, 0x40 + 2 * PHENT) {
            Some(v) => v,
            None => fail(),
        };
        assert_eq!(s0.ptype, PT_LOAD);
        assert_eq!(s0.vaddr, 0x400000);
        assert_eq!(s1.vaddr, 0x401000);
        assert_eq!(s2.vaddr, 0x402000);
        assert_eq!(bias_of(s0.vaddr, s0.off), 0x400000);
        assert_eq!(bias_of(s1.vaddr, s1.off), 0x400000);
        assert_eq!(bias_of(s2.vaddr, s2.off), 0x400000);
    }

    #[test]
    fn screen_classic() {
        let s = match screen_probe(&emit(Defect::None)) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(s.phnum, 3);
        assert_eq!(s.load, 3);
        assert_eq!(s.bias, 0x400000);
    }

    #[test]
    fn screen_bare() {
        let s = match screen_probe(&emit(Defect::Bare)) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(s.phnum, 0);
        assert_eq!(s.load, 0);
    }

    #[test]
    fn screen_pie() {
        let p = emit(Defect::Pie);
        assert_eq!(rd16(p.as_slice(), 16), Some(ET_DYN));
        assert!(screen_probe(&p).is_ok());
    }

    #[test]
    fn screen_single() {
        let s = match screen_probe(&emit(Defect::Single)) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(s.phnum, 1);
        assert_eq!(s.load, 1);
    }

    #[test]
    fn screen_bss() {
        assert!(screen_probe(&emit(Defect::Bss)).is_ok());
    }

    #[test]
    fn screen_align_variants() {
        assert!(screen_probe(&emit(Defect::AlignZero)).is_ok());
        assert!(screen_probe(&emit(Defect::AlignOne)).is_ok());
        assert!(screen_probe(&emit(Defect::TwoMeg)).is_ok());
        assert_eq!(screen_probe(&emit(Defect::Align)).err(), Some(Fault::Seg));
    }

    #[test]
    fn screen_magic_faults() {
        assert_eq!(screen_probe(&emit(Defect::Magic)).err(), Some(Fault::Magic));
        assert_eq!(screen_probe(&emit(Defect::Class)).err(), Some(Fault::Magic));
    }

    #[test]
    fn screen_type_fault() {
        assert_eq!(screen_probe(&emit(Defect::Type)).err(), Some(Fault::Type));
        assert_eq!(Fault::Type.code(), BP_ETYPE);
    }

    #[test]
    fn screen_bounds_fault() {
        assert_eq!(screen_probe(&emit(Defect::Phoff)).err(), Some(Fault::Bounds));
        assert_eq!(screen(&[]).err(), Some(Fault::Bounds));
    }

    #[test]
    fn screen_phent_fault() {
        assert_eq!(screen_probe(&emit(Defect::Phent)).err(), Some(Fault::Phdr));
        assert_eq!(Fault::Phdr.code(), BP_EPHDR);
    }

    #[test]
    fn screen_seg_faults() {
        assert_eq!(screen_probe(&emit(Defect::Overflow)).err(), Some(Fault::Seg));
        assert_eq!(screen_probe(&emit(Defect::Trunc)).err(), Some(Fault::Seg));
        assert_eq!(Fault::Seg.code(), BP_ESEG);
    }

    #[test]
    fn screen_cong_fault() {
        assert_eq!(screen_probe(&emit(Defect::Cong)).err(), Some(Fault::Cong));
        assert_eq!(Fault::Cong.code(), BP_ECONG);
    }

    #[test]
    fn messages() {
        assert_eq!(
            message(Fault::Ok, &emit(Defect::None)).as_str(),
            "ok: phnum=3 load=3 bias=0x400000\n"
        );
        assert_eq!(
            message(Fault::Magic, &emit(Defect::Magic)).as_str(),
            "./prog: not an ELF64 executable (bad identification)\n"
        );
        assert_eq!(
            message(Fault::Type, &emit(Defect::Type)).as_str(),
            "./prog: unsupported ELF type 0x1\n"
        );
        assert_eq!(
            message(Fault::Phdr, &emit(Defect::Phent)).as_str(),
            "./prog: bad program header entry size 55\n"
        );
    }

    #[test]
    fn message_ok_helpers() {
        assert!(message(Fault::Bounds, &emit(Defect::Phoff)).as_str().contains("past end"));
        assert!(message(Fault::Seg, &emit(Defect::Overflow)).as_str().contains("overruns"));
        assert!(message(Fault::Cong, &emit(Defect::Cong)).as_str().contains("not congruent"));
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
        assert_eq!(r.total, 14);
        assert_eq!(r.accept, 8);
        assert_eq!(r.reject, 6);
        assert_eq!(r.magic, 2);
        assert_eq!(r.bounds, 1);
        assert_eq!(r.phdr, 1);
        assert_eq!(r.seg, 2);
        assert_eq!(r.matched, 14);
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
            "probe corpus=14 accept=8 reject=6 magic=2 bounds=1 phdr=1 seg=2 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, BP_EMAGIC))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(BP_EVER));
        assert_eq!(run_with(Some((14, BP_EMAGIC))).err(), Some(BP_ESLOT));
    }

    #[test]
    fn checker_once() {
        let mut c = Checker::new();
        match c.check(&emit(Defect::None)) {
            Ok(s) => assert_eq!(s.load, 3),
            Err(_) => fail(),
        }
        assert_eq!(c.check(&emit(Defect::None)).err(), Some(BP_ESTATE));
        assert_eq!(c.steps(), 1);
        assert_eq!(c.last(), BP_ESTATE);
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
        assert_eq!(l.register(Sample::blank()).err(), Some(BP_ESEAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(BP_ESEAL));
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
        assert_eq!(fold_word(FNV_OFF, 1), fold_word(FNV_OFF, 1));
        assert_ne!(fold_word(FNV_OFF, 1), fold_word(FNV_OFF, 2));
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(hex_text(0x400800).as_str(), "0x400800");
        assert_eq!(hex_text(1000).as_str(), "0x3e8");
    }

    #[test]
    fn slot_bounds() {
        assert!(slot_at(0).is_some());
        assert!(slot_at(CORPUS - 1).is_some());
        assert!(slot_at(CORPUS).is_none());
        assert_eq!(short(Fault::Ok), "ok");
        assert_eq!(short(Fault::Seg), "seg");
    }

    #[test]
    fn probe_seal_stability() {
        let a = emit(Defect::None);
        let b = emit(Defect::None);
        assert_eq!(a.seal(), b.seal());
        assert_ne!(a.seal(), emit(Defect::Pie).seal());
        assert_eq!(a.as_slice().len(), LEN);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11616_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
