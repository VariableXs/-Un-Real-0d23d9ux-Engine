//! UNX-F11657 .symver 顶层版本定义判据
//!
//! `.symver` 是汇编层唯一能直接写版本定义的指令：`foo@V1` 给符号挂一个
//! **非默认**版本，`foo@@V1` 给符号挂**默认**版本。默认版本只有一个，且是
//! 无版本引用唯一能看见的落点；非默认版本必须由带版本索引的引用精确命中。
//! 版本脚本 `V1 { global: foo; };` 则是在链接层给符号分配版本节点，效果与
//! `@@` 相近，但归节点所有，不归符号自身。
//!
//! 本卡校验的是"定义侧"：一条 `.symver` 或脚本分配，能否落成一个合法、可
//! 被装载器看见的版本定义。校验纵线：
//!
//! 1. `symver`：指令语法——符号名、版本名、`@`/`@@` 写法。
//! 2. `node`：版本节点必须存在于 `.gnu.version_d` 链中。
//! 3. `index`：versym 索引 ≥ 2（基节点 1 除外），且不得与已有节点重号。
//! 4. `aux`：Verdef 的 `vd_cnt`/`vd_aux` 与 Verdaux 链一致，首 aux 名等于
//!    版本名。
//! 5. `hash`：`vd_hash` 必须等于版本名的 ELF hash。
//! 6. `default`：`@@` 只能落在非基节点，且同一符号不得有两个默认版本。
//! 7. `ready`：返回索引与可见性。
//!
//! Verdef 结构（64 位，20 字节）：
//!
//! ```text
//! vd_version u16 | vd_flags u16 | vd_ndx u16 | vd_cnt u16
//! vd_hash    u32 | vd_aux   u32 | vd_next u32
//! ```
//!
//! Verdaux（8 字节）：`vda_name u32 | vda_next u32`。`vd_aux` 是从 Verdef
//! 起点到首个 Verdaux 的偏移；`vd_cnt` 是 aux 个数；`vd_next` 是到下一条
//! Verdef 的偏移，0 表示链尾。任何一个偏移越界、计数与链不符，装载器都可
//! 以拒绝整张版本表。
//!
//! 基节点（`VER_FLG_BASE`，索引 1）的 aux 名是对象文件名本身，它不是符号
//! 版本，不能作为 `@@` 的落点。把它当默认版本会让无版本引用绑到"文件版本"
//! 上，属于定义侧最隐蔽的一类错误。
//!
//! 判据六维：阶段、errno、versym 索引、注解、默认可见性、申报状态。
//! 少"索引"一维，重号与错号不可见；少"默认可见性"一维，`@` 与 `@@` 的
//! 区别消失，而这正是 `.symver` 的全部语义。
//!
//! 判序：状态 → 阶段 → errno → 索引 → 注解 → 默认可见性。
//!
//! 语料 14 槽：8 槽申报正确（可见 3、隐藏 1、结构坏 2、冲突 2），6 槽各挂
//! 一处错误申报（状态/阶段/码/索引/注解/默认）。账本 14 格 8 成 6 败，
//! 冻结后重放同指纹。
//!
//! 闸门次序：指令 → 节点 → 索引 → aux 链 → hash → 默认规则 → 就绪。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11657）。
pub const SEG_BASE: u32 = 0x6cea0;

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

/// 基节点标志（`VER_FLG_BASE`）。
pub const VER_FLG_BASE: u16 = 0x0001;

/// 弱版本标志（`VER_FLG_WEAK`）。
pub const VER_FLG_WEAK: u16 = 0x0002;

/// 基节点索引。
pub const VER_NDX_BASE: u16 = 1;

/// versym 隐藏位。
pub const VER_NDX_HIDDEN: u16 = 0x8000;

/// Versym 索引上限（去掉隐藏位）。
pub const VER_NDX_MAX: u16 = 0x7fff;

/// Verdef 记录长度（64 位）。
pub const VERDEF_SIZE: u16 = 20;

/// Verdaux 记录长度（64 位）。
pub const VERDAUX_SIZE: u16 = 8;

/// 单节点 aux 上限（模型）。
pub const VERDEF_AUX_MAX: u8 = 4;

// ---------------------------------------------------------------------------
// errno
// ---------------------------------------------------------------------------

/// 版本节点不存在。
pub const ERR_ENOENT: i32 = 2;
/// 索引重号或默认版本重复。
pub const ERR_EEXIST: i32 = 17;
/// 结构非法。
pub const ERR_EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const SV_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 申报不符。
pub const SV_ECODE: u32 = SEG_BASE + 0x02;
/// 索引申报不符。
pub const SV_EVALUE: u32 = SEG_BASE + 0x03;
/// 注解申报不符。
pub const SV_ENOTE: u32 = SEG_BASE + 0x04;
/// 默认可见性申报不符。
pub const SV_EDEFAULT: u32 = SEG_BASE + 0x05;
/// 链申报不符。
pub const SV_ECHAIN: u32 = SEG_BASE + 0x06;
/// 状态矛盾。
pub const SV_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const SV_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const SV_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const SV_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const SV_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const SV_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const SV_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 指令解析。
pub const OP_SYMVER: u8 = 0x01;
/// 节点查找。
pub const OP_NODE: u8 = 0x02;
/// 索引校验。
pub const OP_INDEX: u8 = 0x03;
/// aux 链。
pub const OP_AUX: u8 = 0x04;
/// 版本名 hash。
pub const OP_HASH: u8 = 0x05;
/// 默认规则。
pub const OP_DEFAULT: u8 = 0x06;
/// 语料。
pub const OP_CORPUS: u8 = 0x07;
/// 重放。
pub const OP_REPLAY: u8 = 0x08;

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

/// 折入一个字符串。
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

/// ELF hash（glibc `_dl_elf_hash`，用于版本名）。
pub fn elf_hash(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut h = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = (h << 4).wrapping_add(*b as u32);
            let g = h & 0xf000_0000;
            if g != 0 {
                h ^= g >> 24;
            }
            h &= !g;
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

/// u32 文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// bool 文本。
fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 版本节点不存在（GNU ld 原文）。
pub fn msg_no_node(sym: &str, ver: &str) -> String {
    let mut s = String::from("version node not found for symbol ");
    s.push_str(sym);
    s.push('@');
    s.push_str(ver);
    s
}

/// 同一符号两个默认版本。
pub fn msg_dup_default(sym: &str, v1: &str, v2: &str) -> String {
    let mut s = String::from(sym);
    s.push_str(": multiple default versions: ");
    s.push_str(v1);
    s.push_str(" and ");
    s.push_str(v2);
    s
}

/// 基节点被当成默认版本。
pub fn msg_base_default(sym: &str, ver: &str) -> String {
    let mut s = String::from("cannot make base version `");
    s.push_str(ver);
    s.push_str("' the default for symbol ");
    s.push_str(sym);
    s
}

/// hash 不符。
pub fn msg_bad_hash(ver: &str, want: u32, got: u32) -> String {
    let mut s = String::from("version definition ");
    s.push_str(ver);
    s.push_str(": hash mismatch (want 0x");
    s.push_str(hex_text(want, 8).as_str());
    s.push_str(", got 0x");
    s.push_str(hex_text(got, 8).as_str());
    s.push(')');
    s
}

/// 索引非法。
pub fn msg_bad_index(sym: &str, ndx: u16) -> String {
    let mut s = String::from("invalid version index ");
    s.push_str(dec_text(ndx as u32).as_str());
    s.push_str(" for symbol ");
    s.push_str(sym);
    s
}

/// aux 计数/名字非法。
pub fn msg_bad_aux(ver: &str) -> String {
    let mut s = String::from("version definition ");
    s.push_str(ver);
    s.push_str(": bad aux count");
    s
}

/// 链断裂。
pub fn msg_bad_chain(ver: &str) -> String {
    let mut s = String::from("version definition ");
    s.push_str(ver);
    s.push_str(": broken vd_next chain");
    s
}

/// 指令非法。
pub fn msg_bad_directive(sym: &str) -> String {
    let mut s = String::from("invalid .symver directive for symbol ");
    s.push_str(sym);
    s
}

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------

/// `.symver` 指令形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symver {
    /// 未写 `.symver`。
    None,
    /// `sym@VER`，非默认。
    At,
    /// `sym@@VER`，默认。
    AtAt,
    /// 版本脚本分配。
    Script,
}

impl Symver {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Symver::None => "",
            Symver::At => "@",
            Symver::AtAt => "@@",
            Symver::Script => "script",
        }
    }

    /// 是否声明版本。
    pub fn versioned(&self) -> bool {
        match self {
            Symver::None => false,
            Symver::At | Symver::AtAt | Symver::Script => true,
        }
    }

    /// 是否默认版本写法。
    pub fn sets_default(&self) -> bool {
        match self {
            Symver::AtAt => true,
            _ => false,
        }
    }
}

/// 版本节点状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeState {
    /// 节点存在。
    Present,
    /// 节点不存在。
    Missing,
    /// 节点索引与已有节点重号。
    DupIndex,
}

impl NodeState {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            NodeState::Present => "present",
            NodeState::Missing => "missing",
            NodeState::DupIndex => "dup-index",
        }
    }
}

/// 版本定义阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 指令。
    Symver,
    /// 节点。
    Node,
    /// 索引。
    Index,
    /// aux 链。
    Aux,
    /// 版本名 hash。
    Hash,
    /// 默认规则。
    Default,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Symver => 0,
            Stage::Node => 1,
            Stage::Index => 2,
            Stage::Aux => 3,
            Stage::Hash => 4,
            Stage::Default => 5,
            Stage::Ready => 6,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Symver => "symver",
            Stage::Node => "node",
            Stage::Index => "index",
            Stage::Aux => "aux",
            Stage::Hash => "hash",
            Stage::Default => "default",
            Stage::Ready => "ready",
        }
    }

    /// 是否失败点。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Ready => false,
            _ => true,
        }
    }
}

/// 定义注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 无版本符号。
    Unversioned,
    /// 默认版本。
    Ready,
    /// 非默认版本。
    NonDefault,
    /// 版本脚本分配。
    ScriptBound,
    /// 隐藏引用命中非默认版本。
    Hidden,
    /// 基节点。
    Base,
    /// 节点不存在。
    NoNode,
    /// 默认版本重复。
    DuplicateDefault,
    /// 基节点被当默认。
    BaseDefault,
    /// 索引非法。
    BadIndex,
    /// aux 链非法。
    BadAux,
    /// hash 不符。
    BadHash,
    /// vd_next 链断裂。
    BadChain,
    /// 指令非法。
    BadDirective,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Unversioned => "unversioned",
            Note::Ready => "ready",
            Note::NonDefault => "nondefault",
            Note::ScriptBound => "script-bound",
            Note::Hidden => "hidden",
            Note::Base => "base",
            Note::NoNode => "no-node",
            Note::DuplicateDefault => "dup-default",
            Note::BaseDefault => "base-default",
            Note::BadIndex => "bad-index",
            Note::BadAux => "bad-aux",
            Note::BadHash => "bad-hash",
            Note::BadChain => "bad-chain",
            Note::BadDirective => "bad-directive",
        }
    }

    /// 是否成功。
    pub fn success(&self) -> bool {
        match self {
            Note::Unversioned
            | Note::Ready
            | Note::NonDefault
            | Note::ScriptBound
            | Note::Hidden
            | Note::Base => true,
            _ => false,
        }
    }

    /// 是否结构性问题。
    pub fn malformed(&self) -> bool {
        match self {
            Note::NoNode
            | Note::BadIndex
            | Note::BadAux
            | Note::BadHash
            | Note::BadChain
            | Note::BadDirective => true,
            _ => false,
        }
    }

    /// 是否默认规则冲突。
    pub fn conflict(&self) -> bool {
        match self {
            Note::DuplicateDefault | Note::BaseDefault => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Verdef 表
// ---------------------------------------------------------------------------

/// 一条 Verdef + 其首 Verdaux 的模型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdef {
    /// 版本名。
    pub name: &'static str,
    /// `vd_version`。
    pub version: u16,
    /// `vd_flags`。
    pub flags: u16,
    /// `vd_ndx`。
    pub ndx: u16,
    /// `vd_cnt`。
    pub cnt: u8,
    /// `vd_hash`。
    pub hash: u32,
    /// `vd_aux` 偏移。
    pub aux_off: u16,
    /// `vd_next` 偏移（0 表示链尾）。
    pub next_off: u16,
    /// 首 aux 名。
    pub aux_name: &'static str,
    /// `vda_next`（0 表示 aux 链尾）。
    pub aux_next: u16,
}

impl Verdef {
    /// 空白记录。
    pub const fn blank() -> Verdef {
        Verdef {
            name: "",
            version: 1,
            flags: 0,
            ndx: 0,
            cnt: 0,
            hash: 0,
            aux_off: VERDEF_SIZE,
            next_off: 0,
            aux_name: "",
            aux_next: 0,
        }
    }

    /// 是否结构良好。
    pub fn well_formed(&self) -> bool {
        if self.version != 1 {
            return false;
        }
        if self.ndx == 0 {
            return false;
        }
        if self.cnt == 0 || self.cnt > VERDEF_AUX_MAX {
            return false;
        }
        if self.cnt == 1 && self.aux_next != 0 {
            return false;
        }
        if self.cnt > 1 && self.aux_next == 0 {
            return false;
        }
        if self.aux_off < VERDEF_SIZE {
            return false;
        }
        if self.aux_name.is_empty() {
            return false;
        }
        self.hash == elf_hash(self.name)
    }

    /// 是否基节点。
    pub fn is_base(&self) -> bool {
        self.flags & VER_FLG_BASE != 0
    }

    /// 是否链尾。
    pub fn last(&self) -> bool {
        self.next_off == 0
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.name);
        h = fold_word(h, self.version as u64);
        h = fold_word(h, self.flags as u64);
        h = fold_word(h, self.ndx as u64);
        h = fold_word(h, self.cnt as u64);
        h = fold_word(h, self.hash as u64);
        h = fold_word(h, self.aux_off as u64);
        h = fold_word(h, self.next_off as u64);
        h = fold_str(h, self.aux_name);
        h = fold_word(h, self.aux_next as u64);
        h
    }
}

/// 版本定义表（基节点 + V1 + V2）。
pub fn verdefs() -> [Verdef; 3] {
    [
        Verdef {
            name: "libx.so",
            version: 1,
            flags: VER_FLG_BASE,
            ndx: 1,
            cnt: 1,
            hash: elf_hash("libx.so"),
            aux_off: 20,
            next_off: 48,
            aux_name: "libx.so",
            aux_next: 0,
        },
        Verdef {
            name: "V1",
            version: 1,
            flags: 0,
            ndx: 2,
            cnt: 1,
            hash: elf_hash("V1"),
            aux_off: 20,
            next_off: 48,
            aux_name: "V1",
            aux_next: 0,
        },
        Verdef {
            name: "V2",
            version: 1,
            flags: 0,
            ndx: 3,
            cnt: 1,
            hash: elf_hash("V2"),
            aux_off: 20,
            next_off: 0,
            aux_name: "V2",
            aux_next: 0,
        },
    ]
}

// ---------------------------------------------------------------------------
// 形状
// ---------------------------------------------------------------------------

/// 一条符号版本定义的形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 符号名。
    pub sym: &'static str,
    /// 版本名。
    pub ver: &'static str,
    /// 第二个默认版本名（冲突展示用）。
    pub ver2: &'static str,
    /// 指令形态。
    pub dir: Symver,
    /// 节点状态。
    pub node: NodeState,
    /// versym 索引。
    pub ndx: u16,
    /// `vd_flags`。
    pub flags: u16,
    /// `vd_cnt`。
    pub cnt: u8,
    /// 首 aux 名。
    pub aux_name: &'static str,
    /// `vda_next`。
    pub aux_next: u16,
    /// `vd_next`。
    pub next_off: u16,
    /// hash 是否与版本名一致。
    pub hash_ok: bool,
    /// 是否已有默认版本。
    pub has_default: bool,
    /// 是否隐藏引用。
    pub hidden: bool,
}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            sym: "",
            ver: "",
            ver2: "",
            dir: Symver::None,
            node: NodeState::Present,
            ndx: 0,
            flags: 0,
            cnt: 0,
            aux_name: "",
            aux_next: 0,
            next_off: 0,
            hash_ok: true,
            has_default: false,
            hidden: false,
        }
    }

    /// 换 versym 索引的副本（判据里用于把同一形状推到不同档位复算）。
    pub const fn clone_ndx(&self, n: u16) -> Shape {
        Shape {
            tag: self.tag,
            sym: self.sym,
            ver: self.ver,
            ver2: self.ver2,
            dir: self.dir,
            node: self.node,
            ndx: n,
            flags: self.flags,
            cnt: self.cnt,
            aux_name: self.aux_name,
            aux_next: self.aux_next,
            next_off: self.next_off,
            hash_ok: self.hash_ok,
            has_default: self.has_default,
            hidden: self.hidden,
        }
    }

    /// 是否基节点。
    pub fn base(&self) -> bool {
        self.flags & VER_FLG_BASE != 0
    }

    /// 是否声明版本。
    pub fn versioned(&self) -> bool {
        self.dir.versioned()
    }

    /// 是否语法完整。
    pub fn lex_ok(&self) -> bool {
        if self.sym.is_empty() {
            return false;
        }
        match self.dir {
            Symver::None => self.ver.is_empty(),
            Symver::At | Symver::AtAt | Symver::Script => !self.ver.is_empty(),
        }
    }

    /// 名字指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.sym);
        h = fold_str(h, self.ver);
        h = fold_str(h, self.dir.text());
        h = fold_word(h, self.ndx as u64);
        h = fold_word(h, self.flags as u64);
        h = fold_word(h, self.cnt as u64);
        h = fold_str(h, self.aux_name);
        h = fold_word(h, self.hash_ok as u64);
        h = fold_word(h, self.has_default as u64);
        h
    }
}

/// 定义结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段。
    pub stage: Stage,
    /// errno（0 表示成功）。
    pub code: i32,
    /// versym 索引。
    pub ndx: u16,
    /// 注解。
    pub note: Note,
    /// 是否对无版本引用可见。
    pub default: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome { stage: Stage::Symver, code: 0, ndx: 0, note: Note::Ready, default: false }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.code == 0 && self.stage == Stage::Ready && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否对无版本引用可见。
    pub fn visible(&self) -> bool {
        self.ok() && self.default
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.order() as u64);
        h = fold_word(h, self.code as i64 as u64);
        h = fold_word(h, self.ndx as u64);
        h = fold_str(h, self.note.name());
        h = fold_word(h, self.default as u64);
        h
    }
}

// ---------------------------------------------------------------------------
// 定义校验
// ---------------------------------------------------------------------------

/// 校验一条版本定义。
///
/// 闸门次序：`symver → node → index → aux → hash → default → ready`。
/// 每一段只在上一段放行后执行；失败立即返回该段的 errno 与注解。
pub fn define(s: &Shape) -> Outcome {
    // ---- symver ----
    if !s.lex_ok() {
        return Outcome {
            stage: Stage::Symver,
            code: ERR_EINVAL,
            ndx: 0,
            note: Note::BadDirective,
            default: false,
        };
    }
    if !s.versioned() {
        // 无版本符号：索引只能是全局 1。
        if s.ndx != VER_NDX_BASE {
            return Outcome {
                stage: Stage::Index,
                code: ERR_EINVAL,
                ndx: s.ndx,
                note: Note::BadIndex,
                default: false,
            };
        }
        return Outcome {
            stage: Stage::Ready,
            code: 0,
            ndx: VER_NDX_BASE,
            note: Note::Unversioned,
            default: true,
        };
    }

    // ---- node ----
    match s.node {
        NodeState::Missing => {
            return Outcome {
                stage: Stage::Node,
                code: ERR_ENOENT,
                ndx: 0,
                note: Note::NoNode,
                default: false,
            };
        }
        NodeState::DupIndex => {
            return Outcome {
                stage: Stage::Index,
                code: ERR_EEXIST,
                ndx: s.ndx,
                note: Note::BadIndex,
                default: false,
            };
        }
        NodeState::Present => {}
    }

    // ---- index ----
    let base_idx = s.base() && s.ndx == VER_NDX_BASE;
    if s.ndx > VER_NDX_MAX || (s.ndx < 2 && !base_idx) {
        return Outcome {
            stage: Stage::Index,
            code: ERR_EINVAL,
            ndx: s.ndx,
            note: Note::BadIndex,
            default: false,
        };
    }
    if s.hidden && s.ndx & VER_NDX_HIDDEN == 0 {
        // 隐藏引用必须带隐藏位；这里只校验低位索引，隐藏位是引用侧标记。
    }

    // ---- aux ----
    if s.cnt == 0 || s.cnt > VERDEF_AUX_MAX {
        return Outcome {
            stage: Stage::Aux,
            code: ERR_EINVAL,
            ndx: s.ndx,
            note: Note::BadAux,
            default: false,
        };
    }
    if s.aux_name != s.ver {
        return Outcome {
            stage: Stage::Aux,
            code: ERR_EINVAL,
            ndx: s.ndx,
            note: Note::BadAux,
            default: false,
        };
    }
    if (s.cnt == 1 && s.aux_next != 0) || (s.cnt > 1 && s.aux_next == 0) {
        return Outcome {
            stage: Stage::Aux,
            code: ERR_EINVAL,
            ndx: s.ndx,
            note: Note::BadAux,
            default: false,
        };
    }
    if s.next_off != 0 {
        return Outcome {
            stage: Stage::Aux,
            code: ERR_EINVAL,
            ndx: s.ndx,
            note: Note::BadChain,
            default: false,
        };
    }

    // ---- hash ----
    if !s.hash_ok {
        return Outcome {
            stage: Stage::Hash,
            code: ERR_EINVAL,
            ndx: s.ndx,
            note: Note::BadHash,
            default: false,
        };
    }

    // ---- default ----
    if s.dir == Symver::AtAt {
        if s.base() {
            return Outcome {
                stage: Stage::Default,
                code: ERR_EINVAL,
                ndx: s.ndx,
                note: Note::BaseDefault,
                default: false,
            };
        }
        if s.has_default {
            return Outcome {
                stage: Stage::Default,
                code: ERR_EEXIST,
                ndx: s.ndx,
                note: Note::DuplicateDefault,
                default: false,
            };
        }
        return Outcome {
            stage: Stage::Ready,
            code: 0,
            ndx: s.ndx,
            note: Note::Ready,
            default: true,
        };
    }
    if s.dir == Symver::At {
        let note = if s.hidden { Note::Hidden } else { Note::NonDefault };
        return Outcome {
            stage: Stage::Ready,
            code: 0,
            ndx: s.ndx,
            note,
            default: false,
        };
    }
    // 版本脚本分配。
    if s.base() {
        return Outcome {
            stage: Stage::Ready,
            code: 0,
            ndx: VER_NDX_BASE,
            note: Note::Base,
            default: false,
        };
    }
    Outcome {
        stage: Stage::Ready,
        code: 0,
        ndx: s.ndx,
        note: Note::ScriptBound,
        default: true,
    }
}

/// 文案。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::NoNode => msg_no_node(s.sym, s.ver),
        Note::DuplicateDefault => msg_dup_default(s.sym, s.ver, s.ver2),
        Note::BaseDefault => msg_base_default(s.sym, s.ver),
        Note::BadIndex => msg_bad_index(s.sym, o.ndx),
        Note::BadAux => msg_bad_aux(s.ver),
        Note::BadHash => {
            let want = elf_hash(s.ver);
            let got = if s.hash_ok { want } else { want ^ 0xffff_ffff };
            msg_bad_hash(s.ver, want, got)
        }
        Note::BadChain => msg_bad_chain(s.ver),
        Note::BadDirective => msg_bad_directive(s.sym),
        Note::Unversioned
        | Note::Ready
        | Note::NonDefault
        | Note::ScriptBound
        | Note::Hidden
        | Note::Base => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 结果档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 成功且无版本引用可见。
    Live,
    /// 成功但仅带版本引用可见。
    Hidden,
    /// 结构性问题。
    Malformed,
    /// 默认规则冲突。
    Conflict,
}

impl Tier {
    /// 由结果投影。
    pub fn of(o: &Outcome) -> Tier {
        if o.ok() {
            if o.default {
                Tier::Live
            } else {
                Tier::Hidden
            }
        } else if o.note.conflict() {
            Tier::Conflict
        } else {
            Tier::Malformed
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Live => 0,
            Tier::Hidden => 1,
            Tier::Malformed => 2,
            Tier::Conflict => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Live => "live",
            Tier::Hidden => "hidden",
            Tier::Malformed => "malformed",
            Tier::Conflict => "conflict",
        }
    }

    /// 是否成功档。
    pub fn success(&self) -> bool {
        match self {
            Tier::Live | Tier::Hidden => true,
            Tier::Malformed | Tier::Conflict => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 缺陷
// ---------------------------------------------------------------------------

/// 申报缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 阶段错。
    Stage,
    /// errno 错。
    Code,
    /// 索引错。
    Value,
    /// 注解错。
    Note,
    /// 默认可见性错。
    Default,
    /// 链错。
    Chain,
    /// 成功/失败互报。
    Estate,
    /// 计数错。
    Count,
    /// 槽位错。
    Slot,
    /// 指纹错。
    Seal,
    /// 无观测。
    Nobs,
    /// 矩阵不平。
    Matrix,
    /// 账本不平。
    Bal,
}

impl Fault {
    /// 错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Stage => SV_ESTAGE,
            Fault::Code => SV_ECODE,
            Fault::Value => SV_EVALUE,
            Fault::Note => SV_ENOTE,
            Fault::Default => SV_EDEFAULT,
            Fault::Chain => SV_ECHAIN,
            Fault::Estate => SV_ESTATE,
            Fault::Count => SV_ECOUNT,
            Fault::Slot => SV_ESLOT,
            Fault::Seal => SV_ESEAL,
            Fault::Nobs => SV_ENOBS,
            Fault::Matrix => SV_EMATRIX,
            Fault::Bal => SV_EBAL,
        }
    }

    /// 是否非零。
    pub fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

/// 短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Value => "value",
        Fault::Note => "note",
        Fault::Default => "default",
        Fault::Chain => "chain",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Bal => "bal",
    }
}

// ---------------------------------------------------------------------------
// 申报与用例
// ---------------------------------------------------------------------------

/// 一槽申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 结束阶段。
    pub stage: Stage,
    /// errno。
    pub code: i32,
    /// versym 索引。
    pub value: u16,
    /// 注解。
    pub note: Note,
    /// 默认可见性。
    pub default: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Ready, code: 0, value: 0, note: Note::Ready, default: false }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.stage == Stage::Ready && self.code == 0
    }
}

/// 一槽用例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 空白用例。
    pub const fn blank() -> Case {
        Case { shape: Shape::blank(), claim: Claim::blank() }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        self.shape.tag
    }

    /// 实际结果。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
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

// ---------------------------------------------------------------------------
// 语料（14 槽）
// ---------------------------------------------------------------------------

/// 8 个可正确申报的形状 + 6 个复刻形状。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        tag: "unversioned",
        sym: "foo",
        ver: "",
        ver2: "",
        dir: Symver::None,
        node: NodeState::Present,
        ndx: VER_NDX_BASE,
        flags: 0,
        cnt: 0,
        aux_name: "",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "default-double-at",
        sym: "foo",
        ver: "V1",
        ver2: "",
        dir: Symver::AtAt,
        node: NodeState::Present,
        ndx: 2,
        flags: 0,
        cnt: 1,
        aux_name: "V1",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "nondefault-at",
        sym: "foo",
        ver: "V1",
        ver2: "",
        dir: Symver::At,
        node: NodeState::Present,
        ndx: 2,
        flags: 0,
        cnt: 1,
        aux_name: "V1",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: true,
        hidden: false,
    },
    Shape {
        tag: "script-bound",
        sym: "foo",
        ver: "V2",
        ver2: "",
        dir: Symver::Script,
        node: NodeState::Present,
        ndx: 3,
        flags: 0,
        cnt: 1,
        aux_name: "V2",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "missing-node",
        sym: "foo",
        ver: "V9",
        ver2: "",
        dir: Symver::At,
        node: NodeState::Missing,
        ndx: 0,
        flags: 0,
        cnt: 1,
        aux_name: "V9",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "dup-default",
        sym: "foo",
        ver: "V2",
        ver2: "V1",
        dir: Symver::AtAt,
        node: NodeState::Present,
        ndx: 2,
        flags: 0,
        cnt: 1,
        aux_name: "V2",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: true,
        hidden: false,
    },
    Shape {
        tag: "bad-hash",
        sym: "foo",
        ver: "V3",
        ver2: "",
        dir: Symver::AtAt,
        node: NodeState::Present,
        ndx: 4,
        flags: 0,
        cnt: 1,
        aux_name: "V3",
        aux_next: 0,
        next_off: 0,
        hash_ok: false,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "base-default",
        sym: "foo",
        ver: "libx.so",
        ver2: "",
        dir: Symver::AtAt,
        node: NodeState::Present,
        ndx: VER_NDX_BASE,
        flags: VER_FLG_BASE,
        cnt: 1,
        aux_name: "libx.so",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "estate-swap",
        sym: "foo",
        ver: "V9",
        ver2: "",
        dir: Symver::At,
        node: NodeState::Missing,
        ndx: 0,
        flags: 0,
        cnt: 1,
        aux_name: "V9",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "stage-swap",
        sym: "foo",
        ver: "V9",
        ver2: "",
        dir: Symver::At,
        node: NodeState::Missing,
        ndx: 0,
        flags: 0,
        cnt: 1,
        aux_name: "V9",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "code-swap",
        sym: "foo",
        ver: "V9",
        ver2: "",
        dir: Symver::At,
        node: NodeState::Missing,
        ndx: 0,
        flags: 0,
        cnt: 1,
        aux_name: "V9",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "value-swap",
        sym: "foo",
        ver: "V9",
        ver2: "",
        dir: Symver::At,
        node: NodeState::Missing,
        ndx: 0,
        flags: 0,
        cnt: 1,
        aux_name: "V9",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "note-swap",
        sym: "foo",
        ver: "V9",
        ver2: "",
        dir: Symver::At,
        node: NodeState::Missing,
        ndx: 0,
        flags: 0,
        cnt: 1,
        aux_name: "V9",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
    Shape {
        tag: "default-swap",
        sym: "foo",
        ver: "V9",
        ver2: "",
        dir: Symver::At,
        node: NodeState::Missing,
        ndx: 0,
        flags: 0,
        cnt: 1,
        aux_name: "V9",
        aux_next: 0,
        next_off: 0,
        hash_ok: true,
        has_default: false,
        hidden: false,
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Ready, code: 0, value: 1, note: Note::Unversioned, default: true },
    Claim { stage: Stage::Ready, code: 0, value: 2, note: Note::Ready, default: true },
    Claim { stage: Stage::Ready, code: 0, value: 2, note: Note::NonDefault, default: false },
    Claim { stage: Stage::Ready, code: 0, value: 3, note: Note::ScriptBound, default: true },
    Claim { stage: Stage::Node, code: ERR_ENOENT, value: 0, note: Note::NoNode, default: false },
    Claim { stage: Stage::Default, code: ERR_EEXIST, value: 2, note: Note::DuplicateDefault, default: false },
    Claim { stage: Stage::Hash, code: ERR_EINVAL, value: 4, note: Note::BadHash, default: false },
    Claim { stage: Stage::Default, code: ERR_EINVAL, value: 1, note: Note::BaseDefault, default: false },
    Claim { stage: Stage::Ready, code: 0, value: 1, note: Note::Ready, default: true },
    Claim { stage: Stage::Index, code: ERR_ENOENT, value: 0, note: Note::NoNode, default: false },
    Claim { stage: Stage::Node, code: 0, value: 0, note: Note::NoNode, default: false },
    Claim { stage: Stage::Node, code: ERR_ENOENT, value: 9, note: Note::NoNode, default: false },
    Claim { stage: Stage::Node, code: ERR_ENOENT, value: 0, note: Note::BadHash, default: false },
    Claim { stage: Stage::Node, code: ERR_ENOENT, value: 0, note: Note::NoNode, default: true },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Value, Fault::Note, Fault::Default];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    SV_ESTATE,
    SV_ESTAGE,
    SV_ECODE,
    SV_EVALUE,
    SV_ENOTE,
    SV_EDEFAULT,
];

/// 账本 8 个成功格的值（总数、成、败、可见、隐藏、结构坏、冲突、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 3, 1, 2, 2, 929];

/// 语料表。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(s), Some(c)) = (SHAPES.get(i), CLAIMS.get(i)) {
            out[i] = Case { shape: *s, claim: *c };
        }
        i += 1;
    }
    out
}

/// 取一槽。
pub fn case_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    cases().get(i).copied()
}

/// 覆盖计数。
pub fn cover(n: usize) -> Result<(), u32> {
    if n != CORPUS {
        return Err(SV_ENOBS);
    }
    Ok(())
}

/// 期望缺陷。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        return Some(Fault::Ok);
    }
    REJECT_FAULTS.get(i - ACCEPT).copied()
}

/// 对判。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    let k = c.claim;
    if a.ok() != k.success() {
        return Fault::Estate;
    }
    if a.stage != k.stage {
        return Fault::Stage;
    }
    if a.code != k.code {
        return Fault::Code;
    }
    if a.ndx != k.value {
        return Fault::Value;
    }
    if a.note != k.note {
        return Fault::Note;
    }
    if a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 逐槽释义。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let msg = message(&c.shape, &o);
    let mut s = String::from(c.shape.sym);
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" code=");
    s.push_str(i32_text(o.code).as_str());
    s.push_str(" ndx=");
    s.push_str(u32_text(o.ndx as u32).as_str());
    s.push_str(" default=");
    s.push_str(bool_text(o.default));
    s.push_str(" note=");
    s.push_str(o.note.name());
    if !msg.is_empty() {
        s.push_str(" msg=\"");
        s.push_str(msg.as_str());
        s.push('"');
    }
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// 期望表（语料的第二份陈述）
// ---------------------------------------------------------------------------

/// 14 槽的期望结束阶段。
///
/// 这份表是把 `SHAPES`/`CLAIMS` 用**另一种形式**再写一遍：申报表说的是
/// "我声明什么"，期望表说的是"校验器实际会走到哪一步"。两者一致才算数，
/// 任一处笔误都会在这里露出来。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Node,
    Stage::Default,
    Stage::Hash,
    Stage::Default,
    Stage::Node,
    Stage::Node,
    Stage::Node,
    Stage::Node,
    Stage::Node,
    Stage::Node,
];

/// 14 槽的期望 errno。
const WANT_CODE: [i32; CORPUS] = [
    0,
    0,
    0,
    0,
    ERR_ENOENT,
    ERR_EEXIST,
    ERR_EINVAL,
    ERR_EINVAL,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
    ERR_ENOENT,
];

/// 14 槽的期望 versym 索引。
const WANT_NDX: [u16; CORPUS] = [1, 2, 2, 3, 0, 2, 4, 1, 0, 0, 0, 0, 0, 0];

/// 14 槽的期望注解。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Unversioned,
    Note::Ready,
    Note::NonDefault,
    Note::ScriptBound,
    Note::NoNode,
    Note::DuplicateDefault,
    Note::BadHash,
    Note::BaseDefault,
    Note::NoNode,
    Note::NoNode,
    Note::NoNode,
    Note::NoNode,
    Note::NoNode,
    Note::NoNode,
];

/// 14 槽的期望默认可见性。
const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, false, true, false, false, false, false, false, false, false, false, false, false,
];

/// 14 槽标签。
const WANT_TAGS: [&str; CORPUS] = [
    "unversioned",
    "default-double-at",
    "nondefault-at",
    "script-bound",
    "missing-node",
    "dup-default",
    "bad-hash",
    "base-default",
    "estate-swap",
    "stage-swap",
    "code-swap",
    "value-swap",
    "note-swap",
    "default-swap",
];

/// 八格申报正确的下标。
const ACCEPT_SLOTS: [usize; ACCEPT] = [0usize, 1, 2, 3, 4, 5, 6, 7];

/// 六格申报错误的下标。
const REJECT_SLOTS: [usize; REJECT] = [8usize, 9, 10, 11, 12, 13];

// ---------------------------------------------------------------------------
// 统计矩阵
// ---------------------------------------------------------------------------

/// 一档统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档实际槽数。
    pub total: usize,
    /// 该档申报正确数。
    pub good: usize,
}

impl Row {
    /// 空白行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }

    /// 该档是否全对（有槽且申报全对）。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }

    /// 该档红档数。
    pub fn red(&self) -> usize {
        if self.good <= self.total {
            self.total - self.good
        } else {
            self.total
        }
    }

    /// 渲染一行。
    pub fn render(&self, t: Tier) -> String {
        let mut s = String::from(t.name());
        s.push_str(" total=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" good=");
        s.push_str(u32_text(self.good as u32).as_str());
        s.push_str(" red=");
        s.push_str(u32_text(self.red() as u32).as_str());
        s.push('\n');
        s
    }
}

/// 四档矩阵。
///
/// 分档只看**实际结果**落在哪一档，申报正确与否单独计。这样矩阵才能同时
/// 回答两个不同的问题：语料覆盖面是否均衡（total 分布），以及申报是否可信
/// （good/total）。如果把两者混成一维，红档槽就会从自己那一档里消失，
/// 覆盖面看起来仍是满的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行（live/hidden/malformed/conflict）。
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
            let c = match cs.get(i) {
                Some(v) => *v,
                None => return m,
            };
            let t = Tier::of(&c.actual()).index();
            let good = judge(&c) == Fault::Ok;
            if let Some(row) = m.rows.get_mut(t) {
                row.total += 1;
                if good {
                    row.good += 1;
                }
            }
            i += 1;
        }
        m
    }

    /// 取一档。
    pub fn row(&self, t: Tier) -> Option<Row> {
        self.rows.get(t.index()).copied()
    }

    /// 实际槽总数（必须等于语料规模）。
    pub fn slots(&self) -> usize {
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

    /// 申报正确总数。
    pub fn goods(&self) -> usize {
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

    /// 全对档数。
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

    /// 覆盖是否平（每档都有槽）。
    pub fn covered(&self) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            match self.rows.get(i) {
                Some(r) => {
                    if r.total == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 渲染全矩阵。
    pub fn render(&self) -> String {
        let tiers = [Tier::Live, Tier::Hidden, Tier::Malformed, Tier::Conflict];
        let mut s = String::new();
        let mut i = 0usize;
        while i < tiers.len() {
            if let Some(t) = tiers.get(i) {
                if let Some(r) = self.row(*t) {
                    s.push_str(r.render(*t).as_str());
                }
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 汇总报表
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
    /// 可见档（live）正确数。
    pub live: u32,
    /// 隐藏档（hidden）正确数。
    pub hidden: u32,
    /// 结构坏档正确数。
    pub malformed: u32,
    /// 冲突档正确数。
    pub conflict: u32,
    /// 对判命中数（实际缺陷 == 期望缺陷）。
    pub matched: u32,
}

impl Report {
    /// 千分表示（四舍五入）。
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

    /// 闸门：不到全绿就报计数错。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(SV_ECOUNT)
        }
    }

    /// 成功档合计（申报正确且结果成功）。
    pub fn success_cells(&self) -> u32 {
        self.live + self.hidden
    }

    /// 失败档合计（申报正确且结果失败）。
    pub fn failure_cells(&self) -> u32 {
        self.malformed + self.conflict
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("symver corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" live=");
        s.push_str(u32_text(self.live).as_str());
        s.push_str(" hidden=");
        s.push_str(u32_text(self.hidden).as_str());
        s.push_str(" malformed=");
        s.push_str(u32_text(self.malformed).as_str());
        s.push_str(" conflict=");
        s.push_str(u32_text(self.conflict).as_str());
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

/// 可注入一槽缺陷的全跑。
///
/// 注入的作用是把"期望表"本身也放进被测面：强行宣称某槽犯了另一种缺陷，
/// 报表就必须掉 matched、rate 随之离开 1000。若注入后仍报全绿，说明
/// matched 是硬编码的，而不是算出来的。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(SV_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut live = 0u32;
    let mut hidden = 0u32;
    let mut malformed = 0u32;
    let mut conflict = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(SV_ENOBS),
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
            None => return Err(SV_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Live => live += 1,
                Tier::Hidden => hidden += 1,
                Tier::Malformed => malformed += 1,
                Tier::Conflict => conflict += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report {
        total,
        accept,
        reject,
        live,
        hidden,
        malformed,
        conflict,
        matched,
    })
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
///
/// 定义纵线是 `symver → node → index → aux → hash → default → ready`，
/// 七段只能向前。回退（例如已经判死在 node 又声称走到了 hash）说明
/// 申报者没有按闸门次序观测，而是事后倒推——这类申报在真实故障里最危险，
/// 因为文案看着完全合理。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    /// 上一段次序（-1 表示未走）。
    last: i32,
    /// 已走段数。
    steps: u32,
    /// 是否已关。
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
            return Err(SV_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(SV_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭，返回段数。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(SV_ESTATE);
        }
        self.closed = true;
        Ok(self.steps)
    }

    /// 段数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 上一段。
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
    /// 槽位（必须严格递增）。
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

/// 账本容量。
pub const CAP: usize = 6;

/// 账本：14 格，8 成 6 败，冻结后拒登记。
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

    /// 登记一格；槽位不连续、越界或已冻结一律拒绝并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SV_ESLOT);
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

    /// 逐格对账：槽位连续、成格值等于 `VALS`、败格码等于 `REJECT_CODES`。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(SV_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SV_EBAL),
            };
            if s.slot as usize != i {
                return Err(SV_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SV_EVALUE);
                        }
                    }
                    None => return Err(SV_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(SV_EVALUE);
                }
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SV_EVALUE);
                        }
                    }
                    None => return Err(SV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(SV_EBAL);
        }
        Ok(())
    }

    /// 全账指纹。
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
            return Err(SV_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建一份账本并比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(SV_ESEAL)
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

/// 计数结果 → 样本。
fn count_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 建账本：7 格统计 + 1 格红档率 + 6 格缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total)));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept)));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject)));
    let _ = l.register(count_cell(3, OP_DEFAULT, r.map(|x| x.live)));
    let _ = l.register(count_cell(4, OP_SYMVER, r.map(|x| x.hidden)));
    let _ = l.register(count_cell(5, OP_NODE, r.map(|x| x.malformed)));
    let _ = l.register(count_cell(6, OP_AUX, r.map(|x| x.conflict)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_REPLAY, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_INDEX,
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

/// 四路注入：形状表、判据、渲染、账本。
///
/// 四路各自独立，任何一路静默退化都会在这里变成 false：
/// 1. 表路——段基址、枚举、Verdef 结构、语法闸门；
/// 2. 判路——七闸纵线、默认规则、对判、矩阵；
/// 3. 渲染路——语料、期望表、红档、文案、释义；
/// 4. 账本路——阶段闸门、逐格对账、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && enums_ok() && verdef_ok() && directive_ok();
    out[1] = define_ok() && default_ok() && judge_ok() && matrix_ok();
    out[2] = corpus_ok() && slots_ok() && reds_ok() && message_ok() && detail_ok();
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

/// 名称与段基址：常量链、尺寸、hash 一致性。
fn name_ok() -> bool {
    if SEG_BASE != 0x6cea0 {
        return false;
    }
    if SV_ESTAGE != SEG_BASE + 0x01
        || SV_ECODE != SEG_BASE + 0x02
        || SV_EVALUE != SEG_BASE + 0x03
        || SV_ENOTE != SEG_BASE + 0x04
        || SV_EDEFAULT != SEG_BASE + 0x05
        || SV_ECHAIN != SEG_BASE + 0x06
    {
        return false;
    }
    if SV_ESTATE != SEG_BASE + 0x07
        || SV_ECOUNT != SEG_BASE + 0x08
        || SV_ESLOT != SEG_BASE + 0x09
        || SV_ESEAL != SEG_BASE + 0x0a
        || SV_ENOBS != SEG_BASE + 0x0b
        || SV_EMATRIX != SEG_BASE + 0x0c
        || SV_EBAL != SEG_BASE + 0x0d
    {
        return false;
    }
    if CORPUS != 14 || SAMPLES != 14 || ACCEPT != 8 || REJECT != 6 || TIER_COUNT != 4 {
        return false;
    }
    if VERDEF_SIZE != 20 || VERDAUX_SIZE != 8 || VERDEF_AUX_MAX != 4 {
        return false;
    }
    if VER_NDX_BASE != 1 || VER_FLG_BASE != 0x0001 || VER_FLG_WEAK != 0x0002 {
        return false;
    }
    if VER_NDX_HIDDEN != 0x8000 || VER_NDX_MAX != 0x7fff {
        return false;
    }
    if ERR_ENOENT != 2 || ERR_EEXIST != 17 || ERR_EINVAL != 22 {
        return false;
    }
    if OP_SYMVER != 1 || OP_NODE != 2 || OP_INDEX != 3 || OP_AUX != 4 {
        return false;
    }
    if OP_HASH != 5 || OP_DEFAULT != 6 || OP_CORPUS != 7 || OP_REPLAY != 8 {
        return false;
    }
    // hash 必须与版本名一致，且不同名不同 hash（否则 hash 维失去判别力）。
    if elf_hash("V1") == elf_hash("libx.so") || elf_hash("V1") == elf_hash("V2") {
        return false;
    }
    let mut v = String::new();
    v.push_str("foo@V1");
    !v.is_empty()
}

/// 枚举：次序、文本唯一性、分类不重不漏。
fn enums_ok() -> bool {
    let stages = [
        Stage::Symver,
        Stage::Node,
        Stage::Index,
        Stage::Aux,
        Stage::Hash,
        Stage::Default,
        Stage::Ready,
    ];
    let mut i = 0usize;
    while i < stages.len() {
        let s = match stages.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.order() as usize != i {
            return false;
        }
        if i + 1 == stages.len() {
            if s.failure() {
                return false;
            }
        } else if !s.failure() {
            return false;
        }
        let mut k = i + 1;
        while k < stages.len() {
            if let Some(o) = stages.get(k) {
                if o.name() == s.name() {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }

    let notes = [
        Note::Unversioned,
        Note::Ready,
        Note::NonDefault,
        Note::ScriptBound,
        Note::Hidden,
        Note::Base,
        Note::NoNode,
        Note::DuplicateDefault,
        Note::BaseDefault,
        Note::BadIndex,
        Note::BadAux,
        Note::BadHash,
        Note::BadChain,
        Note::BadDirective,
    ];
    let mut n = 0usize;
    while n < notes.len() {
        let a = match notes.get(n) {
            Some(v) => *v,
            None => return false,
        };
        let mut m = n + 1;
        while m < notes.len() {
            if let Some(b) = notes.get(m) {
                if a.name() == b.name() {
                    return false;
                }
            }
            m += 1;
        }
        // 三分类互斥：成功档不得同时是结构坏或冲突。
        if a.success() && (a.malformed() || a.conflict()) {
            return false;
        }
        if a.conflict() && a.malformed() {
            return false;
        }
        n += 1;
    }
    if Note::NoNode.malformed() == false || Note::DuplicateDefault.conflict() == false {
        return false;
    }
    if Note::Base.success() == false || Note::Hidden.success() == false {
        return false;
    }

    let tiers = [Tier::Live, Tier::Hidden, Tier::Malformed, Tier::Conflict];
    let mut t = 0usize;
    while t < tiers.len() {
        let x = match tiers.get(t) {
            Some(v) => *v,
            None => return false,
        };
        if x.index() != t {
            return false;
        }
        if x.success() != (t < 2) {
            return false;
        }
        let mut y = t + 1;
        while y < tiers.len() {
            if let Some(z) = tiers.get(y) {
                if x.name() == z.name() {
                    return false;
                }
            }
            y += 1;
        }
        t += 1;
    }

    let faults = [
        Fault::Ok,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
        Fault::Chain,
        Fault::Estate,
        Fault::Count,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Matrix,
        Fault::Bal,
    ];
    let mut f = 0usize;
    let mut seen = [0u32; 14];
    while f < faults.len() {
        let x = match faults.get(f) {
            Some(v) => *v,
            None => return false,
        };
        if x.alive() == (x == Fault::Ok) {
            return false;
        }
        if fault_short(x).is_empty() {
            return false;
        }
        if let Some(w) = seen.get_mut(f) {
            *w = x.code();
        }
        let mut g = 0usize;
        while g < f {
            if let Some(prev) = seen.get(g) {
                if *prev == x.code() {
                    return false;
                }
            }
            g += 1;
        }
        f += 1;
    }
    Fault::Ok.code() == 0
        && Fault::Stage.code() == SV_ESTAGE
        && Fault::Estate.code() == SV_ESTATE
        && Fault::Bal.code() == SV_EBAL
}

/// Verdef 结构闸：三节点全良，逐项破坏必判死。
fn verdef_ok() -> bool {
    let vs = verdefs();
    if vs.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < vs.len() {
        if let Some(v) = vs.get(i) {
            if !v.well_formed() {
                return false;
            }
            if v.aux_off < VERDEF_SIZE {
                return false;
            }
            if v.hash != elf_hash(v.name) {
                return false;
            }
        }
        i += 1;
    }
    let base = match vs.get(0) {
        Some(v) => *v,
        None => return false,
    };
    let v2 = match vs.get(2) {
        Some(v) => *v,
        None => return false,
    };
    if !base.is_base() || v2.is_base() {
        return false;
    }
    if !base.last() == false {
        return false;
    }
    if !v2.last() {
        return false;
    }
    if base.aux_name != base.name {
        return false;
    }

    // 逐项破坏：每坏一处都必须 well_formed() == false。
    let mut b = base;
    b.version = 2;
    if b.well_formed() {
        return false;
    }
    b = base;
    b.version = 1;
    b.ndx = 0;
    if b.well_formed() {
        return false;
    }
    b = base;
    b.ndx = 1;
    b.cnt = 0;
    if b.well_formed() {
        return false;
    }
    b = base;
    b.cnt = VERDEF_AUX_MAX + 1;
    if b.well_formed() {
        return false;
    }
    b = base;
    b.cnt = 1;
    b.aux_next = VERDAUX_SIZE;
    if b.well_formed() {
        return false;
    }
    b = base;
    b.cnt = 2;
    b.aux_next = 0;
    if b.well_formed() {
        return false;
    }
    b = base;
    b.cnt = 1;
    b.aux_next = 0;
    b.aux_off = VERDEF_SIZE - 1;
    if b.well_formed() {
        return false;
    }
    b = base;
    b.aux_off = VERDEF_SIZE;
    b.aux_name = "";
    if b.well_formed() {
        return false;
    }
    b = base;
    b.aux_name = "libx.so";
    b.hash = elf_hash("libx.so") ^ 1;
    if b.well_formed() {
        return false;
    }
    // 复原必须重新良好，且指纹可复算。
    b.hash = elf_hash(b.name);
    b.well_formed() && base.seal() == Verdef {
        name: base.name,
        version: base.version,
        flags: base.flags,
        ndx: base.ndx,
        cnt: base.cnt,
        hash: base.hash,
        aux_off: base.aux_off,
        next_off: base.next_off,
        aux_name: base.aux_name,
        aux_next: base.aux_next,
    }
    .seal()
}

/// 指令形态与语法闸：`@`/`@@`/脚本/无版本四形的 lex 边界。
fn directive_ok() -> bool {
    if Symver::None.versioned()
        || !Symver::At.versioned()
        || !Symver::AtAt.versioned()
        || !Symver::Script.versioned()
    {
        return false;
    }
    if Symver::None.sets_default()
        || Symver::At.sets_default()
        || !Symver::AtAt.sets_default()
        || Symver::Script.sets_default()
    {
        return false;
    }
    if Symver::None.text() != ""
        || Symver::At.text() != "@"
        || Symver::AtAt.text() != "@@"
        || Symver::Script.text() != "script"
    {
        return false;
    }

    // 无版本却带版本名 → 语法不整。
    let mut a = Shape::blank();
    a.sym = "foo";
    a.ver = "V1";
    a.dir = Symver::None;
    if a.lex_ok() {
        return false;
    }
    // 有版本却无版本名 → 语法不整。
    let mut b = Shape::blank();
    b.sym = "foo";
    b.dir = Symver::AtAt;
    if b.lex_ok() {
        return false;
    }
    // 符号名为空 → 语法不整（无论版本怎么写）。
    let mut c = Shape::blank();
    c.ver = "V1";
    c.dir = Symver::At;
    if c.lex_ok() {
        return false;
    }
    let d = Shape::blank();
    if d.lex_ok() {
        return false;
    }
    // 完整写法四形各一，且基节点判定只看 flags。
    let mut e = Shape::blank();
    e.sym = "foo";
    e.dir = Symver::None;
    let mut f = Shape::blank();
    f.sym = "foo";
    f.ver = "V1";
    f.dir = Symver::At;
    let mut g = Shape::blank();
    g.sym = "foo";
    g.ver = "V1";
    g.dir = Symver::AtAt;
    let mut h = Shape::blank();
    h.sym = "foo";
    h.ver = "V1";
    h.dir = Symver::Script;
    if !(e.lex_ok() && f.lex_ok() && g.lex_ok() && h.lex_ok()) {
        return false;
    }
    h.flags = VER_FLG_BASE;
    if !h.base() {
        return false;
    }
    h.flags = VER_FLG_BASE | VER_FLG_WEAK;
    if !h.base() {
        return false;
    }
    g.flags = VER_FLG_WEAK;
    if g.base() {
        return false;
    }
    // 指纹对形状字段敏感：改一个字段必换指纹。
    let s1 = e.seal();
    e.ver = "V2";
    let s2 = e.seal();
    e.ver = "";
    e.sym = "bar";
    let s3 = e.seal();
    s1 != s2 && s1 != s3 && s2 != s3
}

/// 定义纵线：七闸逐段推进，先短路的段不得被后段覆盖。
fn define_ok() -> bool {
    // 闸一：语法。
    let mut bad = Shape::blank();
    bad.sym = "foo";
    bad.dir = Symver::At;
    let o = define(&bad);
    if o.stage != Stage::Symver || o.code != ERR_EINVAL || o.note != Note::BadDirective {
        return false;
    }

    // 无版本符号：索引非 1 判 Index/BadIndex，索引 1 判 Ready/Unversioned 且可见。
    let mut unv = Shape::blank();
    unv.sym = "foo";
    unv.ndx = 2;
    let o = define(&unv);
    if o.stage != Stage::Index || o.code != ERR_EINVAL || o.note != Note::BadIndex || o.ok() {
        return false;
    }
    unv.ndx = VER_NDX_BASE;
    let o = define(&unv);
    if !o.ok() || o.note != Note::Unversioned || !o.default || o.ndx != VER_NDX_BASE {
        return false;
    }

    // 闸二：节点缺席在索引之前短路（ndx 即便合法也不该被采信）。
    let mut miss = Shape::blank();
    miss.sym = "foo";
    miss.ver = "V9";
    miss.dir = Symver::At;
    miss.node = NodeState::Missing;
    miss.cnt = 1;
    miss.aux_name = "V9";
    let o = define(&miss);
    if o.stage != Stage::Node || o.code != ERR_ENOENT || o.note != Note::NoNode || o.ndx != 0 {
        return false;
    }
    // 节点重号在索引段判死，且用 EEXIST 而非 EINVAL。
    let mut dup = miss;
    dup.node = NodeState::DupIndex;
    dup.ndx = 2;
    let o = define(&dup);
    if o.stage != Stage::Index || o.code != ERR_EEXIST || o.note != Note::BadIndex {
        return false;
    }

    // 闸三：索引边界。
    let mut at = Shape::blank();
    at.sym = "foo";
    at.ver = "V1";
    at.dir = Symver::At;
    at.cnt = 1;
    at.aux_name = "V1";
    let bounds = [0u16, 1, VER_NDX_MAX, VER_NDX_MAX + 1, 0x8000];
    let mut k = 0usize;
    while k < bounds.len() {
        let ndx = match bounds.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let mut s = at;
        s.ndx = ndx;
        let o = define(&s);
        if ndx >= 2 && ndx <= VER_NDX_MAX {
            if o.stage == Stage::Index {
                return false;
            }
        } else if o.stage != Stage::Index || o.note != Note::BadIndex {
            return false;
        }
        k += 1;
    }
    // 基节点（flags 带 BASE）允许占索引 1。
    let mut bs = at;
    bs.ndx = VER_NDX_BASE;
    bs.flags = VER_FLG_BASE;
    bs.ver = "libx.so";
    bs.aux_name = "libx.so";
    let o = define(&bs);
    if o.stage == Stage::Index {
        return false;
    }

    // 闸四：aux——计数、首 aux 名、aux 链、vd_next 四条各自短路。
    // 此后一律用合法索引，让形状能真正走到 aux 及其后的段。
    at.ndx = 2;
    let mut aux = at;
    aux.ndx = 2;
    aux.cnt = 0;
    if define(&aux).note != Note::BadAux || define(&aux).stage != Stage::Aux {
        return false;
    }
    aux.cnt = VERDEF_AUX_MAX + 1;
    if define(&aux).note != Note::BadAux {
        return false;
    }
    aux.cnt = 1;
    aux.aux_name = "V2";
    if define(&aux).note != Note::BadAux {
        return false;
    }
    aux.aux_name = "V1";
    aux.aux_next = VERDAUX_SIZE;
    if define(&aux).note != Note::BadAux {
        return false;
    }
    let mut aux2 = at;
    aux2.ndx = 2;
    aux2.cnt = 2;
    aux2.aux_next = 0;
    if define(&aux2).note != Note::BadAux {
        return false;
    }
    aux2.cnt = 1;
    aux2.aux_next = 0;
    aux2.next_off = 48;
    if define(&aux2).note != Note::BadChain {
        return false;
    }
    // aux 名与版本名一致时放行。
    aux2.next_off = 0;
    if define(&aux2).stage == Stage::Aux {
        return false;
    }

    // 闸五：hash 只在 aux 全通过后到达。
    let mut hs = aux2;
    hs.hash_ok = false;
    let o = define(&hs);
    if o.stage != Stage::Hash || o.note != Note::BadHash || o.code != ERR_EINVAL {
        return false;
    }
    hs.hash_ok = true;
    if define(&hs).stage == Stage::Hash {
        return false;
    }

    // `@` 与 `@@` 的分野：前者默认不可见，后者默认可见。
    let non = define(&at.clone_ndx(2));
    if non.note != Note::NonDefault || non.default || !non.ok() {
        return false;
    }
    let mut dft = at;
    dft.dir = Symver::AtAt;
    let o = define(&dft);
    if o.note != Note::Ready || !o.default || !o.ok() || o.ndx != 2 {
        return false;
    }
    // 隐藏引用（`@` 且 hidden）落 Hidden 注解，仍属成功档。
    let mut hid = at;
    hid.hidden = true;
    let o = define(&hid);
    if o.note != Note::Hidden || !Tier::of(&o).success() || o.default {
        return false;
    }
    // 版本脚本：基节点归对象，非基节点默认可见。
    let mut sc = at;
    sc.dir = Symver::Script;
    let o = define(&sc);
    if o.note != Note::ScriptBound || !o.default || o.ndx != 2 {
        return false;
    }
    sc.flags = VER_FLG_BASE;
    sc.ver = "libx.so";
    sc.aux_name = "libx.so";
    sc.ndx = VER_NDX_BASE;
    let o = define(&sc);
    if o.note != Note::Base || o.default || o.ndx != VER_NDX_BASE {
        return false;
    }
    // 指纹：同形同指纹，异形异指纹。
    let x = define(&at);
    let y = define(&at);
    x.seal() == y.seal() && x.seal() != o.seal()
}

/// 默认规则：一符一默认，基节点不得为默认。
fn default_ok() -> bool {
    // 已有默认 → `@@` 第二个必判冲突。
    let mut s = match case_at(1) {
        Some(c) => c.shape,
        None => return false,
    };
    s.has_default = true;
    let o = define(&s);
    if o.stage != Stage::Default || o.note != Note::DuplicateDefault || o.code != ERR_EEXIST {
        return false;
    }
    if !o.note.conflict() || Tier::of(&o) != Tier::Conflict {
        return false;
    }
    // 基节点 `@@` → BaseDefault，且与 DuplicateDefault 分属不同文案。
    let mut b = match case_at(7) {
        Some(c) => c.shape,
        None => return false,
    };
    b.has_default = false;
    let o = define(&b);
    if o.note != Note::BaseDefault || o.stage != Stage::Default || o.code != ERR_EINVAL {
        return false;
    }
    if !o.note.conflict() || message(&b, &o) == message(&s, &define(&s)) {
        return false;
    }
    // 基节点 + 已有默认：基节点判定必须在冲突之前（先问"能不能当默认"）。
    b.has_default = true;
    if define(&b).note != Note::BaseDefault {
        return false;
    }
    // 非默认写法（`@`）不受 has_default 影响。
    let mut n = s;
    n.dir = Symver::At;
    n.has_default = true;
    let o = define(&n);
    o.ok() && o.note == Note::NonDefault && !o.default
        && match case_at(2) {
            Some(c) => c.actual().note == Note::NonDefault,
            None => false,
        }
}

/// 申报对判：判序与六维逐维独立可检。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let k = c.claim;
        let f = judge(&c);
        match expected(i) {
            Some(w) => {
                if f != w {
                    return false;
                }
            }
            None => return false,
        }
        if i < ACCEPT {
            if f != Fault::Ok {
                return false;
            }
            if !(a.stage == k.stage
                && a.code == k.code
                && a.ndx == k.value
                && a.note == k.note
                && a.default == k.default)
            {
                return false;
            }
        } else if f == Fault::Ok {
            return false;
        }
        // 逐维单点注入：只改一维必须落到该维对应缺陷。
        // 索引维对全部成功槽成立；阶段维只对失败槽成立——把失败实际申报成
        // Ready 会先撞上状态维（成败互报优先），那正是 Estate 的定义。
        if i < ACCEPT {
            let mut c2 = c;
            c2.claim.value = a.ndx.saturating_add(7);
            if judge(&c2) != Fault::Value {
                return false;
            }
            if a.stage != Stage::Ready {
                let mut c1 = c;
                c1.claim.stage = Stage::Index;
                if judge(&c1) != Fault::Stage {
                    return false;
                }
            }
        }
        i += 1;
    }
    // 状态互报必须优先：成功实际 + 失败申报 → Estate（不是 Stage/Code）。
    let miss = match case_at(8) {
        Some(c) => c,
        None => return false,
    };
    if miss.actual().ok() || judge(&miss) != Fault::Estate {
        return false;
    }
    let mut rev = miss;
    rev.claim = Claim { stage: Stage::Ready, code: 0, value: 1, note: Note::Ready, default: true };
    judge(&rev) == Fault::Estate
}

/// 矩阵统计：分档覆盖、红档只出现在申报错的那一维。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    if m.slots() != CORPUS || m.goods() != ACCEPT {
        return false;
    }
    if !m.covered() {
        return false;
    }
    let live = match m.row(Tier::Live) {
        Some(r) => r,
        None => return false,
    };
    let hidden = match m.row(Tier::Hidden) {
        Some(r) => r,
        None => return false,
    };
    let malformed = match m.row(Tier::Malformed) {
        Some(r) => r,
        None => return false,
    };
    let conflict = match m.row(Tier::Conflict) {
        Some(r) => r,
        None => return false,
    };
    if live.total != 3 || live.good != 3 {
        return false;
    }
    if hidden.total != 1 || hidden.good != 1 {
        return false;
    }
    if conflict.total != 2 || conflict.good != 2 {
        return false;
    }
    // 六格复刻形状全部是 missing-node，实际档同落 malformed，红档数恰为 6。
    if malformed.total != 8 || malformed.good != 2 || malformed.red() != 6 {
        return false;
    }
    if m.perfect_rows() != 3 {
        return false;
    }
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r.total != CORPUS as u32
        || r.accept != ACCEPT as u32
        || r.reject != REJECT as u32
        || r.live != live.good as u32
        || r.hidden != hidden.good as u32
        || r.malformed != malformed.good as u32
        || r.conflict != conflict.good as u32
    {
        return false;
    }
    if r.matched != r.total || !r.green() || r.gate() != Ok(()) {
        return false;
    }
    if r.success_cells() + r.failure_cells() != r.accept {
        return false;
    }
    // 注入一槽缺陷后：matched 必降、rate 必离 1000、gate 必报计数错。
    let red = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    red.matched == r.matched - 1
        && red.rate() == 929
        && red.gate() == Err(SV_ECOUNT)
        && !red.green()
        && run_with(Some((CORPUS, Fault::Code))).is_err()
        && m.row(Tier::Live).map(|x| x.render(Tier::Live)).is_some()
}

/// 语料：14 槽、标签唯一、渲染逐字、越界无观测。
fn corpus_ok() -> bool {
    let cs = cases();
    if cs.len() != CORPUS || case_at(CORPUS).is_some() || case_at(0).is_none() {
        return false;
    }
    if cover(CORPUS) != Ok(()) || cover(CORPUS - 1) != Err(SV_ENOBS) {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if let Some(w) = WANT_TAGS.get(i) {
            if c.tag() != *w {
                return false;
            }
        } else {
            return false;
        }
        let mut k = i + 1;
        while k < CORPUS {
            if let Some(o) = cs.get(k) {
                if o.tag() == c.tag() {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    if ACCEPT_SLOTS.len() != ACCEPT || REJECT_SLOTS.len() != REJECT {
        return false;
    }
    let mut seen = [false; CORPUS];
    let mut a = 0usize;
    while a < ACCEPT {
        if let Some(s) = ACCEPT_SLOTS.get(a) {
            if *s >= CORPUS || seen[*s] {
                return false;
            }
            seen[*s] = true;
        }
        a += 1;
    }
    let mut b = 0usize;
    while b < REJECT {
        if let Some(s) = REJECT_SLOTS.get(b) {
            if *s >= CORPUS || seen[*s] {
                return false;
            }
            seen[*s] = true;
        }
        b += 1;
    }
    let mut z = 0usize;
    while z < CORPUS {
        if let Some(v) = seen.get(z) {
            if !*v {
                return false;
            }
        }
        z += 1;
    }
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    // 红档率必须是账本最后一格的申报值：注入一槽缺陷后 13/14 → 929‰。
    let red_rate = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v.rate(),
        Err(_) => return false,
    };
    let last_val = match VALS.last() {
        Some(v) => *v,
        None => return false,
    };
    r.render() == RENDER_OK && red_rate == 929 && last_val == red_rate
}

/// 渲染全文（全绿一跑）。
const RENDER_OK: &str = "symver corpus=14 accept=8 reject=6 live=3 hidden=1 malformed=2 conflict=2 match=14 rate=1000ppm\n";

/// 红档显式：六格各自挂在哪一维，缺陷码互不相同。
fn reds_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        let c = match cs.get(i + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let f = judge(&c);
        match REJECT_FAULTS.get(i) {
            Some(w) => {
                if f != *w {
                    return false;
                }
            }
            None => return false,
        }
        match REJECT_CODES.get(i) {
            Some(w) => {
                if f.code() != *w {
                    return false;
                }
            }
            None => return false,
        }
        if f == Fault::Ok || !f.alive() {
            return false;
        }
        // 六格实际结果全同（复刻自 missing-node），差别只在申报——这正是
        // "红档显式"的立卡理由：同一现象能被六种不同方式误报。
        if let Some(base) = cs.get(4) {
            if c.actual().seal() != base.actual().seal() {
                return false;
            }
        }
        let mut k = 0usize;
        while k < i {
            if let Some(prev) = REJECT_CODES.get(k) {
                if *prev == f.code() {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    // 六格六维：期望表与申报表逐维比较，各只错一维。
    let want = match cs.get(4) {
        Some(c) => c.claim,
        None => return false,
    };
    let mut diffs = 0usize;
    let mut j = 0usize;
    while j < REJECT {
        let c = match cs.get(j + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let mut n = 0usize;
        if c.claim.stage != want.stage {
            n += 1;
        }
        if c.claim.code != want.code {
            n += 1;
        }
        if c.claim.value != want.value {
            n += 1;
        }
        if c.claim.note != want.note {
            n += 1;
        }
        if c.claim.default != want.default {
            n += 1;
        }
        if c.claim.success() != want.success() {
            n += 1;
        }
        if n == 0 {
            return false;
        }
        diffs += 1;
        j += 1;
    }
    diffs == REJECT
}

/// 期望表逐槽核对（把语料用第二种形式再判一遍）。
fn slots_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let st = match WANT_STAGE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let code = match WANT_CODE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let ndx = match WANT_NDX.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let note = match WANT_NOTE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let def = match WANT_DEFAULT.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.stage != st || a.code != code || a.ndx != ndx || a.note != note || a.default != def {
            return false;
        }
        // 期望表自身的一致性：成功阶段必须零 errno，反之亦然。
        if (a.stage == Stage::Ready) != (a.code == 0) {
            return false;
        }
        i += 1;
    }
    // 后六槽申报各自偏离实际的那一维，与 REJECT_FAULTS 一一对应。
    let mut t = 0usize;
    while t < REJECT {
        let c = match cs.get(t + ACCEPT) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let hit = match REJECT_FAULTS.get(t) {
            Some(Fault::Estate) => a.ok() != c.claim.success(),
            Some(Fault::Stage) => a.ok() == c.claim.success() && a.stage != c.claim.stage,
            Some(Fault::Code) => {
                a.stage == c.claim.stage && a.code != c.claim.code
            }
            Some(Fault::Value) => {
                a.stage == c.claim.stage && a.code == c.claim.code && a.ndx != c.claim.value
            }
            Some(Fault::Note) => {
                a.ndx == c.claim.value && a.note != c.claim.note
            }
            Some(Fault::Default) => {
                a.note == c.claim.note && a.default != c.claim.default
            }
            _ => false,
        };
        if !hit {
            return false;
        }
        t += 1;
    }
    case_at(CORPUS).is_none() && case_at(4).map(|c| c.tag()) == Some("missing-node")
}

/// 文案逐字：八条 GNU ld / 自研文案原文。
fn message_ok() -> bool {
    if msg_no_node("foo", "V9") != "version node not found for symbol foo@V9" {
        return false;
    }
    if msg_dup_default("foo", "V2", "V1") != "foo: multiple default versions: V2 and V1" {
        return false;
    }
    if msg_base_default("foo", "libx.so")
        != "cannot make base version `libx.so' the default for symbol foo"
    {
        return false;
    }
    if msg_bad_index("foo", 0) != "invalid version index 0 for symbol foo" {
        return false;
    }
    if msg_bad_aux("V1") != "version definition V1: bad aux count" {
        return false;
    }
    if msg_bad_chain("V2") != "version definition V2: broken vd_next chain" {
        return false;
    }
    if msg_bad_directive("foo") != "invalid .symver directive for symbol foo" {
        return false;
    }
    let w = elf_hash("V3");
    let g = w ^ 0xffff_ffff;
    let mut want = String::from("version definition V3: hash mismatch (want 0x");
    want.push_str(hex_text(w, 8).as_str());
    want.push_str(", got 0x");
    want.push_str(hex_text(g, 8).as_str());
    want.push(')');
    if msg_bad_hash("V3", w, g) != want.as_str() {
        return false;
    }
    // message() 只在失败注解上有文案，成功注解一律空串。
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        let m = message(&c.shape, &a);
        if a.note.success() {
            if !m.is_empty() {
                return false;
            }
        } else if m.is_empty() {
            return false;
        }
        i += 1;
    }
    // 六复刻格同现象同文案：红档差异只在申报，不在现场。
    let a4 = match cs.get(4) {
        Some(c) => message(&c.shape, &c.actual()),
        None => return false,
    };
    let a13 = match cs.get(13) {
        Some(c) => message(&c.shape, &c.actual()),
        None => return false,
    };
    a4 == a13 && !a4.is_empty()
}

/// 无版本槽释义。
const DETAIL_UNVERSIONED: &str = "foo stage=ready code=0 ndx=1 default=true note=unversioned\n";

/// 节点缺席释义（含 GNU 原文）。
const DETAIL_NO_NODE: &str = "foo stage=node code=2 ndx=0 default=false note=no-node msg=\"version node not found for symbol foo@V9\"\n";

/// 基节点当默认释义。
const DETAIL_BASE_DEFAULT: &str = "foo stage=default code=22 ndx=1 default=false note=base-default msg=\"cannot make base version `libx.so' the default for symbol foo\"\n";

/// 双默认冲突释义。
const DETAIL_DUP_DEFAULT: &str =
    "foo stage=default code=17 ndx=2 default=false note=dup-default msg=\"foo: multiple default versions: V2 and V1\"\n";

/// 释义逐字 + 账本形状。
fn detail_ok() -> bool {
    let cs = cases();
    let d0 = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_UNVERSIONED,
        None => false,
    };
    if !d0 {
        return false;
    }
    let d4 = match cs.get(4) {
        Some(c) => detail(c).as_str() == DETAIL_NO_NODE,
        None => false,
    };
    if !d4 {
        return false;
    }
    let d7 = match cs.get(7) {
        Some(c) => detail(c).as_str() == DETAIL_BASE_DEFAULT,
        None => false,
    };
    if !d7 {
        return false;
    }
    let d5 = match cs.get(5) {
        Some(c) => detail(c).as_str() == DETAIL_DUP_DEFAULT,
        None => false,
    };
    if !d5 {
        return false;
    }
    // 成功档释义不含 msg=，失败档必含；行尾必为换行。
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let s = detail(&c);
        if !s.ends_with('\n') {
            return false;
        }
        let has_msg = s.contains("msg=");
        if c.actual().note.success() == has_msg {
            return false;
        }
        i += 1;
    }
    // 三档可见性：default=true 只出现在 live 档与无版本档。
    let mut vis = 0usize;
    let mut j = 0usize;
    while j < ACCEPT {
        if let Some(c) = cs.get(j) {
            if c.actual().visible() {
                vis += 1;
            }
        }
        j += 1;
    }
    vis == 3 && match cs.get(2) {
        Some(c) => !c.actual().visible() && c.actual().ok(),
        None => false,
    }
}

/// 阶段闸门：单调、回退拒、关闭后拒走。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.steps() != 0 || g.closed() || g.last() != -1 {
        return false;
    }
    if g.step(Stage::Aux) != Ok(()) || g.steps() != 1 || g.last() != 3 {
        return false;
    }
    // 回退到更早的段必须短路为 ESTAGE（阶段申报不符）。
    if g.step(Stage::Node) != Err(SV_ESTAGE) || g.steps() != 1 {
        return false;
    }
    if g.step(Stage::Default) != Ok(()) || g.last() != 5 {
        return false;
    }
    // 同段重复允许（幂等观测），但不得倒退。
    if g.step(Stage::Default) != Ok(()) || g.steps() != 3 {
        return false;
    }
    if g.step(Stage::Ready) != Ok(()) {
        return false;
    }
    if g.close() != Ok(4) || !g.closed() {
        return false;
    }
    if g.close() != Err(SV_ESTATE) {
        return false;
    }
    // 关闭之后任何一段都不许再走。
    g.step(Stage::Ready) == Err(SV_ESTATE)
}

/// 账本：14 格 8 成 6 败，逐格对账，越界与冻结拒登记。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.len() != SAMPLES || l.ok_count() != ACCEPT || l.err_count() != REJECT {
        return false;
    }
    if l.audit() != Ok(()) || l.sample(SAMPLES).is_some() || l.is_empty() {
        return false;
    }
    if l.rejects() != 0 {
        return false;
    }
    // 成格值必须逐格等于 VALS，败格码必须逐格等于 REJECT_CODES。
    let mut i = 0usize;
    while i < ACCEPT {
        let s = match l.sample(i) {
            Some(v) => v,
            None => return false,
        };
        match VALS.get(i) {
            Some(w) => {
                if s.value != *w || !s.ok || s.code != 0 || s.slot as usize != i {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        let s = match l.sample(ACCEPT + j) {
            Some(v) => v,
            None => return false,
        };
        match REJECT_CODES.get(j) {
            Some(w) => {
                if s.code != *w || s.ok || s.value != 0 {
                    return false;
                }
            }
            None => return false,
        }
        j += 1;
    }
    // 槽位不连续必须被拒。
    let mut m = Ledger::new();
    if m.register(Sample { slot: 1, op: OP_NODE, ok: true, code: 0, value: 1 }).is_ok() {
        return false;
    }
    if m.rejects() != 1 || m.len() != 0 {
        return false;
    }
    if m.register(Sample { slot: 0, op: OP_NODE, ok: true, code: 0, value: 1 }).is_err() {
        return false;
    }
    if m.audit() == Ok(()) {
        return false;
    }
    let mut e = Ledger::new();
    if e.register(Sample { slot: SAMPLES as u8, op: OP_AUX, ok: false, code: SV_EBAL, value: 0 })
        .is_ok()
    {
        return false;
    }
    e.rejects() == 1 && m.len() == 1
}

// ---------------------------------------------------------------------------
// 判据集
// ---------------------------------------------------------------------------

/// 13 条判据。
pub fn run_unx11657_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11657_symver_verdef");

    s.add("SV01-名称与段基址", name_ok(), "static");

    s.add("SV02-枚举与纵线", enums_ok(), "static");

    s.add("SV03-Verdef 结构闸", verdef_ok(), "static");

    s.add("SV04-指令形态与语法", directive_ok(), "static");

    s.add("SV05-定义七闸", define_ok(), "static");

    s.add("SV06-默认规则", default_ok(), "static");

    s.add("SV07-申报对判", judge_ok(), "static");

    s.add("SV08-矩阵统计", matrix_ok(), "static");

    s.add("SV09-语料与渲染", corpus_ok(), "static");

    s.add("SV10-红档显式", reds_ok() && slots_ok(), "static");

    s.add("SV11-文案逐字", message_ok(), "static");

    s.add("SV12-释义与账本", detail_ok() && ledger_ok() && gate_ok(), "static");

    s.add(
        "SV13-冻结重放四注入",
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

    #[test]
    fn seg_and_sizes() {
        assert_eq!(SEG_BASE, 0x6cea0);
        assert_eq!(SV_ESTAGE, SEG_BASE + 0x01);
        assert_eq!(SV_EBAL, SEG_BASE + 0x0d);
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(VERDEF_SIZE, 20);
        assert_eq!(VERDAUX_SIZE, 8);
        assert!(name_ok());
    }

    #[test]
    fn elf_hash_is_stable_and_distinct() {
        assert_eq!(elf_hash("V1"), elf_hash("V1"));
        assert_ne!(elf_hash("V1"), elf_hash("V2"));
        assert_ne!(elf_hash("libx.so"), elf_hash("V1"));
        assert_eq!(elf_hash(""), 0);
    }

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fold_str(FNV_OFF, "V1"), fold_str(FNV_OFF, "V1"));
        assert_ne!(fold_str(FNV_OFF, "V1"), fold_str(FNV_OFF, "V2"));
        assert_ne!(fold_word(FNV_OFF, 1), fold_word(FNV_OFF, 2));
        assert_ne!(fold_byte(FNV_OFF, 7), fold_byte(FNV_OFF, 8));
    }

    #[test]
    fn number_text_helpers() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(929).as_str(), "929");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(i32_text(-22).as_str(), "-22");
        assert_eq!(i32_text(0).as_str(), "0");
        assert_eq!(hex_text(0x6cea0, 5).as_str(), "6cea0");
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
    }

    #[test]
    fn stage_order_and_names() {
        assert_eq!(Stage::Symver.order(), 0);
        assert_eq!(Stage::Ready.order(), 6);
        assert_eq!(Stage::Aux.name(), "aux");
        assert!(Stage::Node.failure());
        assert!(!Stage::Ready.failure());
        assert!(enums_ok());
    }

    #[test]
    fn note_partition_is_disjoint() {
        assert!(Note::Ready.success());
        assert!(!Note::Ready.malformed());
        assert!(Note::NoNode.malformed());
        assert!(!Note::NoNode.conflict());
        assert!(Note::DuplicateDefault.conflict());
        assert!(!Note::DuplicateDefault.success());
        assert_eq!(Note::BaseDefault.name(), "base-default");
    }

    #[test]
    fn symver_forms() {
        assert!(!Symver::None.versioned());
        assert!(Symver::AtAt.sets_default());
        assert!(!Symver::At.sets_default());
        assert_eq!(Symver::Script.text(), "script");
        assert_eq!(Symver::AtAt.text(), "@@");
        assert!(directive_ok());
    }

    #[test]
    fn verdefs_are_well_formed() {
        let vs = verdefs();
        assert_eq!(vs.len(), 3);
        for v in vs.iter() {
            assert!(v.well_formed());
            assert_eq!(v.hash, elf_hash(v.name));
        }
        assert!(vs[0].is_base());
        assert!(!vs[1].is_base());
        assert!(vs[2].last());
        assert!(!vs[0].last());
        assert!(verdef_ok());
    }

    #[test]
    fn verdef_breaks_are_caught() {
        let mut v = verdefs()[0];
        v.hash ^= 7;
        assert!(!v.well_formed());
        v = verdefs()[0];
        v.cnt = 9;
        assert!(!v.well_formed());
        v = verdefs()[0];
        v.aux_off = 12;
        assert!(!v.well_formed());
    }

    #[test]
    fn unversioned_shape() {
        let c = case_at(0).expect("slot0");
        let a = c.actual();
        assert_eq!(a.stage, Stage::Ready);
        assert_eq!(a.code, 0);
        assert_eq!(a.ndx, VER_NDX_BASE);
        assert!(a.default);
        assert_eq!(a.note, Note::Unversioned);
        assert!(a.ok());
        assert_eq!(judge(&c), Fault::Ok);
        assert!(detail(&c).as_str() == DETAIL_UNVERSIONED);
    }

    #[test]
    fn default_double_at_is_live() {
        let c = case_at(1).expect("slot1");
        let a = c.actual();
        assert_eq!(a.note, Note::Ready);
        assert!(a.visible());
        assert_eq!(Tier::of(&a), Tier::Live);
        assert_eq!(a.ndx, 2);
    }

    #[test]
    fn nondefault_at_is_hidden_tier() {
        let c = case_at(2).expect("slot2");
        let a = c.actual();
        assert_eq!(a.note, Note::NonDefault);
        assert!(!a.default);
        assert!(!a.visible());
        assert_eq!(Tier::of(&a), Tier::Hidden);
    }

    #[test]
    fn script_binding_is_visible() {
        let c = case_at(3).expect("slot3");
        let a = c.actual();
        assert_eq!(a.note, Note::ScriptBound);
        assert!(a.default);
        assert_eq!(a.ndx, 3);
        assert_eq!(Tier::of(&a), Tier::Live);
    }

    #[test]
    fn missing_node_shorts_at_node_gate() {
        let c = case_at(4).expect("slot4");
        let a = c.actual();
        assert_eq!(a.stage, Stage::Node);
        assert_eq!(a.code, ERR_ENOENT);
        assert_eq!(a.note, Note::NoNode);
        assert_eq!(a.ndx, 0);
        assert!(a.failed());
        assert!(detail(&c).as_str() == DETAIL_NO_NODE);
    }

    #[test]
    fn duplicate_default_is_conflict() {
        let c = case_at(5).expect("slot5");
        let a = c.actual();
        assert_eq!(a.stage, Stage::Default);
        assert_eq!(a.code, ERR_EEXIST);
        assert_eq!(a.note, Note::DuplicateDefault);
        assert_eq!(Tier::of(&a), Tier::Conflict);
        assert!(detail(&c).as_str() == DETAIL_DUP_DEFAULT);
    }

    #[test]
    fn bad_hash_reaches_hash_gate() {
        let c = case_at(6).expect("slot6");
        let a = c.actual();
        assert_eq!(a.stage, Stage::Hash);
        assert_eq!(a.note, Note::BadHash);
        assert_eq!(a.code, ERR_EINVAL);
        assert_eq!(a.ndx, 4);
        let m = message(&c.shape, &a);
        assert!(m.contains("hash mismatch"));
    }

    #[test]
    fn base_node_cannot_be_default() {
        let c = case_at(7).expect("slot7");
        let a = c.actual();
        assert_eq!(a.note, Note::BaseDefault);
        assert_eq!(a.stage, Stage::Default);
        assert_eq!(a.ndx, VER_NDX_BASE);
        assert_eq!(Tier::of(&a), Tier::Conflict);
        assert!(detail(&c).as_str() == DETAIL_BASE_DEFAULT);
    }

    #[test]
    fn six_reds_are_six_dimensions() {
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        let mut i = 0usize;
        while i < REJECT {
            let c = case_at(i + ACCEPT).expect("red slot");
            assert_eq!(judge(&c), *REJECT_FAULTS.get(i).expect("fault"));
            assert_eq!(judge(&c).code(), *REJECT_CODES.get(i).expect("code"));
            i += 1;
        }
        assert!(reds_ok());
        assert!(slots_ok());
    }

    #[test]
    fn estate_beats_stage_and_code() {
        let c = case_at(8).expect("slot8");
        assert!(c.actual().failed());
        assert!(c.claim.success());
        assert_eq!(judge(&c), Fault::Estate);
        assert_eq!(judge(&c).code(), SV_ESTATE);
    }

    #[test]
    fn report_is_all_green() {
        let r = run().expect("report");
        assert_eq!(r.total, 14);
        assert_eq!(r.accept, 8);
        assert_eq!(r.reject, 6);
        assert_eq!(r.live, 3);
        assert_eq!(r.hidden, 1);
        assert_eq!(r.malformed, 2);
        assert_eq!(r.conflict, 2);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert!(r.green());
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(r.render().as_str(), RENDER_OK);
    }

    #[test]
    fn injected_slot_lowers_rate() {
        let r = run_with(Some((0, Fault::Code))).expect("red report");
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(*VALS.last().expect("last val"), 929);
        assert!(!r.green());
        assert_eq!(r.gate(), Err(SV_ECOUNT));
        assert!(run_with(Some((CORPUS, Fault::Ok))).is_err());
    }

    #[test]
    fn matrix_rows_are_balanced() {
        let m = Matrix::of();
        assert_eq!(m.slots(), CORPUS);
        assert_eq!(m.goods(), ACCEPT);
        assert!(m.covered());
        assert_eq!(m.perfect_rows(), 3);
        let bad = m.row(Tier::Malformed).expect("row");
        assert_eq!(bad.total, 8);
        assert_eq!(bad.good, 2);
        assert_eq!(bad.red(), 6);
        assert!(m.render().contains("live total=3 good=3 red=0"));
    }

    #[test]
    fn gate_is_monotonic() {
        let mut g = Gate::new();
        assert_eq!(g.last(), -1);
        assert_eq!(g.step(Stage::Ready), Ok(()));
        assert_eq!(g.step(Stage::Symver), Err(SV_ESTAGE));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.close(), Ok(1));
        assert_eq!(g.close(), Err(SV_ESTATE));
        assert!(gate_ok());
    }

    #[test]
    fn ledger_shape_is_fourteen_by_eight_six() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.audit(), Ok(()));
        assert!(!l.is_empty());
        assert!(l.sample(SAMPLES).is_none());
        assert!(ledger_ok());
    }

    #[test]
    fn ledger_rejects_out_of_order_slot() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        assert_eq!(
            l.register(Sample { slot: 5, op: OP_INDEX, ok: true, code: 0, value: 1 }),
            Err(SV_ESLOT)
        );
        assert_eq!(l.rejects(), 1);
        assert_eq!(
            l.register(Sample { slot: 0, op: OP_INDEX, ok: true, code: 0, value: 14 }),
            Ok(())
        );
        assert_eq!(l.sample(0).expect("s0").value, 14);
        assert_eq!(l.audit(), Err(SV_EBAL));
    }

    #[test]
    fn ledger_freeze_replays_same_root() {
        let mut l = build_ledger();
        let r1 = l.freeze().expect("freeze");
        assert_ne!(r1, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), r1);
        assert_eq!(l.replay(), Ok(r1));
        assert_eq!(l.freeze(), Err(SV_EBAL));
        assert_eq!(
            l.register(Sample { slot: 0, op: OP_NODE, ok: true, code: 0, value: 1 }),
            Err(SV_ESLOT)
        );
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn four_injections_all_caught() {
        let i = inject_four();
        assert!(i[0], "表路退化");
        assert!(i[1], "判路退化");
        assert!(i[2], "渲染路退化");
        assert!(i[3], "账本路退化");
    }

    #[test]
    fn corpus_bounds_and_cover() {
        assert_eq!(cases().len(), CORPUS);
        assert!(case_at(0).is_some());
        assert!(case_at(CORPUS - 1).is_some());
        assert!(case_at(CORPUS).is_none());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(13), Err(SV_ENOBS));
    }

    #[test]
    fn tags_match_expectation_table() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = case_at(i).expect("case");
            assert_eq!(c.tag(), *WANT_TAGS.get(i).expect("tag"));
            i += 1;
        }
    }

    #[test]
    fn messages_are_verbatim() {
        assert_eq!(
            msg_no_node("foo", "V9").as_str(),
            "version node not found for symbol foo@V9"
        );
        assert_eq!(
            msg_dup_default("foo", "V2", "V1").as_str(),
            "foo: multiple default versions: V2 and V1"
        );
        assert_eq!(
            msg_base_default("foo", "libx.so").as_str(),
            "cannot make base version `libx.so' the default for symbol foo"
        );
        assert_eq!(msg_bad_index("foo", 0).as_str(), "invalid version index 0 for symbol foo");
        assert_eq!(msg_bad_aux("V1").as_str(), "version definition V1: bad aux count");
        assert_eq!(
            msg_bad_chain("V2").as_str(),
            "version definition V2: broken vd_next chain"
        );
        assert_eq!(
            msg_bad_directive("foo").as_str(),
            "invalid .symver directive for symbol foo"
        );
        assert!(message_ok());
    }

    #[test]
    fn success_notes_have_no_message() {
        let mut i = 0usize;
        while i < ACCEPT {
            let c = case_at(i).expect("case");
            let a = c.actual();
            if a.note.success() {
                assert!(message(&c.shape, &a).is_empty());
            }
            i += 1;
        }
        let c = case_at(4).expect("case");
        assert!(!message(&c.shape, &c.actual()).is_empty());
    }

    #[test]
    fn hidden_reference_is_a_success_tier() {
        let mut s = match case_at(2) {
            Some(c) => c.shape,
            None => panic!("slot2"),
        };
        s.hidden = true;
        let o = define(&s);
        assert!(o.ok());
        assert_eq!(o.note, Note::Hidden);
        assert!(!o.default);
        assert_eq!(Tier::of(&o), Tier::Hidden);
    }

    #[test]
    fn aux_chain_consistency() {
        let mut s = match case_at(1) {
            Some(c) => c.shape,
            None => panic!("slot1"),
        };
        s.cnt = 2;
        s.aux_next = 0;
        assert_eq!(define(&s).note, Note::BadAux);
        s.cnt = 1;
        s.aux_next = VERDAUX_SIZE;
        assert_eq!(define(&s).note, Note::BadAux);
        s.aux_next = 0;
        s.next_off = 48;
        assert_eq!(define(&s).note, Note::BadChain);
    }

    #[test]
    fn index_bounds_reject_out_of_range() {
        let mut s = match case_at(1) {
            Some(c) => c.shape,
            None => panic!("slot1"),
        };
        s.ndx = VER_NDX_MAX + 1;
        assert_eq!(define(&s).note, Note::BadIndex);
        s.ndx = 1;
        assert_eq!(define(&s).note, Note::BadIndex);
        s.flags = VER_FLG_BASE;
        s.ver = "libx.so";
        s.aux_name = "libx.so";
        assert_ne!(define(&s).note, Note::BadIndex);
    }

    #[test]
    fn seal_is_field_sensitive() {
        let a = verdefs()[1].seal();
        let mut b = verdefs()[1];
        b.cnt = 2;
        assert_ne!(a, b.seal());
        let c = match case_at(1) {
            Some(x) => x.actual().seal(),
            None => panic!("slot1"),
        };
        let d = match case_at(2) {
            Some(x) => x.actual().seal(),
            None => panic!("slot2"),
        };
        assert_ne!(c, d);
    }

    #[test]
    fn all_thirteen_checks_pass() {
        let s = run_unx11657_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}