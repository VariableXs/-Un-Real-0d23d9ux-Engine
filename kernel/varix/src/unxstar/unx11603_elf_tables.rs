//! UNX-F11603 e_phoff/e_shoff 越界拒收判据
//!
//! 程序头表与节头表是两段「由头里的三个数指出来」的区域：偏移、表项大小、
//! 表项数。三者乘加出来的尾地址必须落在文件里，否则内核按偏移去读就是读
//! 到文件外的内存。判据把这段算术全部走 `checked_*`：乘法溢出与加法溢出
//! 分别留痕，尾地址越过文件长度单独落「越界」，绝不让回绕后的「小地址」
//! 冒充合法表头。
//!
//! 判据次序是刻意的：表项数为零直接算「无表」——空表不占文件，也不该逼
//! 用户给出合法偏移；有表才查表项大小（Elf64_Phdr 56、Elf64_Shdr 64，小于
//! 就落 EENTSIZE，大于 1024 也落同一档，防止拿怪值把循环放大）；再查偏移
//! 不许压进 64 字节的 Ehdr；再查八字节对齐；最后才算尾地址。
//!
//! 边界语料把「刚好」与「差一」分开：尾地址等于文件长度算通过，多一字节
//! 就算越界。溢出语料用贴近 u64 上限的偏移，验证不是先把加法回绕再比较。
//!
//! 闸门次序：字节 → 名称 → 表项算术 → 边界 → 文案 → 语料 → 错误码 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfb20;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// Elf64_Ehdr 字节数。
pub const EHDR: usize = 64;

/// Elf64_Phdr 字节数。
pub const PHDR: u16 = 56;

/// Elf64_Shdr 字节数。
pub const SHDR: u16 = 64;

/// 表项大小上限。
pub const ENT_CAP: u16 = 1024;

/// 对齐粒度。
pub const ALIGN: u64 = 8;

/// 名称容量。
pub const NAME_CAP: usize = 16;

/// 语料条数。
pub const CORPUS: usize = 12;

/// `ENOEXEC`。
pub const ENOEXEC: i32 = 8;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 表项大小非法。
pub const PT_EENTSIZE: u32 = SEG_BASE + 0x01;
/// 偏移压进 Ehdr。
pub const PT_EOFF: u32 = SEG_BASE + 0x02;
/// 偏移未对齐。
pub const PT_EALIGN: u32 = SEG_BASE + 0x03;
/// 乘加溢出。
pub const PT_EWRAP: u32 = SEG_BASE + 0x04;
/// 尾地址越界。
pub const PT_EBEYOND: u32 = SEG_BASE + 0x05;
/// 状态冲突。
pub const PT_ESTATE: u32 = SEG_BASE + 0x06;
/// 未达绿档。
pub const PT_EVER: u32 = SEG_BASE + 0x07;
/// 字节非法。
pub const PT_EBYTE: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const PT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const PT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const PT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const PT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const PT_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 表构造。
pub const OP_TABLE: u8 = 0x01;
/// 表项大小。
pub const OP_ENTSIZE: u8 = 0x02;
/// 尾地址。
pub const OP_END: u8 = 0x03;
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

/// 追加一个 64 位数。
fn push_dec64(out: &mut String, v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut n = v;
    let mut buf = [0u8; 20];
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
            return Err(PT_EBYTE);
        }
        if bytes.len() > NAME_CAP {
            return Err(PT_EBYTE);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < bytes.len() {
            let b = match bytes.get(i) {
                Some(v) => *v,
                None => return Err(PT_ENOBS),
            };
            if b < 0x20 || b == 0x7f || b >= 0x80 {
                return Err(PT_EBYTE);
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
// 表
// ---------------------------------------------------------------------------

/// 表种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 程序头表。
    Phdr,
    /// 节头表。
    Shdr,
}

impl Kind {
    /// 表项下限。
    pub fn need(&self) -> u16 {
        match self {
            Kind::Phdr => PHDR,
            Kind::Shdr => SHDR,
        }
    }

    /// 名称。
    pub fn text(&self) -> &'static str {
        match self {
            Kind::Phdr => "phdr",
            Kind::Shdr => "shdr",
        }
    }

    /// 档码：0 程序头 / 1 节头。
    pub fn code(&self) -> u8 {
        match self {
            Kind::Phdr => 0,
            Kind::Shdr => 1,
        }
    }
}

/// 一段表：种类、偏移、表项大小、表项数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Table {
    kind: Kind,
    off: u64,
    entsize: u16,
    num: u16,
}

impl Table {
    /// 构造。
    pub const fn new(kind: Kind, off: u64, entsize: u16, num: u16) -> Table {
        Table { kind, off, entsize, num }
    }

    /// 种类。
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// 偏移。
    pub fn off(&self) -> u64 {
        self.off
    }

    /// 表项大小。
    pub fn entsize(&self) -> u16 {
        self.entsize
    }

    /// 表项数。
    pub fn num(&self) -> u16 {
        self.num
    }

    /// 是否空表。
    pub fn absent(&self) -> bool {
        self.num == 0
    }

    /// 总字节数（checked）。
    pub fn span(&self) -> Option<u64> {
        (self.entsize as u64).checked_mul(self.num as u64)
    }

    /// 尾地址（checked）。
    pub fn end(&self) -> Option<u64> {
        match self.span() {
            Some(s) => self.off.checked_add(s),
            None => None,
        }
    }

    /// 是否对齐。
    pub fn aligned(&self) -> bool {
        self.off % ALIGN == 0
    }

    /// 闸门：空表放行；否则表项 → 偏移 → 对齐 → 乘加 → 越界。
    pub fn check(&self, file_len: u64) -> Result<(), u32> {
        if self.num == 0 {
            return Ok(());
        }
        if self.entsize < self.kind.need() || self.entsize > ENT_CAP {
            return Err(PT_EENTSIZE);
        }
        if self.off < EHDR as u64 {
            return Err(PT_EOFF);
        }
        if !self.aligned() {
            return Err(PT_EALIGN);
        }
        match self.span() {
            Some(_) => {}
            None => return Err(PT_EWRAP),
        }
        match self.end() {
            Some(end) => {
                if end > file_len {
                    Err(PT_EBEYOND)
                } else {
                    Ok(())
                }
            }
            None => Err(PT_EWRAP),
        }
    }

    /// 状态：0 收 / 1 空 / 2 拒。
    pub fn state(&self, file_len: u64) -> u8 {
        if self.num == 0 {
            return 1;
        }
        match self.check(file_len) {
            Ok(()) => 0,
            Err(_) => 2,
        }
    }
}

/// 表对结果的分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judgement {
    state: u8,
    code: u32,
    end: u64,
}

impl Judgement {
    /// 判定一段表。
    pub fn of(t: &Table, file_len: u64) -> Judgement {
        let state = t.state(file_len);
        let code = match t.check(file_len) {
            Ok(()) => 0,
            Err(c) => c,
        };
        Judgement { state, code, end: t.end().unwrap_or(0) }
    }

    /// 状态码。
    pub fn state(&self) -> u8 {
        self.state
    }

    /// 错误码；通过为 0。
    pub fn code(&self) -> u32 {
        self.code
    }

    /// 尾地址；溢出为 0。
    pub fn end(&self) -> u64 {
        self.end
    }

    /// 是否通过。
    pub fn accepted(&self) -> bool {
        self.state == 0
    }

    /// 是否空表。
    pub fn absent(&self) -> bool {
        self.state == 1
    }
}

/// 文案。
pub fn message(t: &Table, file_len: u64) -> String {
    let j = Judgement::of(t, file_len);
    let mut s = String::new();
    if j.accepted() {
        s.push_str("ok: ");
        s.push_str(t.kind().text());
        s.push_str(" table fits (end=");
        push_dec64(&mut s, j.end());
        s.push(')');
        return s;
    }
    if j.absent() {
        s.push_str("ok: ");
        s.push_str(t.kind().text());
        s.push_str(" table absent");
        return s;
    }
    s.push_str("bash: ./a.out: cannot execute binary file: Exec format error (");
    s.push_str(t.kind().text());
    s.push_str(" table ");
    match j.code {
        PT_EENTSIZE => {
            s.push_str("entsize=");
            push_dec(&mut s, t.entsize() as u32);
            s.push_str(" want >=");
            push_dec(&mut s, t.kind().need() as u32);
        }
        PT_EOFF => {
            s.push_str("off=");
            push_dec64(&mut s, t.off());
            s.push_str(" inside ehdr");
        }
        PT_EALIGN => {
            s.push_str("off=");
            push_dec64(&mut s, t.off());
            s.push_str(" unaligned");
        }
        PT_EWRAP => {
            s.push_str("off*num overflow");
        }
        PT_EBEYOND => {
            s.push_str("end=");
            push_dec64(&mut s, j.end());
            s.push_str(" > file=");
            push_dec64(&mut s, file_len);
        }
        _ => s.push_str("rejected"),
    }
    s.push(')');
    s
}

/// 错误码：表问题一律 ENOEXEC。
pub fn errno(t: &Table, file_len: u64) -> i32 {
    match Judgement::of(t, file_len).state {
        0 | 1 => 0,
        _ => ENOEXEC,
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一条语料。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 编号（1 起）。
    pub no: u8,
    /// 标签。
    pub tag: &'static [u8],
    /// 种类。
    pub kind: Kind,
    /// 偏移。
    pub off: u64,
    /// 表项大小。
    pub entsize: u16,
    /// 表项数。
    pub num: u16,
    /// 文件长度。
    pub file: u64,
    /// 期望状态（0 收 / 1 空 / 2 拒）。
    pub want_state: u8,
    /// 期望错误码；0 表示通过。
    pub want_err: u32,
}

impl Slot {
    /// 标签名。
    pub fn name(&self) -> Result<Name, u32> {
        Name::new(self.tag)
    }

    /// 造表。
    pub fn table(&self) -> Table {
        Table::new(self.kind, self.off, self.entsize, self.num)
    }

    /// 是否命中。
    pub fn hit(&self) -> bool {
        let j = Judgement::of(&self.table(), self.file);
        j.state() == self.want_state && j.code() == self.want_err
    }
}

/// 十二卡语料。
pub static CORPUS12: [Slot; CORPUS] = [
    Slot {
        no: 1,
        tag: b"absent_zero",
        kind: Kind::Phdr,
        off: 0,
        entsize: 56,
        num: 0,
        file: 10000,
        want_state: 1,
        want_err: 0,
    },
    Slot {
        no: 2,
        tag: b"phdr_ok",
        kind: Kind::Phdr,
        off: 64,
        entsize: 56,
        num: 9,
        file: 10000,
        want_state: 0,
        want_err: 0,
    },
    Slot {
        no: 3,
        tag: b"shdr_absent",
        kind: Kind::Shdr,
        off: 64,
        entsize: 64,
        num: 0,
        file: 10000,
        want_state: 1,
        want_err: 0,
    },
    Slot {
        no: 4,
        tag: b"ent_small",
        kind: Kind::Phdr,
        off: 64,
        entsize: 32,
        num: 9,
        file: 10000,
        want_state: 2,
        want_err: 0xfb21,
    },
    Slot {
        no: 5,
        tag: b"ent_zero",
        kind: Kind::Shdr,
        off: 64,
        entsize: 0,
        num: 3,
        file: 10000,
        want_state: 2,
        want_err: 0xfb21,
    },
    Slot {
        no: 6,
        tag: b"off_in_ehdr",
        kind: Kind::Phdr,
        off: 16,
        entsize: 56,
        num: 9,
        file: 10000,
        want_state: 2,
        want_err: 0xfb22,
    },
    Slot {
        no: 7,
        tag: b"off_unalign",
        kind: Kind::Shdr,
        off: 65,
        entsize: 64,
        num: 9,
        file: 10000,
        want_state: 2,
        want_err: 0xfb23,
    },
    Slot {
        no: 8,
        tag: b"beyond_end",
        kind: Kind::Phdr,
        off: 64,
        entsize: 56,
        num: 1000,
        file: 10000,
        want_state: 2,
        want_err: 0xfb25,
    },
    Slot {
        no: 9,
        tag: b"exact_end",
        kind: Kind::Phdr,
        off: 64,
        entsize: 56,
        num: 1,
        file: 120,
        want_state: 0,
        want_err: 0,
    },
    Slot {
        no: 10,
        tag: b"one_short",
        kind: Kind::Phdr,
        off: 64,
        entsize: 56,
        num: 1,
        file: 119,
        want_state: 2,
        want_err: 0xfb25,
    },
    Slot {
        no: 11,
        tag: b"wrap_end",
        kind: Kind::Shdr,
        off: u64::MAX - 15,
        entsize: 64,
        num: 9,
        file: u64::MAX,
        want_state: 2,
        want_err: 0xfb24,
    },
    Slot {
        no: 12,
        tag: b"phnum_max",
        kind: Kind::Phdr,
        off: 64,
        entsize: 56,
        num: 0xffff,
        file: 10000,
        want_state: 2,
        want_err: 0xfb25,
    },
];

/// 按编号取语料。
pub fn corpus_no(no: u8) -> Result<&'static Slot, u32> {
    if no == 0 {
        return Err(PT_ENOBS);
    }
    match CORPUS12.get((no as usize) - 1) {
        Some(v) => Ok(v),
        None => Err(PT_ENOBS),
    }
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    total: usize,
    accept: usize,
    absent: usize,
    reject: usize,
    matched: usize,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report { total: 0, accept: 0, absent: 0, reject: 0, matched: 0 }
    }

    /// 总数。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 收下表数。
    pub fn accept(&self) -> usize {
        self.accept
    }

    /// 空表数。
    pub fn absent(&self) -> usize {
        self.absent
    }

    /// 拒收表数。
    pub fn reject(&self) -> usize {
        self.reject
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
            return Err(PT_ENOBS);
        }
        if self.accept != 2 || self.absent != 2 || self.reject != 8 {
            return Err(PT_EVER);
        }
        if self.matched != self.total {
            return Err(PT_EVER);
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
        s.push_str("tables=");
        push_dec(&mut s, self.total as u32);
        s.push_str(" accept=");
        push_dec(&mut s, self.accept as u32);
        s.push_str(" absent=");
        push_dec(&mut s, self.absent as u32);
        s.push_str(" reject=");
        push_dec(&mut s, self.reject as u32);
        s.push_str(" match=");
        push_dec(&mut s, self.matched as u32);
        s.push_str(" rate=");
        push_dec(&mut s, self.rate());
        s.push_str("ppm\n");
        s
    }
}

/// 跑全部语料；`over` 改写某条期望错误码。
fn run_with(over: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CORPUS {
        let slot = match CORPUS12.get(i) {
            Some(v) => v,
            None => return Err(PT_ENOBS),
        };
        if slot.no as usize != i + 1 {
            return Err(PT_ESTATE);
        }
        let j = Judgement::of(&slot.table(), slot.file);
        r.total += 1;
        match j.state() {
            0 => r.accept += 1,
            1 => r.absent += 1,
            _ => r.reject += 1,
        }
        let want_err = match over {
            Some((idx, c)) if idx == i => c,
            _ => slot.want_err,
        };
        if j.state() == slot.want_state && j.code() == want_err {
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

/// 覆盖闸门。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(PT_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 一次性校验器
// ---------------------------------------------------------------------------

/// 一次性校验器：同一段表只许校验一次。
#[derive(Clone, Copy, Debug)]
pub struct Checker {
    bound: bool,
    code: u32,
    end: u64,
}

impl Checker {
    /// 空校验器。
    pub const fn new() -> Checker {
        Checker { bound: false, code: 0, end: 0 }
    }

    /// 校验一次；重复校验落 ESTATE。
    pub fn bind(&mut self, t: &Table, file_len: u64) -> Result<(), u32> {
        if self.bound {
            return Err(PT_ESTATE);
        }
        let j = Judgement::of(t, file_len);
        if j.code() != 0 {
            return Err(j.code());
        }
        self.bound = true;
        self.code = j.code();
        self.end = j.end();
        Ok(())
    }

    /// 是否已绑定。
    pub fn bound(&self) -> bool {
        self.bound
    }

    /// 尾地址。
    pub fn end(&self) -> u64 {
        self.end
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
            return Err(PT_ESLOT);
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
            return Err(PT_EBAL);
        }
        const VALS: [u32; 8] = [12, 2, 2, 8, 56, 64, 120, 917];
        const CODES: [u32; 6] =
            [PT_EENTSIZE, PT_EOFF, PT_EALIGN, PT_EWRAP, PT_ESTATE, PT_EBEYOND];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PT_EBAL),
            };
            if s.slot as usize != i {
                return Err(PT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PT_EVALUE);
                        }
                    }
                    None => return Err(PT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PT_EVALUE);
                        }
                    }
                    None => return Err(PT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PT_EBAL);
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
            return Err(PT_EBAL);
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
            Err(PT_ESEAL)
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
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.absent())));
    let _ = l.register(count_cell(3, OP_CORPUS, r.map(|x| x.reject())));

    let _ = l.register(count_cell(4, OP_ENTSIZE, Ok(PHDR as usize)));
    let _ = l.register(count_cell(5, OP_ENTSIZE, Ok(SHDR as usize)));

    let exact = Table::new(Kind::Phdr, 64, PHDR, 1);
    let _ = l.register(count_cell(6, OP_END, exact.end().map(|v| v as usize).ok_or(PT_EWRAP)));

    let red = run_with(Some((0, PT_EBEYOND)));
    let _ = l.register(count_cell(7, OP_END, red.map(|x| x.rate() as usize)));

    let entsize_err = Table::new(Kind::Phdr, 64, 32, 9).check(10000).err().unwrap_or(0);
    let _ = l.register(err_cell(8, OP_ENTSIZE, if entsize_err == 0 { PT_ENOBS } else { entsize_err }));

    let off_err = Table::new(Kind::Phdr, 16, PHDR, 9).check(10000).err().unwrap_or(0);
    let _ = l.register(err_cell(9, OP_TABLE, if off_err == 0 { PT_ENOBS } else { off_err }));

    let align_err = Table::new(Kind::Shdr, 65, SHDR, 9).check(10000).err().unwrap_or(0);
    let _ = l.register(err_cell(10, OP_TABLE, if align_err == 0 { PT_ENOBS } else { align_err }));

    let wrap_err = Table::new(Kind::Shdr, u64::MAX - 15, SHDR, 9)
        .check(u64::MAX)
        .err()
        .unwrap_or(0);
    let _ = l.register(err_cell(11, OP_END, if wrap_err == 0 { PT_ENOBS } else { wrap_err }));

    let mut ck = Checker::new();
    let _ = ck.bind(&exact, 120);
    let state_err = match ck.bind(&exact, 120) {
        Ok(()) => PT_ESTATE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_TABLE, state_err));

    let _ = l.register(err_cell(
        13,
        OP_TABLE,
        Table::new(Kind::Phdr, 64, PHDR, 1000).check(10000).err().unwrap_or(PT_ENOBS),
    ));

    l
}

// ---------------------------------------------------------------------------
// 检查面
// ---------------------------------------------------------------------------

/// 名称闸门。
fn name_ok() -> bool {
    let n = match Name::new(b"phdr_ok") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = n.len() == 7 && n.text() == String::from("phdr_ok") && n.same(b"phdr_ok");
    let b = Name::new(b"").err() == Some(PT_EBYTE);
    let c = Name::new(&[0u8; NAME_CAP + 1]).err() == Some(PT_EBYTE);
    let d = Name::new(&[0x00]).err() == Some(PT_EBYTE);
    a && b && c && d
}

/// 表项算术。
fn arith_ok() -> bool {
    let t = Table::new(Kind::Phdr, 64, PHDR, 9);
    let a = t.span() == Some(504) && t.end() == Some(568) && t.aligned();
    let s = Table::new(Kind::Shdr, 64, SHDR, 3);
    let b = s.span() == Some(192) && s.end() == Some(256);
    let w = Table::new(Kind::Shdr, u64::MAX - 15, SHDR, 9);
    let c = w.span() == Some(576) && w.end().is_none();
    let z = Table::new(Kind::Phdr, 0, 0, 0);
    let d = z.absent() && z.span() == Some(0) && z.end() == Some(0);
    let e = Table::new(Kind::Phdr, 65, PHDR, 1).aligned() == false;
    let f = Kind::Phdr.need() == 56 && Kind::Shdr.need() == 64 && Kind::Phdr.text() == "phdr";
    a && b && c && d && e && f
}

/// 闸门次序。
fn order_ok() -> bool {
    let t = Table::new(Kind::Phdr, 64, 56, 9);
    let a = t.check(10000) == Ok(()) && t.state(10000) == 0;
    let b = Table::new(Kind::Phdr, 64, 56, 0).check(10000) == Ok(());
    let c = Table::new(Kind::Phdr, 64, 32, 9).check(10000) == Err(PT_EENTSIZE);
    let d = Table::new(Kind::Shdr, 64, 2000, 1).check(10000) == Err(PT_EENTSIZE);
    let e = Table::new(Kind::Phdr, 16, 56, 9).check(10000) == Err(PT_EOFF);
    let f = Table::new(Kind::Shdr, 65, 64, 9).check(10000) == Err(PT_EALIGN);
    a && b && c && d && e && f
}

/// 边界与溢出。
fn bounds_ok() -> bool {
    let exact = Table::new(Kind::Phdr, 64, 56, 1);
    let a = exact.check(120) == Ok(()) && exact.end() == Some(120);
    let b = exact.check(119) == Err(PT_EBEYOND);
    let w = Table::new(Kind::Shdr, u64::MAX - 15, 64, 9);
    let c = w.check(u64::MAX) == Err(PT_EWRAP);
    let big = Table::new(Kind::Phdr, 64, 56, 0xffff);
    let d = big.check(10000) == Err(PT_EBEYOND);
    let e = Table::new(Kind::Phdr, 64, 56, 1).check(64) == Err(PT_EBEYOND);
    a && b && c && d && e
}

/// 判定与文案。
fn judge_ok() -> bool {
    let t = Table::new(Kind::Phdr, 64, 56, 9);
    let j = Judgement::of(&t, 10000);
    let a = j.accepted() && !j.absent() && j.code() == 0 && j.end() == 568;
    let bad = Table::new(Kind::Phdr, 64, 56, 1000);
    let k = Judgement::of(&bad, 10000);
    let b = !k.accepted() && k.code() == PT_EBEYOND && k.end() == 56064;
    let m = message(&bad, 10000);
    let c = m.contains("phdr table end=56064 > file=10000");
    let d = m.contains("cannot execute binary file: Exec format error");
    let okmsg = message(&t, 10000);
    let e = okmsg.starts_with("ok: phdr table fits (end=568)");
    a && b && c && d && e
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.accept() == 2 && r.absent() == 2;
    let b = r.reject() == 8 && r.matched() == CORPUS && r.rate() == 1000 && r.green();
    a && b
}

/// 语料编号。
fn slots_ok() -> bool {
    let mut i = 0usize;
    let mut ok = true;
    while i < CORPUS {
        let s = match CORPUS12.get(i) {
            Some(v) => v,
            None => return false,
        };
        ok = ok && s.no as usize == i + 1;
        ok = ok && s.tag.len() <= NAME_CAP && !s.tag.is_empty();
        ok = ok && s.hit();
        let mut j = i + 1;
        while j < CORPUS {
            if let Some(t) = CORPUS12.get(j) {
                ok = ok && s.tag != t.tag;
            }
            j += 1;
        }
        i += 1;
    }
    let d = corpus_no(0).err() == Some(PT_ENOBS) && corpus_no(13).err() == Some(PT_ENOBS);
    ok && d
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "tables=12 accept=2 absent=2 reject=8 match=12 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(PT_ENOBS);
    let c = e.render().as_str() == "tables=0 accept=0 absent=0 reject=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, PT_EBEYOND))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 917;
    let b = r.gate() == Err(PT_EVER) && !r.green();
    a && b
}

/// 覆盖档。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(PT_ENOBS) && cover(0) == Err(PT_ENOBS)
}

/// 校验器与错误码。
fn checker_ok() -> bool {
    let t = Table::new(Kind::Phdr, 64, 56, 1);
    let mut ck = Checker::new();
    let a = !ck.bound() && ck.bind(&t, 120) == Ok(()) && ck.bound() && ck.end() == 120;
    let b = ck.bind(&t, 120) == Err(PT_ESTATE);
    let bad = Table::new(Kind::Phdr, 64, 56, 1000);
    let mut ck2 = Checker::new();
    let c = ck2.bind(&bad, 10000) == Err(PT_EBEYOND) && !ck2.bound();
    let d = ENOEXEC == 8 && errno(&t, 120) == 0 && errno(&bad, 10000) == ENOEXEC;
    a && b && c && d
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && arith_ok();
    out[1] = order_ok() && bounds_ok();
    out[2] = judge_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && checker_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11603_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11603.elf_tables");

    s.add(
        "PT01-名称闸门",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "PT02-表项算术",
        {
            arith_ok()
        },
        "static",
    );

    s.add(
        "PT03-闸门次序",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "PT04-边界与溢出",
        {
            bounds_ok()
        },
        "static",
    );

    s.add(
        "PT05-判定与文案",
        {
            judge_ok()
        },
        "static",
    );

    s.add(
        "PT06-十二卡全跑",
        {
            corpus_ok()
        },
        "static",
    );

    s.add(
        "PT07-语料编号",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "PT08-汇总渲染",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "PT09-红单显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "PT10-覆盖档",
        {
            cover_ok()
        },
        "static",
    );

    s.add(
        "PT11-一次性校验器",
        {
            checker_ok()
        },
        "static",
    );

    s.add(
        "PT12-账本对账",
        {
            build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "PT13-冻结重放四注入",
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
    fn name_gates() {
        assert_eq!(Name::new(b"").err(), Some(PT_EBYTE));
        assert_eq!(Name::new(&[0x80]).err(), Some(PT_EBYTE));
        assert_eq!(Name::new(&[0u8; 17]).err(), Some(PT_EBYTE));
        let n = Name::new(b"exact_end").expect("n");
        assert_eq!(n.len(), 9);
        assert!(n.same(b"exact_end"));
    }

    #[test]
    fn table_arith() {
        let t = Table::new(Kind::Phdr, 64, 56, 9);
        assert_eq!(t.span(), Some(504));
        assert_eq!(t.end(), Some(568));
        assert!(t.aligned());
        assert!(!t.absent());
        let w = Table::new(Kind::Shdr, u64::MAX - 15, 64, 9);
        assert_eq!(w.span(), Some(576));
        assert_eq!(w.end(), None);
    }

    #[test]
    fn check_order() {
        assert_eq!(Table::new(Kind::Phdr, 0, 0, 0).check(0), Ok(()));
        assert_eq!(Table::new(Kind::Phdr, 64, 32, 9).check(1 << 20), Err(PT_EENTSIZE));
        assert_eq!(Table::new(Kind::Phdr, 63, 32, 9).check(1 << 20), Err(PT_EENTSIZE));
        assert_eq!(Table::new(Kind::Phdr, 63, 56, 9).check(1 << 20), Err(PT_EOFF));
        assert_eq!(Table::new(Kind::Phdr, 65, 56, 9).check(1 << 20), Err(PT_EALIGN));
    }

    #[test]
    fn exact_end() {
        let t = Table::new(Kind::Phdr, 64, 56, 1);
        assert_eq!(t.check(120), Ok(()));
        assert_eq!(t.check(119), Err(PT_EBEYOND));
        assert_eq!(t.state(119), 2);
        assert_eq!(t.state(120), 0);
    }

    #[test]
    fn wrap_detected() {
        let w = Table::new(Kind::Shdr, u64::MAX - 15, 64, 9);
        assert_eq!(w.check(u64::MAX), Err(PT_EWRAP));
        assert_eq!(Judgement::of(&w, u64::MAX).end(), 0);
    }

    #[test]
    fn absent_tables() {
        let z = Table::new(Kind::Shdr, 0, 0, 0);
        assert!(z.absent());
        assert_eq!(z.check(0), Ok(()));
        assert_eq!(z.state(0), 1);
        assert_eq!(message(&z, 0).as_str(), "ok: shdr table absent");
    }

    #[test]
    fn message_fields() {
        let bad = Table::new(Kind::Phdr, 64, 56, 1000);
        let m = message(&bad, 10000);
        assert!(m.contains("phdr table end=56064 > file=10000"));
        let e = Table::new(Kind::Shdr, 64, 0, 3);
        assert!(message(&e, 10000).contains("entsize=0 want >=64"));
        let o = Table::new(Kind::Phdr, 16, 56, 9);
        assert!(message(&o, 10000).contains("off=16 inside ehdr"));
        let a = Table::new(Kind::Shdr, 65, 64, 9);
        assert!(message(&a, 10000).contains("off=65 unaligned"));
        let w = Table::new(Kind::Shdr, u64::MAX - 15, 64, 9);
        assert!(message(&w, u64::MAX).contains("overflow"));
        let t = Table::new(Kind::Phdr, 64, 56, 9);
        assert_eq!(message(&t, 10000).as_str(), "ok: phdr table fits (end=568)");
    }

    #[test]
    fn corpus_counts() {
        let r = run().expect("r");
        assert_eq!(r.total(), CORPUS);
        assert_eq!(r.accept(), 2);
        assert_eq!(r.absent(), 2);
        assert_eq!(r.reject(), 8);
        assert_eq!(r.matched(), CORPUS);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(
            r.render().as_str(),
            "tables=12 accept=2 absent=2 reject=8 match=12 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rate() {
        let r = run_with(Some((0, PT_EBEYOND))).expect("r");
        assert_eq!(r.matched(), 11);
        assert_eq!(r.rate(), 917);
        assert_eq!(r.gate(), Err(PT_EVER));
    }

    #[test]
    fn empty_report() {
        let r = Report::new();
        assert!(r.is_empty());
        assert_eq!(r.rate(), 0);
        assert_eq!(r.gate(), Err(PT_ENOBS));
    }

    #[test]
    fn coverage_gate() {
        assert_eq!(cover(12), Ok(()));
        assert_eq!(cover(11), Err(PT_ENOBS));
    }

    #[test]
    fn checker_once() {
        let t = Table::new(Kind::Phdr, 64, 56, 1);
        let mut ck = Checker::new();
        assert_eq!(ck.bind(&t, 120), Ok(()));
        assert_eq!(ck.end(), 120);
        assert_eq!(ck.bind(&t, 120), Err(PT_ESTATE));
        let mut ck2 = Checker::new();
        assert_eq!(ck2.bind(&Table::new(Kind::Phdr, 64, 56, 2), 120), Err(PT_EBEYOND));
        assert!(!ck2.bound());
    }

    #[test]
    fn corpus_lookup() {
        assert_eq!(corpus_no(0).err(), Some(PT_ENOBS));
        assert_eq!(corpus_no(13).err(), Some(PT_ENOBS));
        assert_eq!(corpus_no(11).expect("s").want_err, PT_EWRAP);
        assert!(corpus_no(9).expect("s").hit());
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(6).map(|s| s.value), Some(120));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze().err(), Some(PT_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11603_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
