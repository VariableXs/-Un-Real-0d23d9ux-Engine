//! UNX-F11656 弱符号缺版本回退判据
//!
//! 动态符号解析里有一类引用天生"允许失败"：`STB_WEAK` 的未定义符号。链接器
//! 允许它没有定义，装载器在查不到时把它绑成地址 0，而不是中止进程。版本机制
//! 叠上来之后，这条规则被切成两半：
//!
//! - **弱符号 + 缺符号**：查不到定义，绑 0，静默成功。
//! - **弱符号 + 缺版本节点**：版本需求本身不可满足，但因为引用是弱绑定，装载
//!   器降级为按名字查找；找到默认/无版本定义就绑上去，找不到仍然绑 0。
//! - **强符号 + 缺版本节点**：硬错误，`version `V' not found`。
//!
//! 最容易写错的是把"弱"当成一个开关：只要 `st_info` 里是 `STB_WEAK` 就整条
//! 链路都宽容。实际不是。版本节点的 `VER_FLG_WEAK`（vernaux 的 `vna_flags`
//! 第 1 位）与符号绑定 `STB_WEAK` 是两个独立维度，本卡把它们分成两列：
//!
//! - `req_weak`：版本节点自带弱标记，缺节点只告警、继续解析。
//! - `bind`：符号引用本身的绑定强度，查不到定义时决定 0 还是硬错误。
//!
//! 二者可以任意组合，四种组合的落点各不相同。判据必须把这四种都钉住，否则
//! "弱符号"三个字会把版本层的错误一起吞掉。
//!
//! 解析纵线（本卡的闸门次序）：
//!
//! 1. `parse`：对象名/符号名/versym 索引合法。局部索引（0）不允许强引用。
//! 2. `version`：索引 ≥ 2 时查版本节点；缺节点且无任何弱标记 → 硬错误。
//! 3. `lookup`：按作用域查定义；`undef` 与 `local`（hidden）都算查不到。
//! 4. `bind`：按版本索引与定义版本种类决定命中、错配还是回退。
//! 5. `fallback`：弱回退落点——绑 0（NULL）或绑缺省定义。
//! 6. `ready`：返回地址与注解。
//!
//! 回退不是"再查一次"：它是版本层已经放行之后，符号层按**名字**取默认定义。
//! 若定义只有非默认版本（`@`），无版本引用看不见它，强引用得到 relocation
//! error，弱引用得到 NULL。本卡把这条单独列成 `mismatch` 注解。
//!
//! 判据七维：阶段、errno、绑定地址、注解、弱回退标记、申报状态、档位。
//! 少"地址"一维，绑 0 与绑到定义完全不可分；少"弱回退标记"一维，正常命中
//! 与降级命中会被混为一谈。
//!
//! 判序：状态 → 阶段 → errno → 地址 → 注解 → 弱标记。
//!
//! 语料 14 槽：8 槽申报正确（NULL 1、绑定 4、版本失败 1、符号失败 2），
//! 6 槽各挂一处错误申报（状态/阶段/码/地址/注解/弱标记）。账本 14 格
//! 8 成 6 败，冻结后重放同指纹。
//!
//! 闸门次序：词法 → 版本节点 → 符号作用域 → 版本绑定 → 弱回退 → 就绪。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，`audit` 逐格
//! 对账，冻结后 `replay` 重建同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11656）。
pub const SEG_BASE: u32 = 0x6ce90;

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

/// 局部版本索引（`VER_NDX_LOCAL`）。
pub const VER_NDX_LOCAL: u16 = 0;

/// 全局版本索引（`VER_NDX_GLOBAL`）。
pub const VER_NDX_GLOBAL: u16 = 1;

/// versym 隐藏位。
pub const VER_NDX_HIDDEN: u16 = 0x8000;

/// vernaux 弱版本标记（`VER_FLG_WEAK`）。
pub const VER_FLG_WEAK: u16 = 0x0002;

/// 符号绑定：全局。
pub const STB_GLOBAL: u8 = 1;

/// 符号绑定：弱。
pub const STB_WEAK: u8 = 2;

/// 缺省版本名（测试用）。
pub const DEFAULT_VER: &str = "GLIBC_2.34";

/// 弱版本名（测试用）。
pub const WEAK_VER: &str = "VER_9";

// ---------------------------------------------------------------------------
// errno
// ---------------------------------------------------------------------------

/// 版本节点或符号定义不存在。
pub const ERR_ENOENT: i32 = 2;
/// 内存不足。
pub const ERR_ENOMEM: i32 = 12;
/// 索引非法或版本错配。
pub const ERR_EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const WN_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 申报不符。
pub const WN_ECODE: u32 = SEG_BASE + 0x02;
/// 绑定地址申报不符。
pub const WN_EADDR: u32 = SEG_BASE + 0x03;
/// 注解申报不符。
pub const WN_ENOTE: u32 = SEG_BASE + 0x04;
/// 弱回退标记申报不符。
pub const WN_EWEAK: u32 = SEG_BASE + 0x05;
/// 绑定路径申报不符。
pub const WN_EBIND: u32 = SEG_BASE + 0x06;
/// 状态矛盾。
pub const WN_ESTATE: u32 = SEG_BASE + 0x07;
/// 计数不符。
pub const WN_ECOUNT: u32 = SEG_BASE + 0x08;
/// 槽位非法。
pub const WN_ESLOT: u32 = SEG_BASE + 0x09;
/// 指纹不符。
pub const WN_ESEAL: u32 = SEG_BASE + 0x0a;
/// 无观测。
pub const WN_ENOBS: u32 = SEG_BASE + 0x0b;
/// 矩阵不平。
pub const WN_EMATRIX: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const WN_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 解析请求。
pub const OP_RESOLVE: u8 = 0x01;
/// 版本节点查询。
pub const OP_VERSION: u8 = 0x02;
/// 符号查询。
pub const OP_LOOKUP: u8 = 0x03;
/// 弱回退。
pub const OP_FALLBACK: u8 = 0x04;
/// 绑定。
pub const OP_BIND: u8 = 0x05;
/// 弱标记。
pub const OP_WEAK: u8 = 0x06;
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

/// 强版本需求缺节点（glibc 原文）。
pub fn msg_version_not_found(ver: &str, obj: &str) -> String {
    let mut s = String::from("version `");
    s.push_str(ver);
    s.push_str("' not found (required by ");
    s.push_str(obj);
    s.push(')');
    s
}

/// 弱版本需求缺节点（glibc 原文，作为告警继续）。
pub fn msg_weak_version_not_found(ver: &str, obj: &str) -> String {
    let mut s = String::from("weak version `");
    s.push_str(ver);
    s.push_str("' not found (required by ");
    s.push_str(obj);
    s.push(')');
    s
}

/// 强引用在版本节点内找不到符号。
pub fn msg_undefined_symbol(obj: &str, sym: &str, ver: &str) -> String {
    let mut s = String::from(obj);
    s.push_str(": undefined symbol: ");
    s.push_str(sym);
    if !ver.is_empty() {
        s.push_str(", version ");
        s.push_str(ver);
    }
    s
}

/// 无版本引用撞上非默认版本定义（relocation error）。
pub fn msg_version_mismatch(obj: &str, sym: &str, ver: &str, file: &str) -> String {
    let mut s = String::from(obj);
    s.push_str(": relocation error: ");
    s.push_str(obj);
    s.push_str(": symbol ");
    s.push_str(sym);
    s.push_str(", version ");
    s.push_str(ver);
    s.push_str(" not defined in file ");
    s.push_str(file);
    s.push_str(" with link time reference");
    s
}

/// 弱引用回落 NULL（装载器审计文本）。
pub fn msg_weak_null(obj: &str, sym: &str) -> String {
    let mut s = String::from(obj);
    s.push_str(": weak undefined symbol ");
    s.push_str(sym);
    s.push_str(" resolved to NULL");
    s
}

/// 局部索引强引用（非法 versym）。
pub fn msg_bad_reference(obj: &str, sym: &str) -> String {
    let mut s = String::from(obj);
    s.push_str(": invalid symbol reference: ");
    s.push_str(sym);
    s.push_str(" (local version index)");
    s
}

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------

/// 符号引用绑定强度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
    /// `STB_GLOBAL`。
    Strong,
    /// `STB_WEAK`。
    Weak,
}

impl Bind {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Bind::Strong => "strong",
            Bind::Weak => "weak",
        }
    }

    /// 是否弱绑定。
    pub fn is_weak(&self) -> bool {
        match self {
            Bind::Weak => true,
            Bind::Strong => false,
        }
    }

    /// 由 `st_info` 低四位投影。
    pub fn of_stb(v: u8) -> Option<Bind> {
        match v {
            STB_GLOBAL => Some(Bind::Strong),
            STB_WEAK => Some(Bind::Weak),
            _ => None,
        }
    }
}

/// 定义所在作用域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// 全局可见。
    Global,
    /// 弱定义。
    Weak,
    /// 局部/隐藏，跨对象不可见。
    Local,
    /// 没有定义。
    Undef,
}

impl Scope {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Weak => "weak",
            Scope::Local => "local",
            Scope::Undef => "undef",
        }
    }

    /// 是否跨对象可见。
    pub fn visible(&self) -> bool {
        match self {
            Scope::Global | Scope::Weak => true,
            Scope::Local | Scope::Undef => false,
        }
    }
}

/// 定义携带的版本种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefVer {
    /// 无版本定义（老 DSO）。
    Unversioned,
    /// 默认版本（`@@`）。
    Default,
    /// 非默认版本（`@`）。
    NonDefault,
}

impl DefVer {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            DefVer::Unversioned => "none",
            DefVer::Default => "default",
            DefVer::NonDefault => "nondefault",
        }
    }

    /// 是否可作为无版本引用的落点。
    pub fn reachable_unversioned(&self) -> bool {
        match self {
            DefVer::Unversioned | DefVer::Default => true,
            DefVer::NonDefault => false,
        }
    }

    /// 是否带版本节点。
    pub fn versioned(&self) -> bool {
        match self {
            DefVer::Unversioned => false,
            DefVer::Default | DefVer::NonDefault => true,
        }
    }
}

/// versym 索引的种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReqKind {
    /// 索引 0，局部。
    Local,
    /// 索引 1，全局（无版本）。
    Global,
    /// 索引 ≥ 2，带版本。
    Versioned,
    /// 索引置了 0x8000 隐藏位。
    Hidden,
}

impl ReqKind {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            ReqKind::Local => "local",
            ReqKind::Global => "global",
            ReqKind::Versioned => "versioned",
            ReqKind::Hidden => "hidden",
        }
    }

    /// 是否要求版本节点。
    pub fn versioned(&self) -> bool {
        match self {
            ReqKind::Versioned | ReqKind::Hidden => true,
            _ => false,
        }
    }
}

/// 解析阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 词法。
    Parse,
    /// 版本节点。
    Version,
    /// 符号作用域。
    Lookup,
    /// 版本绑定。
    Bind,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Parse => 0,
            Stage::Version => 1,
            Stage::Lookup => 2,
            Stage::Bind => 3,
            Stage::Ready => 4,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Parse => "parse",
            Stage::Version => "version",
            Stage::Lookup => "lookup",
            Stage::Bind => "bind",
            Stage::Ready => "ready",
        }
    }

    /// 是否失败点。
    pub fn failure(&self) -> bool {
        match self {
            Stage::Parse | Stage::Version | Stage::Lookup | Stage::Bind => true,
            Stage::Ready => false,
        }
    }
}

/// 解析注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 正常命中。
    Ready,
    /// 弱引用回落 NULL。
    WeakNull,
    /// 弱符号按名字命中默认定义。
    WeakHit,
    /// 弱版本节点放行后命中。
    WeakVersion,
    /// 强版本需求缺节点。
    VersionError,
    /// 版本存在但符号缺失。
    SymbolError,
    /// 定义存在但不可见。
    HiddenError,
    /// 无版本引用撞非默认定义。
    Mismatch,
    /// 词法/索引非法。
    ParseError,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Ready => "ready",
            Note::WeakNull => "weak-null",
            Note::WeakHit => "weak-hit",
            Note::WeakVersion => "weak-version",
            Note::VersionError => "version-error",
            Note::SymbolError => "symbol-error",
            Note::HiddenError => "hidden-error",
            Note::Mismatch => "mismatch",
            Note::ParseError => "parse-error",
        }
    }

    /// 是否成功。
    pub fn success(&self) -> bool {
        match self {
            Note::Ready | Note::WeakNull | Note::WeakHit | Note::WeakVersion => true,
            _ => false,
        }
    }

    /// 是否硬错误。
    pub fn error(&self) -> bool {
        match self {
            Note::VersionError
            | Note::SymbolError
            | Note::HiddenError
            | Note::Mismatch
            | Note::ParseError => true,
            _ => false,
        }
    }

    /// 是否弱回退落点。
    pub fn weak(&self) -> bool {
        match self {
            Note::WeakNull | Note::WeakHit | Note::WeakVersion => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 形状
// ---------------------------------------------------------------------------

/// 一次符号解析请求的形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 请求对象名。
    pub obj: &'static str,
    /// 符号名。
    pub sym: &'static str,
    /// 绑定强度。
    pub bind: Bind,
    /// versym 索引（含隐藏位）。
    pub req: u16,
    /// 版本节点是否存在于依赖中。
    pub ver_present: bool,
    /// vernaux 是否带 `VER_FLG_WEAK`。
    pub req_weak: bool,
    /// 定义作用域。
    pub def: Scope,
    /// 定义版本种类。
    pub def_ver: DefVer,
    /// 版本需求名。
    pub ver_name: &'static str,
    /// 定义所在文件。
    pub def_file: &'static str,
    /// 定义版本名。
    pub def_ver_name: &'static str,
    /// 定义地址（0 表示无地址）。
    pub addr: u32,
}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            obj: "",
            sym: "",
            bind: Bind::Strong,
            req: VER_NDX_GLOBAL,
            ver_present: false,
            req_weak: false,
            def: Scope::Undef,
            def_ver: DefVer::Unversioned,
            ver_name: "",
            def_file: "",
            def_ver_name: "",
            addr: 0,
        }
    }

    /// versym 索引种类。
    pub fn req_kind(&self) -> ReqKind {
        if self.req & VER_NDX_HIDDEN != 0 {
            return ReqKind::Hidden;
        }
        match self.req {
            VER_NDX_LOCAL => ReqKind::Local,
            VER_NDX_GLOBAL => ReqKind::Global,
            _ => ReqKind::Versioned,
        }
    }

    /// 是否要求版本节点。
    pub fn versioned(&self) -> bool {
        self.req_kind().versioned()
    }

    /// 绑定地址是否为空。
    pub fn null_bound(&self) -> bool {
        self.addr == 0
    }

    /// 是否弱引用。
    pub fn weak_ref(&self) -> bool {
        self.bind.is_weak()
    }

    /// 名字与索引指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.obj);
        h = fold_str(h, self.sym);
        h = fold_word(h, self.bind.is_weak() as u64);
        h = fold_word(h, self.req as u64);
        h = fold_word(h, self.ver_present as u64);
        h = fold_word(h, self.req_weak as u64);
        h = fold_str(h, self.def.name());
        h = fold_str(h, self.def_ver.name());
        h = fold_str(h, self.ver_name);
        h = fold_str(h, self.def_file);
        h = fold_str(h, self.def_ver_name);
        h = fold_word(h, self.addr as u64);
        h
    }
}

/// 解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 结束阶段。
    pub stage: Stage,
    /// errno（0 表示成功）。
    pub code: i32,
    /// 绑定地址（0 表示 NULL）。
    pub addr: u32,
    /// 注解。
    pub note: Note,
    /// 是否走了弱回退。
    pub weak: bool,
}

impl Outcome {
    /// 空白结果。
    pub const fn blank() -> Outcome {
        Outcome { stage: Stage::Parse, code: 0, addr: 0, note: Note::Ready, weak: false }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.code == 0 && self.stage == Stage::Ready && self.note.success()
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否硬错误。
    pub fn hard(&self) -> bool {
        self.note.error()
    }

    /// 是否绑到 NULL。
    pub fn null(&self) -> bool {
        self.ok() && self.addr == 0
    }

    /// 是否绑到定义。
    pub fn bound(&self) -> bool {
        self.ok() && self.addr != 0
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.order() as u64);
        h = fold_word(h, self.code as i64 as u64);
        h = fold_word(h, self.addr as u64);
        h = fold_str(h, self.note.name());
        h = fold_word(h, self.weak as u64);
        h
    }
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// 解析一个引用。
///
/// 闸门次序严格按 `parse → version → lookup → bind → ready`。弱回退只发生
/// 在两个位置：
///
/// - `lookup` 查不到定义：弱引用直接绑 0，强引用硬错误；
/// - `bind` 版本节点已放行但节点缺失：弱标记按名字取默认定义，取不到绑 0。
///
/// 版本节点存在但符号不在该节点内，按 `symbol-error` 处理，与"节点缺失"
/// 区分开：前者的依赖确实声明了该版本，后者连版本都没声明。
pub fn resolve(s: &Shape) -> Outcome {
    // ---- parse ----
    if s.obj.is_empty() || s.sym.is_empty() {
        return Outcome {
            stage: Stage::Parse,
            code: ERR_EINVAL,
            addr: 0,
            note: Note::ParseError,
            weak: s.bind.is_weak(),
        };
    }
    let kind = s.req_kind();
    if kind == ReqKind::Local {
        if s.bind.is_weak() {
            return Outcome {
                stage: Stage::Ready,
                code: 0,
                addr: 0,
                note: Note::WeakNull,
                weak: true,
            };
        }
        return Outcome {
            stage: Stage::Parse,
            code: ERR_EINVAL,
            addr: 0,
            note: Note::ParseError,
            weak: false,
        };
    }

    // ---- version ----
    let mut ver_weak = false;
    if kind.versioned() && !s.ver_present {
        if s.req_weak {
            ver_weak = true;
        } else if !s.bind.is_weak() {
            return Outcome {
                stage: Stage::Version,
                code: ERR_ENOENT,
                addr: 0,
                note: Note::VersionError,
                weak: false,
            };
        }
    }

    // ---- lookup ----
    match s.def {
        Scope::Undef => {
            if s.bind.is_weak() {
                return Outcome {
                    stage: Stage::Ready,
                    code: 0,
                    addr: 0,
                    note: Note::WeakNull,
                    weak: true,
                };
            }
            let note = if ver_weak { Note::VersionError } else { Note::SymbolError };
            let stage = if ver_weak { Stage::Version } else { Stage::Lookup };
            return Outcome { stage, code: ERR_ENOENT, addr: 0, note, weak: false };
        }
        Scope::Local => {
            if s.bind.is_weak() {
                return Outcome {
                    stage: Stage::Ready,
                    code: 0,
                    addr: 0,
                    note: Note::WeakNull,
                    weak: true,
                };
            }
            return Outcome {
                stage: Stage::Lookup,
                code: ERR_ENOENT,
                addr: 0,
                note: Note::HiddenError,
                weak: false,
            };
        }
        Scope::Global | Scope::Weak => {}
    }

    // ---- bind ----
    if kind == ReqKind::Global {
        if !s.def_ver.reachable_unversioned() {
            if s.bind.is_weak() {
                return Outcome {
                    stage: Stage::Ready,
                    code: 0,
                    addr: 0,
                    note: Note::WeakNull,
                    weak: true,
                };
            }
            return Outcome {
                stage: Stage::Bind,
                code: ERR_EINVAL,
                addr: 0,
                note: Note::Mismatch,
                weak: false,
            };
        }
        return Outcome { stage: Stage::Ready, code: 0, addr: s.addr, note: Note::Ready, weak: false };
    }

    // 带版本的引用：节点存在则按节点命中；节点缺失则走名字回退。
    if s.ver_present {
        return Outcome { stage: Stage::Ready, code: 0, addr: s.addr, note: Note::Ready, weak: false };
    }
    if !s.def_ver.reachable_unversioned() {
        if s.bind.is_weak() {
            return Outcome {
                stage: Stage::Ready,
                code: 0,
                addr: 0,
                note: Note::WeakNull,
                weak: true,
            };
        }
        return Outcome {
            stage: Stage::Bind,
            code: ERR_EINVAL,
            addr: 0,
            note: Note::Mismatch,
            weak: false,
        };
    }
    let note = if s.req_weak { Note::WeakVersion } else { Note::WeakHit };
    Outcome { stage: Stage::Ready, code: 0, addr: s.addr, note, weak: true }
}

/// 文案。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::VersionError => msg_version_not_found(s.ver_name, s.obj),
        Note::SymbolError => msg_undefined_symbol(s.obj, s.sym, s.ver_name),
        Note::HiddenError => msg_undefined_symbol(s.obj, s.sym, s.ver_name),
        Note::Mismatch => msg_version_mismatch(s.obj, s.sym, s.def_ver_name, s.def_file),
        Note::WeakNull => msg_weak_null(s.obj, s.sym),
        Note::ParseError => msg_bad_reference(s.obj, s.sym),
        Note::WeakVersion => msg_weak_version_not_found(s.ver_name, s.obj),
        Note::Ready | Note::WeakHit => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 结果档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 绑到 NULL。
    Null,
    /// 绑到定义。
    Bound,
    /// 版本层硬错误。
    VerFail,
    /// 符号层硬错误。
    SymFail,
}

impl Tier {
    /// 由结果投影。
    pub fn of(o: &Outcome) -> Tier {
        if o.ok() {
            if o.addr == 0 {
                Tier::Null
            } else {
                Tier::Bound
            }
        } else if o.note == Note::VersionError {
            Tier::VerFail
        } else {
            Tier::SymFail
        }
    }

    /// 次序。
    pub fn index(&self) -> usize {
        match self {
            Tier::Null => 0,
            Tier::Bound => 1,
            Tier::VerFail => 2,
            Tier::SymFail => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Null => "null",
            Tier::Bound => "bound",
            Tier::VerFail => "verfail",
            Tier::SymFail => "symfail",
        }
    }

    /// 是否成功档。
    pub fn success(&self) -> bool {
        match self {
            Tier::Null | Tier::Bound => true,
            Tier::VerFail | Tier::SymFail => false,
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
    /// 绑定地址错。
    Addr,
    /// 注解错。
    Note,
    /// 弱标记错。
    Weak,
    /// 绑定路径错。
    Bind,
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
            Fault::Stage => WN_ESTAGE,
            Fault::Code => WN_ECODE,
            Fault::Addr => WN_EADDR,
            Fault::Note => WN_ENOTE,
            Fault::Weak => WN_EWEAK,
            Fault::Bind => WN_EBIND,
            Fault::Estate => WN_ESTATE,
            Fault::Count => WN_ECOUNT,
            Fault::Slot => WN_ESLOT,
            Fault::Seal => WN_ESEAL,
            Fault::Nobs => WN_ENOBS,
            Fault::Matrix => WN_EMATRIX,
            Fault::Bal => WN_EBAL,
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
        Fault::Addr => "addr",
        Fault::Note => "note",
        Fault::Weak => "weak",
        Fault::Bind => "bind",
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
    /// 绑定地址。
    pub addr: u32,
    /// 注解。
    pub note: Note,
    /// 弱回退标记。
    pub weak: bool,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Ready, code: 0, addr: 0, note: Note::Ready, weak: false }
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
        resolve(&self.shape)
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
        tag: "weak-null",
        obj: "./a.so",
        sym: "foo",
        bind: Bind::Weak,
        req: 2,
        ver_present: false,
        req_weak: false,
        def: Scope::Undef,
        def_ver: DefVer::Unversioned,
        ver_name: "GLIBC_2.34",
        def_file: "libdep.so",
        def_ver_name: "",
        addr: 0,
    },
    Shape {
        tag: "weak-sym",
        obj: "./a.so",
        sym: "bar",
        bind: Bind::Weak,
        req: VER_NDX_GLOBAL,
        ver_present: false,
        req_weak: false,
        def: Scope::Global,
        def_ver: DefVer::Unversioned,
        ver_name: "",
        def_file: "libdep.so",
        def_ver_name: "",
        addr: 0x1000,
    },
    Shape {
        tag: "weak-ver-weakdef",
        obj: "./a.so",
        sym: "baz",
        bind: Bind::Weak,
        req: 2,
        ver_present: false,
        req_weak: true,
        def: Scope::Weak,
        def_ver: DefVer::Default,
        ver_name: "VER_9",
        def_file: "libdep.so",
        def_ver_name: "VER_9",
        addr: 0x3000,
    },
    Shape {
        tag: "strong-default",
        obj: "./a.so",
        sym: "qux",
        bind: Bind::Strong,
        req: VER_NDX_GLOBAL,
        ver_present: false,
        req_weak: false,
        def: Scope::Global,
        def_ver: DefVer::Default,
        ver_name: "",
        def_file: "libdep.so",
        def_ver_name: "GLIBC_2.34",
        addr: 0x4000,
    },
    Shape {
        tag: "strong-ver",
        obj: "./a.so",
        sym: "memcpy",
        bind: Bind::Strong,
        req: 2,
        ver_present: true,
        req_weak: false,
        def: Scope::Global,
        def_ver: DefVer::Default,
        ver_name: "GLIBC_2.34",
        def_file: "libc.so.6",
        def_ver_name: "GLIBC_2.34",
        addr: 0x5000,
    },
    Shape {
        tag: "strong-missing-ver",
        obj: "./a.so",
        sym: "foo",
        bind: Bind::Strong,
        req: 2,
        ver_present: false,
        req_weak: false,
        def: Scope::Undef,
        def_ver: DefVer::Unversioned,
        ver_name: "GLIBC_2.34",
        def_file: "libdep.so",
        def_ver_name: "",
        addr: 0,
    },
    Shape {
        tag: "strong-mismatch",
        obj: "./a.so",
        sym: "foo",
        bind: Bind::Strong,
        req: VER_NDX_GLOBAL,
        ver_present: false,
        req_weak: false,
        def: Scope::Global,
        def_ver: DefVer::NonDefault,
        ver_name: "",
        def_file: "libdep.so",
        def_ver_name: "GLIBC_2.34",
        addr: 0x6000,
    },
    Shape {
        tag: "strong-hidden",
        obj: "./a.so",
        sym: "foo",
        bind: Bind::Strong,
        req: 2,
        ver_present: true,
        req_weak: false,
        def: Scope::Local,
        def_ver: DefVer::Default,
        ver_name: "GLIBC_2.34",
        def_file: "libdep.so",
        def_ver_name: "GLIBC_2.34",
        addr: 0x7000,
    },
    Shape {
        tag: "estate-swap",
        obj: "./a.so",
        sym: "foo",
        bind: Bind::Strong,
        req: 2,
        ver_present: false,
        req_weak: false,
        def: Scope::Undef,
        def_ver: DefVer::Unversioned,
        ver_name: "GLIBC_2.34",
        def_file: "libdep.so",
        def_ver_name: "",
        addr: 0,
    },
    Shape {
        tag: "stage-swap",
        obj: "./a.so",
        sym: "foo",
        bind: Bind::Strong,
        req: 2,
        ver_present: false,
        req_weak: false,
        def: Scope::Undef,
        def_ver: DefVer::Unversioned,
        ver_name: "GLIBC_2.34",
        def_file: "libdep.so",
        def_ver_name: "",
        addr: 0,
    },
    Shape {
        tag: "code-swap",
        obj: "./a.so",
        sym: "foo",
        bind: Bind::Strong,
        req: 2,
        ver_present: false,
        req_weak: false,
        def: Scope::Undef,
        def_ver: DefVer::Unversioned,
        ver_name: "GLIBC_2.34",
        def_file: "libdep.so",
        def_ver_name: "",
        addr: 0,
    },
    Shape {
        tag: "addr-swap",
        obj: "./a.so",
        sym: "foo",
        bind: Bind::Strong,
        req: 2,
        ver_present: false,
        req_weak: false,
        def: Scope::Undef,
        def_ver: DefVer::Unversioned,
        ver_name: "GLIBC_2.34",
        def_file: "libdep.so",
        def_ver_name: "",
        addr: 0,
    },
    Shape {
        tag: "note-swap",
        obj: "./a.so",
        sym: "foo",
        bind: Bind::Strong,
        req: 2,
        ver_present: false,
        req_weak: false,
        def: Scope::Undef,
        def_ver: DefVer::Unversioned,
        ver_name: "GLIBC_2.34",
        def_file: "libdep.so",
        def_ver_name: "",
        addr: 0,
    },
    Shape {
        tag: "weak-swap",
        obj: "./a.so",
        sym: "foo",
        bind: Bind::Strong,
        req: 2,
        ver_present: false,
        req_weak: false,
        def: Scope::Undef,
        def_ver: DefVer::Unversioned,
        ver_name: "GLIBC_2.34",
        def_file: "libdep.so",
        def_ver_name: "",
        addr: 0,
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { stage: Stage::Ready, code: 0, addr: 0, note: Note::WeakNull, weak: true },
    Claim { stage: Stage::Ready, code: 0, addr: 0x1000, note: Note::Ready, weak: false },
    Claim { stage: Stage::Ready, code: 0, addr: 0x3000, note: Note::WeakVersion, weak: true },
    Claim { stage: Stage::Ready, code: 0, addr: 0x4000, note: Note::Ready, weak: false },
    Claim { stage: Stage::Ready, code: 0, addr: 0x5000, note: Note::Ready, weak: false },
    Claim { stage: Stage::Version, code: ERR_ENOENT, addr: 0, note: Note::VersionError, weak: false },
    Claim { stage: Stage::Bind, code: ERR_EINVAL, addr: 0, note: Note::Mismatch, weak: false },
    Claim { stage: Stage::Lookup, code: ERR_ENOENT, addr: 0, note: Note::HiddenError, weak: false },
    Claim { stage: Stage::Ready, code: 0, addr: 0, note: Note::Ready, weak: false },
    Claim { stage: Stage::Bind, code: ERR_ENOENT, addr: 0, note: Note::VersionError, weak: false },
    Claim { stage: Stage::Version, code: 0, addr: 0, note: Note::VersionError, weak: false },
    Claim { stage: Stage::Version, code: ERR_ENOENT, addr: 0x7777, note: Note::VersionError, weak: false },
    Claim { stage: Stage::Version, code: ERR_ENOENT, addr: 0, note: Note::SymbolError, weak: false },
    Claim { stage: Stage::Version, code: ERR_ENOENT, addr: 0, note: Note::VersionError, weak: true },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Addr, Fault::Note, Fault::Weak];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    WN_ESTATE,
    WN_ESTAGE,
    WN_ECODE,
    WN_EADDR,
    WN_ENOTE,
    WN_EWEAK,
];

/// 账本 8 个成功格的值（总数、成、败、NULL、绑定、版本失败、符号失败、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 1, 4, 1, 2, 929];

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
        return Err(WN_ENOBS);
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
    if a.addr != k.addr {
        return Fault::Addr;
    }
    if a.note != k.note {
        return Fault::Note;
    }
    if a.weak != k.weak {
        return Fault::Weak;
    }
    Fault::Ok
}

/// 逐槽释义。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let msg = message(&c.shape, &o);
    let mut s = String::from(c.shape.obj);
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" code=");
    s.push_str(i32_text(o.code).as_str());
    s.push_str(" addr=0x");
    s.push_str(hex_text(o.addr, 8).as_str());
    s.push_str(" weak=");
    s.push_str(bool_text(o.weak));
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
// 矩阵
// ---------------------------------------------------------------------------

/// 矩阵一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档槽数。
    pub total: u32,
    /// 该档申报正确数。
    pub good: u32,
}

impl Row {
    /// 空白行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }

    /// 是否全对。
    pub fn perfect(&self) -> bool {
        self.total == self.good
    }
}

/// 档位 × 申报 矩阵。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 各行。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 空白矩阵。
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
                let t = Tier::of(&c.actual());
                if let Some(r) = m.rows.get_mut(t.index()) {
                    r.total = r.total.saturating_add(1);
                    if judge(c) == Fault::Ok {
                        r.good = r.good.saturating_add(1);
                    }
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
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                n += r.total as usize;
            }
            i += 1;
        }
        n
    }

    /// 总正确数。
    pub fn good(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                n += r.good as usize;
            }
            i += 1;
        }
        n
    }

    /// 每档都有槽且都有正确申报。
    pub fn balanced(&self) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if r.total == 0 || r.good == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<Row> {
        self.rows.get(i).copied()
    }

    /// 全对行数。
    pub fn perfect_rows(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
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
        while i < self.rows.len() {
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
// 报告
// ---------------------------------------------------------------------------

/// 全跑报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确数。
    pub accept: u32,
    /// 申报错误数。
    pub reject: u32,
    /// NULL 档正确数。
    pub null: u32,
    /// 绑定档正确数。
    pub bound: u32,
    /// 版本失败档正确数。
    pub verfail: u32,
    /// 符号失败档正确数。
    pub symfail: u32,
    /// 弱回退命中数。
    pub weak: u32,
    /// 对判命中数。
    pub matched: u32,
}

impl Report {
    /// 空白报告。
    pub const fn blank() -> Report {
        Report {
            total: 0,
            accept: 0,
            reject: 0,
            null: 0,
            bound: 0,
            verfail: 0,
            symfail: 0,
            weak: 0,
            matched: 0,
        }
    }

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

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(WN_ECOUNT)
        }
    }

    /// 弱回退总次数（含错误申报槽）。
    pub fn weak_fallbacks(&self) -> u32 {
        let cs = cases();
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                if c.actual().weak {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("weak-nover corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" null=");
        s.push_str(u32_text(self.null).as_str());
        s.push_str(" bound=");
        s.push_str(u32_text(self.bound).as_str());
        s.push_str(" verfail=");
        s.push_str(u32_text(self.verfail).as_str());
        s.push_str(" symfail=");
        s.push_str(u32_text(self.symfail).as_str());
        s.push_str(" weak=");
        s.push_str(u32_text(self.weak).as_str());
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
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(WN_ESLOT);
        }
    }
    let cs = cases();
    let mut r = Report::blank();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(WN_ENOBS),
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
            None => return Err(WN_ENOBS),
        };
        r.total = r.total.saturating_add(1);
        if f == want {
            r.matched = r.matched.saturating_add(1);
        }
        if f == Fault::Ok {
            r.accept = r.accept.saturating_add(1);
            match Tier::of(&a) {
                Tier::Null => r.null = r.null.saturating_add(1),
                Tier::Bound => r.bound = r.bound.saturating_add(1),
                Tier::VerFail => r.verfail = r.verfail.saturating_add(1),
                Tier::SymFail => r.symfail = r.symfail.saturating_add(1),
            }
            if a.weak {
                r.weak = r.weak.saturating_add(1);
            }
        } else {
            r.reject = r.reject.saturating_add(1);
        }
        i += 1;
    }
    Ok(r)
}

// ---------------------------------------------------------------------------
// 阶段闸门
// ---------------------------------------------------------------------------

/// 单调阶段闸门。
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
            return Err(WN_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(WN_ESTAGE);
        }
        self.last = o;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(WN_ESTATE);
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
            return Err(WN_ESLOT);
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
        if self.len != SAMPLES || self.ok_count() != ACCEPT || self.err_count() != REJECT {
            return Err(WN_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(WN_EBAL),
            };
            if s.slot as usize != i {
                return Err(WN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(WN_EBAL);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(WN_EBAL);
                        }
                    }
                    None => return Err(WN_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(WN_EBAL);
                        }
                    }
                    None => return Err(WN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(WN_EBAL);
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
            return Err(WN_EBAL);
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
            Err(WN_ESEAL)
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
    let _ = l.register(count_cell(3, OP_FALLBACK, r.map(|x| x.null)));
    let _ = l.register(count_cell(4, OP_BIND, r.map(|x| x.bound)));
    let _ = l.register(count_cell(5, OP_VERSION, r.map(|x| x.verfail)));
    let _ = l.register(count_cell(6, OP_LOOKUP, r.map(|x| x.symfail)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_WEAK, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_REPLAY,
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
    out[0] = name_ok() && shape_ok() && index_ok();
    out[1] = resolve_ok() && judge_ok() && matrix_ok();
    out[2] = corpus_ok() && reds_ok() && detail_ok() && slots_ok() && notes_ok();
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
        WN_ESTAGE, WN_ECODE, WN_EADDR, WN_ENOTE, WN_EWEAK, WN_EBIND, WN_ESTATE, WN_ECOUNT,
        WN_ESLOT, WN_ESEAL, WN_ENOBS, WN_EMATRIX, WN_EBAL,
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
    let a = SEG_BASE == 0x6ce90;
    let b = WN_ESTAGE == SEG_BASE + 0x01 && WN_EBAL == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = TIER_COUNT == 4 && VER_NDX_LOCAL == 0 && VER_NDX_GLOBAL == 1;
    let e = VER_NDX_HIDDEN == 0x8000 && VER_FLG_WEAK == 2 && STB_GLOBAL == 1 && STB_WEAK == 2;
    a && b && c && d && e
}

/// 符号形状。
fn shape_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.tag.is_empty() || s.obj.is_empty() || s.sym.is_empty() {
            return false;
        }
        let mut j = 0usize;
        while j < i {
            if let Some(p) = SHAPES.get(j) {
                if p.tag == s.tag {
                    return false;
                }
            }
            j += 1;
        }
        if s.def.visible() && s.addr == 0 && s.def != Scope::Undef {
            return false;
        }
        if s.req_weak && !s.versioned() {
            return false;
        }
        if s.def_ver != DefVer::Unversioned && s.def_ver_name.is_empty() {
            return false;
        }
        i += 1;
    }
    let a = SHAPES[0].bind == Bind::Weak && SHAPES[0].def == Scope::Undef;
    let b = SHAPES[1].bind == Bind::Weak && SHAPES[1].def == Scope::Global;
    let c = SHAPES[2].req_weak && SHAPES[2].def == Scope::Weak;
    let d = SHAPES[5].bind == Bind::Strong && !SHAPES[5].ver_present;
    let e = SHAPES[6].def_ver == DefVer::NonDefault;
    let f = SHAPES[7].def == Scope::Local;
    let g = SHAPES[0].seal() != SHAPES[1].seal();
    let h = Shape::blank().seal() != SHAPES[0].seal();
    a && b && c && d && e && f && g && h
}

/// 版本索引。
fn index_ok() -> bool {
    let a = SHAPES[0].req_kind() == ReqKind::Versioned;
    let b = SHAPES[1].req_kind() == ReqKind::Global;
    let c = {
        let mut s = Shape::blank();
        s.req = VER_NDX_LOCAL;
        s.req_kind() == ReqKind::Local
    };
    let d = {
        let mut s = Shape::blank();
        s.req = VER_NDX_HIDDEN + 2;
        s.req_kind() == ReqKind::Hidden && s.versioned()
    };
    let e = Bind::of_stb(STB_GLOBAL) == Some(Bind::Strong)
        && Bind::of_stb(STB_WEAK) == Some(Bind::Weak)
        && Bind::of_stb(0) == None;
    let f = DefVer::Unversioned.reachable_unversioned()
        && DefVer::Default.reachable_unversioned()
        && !DefVer::NonDefault.reachable_unversioned();
    let g = DefVer::Unversioned.versioned()
        == false
        && DefVer::Default.versioned()
        && DefVer::NonDefault.versioned();
    let h = Scope::Global.visible() && Scope::Weak.visible() && !Scope::Local.visible();
    let i = !ReqKind::Local.versioned()
        && !ReqKind::Global.versioned()
        && ReqKind::Versioned.versioned()
        && ReqKind::Hidden.versioned();
    a && b && c && d && e && f && g && h && i
}

/// 解析纵线。
fn resolve_ok() -> bool {
    // 弱引用 + 缺节点 + 无定义 → NULL。
    let a = {
        let o = resolve(&SHAPES[0]);
        o.ok() && o.null() && o.weak && o.note == Note::WeakNull && o.stage == Stage::Ready
    };
    // 弱引用 + 无版本 → 绑定无版本定义。
    let b = {
        let o = resolve(&SHAPES[1]);
        o.ok() && o.addr == 0x1000 && !o.weak && o.note == Note::Ready
    };
    // 弱版本节点 + 默认定义 → 名字回退命中。
    let c = {
        let o = resolve(&SHAPES[2]);
        o.ok() && o.addr == 0x3000 && o.weak && o.note == Note::WeakVersion
    };
    // 强引用 + 无版本 + 默认定义 → 命中。
    let d = {
        let o = resolve(&SHAPES[3]);
        o.ok() && o.addr == 0x4000 && !o.weak
    };
    // 强引用 + 版本命中 → 命中。
    let e = {
        let o = resolve(&SHAPES[4]);
        o.ok() && o.addr == 0x5000 && o.note == Note::Ready
    };
    // 强引用 + 缺节点 → 版本错误。
    let f = {
        let o = resolve(&SHAPES[5]);
        o.failed() && o.stage == Stage::Version && o.code == ERR_ENOENT && o.note == Note::VersionError
    };
    // 无版本引用 + 非默认定义 → 错配。
    let g = {
        let o = resolve(&SHAPES[6]);
        o.failed() && o.stage == Stage::Bind && o.code == ERR_EINVAL && o.note == Note::Mismatch
    };
    // 版本命中但定义隐藏 → 符号错误。
    let h = {
        let o = resolve(&SHAPES[7]);
        o.failed() && o.stage == Stage::Lookup && o.note == Note::HiddenError
    };
    // 老 DSO 无版本定义可满足版本引用。
    let i = {
        let mut s = SHAPES[4];
        s.def_ver = DefVer::Unversioned;
        let o = resolve(&s);
        o.ok() && o.addr == 0x5000
    };
    // 版本命中但无定义 → 符号错误。
    let j = {
        let mut s = SHAPES[4];
        s.def = Scope::Undef;
        s.addr = 0;
        let o = resolve(&s);
        o.failed() && o.stage == Stage::Lookup && o.note == Note::SymbolError
    };
    // 弱引用无版本无定义 → NULL。
    let k = {
        let mut s = SHAPES[1];
        s.def = Scope::Undef;
        s.addr = 0;
        let o = resolve(&s);
        o.ok() && o.null() && o.weak
    };
    // 局部索引强引用 → 词法错误。
    let l = {
        let mut s = SHAPES[5];
        s.req = VER_NDX_LOCAL;
        let o = resolve(&s);
        o.failed() && o.stage == Stage::Parse && o.note == Note::ParseError
    };
    // 局部索引弱引用 → NULL。
    let m = {
        let mut s = SHAPES[0];
        s.req = VER_NDX_LOCAL;
        let o = resolve(&s);
        o.ok() && o.null() && o.weak
    };
    // 空符号名 → 词法错误。
    let n = {
        let mut s = SHAPES[0];
        s.sym = "";
        let o = resolve(&s);
        o.failed() && o.stage == Stage::Parse && o.note == Note::ParseError
    };
    // 弱引用 + 非默认定义 → NULL（看不见非默认版本）。
    let p = {
        let mut s = SHAPES[6];
        s.bind = Bind::Weak;
        let o = resolve(&s);
        o.ok() && o.null() && o.weak
    };
    a && b && c && d && e && f && g && h && i && j && k && l && m && n && p
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

/// 矩阵。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    m.row(0) == Some(Row { total: 1, good: 1 })
        && m.row(1) == Some(Row { total: 4, good: 4 })
        && m.row(2) == Some(Row { total: 7, good: 1 })
        && m.row(3) == Some(Row { total: 2, good: 2 })
        && m.row(4).is_none()
        && m.total() == CORPUS
        && m.good() == ACCEPT
        && m.balanced()
        && m.perfect_rows() == 3
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

/// 逐槽注解合规。
fn notes_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if a.ok() && !a.note.success() {
            return false;
        }
        if a.failed() && !a.note.error() {
            return false;
        }
        if a.ok() && a.code != 0 {
            return false;
        }
        if a.null() && !a.weak {
            return false;
        }
        if a.note.weak() && !a.weak {
            return false;
        }
        if a.note == Note::Ready && a.weak {
            return false;
        }
        if detail(&c).as_str().is_empty() {
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

/// 语料与渲染。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.null == 1 && r.bound == 4 && r.verfail == 1 && r.symfail == 2 && r.weak == 2;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = tags_unique() && notes_ok() && table_ok();
    let e = r.render().as_str()
        == "weak-nover corpus=14 accept=8 reject=6 null=1 bound=4 verfail=1 symfail=2 weak=2 match=14 rate=1000ppm\n";
    let f = r.weak_fallbacks() == 2;
    a && b && c && d && e && f
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929;
    let b = r.gate() == Err(WN_ECOUNT) && !r.green();
    let c = run_with(Some((CORPUS, Fault::Code))).err() == Some(WN_ESLOT);
    a && b && c
}

/// 逐字释义：弱引用回 NULL。
const DETAIL_NULL: &str =
    "./a.so stage=ready code=0 addr=0x00000000 weak=true note=weak-null msg=\"./a.so: weak undefined symbol foo resolved to NULL\"\n";

/// 逐字释义：强版本缺节点。
const DETAIL_VERSION: &str =
    "./a.so stage=version code=2 addr=0x00000000 weak=false note=version-error msg=\"version `GLIBC_2.34' not found (required by ./a.so)\"\n";

/// 释义。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_NULL,
        None => false,
    };
    let b = match cs.get(5) {
        Some(c) => detail(c).as_str() == DETAIL_VERSION,
        None => false,
    };
    let c = match cs.get(1) {
        Some(c) => {
            let o = c.actual();
            o.addr == 0x1000 && !o.weak
        }
        None => false,
    };
    let d = match cs.get(6) {
        Some(c) => {
            message(&c.shape, &c.actual()).as_str()
                == "./a.so: relocation error: ./a.so: symbol foo, version GLIBC_2.34 not defined in file libdep.so with link time reference"
        }
        None => false,
    };
    let e = match cs.get(2) {
        Some(c) => {
            message(&c.shape, &c.actual()).as_str()
                == "weak version `VER_9' not found (required by ./a.so)"
        }
        None => false,
    };
    let f = match cs.get(7) {
        Some(c) => {
            message(&c.shape, &c.actual()).as_str()
                == "./a.so: undefined symbol: foo, version GLIBC_2.34"
        }
        None => false,
    };
    let g = match cs.get(1) {
        Some(c) => message(&c.shape, &c.actual()).is_empty(),
        None => false,
    };
    a && b && c && d && e && f && g
}

/// 槽号与档位。
fn slots_ok() -> bool {
    let a = match case_at(0) {
        Some(c) => c.tag() == "weak-null",
        None => false,
    };
    let b = match case_at(13) {
        Some(c) => c.tag() == "weak-swap",
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(3) == Err(WN_ENOBS);
    let e = Stage::Parse.order() == 0
        && Stage::Version.order() == 1
        && Stage::Lookup.order() == 2
        && Stage::Bind.order() == 3
        && Stage::Ready.order() == 4;
    let f = Stage::Parse.failure() && Stage::Bind.failure() && !Stage::Ready.failure();
    let g = Tier::Null.index() == 0
        && Tier::Bound.index() == 1
        && Tier::VerFail.index() == 2
        && Tier::SymFail.index() == 3;
    let h = Tier::Null.name() == "null"
        && Tier::Bound.name() == "bound"
        && Tier::VerFail.name() == "verfail"
        && Tier::SymFail.name() == "symfail";
    let i = fault_short(Fault::Addr) == "addr"
        && fault_short(Fault::Weak) == "weak"
        && fault_short(Fault::Ok) == "ok";
    let j = Note::WeakNull.weak()
        && Note::WeakHit.weak()
        && Note::WeakVersion.weak()
        && !Note::Ready.weak()
        && Note::ParseError.error()
        && Note::VersionError.error();
    a && b && c && d && e && f && g && h && i && j
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Parse) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Version) == Ok(()) && g.last() == 1;
    let d = g.step(Stage::Parse) == Err(WN_ESTAGE) && g.steps() == 2;
    let e = g.close() == Ok(2) && g.closed();
    let f = g.close() == Err(WN_ESTATE);
    let h = g.step(Stage::Ready) == Err(WN_ESTATE);
    a && b && c && d && e && f && h
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
pub fn run_unx11656_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11656_weak_nover");

    s.add("WN01-名称与段基址", name_ok(), "static");

    s.add("WN02-符号形状", shape_ok(), "static");

    s.add("WN03-版本索引", index_ok(), "static");

    s.add("WN04-解析纵线", resolve_ok(), "static");

    s.add("WN05-申报对判", judge_ok(), "static");

    s.add("WN06-矩阵统计", matrix_ok(), "static");

    s.add("WN07-语料与渲染", corpus_ok(), "static");

    s.add("WN08-红档显式", reds_ok(), "static");

    s.add("WN09-释义逐字", detail_ok(), "static");

    s.add("WN10-槽号与档位", slots_ok(), "static");

    s.add("WN11-阶段闸门", gate_ok(), "static");

    s.add("WN12-账本对账", ledger_ok(), "static");

    s.add(
        "WN13-冻结重放四注入",
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
        panic!("unx11656 test fail")
    }

    #[test]
    fn consts() {
        assert_eq!(SEG_BASE, 0x6ce90);
        assert_eq!(SAMPLES, 14);
        assert_eq!(CORPUS, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(VALS[7], 929);
    }

    #[test]
    fn errno_values() {
        assert_eq!(ERR_ENOENT, 2);
        assert_eq!(ERR_ENOMEM, 12);
        assert_eq!(ERR_EINVAL, 22);
    }

    #[test]
    fn codes_spaced() {
        assert_eq!(WN_ESTAGE, 0x6ce91);
        assert_eq!(WN_EADDR, 0x6ce93);
        assert_eq!(WN_EWEAK, 0x6ce95);
        assert_eq!(WN_EBAL, 0x6ce9d);
    }

    #[test]
    fn version_consts() {
        assert_eq!(VER_NDX_LOCAL, 0);
        assert_eq!(VER_NDX_GLOBAL, 1);
        assert_eq!(VER_NDX_HIDDEN, 0x8000);
        assert_eq!(VER_FLG_WEAK, 0x0002);
        assert_eq!(STB_GLOBAL, 1);
        assert_eq!(STB_WEAK, 2);
    }

    #[test]
    fn stage_orders() {
        assert_eq!(Stage::Parse.order(), 0);
        assert_eq!(Stage::Version.order(), 1);
        assert_eq!(Stage::Lookup.order(), 2);
        assert_eq!(Stage::Bind.order(), 3);
        assert_eq!(Stage::Ready.order(), 4);
        assert!(Stage::Version.failure());
        assert!(Stage::Lookup.failure());
        assert!(!Stage::Ready.failure());
    }

    #[test]
    fn note_semantics() {
        assert!(Note::Ready.success());
        assert!(Note::WeakNull.success());
        assert!(Note::WeakHit.success());
        assert!(Note::WeakVersion.success());
        assert!(!Note::VersionError.success());
        assert!(Note::VersionError.error());
        assert!(Note::SymbolError.error());
        assert!(Note::HiddenError.error());
        assert!(Note::Mismatch.error());
        assert!(Note::ParseError.error());
    }

    #[test]
    fn bind_projection() {
        assert_eq!(Bind::of_stb(1), Some(Bind::Strong));
        assert_eq!(Bind::of_stb(2), Some(Bind::Weak));
        assert_eq!(Bind::of_stb(0), None);
        assert!(Bind::Weak.is_weak());
        assert!(!Bind::Strong.is_weak());
        assert_eq!(Bind::Weak.name(), "weak");
    }

    #[test]
    fn scope_visibility() {
        assert!(Scope::Global.visible());
        assert!(Scope::Weak.visible());
        assert!(!Scope::Local.visible());
        assert!(!Scope::Undef.visible());
        assert_eq!(Scope::Local.name(), "local");
    }

    #[test]
    fn defver_reach() {
        assert!(DefVer::Unversioned.reachable_unversioned());
        assert!(DefVer::Default.reachable_unversioned());
        assert!(!DefVer::NonDefault.reachable_unversioned());
        assert!(!DefVer::Unversioned.versioned());
        assert!(DefVer::Default.versioned());
        assert!(DefVer::NonDefault.versioned());
    }

    #[test]
    fn index_kinds() {
        let mut s = Shape::blank();
        s.req = VER_NDX_LOCAL;
        assert_eq!(s.req_kind(), ReqKind::Local);
        s.req = VER_NDX_GLOBAL;
        assert_eq!(s.req_kind(), ReqKind::Global);
        s.req = 2;
        assert_eq!(s.req_kind(), ReqKind::Versioned);
        s.req = VER_NDX_HIDDEN + 3;
        assert_eq!(s.req_kind(), ReqKind::Hidden);
        assert!(s.versioned());
        assert!(!ReqKind::Global.versioned());
    }

    #[test]
    fn weak_null() {
        let o = resolve(&SHAPES[0]);
        assert!(o.ok());
        assert!(o.null());
        assert!(o.weak);
        assert_eq!(o.stage, Stage::Ready);
        assert_eq!(o.note, Note::WeakNull);
        assert_eq!(Tier::of(&o), Tier::Null);
    }

    #[test]
    fn weak_sym() {
        let o = resolve(&SHAPES[1]);
        assert!(o.ok());
        assert_eq!(o.addr, 0x1000);
        assert!(!o.weak);
        assert_eq!(o.note, Note::Ready);
        assert_eq!(Tier::of(&o), Tier::Bound);
    }

    #[test]
    fn weak_version_bind() {
        let o = resolve(&SHAPES[2]);
        assert!(o.ok());
        assert_eq!(o.addr, 0x3000);
        assert!(o.weak);
        assert_eq!(o.note, Note::WeakVersion);
        assert!(o.note.weak());
    }

    #[test]
    fn strong_default() {
        let o = resolve(&SHAPES[3]);
        assert!(o.ok());
        assert_eq!(o.addr, 0x4000);
        assert!(!o.weak);
    }

    #[test]
    fn strong_version() {
        let o = resolve(&SHAPES[4]);
        assert!(o.ok());
        assert_eq!(o.addr, 0x5000);
        assert_eq!(o.note, Note::Ready);
    }

    #[test]
    fn strong_missing_version() {
        let o = resolve(&SHAPES[5]);
        assert!(o.failed());
        assert_eq!(o.stage, Stage::Version);
        assert_eq!(o.code, ERR_ENOENT);
        assert_eq!(o.note, Note::VersionError);
        assert_eq!(Tier::of(&o), Tier::VerFail);
        assert!(!o.weak);
    }

    #[test]
    fn strong_mismatch() {
        let o = resolve(&SHAPES[6]);
        assert!(o.failed());
        assert_eq!(o.stage, Stage::Bind);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(o.note, Note::Mismatch);
        assert_eq!(Tier::of(&o), Tier::SymFail);
    }

    #[test]
    fn strong_hidden() {
        let o = resolve(&SHAPES[7]);
        assert!(o.failed());
        assert_eq!(o.stage, Stage::Lookup);
        assert_eq!(o.note, Note::HiddenError);
    }

    #[test]
    fn unversioned_dso_satisfies() {
        let mut s = SHAPES[4];
        s.def_ver = DefVer::Unversioned;
        let o = resolve(&s);
        assert!(o.ok());
        assert_eq!(o.addr, 0x5000);
        assert!(!o.weak);
    }

    #[test]
    fn symbol_error_present() {
        let mut s = SHAPES[4];
        s.def = Scope::Undef;
        s.addr = 0;
        let o = resolve(&s);
        assert!(o.failed());
        assert_eq!(o.stage, Stage::Lookup);
        assert_eq!(o.note, Note::SymbolError);
    }

    #[test]
    fn weak_unresolved_unversioned() {
        let mut s = SHAPES[1];
        s.def = Scope::Undef;
        s.addr = 0;
        let o = resolve(&s);
        assert!(o.ok());
        assert!(o.null());
        assert!(o.weak);
        assert_eq!(o.note, Note::WeakNull);
    }

    #[test]
    fn local_index_strong() {
        let mut s = SHAPES[5];
        s.req = VER_NDX_LOCAL;
        let o = resolve(&s);
        assert!(o.failed());
        assert_eq!(o.stage, Stage::Parse);
        assert_eq!(o.note, Note::ParseError);
    }

    #[test]
    fn local_index_weak() {
        let mut s = SHAPES[0];
        s.req = VER_NDX_LOCAL;
        let o = resolve(&s);
        assert!(o.ok());
        assert!(o.null());
        assert!(o.weak);
    }

    #[test]
    fn empty_sym_parse_error() {
        let mut s = SHAPES[0];
        s.sym = "";
        let o = resolve(&s);
        assert!(o.failed());
        assert_eq!(o.stage, Stage::Parse);
        assert_eq!(o.code, ERR_EINVAL);
    }

    #[test]
    fn weak_nondefault_null() {
        let mut s = SHAPES[6];
        s.bind = Bind::Weak;
        let o = resolve(&s);
        assert!(o.ok());
        assert!(o.null());
        assert!(o.weak);
    }

    #[test]
    fn messages_exact() {
        let a = message(&SHAPES[5], &resolve(&SHAPES[5]));
        assert_eq!(
            a.as_str(),
            "version `GLIBC_2.34' not found (required by ./a.so)"
        );
        let b = message(&SHAPES[6], &resolve(&SHAPES[6]));
        assert_eq!(
            b.as_str(),
            "./a.so: relocation error: ./a.so: symbol foo, version GLIBC_2.34 not defined in file libdep.so with link time reference"
        );
        let c = message(&SHAPES[0], &resolve(&SHAPES[0]));
        assert_eq!(
            c.as_str(),
            "./a.so: weak undefined symbol foo resolved to NULL"
        );
        let d = message(&SHAPES[7], &resolve(&SHAPES[7]));
        assert_eq!(d.as_str(), "./a.so: undefined symbol: foo, version GLIBC_2.34");
        assert!(message(&SHAPES[1], &resolve(&SHAPES[1])).is_empty());
    }

    #[test]
    fn weak_warning_exact() {
        let m = message(&SHAPES[2], &resolve(&SHAPES[2]));
        assert_eq!(
            m.as_str(),
            "weak version `VER_9' not found (required by ./a.so)"
        );
        assert_eq!(
            msg_weak_version_not_found("VER_9", "./x.so").as_str(),
            "weak version `VER_9' not found (required by ./x.so)"
        );
    }

    #[test]
    fn judge_accept() {
        let cs = cases();
        let mut i = 0usize;
        while i < ACCEPT {
            if let Some(c) = cs.get(i) {
                assert_eq!(judge(c), Fault::Ok);
            } else {
                fail();
            }
            i += 1;
        }
    }

    #[test]
    fn judge_rejects() {
        let cs = cases();
        let mut i = 0usize;
        while i < REJECT {
            let c = match cs.get(i + ACCEPT) {
                Some(v) => *v,
                None => fail(),
            };
            let w = match REJECT_FAULTS.get(i) {
                Some(v) => *v,
                None => fail(),
            };
            assert_eq!(judge(&c), w);
            assert_eq!(judge(&c).code(), REJECT_CODES[i]);
            i += 1;
        }
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        assert_eq!(m.row(0), Some(Row { total: 1, good: 1 }));
        assert_eq!(m.row(1), Some(Row { total: 4, good: 4 }));
        assert_eq!(m.row(2), Some(Row { total: 7, good: 1 }));
        assert_eq!(m.row(3), Some(Row { total: 2, good: 2 }));
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert_eq!(m.perfect_rows(), 3);
        assert!(m.balanced());
    }

    #[test]
    fn report_render() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(r.green());
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(
            r.render().as_str(),
            "weak-nover corpus=14 accept=8 reject=6 null=1 bound=4 verfail=1 symfail=2 weak=2 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rate() {
        let r = match run_with(Some((0, Fault::Code))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(WN_ECOUNT));
        assert_eq!(run_with(Some((14, Fault::Code))).err(), Some(WN_ESLOT));
    }

    #[test]
    fn gate_steps() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Parse), Ok(()));
        assert_eq!(g.step(Stage::Version), Ok(()));
        assert_eq!(g.step(Stage::Lookup), Ok(()));
        assert_eq!(g.step(Stage::Parse), Err(WN_ESTAGE));
        assert_eq!(g.steps(), 3);
        assert_eq!(g.close(), Ok(3));
        assert_eq!(g.close(), Err(WN_ESTATE));
        assert_eq!(g.step(Stage::Ready), Err(WN_ESTATE));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), ACCEPT);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(13).is_some());
        assert!(l.sample(14).is_none());
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
        assert_eq!(l.register(Sample::blank()).err(), Some(WN_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(WN_EBAL));
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
    fn all_checks_green() {
        let s = run_unx11656_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn detail_text() {
        let a = match case_at(0) {
            Some(c) => detail(&c),
            None => fail(),
        };
        assert_eq!(a.as_str(), DETAIL_NULL);
        let b = match case_at(5) {
            Some(c) => detail(&c),
            None => fail(),
        };
        assert_eq!(b.as_str(), DETAIL_VERSION);
    }

    #[test]
    fn tags_and_cover() {
        assert_eq!(show_tags(), 14);
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(13), Err(WN_ENOBS));
        assert!(case_at(13).is_some());
        assert!(case_at(14).is_none());
    }

    fn show_tags() -> usize {
        let cs = cases();
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                if !c.tag().is_empty() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    #[test]
    fn weak_fallback_count() {
        let cs = cases();
        let mut n = 0u32;
        let mut i = 0usize;
        while i < ACCEPT {
            if let Some(c) = cs.get(i) {
                if c.actual().weak {
                    n += 1;
                }
            }
            i += 1;
        }
        assert_eq!(n, 2);
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.weak, 2);
        assert_eq!(r.weak_fallbacks(), 2);
    }

    #[test]
    fn message_bad_reference() {
        assert_eq!(
            msg_bad_reference("./a.so", "foo").as_str(),
            "./a.so: invalid symbol reference: foo (local version index)"
        );
    }

    #[test]
    fn seal_changes() {
        let mut s = SHAPES[0];
        let a = s.seal();
        s.req_weak = true;
        assert_ne!(a, s.seal());
        let mut o = resolve(&SHAPES[1]);
        let b = o.seal();
        o.addr = 0x2222;
        assert_ne!(b, o.seal());
    }
}
