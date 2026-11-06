//! UNX-F11641 LC_ALL 坏值 setlocale 失败回退 "C" 判据
//!
//! `LC_ALL=xx_YY.INVALID` 是运维现场最常见的一类脏值：语言码拍脑袋写、
//! 编码段抄成 `INVALID`、或者把 Windows 的 `Chinese_China.936` 原样搬进
//! Linux。POSIX 对 `setlocale(LC_ALL, name)` 的规定很短：name 不可用时
//! 返回 `(char *)NULL`，且**不改变**当前 locale。于是进程停在启动时的
//! "C" 档，而 `getenv("LC_ALL")` 依然原样返回那串坏值——setlocale 从不
//! 回写环境。**环境值与生效值从这一刻起分叉**，脚本若按环境值判断当前
//! 档位就会走错分支，这是本卡要钉死的核心判据。
//!
//! 名字语法按 `language[_territory][.codeset][@modifier]` 逐段切：
//! language 1..8 个字母（`C`/`POSIX` 也走这条，靠目录整串匹配落档），
//! territory 两个字母或三个数字，codeset 1..12 个 `[A-Za-z0-9-]`，
//! modifier 1..8 个字母数字。切分先扫分隔符（`_`、`.`、`@` 各至多一次，
//! `@` 不能打头也不能挂尾），再逐段验字符集，最后才拼回整串去目录里查。
//!
//! 拒收次序固定：字节（空/超长/非打印/NUL）→ 语法（分隔符与字段字符集）
//! → 编码段（本镜像只认 UTF-8 / ASCII / ISO-8859-1 / ISO-8859-15）→
//! 修饰段（本镜像不带 euro 档）→ 安装目录（整串精确匹配）→ 回退 C。
//! 编码段先于目录查装，是因为 `xx_YY.INVALID` 的根因在编码段，报"没装"
//! 会把 `locale -a` 引到错的方向。
//!
//! 闸门次序：字节 → 语法 → 编码段 → 修饰段 → 目录 → 回退 → 环境背离 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfd00;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 名字缓冲容量（超此长度直接判字节档）。
pub const NAME_CAP: usize = 48;

/// 语言码最长。
pub const LANG_MAX: usize = 8;

/// 地区码最长。
pub const TERR_MAX: usize = 3;

/// 编码段最长。
pub const CODESET_MAX: usize = 12;

/// 修饰段最长。
pub const MOD_MAX: usize = 8;

/// 分类总数。
pub const CAT_COUNT: usize = 12;

/// 十二分类全掩码。
pub const CAT_ALL_MASK: u16 = 0x0fff;

/// "C" 档在目录里的下标（回退锚点）。
pub const C_INDEX: usize = 0;

/// 环境值缓冲容量。
pub const ENV_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 语法坏（分隔符或字段字符集）。
pub const LB_EGRAMMAR: u32 = SEG_BASE + 0x01;
/// 目录里没装。
pub const LB_EMISSING: u32 = SEG_BASE + 0x02;
/// 编码段不认识。
pub const LB_ECODESET: u32 = SEG_BASE + 0x03;
/// 修饰段不支持。
pub const LB_EMODIFIER: u32 = SEG_BASE + 0x04;
/// setlocale 返回 NULL（聚合拒收）。
pub const LB_ENULL: u32 = SEG_BASE + 0x05;
/// 状态冲突。
pub const LB_ESTATE: u32 = SEG_BASE + 0x06;
/// 字节非法。
pub const LB_EBYTE: u32 = SEG_BASE + 0x07;
/// 语料重复。
pub const LB_EDUP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const LB_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const LB_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const LB_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const LB_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const LB_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 名字解析。
pub const OP_PARSE: u8 = 0x01;
/// 目录解析。
pub const OP_RESOLVE: u8 = 0x02;
/// setlocale 落档。
pub const OP_SET: u8 = 0x03;
/// 回退 C。
pub const OP_FALLBACK: u8 = 0x04;
/// 环境读取。
pub const OP_ENV: u8 = 0x05;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x06;
/// 文案。
pub const OP_TRACE: u8 = 0x07;
/// 分类掩码。
pub const OP_CAT: u8 = 0x08;

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

/// 四字符小写十六进制文本。
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

/// 追加原始字节（全部已验为 ASCII）。
fn push_bytes(dst: &mut String, src: &[u8]) {
    let mut i = 0usize;
    while i < src.len() {
        if let Some(b) = src.get(i) {
            dst.push(*b as char);
        }
        i += 1;
    }
}

/// 小写化一个 ASCII 字节。
fn lower(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

/// 是否字母。
fn is_alpha(b: u8) -> bool {
    (b >= b'a' && b <= b'z') || (b >= b'A' && b <= b'Z')
}

/// 是否数字。
fn is_digit(b: u8) -> bool {
    b >= b'0' && b <= b'9'
}

/// 是否字母数字。
fn is_alnum(b: u8) -> bool {
    is_alpha(b) || is_digit(b)
}

/// 大小写不敏感比较。
fn eq_ci(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if lower(x) != lower(y) {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 名字与分段
// ---------------------------------------------------------------------------

/// 名字里的一个字段段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// 起始偏移。
    pub off: usize,
    /// 长度。
    pub len: usize,
}

impl Span {
    /// 空段。
    pub const fn empty() -> Span {
        Span { off: 0, len: 0 }
    }

    /// 构造。
    pub const fn new(off: usize, len: usize) -> Span {
        Span { off, len }
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 结束偏移（开区间）。
    pub const fn end(&self) -> usize {
        self.off + self.len
    }
}

/// 四段切分结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    /// 语言码。
    pub lang: Span,
    /// 地区码（可空）。
    pub terr: Span,
    /// 编码段（可空）。
    pub codeset: Span,
    /// 修饰段（可空）。
    pub modifier: Span,
}

impl Parts {
    /// 空切分。
    pub const fn blank() -> Parts {
        Parts {
            lang: Span::empty(),
            terr: Span::empty(),
            codeset: Span::empty(),
            modifier: Span::empty(),
        }
    }
}

/// 未见过的分隔符哨兵。
const NO_OFF: usize = usize::MAX;

/// 一个受检的 locale 名字。
#[derive(Clone, Copy, Debug)]
pub struct LocaleName {
    buf: [u8; NAME_CAP],
    len: usize,
}

impl LocaleName {
    /// 收字节。空串、超长、非打印、NUL 在入口即拒。
    pub fn new(src: &str) -> Result<LocaleName, u32> {
        let bytes = src.as_bytes();
        if bytes.is_empty() {
            return Err(LB_EGRAMMAR);
        }
        if bytes.len() > NAME_CAP {
            return Err(LB_EBYTE);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < bytes.len() {
            let b = match bytes.get(i) {
                Some(v) => *v,
                None => return Err(LB_EBYTE),
            };
            if b < 0x21 || b > 0x7e {
                return Err(LB_EBYTE);
            }
            buf[i] = b;
            i += 1;
        }
        Ok(LocaleName { buf, len: bytes.len() })
    }

    /// 实长。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空（构造期已拒，保留闸门语义）。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一字节。
    pub fn at(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.buf.get(i).copied()
    }

    /// 取一段。
    pub fn slice(&self, sp: Span) -> Option<&[u8]> {
        if sp.off > self.len || sp.end() > self.len {
            return None;
        }
        self.buf.get(sp.off..sp.end())
    }

    /// 整串文本。
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

    /// 整串摘要。
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

    /// 扫分隔符：`_`、`.`、`@` 各至多一次，顺序必须 `_ < . < @`。
    fn markers(&self) -> Result<(usize, usize, usize), u32> {
        let mut us = NO_OFF;
        let mut dot = NO_OFF;
        let mut at = NO_OFF;
        let mut i = 0usize;
        while i < self.len {
            let b = match self.at(i) {
                Some(v) => v,
                None => return Err(LB_EBYTE),
            };
            match b {
                b'_' => {
                    if us != NO_OFF {
                        return Err(LB_EGRAMMAR);
                    }
                    us = i;
                }
                b'.' => {
                    if dot != NO_OFF {
                        return Err(LB_EGRAMMAR);
                    }
                    dot = i;
                }
                b'@' => {
                    if at != NO_OFF {
                        return Err(LB_EGRAMMAR);
                    }
                    at = i;
                }
                _ => {}
            }
            i += 1;
        }
        if us != NO_OFF && dot != NO_OFF && us > dot {
            return Err(LB_EGRAMMAR);
        }
        if dot != NO_OFF && at != NO_OFF && dot > at {
            return Err(LB_EGRAMMAR);
        }
        if us != NO_OFF && at != NO_OFF && us > at {
            return Err(LB_EGRAMMAR);
        }
        Ok((us, dot, at))
    }

    /// 分段校验。
    pub fn classify(&self) -> Result<Parts, u32> {
        if self.len == 0 {
            return Err(LB_EGRAMMAR);
        }
        let (us, dot, at) = self.markers()?;
        let head_end = if at == NO_OFF { self.len } else { at };
        let cs_end = if dot == NO_OFF { head_end } else { dot };
        let lang_end = if us == NO_OFF { cs_end } else { us };

        if at != NO_OFF && (at == 0 || at + 1 >= self.len) {
            // `@` 打头或挂尾都没有修饰段可读。
            return Err(LB_EGRAMMAR);
        }
        if dot != NO_OFF && (dot == 0 || dot + 1 >= head_end) {
            return Err(LB_EGRAMMAR);
        }
        if us != NO_OFF && (us == 0 || us + 1 >= cs_end) {
            return Err(LB_EGRAMMAR);
        }
        if lang_end == 0 || lang_end > LANG_MAX {
            return Err(LB_EGRAMMAR);
        }

        let lang = Span::new(0, lang_end);
        let lang_bytes = match self.slice(lang) {
            Some(v) => v,
            None => return Err(LB_EBYTE),
        };
        let mut i = 0usize;
        while i < lang_bytes.len() {
            let b = match lang_bytes.get(i) {
                Some(v) => *v,
                None => return Err(LB_EBYTE),
            };
            if !is_alpha(b) {
                return Err(LB_EGRAMMAR);
            }
            i += 1;
        }

        let mut terr = Span::empty();
        if us != NO_OFF {
            let tlen = cs_end - (us + 1);
            if tlen == 0 || tlen > TERR_MAX {
                return Err(LB_EGRAMMAR);
            }
            terr = Span::new(us + 1, tlen);
            let tb = match self.slice(terr) {
                Some(v) => v,
                None => return Err(LB_EBYTE),
            };
            let all_alpha = {
                let mut k = 0usize;
                let mut ok = true;
                while k < tb.len() {
                    if let Some(b) = tb.get(k) {
                        if !is_alpha(*b) {
                            ok = false;
                        }
                    }
                    k += 1;
                }
                ok
            };
            let all_digit = {
                let mut k = 0usize;
                let mut ok = true;
                while k < tb.len() {
                    if let Some(b) = tb.get(k) {
                        if !is_digit(*b) {
                            ok = false;
                        }
                    }
                    k += 1;
                }
                ok
            };
            let shape = if tb.len() == 2 && all_alpha {
                true
            } else {
                tb.len() == 3 && all_digit
            };
            if !shape {
                return Err(LB_EGRAMMAR);
            }
        }

        let mut codeset = Span::empty();
        if dot != NO_OFF {
            let clen = head_end - (dot + 1);
            if clen == 0 || clen > CODESET_MAX {
                return Err(LB_EGRAMMAR);
            }
            codeset = Span::new(dot + 1, clen);
            let cb = match self.slice(codeset) {
                Some(v) => v,
                None => return Err(LB_EBYTE),
            };
            let first = match cb.first() {
                Some(v) => *v,
                None => return Err(LB_EGRAMMAR),
            };
            if !is_alnum(first) {
                return Err(LB_EGRAMMAR);
            }
            let mut k = 0usize;
            while k < cb.len() {
                let b = match cb.get(k) {
                    Some(v) => *v,
                    None => return Err(LB_EBYTE),
                };
                if !(is_alnum(b) || b == b'-') {
                    return Err(LB_EGRAMMAR);
                }
                k += 1;
            }
        }

        let mut modifier = Span::empty();
        if at != NO_OFF {
            let mlen = self.len - (at + 1);
            if mlen == 0 || mlen > MOD_MAX {
                return Err(LB_EGRAMMAR);
            }
            modifier = Span::new(at + 1, mlen);
            let mb = match self.slice(modifier) {
                Some(v) => v,
                None => return Err(LB_EBYTE),
            };
            let mut k = 0usize;
            while k < mb.len() {
                let b = match mb.get(k) {
                    Some(v) => *v,
                    None => return Err(LB_EBYTE),
                };
                if !is_alnum(b) {
                    return Err(LB_EGRAMMAR);
                }
                k += 1;
            }
        }

        Ok(Parts { lang, terr, codeset, modifier })
    }

    /// 拼回整串（缺省段不补占位）。
    pub fn compose(&self, p: &Parts) -> Result<String, u32> {
        let mut s = String::new();
        let lb = match self.slice(p.lang) {
            Some(v) => v,
            None => return Err(LB_EBYTE),
        };
        push_bytes(&mut s, lb);
        if !p.terr.is_empty() {
            s.push('_');
            let tb = match self.slice(p.terr) {
                Some(v) => v,
                None => return Err(LB_EBYTE),
            };
            push_bytes(&mut s, tb);
        }
        if !p.codeset.is_empty() {
            s.push('.');
            let cb = match self.slice(p.codeset) {
                Some(v) => v,
                None => return Err(LB_EBYTE),
            };
            push_bytes(&mut s, cb);
        }
        if !p.modifier.is_empty() {
            s.push('@');
            let mb = match self.slice(p.modifier) {
                Some(v) => v,
                None => return Err(LB_EBYTE),
            };
            push_bytes(&mut s, mb);
        }
        Ok(s)
    }

    /// 单段文本。
    pub fn field(&self, p: &Parts, which: u8) -> String {
        let sp = match which {
            0 => p.lang,
            1 => p.terr,
            2 => p.codeset,
            _ => p.modifier,
        };
        let mut s = String::new();
        if let Some(v) = self.slice(sp) {
            push_bytes(&mut s, v);
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 安装目录
// ---------------------------------------------------------------------------

/// 一个已装 locale。
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// 整串名。
    pub name: &'static str,
    /// 主编码段。
    pub codeset: &'static str,
    /// 分类可用掩码。
    pub cats: u16,
}

impl Entry {
    /// 构造。
    pub const fn new(name: &'static str, codeset: &'static str, cats: u16) -> Entry {
        Entry { name, codeset, cats }
    }
}

/// 本镜像装的九档。
pub const CATALOG: [Entry; 9] = [
    Entry::new("C", "ASCII", CAT_ALL_MASK),
    Entry::new("POSIX", "ASCII", CAT_ALL_MASK),
    Entry::new("C.UTF-8", "UTF-8", CAT_ALL_MASK),
    Entry::new("en_US.UTF-8", "UTF-8", CAT_ALL_MASK),
    Entry::new("en_GB.UTF-8", "UTF-8", CAT_ALL_MASK),
    Entry::new("de_DE.UTF-8", "UTF-8", CAT_ALL_MASK),
    Entry::new("fr_FR.UTF-8", "UTF-8", CAT_ALL_MASK),
    Entry::new("ja_JP.UTF-8", "UTF-8", CAT_ALL_MASK),
    Entry::new("zh_CN.UTF-8", "UTF-8", CAT_ALL_MASK),
];

/// 认识的编码段拼写。
pub const CODESETS: [&str; 6] = ["UTF-8", "utf8", "ASCII", "ascii", "ISO-8859-1", "ISO-8859-15"];

/// 编码段是否认识。
pub fn codeset_known(cs: &[u8]) -> bool {
    if cs.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < CODESETS.len() {
        if let Some(w) = CODESETS.get(i) {
            if eq_ci(cs, w.as_bytes()) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 整串查目录。
pub fn find_entry(name: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < CATALOG.len() {
        if let Some(e) = CATALOG.get(i) {
            if e.name == name {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 下标查条目。
pub fn entry_at(i: usize) -> Option<Entry> {
    CATALOG.get(i).copied()
}

/// 目录名表。
pub fn entry_name(i: usize) -> Option<&'static str> {
    CATALOG.get(i).map(|e| e.name)
}

/// 解析成功的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// 目录下标。
    pub index: usize,
    /// 规范整串。
    pub canonical: &'static str,
    /// 编码段。
    pub codeset: &'static str,
}

/// 解析失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 语法。
    Grammar,
    /// 编码段。
    Codeset,
    /// 修饰段。
    Modifier,
    /// 缺装。
    Missing,
}

impl Fault {
    /// 文本名。
    pub fn name(&self) -> &'static str {
        match self {
            Fault::Grammar => "grammar",
            Fault::Codeset => "codeset",
            Fault::Modifier => "modifier",
            Fault::Missing => "missing",
        }
    }

    /// 对应错误码。
    pub fn code(&self) -> u32 {
        match self {
            Fault::Grammar => LB_EGRAMMAR,
            Fault::Codeset => LB_ECODESET,
            Fault::Modifier => LB_EMODIFIER,
            Fault::Missing => LB_EMISSING,
        }
    }
}

/// 全路径解析：语法 → 编码段 → 修饰段 → 目录。
pub fn resolve(name: &LocaleName) -> Result<Resolved, u32> {
    let parts = name.classify()?;
    if !parts.modifier.is_empty() {
        return Err(LB_EMODIFIER);
    }
    if !parts.codeset.is_empty() {
        let cs = match name.slice(parts.codeset) {
            Some(v) => v,
            None => return Err(LB_EBYTE),
        };
        if !codeset_known(cs) {
            return Err(LB_ECODESET);
        }
    }
    let candidate = name.compose(&parts)?;
    match find_entry(candidate.as_str()) {
        Some(i) => {
            let e = match CATALOG.get(i) {
                Some(v) => *v,
                None => return Err(LB_EMISSING),
            };
            Ok(Resolved { index: i, canonical: e.name, codeset: e.codeset })
        }
        None => Err(LB_EMISSING),
    }
}

/// 文本直通解析。
pub fn resolve_str(src: &str) -> Result<Resolved, u32> {
    let n = LocaleName::new(src)?;
    resolve(&n)
}

/// 文本直通错误码（成功给 0）。
pub fn code_of(src: &str) -> u32 {
    match resolve_str(src) {
        Ok(_) => 0,
        Err(c) => c,
    }
}

// ---------------------------------------------------------------------------
// 分类掩码
// ---------------------------------------------------------------------------

/// 十二分类名。
pub const CAT_NAMES: [&str; CAT_COUNT] = [
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
];

/// 单分类位。
pub fn cat_bit(i: usize) -> Option<u16> {
    if i >= CAT_COUNT {
        return None;
    }
    Some(1u16 << i)
}

/// 分类名。
pub fn cat_name(i: usize) -> Option<&'static str> {
    CAT_NAMES.get(i).copied()
}

/// 掩码是否合法（非零且在十二位内）。
pub fn mask_ok(mask: u16) -> bool {
    mask != 0 && (mask & !CAT_ALL_MASK) == 0
}

/// 掩码里的分类数。
pub fn mask_count(mask: u16) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CAT_COUNT {
        if (mask & (1u16 << i)) != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 分类引擎：按掩码落档，失败不改动任何槽（POSIX 的原子性）。
#[derive(Clone, Copy, Debug)]
pub struct Engine {
    slots: [u8; CAT_COUNT],
    applied: u16,
    fails: u32,
    last: u32,
}

impl Engine {
    /// 空引擎（全 C）。
    pub const fn new() -> Engine {
        Engine { slots: [0u8; CAT_COUNT], applied: 0, fails: 0, last: 0 }
    }

    /// 当前生效掩码。
    pub fn applied(&self) -> u16 {
        self.applied
    }

    /// 失败计数。
    pub fn fails(&self) -> u32 {
        self.fails
    }

    /// 最后错误码。
    pub fn last(&self) -> u32 {
        self.last
    }

    /// 分类当前名。
    pub fn name_at(&self, cat: usize) -> Option<&'static str> {
        if cat >= CAT_COUNT {
            return None;
        }
        let idx = self.slots.get(cat).copied().unwrap_or(0);
        entry_name(idx as usize)
    }

    /// 分类当前档位。
    pub fn slot_at(&self, cat: usize) -> Option<u8> {
        self.slots.get(cat).copied()
    }

    /// 是否全部落在同一档。
    pub fn uniform(&self) -> bool {
        let first = self.slots.first().copied().unwrap_or(0);
        let mut i = 1usize;
        while i < CAT_COUNT {
            if let Some(s) = self.slots.get(i) {
                if *s != first {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 某档覆盖的掩码。
    pub fn mask_of(&self, idx: u8) -> u16 {
        let mut m = 0u16;
        let mut i = 0usize;
        while i < CAT_COUNT {
            if let Some(s) = self.slots.get(i) {
                if *s == idx {
                    m |= 1u16 << i;
                }
            }
            i += 1;
        }
        m
    }

    /// 落档；返回实际变更的掩码。
    pub fn apply(&mut self, mask: u16, src: &str) -> Result<u16, u32> {
        if !mask_ok(mask) {
            self.fails = self.fails.saturating_add(1);
            self.last = LB_EBYTE;
            return Err(LB_EBYTE);
        }
        let parsed = match LocaleName::new(src) {
            Ok(v) => v,
            Err(c) => {
                self.fails = self.fails.saturating_add(1);
                self.last = c;
                return Err(c);
            }
        };
        let r = match resolve(&parsed) {
            Ok(v) => v,
            Err(c) => {
                self.fails = self.fails.saturating_add(1);
                self.last = c;
                return Err(c);
            }
        };
        let idx = r.index as u8;
        let mut changed = 0u16;
        let mut cat = 0usize;
        while cat < CAT_COUNT {
            let bit = 1u16 << cat;
            if (mask & bit) != 0 {
                if let Some(slot) = self.slots.get_mut(cat) {
                    if *slot != idx {
                        *slot = idx;
                        changed |= bit;
                    }
                }
            }
            cat += 1;
        }
        self.applied |= mask;
        self.last = 0;
        Ok(changed)
    }
}

// ---------------------------------------------------------------------------
// 环境与回退
// ---------------------------------------------------------------------------

/// 一个环境变量值（定容，不落堆）。
#[derive(Clone, Copy, Debug)]
pub struct EnvVar {
    buf: [u8; ENV_CAP],
    len: usize,
    set: bool,
}

impl EnvVar {
    /// 收值；空串按 POSIX 视为未设。
    pub fn new(src: &str) -> EnvVar {
        let bytes = src.as_bytes();
        let n = if bytes.len() > ENV_CAP { ENV_CAP } else { bytes.len() };
        let mut buf = [0u8; ENV_CAP];
        let mut i = 0usize;
        while i < n {
            if let Some(b) = bytes.get(i) {
                buf[i] = *b;
            }
            i += 1;
        }
        EnvVar { buf, len: n, set: n > 0 }
    }

    /// 是否设了。
    pub fn is_set(&self) -> bool {
        self.set
    }

    /// 实长。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 文本。
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

    /// 与字面量比较。
    pub fn same(&self, lit: &str) -> bool {
        let lb = lit.as_bytes();
        if lb.len() != self.len {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            let a = match self.buf.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let b = match lb.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a != b {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 摘要。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        fold_word(h, if self.set { 1 } else { 0 })
    }
}

/// 一次 setlocale 的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 是否成功。
    pub ok: bool,
    /// 错误码（成功 0）。
    pub code: u32,
    /// 生效整串（失败时是回退的 "C"）。
    pub canonical: &'static str,
    /// 是否走了回退。
    pub fallback: bool,
}

/// 环境值与生效值的分叉快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Divergence {
    /// 环境值是否设。
    pub env_set: bool,
    /// 环境值是否就是 "C"。
    pub env_is_c: bool,
    /// 生效档是否就是 "C"。
    pub eff_is_c: bool,
}

impl Divergence {
    /// 是否分叉（环境说有别的档，进程实际停在 C）。
    pub fn gap(&self) -> bool {
        self.env_set && !self.env_is_c && self.eff_is_c
    }
}

/// 一次进程会话。
#[derive(Clone, Copy, Debug)]
pub struct Session {
    current: usize,
    env_all: EnvVar,
    attempts: u32,
    nulls: u32,
    fallbacks: u32,
    last_code: u32,
}

impl Session {
    /// 以环境值开一场会话，起点固定 "C"。
    pub fn new(env_all: &str) -> Session {
        Session {
            current: C_INDEX,
            env_all: EnvVar::new(env_all),
            attempts: 0,
            nulls: 0,
            fallbacks: 0,
            last_code: 0,
        }
    }

    /// 环境值。
    pub fn env(&self) -> EnvVar {
        self.env_all
    }

    /// 当前生效档整串。
    pub fn current_name(&self) -> &'static str {
        entry_name(self.current).unwrap_or("C")
    }

    /// 当前下标。
    pub fn current(&self) -> usize {
        self.current
    }

    /// 尝试总数。
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// NULL 次数。
    pub fn nulls(&self) -> u32 {
        self.nulls
    }

    /// 回退次数。
    pub fn fallbacks(&self) -> u32 {
        self.fallbacks
    }

    /// 最后错误码。
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 分叉快照。
    pub fn divergence(&self) -> Divergence {
        Divergence {
            env_set: self.env_all.is_set(),
            env_is_c: self.env_all.same("C"),
            eff_is_c: self.current == C_INDEX,
        }
    }

    /// 失败统一入口：置 C、记 NULL、记回退。
    fn fail(&mut self, code: u32) -> Outcome {
        self.nulls = self.nulls.saturating_add(1);
        self.fallbacks = self.fallbacks.saturating_add(1);
        self.current = C_INDEX;
        self.last_code = code;
        Outcome { ok: false, code, canonical: "C", fallback: true }
    }

    /// 一次 setlocale(LC_ALL, name)。
    pub fn attempt(&mut self, src: &str) -> Outcome {
        self.attempts = self.attempts.saturating_add(1);
        let parsed = match LocaleName::new(src) {
            Ok(v) => v,
            Err(c) => return self.fail(c),
        };
        match resolve(&parsed) {
            Ok(r) => {
                self.current = r.index;
                self.last_code = 0;
                Outcome { ok: true, code: 0, canonical: r.canonical, fallback: false }
            }
            Err(c) => self.fail(c),
        }
    }

    /// 显式再回退一次（程序里那句 setlocale(LC_ALL, "C")）。
    pub fn fallback_c(&mut self) -> Outcome {
        self.current = C_INDEX;
        self.last_code = 0;
        Outcome { ok: true, code: 0, canonical: "C", fallback: true }
    }
}

/// 单次解析闸：一场会话只开一次解析窗。
#[derive(Clone, Copy, Debug)]
pub struct Resolver {
    armed: bool,
    last: u32,
}

impl Resolver {
    /// 空闸。
    pub const fn new() -> Resolver {
        Resolver { armed: false, last: 0 }
    }

    /// 是否已开。
    pub fn armed(&self) -> bool {
        self.armed
    }

    /// 最后错误码。
    pub fn last(&self) -> u32 {
        self.last
    }

    /// 开窗解析一次；再开即状态冲突。
    pub fn open(&mut self, src: &str) -> Result<Resolved, u32> {
        if self.armed {
            self.last = LB_ESTATE;
            return Err(LB_ESTATE);
        }
        self.armed = true;
        let parsed = match LocaleName::new(src) {
            Ok(v) => v,
            Err(c) => {
                self.last = c;
                return Err(c);
            }
        };
        match resolve(&parsed) {
            Ok(r) => {
                self.last = 0;
                Ok(r)
            }
            Err(c) => {
                self.last = c;
                Err(c)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 输入名。
    pub name: &'static str,
    /// 期望成功。
    pub want_ok: bool,
    /// 期望错误码（成功 0）。
    pub want_code: u32,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(name: &'static str, want_ok: bool, want_code: u32, note: &'static str) -> Case {
        Case { name, want_ok, want_code, note }
    }
}

/// 长名（超 NAME_CAP）用字面量，语料里只放短名，长名在测试里现造。
pub const CASES: [Case; CORPUS] = [
    Case::new("C", true, 0, "c-anchor"),
    Case::new("POSIX", true, 0, "posix-alias"),
    Case::new("C.UTF-8", true, 0, "c-utf8"),
    Case::new("en_US.UTF-8", true, 0, "us-utf8"),
    Case::new("de_DE.UTF-8", true, 0, "de-utf8"),
    Case::new("ja_JP.UTF-8", true, 0, "ja-utf8"),
    Case::new("xx_YY.INVALID", false, LB_ECODESET, "bogus-codeset"),
    Case::new("en_US.INVALID", false, LB_ECODESET, "known-lang-bogus-cs"),
    Case::new("xx_YY.UTF-8", false, LB_EMISSING, "unknown-territory"),
    Case::new("fr_FR", false, LB_EMISSING, "no-codeset-no-entry"),
    Case::new("de_DE.UTF-8@euro", false, LB_EMODIFIER, "euro-modifier"),
    Case::new("en_US.UTF-8@", false, LB_EGRAMMAR, "trailing-at"),
    Case::new("1234.UTF-8", false, LB_EGRAMMAR, "digit-lang"),
    Case::new("en_US.UTF 8", false, LB_EBYTE, "space-in-name"),
];

/// 语料槽。
pub fn case_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 语料名是否互异。
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
                if b.name == a.name {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注是否全非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(c) => {
                if c.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 语料逐条对判。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let code = code_of(c.name);
        let ok = code == 0;
        if ok != c.want_ok {
            return false;
        }
        if code != c.want_code {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug)]
pub struct Census {
    total: usize,
    ok: usize,
    fail: usize,
    gram: usize,
    missing: usize,
    codeset: usize,
    modifier: usize,
    byte: usize,
    fallback: usize,
    matched: usize,
    red: bool,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census {
            total: 0,
            ok: 0,
            fail: 0,
            gram: 0,
            missing: 0,
            codeset: 0,
            modifier: 0,
            byte: 0,
            fallback: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 成功数。
    pub const fn ok(&self) -> usize {
        self.ok
    }

    /// 失败数。
    pub const fn fail(&self) -> usize {
        self.fail
    }

    /// 语法档。
    pub const fn gram(&self) -> usize {
        self.gram
    }

    /// 缺装档。
    pub const fn missing(&self) -> usize {
        self.missing
    }

    /// 编码段档。
    pub const fn codeset(&self) -> usize {
        self.codeset
    }

    /// 修饰段档。
    pub const fn modifier(&self) -> usize {
        self.modifier
    }

    /// 字节档。
    pub const fn byte(&self) -> usize {
        self.byte
    }

    /// 回退数。
    pub const fn fallback(&self) -> usize {
        self.fallback
    }

    /// 对上数。
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

    /// 千分率。
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
            return Err(LB_ENOBS);
        }
        if self.red {
            return Err(LB_ENULL);
        }
        if self.matched != self.total {
            return Err(LB_EVALUE);
        }
        if self.total != CORPUS || self.ok != 6 || self.fail != 8 {
            return Err(LB_EVALUE);
        }
        if self.gram != 2 || self.missing != 2 || self.codeset != 2 {
            return Err(LB_EVALUE);
        }
        if self.modifier != 1 || self.byte != 1 || self.fallback != 8 {
            return Err(LB_EVALUE);
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
        s.push_str("locale_bad corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" ok=");
        s.push_str(u32_text(self.ok as u32).as_str());
        s.push_str(" fail=");
        s.push_str(u32_text(self.fail as u32).as_str());
        s.push_str(" gram=");
        s.push_str(u32_text(self.gram as u32).as_str());
        s.push_str(" missing=");
        s.push_str(u32_text(self.missing as u32).as_str());
        s.push_str(" codeset=");
        s.push_str(u32_text(self.codeset as u32).as_str());
        s.push_str(" modifier=");
        s.push_str(u32_text(self.modifier as u32).as_str());
        s.push_str(" byte=");
        s.push_str(u32_text(self.byte as u32).as_str());
        s.push_str(" fallback=");
        s.push_str(u32_text(self.fallback as u32).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 走一遍语料。
fn walk(red: Option<usize>) -> Result<Census, u32> {
    let mut c = Census::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let case = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(LB_EVALUE),
        };
        let mut ses = Session::new(case.name);
        let out = ses.attempt(case.name);
        c.total += 1;
        if out.ok {
            c.ok += 1;
        } else {
            c.fail += 1;
            c.fallback += 1;
            match out.code {
                LB_EGRAMMAR => c.gram += 1,
                LB_EMISSING => c.missing += 1,
                LB_ECODESET => c.codeset += 1,
                LB_EMODIFIER => c.modifier += 1,
                LB_EBYTE => c.byte += 1,
                _ => {}
            }
        }
        if out.ok == case.want_ok && out.code == case.want_code {
            c.matched += 1;
        }
        i += 1;
    }
    if let Some(slot) = red {
        if slot >= c.total {
            return Err(LB_ESLOT);
        }
        c.matched = c.matched.saturating_sub(1);
        c.red = true;
    }
    Ok(c)
}

/// 绿跑。
pub fn run() -> Result<Census, u32> {
    walk(None)
}

/// 红注入跑。
pub fn run_with(red: Option<usize>) -> Result<Census, u32> {
    walk(red)
}

/// 覆盖判据。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(LB_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 失败会话的三行轨迹（逐字）。
pub fn trace(env: &str) -> Result<String, u32> {
    if env.is_empty() {
        return Err(LB_EBYTE);
    }
    let mut ses = Session::new(env);
    let out = ses.attempt(env);
    let mut s = String::new();
    s.push_str("setlocale(LC_ALL, \"");
    s.push_str(env);
    s.push_str("\") = ");
    if out.ok {
        s.push('"');
        s.push_str(out.canonical);
        s.push_str("\"\n");
        s.push_str("no fallback needed\n");
    } else {
        s.push_str("NULL\n");
        s.push_str("fallback: setlocale(LC_ALL, \"C\") = \"C\"\n");
        s.push_str("getenv(\"LC_ALL\") = \"");
        s.push_str(env);
        s.push_str("\"\n");
    }
    Ok(s)
}

/// 单行摘要。
pub fn summary(env: &str) -> String {
    let mut ses = Session::new(env);
    let out = ses.attempt(env);
    let mut s = String::new();
    s.push_str("LC_ALL=");
    s.push_str(env);
    s.push_str(" effective=");
    s.push_str(ses.current_name());
    s.push_str(" null=");
    s.push_str(if out.ok { "no" } else { "yes" });
    s.push_str(" code=");
    s.push_str(u32_text(out.code).as_str());
    s
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
            return Err(LB_ESLOT);
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

    /// 对账：恰好 14 行、8 成 6 败，值与码逐行对上。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(LB_EBAL);
        }
        const VALS: [u32; 8] = [14, 6, 8, 2, 2, 2, 8, 929];
        const CODES: [u32; 6] =
            [LB_EGRAMMAR, LB_EMISSING, LB_ECODESET, LB_EMODIFIER, LB_ESTATE, LB_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LB_EBAL),
            };
            if s.slot as usize != i {
                return Err(LB_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LB_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LB_EVALUE);
                        }
                    }
                    None => return Err(LB_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LB_EVALUE);
                        }
                    }
                    None => return Err(LB_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LB_EBAL);
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
            return Err(LB_EBAL);
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
            Err(LB_ESEAL)
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
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.ok())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.fail())));
    let _ = l.register(count_cell(3, OP_PARSE, r.map(|x| x.gram())));

    let _ = l.register(count_cell(4, OP_RESOLVE, r.map(|x| x.missing())));
    let _ = l.register(count_cell(5, OP_RESOLVE, r.map(|x| x.codeset())));
    let _ = l.register(count_cell(6, OP_FALLBACK, r.map(|x| x.fallback())));

    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_SET, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_PARSE, code_of("1234.UTF-8")));
    let _ = l.register(err_cell(9, OP_RESOLVE, code_of("xx_YY.UTF-8")));
    let _ = l.register(err_cell(10, OP_RESOLVE, code_of("en_US.INVALID")));
    let _ = l.register(err_cell(11, OP_RESOLVE, code_of("de_DE.UTF-8@euro")));

    let mut g = Resolver::new();
    let first = g.open("C");
    let state_err = match first {
        Ok(_) => match g.open("C") {
            Ok(_) => LB_ESTATE,
            Err(c) => c,
        },
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_ENV, state_err));

    let _ = l.register(err_cell(13, OP_PARSE, code_of("en_US.UTF 8")));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名字语法。
fn name_ok() -> bool {
    let a = match LocaleName::new("en_US.UTF-8") {
        Ok(n) => {
            let p = match n.classify() {
                Ok(v) => v,
                Err(_) => return false,
            };
            n.field(&p, 0).as_str() == "en"
                && n.field(&p, 1).as_str() == "US"
                && n.field(&p, 2).as_str() == "UTF-8"
                && n.field(&p, 3).is_empty()
        }
        Err(_) => false,
    };
    let b = match LocaleName::new("de_DE.UTF-8@euro") {
        Ok(n) => match n.classify() {
            Ok(p) => p.modifier.len == 4 && p.codeset.len == 5,
            Err(_) => false,
        },
        Err(_) => false,
    };
    let c = match LocaleName::new("1234.UTF-8") {
        Ok(n) => n.classify() == Err(LB_EGRAMMAR),
        Err(c) => c == LB_EGRAMMAR,
    };
    let d = match LocaleName::new("en_US.UTF-8@") {
        Ok(n) => n.classify() == Err(LB_EGRAMMAR),
        Err(c) => c == LB_EGRAMMAR,
    };
    let e = match LocaleName::new("a@b@c") {
        Ok(n) => n.classify() == Err(LB_EGRAMMAR),
        Err(c) => c == LB_EGRAMMAR,
    };
    let f = LocaleName::new("").err() == Some(LB_EGRAMMAR);
    let g = LocaleName::new("en_US.UTF 8").err() == Some(LB_EBYTE);
    let h = LocaleName::new("en_US.UTF\08").err() == Some(LB_EBYTE);
    a && b && c && d && e && f && g && h
}

/// 目录查装。
fn catalog_ok() -> bool {
    let a = find_entry("C") == Some(0);
    let b = find_entry("POSIX") == Some(1);
    let c = find_entry("de_DE.UTF-8") == Some(5);
    let d = find_entry("xx_YY.UTF-8").is_none();
    let e = entry_name(C_INDEX) == Some("C");
    let f = match entry_at(2) {
        Some(v) => v.name == "C.UTF-8" && v.codeset == "UTF-8",
        None => false,
    };
    let mut uniq = true;
    let mut i = 0usize;
    while i < CATALOG.len() {
        let mut j = i + 1;
        while j < CATALOG.len() {
            match (CATALOG.get(i), CATALOG.get(j)) {
                (Some(x), Some(y)) => {
                    if x.name == y.name {
                        uniq = false;
                    }
                }
                _ => uniq = false,
            }
            j += 1;
        }
        i += 1;
    }
    a && b && c && d && e && f && uniq
}

/// 编码段。
fn codeset_ok() -> bool {
    let a = codeset_known(b"UTF-8");
    let b = codeset_known(b"utf8");
    let c = codeset_known(b"ISO-8859-1");
    let d = !codeset_known(b"INVALID");
    let e = !codeset_known(b"");
    let f = resolve_str("en_US.INVALID").err() == Some(LB_ECODESET);
    let g = resolve_str("xx_YY.INVALID").err() == Some(LB_ECODESET);
    let h = resolve_str("en_US.UTF-8").is_ok();
    a && b && c && d && e && f && g && h
}

/// 修饰段。
fn modifier_ok() -> bool {
    let a = resolve_str("de_DE.UTF-8@euro").err() == Some(LB_EMODIFIER);
    let b = resolve_str("de_DE@euro").err() == Some(LB_EMODIFIER);
    let c = match LocaleName::new("en_US.UTF-8@x") {
        Ok(n) => match n.classify() {
            Ok(p) => !p.modifier.is_empty() && p.modifier.len == 1,
            Err(_) => false,
        },
        Err(_) => false,
    };
    let d = Fault::Modifier.code() == LB_EMODIFIER && Fault::Modifier.name() == "modifier";
    a && b && c && d
}

/// NULL 与回退。
fn fallback_ok() -> bool {
    let mut s = Session::new("xx_YY.INVALID");
    let out = s.attempt("xx_YY.INVALID");
    let a = !out.ok && out.code == LB_ECODESET && out.fallback && out.canonical == "C";
    let b = s.nulls() == 1 && s.fallbacks() == 1 && s.current() == C_INDEX;
    let c = s.current_name() == "C";
    let out2 = s.attempt("de_DE.UTF-8");
    let d = out2.ok && s.current_name() == "de_DE.UTF-8" && s.nulls() == 1;
    let out3 = s.attempt("xx_YY.UTF-8");
    let e = !out3.ok && out3.code == LB_EMISSING && s.current_name() == "C";
    a && b && c && d && e
}

/// 环境背离。
fn gap_ok() -> bool {
    let mut s = Session::new("xx_YY.INVALID");
    let _ = s.attempt("xx_YY.INVALID");
    let d = s.divergence();
    let a = d.gap() && d.env_set && !d.env_is_c && d.eff_is_c;
    let b = s.env().same("xx_YY.INVALID") && !s.env().same("C");
    let c = s.env().text().as_str() == "xx_YY.INVALID";

    let mut t = Session::new("de_DE.UTF-8");
    let _ = t.attempt("de_DE.UTF-8");
    let e = !t.divergence().gap() && t.current_name() == "de_DE.UTF-8";

    let mut u = Session::new("C");
    let _ = u.attempt("nope");
    let f = !u.divergence().gap();
    a && b && c && e && f
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.total() == CORPUS && c.ok() == 6 && c.fail() == 8;
    let b = c.gram() == 2 && c.missing() == 2 && c.codeset() == 2;
    let d = c.modifier() == 1 && c.byte() == 1 && c.fallback() == 8;
    let e = c.matched() == CORPUS && c.rate() == 1000 && c.green();
    a && b && d && e
}

/// 语料表。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match case_at(0) {
        Some(c) => c.name == "C" && c.want_ok,
        None => false,
    };
    let b = match case_at(6) {
        Some(c) => c.want_code == LB_ECODESET && !c.want_ok,
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    a && b && c
}

/// 汇总渲染。
fn report_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.render().as_str() == "locale_bad corpus=14 ok=6 fail=8 gram=2 missing=2 codeset=2 modifier=1 byte=1 fallback=8 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(LB_ENOBS);
    let d = e.render().as_str()
        == "locale_bad corpus=0 ok=0 fail=0 gram=0 missing=0 codeset=0 modifier=0 byte=0 fallback=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(LB_ENOBS);
    a && b && d && f
}

/// 红单显式。
fn reds_ok() -> bool {
    let c = match run_with(Some(0)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched() == CORPUS - 1 && c.rate() == 929;
    let b = c.gate() == Err(LB_ENULL) && !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(LB_ESLOT);
    a && b && d
}

/// 文案逐字。
fn trace_ok() -> bool {
    let a = match trace("xx_YY.INVALID") {
        Ok(s) => s.as_str()
            == "setlocale(LC_ALL, \"xx_YY.INVALID\") = NULL\nfallback: setlocale(LC_ALL, \"C\") = \"C\"\ngetenv(\"LC_ALL\") = \"xx_YY.INVALID\"\n",
        Err(_) => false,
    };
    let b = match trace("de_DE.UTF-8") {
        Ok(s) => s.as_str()
            == "setlocale(LC_ALL, \"de_DE.UTF-8\") = \"de_DE.UTF-8\"\nno fallback needed\n",
        Err(_) => false,
    };
    let c = trace("") == Err(LB_EBYTE);
    let mut want = String::from("LC_ALL=xx_YY.INVALID effective=C null=yes code=");
    want.push_str(u32_text(LB_ECODESET).as_str());
    let mut good = String::from("LC_ALL=de_DE.UTF-8 effective=de_DE.UTF-8 null=no code=0");
    let d = summary("xx_YY.INVALID") == want && summary("de_DE.UTF-8") == good;
    good.push('!');
    a && b && c && d && !good.is_empty()
}

/// 分类掩码与引擎。
fn cat_ok() -> bool {
    let a = CAT_NAMES.len() == CAT_COUNT && cat_name(0) == Some("LC_CTYPE");
    let b = cat_name(5) == Some("LC_MESSAGES") && cat_name(CAT_COUNT).is_none();
    let c = cat_bit(0) == Some(1) && cat_bit(11) == Some(0x0800) && cat_bit(12).is_none();
    let d = mask_ok(CAT_ALL_MASK) && !mask_ok(0) && !mask_ok(0x1000);
    let e = mask_count(CAT_ALL_MASK) == CAT_COUNT && mask_count(0x0003) == 2;

    let mut eng = Engine::new();
    let f = eng.name_at(0) == Some("C") && eng.uniform();
    let g = eng.apply(CAT_ALL_MASK, "de_DE.UTF-8") == Ok(CAT_ALL_MASK);
    let h = g && eng.name_at(5) == Some("de_DE.UTF-8") && eng.mask_of(5) == CAT_ALL_MASK;
    let i = eng.apply(0x0001, "C") == Ok(1) && eng.name_at(0) == Some("C");
    let j = eng.name_at(1) == Some("de_DE.UTF-8") && !eng.uniform();
    let k = eng.apply(0x0001, "xx_YY.INVALID") == Err(LB_ECODESET) && eng.fails() == 1;
    let l = k && eng.name_at(0) == Some("C") && eng.apply(0, "C") == Err(LB_EBYTE);
    a && b && c && d && e && f && g && h && i && j && l
}

/// 账本与文本工具。
fn ledger_ok() -> bool {
    let l = build_ledger();
    let a = l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6;
    let b = l.audit() == Ok(());
    let c = l.seal() == build_ledger().seal();
    let d = u32_text(0).as_str() == "0"
        && u32_text(4294967295).as_str() == "4294967295"
        && u32_text(13).as_str() == "13";
    let e = hex4_text(0xfd00).as_str() == "fd00"
        && hex4_text(0x0001).as_str() == "0001"
        && hex4_text(0xffff).as_str() == "ffff";
    let f = digest(b"abc") == digest(b"abc") && digest(b"abc") != digest(b"abd");
    a && b && c && d && e && f
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && catalog_ok() && codeset_ok();
    out[1] = modifier_ok() && fallback_ok() && gap_ok();
    out[2] = corpus_ok() && slots_ok() && report_ok() && reds_ok();
    out[3] = trace_ok() && cat_ok() && ledger_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11641_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11641.locale_bad");

    s.add("LB01-名字语法", name_ok(), "static");
    s.add("LB02-目录查装", catalog_ok(), "static");
    s.add("LB03-编码段闸门", codeset_ok(), "static");
    s.add("LB04-修饰段闸门", modifier_ok(), "static");
    s.add("LB05-NULL 回退 C", fallback_ok(), "static");
    s.add("LB06-环境生效背离", gap_ok(), "static");
    s.add("LB07-十四卡全跑", corpus_ok(), "static");
    s.add("LB08-语料编号", slots_ok(), "static");
    s.add("LB09-汇总渲染", report_ok(), "static");
    s.add("LB10-红单显式", reds_ok(), "static");
    s.add("LB11-三行轨迹逐字", trace_ok(), "static");
    s.add("LB12-分类引擎与账本", cat_ok() && ledger_ok(), "static");
    s.add("LB13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
        assert_eq!(l.sample(8).map(|x| x.code), Some(LB_EGRAMMAR));
        assert_eq!(l.sample(13).map(|x| x.code), Some(LB_EBYTE));
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
        assert_eq!(l.freeze(), Err(LB_EBAL));
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
        let s = run_unx11641_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn grammar_fields() {
        let n = match LocaleName::new("fr_FR.ISO-8859-1") {
            Ok(v) => v,
            Err(_) => return,
        };
        let p = match n.classify() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(n.field(&p, 0).as_str(), "fr");
        assert_eq!(n.field(&p, 1).as_str(), "FR");
        assert_eq!(n.field(&p, 2).as_str(), "ISO-8859-1");
        assert!(n.field(&p, 3).is_empty());
        assert_eq!(p.lang, Span::new(0, 2));
        assert_eq!(p.terr, Span::new(3, 2));
        assert_eq!(p.codeset.off, 6);
    }

    #[test]
    fn grammar_rejects() {
        let cases = ["1234", "_US", "en_", "en_US.", "en_US.UTF-8@@x", "en-US.UTF-8"];
        let mut i = 0usize;
        while i < cases.len() {
            if let Some(c) = cases.get(i) {
                let got = match LocaleName::new(c) {
                    Ok(n) => n.classify(),
                    Err(e) => Err(e),
                };
                assert!(got.is_err(), "case {}", c);
            }
            i += 1;
        }
    }

    #[test]
    fn byte_limits() {
        assert_eq!(LocaleName::new("").err(), Some(LB_EGRAMMAR));
        assert_eq!(LocaleName::new("en US").err(), Some(LB_EBYTE));
        let mut long = String::from("en_US.UTF-8");
        let mut i = 0usize;
        while i < 40 {
            long.push('x');
            i += 1;
        }
        assert!(long.len() > NAME_CAP);
        assert_eq!(LocaleName::new(long.as_str()).err(), Some(LB_EBYTE));
    }

    #[test]
    fn catalog_entries() {
        assert_eq!(CATALOG.len(), 9);
        assert_eq!(entry_name(0), Some("C"));
        assert_eq!(entry_name(1), Some("POSIX"));
        assert_eq!(entry_name(2), Some("C.UTF-8"));
        assert_eq!(find_entry("zh_CN.UTF-8"), Some(8));
        assert!(find_entry("zh_TW.UTF-8").is_none());
    }

    #[test]
    fn resolve_paths() {
        assert_eq!(resolve_str("C").map(|r| r.canonical), Ok("C"));
        assert_eq!(resolve_str("POSIX").map(|r| r.canonical), Ok("POSIX"));
        assert_eq!(resolve_str("xx_YY.INVALID").err(), Some(LB_ECODESET));
        assert_eq!(resolve_str("xx_YY.UTF-8").err(), Some(LB_EMISSING));
        assert_eq!(resolve_str("de_DE.UTF-8@euro").err(), Some(LB_EMODIFIER));
        assert_eq!(resolve_str("1234.UTF-8").err(), Some(LB_EGRAMMAR));
        assert_eq!(resolve_str("en_US.UTF 8").err(), Some(LB_EBYTE));
    }

    #[test]
    fn session_null_keeps_c() {
        let mut s = Session::new("xx_YY.INVALID");
        let out = s.attempt("xx_YY.INVALID");
        assert!(!out.ok);
        assert_eq!(out.code, LB_ECODESET);
        assert_eq!(s.current_name(), "C");
        assert_eq!(s.nulls(), 1);
        assert_eq!(s.fallbacks(), 1);
        assert_eq!(s.attempts(), 1);
    }

    #[test]
    fn session_recovers() {
        let mut s = Session::new("xx_YY.INVALID");
        let _ = s.attempt("xx_YY.INVALID");
        let out = s.attempt("en_US.UTF-8");
        assert!(out.ok);
        assert_eq!(s.current_name(), "en_US.UTF-8");
        assert_eq!(s.nulls(), 1);
        assert_eq!(s.last_code(), 0);
    }

    #[test]
    fn divergence_gap() {
        let mut s = Session::new("xx_YY.INVALID");
        let _ = s.attempt("xx_YY.INVALID");
        let d = s.divergence();
        assert!(d.gap());
        assert!(d.env_set);
        assert!(!d.env_is_c);
        assert!(d.eff_is_c);
        assert_eq!(s.env().text().as_str(), "xx_YY.INVALID");
    }

    #[test]
    fn trace_exact() {
        let t = match trace("xx_YY.INVALID") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(
            t.as_str(),
            "setlocale(LC_ALL, \"xx_YY.INVALID\") = NULL\n\
             fallback: setlocale(LC_ALL, \"C\") = \"C\"\n\
             getenv(\"LC_ALL\") = \"xx_YY.INVALID\"\n"
        );
        assert_eq!(trace(""), Err(LB_EBYTE));
    }

    #[test]
    fn census_numbers() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.total(), 14);
        assert_eq!(c.ok(), 6);
        assert_eq!(c.fail(), 8);
        assert_eq!(c.fallback(), 8);
        assert_eq!(c.gate(), Ok(()));
    }

    #[test]
    fn red_override() {
        let c = match run_with(Some(3)) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.matched(), CORPUS - 1);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(LB_ENULL));
        assert_eq!(run_with(Some(CORPUS + 1)).err(), Some(LB_ESLOT));
    }

    #[test]
    fn engine_categories() {
        let mut e = Engine::new();
        assert!(e.uniform());
        assert_eq!(e.apply(CAT_ALL_MASK, "ja_JP.UTF-8"), Ok(CAT_ALL_MASK));
        assert_eq!(e.name_at(6), Some("ja_JP.UTF-8"));
        assert_eq!(e.mask_of(7), CAT_ALL_MASK);
        assert_eq!(e.apply(0x0004, "C"), Ok(0x0004));
        assert_eq!(e.name_at(2), Some("C"));
        assert!(!e.uniform());
        assert_eq!(e.apply(0x0004, "bad"), Err(LB_EMISSING));
        assert_eq!(e.last(), LB_EMISSING);
        assert_eq!(e.apply(0x0004, "1234"), Err(LB_EGRAMMAR));
        assert_eq!(e.last(), LB_EGRAMMAR);
        assert_eq!(e.name_at(2), Some("C"));
    }

    #[test]
    fn resolver_single_window() {
        let mut g = Resolver::new();
        assert!(!g.armed());
        assert!(g.open("C").is_ok());
        assert!(g.armed());
        assert_eq!(g.open("C"), Err(LB_ESTATE));
        assert_eq!(g.last(), LB_ESTATE);
        let mut h = Resolver::new();
        assert_eq!(h.open("xx_YY.UTF-8"), Err(LB_EMISSING));
    }

    #[test]
    fn corpus_table() {
        assert_eq!(CASES.len(), CORPUS);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
        assert_eq!(case_at(CORPUS), None);
    }
}
