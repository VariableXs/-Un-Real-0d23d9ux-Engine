//! UNX-F11653 musl 无符号版本机制静默忽略判据（双线差异锚）
//!
//! 同一条 `.so` 在 glibc 与 musl 两条装载线上会得到**互相矛盾**的结论，
//! 而矛盾不是缺陷，是两条线的版本语义根本不同：
//!
//! 1. glibc 把 `.gnu.version_r`（VERNEED）当成硬契约。依赖对象必须在自己
//!    的 VERDEF 表里给出同名版本节点，否则报
//!    ``version `GLIBC_2.34' not found (required by ./a.so)``；
//!    版本节结构坏（版本号错、链断、偏移越界）则更早死在读节阶段。
//! 2. musl **根本不读版本节**。符号绑定只看名字：`memcpy@GLIBC_2.34`
//!    与 `memcpy@GLIBC_2.2.5` 在 musl 眼里都是 `memcpy`。因此
//!    "版本缺失"在 musl 线不是错误，是**静默忽略**（silent-ignore）——
//!    连一条警告都不打。
//!
//! 于是双线差异锚（differential anchor）有三个稳定的格子：
//!
//! - `diverge`：glibc 硬错、musl 成功。凡是"符号在、版本不在"的输入都落
//!   这一格，这也是本卡的主锚。
//! - `agree`：两条线都成功（版本精确命中，或 glibc 侧带 VER_FLG_WEAK 的
//!   弱版本需求）。注意 glibc 成功时**打过版本校验**，musl 成功时**没打**，
//!   两者结论同而依据不同——判据不能只看结论就宣布两线等价。
//! - `both-fail`：符号本身缺失。glibc 报 `undefined symbol: ...`，
//!   musl 报 `Error relocating ...: symbol not found`，文案不同、结论同。
//!
//! 还有一条极易误判的纪律：**弱符号在两条线上都允许落到 NULL**。glibc 对
//! `STB_WEAK` 未定义符号不报错（地址 0），musl 同样。但如果弱符号要求的
//! 版本在 glibc 侧缺失，glibc 先死在版本校验，根本轮不到"弱符号容忍"；
//! 而 musl 因为不读版本节，会一路走到符号绑定并给出 NULL。这一格是本卡
//! 与"弱符号回退卡（F11656）"的分界线：本卡判**版本机制是否被读**，
//! F11656 判**弱符号绑定值**。
//!
//! 判据的六个维度：线（glibc/musl）、阶段（dep/read/check/bind/ready）、
//! errno、绑定值（成功非零 / NULL 为 0 / 失败为 0）、注解、差异类
//! （agree/diverge/both-fail）。少"线"这一维，两条线的结论会被当成同一
//! 模型的抖动；少"绑定值"这一维，弱符号 NULL 与"成功绑定"分不开。
//!
//! 判序：状态 → 阶段 → errno → 绑定值 → 注解 → 差异类。状态最先，因为
//! "成功/失败互报"是语义矛盾而非数值差异。
//!
//! 语料 14 槽：8 槽申报正确（静默 2、版本命中 2、硬错 3、弱档 1），6 槽
//! 各挂一处错误申报（状态/阶段/码/值/注解/差异类）。账本 14 格 8 成 6 败，
//! 冻结后重放同指纹。
//!
//! 闸门次序：依赖 → 读节 → 版本节点 → 符号基线 → 弱容忍 → 绑定 → 就绪。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本在
//! `audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11653）。
pub const SEG_BASE: u32 = 0x6ce60;

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

/// 文案上限（仅文档用，渲染走 String）。
pub const MSG_MAX: usize = 256;

/// 版本名最大字节数。
pub const VER_MAX: usize = 24;

/// 符号表容量。
pub const SYM_MAX: usize = 8;

// ---------------------------------------------------------------------------
// 装载线
// ---------------------------------------------------------------------------

/// 版本节点在依赖版本表里的三种处境。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepVer {
    /// 精确命中。
    Exact,
    /// 依赖有版本表，但没有这个节点。
    Other,
    /// 依赖根本没有版本表。
    Absent,
}

impl DepVer {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            DepVer::Exact => "exact",
            DepVer::Other => "other",
            DepVer::Absent => "absent",
        }
    }

    /// 是否命中。
    pub fn hit(&self) -> bool {
        match self {
            DepVer::Exact => true,
            _ => false,
        }
    }
}

/// 装载线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// glibc：版本节是硬契约。
    Glibc,
    /// musl：版本节被静默忽略。
    Musl,
}

impl Line {
    /// 另一条线。
    pub const fn other(&self) -> Line {
        match self {
            Line::Glibc => Line::Musl,
            Line::Musl => Line::Glibc,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Line::Glibc => "glibc",
            Line::Musl => "musl",
        }
    }

    /// 是否读版本节。
    pub fn reads_verneed(&self) -> bool {
        match self {
            Line::Glibc => true,
            Line::Musl => false,
        }
    }
}

// ---------------------------------------------------------------------------
// errno（本模型的码位约定）
// ---------------------------------------------------------------------------

/// 依赖对象不存在。
pub const ERR_ENOENT: i32 = 2;
/// 符号缺失（本模型借 ESRCH 表示，两条线都不是真 errno）。
pub const ERR_ESRCH: i32 = 3;
/// 版本节结构坏（借 ENOEXEC）。
pub const ERR_ENOEXEC: i32 = 8;
/// 版本不可匹配（借 EINVAL）。
pub const ERR_EINVAL: i32 = 22;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 阶段申报不符。
pub const MV_ESTAGE: u32 = SEG_BASE + 0x01;
/// errno 申报不符。
pub const MV_ECODE: u32 = SEG_BASE + 0x02;
/// 绑定值申报不符。
pub const MV_EVALUE: u32 = SEG_BASE + 0x03;
/// 注解申报不符。
pub const MV_ENOTE: u32 = SEG_BASE + 0x04;
/// 差异类申报不符。
pub const MV_EDIFF: u32 = SEG_BASE + 0x05;
/// 状态矛盾。
pub const MV_ESTATE: u32 = SEG_BASE + 0x06;
/// 计数不符。
pub const MV_ECOUNT: u32 = SEG_BASE + 0x07;
/// 槽位非法。
pub const MV_ESLOT: u32 = SEG_BASE + 0x08;
/// 指纹不符。
pub const MV_ESEAL: u32 = SEG_BASE + 0x09;
/// 无观测。
pub const MV_ENOBS: u32 = SEG_BASE + 0x0a;
/// 矩阵不平。
pub const MV_EMATRIX: u32 = SEG_BASE + 0x0b;
/// 阶段次序倒置。
pub const MV_EORDER: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const MV_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 依赖闸。
pub const OP_DEP: u8 = 0x01;
/// 读节闸。
pub const OP_READ: u8 = 0x02;
/// 版本节点闸。
pub const OP_CHECK: u8 = 0x03;
/// 绑定闸。
pub const OP_BIND: u8 = 0x04;
/// 线选择。
pub const OP_LINE: u8 = 0x05;
/// 差异类。
pub const OP_DIFF: u8 = 0x06;
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

/// 四字节大写十六进制（版本哈希展示）。
pub fn hash_text(h: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(h, 8).as_str());
    s
}

// ---------------------------------------------------------------------------
// 文案（逐字对齐真实装载器）
// ---------------------------------------------------------------------------

/// glibc：版本节点缺失。
pub fn msg_version_not_found(ver: &str, obj: &str) -> String {
    let mut s = String::from("version `");
    s.push_str(ver);
    s.push_str("' not found (required by ");
    s.push_str(obj);
    s.push(')');
    s
}

/// glibc：弱版本节点缺失（只告警，装载继续）。
pub fn msg_weak_version_not_found(ver: &str, obj: &str) -> String {
    let mut s = String::from("weak version `");
    s.push_str(ver);
    s.push_str("' not found (required by ");
    s.push_str(obj);
    s.push(')');
    s
}

/// glibc：版本节结构坏。
pub fn msg_bad_version_struct(dep: &str, obj: &str) -> String {
    let mut s = String::from("invalid version structure in ");
    s.push_str(dep);
    s.push_str(" (required by ");
    s.push_str(obj);
    s.push(')');
    s
}

/// glibc：符号缺失。
pub fn msg_undefined_symbol(sym: &str, ver: &str, obj: &str) -> String {
    let mut s = String::from("undefined symbol: ");
    s.push_str(sym);
    s.push_str(", version ");
    s.push_str(ver);
    s.push_str(" (");
    s.push_str(obj);
    s.push(')');
    s
}

/// glibc：依赖打不开。
pub fn msg_glibc_dep_missing(dep: &str) -> String {
    let mut s = String::from("error while loading shared libraries: ");
    s.push_str(dep);
    s.push_str(": cannot open shared object file: No such file or directory");
    s
}

/// musl：重定位期符号缺失。
pub fn msg_musl_reloc(sym: &str, obj: &str) -> String {
    let mut s = String::from("Error relocating ");
    s.push_str(obj);
    s.push_str(": ");
    s.push_str(sym);
    s.push_str(": symbol not found");
    s
}

/// musl：依赖打不开。
pub fn msg_musl_dep_missing(dep: &str, obj: &str) -> String {
    let mut s = String::from("Error loading shared library ");
    s.push_str(dep);
    s.push_str(": No such file or directory (needed by ");
    s.push_str(obj);
    s.push(')');
    s
}

// ---------------------------------------------------------------------------
// 阶段与注解
// ---------------------------------------------------------------------------

/// 装载阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 依赖解析。
    Dep,
    /// 读版本节。
    Read,
    /// 版本节点校验。
    Check,
    /// 符号绑定。
    Bind,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Dep => 0,
            Stage::Read => 1,
            Stage::Check => 2,
            Stage::Bind => 3,
            Stage::Ready => 4,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Dep => "dep",
            Stage::Read => "read",
            Stage::Check => "check",
            Stage::Bind => "bind",
            Stage::Ready => "ready",
        }
    }
}

/// 本次绑定的注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// musl 静默忽略版本节后按名字绑定。
    SilentIgnore,
    /// glibc 版本精确命中后绑定。
    Versioned,
    /// glibc 弱版本需求缺失但放行。
    WeakVersion,
    /// 弱符号落到 NULL。
    WeakNull,
    /// 版本节点缺失硬错。
    VersionError,
    /// 符号缺失硬错。
    SymbolError,
    /// 版本节结构坏。
    StructError,
    /// 依赖缺失。
    DepMissing,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::SilentIgnore => "silent-ignore",
            Note::Versioned => "versioned",
            Note::WeakVersion => "weak-version",
            Note::WeakNull => "weak-null",
            Note::VersionError => "version-error",
            Note::SymbolError => "symbol-error",
            Note::StructError => "struct-error",
            Note::DepMissing => "dep-missing",
        }
    }

    /// 是否硬失败。
    pub fn hard(&self) -> bool {
        match self {
            Note::VersionError | Note::SymbolError | Note::StructError | Note::DepMissing => true,
            _ => false,
        }
    }

    /// 是否读节阶段就能判死的注解。
    pub fn structural(&self) -> bool {
        match self {
            Note::StructError => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 结果
// ---------------------------------------------------------------------------

/// 一次双线解析的单线结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 所属线。
    pub line: Line,
    /// 走到（或死在）的阶段。
    pub stage: Stage,
    /// errno（成功为 0）。
    pub code: i32,
    /// 绑定值（NULL 与失败均为 0，靠 ok/note 区分）。
    pub value: u32,
    /// 注解。
    pub note: Note,
}

impl Outcome {
    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.stage == Stage::Ready && self.code == 0
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 是否版本机制被读。
    pub fn read_versions(&self) -> bool {
        self.line.reads_verneed()
    }

    /// 是否静默忽略。
    pub fn silent(&self) -> bool {
        self.note == Note::SilentIgnore
    }

    /// 值文本（四位十六进制）。
    pub fn value_text(&self) -> String {
        hash_text(self.value)
    }
}

/// 双线差异类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffClass {
    /// 两线结论相同（都成功）。
    Agree,
    /// 一成一败。
    Diverge,
    /// 两线都失败。
    BothFail,
}

impl DiffClass {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            DiffClass::Agree => "agree",
            DiffClass::Diverge => "diverge",
            DiffClass::BothFail => "both-fail",
        }
    }

    /// 是否要求两线结论显然不同。
    pub fn is_diverge(&self) -> bool {
        match self {
            DiffClass::Diverge => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 语料形状
// ---------------------------------------------------------------------------

/// 一条被装载对象的版本契约形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签（唯一）。
    pub tag: &'static str,
    /// 被装载对象名。
    pub obj: &'static str,
    /// 需要的符号名。
    pub sym: &'static str,
    /// 需要的版本名。
    pub need_ver: &'static str,
    /// 依赖对象名。
    pub dep: &'static str,
    /// 版本节点在依赖版本表里的处境。
    pub dep_ver: DepVer,
    /// 依赖是否导出该符号。
    pub sym_exported: bool,
    /// 符号是否弱。
    pub weak: bool,
    /// 版本需求是否带 VER_FLG_WEAK。
    pub weak_version: bool,
    /// 版本节结构是否规整。
    pub wellformed: bool,
    /// 依赖是否在场。
    pub dep_present: bool,
    /// 本槽申报的线。
    pub line: Line,
    /// 符号若解析成功的地址。
    pub addr: u32,
}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape {
            tag: "",
            obj: "",
            sym: "",
            need_ver: "",
            dep: "",
            dep_ver: DepVer::Absent,
            sym_exported: false,
            weak: false,
            weak_version: false,
            wellformed: true,
            dep_present: true,
            line: Line::Glibc,
            addr: 0,
        }
    }

    /// 本线结果。
    pub fn actual(&self) -> Outcome {
        resolve(self, self.line)
    }

    /// 另一条线结果。
    pub fn cross(&self) -> Outcome {
        resolve(self, self.line.other())
    }

    /// 双线差异类。
    pub fn diff(&self) -> DiffClass {
        differential(self)
    }

    /// 名字指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.tag);
        h = fold_str(h, self.obj);
        h = fold_str(h, self.sym);
        h = fold_str(h, self.need_ver);
        h = fold_str(h, self.dep);
        h = fold_word(h, self.addr as u64);
        h
    }
}

// ---------------------------------------------------------------------------
// 双线解析
// ---------------------------------------------------------------------------

/// glibc 线：依赖 → 版本节结构 → 版本节点 → 符号 → 弱容忍 → 绑定。
pub fn resolve_glibc(s: &Shape) -> Outcome {
    if !s.dep_present {
        return Outcome {
            line: Line::Glibc,
            stage: Stage::Dep,
            code: ERR_ENOENT,
            value: 0,
            note: Note::DepMissing,
        };
    }
    if !s.wellformed {
        return Outcome {
            line: Line::Glibc,
            stage: Stage::Read,
            code: ERR_ENOEXEC,
            value: 0,
            note: Note::StructError,
        };
    }
    if !s.dep_ver.hit() {
        if s.weak_version {
            // 弱版本需求缺失：打一行告警即放行，绑定照常做。
            return Outcome {
                line: Line::Glibc,
                stage: Stage::Ready,
                code: 0,
                value: if s.sym_exported { s.addr } else { 0 },
                note: Note::WeakVersion,
            };
        }
        return Outcome {
            line: Line::Glibc,
            stage: Stage::Check,
            code: ERR_EINVAL,
            value: 0,
            note: Note::VersionError,
        };
    }
    if !s.sym_exported {
        if s.weak {
            // 未定义弱符号允许落 NULL；glibc 不打错误。
            return Outcome {
                line: Line::Glibc,
                stage: Stage::Ready,
                code: 0,
                value: 0,
                note: Note::WeakNull,
            };
        }
        return Outcome {
            line: Line::Glibc,
            stage: Stage::Bind,
            code: ERR_ESRCH,
            value: 0,
            note: Note::SymbolError,
        };
    }
    Outcome {
        line: Line::Glibc,
        stage: Stage::Ready,
        code: 0,
        value: s.addr,
        note: Note::Versioned,
    }
}

/// musl 线：依赖 → 符号名字（版本节整段跳过）→ 弱容忍 → 绑定。
pub fn resolve_musl(s: &Shape) -> Outcome {
    if !s.dep_present {
        return Outcome {
            line: Line::Musl,
            stage: Stage::Dep,
            code: ERR_ENOENT,
            value: 0,
            note: Note::DepMissing,
        };
    }
    // 注意：wellformed 与 dep_ver 在 musl 线上**完全不参与判定**。
    if !s.sym_exported {
        if s.weak {
            return Outcome {
                line: Line::Musl,
                stage: Stage::Ready,
                code: 0,
                value: 0,
                note: Note::WeakNull,
            };
        }
        return Outcome {
            line: Line::Musl,
            stage: Stage::Bind,
            code: ERR_ESRCH,
            value: 0,
            note: Note::SymbolError,
        };
    }
    Outcome {
        line: Line::Musl,
        stage: Stage::Ready,
        code: 0,
        value: s.addr,
        note: Note::SilentIgnore,
    }
}

/// 按线解析。
pub fn resolve(s: &Shape, line: Line) -> Outcome {
    match line {
        Line::Glibc => resolve_glibc(s),
        Line::Musl => resolve_musl(s),
    }
}

/// 双线差异类。
pub fn differential(s: &Shape) -> DiffClass {
    let g = resolve_glibc(s);
    let m = resolve_musl(s);
    if g.ok() && m.ok() {
        DiffClass::Agree
    } else if g.failed() && m.failed() {
        DiffClass::BothFail
    } else {
        DiffClass::Diverge
    }
}

/// 单行文案（成功时为空串，弱档给告警，硬错给装载器原文）。
pub fn message(s: &Shape) -> String {
    match resolve(s, s.line) {
        o if o.note == Note::DepMissing => match s.line {
            Line::Glibc => msg_glibc_dep_missing(s.dep),
            Line::Musl => msg_musl_dep_missing(s.dep, s.obj),
        },
        o if o.note == Note::StructError => msg_bad_version_struct(s.dep, s.obj),
        o if o.note == Note::VersionError => msg_version_not_found(s.need_ver, s.obj),
        o if o.note == Note::WeakVersion => msg_weak_version_not_found(s.need_ver, s.obj),
        o if o.note == Note::SymbolError => match s.line {
            Line::Glibc => msg_undefined_symbol(s.sym, s.need_ver, s.obj),
            Line::Musl => msg_musl_reloc(s.sym, s.obj),
        },
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 判据档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 静默忽略档。
    Silent,
    /// 版本命中档。
    Versioned,
    /// 硬错档。
    Error,
    /// 弱档（弱版本 / 弱符号 NULL）。
    Weak,
}

impl Tier {
    /// 由结果归位。
    pub fn of(o: &Outcome) -> Tier {
        if o.note == Note::WeakNull || o.note == Note::WeakVersion {
            return Tier::Weak;
        }
        if o.ok() {
            if o.silent() {
                return Tier::Silent;
            }
            return Tier::Versioned;
        }
        Tier::Error
    }

    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Tier::Silent => 0,
            Tier::Versioned => 1,
            Tier::Error => 2,
            Tier::Weak => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Silent => "silent",
            Tier::Versioned => "versioned",
            Tier::Error => "error",
            Tier::Weak => "weak",
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
    /// 阶段错。
    Stage,
    /// errno 错。
    Code,
    /// 绑定值错。
    Value,
    /// 注解错。
    Note,
    /// 差异类错。
    Diff,
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
            Fault::Stage => MV_ESTAGE,
            Fault::Code => MV_ECODE,
            Fault::Value => MV_EVALUE,
            Fault::Note => MV_ENOTE,
            Fault::Diff => MV_EDIFF,
            Fault::Estate => MV_ESTATE,
            Fault::Count => MV_ECOUNT,
            Fault::Slot => MV_ESLOT,
            Fault::Seal => MV_ESEAL,
            Fault::Nobs => MV_ENOBS,
            Fault::Matrix => MV_EMATRIX,
            Fault::Order => MV_EORDER,
            Fault::Bal => MV_EBAL,
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
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Value => "value",
        Fault::Note => "note",
        Fault::Diff => "diff",
        Fault::Estate => "estate",
        Fault::Count => "count",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Order => "order",
        Fault::Bal => "bal",
    }
}

// ---------------------------------------------------------------------------
// 申报
// ---------------------------------------------------------------------------

/// 一条申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 申报阶段。
    pub stage: Stage,
    /// 申报 errno。
    pub code: i32,
    /// 申报绑定值。
    pub value: u32,
    /// 申报注解。
    pub note: Note,
    /// 申报差异类。
    pub diff: DiffClass,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            stage: Stage::Dep,
            code: 0,
            value: 0,
            note: Note::SilentIgnore,
            diff: DiffClass::Agree,
        }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.stage == Stage::Ready && self.code == 0
    }
}

/// 一个语料槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 空白槽。
    pub const fn blank() -> Case {
        Case { shape: Shape::blank(), claim: Claim::blank() }
    }

    /// 标签。
    pub fn tag(&self) -> &'static str {
        self.shape.tag
    }

    /// 实际结果。
    pub fn actual(&self) -> Outcome {
        self.shape.actual()
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
        tag: "glibc-exact",
        obj: "./a.so",
        sym: "memcpy",
        need_ver: "GLIBC_2.14",
        dep: "libc.so.6",
        dep_ver: DepVer::Exact,
        sym_exported: true,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Glibc,
        addr: 0x7100,
    },
    Shape {
        tag: "glibc-ver-miss",
        obj: "./a.so",
        sym: "memcpy",
        need_ver: "GLIBC_2.34",
        dep: "libc.so.6",
        dep_ver: DepVer::Other,
        sym_exported: true,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Glibc,
        addr: 0x7100,
    },
    Shape {
        tag: "musl-ver-miss",
        obj: "./a.so",
        sym: "memcpy",
        need_ver: "GLIBC_2.34",
        dep: "libc.so.6",
        dep_ver: DepVer::Other,
        sym_exported: true,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Musl,
        addr: 0x7100,
    },
    Shape {
        tag: "glibc-sym-miss",
        obj: "./b.so",
        sym: "puts",
        need_ver: "GLIBC_2.2.5",
        dep: "libc.so.6",
        dep_ver: DepVer::Exact,
        sym_exported: false,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Glibc,
        addr: 0,
    },
    Shape {
        tag: "musl-sym-miss",
        obj: "./b.so",
        sym: "puts",
        need_ver: "GLIBC_2.2.5",
        dep: "libc.so.6",
        dep_ver: DepVer::Exact,
        sym_exported: false,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Musl,
        addr: 0,
    },
    Shape {
        tag: "musl-weak-null",
        obj: "./c.so",
        sym: "old_api",
        need_ver: "VER_9",
        dep: "libdep.so",
        dep_ver: DepVer::Other,
        sym_exported: false,
        weak: true,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Musl,
        addr: 0,
    },
    Shape {
        tag: "glibc-weak-ver",
        obj: "./c.so",
        sym: "old_api",
        need_ver: "VER_9",
        dep: "libdep.so",
        dep_ver: DepVer::Other,
        sym_exported: true,
        weak: true,
        weak_version: true,
        wellformed: true,
        dep_present: true,
        line: Line::Glibc,
        addr: 0x7140,
    },
    Shape {
        tag: "musl-bad-struct",
        obj: "./d.so",
        sym: "xstat",
        need_ver: "GLIBC_2.33",
        dep: "libc.so.6",
        dep_ver: DepVer::Absent,
        sym_exported: true,
        weak: false,
        weak_version: false,
        wellformed: false,
        dep_present: true,
        line: Line::Musl,
        addr: 0x7120,
    },
    Shape {
        tag: "glibc-as-silent",
        obj: "./a.so",
        sym: "memcpy",
        need_ver: "GLIBC_2.34",
        dep: "libc.so.6",
        dep_ver: DepVer::Other,
        sym_exported: true,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Glibc,
        addr: 0x7100,
    },
    Shape {
        tag: "glibc-stage-swap",
        obj: "./a.so",
        sym: "memcpy",
        need_ver: "GLIBC_2.34",
        dep: "libc.so.6",
        dep_ver: DepVer::Other,
        sym_exported: true,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Glibc,
        addr: 0x7100,
    },
    Shape {
        tag: "glibc-code-swap",
        obj: "./a.so",
        sym: "memcpy",
        need_ver: "GLIBC_2.34",
        dep: "libc.so.6",
        dep_ver: DepVer::Other,
        sym_exported: true,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Glibc,
        addr: 0x7100,
    },
    Shape {
        tag: "musl-note-swap",
        obj: "./a.so",
        sym: "memcpy",
        need_ver: "GLIBC_2.34",
        dep: "libc.so.6",
        dep_ver: DepVer::Other,
        sym_exported: true,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Musl,
        addr: 0x7100,
    },
    Shape {
        tag: "musl-value-swap",
        obj: "./a.so",
        sym: "memcpy",
        need_ver: "GLIBC_2.34",
        dep: "libc.so.6",
        dep_ver: DepVer::Other,
        sym_exported: true,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Musl,
        addr: 0x7100,
    },
    Shape {
        tag: "musl-diff-swap",
        obj: "./a.so",
        sym: "memcpy",
        need_ver: "GLIBC_2.34",
        dep: "libc.so.6",
        dep_ver: DepVer::Other,
        sym_exported: true,
        weak: false,
        weak_version: false,
        wellformed: true,
        dep_present: true,
        line: Line::Musl,
        addr: 0x7100,
    },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        stage: Stage::Ready,
        code: 0,
        value: 0x7100,
        note: Note::Versioned,
        diff: DiffClass::Agree,
    },
    Claim {
        stage: Stage::Check,
        code: ERR_EINVAL,
        value: 0,
        note: Note::VersionError,
        diff: DiffClass::Diverge,
    },
    Claim {
        stage: Stage::Ready,
        code: 0,
        value: 0x7100,
        note: Note::SilentIgnore,
        diff: DiffClass::Diverge,
    },
    Claim {
        stage: Stage::Bind,
        code: ERR_ESRCH,
        value: 0,
        note: Note::SymbolError,
        diff: DiffClass::BothFail,
    },
    Claim {
        stage: Stage::Bind,
        code: ERR_ESRCH,
        value: 0,
        note: Note::SymbolError,
        diff: DiffClass::BothFail,
    },
    Claim {
        stage: Stage::Ready,
        code: 0,
        value: 0,
        note: Note::WeakNull,
        diff: DiffClass::Diverge,
    },
    Claim {
        stage: Stage::Ready,
        code: 0,
        value: 0x7140,
        note: Note::WeakVersion,
        diff: DiffClass::Agree,
    },
    Claim {
        stage: Stage::Ready,
        code: 0,
        value: 0x7120,
        note: Note::SilentIgnore,
        diff: DiffClass::Diverge,
    },
    Claim {
        stage: Stage::Ready,
        code: 0,
        value: 0,
        note: Note::SilentIgnore,
        diff: DiffClass::Diverge,
    },
    Claim {
        stage: Stage::Bind,
        code: ERR_EINVAL,
        value: 0,
        note: Note::VersionError,
        diff: DiffClass::Diverge,
    },
    Claim {
        stage: Stage::Check,
        code: ERR_ENOEXEC,
        value: 0,
        note: Note::VersionError,
        diff: DiffClass::Diverge,
    },
    Claim {
        stage: Stage::Ready,
        code: 0,
        value: 0x7100,
        note: Note::Versioned,
        diff: DiffClass::Diverge,
    },
    Claim {
        stage: Stage::Ready,
        code: 0,
        value: 0x7108,
        note: Note::SilentIgnore,
        diff: DiffClass::Diverge,
    },
    Claim {
        stage: Stage::Ready,
        code: 0,
        value: 0x7100,
        note: Note::SilentIgnore,
        diff: DiffClass::Agree,
    },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Code, Fault::Note, Fault::Value, Fault::Diff];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    MV_ESTATE,
    MV_ESTAGE,
    MV_ECODE,
    MV_ENOTE,
    MV_EVALUE,
    MV_EDIFF,
];

/// 账本 8 个成功格的值（语料总数、成、败、静默、版本、硬错、弱、红档 ppm）。
pub const VALS: [u32; ACCEPT] = [14, 8, 6, 2, 1, 3, 2, 929];

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

/// 取第 i 槽。
pub fn case_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    cases().get(i).copied()
}

/// 覆盖校验。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(MV_ENOBS)
    }
}

/// 第 i 槽的设计期望。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        Some(Fault::Ok)
    } else {
        REJECT_FAULTS.get(i - ACCEPT).copied()
    }
}

/// 对判：状态 → 阶段 → errno → 值 → 注解 → 差异类。
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
    if claim.value != a.value {
        return Fault::Value;
    }
    if claim.note != a.note {
        return Fault::Note;
    }
    if claim.diff != c.shape.diff() {
        return Fault::Diff;
    }
    Fault::Ok
}

// ---------------------------------------------------------------------------
// 释义
// ---------------------------------------------------------------------------

/// 单行释义。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from(c.shape.obj);
    s.push_str(" line=");
    s.push_str(a.line.name());
    s.push_str(" need=");
    s.push_str(c.shape.need_ver);
    s.push_str(" dep=");
    s.push_str(c.shape.dep);
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" code=");
    s.push_str(i32_text(a.code).as_str());
    s.push_str(" value=");
    s.push_str(a.value_text().as_str());
    s.push_str(" note=");
    s.push_str(a.note.name());
    s.push_str(" diff=");
    s.push_str(c.shape.diff().name());
    let m = message(&c.shape);
    if !m.is_empty() {
        s.push_str(" msg=\"");
        s.push_str(m.as_str());
        s.push('"');
    }
    s.push('\n');
    s
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
    /// 行（silent/versioned/error/weak）。
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
                let t = Tier::of(&c.actual()).index();
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

    /// 是否平。
    pub fn balanced(&self) -> bool {
        self.total() == CORPUS && self.good() == ACCEPT
    }

    /// 取一行。
    pub fn row(&self, i: usize) -> Option<Row> {
        self.rows.get(i).copied()
    }

    /// 全对行数。
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
    /// 静默档正确数。
    pub silent: u32,
    /// 版本档正确数。
    pub versioned: u32,
    /// 硬错档正确数。
    pub error: u32,
    /// 弱档正确数。
    pub weak: u32,
    /// 对判命中数。
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

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.green() {
            Ok(())
        } else {
            Err(MV_ECOUNT)
        }
    }

    /// 差异类计数。
    pub fn diverge_count(&self) -> u32 {
        let cs = cases();
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                if c.shape.diff() == DiffClass::Diverge {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("musl-ver corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" silent=");
        s.push_str(u32_text(self.silent).as_str());
        s.push_str(" versioned=");
        s.push_str(u32_text(self.versioned).as_str());
        s.push_str(" error=");
        s.push_str(u32_text(self.error).as_str());
        s.push_str(" weak=");
        s.push_str(u32_text(self.weak).as_str());
        s.push_str(" diverge=");
        s.push_str(u32_text(self.diverge_count()).as_str());
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
            return Err(MV_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut silent = 0u32;
    let mut versioned = 0u32;
    let mut error = 0u32;
    let mut weak = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(MV_ENOBS),
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
            None => return Err(MV_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(&a) {
                Tier::Silent => silent += 1,
                Tier::Versioned => versioned += 1,
                Tier::Error => error += 1,
                Tier::Weak => weak += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, silent, versioned, error, weak, matched })
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
            return Err(MV_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(MV_EORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(MV_ESTATE);
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
            return Err(MV_ESLOT);
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
            return Err(MV_EBAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MV_EBAL),
            };
            if s.slot as usize != i {
                return Err(MV_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MV_EVALUE);
                        }
                    }
                    None => return Err(MV_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MV_EVALUE);
                        }
                    }
                    None => return Err(MV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(MV_EBAL);
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
            return Err(MV_EBAL);
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
            Err(MV_ESEAL)
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
    let _ = l.register(count_cell(3, OP_LINE, r.map(|x| x.silent)));
    let _ = l.register(count_cell(4, OP_CHECK, r.map(|x| x.versioned)));
    let _ = l.register(count_cell(5, OP_BIND, r.map(|x| x.error)));
    let _ = l.register(count_cell(6, OP_DIFF, r.map(|x| x.weak)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_READ, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_DEP,
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
    out[0] = name_ok() && ver_ok() && shape_ok();
    out[1] = resolve_ok() && judge_ok() && matrix_ok();
    out[2] = corpus_ok() && reds_ok() && detail_ok() && slots_ok();
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
        MV_ESTAGE, MV_ECODE, MV_EVALUE, MV_ENOTE, MV_EDIFF, MV_ESTATE, MV_ECOUNT, MV_ESLOT,
        MV_ESEAL, MV_ENOBS, MV_EMATRIX, MV_EORDER, MV_EBAL,
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
    let a = SEG_BASE == 0x6ce60;
    let b = MV_ESTAGE == SEG_BASE + 0x01 && MV_EBAL == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = ERR_ENOENT == 2 && ERR_ESRCH == 3 && ERR_ENOEXEC == 8 && ERR_EINVAL == 22;
    a && b && c && d
}

/// 版本语义常量。
fn ver_ok() -> bool {
    let a = DepVer::Exact.hit() && !DepVer::Other.hit() && !DepVer::Absent.hit();
    let b = DepVer::Exact.name() == "exact"
        && DepVer::Other.name() == "other"
        && DepVer::Absent.name() == "absent";
    let c = Line::Glibc.reads_verneed() && !Line::Musl.reads_verneed();
    let d = Line::Glibc.other() == Line::Musl && Line::Musl.other() == Line::Glibc;
    let e = Line::Glibc.name() == "glibc" && Line::Musl.name() == "musl";
    let f = hash_text(0x7100).as_str() == "0x00007100";
    let g = hash_text(0).as_str() == "0x00000000";
    a && b && c && d && e && f && g
}

/// 形状与差异类。
fn shape_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    let mut uniq = true;
    while i < CORPUS {
        let a = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.tag().is_empty() {
            return false;
        }
        let mut j = 0usize;
        while j < i {
            if let Some(p) = cs.get(j) {
                if p.tag() == a.tag() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let d0 = match cs.get(0) {
        Some(c) => c.shape.diff() == DiffClass::Agree,
        None => false,
    };
    let d1 = match cs.get(1) {
        Some(c) => c.shape.diff() == DiffClass::Diverge,
        None => false,
    };
    let d3 = match cs.get(3) {
        Some(c) => c.shape.diff() == DiffClass::BothFail,
        None => false,
    };
    let d5 = match cs.get(5) {
        Some(c) => c.shape.diff() == DiffClass::Diverge,
        None => false,
    };
    let d6 = match cs.get(6) {
        Some(c) => c.shape.diff() == DiffClass::Agree,
        None => false,
    };
    let seal_a = match cs.get(0) {
        Some(c) => c.shape.seal() != 0 && c.shape.seal() == c.shape.seal(),
        None => false,
    };
    let seal_b = match (cs.get(0), cs.get(1)) {
        (Some(a), Some(b)) => a.shape.seal() != b.shape.seal(),
        _ => false,
    };
    uniq && d0 && d1 && d3 && d5 && d6 && seal_a && seal_b
}

/// 双线解析。
fn resolve_ok() -> bool {
    let g = resolve(&SHAPES[1], Line::Glibc);
    let a = g.stage == Stage::Check && g.code == ERR_EINVAL && g.note == Note::VersionError;
    let m = resolve(&SHAPES[1], Line::Musl);
    let b = m.ok() && m.value == 0x7100 && m.note == Note::SilentIgnore;
    let c = resolve(&SHAPES[3], Line::Glibc).note == Note::SymbolError;
    let d = resolve(&SHAPES[3], Line::Musl).note == Note::SymbolError;
    let e = {
        let w = resolve(&SHAPES[5], Line::Musl);
        w.ok() && w.value == 0 && w.note == Note::WeakNull
    };
    let f = {
        let w = resolve(&SHAPES[6], Line::Glibc);
        w.ok() && w.value == 0x7140 && w.note == Note::WeakVersion
    };
    let h = {
        let s = resolve(&SHAPES[7], Line::Musl);
        s.ok() && s.note == Note::SilentIgnore
    };
    let i = {
        let s = resolve(&SHAPES[7], Line::Glibc);
        s.stage == Stage::Read && s.code == ERR_ENOEXEC && s.note == Note::StructError
    };
    // musl 线对坏结构不读节；glibc 线对坏结构一定死在 check 之前。
    let j = !resolve(&SHAPES[7], Line::Musl).read_versions()
        && resolve(&SHAPES[7], Line::Glibc).read_versions();
    a && b && c && d && e && f && h && i && j
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
    m.row(0) == Some(Row { total: 5, good: 2 })
        && m.row(1) == Some(Row { total: 1, good: 1 })
        && m.row(2) == Some(Row { total: 6, good: 3 })
        && m.row(3) == Some(Row { total: 2, good: 2 })
        && m.row(4).is_none()
        && m.total() == CORPUS
        && m.good() == ACCEPT
        && m.balanced()
        && m.perfect_rows() == 2
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
        if a.ok() && a.note.hard() {
            return false;
        }
        if a.failed() && !a.note.hard() {
            return false;
        }
        if a.ok() && a.value == 0 && a.note != Note::WeakNull {
            return false;
        }
        if a.failed() && a.value != 0 {
            return false;
        }
        if a.note.structural() && a.stage != Stage::Read {
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
    let b = r.silent == 2 && r.versioned == 1 && r.error == 3 && r.weak == 2;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = tags_unique() && notes_ok() && table_ok();
    let e = r.diverge_count() == 10;
    a && b && c && d && e
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929;
    let b = r.gate() == Err(MV_ECOUNT) && !r.green();
    let c = run_with(Some((CORPUS, Fault::Code))).err() == Some(MV_ESLOT);
    a && b && c
}

/// 逐字文案。
const DETAIL_VER: &str =
    "./a.so line=glibc need=GLIBC_2.34 dep=libc.so.6 stage=check code=22 value=0x00000000 note=version-error diff=diverge msg=\"version `GLIBC_2.34' not found (required by ./a.so)\"\n";
/// musl 静默释义。
const DETAIL_SILENT: &str =
    "./a.so line=musl need=GLIBC_2.34 dep=libc.so.6 stage=ready code=0 value=0x00007100 note=silent-ignore diff=diverge\n";

/// 释义。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(1) {
        Some(c) => detail(c).as_str() == DETAIL_VER,
        None => false,
    };
    let b = match cs.get(2) {
        Some(c) => detail(c).as_str() == DETAIL_SILENT,
        None => false,
    };
    let c = match cs.get(5) {
        Some(c) => {
            let m = message(&c.shape);
            m.is_empty()
        }
        None => false,
    };
    let d = match cs.get(6) {
        Some(c) => {
            message(&c.shape).as_str()
                == "weak version `VER_9' not found (required by ./c.so)"
        }
        None => false,
    };
    let e = msg_version_not_found("GLIBC_2.34", "./a.so").as_str()
        == "version `GLIBC_2.34' not found (required by ./a.so)";
    let f = msg_undefined_symbol("memcpy", "GLIBC_2.34", "./a.so").as_str()
        == "undefined symbol: memcpy, version GLIBC_2.34 (./a.so)";
    let g = msg_musl_reloc("memcpy", "./a.so").as_str()
        == "Error relocating ./a.so: memcpy: symbol not found";
    let h = msg_glibc_dep_missing("libc.so.6").as_str()
        == "error while loading shared libraries: libc.so.6: cannot open shared object file: No such file or directory";
    let i = msg_musl_dep_missing("libc.so.6", "./a.so").as_str()
        == "Error loading shared library libc.so.6: No such file or directory (needed by ./a.so)";
    a && b && c && d && e && f && g && h && i
}

/// 槽号与档位。
fn slots_ok() -> bool {
    let a = match case_at(0) {
        Some(c) => c.tag() == "glibc-exact",
        None => false,
    };
    let b = match case_at(13) {
        Some(c) => c.tag() == "musl-diff-swap",
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(MV_ENOBS);
    let e = Stage::Dep.order() == 0
        && Stage::Read.order() == 1
        && Stage::Check.order() == 2
        && Stage::Bind.order() == 3
        && Stage::Ready.order() == 4;
    let f = Stage::Read.name() == "read" && Stage::Check.name() == "check";
    let g = Tier::Silent.index() == 0
        && Tier::Versioned.index() == 1
        && Tier::Error.index() == 2
        && Tier::Weak.index() == 3;
    let h = Tier::Silent.name() == "silent"
        && Tier::Versioned.name() == "versioned"
        && Tier::Error.name() == "error"
        && Tier::Weak.name() == "weak";
    let i = fault_short(Fault::Diff) == "diff"
        && fault_short(Fault::Value) == "value"
        && fault_short(Fault::Ok) == "ok";
    let j = Note::SilentIgnore.name() == "silent-ignore"
        && Note::VersionError.name() == "version-error"
        && Note::WeakNull.name() == "weak-null";
    let k = Note::StructError.structural() && !Note::SymbolError.structural();
    let l = Note::VersionError.hard()
        && Note::SymbolError.hard()
        && !Note::Versioned.hard()
        && !Note::WeakVersion.hard();
    a && b && c && d && e && f && g && h && i && j && k && l
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Read) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Dep) == Err(MV_EORDER) && g.steps() == 1;
    let d = g.step(Stage::Check) == Ok(()) && g.last() == 2 && g.steps() == 2;
    let e = g.close() == Ok(2) && g.closed();
    let f = g.close() == Err(MV_ESTATE);
    let h = g.step(Stage::Ready) == Err(MV_ESTATE);
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
pub fn run_unx11653_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11653_musl_version_silent");

    s.add("MV01-名称与段基址", name_ok(), "static");

    s.add("MV02-版本语义常量", ver_ok(), "static");

    s.add("MV03-形状与差异类", shape_ok(), "static");

    s.add("MV04-双线解析", resolve_ok(), "static");

    s.add("MV05-申报对判", judge_ok(), "static");

    s.add("MV06-矩阵统计", matrix_ok(), "static");

    s.add("MV07-语料与渲染", corpus_ok(), "static");

    s.add("MV08-红档显式", reds_ok(), "static");

    s.add("MV09-释义逐字", detail_ok(), "static");

    s.add("MV10-槽号与档位", slots_ok(), "static");

    s.add("MV11-阶段闸门", gate_ok(), "static");

    s.add("MV12-账本对账", ledger_ok(), "static");

    s.add(
        "MV13-冻结重放四注入",
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
        panic!("unx11653 test fail")
    }

    #[test]
    fn consts() {
        assert_eq!(SEG_BASE, 0x6ce60);
        assert_eq!(SAMPLES, 14);
        assert_eq!(CORPUS, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(VALS[7], 929);
    }

    #[test]
    fn errno_values() {
        assert_eq!(ERR_ENOENT, 2);
        assert_eq!(ERR_ESRCH, 3);
        assert_eq!(ERR_ENOEXEC, 8);
        assert_eq!(ERR_EINVAL, 22);
    }

    #[test]
    fn error_codes_spaced() {
        assert_eq!(MV_ESTAGE, 0x6ce61);
        assert_eq!(MV_EDIFF, 0x6ce65);
        assert_eq!(MV_EBAL, 0x6ce6d);
        assert_eq!(MV_ESTAGE + 12, MV_EBAL);
    }

    #[test]
    fn depver_semantics() {
        assert!(DepVer::Exact.hit());
        assert!(!DepVer::Other.hit());
        assert!(!DepVer::Absent.hit());
        assert_eq!(DepVer::Exact.name(), "exact");
        assert_eq!(DepVer::Absent.name(), "absent");
    }

    #[test]
    fn line_semantics() {
        assert!(Line::Glibc.reads_verneed());
        assert!(!Line::Musl.reads_verneed());
        assert_eq!(Line::Glibc.other(), Line::Musl);
        assert_eq!(Line::Musl.other(), Line::Glibc);
        assert_eq!(Line::Musl.name(), "musl");
    }

    #[test]
    fn stage_order() {
        assert_eq!(Stage::Dep.order(), 0);
        assert_eq!(Stage::Read.order(), 1);
        assert_eq!(Stage::Check.order(), 2);
        assert_eq!(Stage::Bind.order(), 3);
        assert_eq!(Stage::Ready.order(), 4);
        assert_eq!(Stage::Bind.name(), "bind");
    }

    #[test]
    fn notes_text() {
        assert_eq!(Note::SilentIgnore.name(), "silent-ignore");
        assert_eq!(Note::Versioned.name(), "versioned");
        assert_eq!(Note::WeakVersion.name(), "weak-version");
        assert_eq!(Note::WeakNull.name(), "weak-null");
        assert_eq!(Note::VersionError.name(), "version-error");
        assert_eq!(Note::StructError.name(), "struct-error");
    }

    #[test]
    fn glibc_missing_version() {
        let o = resolve(&SHAPES[1], Line::Glibc);
        assert!(o.failed());
        assert_eq!(o.stage, Stage::Check);
        assert_eq!(o.code, ERR_EINVAL);
        assert_eq!(o.note, Note::VersionError);
        assert_eq!(o.value, 0);
    }

    #[test]
    fn musl_ignores_version() {
        let o = resolve(&SHAPES[2], Line::Musl);
        assert!(o.ok());
        assert_eq!(o.stage, Stage::Ready);
        assert_eq!(o.value, 0x7100);
        assert_eq!(o.note, Note::SilentIgnore);
        assert!(!o.read_versions());
    }

    #[test]
    fn musl_ignores_bad_structure() {
        let o = resolve(&SHAPES[7], Line::Musl);
        assert!(o.ok());
        assert_eq!(o.note, Note::SilentIgnore);
    }

    #[test]
    fn glibc_rejects_bad_structure() {
        let o = resolve(&SHAPES[7], Line::Glibc);
        assert!(o.failed());
        assert_eq!(o.stage, Stage::Read);
        assert_eq!(o.code, ERR_ENOEXEC);
        assert_eq!(o.note, Note::StructError);
    }

    #[test]
    fn musl_weak_null() {
        let o = resolve(&SHAPES[5], Line::Musl);
        assert!(o.ok());
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::WeakNull);
    }

    #[test]
    fn glibc_weak_version_tolerated() {
        let o = resolve(&SHAPES[6], Line::Glibc);
        assert!(o.ok());
        assert_eq!(o.value, 0x7140);
        assert_eq!(o.note, Note::WeakVersion);
        assert_eq!(Tier::of(&o), Tier::Weak);
    }

    #[test]
    fn glibc_symbol_missing() {
        let o = resolve(&SHAPES[3], Line::Glibc);
        assert_eq!(o.stage, Stage::Bind);
        assert_eq!(o.code, ERR_ESRCH);
        assert_eq!(o.note, Note::SymbolError);
    }

    #[test]
    fn musl_symbol_missing() {
        let o = resolve(&SHAPES[4], Line::Musl);
        assert_eq!(o.stage, Stage::Bind);
        assert_eq!(o.note, Note::SymbolError);
    }

    #[test]
    fn glibc_dep_missing_message() {
        let mut s = SHAPES[0];
        s.dep_present = false;
        let o = resolve(&s, Line::Glibc);
        assert_eq!(o.stage, Stage::Dep);
        assert_eq!(o.code, ERR_ENOENT);
        assert!(message(&s).as_str().contains("cannot open shared object file"));
    }

    #[test]
    fn musl_dep_missing_message() {
        let mut s = SHAPES[0];
        s.dep_present = false;
        s.line = Line::Musl;
        assert_eq!(resolve(&s, Line::Musl).code, ERR_ENOENT);
        assert_eq!(
            message(&s).as_str(),
            "Error loading shared library libc.so.6: No such file or directory (needed by ./a.so)"
        );
    }

    #[test]
    fn messages_exact() {
        assert_eq!(
            message(&SHAPES[1]).as_str(),
            "version `GLIBC_2.34' not found (required by ./a.so)"
        );
        assert_eq!(
            message(&SHAPES[3]).as_str(),
            "undefined symbol: puts, version GLIBC_2.2.5 (./b.so)"
        );
        assert_eq!(
            message(&SHAPES[4]).as_str(),
            "Error relocating ./b.so: puts: symbol not found"
        );
        let mut bad = SHAPES[7];
        bad.line = Line::Glibc;
        assert_eq!(
            message(&bad).as_str(),
            "invalid version structure in libc.so.6 (required by ./d.so)"
        );
        assert_eq!(message(&SHAPES[0]).as_str(), "");
    }

    #[test]
    fn differential_classes() {
        assert_eq!(differential(&SHAPES[0]), DiffClass::Agree);
        assert_eq!(differential(&SHAPES[1]), DiffClass::Diverge);
        assert_eq!(differential(&SHAPES[3]), DiffClass::BothFail);
        assert_eq!(differential(&SHAPES[5]), DiffClass::Diverge);
        assert_eq!(differential(&SHAPES[6]), DiffClass::Agree);
        assert!(DiffClass::Diverge.is_diverge());
        assert!(!DiffClass::Agree.is_diverge());
    }

    #[test]
    fn tiers() {
        assert_eq!(Tier::of(&resolve(&SHAPES[2], Line::Musl)), Tier::Silent);
        assert_eq!(Tier::of(&resolve(&SHAPES[0], Line::Glibc)), Tier::Versioned);
        assert_eq!(Tier::of(&resolve(&SHAPES[1], Line::Glibc)), Tier::Error);
        assert_eq!(Tier::of(&resolve(&SHAPES[5], Line::Musl)), Tier::Weak);
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
    fn judge_rejects() {
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
    fn reject_specifics() {
        let cs = cases();
        assert_eq!(judge(&cs[8]), Fault::Estate);
        assert_eq!(judge(&cs[9]), Fault::Stage);
        assert_eq!(judge(&cs[10]), Fault::Code);
        assert_eq!(judge(&cs[11]), Fault::Note);
        assert_eq!(judge(&cs[12]), Fault::Value);
        assert_eq!(judge(&cs[13]), Fault::Diff);
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert!(m.balanced());
        assert_eq!(m.row(0), Some(Row { total: 5, good: 2 }));
        assert_eq!(m.row(1), Some(Row { total: 1, good: 1 }));
        assert_eq!(m.row(2), Some(Row { total: 6, good: 3 }));
        assert_eq!(m.row(3), Some(Row { total: 2, good: 2 }));
        assert_eq!(m.perfect_rows(), 2);
    }

    #[test]
    fn matrix_seal() {
        assert_eq!(Matrix::of().seal(), Matrix::of().seal());
        assert_ne!(Matrix::of().seal(), Matrix::blank().seal());
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
        assert_eq!(r.silent, 2);
        assert_eq!(r.versioned, 1);
        assert_eq!(r.error, 3);
        assert_eq!(r.weak, 2);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.diverge_count(), 10);
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "musl-ver corpus=14 accept=8 reject=6 silent=2 versioned=1 error=3 weak=2 diverge=10 match=14 rate=1000ppm\n"
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
        assert_eq!(r.gate(), Err(MV_ECOUNT));
        assert_eq!(run_with(Some((14, Fault::Code))).err(), Some(MV_ESLOT));
    }

    #[test]
    fn detail_exact() {
        let cs = cases();
        assert_eq!(detail(&cs[1]).as_str(), DETAIL_VER);
        assert_eq!(detail(&cs[2]).as_str(), DETAIL_SILENT);
        assert!(detail(&cs[1]).as_str().contains("msg=\""));
    }

    #[test]
    fn slot_bounds() {
        assert!(case_at(0).is_some());
        assert!(case_at(CORPUS - 1).is_some());
        assert!(case_at(CORPUS).is_none());
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(2), Err(MV_ENOBS));
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(fault_short(Fault::Diff), "diff");
    }

    #[test]
    fn gate_steps() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Read), Ok(()));
        assert_eq!(g.step(Stage::Dep), Err(MV_EORDER));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.step(Stage::Check), Ok(()));
        assert_eq!(g.last(), 2);
    }

    #[test]
    fn gate_close() {
        let mut g = Gate::new();
        assert!(g.close().is_ok());
        assert_eq!(g.close(), Err(MV_ESTATE));
        assert_eq!(g.step(Stage::Ready), Err(MV_ESTATE));
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
        assert_eq!(l.register(Sample::blank()).err(), Some(MV_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(MV_EBAL));
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
        assert_eq!(i32_text(-22).as_str(), "-22");
        assert_eq!(i32_text(13).as_str(), "13");
        assert_eq!(hash_text(0x6ce60).as_str(), "0x0006ce60");
        assert_eq!(fault_short(Fault::Estate), "estate");
    }

    #[test]
    fn fold_helpers() {
        let a = fold_str(FNV_OFF, "memcpy");
        let b = fold_str(FNV_OFF, "memcpy");
        let c = fold_str(FNV_OFF, "memcpz");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(fold_word(FNV_OFF, 1), fold_word(FNV_OFF, 2));
        assert_ne!(fold_byte(FNV_OFF, 1), fold_byte(FNV_OFF, 2));
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11653_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
