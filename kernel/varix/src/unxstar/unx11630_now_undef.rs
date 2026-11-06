//! UNX-F11630 RTLD_NOW 即时未定义符号拒收判据
//!
//! 闸门次序：旗标合法 → 重定位类型 → 作用域查找 → 版本匹配 → IFUNC 解析 → 判面。
//! 契约：纯 no_std 风格（仅 `alloc::string::String` 渲染），无 unsafe、无 unwrap/expect/panic!，失败一律 `Result<_, u32>`，域码 + 状态码恰 13 个。
//!
//! `dlopen(path, RTLD_NOW)` 的语义是：**装载期把全部重定位做完**。`JUMP_SLOT`
//! 不再留桩，`GLOB_DAT`、`TLS`、`IRELATIVE` 全部当场解析。任何一条重定位找不
//! 到落点，`dlopen` 直接返回 NULL，`dlerror` 给出 glibc 的逐字文案：
//!
//! ```text
//! ./libbad.so: undefined symbol: missing_fn
//! ```
//!
//! 带版本引用时文案换成版本面：
//!
//! ```text
//! ./libver.so: version `GLIBC_2.99' not found (required by ./libver.so)
//! ```
//!
//! 判据要同时钉住"拒"和"不误拒"两侧：
//!
//! - 强符号未定义 → 拒（`NW_EUNDEF`）；
//! - 带版本需求且版本不匹配 → 拒（`NW_EVER`）；
//! - 符号被 `RTLD_LOCAL` 库遮蔽 → 拒（`NW_ESCOPE`）；
//! - 重定位类型本实现不认识 → 拒（`NW_ERELOC`）；
//! - IFUNC 有符号无 resolver → 拒（`NW_EIFUNC`）；
//! - 绑定模式位非法（LAZY 与 NOW 同时置位、或都不置）→ 拒（`NW_EBIND`）。
//!
//! 因此 `RTLD_NOW` 的拒收是**六类结构化的拒**，不是"看到 undefined 就拒"：
//! 弱未定义符号在 NOW 档同样落零地址而**不拒**；`RELATIVE` 重定位不需要符号；
//! 依赖链里已定义的符号必须被找到；版本恰好匹配必须放行。14 槽语料 8 活 6
//! 死，活档覆盖全解析、弱零、依赖命中、版本命中、IFUNC、GLOBAL、数据槽与
//! 空重定位表。
//!
//! 本模块以同一份语料驱动绑定、渲染与账本，判面文案与报告逐字一致。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfe30;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 重定位容量。
pub const RELOC_CAP: usize = 4;

/// 作用域对象容量。
pub const SCOPE_CAP: usize = 3;

/// 单对象符号容量。
pub const SYM_CAP: usize = 4;

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

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 强符号未定义。
pub const NW_EUNDEF: u32 = SEG_BASE + 0x01;
/// 版本需求找不到匹配版本。
pub const NW_EVER: u32 = SEG_BASE + 0x02;
/// 符号被 `RTLD_LOCAL` 库遮蔽。
pub const NW_ESCOPE: u32 = SEG_BASE + 0x03;
/// 重定位类型不支持。
pub const NW_ERELOC: u32 = SEG_BASE + 0x04;
/// IFUNC 缺 resolver。
pub const NW_EIFUNC: u32 = SEG_BASE + 0x05;
/// 绑定模式旗标非法。
pub const NW_EBIND: u32 = SEG_BASE + 0x06;
/// 账本状态错。
pub const NW_ESTATE: u32 = SEG_BASE + 0x07;
/// 版本/结构期望不符。
pub const NW_EVERIFY: u32 = SEG_BASE + 0x08;
/// 对账不平。
pub const NW_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位越界。
pub const NW_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与常量表不符。
pub const NW_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结指纹失配。
pub const NW_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无可观测样本。
pub const NW_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 语料汇总。
pub const OP_CORPUS: u8 = 1;
/// 绑定。
pub const OP_BIND: u8 = 2;
/// 作用域。
pub const OP_SCOPE: u8 = 3;
/// 版本。
pub const OP_VERSION: u8 = 4;
/// 重定位。
pub const OP_RELOC: u8 = 5;
/// 文案。
pub const OP_TEXT: u8 = 6;

// ---------------------------------------------------------------------------
// FNV-1a
// ---------------------------------------------------------------------------

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折字。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 8 {
        acc = fold_byte(acc, ((w >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    acc
}

/// 字节串指纹。
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

/// 十进制。
pub fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    let mut x = v;
    if x == 0 {
        if let Some(b) = buf.get_mut(0) {
            *b = b'0';
        }
        n = 1;
    }
    while x > 0 {
        if let Some(b) = buf.get_mut(n) {
            *b = b'0' + (x % 10) as u8;
        }
        n += 1;
        x /= 10;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
    }
    s
}

/// 定宽十六进制。
pub fn hex_text(v: u32) -> String {
    const HEX: [u8; 16] = [
        b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd', b'e',
        b'f',
    ];
    let mut s = String::from("0x");
    let mut shift = 28i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0x0f) as usize;
        s.push(HEX.get(nib).copied().unwrap_or(b'0') as char);
        shift -= 4;
    }
    s
}

/// 拼接 `subject: tail`。
pub fn bind_text(subject: &str, tail: &str) -> String {
    let mut s = String::new();
    s.push_str(subject);
    s.push_str(": ");
    s.push_str(tail);
    s
}

// ---------------------------------------------------------------------------
// 绑定模式
// ---------------------------------------------------------------------------

/// 绑定模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindMode {
    /// 即时。
    Now,
    /// 延迟。
    Lazy,
}

impl BindMode {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            BindMode::Now => "now",
            BindMode::Lazy => "lazy",
        }
    }

    /// 对应旗标位。
    pub const fn bit(self) -> u32 {
        match self {
            BindMode::Now => RTLD_NOW,
            BindMode::Lazy => RTLD_LAZY,
        }
    }
}

// ---------------------------------------------------------------------------
// 符号
// ---------------------------------------------------------------------------

/// 绑定类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
    /// 强符号。
    Strong,
    /// 弱符号。
    Weak,
    /// IFUNC（带 resolver）。
    Ifunc,
}

impl Bind {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Bind::Strong => "strong",
            Bind::Weak => "weak",
            Bind::Ifunc => "ifunc",
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
    /// 绑定。
    pub bind: Bind,
    /// 是否有定义。
    pub defined: bool,
    /// 所在库是否只对自身可见。
    pub local: bool,
    /// 是否只在本库内部可见。
    pub hidden: bool,
    /// IFUNC resolver 是否实现。
    pub resolver: bool,
}

impl Sym {
    /// 空符号。
    pub const fn blank() -> Sym {
        Sym {
            name: "",
            version: "",
            bind: Bind::Strong,
            defined: false,
            local: false,
            hidden: false,
            resolver: false,
        }
    }

    /// 强定义。
    pub const fn strong(name: &'static str, version: &'static str) -> Sym {
        Sym {
            name,
            version,
            bind: Bind::Strong,
            defined: true,
            local: false,
            hidden: false,
            resolver: false,
        }
    }

    /// 弱未定义。
    pub const fn weak_undef(name: &'static str) -> Sym {
        Sym {
            name,
            version: "",
            bind: Bind::Weak,
            defined: false,
            local: false,
            hidden: false,
            resolver: false,
        }
    }

    /// IFUNC 定义。
    pub const fn ifunc(name: &'static str, resolver: bool) -> Sym {
        Sym {
            name,
            version: "",
            bind: Bind::Ifunc,
            defined: true,
            local: false,
            hidden: false,
            resolver,
        }
    }

    /// 标记 LOCAL 遮蔽。
    pub fn mark_hidden(&mut self) {
        self.local = true;
        self.hidden = true;
    }

    /// 是否可被外部解析。
    pub fn visible(&self, global: bool) -> bool {
        if !self.defined {
            return false;
        }
        if self.hidden && !global {
            return false;
        }
        true
    }

    /// 版本是否满足。
    pub fn version_ok(&self, req: &str, versioned: bool) -> bool {
        if !versioned {
            return true;
        }
        self.version == req && !req.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 作用域
// ---------------------------------------------------------------------------

/// 作用域中的一个对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeObj {
    /// 对象名。
    pub name: &'static str,
    /// 符号。
    pub syms: [Sym; SYM_CAP],
    /// 条数。
    pub len: u8,
    /// 全局可见。
    pub global: bool,
}

impl ScopeObj {
    /// 空。
    pub const fn blank(name: &'static str) -> ScopeObj {
        ScopeObj { name, syms: [Sym::blank(); SYM_CAP], len: 0, global: false }
    }

    /// 追加。
    pub fn push(&mut self, s: Sym) -> bool {
        if (self.len as usize) >= SYM_CAP {
            return false;
        }
        if let Some(slot) = self.syms.get_mut(self.len as usize) {
            *slot = s;
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

    /// 按名找。
    pub fn find(&self, name: &str) -> Option<Sym> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(s) = self.syms.get(i) {
                if s.name == name {
                    return Some(*s);
                }
            }
            i += 1;
        }
        None
    }

    /// 是否含某个定义。
    pub fn defines(&self, name: &str) -> bool {
        match self.find(name) {
            Some(s) => s.defined,
            None => false,
        }
    }
}

/// 符号作用域（可执行 + 预载 + 依赖，按序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    /// 对象。
    pub objs: [ScopeObj; SCOPE_CAP],
    /// 对象数。
    pub len: u8,
}

impl Scope {
    /// 空。
    pub const fn blank() -> Scope {
        Scope { objs: [ScopeObj::blank(""); SCOPE_CAP], len: 0 }
    }

    /// 追加对象。
    pub fn push(&mut self, o: ScopeObj) -> bool {
        if (self.len as usize) >= SCOPE_CAP {
            return false;
        }
        if let Some(slot) = self.objs.get_mut(self.len as usize) {
            *slot = o;
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

    /// 按序查找第一个定义。
    pub fn find_defined(&self, name: &str) -> Option<(usize, Sym)> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(o) = self.objs.get(i) {
                if let Some(s) = o.find(name) {
                    if s.defined {
                        return Some((i, s));
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 查找弱未定义声明。
    pub fn find_weak(&self, name: &str) -> Option<Sym> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(o) = self.objs.get(i) {
                if let Some(s) = o.find(name) {
                    if !s.defined && s.bind == Bind::Weak {
                        return Some(s);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 首个定义是否被 LOCAL 遮蔽。
    pub fn hidden_hit(&self, name: &str, global: bool) -> Option<(usize, Sym)> {
        match self.find_defined(name) {
            Some((i, s)) => {
                if s.hidden && !global {
                    Some((i, s))
                } else {
                    None
                }
            }
            None => None,
        }
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
    /// `R_X86_64_TLS` 族。
    Tls,
    /// 本实现不认识。
    Unknown,
}

impl RelocKind {
    /// 编码。
    pub const fn code(self) -> u32 {
        match self {
            RelocKind::JumpSlot => 7,
            RelocKind::GlobDat => 6,
            RelocKind::Relative => 8,
            RelocKind::Tls => 18,
            RelocKind::Unknown => 99,
        }
    }

    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            RelocKind::JumpSlot => "jump-slot",
            RelocKind::GlobDat => "glob-dat",
            RelocKind::Relative => "relative",
            RelocKind::Tls => "tls",
            RelocKind::Unknown => "unknown",
        }
    }

    /// 是否需要符号。
    pub const fn needs_sym(self) -> bool {
        match self {
            RelocKind::Relative => false,
            _ => true,
        }
    }
}

/// 一条重定位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reloc {
    /// 种类。
    pub kind: RelocKind,
    /// 原始类型号。
    pub raw: u32,
    /// 符号名。
    pub target: &'static str,
    /// 需求版本。
    pub req_version: &'static str,
    /// 是否带版本需求。
    pub versioned: bool,
}

impl Reloc {
    /// 构造。
    pub const fn new(kind: RelocKind, target: &'static str) -> Reloc {
        Reloc {
            kind,
            raw: kind.code(),
            target,
            req_version: "",
            versioned: false,
        }
    }

    /// 带版本需求。
    pub const fn versioned(kind: RelocKind, target: &'static str, ver: &'static str) -> Reloc {
        Reloc {
            kind,
            raw: kind.code(),
            target,
            req_version: ver,
            versioned: true,
        }
    }

    /// 是否已知类型。
    pub const fn known(&self) -> bool {
        match self.kind {
            RelocKind::Unknown => false,
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 装载请求
// ---------------------------------------------------------------------------

/// 一次 `dlopen` 请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    /// 对象名。
    pub name: &'static str,
    /// 绑定模式。
    pub mode: BindMode,
    /// 旗标。
    pub flags: u32,
    /// 重定位。
    pub relocs: [Reloc; RELOC_CAP],
    /// 重定位条数。
    pub reloc_len: u8,
    /// 作用域。
    pub scope: Scope,
}

impl Link {
    /// 构造。
    pub const fn new(name: &'static str, mode: BindMode, flags: u32, scope: Scope) -> Link {
        Link {
            name,
            mode,
            flags,
            relocs: [Reloc::new(RelocKind::Relative, ""); RELOC_CAP],
            reloc_len: 0,
            scope,
        }
    }

    /// 加速重定位。
    pub fn push(&mut self, r: Reloc) -> bool {
        if (self.reloc_len as usize) >= RELOC_CAP {
            return false;
        }
        if let Some(slot) = self.relocs.get_mut(self.reloc_len as usize) {
            *slot = r;
        }
        self.reloc_len += 1;
        true
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

    /// 是否全局。
    pub const fn global(&self) -> bool {
        (self.flags & RTLD_GLOBAL) != 0
    }

    /// 模式位是否恰好一个且与声明模式一致。
    pub const fn mode_ok(&self) -> bool {
        let bits = self.flags & BIND_BITS;
        if bits != self.mode.bit() {
            return false;
        }
        if bits != RTLD_NOW && bits != RTLD_LAZY {
            return false;
        }
        (self.flags & !FLAG_MASK) == 0
    }

    /// 是否 NOW。
    pub const fn is_now(&self) -> bool {
        match self.mode {
            BindMode::Now => true,
            BindMode::Lazy => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 判面
// ---------------------------------------------------------------------------

/// 成功视图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundView {
    /// 已绑定条数。
    pub resolved: u16,
    /// 弱零地址条数。
    pub weak_zero: u16,
    /// IFUNC 条数。
    pub ifunc: u16,
    /// 是否即时完成。
    pub eager: bool,
}

impl BoundView {
    /// 空。
    pub const fn blank() -> BoundView {
        BoundView { resolved: 0, weak_zero: 0, ifunc: 0, eager: false }
    }

    /// 全部立即完成且无未解析。
    pub const fn complete(&self) -> bool {
        self.eager && self.resolved + self.weak_zero > 0
    }
}

/// 阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 旗标。
    Flags,
    /// 重定位类型。
    Reloc,
    /// 作用域。
    Scope,
    /// 版本。
    Version,
    /// IFUNC。
    Ifunc,
    /// 完成。
    Done,
}

impl Stage {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Flags => "flags",
            Stage::Reloc => "reloc",
            Stage::Scope => "scope",
            Stage::Version => "version",
            Stage::Ifunc => "ifunc",
            Stage::Done => "done",
        }
    }
}

/// 轨迹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trace {
    stages: [Stage; 8],
    ok: [bool; 8],
    len: u8,
}

impl Trace {
    /// 空。
    pub const fn blank() -> Trace {
        Trace { stages: [Stage::Flags; 8], ok: [false; 8], len: 0 }
    }

    /// 记录。
    pub fn note(&mut self, st: Stage, passed: bool) {
        if (self.len as usize) < 8 {
            if let Some(slot) = self.stages.get_mut(self.len as usize) {
                *slot = st;
            }
            if let Some(slot) = self.ok.get_mut(self.len as usize) {
                *slot = passed;
            }
            self.len += 1;
        }
    }

    /// 步数。
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// 取步。
    pub fn at(&self, i: usize) -> Option<(Stage, bool)> {
        if i >= self.len as usize {
            return None;
        }
        match (self.stages.get(i), self.ok.get(i)) {
            (Some(a), Some(b)) => Some((*a, *b)),
            _ => None,
        }
    }

    /// 最后。
    pub fn last(&self) -> Option<(Stage, bool)> {
        if self.len == 0 {
            return None;
        }
        self.at(self.len as usize - 1)
    }
}

/// 一次绑定判决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judge {
    /// 结果。
    pub result: Result<BoundView, u32>,
    /// 轨迹。
    pub trace: Trace,
}

impl Judge {
    /// 码。
    pub fn code(&self) -> u32 {
        match self.result {
            Ok(_) => 0,
            Err(c) => c,
        }
    }

    /// 是否通过。
    pub const fn passed(&self) -> bool {
        match self.result {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// 视图。
    pub fn view(&self) -> Option<BoundView> {
        match self.result {
            Ok(v) => Some(v),
            Err(_) => None,
        }
    }

    /// 文案。
    pub fn text(&self, l: &Link) -> String {
        match self.result {
            Ok(v) => {
                let mut s = String::from("now-ok ");
                s.push_str(l.name);
                s.push_str(" resolved=");
                s.push_str(u32_text(v.resolved as u32).as_str());
                s.push_str(" weak=");
                s.push_str(u32_text(v.weak_zero as u32).as_str());
                s.push_str(" ifunc=");
                s.push_str(u32_text(v.ifunc as u32).as_str());
                s
            }
            Err(NW_EUNDEF) => {
                let name = first_fault_sym(l).unwrap_or("?");
                let mut s = String::from(l.name);
                s.push_str(": undefined symbol: ");
                s.push_str(name);
                s
            }
            Err(NW_EVER) => {
                let name = first_fault_sym(l).unwrap_or("?");
                let ver = first_fault_ver(l);
                let mut s = String::from(l.name);
                s.push_str(": version `");
                s.push_str(ver);
                s.push_str("' not found (required by ");
                s.push_str(l.name);
                s.push_str(") [");
                s.push_str(name);
                s.push(']');
                s
            }
            Err(NW_ESCOPE) => {
                let name = first_hidden(l).unwrap_or("?");
                let mut s = String::from(l.name);
                s.push_str(": symbol `");
                s.push_str(name);
                s.push_str("' hidden in RTLD_LOCAL scope");
                s
            }
            Err(NW_ERELOC) => {
                let raw = first_bad_reloc(l);
                let name = first_bad_reloc_sym(l);
                let mut s = String::from(l.name);
                s.push_str(": unsupported relocation type ");
                s.push_str(u32_text(raw).as_str());
                s.push_str(" for `");
                s.push_str(name);
                s.push('\'');
                s
            }
            Err(NW_EIFUNC) => {
                let name = first_ifunc(l).unwrap_or("?");
                let mut s = String::from(l.name);
                s.push_str(": ifunc resolver for `");
                s.push_str(name);
                s.push_str("' is not implemented");
                s
            }
            Err(NW_EBIND) => {
                let mut s = String::from(l.name);
                s.push_str(": invalid binding mode flags ");
                s.push_str(hex_text(l.flags).as_str());
                s
            }
            Err(_) => bind_text(l.name, "RTLD_NOW binding rejected"),
        }
    }
}

/// 首个故障符号名。
fn first_fault_sym(l: &Link) -> Option<&'static str> {
    let mut i = 0usize;
    while i < l.len() {
        if let Some(r) = l.reloc_at(i) {
            if r.kind.needs_sym() {
                return Some(r.target);
            }
        }
        i += 1;
    }
    None
}

/// 首个故障版本号。
fn first_fault_ver(l: &Link) -> &'static str {
    let mut i = 0usize;
    while i < l.len() {
        if let Some(r) = l.reloc_at(i) {
            if r.versioned {
                return r.req_version;
            }
        }
        i += 1;
    }
    ""
}

/// 首个被遮蔽符号。
fn first_hidden(l: &Link) -> Option<&'static str> {
    let mut i = 0usize;
    while i < l.len() {
        if let Some(r) = l.reloc_at(i) {
            if let Some((_, s)) = l.scope.hidden_hit(r.target, l.global()) {
                let _ = s;
                return Some(r.target);
            }
        }
        i += 1;
    }
    None
}

/// 首个坏类型号。
fn first_bad_reloc(l: &Link) -> u32 {
    let mut i = 0usize;
    while i < l.len() {
        if let Some(r) = l.reloc_at(i) {
            if !r.known() {
                return r.raw;
            }
        }
        i += 1;
    }
    0
}

/// 首个坏类型符号。
fn first_bad_reloc_sym(l: &Link) -> &'static str {
    let mut i = 0usize;
    while i < l.len() {
        if let Some(r) = l.reloc_at(i) {
            if !r.known() {
                return r.target;
            }
        }
        i += 1;
    }
    ""
}

/// 首个 IFUNC 缺 resolver。
fn first_ifunc(l: &Link) -> Option<&'static str> {
    let mut i = 0usize;
    while i < l.len() {
        if let Some(r) = l.reloc_at(i) {
            if let Some((_, s)) = l.scope.find_defined(r.target) {
                if s.bind == Bind::Ifunc && !s.resolver {
                    return Some(r.target);
                }
            }
        }
        i += 1;
    }
    None
}

/// 执行一次 NOW 绑定。
pub fn bind(l: &Link) -> Judge {
    let mut tr = Trace::blank();

    if !l.mode_ok() || !l.is_now() {
        tr.note(Stage::Flags, false);
        return Judge { result: Err(NW_EBIND), trace: tr };
    }
    tr.note(Stage::Flags, true);

    // 重定位类型。
    let mut i = 0usize;
    while i < l.len() {
        match l.reloc_at(i) {
            Some(r) => {
                if !r.known() {
                    tr.note(Stage::Reloc, false);
                    return Judge { result: Err(NW_ERELOC), trace: tr };
                }
            }
            None => {
                tr.note(Stage::Reloc, false);
                return Judge { result: Err(NW_EVALUE), trace: tr };
            }
        }
        i += 1;
    }
    tr.note(Stage::Reloc, true);

    let mut view = BoundView { resolved: 0, weak_zero: 0, ifunc: 0, eager: true };
    i = 0usize;
    while i < l.len() {
        let r = match l.reloc_at(i) {
            Some(v) => v,
            None => {
                tr.note(Stage::Scope, false);
                return Judge { result: Err(NW_EVALUE), trace: tr };
            }
        };
        i += 1;
        if !r.kind.needs_sym() {
            view.resolved = view.resolved.saturating_add(1);
            continue;
        }
        // 作用域：LOCAL 库里的 hidden 定义不可见。
        if l.scope.hidden_hit(r.target, l.global()).is_some() {
            tr.note(Stage::Scope, false);
            return Judge { result: Err(NW_ESCOPE), trace: tr };
        }
        let found = l.scope.find_defined(r.target);
        let sym = match found {
            Some((_, s)) => s,
            None => {
                if l.scope.find_weak(r.target).is_some() {
                    // 弱未定义：NOW 档落零地址，不拒。
                    view.weak_zero = view.weak_zero.saturating_add(1);
                    continue;
                }
                tr.note(Stage::Scope, false);
                return Judge { result: Err(NW_EUNDEF), trace: tr };
            }
        };
        if !sym.version_ok(r.req_version, r.versioned) {
            tr.note(Stage::Version, false);
            return Judge { result: Err(NW_EVER), trace: tr };
        }
        if sym.bind == Bind::Ifunc {
            if !sym.resolver {
                tr.note(Stage::Ifunc, false);
                return Judge { result: Err(NW_EIFUNC), trace: tr };
            }
            view.ifunc = view.ifunc.saturating_add(1);
        }
        view.resolved = view.resolved.saturating_add(1);
    }
    tr.note(Stage::Done, true);
    Judge { result: Ok(view), trace: tr }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 场景。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// 全解析。
    AllNow,
    /// 弱零。
    WeakNow,
    /// 依赖命中。
    DepNow,
    /// 版本命中。
    VerNow,
    /// IFUNC 命中。
    IfuncNow,
    /// GLOBAL 可见。
    GlobalNow,
    /// 数据槽。
    DataNow,
    /// 空表。
    EmptyNow,
    /// 强未定义。
    UndefNow,
    /// 版本缺失。
    VerBadNow,
    /// LOCAL 遮蔽。
    LocalNow,
    /// 坏类型。
    RelocBadNow,
    /// IFUNC 缺 resolver。
    IfuncBadNow,
    /// 旗标非法。
    FlagBadNow,
}

impl Scenario {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            Scenario::AllNow => "all-now",
            Scenario::WeakNow => "weak-now",
            Scenario::DepNow => "dep-now",
            Scenario::VerNow => "ver-now",
            Scenario::IfuncNow => "ifunc-now",
            Scenario::GlobalNow => "global-now",
            Scenario::DataNow => "data-now",
            Scenario::EmptyNow => "empty-now",
            Scenario::UndefNow => "undef-now",
            Scenario::VerBadNow => "ver-bad-now",
            Scenario::LocalNow => "local-now",
            Scenario::RelocBadNow => "reloc-bad-now",
            Scenario::IfuncBadNow => "ifunc-bad-now",
            Scenario::FlagBadNow => "flag-bad-now",
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
    /// 接受。
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
        name: "all",
        scenario: Scenario::AllNow,
        expect: 0,
        note: "malloc 与 printf 都在主程序，NOW 两条全解析",
    },
    Case {
        name: "weak",
        scenario: Scenario::WeakNow,
        expect: 0,
        note: "maybe 是弱未定义，NOW 档落零地址不拒",
    },
    Case {
        name: "dep",
        scenario: Scenario::DepNow,
        expect: 0,
        note: "sin 由依赖 libm 提供，作用域第二段命中",
    },
    Case {
        name: "ver",
        scenario: Scenario::VerNow,
        expect: 0,
        note: "memcpy 需求 GLIBC_2.34，提供方恰好是 2.34",
    },
    Case {
        name: "ifunc",
        scenario: Scenario::IfuncNow,
        expect: 0,
        note: "strlen 是 IFUNC，resolver 已实现",
    },
    Case {
        name: "global",
        scenario: Scenario::GlobalNow,
        expect: 0,
        note: "RTLD_GLOBAL 下 LOCAL 库的 hidden 符号可见",
    },
    Case {
        name: "data",
        scenario: Scenario::DataNow,
        expect: 0,
        note: "stderr 是 GLOB_DAT 数据槽，NOW 立即写槽",
    },
    Case {
        name: "empty",
        scenario: Scenario::EmptyNow,
        expect: 0,
        note: "零重定位对象 NOW 档直接成功",
    },
    Case {
        name: "undef",
        scenario: Scenario::UndefNow,
        expect: NW_EUNDEF,
        note: "missing_fn 强未定义，装载期即拒",
    },
    Case {
        name: "verbad",
        scenario: Scenario::VerBadNow,
        expect: NW_EVER,
        note: "需求 GLIBC_2.99 只有 2.34，版本面拒",
    },
    Case {
        name: "local",
        scenario: Scenario::LocalNow,
        expect: NW_ESCOPE,
        note: "internal_fn 在 RTLD_LOCAL 库内 hidden，外部不可见",
    },
    Case {
        name: "relocbad",
        scenario: Scenario::RelocBadNow,
        expect: NW_ERELOC,
        note: "重定位类型 99 本实现不认识",
    },
    Case {
        name: "ifuncbad",
        scenario: Scenario::IfuncBadNow,
        expect: NW_EIFUNC,
        note: "memmove 声明 IFUNC 但 resolver 缺失",
    },
    Case {
        name: "flagbad",
        scenario: Scenario::FlagBadNow,
        expect: NW_EBIND,
        note: "LAZY|NOW 同时置位，模式位非法",
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
    let _ = main.push(Sym::strong("malloc", ""));
    let _ = main.push(Sym::strong("printf", ""));
    let _ = main.push(Sym::weak_undef("maybe"));
    let _ = main.push(Sym::strong("memcpy", "GLIBC_2.34"));
    let mut s = Scope::blank();
    let _ = s.push(main);
    s
}

/// 依赖 libm 作用域。
fn dep_scope() -> Scope {
    let mut s = main_scope();
    let mut libm = ScopeObj::blank("libm.so.6");
    let _ = libm.push(Sym::strong("sin", ""));
    let _ = libm.push(Sym::strong("cos", ""));
    let _ = libm.push(Sym::ifunc("strlen", true));
    let _ = s.push(libm);
    s
}

/// 造请求。
pub fn link_for(sc: Scenario) -> Link {
    let mut l = Link::new("./libnow.so", BindMode::Now, RTLD_NOW, main_scope());
    match sc {
        Scenario::AllNow => {
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "malloc"));
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "printf"));
        }
        Scenario::WeakNow => {
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "maybe"));
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "malloc"));
        }
        Scenario::DepNow => {
            l.scope = dep_scope();
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "sin"));
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "cos"));
        }
        Scenario::VerNow => {
            let _ = l.push(Reloc::versioned(RelocKind::JumpSlot, "memcpy", "GLIBC_2.34"));
        }
        Scenario::IfuncNow => {
            l.scope = dep_scope();
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "strlen"));
        }
        Scenario::GlobalNow => {
            l.flags = RTLD_NOW | RTLD_GLOBAL;
            let mut local = ScopeObj::blank("liblocal.so");
            let mut s = Sym::strong("puts", "");
            s.local = true;
            s.hidden = true;
            let _ = local.push(s);
            let _ = l.scope.push(local);
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "puts"));
        }
        Scenario::DataNow => {
            let _ = l.push(Reloc::new(RelocKind::GlobDat, "stderr"));
            let mut data = ScopeObj::blank("libdata.so");
            let _ = data.push(Sym::strong("stderr", ""));
            let _ = l.scope.push(data);
        }
        Scenario::EmptyNow => {}
        Scenario::UndefNow => {
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "missing_fn"));
        }
        Scenario::VerBadNow => {
            let _ = l.push(Reloc::versioned(RelocKind::JumpSlot, "memcpy", "GLIBC_2.99"));
        }
        Scenario::LocalNow => {
            let mut local = ScopeObj::blank("liblocal.so");
            let mut s = Sym::strong("internal_fn", "");
            s.mark_hidden();
            let _ = local.push(s);
            let _ = l.scope.push(local);
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "internal_fn"));
        }
        Scenario::RelocBadNow => {
            let _ = l.push(Reloc::new(RelocKind::Unknown, "weird_fn"));
        }
        Scenario::IfuncBadNow => {
            let mut s = l.scope;
            let mut libm = ScopeObj::blank("libm.so.6");
            let _ = libm.push(Sym::ifunc("memmove", false));
            let _ = s.push(libm);
            l.scope = s;
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "memmove"));
        }
        Scenario::FlagBadNow => {
            l.flags = RTLD_NOW | RTLD_LAZY;
            let _ = l.push(Reloc::new(RelocKind::JumpSlot, "malloc"));
        }
    }
    l
}

/// 求解。
pub fn solve(c: &Case) -> Judge {
    bind(&link_for(c.scenario))
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
        if c.name.is_empty() || c.note.is_empty() {
            return false;
        }
        if c.scenario.label().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 逐槽。
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
        if c.accept() && !j.passed() {
            return false;
        }
        if !c.accept() && j.passed() {
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
    symbol: usize,
    version: usize,
    reloc: usize,
    mode: usize,
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
            symbol: 0,
            version: 0,
            reloc: 0,
            mode: 0,
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

    /// 符号桶。
    pub const fn symbol(&self) -> usize {
        self.symbol
    }

    /// 版本桶。
    pub const fn version(&self) -> usize {
        self.version
    }

    /// 重定位桶。
    pub const fn reloc(&self) -> usize {
        self.reloc
    }

    /// 模式桶。
    pub const fn mode(&self) -> usize {
        self.mode
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
            return Err(NW_ENOBS);
        }
        if self.total != CORPUS {
            return Err(NW_EVALUE);
        }
        if self.accept != 8 || self.reject != 6 {
            return Err(NW_EBAL);
        }
        if self.symbol + self.version + self.reloc + self.mode != self.reject {
            return Err(NW_EBAL);
        }
        if self.red {
            return Err(NW_ESTATE);
        }
        if self.matched != self.total {
            return Err(NW_EVERIFY);
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
        let mut s = String::from("nowbind corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" symbol=");
        s.push_str(u32_text(self.symbol as u32).as_str());
        s.push_str(" version=");
        s.push_str(u32_text(self.version as u32).as_str());
        s.push_str(" reloc=");
        s.push_str(u32_text(self.reloc as u32).as_str());
        s.push_str(" mode=");
        s.push_str(u32_text(self.mode as u32).as_str());
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
            None => return Err(NW_EVALUE),
        };
        let j = solve(&c);
        r.total += 1;
        if j.passed() {
            r.accept += 1;
        } else {
            r.reject += 1;
            match j.code() {
                NW_EUNDEF | NW_ESCOPE => r.symbol += 1,
                NW_EVER => r.version += 1,
                NW_ERELOC | NW_EIFUNC => r.reloc += 1,
                NW_EBIND => r.mode += 1,
                _ => return Err(NW_EVERIFY),
            }
        }
        if j.code() == c.expect {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, code)) = red {
        if slot >= r.total {
            return Err(NW_ESLOT);
        }
        if code == 0 {
            return Err(NW_EVALUE);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 前缀封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(NW_ESLOT);
    }
    let mut i = 0usize;
    while i < n {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return Err(NW_EVALUE),
        };
        if solve(&c).code() != c.expect {
            return Err(NW_EVALUE);
        }
        i += 1;
    }
    Ok(())
}

/// 槽码。
fn case_code(i: usize) -> u32 {
    match slot_at(i) {
        Some(c) => solve(&c).code(),
        None => NW_EVALUE,
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
            return Err(NW_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NW_ESLOT);
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
            return Err(NW_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 2, 1, 2, 1, 929];
        const CODES: [u32; 6] =
            [NW_EUNDEF, NW_EVER, NW_ESCOPE, NW_ERELOC, NW_EIFUNC, NW_EBIND];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NW_EBAL),
            };
            if s.slot as usize != i {
                return Err(NW_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NW_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NW_EVALUE);
                        }
                    }
                    None => return Err(NW_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NW_EVALUE);
                        }
                    }
                    None => return Err(NW_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(NW_EBAL);
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
            return Err(NW_ESTATE);
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
            Err(NW_ESEAL)
        }
    }

    /// 指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 冻结。
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
    let rr = run_with(Some((0, NW_EUNDEF)));
    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_SCOPE, r.map(|x| x.symbol())));
    let _ = l.register(count_cell(4, OP_VERSION, r.map(|x| x.version())));
    let _ = l.register(count_cell(5, OP_RELOC, r.map(|x| x.reloc())));
    let _ = l.register(count_cell(6, OP_BIND, r.map(|x| x.mode())));
    let _ = l.register(count_cell(7, OP_TEXT, rr.map(|x| x.rate() as usize)));
    let _ = l.register(err_cell(8, OP_SCOPE, case_code(8)));
    let _ = l.register(err_cell(9, OP_VERSION, case_code(9)));
    let _ = l.register(err_cell(10, OP_SCOPE, case_code(10)));
    let _ = l.register(err_cell(11, OP_RELOC, case_code(11)));
    let _ = l.register(err_cell(12, OP_RELOC, case_code(12)));
    let _ = l.register(err_cell(13, OP_BIND, case_code(13)));
    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 模式与旗标。
fn mode_ok() -> bool {
    let now = Link::new("./a", BindMode::Now, RTLD_NOW, Scope::blank());
    let a = now.mode_ok() && now.is_now() && !now.global();
    let lazy = Link::new("./b", BindMode::Lazy, RTLD_LAZY, Scope::blank());
    let b = lazy.mode_ok() && !lazy.is_now();
    let both = Link::new("./c", BindMode::Now, RTLD_NOW | RTLD_LAZY, Scope::blank());
    let c = !both.mode_ok();
    let none = Link::new("./d", BindMode::Now, 0, Scope::blank());
    let d = !none.mode_ok();
    let gl = Link::new("./e", BindMode::Now, RTLD_NOW | RTLD_GLOBAL, Scope::blank());
    let e = gl.mode_ok() && gl.global();
    let junk = Link::new("./f", BindMode::Now, RTLD_NOW | 0x4000, Scope::blank());
    let f = !junk.mode_ok();
    a && b && c && d && e && f
}

/// 符号与绑定。
fn sym_ok() -> bool {
    let s = Sym::strong("malloc", "");
    let a = s.defined && s.visible(false) && s.version_ok("", false);
    let m = Sym::strong("memcpy", "GLIBC_2.34");
    let b = m.version_ok("GLIBC_2.34", true) && !m.version_ok("GLIBC_2.2.5", true);
    let w = Sym::weak_undef("maybe");
    let c = !w.defined && w.bind == Bind::Weak && !w.visible(true);
    let i = Sym::ifunc("strlen", true);
    let d = i.defined && i.bind == Bind::Ifunc && i.resolver;
    let mut h = Sym::strong("internal", "");
    h.mark_hidden();
    let e = h.hidden && h.local && !h.visible(false) && h.visible(true);
    let f = Bind::Strong.label() == "strong" && Bind::Ifunc.label() == "ifunc";
    a && b && c && d && e && f
}

/// 作用域。
fn scope_ok() -> bool {
    let s = dep_scope();
    let a = s.len() == 2 && s.obj_at(0).map(|o| o.name) == Some("./app");
    let b = s.find_defined("malloc").map(|x| x.0) == Some(0);
    let c = s.find_defined("sin").map(|x| x.0) == Some(1);
    let d = s.find_defined("nope").is_none();
    let e = s.find_weak("maybe").is_some();
    let f = s.find_weak("malloc").is_none();
    let mut local = s;
    let mut o = ScopeObj::blank("liblocal.so");
    let mut h = Sym::strong("internal_fn", "");
    h.mark_hidden();
    let _ = o.push(h);
    let _ = local.push(o);
    let g = local.hidden_hit("internal_fn", false).is_some();
    let h2 = local.hidden_hit("internal_fn", true).is_none();
    a && b && c && d && e && f && g && h2
}

/// 重定位种类。
fn reloc_ok() -> bool {
    let a = RelocKind::JumpSlot.code() == 7 && RelocKind::GlobDat.code() == 6;
    let b = RelocKind::Relative.needs_sym() == false && RelocKind::JumpSlot.needs_sym();
    let c = RelocKind::Unknown.code() == 99 && !Reloc::new(RelocKind::Unknown, "x").known();
    let d = Reloc::new(RelocKind::Tls, "errno").known();
    let e = Reloc::versioned(RelocKind::JumpSlot, "memcpy", "GLIBC_2.34").versioned;
    let f = RelocKind::GlobDat.label() == "glob-dat";
    a && b && c && d && e && f
}

/// 全部立即绑定。
fn all_now_ok() -> bool {
    let l = link_for(Scenario::AllNow);
    let j = bind(&l);
    let a = j.passed() && j.code() == 0;
    let v = match j.view() {
        Some(v) => v,
        None => return false,
    };
    let b = v.resolved == 2 && v.weak_zero == 0 && v.eager;
    let c = l.len() == 2 && l.reloc_at(2).is_none();
    let d = j.trace.last() == Some((Stage::Done, true));
    let e = j.text(&l).as_str() == "now-ok ./libnow.so resolved=2 weak=0 ifunc=0";
    a && b && c && d && e
}

/// 弱零。
fn weak_ok() -> bool {
    let l = link_for(Scenario::WeakNow);
    let j = bind(&l);
    let v = match j.view() {
        Some(v) => v,
        None => return false,
    };
    j.passed() && v.weak_zero == 1 && v.resolved == 1 && v.complete()
}

/// 依赖命中。
fn dep_ok() -> bool {
    let l = link_for(Scenario::DepNow);
    let j = bind(&l);
    let v = match j.view() {
        Some(v) => v,
        None => return false,
    };
    j.passed() && v.resolved == 2 && l.scope.len() == 2
}

/// 版本命中。
fn ver_ok() -> bool {
    let l = link_for(Scenario::VerNow);
    let j = bind(&l);
    let a = j.passed() && j.view().map(|v| v.resolved) == Some(1);
    let bad = link_for(Scenario::VerBadNow);
    let b = bind(&bad).code() == NW_EVER;
    let c = bind(&bad)
        .text(&bad)
        .contains("version `GLIBC_2.99' not found (required by ./libnow.so)");
    a && b && c
}

/// IFUNC。
fn ifunc_ok() -> bool {
    let good = link_for(Scenario::IfuncNow);
    let a = bind(&good).passed();
    let b = bind(&good).view().map(|v| v.ifunc) == Some(1);
    let bad = link_for(Scenario::IfuncBadNow);
    let c = bind(&bad).code() == NW_EIFUNC;
    let d = bind(&bad).text(&bad).contains("ifunc resolver for `memmove'");
    a && b && c && d
}

/// 全局与数据槽。
fn global_data_ok() -> bool {
    let g = link_for(Scenario::GlobalNow);
    let a = g.global() && bind(&g).passed();
    let d = link_for(Scenario::DataNow);
    let b = bind(&d).passed();
    let e = link_for(Scenario::EmptyNow);
    let j = bind(&e);
    let c = j.passed() && j.view().map(|v| v.resolved) == Some(0);
    let f = bind(&d).view().map(|v| v.resolved) == Some(1);
    a && b && c && f
}

/// 六类拒收。
fn rejects_ok() -> bool {
    let undef = link_for(Scenario::UndefNow);
    let a = bind(&undef).code() == NW_EUNDEF && bind(&undef).text(&undef).as_str().ends_with("undefined symbol: missing_fn");
    let local = link_for(Scenario::LocalNow);
    let b = bind(&local).code() == NW_ESCOPE;
    let reloc = link_for(Scenario::RelocBadNow);
    let c = bind(&reloc).code() == NW_ERELOC && bind(&reloc).text(&reloc).contains("relocation type 99");
    let flag = link_for(Scenario::FlagBadNow);
    let d = bind(&flag).code() == NW_EBIND && bind(&flag).text(&flag).contains("invalid binding mode flags 0x00000003");
    let e = bind(&link_for(Scenario::VerBadNow)).code() == NW_EVER;
    let f = bind(&link_for(Scenario::IfuncBadNow)).code() == NW_EIFUNC;
    a && b && c && d && e && f
}

/// 轨迹。
fn trace_ok() -> bool {
    let good = bind(&link_for(Scenario::AllNow));
    let a = good.trace.at(0) == Some((Stage::Flags, true));
    let b = good.trace.last() == Some((Stage::Done, true));
    let c = good.trace.len() == 3;
    let bad = bind(&link_for(Scenario::UndefNow));
    let d = bad.trace.last() == Some((Stage::Scope, false));
    let e = Stage::Version.label() == "version" && Stage::Ifunc.label() == "ifunc";
    let f = good.trace.at(9).is_none();
    a && b && c && d && e && f
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
        && cover(15) == Err(NW_ESLOT)
        && slot_at(14).is_none()
        && slot_at(0).map(|c| c.scenario) == Some(Scenario::AllNow)
        && slot_at(13).map(|c| c.scenario) == Some(Scenario::FlagBadNow)
}

/// 渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render()
        == "nowbind corpus=14 accept=8 reject=6 symbol=2 version=1 reloc=2 mode=1 match=14 rate=1000ppm\n";
    let b = r.rate() == 1000 && r.matched() == 14;
    let c = r.green() && r.symbol() + r.version() + r.reloc() + r.mode() == 6;
    a && b && c
}

/// 红档。
fn reds_ok() -> bool {
    let red = match run_with(Some((5, NW_EUNDEF))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = red.matched() == 13 && red.rate() == 929;
    let b = !red.green() && red.gate() == Err(NW_ESTATE);
    let c = run_with(Some((14, NW_EUNDEF))) == Err(NW_ESLOT);
    let d = run_with(Some((0, 0))) == Err(NW_EVALUE);
    a && b && c && d
}

/// 指纹。
fn hash_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(929) == "929";
    let b = hex_text(0xfe30) == "0x0000fe30" && hex_text(3) == "0x00000003";
    let c = fold_word(FNV_OFF, 0) == digest(&[0u8; 8]);
    let d = digest(b"UNX") != digest(b"UNY");
    a && b && c && d
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
    let e = l.register(Sample::blank()) == Err(NW_ESEAL);
    let f = l.rejects() == 1 && l.freeze() == Err(NW_ESTATE);
    let g = l.sample(7).map(|s| s.value) == Some(929);
    let h = build_ledger().seal() == root;
    a && b && c && d && e && f && g && h
}

/// 四注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = mode_ok() && sym_ok() && scope_ok();
    out[1] = reloc_ok() && all_now_ok() && weak_ok() && dep_ok();
    out[2] = ver_ok() && ifunc_ok() && global_data_ok() && rejects_ok();
    out[3] = trace_ok()
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
pub fn run_unx11630_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11630.now_undef");

    s.add("NW01-绑定模式与旗标位", mode_ok(), "static");
    s.add("NW02-符号绑定类型", sym_ok(), "static");
    s.add("NW03-作用域查找顺序", scope_ok(), "static");
    s.add("NW04-重定位种类与版本字段", reloc_ok(), "static");
    s.add("NW05-NOW 全解析", all_now_ok(), "static");
    s.add("NW06-弱未定义零地址", weak_ok(), "static");
    s.add("NW07-依赖库命中", dep_ok(), "static");
    s.add("NW08-版本匹配与版本拒", ver_ok(), "static");
    s.add("NW09-IFUNC 与 resolver", ifunc_ok(), "static");
    s.add("NW10-GLOBAL 与数据槽", global_data_ok(), "static");
    s.add("NW11-六类拒收与逐字文案", rejects_ok(), "static");
    s.add("NW12-十四槽语料渲染与红档", trace_ok() && corpus_ok() && slots_ok() && report_ok() && reds_ok(), "static");
    s.add("NW13-冻结重放四注入", inject_four() == [true; 4], "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(l.register(Sample::blank()), Err(NW_ESEAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true; 4]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11630_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn text_tools() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0xfe30).as_str(), "0x0000fe30");
        assert_eq!(bind_text("a", "b").as_str(), "a: b");
    }

    #[test]
    fn fnv_known() {
        let mut h = FNV_OFF;
        h = fold_byte(h, b'a');
        assert_eq!(h, 0xaf63dc4c8601ec8c);
        assert_ne!(fold_word(FNV_OFF, 2), FNV_OFF);
    }

    #[test]
    fn mode_bits() {
        let now = Link::new("./a", BindMode::Now, RTLD_NOW, Scope::blank());
        assert!(now.mode_ok());
        assert!(now.is_now());
        assert!(!now.global());
        let lazy = Link::new("./b", BindMode::Lazy, RTLD_LAZY, Scope::blank());
        assert!(lazy.mode_ok());
        assert!(!lazy.is_now());
        assert_eq!(BindMode::Now.bit(), RTLD_NOW);
        assert_eq!(BindMode::Lazy.bit(), RTLD_LAZY);
    }

    #[test]
    fn mode_bad() {
        let both = Link::new("./c", BindMode::Now, RTLD_NOW | RTLD_LAZY, Scope::blank());
        assert!(!both.mode_ok());
        let none = Link::new("./d", BindMode::Now, 0, Scope::blank());
        assert!(!none.mode_ok());
        let junk = Link::new("./e", BindMode::Now, RTLD_NOW | 0x4000, Scope::blank());
        assert!(!junk.mode_ok());
        let j = bind(&both);
        assert_eq!(j.code(), NW_EBIND);
        assert_eq!(j.trace.last(), Some((Stage::Flags, false)));
    }

    #[test]
    fn sym_visibility() {
        let s = Sym::strong("malloc", "");
        assert!(s.visible(false));
        let mut h = Sym::strong("internal", "");
        h.mark_hidden();
        assert!(!h.visible(false));
        assert!(h.visible(true));
        let w = Sym::weak_undef("maybe");
        assert!(!w.visible(true));
        assert_eq!(w.bind, Bind::Weak);
    }

    #[test]
    fn sym_versions() {
        let m = Sym::strong("memcpy", "GLIBC_2.34");
        assert!(m.version_ok("GLIBC_2.34", true));
        assert!(!m.version_ok("GLIBC_2.99", true));
        assert!(m.version_ok("", false));
        let i = Sym::ifunc("strlen", true);
        assert!(i.resolver);
        assert_eq!(i.bind, Bind::Ifunc);
    }

    #[test]
    fn scope_order() {
        let s = dep_scope();
        assert_eq!(s.len(), 2);
        assert_eq!(s.find_defined("malloc").map(|x| x.0), Some(0));
        assert_eq!(s.find_defined("sin").map(|x| x.0), Some(1));
        assert!(s.find_defined("nope").is_none());
        assert!(s.find_weak("maybe").is_some());
    }

    #[test]
    fn reloc_kinds() {
        assert_eq!(RelocKind::JumpSlot.code(), 7);
        assert_eq!(RelocKind::GlobDat.code(), 6);
        assert_eq!(RelocKind::Relative.code(), 8);
        assert!(!RelocKind::Relative.needs_sym());
        assert!(RelocKind::Unknown.needs_sym());
        assert!(!Reloc::new(RelocKind::Unknown, "x").known());
        assert!(Reloc::new(RelocKind::Tls, "errno").known());
    }

    #[test]
    fn all_resolved() {
        let l = link_for(Scenario::AllNow);
        let j = bind(&l);
        assert!(j.passed());
        let v = match j.view() {
            Some(v) => v,
            None => return,
        };
        assert_eq!(v.resolved, 2);
        assert_eq!(v.weak_zero, 0);
        assert!(v.eager);
    }

    #[test]
    fn weak_zero_allowed() {
        let l = link_for(Scenario::WeakNow);
        let j = bind(&l);
        assert!(j.passed());
        let v = match j.view() {
            Some(v) => v,
            None => return,
        };
        assert_eq!(v.weak_zero, 1);
        assert_eq!(v.resolved, 1);
    }

    #[test]
    fn dependency_hit() {
        let l = link_for(Scenario::DepNow);
        let j = bind(&l);
        assert!(j.passed());
        assert_eq!(j.view().map(|v| v.resolved), Some(2));
    }

    #[test]
    fn version_hit_and_miss() {
        let good = link_for(Scenario::VerNow);
        assert!(bind(&good).passed());
        let bad = link_for(Scenario::VerBadNow);
        let j = bind(&bad);
        assert_eq!(j.code(), NW_EVER);
        assert_eq!(j.trace.last(), Some((Stage::Version, false)));
        assert!(j.text(&bad).contains("GLIBC_2.99"));
    }

    #[test]
    fn ifunc_paths() {
        let good = link_for(Scenario::IfuncNow);
        assert!(bind(&good).passed());
        assert_eq!(bind(&good).view().map(|v| v.ifunc), Some(1));
        let bad = link_for(Scenario::IfuncBadNow);
        assert_eq!(bind(&bad).code(), NW_EIFUNC);
    }

    #[test]
    fn global_visibility() {
        let l = link_for(Scenario::GlobalNow);
        assert!(l.global());
        assert!(bind(&l).passed());
        let local = link_for(Scenario::LocalNow);
        assert_eq!(bind(&local).code(), NW_ESCOPE);
    }

    #[test]
    fn data_and_empty() {
        let d = link_for(Scenario::DataNow);
        assert!(bind(&d).passed());
        assert_eq!(bind(&d).view().map(|v| v.resolved), Some(1));
        let e = link_for(Scenario::EmptyNow);
        let j = bind(&e);
        assert!(j.passed());
        assert_eq!(j.view().map(|v| v.resolved), Some(0));
    }

    #[test]
    fn undefined_text() {
        let l = link_for(Scenario::UndefNow);
        let j = bind(&l);
        assert_eq!(j.code(), NW_EUNDEF);
        assert_eq!(j.text(&l).as_str(), "./libnow.so: undefined symbol: missing_fn");
        assert_eq!(j.trace.last(), Some((Stage::Scope, false)));
    }

    #[test]
    fn unsupported_reloc() {
        let l = link_for(Scenario::RelocBadNow);
        let j = bind(&l);
        assert_eq!(j.code(), NW_ERELOC);
        assert_eq!(j.trace.last(), Some((Stage::Reloc, false)));
        assert!(j.text(&l).contains("relocation type 99 for `weird_fn'"));
    }

    #[test]
    fn flag_reject_text() {
        let l = link_for(Scenario::FlagBadNow);
        let j = bind(&l);
        assert_eq!(j.code(), NW_EBIND);
        assert!(j.text(&l).ends_with("invalid binding mode flags 0x00000003"));
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
            "nowbind corpus=14 accept=8 reject=6 symbol=2 version=1 reloc=2 mode=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rate() {
        let r = match run_with(Some((2, NW_EUNDEF))) {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(NW_ESTATE));
    }

    #[test]
    fn cover_prefix() {
        assert_eq!(cover(0), Ok(()));
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(15), Err(NW_ESLOT));
    }

    #[test]
    fn error_codes_distinct() {
        let codes = [
            NW_EUNDEF,
            NW_EVER,
            NW_ESCOPE,
            NW_ERELOC,
            NW_EIFUNC,
            NW_EBIND,
            NW_ESTATE,
            NW_EVERIFY,
            NW_EBAL,
            NW_ESLOT,
            NW_EVALUE,
            NW_ESEAL,
            NW_ENOBS,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i], SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }
}