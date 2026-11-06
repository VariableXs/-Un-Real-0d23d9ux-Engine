//! UNX-F11638 dlsym 未定义符号 NULL 判据
//!
//! 判据原文：
//! UNX-F11638-J1 缺符号 NULL+dlerror 判据绿，与基准机一致。
//!
//! 本模块把 `dlsym` 的“查得到 / 查不到 / 查到了但不可用”三种结局拆成可判定
//! 模型：句柄先过合法性闸门，符号名再拆分出版本段，作用域按装载顺序扫描；
//! 查不到出 `E_DSSYM`，查到了但定义为 `SHN_UNDEF` 的强符号出 `E_DSNULL`，
//! 弱未定义符号返回空值但不置错，三者都不能混为一谈。
//!
//! 闸门次序：句柄 → 符号 → 版本 → 绑定 → 就绪
//!
//! 1. 句柄：`0` 表示默认作用域，非法句柄在查任何符号之前就被拒，
//!    错误码与“符号缺失”不同，且不产生任何符号查找副作用。
//! 2. 符号：符号名先按 `@` 拆成基名与版本段；基名按作用域顺序查找，
//!    缺省句柄从第一个装载对象开始扫，具体句柄只看那一个对象。
//! 3. 版本：请求带版本段时，符号的版本串必须逐字相等；不等即版本未命中，
//!    该错误在“未定义 / 弱 / 已定义”判定之前终止，先于绑定语义。
//! 4. 绑定：强未定义符号置 `E_DSNULL` 并返回空值，弱未定义符号返回空值
//!    但不置错——`dlsym` 的返回值都为 `NULL`，区别只在线程错误串上。
//! 5. 就绪：已定义符号返回地址值，并清空上一次的错误串。
//!
//! 关键差异（本卡深度所在）：
//! - 返回值同为 `NULL` 的三条路径必须靠 `dlerror` 区分：缺符号、强未定义、
//!   弱未定义；弱未定义即使返回 `NULL` 也属于成功调用，会清掉旧错误；
//! - 成功的 `dlsym` 会清空错误串，因此“先失败后成功再取错误”必须取到空；
//! - 错误串是一次性的：取走即清，第二次取到空；
//! - 版本未命中与符号缺失是两种不同的失败，错误码与文案都不同；
//! - 非法句柄比符号缺失更早短路，符号表里有没有这个名字都无关紧要。
//!
//! 本模块只做纯计算：符号表与作用域都是静态表，没有系统调用、没有 IO。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本模块段基址。
pub const SEG_BASE: u32 = 0xfcf0;

/// 请求形状错。
pub const E_DSREQ: u32 = SEG_BASE + 0x01;
/// 阶段次序错。
pub const E_DSSTAGE: u32 = SEG_BASE + 0x02;
/// 句柄错。
pub const E_DSHANDLE: u32 = SEG_BASE + 0x03;
/// 符号缺失错。
pub const E_DSSYM: u32 = SEG_BASE + 0x04;
/// 错误串清除错。
pub const E_DSCLEAR: u32 = SEG_BASE + 0x05;
/// 绑定阶段错。
pub const E_DSNULL: u32 = SEG_BASE + 0x06;
/// 地址值错。
pub const E_DSVALUE: u32 = SEG_BASE + 0x07;
/// 错误状态错。
pub const E_DSSTATE: u32 = SEG_BASE + 0x08;
/// 闸门逆序错。
pub const E_DSORDER: u32 = SEG_BASE + 0x09;
/// 覆盖面错。
pub const E_DSCOVER: u32 = SEG_BASE + 0x0a;
/// 文案名错。
pub const E_DSNAME: u32 = SEG_BASE + 0x0b;
/// 弱符号档错。
pub const E_DSWEAK: u32 = SEG_BASE + 0x0c;
/// 版本档错。
pub const E_DSVER: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;
/// 语料槽数。
pub const CORPUS: usize = 14;
/// 申报正确槽数。
pub const ACCEPT: usize = 8;
/// 申报错误槽数。
pub const REJECT: usize = 6;
/// 矩阵档数。
pub const TIER_COUNT: usize = 4;
/// 作用域容量。
pub const CAP: usize = 4;
/// 符号表容量。
pub const SYM_CAP: usize = 8;

/// 查找操作码。
pub const OP_HANDLE: u8 = 0x01;
/// 绑定操作码。
pub const OP_BIND: u8 = 0x02;
/// 缺失操作码。
pub const OP_SYMBOL: u8 = 0x03;
/// 版本操作码。
pub const OP_CLEAR: u8 = 0x04;
/// 弱符号操作码。
pub const OP_WEAK: u8 = 0x05;
/// 语料操作码。
pub const OP_CORPUS: u8 = 0x06;

/// FNV-1a 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 乘子。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 混入一字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

/// 混入一个字。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < 8 {
        x = fold_byte(x, (w >> (i * 8)) as u8);
        i += 1;
    }
    x
}

/// 混入一个字符串。
fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            x = fold_byte(x, *c);
        }
        i += 1;
    }
    x
}

// ---------------------------------------------------------------------------
// 数值文本
// ---------------------------------------------------------------------------

/// 十进制无符号文本。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    let mut x = v;
    while x != 0 && n < 10 {
        let d = (x % 10) as u8;
        if let Some(c) = buf.get_mut(n) {
            *c = b'0' + d;
        }
        x /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        if let Some(c) = buf.get(i) {
            s.push(*c as char);
        }
    }
    s
}

/// 无符号文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 有符号文本。
pub fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        let m = (v as i64).wrapping_neg() as u32;
        s.push_str(dec_text(m).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 十六进制文本，零值单独成 `0`。
pub fn hex_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 8];
    let mut n = 0usize;
    let mut x = v;
    while x != 0 && n < 8 {
        let d = (x & 0xf) as u8;
        if let Some(c) = buf.get_mut(n) {
            *c = if d < 10 { b'0' + d } else { b'a' + (d - 10) };
        }
        x >>= 4;
        n += 1;
    }
    let mut s = String::from("0x");
    let mut i = n;
    while i > 0 {
        i -= 1;
        if let Some(c) = buf.get(i) {
            s.push(*c as char);
        }
    }
    s
}

// ---------------------------------------------------------------------------
// 符号表
// ---------------------------------------------------------------------------

/// 一个符号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sym {
    /// 基名（不含版本段）。
    pub name: &'static str,
    /// 地址值（未定义为 0）。
    pub value: u32,
    /// 是否有实体定义（`st_shndx != SHN_UNDEF`）。
    pub defined: bool,
    /// 是否弱符号。
    pub weak: bool,
    /// 版本串（空串表示无版本）。
    pub ver: &'static str,
}

impl Sym {
    /// 空白符号。
    pub const fn blank() -> Sym {
        Sym { name: "", value: 0, defined: false, weak: false, ver: "" }
    }

    /// 已定义符号。
    pub fn defined(name: &'static str, value: u32, ver: &'static str) -> Sym {
        Sym { name, value, defined: true, weak: false, ver }
    }

    /// 强未定义符号。
    pub fn undefined(name: &'static str) -> Sym {
        Sym { name, value: 0, defined: false, weak: false, ver: "" }
    }

    /// 弱未定义符号。
    pub fn weak(name: &'static str) -> Sym {
        Sym { name, value: 0, defined: false, weak: true, ver: "" }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_str(FNV_OFF, self.name);
        h = fold_word(h, self.value as u64);
        h = fold_word(h, self.defined as u64);
        h = fold_word(h, self.weak as u64);
        fold_str(h, self.ver)
    }
}

/// 符号表。
#[derive(Clone, Copy, Debug)]
pub struct SymTab {
    syms: [Sym; SYM_CAP],
    len: usize,
}

impl SymTab {
    /// 空表。
    pub const fn new() -> SymTab {
        SymTab { syms: [Sym::blank(); SYM_CAP], len: 0 }
    }

    /// 追加。
    pub fn add(&mut self, s: Sym) -> Option<usize> {
        if self.len >= SYM_CAP {
            return None;
        }
        let i = self.len;
        if let Some(slot) = self.syms.get_mut(i) {
            *slot = s;
        }
        self.len += 1;
        Some(i)
    }

    /// 符号数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一个。
    pub fn get(&self, i: usize) -> Option<Sym> {
        if i >= self.len {
            return None;
        }
        self.syms.get(i).copied()
    }

    /// 按基名查。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.syms.get(i) {
                if s.name == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.syms.get(i) {
                h = fold_word(h, s.seal());
            }
            i += 1;
        }
        fold_word(h, self.len as u64)
    }
}

// ---------------------------------------------------------------------------
// 作用域
// ---------------------------------------------------------------------------

/// 一个装载对象。
#[derive(Clone, Copy, Debug)]
pub struct Lib {
    /// 对象名。
    pub name: &'static str,
    /// 符号表。
    pub syms: SymTab,
    /// 是否已装载。
    pub loaded: bool,
}

impl Lib {
    /// 空对象。
    pub const fn blank() -> Lib {
        Lib { name: "", syms: SymTab::new(), loaded: false }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_str(FNV_OFF, self.name);
        h = fold_word(h, self.syms.seal());
        fold_word(h, self.loaded as u64)
    }
}

/// 第一个对象。
pub fn lib_a() -> Lib {
    let mut t = SymTab::new();
    let _ = t.add(Sym::defined("square", 0x1000, "V1"));
    let _ = t.add(Sym::defined("shared", 0x2000, "V1"));
    let _ = t.add(Sym::defined("counter", 0x3000, "V2"));
    let _ = t.add(Sym::weak("weak_missing"));
    let _ = t.add(Sym::undefined("undef_strong"));
    Lib { name: "liba.so", syms: t, loaded: true }
}

/// 第二个对象。
pub fn lib_b() -> Lib {
    let mut t = SymTab::new();
    let _ = t.add(Sym::defined("shared", 0x9000, "V1"));
    let _ = t.add(Sym::defined("counter", 0x3100, "V2"));
    let _ = t.add(Sym::undefined("dangling"));
    let _ = t.add(Sym::weak("weak_b"));
    Lib { name: "libb.so", syms: t, loaded: true }
}

/// 全局作用域。
#[derive(Clone, Copy, Debug)]
pub struct Scope {
    libs: [Lib; CAP],
    len: usize,
}

impl Scope {
    /// 空作用域。
    pub const fn new() -> Scope {
        Scope { libs: [Lib::blank(); CAP], len: 0 }
    }

    /// 追加。
    pub fn add(&mut self, l: Lib) -> Option<usize> {
        if self.len >= CAP {
            return None;
        }
        let i = self.len;
        if let Some(slot) = self.libs.get_mut(i) {
            *slot = l;
        }
        self.len += 1;
        Some(i)
    }

    /// 对象数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一个。
    pub fn get(&self, i: usize) -> Option<Lib> {
        if i >= self.len {
            return None;
        }
        self.libs.get(i).copied()
    }

    /// 句柄是否合法（`0` 表示默认作用域）。
    pub fn valid_handle(&self, h: u32) -> bool {
        h == 0 || (h as usize) <= self.len
    }

    /// 句柄 → 下标。
    pub fn find_handle(&self, h: u32) -> Option<usize> {
        if h == 0 || h as usize > self.len {
            return None;
        }
        let i = (h - 1) as usize;
        if i < self.len {
            Some(i)
        } else {
            None
        }
    }

    /// 取某对象某符号。
    pub fn sym_at(&self, li: usize, si: usize) -> Option<Sym> {
        match self.libs.get(li) {
            Some(l) => l.syms.get(si),
            None => None,
        }
    }

    /// 按作用域顺序查基名。
    pub fn resolve(&self, h: u32, name: &str) -> Option<(usize, usize)> {
        if h == 0 {
            let mut i = 0usize;
            while i < self.len {
                if let Some(l) = self.libs.get(i) {
                    if l.loaded {
                        if let Some(s) = l.syms.find(name) {
                            return Some((i, s));
                        }
                    }
                }
                i += 1;
            }
            return None;
        }
        let i = (h - 1) as usize;
        if i >= self.len {
            return None;
        }
        match self.libs.get(i) {
            Some(l) => {
                if !l.loaded {
                    None
                } else {
                    l.syms.find(name).map(|s| (i, s))
                }
            }
            None => None,
        }
    }

    /// 搜索顺序。
    pub fn order(&self) -> [u8; CAP] {
        let mut out = [0xffu8; CAP];
        let mut i = 0usize;
        while i < self.len {
            if let Some(slot) = out.get_mut(i) {
                *slot = i as u8;
            }
            i += 1;
        }
        out
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.libs.get(i) {
                h = fold_word(h, l.seal());
            }
            i += 1;
        }
        fold_word(h, self.len as u64)
    }
}

/// 基准作用域：两个对象依序装载。
pub fn scope_two() -> Scope {
    let mut sc = Scope::new();
    let _ = sc.add(lib_a());
    let _ = sc.add(lib_b());
    sc
}

/// 单对象作用域。
pub fn scope_one() -> Scope {
    let mut sc = Scope::new();
    let _ = sc.add(lib_a());
    sc
}

// ---------------------------------------------------------------------------
// 线程错误串
// ---------------------------------------------------------------------------

/// 线程错误状态。
#[derive(Clone, Copy, Debug)]
pub struct Sys {
    /// 错误码。
    pub code: u32,
    /// 请求串。
    pub req: &'static str,
    /// 是否有未取走的错误。
    pub pending: bool,
}

impl Sys {
    /// 空状态。
    pub const fn new() -> Sys {
        Sys { code: 0, req: "", pending: false }
    }

    /// 置错。
    pub fn set(&mut self, code: u32, req: &'static str) {
        self.code = code;
        self.req = req;
        self.pending = true;
    }

    /// 清除。
    pub fn clear(&mut self) {
        self.code = 0;
        self.req = "";
        self.pending = false;
    }

    /// 是否置错。
    pub const fn is_pending(&self) -> bool {
        self.pending
    }

    /// 取错误文本（取走即清）。
    pub fn take_text(&mut self) -> Option<String> {
        if !self.pending {
            return None;
        }
        let t = error_for(self.code, self.req);
        self.clear();
        Some(t)
    }

    /// 只看错误文本（不取走）。
    pub fn peek_text(&self) -> Option<String> {
        if !self.pending {
            return None;
        }
        Some(error_for(self.code, self.req))
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.code as u64);
        h = fold_str(h, self.req);
        fold_word(h, self.pending as u64)
    }
}

// ---------------------------------------------------------------------------
// 名字与版本拆分
// ---------------------------------------------------------------------------

/// 按 `@` 拆出基名和版本段。
pub fn split_ver(name: &str) -> (&str, &str) {
    let b = name.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'@' {
                let base = match name.get(..i) {
                    Some(v) => v,
                    None => name,
                };
                let mut j = i + 1;
                while j < b.len() {
                    if let Some(d) = b.get(j) {
                        if *d != b'@' {
                            break;
                        }
                    }
                    j += 1;
                }
                let ver = match name.get(j..) {
                    Some(v) => v,
                    None => "",
                };
                return (base, ver);
            }
        }
        i += 1;
    }
    (name, "")
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// 解析阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 句柄闸门。
    Handle,
    /// 符号闸门。
    Symbol,
    /// 绑定闸门。
    Bind,
    /// 就绪。
    Ready,
}

impl Stage {
    /// 次序号。
    pub const fn order(self) -> u8 {
        match self {
            Stage::Handle => 0,
            Stage::Symbol => 1,
            Stage::Bind => 2,
            Stage::Ready => 3,
        }
    }

    /// 文本名。
    pub const fn name(self) -> &'static str {
        match self {
            Stage::Handle => "handle",
            Stage::Symbol => "symbol",
            Stage::Bind => "bind",
            Stage::Ready => "ready",
        }
    }
}

/// 解析注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 已绑定。
    Bound,
    /// 弱未定义，返回空值但不置错。
    WeakNull,
    /// 强未定义，返回空值并置错。
    Undefined,
    /// 符号缺失。
    Missing,
    /// 版本未命中。
    VersionMiss,
    /// 句柄非法。
    BadHandle,
}

impl Note {
    /// 文本名。
    pub const fn name(self) -> &'static str {
        match self {
            Note::Bound => "bound",
            Note::WeakNull => "weak-null",
            Note::Undefined => "undefined",
            Note::Missing => "missing",
            Note::VersionMiss => "version-miss",
            Note::BadHandle => "bad-handle",
        }
    }
}

/// 一次解析结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lookup {
    /// 终止阶段。
    pub stage: Stage,
    /// 错误码，成功为 0。
    pub code: u32,
    /// 地址值，失败为 0。
    pub value: u32,
    /// 基名。
    pub sym: &'static str,
    /// 是否弱符号。
    pub weak: bool,
    /// 回显句柄（非法句柄回显 0）。
    pub handle: u32,
    /// 注解。
    pub note: Note,
}

impl Lookup {
    /// 是否成功调用（错误码为零）。
    pub const fn ok(&self) -> bool {
        self.code == 0
    }
}

/// 解析一个符号。
pub fn dlsym(scope: &Scope, sys: &mut Sys, h: u32, req: &'static str) -> Lookup {
    if !scope.valid_handle(h) {
        sys.set(E_DSHANDLE, req);
        return Lookup {
            stage: Stage::Handle,
            code: E_DSHANDLE,
            value: 0,
            sym: split_ver(req).0,
            weak: false,
            handle: 0,
            note: Note::BadHandle,
        };
    }
    let (base, ver) = split_ver(req);
    let found = match scope.resolve(h, base) {
        Some(v) => v,
        None => {
            sys.set(E_DSSYM, req);
            return Lookup {
                stage: Stage::Symbol,
                code: E_DSSYM,
                value: 0,
                sym: base,
                weak: false,
                handle: h,
                note: Note::Missing,
            };
        }
    };
    let sym = match scope.sym_at(found.0, found.1) {
        Some(s) => s,
        None => {
            sys.set(E_DSSYM, req);
            return Lookup {
                stage: Stage::Symbol,
                code: E_DSSYM,
                value: 0,
                sym: base,
                weak: false,
                handle: h,
                note: Note::Missing,
            };
        }
    };
    if !ver.is_empty() && sym.ver != ver {
        sys.set(E_DSVER, req);
        return Lookup {
            stage: Stage::Bind,
            code: E_DSVER,
            value: 0,
            sym: base,
            weak: sym.weak,
            handle: h,
            note: Note::VersionMiss,
        };
    }
    if !sym.defined {
        if sym.weak {
            sys.clear();
            return Lookup {
                stage: Stage::Bind,
                code: 0,
                value: 0,
                sym: base,
                weak: true,
                handle: h,
                note: Note::WeakNull,
            };
        }
        sys.set(E_DSNULL, req);
        return Lookup {
            stage: Stage::Symbol,
            code: E_DSNULL,
            value: 0,
            sym: base,
            weak: false,
            handle: h,
            note: Note::Undefined,
        };
    }
    sys.clear();
    Lookup {
        stage: Stage::Ready,
        code: 0,
        value: sym.value,
        sym: base,
        weak: sym.weak,
        handle: h,
        note: Note::Bound,
    }
}

// ---------------------------------------------------------------------------
// 错误文本
// ---------------------------------------------------------------------------

/// 错误码 → 文本。
pub fn error_for(code: u32, req: &str) -> String {
    match code {
        E_DSHANDLE => String::from("invalid handle\n"),
        E_DSSYM => {
            let mut s = String::from(req);
            s.push_str(": symbol not found\n");
            s
        }
        E_DSNULL => {
            let mut s = String::from(req);
            s.push_str(": undefined symbol: ");
            s.push_str(split_ver(req).0);
            s.push('\n');
            s
        }
        E_DSVER => {
            let mut s = String::from(req);
            s.push_str(": version `");
            s.push_str(split_ver(req).1);
            s.push_str("` not found\n");
            s
        }
        _ => String::new(),
    }
}

/// 结局 → 错误文本。
pub fn message(o: &Lookup, req: &str) -> String {
    if o.ok() {
        return String::new();
    }
    error_for(o.code, req)
}

/// 结局 → 释义。
pub fn explain(o: &Lookup) -> String {
    let mut s = String::from("stage=");
    s.push_str(o.stage.name());
    s.push_str(" code=");
    s.push_str(hex_text(o.code).as_str());
    s.push_str(" value=");
    s.push_str(u32_text(o.value).as_str());
    s.push_str(" weak=");
    s.push_str(if o.weak { "yes" } else { "no" });
    s.push_str(" handle=");
    s.push_str(u32_text(o.handle).as_str());
    s.push_str(" note=");
    s.push_str(o.note.name());
    s.push('\n');
    s
}

/// 槽位释义。
pub fn detail(c: &Case) -> String {
    let a = c.actual;
    let mut s = String::from(c.shape.req);
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" code=");
    s.push_str(hex_text(a.code).as_str());
    s.push_str(" value=");
    s.push_str(u32_text(a.value).as_str());
    s.push_str(" weak=");
    s.push_str(if a.weak { "yes" } else { "no" });
    s.push_str(" note=");
    s.push_str(a.note.name());
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 四档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 成功绑定（含弱空解）。
    Bound,
    /// 版本未命中。
    Version,
    /// 符号缺失 / 强未定义。
    Miss,
    /// 句柄等其他。
    Other,
}

impl Tier {
    /// 由注解归档。
    pub const fn of(n: Note) -> Tier {
        match n {
            Note::Bound | Note::WeakNull => Tier::Bound,
            Note::VersionMiss => Tier::Version,
            Note::Missing | Note::Undefined => Tier::Miss,
            Note::BadHandle => Tier::Other,
        }
    }

    /// 档号。
    pub const fn index(self) -> usize {
        match self {
            Tier::Bound => 0,
            Tier::Version => 1,
            Tier::Miss => 2,
            Tier::Other => 3,
        }
    }

    /// 档名。
    pub const fn name(self) -> &'static str {
        match self {
            Tier::Bound => "bound",
            Tier::Version => "version",
            Tier::Miss => "miss",
            Tier::Other => "other",
        }
    }
}

// ---------------------------------------------------------------------------
// 申报
// ---------------------------------------------------------------------------

/// 判据类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 全对。
    Ok,
    /// 成败申报错。
    Estate,
    /// 阶段申报错。
    Stage,
    /// 错误码申报错。
    Code,
    /// 地址值申报错。
    Value,
    /// 弱符号位申报错。
    Weak,
    /// 句柄申报错。
    Handle,
    /// 注解申报错。
    Note,
}

impl Fault {
    /// 类别码。
    pub const fn code(self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Estate => E_DSREQ,
            Fault::Stage => E_DSSTAGE,
            Fault::Code => E_DSSYM,
            Fault::Value => E_DSVALUE,
            Fault::Weak => E_DSWEAK,
            Fault::Handle => E_DSHANDLE,
            Fault::Note => E_DSNAME,
        }
    }
}

/// 短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Estate => "estate",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Value => "value",
        Fault::Weak => "weak",
        Fault::Handle => "handle",
        Fault::Note => "note",
    }
}

/// 一条申报。
#[derive(Clone, Copy, Debug)]
pub struct Claim {
    /// 申报为成功调用。
    pub success: bool,
    /// 申报阶段。
    pub stage: Stage,
    /// 申报错误码。
    pub code: u32,
    /// 申报地址值。
    pub value: u32,
    /// 申报弱符号位。
    pub weak: bool,
    /// 申报回显句柄。
    pub handle: u32,
    /// 申报注解。
    pub note: Note,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim {
            success: false,
            stage: Stage::Handle,
            code: 0,
            value: 0,
            weak: false,
            handle: 0,
            note: Note::Missing,
        }
    }
}

/// 一个语料形状。
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// 形状名。
    pub tag: &'static str,
    /// 请求句柄。
    pub handle: u32,
    /// 请求串。
    pub req: &'static str,
    /// 作用域构造。
    pub setup: fn() -> Scope,
}

/// 一槽语料。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// 形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
    /// 观测结局。
    pub actual: Lookup,
}

impl Case {
    /// 生成一槽。
    pub fn of(shape: Shape, claim: Claim) -> Case {
        let sc = (shape.setup)();
        let mut sys = Sys::new();
        let actual = dlsym(&sc, &mut sys, shape.handle, shape.req);
        Case { shape, claim, actual }
    }

    /// 观测结局。
    pub const fn actual(&self) -> Lookup {
        self.actual
    }

    /// 形状名。
    pub const fn tag(&self) -> &'static str {
        self.shape.tag
    }

    /// 申报对判。
    pub fn judge(&self) -> Fault {
        judge(self)
    }
}

/// 基准作用域构造器。
pub const SETUPS: [fn() -> Scope; 2] = [scope_two, scope_one];

/// 14 个形状。
pub const SHAPES: [Shape; CORPUS] = [
    Shape { tag: "bound-value", handle: 1, req: "square", setup: scope_two },
    Shape { tag: "weak-null", handle: 1, req: "weak_missing", setup: scope_two },
    Shape { tag: "shared-order", handle: 1, req: "shared", setup: scope_two },
    Shape { tag: "default-scope", handle: 0, req: "shared", setup: scope_two },
    Shape { tag: "version-ok", handle: 1, req: "square@V1", setup: scope_two },
    Shape { tag: "version-miss", handle: 1, req: "square@V9", setup: scope_two },
    Shape { tag: "absent", handle: 1, req: "missing_fn", setup: scope_two },
    Shape { tag: "bad-handle", handle: 9, req: "square", setup: scope_two },
    Shape { tag: "absent-as-bound", handle: 1, req: "missing_fn", setup: scope_two },
    Shape { tag: "weak-as-ready", handle: 1, req: "weak_missing", setup: scope_two },
    Shape { tag: "value-shift", handle: 1, req: "square", setup: scope_one },
    Shape { tag: "undef-as-missing", handle: 2, req: "dangling", setup: scope_two },
    Shape { tag: "weak-as-strong", handle: 1, req: "weak_missing", setup: scope_two },
    Shape { tag: "bad-handle-echo", handle: 9, req: "square", setup: scope_two },
];

/// 14 条申报。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim { success: true, stage: Stage::Ready, code: 0, value: 0x1000, weak: false, handle: 1, note: Note::Bound },
    Claim { success: true, stage: Stage::Bind, code: 0, value: 0, weak: true, handle: 1, note: Note::WeakNull },
    Claim { success: true, stage: Stage::Ready, code: 0, value: 0x2000, weak: false, handle: 1, note: Note::Bound },
    Claim { success: true, stage: Stage::Ready, code: 0, value: 0x2000, weak: false, handle: 0, note: Note::Bound },
    Claim { success: true, stage: Stage::Ready, code: 0, value: 0x1000, weak: false, handle: 1, note: Note::Bound },
    Claim { success: false, stage: Stage::Bind, code: E_DSVER, value: 0, weak: false, handle: 1, note: Note::VersionMiss },
    Claim { success: false, stage: Stage::Symbol, code: E_DSSYM, value: 0, weak: false, handle: 1, note: Note::Missing },
    Claim { success: false, stage: Stage::Handle, code: E_DSHANDLE, value: 0, weak: false, handle: 0, note: Note::BadHandle },
    Claim { success: true, stage: Stage::Ready, code: 0, value: 1, weak: false, handle: 1, note: Note::Bound },
    Claim { success: true, stage: Stage::Ready, code: 0, value: 0, weak: true, handle: 1, note: Note::Bound },
    Claim { success: true, stage: Stage::Ready, code: 0, value: 0x1001, weak: false, handle: 1, note: Note::Bound },
    Claim { success: false, stage: Stage::Symbol, code: E_DSSYM, value: 0, weak: false, handle: 2, note: Note::Missing },
    Claim { success: true, stage: Stage::Bind, code: 0, value: 0, weak: false, handle: 1, note: Note::Bound },
    Claim { success: false, stage: Stage::Handle, code: E_DSHANDLE, value: 0, weak: false, handle: 9, note: Note::BadHandle },
];

/// 六个错误槽的期望类别。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Estate, Fault::Stage, Fault::Value, Fault::Code, Fault::Weak, Fault::Handle];

/// 六个错误槽的期望码。
pub const REJECT_CODES: [u32; REJECT] =
    [E_DSREQ, E_DSSTAGE, E_DSVALUE, E_DSSYM, E_DSWEAK, E_DSHANDLE];

/// 生成语料。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::of(SHAPES[0], CLAIMS[0]); CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let (Some(s), Some(c)) = (SHAPES.get(i), CLAIMS.get(i)) {
            if let Some(slot) = out.get_mut(i) {
                *slot = Case::of(*s, *c);
            }
        }
        i += 1;
    }
    out
}

/// 取一槽。
pub fn case_at(i: usize) -> Option<Case> {
    let cs = cases();
    cs.get(i).copied()
}

/// 覆盖面核对。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else if n > CORPUS {
        Err(E_DSCOVER)
    } else {
        Err(E_DSREQ)
    }
}

/// 第 i 槽的期望类别。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        Some(Fault::Ok)
    } else {
        REJECT_FAULTS.get(i - ACCEPT).copied()
    }
}

/// 逐项对判。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual;
    let k = c.claim;
    if k.success != a.ok() {
        return Fault::Estate;
    }
    if k.stage != a.stage {
        return Fault::Stage;
    }
    if k.code != a.code {
        return Fault::Code;
    }
    if k.value != a.value {
        return Fault::Value;
    }
    if k.weak != a.weak {
        return Fault::Weak;
    }
    if k.handle != a.handle {
        return Fault::Handle;
    }
    if k.note != a.note {
        return Fault::Note;
    }
    Fault::Ok
}

// ---------------------------------------------------------------------------
// 矩阵
// ---------------------------------------------------------------------------

/// 矩阵一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 该档槽数。
    pub total: usize,
    /// 该档申报正确数。
    pub good: usize,
}

impl Row {
    /// 空白行。
    pub const fn blank() -> Row {
        Row { total: 0, good: 0 }
    }

    /// 全对。
    pub fn perfect(&self) -> bool {
        self.total > 0 && self.total == self.good
    }
}

/// 四档矩阵。
#[derive(Clone, Copy, Debug)]
pub struct Matrix {
    /// 行。
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
                let t = Tier::of(c.actual().note).index();
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
    /// 绑定档正确数。
    pub bound: u32,
    /// 版本档正确数。
    pub version: u32,
    /// 缺失档正确数。
    pub miss: u32,
    /// 其他档正确数。
    pub other: u32,
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
            Err(E_DSREQ)
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("dlsym corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" bound=");
        s.push_str(u32_text(self.bound).as_str());
        s.push_str(" version=");
        s.push_str(u32_text(self.version).as_str());
        s.push_str(" miss=");
        s.push_str(u32_text(self.miss).as_str());
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

/// 可注入一槽缺陷的全跑。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(E_DSREQ);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut bound = 0u32;
    let mut version = 0u32;
    let mut miss = 0u32;
    let mut other = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(E_DSREQ),
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
            None => return Err(E_DSREQ),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(a.note) {
                Tier::Bound => bound += 1,
                Tier::Version => version += 1,
                Tier::Miss => miss += 1,
                Tier::Other => other += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, bound, version, miss, other, matched })
}

// ---------------------------------------------------------------------------
// 闸门
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
            return Err(E_DSSTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(E_DSORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(E_DSSTATE);
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
            return Err(E_DSREQ);
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
            return Err(E_DSSTATE);
        }
        const VALS: [u32; ACCEPT] = [14, 8, 6, 5, 1, 1, 1, 929];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(E_DSSTATE),
            };
            if s.slot as usize != i {
                return Err(E_DSREQ);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(E_DSREQ);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(E_DSREQ);
                        }
                    }
                    None => return Err(E_DSSTATE),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(E_DSREQ);
                        }
                    }
                    None => return Err(E_DSSTATE),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(E_DSSTATE);
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
            return Err(E_DSSTATE);
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
            Err(E_DSSTATE)
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
    let _ = l.register(count_cell(3, OP_BIND, r.map(|x| x.bound)));
    let _ = l.register(count_cell(4, OP_CLEAR, r.map(|x| x.version)));
    let _ = l.register(count_cell(5, OP_SYMBOL, r.map(|x| x.miss)));
    let _ = l.register(count_cell(6, OP_HANDLE, r.map(|x| x.other)));

    let red = run_with(Some((0, Fault::Value)));
    let _ = l.register(count_cell(7, OP_WEAK, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_HANDLE,
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

/// 四路注入：符号、绑定、判据、账本。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && symtab_ok() && scope_ok() && split_ok();
    out[1] = bind_ok() && weak_ok() && miss_ok() && version_ok();
    out[2] = judge_ok() && matrix_ok() && corpus_ok() && reds_ok();
    out[3] = sys_ok()
        && message_ok()
        && detail_ok()
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

/// 第 i 槽的期望阶段。
const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Ready,
    Stage::Bind,
    Stage::Ready,
    Stage::Ready,
    Stage::Ready,
    Stage::Bind,
    Stage::Symbol,
    Stage::Handle,
    Stage::Symbol,
    Stage::Bind,
    Stage::Ready,
    Stage::Symbol,
    Stage::Bind,
    Stage::Handle,
];

/// 第 i 槽的期望注解。
const WANT_NOTE: [Note; CORPUS] = [
    Note::Bound,
    Note::WeakNull,
    Note::Bound,
    Note::Bound,
    Note::Bound,
    Note::VersionMiss,
    Note::Missing,
    Note::BadHandle,
    Note::Missing,
    Note::WeakNull,
    Note::Bound,
    Note::Undefined,
    Note::WeakNull,
    Note::BadHandle,
];

/// 第 i 槽的期望地址值。
const WANT_VALUE: [u32; CORPUS] = [0x1000, 0, 0x2000, 0x2000, 0x1000, 0, 0, 0, 0, 0, 0x1000, 0, 0, 0];

/// 名称与段基址。
fn name_ok() -> bool {
    let a = SEG_BASE == 0xfcf0;
    let b = E_DSREQ == SEG_BASE + 0x01
        && E_DSSTAGE == SEG_BASE + 0x02
        && E_DSHANDLE == SEG_BASE + 0x03
        && E_DSSYM == SEG_BASE + 0x04
        && E_DSCLEAR == SEG_BASE + 0x05
        && E_DSNULL == SEG_BASE + 0x06
        && E_DSVALUE == SEG_BASE + 0x07
        && E_DSSTATE == SEG_BASE + 0x08
        && E_DSORDER == SEG_BASE + 0x09
        && E_DSCOVER == SEG_BASE + 0x0a
        && E_DSNAME == SEG_BASE + 0x0b
        && E_DSWEAK == SEG_BASE + 0x0c
        && E_DSVER == SEG_BASE + 0x0d;
    let c = CORPUS == 14 && SAMPLES == 14 && ACCEPT == 8 && REJECT == 6;
    let d = TIER_COUNT == 4 && CAP == 4 && SYM_CAP == 8;
    let e = Stage::Handle.order() == 0
        && Stage::Symbol.order() == 1
        && Stage::Bind.order() == 2
        && Stage::Ready.order() == 3;
    let f = Tier::of(Note::Bound).index() == 0
        && Tier::of(Note::WeakNull).index() == 0
        && Tier::of(Note::VersionMiss).index() == 1
        && Tier::of(Note::Missing).index() == 2
        && Tier::of(Note::Undefined).index() == 2
        && Tier::of(Note::BadHandle).index() == 3;
    a && b && c && d && e && f
}

/// 文本与指纹。
fn text_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(4096) == "4096";
    let b = i32_text(-21) == "-21" && i32_text(21) == "21" && i32_text(0) == "0";
    let c = hex_text(0) == "0" && hex_text(0xfcfd) == "0xfcfd" && hex_text(0x1000) == "0x1000";
    let d = fold_str(FNV_OFF, "square") == fold_str(FNV_OFF, "square");
    let e = fold_str(FNV_OFF, "square") != fold_str(FNV_OFF, "shared");
    let f = fold_word(FNV_OFF, 1) != fold_word(FNV_OFF, 2);
    a && b && c && d && e && f
}

/// 名字拆分。
fn split_ok() -> bool {
    let a = split_ver("square@V9") == ("square", "V9");
    let b = split_ver("plain") == ("plain", "");
    let c = split_ver("foo@@V2") == ("foo", "V2");
    let d = split_ver("@V1") == ("", "V1");
    a && b && c && d
}

/// 符号表形状。
fn symtab_ok() -> bool {
    let t = lib_a().syms;
    let a = t.len() == 5 && !t.is_empty();
    let b = match t.find("square") {
        Some(i) => match t.get(i) {
            Some(s) => s.defined && s.value == 0x1000 && s.ver == "V1",
            None => false,
        },
        None => false,
    };
    let c = match t.find("weak_missing") {
        Some(i) => match t.get(i) {
            Some(s) => !s.defined && s.weak && s.value == 0,
            None => false,
        },
        None => false,
    };
    let d = match t.find("undef_strong") {
        Some(i) => match t.get(i) {
            Some(s) => !s.defined && !s.weak,
            None => false,
        },
        None => false,
    };
    let e = t.find("nope").is_none() && t.get(9).is_none();
    let g = t.seal() != 0;
    a && b && c && d && e && g
}

/// 作用域形状。
fn scope_ok() -> bool {
    let sc = scope_two();
    let a = sc.len() == 2 && !sc.is_empty();
    let b = sc.find_handle(1) == Some(0) && sc.find_handle(2) == Some(1);
    let c = sc.find_handle(0).is_none() && sc.find_handle(3).is_none() && !sc.valid_handle(3);
    let d = sc.valid_handle(0) && sc.valid_handle(1) && sc.valid_handle(2);
    let e = match sc.resolve(0, "shared") {
        Some((li, _)) => li == 0,
        None => false,
    };
    let f = match sc.resolve(2, "shared") {
        Some((li, _)) => li == 1,
        None => false,
    };
    let g = sc.resolve(2, "square").is_none() && sc.resolve(0, "square").is_some();
    let h = sc.order() == [0, 1, 0xff, 0xff] && sc.seal() != 0;
    a && b && c && d && e && f && g && h
}

/// 正常绑定。
fn bind_ok() -> bool {
    let sc = scope_two();
    let mut sys = Sys::new();
    let a = dlsym(&sc, &mut sys, 1, "square");
    let ok1 = a.stage == Stage::Ready
        && a.ok()
        && a.value == 0x1000
        && a.handle == 1
        && !a.weak
        && a.note == Note::Bound
        && !sys.is_pending();
    let b = dlsym(&sc, &mut sys, 1, "shared");
    let ok2 = b.value == 0x2000 && b.ok();
    let c = dlsym(&sc, &mut sys, 0, "shared");
    let ok3 = c.value == 0x2000 && c.handle == 0 && c.ok();
    let d = dlsym(&sc, &mut sys, 2, "shared");
    let ok4 = d.value == 0x9000 && d.handle == 2 && d.ok();
    let e = dlsym(&sc, &mut sys, 2, "counter");
    let ok5 = e.value == 0x3100 && dlsym(&sc, &mut sys, 1, "counter").value == 0x3000;
    ok1 && ok2 && ok3 && ok4 && ok5
}

/// 弱符号空解。
fn weak_ok() -> bool {
    let sc = scope_two();
    let mut sys = Sys::new();
    let a = dlsym(&sc, &mut sys, 1, "weak_missing");
    let ok1 = a.stage == Stage::Bind
        && a.ok()
        && a.value == 0
        && a.weak
        && a.note == Note::WeakNull
        && !sys.is_pending();
    let b = dlsym(&sc, &mut sys, 2, "weak_b");
    let ok2 = b.ok() && b.weak && b.note == Note::WeakNull && b.value == 0;
    let c = dlsym(&sc, &mut sys, 0, "weak_missing");
    let ok3 = c.ok() && c.weak && c.value == 0;
    ok1 && ok2 && ok3
}

/// 缺失与强未定义。
fn miss_ok() -> bool {
    let sc = scope_two();
    let mut sys = Sys::new();
    let a = dlsym(&sc, &mut sys, 1, "missing_fn");
    let ok1 = a.stage == Stage::Symbol
        && !a.ok()
        && a.code == E_DSSYM
        && a.value == 0
        && a.note == Note::Missing
        && sys.is_pending();
    let t1 = sys.take_text();
    let ok2 = t1.as_ref().map(|s| s.as_str()) == Some("missing_fn: symbol not found\n");
    let ok3 = sys.take_text().is_none();
    let b = dlsym(&sc, &mut sys, 2, "dangling");
    let ok4 = b.stage == Stage::Symbol
        && b.code == E_DSNULL
        && b.note == Note::Undefined
        && !b.weak
        && sys.is_pending();
    let ok5 = sys.peek_text().as_ref().map(|s| s.as_str())
        == Some("dangling: undefined symbol: dangling\n");
    let c = dlsym(&sc, &mut sys, 1, "undef_strong");
    let ok6 = c.code == E_DSNULL && c.note == Note::Undefined;
    ok1 && ok2 && ok3 && ok4 && ok5 && ok6
}

/// 版本查询。
fn version_ok() -> bool {
    let sc = scope_two();
    let mut sys = Sys::new();
    let a = dlsym(&sc, &mut sys, 1, "square@V1");
    let ok1 = a.ok() && a.value == 0x1000 && a.note == Note::Bound && !sys.is_pending();
    let b = dlsym(&sc, &mut sys, 1, "square@V9");
    let ok2 = !b.ok() && b.code == E_DSVER && b.note == Note::VersionMiss && sys.is_pending();
    let ok3 = sys.peek_text().as_ref().map(|s| s.as_str())
        == Some("square@V9: version `V9` not found\n");
    let c = dlsym(&sc, &mut sys, 0, "counter@V2");
    let ok4 = c.ok() && c.value == 0x3000;
    let d = dlsym(&sc, &mut sys, 0, "counter@V1");
    let ok5 = d.code == E_DSVER && d.note == Note::VersionMiss;
    let e = dlsym(&sc, &mut sys, 0, "square");
    let ok6 = e.ok() && e.value == 0x1000 && !sys.is_pending();
    ok1 && ok2 && ok3 && ok4 && ok5 && ok6
}

/// 错误状态。
fn sys_ok() -> bool {
    let mut s = Sys::new();
    let a = !s.is_pending() && s.peek_text().is_none() && s.take_text().is_none();
    s.set(E_DSSYM, "ghost");
    let b = s.is_pending();
    let c = s.peek_text().as_ref().map(|x| x.as_str()) == Some("ghost: symbol not found\n");
    let d = s.is_pending();
    let e = s.take_text().as_ref().map(|x| x.as_str()) == Some("ghost: symbol not found\n");
    let f = !s.is_pending() && s.take_text().is_none();
    s.set(E_DSHANDLE, "x");
    s.clear();
    let g = !s.is_pending() && s.peek_text().is_none();
    let h = s.seal() != 0;
    a && b && c && d && e && f && g && h
}

/// 申报对判。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let want = match expected(i) {
            Some(v) => v,
            None => return false,
        };
        if judge(&c) != want {
            return false;
        }
        i += 1;
    }
    let mut b = Case::of(SHAPES[0], CLAIMS[0]);
    let c = b.judge() == Fault::Ok;
    b.claim.handle = 2;
    let d = b.judge() == Fault::Handle;
    b.claim = CLAIMS[0];
    b.claim.note = Note::Missing;
    let e = b.judge() == Fault::Note;
    c && d && e
}

/// 矩阵统计。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    let a = m.total() == CORPUS && m.good() == ACCEPT && m.balanced();
    let b = m.row(0) == Some(Row { total: 8, good: 5 });
    let c = m.row(1) == Some(Row { total: 1, good: 1 });
    let d = m.row(2) == Some(Row { total: 3, good: 1 });
    let e = m.row(3) == Some(Row { total: 2, good: 1 });
    let f = m.perfect_rows() == 1;
    let g = m.row(9).is_none() && m.seal() != 0;
    a && b && c && d && e && f && g
}

/// 语料与渲染。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let want =
        "dlsym corpus=14 accept=8 reject=6 bound=5 version=1 miss=1 other=1 match=14 rate=1000ppm\n";
    r.total == 14
        && r.accept == 8
        && r.reject == 6
        && r.bound == 5
        && r.version == 1
        && r.miss == 1
        && r.other == 1
        && r.matched == 14
        && r.rate() == 1000
        && r.green()
        && r.gate() == Ok(())
        && r.render().as_str() == want
        && cover(CORPUS) == Ok(())
        && cover(13) == Err(E_DSREQ)
        && cover(15) == Err(E_DSCOVER)
}

/// 红档显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Value))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929 && !r.green() && r.gate() == Err(E_DSREQ);
    let b = r.accept == 7 && r.reject == 7;
    let c = run_with(Some((14, Fault::Code))).is_err();
    let d = run_with(Some((8, Fault::Note))).map(|x| x.matched) == Ok(13);
    a && b && c && d
}

/// 文案逐字。
fn message_ok() -> bool {
    let sc = scope_two();
    let mut sys = Sys::new();
    let a = dlsym(&sc, &mut sys, 1, "missing_fn");
    let ok1 = message(&a, "missing_fn").as_str() == "missing_fn: symbol not found\n";
    let b = dlsym(&sc, &mut sys, 2, "dangling");
    let ok2 = message(&b, "dangling").as_str() == "dangling: undefined symbol: dangling\n";
    let c = dlsym(&sc, &mut sys, 1, "square@V9");
    let ok3 = message(&c, "square@V9").as_str() == "square@V9: version `V9` not found\n";
    let d = dlsym(&sc, &mut sys, 9, "square");
    let ok4 = message(&d, "square").as_str() == "invalid handle\n";
    let e = dlsym(&sc, &mut sys, 1, "square");
    let ok5 = message(&e, "square").as_str() == "";
    ok1 && ok2 && ok3 && ok4 && ok5
}

/// 槽号与档位。
fn slots_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let a = c.actual();
        if a.stage != WANT_STAGE[i] || a.note != WANT_NOTE[i] || a.value != WANT_VALUE[i] {
            return false;
        }
        i += 1;
    }
    let tags = [
        "bound-value",
        "weak-null",
        "shared-order",
        "default-scope",
        "version-ok",
        "version-miss",
        "absent",
        "bad-handle",
        "absent-as-bound",
        "weak-as-ready",
        "value-shift",
        "undef-as-missing",
        "weak-as-strong",
        "bad-handle-echo",
    ];
    let mut k = 0usize;
    while k < tags.len() {
        let mut m = k + 1;
        while m < tags.len() {
            if tags[k] == tags[m] {
                return false;
            }
            m += 1;
        }
        if let Some(c) = cs.get(k) {
            if c.tag() != tags[k] {
                return false;
            }
        }
        k += 1;
    }
    let mut t = 0usize;
    while t < REJECT {
        if let Some(c) = cs.get(ACCEPT + t) {
            if judge(c) != expected(ACCEPT + t).unwrap_or(Fault::Note) {
                return false;
            }
        }
        t += 1;
    }
    case_at(0).is_some()
        && case_at(CORPUS).is_none()
        && case_at(9).map(|c| c.tag()) == Some("weak-as-ready")
}

/// 绑定释义。
const DETAIL_BOUND: &str = "square stage=ready code=0 value=4096 weak=no note=bound\n";

/// 弱空解释义。
const DETAIL_WEAK: &str = "weak_missing stage=bind code=0 value=0 weak=yes note=weak-null\n";

/// 版本未命中释义。
const DETAIL_VER: &str =
    "square@V9 stage=bind code=0xfcfd value=0 weak=no note=version-miss\n";

/// 句柄释义。
const DETAIL_HANDLE: &str = "square stage=handle code=0xfcf3 value=0 weak=no note=bad-handle\n";

/// 释义。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_BOUND,
        None => false,
    };
    let b = match cs.get(1) {
        Some(c) => detail(c).as_str() == DETAIL_WEAK,
        None => false,
    };
    let d = match cs.get(5) {
        Some(c) => detail(c).as_str() == DETAIL_VER,
        None => false,
    };
    let e = match cs.get(7) {
        Some(c) => detail(c).as_str() == DETAIL_HANDLE,
        None => false,
    };
    let f = match cs.get(3) {
        Some(c) => {
            let o = c.actual();
            o.handle == 0 && o.stage == Stage::Ready && o.value == 0x2000
        }
        None => false,
    };
    let g = explain(&case_actual(0))
        == "stage=ready code=0 value=4096 weak=no handle=1 note=bound\n";
    a && b && d && e && f && g
}

/// 取第 i 槽观测。
fn case_actual(i: usize) -> Lookup {
    match case_at(i) {
        Some(c) => c.actual(),
        None => Lookup {
            stage: Stage::Handle,
            code: E_DSREQ,
            value: 0,
            sym: "",
            weak: false,
            handle: 0,
            note: Note::Missing,
        },
    }
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Symbol) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Handle) == Err(E_DSORDER);
    let d = g.step(Stage::Ready) == Ok(()) && g.last() == 3;
    let e = g.close() == Ok(2) && g.closed();
    let f = g.close() == Err(E_DSSTATE);
    let h = Gate::new().step(Stage::Bind) == Ok(());
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
        && l.rejects() == 0
}

// ---------------------------------------------------------------------------
// 判据集
// ---------------------------------------------------------------------------

/// 13 条判据。
pub fn run_unx11638_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11638_dlsym_null");

    s.add("DS01-名称与段基址", name_ok(), "static");

    s.add("DS02-文本与指纹", text_ok(), "static");

    s.add("DS03-名字拆分", split_ok(), "static");

    s.add("DS04-符号表形状", symtab_ok(), "static");

    s.add("DS05-作用域与句柄", scope_ok(), "static");

    s.add("DS06-正常绑定", bind_ok(), "static");

    s.add("DS07-弱符号空解", weak_ok(), "static");

    s.add("DS08-缺符号与强未定义", miss_ok(), "static");

    s.add("DS09-版本查询", version_ok(), "static");

    s.add("DS10-错误状态机", sys_ok(), "static");

    s.add("DS11-申报对判", judge_ok(), "static");

    s.add("DS12-矩阵与语料", matrix_ok() && corpus_ok() && reds_ok(), "static");

    s.add(
        "DS13-冻结重放四注入",
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

    /// 测试用最小断言：失败即 panic。
    fn expect(cond: bool) {
        if !cond {
            panic!("unx11638 test fail")
        }
    }

    #[test]
    fn seg_and_codes() {
        expect(SEG_BASE == 0xfcf0);
        expect(E_DSREQ == SEG_BASE + 0x01);
        expect(E_DSVER == SEG_BASE + 0x0d);
        expect(CORPUS == 14 && SAMPLES == 14);
        expect(ACCEPT == 8 && REJECT == 6);
        expect(CAP == 4 && SYM_CAP == 8);
    }

    #[test]
    fn text_renders() {
        expect(u32_text(0) == "0");
        expect(u32_text(4096) == "4096");
        expect(i32_text(-1) == "-1");
        expect(hex_text(0xfcf0) == "0xfcf0");
        expect(hex_text(0) == "0");
    }

    #[test]
    fn fnv_stable() {
        expect(fold_str(FNV_OFF, "dlsym") == fold_str(FNV_OFF, "dlsym"));
        expect(fold_str(FNV_OFF, "dlsym") != fold_str(FNV_OFF, "dlopen"));
        expect(fold_word(FNV_OFF, 7) != fold_word(FNV_OFF, 8));
    }

    #[test]
    fn split_versions() {
        expect(split_ver("square@V9") == ("square", "V9"));
        expect(split_ver("square") == ("square", ""));
        expect(split_ver("a@@B") == ("a", "B"));
    }

    #[test]
    fn symtab_shape() {
        let t = lib_a().syms;
        expect(t.len() == 5);
        expect(t.find("square").is_some());
        expect(t.find("nope").is_none());
        expect(t.seal() != 0);
    }

    #[test]
    fn symtab_capacity() {
        let mut t = SymTab::new();
        let mut i = 0usize;
        while i < SYM_CAP {
            expect(t.add(Sym::defined("x", i as u32, "")).is_some());
            i += 1;
        }
        expect(t.add(Sym::defined("x", 9, "")).is_none());
        expect(t.len() == SYM_CAP);
    }

    #[test]
    fn scope_handles() {
        let sc = scope_two();
        expect(sc.len() == 2);
        expect(sc.find_handle(1) == Some(0));
        expect(sc.find_handle(2) == Some(1));
        expect(sc.find_handle(0).is_none());
        expect(sc.find_handle(9).is_none());
        expect(!sc.valid_handle(3));
        expect(sc.valid_handle(0));
    }

    #[test]
    fn scope_order_search() {
        let sc = scope_two();
        expect(match sc.resolve(0, "shared") {
            Some((li, _)) => li == 0,
            None => false,
        });
        expect(match sc.resolve(2, "shared") {
            Some((li, _)) => li == 1,
            None => false,
        });
        expect(sc.resolve(2, "square").is_none());
    }

    #[test]
    fn scope_capacity() {
        let mut sc = Scope::new();
        let mut i = 0usize;
        while i < CAP {
            expect(sc.add(lib_b()).is_some());
            i += 1;
        }
        expect(sc.add(lib_a()).is_none());
        expect(sc.len() == CAP);
        expect(sc.get(CAP).is_none());
    }

    #[test]
    fn bind_square() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let a = dlsym(&sc, &mut sys, 1, "square");
        expect(a.ok() && a.value == 0x1000 && a.note == Note::Bound);
        expect(!sys.is_pending());
    }

    #[test]
    fn bind_default_scope() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let a = dlsym(&sc, &mut sys, 0, "shared");
        expect(a.ok() && a.value == 0x2000 && a.handle == 0);
        let b = dlsym(&sc, &mut sys, 2, "shared");
        expect(b.value == 0x9000 && b.handle == 2);
    }

    #[test]
    fn weak_returns_null_without_error() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let a = dlsym(&sc, &mut sys, 1, "weak_missing");
        expect(a.ok() && a.value == 0 && a.weak);
        expect(a.note == Note::WeakNull);
        expect(!sys.is_pending());
    }

    #[test]
    fn undefined_returns_null_with_error() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let a = dlsym(&sc, &mut sys, 2, "dangling");
        expect(!a.ok() && a.value == 0 && !a.weak);
        expect(a.code == E_DSNULL && a.note == Note::Undefined);
        expect(sys.is_pending());
    }

    #[test]
    fn missing_symbol_error() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let a = dlsym(&sc, &mut sys, 1, "nope");
        expect(a.code == E_DSSYM && a.stage == Stage::Symbol);
        expect(sys.is_pending());
    }

    #[test]
    fn bad_handle_short_circuits() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let a = dlsym(&sc, &mut sys, 9, "square");
        expect(a.code == E_DSHANDLE && a.stage == Stage::Handle);
        expect(a.handle == 0 && a.note == Note::BadHandle);
        expect(sys.peek_text().as_ref().map(|s| s.as_str()) == Some("invalid handle\n"));
    }

    #[test]
    fn version_match_and_miss() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let a = dlsym(&sc, &mut sys, 1, "square@V1");
        expect(a.ok() && a.value == 0x1000);
        let b = dlsym(&sc, &mut sys, 1, "square@V9");
        expect(b.code == E_DSVER && b.note == Note::VersionMiss);
        expect(sys.is_pending());
    }

    #[test]
    fn version_default_binds() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let a = dlsym(&sc, &mut sys, 1, "square");
        expect(a.ok() && a.value == 0x1000);
        let b = dlsym(&sc, &mut sys, 1, "square@V2");
        expect(b.code == E_DSVER);
    }

    #[test]
    fn error_clears_on_success() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let _ = dlsym(&sc, &mut sys, 1, "missing_fn");
        expect(sys.is_pending());
        let _ = dlsym(&sc, &mut sys, 1, "square");
        expect(!sys.is_pending());
        expect(sys.take_text().is_none());
    }

    #[test]
    fn error_is_one_shot() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let _ = dlsym(&sc, &mut sys, 1, "missing_fn");
        expect(sys.take_text().is_some());
        expect(sys.take_text().is_none());
    }

    #[test]
    fn messages_are_exact() {
        let sc = scope_two();
        let mut sys = Sys::new();
        let a = dlsym(&sc, &mut sys, 1, "missing_fn");
        expect(message(&a, "missing_fn").as_str() == "missing_fn: symbol not found\n");
        let b = dlsym(&sc, &mut sys, 2, "dangling");
        expect(message(&b, "dangling").as_str() == "dangling: undefined symbol: dangling\n");
        let c = dlsym(&sc, &mut sys, 1, "square@V9");
        expect(message(&c, "square@V9").as_str() == "square@V9: version `V9` not found\n");
        let d = dlsym(&sc, &mut sys, 9, "square");
        expect(message(&d, "square").as_str() == "invalid handle\n");
    }

    #[test]
    fn explain_renders() {
        let o = case_actual(0);
        expect(explain(&o) == "stage=ready code=0 value=4096 weak=no handle=1 note=bound\n");
    }

    #[test]
    fn case_actual_oob() {
        let o = case_actual(CORPUS + 3);
        expect(o.code == E_DSREQ);
    }

    #[test]
    fn corpus_has_14_cases() {
        expect(cases().len() == 14);
        expect(case_at(0).is_some());
        expect(case_at(14).is_none());
    }

    #[test]
    fn corpus_judges_clean() {
        let cs = cases();
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(c) = cs.get(i) {
                expect(judge(c) == expected(i).unwrap_or(Fault::Note));
            }
            i += 1;
        }
    }

    #[test]
    fn reject_faults_match_codes() {
        let cs = cases();
        let mut i = 0usize;
        while i < REJECT {
            let want = match REJECT_FAULTS.get(i) {
                Some(f) => *f,
                None => Fault::Note,
            };
            if let Some(c) = cs.get(ACCEPT + i) {
                expect(judge(c) == want);
                expect(judge(c).code() == REJECT_CODES[i]);
            }
            i += 1;
        }
    }

    #[test]
    fn report_is_green() {
        let r = run().unwrap();
        expect(r.green());
        expect(r.rate() == 1000);
        expect(r.gate() == Ok(()));
    }

    #[test]
    fn report_render_exact() {
        let r = run().unwrap();
        expect(
            r.render().as_str()
                == "dlsym corpus=14 accept=8 reject=6 bound=5 version=1 miss=1 other=1 match=14 rate=1000ppm\n",
        );
    }

    #[test]
    fn report_red_injection() {
        let r = run_with(Some((0, Fault::Value))).unwrap();
        expect(r.matched == 13);
        expect(r.rate() == 929);
        expect(!r.green());
    }

    #[test]
    fn report_red_out_of_range() {
        expect(run_with(Some((CORPUS, Fault::Value))).is_err());
        expect(run_with(Some((99, Fault::Note))).is_err());
    }

    #[test]
    fn cover_bounds() {
        expect(cover(CORPUS) == Ok(()));
        expect(cover(13) == Err(E_DSREQ));
        expect(cover(15) == Err(E_DSCOVER));
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        expect(m.row(0) == Some(Row { total: 8, good: 5 }));
        expect(m.row(1) == Some(Row { total: 1, good: 1 }));
        expect(m.row(2) == Some(Row { total: 3, good: 1 }));
        expect(m.row(3) == Some(Row { total: 2, good: 1 }));
        expect(m.perfect_rows() == 1);
    }

    #[test]
    fn matrix_seal_stable() {
        expect(Matrix::of().seal() == Matrix::of().seal());
        expect(Matrix::of().seal() != 0);
    }

    #[test]
    fn detail_bound_is_exact() {
        expect(detail(&cases()[0]).as_str() == DETAIL_BOUND);
    }

    #[test]
    fn detail_weak_is_exact() {
        expect(detail(&cases()[1]).as_str() == DETAIL_WEAK);
    }

    #[test]
    fn detail_bad_handle_is_exact() {
        expect(detail(&cases()[7]).as_str() == DETAIL_HANDLE);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        expect(l.len() == SAMPLES);
        expect(l.ok_count() == ACCEPT);
        expect(l.err_count() == REJECT);
        expect(l.sample(0).is_some());
        expect(l.sample(SAMPLES).is_none());
        expect(!l.is_empty());
    }

    #[test]
    fn ledger_audits() {
        let l = build_ledger();
        expect(l.audit() == Ok(()));
        expect(l.rejects() == 0);
    }

    #[test]
    fn ledger_rejects_bad_slot() {
        let mut l = build_ledger();
        expect(l.register(Sample { slot: 0, op: 0, ok: true, code: 0, value: 0 }).is_err());
        expect(l.register(Sample { slot: 14, op: 0, ok: true, code: 0, value: 0 }).is_err());
        expect(l.rejects() == 2);
    }

    #[test]
    fn ledger_freeze_and_replay() {
        let mut l = build_ledger();
        let r = l.freeze().unwrap();
        expect(r != 0 && l.root() == r && l.is_frozen());
        expect(l.replay() == Ok(r));
        expect(l.freeze().is_err());
    }

    #[test]
    fn injections_all_true() {
        let inj = inject_four();
        expect(inj[0] && inj[1] && inj[2] && inj[3]);
    }

    #[test]
    fn gate_orders_stages() {
        let mut g = Gate::new();
        expect(g.step(Stage::Symbol) == Ok(()));
        expect(g.step(Stage::Handle) == Err(E_DSORDER));
        expect(g.step(Stage::Ready) == Ok(()));
        expect(g.close() == Ok(2));
        expect(g.close() == Err(E_DSSTATE));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx11638_checks();
        expect(s.len() == 13);
        expect(s.all_passed());
        expect(s.failed_count() == 0);
    }
}
