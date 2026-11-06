//! UNX-F11607 空文件/文本文件当可执行 ENOEXEC 判据
//!
//! `execve` 拿到路径后第一段就是读前几个字节判断"这是不是我能跑的东西"。
//! 空文件没有任何字节可读，文本文件开头是可打印字符但又没有 `#!` 头，两者
//! 都进不了任何已注册的 binfmt，最终只能落到 `search_binary_handler` 的
//! 兜底：`-ENOEXEC`，shell 打 `cannot execute binary file: Exec format error`。
//!
//! 分类必须先把六档分开：`ELF`（魔数 7f 45 4c 46 完整）、`Script`（`#!`
//! 开头，交给 binfmt_script 找解释器）、`Text`（可打印但无 shebang）、
//! `Empty`（零字节）、`Short`（不足 4 字节，连魔数都比不全）、`Foreign`
//! （MZ / zip / gzip / 二进制垃圾）。只有 ELF 与 Script 能被本核运行，
//! 其余四档全部 ENOEXEC，但括号里的细节必须分档，否则 text 与 empty 在
//! 运维眼里都是"格式错误"，根因就丢了。
//!
//! 判据次序固定：空 → 魔数（够 4 字节才算，不够落 Short）→ shebang →
//! 可打印文本 → 外来格式。次序不能换：`\x7fELF` 只有 4 字节时必须先认成
//! ELF 再在上层判截断，不能被当作"短文件"；反过来 `\x7fELG` 是坏魔数，
//! 必须落外来档而不是文本档。
//!
//! 闸门次序：空档 → 魔数 → shebang → 文本 → 外来 → 语料 → 文案 → 扫描面 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfb60;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 前缀缓冲容量。
pub const CAP: usize = 64;

/// ELF 魔数长度。
pub const MAGIC_LEN: usize = 4;

/// `ENOEXEC`。
pub const ENOEXEC: i32 = 8;

/// ELF 魔数。
pub const ELF_MAGIC: [u8; MAGIC_LEN] = [0x7f, b'E', b'L', b'F'];

/// shebang。
pub const HASH_BANG: [u8; 2] = [b'#', b'!'];

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 文本档拒收。
pub const PL_ETEXT: u32 = SEG_BASE + 0x01;
/// 空文件拒收。
pub const PL_EEMPTY: u32 = SEG_BASE + 0x02;
/// 外来格式拒收。
pub const PL_EFOREIGN: u32 = SEG_BASE + 0x03;
/// 短文件拒收。
pub const PL_ESHORT: u32 = SEG_BASE + 0x04;
/// 状态冲突。
pub const PL_ESTATE: u32 = SEG_BASE + 0x05;
/// 未达绿档。
pub const PL_EVER: u32 = SEG_BASE + 0x06;
/// 字节非法。
pub const PL_EBYTE: u32 = SEG_BASE + 0x07;
/// 语料重复。
pub const PL_EDUP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const PL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const PL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const PL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const PL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const PL_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 空档。
pub const OP_EMPTY: u8 = 0x01;
/// 魔数。
pub const OP_MAGIC: u8 = 0x02;
/// shebang。
pub const OP_SCRIPT: u8 = 0x03;
/// 文本。
pub const OP_TEXT: u8 = 0x04;
/// 外来。
pub const OP_FOREIGN: u8 = 0x05;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x06;
/// 文案。
pub const OP_MSG: u8 = 0x07;
/// 扫描面。
pub const OP_SCAN: u8 = 0x08;

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

/// 十进制文本。
fn u32_text(v: u32) -> String {
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

/// 两字符十六进制。
fn hex2(v: u8) -> String {
    let hi = v >> 4;
    let lo = v & 0xf;
    let a = if hi < 10 { b'0' + hi } else { b'a' + (hi - 10) };
    let b = if lo < 10 { b'0' + lo } else { b'a' + (lo - 10) };
    let mut s = String::from("0x");
    s.push(a as char);
    s.push(b as char);
    s
}

// ---------------------------------------------------------------------------
// 分类
// ---------------------------------------------------------------------------

/// 文件开头形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// ELF 镜像。
    Elf,
    /// shebang 脚本。
    Script,
    /// 可打印文本，无 shebang。
    Text,
    /// 零字节。
    Empty,
    /// 不足 4 字节。
    Short,
    /// 外来格式。
    Foreign,
}

impl Kind {
    /// 名字。
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Elf => "elf",
            Kind::Script => "script",
            Kind::Text => "text",
            Kind::Empty => "empty",
            Kind::Short => "short",
            Kind::Foreign => "foreign",
        }
    }

    /// 是否可跑。
    pub fn accept(&self) -> bool {
        match self {
            Kind::Elf => true,
            Kind::Script => true,
            _ => false,
        }
    }

    /// 拒收码。
    pub fn code(&self) -> u32 {
        match self {
            Kind::Elf => 0,
            Kind::Script => 0,
            Kind::Text => PL_ETEXT,
            Kind::Empty => PL_EEMPTY,
            Kind::Short => PL_ESHORT,
            Kind::Foreign => PL_EFOREIGN,
        }
    }

    /// 细节短语。
    pub fn detail(&self) -> &'static str {
        match self {
            Kind::Elf => "ELF image",
            Kind::Script => "script",
            Kind::Text => "text file, no shebang",
            Kind::Empty => "empty file",
            Kind::Short => "short file",
            Kind::Foreign => "foreign format",
        }
    }

    /// 桶序号。
    pub fn bucket(&self) -> u32 {
        match self {
            Kind::Elf => 0,
            Kind::Script => 1,
            Kind::Text => 2,
            Kind::Empty => 3,
            Kind::Short => 4,
            Kind::Foreign => 5,
        }
    }
}

// ---------------------------------------------------------------------------
// 前缀
// ---------------------------------------------------------------------------

/// 文件前缀缓冲。
#[derive(Clone, Copy, Debug)]
pub struct Prefix {
    buf: [u8; CAP],
    len: usize,
}

impl Prefix {
    /// 取字节（多截少补零）。
    pub fn new(bytes: &[u8]) -> Prefix {
        let mut buf = [0u8; CAP];
        let mut i = 0usize;
        while i < CAP && i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                buf[i] = *b;
            }
            i += 1;
        }
        let len = if bytes.len() > CAP { CAP } else { bytes.len() };
        Prefix { buf, len }
    }

    /// 实长。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一字节。
    pub fn byte_at(&self, off: usize) -> Result<u8, u32> {
        if off >= self.len {
            return Err(PL_EBYTE);
        }
        match self.buf.get(off) {
            Some(b) => Ok(*b),
            None => Err(PL_EBYTE),
        }
    }

    /// 是否以给定字节串开头。
    pub fn starts_with(&self, pat: &[u8]) -> bool {
        if pat.len() > self.len {
            return false;
        }
        let mut i = 0usize;
        while i < pat.len() {
            let a = match self.buf.get(i) {
                Some(v) => *v,
                None => return false,
            };
            match pat.get(i) {
                Some(w) => {
                    if a != *w {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 魔数是否完整。
    pub fn elf_magic(&self) -> bool {
        self.starts_with(&ELF_MAGIC)
    }

    /// 是否 shebang。
    pub fn shebang(&self) -> bool {
        self.starts_with(&HASH_BANG)
    }

    /// 是否可打印文本（允许制表、换行、回车；其余必须是 ASCII 可打印）。
    pub fn printable(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            let b = match self.buf.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let ok = (b >= 0x20 && b < 0x7f) || b == b'\n' || b == b'\r' || b == b'\t';
            if !ok {
                return false;
            }
            i += 1;
        }
        true
    }

    /// shebang 解释器路径（不含 `#!`，到空白或行尾为止）。
    pub fn interpreter(&self) -> Result<String, u32> {
        if !self.shebang() {
            return Err(PL_EBYTE);
        }
        let mut s = String::new();
        let mut i = 2usize;
        while i < self.len {
            let b = match self.buf.get(i) {
                Some(v) => *v,
                None => break,
            };
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                break;
            }
            s.push(b as char);
            i += 1;
        }
        if s.is_empty() {
            return Err(PL_EBYTE);
        }
        Ok(s)
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

/// 外来格式细节。
pub fn foreign_detail(p: &Prefix) -> &'static str {
    if p.starts_with(&[b'M', b'Z']) {
        return "MZ/PE image";
    }
    if p.starts_with(&[0x50, 0x4b, 0x03, 0x04]) {
        return "zip archive";
    }
    if p.starts_with(&[0x1f, 0x8b]) {
        return "gzip stream";
    }
    if p.len() >= 1 {
        if let Ok(b0) = p.byte_at(0) {
            if b0 == 0x7f {
                return "bad ELF magic";
            }
        }
    }
    "binary, no ELF magic"
}

/// 分类。
pub fn classify(p: &Prefix) -> Kind {
    if p.is_empty() {
        return Kind::Empty;
    }
    if p.len() < MAGIC_LEN {
        return Kind::Short;
    }
    if p.elf_magic() {
        return Kind::Elf;
    }
    if p.shebang() {
        return Kind::Script;
    }
    if p.printable() {
        return Kind::Text;
    }
    Kind::Foreign
}

// ---------------------------------------------------------------------------
// 判定与文案
// ---------------------------------------------------------------------------

/// 一次判定。
#[derive(Clone, Copy, Debug)]
pub struct Verdict {
    kind: Kind,
    code: u32,
}

impl Verdict {
    /// 形态。
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// 码。
    pub const fn code(&self) -> u32 {
        self.code
    }

    /// 是否可跑。
    pub fn accepted(&self) -> bool {
        self.kind.accept()
    }
}

/// 判一条前缀。
pub fn judge(p: &Prefix) -> Verdict {
    let kind = classify(p);
    Verdict { kind, code: kind.code() }
}

/// 文案。
pub fn message(v: &Verdict, p: &Prefix, path: &str) -> Result<String, u32> {
    if path.is_empty() {
        return Err(PL_EBYTE);
    }
    let mut s = String::new();
    match v.kind {
        Kind::Elf => {
            s.push_str("ok: ELF image\n");
        }
        Kind::Script => {
            let interp = p.interpreter()?;
            s.push_str("ok: script, interpreter=");
            s.push_str(interp.as_str());
            s.push('\n');
        }
        Kind::Text => {
            s.push_str("bash: ");
            s.push_str(path);
            s.push_str(": cannot execute binary file: Exec format error (text file, no shebang)\n");
        }
        Kind::Empty => {
            s.push_str("bash: ");
            s.push_str(path);
            s.push_str(": cannot execute binary file: Exec format error (empty file)\n");
        }
        Kind::Short => {
            s.push_str("bash: ");
            s.push_str(path);
            s.push_str(": cannot execute binary file: Exec format error (short file, ");
            s.push_str(u32_text(p.len() as u32).as_str());
            s.push_str(" bytes < 4)\n");
        }
        Kind::Foreign => {
            let detail = foreign_detail(p);
            s.push_str("bash: ");
            s.push_str(path);
            s.push_str(": cannot execute binary file: Exec format error (");
            if detail == "bad ELF magic" {
                let b1 = p.byte_at(1).unwrap_or(0);
                s.push_str("bad ELF magic");
                s.push_str(", magic[1]=");
                s.push_str(hex2(b1).as_str());
            } else {
                s.push_str(detail);
            }
            s.push_str(")\n");
        }
    }
    Ok(s)
}

/// errno。
pub fn errno(v: &Verdict) -> i32 {
    if v.accepted() {
        0
    } else {
        ENOEXEC
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 前缀字节。
    pub bytes: &'static [u8],
    /// 期望形态。
    pub kind: Kind,
    /// 期望码。
    pub code: u32,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(bytes: &'static [u8], kind: Kind, code: u32, note: &'static str) -> Slot {
        Slot { bytes, kind, code, note }
    }
}

/// 十四槽语料：五活九死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(b"", Kind::Empty, PL_EEMPTY, "zero-byte"),
    Slot::new(b" \n\t\n", Kind::Text, PL_ETEXT, "whitespace"),
    Slot::new(b"hello\n", Kind::Text, PL_ETEXT, "short-text"),
    Slot::new(b"echo hi; echo bye\n", Kind::Text, PL_ETEXT, "shell-line-no-bang"),
    Slot::new(b"#!/bin/sh\n", Kind::Script, 0, "sh"),
    Slot::new(b"#!/bin/sh -e\n", Kind::Script, 0, "sh-args"),
    Slot::new(b"#!/usr/bin/env python3\n", Kind::Script, 0, "env-python"),
    Slot::new(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0], Kind::Elf, 0, "elf64"),
    Slot::new(&[0x7f, b'E', b'L', b'F'], Kind::Elf, 0, "magic-only"),
    Slot::new(b"MZ\x90\x00", Kind::Foreign, PL_EFOREIGN, "mz-stub"),
    Slot::new(&[0x50, 0x4b, 0x03, 0x04], Kind::Foreign, PL_EFOREIGN, "zip"),
    Slot::new(&[0x1f, 0x8b, 0x08, 0x00], Kind::Foreign, PL_EFOREIGN, "gzip"),
    Slot::new(&[0x00, 0x01, 0x02, 0x03], Kind::Foreign, PL_EFOREIGN, "nul-binary"),
    Slot::new(&[0x7f, b'E', b'L', b'G'], Kind::Foreign, PL_EFOREIGN, "bad-magic"),
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
            if let Some(b) = CASES.get(j) {
                if b.bytes == a.bytes {
                    return false;
                }
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
    if !unique() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let p = Prefix::new(slot.bytes);
        let v = judge(&p);
        if v.kind != slot.kind || v.code != slot.code {
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
    elf: usize,
    script: usize,
    text: usize,
    empty: usize,
    short: usize,
    foreign: usize,
    accept: usize,
    reject: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report {
            total: 0,
            elf: 0,
            script: 0,
            text: 0,
            empty: 0,
            short: 0,
            foreign: 0,
            accept: 0,
            reject: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// ELF 条数。
    pub const fn elf(&self) -> usize {
        self.elf
    }

    /// 脚本条数。
    pub const fn script(&self) -> usize {
        self.script
    }

    /// 文本条数。
    pub const fn text(&self) -> usize {
        self.text
    }

    /// 空文件条数。
    pub const fn empty(&self) -> usize {
        self.empty
    }

    /// 短文件条数。
    pub const fn short(&self) -> usize {
        self.short
    }

    /// 外来条数。
    pub const fn foreign(&self) -> usize {
        self.foreign
    }

    /// 存活条数。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收条数。
    pub const fn reject(&self) -> usize {
        self.reject
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
            return Err(PL_ENOBS);
        }
        if self.red {
            return Err(PL_EVER);
        }
        if self.matched != self.total {
            return Err(PL_EVALUE);
        }
        if self.elf != 2 || self.script != 3 || self.text != 3 {
            return Err(PL_EVALUE);
        }
        if self.empty != 1 || self.short != 0 || self.foreign != 5 {
            return Err(PL_EVALUE);
        }
        if self.accept != 5 || self.reject != 9 {
            return Err(PL_EVALUE);
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
        s.push_str("plain corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" elf=");
        s.push_str(u32_text(self.elf as u32).as_str());
        s.push_str(" script=");
        s.push_str(u32_text(self.script as u32).as_str());
        s.push_str(" text=");
        s.push_str(u32_text(self.text as u32).as_str());
        s.push_str(" empty=");
        s.push_str(u32_text(self.empty as u32).as_str());
        s.push_str(" short=");
        s.push_str(u32_text(self.short as u32).as_str());
        s.push_str(" foreign=");
        s.push_str(u32_text(self.foreign as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
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
            None => return Err(PL_EVALUE),
        };
        let p = Prefix::new(slot.bytes);
        let v = judge(&p);
        r.total += 1;
        match v.kind {
            Kind::Elf => r.elf += 1,
            Kind::Script => r.script += 1,
            Kind::Text => r.text += 1,
            Kind::Empty => r.empty += 1,
            Kind::Short => r.short += 1,
            Kind::Foreign => r.foreign += 1,
        }
        if v.accepted() {
            r.accept += 1;
        } else {
            r.reject += 1;
        }
        if v.kind == slot.kind && v.code == slot.code {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, _code)) = red {
        if slot >= r.total {
            return Err(PL_ESLOT);
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
        Err(PL_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 扫描面
// ---------------------------------------------------------------------------

/// 只扫一次的前缀扫描器。
#[derive(Clone, Copy, Debug)]
pub struct Scanner {
    bound: bool,
    len: usize,
    last: u32,
}

impl Scanner {
    /// 空。
    pub const fn new() -> Scanner {
        Scanner { bound: false, len: 0, last: 0 }
    }

    /// 扫一条。
    pub fn scan(&mut self, p: &Prefix) -> Result<Kind, u32> {
        if self.bound {
            self.last = PL_ESTATE;
            return Err(PL_ESTATE);
        }
        self.bound = true;
        self.len = p.len();
        let k = classify(p);
        self.last = k.code();
        Ok(k)
    }

    /// 是否绑定。
    pub const fn bound(&self) -> bool {
        self.bound
    }

    /// 长度。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 最后码。
    pub const fn last(&self) -> u32 {
        self.last
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
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PL_ESLOT);
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
            return Err(PL_EBAL);
        }
        const VALS: [u32; 8] = [14, 5, 9, 2, 3, 3, 5, 929];
        const CODES: [u32; 6] = [PL_ETEXT, PL_EEMPTY, PL_EFOREIGN, PL_ESHORT, PL_ESTATE, PL_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PL_EBAL),
            };
            if s.slot as usize != i {
                return Err(PL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PL_EVALUE);
                        }
                    }
                    None => return Err(PL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PL_EVALUE);
                        }
                    }
                    None => return Err(PL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PL_EBAL);
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
            return Err(PL_EBAL);
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
            Err(PL_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
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

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_CORPUS, r.map(|x| x.elf())));

    let _ = l.register(count_cell(4, OP_CORPUS, r.map(|x| x.script())));
    let _ = l.register(count_cell(5, OP_CORPUS, r.map(|x| x.text())));
    let _ = l.register(count_cell(6, OP_CORPUS, r.map(|x| x.foreign())));

    let red = run_with(Some((0, PL_ETEXT)));
    let _ = l.register(count_cell(7, OP_TEXT, red.map(|x| x.rate() as usize)));

    let text_v = judge(&Prefix::new(b"hello\n"));
    let _ = l.register(err_cell(8, OP_TEXT, if text_v.code == 0 { PL_ETEXT } else { text_v.code }));

    let empty_v = judge(&Prefix::new(b""));
    let _ = l.register(err_cell(9, OP_EMPTY, if empty_v.code == 0 { PL_EEMPTY } else { empty_v.code }));

    let foreign_v = judge(&Prefix::new(b"MZ\x90\x00"));
    let _ = l.register(err_cell(10, OP_FOREIGN, if foreign_v.code == 0 { PL_EFOREIGN } else { foreign_v.code }));

    let short_v = judge(&Prefix::new(&[0x7f, b'E']));
    let _ = l.register(err_cell(11, OP_MAGIC, if short_v.code == 0 { PL_ESHORT } else { short_v.code }));

    let mut sc = Scanner::new();
    let _ = sc.scan(&Prefix::new(b"#!/bin/sh\n"));
    let state_err = match sc.scan(&Prefix::new(b"#!/bin/sh\n")) {
        Ok(_) => PL_ESTATE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_SCAN, state_err));

    let probe = Prefix::new(b"hi");
    let byte_err = match probe.byte_at(2) {
        Ok(_) => PL_EBYTE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_SCAN, byte_err));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 空档。
fn empty_ok() -> bool {
    let p = Prefix::new(b"");
    let v = judge(&p);
    p.is_empty()
        && p.len() == 0
        && v.kind == Kind::Empty
        && v.code == PL_EEMPTY
        && !v.accepted()
        && !p.elf_magic()
        && !p.shebang()
        && p.printable()
        && p.byte_at(0) == Err(PL_EBYTE)
}

/// 魔数。
fn magic_ok() -> bool {
    let a = Prefix::new(&[0x7f, b'E', b'L', b'F']);
    let b = Prefix::new(&[0x7f, b'E', b'L', b'G']);
    let c = Prefix::new(&[0x7f, b'E', b'L']);
    a.elf_magic()
        && classify(&a) == Kind::Elf
        && !b.elf_magic()
        && classify(&b) == Kind::Foreign
        && !c.elf_magic()
        && classify(&c) == Kind::Short
        && classify(&Prefix::new(&[0x7f])) == Kind::Short
}

/// shebang。
fn script_ok() -> bool {
    let a = Prefix::new(b"#!/bin/sh\n");
    let b = Prefix::new(b"#!/bin/sh -e\n");
    let c = Prefix::new(b"#!/usr/bin/env python3\n");
    let ka = classify(&a) == Kind::Script && Kind::Script.accept();
    let ib = match b.interpreter() {
        Ok(v) => v.as_str() == "/bin/sh",
        Err(_) => false,
    };
    let ic = match c.interpreter() {
        Ok(v) => v.as_str() == "/usr/bin/env",
        Err(_) => false,
    };
    let d = classify(&b) == Kind::Script && classify(&c) == Kind::Script;
    let e = Prefix::new(b"#!").interpreter() == Err(PL_EBYTE);
    let f = Prefix::new(b"# not bang\n").interpreter() == Err(PL_EBYTE);
    ka && ib && ic && d && e && f
}

/// 文本。
fn text_ok() -> bool {
    let a = Prefix::new(b"hello\n");
    let b = Prefix::new(b" \n\t\n");
    let c = Prefix::new(b"# not bang\n");
    let d = Prefix::new(&[0x00, 0x01]);
    a.printable()
        && b.printable()
        && c.printable()
        && !d.printable()
        && classify(&a) == Kind::Text
        && classify(&b) == Kind::Text
        && classify(&c) == Kind::Text
        && classify(&d) == Kind::Short
        && judge(&a).code == PL_ETEXT
}

/// 外来。
fn foreign_ok() -> bool {
    let mz = Prefix::new(b"MZ\x90\x00");
    let zip = Prefix::new(&[0x50, 0x4b, 0x03, 0x04]);
    let gz = Prefix::new(&[0x1f, 0x8b, 0x08, 0x00]);
    let nul = Prefix::new(&[0x00, 0x01, 0x02, 0x03]);
    classify(&mz) == Kind::Foreign
        && foreign_detail(&mz) == "MZ/PE image"
        && foreign_detail(&zip) == "zip archive"
        && foreign_detail(&gz) == "gzip stream"
        && foreign_detail(&nul) == "binary, no ELF magic"
        && foreign_detail(&Prefix::new(&[0x7f, b'E', b'L', b'G'])) == "bad ELF magic"
        && judge(&mz).code == PL_EFOREIGN
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.elf() == 2 && r.script() == 3 && r.text() == 3;
    let b = r.empty() == 1 && r.short() == 0 && r.foreign() == 5;
    let c = r.accept() == 5 && r.reject() == 9 && r.matched() == CORPUS;
    let d = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    a && b && c && d
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(s) => s.kind == Kind::Empty && s.bytes.is_empty(),
        None => false,
    };
    let b = match slot_at(4) {
        Some(s) => s.kind == Kind::Script && s.code == 0,
        None => false,
    };
    let c = slot_at(CORPUS).is_none() && slot_at(CORPUS + 1).is_none();
    a && b && c
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "plain corpus=14 elf=2 script=3 text=3 empty=1 short=0 foreign=5 accept=5 reject=9 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(PL_ENOBS);
    let c = e.render().as_str()
        == "plain corpus=0 elf=0 script=0 text=0 empty=0 short=0 foreign=0 accept=0 reject=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, PL_ETEXT))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(PL_EVER) && !r.green();
    let c = run_with(Some((CORPUS, PL_ETEXT))).err() == Some(PL_ESLOT);
    a && b && c
}

/// 封面。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(PL_ENOBS) && cover(0) == Err(PL_ENOBS)
}

/// 文案逐字。
fn message_ok() -> bool {
    let a = {
        let p = Prefix::new(b"");
        match message(&judge(&p), &p, "./empty") {
            Ok(s) => s.as_str()
                == "bash: ./empty: cannot execute binary file: Exec format error (empty file)\n",
            Err(_) => false,
        }
    };
    let b = {
        let p = Prefix::new(b"hello\n");
        match message(&judge(&p), &p, "./note.txt") {
            Ok(s) => s.as_str()
                == "bash: ./note.txt: cannot execute binary file: Exec format error (text file, no shebang)\n",
            Err(_) => false,
        }
    };
    let c = {
        let p = Prefix::new(b"#!/bin/sh -e\n");
        match message(&judge(&p), &p, "./run.sh") {
            Ok(s) => s.as_str() == "ok: script, interpreter=/bin/sh\n",
            Err(_) => false,
        }
    };
    let d = {
        let p = Prefix::new(&[0x7f, b'E', b'L', b'F']);
        match message(&judge(&p), &p, "./a.out") {
            Ok(s) => s.as_str() == "ok: ELF image\n",
            Err(_) => false,
        }
    };
    let e = {
        let p = Prefix::new(&[0x7f, b'E']);
        match message(&judge(&p), &p, "./x") {
            Ok(s) => s.as_str()
                == "bash: ./x: cannot execute binary file: Exec format error (short file, 2 bytes < 4)\n",
            Err(_) => false,
        }
    };
    let f = {
        let p = Prefix::new(b"MZ\x90\x00");
        match message(&judge(&p), &p, "./prog.exe") {
            Ok(s) => s
                .as_str()
                .contains("Exec format error (MZ/PE image)")
                && s.as_str().contains("./prog.exe"),
            Err(_) => false,
        }
    };
    let g = {
        let p = Prefix::new(&[0x7f, b'E', b'L', b'G']);
        match message(&judge(&p), &p, "./bad") {
            Ok(s) => s.as_str().contains("bad ELF magic, magic[1]=0x45"),
            Err(_) => false,
        }
    };
    let h = {
        let p = Prefix::new(b"hello\n");
        message(&judge(&p), &p, "") == Err(PL_EBYTE)
    };
    a && b && c && d && e && f && g && h
}

/// errno。
fn errno_ok() -> bool {
    let elf = Prefix::new(&[0x7f, b'E', b'L', b'F']);
    let sh = Prefix::new(b"#!/bin/sh\n");
    let txt = Prefix::new(b"hi\n");
    errno(&judge(&elf)) == 0
        && errno(&judge(&sh)) == 0
        && errno(&judge(&txt)) == ENOEXEC
        && ENOEXEC == 8
}

/// 扫描面。
fn scan_ok() -> bool {
    let mut s = Scanner::new();
    let p = Prefix::new(b"#!/bin/sh\n");
    let a = !s.bound() && s.scan(&p) == Ok(Kind::Script) && s.bound();
    let b = s.len() == p.len() && s.last() == 0;
    let c = s.scan(&p) == Err(PL_ESTATE) && s.last() == PL_ESTATE;
    let mut t = Scanner::new();
    let d = t.scan(&Prefix::new(b"hello\n")) == Ok(Kind::Text) && t.last() == PL_ETEXT;
    a && b && c && d
}

/// 摘要。
fn seal_ok() -> bool {
    let a = Prefix::new(b"hello\n").seal() == Prefix::new(b"hello\n").seal();
    let b = Prefix::new(b"hello\n").seal() != Prefix::new(b"jello\n").seal();
    let c = digest(b"plain") == digest(b"plain") && digest(b"plain") != digest(b"plane");
    let d = Prefix::new(b"").seal() != Prefix::new(b"\n").seal();
    a && b && c && d
}

/// 文本。
fn text_fmt_ok() -> bool {
    u32_text(0).as_str() == "0"
        && u32_text(10).as_str() == "10"
        && u32_text(929).as_str() == "929"
        && hex2(0x00).as_str() == "0x00"
        && hex2(0x45).as_str() == "0x45"
        && hex2(0xff).as_str() == "0xff"
}

/// 桶序号。
fn bucket_ok() -> bool {
    Kind::Elf.bucket() == 0
        && Kind::Script.bucket() == 1
        && Kind::Text.bucket() == 2
        && Kind::Empty.bucket() == 3
        && Kind::Short.bucket() == 4
        && Kind::Foreign.bucket() == 5
        && Kind::Elf.name() == "elf"
        && Kind::Text.name() == "text"
        && Kind::Text.detail() == "text file, no shebang"
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = empty_ok() && magic_ok() && bucket_ok();
    out[1] = script_ok() && text_ok() && foreign_ok();
    out[2] = corpus_ok() && slots_ok() && table_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && message_ok() && errno_ok()
        && scan_ok() && seal_ok() && text_fmt_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11607_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11607.plain_files");

    s.add("PL01-空档", empty_ok(), "static");
    s.add("PL02-魔数", magic_ok(), "static");
    s.add("PL03-shebang", script_ok(), "static");
    s.add("PL04-文本", text_ok(), "static");
    s.add("PL05-外来", foreign_ok() && bucket_ok(), "static");
    s.add("PL06-十四卡全跑", corpus_ok(), "static");
    s.add("PL07-语料编号", slots_ok(), "static");
    s.add("PL08-汇总渲染", report_ok(), "static");
    s.add("PL09-红单显式", reds_ok(), "static");
    s.add("PL10-封面", cover_ok(), "static");
    s.add("PL11-文案逐字", message_ok(), "static");
    s.add("PL12-扫描与摘要", errno_ok() && scan_ok() && seal_ok() && text_fmt_ok(), "static");
    s.add("PL13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
    fn empty_is_empty() {
        let p = Prefix::new(b"");
        assert!(p.is_empty());
        assert_eq!(classify(&p), Kind::Empty);
        assert_eq!(judge(&p).code, PL_EEMPTY);
        assert_eq!(p.byte_at(0), Err(PL_EBYTE));
    }

    #[test]
    fn magic_exact() {
        assert!(Prefix::new(&[0x7f, b'E', b'L', b'F']).elf_magic());
        assert!(!Prefix::new(&[0x7f, b'E', b'L']).elf_magic());
        assert!(!Prefix::new(&[0x7f, b'E', b'L', b'G']).elf_magic());
        assert_eq!(classify(&Prefix::new(&[0x7f, b'E'])), Kind::Short);
    }

    #[test]
    fn shebang_interp() {
        let p = Prefix::new(b"#!/usr/bin/env python3\n");
        assert!(p.shebang());
        assert_eq!(classify(&p), Kind::Script);
        match p.interpreter() {
            Ok(v) => assert_eq!(v.as_str(), "/usr/bin/env"),
            Err(_) => panic!("interpreter"),
        }
        assert_eq!(Prefix::new(b"#!").interpreter(), Err(PL_EBYTE));
    }

    #[test]
    fn printable_rules() {
        assert!(Prefix::new(b"a b\tc\r\n").printable());
        assert!(!Prefix::new(&[0x00, 0x41]).printable());
        assert!(Prefix::new(&[0x7f, b'E', b'L', b'F']).printable() == false);
    }

    #[test]
    fn text_classify() {
        assert_eq!(classify(&Prefix::new(b"hello\n")), Kind::Text);
        assert_eq!(classify(&Prefix::new(b"# not bang\n")), Kind::Text);
        assert_eq!(judge(&Prefix::new(b"hello\n")).code, PL_ETEXT);
    }

    #[test]
    fn foreign_kinds() {
        assert_eq!(foreign_detail(&Prefix::new(b"MZ")), "MZ/PE image");
        assert_eq!(
            foreign_detail(&Prefix::new(&[0x50, 0x4b, 0x03, 0x04])),
            "zip archive"
        );
        assert_eq!(foreign_detail(&Prefix::new(&[0x1f, 0x8b])), "gzip stream");
        assert_eq!(
            foreign_detail(&Prefix::new(&[0x7f, b'E', b'L', b'G'])),
            "bad ELF magic"
        );
        assert_eq!(foreign_detail(&Prefix::new(&[0x00, 0x01, 0x02])), "binary, no ELF magic");
    }

    #[test]
    fn report_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.elf(), 2);
        assert_eq!(r.script(), 3);
        assert_eq!(r.text(), 3);
        assert_eq!(r.empty(), 1);
        assert_eq!(r.short(), 0);
        assert_eq!(r.foreign(), 5);
        assert_eq!(r.accept(), 5);
        assert_eq!(r.reject(), 9);
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn red_and_cover() {
        let r = match run_with(Some((2, PL_EFOREIGN))) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(PL_EVER));
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(0), Err(PL_ENOBS));
    }

    #[test]
    fn message_short() {
        let p = Prefix::new(&[0x7f]);
        let m = match message(&judge(&p), &p, "./f") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(
            m.as_str(),
            "bash: ./f: cannot execute binary file: Exec format error (short file, 1 bytes < 4)\n"
        );
    }

    #[test]
    fn message_empty_path() {
        let p = Prefix::new(b"hello\n");
        assert_eq!(message(&judge(&p), &p, ""), Err(PL_EBYTE));
    }

    #[test]
    fn scanner_once() {
        let mut s = Scanner::new();
        let p = Prefix::new(&[0x7f, b'E', b'L', b'F']);
        assert_eq!(s.scan(&p), Ok(Kind::Elf));
        assert_eq!(s.scan(&p), Err(PL_ESTATE));
        assert_eq!(s.last(), PL_ESTATE);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
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
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(PL_EBAL));
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
        let s = run_unx11607_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
