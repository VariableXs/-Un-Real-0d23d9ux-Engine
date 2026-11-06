//! UNX-F11569 sed y/// 与大小写转换 C 锚判据
//!
//! `sed 'y/abc/ABC/'` 是 sed 里唯一「按位置、逐字节」的变换：源串第 k 个字节
//! 换成目标串第 k 个字节，与正则、回溯、贪婪都无关。这条语义有三处容易走样：
//!
//! 1. **长度必须相等**。`y/ab/c/` 在 GNU sed 里是硬错误，不是「补齐」也不是
//!    「截断」——`tr` 会补齐，`sed` 不会。本单把长度不等归到 `SD_ELEN`，并且
//!    在解析阶段（而非应用阶段）就拒绝，避免半张表生效。
//! 2. **源字节必须唯一**。`y/aa/bb/` 的位置映射自相矛盾（第一个 a 说去 b，
//!    第二个 a 也说去 b，看似一致；但 `y/aa/bc/` 就冲突了）。本单统一要求源
//!    字节唯一，重复给 `SD_EDUP`——单值映射是「逐字节判据」可判定的前提。
//! 3. **转义是字节级**。`\n` 是 0x0a，`\101` 是 0x41，`\x41` 也是 0x41，
//!    `\/` 是被转义的定界符。四类写法必须落到同一条读字节路径上，否则
//!    「`y/\n/\t/` 到底换了什么」会随实现而异。
//!
//! 大小写转换是第二条语义轴，但它**不属于 y**：GNU sed 把 `\U \L \u \l \E`
//! 放在 `s///` 的替换串里。本单实现替换模板的展开器，并把「C 锚」写死：
//! 只有 `a..=z` 与 `A..=Z` 会被折叠，`0x80..=0xff` 一律原样透传。UTF-8 locale
//! 下 `\U` 可能把 `é` 变成 `É`（多字节），C locale 下这不可能发生——这条差异
//! 正是 `SD09` 用 0xe9/0xc3 两个字节钉住的判据。
//!
//! 模板状态机：`\U` 进入恒大写、`\L` 进入恒小写、`\E` 回到保持、`\u`/`\l` 只
//! 作用于**下一个输出字节**然后回落。`&` 展开为整段匹配，`\&` 是字面 `&`，
//! `\\` 是字面反斜杠。`\1`..`\9` 在本层明确不支持（没有捕获组），给 `SD_EBYTE`
//! 而不是静默输出 `1`——静默是这类实现最常见的错。
//!
//! 行处理：sed 的 pattern space 不含换行，换行由输出层补回。本单按 `\n` 切行，
//! 每行依次跑完 y 命令链与可选的 s 命令，再把换行原样写回；尾行无换行时也不
//! 凭空补一个。`SD07` 用「空行」「尾行无换行」「连续空行」三种输入验证这条。
//!
//! 闸门次序：命令字形（`SD_EINVAL`）→ 长度（`SD_ELEN`）→ 转义字节
//! （`SD_EBYTE`）→ 八进制/十六进制范围（`SD_ERANGE`）→ 源字节重复
//! （`SD_EDUP`）→ 截断（`SD_ESHORT`）→ 封存状态（`SD_ESTATE`）→ 尾随标志
//! （`SD_ECONF`）。前六道在 `parse_y`/`read_field`，状态在 `Script::apply`，
//! 尾随标志在字段读完后仍有残余字节时判定。
//!
//! 账本语义：十四格 = 八格成功（折叠项、映射计数、写出长度、高位锚、大小写对
//! 数、模板展开长度、行数、转义表项）+ 六格失败（长度不等、坏转义、八进制
//! 越界、源重复、未闭合、尾随标志）。`audit` 认「八值六码」，`seal` 做 FNV-1a
//! 折叠，`replay` 重建比对，`freeze` 后拒绝再登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf4a0;

/// 命令字形非法。
pub const SD_EINVAL: u32 = SEG_BASE + 0x01;
/// 源串与目标串长度不等，或为空。
pub const SD_ELEN: u32 = SEG_BASE + 0x02;
/// 转义字节非法。
pub const SD_EBYTE: u32 = SEG_BASE + 0x03;
/// 八进制/十六进制越界。
pub const SD_ERANGE: u32 = SEG_BASE + 0x04;
/// 源字节重复。
pub const SD_EDUP: u32 = SEG_BASE + 0x05;
/// 命令或转义被截断。
pub const SD_ESHORT: u32 = SEG_BASE + 0x06;
/// 已封存后继续使用。
pub const SD_ESTATE: u32 = SEG_BASE + 0x07;
/// 尾随标志/定界符冲突。
pub const SD_ECONF: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const SD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const SD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const SD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const SD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const SD_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 单串定容。
pub const SET_CAP: usize = 256;

/// 输出定容。
pub const OUT_CAP: usize = 1024;

/// 脚本命令定容。
pub const CMD_CAP: usize = 4;

/// 模板定容。
pub const TPL_CAP: usize = 128;

/// 操作码：y 解析。
pub const OP_YPARSE: u8 = 0x01;
/// 操作码：y 应用。
pub const OP_YAPPLY: u8 = 0x02;
/// 操作码：长度。
pub const OP_LEN: u8 = 0x03;
/// 操作码：转义。
pub const OP_ESC: u8 = 0x04;
/// 操作码：大小写。
pub const OP_CASE: u8 = 0x05;
/// 操作码：行。
pub const OP_LINE: u8 = 0x06;
/// 操作码：模板。
pub const OP_TPL: u8 = 0x07;
/// 操作码：重复。
pub const OP_DUP: u8 = 0x08;

/// 支持的转义字母（表项数）。
pub const ESCAPES: [u8; 8] = [b'n', b't', b'r', b'f', b'v', b'a', b'b', b'\\'];

/// 默认定界符。
pub const DEF_DELIM: u8 = b'/';

/// FNV-1a 偏移。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold(h: u64, b: u64) -> u64 {
    let mut v = h ^ (b & 0xff);
    let mut i = 0u32;
    while i < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    v
}

/// 十进制拼接。
fn push_dec(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20usize;
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut k = i;
    while k < 20 {
        if let Some(b) = buf.get(k) {
            s.push(*b as char);
        }
        k += 1;
    }
}

/// 有符号十进制拼接。
fn push_i64(s: &mut String, v: i64) {
    if v < 0 {
        s.push('-');
        push_dec(s, v.unsigned_abs());
    } else {
        push_dec(s, v as u64);
    }
}

/// 两位十六进制。
fn push_hex2(s: &mut String, b: u8) {
    const DIG: [u8; 16] = *b"0123456789abcdef";
    if let Some(c) = DIG.get((b >> 4) as usize) {
        s.push(*c as char);
    }
    if let Some(c) = DIG.get((b & 0x0f) as usize) {
        s.push(*c as char);
    }
}

/// 三位八进制。
fn push_oct3(s: &mut String, b: u8) {
    s.push((b'0' + ((b >> 6) & 0x07)) as char);
    s.push((b'0' + ((b >> 3) & 0x07)) as char);
    s.push((b'0' + (b & 0x07)) as char);
}

/// 另起一行（首行不加）。
fn newline(s: &mut String) {
    if !s.is_empty() {
        s.push('\n');
    }
}

/// C 锚大写：只动 `a..=z`。
pub fn toupper_c(b: u8) -> u8 {
    if b >= b'a' && b <= b'z' {
        b.wrapping_sub(0x20)
    } else {
        b
    }
}

/// C 锚小写：只动 `A..=Z`。
pub fn tolower_c(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b.wrapping_add(0x20)
    } else {
        b
    }
}

/// 大小写对数（26）。
pub fn case_pairs() -> u32 {
    let mut n = 0u32;
    let mut i = 0u32;
    while i < 256 {
        if toupper_c(i as u8) != i as u8 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 定容字节串。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteList {
    b: [u8; SET_CAP],
    len: usize,
}

impl ByteList {
    /// 空。
    pub const fn new() -> ByteList {
        ByteList { b: [0; SET_CAP], len: 0 }
    }

    /// 追加。
    pub fn push(&mut self, v: u8) -> Result<(), u32> {
        if self.len >= SET_CAP {
            return Err(SD_ELEN);
        }
        if let Some(d) = self.b.get_mut(self.len) {
            *d = v;
        }
        self.len += 1;
        Ok(())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 个。
    pub fn at(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.b.get(i).copied()
    }

    /// 切片。
    pub fn as_slice(&self) -> &[u8] {
        self.b.get(..self.len).unwrap_or(&[])
    }

    /// 是否含。
    pub fn contains(&self, v: u8) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.b.get(i) {
                if *b == v {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 转义渲染。
    pub fn escaped(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.b.get(i) {
                s.push('\\');
                push_oct3(&mut s, *b);
            }
            i += 1;
        }
        s
    }
}

/// 读一个转义（`i` 指向反斜杠之后）。
pub fn read_escape(raw: &[u8], i: &mut usize, delim: u8) -> Result<u8, u32> {
    let c = match raw.get(*i) {
        Some(v) => *v,
        None => return Err(SD_ESHORT),
    };
    *i += 1;
    if c == delim {
        return Ok(delim);
    }
    match c {
        b'\\' => Ok(b'\\'),
        b'n' => Ok(0x0a),
        b't' => Ok(0x09),
        b'r' => Ok(0x0d),
        b'f' => Ok(0x0c),
        b'v' => Ok(0x0b),
        b'a' => Ok(0x07),
        b'b' => Ok(0x08),
        b'&' => Ok(b'&'),
        b'x' => {
            let mut v = 0u32;
            let mut n = 0u32;
            while n < 2 {
                let d = match raw.get(*i) {
                    Some(v) => *v,
                    None => break,
                };
                let hv = match d {
                    b'0'..=b'9' => (d - b'0') as u32,
                    b'a'..=b'f' => (d - b'a') as u32 + 10,
                    b'A'..=b'F' => (d - b'A') as u32 + 10,
                    _ => break,
                };
                v = v * 16 + hv;
                *i += 1;
                n += 1;
            }
            if n == 0 {
                return Err(SD_EBYTE);
            }
            if v > 0xff {
                return Err(SD_ERANGE);
            }
            Ok(v as u8)
        }
        b'0'..=b'7' => {
            let mut v = (c - b'0') as u32;
            let mut n = 1u32;
            while n < 3 {
                let d = match raw.get(*i) {
                    Some(v) => *v,
                    None => break,
                };
                if !(b'0'..=b'7').contains(&d) {
                    break;
                }
                v = v * 8 + (d - b'0') as u32;
                *i += 1;
                n += 1;
            }
            if v > 0xff {
                return Err(SD_ERANGE);
            }
            Ok(v as u8)
        }
        _ => Err(SD_EBYTE),
    }
}

/// 读一个定界字段。
fn read_field(raw: &[u8], i: &mut usize, delim: u8, out: &mut ByteList) -> Result<(), u32> {
    let mut closed = false;
    while *i < raw.len() {
        let b = match raw.get(*i) {
            Some(v) => *v,
            None => break,
        };
        if b == delim {
            *i += 1;
            closed = true;
            break;
        }
        if b == b'\\' {
            *i += 1;
            let v = read_escape(raw, i, delim)?;
            out.push(v)?;
            continue;
        }
        out.push(b)?;
        *i += 1;
    }
    if closed {
        Ok(())
    } else {
        Err(SD_ESHORT)
    }
}

/// 一条 y 命令。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YCommand {
    map: [u8; 256],
    src: ByteList,
    dst: ByteList,
    folds: u16,
    delim: u8,
}

impl YCommand {
    /// 恒等。
    pub fn identity() -> YCommand {
        let mut map = [0u8; 256];
        let mut i = 0usize;
        while i < 256 {
            map[i] = i as u8;
            i += 1;
        }
        YCommand {
            map,
            src: ByteList::new(),
            dst: ByteList::new(),
            folds: 0,
            delim: DEF_DELIM,
        }
    }

    /// 映射表。
    pub fn mapping(&self) -> &[u8; 256] {
        &self.map
    }

    /// 源串。
    pub fn source(&self) -> &ByteList {
        &self.src
    }

    /// 目标串。
    pub fn target(&self) -> &ByteList {
        &self.dst
    }

    /// 折叠项数。
    pub fn folds(&self) -> u16 {
        self.folds
    }

    /// 定界符。
    pub fn delim(&self) -> u8 {
        self.delim
    }

    /// 是否恒等。
    pub fn is_identity(&self) -> bool {
        self.folds == 0
    }

    /// 应用一行。
    pub fn apply(&self, inp: &[u8], out: &mut [u8]) -> Result<YStats, u32> {
        let mut st = YStats { read: inp.len(), written: 0, mapped: 0 };
        let mut i = 0usize;
        while i < inp.len() {
            let b = match inp.get(i) {
                Some(v) => *v,
                None => break,
            };
            let t = match self.map.get(b as usize) {
                Some(v) => *v,
                None => b,
            };
            if t != b {
                st.mapped += 1;
            }
            match out.get_mut(st.written) {
                Some(d) => *d = t,
                None => return Err(SD_ESHORT),
            }
            st.written += 1;
            i += 1;
        }
        Ok(st)
    }

    /// 求逆（双射才可逆）。
    pub fn inverse(&self) -> Result<YCommand, u32> {
        let mut r = YCommand::identity();
        r.delim = self.delim;
        let mut i = 0usize;
        while i < self.src.len() {
            let a = match self.src.at(i) {
                Some(v) => v,
                None => return Err(SD_ELEN),
            };
            let b = match self.dst.at(i) {
                Some(v) => v,
                None => return Err(SD_ELEN),
            };
            if r.src.contains(b) {
                return Err(SD_EDUP);
            }
            r.src.push(b)?;
            r.dst.push(a)?;
            if b != a {
                r.folds = r.folds.saturating_add(1);
            }
            if let Some(d) = r.map.get_mut(b as usize) {
                *d = a;
            }
            i += 1;
        }
        Ok(r)
    }

    /// 渲染映射表（非恒等项每行 8 对）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut col = 0u32;
        let mut i = 0u32;
        while i < 256 {
            let b = i as u8;
            let t = match self.map.get(i as usize) {
                Some(v) => *v,
                None => b,
            };
            if t != b {
                if col == 0 {
                    newline(&mut s);
                } else {
                    s.push(' ');
                }
                push_hex2(&mut s, b);
                s.push('>');
                push_hex2(&mut s, t);
                col += 1;
                if col == 8 {
                    col = 0;
                }
            }
            i += 1;
        }
        newline(&mut s);
        s.push_str("folds ");
        push_dec(&mut s, self.folds as u64);
        s.push_str(" delim ");
        push_hex2(&mut s, self.delim);
        s
    }

    /// 计划串。
    pub fn plan(&self) -> String {
        let mut s = String::from("y");
        s.push_str(" src=");
        push_dec(&mut s, self.src.len() as u64);
        s.push_str(" dst=");
        push_dec(&mut s, self.dst.len() as u64);
        s.push_str(" folds=");
        push_dec(&mut s, self.folds as u64);
        s
    }
}

/// y 应用统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YStats {
    /// 读入。
    pub read: usize,
    /// 写出。
    pub written: usize,
    /// 值变化次数（同一字节多次出现各计一次）。
    pub mapped: u32,
}

impl YStats {
    /// 零。
    pub const fn zero() -> YStats {
        YStats { read: 0, written: 0, mapped: 0 }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.read as u64);
        s.push('/');
        push_dec(&mut s, self.written as u64);
        s.push('/');
        push_dec(&mut s, self.mapped as u64);
        s
    }
}

/// 解析一条 y 命令（含命令字与定界符）。
pub fn parse_y(raw: &[u8]) -> Result<YCommand, u32> {
    if raw.len() < 4 {
        return Err(SD_ESHORT);
    }
    if raw.first().copied() != Some(b'y') {
        return Err(SD_EINVAL);
    }
    let delim = match raw.get(1) {
        Some(v) => *v,
        None => return Err(SD_ESHORT),
    };
    if delim == b'\\' || delim == 0x0a || delim == b' ' || delim == 0 {
        return Err(SD_EINVAL);
    }
    let mut i = 2usize;
    let mut src = ByteList::new();
    let mut dst = ByteList::new();
    read_field(raw, &mut i, delim, &mut src)?;
    read_field(raw, &mut i, delim, &mut dst)?;
    if i < raw.len() {
        return Err(SD_ECONF);
    }
    if src.len() != dst.len() || src.is_empty() {
        return Err(SD_ELEN);
    }
    let mut cmd = YCommand::identity();
    cmd.delim = delim;
    let mut k = 0usize;
    while k < src.len() {
        let a = match src.at(k) {
            Some(v) => v,
            None => return Err(SD_ELEN),
        };
        let b = match dst.at(k) {
            Some(v) => v,
            None => return Err(SD_ELEN),
        };
        if cmd.src.contains(a) {
            return Err(SD_EDUP);
        }
        cmd.src.push(a)?;
        cmd.dst.push(b)?;
        if a != b {
            cmd.folds = cmd.folds.saturating_add(1);
        }
        if let Some(d) = cmd.map.get_mut(a as usize) {
            *d = b;
        }
        k += 1;
    }
    Ok(cmd)
}

/// 大小写模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseMode {
    /// 保持。
    Keep,
    /// 恒大写。
    Upper,
    /// 恒小写。
    Lower,
}

impl CaseMode {
    /// 名。
    pub fn name(&self) -> &'static str {
        match self {
            CaseMode::Keep => "keep",
            CaseMode::Upper => "upper",
            CaseMode::Lower => "lower",
        }
    }

    /// 施加。
    pub fn apply(&self, b: u8) -> u8 {
        match self {
            CaseMode::Keep => b,
            CaseMode::Upper => toupper_c(b),
            CaseMode::Lower => tolower_c(b),
        }
    }
}

/// 模板展开结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TplStats {
    /// 写出字节。
    pub written: usize,
    /// 实际折叠次数。
    pub folds: u32,
    /// 整段匹配展开次数。
    pub matches: u32,
}

/// 展开替换模板（`matched` 为整段匹配）。
pub fn expand_template(
    tpl: &[u8],
    matched: &[u8],
    out: &mut [u8],
) -> Result<TplStats, u32> {
    let mut st = TplStats { written: 0, folds: 0, matches: 0 };
    let mut mode = CaseMode::Keep;
    let mut once: Option<CaseMode> = None;
    let mut i = 0usize;
    while i < tpl.len() {
        let b = match tpl.get(i) {
            Some(v) => *v,
            None => break,
        };
        if b == b'\\' {
            let c = match tpl.get(i + 1) {
                Some(v) => *v,
                None => return Err(SD_ESHORT),
            };
            i += 2;
            match c {
                b'U' => {
                    mode = CaseMode::Upper;
                    once = None;
                    continue;
                }
                b'L' => {
                    mode = CaseMode::Lower;
                    once = None;
                    continue;
                }
                b'E' => {
                    mode = CaseMode::Keep;
                    once = None;
                    continue;
                }
                b'u' | b'l' => {
                    let m = if c == b'u' { CaseMode::Upper } else { CaseMode::Lower };
                    once = Some(m);
                    continue;
                }
                b'&' => {
                    let v = b'&';
                    let m = once.take().unwrap_or(mode);
                    let t = m.apply(v);
                    if t != v {
                        st.folds += 1;
                    }
                    match out.get_mut(st.written) {
                        Some(d) => *d = t,
                        None => return Err(SD_ESHORT),
                    }
                    st.written += 1;
                    continue;
                }
                b'n' | b't' | b'r' | b'f' | b'v' | b'a' | b'b' | b'\\' => {
                    let mut j = i - 1;
                    let v = read_escape(tpl, &mut j, 0)?;
                    let m = once.take().unwrap_or(mode);
                    let t = m.apply(v);
                    if t != v {
                        st.folds += 1;
                    }
                    match out.get_mut(st.written) {
                        Some(d) => *d = t,
                        None => return Err(SD_ESHORT),
                    }
                    st.written += 1;
                    i = j;
                    continue;
                }
                b'x' | b'0' => {
                    let mut j = i - 1;
                    let v = read_escape(tpl, &mut j, 0)?;
                    let m = once.take().unwrap_or(mode);
                    let t = m.apply(v);
                    if t != v {
                        st.folds += 1;
                    }
                    match out.get_mut(st.written) {
                        Some(d) => *d = t,
                        None => return Err(SD_ESHORT),
                    }
                    st.written += 1;
                    i = j;
                    continue;
                }
                // `\1`..`\9` 是反向引用，本实现不做捕获，按坏转义拒绝；
                // 八进制必须写成 `\0NNN` 形式，避免与反向引用混淆。
                b'1'..=b'9' => return Err(SD_EBYTE),
                _ => return Err(SD_EBYTE),
            }
        }
        if b == b'&' {
            let mut k = 0usize;
            while k < matched.len() {
                let v = match matched.get(k) {
                    Some(x) => *x,
                    None => break,
                };
                let m = once.take().unwrap_or(mode);
                let t = m.apply(v);
                if t != v {
                    st.folds += 1;
                }
                match out.get_mut(st.written) {
                    Some(d) => *d = t,
                    None => return Err(SD_ESHORT),
                }
                st.written += 1;
                k += 1;
            }
            st.matches += 1;
            i += 1;
            continue;
        }
        let m = once.take().unwrap_or(mode);
        let t = m.apply(b);
        if t != b {
            st.folds += 1;
        }
        match out.get_mut(st.written) {
            Some(d) => *d = t,
            None => return Err(SD_ESHORT),
        }
        st.written += 1;
        i += 1;
    }
    Ok(st)
}

/// s 命令：字面模式替换。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SCommand {
    pat: ByteList,
    tpl: ByteList,
    global: bool,
}

impl SCommand {
    /// 模式。
    pub fn pattern(&self) -> &ByteList {
        &self.pat
    }

    /// 模板。
    pub fn template(&self) -> &ByteList {
        &self.tpl
    }

    /// 是否全局。
    pub fn is_global(&self) -> bool {
        self.global
    }

    /// 在一行上执行替换。
    pub fn apply(&self, line: &[u8], out: &mut [u8]) -> Result<TplStats, u32> {
        if self.pat.is_empty() {
            return Err(SD_ELEN);
        }
        let mut st = TplStats { written: 0, folds: 0, matches: 0 };
        let mut i = 0usize;
        let mut hits = 0u32;
        while i < line.len() {
            let mut hit = false;
            if !(hits > 0 && !self.global) && i + self.pat.len() <= line.len() {
                let mut k = 0usize;
                let mut same = true;
                while k < self.pat.len() {
                    match (line.get(i + k), self.pat.at(k)) {
                        (Some(a), Some(b)) => {
                            if *a != b {
                                same = false;
                            }
                        }
                        _ => same = false,
                    }
                    k += 1;
                }
                hit = same;
            }
            if hit {
                let matched = line.get(i..i + self.pat.len()).unwrap_or(&[]);
                let t = expand_template(self.tpl.as_slice(), matched, {
                    let tail = match out.get_mut(st.written..) {
                        Some(v) => v,
                        None => return Err(SD_ESHORT),
                    };
                    tail
                })?;
                st.written += t.written;
                st.folds += t.folds;
                st.matches += 1;
                hits += 1;
                i += self.pat.len();
            } else {
                let b = match line.get(i) {
                    Some(v) => *v,
                    None => break,
                };
                match out.get_mut(st.written) {
                    Some(d) => *d = b,
                    None => return Err(SD_ESHORT),
                }
                st.written += 1;
                i += 1;
            }
        }
        Ok(st)
    }
}

/// 解析 s 命令：`s/pat/rep/[g]`。
pub fn parse_s(raw: &[u8]) -> Result<SCommand, u32> {
    if raw.len() < 4 {
        return Err(SD_ESHORT);
    }
    if raw.first().copied() != Some(b's') {
        return Err(SD_EINVAL);
    }
    let delim = match raw.get(1) {
        Some(v) => *v,
        None => return Err(SD_ESHORT),
    };
    if delim == b'\\' || delim == 0x0a || delim == b' ' || delim == 0 {
        return Err(SD_EINVAL);
    }
    let mut i = 2usize;
    let mut pat = ByteList::new();
    read_field(raw, &mut i, delim, &mut pat)?;
    let mut tpl = ByteList::new();
    let mut j = i;
    // 模板段允许 `\U` 等不被 read_escape 认识的字母，因此按原样收字节，
    // 只处理定界符转义与反斜杠转义，保留模板控制序列交给展开器。
    let mut closed = false;
    while j < raw.len() {
        let b = match raw.get(j) {
            Some(v) => *v,
            None => break,
        };
        if b == delim {
            j += 1;
            closed = true;
            break;
        }
        if b == b'\\' {
            let c = match raw.get(j + 1) {
                Some(v) => *v,
                None => return Err(SD_ESHORT),
            };
            if c == delim {
                tpl.push(delim)?;
                j += 2;
                continue;
            }
            tpl.push(b'\\')?;
            tpl.push(c)?;
            j += 2;
            continue;
        }
        tpl.push(b)?;
        j += 1;
    }
    if !closed {
        return Err(SD_ESHORT);
    }
    let mut global = false;
    while j < raw.len() {
        let c = match raw.get(j) {
            Some(v) => *v,
            None => break,
        };
        if c == b'g' {
            global = true;
        } else {
            return Err(SD_ECONF);
        }
        j += 1;
    }
    if pat.is_empty() {
        return Err(SD_ELEN);
    }
    let _ = i;
    Ok(SCommand { pat, tpl, global })
}

/// 流水线统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipeStats {
    /// 行数。
    pub lines: u32,
    /// 写出字节。
    pub written: usize,
    /// y 映射次数。
    pub mapped: u32,
    /// s 命中次数。
    pub matches: u32,
    /// 折叠次数。
    pub folds: u32,
}

impl PipeStats {
    /// 零。
    pub const fn zero() -> PipeStats {
        PipeStats { lines: 0, written: 0, mapped: 0, matches: 0, folds: 0 }
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.lines as u64);
        s.push(' ');
        push_dec(&mut s, self.written as u64);
        s.push(' ');
        push_dec(&mut s, self.mapped as u64);
        s.push(' ');
        push_dec(&mut s, self.matches as u64);
        s.push(' ');
        push_dec(&mut s, self.folds as u64);
        s
    }
}

/// 脚本：y 链 + 可选 s。
#[derive(Clone, Copy)]
pub struct Script {
    ys: [YCommand; CMD_CAP],
    ny: usize,
    s: Option<SCommand>,
    sealed: bool,
}

impl Script {
    /// 空脚本。
    pub fn new() -> Script {
        Script { ys: [YCommand::identity(); CMD_CAP], ny: 0, s: None, sealed: false }
    }

    /// 追加 y。
    pub fn push_y(&mut self, y: YCommand) -> Result<(), u32> {
        if self.sealed {
            return Err(SD_ESTATE);
        }
        if self.ny >= CMD_CAP {
            return Err(SD_ERANGE);
        }
        if let Some(d) = self.ys.get_mut(self.ny) {
            *d = y;
        }
        self.ny += 1;
        Ok(())
    }

    /// 设置 s。
    pub fn set_s(&mut self, s: SCommand) -> Result<(), u32> {
        if self.sealed {
            return Err(SD_ESTATE);
        }
        self.s = Some(s);
        Ok(())
    }

    /// y 条数。
    pub fn len(&self) -> usize {
        self.ny
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.ny == 0 && self.s.is_none()
    }

    /// 第 i 条 y。
    pub fn at(&self, i: usize) -> Option<YCommand> {
        if i >= self.ny {
            return None;
        }
        self.ys.get(i).copied()
    }

    /// s 命令。
    pub fn s_cmd(&self) -> Option<SCommand> {
        self.s
    }

    /// 封存。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 对整段文本执行。
    pub fn apply(&self, inp: &[u8], out: &mut [u8]) -> Result<PipeStats, u32> {
        if self.sealed {
            return Err(SD_ESTATE);
        }
        if inp.is_empty() {
            return Ok(PipeStats::zero());
        }
        if inp.len() > OUT_CAP {
            return Err(SD_ELEN);
        }
        let mut st = PipeStats::zero();
        let mut pos = 0usize;
        let mut line_start = 0usize;
        let mut tmp = [0u8; OUT_CAP];
        let mut tmp2 = [0u8; OUT_CAP];
        while line_start <= inp.len() {
            let mut end = line_start;
            while end < inp.len() {
                match inp.get(end) {
                    Some(v) => {
                        if *v == 0x0a {
                            break;
                        }
                    }
                    None => break,
                }
                end += 1;
            }
            let line = inp.get(line_start..end).unwrap_or(&[]);
            let mut cur = 0usize;
            let mut k = 0usize;
            while k < line.len() {
                if let Some(b) = line.get(k) {
                    if let Some(d) = tmp.get_mut(cur) {
                        *d = *b;
                    }
                }
                cur += 1;
                k += 1;
            }
            let mut len = cur;
            let mut j = 0usize;
            while j < self.ny {
                if let Some(y) = self.ys.get(j) {
                    let slice = tmp.get(..len).unwrap_or(&[]);
                    match y.apply(slice, &mut tmp2) {
                        Ok(s2) => {
                            st.mapped = st.mapped.saturating_add(s2.mapped);
                            len = s2.written;
                        }
                        Err(c) => return Err(c),
                    }
                    let mut m = 0usize;
                    while m < len {
                        if let (Some(d), Some(s2)) = (tmp.get_mut(m), tmp2.get(m)) {
                            *d = *s2;
                        }
                        m += 1;
                    }
                }
                j += 1;
            }
            if let Some(sc) = self.s {
                let slice = tmp.get(..len).unwrap_or(&[]);
                match sc.apply(slice, &mut tmp2) {
                    Ok(t) => {
                        st.matches = st.matches.saturating_add(t.matches);
                        st.folds = st.folds.saturating_add(t.folds);
                        len = t.written;
                    }
                    Err(c) => return Err(c),
                }
                let mut m = 0usize;
                while m < len {
                    if let (Some(d), Some(s2)) = (tmp.get_mut(m), tmp2.get(m)) {
                        *d = *s2;
                    }
                    m += 1;
                }
            }
            let mut m = 0usize;
            while m < len {
                if let Some(b) = tmp.get(m) {
                    match out.get_mut(pos) {
                        Some(d) => *d = *b,
                        None => return Err(SD_ESHORT),
                    }
                    pos += 1;
                }
                m += 1;
            }
            st.lines = st.lines.saturating_add(1);
            if end < inp.len() {
                match out.get_mut(pos) {
                    Some(d) => *d = 0x0a,
                    None => return Err(SD_ESHORT),
                }
                pos += 1;
                line_start = end + 1;
                if line_start == inp.len() {
                    // 尾随换行后不再产生空行。
                    break;
                }
            } else {
                break;
            }
        }
        st.written = pos;
        Ok(st)
    }
}

/// 行长统计（验证换行回写）。
pub fn split_lines(inp: &[u8]) -> (u32, u32) {
    let mut lines = 0u32;
    let mut trailing_nl = 0u32;
    let mut i = 0usize;
    while i < inp.len() {
        if let Some(b) = inp.get(i) {
            if *b == 0x0a {
                lines += 1;
                trailing_nl = 1;
            } else {
                trailing_nl = 0;
            }
        }
        i += 1;
    }
    // `lines` 是换行个数；总行数在无尾随换行时还要把最后一段算上。
    let total = if inp.is_empty() {
        0
    } else if trailing_nl == 0 {
        lines + 1
    } else {
        lines
    };
    (lines, total)
}

/// 对齐差值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delta {
    /// 字节差。
    pub bytes: i64,
    /// 行差。
    pub lines: i64,
}

impl Delta {
    /// 比对。
    pub fn of(a: &PipeStats, b: &PipeStats) -> Delta {
        Delta { bytes: a.written as i64 - b.written as i64, lines: a.lines as i64 - b.lines as i64 }
    }

    /// 是否零。
    pub fn zero(&self) -> bool {
        self.bytes == 0 && self.lines == 0
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::from("bytes");
        push_i64(&mut s, self.bytes);
        s.push_str(" lines");
        push_i64(&mut s, self.lines);
        s
    }
}

/// 观测格。
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

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SD_ESLOT);
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

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
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

    /// 对账：八成功值 + 六失败码。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SD_EBAL);
        }
        const VALS: [u32; 8] = [3, 6, 6, 1, 26, 4, 2, 8];
        const CODES: [u32; 6] =
            [SD_ELEN, SD_EBYTE, SD_ERANGE, SD_EDUP, SD_ESHORT, SD_ECONF];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SD_EBAL),
            };
            if s.slot as usize != i {
                return Err(SD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SD_EVALUE);
                        }
                    }
                    None => return Err(SD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SD_EVALUE);
                        }
                    }
                    None => return Err(SD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SD_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SD_EBAL);
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
            Err(SD_ESEAL)
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

/// 探针：解析后取折叠数。
fn probe_folds(raw: &[u8]) -> Result<u32, u32> {
    match parse_y(raw) {
        Ok(y) => Ok(y.folds() as u32),
        Err(c) => Err(c),
    }
}

/// 探针：解析后映射一段文本。
fn probe_mapped(raw: &[u8], inp: &[u8]) -> Result<u32, u32> {
    let y = parse_y(raw)?;
    let mut out = [0u8; OUT_CAP];
    match y.apply(inp, &mut out) {
        Ok(st) => Ok(st.mapped),
        Err(c) => Err(c),
    }
}

/// 探针：解析后写出长度。
fn probe_written(raw: &[u8], inp: &[u8]) -> Result<usize, u32> {
    let y = parse_y(raw)?;
    let mut out = [0u8; OUT_CAP];
    match y.apply(inp, &mut out) {
        Ok(st) => Ok(st.written),
        Err(c) => Err(c),
    }
}

/// 探针：模板展开后的长度。
fn probe_tpl(raw: &[u8], matched: &[u8]) -> Result<usize, u32> {
    let mut out = [0u8; OUT_CAP];
    match expand_template(raw, matched, &mut out) {
        Ok(t) => Ok(t.written),
        Err(c) => Err(c),
    }
}

/// 探针：一段文本经脚本处理后的行数。
fn probe_lines(inp: &[u8]) -> Result<u32, u32> {
    let mut sc = Script::new();
    let y = parse_y(b"y/a/b/")?;
    sc.push_y(y)?;
    let mut out = [0u8; OUT_CAP];
    match sc.apply(inp, &mut out) {
        Ok(st) => Ok(st.lines),
        Err(c) => Err(c),
    }
}

/// 高位锚：0x80..=0xff 在两种模式下都原样。
pub fn case_anchor_ok() -> bool {
    let mut i = 0u32;
    while i < 256 {
        let b = i as u8;
        if b >= 0x80 {
            if CaseMode::Upper.apply(b) != b || CaseMode::Lower.apply(b) != b {
                return false;
            }
        }
        if b >= b'a' && b <= b'z' && CaseMode::Upper.apply(b) != b.wrapping_sub(0x20) {
            return false;
        }
        if b >= b'A' && b <= b'Z' && CaseMode::Lower.apply(b) != b.wrapping_add(0x20) {
            return false;
        }
        i += 1;
    }
    true
}

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_YPARSE, probe_folds(b"y/abc/ABC/")));

    let _ = l.register(word_cell(1, OP_YAPPLY, probe_mapped(b"y/abc/ABC/", b"aabbcc")));

    let _ = l.register(count_cell(2, OP_YAPPLY, probe_written(b"y/abc/ABC/", b"aabbcc")));

    let _ = l.register(word_cell(3, OP_CASE, Ok(if case_anchor_ok() { 1 } else { 0 })));

    let _ = l.register(word_cell(4, OP_CASE, Ok(case_pairs())));

    let _ = l.register(count_cell(5, OP_TPL, probe_tpl(b"\\Uab\\Ecd", b"")));

    let _ = l.register(word_cell(6, OP_LINE, probe_lines(b"a\nb\n")));

    let _ = l.register(word_cell(7, OP_ESC, Ok(ESCAPES.len() as u32)));

    let _ = l.register(word_cell(8, OP_LEN, probe_folds(b"y/ab/c/")));

    let _ = l.register(word_cell(9, OP_ESC, probe_folds(b"y/a/\\q/")));

    let _ = l.register(word_cell(10, OP_ESC, probe_folds(b"y/a/\\400/")));

    let _ = l.register(word_cell(11, OP_DUP, probe_folds(b"y/aa/bb/")));

    let _ = l.register(word_cell(12, OP_YPARSE, probe_folds(b"y/abc")));

    let _ = l.register(word_cell(13, OP_YPARSE, probe_folds(b"y/a/b/x")));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = {
        let y = parse_y(b"y/abc/ABC/");
        match y {
            Ok(cmd) => {
                let mut buf = [0u8; 8];
                match cmd.apply(b"cab", &mut buf) {
                    Ok(st) => {
                        st.written == 3
                            && st.mapped == 3
                            && cmd.folds() == 3
                            && buf.get(0) == Some(&b'C')
                            && buf.get(1) == Some(&b'A')
                            && buf.get(2) == Some(&b'B')
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    };

    out[1] = {
        let y = match parse_y(b"y/abc/ABC/") {
            Ok(v) => v,
            Err(_) => YCommand::identity(),
        };
        let m = y.mapping();
        let mut ok = true;
        let mut i = 0u32;
        while i < 256 {
            let b = i as u8;
            let want = match b {
                b'a' => b'A',
                b'b' => b'B',
                b'c' => b'C',
                _ => b,
            };
            if m.get(i as usize) != Some(&want) {
                ok = false;
            }
            let one = [b];
            let mut buf = [0u8; 1];
            match y.apply(&one, &mut buf) {
                Ok(st) => {
                    if buf.first() != Some(&want) || st.mapped != if want == b { 0 } else { 1 } {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            i += 1;
        }
        let inv = y.inverse();
        let round = match inv {
            Ok(z) => {
                let mut buf = [0u8; 8];
                match z.apply(b"ABC", &mut buf) {
                    Ok(_) => buf.get(..3) == Some(b"abc".as_slice()),
                    Err(_) => false,
                }
            }
            Err(_) => false,
        };
        ok && round
    };

    out[2] = {
        let mut l = build_ledger();
        let audited = l.audit().is_ok();
        let sealed = l.seal();
        let replayed = match l.replay() {
            Ok(v) => v == sealed,
            Err(_) => false,
        };
        let frozen = l.freeze().is_ok();
        audited && replayed && frozen && l.is_frozen() && l.rejects() == 0
    };

    out[3] = {
        let mut buf = [0u8; 32];
        let a = match expand_template(b"\\U&\\E", b"hi", &mut buf) {
            Ok(t) => t.written == 2 && t.matches == 1 && buf.get(0) == Some(&b'H'),
            Err(_) => false,
        };
        let b = expand_template(b"\\U\\xe9", b"", &mut buf) == Ok(TplStats { written: 1, folds: 0, matches: 0 })
            && buf.get(0) == Some(&0xe9);
        let c = match parse_s(b"s/a/X/g") {
            Ok(sc) => match sc.apply(b"abcabc", &mut buf) {
                Ok(t) => t.matches == 2 && buf.get(..6) == Some(b"XbcXbc".as_slice()),
                Err(_) => false,
            },
            Err(_) => false,
        };
        let d = {
            let mut sc = Script::new();
            let y1 = parse_y(b"y/abc/ABC/");
            let y2 = parse_y(b"y/ABC/xyz/");
            match (y1, y2) {
                (Ok(p), Ok(q)) => {
                    let p1 = sc.push_y(p);
                    let p2 = sc.push_y(q);
                    match sc.apply(b"abcabc", &mut buf) {
                        Ok(st) => {
                            p1.is_ok()
                                && p2.is_ok()
                                && st.written == 6
                                && buf.get(..6) == Some(b"xyzxyz".as_slice())
                        }
                        Err(_) => false,
                    }
                }
                _ => false,
            }
        };
        a && b && c && d
    };

    out
}

/// 判据集合。
pub fn run_unx11569_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11569.sed");

    s.add(
        "SD01-y 解析",
        {
            let a = match parse_y(b"y/abc/ABC/") {
                Ok(y) => {
                    y.source().len() == 3
                        && y.target().len() == 3
                        && y.folds() == 3
                        && y.delim() == b'/'
                        && !y.is_identity()
                }
                Err(_) => false,
            };
            let b = match parse_y(b"y|abc|ABC|") {
                Ok(y) => y.delim() == b'|' && y.folds() == 3,
                Err(_) => false,
            };
            let c = match parse_y(b"y#012#789#") {
                Ok(y) => y.source().len() == 3 && y.folds() == 3,
                Err(_) => false,
            };
            let d = match parse_y(b"y/a/a/") {
                Ok(y) => y.is_identity() && y.folds() == 0,
                Err(_) => false,
            };
            let e = match parse_y(b"y/\\n\\t/\\t\\n/") {
                Ok(y) => {
                    y.source().at(0) == Some(0x0a)
                        && y.source().at(1) == Some(0x09)
                        && y.target().at(0) == Some(0x09)
                        && y.target().at(1) == Some(0x0a)
                }
                Err(_) => false,
            };
            let f = match parse_y(b"y/abc/ABC/") {
                Ok(y) => y.plan() == String::from("y src=3 dst=3 folds=3"),
                Err(_) => false,
            };
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "SD02-长度闸门",
        {
            let a = parse_y(b"y/ab/c/") == Err(SD_ELEN);
            let b = parse_y(b"y//x/") == Err(SD_ELEN);
            let c = parse_y(b"y///") == Err(SD_ELEN);
            let d = parse_y(b"y/a/ab/") == Err(SD_ELEN);
            let e = parse_y(b"y/abc/ABCD/") == Err(SD_ELEN);
            let f = match parse_y(b"y/abcd/ABCD/") {
                Ok(y) => y.source().len() == 4 && y.folds() == 4,
                Err(_) => false,
            };
            let g = parse_y(b"x/ab/cd/") == Err(SD_EINVAL);
            let h = parse_y(b"y ab cd ") == Err(SD_EINVAL);
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "SD03-逐字节映射",
        {
            let y = match parse_y(b"y/abc/ABC/") {
                Ok(v) => v,
                Err(_) => YCommand::identity(),
            };
            let a = {
                let mut buf = [0u8; 16];
                match y.apply(b"aabbcc", &mut buf) {
                    Ok(st) => {
                        st.mapped == 6 && st.written == 6 && buf.get(..6) == Some(b"AABBCC".as_slice())
                    }
                    Err(_) => false,
                }
            };
            let b = {
                let mut all = [0u8; 256];
                let mut i = 0usize;
                while i < 256 {
                    all[i] = i as u8;
                    i += 1;
                }
                let mut out = [0u8; 256];
                match y.apply(&all, &mut out) {
                    Ok(st) => {
                        let mut ok = st.mapped == 3 && st.written == 256;
                        let mut k = 0usize;
                        while k < 256 {
                            let want = match k as u8 {
                                b'a' => b'A',
                                b'b' => b'B',
                                b'c' => b'C',
                                o => o,
                            };
                            if out.get(k) != Some(&want) {
                                ok = false;
                            }
                            k += 1;
                        }
                        ok
                    }
                    Err(_) => false,
                }
            };
            let c = {
                let m = y.mapping();
                m.get(0x61) == Some(&b'A') && m.get(0x62) == Some(&b'B') && m.get(0x20) == Some(&0x20)
            };
            let d = {
                let mut buf = [0u8; 16];
                match y.apply(b"cabbage", &mut buf) {
                    Ok(st) => st.mapped == 5 && buf.get(..7) == Some(b"CABBAge".as_slice()),
                    Err(_) => false,
                }
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "SD04-逆与双射",
        {
            let y = match parse_y(b"y/abc/ABC/") {
                Ok(v) => v,
                Err(_) => YCommand::identity(),
            };
            let a = match y.inverse() {
                Ok(z) => {
                    let mut buf = [0u8; 8];
                    match z.apply(b"ABC", &mut buf) {
                        Ok(_) => buf.get(..3) == Some(b"abc".as_slice()),
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            };
            let b = match parse_y(b"y/ab/ba/") {
                Ok(v) => match v.inverse() {
                    Ok(z) => {
                        let mut buf = [0u8; 4];
                        match z.apply(b"ab", &mut buf) {
                            Ok(_) => buf.get(..2) == Some(b"ba".as_slice()),
                            Err(_) => false,
                        }
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            };
            let c = match parse_y(b"y/ab/aa/") {
                Ok(v) => v.inverse() == Err(SD_EDUP),
                Err(_) => false,
            };
            let d = match parse_y(b"y/ab/xy/") {
                Ok(v) => match (v.inverse(), v.inverse()) {
                    (Ok(u), Ok(w)) => u == w,
                    _ => false,
                },
                Err(_) => false,
            };
            let e = match (parse_y(b"y/abc/ABC/"), parse_y(b"y/ABC/abc/")) {
                (Ok(u), Ok(w)) => u.folds() == w.folds() && u.mapping() != w.mapping(),
                _ => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "SD05-转义表",
        {
            let a = match parse_y(b"y/\\n/\\t/") {
                Ok(y) => {
                    let mut buf = [0u8; 4];
                    match y.apply(b"a\nb", &mut buf) {
                        Ok(_) => buf.get(1) == Some(&0x09),
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            };
            let b = match parse_y(b"y/\\101/\\141/") {
                Ok(y) => {
                    let mut buf = [0u8; 4];
                    match y.apply(b"AB", &mut buf) {
                        Ok(_) => buf.get(0) == Some(&b'a'),
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            };
            let c = match parse_y(b"y/\\x41/\\x61/") {
                Ok(y) => y.source().at(0) == Some(b'A') && y.target().at(0) == Some(b'a'),
                Err(_) => false,
            };
            let d = match parse_y(b"y/a\\/b/x\\/y/") {
                Ok(y) => y.source().len() == 3 && y.source().at(1) == Some(b'/'),
                Err(_) => false,
            };
            let e = match parse_y(b"y/\\&/\\t/") {
                Ok(y) => y.source().at(0) == Some(b'&'),
                Err(_) => false,
            };
            let f = parse_y(b"y/a/\\q/") == Err(SD_EBYTE);
            let g = parse_y(b"y/a/\\x/") == Err(SD_EBYTE);
            let h = parse_y(b"y/a/\\400/") == Err(SD_ERANGE);
            let i = match parse_y(b"y/\\x41\\x42/ab/") {
                Ok(y) => {
                    y.source().len() == 2
                        && y.source().at(0) == Some(b'A')
                        && y.source().at(1) == Some(b'B')
                }
                Err(_) => false,
            };
            let j = parse_y(b"y/a/\\") == Err(SD_ESHORT);
            let k = parse_y(b"y/\\") == Err(SD_ESHORT);
            let l = parse_y(b"y/a/\\777/") == Err(SD_ERANGE);
            let m = escape_table_text().contains("n 0a");
            a && b && c && d && e && f && g && h && i && j && k && l && m
        },
        "static",
    );

    s.add(
        "SD06-脚本链",
        {
            let mut sc = Script::new();
            let y1 = parse_y(b"y/abc/ABC/");
            let y2 = parse_y(b"y/ABC/xyz/");
            let a = match (y1, y2) {
                (Ok(p), Ok(q)) => sc.push_y(p).is_ok() && sc.push_y(q).is_ok() && sc.len() == 2,
                _ => false,
            };
            let mut buf = [0u8; 16];
            let b = match sc.apply(b"abcabc", &mut buf) {
                Ok(st) => {
                    st.written == 6
                        && st.mapped == 12
                        && st.lines == 1
                        && buf.get(..6) == Some(b"xyzxyz".as_slice())
                }
                Err(_) => false,
            };
            let c = match sc.at(0) {
                Some(v) => v.folds() == 3,
                None => false,
            };
            let d = sc.at(2).is_none() && !sc.is_empty();
            let mut sc2 = Script::new();
            let s1 = parse_s(b"s/b/B/");
            let e = match s1 {
                Ok(v) => {
                    let set = sc2.set_s(v);
                    let mut b2 = [0u8; 16];
                    match sc2.apply(b"abcabc", &mut b2) {
                        Ok(st) => {
                            set.is_ok()
                                && st.matches == 1
                                && b2.get(..6) == Some(b"aBcabc".as_slice())
                        }
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            };
            let mut sc3 = Script::new();
            let f = match (parse_y(b"y/a/X/"), parse_s(b"s/X/y/g")) {
                (Ok(y), Ok(v)) => {
                    let p1 = sc3.push_y(y);
                    let p2 = sc3.set_s(v);
                    let mut b3 = [0u8; 16];
                    match sc3.apply(b"aaa", &mut b3) {
                        Ok(st) => {
                            p1.is_ok()
                                && p2.is_ok()
                                && st.mapped == 3
                                && st.matches == 3
                                && b3.get(..3) == Some(b"yyy".as_slice())
                        }
                        Err(_) => false,
                    }
                }
                _ => false,
            };
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "SD07-行处理",
        {
            let mut sc = Script::new();
            let y = match parse_y(b"y/a/b/") {
                Ok(v) => v,
                Err(_) => YCommand::identity(),
            };
            let _ = sc.push_y(y);
            let a = match sc.apply(b"a\nb\n", &mut [0u8; 16]) {
                Ok(st) => st.lines == 2 && st.written == 4,
                Err(_) => false,
            };
            let b = match sc.apply(b"a\nb", &mut [0u8; 16]) {
                Ok(st) => st.lines == 2 && st.written == 3,
                Err(_) => false,
            };
            let c = match sc.apply(b"\n\n", &mut [0u8; 16]) {
                Ok(st) => st.lines == 2 && st.written == 2,
                Err(_) => false,
            };
            let d = match sc.apply(b"", &mut [0u8; 16]) {
                Ok(st) => st.lines == 0 && st.written == 0,
                Err(_) => false,
            };
            let e = match sc.apply(b"a", &mut [0u8; 16]) {
                Ok(st) => st.lines == 1 && st.written == 1,
                Err(_) => false,
            };
            let f = match sc.apply(b"a\nb", &mut [0u8; 16]) {
                Ok(st) => st.written == 3,
                Err(_) => false,
            };
            let (nl, total) = split_lines(b"a\nb\n");
            let g = nl == 2 && total == 2;
            let (nl2, total2) = split_lines(b"a\nb");
            let h = nl2 == 1 && total2 == 2;
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "SD08-模板展开",
        {
            let mut buf = [0u8; 32];
            let a = match expand_template(b"\\Uab\\Ecd", b"", &mut buf) {
                Ok(t) => {
                    t.written == 4 && t.folds == 2 && buf.get(..4) == Some(b"ABcd".as_slice())
                }
                Err(_) => false,
            };
            let b = match expand_template(b"\\LAB", b"", &mut buf) {
                Ok(t) => t.written == 2 && t.folds == 2 && buf.get(..2) == Some(b"ab".as_slice()),
                Err(_) => false,
            };
            let c = match expand_template(b"\\uab", b"", &mut buf) {
                Ok(t) => t.folds == 1 && buf.get(..2) == Some(b"Ab".as_slice()),
                Err(_) => false,
            };
            let d = match expand_template(b"\\lAB", b"", &mut buf) {
                Ok(t) => t.folds == 1 && buf.get(..2) == Some(b"aB".as_slice()),
                Err(_) => false,
            };
            let e = match expand_template(b"[&]", b"hi", &mut buf) {
                Ok(t) => t.matches == 1 && buf.get(..4) == Some(b"[hi]".as_slice()),
                Err(_) => false,
            };
            let f = match expand_template(b"\\U&", b"hi", &mut buf) {
                Ok(t) => t.matches == 1 && t.folds == 2 && buf.get(..2) == Some(b"HI".as_slice()),
                Err(_) => false,
            };
            let g = match expand_template(b"\\&", b"hi", &mut buf) {
                Ok(t) => t.matches == 0 && t.written == 1 && buf.get(0) == Some(&b'&'),
                Err(_) => false,
            };
            let h = match expand_template(b"\\1", b"", &mut buf) {
                Ok(_) => false,
                Err(c) => c == SD_EBYTE,
            };
            let i = match expand_template(b"\\", b"", &mut buf) {
                Ok(_) => false,
                Err(c) => c == SD_ESHORT,
            };
            let j = match expand_template(b"abcdef", b"", &mut [0u8; 3]) {
                Ok(_) => false,
                Err(c) => c == SD_ESHORT,
            };
            a && b && c && d && e && f && g && h && i && j
        },
        "static",
    );

    s.add(
        "SD09-大小写锚",
        {
            let mut ok = case_anchor_ok();
            let mut i = 0u32;
            while i < 256 {
                let b = i as u8;
                if b >= 0x80 {
                    if CaseMode::Upper.apply(b) != b || CaseMode::Lower.apply(b) != b {
                        ok = false;
                    }
                }
                if b >= b'a' && b <= b'z' && CaseMode::Upper.apply(b) != b.wrapping_sub(0x20) {
                    ok = false;
                }
                if b >= b'A' && b <= b'Z' && CaseMode::Lower.apply(b) != b.wrapping_add(0x20) {
                    ok = false;
                }
                if b >= b'0' && b <= b'9'
                    && (CaseMode::Upper.apply(b) != b || CaseMode::Lower.apply(b) != b)
                {
                    ok = false;
                }
                i += 1;
            }
            let a = case_pairs() == 26 && ok;
            let b = CaseMode::Keep.apply(b'a') == b'a' && CaseMode::Upper.apply(b'a') == b'A';
            let c = CaseMode::Upper.apply(0xe9) == 0xe9 && CaseMode::Upper.apply(0xc3) == 0xc3;
            let d = toupper_c(0xff) == 0xff && tolower_c(0x80) == 0x80;
            let e = CaseMode::Upper.name() == "upper" && CaseMode::Keep.name() == "keep";
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "SD10-渲染总表",
        {
            let y = match parse_y(b"y/abc/ABC/") {
                Ok(v) => v,
                Err(_) => YCommand::identity(),
            };
            let text = y.render();
            let a = text.matches('>').count() == 3;
            let b = text.contains("61>41") && text.contains("62>42") && text.contains("63>43");
            let c = text.contains("folds 3") && text.contains("delim 2f");
            let d = y.plan() == String::from("y src=3 dst=3 folds=3");
            let e = match y.apply(b"aabbcc", &mut [0u8; 16]) {
                Ok(st) => st.text() == String::from("6/6/6"),
                Err(_) => false,
            };
            let f = match parse_s(b"s/ab/X/g") {
                Ok(sc) => {
                    sc.is_global()
                        && sc.pattern().as_slice() == b"ab"
                        && sc.template().as_slice() == b"X"
                }
                Err(_) => false,
            };
            let g = match parse_s(b"s/a/b/x") {
                Ok(_) => false,
                Err(c) => c == SD_ECONF,
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "SD11-容量与封存",
        {
            let mut sc = Script::new();
            let mut n = 0usize;
            let mut last = Ok(());
            while n < CMD_CAP + 1 {
                let y = match parse_y(b"y/a/b/") {
                    Ok(v) => v,
                    Err(_) => YCommand::identity(),
                };
                last = sc.push_y(y);
                n += 1;
            }
            let a = last == Err(SD_ERANGE) && sc.len() == CMD_CAP;
            sc.seal();
            let b = sc.is_sealed();
            let c = sc.push_y(YCommand::identity()) == Err(SD_ESTATE);
            let d = sc.apply(b"a", &mut [0u8; 8]) == Err(SD_ESTATE);
            let sc2 = Script::new();
            let e = match sc2.apply(&[b'a'; OUT_CAP + 1], &mut [0u8; OUT_CAP]) {
                Ok(_) => false,
                Err(c) => c == SD_ELEN,
            };
            let f = match sc2.apply(b"", &mut [0u8; 8]) {
                Ok(st) => st.written == 0 && st.lines == 0,
                Err(_) => false,
            };
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "SD12-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES;
            let b = l.ok_count() == 8 && l.err_count() == 6;
            let c = l.audit().is_ok();
            let d = match l.sample(0) {
                Some(v) => v.ok && v.value == 3 && v.op == OP_YPARSE,
                None => false,
            };
            let e = match l.sample(5) {
                Some(v) => v.ok && v.value == 4,
                None => false,
            };
            let f = match l.sample(13) {
                Some(v) => !v.ok && v.code == SD_ECONF,
                None => false,
            };
            let g = l.replay() == Ok(l.seal());
            let h = l.sample(SAMPLES).is_none() && !l.is_empty();
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "SD13-冻结重放四注入",
        {
            let inj = inject_four();
            let mut ok = inj[0] && inj[1] && inj[2] && inj[3];
            let mut l = build_ledger();
            let sealed = l.seal();
            let replay_ok = l.replay() == Ok(sealed);
            let frozen = l.freeze() == Ok(sealed);
            let late = l.register(Sample { slot: 13, op: OP_LINE, ok: true, code: 0, value: 1 });
            let rej1 = l.rejects();
            let again = l.freeze();
            let mut sc = Script::new();
            let push = match parse_y(b"y/a/b/") {
                Ok(y) => sc.push_y(y),
                Err(_) => Err(SD_EINVAL),
            };
            sc.seal();
            let state = sc.apply(b"a", &mut [0u8; 8]) == Err(SD_ESTATE);
            ok = ok
                && replay_ok
                && frozen
                && late == Err(SD_ESLOT)
                && rej1 == 1
                && again == Err(SD_EBAL)
                && l.rejects() == 2
                && push.is_ok()
                && state;
            ok
        },
        "static",
    );

    s
}

/// 转义表文本。
pub fn escape_table_text() -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < ESCAPES.len() {
        if let Some(c) = ESCAPES.get(i) {
            newline(&mut s);
            s.push(*c as char);
            s.push(' ');
            let v = match *c {
                b'n' => 0x0a,
                b't' => 0x09,
                b'r' => 0x0d,
                b'f' => 0x0c,
                b'v' => 0x0b,
                b'a' => 0x07,
                b'b' => 0x08,
                _ => 0x5c,
            };
            push_hex2(&mut s, v);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let y = parse_y(b"y/abc/ABC/").expect("y");
        assert_eq!(y.source().len(), 3);
        assert_eq!(y.target().len(), 3);
        assert_eq!(y.folds(), 3);
        assert_eq!(y.delim(), b'/');
        assert!(!y.is_identity());
    }

    #[test]
    fn parse_errors() {
        assert_eq!(parse_y(b"y/ab/c/"), Err(SD_ELEN));
        assert_eq!(parse_y(b"y//x/"), Err(SD_ELEN));
        assert_eq!(parse_y(b"y/abc"), Err(SD_ESHORT));
        assert_eq!(parse_y(b"y/a/\\q/"), Err(SD_EBYTE));
        assert_eq!(parse_y(b"y/a/\\400/"), Err(SD_ERANGE));
        assert_eq!(parse_y(b"y/aa/bb/"), Err(SD_EDUP));
        assert_eq!(parse_y(b"y/a/b/x"), Err(SD_ECONF));
    }

    #[test]
    fn apply_positions() {
        let y = parse_y(b"y/abc/ABC/").expect("y");
        let mut buf = [0u8; 16];
        let st = y.apply(b"cabbage", &mut buf).expect("apply");
        assert_eq!(st.mapped, 5);
        assert_eq!(&buf[..7], b"CABBAge");
    }

    #[test]
    fn inverse_bijection() {
        let y = parse_y(b"y/abc/ABC/").expect("y");
        let z = y.inverse().expect("inv");
        let mut buf = [0u8; 8];
        z.apply(b"ABC", &mut buf).expect("apply");
        assert_eq!(&buf[..3], b"abc");
        let bad = parse_y(b"y/ab/aa/").expect("y");
        assert_eq!(bad.inverse(), Err(SD_EDUP));
    }

    #[test]
    fn escapes() {
        let y = parse_y(b"y/\\n/\\t/").expect("y");
        let mut buf = [0u8; 4];
        y.apply(b"a\nb", &mut buf).expect("apply");
        assert_eq!(buf[1], 0x09);
        let a = parse_y(b"y/\\101/\\141/").expect("oct");
        assert_eq!(a.source().at(0), Some(b'A'));
    }

    #[test]
    fn template_modes() {
        let mut buf = [0u8; 32];
        let t = expand_template(b"\\Uab\\Ecd", b"", &mut buf).expect("tpl");
        assert_eq!(t.written, 4);
        assert_eq!(&buf[..4], b"ABcd");
        let u = expand_template(b"\\uab", b"", &mut buf).expect("tpl");
        assert_eq!(u.folds, 1);
        assert_eq!(&buf[..2], b"Ab");
    }

    #[test]
    fn template_whole_match() {
        let mut buf = [0u8; 32];
        let t = expand_template(b"\\U&", b"hi", &mut buf).expect("tpl");
        assert_eq!(t.matches, 1);
        assert_eq!(t.folds, 2);
        assert_eq!(&buf[..2], b"HI");
    }

    #[test]
    fn template_errors() {
        let mut buf = [0u8; 8];
        assert_eq!(expand_template(b"\\1", b"", &mut buf), Err(SD_EBYTE));
        assert_eq!(expand_template(b"\\", b"", &mut buf), Err(SD_ESHORT));
        assert_eq!(expand_template(b"abcdef", b"", &mut [0u8; 2]), Err(SD_ESHORT));
    }

    #[test]
    fn s_command_literal() {
        let sc = parse_s(b"s/ab/X/g").expect("s");
        let mut buf = [0u8; 16];
        let t = sc.apply(b"abab", &mut buf).expect("apply");
        assert_eq!(t.matches, 2);
        assert_eq!(&buf[..2], b"XX");
        let one = parse_s(b"s/ab/X/").expect("s");
        let t2 = one.apply(b"abab", &mut buf).expect("apply");
        assert_eq!(t2.matches, 1);
        assert_eq!(&buf[..3], b"Xab");
    }

    #[test]
    fn script_chain() {
        let mut sc = Script::new();
        sc.push_y(parse_y(b"y/abc/ABC/").expect("a")).expect("push");
        sc.push_y(parse_y(b"y/ABC/xyz/").expect("b")).expect("push");
        let mut buf = [0u8; 16];
        let st = sc.apply(b"abcabc", &mut buf).expect("apply");
        assert_eq!(st.written, 6);
        assert_eq!(st.mapped, 12);
        assert_eq!(&buf[..6], b"xyzxyz");
    }

    #[test]
    fn line_accounting() {
        let mut sc = Script::new();
        sc.push_y(parse_y(b"y/a/b/").expect("y")).expect("push");
        let st = sc.apply(b"a\nb\n", &mut [0u8; 16]).expect("apply");
        assert_eq!(st.lines, 2);
        assert_eq!(st.written, 4);
        let empty = sc.apply(b"", &mut [0u8; 16]).expect("apply");
        assert_eq!(empty.lines, 0);
    }

    #[test]
    fn capacity_and_state() {
        let mut sc = Script::new();
        let mut i = 0usize;
        while i < CMD_CAP {
            sc.push_y(parse_y(b"y/a/b/").expect("y")).expect("push");
            i += 1;
        }
        assert_eq!(sc.push_y(YCommand::identity()), Err(SD_ERANGE));
        sc.seal();
        assert!(sc.is_sealed());
        assert_eq!(sc.apply(b"a", &mut [0u8; 8]), Err(SD_ESTATE));
    }

    #[test]
    fn anchor_sweep() {
        assert!(case_anchor_ok());
        assert_eq!(case_pairs(), 26);
        let mut i = 0u32;
        while i < 256 {
            let b = i as u8;
            if b >= 0x80 {
                assert_eq!(CaseMode::Upper.apply(b), b);
                assert_eq!(CaseMode::Lower.apply(b), b);
            }
            i += 1;
        }
    }

    #[test]
    fn ledger_balances() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(6).map(|v| v.value), Some(2));
        assert_eq!(l.sample(12).map(|v| v.code), Some(SD_ESHORT));
    }

    #[test]
    fn seal_freeze_replay() {
        let mut l = build_ledger();
        let sealed = l.seal();
        assert_eq!(l.replay().expect("replay"), sealed);
        assert_eq!(l.freeze().expect("freeze"), sealed);
        assert!(l.is_frozen());
        let late = Sample { slot: 13, op: OP_LINE, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(late), Err(SD_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze(), Err(SD_EBAL));
    }

    #[test]
    fn injection_four() {
        let inj = inject_four();
        assert!(inj[0]);
        assert!(inj[1]);
        assert!(inj[2]);
        assert!(inj[3]);
    }

    #[test]
    fn checks_all_pass() {
        assert!(run_unx11569_checks().all_passed());
    }
}

