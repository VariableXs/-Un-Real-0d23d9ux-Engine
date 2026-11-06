//! UNX-F11502 dirname 目录前缀提取
//!
//! `dirname` 与 `basename` 是一对**互补**操作：`dirname p` / `basename p`
//! 拼回去必须还是 `p`（归一化之后）。互补性不是装饰，而是判据：
//! 两条命令在同一张路径表上各取一半，任何一边多剥或少剥一个斜杠，
//! 拼回来的路径就会多一个或丢一个分隔符。本模块把"前缀提取"与
//! "配对自洽"放在同一张判据表里，避免只测 dirname 单边而漏掉边界。
//!
//! 前缀提取的闸门次序：
//!
//! 选项 → 元开关（--help/--version） → 空路径 → 全斜杠 → 尾斜杠 → 头部 → 前缀。
//!
//! 四个容易混的边角：
//!
//! 1. **空路径与单段名**：`dirname ""` 与 `dirname a` 都是 `.`。空串不是
//!    "空前缀"，而是"没有目录部分"，与单段名同归点档。
//! 2. **全斜杠**：`dirname /` 是 `/`；`dirname ///` 也是 `/`；但 `dirname //`
//!    按 POSIX 的双斜杠保留规则输出 `//`——恰好两个前导斜杠是有语义的
//!    （实现定义的网络根），三个及以上塌回单斜杠。把 `//` 与 `///` 合并，
//!    配对检查会在 `//a` 上失败。
//! 3. **尾斜杠**：`dirname src/lib/` 是 `src` 而不是 `src/lib`。尾斜杠先剥，
//!    再找最后一个分隔符；次序反过来会把 `lib` 当成文件。
//! 4. **绝对路径的单段**：`dirname /a` 是 `/` 而不是空串。头部剥到只剩根
//!    时必须保留一个斜杠，否则拼回去会变成 `a`。
//!
//! 多操作数是 dirname 的常态（不同于 basename 需要 `-a`）：每个操作数
//! 独立成行，`-z` 只把行终止符换成 NUL。判决阶段取最后一个操作数的阶段，
//! 因为阶段是判决路径而不是输出内容。
//!
//! 判据七个维度：阶段、退出码、文本、文本长度、注解、配对自洽、路径
//! 分解。少"配对"一维，`dirname` 单独看每行都对、拼回去却多一个斜杠的
//! 缺陷就抓不到。
//!
//! 判序：状态 → 阶段 → 退出码 → 文本 → 长度 → 注解 → 配对。
//!
//! 语料 14 槽：8 槽申报正确（前缀 4、根 2、点 1、双根 1），6 槽各挂一处
//! 错误申报。账本 14 格 8 成 6 败，冻结后重放同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本在
//! `audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x6ce20;

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

/// 单次运行的最大操作数。
pub const MAX_OPERANDS: usize = 3;

/// 退出码：成功。
pub const EXIT_OK: u32 = 0;

/// 退出码：操作数/路径错误。
pub const EXIT_FAIL: u32 = 1;

/// 退出码：用法错误。
pub const EXIT_USAGE: u32 = 125;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 判果互报。
pub const DN_ESTATE: u32 = SEG_BASE + 0x01;
/// 阶段申报不符。
pub const DN_ESTAGE: u32 = SEG_BASE + 0x02;
/// 退出码申报不符。
pub const DN_ECODE: u32 = SEG_BASE + 0x03;
/// 文本申报不符。
pub const DN_ETEXT: u32 = SEG_BASE + 0x04;
/// 长度申报不符。
pub const DN_ELEN: u32 = SEG_BASE + 0x05;
/// 注解申报不符。
pub const DN_ENOTE: u32 = SEG_BASE + 0x06;
/// 配对不自洽。
pub const DN_EPAIR: u32 = SEG_BASE + 0x07;
/// 槽位非法。
pub const DN_ESLOT: u32 = SEG_BASE + 0x08;
/// 指纹不符。
pub const DN_ESEAL: u32 = SEG_BASE + 0x09;
/// 无观测。
pub const DN_ENOBS: u32 = SEG_BASE + 0x0a;
/// 矩阵不平。
pub const DN_EMATRIX: u32 = SEG_BASE + 0x0b;
/// 阶段次序倒置。
pub const DN_EORDER: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const DN_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 选项解析。
pub const OP_OPT: u8 = 0x01;
/// 尾斜杠归一。
pub const OP_TAIL: u8 = 0x02;
/// 头部剥离。
pub const OP_HEAD: u8 = 0x03;
/// 双斜杠规则。
pub const OP_DBL: u8 = 0x04;
/// 配对。
pub const OP_PAIR: u8 = 0x05;
/// 语料。
pub const OP_CORPUS: u8 = 0x06;
/// 终止符。
pub const OP_TERM: u8 = 0x07;

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
    let mut v = h;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            v = fold_byte(v, *b);
        }
        i += 1;
    }
    fold_byte(v, 0x1d)
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

/// u32 文本。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 是否全斜杠（非空）。
pub fn all_slashes(p: &str) -> bool {
    let b = p.as_bytes();
    if b.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(c) => {
                if *c != b'/' {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 尾部斜杠剥掉后的结束下标与"是否全斜杠"标志。
pub fn tail_end(p: &str) -> (usize, bool) {
    let b = p.as_bytes();
    let len = b.len();
    if len == 0 {
        return (0, false);
    }
    let mut end = len;
    while end > 1 {
        match b.get(end - 1) {
            Some(c) => {
                if *c == b'/' {
                    end -= 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    let all = end == 1 && matches!(b.get(0), Some(c) if *c == b'/');
    (end, all)
}

/// 在 `[0, end)` 内找最后一个斜杠。
pub fn last_slash(p: &str, end: usize) -> Option<usize> {
    let b = p.as_bytes();
    let mut i = end;
    while i > 0 {
        i -= 1;
        match b.get(i) {
            Some(c) => {
                if *c == b'/' {
                    return Some(i);
                }
            }
            None => return None,
        }
    }
    None
}

/// 前缀尾部去斜杠，但至少留一个字符。
pub fn head_trim(p: &str, i: usize) -> usize {
    let b = p.as_bytes();
    let mut e = i;
    while e > 1 {
        match b.get(e - 1) {
            Some(c) => {
                if *c == b'/' {
                    e -= 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    e
}

/// 恰好两个前导斜杠、且第三个字符不是斜杠（或不存在）。
pub fn double_leading(p: &str) -> bool {
    let b = p.as_bytes();
    if b.len() < 2 {
        return false;
    }
    let a = b.get(0);
    let c = b.get(1);
    let d = b.get(2);
    let two = matches!((a, c), (Some(x), Some(y)) if *x == b'/' && *y == b'/');
    if !two {
        return false;
    }
    match d {
        Some(z) => *z != b'/',
        None => true,
    }
}

// ---------------------------------------------------------------------------
// 阶段与注解
// ---------------------------------------------------------------------------

/// 判决阶段，次序即闸门次序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 选项解析。
    Option,
    /// 元开关。
    Meta,
    /// 空/点。
    Empty,
    /// 全斜杠。
    AllSlash,
    /// 尾斜杠归一。
    Tail,
    /// 头部剥离。
    Head,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Option => 0,
            Stage::Meta => 1,
            Stage::Empty => 2,
            Stage::AllSlash => 3,
            Stage::Tail => 4,
            Stage::Head => 5,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Option => "option",
            Stage::Meta => "meta",
            Stage::Empty => "empty",
            Stage::AllSlash => "allslash",
            Stage::Tail => "tail",
            Stage::Head => "head",
        }
    }
}

/// 注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 普通前缀。
    Prefix,
    /// 剥过尾斜杠。
    Slash,
    /// 单根。
    Root,
    /// 恰好两个前导斜杠。
    DoubleRoot,
    /// 点档。
    Dot,
    /// --help。
    Help,
    /// --version。
    Version,
    /// 用法错误。
    BadOpt,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Prefix => "prefix",
            Note::Slash => "slash",
            Note::Root => "root",
            Note::DoubleRoot => "double-root",
            Note::Dot => "dot",
            Note::Help => "help",
            Note::Version => "version",
            Note::BadOpt => "bad-opt",
        }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        match self {
            Note::BadOpt => false,
            _ => true,
        }
    }
}

/// 单段结果（借用输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part<'a> {
    /// 阶段。
    pub stage: Stage,
    /// 退出码。
    pub code: u32,
    /// 注解。
    pub note: Note,
    /// 结果文本。
    pub text: &'a str,
    /// 文本长度。
    pub len: u32,
}

impl<'a> Part<'a> {
    /// 失败段。
    pub const fn bad(stage: Stage, code: u32, note: Note) -> Part<'static> {
        Part { stage, code, note, text: "", len: 0 }
    }
}

/// 一次运行的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Out {
    /// 阶段。
    pub stage: Stage,
    /// 退出码。
    pub code: u32,
    /// 注解。
    pub note: Note,
    /// 文本（多操作数用 `\n` 连接）。
    pub text: String,
    /// 长度之和（不含连接符与终止符）。
    pub len: u32,
    /// 操作数个数。
    pub count: u32,
}

impl Out {
    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.code == EXIT_OK && self.note.ok()
    }

    /// 失败。
    pub fn failed(&self) -> bool {
        !self.ok()
    }

    /// 渲染：文本 + 终止符。
    pub fn render(&self, zero: bool) -> String {
        let mut s = String::from(self.text.as_str());
        s.push(if zero { '\u{0}' } else { '\n' });
        s
    }

    /// 长度文本。
    pub fn len_text(&self) -> String {
        dec_text(self.len)
    }
}

// ---------------------------------------------------------------------------
// 前缀提取
// ---------------------------------------------------------------------------

/// 对单条路径取目录前缀。
pub fn prefix_of(path: &str) -> Part<'_> {
    let bytes = path.as_bytes();
    if bytes.is_empty() {
        return Part { stage: Stage::Empty, code: EXIT_OK, note: Note::Dot, text: ".", len: 1 };
    }
    let (end, root) = tail_end(path);
    if root {
        if double_leading(path) {
            return Part {
                stage: Stage::AllSlash,
                code: EXIT_OK,
                note: Note::DoubleRoot,
                text: "//",
                len: 2,
            };
        }
        return Part { stage: Stage::AllSlash, code: EXIT_OK, note: Note::Root, text: "/", len: 1 };
    }
    let had_tail = end < bytes.len();
    match last_slash(path, end) {
        None => Part { stage: Stage::Empty, code: EXIT_OK, note: Note::Dot, text: ".", len: 1 },
        Some(0) => Part { stage: Stage::AllSlash, code: EXIT_OK, note: Note::Root, text: "/", len: 1 },
        Some(i) => {
            let pe = head_trim(path, i);
            if pe == 0 {
                return Part {
                    stage: Stage::AllSlash,
                    code: EXIT_OK,
                    note: Note::Root,
                    text: "/",
                    len: 1,
                };
            }
            if pe == 1 && matches!(path.as_bytes().get(0), Some(c) if *c == b'/') {
                if double_leading(path) {
                    return Part {
                        stage: Stage::AllSlash,
                        code: EXIT_OK,
                        note: Note::DoubleRoot,
                        text: "//",
                        len: 2,
                    };
                }
                return Part {
                    stage: Stage::AllSlash,
                    code: EXIT_OK,
                    note: Note::Root,
                    text: "/",
                    len: 1,
                };
            }
            let seg = match path.get(0..pe) {
                Some(v) => v,
                None => return Part::bad(Stage::Head, EXIT_FAIL, Note::BadOpt),
            };
            if had_tail {
                Part {
                    stage: Stage::Tail,
                    code: EXIT_OK,
                    note: Note::Slash,
                    text: seg,
                    len: seg.len() as u32,
                }
            } else {
                Part {
                    stage: Stage::Head,
                    code: EXIT_OK,
                    note: Note::Prefix,
                    text: seg,
                    len: seg.len() as u32,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 与 basename 的互补
// ---------------------------------------------------------------------------

/// 本地 basename（只为本模块的配对检查服务，独立实现，不跨卡片依赖）。
pub fn base_of(path: &str) -> &str {
    let bytes = path.as_bytes();
    if bytes.is_empty() {
        return ".";
    }
    let (end, _) = tail_end(path);
    if all_slashes(path) {
        return "/";
    }
    match last_slash(path, end) {
        Some(0) => match path.get(1..end) {
            Some(v) => v,
            None => ".",
        },
        Some(i) => match path.get(i + 1..end) {
            Some(v) => v,
            None => ".",
        },
        None => match path.get(0..end) {
            Some(v) => v,
            None => ".",
        },
    }
}

/// 归一化：只剥尾部斜杠（保留根与空串原样）。
pub fn normalize(path: &str) -> &str {
    let (end, _) = tail_end(path);
    match path.get(0..end) {
        Some(v) => v,
        None => path,
    }
}

/// 拼接 dir + "/" + base，根与点档走特例。
pub fn join_pair(dir: &str, base: &str) -> String {
    let mut s = String::new();
    if dir == "/" && base == "/" {
        return String::from("/");
    }
    if dir == "//" && base == "/" {
        return String::from("//");
    }
    if dir == "." && base == "." {
        return String::from(".");
    }
    s.push_str(dir);
    if !dir.ends_with('/') {
        s.push('/');
    }
    s.push_str(base);
    s
}

/// 折叠重复斜杠（保留开头至多一个斜杠，去掉尾部斜杠）。
pub fn collapse(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut s = String::new();
    let mut prev_slash = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match bytes.get(i) {
            Some(v) => *v,
            None => break,
        };
        if c == b'/' {
            if !prev_slash {
                s.push('/');
                prev_slash = true;
            }
        } else {
            s.push(c as char);
            prev_slash = false;
        }
        i += 1;
    }
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

/// 单条路径的配对是否自洽：
/// `dirname` 与本地 `basename` 拼回后，折叠重复斜杠应与原路径同形。
pub fn pair_ok(path: &str) -> bool {
    if path.is_empty() {
        return true;
    }
    let d = prefix_of(path);
    let b = base_of(path);
    if !d.note.ok() || b.is_empty() {
        return false;
    }
    let joined = join_pair(d.text, b);
    let want = normalize(path);
    if joined.as_str() == want {
        return true;
    }
    // 单段相对："a" 的前缀是 "."，拼出 "./a"，语义等价。
    if d.text == "." && b == want {
        return true;
    }
    // 重复斜杠：dirname 会折叠，拼回后折叠再比。
    collapse(joined.as_str()) == collapse(want)
}

// ---------------------------------------------------------------------------
// 运行规格
// ---------------------------------------------------------------------------

/// 一次 `dirname` 调用的规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    /// 操作数。
    pub operands: [&'static str; MAX_OPERANDS],
    /// 有效个数。
    pub count: u8,
    /// `-z`。
    pub zero: bool,
    /// `--help`。
    pub help: bool,
    /// `--version`。
    pub version: bool,
    /// 未知选项。
    pub bad_opt: bool,
}

impl Spec {
    /// 单操作数。
    pub const fn one(path: &'static str) -> Spec {
        Spec { operands: [path, "", ""], count: 1, zero: false, help: false, version: false, bad_opt: false }
    }

    /// 三操作数。
    pub const fn three(a: &'static str, b: &'static str, c: &'static str) -> Spec {
        Spec { operands: [a, b, c], count: 3, zero: false, help: false, version: false, bad_opt: false }
    }

    /// 取操作数。
    pub fn operand(&self, i: usize) -> Option<&'static str> {
        if i >= self.count as usize {
            return None;
        }
        self.operands.get(i).copied()
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.count as u64);
        let mut i = 0usize;
        while i < self.count as usize {
            if let Some(p) = self.operands.get(i) {
                h = fold_str(h, p);
            }
            i += 1;
        }
        h = fold_word(h, if self.zero { 1 } else { 0 });
        h = fold_word(h, if self.help { 1 } else { 0 });
        h = fold_word(h, if self.version { 1 } else { 0 });
        h = fold_word(h, if self.bad_opt { 1 } else { 0 });
        h
    }

    /// 运行。
    pub fn run(&self) -> Out {
        if self.bad_opt {
            return Out {
                stage: Stage::Option,
                code: EXIT_USAGE,
                note: Note::BadOpt,
                text: String::new(),
                len: 0,
                count: 0,
            };
        }
        if self.help {
            return Out {
                stage: Stage::Meta,
                code: EXIT_OK,
                note: Note::Help,
                text: String::from("Usage: dirname [OPTION] NAME...\n"),
                len: 32,
                count: 0,
            };
        }
        if self.version {
            return Out {
                stage: Stage::Meta,
                code: EXIT_OK,
                note: Note::Version,
                text: String::from("dirname (Varix coreutils) 9.4\n"),
                len: 30,
                count: 0,
            };
        }
        if self.count == 0 {
            return Out {
                stage: Stage::Option,
                code: EXIT_USAGE,
                note: Note::BadOpt,
                text: String::new(),
                len: 0,
                count: 0,
            };
        }
        if self.count as usize > MAX_OPERANDS {
            return Out {
                stage: Stage::Option,
                code: EXIT_USAGE,
                note: Note::BadOpt,
                text: String::new(),
                len: 0,
                count: self.count as u32,
            };
        }
        let mut out = Out {
            stage: Stage::Empty,
            code: EXIT_OK,
            note: Note::Dot,
            text: String::new(),
            len: 0,
            count: self.count as u32,
        };
        let mut i = 0usize;
        while i < self.count as usize {
            let p = match self.operand(i) {
                Some(v) => v,
                None => {
                    out.stage = Stage::Option;
                    out.code = EXIT_USAGE;
                    out.note = Note::BadOpt;
                    out.text = String::new();
                    out.len = 0;
                    return out;
                }
            };
            let part = prefix_of(p);
            if !part.note.ok() {
                out.stage = part.stage;
                out.code = part.code;
                out.note = part.note;
                out.text = String::new();
                out.len = 0;
                return out;
            }
            if i > 0 {
                out.text.push('\n');
            }
            out.text.push_str(part.text);
            out.len = out.len.saturating_add(part.len);
            out.stage = part.stage;
            out.note = part.note;
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 语料形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 标签。
    pub tag: &'static str,
    /// 规格。
    pub spec: Spec,
}

impl Shape {
    /// 空白。
    pub const fn blank() -> Shape {
        Shape { tag: "", spec: Spec::one("") }
    }

    /// 实际结果。
    pub fn actual(&self) -> Out {
        self.spec.run()
    }
}

/// 申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 阶段。
    pub stage: Stage,
    /// 退出码。
    pub code: u32,
    /// 文本。
    pub text: &'static str,
    /// 长度。
    pub len: u32,
    /// 注解。
    pub note: Note,
}

impl Claim {
    /// 空白。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Option, code: 0, text: "", len: 0, note: Note::Dot }
    }

    /// 是否成功。
    pub fn success(&self) -> bool {
        self.code == EXIT_OK && self.note.ok()
    }
}

/// 语料槽。
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

    /// 实际。
    pub fn actual(&self) -> Out {
        self.shape.actual()
    }

    /// 对判。
    pub fn judge(&self) -> Fault {
        judge(self)
    }

    /// 一致。
    pub fn ok(&self) -> bool {
        judge(self) == Fault::Ok
    }
}

// ---------------------------------------------------------------------------
// 缺陷
// ---------------------------------------------------------------------------

/// 非 Ok 恰 13 个。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 一致。
    Ok,
    /// 判果互报。
    Estate,
    /// 阶段错。
    Stage,
    /// 退出码错。
    Code,
    /// 文本错。
    Text,
    /// 长度错。
    Len,
    /// 注解错。
    Note,
    /// 配对错。
    Pair,
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
            Fault::Estate => DN_ESTATE,
            Fault::Stage => DN_ESTAGE,
            Fault::Code => DN_ECODE,
            Fault::Text => DN_ETEXT,
            Fault::Len => DN_ELEN,
            Fault::Note => DN_ENOTE,
            Fault::Pair => DN_EPAIR,
            Fault::Slot => DN_ESLOT,
            Fault::Seal => DN_ESEAL,
            Fault::Nobs => DN_ENOBS,
            Fault::Matrix => DN_EMATRIX,
            Fault::Order => DN_EORDER,
            Fault::Bal => DN_EBAL,
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

/// 短名。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "ok",
        Fault::Estate => "estate",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Text => "text",
        Fault::Len => "len",
        Fault::Note => "note",
        Fault::Pair => "pair",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Order => "order",
        Fault::Bal => "bal",
    }
}

// ---------------------------------------------------------------------------
// 语料表
// ---------------------------------------------------------------------------

/// 14 个形状。
pub const SHAPES: [Shape; CORPUS] = [
    Shape { tag: "file", spec: Spec::one("src/main.rs") },
    Shape { tag: "nested", spec: Spec::one("/usr/local/bin/tool") },
    Shape { tag: "rel", spec: Spec::one("a") },
    Shape { tag: "root", spec: Spec::one("/") },
    Shape { tag: "dblroot", spec: Spec::one("//") },
    Shape { tag: "trailing", spec: Spec::one("src/lib/") },
    Shape { tag: "dblslashes", spec: Spec::one("a//b") },
    Shape { tag: "absone", spec: Spec::one("/a") },
    Shape { tag: "dblroot-as-root", spec: Spec::one("//") },
    Shape { tag: "rel-as-tail", spec: Spec::one("a") },
    Shape { tag: "trailing-len", spec: Spec::one("src/lib/") },
    Shape { tag: "rel-as-root-note", spec: Spec::one("a") },
    Shape { tag: "file-as-parent", spec: Spec::one("src/main.rs") },
    Shape { tag: "root-as-error", spec: Spec::one("/") },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    // 0 普通前缀。
    Claim { stage: Stage::Head, code: EXIT_OK, text: "src", len: 3, note: Note::Prefix },
    // 1 深层绝对前缀。
    Claim { stage: Stage::Head, code: EXIT_OK, text: "/usr/local/bin", len: 14, note: Note::Prefix },
    // 2 单段名 → 点。
    Claim { stage: Stage::Empty, code: EXIT_OK, text: ".", len: 1, note: Note::Dot },
    // 3 根。
    Claim { stage: Stage::AllSlash, code: EXIT_OK, text: "/", len: 1, note: Note::Root },
    // 4 双根。
    Claim { stage: Stage::AllSlash, code: EXIT_OK, text: "//", len: 2, note: Note::DoubleRoot },
    // 5 尾斜杠。
    Claim { stage: Stage::Tail, code: EXIT_OK, text: "src", len: 3, note: Note::Slash },
    // 6 中间双斜杠。
    Claim { stage: Stage::Head, code: EXIT_OK, text: "a", len: 1, note: Note::Prefix },
    // 7 绝对单段 → 根。
    Claim { stage: Stage::AllSlash, code: EXIT_OK, text: "/", len: 1, note: Note::Root },
    // 8 双根报成单根 —— 文本错。
    Claim { stage: Stage::AllSlash, code: EXIT_OK, text: "/", len: 1, note: Note::DoubleRoot },
    // 9 单段名报成尾斜杠阶段 —— 阶段错。
    Claim { stage: Stage::Tail, code: EXIT_OK, text: ".", len: 1, note: Note::Dot },
    // 10 尾斜杠前缀长度多报一位 —— 长度错。
    Claim { stage: Stage::Tail, code: EXIT_OK, text: "src", len: 4, note: Note::Slash },
    // 11 单段名的注解报成根 —— 注解错。
    Claim { stage: Stage::Empty, code: EXIT_OK, text: ".", len: 1, note: Note::Root },
    // 12 前缀带上分隔符 —— 文本错。
    Claim { stage: Stage::Head, code: EXIT_OK, text: "src/", len: 4, note: Note::Prefix },
    // 13 根报成错误 —— 状态互报。
    Claim { stage: Stage::Option, code: EXIT_FAIL, text: "", len: 0, note: Note::BadOpt },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Text, Fault::Stage, Fault::Len, Fault::Note, Fault::Text, Fault::Estate];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    DN_ETEXT,
    DN_ESTAGE,
    DN_ELEN,
    DN_ENOTE,
    DN_ETEXT,
    DN_ESTATE,
];

// ---------------------------------------------------------------------------
// 语料访问与对判
// ---------------------------------------------------------------------------

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
        Err(DN_ENOBS)
    }
}

/// 设计期望。
pub fn expected(i: usize) -> Option<Fault> {
    if i < ACCEPT {
        Some(Fault::Ok)
    } else {
        REJECT_FAULTS.get(i - ACCEPT).copied()
    }
}

/// 对判：状态 → 阶段 → 退出码 → 文本 → 长度 → 注解 → 配对。
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
    if claim.text != a.text.as_str() {
        return Fault::Text;
    }
    if claim.len != a.len {
        return Fault::Len;
    }
    if claim.note != a.note {
        return Fault::Note;
    }
    let path = match c.shape.spec.operands.first() {
        Some(p) => *p,
        None => "",
    };
    if !pair_ok(path) {
        return Fault::Pair;
    }
    Fault::Ok
}

// ---------------------------------------------------------------------------
// 释义
// ---------------------------------------------------------------------------

/// 单槽释义：
/// `dirname <path> text=<t> len=<n> stage=<s> note=<n>`。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from("dirname ");
    match c.shape.spec.operands.first() {
        Some(p) => s.push_str(p),
        None => s.push_str(""),
    }
    s.push_str(" text=");
    s.push_str(a.text.as_str());
    s.push_str(" len=");
    s.push_str(a.len_text().as_str());
    s.push_str(" stage=");
    s.push_str(a.stage.name());
    s.push_str(" note=");
    s.push_str(a.note.name());
    s.push('\n');
    s
}

/// 段释义：`<text> len=<n> stage=<s> note=<n>`。
pub fn part_text(p: &Part) -> String {
    let mut s = String::from(p.text);
    s.push_str(" len=");
    s.push_str(dec_text(p.len).as_str());
    s.push_str(" stage=");
    s.push_str(p.stage.name());
    s.push_str(" note=");
    s.push_str(p.note.name());
    s
}

// ---------------------------------------------------------------------------
// 档位与矩阵
// ---------------------------------------------------------------------------

/// 判据档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 普通前缀（含剥过尾斜杠的前缀）。
    Prefix,
    /// 根与双根。
    Root,
    /// 点档。
    Dot,
    /// 元开关与用法错误。
    Meta,
}

impl Tier {
    /// 由注解归位。
    pub fn of(n: Note) -> Tier {
        match n {
            Note::Prefix => Tier::Prefix,
            Note::Slash => Tier::Prefix,
            Note::Root => Tier::Root,
            Note::DoubleRoot => Tier::Root,
            Note::Dot => Tier::Dot,
            Note::Help => Tier::Meta,
            Note::Version => Tier::Meta,
            Note::BadOpt => Tier::Meta,
        }
    }

    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Tier::Prefix => 0,
            Tier::Root => 1,
            Tier::Dot => 2,
            Tier::Meta => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Prefix => "prefix",
            Tier::Root => "root",
            Tier::Dot => "dot",
            Tier::Meta => "meta",
        }
    }
}

/// 一行统计。
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// 行。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 空矩阵。
    pub const fn blank() -> Matrix {
        Matrix { rows: [Row::blank(); TIER_COUNT] }
    }

    /// 由语料统计（全部 14 槽）。
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确数。
    pub accept: u32,
    /// 申报错误数。
    pub reject: u32,
    /// 前缀档数。
    pub prefix: u32,
    /// 根档数。
    pub root: u32,
    /// 点档数。
    pub dot: u32,
    /// 元开关档数。
    pub meta: u32,
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
            Err(DN_ESTAGE)
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("dirname corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" prefix=");
        s.push_str(u32_text(self.prefix).as_str());
        s.push_str(" root=");
        s.push_str(u32_text(self.root).as_str());
        s.push_str(" dot=");
        s.push_str(u32_text(self.dot).as_str());
        s.push_str(" meta=");
        s.push_str(u32_text(self.meta).as_str());
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
            return Err(DN_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut prefix = 0u32;
    let mut root = 0u32;
    let mut dot = 0u32;
    let mut meta = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(DN_ENOBS),
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
            None => return Err(DN_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(a.note) {
                Tier::Prefix => prefix += 1,
                Tier::Root => root += 1,
                Tier::Dot => dot += 1,
                Tier::Meta => meta += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, prefix, root, dot, meta, matched })
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
            return Err(DN_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(DN_EORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(DN_ESTATE);
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
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DN_ESLOT);
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
            return Err(DN_EBAL);
        }
        // 前三格是语料尺寸，四到七格是档位实数，第八格是强制红后的 929‰。
        const VALS: [u32; ACCEPT] = [14, 8, 6, 4, 3, 1, 0, 929];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DN_EBAL),
            };
            if s.slot as usize != i {
                return Err(DN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DN_EBAL);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DN_EBAL);
                        }
                    }
                    None => return Err(DN_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DN_EBAL);
                        }
                    }
                    None => return Err(DN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(DN_EBAL);
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
            return Err(DN_EBAL);
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
            Err(DN_ESEAL)
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
    let _ = l.register(count_cell(3, OP_HEAD, r.map(|x| x.prefix)));
    let _ = l.register(count_cell(4, OP_DBL, r.map(|x| x.root)));
    let _ = l.register(count_cell(5, OP_PAIR, r.map(|x| x.dot)));
    let _ = l.register(count_cell(6, OP_TERM, r.map(|x| x.meta)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_OPT, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_TAIL,
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

/// 四路注入：表、归一化、渲染、账本。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && spec_ok();
    out[1] = prefix_ok() && root_ok() && pairing_ok();
    out[2] = matrix_ok() && corpus_ok() && reds_ok() && detail_ok();
    out[3] = slots_ok()
        && gate_ok()
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

/// 段基址、码表、规模常量。
fn name_ok() -> bool {
    let codes = [
        DN_ESTATE, DN_ESTAGE, DN_ECODE, DN_ETEXT, DN_ELEN, DN_ENOTE, DN_EPAIR, DN_ESLOT,
        DN_ESEAL, DN_ENOBS, DN_EMATRIX, DN_EORDER, DN_EBAL,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        let c = match codes.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            match codes.get(j) {
                Some(d) => {
                    if c == *d {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let ops = [OP_OPT, OP_TAIL, OP_HEAD, OP_DBL, OP_PAIR, OP_CORPUS, OP_TERM];
    let mut a = 0usize;
    while a < ops.len() {
        let mut b = a + 1;
        while b < ops.len() {
            match (ops.get(a), ops.get(b)) {
                (Some(x), Some(y)) => {
                    if x == y {
                        return false;
                    }
                }
                _ => return false,
            }
            b += 1;
        }
        a += 1;
    }
    SEG_BASE == 0x6ce20
        && SAMPLES == 14
        && CORPUS == 14
        && ACCEPT == 8
        && REJECT == 6
        && ACCEPT + REJECT == CORPUS
        && TIER_COUNT == 4
        && MAX_OPERANDS == 3
        && EXIT_OK == 0
        && EXIT_FAIL == 1
        && EXIT_USAGE == 125
}

/// 规格与运行。
fn spec_ok() -> bool {
    let a = Spec::one("src/main.rs").run();
    if !a.ok()
        || a.stage != Stage::Head
        || a.note != Note::Prefix
        || a.text.as_str() != "src"
        || a.len != 3
        || a.count != 1
    {
        return false;
    }
    let d = Spec::one("a").run();
    if d.stage != Stage::Empty || d.note != Note::Dot || d.text.as_str() != "." || d.len != 1 {
        return false;
    }
    let r = Spec::one("/").run();
    if r.stage != Stage::AllSlash || r.note != Note::Root || r.text.as_str() != "/" {
        return false;
    }
    let q = Spec::one("//").run();
    if q.note != Note::DoubleRoot || q.text.as_str() != "//" || q.len != 2 {
        return false;
    }
    let m = Spec::three("a/b", "c/d", "e/f").run();
    if !m.ok()
        || m.text.as_str() != "a\nc\ne"
        || m.len != 3
        || m.count != 3
        || m.stage != Stage::Head
        || m.note != Note::Prefix
    {
        return false;
    }
    let help = Spec { help: true, ..Spec::one("a") }.run();
    if !help.ok()
        || help.stage != Stage::Meta
        || help.note != Note::Help
        || help.text.as_str() != "Usage: dirname [OPTION] NAME...\n"
        || help.len != 32
        || help.count != 0
    {
        return false;
    }
    let ver = Spec { version: true, ..Spec::one("a") }.run();
    if !ver.ok()
        || ver.stage != Stage::Meta
        || ver.note != Note::Version
        || ver.text.as_str() != "dirname (Varix coreutils) 9.4\n"
        || ver.len != 30
    {
        return false;
    }
    let bad = Spec { bad_opt: true, ..Spec::one("a") }.run();
    if bad.ok()
        || bad.stage != Stage::Option
        || bad.code != EXIT_USAGE
        || bad.note != Note::BadOpt
        || !bad.text.is_empty()
    {
        return false;
    }
    let zero = Spec { count: 0, ..Spec::one("a") }.run();
    if zero.ok() || zero.stage != Stage::Option || zero.code != EXIT_USAGE || zero.note != Note::BadOpt
    {
        return false;
    }
    let many = Spec { count: 4, ..Spec::one("a") }.run();
    if many.ok() || many.code != EXIT_USAGE || many.count != 4 {
        return false;
    }
    let mut z = Spec::one("a");
    z.zero = true;
    let zo = z.run();
    if zo.render(false).as_str() != ".\n" || zo.render(true).as_str() != ".\u{0}" {
        return false;
    }
    Spec::one("a").seal() != Spec::one("b").seal()
        && Spec::one("a").seal() == Spec::one("a").seal()
        && Spec::one("a").seal() != Spec::three("a", "b", "c").seal()
        && Spec::one("a").seal() != Spec { help: true, ..Spec::one("a") }.seal()
}

/// 前缀提取表。
fn prefix_ok() -> bool {
    let rows: [(&str, Stage, Note, &str, u32); 19] = [
        ("", Stage::Empty, Note::Dot, ".", 1),
        ("a", Stage::Empty, Note::Dot, ".", 1),
        ("e/", Stage::Empty, Note::Dot, ".", 1),
        ("./a", Stage::Head, Note::Prefix, ".", 1),
        ("src/main.rs", Stage::Head, Note::Prefix, "src", 3),
        ("a/b", Stage::Head, Note::Prefix, "a", 1),
        ("/usr/local/bin/tool", Stage::Head, Note::Prefix, "/usr/local/bin", 14),
        ("src/lib/", Stage::Tail, Note::Slash, "src", 3),
        ("src/lib//", Stage::Tail, Note::Slash, "src", 3),
        ("a//b", Stage::Head, Note::Prefix, "a", 1),
        ("a//b/", Stage::Tail, Note::Slash, "a", 1),
        ("/", Stage::AllSlash, Note::Root, "/", 1),
        ("//", Stage::AllSlash, Note::DoubleRoot, "//", 2),
        ("///", Stage::AllSlash, Note::Root, "/", 1),
        ("/a", Stage::AllSlash, Note::Root, "/", 1),
        ("//a", Stage::AllSlash, Note::DoubleRoot, "//", 2),
        ("///a", Stage::AllSlash, Note::Root, "/", 1),
        ("//a/b", Stage::Head, Note::Prefix, "//a", 3),
        ("a/b/", Stage::Tail, Note::Slash, "a", 1),
    ];
    let mut i = 0usize;
    while i < rows.len() {
        let row = match rows.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let p = prefix_of(row.0);
        if p.stage != row.1
            || p.note != row.2
            || p.text != row.3
            || p.len != row.4
            || p.code != EXIT_OK
        {
            return false;
        }
        i += 1;
    }
    true
}

/// 双斜杠与全斜杠规则。
fn root_ok() -> bool {
    if !double_leading("//")
        || double_leading("///")
        || double_leading("///a")
        || double_leading("/a")
        || double_leading("a//")
        || double_leading("")
        || double_leading("/")
    {
        return false;
    }
    if !double_leading("//a") {
        return false;
    }
    if !all_slashes("/") || !all_slashes("//") || !all_slashes("////") {
        return false;
    }
    if all_slashes("") || all_slashes("a") || all_slashes("/a") {
        return false;
    }
    let a = prefix_of("//");
    let b = prefix_of("///");
    let c = prefix_of("//a");
    let d = prefix_of("///a");
    a.text == "//"
        && a.note == Note::DoubleRoot
        && b.text == "/"
        && b.note == Note::Root
        && c.text == "//"
        && c.note == Note::DoubleRoot
        && d.text == "/"
        && d.note == Note::Root
        && collapse("//").as_str() == "/"
        && collapse("a////b").as_str() == "a/b"
}

/// 配对自洽与互补表。
fn pairing_ok() -> bool {
    if base_of("src/main.rs") != "main.rs"
        || base_of("a") != "a"
        || base_of("/") != "/"
        || base_of("//") != "/"
        || base_of("///") != "/"
        || base_of("a/") != "a"
        || base_of("a//") != "a"
        || base_of("/a") != "a"
        || base_of("a//b") != "b"
        || base_of("src/lib/") != "lib"
        || base_of("") != "."
    {
        return false;
    }
    if normalize("a/b//") != "a/b"
        || normalize("") != ""
        || normalize("//") != "/"
        || normalize("/") != "/"
    {
        return false;
    }
    if join_pair("src", "lib") != "src/lib"
        || join_pair("/", "a") != "/a"
        || join_pair("//", "/") != "//"
        || join_pair(".", ".") != "."
        || join_pair(".", "a") != "./a"
        || join_pair("src/", "lib") != "src/lib"
    {
        return false;
    }
    let paths = [
        "src/main.rs",
        "/usr/local/bin/tool",
        "a",
        "/",
        "//",
        "src/lib/",
        "a//b",
        "/a",
        "./a",
        "",
    ];
    let mut i = 0usize;
    while i < paths.len() {
        match paths.get(i) {
            Some(p) => {
                if !pair_ok(p) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
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
        if judge(&c) != want || c.judge() != want || c.ok() != (want == Fault::Ok) {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        let f = match REJECT_FAULTS.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if !f.alive() {
            return false;
        }
        match REJECT_CODES.get(j) {
            Some(c) => {
                if f.code() != *c {
                    return false;
                }
            }
            None => return false,
        }
        j += 1;
    }
    true
}

/// 档位矩阵。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    m.total() == CORPUS
        && m.good() == ACCEPT
        && m.balanced()
        && m.row(0) == Some(Row { total: 6, good: 4 })
        && m.row(1) == Some(Row { total: 5, good: 3 })
        && m.row(2) == Some(Row { total: 3, good: 1 })
        && m.row(3) == Some(Row { total: 0, good: 0 })
        && m.row(4).is_none()
        && m.perfect_rows() == 0
        && m.seal() == Matrix::of().seal()
        && m.seal() != Matrix::blank().seal()
}

/// 语料与渲染。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    r.total == CORPUS as u32
        && r.accept == ACCEPT as u32
        && r.reject == REJECT as u32
        && r.prefix == 4
        && r.root == 3
        && r.dot == 1
        && r.meta == 0
        && r.matched == CORPUS as u32
        && r.rate() == 1000
        && r.green()
        && r.gate() == Ok(())
        && r.render().as_str()
            == "dirname corpus=14 accept=8 reject=6 prefix=4 root=3 dot=1 meta=0 match=14 rate=1000ppm\n"
}

/// 红档显式。
fn reds_ok() -> bool {
    let faults = [
        Fault::Estate,
        Fault::Stage,
        Fault::Code,
        Fault::Text,
        Fault::Len,
        Fault::Note,
        Fault::Pair,
        Fault::Slot,
        Fault::Seal,
        Fault::Nobs,
        Fault::Matrix,
        Fault::Order,
        Fault::Bal,
    ];
    let mut i = 0usize;
    while i < faults.len() {
        let f = match faults.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let r = match run_with(Some((0, f))) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if r.matched != 13
            || r.rate() != 929
            || r.accept != 7
            || r.reject != 7
            || r.green()
            || r.gate() != Err(DN_ESTAGE)
        {
            return false;
        }
        i += 1;
    }
    run_with(Some((CORPUS, Fault::Code))).err() == Some(DN_ESLOT)
        && run_with(Some((CORPUS + 1, Fault::Ok))).err() == Some(DN_ESLOT)
}

/// 释义逐字。
fn detail_ok() -> bool {
    let cs = cases();
    let wants = [
        (0usize, "dirname src/main.rs text=src len=3 stage=head note=prefix\n"),
        (2, "dirname a text=. len=1 stage=empty note=dot\n"),
        (3, "dirname / text=/ len=1 stage=allslash note=root\n"),
        (4, "dirname // text=// len=2 stage=allslash note=double-root\n"),
        (5, "dirname src/lib/ text=src len=3 stage=tail note=slash\n"),
    ];
    let mut i = 0usize;
    while i < wants.len() {
        let w = match wants.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let c = match cs.get(w.0) {
            Some(v) => *v,
            None => return false,
        };
        if detail(&c).as_str() != w.1 {
            return false;
        }
        i += 1;
    }
    part_text(&prefix_of("src/lib/")).as_str() == "src len=3 stage=tail note=slash"
        && part_text(&prefix_of("//")).as_str() == "// len=2 stage=allslash note=double-root"
}

/// 槽号与命名。
fn slots_ok() -> bool {
    if case_at(0).is_none() || case_at(CORPUS - 1).is_none() || case_at(CORPUS).is_some() {
        return false;
    }
    if cover(CORPUS) != Ok(()) || cover(0) != Err(DN_ENOBS) || cover(CORPUS + 1) != Err(DN_ENOBS) {
        return false;
    }
    if case_at(0).map(|c| c.tag()) != Some("file")
        || case_at(7).map(|c| c.tag()) != Some("absone")
        || case_at(13).map(|c| c.tag()) != Some("root-as-error")
    {
        return false;
    }
    let mut i = 0usize;
    while i < ACCEPT {
        if expected(i) != Some(Fault::Ok) {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < REJECT {
        if expected(ACCEPT + j) != REJECT_FAULTS.get(j).copied() {
            return false;
        }
        j += 1;
    }
    expected(CORPUS).is_none() && expected(CORPUS + 1).is_none()
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.step(Stage::Option) != Ok(()) {
        return false;
    }
    if g.step(Stage::Meta) != Ok(()) {
        return false;
    }
    if g.step(Stage::Empty) != Ok(()) {
        return false;
    }
    if g.step(Stage::Empty) != Ok(()) {
        return false;
    }
    if g.last() != 2 || g.steps() != 4 {
        return false;
    }
    if g.step(Stage::Option) != Err(DN_EORDER) || g.last() != 2 || g.steps() != 4 {
        return false;
    }
    if g.step(Stage::AllSlash) != Ok(()) || g.step(Stage::Tail) != Ok(()) {
        return false;
    }
    if g.step(Stage::Head) != Ok(()) || g.steps() != 7 {
        return false;
    }
    if g.close() != Ok(7) || !g.closed() {
        return false;
    }
    g.close() == Err(DN_ESTATE) && g.step(Stage::Head) == Err(DN_ESTATE)
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
        && l.sample(7).map(|s| s.value) == Some(929)
        && l.sample(8).map(|s| s.code) == Some(DN_ETEXT)
        && l.sample(13).map(|s| s.code) == Some(DN_ESTATE)
}

// ---------------------------------------------------------------------------
// 判据集
// ---------------------------------------------------------------------------

/// 13 条判据。
pub fn run_unx11502_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11502_dirname_prefix");

    s.add("DN01-名称与段基址", name_ok(), "static");

    s.add("DN02-语料规格表", spec_ok(), "static");

    s.add("DN03-前缀提取表", prefix_ok(), "static");

    s.add("DN04-双斜杠规则", root_ok(), "static");

    s.add("DN05-配对自洽", pairing_ok(), "static");

    s.add("DN06-申报对判", judge_ok(), "static");

    s.add("DN07-档位矩阵", matrix_ok(), "static");

    s.add("DN08-语料与渲染", corpus_ok(), "static");

    s.add("DN09-红档显式", reds_ok(), "static");

    s.add("DN10-释义逐字", detail_ok(), "static");

    s.add("DN11-槽号与命名", slots_ok(), "static");

    s.add("DN12-阶段闸门", gate_ok(), "static");

    s.add(
        "DN13-冻结重放四注入",
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
        panic!("unx11502 test fail")
    }

    #[test]
    fn seg_consts() {
        assert_eq!(SEG_BASE, 0x6ce20);
        assert_eq!(SAMPLES, 14);
        assert_eq!(CORPUS, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(MAX_OPERANDS, 3);
        assert_eq!(EXIT_OK, 0);
        assert_eq!(EXIT_FAIL, 1);
        assert_eq!(EXIT_USAGE, 125);
    }

    #[test]
    fn code_uniqueness() {
        let codes = [
            DN_ESTATE, DN_ESTAGE, DN_ECODE, DN_ETEXT, DN_ELEN, DN_ENOTE, DN_EPAIR, DN_ESLOT,
            DN_ESEAL, DN_ENOBS, DN_EMATRIX, DN_EORDER, DN_EBAL,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                assert_ne!(codes[i], codes[j]);
                j += 1;
            }
            assert_eq!(codes[i], SEG_BASE + (i as u32) + 1);
            i += 1;
        }
        assert_eq!(DN_ESEAL, SEG_BASE + 9);
        assert_eq!(DN_EBAL, SEG_BASE + 0x0d);
    }

    #[test]
    fn helper_text() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(u32_text(1000).as_str(), "1000");
        assert_eq!(hex_text(0x6ce20, 5).as_str(), "6ce20");
        assert_eq!(dec_text(14).as_str(), "14");
    }

    #[test]
    fn all_slashes_table() {
        assert!(all_slashes("/"));
        assert!(all_slashes("//"));
        assert!(all_slashes("////"));
        assert!(!all_slashes(""));
        assert!(!all_slashes("a"));
        assert!(!all_slashes("/a"));
    }

    #[test]
    fn tail_end_table() {
        assert_eq!(tail_end(""), (0, false));
        assert_eq!(tail_end("a"), (1, false));
        assert_eq!(tail_end("a/"), (1, false));
        assert_eq!(tail_end("a//"), (1, false));
        assert_eq!(tail_end("/"), (1, true));
        assert_eq!(tail_end("///"), (1, true));
        assert_eq!(tail_end("a/b//"), (3, false));
        assert_eq!(tail_end("//a"), (3, false));
    }

    #[test]
    fn last_slash_table() {
        assert_eq!(last_slash("a/b", 3), Some(1));
        assert_eq!(last_slash("ab", 2), None);
        assert_eq!(last_slash("/ab", 3), Some(0));
        assert_eq!(last_slash("", 0), None);
        assert_eq!(last_slash("a//b", 4), Some(2));
    }

    #[test]
    fn head_trim_table() {
        assert_eq!(head_trim("src/lib", 3), 3);
        assert_eq!(head_trim("a//b", 3), 1);
        assert_eq!(head_trim("//a", 1), 1);
        assert_eq!(head_trim("a/b", 0), 0);
        assert_eq!(head_trim("/a", 1), 1);
    }

    #[test]
    fn double_leading_table() {
        assert!(double_leading("//"));
        assert!(double_leading("//a"));
        assert!(!double_leading("///"));
        assert!(!double_leading("///a"));
        assert!(!double_leading("/a"));
        assert!(!double_leading("a//"));
        assert!(!double_leading(""));
        assert!(!double_leading("/"));
    }

    #[test]
    fn collapse_table() {
        assert_eq!(collapse("a//b").as_str(), "a/b");
        assert_eq!(collapse("//").as_str(), "/");
        assert_eq!(collapse("a/b//").as_str(), "a/b");
        assert_eq!(collapse("").as_str(), "");
        assert_eq!(collapse("/").as_str(), "/");
    }

    #[test]
    fn prefix_file() {
        let p = prefix_of("src/main.rs");
        assert_eq!(p.text, "src");
        assert_eq!(p.len, 3);
        assert_eq!(p.stage, Stage::Head);
        assert_eq!(p.note, Note::Prefix);
        assert_eq!(p, Part { stage: Stage::Head, code: EXIT_OK, note: Note::Prefix, text: "src", len: 3 });
    }

    #[test]
    fn prefix_nested() {
        let p = prefix_of("/usr/local/bin/tool");
        assert_eq!(p.text, "/usr/local/bin");
        assert_eq!(p.len, 14);
        assert_eq!(p.stage, Stage::Head);
        assert_eq!(p.note, Note::Prefix);
    }

    #[test]
    fn prefix_relative_and_empty() {
        for p in ["", "a", "name"] {
            let q = prefix_of(p);
            assert_eq!(q.text, ".");
            assert_eq!(q.len, 1);
            assert_eq!(q.stage, Stage::Empty);
            assert_eq!(q.note, Note::Dot);
        }
        let d = prefix_of("./a");
        assert_eq!(d.text, ".");
        assert_eq!(d.stage, Stage::Head);
        assert_eq!(d.note, Note::Prefix);
    }

    #[test]
    fn prefix_roots() {
        for p in ["/", "///", "/a"] {
            let q = prefix_of(p);
            assert_eq!(q.text, "/");
            assert_eq!(q.len, 1);
            assert_eq!(q.stage, Stage::AllSlash);
            assert_eq!(q.note, Note::Root);
        }
        let d = prefix_of("//");
        assert_eq!(d.text, "//");
        assert_eq!(d.len, 2);
        assert_eq!(d.note, Note::DoubleRoot);
    }

    #[test]
    fn prefix_double_root() {
        assert_eq!(prefix_of("//a").text, "//");
        assert_eq!(prefix_of("//a").note, Note::DoubleRoot);
        assert_eq!(prefix_of("///a").text, "/");
        assert_eq!(prefix_of("///a").note, Note::Root);
        assert_eq!(prefix_of("//a/b").text, "//a");
        assert_eq!(prefix_of("//a/b").stage, Stage::Head);
    }

    #[test]
    fn prefix_trailing() {
        let a = prefix_of("src/lib/");
        assert_eq!(a.text, "src");
        assert_eq!(a.stage, Stage::Tail);
        assert_eq!(a.note, Note::Slash);
        let b = prefix_of("src/lib//");
        assert_eq!(b.text, "src");
        assert_eq!(b.stage, Stage::Tail);
        let c = prefix_of("a/b/");
        assert_eq!(c.text, "a");
        assert_eq!(c.note, Note::Slash);
    }

    #[test]
    fn prefix_inner_double() {
        let a = prefix_of("a//b");
        assert_eq!(a.text, "a");
        assert_eq!(a.stage, Stage::Head);
        assert_eq!(a.note, Note::Prefix);
        let b = prefix_of("a//b/");
        assert_eq!(b.text, "a");
        assert_eq!(b.stage, Stage::Tail);
    }

    #[test]
    fn prefix_abs_single() {
        let a = prefix_of("/a");
        assert_eq!(a.text, "/");
        assert_eq!(a.code, EXIT_OK);
        assert_eq!(a, Part { stage: Stage::AllSlash, code: EXIT_OK, note: Note::Root, text: "/", len: 1 });
    }

    #[test]
    fn base_of_table() {
        assert_eq!(base_of("src/main.rs"), "main.rs");
        assert_eq!(base_of("a"), "a");
        assert_eq!(base_of("/"), "/");
        assert_eq!(base_of("//"), "/");
        assert_eq!(base_of("///"), "/");
        assert_eq!(base_of("a/"), "a");
        assert_eq!(base_of("a//"), "a");
        assert_eq!(base_of("/a"), "a");
        assert_eq!(base_of("a//b"), "b");
        assert_eq!(base_of("src/lib/"), "lib");
        assert_eq!(base_of(""), ".");
    }

    #[test]
    fn join_pair_table() {
        assert_eq!(join_pair("src", "lib").as_str(), "src/lib");
        assert_eq!(join_pair("/", "a").as_str(), "/a");
        assert_eq!(join_pair("//", "/").as_str(), "//");
        assert_eq!(join_pair("/", "/").as_str(), "/");
        assert_eq!(join_pair(".", ".").as_str(), ".");
        assert_eq!(join_pair(".", "a").as_str(), "./a");
        assert_eq!(join_pair("src/", "lib").as_str(), "src/lib");
    }

    #[test]
    fn pair_table() {
        assert!(pair_ok(""));
        assert!(pair_ok("a"));
        assert!(pair_ok("a//b"));
        assert!(pair_ok("./a"));
        assert!(pair_ok("src/main.rs"));
        assert!(pair_ok("/usr/local/bin/tool"));
        assert!(pair_ok("//"));
        assert!(pair_ok("///"));
        assert!(pair_ok("/"));
        assert!(pair_ok("src/lib/"));
    }

    #[test]
    fn spec_one() {
        let s = Spec::one("a/b");
        assert_eq!(s.count, 1);
        assert_eq!(s.operand(0), Some("a/b"));
        assert_eq!(s.operand(1), None);
        let o = s.run();
        assert!(o.ok());
        assert_eq!(o.text.as_str(), "a");
        assert_eq!(o.stage, Stage::Head);
        assert_eq!(o.note, Note::Prefix);
        assert_eq!(o.len, 1);
    }

    #[test]
    fn spec_multi() {
        let s = Spec::three("a/b", "c/d", "e/f");
        let o = s.run();
        assert!(o.ok());
        assert_eq!(o.text.as_str(), "a\nc\ne");
        assert_eq!(o.len, 3);
        assert_eq!(o.count, 3);
        assert_eq!(o.stage, Stage::Head);
        assert_eq!(o.note, Note::Prefix);
        assert_eq!(s.operand(2), Some("e/f"));
        assert_eq!(s.operand(3), None);
    }

    #[test]
    fn spec_meta_help() {
        let s = Spec { help: true, ..Spec::one("a") };
        let o = s.run();
        assert!(o.ok());
        assert_eq!(o.stage, Stage::Meta);
        assert_eq!(o.note, Note::Help);
        assert_eq!(o.text.as_str(), "Usage: dirname [OPTION] NAME...\n");
        assert_eq!(o.len, 32);
        assert_eq!(o.count, 0);
    }

    #[test]
    fn spec_meta_version() {
        let s = Spec { version: true, ..Spec::one("a") };
        let o = s.run();
        assert!(o.ok());
        assert_eq!(o.stage, Stage::Meta);
        assert_eq!(o.note, Note::Version);
        assert_eq!(o.text.as_str(), "dirname (Varix coreutils) 9.4\n");
        assert_eq!(o.len, 30);
    }

    #[test]
    fn spec_bad_opt() {
        let s = Spec { bad_opt: true, ..Spec::one("a") };
        let o = s.run();
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Option);
        assert_eq!(o.code, EXIT_USAGE);
        assert_eq!(o.note, Note::BadOpt);
        assert!(o.text.is_empty());
    }

    #[test]
    fn spec_zero() {
        let s = Spec { count: 0, ..Spec::one("a") };
        let o = s.run();
        assert!(!o.ok());
        assert_eq!(o.code, EXIT_USAGE);
        assert_eq!(o.note, Note::BadOpt);
        let m = Spec { count: 4, ..Spec::one("a") };
        let p = m.run();
        assert!(!p.ok());
        assert_eq!(p.count, 4);
    }

    #[test]
    fn spec_seal() {
        let a = Spec::one("a").seal();
        assert_eq!(a, Spec::one("a").seal());
        assert_ne!(a, Spec::one("b").seal());
        assert_ne!(a, Spec::three("a", "b", "c").seal());
        assert_ne!(a, Spec { help: true, ..Spec::one("a") }.seal());
        assert_ne!(a, Spec { zero: true, ..Spec::one("a") }.seal());
    }

    #[test]
    fn render_terminators() {
        let o = Spec::one("a/b").run();
        assert_eq!(o.render(false).as_str(), "a\n");
        assert_eq!(o.render(true).as_str(), "a\u{0}");
        assert_eq!(o.len_text().as_str(), "1");
        let m = Spec::three("a/b", "c/d", "e/f").run();
        assert_eq!(m.render(false).as_str(), "a\nc\ne\n");
        assert_eq!(m.len_text().as_str(), "3");
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert!(m.balanced());
        assert_eq!(m.row(0), Some(Row { total: 6, good: 4 }));
        assert_eq!(m.row(1), Some(Row { total: 5, good: 3 }));
        assert_eq!(m.row(2), Some(Row { total: 3, good: 1 }));
        assert_eq!(m.row(3), Some(Row { total: 0, good: 0 }));
        assert_eq!(m.row(4), None);
        assert_eq!(m.perfect_rows(), 0);
        assert!(!m.row(3).map(|r| r.perfect()).unwrap_or(true));
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
        assert_eq!(r.prefix, 4);
        assert_eq!(r.root, 3);
        assert_eq!(r.dot, 1);
        assert_eq!(r.meta, 0);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert!(r.green());
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "dirname corpus=14 accept=8 reject=6 prefix=4 root=3 dot=1 meta=0 match=14 rate=1000ppm\n"
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
        assert_eq!(r.accept, 7);
        assert_eq!(r.reject, 7);
        assert_eq!(r.gate(), Err(DN_ESTAGE));
        assert_eq!(run_with(Some((14, Fault::Code))).err(), Some(DN_ESLOT));
    }

    #[test]
    fn detail_exact() {
        let cs = cases();
        assert_eq!(detail(&cs[0]).as_str(), "dirname src/main.rs text=src len=3 stage=head note=prefix\n");
        assert_eq!(detail(&cs[2]).as_str(), "dirname a text=. len=1 stage=empty note=dot\n");
        assert_eq!(detail(&cs[3]).as_str(), "dirname / text=/ len=1 stage=allslash note=root\n");
        assert_eq!(detail(&cs[4]).as_str(), "dirname // text=// len=2 stage=allslash note=double-root\n");
        assert_eq!(detail(&cs[5]).as_str(), "dirname src/lib/ text=src len=3 stage=tail note=slash\n");
    }

    #[test]
    fn part_text_exact() {
        assert_eq!(part_text(&prefix_of("src/lib/")).as_str(), "src len=3 stage=tail note=slash");
        assert_eq!(part_text(&prefix_of("//")).as_str(), "// len=2 stage=allslash note=double-root");
        assert_eq!(part_text(&prefix_of("a")).as_str(), ". len=1 stage=empty note=dot");
    }

    #[test]
    fn slots_bounds() {
        assert!(case_at(0).is_some());
        assert!(case_at(13).is_some());
        assert!(case_at(14).is_none());
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(0), Err(DN_ENOBS));
        assert_eq!(case_at(7).map(|c| c.tag()), Some("absone"));
        assert_eq!(case_at(13).map(|c| c.tag()), Some("root-as-error"));
    }

    #[test]
    fn stage_order() {
        assert_eq!(Stage::Option.order(), 0);
        assert_eq!(Stage::Meta.order(), 1);
        assert_eq!(Stage::Empty.order(), 2);
        assert_eq!(Stage::AllSlash.order(), 3);
        assert_eq!(Stage::Tail.order(), 4);
        assert_eq!(Stage::Head.order(), 5);
    }

    #[test]
    fn tier_names() {
        assert_eq!(Tier::of(Note::Prefix).name(), "prefix");
        assert_eq!(Tier::of(Note::Slash).name(), "prefix");
        assert_eq!(Tier::of(Note::Root).name(), "root");
        assert_eq!(Tier::of(Note::DoubleRoot).name(), "root");
        assert_eq!(Tier::of(Note::Dot).name(), "dot");
        assert_eq!(Tier::of(Note::Help).name(), "meta");
        assert_eq!(Tier::of(Note::Version).name(), "meta");
        assert_eq!(Tier::of(Note::BadOpt).name(), "meta");
        assert_eq!(Tier::of(Note::Dot).index(), 2);
    }

    #[test]
    fn fault_names() {
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(fault_short(Fault::Stage), "stage");
        assert_eq!(fault_short(Fault::Pair), "pair");
        assert!(!Fault::Ok.alive());
        assert!(Fault::Len.alive());
        assert_eq!(Fault::Note.code(), DN_ENOTE);
        assert_eq!(Fault::Estate.code(), DN_ESTATE);
    }

    #[test]
    fn expected_table() {
        let mut i = 0usize;
        while i < ACCEPT {
            assert_eq!(expected(i), Some(Fault::Ok));
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            assert_eq!(expected(ACCEPT + j), Some(REJECT_FAULTS[j]));
            assert_eq!(REJECT_CODES[j], REJECT_FAULTS[j].code());
            j += 1;
        }
        assert_eq!(expected(CORPUS), None);
    }

    #[test]
    fn gate_monotonic() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Empty), Ok(()));
        assert_eq!(g.step(Stage::Option), Err(DN_EORDER));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.step(Stage::Head), Ok(()));
        assert_eq!(g.last(), 5);
        assert_eq!(g.step(Stage::Tail), Err(DN_EORDER));
        assert_eq!(g.close(), Ok(2));
    }

    #[test]
    fn gate_close() {
        let mut g = Gate::new();
        assert_eq!(g.close(), Ok(0));
        assert!(g.closed());
        assert_eq!(g.close(), Err(DN_ESTATE));
        assert_eq!(g.step(Stage::Empty), Err(DN_ESTATE));
        assert_eq!(g.steps(), 0);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(100).is_none());
        assert_eq!(l.sample(7).map(|s| s.value), Some(929));
        assert_eq!(l.sample(8).map(|s| s.code), Some(DN_ETEXT));
        assert_eq!(l.sample(9).map(|s| s.code), Some(DN_ESTAGE));
        assert_eq!(l.sample(13).map(|s| s.code), Some(DN_ESTATE));
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
        assert_eq!(l.register(Sample::blank()).err(), Some(DN_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(DN_EBAL));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0], "inject0 spec");
        assert!(out[1], "inject1 prefix");
        assert!(out[2], "inject2 render");
        assert!(out[3], "inject3 ledger");
    }

    #[test]
    fn spec_debug() {
        let a = Spec::one("src/main.rs").run();
        assert!(a.ok(), "a ok");
        assert_eq!(a.stage, Stage::Head, "a stage");
        assert_eq!(a.note, Note::Prefix, "a note");
        assert_eq!(a.text.as_str(), "src", "a text");
        assert_eq!(a.len, 3, "a len");
        assert_eq!(a.count, 1, "a count");
        let m = Spec::three("a/b", "c/d", "e/f").run();
        assert_eq!(m.text.as_str(), "a\nc\ne", "m text");
        assert_eq!(m.len, 3, "m len");
        assert_eq!(m.stage, Stage::Head, "m stage");
        assert_eq!(m.note, Note::Prefix, "m note");
        let help = Spec { help: true, ..Spec::one("a") }.run();
        assert_eq!(help.stage, Stage::Meta, "h stage");
        assert_eq!(help.len, 32, "h len");
        assert_eq!(help.count, 0, "h count");
        let ver = Spec { version: true, ..Spec::one("a") }.run();
        assert_eq!(ver.len, 30, "v len");
        let many = Spec { count: 4, ..Spec::one("a") }.run();
        assert_eq!(many.count, 4, "many count");
        assert!(!many.ok(), "many ok");
        let z = Spec { zero: true, ..Spec::one("a") }.run();
        assert_eq!(z.render(true).as_str(), ".\u{0}", "z nul");
        assert_ne!(Spec::one("a").seal(), Spec { zero: true, ..Spec::one("a") }.seal(), "z seal");
        assert_ne!(Spec::one("a").seal(), Spec { help: true, ..Spec::one("a") }.seal(), "h seal");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11502_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
