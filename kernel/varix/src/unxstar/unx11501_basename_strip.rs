//! UNX-F11501 basename 路径末段提取
//!
//! `basename` 看起来只是"取最后一个 `/` 之后的部分"，真正难的是**三道
//! 归一化在同一条路径上互相遮挡**：
//!
//! 1. **尾部斜杠**：`src/lib/` 的末段是 `lib` 而不是空串。尾部斜杠必须
//!    先剥掉，但剥的时候不能把"整条路径都是斜杠"剥成空——`/`、`//`、`///`
//!    的末段都是 `/`。把这条归一化放到取段之后，就会先取到空串，再拿空串
//!    去比后缀，结论全错。
//! 2. **空路径**：`basename ""` 输出 `.`，且后缀规则对它不生效。空串不是
//!    "空末段"，而是一个有名字的特例（当前目录）。把空串与"全是斜杠"
//!    合并处理，会让 `basename -s . ""` 输出空串而不是 `.`。
//! 3. **后缀剥离**：`-s` 只在**末段以该后缀结尾、且后缀不是整个末段**时
//!    剥离。`basename .gz -s .gz` 仍是 `.gz`——剥成全空没有意义；GNU 与
//!    POSIX 在这一点上一致。后缀比末段长、或干脆不匹配，都原样输出。
//!
//! 三道归一化的次序（也是本模块的闸门次序）：
//!
//! 选项 → 空路径 → 全斜杠 → 尾部剥斜杠 → 取末段 → 后缀比较 → 剥离/守门 → 就绪。
//!
//! 把"取末段"提到"尾部剥斜杠"之前，`src/lib/` 会得到空串；把"后缀比较"
//! 提到"全斜杠"之前，`basename -s / /` 会得到空串；把"空路径"并进"全斜杠"，
//! `""` 会被当成根。三者都是同一类错误：**归一化的对象还没定下来就做了
//! 归一化**。
//!
//! `-a` 是另一条轴：多个操作数必须显式声明 `-a`，否则第二个操作数就是
//! `extra operand`（退出码 1）。多操作数的文本是逐段用分隔符串起来的，
//! 末段、后缀、终止符三条规则对每个操作数独立生效；本模块把"最后一个
//! 操作数的阶段"作为整次运行的阶段，因为阶段是**判决路径**，不是输出
//! 内容。`-z` 只改终止符（`\0` 而不是 `\n`），不改判决。
//!
//! 判据六个维度：阶段、退出码、文本、文本长度、是否剥离、注解。少"长度"
//! 一维，`a` 与 `a\n` 这种尾随分隔符错误就看不出来；少"是否剥离"一维，
//! `x.gz` 与 `x.gz`（未剥）在文本上无法区分，只能靠注解猜。
//!
//! 判序：状态 → 阶段 → 退出码 → 文本 → 长度 → 剥离 → 注解。
//!
//! 语料 14 槽：8 槽申报正确（纯段 1、尾斜杠 3、根 1、空 1、后缀 1、
//! 守门 1），6 槽各挂一处错误申报。账本 14 格 8 成 6 败，冻结后重放
//! 同指纹。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本在
//! `audit` 逐格对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x6ce10;

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
pub const MAX_OPERANDS: usize = 2;

/// 退出码：成功。
pub const EXIT_OK: u32 = 0;

/// 退出码：操作数错误（extra operand）。
pub const EXIT_FAIL: u32 = 1;

/// 退出码：用法错误（未知选项、缺参）。
pub const EXIT_USAGE: u32 = 125;

// ---------------------------------------------------------------------------
// 错误码（13 个）
// ---------------------------------------------------------------------------

/// 判果互报。
pub const BN_ESTATE: u32 = SEG_BASE + 0x01;
/// 阶段申报不符。
pub const BN_ESTAGE: u32 = SEG_BASE + 0x02;
/// 退出码申报不符。
pub const BN_ECODE: u32 = SEG_BASE + 0x03;
/// 文本申报不符。
pub const BN_ETEXT: u32 = SEG_BASE + 0x04;
/// 长度申报不符。
pub const BN_ELEN: u32 = SEG_BASE + 0x05;
/// 剥离标志申报不符。
pub const BN_ESTRIP: u32 = SEG_BASE + 0x06;
/// 注解申报不符。
pub const BN_ENOTE: u32 = SEG_BASE + 0x07;
/// 槽位非法。
pub const BN_ESLOT: u32 = SEG_BASE + 0x08;
/// 指纹不符。
pub const BN_ESEAL: u32 = SEG_BASE + 0x09;
/// 无观测。
pub const BN_ENOBS: u32 = SEG_BASE + 0x0a;
/// 矩阵不平。
pub const BN_EMATRIX: u32 = SEG_BASE + 0x0b;
/// 阶段次序倒置。
pub const BN_EORDER: u32 = SEG_BASE + 0x0c;
/// 账本不平。
pub const BN_EBAL: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 选项解析。
pub const OP_OPT: u8 = 0x01;
/// 尾部归一。
pub const OP_TAIL: u8 = 0x02;
/// 取末段。
pub const OP_SEG: u8 = 0x03;
/// 后缀比较。
pub const OP_SUFFIX: u8 = 0x04;
/// 多操作数。
pub const OP_MULTI: u8 = 0x05;
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

/// 折入一个字符串（路径与后缀都要进指纹）。
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
    fold_byte(v, 0x1e)
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

/// 是否全斜杠路径（非空）。
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

/// 尾部斜杠剥掉后的结束下标；`all` 为真表示整条路径都是斜杠。
///
/// 保留至少一个字符，这样 `/` 不会被剥成空串——根的长度为一，是
/// "全斜杠"这一特例的锚点。
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

/// 在 `[0, end)` 里找最后一个斜杠。
fn last_slash(p: &str, end: usize) -> Option<usize> {
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

/// 后缀是否应剥离（严格短于末段且为其真后缀）。
///
/// 空后缀不剥；后缀等于整个末段不剥；后缀更长不剥。这三条不是优化，
/// 而是"剥完必须仍有名字"的不变式。
pub fn should_strip(seg: &str, suffix: &str) -> bool {
    if suffix.is_empty() {
        return false;
    }
    if suffix.len() >= seg.len() {
        return false;
    }
    ends_with(seg, suffix)
}

/// 字符串尾匹配（逐字节，避免 `str::ends_with` 之外再引依赖）。
pub fn ends_with(s: &str, tail: &str) -> bool {
    let a = s.as_bytes();
    let b = tail.as_bytes();
    if b.len() > a.len() {
        return false;
    }
    let off = a.len() - b.len();
    let mut i = 0usize;
    while i < b.len() {
        match (a.get(off + i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 去掉末尾 `n` 个字节（保证边界，失败返回原串）。
pub fn drop_tail(s: &str, n: usize) -> &str {
    if n == 0 || n >= s.len() {
        return s;
    }
    match s.get(0..s.len() - n) {
        Some(v) => v,
        None => s,
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
    /// 多操作数未声明 `-a`。
    Extra,
    /// 空路径。
    Empty,
    /// 全斜杠。
    Root,
    /// 尾部剥斜杠。
    Tail,
    /// 取末段。
    Segment,
    /// 后缀剥离。
    Suffix,
    /// 后缀守门（等于整段，不剥）。
    Guard,
}

impl Stage {
    /// 次序。
    pub fn order(&self) -> u8 {
        match self {
            Stage::Option => 0,
            Stage::Extra => 1,
            Stage::Empty => 2,
            Stage::Root => 3,
            Stage::Tail => 4,
            Stage::Segment => 5,
            Stage::Suffix => 6,
            Stage::Guard => 7,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Option => "option",
            Stage::Extra => "extra",
            Stage::Empty => "empty",
            Stage::Root => "root",
            Stage::Tail => "tail",
            Stage::Segment => "segment",
            Stage::Suffix => "suffix",
            Stage::Guard => "guard",
        }
    }
}

/// 注解。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// 纯末段，未做任何剥离。
    Plain,
    /// 剥过尾部斜杠。
    Slash,
    /// 根。
    Root,
    /// 空路径 → `.`。
    Dot,
    /// 后缀已剥离。
    Suffix,
    /// 后缀守门（整段即后缀）。
    Guard,
    /// 多操作数缺 `-a`。
    Extra,
    /// 用法错误。
    BadOpt,
    /// 路径边界非法。
    BadPath,
}

impl Note {
    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Note::Plain => "plain",
            Note::Slash => "slash",
            Note::Root => "root",
            Note::Dot => "dot",
            Note::Suffix => "suffix",
            Note::Guard => "guard",
            Note::Extra => "extra",
            Note::BadOpt => "bad-opt",
            Note::BadPath => "bad-path",
        }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        match self {
            Note::Plain => true,
            Note::Slash => true,
            Note::Root => true,
            Note::Dot => true,
            Note::Suffix => true,
            Note::Guard => true,
            _ => false,
        }
    }
}

/// 一段的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece<'a> {
    /// 阶段。
    pub stage: Stage,
    /// 退出码。
    pub code: u32,
    /// 注解。
    pub note: Note,
    /// 结果文本（不含终止符）。
    pub text: &'a str,
    /// 文本长度。
    pub len: u32,
    /// 是否剥离。
    pub stripped: bool,
}

impl<'a> Piece<'a> {
    /// 失败段（文本恒为空，故可静态）。
    pub const fn bad(stage: Stage, code: u32, note: Note) -> Piece<'static> {
        Piece { stage, code, note, text: "", len: 0, stripped: false }
    }
}

/// 一次运行的结果（多操作数时 `text` 已用 `\n` 连接）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Out {
    /// 阶段（最后一个操作数的阶段）。
    pub stage: Stage,
    /// 退出码。
    pub code: u32,
    /// 注解（最后一个操作数的注解）。
    pub note: Note,
    /// 文本（不含终止符）。
    pub text: String,
    /// 各段长度之和（不含连接符与终止符）。
    pub len: u32,
    /// 任一段发生剥离。
    pub stripped: bool,
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

    /// 渲染：文本 + 终止符（`-z` 时为 NUL）。
    pub fn render(&self, zero: bool) -> String {
        let mut s = String::from(self.text.as_str());
        s.push(if zero { '\u{0}' } else { '\n' });
        s
    }

    /// 文本长度文本。
    pub fn len_text(&self) -> String {
        dec_text(self.len)
    }
}

// ---------------------------------------------------------------------------
// 单段提取
// ---------------------------------------------------------------------------

/// 对单条路径做三段归一化并返回静态文本。
///
/// 返回值里的 `text` 取自输入串的切片，因此借用期与输入一致；调用方在
/// 组装 `Out` 时再复制到 `String`。这样单段路径零分配，只有多操作数
/// 拼接时才分配。
pub fn extract<'a>(path: &'a str, suffix: Option<&str>) -> Extracted<'a> {
    let bytes = path.as_bytes();
    if bytes.is_empty() {
        return Extracted {
            piece: Piece { stage: Stage::Empty, code: EXIT_OK, note: Note::Dot, text: ".", len: 1, stripped: false },
            zero_ok: true,
        };
    }
    let (end, root) = tail_end(path);
    if root {
        return Extracted {
            piece: Piece { stage: Stage::Root, code: EXIT_OK, note: Note::Root, text: "/", len: 1, stripped: false },
            zero_ok: true,
        };
    }
    let had_tail = end < bytes.len();
    let start = match last_slash(path, end) {
        Some(i) => i + 1,
        None => 0,
    };
    let seg = match path.get(start..end) {
        Some(s) => s,
        None => {
            return Extracted {
                piece: Piece::bad(Stage::Segment, EXIT_FAIL, Note::BadPath),
                zero_ok: false,
            };
        }
    };
    if seg.is_empty() {
        // 理论上不可达：尾斜杠已剥、且 end > start。保留显式拒绝。
        return Extracted {
            piece: Piece::bad(Stage::Segment, EXIT_FAIL, Note::BadPath),
            zero_ok: false,
        };
    }
    let mut stage = if had_tail { Stage::Tail } else { Stage::Segment };
    let mut note = if had_tail { Note::Slash } else { Note::Plain };
    let mut text = seg;
    let mut stripped = false;
    if let Some(suf) = suffix {
        if should_strip(seg, suf) {
            text = drop_tail(seg, suf.len());
            stage = Stage::Suffix;
            note = Note::Suffix;
            stripped = true;
        } else if !suf.is_empty() && suf.len() == seg.len() && seg == suf {
            stage = Stage::Guard;
            note = Note::Guard;
        }
    }
    Extracted {
        piece: Piece {
            stage,
            code: EXIT_OK,
            note,
            text,
            len: text.len() as u32,
            stripped,
        },
        zero_ok: true,
    }
}

/// 单段提取的借用视图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extracted<'a> {
    /// 判决段。
    pub piece: Piece<'a>,
    /// 保留位（单段无 NUL 风险）。
    pub zero_ok: bool,
}

impl<'a> Extracted<'a> {
    /// 文本长度。
    pub fn len(&self) -> u32 {
        self.piece.len
    }
}

// ---------------------------------------------------------------------------
// 运行规格
// ---------------------------------------------------------------------------

/// 一次 `basename` 调用的规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    /// 操作数（最多两个）。
    pub operands: [&'static str; MAX_OPERANDS],
    /// 有效操作数个数。
    pub count: u8,
    /// `-s` 后缀。
    pub suffix: Option<&'static str>,
    /// 是否给了 `-a`。
    pub all: bool,
    /// 是否 `-z`。
    pub zero: bool,
    /// 是否给了未知选项。
    pub bad_opt: bool,
    /// `-s` 是否缺参。
    pub missing_arg: bool,
}

impl Spec {
    /// 单操作数规格。
    pub const fn one(path: &'static str, suffix: Option<&'static str>) -> Spec {
        Spec {
            operands: [path, ""],
            count: 1,
            suffix,
            all: false,
            zero: false,
            bad_opt: false,
            missing_arg: false,
        }
    }

    /// 双操作数规格（必须 `-a`）。
    pub const fn two(a: &'static str, b: &'static str, suffix: Option<&'static str>, all: bool) -> Spec {
        Spec {
            operands: [a, b],
            count: 2,
            suffix,
            all,
            zero: false,
            bad_opt: false,
            missing_arg: false,
        }
    }

    /// 取第 i 个操作数。
    pub fn operand(&self, i: usize) -> Option<&'static str> {
        if i >= self.count as usize {
            return None;
        }
        self.operands.get(i).copied()
    }

    /// 是否空规格。
    pub fn empty(&self) -> bool {
        self.count == 0
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
        if let Some(s) = self.suffix {
            h = fold_str(h, s);
        }
        h = fold_word(h, if self.all { 1 } else { 0 });
        h = fold_word(h, if self.zero { 1 } else { 0 });
        h = fold_word(h, if self.bad_opt { 1 } else { 0 });
        h = fold_word(h, if self.missing_arg { 1 } else { 0 });
        h
    }

    /// 运行。
    pub fn run(&self) -> Out {
        if self.bad_opt || self.missing_arg {
            return Out {
                stage: Stage::Option,
                code: EXIT_USAGE,
                note: Note::BadOpt,
                text: String::new(),
                len: 0,
                stripped: false,
                count: 0,
            };
        }
        if self.count == 0 {
            return Out {
                stage: Stage::Option,
                code: EXIT_USAGE,
                note: Note::BadPath,
                text: String::new(),
                len: 0,
                stripped: false,
                count: 0,
            };
        }
        if self.count as usize > MAX_OPERANDS {
            return Out {
                stage: Stage::Extra,
                code: EXIT_FAIL,
                note: Note::Extra,
                text: String::new(),
                len: 0,
                stripped: false,
                count: self.count as u32,
            };
        }
        if (self.count as usize) > 1 && !self.all {
            // `-a` 缺失：第二个操作数是"多余操作数"，一个字符都不输出。
            return Out {
                stage: Stage::Extra,
                code: EXIT_FAIL,
                note: Note::Extra,
                text: String::new(),
                len: 0,
                stripped: false,
                count: self.count as u32,
            };
        }
        let mut out = Out {
            stage: Stage::Option,
            code: EXIT_OK,
            note: Note::Plain,
            text: String::new(),
            len: 0,
            stripped: false,
            count: self.count as u32,
        };
        let mut i = 0usize;
        while i < self.count as usize {
            let p = match self.operand(i) {
                Some(v) => v,
                None => {
                    out.code = EXIT_USAGE;
                    out.note = Note::BadPath;
                    out.stage = Stage::Option;
                    return out;
                }
            };
            let e = extract(p, self.suffix);
            if !e.piece.note.ok() {
                out.stage = e.piece.stage;
                out.code = e.piece.code;
                out.note = e.piece.note;
                out.text = String::new();
                out.len = 0;
                out.stripped = false;
                return out;
            }
            if i > 0 {
                out.text.push('\n');
            }
            out.text.push_str(e.piece.text);
            out.len = out.len.saturating_add(e.piece.len);
            if e.piece.stripped {
                out.stripped = true;
            }
            out.stage = e.piece.stage;
            out.note = e.piece.note;
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 语料形状与申报
// ---------------------------------------------------------------------------

/// 语料形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// 槽标签。
    pub tag: &'static str,
    /// 规格。
    pub spec: Spec,
}

impl Shape {
    /// 空白形状。
    pub const fn blank() -> Shape {
        Shape { tag: "", spec: Spec::one("", None) }
    }

    /// 实际结果。
    pub fn actual(&self) -> Out {
        self.spec.run()
    }
}

/// 一条申报。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Claim {
    /// 阶段。
    pub stage: Stage,
    /// 退出码。
    pub code: u32,
    /// 文本。
    pub text: &'static str,
    /// 文本长度。
    pub len: u32,
    /// 是否剥离。
    pub stripped: bool,
    /// 注解。
    pub note: Note,
}

impl Claim {
    /// 空白申报。
    pub const fn blank() -> Claim {
        Claim { stage: Stage::Option, code: 0, text: "", len: 0, stripped: false, note: Note::Plain }
    }

    /// 是否申报成功。
    pub fn success(&self) -> bool {
        self.code == EXIT_OK && self.note.ok()
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
    pub fn actual(&self) -> Out {
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
// 缺陷
// ---------------------------------------------------------------------------

/// 非 Ok 恰 13 个，与 13 个错误码一一对应。
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
    /// 剥离标志错。
    Stripped,
    /// 注解错。
    Note,
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
            Fault::Estate => BN_ESTATE,
            Fault::Stage => BN_ESTAGE,
            Fault::Code => BN_ECODE,
            Fault::Text => BN_ETEXT,
            Fault::Len => BN_ELEN,
            Fault::Stripped => BN_ESTRIP,
            Fault::Note => BN_ENOTE,
            Fault::Slot => BN_ESLOT,
            Fault::Seal => BN_ESEAL,
            Fault::Nobs => BN_ENOBS,
            Fault::Matrix => BN_EMATRIX,
            Fault::Order => BN_EORDER,
            Fault::Bal => BN_EBAL,
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
        Fault::Estate => "estate",
        Fault::Stage => "stage",
        Fault::Code => "code",
        Fault::Text => "text",
        Fault::Len => "len",
        Fault::Stripped => "stripped",
        Fault::Note => "note",
        Fault::Slot => "slot",
        Fault::Seal => "seal",
        Fault::Nobs => "nobs",
        Fault::Matrix => "matrix",
        Fault::Order => "order",
        Fault::Bal => "bal",
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 14 个形状。
pub const SHAPES: [Shape; CORPUS] = [
    Shape { tag: "plain", spec: Spec::one("src/main.rs", None) },
    Shape { tag: "trailing", spec: Spec::one("src/lib/", None) },
    Shape { tag: "root", spec: Spec::one("///", None) },
    Shape { tag: "empty", spec: Spec::one("", None) },
    Shape { tag: "suffix", spec: Spec::one("archive.tar.gz", Some(".gz")) },
    Shape { tag: "guard", spec: Spec::one(".gz", Some(".gz")) },
    Shape { tag: "doubleslash", spec: Spec::one("a//", None) },
    Shape { tag: "multi", spec: Spec::two("a/b", "c/", None, true) },
    Shape { tag: "root-as-dot", spec: Spec::one("///", None) },
    Shape { tag: "suffix-as-whole", spec: Spec::one("archive.tar.gz", Some(".gz")) },
    Shape { tag: "tail-as-segment", spec: Spec::one("src/lib/", None) },
    Shape { tag: "empty-as-root", spec: Spec::one("", None) },
    Shape { tag: "plain-len", spec: Spec::one("src/main.rs", None) },
    Shape { tag: "root-as-missing", spec: Spec::one("///", None) },
];

/// 与 `SHAPES` 一一对应的申报。
pub const CLAIMS: [Claim; CORPUS] = [
    // 0 纯末段。
    Claim { stage: Stage::Segment, code: EXIT_OK, text: "main.rs", len: 7, stripped: false, note: Note::Plain },
    // 1 尾斜杠。
    Claim { stage: Stage::Tail, code: EXIT_OK, text: "lib", len: 3, stripped: false, note: Note::Slash },
    // 2 根。
    Claim { stage: Stage::Root, code: EXIT_OK, text: "/", len: 1, stripped: false, note: Note::Root },
    // 3 空路径。
    Claim { stage: Stage::Empty, code: EXIT_OK, text: ".", len: 1, stripped: false, note: Note::Dot },
    // 4 后缀剥离。
    Claim { stage: Stage::Suffix, code: EXIT_OK, text: "archive.tar", len: 11, stripped: true, note: Note::Suffix },
    // 5 后缀守门。
    Claim { stage: Stage::Guard, code: EXIT_OK, text: ".gz", len: 3, stripped: false, note: Note::Guard },
    // 6 双尾斜杠。
    Claim { stage: Stage::Tail, code: EXIT_OK, text: "a", len: 1, stripped: false, note: Note::Slash },
    // 7 双操作数（末段阶段取最后一个操作数）。
    Claim { stage: Stage::Tail, code: EXIT_OK, text: "b\nc", len: 2, stripped: false, note: Note::Slash },
    // 8 根报成点 —— 文本错。
    Claim { stage: Stage::Root, code: EXIT_OK, text: ".", len: 1, stripped: false, note: Note::Root },
    // 9 后缀已剥却报未剥 —— 剥离标志错。
    Claim { stage: Stage::Suffix, code: EXIT_OK, text: "archive.tar", len: 11, stripped: false, note: Note::Suffix },
    // 10 尾斜杠报成普通末段 —— 阶段错。
    Claim { stage: Stage::Segment, code: EXIT_OK, text: "lib", len: 3, stripped: false, note: Note::Slash },
    // 11 空路径的注解报成根 —— 注解错。
    Claim { stage: Stage::Empty, code: EXIT_OK, text: ".", len: 1, stripped: false, note: Note::Root },
    // 12 长度多报一位 —— 长度错。
    Claim { stage: Stage::Segment, code: EXIT_OK, text: "main.rs", len: 8, stripped: false, note: Note::Plain },
    // 13 根报成缺文件 —— 状态互报。
    Claim { stage: Stage::Option, code: EXIT_FAIL, text: "", len: 0, stripped: false, note: Note::BadPath },
];

/// 后 6 槽的期望缺陷。
pub const REJECT_FAULTS: [Fault; REJECT] =
    [Fault::Text, Fault::Stripped, Fault::Stage, Fault::Note, Fault::Len, Fault::Estate];

/// 后 6 槽的缺陷码。
pub const REJECT_CODES: [u32; REJECT] = [
    BN_ETEXT,
    BN_ESTRIP,
    BN_ESTAGE,
    BN_ENOTE,
    BN_ELEN,
    BN_ESTATE,
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
        Err(BN_ENOBS)
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

/// 对判：状态 → 阶段 → 退出码 → 文本 → 长度 → 剥离 → 注解。
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
    if claim.stripped != a.stripped {
        return Fault::Stripped;
    }
    if claim.note != a.note {
        return Fault::Note;
    }
    Fault::Ok
}

// ---------------------------------------------------------------------------
// 释义
// ---------------------------------------------------------------------------

/// 单槽释义：
/// `basename <path> text=<t> len=<n> stage=<s> note=<n> strip=<yes|no>`。
pub fn detail(c: &Case) -> String {
    let a = c.actual();
    let mut s = String::from("basename ");
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
    s.push_str(" strip=");
    s.push_str(if a.stripped { "yes" } else { "no" });
    s.push_str("\n");
    s
}

/// 段释义（供多操作数逐段对账）：
/// `<text>[\\n] 长度=<n> 阶段=<s> 剥离=<0|1>`。
pub fn piece_text(p: &Piece) -> String {
    let mut s = String::new();
    s.push_str(p.text);
    s.push_str(" len=");
    s.push_str(dec_text(p.len).as_str());
    s.push_str(" stage=");
    s.push_str(p.stage.name());
    s.push_str(" strip=");
    s.push_str(if p.stripped { "1" } else { "0" });
    s
}

// ---------------------------------------------------------------------------
// 档位与矩阵
// ---------------------------------------------------------------------------

/// 判据档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 纯末段。
    Pure,
    /// 后缀剥离。
    Suffix,
    /// 尾部斜杠归一。
    Tail,
    /// 根/空/守门等特例。
    Special,
}

impl Tier {
    /// 由注解归位。
    pub fn of(n: Note) -> Tier {
        match n {
            Note::Plain => Tier::Pure,
            Note::Suffix => Tier::Suffix,
            Note::Slash => Tier::Tail,
            Note::Root => Tier::Special,
            Note::Dot => Tier::Special,
            Note::Guard => Tier::Special,
            Note::Extra => Tier::Special,
            Note::BadOpt => Tier::Special,
            Note::BadPath => Tier::Special,
        }
    }

    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Tier::Pure => 0,
            Tier::Suffix => 1,
            Tier::Tail => 2,
            Tier::Special => 3,
        }
    }

    /// 文本。
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Pure => "pure",
            Tier::Suffix => "suffix",
            Tier::Tail => "tail",
            Tier::Special => "special",
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总槽数。
    pub total: u32,
    /// 申报正确数。
    pub accept: u32,
    /// 申报错误数。
    pub reject: u32,
    /// 纯段档数。
    pub pure: u32,
    /// 剥离档数。
    pub suffix: u32,
    /// 尾斜杠档数。
    pub tail: u32,
    /// 特例档数。
    pub special: u32,
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
            Err(BN_ESTAGE)
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("basename corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" pure=");
        s.push_str(u32_text(self.pure).as_str());
        s.push_str(" suffix=");
        s.push_str(u32_text(self.suffix).as_str());
        s.push_str(" tail=");
        s.push_str(u32_text(self.tail).as_str());
        s.push_str(" special=");
        s.push_str(u32_text(self.special).as_str());
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
            return Err(BN_ESLOT);
        }
    }
    let cs = cases();
    let mut total = 0u32;
    let mut accept = 0u32;
    let mut reject = 0u32;
    let mut pure = 0u32;
    let mut suffix = 0u32;
    let mut tail = 0u32;
    let mut special = 0u32;
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match cs.get(i) {
            Some(v) => *v,
            None => return Err(BN_ENOBS),
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
            None => return Err(BN_ENOBS),
        };
        total += 1;
        if f == want {
            matched += 1;
        }
        if f == Fault::Ok {
            accept += 1;
            match Tier::of(a.note) {
                Tier::Pure => pure += 1,
                Tier::Suffix => suffix += 1,
                Tier::Tail => tail += 1,
                Tier::Special => special += 1,
            }
        } else {
            reject += 1;
        }
        i += 1;
    }
    Ok(Report { total, accept, reject, pure, suffix, tail, special, matched })
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
            return Err(BN_ESTATE);
        }
        let o = s.order() as i32;
        if o < self.last {
            return Err(BN_EORDER);
        }
        self.last = o;
        self.steps += 1;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(BN_ESTATE);
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
            return Err(BN_ESLOT);
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
            return Err(BN_EBAL);
        }
        // 前三格是语料尺寸，四到七格是档位实数，第八格是强制红后的 929‰。
        const VALS: [u32; ACCEPT] = [14, 8, 6, 1, 1, 3, 3, 929];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(BN_EBAL),
            };
            if s.slot as usize != i {
                return Err(BN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(BN_EBAL);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(BN_EBAL);
                        }
                    }
                    None => return Err(BN_EBAL),
                }
                vi += 1;
            } else {
                match REJECT_CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(BN_EBAL);
                        }
                    }
                    None => return Err(BN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != ACCEPT || ci != REJECT {
            return Err(BN_EBAL);
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
            return Err(BN_EBAL);
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
            Err(BN_ESEAL)
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
    let _ = l.register(count_cell(3, OP_SEG, r.map(|x| x.pure)));
    let _ = l.register(count_cell(4, OP_SUFFIX, r.map(|x| x.suffix)));
    let _ = l.register(count_cell(5, OP_TAIL, r.map(|x| x.tail)));
    let _ = l.register(count_cell(6, OP_TERM, r.map(|x| x.special)));

    let red = run_with(Some((0, Fault::Code)));
    let _ = l.register(count_cell(7, OP_OPT, red.map(|x| x.rate())));

    let cs = cases();
    let mut i = 0usize;
    while i < REJECT {
        if let Some(c) = cs.get(i + ACCEPT) {
            let f = judge(c);
            let _ = l.register(Sample {
                slot: (ACCEPT + i) as u8,
                op: OP_MULTI,
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
    out[1] = extract_ok() && suffix_ok() && multi_ok();
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
        BN_ESTATE, BN_ESTAGE, BN_ECODE, BN_ETEXT, BN_ELEN, BN_ESTRIP, BN_ENOTE, BN_ESLOT,
        BN_ESEAL, BN_ENOBS, BN_EMATRIX, BN_EORDER, BN_EBAL,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            match (codes.get(i), codes.get(j)) {
                (Some(a), Some(b)) => {
                    if a == b {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let a = SEG_BASE == 0x6ce10;
    let b = BN_ESTATE == SEG_BASE + 0x01 && BN_EBAL == SEG_BASE + 0x0d;
    let c = SAMPLES == 14 && CORPUS == 14 && ACCEPT == 8 && REJECT == 6;
    let d = EXIT_OK == 0 && EXIT_FAIL == 1 && EXIT_USAGE == 125;
    let e = MAX_OPERANDS == 2 && TIER_COUNT == 4;
    a && b && c && d && e
}

/// 语料规格表。
fn spec_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let s = match SHAPES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.tag.is_empty() || s.spec.count == 0 || s.spec.count as usize > MAX_OPERANDS {
            return false;
        }
        if s.spec.count > 1 && !s.spec.all {
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
        i += 1;
    }
    let a = SHAPES[0].spec.seal() != SHAPES[1].spec.seal();
    let b = SHAPES[4].spec.seal() != SHAPES[5].spec.seal();
    let c = SHAPES[4].spec.suffix == Some(".gz") && SHAPES[0].spec.suffix.is_none();
    a && b && c
}

/// 单段提取表。
fn extract_ok() -> bool {
    let mut ok = true;
    // (path, suffix, text, stage, note)
    let table = [
        ("src/main.rs", None, "main.rs", Stage::Segment, Note::Plain),
        ("/", None, "/", Stage::Root, Note::Root),
        ("//", None, "/", Stage::Root, Note::Root),
        ("///", None, "/", Stage::Root, Note::Root),
        ("", None, ".", Stage::Empty, Note::Dot),
        ("a/", None, "a", Stage::Tail, Note::Slash),
        ("a//", None, "a", Stage::Tail, Note::Slash),
        ("a/b/", None, "b", Stage::Tail, Note::Slash),
        ("a/b", None, "b", Stage::Segment, Note::Plain),
        ("a//b//", None, "b", Stage::Tail, Note::Slash),
        ("/a", None, "a", Stage::Segment, Note::Plain),
        ("//a", None, "a", Stage::Segment, Note::Plain),
        (".", None, ".", Stage::Segment, Note::Plain),
        ("..", None, "..", Stage::Segment, Note::Plain),
        ("a/.", None, ".", Stage::Segment, Note::Plain),
        ("a/..", None, "..", Stage::Segment, Note::Plain),
        ("x.gz", Some(".gz"), "x", Stage::Suffix, Note::Suffix),
        (".gz", Some(".gz"), ".gz", Stage::Guard, Note::Guard),
        ("x", Some(".gz"), "x", Stage::Segment, Note::Plain),
        ("", Some(".gz"), ".", Stage::Empty, Note::Dot),
    ];
    let mut i = 0usize;
    while i < table.len() {
        if let Some(row) = table.get(i) {
            let e = extract(row.0, row.1);
            if e.piece.text != row.2 {
                ok = false;
            }
            if e.piece.stage != row.3 || e.piece.note != row.4 {
                ok = false;
            }
            if e.piece.len as usize != row.2.len() {
                ok = false;
            }
            if e.piece.stripped != (row.3 == Stage::Suffix) {
                ok = false;
            }
        }
        i += 1;
    }
    ok
}

/// 后缀剥离的守门表。
fn suffix_ok() -> bool {
    let a = should_strip("x.gz", ".gz");
    let b = !should_strip(".gz", ".gz");
    let c = !should_strip("x", ".gz");
    let d = !should_strip("x", "");
    let e = !should_strip("gz", ".gz");
    let f = should_strip("a.b", "b") && should_strip("a.b", ".b");
    let g = !should_strip("a.b", ".a");
    let h = ends_with("archive.tar.gz", ".gz") && !ends_with("archive.tar.gz", ".bz2");
    let i = drop_tail("archive.tar.gz", 3) == "archive.tar";
    let j = drop_tail("x", 1) == "x" && drop_tail("x", 0) == "x";
    let k = match extract("dir/", Some("dir")).piece.note {
        Note::Guard => true,
        _ => false,
    };
    let l = match extract("dir/", Some("ir")).piece.text {
        "d" => true,
        _ => false,
    };
    a && b && c && d && e && f && g && h && i && j && k && l
}

/// 多操作数与终止符。
fn multi_ok() -> bool {
    let two = Spec::two("a/b", "c/", None, true);
    let o = two.run();
    let a = o.ok()
        && o.text.as_str() == "b\nc"
        && o.len == 2
        && o.stage == Stage::Tail
        && o.note == Note::Slash
        && o.count == 2;
    let b = o.render(false).as_str() == "b\nc\n";
    let c = o.render(true).as_str() == "b\nc\u{0}";
    let bad = Spec::two("a/b", "c/", None, false).run();
    let d = bad.stage == Stage::Extra
        && bad.code == EXIT_FAIL
        && bad.note == Note::Extra
        && bad.text.as_str().is_empty();
    let one = Spec::one("a/b", None).run();
    let e = one.render(true).as_str() == "b\u{0}" && one.render(false).as_str() == "b\n";
    let f = Spec::one("x", None).run().count == 1;
    a && b && c && d && e && f
}

/// 矩阵。
fn matrix_ok() -> bool {
    let m = Matrix::of();
    m.row(0) == Some(Row { total: 2, good: 1 })
        && m.row(1) == Some(Row { total: 2, good: 1 })
        && m.row(2) == Some(Row { total: 4, good: 3 })
        && m.row(3) == Some(Row { total: 6, good: 3 })
        && m.row(4).is_none()
        && m.total() == CORPUS
        && m.good() == ACCEPT
        && m.balanced()
        && m.perfect_rows() == 0
        && m.seal() == Matrix::of().seal()
        && m.seal() != Matrix::blank().seal()
}

/// 申报对判。
fn judge_ok() -> bool {
    let cs = cases();
    let mut i = 0usize;
    while i < ACCEPT {
        match cs.get(i) {
            Some(c) => {
                if judge(c) != Fault::Ok {
                    return false;
                }
            }
            None => return false,
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
        if judge(&c) != want || judge(&c).code() != code {
            return false;
        }
        j += 1;
    }
    true
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

/// 语料与渲染。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.pure == 1 && r.suffix == 1 && r.tail == 3 && r.special == 3;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = tags_unique() && judge_ok() && detail_ok();
    let e = SHAPES[4].actual().text.as_str() == "archive.tar";
    let f = SHAPES[7].actual().text.as_str() == "b\nc";
    a && b && c && d && e && f
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, Fault::Code))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == 13 && r.rate() == 929;
    let b = r.gate() == Err(BN_ESTAGE) && !r.green();
    let c = run_with(Some((CORPUS, Fault::Code))).err() == Some(BN_ESLOT);
    a && b && c
}

/// 纯段释义。
const DETAIL_PLAIN: &str =
    "basename src/main.rs text=main.rs len=7 stage=segment note=plain strip=no\n";
/// 尾斜杠释义。
const DETAIL_TAIL: &str = "basename src/lib/ text=lib len=3 stage=tail note=slash strip=no\n";
/// 后缀释义。
const DETAIL_SUFFIX: &str =
    "basename archive.tar.gz text=archive.tar len=11 stage=suffix note=suffix strip=yes\n";
/// 守门释义。
const DETAIL_GUARD: &str = "basename .gz text=.gz len=3 stage=guard note=guard strip=no\n";

/// 释义逐字。
fn detail_ok() -> bool {
    let cs = cases();
    let a = match cs.get(0) {
        Some(c) => detail(c).as_str() == DETAIL_PLAIN,
        None => false,
    };
    let b = match cs.get(1) {
        Some(c) => detail(c).as_str() == DETAIL_TAIL,
        None => false,
    };
    let c = match cs.get(4) {
        Some(c) => detail(c).as_str() == DETAIL_SUFFIX,
        None => false,
    };
    let d = match cs.get(5) {
        Some(c) => detail(c).as_str() == DETAIL_GUARD,
        None => false,
    };
    let e = piece_text(&extract("a/bc/", Some("c")).piece).as_str()
        == "b len=1 stage=suffix strip=1";
    a && b && c && d && e
}

/// 槽号、阶段名、档位名。
fn slots_ok() -> bool {
    let a = match case_at(0) {
        Some(c) => {
            c.tag() == "plain"
                && match c.shape.spec.operands.first() {
                    Some(p) => *p == "src/main.rs",
                    None => false,
                }
        }
        None => false,
    };
    let b = match case_at(13) {
        Some(c) => c.tag() == "root-as-missing",
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(BN_ENOBS);
    let e = Stage::Option.order() == 0
        && Stage::Extra.order() == 1
        && Stage::Empty.order() == 2
        && Stage::Root.order() == 3
        && Stage::Tail.order() == 4
        && Stage::Segment.order() == 5
        && Stage::Suffix.order() == 6
        && Stage::Guard.order() == 7;
    let f = Stage::Empty.name() == "empty"
        && Stage::Root.name() == "root"
        && Stage::Tail.name() == "tail"
        && Stage::Segment.name() == "segment"
        && Stage::Suffix.name() == "suffix"
        && Stage::Guard.name() == "guard";
    let g = Tier::Pure.index() == 0
        && Tier::Suffix.index() == 1
        && Tier::Tail.index() == 2
        && Tier::Special.index() == 3;
    let h = Tier::Pure.name() == "pure"
        && Tier::Suffix.name() == "suffix"
        && Tier::Tail.name() == "tail"
        && Tier::Special.name() == "special";
    let i = fault_short(Fault::Text) == "text"
        && fault_short(Fault::Stripped) == "stripped"
        && fault_short(Fault::Ok) == "ok";
    let j = Note::BadOpt.name() == "bad-opt" && Note::Dot.name() == "dot";
    let k = all_slashes("///") && !all_slashes("") && !all_slashes("/a");
    a && b && c && d && e && f && g && h && i && j && k
}

/// 阶段闸门。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    let a = g.steps() == 0 && !g.closed() && g.last() == -1;
    let b = g.step(Stage::Empty) == Ok(()) && g.steps() == 1;
    let c = g.step(Stage::Option) == Err(BN_EORDER) && g.steps() == 1;
    let d = g.step(Stage::Root) == Ok(()) && g.last() == 3 && g.steps() == 2;
    let e = g.step(Stage::Suffix) == Ok(()) && g.step(Stage::Guard) == Ok(()) && g.steps() == 4;
    let f = g.close() == Ok(4) && g.closed();
    let h = g.close() == Err(BN_ESTATE);
    let i = g.step(Stage::Guard) == Err(BN_ESTATE);
    a && b && c && d && e && f && h && i
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
pub fn run_unx11501_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11501_basename_strip");

    s.add("BN01-名称与段基址", name_ok(), "static");

    s.add("BN02-语料规格表", spec_ok(), "static");

    s.add("BN03-末段提取表", extract_ok(), "static");

    s.add("BN04-后缀守门表", suffix_ok(), "static");

    s.add("BN05-多操作数与终止符", multi_ok(), "static");

    s.add("BN06-申报对判", judge_ok(), "static");

    s.add("BN07-档位矩阵", matrix_ok(), "static");

    s.add("BN08-语料与渲染", corpus_ok(), "static");

    s.add("BN09-红档显式", reds_ok(), "static");

    s.add("BN10-释义逐字", detail_ok(), "static");

    s.add("BN11-槽号与命名", slots_ok(), "static");

    s.add("BN12-阶段闸门", gate_ok(), "static");

    s.add(
        "BN13-冻结重放四注入",
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
        panic!("unx11501 test fail")
    }

    #[test]
    fn seg_consts() {
        assert_eq!(SEG_BASE, 0x6ce10);
        assert_eq!(SAMPLES, 14);
        assert_eq!(CORPUS, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(EXIT_OK, 0);
        assert_eq!(EXIT_FAIL, 1);
        assert_eq!(EXIT_USAGE, 125);
    }

    #[test]
    fn code_uniqueness() {
        let codes = [
            BN_ESTATE, BN_ESTAGE, BN_ECODE, BN_ETEXT, BN_ELEN, BN_ESTRIP, BN_ENOTE, BN_ESLOT,
            BN_ESEAL, BN_ENOBS, BN_EMATRIX, BN_EORDER, BN_EBAL,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                assert_ne!(codes[i], codes[j]);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(BN_ESEAL, SEG_BASE + 9);
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
    }

    #[test]
    fn last_slash_table() {
        assert_eq!(last_slash("a/b", 3), Some(1));
        assert_eq!(last_slash("ab", 2), None);
        assert_eq!(last_slash("/ab", 3), Some(0));
        assert_eq!(last_slash("", 0), None);
    }

    #[test]
    fn ends_with_table() {
        assert!(ends_with("archive.tar.gz", ".gz"));
        assert!(!ends_with("archive.tar.gz", ".bz2"));
        assert!(!ends_with("x", "xx"));
        assert!(ends_with("x", ""));
        assert!(ends_with("abc", "abc"));
    }

    #[test]
    fn extract_plain() {
        let e = extract("src/main.rs", None);
        assert_eq!(e.piece.text, "main.rs");
        assert_eq!(e.piece.stage, Stage::Segment);
        assert_eq!(e.piece.note, Note::Plain);
        assert_eq!(e.piece.len, 7);
        assert!(!e.piece.stripped);
    }

    #[test]
    fn extract_trailing() {
        let e = extract("src/lib/", None);
        assert_eq!(e.piece.text, "lib");
        assert_eq!(e.piece.stage, Stage::Tail);
        assert_eq!(e.piece.note, Note::Slash);
        let d = extract("a//", None);
        assert_eq!(d.piece.text, "a");
        assert_eq!(d.piece.stage, Stage::Tail);
    }

    #[test]
    fn extract_roots() {
        let a = extract("/", None);
        assert_eq!(a.piece.text, "/");
        assert_eq!(a.piece.stage, Stage::Root);
        let b = extract("//", None);
        assert_eq!(b.piece.text, "/");
        let c = extract("///", None);
        assert_eq!(c.piece.text, "/");
        assert_eq!(c.piece.note, Note::Root);
    }

    #[test]
    fn extract_empty() {
        let e = extract("", None);
        assert_eq!(e.piece.text, ".");
        assert_eq!(e.piece.stage, Stage::Empty);
        assert_eq!(e.piece.note, Note::Dot);
        assert_eq!(e.piece.len, 1);
        let s = extract("", Some(".gz"));
        assert_eq!(s.piece.text, ".");
        assert_eq!(s.piece.note, Note::Dot);
    }

    #[test]
    fn extract_suffix() {
        let e = extract("archive.tar.gz", Some(".gz"));
        assert_eq!(e.piece.text, "archive.tar");
        assert_eq!(e.piece.stage, Stage::Suffix);
        assert!(e.piece.stripped);
        assert_eq!(e.piece.len, 11);
    }

    #[test]
    fn extract_guard() {
        let e = extract(".gz", Some(".gz"));
        assert_eq!(e.piece.text, ".gz");
        assert_eq!(e.piece.stage, Stage::Guard);
        assert_eq!(e.piece.note, Note::Guard);
        assert!(!e.piece.stripped);
        let d = extract("dir/", Some("dir"));
        assert_eq!(d.piece.stage, Stage::Guard);
        assert_eq!(d.piece.text, "dir");
    }

    #[test]
    fn extract_suffix_miss() {
        let a = extract("x", Some(".gz"));
        assert_eq!(a.piece.stage, Stage::Segment);
        assert_eq!(a.piece.text, "x");
        let b = extract("x", Some(""));
        assert_eq!(b.piece.stage, Stage::Segment);
        assert_eq!(b.piece.text, "x");
        let c = extract("gz", Some(".gz"));
        assert_eq!(c.piece.text, "gz");
    }

    #[test]
    fn spec_one() {
        let s = Spec::one("a/b", None);
        assert_eq!(s.count, 1);
        assert_eq!(s.operand(0), Some("a/b"));
        assert_eq!(s.operand(1), None);
        assert!(!s.empty());
        let o = s.run();
        assert!(o.ok());
        assert_eq!(o.text.as_str(), "b");
    }

    #[test]
    fn spec_two_all() {
        let s = Spec::two("a/b", "c/", None, true);
        let o = s.run();
        assert!(o.ok());
        assert_eq!(o.text.as_str(), "b\nc");
        assert_eq!(o.len, 2);
        assert_eq!(o.stage, Stage::Tail);
        assert_eq!(o.count, 2);
    }

    #[test]
    fn spec_two_extra() {
        let s = Spec::two("a/b", "c/", None, false);
        let o = s.run();
        assert!(!o.ok());
        assert_eq!(o.stage, Stage::Extra);
        assert_eq!(o.code, EXIT_FAIL);
        assert_eq!(o.note, Note::Extra);
        assert_eq!(o.text.as_str(), "");
    }

    #[test]
    fn spec_bad_opt() {
        let mut s = Spec::one("a", None);
        s.bad_opt = true;
        let o = s.run();
        assert_eq!(o.stage, Stage::Option);
        assert_eq!(o.code, EXIT_USAGE);
        assert_eq!(o.note, Note::BadOpt);
        let mut m = Spec::one("a", None);
        m.missing_arg = true;
        assert_eq!(m.run().code, EXIT_USAGE);
    }

    #[test]
    fn spec_zero_operands() {
        let mut s = Spec::one("a", None);
        s.count = 0;
        let o = s.run();
        assert_eq!(o.code, EXIT_USAGE);
        assert_eq!(o.note, Note::BadPath);
        assert!(s.empty());
    }

    #[test]
    fn render_terminators() {
        let o = Spec::one("a/b", None).run();
        assert_eq!(o.render(false).as_str(), "b\n");
        assert_eq!(o.render(true).as_str(), "b\u{0}");
        assert_eq!(o.len_text().as_str(), "1");
    }

    #[test]
    fn matrix_rows() {
        let m = Matrix::of();
        assert_eq!(m.total(), 14);
        assert_eq!(m.good(), 8);
        assert!(m.balanced());
        assert_eq!(m.row(0), Some(Row { total: 2, good: 1 }));
        assert_eq!(m.row(1), Some(Row { total: 2, good: 1 }));
        assert_eq!(m.row(2), Some(Row { total: 4, good: 3 }));
        assert_eq!(m.row(3), Some(Row { total: 6, good: 3 }));
        assert_eq!(m.row(4), None);
        assert_eq!(m.perfect_rows(), 0);
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
        assert_eq!(r.pure, 1);
        assert_eq!(r.suffix, 1);
        assert_eq!(r.tail, 3);
        assert_eq!(r.special, 3);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert!(r.green());
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "basename corpus=14 accept=8 reject=6 pure=1 suffix=1 tail=3 special=3 match=14 rate=1000ppm\n"
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
        assert_eq!(r.gate(), Err(BN_ESTAGE));
        assert_eq!(run_with(Some((14, Fault::Code))).err(), Some(BN_ESLOT));
    }

    #[test]
    fn detail_exact() {
        let cs = cases();
        assert_eq!(detail(&cs[0]).as_str(), DETAIL_PLAIN);
        assert_eq!(detail(&cs[1]).as_str(), DETAIL_TAIL);
        assert_eq!(detail(&cs[4]).as_str(), DETAIL_SUFFIX);
        assert_eq!(detail(&cs[5]).as_str(), DETAIL_GUARD);
    }

    #[test]
    fn piece_text_exact() {
        let p = extract("a/bc/", Some("c"));
        assert_eq!(piece_text(&p.piece).as_str(), "b len=1 stage=suffix strip=1");
    }

    #[test]
    fn slots_bounds() {
        assert!(case_at(0).is_some());
        assert!(case_at(13).is_some());
        assert!(case_at(14).is_none());
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(0), Err(BN_ENOBS));
        assert_eq!(case_at(7).map(|c| c.tag()), Some("multi"));
    }

    #[test]
    fn stage_order() {
        assert_eq!(Stage::Option.order(), 0);
        assert_eq!(Stage::Extra.order(), 1);
        assert_eq!(Stage::Empty.order(), 2);
        assert_eq!(Stage::Root.order(), 3);
        assert_eq!(Stage::Tail.order(), 4);
        assert_eq!(Stage::Segment.order(), 5);
        assert_eq!(Stage::Suffix.order(), 6);
        assert_eq!(Stage::Guard.order(), 7);
    }

    #[test]
    fn tier_names() {
        assert_eq!(Tier::of(Note::Plain).name(), "pure");
        assert_eq!(Tier::of(Note::Suffix).name(), "suffix");
        assert_eq!(Tier::of(Note::Slash).name(), "tail");
        assert_eq!(Tier::of(Note::Root).name(), "special");
        assert_eq!(Tier::of(Note::Dot).name(), "special");
        assert_eq!(Tier::of(Note::Guard).name(), "special");
    }

    #[test]
    fn fault_names() {
        assert_eq!(fault_short(Fault::Ok), "ok");
        assert_eq!(fault_short(Fault::Text), "text");
        assert_eq!(fault_short(Fault::Stripped), "stripped");
        assert!(!Fault::Ok.alive());
        assert!(Fault::Len.alive());
        assert_eq!(Fault::Note.code(), BN_ENOTE);
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
            j += 1;
        }
        assert_eq!(expected(CORPUS), None);
    }

    #[test]
    fn gate_monotonic() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Empty), Ok(()));
        assert_eq!(g.step(Stage::Option), Err(BN_EORDER));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.step(Stage::Segment), Ok(()));
        assert_eq!(g.last(), 5);
        assert_eq!(g.step(Stage::Guard), Ok(()));
        assert_eq!(g.steps(), 3);
    }

    #[test]
    fn gate_close() {
        let mut g = Gate::new();
        assert_eq!(g.close(), Ok(0));
        assert!(g.closed());
        assert_eq!(g.close(), Err(BN_ESTATE));
        assert_eq!(g.step(Stage::Empty), Err(BN_ESTATE));
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
        assert_eq!(l.sample(8).map(|s| s.code), Some(BN_ETEXT));
        assert_eq!(l.sample(13).map(|s| s.code), Some(BN_ESTATE));
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
        assert_eq!(l.register(Sample::blank()).err(), Some(BN_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(BN_EBAL));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0], "inject0 spec");
        assert!(out[1], "inject1 extract");
        assert!(out[2], "inject2 render");
        assert!(out[3], "inject3 ledger");
    }

    #[test]
    fn seal_differs() {
        let a = Spec::one("a/b", None).seal();
        let b = Spec::one("a/c", None).seal();
        let c = Spec::one("a/b", Some(".b")).seal();
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, Spec::one("a/b", None).seal());
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(u32_text(1000).as_str(), "1000");
        assert_eq!(hex_text(0x6ce10, 5).as_str(), "6ce10");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11501_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
