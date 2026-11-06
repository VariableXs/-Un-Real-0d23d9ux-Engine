//! UNX-F11614 e_phentsize/e_shentsize 悖谬拒收判据
//!
//! 镜面：ELF 头里的表项尺寸不是「约定俗成的注释」，而是装载器能否按步
//! 长遍历段表/节表的结构前提。内核与 ld.so 都要求：
//!
//! 1. `e_phentsize` 只在 `e_phnum == 0` 时允许为 0 或 56，否则必须恰为 56；
//! 2. `e_shentsize` 只在 `e_shnum == 0` 时允许为 0，否则必须恰为 64；
//! 3. `e_phnum` 超过 `PN_XNUM`（0xffff）的扩展编号不在本闸支持面内；
//! 4. 计数为零时表偏移必须为零——「幻影表」一律拒；
//! 5. 表区间 `phoff + phnum*phentsize`、`shoff + shnum*shentsize` 必须落在文件内，
//!    乘加全程防溢出；
//! 6. 节名索引 `e_shstrndx` 必须为 0，或严格小于 `e_shnum`；`SHN_XINDEX`
//!    （0xffff）在本闸不支持面内；
//! 7. `e_ident` 类别必须是 ELFCLASS64。
//!
//! 语料十四槽，八活六死——表项悖谬一、节项悖谬一、计数/幻影一、区间越界一、
//! 索引越界一、类别错一。
//!
//! 次序要害：类别先于一切（没有正确的字长就谈不上任何步长）；计数先于
//! 表项（0x10000 个表项的错误比步长错误更根本）；表项先于区间（步长错时
//! 区间乘积没有意义）；区间先于索引（索引的意义依赖节表在文件内）。
//!
//! 闸门次序：名称 → 几何算术 → 正件通过 → 表项/节项 → 计数与幻影 →
//! 区间 → 索引 → 类别 → 文案 → 语料 → 汇总 → 冻结重放。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfbd0;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// ELFCLASS64。
pub const ELFCLASS64: u8 = 2;

/// ELF64 程序头表项字节。
pub const PHENT: u16 = 56;

/// ELF64 节头表项字节。
pub const SHENT: u16 = 64;

/// PN_XNUM。
pub const PN_XNUM: u32 = 0xffff;

/// SHN_UNDEF。
pub const SHN_UNDEF: u32 = 0;

/// SHN_XINDEX。
pub const SHN_XINDEX: u32 = 0xffff;

/// ENOEXEC。
pub const ENOEXEC: i32 = 8;

/// EINVAL。
pub const EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 程序头表项尺寸悖谬。
pub const TS_EPHENT: u32 = SEG_BASE + 0x01;
/// 节头表项尺寸悖谬。
pub const TS_ESHENT: u32 = SEG_BASE + 0x02;
/// 计数越界或幻影表。
pub const TS_ECNT: u32 = SEG_BASE + 0x03;
/// 表区间越界或乘法溢出。
pub const TS_EOFF: u32 = SEG_BASE + 0x04;
/// 节名索引越界。
pub const TS_ESTR: u32 = SEG_BASE + 0x05;
/// 类别/标识错。
pub const TS_EMAGIC: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const TS_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const TS_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const TS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const TS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const TS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const TS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const TS_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 几何算术。
pub const OP_GEOM: u32 = 1;
/// 表项。
pub const OP_ENT: u32 = 2;
/// 计数。
pub const OP_COUNT: u32 = 3;
/// 区间。
pub const OP_SPAN: u32 = 4;
/// 索引。
pub const OP_INDEX: u32 = 5;
/// 文案。
pub const OP_TEXT: u32 = 6;
/// 汇总。
pub const OP_REPORT: u32 = 7;

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
// 表几何
// ---------------------------------------------------------------------------

/// 程序头表字节数。
pub const fn phdr_bytes(e: &Ehdr) -> Option<u64> {
    (e.phnum as u64).checked_mul(e.phentsize as u64)
}

/// 程序头表末端。
pub const fn phdr_end(e: &Ehdr) -> Option<u64> {
    match phdr_bytes(e) {
        Some(n) => e.phoff.checked_add(n),
        None => None,
    }
}

/// 节头表字节数。
pub const fn shdr_bytes(e: &Ehdr) -> Option<u64> {
    (e.shnum as u64).checked_mul(e.shentsize as u64)
}

/// 节头表末端。
pub const fn shdr_end(e: &Ehdr) -> Option<u64> {
    match shdr_bytes(e) {
        Some(n) => e.shoff.checked_add(n),
        None => None,
    }
}

/// 第 i 个程序头的文件偏移。
pub fn phdr_at(e: &Ehdr, i: u32) -> Option<u64> {
    if i >= e.phnum {
        return None;
    }
    let step = match (i as u64).checked_mul(e.phentsize as u64) {
        Some(v) => v,
        None => return None,
    };
    e.phoff.checked_add(step)
}

/// 第 i 个节头的文件偏移。
pub fn shdr_at(e: &Ehdr, i: u32) -> Option<u64> {
    if i >= e.shnum {
        return None;
    }
    let step = match (i as u64).checked_mul(e.shentsize as u64) {
        Some(v) => v,
        None => return None,
    };
    e.shoff.checked_add(step)
}

/// 表区间是否落于文件内。
pub const fn tables_within(e: &Ehdr) -> bool {
    let p = match phdr_end(e) {
        Some(v) => v,
        None => return false,
    };
    let s = match shdr_end(e) {
        Some(v) => v,
        None => return false,
    };
    p <= e.file_len && s <= e.file_len
}

// ---------------------------------------------------------------------------
// ELF 头视图
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 通过。
    Ok,
    /// 程序头表项悖谬。
    PhEnt,
    /// 节头表项悖谬。
    ShEnt,
    /// 计数越界或幻影表。
    Count,
    /// 表区间越界。
    Off,
    /// 节名索引越界。
    Str,
    /// 类别错。
    Magic,
}

impl Fault {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::PhEnt => "phentsize",
            Fault::ShEnt => "shentsize",
            Fault::Count => "count",
            Fault::Off => "table-span",
            Fault::Str => "shstrndx",
            Fault::Magic => "class",
        }
    }

    /// 错误码。
    pub const fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::PhEnt => TS_EPHENT,
            Fault::ShEnt => TS_ESHENT,
            Fault::Count => TS_ECNT,
            Fault::Off => TS_EOFF,
            Fault::Str => TS_ESTR,
            Fault::Magic => TS_EMAGIC,
        }
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        match self {
            Fault::Ok => 0,
            Fault::Magic => ENOEXEC,
            _ => EINVAL,
        }
    }

    /// 是否拒收面。
    pub const fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// ELF64 头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ehdr {
    /// e_ident 类别。
    pub class: u8,
    /// 程序头表偏移。
    pub phoff: u64,
    /// 程序头表项字节。
    pub phentsize: u16,
    /// 程序头数。
    pub phnum: u32,
    /// 节头表偏移。
    pub shoff: u64,
    /// 节头表项字节。
    pub shentsize: u16,
    /// 节头数。
    pub shnum: u32,
    /// 节名索引。
    pub shstrndx: u32,
    /// 文件长度。
    pub file_len: u64,
    /// 入口地址（仅参与指纹，使不同映像互异）。
    pub entry: u64,
}

/// 默认文件长度。
pub const FILE_LEN: u64 = 0x80_0000;

impl Ehdr {
    /// 构造。
    pub const fn new(
        class: u8,
        phoff: u64,
        phentsize: u16,
        phnum: u32,
        shoff: u64,
        shentsize: u16,
        shnum: u32,
        shstrndx: u32,
        file_len: u64,
        entry: u64,
    ) -> Ehdr {
        Ehdr { class, phoff, phentsize, phnum, shoff, shentsize, shnum, shstrndx, file_len, entry }
    }

    /// 带默认文件长度与入口。
    pub const fn of(
        phoff: u64,
        phentsize: u16,
        phnum: u32,
        shoff: u64,
        shentsize: u16,
        shnum: u32,
        shstrndx: u32,
    ) -> Ehdr {
        Ehdr::new(ELFCLASS64, phoff, phentsize, phnum, shoff, shentsize, shnum, shstrndx, FILE_LEN, 0x401000)
    }

    /// 典型可执行件。
    pub const fn classic() -> Ehdr {
        Ehdr::of(0x40, PHENT, 3, 0x200, SHENT, 5, 4)
    }

    /// 典型 PIE。
    pub const fn pie() -> Ehdr {
        Ehdr::new(ELFCLASS64, 0x40, PHENT, 4, 0x200, SHENT, 6, 5, FILE_LEN, 0x1040)
    }

    /// 无表映像。
    pub const fn bare() -> Ehdr {
        Ehdr::of(0, 0, 0, 0, 0, 0, 0)
    }

    /// 程序头表项数。
    pub const fn phnum(&self) -> u32 {
        self.phnum
    }

    /// 节头表项数。
    pub const fn shnum(&self) -> u32 {
        self.shnum
    }

    /// 是否含节表。
    pub const fn has_shdr(&self) -> bool {
        self.shnum > 0
    }

    /// 是否含程序头表。
    pub const fn has_phdr(&self) -> bool {
        self.phnum > 0
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.class as u64);
        h = fold_word(h, self.phoff);
        h = fold_word(h, self.phentsize as u64);
        h = fold_word(h, self.phnum as u64);
        h = fold_word(h, self.shoff);
        h = fold_word(h, self.shentsize as u64);
        h = fold_word(h, self.shnum as u64);
        h = fold_word(h, self.shstrndx as u64);
        h = fold_word(h, self.file_len);
        h = fold_word(h, self.entry);
        h
    }
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 全头判定。
pub fn check(e: &Ehdr) -> Fault {
    if e.class != ELFCLASS64 {
        return Fault::Magic;
    }
    if e.phnum > PN_XNUM {
        return Fault::Count;
    }
    if e.phnum > 0 && e.phentsize != PHENT {
        return Fault::PhEnt;
    }
    if e.phnum == 0 && e.phoff != 0 {
        return Fault::Count;
    }
    if e.phnum == 0 && e.phentsize != 0 && e.phentsize != PHENT {
        return Fault::PhEnt;
    }
    if e.shnum > 0 && e.shentsize != SHENT {
        return Fault::ShEnt;
    }
    if e.shnum == 0 && e.shentsize != 0 {
        return Fault::ShEnt;
    }
    if e.shnum == 0 && e.shoff != 0 {
        return Fault::Count;
    }
    if !tables_within(e) {
        return Fault::Off;
    }
    if e.shnum > 0 {
        if e.shstrndx == SHN_XINDEX {
            return Fault::Str;
        }
        if e.shstrndx != SHN_UNDEF && e.shstrndx >= e.shnum {
            return Fault::Str;
        }
    } else if e.shstrndx != SHN_UNDEF {
        return Fault::Str;
    }
    Fault::Ok
}

/// 判定并给出首个失配原因。
pub fn verdict(e: &Ehdr) -> Verdict {
    let f = check(e);
    Verdict { fault: f, code: f.code(), errno: f.errno(), accepted: f == Fault::Ok }
}

/// 判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// 判面。
    pub fault: Fault,
    /// 码。
    pub code: u32,
    /// errno。
    pub errno: i32,
    /// 是否放行。
    pub accepted: bool,
}

impl Verdict {
    /// 是否放行。
    pub const fn ok(&self) -> bool {
        self.accepted
    }
}

/// 单行诊断。
pub fn message(f: Fault, e: &Ehdr) -> String {
    let mut s = String::new();
    match f {
        Fault::Ok => {
            s.push_str("ok: phnum=");
            s.push_str(&u32_text(e.phnum));
            s.push_str(" phentsize=");
            s.push_str(&hex_text(e.phentsize as u64));
            s.push_str(" shnum=");
            s.push_str(&u32_text(e.shnum));
            s.push_str(" shentsize=");
            s.push_str(&hex_text(e.shentsize as u64));
            s.push('\n');
        }
        Fault::Magic => {
            s.push_str("./prog: bad ELF header: e_ident class=");
            s.push_str(&u32_text(e.class as u32));
            s.push_str(" (want 2)\n");
        }
        Fault::PhEnt => {
            s.push_str("./prog: bad ELF header: e_phentsize=");
            s.push_str(&hex_text(e.phentsize as u64));
            s.push_str(" != ");
            s.push_str(&hex_text(PHENT as u64));
            s.push('\n');
        }
        Fault::ShEnt => {
            s.push_str("./prog: bad ELF header: e_shentsize=");
            s.push_str(&hex_text(e.shentsize as u64));
            s.push_str(" != ");
            s.push_str(&hex_text(SHENT as u64));
            s.push('\n');
        }
        Fault::Count => {
            if e.phnum > PN_XNUM {
                s.push_str("./prog: bad ELF header: e_phnum=");
                s.push_str(&hex_text(e.phnum as u64));
                s.push_str(" exceeds PN_XNUM\n");
            } else if e.phnum == 0 && e.phoff != 0 {
                s.push_str("./prog: bad ELF header: e_phnum=0 but e_phoff=");
                s.push_str(&hex_text(e.phoff));
                s.push('\n');
            } else if e.shnum == 0 && e.shoff != 0 {
                s.push_str("./prog: bad ELF header: e_shnum=0 but e_shoff=");
                s.push_str(&hex_text(e.shoff));
                s.push('\n');
            } else {
                s.push_str("./prog: bad ELF header: table count underflow\n");
            }
        }
        Fault::Off => {
            if let Some(p) = phdr_end(e) {
                if p > e.file_len {
                    s.push_str("./prog: bad ELF header: program headers end=");
                    s.push_str(&hex_text(p));
                    s.push_str(" past file len=");
                    s.push_str(&hex_text(e.file_len));
                    s.push('\n');
                    return s;
                }
            }
            if let Some(t) = shdr_end(e) {
                s.push_str("./prog: bad ELF header: section headers end=");
                s.push_str(&hex_text(t));
                s.push_str(" past file len=");
                s.push_str(&hex_text(e.file_len));
                s.push('\n');
            } else {
                s.push_str("./prog: bad ELF header: table span overflows u64\n");
            }
        }
        Fault::Str => {
            s.push_str("./prog: bad ELF header: e_shstrndx=");
            s.push_str(&hex_text(e.shstrndx as u64));
            s.push_str(" >= e_shnum=");
            s.push_str(&hex_text(e.shnum as u64));
            s.push('\n');
        }
    }
    s
}

/// 短标签。
pub fn short(f: Fault) -> &'static str {
    f.name()
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 头。
    pub ehdr: Ehdr,
    /// 期望判面。
    pub fault: Fault,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(ehdr: Ehdr, fault: Fault, note: &'static str) -> Slot {
        Slot { ehdr, fault, note }
    }
}

/// 十四槽语料：八活六死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(Ehdr::classic(), Fault::Ok, "classic-exec"),
    Slot::new(Ehdr::bare(), Fault::Ok, "bare-no-tables"),
    Slot::new(Ehdr::of(0, PHENT, 1, 0, 0, 0, 0), Fault::Ok, "phdr-at-zero"),
    Slot::new(Ehdr::of(0x40, PHENT, 2, 0, 0, 0, 0), Fault::Ok, "phdr-only"),
    Slot::new(Ehdr::of(0, 0, 0, 0x200, SHENT, 0xffff, 0), Fault::Ok, "max-shnum"),
    Slot::new(Ehdr::of(0, PHENT, 0, 0, 0, 0, 0), Fault::Ok, "zero-phnum-entsize"),
    Slot::new(Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 3, 0), Fault::Ok, "undef-str"),
    Slot::new(Ehdr::of(0x40, PHENT, 1, 0x300, SHENT, 1, 0), Fault::Ok, "one-section"),
    Slot::new(Ehdr::of(0x40, 0x40, 3, 0, 0, 0, 0), Fault::PhEnt, "phent-64"),
    Slot::new(Ehdr::of(0x40, PHENT, 1, 0x200, 0x20, 5, 0), Fault::ShEnt, "shent-32"),
    Slot::new(Ehdr::of(0x40, PHENT, 0x10000, 0, 0, 0, 0), Fault::Count, "phnum-xnum"),
    Slot::new(Ehdr::new(ELFCLASS64, 0x3f0, PHENT, 3, 0, 0, 0, 0, 0x400, 0x401000), Fault::Off, "phdr-past-eof"),
    Slot::new(Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 5, 9), Fault::Str, "shstrndx-oob"),
    Slot::new(Ehdr::new(1, 0x40, PHENT, 3, 0x200, SHENT, 5, 4, FILE_LEN, 0x401000), Fault::Magic, "class-32"),
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<Slot> {
    CASES.get(i).copied()
}

/// 语料是否互异。
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
            if a.ehdr == b.ehdr || a.note == b.note || a.ehdr.seal() == b.ehdr.seal() {
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
                if v.note.is_empty() {
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
        let s = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if check(&s.ehdr) != s.fault {
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
    /// 表项悖谬。
    pub phent: u32,
    /// 节项悖谬。
    pub shent: u32,
    /// 计数/幻影。
    pub cnt: u32,
    /// 区间越界。
    pub off: u32,
    /// 索引越界。
    pub str: u32,
    /// 类别错。
    pub magic: u32,
    /// 对判命中。
    pub matched: u32,
}

impl Report {
    /// 空面。
    pub const fn new() -> Report {
        Report {
            total: 0,
            accept: 0,
            reject: 0,
            phent: 0,
            shent: 0,
            cnt: 0,
            off: 0,
            str: 0,
            magic: 0,
            matched: 0,
        }
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
            return Err(TS_ENOBS);
        }
        if self.matched != self.total {
            return Err(TS_EVER);
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
            Fault::PhEnt => self.phent += 1,
            Fault::ShEnt => self.shent += 1,
            Fault::Count => self.cnt += 1,
            Fault::Off => self.off += 1,
            Fault::Str => self.str += 1,
            Fault::Magic => self.magic += 1,
            _ => {}
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("phent corpus=");
        s.push_str(&u32_text(self.total));
        s.push_str(" accept=");
        s.push_str(&u32_text(self.accept));
        s.push_str(" reject=");
        s.push_str(&u32_text(self.reject));
        s.push_str(" phent=");
        s.push_str(&u32_text(self.phent));
        s.push_str(" shent=");
        s.push_str(&u32_text(self.shent));
        s.push_str(" cnt=");
        s.push_str(&u32_text(self.cnt));
        s.push_str(" off=");
        s.push_str(&u32_text(self.off));
        s.push_str(" str=");
        s.push_str(&u32_text(self.str));
        s.push_str(" magic=");
        s.push_str(&u32_text(self.magic));
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
            return Err(TS_ESLOT);
        }
    }
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(TS_ESLOT),
        };
        let got = check(&slot.ehdr);
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
        Err(TS_ENOBS)
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

    /// 校验一头。
    pub fn check(&mut self, e: &Ehdr) -> Result<Fault, u32> {
        if self.used {
            self.last = TS_ESTATE;
            return Err(TS_ESTATE);
        }
        self.used = true;
        self.steps = 1;
        let f = check(e);
        self.last = f.code();
        Ok(f)
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
            return Err(TS_ESEAL);
        }
        if self.len >= SAMPLES {
            return Err(TS_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
            self.len += 1;
            Ok(())
        } else {
            Err(TS_ESLOT)
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
            return Err(TS_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 1, 1, 1, 1, 929];
        const CODES: [u32; 6] = [TS_EPHENT, TS_ESHENT, TS_ECNT, TS_EOFF, TS_ESTR, TS_EMAGIC];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TS_EBAL),
            };
            if s.slot as usize != i {
                return Err(TS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TS_EVALUE);
                        }
                    }
                    None => return Err(TS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TS_EVALUE);
                        }
                    }
                    None => return Err(TS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TS_EBAL);
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
            return Err(TS_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(TS_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(TS_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(TS_ESEAL);
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
    let _ = l.register(count_cell(3, OP_ENT, r.map(|x| x.phent as usize)));

    let _ = l.register(count_cell(4, OP_ENT, r.map(|x| x.shent as usize)));
    let _ = l.register(count_cell(5, OP_COUNT, r.map(|x| x.cnt as usize)));
    let _ = l.register(count_cell(6, OP_SPAN, r.map(|x| x.off as usize)));

    let red = run_with(Some((0, TS_EPHENT)));
    let _ = l.register(count_cell(7, OP_TEXT, red.map(|x| x.rate() as usize)));

    let pe = CASES[8].fault;
    let _ = l.register(err_cell(8, OP_ENT, pe.code()));

    let se = CASES[9].fault;
    let _ = l.register(err_cell(9, OP_ENT, se.code()));

    let cn = CASES[10].fault;
    let _ = l.register(err_cell(10, OP_COUNT, cn.code()));

    let of = CASES[11].fault;
    let _ = l.register(err_cell(11, OP_SPAN, of.code()));

    let st = CASES[12].fault;
    let _ = l.register(err_cell(12, OP_INDEX, st.code()));

    let mg = CASES[13].fault;
    let _ = l.register(err_cell(13, OP_TEXT, mg.code()));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称与段基址。
fn name_ok() -> bool {
    let names = [
        Fault::Ok.name(),
        Fault::PhEnt.name(),
        Fault::ShEnt.name(),
        Fault::Count.name(),
        Fault::Off.name(),
        Fault::Str.name(),
        Fault::Magic.name(),
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
    TS_EPHENT == SEG_BASE + 0x01
        && TS_ESHENT == SEG_BASE + 0x02
        && TS_ECNT == SEG_BASE + 0x03
        && TS_EOFF == SEG_BASE + 0x04
        && TS_ESTR == SEG_BASE + 0x05
        && TS_EMAGIC == SEG_BASE + 0x06
        && TS_ESTATE == SEG_BASE + 0x07
        && TS_EVER == SEG_BASE + 0x08
        && TS_EBAL == SEG_BASE + 0x09
        && TS_ESLOT == SEG_BASE + 0x0a
        && TS_EVALUE == SEG_BASE + 0x0b
        && TS_ESEAL == SEG_BASE + 0x0c
        && TS_ENOBS == SEG_BASE + 0x0d
        && TS_EMAGIC != TS_EOFF
}

/// 几何算术。
fn geom_ok() -> bool {
    let e = Ehdr::classic();
    let a = phdr_bytes(&e) == Some(3 * 56) && phdr_end(&e) == Some(0x40 + 3 * 56);
    let b = shdr_bytes(&e) == Some(5 * 64) && shdr_end(&e) == Some(0x200 + 5 * 64);
    let c = phdr_at(&e, 0) == Some(0x40) && phdr_at(&e, 2) == Some(0x40 + 2 * 56);
    let d = phdr_at(&e, 3).is_none() && shdr_at(&e, 5).is_none();
    let f = shdr_at(&e, 4) == Some(0x200 + 4 * 64);
    let g = tables_within(&e);
    let h = Ehdr::new(ELFCLASS64, 0, u16::MAX, u32::MAX, 0, u16::MAX, u32::MAX, 0, 0, 0);
    let i = phdr_bytes(&h).is_some() && phdr_end(&h).is_some() && !tables_within(&h);
    a && b && c && d && f && g && i
}

/// 正件通过面。
fn positive_ok() -> bool {
    let a = check(&Ehdr::classic()) == Fault::Ok;
    let b = check(&Ehdr::pie()) == Fault::Ok;
    let c = check(&Ehdr::bare()) == Fault::Ok;
    let d = check(&Ehdr::of(0, PHENT, 1, 0, 0, 0, 0)) == Fault::Ok;
    let e = check(&Ehdr::of(0, PHENT, 0, 0, 0, 0, 0)) == Fault::Ok;
    let f = check(&Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 3, 0)) == Fault::Ok;
    let g = check(&Ehdr::of(0x40, PHENT, 1, 0x300, SHENT, 1, 0)) == Fault::Ok;
    a && b && c && d && e && f && g
}

/// 表项悖谬。
fn phent_ok() -> bool {
    let a = check(&Ehdr::of(0x40, 0x40, 3, 0, 0, 0, 0)) == Fault::PhEnt;
    let b = check(&Ehdr::of(0x40, 0, 3, 0, 0, 0, 0)) == Fault::PhEnt;
    let c = check(&Ehdr::of(0x40, 0x38, 3, 0, 0, 0, 0)) == Fault::Ok;
    let d = check(&Ehdr::of(0, PHENT, 0, 0, 0, 0, 0)) == Fault::Ok;
    let e = Fault::PhEnt.code() == TS_EPHENT && Fault::PhEnt.errno() == EINVAL;
    a && b && c && d && e
}

/// 节项悖谬。
fn shent_ok() -> bool {
    let a = check(&Ehdr::of(0x40, PHENT, 1, 0x200, 0x20, 5, 0)) == Fault::ShEnt;
    let b = check(&Ehdr::of(0x40, PHENT, 1, 0x200, 0, 5, 0)) == Fault::ShEnt;
    let c = check(&Ehdr::of(0, 0, 0, 0, 0, 0, 0)) == Fault::Ok;
    let d = check(&Ehdr::of(0, 0, 0, 0, 0, 5, 0)) == Fault::ShEnt;
    let e = Fault::ShEnt.code() == TS_ESHENT;
    a && b && c && d && e
}

/// 计数与幻影表。
fn count_ok() -> bool {
    let a = check(&Ehdr::of(0x40, PHENT, 0x10000, 0, 0, 0, 0)) == Fault::Count;
    let b = check(&Ehdr::of(0x40, PHENT, 0, 0, 0, 0, 0)) == Fault::Count;
    let c = check(&Ehdr::of(0, 0, 0, 0x200, 0, 0, 0)) == Fault::Count;
    let d = check(&Ehdr::of(0, 0, 0, 0, 0, 0, 0)) == Fault::Ok;
    let e = check(&Ehdr::new(ELFCLASS64, 0x40, PHENT, PN_XNUM, 0, 0, 0, 0, 0x1000, 0)) == Fault::Off;
    let f = Fault::Count.code() == TS_ECNT && Fault::Count.errno() == EINVAL;
    a && b && c && d && e && f
}

/// 区间越界。
fn span_ok() -> bool {
    let a = check(&Ehdr::new(ELFCLASS64, 0x3f0, PHENT, 3, 0, 0, 0, 0, 0x400, 0)) == Fault::Off;
    let b = check(&Ehdr::new(ELFCLASS64, 0x40, PHENT, 3, 0x3f0, SHENT, 5, 0, 0x400, 0)) == Fault::Off;
    let c = check(&Ehdr::new(ELFCLASS64, 0x40, PHENT, 3, 0x200, SHENT, 5, 0, 0x400, 0)) == Fault::Ok;
    let d = check(&Ehdr::new(ELFCLASS64, 0, u16::MAX, u32::MAX, 0, 0, 0, 0, 0, 0)) == Fault::Count;
    let e = Fault::Off.code() == TS_EOFF;
    a && b && c && d && e
}

/// 索引越界。
fn index_ok() -> bool {
    let a = check(&Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 5, 9)) == Fault::Str;
    let b = check(&Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 5, 4)) == Fault::Ok;
    let c = check(&Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 5, SHN_XINDEX)) == Fault::Str;
    let d = check(&Ehdr::of(0, 0, 0, 0x200, SHENT, 5, 7)) == Fault::Str;
    let e = check(&Ehdr::of(0, 0, 0, 0x200, SHENT, 5, 0)) == Fault::Ok;
    let f = Fault::Str.code() == TS_ESTR;
    a && b && c && d && e && f
}

/// 类别门。
fn class_ok() -> bool {
    let bad = Ehdr::new(1, 0x40, PHENT, 3, 0x200, SHENT, 5, 4, FILE_LEN, 0x401000);
    let a = check(&bad) == Fault::Magic;
    let b = Fault::Magic.code() == TS_EMAGIC && Fault::Magic.errno() == ENOEXEC;
    let c = check(&Ehdr::classic()) == Fault::Ok;
    let d = verdict(&bad);
    let e = !d.ok() && d.fault == Fault::Magic && d.code == TS_EMAGIC && d.errno == ENOEXEC;
    a && b && c && e
}

/// 文案。
fn message_ok() -> bool {
    let a = message(Fault::PhEnt, &Ehdr::of(0x40, 0x40, 3, 0, 0, 0, 0)).as_str()
        == "./prog: bad ELF header: e_phentsize=0x40 != 0x38\n";
    let b = message(Fault::ShEnt, &Ehdr::of(0x40, PHENT, 1, 0x200, 0x20, 5, 0)).as_str()
        == "./prog: bad ELF header: e_shentsize=0x20 != 0x40\n";
    let c = message(Fault::Count, &Ehdr::of(0x40, PHENT, 0x10000, 0, 0, 0, 0)).as_str()
        == "./prog: bad ELF header: e_phnum=0x10000 exceeds PN_XNUM\n";
    let d = message(Fault::Count, &Ehdr::of(0x40, PHENT, 0, 0, 0, 0, 0)).as_str()
        == "./prog: bad ELF header: e_phnum=0 but e_phoff=0x40\n";
    let e = message(Fault::Count, &Ehdr::of(0, 0, 0, 0x200, 0, 0, 0)).as_str()
        == "./prog: bad ELF header: e_shnum=0 but e_shoff=0x200\n";
    let f = message(Fault::Magic, &Ehdr::new(1, 0, 0, 0, 0, 0, 0, 0, 0, 0)).as_str()
        == "./prog: bad ELF header: e_ident class=1 (want 2)\n";
    a && b && c && d && e && f
}

/// 后半文案。
fn tail_message_ok() -> bool {
    let a = message(
        Fault::Off,
        &Ehdr::new(ELFCLASS64, 0x3f0, PHENT, 3, 0, 0, 0, 0, 0x400, 0),
    )
    .as_str()
        == "./prog: bad ELF header: program headers end=0x498 past file len=0x400\n";
    let b = message(
        Fault::Off,
        &Ehdr::new(ELFCLASS64, 0x40, PHENT, 3, 0x3f0, SHENT, 5, 0, 0x400, 0),
    )
    .as_str()
        == "./prog: bad ELF header: section headers end=0x530 past file len=0x400\n";
    let c = message(Fault::Str, &Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 5, 9)).as_str()
        == "./prog: bad ELF header: e_shstrndx=0x9 >= e_shnum=0x5\n";
    let d = message(Fault::Ok, &Ehdr::classic()).as_str()
        == "ok: phnum=3 phentsize=0x38 shnum=5 shentsize=0x40\n";
    a && b && c && d
}

/// 语料与汇总。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.phent == 1 && r.shent == 1 && r.cnt == 1 && r.off == 1;
    let c = r.str == 1 && r.magic == 1;
    let d = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let e = table_ok() && unique() && notes_ok();
    a && b && c && d && e
}

/// 槽号与覆盖。
fn slots_ok() -> bool {
    let a = match slot_at(0) {
        Some(s) => s.fault == Fault::Ok && s.ehdr == Ehdr::classic(),
        None => false,
    };
    let b = match slot_at(13) {
        Some(s) => s.fault == Fault::Magic && s.ehdr.class == 1,
        None => false,
    };
    let c = slot_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(0) == Err(TS_ENOBS);
    let e = short(Fault::Off) == "table-span" && short(Fault::Ok) == "ok";
    a && b && c && d && e
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, TS_EPHENT))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == CORPUS as u32 - 1 && r.rate() == 929;
    let b = r.gate() == Err(TS_EVER) && !r.green();
    let c = run_with(Some((CORPUS, TS_EPHENT))).err() == Some(TS_ESLOT);
    a && b && c
}

/// 校验器。
fn checker_ok() -> bool {
    let mut c = Checker::new();
    let a = !c.bound() && c.steps() == 0;
    let b = match c.check(&Ehdr::classic()) {
        Ok(v) => v == Fault::Ok && c.last() == 0,
        Err(_) => false,
    };
    let d = c.check(&Ehdr::classic()).err() == Some(TS_ESTATE);
    let e = c.bound() && c.steps() == 1 && c.last() == TS_ESTATE;
    a && b && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && geom_ok();
    out[1] = positive_ok() && phent_ok() && shent_ok();
    out[2] = count_ok() && span_ok() && index_ok() && class_ok();
    out[3] = message_ok() && tail_message_ok() && corpus_ok() && slots_ok()
        && reds_ok() && checker_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11614_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11614.phentsize_paradox");

    s.add(
        "TS01-名称与段基址",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "TS02-表几何算术",
        {
            geom_ok()
        },
        "static",
    );

    s.add(
        "TS03-正件通过面",
        {
            positive_ok()
        },
        "static",
    );

    s.add(
        "TS04-表项悖谬",
        {
            phent_ok()
        },
        "static",
    );

    s.add(
        "TS05-节项悖谬",
        {
            shent_ok()
        },
        "static",
    );

    s.add(
        "TS06-计数与幻影表",
        {
            count_ok()
        },
        "static",
    );

    s.add(
        "TS07-表区间越界",
        {
            span_ok()
        },
        "static",
    );

    s.add(
        "TS08-节名索引",
        {
            index_ok()
        },
        "static",
    );

    s.add(
        "TS09-类别门",
        {
            class_ok()
        },
        "static",
    );

    s.add(
        "TS10-文案逐字",
        {
            message_ok() && tail_message_ok()
        },
        "static",
    );

    s.add(
        "TS11-语料全跑",
        {
            corpus_ok() && slots_ok()
        },
        "static",
    );

    s.add(
        "TS12-汇总红档与校验器",
        {
            reds_ok() && checker_ok()
        },
        "static",
    );

    s.add(
        "TS13-冻结重放四注入",
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
        panic!("unx11614 test fail")
    }

    #[test]
    fn geom_math() {
        let e = Ehdr::classic();
        assert_eq!(phdr_bytes(&e), Some(3 * 56));
        assert_eq!(phdr_end(&e), Some(0x40 + 3 * 56));
        assert_eq!(shdr_bytes(&e), Some(5 * 64));
        assert_eq!(shdr_end(&e), Some(0x200 + 5 * 64));
        assert_eq!(phdr_at(&e, 0), Some(0x40));
        assert_eq!(phdr_at(&e, 2), Some(0x40 + 112));
        assert!(phdr_at(&e, 3).is_none());
        assert_eq!(shdr_at(&e, 4), Some(0x200 + 256));
        assert!(shdr_at(&e, 5).is_none());
        assert!(tables_within(&e));
    }

    #[test]
    fn classic_ok() {
        assert_eq!(check(&Ehdr::classic()), Fault::Ok);
        assert_eq!(check(&Ehdr::pie()), Fault::Ok);
        assert_eq!(verdict(&Ehdr::classic()).code, 0);
    }

    #[test]
    fn no_phdr_ok() {
        assert_eq!(check(&Ehdr::bare()), Fault::Ok);
        assert_eq!(check(&Ehdr::of(0, PHENT, 0, 0, 0, 0, 0)), Fault::Ok);
    }

    #[test]
    fn minimal_ok() {
        assert_eq!(check(&Ehdr::of(0, PHENT, 1, 0, 0, 0, 0)), Fault::Ok);
        assert_eq!(check(&Ehdr::of(0x40, PHENT, 1, 0, 0, 0, 0)), Fault::Ok);
    }

    #[test]
    fn max_shnum_ok() {
        let e = Ehdr::of(0, 0, 0, 0x200, SHENT, 0xffff, 0);
        assert_eq!(check(&e), Fault::Ok);
        assert_eq!(shdr_bytes(&e), Some(0xffffu64 * 64));
    }

    #[test]
    fn phent_fault() {
        assert_eq!(check(&Ehdr::of(0x40, 0x40, 3, 0, 0, 0, 0)), Fault::PhEnt);
        assert_eq!(check(&Ehdr::of(0x40, 0, 3, 0, 0, 0, 0)), Fault::PhEnt);
        assert_eq!(Fault::PhEnt.code(), TS_EPHENT);
    }

    #[test]
    fn phent_zero_phnum_ok() {
        assert_eq!(check(&Ehdr::of(0, 0x20, 0, 0, 0, 0, 0)), Fault::PhEnt);
        assert_eq!(check(&Ehdr::of(0, PHENT, 0, 0, 0, 0, 0)), Fault::Ok);
    }

    #[test]
    fn shent_fault() {
        assert_eq!(check(&Ehdr::of(0x40, PHENT, 1, 0x200, 0x20, 5, 0)), Fault::ShEnt);
        assert_eq!(check(&Ehdr::of(0, 0, 0, 0, 0, 5, 0)), Fault::ShEnt);
        assert_eq!(Fault::ShEnt.code(), TS_ESHENT);
    }

    #[test]
    fn count_xnum() {
        assert_eq!(check(&Ehdr::of(0x40, PHENT, 0x10000, 0, 0, 0, 0)), Fault::Count);
        assert_eq!(check(&Ehdr::of(0x40, PHENT, PN_XNUM, 0, 0, 0, 0)), Fault::Ok);
        assert_eq!(
            check(&Ehdr::new(ELFCLASS64, 0x40, PHENT, PN_XNUM, 0, 0, 0, 0, 0x1000, 0)),
            Fault::Off
        );
    }

    #[test]
    fn phantom_phoff() {
        assert_eq!(check(&Ehdr::of(0x40, PHENT, 0, 0, 0, 0, 0)), Fault::Count);
    }

    #[test]
    fn phantom_shoff() {
        assert_eq!(check(&Ehdr::of(0, 0, 0, 0x200, 0, 0, 0)), Fault::Count);
    }

    #[test]
    fn off_phdr() {
        let e = Ehdr::new(ELFCLASS64, 0x3f0, PHENT, 3, 0, 0, 0, 0, 0x400, 0);
        assert_eq!(check(&e), Fault::Off);
        assert_eq!(phdr_end(&e), Some(0x498));
    }

    #[test]
    fn off_shdr() {
        let e = Ehdr::new(ELFCLASS64, 0x40, PHENT, 3, 0x3f0, SHENT, 5, 0, 0x400, 0);
        assert_eq!(check(&e), Fault::Off);
        assert_eq!(shdr_end(&e), Some(0x530));
    }

    #[test]
    fn str_range() {
        assert_eq!(check(&Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 5, 5)), Fault::Str);
        assert_eq!(check(&Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 5, 4)), Fault::Ok);
        assert_eq!(check(&Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 5, 0)), Fault::Ok);
    }

    #[test]
    fn xindex_reject() {
        assert_eq!(check(&Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 5, SHN_XINDEX)), Fault::Str);
    }

    #[test]
    fn class_fault() {
        let e = Ehdr::new(1, 0x40, PHENT, 3, 0x200, SHENT, 5, 4, FILE_LEN, 0x401000);
        assert_eq!(check(&e), Fault::Magic);
        assert_eq!(Fault::Magic.errno(), ENOEXEC);
    }

    #[test]
    fn order_phent_before_off() {
        let e = Ehdr::new(ELFCLASS64, 0x3f0, 0x40, 3, 0, 0, 0, 0, 0x400, 0);
        assert_eq!(check(&e), Fault::PhEnt);
    }

    #[test]
    fn order_count_before_entsize() {
        let e = Ehdr::of(0x40, 0x40, 0x10000, 0, 0, 0, 0);
        assert_eq!(check(&e), Fault::Count);
    }

    #[test]
    fn message_verbatim() {
        assert_eq!(
            message(Fault::PhEnt, &Ehdr::of(0x40, 0x40, 3, 0, 0, 0, 0)).as_str(),
            "./prog: bad ELF header: e_phentsize=0x40 != 0x38\n"
        );
        assert_eq!(
            message(Fault::ShEnt, &Ehdr::of(0x40, PHENT, 1, 0x200, 0x20, 5, 0)).as_str(),
            "./prog: bad ELF header: e_shentsize=0x20 != 0x40\n"
        );
        assert_eq!(
            message(Fault::Ok, &Ehdr::classic()).as_str(),
            "ok: phnum=3 phentsize=0x38 shnum=5 shentsize=0x40\n"
        );
    }

    #[test]
    fn message_tail() {
        assert_eq!(
            message(Fault::Count, &Ehdr::of(0x40, PHENT, 0x10000, 0, 0, 0, 0)).as_str(),
            "./prog: bad ELF header: e_phnum=0x10000 exceeds PN_XNUM\n"
        );
        assert_eq!(
            message(Fault::Str, &Ehdr::of(0x40, PHENT, 1, 0x200, SHENT, 5, 9)).as_str(),
            "./prog: bad ELF header: e_shstrndx=0x9 >= e_shnum=0x5\n"
        );
        assert_eq!(
            message(Fault::Magic, &Ehdr::new(1, 0, 0, 0, 0, 0, 0, 0, 0, 0)).as_str(),
            "./prog: bad ELF header: e_ident class=1 (want 2)\n"
        );
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
        assert_eq!(r.phent, 1);
        assert_eq!(r.shent, 1);
        assert_eq!(r.cnt, 1);
        assert_eq!(r.off, 1);
        assert_eq!(r.str, 1);
        assert_eq!(r.magic, 1);
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
            "phent corpus=14 accept=8 reject=6 phent=1 shent=1 cnt=1 off=1 str=1 magic=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, TS_EPHENT))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(TS_EVER));
        assert_eq!(run_with(Some((14, TS_EPHENT))).err(), Some(TS_ESLOT));
    }

    #[test]
    fn checker_once() {
        let mut c = Checker::new();
        match c.check(&Ehdr::classic()) {
            Ok(v) => assert_eq!(v, Fault::Ok),
            Err(_) => fail(),
        }
        assert_eq!(c.check(&Ehdr::classic()).err(), Some(TS_ESTATE));
        assert_eq!(c.steps(), 1);
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
        assert_eq!(l.register(Sample::blank()).err(), Some(TS_ESEAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(TS_ESEAL));
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
        assert_eq!(Ehdr::classic().seal(), Ehdr::classic().seal());
        assert_ne!(Ehdr::classic().seal(), Ehdr::pie().seal());
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0).as_str(), "0x0");
        assert_eq!(hex_text(0x38).as_str(), "0x38");
        assert_eq!(hex_text(0x10000).as_str(), "0x10000");
    }

    #[test]
    fn slot_bounds() {
        assert!(slot_at(0).is_some());
        assert!(slot_at(CORPUS - 1).is_some());
        assert!(slot_at(CORPUS).is_none());
        assert_eq!(short(Fault::Ok), "ok");
        assert_eq!(short(Fault::Count), "count");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11614_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
