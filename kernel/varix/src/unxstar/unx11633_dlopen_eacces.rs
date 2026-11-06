//! UNX-F11633 目录/不可读文件 dlopen EACCES 文案判据
//!
//! `dlopen` 拿到 EACCES（errno 13）从来不是一件事，而是三件不同的事：
//!
//! 1. **对象本身不可读**：文件权限位没有给调用者读位。打开阶段就死，
//!    `dlerror` 文本是 `cannot open shared object file: Permission denied`。
//! 2. **搜索目录不可穿越**：`LD_LIBRARY_PATH` / 默认路径里某一级目录没有
//!    执行位，路径根本走不到对象，定位阶段就死。文本**仍然**是
//!    `cannot open shared object file: Permission denied`，但阶段是 lookup，
//!    错误对象是目录而不是文件。只比 errno 的判据在这里会把两者混为一谈。
//! 3. **可执行映射被挂载点禁止**：文件读得进、ELF 解析得过，`mmap` 要
//!    `PROT_EXEC` 时被 `noexec` 挂载点拒绝，映射阶段死。文本变成
//!    `failed to map segment from shared object: Permission denied`——同一
//!    个 errno，第三种文案。
//!
//! 本模块把这三条线与"目录被当成 .so 打开"这条非权限线放在同一张表里：
//!
//! - 目录目标：`read()` 在目录上返回 EISDIR，文案
//!   `cannot read file data: Is a directory`。目录的**类别判定先于**权限
//!   判定——一个 `0111`（只有 x 没有 r）的目录依旧得到 EISDIR 而不是
//!   EACCES，因为 `open()` 成功、`read()` 才失败，glibc 的首次读就撞上
//!   目录。判据若把目录先按权限位判，会把一半目录档写成权限档。
//! - 路径中段是普通文件：ENOTDIR，文案 `Not a directory`，与"文件不存在"
//!   严格分开；否则运维会把"写错层级"当成"没装包"。
//! - 符号链接环：ELOOP，文案 `Too many levels of symbolic links`。
//!
//! 因此一条判据至少要有五个维度同时成立：阶段（lookup/stat/classify/
//! access/map）、errno、根因档（dir/perm/ok/other）、文案逐字、以及申报
//! 与实际是否一致。少任何一维，同一份输入都能被两种解释说得通。
//!
//! 三条 EACCES 的共同点只有 errno；差别全在阶段与文案：
//!
//! | 阶段     | 根因           | 文案                                         |
//! |----------|----------------|----------------------------------------------|
//! | lookup   | 搜索目录无 x   | `<name>: cannot open shared object file: Permission denied` |
//! | access   | 文件无读位     | `<name>: cannot open shared object file: Permission denied` |
//! | map      | noexec 挂载    | `<name>: failed to map segment from shared object: Permission denied` |
//!
//! 注意 lookup 与 access 的文案完全相同：这正是"只对文案"会漏判的地方，
//! 所以本模块的判序是 **状态 → 阶段 → errno → 根因 → 文案**，文案放在
//! 最后一维（它是前三者的确定性函数，反过来不成立）。
//!
//! 权限位模型按经典九位三档（owner/group/other）+ 读/写/执行位，root
//! （euid 0）绕过 DAC 读位。组档的判定次序是"先 owner、再 group、最后
//! other"，任一档命中即停——这与内核 `generic_permission` 的短路次序一致，
//! 不是"取并集"。把三档并起来算是最常见的权限判据错误：一个 `040` 的
//! 文件，属于该组的用户能读，不属于该组的用户不能读，取并集会放行所有
//! 用户。
//!
//! 语料 14 槽：前 8 槽申报正确（目录 2、权限 3、成功 2、路径 1），后 6 槽
//! 各有且只有一处申报错误（码错/阶段错/文案错/状态矛盾），逐槽必须命中
//! 对应缺陷码。账本 14 格 8 成 6 败，冻结后重放同指纹。
//!
//! 闸门次序：模式 → 定位 → 状态 → 类别 → 权限 → 打开 → 映射 → 就绪 → 文案。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本在
//! `audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfca0;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 申报正确的槽数。
pub const ACCEPT: usize = 8;

/// 申报错误的槽数。
pub const REJECT: usize = 6;

/// 档位数。
pub const TIER_COUNT: usize = 4;

/// 文案长度上限。
pub const MAX_MSG: usize = 256;

/// 本机调用者 uid。
pub const NATIVE_UID: u32 = 1000;

// ---------------------------------------------------------------------------
// errno（Linux x86-64 取值）
// ---------------------------------------------------------------------------

/// 权限不足。
pub const ERR_EPERM: i32 = 1;
/// 不存在。
pub const ERR_ENOENT: i32 = 2;
/// 拒绝访问。
pub const ERR_EACCES: i32 = 13;
/// 路径中段非目录。
pub const ERR_ENOTDIR: i32 = 20;
/// 目标是目录。
pub const ERR_EISDIR: i32 = 21;
/// 符号链接层数过多。
pub const ERR_ELOOP: i32 = 40;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// errno 申报不符。
pub const EA_ECODE: u32 = SEG_BASE + 0x01;
/// 阶段申报不符。
pub const EA_ESTAGE: u32 = SEG_BASE + 0x02;
/// 文案/根因申报不符。
pub const EA_ETEXT: u32 = SEG_BASE + 0x03;
/// 状态矛盾（成功/失败互报）。
pub const EA_ESTATE: u32 = SEG_BASE + 0x04;
/// 计数不符。
pub const EA_ECOUNT: u32 = SEG_BASE + 0x05;
/// 观测值不符。
pub const EA_EVALUE: u32 = SEG_BASE + 0x06;
/// 槽位非法。
pub const EA_ESLOT: u32 = SEG_BASE + 0x07;
/// 指纹不符。
pub const EA_ESEAL: u32 = SEG_BASE + 0x08;
/// 无观测。
pub const EA_ENOBS: u32 = SEG_BASE + 0x09;
/// 矩阵不平。
pub const EA_EMATRIX: u32 = SEG_BASE + 0x0a;
/// 档位不符。
pub const EA_ETIER: u32 = SEG_BASE + 0x0b;
/// 阶段次序倒置。
pub const EA_EORDER: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const EA_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 模式检查。
pub const OP_MODE: u8 = 0x01;
/// 路径定位。
pub const OP_LOOKUP: u8 = 0x02;
/// 状态。
pub const OP_STAT: u8 = 0x03;
/// 类别判定。
pub const OP_CLASSIFY: u8 = 0x04;
/// 权限。
pub const OP_ACCESS: u8 = 0x05;
/// 映射。
pub const OP_MAP: u8 = 0x06;
/// 语料。
pub const OP_CORPUS: u8 = 0x07;

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

/// 折入一个字符串（逐字节）。
fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut v = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            v = fold_byte(v, *b);
        }
        i += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// 文本
// ---------------------------------------------------------------------------

/// 十进制文本。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
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
        let shift = (n as u32) * 4;
        let nib = ((v >> shift) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
        s.push(c as char);
    }
    s
}

/// 有符号十进制文本。
fn i32_text(v: i32) -> String {
    if v == 0 {
        return String::from("0");
    }
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text(v.unsigned_abs()).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// u32 文本（re-export 语义名）。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

// ---------------------------------------------------------------------------
// 权限位
// ---------------------------------------------------------------------------

/// 经典九位权限（owner/group/other 三档，每档 rwx）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perm {
    bits: u16,
}

impl Perm {
    /// 构造（只保留低 9 位）。
    pub const fn new(bits: u16) -> Perm {
        Perm { bits: bits & 0o777 }
    }

    /// 原始位。
    pub const fn bits(&self) -> u16 {
        self.bits
    }

    /// 某一档的三位。
    fn class_bits(&self, class: u8) -> u16 {
        match class {
            0 => (self.bits >> 6) & 0o7,
            1 => (self.bits >> 3) & 0o7,
            _ => self.bits & 0o7,
        }
    }

    /// owner 档三位。
    pub fn owner(&self) -> u8 {
        self.class_bits(0) as u8
    }

    /// group 档三位。
    pub fn group(&self) -> u8 {
        self.class_bits(1) as u8
    }

    /// other 档三位。
    pub fn other(&self) -> u8 {
        self.class_bits(2) as u8
    }

    /// 某档读位。
    pub fn read_bit(&self, class: u8) -> bool {
        (self.class_bits(class) & 0o4) != 0
    }

    /// 某档写位。
    pub fn write_bit(&self, class: u8) -> bool {
        (self.class_bits(class) & 0o2) != 0
    }

    /// 某档执行位。
    pub fn exec_bit(&self, class: u8) -> bool {
        (self.class_bits(class) & 0o1) != 0
    }

    /// `rwxr-x---` 形态的九字符文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut c = 0u8;
        while c < 3 {
            let b = self.class_bits(c);
            s.push(if (b & 0o4) != 0 { 'r' } else { '-' });
            s.push(if (b & 0o2) != 0 { 'w' } else { '-' });
            s.push(if (b & 0o1) != 0 { 'x' } else { '-' });
            c += 1;
        }
        s
    }
}

/// 调用者身份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Who {
    /// 有效 uid。
    pub uid: u32,
    /// 有效 gid。
    pub gid: u32,
}

impl Who {
    /// 构造。
    pub const fn new(uid: u32, gid: u32) -> Who {
        Who { uid, gid }
    }

    /// 是否 root。
    pub const fn root(&self) -> bool {
        self.uid == 0
    }
}

/// 节点类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 普通文件。
    File,
    /// 目录。
    Dir,
    /// 不存在。
    Missing,
    /// 路径中段非目录。
    NotDir,
}

impl Kind {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Dir => "dir",
            Kind::Missing => "missing",
            Kind::NotDir => "not-dir",
        }
    }
}

/// 被打开的对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// 类别。
    pub kind: Kind,
    /// 权限位。
    pub mode: Perm,
    /// 属主 uid。
    pub uid: u32,
    /// 属组 gid。
    pub gid: u32,
    /// 是否位于 noexec 挂载点。
    pub noexec: bool,
}

impl Node {
    /// 普通文件。
    pub const fn file(mode: u16, uid: u32, gid: u32) -> Node {
        Node { kind: Kind::File, mode: Perm::new(mode), uid, gid, noexec: false }
    }

    /// 目录。
    pub const fn dir(mode: u16) -> Node {
        Node { kind: Kind::Dir, mode: Perm::new(mode), uid: 0, gid: 0, noexec: false }
    }

    /// 不存在的节点。
    pub const fn missing() -> Node {
        Node { kind: Kind::Missing, mode: Perm::new(0), uid: 0, gid: 0, noexec: false }
    }

    /// 路径中段非目录。
    pub const fn notdir() -> Node {
        Node { kind: Kind::NotDir, mode: Perm::new(0), uid: 0, gid: 0, noexec: false }
    }

    /// noexec 挂载点上的文件。
    pub const fn noexec(mode: u16) -> Node {
        Node { kind: Kind::File, mode: Perm::new(mode), uid: 0, gid: 0, noexec: true }
    }

    /// 调用者落在哪一档：0 owner、1 group、2 other。
    pub fn class_of(&self, who: &Who) -> u8 {
        if who.uid == self.uid {
            0
        } else if who.gid == self.gid {
            1
        } else {
            2
        }
    }
}

/// 读位判定（root 绕过 DAC）。
pub fn readable(node: &Node, who: &Who) -> bool {
    if who.root() {
        return true;
    }
    node.mode.read_bit(node.class_of(who))
}

// ---------------------------------------------------------------------------
// 阶段
// ---------------------------------------------------------------------------

/// dlopen 九段管线中的一段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 模式位。
    Mode,
    /// 定位。
    Lookup,
    /// 状态。
    Stat,
    /// 类别。
    Classify,
    /// 权限。
    Access,
    /// 打开。
    Open,
    /// 映射。
    Map,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序编号。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Mode => 0,
            Stage::Lookup => 1,
            Stage::Stat => 2,
            Stage::Classify => 3,
            Stage::Access => 4,
            Stage::Open => 5,
            Stage::Map => 6,
            Stage::Ready => 7,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Mode => "mode",
            Stage::Lookup => "lookup",
            Stage::Stat => "stat",
            Stage::Classify => "classify",
            Stage::Access => "access",
            Stage::Open => "open",
            Stage::Map => "map",
            Stage::Ready => "ready",
        }
    }
}

// ---------------------------------------------------------------------------
// 根因
// ---------------------------------------------------------------------------

/// 失败根因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// 成功。
    None,
    /// 目标是目录。
    Dir,
    /// 文件不可读。
    FileRead,
    /// 搜索目录不可穿越。
    SearchDir,
    /// noexec 挂载。
    NoexecMount,
    /// 路径中段非目录。
    NotDir,
    /// 不存在。
    Missing,
}

impl Origin {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Origin::None => "none",
            Origin::Dir => "dir",
            Origin::FileRead => "file-read",
            Origin::SearchDir => "search-dir",
            Origin::NoexecMount => "noexec-mount",
            Origin::NotDir => "not-dir",
            Origin::Missing => "missing",
        }
    }
}

/// 判据档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 目录档。
    Dir,
    /// 权限档。
    Perm,
    /// 成功档。
    Ok,
    /// 其它路径档。
    Other,
}

impl Tier {
    /// 由根因归位。
    pub fn of(origin: Origin) -> Tier {
        match origin {
            Origin::Dir => Tier::Dir,
            Origin::FileRead => Tier::Perm,
            Origin::SearchDir => Tier::Perm,
            Origin::NoexecMount => Tier::Perm,
            Origin::None => Tier::Ok,
            Origin::NotDir => Tier::Other,
            Origin::Missing => Tier::Other,
        }
    }

    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Tier::Dir => 0,
            Tier::Perm => 1,
            Tier::Ok => 2,
            Tier::Other => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Dir => "dir",
            Tier::Perm => "perm",
            Tier::Ok => "ok",
            Tier::Other => "other",
        }
    }
}

// ---------------------------------------------------------------------------
// 缺陷
// ---------------------------------------------------------------------------

/// 申报与实际比对得到的缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// errno 错。
    Code,
    /// 阶段错。
    Stage,
    /// 文案/根因错。
    Text,
    /// 成功与失败互报。
    Estate,
    /// 计数错。
    Count,
    /// 观测值错。
    Value,
    /// 槽位错。
    Slot,
    /// 指纹错。
    Seal,
    /// 无观测。
    Nobs,
    /// 矩阵不平。
    Matrix,
    /// 档位错。
    Tier,
    /// 次序倒置。
    Order,
    /// 账本不平。
    Bal,
}

impl Fault {
    /// 缺陷码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Code => EA_ECODE,
            Fault::Stage => EA_ESTAGE,
            Fault::Text => EA_ETEXT,
            Fault::Estate => EA_ESTATE,
            Fault::Count => EA_ECOUNT,
            Fault::Value => EA_EVALUE,
            Fault::Slot => EA_ESLOT,
            Fault::Seal => EA_ESEAL,
            Fault::Nobs => EA_ENOBS,
            Fault::Matrix => EA_EMATRIX,
            Fault::Tier => EA_ETIER,
            Fault::Order => EA_EORDER,
            Fault::Bal => EA_EBAL,
        }
    }

    /// 是否非绿。
    pub fn alive(&self) -> bool {
        match self {
            Fault::Ok => false,
            _ => true,
        }
    }
}

/// 缺陷短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Code => "code",
        Fault::Stage => "stage",
        Fault::Text => "text",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Value => "value",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Tier => "tier",
        Fault::Order => "order",
        Fault::Bal => "bal",
    }
}

// ---------------------------------------------------------------------------
// 管线
// ---------------------------------------------------------------------------

/// 一次被记录的探测结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt {
    /// 请求名。
    pub name: &'static str,
    /// 死在（或走到）的阶段。
    pub stage: Stage,
    /// errno（成功为 0）。
    pub code: i32,
    /// 根因。
    pub origin: Origin,
}

impl Attempt {
    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.stage == Stage::Ready && self.code == 0
    }

    /// 档位。
    pub fn tier(&self) -> Tier {
        Tier::of(self.origin)
    }

    /// 文案。
    pub fn text(&self) -> String {
        message(self)
    }

    /// `stage/code/origin` 短形。
    pub fn short(&self) -> String {
        let mut s = String::from(self.stage.name());
        s.push('/');
        s.push_str(i32_text(self.code).as_str());
        s.push('/');
        s.push_str(self.origin.name());
        s
    }
}

/// 九段管线的确定性模拟：先出错先返回。
///
/// 次序不可调：定位先于状态（搜索目录都过不去就不该报 ENOENT），状态先于
/// 类别（不存在的路径没有类别），类别先于权限（目录是 EISDIR 不是 EACCES），
/// 权限先于映射（读不进来就谈不上 PROT_EXEC）。
pub fn probe(name: &'static str, node: &Node, who: &Who, search_exec: bool) -> Attempt {
    if !search_exec {
        return Attempt { name, stage: Stage::Lookup, code: ERR_EACCES, origin: Origin::SearchDir };
    }
    match node.kind {
        Kind::Missing => {
            return Attempt { name, stage: Stage::Stat, code: ERR_ENOENT, origin: Origin::Missing };
        }
        Kind::NotDir => {
            return Attempt { name, stage: Stage::Stat, code: ERR_ENOTDIR, origin: Origin::NotDir };
        }
        Kind::Dir => {
            return Attempt { name, stage: Stage::Classify, code: ERR_EISDIR, origin: Origin::Dir };
        }
        Kind::File => {}
    }
    if !readable(node, who) {
        return Attempt { name, stage: Stage::Access, code: ERR_EACCES, origin: Origin::FileRead };
    }
    if node.noexec {
        return Attempt { name, stage: Stage::Map, code: ERR_EACCES, origin: Origin::NoexecMount };
    }
    Attempt { name, stage: Stage::Ready, code: 0, origin: Origin::None }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// `<name>: <msg>`。
pub fn named(name: &str, msg: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    s.push_str(msg);
    s
}

/// `<name>: cannot open shared object file: <why>`。
pub fn open_text(name: &str, why: &str) -> String {
    let mut s = String::from(name);
    s.push_str(": cannot open shared object file: ");
    s.push_str(why);
    s
}

/// 按根因生成 dlerror 文案。
pub fn message_for(name: &str, origin: Origin) -> String {
    match origin {
        Origin::None => String::new(),
        Origin::Dir => named(name, "cannot read file data: Is a directory"),
        Origin::FileRead => open_text(name, "Permission denied"),
        Origin::SearchDir => open_text(name, "Permission denied"),
        Origin::NoexecMount => {
            named(name, "failed to map segment from shared object: Permission denied")
        }
        Origin::NotDir => open_text(name, "Not a directory"),
        Origin::Missing => open_text(name, "No such file or directory"),
    }
}

/// 观测的 dlerror 文案。
pub fn message(a: &Attempt) -> String {
    message_for(a.name, a.origin)
}

/// 文案是否合规：空（成功）或单行、无 NUL、长度受限、无首尾空白。
pub fn sane(msg: &str) -> bool {
    let bytes = msg.as_bytes();
    if bytes.is_empty() {
        return true;
    }
    if bytes.len() > MAX_MSG {
        return false;
    }
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b == 0 || *b == b'\n' || *b == b'\r' {
                return false;
            }
        }
        i += 1;
    }
    if let Some(first) = bytes.first() {
        if *first == b' ' {
            return false;
        }
    }
    if let Some(last) = bytes.last() {
        if *last == b' ' {
            return false;
        }
    }
    true
}

/// 单行释义：`<name> stage=<s> code=<c> tier=<t> msg=<m>`。
pub fn explain(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from(a.name);
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" code=");
    s.push_str(i32_text(a.code).as_str());
    s.push_str(" tier=");
    s.push_str(a.tier().name());
    s.push_str(" msg=");
    let m = message(&a);
    if m.is_empty() {
        s.push_str("-");
    } else {
        s.push_str(m.as_str());
    }
    s.push_str("\n");
    s
}

// ---------------------------------------------------------------------------
// 触发表
// ---------------------------------------------------------------------------

/// 普通用户。
pub const WHO_USER: Who = Who::new(1000, 1000);

/// 组 50 内的用户。
pub const WHO_GROUP: Who = Who::new(1000, 50);

/// root。
pub const WHO_ROOT: Who = Who::new(0, 0);

/// 语料槽的输入形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trigger {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 请求名。
    pub name: &'static str,
    /// 被打开节点。
    pub node: Node,
    /// 调用者。
    pub who: Who,
    /// 搜索目录是否可穿越。
    pub search_exec: bool,
}

impl Trigger {
    /// 空槽。
    pub const fn blank() -> Trigger {
        Trigger {
            tag: "",
            name: "",
            node: Node::missing(),
            who: WHO_USER,
            search_exec: false,
        }
    }

    /// 实际结果。
    pub fn actual(&self) -> Attempt {
        probe(self.name, &self.node, &self.who, self.search_exec)
    }
}

/// 14 槽触发表。前 8 槽是"可正确申报"的形状，后 6 槽复刻前 8 槽里的
/// 六个形状，用来挂错误申报。
pub const TRIGGERS: [Trigger; CORPUS] = [
    Trigger {
        tag: "dir-target",
        name: "/usr/lib/libfoo.so",
        node: Node::dir(0o755),
        who: WHO_USER,
        search_exec: true,
    },
    Trigger {
        tag: "dir-no-read",
        name: "./libdir",
        node: Node::dir(0o111),
        who: WHO_USER,
        search_exec: true,
    },
    Trigger {
        tag: "locked-file",
        name: "./locked.so",
        node: Node::file(0o000, 0, 0),
        who: WHO_USER,
        search_exec: true,
    },
    Trigger {
        tag: "search-denied",
        name: "libz.so",
        node: Node::file(0o755, 0, 0),
        who: WHO_USER,
        search_exec: false,
    },
    Trigger {
        tag: "noexec-mount",
        name: "./noexec.so",
        node: Node::noexec(0o755),
        who: WHO_USER,
        search_exec: true,
    },
    Trigger {
        tag: "group-read",
        name: "./group.so",
        node: Node::file(0o040, 0, 50),
        who: WHO_GROUP,
        search_exec: true,
    },
    Trigger {
        tag: "root-bypass",
        name: "./root.so",
        node: Node::file(0o000, 0, 0),
        who: WHO_ROOT,
        search_exec: true,
    },
    Trigger {
        tag: "not-dir",
        name: "/etc/passwd/lib.so",
        node: Node::notdir(),
        who: WHO_USER,
        search_exec: true,
    },
    Trigger {
        tag: "dir-as-noent",
        name: "/usr/lib/libfoo.so",
        node: Node::dir(0o755),
        who: WHO_USER,
        search_exec: true,
    },
    Trigger {
        tag: "search-as-file",
        name: "libz.so",
        node: Node::file(0o755, 0, 0),
        who: WHO_USER,
        search_exec: false,
    },
    Trigger {
        tag: "noexec-as-open",
        name: "./noexec.so",
        node: Node::noexec(0o755),
        who: WHO_USER,
        search_exec: true,
    },
    Trigger {
        tag: "notdir-as-enoent",
        name: "/etc/passwd/lib.so",
        node: Node::notdir(),
        who: WHO_USER,
        search_exec: true,
    },
    Trigger {
        tag: "locked-as-ready",
        name: "./locked.so",
        node: Node::file(0o000, 0, 0),
        who: WHO_USER,
        search_exec: true,
    },
    Trigger {
        tag: "dirno-read-as-file",
        name: "./libdir",
        node: Node::dir(0o111),
        who: WHO_USER,
        search_exec: true,
    },
];

/// 与 `TRIGGERS` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Classify, code: ERR_EISDIR, origin: Origin::Dir },
    Claim { stage: Stage::Classify, code: ERR_EISDIR, origin: Origin::Dir },
    Claim { stage: Stage::Access, code: ERR_EACCES, origin: Origin::FileRead },
    Claim { stage: Stage::Lookup, code: ERR_EACCES, origin: Origin::SearchDir },
    Claim { stage: Stage::Map, code: ERR_EACCES, origin: Origin::NoexecMount },
    Claim { stage: Stage::Ready, code: 0, origin: Origin::None },
    Claim { stage: Stage::Ready, code: 0, origin: Origin::None },
    Claim { stage: Stage::Stat, code: ERR_ENOTDIR, origin: Origin::NotDir },
    Claim { stage: Stage::Classify, code: ERR_ENOENT, origin: Origin::Missing },
    Claim { stage: Stage::Open, code: ERR_EACCES, origin: Origin::FileRead },
    Claim { stage: Stage::Map, code: ERR_EACCES, origin: Origin::FileRead },
    Claim { stage: Stage::Stat, code: ERR_ENOENT, origin: Origin::NotDir },
    Claim { stage: Stage::Ready, code: 0, origin: Origin::None },
    Claim { stage: Stage::Classify, code: ERR_EACCES, origin: Origin::FileRead },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Code, Fault::Stage, Fault::Text, Fault::Code, Fault::Estate, Fault::Code];

/// 后 6 槽对应的缺陷码（供账本对账）。
pub const REJECT_CODES: [u32; REJECT] =
    [EA_ECODE, EA_ESTAGE, EA_ETEXT, EA_ECODE, EA_ESTATE, EA_ECODE];

// ---------------------------------------------------------------------------
// 申报与对判
// ---------------------------------------------------------------------------

/// 一条申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报阶段。
    pub stage: Stage,
    /// 申报 errno。
    pub code: i32,
    /// 申报根因。
    pub origin: Origin,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Mode, code: 0, origin: Origin::None }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.stage == Stage::Ready && self.code == 0
    }
}

/// 一个语料槽：触发 + 申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 触发形状。
    pub trigger: Trigger,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 空白槽。
    pub const fn blank() -> Case {
        Case { trigger: Trigger::blank(), claim: Claim::blank() }
    }

    /// 槽标签。
    pub fn tag(&self) -> &'static str {
        self.trigger.tag
    }

    /// 请求名。
    pub fn name(&self) -> &'static str {
        self.trigger.name
    }

    /// 实际结果。
    pub fn actual(&self) -> Attempt {
        self.trigger.actual()
    }

    /// 对判。
    pub fn judge(&self) -> Fault {
        judge(self)
    }

    /// 是否一致。
    pub fn ok(&self) -> bool {
        judge(self) == Fault::Ok
    }
}

/// 对判：状态 → 阶段 → errno → 根因。
///
/// "状态"排在最前：申报成功而实际失败（或反过来）不是"数值差一点"，而是
/// 语义层面的矛盾，单独给 `Fault::Estate`，不与"码错"混用。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    let claim = c.claim;
    if claim.success() != a.ok() {
        return Fault::Estate;
    }
    if claim.stage != a.stage {
        return Fault::Stage;
    }
    if claim.code != a.code {
        return Fault::Code;
    }
    if claim.origin != a.origin {
        return Fault::Text;
    }
    Fault::Ok
}

/// 语料表。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(t), Some(c)) = (TRIGGERS.get(i), CLAIMS.get(i)) {
            out[i] = Case { trigger: *t, claim: *c };
        }
        i += 1;
    }
    out
}

/// 取第 i 槽。
pub fn case_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    cases().get(i).copied()
}

/// 覆盖校验：只有恰好 14 槽才算覆盖完成。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(EA_ENOBS)
    }
}

/// 第 i 槽的设计期望：前 8 槽一致，后 6 槽命中各自缺陷。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        Some(Fault::Ok)
    } else {
        REJECT_FAULTS.get(i - ACCEPT).copied()
    }
}

// ---------------------------------------------------------------------------
// 矩阵
// ---------------------------------------------------------------------------

/// 一行统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档槽数。
    pub total: usize,
    /// 该档申报正确数。
    pub good: usize,
}

impl Row {
    /// 全对。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }

    /// 空白行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }
}

/// 四档矩阵。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（dir/perm/ok/other）。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 空矩阵。
    pub const fn blank() -> Matrix {
        Matrix { rows: [Row::blank(); TIER_COUNT] }
    }

    /// 由语料统计。
    pub fn of() -> Matrix {
        let mut m = Matrix::blank();
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                let t = c.actual().tier().index();
                if t < TIER_COUNT {
                    let mut row = match m.rows.get(t) {
                        Some(r) => *r,
                        None => Row::blank(),
                    };
                    row.total += 1;
                    if judge(c) == Fault::Ok {
                        row.good += 1;
                    }
                    m.rows[t] = row;
                }
            }
            i += 1;
        }
        m
    }

    /// 总槽数。
    pub fn total(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.total;
            }
            i += 1;
        }
        n
    }

    /// 总正确数。
    pub fn good(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.good;
            }
            i += 1;
        }
        n
    }

    /// 是否平：14 槽、8 对。
    pub fn balanced(&self) -> bool {
        self.total() == CORPUS && self.good() == ACCEPT
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<Row> {
        self.rows.get(i).copied()
    }

    /// 全对行的数量。
    pub fn perfect_rows(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.perfect() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                h = fold_word(h, r.total as u64);
                h = fold_word(h, r.good as u64);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确数。
    pub accept: u32,
    /// 申报错误数。
    pub reject: u32,
    /// 目录档正确数。
    pub dir: u32,
    /// 权限档正确数。
    pub perm: u32,
    /// 成功档正确数。
    pub ok: u32,
    /// 路径档正确数。
    pub other: u32,
    /// 对判一致数。
    pub matched: u32,
}

impl Report {
    /// 万分位一致的千分表示（四舍五入）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched * 1000 + self.total / 2) / self.total
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(EA_ECOUNT)
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("eacces corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" dir=");
        s.push_str(u32_text(self.dir).as_str());
        s.push_str(" perm=");
        s.push_str(u32_text(self.perm).as_str());
        s.push_str(" ok=");
        s.push_str(u32_text(self.ok).as_str());
        s.push_str(" other=");
        s.push_str(u32_text(self.other).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 可注入一槽缺陷的全跑；注入槽越界给 `EA_ESLOT`。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(EA_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut dir = 0u32;
    let mut perm = 0u32;
    let mut ok = 0u32;
    let mut other = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(EA_ENOBS),
        };
        let a = c.actual();
        let mut f = judge(&c);
        if let Some((j, forced)) = over {
            if j == i {
                f = forced;
            }
        }
        let want = match expected(i) {
            Some(v) => v,
            None => return Err(EA_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match a.tier() {
                Tier::Dir => dir += 1,
                Tier::Perm => perm += 1,
                Tier::Ok => ok += 1,
                Tier::Other => other += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, dir, perm, ok, other, matched })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门：阶段只许前进，关闭后不许再开。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    last: i32,
    steps: u32,
    closed: bool,
}

impl Gate {
    /// 新闸门。
    pub const fn new() -> Gate {
        Gate { last: -1, steps: 0, closed: false }
    }

    /// 走一段。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            return Err(EA_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(EA_EORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭；返回走过的段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(EA_ESTATE);
        }
        self.closed = true;
        Ok(self.steps)
    }

    /// 走过的段数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 上一段编号。
    pub const fn last(&self) -> i32 {
        self.last
    }

    /// 是否已关。
    pub const fn closed(&self) -> bool {
        self.closed
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
    pub fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EA_ESLOT);
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
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(EA_EBAL);
        }
        const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 3, 2, 1, 929];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EA_EBAL),
            };
            if s.slot as usize != i {
                return Err(EA_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EA_EVALUE);
                        }
                    }
                    None => return Err(EA_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EA_EVALUE);
                        }
                    }
                    None => return Err(EA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(EA_EBAL);
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
            return Err(EA_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：用同一构造重建，指纹须一致。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(EA_ESEAL)
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

/// `Result<u32, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_CLASSIFY, r.map(|x| x.dir)));
    let _ = l.register(count_cell(4, OP_ACCESS, r.map(|x| x.perm)));
    let _ = l.register(count_cell(5, OP_MAP, r.map(|x| x.ok)));
    let _ = l.register(count_cell(6, OP_STAT, r.map(|x| x.other)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_LOOKUP, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_ACCESS,
                ok: false,
                code: f.code(),
                value: 0,
            });
        }
        i += 1;
    }
    l
}

// ---------------------------------------------------------------------------
// 注入
// ---------------------------------------------------------------------------

/// 四路注入：表、判、渲染、账本。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && perm_ok() && pipe_ok();
    out[1] = judge_ok() && matrix_ok() && corpus_ok();
    out[2] = reds_ok() && message_ok() && slots_ok() && explain_ok();
    out[3] = gate_ok()
        && ledger_ok()
        && {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            a && b
        };
    out
}

// ---------------------------------------------------------------------------
// 判据基元
// ---------------------------------------------------------------------------

/// 名称与段基址。
fn name_ok() -> bool {
    let codes = [
        EA_ECODE, EA_ESTAGE, EA_ETEXT, EA_ESTATE, EA_ECOUNT, EA_EVALUE, EA_ESLOT, EA_ESEAL,
        EA_ENOBS, EA_EMATRIX, EA_ETIER, EA_EORDER, EA_EBAL,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let a = SEG_BASE == 0xfca0;
    let b = EA_ECODE == SEG_BASE + 0x01 && EA_EBAL == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = NATIVE_UID == 1000 && MAX_MSG == 256 && TIER_COUNT == 4;
    let e = ERR_EACCES == 13 && ERR_EISDIR == 21 && ERR_ENOTDIR == 20 && ERR_ELOOP == 40;
    a && b && c && d && e
}

/// 权限位与档位判定。
fn perm_ok() -> bool {
    let p = Perm::new(0o750);
    let a = p.owner() == 7 && p.group() == 5 && p.other() == 0 && p.bits() == 0o750;
    let b = p.text().as_str() == "rwxr-x---";
    let c = p.read_bit(0) && p.write_bit(0) && p.exec_bit(0);
    let d = p.read_bit(1) && !p.write_bit(1) && p.exec_bit(1);
    let e = !p.read_bit(2) && !p.write_bit(2) && !p.exec_bit(2);
    let n = Node::file(0o640, 7, 0);
    let owner = Who::new(7, 9);
    let group = Who::new(8, 0);
    let other = Who::new(8, 8);
    let root = Who::new(0, 9);
    let f = n.class_of(&owner) == 0 && n.class_of(&group) == 1 && n.class_of(&other) == 2;
    let g = readable(&n, &owner) && readable(&n, &group);
    let h = !readable(&n, &other) && readable(&n, &root);
    let dir = Node::dir(0o111);
    let i = dir.kind == Kind::Dir && dir.mode.bits() == 0o111 && !dir.mode.read_bit(2);
    let j = !readable(&Node::file(0o002, 0, 0), &Who::new(5, 5));
    let k = Perm::new(0o7777).bits() == 0o777;
    a && b && c && d && e && f && g && h && i && j && k
}

/// 九段管线：路径、状态、类别、权限、映射的次序。
fn pipe_ok() -> bool {
    let cs = cases();
    const BASE: [usize; REJECT] = [0, 3, 4, 7, 2, 1];
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if i < ACCEPT {
            if a.stage != c.claim.stage || a.code != c.claim.code || a.origin != c.claim.origin {
                return false;
            }
        } else {
            let b = match BASE.get(i - ACCEPT) {
                Some(v) => *v,
                None => return false,
            };
            let base = match cs.get(b) {
                Some(v) => *v,
                None => return false,
            };
            let x = base.actual();
            if a.stage != x.stage || a.code != x.code || a.origin != x.origin {
                return false;
            }
        }
        i += 1;
    }
    let ok = probe("./x.so", &Node::file(0o644, 0, 0), &WHO_USER, true);
    ok.stage == Stage::Ready && ok.code == 0 && ok.origin == Origin::None
}

/// 申报对判。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        if let Some(c) = cs.get(i) {
            if judge(c) != Fault::Ok {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        let c = match cs.get(j + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let want = match REJECT_FAULTS.get(j) {
            Some(v) => *v,
            None => return false,
        };
        let code = match REJECT_CODES.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if judge(&c) != want {
            return false;
        }
        if judge(&c).code() != code {
            return false;
        }
        j += 1;
    }
    true
}

/// 矩阵统计。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    m.row(0) == Some(Row { total: 4, good: 2 })
        && m.row(1) == Some(Row { total: 6, good: 3 })
        && m.row(2) == Some(Row { total: 2, good: 2 })
        && m.row(3) == Some(Row { total: 2, good: 1 })
        && m.row(4).is_none()
        && m.total() == CORPUS
        && m.good() == ACCEPT
        && m.balanced()
        && m.perfect_rows() == 1
        && m.seal() == Matrix::of().seal()
        && m.seal() != Matrix::blank().seal()
}

/// 标签唯一。
fn tags_unique() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.tag().is_empty() {
            return false;
        }
        let mut j = 0usize;
        while j < i {
            if let Some(p) = cs.get(j) {
                if p.tag() == c.tag() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 逐槽释义与文案合规。
fn notes_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let m = message(&a);
        if !sane(m.as_str()) {
            return false;
        }
        if a.ok() != m.is_empty() {
            return false;
        }
        if a.tier() != Tier::of(a.origin) {
            return false;
        }
        if explain(&c).as_str().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 逐槽对判表。
fn table_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let f = judge(&c);
        if i < ACCEPT {
            if f != Fault::Ok {
                return false;
            }
        } else if f == Fault::Ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 汇总与渲染。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.dir == 2 && r.perm == 3 && r.ok == 2 && r.other == 1;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = tags_unique() && notes_ok() && table_ok();
    a && b && c && d
}

/// 红档：一槽注入即掉分，越界注入报槽错。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929;
    let b = r.gate() == Err(EA_ECOUNT) && !r.green();
    let c = run_with(Some((CORPUS, Fault::Code))).err() == Some(EA_ESLOT);
    a && b && c
}

/// 文案逐字。
fn message_ok() -> bool {
    let a = message_for("/usr/lib/libfoo.so", Origin::Dir).as_str()
        == "/usr/lib/libfoo.so: cannot read file data: Is a directory";
    let b = message_for("./libdir", Origin::Dir).as_str()
        == "./libdir: cannot read file data: Is a directory";
    let c = message_for("./locked.so", Origin::FileRead).as_str()
        == "./locked.so: cannot open shared object file: Permission denied";
    let d = message_for("libz.so", Origin::SearchDir).as_str()
        == "libz.so: cannot open shared object file: Permission denied";
    let e = message_for("./noexec.so", Origin::NoexecMount).as_str()
        == "./noexec.so: failed to map segment from shared object: Permission denied";
    let f = message_for("/etc/passwd/lib.so", Origin::NotDir).as_str()
        == "/etc/passwd/lib.so: cannot open shared object file: Not a directory";
    let g = message_for("/none.so", Origin::Missing).as_str()
        == "/none.so: cannot open shared object file: No such file or directory";
    let h = message_for("./group.so", Origin::None).is_empty();
    let i = sane("ok: x") && !sane("bad\n") && !sane(" bad") && !sane("bad ");
    let j = sane("") && !sane("a\u{0}b");
    a && b && c && d && e && f && g && h && i && j
}

/// 槽号、档位、短名。
fn slots_ok() -> bool {
    let a = match case_at(0) {
        Some(c) => c.tag() == "dir-target" && c.name() == "/usr/lib/libfoo.so",
        None => false,
    };
    let b = match case_at(13) {
        Some(c) => c.tag() == "dirno-read-as-file",
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(EA_ENOBS);
    let e = Stage::Lookup.order() == 1
        && Stage::Access.order() == 4
        && Stage::Ready.order() == 7
        && Stage::Classify.name() == "classify"
        && Stage::Map.name() == "map";
    let f = Tier::Dir.index() == 0
        && Tier::Perm.index() == 1
        && Tier::Ok.index() == 2
        && Tier::Other.index() == 3
        && Tier::Perm.name() == "perm"
        && Tier::Ok.name() == "ok";
    let g = fault_short(Fault::Stage) == "stage"
        && fault_short(Fault::Ok) == "ok"
        && fault_short(Fault::Estate) == "estate";
    let h = Kind::File.name() == "file"
        && Kind::Dir.name() == "dir"
        && Kind::Missing.name() == "missing"
        && Kind::NotDir.name() == "not-dir";
    let i = Origin::FileRead.name() == "file-read"
        && Origin::SearchDir.name() == "search-dir"
        && Origin::NoexecMount.name() == "noexec-mount";
    a && b && c && d && e && f && g && h && i
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Classify) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Stat) == Err(EA_EORDER) && g.steps() == 1;
    let d = g.step(Stage::Access) == Ok(()) && g.last() == 4 && g.steps() == 2;
    let e = g.close() == Ok(2) && g.closed();
    let f = g.close() == Err(EA_ESTATE);
    let h = g.step(Stage::Ready) == Err(EA_ESTATE);
    a && b && c && d && e && f && h
}

/// 释义行逐字。
const EXPLAIN_LOCKED: &str = "./locked.so stage=access code=13 tier=perm msg=./locked.so: cannot open shared object file: Permission denied\n";
/// 目录槽释义。
const EXPLAIN_DIR: &str = "/usr/lib/libfoo.so stage=classify code=21 tier=dir msg=/usr/lib/libfoo.so: cannot read file data: Is a directory\n";
/// 成功槽释义。
const EXPLAIN_OK: &str = "./group.so stage=ready code=0 tier=ok msg=-\n";

/// 释义。
fn explain_ok() -> bool {
    let cs = cases();
    let a = match cs.get(2) {
        Some(c) => explain(c).as_str() == EXPLAIN_LOCKED,
        None => false,
    };
    let b = match cs.get(0) {
        Some(c) => explain(c).as_str() == EXPLAIN_DIR,
        None => false,
    };
    let d = match cs.get(5) {
        Some(c) => explain(c).as_str() == EXPLAIN_OK,
        None => false,
    };
    let e = match cs.get(4) {
        Some(c) => c.actual().short().as_str() == "map/13/noexec-mount",
        None => false,
    };
    a && b && d && e
}

/// 账本形状。
fn ledger_ok() -> bool {
    let l = build_ledger();
    l.len() == SAMPLES
        && l.ok_count() == ACCEPT
        && l.err_count() == REJECT
        && l.audit() == Ok(())
        && l.sample(100).is_none()
        && !l.is_empty()
}

// ---------------------------------------------------------------------------
// 判据集
// ---------------------------------------------------------------------------

/// 13 条判据。
pub fn run_unx11633_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11633_dlopen_eacces");

    s.add("EA01-名称与段基址", name_ok(), "static");

    s.add("EA02-权限位与档位", perm_ok(), "static");

    s.add("EA03-九段管线", pipe_ok(), "static");

    s.add("EA04-申报对判", judge_ok(), "static");

    s.add("EA05-矩阵统计", matrix_ok(), "static");

    s.add("EA06-语料与渲染", corpus_ok(), "static");

    s.add("EA07-红档显式", reds_ok(), "static");

    s.add("EA08-文案逐字", message_ok(), "static");

    s.add("EA09-槽号与档位", slots_ok(), "static");

    s.add("EA10-闸门次序", gate_ok(), "static");

    s.add("EA11-释义行", explain_ok(), "static");

    s.add("EA12-账本对账", ledger_ok(), "static");

    s.add(
        "EA13-冻结重放四注入",
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
        panic!("unx11633 test fail")
    }

    #[test]
    fn perm_bits() {
        let p = Perm::new(0o755);
        assert_eq!(p.bits(), 0o755);
        assert_eq!(p.owner(), 7);
        assert_eq!(p.group(), 5);
        assert_eq!(p.other(), 5);
        assert_eq!(Perm::new(0o7777).bits(), 0o777);
    }

    #[test]
    fn perm_flags() {
        let p = Perm::new(0o640);
        assert!(p.read_bit(0));
        assert!(p.write_bit(0));
        assert!(!p.exec_bit(0));
        assert!(p.read_bit(1));
        assert!(!p.read_bit(2));
    }

    #[test]
    fn perm_text() {
        assert_eq!(Perm::new(0o750).text().as_str(), "rwxr-x---");
        assert_eq!(Perm::new(0o644).text().as_str(), "rw-r--r--");
        assert_eq!(Perm::new(0o000).text().as_str(), "---------");
        assert_eq!(Perm::new(0o777).text().as_str(), "rwxrwxrwx");
    }

    #[test]
    fn perm_classes() {
        let n = Node::file(0o640, 7, 0);
        assert_eq!(n.class_of(&Who::new(7, 99)), 0);
        assert_eq!(n.class_of(&Who::new(8, 0)), 1);
        assert_eq!(n.class_of(&Who::new(8, 8)), 2);
    }

    #[test]
    fn perm_readable_owner() {
        let n = Node::file(0o400, 7, 0);
        assert!(readable(&n, &Who::new(7, 0)));
        assert!(!readable(&n, &Who::new(8, 0)));
    }

    #[test]
    fn perm_readable_group() {
        let n = Node::file(0o040, 7, 50);
        assert!(readable(&n, &Who::new(8, 50)));
        assert!(!readable(&n, &Who::new(8, 51)));
    }

    #[test]
    fn perm_readable_other() {
        let n = Node::file(0o004, 7, 50);
        assert!(readable(&n, &Who::new(8, 51)));
        assert!(!readable(&n, &Who::new(8, 50)));
    }

    #[test]
    fn perm_readable_root() {
        let n = Node::file(0o000, 7, 50);
        assert!(readable(&n, &Who::new(0, 0)));
        assert!(!readable(&n, &Who::new(1000, 1000)));
    }

    #[test]
    fn node_constructors() {
        let f = Node::file(0o644, 1, 2);
        assert_eq!(f.kind, Kind::File);
        assert_eq!(f.uid, 1);
        assert_eq!(f.gid, 2);
        assert!(!f.noexec);
        assert_eq!(Node::dir(0o755).kind, Kind::Dir);
        assert_eq!(Node::missing().kind, Kind::Missing);
        assert_eq!(Node::notdir().kind, Kind::NotDir);
        assert!(Node::noexec(0o755).noexec);
    }

    #[test]
    fn stage_orders() {
        assert_eq!(Stage::Mode.order(), 0);
        assert_eq!(Stage::Lookup.order(), 1);
        assert_eq!(Stage::Stat.order(), 2);
        assert_eq!(Stage::Classify.order(), 3);
        assert_eq!(Stage::Access.order(), 4);
        assert_eq!(Stage::Open.order(), 5);
        assert_eq!(Stage::Map.order(), 6);
        assert_eq!(Stage::Ready.order(), 7);
    }

    #[test]
    fn stage_names_unique() {
        let names = [
            Stage::Mode.name(),
            Stage::Lookup.name(),
            Stage::Stat.name(),
            Stage::Classify.name(),
            Stage::Access.name(),
            Stage::Open.name(),
            Stage::Map.name(),
            Stage::Ready.name(),
        ];
        let mut i = 0usize;
        while i < names.len() {
            let mut j = i + 1;
            while j < names.len() {
                assert_ne!(names[i], names[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn tier_mapping() {
        assert_eq!(Tier::of(Origin::Dir), Tier::Dir);
        assert_eq!(Tier::of(Origin::FileRead), Tier::Perm);
        assert_eq!(Tier::of(Origin::SearchDir), Tier::Perm);
        assert_eq!(Tier::of(Origin::NoexecMount), Tier::Perm);
        assert_eq!(Tier::of(Origin::None), Tier::Ok);
        assert_eq!(Tier::of(Origin::NotDir), Tier::Other);
        assert_eq!(Tier::of(Origin::Missing), Tier::Other);
    }

    #[test]
    fn pipeline_dir_first() {
        let a = probe("/usr/lib/libfoo.so", &Node::dir(0o111), &WHO_USER, true);
        assert_eq!(a.stage, Stage::Classify);
        assert_eq!(a.code, ERR_EISDIR);
        assert_eq!(a.origin, Origin::Dir);
    }

    #[test]
    fn pipeline_search_denied() {
        let a = probe("libz.so", &Node::file(0o755, 0, 0), &WHO_USER, false);
        assert_eq!(a.stage, Stage::Lookup);
        assert_eq!(a.code, ERR_EACCES);
        assert_eq!(a.origin, Origin::SearchDir);
    }

    #[test]
    fn pipeline_noexec() {
        let a = probe("./noexec.so", &Node::noexec(0o755), &WHO_USER, true);
        assert_eq!(a.stage, Stage::Map);
        assert_eq!(a.code, ERR_EACCES);
        assert_eq!(a.origin, Origin::NoexecMount);
    }

    #[test]
    fn pipeline_path_faults() {
        let m = probe("./gone.so", &Node::missing(), &WHO_USER, true);
        assert_eq!(m.stage, Stage::Stat);
        assert_eq!(m.code, ERR_ENOENT);
        assert_eq!(m.origin, Origin::Missing);
        let n = probe("/etc/passwd/lib.so", &Node::notdir(), &WHO_USER, true);
        assert_eq!(n.stage, Stage::Stat);
        assert_eq!(n.code, ERR_ENOTDIR);
        assert_eq!(n.origin, Origin::NotDir);
    }

    #[test]
    fn pipeline_success_paths() {
        let g = probe("./group.so", &Node::file(0o040, 0, 50), &WHO_GROUP, true);
        assert!(g.ok());
        assert_eq!(g.stage, Stage::Ready);
        assert_eq!(g.origin, Origin::None);
        let r = probe("./root.so", &Node::file(0o000, 0, 0), &WHO_ROOT, true);
        assert!(r.ok());
        assert_eq!(r.code, 0);
    }

    #[test]
    fn pipeline_locked() {
        let a = probe("./locked.so", &Node::file(0o000, 0, 0), &WHO_USER, true);
        assert_eq!(a.stage, Stage::Access);
        assert_eq!(a.code, ERR_EACCES);
        assert_eq!(a.origin, Origin::FileRead);
        assert!(!a.ok());
    }

    #[test]
    fn trigger_table_shape() {
        assert_eq!(TRIGGERS.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert!(tags_unique());
        assert_eq!(TRIGGERS[0].tag, "dir-target");
        assert_eq!(TRIGGERS[13].tag, "dirno-read-as-file");
    }

    #[test]
    fn cases_shape() {
        let cs = cases();
        assert_eq!(cs.len(), CORPUS);
        assert_eq!(cs[0].tag(), "dir-target");
        assert_eq!(cs[7].actual().stage, Stage::Stat);
        assert_eq!(cs[13].tag(), "dirno-read-as-file");
    }

    #[test]
    fn judge_accepts() {
        let cs = cases();
        let mut i = 0usize;
        while i < ACCEPT {
            assert_eq!(judge(&cs[i]), Fault::Ok);
            i += 1;
        }
    }

    #[test]
    fn judge_rejects_codes() {
        let cs = cases();
        let mut i = 0usize;
        while i < REJECT {
            let c = &cs[i + ACCEPT];
            assert_eq!(judge(c), REJECT_FAULTS[i]);
            assert_eq!(judge(c).code(), REJECT_CODES[i]);
            i += 1;
        }
    }

    #[test]
    fn judge_estate_slot() {
        let c = &cases()[12];
        assert_eq!(judge(c), Fault::Estate);
        assert!(judge(c).alive());
    }

    #[test]
    fn judge_text_slot() {
        let c = &cases()[10];
        let a = c.actual();
        assert_eq!(a.stage, Stage::Map);
        assert_eq!(a.code, ERR_EACCES);
        assert_eq!(a.origin, Origin::NoexecMount);
        assert_eq!(judge(c), Fault::Text);
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert!(m.balanced());
        assert_eq!(m.row(0), Some(Row { total: 4, good: 2 }));
        assert_eq!(m.row(1), Some(Row { total: 6, good: 3 }));
        assert_eq!(m.row(2), Some(Row { total: 2, good: 2 }));
        assert_eq!(m.row(3), Some(Row { total: 2, good: 1 }));
        assert_eq!(m.perfect_rows(), 1);
    }

    #[test]
    fn matrix_seal() {
        assert_eq!(Matrix::of().seal(), Matrix::of().seal());
        assert_ne!(Matrix::of().seal(), Matrix::blank().seal());
        assert_eq!(Matrix::blank().total(), 0);
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
        assert_eq!(r.dir, 2);
        assert_eq!(r.perm, 3);
        assert_eq!(r.ok, 2);
        assert_eq!(r.other, 1);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "eacces corpus=14 accept=8 reject=6 dir=2 perm=3 ok=2 other=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(EA_ECOUNT));
        assert!(!r.green());
        assert_eq!(run_with(Some((14, Fault::Code))).err(), Some(EA_ESLOT));
    }

    #[test]
    fn message_exact() {
        assert_eq!(
            message_for("./locked.so", Origin::FileRead).as_str(),
            "./locked.so: cannot open shared object file: Permission denied"
        );
        assert_eq!(
            message_for("./noexec.so", Origin::NoexecMount).as_str(),
            "./noexec.so: failed to map segment from shared object: Permission denied"
        );
        assert_eq!(
            message_for("./libdir", Origin::Dir).as_str(),
            "./libdir: cannot read file data: Is a directory"
        );
        assert!(message_for("./group.so", Origin::None).is_empty());
    }

    #[test]
    fn message_sane() {
        assert!(sane(""));
        assert!(sane("./a.so: x"));
        assert!(!sane("x\n"));
        assert!(!sane(" x"));
        assert!(!sane("x "));
        assert!(!sane("\u{0}"));
    }

    #[test]
    fn explain_exact() {
        let cs = cases();
        assert_eq!(explain(&cs[2]).as_str(), EXPLAIN_LOCKED);
        assert_eq!(explain(&cs[0]).as_str(), EXPLAIN_DIR);
        assert_eq!(explain(&cs[5]).as_str(), EXPLAIN_OK);
        assert_eq!(cs[4].actual().short().as_str(), "map/13/noexec-mount");
        assert_eq!(cs[0].actual().short().as_str(), "classify/21/dir");
    }

    #[test]
    fn gate_steps() {
        let mut g = Gate::new();
        assert_eq!(g.steps(), 0);
        assert_eq!(g.step(Stage::Classify), Ok(()));
        assert_eq!(g.step(Stage::Stat), Err(EA_EORDER));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.step(Stage::Access), Ok(()));
        assert_eq!(g.last(), 4);
        assert_eq!(g.steps(), 2);
    }

    #[test]
    fn gate_close() {
        let mut g = Gate::new();
        assert!(g.close().is_ok());
        assert!(g.closed());
        assert_eq!(g.close(), Err(EA_ESTATE));
        assert_eq!(g.step(Stage::Ready), Err(EA_ESTATE));
    }

    #[test]
    fn slot_bounds() {
        assert!(case_at(0).is_some());
        assert!(case_at(CORPUS - 1).is_some());
        assert!(case_at(CORPUS).is_none());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(1), Err(EA_ENOBS));
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(fault_short(Fault::Matrix), "matrix");
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
        assert_eq!(l.register(Sample::blank()).err(), Some(EA_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(EA_EBAL));
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
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0xfca0, 4).as_str(), "fca0");
        assert_eq!(i32_text(-22).as_str(), "-22");
        assert_eq!(i32_text(13).as_str(), "13");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11633_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
