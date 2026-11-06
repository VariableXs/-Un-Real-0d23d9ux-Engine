//! UNX-F11694 ENOSPC 文案逐字判据（No space left on device）
//!
//! 这一卡只判一件事：**那 23 个字节**。`No space left on device` 是 `strerror(28)`
//! 的原文，二十三个 ASCII 字节，没有句号、没有空格、没有大小写变体。七个工具
//! （glibc 的 `error()`、busybox dd、GNU dd、rust coreutils、rsync、tee、以及
//! `strerror` 本身）各有各的承载方式：有的裸说，有的加前缀，有的带引号包住路径，
//! 有的在尾巴上再挂一个 errno。判据要求**分解与合成两条路逐字相等**：把前缀、
//! 裸句、后缀、换行四段拆开量一遍，再把整行逐字节对一遍，两路任何一处不合即红。
//!
//! ```text
//! strerror(28)      No space left on device                                   裸
//! glibc error()     write error: No space left on device\n                    前缀
//! busybox dd        dd: writing 'x': No space left on device\n                引号
//! GNU dd            dd: error writing 'x': No space left on device\n          引号
//! rust coreutils    failed to write: No space left on device (os error 28)\n  后缀
//! rsync             rsync: write failed on "x": No space left on device (28)\n 后缀
//! tee               tee: 'x': No space left on device\n                       引号
//! ```
//!
//! 四型体例（`TextShape`）是本卡的粗判：
//!
//! ```text
//! Bare      前缀与后缀都空，且不带换行——只有 strerror 本身
//! Prefixed  有前缀、无后缀，不夹引号——glibc 的 error()
//! Suffixed  前缀之后还挂一段 errno 尾巴——rust coreutils、rsync
//! Quoted    前缀里夹着被引号包住的路径——busybox dd、GNU dd、tee
//! ```
//!
//! 体例不是"风格分类"，它是**判死依据**：`Suffixed` 型必须真有后缀，
//! `Quoted` 型必须真带引号，`Bare` 型必须真的一句不加。把 rust 的行判成
//! `Bare`，等于把 ` (os error 28)` 这十五个字节直接抹掉——逐字判据当场失效。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八段，秩 1..=8，只许前进：
//!
//! 1. **Arg**——错误号入表。必须恰好 `28`；`122`（EDQUOT）不算"差不多"，
//!    那是另一张卡（F11695）要分清的邻居。
//! 2. **Tool**——整行归属。整行必须以该工具自己的前缀开头；连前缀都不认，
//!    后面的裸句对齐只是巧合。
//! 3. **Shape**——体例归属。工具声明的体例必须与现场观测一致。
//! 4. **Prefix**——前缀逐字。多一个空格、少一个冒号都判死。
//! 5. **Core**——裸句逐字。二十三个字节，不允许尾随句号或空格。
//! 6. **Suffix**——后缀逐字。` (os error 28)` 与 ` (28)` 不许互换。
//! 7. **Bytes**——整行逐字节。长度、换行位、整行文本三处同时成立才算过。
//! 8. **Rate**——结算。没有判决落账的现场一律进兜底档，不许当样本。
//!
//! 次序不能倒：Arg 在 Tool 之前（错号了还谈什么文案），Tool 在 Shape 之前
//! （行都不属于这个工具，体例分类毫无意义），Prefix/Core/Suffix 在 Bytes 之前
//! （先分段量，再整行对；反过来整行一对，分段里藏的错误就被一个"相等"吞掉）。
//!
//! ## 关键差异（本卡深度所在，谁判死谁放行）
//!
//! * **与 F11695（ENOSPC vs EDQUOT，段 `0x6d3e0`）分层**：F11695 判"28 与 122
//!   别混"，本卡判"28 的文案一个字都不许改"。本卡里 `122` 只在 Arg 闸被拒一次，
//!   正文全是 28。两卡共用裸句常量，但判据面不重叠。
//! * **与 F11697（dd 无错却 ENOSPC 分档，段 `0x6d410`）分层**：F11697 判"dd 的
//!   报错分档"，本卡判"报错里的那 23 字节"。dd 只是七个承载工具之一。
//! * **与 F11679（场景 × 退出码矩阵，段 `0x6d2e0`）分层**：F11679 判码，本卡判
//!   字。码对字错，F11679 绿、本卡红；字对码错，反之亦然。
//! * **与 F11696（释放后重试恢复，段 `0x6d3f0`）分层**：F11696 判"重试成功后的
//!   字节数与残余"，本卡判"报错那一刻的字"。前者是正判据面，后者是文案面。
//! * **谁判死**：错号、异行、体例不符、前缀/裸句/后缀任一漂移、字节数不对、
//!   换行位不对——各落不同的码，使"字错"与"号错""体例错"分得开。
//! * **谁放行**：七工具各自的整行逐字成立即放行；`Bare` 型额外要求无换行，
//!   `Quoted` 型额外要求引号对，`Suffixed` 型额外要求后缀非空。
//! * **两种写法**：`strerror(28)` 走裸句直取；工具行走"前缀 + 裸句 + 后缀 +
//!   换行"四段合成。两条路径必须给同一串字节，任何一侧笔误都露出来。
//!
//! 语料十四槽：前八槽覆盖裸句型与六种承载型（含 repeat 抽样），后六槽各坏一维
//! （estate/stage/code/value/note/default）。账本十四格 = 七统计 + 一红档率
//! （929）+ 六缺陷码，冻结后重放同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本卡段基址（全仓唯一，VX5 分配表给定，不许改）。
pub const SEG_BASE: u32 = 0x6d3d0;

// ---------------------------------------------------------------------------
// 联签锚
// ---------------------------------------------------------------------------

/// 前置邻居 F11695（ENOSPC vs EDQUOT）的段基址。
pub const ANCHOR_F11695_SEG: u32 = 0x6d3e0;

/// 后继邻居 F11696（释放后重试恢复）的段基址。
pub const ANCHOR_F11696_SEG: u32 = 0x6d3f0;

/// 上游同族卡 F11693 的推定段基址（文案面之前一卡）。
pub const ANCHOR_F11693_SEG: u32 = 0x6d3c0;

/// 与前置邻居的段位差。
pub const ANCHOR_F11695_GAP: u32 = ANCHOR_F11695_SEG - SEG_BASE;

/// 与后继邻居的段位差。
pub const ANCHOR_F11696_GAP: u32 = ANCHOR_F11696_SEG - SEG_BASE;

// ---------------------------------------------------------------------------
// 规模常量（契约给定，逐卡一致）
// ---------------------------------------------------------------------------

/// 语料槽数。
pub const CORPUS: usize = 14;
/// 账本槽数。
pub const SAMPLES: usize = 14;
/// 正例槽数。
pub const ACCEPT: usize = 8;
/// 反例槽数。
pub const REJECT: usize = 6;
/// 矩阵档位数。
pub const TIER_COUNT: usize = 4;
/// 闸门段数。
pub const STAGE_COUNT: u32 = 8;
/// 闸门末段秩。
pub const STAGE_LAST_RANK: u32 = 8;
/// 统计格数。
pub const STAT_CELLS: usize = 7;
/// 红档率千分比。
pub const RED_RATE: u32 = 929;
/// 满额千分比。
pub const RATE_FULL: u32 = 1000;

// ---------------------------------------------------------------------------
// 本卡规模：七工具 × 四型体例
// ---------------------------------------------------------------------------

/// 承载工具数。
pub const TOOL_COUNT: usize = 7;
/// 体例型数。
pub const SHAPE_COUNT: usize = 4;
/// 裸句字节数（`No space left on device` 恰好 23 个 ASCII 字节）。
pub const BARE_LEN: u32 = 23;
/// ENOSPC 错误号。
pub const ENOSPC: i32 = 28;
/// EDQUOT 错误号（邻居卡的对象，本卡只用来拒绝）。
pub const EDQUOT: i32 = 122;

/// 裸句型槽数（strerror 两条抽样）。
pub const BARE_SAMPLES: usize = 2;
/// 承载型槽数（六条工具行）。
pub const CARRIED_SAMPLES: usize = 6;
/// 语料落 `Void` 的槽数（体例错一条）。
pub const VOID_SAMPLES: usize = 1;
/// 语料落 `Unrun` 的槽数（其余五条反例）。
pub const UNRUN_SAMPLES: usize = 5;
/// 带后缀的工具数（rust coreutils 与 rsync）。
pub const SUFFIX_TOOLS: usize = 2;
/// 带引号路径的工具数（busybox dd、GNU dd、tee）。
pub const QUOTED_TOOLS: usize = 3;
/// 需要换行的工具数（除 strerror 外全部）。
pub const NEWLINE_TOOLS: usize = TOOL_COUNT - 1;

// ---------------------------------------------------------------------------
// 错误码（13 个，SEG_BASE + 0x01..0x0d）
// ---------------------------------------------------------------------------

/// 入参非法：错误号不是 28。
pub const TX_EARG: u32 = SEG_BASE + 0x01;
/// 整行不归属该工具（前缀对不上）。
pub const TX_ETOOL: u32 = SEG_BASE + 0x02;
/// 体例不符。
pub const TX_ESHAPE: u32 = SEG_BASE + 0x03;
/// 前缀漂移。
pub const TX_EPREFIX: u32 = SEG_BASE + 0x04;
/// 裸句漂移。
pub const TX_ECORE: u32 = SEG_BASE + 0x05;
/// 后缀漂移。
pub const TX_ESUFFIX: u32 = SEG_BASE + 0x06;
/// 字节数或换行位不符。
pub const TX_EBYTES: u32 = SEG_BASE + 0x07;
/// 阶段不符或倒退。
pub const TX_ESTAGE: u32 = SEG_BASE + 0x08;
/// 成败互报。
pub const TX_ESTATE: u32 = SEG_BASE + 0x09;
/// 统计值不符。
pub const TX_EVALUE: u32 = SEG_BASE + 0x0a;
/// 注记不符。
pub const TX_ENOTE: u32 = SEG_BASE + 0x0b;
/// 兜底档被当成结论。
pub const TX_EDEFAULT: u32 = SEG_BASE + 0x0c;
/// 槽位越界。
pub const TX_ESLOT: u32 = SEG_BASE + 0x0d;

/// 十三码全表。
pub const ALL_CODES: [u32; 13] = [
    TX_EARG, TX_ETOOL, TX_ESHAPE, TX_EPREFIX, TX_ECORE, TX_ESUFFIX, TX_ESTAGE, TX_ESTATE,
    TX_EVALUE, TX_ENOTE, TX_EDEFAULT, TX_ESLOT, TX_EBYTES,
];

/// 反例六维对应的六个码。
pub const REJECT_CODES: [u32; REJECT] = [
    TX_ESTATE,
    TX_ESTAGE,
    TX_EBYTES,
    TX_EVALUE,
    TX_ENOTE,
    TX_EDEFAULT,
];

/// 七个统计格的钉值 + 第八格红档率（929）。
pub const VALS: [u32; ACCEPT] = [
    TOOL_COUNT as u32,
    SHAPE_COUNT as u32,
    ACCEPT as u32,
    REJECT as u32,
    BARE_LEN,
    ENOSPC as u32,
    RATE_FULL,
    RED_RATE,
];

/// 七个统计格归属的段号。
pub const STAT_OPS: [u8; STAT_CELLS] = [1, 2, 3, 4, 5, 6, 7];

/// 红档格在账本里的段号。
pub const OPID_STAT_RED: u8 = 0x08;
/// 六个缺陷码格的段号起点。
pub const DEFECT_OP_BASE: u8 = 0x21;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 初始偏移量。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折一字节进哈希。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折一个 u64。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = w;
    let mut acc = h;
    let mut i = 0usize;
    while i < 8 {
        acc = fold_byte(acc, (v & 0xff) as u8);
        v = v >> 8;
        i += 1;
    }
    acc
}

/// 折一条字符串。
fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc = fold_byte(acc, *b);
    }
    acc
}

/// 折一个 i32。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as u32 as u64)
}

/// 折一个布尔。
fn fold_bool(h: u64, v: bool) -> u64 {
    fold_byte(h, if v { 1 } else { 0 })
}

// ---------------------------------------------------------------------------
// 数字/布尔文本
// ---------------------------------------------------------------------------

/// u32 → 十进制文本。
fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    while n > 0 && i > 0 {
        let d = (n % 10) as u8;
        n /= 10;
        i -= 1;
        if let Some(cell) = buf.get_mut(i) {
            *cell = b'0' + d;
        }
    }
    let mut s = String::new();
    let mut j = i;
    while j < 10 {
        if let Some(c) = buf.get(j) {
            s.push(*c as char);
        }
        j += 1;
    }
    s
}

/// u32 → 定宽十六进制文本。
fn hex_text(v: u32, digits: usize) -> String {
    let table: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::new();
    if digits == 0 {
        return s;
    }
    let mut i = digits;
    while i > 0 {
        i -= 1;
        let nib = ((v >> (i * 4)) & 0xf) as usize;
        let c = match table.get(nib) {
            Some(x) => *x,
            None => b'0',
        };
        s.push(c as char);
    }
    s
}

/// u32 文本别名。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 文本（负号显式）。
fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text((v as i64).unsigned_abs() as u32).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 布尔文本。
fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// 段基址文本。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 5).as_str());
    s
}

/// 千分比文本。
pub fn permille_text(v: u32) -> String {
    let mut s = dec_text(v);
    s.push_str("permille");
    s
}

/// 字节数文本：`23B`。
pub fn bytes_text(v: u32) -> String {
    let mut s = dec_text(v);
    s.push('B');
    s
}

/// 错误号文本：`errno=28`。
pub fn errno_text(v: i32) -> String {
    let mut s = String::from("errno=");
    s.push_str(i32_text(v).as_str());
    s
}

// ---------------------------------------------------------------------------
// 上游文案逐字复刻
// ---------------------------------------------------------------------------

/// 裸句：`strerror(28)` 原文，恰好 23 字节，无标点、无换行。
pub const BARE: &str = "No space left on device";
/// EDQUOT 的裸句（邻居卡对象，本卡只作对照）。
pub const BARE_EDQUOT: &str = "Disk quota exceeded";
/// glibc `error()` 一行（含换行）。
pub const T_GLIBC: &str = "write error: No space left on device\n";
/// busybox dd 一行（含换行）。
pub const T_BUSYBOX: &str = "dd: writing 'x': No space left on device\n";
/// GNU dd 一行（含换行）。
pub const T_GNUDD: &str = "dd: error writing 'x': No space left on device\n";
/// rust coreutils 一行（含换行）。
pub const T_RUST: &str = "failed to write: No space left on device (os error 28)\n";
/// rsync 一行（含换行）。
pub const T_RSYNC: &str = "rsync: write failed on \"x\": No space left on device (28)\n";
/// tee 一行（含换行）。
pub const T_TEE: &str = "tee: 'x': No space left on device\n";

/// glibc 前缀。
pub const P_GLIBC: &str = "write error: ";
/// busybox dd 前缀。
pub const P_BUSYBOX: &str = "dd: writing 'x': ";
/// GNU dd 前缀。
pub const P_GNUDD: &str = "dd: error writing 'x': ";
/// rust coreutils 前缀。
pub const P_RUST: &str = "failed to write: ";
/// rsync 前缀。
pub const P_RSYNC: &str = "rsync: write failed on \"x\": ";
/// tee 前缀。
pub const P_TEE: &str = "tee: 'x': ";
/// 空前缀。
pub const P_NONE: &str = "";

/// rust coreutils 后缀。
pub const S_RUST: &str = " (os error 28)";
/// rsync 后缀。
pub const S_RSYNC: &str = " (28)";
/// 空后缀。
pub const S_NONE: &str = "";

/// 近失裸句：多一个句号（本卡用它证明逐字判据真的在逐字看）。
pub const BARE_PERIOD: &str = "No space left on device.";
/// 近失裸句：多一个尾随空格。
pub const BARE_TRAIL: &str = "No space left on device ";
/// 近失前缀：少一个空格。
pub const P_GLIBC_TIGHT: &str = "write error:";
/// 近失后缀：多一个句号。
pub const S_RUST_PERIOD: &str = " (os error 28).";
/// 近失后缀：把 rust 的尾巴挂到别处。
pub const S_WRONG: &str = " (os error 5)";

/// 空文案哨兵（工具行永远非空，用它在 Tool 闸判死）。
pub const T_EMPTY: &str = "";

/// 按错误号取裸句；只认 28 与 122，其余给 None（绝不夹带默认文案）。
pub fn strerror(e: i32) -> Option<&'static str> {
    match e {
        28 => Some(BARE),
        122 => Some(BARE_EDQUOT),
        _ => None,
    }
}

/// 合成一行：`前缀 + 裸句 + 后缀 + 换行`。
pub fn compose(prefix: &str, bare: &str, suffix: &str, newline: bool) -> String {
    let mut s = String::new();
    s.push_str(prefix);
    s.push_str(bare);
    s.push_str(suffix);
    if newline {
        s.push('\n');
    }
    s
}

/// 拆分一行：返回 `(前缀长度, 裸句长度, 后缀长度, 是否换行)`；裸句不在行内给 None。
pub fn split_line(text: &str, bare: &str) -> Option<(usize, usize, usize, bool)> {
    if bare.len() == 0 {
        return None;
    }
    let found = text.find(bare)?;
    let mut newline = false;
    let mut body = text.len();
    if text.ends_with('\n') {
        newline = true;
        body = body.saturating_sub(1);
    }
    let prefix = found;
    let tail = body.saturating_sub(found + bare.len());
    let suffix = tail;
    Some((prefix, bare.len(), suffix, newline))
}

/// 工具名与裸句的报告行。
pub fn msg_line(tool: &str, bytes: u32) -> String {
    let mut s = String::new();
    s.push_str(tool);
    s.push_str(": ");
    s.push_str(bytes_text(bytes).as_str());
    s.push_str(" verbatim");
    s
}

/// 前缀漂移。
pub fn msg_prefix(tool: &str, a: &str, b: &str) -> String {
    let mut s = String::new();
    s.push_str(tool);
    s.push_str(" prefix ");
    s.push_str(a);
    s.push_str(" != ");
    s.push_str(b);
    s
}

/// 裸句漂移。
pub fn msg_core(tool: &str, got: &str) -> String {
    let mut s = String::new();
    s.push_str(tool);
    s.push_str(" core is ");
    s.push_str(got);
    s
}

/// 后缀漂移。
pub fn msg_suffix(tool: &str, a: &str, b: &str) -> String {
    let mut s = String::new();
    s.push_str(tool);
    s.push_str(" suffix ");
    s.push_str(a);
    s.push_str(" != ");
    s.push_str(b);
    s
}

/// 体例不符。
pub fn msg_shape(tool: &str, want: &str, got: &str) -> String {
    let mut s = String::new();
    s.push_str(tool);
    s.push_str(" shape ");
    s.push_str(got);
    s.push_str(" != ");
    s.push_str(want);
    s
}

/// 字节数不符。
pub fn msg_bytes(want: u32, got: u32) -> String {
    let mut s = String::from("bytes ");
    s.push_str(dec_text(got).as_str());
    s.push_str(" != ");
    s.push_str(dec_text(want).as_str());
    s
}

/// 错误号不符。
pub fn msg_errno(got: i32) -> String {
    let mut s = String::from("expected ");
    s.push_str(errno_text(ENOSPC).as_str());
    s.push_str(" got ");
    s.push_str(errno_text(got).as_str());
    s
}

/// 归属不符。
pub fn msg_foreign(tool: &str) -> String {
    let mut s = String::new();
    s.push_str(tool);
    s.push_str(" line does not start with its own prefix");
    s
}

/// 一致率不足。
pub fn msg_rate_short(matched: u32, total: u32) -> String {
    let mut s = String::from("verbatim rate ");
    s.push_str(dec_text(matched).as_str());
    s.push('/');
    s.push_str(dec_text(total).as_str());
    s.push_str(" below ");
    s.push_str(dec_text(RATE_FULL).as_str());
    s
}

/// 用法。
pub fn msg_usage() -> String {
    String::from("usage: enospc-text <tool> <errno>")
}

// ---------------------------------------------------------------------------
// 七工具
// ---------------------------------------------------------------------------

/// 承载 `No space left on device` 的七条上游路径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// `strerror(28)` 本身：裸句，无换行。
    Strerror,
    /// glibc `error()`：`write error: ` 前缀。
    GlibcWrite,
    /// busybox dd：前缀里夹引号路径。
    BusyboxDd,
    /// GNU dd：前缀里夹引号路径，且多一个 `error`。
    GnuDd,
    /// rust coreutils：带 ` (os error 28)` 后缀。
    RustCoreutils,
    /// rsync：带 ` (28)` 后缀。
    Rsync,
    /// tee：前缀里夹引号路径。
    Tee,
}

impl Tool {
    /// 七工具全表（次序即语料承载序）。
    pub fn all() -> [Tool; TOOL_COUNT] {
        [
            Tool::Strerror,
            Tool::GlibcWrite,
            Tool::BusyboxDd,
            Tool::GnuDd,
            Tool::RustCoreutils,
            Tool::Rsync,
            Tool::Tee,
        ]
    }

    /// 工具名。
    pub fn name(&self) -> &'static str {
        match *self {
            Tool::Strerror => "strerror",
            Tool::GlibcWrite => "glibc-write",
            Tool::BusyboxDd => "busybox-dd",
            Tool::GnuDd => "gnu-dd",
            Tool::RustCoreutils => "rust-coreutils",
            Tool::Rsync => "rsync",
            Tool::Tee => "tee",
        }
    }

    /// 工具号（账本存号不存枚举）。
    pub fn id(&self) -> u8 {
        match *self {
            Tool::Strerror => 1,
            Tool::GlibcWrite => 2,
            Tool::BusyboxDd => 3,
            Tool::GnuDd => 4,
            Tool::RustCoreutils => 5,
            Tool::Rsync => 6,
            Tool::Tee => 7,
        }
    }

    /// 由工具号构造；越界给 None。
    pub fn of_id(i: u8) -> Option<Tool> {
        match i {
            1 => Some(Tool::Strerror),
            2 => Some(Tool::GlibcWrite),
            3 => Some(Tool::BusyboxDd),
            4 => Some(Tool::GnuDd),
            5 => Some(Tool::RustCoreutils),
            6 => Some(Tool::Rsync),
            7 => Some(Tool::Tee),
            _ => None,
        }
    }

    /// 体例型。
    pub fn shape(&self) -> TextShape {
        match *self {
            Tool::Strerror => TextShape::Bare,
            Tool::GlibcWrite => TextShape::Prefixed,
            Tool::BusyboxDd | Tool::GnuDd | Tool::Tee => TextShape::Quoted,
            Tool::RustCoreutils | Tool::Rsync => TextShape::Suffixed,
        }
    }

    /// 前缀（不含裸句）。
    pub fn prefix(&self) -> &'static str {
        match *self {
            Tool::Strerror => P_NONE,
            Tool::GlibcWrite => P_GLIBC,
            Tool::BusyboxDd => P_BUSYBOX,
            Tool::GnuDd => P_GNUDD,
            Tool::RustCoreutils => P_RUST,
            Tool::Rsync => P_RSYNC,
            Tool::Tee => P_TEE,
        }
    }

    /// 后缀（不含换行）。
    pub fn suffix(&self) -> &'static str {
        match *self {
            Tool::RustCoreutils => S_RUST,
            Tool::Rsync => S_RSYNC,
            _ => S_NONE,
        }
    }

    /// 整行是否带尾随换行（只有 strerror 不带）。
    pub fn newline(&self) -> bool {
        match *self {
            Tool::Strerror => false,
            _ => true,
        }
    }

    /// 整行原文（含换行位）。
    pub fn text(&self) -> &'static str {
        match *self {
            Tool::Strerror => BARE,
            Tool::GlibcWrite => T_GLIBC,
            Tool::BusyboxDd => T_BUSYBOX,
            Tool::GnuDd => T_GNUDD,
            Tool::RustCoreutils => T_RUST,
            Tool::Rsync => T_RSYNC,
            Tool::Tee => T_TEE,
        }
    }

    /// 该行合成后的字节数。
    pub fn bytes(&self) -> u32 {
        let mut n = self.prefix().len() + BARE.len() + self.suffix().len();
        if self.newline() {
            n += 1;
        }
        n as u32
    }

    /// 裸句在该行里的偏移（即前缀长度）。
    pub fn bare_offset(&self) -> usize {
        self.prefix().len()
    }

    /// 前缀里是否夹着被引号包住的路径。
    pub fn quoted_subject(&self) -> bool {
        let mut quoted = false;
        for b in self.prefix().as_bytes() {
            if *b == b'\'' || *b == b'"' {
                quoted = true;
            }
        }
        quoted
    }

    /// 该行是否真的以自身前缀开头（自洽性）。
    pub fn owns_text(&self) -> bool {
        if self.text().len() == 0 {
            return false;
        }
        self.text().starts_with(self.prefix())
    }

    /// 该行是否恰好由四段合成（自洽性）。
    pub fn composed(&self) -> bool {
        compose(self.prefix(), BARE, self.suffix(), self.newline()).as_str() == self.text()
    }

    /// 工具指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.id());
        h = fold_str(h, self.name());
        h = fold_str(h, self.shape().name());
        h = fold_str(h, self.prefix());
        h = fold_str(h, self.suffix());
        h = fold_bool(h, self.newline());
        h = fold_word(h, self.bytes() as u64);
        h
    }
}

/// 四型体例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextShape {
    /// 裸句：前缀与后缀都空，且不带换行。
    Bare,
    /// 有前缀、无后缀。
    Prefixed,
    /// 有前缀且带 errno 尾巴。
    Suffixed,
    /// 前缀里夹着被引号包住的路径。
    Quoted,
}

impl TextShape {
    /// 四型全表。
    pub fn all() -> [TextShape; SHAPE_COUNT] {
        [
            TextShape::Bare,
            TextShape::Prefixed,
            TextShape::Suffixed,
            TextShape::Quoted,
        ]
    }

    /// 型名。
    pub fn name(&self) -> &'static str {
        match *self {
            TextShape::Bare => "bare",
            TextShape::Prefixed => "prefixed",
            TextShape::Suffixed => "suffixed",
            TextShape::Quoted => "quoted",
        }
    }

    /// 型号。
    pub fn id(&self) -> u8 {
        match *self {
            TextShape::Bare => 1,
            TextShape::Prefixed => 2,
            TextShape::Suffixed => 3,
            TextShape::Quoted => 4,
        }
    }

    /// 该型是否必须带前缀。
    pub fn needs_prefix(&self) -> bool {
        match *self {
            TextShape::Bare => false,
            _ => true,
        }
    }

    /// 该型是否必须带后缀。
    pub fn needs_suffix(&self) -> bool {
        match *self {
            TextShape::Suffixed => true,
            _ => false,
        }
    }

    /// 该型是否必须带引号路径。
    pub fn needs_quotes(&self) -> bool {
        match *self {
            TextShape::Quoted => true,
            _ => false,
        }
    }

    /// 该型是否必须带换行。
    pub fn needs_newline(&self) -> bool {
        match *self {
            TextShape::Bare => false,
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 八闸
// ---------------------------------------------------------------------------

/// 闸门段（秩 1..=8）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 错误号入表。
    Arg,
    /// 整行归属。
    Tool,
    /// 体例归属。
    Shape,
    /// 前缀逐字。
    Prefix,
    /// 裸句逐字。
    Core,
    /// 后缀逐字。
    Suffix,
    /// 整行逐字节。
    Bytes,
    /// 结算。
    Rate,
}

impl Stage {
    /// 八闸全表（顺序即纵线）。
    pub fn all() -> [Stage; 8] {
        [
            Stage::Arg,
            Stage::Tool,
            Stage::Shape,
            Stage::Prefix,
            Stage::Core,
            Stage::Suffix,
            Stage::Bytes,
            Stage::Rate,
        ]
    }

    /// 秩（1 起）。
    pub fn rank(&self) -> u32 {
        match *self {
            Stage::Arg => 1,
            Stage::Tool => 2,
            Stage::Shape => 3,
            Stage::Prefix => 4,
            Stage::Core => 5,
            Stage::Suffix => 6,
            Stage::Bytes => 7,
            Stage::Rate => 8,
        }
    }

    /// 由秩构造；越界给 None。
    pub fn of_rank(r: u32) -> Option<Stage> {
        match r {
            1 => Some(Stage::Arg),
            2 => Some(Stage::Tool),
            3 => Some(Stage::Shape),
            4 => Some(Stage::Prefix),
            5 => Some(Stage::Core),
            6 => Some(Stage::Suffix),
            7 => Some(Stage::Bytes),
            8 => Some(Stage::Rate),
            _ => None,
        }
    }

    /// 段名。
    pub fn name(&self) -> &'static str {
        match *self {
            Stage::Arg => "arg",
            Stage::Tool => "tool",
            Stage::Shape => "shape",
            Stage::Prefix => "prefix",
            Stage::Core => "core",
            Stage::Suffix => "suffix",
            Stage::Bytes => "bytes",
            Stage::Rate => "rate",
        }
    }

    /// 该段挂的码。
    pub fn fault_code(&self) -> u32 {
        match *self {
            Stage::Arg => TX_EARG,
            Stage::Tool => TX_ETOOL,
            Stage::Shape => TX_ESHAPE,
            Stage::Prefix => TX_EPREFIX,
            Stage::Core => TX_ECORE,
            Stage::Suffix => TX_ESUFFIX,
            Stage::Bytes => TX_EBYTES,
            Stage::Rate => TX_EDEFAULT,
        }
    }

    /// 是否在"逐字节"段（结算前的最后一道实质闸）。
    pub fn is_byte_gate(&self) -> bool {
        self.rank() >= Stage::Bytes.rank()
    }
}

// ---------------------------------------------------------------------------
// 注记 / 判决 / 缺陷维
// ---------------------------------------------------------------------------

/// 注记——每一格"为什么"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 无注记（兜底落点）。
    Nil,
    /// 逐字成立。
    Verbatim,
    /// 体例不符。
    ShapeWrong,
    /// 前缀漂移。
    PrefixDrift,
    /// 裸句漂移。
    CoreDrift,
    /// 后缀漂移。
    SuffixDrift,
    /// 字节或换行不符。
    ByteDrift,
    /// 错误号不符。
    ErrnoDrift,
    /// 整行不归属该工具。
    Foreign,
}

impl Note {
    /// 注记名。
    pub fn name(&self) -> &'static str {
        match *self {
            Note::Nil => "nil",
            Note::Verbatim => "verbatim",
            Note::ShapeWrong => "shape-wrong",
            Note::PrefixDrift => "prefix-drift",
            Note::CoreDrift => "core-drift",
            Note::SuffixDrift => "suffix-drift",
            Note::ByteDrift => "byte-drift",
            Note::ErrnoDrift => "errno-drift",
            Note::Foreign => "foreign",
        }
    }

    /// 该注记是否让整条现场作废（不进分母）。
    pub fn voids(&self) -> bool {
        match *self {
            Note::ErrnoDrift | Note::Foreign | Note::ShapeWrong => true,
            _ => false,
        }
    }
}

/// 逐字判决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 裸句型逐字成立。
    Exact,
    /// 承载型逐字成立。
    Carried,
    /// 任一段漂移。
    Mismatch,
}

impl Verdict {
    /// 判决名。
    pub fn name(&self) -> &'static str {
        match *self {
            Verdict::Exact => "exact",
            Verdict::Carried => "carried",
            Verdict::Mismatch => "mismatch",
        }
    }

    /// 是否进分子。
    pub fn counts(&self) -> bool {
        match *self {
            Verdict::Exact | Verdict::Carried => true,
            Verdict::Mismatch => false,
        }
    }

    /// 是否裸句型。
    pub fn is_exact(&self) -> bool {
        match *self {
            Verdict::Exact => true,
            _ => false,
        }
    }
}

/// 缺陷维——申报对判的落点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 成败互报。
    Estate,
    /// 阶段不符。
    Stage,
    /// 错误码不符。
    Code,
    /// 数值不符。
    Value,
    /// 注记不符。
    Note,
    /// 兜底档被当成结论。
    Default,
    /// 无缺陷。
    Ok,
}

impl Fault {
    /// 六维反例表（契约给定次序）。
    pub fn reject_faults() -> [Fault; REJECT] {
        [
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ]
    }

    /// 缺陷维对应的错误码。
    pub fn code(&self) -> u32 {
        match *self {
            Fault::Estate => TX_ESTATE,
            Fault::Stage => TX_ESTAGE,
            Fault::Code => TX_EBYTES,
            Fault::Value => TX_EVALUE,
            Fault::Note => TX_ENOTE,
            Fault::Default => TX_EDEFAULT,
            Fault::Ok => 0,
        }
    }

    /// 缺陷维名。
    pub fn name(&self) -> &'static str {
        match *self {
            Fault::Estate => "estate",
            Fault::Stage => "stage",
            Fault::Code => "code",
            Fault::Value => "value",
            Fault::Note => "note",
            Fault::Default => "default",
            Fault::Ok => "ok",
        }
    }
}

/// 反例六维表（复写一份，和 `Fault::reject_faults()` 交叉校验）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

// ---------------------------------------------------------------------------
// 现场 / 结果 / 申报 / 语料
// ---------------------------------------------------------------------------

/// 一条现场：七个观测位（工具、号、型、三段、整行、换行、字节）加两个开关。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 工具。
    pub tool: Tool,
    /// 观测到的错误号。
    pub errno: i32,
    /// 观测到的体例型。
    pub shape: TextShape,
    /// 观测到的裸句。
    pub bare: &'static str,
    /// 观测到的前缀。
    pub prefix: &'static str,
    /// 观测到的后缀。
    pub suffix: &'static str,
    /// 观测到的整行。
    pub text: &'static str,
    /// 观测到的换行位。
    pub newline: bool,
    /// 观测到的字节数。
    pub bytes: u32,
    /// 结算前没有任何判决（未判档）。
    pub unjudged: bool,
    /// 兜底档被当成结论。
    pub default: bool,
}

impl Shape {
    /// 一条完全合法的现场：整行就是该工具的逐字原文。
    pub fn exact(tool: Tool) -> Shape {
        Shape {
            tool,
            errno: ENOSPC,
            shape: tool.shape(),
            bare: BARE,
            prefix: tool.prefix(),
            suffix: tool.suffix(),
            text: tool.text(),
            newline: tool.newline(),
            bytes: tool.text().len() as u32,
            unjudged: false,
            default: false,
        }
    }

    /// 换号。
    pub fn with_errno(&self, errno: i32) -> Shape {
        Shape { errno, ..*self }
    }

    /// 换体例型。
    pub fn with_shape(&self, shape: TextShape) -> Shape {
        Shape { shape, ..*self }
    }

    /// 换裸句。
    pub fn with_bare(&self, bare: &'static str) -> Shape {
        Shape { bare, ..*self }
    }

    /// 换前缀。
    pub fn with_prefix(&self, prefix: &'static str) -> Shape {
        Shape { prefix, ..*self }
    }

    /// 换后缀。
    pub fn with_suffix(&self, suffix: &'static str) -> Shape {
        Shape { suffix, ..*self }
    }

    /// 换整行。
    pub fn with_text(&self, text: &'static str) -> Shape {
        Shape { text, ..*self }
    }

    /// 换换行位。
    pub fn with_newline(&self, newline: bool) -> Shape {
        Shape { newline, ..*self }
    }

    /// 换字节数。
    pub fn with_bytes(&self, bytes: u32) -> Shape {
        Shape { bytes, ..*self }
    }

    /// 换未判档位。
    pub fn with_unjudged(&self, unjudged: bool) -> Shape {
        Shape { unjudged, ..*self }
    }

    /// 现场指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.tool.seal());
        h = fold_i32(h, self.errno);
        h = fold_byte(h, self.shape.id());
        h = fold_str(h, self.bare);
        h = fold_str(h, self.prefix);
        h = fold_str(h, self.suffix);
        h = fold_str(h, self.text);
        h = fold_bool(h, self.newline);
        h = fold_word(h, self.bytes as u64);
        h = fold_bool(h, self.unjudged);
        h = fold_bool(h, self.default);
        h
    }
}

/// `define` 的输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 最后成立的闸。
    pub stage: Stage,
    /// 错误码（0 = 无）。
    pub code: u32,
    /// 统计值。
    pub value: u32,
    /// 注记。
    pub note: Note,
    /// 兜底档。
    pub default: bool,
    /// 本条是否可作为分子。
    pub ok: bool,
    /// 逐字判决。
    pub verdict: Verdict,
}

impl Outcome {
    /// 一条正例结果。
    fn pass(stage: Stage, note: Note, value: u32, verdict: Verdict) -> Outcome {
        Outcome {
            stage,
            code: 0,
            value,
            note,
            default: false,
            ok: true,
            verdict,
        }
    }

    /// 一条失败结果。
    fn fail(stage: Stage, code: u32, note: Note, value: u32) -> Outcome {
        Outcome {
            stage,
            code,
            value,
            note,
            default: false,
            ok: false,
            verdict: Verdict::Mismatch,
        }
    }

    /// 是否成功。
    pub fn ok(&self) -> bool {
        self.ok
    }

    /// 是否失败。
    pub fn failed(&self) -> bool {
        !self.ok
    }

    /// 是否进入分子。
    pub fn counts(&self) -> bool {
        self.ok && self.verdict.counts()
    }

    /// 是否被闸门侧判死（根本不该进逐字分母）。
    pub fn refused(&self) -> bool {
        self.code == TX_EARG || self.code == TX_ETOOL || self.code == TX_ESHAPE
    }

    /// 结果指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_str(h, self.verdict.name());
        h = fold_bool(h, self.default);
        h
    }
}

/// 申报表：对一条现场声称的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 声称成功。
    pub success: bool,
    /// 声称到达的闸。
    pub stage: Stage,
    /// 声称的错误码。
    pub code: u32,
    /// 声称的统计值。
    pub value: u32,
    /// 声称的注记。
    pub note: Note,
    /// 声称的逐字判决。
    pub verdict: Verdict,
    /// 声称的兜底位。
    pub default: bool,
    /// 反例槽的缺陷标签。
    pub tag: &'static str,
}

impl Claim {
    /// 正例申报。
    pub fn good(stage: Stage, verdict: Verdict, tag: &'static str) -> Claim {
        Claim {
            success: true,
            stage,
            code: 0,
            value: 1,
            note: Note::Verbatim,
            verdict,
            default: false,
            tag,
        }
    }

    /// 反例申报。
    pub fn bad(stage: Stage, code: u32, note: Note, value: u32, tag: &'static str) -> Claim {
        Claim {
            success: false,
            stage,
            code,
            value,
            note,
            verdict: Verdict::Mismatch,
            default: false,
            tag,
        }
    }

    /// 申报是否声称成功。
    pub fn success(&self) -> bool {
        self.success
    }

    /// 申报指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_bool(h, self.success);
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_str(h, self.verdict.name());
        h = fold_bool(h, self.default);
        h = fold_str(h, self.tag);
        h
    }
}

/// 一条语料：现场 + 申报。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    /// 现场。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 真实结果。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 缺陷标签。
    pub fn tag(&self) -> &'static str {
        self.claim.tag
    }

    /// 判定。
    pub fn fault(&self) -> Fault {
        judge(self)
    }

    /// 是否合格。
    pub fn ok(&self) -> bool {
        self.fault() == Fault::Ok
    }

    /// 语料指纹。
    pub fn seal(&self) -> u64 {
        fold_word(self.shape.seal(), self.claim.seal())
    }
}

// ---------------------------------------------------------------------------
// 八闸纵线
// ---------------------------------------------------------------------------

/// 主判据纵线：一条现场如何一步步走到逐字结论。
///
/// 顺序不可调：Arg 在 Tool 之前（号错了一切免谈），Tool 在 Shape 之前（行不归属
/// 这个工具，体例分类就是无源之水），Prefix/Core/Suffix 在 Bytes 之前（先分段量
/// 再整行对；反过来整行一对，分段里藏的错误会被一个"相等"吞掉）。
pub fn define(s: &Shape) -> Outcome {
    // 1 Arg——错误号必须恰好 ENOSPC。
    if s.errno != ENOSPC {
        return Outcome::fail(Stage::Arg, TX_EARG, Note::ErrnoDrift, 0);
    }
    // 2 Tool——整行必须以该工具自己的前缀开头。
    if s.text.len() == 0 || !s.text.starts_with(s.tool.prefix()) {
        return Outcome::fail(Stage::Tool, TX_ETOOL, Note::Foreign, 0);
    }
    // 3 Shape——体例必须与该工具一致，且型自身的义务要满足。
    if s.shape != s.tool.shape() {
        return Outcome::fail(Stage::Shape, TX_ESHAPE, Note::ShapeWrong, 0);
    }
    if s.shape.needs_prefix() && s.tool.prefix().len() == 0 {
        return Outcome::fail(Stage::Shape, TX_ESHAPE, Note::ShapeWrong, 0);
    }
    if s.shape.needs_suffix() && s.tool.suffix().len() == 0 {
        return Outcome::fail(Stage::Shape, TX_ESHAPE, Note::ShapeWrong, 0);
    }
    if s.shape.needs_quotes() && !s.tool.quoted_subject() {
        return Outcome::fail(Stage::Shape, TX_ESHAPE, Note::ShapeWrong, 0);
    }
    // 4 Prefix——前缀逐字。
    if s.prefix != s.tool.prefix() {
        return Outcome::fail(Stage::Prefix, TX_EPREFIX, Note::PrefixDrift, 0);
    }
    // 5 Core——裸句逐字（二十三个字节，不许尾随句号或空格）。
    if s.bare != BARE {
        return Outcome::fail(Stage::Core, TX_ECORE, Note::CoreDrift, 0);
    }
    // 6 Suffix——后缀逐字。
    if s.suffix != s.tool.suffix() {
        return Outcome::fail(Stage::Suffix, TX_ESUFFIX, Note::SuffixDrift, 0);
    }
    // 7 Bytes——换行位、字节数、整行合成三处同时成立。
    if s.newline != s.tool.newline() {
        return Outcome::fail(Stage::Bytes, TX_EBYTES, Note::ByteDrift, 0);
    }
    if s.bytes as usize != s.text.len() {
        return Outcome::fail(Stage::Bytes, TX_EBYTES, Note::ByteDrift, 0);
    }
    let composed = compose(s.prefix, s.bare, s.suffix, s.newline);
    if composed.as_str() != s.text {
        return Outcome::fail(Stage::Bytes, TX_EBYTES, Note::ByteDrift, 0);
    }
    // 没有判决落账的现场：七闸都过了也不许当样本。
    if s.unjudged {
        return Outcome {
            stage: Stage::Rate,
            code: TX_EDEFAULT,
            value: 0,
            note: Note::Nil,
            default: true,
            ok: false,
            verdict: Verdict::Mismatch,
        };
    }
    if s.default {
        return Outcome::fail(Stage::Rate, TX_EDEFAULT, Note::Nil, 0);
    }
    if s.tool.shape() == TextShape::Bare {
        Outcome::pass(Stage::Bytes, Note::Verbatim, 1, Verdict::Exact)
    } else {
        Outcome::pass(Stage::Bytes, Note::Verbatim, 1, Verdict::Carried)
    }
}

/// 现场文案呈现（红项定位用）。
pub fn message(s: &Shape, o: &Outcome) -> String {
    match o.note {
        Note::Verbatim => msg_line(s.tool.name(), s.bytes),
        Note::ErrnoDrift => msg_errno(s.errno),
        Note::Foreign => msg_foreign(s.tool.name()),
        Note::ShapeWrong => msg_shape(
            s.tool.name(),
            s.tool.shape().name(),
            s.shape.name(),
        ),
        Note::PrefixDrift => msg_prefix(s.tool.name(), s.prefix, s.tool.prefix()),
        Note::CoreDrift => msg_core(s.tool.name(), s.bare),
        Note::SuffixDrift => msg_suffix(s.tool.name(), s.suffix, s.tool.suffix()),
        Note::ByteDrift => msg_bytes(s.tool.bytes(), s.bytes),
        Note::Nil => {
            let mut t = String::from("enospc-text: ");
            t.push_str(s.tool.name());
            t.push_str(" stopped at stage ");
            t.push_str(o.stage.name());
            t
        }
    }
}

/// 释义——把一条现场讲成人话。
pub fn detail(c: &Case) -> String {
    let o = c.actual();
    let mut s = String::from(c.tag());
    s.push_str(" tool=");
    s.push_str(c.shape.tool.name());
    s.push_str(" errno=");
    s.push_str(i32_text(c.shape.errno).as_str());
    s.push_str(" shape=");
    s.push_str(c.shape.shape.name());
    s.push_str(" stage=");
    s.push_str(o.stage.name());
    s.push_str(" err=");
    s.push_str(seg_text(o.code).as_str());
    s.push_str(" note=");
    s.push_str(o.note.name());
    s.push_str(" verdict=");
    s.push_str(o.verdict.name());
    s.push_str(" bytes=");
    s.push_str(dec_text(c.shape.bytes).as_str());
    s
}

// ---------------------------------------------------------------------------
// 语料十四槽
// ---------------------------------------------------------------------------

/// 合成一行的字节数（编译期可算，语料里的 `bytes` 全部走它）。
const fn text_len(prefix: &str, suffix: &str, newline: bool) -> u32 {
    let mut n = prefix.len() + BARE.len() + suffix.len();
    if newline {
        n += 1;
    }
    n as u32
}

/// 语料基型：`strerror(28)` 裸句。
const BASE: Shape = Shape {
    tool: Tool::Strerror,
    errno: ENOSPC,
    shape: TextShape::Bare,
    bare: BARE,
    prefix: P_NONE,
    suffix: S_NONE,
    text: BARE,
    newline: false,
    bytes: BARE_LEN,
    unjudged: false,
    default: false,
};

/// glibc `error()` 的合法基型。
const GLIBC_BASE: Shape = Shape {
    tool: Tool::GlibcWrite,
    errno: ENOSPC,
    shape: TextShape::Prefixed,
    bare: BARE,
    prefix: P_GLIBC,
    suffix: S_NONE,
    text: T_GLIBC,
    newline: true,
    bytes: text_len(P_GLIBC, S_NONE, true),
    unjudged: false,
    default: false,
};

/// rust coreutils 的合法基型。
const RUST_BASE: Shape = Shape {
    tool: Tool::RustCoreutils,
    errno: ENOSPC,
    shape: TextShape::Suffixed,
    bare: BARE,
    prefix: P_RUST,
    suffix: S_RUST,
    text: T_RUST,
    newline: true,
    bytes: text_len(P_RUST, S_RUST, true),
    unjudged: false,
    default: false,
};

/// 十四槽现场：前八槽覆盖裸句型与六种承载型（含重复抽样），后六槽各坏一维。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 strerror(28)：裸句，无前缀、无后缀、无换行。
    Shape { ..BASE },
    // 1 glibc error()：前缀型。
    Shape { ..GLIBC_BASE },
    // 2 busybox dd：引号型。
    Shape {
        tool: Tool::BusyboxDd,
        shape: TextShape::Quoted,
        prefix: P_BUSYBOX,
        text: T_BUSYBOX,
        newline: true,
        bytes: text_len(P_BUSYBOX, S_NONE, true),
        ..BASE
    },
    // 3 GNU dd：引号型，前缀更长。
    Shape {
        tool: Tool::GnuDd,
        shape: TextShape::Quoted,
        prefix: P_GNUDD,
        text: T_GNUDD,
        newline: true,
        bytes: text_len(P_GNUDD, S_NONE, true),
        ..BASE
    },
    // 4 rust coreutils：后缀型。
    Shape { ..RUST_BASE },
    // 5 rsync：后缀型。
    Shape {
        tool: Tool::Rsync,
        shape: TextShape::Suffixed,
        prefix: P_RSYNC,
        suffix: S_RSYNC,
        text: T_RSYNC,
        newline: true,
        bytes: text_len(P_RSYNC, S_RSYNC, true),
        ..BASE
    },
    // 6 tee：引号型。
    Shape {
        tool: Tool::Tee,
        shape: TextShape::Quoted,
        prefix: P_TEE,
        text: T_TEE,
        newline: true,
        bytes: text_len(P_TEE, S_NONE, true),
        ..BASE
    },
    // 7 strerror(28) 的第二种形态：同一句再抽一次，逐字相同。
    Shape { ..BASE },
    // 8 Estate：整行少算一个字节，申报却声称逐字成立。
    Shape {
        bytes: text_len(P_GLIBC, S_NONE, true) - 1,
        ..GLIBC_BASE
    },
    // 9 Stage：体例错标成裸句，申报却声称已量到裸句段。
    Shape {
        shape: TextShape::Bare,
        ..GLIBC_BASE
    },
    // 10 Code：前缀少一个空格，申报却把码记成裸句漂移。
    Shape {
        prefix: P_GLIBC_TIGHT,
        ..GLIBC_BASE
    },
    // 11 Value：裸句多一个句号，申报却按进分子计。
    Shape {
        bare: BARE_PERIOD,
        ..GLIBC_BASE
    },
    // 12 Note：后缀多一个句号，申报却注成逐字成立。
    Shape {
        suffix: S_RUST_PERIOD,
        ..RUST_BASE
    },
    // 13 Default：七闸都过却没有任何判决落账（未判档）。
    Shape {
        unjudged: true,
        ..GLIBC_BASE
    },
];

/// 申报基型：与裸句型现场的实际结论逐格相同。
const GOOD_CLAIM: Claim = Claim {
    success: true,
    stage: Stage::Bytes,
    code: 0,
    value: 1,
    note: Note::Verbatim,
    verdict: Verdict::Exact,
    default: false,
    tag: "",
};

/// 承载型的申报基型。
const CARRIED_CLAIM: Claim = Claim {
    verdict: Verdict::Carried,
    ..GOOD_CLAIM
};

/// 十四格申报：前八格与 `define(SHAPES[i])` 逐格相同，后六格各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        tag: "f11694-strerror-bare",
        ..GOOD_CLAIM
    },
    Claim {
        tag: "f11694-glibc-prefixed",
        ..CARRIED_CLAIM
    },
    Claim {
        tag: "f11694-busybox-quoted",
        ..CARRIED_CLAIM
    },
    Claim {
        tag: "f11694-gnudd-quoted",
        ..CARRIED_CLAIM
    },
    Claim {
        tag: "f11694-rust-suffixed",
        ..CARRIED_CLAIM
    },
    Claim {
        tag: "f11694-rsync-suffixed",
        ..CARRIED_CLAIM
    },
    Claim {
        tag: "f11694-tee-quoted",
        ..CARRIED_CLAIM
    },
    Claim {
        tag: "f11694-strerror-repeat",
        ..GOOD_CLAIM
    },
    // 8 成败互报：字节数少一，申报成逐字成立。
    Claim {
        success: true,
        stage: Stage::Bytes,
        code: 0,
        value: 1,
        note: Note::Verbatim,
        verdict: Verdict::Carried,
        default: false,
        tag: "f11694-estate-shortbyte",
    },
    // 9 阶段不符：体例错现场申报成"已到裸句段"。
    Claim {
        success: false,
        stage: Stage::Core,
        code: TX_ESHAPE,
        value: 0,
        note: Note::ShapeWrong,
        verdict: Verdict::Mismatch,
        default: false,
        tag: "f11694-stage-shape",
    },
    // 10 码档不符：前缀漂移申报成裸句漂移。
    Claim {
        success: false,
        stage: Stage::Prefix,
        code: TX_ECORE,
        value: 0,
        note: Note::PrefixDrift,
        verdict: Verdict::Mismatch,
        default: false,
        tag: "f11694-code-prefix",
    },
    // 11 数值不符：裸句漂移却按进分子计。
    Claim {
        success: false,
        stage: Stage::Core,
        code: TX_ECORE,
        value: 1,
        note: Note::CoreDrift,
        verdict: Verdict::Mismatch,
        default: false,
        tag: "f11694-value-core",
    },
    // 12 注记不符：后缀漂移申报成逐字成立。
    Claim {
        success: false,
        stage: Stage::Suffix,
        code: TX_ESUFFIX,
        value: 0,
        note: Note::Verbatim,
        verdict: Verdict::Mismatch,
        default: false,
        tag: "f11694-note-suffix",
    },
    // 13 兜底档：未判现场申报成"没有兜底"。
    Claim {
        success: false,
        stage: Stage::Rate,
        code: TX_EDEFAULT,
        value: 0,
        note: Note::Nil,
        verdict: Verdict::Mismatch,
        default: false,
        tag: "f11694-default-unjudged",
    },
];

// ---------------------------------------------------------------------------
// 期望表（申报表的第二种形式）
// ---------------------------------------------------------------------------

/// 期望阶段（现场**真实**落到的阶段）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Bytes,
    Stage::Bytes,
    Stage::Bytes,
    Stage::Bytes,
    Stage::Bytes,
    Stage::Bytes,
    Stage::Bytes,
    Stage::Bytes,
    Stage::Bytes,
    Stage::Shape,
    Stage::Prefix,
    Stage::Core,
    Stage::Suffix,
    Stage::Rate,
];

/// 期望错误码（0 = 无码）。
pub const WANT_CODE: [u32; CORPUS] = [
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    TX_EBYTES,
    TX_ESHAPE,
    TX_EPREFIX,
    TX_ECORE,
    TX_ESUFFIX,
    TX_EDEFAULT,
];

/// 期望缺陷维序位（0..5 六维，6 = 无缺陷）。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望注记（现场真实注记）。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Verbatim,
    Note::Verbatim,
    Note::Verbatim,
    Note::Verbatim,
    Note::Verbatim,
    Note::Verbatim,
    Note::Verbatim,
    Note::Verbatim,
    Note::ByteDrift,
    Note::ShapeWrong,
    Note::PrefixDrift,
    Note::CoreDrift,
    Note::SuffixDrift,
    Note::Nil,
];

/// 期望逐字判决。
pub const WANT_VERDICT: [Verdict; CORPUS] = [
    Verdict::Exact,
    Verdict::Carried,
    Verdict::Carried,
    Verdict::Carried,
    Verdict::Carried,
    Verdict::Carried,
    Verdict::Carried,
    Verdict::Exact,
    Verdict::Mismatch,
    Verdict::Mismatch,
    Verdict::Mismatch,
    Verdict::Mismatch,
    Verdict::Mismatch,
    Verdict::Mismatch,
];

/// 期望兜底位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望短名（标签必须挂本卡号）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "f11694-strerror-bare",
    "f11694-glibc-prefixed",
    "f11694-busybox-quoted",
    "f11694-gnudd-quoted",
    "f11694-rust-suffixed",
    "f11694-rsync-suffixed",
    "f11694-tee-quoted",
    "f11694-strerror-repeat",
    "f11694-estate-shortbyte",
    "f11694-stage-shape",
    "f11694-code-prefix",
    "f11694-value-core",
    "f11694-note-suffix",
    "f11694-default-unjudged",
];

/// 期望统计值（现场真实统计值）。
pub const WANT_VALUE: [u32; CORPUS] = [1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0];

/// 期望成败（现场真实成败）。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, false, false, false, false, false, false,
];

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 缺陷维 → 序位。
pub fn fault_ndx(f: Fault) -> usize {
    match f {
        Fault::Estate => 0,
        Fault::Stage => 1,
        Fault::Code => 2,
        Fault::Value => 3,
        Fault::Note => 4,
        Fault::Default => 5,
        Fault::Ok => 6,
    }
}

/// 序位 → 缺陷维。
pub fn ndx_fault(n: usize) -> Option<Fault> {
    match n {
        0 => Some(Fault::Estate),
        1 => Some(Fault::Stage),
        2 => Some(Fault::Code),
        3 => Some(Fault::Value),
        4 => Some(Fault::Note),
        5 => Some(Fault::Default),
        6 => Some(Fault::Ok),
        _ => None,
    }
}

/// 申报对判——固定次序 Estate → Stage → Code → Value → Note → Default → Ok。
///
/// 成败互报优先：把"字错了"申报成"字对了"，无论后面几维对不对，都是最严重的
/// 那类谎——逐字判据的全部价值就是"这串字节一个不差"。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    let k = &c.claim;
    if a.ok() != k.success() {
        return Fault::Estate;
    }
    if a.stage != k.stage {
        return Fault::Stage;
    }
    if a.code != k.code {
        return Fault::Code;
    }
    if a.value != k.value {
        return Fault::Value;
    }
    if a.note != k.note {
        return Fault::Note;
    }
    if a.verdict != k.verdict {
        return Fault::Note;
    }
    // 兜底档双向判死：真落了未判档不许当结论；没落却申报"落了"同样不许。
    if a.default || a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 语料取槽。
pub fn case_at(i: usize) -> Option<Case> {
    match (SHAPES.get(i), CLAIMS.get(i)) {
        (Some(s), Some(c)) => Some(Case {
            shape: *s,
            claim: *c,
        }),
        _ => None,
    }
}

/// 全语料。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case {
        shape: BASE,
        claim: GOOD_CLAIM,
    }; CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if let Some(slot) = out.get_mut(i) {
                *slot = c;
            }
        }
        i += 1;
    }
    out
}

/// 覆盖校验：槽号必须落在语料范围内。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(TX_ESLOT);
    }
    Ok(())
}

/// 第 `i` 槽期望的缺陷维。
pub fn expected(i: usize) -> Option<Fault> {
    match WANT_NDX.get(i) {
        Some(n) => ndx_fault(*n),
        None => None,
    }
}

/// 第 `i` 槽（反例区）期望的缺陷维；越界按无缺陷（由调用方先做范围校验）。
fn expected_reject_ndx(i: usize) -> Fault {
    match expected(i) {
        Some(f) => f,
        None => Fault::Ok,
    }
}

// ---------------------------------------------------------------------------
// 档位与矩阵
// ---------------------------------------------------------------------------

/// 逐字四档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 裸句型逐字成立。
    Exact,
    /// 承载型逐字成立。
    Carried,
    /// 闸门侧判死，现场作废（不进分母）。
    Void,
    /// 现场漂移或未判档。
    Unrun,
}

impl Tier {
    /// 四档全表。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Exact, Tier::Carried, Tier::Void, Tier::Unrun]
    }

    /// 档名。
    pub fn name(&self) -> &'static str {
        match *self {
            Tier::Exact => "exact",
            Tier::Carried => "carried",
            Tier::Void => "void",
            Tier::Unrun => "unrun",
        }
    }

    /// 该档是否是绿档（只有前两档算绿）。
    pub fn is_green(&self) -> bool {
        match *self {
            Tier::Exact | Tier::Carried => true,
            _ => false,
        }
    }

    /// 由一次结果归档。
    pub fn of(o: &Outcome) -> Tier {
        if o.refused() {
            return Tier::Void;
        }
        if o.default || o.code == TX_EDEFAULT {
            return Tier::Unrun;
        }
        if o.ok {
            if o.verdict == Verdict::Exact {
                return Tier::Exact;
            }
            return Tier::Carried;
        }
        Tier::Unrun
    }
}

/// 一档的统计行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    /// 档位。
    pub tier: Tier,
    /// 该档扫过的槽数。
    pub total: usize,
    /// 该档判对的槽数。
    pub good: usize,
}

impl Row {
    /// 该行是否全对。
    pub fn perfect(&self) -> bool {
        self.total == self.good
    }

    /// 该行红格数。
    pub fn red(&self) -> usize {
        if self.total > self.good {
            self.total - self.good
        } else {
            0
        }
    }

    /// 呈现一行。
    pub fn render(&self) -> String {
        let mut s = String::from(self.tier.name());
        s.push('=');
        s.push_str(dec_text(self.good as u32).as_str());
        s.push('/');
        s.push_str(dec_text(self.total as u32).as_str());
        s
    }
}

/// 四档矩阵：横看语料槽，纵看档位。
///
/// 判对的条件是"落档与应落档一致"：正例要落进它自己那格绿档，反例**绝不许**
/// 落进绿档——一条漂移现场如果被归档成 `Exact`，逐字判据就把它当成原件交付了。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 由语料构造。
    pub fn build() -> Matrix {
        let table = Tier::all();
        let mut rows = [Row {
            tier: Tier::Exact,
            total: 0,
            good: 0,
        }; TIER_COUNT];
        let mut t = 0usize;
        while t < TIER_COUNT {
            let tier = match table.get(t) {
                Some(x) => *x,
                None => return Matrix { rows },
            };
            let mut total = 0usize;
            let mut good = 0usize;
            let mut i = 0usize;
            while i < CORPUS {
                let c = match case_at(i) {
                    Some(x) => x,
                    None => return Matrix { rows },
                };
                total += 1;
                let landed = Tier::of(&c.actual());
                let hit = if i < ACCEPT {
                    landed == tier && c.ok()
                } else {
                    landed == tier && !tier.is_green() && c.fault() == expected_reject_ndx(i)
                };
                if hit {
                    good += 1;
                }
                i += 1;
            }
            if let Some(slot) = rows.get_mut(t) {
                *slot = Row { tier, total, good };
            }
            t += 1;
        }
        Matrix { rows }
    }

    /// 取一档。
    pub fn row(&self, t: Tier) -> Option<Row> {
        let table = Tier::all();
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(x) = table.get(i) {
                if *x == t {
                    if let Some(r) = self.rows.get(i) {
                        return Some(*r);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 扫过的格总数。
    pub fn slots(&self) -> usize {
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

    /// 判对的格总数。
    pub fn goods(&self) -> usize {
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

    /// 全对的行数。
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

    /// 红格总数。
    pub fn red_total(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.red();
            }
            i += 1;
        }
        n
    }

    /// 四档是否都有落点（空档说明归档逻辑漏了形态）。
    pub fn covered(&self) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.good == 0 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 呈现矩阵。
    pub fn render(&self) -> String {
        let mut s = String::from("matrix ");
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if i > 0 {
                    s.push(' ');
                }
                s.push_str(r.render().as_str());
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 结算报告
// ---------------------------------------------------------------------------

/// 全绿渲染文本（逐字钉住，改一个字都要红）。
pub const RENDER_OK: &str =
    "enospc-text tools=7 shapes=4 bare=23 errno=28 rate=1000permille\n";

/// 逐字结算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 结算分母：工具行数。
    pub total: usize,
    /// 正例槽数。
    pub accept: usize,
    /// 反例槽数。
    pub reject: usize,
    /// 裸句型计数。
    pub exact: usize,
    /// 承载型计数。
    pub carried: usize,
    /// 作废档计数。
    pub void: usize,
    /// 未判档计数。
    pub unrun: usize,
    /// 计入逐字的行数。
    pub matched: usize,
    /// 违约行数。
    pub conflicts: usize,
}

impl Report {
    /// 逐字一致率千分比。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched as u32 * RATE_FULL + (self.total as u32) / 2) / (self.total as u32)
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.conflicts == 0
            && self.rate() == RATE_FULL
            && self.accept == ACCEPT
            && self.reject == REJECT
            && self.exact == BARE_SAMPLES
            && self.carried == CARRIED_SAMPLES
            && self.void == VOID_SAMPLES
            && self.unrun == UNRUN_SAMPLES
            && self.exact + self.carried == self.accept
    }

    /// 红项计数。
    pub fn reds(&self) -> u32 {
        (self.conflicts as u32) + (self.unrun as u32).saturating_sub(UNRUN_SAMPLES as u32)
    }

    /// 闸门：全绿才放行，否则给逐字码。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(TX_EBYTES);
        }
        if self.conflicts > 0 {
            return Err(TX_ENOTE);
        }
        if self.rate() < RATE_FULL {
            return Err(TX_EVALUE);
        }
        if !self.green() {
            return Err(TX_EDEFAULT);
        }
        Ok(())
    }

    /// 三档格数。
    pub fn tier_cells(&self) -> usize {
        self.exact + self.carried + self.void + self.unrun
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("enospc-text tools=");
        s.push_str(dec_text(TOOL_COUNT as u32).as_str());
        s.push_str(" shapes=");
        s.push_str(dec_text(SHAPE_COUNT as u32).as_str());
        s.push_str(" bare=");
        s.push_str(dec_text(BARE_LEN).as_str());
        s.push_str(" errno=");
        s.push_str(i32_text(ENOSPC).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("permille\n");
        s
    }
}

/// 空报告（未跑任何工具时的形态，绝不与全绿混淆）。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        exact: 0,
        carried: 0,
        void: 0,
        unrun: 0,
        matched: 0,
        conflicts: 0,
    }
}

/// 由工具面与语料面两路结算。
///
/// 分子由七条工具行逐字成立给出，语料面给违约项：反例按申报那一维落红不算违约，
/// 正例落红、反例落错维、注入错报才算。
fn settle(over: Option<(usize, Fault)>) -> Report {
    let mut rep = Report {
        total: TOOL_COUNT,
        accept: ACCEPT,
        reject: REJECT,
        exact: 0,
        carried: 0,
        void: 0,
        unrun: 0,
        matched: 0,
        conflicts: 0,
    };
    let mut verified = 0usize;
    let tools = Tool::all();
    let mut t = 0usize;
    while t < TOOL_COUNT {
        let tool = match tools.get(t) {
            Some(x) => *x,
            None => return empty_report(),
        };
        let o = define(&Shape::exact(tool));
        if o.ok() {
            verified += 1;
        }
        t += 1;
    }
    let mut violations = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return empty_report(),
        };
        match Tier::of(&c.actual()) {
            Tier::Exact => rep.exact += 1,
            Tier::Carried => rep.carried += 1,
            Tier::Void => rep.void += 1,
            Tier::Unrun => rep.unrun += 1,
        }
        let landed = match over {
            Some((slot, fault)) => {
                if slot == i {
                    fault
                } else {
                    c.fault()
                }
            }
            None => c.fault(),
        };
        if i < ACCEPT {
            if landed != Fault::Ok {
                violations += 1;
            }
        } else if landed != expected_reject_ndx(i) {
            violations += 1;
        }
        i += 1;
    }
    rep.conflicts = violations;
    rep.matched = verified.saturating_sub(violations);
    rep
}

/// 主结算：全绿才给报告，否则给闸门码。
pub fn run() -> Result<Report, u32> {
    let rep = settle(None);
    match rep.gate() {
        Ok(()) => Ok(rep),
        Err(code) => Err(code),
    }
}

/// 带注入的结算：越界槽给槽位码，否则返回注入后的报告。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(TX_ESLOT);
        }
    }
    Ok(settle(over))
}

/// 报告与矩阵是否互相印证（两条独立路径必须给同一批档数）。
pub fn report_agrees_with_matrix() -> bool {
    let rep = settle(None);
    let m = Matrix::build();
    rep.exact == BARE_SAMPLES
        && rep.carried == CARRIED_SAMPLES
        && rep.exact + rep.carried == ACCEPT
        && m.goods() == CORPUS
        && m.row(Tier::Exact).map(|r| r.good) == Some(BARE_SAMPLES)
        && m.row(Tier::Carried).map(|r| r.good) == Some(CARRIED_SAMPLES)
}

/// 一致率在注入一路红之后必须掉下来（929 档的口径标定）。
pub fn red_rate_calibrated() -> bool {
    let base = settle(None);
    let bad = match run_with(Some((1, Fault::Value))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if bad.matched >= base.matched {
        return false;
    }
    if bad.rate() >= base.rate() {
        return false;
    }
    RED_RATE == 929 && RATE_FULL == 1000
}

/// 注入一路红，四路自检必须各指各的。
pub fn inject_four() -> [bool; 4] {
    let tools_route = {
        let tools = Tool::all();
        let mut all = true;
        let mut t = 0usize;
        while t < TOOL_COUNT {
            let tool = match tools.get(t) {
                Some(x) => *x,
                None => return [false, false, false, false],
            };
            if !tool.owns_text() || !tool.composed() {
                all = false;
            }
            if tool.text().len() as u32 != tool.bytes() {
                all = false;
            }
            let o = define(&Shape::exact(tool));
            if !o.ok() {
                all = false;
            }
            t += 1;
        }
        all
    };
    let judge_route = {
        let mut all_ok = true;
        let mut i = 0usize;
        while i < ACCEPT {
            if let Some(c) = case_at(i) {
                if c.fault() != Fault::Ok {
                    all_ok = false;
                }
            }
            i += 1;
        }
        let mut k = ACCEPT;
        while k < CORPUS {
            if let Some(c) = case_at(k) {
                if c.fault() == Fault::Ok {
                    all_ok = false;
                }
                if c.fault() != expected_reject_ndx(k) {
                    all_ok = false;
                }
            }
            k += 1;
        }
        all_ok
    };
    let render_route = {
        let rep = settle(None);
        rep.green() && rep.render().as_str() == RENDER_OK
    };
    let ledger_route = {
        let mut l = build_ledger();
        match l.freeze() {
            Ok(h) => h != 0 && l.is_frozen() && l.replay() == Ok(h),
            Err(_) => false,
        }
    };
    [tools_route, judge_route, render_route, ledger_route]
}

// ---------------------------------------------------------------------------
// 阶段闸门：Arg→Tool→Shape→Prefix→Core→Suffix→Bytes→Rate
// ---------------------------------------------------------------------------

/// 一次结算的走闸状态机。
///
/// 这条纵线为什么不能倒：号归位必须先于行归位，行归位必须先于体例，体例必须先于
/// 分段，分段必须先于整行。一旦允许倒退，就能出现"先对整行再补分段"——分段里
/// 藏的错误被整行相等吞掉，逐字判据当场失效。倒退判死（`TX_ESTAGE`），关闭后
/// 再走一步也判死（`TX_ESTATE`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    /// 当前档；`None` 表示还没进过任何闸。
    current: Option<Stage>,
    /// 是否已封账。
    closed: bool,
    /// 成功走档次数。
    steps: u32,
    /// 被拒次数。
    refusals: u32,
    /// 走档轨迹指纹。
    root: u64,
}

impl Gate {
    /// 空闸门。
    pub fn new() -> Gate {
        Gate {
            current: None,
            closed: false,
            steps: 0,
            refusals: 0,
            root: FNV_OFF,
        }
    }

    /// 走一档。倒退即拒，且不改状态。
    pub fn step(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.refusals += 1;
            return Err(TX_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(TX_ESTAGE);
            }
        }
        self.root = fold_word(self.root, s.rank() as u64);
        self.root = fold_str(self.root, s.name());
        self.current = Some(s);
        self.steps += 1;
        Ok(())
    }

    /// 封账：之后任何走档与再次封账都拒。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.closed {
            self.refusals += 1;
            return Err(TX_ESTATE);
        }
        if self.current.is_none() {
            return Err(TX_ESTAGE);
        }
        self.closed = true;
        self.root = fold_str(self.root, "closed");
        Ok(self.root)
    }

    /// 当前档。
    pub fn stage(&self) -> Option<Stage> {
        self.current
    }

    /// 成功走档次数。
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// 被拒次数。
    pub fn refusals(&self) -> u32 {
        self.refusals
    }

    /// 是否已封账。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 轨迹指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 依次走一串档。
    pub fn walk(stages: &[Stage]) -> Result<Gate, u32> {
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < stages.len() {
            let s = match stages.get(i) {
                Some(x) => *x,
                None => return Err(TX_ESLOT),
            };
            match g.step(s) {
                Ok(()) => {}
                Err(code) => return Err(code),
            }
            i += 1;
        }
        Ok(g)
    }

    /// 全档正序走一遍并封账。
    pub fn full_pass() -> Result<u64, u32> {
        let table = Stage::all();
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < table.len() {
            let s = match table.get(i) {
                Some(x) => *x,
                None => return Err(TX_ESLOT),
            };
            match g.step(s) {
                Ok(()) => {}
                Err(code) => return Err(code),
            }
            i += 1;
        }
        g.close()
    }

    /// 呈现闸门。
    pub fn render(&self) -> String {
        let mut s = String::from("gate stage=");
        match self.current {
            Some(cur) => s.push_str(cur.name()),
            None => s.push_str("none"),
        }
        s.push_str(" steps=");
        s.push_str(dec_text(self.steps).as_str());
        s.push_str(" refusals=");
        s.push_str(dec_text(self.refusals).as_str());
        s.push_str(" closed=");
        s.push_str(bool_text(self.closed));
        s
    }
}

// ---------------------------------------------------------------------------
// 账本：逐格登记本卡的统计量与缺陷码
// ---------------------------------------------------------------------------

/// 一格台账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 槽号，必须严格等于登记时的当前长度。
    pub slot: u8,
    /// 这一格对应的算子号。
    pub op: u8,
    /// 这一格记的是成功还是败。
    pub ok: bool,
    /// 错误码（成功格必须为 0）。
    pub code: u32,
    /// 统计值（败格必须为 0）。
    pub value: u32,
}

impl Sample {
    /// 一格的指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_byte(h, self.slot);
        h = fold_byte(h, self.op);
        h = fold_bool(h, self.ok);
        h = fold_word(h, self.code as u64);
        h = fold_word(h, self.value as u64);
        h
    }

    /// 呈现一格。
    pub fn render(&self) -> String {
        let mut s = String::from("slot ");
        s.push_str(dec_text(self.slot as u32).as_str());
        s.push_str(" op ");
        s.push_str(hex_text(self.op as u32, 2).as_str());
        s.push(' ');
        s.push_str(bool_text(self.ok));
        s.push_str(" code ");
        s.push_str(hex_text(self.code, 5).as_str());
        s.push_str(" value ");
        s.push_str(dec_text(self.value).as_str());
        s
    }
}

/// 账本：十四格 = 七统计格 + 一红档格 + 六缺陷码格。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    slots: [Option<Sample>; SAMPLES],
    len: usize,
    rejects: usize,
    frozen: bool,
    root: u64,
}

impl Ledger {
    /// 空账。
    pub fn new() -> Ledger {
        Ledger {
            slots: [None; SAMPLES],
            len: 0,
            rejects: 0,
            frozen: false,
            root: 0,
        }
    }

    /// 登记一格；槽号错位一律拒。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TX_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(TX_ESLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(TX_ESLOT);
        }
        if let Some(cell) = self.slots.get_mut(self.len) {
            *cell = Some(s);
        }
        self.len += 1;
        Ok(())
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        match self.slots.get(i) {
            Some(x) => *x,
            None => None,
        }
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 拒收次数。
    pub fn rejects(&self) -> usize {
        self.rejects
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 冻结后的指纹；未冻结时为 0。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 逐格复核。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(TX_ESLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(TX_ESLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(TX_ESLOT),
            };
            if !s.ok {
                return Err(TX_ESTATE);
            }
            if s.code != 0 {
                return Err(TX_EBYTES);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(TX_ESLOT),
            };
            if s.value != want_value {
                return Err(TX_EVALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(TX_ESLOT),
            };
            if s.op != want_op {
                return Err(TX_EBYTES);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(TX_ESLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(TX_ESLOT),
            };
            if s.ok {
                return Err(TX_ESTATE);
            }
            if s.value != 0 {
                return Err(TX_EVALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(TX_ESLOT),
            };
            if s.code != want_code {
                return Err(TX_EBYTES);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(TX_ESLOT),
            };
            if s.op != want_op {
                return Err(TX_EBYTES);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                let other = match REJECT_CODES.get(k) {
                    Some(c) => *c,
                    None => return Err(TX_ESLOT),
                };
                if other == want_code {
                    return Err(TX_EBYTES);
                }
                k += 1;
            }
            j += 1;
        }
        Ok(())
    }

    /// 第一处不合规的格子与原因。
    pub fn first_mismatch(&self) -> Option<(usize, &'static str)> {
        if self.len != SAMPLES {
            return Some((self.len, "len"));
        }
        let mut i = 0usize;
        while i < SAMPLES {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Some((i, "hole")),
            };
            if i < STAT_CELLS + 1 {
                let want_value = match VALS.get(i) {
                    Some(v) => *v,
                    None => return Some((i, "slot")),
                };
                if !s.ok {
                    return Some((i, "ok-flag"));
                }
                if s.code != 0 {
                    return Some((i, "stat-code"));
                }
                if s.value != want_value {
                    return Some((i, "stat-value"));
                }
            } else {
                let j = i - (STAT_CELLS + 1);
                let want_code = match REJECT_CODES.get(j) {
                    Some(c) => *c,
                    None => return Some((i, "slot")),
                };
                if s.ok {
                    return Some((i, "ok-flag"));
                }
                if s.value != 0 {
                    return Some((i, "defect-value"));
                }
                if s.code != want_code {
                    return Some((i, "defect-code"));
                }
            }
            i += 1;
        }
        None
    }

    /// 统计格应有的算子号。
    fn stat_op_want(&self, i: usize) -> Option<u8> {
        if i < STAT_CELLS {
            return match STAT_OPS.get(i) {
                Some(v) => Some(*v),
                None => None,
            };
        }
        if i == STAT_CELLS {
            return Some(OPID_STAT_RED);
        }
        None
    }

    /// 折叠全账得指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.len as u64);
        h = fold_word(h, self.rejects as u64);
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                h = fold_word(h, s.seal());
            }
            i += 1;
        }
        h
    }

    /// 冻结：先复核再折叠，复核不过一律拒。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(TX_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(TX_EVALUE);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(TX_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(TX_EVALUE),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(TX_EVALUE),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(TX_EVALUE);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许）。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TX_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(TX_ESLOT),
                }
                Ok(())
            }
            None => Err(TX_ESLOT),
        }
    }

    /// 呈现全账。
    pub fn render(&self) -> String {
        let mut s = String::from("ledger len=");
        s.push_str(dec_text(self.len as u32).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok_count() as u32).as_str());
        s.push_str(" err=");
        s.push_str(dec_text(self.err_count() as u32).as_str());
        s.push_str(" rejects=");
        s.push_str(dec_text(self.rejects as u32).as_str());
        s.push_str(" frozen=");
        s.push_str(bool_text(self.frozen));
        s
    }
}

/// 十四格标准台账：七统计 + 一红档 + 六缺陷码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut i = 0usize;
    while i < STAT_CELLS {
        let op = match STAT_OPS.get(i) {
            Some(x) => *x,
            None => return l,
        };
        let value = match VALS.get(i) {
            Some(x) => *x,
            None => return l,
        };
        let s = Sample {
            slot: i as u8,
            op,
            ok: true,
            code: 0,
            value,
        };
        match l.register(s) {
            Ok(()) => {}
            Err(_) => return l,
        }
        i += 1;
    }
    let red = Sample {
        slot: STAT_CELLS as u8,
        op: OPID_STAT_RED,
        ok: true,
        code: 0,
        value: RED_RATE,
    };
    match l.register(red) {
        Ok(()) => {}
        Err(_) => return l,
    }
    let mut j = 0usize;
    while j < REJECT {
        let code = match REJECT_CODES.get(j) {
            Some(x) => *x,
            None => return l,
        };
        let op = match DEFECT_OP_BASE.checked_add(j as u8) {
            Some(x) => x,
            None => return l,
        };
        let slot = (STAT_CELLS + 1 + j) as u8;
        let s = Sample {
            slot,
            op,
            ok: false,
            code,
            value: 0,
        };
        match l.register(s) {
            Ok(()) => {}
            Err(_) => return l,
        }
        j += 1;
    }
    l
}

/// 注入一维错的台账。
pub fn build_ledger_broken(which: u8) -> Ledger {
    let mut l = build_ledger();
    let (cell, value) = match which {
        0 => match (STAT_CELLS, VALS.get(STAT_CELLS)) {
            (c, Some(v)) => (c, v + 1),
            _ => return l,
        },
        1 => match (0usize, VALS.get(0)) {
            (c, Some(v)) => (c, v + 7),
            _ => return l,
        },
        2 => match STAT_CELLS.checked_add(1) {
            Some(c) => (c, 1u32),
            None => return l,
        },
        _ => return l,
    };
    match l.poke(cell, value) {
        Ok(()) => {}
        Err(_) => {}
    }
    l
}

/// 申报表第一格的标签。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "f11694-strerror-bare",
    }
}

/// 统计表第一格的值。
pub fn head_val() -> u32 {
    match VALS.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// 十三条判据谓词（全部私有，只在 run_unx11694_checks 里汇总）
// ---------------------------------------------------------------------------

/// 子串出现次数（逐字判据要证明裸句只出现一次）。
fn count_sub(hay: &str, needle: &str) -> usize {
    if needle.len() == 0 || needle.len() > hay.len() {
        return 0;
    }
    let mut n = 0usize;
    let bytes = hay.as_bytes();
    let pat = needle.as_bytes();
    let mut i = 0usize;
    while i + pat.len() <= bytes.len() {
        let mut hit = true;
        let mut k = 0usize;
        while k < pat.len() {
            match (bytes.get(i + k), pat.get(k)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        hit = false;
                    }
                }
                _ => hit = false,
            }
            k += 1;
        }
        if hit {
            n += 1;
            i += pat.len();
        } else {
            i += 1;
        }
    }
    n
}

/// 01 名称与段基址：常量层自洽。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d3d0 {
        return false;
    }
    if ANCHOR_F11695_SEG != SEG_BASE + 0x10 {
        return false;
    }
    if ANCHOR_F11696_SEG != SEG_BASE + 0x20 {
        return false;
    }
    if ANCHOR_F11693_SEG != SEG_BASE - 0x10 {
        return false;
    }
    if ANCHOR_F11695_GAP != 0x10 || ANCHOR_F11696_GAP != 0x20 {
        return false;
    }
    if CORPUS != 14 || SAMPLES != CORPUS {
        return false;
    }
    if ACCEPT != 8 || REJECT != 6 || ACCEPT + REJECT != CORPUS {
        return false;
    }
    if TIER_COUNT != 4 || STAGE_COUNT != 8 || STAGE_LAST_RANK != 8 {
        return false;
    }
    if STAT_CELLS != 7 || STAT_OPS.len() != STAT_CELLS {
        return false;
    }
    if TOOL_COUNT != 7 || SHAPE_COUNT != 4 {
        return false;
    }
    if BARE_LEN != 23 || ENOSPC != 28 || EDQUOT != 122 {
        return false;
    }
    if BARE_SAMPLES + CARRIED_SAMPLES != ACCEPT {
        return false;
    }
    if VOID_SAMPLES + UNRUN_SAMPLES != REJECT {
        return false;
    }
    if SUFFIX_TOOLS != 2 || QUOTED_TOOLS != 3 || NEWLINE_TOOLS != TOOL_COUNT - 1 {
        return false;
    }
    if VALS.len() != ACCEPT {
        return false;
    }
    if VALS.get(0) != Some(&(TOOL_COUNT as u32)) {
        return false;
    }
    if VALS.get(1) != Some(&(SHAPE_COUNT as u32)) {
        return false;
    }
    if VALS.get(4) != Some(&BARE_LEN) {
        return false;
    }
    if VALS.get(5) != Some(&(ENOSPC as u32)) {
        return false;
    }
    if VALS.get(6) != Some(&RATE_FULL) || VALS.get(7) != Some(&RED_RATE) {
        return false;
    }
    if RED_RATE >= RATE_FULL {
        return false;
    }
    if head_tag() != "f11694-strerror-bare" {
        return false;
    }
    if head_val() != TOOL_COUNT as u32 {
        return false;
    }
    true
}

/// 02 十三码互异：全在段内、两两不同、六维反例码与缺陷维对齐。
fn codes_ok() -> bool {
    if ALL_CODES.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let code = match ALL_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if code <= SEG_BASE || code > SEG_BASE + 0x0d {
            return false;
        }
        let mut j = i + 1usize;
        while j < ALL_CODES.len() {
            if ALL_CODES.get(j) == Some(&code) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let faults = Fault::reject_faults();
    if faults.len() != REJECT || REJECT_CODES.len() != REJECT {
        return false;
    }
    let mut k = 0usize;
    while k < REJECT {
        let f = match faults.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if f == Fault::Ok {
            return false;
        }
        if REJECT_CODES.get(k) != Some(&f.code()) {
            return false;
        }
        if REJECT_FAULTS.get(k) != Some(&f) {
            return false;
        }
        k += 1;
    }
    let mut m = 0usize;
    while m < STAT_OPS.len() {
        let op = match STAT_OPS.get(m) {
            Some(x) => *x,
            None => return false,
        };
        if op == 0 {
            return false;
        }
        let mut n = m + 1usize;
        while n < STAT_OPS.len() {
            if STAT_OPS.get(n) == Some(&op) {
                return false;
            }
            n += 1;
        }
        m += 1;
    }
    if OPID_STAT_RED != 0x08 || DEFECT_OP_BASE != 0x21 {
        return false;
    }
    if STAT_OPS.get(STAT_CELLS - 1) == Some(&OPID_STAT_RED) {
        return false;
    }
    true
}

/// 03 七工具全表：归属、合成、字节数、指纹全部自洽。
fn tools_ok() -> bool {
    let tools = Tool::all();
    if tools.len() != TOOL_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < TOOL_COUNT {
        let tool = match tools.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if tool.id() as usize != i + 1 {
            return false;
        }
        if Tool::of_id(tool.id()) != Some(tool) {
            return false;
        }
        if Tool::of_id(0).is_some() || Tool::of_id(8).is_some() {
            return false;
        }
        if tool.name().len() == 0 {
            return false;
        }
        if !tool.owns_text() {
            return false;
        }
        if !tool.composed() {
            return false;
        }
        if tool.text().len() as u32 != tool.bytes() {
            return false;
        }
        if count_sub(tool.text(), BARE) != 1 {
            return false;
        }
        if tool.bare_offset() != tool.prefix().len() {
            return false;
        }
        let o = define(&Shape::exact(tool));
        if !o.ok() || o.stage != Stage::Bytes {
            return false;
        }
        if o.note != Note::Verbatim || o.value != 1 {
            return false;
        }
        let want = if tool.shape() == TextShape::Bare {
            Verdict::Exact
        } else {
            Verdict::Carried
        };
        if o.verdict != want {
            return false;
        }
        let mut j = i + 1usize;
        while j < TOOL_COUNT {
            if let Some(other) = tools.get(j) {
                if other.seal() == tool.seal() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    if Tool::Strerror.prefix().len() != 0 {
        return false;
    }
    let mut nonempty = 0usize;
    let mut t = 0usize;
    while t < TOOL_COUNT {
        if let Some(tool) = tools.get(t) {
            if tool.prefix().len() > 0 {
                nonempty += 1;
            }
        }
        t += 1;
    }
    if nonempty != TOOL_COUNT - 1 {
        return false;
    }
    true
}

/// 04 裸句二十三字节：长度、字符集、标点、错误号两路互校。
fn bare_ok() -> bool {
    if BARE.len() != BARE_LEN as usize {
        return false;
    }
    if BARE.len() != 23 {
        return false;
    }
    if BARE.as_bytes().len() != 23 {
        return false;
    }
    let first = match BARE.as_bytes().first() {
        Some(x) => *x,
        None => return false,
    };
    let last = match BARE.as_bytes().last() {
        Some(x) => *x,
        None => return false,
    };
    if first != b'N' || last != b'e' {
        return false;
    }
    // 全是可打印 ASCII，且没有尾随标点。
    for b in BARE.as_bytes() {
        if *b < 0x20 || *b > 0x7e {
            return false;
        }
    }
    if BARE.ends_with(' ') || BARE.ends_with('.') {
        return false;
    }
    if BARE.contains("  ") {
        return false;
    }
    if BARE_PERIOD.len() != BARE.len() + 1 {
        return false;
    }
    if BARE_TRAIL.len() != BARE.len() + 1 {
        return false;
    }
    if BARE_PERIOD == BARE || BARE_TRAIL == BARE {
        return false;
    }
    if BARE_EDQUOT.len() == BARE.len() {
        return false;
    }
    if BARE_EDQUOT == BARE {
        return false;
    }
    true
}

/// 05 strerror 逐字：28 与 122 各给各的，表外号绝不夹带默认文案。
fn strerror_ok() -> bool {
    if strerror(ENOSPC) != Some(BARE) {
        return false;
    }
    if strerror(EDQUOT) != Some(BARE_EDQUOT) {
        return false;
    }
    if strerror(0).is_some() || strerror(2).is_some() || strerror(13).is_some() {
        return false;
    }
    if strerror(-1).is_some() {
        return false;
    }
    let a = match strerror(28) {
        Some(x) => x,
        None => return false,
    };
    let b = match strerror(122) {
        Some(x) => x,
        None => return false,
    };
    if a == b {
        return false;
    }
    if a.len() != 23 {
        return false;
    }
    if b.as_bytes().len() != a.len() - 4 {
        // "Disk quota exceeded" 比 "No space left on device" 短四个字节。
        return false;
    }
    if count_sub(a, b) != 0 || count_sub(b, a) != 0 {
        return false;
    }
    // 裸句本身不带换行、不带前缀后缀。
    if a.ends_with('\n') {
        return false;
    }
    let split = match split_line(a, BARE) {
        Some(x) => x,
        None => return false,
    };
    if split != (0usize, 23usize, 0usize, false) {
        return false;
    }
    true
}

/// 06 四型体例互斥：每型的义务都成立，型与工具一一对得上。
fn shapes_ok() -> bool {
    let table = TextShape::all();
    if table.len() != SHAPE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < SHAPE_COUNT {
        let sh = match table.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if sh.name().len() == 0 {
            return false;
        }
        if sh.id() as usize != i + 1 {
            return false;
        }
        let mut j = i + 1usize;
        while j < SHAPE_COUNT {
            if let Some(other) = table.get(j) {
                if other.name() == sh.name() {
                    return false;
                }
                if other.id() == sh.id() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 每型至少有一条工具，且义务成立。
    let tools = Tool::all();
    let mut t = 0usize;
    while t < TOOL_COUNT {
        let tool = match tools.get(t) {
            Some(x) => *x,
            None => return false,
        };
        let sh = tool.shape();
        if sh.needs_prefix() && tool.prefix().len() == 0 {
            return false;
        }
        if !sh.needs_prefix() && tool.prefix().len() != 0 {
            return false;
        }
        if sh.needs_suffix() && tool.suffix().len() == 0 {
            return false;
        }
        if !sh.needs_suffix() && tool.suffix().len() != 0 {
            return false;
        }
        // 引号型必须夹引号路径；其余型只要求不带引号以外的东西（rsync 的后缀型前缀亦可带引号）。
        if sh.needs_quotes() && !tool.quoted_subject() {
            return false;
        }
        if sh == TextShape::Prefixed && tool.quoted_subject() {
            return false;
        }
        if sh.needs_newline() != tool.newline() {
            return false;
        }
        t += 1;
    }
    if count_shape(&tools, TextShape::Bare) != 1 {
        return false;
    }
    if count_shape(&tools, TextShape::Prefixed) != 1 {
        return false;
    }
    if count_shape(&tools, TextShape::Suffixed) != SUFFIX_TOOLS {
        return false;
    }
    if count_shape(&tools, TextShape::Quoted) != QUOTED_TOOLS {
        return false;
    }
    // 体例错标必须判死，且落 Shape 闸。
    let bad = Shape::exact(Tool::GlibcWrite).with_shape(TextShape::Bare);
    let o = define(&bad);
    if o.ok() || o.stage != Stage::Shape || o.code != TX_ESHAPE {
        return false;
    }
    if !o.refused() {
        return false;
    }
    true
}

/// 统计某体例下的工具条数。
fn count_shape(tools: &[Tool; TOOL_COUNT], sh: TextShape) -> usize {
    let mut n = 0usize;
    for t in tools.iter() {
        if t.shape() == sh {
            n += 1;
        }
    }
    n
}

/// 07 前缀逐字：七个前缀与常量表逐字相同，整行归属由前缀决定。
fn prefix_ok() -> bool {
    if Tool::Strerror.prefix() != P_NONE {
        return false;
    }
    if Tool::GlibcWrite.prefix() != P_GLIBC {
        return false;
    }
    if Tool::BusyboxDd.prefix() != P_BUSYBOX {
        return false;
    }
    if Tool::GnuDd.prefix() != P_GNUDD {
        return false;
    }
    if Tool::RustCoreutils.prefix() != P_RUST {
        return false;
    }
    if Tool::Rsync.prefix() != P_RSYNC {
        return false;
    }
    if Tool::Tee.prefix() != P_TEE {
        return false;
    }
    if P_GLIBC_TIGHT.len() + 1 != P_GLIBC.len() {
        return false;
    }
    if P_GLIBC_TIGHT == P_GLIBC {
        return false;
    }
    // 行归属：text 以本工具前缀开头。
    let tools = Tool::all();
    let mut i = 0usize;
    while i < TOOL_COUNT {
        let tool = match tools.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !tool.text().starts_with(tool.prefix()) {
            return false;
        }
        if tool.prefix().len() > 0 && tool.text() == BARE {
            return false;
        }
        i += 1;
    }
    // 前缀漂移必须判死，且落 Prefix 闸。
    let bad = Shape::exact(Tool::GlibcWrite).with_prefix(P_GLIBC_TIGHT);
    let o = define(&bad);
    if o.ok() || o.stage != Stage::Prefix || o.code != TX_EPREFIX {
        return false;
    }
    if o.note != Note::PrefixDrift {
        return false;
    }
    // 前缀被换到别的工具上：Tool 闸先拦（归属），不会走到 Prefix 闸。
    let foreign = Shape::exact(Tool::GlibcWrite).with_text(T_BUSYBOX);
    let f = define(&foreign);
    if f.stage != Stage::Tool || f.code != TX_ETOOL {
        return false;
    }
    true
}

/// 08 后缀逐字：只有两条工具带后缀，且尾巴各不相同。
fn suffix_ok() -> bool {
    if Tool::RustCoreutils.suffix() != S_RUST {
        return false;
    }
    if Tool::Rsync.suffix() != S_RSYNC {
        return false;
    }
    if Tool::Strerror.suffix() != S_NONE {
        return false;
    }
    if Tool::GlibcWrite.suffix() != S_NONE {
        return false;
    }
    if Tool::BusyboxDd.suffix() != S_NONE || Tool::GnuDd.suffix() != S_NONE {
        return false;
    }
    if Tool::Tee.suffix() != S_NONE {
        return false;
    }
    if S_RUST == S_RSYNC || S_RUST.len() == 0 || S_RSYNC.len() == 0 {
        return false;
    }
    if S_RUST_PERIOD == S_RUST || S_WRONG == S_RUST {
        return false;
    }
    if !S_RUST.contains("os error 28") {
        return false;
    }
    let mut with_suffix = 0usize;
    let tools = Tool::all();
    let mut i = 0usize;
    while i < TOOL_COUNT {
        if let Some(tool) = tools.get(i) {
            if tool.suffix().len() > 0 {
                with_suffix += 1;
            }
        }
        i += 1;
    }
    if with_suffix != SUFFIX_TOOLS {
        return false;
    }
    let bad = Shape::exact(Tool::RustCoreutils).with_suffix(S_RUST_PERIOD);
    let o = define(&bad);
    if o.ok() || o.stage != Stage::Suffix || o.code != TX_ESUFFIX {
        return false;
    }
    let wrong = Shape::exact(Tool::RustCoreutils).with_suffix(S_WRONG);
    let w = define(&wrong);
    if w.ok() || w.code != TX_ESUFFIX {
        return false;
    }
    true
}

/// 09 字节与换行：四段合成、整行长度、拆分回读三处必须同数。
fn bytes_ok() -> bool {
    let tools = Tool::all();
    let mut i = 0usize;
    let mut newlines = 0usize;
    while i < TOOL_COUNT {
        let tool = match tools.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want = tool.prefix().len() + BARE.len() + tool.suffix().len()
            + if tool.newline() { 1 } else { 0 };
        if tool.bytes() as usize != want {
            return false;
        }
        if tool.text().len() != want {
            return false;
        }
        let split = match split_line(tool.text(), BARE) {
            Some(x) => x,
            None => return false,
        };
        if split != (tool.prefix().len(), BARE.len(), tool.suffix().len(), tool.newline()) {
            return false;
        }
        let composed = compose(tool.prefix(), BARE, tool.suffix(), tool.newline());
        if composed.as_str() != tool.text() {
            return false;
        }
        if tool.newline() {
            newlines += 1;
            if !tool.text().ends_with('\n') {
                return false;
            }
        } else if tool.text().ends_with('\n') {
            return false;
        }
        i += 1;
    }
    if newlines != NEWLINE_TOOLS {
        return false;
    }
    if split_line(BARE_EDQUOT, BARE).is_some() {
        // 不含裸句的行必须给 None，不许硬凑偏移。
        return false;
    }
    if split_line("", BARE).is_some() {
        return false;
    }
    if split_line(T_TEE, "").is_some() {
        // 裸句为空时禁止给出偏移。
        return false;
    }
    // 字节少一必须判死，且落 Bytes 闸。
    let bad = Shape::exact(Tool::GlibcWrite).with_bytes(tool_bytes(Tool::GlibcWrite) - 1);
    let o = define(&bad);
    if o.ok() || o.stage != Stage::Bytes || o.code != TX_EBYTES {
        return false;
    }
    // 换行位反了同样判死。
    let nl = Shape::exact(Tool::GlibcWrite).with_newline(false);
    let n = define(&nl);
    if n.ok() || n.stage != Stage::Bytes || n.code != TX_EBYTES {
        return false;
    }
    true
}

/// 取工具行字节数（避免在谓词里重复算式）。
fn tool_bytes(t: Tool) -> u32 {
    t.bytes()
}

/// 10 语料十四槽互校：申报、期望表、判定三路必须给同一结论。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if WANT_STAGE.len() != CORPUS || WANT_CODE.len() != CORPUS {
        return false;
    }
    if WANT_NOTE.len() != CORPUS || WANT_TAGS.len() != CORPUS {
        return false;
    }
    if WANT_VALUE.len() != CORPUS || WANT_SUCCESS.len() != CORPUS {
        return false;
    }
    if WANT_NDX.len() != CORPUS || WANT_DEFAULT.len() != CORPUS {
        return false;
    }
    if WANT_VERDICT.len() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        let want_tag = match WANT_TAGS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c.tag() != want_tag {
            return false;
        }
        if c.shape.tool.id() as usize > TOOL_COUNT {
            return false;
        }
        let o = c.actual();
        if WANT_STAGE.get(i).copied() != Some(o.stage) {
            return false;
        }
        if WANT_CODE.get(i).copied() != Some(o.code) {
            return false;
        }
        if WANT_NOTE.get(i).copied() != Some(o.note) {
            return false;
        }
        if WANT_VERDICT.get(i).copied() != Some(o.verdict) {
            return false;
        }
        if WANT_VALUE.get(i).copied() != Some(o.value) {
            return false;
        }
        if WANT_SUCCESS.get(i).copied() != Some(o.ok()) {
            return false;
        }
        if WANT_DEFAULT.get(i).copied() != Some(o.default) {
            return false;
        }
        if judge(&c) != expected_reject_ndx(i) {
            return false;
        }
        if i < ACCEPT && !c.ok() {
            return false;
        }
        if i >= ACCEPT && c.ok() {
            return false;
        }
        let mut j = i + 1usize;
        while j < CORPUS {
            if let Some(other) = case_at(j) {
                if other.seal() == c.seal() {
                    return false;
                }
            }
            j += 1;
        }
        if cover(i).is_err() {
            return false;
        }
        i += 1;
    }
    if cover(CORPUS + 1).is_ok() {
        return false;
    }
    let all = cases();
    if all.len() != CORPUS {
        return false;
    }
    if all.get(0).map(|c| c.tag()) != Some("f11694-strerror-bare") {
        return false;
    }
    if SHAPES.get(0) != Some(&BASE) {
        return false;
    }
    if SHAPES.get(7) != Some(&BASE) {
        return false;
    }
    if CLAIMS.get(0).map(|c| c.tag) != Some("f11694-strerror-bare") {
        return false;
    }
    let excused = match CLAIMS.get(6) {
        Some(c) => *c,
        None => return false,
    };
    if excused.tag != "f11694-tee-quoted" || excused.verdict != Verdict::Carried {
        return false;
    }
    true
}

/// 11 判序六维互斥：六维一一对应、顺序固定、各指各的。
fn judge_ok() -> bool {
    let mut n = 0usize;
    while n <= 6 {
        let f = match ndx_fault(n) {
            Some(x) => x,
            None => return false,
        };
        if fault_ndx(f) != n {
            return false;
        }
        if n < REJECT && REJECT_CODES.get(n) != Some(&f.code()) {
            return false;
        }
        n += 1;
    }
    if ndx_fault(7).is_some() {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        if judge(&c) != expected_reject_ndx(i) {
            return false;
        }
        i += 1;
    }
    // 次序：同时谎报成败与阶段，必须挂 Estate。
    let c8 = match case_at(8) {
        Some(x) => x,
        None => return false,
    };
    let mut cross = c8.claim;
    cross.stage = Stage::Rate;
    let case_cross = Case {
        shape: c8.shape,
        claim: cross,
    };
    if judge(&case_cross) != Fault::Estate {
        return false;
    }
    // 次序：只谎报阶段，必须挂 Stage。
    let c9 = match case_at(9) {
        Some(x) => x,
        None => return false,
    };
    let mut stage_only = c9.claim;
    stage_only.code = 0;
    stage_only.value = 1;
    stage_only.note = c9.actual().note;
    let case_stage = Case {
        shape: c9.shape,
        claim: stage_only,
    };
    if judge(&case_stage) != Fault::Stage {
        return false;
    }
    // 次序：码与值同时谎报，必须挂 Code。
    let c10 = match case_at(10) {
        Some(x) => x,
        None => return false,
    };
    let mut code_first = c10.claim;
    code_first.value = 5;
    let case_code = Case {
        shape: c10.shape,
        claim: code_first,
    };
    if judge(&case_code) != Fault::Code {
        return false;
    }
    // 次序：值与注记同时谎报，必须挂 Value。
    let c11 = match case_at(11) {
        Some(x) => x,
        None => return false,
    };
    let mut value_first = c11.claim;
    value_first.note = Note::Verbatim;
    let case_value = Case {
        shape: c11.shape,
        claim: value_first,
    };
    if judge(&case_value) != Fault::Value {
        return false;
    }
    // 次序：注记与兜底同时谎报，必须挂 Note。
    let c13 = match case_at(13) {
        Some(x) => x,
        None => return false,
    };
    let mut note_first = c13.claim;
    note_first.note = Note::Verbatim;
    let case_note = Case {
        shape: c13.shape,
        claim: note_first,
    };
    if judge(&case_note) != Fault::Note {
        return false;
    }
    let only_default = Case {
        shape: c13.shape,
        claim: c13.claim,
    };
    if judge(&only_default) != Fault::Default {
        return false;
    }
    if Fault::Ok.code() != 0 {
        return false;
    }
    true
}

/// 12 四档矩阵覆盖：四档全有落点、全对、红格与判对互补。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS * TIER_COUNT {
        return false;
    }
    if m.goods() != CORPUS {
        return false;
    }
    if m.red_total() + m.goods() != m.slots() {
        return false;
    }
    if !m.covered() {
        return false;
    }
    let want = [
        (Tier::Exact, BARE_SAMPLES),
        (Tier::Carried, CARRIED_SAMPLES),
        (Tier::Void, VOID_SAMPLES),
        (Tier::Unrun, UNRUN_SAMPLES),
    ];
    let mut i = 0usize;
    while i < want.len() {
        let (tier, good) = match want.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let r = match m.row(tier) {
            Some(x) => x,
            None => return false,
        };
        if r.good != good || r.total != CORPUS {
            return false;
        }
        if r.red() + r.good != r.total {
            return false;
        }
        i += 1;
    }
    if m.render().len() == 0 {
        return false;
    }
    let exact_row = match m.row(Tier::Exact) {
        Some(r) => r,
        None => return false,
    };
    if exact_row.render().as_str() != "exact=2/14" {
        return false;
    }
    if Tier::all().len() != TIER_COUNT {
        return false;
    }
    if !Tier::Exact.is_green() || !Tier::Carried.is_green() {
        return false;
    }
    if Tier::Void.is_green() || Tier::Unrun.is_green() {
        return false;
    }
    // 归档：体例错进 Void，未判进 Unrun。
    let dead = match SHAPES.get(ACCEPT + 1) {
        Some(x) => *x,
        None => return false,
    };
    if Tier::of(&define(&dead)) != Tier::Void {
        return false;
    }
    let unjudged = match SHAPES.get(CORPUS - 1) {
        Some(x) => *x,
        None => return false,
    };
    if Tier::of(&define(&unjudged)) != Tier::Unrun {
        return false;
    }
    true
}

/// 13 汇总用：渲染 + 闸门 + 台账 + 四注入。
fn freeze_ok() -> bool {
    let rep = match run() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if !rep.green() || rep.render().as_str() != RENDER_OK {
        return false;
    }
    if rep.rate() != RATE_FULL || rep.matched != TOOL_COUNT {
        return false;
    }
    if rep.exact != BARE_SAMPLES || rep.carried != CARRIED_SAMPLES {
        return false;
    }
    if rep.void != VOID_SAMPLES || rep.unrun != UNRUN_SAMPLES {
        return false;
    }
    if rep.tier_cells() != CORPUS {
        return false;
    }
    if rep.gate() != Ok(()) || !report_agrees_with_matrix() {
        return false;
    }
    if !red_rate_calibrated() {
        return false;
    }
    if run_with(Some((CORPUS, Fault::Ok))) != Err(TX_ESLOT) {
        return false;
    }
    let mut l = build_ledger();
    if l.audit() != Ok(()) || l.len() != SAMPLES {
        return false;
    }
    let h = match l.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 || !l.is_frozen() || l.replay() != Ok(h) {
        return false;
    }
    if Gate::full_pass().is_err() {
        return false;
    }
    let four = inject_four();
    let mut i = 0usize;
    while i < four.len() {
        if let Some(x) = four.get(i) {
            if !*x {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 汇总：十三条判据，一条不多一条不少。
pub fn run_unx11694_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11694_enospc_text_verbatim");
    s.add("TX01-名称与段基址", name_ok(), "static");
    s.add("TX02-十三码互异", codes_ok(), "static");
    s.add("TX03-七工具全表", tools_ok(), "static");
    s.add("TX04-裸句二十三字节", bare_ok(), "static");
    s.add("TX05-strerror 逐字", strerror_ok(), "static");
    s.add("TX06-四型体例互斥", shapes_ok(), "static");
    s.add("TX07-前缀逐字", prefix_ok(), "static");
    s.add("TX08-后缀逐字", suffix_ok(), "static");
    s.add("TX09-字节与换行", bytes_ok(), "static");
    s.add("TX10-语料十四槽互校", corpus_ok(), "static");
    s.add("TX11-判序六维互斥", judge_ok(), "static");
    s.add("TX12-四档矩阵覆盖", matrix_ok(), "static");
    s.add("TX13-冻结重放四注入", freeze_ok(), "static");
    s
}

// ---------------------------------------------------------------------------
// 单测：逐条钉住本卡的每一个数、每一道闸、每一个码
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 取一条语料；槽号越界在单测里直接判红（生产路径一概不 panic）。
    fn need_case(i: usize) -> Case {
        match case_at(i) {
            Some(c) => c,
            None => {
                assert!(false, "corpus slot {} missing", i);
                Case {
                    shape: BASE,
                    claim: GOOD_CLAIM,
                }
            }
        }
    }

    /// 取一条语料现场。
    fn need_shape(i: usize) -> Shape {
        match SHAPES.get(i) {
            Some(s) => *s,
            None => {
                assert!(false, "shape slot {} missing", i);
                BASE
            }
        }
    }

    /// 取一格台账。
    fn need_sample(l: &Ledger, i: usize) -> Sample {
        match l.sample(i) {
            Some(s) => s,
            None => {
                assert!(false, "ledger slot {} missing", i);
                Sample {
                    slot: 0,
                    op: 0,
                    ok: false,
                    code: 0,
                    value: 0,
                }
            }
        }
    }

    /// 整行里裸句那一段（按前缀长度切）。
    fn core_slice(t: Tool) -> &'static str {
        let start = t.prefix().len();
        let end = start + BARE.len();
        match t.text().get(start..end) {
            Some(s) => s,
            None => "",
        }
    }

    // --- 段基址 / 锚点 / 错误码偏移 ---------------------------------------

    #[test]
    fn t01_seg_base_pinned() {
        assert_eq!(SEG_BASE, 0x6d3d0);
        assert_eq!(seg_text(SEG_BASE).as_str(), "0x6D3D0");
    }

    #[test]
    fn t02_neighbor_anchor_gaps() {
        assert_eq!(ANCHOR_F11695_SEG, 0x6d3e0);
        assert_eq!(ANCHOR_F11696_SEG, 0x6d3f0);
        assert_eq!(ANCHOR_F11693_SEG, 0x6d3c0);
        assert_eq!(ANCHOR_F11695_GAP, 0x10);
        assert_eq!(ANCHOR_F11696_GAP, 0x20);
        assert_eq!(SEG_BASE + ANCHOR_F11695_GAP, ANCHOR_F11695_SEG);
        assert_eq!(SEG_BASE + ANCHOR_F11696_GAP, ANCHOR_F11696_SEG);
        assert_eq!(SEG_BASE - ANCHOR_F11693_SEG, 0x10);
        assert!(ANCHOR_F11693_SEG < SEG_BASE);
        assert!(ANCHOR_F11695_SEG != ANCHOR_F11696_SEG);
    }

    #[test]
    fn t03_thirteen_codes_at_fixed_offsets() {
        assert_eq!(TX_EARG, SEG_BASE + 0x01);
        assert_eq!(TX_ETOOL, SEG_BASE + 0x02);
        assert_eq!(TX_ESHAPE, SEG_BASE + 0x03);
        assert_eq!(TX_EPREFIX, SEG_BASE + 0x04);
        assert_eq!(TX_ECORE, SEG_BASE + 0x05);
        assert_eq!(TX_ESUFFIX, SEG_BASE + 0x06);
        assert_eq!(TX_EBYTES, SEG_BASE + 0x07);
        assert_eq!(TX_ESTAGE, SEG_BASE + 0x08);
        assert_eq!(TX_ESTATE, SEG_BASE + 0x09);
        assert_eq!(TX_EVALUE, SEG_BASE + 0x0a);
        assert_eq!(TX_ENOTE, SEG_BASE + 0x0b);
        assert_eq!(TX_EDEFAULT, SEG_BASE + 0x0c);
        assert_eq!(TX_ESLOT, SEG_BASE + 0x0d);
        assert_eq!(ALL_CODES.len(), 13);
    }

    #[test]
    fn t04_codes_distinct_and_inside_segment() {
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let code = match ALL_CODES.get(i) {
                Some(c) => *c,
                None => {
                    assert!(false, "code {} missing", i);
                    return;
                }
            };
            assert!(code > SEG_BASE);
            assert!(code <= SEG_BASE + 0x0d);
            let mut j = i + 1usize;
            while j < ALL_CODES.len() {
                if let Some(other) = ALL_CODES.get(j) {
                    assert!(code != *other);
                }
                j += 1;
            }
            i += 1;
        }
    }

    // --- 清单 / 表 / 标签 / 指纹 -----------------------------------------

    #[test]
    fn t05_manifest_scale_constants() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(SAMPLES, CORPUS);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(STAGE_COUNT, 8);
        assert_eq!(STAGE_LAST_RANK, 8);
        assert_eq!(STAT_CELLS, 7);
        assert_eq!(TOOL_COUNT, 7);
        assert_eq!(SHAPE_COUNT, 4);
        assert_eq!(BARE_SAMPLES + CARRIED_SAMPLES, ACCEPT);
        assert_eq!(VOID_SAMPLES + UNRUN_SAMPLES, REJECT);
        assert_eq!(NEWLINE_TOOLS, TOOL_COUNT - 1);
        assert_eq!(RED_RATE, 929);
        assert_eq!(RATE_FULL, 1000);
        assert!(RED_RATE < RATE_FULL);
    }

    #[test]
    fn t06_corpus_tables_are_sized() {
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert_eq!(WANT_STAGE.len(), CORPUS);
        assert_eq!(WANT_CODE.len(), CORPUS);
        assert_eq!(WANT_NDX.len(), CORPUS);
        assert_eq!(WANT_NOTE.len(), CORPUS);
        assert_eq!(WANT_VERDICT.len(), CORPUS);
        assert_eq!(WANT_DEFAULT.len(), CORPUS);
        assert_eq!(WANT_TAGS.len(), CORPUS);
        assert_eq!(WANT_VALUE.len(), CORPUS);
        assert_eq!(WANT_SUCCESS.len(), CORPUS);
        assert_eq!(VALS.len(), ACCEPT);
        assert_eq!(STAT_OPS.len(), STAT_CELLS);
        assert_eq!(REJECT_CODES.len(), REJECT);
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(Fault::reject_faults().len(), REJECT);
        assert_eq!(Fault::reject_faults(), REJECT_FAULTS);
    }

    #[test]
    fn t07_tags_unique_and_card_prefixed() {
        let mut i = 0usize;
        while i < CORPUS {
            let tag = match WANT_TAGS.get(i) {
                Some(t) => *t,
                None => {
                    assert!(false, "tag {} missing", i);
                    return;
                }
            };
            assert!(tag.starts_with("f11694-"));
            assert_eq!(need_case(i).tag(), tag);
            assert_eq!(need_case(i).claim.tag, tag);
            let mut j = i + 1usize;
            while j < CORPUS {
                if let Some(other) = WANT_TAGS.get(j) {
                    assert!(tag != *other);
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(head_tag(), "f11694-strerror-bare");
        assert_eq!(head_tag(), WANT_TAGS[0]);
    }

    #[test]
    fn t08_tool_and_case_seals_unique() {
        let tools = Tool::all();
        let mut i = 0usize;
        while i < TOOL_COUNT {
            let tool = match tools.get(i) {
                Some(t) => *t,
                None => {
                    assert!(false, "tool {} missing", i);
                    return;
                }
            };
            assert!(tool.seal() != 0);
            let mut j = i + 1usize;
            while j < TOOL_COUNT {
                if let Some(other) = tools.get(j) {
                    assert!(tool.seal() != other.seal());
                }
                j += 1;
            }
            i += 1;
        }
        let mut a = 0usize;
        while a < CORPUS {
            let ca = need_case(a);
            assert!(ca.seal() != 0);
            let mut b = a + 1usize;
            while b < CORPUS {
                assert!(ca.seal() != need_case(b).seal());
                b += 1;
            }
            a += 1;
        }
        // 现场指纹对任何一位改动都敏感。
        let base = Shape::exact(Tool::GlibcWrite);
        assert!(base.seal() != base.with_bare(BARE_PERIOD).seal());
        assert!(base.seal() != base.with_bytes(base.bytes + 1).seal());
    }

    // --- 裸句逐字 ---------------------------------------------------------

    #[test]
    fn t09_bare_is_twenty_three_printable_bytes() {
        assert_eq!(BARE, "No space left on device");
        assert_eq!(BARE.len(), 23);
        assert_eq!(BARE.len() as u32, BARE_LEN);
        assert_eq!(BARE.as_bytes().len(), 23);
        assert_eq!(BARE.as_bytes().first(), Some(&b'N'));
        assert_eq!(BARE.as_bytes().last(), Some(&b'e'));
        for b in BARE.as_bytes() {
            assert!(*b >= 0x20 && *b <= 0x7e);
        }
        assert!(!BARE.ends_with(' '));
        assert!(!BARE.ends_with('.'));
        assert!(!BARE.contains("  "));
        assert_eq!(BARE_PERIOD.len(), BARE.len() + 1);
        assert_eq!(BARE_TRAIL.len(), BARE.len() + 1);
        assert_eq!(BARE_EDQUOT.len(), BARE.len() - 4);
    }

    #[test]
    fn t10_same_wording_inside_two_tools() {
        // glibc 与 rust coreutils 两条承载路，裸句段必须逐字节同一串。
        assert_eq!(core_slice(Tool::GlibcWrite), BARE);
        assert_eq!(core_slice(Tool::RustCoreutils), BARE);
        assert_eq!(core_slice(Tool::GlibcWrite), core_slice(Tool::RustCoreutils));
        assert_eq!(T_GLIBC.get(13..36), Some(BARE));
        assert_eq!(T_RUST.get(17..40), Some(BARE));
        assert_eq!(Tool::GlibcWrite.bytes(), 37);
        assert_eq!(Tool::RustCoreutils.bytes(), 55);
        assert_eq!(T_GLIBC.len() as u32, Tool::GlibcWrite.bytes());
        assert_eq!(T_RUST.len() as u32, Tool::RustCoreutils.bytes());
    }

    #[test]
    fn t11_all_tools_split_back_to_bare() {
        let tools = Tool::all();
        let mut i = 0usize;
        while i < TOOL_COUNT {
            let tool = match tools.get(i) {
                Some(t) => *t,
                None => {
                    assert!(false, "tool {} missing", i);
                    return;
                }
            };
            assert_eq!(core_slice(tool), BARE);
            assert_eq!(count_sub(tool.text(), BARE), 1);
            assert_eq!(
                split_line(tool.text(), BARE),
                Some((
                    tool.prefix().len(),
                    BARE.len(),
                    tool.suffix().len(),
                    tool.newline()
                ))
            );
            assert!(tool.composed());
            assert!(tool.owns_text());
            i += 1;
        }
    }

    #[test]
    fn t12_drifted_wording_caught_at_core_gate() {
        let base = Shape::exact(Tool::GlibcWrite);
        let period = define(&base.with_bare(BARE_PERIOD));
        assert!(!period.ok());
        assert_eq!(period.stage, Stage::Core);
        assert_eq!(period.code, TX_ECORE);
        assert_eq!(period.note, Note::CoreDrift);
        assert_eq!(period.verdict, Verdict::Mismatch);
        assert_eq!(period.value, 0);
        assert!(!period.refused());
        let trail = define(&base.with_bare(BARE_TRAIL));
        assert_eq!(trail.stage, Stage::Core);
        assert_eq!(trail.code, TX_ECORE);
        // 整行被挪到别的工具上：Tool 闸先拦，走不到 Core。
        let foreign = define(&Shape::exact(Tool::Tee).with_text(T_GLIBC));
        assert_eq!(foreign.stage, Stage::Tool);
        assert_eq!(foreign.code, TX_ETOOL);
        assert_eq!(foreign.note, Note::Foreign);
    }

    #[test]
    fn t13_strerror_knows_only_28_and_122() {
        assert_eq!(strerror(ENOSPC), Some(BARE));
        assert_eq!(strerror(EDQUOT), Some(BARE_EDQUOT));
        assert_eq!(strerror(0), None);
        assert_eq!(strerror(-1), None);
        assert_eq!(strerror(5), None);
        assert_eq!(strerror(13), None);
        assert!(BARE != BARE_EDQUOT);
        assert!(!BARE.ends_with('\n'));
    }

    // --- 逐槽 define / judge ---------------------------------------------

    #[test]
    fn t14_accept_slots_judge_ok() {
        let mut i = 0usize;
        while i < ACCEPT {
            let c = need_case(i);
            let o = c.actual();
            assert!(o.ok(), "slot {}", i);
            assert_eq!(o.stage, Stage::Bytes);
            assert_eq!(o.code, 0);
            assert_eq!(o.value, 1);
            assert_eq!(o.note, Note::Verbatim);
            assert!(!o.default);
            assert_eq!(c.fault(), Fault::Ok);
            assert!(c.ok());
            assert_eq!(c.claim.success, true);
            i += 1;
        }
    }

    #[test]
    fn t15_reject_slots_match_expectation_table() {
        let mut i = ACCEPT;
        while i < CORPUS {
            let c = need_case(i);
            let o = c.actual();
            assert!(!o.ok(), "slot {}", i);
            assert!(!c.ok());
            assert_eq!(o.stage, WANT_STAGE[i]);
            assert_eq!(o.code, WANT_CODE[i]);
            assert_eq!(o.note, WANT_NOTE[i]);
            assert_eq!(o.verdict, WANT_VERDICT[i]);
            assert_eq!(o.value, WANT_VALUE[i]);
            assert_eq!(o.default, WANT_DEFAULT[i]);
            assert_eq!(o.ok(), WANT_SUCCESS[i]);
            i += 1;
        }
    }

    #[test]
    fn t16_each_reject_slot_fault_code_matches() {
        let mut i = ACCEPT;
        while i < CORPUS {
            let c = need_case(i);
            let f = c.fault();
            assert!(f != Fault::Ok, "slot {}", i);
            assert_eq!(Some(f), expected(i));
            assert_eq!(fault_ndx(f), WANT_NDX[i]);
            assert_eq!(fault_ndx(f), i - ACCEPT);
            i += 1;
        }
    }

    #[test]
    fn t17_verdict_split_is_exact_two_carried_six() {
        assert_eq!(need_case(0).actual().verdict, Verdict::Exact);
        assert_eq!(need_case(7).actual().verdict, Verdict::Exact);
        let mut i = 1usize;
        while i < 7 {
            assert_eq!(need_case(i).actual().verdict, Verdict::Carried);
            i += 1;
        }
        assert_eq!(WANT_VERDICT[0], Verdict::Exact);
        assert_eq!(WANT_VERDICT[7], Verdict::Exact);
        assert!(Verdict::Exact.counts());
        assert!(Verdict::Carried.counts());
        assert!(!Verdict::Mismatch.counts());
    }

    #[test]
    fn t18_reject_slots_cover_six_dimensions_once() {
        assert_eq!(need_case(8).fault(), Fault::Estate);
        assert_eq!(need_case(9).fault(), Fault::Stage);
        assert_eq!(need_case(10).fault(), Fault::Code);
        assert_eq!(need_case(11).fault(), Fault::Value);
        assert_eq!(need_case(12).fault(), Fault::Note);
        assert_eq!(need_case(13).fault(), Fault::Default);
        assert_eq!(REJECT_CODES[0], Fault::Estate.code());
        assert_eq!(REJECT_CODES[1], Fault::Stage.code());
        assert_eq!(REJECT_CODES[2], Fault::Code.code());
        assert_eq!(REJECT_CODES[3], Fault::Value.code());
        assert_eq!(REJECT_CODES[4], Fault::Note.code());
        assert_eq!(REJECT_CODES[5], Fault::Default.code());
        assert_eq!(Fault::Code.code(), TX_EBYTES);
        assert_eq!(Fault::Stage.code(), TX_ESTAGE);
        assert_eq!(Fault::Ok.code(), 0);
    }

    #[test]
    fn t19_reject_slot_stages_and_refusals() {
        let shape = define(&need_shape(9));
        assert_eq!(shape.stage, Stage::Shape);
        assert_eq!(shape.code, TX_ESHAPE);
        assert_eq!(shape.note, Note::ShapeWrong);
        assert!(shape.refused());
        let prefix = define(&need_shape(10));
        assert_eq!(prefix.stage, Stage::Prefix);
        assert_eq!(prefix.code, TX_EPREFIX);
        assert_eq!(prefix.note, Note::PrefixDrift);
        assert!(!prefix.refused());
        let core = define(&need_shape(11));
        assert_eq!(core.stage, Stage::Core);
        assert_eq!(core.code, TX_ECORE);
        let suffix = define(&need_shape(12));
        assert_eq!(suffix.stage, Stage::Suffix);
        assert_eq!(suffix.code, TX_ESUFFIX);
        let ned = define(&need_shape(13));
        assert_eq!(ned.stage, Stage::Rate);
        assert_eq!(ned.code, TX_EDEFAULT);
        assert_eq!(ned.note, Note::Nil);
        assert!(ned.default);
        assert!(!ned.refused());
    }

    #[test]
    fn t20_estate_slot_is_one_byte_short() {
        let s = need_shape(8);
        assert_eq!(s.bytes, Shape::exact(Tool::GlibcWrite).bytes - 1);
        assert_eq!(s.bytes, 36);
        assert_eq!(s.text, T_GLIBC);
        let o = define(&s);
        assert_eq!(o.stage, Stage::Bytes);
        assert_eq!(o.code, TX_EBYTES);
        assert_eq!(o.note, Note::ByteDrift);
        // 换行位反了同样落 Bytes 闸。
        let nl = define(&Shape::exact(Tool::GlibcWrite).with_newline(false));
        assert_eq!(nl.stage, Stage::Bytes);
        assert_eq!(nl.code, TX_EBYTES);
    }

    // --- 报告 / 矩阵 / 档位 ----------------------------------------------

    #[test]
    fn t21_report_tier_split() {
        let rep = match run() {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "run() refused the green corpus");
                return;
            }
        };
        assert_eq!(rep.exact, BARE_SAMPLES);
        assert_eq!(rep.carried, CARRIED_SAMPLES);
        assert_eq!(rep.void, VOID_SAMPLES);
        assert_eq!(rep.unrun, UNRUN_SAMPLES);
        assert_eq!(rep.exact, 2);
        assert_eq!(rep.carried, 6);
        assert_eq!(rep.void, 1);
        assert_eq!(rep.unrun, 5);
        assert_eq!(rep.tier_cells(), CORPUS);
        assert_eq!(rep.tier_cells(), 14);
        assert_eq!(rep.accept, 8);
        assert_eq!(rep.reject, 6);
    }

    #[test]
    fn t22_matrix_rows_sum_to_corpus() {
        let m = Matrix::build();
        let exact = match m.row(Tier::Exact) {
            Some(r) => r,
            None => {
                assert!(false, "exact row missing");
                return;
            }
        };
        assert_eq!(exact.good, BARE_SAMPLES);
        assert_eq!(exact.total, CORPUS);
        let carried = match m.row(Tier::Carried) {
            Some(r) => r,
            None => {
                assert!(false, "carried row missing");
                return;
            }
        };
        assert_eq!(carried.good, CARRIED_SAMPLES);
        assert_eq!(carried.total, CORPUS);
        let void = match m.row(Tier::Void) {
            Some(r) => r,
            None => {
                assert!(false, "void row missing");
                return;
            }
        };
        assert_eq!(void.good, VOID_SAMPLES);
        let unrun = match m.row(Tier::Unrun) {
            Some(r) => r,
            None => {
                assert!(false, "unrun row missing");
                return;
            }
        };
        assert_eq!(unrun.good, UNRUN_SAMPLES);
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert_eq!(m.slots(), 56);
        assert_eq!(m.goods(), CORPUS);
        assert_eq!(m.red_total(), m.slots() - m.goods());
        assert_eq!(m.red_total(), 42);
        assert!(m.covered());
        // 每档只判对自己那一小格，故没有整行全对。
        assert_eq!(m.perfect_rows(), 0);
    }

    #[test]
    fn t23_matrix_row_and_full_render() {
        let m = Matrix::build();
        let exact = match m.row(Tier::Exact) {
            Some(r) => r,
            None => {
                assert!(false, "exact row missing");
                return;
            }
        };
        assert_eq!(exact.render().as_str(), "exact=2/14");
        let carried = match m.row(Tier::Carried) {
            Some(r) => r,
            None => {
                assert!(false, "carried row missing");
                return;
            }
        };
        assert_eq!(carried.render().as_str(), "carried=6/14");
        let void = match m.row(Tier::Void) {
            Some(r) => r,
            None => {
                assert!(false, "void row missing");
                return;
            }
        };
        assert_eq!(void.render().as_str(), "void=1/14");
        let unrun = match m.row(Tier::Unrun) {
            Some(r) => r,
            None => {
                assert!(false, "unrun row missing");
                return;
            }
        };
        assert_eq!(unrun.render().as_str(), "unrun=5/14");
        assert_eq!(
            m.render().as_str(),
            "matrix exact=2/14 carried=6/14 void=1/14 unrun=5/14"
        );
    }

    #[test]
    fn t24_tier_archive_of_outcomes() {
        assert_eq!(Tier::of(&define(&need_shape(0))), Tier::Exact);
        assert_eq!(Tier::of(&define(&need_shape(1))), Tier::Carried);
        assert_eq!(Tier::of(&define(&need_shape(9))), Tier::Void);
        assert_eq!(Tier::of(&define(&need_shape(8))), Tier::Unrun);
        assert_eq!(Tier::of(&define(&need_shape(13))), Tier::Unrun);
        assert!(Tier::Exact.is_green());
        assert!(Tier::Carried.is_green());
        assert!(!Tier::Void.is_green());
        assert!(!Tier::Unrun.is_green());
        assert_eq!(Tier::all().len(), TIER_COUNT);
    }

    // --- run / run_with ---------------------------------------------------

    #[test]
    fn t25_run_report_fields() {
        let rep = match run() {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "run() refused the green corpus");
                return;
            }
        };
        assert_eq!(rep.total, TOOL_COUNT);
        assert_eq!(rep.total, 7);
        assert_eq!(rep.matched, rep.total);
        assert_eq!(rep.matched, 7);
        assert_eq!(rep.conflicts, 0);
        assert_eq!(rep.rate(), RATE_FULL);
        assert_eq!(rep.rate(), 1000);
        assert!(rep.green());
        assert_eq!(rep.gate(), Ok(()));
        assert_eq!(rep.reds(), 0);
    }

    #[test]
    fn t26_run_render_is_byte_identical() {
        let rep = match run() {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "run() refused the green corpus");
                return;
            }
        };
        assert_eq!(
            rep.render().as_str(),
            "enospc-text tools=7 shapes=4 bare=23 errno=28 rate=1000permille\n"
        );
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(
            RENDER_OK,
            "enospc-text tools=7 shapes=4 bare=23 errno=28 rate=1000permille\n"
        );
    }

    #[test]
    fn t27_run_with_rejects_out_of_range_slot() {
        assert_eq!(run_with(Some((CORPUS, Fault::Ok))), Err(TX_ESLOT));
        assert_eq!(run_with(Some((CORPUS + 1, Fault::Default))), Err(TX_ESLOT));
        assert_eq!(run_with(Some((100, Fault::Estate))), Err(TX_ESLOT));
        // 槽 13 是最后一个合法槽位，必须放行。
        let last = run_with(Some((CORPUS - 1, Fault::Ok)));
        assert!(last.is_ok());
        // 无注入与 run() 同一条路径。
        assert_eq!(run_with(None), run());
    }

    #[test]
    fn t28_run_with_in_range_breaks_one_slot() {
        let base = match run() {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "run() refused the green corpus");
                return;
            }
        };
        let bad = match run_with(Some((1, Fault::Value))) {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "in-range override must still settle");
                return;
            }
        };
        assert_eq!(bad.conflicts, base.conflicts + 1);
        assert_eq!(bad.conflicts, 1);
        assert_eq!(bad.matched, base.matched - 1);
        assert_eq!(bad.matched, 6);
        assert_eq!(bad.rate(), 857);
        assert!(!bad.green());
        assert_eq!(bad.gate(), Err(TX_ENOTE));
        assert_eq!(bad.reds(), 1);
        assert!(bad.render().as_str() != RENDER_OK);
        assert_eq!(
            bad.render().as_str(),
            "enospc-text tools=7 shapes=4 bare=23 errno=28 rate=857permille\n"
        );
        // 档位统计不受注入影响（注入只动违约计数）。
        assert_eq!(bad.exact, base.exact);
        assert_eq!(bad.carried, base.carried);
        assert_eq!(bad.void, base.void);
        assert_eq!(bad.unrun, base.unrun);
    }

    #[test]
    fn t29_run_with_matching_reject_override_stays_green() {
        let good = match run_with(Some((8, Fault::Estate))) {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "matching reject override must settle");
                return;
            }
        };
        assert_eq!(good.conflicts, 0);
        assert_eq!(good.matched, 7);
        assert_eq!(good.rate(), RATE_FULL);
        assert!(good.green());
        // 把反例的期望维换成 Ok 就多一处违约。
        let tampered = match run_with(Some((13, Fault::Ok))) {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "in-range override must still settle");
                return;
            }
        };
        assert_eq!(tampered.conflicts, 1);
        assert_eq!(tampered.matched, 6);
    }

    // --- 闸门 -------------------------------------------------------------

    #[test]
    fn t30_gate_monotone_walk() {
        let all = Stage::all();
        assert_eq!(all.len() as u32, STAGE_COUNT);
        let g = match Gate::walk(&all) {
            Ok(x) => x,
            Err(_) => {
                assert!(false, "monotone walk must be accepted");
                return;
            }
        };
        assert_eq!(g.steps(), STAGE_COUNT);
        assert_eq!(g.stage(), Some(Stage::Rate));
        assert!(!g.is_closed());
        assert_eq!(g.refusals(), 0);
        assert!(g.root() != 0);
        let h = match Gate::full_pass() {
            Ok(x) => x,
            Err(_) => {
                assert!(false, "full_pass must be accepted");
                return;
            }
        };
        assert!(h != 0);
        let mut i = 1u32;
        while i <= STAGE_LAST_RANK {
            assert_eq!(Stage::of_rank(i).map(|s| s.rank()), Some(i));
            i += 1;
        }
        assert_eq!(Stage::of_rank(0), None);
        assert_eq!(Stage::of_rank(9), None);
    }

    #[test]
    fn t31_gate_regression_rejected() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Arg), Ok(()));
        assert_eq!(g.step(Stage::Tool), Ok(()));
        assert_eq!(g.step(Stage::Shape), Ok(()));
        assert_eq!(g.step(Stage::Arg), Err(TX_ESTAGE));
        assert_eq!(g.stage(), Some(Stage::Shape));
        assert_eq!(g.steps(), 3);
        assert_eq!(g.refusals(), 1);
        assert!(!g.is_closed());
        // 同秩重复允许（只禁倒退）。
        assert_eq!(g.step(Stage::Shape), Ok(()));
        assert_eq!(g.steps(), 4);
        // 整串逆序：第一步 Rate 成立，第二步 Arg 倒退。
        assert_eq!(
            Gate::walk(&[Stage::Rate, Stage::Arg]),
            Err(TX_ESTAGE)
        );
    }

    #[test]
    fn t32_gate_post_seal_write_rejected() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Arg), Ok(()));
        let h = match g.close() {
            Ok(x) => x,
            Err(_) => {
                assert!(false, "close after a step must succeed");
                return;
            }
        };
        assert!(g.is_closed());
        assert_eq!(g.root(), h);
        assert_eq!(g.step(Stage::Rate), Err(TX_ESTATE));
        assert_eq!(g.close(), Err(TX_ESTATE));
        assert_eq!(g.refusals(), 2);
        assert_eq!(g.steps(), 1);
        // 没进过任何闸就封账：判死 ESTAGE，且不计拒收。
        let mut fresh = Gate::new();
        assert_eq!(fresh.close(), Err(TX_ESTAGE));
        assert_eq!(fresh.refusals(), 0);
        assert!(!fresh.is_closed());
    }

    #[test]
    fn t33_gate_render_text() {
        let mut g = Gate::new();
        assert_eq!(
            g.render().as_str(),
            "gate stage=none steps=0 refusals=0 closed=false"
        );
        assert_eq!(g.step(Stage::Arg), Ok(()));
        assert_eq!(g.step(Stage::Tool), Ok(()));
        assert_eq!(
            g.render().as_str(),
            "gate stage=tool steps=2 refusals=0 closed=false"
        );
        assert_eq!(g.close().is_ok(), true);
        assert_eq!(
            g.render().as_str(),
            "gate stage=tool steps=2 refusals=0 closed=true"
        );
    }

    // --- 台账 -------------------------------------------------------------

    #[test]
    fn t34_ledger_build_and_audit() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.len(), 14);
        assert!(!l.is_empty());
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.first_mismatch(), None);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(!l.is_frozen());
        let first = need_sample(&l, 0);
        assert_eq!(first.slot, 0);
        assert_eq!(first.op, STAT_OPS[0]);
        assert!(first.ok);
        assert_eq!(first.code, 0);
        assert_eq!(first.value, 7);
        let red = need_sample(&l, 7);
        assert_eq!(red.op, OPID_STAT_RED);
        assert_eq!(red.value, RED_RATE);
        let defect = need_sample(&l, 8);
        assert!(!defect.ok);
        assert_eq!(defect.code, TX_ESTATE);
        assert_eq!(defect.op, DEFECT_OP_BASE);
        assert_eq!(defect.value, 0);
        let last = need_sample(&l, 13);
        assert_eq!(last.code, TX_EDEFAULT);
        assert_eq!(last.op, DEFECT_OP_BASE + 5);
    }

    #[test]
    fn t35_ledger_freeze_and_replay() {
        let mut l = build_ledger();
        let h = match l.freeze() {
            Ok(x) => x,
            Err(_) => {
                assert!(false, "freeze of a clean ledger must succeed");
                return;
            }
        };
        assert!(h != 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), h);
        assert_eq!(l.seal(), h);
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(TX_ESTATE));
    }

    #[test]
    fn t36_frozen_ledger_refuses_poke_and_replay_holds() {
        let mut l = build_ledger();
        let h = match l.freeze() {
            Ok(x) => x,
            Err(_) => {
                assert!(false, "freeze of a clean ledger must succeed");
                return;
            }
        };
        // 封账后任何改动都被拒：账本不可能被"冻后篡改"。
        assert_eq!(l.poke(0, 1), Err(TX_ESTATE));
        assert_eq!(l.rejects(), 0);
        let extra = Sample {
            slot: 14,
            op: DEFECT_OP_BASE + 6,
            ok: false,
            code: TX_EDEFAULT,
            value: 0,
        };
        assert_eq!(l.register(extra), Err(TX_ESTATE));
        assert_eq!(l.rejects(), 1);
        // 篡改被拒，重放仍给同一指纹。
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.root(), h);
        assert_eq!(need_sample(&l, 0).value, VALS[0]);
    }

    #[test]
    fn t37_poked_ledger_cannot_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.poke(0, 99), Ok(()));
        assert_eq!(need_sample(&l, 0).value, 99);
        assert_eq!(l.audit(), Err(TX_EVALUE));
        assert_eq!(l.freeze(), Err(TX_EVALUE));
        assert!(!l.is_frozen());
        assert_eq!(l.replay(), Err(TX_ESTATE));
        // 改回钉值后复核恢复，冻结与重放重新成立。
        assert_eq!(l.poke(0, VALS[0]), Ok(()));
        assert_eq!(l.audit(), Ok(()));
        let h = match l.freeze() {
            Ok(x) => x,
            Err(_) => {
                assert!(false, "restored ledger must freeze");
                return;
            }
        };
        assert_eq!(l.replay(), Ok(h));
    }

    #[test]
    fn t38_ledger_out_of_order_push_rejected() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        let out_of_order = Sample {
            slot: 1,
            op: 1,
            ok: true,
            code: 0,
            value: 7,
        };
        assert_eq!(l.register(out_of_order), Err(TX_ESLOT));
        assert_eq!(l.len(), 0);
        assert_eq!(l.rejects(), 1);
        let head = Sample {
            slot: 0,
            op: 1,
            ok: true,
            code: 0,
            value: 7,
        };
        assert_eq!(l.register(head), Ok(()));
        assert_eq!(l.len(), 1);
        assert_eq!(l.register(head), Err(TX_ESLOT));
        assert_eq!(l.rejects(), 2);
        // 满账再推一格：槽位越界。
        let mut full = build_ledger();
        let overflow = Sample {
            slot: 14,
            op: DEFECT_OP_BASE + 6,
            ok: false,
            code: TX_EDEFAULT,
            value: 0,
        };
        assert_eq!(full.register(overflow), Err(TX_ESLOT));
    }

    #[test]
    fn t39_ledger_broken_injections_audited() {
        let b0 = build_ledger_broken(0);
        assert_eq!(b0.audit(), Err(TX_EVALUE));
        assert_eq!(b0.first_mismatch(), Some((7, "stat-value")));
        let b1 = build_ledger_broken(1);
        assert_eq!(b1.audit(), Err(TX_EVALUE));
        assert_eq!(b1.first_mismatch(), Some((0, "stat-value")));
        let b2 = build_ledger_broken(2);
        assert_eq!(b2.audit(), Err(TX_EVALUE));
        assert_eq!(b2.first_mismatch(), Some((8, "defect-value")));
        // 未知注入号原样返回，账仍成立。
        assert_eq!(build_ledger_broken(3).audit(), Ok(()));
        let mut frozen = build_ledger_broken(0);
        assert_eq!(frozen.freeze(), Err(TX_EVALUE));
        assert!(!frozen.is_frozen());
    }

    #[test]
    fn t40_ledger_render_text() {
        let l = build_ledger();
        assert_eq!(
            l.render().as_str(),
            "ledger len=14 ok=8 err=6 rejects=0 frozen=false"
        );
        let mut f = build_ledger();
        assert!(f.freeze().is_ok());
        assert_eq!(
            f.render().as_str(),
            "ledger len=14 ok=8 err=6 rejects=0 frozen=true"
        );
    }

    // --- 四注入 / 汇总 ----------------------------------------------------

    #[test]
    fn t41_inject_four_all_caught() {
        assert_eq!(inject_four(), [true, true, true, true]);
        assert!(report_agrees_with_matrix());
        assert!(red_rate_calibrated());
    }

    #[test]
    fn t42_checkset_has_thirteen_green_checks() {
        let cs = run_unx11694_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
        assert_eq!(cs.passed_count(), 13);
        assert_eq!(cs.failed_count(), 0);
        assert_eq!(cs.tally(), (13, 0));
        let mut i = 0usize;
        while i < cs.len() {
            let c = match cs.get(i) {
                Some(x) => x,
                None => {
                    assert!(false, "check {} missing", i);
                    return;
                }
            };
            assert!(c.passed);
            assert!(c.name.starts_with("TX"));
            assert_eq!(c.detail, "static");
            i += 1;
        }
        let first = match cs.get(0) {
            Some(x) => x,
            None => {
                assert!(false, "first check missing");
                return;
            }
        };
        assert_eq!(first.name, "TX01-名称与段基址");
        let last = match cs.get(12) {
            Some(x) => x,
            None => {
                assert!(false, "last check missing");
                return;
            }
        };
        assert_eq!(last.name, "TX13-冻结重放四注入");
    }

    // --- 呈现 / 文案 -------------------------------------------------------

    #[test]
    fn t43_sample_render_text() {
        let l = build_ledger();
        assert_eq!(
            need_sample(&l, 0).render().as_str(),
            "slot 0 op 01 true code 00000 value 7"
        );
        assert_eq!(
            need_sample(&l, 8).render().as_str(),
            "slot 8 op 21 false code 6D3D9 value 0"
        );
    }

    #[test]
    fn t44_verbatim_message_helpers() {
        let g = Shape::exact(Tool::GlibcWrite);
        assert_eq!(message(&g, &define(&g)).as_str(), "glibc-write: 37B verbatim");
        let s = Shape::exact(Tool::Strerror);
        assert_eq!(message(&s, &define(&s)).as_str(), "strerror: 23B verbatim");
        let t = Shape::exact(Tool::Tee);
        assert_eq!(message(&t, &define(&t)).as_str(), "tee: 34B verbatim");
        assert_eq!(bytes_text(BARE_LEN).as_str(), "23B");
        assert_eq!(errno_text(ENOSPC).as_str(), "errno=28");
        assert_eq!(errno_text(EDQUOT).as_str(), "errno=122");
        assert_eq!(permille_text(RATE_FULL).as_str(), "1000permille");
        assert_eq!(msg_errno(EDQUOT).as_str(), "expected errno=28 got errno=122");
        assert_eq!(msg_bytes(37, 36).as_str(), "bytes 36 != 37");
        assert_eq!(msg_rate_short(6, 7).as_str(), "verbatim rate 6/7 below 1000");
        assert_eq!(msg_usage().as_str(), "usage: enospc-text <tool> <errno>");
        assert_eq!(
            msg_foreign("tee").as_str(),
            "tee line does not start with its own prefix"
        );
        assert_eq!(
            msg_core("glibc-write", BARE_PERIOD).as_str(),
            "glibc-write core is No space left on device."
        );
        assert_eq!(
            msg_prefix("glibc-write", P_GLIBC_TIGHT, P_GLIBC).as_str(),
            "glibc-write prefix write error: != write error: "
        );
        assert_eq!(
            msg_shape("glibc-write", "prefixed", "bare").as_str(),
            "glibc-write shape bare != prefixed"
        );
        assert_eq!(
            msg_suffix("rust-coreutils", S_RUST_PERIOD, S_RUST).as_str(),
            "rust-coreutils suffix  (os error 28). !=  (os error 28)"
        );
    }

    #[test]
    fn t45_detail_of_first_and_last_case() {
        assert_eq!(
            detail(&need_case(0)).as_str(),
            "f11694-strerror-bare tool=strerror errno=28 shape=bare stage=bytes \
             err=0x00000 note=verbatim verdict=exact bytes=23"
        );
        assert_eq!(
            detail(&need_case(13)).as_str(),
            "f11694-default-unjudged tool=glibc-write errno=28 shape=prefixed \
             stage=rate err=0x6D3DC note=nil verdict=mismatch bytes=37"
        );
    }

    #[test]
    fn t46_compose_and_split_roundtrip() {
        assert_eq!(compose(P_RUST, BARE, S_RUST, true).as_str(), T_RUST);
        assert_eq!(compose(P_RSYNC, BARE, S_RSYNC, true).as_str(), T_RSYNC);
        assert_eq!(compose(P_NONE, BARE, S_NONE, false).as_str(), BARE);
        assert_eq!(
            split_line(T_RSYNC, BARE),
            Some((P_RSYNC.len(), BARE.len(), S_RSYNC.len(), true))
        );
        assert_eq!(split_line(T_BUSYBOX, BARE), Some((17, 23, 0, true)));
        assert_eq!(split_line(BARE, BARE), Some((0, 23, 0, false)));
        assert_eq!(split_line(BARE_EDQUOT, BARE), None);
        assert_eq!(split_line("", BARE), None);
        assert_eq!(split_line(T_TEE, ""), None);
    }

    #[test]
    fn t47_tool_table_roundtrip() {
        let tools = Tool::all();
        let mut i = 0usize;
        while i < TOOL_COUNT {
            let tool = match tools.get(i) {
                Some(t) => *t,
                None => {
                    assert!(false, "tool {} missing", i);
                    return;
                }
            };
            assert_eq!(tool.id() as usize, i + 1);
            assert_eq!(Tool::of_id(tool.id()), Some(tool));
            assert!(tool.name().len() > 0);
            assert_eq!(tool.bare_offset(), tool.prefix().len());
            assert_eq!(tool.bytes() as usize, tool.text().len());
            i += 1;
        }
        assert_eq!(Tool::of_id(0), None);
        assert_eq!(Tool::of_id(8), None);
        assert_eq!(Tool::Strerror.text(), BARE);
        assert_eq!(Tool::Strerror.newline(), false);
        assert_eq!(Tool::Strerror.prefix(), P_NONE);
        assert_eq!(Tool::RustCoreutils.suffix(), S_RUST);
        assert_eq!(Tool::Rsync.suffix(), S_RSYNC);
        assert_eq!(Tool::Tee.bytes(), 34);
        assert_eq!(Tool::BusyboxDd.bytes(), 41);
        assert_eq!(Tool::GnuDd.bytes(), 47);
    }

    #[test]
    fn t48_shape_counts_and_obligations() {
        let tools = Tool::all();
        assert_eq!(count_shape(&tools, TextShape::Bare), 1);
        assert_eq!(count_shape(&tools, TextShape::Prefixed), 1);
        assert_eq!(count_shape(&tools, TextShape::Suffixed), SUFFIX_TOOLS);
        assert_eq!(count_shape(&tools, TextShape::Quoted), QUOTED_TOOLS);
        assert_eq!(count_shape(&tools, TextShape::Quoted), 3);
        assert!(!TextShape::Bare.needs_prefix());
        assert!(!TextShape::Bare.needs_suffix());
        assert!(!TextShape::Bare.needs_quotes());
        assert!(!TextShape::Bare.needs_newline());
        assert!(TextShape::Prefixed.needs_prefix());
        assert!(!TextShape::Prefixed.needs_suffix());
        assert!(TextShape::Suffixed.needs_suffix());
        assert!(TextShape::Quoted.needs_quotes());
        assert!(TextShape::Quoted.needs_newline());
        assert!(Tool::Tee.quoted_subject());
        assert!(Tool::GnuDd.quoted_subject());
        assert!(!Tool::GlibcWrite.quoted_subject());
        let mut i = 0usize;
        while i < SHAPE_COUNT {
            let sh = match TextShape::all().get(i) {
                Some(x) => *x,
                None => {
                    assert!(false, "shape {} missing", i);
                    return;
                }
            };
            assert_eq!(sh.id() as usize, i + 1);
            assert!(sh.name().len() > 0);
            i += 1;
        }
    }

    #[test]
    fn t49_cover_and_expected_bounds() {
        assert_eq!(cover(0), Ok(()));
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS + 1), Err(TX_ESLOT));
        assert_eq!(expected(0), Some(Fault::Ok));
        assert_eq!(expected(7), Some(Fault::Ok));
        assert_eq!(expected(8), Some(Fault::Estate));
        assert_eq!(expected(13), Some(Fault::Default));
        assert_eq!(expected(14), None);
        assert_eq!(fault_ndx(Fault::Ok), 6);
        assert_eq!(ndx_fault(6), Some(Fault::Ok));
        assert_eq!(ndx_fault(7), None);
        assert_eq!(ndx_fault(0), Some(Fault::Estate));
    }

    #[test]
    fn t50_value_and_op_tables() {
        assert_eq!(VALS, [7u32, 4, 8, 6, 23, 28, 1000, 929]);
        assert_eq!(VALS[0], TOOL_COUNT as u32);
        assert_eq!(VALS[1], SHAPE_COUNT as u32);
        assert_eq!(VALS[4], BARE_LEN);
        assert_eq!(VALS[5], ENOSPC as u32);
        assert_eq!(VALS[6], RATE_FULL);
        assert_eq!(VALS[7], RED_RATE);
        assert_eq!(STAT_OPS, [1u8, 2, 3, 4, 5, 6, 7]);
        assert_eq!(OPID_STAT_RED, 0x08);
        assert_eq!(DEFECT_OP_BASE, 0x21);
        assert_eq!(head_val(), TOOL_COUNT as u32);
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(SEG_BASE, 5).as_str(), "6D3D0");
        assert_eq!(hex_text(0, 5).as_str(), "00000");
        assert_eq!(hex_text(0x2a, 0).as_str(), "");
    }
}
