//! UNX-F11670 无效参数值（非法数值 / 非法枚举）判据
//!
//! 本卡只判一件事：**给选项喂一个不合法的值时，程序打出的那几行字与退出的那一档码**。
//! 它既不是"未知选项"（那是 F11669），也不是"未名 applet"（那是 F11674），更不是
//! "帮助正文逐字"（那是 F11668）——本卡的现场永远是"选项认了，值没认"。
//!
//! # 闸门次序（判据的七道闸，秩从 1 起，只许前进）
//!
//! 1. `Dispatch`  —— 这个选项是不是本卡管辖：`head -n` / `tail -n` / `ls --color` /
//!    `ls --sort` / `cp --reflink` 认，别的名字直接落兜底（`default = true`）。
//! 2. `Option`    —— 这条文本线上有没有这个选项：busybox 的 `ls` 没有 `--sort`，
//!    枚举型值在 busybox 上根本没有"值"这一层，落 `no-such-option`（拒收）。
//! 3. `Value`     —— 值本身合不合法：十进制数值要能整串解析（`12x` 不算数），
//!    枚举要落在该选项的合法集里。**合法值不是本卡的现场**：落 `value-accepted`，
//!    兜底位为真，谁把它当结论谁红。
//! 4. `Message`   —— 取该选项该家族的那一格逐字文案；采集只留首行的落 `truncated`。
//! 5. `Channel`   —— 诊断恒走 **stderr**；打到 stdout 就是通道误读，落 `misrouted`。
//! 6. `Code`      —— 码档：数值型非法退 1（`EXIT_FAIL`），枚举型非法退 2
//!    （`EXIT_USAGE`，coreutils 的用法错），busybox 短报退 1。
//! 7. `Exit`      —— 结算注解档（正文家族）并封账。
//!
//! 这条纵线为什么不能倒：先辨选项才谈得上值，先定值才拼得出文案，先拼文案才有
//! 通道与码档可结算。允许倒退就会出现"先定退 2 再回头找选项"——一个 `head -n abc`
//! 被回填成枚举用法错，本卡判据当场失效。
//!
//! # 关键差异（两族三线，逐字分开）
//!
//! * **数值错**：coreutils 说 `head: invalid number of lines: 'abc'` 并附一行
//!   `Try 'head --help' for more information.`（两行，退 1）；busybox 只说
//!   `head: invalid number: 'abc'`（一行，退 1），**没有** `of lines`，也**没有** Try 行。
//! * **枚举错**：coreutils 说 `ls: invalid argument 'bogus' for '--color'`，随后
//!   `Valid arguments are:` 与三/五条候选，最后 `Try 'ls --help' for more information.`，
//!   退 **2**。候选条数是本卡唯一允许变长的量：`--color` 三条、`--sort` 五条、
//!   `--reflink` 三条。
//! * **引号**：C locale 用 `'abc'`（ASCII 直角单引号），UTF-8 locale 用 `‘abc’`
//!   （U+2018/U+2019）。行数一字不差，字节数不同——这一维单独成档，不许与逐字档混记。
//! * **`-h` 不是帮助**：`-h` 是 `--human-readable`，本卡不管；但 `head -c -3` 这类
//!   合法负值也不在本卡（不合法才在）。
//!
//! 判据原文：`UNX-F11670-J1 非法值报错文案与码档判据绿（stderr 通道，两族分家）`。

use crate::alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 段基址与锚点
// ---------------------------------------------------------------------------

/// 本卡段基址（分配表钉死，不许改）。
pub const SEG_BASE: u32 = 0x6d220;

/// 前一张卡（F11668 帮助逐字）的段基址。
pub const ANCHOR_F11668_SEG: u32 = 0x6d1f0;

/// 本卡与前卡的间距（0x30：给 F11668 的十三码留足 0x0d 的余量）。
pub const ANCHOR_F11668_GAP: u32 = 0x30;

/// 后一张卡（F11672 install 降级）的段基址。
pub const ANCHOR_F11672_SEG: u32 = 0x6d250;

/// 本卡与后卡的间距。
pub const ANCHOR_F11672_GAP: u32 = 0x30;

/// 锚点：F11674（未名 applet 文案）。
pub const ANCHOR_F11674_SEG: u32 = 0x6d270;

/// 锚点：F11669（未知选项）。
pub const ANCHOR_F11669_SEG: u32 = 0x6d210;

/// 锚点：F11673（相邻批次的 install 判据）。
pub const ANCHOR_F11673_SEG: u32 = 0x6d260;

// ---------------------------------------------------------------------------
// 规模常量
// ---------------------------------------------------------------------------

/// 语料槽数（八活六死）。
pub const CORPUS: usize = 14;

/// 台账格数（七统计 + 一红档 + 六缺陷码）。
pub const SAMPLES: usize = 14;

/// 正例槽数。
pub const ACCEPT: usize = 8;

/// 反例槽数。
pub const REJECT: usize = 6;

/// 档位数。
pub const TIER_COUNT: usize = 4;

/// 闸数。
pub const STAGE_COUNT: u32 = 7;

/// 末闸秩。
pub const STAGE_LAST_RANK: u32 = 7;

/// 统计格数（不含红档格）。
pub const STAT_CELLS: usize = 7;

/// 红档率标定值：14 格里 13 格判对的千分率。
pub const RED_RATE: u32 = 929;

/// 满一致率千分比。
pub const RATE_FULL: u32 = 1000;

/// 选项总数（含未名）。
pub const OPT_COUNT: usize = 6;

/// 进本卡的选项数。
pub const OPT_JUDGED: usize = 5;

/// 文本家族数（C / UTF-8 / busybox）。
pub const FAMILY_COUNT: usize = 3;

/// 值种类数（数值 / 枚举）。
pub const KIND_COUNT: usize = 2;

/// 文案表格数：数值两条线各 2 格（C、UTF-8）+ 短报 2 格，枚举 3 选项 × 2 线 = 6，
/// 合计 12 格。
pub const MESSAGE_CELLS: usize = 12;

/// 采集只留首行时允许的最大字节数。
pub const FIRST_BYTE_CAP: u32 = 160;

/// 成功码。
pub const EXIT_OK: u32 = 0;

/// 运行期错（数值非法、busybox 短报）。
pub const EXIT_FAIL: u32 = 1;

/// 用法错（coreutils 枚举非法）。
pub const EXIT_USAGE: u32 = 2;

/// 未名档（归 F11674，本卡只借作兜底锚）。
pub const EXIT_NOTFOUND: u32 = 127;

/// 本卡不认识的选项名（兜底档的主语）。
pub const UNKNOWN_OPT: &str = "--definitely-not-an-option";

/// coreutils 的 Try 行前缀。
pub const TRY_HEAD: &str = "Try '";

/// coreutils 的 Try 行尾巴。
pub const TRY_TAIL: &str = "--help' for more information.";

/// 枚举错的候选引导行。
pub const VALID_HEAD: &str = "Valid arguments are:";

/// C locale 的左引号。
pub const QUOTE_OPEN_C: &str = "'";

/// C locale 的右引号。
pub const QUOTE_CLOSE_C: &str = "'";

/// UTF-8 locale 的左引号。
pub const QUOTE_OPEN_U: &str = "\u{2018}";

/// UTF-8 locale 的右引号。
pub const QUOTE_CLOSE_U: &str = "\u{2019}";

// ---------------------------------------------------------------------------
// 十三码
// ---------------------------------------------------------------------------

/// 成败互报。
pub const VB_ESTATE: u32 = SEG_BASE + 0x01;

/// 退出码档错。
pub const VB_ECODE: u32 = SEG_BASE + 0x02;

/// 落点闸错。
pub const VB_ESTAGE: u32 = SEG_BASE + 0x03;

/// 行数（正文量）错。
pub const VB_EVALUE: u32 = SEG_BASE + 0x04;

/// 文本家族档错。
pub const VB_ENOTE: u32 = SEG_BASE + 0x05;

/// 兜底锚错。
pub const VB_EDEFAULT: u32 = SEG_BASE + 0x06;

/// 文案表缺格/串格。
pub const VB_EBODY: u32 = SEG_BASE + 0x07;

/// 通道错（诊断不在 stderr）。
pub const VB_ECHAN: u32 = SEG_BASE + 0x08;

/// 槽位越界。
pub const VB_ESLOT: u32 = SEG_BASE + 0x09;

/// 指纹为零。
pub const VB_ESEAL: u32 = SEG_BASE + 0x0a;

/// 无观测。
pub const VB_ENOBS: u32 = SEG_BASE + 0x0b;

/// 矩阵不齐。
pub const VB_EMATRIX: u32 = SEG_BASE + 0x0c;

/// 账目不平。
pub const VB_EBAL: u32 = SEG_BASE + 0x0d;

/// 十三码全集（顺序即偏移）。
pub const ALL_CODES: [u32; 13] = [
    VB_ESTATE,
    VB_ECODE,
    VB_ESTAGE,
    VB_EVALUE,
    VB_ENOTE,
    VB_EDEFAULT,
    VB_EBODY,
    VB_ECHAN,
    VB_ESLOT,
    VB_ESEAL,
    VB_ENOBS,
    VB_EMATRIX,
    VB_EBAL,
];

// ---------------------------------------------------------------------------
// 算子号（台账格用）
// ---------------------------------------------------------------------------

/// 算子：闸 1 dispatch。
pub const OP_DISPATCH: u8 = 0x01;

/// 算子：闸 2 option。
pub const OP_OPTION: u8 = 0x02;

/// 算子：闸 3 value。
pub const OP_VALUE: u8 = 0x03;

/// 算子：闸 4 message。
pub const OP_MESSAGE: u8 = 0x04;

/// 算子：闸 5 channel。
pub const OP_CHANNEL: u8 = 0x05;

/// 算子：闸 6 code。
pub const OP_CODE: u8 = 0x06;

/// 算子：闸 7 exit。
pub const OP_EXIT: u8 = 0x07;

/// 七个统计格各自对应的算子（顺序即闸序）。
pub const STAT_OPS: [u8; 7] = [
    OP_DISPATCH,
    OP_OPTION,
    OP_VALUE,
    OP_MESSAGE,
    OP_CHANNEL,
    OP_CODE,
    OP_EXIT,
];

/// 红档统计格的算子号。
pub const OPID_STAT_RED: u8 = 0x08;

/// 缺陷码格算子的基号（六个缺陷格依次 +0..+5）。
pub const DEFECT_OP_BASE: u8 = 0x21;

// ---------------------------------------------------------------------------
// FNV-1a 折叠
// ---------------------------------------------------------------------------

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 折一个字。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut out = h;
    let mut i = 0u32;
    while i < 8 {
        let shift = i * 8;
        let b = ((w >> shift) & 0xff) as u8;
        out = fold_byte(out, b);
        i += 1;
    }
    out
}

/// 折一个 i32（先取补码再折字，负号不会塌成同一条）。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as u32 as u64)
}

/// 折一个布尔。
fn fold_bool(h: u64, b: bool) -> u64 {
    fold_byte(h, if b { 1u8 } else { 0u8 })
}

/// 折一串文本（逐字节）。
fn fold_str(h: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut out = h;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            out = fold_byte(out, *c);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 文本助手
// ---------------------------------------------------------------------------

/// u32 → 十进制文本（不递归）。
fn dec_text(mut v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = 10usize;
    loop {
        let d = (v % 10) as u8;
        n = n.saturating_sub(1);
        if let Some(slot) = buf.get_mut(n) {
            *slot = b'0' + d;
        }
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let mut s = String::new();
    let mut i = n;
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// u32 → 固定宽度小写十六进制。
fn hex_text(v: u32, width: usize) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    let mut i = width;
    while i > 0 {
        i = i.saturating_sub(1);
        let shift = (i * 4) as u32;
        if shift >= 32 {
            s.push('0');
            continue;
        }
        let nib = (v >> shift) & 0x0f;
        if let Some(c) = DIGITS.get(nib as usize) {
            s.push(*c as char);
        }
    }
    s
}

/// 段址文本（六位十六进制）。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 6).as_str());
    s
}

/// 无符号数文本对外口。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 有符号数文本（带负号）。
fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text(v.unsigned_abs()).as_str());
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

/// 千分率文本。
pub fn permille_text(v: u32) -> String {
    let mut s = dec_text(v);
    s.push_str("permille");
    s
}

/// 文本行数（按 `\n` 计）。
fn lines_of(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut n = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'\n' {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 文本字节数。
fn bytes_of(s: &str) -> u32 {
    s.as_bytes().len() as u32
}

// ---------------------------------------------------------------------------
// 域模型
// ---------------------------------------------------------------------------

/// 本卡管辖的选项（argv 里那个"认了、值没认"的开关）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    /// `head -n/--lines`：数值型，单位是行。
    HeadN,
    /// `tail -n/--lines`：数值型，单位是行。
    TailN,
    /// `ls --color`：枚举型，三组九值。
    LsColor,
    /// `ls --sort`：枚举型，五值。
    LsSort,
    /// `cp --reflink`：枚举型，三值。
    CpReflink,
    /// 不在本卡的选项名（兜底档主语）。
    Unknown,
}

impl Opt {
    /// 选项短名。
    pub fn name(self) -> &'static str {
        match self {
            Opt::HeadN => "head -n",
            Opt::TailN => "tail -n",
            Opt::LsColor => "ls --color",
            Opt::LsSort => "ls --sort",
            Opt::CpReflink => "cp --reflink",
            Opt::Unknown => UNKNOWN_OPT,
        }
    }

    /// 序号（查表用）。
    pub const fn index(self) -> usize {
        match self {
            Opt::HeadN => 0usize,
            Opt::TailN => 1usize,
            Opt::LsColor => 2usize,
            Opt::LsSort => 3usize,
            Opt::CpReflink => 4usize,
            Opt::Unknown => 5usize,
        }
    }

    /// 按序号取选项；越界给 `None`。
    pub fn at(i: usize) -> Option<Opt> {
        match i {
            0usize => Some(Opt::HeadN),
            1usize => Some(Opt::TailN),
            2usize => Some(Opt::LsColor),
            3usize => Some(Opt::LsSort),
            4usize => Some(Opt::CpReflink),
            5usize => Some(Opt::Unknown),
            _ => None,
        }
    }

    /// 全集（顺序=序号）。
    pub fn all() -> [Opt; OPT_COUNT] {
        [
            Opt::HeadN,
            Opt::TailN,
            Opt::LsColor,
            Opt::LsSort,
            Opt::CpReflink,
            Opt::Unknown,
        ]
    }

    /// 是否进本卡表（未名选项不进）。
    pub fn judged(self) -> bool {
        matches!(
            self,
            Opt::HeadN | Opt::TailN | Opt::LsColor | Opt::LsSort | Opt::CpReflink
        )
    }

    /// 主语程序名（错误行第一个词）。
    pub fn bin(self) -> &'static str {
        match self {
            Opt::HeadN => "head",
            Opt::TailN => "tail",
            Opt::LsColor => "ls",
            Opt::LsSort => "ls",
            Opt::CpReflink => "cp",
            Opt::Unknown => "varix",
        }
    }

    /// 长选项写法（枚举错文案里被引号括住的那个词）。
    pub fn long(self) -> &'static str {
        match self {
            Opt::HeadN => "--lines",
            Opt::TailN => "--lines",
            Opt::LsColor => "--color",
            Opt::LsSort => "--sort",
            Opt::CpReflink => "--reflink",
            Opt::Unknown => UNKNOWN_OPT,
        }
    }

    /// 值的种类。
    pub fn kind(self) -> Kind {
        match self {
            Opt::HeadN | Opt::TailN => Kind::Numeric,
            Opt::LsColor | Opt::LsSort | Opt::CpReflink => Kind::Enum,
            Opt::Unknown => Kind::Numeric,
        }
    }

    /// 数值型的"量词"（`invalid number of lines` 里的 lines）。
    pub fn unit(self) -> &'static str {
        match self {
            Opt::HeadN | Opt::TailN => "lines",
            _ => "arguments",
        }
    }
}

/// 值的种类：这一格该按数值解析还是按枚举比表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// 十进制整数。
    Numeric,
    /// 有限集合里的一个词。
    Enum,
}

impl Kind {
    /// 种类名。
    pub fn name(self) -> &'static str {
        match self {
            Kind::Numeric => "numeric",
            Kind::Enum => "enum",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Kind::Numeric => 0usize,
            Kind::Enum => 1usize,
        }
    }

    /// 全集。
    pub fn all() -> [Kind; KIND_COUNT] {
        [Kind::Numeric, Kind::Enum]
    }

    /// 这一类是否按整串解析判合法。
    pub fn parses(self) -> bool {
        matches!(self, Kind::Numeric)
    }
}

/// 文本家族：同一个错在三条线上怎么说话。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// coreutils 8.32，C locale（ASCII 直角引号）。
    Core,
    /// coreutils 8.32，UTF-8 locale（弯引号）。
    CoreUtf8,
    /// BusyBox 1.36.1（短报，无 Try 行，无候选表）。
    Busybox,
}

impl Family {
    /// 家族名。
    pub fn name(self) -> &'static str {
        match self {
            Family::Core => "core",
            Family::CoreUtf8 => "core-utf8",
            Family::Busybox => "busybox",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Family::Core => 0usize,
            Family::CoreUtf8 => 1usize,
            Family::Busybox => 2usize,
        }
    }

    /// 全集。
    pub fn all() -> [Family; FAMILY_COUNT] {
        [Family::Core, Family::CoreUtf8, Family::Busybox]
    }

    /// 这条线是不是 coreutils（含 UTF-8 变体）。
    pub fn is_core(self) -> bool {
        matches!(self, Family::Core | Family::CoreUtf8)
    }

    /// 这条线用不用弯引号。
    pub fn curly(self) -> bool {
        matches!(self, Family::CoreUtf8)
    }

    /// 这条线认不认枚举选项（busybox 的 `--color/--sort/--reflink` 没有值层）。
    pub fn enum_capable(self) -> bool {
        self.is_core()
    }

    /// 这条线的数值错有几行（coreutils 两行，busybox 一行）。
    pub fn numeric_lines(self) -> u32 {
        if self.is_core() {
            2u32
        } else {
            1u32
        }
    }
}

/// 输出通道：诊断只许走 stderr。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    /// 标准错误（唯一合法通道）。
    Stderr,
    /// 标准输出（诊断跑这里就是通道误读）。
    Stdout,
    /// 一个字都没出。
    Nowhere,
}

impl Channel {
    /// 通道名。
    pub fn name(self) -> &'static str {
        match self {
            Channel::Stderr => "stderr",
            Channel::Stdout => "stdout",
            Channel::Nowhere => "nowhere",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Channel::Stderr => 0usize,
            Channel::Stdout => 1usize,
            Channel::Nowhere => 2usize,
        }
    }

    /// 全集。
    pub fn all() -> [Channel; 3] {
        [Channel::Stderr, Channel::Stdout, Channel::Nowhere]
    }

    /// 这条通道是否承载诊断字节。
    pub fn carries_bytes(self) -> bool {
        !matches!(self, Channel::Nowhere)
    }

    /// 是否诊断该走的通道。
    pub fn is_diag(self) -> bool {
        matches!(self, Channel::Stderr)
    }
}

/// 阶段：本卡七道闸，秩从 1 起。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 闸 1：选项定名与管辖。
    Dispatch,
    /// 闸 2：文本线上有没有这个选项。
    Option,
    /// 闸 3：值本身合不合法。
    Value,
    /// 闸 4：文案取格与截断复核。
    Message,
    /// 闸 5：通道判定。
    Channel,
    /// 闸 6：码档结算。
    Code,
    /// 闸 7：注解档与封账。
    Exit,
}

impl Stage {
    /// 秩（1..7）。
    pub const fn rank(self) -> u32 {
        match self {
            Stage::Dispatch => 1u32,
            Stage::Option => 2u32,
            Stage::Value => 3u32,
            Stage::Message => 4u32,
            Stage::Channel => 5u32,
            Stage::Code => 6u32,
            Stage::Exit => 7u32,
        }
    }

    /// 小写阶段名。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Dispatch => "dispatch",
            Stage::Option => "option",
            Stage::Value => "value",
            Stage::Message => "message",
            Stage::Channel => "channel",
            Stage::Code => "code",
            Stage::Exit => "exit",
        }
    }

    /// 按秩取阶段；越界给 `None`。
    pub fn from_rank(r: u32) -> Option<Stage> {
        match r {
            1u32 => Some(Stage::Dispatch),
            2u32 => Some(Stage::Option),
            3u32 => Some(Stage::Value),
            4u32 => Some(Stage::Message),
            5u32 => Some(Stage::Channel),
            6u32 => Some(Stage::Code),
            7u32 => Some(Stage::Exit),
            _ => None,
        }
    }

    /// 全集（按闸序）。
    pub fn all() -> [Stage; 7] {
        [
            Stage::Dispatch,
            Stage::Option,
            Stage::Value,
            Stage::Message,
            Stage::Channel,
            Stage::Code,
            Stage::Exit,
        ]
    }

    /// 这一闸是否还没取到文案。
    pub fn before_text(self) -> bool {
        matches!(self, Stage::Dispatch | Stage::Option | Stage::Value)
    }

    /// 这一闸是否已经给出路点闸。
    pub fn settled_gate(self) -> bool {
        !matches!(self, Stage::Exit)
    }
}

/// 注解档：这一格该长成什么样（文案家族 + 有无正文）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// coreutils 数值错：`invalid number of lines` + Try 行（两行）。
    InvalidNumber,
    /// coreutils 数值错，UTF-8 引号。
    InvalidNumberUtf8,
    /// busybox 数值短报：`invalid number`（一行）。
    InvalidNumberBb,
    /// coreutils 枚举错：候选表（`--color` 三条 / `--sort` 五条 / `--reflink` 三条）。
    InvalidEnum,
    /// coreutils 枚举错，UTF-8 引号。
    InvalidEnumUtf8,
    /// 这条文本线上没有这个选项（busybox 的枚举选项），拒收。
    NoSuchOption,
    /// 值本身合法：不是本卡的现场，兜底档。
    ValueAccepted,
    /// 采集只留首行。
    Truncated,
    /// 诊断跑到 stdout。
    Misrouted,
    /// 文案表缺格，一字不出。
    Silent,
    /// 选项不归本卡，兜底档。
    Fallback,
}

impl Note {
    /// 档名（明细行逐字出现）。
    pub fn name(self) -> &'static str {
        match self {
            Note::InvalidNumber => "invalid-number",
            Note::InvalidNumberUtf8 => "invalid-number-utf8",
            Note::InvalidNumberBb => "invalid-number-bb",
            Note::InvalidEnum => "invalid-enum",
            Note::InvalidEnumUtf8 => "invalid-enum-utf8",
            Note::NoSuchOption => "no-such-option",
            Note::ValueAccepted => "value-accepted",
            Note::Truncated => "truncated",
            Note::Misrouted => "misrouted",
            Note::Silent => "silent",
            Note::Fallback => "fallback",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Note::InvalidNumber => 0usize,
            Note::InvalidNumberUtf8 => 1usize,
            Note::InvalidNumberBb => 2usize,
            Note::InvalidEnum => 3usize,
            Note::InvalidEnumUtf8 => 4usize,
            Note::NoSuchOption => 5usize,
            Note::ValueAccepted => 6usize,
            Note::Truncated => 7usize,
            Note::Misrouted => 8usize,
            Note::Silent => 9usize,
            Note::Fallback => 10usize,
        }
    }

    /// 全集。
    pub fn all() -> [Note; Note::COUNT] {
        [
            Note::InvalidNumber,
            Note::InvalidNumberUtf8,
            Note::InvalidNumberBb,
            Note::InvalidEnum,
            Note::InvalidEnumUtf8,
            Note::NoSuchOption,
            Note::ValueAccepted,
            Note::Truncated,
            Note::Misrouted,
            Note::Silent,
            Note::Fallback,
        ]
    }

    /// 档数。
    pub const COUNT: usize = 11;

    /// 兜底位：这一格**没有结论**。
    pub fn defaulted(self) -> bool {
        matches!(self, Note::ValueAccepted | Note::Fallback)
    }

    /// 这一格有没有可对账的诊断正文。
    pub fn has_body(self) -> bool {
        matches!(
            self,
            Note::InvalidNumber
                | Note::InvalidNumberUtf8
                | Note::InvalidNumberBb
                | Note::InvalidEnum
                | Note::InvalidEnumUtf8
        )
    }

    /// 这一格说不说法（红档面）。
    pub fn speaks_error(self) -> bool {
        matches!(
            self,
            Note::InvalidNumber
                | Note::InvalidNumberUtf8
                | Note::InvalidNumberBb
                | Note::InvalidEnum
                | Note::InvalidEnumUtf8
                | Note::NoSuchOption
                | Note::Truncated
                | Note::Misrouted
        )
    }

    /// 这一格落 coreutils 线。
    pub fn is_core(self) -> bool {
        matches!(
            self,
            Note::InvalidNumber | Note::InvalidNumberUtf8 | Note::InvalidEnum | Note::InvalidEnumUtf8
        )
    }

    /// 这一格落 busybox 线。
    pub fn is_busybox(self) -> bool {
        matches!(self, Note::InvalidNumberBb)
    }

    /// 这一格的正文是不是弯引号版。
    pub fn curly(self) -> bool {
        matches!(self, Note::InvalidNumberUtf8 | Note::InvalidEnumUtf8)
    }

    /// 这一格该不该附 Try 行。
    pub fn needs_try(self) -> bool {
        self.is_core()
    }
}

/// 四档：按**实际结论**归档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一档：coreutils 逐字文案（含命令行）。
    Verbatim,
    /// 二档：coreutils 的 UTF-8 引号变体（行数相同，字节不同）。
    Bannered,
    /// 三档：busybox 单行短报（无 Try 行、无候选表）。
    NoBody,
    /// 四档：拒收或兜底（没有可对账的正文）。
    Refused,
}

impl Tier {
    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            Tier::Verbatim => "verbatim",
            Tier::Bannered => "bannered",
            Tier::NoBody => "nobody",
            Tier::Refused => "refused",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Tier::Verbatim => 0usize,
            Tier::Bannered => 1usize,
            Tier::NoBody => 2usize,
            Tier::Refused => 3usize,
        }
    }

    /// 全表。
    pub fn all() -> [Tier; TIER_COUNT] {
        [Tier::Verbatim, Tier::Bannered, Tier::NoBody, Tier::Refused]
    }

    /// 该档是否算"有完整正文可对账"。
    pub fn has_text(self) -> bool {
        matches!(self, Tier::Verbatim | Tier::Bannered)
    }

    /// 由一次结果归档；四档互斥且全覆盖。
    pub fn of(o: &Outcome) -> Tier {
        if !o.consistent || !o.note.has_body() {
            return Tier::Refused;
        }
        if matches!(
            o.note,
            Note::InvalidNumberBb
        ) {
            return Tier::NoBody;
        }
        if o.note.curly() {
            return Tier::Bannered;
        }
        Tier::Verbatim
    }
}

/// 缺陷维：判序 Estate → Stage → Code → Value → Note → Default → Ok。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 全对。
    Ok,
    /// 成败互报。
    Estate,
    /// 落点闸错。
    Stage,
    /// 退出码档错。
    Code,
    /// 正文行数错。
    Value,
    /// 文本家族档错。
    Note,
    /// 兜底锚错。
    Default,
}

impl Fault {
    /// 本维错误码（`Ok` 给 0）。
    pub const fn code(self) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => VB_ESTATE,
            Fault::Stage => VB_ESTAGE,
            Fault::Code => VB_ECODE,
            Fault::Value => VB_EVALUE,
            Fault::Note => VB_ENOTE,
            Fault::Default => VB_EDEFAULT,
        }
    }

    /// 维名。
    pub fn name(self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Estate => "estate",
            Fault::Stage => "stage",
            Fault::Code => "code",
            Fault::Value => "value",
            Fault::Note => "note",
            Fault::Default => "default",
        }
    }

    /// 判序位次（`Ok` 排最后）。
    pub const fn order(self) -> usize {
        match self {
            Fault::Estate => 0usize,
            Fault::Stage => 1usize,
            Fault::Code => 2usize,
            Fault::Value => 3usize,
            Fault::Note => 4usize,
            Fault::Default => 5usize,
            Fault::Ok => 6usize,
        }
    }
}

/// 短名（排障行用）。
pub fn fault_short(f: Fault) -> &'static str {
    match f {
        Fault::Ok => "OK",
        Fault::Estate => "ESTATE",
        Fault::Stage => "ESTAGE",
        Fault::Code => "ECODE",
        Fault::Value => "EVALUE",
        Fault::Note => "ENOTE",
        Fault::Default => "EDEFAUL",
    }
}

/// 判序全集（六维顺序固定）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 六个缺陷码（与判序一一对应，两两互异）。
pub const REJECT_CODES: [u32; REJECT] = [
    VB_ESTATE,
    VB_ESTAGE,
    VB_ECODE,
    VB_EVALUE,
    VB_ENOTE,
    VB_EDEFAULT,
];

// ---------------------------------------------------------------------------
// 合法值集合与解析
// ---------------------------------------------------------------------------

/// `ls --color` 的九个合法值（三组三值）。
pub const COLOR_OK: [&str; 9] = [
    "always", "yes", "force", "never", "no", "none", "auto", "tty", "if-tty",
];

/// `ls --sort` 的五个合法值。
pub const SORT_OK: [&str; 5] = ["none", "time", "size", "extension", "version"];

/// `cp --reflink` 的三个合法值。
pub const REFLINK_OK: [&str; 3] = ["always", "auto", "never"];

/// 在集合里找一个词（逐格扫，不用下标）。
fn in_set(set: &[&str], v: &str) -> bool {
    let mut i = 0usize;
    while i < set.len() {
        if let Some(x) = set.get(i) {
            if *x == v {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 这个值是这一格枚举选项的合法值吗。
pub fn enum_accepts(o: Opt, v: &str) -> bool {
    match o {
        Opt::LsColor => in_set(&COLOR_OK, v),
        Opt::LsSort => in_set(&SORT_OK, v),
        Opt::CpReflink => in_set(&REFLINK_OK, v),
        _ => false,
    }
}

/// 十进制整串解析：空串、带尾缀、溢出都不算数。
pub fn parse_num(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(x) => *x,
            None => return None,
        };
        if c < b'0' || c > b'9' {
            return None;
        }
        let d = (c - b'0') as u32;
        v = match v.checked_mul(10).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return None,
        };
        i += 1;
    }
    Some(v)
}

// ---------------------------------------------------------------------------
// 文案表：五个选项 × 它能落的那几条文本线，逐字快照
// ---------------------------------------------------------------------------

/// 一格文案快照：模板（`{V}` 是值的位置）+ 该格的退出码。
#[derive(Clone, Copy)]
pub struct MsgCell {
    /// 哪个选项。
    pub opt: Opt,
    /// 哪条文本线。
    pub family: Family,
    /// 模板（`{V}` 待填）。
    pub tpl: &'static str,
    /// 这一格的退出码。
    pub code: u32,
}

/// 兜底格（取不到时用来判错的空快照，绝不冒充真快照）。
pub const CELL_NONE: MsgCell = MsgCell {
    opt: Opt::Unknown,
    family: Family::Busybox,
    tpl: "",
    code: EXIT_FAIL,
};

/// 十二格文案表。
pub const MSG_TABLE: [MsgCell; MESSAGE_CELLS] = [
    MsgCell {
        opt: Opt::HeadN,
        family: Family::Core,
        tpl: "head: invalid number of lines: '{V}'\nTry 'head --help' for more information.\n",
        code: EXIT_FAIL,
    },
    MsgCell {
        opt: Opt::HeadN,
        family: Family::CoreUtf8,
        tpl: "head: invalid number of lines: ‘{V}’\nTry ‘head --help’ for more information.\n",
        code: EXIT_FAIL,
    },
    MsgCell {
        opt: Opt::HeadN,
        family: Family::Busybox,
        tpl: "head: invalid number: '{V}'\n",
        code: EXIT_FAIL,
    },
    MsgCell {
        opt: Opt::TailN,
        family: Family::Core,
        tpl: "tail: invalid number of lines: '{V}'\nTry 'tail --help' for more information.\n",
        code: EXIT_FAIL,
    },
    MsgCell {
        opt: Opt::TailN,
        family: Family::CoreUtf8,
        tpl: "tail: invalid number of lines: ‘{V}’\nTry ‘tail --help’ for more information.\n",
        code: EXIT_FAIL,
    },
    MsgCell {
        opt: Opt::TailN,
        family: Family::Busybox,
        tpl: "tail: invalid number: '{V}'\n",
        code: EXIT_FAIL,
    },
    MsgCell {
        opt: Opt::LsColor,
        family: Family::Core,
        tpl: "ls: invalid argument '{V}' for '--color'\n\
Valid arguments are:\n\
\x20 - 'always', 'yes', 'force'\n\
\x20 - 'never', 'no', 'none'\n\
\x20 - 'auto', 'tty', 'if-tty'\n\
Try 'ls --help' for more information.\n",
        code: EXIT_USAGE,
    },
    MsgCell {
        opt: Opt::LsColor,
        family: Family::CoreUtf8,
        tpl: "ls: invalid argument ‘{V}’ for ‘--color’\n\
Valid arguments are:\n\
\x20 - ‘always’, ‘yes’, ‘force’\n\
\x20 - ‘never’, ‘no’, ‘none’\n\
\x20 - ‘auto’, ‘tty’, ‘if-tty’\n\
Try ‘ls --help’ for more information.\n",
        code: EXIT_USAGE,
    },
    MsgCell {
        opt: Opt::LsSort,
        family: Family::Core,
        tpl: "ls: invalid argument '{V}' for '--sort'\n\
Valid arguments are:\n\
\x20 - 'none'\n\
\x20 - 'time'\n\
\x20 - 'size'\n\
\x20 - 'extension'\n\
\x20 - 'version'\n\
Try 'ls --help' for more information.\n",
        code: EXIT_USAGE,
    },
    MsgCell {
        opt: Opt::LsSort,
        family: Family::CoreUtf8,
        tpl: "ls: invalid argument ‘{V}’ for ‘--sort’\n\
Valid arguments are:\n\
\x20 - ‘none’\n\
\x20 - ‘time’\n\
\x20 - ‘size’\n\
\x20 - ‘extension’\n\
\x20 - ‘version’\n\
Try ‘ls --help’ for more information.\n",
        code: EXIT_USAGE,
    },
    MsgCell {
        opt: Opt::CpReflink,
        family: Family::Core,
        tpl: "cp: invalid argument '{V}' for '--reflink'\n\
Valid arguments are:\n\
\x20 - 'always'\n\
\x20 - 'auto'\n\
\x20 - 'never'\n\
Try 'cp --help' for more information.\n",
        code: EXIT_USAGE,
    },
    MsgCell {
        opt: Opt::CpReflink,
        family: Family::CoreUtf8,
        tpl: "cp: invalid argument ‘{V}’ for ‘--reflink’\n\
Valid arguments are:\n\
\x20 - ‘always’\n\
\x20 - ‘auto’\n\
\x20 - ‘never’\n\
Try ‘cp --help’ for more information.\n",
        code: EXIT_USAGE,
    },
];

/// 取一格文案快照（逐格扫表，不用下标）。
pub fn msg_cell(o: Opt, f: Family) -> Option<&'static MsgCell> {
    let mut i = 0usize;
    while i < MESSAGE_CELLS {
        if let Some(c) = MSG_TABLE.get(i) {
            if c.opt == o && c.family == f {
                return Some(c);
            }
        }
        i += 1;
    }
    None
}

/// 把模板里的 `{V}` 换成真值（逐段拼接，不碰字符串下标）。
pub fn subst(tpl: &str, val: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    for part in tpl.split("{V}") {
        if !first {
            out.push_str(val);
        }
        out.push_str(part);
        first = false;
    }
    out
}

/// 这一格的实际文案（取不到格给空串）。
pub fn msg_text(o: Opt, f: Family, val: &str) -> String {
    match msg_cell(o, f) {
        Some(c) => subst(c.tpl, val),
        None => String::new(),
    }
}

/// 这一格的退出码（取不到格给 1）。
pub fn msg_code(o: Opt, f: Family) -> u32 {
    match msg_cell(o, f) {
        Some(c) => c.code,
        None => EXIT_FAIL,
    }
}

/// 选项的词形（不带破折号，busybox 的 unrecognized 行用它）。
pub fn opt_word(o: Opt) -> &'static str {
    match o {
        Opt::HeadN => "lines",
        Opt::TailN => "lines",
        Opt::LsColor => "color",
        Opt::LsSort => "sort",
        Opt::CpReflink => "reflink",
        Opt::Unknown => "unknown",
    }
}

/// 这条线上没有这个选项时的那一行（拒收档的文案，逐字归 F11669）。
pub fn msg_nosuch(o: Opt) -> String {
    let mut s = String::from(o.bin());
    s.push_str(": unrecognized option: ");
    s.push_str(opt_word(o));
    s.push('\n');
    s
}

/// 只留首行（截断档的现场形态）。
fn first_line_only(s: &str) -> String {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut cut = b.len();
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'\n' {
                cut = i;
                break;
            }
        }
        i += 1;
    }
    let mut out = String::new();
    let mut k = 0usize;
    while k < cut {
        if let Some(c) = s.get(k..k + 1) {
            out.push_str(c);
        }
        k += 1;
    }
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// Shape：一次"值没认"的现场能读到的全部维度
// ---------------------------------------------------------------------------

/// 一格输入形状。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 哪个选项。
    pub opt: Opt,
    /// 哪条文本线。
    pub family: Family,
    /// 喂进去的那个值（原样，不预先清洗）。
    pub value: &'static str,
    /// 诊断落在哪条通道。
    pub channel: Channel,
    /// 采集记下的退出码。
    pub exit_declared: u32,
    /// 采集是否只留了首行。
    pub truncated: bool,
    /// locale 是不是 UTF-8（与家族必须对上）。
    pub lang_utf8: bool,
    /// 是不是终端（只影响 `--color=tty` 之类的合法值，本卡只记不解）。
    pub tty: bool,
}

impl Shape {
    /// 基型：`head -n abc`，coreutils，stderr，退 1。
    pub const fn blank() -> Shape {
        Shape {
            opt: Opt::HeadN,
            family: Family::Core,
            value: "abc",
            channel: Channel::Stderr,
            exit_declared: EXIT_FAIL,
            truncated: false,
            lang_utf8: false,
            tty: false,
        }
    }

    /// 形状整体是否自洽。不自洽只把 `consistent` 压假，不改变落点。
    pub fn valid(&self) -> bool {
        if self.exit_declared > EXIT_NOTFOUND {
            return false;
        }
        // UTF-8 家族必须真的在 UTF-8 locale；C 家族必须不在。
        if self.family.curly() != self.lang_utf8 {
            return false;
        }
        // 截断只可能是 coreutils 的两行以上文案被切断。
        if self.truncated && !self.family.is_core() {
            return false;
        }
        // 枚举选项在 busybox 上连值层都没有，不许声称"采集到了值错文案"。
        if self.opt.kind() == Kind::Enum && !self.family.enum_capable() {
            return false;
        }
        true
    }

    /// 只换选项。
    pub fn with_opt(&self, o: Opt) -> Shape {
        let mut s = *self;
        s.opt = o;
        s
    }

    /// 只换文本线。
    pub fn with_family(&self, f: Family) -> Shape {
        let mut s = *self;
        s.family = f;
        s
    }

    /// 只换值。
    pub fn with_value(&self, v: &'static str) -> Shape {
        let mut s = *self;
        s.value = v;
        s
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.opt.index() as u64);
        h = fold_word(h, self.family.index() as u64);
        h = fold_str(h, self.value);
        h = fold_word(h, self.channel.index() as u64);
        h = fold_word(h, self.exit_declared as u64);
        h = fold_bool(h, self.truncated);
        h = fold_bool(h, self.lang_utf8);
        fold_bool(h, self.tty)
    }
}

/// 一次现场的实际结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 落点闸。
    pub stage: Stage,
    /// 退出码。
    pub code: i32,
    /// 诊断正文行数（无正文为 0）。
    pub value: u32,
    /// 文本家族档。
    pub note: Note,
    /// 兜底位：这一格没有可对账的结论。
    pub default: bool,
    /// 结构自洽：谎报的形状在这里被压假。
    pub consistent: bool,
}

impl Outcome {
    /// 全零初值（取不到任何现场时的形态）。
    pub const fn blank() -> Outcome {
        Outcome {
            stage: Stage::Exit,
            code: 0i32,
            value: 0u32,
            note: Note::Silent,
            default: true,
            consistent: true,
        }
    }

    /// 构造一格。
    pub const fn make(
        stage: Stage,
        code: u32,
        value: u32,
        note: Note,
        default: bool,
        consistent: bool,
    ) -> Outcome {
        Outcome {
            stage,
            code: code as i32,
            value,
            note,
            default,
            consistent,
        }
    }

    /// 是否绿（比 `code == 0` 更严：不自洽一律不绿）。
    pub fn ok(&self) -> bool {
        self.code == 0 && self.consistent
    }

    /// 是否红。
    pub fn failed(&self) -> bool {
        self.code != 0
    }

    /// 是否落进兜底档。
    pub fn anchored(&self) -> bool {
        self.default
    }

    /// 是否拒收。
    pub fn refused(&self) -> bool {
        !self.consistent
    }

    /// 应然退出码。
    pub fn exit(&self) -> u32 {
        if self.ok() {
            return EXIT_OK;
        }
        if self.code < 0 {
            return EXIT_USAGE;
        }
        self.code as u32
    }

    /// 诊断是否成立（有正文 + 码非零 + 自洽）。
    pub fn diag_ok(&self) -> bool {
        self.consistent && self.code != 0 && self.note.has_body() && self.value > 0
    }

    /// 结论指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_i32(h, self.code);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        h = fold_bool(h, self.default);
        fold_bool(h, self.consistent)
    }
}

/// 申报（读到的那一段账；与 `Outcome` 同构但没有自洽权）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 申报读成绿还是红。
    pub success: bool,
    /// 申报落在哪道闸。
    pub stage: Stage,
    /// 申报的退出码。
    pub code: i32,
    /// 申报的正文行数。
    pub value: u32,
    /// 申报的文本家族档。
    pub note: Note,
    /// 申报的兜底位。
    pub default: bool,
}

impl Claim {
    /// 全零初值。
    pub const fn blank() -> Claim {
        Claim {
            success: false,
            stage: Stage::Dispatch,
            code: 0i32,
            value: 0u32,
            note: Note::Silent,
            default: false,
        }
    }

    /// 构造一格。
    pub const fn make(
        success: bool,
        stage: Stage,
        code: i32,
        value: u32,
        note: Note,
        default: bool,
    ) -> Claim {
        Claim {
            success,
            stage,
            code,
            value,
            note,
            default,
        }
    }

    /// 申报的成败。
    pub fn success(&self) -> bool {
        self.success
    }

    /// 申报指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_bool(h, self.success);
        h = fold_word(h, self.stage.rank() as u64);
        h = fold_i32(h, self.code);
        h = fold_word(h, self.value as u64);
        h = fold_str(h, self.note.name());
        fold_bool(h, self.default)
    }
}

/// 一槽 = 形状 + 申报 + 短名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    /// 输入形状。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
    /// 短名。
    pub tag: &'static str,
}

impl Case {
    /// 空格。
    pub const fn blank() -> Case {
        Case {
            shape: Shape::blank(),
            claim: Claim::blank(),
            tag: "f11670-blank",
        }
    }

    /// 现场真值。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 短名。
    pub fn tag(&self) -> &'static str {
        self.tag
    }

    /// 本槽的判读结果。
    pub fn fault(&self) -> Fault {
        judge(self)
    }

    /// 是否判对。
    pub fn ok(&self) -> bool {
        self.fault() == Fault::Ok
    }

    /// 本槽指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.shape.seal());
        h = fold_word(h, self.claim.seal());
        fold_str(h, self.tag)
    }
}

// ---------------------------------------------------------------------------
// 判据本体：七道闸，谁先谁后钉死
// ---------------------------------------------------------------------------

/// 由形状推出实际结论（本卡的唯一真值来源）。
pub fn define(s: &Shape) -> Outcome {
    let mut consistent = s.valid();

    // --- 闸 1：dispatch ----------------------------------------------------
    if !s.opt.judged() {
        // 选项不归本卡：兜底档，没有可对账的正文。
        return Outcome::make(Stage::Dispatch, EXIT_FAIL, 0, Note::Fallback, true, consistent);
    }

    // --- 闸 2：option ------------------------------------------------------
    if s.opt.kind() == Kind::Enum && !s.family.enum_capable() {
        // busybox 上这个选项压根没有"值"这一层：拒收。
        let text = msg_nosuch(s.opt);
        return Outcome::make(
            Stage::Option,
            EXIT_FAIL,
            lines_of(text.as_str()),
            Note::NoSuchOption,
            false,
            false,
        );
    }

    // --- 闸 3：value -------------------------------------------------------
    let legal = match s.opt.kind() {
        Kind::Numeric => parse_num(s.value).is_some(),
        Kind::Enum => enum_accepts(s.opt, s.value),
    };
    if legal {
        // 值合法：程序照常跑，本卡没有现场可判——兜底位为真。
        return Outcome::make(Stage::Value, EXIT_OK, 0, Note::ValueAccepted, true, consistent);
    }

    // --- 闸 4：message -----------------------------------------------------
    let cell = match msg_cell(s.opt, s.family) {
        Some(c) => c,
        None => {
            return Outcome::make(Stage::Message, EXIT_FAIL, 0, Note::Silent, true, false);
        }
    };
    let text = msg_text(s.opt, s.family, s.value);
    let lines = lines_of(text.as_str());
    if s.truncated {
        let cut = first_line_only(text.as_str());
        return Outcome::make(
            Stage::Message,
            cell.code,
            lines_of(cut.as_str()),
            Note::Truncated,
            false,
            false,
        );
    }

    // --- 闸 5：channel -----------------------------------------------------
    if !s.channel.is_diag() {
        return Outcome::make(
            Stage::Channel,
            cell.code,
            lines,
            Note::Misrouted,
            false,
            false,
        );
    }

    // --- 闸 6：code --------------------------------------------------------
    if s.exit_declared != cell.code {
        consistent = false;
    }

    // --- 闸 7：exit --------------------------------------------------------
    let note = match (s.opt.kind(), s.family) {
        (Kind::Numeric, Family::Core) => Note::InvalidNumber,
        (Kind::Numeric, Family::CoreUtf8) => Note::InvalidNumberUtf8,
        (Kind::Numeric, Family::Busybox) => Note::InvalidNumberBb,
        (Kind::Enum, Family::CoreUtf8) => Note::InvalidEnumUtf8,
        (Kind::Enum, _) => Note::InvalidEnum,
    };
    Outcome::make(Stage::Exit, cell.code, lines, note, false, consistent)
}

// ---------------------------------------------------------------------------
// 语料：八活六死，逐槽手写
// ---------------------------------------------------------------------------

/// 语料现场表。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 母版锚：`head -n abc`，coreutils 两行，退 1。
    Shape {
        ..Shape::blank()
    },
    // 1 `tail -n abc`：单位词一样，程序名不同。
    Shape {
        opt: Opt::TailN,
        ..Shape::blank()
    },
    // 2 `head -n 12x`：尾缀垃圾，数值解析失败。
    Shape {
        value: "12x",
        ..Shape::blank()
    },
    // 3 `ls --color=bogus`：枚举错，候选三条，退 2。
    Shape {
        opt: Opt::LsColor,
        value: "bogus",
        exit_declared: EXIT_USAGE,
        ..Shape::blank()
    },
    // 4 `ls --sort=bogus`：候选五条，全表最长的一格。
    Shape {
        opt: Opt::LsSort,
        value: "bogus",
        exit_declared: EXIT_USAGE,
        ..Shape::blank()
    },
    // 5 `cp --reflink=bogus`：候选三条。
    Shape {
        opt: Opt::CpReflink,
        value: "bogus",
        exit_declared: EXIT_USAGE,
        ..Shape::blank()
    },
    // 6 `head -n abc`（UTF-8 locale）：弯引号，行数不变。
    Shape {
        family: Family::CoreUtf8,
        lang_utf8: true,
        ..Shape::blank()
    },
    // 7 `head -n abc`（busybox）：一行短报，没有 Try 行。
    Shape {
        family: Family::Busybox,
        ..Shape::blank()
    },
    // 8 Estate：`tail -n 12x` 短报现场（拒收），申报却把成败读反。
    Shape {
        opt: Opt::TailN,
        family: Family::Busybox,
        value: "12x",
        ..Shape::blank()
    },
    // 9 Stage：`ls --color=bogus`（UTF-8）现场，申报的落点闸说成 message。
    Shape {
        opt: Opt::LsColor,
        family: Family::CoreUtf8,
        value: "bogus",
        lang_utf8: true,
        exit_declared: EXIT_USAGE,
        ..Shape::blank()
    },
    // 10 Code：`ls --sort=bogus`（UTF-8）现场，申报的码说成 1。
    Shape {
        opt: Opt::LsSort,
        family: Family::CoreUtf8,
        value: "bogus",
        lang_utf8: true,
        exit_declared: EXIT_USAGE,
        ..Shape::blank()
    },
    // 11 Value：`cp --reflink=bogus`（UTF-8）现场，申报的正文行数说成 5。
    Shape {
        opt: Opt::CpReflink,
        family: Family::CoreUtf8,
        value: "bogus",
        lang_utf8: true,
        exit_declared: EXIT_USAGE,
        ..Shape::blank()
    },
    // 12 Note：busybox 上问 `ls --sort`（拒收），申报的注解说成数值短报。
    Shape {
        opt: Opt::LsSort,
        family: Family::Busybox,
        value: "bogus",
        ..Shape::blank()
    },
    // 13 Default：`head -n 10` 值合法（兜底），申报却说"没落兜底"。
    Shape {
        value: "10",
        exit_declared: EXIT_OK,
        ..Shape::blank()
    },
];

/// 申报表。
pub const CLAIMS: [Claim; CORPUS] = [
    // 0-2 数值两行档。
    Claim {
        ..GOOD_CLAIM
    },
    Claim {
        ..GOOD_CLAIM
    },
    Claim {
        ..GOOD_CLAIM
    },
    // 3-5 枚举档：行数按候选条数分家。
    Claim {
        code: EXIT_USAGE as i32,
        value: 6,
        note: Note::InvalidEnum,
        ..GOOD_CLAIM
    },
    Claim {
        code: EXIT_USAGE as i32,
        value: 8,
        note: Note::InvalidEnum,
        ..GOOD_CLAIM
    },
    Claim {
        code: EXIT_USAGE as i32,
        value: 6,
        note: Note::InvalidEnum,
        ..GOOD_CLAIM
    },
    // 6 UTF-8 数值档。
    Claim {
        note: Note::InvalidNumberUtf8,
        ..GOOD_CLAIM
    },
    // 7 busybox 短报档。
    Claim {
        value: 1,
        note: Note::InvalidNumberBb,
        ..GOOD_CLAIM
    },
    // 8 成败互报：一行短报被申报成"判绿"；其余五维照实现场。
    Claim {
        success: true,
        stage: Stage::Exit,
        code: EXIT_FAIL as i32,
        value: 1,
        note: Note::InvalidNumberBb,
        default: false,
    },
    // 9 阶段不符：落点是 exit，申报说成 message；其余五维照实现场。
    Claim {
        success: false,
        stage: Stage::Message,
        code: EXIT_USAGE as i32,
        value: 6,
        note: Note::InvalidEnumUtf8,
        default: false,
    },
    // 10 码档不符：退 2 的用法错被申报成退 1；其余五维照实现场。
    Claim {
        success: false,
        stage: Stage::Exit,
        code: EXIT_FAIL as i32,
        value: 8,
        note: Note::InvalidEnumUtf8,
        default: false,
    },
    // 11 数值不符：六行文案被申报成五行；其余五维照实现场。
    Claim {
        success: false,
        stage: Stage::Exit,
        code: EXIT_USAGE as i32,
        value: 5,
        note: Note::InvalidEnumUtf8,
        default: false,
    },
    // 12 注解不符：拒收现场被申报成 busybox 数值短报；其余五维照实现场。
    Claim {
        success: false,
        stage: Stage::Option,
        code: EXIT_FAIL as i32,
        value: 1,
        note: Note::InvalidNumberBb,
        default: false,
    },
    // 13 兜底不符：值合法（兜底）被申报成"没落兜底"；其余五维照实现场。
    Claim {
        success: true,
        stage: Stage::Value,
        code: EXIT_OK as i32,
        value: 0,
        note: Note::ValueAccepted,
        default: false,
    },
];

/// 母版申报：coreutils 数值错两行、退 1，判绿。
pub const GOOD_CLAIM: Claim = Claim {
    success: false,
    stage: Stage::Exit,
    code: EXIT_FAIL as i32,
    value: 2,
    note: Note::InvalidNumber,
    default: false,
};

// ---------------------------------------------------------------------------
// 期望表（申报表的第二种形式，任一处笔误都要露出来）
// ---------------------------------------------------------------------------

/// 期望阶段（现场**真实**落到的闸）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Exit,
    Stage::Option,
    Stage::Value,
];

/// 期望退出码。
pub const WANT_CODE: [u32; CORPUS] = [
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_USAGE,
    EXIT_USAGE,
    EXIT_USAGE,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_FAIL,
    EXIT_USAGE,
    EXIT_USAGE,
    EXIT_USAGE,
    EXIT_FAIL,
    EXIT_OK,
];

/// 期望正文行数。
pub const WANT_VALUE: [u32; CORPUS] = [2, 2, 2, 6, 8, 6, 2, 1, 1, 6, 8, 6, 1, 0];

/// 期望注解。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::InvalidNumber,
    Note::InvalidNumber,
    Note::InvalidNumber,
    Note::InvalidEnum,
    Note::InvalidEnum,
    Note::InvalidEnum,
    Note::InvalidNumberUtf8,
    Note::InvalidNumberBb,
    Note::InvalidNumberBb,
    Note::InvalidEnumUtf8,
    Note::InvalidEnumUtf8,
    Note::InvalidEnumUtf8,
    Note::NoSuchOption,
    Note::ValueAccepted,
];

/// 期望兜底位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望缺陷维序位（0..5 六维，6 = 无缺陷）。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望成败。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false, true,
];

/// 期望短名（标签必须挂本卡号）。
pub const WANT_TAGS: [&str; CORPUS] = [
    "f11670-head-n-abc",
    "f11670-tail-n-abc",
    "f11670-head-n-12x",
    "f11670-ls-color-bogus",
    "f11670-ls-sort-bogus",
    "f11670-cp-reflink-bogus",
    "f11670-head-n-abc-utf8",
    "f11670-head-n-abc-bb",
    "f11670-estate-bb-toggle",
    "f11670-stage-skip",
    "f11670-code-shift",
    "f11670-value-drift",
    "f11670-note-flip",
    "f11670-default-legal",
];

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 序位 → 缺陷维。
pub fn fault_ndx(f: Fault) -> usize {
    f.order()
}

/// 序位 → 缺陷维（越界给 `None`）。
pub fn ndx_fault(n: usize) -> Option<Fault> {
    match n {
        0usize => Some(Fault::Estate),
        1usize => Some(Fault::Stage),
        2usize => Some(Fault::Code),
        3usize => Some(Fault::Value),
        4usize => Some(Fault::Note),
        5usize => Some(Fault::Default),
        6usize => Some(Fault::Ok),
        _ => None,
    }
}

/// 申报对判——固定次序 Estate → Stage → Code → Value → Note → Default → Ok。
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
    // 兜底双向判死：现场真落兜底，无论怎么申报都不许当结论；现场没落兜底却
    // 申报"落了"，同样是把结论塞进兜底。
    if a.default || a.default != k.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 语料取槽。
pub fn case_at(i: usize) -> Option<Case> {
    match (SHAPES.get(i), CLAIMS.get(i), WANT_TAGS.get(i)) {
        (Some(s), Some(c), Some(t)) => Some(Case {
            shape: *s,
            claim: *c,
            tag: *t,
        }),
        _ => None,
    }
}

/// 全语料。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case::blank(); CORPUS];
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

/// 覆盖校验：槽数必须落在语料范围内。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(VB_ESLOT);
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

/// 反例区第 `i` 槽期望的缺陷维（取不到按 `Ok`，绝不冒充某一维）。
fn expected_reject_ndx(i: usize) -> Fault {
    match expected(i) {
        Some(f) => f,
        None => Fault::Ok,
    }
}

// ---------------------------------------------------------------------------
// 四档矩阵
// ---------------------------------------------------------------------------

/// 一档的统计行：`total` 与 `good` 两维独立。
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
        self.total.saturating_sub(self.good)
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
/// 与"帮助卡"的差别：本卡的反例本来就落在**有正文的档**上（UTF-8 引号变体是
/// 真文案），所以反例判对的条件是"落档对 + 命中它自己那一维"，不再附加"该档
/// 无正文"的限制——否则把一个整档凭空抹掉，分母对不上。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 由语料构造。
    pub fn build() -> Matrix {
        let table = Tier::all();
        let mut rows = [Row {
            tier: Tier::Verbatim,
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
                    landed == tier && c.fault() == expected_reject_ndx(i)
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

    /// 四档是否都有落点。
    pub fn covered(&self) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.total == 0 {
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
    "bad-value corpus=14 accept=8 reject=6 verbatim=6 bannered=4 nobody=2 refused=2 match=14 rate=1000permille\n";

/// 非法值批结算报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 语料分母。
    pub total: usize,
    /// 正例槽数。
    pub accept: usize,
    /// 反例槽数。
    pub reject: usize,
    /// 逐字档计数。
    pub verbatim: usize,
    /// 引号变体档计数（字段名沿用契约的 `bannered`）。
    pub bannered: usize,
    /// 单行短报档计数（字段名沿用契约的 `nobody`）。
    pub nobody: usize,
    /// 拒收档计数。
    pub refused: usize,
    /// 判对槽数。
    pub matched: usize,
}

impl Report {
    /// 一致率千分比。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched as u32 * RATE_FULL + (self.total as u32) / 2) / (self.total as u32)
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.accept == ACCEPT
            && self.reject == REJECT
            && self.total == CORPUS
            && self.tier_cells() == CORPUS as u32
            && self.matched == self.total
            && self.rate() == RATE_FULL
    }

    /// 红项计数。
    pub fn reds(&self) -> u32 {
        (self.total.saturating_sub(self.matched)) as u32
    }

    /// 闸门：全绿才放行。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(VB_ENOBS);
        }
        if self.tier_cells() != self.total as u32 {
            return Err(VB_EMATRIX);
        }
        if self.matched > self.total {
            return Err(VB_EBAL);
        }
        if self.rate() < RATE_FULL {
            return Err(VB_EBAL);
        }
        Ok(())
    }

    /// 四档格数。
    pub fn tier_cells(&self) -> u32 {
        (self.verbatim as u32) + (self.bannered as u32) + (self.nobody as u32) + (self.refused as u32)
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("bad-value corpus=");
        s.push_str(dec_text(self.accept as u32 + self.reject as u32).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.reject as u32).as_str());
        s.push_str(" verbatim=");
        s.push_str(dec_text(self.verbatim as u32).as_str());
        s.push_str(" bannered=");
        s.push_str(dec_text(self.bannered as u32).as_str());
        s.push_str(" nobody=");
        s.push_str(dec_text(self.nobody as u32).as_str());
        s.push_str(" refused=");
        s.push_str(dec_text(self.refused as u32).as_str());
        s.push_str(" match=");
        s.push_str(dec_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push_str("permille\n");
        s
    }
}

/// 空报告（未跑任何语料时的形态，绝不与全绿混淆）。
pub fn empty_report() -> Report {
    Report {
        total: 0,
        accept: 0,
        reject: 0,
        verbatim: 0,
        bannered: 0,
        nobody: 0,
        refused: 0,
        matched: 0,
    }
}

/// 由语料结算。
fn settle(over: Option<(usize, Fault)>) -> Report {
    let mut rep = Report {
        total: CORPUS,
        accept: ACCEPT,
        reject: REJECT,
        verbatim: 0,
        bannered: 0,
        nobody: 0,
        refused: 0,
        matched: 0,
    };
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return empty_report(),
        };
        match Tier::of(&c.actual()) {
            Tier::Verbatim => rep.verbatim += 1,
            Tier::Bannered => rep.bannered += 1,
            Tier::NoBody => rep.nobody += 1,
            Tier::Refused => rep.refused += 1,
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
        let want = if i < ACCEPT {
            Fault::Ok
        } else {
            expected_reject_ndx(i)
        };
        if landed == want {
            rep.matched += 1;
        }
        i += 1;
    }
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

/// 带注入的结算：越界槽给槽位码。
pub fn run_with(over: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = over {
        if slot >= CORPUS {
            return Err(VB_ESLOT);
        }
    }
    Ok(settle(over))
}

/// 报告与矩阵是否互相印证。
pub fn report_agrees_with_matrix() -> bool {
    let rep = settle(None);
    let m = Matrix::build();
    rep.total == CORPUS
        && m.slots() == CORPUS * TIER_COUNT
        && m.goods() == CORPUS
        && rep.tier_cells() == CORPUS as u32
        && m.covered()
}

/// 红档率标定：注入一维红之后分子必须掉到 13/14。
pub fn red_rate_calibrated() -> bool {
    let base = settle(None);
    let bad = match run_with(Some((2, Fault::Value))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if bad.matched >= base.matched {
        return false;
    }
    if bad.total < 1 {
        return false;
    }
    let want = (13u32 * RATE_FULL + (bad.total as u32) / 2) / (bad.total as u32);
    want == RED_RATE && RED_RATE == 929
}

/// 注入一路红，四路自检必须各指各的。
pub fn inject_four() -> [bool; 4] {
    let table_route = {
        let mut ok = true;
        let mut cells = 0usize;
        let mut i = 0usize;
        while i < OPT_JUDGED {
            let o = match Opt::at(i) {
                Some(x) => x,
                None => return [false, false, false, false],
            };
            let mut f = 0usize;
            while f < FAMILY_COUNT {
                let fam = match Family::all().get(f) {
                    Some(x) => *x,
                    None => return [false, false, false, false],
                };
                match msg_cell(o, fam) {
                    Some(c) => {
                        cells += 1;
                        let text = subst(c.tpl, "abc");
                        if lines_of(text.as_str()) == 0 {
                            ok = false;
                        }
                        if !text.contains(o.bin()) {
                            ok = false;
                        }
                        if c.code != msg_code(o, fam) {
                            ok = false;
                        }
                    }
                    None => {
                        // 只有 busybox 上的枚举选项允许缺格。
                        if !(o.kind() == Kind::Enum && !fam.enum_capable()) {
                            ok = false;
                        }
                    }
                }
                f += 1;
            }
            i += 1;
        }
        ok && cells == MESSAGE_CELLS
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
    [table_route, judge_route, render_route, ledger_route]
}

// ---------------------------------------------------------------------------
// 阶段闸门：Dispatch→Option→Value→Message→Channel→Code→Exit
// ---------------------------------------------------------------------------

/// 一次非法值判据的走闸状态机。
///
/// 倒退判死（`VB_ESTAGE`），封账后再走一步也判死（`VB_ESTATE`）。
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
            return Err(VB_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(VB_ESTAGE);
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
            return Err(VB_ESTATE);
        }
        if self.current.is_none() {
            return Err(VB_ESTAGE);
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
                None => return Err(VB_ESLOT),
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
                None => return Err(VB_ESLOT),
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
        fold_word(h, self.value as u64)
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
            return Err(VB_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(VB_ESLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(VB_ESLOT);
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
        self.len.saturating_sub(self.ok_count())
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

    /// 逐格复核。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(VB_ESLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(VB_ESLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(VB_ESLOT),
            };
            if !s.ok {
                return Err(VB_ESTATE);
            }
            if s.code != 0 {
                return Err(VB_ECODE);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(VB_ESLOT),
            };
            if s.value != want_value {
                return Err(VB_EVALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(VB_ESLOT),
            };
            if s.op != want_op {
                return Err(VB_ECODE);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(VB_ESLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(VB_ESLOT),
            };
            if s.ok {
                return Err(VB_ESTATE);
            }
            if s.value != 0 {
                return Err(VB_EVALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(VB_ESLOT),
            };
            if s.code != want_code {
                return Err(VB_ECODE);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(VB_ESLOT),
            };
            if s.op != want_op {
                return Err(VB_ECODE);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                let other = match REJECT_CODES.get(k) {
                    Some(c) => *c,
                    None => return Err(VB_ESLOT),
                };
                if other == want_code {
                    return Err(VB_ECODE);
                }
                k += 1;
            }
            j += 1;
        }
        Ok(())
    }

    /// 第一处不合规的格子与原因（排障用，不改判定）。
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
            return Err(VB_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(VB_ESEAL);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(VB_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(VB_ESEAL),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(VB_ESEAL),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(VB_ESEAL);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许），用于注入红。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(VB_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(VB_ESLOT),
                }
                Ok(())
            }
            None => Err(VB_ESLOT),
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

/// 统计表：七个统计量 + 红档率。
pub const VALS: [u32; ACCEPT] = [
    STAGE_COUNT,
    OPT_JUDGED as u32,
    MESSAGE_CELLS as u32,
    CORPUS as u32,
    ACCEPT as u32,
    REJECT as u32,
    RATE_FULL,
    RED_RATE,
];

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

/// 注入一维错的台账（证明复核真的在逐格看）。
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
        3 => match STAT_CELLS.checked_add(3) {
            Some(c) => (c, 9u32),
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

// ---------------------------------------------------------------------------
// 十三条判据谓词
// ---------------------------------------------------------------------------

/// 01 名称与段基址。
fn name_ok() -> bool {
    if SEG_BASE != 0x6d220 {
        return false;
    }
    if ANCHOR_F11668_SEG.checked_add(ANCHOR_F11668_GAP) != Some(SEG_BASE) {
        return false;
    }
    if SEG_BASE + ANCHOR_F11672_GAP != ANCHOR_F11672_SEG {
        return false;
    }
    if CORPUS != SAMPLES || CORPUS != ACCEPT + REJECT {
        return false;
    }
    if TIER_COUNT != Tier::all().len() {
        return false;
    }
    if STAGE_COUNT != STAGE_LAST_RANK || Stage::all().len() as u32 != STAGE_COUNT {
        return false;
    }
    if OPT_COUNT != Opt::all().len() || OPT_JUDGED >= OPT_COUNT {
        return false;
    }
    true
}

/// 02 十三码互异。
fn codes_ok() -> bool {
    if ALL_CODES.len() != 13 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let a = match ALL_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a <= SEG_BASE || a >= ANCHOR_F11672_SEG {
            return false;
        }
        let mut j = i + 1usize;
        while j < ALL_CODES.len() {
            match ALL_CODES.get(j) {
                Some(b) => {
                    if *b == a {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    VB_ESTATE == SEG_BASE + 0x01
        && VB_ECODE == SEG_BASE + 0x02
        && VB_ESTAGE == SEG_BASE + 0x03
        && VB_EVALUE == SEG_BASE + 0x04
        && VB_ENOTE == SEG_BASE + 0x05
        && VB_EDEFAULT == SEG_BASE + 0x06
        && VB_EBODY == SEG_BASE + 0x07
        && VB_ECHAN == SEG_BASE + 0x08
        && VB_ESLOT == SEG_BASE + 0x09
        && VB_ESEAL == SEG_BASE + 0x0a
        && VB_ENOBS == SEG_BASE + 0x0b
        && VB_EMATRIX == SEG_BASE + 0x0c
        && VB_EBAL == SEG_BASE + 0x0d
}

/// 03 文案表十二格齐。
fn table_ok() -> bool {
    if MSG_TABLE.len() != MESSAGE_CELLS {
        return false;
    }
    let mut cells = 0usize;
    let mut i = 0usize;
    while i < OPT_JUDGED {
        let o = match Opt::at(i) {
            Some(x) => x,
            None => return false,
        };
        if !o.judged() {
            return false;
        }
        let mut f = 0usize;
        while f < FAMILY_COUNT {
            let fam = match Family::all().get(f) {
                Some(x) => *x,
                None => return false,
            };
            match msg_cell(o, fam) {
                Some(c) => {
                    cells += 1;
                    if c.opt != o || c.family != fam {
                        return false;
                    }
                    if c.tpl.is_empty() {
                        return false;
                    }
                    if !c.tpl.contains("{V}") {
                        return false;
                    }
                    if !c.tpl.contains(o.bin()) {
                        return false;
                    }
                    let want = match o.kind() {
                        Kind::Numeric => EXIT_FAIL,
                        Kind::Enum => EXIT_USAGE,
                    };
                    if c.code != want {
                        return false;
                    }
                }
                None => {
                    if !(o.kind() == Kind::Enum && !fam.enum_capable()) {
                        return false;
                    }
                }
            }
            f += 1;
        }
        i += 1;
    }
    if cells != MESSAGE_CELLS {
        return false;
    }
    msg_cell(Opt::Unknown, Family::Core).is_none()
        && msg_cell(Opt::LsColor, Family::Busybox).is_none()
        && msg_cell(Opt::LsSort, Family::Busybox).is_none()
        && msg_cell(Opt::CpReflink, Family::Busybox).is_none()
}

/// 04 首行/行数按族分家。
fn template_ok() -> bool {
    // 数值：coreutils 两行，busybox 一行。
    if lines_of(msg_text(Opt::HeadN, Family::Core, "abc").as_str()) != 2 {
        return false;
    }
    if lines_of(msg_text(Opt::HeadN, Family::Busybox, "abc").as_str()) != 1 {
        return false;
    }
    // 枚举：候选条数决定行数。
    if lines_of(msg_text(Opt::LsColor, Family::Core, "bogus").as_str()) != 6 {
        return false;
    }
    if lines_of(msg_text(Opt::LsSort, Family::Core, "bogus").as_str()) != 8 {
        return false;
    }
    if lines_of(msg_text(Opt::CpReflink, Family::Core, "bogus").as_str()) != 6 {
        return false;
    }
    // UTF-8 变体行数不变，字节数变大。
    let core = msg_text(Opt::LsColor, Family::Core, "bogus");
    let utf8 = msg_text(Opt::LsColor, Family::CoreUtf8, "bogus");
    if lines_of(core.as_str()) != lines_of(utf8.as_str()) {
        return false;
    }
    if bytes_of(utf8.as_str()) <= bytes_of(core.as_str()) {
        return false;
    }
    // C locale 只有直角引号，UTF-8 只有弯引号。
    if !core.contains(QUOTE_OPEN_C) || core.contains(QUOTE_OPEN_U) {
        return false;
    }
    if !utf8.contains(QUOTE_OPEN_U) || utf8.contains(QUOTE_OPEN_C) {
        return false;
    }
    // busybox 短报没有单位词、没有 Try 行。
    let bb = msg_text(Opt::HeadN, Family::Busybox, "abc");
    if bb.contains("of lines") || bb.contains(TRY_HEAD) {
        return false;
    }
    // coreutils 数值报有单位词与 Try 行。
    let cu = msg_text(Opt::HeadN, Family::Core, "abc");
    cu.contains("of lines") && cu.contains(TRY_HEAD) && cu.contains("head")
}

/// 05 诊断恒走 stderr。
fn channel_ok() -> bool {
    let good = define(&Shape::blank());
    if good.stage != Stage::Exit || good.note != Note::InvalidNumber {
        return false;
    }
    let mut s = Shape::blank();
    s.channel = Channel::Stdout;
    let bad = define(&s);
    if bad.stage != Stage::Channel || bad.note != Note::Misrouted {
        return false;
    }
    if bad.consistent {
        return false;
    }
    let mut quiet = Shape::blank();
    quiet.channel = Channel::Nowhere;
    let none = define(&quiet);
    if none.stage != Stage::Channel || none.note != Note::Misrouted {
        return false;
    }
    if Channel::Stderr.is_diag() != true || Channel::Stdout.is_diag() {
        return false;
    }
    !Channel::Nowhere.carries_bytes()
}

/// 06 码档：数值 1、枚举 2。
fn code_ok() -> bool {
    let numeric = define(&Shape::blank());
    if numeric.code != EXIT_FAIL as i32 {
        return false;
    }
    let mut e = Shape::blank();
    e.opt = Opt::LsColor;
    e.value = "bogus";
    e.exit_declared = EXIT_USAGE;
    let en = define(&e);
    if en.code != EXIT_USAGE as i32 {
        return false;
    }
    // 采集记的码与真码不符：结构不自洽。
    let mut lie = e;
    lie.exit_declared = EXIT_FAIL;
    let lied = define(&lie);
    if lied.consistent || lied.stage != Stage::Exit {
        return false;
    }
    // 127 是未名档，本卡两类错都不许用它。
    numeric.code != EXIT_NOTFOUND as i32 && en.code != EXIT_NOTFOUND as i32
}

/// 07 值辨析：数值整串解析、枚举比表。
fn parse_ok() -> bool {
    if parse_num("0") != Some(0) {
        return false;
    }
    if parse_num("10") != Some(10) {
        return false;
    }
    if parse_num("4294967295") != Some(4294967295) {
        return false;
    }
    if parse_num("").is_some() || parse_num("12x").is_some() || parse_num("abc").is_some() {
        return false;
    }
    if parse_num("4294967296").is_some() || parse_num("-3").is_some() {
        return false;
    }
    if !enum_accepts(Opt::LsColor, "always") || !enum_accepts(Opt::LsColor, "if-tty") {
        return false;
    }
    if enum_accepts(Opt::LsColor, "bogus") || enum_accepts(Opt::LsColor, "never!") {
        return false;
    }
    if !enum_accepts(Opt::LsSort, "extension") || enum_accepts(Opt::LsSort, "color") {
        return false;
    }
    if !enum_accepts(Opt::CpReflink, "auto") || enum_accepts(Opt::CpReflink, "auto!") {
        return false;
    }
    // 线性数字在 head 上合法：这是"不是本卡现场"的那一类。
    let mut okv = Shape::blank();
    okv.value = "10";
    let o = define(&okv);
    o.stage == Stage::Value && o.note == Note::ValueAccepted && o.default
}

/// 08 语料十四槽互校。
fn corpus_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        let a = c.actual();
        let want_stage = match WANT_STAGE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_code = match WANT_CODE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_value = match WANT_VALUE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_note = match WANT_NOTE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_default = match WANT_DEFAULT.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_success = match WANT_SUCCESS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.stage != want_stage
            || a.code as u32 != want_code
            || a.value != want_value
            || a.note != want_note
            || a.default != want_default
            || a.ok() != want_success
        {
            return false;
        }
        if i >= ACCEPT {
            let ndx = match WANT_NDX.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let k = &c.claim;
            let mut offs = 0usize;
            if a.ok() != k.success() {
                offs += 1;
            }
            if a.stage != k.stage {
                offs += 1;
            }
            if a.code != k.code {
                offs += 1;
            }
            if a.value != k.value {
                offs += 1;
            }
            if a.note != k.note {
                offs += 1;
            }
            if a.default != k.default {
                offs += 1;
            }
            if offs != 1 {
                return false;
            }
            if fault_ndx(c.fault()) != ndx {
                return false;
            }
        }
        i += 1;
    }
    let mut t = 0usize;
    while t < CORPUS {
        let a = match WANT_TAGS.get(t) {
            Some(x) => *x,
            None => return false,
        };
        if !a.starts_with("f11670-") {
            return false;
        }
        let mut u = t + 1usize;
        while u < CORPUS {
            if let Some(b) = WANT_TAGS.get(u) {
                if *b == a {
                    return false;
                }
            }
            u += 1;
        }
        t += 1;
    }
    WANT_STAGE.len() == CORPUS
        && WANT_CODE.len() == CORPUS
        && WANT_VALUE.len() == CORPUS
        && WANT_NOTE.len() == CORPUS
        && WANT_DEFAULT.len() == CORPUS
        && WANT_SUCCESS.len() == CORPUS
        && WANT_NDX.len() == CORPUS
}

/// 09 判序六维互斥。
fn judge_ok() -> bool {
    let mut good = Case::blank();
    good.shape = Shape::blank();
    good.claim = Claim {
        success: false,
        stage: Stage::Exit,
        code: EXIT_FAIL as i32,
        value: define(&Shape::blank()).value,
        note: Note::InvalidNumber,
        default: false,
    };
    if judge(&good) != Fault::Ok {
        return false;
    }
    // 同时坏 Estate 与 Stage：必须报 Estate。
    let mut both = good;
    both.claim.success = true;
    both.claim.stage = Stage::Value;
    if judge(&both) != Fault::Estate {
        return false;
    }
    // 同时坏 Stage 与 Code：必须报 Stage。
    let mut sc = good;
    sc.claim.stage = Stage::Message;
    sc.claim.code = EXIT_USAGE as i32;
    if judge(&sc) != Fault::Stage {
        return false;
    }
    // 同时坏 Code 与 Value：必须报 Code。
    let mut cv = good;
    cv.claim.code = EXIT_USAGE as i32;
    cv.claim.value = 9;
    if judge(&cv) != Fault::Code {
        return false;
    }
    // 同时坏 Value 与 Note：必须报 Value。
    let mut vn = good;
    vn.claim.value = 9;
    vn.claim.note = Note::InvalidEnum;
    if judge(&vn) != Fault::Value {
        return false;
    }
    // 同时坏 Note 与 Default：必须报 Note。
    let mut nd = good;
    nd.claim.note = Note::InvalidEnum;
    nd.claim.default = true;
    if judge(&nd) != Fault::Note {
        return false;
    }
    // 只坏 Default：报 Default。
    let mut d = good;
    d.claim.default = true;
    if judge(&d) != Fault::Default {
        return false;
    }
    // 现场真落兜底时，怎么申报都不许当结论。
    let mut fallback = Case::blank();
    fallback.shape = Shape::blank().with_opt(Opt::Unknown);
    fallback.claim = Claim {
        success: false,
        stage: Stage::Dispatch,
        code: EXIT_FAIL as i32,
        value: 0,
        note: Note::Fallback,
        default: true,
    };
    judge(&fallback) == Fault::Default
}

/// 10 四档矩阵覆盖。
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
        (Tier::Verbatim, 6usize),
        (Tier::Bannered, 4usize),
        (Tier::NoBody, 2usize),
        (Tier::Refused, 2usize),
    ];
    let mut i = 0usize;
    while i < want.len() {
        let (tier, good) = match want.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let row = match m.row(tier) {
            Some(r) => r,
            None => return false,
        };
        if row.good != good || row.total != CORPUS {
            return false;
        }
        i += 1;
    }
    // 抽样归档：
    let mut e = Shape::blank();
    e.opt = Opt::LsSort;
    e.value = "bogus";
    e.exit_declared = EXIT_USAGE;
    if Tier::of(&define(&e)) != Tier::Verbatim {
        return false;
    }
    e.family = Family::CoreUtf8;
    e.lang_utf8 = true;
    if Tier::of(&define(&e)) != Tier::Bannered {
        return false;
    }
    let bb = Shape {
        family: Family::Busybox,
        ..Shape::blank()
    };
    if Tier::of(&define(&bb)) != Tier::NoBody {
        return false;
    }
    let missing = Shape {
        opt: Opt::LsSort,
        family: Family::Busybox,
        value: "bogus",
        ..Shape::blank()
    };
    Tier::of(&define(&missing)) == Tier::Refused
}

/// 11 全绿渲染逐字。
fn render_ok() -> bool {
    match run() {
        Ok(rep) => rep.green() && rep.render().as_str() == RENDER_OK && rep.rate() == RATE_FULL,
        Err(_) => false,
    }
}

/// 12 阶段闸门不倒退。
fn gate_ok() -> bool {
    let root = match Gate::full_pass() {
        Ok(h) => h,
        Err(_) => return false,
    };
    if root == 0 {
        return false;
    }
    let mut g = Gate::new();
    if g.step(Stage::Message) != Ok(()) {
        return false;
    }
    if g.step(Stage::Option) != Err(VB_ESTAGE) {
        return false;
    }
    if g.stage() != Some(Stage::Message) || g.refusals() != 1 {
        return false;
    }
    let mut closed = match Gate::walk(&Stage::all()) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if closed.close().is_err() {
        return false;
    }
    if closed.step(Stage::Exit) != Err(VB_ESTATE) {
        return false;
    }
    if closed.close() != Err(VB_ESTATE) {
        return false;
    }
    let mut empty = Gate::new();
    empty.close() == Err(VB_ESTAGE) && closed.is_closed()
}

/// 13 冻结重放四注入。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES || l.audit() != Ok(()) {
        return false;
    }
    if l.ok_count() != STAT_CELLS + 1 || l.err_count() != REJECT {
        return false;
    }
    let h = match l.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 || !l.is_frozen() || l.replay() != Ok(h) {
        return false;
    }
    let mut w = 0u8;
    while w < 4u8 {
        let broken = build_ledger_broken(w);
        if broken.audit().is_err() != true {
            return false;
        }
        w += 1;
    }
    if build_ledger_broken(9).audit() != Ok(()) {
        return false;
    }
    let mut wrong = Ledger::new();
    let stray = Sample {
        slot: 3,
        op: OP_DISPATCH,
        ok: true,
        code: 0,
        value: 1,
    };
    wrong.register(stray) == Err(VB_ESLOT) && wrong.rejects() == 1 && wrong.is_empty()
}

/// 申报表第一格的标签。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "f11670-head-n-abc",
    }
}

/// 统计表第一格的值。
pub fn head_val() -> u32 {
    match VALS.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

/// 汇总：十三条判据，一条不多一条不少。
pub fn run_unx11670_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11670_bad_option_value");
    s.add("VB01-名称与段基址", name_ok(), "static");
    s.add("VB02-十三码互异", codes_ok(), "static");
    s.add("VB03-文案表十二格齐", table_ok(), "static");
    s.add("VB04-行数按族分家", template_ok(), "static");
    s.add("VB05-诊断走stderr", channel_ok(), "static");
    s.add("VB06-码档1与2对齐", code_ok(), "static");
    s.add("VB07-数值与枚举辨析", parse_ok(), "static");
    s.add("VB08-语料十四槽互校", corpus_ok(), "static");
    s.add("VB09-判序六维互斥", judge_ok(), "static");
    s.add("VB10-四档矩阵覆盖", matrix_ok(), "static");
    s.add("VB11-全绿渲染逐字", render_ok(), "static");
    s.add("VB12-阶段闸门不倒退", gate_ok(), "static");
    s.add(
        "VB13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let audited = l.audit() == Ok(());
            let frozen = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let four = inject_four();
            let mut all_four = true;
            let mut i = 0usize;
            while i < four.len() {
                if let Some(x) = four.get(i) {
                    if !*x {
                        all_four = false;
                    }
                }
                i += 1;
            }
            audited && frozen && ledger_ok() && all_four
        },
        "static",
    );
    s
}

// ---------------------------------------------------------------------------
// 单元测试：每条只钉一个可复算的事实
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 取一槽的实际结论；缺槽直接失败。
    fn out(i: usize) -> Outcome {
        case_at(i).expect("slot must exist").actual()
    }

    /// 枚举错的一格（coreutils，退 2）。
    fn enum_shape(o: Opt) -> Shape {
        Shape {
            opt: o,
            value: "bogus",
            exit_declared: EXIT_USAGE,
            ..Shape::blank()
        }
    }

    /// 同一格的 UTF-8 变体。
    fn enum_utf8(o: Opt) -> Shape {
        Shape {
            family: Family::CoreUtf8,
            lang_utf8: true,
            ..enum_shape(o)
        }
    }

    #[test]
    fn t01_seg_base_is_pinned() {
        assert_eq!(SEG_BASE, 0x6d220);
        assert_eq!(ANCHOR_F11668_SEG + ANCHOR_F11668_GAP, SEG_BASE);
        assert_eq!(SEG_BASE + ANCHOR_F11672_GAP, ANCHOR_F11672_SEG);
        assert_eq!(ANCHOR_F11674_SEG, 0x6d270);
        assert_eq!(ANCHOR_F11669_SEG, 0x6d210);
        assert!(ANCHOR_F11672_SEG > SEG_BASE);
    }

    #[test]
    fn t02_scale_relations_hold() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, CORPUS);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(STAGE_COUNT, 7);
        assert_eq!(STAGE_LAST_RANK, STAGE_COUNT);
        assert_eq!(OPT_COUNT, 6);
        assert_eq!(OPT_JUDGED, 5);
        assert_eq!(FAMILY_COUNT, 3);
        assert_eq!(KIND_COUNT, 2);
        assert_eq!(MESSAGE_CELLS, 12);
        assert_eq!(STAT_CELLS, 7);
        assert_eq!(VALS.len(), ACCEPT);
    }

    #[test]
    fn t03_code_offsets_are_exact() {
        assert_eq!(VB_ESTATE, SEG_BASE + 0x01);
        assert_eq!(VB_ECODE, SEG_BASE + 0x02);
        assert_eq!(VB_ESTAGE, SEG_BASE + 0x03);
        assert_eq!(VB_EVALUE, SEG_BASE + 0x04);
        assert_eq!(VB_ENOTE, SEG_BASE + 0x05);
        assert_eq!(VB_EDEFAULT, SEG_BASE + 0x06);
        assert_eq!(VB_EBODY, SEG_BASE + 0x07);
        assert_eq!(VB_ECHAN, SEG_BASE + 0x08);
        assert_eq!(VB_ESLOT, SEG_BASE + 0x09);
        assert_eq!(VB_ESEAL, SEG_BASE + 0x0a);
        assert_eq!(VB_ENOBS, SEG_BASE + 0x0b);
        assert_eq!(VB_EMATRIX, SEG_BASE + 0x0c);
        assert_eq!(VB_EBAL, SEG_BASE + 0x0d);
    }

    #[test]
    fn t04_thirteen_codes_are_distinct() {
        assert_eq!(ALL_CODES.len(), 13);
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(x) => *x,
                None => break,
            };
            assert!(c > SEG_BASE);
            assert!(c < ANCHOR_F11672_SEG);
            let mut j = i + 1usize;
            while j < ALL_CODES.len() {
                if let Some(d) = ALL_CODES.get(j) {
                    assert_ne!(c, *d);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t05_message_table_has_twelve_cells() {
        assert_eq!(MSG_TABLE.len(), MESSAGE_CELLS);
        let mut cells = 0usize;
        let mut i = 0usize;
        while i < OPT_JUDGED {
            let o = Opt::at(i).expect("option in range");
            assert!(o.judged());
            let mut f = 0usize;
            while f < FAMILY_COUNT {
                let fam = *Family::all().get(f).expect("family");
                match msg_cell(o, fam) {
                    Some(c) => {
                        cells += 1;
                        assert!(c.tpl.contains("{V}"));
                        assert!(c.tpl.contains(o.bin()));
                    }
                    None => {
                        assert_eq!(o.kind(), Kind::Enum);
                        assert!(!fam.enum_capable());
                    }
                }
                f += 1;
            }
            i += 1;
        }
        assert_eq!(cells, MESSAGE_CELLS);
        assert!(msg_cell(Opt::Unknown, Family::Core).is_none());
    }

    #[test]
    fn t06_table_codes_follow_the_kind() {
        assert_eq!(msg_code(Opt::HeadN, Family::Core), EXIT_FAIL);
        assert_eq!(msg_code(Opt::TailN, Family::Busybox), EXIT_FAIL);
        assert_eq!(msg_code(Opt::LsColor, Family::Core), EXIT_USAGE);
        assert_eq!(msg_code(Opt::LsSort, Family::CoreUtf8), EXIT_USAGE);
        assert_eq!(msg_code(Opt::CpReflink, Family::Core), EXIT_USAGE);
        assert_eq!(msg_code(Opt::Unknown, Family::Core), EXIT_FAIL);
        assert_eq!(Opt::HeadN.kind(), Kind::Numeric);
        assert_eq!(Opt::LsColor.kind(), Kind::Enum);
        assert_eq!(Opt::HeadN.unit(), "lines");
    }

    #[test]
    fn t07_numeric_text_is_two_lines_on_coreutils() {
        let t = msg_text(Opt::HeadN, Family::Core, "abc");
        assert_eq!(
            t,
            "head: invalid number of lines: 'abc'\nTry 'head --help' for more information.\n"
        );
        assert_eq!(lines_of(t.as_str()), 2);
        let u = msg_text(Opt::TailN, Family::Core, "12x");
        assert!(u.starts_with("tail: invalid number of lines: '12x'"));
        assert!(u.contains(TRY_HEAD));
        assert!(u.ends_with("for more information.\n"));
    }

    #[test]
    fn t08_busybox_text_is_one_line() {
        let t = msg_text(Opt::HeadN, Family::Busybox, "abc");
        assert_eq!(t, "head: invalid number: 'abc'\n");
        assert_eq!(lines_of(t.as_str()), 1);
        assert!(!t.contains("of lines"));
        assert!(!t.contains(TRY_HEAD));
        let v = msg_text(Opt::TailN, Family::Busybox, "12x");
        assert_eq!(v, "tail: invalid number: '12x'\n");
        assert_eq!(Family::Busybox.numeric_lines(), 1);
        assert_eq!(Family::Core.numeric_lines(), 2);
    }

    #[test]
    fn t09_color_text_lists_nine_values_in_three_lines() {
        let t = msg_text(Opt::LsColor, Family::Core, "bogus");
        assert_eq!(lines_of(t.as_str()), 6);
        assert!(t.starts_with("ls: invalid argument 'bogus' for '--color'\n"));
        assert!(t.contains(VALID_HEAD));
        assert!(t.contains("  - 'always', 'yes', 'force'"));
        assert!(t.contains("  - 'never', 'no', 'none'"));
        assert!(t.contains("  - 'auto', 'tty', 'if-tty'"));
        assert!(t.ends_with("Try 'ls --help' for more information.\n"));
        let mut n = 0usize;
        let mut i = 0usize;
        while i < COLOR_OK.len() {
            if let Some(v) = COLOR_OK.get(i) {
                if t.contains(v) {
                    n += 1;
                }
            }
            i += 1;
        }
        assert_eq!(n, COLOR_OK.len());
    }

    #[test]
    fn t10_sort_and_reflink_line_counts_differ() {
        let s = msg_text(Opt::LsSort, Family::Core, "bogus");
        assert_eq!(lines_of(s.as_str()), 8);
        assert!(s.contains("  - 'extension'"));
        assert!(s.contains("  - 'version'"));
        let mut n = 0usize;
        let mut i = 0usize;
        while i < SORT_OK.len() {
            if let Some(v) = SORT_OK.get(i) {
                if s.contains(v) {
                    n += 1;
                }
            }
            i += 1;
        }
        assert_eq!(n, SORT_OK.len());
        let r = msg_text(Opt::CpReflink, Family::Core, "bogus");
        assert_eq!(lines_of(r.as_str()), 6);
        assert!(r.contains("  - 'always'"));
        assert!(r.contains("  - 'auto'"));
        assert!(r.contains("  - 'never'"));
        assert!(r.contains("Try 'cp --help'"));
    }

    #[test]
    fn t11_utf8_variant_only_changes_quotes() {
        let c = msg_text(Opt::LsSort, Family::Core, "bogus");
        let u = msg_text(Opt::LsSort, Family::CoreUtf8, "bogus");
        assert_eq!(lines_of(c.as_str()), lines_of(u.as_str()));
        assert!(u.contains(QUOTE_OPEN_U));
        assert!(u.contains(QUOTE_CLOSE_U));
        assert!(!u.contains(QUOTE_OPEN_C));
        assert!(bytes_of(u.as_str()) > bytes_of(c.as_str()));
        assert!(Family::CoreUtf8.curly());
        assert!(!Family::Core.curly());
        let n = msg_text(Opt::HeadN, Family::CoreUtf8, "abc");
        assert_eq!(n, "head: invalid number of lines: ‘abc’\nTry ‘head --help’ for more information.\n");
        assert_eq!(lines_of(n.as_str()), 2);
    }

    #[test]
    fn t12_subst_fills_every_placeholder() {
        assert_eq!(subst("a{V}b", "x"), "axb");
        assert_eq!(subst("{V}{V}", "x"), "xx");
        assert_eq!(subst("novalue", "x"), "novalue");
        assert_eq!(subst("", "x"), "");
        assert_eq!(subst("p{V}q{V}r", "12x"), "p12xq12xr");
    }

    #[test]
    fn t13_option_metadata_is_closed() {
        assert_eq!(Opt::all().len(), OPT_COUNT);
        assert_eq!(Opt::at(0), Some(Opt::HeadN));
        assert_eq!(Opt::at(5), Some(Opt::Unknown));
        assert_eq!(Opt::at(6), None);
        assert!(!Opt::Unknown.judged());
        assert_eq!(Opt::HeadN.bin(), "head");
        assert_eq!(Opt::TailN.bin(), "tail");
        assert_eq!(Opt::LsSort.bin(), "ls");
        assert_eq!(Opt::CpReflink.bin(), "cp");
        assert_eq!(Opt::LsSort.long(), "--sort");
        assert_eq!(Opt::CpReflink.long(), "--reflink");
        assert_eq!(opt_word(Opt::LsColor), "color");
        assert_eq!(Opt::LsSort.index(), 3);
    }

    #[test]
    fn t14_parse_num_is_strict() {
        assert_eq!(parse_num("0"), Some(0));
        assert_eq!(parse_num("007"), Some(7));
        assert_eq!(parse_num("4294967295"), Some(4294967295));
        assert_eq!(parse_num(""), None);
        assert_eq!(parse_num("abc"), None);
        assert_eq!(parse_num("12x"), None);
        assert_eq!(parse_num("-3"), None);
        assert_eq!(parse_num("+3"), None);
        assert_eq!(parse_num("4294967296"), None);
        assert_eq!(parse_num("1 2"), None);
    }

    #[test]
    fn t15_enum_sets_are_exact() {
        assert_eq!(COLOR_OK.len(), 9);
        assert_eq!(SORT_OK.len(), 5);
        assert_eq!(REFLINK_OK.len(), 3);
        let mut i = 0usize;
        while i < COLOR_OK.len() {
            let v = *COLOR_OK.get(i).expect("color value");
            assert!(enum_accepts(Opt::LsColor, v));
            if !in_set(&SORT_OK, v) {
                assert!(!enum_accepts(Opt::LsSort, v));
            }
            i += 1;
        }
        assert!(enum_accepts(Opt::LsSort, "extension"));
        assert!(!enum_accepts(Opt::LsSort, "extensions"));
        assert!(enum_accepts(Opt::CpReflink, "never"));
        assert!(!enum_accepts(Opt::HeadN, "always"));
        assert!(!enum_accepts(Opt::Unknown, "always"));
    }

    #[test]
    fn t16_channel_traits() {
        assert_eq!(Channel::all().len(), 3);
        assert!(Channel::Stderr.is_diag());
        assert!(Channel::Stdout.carries_bytes());
        assert!(!Channel::Stdout.is_diag());
        assert!(!Channel::Nowhere.carries_bytes());
        assert_eq!(Channel::Stderr.name(), "stderr");
    }

    #[test]
    fn t17_stage_ranks_are_monotone() {
        let table = Stage::all();
        assert_eq!(table.len() as u32, STAGE_COUNT);
        let mut i = 0usize;
        while i < table.len() {
            let s = *table.get(i).expect("stage");
            assert_eq!(s.rank(), i as u32 + 1);
            assert_eq!(Stage::from_rank(s.rank()), Some(s));
            assert_eq!(s.before_text(), i < 3);
            assert_eq!(s.settled_gate(), s != Stage::Exit);
            i += 1;
        }
        assert_eq!(Stage::from_rank(0), None);
        assert_eq!(Stage::from_rank(8), None);
        assert_eq!(Stage::Exit.rank(), STAGE_LAST_RANK);
    }

    #[test]
    fn t18_note_classes_are_disjoint() {
        assert_eq!(Note::all().len(), Note::COUNT);
        assert_eq!(Note::COUNT, 11);
        let mut defaulted = 0usize;
        let mut bodies = 0usize;
        let mut curly = 0usize;
        let mut bb = 0usize;
        let mut i = 0usize;
        while i < Note::COUNT {
            let n = *Note::all().get(i).expect("note");
            if n.defaulted() {
                defaulted += 1;
            }
            if n.has_body() {
                bodies += 1;
            }
            if n.curly() {
                curly += 1;
            }
            if n.is_busybox() {
                bb += 1;
            }
            assert_eq!(n.index(), i);
            i += 1;
        }
        assert_eq!(defaulted, 2);
        assert_eq!(bodies, 5);
        assert_eq!(curly, 2);
        assert_eq!(bb, 1);
        assert!(Note::ValueAccepted.defaulted());
        assert!(Note::Fallback.defaulted());
        assert!(!Note::NoSuchOption.defaulted());
        assert!(!Note::NoSuchOption.has_body());
        assert!(Note::InvalidEnum.needs_try());
        assert!(!Note::InvalidNumberBb.needs_try());
    }

    #[test]
    fn t19_note_names_are_unique() {
        let mut i = 0usize;
        while i < Note::COUNT {
            let a = *Note::all().get(i).expect("note");
            assert!(!a.name().is_empty());
            let mut j = i + 1usize;
            while j < Note::COUNT {
                let b = *Note::all().get(j).expect("note");
                assert_ne!(a.name(), b.name());
                j += 1;
            }
            i += 1;
        }
        assert_eq!(Note::InvalidEnumUtf8.name(), "invalid-enum-utf8");
    }

    #[test]
    fn t20_tier_classification_is_total() {
        let num = Outcome::make(Stage::Exit, EXIT_FAIL, 2, Note::InvalidNumber, false, true);
        let utf8 = Outcome::make(Stage::Exit, EXIT_FAIL, 2, Note::InvalidNumberUtf8, false, true);
        let terse = Outcome::make(Stage::Exit, EXIT_FAIL, 1, Note::InvalidNumberBb, false, true);
        let gone = Outcome::make(Stage::Value, 0, 0, Note::ValueAccepted, true, true);
        assert_eq!(Tier::of(&num), Tier::Verbatim);
        assert_eq!(Tier::of(&utf8), Tier::Bannered);
        assert_eq!(Tier::of(&terse), Tier::NoBody);
        assert_eq!(Tier::of(&gone), Tier::Refused);
        assert_eq!(Tier::all().len(), TIER_COUNT);
        assert!(Tier::Verbatim.has_text());
        assert!(!Tier::NoBody.has_text());
        assert_eq!(Tier::all().get(3).map(|t| t.name()), Some("refused"));
    }

    #[test]
    fn t21_fault_codes_and_order() {
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(Fault::Estate.code(), VB_ESTATE);
        assert_eq!(Fault::Stage.code(), VB_ESTAGE);
        assert_eq!(Fault::Code.code(), VB_ECODE);
        assert_eq!(Fault::Value.code(), VB_EVALUE);
        assert_eq!(Fault::Note.code(), VB_ENOTE);
        assert_eq!(Fault::Default.code(), VB_EDEFAULT);
        assert_eq!(Fault::Ok.order(), 6);
        assert_eq!(Fault::Estate.order(), 0);
        assert_eq!(fault_short(Fault::Code), "ECODE");
        assert_eq!(Fault::Value.name(), "value");
    }

    #[test]
    fn t22_reject_tables_align() {
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        let mut i = 0usize;
        while i < REJECT {
            let f = *REJECT_FAULTS.get(i).expect("fault");
            let c = *REJECT_CODES.get(i).expect("code");
            assert_eq!(f.code(), c);
            assert_eq!(f.order(), i);
            assert_eq!(ndx_fault(i), Some(f));
            i += 1;
        }
        assert_eq!(ndx_fault(6), Some(Fault::Ok));
        assert_eq!(ndx_fault(7), None);
    }

    #[test]
    fn t23_define_blank_is_numeric_error() {
        let o = define(&Shape::blank());
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.code, EXIT_FAIL as i32);
        assert_eq!(o.value, 2);
        assert_eq!(o.note, Note::InvalidNumber);
        assert!(!o.default);
        assert!(o.consistent);
        assert!(!o.ok());
        assert!(o.failed());
        assert!(o.diag_ok());
        assert_eq!(o.exit(), EXIT_FAIL);
        assert_eq!(Tier::of(&o), Tier::Verbatim);
    }

    #[test]
    fn t24_define_busybox_is_terse() {
        let s = Shape {
            family: Family::Busybox,
            ..Shape::blank()
        };
        let o = define(&s);
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.code, EXIT_FAIL as i32);
        assert_eq!(o.value, 1);
        assert_eq!(o.note, Note::InvalidNumberBb);
        assert!(o.diag_ok());
        assert_eq!(Tier::of(&o), Tier::NoBody);
    }

    #[test]
    fn t25_define_enum_errors_exit_two() {
        let color = define(&enum_shape(Opt::LsColor));
        assert_eq!(color.stage, Stage::Exit);
        assert_eq!(color.code, EXIT_USAGE as i32);
        assert_eq!(color.value, 6);
        assert_eq!(color.note, Note::InvalidEnum);
        let sort = define(&enum_shape(Opt::LsSort));
        assert_eq!(sort.value, 8);
        let reflink = define(&enum_shape(Opt::CpReflink));
        assert_eq!(reflink.value, 6);
        assert_eq!(reflink.code, EXIT_USAGE as i32);
        assert_eq!(Tier::of(&reflink), Tier::Verbatim);
    }

    #[test]
    fn t26_define_utf8_is_the_quoted_tier() {
        let o = define(&enum_utf8(Opt::LsSort));
        assert_eq!(o.note, Note::InvalidEnumUtf8);
        assert_eq!(o.value, 8);
        assert_eq!(o.code, EXIT_USAGE as i32);
        assert_eq!(Tier::of(&o), Tier::Bannered);
        let n = define(&Shape {
            family: Family::CoreUtf8,
            lang_utf8: true,
            ..Shape::blank()
        });
        assert_eq!(n.note, Note::InvalidNumberUtf8);
        assert_eq!(n.value, 2);
    }

    #[test]
    fn t27_define_missing_option_is_refused() {
        let s = Shape {
            opt: Opt::LsSort,
            family: Family::Busybox,
            value: "bogus",
            ..Shape::blank()
        };
        assert!(!s.valid());
        let o = define(&s);
        assert_eq!(o.stage, Stage::Option);
        assert_eq!(o.code, EXIT_FAIL as i32);
        assert_eq!(o.value, 1);
        assert_eq!(o.note, Note::NoSuchOption);
        assert!(!o.consistent);
        assert!(o.refused());
        assert!(!o.diag_ok());
        assert_eq!(Tier::of(&o), Tier::Refused);
        assert_eq!(msg_nosuch(Opt::LsSort), "ls: unrecognized option: sort\n");
    }

    #[test]
    fn t28_define_legal_value_is_the_fallback() {
        let o = define(&Shape::blank().with_value("10"));
        assert_eq!(o.stage, Stage::Value);
        assert_eq!(o.code, 0);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::ValueAccepted);
        assert!(o.default);
        assert!(o.anchored());
        assert!(o.ok());
        assert!(!o.diag_ok());
        assert_eq!(Tier::of(&o), Tier::Refused);
        let e = define(&Shape {
            opt: Opt::LsColor,
            value: "always",
            exit_declared: EXIT_OK,
            ..Shape::blank()
        });
        assert_eq!(e.note, Note::ValueAccepted);
        assert!(e.default);
    }

    #[test]
    fn t29_define_misrouted_channel_is_refused() {
        let s = Shape {
            channel: Channel::Stdout,
            ..Shape::blank()
        };
        let o = define(&s);
        assert_eq!(o.stage, Stage::Channel);
        assert_eq!(o.code, EXIT_FAIL as i32);
        assert_eq!(o.value, 2);
        assert_eq!(o.note, Note::Misrouted);
        assert!(o.refused());
        assert_eq!(Tier::of(&o), Tier::Refused);
        let quiet = Shape {
            channel: Channel::Nowhere,
            ..Shape::blank()
        };
        assert_eq!(define(&quiet).note, Note::Misrouted);
    }

    #[test]
    fn t30_define_truncated_keeps_one_line() {
        let s = Shape {
            truncated: true,
            ..Shape::blank()
        };
        let o = define(&s);
        assert_eq!(o.stage, Stage::Message);
        assert_eq!(o.value, 1);
        assert_eq!(o.note, Note::Truncated);
        assert!(o.refused());
        assert_eq!(Tier::of(&o), Tier::Refused);
        // 枚举格被截断也只剩首行，但行数不是 1 以外的值。
        let e = define(&Shape {
            truncated: true,
            ..enum_shape(Opt::LsSort)
        });
        assert_eq!(e.value, 1);
        assert_eq!(e.note, Note::Truncated);
    }

    #[test]
    fn t31_define_unknown_option_falls_back() {
        let o = define(&Shape::blank().with_opt(Opt::Unknown));
        assert_eq!(o.stage, Stage::Dispatch);
        assert_eq!(o.code, EXIT_FAIL as i32);
        assert_eq!(o.value, 0);
        assert_eq!(o.note, Note::Fallback);
        assert!(o.default);
        assert!(o.consistent);
        assert_eq!(Tier::of(&o), Tier::Refused);
        assert_eq!(Opt::Unknown.name(), UNKNOWN_OPT);
    }

    #[test]
    fn t32_judge_reads_the_master_claim_green() {
        let c = case_at(0).expect("slot 0");
        assert_eq!(judge(&c), Fault::Ok);
        assert!(c.ok());
    }

    #[test]
    fn t33_judge_puts_estate_first() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.success = true;
        c.claim.stage = Stage::Value;
        c.claim.code = 0;
        assert_eq!(judge(&c), Fault::Estate);
    }

    #[test]
    fn t34_judge_orders_stage_before_code() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.stage = Stage::Message;
        c.claim.code = EXIT_USAGE as i32;
        assert_eq!(judge(&c), Fault::Stage);
    }

    #[test]
    fn t35_judge_orders_code_before_value() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.code = EXIT_USAGE as i32;
        c.claim.value = 9;
        assert_eq!(judge(&c), Fault::Code);
    }

    #[test]
    fn t36_judge_orders_value_before_note() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.value = 9;
        c.claim.note = Note::InvalidEnum;
        assert_eq!(judge(&c), Fault::Value);
    }

    #[test]
    fn t37_judge_orders_note_before_default() {
        let mut c = case_at(0).expect("slot 0");
        c.claim.note = Note::InvalidEnum;
        c.claim.default = true;
        assert_eq!(judge(&c), Fault::Note);
        let mut d = case_at(0).expect("slot 0");
        d.claim.default = true;
        assert_eq!(judge(&d), Fault::Default);
    }

    #[test]
    fn t38_corpus_is_eight_live_six_dead() {
        let all = cases();
        assert_eq!(all.len(), CORPUS);
        let mut i = 0usize;
        while i < CORPUS {
            let c = *all.get(i).expect("case");
            if i < ACCEPT {
                assert_eq!(c.fault(), Fault::Ok);
            } else {
                assert_ne!(c.fault(), Fault::Ok);
                assert_eq!(Some(c.fault()), expected(i));
            }
            i += 1;
        }
        assert!(case_at(CORPUS).is_none());
    }

    #[test]
    fn t39_corpus_slots_differ_in_one_dimension() {
        let mut i = ACCEPT;
        while i < CORPUS {
            let c = case_at(i).expect("reject slot");
            let a = c.actual();
            let k = &c.claim;
            let mut offs = 0usize;
            if a.ok() != k.success() {
                offs += 1;
            }
            if a.stage != k.stage {
                offs += 1;
            }
            if a.code != k.code {
                offs += 1;
            }
            if a.value != k.value {
                offs += 1;
            }
            if a.note != k.note {
                offs += 1;
            }
            if a.default != k.default {
                offs += 1;
            }
            assert_eq!(offs, 1);
            assert_eq!(WANT_NDX.get(i), Some(&c.fault().order()));
            i += 1;
        }
    }

    #[test]
    fn t40_tags_are_prefixed_and_unique() {
        assert_eq!(WANT_TAGS.len(), CORPUS);
        let mut i = 0usize;
        while i < CORPUS {
            let a = *WANT_TAGS.get(i).expect("tag");
            assert!(a.starts_with("f11670-"));
            let mut j = i + 1usize;
            while j < CORPUS {
                assert_ne!(a, *WANT_TAGS.get(j).expect("tag"));
                j += 1;
            }
            i += 1;
        }
        assert_eq!(head_tag(), "f11670-head-n-abc");
    }

    #[test]
    fn t41_expected_mapping_is_pinned() {
        assert_eq!(expected(0), Some(Fault::Ok));
        assert_eq!(expected(7), Some(Fault::Ok));
        assert_eq!(expected(8), Some(Fault::Estate));
        assert_eq!(expected(9), Some(Fault::Stage));
        assert_eq!(expected(10), Some(Fault::Code));
        assert_eq!(expected(11), Some(Fault::Value));
        assert_eq!(expected(12), Some(Fault::Note));
        assert_eq!(expected(13), Some(Fault::Default));
        assert_eq!(expected(14), None);
        assert_eq!(fault_ndx(Fault::Default), 5);
    }

    #[test]
    fn t42_cover_bounds_are_checked() {
        assert_eq!(cover(0), Ok(()));
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS + 1), Err(VB_ESLOT));
        assert_eq!(cover(usize::MAX), Err(VB_ESLOT));
    }

    #[test]
    fn t43_matrix_covers_four_tiers() {
        let m = Matrix::build();
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert_eq!(m.goods(), CORPUS);
        assert_eq!(m.red_total(), CORPUS * TIER_COUNT - CORPUS);
        assert_eq!(m.perfect_rows(), 0);
        assert!(m.covered());
        assert_eq!(m.row(Tier::Verbatim).map(|r| r.good), Some(6));
        assert_eq!(m.row(Tier::Bannered).map(|r| r.good), Some(4));
        assert_eq!(m.row(Tier::NoBody).map(|r| r.good), Some(2));
        assert_eq!(m.row(Tier::Refused).map(|r| r.good), Some(2));
        let r = m.row(Tier::Verbatim).expect("verbatim row");
        assert_eq!(r.total, CORPUS);
        assert_eq!(r.red(), CORPUS - 6);
        assert_eq!(r.render(), "verbatim=6/14");
        assert!(m.render().starts_with("matrix "));
        assert!(m.render().contains("refused=2/14"));
    }

    #[test]
    fn t44_report_render_is_byte_pinned() {
        let rep = run().expect("all green");
        assert_eq!(rep.render(), RENDER_OK);
        assert_eq!(rep.total, CORPUS);
        assert_eq!(rep.accept, ACCEPT);
        assert_eq!(rep.reject, REJECT);
        assert_eq!(rep.verbatim, 6);
        assert_eq!(rep.bannered, 4);
        assert_eq!(rep.nobody, 2);
        assert_eq!(rep.refused, 2);
        assert_eq!(rep.matched, CORPUS);
        assert_eq!(rep.tier_cells(), CORPUS as u32);
    }

    #[test]
    fn t45_report_rate_and_gate() {
        let rep = run().expect("all green");
        assert_eq!(rep.rate(), RATE_FULL);
        assert_eq!(rep.rate(), 1000);
        assert!(rep.green());
        assert_eq!(rep.reds(), 0);
        assert_eq!(rep.gate(), Ok(()));
        let empty = empty_report();
        assert_eq!(empty.rate(), 0);
        assert_eq!(empty.gate(), Err(VB_ENOBS));
        assert!(!empty.green());
    }

    #[test]
    fn t46_run_with_rejects_out_of_range_slot() {
        assert_eq!(run_with(Some((CORPUS, Fault::Value))), Err(VB_ESLOT));
        let noop = run_with(Some((0, Fault::Ok))).expect("no-op injection");
        assert_eq!(noop.matched, CORPUS);
        let dropped = run_with(Some((2, Fault::Value))).expect("injected report");
        assert_eq!(dropped.matched, CORPUS - 1);
        assert_eq!(dropped.rate(), RED_RATE);
        let keep = run_with(Some((8, Fault::Estate))).expect("same fault");
        assert_eq!(keep.matched, CORPUS);
    }

    #[test]
    fn t47_red_rate_is_calibrated() {
        assert_eq!(RED_RATE, 929);
        assert!(red_rate_calibrated());
        let bad = run_with(Some((11, Fault::Note))).expect("injected report");
        assert_eq!(bad.rate(), RED_RATE);
    }

    #[test]
    fn t48_four_routes_agree() {
        assert!(report_agrees_with_matrix());
        let four = inject_four();
        assert_eq!(four.len(), 4);
        let mut i = 0usize;
        while i < four.len() {
            assert_eq!(four.get(i), Some(&true));
            i += 1;
        }
    }

    #[test]
    fn t49_gate_walks_all_stages_in_order() {
        let root = Gate::full_pass().expect("monotone walk");
        assert_ne!(root, 0);
        let mut g = Gate::new();
        assert_eq!(g.close(), Err(VB_ESTAGE));
        let table = Stage::all();
        let mut i = 0usize;
        while i < table.len() {
            assert_eq!(g.step(*table.get(i).expect("stage")), Ok(()));
            i += 1;
        }
        assert_eq!(g.steps(), STAGE_COUNT);
        assert_eq!(g.stage(), Some(Stage::Exit));
        let h = g.close().expect("close");
        assert_ne!(h, 0);
        assert!(g.is_closed());
        assert!(g.render().contains("closed=true"));
    }

    #[test]
    fn t50_gate_refuses_regression_and_reuse() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Code), Ok(()));
        assert_eq!(g.step(Stage::Value), Err(VB_ESTAGE));
        assert_eq!(g.refusals(), 1);
        assert_eq!(g.steps(), 1);
        assert_eq!(g.stage(), Some(Stage::Code));
        assert_eq!(g.step(Stage::Code), Ok(()));
        assert_eq!(g.refusals(), 1);
        let mut closed = Gate::walk(&Stage::all()).expect("walk");
        assert!(closed.close().is_ok());
        assert_eq!(closed.step(Stage::Exit), Err(VB_ESTATE));
        assert_eq!(closed.close(), Err(VB_ESTATE));
        assert_eq!(Gate::walk(&[Stage::Exit, Stage::Option]), Err(VB_ESTAGE));
        assert_eq!(Gate::new().root(), FNV_OFF);
    }

    #[test]
    fn t51_ledger_shape_and_audit() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.ok_count(), STAT_CELLS + 1);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.first_mismatch(), None);
        assert_eq!(l.root(), 0);
        assert!(l.sample(SAMPLES).is_none());
        let last = l.sample(SAMPLES - 1).expect("last cell");
        assert_eq!(last.code, VB_EDEFAULT);
    }

    #[test]
    fn t52_ledger_refuses_misplaced_slots() {
        let mut l = Ledger::new();
        let stray = Sample {
            slot: 1,
            op: OP_OPTION,
            ok: true,
            code: 0,
            value: 0,
        };
        assert_eq!(l.register(stray), Err(VB_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert!(l.is_empty());
        let first = Sample {
            slot: 0,
            op: OP_DISPATCH,
            ok: true,
            code: 0,
            value: 1,
        };
        assert_eq!(l.register(first), Ok(()));
        assert_eq!(l.register(first), Err(VB_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn t53_ledger_freezes_replays_and_locks() {
        let mut l = build_ledger();
        let h = l.freeze().expect("freeze");
        assert_ne!(h, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), h);
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(VB_ESTATE));
        assert_eq!(l.poke(0, 1), Err(VB_ESTATE));
        let mut broken = build_ledger_broken(0);
        assert!(broken.freeze().is_err());
        assert!(!broken.is_frozen());
    }

    #[test]
    fn t54_ledger_poke_and_broken_ledgers() {
        let mut l = build_ledger();
        assert_eq!(l.poke(0, 1), Ok(()));
        assert_eq!(l.audit(), Err(VB_EVALUE));
        assert_eq!(l.first_mismatch(), Some((0, "stat-value")));
        assert_eq!(l.poke(SAMPLES, 1), Err(VB_ESLOT));
        let mut w = 0u8;
        while w < 4u8 {
            let b = build_ledger_broken(w);
            assert_eq!(b.len(), SAMPLES);
            assert!(b.audit().is_err());
            assert!(b.first_mismatch().is_some());
            w += 1;
        }
        assert_eq!(build_ledger_broken(0).audit(), Err(VB_EVALUE));
        assert_eq!(build_ledger_broken(1).audit(), Err(VB_EVALUE));
        assert_eq!(build_ledger_broken(2).audit(), Err(VB_EVALUE));
        assert_eq!(build_ledger_broken(3).audit(), Err(VB_EVALUE));
        assert_eq!(build_ledger_broken(9).audit(), Ok(()));
    }

    #[test]
    fn t55_ledger_render_is_stable() {
        let l = build_ledger();
        assert_eq!(l.render(), "ledger len=14 ok=8 err=6 rejects=0 frozen=false");
        let mut f = l;
        assert!(f.freeze().is_ok());
        assert_eq!(f.render(), "ledger len=14 ok=8 err=6 rejects=0 frozen=true");
        let one = Sample {
            slot: 0,
            op: OP_DISPATCH,
            ok: true,
            code: 0,
            value: STAGE_COUNT,
        };
        assert!(one.render().starts_with("slot 0 op 01 true code 00000 value 7"));
    }

    #[test]
    fn t56_vals_pin_the_red_cell() {
        assert_eq!(VALS.get(0), Some(&STAGE_COUNT));
        assert_eq!(VALS.get(1), Some(&(OPT_JUDGED as u32)));
        assert_eq!(VALS.get(2), Some(&(MESSAGE_CELLS as u32)));
        assert_eq!(VALS.get(3), Some(&(CORPUS as u32)));
        assert_eq!(VALS.get(4), Some(&(ACCEPT as u32)));
        assert_eq!(VALS.get(5), Some(&(REJECT as u32)));
        assert_eq!(VALS.get(6), Some(&RATE_FULL));
        assert_eq!(VALS.get(7), Some(&929));
        assert_eq!(head_val(), STAGE_COUNT);
        assert_eq!(STAT_OPS.len(), STAT_CELLS);
        assert_eq!(OPID_STAT_RED, 0x08);
        assert_eq!(DEFECT_OP_BASE, 0x21);
    }

    #[test]
    fn t57_checks_are_thirteen_and_green() {
        let s = run_unx11670_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
        assert_eq!(s.get(0).map(|c| c.name), Some("VB01-名称与段基址"));
        assert_eq!(s.get(12).map(|c| c.name), Some("VB13-冻结重放四注入"));
        assert_eq!(s.get(13), None);
        let mut i = 0usize;
        while i < s.len() {
            if let Some(c) = s.get(i) {
                assert!(c.passed);
                assert_eq!(c.detail, "static");
            }
            i += 1;
        }
    }

    #[test]
    fn t58_text_helpers_are_exact() {
        assert_eq!(seg_text(SEG_BASE), "0x06d220");
        assert_eq!(u32_text(0), "0");
        assert_eq!(u32_text(929), "929");
        assert_eq!(dec_text(4294967295), "4294967295");
        assert_eq!(hex_text(0x0d, 2), "0d");
        assert_eq!(hex_text(VB_EBAL - SEG_BASE, 2), "0d");
        assert_eq!(i32_text(-127), "-127");
        assert_eq!(i32_text(127), "127");
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
        assert_eq!(permille_text(929), "929permille");
        assert_eq!(lines_of("a\nb\n"), 2);
        assert_eq!(bytes_of("abc"), 3);
    }

    #[test]
    fn t59_shape_validity_rules() {
        assert!(Shape::blank().valid());
        assert!(enum_shape(Opt::LsColor).valid());
        assert!(enum_utf8(Opt::LsSort).valid());
        assert!(!Shape {
            family: Family::CoreUtf8,
            ..Shape::blank()
        }
        .valid());
        assert!(!Shape {
            opt: Opt::LsSort,
            family: Family::Busybox,
            value: "bogus",
            ..Shape::blank()
        }
        .valid());
        assert!(!Shape {
            family: Family::Busybox,
            truncated: true,
            ..Shape::blank()
        }
        .valid());
        assert!(!Shape {
            exit_declared: 200,
            ..Shape::blank()
        }
        .valid());
    }

    #[test]
    fn t60_define_exit_declared_lie_is_inconsistent() {
        let s = Shape {
            exit_declared: EXIT_OK,
            ..enum_shape(Opt::LsColor)
        };
        let o = define(&s);
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.code, EXIT_USAGE as i32);
        assert!(!o.consistent);
        assert!(o.refused());
        assert!(!o.diag_ok());
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    #[test]
    fn t61_seals_are_sensitive_to_every_dimension() {
        let base = Shape::blank().seal();
        assert_eq!(base, Shape::blank().seal());
        assert_ne!(base, Shape::blank().with_value("12x").seal());
        assert_ne!(base, Shape::blank().with_opt(Opt::TailN).seal());
        assert_ne!(base, Shape::blank().with_family(Family::Busybox).seal());
        let o = define(&Shape::blank());
        assert_eq!(o.seal(), define(&Shape::blank()).seal());
        // 判据的粒度是"行数"，两个不同的垃圾数值落在同一格：结论必须一样。
        assert_eq!(o.seal(), define(&Shape::blank().with_value("12x")).seal());
        // 换一个种类（枚举）就必须换结论。
        assert_ne!(o.seal(), define(&enum_shape(Opt::LsSort)).seal());
        let c = case_at(0).expect("slot 0");
        assert_eq!(c.seal(), case_at(0).expect("slot 0").seal());
        assert_ne!(c.seal(), case_at(1).expect("slot 1").seal());
        assert_eq!(c.tag(), "f11670-head-n-abc");
        assert_ne!(c.claim.seal(), case_at(8).expect("slot 8").claim.seal());
    }

    #[test]
    fn t62_want_tables_match_the_live_corpus() {
        let mut i = 0usize;
        while i < CORPUS {
            let a = out(i);
            assert_eq!(WANT_STAGE.get(i), Some(&a.stage));
            assert_eq!(WANT_CODE.get(i), Some(&(a.code as u32)));
            assert_eq!(WANT_VALUE.get(i), Some(&a.value));
            assert_eq!(WANT_NOTE.get(i), Some(&a.note));
            assert_eq!(WANT_DEFAULT.get(i), Some(&a.default));
            assert_eq!(WANT_SUCCESS.get(i), Some(&a.ok()));
            i += 1;
        }
        assert_eq!(WANT_STAGE.len(), CORPUS);
        assert_eq!(WANT_SUCCESS.len(), CORPUS);
    }
}




