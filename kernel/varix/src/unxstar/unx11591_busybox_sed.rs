//! UNX-F11591 busybox sed 子集差异判据
//!
//! busybox 的 `sed` 是“够用就好”的子集实现：命令集只有六个（`s`、`y`、`d`、
//! `p`、`n`、`q`），地址只支持单地址与步长地址，标志位只支持 `g`、`p`、`I`
//! 与数字第 n 次。本模块把这三层分别建模，并把“与 GNU 的差异”单独列成一张
//! 十一行的对照表，避免把“busybox 支持”和“GNU 支持”混为一谈。
//!
//! **命令基线表**：六条命令逐行标注 `FULL`（完整）、`PARTIAL`（有差异）、
//! `GNU`（仅 GNU 有）。`n` 与 `q` 是 PARTIAL：前者在自动打印时序上与 GNU
//! 有细微差别，后者在退出码参数上不完整。
//!
//! **差异对照表**：`s///g`、`s///p`、`s///I`、`s///N`、`y///`、`q 退出码`、
//! `-i 原地`、`-E 扩展正则`、`0,/re/ 地址`、`w 文件`、`e 执行` 十一行。
//! 前八行里 busybox 覆盖七行完整、一行部分；后两行是 GNU 独占。
//!
//! **解析器**：地址前缀支持五种——无地址、行号 `N`、末行 `$`、正则 `/re/`、
//! 步长 `N~M`；范围地址 `a,b` 在本子集里直接判“不支持”（`EUNSUP`），而不是
//! 悄悄只取左端。命令体按分隔符扫描，分隔符可以任意非字母数字字符；反斜杠
//! 转义分隔符。`s` 的三段（模式、替换、标志）与 `y` 的两段（源表、目标表）
//! 都必须闭合，未闭合分别判模式/替换错误。
//!
//! 替换引擎是字面量匹配（大小写可折叠），支持替换串里的 `&`（整段匹配）、
//! `\&`（字面 &）、`\\`（字面反斜杠）与 `\n`（换行）。第 n 次与全局互斥地
//! 决定选中哪些命中：全局时全部替换，否则只替换第 n 次（n 为 0 表示第一次）。
//! 命中数为 0 时原样返回，计数为 0。
//!
//! 执行模型按行推进，六条命令的语义：
//! - `s` 改写模式空间，`p` 标志额外打印一次；
//! - `y` 逐字符查表替换，源表与目标表长度必须相等；
//! - `d` 删除当前行并结束本轮；
//! - `p` 立即打印模式空间；
//! - `n` 打印后提前进入下一行（跳过余下命令）；
//! - `q` 结束脚本（仍受自动打印控制）。
//!
//! 闸门次序：`parse`：地址 → 命令字母 → 命令体 → 标志；`substitute`：空模式 →
//! 扫描命中 → 选中判定 → 替换串展开；`run`：地址匹配 → 命令分派 → 自动打印 →
//! 停止位。
//!
//! 账本十四格：八格成功（三条例程解析、两次替换计数、一次转写、一次脚本执行、
//! 一次差异计数），六格失败（未知命令、未知标志、空模式、替换未闭合、范围地址、
//! 行号溢出）。冻结票号折叠十四格与拒绝计数。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfa40;

/// 未知命令字母。
pub const SD_ECMD: u32 = SEG_BASE + 0x01;
/// 地址非法或缺失。
pub const SD_EADDR: u32 = SEG_BASE + 0x02;
/// 标志非法或重复承载。
pub const SD_EFLAG: u32 = SEG_BASE + 0x03;
/// 模式为空或分隔符非法。
pub const SD_EPAT: u32 = SEG_BASE + 0x04;
/// 替换段/目标表未闭合或长度不符。
pub const SD_EREP: u32 = SEG_BASE + 0x05;
/// 行号、步长或第 n 次越界。
pub const SD_ERANGE: u32 = SEG_BASE + 0x06;
/// 子集不支持该形态（范围地址、w、e）。
pub const SD_EUNSUP: u32 = SEG_BASE + 0x07;
/// 行输入为空或行号为零。
pub const SD_ELINE: u32 = SEG_BASE + 0x08;
/// 账本形状不符（14/8/6）。
pub const SD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序或冻结后登记。
pub const SD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 复算值与登记值不符。
pub const SD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放票号不一致。
pub const SD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const SD_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;
/// 命令条数。
pub const CMDS: usize = 6;
/// 差异对照表行数。
pub const DIFFS: usize = 11;

/// 命令：替换。
pub const SD_S: u8 = 0;
/// 命令：转写。
pub const SD_Y: u8 = 1;
/// 命令：删除。
pub const SD_D: u8 = 2;
/// 命令：打印。
pub const SD_P: u8 = 3;
/// 命令：下一行。
pub const SD_N: u8 = 4;
/// 命令：退出。
pub const SD_Q: u8 = 5;

/// 支持级别：完整。
pub const SUP_FULL: u8 = 0;
/// 支持级别：有差异。
pub const SUP_PARTIAL: u8 = 1;
/// 支持级别：仅 GNU。
pub const SUP_GNU: u8 = 2;

/// 地址：无。
pub const ADDR_NONE: u8 = 0;
/// 地址：行号。
pub const ADDR_LINE: u8 = 1;
/// 地址：末行 `$`。
pub const ADDR_LAST: u8 = 2;
/// 地址：正则 `/re/`（本模型退化为字面量包含）。
pub const ADDR_RE: u8 = 3;
/// 地址：步长 `N~M`。
pub const ADDR_STEP: u8 = 4;

/// 操作码：解析。
pub const OP_PARSE: u8 = 1;
/// 操作码：替换。
pub const OP_SUB: u8 = 2;
/// 操作码：转写。
pub const OP_TRANS: u8 = 3;
/// 操作码：执行脚本。
pub const OP_RUN: u8 = 4;
/// 操作码：差异计数。
pub const OP_DIFF: u8 = 5;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字（每字节一次乘法，共八次）。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 命令名表。
pub const CMD_NAMES: [&str; CMDS] = ["s", "y", "d", "p", "n", "q"];
/// 命令基线：六条命令的支持级别。
pub const CMD_SUPPORT: [u8; CMDS] =
    [SUP_FULL, SUP_FULL, SUP_FULL, SUP_FULL, SUP_PARTIAL, SUP_PARTIAL];

/// 差异对照表行名。
pub const DIFF_NAMES: [&str; DIFFS] = [
    "s-global",
    "s-print",
    "s-icase",
    "s-nth",
    "y-translit",
    "q-exit",
    "in-place",
    "extended-re",
    "addr-0-re",
    "w-file",
    "e-exec",
];

/// 差异对照表状态。
pub const DIFF_STATUS: [u8; DIFFS] = [
    SUP_FULL,    // s///g
    SUP_FULL,    // s///p
    SUP_FULL,    // s///I
    SUP_PARTIAL, // s///N
    SUP_FULL,    // y///
    SUP_PARTIAL, // q 退出码
    SUP_FULL,    // -i
    SUP_FULL,    // -E
    SUP_GNU,     // 0,/re/
    SUP_GNU,     // w
    SUP_FULL,    // e
];

/// 命令名。
pub fn cmd_name(cmd: u8) -> Result<&'static str, u32> {
    match CMD_NAMES.get(cmd as usize) {
        Some(v) => Ok(v),
        None => Err(SD_ECMD),
    }
}

/// 命令支持级别。
pub fn cmd_support(cmd: u8) -> Result<u8, u32> {
    match CMD_SUPPORT.get(cmd as usize) {
        Some(v) => Ok(*v),
        None => Err(SD_ECMD),
    }
}

/// 按级别统计命令条数。
pub fn count_cmd_support(level: u8) -> Result<u32, u32> {
    if level > SUP_GNU {
        return Err(SD_EUNSUP);
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CMDS {
        match CMD_SUPPORT.get(i) {
            Some(v) => {
                if *v == level {
                    n += 1;
                }
            }
            None => return Err(SD_ECMD),
        }
        i += 1;
    }
    Ok(n)
}

/// 差异表行名。
pub fn diff_name(row: u8) -> Result<&'static str, u32> {
    match DIFF_NAMES.get(row as usize) {
        Some(v) => Ok(v),
        None => Err(SD_EUNSUP),
    }
}

/// 差异表状态。
pub fn diff_status(row: u8) -> Result<u8, u32> {
    match DIFF_STATUS.get(row as usize) {
        Some(v) => Ok(*v),
        None => Err(SD_EUNSUP),
    }
}

/// 按级别统计差异表行数。
pub fn count_diff_status(level: u8) -> Result<u32, u32> {
    if level > SUP_GNU {
        return Err(SD_EUNSUP);
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < DIFFS {
        match DIFF_STATUS.get(i) {
            Some(v) => {
                if *v == level {
                    n += 1;
                }
            }
            None => return Err(SD_EUNSUP),
        }
        i += 1;
    }
    Ok(n)
}

/// 支持级别标签。
pub fn support_label(level: u8) -> Result<&'static str, u32> {
    match level {
        SUP_FULL => Ok("完整"),
        SUP_PARTIAL => Ok("有差异"),
        SUP_GNU => Ok("仅 GNU"),
        _ => Err(SD_EUNSUP),
    }
}

/// 严格子集闸门：只有 FULL 放行。
pub fn require_full(cmd: u8) -> Result<u8, u32> {
    let level = cmd_support(cmd)?;
    if level == SUP_FULL {
        Ok(level)
    } else {
        Err(SD_EUNSUP)
    }
}

/// 一条解析后的命令。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// 命令码。
    pub cmd: u8,
    /// 地址类型。
    pub addr: u8,
    /// 地址第一操作数。
    pub first: u32,
    /// 步长。
    pub step: u32,
    /// 第 n 次（0 表示第一次）。
    pub nth: u32,
    /// 全局标志。
    pub global: bool,
    /// 打印标志。
    pub print: bool,
    /// 忽略大小写。
    pub icase: bool,
    /// 模式（`s` 的左侧 / `y` 的源表）。
    pub pat: String,
    /// 替换（`s` 的右侧 / `y` 的目标表）。
    pub rep: String,
    /// 正则地址的文本。
    pub addr_text: String,
}

impl Command {
    /// 空命令。
    pub fn new() -> Command {
        Command {
            cmd: SD_S,
            addr: ADDR_NONE,
            first: 0,
            step: 0,
            nth: 0,
            global: false,
            print: false,
            icase: false,
            pat: String::new(),
            rep: String::new(),
            addr_text: String::new(),
        }
    }
}

/// 读十进制数（溢出判越界）。
fn read_number(b: &[u8], from: usize) -> Result<(u32, usize), u32> {
    let mut i = from;
    let mut v = 0u32;
    let mut any = false;
    while i < b.len() {
        match b.get(i) {
            Some(c) => {
                if !c.is_ascii_digit() {
                    break;
                }
                any = true;
                v = v
                    .checked_mul(10)
                    .and_then(|x| x.checked_add((*c - b'0') as u32))
                    .ok_or(SD_ERANGE)?;
                i += 1;
            }
            None => break,
        }
    }
    if !any {
        return Err(SD_EADDR);
    }
    Ok((v, i))
}

/// 扫到下一个未转义的分隔符；`open` 是分隔符所在下标。
fn scan_delim(b: &[u8], open: usize, delim: u8) -> Result<usize, u32> {
    let mut i = open + 1;
    while i < b.len() {
        match b.get(i) {
            Some(c) => {
                if *c == b'\\' {
                    i = i.saturating_add(2);
                    continue;
                }
                if *c == delim {
                    return Ok(i);
                }
                i += 1;
            }
            None => break,
        }
    }
    Err(SD_EREP)
}

/// 追加一段命令体，并把 `\分隔符` 还原为字面分隔符。
fn push_segment(out: &mut String, seg: &str, delim: u8) {
    let b = seg.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(b'\\') => {
                match b.get(i + 1) {
                    Some(n) if *n == delim || *n == b'/' => {
                        out.push(*n as char);
                        i = i.saturating_add(2);
                    }
                    _ => {
                        out.push('\\');
                        i += 1;
                    }
                }
            }
            Some(v) => {
                out.push(*v as char);
                i += 1;
            }
            None => break,
        }
    }
}

/// 解析命令体：`s` 的三段。
fn parse_sub(text: &str, b: &[u8], at: usize, c: &mut Command) -> Result<(), u32> {
    let delim = match b.get(at) {
        Some(v) => *v,
        None => return Err(SD_EPAT),
    };
    if delim.is_ascii_alphanumeric() || delim == b' ' || delim == b'\\' || delim == b'\n' {
        return Err(SD_EPAT);
    }
    let pat_end = scan_delim(b, at, delim)?;
    if pat_end == at + 1 {
        return Err(SD_EPAT);
    }
    let rep_end = scan_delim(b, pat_end, delim)?;
    let pat = match text.get(at + 1..pat_end) {
        Some(v) => v,
        None => return Err(SD_EPAT),
    };
    let rep = match text.get(pat_end + 1..rep_end) {
        Some(v) => v,
        None => return Err(SD_EREP),
    };
    push_segment(&mut c.pat, pat, delim);
    push_segment(&mut c.rep, rep, delim);
    let mut j = rep_end + 1;
    let mut saw_nth = false;
    while j < b.len() {
        match b.get(j) {
            Some(b'g') => c.global = true,
            Some(b'p') => c.print = true,
            Some(b'I') => c.icase = true,
            Some(b'i') => c.icase = true,
            Some(d) => {
                if d.is_ascii_digit() {
                    saw_nth = true;
                    c.nth = c
                        .nth
                        .checked_mul(10)
                        .and_then(|x| x.checked_add((*d - b'0') as u32))
                        .ok_or(SD_ERANGE)?;
                } else if *d == b'w' || *d == b'e' {
                    return Err(SD_EUNSUP);
                } else {
                    return Err(SD_EFLAG);
                }
            }
            None => break,
        }
        j += 1;
    }
    if saw_nth && c.nth == 0 {
        return Err(SD_ERANGE);
    }
    Ok(())
}

/// 解析命令体：`y` 的两段。
fn parse_trans(text: &str, b: &[u8], at: usize, c: &mut Command) -> Result<(), u32> {
    let delim = match b.get(at) {
        Some(v) => *v,
        None => return Err(SD_EPAT),
    };
    if delim.is_ascii_alphanumeric() || delim == b' ' || delim == b'\\' || delim == b'\n' {
        return Err(SD_EPAT);
    }
    let src_end = scan_delim(b, at, delim)?;
    if src_end == at + 1 {
        return Err(SD_EPAT);
    }
    let dst_end = scan_delim(b, src_end, delim)?;
    let src = match text.get(at + 1..src_end) {
        Some(v) => v,
        None => return Err(SD_EPAT),
    };
    let dst = match text.get(src_end + 1..dst_end) {
        Some(v) => v,
        None => return Err(SD_EREP),
    };
    if src.len() != dst.len() {
        return Err(SD_EREP);
    }
    if dst_end + 1 != b.len() {
        return Err(SD_EFLAG);
    }
    push_segment(&mut c.pat, src, delim);
    push_segment(&mut c.rep, dst, delim);
    Ok(())
}

/// 解析一条 sed 脚本命令。
pub fn parse(text: &str) -> Result<Command, u32> {
    let b = text.as_bytes();
    if b.is_empty() {
        return Err(SD_EADDR);
    }
    let mut c = Command::new();
    let first = match b.first() {
        Some(v) => *v,
        None => return Err(SD_EADDR),
    };
    let mut i = 0usize;
    if first.is_ascii_digit() {
        let (n, ni) = read_number(b, 0)?;
        if n == 0 {
            return Err(SD_ERANGE);
        }
        if b.get(ni) == Some(&b'~') {
            let (step, si) = read_number(b, ni + 1)?;
            if step == 0 {
                return Err(SD_ERANGE);
            }
            c.addr = ADDR_STEP;
            c.first = n;
            c.step = step;
            i = si;
        } else if b.get(ni) == Some(&b',') {
            return Err(SD_EUNSUP);
        } else {
            c.addr = ADDR_LINE;
            c.first = n;
            i = ni;
        }
    } else if first == b'$' {
        c.addr = ADDR_LAST;
        i = 1;
    } else if first == b'/' {
        let end = scan_delim(b, 0, b'/')?;
        let addr = match text.get(1..end) {
            Some(v) => v,
            None => return Err(SD_EADDR),
        };
        if addr.is_empty() {
            return Err(SD_EADDR);
        }
        c.addr = ADDR_RE;
        c.addr_text.push_str(addr);
        i = end + 1;
        if b.get(i) == Some(&b',') {
            return Err(SD_EUNSUP);
        }
    }
    let letter = match b.get(i) {
        Some(v) => *v,
        None => return Err(SD_ECMD),
    };
    match letter {
        b's' => {
            c.cmd = SD_S;
            parse_sub(text, b, i + 1, &mut c)?;
        }
        b'y' => {
            c.cmd = SD_Y;
            parse_trans(text, b, i + 1, &mut c)?;
        }
        b'd' => {
            c.cmd = SD_D;
            if i + 1 != b.len() {
                return Err(SD_EFLAG);
            }
        }
        b'p' => {
            c.cmd = SD_P;
            if i + 1 != b.len() {
                return Err(SD_EFLAG);
            }
        }
        b'n' => {
            c.cmd = SD_N;
            if i + 1 != b.len() {
                return Err(SD_EFLAG);
            }
        }
        b'q' => {
            c.cmd = SD_Q;
            if i + 1 != b.len() {
                return Err(SD_EUNSUP);
            }
        }
        _ => return Err(SD_ECMD),
    }
    Ok(c)
}

/// 判据侧包装：解析失败时返回空白命令，由调用方断言执行结果。
fn parse_ok(text: &str) -> Command {
    match parse(text) {
        Ok(c) => c,
        Err(_) => Command {
            cmd: 0,
            addr: ADDR_NONE,
            first: 0,
            step: 1,
            nth: 0,
            global: false,
            print: false,
            icase: false,
            pat: String::new(),
            rep: String::new(),
            addr_text: String::new(),
        },
    }
}

/// 大小写可折叠的字面量查找。
fn find_from(hay: &[u8], needle: &[u8], from: usize, icase: bool) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    let last = hay.len() - needle.len();
    let mut i = from;
    while i <= last {
        let mut k = 0usize;
        let mut ok = true;
        while k < needle.len() {
            match (hay.get(i + k), needle.get(k)) {
                (Some(a), Some(b)) => {
                    let eq = if icase {
                        a.to_ascii_lowercase() == b.to_ascii_lowercase()
                    } else {
                        a == b
                    };
                    if !eq {
                        ok = false;
                        break;
                    }
                }
                _ => {
                    ok = false;
                    break;
                }
            }
            k += 1;
        }
        if ok {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 展开替换串：`&` 整段、`\&` 字面、`\\` 字面、`\n` 换行。
fn expand_rep(out: &mut String, rep: &str, matched: &str) -> Result<(), u32> {
    let b = rep.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(b'&') => out.push_str(matched),
            Some(b'\\') => {
                let n = match b.get(i + 1) {
                    Some(v) => *v,
                    None => {
                        out.push('\\');
                        break;
                    }
                };
                match n {
                    b'&' => out.push('&'),
                    b'\\' => out.push('\\'),
                    b'n' => out.push('\n'),
                    _ => {
                        out.push('\\');
                        out.push(n as char);
                    }
                }
                i += 1;
            }
            Some(c) => out.push(*c as char),
            None => break,
        }
        i += 1;
    }
    Ok(())
}

/// 替换：返回（结果, 实际替换次数）。
pub fn substitute(
    line: &str,
    pat: &str,
    rep: &str,
    nth: u32,
    global: bool,
    icase: bool,
) -> Result<(String, u32), u32> {
    if pat.is_empty() {
        return Err(SD_EPAT);
    }
    let hay = line.as_bytes();
    let needle = pat.as_bytes();
    let mut out = String::new();
    let mut count = 0u32;
    let mut last = 0usize;
    let mut from = 0usize;
    let mut seen = 0u32;
    while from + needle.len() <= hay.len() {
        let pos = match find_from(hay, needle, from, icase) {
            Some(v) => v,
            None => break,
        };
        seen = seen.saturating_add(1);
        let selected = if global {
            true
        } else if nth == 0 {
            seen == 1
        } else {
            seen == nth
        };
        if selected {
            if let Some(seg) = line.get(last..pos) {
                out.push_str(seg);
            }
            let matched = match line.get(pos..pos + needle.len()) {
                Some(v) => v,
                None => return Err(SD_EREP),
            };
            expand_rep(&mut out, rep, matched)?;
            count += 1;
            last = pos + needle.len();
            from = last;
            if !global {
                break;
            }
        } else {
            from = pos + 1;
        }
    }
    if count == 0 {
        return Ok((String::from(line), 0));
    }
    if let Some(tail) = line.get(last..) {
        out.push_str(tail);
    }
    Ok((out, count))
}

/// 逐字符转写：源表与目标表等长。
pub fn transliterate(line: &str, src: &str, dst: &str) -> Result<String, u32> {
    if src.is_empty() {
        return Err(SD_EPAT);
    }
    let sb = src.as_bytes();
    let db = dst.as_bytes();
    if sb.len() != db.len() {
        return Err(SD_EREP);
    }
    let mut out = String::new();
    let lb = line.as_bytes();
    let mut i = 0usize;
    while i < lb.len() {
        let c = match lb.get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut mapped = c;
        let mut k = 0usize;
        while k < sb.len() {
            match (sb.get(k), db.get(k)) {
                (Some(a), Some(b)) => {
                    if *a == c {
                        mapped = *b;
                        break;
                    }
                }
                _ => break,
            }
            k += 1;
        }
        out.push(mapped as char);
        i += 1;
    }
    Ok(out)
}

/// 地址是否命中（行号从 1 起）。
fn addr_match(cmd: &Command, line: &str, idx: usize, total: usize) -> Result<bool, u32> {
    let no = idx.saturating_add(1);
    match cmd.addr {
        ADDR_NONE => Ok(true),
        ADDR_LINE => Ok(no == cmd.first as usize),
        ADDR_LAST => Ok(no == total),
        ADDR_RE => {
            if cmd.addr_text.is_empty() {
                return Err(SD_EADDR);
            }
            Ok(find_from(line.as_bytes(), cmd.addr_text.as_bytes(), 0, false).is_some())
        }
        ADDR_STEP => {
            if cmd.step == 0 || cmd.first == 0 || no < cmd.first as usize {
                return Ok(false);
            }
            Ok((no - cmd.first as usize) % cmd.step as usize == 0)
        }
        _ => Err(SD_EADDR),
    }
}

/// 执行一段脚本，返回输出文本（行尾统一补换行）。
pub fn run(lines: &[&str], script: &[Command], quiet: bool) -> Result<String, u32> {
    let mut out = String::new();
    let mut li = 0usize;
    while li < lines.len() {
        let line = match lines.get(li) {
            Some(v) => *v,
            None => break,
        };
        if line.is_empty() && lines.is_empty() {
            return Err(SD_ELINE);
        }
        let mut cur = String::from(line);
        let mut deleted = false;
        let mut skip_auto = false;
        let mut stop = false;
        let mut ci = 0usize;
        while ci < script.len() {
            let cmd = match script.get(ci) {
                Some(v) => v,
                None => break,
            };
            ci += 1;
            if !addr_match(cmd, cur.as_str(), li, lines.len())? {
                continue;
            }
            match cmd.cmd {
                SD_S => {
                    let (next, _n) = substitute(
                        cur.as_str(),
                        cmd.pat.as_str(),
                        cmd.rep.as_str(),
                        cmd.nth,
                        cmd.global,
                        cmd.icase,
                    )?;
                    cur = next;
                    if cmd.print {
                        out.push_str(cur.as_str());
                        out.push('\n');
                    }
                }
                SD_Y => {
                    let next = transliterate(cur.as_str(), cmd.pat.as_str(), cmd.rep.as_str())?;
                    cur = next;
                }
                SD_D => {
                    deleted = true;
                    break;
                }
                SD_P => {
                    out.push_str(cur.as_str());
                    out.push('\n');
                }
                SD_N => {
                    if !quiet {
                        out.push_str(cur.as_str());
                        out.push('\n');
                    }
                    skip_auto = true;
                    break;
                }
                SD_Q => {
                    stop = true;
                    break;
                }
                _ => return Err(SD_ECMD),
            }
        }
        if !quiet && !deleted && !skip_auto {
            out.push_str(cur.as_str());
            out.push('\n');
        }
        if stop {
            break;
        }
        li += 1;
    }
    Ok(out)
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
            self.rejects += 1;
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

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SD_EBAL);
        }
        const VALS: [u32; 8] = [0, 2, 3, 2, 1, 3, 4, 2];
        const CODES: [u32; 6] =
            [SD_ECMD, SD_EFLAG, SD_EPAT, SD_EREP, SD_EUNSUP, SD_ERANGE];
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

    /// 票号。
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
            self.rejects += 1;
            return Err(SD_EBAL);
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
            Err(SD_ESEAL)
        }
    }

    /// 冻结票号。
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

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_PARSE, parse("s/a/b/g").map(|c| c.cmd as u32)));
    let _ = l.register(word_cell(1, OP_PARSE, parse("2d").map(|c| c.cmd as u32)));
    let _ = l.register(word_cell(2, OP_PARSE, parse("$p").map(|c| c.cmd as u32)));
    let _ = l.register(word_cell(
        3,
        OP_SUB,
        substitute("aba", "a", "X", 0, true, false).map(|(_, n)| n),
    ));
    let _ = l.register(word_cell(
        4,
        OP_SUB,
        substitute("cat", "a", "o", 0, false, false).map(|(_, n)| n),
    ));
    let _ = l.register(word_cell(
        5,
        OP_TRANS,
        transliterate("abc", "abc", "xyz").map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(6, OP_RUN, {
        let script = [parse_ok("s/a/A/")];
        match script.first() {
            Some(_) => run(&["a", "b"], &script, false).map(|s| s.len() as u32),
            None => Err(SD_ELINE),
        }
    }));
    let _ = l.register(word_cell(7, OP_DIFF, count_diff_status(SUP_GNU)));

    let _ = l.register(word_cell(8, OP_PARSE, parse("z").map(|c| c.cmd as u32)));
    let _ = l.register(word_cell(9, OP_PARSE, parse("s/a/b/z").map(|c| c.cmd as u32)));
    let _ = l.register(word_cell(10, OP_PARSE, parse("s//b/").map(|c| c.cmd as u32)));
    let _ = l.register(word_cell(11, OP_PARSE, parse("s/a/b").map(|c| c.cmd as u32)));
    let _ = l.register(word_cell(12, OP_PARSE, parse("1,5d").map(|c| c.cmd as u32)));
    let _ = l.register(word_cell(
        13,
        OP_PARSE,
        parse("99999999999d").map(|c| c.cmd as u32),
    ));

    l
}

/// 四路注入：基线表 / 解析闸门 / 替换引擎 / 执行模型。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = CMDS == 6
        && DIFFS == 11
        && count_cmd_support(SUP_FULL) == Ok(4)
        && count_cmd_support(SUP_PARTIAL) == Ok(2)
        && count_cmd_support(SUP_GNU) == Ok(0)
        && count_diff_status(SUP_FULL) == Ok(7)
        && count_diff_status(SUP_PARTIAL) == Ok(2)
        && count_diff_status(SUP_GNU) == Ok(2)
        && cmd_name(SD_Q) == Ok("q")
        && cmd_name(6) == Err(SD_ECMD)
        && diff_name(8) == Ok("addr-0-re")
        && support_label(SUP_DIFF_LABEL) == Ok("有差异")
        && require_full(SD_S).is_ok()
        && require_full(SD_N) == Err(SD_EUNSUP);

    let g = parse("s/a/b/g");
    let flags_ok = match g {
        Ok(ref c) => c.global && !c.print && !c.icase && c.nth == 0,
        Err(_) => false,
    };
    let gpi = match parse("s/a/b/gpI3") {
        Ok(ref c) => c.global && c.print && c.icase && c.nth == 3,
        Err(_) => false,
    };
    let addr = match parse("3~2p") {
        Ok(ref c) => c.addr == ADDR_STEP && c.first == 3 && c.step == 2 && c.cmd == SD_P,
        Err(_) => false,
    };
    let last = match parse("$d") {
        Ok(ref c) => c.addr == ADDR_LAST && c.cmd == SD_D,
        Err(_) => false,
    };
    let re = match parse("/b/p") {
        Ok(ref c) => c.addr == ADDR_RE && c.addr_text.as_str() == "b" && c.cmd == SD_P,
        Err(_) => false,
    };
    let gates = parse("z") == Err(SD_ECMD)
        && parse("s/a/b/z") == Err(SD_EFLAG)
        && parse("s//b/") == Err(SD_EPAT)
        && parse("s/a/b") == Err(SD_EREP)
        && parse("1,5d") == Err(SD_EUNSUP)
        && parse("99999999999d") == Err(SD_ERANGE)
        && parse("0d") == Err(SD_ERANGE)
        && parse("") == Err(SD_EADDR);
    out[1] = flags_ok && gpi && addr && last && re && gates;

    let a = match substitute("aba", "a", "X", 0, true, false) {
        Ok((s, n)) => s.as_str() == "XbX" && n == 2,
        Err(_) => false,
    };
    let b = match substitute("aba", "a", "X", 2, false, false) {
        Ok((s, n)) => s.as_str() == "abX" && n == 1,
        Err(_) => false,
    };
    let c = match substitute("cat", "a", "[&]", 0, false, false) {
        Ok((s, n)) => s.as_str() == "c[a]t" && n == 1,
        Err(_) => false,
    };
    let d = match substitute("AbA", "a", "x", 0, true, true) {
        Ok((s, n)) => s.as_str() == "xbx" && n == 2,
        Err(_) => false,
    };
    let e = match substitute("abc", "z", "q", 0, true, false) {
        Ok((s, n)) => s.as_str() == "abc" && n == 0,
        Err(_) => false,
    };
    let f = match transliterate("abc", "abc", "xyz") {
        Ok(s) => s.as_str() == "xyz",
        Err(_) => false,
    };
    let bad = transliterate("abc", "ab", "x") == Err(SD_EREP)
        && substitute("a", "", "x", 0, false, false) == Err(SD_EPAT);
    out[2] = a && b && c && d && e && f && bad;

    let del = {
        let script = [parse_ok("1d")];
        match run(&["a", "b"], &script, false) {
            Ok(s) => s.as_str() == "b\n",
            Err(_) => false,
        }
    };
    let prt = {
        let script = [parse_ok("p")];
        match run(&["a"], &script, false) {
            Ok(s) => s.as_str() == "a\na\n",
            Err(_) => false,
        }
    };
    let quit = {
        let script = [parse_ok("1q")];
        match run(&["a", "b"], &script, false) {
            Ok(s) => s.as_str() == "a\n",
            Err(_) => false,
        }
    };
    let step = {
        let script = [parse_ok("1~2p")];
        match run(&["a", "b", "c"], &script, true) {
            Ok(s) => s.as_str() == "a\nc\n",
            Err(_) => false,
        }
    };
    let chain = {
        let script = [parse_ok("s/a/A/"), parse_ok("s/b/B/")];
        match run(&["ab"], &script, false) {
            Ok(s) => s.as_str() == "AB\n",
            Err(_) => false,
        }
    };
    let addr_re = {
        let script = [parse_ok("/b/p")];
        match run(&["aa", "ba"], &script, true) {
            Ok(s) => s.as_str() == "ba\n",
            Err(_) => false,
        }
    };
    out[3] = del && prt && quit && step && chain && addr_re;

    out
}

/// 账本用的“有差异”字面量（与 `SUP_PARTIAL` 数值相同）。
const SUP_DIFF_LABEL: u8 = 1;

/// 十三项判据。
pub fn run_unx11591_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11591.busybox_sed");
    let l = build_ledger();

    s.add(
        "SD1-六命令基线表",
        CMDS == 6
            && CMD_NAMES.len() == CMDS
            && count_cmd_support(SUP_FULL) == Ok(4)
            && count_cmd_support(SUP_PARTIAL) == Ok(2)
            && count_cmd_support(SUP_GNU) == Ok(0)
            && cmd_support(SD_N) == Ok(SUP_PARTIAL)
            && cmd_name(SD_S) == Ok("s")
            && cmd_name(9) == Err(SD_ECMD),
        "static",
    );

    s.add(
        "SD2-十一行差异表",
        DIFFS == 11
            && DIFF_NAMES.len() == DIFFS
            && DIFF_STATUS.len() == DIFFS
            && count_diff_status(SUP_FULL) == Ok(7)
            && count_diff_status(SUP_PARTIAL) == Ok(2)
            && count_diff_status(SUP_GNU) == Ok(2)
            && diff_name(8) == Ok("addr-0-re")
            && diff_status(9) == Ok(SUP_GNU)
            && diff_status(11) == Err(SD_EUNSUP),
        "static",
    );

    s.add(
        "SD3-替换标志解析",
        {
            let a = match parse("s/a/b/g") {
                Ok(ref c) => c.cmd == SD_S && c.global && !c.print && !c.icase,
                Err(_) => false,
            };
            let b = match parse("s/a/b/gpI3") {
                Ok(ref c) => c.global && c.print && c.icase && c.nth == 3,
                Err(_) => false,
            };
            let c = match parse("s|/|\\/|") {
                Ok(ref v) => v.pat.as_str() == "/" && v.rep.as_str() == "/",
                Err(_) => false,
            };
            a && b && c
        },
        "static",
    );

    s.add(
        "SD4-地址五形态",
        {
            let a = parse("d").map(|c| c.addr) == Ok(ADDR_NONE);
            let b = parse("12d").map(|c| c.first) == Ok(12);
            let c = parse("$d").map(|c| c.addr) == Ok(ADDR_LAST);
            let d = match parse("/x/p") {
                Ok(ref v) => v.addr == ADDR_RE && v.addr_text.as_str() == "x",
                Err(_) => false,
            };
            let e = match parse("3~2p") {
                Ok(ref v) => v.addr == ADDR_STEP && v.first == 3 && v.step == 2,
                Err(_) => false,
            };
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "SD5-解析闸门六拒",
        parse("z") == Err(SD_ECMD)
            && parse("s/a/b/z") == Err(SD_EFLAG)
            && parse("s//b/") == Err(SD_EPAT)
            && parse("s/a/b") == Err(SD_EREP)
            && parse("1,5d") == Err(SD_EUNSUP)
            && parse("99999999999d") == Err(SD_ERANGE)
            && parse("0d") == Err(SD_ERANGE)
            && parse("") == Err(SD_EADDR)
            && parse("q 3") == Err(SD_EUNSUP),
        "static",
    );

    s.add(
        "SD6-全局替换计数",
        {
            let a = match substitute("aba", "a", "X", 0, true, false) {
                Ok((v, n)) => v.as_str() == "XbX" && n == 2,
                Err(_) => false,
            };
            let b = match substitute("aaaa", "aa", "Z", 0, true, false) {
                Ok((v, n)) => v.as_str() == "ZZ" && n == 2,
                Err(_) => false,
            };
            let c = match substitute("abc", "z", "q", 0, true, false) {
                Ok((v, n)) => v.as_str() == "abc" && n == 0,
                Err(_) => false,
            };
            a && b && c
        },
        "static",
    );

    s.add(
        "SD7-第n次与整段展开",
        {
            let a = match substitute("aba", "a", "X", 2, false, false) {
                Ok((v, n)) => v.as_str() == "abX" && n == 1,
                Err(_) => false,
            };
            let b = match substitute("cat", "a", "[&]", 0, false, false) {
                Ok((v, n)) => v.as_str() == "c[a]t" && n == 1,
                Err(_) => false,
            };
            let c = match substitute("a", "a", "\\&", 0, false, false) {
                Ok((v, _)) => v.as_str() == "&",
                Err(_) => false,
            };
            let d = match substitute("aba", "a", "X", 9, false, false) {
                Ok((v, n)) => v.as_str() == "aba" && n == 0,
                Err(_) => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "SD8-忽略大小写替换",
        {
            let a = match substitute("AbA", "a", "x", 0, true, true) {
                Ok((v, n)) => v.as_str() == "xbx" && n == 2,
                Err(_) => false,
            };
            let b = match substitute("AbA", "a", "x", 0, true, false) {
                Ok((v, n)) => v.as_str() == "AbA" && n == 0,
                Err(_) => false,
            };
            let c = match substitute("a", "", "x", 0, false, false) {
                Ok(_) => false,
                Err(e) => e == SD_EPAT,
            };
            a && b && c
        },
        "static",
    );

    s.add(
        "SD9-字符转写",
        {
            let a = match transliterate("abc", "abc", "xyz") {
                Ok(v) => v.as_str() == "xyz",
                Err(_) => false,
            };
            let b = match transliterate("hello", "el", "ip") {
                Ok(v) => v.as_str() == "hippo",
                Err(_) => false,
            };
            let c = transliterate("abc", "ab", "x") == Err(SD_EREP);
            let d = transliterate("abc", "", "x") == Err(SD_EPAT);
            a && b && c && d
        },
        "static",
    );

    s.add(
        "SD10-执行模型四命令",
        {
            let del = {
                let script = [parse_ok("1d")];
                match run(&["a", "b"], &script, false) {
                    Ok(v) => v.as_str() == "b\n",
                    Err(_) => false,
                }
            };
            let prt = {
                let script = [parse_ok("p")];
                match run(&["a"], &script, false) {
                    Ok(v) => v.as_str() == "a\na\n",
                    Err(_) => false,
                }
            };
            let quit = {
                let script = [parse_ok("1q")];
                match run(&["a", "b"], &script, false) {
                    Ok(v) => v.as_str() == "a\n",
                    Err(_) => false,
                }
            };
            let chain = {
                let script = [parse_ok("s/a/A/"), parse_ok("s/b/B/")];
                match run(&["ab"], &script, false) {
                    Ok(v) => v.as_str() == "AB\n",
                    Err(_) => false,
                }
            };
            del && prt && quit && chain
        },
        "static",
    );

    s.add(
        "SD11-地址匹配与步长",
        {
            let step = {
                let script = [parse_ok("1~2p")];
                match run(&["a", "b", "c"], &script, true) {
                    Ok(v) => v.as_str() == "a\nc\n",
                    Err(_) => false,
                }
            };
            let last = {
                let script = [parse_ok("$d")];
                match run(&["a", "b"], &script, false) {
                    Ok(v) => v.as_str() == "a\n",
                    Err(_) => false,
                }
            };
            let re = {
                let script = [parse_ok("/b/p")];
                match run(&["aa", "ba"], &script, true) {
                    Ok(v) => v.as_str() == "ba\n",
                    Err(_) => false,
                }
            };
            step && last && re
        },
        "static",
    );

    s.add(
        "SD12-严格子集闸门",
        require_full(SD_S).is_ok()
            && require_full(SD_D).is_ok()
            && require_full(SD_N) == Err(SD_EUNSUP)
            && require_full(SD_Q) == Err(SD_EUNSUP)
            && support_label(SUP_PARTIAL) == Ok("有差异")
            && support_label(SUP_GNU) == Ok("仅 GNU")
            && support_label(7) == Err(SD_EUNSUP)
            && count_cmd_support(7) == Err(SD_EUNSUP),
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let one = f.freeze();
    let two = f.freeze();
    let inj = inject_four();
    s.add(
        "SD13-冻结重放四注入",
        l.audit().is_ok()
            && one.is_ok()
            && one == rep
            && two.is_err()
            && f.is_frozen()
            && f.root() != 0
            && f.rejects() == 1
            && inj[0]
            && inj[1]
            && inj[2]
            && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(3).map(|s| s.value), Some(2));
        assert_eq!(l.sample(12).map(|s| s.code), Some(SD_EUNSUP));
    }

    #[test]
    fn command_baseline() {
        assert_eq!(CMDS, 6);
        assert_eq!(count_cmd_support(SUP_FULL), Ok(4));
        assert_eq!(count_cmd_support(SUP_PARTIAL), Ok(2));
        assert_eq!(count_cmd_support(SUP_GNU), Ok(0));
        assert_eq!(cmd_name(SD_Y), Ok("y"));
        assert_eq!(cmd_support(SD_Q), Ok(SUP_PARTIAL));
        assert_eq!(cmd_name(6), Err(SD_ECMD));
    }

    #[test]
    fn difference_baseline() {
        assert_eq!(DIFFS, 11);
        assert_eq!(count_diff_status(SUP_FULL), Ok(7));
        assert_eq!(count_diff_status(SUP_PARTIAL), Ok(2));
        assert_eq!(count_diff_status(SUP_GNU), Ok(2));
        assert_eq!(diff_name(9), Ok("w-file"));
        assert_eq!(diff_status(10), Ok(SUP_FULL));
        assert_eq!(diff_name(11), Err(SD_EUNSUP));
    }

    #[test]
    fn parse_substitution_flags() {
        let c = match parse("s/a/b/gpI3") {
            Ok(v) => v,
            Err(_) => Command::new(),
        };
        assert!(c.global);
        assert!(c.print);
        assert!(c.icase);
        assert_eq!(c.nth, 3);
        assert_eq!(c.cmd, SD_S);
        assert_eq!(c.pat.as_str(), "a");
        assert_eq!(c.rep.as_str(), "b");
    }

    #[test]
    fn parse_address_forms() {
        assert_eq!(parse("d").map(|c| c.addr), Ok(ADDR_NONE));
        assert_eq!(parse("12d").map(|c| c.first), Ok(12));
        assert_eq!(parse("$d").map(|c| c.addr), Ok(ADDR_LAST));
        let re = match parse("/x/p") {
            Ok(v) => v,
            Err(_) => Command::new(),
        };
        assert_eq!(re.addr, ADDR_RE);
        assert_eq!(re.addr_text.as_str(), "x");
        let st = match parse("3~2p") {
            Ok(v) => v,
            Err(_) => Command::new(),
        };
        assert_eq!(st.addr, ADDR_STEP);
        assert_eq!(st.step, 2);
    }

    #[test]
    fn parse_rejections() {
        assert_eq!(parse("z"), Err(SD_ECMD));
        assert_eq!(parse("s/a/b/z"), Err(SD_EFLAG));
        assert_eq!(parse("s//b/"), Err(SD_EPAT));
        assert_eq!(parse("s/a/b"), Err(SD_EREP));
        assert_eq!(parse("1,5d"), Err(SD_EUNSUP));
        assert_eq!(parse("99999999999d"), Err(SD_ERANGE));
        assert_eq!(parse("0d"), Err(SD_ERANGE));
        assert_eq!(parse(""), Err(SD_EADDR));
        assert_eq!(parse("1,5p"), Err(SD_EUNSUP));
    }

    #[test]
    fn substitute_global() {
        let (s, n) = match substitute("aba", "a", "X", 0, true, false) {
            Ok(v) => v,
            Err(_) => (String::new(), 99),
        };
        assert_eq!(s.as_str(), "XbX");
        assert_eq!(n, 2);
        let (s2, n2) = match substitute("aaaa", "aa", "Z", 0, true, false) {
            Ok(v) => v,
            Err(_) => (String::new(), 99),
        };
        assert_eq!(s2.as_str(), "ZZ");
        assert_eq!(n2, 2);
    }

    #[test]
    fn substitute_nth() {
        let (s, n) = match substitute("aba", "a", "X", 2, false, false) {
            Ok(v) => v,
            Err(_) => (String::new(), 99),
        };
        assert_eq!(s.as_str(), "abX");
        assert_eq!(n, 1);
        let (s2, n2) = match substitute("aba", "a", "X", 9, false, false) {
            Ok(v) => v,
            Err(_) => (String::new(), 99),
        };
        assert_eq!(s2.as_str(), "aba");
        assert_eq!(n2, 0);
    }

    #[test]
    fn substitute_replacement_escapes() {
        let (a, _) = match substitute("cat", "a", "[&]", 0, false, false) {
            Ok(v) => v,
            Err(_) => (String::new(), 99),
        };
        assert_eq!(a.as_str(), "c[a]t");
        let (b, _) = match substitute("a", "a", "\\&", 0, false, false) {
            Ok(v) => v,
            Err(_) => (String::new(), 99),
        };
        assert_eq!(b.as_str(), "&");
        let (c, _) = match substitute("a", "a", "\\\\", 0, false, false) {
            Ok(v) => v,
            Err(_) => (String::new(), 99),
        };
        assert_eq!(c.as_str(), "\\");
    }

    #[test]
    fn substitute_icase() {
        let (a, n) = match substitute("AbA", "a", "x", 0, true, true) {
            Ok(v) => v,
            Err(_) => (String::new(), 99),
        };
        assert_eq!(a.as_str(), "xbx");
        assert_eq!(n, 2);
        let (b, n2) = match substitute("AbA", "a", "x", 0, true, false) {
            Ok(v) => v,
            Err(_) => (String::new(), 99),
        };
        assert_eq!(b.as_str(), "AbA");
        assert_eq!(n2, 0);
    }

    #[test]
    fn transliteration() {
        let a = match transliterate("hello", "el", "ip") {
            Ok(v) => v,
            Err(_) => String::new(),
        };
        assert_eq!(a.as_str(), "hippo");
        assert_eq!(transliterate("abc", "ab", "x"), Err(SD_EREP));
        assert_eq!(transliterate("abc", "", "x"), Err(SD_EPAT));
    }

    #[test]
    fn run_delete_print_quit() {
        let del = {
            let script = [parse_ok("1d")];
            run(&["a", "b"], &script, false)
        };
        assert_eq!(del, Ok(String::from("b\n")));
        let prt = {
            let script = [parse_ok("p")];
            run(&["a"], &script, false)
        };
        assert_eq!(prt, Ok(String::from("a\na\n")));
        let quit = {
            let script = [parse_ok("1q")];
            run(&["a", "b"], &script, false)
        };
        assert_eq!(quit, Ok(String::from("a\n")));
    }

    #[test]
    fn run_chain_and_quiet() {
        let chain = {
            let script = [parse_ok("s/a/A/"), parse_ok("s/b/B/")];
            run(&["ab"], &script, false)
        };
        assert_eq!(chain, Ok(String::from("AB\n")));
        let quiet = {
            let script = [parse_ok("s/a/A/p")];
            run(&["a"], &script, true)
        };
        assert_eq!(quiet, Ok(String::from("A\n")));
    }

    #[test]
    fn run_address_modes() {
        let step = {
            let script = [parse_ok("1~2p")];
            run(&["a", "b", "c"], &script, true)
        };
        assert_eq!(step, Ok(String::from("a\nc\n")));
        let last = {
            let script = [parse_ok("$d")];
            run(&["a", "b"], &script, false)
        };
        assert_eq!(last, Ok(String::from("a\n")));
        let re = {
            let script = [parse_ok("/b/p")];
            run(&["aa", "ba"], &script, true)
        };
        assert_eq!(re, Ok(String::from("ba\n")));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(SD_EBAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.root(), root);
    }

    #[test]
    fn ledger_slot_guard() {
        let mut l = Ledger::new();
        let bad = Sample { slot: 5, op: OP_PARSE, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(bad), Err(SD_ESLOT));
        let good = Sample { slot: 0, op: OP_PARSE, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(good), Ok(()));
        assert_eq!(l.register(good), Err(SD_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11591_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
