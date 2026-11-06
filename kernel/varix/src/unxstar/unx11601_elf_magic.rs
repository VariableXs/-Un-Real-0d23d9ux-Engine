//! UNX-F11601 ELF 魔数损坏 execve ENOEXEC 判据
//!
//! execve 的第一道门只认四个字节：`7f 45 4c 46`。这四个字节对不上，内核
//! 就不再往下看任何字段——e_ident 的 class、data、osabi 全都无从谈起，直接
//! 回 `ENOEXEC`，shell 侧落成 `cannot execute binary file: Exec format
//! error`。本模块把这四个字节的判定做成可对账的判据面：位置、期望值、实际
//! 值、以及「差一个字节」的边界（`7f 45 4c 45`）全部显式留痕。
//!
//! 判据分四档：
//!   · ELF     —— 四字节全对，exec 继续走后续头校验；
//!   · CORRUPT —— 四字节内某一位不符，报错带上位置与两值；
//!   · SHORT   —— 不足四字节，没法判魔数，同样落 ENOEXEC；
//!   · TEXT    —— 前四字节全是可打印字符（或 `#!` 开头），按文本文件处理。
//!
//! 文本档不是「格式错误」的同义词：文本文件在 execve 里同样落 ENOEXEC，
//! 但错误文案要能指出它是文本而非二进制，免得用户拿着十六进制视图找魔数。
//!
//! 闸门次序：字节 → 名称 → 魔数 → 判定 → 文案 → 语料 → 错误码 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfb00;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// ELF 魔数。
pub const MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// 头部前缀容量。
pub const IDENT_CAP: usize = 16;

/// 名称容量。
pub const NAME_CAP: usize = 16;

/// 语料条数。
pub const CORPUS: usize = 16;

/// 语料内 ELF 占比。
pub const ELF_N: usize = 3;

/// 语料内损坏占比。
pub const CORRUPT_N: usize = 8;

/// 语料内短头占比。
pub const SHORT_N: usize = 3;

/// 语料内文本占比。
pub const TEXT_N: usize = 2;

/// `ENOEXEC`。
pub const ENOEXEC: i32 = 8;

/// `EACCES`。
pub const EACCES: i32 = 13;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 字节非法。
pub const EL_EBYTE: u32 = SEG_BASE + 0x01;
/// 名称为空。
pub const EL_ENAME: u32 = SEG_BASE + 0x02;
/// 语料编号重复。
pub const EL_EDUP: u32 = SEG_BASE + 0x03;
/// 超长。
pub const EL_ELEN: u32 = SEG_BASE + 0x04;
/// 魔数不符。
pub const EL_EMAGIC: u32 = SEG_BASE + 0x05;
/// 头部不足四字节。
pub const EL_ESHORT: u32 = SEG_BASE + 0x06;
/// 状态冲突。
pub const EL_ESTATE: u32 = SEG_BASE + 0x07;
/// 未达绿档。
pub const EL_EVER: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const EL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const EL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const EL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const EL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const EL_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 头部装载。
pub const OP_HEAD: u8 = 0x01;
/// 魔数比对。
pub const OP_MAGIC: u8 = 0x02;
/// 判定。
pub const OP_JUDGE: u8 = 0x03;
/// 文案。
pub const OP_MSG: u8 = 0x04;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x05;
/// 错误码映射。
pub const OP_ERRNO: u8 = 0x06;

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
// 渲染
// ---------------------------------------------------------------------------

/// 追加一个无符号十进制数。
fn push_dec(out: &mut String, v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut n = v;
    let mut buf = [0u8; 10];
    let mut len = 0usize;
    while n > 0 {
        let d = (n % 10) as u8;
        if let Some(dst) = buf.get_mut(len) {
            *dst = b'0' + d;
        }
        n /= 10;
        len += 1;
    }
    while len > 0 {
        len -= 1;
        if let Some(b) = buf.get(len) {
            out.push(*b as char);
        }
    }
}

/// 追加一个两位十六进制字节（小写，带 `0x` 前缀）。
fn push_hex(out: &mut String, b: u8) {
    const HEX: [u8; 16] = *b"0123456789abcdef";
    out.push_str("0x");
    if let Some(h) = HEX.get((b >> 4) as usize) {
        out.push(*h as char);
    }
    if let Some(l) = HEX.get((b & 0x0f) as usize) {
        out.push(*l as char);
    }
}

// ---------------------------------------------------------------------------
// 名称
// ---------------------------------------------------------------------------

/// 定长名称。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAME_CAP],
    len: usize,
}

impl Name {
    /// 装载：可打印 ASCII，16 字节内。
    pub fn new(bytes: &[u8]) -> Result<Name, u32> {
        if bytes.is_empty() {
            return Err(EL_ENAME);
        }
        if bytes.len() > NAME_CAP {
            return Err(EL_ELEN);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < bytes.len() {
            let b = match bytes.get(i) {
                Some(v) => *v,
                None => return Err(EL_ENOBS),
            };
            if b < 0x20 || b == 0x7f || b >= 0x80 {
                return Err(EL_EBYTE);
            }
            if let Some(dst) = buf.get_mut(i) {
                *dst = b;
            }
            i += 1;
        }
        Ok(Name { buf, len: bytes.len() })
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节视图。
    pub fn bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    /// 逐字节比对。
    pub fn same(&self, other: &[u8]) -> bool {
        if self.len != other.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            match (self.buf.get(i), other.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 文本视图。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 头部前缀
// ---------------------------------------------------------------------------

/// 文件头部前若干字节（最多 16）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Head {
    buf: [u8; IDENT_CAP],
    len: usize,
}

impl Head {
    /// 装载：0..=16 字节，不校验内容。
    pub fn new(raw: &[u8]) -> Result<Head, u32> {
        if raw.len() > IDENT_CAP {
            return Err(EL_ELEN);
        }
        let mut buf = [0u8; IDENT_CAP];
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(v) => *v,
                None => return Err(EL_ENOBS),
            };
            if let Some(dst) = buf.get_mut(i) {
                *dst = b;
            }
            i += 1;
        }
        Ok(Head { buf, len: raw.len() })
    }

    /// 实际字节数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一字节；越界为 None。
    pub fn byte(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.buf.get(i).copied()
    }

    /// 摘要。
    pub fn sum(&self) -> u64 {
        digest(self.bytes())
    }

    /// 字节视图。
    pub fn bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    /// 是否四字节齐备。
    pub fn full4(&self) -> bool {
        self.len >= 4
    }

    /// 第一处不符的偏移；齐备且全对为 None。
    pub fn mismatch(&self) -> Option<u8> {
        let mut i = 0usize;
        while i < 4 {
            match (self.byte(i), MAGIC.get(i)) {
                (Some(b), Some(w)) => {
                    if b != *w {
                        return Some(i as u8);
                    }
                }
                _ => return Some(i as u8),
            }
            i += 1;
        }
        None
    }

    /// 四字节全对。
    pub fn magic_ok(&self) -> bool {
        self.full4() && self.mismatch().is_none()
    }

    /// 四字节位置上的实际值（缺位为 0）。
    pub fn seen(&self, pos: u8) -> u8 {
        self.byte(pos as usize).unwrap_or(0)
    }

    /// 魔数闸门：短头落 SHORT，不符落 MAGIC。
    pub fn check_magic(&self) -> Result<(), u32> {
        if !self.full4() {
            return Err(EL_ESHORT);
        }
        match self.mismatch() {
            Some(_) => Err(EL_EMAGIC),
            None => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 判定档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 四字节全对。
    Elf,
    /// 四字节内不符。
    Corrupt,
    /// 不足四字节。
    Short,
    /// 文本文件。
    Text,
}

impl Kind {
    /// 档码：0 绿 / 1 损坏 / 2 短头 / 3 文本。
    pub fn code(&self) -> u8 {
        match self {
            Kind::Elf => 0,
            Kind::Corrupt => 1,
            Kind::Short => 2,
            Kind::Text => 3,
        }
    }

    /// 档名。
    pub fn text(&self) -> &'static str {
        match self {
            Kind::Elf => "ELF",
            Kind::Corrupt => "CORRUPT",
            Kind::Short => "SHORT",
            Kind::Text => "TEXT",
        }
    }

    /// 由档码建档。
    pub fn of(code: u8) -> Option<Kind> {
        match code {
            0 => Some(Kind::Elf),
            1 => Some(Kind::Corrupt),
            2 => Some(Kind::Short),
            3 => Some(Kind::Text),
            _ => None,
        }
    }
}

/// 判定结果：档、位置、位置上的实际字节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judgement {
    kind: Kind,
    pos: u8,
    seen: u8,
}

impl Judgement {
    /// 构造。
    pub fn new(kind: Kind, pos: u8, seen: u8) -> Judgement {
        Judgement { kind, pos, seen }
    }

    /// 档。
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// 档码。
    pub fn kind_code(&self) -> u8 {
        self.kind.code()
    }

    /// 位置（短头档为实际长度）。
    pub fn pos(&self) -> u8 {
        self.pos
    }

    /// 位置上的实际字节。
    pub fn seen(&self) -> u8 {
        self.seen
    }

    /// 是否可执行。
    pub fn executable(&self) -> bool {
        self.kind == Kind::Elf
    }
}

/// 可打印字节。
fn printable(b: u8) -> bool {
    (b >= 0x20 && b <= 0x7e) || b == b'\n' || b == b'\r' || b == b'\t'
}

/// 文本特征：`#!` 开头，或前四字节全可打印。
fn text_like(h: &Head) -> bool {
    if h.byte(0) == Some(b'#') && h.byte(1) == Some(b'!') {
        return true;
    }
    let mut i = 0usize;
    while i < 4 {
        match h.byte(i) {
            Some(b) => {
                if !printable(b) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 判定：短头优先，其次文本，其次魔数，最后通过。
pub fn judge(h: &Head) -> Judgement {
    if !h.full4() {
        return Judgement::new(Kind::Short, h.len() as u8, 0);
    }
    if text_like(h) {
        return Judgement::new(Kind::Text, 0, h.seen(0));
    }
    match h.mismatch() {
        Some(p) => Judgement::new(Kind::Corrupt, p, h.seen(p)),
        None => Judgement::new(Kind::Elf, 0, h.seen(0)),
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 判定文案：损坏带位置与两值，短头带长度，文本显式标注。
pub fn message(j: Judgement) -> String {
    let mut s = String::new();
    match j.kind {
        Kind::Elf => {
            s.push_str("ok: ELF64 executable accepted (magic 7f 45 4c 46)");
            s
        }
        Kind::Corrupt => {
            s.push_str("bash: ./a.out: cannot execute binary file: Exec format error");
            s.push_str(" (magic[");
            push_dec(&mut s, j.pos as u32);
            s.push_str("]=");
            push_hex(&mut s, j.seen);
            s.push_str(" want ");
            match MAGIC.get(j.pos as usize) {
                Some(w) => push_hex(&mut s, *w),
                None => s.push_str("??"),
            }
            s.push(')');
            s
        }
        Kind::Short => {
            s.push_str("bash: ./a.out: cannot execute binary file: Exec format error");
            s.push_str(" (header ");
            push_dec(&mut s, j.pos as u32);
            s.push_str(" bytes < 4)");
            s
        }
        Kind::Text => {
            s.push_str("bash: ./a.out: cannot execute binary file: Exec format error");
            s.push_str(" (text file, not ELF)");
            s
        }
    }
}

/// 错误码映射：魔数问题一律 ENOEXEC。
pub fn errno(j: Judgement) -> i32 {
    match j.kind {
        Kind::Elf => 0,
        Kind::Corrupt => ENOEXEC,
        Kind::Short => ENOEXEC,
        Kind::Text => ENOEXEC,
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一条语料：编号、标签、原始字节、期望档、期望位置。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 编号（1 起）。
    pub no: u8,
    /// 标签。
    pub tag: &'static [u8],
    /// 原始头部字节。
    pub bytes: &'static [u8],
    /// 期望档码。
    pub want: u8,
    /// 期望位置。
    pub want_pos: u8,
}

impl Slot {
    /// 标签名。
    pub fn name(&self) -> Result<Name, u32> {
        Name::new(self.tag)
    }

    /// 判定本条。
    pub fn judge(&self) -> Result<Judgement, u32> {
        let h = Head::new(self.bytes)?;
        Ok(judge(&h))
    }

    /// 是否命中期望。
    pub fn hit(&self) -> Result<bool, u32> {
        let j = self.judge()?;
        Ok(j.kind_code() == self.want && j.pos() == self.want_pos)
    }
}

/// 十六卡语料：三 ELF、八损坏、三短头、两文本。
pub static CORPUS16: [Slot; CORPUS] = [
    Slot {
        no: 1,
        tag: b"elf64_full",
        bytes: &[0x7f, 0x45, 0x4c, 0x46, 0x02, 0x01, 0x01, 0x00],
        want: 0,
        want_pos: 0,
    },
    Slot {
        no: 2,
        tag: b"elf32_full",
        bytes: &[0x7f, 0x45, 0x4c, 0x46, 0x01, 0x01, 0x01, 0x00],
        want: 0,
        want_pos: 0,
    },
    Slot {
        no: 3,
        tag: b"elf_pie",
        bytes: &[0x7f, 0x45, 0x4c, 0x46, 0x02, 0x01, 0x01, 0x03],
        want: 0,
        want_pos: 0,
    },
    Slot {
        no: 4,
        tag: b"bad_magic0",
        bytes: &[0x00, 0x45, 0x4c, 0x46, 0x02, 0x01, 0x01, 0x00],
        want: 1,
        want_pos: 0,
    },
    Slot {
        no: 5,
        tag: b"bad_magic1",
        bytes: &[0x7f, 0x00, 0x4c, 0x46, 0x02, 0x01, 0x01, 0x00],
        want: 1,
        want_pos: 1,
    },
    Slot {
        no: 6,
        tag: b"bad_magic2",
        bytes: &[0x7f, 0x45, 0x00, 0x46, 0x02, 0x01, 0x01, 0x00],
        want: 1,
        want_pos: 2,
    },
    Slot {
        no: 7,
        tag: b"bad_magic3",
        bytes: &[0x7f, 0x45, 0x4c, 0x00, 0x02, 0x01, 0x01, 0x00],
        want: 1,
        want_pos: 3,
    },
    Slot {
        no: 8,
        tag: b"mz_stub",
        bytes: &[0x4d, 0x5a, 0x90, 0x00, 0x03, 0x00, 0x00, 0x00],
        want: 1,
        want_pos: 0,
    },
    Slot {
        no: 9,
        tag: b"zip_archive",
        bytes: &[0x50, 0x4b, 0x03, 0x04, 0x14, 0x00, 0x00, 0x00],
        want: 1,
        want_pos: 0,
    },
    Slot {
        no: 10,
        tag: b"gzip_stream",
        bytes: &[0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00],
        want: 1,
        want_pos: 0,
    },
    Slot {
        no: 11,
        tag: b"text_plain",
        bytes: &[0x61, 0x62, 0x63, 0x0a, 0x64, 0x65, 0x66, 0x0a],
        want: 3,
        want_pos: 0,
    },
    Slot {
        no: 12,
        tag: b"shebang_sh",
        bytes: &[0x23, 0x21, 0x2f, 0x62, 0x69, 0x6e, 0x2f, 0x73],
        want: 3,
        want_pos: 0,
    },
    Slot {
        no: 13,
        tag: b"short_empty",
        bytes: &[],
        want: 2,
        want_pos: 0,
    },
    Slot {
        no: 14,
        tag: b"short_one",
        bytes: &[0x7f],
        want: 2,
        want_pos: 1,
    },
    Slot {
        no: 15,
        tag: b"short_three",
        bytes: &[0x7f, 0x45, 0x4c],
        want: 2,
        want_pos: 3,
    },
    Slot {
        no: 16,
        tag: b"near_miss",
        bytes: &[0x7f, 0x45, 0x4c, 0x45, 0x02, 0x01, 0x01, 0x00],
        want: 1,
        want_pos: 3,
    },
];

/// 按编号取语料；0 与越界都为无观测。
pub fn corpus_no(no: u8) -> Result<&'static Slot, u32> {
    if no == 0 {
        return Err(EL_ENOBS);
    }
    match CORPUS16.get((no as usize) - 1) {
        Some(v) => Ok(v),
        None => Err(EL_ENOBS),
    }
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    total: usize,
    elf: usize,
    corrupt: usize,
    short: usize,
    text: usize,
    matched: usize,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report { total: 0, elf: 0, corrupt: 0, short: 0, text: 0, matched: 0 }
    }

    /// 总数。
    pub fn total(&self) -> usize {
        self.total
    }

    /// ELF 数。
    pub fn elf(&self) -> usize {
        self.elf
    }

    /// 损坏数。
    pub fn corrupt(&self) -> usize {
        self.corrupt
    }

    /// 短头数。
    pub fn short(&self) -> usize {
        self.short
    }

    /// 文本数。
    pub fn text(&self) -> usize {
        self.text
    }

    /// 命中数。
    pub fn matched(&self) -> usize {
        self.matched
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 命中率，千分比向上取整。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let n = (self.matched as u32).saturating_mul(1000);
        let t = self.total as u32;
        (n + t - 1) / t
    }

    /// 绿档闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS {
            return Err(EL_ENOBS);
        }
        if self.elf != ELF_N
            || self.corrupt != CORRUPT_N
            || self.short != SHORT_N
            || self.text != TEXT_N
        {
            return Err(EL_EMAGIC);
        }
        if self.matched != self.total {
            return Err(EL_EVER);
        }
        Ok(())
    }

    /// 是否绿。
    pub fn green(&self) -> bool {
        self.gate() == Ok(())
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("elf magic corpus=");
        push_dec(&mut s, self.total as u32);
        s.push_str(" elf=");
        push_dec(&mut s, self.elf as u32);
        s.push_str(" corrupt=");
        push_dec(&mut s, self.corrupt as u32);
        s.push_str(" short=");
        push_dec(&mut s, self.short as u32);
        s.push_str(" text=");
        push_dec(&mut s, self.text as u32);
        s.push_str(" match=");
        push_dec(&mut s, self.matched as u32);
        s.push_str(" rate=");
        push_dec(&mut s, self.rate());
        s.push_str("ppm\n");
        s
    }
}

/// 跑全部语料；`over` 可改写某索引的期望档码，用于红单注入。
fn run_with(over: Option<(usize, u8)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CORPUS {
        let slot = match CORPUS16.get(i) {
            Some(v) => v,
            None => return Err(EL_ENOBS),
        };
        if slot.no as usize != i + 1 {
            return Err(EL_ESTATE);
        }
        let j = slot.judge()?;
        r.total += 1;
        match j.kind() {
            Kind::Elf => r.elf += 1,
            Kind::Corrupt => r.corrupt += 1,
            Kind::Short => r.short += 1,
            Kind::Text => r.text += 1,
        }
        let want = match over {
            Some((idx, k)) if idx == i => k,
            _ => slot.want,
        };
        if j.kind_code() == want && j.pos() == slot.want_pos {
            r.matched += 1;
        }
        i += 1;
    }
    Ok(r)
}

/// 跑全量语料。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 覆盖闸门：必须满 16 条。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(EL_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 一次性判定器
// ---------------------------------------------------------------------------

/// 一次判定器：同一条头只许判一次。
#[derive(Clone, Copy, Debug)]
pub struct Guard {
    bound: bool,
    kind: u8,
    seen: u8,
}

impl Guard {
    /// 空判定器。
    pub const fn new() -> Guard {
        Guard { bound: false, kind: 0, seen: 0 }
    }

    /// 绑定一次判定；重复绑定落 ESTATE。
    pub fn bind(&mut self, j: Judgement) -> Result<(), u32> {
        if self.bound {
            return Err(EL_ESTATE);
        }
        self.bound = true;
        self.kind = j.kind_code();
        self.seen = j.seen();
        Ok(())
    }

    /// 是否已绑定。
    pub fn bound(&self) -> bool {
        self.bound
    }

    /// 已绑定的档码。
    pub fn kind_code(&self) -> u8 {
        self.kind
    }

    /// 已绑定的字节。
    pub fn seen(&self) -> u8 {
        self.seen
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
            return Err(EL_ESLOT);
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
            return Err(EL_EBAL);
        }
        const VALS: [u32; 8] = [16, 3, 8, 3, 2, 16, 8, 1];
        const CODES: [u32; 6] =
            [EL_EMAGIC, EL_ESHORT, EL_ELEN, EL_EBYTE, EL_ENOBS, EL_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EL_EBAL),
            };
            if s.slot as usize != i {
                return Err(EL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EL_EVALUE);
                        }
                    }
                    None => return Err(EL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EL_EVALUE);
                        }
                    }
                    None => return Err(EL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(EL_EBAL);
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
            return Err(EL_EBAL);
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
            Err(EL_ESEAL)
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

/// 文案锚点是否全过。
fn messages_ok() -> bool {
    let mut ok = true;
    let elf = match Head::new(&MAGIC) {
        Ok(h) => h,
        Err(_) => return false,
    };
    ok = ok && message(judge(&elf)).starts_with("ok: ELF64");
    let bad = match Head::new(&[0x7f, 0x00, 0x4c, 0x46]) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let m = message(judge(&bad));
    ok = ok && m.contains("cannot execute binary file: Exec format error");
    ok = ok && m.contains("magic[1]=0x00 want 0x45");
    let short = match Head::new(&[0x7f, 0x45, 0x4c]) {
        Ok(h) => h,
        Err(_) => return false,
    };
    ok = ok && message(judge(&short)).contains("header 3 bytes < 4");
    let text = match Head::new(b"abc\n") {
        Ok(h) => h,
        Err(_) => return false,
    };
    ok = ok && message(judge(&text)).contains("text file, not ELF");
    ok
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.elf())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.corrupt())));
    let _ = l.register(count_cell(3, OP_CORPUS, r.map(|x| x.short())));
    let _ = l.register(count_cell(4, OP_CORPUS, r.map(|x| x.text())));
    let _ = l.register(count_cell(5, OP_CORPUS, r.map(|x| x.matched())));

    let errno_cell = match Head::new(&[0x7f, 0x00, 0x4c, 0x46]) {
        Ok(h) => Ok(errno(judge(&h)) as usize),
        Err(c) => Err(c),
    };
    let _ = l.register(count_cell(6, OP_ERRNO, errno_cell));

    let msg_cell = if messages_ok() { Ok(1usize) } else { Ok(0usize) };
    let _ = l.register(count_cell(7, OP_MSG, msg_cell));

    let magic_err = match Head::new(&[0x7f, 0x00, 0x4c, 0x46]) {
        Ok(h) => h.check_magic().err().unwrap_or(EL_ENOBS),
        Err(c) => c,
    };
    let _ = l.register(err_cell(8, OP_MAGIC, if magic_err == 0 { EL_ENOBS } else { magic_err }));

    let short_err = match Head::new(&[0x7f]) {
        Ok(h) => h.check_magic().err().unwrap_or(EL_ENOBS),
        Err(c) => c,
    };
    let _ = l.register(err_cell(9, OP_HEAD, short_err));

    let long_err = match Head::new(&[0u8; IDENT_CAP + 1]) {
        Ok(_) => EL_ELEN,
        Err(c) => c,
    };
    let _ = l.register(err_cell(10, OP_HEAD, long_err));

    let byte_err = match Name::new(&[0x00]) {
        Ok(_) => EL_EBYTE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(11, OP_HEAD, byte_err));

    let noobs_err = match corpus_no(17) {
        Ok(_) => EL_ENOBS,
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_JUDGE, noobs_err));

    let mut g = Guard::new();
    let first = match Head::new(&MAGIC) {
        Ok(h) => judge(&h),
        Err(_) => Judgement::new(Kind::Short, 0, 0),
    };
    let _ = g.bind(first);
    let state_err = match g.bind(first) {
        Ok(()) => EL_ESTATE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_JUDGE, state_err));

    l
}

// ---------------------------------------------------------------------------
// 检查面
// ---------------------------------------------------------------------------

/// 头部装载与字节视图。
fn head_ok() -> bool {
    let h = match Head::new(&MAGIC) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = h.len() == 4 && !h.is_empty() && h.byte(1) == Some(0x45) && h.byte(4).is_none();
    let b = h.magic_ok() && h.mismatch().is_none() && h.check_magic() == Ok(());
    let c = match Head::new(&[0u8; IDENT_CAP + 1]) {
        Ok(_) => false,
        Err(k) => k == EL_ELEN,
    };
    let empty = match Head::new(b"") {
        Ok(e) => e.is_empty() && !e.full4() && e.check_magic() == Err(EL_ESHORT),
        Err(_) => false,
    };
    a && b && c && empty
}

/// 名称与字节闸门。
fn name_ok() -> bool {
    let n = match Name::new(b"bad_magic1") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = n.len() == 10 && !n.is_empty() && n.text() == String::from("bad_magic1");
    let b = n.same(b"bad_magic1") && !n.same(b"bad_magic2");
    let c = Name::new(b"").err() == Some(EL_ENAME);
    let d = Name::new(&[0u8; NAME_CAP + 1]).err() == Some(EL_ELEN);
    let e = Name::new(&[0x00]).err() == Some(EL_EBYTE);
    let f = Name::new(&[0x80]).err() == Some(EL_EBYTE);
    a && b && c && d && e && f
}

/// 魔数四位置与缺位。
fn magic_ok() -> bool {
    let mut i = 0usize;
    let mut pos_ok = true;
    while i < 4 {
        let mut raw = MAGIC;
        if let Some(dst) = raw.get_mut(i) {
            *dst = 0x00;
        }
        let h = match Head::new(&raw) {
            Ok(v) => v,
            Err(_) => return false,
        };
        pos_ok = pos_ok && h.mismatch() == Some(i as u8) && !h.magic_ok();
        pos_ok = pos_ok && h.check_magic() == Err(EL_EMAGIC);
        i += 1;
    }
    let short = match Head::new(&[0x7f, 0x45]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    pos_ok && short.mismatch() == Some(2) && short.check_magic() == Err(EL_ESHORT)
}

/// 判定表。
fn judge_ok() -> bool {
    let elf = match Head::new(&MAGIC) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = judge(&elf).kind() == Kind::Elf && judge(&elf).executable();
    let bad = match Head::new(&[0x7f, 0x00, 0x4c, 0x46]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = judge(&bad).kind() == Kind::Corrupt && judge(&bad).pos() == 1;
    let c2 = judge(&bad).seen() == 0x00 && judge(&bad).kind_code() == 1;
    let short = match Head::new(&[0x7f]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = judge(&short).kind() == Kind::Short && judge(&short).pos() == 1;
    let text = match Head::new(b"#!/bin/s") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let e = judge(&text).kind() == Kind::Text;
    let f = Kind::of(0) == Some(Kind::Elf) && Kind::of(1) == Some(Kind::Corrupt);
    let g = Kind::of(2) == Some(Kind::Short) && Kind::of(3) == Some(Kind::Text) && Kind::of(4).is_none();
    let h = Kind::Elf.text() == "ELF" && Kind::Text.text() == "TEXT" && Kind::Short.code() == 2;
    a && c && c2 && d && e && f && g && h
}

/// 文案锚点。
fn msg_ok() -> bool {
    if !messages_ok() {
        return false;
    }
    let bad = match Head::new(&[0x7f, 0x45, 0x00, 0x46]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let m = message(judge(&bad));
    let a = m.contains("magic[2]=0x00 want 0x4c");
    let text = match Head::new(b"abc\n") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = message(judge(&text)).starts_with("bash: ./a.out: cannot execute");
    let elf = match Head::new(&MAGIC) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = !message(judge(&elf)).contains("Exec format error");
    a && b && c
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.elf() == ELF_N && r.corrupt() == CORRUPT_N;
    let b = r.short() == SHORT_N && r.text() == TEXT_N && r.matched() == CORPUS;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    a && b && c
}

/// 语料编号与标签。
fn order_ok() -> bool {
    let mut i = 0usize;
    let mut ok = true;
    while i < CORPUS {
        let s = match CORPUS16.get(i) {
            Some(v) => v,
            None => return false,
        };
        ok = ok && s.no as usize == i + 1;
        ok = ok && s.bytes.len() <= IDENT_CAP;
        ok = ok && s.tag.len() <= NAME_CAP && !s.tag.is_empty();
        ok = ok && s.want <= 3;
        ok = ok && s.hit().unwrap_or(false);
        let mut j = i + 1;
        while j < CORPUS {
            if let Some(t) = CORPUS16.get(j) {
                ok = ok && s.tag != t.tag;
            }
            j += 1;
        }
        i += 1;
    }
    let d = corpus_no(0).err() == Some(EL_ENOBS) && corpus_no(17).err() == Some(EL_ENOBS);
    let e = match corpus_no(5) {
        Ok(s) => s.no == 5 && s.name().map(|n| n.same(b"bad_magic1")).unwrap_or(false),
        Err(_) => false,
    };
    ok && d && e
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "elf magic corpus=16 elf=3 corrupt=8 short=3 text=2 match=16 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.render().as_str()
        == "elf magic corpus=0 elf=0 corrupt=0 short=0 text=0 match=0 rate=0ppm\n";
    let c = e.gate() == Err(EL_ENOBS);
    a && b && c
}

/// 红单：一条期望被改写后必须掉出绿档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, 1))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.matched() != r.total();
    let b = r.rate() == 938;
    let c = r.gate() == Err(EL_EVER) && !r.green();
    let d = r.elf() == ELF_N && r.corrupt() == CORRUPT_N;
    a && b && c && d
}

/// 覆盖档。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(EL_ENOBS) && cover(0) == Err(EL_ENOBS)
}

/// 判定器与错误码。
fn bounds_ok() -> bool {
    let mut g = Guard::new();
    let j = Judgement::new(Kind::Corrupt, 1, 0x00);
    let a = !g.bound() && g.bind(j) == Ok(()) && g.bound();
    let b = g.kind_code() == 1 && g.seen() == 0x00;
    let c = g.bind(j) == Err(EL_ESTATE);
    let d = ENOEXEC == 8 && EACCES == 13;
    let e = errno(Judgement::new(Kind::Elf, 0, 0)) == 0;
    let f = errno(Judgement::new(Kind::Text, 0, 0)) == 8;
    a && b && c && d && e && f
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = head_ok() && name_ok();
    out[1] = magic_ok() && judge_ok();
    out[2] = msg_ok() && corpus_ok() && order_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && bounds_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11601_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11601.elf_magic");

    s.add(
        "EL01-头部装载",
        {
            head_ok()
        },
        "static",
    );

    s.add(
        "EL02-名称与字节",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "EL03-魔数四位置",
        {
            magic_ok()
        },
        "static",
    );

    s.add(
        "EL04-四档判定",
        {
            judge_ok()
        },
        "static",
    );

    s.add(
        "EL05-文案锚点",
        {
            msg_ok()
        },
        "static",
    );

    s.add(
        "EL06-十六卡全跑",
        {
            corpus_ok()
        },
        "static",
    );

    s.add(
        "EL07-语料编号",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "EL08-汇总渲染",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "EL09-红单显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "EL10-覆盖档",
        {
            cover_ok()
        },
        "static",
    );

    s.add(
        "EL11-判定器与错误码",
        {
            bounds_ok()
        },
        "static",
    );

    s.add(
        "EL12-账本对账",
        {
            build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "EL13-冻结重放四注入",
        {
            let four = inject_four();
            four[0] && four[1] && four[2] && four[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_shape() {
        let h = Head::new(&MAGIC).expect("h");
        assert_eq!(h.len(), 4);
        assert_eq!(h.byte(0), Some(0x7f));
        assert_eq!(h.byte(4), None);
        assert!(h.magic_ok());
        assert_eq!(h.check_magic(), Ok(()));
        assert_eq!(Head::new(&[0u8; 17]).err(), Some(EL_ELEN));
        assert!(Head::new(b"").expect("e").is_empty());
    }

    #[test]
    fn name_gates() {
        assert_eq!(Name::new(b"").err(), Some(EL_ENAME));
        assert_eq!(Name::new(&[0x7f]).err(), Some(EL_EBYTE));
        assert_eq!(Name::new(&[0u8; 17]).err(), Some(EL_ELEN));
        let n = Name::new(b"elf64_full").expect("n");
        assert_eq!(n.text().as_str(), "elf64_full");
        assert!(n.same(b"elf64_full"));
        assert!(!n.same(b"elf64_fulla"));
    }

    #[test]
    fn magic_positions() {
        for i in 0..4usize {
            let mut raw = MAGIC;
            raw[i] = 0x00;
            let h = Head::new(&raw).expect("h");
            assert_eq!(h.mismatch(), Some(i as u8));
            assert_eq!(h.check_magic(), Err(EL_EMAGIC));
        }
        let s = Head::new(&[0x7f]).expect("s");
        assert_eq!(s.mismatch(), Some(1));
        assert_eq!(s.check_magic(), Err(EL_ESHORT));
    }

    #[test]
    fn judge_table() {
        let elf = Head::new(&MAGIC).expect("e");
        assert_eq!(judge(&elf).kind(), Kind::Elf);
        let bad = Head::new(&[0x7f, 0x45, 0x4c, 0x45]).expect("b");
        assert_eq!(judge(&bad).kind(), Kind::Corrupt);
        assert_eq!(judge(&bad).pos(), 3);
        assert_eq!(judge(&bad).seen(), 0x45);
        let short = Head::new(&[0x7f, 0x45, 0x4c]).expect("s");
        assert_eq!(judge(&short).kind(), Kind::Short);
        assert_eq!(judge(&short).pos(), 3);
        let text = Head::new(b"abc\n").expect("t");
        assert_eq!(judge(&text).kind(), Kind::Text);
    }

    #[test]
    fn message_anchor() {
        let bad = Head::new(&[0x7f, 0x00, 0x4c, 0x46]).expect("b");
        let m = message(judge(&bad));
        assert!(m.contains("cannot execute binary file: Exec format error"));
        assert!(m.contains("magic[1]=0x00 want 0x45"));
        let short = Head::new(&[0x7f]).expect("s");
        assert!(message(judge(&short)).contains("header 1 bytes < 4"));
        let text = Head::new(b"#!/bin/s").expect("t");
        assert!(message(judge(&text)).contains("text file, not ELF"));
        let elf = Head::new(&MAGIC).expect("e");
        assert!(message(judge(&elf)).starts_with("ok: ELF64"));
    }

    #[test]
    fn corpus_run() {
        let r = run().expect("r");
        assert_eq!(r.total(), CORPUS);
        assert_eq!(r.elf(), ELF_N);
        assert_eq!(r.corrupt(), CORRUPT_N);
        assert_eq!(r.short(), SHORT_N);
        assert_eq!(r.text(), TEXT_N);
        assert_eq!(r.matched(), CORPUS);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(
            r.render().as_str(),
            "elf magic corpus=16 elf=3 corrupt=8 short=3 text=2 match=16 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_injection() {
        let r = run_with(Some((0, 1))).expect("r");
        assert_eq!(r.matched(), CORPUS - 1);
        assert_eq!(r.rate(), 938);
        assert_eq!(r.gate(), Err(EL_EVER));
        assert!(!r.green());
    }

    #[test]
    fn empty_report() {
        let r = Report::new();
        assert!(r.is_empty());
        assert_eq!(r.rate(), 0);
        assert_eq!(r.gate(), Err(EL_ENOBS));
        assert_eq!(
            r.render().as_str(),
            "elf magic corpus=0 elf=0 corrupt=0 short=0 text=0 match=0 rate=0ppm\n"
        );
    }

    #[test]
    fn coverage_gate() {
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS - 1), Err(EL_ENOBS));
        assert_eq!(cover(0), Err(EL_ENOBS));
    }

    #[test]
    fn guard_gates() {
        let mut g = Guard::new();
        let j = Judgement::new(Kind::Text, 0, b'a');
        assert!(!g.bound());
        assert_eq!(g.bind(j), Ok(()));
        assert!(g.bound());
        assert_eq!(g.kind_code(), 3);
        assert_eq!(g.seen(), b'a');
        assert_eq!(g.bind(j), Err(EL_ESTATE));
    }

    #[test]
    fn errno_map() {
        assert_eq!(ENOEXEC, 8);
        assert_eq!(EACCES, 13);
        assert_eq!(errno(Judgement::new(Kind::Elf, 0, 0)), 0);
        assert_eq!(errno(Judgement::new(Kind::Corrupt, 1, 0)), ENOEXEC);
        assert_eq!(errno(Judgement::new(Kind::Short, 2, 0)), ENOEXEC);
        assert_eq!(errno(Judgement::new(Kind::Text, 0, 0)), ENOEXEC);
    }

    #[test]
    fn kind_table() {
        assert_eq!(Kind::Elf.code(), 0);
        assert_eq!(Kind::Corrupt.code(), 1);
        assert_eq!(Kind::Short.code(), 2);
        assert_eq!(Kind::Text.code(), 3);
        assert_eq!(Kind::Corrupt.text(), "CORRUPT");
        assert_eq!(Kind::of(4), None);
        assert_eq!(Kind::of(2), Some(Kind::Short));
    }

    #[test]
    fn corpus_lookup() {
        assert_eq!(corpus_no(0).err(), Some(EL_ENOBS));
        assert_eq!(corpus_no(17).err(), Some(EL_ENOBS));
        let s = corpus_no(16).expect("s");
        assert_eq!(s.no, 16);
        assert!(s.hit().expect("h"));
        assert_eq!(s.name().expect("n").text().as_str(), "near_miss");
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(7).map(|s| s.value), Some(1));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze().err(), Some(EL_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11601_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
