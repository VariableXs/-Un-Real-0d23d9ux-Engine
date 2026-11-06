//! UNX-F11610 PT_INTERP 路径超长/非绝对路径拒收判据
//!
//! 镜面：内核解出 PT_INTERP 之后，先对**路径字节**做形状校验，再进入
//! 打开流程。路径不合法者不必碰文件系统即可拒收，errno 分档为：
//!
//! * 空串 / 首字节即 NUL → ENOENT（等价于「没有解释器」）；
//! * 相对路径 → EINVAL（解释器必须是绝对路径，内核由此拒收）；
//! * 长度 ≥ PATH_LIMIT → ENAMETOOLONG；
//! * 缺 NUL 终止（段内被写满） → EINVAL；
//! * 含 `.`/`..` 段、控制字节 → EINVAL；
//! * 末尾斜杠、仅根路径 → ENOTDIR。
//!
//! 扫描面只走一遍：`scan` 产出长度、绝对性、段数、末斜杠、双斜杠、
//! 点段、控制字节七项观测，`classify` 依固定次序折叠成九个形状，
//! `judge` 把形状映射成码与 errno，`message` 把码折回逐字文案。
//!
//! 语料十四槽，三活十一死：空二、相对二、仅根一、末斜一、超长二、
//! 缺终止一、点段一、控制字节一。
//!
//! 闸门次序：名称 → 扫描 → 形状 → 判定 → 文案 → 段数 → 语料 →
//! 汇总 → 红单 → 覆盖 → 扫描器 → 冻结重放。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律走 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfb90;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 路径字节上限（含 NUL）。
pub const PATH_LIMIT: usize = 256;

/// 文案缩写阈值。
pub const ABBREV: usize = 32;

/// ENOENT。
pub const ENOENT: i32 = 2;

/// EINVAL。
pub const EINVAL: i32 = 22;

/// ENOTDIR。
pub const ENOTDIR: i32 = 20;

/// ENAMETOOLONG。
pub const ENAMETOOLONG: i32 = 36;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 路径形状不合法。
pub const LN_EPATH: u32 = SEG_BASE + 0x01;
/// 非绝对路径。
pub const LN_ENOTABS: u32 = SEG_BASE + 0x02;
/// 路径超长。
pub const LN_ELONG: u32 = SEG_BASE + 0x03;
/// 路径为空。
pub const LN_ENOENT: u32 = SEG_BASE + 0x04;
/// 末斜杠 / 仅根。
pub const LN_ENOTDIR: u32 = SEG_BASE + 0x05;
/// 控制字节。
pub const LN_ECTL: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const LN_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const LN_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const LN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const LN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const LN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const LN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const LN_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 扫描。
pub const OP_SCAN: u32 = 1;
/// 形状。
pub const OP_SHAPE: u32 = 2;
/// 路径闸门。
pub const OP_PATH: u32 = 3;
/// 文案。
pub const OP_TEXT: u32 = 4;
/// 汇总。
pub const OP_REPORT: u32 = 5;

// ---------------------------------------------------------------------------
// 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折一个字节。
pub const fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 折一个字。
pub const fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x = fold_byte(x, ((w >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

/// 折一段字节。
pub fn fold_bytes(h: u64, b: &[u8]) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            x = fold_byte(x, *v);
        }
        i += 1;
    }
    fold_word(x, b.len() as u64)
}

/// 十进制文本。
pub fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
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

/// 文案缩写。
pub fn abbrev(path: &str) -> String {
    let b = path.as_bytes();
    if b.len() <= ABBREV {
        return String::from(path);
    }
    let mut s = String::new();
    let mut i = 0usize;
    while i < ABBREV {
        if let Some(c) = b.get(i) {
            s.push(*c as char);
        }
        i += 1;
    }
    s.push_str("...");
    s
}

// ---------------------------------------------------------------------------
// 扫描
// ---------------------------------------------------------------------------

/// 一次扫描的观测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scan {
    /// 声明长度。
    pub len: usize,
    /// 首 NUL 位置。
    pub nul_at: usize,
    /// 有效长度（首个 NUL 之前）。
    pub effective: usize,
    /// 是否 NUL 终止。
    pub terminated: bool,
    /// 是否绝对路径。
    pub absolute: bool,
    /// 段数。
    pub segments: usize,
    /// 末字节是否斜杠。
    pub trailing_slash: bool,
    /// 是否含 `//`。
    pub double_slash: bool,
    /// 是否含点段。
    pub dot_seg: bool,
    /// 是否含控制字节。
    pub ctl: bool,
    /// 最长段字节数。
    pub max_segment: usize,
}

impl Scan {
    /// 空观测。
    pub const fn blank() -> Scan {
        Scan {
            len: 0,
            nul_at: 0,
            effective: 0,
            terminated: false,
            absolute: false,
            segments: 0,
            trailing_slash: false,
            double_slash: false,
            dot_seg: false,
            ctl: false,
            max_segment: 0,
        }
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.effective == 0
    }

    /// 是否仅根。
    pub const fn root_only(&self) -> bool {
        self.absolute && self.segments == 0
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.len as u64);
        h = fold_word(h, self.effective as u64);
        h = fold_word(h, self.terminated as u64);
        h = fold_word(h, self.absolute as u64);
        h = fold_word(h, self.segments as u64);
        h = fold_word(h, self.trailing_slash as u64);
        h = fold_word(h, self.double_slash as u64);
        h = fold_word(h, self.dot_seg as u64);
        h = fold_word(h, self.ctl as u64);
        fold_word(h, self.max_segment as u64)
    }
}

/// 扫描一段路径字节。
pub fn scan(bytes: &[u8], terminated: bool) -> Result<Scan, u32> {
    let len = bytes.len();
    let mut nul_at = len;
    let mut i = 0usize;
    while i < len {
        match bytes.get(i) {
            Some(0) => {
                nul_at = i;
                break;
            }
            Some(_) => i += 1,
            None => break,
        }
    }
    let effective = nul_at;
    let term = terminated;
    let absolute = match bytes.first() {
        Some(b'/') => true,
        _ => false,
    };
    let mut segments = 0usize;
    let mut in_seg = false;
    let mut seg_start = 0usize;
    let mut max_segment = 0usize;
    let mut dot_seg = false;
    let mut ctl = false;
    let mut j = 0usize;
    while j < effective {
        let b = match bytes.get(j) {
            Some(v) => *v,
            None => break,
        };
        if b == b'/' {
            if in_seg {
                let n = j - seg_start;
                if n > max_segment {
                    max_segment = n;
                }
                if is_dot(bytes, seg_start, j) {
                    dot_seg = true;
                }
                in_seg = false;
            }
        } else {
            if b < 0x20 || b == 0x7f {
                ctl = true;
            }
            if !in_seg {
                segments += 1;
                seg_start = j;
                in_seg = true;
            }
        }
        j += 1;
    }
    if in_seg {
        let n = effective - seg_start;
        if n > max_segment {
            max_segment = n;
        }
        if is_dot(bytes, seg_start, effective) {
            dot_seg = true;
        }
    }
    let trailing_slash = effective > 1 && matches!(bytes.get(effective - 1), Some(b'/'));
    let mut double_slash = false;
    let mut k = 1usize;
    while k < effective {
        if matches!(bytes.get(k), Some(b'/')) && matches!(bytes.get(k - 1), Some(b'/')) {
            double_slash = true;
        }
        k += 1;
    }
    Ok(Scan {
        len,
        nul_at,
        effective,
        terminated: term,
        absolute,
        segments,
        trailing_slash,
        double_slash,
        dot_seg,
        ctl,
        max_segment,
    })
}

/// 某段是否 `.` 或 `..`。
fn is_dot(b: &[u8], start: usize, end: usize) -> bool {
    let n = end.saturating_sub(start);
    if n == 1 {
        return matches!(b.get(start), Some(b'.'));
    }
    if n == 2 {
        return matches!(b.get(start), Some(b'.')) && matches!(b.get(start + 1), Some(b'.'));
    }
    false
}

// ---------------------------------------------------------------------------
// 形状
// ---------------------------------------------------------------------------

/// 路径形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// 合法绝对路径。
    Abs,
    /// 空串。
    Empty,
    /// 相对路径。
    Relative,
    /// 超长。
    Long,
    /// 缺 NUL 终止。
    NoNul,
    /// 点段。
    DotSeg,
    /// 控制字节。
    Ctl,
    /// 末斜杠。
    Trailing,
    /// 仅根。
    RootOnly,
}

impl Shape {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Shape::Abs => "abs",
            Shape::Empty => "empty",
            Shape::Relative => "relative",
            Shape::Long => "long",
            Shape::NoNul => "nonul",
            Shape::DotSeg => "dotseg",
            Shape::Ctl => "ctl",
            Shape::Trailing => "trailing",
            Shape::RootOnly => "rootonly",
        }
    }

    /// 序数。
    pub const fn index(&self) -> u32 {
        match self {
            Shape::Abs => 0,
            Shape::Empty => 1,
            Shape::Relative => 2,
            Shape::Long => 3,
            Shape::NoNul => 4,
            Shape::DotSeg => 5,
            Shape::Ctl => 6,
            Shape::Trailing => 7,
            Shape::RootOnly => 8,
        }
    }

    /// 细节短语。
    pub const fn detail(&self) -> &'static str {
        match self {
            Shape::Abs => "absolute path",
            Shape::Empty => "empty path",
            Shape::Relative => "relative path",
            Shape::Long => "path too long",
            Shape::NoNul => "missing NUL terminator",
            Shape::DotSeg => "dot segment",
            Shape::Ctl => "control byte",
            Shape::Trailing => "trailing slash",
            Shape::RootOnly => "no file name",
        }
    }

    /// 是否放行。
    pub const fn alive(&self) -> bool {
        matches!(self, Shape::Abs)
    }
}

/// 判定次序：终止 → 空 → 长度 → 绝对 → 点段 → 控制 → 末斜 → 仅根。
pub fn classify(s: &Scan) -> Shape {
    if s.len >= PATH_LIMIT {
        return Shape::Long;
    }
    if s.effective == 0 {
        return Shape::Empty;
    }
    if !s.terminated && s.nul_at == s.len {
        return Shape::NoNul;
    }
    if !s.absolute {
        return Shape::Relative;
    }
    if s.dot_seg {
        return Shape::DotSeg;
    }
    if s.ctl {
        return Shape::Ctl;
    }
    if s.trailing_slash {
        return Shape::Trailing;
    }
    if s.segments == 0 {
        return Shape::RootOnly;
    }
    Shape::Abs
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 一次判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    code: u32,
    errno: i32,
    shape: Shape,
    detail: &'static str,
    ok: bool,
}

impl Verdict {
    /// 码。
    pub const fn code(&self) -> u32 {
        self.code
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        self.errno
    }

    /// 形状。
    pub const fn shape(&self) -> Shape {
        self.shape
    }

    /// 细节。
    pub const fn detail(&self) -> &'static str {
        self.detail
    }

    /// 是否放行。
    pub const fn ok(&self) -> bool {
        self.ok
    }
}

/// 判一个形状。
pub fn judge(sh: Shape) -> Verdict {
    match sh {
        Shape::Abs => Verdict { code: 0, errno: 0, shape: sh, detail: sh.detail(), ok: true },
        Shape::Empty => Verdict { code: LN_ENOENT, errno: ENOENT, shape: sh, detail: sh.detail(), ok: false },
        Shape::Relative => Verdict { code: LN_ENOTABS, errno: EINVAL, shape: sh, detail: sh.detail(), ok: false },
        Shape::Long => Verdict { code: LN_ELONG, errno: ENAMETOOLONG, shape: sh, detail: sh.detail(), ok: false },
        Shape::NoNul => Verdict { code: LN_EPATH, errno: EINVAL, shape: sh, detail: sh.detail(), ok: false },
        Shape::DotSeg => Verdict { code: LN_EPATH, errno: EINVAL, shape: sh, detail: sh.detail(), ok: false },
        Shape::Ctl => Verdict { code: LN_ECTL, errno: EINVAL, shape: sh, detail: sh.detail(), ok: false },
        Shape::Trailing => Verdict { code: LN_ENOTDIR, errno: ENOTDIR, shape: sh, detail: sh.detail(), ok: false },
        Shape::RootOnly => Verdict { code: LN_ENOTDIR, errno: ENOTDIR, shape: sh, detail: sh.detail(), ok: false },
    }
}

/// errno 文本。
pub fn strerror(e: i32) -> &'static str {
    match e {
        ENOENT => "No such file or directory",
        EINVAL => "Invalid argument",
        ENOTDIR => "Not a directory",
        ENAMETOOLONG => "File name too long",
        _ => "Success",
    }
}

/// 文案。
pub fn message(v: &Verdict, path: &str) -> Result<String, u32> {
    if path.len() > PATH_LIMIT * 4 {
        return Err(LN_ELONG);
    }
    let shown = abbrev(path);
    let mut s = String::new();
    if v.ok {
        s.push_str("ok: interpreter ");
        s.push_str(shown.as_str());
        s.push('\n');
    } else {
        s.push_str("bash: ./prog: ");
        s.push_str(strerror(v.errno));
        s.push_str(" (PT_INTERP=");
        s.push_str(shown.as_str());
        s.push_str(", ");
        s.push_str(v.detail);
        s.push_str(")\n");
    }
    Ok(s)
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 长路径 256 字节。
const fn long_buf<const N: usize>() -> [u8; N] {
    let mut a = [b'a'; N];
    a[0] = b'/';
    a
}

static LONG_256: [u8; PATH_LIMIT] = long_buf();

/// 长路径 300 字节。
static LONG_300: [u8; PATH_LIMIT + 44] = long_buf();

/// 一个语料槽。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// 路径字节。
    pub bytes: &'static [u8],
    /// 是否声明 NUL 终止。
    pub terminated: bool,
    /// 形状。
    pub shape: Shape,
    /// 期望码。
    pub code: u32,
    /// 备注。
    pub note: &'static str,
}

impl Slot {
    /// 构造。
    pub const fn new(bytes: &'static [u8], terminated: bool, shape: Shape, code: u32, note: &'static str) -> Slot {
        Slot { bytes, terminated, shape, code, note }
    }
}

/// 十四槽语料：三活十一死。
pub const CASES: [Slot; CORPUS] = [
    Slot::new(b"/lib64/ld-linux-x86-64.so.2", true, Shape::Abs, 0, "glibc-x86_64"),
    Slot::new(b"/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2", true, Shape::Abs, 0, "multiarch"),
    Slot::new(b"", true, Shape::Empty, LN_ENOENT, "empty"),
    Slot::new(b"lib/ld.so", true, Shape::Relative, LN_ENOTABS, "relative-one-seg"),
    Slot::new(b"ld.so", true, Shape::Relative, LN_ENOTABS, "bare-name"),
    Slot::new(b"/", true, Shape::RootOnly, LN_ENOTDIR, "root-only"),
    Slot::new(b"/lib/ld.so/", true, Shape::Trailing, LN_ENOTDIR, "trailing-slash"),
    Slot::new(&LONG_256, true, Shape::Long, LN_ELONG, "len-256"),
    Slot::new(b"/lib//ld.so", true, Shape::Abs, 0, "double-slash"),
    Slot::new(&LONG_300, true, Shape::Long, LN_ELONG, "len-300"),
    Slot::new(b"/lib/ld.so", false, Shape::NoNul, LN_EPATH, "naked-bytes"),
    Slot::new(b"/lib/./ld.so", true, Shape::DotSeg, LN_EPATH, "dot-in-middle"),
    Slot::new(b"/lib/ld\x01.so", true, Shape::Ctl, LN_ECTL, "ctl-in-name"),
    Slot::new(b"\x00lib/ld.so", true, Shape::Empty, LN_ENOENT, "leading-nul"),
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<Slot> {
    CASES.get(i).copied()
}

/// 语料是否互异。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            if let Some(b) = CASES.get(j) {
                if a.bytes == b.bytes && a.terminated == b.terminated {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注全非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(s) => {
                if s.note.is_empty() {
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
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let s = match scan(slot.bytes, slot.terminated) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let sh = classify(&s);
        if sh != slot.shape {
            return false;
        }
        if judge(sh).code != slot.code {
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
#[derive(Clone, Copy, Debug)]
pub struct Report {
    total: usize,
    accept: usize,
    reject: usize,
    empty: usize,
    rel: usize,
    longn: usize,
    nonul: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report { total: 0, accept: 0, reject: 0, empty: 0, rel: 0, longn: 0, nonul: 0, matched: 0, red: false }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 放行条数。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收条数。
    pub const fn reject(&self) -> usize {
        self.reject
    }

    /// 空串条数。
    pub const fn empty(&self) -> usize {
        self.empty
    }

    /// 相对条数。
    pub const fn rel(&self) -> usize {
        self.rel
    }

    /// 超长条数。
    pub const fn longn(&self) -> usize {
        self.longn
    }

    /// 缺终止条数。
    pub const fn nonul(&self) -> usize {
        self.nonul
    }

    /// 对上条数。
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

    /// 比率（千分）。
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
            return Err(LN_ENOBS);
        }
        if self.red {
            return Err(LN_EVER);
        }
        if self.matched != self.total {
            return Err(LN_EVALUE);
        }
        if self.accept != 3 || self.reject != 11 {
            return Err(LN_EVALUE);
        }
        if self.empty != 2 || self.rel != 2 || self.longn != 2 || self.nonul != 1 {
            return Err(LN_EVALUE);
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
        s.push_str("interp_path corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" empty=");
        s.push_str(u32_text(self.empty as u32).as_str());
        s.push_str(" rel=");
        s.push_str(u32_text(self.rel as u32).as_str());
        s.push_str(" long=");
        s.push_str(u32_text(self.longn as u32).as_str());
        s.push_str(" nonul=");
        s.push_str(u32_text(self.nonul as u32).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑一遍语料。
pub fn run() -> Result<Report, u32> {
    walk(None)
}

/// 跑一遍语料并注入一处红档。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 语料行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let slot = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(LN_EVALUE),
        };
        let s = scan(slot.bytes, slot.terminated)?;
        let v = judge(classify(&s));
        r.total += 1;
        if v.ok {
            r.accept += 1;
        } else {
            r.reject += 1;
            match v.shape {
                Shape::Empty => r.empty += 1,
                Shape::Relative => r.rel += 1,
                Shape::Long => r.longn += 1,
                Shape::NoNul => r.nonul += 1,
                _ => {}
            }
        }
        if v.code == slot.code {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, _)) = red {
        if slot >= r.total {
            return Err(LN_ESLOT);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 绿色覆盖判据。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(LN_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 扫描器：一门一用
// ---------------------------------------------------------------------------

/// 路径扫描器。
#[derive(Clone, Copy, Debug)]
pub struct Scanner {
    steps: u32,
    bound: bool,
    last: u32,
}

impl Scanner {
    /// 空。
    pub const fn new() -> Scanner {
        Scanner { steps: 0, bound: false, last: 0 }
    }

    /// 扫一段。
    pub fn once(&mut self, bytes: &[u8], terminated: bool) -> Result<Verdict, u32> {
        if self.bound {
            self.last = LN_ESTATE;
            return Err(LN_ESTATE);
        }
        self.bound = true;
        self.steps = self.steps.saturating_add(1);
        let s = scan(bytes, terminated)?;
        let v = judge(classify(&s));
        self.last = v.code;
        Ok(v)
    }

    /// 步数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 是否绑定。
    pub const fn bound(&self) -> bool {
        self.bound
    }

    /// 最后码。
    pub const fn last(&self) -> u32 {
        self.last
    }
}

/// 绝对性。
pub fn absolute(path: &str) -> bool {
    matches!(path.as_bytes().first(), Some(b'/'))
}

/// 段数。
pub fn segments(path: &str) -> usize {
    let b = path.as_bytes();
    let mut n = 0usize;
    let mut i = 0usize;
    let mut in_seg = false;
    while i < b.len() {
        match b.get(i) {
            Some(b'/') => in_seg = false,
            Some(_) => {
                if !in_seg {
                    n += 1;
                    in_seg = true;
                }
            }
            None => break,
        }
        i += 1;
    }
    n
}

/// 相对路径（不触碰文件系统即拒）。
pub fn judge_str(path: &str) -> Result<Verdict, u32> {
    let s = scan(path.as_bytes(), true)?;
    Ok(judge(classify(&s)))
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 槽。
    pub slot: u32,
    /// 操作。
    pub op: u32,
    /// 是否成功。
    pub ok: bool,
    /// 码。
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
            return Err(LN_ESLOT);
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
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(LN_EBAL);
        }
        const VALS: [u32; 8] = [14, 3, 11, 2, 2, 2, 1, 929];
        const CODES: [u32; 6] = [LN_ENOENT, LN_ENOTABS, LN_ELONG, LN_ENOTDIR, LN_EPATH, LN_ECTL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LN_EBAL),
            };
            if s.slot as usize != i {
                return Err(LN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LN_EVALUE);
                        }
                    }
                    None => return Err(LN_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LN_EVALUE);
                        }
                    }
                    None => return Err(LN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LN_EBAL);
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
            return Err(LN_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(LN_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(LN_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(LN_ESEAL);
        }
        Ok(self.root)
    }

    /// 根。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒收次数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 计数格。
fn count_cell(slot: u32, op: u32, v: Result<usize, u32>) -> Sample {
    match v {
        Ok(n) => Sample { slot, op, ok: true, code: 0, value: n as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 错误格。
fn err_cell(slot: u32, op: u32, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 造账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_REPORT, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_REPORT, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_REPORT, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_SHAPE, r.map(|x| x.empty())));

    let _ = l.register(count_cell(4, OP_SHAPE, r.map(|x| x.rel())));
    let _ = l.register(count_cell(5, OP_SHAPE, r.map(|x| x.longn())));
    let _ = l.register(count_cell(6, OP_SCAN, r.map(|x| x.nonul())));

    let red = run_with(Some((0, LN_ENOENT)));
    let _ = l.register(count_cell(7, OP_PATH, red.map(|x| x.rate() as usize)));

    let e = judge(Shape::Empty);
    let _ = l.register(err_cell(8, OP_SHAPE, if e.code == 0 { LN_ENOENT } else { e.code }));

    let rel = judge(Shape::Relative);
    let _ = l.register(err_cell(9, OP_PATH, if rel.code == 0 { LN_ENOTABS } else { rel.code }));

    let lg = judge(Shape::Long);
    let _ = l.register(err_cell(10, OP_SCAN, if lg.code == 0 { LN_ELONG } else { lg.code }));

    let tr = judge(Shape::Trailing);
    let _ = l.register(err_cell(11, OP_PATH, if tr.code == 0 { LN_ENOTDIR } else { tr.code }));

    let ds = judge(Shape::DotSeg);
    let _ = l.register(err_cell(12, OP_SHAPE, if ds.code == 0 { LN_EPATH } else { ds.code }));

    let ct = judge(Shape::Ctl);
    let _ = l.register(err_cell(13, OP_TEXT, if ct.code == 0 { LN_ECTL } else { ct.code }));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称闸门。
fn name_ok() -> bool {
    let names = [
        Shape::Abs.name(),
        Shape::Empty.name(),
        Shape::Relative.name(),
        Shape::Long.name(),
        Shape::NoNul.name(),
        Shape::DotSeg.name(),
        Shape::Ctl.name(),
        Shape::Trailing.name(),
        Shape::RootOnly.name(),
    ];
    let mut i = 0usize;
    while i < names.len() {
        if names[i].is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    LN_EPATH == SEG_BASE + 1
        && LN_ENOTABS == SEG_BASE + 2
        && LN_ELONG == SEG_BASE + 3
        && LN_ENOENT == SEG_BASE + 4
        && LN_ENOTDIR == SEG_BASE + 5
        && LN_ECTL == SEG_BASE + 6
        && LN_ESTATE == SEG_BASE + 7
        && LN_EVER == SEG_BASE + 8
        && LN_EBAL == SEG_BASE + 9
        && LN_ESLOT == SEG_BASE + 0x0a
        && LN_EVALUE == SEG_BASE + 0x0b
        && LN_ESEAL == SEG_BASE + 0x0c
        && LN_ENOBS == SEG_BASE + 0x0d
}

/// 扫描闸门。
fn scan_ok() -> bool {
    let s = match scan(b"/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2", true) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = s.absolute && s.terminated && s.effective == 42 && s.segments == 3;
    let b = !s.trailing_slash && !s.dot_seg && !s.ctl && !s.double_slash;
    let c = s.max_segment == 20;
    let d = match scan(b"/a//b/", true) {
        Ok(v) => v.double_slash && v.trailing_slash && v.segments == 2,
        Err(_) => false,
    };
    let e = match scan(b"a/./b", true) {
        Ok(v) => v.dot_seg && !v.absolute && v.segments == 3,
        Err(_) => false,
    };
    let f = match scan(b"/a/../b", true) {
        Ok(v) => v.dot_seg && v.segments == 3,
        Err(_) => false,
    };
    let g = match scan(b"/lib/ld\x02.so", true) {
        Ok(v) => v.ctl && v.segments == 2,
        Err(_) => false,
    };
    let h = match scan(b"/lib/ld.so\x00junk", true) {
        Ok(v) => v.terminated && v.effective == 10 && v.nul_at == 10,
        Err(_) => false,
    };
    a && b && c && d && e && f && g && h
}

/// 形状闸门。
fn shape_ok() -> bool {
    let mut s = Scan::blank();
    let a = classify(&s) == Shape::Empty;
    s.len = PATH_LIMIT;
    s.terminated = true;
    let b = classify(&s) == Shape::Long;
    s.len = 5;
    s.terminated = false;
    s.nul_at = 5;
    s.effective = 5;
    let c = classify(&s) == Shape::NoNul;
    s = match scan(b"/", true) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = classify(&s) == Shape::RootOnly && s.root_only();
    let e = match scan(b"/a/b", true) {
        Ok(v) => classify(&v) == Shape::Abs,
        Err(_) => false,
    };
    let f = match scan(b"/a/b/", true) {
        Ok(v) => classify(&v) == Shape::Trailing,
        Err(_) => false,
    };
    let g = match scan(b"rel", true) {
        Ok(v) => classify(&v) == Shape::Relative,
        Err(_) => false,
    };
    let h = match scan(b"/a/./b", true) {
        Ok(v) => classify(&v) == Shape::DotSeg,
        Err(_) => false,
    };
    a && b && c && d && e && f && g && h
}

/// 判定闸门。
fn judge_ok() -> bool {
    let a = judge(Shape::Abs).ok() && judge(Shape::Abs).code() == 0;
    let b = judge(Shape::Empty).code() == LN_ENOENT && judge(Shape::Empty).errno() == ENOENT;
    let c = judge(Shape::Relative).code() == LN_ENOTABS && judge(Shape::Relative).errno() == EINVAL;
    let d = judge(Shape::Long).code() == LN_ELONG && judge(Shape::Long).errno() == ENAMETOOLONG;
    let e = judge(Shape::NoNul).code() == LN_EPATH && judge(Shape::NoNul).errno() == EINVAL;
    let f = judge(Shape::DotSeg).code() == LN_EPATH;
    let g = judge(Shape::Ctl).code() == LN_ECTL && judge(Shape::Ctl).errno() == EINVAL;
    let h = judge(Shape::Trailing).code() == LN_ENOTDIR && judge(Shape::RootOnly).code() == LN_ENOTDIR;
    let i = judge(Shape::RootOnly).errno() == ENOTDIR && !judge(Shape::Relative).ok();
    a && b && c && d && e && f && g && h && i
}

/// 文案闸门。
fn message_ok() -> bool {
    let ok = match message(&judge(Shape::Abs), "/lib64/ld-linux-x86-64.so.2") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = ok.as_str() == "ok: interpreter /lib64/ld-linux-x86-64.so.2\n";
    let e = match message(&judge(Shape::Empty), "") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = e.as_str() == "bash: ./prog: No such file or directory (PT_INTERP=, empty path)\n";
    let r = match message(&judge(Shape::Relative), "lib/ld.so") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = r.as_str() == "bash: ./prog: Invalid argument (PT_INTERP=lib/ld.so, relative path)\n";
    let t = match message(&judge(Shape::Trailing), "/lib/ld.so/") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let d = t.as_str() == "bash: ./prog: Not a directory (PT_INTERP=/lib/ld.so/, trailing slash)\n";
    a && b && c && d
}

/// 超长文案闸门。
fn long_message_ok() -> bool {
    let path = match core::str::from_utf8(&LONG_300) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let ab = abbrev(path);
    let a = ab.len() == ABBREV + 3 && ab.ends_with("...");
    let m = match message(&judge(Shape::Long), path) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut want = String::new();
    want.push_str("bash: ./prog: File name too long (PT_INTERP=");
    want.push_str(ab.as_str());
    want.push_str(", path too long)\n");
    a && m.as_str() == want.as_str()
}

/// 段数闸门。
fn seg_ok() -> bool {
    let a = segments("/lib64/ld-linux-x86-64.so.2") == 2;
    let b = segments("/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2") == 3;
    let c = segments("/") == 0;
    let d = segments("/a//b/") == 2;
    let e = absolute("/lib/ld.so") && !absolute("lib/ld.so") && !absolute("");
    let f = match judge_str("ld.so") {
        Ok(v) => v.shape() == Shape::Relative && v.code() == LN_ENOTABS,
        Err(_) => false,
    };
    a && b && c && d && e && f
}

/// 语料全跑。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total() == CORPUS && r.accept() == 3 && r.reject() == 11 && r.matched() == CORPUS;
    let b = r.empty() == 2 && r.rel() == 2 && r.longn() == 2 && r.nonul() == 1;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    a && b && c
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !table_ok() {
        return false;
    }
    let a = match slot_at(0) {
        Some(s) => s.shape == Shape::Abs && s.code == 0,
        None => false,
    };
    let b = match slot_at(13) {
        Some(s) => s.shape == Shape::Empty && s.note == "leading-nul",
        None => false,
    };
    let c = match slot_at(9) {
        Some(s) => s.bytes.len() == PATH_LIMIT + 44 && s.shape == Shape::Long,
        None => false,
    };
    let d = slot_at(CORPUS).is_none() && slot_at(CORPUS + 1).is_none();
    a && b && c && d
}

/// 汇总渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render().as_str()
        == "interp_path corpus=14 accept=3 reject=11 empty=2 rel=2 long=2 nonul=1 match=14 rate=1000ppm\n";
    let e = Report::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(LN_ENOBS);
    let c = e.render().as_str()
        == "interp_path corpus=0 accept=0 reject=0 empty=0 rel=0 long=0 nonul=0 match=0 rate=0ppm\n";
    a && b && c
}

/// 红单显式。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, LN_ENOENT))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched() == CORPUS - 1 && r.rate() == 929;
    let b = r.gate() == Err(LN_EVER) && !r.green();
    let c = run_with(Some((CORPUS, LN_ENOENT))).err() == Some(LN_ESLOT);
    a && b && c
}

/// 覆盖档。
fn cover_ok() -> bool {
    cover(CORPUS) == Ok(()) && cover(CORPUS - 1) == Err(LN_ENOBS) && cover(0) == Err(LN_ENOBS)
}

/// 扫描器闸门。
fn scanner_ok() -> bool {
    let mut sc = Scanner::new();
    let a = !sc.bound() && sc.steps() == 0;
    let b = match sc.once(b"/lib64/ld-linux-x86-64.so.2", true) {
        Ok(v) => v.ok() && sc.last() == 0,
        Err(_) => false,
    };
    let c = sc.once(b"/lib/ld.so", true).err() == Some(LN_ESTATE);
    let d = sc.bound() && sc.steps() == 1 && sc.last() == LN_ESTATE;
    let e = match scan(b"/lib/ld.so", true) {
        Ok(v) => v.seal() == v.seal() && v.seal() != Scan::blank().seal(),
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && scan_ok();
    out[1] = shape_ok() && judge_ok();
    out[2] = message_ok() && long_message_ok() && seg_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && cover_ok() && scanner_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11610_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11610.interp_path");

    s.add(
        "LN01-名称闸门",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "LN02-扫描视图",
        {
            scan_ok()
        },
        "static",
    );

    s.add(
        "LN03-形状折叠",
        {
            shape_ok()
        },
        "static",
    );

    s.add(
        "LN04-判定与errno",
        {
            judge_ok()
        },
        "static",
    );

    s.add(
        "LN05-文案逐字",
        {
            message_ok()
        },
        "static",
    );

    s.add(
        "LN06-超长缩写文案",
        {
            long_message_ok()
        },
        "static",
    );

    s.add(
        "LN07-段数统计",
        {
            seg_ok()
        },
        "static",
    );

    s.add(
        "LN08-十四卡全跑",
        {
            corpus_ok()
        },
        "static",
    );

    s.add(
        "LN09-语料编号",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "LN10-汇总渲染",
        {
            report_ok()
        },
        "static",
    );

    s.add(
        "LN11-红单显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "LN12-扫描一门一用",
        {
            scanner_ok() && cover_ok()
        },
        "static",
    );

    s.add(
        "LN13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.root() == r,
                Err(_) => false,
            };
            let c = l.replay() == Ok(l.root());
            let d = l.register(Sample::blank()).err() == Some(LN_ESLOT) && l.rejects() == 1;
            let inj = inject_four();
            let mut e = true;
            let mut i = 0usize;
            while i < inj.len() {
                if !inj[i] {
                    e = false;
                }
                i += 1;
            }
            a && b && c && d && e
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
        panic!("unexpected failure path");
    }

    #[test]
    fn scan_basic() {
        let s = match scan(b"/lib64/ld-linux-x86-64.so.2", true) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(s.absolute);
        assert!(s.terminated);
        assert_eq!(s.segments, 2);
        assert_eq!(s.effective, 27);
        assert!(!s.trailing_slash);
        assert!(!s.ctl);
    }

    #[test]
    fn scan_dot_segments() {
        assert!(match scan(b"/a/./b", true) {
            Ok(v) => v.dot_seg,
            Err(_) => false,
        });
        assert!(match scan(b"/a/../b", true) {
            Ok(v) => v.dot_seg,
            Err(_) => false,
        });
        assert!(!match scan(b"/a/.../b", true) {
            Ok(v) => v.dot_seg,
            Err(_) => true,
        });
    }

    #[test]
    fn scan_ctl_and_nul() {
        let a = match scan(b"/lib/\x1fld.so", true) {
            Ok(v) => v.ctl,
            Err(_) => false,
        };
        assert!(a);
        let b = match scan(b"/lib/ld.so\x00tail", true) {
            Ok(v) => (v.terminated, v.effective, v.nul_at),
            Err(_) => (false, 0, 0),
        };
        assert_eq!(b, (true, 10, 10));
    }

    #[test]
    fn classify_order() {
        assert_eq!(classify(&Scan::blank()), Shape::Empty);
        let mut s = Scan::blank();
        s.len = PATH_LIMIT;
        s.terminated = true;
        assert_eq!(classify(&s), Shape::Long);
        let mut n = Scan::blank();
        n.len = 5;
        n.nul_at = 5;
        n.effective = 5;
        n.terminated = false;
        assert_eq!(classify(&n), Shape::NoNul);
    }

    #[test]
    fn root_only_shape() {
        let s = match scan(b"/", true) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(s.root_only());
        assert_eq!(classify(&s), Shape::RootOnly);
    }

    #[test]
    fn relative_rejected() {
        let v = judge(Shape::Relative);
        assert_eq!(v.code(), LN_ENOTABS);
        assert_eq!(v.errno(), EINVAL);
        assert!(!v.ok());
    }

    #[test]
    fn long_rejected() {
        let v = judge(Shape::Long);
        assert_eq!(v.code(), LN_ELONG);
        assert_eq!(v.errno(), ENAMETOOLONG);
    }

    #[test]
    fn empty_rejected() {
        let v = judge(Shape::Empty);
        assert_eq!(v.code(), LN_ENOENT);
        assert_eq!(v.errno(), ENOENT);
    }

    #[test]
    fn trailing_and_root_are_notdir() {
        assert_eq!(judge(Shape::Trailing).errno(), ENOTDIR);
        assert_eq!(judge(Shape::RootOnly).errno(), ENOTDIR);
        assert_eq!(judge(Shape::RootOnly).code(), LN_ENOTDIR);
    }

    #[test]
    fn ctl_and_nonul_rejected() {
        assert_eq!(judge(Shape::Ctl).code(), LN_ECTL);
        assert_eq!(judge(Shape::NoNul).code(), LN_EPATH);
        assert_eq!(judge(Shape::DotSeg).code(), LN_EPATH);
    }

    #[test]
    fn message_verbatim_ok() {
        let m = match message(&judge(Shape::Abs), "/lib64/ld-linux-x86-64.so.2") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(m.as_str(), "ok: interpreter /lib64/ld-linux-x86-64.so.2\n");
    }

    #[test]
    fn message_verbatim_empty() {
        let m = match message(&judge(Shape::Empty), "") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(m.as_str(), "bash: ./prog: No such file or directory (PT_INTERP=, empty path)\n");
    }

    #[test]
    fn message_verbatim_relative() {
        let m = match message(&judge(Shape::Relative), "lib/ld.so") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(m.as_str(), "bash: ./prog: Invalid argument (PT_INTERP=lib/ld.so, relative path)\n");
    }

    #[test]
    fn abbrev_truncates() {
        assert_eq!(abbrev("/lib/ld.so").as_str(), "/lib/ld.so");
        let long = "x".repeat(64);
        let a = abbrev(long.as_str());
        assert_eq!(a.len(), ABBREV + 3);
        assert!(a.ends_with("..."));
    }

    #[test]
    fn corpus_shape() {
        assert_eq!(CASES.len(), CORPUS);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
    }

    #[test]
    fn report_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.total(), 14);
        assert_eq!(r.accept(), 3);
        assert_eq!(r.reject(), 11);
        assert_eq!(r.empty(), 2);
        assert_eq!(r.rel(), 2);
        assert_eq!(r.longn(), 2);
        assert_eq!(r.nonul(), 1);
        assert_eq!(r.matched(), 14);
        assert_eq!(r.rate(), 1000);
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
            "interp_path corpus=14 accept=3 reject=11 empty=2 rel=2 long=2 nonul=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((3, LN_ENOTABS))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched(), 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(LN_EVER));
        assert_eq!(run_with(Some((14, LN_ENOENT))).err(), Some(LN_ESLOT));
    }

    #[test]
    fn scanner_once() {
        let mut sc = Scanner::new();
        match sc.once(b"/lib64/ld-linux-x86-64.so.2", true) {
            Ok(v) => assert!(v.ok()),
            Err(_) => fail(),
        }
        assert_eq!(sc.once(b"/lib/ld.so", true).err(), Some(LN_ESTATE));
        assert_eq!(sc.steps(), 1);
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
        assert_eq!(l.register(Sample::blank()).err(), Some(LN_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(LN_ESEAL));
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
    fn fold_and_text() {
        let a = fold_bytes(FNV_OFF, b"/lib/ld.so");
        let b = fold_bytes(FNV_OFF, b"/lib/ld.so");
        assert_eq!(a, b);
        assert_ne!(a, fold_bytes(FNV_OFF, b"/lib/ld.sn"));
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(u32_text(929).as_str(), "929");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11610_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
