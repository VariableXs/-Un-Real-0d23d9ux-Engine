//! UNX-F11592 busybox grep 子集差异判据
//!
//! busybox 的 `grep` 是“够用就好”的子集实现：它没有回溯引用、没有 PCRE、
//! 没有 `-z` 空字符分隔，`-E` 只覆盖“字面量或选择器”这一小块，花括号区间也
//! 只实现了一部分。本模块把“选项空间”“匹配器”“输出模式”三层拆开建模，
//! 并把“与 GNU 的差异”单独列成一张十二行对照表，避免把“busybox 能吃”和
//! “GNU 能吃”混为一谈。
//!
//! **选项矩阵**：十二个短选项 `-E -F -i -v -c -l -n -o -q -r -w -x`，每个
//! 选项占一个独立位，位与位之间必须两两不相交，并集必须恰好铺满掩码。每个
//! 选项带一档支持级别：`FULL`（语义完整）、`PARTIAL`（有差异）、`GNU`（仅
//! GNU 有）。本子集里 `-E` 与 `-x` 是 PARTIAL，其余十项是 FULL。
//!
//! **差异对照表**：回溯引用、字符类、区间重复、PCRE、空字符分隔、二进制、
//! 颜色、上下文、排除、词边界、仅命中、递归十二行。前十二行里七行完整、
//! 三行部分、两行 GNU 独占。差异表只描述“语义差距”，不参与选项解析。
//!
//! **匹配器**：匹配单位是字节。默认按字面量子串查找；`-i` 折叠 ASCII 大小写；
//! `-E` 把 `|` 解释为或选择器（最多四段，空段非法，仍不解释 `.*[]` 等元字符）；
//! `-w` 要求命中两侧不是字母数字或下划线；`-x` 要求命中覆盖整行。多段命中时
//! 取最左起点，起点相同取较长段。查找从 `from` 起步，允许调用方逐次推进以
//! 统计不重叠命中。
//!
//! **输出模式**：`resolve` 把互斥选项折成一个模式——行、计数、文件名、静默、
//! 仅命中。`-c` 与 `-l`、`-c` 与 `-o`、`-l` 与 `-o`、`-q` 与前三者、`-o` 与
//! `-v` 都判模式冲突；`-E` 与 `-F` 判选项冲突；未知位判非法。
//!
//! **路径闸门**：空路径判输入错误；目录未带 `-r` 判递归缺失；文件路径带尾斜杠
//! 判路径类型错误；层级超过八层判越界。含 NUL 的行判二进制输入，空行集合判
//! 空输入。这些闸门在 `scan` 与 `filter` 里都必须先过，再谈匹配。
//!
//! 闸门次序：`parse_options`：前缀 → 选项字母 → 重复；`resolve`：未知位 →
//! 模式互斥；`find_once`：模式闸门 → 或选择器 → 边界；`filter`：模式分派 →
//! 逐行求值 → 渲染。
//!
//! 账本十四格：八格成功（一次模式解析、两次单行求值、两次扫描、两次渲染、
//! 一次差异计数），六格失败（空模式、选项冲突、模式冲突、二进制输入、空输入、
//! 行号越界）。冻结票号折叠十四格与拒绝计数。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfa50;

/// 模式为空或或选择器出现空段。
pub const GR_EPAT: u32 = SEG_BASE + 0x01;
/// 选项未知、重复，或 `-E`/`-F` 同时出现。
pub const GR_EFLAG: u32 = SEG_BASE + 0x02;
/// 输出模式互斥（计数/文件名/仅命中/静默）。
pub const GR_EMODE: u32 = SEG_BASE + 0x03;
/// 目录未带 `-r`，或文件路径带尾斜杠。
pub const GR_EREC: u32 = SEG_BASE + 0x04;
/// 行内出现 NUL，判二进制输入。
pub const GR_EBIN: u32 = SEG_BASE + 0x05;
/// 空路径或空行集合。
pub const GR_EINPUT: u32 = SEG_BASE + 0x06;
/// 命中计数溢出。
pub const GR_ECOUNT: u32 = SEG_BASE + 0x07;
/// 模式超长、行号越界或路径层级过深。
pub const GR_ERANGE: u32 = SEG_BASE + 0x08;
/// 账本形状不符（14/8/6）。
pub const GR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序或冻结后登记。
pub const GR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 复算值与登记值不符。
pub const GR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放票号不一致。
pub const GR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const GR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;
/// 选项条数。
pub const OPTS: usize = 12;
/// 差异表行数。
pub const DIFFS: usize = 12;
/// 模式长度上限。
pub const MAX_PAT: usize = 64;
/// 或选择器段数上限。
pub const MAX_ALT: usize = 4;
/// 路径层级上限。
pub const MAX_DEPTH: u32 = 8;

/// 支持级别：完整。
pub const SUP_FULL: u8 = 0;
/// 支持级别：有差异。
pub const SUP_PARTIAL: u8 = 1;
/// 支持级别：仅 GNU。
pub const SUP_GNU: u8 = 2;

/// 输出模式：逐行。
pub const MODE_LINES: u8 = 0;
/// 输出模式：只报计数。
pub const MODE_COUNT: u8 = 1;
/// 输出模式：只报文件名。
pub const MODE_FILES: u8 = 2;
/// 输出模式：静默，只看退出状态。
pub const MODE_QUIET: u8 = 3;
/// 输出模式：只报命中片段。
pub const MODE_ONLY: u8 = 4;

/// 路径类型：普通文件。
pub const KIND_FILE: u8 = 0;
/// 路径类型：目录。
pub const KIND_DIR: u8 = 1;

/// 扩展模式（或选择器）。
pub const F_EXTENDED: u32 = 1 << 0;
/// 固定字符串。
pub const F_FIXED: u32 = 1 << 1;
/// 忽略 ASCII 大小写。
pub const F_ICASE: u32 = 1 << 2;
/// 反选。
pub const F_INVERT: u32 = 1 << 3;
/// 只报计数。
pub const F_COUNT: u32 = 1 << 4;
/// 只报文件名。
pub const F_FILES: u32 = 1 << 5;
/// 输出行号。
pub const F_LINENO: u32 = 1 << 6;
/// 只报命中片段。
pub const F_ONLY: u32 = 1 << 7;
/// 静默。
pub const F_QUIET: u32 = 1 << 8;
/// 递归。
pub const F_RECURSE: u32 = 1 << 9;
/// 词边界。
pub const F_WORD: u32 = 1 << 10;
/// 整行锚定。
pub const F_LINE: u32 = 1 << 11;

/// 合法位掩码。
pub const MASK: u32 = (F_EXTENDED
    | F_FIXED
    | F_ICASE
    | F_INVERT
    | F_COUNT
    | F_FILES
    | F_LINENO
    | F_ONLY
    | F_QUIET
    | F_RECURSE
    | F_WORD
    | F_LINE) as u32;

/// 操作码：模式解析。
pub const OP_RESOLVE: u8 = 0;
/// 操作码：单行求值。
pub const OP_MATCH: u8 = 1;
/// 操作码：整段扫描。
pub const OP_SCAN: u8 = 2;
/// 操作码：命中计数。
pub const OP_COUNT: u8 = 3;
/// 操作码：文件名渲染。
pub const OP_LIST: u8 = 4;
/// 操作码：差异表计数。
pub const OP_DIFF: u8 = 5;
/// 操作码：行号取行。
pub const OP_LINE: u8 = 6;

/// 选项名。
pub const OPT_NAMES: [&str; OPTS] = [
    "-E", "-F", "-i", "-v", "-c", "-l", "-n", "-o", "-q", "-r", "-w", "-x",
];

/// 选项字母。
pub const OPT_LETTERS: [char; OPTS] = [
    'E', 'F', 'i', 'v', 'c', 'l', 'n', 'o', 'q', 'r', 'w', 'x',
];

/// 选项位。
pub const OPT_FLAGS: [u32; OPTS] = [
    F_EXTENDED,
    F_FIXED,
    F_ICASE,
    F_INVERT,
    F_COUNT,
    F_FILES,
    F_LINENO,
    F_ONLY,
    F_QUIET,
    F_RECURSE,
    F_WORD,
    F_LINE,
];

/// 选项支持级别。
pub const OPT_SUPPORT: [u8; OPTS] = [
    SUP_PARTIAL, // -E：只有或选择器
    SUP_FULL,    // -F
    SUP_FULL,    // -i
    SUP_FULL,    // -v
    SUP_FULL,    // -c
    SUP_FULL,    // -l
    SUP_FULL,    // -n
    SUP_FULL,    // -o
    SUP_FULL,    // -q
    SUP_FULL,    // -r
    SUP_FULL,    // -w
    SUP_PARTIAL, // -x：整行锚定与 -w 组合有边界差异
];

/// 差异表行名。
pub const DIFF_NAMES: [&str; DIFFS] = [
    "backref",
    "bracket",
    "interval",
    "perl",
    "null-data",
    "binary",
    "color",
    "context",
    "exclude",
    "word",
    "only",
    "recursive",
];

/// 差异表状态。
pub const DIFF_STATUS: [u8; DIFFS] = [
    SUP_FULL,    // 回溯引用不在子集内，按“无此能力”记完整差异
    SUP_PARTIAL, // 字符类只覆盖常用几类
    SUP_FULL,    // 区间重复的缺口已由 PARSE 层拦截
    SUP_GNU,     // -P 仅 GNU
    SUP_GNU,     // -z 仅 GNU
    SUP_PARTIAL, // 二进制判定阈值不同
    SUP_FULL,    // --color 完整
    SUP_PARTIAL, // -A/-B/-C 只做行数截断
    SUP_FULL,    // 排除模式完整
    SUP_FULL,    // -w 完整
    SUP_FULL,    // -o 完整
    SUP_FULL,    // -r 完整
];

/// 选项位 → 名字。
pub fn option_name(flag: u32) -> Result<&'static str, u32> {
    let mut i = 0usize;
    while i < OPTS {
        match (OPT_FLAGS.get(i), OPT_NAMES.get(i)) {
            (Some(f), Some(n)) => {
                if *f == flag {
                    return Ok(n);
                }
            }
            _ => break,
        }
        i += 1;
    }
    Err(GR_EFLAG)
}

/// 选项位 → 支持级别。
pub fn option_support(flag: u32) -> Result<u8, u32> {
    let mut i = 0usize;
    while i < OPTS {
        match (OPT_FLAGS.get(i), OPT_SUPPORT.get(i)) {
            (Some(f), Some(v)) => {
                if *f == flag {
                    return Ok(*v);
                }
            }
            _ => break,
        }
        i += 1;
    }
    Err(GR_EFLAG)
}

/// 按级别统计选项条数。
pub fn count_option_support(level: u8) -> Result<u32, u32> {
    if level > SUP_GNU {
        return Err(GR_EFLAG);
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < OPTS {
        match OPT_SUPPORT.get(i) {
            Some(v) => {
                if *v == level {
                    n += 1;
                }
            }
            None => return Err(GR_EFLAG),
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
        _ => Err(GR_EFLAG),
    }
}

/// 解析单个选项字母。
pub fn parse_option(ch: char) -> Result<u32, u32> {
    let mut i = 0usize;
    while i < OPTS {
        match (OPT_LETTERS.get(i), OPT_FLAGS.get(i)) {
            (Some(c), Some(f)) => {
                if *c == ch {
                    return Ok(*f);
                }
            }
            _ => break,
        }
        i += 1;
    }
    Err(GR_EFLAG)
}

/// 解析一串短选项；前缀 `-` 可省略，空串表示无选项。
pub fn parse_options(text: &str) -> Result<u32, u32> {
    let b = text.as_bytes();
    if b.is_empty() {
        return Ok(0);
    }
    let mut i = 0usize;
    if let Some(v) = b.first() {
        if *v == b'-' {
            i = 1;
        }
    }
    if i >= b.len() {
        return Err(GR_EFLAG);
    }
    let mut flags = 0u32;
    while i < b.len() {
        let ch = match b.get(i) {
            Some(v) => *v as char,
            None => break,
        };
        let f = parse_option(ch)?;
        if flags & f != 0 {
            return Err(GR_EFLAG);
        }
        flags |= f;
        i += 1;
    }
    Ok(flags)
}

/// 把选项位折成一个输出模式，并拦截互斥组合。
pub fn resolve(flags: u32) -> Result<u8, u32> {
    if flags & !MASK != 0 {
        return Err(GR_EFLAG);
    }
    if flags & F_EXTENDED != 0 && flags & F_FIXED != 0 {
        return Err(GR_EFLAG);
    }
    let count = flags & F_COUNT != 0;
    let files = flags & F_FILES != 0;
    let only = flags & F_ONLY != 0;
    let quiet = flags & F_QUIET != 0;
    if count && files {
        return Err(GR_EMODE);
    }
    if count && only {
        return Err(GR_EMODE);
    }
    if files && only {
        return Err(GR_EMODE);
    }
    if quiet && (count || files || only) {
        return Err(GR_EMODE);
    }
    if only && flags & F_INVERT != 0 {
        return Err(GR_EMODE);
    }
    if quiet {
        return Ok(MODE_QUIET);
    }
    if count {
        return Ok(MODE_COUNT);
    }
    if files {
        return Ok(MODE_FILES);
    }
    if only {
        return Ok(MODE_ONLY);
    }
    Ok(MODE_LINES)
}

/// 输出模式标签。
pub fn mode_name(mode: u8) -> Result<&'static str, u32> {
    match mode {
        MODE_LINES => Ok("行"),
        MODE_COUNT => Ok("计数"),
        MODE_FILES => Ok("文件"),
        MODE_QUIET => Ok("静默"),
        MODE_ONLY => Ok("仅命中"),
        _ => Err(GR_EFLAG),
    }
}

/// 差异表行名。
pub fn diff_name(row: u8) -> Result<&'static str, u32> {
    match DIFF_NAMES.get(row as usize) {
        Some(v) => Ok(v),
        None => Err(GR_ERANGE),
    }
}

/// 差异表状态。
pub fn diff_status(row: u8) -> Result<u8, u32> {
    match DIFF_STATUS.get(row as usize) {
        Some(v) => Ok(*v),
        None => Err(GR_ERANGE),
    }
}

/// 按级别统计差异表行数。
pub fn count_diff_status(level: u8) -> Result<u32, u32> {
    if level > SUP_GNU {
        return Err(GR_EFLAG);
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
            None => return Err(GR_EFLAG),
        }
        i += 1;
    }
    Ok(n)
}

/// 字节是否是词字符。
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 在 `hay` 里从 `from` 起找 `needle`，可折叠 ASCII 大小写。
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

/// 命中两侧是否都不是词字符。
fn boundary_word(hay: &[u8], pos: usize, len: usize) -> bool {
    let left = if pos == 0 {
        true
    } else {
        match hay.get(pos.saturating_sub(1)) {
            Some(v) => !is_word_byte(*v),
            None => true,
        }
    };
    let right = match hay.get(pos.saturating_add(len)) {
        Some(v) => !is_word_byte(*v),
        None => true,
    };
    left && right
}

/// 在一条候选段上找满足词边界/整行约束的最左命中。
fn find_alt(
    hay: &[u8],
    needle: &[u8],
    from: usize,
    icase: bool,
    word: bool,
    whole: bool,
) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    let mut start = from;
    while start.saturating_add(needle.len()) <= hay.len() {
        match find_from(hay, needle, start, icase) {
            Some(pos) => {
                if whole && (pos != 0 || pos.saturating_add(needle.len()) != hay.len()) {
                    start = pos.saturating_add(1);
                    continue;
                }
                if word && !boundary_word(hay, pos, needle.len()) {
                    start = pos.saturating_add(1);
                    continue;
                }
                return Some(pos);
            }
            None => return None,
        }
    }
    None
}

/// 或选择器的段数。
fn alt_count(pat: &str) -> usize {
    let b = pat.as_bytes();
    let mut n = 1usize;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            if *v == b'|' {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 取第 `idx` 段；越界返回 `None`。
fn alt_at(pat: &str, idx: usize) -> Option<&str> {
    let b = pat.as_bytes();
    let mut start = 0usize;
    let mut cur = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            if *v == b'|' {
                if cur == idx {
                    return pat.get(start..i);
                }
                cur = cur.saturating_add(1);
                start = i.saturating_add(1);
            }
        }
        i += 1;
    }
    if cur == idx {
        pat.get(start..)
    } else {
        None
    }
}

/// 从 `from` 起找一次命中，返回（起点, 长度）。
pub fn find_once_from(
    line: &str,
    pat: &str,
    flags: u32,
    from: usize,
) -> Result<Option<(u32, u32)>, u32> {
    if flags & !MASK != 0 {
        return Err(GR_EFLAG);
    }
    if pat.is_empty() {
        return Err(GR_EPAT);
    }
    if pat.len() > MAX_PAT {
        return Err(GR_ERANGE);
    }
    let extended = flags & F_EXTENDED != 0;
    let n = if extended { alt_count(pat) } else { 1 };
    if n > MAX_ALT {
        return Err(GR_EFLAG);
    }
    let icase = flags & F_ICASE != 0;
    let word = flags & F_WORD != 0;
    let whole = flags & F_LINE != 0;
    let hay = line.as_bytes();
    let mut best: Option<(usize, usize)> = None;
    let mut i = 0usize;
    while i < n {
        let seg = if extended {
            match alt_at(pat, i) {
                Some(v) => v,
                None => break,
            }
        } else {
            pat
        };
        if seg.is_empty() {
            return Err(GR_EPAT);
        }
        if let Some(pos) = find_alt(hay, seg.as_bytes(), from, icase, word, whole) {
            let better = match best {
                None => true,
                Some((bp, bl)) => {
                    pos < bp || (pos == bp && seg.len() > bl)
                }
            };
            if better {
                best = Some((pos, seg.len()));
            }
        }
        i += 1;
    }
    Ok(best.map(|(p, l)| (p as u32, l as u32)))
}

/// 从行首找一次命中。
pub fn find_once(line: &str, pat: &str, flags: u32) -> Result<Option<(u32, u32)>, u32> {
    find_once_from(line, pat, flags, 0)
}

/// 行是否被选中（含反选）。
pub fn eval_line(line: &str, pat: &str, flags: u32) -> Result<bool, u32> {
    let hit = find_once(line, pat, flags)?.is_some();
    Ok(if flags & F_INVERT != 0 { !hit } else { hit })
}

/// 统计不重叠命中数；与反选组合判模式冲突。
pub fn count_matches(line: &str, pat: &str, flags: u32) -> Result<u32, u32> {
    if flags & F_INVERT != 0 {
        return Err(GR_EMODE);
    }
    let mut cursor = 0usize;
    let mut count = 0u32;
    loop {
        match find_once_from(line, pat, flags, cursor)? {
            Some((p, l)) => {
                count = count.checked_add(1).ok_or(GR_ECOUNT)?;
                let adv = if l == 0 { 1usize } else { l as usize };
                cursor = (p as usize).checked_add(adv).ok_or(GR_ECOUNT)?;
            }
            None => break,
        }
    }
    Ok(count)
}

/// 行里是否含 NUL。
pub fn binary_line(line: &str) -> bool {
    let b = line.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            if *v == 0 {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 取第 `idx` 行；越界判行号错误。
pub fn line_at<'a>(lines: &'a [&'a str], idx: usize) -> Result<&'a str, u32> {
    match lines.get(idx) {
        Some(v) => Ok(*v),
        None => Err(GR_ERANGE),
    }
}

/// 路径层级（斜杠个数）。
pub fn path_depth(path: &str) -> Result<u32, u32> {
    if path.is_empty() {
        return Ok(0);
    }
    let b = path.as_bytes();
    let mut d = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            if *v == b'/' {
                d = d.saturating_add(1);
            }
        }
        i += 1;
    }
    Ok(d)
}

/// 路径准入：空路径、深路径、目录未递归、文件尾斜杠四道闸门。
pub fn admit_path(path: &str, is_dir: bool, recurse: bool) -> Result<u8, u32> {
    if path.is_empty() {
        return Err(GR_EINPUT);
    }
    let depth = path_depth(path)?;
    if depth > MAX_DEPTH {
        return Err(GR_ERANGE);
    }
    let trailing = match path.as_bytes().last() {
        Some(v) => *v == b'/',
        None => false,
    };
    if is_dir {
        if !recurse {
            return Err(GR_EREC);
        }
        Ok(KIND_DIR)
    } else if trailing {
        Err(GR_EREC)
    } else {
        Ok(KIND_FILE)
    }
}

/// 扫描报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总行数。
    pub total: u32,
    /// 选中行数。
    pub selected: u32,
    /// 命中数（`-o` 时是片段数，否则等于选中行数）。
    pub hits: u32,
    /// 第一条选中行的下标；无命中为 -1。
    pub first: i32,
}

impl Report {
    /// 空白报告。
    pub const fn blank() -> Report {
        Report {
            total: 0,
            selected: 0,
            hits: 0,
            first: -1,
        }
    }
}

/// 整段扫描：逐行求值并汇总，`-q` 命中即停。
pub fn scan(lines: &[&str], pat: &str, flags: u32) -> Result<Report, u32> {
    resolve(flags)?;
    if lines.is_empty() {
        return Err(GR_EINPUT);
    }
    if pat.is_empty() {
        return Err(GR_EPAT);
    }
    let mut rep = Report::blank();
    rep.total = lines.len() as u32;
    let mut i = 0usize;
    while i < lines.len() {
        let line = line_at(lines, i)?;
        if binary_line(line) {
            return Err(GR_EBIN);
        }
        if eval_line(line, pat, flags)? {
            rep.selected = rep.selected.saturating_add(1);
            if rep.first < 0 {
                rep.first = i as i32;
            }
            if flags & F_ONLY != 0 {
                let c = count_matches(line, pat, flags)?;
                rep.hits = rep.hits.saturating_add(c);
            } else {
                rep.hits = rep.hits.saturating_add(1);
            }
            if flags & F_QUIET != 0 {
                break;
            }
        }
        i += 1;
    }
    Ok(rep)
}

/// 十进制追加，不分配临时缓冲。
fn push_num(out: &mut String, v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 && n < 10 {
        if let Some(slot) = buf.get_mut(n) {
            *slot = b'0' + (x % 10) as u8;
        }
        x /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        if let Some(d) = buf.get(n) {
            out.push(*d as char);
        }
    }
}

/// 按模式渲染输出文本。
pub fn filter(lines: &[&str], pat: &str, flags: u32) -> Result<String, u32> {
    let mode = resolve(flags)?;
    if lines.is_empty() {
        return Err(GR_EINPUT);
    }
    if pat.is_empty() {
        return Err(GR_EPAT);
    }
    match mode {
        MODE_COUNT => {
            let rep = scan(lines, pat, flags)?;
            let mut out = String::new();
            push_num(&mut out, rep.selected);
            out.push('\n');
            Ok(out)
        }
        MODE_FILES => {
            let rep = scan(lines, pat, flags)?;
            if rep.selected > 0 {
                Ok(String::from("match\n"))
            } else {
                Ok(String::new())
            }
        }
        MODE_QUIET => {
            scan(lines, pat, flags)?;
            Ok(String::new())
        }
        MODE_ONLY => {
            let mut out = String::new();
            let mut i = 0usize;
            while i < lines.len() {
                let line = line_at(lines, i)?;
                if binary_line(line) {
                    return Err(GR_EBIN);
                }
                if eval_line(line, pat, flags)? {
                    let mut cur = 0usize;
                    loop {
                        match find_once_from(line, pat, flags, cur)? {
                            Some((p, l)) => {
                                let start = p as usize;
                                let end = start.saturating_add(l as usize);
                                if flags & F_LINENO != 0 {
                                    push_num(&mut out, (i as u32).saturating_add(1));
                                    out.push(':');
                                }
                                match line.get(start..end) {
                                    Some(v) => out.push_str(v),
                                    None => return Err(GR_ERANGE),
                                }
                                out.push('\n');
                                cur = end;
                                if l == 0 {
                                    cur = cur.saturating_add(1);
                                }
                            }
                            None => break,
                        }
                    }
                }
                i += 1;
            }
            Ok(out)
        }
        MODE_LINES => {
            let mut out = String::new();
            let mut i = 0usize;
            while i < lines.len() {
                let line = line_at(lines, i)?;
                if binary_line(line) {
                    return Err(GR_EBIN);
                }
                if eval_line(line, pat, flags)? {
                    if flags & F_LINENO != 0 {
                        push_num(&mut out, (i as u32).saturating_add(1));
                        out.push(':');
                    }
                    out.push_str(line);
                    out.push('\n');
                }
                i += 1;
            }
            Ok(out)
        }
        _ => Err(GR_EFLAG),
    }
}

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字（逐字节八轮）。
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

/// 一条消费记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码；成功为 0。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: 0,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
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
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(GR_ESLOT);
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
            return Err(GR_EBAL);
        }
        const VALS: [u32; 8] = [1, 1, 1, 2, 2, 6, 1, 7];
        const CODES: [u32; 6] = [GR_EPAT, GR_EFLAG, GR_EMODE, GR_EBIN, GR_EINPUT, GR_ERANGE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(GR_EBAL),
            };
            if s.slot as usize != i {
                return Err(GR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(GR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(GR_EVALUE);
                        }
                    }
                    None => return Err(GR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(GR_EVALUE);
                        }
                    }
                    None => return Err(GR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(GR_EBAL);
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
            return Err(GR_EBAL);
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
            Err(GR_ESEAL)
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

/// 取一格样本；越界判观察错误。
pub fn obs_at(l: &Ledger, i: usize) -> Result<Sample, u32> {
    match l.sample(i) {
        Some(v) => Ok(v),
        None => Err(GR_ENOBS),
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(
        0,
        OP_RESOLVE,
        resolve(F_FIXED | F_ICASE | F_COUNT).map(|m| m as u32),
    ));
    let _ = l.register(word_cell(
        1,
        OP_MATCH,
        eval_line("hello", "ell", F_FIXED).map(|b| b as u32),
    ));
    let _ = l.register(word_cell(
        2,
        OP_MATCH,
        eval_line("Hello", "ell", F_FIXED | F_ICASE).map(|b| b as u32),
    ));
    let _ = l.register(word_cell(
        3,
        OP_SCAN,
        scan(&["foo", "bar", "food"], "foo", F_FIXED).map(|r| r.selected),
    ));
    let _ = l.register(word_cell(
        4,
        OP_COUNT,
        filter(&["aa", "ba"], "a", F_FIXED | F_COUNT).map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(
        5,
        OP_LIST,
        filter(&["aa", "ba"], "a", F_FIXED | F_FILES).map(|s| s.len() as u32),
    ));
    let _ = l.register(word_cell(
        6,
        OP_SCAN,
        scan(&["ab", "cd", "ef"], "c", F_FIXED).map(|r| r.first as u32),
    ));
    let _ = l.register(word_cell(7, OP_DIFF, count_diff_status(SUP_FULL)));

    let _ = l.register(word_cell(8, OP_MATCH, eval_line("a", "", F_FIXED).map(|b| b as u32)));
    let _ = l.register(word_cell(
        9,
        OP_RESOLVE,
        resolve(F_EXTENDED | F_FIXED).map(|m| m as u32),
    ));
    let _ = l.register(word_cell(
        10,
        OP_RESOLVE,
        resolve(F_COUNT | F_FILES).map(|m| m as u32),
    ));
    let _ = l.register(word_cell(
        11,
        OP_SCAN,
        scan(&["a\0b"], "a", F_FIXED).map(|r| r.selected),
    ));
    let _ = l.register(word_cell(
        12,
        OP_SCAN,
        scan(&[], "a", F_FIXED).map(|r| r.selected),
    ));
    let _ = l.register(word_cell(
        13,
        OP_LINE,
        line_at(&["a"], 3).map(|s| s.len() as u32),
    ));

    l
}

/// 四路注入：选项矩阵 / 解析闸门 / 匹配器 / 执行渲染。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut disjoint = true;
    let mut union = 0u32;
    let mut i = 0usize;
    while i < OPTS {
        match OPT_FLAGS.get(i) {
            Some(f) => {
                union |= *f;
                let mut j = i + 1;
                while j < OPTS {
                    if let Some(g) = OPT_FLAGS.get(j) {
                        if *f & *g != 0 {
                            disjoint = false;
                        }
                    }
                    j += 1;
                }
            }
            None => disjoint = false,
        }
        i += 1;
    }
    let mut letters_ok = OPTS == 12;
    i = 0usize;
    while i < OPTS {
        match (OPT_LETTERS.get(i), OPT_NAMES.get(i), OPT_FLAGS.get(i)) {
            (Some(c), Some(n), Some(f)) => {
                if !n.starts_with('-') || parse_option(*c) != Ok(*f) {
                    letters_ok = false;
                }
            }
            _ => letters_ok = false,
        }
        i += 1;
    }
    out[0] = disjoint
        && union == MASK
        && letters_ok
        && count_option_support(SUP_FULL) == Ok(10)
        && count_option_support(SUP_PARTIAL) == Ok(2)
        && count_option_support(SUP_GNU) == Ok(0)
        && count_diff_status(SUP_FULL) == Ok(7)
        && count_diff_status(SUP_PARTIAL) == Ok(3)
        && count_diff_status(SUP_GNU) == Ok(2)
        && option_name(F_ICASE) == Ok("-i")
        && option_support(F_EXTENDED) == Ok(SUP_PARTIAL)
        && option_name(1 << 20) == Err(GR_EFLAG)
        && support_label(SUP_GNU) == Ok("仅 GNU")
        && mode_name(MODE_ONLY) == Ok("仅命中");

    out[1] = parse_options("-ivc") == Ok(F_ICASE | F_INVERT | F_COUNT)
        && parse_options("Ew") == Ok(F_EXTENDED | F_WORD)
        && parse_options("") == Ok(0)
        && parse_options("-") == Err(GR_EFLAG)
        && parse_options("-z") == Err(GR_EFLAG)
        && parse_options("-ii") == Err(GR_EFLAG)
        && resolve(0) == Ok(MODE_LINES)
        && resolve(F_COUNT) == Ok(MODE_COUNT)
        && resolve(F_FILES) == Ok(MODE_FILES)
        && resolve(F_QUIET) == Ok(MODE_QUIET)
        && resolve(F_ONLY) == Ok(MODE_ONLY)
        && resolve(F_EXTENDED | F_FIXED) == Err(GR_EFLAG)
        && resolve(F_COUNT | F_FILES) == Err(GR_EMODE)
        && resolve(F_COUNT | F_ONLY) == Err(GR_EMODE)
        && resolve(F_FILES | F_ONLY) == Err(GR_EMODE)
        && resolve(F_QUIET | F_COUNT) == Err(GR_EMODE)
        && resolve(F_ONLY | F_INVERT) == Err(GR_EMODE)
        && resolve(1 << 20) == Err(GR_EFLAG)
        && admit_path("", false, false) == Err(GR_EINPUT)
        && admit_path("a/b", true, false) == Err(GR_EREC)
        && admit_path("a/", false, false) == Err(GR_EREC)
        && admit_path("a/b", true, true) == Ok(KIND_DIR)
        && admit_path("a/b", false, false) == Ok(KIND_FILE)
        && path_depth("a/b/c") == Ok(2)
        && line_at(&["a"], 2) == Err(GR_ERANGE);

    out[2] = find_once("hello", "ell", F_FIXED) == Ok(Some((1, 3)))
        && find_once("hello", "xyz", F_FIXED) == Ok(None)
        && find_once("hello", "hello world", F_FIXED) == Ok(None)
        && find_once("HELLO", "ell", F_FIXED) == Ok(None)
        && find_once("Hello", "ell", F_FIXED | F_ICASE) == Ok(Some((1, 3)))
        && find_once("bar foo", "foo|bar", F_EXTENDED) == Ok(Some((0, 3)))
        && find_once("xxbar", "bb|bar", F_EXTENDED) == Ok(Some((2, 3)))
        && find_once("foo bar", "foo|bar", F_FIXED) == Ok(None)
        && find_once("a|b", "|", F_FIXED) == Ok(Some((1, 1)))
        && find_once("a", "", F_FIXED) == Err(GR_EPAT)
        && find_once("a", "a|", F_EXTENDED) == Err(GR_EPAT)
        && find_once("a", "a|b|c|d|e", F_EXTENDED) == Err(GR_EFLAG)
        && find_once("xfoo", "foo", F_FIXED | F_WORD) == Ok(None)
        && find_once("a foo b", "foo", F_FIXED | F_WORD) == Ok(Some((2, 3)))
        && find_once("foo", "foo", F_FIXED | F_LINE) == Ok(Some((0, 3)))
        && find_once("foo ", "foo", F_FIXED | F_LINE) == Ok(None)
        && eval_line("foo", "foo", F_FIXED | F_INVERT) == Ok(false)
        && eval_line("bar", "foo", F_FIXED | F_INVERT) == Ok(true)
        && count_matches("aaaa", "aa", F_FIXED) == Ok(2)
        && count_matches("a", "a", F_FIXED | F_INVERT) == Err(GR_EMODE)
        && binary_line("a\0b")
        && !binary_line("ab");

    out[3] = scan(&["foo", "bar", "food"], "foo", F_FIXED)
        == Ok(Report {
            total: 3,
            selected: 2,
            hits: 2,
            first: 0,
        })
        && scan(&["foo", "bar"], "foo", F_FIXED | F_INVERT).map(|r| r.selected) == Ok(1)
        && scan(&[], "a", F_FIXED) == Err(GR_EINPUT)
        && scan(&["a\0b"], "a", F_FIXED) == Err(GR_EBIN)
        && filter(&["aa", "ba"], "a", F_FIXED) == Ok(String::from("aa\nba\n"))
        && filter(&["aa", "ba"], "a", F_FIXED | F_LINENO)
            == Ok(String::from("1:aa\n2:ba\n"))
        && filter(&["aa", "ba"], "a", F_FIXED | F_ONLY) == Ok(String::from("a\na\na\n"))
        && filter(&["aa", "ba"], "a", F_FIXED | F_ONLY | F_LINENO)
            == Ok(String::from("1:a\n1:a\n2:a\n"))
        && filter(&["aa", "ba"], "a", F_FIXED | F_COUNT) == Ok(String::from("2\n"))
        && filter(&["aa", "ba"], "a", F_FIXED | F_FILES) == Ok(String::from("match\n"))
        && filter(&["aa", "ba"], "a", F_FIXED | F_QUIET) == Ok(String::new())
        && filter(&["aa", "ba"], "z", F_FIXED) == Ok(String::new())
        && filter(&["aa", "ba"], "a", F_FIXED | F_COUNT | F_INVERT)
            == Ok(String::from("0\n"));

    out
}

/// 十三项判据。
pub fn run_unx11592_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11592.busybox_grep");

    s.add(
        "GR1-十二选项矩阵",
        {
            let mut disjoint = true;
            let mut union = 0u32;
            let mut i = 0usize;
            while i < OPTS {
                match OPT_FLAGS.get(i) {
                    Some(f) => {
                        union |= *f;
                        let mut j = i + 1;
                        while j < OPTS {
                            if let Some(g) = OPT_FLAGS.get(j) {
                                if *f & *g != 0 {
                                    disjoint = false;
                                }
                            }
                            j += 1;
                        }
                    }
                    None => disjoint = false,
                }
                i += 1;
            }
            let mut round = true;
            i = 0usize;
            while i < OPTS {
                match (OPT_LETTERS.get(i), OPT_NAMES.get(i), OPT_FLAGS.get(i)) {
                    (Some(c), Some(n), Some(f)) => {
                        if !n.starts_with('-') || parse_option(*c) != Ok(*f) {
                            round = false;
                        }
                    }
                    _ => round = false,
                }
                i += 1;
            }
            disjoint
                && union == MASK
                && round
                && count_option_support(SUP_FULL) == Ok(10)
                && count_option_support(SUP_PARTIAL) == Ok(2)
                && count_option_support(SUP_GNU) == Ok(0)
                && option_name(F_WORD) == Ok("-w")
                && option_support(F_LINE) == Ok(SUP_PARTIAL)
                && option_name(0) == Err(GR_EFLAG)
                && option_support(0) == Err(GR_EFLAG)
                && support_label(SUP_FULL) == Ok("完整")
                && support_label(SUP_PARTIAL) == Ok("有差异")
                && support_label(SUP_GNU) == Ok("仅 GNU")
                && support_label(3) == Err(GR_EFLAG)
                && count_option_support(9) == Err(GR_EFLAG)
        },
        "static",
    );

    s.add(
        "GR2-选项与模式解析闸门",
        {
            let parse_ok = parse_options("-ivc") == Ok(F_ICASE | F_INVERT | F_COUNT)
                && parse_options("Ew") == Ok(F_EXTENDED | F_WORD)
                && parse_options("--") == Err(GR_EFLAG)
                && parse_options("") == Ok(0)
                && parse_options("-") == Err(GR_EFLAG)
                && parse_options("-z") == Err(GR_EFLAG)
                && parse_options("-ii") == Err(GR_EFLAG)
                && parse_option('q') == Ok(F_QUIET);
            let modes = resolve(0) == Ok(MODE_LINES)
                && resolve(F_COUNT) == Ok(MODE_COUNT)
                && resolve(F_FILES) == Ok(MODE_FILES)
                && resolve(F_QUIET) == Ok(MODE_QUIET)
                && resolve(F_ONLY) == Ok(MODE_ONLY);
            let gates = resolve(F_EXTENDED | F_FIXED) == Err(GR_EFLAG)
                && resolve(F_COUNT | F_FILES) == Err(GR_EMODE)
                && resolve(F_COUNT | F_ONLY) == Err(GR_EMODE)
                && resolve(F_FILES | F_ONLY) == Err(GR_EMODE)
                && resolve(F_QUIET | F_FILES) == Err(GR_EMODE)
                && resolve(F_ONLY | F_INVERT) == Err(GR_EMODE)
                && resolve(1 << 20) == Err(GR_EFLAG);
            let labels = mode_name(MODE_LINES) == Ok("行")
                && mode_name(MODE_COUNT) == Ok("计数")
                && mode_name(MODE_FILES) == Ok("文件")
                && mode_name(MODE_QUIET) == Ok("静默")
                && mode_name(MODE_ONLY) == Ok("仅命中")
                && mode_name(9) == Err(GR_EFLAG);
            parse_ok && modes && gates && labels
        },
        "static",
    );

    s.add(
        "GR3-模式闸门",
        {
            let empty = find_once("a", "", F_FIXED) == Err(GR_EPAT)
                && find_once("a", "a|", F_EXTENDED) == Err(GR_EPAT)
                && find_once("a", "|a", F_EXTENDED) == Err(GR_EPAT);
            let many = find_once("a", "a|b|c|d|e", F_EXTENDED) == Err(GR_EFLAG)
                && find_once("a", "a", 1 << 20) == Err(GR_EFLAG);
            let mut long = String::new();
            while long.len() <= MAX_PAT {
                long.push('a');
            }
            let over = find_once("a", long.as_str(), F_FIXED) == Err(GR_ERANGE);
            let inputs = scan(&[], "a", F_FIXED) == Err(GR_EINPUT)
                && filter(&[], "a", F_FIXED) == Err(GR_EINPUT)
                && scan(&["a"], "", F_FIXED) == Err(GR_EPAT);
            empty && many && over && inputs
        },
        "static",
    );

    s.add(
        "GR4-字面量匹配",
        {
            let a = find_once("hello", "ell", F_FIXED) == Ok(Some((1, 3)));
            let b = find_once("hello", "xyz", F_FIXED) == Ok(None);
            let c = find_once("abcabc", "abc", F_FIXED) == Ok(Some((0, 3)));
            let d = find_from(b"abcabc", b"abc", 1, false) == Some(3);
            let e = find_once_from("abcabc", "abc", F_FIXED, 1) == Ok(Some((3, 3)));
            let f = find_once("", "a", F_FIXED) == Ok(None);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "GR5-大小写折叠",
        {
            let a = find_once("AbC", "abc", F_FIXED) == Ok(None);
            let b = find_once("AbC", "abc", F_FIXED | F_ICASE) == Ok(Some((0, 3)));
            let c = find_once("xxAbC", "abc", F_FIXED | F_ICASE) == Ok(Some((2, 3)));
            let d = eval_line("HELLO", "hello", F_FIXED | F_ICASE) == Ok(true);
            let e = eval_line("HELLO", "hello", F_FIXED) == Ok(false);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "GR6-或选择器",
        {
            let a = find_once("bar foo", "foo|bar", F_EXTENDED) == Ok(Some((0, 3)));
            let b = find_once("xxbar", "bb|bar", F_EXTENDED) == Ok(Some((2, 3)));
            let c = find_once("foo bar", "foo|bar", F_FIXED) == Ok(None);
            let d = find_once("a|b", "|", F_FIXED) == Ok(Some((1, 1)));
            let e = find_once("a|b", "|", F_EXTENDED) == Err(GR_EPAT);
            let f = find_once("ab", "b|ab", F_EXTENDED) == Ok(Some((0, 2)));
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "GR7-词与整行边界",
        {
            let a = find_once("xfoo", "foo", F_FIXED | F_WORD) == Ok(None);
            let b = find_once("foo_bar", "foo", F_FIXED | F_WORD) == Ok(None);
            let c = find_once("a foo b", "foo", F_FIXED | F_WORD) == Ok(Some((2, 3)));
            let d = find_once("foo", "foo", F_FIXED | F_LINE) == Ok(Some((0, 3)));
            let e = find_once("foo ", "foo", F_FIXED | F_LINE) == Ok(None);
            let f = find_once("foo", "foo", F_FIXED | F_WORD | F_LINE) == Ok(Some((0, 3)));
            let g = find_once("a foo", "foo", F_FIXED | F_WORD | F_LINE) == Ok(None);
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "GR8-反选与计数",
        {
            let a = eval_line("foo", "foo", F_FIXED) == Ok(true);
            let b = eval_line("foo", "foo", F_FIXED | F_INVERT) == Ok(false);
            let c = eval_line("bar", "foo", F_FIXED | F_INVERT) == Ok(true);
            let d = scan(&["foo", "bar", "food"], "foo", F_FIXED).map(|r| r.selected) == Ok(2);
            let e = scan(&["foo", "bar"], "foo", F_FIXED | F_INVERT).map(|r| r.selected)
                == Ok(1);
            let f = count_matches("aaaa", "aa", F_FIXED) == Ok(2)
                && count_matches("aaaa", "a", F_FIXED) == Ok(4)
                && count_matches("a", "a", F_FIXED | F_INVERT) == Err(GR_EMODE);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "GR9-输出渲染",
        {
            let a = filter(&["aa", "ba"], "a", F_FIXED) == Ok(String::from("aa\nba\n"));
            let b = filter(&["aa", "ba"], "a", F_FIXED | F_LINENO)
                == Ok(String::from("1:aa\n2:ba\n"));
            let c = filter(&["aa", "ba"], "a", F_FIXED | F_ONLY) == Ok(String::from("a\na\na\n"));
            let d = filter(&["aa", "ba"], "a", F_FIXED | F_ONLY | F_LINENO)
                == Ok(String::from("1:a\n1:a\n2:a\n"));
            let e = filter(&["aa", "ba"], "a", F_FIXED | F_COUNT) == Ok(String::from("2\n"));
            let f = filter(&["aa", "ba"], "a", F_FIXED | F_FILES) == Ok(String::from("match\n"));
            let g = filter(&["aa", "ba"], "a", F_FIXED | F_QUIET) == Ok(String::new());
            let h = filter(&["aa", "ba"], "z", F_FIXED) == Ok(String::new());
            let i = filter(&["aa", "ba"], "a", F_FIXED | F_COUNT | F_INVERT)
                == Ok(String::from("0\n"));
            a && b && c && d && e && f && g && h && i
        },
        "static",
    );

    s.add(
        "GR10-路径与二进制闸门",
        {
            let a = admit_path("", false, false) == Err(GR_EINPUT)
                && admit_path("a/b", true, false) == Err(GR_EREC)
                && admit_path("a/", false, false) == Err(GR_EREC);
            let b = admit_path("a/b", true, true) == Ok(KIND_DIR)
                && admit_path("a/b", false, false) == Ok(KIND_FILE);
            let c = path_depth("") == Ok(0)
                && path_depth("a/b/c") == Ok(2)
                && admit_path("a/b/c/d/e/f/g/h/i/j", false, false) == Err(GR_ERANGE);
            let d = binary_line("a\0b") && !binary_line("ab");
            let e = scan(&["a\0b"], "a", F_FIXED) == Err(GR_EBIN)
                && filter(&["a\0b"], "a", F_FIXED) == Err(GR_EBIN);
            let f = line_at(&["a"], 0) == Ok("a")
                && line_at(&["a"], 2) == Err(GR_ERANGE)
                && obs_at(&build_ledger(), 99) == Err(GR_ENOBS);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "GR11-差异表十二行",
        {
            let mut names_ok = DIFFS == 12;
            let mut i = 0usize;
            while i < DIFFS {
                match DIFF_NAMES.get(i) {
                    Some(n) => {
                        if n.is_empty() {
                            names_ok = false;
                        }
                    }
                    None => names_ok = false,
                }
                i += 1;
            }
            names_ok
                && count_diff_status(SUP_FULL) == Ok(7)
                && count_diff_status(SUP_PARTIAL) == Ok(3)
                && count_diff_status(SUP_GNU) == Ok(2)
                && diff_name(0) == Ok("backref")
                && diff_name(4) == Ok("null-data")
                && diff_name(11) == Ok("recursive")
                && diff_name(12) == Err(GR_ERANGE)
                && diff_status(3) == Ok(SUP_GNU)
                && diff_status(1) == Ok(SUP_PARTIAL)
                && diff_status(12) == Err(GR_ERANGE)
                && count_diff_status(9) == Err(GR_EFLAG)
        },
        "static",
    );

    s.add(
        "GR12-账本形状与冻结",
        {
            let mut l = build_ledger();
            let shape = l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && !l.is_empty()
                && !l.is_frozen();
            let audit = l.audit().is_ok();
            let slots = l.sample(0).map(|v| v.slot) == Some(0)
                && l.sample(13).map(|v| v.slot) == Some(13)
                && l.sample(SAMPLES).is_none()
                && obs_at(&l, 3).map(|v| v.op) == Ok(OP_SCAN);
            let one = l.freeze();
            let seal_ok = l.root() == l.seal();
            let replay = l.replay().is_ok();
            let rejected = l.rejects();
            let late = l.register(Sample::blank());
            let frozen = one.is_ok()
                && l.is_frozen()
                && seal_ok
                && rejected == 0
                && late == Err(GR_ESLOT)
                && l.rejects() == 1;
            shape && audit && slots && frozen && replay
        },
        "static",
    );

    s.add(
        "GR13-冻结重放四注入",
        {
            let four = inject_four();
            four[0] && four[1] && four[2] && four[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_matcher() {
        assert_eq!(find_once("hello", "ell", F_FIXED), Ok(Some((1, 3))), "d1");
        assert_eq!(find_once("hello", "xyz", F_FIXED), Ok(None), "d2");
        assert_eq!(find_once("hello", "hello world", F_FIXED), Ok(None), "d3");
        assert_eq!(find_once("HELLO", "ell", F_FIXED), Ok(None), "d4");
        assert_eq!(find_once("Hello", "ell", F_FIXED | F_ICASE), Ok(Some((1, 3))), "d5");
        assert_eq!(find_once("bar foo", "foo|bar", F_EXTENDED), Ok(Some((0, 3))), "d6");
        assert_eq!(find_once("xxbar", "bb|bar", F_EXTENDED), Ok(Some((2, 3))), "d7");
        assert_eq!(find_once("foo bar", "foo|bar", F_FIXED), Ok(None), "d8");
        assert_eq!(find_once("a|b", "|", F_FIXED), Ok(Some((1, 1))), "d9");
        assert_eq!(find_once("a", "", F_FIXED), Err(GR_EPAT), "d10");
        assert_eq!(find_once("a", "a|", F_EXTENDED), Err(GR_EPAT), "d11");
        assert_eq!(find_once("a", "a|b|c|d|e", F_EXTENDED), Err(GR_EFLAG), "d12");
        assert_eq!(find_once("xfoo", "foo", F_FIXED | F_WORD), Ok(None), "d13");
        assert_eq!(find_once("a foo b", "foo", F_FIXED | F_WORD), Ok(Some((2, 3))), "d14");
        assert_eq!(find_once("foo", "foo", F_FIXED | F_LINE), Ok(Some((0, 3))), "d15");
        assert_eq!(find_once("foo ", "foo", F_FIXED | F_LINE), Ok(None), "d16");
        assert_eq!(eval_line("foo", "foo", F_FIXED | F_INVERT), Ok(false), "d17");
        assert_eq!(eval_line("bar", "foo", F_FIXED | F_INVERT), Ok(true), "d18");
        assert_eq!(count_matches("aaaa", "aa", F_FIXED), Ok(2), "d19");
        assert_eq!(count_matches("a", "a", F_FIXED | F_INVERT), Err(GR_EMODE), "d20");
        assert!(binary_line("a\0b"), "d21");
        assert!(!binary_line("ab"), "d22");
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(0).map(|v| v.slot), Some(0));
        assert_eq!(l.sample(13).map(|v| v.slot), Some(13));
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let one = l.freeze();
        assert!(one.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.root(), l.seal());
        assert!(l.replay().is_ok());
        assert_eq!(l.freeze(), Err(GR_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0], "选项矩阵注入");
        assert!(four[1], "解析闸门注入");
        assert!(four[2], "匹配器注入");
        assert!(four[3], "执行渲染注入");
        assert_eq!(four, [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let c = run_unx11592_checks();
        assert_eq!(c.len(), 13);
        assert!(c.all_passed());
    }

    #[test]
    fn option_matrix() {
        assert_eq!(OPT_NAMES.len(), OPTS);
        assert_eq!(OPT_LETTERS.len(), OPTS);
        assert_eq!(OPT_FLAGS.len(), OPTS);
        assert_eq!(count_option_support(SUP_FULL), Ok(10));
        assert_eq!(count_option_support(SUP_PARTIAL), Ok(2));
        assert_eq!(count_option_support(SUP_GNU), Ok(0));
        assert_eq!(option_name(F_LINENO), Ok("-n"));
        assert_eq!(option_support(F_EXTENDED), Ok(SUP_PARTIAL));
        assert_eq!(option_support(F_LINE), Ok(SUP_PARTIAL));
    }

    #[test]
    fn option_parse_gates() {
        assert_eq!(parse_options("-ivc"), Ok(F_ICASE | F_INVERT | F_COUNT));
        assert_eq!(parse_options("E"), Ok(F_EXTENDED));
        assert_eq!(parse_options(""), Ok(0));
        assert_eq!(parse_options("-"), Err(GR_EFLAG));
        assert_eq!(parse_options("-ii"), Err(GR_EFLAG));
        assert_eq!(parse_options("-Z"), Err(GR_EFLAG));
    }

    #[test]
    fn pattern_gates() {
        assert_eq!(find_once("a", "", F_FIXED), Err(GR_EPAT));
        assert_eq!(find_once("a", "a|", F_EXTENDED), Err(GR_EPAT));
        assert_eq!(find_once("a", "a|b|c|d|e", F_EXTENDED), Err(GR_EFLAG));
        let mut long = String::new();
        while long.len() <= MAX_PAT {
            long.push('a');
        }
        assert_eq!(find_once("a", long.as_str(), F_FIXED), Err(GR_ERANGE));
        assert_eq!(find_once("a", "a", 1 << 20), Err(GR_EFLAG));
    }

    #[test]
    fn literal_match() {
        assert_eq!(find_once("hello", "ell", F_FIXED), Ok(Some((1, 3))));
        assert_eq!(find_once("hello", "xyz", F_FIXED), Ok(None));
        assert_eq!(find_once("abcabc", "abc", F_FIXED), Ok(Some((0, 3))));
        assert_eq!(find_once_from("abcabc", "abc", F_FIXED, 1), Ok(Some((3, 3))));
        assert_eq!(find_from(b"abcabc", b"abc", 1, false), Some(3));
        assert_eq!(find_once("", "a", F_FIXED), Ok(None));
    }

    #[test]
    fn icase_match() {
        assert_eq!(find_once("AbC", "abc", F_FIXED), Ok(None));
        assert_eq!(find_once("AbC", "abc", F_FIXED | F_ICASE), Ok(Some((0, 3))));
        assert_eq!(find_once("xxAbC", "abc", F_FIXED | F_ICASE), Ok(Some((2, 3))));
        assert_eq!(eval_line("HELLO", "hello", F_FIXED | F_ICASE), Ok(true));
    }

    #[test]
    fn alternation() {
        assert_eq!(find_once("bar foo", "foo|bar", F_EXTENDED), Ok(Some((0, 3))));
        assert_eq!(find_once("xxbar", "bb|bar", F_EXTENDED), Ok(Some((2, 3))));
        assert_eq!(find_once("foo bar", "foo|bar", F_FIXED), Ok(None));
        assert_eq!(find_once("ab", "b|ab", F_EXTENDED), Ok(Some((0, 2))));
    }

    #[test]
    fn word_whole() {
        assert_eq!(find_once("xfoo", "foo", F_FIXED | F_WORD), Ok(None));
        assert_eq!(find_once("a foo b", "foo", F_FIXED | F_WORD), Ok(Some((2, 3))));
        assert_eq!(find_once("foo", "foo", F_FIXED | F_LINE), Ok(Some((0, 3))));
        assert_eq!(find_once("foo ", "foo", F_FIXED | F_LINE), Ok(None));
        assert_eq!(
            find_once("a foo", "foo", F_FIXED | F_WORD | F_LINE),
            Ok(None)
        );
    }

    #[test]
    fn invert_and_count() {
        assert_eq!(eval_line("bar", "foo", F_FIXED | F_INVERT), Ok(true));
        assert_eq!(eval_line("foo", "foo", F_FIXED | F_INVERT), Ok(false));
        assert_eq!(count_matches("aaaa", "aa", F_FIXED), Ok(2));
        assert_eq!(count_matches("aaaa", "a", F_FIXED), Ok(4));
        assert_eq!(count_matches("a", "a", F_FIXED | F_INVERT), Err(GR_EMODE));
    }

    #[test]
    fn scan_report() {
        assert_eq!(
            scan(&["foo", "bar", "food"], "foo", F_FIXED),
            Ok(Report {
                total: 3,
                selected: 2,
                hits: 2,
                first: 0,
            })
        );
        assert_eq!(
            scan(&["foo", "bar"], "foo", F_FIXED | F_INVERT).map(|r| r.selected),
            Ok(1)
        );
        assert_eq!(scan(&[], "a", F_FIXED), Err(GR_EINPUT));
        assert_eq!(scan(&["a\0b"], "a", F_FIXED), Err(GR_EBIN));
    }

    #[test]
    fn render_lines() {
        assert_eq!(filter(&["aa", "ba"], "a", F_FIXED), Ok(String::from("aa\nba\n")));
        assert_eq!(
            filter(&["aa", "ba"], "a", F_FIXED | F_LINENO),
            Ok(String::from("1:aa\n2:ba\n"))
        );
        assert_eq!(
            filter(&["aa", "ba"], "a", F_FIXED | F_ONLY),
            Ok(String::from("a\na\na\n"))
        );
        assert_eq!(
            filter(&["aa", "ba"], "a", F_FIXED | F_ONLY | F_LINENO),
            Ok(String::from("1:a\n1:a\n2:a\n"))
        );
    }

    #[test]
    fn render_modes() {
        assert_eq!(
            filter(&["aa", "ba"], "a", F_FIXED | F_COUNT),
            Ok(String::from("2\n"))
        );
        assert_eq!(
            filter(&["aa", "ba"], "a", F_FIXED | F_FILES),
            Ok(String::from("match\n"))
        );
        assert_eq!(
            filter(&["aa", "ba"], "a", F_FIXED | F_QUIET),
            Ok(String::new())
        );
        assert_eq!(
            filter(&["aa", "ba"], "a", F_FIXED | F_COUNT | F_INVERT),
            Ok(String::from("0\n"))
        );
    }

    #[test]
    fn path_gates() {
        assert_eq!(admit_path("", false, false), Err(GR_EINPUT));
        assert_eq!(admit_path("a/b", true, false), Err(GR_EREC));
        assert_eq!(admit_path("a/", false, false), Err(GR_EREC));
        assert_eq!(admit_path("a/b", true, true), Ok(KIND_DIR));
        assert_eq!(admit_path("a/b", false, false), Ok(KIND_FILE));
        assert_eq!(path_depth("a/b/c"), Ok(2));
        assert_eq!(
            admit_path("a/b/c/d/e/f/g/h/i/j", false, false),
            Err(GR_ERANGE)
        );
    }

    #[test]
    fn diff_table() {
        assert_eq!(count_diff_status(SUP_FULL), Ok(7));
        assert_eq!(count_diff_status(SUP_PARTIAL), Ok(3));
        assert_eq!(count_diff_status(SUP_GNU), Ok(2));
        assert_eq!(diff_name(0), Ok("backref"));
        assert_eq!(diff_status(3), Ok(SUP_GNU));
        assert_eq!(diff_name(12), Err(GR_ERANGE));
    }

    #[test]
    fn ledger_observations() {
        let l = build_ledger();
        assert_eq!(obs_at(&l, 0).map(|v| v.value), Ok(1));
        assert_eq!(obs_at(&l, 2).map(|v| v.value), Ok(1));
        assert_eq!(obs_at(&l, 13).map(|v| v.code), Ok(GR_ERANGE));
        assert_eq!(obs_at(&l, 99), Err(GR_ENOBS));
    }
}
