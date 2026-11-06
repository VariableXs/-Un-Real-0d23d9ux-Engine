//! UNX-F11605 e_type 不受支持档（ET_REL/ET_CORE exec 拒收）
//!
//! execve 走 load_elf_binary 时，第一件事是把 `e_ident` 认完，接着读
//! `e_type`（Elf64_Ehdr 偏移 16..18，小端 2 字节）。这一格决定镜像是什么
//! 东西：只有 `ET_EXEC`（固定地址可执行）和 `ET_DYN`（PIE / 共享对象，由
//! 装载器自己挑基址）可以当用户程序跑；`ET_REL` 是还没链接的目标文件，
//! `ET_CORE` 是进程崩溃转储，`ET_NONE` 根本没有类型，OS 私有段
//! （0xfe00..=0xfeff）与处理器私有段（0xff00..=0xffff）内核一概不认识。
//!
//! 拒收必须给出和真实内核一致的形态：`execve` 返回 -1、errno=ENOEXEC(8)、
//! shell 打 `cannot execute binary file: Exec format error`，括号里带
//! e_type 的名字与十进制值，方便对着 `readelf -h` 排查。文案要逐字稳定，
//! 否则上层脚本按字符串分支的地方会静默走错档。
//!
//! 判据次序固定：先读字节（不足 18 字节连 e_type 都读不到，落字节档），
//! 再分族（空/可重定位/可执行/共享/核心/OS 私有/处理器私有/保留），
//! 再定存亡（只有可执行与共享存活），再给拒收码（空→无类型、可重定位→
//! 目标文件、核心→转储、其余→范围），最后才拼文案。族分完不许再回读原始
//! 值，避免同一格被两种口径解释。
//!
//! 闸门次序：字节 → 族 → 存亡 → 拒收码 → 语料 → 文案 → 打开器 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfb40;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// `e_type` 在 Elf64_Ehdr 里的偏移。
pub const E_TYPE_OFF: usize = 16;

/// `e_type` 宽度。
pub const E_TYPE_SIZE: usize = 2;

/// 读 e_type 所需的最短头长。
pub const MIN_EHDR: usize = E_TYPE_OFF + E_TYPE_SIZE;

/// `ENOEXEC`。
pub const ENOEXEC: i32 = 8;

// ---------------------------------------------------------------------------
// e_type 取值
// ---------------------------------------------------------------------------

/// 无类型。
pub const ET_NONE: u16 = 0x0000;

/// 可重定位目标文件。
pub const ET_REL: u16 = 0x0001;

/// 固定地址可执行。
pub const ET_EXEC: u16 = 0x0002;

/// 共享对象 / PIE。
pub const ET_DYN: u16 = 0x0003;

/// 核心转储。
pub const ET_CORE: u16 = 0x0004;

/// OS 私有段下界。
pub const ET_LOOS: u16 = 0xfe00;

/// OS 私有段上界。
pub const ET_HIOS: u16 = 0xfeff;

/// 处理器私有段下界。
pub const ET_LOPROC: u16 = 0xff00;

/// 处理器私有段上界。
pub const ET_HIPROC: u16 = 0xffff;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 可重定位目标文件拒收。
pub const ET_EREL: u32 = SEG_BASE + 0x01;
/// 核心转储拒收。
pub const ET_ECORE: u32 = SEG_BASE + 0x02;
/// 无类型拒收。
pub const ET_ENONE: u32 = SEG_BASE + 0x03;
/// 范围外/私有权值拒收。
pub const ET_ERANGE: u32 = SEG_BASE + 0x04;
/// 状态冲突。
pub const ET_ESTATE: u32 = SEG_BASE + 0x05;
/// 未达绿档。
pub const ET_EVER: u32 = SEG_BASE + 0x06;
/// 字节非法。
pub const ET_EBYTE: u32 = SEG_BASE + 0x07;
/// 语料重复。
pub const ET_EDUP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const ET_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const ET_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const ET_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const ET_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const ET_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 读 e_type。
pub const OP_ETYPE: u8 = 0x01;
/// 分族。
pub const OP_FAMILY: u8 = 0x02;
/// 存亡判定。
pub const OP_FATE: u8 = 0x03;
/// 文案。
pub const OP_MSG: u8 = 0x04;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x05;
/// 字段读取。
pub const OP_FIELD: u8 = 0x06;
/// 打开器。
pub const OP_OPEN: u8 = 0x07;

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

/// 两字符小写十六进制文本（不含前缀）。
fn hex4_text(v: u16) -> String {
    let mut s = String::new();
    let mut shift = 12u32;
    loop {
        let nib = ((v >> shift) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
        s.push(c as char);
        if shift == 0 {
            break;
        }
        shift -= 4;
    }
    s
}

// ---------------------------------------------------------------------------
// 族与存亡
// ---------------------------------------------------------------------------

/// e_type 族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// 无类型。
    Empty,
    /// 可重定位目标。
    Relocatable,
    /// 固定地址可执行。
    Executable,
    /// 共享对象 / PIE。
    Shared,
    /// 核心转储。
    Core,
    /// OS 私有。
    Os,
    /// 处理器私有。
    Proc,
    /// 保留 / 未知。
    Reserved,
}

impl Family {
    /// 族名。
    pub fn name(&self) -> &'static str {
        match self {
            Family::Empty => "empty",
            Family::Relocatable => "relocatable",
            Family::Executable => "executable",
            Family::Shared => "shared",
            Family::Core => "core",
            Family::Os => "os-specific",
            Family::Proc => "proc-specific",
            Family::Reserved => "reserved",
        }
    }

    /// 族序号（用于账本观测）。
    pub fn index(&self) -> u32 {
        match self {
            Family::Empty => 0,
            Family::Relocatable => 1,
            Family::Executable => 2,
            Family::Shared => 3,
            Family::Core => 4,
            Family::Os => 5,
            Family::Proc => 6,
            Family::Reserved => 7,
        }
    }
}

/// 存亡。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    /// 可当用户程序跑。
    Accept,
    /// 内核拒收。
    Reject,
}

impl Fate {
    /// 是否存活。
    pub fn alive(&self) -> bool {
        match self {
            Fate::Accept => true,
            Fate::Reject => false,
        }
    }

    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Fate::Accept => "accept",
            Fate::Reject => "reject",
        }
    }
}

// ---------------------------------------------------------------------------
// e_type
// ---------------------------------------------------------------------------

/// e_type 原始值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Etype {
    raw: u16,
}

impl Etype {
    /// 构造。
    pub const fn new(raw: u16) -> Etype {
        Etype { raw }
    }

    /// 原始值。
    pub const fn raw(&self) -> u16 {
        self.raw
    }

    /// 分族。
    pub fn family(&self) -> Family {
        match self.raw {
            ET_NONE => Family::Empty,
            ET_REL => Family::Relocatable,
            ET_EXEC => Family::Executable,
            ET_DYN => Family::Shared,
            ET_CORE => Family::Core,
            ET_LOOS..=ET_HIOS => Family::Os,
            ET_LOPROC..=ET_HIPROC => Family::Proc,
            _ => Family::Reserved,
        }
    }

    /// 存亡。
    pub fn fate(&self) -> Fate {
        match self.family() {
            Family::Executable => Fate::Accept,
            Family::Shared => Fate::Accept,
            _ => Fate::Reject,
        }
    }

    /// 拒收码；存活时给 0。
    pub fn reject_code(&self) -> u32 {
        match self.family() {
            Family::Empty => ET_ENONE,
            Family::Relocatable => ET_EREL,
            Family::Core => ET_ECORE,
            Family::Executable => 0,
            Family::Shared => 0,
            Family::Os => ET_ERANGE,
            Family::Proc => ET_ERANGE,
            Family::Reserved => ET_ERANGE,
        }
    }

    /// 名称。
    pub fn name(&self) -> &'static str {
        match self.raw {
            ET_NONE => "ET_NONE",
            ET_REL => "ET_REL",
            ET_EXEC => "ET_EXEC",
            ET_DYN => "ET_DYN",
            ET_CORE => "ET_CORE",
            ET_LOOS => "ET_LOOS",
            ET_HIOS => "ET_HIOS",
            ET_LOPROC => "ET_LOPROC",
            ET_HIPROC => "ET_HIPROC",
            _ => "ET_UNKNOWN",
        }
    }

    /// `ET_REL/1` 形态的标签。
    pub fn tag(&self) -> String {
        let mut s = String::from(self.name());
        s.push('/');
        s.push_str(u32_text(self.raw as u32).as_str());
        s
    }

    /// 细节短语。
    pub fn detail(&self) -> &'static str {
        match self.family() {
            Family::Empty => "no file type",
            Family::Relocatable => "relocatable object",
            Family::Executable => "executable",
            Family::Shared => "shared object (PIE)",
            Family::Core => "core dump",
            Family::Os => "os-specific type",
            Family::Proc => "processor-specific type",
            Family::Reserved => "reserved type",
        }
    }

    /// 判定。
    pub fn check(&self) -> Result<(), u32> {
        match self.fate() {
            Fate::Accept => Ok(()),
            Fate::Reject => Err(self.reject_code()),
        }
    }

    /// 是否 16 位范围外的伪值（不可能，但保留闸门语义）。
    pub fn sane(&self) -> bool {
        self.raw <= ET_HIPROC
    }
}

// ---------------------------------------------------------------------------
// 判定与文案
// ---------------------------------------------------------------------------

/// 一次判定。
#[derive(Clone, Copy, Debug)]
pub struct Judgement {
    etype: Etype,
    fate: Fate,
    code: u32,
}

impl Judgement {
    /// 类型。
    pub const fn etype(&self) -> Etype {
        self.etype
    }

    /// 存亡。
    pub const fn fate(&self) -> Fate {
        self.fate
    }

    /// 拒收码。
    pub const fn code(&self) -> u32 {
        self.code
    }

    /// 是否存活。
    pub fn accepted(&self) -> bool {
        self.fate.alive()
    }
}

/// 判一条 e_type。
pub fn judge(raw: u16) -> Judgement {
    let etype = Etype::new(raw);
    let fate = etype.fate();
    let code = match fate {
        Fate::Accept => 0,
        Fate::Reject => etype.reject_code(),
    };
    Judgement { etype, fate, code }
}

/// 文案：存活给 ok 行，拒收给 execve 形态。
pub fn message(j: &Judgement, path: &str) -> Result<String, u32> {
    if path.is_empty() {
        return Err(ET_EBYTE);
    }
    let mut s = String::new();
    match j.fate {
        Fate::Accept => {
            s.push_str("ok: e_type=");
            s.push_str(j.etype.tag().as_str());
            s.push(' ');
            s.push_str(j.etype.detail());
            s.push('\n');
        }
        Fate::Reject => {
            s.push_str("bash: ");
            s.push_str(path);
            s.push_str(": cannot execute binary file: Exec format error (e_type=");
            s.push_str(j.etype.tag().as_str());
            s.push_str(", ");
            s.push_str(j.etype.detail());
            s.push_str(")\n");
        }
    }
    Ok(s)
}

/// errno。
pub fn errno(j: &Judgement) -> i32 {
    match j.fate {
        Fate::Accept => 0,
        Fate::Reject => ENOEXEC,
    }
}

// ---------------------------------------------------------------------------
// 字段读取
// ---------------------------------------------------------------------------

/// 最小头缓冲（只装到 e_type 结尾）。
#[derive(Clone, Copy, Debug)]
pub struct Field {
    buf: [u8; MIN_EHDR],
    len: usize,
}

impl Field {
    /// 从字节切片取（多截少补零，长度按实长记）。
    pub fn new(bytes: &[u8]) -> Field {
        let mut buf = [0u8; MIN_EHDR];
        let mut i = 0usize;
        while i < MIN_EHDR && i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                buf[i] = *b;
            }
            i += 1;
        }
        let len = if bytes.len() > MIN_EHDR { MIN_EHDR } else { bytes.len() };
        Field { buf, len }
    }

    /// 实长。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否够读 e_type。
    pub fn complete(&self) -> bool {
        self.len >= MIN_EHDR
    }

    /// 缺口。
    pub fn short_by(&self) -> usize {
        MIN_EHDR.saturating_sub(self.len)
    }

    /// 取一字节。
    pub fn byte_at(&self, off: usize) -> Result<u8, u32> {
        if off >= self.len {
            return Err(ET_EBYTE);
        }
        match self.buf.get(off) {
            Some(b) => Ok(*b),
            None => Err(ET_EBYTE),
        }
    }

    /// 小端 16 位。
    pub fn le16(&self, off: usize) -> Result<u16, u32> {
        let lo = self.byte_at(off)? as u16;
        let hi = self.byte_at(off + 1)? as u16;
        Ok(lo | (hi << 8))
    }

    /// 读 e_type。
    pub fn e_type(&self) -> Result<Etype, u32> {
        if !self.complete() {
            return Err(ET_EBYTE);
        }
        match self.le16(E_TYPE_OFF) {
            Ok(v) => Ok(Etype::new(v)),
            Err(c) => Err(c),
        }
    }

    /// 摘要（覆盖实长）。
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

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 原始 e_type。
    pub raw: u16,
    /// 期望存亡。
    pub expect: Fate,
    /// 期望拒收码（存活为 0）。
    pub code: u32,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(raw: u16, expect: Fate, code: u32, note: &'static str) -> Slot {
        Slot { raw, expect, code, note }
    }
}

/// 十四槽语料：两活十二死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(ET_NONE, Fate::Reject, ET_ENONE, "none"),
    Slot::new(ET_REL, Fate::Reject, ET_EREL, "rel-object"),
    Slot::new(ET_EXEC, Fate::Accept, 0, "exec"),
    Slot::new(ET_DYN, Fate::Accept, 0, "pie-dyn"),
    Slot::new(ET_CORE, Fate::Reject, ET_ECORE, "core-dump"),
    Slot::new(0x0005, Fate::Reject, ET_ERANGE, "reserved05"),
    Slot::new(0x0006, Fate::Reject, ET_ERANGE, "reserved06"),
    Slot::new(0x0007, Fate::Reject, ET_ERANGE, "reserved07"),
    Slot::new(ET_LOOS, Fate::Reject, ET_ERANGE, "loos"),
    Slot::new(ET_HIOS, Fate::Reject, ET_ERANGE, "hios"),
    Slot::new(ET_LOPROC, Fate::Reject, ET_ERANGE, "loproc"),
    Slot::new(ET_HIPROC, Fate::Reject, ET_ERANGE, "hiproc"),
    Slot::new(0x1000, Fate::Reject, ET_ERANGE, "unknown1000"),
    Slot::new(0x8000, Fate::Reject, ET_ERANGE, "unknown8000"),
];

/// 语料槽。
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
                if b.raw == a.raw {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注是否全部非空。
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

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    total: usize,
    accept: usize,
    reject: usize,
    rel: usize,
    core: usize,
    none: usize,
    range: usize,
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
            rel: 0,
            core: 0,
            none: 0,
            range: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 存活条数。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收条数。
    pub const fn reject(&self) -> usize {
        self.reject
    }

    /// 目标文件拒收数。
    pub const fn rel(&self) -> usize {
        self.rel
    }

    /// 核心转储拒收数。
    pub const fn core(&self) -> usize {
        self.core
    }

    /// 无类型拒收数。
    pub const fn none(&self) -> usize {
        self.none
    }

    /// 范围拒收数。
    pub const fn range(&self) -> usize {
        self.range
    }

    /// 对上的条数。
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
            return Err(ET_ENOBS);
        }
        if self.red {
            return Err(ET_EVER);
        }
        if self.matched != self.total {
            return Err(ET_EVALUE);
        }
        if self.accept != 2 || self.reject != 12 {
            return Err(ET_EVALUE);
        }
        if self.rel != 1 || self.core != 1 || self.none != 1 || self.range != 9 {
            return Err(ET_EVALUE);
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
        s.push_str("etype corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" rel=");
        s.push_str(u32_text(self.rel as u32).as_str());
        s.push_str(" core=");
        s.push_str(u32_text(self.core as u32).as_str());
        s.push_str(" none=");
        s.push_str(u32_text(self.none as u32).as_str());
        s.push_str(" range=");
        s.push_str(u32_text(self.range as u32).as_str());
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
            None => return Err(ET_EVALUE),
        };
        let j = judge(slot.raw);
        r.total += 1;
        match j.fate {
            Fate::Accept => {
                r.accept += 1;
                if slot.expect == Fate::Accept {
                    r.matched += 1;
                }
            }
            Fate::Reject => {
                r.reject += 1;
                match j.code {
                    ET_EREL => r.rel += 1,
                    ET_ECORE => r.core += 1,
                    ET_ENONE => r.none += 1,
                    ET_ERANGE => r.range += 1,
                    _ => {}
                }
                if slot.expect == Fate::Reject && slot.code == j.code {
                    r.matched += 1;
                }
            }
        }
        i += 1;
    }
    if let Some((slot, _code)) = red {
        if slot >= r.total {
            return Err(ET_ESLOT);
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
        Err(ET_ENOBS)
    }
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
        let j = judge(slot.raw);
        if j.fate != slot.expect {
            return false;
        }
        if j.code != slot.code {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 打开器
// ---------------------------------------------------------------------------

/// 一次打开：只允许打开一扇门。
#[derive(Clone, Copy, Debug)]
pub struct Opener {
    opened: u32,
    raw: u16,
    code: u32,
}

impl Opener {
    /// 空。
    pub const fn new() -> Opener {
        Opener { opened: 0, raw: 0, code: 0 }
    }

    /// 打开。
    pub fn open(&mut self, e: Etype) -> Result<(), u32> {
        if self.opened > 0 {
            self.code = ET_ESTATE;
            return Err(ET_ESTATE);
        }
        self.opened = 1;
        self.raw = e.raw();
        match e.check() {
            Ok(()) => {
                self.code = 0;
                Ok(())
            }
            Err(c) => {
                self.code = c;
                Err(c)
            }
        }
    }

    /// 已开次数。
    pub const fn opened(&self) -> u32 {
        self.opened
    }

    /// 最后 raw。
    pub const fn raw(&self) -> u16 {
        self.raw
    }

    /// 最后码。
    pub const fn code(&self) -> u32 {
        self.code
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
            return Err(ET_ESLOT);
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
            return Err(ET_EBAL);
        }
        const VALS: [u32; 8] = [14, 2, 12, 1, 1, 1, 9, 929];
        const CODES: [u32; 6] = [ET_EREL, ET_ECORE, ET_ENONE, ET_ERANGE, ET_ESTATE, ET_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(ET_EBAL),
            };
            if s.slot as usize != i {
                return Err(ET_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(ET_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(ET_EVALUE);
                        }
                    }
                    None => return Err(ET_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(ET_EVALUE);
                        }
                    }
                    None => return Err(ET_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(ET_EBAL);
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
            return Err(ET_EBAL);
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
            Err(ET_ESEAL)
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
    let _ = l.register(count_cell(3, OP_CORPUS, r.map(|x| x.rel())));

    let _ = l.register(count_cell(4, OP_CORPUS, r.map(|x| x.core())));
    let _ = l.register(count_cell(5, OP_CORPUS, r.map(|x| x.none())));
    let _ = l.register(count_cell(6, OP_CORPUS, r.map(|x| x.range())));

    let red = run_with(Some((0, ET_EREL)));
    let _ = l.register(count_cell(7, OP_ETYPE, red.map(|x| x.rate() as usize)));

    let rel_err = match Etype::new(ET_REL).check() {
        Ok(()) => ET_ENOBS,
        Err(c) => c,
    };
    let _ = l.register(err_cell(8, OP_FATE, rel_err));

    let core_err = match Etype::new(ET_CORE).check() {
        Ok(()) => ET_ENOBS,
        Err(c) => c,
    };
    let _ = l.register(err_cell(9, OP_FATE, core_err));

    let none_err = match Etype::new(ET_NONE).check() {
        Ok(()) => ET_ENOBS,
        Err(c) => c,
    };
    let _ = l.register(err_cell(10, OP_FATE, none_err));

    let range_err = match Etype::new(ET_LOOS).check() {
        Ok(()) => ET_ENOBS,
        Err(c) => c,
    };
    let _ = l.register(err_cell(11, OP_FAMILY, range_err));

    let mut op = Opener::new();
    let _ = op.open(Etype::new(ET_EXEC));
    let state_err = match op.open(Etype::new(ET_DYN)) {
        Ok(()) => ET_ESTATE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_OPEN, state_err));

    let short = [0u8; MIN_EHDR - 1];
    let f = Field::new(&short);
    let byte_err = match f.e_type() {
        Ok(_) => ET_EBYTE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_FIELD, byte_err));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称闸门。
fn name_ok() -> bool {
    Etype::new(ET_NONE).name() == "ET_NONE"
        && Etype::new(ET_REL).name() == "ET_REL"
        && Etype::new(ET_EXEC).name() == "ET_EXEC"
        && Etype::new(ET_DYN).name() == "ET_DYN"
        && Etype::new(ET_CORE).name() == "ET_CORE"
        && Etype::new(ET_LOOS).name() == "ET_LOOS"
        && Etype::new(ET_HIOS).name() == "ET_HIOS"
        && Etype::new(ET_LOPROC).name() == "ET_LOPROC"
        && Etype::new(ET_HIPROC).name() == "ET_HIPROC"
        && Etype::new(0x1234).name() == "ET_UNKNOWN"
        && Etype::new(ET_REL).tag().as_str() == "ET_REL/1"
        && Etype::new(ET_EXEC).tag().as_str() == "ET_EXEC/2"
        && Etype::new(0x1234).tag().as_str() == "ET_UNKNOWN/4660"
}

/// 族分档。
fn family_ok() -> bool {
    let a = Etype::new(ET_NONE).family() == Family::Empty;
    let b = Etype::new(ET_REL).family() == Family::Relocatable;
    let c = Etype::new(ET_EXEC).family() == Family::Executable;
    let d = Etype::new(ET_DYN).family() == Family::Shared;
    let e = Etype::new(ET_CORE).family() == Family::Core;
    let f = Etype::new(0xfdff).family() == Family::Reserved;
    let g = Etype::new(ET_LOOS).family() == Family::Os;
    let h = Etype::new(ET_HIOS).family() == Family::Os;
    let i = Etype::new(ET_LOPROC).family() == Family::Proc;
    let j = Etype::new(ET_HIPROC).family() == Family::Proc;
    a && b && c && d && e && f && g && h && i && j
}

/// 存亡判定。
fn fate_ok() -> bool {
    let a = Etype::new(ET_EXEC).fate() == Fate::Accept;
    let b = Etype::new(ET_DYN).fate() == Fate::Accept;
    let c = Etype::new(ET_REL).fate() == Fate::Reject;
    let d = Etype::new(ET_CORE).fate() == Fate::Reject;
    let e = Etype::new(ET_NONE).fate() == Fate::Reject;
    let f = Etype::new(ET_LOPROC).fate() == Fate::Reject;
    let g = Fate::Accept.alive() && !Fate::Reject.alive();
    a && b && c && d && e && f && g
}

/// 拒收码映射。
fn codes_ok() -> bool {
    let a = Etype::new(ET_REL).reject_code() == ET_EREL;
    let b = Etype::new(ET_CORE).reject_code() == ET_ECORE;
    let c = Etype::new(ET_NONE).reject_code() == ET_ENONE;
    let d = Etype::new(0x0100).reject_code() == ET_ERANGE;
    let e = Etype::new(ET_LOOS).reject_code() == ET_ERANGE;
    let f = Etype::new(ET_LOPROC).reject_code() == ET_ERANGE;
    let g = Etype::new(ET_EXEC).reject_code() == 0;
    let h = Etype::new(ET_DYN).reject_code() == 0;
    a && b && c && d && e && f && g && h
}

/// 判据汇总。
fn judge_ok() -> bool {
    let a = judge(ET_EXEC).accepted() && judge(ET_EXEC).code() == 0;
    let b = !judge(ET_REL).accepted() && judge(ET_REL).code() == ET_EREL;
    let c = judge(ET_CORE).code() == ET_ECORE;
    let d = judge(ET_NONE).code() == ET_ENONE;
    let e = judge(ET_HIPROC).code() == ET_ERANGE;
    let f = errno(&judge(ET_EXEC)) == 0 && errno(&judge(ET_REL)) == ENOEXEC;
    a && b && c && d && e && f
}

/// 字段读取。
fn field_ok() -> bool {
    let mut buf = [0u8; MIN_EHDR + 2];
    buf[E_TYPE_OFF] = 0x02;
    buf[E_TYPE_OFF + 1] = 0x00;
    let f = Field::new(&buf);
    let a = f.complete() && f.len() == MIN_EHDR && f.byte_at(E_TYPE_OFF) == Ok(0x02);
    let b = match f.e_type() {
        Ok(e) => e == Etype::new(ET_EXEC),
        Err(_) => false,
    };
    let c = f.le16(E_TYPE_OFF) == Ok(0x0002);
    let d = f.le16(MIN_EHDR) == Err(ET_EBYTE);
    let mut buf2 = [0u8; MIN_EHDR];
    buf2[E_TYPE_OFF] = 0x03;
    buf2[E_TYPE_OFF + 1] = 0x00;
    let g = match Field::new(&buf2).e_type() {
        Ok(e) => e == Etype::new(ET_DYN),
        Err(_) => false,
    };
    a && b && c && d && g
}

/// 短头拒收。
fn short_ok() -> bool {
    let a = {
        let f = Field::new(&[0u8; 0]);
        f.is_empty() && f.short_by() == MIN_EHDR && f.e_type() == Err(ET_EBYTE)
    };
    let b = {
        let f = Field::new(&[0u8; MIN_EHDR - 1]);
        !f.complete() && f.short_by() == 1 && f.e_type() == Err(ET_EBYTE)
    };
    let c = {
        let f = Field::new(&[0u8; MIN_EHDR]);
        f.complete() && f.short_by() == 0
    };
    a && b && c
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.accept() == 2 && r.reject() == 12 && r.matched() == CORPUS;
    let b = r.rel() == 1 && r.core() == 1 && r.none() == 1 && r.range() == 9;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    a && b && c
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(s) => s.raw == ET_NONE && s.expect == Fate::Reject,
        None => false,
    };
    let b = match slot_at(2) {
        Some(s) => s.raw == ET_EXEC && s.expect == Fate::Accept && s.code == 0,
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
        == "etype corpus=14 accept=2 reject=12 rel=1 core=1 none=1 range=9 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(ET_ENOBS);
    let c = e.render().as_str()
        == "etype corpus=0 accept=0 reject=0 rel=0 core=0 none=0 range=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, ET_EREL))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(ET_EVER) && !r.green();
    c_red_ok() && a && b
}

/// 红档越位。
fn c_red_ok() -> bool {
    run_with(Some((CORPUS, ET_EREL))).err() == Some(ET_ESLOT)
        && run_with(Some((CORPUS + 1, ET_EREL))).err() == Some(ET_ESLOT)
}

/// 封面。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(ET_ENOBS) && cover(0) == Err(ET_ENOBS)
}

/// 文案逐字。
fn message_ok() -> bool {
    let a = match message(&judge(ET_REL), "./a.out") {
        Ok(s) => s.as_str()
            == "bash: ./a.out: cannot execute binary file: Exec format error (e_type=ET_REL/1, relocatable object)\n",
        Err(_) => false,
    };
    let b = match message(&judge(ET_CORE), "/tmp/core") {
        Ok(s) => s.as_str()
            == "bash: /tmp/core: cannot execute binary file: Exec format error (e_type=ET_CORE/4, core dump)\n",
        Err(_) => false,
    };
    let c = match message(&judge(ET_EXEC), "./prog") {
        Ok(s) => s.as_str() == "ok: e_type=ET_EXEC/2 executable\n",
        Err(_) => false,
    };
    let d = match message(&judge(ET_DYN), "./pie") {
        Ok(s) => s.as_str() == "ok: e_type=ET_DYN/3 shared object (PIE)\n",
        Err(_) => false,
    };
    let e = message(&judge(ET_EXEC), "") == Err(ET_EBYTE);
    let f = match message(&judge(0x1234), "./x") {
        Ok(s) => s
            .as_str()
            .contains("e_type=ET_UNKNOWN/4660, reserved type")
            && s.as_str().contains("cannot execute binary file"),
        Err(_) => false,
    };
    a && b && c && d && e && f
}

/// 打开器。
fn opener_ok() -> bool {
    let mut o = Opener::new();
    let a = o.opened() == 0 && o.code() == 0;
    let b = o.open(Etype::new(ET_EXEC)) == Ok(()) && o.opened() == 1 && o.raw() == ET_EXEC;
    let c = o.open(Etype::new(ET_DYN)) == Err(ET_ESTATE) && o.code() == ET_ESTATE;
    let mut p = Opener::new();
    let d = p.open(Etype::new(ET_REL)) == Err(ET_EREL) && p.code() == ET_EREL && p.raw() == ET_REL;
    let e = p.open(Etype::new(ET_EXEC)) == Err(ET_ESTATE);
    a && b && c && d && e
}

/// 族序号。
fn index_ok() -> bool {
    Family::Empty.index() == 0
        && Family::Relocatable.index() == 1
        && Family::Executable.index() == 2
        && Family::Shared.index() == 3
        && Family::Core.index() == 4
        && Family::Os.index() == 5
        && Family::Proc.index() == 6
        && Family::Reserved.index() == 7
}

/// 摘要稳定。
fn digest_ok() -> bool {
    let mut buf = [0u8; MIN_EHDR];
    buf[E_TYPE_OFF] = 0x02;
    let a = Field::new(&buf).seal() == Field::new(&buf).seal();
    let mut buf2 = [0u8; MIN_EHDR];
    buf2[E_TYPE_OFF] = 0x03;
    let b = Field::new(&buf).seal() != Field::new(&buf2).seal();
    let c = digest(b"abcdef") == digest(b"abcdef") && digest(b"abcdef") != digest(b"abcdeg");
    a && b && c
}

/// 十六进制文本。
fn hex_ok() -> bool {
    hex4_text(0x0000).as_str() == "0000"
        && hex4_text(0x0008).as_str() == "0008"
        && hex4_text(0x00ff).as_str() == "00ff"
        && hex4_text(0xabcd).as_str() == "abcd"
        && hex4_text(0xffff).as_str() == "ffff"
}

/// 十进制文本。
fn text_ok() -> bool {
    u32_text(0).as_str() == "0"
        && u32_text(7).as_str() == "7"
        && u32_text(10).as_str() == "10"
        && u32_text(4660).as_str() == "4660"
        && u32_text(4294967295).as_str() == "4294967295"
}

/// 列表字符串。
fn text_ok2() -> bool {
    let a = String::from("ET_REL/1").contains("ET_REL");
    let b = String::from("Exec format error").contains("format");
    let c = Family::Executable.name() == "executable";
    let d = Fate::Accept.text() == "accept" && Fate::Reject.text() == "reject";
    let e = Etype::new(ET_DYN).detail() == "shared object (PIE)";
    a && b && c && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && family_ok() && index_ok();
    out[1] = fate_ok() && codes_ok() && judge_ok();
    out[2] = field_ok() && short_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && message_ok() && opener_ok()
        && digest_ok() && hex_ok() && text_ok() && text_ok2()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11605_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11605.etype_gate");

    s.add("ET01-名称闸门", name_ok(), "static");
    s.add("ET02-族分档", family_ok() && index_ok(), "static");
    s.add("ET03-存亡判定", fate_ok() && codes_ok(), "static");
    s.add("ET04-判定汇总", judge_ok(), "static");
    s.add("ET05-字段读取", field_ok() && short_ok(), "static");
    s.add("ET06-十四卡全跑", corpus_ok(), "static");
    s.add("ET07-语料编号", slots_ok(), "static");
    s.add("ET08-汇总渲染", report_ok(), "static");
    s.add("ET09-红单显式", reds_ok(), "static");
    s.add("ET10-封面", cover_ok(), "static");
    s.add("ET11-文案逐字", message_ok(), "static");
    s.add("ET12-打开器与摘要", opener_ok() && digest_ok() && hex_ok() && text_ok() && text_ok2(), "static");
    s.add("ET13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
    }

    #[test]
    fn family_boundaries() {
        assert_eq!(Etype::new(0xfdff).family(), Family::Reserved);
        assert_eq!(Etype::new(ET_LOOS).family(), Family::Os);
        assert_eq!(Etype::new(ET_HIOS).family(), Family::Os);
        assert_eq!(Etype::new(ET_LOPROC).family(), Family::Proc);
        assert_eq!(Etype::new(ET_HIPROC).family(), Family::Proc);
        assert_eq!(Etype::new(4).family(), Family::Core);
    }

    #[test]
    fn fate_policy() {
        assert!(Etype::new(ET_EXEC).fate().alive());
        assert!(Etype::new(ET_DYN).fate().alive());
        assert!(!Etype::new(ET_REL).fate().alive());
        assert!(!Etype::new(ET_CORE).fate().alive());
        assert!(!Etype::new(ET_NONE).fate().alive());
    }

    #[test]
    fn reject_codes() {
        assert_eq!(Etype::new(ET_REL).check(), Err(ET_EREL));
        assert_eq!(Etype::new(ET_CORE).check(), Err(ET_ECORE));
        assert_eq!(Etype::new(ET_NONE).check(), Err(ET_ENONE));
        assert_eq!(Etype::new(0xff05).check(), Err(ET_ERANGE));
        assert_eq!(Etype::new(ET_EXEC).check(), Ok(()));
        assert_eq!(Etype::new(ET_DYN).check(), Ok(()));
    }

    #[test]
    fn field_le16() {
        let mut buf = [0u8; MIN_EHDR];
        buf[16] = 0x03;
        buf[17] = 0x00;
        let f = Field::new(&buf);
        assert!(f.complete());
        assert_eq!(f.le16(16), Ok(3));
        assert_eq!(f.le16(18), Err(ET_EBYTE));
        assert_eq!(f.le16(17), Err(ET_EBYTE));
    }

    #[test]
    fn field_short() {
        let f = Field::new(&[0u8; 4]);
        assert!(!f.complete());
        assert_eq!(f.short_by(), MIN_EHDR - 4);
        assert_eq!(f.e_type(), Err(ET_EBYTE));
    }

    #[test]
    fn judge_exec_ok() {
        let j = judge(ET_EXEC);
        assert!(j.accepted());
        assert_eq!(j.code(), 0);
        assert_eq!(errno(&j), 0);
        assert_eq!(j.etype().name(), "ET_EXEC");
    }

    #[test]
    fn judge_rel_reject() {
        let j = judge(ET_REL);
        assert!(!j.accepted());
        assert_eq!(j.code(), ET_EREL);
        assert_eq!(errno(&j), ENOEXEC);
    }

    #[test]
    fn message_rel() {
        let j = judge(ET_REL);
        let m = match message(&j, "./a.out") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(m.as_str().contains("cannot execute binary file: Exec format error"));
        assert!(m.as_str().contains("e_type=ET_REL/1, relocatable object"));
    }

    #[test]
    fn message_exec() {
        let j = judge(ET_DYN);
        let m = match message(&j, "./pie") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(m.as_str(), "ok: e_type=ET_DYN/3 shared object (PIE)\n");
    }

    #[test]
    fn message_empty_path() {
        assert_eq!(message(&judge(ET_EXEC), ""), Err(ET_EBYTE));
        assert_eq!(message(&judge(ET_REL), ""), Err(ET_EBYTE));
    }

    #[test]
    fn report_render() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(
            r.render().as_str(),
            "etype corpus=14 accept=2 reject=12 rel=1 core=1 none=1 range=9 match=14 rate=1000ppm\n"
        );
        assert_eq!(r.gate(), Ok(()));
        assert!(r.green());
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, ET_EREL))) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(r.matched(), CORPUS - 1);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(ET_EVER));
        assert_eq!(run_with(Some((14, ET_EREL))).err(), Some(ET_ESLOT));
    }

    #[test]
    fn opener_once() {
        let mut o = Opener::new();
        assert_eq!(o.open(Etype::new(ET_EXEC)), Ok(()));
        assert_eq!(o.open(Etype::new(ET_EXEC)), Err(ET_ESTATE));
        let mut p = Opener::new();
        assert_eq!(p.open(Etype::new(ET_CORE)), Err(ET_ECORE));
        assert_eq!(p.code(), ET_ECORE);
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
        assert_eq!(l.freeze(), Err(ET_EBAL));
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
        let s = run_unx11605_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
