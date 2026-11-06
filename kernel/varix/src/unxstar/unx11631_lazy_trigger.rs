//! UNX-F11631 RTLD_LAZY 延迟绑定触发点判据（对照面）
//!
//! 闸门次序：装载 → PLT/GOT 自洽 → 首调触发 → 解析 → 补槽 → 复审。
//! 契约：纯 no_std 风格（仅 `alloc::string::String` 渲染），无 unsafe、无 unwrap/expect/panic!，失败一律 `Result<_, u32>`，域码 + 状态码恰 13 个。
//!
//! `dlopen(path, RTLD_LAZY)` 和 `RTLD_NOW` 的差别不在"能不能解析"，而在
//! **解析发生在哪一刻**。`RTLD_NOW` 在装载期把 `JUMP_SLOT` 全部写实；`RTLD_LAZY`
//! 把 `JUMP_SLOT` 指向 PLT0 的解析桩，直到该槽**第一次被调用**才进入
//! `_dl_runtime_resolve`：查作用域、做版本校验、把目标地址补进 GOT，然后跳过去。
//! 第二次调用走已补好的槽，不再进解析器。判据必须钉住这条时间线：
//!
//! - 装载后、首调前，函数槽仍是桩（装载期零解析）；
//! - 首调时触发一次解析，GOT 槽在返回前补好；
//! - 同槽二次调用直接命中，触发计数保持 1；
//! - 不同槽各自独立触发，互不串扰；
//! - 数据重定位（`GLOB_DAT`）不参与延迟，装载期就写槽；
//! - 弱未定义在首调时补零地址，不算失败；
//! - 依赖库里的符号在首调时才进第二段作用域查找；
//! - `RTLD_NOW` 对照对象在装载期就把两个函数槽写完，触发计数为 0。
//!
//! 拒收面同样围绕"触发点"：桩表结构坏了（`LZ_ESTRUCT`）、装载期提前解析或
//! 复审后才调用（`LZ_EORDER` / `LZ_ELATE`）、首调解析失败（`LZ_ERESOLVE`）、
//! 补槽补错位置（`LZ_EPATCH`）、槽没补好导致每次调用都重新触发（`LZ_EAGAIN`）。
//! 14 槽语料 8 活 6 死，活档覆盖未调用、首调、二调、双槽、数据槽、弱零、依赖
//! 命中与 NOW 对照；死档对应上面六类。
//!
//! 本模块的判据面、渲染面与账本面共用同一份语料与同一组域码，任何一面漂移
//! 都会在 `run_unx11631_checks()` 里变红。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfe40;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// PLT 条目容量（含 PLT0 之外的最多 6 个槽）。
pub const PLT_CAP: usize = 6;

/// GOT.PLT 槽容量。
pub const GOT_CAP: usize = 8;

/// 作用域对象容量。
pub const SCOPE_CAP: usize = 3;

/// 单对象符号容量。
pub const SYM_CAP: usize = 4;

/// 解析轨迹容量。
pub const TRACE_CAP: usize = 12;

/// GOT.PLT 保留项个数（dynamic、link_map、resolver）。
pub const GOT_RESERVED: u8 = 3;

/// 合法旗标掩码：LAZY|NOW|GLOBAL|DEEPBIND。
pub const FLAG_MASK: u32 = 0x0000_010b;

/// `RTLD_LAZY`。
pub const RTLD_LAZY: u32 = 0x0001;

/// `RTLD_NOW`。
pub const RTLD_NOW: u32 = 0x0002;

/// `RTLD_GLOBAL`。
pub const RTLD_GLOBAL: u32 = 0x0100;

/// `RTLD_DEEPBIND`。
pub const RTLD_DEEPBIND: u32 = 0x0008;

/// 绑定模式位。
pub const BIND_BITS: u32 = 0x0003;

/// 未解析槽里的 PLT0 桩地址标记。
pub const PLT0_STUB: u64 = 0x0000_0000_0bad_f00d;

/// 弱未定义解析出的零地址。
pub const WEAK_TARGET: u64 = 0;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// PLT/GOT 结构损坏。
pub const LZ_ESTRUCT: u32 = SEG_BASE + 0x01;
/// 触发时点错误（装载期提前解析）。
pub const LZ_EORDER: u32 = SEG_BASE + 0x02;
/// 首调解析失败（未定义或版本不匹配）。
pub const LZ_ERESOLVE: u32 = SEG_BASE + 0x03;
/// 补槽位置或补槽值错误。
pub const LZ_EPATCH: u32 = SEG_BASE + 0x04;
/// 复审之后仍发生调用。
pub const LZ_ELATE: u32 = SEG_BASE + 0x05;
/// 槽未补好导致重复触发。
pub const LZ_EAGAIN: u32 = SEG_BASE + 0x06;
/// 账本状态错。
pub const LZ_ESTATE: u32 = SEG_BASE + 0x07;
/// 期望值不符。
pub const LZ_EVERIFY: u32 = SEG_BASE + 0x08;
/// 对账不平。
pub const LZ_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位越界。
pub const LZ_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与常量表不符。
pub const LZ_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结指纹失配。
pub const LZ_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无可观测样本。
pub const LZ_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 语料汇总。
pub const OP_CORPUS: u8 = 1;
/// 触发。
pub const OP_TRIGGER: u8 = 2;
/// 解析。
pub const OP_RESOLVE: u8 = 3;
/// 补槽。
pub const OP_PATCH: u8 = 4;
/// 数据槽。
pub const OP_DATA: u8 = 5;
/// 文案。
pub const OP_TEXT: u8 = 6;

// ---------------------------------------------------------------------------
// FNV-1a
// ---------------------------------------------------------------------------

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折叠一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

/// 折叠一个字（大端 8 字节）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < 8 {
        let shift = (7 - i) * 8;
        let b = ((w >> shift) & 0xff) as u8;
        x = fold_byte(x, b);
        i += 1;
    }
    x
}

/// 计算字节串摘要。
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

/// 十进制文本。
pub fn u32_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    let mut n = v;
    while n > 0 {
        let d = (n % 10) as u8;
        i -= 1;
        if let Some(slot) = buf.get_mut(i) {
            *slot = b'0' + d;
        }
        n /= 10;
    }
    let mut s = String::new();
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 固定八位十六进制文本。
pub fn hex_text(v: u32) -> String {
    const HEX: [u8; 16] = [
        b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd', b'e',
        b'f',
    ];
    let mut s = String::from("0x");
    let mut i = 0usize;
    while i < 8 {
        let shift = (7 - i) * 4;
        let nib = ((v >> shift) & 0xf) as usize;
        match HEX.get(nib) {
            Some(b) => s.push(*b as char),
            None => s.push('0'),
        }
        i += 1;
    }
    s
}

/// 符号名到 GOT 槽值的稳定标记。
pub fn token(name: &str) -> u64 {
    let mut h = digest(name.as_bytes()) | 1;
    if h == PLT0_STUB || h == WEAK_TARGET {
        h ^= 0x5a5a_5a5a_5a5a_5a5a;
        h |= 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 绑定模式
// ---------------------------------------------------------------------------

/// 绑定模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 延迟绑定。
    Lazy,
    /// 即时绑定。
    Now,
}

impl Mode {
    /// 旗标位。
    pub const fn bit(self) -> u32 {
        match self {
            Mode::Lazy => RTLD_LAZY,
            Mode::Now => RTLD_NOW,
        }
    }

    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Mode::Lazy => "lazy",
            Mode::Now => "now",
        }
    }

    /// 是否延迟。
    pub const fn is_lazy(self) -> bool {
        match self {
            Mode::Lazy => true,
            Mode::Now => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 符号与作用域
// ---------------------------------------------------------------------------

/// 符号绑定类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
    /// 强符号。
    Strong,
    /// 弱符号。
    Weak,
}

impl Bind {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Bind::Strong => "strong",
            Bind::Weak => "weak",
        }
    }
}

/// 一个符号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sym {
    /// 名字。
    pub name: &'static str,
    /// 版本。
    pub version: &'static str,
    /// 绑定类型。
    pub bind: Bind,
    /// 是否有定义。
    pub defined: bool,
}

impl Sym {
    /// 空格。
    pub const fn blank() -> Sym {
        Sym { name: "", version: "", bind: Bind::Strong, defined: false }
    }

    /// 强定义。
    pub const fn strong(name: &'static str) -> Sym {
        Sym { name, version: "", bind: Bind::Strong, defined: true }
    }

    /// 带版本的强定义。
    pub const fn versioned(name: &'static str, ver: &'static str) -> Sym {
        Sym { name, version: ver, bind: Bind::Strong, defined: true }
    }

    /// 弱未定义。
    pub const fn weak(name: &'static str) -> Sym {
        Sym { name, version: "", bind: Bind::Weak, defined: false }
    }

    /// 槽值。
    pub fn value(&self) -> u64 {
        token(self.name)
    }
}

/// 一个作用域对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeObj {
    /// 对象名。
    pub name: &'static str,
    /// 符号表。
    pub syms: [Sym; SYM_CAP],
    /// 符号数。
    pub len: u8,
}

impl ScopeObj {
    /// 空对象。
    pub const fn blank(name: &'static str) -> ScopeObj {
        ScopeObj { name, syms: [Sym::blank(); SYM_CAP], len: 0 }
    }

    /// 追加符号。
    pub fn push(&mut self, s: Sym) -> bool {
        if (self.len as usize) >= SYM_CAP {
            return false;
        }
        if let Some(dst) = self.syms.get_mut(self.len as usize) {
            *dst = s;
        }
        self.len += 1;
        true
    }

    /// 取符号。
    pub fn sym_at(&self, i: usize) -> Option<Sym> {
        if i >= self.len as usize {
            return None;
        }
        self.syms.get(i).copied()
    }

    /// 查已定义符号。
    pub fn find_defined(&self, name: &str) -> Option<Sym> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(s) = self.syms.get(i) {
                if s.defined && s.name == name {
                    return Some(*s);
                }
            }
            i += 1;
        }
        None
    }

    /// 查弱未定义符号。
    pub fn find_weak(&self, name: &str) -> Option<Sym> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(s) = self.syms.get(i) {
                if !s.defined && s.bind == Bind::Weak && s.name == name {
                    return Some(*s);
                }
            }
            i += 1;
        }
        None
    }
}

/// 作用域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    /// 对象表。
    pub objs: [ScopeObj; SCOPE_CAP],
    /// 对象数。
    pub len: u8,
}

impl Scope {
    /// 空作用域。
    pub const fn blank() -> Scope {
        Scope { objs: [ScopeObj::blank(""); SCOPE_CAP], len: 0 }
    }

    /// 追加对象。
    pub fn push(&mut self, o: ScopeObj) -> bool {
        if (self.len as usize) >= SCOPE_CAP {
            return false;
        }
        if let Some(dst) = self.objs.get_mut(self.len as usize) {
            *dst = o;
        }
        self.len += 1;
        true
    }

    /// 对象数。
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// 取对象。
    pub fn obj_at(&self, i: usize) -> Option<ScopeObj> {
        if i >= self.len as usize {
            return None;
        }
        self.objs.get(i).copied()
    }

    /// 按装载顺序查已定义符号。
    pub fn find_defined(&self, name: &str) -> Option<(usize, Sym)> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(o) = self.objs.get(i) {
                if let Some(s) = o.find_defined(name) {
                    return Some((i, s));
                }
            }
            i += 1;
        }
        None
    }

    /// 按装载顺序查弱未定义符号。
    pub fn find_weak(&self, name: &str) -> Option<(usize, Sym)> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(o) = self.objs.get(i) {
                if let Some(s) = o.find_weak(name) {
                    return Some((i, s));
                }
            }
            i += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 重定位
// ---------------------------------------------------------------------------

/// 重定位种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelocKind {
    /// `R_X86_64_JUMP_SLOT`。
    JumpSlot,
    /// `R_X86_64_GLOB_DAT`。
    GlobDat,
    /// `R_X86_64_RELATIVE`。
    Relative,
}

impl RelocKind {
    /// 编码。
    pub const fn code(self) -> u32 {
        match self {
            RelocKind::JumpSlot => 7,
            RelocKind::GlobDat => 6,
            RelocKind::Relative => 8,
        }
    }

    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            RelocKind::JumpSlot => "jump-slot",
            RelocKind::GlobDat => "glob-dat",
            RelocKind::Relative => "relative",
        }
    }
}

/// 一条重定位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reloc {
    /// 种类。
    pub kind: RelocKind,
    /// 槽号。
    pub slot: u8,
    /// 符号名。
    pub target: &'static str,
    /// 需求版本。
    pub version: &'static str,
    /// 是否带版本需求。
    pub versioned: bool,
}

impl Reloc {
    /// 函数跳转槽。
    pub const fn jump(slot: u8, target: &'static str) -> Reloc {
        Reloc {
            kind: RelocKind::JumpSlot,
            slot,
            target,
            version: "",
            versioned: false,
        }
    }

    /// 带版本需求的函数跳转槽。
    pub const fn versioned_jump(slot: u8, target: &'static str, ver: &'static str) -> Reloc {
        Reloc {
            kind: RelocKind::JumpSlot,
            slot,
            target,
            version: ver,
            versioned: true,
        }
    }

    /// 数据槽。
    pub const fn data(slot: u8, target: &'static str) -> Reloc {
        Reloc {
            kind: RelocKind::GlobDat,
            slot,
            target,
            version: "",
            versioned: false,
        }
    }

    /// 是否需要符号。
    pub const fn needs_sym(&self) -> bool {
        match self.kind {
            RelocKind::Relative => false,
            _ => true,
        }
    }

    /// 是否函数跳转槽。
    pub const fn is_jump(&self) -> bool {
        match self.kind {
            RelocKind::JumpSlot => true,
            _ => false,
        }
    }

    /// 是否数据槽。
    pub const fn is_data(&self) -> bool {
        match self.kind {
            RelocKind::GlobDat => true,
            _ => false,
        }
    }
}

/// 版本需求是否被符号满足。
fn version_ok(s: &Sym, r: &Reloc) -> bool {
    if r.versioned {
        s.version == r.version
    } else {
        true
    }
}

// ---------------------------------------------------------------------------
// PLT 与 GOT
// ---------------------------------------------------------------------------

/// 一条 PLT 条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PltEntry {
    /// 槽号。
    pub slot: u8,
    /// 对应重定位下标。
    pub reloc: u8,
}

impl PltEntry {
    /// 空格。
    pub const fn blank() -> PltEntry {
        PltEntry { slot: 0xff, reloc: 0xff }
    }

    /// 是否有效。
    pub const fn valid(&self) -> bool {
        self.slot != 0xff && self.reloc != 0xff
    }
}

/// PLT 表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plt {
    /// 条目。
    pub entries: [PltEntry; PLT_CAP],
    /// 条目数。
    pub len: u8,
    /// PLT0 解析桩是否存在。
    pub plt0: bool,
}

impl Plt {
    /// 空表（PLT0 存在）。
    pub const fn blank() -> Plt {
        Plt { entries: [PltEntry::blank(); PLT_CAP], len: 0, plt0: true }
    }

    /// 追加条目。
    pub fn push(&mut self, slot: u8, reloc: u8) -> bool {
        if (self.len as usize) >= PLT_CAP {
            return false;
        }
        if let Some(dst) = self.entries.get_mut(self.len as usize) {
            *dst = PltEntry { slot, reloc };
        }
        self.len += 1;
        true
    }

    /// 条目数。
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// 按槽号取条目。
    pub fn entry(&self, slot: usize) -> Option<PltEntry> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(e) = self.entries.get(i) {
                if e.slot as usize == slot {
                    return Some(*e);
                }
            }
            i += 1;
        }
        None
    }
}

/// GOT.PLT 表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Got {
    /// 槽值。
    pub slots: [u64; GOT_CAP],
    /// 已用槽数。
    pub n: u8,
    /// 保留项个数。
    pub reserved: u8,
}

impl Got {
    /// 空表。
    pub const fn blank() -> Got {
        Got { slots: [PLT0_STUB; GOT_CAP], n: 0, reserved: GOT_RESERVED }
    }

    /// 取槽值。
    pub fn value(&self, slot: usize) -> Option<u64> {
        if slot >= self.n as usize {
            return None;
        }
        self.slots.get(slot).copied()
    }

    /// 写槽值。
    pub fn set(&mut self, slot: usize, v: u64) -> bool {
        if slot >= GOT_CAP {
            return false;
        }
        if let Some(dst) = self.slots.get_mut(slot) {
            *dst = v;
        }
        if (slot as u8) >= self.n {
            self.n = (slot as u8).saturating_add(1);
        }
        true
    }

    /// 保留项是否齐备。
    pub const fn reserved_ok(&self) -> bool {
        self.reserved == GOT_RESERVED
    }

    /// 槽是否仍是桩。
    pub fn unresolved(&self, slot: usize) -> bool {
        match self.value(slot) {
            Some(v) => v == PLT0_STUB,
            None => true,
        }
    }
}

/// 一次补槽记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patch {
    /// 被调用的槽。
    pub called: u8,
    /// 实际补的槽。
    pub patched: u8,
    /// 写入值。
    pub to: u64,
}

impl Patch {
    /// 空格。
    pub const fn blank() -> Patch {
        Patch { called: 0xff, patched: 0xff, to: 0 }
    }

    /// 位置正确。
    pub const fn aligned(&self) -> bool {
        self.called == self.patched
    }
}

// ---------------------------------------------------------------------------
// 轨迹
// ---------------------------------------------------------------------------

/// 阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 装载。
    Load,
    /// 调用。
    Call,
    /// 解析。
    Resolve,
    /// 补槽。
    Patch,
    /// 复审。
    Audit,
    /// 完成。
    Done,
}

impl Stage {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Load => "load",
            Stage::Call => "call",
            Stage::Resolve => "resolve",
            Stage::Patch => "patch",
            Stage::Audit => "audit",
            Stage::Done => "done",
        }
    }
}

/// 轨迹中的一个注记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    /// 阶段。
    pub stage: Stage,
    /// 成败。
    pub ok: bool,
    /// 槽号。
    pub slot: u8,
}

impl Note {
    /// 空格。
    pub const fn blank() -> Note {
        Note { stage: Stage::Load, ok: false, slot: 0xff }
    }
}

/// 轨迹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trace {
    /// 注记。
    pub notes: [Note; TRACE_CAP],
    /// 注记数。
    pub len: u8,
}

impl Trace {
    /// 空轨迹。
    pub const fn blank() -> Trace {
        Trace { notes: [Note::blank(); TRACE_CAP], len: 0 }
    }

    /// 追加注记。
    pub fn note(&mut self, stage: Stage, ok: bool, slot: u8) {
        if (self.len as usize) >= TRACE_CAP {
            return;
        }
        if let Some(dst) = self.notes.get_mut(self.len as usize) {
            *dst = Note { stage, ok, slot };
        }
        self.len += 1;
    }

    /// 注记数。
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// 取注记。
    pub fn at(&self, i: usize) -> Option<(Stage, bool, u8)> {
        if i >= self.len as usize {
            return None;
        }
        match self.notes.get(i) {
            Some(n) => Some((n.stage, n.ok, n.slot)),
            None => None,
        }
    }

    /// 最后一条。
    pub fn last(&self) -> Option<(Stage, bool, u8)> {
        if self.len == 0 {
            return None;
        }
        self.at(self.len as usize - 1)
    }

    /// 是否含某阶段。
    pub fn has(&self, stage: Stage) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(n) = self.notes.get(i) {
                if n.stage == stage {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 某阶段出现次数。
    pub fn count(&self, stage: Stage) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(x) = self.notes.get(i) {
                if x.stage == stage {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 延迟对象
// ---------------------------------------------------------------------------

/// 一次调用的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallView {
    /// 槽号。
    pub slot: u8,
    /// 是否触发解析。
    pub triggered: bool,
    /// 是否直接命中。
    pub direct: bool,
    /// 跳转目标。
    pub target: u64,
}

impl CallView {
    /// 空。
    pub const fn blank() -> CallView {
        CallView { slot: 0, triggered: false, direct: false, target: 0 }
    }

    /// 首次调用。
    pub const fn first(slot: u8, target: u64) -> CallView {
        CallView { slot, triggered: true, direct: false, target }
    }

    /// 直接命中。
    pub const fn hit(slot: u8, target: u64) -> CallView {
        CallView { slot, triggered: false, direct: true, target }
    }
}

/// 一次复审的快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Audit {
    /// 模式。
    pub mode: Mode,
    /// 触发总次数。
    pub triggers: u16,
    /// 解析器调用次数。
    pub resolver: u16,
    /// 直接命中次数。
    pub direct: u16,
    /// 补槽次数。
    pub patches: u16,
    /// 装载期数据槽解析数。
    pub data: u16,
    /// 装载期函数槽解析数（应为 0，NOW 对照除外）。
    pub load_resolved: u16,
    /// 弱零解析次数。
    pub weak: u16,
}

impl Audit {
    /// 空。
    pub const fn blank(mode: Mode) -> Audit {
        Audit {
            mode,
            triggers: 0,
            resolver: 0,
            direct: 0,
            patches: 0,
            data: 0,
            load_resolved: 0,
            weak: 0,
        }
    }

    /// 是否发生过触发。
    pub const fn touched(&self) -> bool {
        self.triggers > 0
    }

    /// 是否每个触发都被补槽。
    pub const fn settled(&self) -> bool {
        self.triggers == self.patches
    }
}

/// 延迟绑定对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Object {
    /// 对象名。
    pub name: &'static str,
    /// 模式。
    pub mode: Mode,
    /// 旗标。
    pub flags: u32,
    /// PLT。
    pub plt: Plt,
    /// GOT。
    pub got: Got,
    /// 重定位。
    pub relocs: [Reloc; GOT_CAP],
    /// 重定位条数。
    pub reloc_len: u8,
    /// 作用域。
    pub scope: Scope,
    /// 装载期提前解析标记。
    pub early: bool,
    /// 补槽补错位置标记。
    pub wrong_patch: bool,
    /// 补槽不落槽标记。
    pub stale: bool,
    /// 已关闭。
    pub closed: bool,
    /// 已复审。
    pub audit_done: bool,
    /// 解析器调用数。
    pub resolver_calls: u16,
    /// 每槽触发数。
    pub triggers: [u16; PLT_CAP],
    /// 直接命中数。
    pub direct: u16,
    /// 补槽记录。
    pub patches: [Patch; PLT_CAP],
    /// 补槽记录数。
    pub patch_len: u8,
    /// 装载期解析数。
    pub load_resolved: u16,
    /// 装载期数据槽解析数。
    pub data_resolved: u16,
    /// 弱零次数。
    pub weak_zero: u16,
}

impl Object {
    /// 空对象。
    pub fn blank(name: &'static str, mode: Mode) -> Object {
        Object {
            name,
            mode,
            flags: mode.bit(),
            plt: Plt::blank(),
            got: Got::blank(),
            relocs: [Reloc::jump(0, ""); GOT_CAP],
            reloc_len: 0,
            scope: Scope::blank(),
            early: false,
            wrong_patch: false,
            stale: false,
            closed: false,
            audit_done: false,
            resolver_calls: 0,
            triggers: [0; PLT_CAP],
            direct: 0,
            patches: [Patch::blank(); PLT_CAP],
            patch_len: 0,
            load_resolved: 0,
            data_resolved: 0,
            weak_zero: 0,
        }
    }

    /// 旗标合法。
    pub const fn mode_ok(&self) -> bool {
        let bits = self.flags & BIND_BITS;
        if bits != self.mode.bit() {
            return false;
        }
        (self.flags & !FLAG_MASK) == 0
    }

    /// 追加重定位。
    pub fn add_reloc(&mut self, r: Reloc) -> bool {
        if (self.reloc_len as usize) >= GOT_CAP {
            return false;
        }
        if let Some(dst) = self.relocs.get_mut(self.reloc_len as usize) {
            *dst = r;
        }
        self.reloc_len += 1;
        true
    }

    /// 追加 PLT 条目。
    pub fn add_plt(&mut self, slot: u8, reloc: u8) -> bool {
        self.plt.push(slot, reloc)
    }

    /// 重定位条数。
    pub const fn len(&self) -> usize {
        self.reloc_len as usize
    }

    /// 取重定位。
    pub fn reloc_at(&self, i: usize) -> Option<Reloc> {
        if i >= self.reloc_len as usize {
            return None;
        }
        self.relocs.get(i).copied()
    }

    /// 函数槽重定位数。
    pub fn jump_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.reloc_len as usize {
            if let Some(r) = self.relocs.get(i) {
                if r.is_jump() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 数据槽重定位数。
    pub fn data_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.reloc_len as usize {
            if let Some(r) = self.relocs.get(i) {
                if r.is_data() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 装载期解析：数据槽总做，函数槽只有 NOW 做。
    pub fn resolve_load(&mut self) {
        let mut i = 0usize;
        while i < self.reloc_len as usize {
            let r = match self.relocs.get(i) {
                Some(v) => *v,
                None => return,
            };
            let do_it = r.is_data() || !self.mode.is_lazy();
            if do_it {
                let t = token(r.target);
                if self.got.set(r.slot as usize, t) {
                    self.load_resolved = self.load_resolved.saturating_add(1);
                    if r.is_data() {
                        self.data_resolved = self.data_resolved.saturating_add(1);
                    }
                }
            }
            i += 1;
        }
    }

    /// 强制预补一个函数槽（模拟装载期提前解析的坏实现）。
    pub fn prepatch(&mut self, slot: u8) -> bool {
        let mut found = false;
        let mut i = 0usize;
        while i < self.reloc_len as usize {
            if let Some(r) = self.relocs.get(i) {
                if r.is_jump() && r.slot == slot {
                    found = true;
                }
            }
            i += 1;
        }
        if !found {
            return false;
        }
        let t = token("printf");
        if self.got.set(slot as usize, t) {
            self.load_resolved = self.load_resolved.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// PLT 与重定位是否一一对齐。
    pub fn plt_shape_ok(&self) -> bool {
        if !self.plt.plt0 || !self.got.reserved_ok() {
            return false;
        }
        let mut i = 0usize;
        while i < self.plt.len() {
            let e = match self.plt.entries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if !e.valid() {
                return false;
            }
            let r = match self.reloc_at(e.reloc as usize) {
                Some(v) => v,
                None => return false,
            };
            if !r.is_jump() || r.slot != e.slot {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 一次调用。
    pub fn call(&mut self, slot: usize) -> Result<CallView, u32> {
        if self.audit_done || self.closed {
            return Err(LZ_ELATE);
        }
        if self.early {
            return Err(LZ_EORDER);
        }
        if !self.plt_shape_ok() {
            return Err(LZ_ESTRUCT);
        }
        let ent = match self.plt.entry(slot) {
            Some(v) => v,
            None => return Err(LZ_EORDER),
        };
        if ent.slot as usize != slot {
            return Err(LZ_EORDER);
        }
        let rel = match self.reloc_at(ent.reloc as usize) {
            Some(v) => v,
            None => return Err(LZ_ESTRUCT),
        };
        if rel.slot as usize != slot {
            return Err(LZ_ESTRUCT);
        }

        // 已补槽：直接命中，不再触发。
        if let Some(v) = self.got.value(slot) {
            if v != PLT0_STUB {
                self.direct = self.direct.saturating_add(1);
                return Ok(CallView::hit(slot as u8, v));
            }
        }

        // NOW 档不该走到这里；装载期已全部补完。
        if !self.mode.is_lazy() {
            return Err(LZ_EORDER);
        }

        // 首调：触发计数加一。
        if let Some(t) = self.triggers.get_mut(slot) {
            *t = t.saturating_add(1);
            if *t > 1 {
                // 上一次没有补槽，本次再次触发 —— 说明补槽失效。
                return Err(LZ_EAGAIN);
            }
        } else {
            return Err(LZ_ESLOT);
        }
        self.resolver_calls = self.resolver_calls.saturating_add(1);

        // 作用域查找。
        let target = match self.scope.find_defined(rel.target) {
            Some((_, s)) => {
                if !version_ok(&s, &rel) {
                    return Err(LZ_ERESOLVE);
                }
                s.value()
            }
            None => {
                if self.scope.find_weak(rel.target).is_some() {
                    self.weak_zero = self.weak_zero.saturating_add(1);
                    WEAK_TARGET
                } else {
                    return Err(LZ_ERESOLVE);
                }
            }
        };

        // 坏实现：把值补到别的槽。
        if self.wrong_patch {
            let other = if slot == 0 { 1u8 } else { 0u8 };
            if let Some(dst) = self.patches.get_mut(self.patch_len as usize) {
                *dst = Patch { called: slot as u8, patched: other, to: target };
            }
            self.patch_len = self.patch_len.saturating_add(1);
            return Err(LZ_EPATCH);
        }

        // 记录补槽；stale 档只记录不落槽。
        if let Some(dst) = self.patches.get_mut(self.patch_len as usize) {
            *dst = Patch { called: slot as u8, patched: slot as u8, to: target };
        }
        self.patch_len = self.patch_len.saturating_add(1);
        if !self.stale {
            if !self.got.set(slot, target) {
                return Err(LZ_ESTRUCT);
            }
        }
        Ok(CallView::first(slot as u8, target))
    }

    /// 复审：钉住"触发只发生在调用时、补槽只发生在触发后"。
    pub fn close_audit(&mut self) -> Result<Audit, u32> {
        if self.audit_done {
            return Err(LZ_ESTATE);
        }
        if self.closed {
            return Err(LZ_ELATE);
        }
        if self.early {
            return Err(LZ_EORDER);
        }
        if !self.plt_shape_ok() {
            return Err(LZ_ESTRUCT);
        }

        // 装载期解析量必须与模式匹配。
        if self.mode.is_lazy() {
            if self.load_resolved as usize != self.data_count() {
                return Err(LZ_EORDER);
            }
        } else if self.load_resolved as usize != self.jump_count() + self.data_count() {
            return Err(LZ_EORDER);
        }

        // 补槽位置必须与调用槽一致。
        let mut i = 0usize;
        while i < self.patch_len as usize {
            if let Some(p) = self.patches.get(i) {
                if !p.aligned() {
                    return Err(LZ_EPATCH);
                }
            }
            i += 1;
        }

        // 触发过的槽必须已经有落点。
        let mut s = 0usize;
        while s < PLT_CAP {
            if let Some(t) = self.triggers.get(s) {
                if *t > 1 {
                    return Err(LZ_EAGAIN);
                }
                if *t == 1 && self.got.unresolved(s) {
                    return Err(LZ_EAGAIN);
                }
            }
            s += 1;
        }

        self.audit_done = true;
        Ok(self.snapshot())
    }

    /// 关闭对象。
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// 取快照。
    pub fn snapshot(&self) -> Audit {
        let mut a = Audit::blank(self.mode);
        let mut i = 0usize;
        while i < PLT_CAP {
            if let Some(t) = self.triggers.get(i) {
                a.triggers = a.triggers.saturating_add(*t);
            }
            i += 1;
        }
        a.resolver = self.resolver_calls;
        a.direct = self.direct;
        a.patches = self.patch_len as u16;
        a.data = self.data_resolved;
        a.load_resolved = self.load_resolved;
        a.weak = self.weak_zero;
        a
    }
}

// ---------------------------------------------------------------------------
// 判面输出
// ---------------------------------------------------------------------------

/// 一次场景执行的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 对象名。
    pub name: &'static str,
    /// 模式。
    pub mode: Mode,
    /// 码；0 表示通过。
    pub code: u32,
    /// 轨迹。
    pub trace: Trace,
    /// 复审快照。
    pub audit: Audit,
}

impl Outcome {
    /// 空。
    pub const fn blank() -> Outcome {
        Outcome {
            name: "",
            mode: Mode::Lazy,
            code: 0,
            trace: Trace::blank(),
            audit: Audit::blank(Mode::Lazy),
        }
    }

    /// 通过。
    pub const fn passed(&self) -> bool {
        self.code == 0
    }

    /// 码。
    pub const fn code(&self) -> u32 {
        self.code
    }

    /// 复审。
    pub const fn audit(&self) -> Audit {
        self.audit
    }

    /// 轨迹。
    pub const fn trace(&self) -> Trace {
        self.trace
    }

    /// 文案。
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.passed() {
            s.push_str("lazy-ok ");
        } else {
            s.push_str("lazy-reject ");
        }
        s.push_str(self.name);
        s.push_str(" triggers=");
        s.push_str(u32_text(self.audit.triggers as u32).as_str());
        if !self.passed() {
            s.push_str(" code=");
            s.push_str(hex_text(self.code).as_str());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 场景。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// 装载后从不调用。
    NeverCalled,
    /// 首调触发。
    FirstCall,
    /// 二调直接。
    SecondDirect,
    /// 双槽独立。
    TwoSlots,
    /// 数据槽装载期解析。
    DataEager,
    /// 弱未定义首调补零。
    WeakZero,
    /// 依赖命中。
    DepHit,
    /// NOW 对照。
    NowContrast,
    /// 装载期提前解析。
    EarlyResolve,
    /// PLT0 缺失。
    MissingPlt0,
    /// 复审后调用。
    LateCall,
    /// 首调解析失败。
    ResolverUndef,
    /// 补槽补错位置。
    PatchWrong,
    /// 补槽不落导致重复触发。
    PatchStale,
}

impl Scenario {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Scenario::NeverCalled => "never-called",
            Scenario::FirstCall => "first-call",
            Scenario::SecondDirect => "second-direct",
            Scenario::TwoSlots => "two-slots",
            Scenario::DataEager => "data-eager",
            Scenario::WeakZero => "weak-zero",
            Scenario::DepHit => "dep-hit",
            Scenario::NowContrast => "now-contrast",
            Scenario::EarlyResolve => "early-resolve",
            Scenario::MissingPlt0 => "missing-plt0",
            Scenario::LateCall => "late-call",
            Scenario::ResolverUndef => "resolver-undef",
            Scenario::PatchWrong => "patch-wrong",
            Scenario::PatchStale => "patch-stale",
        }
    }
}

/// 一条语料。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 名字。
    pub name: &'static str,
    /// 场景。
    pub scenario: Scenario,
    /// 期望码。
    pub expect: u32,
    /// 说明。
    pub note: &'static str,
}

impl Case {
    /// 是否为活档。
    pub const fn accept(&self) -> bool {
        self.expect == 0
    }

    /// 输入键。
    pub fn input_key(&self) -> u64 {
        let mut h = digest(self.name.as_bytes());
        h = fold_word(h, self.scenario as u64);
        h
    }

    /// 说明键。
    pub fn note_key(&self) -> u64 {
        digest(self.note.as_bytes())
    }
}

/// 14 槽：8 活 6 死。
pub const CASES: [Case; CORPUS] = [
    Case {
        name: "never",
        scenario: Scenario::NeverCalled,
        expect: 0,
        note: "装载后一次不调，函数槽保持 PLT0 桩，触发零次",
    },
    Case {
        name: "first",
        scenario: Scenario::FirstCall,
        expect: 0,
        note: "第一次调用 printf 才触发解析，返回前 GOT 槽补好",
    },
    Case {
        name: "second",
        scenario: Scenario::SecondDirect,
        expect: 0,
        note: "同槽第二次调用直接命中已补槽，触发计数仍为 1",
    },
    Case {
        name: "two",
        scenario: Scenario::TwoSlots,
        expect: 0,
        note: "printf 与 sin 两槽各自首调触发，互不串扰",
    },
    Case {
        name: "data",
        scenario: Scenario::DataEager,
        expect: 0,
        note: "stderr 是 GLOB_DAT 数据槽，装载期就写，函数槽仍留桩",
    },
    Case {
        name: "weak",
        scenario: Scenario::WeakZero,
        expect: 0,
        note: "maybe 弱未定义，首调补零地址，触发一次不拒",
    },
    Case {
        name: "dep",
        scenario: Scenario::DepHit,
        expect: 0,
        note: "sin 住在依赖 libm，首调时才进第二段作用域查找",
    },
    Case {
        name: "now",
        scenario: Scenario::NowContrast,
        expect: 0,
        note: "RTLD_NOW 对照：两个函数槽装载期写完，触发零次",
    },
    Case {
        name: "early",
        scenario: Scenario::EarlyResolve,
        expect: LZ_EORDER,
        note: "LAZY 对象装载期就补了函数槽，触发时点提前，拒",
    },
    Case {
        name: "noplt",
        scenario: Scenario::MissingPlt0,
        expect: LZ_ESTRUCT,
        note: "PLT0 解析桩缺失，槽表结构不成立，装载模型拒",
    },
    Case {
        name: "late",
        scenario: Scenario::LateCall,
        expect: LZ_ELATE,
        note: "复审封账之后又发生调用，触发点落在时间线外",
    },
    Case {
        name: "undef",
        scenario: Scenario::ResolverUndef,
        expect: LZ_ERESOLVE,
        note: "missing_fn 强未定义，首调进解析器时失败",
    },
    Case {
        name: "wrong",
        scenario: Scenario::PatchWrong,
        expect: LZ_EPATCH,
        note: "解析器把 printf 的落点补到相邻槽，补槽位置错",
    },
    Case {
        name: "stale",
        scenario: Scenario::PatchStale,
        expect: LZ_EAGAIN,
        note: "补槽没落，第二次调用重新触发解析，触发计数越界",
    },
];

/// 取槽。
pub fn slot_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    CASES.get(i).copied()
}

/// 主程序作用域。
fn main_scope() -> Scope {
    let mut main = ScopeObj::blank("./app");
    let _ = main.push(Sym::strong("malloc"));
    let _ = main.push(Sym::strong("printf"));
    let _ = main.push(Sym::strong("stderr"));
    let _ = main.push(Sym::weak("maybe"));
    let mut s = Scope::blank();
    let _ = s.push(main);
    s
}

/// 依赖 libm 作用域。
fn dep_scope() -> Scope {
    let mut s = main_scope();
    let mut libm = ScopeObj::blank("libm.so.6");
    let _ = libm.push(Sym::strong("sin"));
    let _ = libm.push(Sym::strong("cos"));
    let _ = s.push(libm);
    s
}

/// 造 NOW 对照对象。
fn build_now() -> Object {
    let mut o = Object::blank("./libnow.so", Mode::Now);
    o.scope = main_scope();
    let _ = o.add_reloc(Reloc::jump(0, "malloc"));
    let _ = o.add_plt(0, 0);
    let _ = o.add_reloc(Reloc::jump(1, "printf"));
    let _ = o.add_plt(1, 1);
    o.resolve_load();
    o
}

/// 造一个场景对象。
pub fn build(sc: Scenario) -> Object {
    if sc == Scenario::NowContrast {
        return build_now();
    }
    let mut o = Object::blank("./liblazy.so", Mode::Lazy);
    o.scope = main_scope();
    match sc {
        Scenario::NeverCalled
        | Scenario::FirstCall
        | Scenario::SecondDirect
        | Scenario::LateCall
        | Scenario::EarlyResolve
        | Scenario::MissingPlt0
        | Scenario::PatchWrong
        | Scenario::PatchStale => {
            let _ = o.add_reloc(Reloc::jump(0, "printf"));
            let _ = o.add_plt(0, 0);
        }
        Scenario::ResolverUndef => {
            let _ = o.add_reloc(Reloc::jump(0, "missing_fn"));
            let _ = o.add_plt(0, 0);
        }
        Scenario::TwoSlots => {
            o.scope = dep_scope();
            let _ = o.add_reloc(Reloc::jump(0, "printf"));
            let _ = o.add_plt(0, 0);
            let _ = o.add_reloc(Reloc::jump(1, "sin"));
            let _ = o.add_plt(1, 1);
        }
        Scenario::DepHit => {
            o.scope = dep_scope();
            let _ = o.add_reloc(Reloc::jump(0, "sin"));
            let _ = o.add_plt(0, 0);
        }
        Scenario::WeakZero => {
            let _ = o.add_reloc(Reloc::jump(0, "maybe"));
            let _ = o.add_plt(0, 0);
        }
        Scenario::DataEager => {
            let _ = o.add_reloc(Reloc::jump(0, "printf"));
            let _ = o.add_plt(0, 0);
            let _ = o.add_reloc(Reloc::data(2, "stderr"));
        }
        Scenario::NowContrast => {}
    }
    o.resolve_load();
    match sc {
        Scenario::EarlyResolve => {
            let _ = o.prepatch(0);
            o.early = true;
        }
        Scenario::MissingPlt0 => {
            o.plt.plt0 = false;
        }
        Scenario::PatchWrong => {
            o.wrong_patch = true;
        }
        Scenario::PatchStale => {
            o.stale = true;
        }
        _ => {}
    }
    o
}

/// 驱动一串调用。
fn drive(o: &mut Object, slots: &[usize], tr: &mut Trace) -> u32 {
    let mut i = 0usize;
    while i < slots.len() {
        let slot = match slots.get(i) {
            Some(v) => *v,
            None => return LZ_EVALUE,
        };
        match o.call(slot) {
            Ok(v) => {
                tr.note(Stage::Call, true, v.slot);
                if v.triggered {
                    tr.note(Stage::Resolve, true, v.slot);
                    tr.note(Stage::Patch, true, v.slot);
                }
            }
            Err(c) => {
                tr.note(Stage::Call, false, slot as u8);
                return c;
            }
        }
        i += 1;
    }
    0
}

/// 执行一个场景。
pub fn execute(sc: Scenario) -> Outcome {
    let mut o = build(sc);
    let mut tr = Trace::blank();
    tr.note(Stage::Load, true, 0);
    let mut code = 0u32;
    match sc {
        Scenario::NeverCalled | Scenario::DataEager | Scenario::NowContrast => {}
        Scenario::FirstCall
        | Scenario::WeakZero
        | Scenario::DepHit
        | Scenario::EarlyResolve
        | Scenario::MissingPlt0
        | Scenario::ResolverUndef
        | Scenario::PatchWrong => {
            code = drive(&mut o, &[0], &mut tr);
        }
        Scenario::SecondDirect | Scenario::PatchStale => {
            code = drive(&mut o, &[0, 0], &mut tr);
        }
        Scenario::TwoSlots => {
            code = drive(&mut o, &[0, 1], &mut tr);
        }
        Scenario::LateCall => {
            match o.close_audit() {
                Ok(_) => tr.note(Stage::Audit, true, 0),
                Err(c) => {
                    tr.note(Stage::Audit, false, 0);
                    code = c;
                }
            }
            if code == 0 {
                code = drive(&mut o, &[0], &mut tr);
            }
        }
    }
    if code == 0 {
        match o.close_audit() {
            Ok(_) => {
                tr.note(Stage::Audit, true, 0);
                tr.note(Stage::Done, true, 0);
            }
            Err(c) => {
                tr.note(Stage::Audit, false, 0);
                code = c;
            }
        }
    }
    Outcome { name: o.name, mode: o.mode, code, trace: tr, audit: o.snapshot() }
}

/// 求解一条语料。
pub fn solve(c: &Case) -> Outcome {
    execute(c.scenario)
}

/// 唯一性。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            let b = match CASES.get(j) {
                Some(v) => v,
                None => return false,
            };
            if a.name == b.name || a.note == b.note || a.scenario == b.scenario {
                return false;
            }
            if a.input_key() == b.input_key() || a.note_key() == b.note_key() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 说明自洽。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if c.name.is_empty() || c.note.is_empty() || c.scenario.label().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 逐槽对齐。
pub fn table_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        let j = solve(&c);
        if j.code() != c.expect {
            return false;
        }
        if c.accept() != j.passed() {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    total: usize,
    accept: usize,
    reject: usize,
    structure: usize,
    order: usize,
    resolve: usize,
    patch: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空。
    pub const fn new() -> Report {
        Report {
            total: 0,
            accept: 0,
            reject: 0,
            structure: 0,
            order: 0,
            resolve: 0,
            patch: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 接受。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收。
    pub const fn reject(&self) -> usize {
        self.reject
    }

    /// 结构桶。
    pub const fn structure(&self) -> usize {
        self.structure
    }

    /// 时点桶。
    pub const fn order(&self) -> usize {
        self.order
    }

    /// 解析桶。
    pub const fn resolve(&self) -> usize {
        self.resolve
    }

    /// 补槽桶。
    pub const fn patch(&self) -> usize {
        self.patch
    }

    /// 对数。
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// 红档。
    pub const fn red(&self) -> bool {
        self.red
    }

    /// 空。
    pub const fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 千分率。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        ((self.matched as u32) * 1000 + (self.total as u32) / 2) / (self.total as u32)
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(LZ_ENOBS);
        }
        if self.total != CORPUS {
            return Err(LZ_EVALUE);
        }
        if self.accept != 8 || self.reject != 6 {
            return Err(LZ_EBAL);
        }
        if self.structure + self.order + self.resolve + self.patch != self.reject {
            return Err(LZ_EBAL);
        }
        if self.red {
            return Err(LZ_ESTATE);
        }
        if self.matched != self.total {
            return Err(LZ_EVERIFY);
        }
        Ok(())
    }

    /// 绿。
    pub fn green(&self) -> bool {
        match self.gate() {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("lazybind corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" structure=");
        s.push_str(u32_text(self.structure as u32).as_str());
        s.push_str(" order=");
        s.push_str(u32_text(self.order as u32).as_str());
        s.push_str(" resolve=");
        s.push_str(u32_text(self.resolve as u32).as_str());
        s.push_str(" patch=");
        s.push_str(u32_text(self.patch as u32).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑。
pub fn run() -> Result<Report, u32> {
    walk(None)
}

/// 跑并注入。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(LZ_EVALUE),
        };
        let j = solve(&c);
        r.total += 1;
        if j.passed() {
            r.accept += 1;
        } else {
            r.reject += 1;
            match j.code() {
                LZ_ESTRUCT => r.structure += 1,
                LZ_EORDER | LZ_ELATE => r.order += 1,
                LZ_ERESOLVE => r.resolve += 1,
                LZ_EPATCH | LZ_EAGAIN => r.patch += 1,
                _ => return Err(LZ_EVERIFY),
            }
        }
        if j.code() == c.expect {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, code)) = red {
        if slot >= r.total {
            return Err(LZ_ESLOT);
        }
        if code == 0 {
            return Err(LZ_EVALUE);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 前缀封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(LZ_ESLOT);
    }
    let mut i = 0usize;
    while i < n {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return Err(LZ_EVALUE),
        };
        if solve(&c).code() != c.expect {
            return Err(LZ_EVALUE);
        }
        i += 1;
    }
    Ok(())
}

/// 槽码。
fn case_code(i: usize) -> u32 {
    match slot_at(i) {
        Some(c) => solve(&c).code(),
        None => LZ_EVALUE,
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 成败。
    pub ok: bool,
    /// 码。
    pub code: u32,
    /// 值。
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
    /// 空。
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
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(LZ_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(LZ_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取格。
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
            return Err(LZ_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 1, 2, 1, 2, 929];
        const CODES: [u32; 6] =
            [LZ_EORDER, LZ_ESTRUCT, LZ_ELATE, LZ_ERESOLVE, LZ_EPATCH, LZ_EAGAIN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LZ_EBAL),
            };
            if s.slot as usize != i {
                return Err(LZ_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LZ_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LZ_EVALUE);
                        }
                    }
                    None => return Err(LZ_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LZ_EVALUE);
                        }
                    }
                    None => return Err(LZ_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LZ_EBAL);
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
            return Err(LZ_ESTATE);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(LZ_ESEAL)
        }
    }

    /// 指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 计数格。
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

/// 建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let rr = run_with(Some((0, LZ_EORDER)));
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_TRIGGER, r.map(|x| x.structure())));
    let _ = l.register(count_cell(4, OP_RESOLVE, r.map(|x| x.order())));
    let _ = l.register(count_cell(5, OP_RESOLVE, r.map(|x| x.resolve())));
    let _ = l.register(count_cell(6, OP_PATCH, r.map(|x| x.patch())));
    let _ = l.register(count_cell(7, OP_TEXT, rr.map(|x| x.rate() as usize)));
    let _ = l.register(err_cell(8, OP_TRIGGER, case_code(8)));
    let _ = l.register(err_cell(9, OP_PATCH, case_code(9)));
    let _ = l.register(err_cell(10, OP_TRIGGER, case_code(10)));
    let _ = l.register(err_cell(11, OP_RESOLVE, case_code(11)));
    let _ = l.register(err_cell(12, OP_PATCH, case_code(12)));
    let _ = l.register(err_cell(13, OP_PATCH, case_code(13)));
    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 模式与旗标。
fn flag_ok() -> bool {
    let lazy = Object::blank("./a", Mode::Lazy);
    let a = lazy.mode_ok() && lazy.mode.is_lazy() && lazy.flags == RTLD_LAZY;
    let now = Object::blank("./b", Mode::Now);
    let b = now.mode_ok() && !now.mode.is_lazy() && now.flags == RTLD_NOW;
    let mut both = Object::blank("./c", Mode::Lazy);
    both.flags = RTLD_LAZY | RTLD_NOW;
    let c = !both.mode_ok();
    let mut junk = Object::blank("./d", Mode::Lazy);
    junk.flags = RTLD_LAZY | 0x4000;
    let d = !junk.mode_ok();
    let mut zero = Object::blank("./e", Mode::Lazy);
    zero.flags = 0;
    let e = !zero.mode_ok();
    a && b && c && d && e
}

/// PLT/GOT 布局。
fn plt_ok() -> bool {
    let mut p = Plt::blank();
    let a = p.push(0, 0) && p.push(1, 1);
    let b = p.len() == 2 && p.plt0;
    let c = p.entry(0) == Some(PltEntry { slot: 0, reloc: 0 });
    let d = p.entry(1) == Some(PltEntry { slot: 1, reloc: 1 });
    let e = p.entry(3).is_none();
    let mut g = Got::blank();
    let f = g.reserved_ok() && g.unresolved(0);
    let h = g.set(0, token("printf")) && !g.unresolved(0);
    let i = g.value(0) == Some(token("printf")) && g.value(9).is_none();
    a && b && c && d && e && f && h && i
}

/// 作用域与符号。
fn scope_ok() -> bool {
    let s = dep_scope();
    let a = s.len() == 2 && s.obj_at(0).map(|o| o.name) == Some("./app");
    let b = s.find_defined("printf").map(|x| x.0) == Some(0);
    let c = s.find_defined("sin").map(|x| x.0) == Some(1);
    let d = s.find_defined("nope").is_none();
    let e = s.find_weak("maybe").is_some();
    let f = s.find_weak("printf").is_none();
    let g = Sym::strong("sin").value() == token("sin");
    let h = Sym::strong("sin").bind.label() == "strong" && Bind::Weak.label() == "weak";
    a && b && c && d && e && f && g && h
}

/// 未调用：装载期零解析。
fn never_ok() -> bool {
    let j = execute(Scenario::NeverCalled);
    if !j.passed() {
        return false;
    }
    j.audit.mode == Mode::Lazy
        && j.audit.triggers == 0
        && j.audit.resolver == 0
        && j.audit.load_resolved == 0
        && j.audit.patches == 0
        && j.trace.last() == Some((Stage::Done, true, 0))
}

/// 首调触发。
fn first_ok() -> bool {
    let j = execute(Scenario::FirstCall);
    if !j.passed() {
        return false;
    }
    let a = j.audit.triggers == 1 && j.audit.resolver == 1;
    let b = j.audit.patches == 1 && j.audit.direct == 0;
    let c = j.audit.load_resolved == 0 && j.trace.has(Stage::Resolve);
    let d = j.trace.has(Stage::Patch) && j.trace.count(Stage::Call) == 1;
    let o = build(Scenario::FirstCall);
    let mut m = o;
    let e = m.call(0).is_ok() && m.got.value(0) == Some(token("printf"));
    a && b && c && d && e
}

/// 二调直接。
fn second_ok() -> bool {
    let j = execute(Scenario::SecondDirect);
    if !j.passed() {
        return false;
    }
    j.audit.triggers == 1
        && j.audit.resolver == 1
        && j.audit.direct == 1
        && j.audit.patches == 1
        && j.audit.settled()
}

/// 双槽独立。
fn two_ok() -> bool {
    let j = execute(Scenario::TwoSlots);
    if !j.passed() {
        return false;
    }
    j.audit.triggers == 2
        && j.audit.resolver == 2
        && j.audit.patches == 2
        && j.audit.direct == 0
        && j.audit.load_resolved == 0
}

/// 数据槽装载期解析。
fn data_ok() -> bool {
    let j = execute(Scenario::DataEager);
    if !j.passed() {
        return false;
    }
    j.audit.data == 1
        && j.audit.load_resolved == 1
        && j.audit.triggers == 0
        && j.audit.resolver == 0
}

/// 弱零。
fn weak_ok() -> bool {
    let j = execute(Scenario::WeakZero);
    if !j.passed() {
        return false;
    }
    j.audit.weak == 1
        && j.audit.triggers == 1
        && j.audit.resolver == 1
        && j.audit.patches == 1
}

/// 依赖命中。
fn dep_ok() -> bool {
    let j = execute(Scenario::DepHit);
    if !j.passed() {
        return false;
    }
    let o = build(Scenario::DepHit);
    j.audit.triggers == 1
        && j.audit.resolver == 1
        && o.scope.len() == 2
        && o.scope.find_defined("sin").map(|x| x.0) == Some(1)
}

/// NOW 对照。
fn now_ok() -> bool {
    let j = execute(Scenario::NowContrast);
    if !j.passed() {
        return false;
    }
    j.audit.mode == Mode::Now
        && j.audit.load_resolved == 2
        && j.audit.triggers == 0
        && j.audit.resolver == 0
        && j.audit.data == 0
}

/// 六类拒收。
fn rejects_ok() -> bool {
    let a = execute(Scenario::EarlyResolve).code() == LZ_EORDER;
    let b = execute(Scenario::MissingPlt0).code() == LZ_ESTRUCT;
    let c = execute(Scenario::LateCall).code() == LZ_ELATE;
    let d = execute(Scenario::ResolverUndef).code() == LZ_ERESOLVE;
    let e = execute(Scenario::PatchWrong).code() == LZ_EPATCH;
    let f = execute(Scenario::PatchStale).code() == LZ_EAGAIN;
    let g = execute(Scenario::EarlyResolve).text().as_str()
        == "lazy-reject ./liblazy.so triggers=0 code=0x0000fe42";
    a && b && c && d && e && f && g
}

/// 轨迹。
fn trace_ok() -> bool {
    let good = execute(Scenario::FirstCall);
    let a = good.trace.at(0) == Some((Stage::Load, true, 0));
    let b = good.trace.last() == Some((Stage::Done, true, 0));
    let c = good.trace.count(Stage::Call) == 1 && good.trace.count(Stage::Resolve) == 1;
    let d = good.trace.count(Stage::Patch) == 1 && good.trace.count(Stage::Audit) == 1;
    let bad = execute(Scenario::ResolverUndef);
    let e = match bad.trace.last() {
        Some((Stage::Call, false, _)) => true,
        _ => false,
    };
    let f = Stage::Resolve.label() == "resolve" && Stage::Patch.label() == "patch";
    let g = good.trace.at(20).is_none() && good.trace.len() == 6;
    a && b && c && d && e && f && g
}

/// 语料形态。
fn corpus_ok() -> bool {
    if CASES.len() != CORPUS {
        return false;
    }
    let mut ok = 0usize;
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(c) => {
                if c.accept() {
                    ok += 1;
                } else {
                    bad += 1;
                }
            }
            None => return false,
        }
        i += 1;
    }
    ok == 8 && bad == 6
}

/// 槽。
fn slots_ok() -> bool {
    unique()
        && notes_ok()
        && table_ok()
        && cover(0).is_ok()
        && cover(14).is_ok()
        && cover(15) == Err(LZ_ESLOT)
        && slot_at(14).is_none()
        && slot_at(0).map(|c| c.scenario) == Some(Scenario::NeverCalled)
        && slot_at(13).map(|c| c.scenario) == Some(Scenario::PatchStale)
}

/// 渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render()
        == "lazybind corpus=14 accept=8 reject=6 structure=1 order=2 resolve=1 patch=2 match=14 rate=1000ppm\n";
    let b = r.rate() == 1000 && r.matched() == 14;
    let c = r.green() && r.structure() + r.order() + r.resolve() + r.patch() == 6;
    a && b && c
}

/// 红档。
fn reds_ok() -> bool {
    let red = match run_with(Some((5, LZ_EORDER))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = red.matched() == 13 && red.rate() == 929;
    let b = !red.green() && red.gate() == Err(LZ_ESTATE);
    let c = run_with(Some((14, LZ_EORDER))) == Err(LZ_ESLOT);
    let d = run_with(Some((0, 0))) == Err(LZ_EVALUE);
    a && b && c && d
}

/// 指纹。
fn hash_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(929) == "929";
    let b = hex_text(0xfe40) == "0x0000fe40" && hex_text(3) == "0x00000003";
    let c = fold_word(FNV_OFF, 0) == digest(&[0u8; 8]);
    let d = digest(b"UNX") != digest(b"UNY");
    let e = token("printf") != token("sin") && token("") != PLT0_STUB;
    a && b && c && d && e
}

/// 冻结重放。
fn pin_ok() -> bool {
    let mut l = build_ledger();
    let a = l.audit() == Ok(()) && l.len() == SAMPLES;
    let b = l.ok_count() == 8 && l.err_count() == 6;
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = root != 0 && l.root() == root;
    let d = l.replay() == Ok(root);
    let e = l.register(Sample::blank()) == Err(LZ_ESEAL);
    let f = l.rejects() == 1 && l.freeze() == Err(LZ_ESTATE);
    let g = l.sample(7).map(|s| s.value) == Some(929);
    let h = build_ledger().seal() == root;
    a && b && c && d && e && f && g && h
}

/// 四注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = flag_ok() && plt_ok() && scope_ok();
    out[1] = never_ok() && first_ok() && second_ok() && two_ok();
    out[2] = data_ok() && weak_ok() && dep_ok() && now_ok();
    out[3] = rejects_ok()
        && trace_ok()
        && corpus_ok()
        && slots_ok()
        && report_ok()
        && reds_ok()
        && hash_ok()
        && pin_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11631_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11631.lazy_trigger");

    s.add("LZ01-延迟模式与旗标位", flag_ok(), "static");
    s.add("LZ02-PLT/GOT 布局", plt_ok(), "static");
    s.add("LZ03-作用域查找顺序", scope_ok(), "static");
    s.add("LZ04-装载期零解析", never_ok(), "static");
    s.add("LZ05-首调触发并补槽", first_ok(), "static");
    s.add("LZ06-二次调用直接命中", second_ok(), "static");
    s.add("LZ07-双槽独立触发", two_ok(), "static");
    s.add("LZ08-数据槽装载期解析", data_ok(), "static");
    s.add("LZ09-弱零与依赖命中", weak_ok() && dep_ok(), "static");
    s.add("LZ10-NOW 对照面", now_ok(), "static");
    s.add("LZ11-六类触发点拒收", rejects_ok(), "static");
    s.add(
        "LZ12-十四槽渲染与红档",
        trace_ok() && corpus_ok() && slots_ok() && report_ok() && reds_ok(),
        "static",
    );
    s.add("LZ13-冻结重放四注入", inject_four() == [true; 4], "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0xfe40).as_str(), "0x0000fe40");
        assert_eq!(hex_text(0xfe4d).as_str(), "0x0000fe4d");
    }

    #[test]
    fn fnv_known() {
        let mut h = FNV_OFF;
        h = fold_byte(h, b'a');
        assert_eq!(h, 0xaf63dc4c8601ec8c);
        assert_ne!(fold_word(FNV_OFF, 2), FNV_OFF);
        assert_eq!(fold_word(FNV_OFF, 0), digest(&[0u8; 8]));
    }

    #[test]
    fn tokens_are_stable() {
        assert_eq!(token("printf"), token("printf"));
        assert_ne!(token("printf"), token("sin"));
        assert_ne!(token("printf"), PLT0_STUB);
        assert_ne!(token("x"), WEAK_TARGET);
    }

    #[test]
    fn flag_gate() {
        let lazy = Object::blank("./a", Mode::Lazy);
        assert!(lazy.mode_ok());
        assert!(lazy.mode.is_lazy());
        assert_eq!(Mode::Lazy.label(), "lazy");
        assert_eq!(Mode::Now.bit(), RTLD_NOW);
        let mut both = Object::blank("./c", Mode::Lazy);
        both.flags = RTLD_LAZY | RTLD_NOW;
        assert!(!both.mode_ok());
        let mut zero = Object::blank("./e", Mode::Lazy);
        zero.flags = 0;
        assert!(!zero.mode_ok());
    }

    #[test]
    fn plt_entries() {
        let mut p = Plt::blank();
        assert!(p.push(0, 0));
        assert!(p.push(1, 1));
        assert_eq!(p.len(), 2);
        assert_eq!(p.entry(1), Some(PltEntry { slot: 1, reloc: 1 }));
        assert!(p.entry(2).is_none());
        assert!(!PltEntry::blank().valid());
    }

    #[test]
    fn got_slots() {
        let mut g = Got::blank();
        assert!(g.reserved_ok());
        assert!(g.unresolved(0));
        assert!(g.set(0, token("printf")));
        assert!(!g.unresolved(0));
        assert_eq!(g.value(0), Some(token("printf")));
        assert!(g.value(7).is_none());
        assert!(!g.set(9, 1));
    }

    #[test]
    fn scope_lookup() {
        let s = dep_scope();
        assert_eq!(s.len(), 2);
        assert_eq!(s.find_defined("printf").map(|x| x.0), Some(0));
        assert_eq!(s.find_defined("sin").map(|x| x.0), Some(1));
        assert!(s.find_defined("nope").is_none());
        assert!(s.find_weak("maybe").is_some());
        assert!(s.find_weak("printf").is_none());
    }

    #[test]
    fn reloc_kinds() {
        assert_eq!(Reloc::jump(0, "printf").kind.code(), 7);
        assert_eq!(Reloc::data(1, "stderr").kind.code(), 6);
        assert!(Reloc::jump(0, "x").is_jump());
        assert!(Reloc::data(1, "x").is_data());
        assert!(!Reloc::data(1, "x").is_jump());
        assert_eq!(Reloc::versioned_jump(0, "memcpy", "GLIBC_2.34").version, "GLIBC_2.34");
    }

    #[test]
    fn version_checks() {
        let s = Sym::versioned("memcpy", "GLIBC_2.34");
        let good = Reloc::versioned_jump(0, "memcpy", "GLIBC_2.34");
        let bad = Reloc::versioned_jump(0, "memcpy", "GLIBC_2.99");
        let plain = Reloc::jump(0, "memcpy");
        assert!(version_ok(&s, &good));
        assert!(!version_ok(&s, &bad));
        assert!(version_ok(&s, &plain));
    }

    #[test]
    fn load_leaves_stub() {
        let o = build(Scenario::NeverCalled);
        assert_eq!(o.len(), 1);
        assert_eq!(o.load_resolved, 0);
        assert!(o.got.unresolved(0));
        assert!(o.plt_shape_ok());
    }

    #[test]
    fn first_call_triggers() {
        let j = execute(Scenario::FirstCall);
        assert!(j.passed());
        assert_eq!(j.audit.triggers, 1);
        assert_eq!(j.audit.resolver, 1);
        assert_eq!(j.audit.patches, 1);
        assert!(j.audit.settled());
        assert_eq!(j.text().as_str(), "lazy-ok ./liblazy.so triggers=1");
    }

    #[test]
    fn second_call_direct() {
        let j = execute(Scenario::SecondDirect);
        assert!(j.passed());
        assert_eq!(j.audit.triggers, 1);
        assert_eq!(j.audit.direct, 1);
        assert_eq!(j.audit.resolver, 1);
    }

    #[test]
    fn two_slots_independent() {
        let j = execute(Scenario::TwoSlots);
        assert!(j.passed());
        assert_eq!(j.audit.triggers, 2);
        assert_eq!(j.audit.resolver, 2);
        assert_eq!(j.audit.patches, 2);
    }

    #[test]
    fn data_resolved_at_load() {
        let j = execute(Scenario::DataEager);
        assert!(j.passed());
        assert_eq!(j.audit.data, 1);
        assert_eq!(j.audit.load_resolved, 1);
        assert_eq!(j.audit.triggers, 0);
        let o = build(Scenario::DataEager);
        assert_eq!(o.got.value(2), Some(token("stderr")));
        assert!(o.got.unresolved(0));
    }

    #[test]
    fn weak_resolves_zero() {
        let j = execute(Scenario::WeakZero);
        assert!(j.passed());
        assert_eq!(j.audit.weak, 1);
        assert_eq!(j.audit.triggers, 1);
        let o = build(Scenario::WeakZero);
        let mut m = o;
        let v = match m.call(0) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(v.target, WEAK_TARGET);
        assert_eq!(m.got.value(0), Some(WEAK_TARGET));
    }

    #[test]
    fn dependency_hit_at_call() {
        let j = execute(Scenario::DepHit);
        assert!(j.passed());
        assert_eq!(j.audit.triggers, 1);
        let o = build(Scenario::DepHit);
        assert_eq!(o.scope.len(), 2);
        assert_eq!(o.load_resolved, 0);
    }

    #[test]
    fn now_contrast_resolves_at_load() {
        let j = execute(Scenario::NowContrast);
        assert!(j.passed());
        assert_eq!(j.audit.mode, Mode::Now);
        assert_eq!(j.audit.load_resolved, 2);
        assert_eq!(j.audit.triggers, 0);
        let o = build(Scenario::NowContrast);
        assert_eq!(o.got.value(0), Some(token("malloc")));
        assert_eq!(o.got.value(1), Some(token("printf")));
    }

    #[test]
    fn early_resolution_rejected() {
        let j = execute(Scenario::EarlyResolve);
        assert_eq!(j.code(), LZ_EORDER);
        assert!(!j.passed());
        let o = build(Scenario::EarlyResolve);
        assert_eq!(o.load_resolved, 1);
        assert!(!o.got.unresolved(0));
    }

    #[test]
    fn missing_plt0_rejected() {
        let j = execute(Scenario::MissingPlt0);
        assert_eq!(j.code(), LZ_ESTRUCT);
        let o = build(Scenario::MissingPlt0);
        assert!(!o.plt_shape_ok());
    }

    #[test]
    fn late_call_rejected() {
        let j = execute(Scenario::LateCall);
        assert_eq!(j.code(), LZ_ELATE);
        assert_eq!(j.trace.count(Stage::Audit), 1);
    }

    #[test]
    fn resolver_undefined_rejected() {
        let j = execute(Scenario::ResolverUndef);
        assert_eq!(j.code(), LZ_ERESOLVE);
        assert_eq!(j.audit.triggers, 1);
        assert_eq!(j.audit.resolver, 1);
    }

    #[test]
    fn wrong_patch_rejected() {
        let j = execute(Scenario::PatchWrong);
        assert_eq!(j.code(), LZ_EPATCH);
        let o = build(Scenario::PatchWrong);
        let mut m = o;
        assert_eq!(m.call(0), Err(LZ_EPATCH));
        let p = match m.patches.first() {
            Some(v) => *v,
            None => return,
        };
        assert!(!p.aligned());
    }

    #[test]
    fn stale_patch_repeats() {
        let j = execute(Scenario::PatchStale);
        assert_eq!(j.code(), LZ_EAGAIN);
        let o = build(Scenario::PatchStale);
        let mut m = o;
        assert!(m.call(0).is_ok());
        assert_eq!(m.call(0), Err(LZ_EAGAIN));
    }

    #[test]
    fn audit_is_once() {
        let mut o = build(Scenario::NeverCalled);
        assert!(o.close_audit().is_ok());
        assert_eq!(o.close_audit(), Err(LZ_ESTATE));
    }

    #[test]
    fn close_blocks_calls() {
        let mut o = build(Scenario::NeverCalled);
        o.close();
        assert_eq!(o.call(0), Err(LZ_ELATE));
        assert_eq!(o.close_audit(), Err(LZ_ELATE));
    }

    #[test]
    fn bad_slot_call() {
        let mut o = build(Scenario::NeverCalled);
        assert_eq!(o.call(4), Err(LZ_EORDER));
        assert!(o.triggers.iter().all(|t| *t == 0));
    }

    #[test]
    fn trace_shape() {
        let j = execute(Scenario::TwoSlots);
        assert!(j.passed());
        assert_eq!(j.trace.len(), 9);
        assert_eq!(j.trace.count(Stage::Call), 2);
        assert_eq!(j.trace.count(Stage::Resolve), 2);
        assert_eq!(j.trace.count(Stage::Patch), 2);
        assert_eq!(j.trace.last(), Some((Stage::Done, true, 0)));
    }

    #[test]
    fn corpus_shape() {
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
        assert_eq!(CASES.len(), 14);
        assert!(slot_at(14).is_none());
    }

    #[test]
    fn corpus_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.total(), 14);
        assert_eq!(r.accept(), 8);
        assert_eq!(r.reject(), 6);
        assert_eq!(r.structure(), 1);
        assert_eq!(r.order(), 2);
        assert_eq!(r.resolve(), 1);
        assert_eq!(r.patch(), 2);
        assert!(r.green());
    }

    #[test]
    fn render_literal() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(
            r.render().as_str(),
            "lazybind corpus=14 accept=8 reject=6 structure=1 order=2 resolve=1 patch=2 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rate() {
        let r = match run_with(Some((2, LZ_EORDER))) {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.rate(), 929);
        assert_eq!(r.matched(), 13);
        assert_eq!(r.gate(), Err(LZ_ESTATE));
    }

    #[test]
    fn cover_prefix() {
        assert_eq!(cover(0), Ok(()));
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(15), Err(LZ_ESLOT));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_ne!(root, 0);
        assert_eq!(l.root(), root);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.register(Sample::blank()), Err(LZ_ESEAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true; 4]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11631_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn error_codes_distinct() {
        let codes = [
            LZ_ESTRUCT,
            LZ_EORDER,
            LZ_ERESOLVE,
            LZ_EPATCH,
            LZ_ELATE,
            LZ_EAGAIN,
            LZ_ESTATE,
            LZ_EVERIFY,
            LZ_EBAL,
            LZ_ESLOT,
            LZ_EVALUE,
            LZ_ESEAL,
            LZ_ENOBS,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i], SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }
}
