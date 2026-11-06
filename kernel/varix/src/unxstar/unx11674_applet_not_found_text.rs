//! UNX-F11674 未知 applet 文案逐字判据（`applet not found`）
//!
//! 本卡只判一件事：**argv 里那个名字查不到时，程序打出的那一行字与退出的那一档码**。
//! 它不管"选项值非法"（F11670），不管"未知选项"（F11669），不管"帮助正文"（F11668）：
//! 本卡的现场永远是"名字整条不在表里"。
//!
//! # 闸门次序（秩从 1 起，只许前进）
//!
//! 1. `Dispatch` —— 这次调用走的是哪条入口：`busybox <名>`（多调用参数）/
//!    `<名>`（argv[0] 是符号链接）/ `coreutils <名>` / 说不清（兜底）。
//! 2. `Name`     —— 名字怎么取：`argv[0]` 的**basename**、多调用参数原样、
//!    空串（用多调用二进制名顶替）。路径前缀必须在这一步被剥掉。
//! 3. `Lookup`   —— 名字在不在表里：在本卡管辖的 applet 集合里 → 不是本卡现场
//!    （兜底位为真）；不在 → 继续走。
//! 4. `Message`  —— 取那一格逐字模板：`<名>: applet not found`；截断档只留首行。
//! 5. `Channel`  —— 这一行恒走 **stderr**；打到 stdout 就是通道误读。
//! 6. `Code`     —— 码档：busybox 退 **127**；coreutils 形状退 **1**。
//! 7. `Exit`     —— 结算注解档并封账。
//!
//! 这条纵线为什么不能倒：先定入口才谈得上名字怎么取，先取到名字才查得了表，
//! 查完表才拼文案，拼完才有通道与码档。允许倒退就会出现"先写 127 再回头凑名字"——
//! 一个 `coreutils` 形状被回填成 busybox 的 127 档，本卡判据当场失效。
//!
//! # 关键差异（三条线，逐字分开）
//!
//! * **busybox 符号链接入口**：`<basename>: applet not found`（一行，stderr，退 **127**）。
//!   argv[0] 里的目录前缀**不出现**在文案里——`/opt/bin/nosuch` 打的是 `nosuch: ...`。
//! * **busybox 多调用参数**：`busybox nosuch` 打 `<原样名>: applet not found`，同样退 127。
//! * **空名**：`busybox ''` 没有名字可主语，文案回落到多调用二进制名：
//!   `busybox: applet not found`（仍是一行，仍退 127）。
//! * **coreutils 形状**：`coreutils: '<名>' is not a coreutils command` 加一行
//!   `Try 'coreutils --help' for more information.`（两行，stderr，退 **1**）——
//!   这一档与 busybox 差在"码"与"行数"两个维度上，绝不许混记。
//! * **名字里有非 ASCII**：逐字节原样嵌入，不做转义、不截断。
//!
//! 判据原文：`UNX-F11674-J1 文案模板逐字判据绿（名前缀呈现，127 档）`。

use crate::alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 段基址与锚点
// ---------------------------------------------------------------------------

/// 本卡段基址（分配表钉死，不许改）。
pub const SEG_BASE: u32 = 0x6d270;

/// 前一张卡（F11672 install 降级）的段基址。
pub const ANCHOR_F11672_SEG: u32 = 0x6d250;

/// 本卡与前卡的间距。
pub const ANCHOR_F11672_GAP: u32 = 0x20;

/// 后一张卡（F11678）的段基址。
pub const ANCHOR_F11678_SEG: u32 = 0x6d2d0;

/// 本卡与后卡的间距。
pub const ANCHOR_F11678_GAP: u32 = 0x60;

/// 锚点：F11668（帮助逐字）。
pub const ANCHOR_F11668_SEG: u32 = 0x6d1f0;

/// 锚点：F11670（非法值）。
pub const ANCHOR_F11670_SEG: u32 = 0x6d220;

// ---------------------------------------------------------------------------
// 规模常量
// ---------------------------------------------------------------------------

/// 语料槽数（八活六死）。
pub const CORPUS: usize = 14;

/// 台账格数。
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

/// 统计格数。
pub const STAT_CELLS: usize = 7;

/// 红档率标定值。
pub const RED_RATE: u32 = 929;

/// 满一致率千分比。
pub const RATE_FULL: u32 = 1000;

/// 入口数。
pub const ENTRY_COUNT: usize = 4;

/// 进本卡判的入口数（`Unknown` 只作兜底）。
pub const ENTRY_JUDGED: usize = 3;

/// 名字形态数（裸名 / 带路径 / 空名）。
pub const FORM_COUNT: usize = 3;

/// 文案表格数：三条入口 × 名字形态的可落组合 + coreutils 一格 = 5 格。
pub const MESSAGE_CELLS: usize = 5;

/// 名字长度上限（超过就不是本卡现场）。
pub const NAME_CAP: u32 = 255;

/// 成功码（查表查到时的正常退出）。
pub const EXIT_OK: u32 = 0;

/// coreutils 形状的未知命令码。
pub const EXIT_FAIL: u32 = 1;

/// busybox 的未知 applet 码（本卡的正锚）。
pub const EXIT_NOTFOUND: u32 = 127;

/// 本卡的正锚模板尾巴。
pub const NOT_FOUND_TAIL: &str = ": applet not found";

/// 多调用二进制名（空名时的主语）。
pub const MULTICALL_NAME: &str = "busybox";

/// coreutils 的模板头。
pub const CORE_HEAD: &str = "coreutils: '";

/// coreutils 的模板尾。
pub const CORE_TAIL: &str = "' is not a coreutils command";

/// coreutils 的 Try 行。
pub const CORE_TRY: &str = "Try 'coreutils --help' for more information.";

/// 本卡的占位名（语料里的"查不到"）。
pub const NOSUCH: &str = "nosuchapplet";

/// 带目录前缀的 argv[0]。
pub const NOSUCH_PATH: &str = "/opt/bin/nosuchapplet";

/// 带点号的怪名。
pub const DOTTED: &str = "no.such";

/// 非 ASCII 名。
pub const UTF8_NAME: &str = "h\u{e9}llo";

/// 长名（64 字）。
pub const LONG_NAME: &str = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijkl";

// ---------------------------------------------------------------------------
// 十三码
// ---------------------------------------------------------------------------

/// 成败互报。
pub const NF_ESTATE: u32 = SEG_BASE + 0x01;

/// 退出码档错。
pub const NF_ECODE: u32 = SEG_BASE + 0x02;

/// 落点闸错。
pub const NF_ESTAGE: u32 = SEG_BASE + 0x03;

/// 行数（正文量）错。
pub const NF_EVALUE: u32 = SEG_BASE + 0x04;

/// 文案家族档错。
pub const NF_ENOTE: u32 = SEG_BASE + 0x05;

/// 兜底锚错。
pub const NF_EDEFAULT: u32 = SEG_BASE + 0x06;

/// 文案表缺格/串格。
pub const NF_EBODY: u32 = SEG_BASE + 0x07;

/// 通道错。
pub const NF_ECHAN: u32 = SEG_BASE + 0x08;

/// 槽位越界。
pub const NF_ESLOT: u32 = SEG_BASE + 0x09;

/// 指纹为零。
pub const NF_ESEAL: u32 = SEG_BASE + 0x0a;

/// 无观测。
pub const NF_ENOBS: u32 = SEG_BASE + 0x0b;

/// 矩阵不齐。
pub const NF_EMATRIX: u32 = SEG_BASE + 0x0c;

/// 账目不平。
pub const NF_EBAL: u32 = SEG_BASE + 0x0d;

/// 十三码全集。
pub const ALL_CODES: [u32; 13] = [
    NF_ESTATE,
    NF_ECODE,
    NF_ESTAGE,
    NF_EVALUE,
    NF_ENOTE,
    NF_EDEFAULT,
    NF_EBODY,
    NF_ECHAN,
    NF_ESLOT,
    NF_ESEAL,
    NF_ENOBS,
    NF_EMATRIX,
    NF_EBAL,
];

// ---------------------------------------------------------------------------
// 算子号
// ---------------------------------------------------------------------------

/// 闸 1 dispatch。
pub const OP_DISPATCH: u8 = 0x01;

/// 闸 2 name。
pub const OP_NAME: u8 = 0x02;

/// 闸 3 lookup。
pub const OP_LOOKUP: u8 = 0x03;

/// 闸 4 message。
pub const OP_MESSAGE: u8 = 0x04;

/// 闸 5 channel。
pub const OP_CHANNEL: u8 = 0x05;

/// 闸 6 code。
pub const OP_CODE: u8 = 0x06;

/// 闸 7 exit。
pub const OP_EXIT: u8 = 0x07;

/// 七个统计格的算子。
pub const STAT_OPS: [u8; 7] = [
    OP_DISPATCH,
    OP_NAME,
    OP_LOOKUP,
    OP_MESSAGE,
    OP_CHANNEL,
    OP_CODE,
    OP_EXIT,
];

/// 红档统计格算子。
pub const OPID_STAT_RED: u8 = 0x08;

/// 缺陷格算子基号。
pub const DEFECT_OP_BASE: u8 = 0x21;

// ---------------------------------------------------------------------------
// FNV-1a
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

/// 折一个 i32。
fn fold_i32(h: u64, v: i32) -> u64 {
    fold_word(h, v as u32 as u64)
}

/// 折一个布尔。
fn fold_bool(h: u64, b: bool) -> u64 {
    fold_byte(h, if b { 1u8 } else { 0u8 })
}

/// 折一串文本。
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

/// u32 → 十进制文本。
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

/// 段址文本。
pub fn seg_text(v: u32) -> String {
    let mut s = String::from("0x");
    s.push_str(hex_text(v, 6).as_str());
    s
}

/// 无符号数文本对外口。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// 有符号数文本。
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

/// 行数（按 `\n` 计）。
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

/// 字节数。
fn bytes_of(s: &str) -> u32 {
    s.as_bytes().len() as u32
}

/// 取 basename（最后一个 `/` 之后；没有 `/` 就是全串）。
pub fn basename(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut cut = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'/' {
                cut = i + 1;
            }
        }
        i += 1;
    }
    match s.get(cut..b.len()) {
        Some(x) => x,
        None => "",
    }
}

/// 名字是否可作主语：非空且不超过上限。
pub fn name_ok(n: &str) -> bool {
    let l = bytes_of(n);
    l > 0 && l <= NAME_CAP
}

// ---------------------------------------------------------------------------
// 域模型
// ---------------------------------------------------------------------------

/// 这次调用打哪儿进来。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// `<名> ...`：argv[0] 是指向多调用二进制的符号链接。
    Symlink,
    /// `busybox <名> ...`：名字是第一个参数。
    Arg,
    /// `coreutils <名> ...`：coreutils 的多调用形状。
    CoreArg,
    /// 说不清（兜底）。
    Unknown,
}

impl Entry {
    /// 入口名。
    pub fn name(self) -> &'static str {
        match self {
            Entry::Symlink => "symlink",
            Entry::Arg => "arg",
            Entry::CoreArg => "core-arg",
            Entry::Unknown => "unknown",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Entry::Symlink => 0usize,
            Entry::Arg => 1usize,
            Entry::CoreArg => 2usize,
            Entry::Unknown => 3usize,
        }
    }

    /// 按序号取。
    pub fn at(i: usize) -> Option<Entry> {
        match i {
            0usize => Some(Entry::Symlink),
            1usize => Some(Entry::Arg),
            2usize => Some(Entry::CoreArg),
            3usize => Some(Entry::Unknown),
            _ => None,
        }
    }

    /// 全集。
    pub fn all() -> [Entry; ENTRY_COUNT] {
        [Entry::Symlink, Entry::Arg, Entry::CoreArg, Entry::Unknown]
    }

    /// 是否进本卡判。
    pub fn judged(self) -> bool {
        matches!(self, Entry::Symlink | Entry::Arg | Entry::CoreArg)
    }

    /// 名字是不是 argv[0]（要剥路径）。
    pub fn from_argv0(self) -> bool {
        matches!(self, Entry::Symlink)
    }

    /// 这条入口是不是 coreutils 形状。
    pub fn is_core(self) -> bool {
        matches!(self, Entry::CoreArg)
    }

    /// 这条入口的码档。
    pub fn exit_code(self) -> u32 {
        match self {
            Entry::CoreArg => EXIT_FAIL,
            Entry::Symlink | Entry::Arg => EXIT_NOTFOUND,
            Entry::Unknown => EXIT_FAIL,
        }
    }
}

/// 名字形态：主语怎么来的。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 裸名：`nosuchapplet`。
    Bare,
    /// 带目录前缀的 argv[0]：`/opt/bin/nosuchapplet`。
    Path,
    /// 空名：主语回落到多调用二进制名。
    Empty,
}

impl Form {
    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            Form::Bare => "bare",
            Form::Path => "path",
            Form::Empty => "empty",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Form::Bare => 0usize,
            Form::Path => 1usize,
            Form::Empty => 2usize,
        }
    }

    /// 全集。
    pub fn all() -> [Form; FORM_COUNT] {
        [Form::Bare, Form::Path, Form::Empty]
    }

    /// 这一形态要不要剥路径。
    pub fn strips_path(self) -> bool {
        matches!(self, Form::Path)
    }

    /// 这一形态的主语是不是回落的二进制名。
    pub fn falls_back(self) -> bool {
        matches!(self, Form::Empty)
    }

    /// 按序号取。
    pub fn at(i: usize) -> Option<Form> {
        match i {
            0usize => Some(Form::Bare),
            1usize => Some(Form::Path),
            2usize => Some(Form::Empty),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 通道：`applet not found` 恒走 stderr
// ---------------------------------------------------------------------------

/// 诊断落在哪条通道。
///
/// 这一行字是**红**，红走 stdout 就是通道误读：脚本里 `2>/dev/null` 之后
/// 还会看到"名字找不到"，调用方的判断全乱。本卡把"去哪条通道"单列成闸 5。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    /// 标准错误（唯一合法通道）。
    Stderr,
    /// 标准输出（通道误读）。
    Stdout,
    /// 一个字都没出（采集失败或根本没跑起来）。
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

    /// 按序号取。
    pub fn at(i: usize) -> Option<Channel> {
        match i {
            0usize => Some(Channel::Stderr),
            1usize => Some(Channel::Stdout),
            2usize => Some(Channel::Nowhere),
            _ => None,
        }
    }

    /// 这条通道是否承载字节。
    pub fn carries_bytes(self) -> bool {
        !matches!(self, Channel::Nowhere)
    }

    /// 是否诊断该走的通道。
    pub fn is_diag(self) -> bool {
        matches!(self, Channel::Stderr)
    }
}

// ---------------------------------------------------------------------------
// 阶段：七道闸，秩从 1 起
// ---------------------------------------------------------------------------

/// 阶段：本卡七道闸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 闸 1：入口（符号链接 / 多调用参数 / coreutils / 说不清）。
    Dispatch,
    /// 闸 2：名字取形（裸名 / 剥路径 / 空名回落）。
    Name,
    /// 闸 3：查表（在不在 applet 表里）。
    Lookup,
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
            Stage::Name => 2u32,
            Stage::Lookup => 3u32,
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
            Stage::Name => "name",
            Stage::Lookup => "lookup",
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
            2u32 => Some(Stage::Name),
            3u32 => Some(Stage::Lookup),
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
            Stage::Name,
            Stage::Lookup,
            Stage::Message,
            Stage::Channel,
            Stage::Code,
            Stage::Exit,
        ]
    }

    /// 这一闸之前还没有文案可对。
    pub fn before_text(self) -> bool {
        matches!(self, Stage::Dispatch | Stage::Name | Stage::Lookup)
    }

    /// 这一闸是否已经给出路点闸（未走到封账）。
    pub fn settled_gate(self) -> bool {
        !matches!(self, Stage::Exit)
    }

    /// 这一闸是否已经定下主语。
    pub fn after_name(self) -> bool {
        self.rank() > 2u32
    }
}

// ---------------------------------------------------------------------------
// 注解档：这一格该长成什么样
// ---------------------------------------------------------------------------

/// 注解档：文案家族 + 有无正文。
///
/// 五个有正文的档位对应 `MESSAGE_CELLS` 的五格；其余四档是"拒收/兜底"的四种由来。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// busybox 一行：`<名>: applet not found`，名是 ASCII 裸名。
    AppletNotFound,
    /// busybox 一行，名由 argv[0] 的 basename 剥出（路径前缀不许出现）。
    AppletNotFoundPath,
    /// busybox 一行，名里有非 ASCII 字节，逐字节原样嵌入。
    AppletNotFoundUtf8,
    /// busybox 一行，空名回落到多调用二进制名。
    AppletNotFoundEmpty,
    /// coreutils 两行：`coreutils: '<名>' is not a coreutils command` + Try 行。
    CoreNotCommand,
    /// 名字在 applet 表里：不是本卡现场，兜底档。
    KnownApplet,
    /// 采集只留首行。
    Truncated,
    /// 诊断跑到 stdout。
    Misrouted,
    /// 一个字都取不到。
    Silent,
    /// 入口说不清或名字超限：兜底档。
    Fallback,
}

impl Note {
    /// 档名（明细行逐字出现）。
    pub fn name(self) -> &'static str {
        match self {
            Note::AppletNotFound => "applet-not-found",
            Note::AppletNotFoundPath => "applet-not-found-path",
            Note::AppletNotFoundUtf8 => "applet-not-found-utf8",
            Note::AppletNotFoundEmpty => "applet-not-found-empty",
            Note::CoreNotCommand => "core-not-command",
            Note::KnownApplet => "known-applet",
            Note::Truncated => "truncated",
            Note::Misrouted => "misrouted",
            Note::Silent => "silent",
            Note::Fallback => "fallback",
        }
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Note::AppletNotFound => 0usize,
            Note::AppletNotFoundPath => 1usize,
            Note::AppletNotFoundUtf8 => 2usize,
            Note::AppletNotFoundEmpty => 3usize,
            Note::CoreNotCommand => 4usize,
            Note::KnownApplet => 5usize,
            Note::Truncated => 6usize,
            Note::Misrouted => 7usize,
            Note::Silent => 8usize,
            Note::Fallback => 9usize,
        }
    }

    /// 全集。
    pub fn all() -> [Note; Note::COUNT] {
        [
            Note::AppletNotFound,
            Note::AppletNotFoundPath,
            Note::AppletNotFoundUtf8,
            Note::AppletNotFoundEmpty,
            Note::CoreNotCommand,
            Note::KnownApplet,
            Note::Truncated,
            Note::Misrouted,
            Note::Silent,
            Note::Fallback,
        ]
    }

    /// 档数。
    pub const COUNT: usize = 10;

    /// 兜底位：这一格**没有结论**。
    pub fn defaulted(self) -> bool {
        matches!(self, Note::KnownApplet | Note::Fallback)
    }

    /// 这一格有没有可对账的诊断正文。
    pub fn has_body(self) -> bool {
        matches!(
            self,
            Note::AppletNotFound
                | Note::AppletNotFoundPath
                | Note::AppletNotFoundUtf8
                | Note::AppletNotFoundEmpty
                | Note::CoreNotCommand
        )
    }

    /// 这一格说不说法（红档面）。
    pub fn speaks_error(self) -> bool {
        matches!(
            self,
            Note::AppletNotFound
                | Note::AppletNotFoundPath
                | Note::AppletNotFoundUtf8
                | Note::AppletNotFoundEmpty
                | Note::CoreNotCommand
                | Note::Truncated
                | Note::Misrouted
        )
    }

    /// 这一格落 coreutils 线。
    pub fn is_core(self) -> bool {
        matches!(self, Note::CoreNotCommand)
    }

    /// 这一格落 busybox 线。
    pub fn is_busybox(self) -> bool {
        matches!(
            self,
            Note::AppletNotFound
                | Note::AppletNotFoundPath
                | Note::AppletNotFoundUtf8
                | Note::AppletNotFoundEmpty
        )
    }

    /// 这一格的主语是不是回落的二进制名（"没有自己的名字"档）。
    pub fn subject_fallback(self) -> bool {
        matches!(self, Note::AppletNotFoundEmpty)
    }

    /// 这一格的主语是不是由 argv[0] 剥出来的。
    pub fn subject_stripped(self) -> bool {
        matches!(self, Note::AppletNotFoundPath)
    }

    /// 这一格该不该附 Try 行。
    pub fn needs_try(self) -> bool {
        self.is_core()
    }
}

// ---------------------------------------------------------------------------
// 四档
// ---------------------------------------------------------------------------

/// 四档：按**实际结论**归档。
///
/// * `Verbatim` —— busybox 单行逐字（含剥路径与逐字节两种）。
/// * `Bannered` —— 换了家族的 coreutils 两行（多一条 Try 行）。
/// * `NoBody`   —— 主语不是查的那个名字（空名回落），单行但换了主语。
/// * `Refused`  —— 截断/兜底/通道误读，没有可对账的完整正文。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一档：busybox 一行逐字。
    Verbatim,
    /// 二档：coreutils 两行（带 Try 行）。
    Bannered,
    /// 三档：空名回落（主语换了）。
    NoBody,
    /// 四档：拒收或兜底。
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
        if o.note.is_core() {
            return Tier::Bannered;
        }
        if o.note.subject_fallback() {
            return Tier::NoBody;
        }
        Tier::Verbatim
    }
}

// ---------------------------------------------------------------------------
// 缺陷维
// ---------------------------------------------------------------------------

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
    /// 文案家族档错。
    Note,
    /// 兜底锚错。
    Default,
}

impl Fault {
    /// 本维错误码（`Ok` 给 0）。
    pub const fn code(self) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => NF_ESTATE,
            Fault::Stage => NF_ESTAGE,
            Fault::Code => NF_ECODE,
            Fault::Value => NF_EVALUE,
            Fault::Note => NF_ENOTE,
            Fault::Default => NF_EDEFAULT,
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
    NF_ESTATE,
    NF_ESTAGE,
    NF_ECODE,
    NF_EVALUE,
    NF_ENOTE,
    NF_EDEFAULT,
];

// ---------------------------------------------------------------------------
// 文案表：五格
// ---------------------------------------------------------------------------

/// 一格文案：入口 × 名字形态 → 逐字模板 + 码档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MsgCell {
    /// 这条入口。
    pub entry: Entry,
    /// 这个名字形态。
    pub form: Form,
    /// 逐字模板（`{N}` 是主语占位）。
    pub tpl: &'static str,
    /// 这一格的退出码。
    pub code: u32,
    /// 这一格应有几行。
    pub lines: u32,
}

/// busybox 一行的逐字模板。
pub const BB_TPL: &str = "{N}: applet not found\n";

/// coreutils 两行的逐字模板。
pub const CORE_TPL: &str =
    "coreutils: '{N}' is not a coreutils command\nTry 'coreutils --help' for more information.\n";

/// 缺格哨兵（不会被 `msg_cell` 命中）。
pub const CELL_NONE: MsgCell = MsgCell {
    entry: Entry::Unknown,
    form: Form::Bare,
    tpl: "",
    code: EXIT_FAIL,
    lines: 0,
};

/// 五格文案表：三条入口 × 名字形态里真正会落地的组合。
pub const MSG_TABLE: [MsgCell; MESSAGE_CELLS] = [
    // 0 符号链接 + 裸名：`nosuchapplet: applet not found`。
    MsgCell {
        entry: Entry::Symlink,
        form: Form::Bare,
        tpl: BB_TPL,
        code: EXIT_NOTFOUND,
        lines: 1,
    },
    // 1 符号链接 + 带路径 argv[0]：主语先剥成 basename，仍是一行。
    MsgCell {
        entry: Entry::Symlink,
        form: Form::Path,
        tpl: BB_TPL,
        code: EXIT_NOTFOUND,
        lines: 1,
    },
    // 2 多调用参数 + 裸名：`busybox nosuchapplet`，一行。
    MsgCell {
        entry: Entry::Arg,
        form: Form::Bare,
        tpl: BB_TPL,
        code: EXIT_NOTFOUND,
        lines: 1,
    },
    // 3 多调用参数 + 空名：主语回落到 `busybox`，仍是一行。
    MsgCell {
        entry: Entry::Arg,
        form: Form::Empty,
        tpl: BB_TPL,
        code: EXIT_NOTFOUND,
        lines: 1,
    },
    // 4 coreutils 形状：两行（第二行是 Try 行），码档降到 1。
    MsgCell {
        entry: Entry::CoreArg,
        form: Form::Bare,
        tpl: CORE_TPL,
        code: EXIT_FAIL,
        lines: 2,
    },
];

/// 按（入口，形态）取格。
pub fn msg_cell(entry: Entry, form: Form) -> Option<&'static MsgCell> {
    let mut i = 0usize;
    while i < MESSAGE_CELLS {
        if let Some(c) = MSG_TABLE.get(i) {
            if c.entry == entry && c.form == form {
                return Some(c);
            }
        }
        i += 1;
    }
    None
}

/// 主语解析：剥路径 / 回落二进制名 / 原样。
///
/// 这一步是本卡的**关键差异**所在：
/// * `Form::Path` 只取最后一个 `/` 之后的部分——`/opt/bin/nosuchapplet` 打的是
///   `nosuchapplet: applet not found`，路径前缀绝不出现。
/// * `Form::Empty` 主语回落到多调用二进制名——`busybox ''` 打的是
///   `busybox: applet not found`，不是 `: applet not found`。
pub fn msg_name<'a>(entry: Entry, form: Form, raw: &'a str) -> &'a str {
    let _ = entry;
    if form.falls_back() {
        return MULTICALL_NAME;
    }
    if form.strips_path() {
        return basename(raw);
    }
    raw
}

/// 按模板把主语填进去（逐段 `push_str`，不碰格式化宏）。
pub fn subst(tpl: &str, name: &str) -> String {
    let mut s = String::new();
    let mut first = true;
    for part in tpl.split("{N}") {
        if !first {
            s.push_str(name);
        }
        s.push_str(part);
        first = false;
    }
    s
}

/// 这一格的完整正文。
pub fn msg_text(entry: Entry, form: Form, raw: &str) -> String {
    let cell = match msg_cell(entry, form) {
        Some(c) => c,
        None => return String::new(),
    };
    let name = msg_name(entry, form, raw);
    subst(cell.tpl, name)
}

/// 这一格的码档。
pub fn msg_code(entry: Entry, form: Form) -> u32 {
    match msg_cell(entry, form) {
        Some(c) => c.code,
        None => EXIT_FAIL,
    }
}

/// 这一格应有的行数。
pub fn msg_lines(entry: Entry, form: Form) -> u32 {
    match msg_cell(entry, form) {
        Some(c) => c.lines,
        None => 0u32,
    }
}

/// 本卡肯定不认的五个名字（全是 ASCII，长度都远低于上限）。
pub const UNKNOWN_NAMES: [&str; 5] = [NOSUCH, DOTTED, "zzz", "nope", "notanapplet"];

/// 主语表里已知的 applet（落到这些名字就不是本卡现场）。
pub const KNOWN_APPLETS: [&str; 6] = ["ls", "cat", "head", "sed", "awk", "grep"];

/// 名字是否在 applet 表里。
pub fn is_known(name: &str) -> bool {
    let mut i = 0usize;
    while i < KNOWN_APPLETS.len() {
        if let Some(x) = KNOWN_APPLETS.get(i) {
            if *x == name {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 文本是不是纯 ASCII（决定走 `AppletNotFound` 还是 `AppletNotFoundUtf8`）。
pub fn is_ascii_text(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c >= 0x80 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 文本里有没有 `/`（裸名里不该有）。
pub fn has_slash(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'/' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 只留首行（含行尾换行）。
pub fn first_line_only(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'\n' {
                out.push_str("\n");
                return out;
            }
            out.push(*c as char);
        }
        i += 1;
    }
    out
}

/// 一个格子的文案指纹（模板 + 码档 + 行数）。
pub fn cell_seal(c: &MsgCell) -> u64 {
    let mut h = FNV_OFF;
    h = fold_word(h, c.entry.index() as u64);
    h = fold_word(h, c.form.index() as u64);
    h = fold_str(h, c.tpl);
    h = fold_word(h, c.code as u64);
    fold_word(h, c.lines as u64)
}

/// 文案表整体指纹。
pub fn table_seal() -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < MESSAGE_CELLS {
        if let Some(c) = MSG_TABLE.get(i) {
            h = fold_word(h, cell_seal(c));
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 形状 / 结论 / 申报 / 一槽
// ---------------------------------------------------------------------------

/// 一次现场：从哪儿进来、名字长什么样、采集到了什么。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 入口。
    pub entry: Entry,
    /// 名字形态。
    pub form: Form,
    /// argv 里那个名字（原样，不预先剥路径）。
    pub name: &'static str,
    /// 诊断落在哪条通道。
    pub channel: Channel,
    /// 采集记下的退出码。
    pub exit_declared: u32,
    /// 采集是否只留了首行。
    pub truncated: bool,
    /// locale 是不是 UTF-8（只对 coreutils 有约束：那边文案恒 ASCII）。
    pub lang_utf8: bool,
}

impl Shape {
    /// 基型：`nosuchapplet` 走符号链接入口，stderr，退 127。
    pub const fn blank() -> Shape {
        Shape {
            entry: Entry::Symlink,
            form: Form::Bare,
            name: NOSUCH,
            channel: Channel::Stderr,
            exit_declared: EXIT_NOTFOUND,
            truncated: false,
            lang_utf8: false,
        }
    }

    /// 形状整体是否自洽。不自洽只把 `consistent` 压假，不改变落点。
    pub fn valid(&self) -> bool {
        if self.exit_declared > EXIT_NOTFOUND {
            return false;
        }
        // 路径只可能来自 argv[0]：别的入口没地方带目录前缀。
        if self.form.strips_path() && !self.entry.from_argv0() {
            return false;
        }
        // 空名只出现在多调用参数入口：`busybox ''`。
        if self.form.falls_back() && self.entry != Entry::Arg {
            return false;
        }
        // coreutils 只有"参数式"入口，没有 argv[0] 形态。
        if self.entry == Entry::CoreArg && self.form != Form::Bare {
            return false;
        }
        // 截断只可能是两行以上的 coreutils 文案被切断。
        if self.truncated && !self.entry.is_core() {
            return false;
        }
        // 裸名里不许有 `/`：有 `/` 就必须走剥路径形态。
        if has_slash(self.name) != self.form.strips_path() {
            return false;
        }
        // 空名形态必须真的空，非空形态必须真的有名字。
        if self.form.falls_back() != (bytes_of(self.name) == 0) {
            return false;
        }
        // coreutils 的文案恒用 ASCII 单引号，locale 改不了它。
        if self.lang_utf8 && self.entry.is_core() {
            return false;
        }
        true
    }

    /// 只换入口。
    pub fn with_entry(&self, e: Entry) -> Shape {
        let mut s = *self;
        s.entry = e;
        s
    }

    /// 只换形态。
    pub fn with_form(&self, f: Form) -> Shape {
        let mut s = *self;
        s.form = f;
        s
    }

    /// 只换名字。
    pub fn with_name(&self, n: &'static str) -> Shape {
        let mut s = *self;
        s.name = n;
        s
    }

    /// 只换通道。
    pub fn with_channel(&self, c: Channel) -> Shape {
        let mut s = *self;
        s.channel = c;
        s
    }

    /// 只换申报退出码。
    pub fn with_exit(&self, c: u32) -> Shape {
        let mut s = *self;
        s.exit_declared = c;
        s
    }

    /// 只换截断标记。
    pub fn with_truncated(&self, t: bool) -> Shape {
        let mut s = *self;
        s.truncated = t;
        s
    }

    /// 本形状解析出来的主语。
    pub fn subject(&self) -> &'static str {
        msg_name(self.entry, self.form, self.name)
    }

    /// 本形状应打出的正文。
    pub fn text(&self) -> String {
        msg_text(self.entry, self.form, self.name)
    }

    /// 形状指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.entry.index() as u64);
        h = fold_word(h, self.form.index() as u64);
        h = fold_str(h, self.name);
        h = fold_word(h, self.channel.index() as u64);
        h = fold_word(h, self.exit_declared as u64);
        h = fold_bool(h, self.truncated);
        fold_bool(h, self.lang_utf8)
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
    /// 文案家族档。
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
            return EXIT_FAIL;
        }
        self.code as u32
    }

    /// 诊断是否成立（有正文 + 码非零 + 自洽）。
    pub fn diag_ok(&self) -> bool {
        self.consistent && self.code != 0 && self.note.has_body() && self.value > 0
    }

    /// 这一格的码是不是 busybox 的 127 档。
    pub fn is_notfound_code(&self) -> bool {
        self.code == EXIT_NOTFOUND as i32
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
    /// 申报的文案家族档。
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
            tag: "f11674-blank",
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
///
/// 七道闸只许前进：
/// 1. `Dispatch`：入口说不清 → 兜底。
/// 2. `Name`：主语解析（剥路径 / 回落二进制名）后必须非空且不超上限。
/// 3. `Lookup`：名字在 applet 表里 → 不是本卡现场（兜底）。
/// 4. `Message`：取格、截断复核（截断一律拒收）。
/// 5. `Channel`：不是 stderr → 通道误读，拒收。
/// 6. `Code`：采集码与格里的码不符 → 压假自洽性。
/// 7. `Exit`：定注解档并封账。
pub fn define(s: &Shape) -> Outcome {
    let mut consistent = s.valid();

    // --- 闸 1：dispatch ----------------------------------------------------
    if !s.entry.judged() {
        // 入口说不清：兜底档，没有可对账的正文。
        return Outcome::make(Stage::Dispatch, EXIT_FAIL, 0, Note::Fallback, true, consistent);
    }

    // --- 闸 2：name --------------------------------------------------------
    let subject = msg_name(s.entry, s.form, s.name);
    let subject_len = bytes_of(subject);
    if subject_len == 0 {
        // 连主语都填不出来：模板没法成立，拒收。
        return Outcome::make(Stage::Name, EXIT_FAIL, 0, Note::Silent, false, false);
    }
    if subject_len > NAME_CAP {
        // 超过名字上限：不是本卡现场，兜底。
        return Outcome::make(Stage::Name, EXIT_FAIL, 0, Note::Fallback, true, consistent);
    }

    // --- 闸 3：lookup ------------------------------------------------------
    if is_known(subject) {
        // 名字在 applet 表里：程序会正常跑，本卡管不着——兜底位为真。
        return Outcome::make(Stage::Lookup, EXIT_OK, 0, Note::KnownApplet, true, consistent);
    }

    // --- 闸 4：message -----------------------------------------------------
    let cell = match msg_cell(s.entry, s.form) {
        Some(c) => c,
        None => return Outcome::make(Stage::Message, EXIT_FAIL, 0, Note::Silent, true, false),
    };
    let text = msg_text(s.entry, s.form, s.name);
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
        return Outcome::make(Stage::Channel, cell.code, lines, Note::Misrouted, false, false);
    }

    // --- 闸 6：code --------------------------------------------------------
    if s.exit_declared != cell.code {
        consistent = false;
    }

    // --- 闸 7：exit --------------------------------------------------------
    let note = if s.entry.is_core() {
        Note::CoreNotCommand
    } else if s.form.falls_back() {
        Note::AppletNotFoundEmpty
    } else if s.form.strips_path() {
        Note::AppletNotFoundPath
    } else if is_ascii_text(subject) {
        Note::AppletNotFound
    } else {
        Note::AppletNotFoundUtf8
    };
    Outcome::make(Stage::Exit, cell.code, lines, note, false, consistent)
}

// ---------------------------------------------------------------------------
// 语料：八活六死，逐槽手写
// ---------------------------------------------------------------------------

/// 超过名字上限的怪名（300 字）。
pub const OVERLONG_NAME: &str = "abcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghijabcdefghij";

/// 语料现场表。
pub const SHAPES: [Shape; CORPUS] = [
    // 0 母版锚：符号链接入口，裸名 `nosuchapplet`，stderr，退 127。
    Shape {
        ..Shape::blank()
    },
    // 1 多调用参数入口：`busybox nosuchapplet`，文案一模一样。
    Shape {
        entry: Entry::Arg,
        form: Form::Bare,
        name: NOSUCH,
        channel: Channel::Stderr,
        exit_declared: EXIT_NOTFOUND,
        truncated: false,
        lang_utf8: false,
    },
    // 2 带路径 argv[0]：`/opt/bin/nosuchapplet`，主语必须剥成 basename。
    Shape {
        entry: Entry::Symlink,
        form: Form::Path,
        name: NOSUCH_PATH,
        channel: Channel::Stderr,
        exit_declared: EXIT_NOTFOUND,
        truncated: false,
        lang_utf8: false,
    },
    // 3 空名：`busybox ''`，主语回落到多调用二进制名。
    Shape {
        entry: Entry::Arg,
        form: Form::Empty,
        name: "",
        channel: Channel::Stderr,
        exit_declared: EXIT_NOTFOUND,
        truncated: false,
        lang_utf8: false,
    },
    // 4 coreutils 形状：两行，退 1。
    Shape {
        entry: Entry::CoreArg,
        form: Form::Bare,
        name: NOSUCH,
        channel: Channel::Stderr,
        exit_declared: EXIT_FAIL,
        truncated: false,
        lang_utf8: false,
    },
    // 5 带点号的怪名：点号不是分隔符，只影响查表结果。
    Shape {
        name: DOTTED,
        ..Shape::blank()
    },
    // 6 长名（64 字）：仍在 255 上限之内。
    Shape {
        name: LONG_NAME,
        ..Shape::blank()
    },
    // 7 非 ASCII 名：逐字节原样嵌入，不做转义。
    Shape {
        name: UTF8_NAME,
        ..Shape::blank()
    },
    // 8 反例（成败互报）：真值红，申报说绿。
    Shape {
        ..Shape::blank()
    },
    // 9 反例（落点闸错）：申报落在 Value 闸。
    Shape {
        ..Shape::blank()
    },
    // 10 反例（码档错）：申报 1，真值 127。
    Shape {
        entry: Entry::Arg,
        form: Form::Bare,
        name: NOSUCH,
        channel: Channel::Stderr,
        exit_declared: EXIT_NOTFOUND,
        truncated: false,
        lang_utf8: false,
    },
    // 11 反例（行数错）：申报两行，真值一行。
    Shape {
        ..Shape::blank()
    },
    // 12 反例（家族档错）：coreutils 两行被截成首行，真值拒收。
    Shape {
        entry: Entry::CoreArg,
        form: Form::Bare,
        name: NOSUCH,
        channel: Channel::Stderr,
        exit_declared: EXIT_FAIL,
        truncated: true,
        lang_utf8: false,
    },
    // 13 反例（兜底锚错）：入口说不清，真值落兜底。
    Shape {
        entry: Entry::Unknown,
        form: Form::Bare,
        name: NOSUCH,
        channel: Channel::Stderr,
        exit_declared: EXIT_FAIL,
        truncated: false,
        lang_utf8: false,
    },
];

/// 语料申报表（八活六死；反例各自只错一维）。
pub const CLAIMS: [Claim; CORPUS] = [
    // 0 母版：一对一。
    Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFound, false),
    // 1 多调用参数：文案与母版一致。
    Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFound, false),
    // 2 剥路径：注解档单列，行数仍是一。
    Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFoundPath, false),
    // 3 空名回落：注解档单列，主语是二进制名。
    Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFoundEmpty, false),
    // 4 coreutils：两行，退 1。
    Claim::make(false, Stage::Exit, 1, 2, Note::CoreNotCommand, false),
    // 5 点号怪名：按普通裸名处理。
    Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFound, false),
    // 6 长名：按普通裸名处理。
    Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFound, false),
    // 7 非 ASCII：逐字节档。
    Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFoundUtf8, false),
    // 8 反例：把红说成绿 —— 只错 Estate 一维。
    Claim::make(true, Stage::Exit, 127, 1, Note::AppletNotFound, false),
    // 9 反例：落点闸说错 —— 只错 Stage 一维。
    Claim::make(false, Stage::Message, 127, 1, Note::AppletNotFound, false),
    // 10 反例：码档说错 —— 只错 Code 一维。
    Claim::make(false, Stage::Exit, 1, 1, Note::AppletNotFound, false),
    // 11 反例：行数说错 —— 只错 Value 一维。
    Claim::make(false, Stage::Exit, 127, 2, Note::AppletNotFound, false),
    // 12 反例：家族档说错 —— 只错 Note 一维（真值是截断）。
    Claim::make(false, Stage::Message, 1, 1, Note::CoreNotCommand, false),
    // 13 反例：把兜底当结论 —— 只错 Default 一维。
    Claim::make(false, Stage::Dispatch, 1, 0, Note::Fallback, true),
];

// ---------------------------------------------------------------------------
// 期望表：逐槽钉死真值
// ---------------------------------------------------------------------------

/// 期望落点闸。
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
    Stage::Message,
    Stage::Dispatch,
];

/// 期望退出码。
pub const WANT_CODE: [u32; CORPUS] = [
    EXIT_NOTFOUND,
    EXIT_NOTFOUND,
    EXIT_NOTFOUND,
    EXIT_NOTFOUND,
    EXIT_FAIL,
    EXIT_NOTFOUND,
    EXIT_NOTFOUND,
    EXIT_NOTFOUND,
    EXIT_NOTFOUND,
    EXIT_NOTFOUND,
    EXIT_NOTFOUND,
    EXIT_NOTFOUND,
    EXIT_FAIL,
    EXIT_FAIL,
];

/// 期望正文行数。
pub const WANT_VALUE: [u32; CORPUS] = [1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 0];

/// 期望注解档。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::AppletNotFound,
    Note::AppletNotFound,
    Note::AppletNotFoundPath,
    Note::AppletNotFoundEmpty,
    Note::CoreNotCommand,
    Note::AppletNotFound,
    Note::AppletNotFound,
    Note::AppletNotFoundUtf8,
    Note::AppletNotFound,
    Note::AppletNotFound,
    Note::AppletNotFound,
    Note::AppletNotFound,
    Note::Truncated,
    Note::Fallback,
];

/// 期望兜底位（只有末槽真落兜底）。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, false, false, false, false, false,
    true,
];

/// 反例区期望的缺陷维位次（六维各一）。
pub const WANT_NDX: [usize; CORPUS] = [6, 6, 6, 6, 6, 6, 6, 6, 0, 1, 2, 3, 4, 5];

/// 期望申报成败（只有第 8 槽那个反例把红说成绿）。
pub const WANT_SUCCESS: [bool; CORPUS] = [
    false, false, false, false, false, false, false, false, true, false, false, false, false,
    false,
];

/// 逐槽短名。
pub const WANT_TAGS: [&str; CORPUS] = [
    "f11674-symlink-bare",
    "f11674-arg-bare",
    "f11674-symlink-path",
    "f11674-arg-empty",
    "f11674-core-arg",
    "f11674-dotted",
    "f11674-long-name",
    "f11674-utf8-name",
    "f11674-lie-estate",
    "f11674-lie-stage",
    "f11674-lie-code",
    "f11674-lie-value",
    "f11674-lie-note",
    "f11674-lie-default",
];

// ---------------------------------------------------------------------------
// 判序：Estate → Stage → Code → Value → Note → Default → Ok
// ---------------------------------------------------------------------------

/// 缺陷维 → 位次。
pub fn fault_ndx(f: Fault) -> usize {
    f.order()
}

/// 位次 → 缺陷维（越界给 `None`）。
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

/// 判一槽：七维按序，先命中先定。
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
        return Err(NF_ESLOT);
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
/// 与"未知选项卡"的差别：本卡的反例可以落在**有正文的档**上——一行真实文案
/// 配一句说错的申报，仍然是"落档对 + 命中它自己那一维"。所以反例判对的条件
/// 只要求"落档对 + 缺陷维对"，不附加"该档无正文"的限制。
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
    "applet-not-found corpus=14 accept=8 reject=6 verbatim=10 bannered=1 nobody=1 refused=2 match=14 rate=1000permille\n";

/// 未知 applet 批结算报告。
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
    /// 换家族档计数（字段名沿用契约的 `bannered`）。
    pub bannered: usize,
    /// 空名回落档计数（字段名沿用契约的 `nobody`）。
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
            return Err(NF_ENOBS);
        }
        if self.tier_cells() != self.total as u32 {
            return Err(NF_EMATRIX);
        }
        if self.matched > self.total {
            return Err(NF_EBAL);
        }
        if self.rate() < RATE_FULL {
            return Err(NF_EBAL);
        }
        Ok(())
    }

    /// 四档格数。
    pub fn tier_cells(&self) -> u32 {
        (self.verbatim as u32)
            + (self.bannered as u32)
            + (self.nobody as u32)
            + (self.refused as u32)
    }

    /// 逐字渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("applet-not-found corpus=");
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
            return Err(NF_ESLOT);
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
        let mut e = 0usize;
        while e < ENTRY_JUDGED {
            let entry = match Entry::at(e) {
                Some(x) => x,
                None => return [false, false, false, false],
            };
            let mut f = 0usize;
            while f < FORM_COUNT {
                let form = match Form::at(f) {
                    Some(x) => x,
                    None => return [false, false, false, false],
                };
                if let Some(c) = msg_cell(entry, form) {
                    cells += 1;
                    let text = msg_text(entry, form, NOSUCH);
                    if lines_of(text.as_str()) != c.lines {
                        ok = false;
                    }
                    if !text.contains(msg_name(entry, form, NOSUCH)) {
                        ok = false;
                    }
                    if c.code != msg_code(entry, form) {
                        ok = false;
                    }
                    if c.tpl != BB_TPL && c.tpl != CORE_TPL {
                        ok = false;
                    }
                }
                f += 1;
            }
            e += 1;
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
// 阶段闸门：Dispatch→Name→Lookup→Message→Channel→Code→Exit
// ---------------------------------------------------------------------------

/// 一次未知 applet 判据的走闸状态机。
///
/// 倒退判死（`NF_ESTAGE`），封账后再走一步也判死（`NF_ESTATE`）。
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
            return Err(NF_ESTATE);
        }
        if let Some(cur) = self.current {
            if s.rank() < cur.rank() {
                self.refusals += 1;
                return Err(NF_ESTAGE);
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
            return Err(NF_ESTATE);
        }
        if self.current.is_none() {
            return Err(NF_ESTAGE);
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

    /// 一次走完七档并封账（正路）。
    pub fn walk_all() -> Result<u64, u32> {
        let mut g = Gate::new();
        let all = Stage::all();
        let mut i = 0usize;
        while i < all.len() {
            let s = match all.get(i) {
                Some(x) => *x,
                None => return Err(NF_ESTAGE),
            };
            match g.step(s) {
                Ok(()) => {}
                Err(c) => return Err(c),
            }
            i += 1;
        }
        g.close()
    }
}

// ---------------------------------------------------------------------------
// 台账：十四格
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
            return Err(NF_ESTATE);
        }
        if self.len >= SAMPLES {
            self.rejects += 1;
            return Err(NF_ESLOT);
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(NF_ESLOT);
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
            return Err(NF_ESLOT);
        }
        let stat_last = match STAT_CELLS.checked_add(1) {
            Some(x) => x,
            None => return Err(NF_ESLOT),
        };
        let mut i = 0usize;
        while i < stat_last {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(NF_ESLOT),
            };
            if !s.ok {
                return Err(NF_ESTATE);
            }
            if s.code != 0 {
                return Err(NF_ECODE);
            }
            let want_value = match VALS.get(i) {
                Some(v) => *v,
                None => return Err(NF_ESLOT),
            };
            if s.value != want_value {
                return Err(NF_EVALUE);
            }
            let want_op = match self.stat_op_want(i) {
                Some(v) => v,
                None => return Err(NF_ESLOT),
            };
            if s.op != want_op {
                return Err(NF_ECODE);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < REJECT {
            let cell = match STAT_CELLS.checked_add(1).and_then(|x| x.checked_add(j)) {
                Some(x) => x,
                None => return Err(NF_ESLOT),
            };
            let s = match self.sample(cell) {
                Some(x) => x,
                None => return Err(NF_ESLOT),
            };
            if s.ok {
                return Err(NF_ESTATE);
            }
            if s.value != 0 {
                return Err(NF_EVALUE);
            }
            let want_code = match REJECT_CODES.get(j) {
                Some(c) => *c,
                None => return Err(NF_ESLOT),
            };
            if s.code != want_code {
                return Err(NF_ECODE);
            }
            let want_op = match DEFECT_OP_BASE.checked_add(j as u8) {
                Some(x) => x,
                None => return Err(NF_ESLOT),
            };
            if s.op != want_op {
                return Err(NF_ECODE);
            }
            let mut k = j + 1usize;
            while k < REJECT {
                let other = match REJECT_CODES.get(k) {
                    Some(c) => *c,
                    None => return Err(NF_ESLOT),
                };
                if other == want_code {
                    return Err(NF_ECODE);
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
            return Err(NF_ESTATE);
        }
        match self.audit() {
            Ok(()) => {}
            Err(code) => return Err(code),
        }
        let h = self.seal();
        if h == 0 {
            return Err(NF_ESEAL);
        }
        self.root = h;
        self.frozen = true;
        Ok(h)
    }

    /// 重放：以同一批格子重建账本，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(NF_ESTATE);
        }
        let mut fresh = Ledger::new();
        let mut i = 0usize;
        while i < self.len {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(NF_ESEAL),
            };
            match fresh.register(s) {
                Ok(()) => {}
                Err(_) => return Err(NF_ESEAL),
            }
            i += 1;
        }
        let h = fresh.seal();
        if h != self.root {
            return Err(NF_ESEAL);
        }
        Ok(h)
    }

    /// 改动一格（仅未冻结时允许），用于注入红。
    pub fn poke(&mut self, i: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(NF_ESTATE);
        }
        match self.slots.get_mut(i) {
            Some(cell) => {
                match *cell {
                    Some(ref mut s) => {
                        s.value = value;
                    }
                    None => return Err(NF_ESLOT),
                }
                Ok(())
            }
            None => Err(NF_ESLOT),
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
    ENTRY_JUDGED as u32,
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
fn basis_ok() -> bool {
    if SEG_BASE != 0x6d270 {
        return false;
    }
    if ANCHOR_F11672_SEG.checked_add(ANCHOR_F11672_GAP) != Some(SEG_BASE) {
        return false;
    }
    if ANCHOR_F11668_SEG.checked_add(0x80) != Some(SEG_BASE) {
        return false;
    }
    if ANCHOR_F11670_SEG.checked_add(0x50) != Some(SEG_BASE) {
        return false;
    }
    if ANCHOR_F11678_SEG <= SEG_BASE {
        return false;
    }
    if ANCHOR_F11678_SEG - SEG_BASE != ANCHOR_F11678_GAP {
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
    if ENTRY_COUNT != Entry::all().len() || ENTRY_JUDGED >= ENTRY_COUNT {
        return false;
    }
    if FORM_COUNT != Form::all().len() {
        return false;
    }
    if Note::all().len() != Note::COUNT {
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
        if a <= SEG_BASE || a >= ANCHOR_F11678_SEG {
            return false;
        }
        let want = match SEG_BASE.checked_add(1 + i as u32) {
            Some(x) => x,
            None => return false,
        };
        if a != want {
            return false;
        }
        let mut j = i + 1usize;
        while j < ALL_CODES.len() {
            if let Some(b) = ALL_CODES.get(j) {
                if *b == a {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 六维码与判序对得上，且两两互异
    if REJECT_CODES.len() != REJECT || REJECT_FAULTS.len() != REJECT {
        return false;
    }
    let mut k = 0usize;
    while k < REJECT {
        let f = match REJECT_FAULTS.get(k) {
            Some(x) => *x,
            None => return false,
        };
        let c = match REJECT_CODES.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if f.code() != c {
            return false;
        }
        if f.order() != k {
            return false;
        }
        if !ALL_CODES.iter().any(|x| *x == c) {
            return false;
        }
        k += 1;
    }
    true
}

/// 03 文案表五格齐。
fn table_ok() -> bool {
    if MSG_TABLE.len() != MESSAGE_CELLS {
        return false;
    }
    let mut i = 0usize;
    while i < MESSAGE_CELLS {
        let c = match MSG_TABLE.get(i) {
            Some(x) => x,
            None => return false,
        };
        if !c.entry.judged() {
            return false;
        }
        if c.tpl.is_empty() {
            return false;
        }
        if !c.tpl.contains("{N}") {
            return false;
        }
        if lines_of(c.tpl) != c.lines {
            return false;
        }
        if c.code != EXIT_FAIL && c.code != EXIT_NOTFOUND {
            return false;
        }
        // coreutils 那格是两行，busybox 那几格是一行——这一条不许混。
        if c.entry.is_core() != (c.lines == 2) {
            return false;
        }
        if c.entry.is_core() != (c.code == EXIT_FAIL) {
            return false;
        }
        let mut j = i + 1usize;
        while j < MESSAGE_CELLS {
            if let Some(o) = MSG_TABLE.get(j) {
                if o.entry == c.entry && o.form == c.form {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 缺格哨兵不许被命中
    if msg_cell(Entry::Unknown, Form::Bare).is_some() {
        return false;
    }
    if CELL_NONE.lines != 0 {
        return false;
    }
    // 表指纹非零且稳定
    let s1 = table_seal();
    let s2 = table_seal();
    s1 != 0 && s1 == s2
}

/// 04 名字取形分家（剥路径 / 空名回落 / 逐字节）。
fn text_ok() -> bool {
    // 剥路径：路径前缀绝不出现。
    let path_text = msg_text(Entry::Symlink, Form::Path, NOSUCH_PATH);
    if path_text.as_str() != "nosuchapplet: applet not found\n" {
        return false;
    }
    if path_text.contains("/") {
        return false;
    }
    // 空名：主语回落到多调用二进制名。
    let empty_text = msg_text(Entry::Arg, Form::Empty, "");
    let mut want = String::from(MULTICALL_NAME);
    want.push_str(NOT_FOUND_TAIL);
    want.push('\n');
    if empty_text.as_str() != want.as_str() {
        return false;
    }
    // 裸名：主语原样。
    let bare = msg_text(Entry::Symlink, Form::Bare, NOSUCH);
    if !bare.contains(NOSUCH) || !bare.contains(NOT_FOUND_TAIL) {
        return false;
    }
    if lines_of(bare.as_str()) != 1 {
        return false;
    }
    // 非 ASCII：逐字节原样嵌入，不转义。
    let utf8 = msg_text(Entry::Symlink, Form::Bare, UTF8_NAME);
    if !utf8.contains(UTF8_NAME) {
        return false;
    }
    if bytes_of(utf8.as_str()) != bytes_of(UTF8_NAME) + bytes_of(NOT_FOUND_TAIL) + 1 {
        return false;
    }
    // coreutils：两行，头尾与 Try 行都在。
    let core = msg_text(Entry::CoreArg, Form::Bare, NOSUCH);
    if lines_of(core.as_str()) != 2 {
        return false;
    }
    if !core.starts_with("coreutils") {
        return false;
    }
    if !core.contains(CORE_HEAD) || !core.contains(CORE_TAIL) || !core.contains(CORE_TRY) {
        return false;
    }
    // basename 助手。
    if basename(NOSUCH_PATH) != NOSUCH {
        return false;
    }
    if basename(NOSUCH) != NOSUCH {
        return false;
    }
    if basename("/") != "" {
        return false;
    }
    if !is_ascii_text(NOSUCH) || is_ascii_text(UTF8_NAME) {
        return false;
    }
    if !has_slash(NOSUCH_PATH) || has_slash(NOSUCH) {
        return false;
    }
    // 只留首行。
    let cut = first_line_only(core.as_str());
    if lines_of(cut.as_str()) != 1 {
        return false;
    }
    true
}

/// 05 诊断走 stderr。
fn channel_ok() -> bool {
    if !Channel::Stderr.is_diag() {
        return false;
    }
    if Channel::Stdout.is_diag() || Channel::Nowhere.is_diag() {
        return false;
    }
    if !Channel::Stderr.carries_bytes() || Channel::Nowhere.carries_bytes() {
        return false;
    }
    let out = define(&Shape::blank().with_channel(Channel::Stdout));
    if out.stage != Stage::Channel || out.note != Note::Misrouted || out.consistent {
        return false;
    }
    if out.code != EXIT_NOTFOUND as i32 {
        return false;
    }
    let mut o = 0usize;
    while o < Channel::all().len() {
        if let Some(c) = Channel::all().get(o) {
            if c.index() != o {
                return false;
            }
        }
        o += 1;
    }
    if Channel::at(3).is_some() {
        return false;
    }
    true
}

/// 06 码档 127 与 1 对齐。
fn code_ok() -> bool {
    if EXIT_OK != 0 || EXIT_FAIL != 1 || EXIT_NOTFOUND != 127 {
        return false;
    }
    if Entry::Symlink.exit_code() != EXIT_NOTFOUND || Entry::Arg.exit_code() != EXIT_NOTFOUND {
        return false;
    }
    if Entry::CoreArg.exit_code() != EXIT_FAIL {
        return false;
    }
    if msg_code(Entry::Symlink, Form::Bare) != EXIT_NOTFOUND {
        return false;
    }
    if msg_code(Entry::Arg, Form::Empty) != EXIT_NOTFOUND {
        return false;
    }
    if msg_code(Entry::CoreArg, Form::Bare) != EXIT_FAIL {
        return false;
    }
    // 申报错码：真值不动，只压假自洽性。
    let wrong = define(&Shape::blank().with_exit(EXIT_FAIL));
    if wrong.consistent {
        return false;
    }
    if wrong.code != EXIT_NOTFOUND as i32 {
        return false;
    }
    if wrong.note != Note::AppletNotFound {
        return false;
    }
    if !wrong.is_notfound_code() {
        return false;
    }
    // 申报成"高码"同样压假。
    let high = define(&Shape::blank().with_exit(EXIT_NOTFOUND + 1));
    if high.consistent {
        return false;
    }
    true
}

/// 07 查表与名字上限。
fn lookup_ok() -> bool {
    let mut i = 0usize;
    while i < KNOWN_APPLETS.len() {
        let n = match KNOWN_APPLETS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if !is_known(n) {
            return false;
        }
        let out = define(&Shape::blank().with_name(n));
        if out.stage != Stage::Lookup || out.note != Note::KnownApplet {
            return false;
        }
        if !out.default || out.code != EXIT_OK as i32 || out.value != 0 {
            return false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < UNKNOWN_NAMES.len() {
        match UNKNOWN_NAMES.get(k) {
            Some(n) => {
                if is_known(n) {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    // 名字上限：正上界通过，超一字节就落兜底。
    if bytes_of(NOSUCH) > NAME_CAP || bytes_of(LONG_NAME) > NAME_CAP {
        return false;
    }
    if bytes_of(OVERLONG_NAME) <= NAME_CAP {
        return false;
    }
    if !name_ok(NOSUCH) || name_ok("") || name_ok(OVERLONG_NAME) {
        return false;
    }
    let over = define(&Shape::blank().with_name(OVERLONG_NAME));
    if over.stage != Stage::Name || over.note != Note::Fallback || !over.default {
        return false;
    }
    // 空主语：连模板都填不出来，拒收。
    let silent = define(&Shape::blank().with_name(""));
    if silent.stage != Stage::Name || silent.note != Note::Silent || silent.consistent {
        return false;
    }
    true
}

/// 08 语料十四槽互校。
fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS || WANT_TAGS.len() != CORPUS {
        return false;
    }
    if WANT_STAGE.len() != CORPUS
        || WANT_CODE.len() != CORPUS
        || WANT_VALUE.len() != CORPUS
        || WANT_NOTE.len() != CORPUS
        || WANT_DEFAULT.len() != CORPUS
        || WANT_NDX.len() != CORPUS
        || WANT_SUCCESS.len() != CORPUS
    {
        return false;
    }
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
        if a.stage != want_stage {
            return false;
        }
        if a.exit() != want_code {
            return false;
        }
        if a.value != want_value {
            return false;
        }
        if a.note != want_note {
            return false;
        }
        if a.default != want_default {
            return false;
        }
        if c.claim.success() != want_success {
            return false;
        }
        // 前八槽必须判对，后六槽必须命中各自那一维。
        if i < ACCEPT {
            if c.fault() != Fault::Ok {
                return false;
            }
            if !a.ok() && c.claim.success() {
                return false;
            }
        } else {
            let want = expected_reject_ndx(i);
            if c.fault() != want {
                return false;
            }
            if want == Fault::Ok {
                return false;
            }
        }
        // 槽间指纹互异：每一槽都得是判得出来的不同现场。
        let mut j = i + 1usize;
        while j < CORPUS {
            if let Some(o) = case_at(j) {
                if o.seal() == c.seal() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    // 反例六维各占一位，一个不多一个不少。
    let mut seen = [false; REJECT];
    let mut k = ACCEPT;
    while k < CORPUS {
        let f = expected_reject_ndx(k);
        let ndx = fault_ndx(f);
        if ndx >= REJECT {
            return false;
        }
        if let Some(slot) = seen.get_mut(ndx) {
            *slot = true;
        }
        k += 1;
    }
    let mut n = 0usize;
    while n < REJECT {
        match seen.get(n) {
            Some(true) => {}
            _ => return false,
        }
        n += 1;
    }
    if cover(CORPUS) != Ok(()) || cover(CORPUS + 1) != Err(NF_ESLOT) {
        return false;
    }
    if case_at(CORPUS).is_some() || cases().len() != CORPUS {
        return false;
    }
    true
}

/// 09 判序六维互斥。
fn judge_ok() -> bool {
    let base = Shape::blank();
    let truth = define(&base);
    if truth.note != Note::AppletNotFound || truth.value != 1 {
        return false;
    }
    if truth.code != EXIT_NOTFOUND as i32 {
        return false;
    }
    let one_to_one = Case {
        shape: base,
        claim: Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFound, false),
        tag: "f11674-one-to-one",
    };
    if judge(&one_to_one) != Fault::Ok {
        return false;
    }
    let lie_estate = Case {
        shape: base,
        claim: Claim::make(true, Stage::Exit, 127, 1, Note::AppletNotFound, false),
        tag: "f11674-lie-estate",
    };
    if judge(&lie_estate) != Fault::Estate {
        return false;
    }
    let lie_stage = Case {
        shape: base,
        claim: Claim::make(false, Stage::Message, 127, 1, Note::AppletNotFound, false),
        tag: "f11674-lie-stage",
    };
    if judge(&lie_stage) != Fault::Stage {
        return false;
    }
    let lie_code = Case {
        shape: base,
        claim: Claim::make(false, Stage::Exit, 1, 1, Note::AppletNotFound, false),
        tag: "f11674-lie-code",
    };
    if judge(&lie_code) != Fault::Code {
        return false;
    }
    let lie_value = Case {
        shape: base,
        claim: Claim::make(false, Stage::Exit, 127, 2, Note::AppletNotFound, false),
        tag: "f11674-lie-value",
    };
    if judge(&lie_value) != Fault::Value {
        return false;
    }
    let lie_note = Case {
        shape: base,
        claim: Claim::make(false, Stage::Exit, 127, 1, Note::CoreNotCommand, false),
        tag: "f11674-lie-note",
    };
    if judge(&lie_note) != Fault::Note {
        return false;
    }
    let lie_default = Case {
        shape: base,
        claim: Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFound, true),
        tag: "f11674-lie-default",
    };
    if judge(&lie_default) != Fault::Default {
        return false;
    }
    // 两维同时错：只报判序靠前的那一维。
    let lie_two = Case {
        shape: base,
        claim: Claim::make(false, Stage::Message, 1, 2, Note::CoreNotCommand, false),
        tag: "f11674-lie-two",
    };
    if judge(&lie_two) != Fault::Stage {
        return false;
    }
    let lie_estate_first = Case {
        shape: base,
        claim: Claim::make(true, Stage::Message, 1, 2, Note::CoreNotCommand, false),
        tag: "f11674-lie-estate-first",
    };
    if judge(&lie_estate_first) != Fault::Estate {
        return false;
    }
    // 位次与维一一对应。
    let order = [
        Fault::Estate,
        Fault::Stage,
        Fault::Code,
        Fault::Value,
        Fault::Note,
        Fault::Default,
        Fault::Ok,
    ];
    let mut i = 0usize;
    while i < order.len() {
        let f = match order.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if fault_ndx(f) != i {
            return false;
        }
        if ndx_fault(i) != Some(f) {
            return false;
        }
        i += 1;
    }
    if ndx_fault(7).is_some() {
        return false;
    }
    true
}

/// 10 四档矩阵覆盖。
///
/// 注意每一行的 `total` 是"该行扫过的槽数"（四行都扫全语料，故恒为 14），
/// `good` 才是"落在这一档**且**判对"的格数——两维不许混看。
fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS * TIER_COUNT {
        return false;
    }
    if m.goods() != CORPUS {
        return false;
    }
    if !m.covered() {
        return false;
    }
    if m.red_total() != CORPUS * TIER_COUNT - CORPUS {
        return false;
    }
    let rep = settle(None);
    let mut i = 0usize;
    while i < TIER_COUNT {
        let tier = match Tier::all().get(i) {
            Some(x) => *x,
            None => return false,
        };
        let row = match m.row(tier) {
            Some(x) => x,
            None => return false,
        };
        let want = match tier {
            Tier::Verbatim => rep.verbatim,
            Tier::Bannered => rep.bannered,
            Tier::NoBody => rep.nobody,
            Tier::Refused => rep.refused,
        };
        if row.total != CORPUS {
            return false;
        }
        if row.good != want {
            return false;
        }
        if row.red() != CORPUS - want {
            return false;
        }
        if row.perfect() != (want == CORPUS) {
            return false;
        }
        i += 1;
    }
    if m.perfect_rows() != 0 {
        return false;
    }
    if !m.render().starts_with("matrix ") {
        return false;
    }
    report_agrees_with_matrix()
}

/// 11 全绿渲染逐字。
fn render_ok() -> bool {
    let rep = settle(None);
    if !rep.green() {
        return false;
    }
    if rep.render().as_str() != RENDER_OK {
        return false;
    }
    if rep.rate() != RATE_FULL || rep.reds() != 0 {
        return false;
    }
    if rep.gate() != Ok(()) {
        return false;
    }
    if empty_report().gate() != Err(NF_ENOBS) {
        return false;
    }
    // 注入一维红：渲染必须变，且闸门必须拦。
    let bad = match run_with(Some((3, Fault::Code))) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if bad.green() {
        return false;
    }
    if bad.render().as_str() == RENDER_OK {
        return false;
    }
    if bad.gate() == Ok(()) {
        return false;
    }
    // 越界槽。
    if run_with(Some((CORPUS, Fault::Ok))) != Err(NF_ESLOT) {
        return false;
    }
    if run() != Ok(rep) {
        return false;
    }
    red_rate_calibrated()
}

/// 12 阶段闸门不倒退。
fn gate_ok() -> bool {
    let root = match Gate::walk_all() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if root == 0 {
        return false;
    }
    let mut g = Gate::new();
    if g.step(Stage::Exit) != Ok(()) {
        return false;
    }
    if g.step(Stage::Code) != Err(NF_ESTAGE) {
        return false;
    }
    if g.stage() != Some(Stage::Exit) || g.refusals() != 1 || g.steps() != 1 {
        return false;
    }
    let mut h = Gate::new();
    if h.step(Stage::Dispatch) != Ok(()) {
        return false;
    }
    let closed = match h.close() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if closed == 0 || !h.is_closed() || h.root() != closed {
        return false;
    }
    if h.step(Stage::Name) != Err(NF_ESTATE) {
        return false;
    }
    if h.close() != Err(NF_ESTATE) {
        return false;
    }
    if h.refusals() != 2 {
        return false;
    }
    let mut k = Gate::new();
    if k.close() != Err(NF_ESTAGE) {
        return false;
    }
    if k.stage().is_some() {
        return false;
    }
    // 同档重复不算倒退。
    let mut d = Gate::new();
    if d.step(Stage::Name) != Ok(()) || d.step(Stage::Name) != Ok(()) {
        return false;
    }
    if d.steps() != 2 || d.stage() != Some(Stage::Name) {
        return false;
    }
    // 跳档允许（只禁倒退）。
    let mut j = Gate::new();
    if j.step(Stage::Dispatch) != Ok(()) || j.step(Stage::Exit) != Ok(()) {
        return false;
    }
    // 阶段表本身单调。
    let all = Stage::all();
    let mut i = 1usize;
    while i < all.len() {
        let prev = match all.get(i - 1) {
            Some(x) => *x,
            None => return false,
        };
        let cur = match all.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if cur.rank() != prev.rank() + 1 {
            return false;
        }
        if Stage::from_rank(cur.rank()) != Some(cur) {
            return false;
        }
        i += 1;
    }
    true
}

/// 13 冻结重放四注入（台账部分）。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.len() != SAMPLES || l.audit() != Ok(()) {
        return false;
    }
    if l.ok_count() != STAT_CELLS + 1 || l.err_count() != REJECT {
        return false;
    }
    if l.rejects() != 0 || l.is_empty() || l.is_frozen() || l.root() != 0 {
        return false;
    }
    if l.first_mismatch().is_some() {
        return false;
    }
    let mut f = build_ledger();
    let h = match f.freeze() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if h == 0 || f.root() != h {
        return false;
    }
    if f.freeze() != Err(NF_ESTATE) {
        return false;
    }
    if f.replay() != Ok(h) {
        return false;
    }
    if f.register(Sample {
        slot: 0,
        op: OP_DISPATCH,
        ok: true,
        code: 0,
        value: 1,
    }) != Err(NF_ESTATE)
    {
        return false;
    }
    let u = build_ledger();
    if u.replay() != Err(NF_ESTATE) {
        return false;
    }
    let mut w = 0usize;
    while w < 4 {
        let mut b = build_ledger_broken(w as u8);
        if b.audit() == Ok(()) {
            return false;
        }
        if b.freeze().is_ok() {
            return false;
        }
        if b.first_mismatch().is_none() {
            return false;
        }
        w += 1;
    }
    let mut wrong = Ledger::new();
    let stray = Sample {
        slot: 1,
        op: OP_DISPATCH,
        ok: true,
        code: 0,
        value: 1,
    };
    if wrong.register(stray) != Err(NF_ESLOT) {
        return false;
    }
    if wrong.rejects() != 1 || !wrong.is_empty() {
        return false;
    }
    // 统计表八格与台账对得上。
    if VALS.len() != ACCEPT || VALS.get(0) != Some(&STAGE_COUNT) {
        return false;
    }
    if VALS.get(7) != Some(&RED_RATE) {
        return false;
    }
    true
}

/// 申报表第一格的标签。
pub fn head_tag() -> &'static str {
    match WANT_TAGS.get(0) {
        Some(x) => *x,
        None => "f11674-symlink-bare",
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
pub fn run_unx11674_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11674_applet_not_found_text");
    s.add("NF01-名称与段基址", basis_ok(), "static");
    s.add("NF02-十三码互异", codes_ok(), "static");
    s.add("NF03-文案表五格齐", table_ok(), "static");
    s.add("NF04-名字取形分家", text_ok(), "static");
    s.add("NF05-诊断走stderr", channel_ok(), "static");
    s.add("NF06-码档127与1对齐", code_ok(), "static");
    s.add("NF07-查表与名字上限", lookup_ok(), "static");
    s.add("NF08-语料十四槽互校", corpus_ok(), "static");
    s.add("NF09-判序六维互斥", judge_ok(), "static");
    s.add("NF10-四档矩阵覆盖", matrix_ok(), "static");
    s.add("NF11-全绿渲染逐字", render_ok(), "static");
    s.add("NF12-阶段闸门不倒退", gate_ok(), "static");
    s.add(
        "NF13-冻结重放四注入",
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

    // --- 01 段基址与锚点 ---------------------------------------------------

    #[test]
    fn t01_seg_base_pinned() {
        assert_eq!(SEG_BASE, 0x6d270);
        assert_eq!(ANCHOR_F11672_SEG + ANCHOR_F11672_GAP, SEG_BASE);
        assert_eq!(ANCHOR_F11678_SEG - SEG_BASE, ANCHOR_F11678_GAP);
        assert_eq!(ANCHOR_F11668_SEG + 0x80, SEG_BASE);
        assert_eq!(ANCHOR_F11670_SEG + 0x50, SEG_BASE);
        assert!(ANCHOR_F11678_SEG > SEG_BASE);
    }

    // --- 02 规模常量 -------------------------------------------------------

    #[test]
    fn t02_scales() {
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert_eq!(ACCEPT, 8);
        assert_eq!(REJECT, 6);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(STAGE_COUNT, 7);
        assert_eq!(STAGE_LAST_RANK, 7);
        assert_eq!(STAT_CELLS, 7);
        assert_eq!(MESSAGE_CELLS, 5);
        assert_eq!(ENTRY_COUNT, 4);
        assert_eq!(ENTRY_JUDGED, 3);
        assert_eq!(FORM_COUNT, 3);
        assert_eq!(NAME_CAP, 255);
        assert_eq!(RED_RATE, 929);
        assert_eq!(RATE_FULL, 1000);
    }

    // --- 03 十三码 ---------------------------------------------------------

    #[test]
    fn t03_code_offsets() {
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            assert_eq!(ALL_CODES[i], SEG_BASE + 1 + i as u32);
            i += 1;
        }
        assert_eq!(NF_ESTATE, SEG_BASE + 0x01);
        assert_eq!(NF_ECODE, SEG_BASE + 0x02);
        assert_eq!(NF_ESTAGE, SEG_BASE + 0x03);
        assert_eq!(NF_EBAL, SEG_BASE + 0x0d);
    }

    #[test]
    fn t04_codes_distinct_and_bounded() {
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            assert!(ALL_CODES[i] > SEG_BASE);
            assert!(ALL_CODES[i] < ANCHOR_F11678_SEG);
            let mut j = i + 1usize;
            while j < ALL_CODES.len() {
                assert_ne!(ALL_CODES[i], ALL_CODES[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t05_fault_codes_match_judgement_order() {
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        assert_eq!(REJECT_FAULTS[0], Fault::Estate);
        assert_eq!(REJECT_FAULTS[1], Fault::Stage);
        assert_eq!(REJECT_FAULTS[2], Fault::Code);
        assert_eq!(REJECT_FAULTS[3], Fault::Value);
        assert_eq!(REJECT_FAULTS[4], Fault::Note);
        assert_eq!(REJECT_FAULTS[5], Fault::Default);
        let mut i = 0usize;
        while i < REJECT {
            assert_eq!(REJECT_FAULTS[i].code(), REJECT_CODES[i]);
            assert_eq!(REJECT_FAULTS[i].order(), i);
            i += 1;
        }
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(Fault::Ok.order(), 6);
    }

    // --- 06 入口表 ---------------------------------------------------------

    #[test]
    fn t06_entry_table() {
        assert_eq!(Entry::all().len(), ENTRY_COUNT);
        assert_eq!(Entry::Symlink.name(), "symlink");
        assert_eq!(Entry::Arg.name(), "arg");
        assert_eq!(Entry::CoreArg.name(), "core-arg");
        assert_eq!(Entry::Unknown.name(), "unknown");
        let mut i = 0usize;
        while i < ENTRY_COUNT {
            let e = Entry::all()[i];
            assert_eq!(e.index(), i);
            assert_eq!(Entry::at(i), Some(e));
            i += 1;
        }
        assert!(Entry::at(ENTRY_COUNT).is_none());
        assert!(Entry::Symlink.judged());
        assert!(Entry::Arg.judged());
        assert!(Entry::CoreArg.judged());
        assert!(!Entry::Unknown.judged());
        assert!(Entry::Symlink.from_argv0());
        assert!(!Entry::Arg.from_argv0());
        assert!(!Entry::CoreArg.from_argv0());
        assert!(Entry::CoreArg.is_core());
        assert!(!Entry::Symlink.is_core());
        assert_eq!(Entry::Symlink.exit_code(), EXIT_NOTFOUND);
        assert_eq!(Entry::Arg.exit_code(), EXIT_NOTFOUND);
        assert_eq!(Entry::CoreArg.exit_code(), EXIT_FAIL);
        assert_eq!(ENTRY_JUDGED, 3);
    }

    // --- 07 名字形态表 -----------------------------------------------------

    #[test]
    fn t07_form_table() {
        assert_eq!(Form::all().len(), FORM_COUNT);
        assert_eq!(Form::Bare.name(), "bare");
        assert_eq!(Form::Path.name(), "path");
        assert_eq!(Form::Empty.name(), "empty");
        let mut i = 0usize;
        while i < FORM_COUNT {
            let f = Form::all()[i];
            assert_eq!(f.index(), i);
            assert_eq!(Form::at(i), Some(f));
            i += 1;
        }
        assert!(Form::at(FORM_COUNT).is_none());
        assert!(Form::Path.strips_path());
        assert!(!Form::Bare.strips_path());
        assert!(!Form::Empty.strips_path());
        assert!(Form::Empty.falls_back());
        assert!(!Form::Bare.falls_back());
    }

    // --- 08 通道表 ---------------------------------------------------------

    #[test]
    fn t08_channel_table() {
        assert_eq!(Channel::all().len(), 3);
        assert_eq!(Channel::Stderr.name(), "stderr");
        assert_eq!(Channel::Stdout.name(), "stdout");
        assert_eq!(Channel::Nowhere.name(), "nowhere");
        let mut i = 0usize;
        while i < 3 {
            let c = Channel::all()[i];
            assert_eq!(c.index(), i);
            assert_eq!(Channel::at(i), Some(c));
            i += 1;
        }
        assert!(Channel::at(3).is_none());
        assert!(Channel::Stderr.is_diag());
        assert!(!Channel::Stdout.is_diag());
        assert!(!Channel::Nowhere.is_diag());
        assert!(Channel::Stdout.carries_bytes());
        assert!(!Channel::Nowhere.carries_bytes());
    }

    // --- 09 阶段表 ---------------------------------------------------------

    #[test]
    fn t09_stage_table() {
        assert_eq!(Stage::all().len() as u32, STAGE_COUNT);
        assert_eq!(Stage::Dispatch.rank(), 1);
        assert_eq!(Stage::Name.rank(), 2);
        assert_eq!(Stage::Lookup.rank(), 3);
        assert_eq!(Stage::Message.rank(), 4);
        assert_eq!(Stage::Channel.rank(), 5);
        assert_eq!(Stage::Code.rank(), 6);
        assert_eq!(Stage::Exit.rank(), STAGE_LAST_RANK);
        let mut i = 0usize;
        while i < Stage::all().len() {
            let s = Stage::all()[i];
            assert_eq!(s.rank(), 1 + i as u32);
            assert_eq!(Stage::from_rank(s.rank()), Some(s));
            i += 1;
        }
        assert!(Stage::from_rank(0).is_none());
        assert!(Stage::from_rank(8).is_none());
        assert_eq!(Stage::Dispatch.name(), "dispatch");
        assert_eq!(Stage::Exit.name(), "exit");
        assert!(Stage::Dispatch.before_text());
        assert!(Stage::Lookup.before_text());
        assert!(!Stage::Message.before_text());
        assert!(!Stage::Exit.settled_gate());
        assert!(Stage::Code.settled_gate());
        assert!(!Stage::Name.after_name());
        assert!(Stage::Lookup.after_name());
    }

    // --- 10 注解档表 -------------------------------------------------------

    #[test]
    fn t10_note_table() {
        assert_eq!(Note::all().len(), Note::COUNT);
        assert_eq!(Note::COUNT, 10);
        assert_eq!(Note::AppletNotFound.name(), "applet-not-found");
        assert_eq!(Note::AppletNotFoundPath.name(), "applet-not-found-path");
        assert_eq!(Note::AppletNotFoundUtf8.name(), "applet-not-found-utf8");
        assert_eq!(Note::AppletNotFoundEmpty.name(), "applet-not-found-empty");
        assert_eq!(Note::CoreNotCommand.name(), "core-not-command");
        assert_eq!(Note::KnownApplet.name(), "known-applet");
        assert_eq!(Note::Truncated.name(), "truncated");
        assert_eq!(Note::Misrouted.name(), "misrouted");
        assert_eq!(Note::Silent.name(), "silent");
        assert_eq!(Note::Fallback.name(), "fallback");
        let mut i = 0usize;
        while i < Note::COUNT {
            assert_eq!(Note::all()[i].index(), i);
            i += 1;
        }
        assert!(Note::KnownApplet.defaulted());
        assert!(Note::Fallback.defaulted());
        assert!(!Note::AppletNotFound.defaulted());
        assert!(!Note::Silent.defaulted());
        assert!(Note::AppletNotFound.has_body());
        assert!(Note::CoreNotCommand.has_body());
        assert!(!Note::KnownApplet.has_body());
        assert!(!Note::Truncated.has_body());
        assert!(Note::Truncated.speaks_error());
        assert!(!Note::Fallback.speaks_error());
        assert!(!Note::KnownApplet.speaks_error());
        assert!(Note::CoreNotCommand.is_core());
        assert!(Note::CoreNotCommand.needs_try());
        assert!(!Note::AppletNotFound.is_core());
        assert!(Note::AppletNotFound.is_busybox());
        assert!(Note::AppletNotFoundPath.is_busybox());
        assert!(!Note::CoreNotCommand.is_busybox());
        assert!(Note::AppletNotFoundEmpty.subject_fallback());
        assert!(!Note::AppletNotFound.subject_fallback());
        assert!(Note::AppletNotFoundPath.subject_stripped());
        assert!(!Note::AppletNotFound.subject_stripped());
    }

    // --- 11 档位表 ---------------------------------------------------------

    #[test]
    fn t11_tier_table() {
        assert_eq!(Tier::all().len(), TIER_COUNT);
        assert_eq!(Tier::Verbatim.name(), "verbatim");
        assert_eq!(Tier::Bannered.name(), "bannered");
        assert_eq!(Tier::NoBody.name(), "nobody");
        assert_eq!(Tier::Refused.name(), "refused");
        let mut i = 0usize;
        while i < TIER_COUNT {
            assert_eq!(Tier::all()[i].index(), i);
            i += 1;
        }
        assert!(Tier::Verbatim.has_text());
        assert!(Tier::Bannered.has_text());
        assert!(!Tier::NoBody.has_text());
        assert!(!Tier::Refused.has_text());
    }

    // --- 12 缺陷维表 -------------------------------------------------------

    #[test]
    fn t12_fault_table() {
        assert_eq!(Fault::Ok.name(), "ok");
        assert_eq!(Fault::Estate.name(), "estate");
        assert_eq!(Fault::Stage.name(), "stage");
        assert_eq!(Fault::Code.name(), "code");
        assert_eq!(Fault::Value.name(), "value");
        assert_eq!(Fault::Note.name(), "note");
        assert_eq!(Fault::Default.name(), "default");
        assert_eq!(fault_short(Fault::Ok), "OK");
        assert_eq!(fault_short(Fault::Estate), "ESTATE");
        assert_eq!(fault_short(Fault::Stage), "ESTAGE");
        assert_eq!(fault_short(Fault::Code), "ECODE");
        assert_eq!(fault_short(Fault::Value), "EVALUE");
        assert_eq!(fault_short(Fault::Note), "ENOTE");
        assert_eq!(fault_short(Fault::Default), "EDEFAUL");
        assert_eq!(fault_ndx(Fault::Ok), 6);
        assert_eq!(ndx_fault(6), Some(Fault::Ok));
        assert!(ndx_fault(7).is_none());
    }

    // --- 13 文案表 ---------------------------------------------------------

    #[test]
    fn t13_message_table_cells() {
        assert_eq!(MSG_TABLE.len(), MESSAGE_CELLS);
        assert_eq!(MESSAGE_CELLS, 5);
        let mut i = 0usize;
        while i < MESSAGE_CELLS {
            let c = MSG_TABLE[i];
            assert!(c.entry.judged());
            assert!(c.tpl.contains("{N}"));
            assert!(!c.tpl.is_empty());
            assert_eq!(c.lines, lines_of(c.tpl));
            assert!(c.code == EXIT_FAIL || c.code == EXIT_NOTFOUND);
            assert_eq!(c.entry.is_core(), c.lines == 2);
            let mut j = i + 1usize;
            while j < MESSAGE_CELLS {
                let o = MSG_TABLE[j];
                assert!(!(o.entry == c.entry && o.form == c.form));
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t14_busybox_one_line_core_two_lines() {
        assert_eq!(msg_lines(Entry::Symlink, Form::Bare), 1);
        assert_eq!(msg_lines(Entry::Symlink, Form::Path), 1);
        assert_eq!(msg_lines(Entry::Arg, Form::Bare), 1);
        assert_eq!(msg_lines(Entry::Arg, Form::Empty), 1);
        assert_eq!(msg_lines(Entry::CoreArg, Form::Bare), 2);
        assert_eq!(lines_of(BB_TPL), 1);
        assert_eq!(lines_of(CORE_TPL), 2);
        assert_eq!(lines_of(CORE_TPL), msg_lines(Entry::CoreArg, Form::Bare));
    }

    #[test]
    fn t15_message_cell_lookup() {
        assert!(msg_cell(Entry::Symlink, Form::Bare).is_some());
        assert!(msg_cell(Entry::Symlink, Form::Path).is_some());
        assert!(msg_cell(Entry::Arg, Form::Bare).is_some());
        assert!(msg_cell(Entry::Arg, Form::Empty).is_some());
        assert!(msg_cell(Entry::CoreArg, Form::Bare).is_some());
        // 没有落点的四组组合一律缺格。
        assert!(msg_cell(Entry::Symlink, Form::Empty).is_none());
        assert!(msg_cell(Entry::Arg, Form::Path).is_none());
        assert!(msg_cell(Entry::CoreArg, Form::Path).is_none());
        assert!(msg_cell(Entry::CoreArg, Form::Empty).is_none());
        assert!(msg_cell(Entry::Unknown, Form::Bare).is_none());
        assert_eq!(CELL_NONE.lines, 0);
        assert_eq!(CELL_NONE.tpl, "");
    }

    // --- 14 名字取形 -------------------------------------------------------

    #[test]
    fn t16_msg_name_forms() {
        assert_eq!(msg_name(Entry::Symlink, Form::Bare, NOSUCH), NOSUCH);
        assert_eq!(msg_name(Entry::Symlink, Form::Path, NOSUCH_PATH), NOSUCH);
        assert_eq!(msg_name(Entry::Arg, Form::Bare, DOTTED), DOTTED);
        assert_eq!(msg_name(Entry::Arg, Form::Empty, ""), MULTICALL_NAME);
        assert_eq!(msg_name(Entry::Symlink, Form::Bare, UTF8_NAME), UTF8_NAME);
    }

    #[test]
    fn t17_basename_edges() {
        assert_eq!(basename(NOSUCH_PATH), NOSUCH);
        assert_eq!(basename(NOSUCH), NOSUCH);
        assert_eq!(basename("a/b/c"), "c");
        assert_eq!(basename("/"), "");
        assert_eq!(basename("dir/"), "");
        assert_eq!(basename(""), "");
        assert_eq!(basename("/opt/bin/"), "");
        assert_eq!(basename("x/y.z"), "y.z");
    }

    #[test]
    fn t18_subst_replaces_placeholder() {
        let out = subst("{N}: applet not found\n", "x");
        assert_eq!(out.as_str(), "x: applet not found\n");
        assert!(!out.contains("{N}"));
        let plain = subst("no placeholder here", "x");
        assert_eq!(plain.as_str(), "no placeholder here");
        let twice = subst("{N}{N}", "ab");
        assert_eq!(twice.as_str(), "abab");
        let empty = subst("{N}", "");
        assert_eq!(empty.as_str(), "");
    }

    #[test]
    fn t19_bare_text_exact() {
        let mut want = String::from(NOSUCH);
        want.push_str(NOT_FOUND_TAIL);
        want.push('\n');
        assert_eq!(
            msg_text(Entry::Symlink, Form::Bare, NOSUCH).as_str(),
            want.as_str()
        );
        assert_eq!(
            msg_text(Entry::Arg, Form::Bare, NOSUCH).as_str(),
            want.as_str()
        );
    }

    #[test]
    fn t20_path_text_strips_prefix() {
        let text = msg_text(Entry::Symlink, Form::Path, NOSUCH_PATH);
        assert_eq!(text.as_str(), "nosuchapplet: applet not found\n");
        assert!(!text.contains("/"));
        assert!(!text.contains("opt"));
    }

    #[test]
    fn t21_empty_text_falls_back() {
        let mut want = String::from(MULTICALL_NAME);
        want.push_str(NOT_FOUND_TAIL);
        want.push('\n');
        assert_eq!(msg_text(Entry::Arg, Form::Empty, "").as_str(), want.as_str());
        assert_eq!(
            msg_text(Entry::Arg, Form::Empty, "").as_str(),
            "busybox: applet not found\n"
        );
    }

    #[test]
    fn t22_core_text_exact() {
        let mut want = String::from(CORE_HEAD);
        want.push_str(NOSUCH);
        want.push_str(CORE_TAIL);
        want.push('\n');
        want.push_str(CORE_TRY);
        want.push('\n');
        let got = msg_text(Entry::CoreArg, Form::Bare, NOSUCH);
        assert_eq!(got.as_str(), want.as_str());
        assert_eq!(lines_of(got.as_str()), 2);
        assert!(got.starts_with("coreutils: '"));
        assert!(got.contains(CORE_TRY));
    }

    #[test]
    fn t23_utf8_text_keeps_bytes() {
        let got = msg_text(Entry::Symlink, Form::Bare, UTF8_NAME);
        assert!(got.contains(UTF8_NAME));
        assert!(!got.contains("\\"));
        assert_eq!(
            bytes_of(got.as_str()),
            bytes_of(UTF8_NAME) + bytes_of(NOT_FOUND_TAIL) + 1
        );
        assert_eq!(lines_of(got.as_str()), 1);
    }

    #[test]
    fn t24_first_line_only_cuts_core() {
        let core = msg_text(Entry::CoreArg, Form::Bare, NOSUCH);
        let cut = first_line_only(core.as_str());
        assert_eq!(lines_of(cut.as_str()), 1);
        assert!(cut.contains(CORE_HEAD));
        assert!(cut.contains(CORE_TAIL));
        assert!(!cut.contains(CORE_TRY));
        let bare = msg_text(Entry::Symlink, Form::Bare, NOSUCH);
        assert_eq!(first_line_only(bare.as_str()).as_str(), bare.as_str());
    }

    #[test]
    fn t25_text_scanners() {
        assert_eq!(lines_of(""), 0);
        assert_eq!(lines_of("a"), 0);
        assert_eq!(lines_of("a\n"), 1);
        assert_eq!(lines_of("a\nb\n"), 2);
        assert_eq!(bytes_of(""), 0);
        assert_eq!(bytes_of(NOSUCH), 12);
        assert_eq!(bytes_of(UTF8_NAME), 6);
        assert!(is_ascii_text(NOSUCH));
        assert!(is_ascii_text(""));
        assert!(!is_ascii_text(UTF8_NAME));
        assert!(has_slash(NOSUCH_PATH));
        assert!(!has_slash(NOSUCH));
        assert!(has_slash("a/b"));
        assert!(!has_slash(""));
    }

    #[test]
    fn t26_seals_nonzero_and_distinct() {
        let mut i = 0usize;
        while i < MESSAGE_CELLS {
            assert_ne!(cell_seal(&MSG_TABLE[i]), 0);
            let mut j = i + 1usize;
            while j < MESSAGE_CELLS {
                assert_ne!(cell_seal(&MSG_TABLE[i]), cell_seal(&MSG_TABLE[j]));
                j += 1;
            }
            i += 1;
        }
        assert_ne!(table_seal(), 0);
        assert_eq!(table_seal(), table_seal());
    }

    // --- 15 形状 -----------------------------------------------------------

    #[test]
    fn t27_shape_blank() {
        let s = Shape::blank();
        assert_eq!(s.entry, Entry::Symlink);
        assert_eq!(s.form, Form::Bare);
        assert_eq!(s.name, NOSUCH);
        assert_eq!(s.channel, Channel::Stderr);
        assert_eq!(s.exit_declared, EXIT_NOTFOUND);
        assert!(!s.truncated);
        assert!(!s.lang_utf8);
        assert!(s.valid());
        assert_ne!(s.seal(), 0);
    }

    #[test]
    fn t28_shape_valid_rules() {
        assert!(!Shape::blank().with_form(Form::Path).valid());
        assert!(!Shape::blank().with_form(Form::Empty).valid());
        assert!(!Shape::blank().with_entry(Entry::Arg).with_form(Form::Empty).valid());
        assert!(!Shape::blank().with_entry(Entry::CoreArg).with_form(Form::Path).valid());
        assert!(!Shape::blank().with_truncated(true).valid());
        assert!(!Shape::blank().with_name("a/b").valid());
        assert!(!Shape::blank().with_name("").valid());
        assert!(!Shape::blank().with_exit(EXIT_NOTFOUND + 1).valid());
        let mut core_lang = Shape::blank()
            .with_entry(Entry::CoreArg)
            .with_exit(EXIT_FAIL);
        core_lang.lang_utf8 = true;
        assert!(!core_lang.valid());
        // 正路形态。
        assert!(Shape::blank().with_form(Form::Path).with_name(NOSUCH_PATH).valid());
        assert!(Shape::blank()
            .with_entry(Entry::Arg)
            .with_form(Form::Empty)
            .with_name("")
            .valid());
        assert!(Shape::blank()
            .with_entry(Entry::CoreArg)
            .with_exit(EXIT_FAIL)
            .valid());
        assert!(Shape::blank().with_truncated(true).with_entry(Entry::CoreArg).with_exit(EXIT_FAIL).valid());
    }

    #[test]
    fn t29_shape_builders() {
        let b = Shape::blank();
        assert_eq!(b.with_entry(Entry::Arg).entry, Entry::Arg);
        assert_eq!(b.with_form(Form::Path).form, Form::Path);
        assert_eq!(b.with_name(DOTTED).name, DOTTED);
        assert_eq!(b.with_channel(Channel::Stdout).channel, Channel::Stdout);
        assert_eq!(b.with_exit(EXIT_FAIL).exit_declared, EXIT_FAIL);
        assert!(b.with_truncated(true).truncated);
        // 原形状不被改动。
        assert_eq!(b.entry, Entry::Symlink);
        assert_eq!(b.form, Form::Bare);
        assert_eq!(b.name, NOSUCH);
        assert_eq!(b.exit_declared, EXIT_NOTFOUND);
    }

    #[test]
    fn t30_shape_subject_and_text() {
        assert_eq!(Shape::blank().subject(), NOSUCH);
        assert_eq!(
            Shape::blank()
                .with_form(Form::Path)
                .with_name(NOSUCH_PATH)
                .subject(),
            NOSUCH
        );
        assert_eq!(
            Shape::blank()
                .with_entry(Entry::Arg)
                .with_form(Form::Empty)
                .with_name("")
                .subject(),
            MULTICALL_NAME
        );
        assert_eq!(
            Shape::blank().text().as_str(),
            "nosuchapplet: applet not found\n"
        );
        assert_eq!(
            Shape::blank()
                .with_form(Form::Path)
                .with_name(NOSUCH_PATH)
                .text()
                .as_str(),
            "nosuchapplet: applet not found\n"
        );
    }

    #[test]
    fn t31_shape_seal_fields() {
        let b = Shape::blank().seal();
        assert_ne!(b, 0);
        assert_ne!(b, Shape::blank().with_entry(Entry::Arg).seal());
        assert_ne!(
            b,
            Shape::blank()
                .with_form(Form::Path)
                .with_name(NOSUCH_PATH)
                .seal()
        );
        assert_ne!(b, Shape::blank().with_name(DOTTED).seal());
        assert_ne!(b, Shape::blank().with_channel(Channel::Stdout).seal());
        assert_ne!(b, Shape::blank().with_exit(EXIT_FAIL).seal());
        assert_ne!(b, Shape::blank().with_truncated(true).seal());
        assert_eq!(b, Shape::blank().seal());
    }

    // --- 16 define 七闸 ----------------------------------------------------

    #[test]
    fn t32_define_bare() {
        let o = define(&Shape::blank());
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.code, 127);
        assert_eq!(o.value, 1);
        assert_eq!(o.note, Note::AppletNotFound);
        assert!(!o.default);
        assert!(o.consistent);
        assert!(!o.ok());
        assert!(o.failed());
        assert_eq!(Tier::of(&o), Tier::Verbatim);
    }

    #[test]
    fn t33_define_path() {
        let s = Shape::blank().with_form(Form::Path).with_name(NOSUCH_PATH);
        let o = define(&s);
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.note, Note::AppletNotFoundPath);
        assert_eq!(o.code, 127);
        assert_eq!(o.value, 1);
        assert!(o.consistent);
        assert_eq!(Tier::of(&o), Tier::Verbatim);
    }

    #[test]
    fn t34_define_empty_form() {
        let s = Shape::blank()
            .with_entry(Entry::Arg)
            .with_form(Form::Empty)
            .with_name("");
        let o = define(&s);
        assert_eq!(o.note, Note::AppletNotFoundEmpty);
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.code, 127);
        assert_eq!(o.value, 1);
        assert!(o.consistent);
        assert_eq!(Tier::of(&o), Tier::NoBody);
    }

    #[test]
    fn t35_define_core() {
        let s = Shape::blank()
            .with_entry(Entry::CoreArg)
            .with_exit(EXIT_FAIL);
        let o = define(&s);
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.code, 1);
        assert_eq!(o.value, 2);
        assert_eq!(o.note, Note::CoreNotCommand);
        assert!(o.consistent);
        assert!(!o.is_notfound_code());
        assert_eq!(Tier::of(&o), Tier::Bannered);
    }

    #[test]
    fn t36_define_stdout_misrouted() {
        let o = define(&Shape::blank().with_channel(Channel::Stdout));
        assert_eq!(o.stage, Stage::Channel);
        assert_eq!(o.note, Note::Misrouted);
        assert!(!o.consistent);
        assert_eq!(o.code, 127);
        assert_eq!(o.value, 1);
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    #[test]
    fn t37_define_nowhere_misrouted() {
        let o = define(&Shape::blank().with_channel(Channel::Nowhere));
        assert_eq!(o.stage, Stage::Channel);
        assert_eq!(o.note, Note::Misrouted);
        assert_eq!(o.value, 1);
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    #[test]
    fn t38_define_truncated() {
        let s = Shape::blank()
            .with_entry(Entry::CoreArg)
            .with_exit(EXIT_FAIL)
            .with_truncated(true);
        let o = define(&s);
        assert_eq!(o.stage, Stage::Message);
        assert_eq!(o.note, Note::Truncated);
        assert_eq!(o.code, 1);
        assert_eq!(o.value, 1);
        assert!(!o.consistent);
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    #[test]
    fn t39_define_unknown_entry() {
        let s = Shape::blank()
            .with_entry(Entry::Unknown)
            .with_exit(EXIT_FAIL);
        let o = define(&s);
        assert_eq!(o.stage, Stage::Dispatch);
        assert_eq!(o.note, Note::Fallback);
        assert!(o.default);
        assert_eq!(o.code, 1);
        assert_eq!(o.value, 0);
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    #[test]
    fn t40_define_known_applet() {
        let o = define(&Shape::blank().with_name("ls"));
        assert_eq!(o.stage, Stage::Lookup);
        assert_eq!(o.note, Note::KnownApplet);
        assert!(o.default);
        assert_eq!(o.code, 0);
        assert_eq!(o.value, 0);
        assert!(o.ok());
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    #[test]
    fn t41_define_limits() {
        let over = define(&Shape::blank().with_name(OVERLONG_NAME));
        assert_eq!(over.stage, Stage::Name);
        assert_eq!(over.note, Note::Fallback);
        assert!(over.default);
        assert_eq!(over.value, 0);
        let empty = define(&Shape::blank().with_name(""));
        assert_eq!(empty.stage, Stage::Name);
        assert_eq!(empty.note, Note::Silent);
        assert!(!empty.consistent);
        assert!(!empty.default);
        assert_eq!(empty.value, 0);
    }

    #[test]
    fn t42_define_wrong_declared_code() {
        let o = define(&Shape::blank().with_exit(EXIT_FAIL));
        assert!(!o.consistent);
        assert_eq!(o.code, 127);
        assert_eq!(o.note, Note::AppletNotFound);
        assert_eq!(o.stage, Stage::Exit);
        assert_eq!(o.value, 1);
        assert!(!o.ok());
        assert_eq!(Tier::of(&o), Tier::Refused);
    }

    // --- 17 结论 / 申报 / 一槽 ---------------------------------------------

    #[test]
    fn t43_outcome_helpers() {
        let b = Outcome::blank();
        assert_eq!(b.code, 0);
        assert_eq!(b.value, 0);
        assert_eq!(b.note, Note::Silent);
        assert!(!b.failed());
        assert!(!b.refused());
        assert_eq!(b.exit(), EXIT_OK);
        assert!(!b.diag_ok());
        assert!(!b.is_notfound_code());
        let c = define(&Shape::blank());
        assert!(!c.ok());
        assert!(c.failed());
        assert!(!c.anchored());
        assert!(!c.refused());
        assert_eq!(c.exit(), EXIT_NOTFOUND);
        assert!(c.diag_ok());
        assert!(c.is_notfound_code());
        let d = define(&Shape::blank()
            .with_entry(Entry::Unknown)
            .with_exit(EXIT_FAIL));
        assert!(d.anchored());
        assert_eq!(d.exit(), EXIT_FAIL);
        assert!(!d.diag_ok());
    }

    #[test]
    fn t44_outcome_seal() {
        let a = define(&Shape::blank());
        let b = define(&Shape::blank().with_channel(Channel::Stdout));
        assert_ne!(a.seal(), 0);
        assert_ne!(a.seal(), b.seal());
        assert_eq!(a.seal(), define(&Shape::blank()).seal());
        assert_ne!(Outcome::blank().seal(), a.seal());
    }

    #[test]
    fn t45_claim_table() {
        let c = Claim::blank();
        assert!(!c.success());
        assert_eq!(c.stage, Stage::Dispatch);
        assert_eq!(c.code, 0);
        assert_eq!(c.value, 0);
        assert_eq!(c.note, Note::Silent);
        assert!(!c.default);
        let k = Claim::make(true, Stage::Exit, 127, 1, Note::AppletNotFound, false);
        assert!(k.success());
        assert_ne!(k.seal(), c.seal());
        assert_eq!(k.seal(), k.seal());
    }

    #[test]
    fn t46_case_blank() {
        let c = Case::blank();
        assert_eq!(c.tag(), "f11674-blank");
        assert_eq!(c.actual().note, Note::AppletNotFound);
        assert_eq!(c.fault(), Fault::Stage);
        assert!(!c.ok());
        assert_ne!(c.seal(), 0);
    }

    // --- 18 判序 -----------------------------------------------------------

    #[test]
    fn t47_judge_order() {
        let base = Shape::blank();
        let one_to_one = Case {
            shape: base,
            claim: Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFound, false),
            tag: "f11674-t47-ok",
        };
        assert_eq!(judge(&one_to_one), Fault::Ok);
        let estate = Case {
            shape: base,
            claim: Claim::make(true, Stage::Exit, 127, 1, Note::AppletNotFound, false),
            tag: "f11674-t47-estate",
        };
        assert_eq!(judge(&estate), Fault::Estate);
        let stage = Case {
            shape: base,
            claim: Claim::make(false, Stage::Message, 127, 1, Note::AppletNotFound, false),
            tag: "f11674-t47-stage",
        };
        assert_eq!(judge(&stage), Fault::Stage);
        let code = Case {
            shape: base,
            claim: Claim::make(false, Stage::Exit, 1, 1, Note::AppletNotFound, false),
            tag: "f11674-t47-code",
        };
        assert_eq!(judge(&code), Fault::Code);
        let value = Case {
            shape: base,
            claim: Claim::make(false, Stage::Exit, 127, 2, Note::AppletNotFound, false),
            tag: "f11674-t47-value",
        };
        assert_eq!(judge(&value), Fault::Value);
        let note = Case {
            shape: base,
            claim: Claim::make(false, Stage::Exit, 127, 1, Note::CoreNotCommand, false),
            tag: "f11674-t47-note",
        };
        assert_eq!(judge(&note), Fault::Note);
        let def = Case {
            shape: base,
            claim: Claim::make(false, Stage::Exit, 127, 1, Note::AppletNotFound, true),
            tag: "f11674-t47-default",
        };
        assert_eq!(judge(&def), Fault::Default);
        // 多维同错：只报判序最前的那一维。
        let two = Case {
            shape: base,
            claim: Claim::make(false, Stage::Message, 1, 2, Note::CoreNotCommand, false),
            tag: "f11674-t47-two",
        };
        assert_eq!(judge(&two), Fault::Stage);
        let estate_first = Case {
            shape: base,
            claim: Claim::make(true, Stage::Message, 1, 2, Note::CoreNotCommand, false),
            tag: "f11674-t47-estate-first",
        };
        assert_eq!(judge(&estate_first), Fault::Estate);
    }

    // --- 19 语料 -----------------------------------------------------------

    #[test]
    fn t48_accepts_all_green() {
        let mut i = 0usize;
        while i < ACCEPT {
            match case_at(i) {
                Some(c) => {
                    assert_eq!(c.fault(), Fault::Ok);
                    assert!(c.ok());
                    assert!(!c.actual().default);
                }
                None => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn t49_rejects_one_dimension_each() {
        let want = [
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ];
        let mut k = ACCEPT;
        while k < CORPUS {
            let c = match case_at(k) {
                Some(x) => x,
                None => {
                    assert!(false);
                    return;
                }
            };
            let a = c.actual();
            let mut offs = 0usize;
            if a.ok() != c.claim.success() {
                offs += 1;
            }
            if a.stage != c.claim.stage {
                offs += 1;
            }
            if a.code != c.claim.code {
                offs += 1;
            }
            if a.value != c.claim.value {
                offs += 1;
            }
            if a.note != c.claim.note {
                offs += 1;
            }
            if a.default != c.claim.default {
                offs += 1;
            }
            assert_eq!(c.fault(), want[k - ACCEPT]);
            if k == CORPUS - 1 {
                // 末槽：申报与真值一个字不差，但真值本身落兜底——兜底位独立判死。
                assert_eq!(offs, 0);
                assert!(a.default);
                assert!(c.claim.default);
            } else {
                assert_eq!(offs, 1);
            }
            k += 1;
        }
    }

    #[test]
    fn t50_corpus_matches_want_tables() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => {
                    assert!(false);
                    return;
                }
            };
            let a = c.actual();
            assert_eq!(a.stage, WANT_STAGE[i]);
            assert_eq!(a.exit(), WANT_CODE[i]);
            assert_eq!(a.value, WANT_VALUE[i]);
            assert_eq!(a.note, WANT_NOTE[i]);
            assert_eq!(a.default, WANT_DEFAULT[i]);
            assert_eq!(c.claim.success(), WANT_SUCCESS[i]);
            assert_eq!(fault_ndx(c.fault()), WANT_NDX[i]);
            i += 1;
        }
    }

    #[test]
    fn t51_tags_distinct() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => {
                    assert!(false);
                    return;
                }
            };
            assert!(!c.tag().is_empty());
            assert!(c.tag().starts_with("f11674-"));
            let mut j = i + 1usize;
            while j < CORPUS {
                if let Some(o) = case_at(j) {
                    assert_ne!(c.tag(), o.tag());
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t52_slot_seals_distinct() {
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => {
                    assert!(false);
                    return;
                }
            };
            assert_ne!(c.seal(), 0);
            let mut j = i + 1usize;
            while j < CORPUS {
                if let Some(o) = case_at(j) {
                    assert_ne!(c.seal(), o.seal());
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t53_case_lookup_edges() {
        assert!(case_at(CORPUS).is_none());
        assert!(case_at(CORPUS + 9).is_none());
        assert_eq!(cases().len(), CORPUS);
        assert_eq!(cover(0), Ok(()));
        assert_eq!(cover(CORPUS), Ok(()));
        assert_eq!(cover(CORPUS + 1), Err(NF_ESLOT));
        assert_eq!(expected(ACCEPT), Some(Fault::Estate));
        assert_eq!(expected(CORPUS - 1), Some(Fault::Default));
        assert!(expected(CORPUS).is_none());
    }

    // --- 20 矩阵与报告 -----------------------------------------------------

    #[test]
    fn t54_matrix_counts() {
        let m = Matrix::build();
        assert_eq!(m.slots(), CORPUS * TIER_COUNT);
        assert_eq!(m.slots(), 56);
        assert_eq!(m.goods(), CORPUS);
        assert_eq!(m.goods(), 14);
        assert_eq!(m.red_total(), 56 - 14);
        assert_eq!(m.perfect_rows(), 0);
        assert!(m.covered());
        assert_eq!(m.render().starts_with("matrix "), true);
        match m.row(Tier::Verbatim) {
            Some(r) => assert_eq!(r.good, 10),
            None => assert!(false),
        }
        match m.row(Tier::Bannered) {
            Some(r) => assert_eq!(r.good, 1),
            None => assert!(false),
        }
        match m.row(Tier::NoBody) {
            Some(r) => assert_eq!(r.good, 1),
            None => assert!(false),
        }
        match m.row(Tier::Refused) {
            Some(r) => assert_eq!(r.good, 2),
            None => assert!(false),
        }
    }

    #[test]
    fn t55_report_counts() {
        let rep = match run() {
            Ok(r) => r,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        assert_eq!(rep.total, 14);
        assert_eq!(rep.accept, 8);
        assert_eq!(rep.reject, 6);
        assert_eq!(rep.verbatim, 10);
        assert_eq!(rep.bannered, 1);
        assert_eq!(rep.nobody, 1);
        assert_eq!(rep.refused, 2);
        assert_eq!(rep.matched, 14);
        assert_eq!(rep.tier_cells(), 14);
        assert_eq!(rep.rate(), 1000);
        assert_eq!(rep.reds(), 0);
        assert!(rep.green());
        assert_eq!(rep.render().as_str(), RENDER_OK);
        assert_eq!(rep.gate(), Ok(()));
    }

    #[test]
    fn t56_empty_report_refuses() {
        let e = empty_report();
        assert_eq!(e.total, 0);
        assert_eq!(e.rate(), 0);
        assert!(!e.green());
        assert_eq!(e.gate(), Err(NF_ENOBS));
        assert_eq!(e.reds(), 0);
        assert_ne!(e.render().as_str(), RENDER_OK);
    }

    #[test]
    fn t57_run_and_run_with() {
        assert!(run().is_ok());
        let bad = match run_with(Some((2, Fault::Value))) {
            Ok(r) => r,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        assert_eq!(bad.matched, 13);
        assert_eq!(bad.rate(), 929);
        assert!(!bad.green());
        assert_eq!(bad.gate(), Err(NF_EBAL));
        assert_ne!(bad.render().as_str(), RENDER_OK);
        assert_eq!(run_with(Some((CORPUS, Fault::Ok))), Err(NF_ESLOT));
        assert_eq!(run_with(Some((CORPUS + 3, Fault::Note))), Err(NF_ESLOT));
        assert!(run_with(None).is_ok());
    }

    #[test]
    fn t58_red_rate_calibration() {
        assert_eq!(RED_RATE, 929);
        assert!(red_rate_calibrated());
        let want = (13u32 * RATE_FULL + 7) / 14;
        assert_eq!(want, RED_RATE);
    }

    #[test]
    fn t59_inject_four_routes() {
        let four = inject_four();
        assert_eq!(four.len(), 4);
        let mut i = 0usize;
        while i < four.len() {
            assert!(four[i]);
            i += 1;
        }
        assert!(report_agrees_with_matrix());
    }

    // --- 21 阶段闸门 -------------------------------------------------------

    #[test]
    fn t60_gate_new_and_walk() {
        let g = Gate::new();
        assert!(g.stage().is_none());
        assert_eq!(g.steps(), 0);
        assert_eq!(g.refusals(), 0);
        assert!(!g.is_closed());
        assert_eq!(g.root(), FNV_OFF);
        let root = match Gate::walk_all() {
            Ok(x) => x,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        assert_ne!(root, 0);
        assert_ne!(root, FNV_OFF);
    }

    #[test]
    fn t61_gate_regression_refused() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Exit), Ok(()));
        assert_eq!(g.step(Stage::Code), Err(NF_ESTAGE));
        assert_eq!(g.stage(), Some(Stage::Exit));
        assert_eq!(g.steps(), 1);
        assert_eq!(g.refusals(), 1);
        assert_eq!(g.step(Stage::Dispatch), Err(NF_ESTAGE));
        assert_eq!(g.refusals(), 2);
    }

    #[test]
    fn t62_gate_closed_refuses_all() {
        let mut g = Gate::new();
        assert_eq!(g.step(Stage::Dispatch), Ok(()));
        let root = match g.close() {
            Ok(x) => x,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        assert_ne!(root, 0);
        assert!(g.is_closed());
        assert_eq!(g.root(), root);
        assert_eq!(g.step(Stage::Name), Err(NF_ESTATE));
        assert_eq!(g.close(), Err(NF_ESTATE));
        assert_eq!(g.refusals(), 2);
        assert_eq!(g.steps(), 1);
    }

    #[test]
    fn t63_gate_close_needs_a_step() {
        let mut g = Gate::new();
        assert_eq!(g.close(), Err(NF_ESTAGE));
        assert!(!g.is_closed());
        assert_eq!(g.steps(), 0);
        // 同档重复不算倒退；跳档允许。
        let mut d = Gate::new();
        assert_eq!(d.step(Stage::Name), Ok(()));
        assert_eq!(d.step(Stage::Name), Ok(()));
        assert_eq!(d.steps(), 2);
        assert_eq!(d.refusals(), 0);
        assert_eq!(d.step(Stage::Exit), Ok(()));
        assert_eq!(d.stage(), Some(Stage::Exit));
    }

    // --- 22 台账 -----------------------------------------------------------

    #[test]
    fn t64_ledger_empty() {
        let l = Ledger::new();
        assert_eq!(l.len(), 0);
        assert!(l.is_empty());
        assert_eq!(l.ok_count(), 0);
        assert_eq!(l.err_count(), 0);
        assert_eq!(l.rejects(), 0);
        assert!(!l.is_frozen());
        assert_eq!(l.root(), 0);
        assert_eq!(l.first_mismatch(), Some((0, "len")));
        assert_eq!(l.audit(), Err(NF_ESLOT));
        assert_eq!(l.replay(), Err(NF_ESTATE));
    }

    #[test]
    fn t65_ledger_register_order() {
        let mut l = Ledger::new();
        let stray = Sample {
            slot: 1,
            op: OP_DISPATCH,
            ok: true,
            code: 0,
            value: 1,
        };
        assert_eq!(l.register(stray), Err(NF_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert!(l.is_empty());
        let first = Sample {
            slot: 0,
            op: OP_DISPATCH,
            ok: true,
            code: 0,
            value: STAGE_COUNT,
        };
        assert_eq!(l.register(first), Ok(()));
        assert_eq!(l.len(), 1);
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.sample(0), Some(first));
        assert!(l.sample(1).is_none());
    }

    #[test]
    fn t66_ledger_audit_counts() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), STAT_CELLS + 1);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), REJECT);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.first_mismatch().is_none());
        assert_ne!(l.render().as_str(), "");
    }

    #[test]
    fn t67_ledger_freeze_and_replay() {
        let mut l = build_ledger();
        let h = match l.freeze() {
            Ok(x) => x,
            Err(_) => {
                assert!(false);
                return;
            }
        };
        assert_ne!(h, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), h);
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(NF_ESTATE));
        assert_eq!(l.rejects(), 0);
    }

    #[test]
    fn t68_ledger_poke() {
        let mut l = build_ledger();
        assert_eq!(l.poke(0, 1), Ok(()));
        assert_eq!(l.audit(), Err(NF_EVALUE));
        assert_eq!(l.poke(SAMPLES, 1), Err(NF_ESLOT));
        let mut f = build_ledger();
        assert!(f.freeze().is_ok());
        assert_eq!(f.poke(0, 1), Err(NF_ESTATE));
    }

    #[test]
    fn t69_ledger_injections_caught() {
        let mut w = 0u8;
        while w < 4 {
            let mut b = build_ledger_broken(w);
            assert_ne!(b.audit(), Ok(()));
            assert!(b.freeze().is_err());
            assert!(b.first_mismatch().is_some());
            w += 1;
        }
        // 越界注入号不改账本。
        let untouched = build_ledger_broken(9);
        assert_eq!(untouched.audit(), Ok(()));
    }

    // --- 23 统计表与判据汇总 -----------------------------------------------

    #[test]
    fn t70_vals_table() {
        assert_eq!(VALS.len(), ACCEPT);
        assert_eq!(VALS[0], STAGE_COUNT);
        assert_eq!(VALS[1], ENTRY_JUDGED as u32);
        assert_eq!(VALS[2], MESSAGE_CELLS as u32);
        assert_eq!(VALS[3], CORPUS as u32);
        assert_eq!(VALS[4], ACCEPT as u32);
        assert_eq!(VALS[5], REJECT as u32);
        assert_eq!(VALS[6], RATE_FULL);
        assert_eq!(VALS[7], RED_RATE);
        assert_eq!(head_val(), STAGE_COUNT);
    }

    #[test]
    fn t71_checks_are_thirteen() {
        let s = run_unx11674_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.passed_count(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        match s.get(0) {
            Some(c) => assert_eq!(c.name, "NF01-名称与段基址"),
            None => assert!(false),
        }
        match s.get(12) {
            Some(c) => {
                assert!(c.name.contains("13-冻结重放四注入"));
                assert!(c.passed);
            }
            None => assert!(false),
        }
        assert!(s.get(13).is_none());
    }

    #[test]
    fn t72_head_tag() {
        assert_eq!(head_tag(), "f11674-symlink-bare");
        assert_eq!(head_tag(), WANT_TAGS[0]);
    }

    #[test]
    fn t73_text_helpers() {
        assert_eq!(dec_text(0).as_str(), "0");
        assert_eq!(dec_text(127).as_str(), "127");
        assert_eq!(dec_text(1000).as_str(), "1000");
        assert_eq!(hex_text(0x6d270, 6).as_str(), "06d270");
        assert_eq!(seg_text(SEG_BASE).as_str(), "0x06d270");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(i32_text(-5).as_str(), "-5");
        assert_eq!(i32_text(0).as_str(), "0");
        assert_eq!(bool_text(true), "true");
        assert_eq!(bool_text(false), "false");
        assert_eq!(permille_text(1000).as_str(), "1000permille");
    }
}
