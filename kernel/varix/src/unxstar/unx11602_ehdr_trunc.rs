//! UNX-F11602 ELF header 截断（不足 Elf64_Ehdr）拒收判据
//!
//! 魔数通过只说明前四字节对，不代表头齐。Elf64_Ehdr 固定 64 字节：前 16
//! 字节是 e_ident，其后 13 个字段按 16/18/20/24/32/40/48/52/54/56/58/60/62
//! 的偏移排列，最后一个 e_shstrndx 占 62..64。任何一段被文件尾截掉，内核
//! 都不能去读它——读越界就是拿别人的页当字段，比报错危险得多。
//!
//! 判据按「第一个不完整的字段」定位，而不是笼统说「头太短」：把 63 字节的
//! 文件说成「缺 e_shstrndx 的最后一字节」，用户才知道是拷坏了还是写坏了。
//! 定位顺序严格按字段偏移走：e_type 缺就报 e_type，不会跳过它去报更靠后的
//! e_phoff；e_ident 不足 16 时整段都没有字段可谈，报 ident 段。
//!
//! 表里 14 个长度把边界钉死：0/1/4/8/15 落在 ident，16/17 落在 e_type，20 落
//! 在 e_version，24 落在 e_entry，32/40 落在两个偏移表指针，52 落在 e_ehsize，
//! 63 只差 e_shstrndx 的最后一字节，64 刚好齐。
//!
//! 闸门次序：字节 → 名称 → 字段表 → 缺口 → 文案 → 语料 → 错误码 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfb10;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// Elf64_Ehdr 字节数。
pub const EHDR: usize = 64;

/// e_ident 字节数。
pub const IDENT: usize = 16;

/// 名称容量。
pub const NAME_CAP: usize = 16;

/// 字段数（ident 之后的 13 个）。
pub const FIELD_N: usize = 13;

/// 语料长度条数。
pub const CORPUS: usize = 14;

/// e_ident 的伪字段号。
pub const IDENT_FIELD: u8 = 13;

/// 齐备哨兵。
pub const COMPLETE: u8 = 0xff;

/// `ENOEXEC`。
pub const ENOEXEC: i32 = 8;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 字节非法。
pub const TR_EBYTE: u32 = SEG_BASE + 0x01;
/// 名称为空。
pub const TR_ENAME: u32 = SEG_BASE + 0x02;
/// 语料编号重复。
pub const TR_EDUP: u32 = SEG_BASE + 0x03;
/// 超长。
pub const TR_ELEN: u32 = SEG_BASE + 0x04;
/// 头部截断。
pub const TR_ESHORT: u32 = SEG_BASE + 0x05;
/// 缺口不符。
pub const TR_EGAP: u32 = SEG_BASE + 0x06;
/// 状态冲突。
pub const TR_ESTATE: u32 = SEG_BASE + 0x07;
/// 未达绿档。
pub const TR_EVER: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const TR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const TR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const TR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const TR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const TR_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// ident 段。
pub const OP_IDENT: u8 = 0x01;
/// 字段表。
pub const OP_FIELD: u8 = 0x02;
/// 缺口定位。
pub const OP_GAP: u8 = 0x03;
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

/// 追加一个两位十六进制字节。
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
            return Err(TR_ENAME);
        }
        if bytes.len() > NAME_CAP {
            return Err(TR_ELEN);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < bytes.len() {
            let b = match bytes.get(i) {
                Some(v) => *v,
                None => return Err(TR_ENOBS),
            };
            if b < 0x20 || b == 0x7f || b >= 0x80 {
                return Err(TR_EBYTE);
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
// 字段表
// ---------------------------------------------------------------------------

/// 一个头字段：名、偏移、字节数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    /// 字段名。
    pub name: &'static str,
    /// 偏移。
    pub off: usize,
    /// 字节数。
    pub size: usize,
}

impl Field {
    /// 结束偏移。
    pub fn end(&self) -> usize {
        self.off + self.size
    }

    /// 定长读取：返回该字段的字节窗口。
    pub fn window<'a>(&self, raw: &'a [u8]) -> Result<&'a [u8], u32> {
        if raw.len() < self.end() {
            return Err(TR_ESHORT);
        }
        match raw.get(self.off..self.end()) {
            Some(v) => Ok(v),
            None => Err(TR_ESHORT),
        }
    }

    /// 小端 16 位。
    pub fn le16(&self, raw: &[u8]) -> Result<u16, u32> {
        let w = self.window(raw)?;
        match (w.first(), w.get(1)) {
            (Some(a), Some(b)) => Ok(u16::from_le_bytes([*a, *b])),
            _ => Err(TR_ESHORT),
        }
    }

    /// 小端 32 位。
    pub fn le32(&self, raw: &[u8]) -> Result<u32, u32> {
        let w = self.window(raw)?;
        if w.len() < 4 {
            return Err(TR_ESHORT);
        }
        let mut b = [0u8; 4];
        let mut i = 0usize;
        while i < 4 {
            if let (Some(dst), Some(src)) = (b.get_mut(i), w.get(i)) {
                *dst = *src;
            }
            i += 1;
        }
        Ok(u32::from_le_bytes(b))
    }

    /// 小端 64 位。
    pub fn le64(&self, raw: &[u8]) -> Result<u64, u32> {
        let w = self.window(raw)?;
        if w.len() < 8 {
            return Err(TR_ESHORT);
        }
        let mut b = [0u8; 8];
        let mut i = 0usize;
        while i < 8 {
            if let (Some(dst), Some(src)) = (b.get_mut(i), w.get(i)) {
                *dst = *src;
            }
            i += 1;
        }
        Ok(u64::from_le_bytes(b))
    }
}

/// 十三字段表。
pub static FIELDS: [Field; FIELD_N] = [
    Field { name: "e_type", off: 16, size: 2 },
    Field { name: "e_machine", off: 18, size: 2 },
    Field { name: "e_version", off: 20, size: 4 },
    Field { name: "e_entry", off: 24, size: 8 },
    Field { name: "e_phoff", off: 32, size: 8 },
    Field { name: "e_shoff", off: 40, size: 8 },
    Field { name: "e_flags", off: 48, size: 4 },
    Field { name: "e_ehsize", off: 52, size: 2 },
    Field { name: "e_phentsize", off: 54, size: 2 },
    Field { name: "e_phnum", off: 56, size: 2 },
    Field { name: "e_shentsize", off: 58, size: 2 },
    Field { name: "e_shnum", off: 60, size: 2 },
    Field { name: "e_shstrndx", off: 62, size: 2 },
];

/// 按序号取字段。
pub fn field_at(i: usize) -> Result<&'static Field, u32> {
    match FIELDS.get(i) {
        Some(v) => Ok(v),
        None => Err(TR_ENOBS),
    }
}

/// 按名取字段。
pub fn field_named(name: &[u8]) -> Result<&'static Field, u32> {
    let mut i = 0usize;
    while i < FIELD_N {
        if let Some(f) = FIELDS.get(i) {
            if f.name.as_bytes() == name {
                return Ok(f);
            }
        }
        i += 1;
    }
    Err(TR_ENOBS)
}

// ---------------------------------------------------------------------------
// 截断
// ---------------------------------------------------------------------------

/// 头部缓冲：最多 64 字节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trunc {
    buf: [u8; EHDR],
    len: usize,
}

impl Trunc {
    /// 装载：0..=64 字节。
    pub fn new(raw: &[u8]) -> Result<Trunc, u32> {
        if raw.len() > EHDR {
            return Err(TR_ELEN);
        }
        let mut buf = [0u8; EHDR];
        let mut i = 0usize;
        while i < raw.len() {
            let b = match raw.get(i) {
                Some(v) => *v,
                None => return Err(TR_ENOBS),
            };
            if let Some(dst) = buf.get_mut(i) {
                *dst = b;
            }
            i += 1;
        }
        Ok(Trunc { buf, len: raw.len() })
    }

    /// 已装载字节数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否齐备。
    pub fn complete(&self) -> bool {
        self.len >= EHDR
    }

    /// 缺多少字节。
    pub fn short_by(&self) -> usize {
        EHDR.saturating_sub(self.len)
    }

    /// 字节视图。
    pub fn bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    /// 摘要。
    pub fn sum(&self) -> u64 {
        digest(self.bytes())
    }

    /// 取一字节。
    pub fn byte(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.buf.get(i).copied()
    }

    /// 第一处缺口；齐备为 None。
    pub fn gap(&self) -> Option<Gap> {
        if self.len < IDENT {
            return Some(Gap { field: IDENT_FIELD, off: 0, need: IDENT, got: self.len });
        }
        let mut i = 0usize;
        while i < FIELD_N {
            if let Some(f) = FIELDS.get(i) {
                if f.end() > self.len {
                    return Some(Gap {
                        field: i as u8,
                        off: f.off,
                        need: f.end(),
                        got: self.len.saturating_sub(f.off),
                    });
                }
            }
            i += 1;
        }
        None
    }

    /// 截断闸门。
    pub fn check(&self) -> Result<(), u32> {
        match self.gap() {
            Some(_) => Err(TR_ESHORT),
            None => Ok(()),
        }
    }

    /// 字段读取。
    pub fn field(&self, f: &Field) -> Result<&[u8], u32> {
        f.window(self.bytes())
    }
}

/// 缺口：字段号（13 为 ident 段）、字段偏移、需要到达的位置、实际拿到几字节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    field: u8,
    off: usize,
    need: usize,
    got: usize,
}

impl Gap {
    /// 字段号。
    pub fn field(&self) -> u8 {
        self.field
    }

    /// 字段偏移。
    pub fn off(&self) -> usize {
        self.off
    }

    /// 需要到达的位置。
    pub fn need(&self) -> usize {
        self.need
    }

    /// 实际拿到的字节数。
    pub fn got(&self) -> usize {
        self.got
    }

    /// 是否 ident 段缺口。
    pub fn ident(&self) -> bool {
        self.field == IDENT_FIELD
    }

    /// 缺的字节数。
    pub fn missing(&self) -> usize {
        self.need.saturating_sub(self.off).saturating_sub(self.got)
    }

    /// 字段名。
    pub fn field_name(&self) -> &'static str {
        if self.ident() {
            return "e_ident";
        }
        match FIELDS.get(self.field as usize) {
            Some(f) => f.name,
            None => "?",
        }
    }
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// ident 段不足。
    Ident,
    /// 头字段不足。
    Header,
    /// 齐备。
    Ready,
}

/// 档码：0 齐备 / 1 头截断 / 2 ident 截断。
impl Stage {
    /// 档码。
    pub fn code(&self) -> u8 {
        match self {
            Stage::Ready => 0,
            Stage::Header => 1,
            Stage::Ident => 2,
        }
    }

    /// 档名。
    pub fn text(&self) -> &'static str {
        match self {
            Stage::Ready => "READY",
            Stage::Header => "HEADER",
            Stage::Ident => "IDENT",
        }
    }

    /// 由档码建档。
    pub fn of(code: u8) -> Option<Stage> {
        match code {
            0 => Some(Stage::Ready),
            1 => Some(Stage::Header),
            2 => Some(Stage::Ident),
            _ => None,
        }
    }
}

/// 判定：截断档 + 缺口。
pub fn stage_of(t: &Trunc) -> (Stage, Option<Gap>) {
    match t.gap() {
        None => (Stage::Ready, None),
        Some(g) => {
            if g.ident() {
                (Stage::Ident, Some(g))
            } else {
                (Stage::Header, Some(g))
            }
        }
    }
}

/// 文案：截断必须带字段名与区间。
pub fn message(t: &Trunc) -> String {
    let (st, gap) = stage_of(t);
    let mut s = String::new();
    match st {
        Stage::Ready => {
            s.push_str("ok: Elf64_Ehdr complete (64/64)");
            s
        }
        Stage::Ident => {
            s.push_str("bash: ./a.out: cannot execute binary file: Exec format error");
            s.push_str(" (ELF header truncated: e_ident ");
            match gap {
                Some(g) => {
                    push_dec(&mut s, g.got() as u32);
                    s.push_str("/16 bytes)");
                }
                None => s.push(')'),
            }
            s
        }
        Stage::Header => {
            s.push_str("bash: ./a.out: cannot execute binary file: Exec format error");
            s.push_str(" (ELF header truncated: ");
            match gap {
                Some(g) => {
                    s.push_str(g.field_name());
                    s.push_str(" at ");
                    push_dec(&mut s, g.off() as u32);
                    s.push_str("..");
                    push_dec(&mut s, g.need() as u32);
                    s.push_str(", have ");
                    push_dec(&mut s, g.got() as u32);
                }
                None => s.push('?'),
            }
            s.push(')');
            s
        }
    }
}

/// 错误码：截断一律 ENOEXEC。
pub fn errno(t: &Trunc) -> i32 {
    match stage_of(t).0 {
        Stage::Ready => 0,
        Stage::Ident => ENOEXEC,
        Stage::Header => ENOEXEC,
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一条语料：长度、标签、期望档、期望字段、期望已得字节。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 长度。
    pub len: usize,
    /// 标签。
    pub tag: &'static [u8],
    /// 期望档码（0 齐备 / 1 头 / 2 ident）。
    pub want: u8,
    /// 期望字段号（齐备为 0xff）。
    pub want_field: u8,
    /// 期望已得字节。
    pub want_got: u8,
}

impl Slot {
    /// 标签名。
    pub fn name(&self) -> Result<Name, u32> {
        Name::new(self.tag)
    }

    /// 造一条全零缓冲。
    pub fn make(&self) -> Result<Trunc, u32> {
        let mut raw = [0u8; EHDR];
        let mut i = 0usize;
        while i < self.len {
            if let Some(dst) = raw.get_mut(i) {
                *dst = 0u8;
            }
            i += 1;
        }
        Trunc::new(raw.get(..self.len).unwrap_or(&[]))
    }

    /// 是否命中。
    pub fn hit(&self) -> Result<bool, u32> {
        let t = self.make()?;
        let (st, gap) = stage_of(&t);
        if st.code() != self.want {
            return Ok(false);
        }
        match gap {
            None => Ok(self.want_field == COMPLETE),
            Some(g) => Ok(g.field() == self.want_field && g.got() as u8 == self.want_got),
        }
    }
}

/// 十四卡长度语料。
pub static CORPUS14: [Slot; CORPUS] = [
    Slot { len: 0, tag: b"len_0", want: 2, want_field: 13, want_got: 0 },
    Slot { len: 1, tag: b"len_1", want: 2, want_field: 13, want_got: 1 },
    Slot { len: 4, tag: b"len_4", want: 2, want_field: 13, want_got: 4 },
    Slot { len: 8, tag: b"len_8", want: 2, want_field: 13, want_got: 8 },
    Slot { len: 15, tag: b"len_15", want: 2, want_field: 13, want_got: 15 },
    Slot { len: 16, tag: b"len_16", want: 1, want_field: 0, want_got: 0 },
    Slot { len: 17, tag: b"len_17", want: 1, want_field: 0, want_got: 1 },
    Slot { len: 20, tag: b"len_20", want: 1, want_field: 2, want_got: 0 },
    Slot { len: 24, tag: b"len_24", want: 1, want_field: 3, want_got: 0 },
    Slot { len: 32, tag: b"len_32", want: 1, want_field: 4, want_got: 0 },
    Slot { len: 40, tag: b"len_40", want: 1, want_field: 5, want_got: 0 },
    Slot { len: 52, tag: b"len_52", want: 1, want_field: 7, want_got: 0 },
    Slot { len: 63, tag: b"len_63", want: 1, want_field: 12, want_got: 1 },
    Slot { len: 64, tag: b"len_64", want: 0, want_field: COMPLETE, want_got: 0 },
];

/// 按编号取语料。
pub fn corpus_no(no: u8) -> Result<&'static Slot, u32> {
    if no == 0 {
        return Err(TR_ENOBS);
    }
    match CORPUS14.get((no as usize) - 1) {
        Some(v) => Ok(v),
        None => Err(TR_ENOBS),
    }
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    total: usize,
    ready: usize,
    header: usize,
    ident: usize,
    matched: usize,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report { total: 0, ready: 0, header: 0, ident: 0, matched: 0 }
    }

    /// 总数。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 齐备数。
    pub fn ready(&self) -> usize {
        self.ready
    }

    /// 头截断数。
    pub fn header(&self) -> usize {
        self.header
    }

    /// ident 截断数。
    pub fn ident(&self) -> usize {
        self.ident
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

    /// 绿档闸门：14 条齐跑、1 齐 13 截、全命中。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS {
            return Err(TR_ENOBS);
        }
        if self.ready != 1 || self.header + self.ident != CORPUS - 1 {
            return Err(TR_EGAP);
        }
        if self.matched != self.total {
            return Err(TR_EVER);
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
        s.push_str("ehdr trunc corpus=");
        push_dec(&mut s, self.total as u32);
        s.push_str(" ready=");
        push_dec(&mut s, self.ready as u32);
        s.push_str(" header=");
        push_dec(&mut s, self.header as u32);
        s.push_str(" ident=");
        push_dec(&mut s, self.ident as u32);
        s.push_str(" match=");
        push_dec(&mut s, self.matched as u32);
        s.push_str(" rate=");
        push_dec(&mut s, self.rate());
        s.push_str("ppm\n");
        s
    }
}

/// 跑全部语料；`over` 改写某条期望档码。
fn run_with(over: Option<(usize, u8)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CORPUS {
        let slot = match CORPUS14.get(i) {
            Some(v) => v,
            None => return Err(TR_ENOBS),
        };
        let t = slot.make()?;
        let (st, gap) = stage_of(&t);
        r.total += 1;
        match st {
            Stage::Ready => r.ready += 1,
            Stage::Header => r.header += 1,
            Stage::Ident => r.ident += 1,
        }
        let want = match over {
            Some((idx, k)) if idx == i => k,
            _ => slot.want,
        };
        let hit = if st.code() != want {
            false
        } else {
            match gap {
                None => slot.want_field == COMPLETE,
                Some(g) => g.field() == slot.want_field && g.got() as u8 == slot.want_got,
            }
        };
        if hit {
            r.matched += 1;
        }
        i += 1;
    }
    Ok(r)
}

/// 跑全量。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 覆盖闸门：必须满 14 条。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(TR_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 一次性读取器
// ---------------------------------------------------------------------------

/// 一次性读取器：同一字段只许读一次。
#[derive(Clone, Copy, Debug)]
pub struct Reader {
    used: u16,
    reads: u32,
}

impl Reader {
    /// 空读取器。
    pub const fn new() -> Reader {
        Reader { used: 0, reads: 0 }
    }

    /// 读一个字段；重复读落 ESTATE。
    pub fn take(&mut self, i: usize) -> Result<(), u32> {
        if i >= FIELD_N {
            return Err(TR_ENOBS);
        }
        let bit = 1u16 << (i as u32);
        if self.used & bit != 0 {
            return Err(TR_ESTATE);
        }
        self.used |= bit;
        self.reads += 1;
        Ok(())
    }

    /// 已读字段数。
    pub fn reads(&self) -> u32 {
        self.reads
    }

    /// 是否已读。
    pub fn took(&self, i: usize) -> bool {
        i < FIELD_N && (self.used & (1u16 << (i as u32))) != 0
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
            return Err(TR_ESLOT);
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
            return Err(TR_EBAL);
        }
        const VALS: [u32; 8] = [14, 1, 13, 64, 16, 13, 1, 929];
        const CODES: [u32; 6] =
            [TR_ESHORT, TR_ELEN, TR_EBYTE, TR_ENAME, TR_ENOBS, TR_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TR_EBAL),
            };
            if s.slot as usize != i {
                return Err(TR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TR_EVALUE);
                        }
                    }
                    None => return Err(TR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TR_EVALUE);
                        }
                    }
                    None => return Err(TR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TR_EBAL);
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
            return Err(TR_EBAL);
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
            Err(TR_ESEAL)
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
    let full = match Trunc::new(&[0u8; EHDR]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut ok = message(&full).starts_with("ok: Elf64_Ehdr complete");
    let short = match Trunc::new(&[0u8; 63]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let m = message(&short);
    ok = ok && m.contains("cannot execute binary file: Exec format error");
    ok = ok && m.contains("e_shstrndx at 62..64, have 1");
    let tiny = match Trunc::new(&[0u8; 8]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    ok = ok && message(&tiny).contains("e_ident 8/16 bytes");
    ok
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.ready())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.header() + x.ident())));

    let _ = l.register(count_cell(3, OP_IDENT, Ok(EHDR)));
    let _ = l.register(count_cell(4, OP_IDENT, Ok(IDENT)));
    let _ = l.register(count_cell(5, OP_FIELD, Ok(FIELD_N)));

    let msg_cell = if messages_ok() { Ok(1usize) } else { Ok(0usize) };
    let _ = l.register(count_cell(6, OP_MSG, msg_cell));

    let red = run_with(Some((0, 0)));
    let _ = l.register(count_cell(7, OP_GAP, red.map(|x| x.rate() as usize)));

    let short_err = match Trunc::new(&[0u8; 63]) {
        Ok(t) => t.check().err().unwrap_or(TR_ENOBS),
        Err(c) => c,
    };
    let _ = l.register(err_cell(8, OP_GAP, if short_err == 0 { TR_ENOBS } else { short_err }));

    let long_err = match Trunc::new(&[0u8; EHDR + 1]) {
        Ok(_) => TR_ELEN,
        Err(c) => c,
    };
    let _ = l.register(err_cell(9, OP_IDENT, long_err));

    let byte_err = match Name::new(&[0x00]) {
        Ok(_) => TR_EBYTE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(10, OP_FIELD, byte_err));

    let name_err = match Name::new(b"") {
        Ok(_) => TR_ENAME,
        Err(c) => c,
    };
    let _ = l.register(err_cell(11, OP_FIELD, name_err));

    let noobs_err = match field_at(FIELD_N) {
        Ok(_) => TR_ENOBS,
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_FIELD, noobs_err));

    let mut rd = Reader::new();
    let _ = rd.take(4);
    let state_err = match rd.take(4) {
        Ok(()) => TR_ESTATE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_FIELD, state_err));

    l
}

// ---------------------------------------------------------------------------
// 检查面
// ---------------------------------------------------------------------------

/// 缓冲装载。
fn buffer_ok() -> bool {
    let t = match Trunc::new(&[0u8; 63]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = t.len() == 63 && !t.is_empty() && !t.complete() && t.short_by() == 1;
    let b = t.byte(62) == Some(0) && t.byte(63).is_none();
    let c = match Trunc::new(&[0u8; EHDR + 1]) {
        Ok(_) => false,
        Err(k) => k == TR_ELEN,
    };
    let e = match Trunc::new(b"") {
        Ok(v) => v.is_empty() && v.gap().map(|g| g.ident()).unwrap_or(false),
        Err(_) => false,
    };
    a && b && c && e
}

/// 名称闸门。
fn name_ok() -> bool {
    let n = match Name::new(b"len_63") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = n.len() == 6 && n.text() == String::from("len_63") && n.same(b"len_63");
    let b = Name::new(b"").err() == Some(TR_ENAME);
    let c = Name::new(&[0u8; NAME_CAP + 1]).err() == Some(TR_ELEN);
    let d = Name::new(&[0x7f]).err() == Some(TR_EBYTE);
    a && b && c && d
}

/// 字段表。
fn field_ok() -> bool {
    let mut i = 0usize;
    let mut ok = true;
    let mut off = IDENT;
    while i < FIELD_N {
        let f = match FIELDS.get(i) {
            Some(v) => v,
            None => return false,
        };
        ok = ok && f.off == off && f.size > 0 && f.end() == off + f.size;
        off = f.end();
        i += 1;
    }
    ok = ok && off == EHDR;
    let t = match field_named(b"e_phoff") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = t.off == 32 && t.size == 8 && t.end() == 40;
    let b = field_named(b"nope").err() == Some(TR_ENOBS);
    let c = field_at(FIELD_N).err() == Some(TR_ENOBS);
    ok && a && b && c
}

/// 小端读取。
fn read_ok() -> bool {
    let mut raw = [0u8; EHDR];
    raw[16] = 0x02;
    raw[17] = 0x00;
    raw[24] = 0x78;
    raw[25] = 0x56;
    raw[26] = 0x34;
    raw[27] = 0x12;
    raw[32] = 0x40;
    let t = match Trunc::new(&raw) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let etype = match field_named(b"e_type") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let entry = match field_named(b"e_entry") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let phoff = match field_named(b"e_phoff") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = etype.le16(t.bytes()) == Ok(2);
    let b = entry.le32(t.bytes()) == Ok(0x1234_5678);
    let c = phoff.le64(t.bytes()) == Ok(0x40);
    let s = match Trunc::new(&[0u8; 10]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = etype.le16(s.bytes()).is_err() && entry.le64(s.bytes()).is_err();
    let f = t.field(etype).map(|w| w.len()).unwrap_or(0) == 2;
    a && b && c && d && f
}

/// 缺口定位。
fn gap_ok() -> bool {
    let mut i = 0usize;
    let mut ok = true;
    while i < CORPUS {
        let s = match CORPUS14.get(i) {
            Some(v) => v,
            None => return false,
        };
        ok = ok && s.hit().unwrap_or(false);
        i += 1;
    }
    let t63 = match Trunc::new(&[0u8; 63]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let g = match t63.gap() {
        Some(v) => v,
        None => return false,
    };
    let a = g.field() == 12 && g.field_name() == "e_shstrndx";
    let b = g.off() == 62 && g.need() == 64 && g.got() == 1 && g.missing() == 1;
    let c = !g.ident() && t63.check() == Err(TR_ESHORT);
    let full = match Trunc::new(&[0u8; EHDR]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = full.gap().is_none() && full.check() == Ok(());
    ok && a && b && c && d
}

/// 文案。
fn msg_ok() -> bool {
    if !messages_ok() {
        return false;
    }
    let t = match Trunc::new(&[0u8; 20]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let m = message(&t);
    let a = m.contains("e_version at 20..24, have 0");
    let b = m.starts_with("bash: ./a.out: cannot execute");
    let full = match Trunc::new(&[0u8; EHDR]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = !message(&full).contains("truncated");
    a && b && c
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.ready() == 1 && r.header() == 8 && r.ident() == 5;
    let b = r.matched() == CORPUS && r.rate() == 1000 && r.green();
    a && b
}

/// 语料编号与标签。
fn order_ok() -> bool {
    let mut i = 0usize;
    let mut ok = true;
    while i < CORPUS {
        let s = match CORPUS14.get(i) {
            Some(v) => v,
            None => return false,
        };
        ok = ok && s.len <= EHDR;
        ok = ok && s.tag.len() <= NAME_CAP && !s.tag.is_empty();
        ok = ok && s.make().map(|t| t.len()).unwrap_or(usize::MAX) == s.len;
        let mut j = i + 1;
        while j < CORPUS {
            if let Some(t) = CORPUS14.get(j) {
                ok = ok && s.tag != t.tag;
            }
            j += 1;
        }
        i += 1;
    }
    let d = corpus_no(0).err() == Some(TR_ENOBS) && corpus_no(15).err() == Some(TR_ENOBS);
    let e = match corpus_no(14) {
        Ok(s) => s.len == 64 && s.name().map(|n| n.same(b"len_64")).unwrap_or(false),
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
        == "ehdr trunc corpus=14 ready=1 header=8 ident=5 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(TR_ENOBS);
    let c = e.render().as_str()
        == "ehdr trunc corpus=0 ready=0 header=0 ident=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, 0))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(TR_EVER) && !r.green();
    let c = r.ready() == 1 && r.ident() == 5;
    a && b && c
}

/// 覆盖档。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(TR_ENOBS) && cover(0) == Err(TR_ENOBS)
}

/// 读取器。
fn reader_ok() -> bool {
    let mut rd = Reader::new();
    let a = rd.reads() == 0 && !rd.took(4);
    let b = rd.take(4) == Ok(()) && rd.took(4) && rd.reads() == 1;
    let c = rd.take(4) == Err(TR_ESTATE);
    let d = rd.take(FIELD_N) == Err(TR_ENOBS);
    let e = rd.take(5) == Ok(()) && rd.reads() == 2;
    a && b && c && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = buffer_ok() && name_ok();
    out[1] = field_ok() && read_ok();
    out[2] = gap_ok() && msg_ok() && corpus_ok() && order_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && reader_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11602_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11602.ehdr_trunc");

    s.add(
        "TR01-缓冲装载",
        {
            buffer_ok()
        },
        "static",
    );

    s.add(
        "TR02-名称闸门",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "TR03-字段表",
        {
            field_ok()
        },
        "static",
    );

    s.add(
        "TR04-小端读取",
        {
            read_ok()
        },
        "static",
    );

    s.add(
        "TR05-缺口定位",
        {
            gap_ok()
        },
        "static",
    );

    s.add(
        "TR06-截断文案",
        {
            msg_ok()
        },
        "static",
    );

    s.add(
        "TR07-十四卡全跑",
        {
            corpus_ok()
        },
        "static",
    );

    s.add(
        "TR08-语料编号",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "TR09-汇总渲染",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "TR10-红单显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "TR11-覆盖档",
        {
            cover_ok()
        },
        "static",
    );

    s.add(
        "TR12-一次性读取器",
        {
            reader_ok() && build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "TR13-冻结重放四注入",
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
    fn buffer_shape() {
        let t = Trunc::new(&[0u8; 64]).expect("t");
        assert!(t.complete());
        assert_eq!(t.short_by(), 0);
        assert_eq!(t.gap(), None);
        assert_eq!(Trunc::new(&[0u8; 65]).err(), Some(TR_ELEN));
        assert!(Trunc::new(b"").expect("e").is_empty());
    }

    #[test]
    fn name_gates() {
        assert_eq!(Name::new(b"").err(), Some(TR_ENAME));
        assert_eq!(Name::new(&[0x80]).err(), Some(TR_EBYTE));
        assert_eq!(Name::new(&[0u8; 17]).err(), Some(TR_ELEN));
        let n = Name::new(b"len_63").expect("n");
        assert_eq!(n.len(), 6);
        assert!(n.same(b"len_63"));
    }

    #[test]
    fn fields_tile() {
        let mut off = 16usize;
        for f in FIELDS.iter() {
            assert_eq!(f.off, off);
            off = f.end();
        }
        assert_eq!(off, 64);
        assert_eq!(field_named(b"e_shstrndx").expect("f").off, 62);
        assert_eq!(field_named(b"x").err(), Some(TR_ENOBS));
    }

    #[test]
    fn little_endian() {
        let mut raw = [0u8; 64];
        raw[16] = 0x03;
        raw[17] = 0x00;
        raw[32] = 0x40;
        let t = Trunc::new(&raw).expect("t");
        assert_eq!(field_named(b"e_type").expect("f").le16(t.bytes()), Ok(3));
        assert_eq!(field_named(b"e_phoff").expect("f").le64(t.bytes()), Ok(0x40));
        let s = Trunc::new(&[0u8; 4]).expect("s");
        assert_eq!(field_named(b"e_type").expect("f").le16(s.bytes()), Err(TR_ESHORT));
    }

    #[test]
    fn gap_chain() {
        let cases = [
            (0usize, 13u8, 0usize),
            (15, 13, 15),
            (16, 0, 0),
            (17, 0, 1),
            (20, 2, 0),
            (24, 3, 0),
            (32, 4, 0),
            (40, 5, 0),
            (52, 7, 0),
            (63, 12, 1),
        ];
        for (len, field, got) in cases.iter() {
            let t = Trunc::new(&vec_zero(*len)[..*len]).expect("t");
            let g = t.gap().expect("g");
            assert_eq!(g.field(), *field, "len {}", len);
            assert_eq!(g.got(), *got, "len {}", len);
        }
    }

    #[test]
    fn stage_codes() {
        assert_eq!(Stage::Ready.code(), 0);
        assert_eq!(Stage::Header.code(), 1);
        assert_eq!(Stage::Ident.code(), 2);
        assert_eq!(Stage::of(1), Some(Stage::Header));
        assert_eq!(Stage::of(3), None);
    }

    #[test]
    fn message_points() {
        let t = Trunc::new(&vec_zero(63)[..63]).expect("t");
        let m = message(&t);
        assert!(m.contains("e_shstrndx at 62..64, have 1"));
        assert!(m.contains("Exec format error"));
        let tiny = Trunc::new(&vec_zero(8)[..8]).expect("s");
        assert!(message(&tiny).contains("e_ident 8/16 bytes"));
        assert_eq!(errno(&tiny), ENOEXEC);
    }

    #[test]
    fn corpus_counts() {
        let r = run().expect("r");
        assert_eq!(r.ready(), 1);
        assert_eq!(r.header(), 8);
        assert_eq!(r.ident(), 5);
        assert_eq!(r.matched(), 14);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(
            r.render().as_str(),
            "ehdr trunc corpus=14 ready=1 header=8 ident=5 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rate() {
        let r = run_with(Some((0, 0))).expect("r");
        assert_eq!(r.matched(), 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(TR_EVER));
    }

    #[test]
    fn empty_report() {
        let r = Report::new();
        assert!(r.is_empty());
        assert_eq!(r.rate(), 0);
        assert_eq!(r.gate(), Err(TR_ENOBS));
    }

    #[test]
    fn coverage_gate() {
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(13), Err(TR_ENOBS));
    }

    #[test]
    fn reader_once() {
        let mut rd = Reader::new();
        assert_eq!(rd.take(12), Ok(()));
        assert!(rd.took(12));
        assert_eq!(rd.take(12), Err(TR_ESTATE));
        assert_eq!(rd.reads(), 1);
    }

    #[test]
    fn corpus_lookup() {
        assert_eq!(corpus_no(0).err(), Some(TR_ENOBS));
        assert_eq!(corpus_no(15).err(), Some(TR_ENOBS));
        assert_eq!(corpus_no(14).expect("s").len, 64);
        assert_eq!(corpus_no(13).expect("s").want_field, 12);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(7).map(|s| s.value), Some(929));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze().err(), Some(TR_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11602_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    fn vec_zero(n: usize) -> [u8; EHDR] {
        let mut raw = [0u8; EHDR];
        let mut i = 0usize;
        while i < n && i < EHDR {
            raw[i] = 0u8;
            i += 1;
        }
        raw
    }
}
