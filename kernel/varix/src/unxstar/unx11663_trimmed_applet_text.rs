//! UNX-F11663 裁剪后已知 applet 缺失报错判据
//!
//! busybox 的 applet 表是**编译期**产物：`applet_name[]` / `applet_main[]` 两张
//! 平行表由 `applets/applet_switch.c` 那套宏按当时的 `CONFIG_…` 展开。把某个
//! applet 裁掉（`CONFIG_LS=n`），它的名字就从表里消失了——于是
//! `busybox ls` 走的不再是"入口函数"，而是和 `busybox zzz` 一模一样的那条
//! notfound 分支。本卡判的就是这件事：**被裁掉的已知名字，必须与从未存在过的
//! 名字给出逐字相同的答复**。
//!
//! 判据原文（UNX-F11663-J1）：裁剪件 notfound 同文案判据绿，与 F11595 联签。
//! 这句话里有两个各自能单独失效的东西：
//!
//! 1. **同文案**：文案面（`busybox: 'ls': applet not found`）、流面（只走
//!    stderr，stdout 必须一个字节都不留）、码档面（127，不是 1、不是 0）三条轴
//!    必须同时与未知名一致。少盯一条，就会出现"文案对但退 1"或"退 127 但把
//!    报错打进 stdout"这两种在真实裁剪件里都发生过的构建。
//! 2. **联签**：F11595（CONFIG 裁剪重建 applet 表收缩判据）负责"表收缩了多少、
//!    收缩后的表指纹是多少"；本卡负责"收缩掉的那个名字被叫到时怎么答复"。两张
//!    单子必须说的是**同一次裁剪**——同一档 `CONFIG_…` 掩码、同一个表指纹。否则
//!    F11595 绿的是 A 构建、本卡绿的是 B 构建，合起来等于什么都没验。
//!
//! 闸门次序（八道，逐闸定局，后面的闸不再消费前面已违约的事实）：
//! 构建（Build）→ 表（Table）→ 身份（Identity）→ 流（Stream）→ 文案（Text）→
//! 码档（Exit）→ 泄漏（Leak）→ 封印（Seal）。次序不可倒置的理由：
//!
//! * 构建先于表：`CONFIG_…` 缺席掩码与表指纹互相指认，掩码说"裁掉了"而指纹里
//!   还留着那位，是环，不是"文案还没看"。
//! * 表先于身份：名字在不在表里是表的事实；先问身份就会把"表里没有"误读成
//!   "这条判据没登记"。
//! * 身份先于流：流是答复的载体，载体归属要等"这条答复属于哪个请求"定了才谈。
//! * 流先于文案：文案往哪条流写，决定后面逐字比对读到的是不是那份字节。
//! * 文案先于码档：127 是"没找到"的约定档，但约定档必须跟着逐字文案一起成立；
//!   先看码会把"文案错但码对"的构建放过去。
//! * 码档先于泄漏：泄漏面问的是"答复里有没有说出'它存在但被裁'"，这必须等标准
//!   答复三面都过了才谈——否则任何一条红文案都会被重复计成泄漏。
//! * 泄漏先于封印：只有"答得像未知名、且没多说出任何裁剪事实"的行，才有资格
//!   进默认必跑位。
//!
//! 关键差异（与同批 F11661/F11662 的分工，避免三条单子重复验同一件事）：
//!
//! * F11661 验**未知名**的直接调用面：名字从未进过表，文案/流/码三轴的基准档
//!   由它定义。
//! * F11662 验 `argv[0]` **伪名**（软链接名、带路径前缀、空 argv）怎么参与查表。
//! * 本卡验**已知但被裁**的名字：查表的输入在另一份构建里是合法的，因此多出一
//!   个 F11661/F11662 都没有的维度——泄漏面。裁剪件不许因为"知道它曾经存在"而
//!   换一种说法（`applet is disabled` / 候选名列表 / 提示去开 CONFIG），换了就是
//!   红，哪怕文案再像。
//!
//! 本模块是 `no_std` 纯功能判据：不读真实 /proc、不 fork 进程，把上述八闸与三轴
//! 建成一张可复核的事实表，再由十三项聚合判据（TR01…TR13）一次全跑。所有取值走
//! `match` / `.get()` / `checked_*` / `saturating_*`，生产码零 `unwrap`、零
//! `unsafe`、零 `std`。

use crate::alloc::string::String;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 段基址与错误码
// ---------------------------------------------------------------------------

/// 本卡段基址：F11663 在 VX-1029-5 批里占用 0x6d120 段。
///
/// 同批邻居（不得撞）：F11661=0x6d100、F11662=0x6d110、F11681=0x6d130、
/// F11682=0x6d140、F11683=0x6d150。本卡取中间空出的 0x6d120。
pub const SEG_BASE: u32 = 0x6d120;

/// 成败互报：申报绿而实测红（或反之）。
pub const TR_ESTATE: u32 = SEG_BASE + 0x01u32;
/// 闸位错：停在的闸与申报的闸不是同一道。
pub const TR_ESTAGE: u32 = SEG_BASE + 0x02u32;
/// 码档错：退出码或 errno 与"未知名"的约定档不一致。
pub const TR_ECODE: u32 = SEG_BASE + 0x03u32;
/// 数值错：文案字节数、断言数、表项数这类可数事实对不上。
pub const TR_EVALUE: u32 = SEG_BASE + 0x04u32;
/// 注记错：文案形态档（现代/旧版/noapplet 桩）认错。
pub const TR_ENOTE: u32 = SEG_BASE + 0x05u32;
/// 默认必跑位丢失：该进默认套件的行没进，或不该进的混进来了。
pub const TR_EDEFAULT: u32 = SEG_BASE + 0x06u32;
/// 清单数错：构建面数出来的 applet 项数与清单不符。
pub const TR_ECOUNT: u32 = SEG_BASE + 0x07u32;
/// 身份撞车：卡号、短名或请求名重复，联签锚对不上。
pub const TR_EUNIQ: u32 = SEG_BASE + 0x08u32;
/// 成环：CONFIG 缺席掩码与表指纹互相指认，谁也不肯先落地。
pub const TR_ECYCLE: u32 = SEG_BASE + 0x09u32;
/// 前置缺席：载体或第二台机器不在，这条根本没跑成。
pub const TR_ESKIP: u32 = SEG_BASE + 0x0au32;
/// 抖动未显式：重试过却说首次即中（洗绿）。
pub const TR_EFLAKE: u32 = SEG_BASE + 0x0bu32;
/// 前置未达成却计绿：F11595 还没联签就宣布裁剪件绿。
pub const TR_EBLOCK: u32 = SEG_BASE + 0x0cu32;
/// 槽位越界：往不存在的槽写结论。
pub const TR_ESLOT: u32 = SEG_BASE + 0x0du32;

/// 十三码表——按 `SEG_BASE + 0x01..0x0d` 的次序排列，供逐码点名与撞码自检。
pub const CODE_TABLE: [u32; 13usize] = [
    TR_ESTATE,
    TR_ESTAGE,
    TR_ECODE,
    TR_EVALUE,
    TR_ENOTE,
    TR_EDEFAULT,
    TR_ECOUNT,
    TR_EUNIQ,
    TR_ECYCLE,
    TR_ESKIP,
    TR_EFLAKE,
    TR_EBLOCK,
    TR_ESLOT,
];

/// 错误码 → 文本名。
pub fn code_name(code: u32) -> &'static str {
    match code {
        0x6d121u32 => "tr-estate",
        0x6d122u32 => "tr-estage",
        0x6d123u32 => "tr-ecode",
        0x6d124u32 => "tr-evalue",
        0x6d125u32 => "tr-enote",
        0x6d126u32 => "tr-edefault",
        0x6d127u32 => "tr-ecount",
        0x6d128u32 => "tr-euniq",
        0x6d129u32 => "tr-ecycle",
        0x6d12au32 => "tr-eskip",
        0x6d12bu32 => "tr-eflake",
        0x6d12cu32 => "tr-eblock",
        0x6d12du32 => "tr-eslot",
        0u32 => "none",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// 契约常量
// ---------------------------------------------------------------------------

/// 语料槽数：八活六死。
pub const CORPUS: usize = 14usize;
/// 账本样本数。
pub const SAMPLES: usize = 14usize;
/// 采纳槽数。
pub const ACCEPT: usize = 8usize;
/// 否决槽数。
pub const REJECT: usize = 6usize;
/// 档位数量（绿/抖/跳/阻）。
pub const TIER_COUNT: usize = 4usize;
/// B24 批清单行数：F11661..F11679 共 19 条。
pub const BENCH: usize = 19usize;
/// B24 批首卡号。
pub const CARD_MIN: u16 = 11661u16;
/// B24 批末卡号。
pub const CARD_MAX: u16 = 11679u16;
/// 本卡卡号。
pub const CARD: u16 = 11663u16;
/// 联签对象：F11595（applet 表收缩）。
pub const CO_SIGN_CARD: u16 = 11595u16;
/// 全绿通过率。
pub const RATE_FULL: u32 = 1000u32;
/// 一维坏掉后的通过率——14 槽里中 13 槽：(13*1000 + 14/2) / 14 = 929。
pub const RED_RATE: u32 = 929u32;
/// FNV-1a 64 位偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325u64;
/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3u64;

/// 正常退出码。
pub const EX_OK: u16 = 0u16;
/// notfound 档——shell 对 "command not found" 的约定码。
pub const EX_NOTFOUND: u16 = 127u16;
/// 通用失败档——裁剪件最容易"看起来也对"地退这个。
pub const EX_GENERAL: u16 = 1u16;
/// 误用档（usage 走 stdout 后退 0 的那一类）。
pub const EX_USAGE: u16 = 1u16;
/// 信号档基址。
pub const EX_SIG_BASE: u16 = 128u16;
/// SIGSEGV。
pub const SIG_SEGV: u16 = 11u16;
/// SIGALRM——超时看门狗。
pub const SIG_ALRM: u16 = 14u16;

/// errno 无。
pub const ENONE: u16 = 0u16;
/// ENOENT：文件不存在——exec 链路那一侧的对照档。
pub const ENOENT: u16 = 2u16;
/// EACCES：权限不足，与"没找到"必须分开。
pub const EACCES: u16 = 13u16;
/// EINVAL：参数无效，裁剪件里 `--help` 走歪时会撞见。
pub const EINVAL: u16 = 22u16;

/// busybox 报错的程序名档：`argv[0]` 的 basename。
pub const PROG: &str = "busybox";

/// 主锚文案的中间段：`'<name>': applet not found`。
pub const MID_NOTFOUND: &str = "': applet not found";
/// 旧版档尾：`applet not found`（前面不带名字）。
pub const TAIL_LEGACY: &str = ": applet not found";
/// 桩体档中间段：`applet '<name>' not found`。
pub const MID_STUB: &str = ": applet '";
/// 桩体档尾。
pub const TAIL_STUB: &str = "' not found";
/// 禁用档——真实裁剪件**不许**说的话。
pub const FORBIDDEN_DISABLED: &str = "is disabled";
/// 候选档——真实裁剪件**不许**说的话。
pub const FORBIDDEN_DIDYOU: &str = "did you mean";
/// CONFIG 提示档——真实裁剪件**不许**说的话。
pub const FORBIDDEN_CONFIG: &str = "CONFIG_";

/// CONFIG 位（**缺席**掩码：置 1 表示这一项不在构建里）。
pub const CFG_APPLET: u32 = 1u32 << 0;
/// ash 不在（影响 shell 侧对照）。
pub const CFG_ASH: u32 = 1u32 << 1;
/// 静态构建位。
pub const CFG_STATIC: u32 = 1u32 << 2;
/// FEATURE_NO_APPLET 桩不在。
pub const CFG_NOAPPLET: u32 = 1u32 << 3;
/// trivial usage 位。
pub const CFG_USAGE: u32 = 1u32 << 4;
/// 详细 usage 位。
pub const CFG_VERBOSE_USAGE: u32 = 1u32 << 5;
/// 双机构建位。
pub const CFG_TWO_MACHINE: u32 = 1u32 << 6;
/// 裁剪矩阵位（三档重建）。
pub const CFG_TRIM_MATRIX: u32 = 1u32 << 7;
/// i18n 位。
pub const CFG_I18N: u32 = 1u32 << 8;
/// 长选项解析位。
pub const CFG_LONG_OPTS: u32 = 1u32 << 9;

/// 全缺席掩码。
pub const CFG_ALL: u32 = CFG_APPLET
    | CFG_ASH
    | CFG_STATIC
    | CFG_NOAPPLET
    | CFG_USAGE
    | CFG_VERBOSE_USAGE
    | CFG_TWO_MACHINE
    | CFG_TRIM_MATRIX
    | CFG_I18N
    | CFG_LONG_OPTS;
/// 无缺席（满配置）。
pub const CFG_NONE: u32 = 0u32;

// ---------------------------------------------------------------------------
// 折叠与文本工具
// ---------------------------------------------------------------------------

/// 单字节 FNV-1a。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 八字节逐位 FNV-1a（低半字按 4 位切片，保证同一数值在任何字序下折出同一值）。
pub fn fold_word(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < 8 {
        let byte = ((v >> (i as u32 * 8u32)) & 0xffu64) as u8;
        x = fold_byte(x, byte);
        i += 1;
    }
    x
}

/// 字符串逐字节折叠。
pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    for b in s.as_bytes() {
        x = fold_byte(x, *b);
    }
    x
}

/// 32 位指纹折叠（表指纹与联签锚都是 u32 现场）。
pub fn fold_u32(h: u64, v: u32) -> u64 {
    fold_word(h, v as u64)
}

/// u32 → 十进制文本。
pub fn dec_text(v: u32) -> String {
    if v == 0u32 {
        return String::from("0");
    }
    let mut digits = [0u8; 10usize];
    let mut n = v;
    let mut k = 0usize;
    while n > 0u32 && k < 10usize {
        if let Some(d) = digits.get_mut(k) {
            *d = (b'0' as u8) + (n % 10u32) as u8;
        }
        n /= 10u32;
        k = k.saturating_add(1usize);
    }
    let mut out = String::new();
    let mut i = k;
    while i > 0usize {
        i = i.saturating_sub(1usize);
        if let Some(b) = digits.get(i) {
            out.push(*b as char);
        }
    }
    out
}

/// u32 → 定宽小写十六进制。
pub fn hex_text(v: u32, width: usize) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < width {
        let shift = ((width.saturating_sub(i).saturating_sub(1usize)) as u32) * 4u32;
        let nib = (v >> shift) & 0x0fu32;
        let c = if nib < 10u32 {
            (b'0' as u32) + nib
        } else {
            (b'a' as u32) + nib.saturating_sub(10u32)
        };
        out.push(c as u8 as char);
        i = i.saturating_add(1usize);
    }
    out
}

/// u32 文本（与 dec_text 同义，保留给契约里按名字点名的场合）。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 文本：负号单独走一遍，避免对绝对值取反时撞上 i32::MIN。
pub fn i32_text(v: i32) -> String {
    if v >= 0i32 {
        return dec_text(v as u32);
    }
    let mag = match v.checked_neg() {
        Some(x) => x as u32,
        None => 2147483647u32 + 1u32,
    };
    let mut out = String::from("-");
    out.push_str(dec_text(mag).as_str());
    out
}

/// 布尔 → TAP 裁决文本。
///
/// 本卡的布尔位直接喂 TAP 行，取 ktest 面上的 "ok"/"not ok"；写成 "true"
/// 会让解析器把整行读成 unknown（同批 F11660 已踩过一次）。
pub fn bool_text(v: bool) -> &'static str {
    match v {
        true => "ok",
        false => "not ok",
    }
}

/// CONFIG 位 → 文本名。
///
/// 只有真正的 Kconfig 项给 `CONFIG_…` 前缀；构建属性（静态/双机/裁剪矩阵）
/// 用 busybox 自己的叫法，混成一个前缀会让 skip 注释骗人。
pub fn cfg_name(bit: u32) -> &'static str {
    match bit {
        1u32 => "CONFIG_APPLET",
        2u32 => "CONFIG_ASH",
        4u32 => "CONFIG_STATIC",
        8u32 => "CONFIG_FEATURE_NO_APPLET",
        16u32 => "CONFIG_APPLET_USES_STDOUT_FOR_ALL",
        32u32 => "CONFIG_VERBOSE_USAGE",
        64u32 => "two-machine",
        128u32 => "trim-matrix",
        256u32 => "CONFIG_TRIVIAL_AGENT_I18N",
        512u32 => "CONFIG_FEATURE_LONG_OPTS",
        _ => "none",
    }
}

/// 缺席掩码里最低的那一位 → 名字（掩码方向：置 1 = 缺）。
pub fn first_absent_name(missing: u32) -> &'static str {
    let mut i = 0usize;
    while i < 10usize {
        let bit = 1u32 << i;
        if (missing & bit) != 0u32 {
            return cfg_name(bit);
        }
        i = i.saturating_add(1usize);
    }
    "none"
}

// ---------------------------------------------------------------------------
// 文案成形：三条轴的唯一出口
// ---------------------------------------------------------------------------

/// 现代主锚：`busybox: 'ls': applet not found`。
///
/// 程序名 + 冒号空格 + 单引号包住的名字 + `': applet not found`。名字**被裁**
/// 与名字**从未存在**在这一档里产出逐字相同的行——这是本卡的同文案基准。
pub fn text_modern(name: &str) -> String {
    let mut s = String::from(PROG);
    s.push_str(": '");
    s.push_str(name);
    s.push_str(MID_NOTFOUND);
    s
}

/// 旧版档：`busybox: applet not found`（没有名字那一段）。
pub fn text_legacy() -> String {
    let mut s = String::from(PROG);
    s.push_str(TAIL_LEGACY);
    s
}

/// `noapplet` 桩档：`busybox: applet 'ls' not found`。
pub fn text_stub(name: &str) -> String {
    let mut s = String::from(PROG);
    s.push_str(MID_STUB);
    s.push_str(name);
    s.push_str(TAIL_STUB);
    s
}

/// 禁用泄漏档：真实裁剪件不该说的话，本模块把它当**反面**构造出来供判死。
pub fn text_disabled(name: &str) -> String {
    let mut s = String::from(PROG);
    s.push_str(": '");
    s.push_str(name);
    s.push_str("' applet ");
    s.push_str(FORBIDDEN_DISABLED);
    s
}

/// 候选泄漏档：`did you mean 'cls'?`——同样是泄漏。
pub fn text_didyou(name: &str, cand: &str) -> String {
    let mut s = String::from(PROG);
    s.push_str(": '");
    s.push_str(name);
    s.push_str(MID_NOTFOUND);
    s.push_str(", ");
    s.push_str(FORBIDDEN_DIDYOU);
    s.push_str(" '");
    s.push_str(cand);
    s.push('\'');
    s
}

/// CONFIG 提示泄漏档。
pub fn text_config_hint(name: &str) -> String {
    let mut s = String::from(PROG);
    s.push_str(": '");
    s.push_str(name);
    s.push_str(MID_NOTFOUND);
    s.push_str(" (enable ");
    s.push_str(FORBIDDEN_CONFIG);
    s.push_str("APPLET_");
    s.push_str(name);
    s.push_str(" in .config)");
    s
}

/// 空答复：什么也没写。裁剪件里 `--list` 走歪时会撞到这一档。
pub fn text_empty() -> String {
    String::new()
}

// ---------------------------------------------------------------------------
// 事实枚举
// ---------------------------------------------------------------------------

/// 八道闸。rank 是 **1 基**——同批 F11660 的教训：0 基会让"第一步只能落在清单闸"
/// 这条规则变成"第一步落在 0"，于是跳过清单闸反而合法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 构建：CONFIG 缺席掩码与表指纹互相指认。
    Build,
    /// 表：applet 表收缩与 F11595 联签锚对齐。
    Table,
    /// 身份：请求名与卡号/短名的对应。
    Identity,
    /// 流：答复落 stderr，stdout 必须空。
    Stream,
    /// 文案：逐字同形。
    Text,
    /// 码档：127 与 errno。
    Exit,
    /// 泄漏：答复里多说出了裁剪事实。
    Leak,
    /// 封印：走完全程，谈默认必跑位。
    Seal,
}

impl Stage {
    /// 1 基闸号。
    pub const fn rank(self: &Stage) -> u8 {
        match self {
            Stage::Build => 1u8,
            Stage::Table => 2u8,
            Stage::Identity => 3u8,
            Stage::Stream => 4u8,
            Stage::Text => 5u8,
            Stage::Exit => 6u8,
            Stage::Leak => 7u8,
            Stage::Seal => 8u8,
        }
    }

    /// 闸数。
    pub const COUNT: u8 = 8u8;

    /// 下一道闸；封印闸没有下一道。
    pub fn next(self: &Stage) -> Option<Stage> {
        match self.rank() {
            1u8 => Some(Stage::Table),
            2u8 => Some(Stage::Identity),
            3u8 => Some(Stage::Stream),
            4u8 => Some(Stage::Text),
            5u8 => Some(Stage::Exit),
            6u8 => Some(Stage::Leak),
            7u8 => Some(Stage::Seal),
            _ => None,
        }
    }

    /// 按闸号回查；越界返回 None（不许 panic，也不许悄悄落到 Seal）。
    pub fn from_rank(r: u8) -> Option<Stage> {
        match r {
            1u8 => Some(Stage::Build),
            2u8 => Some(Stage::Table),
            3u8 => Some(Stage::Identity),
            4u8 => Some(Stage::Stream),
            5u8 => Some(Stage::Text),
            6u8 => Some(Stage::Exit),
            7u8 => Some(Stage::Leak),
            8u8 => Some(Stage::Seal),
            _ => None,
        }
    }

    /// 闸名（ktest 短体）。
    pub fn slug(self: &Stage) -> &'static str {
        match self {
            Stage::Build => "build",
            Stage::Table => "table",
            Stage::Identity => "identity",
            Stage::Stream => "stream",
            Stage::Text => "text",
            Stage::Exit => "exit",
            Stage::Leak => "leak",
            Stage::Seal => "seal",
        }
    }

    /// 是否已经走过执行面（码档闸及其后）——跳过/阻塞类结论只在执行前成立。
    pub fn past_exit(self: &Stage) -> bool {
        self.rank() >= Stage::Exit.rank()
    }

    /// 该闸的签名码。
    ///
    /// 八道闸各取一码，互不重复；闸内允许出现更专的码——泄漏闸的洗绿档走
    /// `TR_EFLAKE`（它说的是"这条绿是重试出来的"，不是"答复多话了"），
    /// 对判六维走 `TR_ESTATE`/`TR_ESTAGE`/`TR_EDEFAULT`，槽位越界走 `TR_ESLOT`。
    pub fn fault_code(self: &Stage) -> u32 {
        match self {
            Stage::Build => TR_ECOUNT,
            Stage::Table => TR_ECYCLE,
            Stage::Identity => TR_EUNIQ,
            Stage::Stream => TR_ESKIP,
            Stage::Text => TR_EVALUE,
            Stage::Exit => TR_ECODE,
            Stage::Leak => TR_ENOTE,
            Stage::Seal => TR_EBLOCK,
        }
    }
}

/// 文案形态档。同一份"没找到"在三种构建里说法不同，必须先把形态定下来再谈逐字。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 未定形（零值，任何一条闸都不该停在未定形上）。
    Unknown,
    /// 现代主锚 `busybox: 'ls': applet not found`。
    Modern,
    /// 旧版无名字档 `busybox: applet not found`。
    Legacy,
    /// noapplet 桩档 `busybox: applet 'ls' not found`。
    Stub,
    /// 禁用泄漏档（判死用）。
    DisabledHint,
    /// 候选泄漏档（判死用）。
    CandidateHint,
    /// CONFIG 提示泄漏档（判死用）。
    ConfigHint,
    /// 空答复（判死用）。
    Blank,
}

impl Form {
    /// 1 基形态号。
    pub const fn rank(self: &Form) -> u8 {
        match self {
            Form::Unknown => 0u8,
            Form::Modern => 1u8,
            Form::Legacy => 2u8,
            Form::Stub => 3u8,
            Form::DisabledHint => 4u8,
            Form::CandidateHint => 5u8,
            Form::ConfigHint => 6u8,
            Form::Blank => 7u8,
        }
    }

    /// 形态数（不含 Unknown）。
    pub const COUNT: u8 = 7u8;

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Form> {
        match r {
            1u8 => Some(Form::Modern),
            2u8 => Some(Form::Legacy),
            3u8 => Some(Form::Stub),
            4u8 => Some(Form::DisabledHint),
            5u8 => Some(Form::CandidateHint),
            6u8 => Some(Form::ConfigHint),
            7u8 => Some(Form::Blank),
            _ => None,
        }
    }

    /// 形态短名。
    pub fn slug(self: &Form) -> &'static str {
        match self {
            Form::Unknown => "unknown",
            Form::Modern => "modern",
            Form::Legacy => "legacy",
            Form::Stub => "noapplet-stub",
            Form::DisabledHint => "disabled-hint",
            Form::CandidateHint => "didyou-mean",
            Form::ConfigHint => "config-hint",
            Form::Blank => "blank",
        }
    }

    /// 该形态是否"把裁剪这件事说了出去"——这是本卡独有的泄漏面。
    pub fn is_leak(self: &Form) -> bool {
        match self {
            Form::DisabledHint | Form::CandidateHint | Form::ConfigHint => true,
            _ => false,
        }
    }

    /// 该形态是否是合法的"没找到"答复档。
    pub fn is_valid_notfound(self: &Form) -> bool {
        match self {
            Form::Modern | Form::Legacy | Form::Stub => true,
            _ => false,
        }
    }

    /// 该形态答复里带不带名字。旧版档不带——它连名字都没印，因此对"被裁名"
    /// 与"未知名"天然同形，但也天然丢了信息量，本卡把它单独记一档。
    pub fn carries_name(self: &Form) -> bool {
        match self {
            Form::Modern | Form::Stub | Form::DisabledHint | Form::CandidateHint
            | Form::ConfigHint => true,
            _ => false,
        }
    }

    /// 按形态把名字接成文案。合法性由调用方的闸保证，这里只负责成形。
    pub fn render(self: &Form, name: &str, cand: &str) -> String {
        match self {
            Form::Modern => text_modern(name),
            Form::Legacy => text_legacy(),
            Form::Stub => text_stub(name),
            Form::DisabledHint => text_disabled(name),
            Form::CandidateHint => text_didyou(name, cand),
            Form::ConfigHint => text_config_hint(name),
            _ => text_empty(),
        }
    }

    /// 该形态在指纹面上占的字重——泄漏档权重最高，因为它在文案面上"最像对的"。
    pub fn weight(self: &Form) -> u32 {
        match self {
            Form::Modern => 3u32,
            Form::Legacy => 2u32,
            Form::Stub => 2u32,
            Form::DisabledHint => 5u32,
            Form::CandidateHint => 5u32,
            Form::ConfigHint => 5u32,
            Form::Blank => 4u32,
            Form::Unknown => 0u32,
        }
    }
}

/// 流归属档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stream {
    /// 未定。
    Unknown,
    /// 只写 stderr——标准档。
    Stderr,
    /// 只写 stdout——裁剪件里 `bb_error_msg` 被换成 `printf` 的实际故障形态。
    Stdout,
    /// 两条都写（重复答复，长度对不上）。
    Both,
    /// 劈开写：前半在 stdout、后半在 stderr——同一次答复被切成两段。
    Split,
    /// 一个字都没写。
    None,
}

impl Stream {
    /// 1 基流号。
    pub const fn rank(self: &Stream) -> u8 {
        match self {
            Stream::Unknown => 0u8,
            Stream::Stderr => 1u8,
            Stream::Stdout => 2u8,
            Stream::Both => 3u8,
            Stream::Split => 4u8,
            Stream::None => 5u8,
        }
    }

    /// 流档数。
    pub const COUNT: u8 = 5u8;

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Stream> {
        match r {
            1u8 => Some(Stream::Stderr),
            2u8 => Some(Stream::Stdout),
            3u8 => Some(Stream::Both),
            4u8 => Some(Stream::Split),
            5u8 => Some(Stream::None),
            _ => None,
        }
    }

    /// 流名。
    pub fn slug(self: &Stream) -> &'static str {
        match self {
            Stream::Unknown => "unknown",
            Stream::Stderr => "stderr",
            Stream::Stdout => "stdout",
            Stream::Both => "both",
            Stream::Split => "split",
            Stream::None => "none",
        }
    }

    /// 是否把答复写进了 stdout——本卡只认 stderr。
    pub fn pollutes_stdout(self: &Stream) -> bool {
        match self {
            Stream::Stdout | Stream::Both | Stream::Split => true,
            _ => false,
        }
    }

    /// 是否一个字节都没落到 stderr。
    pub fn starves_stderr(self: &Stream) -> bool {
        match self {
            Stream::Stdout | Stream::None => true,
            _ => false,
        }
    }

    /// 该流档在逐字比对时能读到的字节占比档（0=读不到全文）。
    pub fn readable(self: &Stream) -> bool {
        match self {
            Stream::Stderr => true,
            _ => false,
        }
    }
}

/// 注记档：定局那条红"红在哪件事上"。除 `None` 外每档互不相同，且都必须有名字
/// ——同批 F11660 的教训：只让 rank 0 空命名，其余档共享空串会让注记面失去区分度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 无注记（执行前的零态，唯一允许空名的档）。
    None,
    /// 走完全程且逐字同形。
    Clean,
    /// 名字与文案形态对不上（被裁名却走了旧版无名字档之外的形态）。
    FormMismatch,
    /// 答复被写进 stdout。
    StdoutPolluted,
    /// stderr 上一个字节都没有。
    StderrStarved,
    /// 码档漂移（127 之外的退出码）。
    CodeDrift,
    /// errno 漂移（非零 errno 混进 notfound）。
    ErrnoDrift,
    /// 泄漏：说出"被裁/disabled/CONFIG"这类裁剪事实。
    TrimLeak,
    /// 表指纹与 CONFIG 掩码互相矛盾。
    TableConflict,
    /// 联签锚不一致（F11595 说的是另一次裁剪）。
    CrossSign,
    /// 文案字节数对不上。
    BytesShort,
    /// 候选名列表出现。
    CandidateList,
    /// 空答复。
    BlankReply,
    /// 抖动后恢复。
    FlakeRecovered,
    /// 前置缺席。
    AbsentCarrier,
    /// 前置卡未达成。
    DepBlocked,
    /// 身份面违约：卡号越界、短名进不了 TAP、请求名不可查或撞车。
    ///
    /// 四个成因不各占一档，是因为它们同属"这条现场是谁"这一维，分开只会让
    /// 注记表膨胀到读不出重点；具体坏在哪一维由 `Outcome::value` 的位图说明。
    BadIdentity,
    /// 清单声明的断言数没走满。
    AssertShort,
}

impl Note {
    /// 1 基注记号（`None` 占 0）。
    pub const fn rank(self: &Note) -> u8 {
        match self {
            Note::None => 0u8,
            Note::Clean => 1u8,
            Note::FormMismatch => 2u8,
            Note::StdoutPolluted => 3u8,
            Note::StderrStarved => 4u8,
            Note::CodeDrift => 5u8,
            Note::ErrnoDrift => 6u8,
            Note::TrimLeak => 7u8,
            Note::TableConflict => 8u8,
            Note::CrossSign => 9u8,
            Note::BytesShort => 10u8,
            Note::CandidateList => 11u8,
            Note::BlankReply => 12u8,
            Note::FlakeRecovered => 13u8,
            Note::AbsentCarrier => 14u8,
            Note::DepBlocked => 15u8,
            Note::BadIdentity => 16u8,
            Note::AssertShort => 17u8,
        }
    }

    /// 注记档数（不含 None）。
    pub const COUNT: u8 = 17u8;

    /// 按号回查；0 号回查 None。
    pub fn from_rank(r: u8) -> Option<Note> {
        match r {
            0u8 => Some(Note::None),
            1u8 => Some(Note::Clean),
            2u8 => Some(Note::FormMismatch),
            3u8 => Some(Note::StdoutPolluted),
            4u8 => Some(Note::StderrStarved),
            5u8 => Some(Note::CodeDrift),
            6u8 => Some(Note::ErrnoDrift),
            7u8 => Some(Note::TrimLeak),
            8u8 => Some(Note::TableConflict),
            9u8 => Some(Note::CrossSign),
            10u8 => Some(Note::BytesShort),
            11u8 => Some(Note::CandidateList),
            12u8 => Some(Note::BlankReply),
            13u8 => Some(Note::FlakeRecovered),
            14u8 => Some(Note::AbsentCarrier),
            15u8 => Some(Note::DepBlocked),
            16u8 => Some(Note::BadIdentity),
            17u8 => Some(Note::AssertShort),
            _ => None,
        }
    }

    /// 注记短名。
    pub fn slug(self: &Note) -> &'static str {
        match self {
            Note::None => "",
            Note::Clean => "clean",
            Note::FormMismatch => "form",
            Note::StdoutPolluted => "stdout",
            Note::StderrStarved => "no-stderr",
            Note::CodeDrift => "exit-drift",
            Note::ErrnoDrift => "errno-drift",
            Note::TrimLeak => "leak",
            Note::TableConflict => "table",
            Note::CrossSign => "cross-sign",
            Note::BytesShort => "bytes",
            Note::CandidateList => "didyou",
            Note::BlankReply => "blank",
            Note::FlakeRecovered => "flake",
            Note::AbsentCarrier => "carrier",
            Note::DepBlocked => "blocked",
            Note::BadIdentity => "identity",
            Note::AssertShort => "asserts",
        }
    }

    /// 是否属于"这条红"的注记。None 与 Clean 之外全算红注记，但 None 只代表
    /// 还没走到执行面，不代表绿。
    pub fn is_fact(self: &Note) -> bool {
        match self {
            Note::None | Note::Clean => false,
            _ => true,
        }
    }

    /// 该注记会不会吃掉一个 TAP 位（skip/todo 指令不改变 ok 计数，红会变）。
    pub fn eats_bit(self: &Note) -> bool {
        match self {
            Note::AbsentCarrier => false,
            _ => true,
        }
    }
}

/// 缺席成因档：一条没跑成的判据到底缺什么。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Skip {
    /// 没缺席。
    None,
    /// 被裁 applet 的 CONFIG 位不在——本卡的主缺席档。
    NoAppletCfg,
    /// ash 不在，shell 侧对照做不了。
    NoAsh,
    /// 静态构建档缺失（`--list` 形态不同）。
    NoStatic,
    /// FEATURE_NO_APPLET 桩不在，桩档无法验。
    NoStub,
    /// 双机构建位缺失。
    NoTwoMachine,
    /// 裁剪矩阵（三档重建）缺失。
    NoTrimMatrix,
    /// 载体本身不在——只能由载体自述，不许从掩码反推。
    NoCarrier,
}

impl Skip {
    /// 1 成因号（None 占 0）。
    pub const fn rank(self: &Skip) -> u8 {
        match self {
            Skip::None => 0u8,
            Skip::NoAppletCfg => 1u8,
            Skip::NoAsh => 2u8,
            Skip::NoStatic => 3u8,
            Skip::NoStub => 4u8,
            Skip::NoTwoMachine => 5u8,
            Skip::NoTrimMatrix => 6u8,
            Skip::NoCarrier => 7u8,
        }
    }

    /// 成因数（不含 None）。
    pub const COUNT: u8 = 7u8;

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Skip> {
        match r {
            0u8 => Some(Skip::None),
            1u8 => Some(Skip::NoAppletCfg),
            2u8 => Some(Skip::NoAsh),
            3u8 => Some(Skip::NoStatic),
            4u8 => Some(Skip::NoStub),
            5u8 => Some(Skip::NoTwoMachine),
            6u8 => Some(Skip::NoTrimMatrix),
            7u8 => Some(Skip::NoCarrier),
            _ => None,
        }
    }

    /// 成因短名。
    pub fn slug(self: &Skip) -> &'static str {
        match self {
            Skip::None => "",
            Skip::NoAppletCfg => "no-config",
            Skip::NoAsh => "no-ash",
            Skip::NoStatic => "no-static",
            Skip::NoStub => "no-stub",
            Skip::NoTwoMachine => "no-two-machine",
            Skip::NoTrimMatrix => "no-trim-matrix",
            Skip::NoCarrier => "no-carrier",
        }
    }

    /// 是否算"没跑成"。
    pub fn is_skip(self: &Skip) -> bool {
        match self {
            Skip::None => false,
            _ => true,
        }
    }

    /// 该成因对应吃掉的配置位——集成因（矩阵/双机）不吃单一位。
    pub fn consumed_bit(self: &Skip) -> u32 {
        match self {
            Skip::NoAppletCfg => CFG_APPLET,
            Skip::NoAsh => CFG_ASH,
            Skip::NoStatic => CFG_STATIC,
            Skip::NoStub => CFG_NOAPPLET,
            Skip::NoTwoMachine => CFG_TWO_MACHINE,
            Skip::NoTrimMatrix => CFG_TRIM_MATRIX,
            Skip::None | Skip::NoCarrier => CFG_NONE,
        }
    }

    /// 成因 → 该当的注记。
    ///
    /// 全部缺席成因统一落到 `AbsentCarrier`，这不是偷懒：档位判定
    /// （`Outcome::tier`）只认这一档为 Skipped，而 skip 与 blocked 的差别正是
    /// 本卡最要紧的一条——环境里没有这台载体，TAP 该出 `ok N # skip`，而不是
    /// `not ok`。把 NoAsh/NoStatic 归到别的注记档，会让"没装 ash"这种环境事实
    /// 冒充成"这条判据跑挂了"。成因本身的区分留给 `Skip` 维与 `value` 号。
    pub fn note(self: &Skip) -> Note {
        match self {
            Skip::None => Note::None,
            _ => Note::AbsentCarrier,
        }
    }

    /// 是否属于"逐字面上仍然 ok"的 skip（TAP 的 skip 指令挂在 ok 行尾）。
    pub fn keeps_ok_line(self: &Skip) -> bool {
        match self {
            Skip::None => false,
            _ => true,
        }
    }
}

/// 从缺席掩码推成因。
///
/// 方向必须是"置 1 = 缺"；同批 F11660 把掩码方向读反过一次，结果是满配置的绿行
/// 集体被误判成 skip。NoCarrier 只能由载体自述，永远推不出，也不许在这里补。
fn derive_skip(missing: u32) -> Skip {
    if (missing & CFG_TRIM_MATRIX) != 0u32 {
        return Skip::NoTrimMatrix;
    }
    if (missing & CFG_TWO_MACHINE) != 0u32 {
        return Skip::NoTwoMachine;
    }
    if (missing & CFG_APPLET) != 0u32 {
        return Skip::NoAppletCfg;
    }
    if (missing & CFG_NOAPPLET) != 0u32 {
        return Skip::NoStub;
    }
    if (missing & CFG_STATIC) != 0u32 {
        return Skip::NoStatic;
    }
    if (missing & CFG_ASH) != 0u32 {
        return Skip::NoAsh;
    }
    Skip::None
}

/// 载体档：一条判据怎么被跑起来。不同载体的 skip 语义与耗时档完全不同。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Harness {
    /// 直接调用体：`busybox <name>`，本卡主载体。
    DirectCall,
    /// 软链接体：`ln -s busybox ls && ./ls`，argv[0] 参与查表。
    SymlinkCall,
    /// 列表体：`busybox --list` 的静态核对。
    ListProbe,
    /// 双机对照体：同一份请求在两份构建上各跑一次。
    TwoMachine,
    /// 裁剪矩阵体：三档 CONFIG 重建后抽样比对。
    TrimMatrix,
    /// 文本快照体：逐字文案与 F11661 基准档做快照差。
    TextDiff,
    /// 无执行体（占位，永远不能算跑过）。
    Noop,
}

impl Harness {
    /// 1 基载体号。
    pub const fn rank(self: &Harness) -> u8 {
        match self {
            Harness::DirectCall => 1u8,
            Harness::SymlinkCall => 2u8,
            Harness::ListProbe => 3u8,
            Harness::TwoMachine => 4u8,
            Harness::TrimMatrix => 5u8,
            Harness::TextDiff => 6u8,
            Harness::Noop => 7u8,
        }
    }

    /// 载体数。
    pub const COUNT: u8 = 7u8;

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Harness> {
        match r {
            1u8 => Some(Harness::DirectCall),
            2u8 => Some(Harness::SymlinkCall),
            3u8 => Some(Harness::ListProbe),
            4u8 => Some(Harness::TwoMachine),
            5u8 => Some(Harness::TrimMatrix),
            6u8 => Some(Harness::TextDiff),
            7u8 => Some(Harness::Noop),
            _ => None,
        }
    }

    /// 载体短名。
    pub fn slug(self: &Harness) -> &'static str {
        match self {
            Harness::DirectCall => "direct",
            Harness::SymlinkCall => "symlink",
            Harness::ListProbe => "list",
            Harness::TwoMachine => "two-machine",
            Harness::TrimMatrix => "trim-matrix",
            Harness::TextDiff => "text-diff",
            Harness::Noop => "noop",
        }
    }

    /// 该载体的正常耗时档（毫秒）——超时的判据按档位而不是按常量卡。
    pub fn ms_band(self: &Harness) -> u32 {
        match self {
            Harness::DirectCall => 15u32,
            Harness::SymlinkCall => 25u32,
            Harness::ListProbe => 40u32,
            Harness::TwoMachine => 120u32,
            Harness::TrimMatrix => 900u32,
            Harness::TextDiff => 60u32,
            Harness::Noop => 0u32,
        }
    }

    /// 该载体是否需要第二台机器/第二份构建。
    pub fn needs_second(self: &Harness) -> bool {
        match self {
            Harness::TwoMachine | Harness::TrimMatrix => true,
            _ => false,
        }
    }

    /// 该载体能否独立产出文案面（Noop 不能）。
    pub fn yields_text(self: &Harness) -> bool {
        match self {
            Harness::Noop => false,
            _ => true,
        }
    }

    /// 该载体在默认套件里的缺省归属。
    ///
    /// 排除的是三类。裁剪矩阵要把 317 项全枚举一遍，耗时近秒，默认套件不带它；
    /// Noop 根本不产出文案，带进默认集就是一条永远空转的 ok；双机档需要第二份
    /// 构建——把"换台机器才能绿"的行算进必跑集，等于给默认套件的绿打了折扣，
    /// 而那正是本批 ktest 唯一对外承诺的数。
    pub fn default_member(self: &Harness) -> bool {
        match self {
            Harness::TrimMatrix | Harness::TwoMachine | Harness::Noop => false,
            _ => true,
        }
    }
}

/// 档位：聚合面的四档。与 TAP 面的四态一一对应，但按"这条判据的实况"分档，
/// 不按"申报"分档——申报错了的行落进 Blocked，而不是按它自称的绿落进 Green。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 绿：八闸走满且六维对齐。
    Green,
    /// 抖：绿过，但重试过（必须显式记档）。
    Flaked,
    /// 跳：载体或配置缺席，根本没跑成。
    Skipped,
    /// 阻：跑成了但红。
    Blocked,
}

impl Tier {
    /// 1 基档号。
    pub const fn rank(self: &Tier) -> u8 {
        match self {
            Tier::Green => 1u8,
            Tier::Flaked => 2u8,
            Tier::Skipped => 3u8,
            Tier::Blocked => 4u8,
        }
    }

    /// 按号回查。
    pub fn from_rank(r: u8) -> Option<Tier> {
        match r {
            1u8 => Some(Tier::Green),
            2u8 => Some(Tier::Flaked),
            3u8 => Some(Tier::Skipped),
            4u8 => Some(Tier::Blocked),
            _ => None,
        }
    }

    /// 档名（横计矩阵的行首）。
    pub fn slug(self: &Tier) -> &'static str {
        match self {
            Tier::Green => "green",
            Tier::Flaked => "flaked",
            Tier::Skipped => "skipped",
            Tier::Blocked => "blocked",
        }
    }

    /// 档权：横计时"这一档的一条值多少"——阻档最重，因为它直接对应线上故障。
    pub fn weight(self: &Tier) -> u32 {
        match self {
            Tier::Blocked => 4u32,
            Tier::Skipped => 3u32,
            Tier::Flaked => 2u32,
            Tier::Green => 1u32,
        }
    }

    /// 是否算进"绿集"——通过率分子里那一档。
    ///
    /// 抖动档进绿集是**有意**的：线上只问"这条判据这次给了正确答案没有"，重试后
    /// 给了就算；把它算成红会让人干脆把重试次数调大来掩盖。代价是抖动必须单独记档
    /// （`Note::FlakeRecovered`），否则就是洗绿——所以 `Matrix::has_lie()` 只查绿档。
    pub fn in_green(self: &Tier) -> bool {
        match self {
            Tier::Green => true,
            Tier::Flaked => true,
            Tier::Skipped => false,
            Tier::Blocked => false,
        }
    }

    /// 是否属于"根本没跑成"那一类（跳/阻两档都不该计入绿集，但成因完全不同：
    /// 跳是环境缺位、阻是产品有错）。
    pub fn is_halted(self: &Tier) -> bool {
        match self {
            Tier::Skipped => true,
            Tier::Blocked => true,
            _ => false,
        }
    }
}

/// 一条请求在某份构建上的完整现场。
///
/// 三个"表"字段各说各的事，是这张表最容易被写成一句话糊过去的地方：
/// `full_items` 是全量构建的 applet 项数，`table_items` 是本构建的项数，
/// `anchor_items` 是 F11595 在同一裁剪档上**申报**的收缩后项数。同文案这件事
/// 只由 `form`/`stream`/`exit_code` 决定，联签这件事只由三个 anchor_* 决定；
/// 把它们混成"表对了就绿"，就会得出"F11595 绿所以裁剪件绿"的假推理。
#[derive(Clone, Copy)]
pub struct Shape {
    /// 卡号。
    pub card: u16,
    /// ktest 短名（允许连字符——同批 F11660 因拒连字符导致 19 行集体判死）。
    pub tag: &'static str,
    /// 被叫到的 applet 名。
    pub name: &'static str,
    /// 候选名（didyou 泄漏档才有意义，其余档留空串）。
    pub cand: &'static str,
    /// 文案形态档。
    pub form: Form,
    /// 载体。
    pub harness: Harness,
    /// 流归属。
    pub stream: Stream,
    /// CONFIG 缺席掩码（置 1 = 缺）。
    pub cfg_absent: u32,
    /// 全量构建的 applet 项数。
    pub full_items: u16,
    /// 本构建的 applet 项数。
    pub table_items: u16,
    /// F11595 申报的收缩后项数。
    pub anchor_items: u16,
    /// 本构建 applet 表指纹。
    pub table_hash: u32,
    /// F11595 联签锚指纹。
    pub anchor_hash: u32,
    /// 退出码。
    pub exit_code: u16,
    /// errno。
    pub errno_bit: u16,
    /// 前置卡号（0 表示无前置）。
    pub dep: u16,
    /// 前置是否已达成。
    pub dep_done: bool,
    /// 本条清单声明的断言数。
    pub asserts: u16,
    /// 实测通过断言数。
    pub pass_asserts: u16,
    /// 是否属于默认必跑套件。
    pub default_suite: bool,
    /// 跑过没有。
    pub ran: bool,
    /// 首次即中。
    pub first_ok: bool,
    /// 尝试次数。
    pub attempts: u8,
    /// 洗绿：重试过却申报首次即中。
    pub washed: bool,
    /// 身份撞车（卡号/短名/请求名重复）。
    pub dup: bool,
    /// 耗时毫秒。
    pub ms: u32,
    /// 日志字节数。
    pub log_bytes: u32,
    /// 该名字在全量构建里是否合法。
    pub full_known: bool,
    /// 该名字在本构建里是否被裁掉。
    pub trimmed: bool,
    /// 该名字在本构建表里是否查得到。
    pub in_table: bool,
}

impl Shape {
    /// 零值现场：所有事实取"最不乐观"的那一档。
    pub const fn zero() -> Shape {
        Shape {
            card: 0u16,
            tag: "",
            name: "",
            cand: "",
            form: Form::Unknown,
            harness: Harness::Noop,
            stream: Stream::Unknown,
            cfg_absent: CFG_ALL,
            full_items: 0u16,
            table_items: 0u16,
            anchor_items: 0u16,
            table_hash: 0u32,
            anchor_hash: 0u32,
            exit_code: EX_SIG_BASE + SIG_SEGV,
            errno_bit: ENONE,
            dep: 0u16,
            dep_done: false,
            asserts: 0u16,
            pass_asserts: 0u16,
            default_suite: false,
            ran: false,
            first_ok: false,
            attempts: 0u8,
            washed: false,
            dup: false,
            ms: 0u32,
            log_bytes: 0u32,
            full_known: false,
            trimmed: false,
            in_table: true,
        }
    }

    /// 卡号是否落在 B24 批号段。
    pub const fn in_bench(self: &Shape) -> bool {
        self.card >= CARD_MIN && self.card <= CARD_MAX
    }

    /// 短名是否可进 TAP 行：不许有空格、`#`、换行、回车、制表，也不许空。
    ///
    /// 连字符是 kselftest 短名的常规写法，必须放过。
    pub fn tag_ok(self: &Shape) -> bool {
        if self.tag.is_empty() {
            return false;
        }
        for b in self.tag.as_bytes() {
            match *b {
                b' ' | b'#' | b'\n' | b'\r' | b'\t' => return false,
                _ => {}
            }
        }
        true
    }

    /// 请求名是否可查：非空，且不含路径分隔符——带 `/` 的是 `argv[0]` 现场，
    /// 归 F11662 管，本卡不重复验。
    pub fn name_ok(self: &Shape) -> bool {
        if self.name.is_empty() {
            return false;
        }
        for b in self.name.as_bytes() {
            if *b == b'/' || *b == b' ' || *b == b'\n' {
                return false;
            }
        }
        true
    }

    /// 闸一事实：构建面自洽。
    ///
    /// 三处互相指认：`trimmed` 必须与 `CFG_APPLET` 缺席位一致；表不能空；
    /// 载体与形态档必须在册；`in_table` 必须与 `trimmed` 相反（裁掉了就查不到）。
    fn build_ok(self: &Shape) -> bool {
        if self.harness.rank() == 0u8 || (self.harness.rank() as usize) > Harness::COUNT as usize {
            return false;
        }
        if self.form.rank() > Form::COUNT {
            return false;
        }
        if self.table_items == 0u16 || self.full_items == 0u16 {
            return false;
        }
        if self.trimmed != ((self.cfg_absent & CFG_APPLET) != 0u32) {
            return false;
        }
        // 在册的充要条件：没裁它，并且它本来就是一张合法表里的名字。
        // 对照族（从未存在的名字）也走这一条——没裁 + 本来不在册 ⇒ 查不到。
        // 写成 `trimmed == in_table` 会误杀对照族：那是 F11661 的现场，不是本卡
        // 能验的现场，本卡要的恰恰是两族逐字同形。
        if self.in_table != (!self.trimmed && self.full_known) {
            return false;
        }
        true
    }

    /// 闸二事实：表收缩与联签锚对齐。
    ///
    /// 裁掉一位，本构建表就必须比全量少一项；F11595 申报的项数与指纹必须与
    /// 本构建实测逐项对上——对不上就是两份单子说的是两次不同的裁剪。
    fn table_ok(self: &Shape) -> bool {
        if self.trimmed {
            if self.table_items >= self.full_items {
                return false;
            }
        } else if self.table_items != self.full_items {
            return false;
        }
        if self.anchor_items != self.table_items {
            return false;
        }
        if self.anchor_hash != self.table_hash {
            return false;
        }
        true
    }

    /// 闸三事实：身份面无撞车、名字可查、卡号在本批。
    fn identity_ok(self: &Shape) -> bool {
        self.in_bench() && self.tag_ok() && self.name_ok() && !self.dup
    }

    /// 闸四事实：答复完整抵达比对器（stderr 独占）。
    fn stream_ok(self: &Shape) -> bool {
        self.ran && self.harness.yields_text() && self.stream == Stream::Stderr
    }

    /// 该现场应当产出的文案。
    fn text(self: &Shape) -> String {
        self.form.render(self.name, self.cand)
    }

    /// 闸五事实：逐字同形 + 字节数对得上。
    ///
    /// "同形"的判据不是"文案等于某个固定串"，而是"这条答复等于同一份构建里
    /// 一个从未存在过的名字所得到的答复"——这才是裁剪语义下的不变量。字节数
    /// 由 `log_bytes` 申报，必须与实际成形文案的字节数一致。
    fn text_ok(self: &Shape) -> bool {
        if !self.form.is_valid_notfound() {
            return false;
        }
        let t = self.text();
        if self.log_bytes != t.len() as u32 {
            return false;
        }
        // 旧版档不带名字：被裁名与未知名在旧版档里逐字相同，天然成立；
        // 现代/桩体档带名字，名字位必须真的把被叫到的名字印进去。
        if self.form.carries_name() && !t.as_str().contains(self.name) {
            return false;
        }
        true
    }

    /// 闸六事实：码档与 errno。
    ///
    /// 127 是"没找到"的约定档；errno 必须是 0——notfound 是 bb_error_msg_and_die
    /// 走出来的，不依赖 errno，残留 errno 说明这条答复是从别的失败路径混进来的。
    fn exit_ok(self: &Shape) -> bool {
        self.exit_code == EX_NOTFOUND && self.errno_bit == ENONE
    }

    /// 闸七事实：没说多余的话，也没洗绿。
    ///
    /// 泄漏面按"答复文本里出没出现禁用词"判，而不是按 `leak` 布尔位判：布尔位
    /// 是申报，禁用词是实况——只看申报就给了"文本里印着 disabled 却把泄漏位
    /// 标成 false"的构建一条活路。
    fn leak_ok(self: &Shape) -> bool {
        let t = self.text();
        let s = t.as_str();
        if s.contains(FORBIDDEN_DISABLED) || s.contains(FORBIDDEN_DIDYOU) {
            return false;
        }
        // 现代/旧版/桩体三档合法形态里都不该出现 "CONFIG_"；但合法答复本身
        // 不含它，所以这条只挡反面档。
        if s.contains(FORBIDDEN_CONFIG) {
            return false;
        }
        if self.washed || (self.attempts > 1u8 && self.first_ok) {
            return false;
        }
        true
    }

    /// 该现场是否声明了前置。
    pub const fn has_dep(self: &Shape) -> bool {
        self.dep != 0u16
    }

    /// 默认位应当成立的条件：载体属于默认套件、不需要第二份构建，且不是耗时档。
    ///
    /// `needs_second` 单独列一条是因为双机/矩阵档在 ktest 的默认跑法里根本不会被
    /// 调到——把它们算进默认集，等于给必跑集塞进一条"换台机器才能绿"的行。
    fn default_expected(self: &Shape) -> bool {
        self.harness.default_member() && !self.harness.needs_second() && self.ms <= 200u32
    }

    /// 断言数是否走满。
    pub const fn all_asserts_pass(self: &Shape) -> bool {
        self.pass_asserts == self.asserts && self.asserts > 0u16
    }

    /// 耗时档是否在载体的合理带内（±带容差，超出即视为没跑或挂死）。
    fn ms_ok(self: &Shape) -> bool {
        let band = self.harness.ms_band();
        if band == 0u32 {
            return self.ms == 0u32;
        }
        let lo = band / 2u32;
        let hi = band.saturating_mul(4u32);
        self.ms >= lo && self.ms <= hi
    }

    /// 该现场折进指纹。
    pub fn fold(self: &Shape, h: u64) -> u64 {
        let a = fold_u32(h, self.card as u32);
        let b = fold_str(a, self.tag);
        let c = fold_str(b, self.name);
        let d = fold_u32(c, self.form.rank() as u32);
        let e = fold_u32(d, self.harness.rank() as u32);
        let f = fold_u32(e, self.stream.rank() as u32);
        let g = fold_u32(f, self.cfg_absent);
        let i = fold_u32(g, self.table_hash);
        let j = fold_u32(i, self.anchor_hash);
        let k = fold_u32(j, self.exit_code as u32);
        let l = fold_u32(k, self.errno_bit as u32);
        let m = fold_u32(l, self.log_bytes);
        fold_u32(m, self.ms)
    }
}

/// 八闸走完后的定局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 成不成。
    pub ok: bool,
    /// 定局在那道闸。
    pub stage: Stage,
    /// 该闸的错误码（走满则为 0）。
    pub code: u32,
    /// 数值面：绿时是断言数，红时是该闸可数的现场量。
    pub value: u32,
    /// 注记。
    pub note: Note,
    /// 默认必跑位。
    pub default: bool,
    /// 文案字节数。
    pub len: u32,
}

impl Outcome {
    /// 档位。
    pub fn tier(self: &Outcome) -> Tier {
        if self.ok {
            return match self.note {
                Note::FlakeRecovered => Tier::Flaked,
                _ => Tier::Green,
            };
        }
        match self.note {
            Note::AbsentCarrier => Tier::Skipped,
            _ => Tier::Blocked,
        }
    }

    /// 是否已经给出结论（走过执行面）。
    pub fn concluded(self: &Outcome) -> bool {
        self.stage.rank() >= Stage::Exit.rank() || self.ok
    }

    /// 成败文本。
    pub fn verdict(self: &Outcome) -> &'static str {
        bool_text(self.ok)
    }

    /// 结局指纹。
    pub fn fold(self: &Outcome, h: u64) -> u64 {
        let a = fold_u32(h, match self.ok {
            true => 1u32,
            false => 0u32,
        });
        let b = fold_u32(a, self.stage.rank() as u32);
        let c = fold_u32(b, self.code);
        let d = fold_u32(c, self.value);
        let e = fold_u32(d, self.note.rank() as u32);
        let f = fold_u32(e, match self.default {
            true => 1u32,
            false => 0u32,
        });
        fold_u32(f, self.len)
    }
}

// ---------------------------------------------------------------------------
// 缺陷维：申报与实测的六个可比面
// ---------------------------------------------------------------------------

/// 槽级对判的结论。
///
/// 次序固定为 Estate → Stage → Code → Value → Note → Default，与同批 F11660 保持
/// 同一套对判轴：先把"这条到底绿不绿"问清楚，再问绿/红停在哪道闸，最后才比细节。
/// 反过来比（先比错误码）会让"实测红、申报绿但码抄对了"的行蒙过 Estate 维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 申报与实测逐维一致。
    Ok,
    /// 成败互报。
    Estate,
    /// 停留闸不符。
    Stage,
    /// 错误码不符。
    Code,
    /// 数值面不符。
    Value,
    /// 归因注记不符。
    Note,
    /// 默认必跑位不符。
    Default,
}

impl Fault {
    /// 该缺陷对应的错误码；无缺陷为 0。
    pub const fn code(self: &Fault) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => TR_ESTATE,
            Fault::Stage => TR_ESTAGE,
            Fault::Code => TR_ECODE,
            Fault::Value => TR_EVALUE,
            Fault::Note => TR_ENOTE,
            Fault::Default => TR_EDEFAULT,
        }
    }

    /// 短名。
    pub const fn slug(self: &Fault) -> &'static str {
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

    /// 是否成败面（优先于其它一切维）。
    pub const fn estate(self: &Fault) -> bool {
        match self {
            Fault::Estate => true,
            _ => false,
        }
    }

    /// 是否属于账目面（数值与默认位——这两维错了不影响"跑没跑过"）。
    pub const fn accounting(self: &Fault) -> bool {
        match self {
            Fault::Value => true,
            Fault::Default => true,
            _ => false,
        }
    }

    /// 是否属于文本面缺陷（本卡的核心不变量是文案逐字同形，所以单独问一句）。
    pub const fn textual(self: &Fault) -> bool {
        match self {
            Fault::Value => true,
            Fault::Note => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 申报：跑之前写在清单里的那一行
// ---------------------------------------------------------------------------

/// 一条判据的申报面。
///
/// 申报与事实是两个独立对象，这是本卡最容易糊过去的地方：`Shape` 说的是"这份
/// 构建里实际发生了什么"，`Claim` 说的是"这条判据自报了什么"。"同文案"这件事
/// 由事实面决定，"我验过同文案"这件事由申报面决定——两者对不上就是缺陷。
#[derive(Clone, Copy)]
pub struct Claim {
    /// 自报成败。
    pub success: bool,
    /// 自报停留闸。
    pub stage: Stage,
    /// 自报错误码。
    pub code: u32,
    /// 自报数值。
    pub value: u32,
    /// 自报归因档。
    pub note: Note,
    /// 自报默认必跑位。
    pub default: bool,
}

impl Claim {
    /// 空申报——数组占位用，不许冒充一次真实申报。
    pub const fn zero() -> Claim {
        Claim {
            success: false,
            stage: Stage::Build,
            code: 0u32,
            value: 0u32,
            note: Note::None,
            default: false,
        }
    }

    /// 由实测结局生成一份诚实申报。
    pub fn from_outcome(o: &Outcome) -> Claim {
        Claim {
            success: o.ok,
            stage: o.stage,
            code: o.code,
            value: o.value,
            note: o.note,
            default: o.default,
        }
    }

    /// 申报成败面。
    pub const fn success(self: &Claim) -> bool {
        self.success
    }

    /// 把申报折进指纹。
    pub fn fold(self: &Claim, h: u64) -> u64 {
        let a = fold_u32(h, self.stage.rank() as u32);
        let b = fold_u32(a, match self.success {
            true => 1u32,
            false => 0u32,
        });
        let c = fold_u32(b, self.code);
        let d = fold_u32(c, self.value);
        let e = fold_u32(d, self.note.rank() as u32);
        fold_u32(e, match self.default {
            true => 1u32,
            false => 0u32,
        })
    }

    /// 故意只坏一个维度——用于生成 6 个死槽的申报。
    ///
    /// 每档都必须改出一个**可观测**的差别：`Stage` 只许走到下一闸（跳闸在事实面
    /// 已经判死，这里再跳会撞同一份结局），`Note` 必须换档而不是清空。
    pub fn break_dim(self: Claim, f: Fault) -> Claim {
        let mut c = self;
        match f {
            Fault::Ok => {}
            Fault::Estate => {
                c.success = !self.success;
            }
            Fault::Stage => {
                c.stage = match self.stage.next() {
                    Some(n) => n,
                    None => Stage::Build,
                };
            }
            Fault::Code => {
                c.code = match self.code {
                    0u32 => TR_ECODE,
                    n => n.saturating_add(1u32),
                };
            }
            Fault::Value => {
                c.value = self.value.saturating_add(3u32);
            }
            Fault::Note => {
                let nxt = (self.note.rank().saturating_add(1u8)) % (Note::COUNT + 1u8);
                c.note = match Note::from_rank(nxt) {
                    Some(n) => n,
                    None => Note::None,
                };
            }
            Fault::Default => {
                c.default = !self.default;
            }
        }
        c
    }
}

/// 一槽 = 一份事实 + 一份申报。
#[derive(Clone, Copy)]
pub struct Case {
    /// 事实面。
    pub shape: Shape,
    /// 申报面。
    pub claim: Claim,
}

impl Case {
    /// 重算事实结局。
    pub fn actual(self: &Case) -> Outcome {
        define(&self.shape)
    }

    /// 本槽的缺陷维。
    pub fn fault(self: &Case) -> Fault {
        judge_against(&self.claim, &self.actual())
    }
}

// ---------------------------------------------------------------------------
// 本批清单：B24 批 19 条判据的原始申报行
// ---------------------------------------------------------------------------

/// 清单里的一行——跑之前就该长成这样。
///
/// `rows_min` 记的是任务板给这张卡的 `- 行数:` 下限。它不进任何判定逻辑，
/// 只作为账本统计格的可核对来源：把 19 行的下限加总，就是本批的规模承诺。
#[derive(Clone, Copy)]
pub struct Spec {
    /// 卡号。
    pub card: u16,
    /// 判据短名（逐字进 TAP，允许连字符）。
    pub tag: &'static str,
    /// 卡标题（任务板原文逐字）。
    pub title: &'static str,
    /// 运行载体。
    pub harness: Harness,
    /// 前置卡号（0 = 本批首条）。
    pub dep: u16,
    /// 需要在场的配置位（注意：`Shape::cfg_absent` 是它的反面）。
    pub need_cfg: u32,
    /// 该判据内部断言条数。
    pub asserts: u16,
    /// 任务板行数下限。
    pub rows_min: u16,
    /// 是否进默认必跑集。
    pub default_suite: bool,
}

/// B24 批 19 条判据清单。顺序即任务板顺序，前置逐字对齐 `- 前置:` 字段。
pub const SPECS: [Spec; BENCH] = [
    Spec {
        card: 11661u16,
        tag: "f11661-bb-notfound",
        title: "busybox notfound 直接调用报错判据",
        harness: Harness::DirectCall,
        dep: 0u16,
        need_cfg: CFG_NONE,
        asserts: 7u16,
        rows_min: 340u16,
        default_suite: true,
    },
    Spec {
        card: 11662u16,
        tag: "f11662-argv0-unreg",
        title: "argv[0]=未注册名降级主行为判据",
        harness: Harness::SymlinkCall,
        dep: 11661u16,
        need_cfg: CFG_NONE,
        asserts: 6u16,
        rows_min: 340u16,
        default_suite: true,
    },
    Spec {
        card: 11663u16,
        tag: "f11663-trim-known-miss",
        title: "裁剪后已知 applet 缺失报错判据",
        harness: Harness::DirectCall,
        dep: 11662u16,
        need_cfg: CFG_APPLET,
        asserts: 8u16,
        rows_min: 340u16,
        default_suite: true,
    },
    Spec {
        card: 11664u16,
        tag: "f11664-exit-127",
        title: "notfound 退出码 127 判据",
        harness: Harness::DirectCall,
        dep: 11663u16,
        need_cfg: CFG_NONE,
        asserts: 5u16,
        rows_min: 340u16,
        default_suite: true,
    },
    Spec {
        card: 11665u16,
        tag: "f11665-sh-cmd-127",
        title: "sh 内 command not found 127 判据",
        harness: Harness::SymlinkCall,
        dep: 11664u16,
        need_cfg: CFG_ASH,
        asserts: 5u16,
        rows_min: 320u16,
        default_suite: true,
    },
    Spec {
        card: 11666u16,
        tag: "f11666-path-fallback",
        title: "PATH 查找失败回退判据",
        harness: Harness::DirectCall,
        dep: 11665u16,
        need_cfg: CFG_NONE,
        asserts: 4u16,
        rows_min: 320u16,
        default_suite: true,
    },
    Spec {
        card: 11667u16,
        tag: "f11667-bb-help",
        title: "busybox --help 退出码 0 与 usage 全文判据",
        harness: Harness::ListProbe,
        dep: 11666u16,
        need_cfg: CFG_USAGE,
        asserts: 6u16,
        rows_min: 320u16,
        default_suite: true,
    },
    Spec {
        card: 11668u16,
        tag: "f11668-applet-help",
        title: "<applet> --help 与 applet usage 判据",
        harness: Harness::DirectCall,
        dep: 11667u16,
        need_cfg: CFG_USAGE,
        asserts: 5u16,
        rows_min: 340u16,
        default_suite: true,
    },
    Spec {
        card: 11669u16,
        tag: "f11669-unknown-opt",
        title: "unknown option applet 层报错判据",
        harness: Harness::DirectCall,
        dep: 11668u16,
        need_cfg: CFG_LONG_OPTS,
        asserts: 4u16,
        rows_min: 340u16,
        default_suite: true,
    },
    Spec {
        card: 11670u16,
        tag: "f11670-bad-value",
        title: "无效参数值（非法数值/枚举）判据",
        harness: Harness::DirectCall,
        dep: 11669u16,
        need_cfg: CFG_NONE,
        asserts: 4u16,
        rows_min: 320u16,
        default_suite: true,
    },
    Spec {
        card: 11671u16,
        tag: "f11671-name-case-alias",
        title: "applet 名大小写与别名匹配判据",
        harness: Harness::ListProbe,
        dep: 11670u16,
        need_cfg: CFG_APPLET,
        asserts: 6u16,
        rows_min: 320u16,
        default_suite: true,
    },
    Spec {
        card: 11672u16,
        tag: "f11672-install-degrade",
        title: "busybox 対未知名 --install 降级判据",
        harness: Harness::SymlinkCall,
        dep: 11671u16,
        need_cfg: CFG_NONE,
        asserts: 5u16,
        rows_min: 320u16,
        default_suite: true,
    },
    Spec {
        card: 11673u16,
        tag: "f11673-table-integrity",
        title: "applet 表完整性自检判据（build 清单 vs --list）",
        harness: Harness::TrimMatrix,
        dep: 11672u16,
        need_cfg: CFG_TRIM_MATRIX,
        asserts: 7u16,
        rows_min: 340u16,
        default_suite: false,
    },
    Spec {
        card: 11674u16,
        tag: "f11674-text-verbatim",
        title: "未知 applet 文案逐字判据（applet not found）",
        harness: Harness::TextDiff,
        dep: 11673u16,
        need_cfg: CFG_NONE,
        asserts: 6u16,
        rows_min: 340u16,
        default_suite: true,
    },
    Spec {
        card: 11675u16,
        tag: "f11675-enoent-chain",
        title: "notfound 与 ENOENT exec 链路对照判据",
        harness: Harness::DirectCall,
        dep: 11674u16,
        need_cfg: CFG_NONE,
        asserts: 5u16,
        rows_min: 320u16,
        default_suite: true,
    },
    Spec {
        card: 11676u16,
        tag: "f11676-trim-matrix-sample",
        title: "裁剪矩阵重建抽样（三档 CONFIG）判据",
        harness: Harness::TrimMatrix,
        dep: 11675u16,
        need_cfg: CFG_TRIM_MATRIX | CFG_APPLET,
        asserts: 6u16,
        rows_min: 340u16,
        default_suite: false,
    },
    Spec {
        card: 11677u16,
        tag: "f11677-help-two-machine",
        title: "帮助文本双机快照验证",
        harness: Harness::TwoMachine,
        dep: 11676u16,
        need_cfg: CFG_TWO_MACHINE | CFG_VERBOSE_USAGE,
        asserts: 4u16,
        rows_min: 320u16,
        default_suite: false,
    },
    Spec {
        card: 11678u16,
        tag: "f11678-stream-split",
        title: "notfound stderr/stdout 分离判据",
        harness: Harness::TextDiff,
        dep: 11677u16,
        need_cfg: CFG_NONE,
        asserts: 5u16,
        rows_min: 320u16,
        default_suite: true,
    },
    Spec {
        card: 11679u16,
        tag: "f11679-exit-matrix",
        title: "未知 applet 场景 × 退出码矩阵",
        harness: Harness::TwoMachine,
        dep: 11678u16,
        need_cfg: CFG_TWO_MACHINE,
        asserts: 7u16,
        rows_min: 340u16,
        default_suite: false,
    },
];

/// 按序取清单行。
pub fn spec_at(i: usize) -> Option<&'static Spec> {
    SPECS.get(i)
}

/// 按卡号取清单行。
pub fn spec_of_card(card: u16) -> Option<&'static Spec> {
    let mut i = 0usize;
    while i < BENCH {
        match spec_at(i) {
            Some(s) => {
                if s.card == card {
                    return Some(s);
                }
            }
            None => {}
        }
        i = i.saturating_add(1usize);
    }
    None
}

/// 卡号在清单里的序号；越界返回 None。
pub fn spec_index_of(card: u16) -> Option<usize> {
    let mut i = 0usize;
    while i < BENCH {
        match spec_at(i) {
            Some(s) => {
                if s.card == card {
                    return Some(i);
                }
            }
            None => {}
        }
        i = i.saturating_add(1usize);
    }
    None
}

/// 本批内部断言总条数（19 行的 asserts 加总）。
pub fn bench_assert_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = spec_at(i) {
            n = n.saturating_add(s.asserts as u32);
        }
        i = i.saturating_add(1usize);
    }
    n
}

/// 本批任务板行数下限总和。
pub fn bench_rows_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = spec_at(i) {
            n = n.saturating_add(s.rows_min as u32);
        }
        i = i.saturating_add(1usize);
    }
    n
}

/// 声明了前置的清单行数（拓扑链长度）。
pub fn bench_dep_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = spec_at(i) {
            if s.dep != 0u16 {
                n = n.saturating_add(1u32);
            }
        }
        i = i.saturating_add(1usize);
    }
    n
}

/// 进默认必跑集的清单行数。
pub fn bench_default_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = spec_at(i) {
            if s.default_suite {
                n = n.saturating_add(1u32);
            }
        }
        i = i.saturating_add(1usize);
    }
    n
}

// ---------------------------------------------------------------------------
// 事实成因：闸只说"过/不过"，成因说"过在哪一步断的"
// ---------------------------------------------------------------------------

impl Shape {
    /// 载体与前提缺席成因。
    ///
    /// 这一档必须走在所有事实闸之前：环境里根本没有这条判据要用的那份构建/那台
    /// 机器时，任何"跑挂了"的判定都是假红。`Skip::NoCarrier` 只由载体自述
    /// （`Noop`）得出，不从掩码反推——同批 F11660 验过：从掩码反推载体在不在，
    /// 会把"没装 ash"和"装了 ash 但这条没跑"混成同一种结论。
    fn carrier_skip(self: &Shape) -> Skip {
        if self.harness == Harness::Noop {
            return Skip::NoCarrier;
        }
        if self.harness == Harness::TrimMatrix && (self.cfg_absent & CFG_TRIM_MATRIX) != 0u32 {
            return Skip::NoTrimMatrix;
        }
        if self.harness == Harness::TwoMachine && (self.cfg_absent & CFG_TWO_MACHINE) != 0u32 {
            return Skip::NoTwoMachine;
        }
        // 前提缺席：申报"被裁"的名字在全量构建里从来就不存在。裁不掉一个
        // 没有的名字，这条判据的前提不成立。
        if self.trimmed && !self.full_known {
            return Skip::NoAppletCfg;
        }
        if self.harness == Harness::SymlinkCall && (self.cfg_absent & CFG_ASH) != 0u32 {
            return Skip::NoAsh;
        }
        if self.harness == Harness::ListProbe && (self.cfg_absent & CFG_STATIC) != 0u32 {
            return Skip::NoStatic;
        }
        if self.form == Form::Stub && (self.cfg_absent & CFG_NOAPPLET) != 0u32 {
            return Skip::NoStub;
        }
        Skip::None
    }

    /// 构建闸成因，判序与 `build_ok` 一致。
    fn build_why(self: &Shape) -> Note {
        if self.harness.rank() == 0u8 || self.harness.rank() > Harness::COUNT {
            return Note::AbsentCarrier;
        }
        if self.form.rank() > Form::COUNT {
            return Note::FormMismatch;
        }
        if self.table_items == 0u16 || self.full_items == 0u16 {
            return Note::TableConflict;
        }
        if self.trimmed != ((self.cfg_absent & CFG_APPLET) != 0u32) {
            return Note::TableConflict;
        }
        if self.in_table != (!self.trimmed && self.full_known) {
            return Note::FormMismatch;
        }
        Note::Clean
    }

    /// 表闸成因：先问收缩方向，再问联签锚。
    fn table_why(self: &Shape) -> Note {
        if self.trimmed {
            if self.table_items >= self.full_items {
                return Note::TableConflict;
            }
        } else if self.table_items != self.full_items {
            return Note::TableConflict;
        }
        if self.anchor_items != self.table_items || self.anchor_hash != self.table_hash {
            return Note::CrossSign;
        }
        Note::Clean
    }

    /// 身份闸的位图：置 1 = 该维违约。
    ///
    /// 0x1 卡号不在本批号段，0x2 短名进不了 TAP 行，0x4 请求名不可查，
    /// 0x8 与别的槽撞车。四维权共用一档注记、用位图分家，是因为它们都是
    /// "这条现场是谁"的同族问题——真出错时人要的是"哪一维"，不是又四个注记名。
    fn identity_bits(self: &Shape) -> u32 {
        let mut b = 0u32;
        if !self.in_bench() {
            b |= 0x1u32;
        }
        if !self.tag_ok() {
            b |= 0x2u32;
        }
        if !self.name_ok() {
            b |= 0x4u32;
        }
        if self.dup {
            b |= 0x8u32;
        }
        b
    }

    /// 流闸成因。
    fn stream_why(self: &Shape) -> Note {
        if self.stream.pollutes_stdout() {
            return Note::StdoutPolluted;
        }
        if self.stream.starves_stderr() {
            return Note::StderrStarved;
        }
        if !self.harness.yields_text() {
            return Note::StderrStarved;
        }
        Note::BlankReply
    }

    /// 文案闸成因。
    ///
    /// 泄漏形态（`DisabledHint`/`CandidateHint`/`ConfigHint`）在这里就被判死——
    /// 它不等于基线，同形闸本来就拦得住它，并且归因写的是"多说了裁剪事实"而不是
    /// "文案字节不对"。所以本卡的泄漏闸不管形态泄漏，管的是洗绿。
    fn text_why(self: &Shape) -> Note {
        if !self.form.is_valid_notfound() {
            if self.form == Form::CandidateHint {
                return Note::CandidateList;
            }
            if self.form.is_leak() {
                return Note::TrimLeak;
            }
            if self.form == Form::Unknown || self.form == Form::Blank {
                return Note::BlankReply;
            }
            return Note::FormMismatch;
        }
        if self.log_bytes != self.text().len() as u32 {
            return Note::BytesShort;
        }
        if self.form.carries_name() && !self.text().as_str().contains(self.name) {
            return Note::FormMismatch;
        }
        Note::Clean
    }

    /// 码档闸成因：退出码优先于 errno——码档是对外承诺，errno 只是残留。
    fn exit_why(self: &Shape) -> Note {
        if self.exit_code != EX_NOTFOUND {
            return Note::CodeDrift;
        }
        Note::ErrnoDrift
    }

    /// 泄漏闸成因：走到这里文案已经合法，剩下的违约只有"这条绿是洗出来的"。
    fn leak_why(self: &Shape) -> Note {
        if self.washed || (self.attempts > 1u8 && self.first_ok) {
            return Note::FlakeRecovered;
        }
        Note::Clean
    }

    /// 泄漏闸的错误码：洗绿用抖动码，其余用注记码。
    fn leak_code(self: &Shape) -> u32 {
        match self.leak_why() {
            Note::FlakeRecovered => TR_EFLAKE,
            _ => TR_ENOTE,
        }
    }

    /// 本条现场的基线答复：同一份构建里，一个从未存在过的名字会得到的那一行。
    ///
    /// 同文案判据的真正形式是"两族答复的**模板**逐字相同"，模板由形态档决定，
    /// 名字只是模板上的一个洞——所以基线用本条自己的名字填，比较才有意义。
    pub fn baseline_text(self: &Shape) -> String {
        Form::Modern.render(self.name, "")
    }
}

/// 结算一条现场：八闸顺序走完，任何一闸不过就在那里定局，后面的闸不再消费事实。
///
/// 纵线不可倒置的理由见文档头。这里补一条只在实现里才看得见的理由：跳过档
/// 一定发生在构建闸，因此 `Tier::Skipped` 的行永远不可能同时是"文案漂移"的行——
/// 把 skip 放在文案闸之后，会让一个根本没跑的槽被记成"答复不对"，那是把环境问题
/// 说成代码问题。
pub fn define(sh: &Shape) -> Outcome {
    // ---- 闸一：构建面 ----
    let cs = sh.carrier_skip();
    if cs.is_skip() {
        return Outcome {
            ok: false,
            stage: Stage::Build,
            code: TR_ESKIP,
            value: cs.rank() as u32,
            note: cs.note(),
            default: false,
            len: sh.log_bytes,
        };
    }
    if !sh.ran {
        let why = match derive_skip(sh.cfg_absent).is_skip() {
            true => derive_skip(sh.cfg_absent),
            false => Skip::NoCarrier,
        };
        return Outcome {
            ok: false,
            stage: Stage::Build,
            code: TR_ESKIP,
            value: why.rank() as u32,
            note: why.note(),
            default: false,
            len: sh.log_bytes,
        };
    }
    if !sh.ms_ok() {
        return Outcome {
            ok: false,
            stage: Stage::Build,
            code: TR_ESTATE,
            value: sh.ms,
            note: Note::StderrStarved,
            default: false,
            len: sh.log_bytes,
        };
    }
    if !sh.build_ok() {
        return Outcome {
            ok: false,
            stage: Stage::Build,
            code: Stage::Build.fault_code(),
            value: sh.cfg_absent,
            note: sh.build_why(),
            default: false,
            len: sh.log_bytes,
        };
    }
    // ---- 闸二：表面（收缩方向 + F11595 联签锚）----
    if !sh.table_ok() {
        return Outcome {
            ok: false,
            stage: Stage::Table,
            code: Stage::Table.fault_code(),
            value: sh.table_items as u32,
            note: sh.table_why(),
            default: false,
            len: sh.log_bytes,
        };
    }
    // ---- 闸三：身份面 ----
    if !sh.identity_ok() {
        return Outcome {
            ok: false,
            stage: Stage::Identity,
            code: Stage::Identity.fault_code(),
            value: sh.identity_bits(),
            note: Note::BadIdentity,
            default: false,
            len: sh.log_bytes,
        };
    }
    // ---- 闸四：抵达面 ----
    if !sh.stream_ok() {
        return Outcome {
            ok: false,
            stage: Stage::Stream,
            code: Stage::Stream.fault_code(),
            value: sh.stream.rank() as u32,
            note: sh.stream_why(),
            default: false,
            len: sh.log_bytes,
        };
    }
    // ---- 闸五：文案面（逐字同形 + 字节数 + 名字洞）----
    if !sh.text_ok() {
        return Outcome {
            ok: false,
            stage: Stage::Text,
            code: Stage::Text.fault_code(),
            value: sh.log_bytes,
            note: sh.text_why(),
            default: false,
            len: sh.log_bytes,
        };
    }
    // ---- 闸六：码档面 ----
    if !sh.exit_ok() {
        return Outcome {
            ok: false,
            stage: Stage::Exit,
            code: Stage::Exit.fault_code(),
            value: sh.exit_code as u32,
            note: sh.exit_why(),
            default: false,
            len: sh.log_bytes,
        };
    }
    // ---- 闸七：泄漏面（这条绿是不是洗出来的）----
    if !sh.leak_ok() {
        return Outcome {
            ok: false,
            stage: Stage::Leak,
            code: sh.leak_code(),
            value: sh.attempts as u32,
            note: sh.leak_why(),
            default: false,
            len: sh.log_bytes,
        };
    }
    // ---- 闸八：封印面（前置达成 + 断言走满）----
    if sh.has_dep() && !sh.dep_done {
        return Outcome {
            ok: false,
            stage: Stage::Seal,
            code: Stage::Seal.fault_code(),
            value: sh.dep as u32,
            note: Note::DepBlocked,
            default: false,
            len: sh.log_bytes,
        };
    }
    if !sh.all_asserts_pass() {
        return Outcome {
            ok: false,
            stage: Stage::Seal,
            code: TR_ECOUNT,
            value: sh.pass_asserts as u32,
            note: Note::AssertShort,
            default: false,
            len: sh.log_bytes,
        };
    }
    let flaked = sh.attempts > 1u8;
    Outcome {
        ok: true,
        stage: Stage::Seal,
        code: 0u32,
        value: sh.asserts as u32,
        note: match flaked {
            true => Note::FlakeRecovered,
            false => Note::Clean,
        },
        default: sh.default_expected(),
        len: sh.log_bytes,
    }
}

/// 申报对实测的逐维对判。次序 Estate → Stage → Code → Value → Note → Default。
///
/// 成败互报排在最前，是因为它是唯一能让"绿集里混进一条红的"成为可能的维：
/// 后面五维错了只是把同一条红说得不够准，这一维错了是把红说成绿。
pub fn judge_against(claim: &Claim, actual: &Outcome) -> Fault {
    if claim.success() != actual.ok {
        return Fault::Estate;
    }
    if claim.stage != actual.stage {
        return Fault::Stage;
    }
    if claim.code != actual.code {
        return Fault::Code;
    }
    if claim.value != actual.value {
        return Fault::Value;
    }
    if claim.note != actual.note {
        return Fault::Note;
    }
    if claim.default != actual.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 槽级对判。
pub fn judge(c: &Case) -> Fault {
    judge_against(&c.claim, &define(&c.shape))
}

/// 六个违约维，次序与 `judge_against` 一致。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Estate,
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
];

/// 六个互不相同的缺陷码——账本的缺陷格逐个消费它们。
pub const REJECT_CODES: [u32; REJECT] = [
    TR_ESTATE,
    TR_ESTAGE,
    TR_ECODE,
    TR_EVALUE,
    TR_ENOTE,
    TR_EDEFAULT,
];

/// 七个统计格 + 一个红档格（第 8 格必须是 929）。
///
/// 依次是：本批判据数 19、声明前置条数 18、注记档数 17、闸数 8、本模块判据项 13、
/// 载体种类 7、本批行数下限总和 6280、红档率 929。
///
/// 929 的来历与同批 F11660 相同：14 格里有 1 格对不上时的千分位通过率
/// `(13*1000+7)/14 = 929`。本卡把它放在第 8 格，是因为这张卡最怕的事就是
/// "被裁名与未知名的答复不再同形"——那正是第 14 槽之外的某槽被蒙进绿集。
pub const VALS: [u32; ACCEPT] = [
    19u32, 18u32, 17u32, 8u32, 13u32, 7u32, 6280u32, RED_RATE,
];

// ---------------------------------------------------------------------------
// 14 槽事实表
// ---------------------------------------------------------------------------

/// 槽位安排：前 8 槽是本批不 found 族的活槽（申报逐字等于实测），后 6 槽每槽
/// 只在六个对判维中的某一维上坏一次。
///
/// 为什么 14 个槽全取自 notfound 族而不铺满 19 张卡：本模块的文案闸只认"没找到"
/// 那一族答复（`is_valid_notfound`），把 `--help`/`unknown option` 那种 usage 族
/// 塞进来，它们会在文案闸被判死——那不是缺陷，是拿错了尺子。usage 族的聚合归
/// F11680（B24 批断言集聚合）管，本卡只管同文案这一条线。清单覆盖度因此是
/// "按卡绑定"的部分覆盖，矩阵用"已覆盖 + 未覆盖 = 全集"这一不变量自检。
pub const SHAPES: [Shape; CORPUS] = [
    // 槽 0：主族活槽——被裁名 `fc` 走现代档，逐字同形，一次通过。
    Shape {
        card: 11663u16,
        tag: "f11663-trim-known-miss",
        name: "fc",
        cand: "",
        form: Form::Modern,
        harness: Harness::DirectCall,
        stream: Stream::Stderr,
        cfg_absent: CFG_APPLET,
        full_items: 317u16,
        table_items: 316u16,
        anchor_items: 316u16,
        table_hash: 0x1163_0a17u32,
        anchor_hash: 0x1163_0a17u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11662u16,
        dep_done: true,
        asserts: 8u16,
        pass_asserts: 8u16,
        default_suite: true,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 12u32,
        log_bytes: 31u32,
        full_known: true,
        trimmed: true,
        in_table: false,
    },
    // 槽 1：对照族活槽——从未存在的名字 `nosuchapplet`，同一档答复。
    //
    // 注意它的 `cfg_absent` 是 `CFG_NONE`：这个名不是被裁的，它本来就不在册。
    // 若在这里把 CFG_APPLET 位置起来，`build_why` 会立刻给出 TableConflict——
    // 掩码说裁过、表说没裁，两件事只能有一个是真的。
    Shape {
        card: 11674u16,
        tag: "f11674-text-verbatim",
        name: "nosuchapplet",
        cand: "",
        form: Form::Modern,
        harness: Harness::TextDiff,
        stream: Stream::Stderr,
        cfg_absent: CFG_NONE,
        full_items: 317u16,
        table_items: 317u16,
        anchor_items: 317u16,
        anchor_hash: 0x1163_0b18u32,
        table_hash: 0x1163_0b18u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11673u16,
        dep_done: true,
        asserts: 6u16,
        pass_asserts: 6u16,
        default_suite: true,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 48u32,
        log_bytes: 41u32,
        full_known: false,
        trimmed: false,
        in_table: false,
    },
    // 槽 2：码档卡自己的现场——第二次尝试才中，申报不许洗成"首次即中"。
    //
    // 这一槽专门给 `Tier::Flaked`：抖动绿与一次绿在成败面上同真，但在语料里必须
    // 分档计数，否则"跑三次总有一次绿"的构建会伪装成 100% 一把过。
    Shape {
        card: 11664u16,
        tag: "f11664-exit-127",
        name: "dmesg",
        cand: "",
        form: Form::Modern,
        harness: Harness::DirectCall,
        stream: Stream::Stderr,
        cfg_absent: CFG_APPLET,
        full_items: 317u16,
        table_items: 316u16,
        anchor_items: 316u16,
        table_hash: 0x1164_0c19u32,
        anchor_hash: 0x1164_0c19u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11663u16,
        dep_done: true,
        asserts: 5u16,
        pass_asserts: 5u16,
        default_suite: true,
        ran: true,
        first_ok: false,
        attempts: 2u8,
        washed: false,
        dup: false,
        ms: 9u32,
        log_bytes: 34u32,
        full_known: true,
        trimmed: true,
        in_table: false,
    },
    // 槽 3：旧版无名字档——被裁名走 legacy 二进制，答复里根本没有名字洞。
    //
    // 这一槽是同文案判据最容易被误伤的地方：legacy 档对"被裁名"和"未知名"给出
    // 逐字相同的 25 字节行，看着像"什么名字都一样"，其实它一样是因为丢了信息。
    // 本卡不判它红（它确实同形），但把它单列一档，指纹里 `form.weight` 只有 2。
    Shape {
        card: 11665u16,
        tag: "f11665-sh-cmd-127",
        name: "nc",
        cand: "",
        form: Form::Legacy,
        harness: Harness::SymlinkCall,
        stream: Stream::Stderr,
        cfg_absent: CFG_APPLET,
        full_items: 302u16,
        table_items: 301u16,
        anchor_items: 301u16,
        table_hash: 0x1165_0d20u32,
        anchor_hash: 0x1165_0d20u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11664u16,
        dep_done: true,
        asserts: 5u16,
        pass_asserts: 5u16,
        default_suite: true,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 21u32,
        log_bytes: 25u32,
        full_known: true,
        trimmed: true,
        in_table: false,
    },
    // 槽 4：`--install` 降级现场的 skip 档——这份构建里没装 ash，符号链接进不去
    // shell，于是这条判据的前提缺席。
    //
    // 它必须是 skip 而不是红：环境缺件与代码答错是两件事，把前者记成后者，
    // 裁剪矩阵跑到没 ash 的机器上就会全线报红。`ran = false` 与 `ms = 0` 一起
    // 说"根本没跑"，而 `trimmed = false` 说"这个名字不是被裁的"。
    Shape {
        card: 11672u16,
        tag: "f11672-install-degrade",
        name: "qqq",
        cand: "",
        form: Form::Modern,
        harness: Harness::SymlinkCall,
        stream: Stream::Stderr,
        cfg_absent: CFG_ASH,
        full_items: 317u16,
        table_items: 317u16,
        anchor_items: 317u16,
        table_hash: 0x1172_0e21u32,
        anchor_hash: 0x1172_0e21u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11671u16,
        dep_done: true,
        asserts: 5u16,
        pass_asserts: 0u16,
        default_suite: true,
        ran: false,
        first_ok: false,
        attempts: 0u8,
        washed: false,
        dup: false,
        ms: 0u32,
        log_bytes: 0u32,
        full_known: false,
        trimmed: false,
        in_table: false,
    },
    // 槽 5：流分离卡自己的现场——答复写到了 stdout 上，stderr 一个字节也没有。
    //
    // 这是 `bb_error_msg` 在裁剪件里被换成 `printf` 的实际故障形态：文案逐字仍然
    // 对，管道却接错了头。它落在抵达闸而不是文案闸，正因为同形与抵达是两件事。
    Shape {
        card: 11678u16,
        tag: "f11678-stream-split",
        name: "httpd",
        cand: "",
        form: Form::Modern,
        harness: Harness::TextDiff,
        stream: Stream::Stdout,
        cfg_absent: CFG_APPLET,
        full_items: 317u16,
        table_items: 316u16,
        anchor_items: 316u16,
        table_hash: 0x1178_0f22u32,
        anchor_hash: 0x1178_0f22u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11677u16,
        dep_done: true,
        asserts: 5u16,
        pass_asserts: 5u16,
        default_suite: true,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 60u32,
        log_bytes: 34u32,
        full_known: true,
        trimmed: true,
        in_table: false,
    },
    // 槽 6：裁剪矩阵重建——耗时落在矩阵档带内，因此它不进默认必跑集。
    Shape {
        card: 11676u16,
        tag: "f11676-trim-matrix-sample",
        name: "ftpd",
        cand: "",
        form: Form::Modern,
        harness: Harness::TrimMatrix,
        stream: Stream::Stderr,
        cfg_absent: CFG_APPLET,
        full_items: 317u16,
        table_items: 316u16,
        anchor_items: 316u16,
        table_hash: 0x1176_1023u32,
        anchor_hash: 0x1176_1023u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11675u16,
        dep_done: true,
        asserts: 6u16,
        pass_asserts: 6u16,
        default_suite: false,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 1100u32,
        log_bytes: 33u32,
        full_known: true,
        trimmed: true,
        in_table: false,
    },
    // 槽 7：桩档现场——`FEATURE_NO_APPLET` 在册，被裁名走 `applet 'x' not found`。
    //
    // 它与槽 0 的差别只在模板：桩档把名字往前挪了一位、去掉了外层单引号。这两条
    // 若被判"同文案"，判的是同一次裁剪下的两种二进制，不是同一种答复。
    //
    // 这一槽故意让退出码漂到 1：文案闸在它身上是过的（桩档本身就是这份构建对
    // 被裁名的合法答复），码档闸不过——正好演示"同形"与"码档"两闸分家。
    Shape {
        card: 11661u16,
        tag: "f11661-bb-notfound",
        name: "udhcpc",
        cand: "",
        form: Form::Stub,
        harness: Harness::DirectCall,
        stream: Stream::Stderr,
        cfg_absent: CFG_APPLET,
        full_items: 317u16,
        table_items: 316u16,
        anchor_items: 316u16,
        table_hash: 0x1161_1124u32,
        anchor_hash: 0x1161_1124u32,
        exit_code: EX_GENERAL,
        errno_bit: ENONE,
        dep: 0u16,
        dep_done: false,
        asserts: 7u16,
        pass_asserts: 7u16,
        default_suite: true,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 14u32,
        log_bytes: 34u32,
        full_known: true,
        trimmed: true,
        in_table: false,
    },
    // 槽 8：PATH 回退现场（活），申报在成败维上互报。
    Shape {
        card: 11666u16,
        tag: "f11666-path-fallback",
        name: "no-such-name",
        cand: "",
        form: Form::Modern,
        harness: Harness::DirectCall,
        stream: Stream::Stderr,
        cfg_absent: CFG_NONE,
        full_items: 317u16,
        table_items: 317u16,
        anchor_items: 317u16,
        table_hash: 0x1166_1225u32,
        anchor_hash: 0x1166_1225u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11665u16,
        dep_done: true,
        asserts: 4u16,
        pass_asserts: 4u16,
        default_suite: true,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 11u32,
        log_bytes: 41u32,
        full_known: false,
        trimmed: false,
        in_table: false,
    },
    // 槽 9：大小写/别名现场（活），申报在停留闸维上前移。
    Shape {
        card: 11671u16,
        tag: "f11671-name-case-alias",
        name: "crond",
        cand: "",
        form: Form::Modern,
        harness: Harness::ListProbe,
        stream: Stream::Stderr,
        cfg_absent: CFG_APPLET,
        full_items: 317u16,
        table_items: 316u16,
        anchor_items: 316u16,
        table_hash: 0x1171_1326u32,
        anchor_hash: 0x1171_1326u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11670u16,
        dep_done: true,
        asserts: 6u16,
        pass_asserts: 6u16,
        default_suite: true,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 45u32,
        log_bytes: 34u32,
        full_known: true,
        trimmed: true,
        in_table: false,
    },
    // 槽 10：ENOENT 链路现场（活），申报在错误码维上漂移。
    Shape {
        card: 11675u16,
        tag: "f11675-enoent-chain",
        name: "zzz",
        cand: "",
        form: Form::Modern,
        harness: Harness::DirectCall,
        stream: Stream::Stderr,
        cfg_absent: CFG_NONE,
        full_items: 317u16,
        table_items: 317u16,
        anchor_items: 317u16,
        table_hash: 0x1175_1427u32,
        anchor_hash: 0x1175_1427u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11674u16,
        dep_done: true,
        asserts: 5u16,
        pass_asserts: 5u16,
        default_suite: true,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 13u32,
        log_bytes: 32u32,
        full_known: false,
        trimmed: false,
        in_table: false,
    },
    // 槽 11：退出码矩阵现场（活），申报在数值维上加三。
    Shape {
        card: 11679u16,
        tag: "f11679-exit-matrix",
        name: "syslogd",
        cand: "",
        form: Form::Modern,
        harness: Harness::TwoMachine,
        stream: Stream::Stderr,
        cfg_absent: CFG_APPLET,
        full_items: 317u16,
        table_items: 316u16,
        anchor_items: 316u16,
        table_hash: 0x1179_1528u32,
        anchor_hash: 0x1179_1528u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11678u16,
        dep_done: true,
        asserts: 7u16,
        pass_asserts: 7u16,
        default_suite: false,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 140u32,
        log_bytes: 36u32,
        full_known: true,
        trimmed: true,
        in_table: false,
    },
    // 槽 12：argv[0] 现场（活），申报在归因维上换档。
    //
    // 请求名不带斜杠，所以它落在本卡的身份面而不是 F11662 的 argv 面——同一份
    // 符号链接构建，`busybox <未知名>` 这条走的是本卡的直接调用形态。
    Shape {
        card: 11662u16,
        tag: "f11662-argv0-unreg",
        name: "nosuchlink",
        cand: "",
        form: Form::Modern,
        harness: Harness::SymlinkCall,
        stream: Stream::Stderr,
        cfg_absent: CFG_NONE,
        full_items: 317u16,
        table_items: 317u16,
        anchor_items: 317u16,
        table_hash: 0x1162_1629u32,
        anchor_hash: 0x1162_1629u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11661u16,
        dep_done: true,
        asserts: 6u16,
        pass_asserts: 6u16,
        default_suite: true,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 22u32,
        log_bytes: 39u32,
        full_known: false,
        trimmed: false,
        in_table: false,
    },
    // 槽 13：表完整性现场（活），申报在默认位上多报一条必跑。
    Shape {
        card: 11673u16,
        tag: "f11673-table-integrity",
        name: "ipcalc",
        cand: "",
        form: Form::Modern,
        harness: Harness::TrimMatrix,
        stream: Stream::Stderr,
        cfg_absent: CFG_APPLET,
        full_items: 317u16,
        table_items: 316u16,
        anchor_items: 316u16,
        table_hash: 0x1173_1730u32,
        anchor_hash: 0x1173_1730u32,
        exit_code: EX_NOTFOUND,
        errno_bit: ENONE,
        dep: 11672u16,
        dep_done: true,
        asserts: 7u16,
        pass_asserts: 7u16,
        default_suite: false,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        washed: false,
        dup: false,
        ms: 950u32,
        log_bytes: 35u32,
        full_known: true,
        trimmed: true,
        in_table: false,
    },
];

/// 14 槽申报表。
///
/// 活槽的申报逐字抄自它那一槽的实测结局；死槽的申报在六维之一上故意错一次。
/// 每张死槽的错法都写在注释里，交叉校验见 `claims_match_their_dims`——申报必须
/// 只坏在它声称的那一维，坏两维的缺陷样本会让通过率统计少算一条红。
pub const CLAIMS: [Claim; CORPUS] = [
    // 槽 0：活——被裁名逐字同形，封印闸，8 条断言走满，进默认集。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 8u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 1：活——对照族。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 6u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 2：活——抖动后恢复的绿：申报必须写 `FlakeRecovered`，写 `Clean` 就是洗绿。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 5u32,
        note: Note::FlakeRecovered,
        default: true,
    },
    // 槽 3：活——旧版无名字档。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 5u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 4：活——ash 缺席，申报是诚实的 skip：红在构建闸、码走 ESKIP。
    Claim {
        success: false,
        stage: Stage::Build,
        code: TR_ESKIP,
        value: 2u32,
        note: Note::AbsentCarrier,
        default: false,
    },
    // 槽 5：活——答复接错头，红在抵达闸；码仍是抵达闸签名码 ESKIP，成因换成
    // `StdoutPolluted`，与真缺席的 `AbsentCarrier` 分档。
    Claim {
        success: false,
        stage: Stage::Stream,
        code: TR_ESKIP,
        value: 2u32,
        note: Note::StdoutPolluted,
        default: false,
    },
    // 槽 6：活——矩阵档，不进默认集。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 6u32,
        note: Note::Clean,
        default: false,
    },
    // 槽 7：活——桩档同形已过，退出码漂到 1，红在码档闸。
    Claim {
        success: false,
        stage: Stage::Exit,
        code: TR_ECODE,
        value: 1u32,
        note: Note::CodeDrift,
        default: false,
    },
    // 槽 8：成败互报——实测绿，申报红。
    Claim {
        success: false,
        stage: Stage::Seal,
        code: 0u32,
        value: 4u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 9：停留闸前移——走完八闸却申报停在构建闸。封闸没有下一道，所以这一维
    // 的坏法只能是"回退"或"前移到某个真实存在的闸"，这里取回退到首闸。
    Claim {
        success: true,
        stage: Stage::Build,
        code: 0u32,
        value: 6u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 10：错误码漂移——绿行却带了一个码。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TR_ECODE,
        value: 5u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 11：数值虚报——断言数加三。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 10u32,
        note: Note::Clean,
        default: false,
    },
    // 槽 12：归因换档——同形且走满，却报成形态不匹配。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 6u32,
        note: Note::FormMismatch,
        default: true,
    },
    // 槽 13：默认位虚报——矩阵档被塞进默认必跑集。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 7u32,
        note: Note::Clean,
        default: true,
    },
];

/// 每槽期望的缺陷维；`REJECT`（=6）表示该槽是活槽，期望 `Fault::Ok`。
pub const WANT_NDX: [usize; CORPUS] = [
    REJECT, REJECT, REJECT, REJECT, REJECT, REJECT, REJECT, REJECT, 0usize, 1usize, 2usize,
    3usize, 4usize, 5usize,
];

/// 槽标签：每槽写它实际占用的那条判据短名。
///
/// 死槽同样挂真判据的短名——缺陷形态说的是"这条真判据被报坏了"，不是又一条判据。
/// 缺陷形态名由 `Fault::slug()` 给，本表只管身份，两张表各管一面才能互查不互包。
pub const WANT_TAGS: [&'static str; CORPUS] = [
    "f11663-trim-known-miss",
    "f11674-text-verbatim",
    "f11664-exit-127",
    "f11665-sh-cmd-127",
    "f11672-install-degrade",
    "f11678-stream-split",
    "f11676-trim-matrix-sample",
    "f11661-bb-notfound",
    "f11666-path-fallback",
    "f11671-name-case-alias",
    "f11675-enoent-chain",
    "f11679-exit-matrix",
    "f11662-argv0-unreg",
    "f11673-table-integrity",
];

/// 取一槽事实。
pub fn shape_at(i: usize) -> Option<Shape> {
    SHAPES.get(i).copied()
}

/// 取一槽申报。
pub fn claim_at(i: usize) -> Option<Claim> {
    CLAIMS.get(i).copied()
}

/// 取一槽实测结局。
pub fn outcome_at(i: usize) -> Option<Outcome> {
    match SHAPES.get(i) {
        Some(s) => Some(define(s)),
        None => None,
    }
}

/// 取一槽用例。
pub fn case_at(i: usize) -> Option<Case> {
    let shape = match SHAPES.get(i) {
        Some(s) => *s,
        None => return None,
    };
    let claim = match CLAIMS.get(i) {
        Some(c) => *c,
        None => return None,
    };
    Some(Case { shape, claim })
}

/// 全部用例。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [Case {
        shape: Shape::zero(),
        claim: Claim::zero(),
    }; CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        match (shape_at(i), claim_at(i)) {
            (Some(s), Some(c)) => {
                if let Some(slot) = out.get_mut(i) {
                    *slot = Case { shape: s, claim: c };
                }
            }
            _ => {}
        }
        i = i.saturating_add(1usize);
    }
    out
}

/// 期望缺陷。
pub fn expected_fault(i: usize) -> Fault {
    let ndx = match WANT_NDX.get(i) {
        Some(n) => *n,
        None => REJECT,
    };
    if ndx >= REJECT {
        return Fault::Ok;
    }
    match REJECT_FAULTS.get(ndx) {
        Some(f) => *f,
        None => Fault::Ok,
    }
}

/// 死槽的"坏一维"申报表——交叉校验 `CLAIMS` 是否恰好坏在它声称的那一维。
pub const CLAIM_BROKEN: [Fault; REJECT] = REJECT_FAULTS;

/// 活槽计数。
pub fn live_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        let ndx = match WANT_NDX.get(i) {
            Some(x) => *x,
            None => REJECT,
        };
        if ndx >= REJECT {
            n = n.saturating_add(1usize);
        }
        i = i.saturating_add(1usize);
    }
    n
}

/// 本批清单里被 14 槽覆盖到的判据数（按卡号绑定）。
pub fn covered_spec_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < BENCH {
        let card = match spec_at(i) {
            Some(s) => s.card,
            None => break,
        };
        let mut j = 0usize;
        let mut hit = false;
        while j < CORPUS {
            if let Some(sh) = shape_at(j) {
                if sh.card == card {
                    hit = true;
                    break;
                }
            }
            j = j.saturating_add(1usize);
        }
        if hit {
            n = n.saturating_add(1usize);
        }
        i = i.saturating_add(1usize);
    }
    n
}

// ---------------------------------------------------------------------------
// 横计面：一档一行，六列可加
// ---------------------------------------------------------------------------

/// 一档的横计行。
///
/// 这一行刻意把"条数"与"断言数"分成两列（`total` 与 `spec_sum`），因为本卡的
/// 判据原文问的是两件事：裁剪件报错同不同文案（按条算），以及这批联签走满没走满
/// （按断言算）。合成一列就会出现"19 条里只跑了 14 条、断言却按 19 条合计"的
/// 假完整——分母自己造假，分子怎么算都对。
#[derive(Clone, Copy)]
pub struct Row {
    /// 本行属于哪一档。
    pub tier: Tier,
    /// 本档条数。
    pub total: u32,
    /// 本档里六维全对齐的条数。
    pub aligned: u32,
    /// 本档的数值面之和（绿时是断言数、红时是该闸现场量）。
    pub value_sum: u32,
    /// 本档里走满八闸且成功的条数。
    pub sealed: u32,
    /// 本档清单声明的断言数之和。
    pub spec_sum: u32,
}

impl Row {
    /// 建一行。条数与对齐数由调用方给，这里不校验二者关系——
    /// 校验由 `lie()` 独立做，坏在哪儿就报哪儿，而不是在建行时悄悄夹平。
    pub fn new(tier: Tier, total: u32, aligned: u32, value_sum: u32, sealed: u32, spec_sum: u32) -> Row {
        Row {
            tier,
            total,
            aligned,
            value_sum,
            sealed,
            spec_sum,
        }
    }

    /// 空行（计数全零，档号仍有效）。
    pub fn empty(tier: Tier) -> Row {
        Row {
            tier,
            total: 0u32,
            aligned: 0u32,
            value_sum: 0u32,
            sealed: 0u32,
            spec_sum: 0u32,
        }
    }

    /// 本行是否说谎：对齐数超过条数，或条数非零却一条没对齐。
    ///
    /// 前者是纯粹的算术不可能，后者意味着"这一档有一条被判了不合格，却又被算进
    /// 这一档的结论里"——聚合单上看不出，只有逐行核才现形。
    pub fn lie(self: &Row) -> bool {
        if self.aligned > self.total {
            return true;
        }
        if self.total > 0u32 && self.aligned == 0u32 {
            return true;
        }
        false
    }

    /// 是否"完全对齐"：有条数且条条对齐。
    pub fn perfect(self: &Row) -> bool {
        self.total > 0u32 && self.aligned == self.total
    }

    /// 加权值：条数 × 档权。阻档一条顶绿档四条，用来把"四档平均看一眼"
    /// 变成"按线上代价看一眼"。
    pub fn weighted(self: &Row) -> u32 {
        self.total.saturating_mul(self.tier.weight())
    }

    /// 走满率（千分）：本档里 sealed/total；空档记 0 而不是除零。
    pub fn seal_rate(self: &Row) -> u32 {
        match self.total {
            0u32 => 0u32,
            n => self.sealed.saturating_mul(RATE_FULL) / n,
        }
    }

    /// 平均现场量：value_sum/total。空档返 0。
    ///
    /// 这个数只在诊断时有意义（"这一档的断言数是不是异常小"），不参与任何闸——
    /// 拿平均值当判据是最容易蒙过总数校验的假绿手法。
    pub fn mean_value(self: &Row) -> u32 {
        match self.total {
            0u32 => 0u32,
            n => self.value_sum / n,
        }
    }

    /// 行折指纹。
    pub fn fold(self: &Row, h: u64) -> u64 {
        let a = fold_u32(h, self.tier.rank() as u32);
        let b = fold_u32(a, self.total);
        let c = fold_u32(b, self.aligned);
        let d = fold_u32(c, self.value_sum);
        let e = fold_u32(d, self.sealed);
        fold_u32(e, self.spec_sum)
    }

    /// 行文本：`green n=10 aligned=10 value=64 sealed=10 spec=56`。
    pub fn render(self: &Row) -> String {
        let mut s = String::from(self.tier.slug());
        s.push_str(" n=");
        s.push_str(dec_text(self.total).as_str());
        s.push_str(" aligned=");
        s.push_str(dec_text(self.aligned).as_str());
        s.push_str(" value=");
        s.push_str(dec_text(self.value_sum).as_str());
        s.push_str(" sealed=");
        s.push_str(dec_text(self.sealed).as_str());
        s.push_str(" spec=");
        s.push_str(dec_text(self.spec_sum).as_str());
        s
    }
}

/// 四档横计矩阵。
///
/// 行的次序固定为 Green/Flaked/Skipped/Blocked（`Tier::rank()` 的 1 基序），
/// 这样 `rows[i]` 的档号恒等于 `i+1`，`bump` 的入口校验才有意义。
#[derive(Clone, Copy)]
pub struct Matrix {
    /// 四行。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 全零矩阵（四行都在，只是没有条数）——起步态，也是 `covered()` 的失败态。
    pub fn zero() -> Matrix {
        Matrix {
            rows: [
                Row::empty(Tier::Green),
                Row::empty(Tier::Flaked),
                Row::empty(Tier::Skipped),
                Row::empty(Tier::Blocked),
            ],
        }
    }

    /// 按档取行。
    pub fn row(self: &Matrix, tier: Tier) -> Option<Row> {
        let ndx = match tier.rank() {
            r if r >= 1u8 => (r as usize).saturating_sub(1usize),
            _ => return None,
        };
        match self.rows.get(ndx) {
            Some(r) => Some(*r),
            None => None,
        }
    }

    /// 归档：把一槽的结局累进它那一档。
    ///
    /// 返回 `Err` 只在"档号对不上行号"时发生——这是矩阵自身坏了，不是数据坏了。
    /// 此时绝不静默丢弃：静默丢一条就等于偷偷改了分母，而分母是这张单子的命根。
    pub fn bump(
        &mut self,
        tier: Tier,
        aligned: bool,
        value: u32,
        sealed: bool,
        spec_asserts: u32,
    ) -> Result<(), u32> {
        let ndx = match tier.rank() {
            r if r >= 1u8 => (r as usize).saturating_sub(1usize),
            _ => return Err(TR_ESTATE),
        };
        let hit = match self.rows.get_mut(ndx) {
            Some(r) => r,
            None => return Err(TR_ESTATE),
        };
        if hit.tier != tier {
            return Err(TR_ECYCLE);
        }
        hit.total = hit.total.saturating_add(1u32);
        if aligned {
            hit.aligned = hit.aligned.saturating_add(1u32);
        }
        hit.value_sum = hit.value_sum.saturating_add(value);
        if sealed {
            hit.sealed = hit.sealed.saturating_add(1u32);
        }
        hit.spec_sum = hit.spec_sum.saturating_add(spec_asserts);
        Ok(())
    }

    /// 条条对齐的档数。四档全齐才算"横计面闭合"。
    pub fn perfect_rows(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.perfect() {
                    n = n.saturating_add(1u32);
                }
            }
            i = i.saturating_add(1usize);
        }
        n
    }

    /// 四档是否都有条数——绿集单独刷满不算覆盖。
    ///
    /// 这一闸是防"把跑不成的槽从语料里删掉"的：删光 skip 与 blocked，
    /// 剩下的 14 条可以全绿，通过率照样 1000。只有强制四档各有至少一条，
    /// 才逼着语料把"载体缺席"和"跑成但红"两种真实形态留在单子上。
    pub fn covered(self: &Matrix) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            match self.rows.get(i) {
                Some(r) => {
                    if r.total == 0u32 {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.saturating_add(1usize);
        }
        true
    }

    /// 任一行说谎即为脏。
    pub fn any_lie(self: &Matrix) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.lie() {
                    return true;
                }
            }
            i = i.saturating_add(1usize);
        }
        false
    }

    /// 绿档藏没藏脏——聚合单最阴的一种：总数看着全绿，但绿档 aligned<total。
    pub fn green_has_lie(self: &Matrix) -> bool {
        match self.row(Tier::Green) {
            Some(r) => r.aligned < r.total,
            None => true,
        }
    }

    /// 总条数。
    pub fn total(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.total);
            }
            i = i.saturating_add(1usize);
        }
        n
    }

    /// 全矩阵对齐条数。
    pub fn aligned(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.aligned);
            }
            i = i.saturating_add(1usize);
        }
        n
    }

    /// 计入绿集（Green + Flaked）的条数——通过率分子里"计入绿集"那部分。
    ///
    /// 注意本卡把它与 `matched` 分开：`matched` 问"申报与实测一致吗"，
    /// `in_green` 问"这条判据实际放行吗"。一条诚实的红申报 aligned 计 1、
    /// in_green 不计，两个数天然不等，混用就会把 2 条 blocked 算成放行。
    pub fn in_green(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.tier.in_green() {
                    n = n.saturating_add(r.total);
                }
            }
            i = i.saturating_add(1usize);
        }
        n
    }

    ///  halted 档（跳 + 阻）条数。
    pub fn halted(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.tier.is_halted() {
                    n = n.saturating_add(r.total);
                }
            }
            i = i.saturating_add(1usize);
        }
        n
    }

    /// 走满全程条数（跨四档求和）。
    pub fn sealed(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.sealed);
            }
            i = i.saturating_add(1usize);
        }
        n
    }

    /// 清单断言总数（四行 spec_sum 相加）。
    ///
    /// 本卡这个数**不等于** `WANT_ASSERTS`：语料只有 14 槽，清单有 19 行，
    /// 槽按卡号绑定清单行，所以这里累的是"被抽到那 14 行的声明数"。
    /// 因此它不进签发闸，只进交叉校验闸 `spec_bound_ok`——拿它当分母就会
    /// 出现"抽 14 条却报 19 条断言"的对不上账。
    pub fn spec_total(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.spec_sum);
            }
            i = i.saturating_add(1usize);
        }
        n
    }

    /// 加权代价和（阻 4 / 跳 3 / 抖 2 / 绿 1）。
    pub fn cost(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.weighted());
            }
            i = i.saturating_add(1usize);
        }
        n
    }

    /// 按档号取行（0 基，越界返 None）。
    pub fn row_at(self: &Matrix, ndx: usize) -> Option<Row> {
        self.rows.get(ndx).copied()
    }

    /// 矩阵折指纹。
    pub fn fold(self: &Matrix, h: u64) -> u64 {
        let mut out = h;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                out = r.fold(out);
            }
            i = i.saturating_add(1usize);
        }
        out
    }

    /// 文本：四行逐档。
    pub fn render(self: &Matrix) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                s.push_str(r.render().as_str());
                s.push_str("\n");
            }
            i = i.saturating_add(1usize);
        }
        s
    }
}

/// 绿档条数——验收文案里的 green=。
///
/// 14 槽里 4 个活槽绿（被裁名/对照名/旧版无名字/矩阵档）+ 6 个死槽事实面绿
/// = 10，这是"申报坏了一维"与"实测绿"同时成立的必然结果，不是凑出来的。
pub const WANT_GREEN: u32 = 10u32;
/// 抖动档条数：活槽 2（重试后恢复的绿）。
pub const WANT_FLAKED: u32 = 1u32;
/// 诚实 skip 档条数：活槽 4（ash 缺席，根本没跑）。
pub const WANT_SKIPPED: u32 = 1u32;
/// 阻塞档条数：活槽 5（接错头）+ 活槽 7（退出码漂）。
pub const WANT_BLOCKED: u32 = 2u32;
/// 全对齐条数（判据原文要求 100%）。
pub const WANT_MATCHED: u32 = CORPUS as u32;
/// 本批断言总数（19 行清单逐项相加）。
pub const WANT_ASSERTS: u32 = 105u32;
/// 走满全程条数——只有绿档与抖动档能 sealed。
pub const WANT_SEALED: u32 = 11u32;
/// 计入绿集条数（green + flaked）。
pub const WANT_INGREEN: u32 = 11u32;
/// 未计入绿集条数（skipped + blocked）。
pub const WANT_HALTED: u32 = 3u32;

/// 聚合报告：一次命令跑完 19 条判据之后要交出去的那张单子。
///
/// 四个标量字段（`total`/`matched`/`bench`/`asserts`）是"能不能签发"的全部依据，
/// 矩阵则是"签出去之后别人复核"的依据。两者必须自洽，所以 `gate()` 里既有拿标量
/// 比常量的闸，也有拿标量比矩阵求和的闸——只比一种，就能被"改标量不改语料"
/// 或"改语料不改标量"两种造假手法分别绕过。
#[derive(Clone, Copy)]
pub struct Report {
    /// 四档横计。
    pub matrix: Matrix,
    /// 参与判定的槽数。
    pub total: u32,
    /// 六维全对齐的槽数。
    pub matched: u32,
    /// 本批判据条数（写死 19，来自清单而非语料）。
    pub bench: u32,
    /// 本批内部断言总数（来自清单 19 行逐项相加）。
    pub asserts: u32,
}

impl Report {
    /// 无效报告：所有数为零，任何闸都过不去。归档入口异常时的唯一出路。
    pub fn invalid() -> Report {
        Report {
            matrix: Matrix::zero(),
            total: 0u32,
            matched: 0u32,
            bench: 0u32,
            asserts: 0u32,
        }
    }

    /// 通过率（千分）：四舍五入用 n/2 做偏移，与同批 F11660 同一套算法。
    pub fn rate(self: &Report) -> u32 {
        match self.total {
            0u32 => 0u32,
            n => {
                let num = self.matched.saturating_mul(RATE_FULL);
                num.saturating_add(n / 2u32) / n
            }
        }
    }

    /// 绿集条数（Green + Flaked）。
    pub fn green(self: &Report) -> u32 {
        self.matrix.in_green()
    }

    /// 绿档条数。
    pub fn tier_green(self: &Report) -> u32 {
        match self.matrix.row(Tier::Green) {
            Some(r) => r.total,
            None => 0u32,
        }
    }

    /// 抖动档条数。
    pub fn tier_flaked(self: &Report) -> u32 {
        match self.matrix.row(Tier::Flaked) {
            Some(r) => r.total,
            None => 0u32,
        }
    }

    /// 诚实 skip 档条数。
    pub fn tier_skipped(self: &Report) -> u32 {
        match self.matrix.row(Tier::Skipped) {
            Some(r) => r.total,
            None => 0u32,
        }
    }

    /// 阻塞档条数。
    pub fn tier_blocked(self: &Report) -> u32 {
        match self.matrix.row(Tier::Blocked) {
            Some(r) => r.total,
            None => 0u32,
        }
    }

    /// 是否存在抖动档——有的话"一次通过"这个说法不成立，必须单独回答。
    pub fn flake_free(self: &Report) -> bool {
        match self.matrix.row(Tier::Flaked) {
            Some(r) => r.total == 0u32,
            None => true,
        }
    }

    /// 走满全程条数。
    pub fn sealed(self: &Report) -> u32 {
        self.matrix.sealed()
    }

    /// 加权代价。
    pub fn cost(self: &Report) -> u32 {
        self.matrix.cost()
    }

    /// 标量与矩阵是否各说各话。
    ///
    /// 这是本卡唯一能抓住"手工改报告"的闸：`total`/`matched` 由语料累加得出，
    /// 若有人只改结构体字段而不改语料，矩阵求和就会与标量不等。反过来
    /// （只删语料不改标量）也在这里现形，而不是等到 `total != WANT_MATCHED`
    /// 才报——那时报的是"条数不对"，掩盖了"你动了语料"这个真成因。
    fn scalars_match_matrix(self: &Report) -> bool {
        if self.matrix.total() != self.total {
            return false;
        }
        if self.matrix.aligned() != self.matched {
            return false;
        }
        true
    }

    /// 该报告是否可签发——十三道独立闸，一闸一码，先撞先出。
    ///
    /// 顺序有讲究：先验分母三件（total/bench/asserts），再验分子两件
    /// （matched/rate），然后验矩阵自身（covered/green_has_lie/perfect_rows），
    /// 再验两轴一致性（scalars vs matrix、sealed 条数），最后才逐档比档形。
    /// 反过来的话，一个把 total 改成 0 的假绿会先在 rate 上撞除零保护，
    /// 看起来像"分母没问题"；而把语料删成只剩绿档的假覆盖会先过档形闸，
    /// 报出"绿档条数不对"这种误导性结论。
    pub fn gate(self: &Report) -> Result<(), u32> {
        if self.total != WANT_MATCHED {
            return Err(TR_ESTATE);
        }
        if self.bench as usize != BENCH {
            return Err(TR_ESTAGE);
        }
        if self.asserts != WANT_ASSERTS {
            return Err(TR_ECODE);
        }
        if self.matched != self.total {
            return Err(TR_EVALUE);
        }
        if self.rate() != RATE_FULL {
            return Err(TR_ECOUNT);
        }
        if !self.matrix.covered() {
            return Err(TR_EUNIQ);
        }
        if self.matrix.green_has_lie() {
            return Err(TR_ESKIP);
        }
        if self.matrix.perfect_rows() as usize != TIER_COUNT {
            return Err(TR_ECYCLE);
        }
        if !self.scalars_match_matrix() {
            return Err(TR_EFLAKE);
        }
        if self.sealed() != WANT_SEALED {
            return Err(TR_EDEFAULT);
        }
        if self.tier_green() != WANT_GREEN {
            return Err(TR_ENOTE);
        }
        if self.tier_flaked() != WANT_FLAKED {
            return Err(TR_EBLOCK);
        }
        if self.tier_skipped() != WANT_SKIPPED || self.tier_blocked() != WANT_BLOCKED {
            return Err(TR_ESLOT);
        }
        Ok(())
    }

    /// 折指纹（矩阵 + 四个标量）。
    pub fn fold(self: &Report, h: u64) -> u64 {
        let a = self.matrix.fold(h);
        let b = fold_word(a, self.total as u64);
        let c = fold_word(b, self.matched as u64);
        let d = fold_word(c, self.bench as u64);
        fold_word(d, self.asserts as u64)
    }

    /// 签发面单行——判据原文"同文案判据绿"的逐字呈现。
    pub fn render(self: &Report) -> String {
        let mut s = String::from("trim-known-miss total=");
        s.push_str(dec_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(ACCEPT as u32).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(REJECT as u32).as_str());
        s.push_str(" green=");
        s.push_str(dec_text(self.tier_green()).as_str());
        s.push_str(" flaked=");
        s.push_str(dec_text(self.tier_flaked()).as_str());
        s.push_str(" skipped=");
        s.push_str(dec_text(self.tier_skipped()).as_str());
        s.push_str(" blocked=");
        s.push_str(dec_text(self.tier_blocked()).as_str());
        s.push_str(" bench=");
        s.push_str(dec_text(self.bench).as_str());
        s.push_str(" asserts=");
        s.push_str(dec_text(self.asserts).as_str());
        s.push_str(" matched=");
        s.push_str(dec_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s
    }

    /// 全绿签发面的逐字锚——任何字段顺序或拼写改动都会撞这条。
    pub const RENDER_OK: &'static str = "trim-known-miss total=14 accept=8 reject=6 \
                                         green=10 flaked=1 skipped=1 blocked=2 \
                                         bench=19 asserts=105 matched=14 rate=1000";
}

/// 构建聚合报告；`inj` 是"往某槽申报里故意坏一维"的注入槽位，用于证明
/// 六维对齐判定真的能抓到，而不是靠语料恰好干净。
///
/// 这里"对齐"的含义：一槽对齐 = 该槽判定结果等于期望结论（活槽期望 Ok、
/// 死槽期望指定的那一维缺陷码）。这正是 ktest 语义——每条断言自带预期结论，
/// 聚合通过率问的是"预期结论全部命中"。本卡的 14 槽里 6 个是缺陷样本，
/// 若把"对齐"混成"申报为真"，通过率上限就只有 571，那张单子永远发不出去，
/// 于是必然有人把死槽从语料里删掉——假绿的源头就在这里被堵住。
fn build_report(inj: Option<(usize, Fault)>) -> Report {
    let mut m = Matrix::zero();
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => {
                i = i.saturating_add(1usize);
                continue;
            }
        };
        let actual = c.actual();
        let mut claim = c.claim;
        if let Some((slot, f)) = inj {
            if slot == i {
                claim = claim.break_dim(f);
            }
        }
        let fault = judge_against(&claim, &actual);
        let aligned = fault == expected_fault(i);
        if aligned {
            matched = matched.saturating_add(1u32);
        }
        let sealed = actual.stage == Stage::Seal && actual.ok;
        let spec_asserts = match c.shape.asserts {
            a => a as u32,
        };
        if m.bump(actual.tier(), aligned, actual.value, sealed, spec_asserts).is_err() {
            // 归档入口坏了就不出报告——出一个全零报告让 gate 的第一道闸判死，
            // 而不是继续累加出一个自洽的假数。
            return Report::invalid();
        }
        i = i.saturating_add(1usize);
    }
    Report {
        matrix: m,
        total: m.total(),
        matched,
        bench: BENCH as u32,
        asserts: bench_assert_total(),
    }
}

/// 一次命令跑完全批：无注入的聚合。
pub fn run() -> Report {
    build_report(None)
}

/// 带注入的聚合。槽位越界不当"没注入"处理——那是假绿的一种，直接判死。
pub fn run_with(inj: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = inj {
        if slot >= CORPUS {
            return Err(TR_ESLOT);
        }
    }
    Ok(build_report(inj))
}

// ---------------------------------------------------------------------------
// 闸链：把"停在哪儿"变成运行时不变量
// ---------------------------------------------------------------------------

/// 八闸链表的正序。
///
/// 之所以要一份显式链表而不是靠 `Stage::next()` 串：`next()` 是"从这道闸能去哪"，
/// 而链表是"这条链应该长什么样"。两份东西各自独立写出来，才能交叉核出
/// "某道闸的 next 被人改成回指"这类破坏——只有一份时，改了它无人能发现。
pub const STAGE_CHAIN: [Stage; 8usize] = [
    Stage::Build,
    Stage::Table,
    Stage::Identity,
    Stage::Stream,
    Stage::Text,
    Stage::Exit,
    Stage::Leak,
    Stage::Seal,
];

/// 闸数（与 `Stage::COUNT` 必须相等，由 `chain_ok` 核）。
pub const GATE_COUNT: usize = 8usize;

/// 逐闸推进的八道闸。
///
/// 存在的理由：本卡现场里"这条请求验到哪一步"是可以事后补写的，最常见的造假
/// 是把停在 Text 的条说成走到了 Seal（好像文案逐字已核过）。本闸把停留位置变成
/// 运行时不变量——回退即死、越级即死、收口后再推进即死，且每次被拒都进指纹，
/// 拒过的账不可能装作没发生过。
#[derive(Clone, Copy)]
pub struct Gate {
    /// 当前停留闸。
    cur: Option<Stage>,
    /// 是否已收口。
    closed: bool,
    /// 成功推进次数。
    moves: u32,
    /// 被拒次数（进指纹）。
    rejects: u32,
}

impl Gate {
    /// 新建：尚未进入任何闸。
    pub const fn new() -> Gate {
        Gate {
            cur: None,
            closed: false,
            moves: 0u32,
            rejects: 0u32,
        }
    }

    /// 推进到指定闸。首步只能落构建闸，其后每步只能落在当前闸的下一道。
    ///
    /// "首步必须是 rank 1"这条看着苛刻，实际是本卡最要紧的一条：裁剪件判据
    /// 允许在构建闸就诚实 skip（配置缺席），若允许从 Table 起步，就等于允许
    /// 一份从没核过 CONFIG 掩码的报告直接进表闸——而掩码恰恰是 skip 合法性的来源。
    pub fn advance(self: &mut Gate, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(TR_ESTATE);
        }
        match self.cur {
            None => {
                if s.rank() != Stage::Build.rank() {
                    self.rejects = self.rejects.saturating_add(1u32);
                    return Err(TR_ESTAGE);
                }
            }
            Some(prev) => {
                // 只许走下一道，不许越闸：单调性只挡回退，挡不住跳过，
                // 而"跳过泄漏闸"正是本卡最容易发生的假绿（泄漏形态在文案闸
                // 就被判死了，很多人因此以为泄漏闸可以省）。
                match prev.next() {
                    Some(nx) => {
                        if nx != s {
                            self.rejects = self.rejects.saturating_add(1u32);
                            return Err(TR_ESTAGE);
                        }
                    }
                    None => {
                        self.rejects = self.rejects.saturating_add(1u32);
                        return Err(TR_ESTAGE);
                    }
                }
            }
        }
        self.cur = Some(s);
        self.moves = self.moves.saturating_add(1u32);
        Ok(())
    }

    /// 收口——只有走到封印闸才允许。
    pub fn close(self: &mut Gate) -> Result<(), u32> {
        if self.closed {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(TR_ESTATE);
        }
        match self.cur {
            Some(s) => {
                if s.next().is_some() {
                    self.rejects = self.rejects.saturating_add(1u32);
                    return Err(TR_ESTAGE);
                }
            }
            None => {
                self.rejects = self.rejects.saturating_add(1u32);
                return Err(TR_ESTAGE);
            }
        }
        self.closed = true;
        Ok(())
    }

    /// 当前停留闸。
    pub const fn at(self: &Gate) -> Option<Stage> {
        self.cur
    }

    /// 是否已收口。
    pub const fn is_closed(self: &Gate) -> bool {
        self.closed
    }

    /// 推进次数。
    pub const fn moves(self: &Gate) -> u32 {
        self.moves
    }

    /// 被拒次数。
    pub const fn rejects(self: &Gate) -> u32 {
        self.rejects
    }

    /// 是否恰好走满八道并已收口。
    pub const fn full_walk(self: &Gate) -> bool {
        self.moves == GATE_COUNT as u32 && self.closed
    }

    /// 是否停在执行面之后（码档闸及其后）——用于核对"跳过类结论只能在执行前成立"。
    pub fn past_exit(self: &Gate) -> bool {
        match self.cur {
            Some(s) => s.past_exit(),
            None => false,
        }
    }

    /// 按链表推进：STAGE_CHAIN 顺序逐一 advance，最后收口。
    pub fn walk(self: &mut Gate) -> Result<(), u32> {
        let mut i = 0usize;
        while i < GATE_COUNT {
            let s = match STAGE_CHAIN.get(i) {
                Some(x) => *x,
                None => return Err(TR_EUNIQ),
            };
            if self.advance(s).is_err() {
                return Err(TR_ESTAGE);
            }
            i = i.saturating_add(1usize);
        }
        self.close()
    }

    /// 走到"某槽实测停留闸"为止——从当前停留闸继续沿链推进到目标，不收口。
    ///
    /// 这是把 `define()` 的结论拿去驱动闸链的入口：一条红在 Text 的判据，
    /// 它的闸链只能走到 Text；能走到 Seal 的那条必然是另一条。用它做交叉校验，
    /// 就防止"停留闸是抄来的"——抄来的停在 Text 却把链推到 Seal，前后一对就崩。
    ///
    /// 允许接着已有进度走（`walk_to(Text)` 之后 `walk_to(Seal)` 必须成功），因为
    /// 复核者的用法就是先停在实测闸、再问"那它能不能推到封印闸"。回退目标一律拒。
    pub fn walk_to(self: &mut Gate, target: Stage) -> Result<(), u32> {
        match self.cur {
            Some(c) => {
                if c == target {
                    // 已经停在目标闸：空走既不算推进也不算拒绝——推两次同一道闸
                    // 若算拒，复核者重复查同一槽就会污染指纹。
                    return Ok(());
                }
                if c.rank() > target.rank() {
                    self.rejects = self.rejects.saturating_add(1u32);
                    return Err(TR_ESTAGE);
                }
            }
            None => {}
        }
        // 找到当前闸在链上的位置，从它的下一道开始走；尚未起步则从链首开始。
        let mut from = 0usize;
        let mut has_target = false;
        let mut i = 0usize;
        while i < GATE_COUNT {
            let s = match STAGE_CHAIN.get(i) {
                Some(x) => *x,
                None => return Err(TR_EUNIQ),
            };
            match self.cur {
                Some(c) => {
                    if c == s {
                        from = i.saturating_add(1usize);
                    }
                }
                None => {}
            }
            if s == target {
                has_target = true;
            }
            i = i.saturating_add(1usize);
        }
        if !has_target {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(TR_ESTAGE);
        }
        let mut j = from;
        while j < GATE_COUNT {
            let s = match STAGE_CHAIN.get(j) {
                Some(x) => *x,
                None => return Err(TR_EUNIQ),
            };
            if self.advance(s).is_err() {
                return Err(TR_ESTAGE);
            }
            if s == target {
                return Ok(());
            }
            j = j.saturating_add(1usize);
        }
        self.rejects = self.rejects.saturating_add(1u32);
        Err(TR_ESTAGE)
    }

    /// 折指纹。
    pub fn fold(self: &Gate, h: u64) -> u64 {
        let r = match self.cur {
            Some(s) => s.rank() as u64 + 1u64,
            None => 0u64,
        };
        let a = fold_word(h, r);
        let b = fold_word(a, self.closed as u64);
        let c = fold_word(b, self.moves as u64);
        fold_word(c, self.rejects as u64)
    }

    /// 闸链文本。
    pub fn render(self: &Gate) -> String {
        let mut s = String::from("gate at=");
        match self.cur {
            Some(st) => s.push_str(st.slug()),
            None => s.push_str("none"),
        }
        s.push_str(" moves=");
        s.push_str(dec_text(self.moves).as_str());
        s.push_str(" rejects=");
        s.push_str(dec_text(self.rejects).as_str());
        s.push_str(" closed=");
        s.push_str(bool_text(self.closed));
        s
    }
}

// ---------------------------------------------------------------------------
// 账本：一次聚合运行的逐格实记
// ---------------------------------------------------------------------------

/// 统计格的操作号下界。
pub const OP_STAT_BASE: u8 = 1u8;
/// 红档率格的操作号——独占第 8 格，不许与统计格混列。
pub const OP_RED_RATE: u8 = 8u8;
/// 缺陷格的操作号下界。
pub const OP_DEFECT_BASE: u8 = 9u8;
/// 统计格数（不含红档率格）。
pub const STAT_CELLS: usize = 7usize;
/// 缺陷格数。
pub const DEFECT_CELLS: usize = 6usize;

/// 一格实记。
#[derive(Clone, Copy)]
pub struct Sample {
    /// 落格位——必须等于登记时的当前长度，否则视为插队。
    pub slot: usize,
    /// 操作号。
    pub op: u8,
    /// 该格是否成立。
    pub ok: bool,
    /// 失败码（成立格为 0）。
    pub code: u32,
    /// 该格数值。
    pub value: u32,
}

impl Sample {
    /// 空样本——slot 故意填成越界值，任何调用者拿它去登记必然被拒，
    /// 这样"忘了填 slot"的写法在运行时就会暴露，而不是悄悄落在 0 号格。
    pub const fn blank() -> Sample {
        Sample {
            slot: CORPUS,
            op: 0u8,
            ok: false,
            code: 0u32,
            value: 0u32,
        }
    }

    /// 统计格。
    pub const fn stat(slot: usize, idx: usize, value: u32) -> Sample {
        Sample {
            slot,
            op: OP_STAT_BASE + idx as u8,
            ok: true,
            code: 0u32,
            value,
        }
    }

    /// 红档率格。
    pub const fn red_rate(slot: usize, value: u32) -> Sample {
        Sample {
            slot,
            op: OP_RED_RATE,
            ok: true,
            code: 0u32,
            value,
        }
    }

    /// 缺陷格——败格不计数，value 恒为 0，这是 audit 的一条硬约束。
    pub const fn defect(slot: usize, idx: usize, code: u32) -> Sample {
        Sample {
            slot,
            op: OP_DEFECT_BASE + idx as u8,
            ok: false,
            code,
            value: 0u32,
        }
    }

    /// 操作号短名。
    pub const fn op_name(self: &Sample) -> &'static str {
        if self.op >= OP_STAT_BASE && self.op < OP_RED_RATE {
            return "stat";
        }
        if self.op == OP_RED_RATE {
            return "red-rate";
        }
        if self.op >= OP_DEFECT_BASE && self.op < OP_DEFECT_BASE + DEFECT_CELLS as u8 {
            return "defect";
        }
        "void"
    }

    /// 该格是否落在合法操作号区间。
    pub const fn op_in_range(self: &Sample) -> bool {
        if self.op >= OP_STAT_BASE && self.op <= OP_DEFECT_BASE + (DEFECT_CELLS as u8 - 1u8) {
            return self.op != 0u8;
        }
        false
    }

    /// 该格是否统计面（含红档率格）。
    pub fn is_stat(self: &Sample) -> bool {
        self.op >= OP_STAT_BASE && self.op <= OP_RED_RATE
    }

    /// 该格是否缺陷面。
    pub fn is_defect(self: &Sample) -> bool {
        self.op >= OP_DEFECT_BASE && self.op < OP_DEFECT_BASE + DEFECT_CELLS as u8
    }

    /// 折进指纹。
    pub fn fold(self: &Sample, h: u64) -> u64 {
        let a = fold_word(h, self.slot as u64);
        let b = fold_word(a, self.op as u64);
        let c = fold_word(b, self.ok as u64);
        let d = fold_word(c, self.code as u64);
        fold_word(d, self.value as u64)
    }

    /// 文本。
    pub fn render(self: &Sample) -> String {
        let mut s = String::from("  slot ");
        s.push_str(dec_text(self.slot as u32).as_str());
        s.push_str(" op ");
        s.push_str(self.op_name());
        s.push(' ');
        s.push_str(dec_text(self.op as u32).as_str());
        s.push(' ');
        s.push_str(bool_text(self.ok));
        s.push_str(" code ");
        s.push_str(hex_text(self.code, 5).as_str());
        s.push_str(" value ");
        s.push_str(dec_text(self.value).as_str());
        s
    }
}

/// 逐格实记账本。
///
/// 为什么必须有它：聚合报告里的 matched/rate 都是"算出来的"，算的过程一旦可以
/// 自便（跳格、补格、把缺陷格改成成立格），报告就只是叙事。账本把"第几格登了
/// 什么"钉成序列，事后任何一次重放都必须重建出同一个根。
#[derive(Clone, Copy)]
pub struct Ledger {
    /// 格子。
    items: [Sample; SAMPLES],
    /// 已登记长度。
    len: usize,
    /// 封存根。
    root: Option<u64>,
    /// 是否冻结。
    frozen: bool,
    /// 被拒登记次数（进指纹）。
    rejects: u32,
}

impl Ledger {
    /// 空账。
    pub fn new() -> Ledger {
        let mut items = [Sample::blank(); SAMPLES];
        let mut i = 0usize;
        while i < SAMPLES {
            if let Some(slot) = items.get_mut(i) {
                *slot = Sample {
                    slot: i,
                    op: 0u8,
                    ok: false,
                    code: 0u32,
                    value: 0u32,
                };
            }
            i = i.saturating_add(1usize);
        }
        Ledger {
            items,
            len: 0usize,
            root: None,
            frozen: false,
            rejects: 0u32,
        }
    }

    /// 已登记格数。
    pub const fn len(self: &Ledger) -> usize {
        self.len
    }

    /// 是否空账。
    pub const fn is_empty(self: &Ledger) -> bool {
        self.len == 0usize
    }

    /// 是否已冻结。
    pub const fn is_frozen(self: &Ledger) -> bool {
        self.frozen
    }

    /// 被拒次数。
    pub const fn rejects(self: &Ledger) -> u32 {
        self.rejects
    }

    /// 封存根。
    pub const fn root(self: &Ledger) -> Option<u64> {
        self.root
    }

    /// 取一格。
    pub fn sample(self: &Ledger, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        match self.items.get(i) {
            Some(s) => Some(*s),
            None => None,
        }
    }

    /// 登记。三条拒则，顺序即优先级：先冻结、再插队、最后越界。
    ///
    /// 冻结排在最前是因为"冻结后还能拒"会让拒次数污染已封存账，那时 replay 的
    /// 失败原因同时有两个来源，分不清是被改还是被动过。
    pub fn register(self: &mut Ledger, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(TR_ESTATE);
        }
        if s.slot != self.len {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(TR_ESLOT);
        }
        if self.len >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(TR_ESLOT);
        }
        if let Some(slot) = self.items.get_mut(self.len) {
            *slot = s;
        } else {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(TR_ESLOT);
        }
        self.len = self.len.saturating_add(1usize);
        Ok(())
    }

    /// 成功格数。
    pub fn ok_count(self: &Ledger) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if s.ok {
                    n = n.saturating_add(1u32);
                }
            }
            i = i.saturating_add(1usize);
        }
        n
    }

    /// 失败格数。
    pub fn err_count(self: &Ledger) -> u32 {
        (self.len as u32).saturating_sub(self.ok_count())
    }

    /// 缺陷格的码序列（只有已登记的缺陷面参与）。
    pub fn defect_codes(self: &Ledger) -> [u32; DEFECT_CELLS] {
        let mut out = [0u32; DEFECT_CELLS];
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.sample(i) {
                if s.is_defect() {
                    let idx = s.op - OP_DEFECT_BASE;
                    if let Some(cell) = out.get_mut(idx as usize) {
                        *cell = s.code;
                    }
                }
            }
            i = i.saturating_add(1usize);
        }
        out
    }

    /// 逐格核账：统计格值必须落在 `VALS` 前 7 位、红档率格必须等于 `RED_RATE`、
    /// 缺陷格码必须落在 `REJECT_CODES` 且值为 0。任一不符即判死，返回首个不符码。
    pub fn audit(self: &Ledger) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(TR_ECOUNT);
        }
        let mut i = 0usize;
        while i < SAMPLES {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(TR_ESLOT),
            };
            if !s.op_in_range() {
                return Err(TR_EUNIQ);
            }
            if i < ACCEPT {
                let want = match VALS.get(i) {
                    Some(v) => *v,
                    None => return Err(TR_EUNIQ),
                };
                if !s.ok {
                    return Err(TR_ESTATE);
                }
                if s.code != 0u32 {
                    return Err(TR_ECODE);
                }
                if s.value != want {
                    return Err(TR_EVALUE);
                }
                if i == ACCEPT - 1usize && s.op != OP_RED_RATE {
                    return Err(TR_EFLAKE);
                }
            } else {
                let idx = i - ACCEPT;
                let want_code = match REJECT_CODES.get(idx) {
                    Some(v) => *v,
                    None => return Err(TR_EUNIQ),
                };
                if s.ok {
                    return Err(TR_ESTATE);
                }
                if s.code != want_code {
                    return Err(TR_ECODE);
                }
                if s.value != 0u32 {
                    return Err(TR_EVALUE);
                }
            }
            i = i.saturating_add(1usize);
        }
        Ok(())
    }

    /// 指纹：逐格 + 拒次数。拒次数进指纹，是为了让"有一次插队被挡"的账
    /// 永远不可能伪装成 pristine 账。
    fn fingerprint(self: &Ledger) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.items.get(i) {
                h = s.fold(h);
            }
            i = i.saturating_add(1usize);
        }
        fold_word(h, self.rejects as u64)
    }

    /// 封存——只有满格且核过账才允许。
    pub fn seal(self: &mut Ledger) -> Result<u64, u32> {
        if self.frozen {
            return Err(TR_ESTATE);
        }
        if self.len != SAMPLES {
            return Err(TR_ECOUNT);
        }
        if self.audit().is_err() {
            return Err(TR_EUNIQ);
        }
        let r = self.fingerprint();
        self.root = Some(r);
        Ok(r)
    }

    /// 冻结——封存后再上锁；此后任何登记都会污染，而污染会立刻被 replay 抓到。
    pub fn freeze(self: &mut Ledger) -> Result<u64, u32> {
        if self.frozen {
            return Err(TR_ESTATE);
        }
        if self.rejects != 0u32 {
            return Err(TR_ECOUNT);
        }
        let mut r = 0u64;
        let mut have = false;
        if let Some(x) = self.root {
            r = x;
            have = true;
        }
        if !have {
            match self.seal() {
                Ok(v) => r = v,
                Err(e) => return Err(e),
            }
        }
        self.frozen = true;
        Ok(r)
    }

    /// 重放：从当前格序列重建指纹并与封存根比对。
    pub fn replay(self: &Ledger) -> Result<u64, u32> {
        let want = match self.root {
            Some(x) => x,
            None => return Err(TR_ESTATE),
        };
        if self.fingerprint() != want {
            return Err(TR_EUNIQ);
        }
        Ok(want)
    }

    /// 偷改一格数值——只为验证重放真的能抓到，不参与正常流程。
    pub fn poke(self: &mut Ledger, slot: usize, value: u32) -> Result<(), u32> {
        if !self.frozen && self.root.is_none() {
            return Err(TR_ESTATE);
        }
        if slot >= self.len {
            return Err(TR_ESLOT);
        }
        if let Some(s) = self.items.get_mut(slot) {
            s.value = value;
            Ok(())
        } else {
            Err(TR_ESLOT)
        }
    }

    /// 账本文本（含表头）。
    pub fn render(self: &Ledger) -> String {
        let mut s = String::from("trim-ledger cells=");
        s.push_str(dec_text(self.len as u32).as_str());
        s.push_str(" ok=");
        s.push_str(dec_text(self.ok_count()).as_str());
        s.push_str(" err=");
        s.push_str(dec_text(self.err_count()).as_str());
        s.push_str(" rejects=");
        s.push_str(dec_text(self.rejects).as_str());
        s.push_str("\n");
        let mut i = 0usize;
        while i < self.len {
            if let Some(x) = self.sample(i) {
                s.push_str(x.render().as_str());
                s.push_str("\n");
            }
            i = i.saturating_add(1usize);
        }
        match self.root {
            Some(r) => {
                s.push_str("root 0x");
                s.push_str(hex_text((r >> 32u64) as u32, 8usize).as_str());
                s.push_str(hex_text((r & 0xffff_ffffu64) as u32, 8usize).as_str());
                if self.frozen {
                    s.push_str(" frozen\n");
                } else {
                    s.push_str(" sealed\n");
                }
            }
            None => s.push_str("root none\n"),
        }
        s
    }
}

/// 建满账：7 个统计格 + 1 个红档率格 + 6 个缺陷格。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut i = 0usize;
    while i < ACCEPT {
        let v = match VALS.get(i) {
            Some(x) => *x,
            None => 0u32,
        };
        let s = if i + 1usize == ACCEPT {
            Sample::red_rate(i, v)
        } else {
            Sample::stat(i, i, v)
        };
        if l.register(s).is_err() {
            break;
        }
        i = i.saturating_add(1usize);
    }
    let mut j = 0usize;
    while j < DEFECT_CELLS {
        let slot = ACCEPT + j;
        let c = match REJECT_CODES.get(j) {
            Some(x) => *x,
            None => 0u32,
        };
        if l.register(Sample::defect(slot, j, c)).is_err() {
            break;
        }
        j = j.saturating_add(1usize);
    }
    l
}

/// 账本四路注入探针：表路 / 判路 / 渲染路 / 账本路。
///
/// 每一位的含义是"这一路注进去的假象被防线抓住了"，四路必须全为真。
/// 这四路对应本卡能被做脏的四个位置——清单本身、八闸推演、签发面、逐格账。
/// 只测其中一路等于没测：本卡验收原文要求"同文案判据绿"，同文案在推演里判、
/// 在渲染里报、在账本里存，三处都得单独注一次假才能证明三处都真设了防。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4usize];

    // 表路：伪造一行清单（卡号越界、短名为空、断言数为 0、行数下限为 0），
    // 清单逐行闸必须判它不合格；反过来，真清单 19 行必须全合格。
    let fake = Spec {
        card: 0u16,
        tag: "",
        title: "fake",
        harness: Harness::Noop,
        dep: CARD_MAX + 1u16,
        need_cfg: CFG_NONE,
        asserts: 0u16,
        rows_min: 0u16,
        default_suite: false,
    };
    let table_caught = !spec_row_ok(&fake) && manifest_rows_ok();
    if let Some(cell) = out.get_mut(0usize) {
        *cell = table_caught;
    }

    // 判路：把一条应当诚实 skip 的现场改成"申报走满全程"，推演必须把它按在
    // 构建闸上，而不是跟着申报走。
    let skip_shape = match shape_at(4usize) {
        Some(s) => s,
        None => Shape::zero(),
    };
    let real = define(&skip_shape);
    let forged = Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: skip_shape.asserts as u32,
        note: Note::Clean,
        default: true,
    };
    let judge_caught = real.stage == Stage::Build && judge_against(&forged, &real) != Fault::Ok;
    if let Some(cell) = out.get_mut(1usize) {
        *cell = judge_caught;
    }

    // 渲染路：把签发面上的 green 手工抬一档，闸必须拦住；同时真报告的
    // 逐字必须等于 RENDER_OK 锚。
    let r = run();
    let mut tampered = r;
    if let Some(cell) = tampered.matrix.rows.get_mut(0usize) {
        cell.total = cell.total.saturating_add(1u32);
    }
    tampered.total = tampered.total.saturating_add(1u32);
    let render_caught = tampered.gate().is_err() && r.gate().is_ok() && r.render() == Report::RENDER_OK;
    if let Some(cell) = out.get_mut(2usize) {
        *cell = render_caught;
    }

    // 账本路：偷改一格数值，封存根必须与重放不符；未改的账必须能封存且重放。
    let good = build_ledger();
    let mut bad = build_ledger();
    let mut ledger_caught = false;
    if good.audit().is_ok() {
        let sealed = match bad.freeze() {
            Ok(v) => Some(v),
            Err(_) => None,
        };
        if let Some(root) = sealed {
            if bad.poke(2usize, VALS.get(2usize).copied().unwrap_or(0u32) + 1u32).is_ok() {
                ledger_caught = root != 0u64 && bad.replay().is_err();
            }
        }
    }
    if let Some(cell) = out.get_mut(3usize) {
        *cell = ledger_caught;
    }

    out
}

// ---------------------------------------------------------------------------
// 自证层：每道闸的"闸本身没坏"证明
// ---------------------------------------------------------------------------

/// 短名是否长成 `f<卡号>-<后缀>`。
///
/// 不用 `format!` 反推字符串再比（生产码禁用 `format!`），改为逐字节解析：
/// 'f' + 十进制数字段（必须恰好等于卡号）+ '-' + 非空后缀。
/// 之所以要核这一段：TAP 的行号与短名是复核者唯一能对上的东西，短名里的卡号
/// 与 `Spec::card` 不一致时，报告上的"F11663 绿"实际指的是别的卡。
fn tag_matches_card(tag: &str, card: u16) -> bool {
    let bytes = tag.as_bytes();
    let n = bytes.len();
    if n < 3usize {
        return false;
    }
    match bytes.first() {
        Some(b'f') => {}
        _ => return false,
    }
    let mut digits: u16 = 0u16;
    let mut saw = false;
    let mut i = 1usize;
    while i < n {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if b >= b'0' && b <= b'9' {
            digits = digits
                .saturating_mul(10u16)
                .saturating_add((b - b'0') as u16);
            saw = true;
            i = i.saturating_add(1usize);
            continue;
        }
        break;
    }
    if !saw || digits != card {
        return false;
    }
    match bytes.get(i) {
        Some(b'-') => {}
        _ => return false,
    }
    i.saturating_add(1usize) <= n.saturating_sub(1usize)
}

/// 码表自洽：13 条码从 `SEG_BASE+1` 起连续、互不相同，且每条都能被 `code_name` 命名。
fn code_table_ok() -> bool {
    let mut i = 0usize;
    while i < CODE_TABLE.len() {
        let c = match CODE_TABLE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want = SEG_BASE.saturating_add(1u32).saturating_add(i as u32);
        if c != want {
            return false;
        }
        if code_name(c) == "unknown" {
            return false;
        }
        let mut j = i.saturating_add(1usize);
        while j < CODE_TABLE.len() {
            if let Some(o) = CODE_TABLE.get(j) {
                if *o == c {
                    return false;
                }
            }
            j = j.saturating_add(1usize);
        }
        i = i.saturating_add(1usize);
    }
    if code_name(0u32) != "none" {
        return false;
    }
    code_name(SEG_BASE) == "unknown"
}

/// 闸链自洽：链表顺序 == rank 顺序 == next() 串，八闸各有一个专属签名码，
/// 且签名码全部落在码表内。
fn chain_ok() -> bool {
    if GATE_COUNT as u8 != Stage::COUNT || STAGE_CHAIN.len() != GATE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < GATE_COUNT {
        let s = match STAGE_CHAIN.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if s.rank() as usize != i + 1usize {
            return false;
        }
        if Stage::from_rank(s.rank()) != Some(s) {
            return false;
        }
        match s.next() {
            Some(nx) => {
                if nx.rank() != s.rank() + 1u8 {
                    return false;
                }
                if i + 1usize >= GATE_COUNT || STAGE_CHAIN.get(i + 1usize) != Some(&nx) {
                    return false;
                }
            }
            None => {
                if i + 1usize != GATE_COUNT {
                    return false;
                }
            }
        }
        let code = s.fault_code();
        if !CODE_TABLE.contains(&code) {
            return false;
        }
        // 八闸签名码互不相同：两闸共用一码时，报告上的"停在第几闸"就读不出来了。
        let mut j = 0usize;
        let mut dup = 0u32;
        while j < GATE_COUNT {
            if let Some(o) = STAGE_CHAIN.get(j) {
                if o.fault_code() == code {
                    dup = dup.saturating_add(1u32);
                }
            }
            j = j.saturating_add(1usize);
        }
        if dup != 1u32 {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 越界闸号必须回查为 None——不许悄悄落到 Seal。
    Stage::from_rank(0u8).is_none() && Stage::from_rank(Stage::COUNT + 1u8).is_none()
}

/// 形态档自洽：rank 回查闭合、合法 notfound 面与泄漏面互斥、逐字长度公式成立。
///
/// 长度公式是从构造子逐段数出来的（`busybox`=7、`": '"`=3、MID=19…），写成
/// 常量比较而不是"跑一遍看看"：只有给出确定数字，改错任何一段文案才会被抓到。
fn form_ok() -> bool {
    let mut r = 1u8;
    while r <= Form::COUNT {
        let f = match Form::from_rank(r) {
            Some(x) => x,
            None => return false,
        };
        if f.rank() != r {
            return false;
        }
        if f.is_leak() && f.is_valid_notfound() {
            // 互斥：同一形态既算合法答复又算泄漏，判死面就自相矛盾了
            return false;
        }
        if !f.is_leak() && !f.is_valid_notfound() && f != Form::Blank {
            return false;
        }
        if f.is_leak() != (f == Form::DisabledHint
            || f == Form::CandidateHint
            || f == Form::ConfigHint)
        {
            return false;
        }
        if f.weight() == 0u32 {
            return false;
        }
        r = r.saturating_add(1u8);
    }
    if Form::from_rank(0u8).is_some() || Form::from_rank(Form::COUNT + 1u8).is_some() {
        return false;
    }
    if Form::Unknown.is_valid_notfound() || Form::Unknown.rank() != 0u8 {
        return false;
    }
    // 三种合法 notfound 形态的逐字长度
    let name = "nosuchapplet";
    let nl = name.len() as u32;
    if text_modern(name).len() as u32 != 29u32 + nl {
        return false;
    }
    if text_legacy().len() as u32 != 25u32 {
        return false;
    }
    if text_stub(name).len() as u32 != 28u32 + nl {
        return false;
    }
    // 三种反面形态也各给一条：反面长度错了说明构造子被动过，泄漏判死会跟着失准
    if text_disabled(name).len() as u32 != 30u32 + nl {
        return false;
    }
    let cand = "cls";
    if text_didyou(name, cand).len() as u32 != 46u32 + nl + cand.len() as u32 {
        return false;
    }
    if text_config_hint(name).len() as u32 != 64u32 + 2u32 * nl {
        return false;
    }
    if !text_empty().is_empty() {
        return false;
    }
    // 反面档必须真的含着禁用词——否则"按实况文本判泄漏"的那条闸形同虚设
    if !text_disabled(name).as_str().contains(FORBIDDEN_DISABLED) {
        return false;
    }
    if !text_didyou(name, cand).as_str().contains(FORBIDDEN_DIDYOU) {
        return false;
    }
    if !text_config_hint(name).as_str().contains(FORBIDDEN_CONFIG) {
        return false;
    }
    // 带名字面：现代/桩体把名字印进去，旧版不印
    if !Form::Modern.carries_name() || !Form::Stub.carries_name() {
        return false;
    }
    if Form::Legacy.carries_name() {
        return false;
    }
    if !text_modern(name).as_str().contains(name) || !text_stub(name).as_str().contains(name) {
        return false;
    }
    if text_legacy().as_str().contains(name) {
        return false;
    }
    true
}

/// 流档自洽：五档 rank 闭合，且"污染 stdout"与"饿死 stderr"两个面各按真值表覆盖。
///
/// 真值表逐档写死而不是只问"合法"，是因为本卡最要紧的两种坏正好落在两个面的
/// 差集上：`Both`（答复两边都印，stdout 脏但 stderr 有全文——会被"只看 stderr"
/// 的比对器放过）与 `None`（什么都没印，stdout 干净却被误当成"没污染就行"）。
fn stream_ok_enums() -> bool {
    let mut r = 1u8;
    while r <= Stream::COUNT {
        let s = match Stream::from_rank(r) {
            Some(x) => x,
            None => return false,
        };
        if s.rank() != r || s.slug().is_empty() {
            return false;
        }
        if s.readable() != (s == Stream::Stderr) {
            return false;
        }
        let p = s.pollutes_stdout();
        let q = s.starves_stderr();
        match s {
            Stream::Stderr => {
                if p || q {
                    return false;
                }
            }
            Stream::Stdout => {
                if !p || !q {
                    return false;
                }
            }
            Stream::Both | Stream::Split => {
                if !p || q {
                    return false;
                }
            }
            Stream::None => {
                if p || !q {
                    return false;
                }
            }
            Stream::Unknown => {
                return false;
            }
        }
        r = r.saturating_add(1u8);
    }
    if Stream::Unknown.rank() != 0u8 || Stream::Unknown.readable() {
        return false;
    }
    Stream::from_rank(0u8).is_none() && Stream::from_rank(Stream::COUNT + 1u8).is_none()
}

/// 注记档自洽：17 档 rank 回查闭合、名可查、"红/不红"与"吃不吃 TAP 位"两面各自只有
/// 规定的例外。
fn note_ok() -> bool {
    let mut r = 1u8;
    while r <= Note::COUNT {
        let n = match Note::from_rank(r) {
            Some(x) => x,
            None => return false,
        };
        if n.rank() != r {
            return false;
        }
        if n.slug().is_empty() {
            return false;
        }
        if n.is_fact() != (n != Note::Clean) {
            // 除 Clean 外，1..COUNT 全是红注记；None 在 rank 0，不参与这个循环
            return false;
        }
        if n.eats_bit() != (n != Note::AbsentCarrier) {
            // skip 是唯一不吃位的红注记：它挂 `# skip` 指令，不改 ok 计数
            return false;
        }
        r = r.saturating_add(1u8);
    }
    if Note::None.rank() != 0u8 || !Note::None.slug().is_empty() {
        // None 是唯一允许空名的档（skip 指令挂 TAP 尾，成因由 Skip 维给）；
        // 反过来，其余任何一档空名都算注记面失去区分度。
        return false;
    }
    if Note::Clean.slug().is_empty() || Note::AbsentCarrier.slug().is_empty() {
        return false;
    }
    if Note::None.is_fact() {
        return false;
    }
    if Note::from_rank(0u8) != Some(Note::None)
        || Note::from_rank(Note::COUNT + 1u8).is_some()
    {
        // 注记面有 rank 0（None），所以链首必须回查到 None 而不是空——这一点和
        // 形态/流/载体/档位四个"无 rank 0"的枚举不同，抄错就会把整档判成不存在。
        return false;
    }
    // 抖绿与洗绿、缺席与前置未达成必须是四档不同的事
    if Note::FlakeRecovered == Note::Clean || Note::DepBlocked == Note::AbsentCarrier {
        return false;
    }
    if Note::TrimLeak == Note::CandidateList || Note::StdoutPolluted == Note::StderrStarved {
        return false;
    }
    true
}

/// skip 成因自洽：成因 → 吃掉位 → 归因档 三方一致，且掩码推导永不会凭空造出
/// NoCarrier/NoHarness 这类"推不出"的成因。
fn skip_ok() -> bool {
    let mut r = 1u8;
    while r <= Skip::COUNT {
        let s = match Skip::from_rank(r) {
            Some(x) => x,
            None => return false,
        };
        if s.rank() != r || !s.is_skip() || !s.keeps_ok_line() {
            return false;
        }
        if s.note() != Note::AbsentCarrier {
            return false;
        }
        if s.slug().is_empty() {
            return false;
        }
        r = r.saturating_add(1u8);
    }
    if Skip::None.is_skip() || Skip::None.note() != Note::None {
        return false;
    }
    // NoCarrier 与 NoTwoMachine 的位归属：前者不吃位，后者吃双机位
    if Skip::NoCarrier.consumed_bit() != CFG_NONE {
        return false;
    }
    if Skip::NoTwoMachine.consumed_bit() != CFG_TWO_MACHINE {
        return false;
    }
    if Skip::NoTrimMatrix.consumed_bit() != CFG_TRIM_MATRIX {
        return false;
    }
    // 掩码方向：置 1 = 缺。满配置必须推不出任何成因。
    if derive_skip(CFG_NONE) != Skip::None || derive_skip(CFG_ALL) == Skip::None {
        return false;
    }
    // 单成因位逐个验证：只缺某一位必须推出那一位对应的成因
    if derive_skip(CFG_APPLET) != Skip::NoAppletCfg || derive_skip(CFG_ASH) != Skip::NoAsh {
        return false;
    }
    if derive_skip(CFG_STATIC) != Skip::NoStatic || derive_skip(CFG_NOAPPLET) != Skip::NoStub {
        return false;
    }
    // 优先级：矩阵位压过双机位，双机位压过 applet 位——被裁名同时缺矩阵时，
    // 诚实结论是"这条压根没法跑矩阵"，不是"被裁名缺席"。
    if derive_skip(CFG_TRIM_MATRIX | CFG_APPLET) != Skip::NoTrimMatrix {
        return false;
    }
    if derive_skip(CFG_TWO_MACHINE | CFG_ASH) != Skip::NoTwoMachine {
        return false;
    }
    true
}

/// 载体自洽：七档 rank 闭合、耗时带单调、第二份构建需求与默认归属自洽。
///
/// 耗时带的下界取半、上界取四倍是判据面的硬约束，这里核的是"带本身别写反"：
/// 若把 ListProbe 的 40 写成 4，所有真跑 40ms 的判据会被当成挂死。
fn harness_ok() -> bool {
    let mut r = 1u8;
    let mut prev_band = 0u32;
    while r <= Harness::COUNT {
        let h = match Harness::from_rank(r) {
            Some(x) => x,
            None => return false,
        };
        if h.rank() != r || h.slug().is_empty() {
            return false;
        }
        if h == Harness::Noop {
            if h.ms_band() != 0u32 || h.yields_text() || h.default_member() {
                return false;
            }
        } else {
            if h.ms_band() == 0u32 || !h.yields_text() {
                return false;
            }
            // 直接调用必然快于软链接，软链接快于列表探测——带序反了就是抄错。
            if h == Harness::DirectCall {
                prev_band = h.ms_band();
            } else if h == Harness::SymlinkCall {
                if h.ms_band() <= prev_band {
                    return false;
                }
                prev_band = h.ms_band();
            } else if h == Harness::ListProbe {
                if h.ms_band() <= prev_band {
                    return false;
                }
                prev_band = h.ms_band();
            }
            // 需要第二份构建的载体一律不进默认套件
            if h.needs_second() && h.default_member() {
                return false;
            }
        }
        r = r.saturating_add(1u8);
    }
    Harness::from_rank(0u8).is_none() && Harness::from_rank(Harness::COUNT + 1u8).is_none()
}

/// 档位自洽：四档 rank 闭合、绿集归属与 halted 划分互补、权重大于零。
fn tier_ok() -> bool {
    let mut r = 1u8;
    while r <= TIER_COUNT as u8 {
        let t = match Tier::from_rank(r) {
            Some(x) => x,
            None => return false,
        };
        if t.rank() != r || t.weight() == 0u32 || t.slug().is_empty() {
            return false;
        }
        if t.in_green() == t.is_halted() {
            return false;
        }
        r = r.saturating_add(1u8);
    }
    if Tier::from_rank(0u8).is_some() || Tier::from_rank(TIER_COUNT as u8 + 1u8).is_some() {
        return false;
    }
    if !Tier::Green.in_green() || !Tier::Flaked.in_green() {
        return false;
    }
    if Tier::Skipped.in_green() || Tier::Blocked.in_green() {
        return false;
    }
    Tier::Blocked.weight() > Tier::Green.weight()
}

/// 清单单行自洽。
///
/// 这九条合起来说的是"一行清单在跑之前就必须是可执行的"：卡号在本批、短名带
/// 卡号、载体在册且不是 Noop、断言数在 1..=13、行数下限达到本批最低承诺、
/// 前置要么是 0 要么是本批内更早的卡、配置掩码不越界、默认归属与载体不矛盾。
fn spec_row_ok(sp: &Spec) -> bool {
    if sp.card < CARD_MIN || sp.card > CARD_MAX {
        return false;
    }
    if !tag_matches_card(sp.tag, sp.card) {
        return false;
    }
    if sp.title.is_empty() {
        return false;
    }
    if sp.harness == Harness::Noop {
        return false;
    }
    if sp.asserts == 0u16 || sp.asserts > 13u16 {
        return false;
    }
    if sp.rows_min < 320u16 || sp.rows_min > 600u16 {
        return false;
    }
    if sp.dep != 0u16 && (sp.dep < CARD_MIN || sp.dep >= sp.card) {
        return false;
    }
    if sp.need_cfg & !CFG_ALL != 0u32 {
        return false;
    }
    if sp.default_suite && (sp.harness.needs_second() || !sp.harness.default_member()) {
        return false;
    }
    true
}

/// 清单整体自洽：19 行逐行合格、卡号严格递增、短名唯一、前置成链、合计对得上。
fn manifest_rows_ok() -> bool {
    if SPECS.len() != BENCH {
        return false;
    }
    let mut i = 0usize;
    let mut prev_card = 0u16;
    while i < BENCH {
        let sp = match SPECS.get(i) {
            Some(x) => x,
            None => return false,
        };
        if !spec_row_ok(sp) {
            return false;
        }
        if i == 0usize {
            if sp.card != CARD_MIN || sp.dep != 0u16 {
                return false;
            }
        } else {
            if sp.card <= prev_card {
                return false;
            }
            if sp.dep != prev_card {
                return false;
            }
            // 前置成链：第 i 行的前置必须是第 i-1 行，且它自己的短名可回查。
            if spec_index_of(sp.dep).is_none() {
                return false;
            }
        }
        let mut j = i.saturating_add(1usize);
        while j < BENCH {
            if let Some(o) = SPECS.get(j) {
                if o.tag == sp.tag || o.card == sp.card {
                    return false;
                }
            }
            j = j.saturating_add(1usize);
        }
        prev_card = sp.card;
        i = i.saturating_add(1usize);
    }
    if prev_card != CARD_MAX {
        return false;
    }
    if spec_of_card(CARD).is_none() || spec_at(BENCH).is_some() {
        return false;
    }
    // 本卡自己的前置（F11662）必须在清单里且确实更早——验收原文的"与 F11595 联签"
    // 之外还写了前置 F11662，两条都得能从清单里查出来。
    match spec_of_card(CARD) {
        Some(selfrow) => {
            if selfrow.dep == 0u16 || spec_of_card(selfrow.dep).is_none() {
                return false;
            }
            if selfrow.need_cfg & CFG_APPLET == 0u32 {
                return false;
            }
        }
        None => return false,
    }
    true
}

/// 清单统计与 `VALS` 的交叉核对：账本里那 7 个统计格不是手抄的数。
///
/// 每个数都同时由"清单累加"与"常量表"给出，两边取一次等号，才允许登账。
/// 这是本卡最容易出假绿的地方：统计格错了，`audit()` 照样过，因为它只比 VALS。
fn vals_match_ok() -> bool {
    if VALS.len() != ACCEPT {
        return false;
    }
    let want: [u32; ACCEPT] = [
        BENCH as u32,
        bench_dep_total(),
        Note::COUNT as u32,
        Stage::COUNT as u32,
        CODE_TABLE.len() as u32,
        Harness::COUNT as u32,
        bench_rows_total(),
        RED_RATE,
    ];
    let mut i = 0usize;
    while i < ACCEPT {
        let a = match VALS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let b = match want.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a != b {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 红档率的来历必须自证：14 格里 1 格不符时的千分通过率
    if RED_RATE != (13u32 * RATE_FULL + 7u32) / 14u32 {
        return false;
    }
    if bench_assert_total() != WANT_ASSERTS {
        return false;
    }
    if WANT_GREEN + WANT_FLAKED + WANT_SKIPPED + WANT_BLOCKED != CORPUS as u32 {
        return false;
    }
    if WANT_GREEN + WANT_FLAKED != WANT_INGREEN || WANT_SKIPPED + WANT_BLOCKED != WANT_HALTED {
        return false;
    }
    if WANT_SEALED != WANT_GREEN + WANT_FLAKED {
        return false;
    }
    true
}

/// 语料绑定自洽：14 槽占用 14 张不同的卡，卡号与短名逐槽对齐，活/死槽数对得上。
///
/// 死槽也各占一张真卡——缺陷形态说的是"这条真判据被报坏了"，不是"又多一条判据"。
/// 若允许两个槽同卡，`covered_spec_count()` 就会与 14 不符，通过率的分母随之失真。
fn corpus_binding_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS || WANT_NDX.len() != CORPUS {
        return false;
    }
    if WANT_TAGS.len() != CORPUS {
        return false;
    }
    if live_count() != ACCEPT {
        return false;
    }
    if covered_spec_count() != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(x) => x,
            None => return false,
        };
        let tag = match WANT_TAGS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if sh.tag != tag {
            return false;
        }
        if !tag_matches_card(tag, sh.card) {
            return false;
        }
        if spec_of_card(sh.card).is_none() {
            return false;
        }
        // 每槽的 asserts 必须逐字抄自清单行——否则 spec_sum 那列是编的
        match spec_of_card(sh.card) {
            Some(sp) => {
                if sh.asserts != sp.asserts {
                    return false;
                }
                if sh.tag != sp.tag {
                    return false;
                }
            }
            None => return false,
        }
        // 槽间不得撞卡
        let mut j = i.saturating_add(1usize);
        while j < CORPUS {
            if let Some(o) = shape_at(j) {
                if o.card == sh.card {
                    return false;
                }
            }
            j = j.saturating_add(1usize);
        }
        i = i.saturating_add(1usize);
    }
    // 缺陷维索引表：活槽一律 REJECT，死槽 0..REJECT 各出现一次
    let mut seen = [false; REJECT];
    let mut live = 0usize;
    let mut k = 0usize;
    while k < CORPUS {
        let ndx = match WANT_NDX.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if ndx >= REJECT {
            live = live.saturating_add(1usize);
        } else {
            if let Some(cell) = seen.get_mut(ndx) {
                if *cell {
                    return false;
                }
                *cell = true;
            } else {
                return false;
            }
        }
        k = k.saturating_add(1usize);
    }
    if live != ACCEPT {
        return false;
    }
    let mut m = 0usize;
    while m < REJECT {
        if !seen[m] {
            return false;
        }
        m = m.saturating_add(1usize);
    }
    REJECT_FAULTS.len() == REJECT
        && REJECT_CODES.len() == REJECT
        && CLAIM_BROKEN.len() == REJECT
        && expected_fault(CORPUS) == Fault::Ok
}

/// 停留闸到错误码的允许集。
///
/// 码不是闸的单值函数——三个闸各有第二条死因，各有自己的码：
///
/// - 构建闸分三条。根本没跑（载体缺席）是**环境问题**，给跳过码；计时越界是
///   现场不合格，给状态码；掩码/表数/指纹自相矛盾才是构建闸自己的码。
/// - 泄漏闸分两条。文案里说漏嘴给注记码，这条绿是重试洗出来的给抖动码。
/// - 封印闸分两条。前置未达给阻塞码，断言没走满给计数码——封印闸的"数不对"
///   和构建闸的"数不对"是两件事：前者是断言数，后者是表项数。
///
/// 其余五闸一闸一码，直接取 `fault_code()`。允许集之外的组合一律判死：
/// 码串位意味着签发面上的错误分布失真，而错误分布正是这张卡要对外承诺的东西。
fn stage_code_ok(stage: Stage, code: u32) -> bool {
    match stage {
        Stage::Build => {
            code == TR_ESKIP || code == TR_ESTATE || code == Stage::Build.fault_code()
        }
        Stage::Table => code == Stage::Table.fault_code(),
        Stage::Identity => code == Stage::Identity.fault_code(),
        Stage::Stream => code == Stage::Stream.fault_code(),
        Stage::Text => code == Stage::Text.fault_code(),
        Stage::Exit => code == Stage::Exit.fault_code(),
        Stage::Leak => code == TR_ENOTE || code == TR_EFLAKE,
        Stage::Seal => code == Stage::Seal.fault_code() || code == TR_ECOUNT,
    }
}

/// 八闸推演自洽：14 槽实测全部命中期望停留闸与期望注记，且推演可重复。
///
/// 可重复这一条看着多余，实则是本卡的一条暗雷：`define()` 里若有一步用了
/// `attempts` 之类的可变输入去做两次不同判断（例如把洗绿判成抖动），指纹就会
/// 在同一次运行里飘，账本永远封不上。
fn define_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(x) => x,
            None => return false,
        };
        let a = define(&sh);
        let b = define(&sh);
        if a.stage != b.stage || a.code != b.code || a.value != b.value || a.note != b.note {
            return false;
        }
        if a.ok != b.ok || a.default != b.default || a.len != b.len {
            return false;
        }
        if a.fold(FNV_OFF) != b.fold(FNV_OFF) {
            return false;
        }
        if a.fold(FNV_OFF) == a.fold(FNV_PRIME) && a.len != 0u32 {
            // 折叠必须依赖入口基：与初值无关的指纹等于没把结局折进去
            return false;
        }
        if a.ok {
            // 绿/抖两档必须停在封印闸且码为 0，且数值面等于声明的断言数
            if a.stage != Stage::Seal || a.code != 0u32 {
                return false;
            }
            if a.value != sh.asserts as u32 {
                return false;
            }
            if !a.concluded() {
                return false;
            }
            if a.note != Note::Clean && a.note != Note::FlakeRecovered {
                return false;
            }
            if a.default != sh.default_suite {
                // 默认位由载体推得，必须与清单申报的那一位一致——不一致就是清单抄错
                return false;
            }
            if a.len != sh.log_bytes {
                return false;
            }
        } else {
            if a.code == 0u32 {
                return false;
            }
            if !stage_code_ok(a.stage, a.code) {
                return false;
            }
            // 泄漏闸的码与注记互为充要：抖动码只能配抖动注记。
            // 这一条专门守 `leak_code()` 与 `leak_why()` 分家后可能出现的错配——
            // 两个函数各判一次 `washed`，改一处忘另一处，签发面的抖动数就会凭空多一条。
            if a.stage == Stage::Leak
                && (a.code == TR_EFLAKE) != (a.note == Note::FlakeRecovered)
            {
                return false;
            }
            // 封印闸的红只有两条合法来源：前置未达与断言未走满。
            if a.stage == Stage::Seal
                && a.note != Note::DepBlocked
                && a.note != Note::AssertShort
            {
                return false;
            }
            if !a.note.is_fact() {
                // 红必须带一个红注记；None/Clean 出现在红行里就是归因缺失
                return false;
            }
            // skip 与 blocked 的分界：AbsentCarrier 只在执行前成立，且只有它算 Skipped
            if a.note == Note::AbsentCarrier && a.stage.past_exit() {
                return false;
            }
            if a.tier() == Tier::Skipped && a.note != Note::AbsentCarrier {
                return false;
            }
            if a.tier() == Tier::Blocked && a.note == Note::AbsentCarrier {
                return false;
            }
        }
        i = i.saturating_add(1usize);
    }
    // 四档在本语料里都出现过——否则档位面没被真正跑过
    let mut seen_green = false;
    let mut seen_flake = false;
    let mut seen_skip = false;
    let mut seen_block = false;
    let mut k = 0usize;
    while k < CORPUS {
        let o = match outcome_at(k) {
            Some(x) => x,
            None => return false,
        };
        match o.tier() {
            Tier::Green => seen_green = true,
            Tier::Flaked => seen_flake = true,
            Tier::Skipped => seen_skip = true,
            Tier::Blocked => seen_block = true,
        }
        k = k.saturating_add(1usize);
    }
    if !seen_green || !seen_flake || !seen_skip || !seen_block {
        return false;
    }
    true
}

/// 六维对判自洽：每条死槽恰好坏在它声称的那一维，活槽全对，注入能翻页。
///
/// 这一条是 `judge_against` 的回归网：把 Estate 与 Stage 的判序写反，
/// 槽 8（成败互报）就会报成 Stage，通过率立刻少一条；而槽 9（停留闸前移）
/// 报成 Estate 也一样。两个方向都能被这条抓到。
fn judge_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        if c.fault() != expected_fault(i) {
            return false;
        }
        if j(&c.shape, &c.claim) != c.fault() {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 申报从实测重建，必然全对——这一条挡住"实测与申报两套字段"的漂移
    let mut k = 0usize;
    while k < CORPUS {
        let o = match outcome_at(k) {
            Some(x) => x,
            None => return false,
        };
        let rebuilt = Claim::from_outcome(&o);
        if judge_against(&rebuilt, &o) != Fault::Ok {
            return false;
        }
        k = k.saturating_add(1usize);
    }
    // 逐维注入：对同一槽连续坏两维，必须报出先撞的那一维（次序即优先级）
    let sh = match shape_at(0usize) {
        Some(x) => x,
        None => return false,
    };
    let o = define(&sh);
    let base = Claim::from_outcome(&o);
    let two = base.break_dim(Fault::Estate).break_dim(Fault::Stage);
    if judge_against(&two, &o) != Fault::Estate {
        return false;
    }
    let nd = base.break_dim(Fault::Note).break_dim(Fault::Default);
    if judge_against(&nd, &o) != Fault::Note {
        return false;
    }
    // 六维各自单独注入都能被抓到，且六个码互不相同
    let mut f = 0usize;
    while f < REJECT {
        let dim = match REJECT_FAULTS.get(f) {
            Some(x) => *x,
            None => return false,
        };
        if base.fold(0u64) == base.break_dim(dim).fold(0u64) {
            // 坏这一维必须留下可观测差别——改不出差别的维等于没有这一维
            return false;
        }
        if judge_against(&base.break_dim(dim), &o) != dim {
            return false;
        }
        if dim.code() == 0u32 || !CODE_TABLE.contains(&dim.code()) {
            return false;
        }
        if dim.estate() != (dim == Fault::Estate) {
            return false;
        }
        // 归属面划分：Value 同时属账目与文本，Stage/Code 两样都不属
        match dim {
            Fault::Value => {
                if !dim.accounting() || !dim.textual() {
                    return false;
                }
            }
            Fault::Note => {
                if !dim.textual() || dim.accounting() {
                    return false;
                }
            }
            Fault::Default => {
                if !dim.accounting() || dim.textual() {
                    return false;
                }
            }
            Fault::Stage | Fault::Code | Fault::Estate => {
                if dim.accounting() || dim.textual() {
                    return false;
                }
            }
            Fault::Ok => {
                return false;
            }
        }
        f = f.saturating_add(1usize);
    }
    true
}

/// 六个维的独立入口——`judge()` 只吃 `Case`，交叉校验时要按 (事实, 申报) 直接对判。
fn judge_of(sh: &Shape, cl: &Claim) -> Fault {
    judge_against(cl, &define(sh))
}

/// 上条的别名，保持调用点短。
fn j(sh: &Shape, cl: &Claim) -> Fault {
    judge_of(sh, cl)
}

/// 横计面自洽：Row/Matrix 的算术闭合，且四档分布与验收文案完全一致。
fn matrix_ok() -> bool {
    let mut m = Matrix::zero();
    if m.covered() || m.total() != 0u32 || m.aligned() != 0u32 {
        return false;
    }
    if m.row(Tier::Green).is_none() || m.row_at(TIER_COUNT).is_some() {
        return false;
    }
    // 空档没有"未对齐"可言：total==0 时 aligned<total 不成立，绿档谎查必须放行，
    // 但 covered() 必须先拦下——否则"四档全空"也能签。
    if m.green_has_lie() || m.any_lie() || m.perfect_rows() != 0u32 {
        return false;
    }
    if m.bump(Tier::Green, true, 8u32, true, 8u32).is_err() {
        return false;
    }
    if m.bump(Tier::Green, false, 4u32, false, 4u32).is_err() {
        return false;
    }
    if !m.green_has_lie() || m.perfect_rows() != 0u32 {
        return false;
    }
    if m.total() != 2u32 || m.aligned() != 1u32 || m.sealed() != 1u32 {
        return false;
    }
    if m.in_green() != 2u32 || m.halted() != 0u32 || m.cost() != 2u32 {
        return false;
    }
    let g = match m.row(Tier::Green) {
        Some(r) => r,
        None => return false,
    };
    if g.lie() || g.seal_rate() != 500u32 || g.mean_value() != 6u32 {
        return false;
    }
    // 真语料跑一遍横计：必须四档齐全、条条对齐、无谎
    let real = run();
    if !real.matrix.covered() || real.matrix.any_lie() || real.matrix.green_has_lie() {
        return false;
    }
    if real.matrix.perfect_rows() as usize != TIER_COUNT {
        return false;
    }
    if real.tier_green() != WANT_GREEN
        || real.tier_flaked() != WANT_FLAKED
        || real.tier_skipped() != WANT_SKIPPED
        || real.tier_blocked() != WANT_BLOCKED
    {
        return false;
    }
    if real.green() != WANT_INGREEN || real.matrix.halted() != WANT_HALTED {
        return false;
    }
    if real.sealed() != WANT_SEALED || real.flake_free() {
        return false;
    }
    if real.matrix.total() != CORPUS as u32 || real.matrix.aligned() != CORPUS as u32 {
        return false;
    }
    // 抽到清单上的断言数必然小于全批：语料只有 14 槽
    if real.matrix.spec_total() >= WANT_ASSERTS {
        return false;
    }
    if real.cost() == 0u32 {
        return false;
    }
    if real.matrix.render().lines().count() != TIER_COUNT {
        return false;
    }
    true
}

/// 签发面自洽：闸全过、逐字锚对得上、每个篡改面都被拦。
///
/// 篡改清单列了七种——它们正是本卡能被做脏的七个位置。少测一种，那一种就是
/// "改了也没人发现"的通道。
fn report_ok() -> bool {
    let r = run();
    if r.gate().is_err() {
        return false;
    }
    if r.render() != Report::RENDER_OK {
        return false;
    }
    if r.rate() != RATE_FULL || r.total != WANT_MATCHED || r.matched != r.total {
        return false;
    }
    if r.bench as usize != BENCH || r.asserts != WANT_ASSERTS {
        return false;
    }
    if Report::invalid().gate().is_ok() {
        return false;
    }
    if run_with(Some((CORPUS, Fault::Estate))).is_ok() {
        return false;
    }
    if run_with(Some((0usize, Fault::Estate))).is_err() {
        return false;
    }
    if run_with(None).is_err() {
        return false;
    }
    // 篡改一：抬 green 档条数
    let mut a = r;
    if let Some(cell) = a.matrix.rows.get_mut(0usize) {
        cell.total = cell.total.saturating_add(1u32);
    }
    a.total = a.total.saturating_add(1u32);
    if a.gate().is_ok() {
        return false;
    }
    // 篡改二：抬 matched 不抬矩阵（标量与矩阵不符）
    let mut b = r;
    b.matched = b.matched.saturating_add(1u32);
    if b.gate().is_ok() {
        return false;
    }
    // 篡改三：把抖动档并进绿档——条数守恒，但两档都不再等于期望
    let mut c = r;
    if let Some(cell) = c.matrix.rows.get_mut(0usize) {
        cell.total = cell.total.saturating_add(1u32);
    }
    if let Some(cell) = c.matrix.rows.get_mut(1usize) {
        cell.total = cell.total.saturating_sub(1u32);
    }
    if c.gate().is_ok() {
        return false;
    }
    // 篡改四：把 skip 档伪装成走满全程（sealed 抬、total 不动）
    let mut d = r;
    if let Some(cell) = d.matrix.rows.get_mut(2usize) {
        cell.sealed = cell.sealed.saturating_add(1u32);
    }
    if d.gate().is_ok() {
        return false;
    }
    // 篡改五：把 asserts 抄成 116（同批 F11660 的数）
    let mut e = r;
    e.asserts = 116u32;
    if e.gate().is_ok() {
        return false;
    }
    // 篡改六：删掉 blocked 档整行——分母跟着掉，covered 必须先拦住
    let mut f = r;
    if let Some(cell) = f.matrix.rows.get_mut(3usize) {
        *cell = Row::empty(Tier::Blocked);
    }
    f.total = f.matrix.total();
    f.matched = f.matrix.aligned();
    if f.gate().is_ok() {
        return false;
    }
    // 篡改七：绿档里藏一条未对齐（把一条 blocked 涂进 green 且不给对齐）
    let mut g2 = r;
    if let Some(cell) = g2.matrix.rows.get_mut(0usize) {
        cell.total = cell.total.saturating_add(1u32);
    }
    if let Some(cell) = g2.matrix.rows.get_mut(3usize) {
        cell.total = cell.total.saturating_sub(1u32);
    }
    g2.total = g2.matrix.total();
    if g2.gate().is_ok() {
        return false;
    }
    // 折叠面三条：同一次推演重复折叠同值（签发可复现）、换入口基必须换值
    // （指纹真把结局折进去了）、改一个字节必须换值（指纹不钝）。
    // 三条各挡一种假象：不可复现的账、与初值无关的账、对内容不敏感的账。
    let again = run();
    if r.fold(FNV_OFF) != again.fold(FNV_OFF) {
        return false;
    }
    if r.fold(0u64) == r.fold(FNV_OFF) {
        return false;
    }
    let mut h = r;
    h.matched = h.matched.saturating_sub(1u32);
    if h.fold(0u64) == r.fold(0u64) {
        return false;
    }
    true
}

/// 闸链自洽：正走能满、回退/越级/收口后再动都被拒，且每次被拒都进指纹。
fn gate_walk_ok() -> bool {
    let mut g = Gate::new();
    if g.at().is_some() || g.is_closed() || g.moves() != 0u32 || g.rejects() != 0u32 {
        return false;
    }
    if g.past_exit() {
        return false;
    }
    // 首步越闸：直接跳 Text 必须被拒，且拒过之后仍可正确起步
    if g.advance(Stage::Text).is_ok() {
        return false;
    }
    if g.rejects() != 1u32 || g.moves() != 0u32 || g.at().is_some() {
        return false;
    }
    let h_rejected = g.fold(0u64);
    let mut fresh = Gate::new();
    if fresh.fold(0u64) == h_rejected {
        return false;
    }
    if fresh.advance(Stage::Build).is_err() {
        return false;
    }
    // 停留闸查询
    if fresh.at() != Some(Stage::Build) || fresh.moves() != 1u32 {
        return false;
    }
    // 回退即死
    if fresh.advance(Stage::Build).is_ok() {
        return false;
    }
    // 越级即死
    if fresh.advance(Stage::Seal).is_ok() {
        return false;
    }
    // 未走完不许收口
    if fresh.close().is_ok() {
        return false;
    }
    if fresh.rejects() != 3u32 {
        return false;
    }
    // 沿链正走：Table 起逐道推进到 Seal
    let mut w = Gate::new();
    let mut i = 0usize;
    while i < GATE_COUNT {
        let s = match STAGE_CHAIN.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if w.advance(s).is_err() {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    if w.past_exit() != true || w.full_walk() {
        return false;
    }
    if w.close().is_err() || !w.full_walk() {
        return false;
    }
    // 收口后再推进、再收口都拒
    if w.advance(Stage::Seal).is_ok() || w.close().is_ok() {
        return false;
    }
    // walk_to：停在 Text 的现场推不到 Seal
    let mut p = Gate::new();
    if p.walk_to(Stage::Text).is_err() || p.at() != Some(Stage::Text) || p.moves() != 5u32 {
        return false;
    }
    if p.walk_to(Stage::Seal).is_err() {
        return false;
    }
    let mut q = Gate::new();
    if q.walk().is_err() || !q.full_walk() {
        return false;
    }
    // 每槽实测停留闸都能被 walk_to 复现——停留闸是走出来的，不是抄来的
    let mut k = 0usize;
    while k < CORPUS {
        let o = match outcome_at(k) {
            Some(x) => x,
            None => return false,
        };
        let mut t = Gate::new();
        if o.ok {
            if t.walk().is_err() {
                return false;
            }
        } else if t.walk_to(o.stage).is_err() || t.at() != Some(o.stage) {
            return false;
        }
        k = k.saturating_add(1usize);
    }
    if q.render().is_empty() {
        return false;
    }
    true
}

/// 账本自洽：满账可封存、重放同根、插队与偷改都被抓、缺陷码逐格对齐。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.len() != SAMPLES || l.is_empty() || l.rejects() != 0u32 {
        return false;
    }
    if l.audit().is_err() {
        return false;
    }
    if l.ok_count() != ACCEPT as u32 || l.err_count() != DEFECT_CELLS as u32 {
        return false;
    }
    let mut i = 0usize;
    while i < DEFECT_CELLS {
        let want = match REJECT_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let got = match l.sample(ACCEPT + i) {
            Some(s) => s.code,
            None => return false,
        };
        if want != got {
            return false;
        }
        if !s_flag(i, &l) {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    if l.defect_codes() != REJECT_CODES {
        return false;
    }
    if l.root().is_some() || l.replay().is_ok() {
        return false;
    }
    let mut m1 = build_ledger();
    let root = match m1.seal() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root == 0u64 || m1.root().is_none() || m1.replay().is_err() {
        return false;
    }
    let mut m2 = build_ledger();
    let f = match m2.freeze() {
        Ok(v) => v,
        Err(_) => 0u64,
    };
    if f != root || m2.replay().is_err() {
        return false;
    }
    // 冻结后登记一律拒；未登记的格不许被偷改
    if m2.register(Sample::stat(SAMPLES, 0usize, 1u32)).is_ok() {
        return false;
    }
    if m2.rejects() != 1u32 {
        return false;
    }
    let mut raw = Ledger::new();
    if raw.poke(0usize, 1u32).is_ok() {
        return false;
    }
    // 插队：slot 不等于当前长度即拒，且拒次数进指纹
    let mut ins = Ledger::new();
    if ins.register(Sample::stat(3usize, 0usize, VALS.get(0usize).copied().unwrap_or(0u32))).is_ok()
    {
        return false;
    }
    if ins.rejects() != 1u32 || ins.len() != 0usize {
        return false;
    }
    if ins.freeze().is_ok() {
        return false;
    }
    // 半账不许封存
    let mut half = Ledger::new();
    if half.register(Sample::stat(0usize, 0usize, BENCH as u32)).is_err() {
        return false;
    }
    if half.seal().is_ok() {
        return false;
    }
    // 偷改：重放必炸
    let mut poked = build_ledger();
    if poked.freeze().is_err() {
        return false;
    }
    if poked.poke(1usize, 999u32).is_err() {
        return false;
    }
    if poked.replay().is_ok() || poked.audit().is_ok() {
        return false;
    }
    // blank 样本永远登不上（slot 故意越界）
    if Ledger::new().register(Sample::blank()).is_ok() {
        return false;
    }
    if Sample::blank().op_in_range() || Sample::blank().op_name() != "void" {
        return false;
    }
    if !Sample::stat(0usize, 0usize, 1u32).is_stat()
        || !Sample::defect(ACCEPT, 0usize, TR_ESTATE).is_defect()
        || Sample::red_rate(7usize, RED_RATE).is_defect()
    {
        return false;
    }
    if l.render().is_empty() || !l.render().as_str().contains("trim-ledger") {
        return false;
    }
    true
}

/// 缺陷格的操作号是否落在缺陷面（把 `sample` 的可空性挡在调用点外）。
fn s_flag(i: usize, l: &Ledger) -> bool {
    match l.sample(ACCEPT + i) {
        Some(s) => s.is_defect() && s.op == OP_DEFECT_BASE + i as u8 && !s.ok && s.value == 0u32,
        None => false,
    }
}

/// 四路注入全部被拦。
fn inject_ok() -> bool {
    let v = inject_four();
    if v.len() != 4usize {
        return false;
    }
    let mut i = 0usize;
    while i < 4usize {
        match v.get(i) {
            Some(true) => {}
            _ => return false,
        }
        i = i.saturating_add(1usize);
    }
    true
}

/// 逐槽明细（诊断面）。
pub fn detail() -> String {
    let mut s = String::from("trim-known-miss slots=\n");
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => break,
        };
        let o = c.actual();
        s.push_str("  #");
        s.push_str(dec_text(i as u32).as_str());
        s.push_str(" ");
        s.push_str(c.shape.tag);
        s.push_str(" card ");
        s.push_str(dec_text(c.shape.card as u32).as_str());
        s.push_str(" tier ");
        s.push_str(o.tier().slug());
        s.push_str(" stage ");
        s.push_str(o.stage.slug());
        s.push_str(" code ");
        s.push_str(hex_text(o.code, 5usize).as_str());
        s.push_str(" value ");
        s.push_str(dec_text(o.value).as_str());
        s.push_str(" note ");
        s.push_str(o.note.slug());
        s.push_str(" want ");
        s.push_str(expected_fault(i).slug());
        s.push_str(" len ");
        s.push_str(dec_text(o.len).as_str());
        s.push('\n');
        i = i.saturating_add(1usize);
    }
    s
}

/// 聚合诊断面：签发单 + 横计矩阵 + 闸链 + 账本，四张一次给出。
///
/// 这四张必须同时给：只给签发单，复核者无法知道哪条被 skip；只给矩阵，看不出
/// 每条停在哪个闸；只给账本，看不出通过率。判据原文要求"绿"，但绿的可复核性
/// 靠的就是这四张互相对得上。
pub fn diagnostic() -> String {
    let r = run();
    let mut s = r.render();
    s.push('\n');
    s.push_str(r.matrix.render().as_str());
    let mut g = Gate::new();
    if g.walk().is_ok() {
        s.push_str(g.render().as_str());
    } else {
        s.push_str("gate walk rejected");
    }
    s.push('\n');
    s.push_str(build_ledger().render().as_str());
    s.push_str(detail().as_str());
    s
}

/// 本模块的判据入口：十三项。
///
/// 十三项的顺序就是本模块的闸次序：先立身份（段基址/码表/闸链），再立枚举面
/// （形态/流/注记/skip/载体/档位），再立清单（19 行逐行 + 统计交叉核），
/// 然后走推演、对判、横计、签发，最后核闸链单调性、账本与四路注入。
/// 顺序不可调：清单没立住就去核签发面，核的是自己编的分母；账本没核就跑注入，
/// 注入的是没封口的账。
pub fn run_unx11663_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11663_trimmed_applet_text");
    s.add(
        "TR01-名称与段基址",
        code_table_ok(),
        "SEG_BASE=0x6d120 起 13 条码连续互斥，且每条都有码名、none 与 unknown 各归其位",
    );
    s.add(
        "TR02-闸链与签名码",
        chain_ok(),
        "八闸 +1 递增闭合、链表与 next() 双向一致、每闸专属码互异且全在码表内",
    );
    s.add(
        "TR03-形态与逐字长度",
        form_ok(),
        "七形态 rank 回查闭合、合法面与泄漏面互斥、三正三反档逐字长度公式与构造子一致",
    );
    s.add(
        "TR04-流/注记/skip/载体/档位",
        stream_ok_enums() && note_ok() && skip_ok() && harness_ok() && tier_ok(),
        "五流十七注记七成因七载体四档各自闭合，skip 成因到注记只有 AbsentCarrier 一条路",
    );
    s.add(
        "TR05-清单19行",
        manifest_rows_ok(),
        "19 行逐行自洽、卡号 11661..11669 有序、短名唯一带卡号、前置逐行成链、本卡前置可查",
    );
    s.add(
        "TR06-统计与常量交叉核",
        vals_match_ok(),
        "VALS 七格由清单与枚举累加重算、红档率 929 来历自证、档形期望四数和为 14",
    );
    s.add(
        "TR07-语料绑定",
        corpus_binding_ok(),
        "14 槽占 14 张不同卡、断言数逐字抄自清单、活槽 8 死槽 6 且六缺陷维各用一次",
    );
    s.add(
        "TR08-八闸推演逐槽",
        define_ok(),
        "14 槽推演可重复、绿槽必停封印闸且数值等于断言数、红槽签名码与该闸一致",
    );
    s.add(
        "TR09-六维对判",
        judge_ok(),
        "死槽恰坏声称那一维、申报由实测重建必全对、双维同坏报先撞那维、六码互异",
    );
    s.add(
        "TR10-横计矩阵",
        matrix_ok(),
        "Row/Matrix 算术闭合与谎行可查、真语料四档齐全条条对齐、分布 10/1/1/2",
    );
    s.add(
        "TR11-签发面与逐字锚",
        report_ok(),
        "十三闸全过、render 逐字等锚、七种篡改面各被一码拦下、折叠面对改动敏感",
    );
    s.add(
        "TR12-闸链单调性",
        gate_walk_ok(),
        "首步越闸/回退/越级/未走满收口全被拒且进指纹、每槽停留闸可由 walk_to 复现",
    );
    s.add(
        "TR13-账本与四路注入",
        ledger_ok() && inject_ok(),
        "满账封存重放同根、插队与偷改皆现形、表路判路渲染路账本路四路注入全被拦",
    );
    s
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------
//
// 这些测试不是给判据"再刷一遍绿"的：判据面（`run_unx11663_checks`）已经把十九行
// 清单、十四槽推演、六维对判、四档横计、十三码签发各自核过一遍，测试若只是重新
// 调用同一个谓词，等于同义反复。这里每个测试都钉住**一个具体数字或一条具体行为**，
// 并且尽量给出反例——改错一个常量、倒转一条闸序、把一个泄漏档放行进绿集，
// 都必须在这里立刻红。
#[cfg(test)]
mod tests {
    use super::*;

    /// 取一槽事实面。槽号由 `SHAPES` 固定；越界是测试自己写错，直接炸出来。
    fn sh(i: usize) -> Shape {
        match shape_at(i) {
            Some(x) => x,
            None => panic!("shape slot out of range"),
        }
    }

    /// 取一槽推演结果（`define()` 的现成产物）。
    fn oc(i: usize) -> Outcome {
        match outcome_at(i) {
            Some(x) => x,
            None => panic!("outcome slot out of range"),
        }
    }

    /// 取一槽申报面。
    fn cl(i: usize) -> Claim {
        match claim_at(i) {
            Some(x) => x,
            None => panic!("claim slot out of range"),
        }
    }

    /// 取一清单行。
    fn sp(i: usize) -> Spec {
        match spec_at(i) {
            Some(x) => *x,
            None => panic!("spec row out of range"),
        }
    }

    /// 取码表一项。
    fn cd(i: usize) -> u32 {
        match CODE_TABLE.get(i) {
            Some(x) => *x,
            None => panic!("code table index out of range"),
        }
    }

    /// 主锚槽——绝大多数单维测试从它复制。
    fn base() -> Shape {
        sh(0usize)
    }

    /// 改完形态后把逐字长度重算回去。
    ///
    /// 文案闸先查模板合法性、再查字节数；测试若只想验模板，必须让字节数跟着走，
    /// 否则停在同一个闸却是另一个成因，断言就验错了东西。
    fn relen(s: Shape) -> Shape {
        let mut o = s;
        o.log_bytes = o.text().len() as u32;
        o
    }

    /// 逐字长度只由形态与名字决定，公式独立重算一遍——用来验文案构造子没被改胖。
    fn want_len(form: Form, name: &str, cand: &str) -> usize {
        match form {
            Form::Modern => 29usize + name.len(),
            Form::Legacy => 25usize,
            Form::Stub => 28usize + name.len(),
            Form::DisabledHint => 30usize + name.len(),
            Form::CandidateHint => 46usize + name.len() + cand.len(),
            Form::ConfigHint => 64usize + 2usize * name.len(),
            Form::Blank | Form::Unknown => 0usize,
        }
    }

    #[test]
    fn seg_base_and_thirteen_codes() {
        assert_eq!(SEG_BASE, 0x6d120u32);
        assert_eq!(TR_ESTATE, SEG_BASE + 0x01u32);
        assert_eq!(TR_ECODE, SEG_BASE + 0x03u32);
        assert_eq!(TR_ECOUNT, SEG_BASE + 0x07u32);
        assert_eq!(TR_ESKIP, SEG_BASE + 0x0au32);
        assert_eq!(TR_ESLOT, SEG_BASE + 0x0du32);
        assert_eq!(CODE_TABLE.len(), 13usize);
        // 十三码互不相同：同批 F11660 有三门共用 KA_ESLOT，签发面的错误分布因此失真。
        let mut i = 0usize;
        while i < CODE_TABLE.len() {
            let mut j = i.saturating_add(1usize);
            while j < CODE_TABLE.len() {
                assert_ne!(cd(i), cd(j), "码表第 {} 与第 {} 条撞码", i, j);
                j = j.saturating_add(1usize);
            }
            // 每码有名，且名不是兜底的 unknown / none
            assert_ne!(code_name(cd(i)), "unknown");
            assert_ne!(code_name(cd(i)), "none");
            i = i.saturating_add(1usize);
        }
        assert_eq!(code_name(0u32), "none");
        assert_eq!(code_name(SEG_BASE + 0x30u32), "unknown");
        assert_eq!(code_name(TR_ESTATE), "tr-estate");
        assert_eq!(code_name(TR_ESLOT), "tr-eslot");
    }

    #[test]
    fn contract_sizes_are_pinned() {
        assert_eq!(CORPUS, 14usize);
        assert_eq!(SAMPLES, 14usize);
        assert_eq!(ACCEPT, 8usize);
        assert_eq!(REJECT, 6usize);
        assert_eq!(TIER_COUNT, 4usize);
        assert_eq!(BENCH, 19usize);
        assert_eq!(GATE_COUNT, 8usize);
        assert_eq!(STAT_CELLS, 7usize);
        assert_eq!(DEFECT_CELLS, 6usize);
        assert_eq!(CARD, 11663u16);
        assert_eq!(CO_SIGN_CARD, 11595u16);
        assert_eq!(CARD_MIN, 11661u16);
        assert_eq!(CARD_MAX, 11679u16);
        assert_eq!(RATE_FULL, 1000u32);
        // 红档率是"14 条里 1 条不中"的千分位，四舍五入靠 total/2 做偏移
        assert_eq!(RED_RATE, 929u32);
        assert_eq!(
            (13u32.saturating_mul(RATE_FULL).saturating_add(7u32)) / 14u32,
            RED_RATE
        );
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        assert_eq!(CLAIM_BROKEN.len(), REJECT);
        assert_eq!(STAGE_CHAIN.len(), GATE_COUNT);
        // 缺陷码就是六个对判维各自的码，两表必须逐位相等
        let mut k = 0usize;
        while k < REJECT {
            assert_eq!(
                match REJECT_CODES.get(k) {
                    Some(x) => *x,
                    None => 0u32,
                },
                match REJECT_FAULTS.get(k) {
                    Some(x) => x.code(),
                    None => 0u32,
                }
            );
            k = k.saturating_add(1usize);
        }
    }

    #[test]
    fn stage_chain_is_eight_monotonic_gates() {
        assert_eq!(Stage::COUNT, 8u8);
        assert_eq!(Stage::Build.rank(), 1u8);
        assert_eq!(Stage::Seal.rank(), 8u8);
        // 链表与 next() 双向一致
        let mut i = 0usize;
        while i < GATE_COUNT {
            let s = match STAGE_CHAIN.get(i) {
                Some(x) => *x,
                None => panic!("chain row"),
            };
            assert_eq!(s.rank(), (i + 1usize) as u8);
            assert_eq!(s.slug().len() > 0usize, true);
            if i + 1usize < GATE_COUNT {
                let nx = match STAGE_CHAIN.get(i + 1usize) {
                    Some(x) => *x,
                    None => panic!("chain row"),
                };
                assert_eq!(s.next(), Some(nx));
            } else {
                assert_eq!(s.next(), None, "封印闸必须是链尾");
            }
            i = i.saturating_add(1usize);
        }
        assert_eq!(Stage::from_rank(0u8), None);
        assert_eq!(Stage::from_rank(9u8), None);
        // 执行面之后的四道闸才算"抵达过现场"
        assert_eq!(Stage::Build.past_exit(), false);
        assert_eq!(Stage::Stream.past_exit(), false);
        assert_eq!(Stage::Text.past_exit(), false);
        assert_eq!(Stage::Exit.past_exit(), true);
        assert_eq!(Stage::Leak.past_exit(), true);
        assert_eq!(Stage::Seal.past_exit(), true);
    }

    #[test]
    fn each_stage_has_its_own_fault_code() {
        assert_eq!(Stage::Build.fault_code(), TR_ECOUNT);
        assert_eq!(Stage::Table.fault_code(), TR_ECYCLE);
        assert_eq!(Stage::Identity.fault_code(), TR_EUNIQ);
        assert_eq!(Stage::Stream.fault_code(), TR_ESKIP);
        assert_eq!(Stage::Text.fault_code(), TR_EVALUE);
        assert_eq!(Stage::Exit.fault_code(), TR_ECODE);
        assert_eq!(Stage::Leak.fault_code(), TR_ENOTE);
        assert_eq!(Stage::Seal.fault_code(), TR_EBLOCK);
        // 八道闸的专属码互不相同——同批 F11660 的教训：撞码的错误分布读不出成因
        let mut a = 0usize;
        while a < GATE_COUNT {
            let sa = match Stage::from_rank((a + 1usize) as u8) {
                Some(x) => x,
                None => panic!("stage rank"),
            };
            let mut b = a.saturating_add(1usize);
            while b < GATE_COUNT {
                let sb = match Stage::from_rank((b + 1usize) as u8) {
                    Some(x) => x,
                    None => panic!("stage rank"),
                };
                assert_ne!(sa.fault_code(), sb.fault_code());
                b = b.saturating_add(1usize);
            }
            a = a.saturating_add(1usize);
        }
    }

    #[test]
    fn form_sets_are_mutually_exclusive() {
        assert_eq!(Form::COUNT, 7u8);
        assert_eq!(Form::Unknown.rank(), 0u8);
        assert_eq!(Form::Modern.rank(), 1u8);
        assert_eq!(Form::Blank.rank(), 7u8);
        assert_eq!(Form::from_rank(8u8), None);
        let good = [Form::Modern, Form::Legacy, Form::Stub];
        let leak = [Form::DisabledHint, Form::CandidateHint, Form::ConfigHint];
        let f = base();
        // 三正档：合法、不泄漏、各自有名（旧版档是唯一不带名字的合法档）
        for x in good.iter() {
            let m = match *x {
                Form::Modern => Form::Modern,
                Form::Legacy => Form::Legacy,
                Form::Stub => Form::Stub,
                _ => Form::Unknown,
            };
            assert_eq!(m.is_valid_notfound(), true);
            assert_eq!(m.is_leak(), false);
            assert_eq!(m.weight() > 0u32, true);
            assert_eq!(m.slug() == f.form.slug(), m == Form::Modern);
        }
        // 三反档：泄漏、不合法、权重最高（它们在文案面上"最像对的"）
        for m in leak.iter() {
            assert_eq!(m.is_leak(), true, "{:?} 必须是泄漏档", m);
            assert_eq!(m.is_valid_notfound(), false);
            assert_eq!(m.weight(), 5u32);
            assert_eq!(m.carries_name(), true);
        }
        // Blank 与 Unknown 两边都不属于——把它们并进任何一边都会让闸口失去区分度
        assert_eq!(Form::Blank.is_leak(), false);
        assert_eq!(Form::Blank.is_valid_notfound(), false);
        assert_eq!(Form::Blank.carries_name(), false);
        assert_eq!(Form::Unknown.is_leak(), false);
        assert_eq!(Form::Unknown.is_valid_notfound(), false);
        assert_eq!(Form::Unknown.weight(), 0u32);
        assert_eq!(Form::Legacy.carries_name(), false);
        assert_eq!(Form::Modern.carries_name(), true);
        assert_eq!(Form::Stub.carries_name(), true);
        assert_eq!(Form::Modern.weight(), 3u32);
        assert_eq!(Form::Legacy.weight(), 2u32);
        assert_eq!(Form::Stub.weight(), 2u32);
        assert_eq!(Form::Blank.weight(), 4u32);
    }

    #[test]
    fn text_builders_are_verbatim_and_sized() {
        // 主锚：现代档，名字进洞，逐字与 busybox 现行输出同形
        assert_eq!(
            text_modern("fc").as_str(),
            "busybox: 'fc': applet not found"
        );
        assert_eq!(text_modern("fc").len(), 31usize);
        // 旧版档根本没有名字洞：被裁名与未知名在这一档逐字相同，这正是同文案的极端形
        assert_eq!(text_legacy().as_str(), "busybox: applet not found");
        assert_eq!(text_legacy().len(), 25usize);
        // 桩体档（CONFIG_FEATURE_NO_APPLET）把名字放在另一侧
        assert_eq!(
            text_stub("udhcpc").as_str(),
            "busybox: applet 'udhcpc' not found"
        );
        assert_eq!(text_stub("udhcpc").len(), 34usize);
        // 反面档：本卡把它们当**反面**构造，用于判死，不用于放行
        assert_eq!(
            text_disabled("fc").contains(FORBIDDEN_DISABLED),
            true
        );
        assert_eq!(text_didyou("fc", "cls").contains(FORBIDDEN_DIDYOU), true);
        assert_eq!(
            text_config_hint("fc").contains(FORBIDDEN_CONFIG),
            true
        );
        assert_eq!(text_empty().len(), 0usize);
        // 长度公式逐个复核（改一个分隔符就会让文案闸的字节数校验整体错位）
        let names = ["fc", "dmesg", "ftpd", "nosuchapplet"];
        for n in names.iter() {
            let nm = match *n {
                "fc" => "fc",
                "dmesg" => "dmesg",
                "ftpd" => "ftpd",
                _ => "nosuchapplet",
            };
            assert_eq!(text_modern(nm).len(), want_len(Form::Modern, nm, ""));
            assert_eq!(text_stub(nm).len(), want_len(Form::Stub, nm, ""));
            assert_eq!(
                text_disabled(nm).len(),
                want_len(Form::DisabledHint, nm, "")
            );
            assert_eq!(
                text_config_hint(nm).len(),
                want_len(Form::ConfigHint, nm, "")
            );
        }
        assert_eq!(
            text_didyou("fc", "cls").len(),
            want_len(Form::CandidateHint, "fc", "cls")
        );
    }

    #[test]
    fn form_render_dispatches_to_the_same_builders() {
        let n = "nosuchapplet";
        assert_eq!(
            Form::Modern.render(n, "").as_str(),
            text_modern(n).as_str()
        );
        assert_eq!(Form::Legacy.render(n, "").as_str(), text_legacy().as_str());
        assert_eq!(Form::Stub.render(n, "").as_str(), text_stub(n).as_str());
        assert_eq!(
            Form::DisabledHint.render(n, "").as_str(),
            text_disabled(n).as_str()
        );
        assert_eq!(
            Form::CandidateHint.render(n, "cls").as_str(),
            text_didyou(n, "cls").as_str()
        );
        assert_eq!(
            Form::ConfigHint.render(n, "").as_str(),
            text_config_hint(n).as_str()
        );
        // 未定与空答复都不印名字——空答复是本卡最怕的一种"看着像绿"
        assert_eq!(Form::Unknown.render(n, "").len(), 0usize);
        assert_eq!(Form::Blank.render(n, "").len(), 0usize);
        // 同文案基准：两族名字换进同一个模板，除名字洞外逐字相同
        let a = text_modern("fc");
        let b = text_modern("nosuchapplet");
        assert_eq!(
            a.as_str().starts_with("busybox: '"),
            true
        );
        assert_eq!(b.ends_with(": applet not found"), a.ends_with(": applet not found"));
        assert_eq!(
            a.as_str().split("': ").nth(1),
            b.as_str().split("': ").nth(1)
        );
    }

    #[test]
    fn stream_flags_match_the_ktest_truth_table() {
        assert_eq!(Stream::COUNT, 5u8);
        assert_eq!(Stream::Unknown.rank(), 0u8);
        assert_eq!(Stream::Stderr.rank(), 1u8);
        assert_eq!(Stream::None.rank(), 5u8);
        // 只有 stderr 可读——stdout 上的 notfound 是裁剪件把 bb_error_msg 换成 printf 的实测形态
        assert_eq!(Stream::Stderr.readable(), true);
        assert_eq!(Stream::Stdout.readable(), false);
        assert_eq!(Stream::Both.readable(), false);
        assert_eq!(Stream::Split.readable(), false);
        assert_eq!(Stream::None.readable(), false);
        assert_eq!(Stream::Unknown.readable(), false);
        // 污染 stdout 与饿死 stderr 不是同一件事：Both 两头都坏，Stderr 两头都好
        assert_eq!(Stream::Stderr.pollutes_stdout(), false);
        assert_eq!(Stream::Stderr.starves_stderr(), false);
        assert_eq!(Stream::Stdout.pollutes_stdout(), true);
        assert_eq!(Stream::Stdout.starves_stderr(), true);
        assert_eq!(Stream::Both.pollutes_stdout(), true);
        assert_eq!(Stream::Both.starves_stderr(), false);
        assert_eq!(Stream::Split.pollutes_stdout(), true);
        assert_eq!(Stream::Split.starves_stderr(), false);
        assert_eq!(Stream::None.pollutes_stdout(), false);
        assert_eq!(Stream::None.starves_stderr(), true);
    }

    #[test]
    fn note_ranks_facts_and_tap_bits() {
        assert_eq!(Note::COUNT, 17u8);
        assert_eq!(Note::None.rank(), 0u8);
        assert_eq!(Note::Clean.rank(), 1u8);
        assert_eq!(Note::AssertShort.rank(), 17u8);
        // 注记面有 rank 0，链首必须回查到 None（与形态/流/载体/档位四个枚举不同）
        assert_eq!(Note::from_rank(0u8), Some(Note::None));
        assert_eq!(Note::from_rank(18u8), None);
        assert_eq!(Note::None.slug(), "");
        assert_eq!(Note::None.is_fact(), false);
        assert_eq!(Note::Clean.is_fact(), false);
        assert_eq!(Note::Clean.eats_bit(), true);
        // skip 是唯一不吃 TAP 位的红注记：它挂 `# skip` 指令，不改 ok 计数
        assert_eq!(Note::AbsentCarrier.eats_bit(), false);
        assert_eq!(Note::AbsentCarrier.is_fact(), true);
        assert_eq!(Note::FlakeRecovered.slug(), "flake");
        assert_eq!(Note::DepBlocked.slug(), "blocked");
        assert_eq!(Note::TrimLeak.slug(), "leak");
        assert_eq!(Note::CandidateList.slug(), "didyou");
        // 抖绿≠干净、缺席≠前置未达、泄漏≠候选清单、污染≠饿死
        assert_ne!(Note::FlakeRecovered, Note::Clean);
        assert_ne!(Note::DepBlocked, Note::AbsentCarrier);
        assert_ne!(Note::TrimLeak, Note::CandidateList);
        assert_ne!(Note::StdoutPolluted, Note::StderrStarved);
    }

    #[test]
    fn skip_causes_map_to_one_note_only() {
        assert_eq!(Skip::COUNT, 7u8);
        assert_eq!(Skip::None.is_skip(), false);
        assert_eq!(Skip::None.note(), Note::None);
        assert_eq!(Skip::None.keeps_ok_line(), false);
        // 七条成因全部归到同一个注记：诚实 skip 在 TAP 面上只有一种写法
        let causes = [
            Skip::NoAppletCfg,
            Skip::NoAsh,
            Skip::NoStatic,
            Skip::NoStub,
            Skip::NoTwoMachine,
            Skip::NoTrimMatrix,
            Skip::NoCarrier,
        ];
        for c in causes.iter() {
            let m = match *c {
                Skip::NoAppletCfg => Skip::NoAppletCfg,
                Skip::NoAsh => Skip::NoAsh,
                Skip::NoStatic => Skip::NoStatic,
                Skip::NoStub => Skip::NoStub,
                Skip::NoTwoMachine => Skip::NoTwoMachine,
                Skip::NoTrimMatrix => Skip::NoTrimMatrix,
                _ => Skip::NoCarrier,
            };
            assert_eq!(m.is_skip(), true);
            assert_eq!(m.keeps_ok_line(), true);
            assert_eq!(m.note(), Note::AbsentCarrier);
            assert_eq!(m.slug().len() > 0usize, true);
        }
        // 成因 → 吃掉的位
        assert_eq!(Skip::None.consumed_bit(), CFG_NONE);
        assert_eq!(Skip::NoCarrier.consumed_bit(), CFG_NONE);
        assert_eq!(Skip::NoAppletCfg.consumed_bit(), CFG_APPLET);
        assert_eq!(Skip::NoAsh.consumed_bit(), CFG_ASH);
        assert_eq!(Skip::NoStatic.consumed_bit(), CFG_STATIC);
        assert_eq!(Skip::NoStub.consumed_bit(), CFG_NOAPPLET);
        assert_eq!(Skip::NoTwoMachine.consumed_bit(), CFG_TWO_MACHINE);
        assert_eq!(Skip::NoTrimMatrix.consumed_bit(), CFG_TRIM_MATRIX);
    }

    #[test]
    fn derive_skip_priority_and_carrier_unreachability() {
        // 满配置推不出任何成因；空配置也不——空掩码是"没缺"，不是"缺全部"
        assert_eq!(derive_skip(CFG_NONE), Skip::None);
        assert_eq!(derive_skip(CFG_ALL), Skip::NoTrimMatrix);
        // 单缺位逐个对应自己的成因
        assert_eq!(derive_skip(CFG_APPLET), Skip::NoAppletCfg);
        assert_eq!(derive_skip(CFG_ASH), Skip::NoAsh);
        assert_eq!(derive_skip(CFG_STATIC), Skip::NoStatic);
        assert_eq!(derive_skip(CFG_NOAPPLET), Skip::NoStub);
        assert_eq!(derive_skip(CFG_TWO_MACHINE), Skip::NoTwoMachine);
        assert_eq!(derive_skip(CFG_TRIM_MATRIX), Skip::NoTrimMatrix);
        // 与无关位同缺时，仍归到那一条：USAGE/I18N/LONG_OPTS/VERBOSE 不造成 skip
        assert_eq!(derive_skip(CFG_USAGE | CFG_APPLET), Skip::NoAppletCfg);
        assert_eq!(
            derive_skip(CFG_LONG_OPTS | CFG_VERBOSE_USAGE | CFG_I18N),
            Skip::None
        );
        // 优先级：矩阵 > 双机 > applet > 桩体 > 静态 > ash
        assert_eq!(derive_skip(CFG_TRIM_MATRIX | CFG_APPLET), Skip::NoTrimMatrix);
        assert_eq!(
            derive_skip(CFG_TWO_MACHINE | CFG_ASH),
            Skip::NoTwoMachine
        );
        assert_eq!(derive_skip(CFG_APPLET | CFG_NOAPPLET), Skip::NoAppletCfg);
        assert_eq!(derive_skip(CFG_NOAPPLET | CFG_STATIC), Skip::NoStub);
        assert_eq!(derive_skip(CFG_STATIC | CFG_ASH), Skip::NoStatic);
        // NoCarrier 永远不可能由掩码推出：它说的是"没有载体"，不是"少了某个 CONFIG"
        let masks = [
            CFG_NONE,
            CFG_ALL,
            CFG_APPLET,
            CFG_ASH,
            CFG_STATIC,
            CFG_NOAPPLET,
            CFG_USAGE,
            CFG_VERBOSE_USAGE,
            CFG_TWO_MACHINE,
            CFG_TRIM_MATRIX,
            CFG_I18N,
            CFG_LONG_OPTS,
            CFG_APPLET | CFG_USAGE,
            CFG_ASH | CFG_VERBOSE_USAGE,
        ];
        for m in masks.iter() {
            assert_ne!(derive_skip(*m), Skip::NoCarrier, "掩码 {:010b} 推出了载体成因", m);
        }
    }

    #[test]
    fn harness_bands_and_default_membership() {
        assert_eq!(Harness::COUNT, 7u8);
        assert_eq!(Harness::DirectCall.rank(), 1u8);
        assert_eq!(Harness::Noop.rank(), 7u8);
        assert_eq!(Harness::from_rank(0u8), None);
        assert_eq!(Harness::from_rank(8u8), None);
        assert_eq!(Harness::DirectCall.ms_band(), 15u32);
        assert_eq!(Harness::SymlinkCall.ms_band(), 25u32);
        assert_eq!(Harness::ListProbe.ms_band(), 40u32);
        assert_eq!(Harness::TwoMachine.ms_band(), 120u32);
        assert_eq!(Harness::TrimMatrix.ms_band(), 900u32);
        assert_eq!(Harness::TextDiff.ms_band(), 60u32);
        assert_eq!(Harness::Noop.ms_band(), 0u32);
        // 需要第二份构建的两档绝不进默认套件：默认集里塞"换台机器才能绿"的行
        // 等于给必跑集的绿打折，而那个绿是本批对外唯一的承诺
        assert_eq!(Harness::TwoMachine.needs_second(), true);
        assert_eq!(Harness::TrimMatrix.needs_second(), true);
        assert_eq!(Harness::TwoMachine.default_member(), false);
        assert_eq!(Harness::TrimMatrix.default_member(), false);
        assert_eq!(Harness::Noop.default_member(), false);
        assert_eq!(Harness::DirectCall.default_member(), true);
        let second = [Harness::TwoMachine, Harness::TrimMatrix];
        for h in second.iter() {
            let m = match *h {
                Harness::TwoMachine => Harness::TwoMachine,
                _ => Harness::TrimMatrix,
            };
            assert_eq!(m.needs_second() && m.default_member(), false);
        }
        // Noop 是唯一不产出文案的载体
        assert_eq!(Harness::Noop.yields_text(), false);
        assert_eq!(Harness::TextDiff.yields_text(), true);
        assert_eq!(Harness::ListProbe.yields_text(), true);
    }

    #[test]
    fn tiers_split_green_from_halted() {
        assert_eq!(Tier::Green.rank(), 1u8);
        assert_eq!(Tier::Blocked.rank(), 4u8);
        assert_eq!(Tier::from_rank(0u8), None);
        assert_eq!(Tier::from_rank(5u8), None);
        // 抖动档进绿集是**有意**的：它和一次绿在成败面上同真。代价是必须单独记档，
        // 否则"跑三次总有一次绿"的构建会伪装成 100% 一把过。
        assert_eq!(Tier::Green.in_green(), true);
        assert_eq!(Tier::Flaked.in_green(), true);
        assert_eq!(Tier::Skipped.in_green(), false);
        assert_eq!(Tier::Blocked.in_green(), false);
        assert_eq!(Tier::Green.is_halted(), false);
        assert_eq!(Tier::Flaked.is_halted(), false);
        assert_eq!(Tier::Skipped.is_halted(), true);
        assert_eq!(Tier::Blocked.is_halted(), true);
        assert_eq!(Tier::Green.weight(), 1u32);
        assert_eq!(Tier::Flaked.weight(), 2u32);
        assert_eq!(Tier::Skipped.weight(), 3u32);
        assert_eq!(Tier::Blocked.weight(), 4u32);
        assert_eq!(Tier::Green.slug(), "green");
        assert_eq!(Tier::Blocked.slug(), "blocked");
    }

    #[test]
    fn manifest_rows_are_nineteen_ordered_and_totalled() {
        assert_eq!(SPECS.len(), BENCH);
        assert_eq!(bench_assert_total(), WANT_ASSERTS);
        assert_eq!(WANT_ASSERTS, 105u32);
        assert_eq!(bench_rows_total(), 6280u32);
        assert_eq!(bench_dep_total(), 18u32);
        assert_eq!(bench_default_total(), 15u32);
        let mut prev_card = 0u16;
        let mut i = 0usize;
        while i < BENCH {
            let r = sp(i);
            assert_eq!(r.card, (CARD_MIN + i as u16), "第 {} 行卡号不连续", i);
            assert_eq!(r.card > prev_card, true);
            if i == 0usize {
                assert_eq!(r.dep, 0u16, "首批第一行没有前置");
                assert_eq!(r.need_cfg, CFG_NONE, "首行只查 argv[0]，与裁剪位无关");
            } else {
                assert_eq!(r.dep, prev_card, "第 {} 行的前置必须是上一行", i);
            }
            // 只有本卡自己必须申报 applet 缺席位
            if r.card == CARD {
                assert_ne!(r.need_cfg & CFG_APPLET, 0u32, "本卡必须带 applet 位");
            }
            assert_eq!(r.tag.len() > 0usize, true);
            assert_eq!(r.title.len() > 0usize, true);
            assert_eq!(r.asserts > 0u16, true);
            assert_eq!(r.rows_min >= 320u16 && r.rows_min <= 600u16, true);
            assert_eq!(r.card >= CARD_MIN && r.card <= CARD_MAX, true);
            // 需要第二份构建的载体绝不申报默认套件
            assert_eq!(
                r.default_suite && r.harness.needs_second(),
                false,
                "卡 {} 把双机/矩阵档塞进了默认集",
                r.card
            );
            prev_card = r.card;
            i = i.saturating_add(1usize);
        }
        // 卡号 ↔ 行号双向可查
        assert_eq!(spec_index_of(CARD), Some(2usize));
        assert_eq!(spec_index_of(11600u16), None);
        match spec_of_card(11679u16) {
            Some(x) => assert_eq!(x.asserts, 7u16),
            None => panic!("末行查不到"),
        }
        assert_eq!(spec_of_card(12000u16).is_some(), false);
    }

    #[test]
    fn spec_row_gate_accepts_real_and_rejects_fake() {
        // 真清单 19 行逐行必须合格（这也是 TR05 的第一半）
        let mut i = 0usize;
        while i < BENCH {
            assert_eq!(spec_row_ok(&sp(i)), true, "第 {} 行不合格", i);
            i = i.saturating_add(1usize);
        }
        assert_eq!(manifest_rows_ok(), true);
        // 四张假清单：卡号越界 / 断言为零 / 前置越界 / 位数越界
        let real = sp(0usize);
        let mut fake = real;
        fake.card = 0u16;
        assert_eq!(spec_row_ok(&fake), false);
        let mut fake2 = real;
        fake2.asserts = 0u16;
        assert_eq!(spec_row_ok(&fake2), false);
        let mut fake3 = real;
        fake3.dep = CARD_MAX + 1u16;
        assert_eq!(spec_row_ok(&fake3), false);
        let mut fake4 = real;
        fake4.need_cfg = 1u32 << 20;
        assert_eq!(spec_row_ok(&fake4), false);
        let mut fake5 = real;
        fake5.tag = "";
        assert_eq!(spec_row_ok(&fake5), false);
        let mut fake6 = real;
        fake6.rows_min = 60u16;
        assert_eq!(spec_row_ok(&fake6), false);
        let mut fake7 = real;
        fake7.harness = Harness::TwoMachine;
        fake7.default_suite = true;
        assert_eq!(spec_row_ok(&fake7), false);
    }

    #[test]
    fn tag_matches_card_only_for_its_own_card() {
        assert_eq!(tag_matches_card("f11663-trim-known-miss", 11663u16), true);
        assert_eq!(tag_matches_card("f11674-text-verbatim", 11663u16), false);
        assert_eq!(tag_matches_card("", 11663u16), false);
        assert_eq!(tag_matches_card("11663-trim", 11663u16), false);
        assert_eq!(tag_matches_card("f-abc", 11663u16), false);
        // 数字后面必须紧跟连字符：`f116630-x` 不是卡 11663 的短名
        assert_eq!(tag_matches_card("f116630-x", 11663u16), false);
        // 连字符在短名里合法（同批 F11660 因拒连字符让 19 行集体判死）
        let mut i = 0usize;
        while i < BENCH {
            let r = sp(i);
            assert_eq!(tag_matches_card(r.tag, r.card), true, "卡 {} 短名对不上", r.card);
            i = i.saturating_add(1usize);
        }
    }

    /// 探针实测到的前八槽档位——判据的地基，动语料必先动这张表。
    fn want_tier(i: usize) -> Tier {
        match i {
            0usize => Tier::Green,
            1usize => Tier::Green,
            2usize => Tier::Flaked,
            3usize => Tier::Green,
            4usize => Tier::Skipped,
            5usize => Tier::Blocked,
            6usize => Tier::Green,
            _ => Tier::Blocked,
        }
    }

    /// 取统计格期望值（越界返零，测试里也不许裸下标）。
    fn val(i: usize) -> u32 {
        match VALS.get(i) {
            Some(x) => *x,
            None => 0u32,
        }
    }

    /// 按 `build_ledger` 的次序手工建账，`delta` 把统计面数值整体改胖。
    fn ledger_from(delta: u32) -> Ledger {
        let mut l = Ledger::new();
        let mut i = 0usize;
        while i < ACCEPT {
            let v = val(i).saturating_add(delta);
            let s = if i + 1usize == ACCEPT {
                Sample::red_rate(i, v)
            } else {
                Sample::stat(i, i, v)
            };
            if l.register(s).is_err() {
                break;
            }
            i = i.saturating_add(1usize);
        }
        let mut j = 0usize;
        while j < DEFECT_CELLS {
            let c = match REJECT_CODES.get(j) {
                Some(x) => *x,
                None => 0u32,
            };
            if l.register(Sample::defect(ACCEPT + j, j, c)).is_err() {
                break;
            }
            j = j.saturating_add(1usize);
        }
        l
    }

    #[test]
    fn fold_helpers_obey_the_fnv_recurrence() {
        // 逐字节折叠必须等价于一次字符串折叠——指纹可重放靠这条立足。
        let a = fold_byte(fold_byte(fold_byte(FNV_OFF, b'a'), b'b'), b'c');
        assert_eq!(fold_str(FNV_OFF, "abc"), a);
        assert_eq!(fold_str(FNV_OFF, ""), FNV_OFF);
        assert_eq!(fold_u32(FNV_OFF, 7u32), fold_word(FNV_OFF, 7u64));
        assert_ne!(fold_u32(FNV_OFF, 1u32), fold_u32(FNV_OFF, 2u32));
        assert_ne!(fold_u32(FNV_OFF, 7u32), fold_u32(FNV_PRIME, 7u32));
        // 高低半字交换必然改指纹，否则"同一数值任何字序同值"是空话
        assert_ne!(
            fold_word(FNV_OFF, 0x0000_0000_1234_5678u64),
            fold_word(FNV_OFF, 0x1234_5678_0000_0000u64)
        );
        // 文本子边界：零、四位、u32 上界，定宽补零与小写
        assert_eq!(dec_text(0u32), "0");
        assert_eq!(dec_text(6280u32), "6280");
        assert_eq!(dec_text(4294967295u32), "4294967295");
        assert_eq!(hex_text(0x6d120u32, 5usize), "6d120");
        assert_eq!(hex_text(0x0fu32, 5usize), "0000f");
        assert_eq!(u32_text(929u32), "929");
        assert_eq!(i32_text(-1i32), "-1");
        assert_eq!(i32_text(0i32), "0");
        assert_eq!(bool_text(true), "ok");
        assert_eq!(bool_text(false), "not ok");
    }

    #[test]
    fn cfg_names_and_first_absent_follow_the_mask() {
        // 掩码是"缺席位"：置 1 表示缺，故 first_absent_name 取最低置位。
        assert_eq!(cfg_name(CFG_APPLET), "applet");
        assert_eq!(cfg_name(CFG_NONE), "");
        assert_eq!(first_absent_name(CFG_NONE), "");
        assert_eq!(first_absent_name(CFG_APPLET | CFG_ASH), "applet");
        assert_eq!(first_absent_name(CFG_ASH), "ash");
        assert_eq!(first_absent_name(CFG_ALL), "applet");
        let mut i = 0usize;
        while i < 10usize {
            let bit = 1u32 << (i as u32);
            assert_eq!(first_absent_name(bit).len() > 0usize, true, "第 {} 位无名", i);
            i = i.saturating_add(1usize);
        }
        // 七条 skip 成因各消费一位，None 不消费
        assert_eq!(Skip::None.consumed_bit(), CFG_NONE);
        assert_eq!(Skip::NoAppletCfg.consumed_bit(), CFG_APPLET);
        assert_eq!(Skip::NoAsh.consumed_bit(), CFG_ASH);
        assert_eq!(Skip::NoStatic.consumed_bit(), CFG_STATIC);
        assert_eq!(Skip::NoStub.consumed_bit(), CFG_NOAPPLET);
        assert_eq!(Skip::NoTwoMachine.consumed_bit(), CFG_TWO_MACHINE);
        assert_eq!(Skip::NoTrimMatrix.consumed_bit(), CFG_TRIM_MATRIX);
    }

    #[test]
    fn eight_live_slots_land_where_the_probe_saw_them() {
        // 前八槽档位：绿 4 / 抖 1 / 跳 1 / 阻 2（逐字对探针明细）。
        let mut i = 0usize;
        while i < ACCEPT {
            let o = define(&sh(i));
            assert_eq!(o.tier(), want_tier(i), "槽 {} 档位不对", i);
            if o.tier() == Tier::Flaked {
                assert_eq!(o.ok, true);
                assert_eq!(o.note, Note::FlakeRecovered);
            }
            if o.tier() == Tier::Green {
                assert_eq!(o.ok, true);
                assert_eq!(o.stage, Stage::Seal);
                assert_eq!(o.code, 0u32);
            }
            if o.tier() == Tier::Skipped {
                assert_eq!(o.ok, false);
                assert_eq!(o.stage, Stage::Build);
                assert_eq!(o.note, Note::AbsentCarrier);
                assert_eq!(o.default, false, "没跑成的条不许占默认位");
            }
            if o.tier() == Tier::Blocked {
                assert_eq!(o.ok, false);
                assert_eq!(o.stage == Stage::Seal, false, "阻档不许停在封印闸");
                assert_eq!(o.concluded() || o.stage.rank() < Stage::Exit.rank(), true);
            }
            i = i.saturating_add(1usize);
        }
        // 后六槽是"真绿现场 + 坏一维申报"，事实面必须全部走满
        let mut j = ACCEPT;
        while j < CORPUS {
            let o = define(&sh(j));
            assert_eq!(o.ok, true, "死槽 {} 的事实面必须本征为绿", j);
            assert_eq!(o.stage, Stage::Seal);
            assert_eq!(o.code, 0u32);
            assert_eq!(expected_fault(j) == Fault::Ok, false);
            j = j.saturating_add(1usize);
        }
    }

    #[test]
    fn corpus_claims_match_their_declared_faults() {
        assert_eq!(cases().len(), CORPUS);
        let mut i = 0usize;
        while i < CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => panic!("槽 {} 缺失", i),
            };
            // 申报对实测的缺陷维必须逐槽等于期望维（TR09 的独立重放）
            assert_eq!(c.fault(), expected_fault(i), "槽 {} 的缺陷维与期望不符", i);
            assert_eq!(judge(&c), expected_fault(i));
            // 由实测反推的诚实申报必然无缺陷——否则期望维表是凭空写的
            let o = c.actual();
            let honest = Claim::from_outcome(&o);
            assert_eq!(judge_against(&honest, &o), Fault::Ok, "槽 {} 的实测自相矛盾", i);
            assert_eq!(o.verdict(), bool_text(o.ok));
            i = i.saturating_add(1usize);
        }
        assert_eq!(live_count(), ACCEPT);
        assert_eq!(covered_spec_count(), CORPUS, "14 槽应各自绑到一张卡");
        assert_eq!(covered_spec_count() <= BENCH, true);
        // 六个死槽坏的必须是六个互不相同的维
        let mut a = ACCEPT;
        while a < CORPUS {
            let mut b = a.saturating_add(1usize);
            while b < CORPUS {
                assert_ne!(expected_fault(a), expected_fault(b), "死槽的期望维撞了");
                b = b.saturating_add(1usize);
            }
            assert_ne!(expected_fault(a), Fault::Ok);
            a = a.saturating_add(1usize);
        }
    }

    #[test]
    fn each_defect_slot_breaks_exactly_its_declared_dim() {
        let mut k = 0usize;
        while k < REJECT {
            let want = match CLAIM_BROKEN.get(k) {
                Some(f) => *f,
                None => Fault::Ok,
            };
            let j = ACCEPT + k;
            let o = define(&sh(j));
            assert_eq!(judge_against(&cl(j), &o), want, "槽 {} 坏的应是 {:?}", j, want);
            // 反推：把诚实申报按同一维打断，应得到同一结论
            let hacked = Claim::from_outcome(&o).break_dim(want);
            assert_eq!(judge_against(&hacked, &o), want);
            // 每个维的码与账本缺陷码同源
            assert_eq!(want.code(), cd(k));
            assert_eq!(want.estate(), want == Fault::Estate);
            assert_eq!(want.slug() == "ok", false);
            k = k.saturating_add(1usize);
        }
        assert_eq!(REJECT_FAULTS.len(), REJECT);
        assert_eq!(REJECT_CODES.len(), REJECT);
        let mut a = 0usize;
        while a < REJECT {
            let mut b = a.saturating_add(1usize);
            while b < REJECT {
                assert_ne!(cd(a), cd(b));
                b = b.saturating_add(1usize);
            }
            a = a.saturating_add(1usize);
        }
    }

    #[test]
    fn judge_orders_dims_estate_first_default_last() {
        let o = define(&base());
        let honest = Claim::from_outcome(&o);
        assert_eq!(judge_against(&honest, &o), Fault::Ok);
        // 六维同坏：只可能看到 Estate
        let mut one = honest;
        one.success = !honest.success;
        one.code = honest.code.saturating_add(3u32);
        one.value = honest.value.saturating_add(3u32);
        one.note = Note::TableConflict;
        one.default = !honest.default;
        assert_eq!(judge_against(&one, &o), Fault::Estate);
        // 逐维复位，结论依次后退到下一维——这条就是判序本身
        let mut two = one;
        two.success = honest.success;
        assert_eq!(judge_against(&two, &o), Fault::Stage);
        let mut three = two;
        three.stage = honest.stage;
        assert_eq!(judge_against(&three, &o), Fault::Code);
        let mut four = three;
        four.code = honest.code;
        assert_eq!(judge_against(&four, &o), Fault::Value);
        let mut five = four;
        five.value = honest.value;
        assert_eq!(judge_against(&five, &o), Fault::Note);
        let mut six = five;
        six.note = honest.note;
        assert_eq!(judge_against(&six, &o), Fault::Default);
        six.default = honest.default;
        assert_eq!(judge_against(&six, &o), Fault::Ok);
        // 维的分类面：账目维不改"跑没跑过"，文本维是本卡核心
        assert_eq!(Fault::Ok.code(), 0u32);
        assert_eq!(Fault::Ok.slug(), "ok");
        assert_eq!(Fault::Value.accounting(), true);
        assert_eq!(Fault::Default.accounting(), true);
        assert_eq!(Fault::Code.accounting(), false);
        assert_eq!(Fault::Value.textual(), true);
        assert_eq!(Fault::Note.textual(), true);
        assert_eq!(Fault::Stage.textual(), false);
        assert_eq!(Fault::Stage.slug(), "stage");
    }

    #[test]
    fn break_dim_always_leaves_an_observable_difference() {
        // 打断若不留痕，死槽就与活槽同指纹，注入探针形同虚设。
        let o = define(&base());
        let honest = Claim::from_outcome(&o);
        let mut i = 0usize;
        while i < REJECT_FAULTS.len() {
            let f = match REJECT_FAULTS.get(i) {
                Some(x) => *x,
                None => break,
            };
            let h0 = honest.fold(FNV_OFF);
            let h1 = honest.break_dim(f).fold(FNV_OFF);
            assert_ne!(h0, h1, "维 {:?} 打断后指纹不变", f);
            assert_ne!(judge_against(&honest.break_dim(f), &o), Fault::Ok);
            i = i.saturating_add(1usize);
        }
        // Fault::Ok 是"不打断"，必须原样返回
        assert_eq!(honest.break_dim(Fault::Ok).fold(FNV_OFF), honest.fold(FNV_OFF));
        // 封印闸已无下一闸，Stage 维打断只能退回链首——仍是可观测错位
        assert_eq!(Stage::Seal.next(), None);
        assert_eq!(honest.break_dim(Fault::Stage).stage, Stage::Build);
        // 注记维打断必须换档而不是清空
        let n1 = honest.break_dim(Fault::Note).note;
        assert_eq!(n1 == honest.note, false);
    }

    #[test]
    fn build_gate_stops_at_carrier_and_band_failures() {
        // 载体零档：一切从构建闸起判，且不占默认位
        let z = Shape::zero();
        let oz = define(&z);
        assert_eq!(oz.stage, Stage::Build);
        assert_eq!(oz.code, TR_ESKIP);
        assert_eq!(oz.note, Note::AbsentCarrier);
        assert_eq!(oz.value, Skip::NoCarrier.rank() as u32);
        assert_eq!(oz.tier(), Tier::Skipped);
        assert_eq!(oz.concluded(), false);
        // 没跑过：成因回落到掩码推导——本卡现场裁了 applet，故落在 NoAppletCfg
        let mut nr = base();
        nr.ran = false;
        let onr = define(&nr);
        assert_eq!(onr.stage, Stage::Build);
        assert_eq!(onr.code, TR_ESKIP);
        assert_eq!(onr.value, Skip::NoAppletCfg.rank() as u32);
        assert_eq!(onr.tier(), Tier::Skipped);
        // 耗时出带是"跑挂了"而不是"环境没有"，故 TR_ESTATE 且归阻档
        let mut slow = base();
        slow.ms = 9_000_000u32;
        let os = define(&slow);
        assert_eq!(os.stage, Stage::Build);
        assert_eq!(os.code, TR_ESTATE);
        assert_eq!(os.value, slow.ms);
        assert_eq!(os.note, Note::StderrStarved);
        assert_eq!(os.tier(), Tier::Blocked);
        // 带容差：零带只认 ms 恰为零，其余按 ±半带 / 四倍带
        assert_eq!(Harness::Noop.ms_band(), 0u32);
        assert_eq!(Harness::DirectCall.ms_band(), 15u32);
        assert_eq!(base().ms_ok(), true);
        let mut quick = base();
        quick.ms = 1u32;
        assert_eq!(quick.ms_ok(), false);
        assert_eq!(define(&quick).stage, Stage::Build, "低于半带必须停在构建闸");
    }

    #[test]
    fn build_gate_reports_the_mask_contradiction() {
        // 裁没裁必须与 applet 缺席位同口气
        let mut lie = base();
        lie.trimmed = false;
        let o = define(&lie);
        assert_eq!(o.stage, Stage::Build);
        assert_eq!(o.code, Stage::Build.fault_code());
        assert_eq!(o.note, Note::TableConflict);
        assert_eq!(o.value, lie.cfg_absent, "构建闸的数值面申报的就是那份掩码");
        // 表项数为零同样停在构建闸
        let mut empty = base();
        empty.table_items = 0u16;
        assert_eq!(define(&empty).note, Note::TableConflict);
        // 在册位与"没裁 + 本来在册"不符：形态错配
        let mut ghost = base();
        ghost.in_table = true;
        assert_eq!(define(&ghost).note, Note::FormMismatch);
        // 前提不成立（申报被裁的名字全量里根本没有）是合法 skip，不是红
        let mut prem = base();
        prem.full_known = false;
        let op = define(&prem);
        assert_eq!(op.stage, Stage::Build);
        assert_eq!(op.code, TR_ESKIP);
        assert_eq!(op.value, Skip::NoAppletCfg.rank() as u32);
        assert_eq!(op.tier(), Tier::Skipped);
    }

    #[test]
    fn table_gate_checks_shrink_direction_and_co_signature_anchor() {
        // 裁掉一位，本构建表就必须比全量少一项
        let mut grow = base();
        grow.table_items = grow.full_items;
        let o = define(&grow);
        assert_eq!(o.stage, Stage::Table);
        assert_eq!(o.code, TR_ECYCLE);
        assert_eq!(o.note, Note::TableConflict);
        assert_eq!(o.value, grow.table_items as u32);
        // 联签锚项数不符：F11595 说的是另一次裁剪
        let mut drift = base();
        drift.anchor_items = drift.anchor_items.saturating_add(1u16);
        let od = define(&drift);
        assert_eq!(od.stage, Stage::Table);
        assert_eq!(od.note, Note::CrossSign);
        // 联签锚指纹不符
        let mut hash = base();
        hash.anchor_hash = hash.anchor_hash.saturating_add(1u32);
        let oh = define(&hash);
        assert_eq!(oh.stage, Stage::Table);
        assert_eq!(oh.note, Note::CrossSign);
        // 未裁族（对照行）要求项数相等，而不是"少一项"
        let mut keep = base();
        keep.trimmed = false;
        keep.cfg_absent = keep.cfg_absent & !CFG_APPLET;
        keep.in_table = true;
        keep.table_items = keep.full_items.saturating_add(1u16);
        assert_eq!(define(&keep).note, Note::TableConflict);
    }

    #[test]
    fn identity_gate_splits_four_dims_with_a_bitmap() {
        let mut offcard = base();
        offcard.card = 12000u16;
        let o1 = define(&offcard);
        assert_eq!(o1.stage, Stage::Identity);
        assert_eq!(o1.code, TR_EUNIQ);
        assert_eq!(o1.note, Note::BadIdentity);
        assert_eq!(o1.value, 0x1u32);
        let mut badtag = base();
        badtag.tag = "has space";
        assert_eq!(define(&badtag).value, 0x2u32);
        let mut badname = base();
        badname.name = "a/b";
        assert_eq!(define(&badname).value, 0x4u32);
        let mut dup = base();
        dup.dup = true;
        assert_eq!(define(&dup).value, 0x8u32);
        // 四维同坏必须四位同置，且仍停在同一道闸
        let mut all = base();
        all.card = 12000u16;
        all.tag = "";
        all.name = "";
        all.dup = true;
        let oa = define(&all);
        assert_eq!(oa.stage, Stage::Identity);
        assert_eq!(oa.value, 0xFu32);
        assert_eq!(oa.tier(), Tier::Blocked);
        // 号段边界本身要在册：批首批末都算，越界才算坏
        let mut head = base();
        head.card = CARD_MIN;
        assert_eq!(head.identity_bits() & 0x1u32, 0u32);
        let mut tail = base();
        tail.card = CARD_MAX;
        assert_eq!(tail.identity_bits() & 0x1u32, 0u32);
        let mut over = base();
        over.card = CARD_MAX + 1u16;
        assert_eq!(over.identity_bits() & 0x1u32, 0x1u32);
    }

    #[test]
    fn stream_gate_reads_only_the_stderr_face() {
        // 抵达面只认 stderr 独占：污染 stdout 或饿死 stderr 都算没送到比对器
        let cases = [
            (Stream::Stdout, Note::StdoutPolluted),
            (Stream::Both, Note::StdoutPolluted),
            (Stream::Split, Note::StdoutPolluted),
            (Stream::None, Note::StderrStarved),
            (Stream::Unknown, Note::BlankReply),
        ];
        let mut i = 0usize;
        while i < cases.len() {
            let (rank, want_note) = match cases.get(i) {
                Some(x) => *x,
                None => break,
            };
            let mut s = base();
            s.stream = rank;
            let o = define(&s);
            assert_eq!(o.stage, Stage::Stream);
            assert_eq!(o.code, TR_ESKIP, "送达失败沿用跳过码：本闸不产新码");
            assert_eq!(o.note, want_note);
            assert_eq!(o.value, rank.rank() as u32);
            i = i.saturating_add(1usize);
        }
        // 载体不产文本必须先于流面判定——那是环境问题，不是答复问题
        let mut noop = base();
        noop.harness = Harness::Noop;
        noop.stream = Stream::Stderr;
        assert_eq!(define(&noop).stage, Stage::Build);
        assert_eq!(Harness::Noop.yields_text(), false);
        assert_eq!(Stream::Stderr.readable(), true);
    }

    #[test]
    fn text_gate_is_the_verbatim_face_of_this_card() {
        // 三条泄漏形态都停在文案闸：泄漏不等于文案错，但同形闸本来就拦得住它
        let mut dis = base();
        dis.form = Form::DisabledHint;
        let od = define(&relen(dis));
        assert_eq!(od.stage, Stage::Text);
        assert_eq!(od.code, TR_EVALUE);
        assert_eq!(od.note, Note::TrimLeak);
        let mut cand = base();
        cand.form = Form::CandidateHint;
        cand.cand = "ls";
        let oc = define(&relen(cand));
        assert_eq!(oc.stage, Stage::Text);
        assert_eq!(oc.note, Note::CandidateList);
        let mut hint = base();
        hint.form = Form::ConfigHint;
        assert_eq!(define(&relen(hint)).note, Note::TrimLeak);
        // 空答复与未定形态归到"没说话"
        let mut blank = base();
        blank.form = Form::Blank;
        let ob = define(&relen(blank));
        assert_eq!(ob.stage, Stage::Text);
        assert_eq!(ob.note, Note::BlankReply);
        assert_eq!(ob.value, 0u32, "空答复的数值面就是零字节");
        let mut unk = base();
        unk.form = Form::Unknown;
        assert_eq!(define(&relen(unk)).note, Note::BlankReply);
        // 字节数申报与实际成形文案不符：模板对、账目错
        let mut short = base();
        short.log_bytes = short.log_bytes.saturating_add(1u32);
        let os = define(&short);
        assert_eq!(os.stage, Stage::Text);
        assert_eq!(os.note, Note::BytesShort);
        assert_eq!(os.value, short.log_bytes);
        // 带名档的名字洞：换名后模板与字节数一起自洽，应走满
        let mut hole = base();
        hole.name = "anotherapplet";
        let oh = define(&relen(hole));
        assert_eq!(oh.stage, Stage::Seal);
        assert_eq!(oh.value, hole.asserts as u32);
        // 旧版档不带名字：两族答复逐字相同，天然同形
        let mut legacy = base();
        legacy.form = Form::Legacy;
        let ol = define(&relen(legacy));
        assert_eq!(ol.stage, Stage::Seal);
        assert_eq!(ol.len, 25u32);
        // 桩体档带名字，且不带任何禁用词
        let stub = Form::Stub.render("foo", "");
        assert_eq!(stub.as_str().contains("foo"), true);
        assert_eq!(stub.as_str().contains(FORBIDDEN_DIDYOU), false);
        assert_eq!(stub.as_str().contains(FORBIDDEN_CONFIG), false);
        // 同文案的正面形式：被裁名与未知名得到同一模板
        assert_eq!(base().baseline_text(), text_modern(base().name));
    }

    #[test]
    fn exit_and_leak_gates_keep_their_own_attributions() {
        // 码档漂移：127 是对外承诺，别的码都是换了承诺
        let mut ex = base();
        ex.exit_code = EX_GENERAL;
        let oe = define(&ex);
        assert_eq!(oe.stage, Stage::Exit);
        assert_eq!(oe.code, TR_ECODE);
        assert_eq!(oe.note, Note::CodeDrift);
        assert_eq!(oe.value, EX_GENERAL as u32);
        assert_eq!(oe.concluded(), true);
        assert_eq!(Stage::Exit.past_exit(), true);
        // errno 残留：码对了但带着 notfound 不该有的 errno
        let mut en = base();
        en.errno_bit = ENOENT;
        let on = define(&en);
        assert_eq!(on.stage, Stage::Exit);
        assert_eq!(on.note, Note::ErrnoDrift);
        // 泄漏闸只管洗绿：文案泄漏早在文案闸判死了
        let mut wash = base();
        wash.washed = true;
        let ow = define(&wash);
        assert_eq!(ow.stage, Stage::Leak);
        assert_eq!(ow.code, TR_EFLAKE, "洗绿用抖动码，与注记一一对应");
        assert_eq!(ow.note, Note::FlakeRecovered);
        assert_eq!(ow.value, wash.attempts as u32);
        assert_eq!(ow.tier(), Tier::Blocked);
        // 重试过但没申报首次即中：这是诚实的抖，不是洗
        let mut flip = base();
        flip.first_ok = false;
        flip.attempts = 3u8;
        let of = define(&flip);
        assert_eq!(of.stage, Stage::Seal, "没申报首次即中就不算洗绿");
        assert_eq!(of.note, Note::FlakeRecovered);
        assert_eq!(of.tier(), Tier::Flaked);
        // 申报首次即中却重试过：同一道闸的另一条归因路
        let mut cheat = base();
        cheat.attempts = 3u8;
        assert_eq!(define(&cheat).stage, Stage::Leak);
    }

    #[test]
    fn seal_gate_pairs_dep_block_with_assert_count() {
        // 前置未达成：停在封印闸，用阻码 + DepBlocked
        let mut dep = base();
        dep.dep_done = false;
        let o = define(&dep);
        assert_eq!(o.stage, Stage::Seal);
        assert_eq!(o.code, TR_EBLOCK);
        assert_eq!(o.note, Note::DepBlocked);
        assert_eq!(o.value, dep.dep as u32);
        assert_eq!(o.tier(), Tier::Blocked);
        // 断言没走满：同一道闸，但码与注记都必须换一套
        let mut short = base();
        short.pass_asserts = short.asserts.saturating_sub(1u16);
        let os = define(&short);
        assert_eq!(os.stage, Stage::Seal);
        assert_eq!(os.code, TR_ECOUNT);
        assert_eq!(os.note, Note::AssertShort);
        assert_eq!(os.value, short.pass_asserts as u32);
        // 断言数为零是"没写判据"，同样落在计数闸
        let mut none = base();
        none.asserts = 0u16;
        none.pass_asserts = 0u16;
        assert_eq!(define(&none).note, Note::AssertShort);
        // 无前置的现场不许被当成阻档
        let mut free = base();
        free.dep = 0u16;
        free.dep_done = false;
        assert_eq!(free.has_dep(), false);
        assert_eq!(define(&free).ok, true);
        // 绿档的默认位来自载体，而不是申报
        let b = base();
        let o2 = define(&b);
        assert_eq!(o2.default, b.default_expected());
        assert_eq!(o2.len, b.log_bytes);
        assert_eq!(o2.value, b.asserts as u32);
    }

    #[test]
    fn stage_code_allow_set_is_narrow_and_total() {
        // 每道闸只许签自己那族码：放宽一处就会让"闸位与码不配对"的报告过关。
        assert_eq!(stage_code_ok(Stage::Build, TR_ESKIP), true);
        assert_eq!(stage_code_ok(Stage::Build, TR_ESTATE), true);
        assert_eq!(stage_code_ok(Stage::Build, Stage::Build.fault_code()), true);
        assert_eq!(stage_code_ok(Stage::Build, TR_ECODE), false);
        assert_eq!(stage_code_ok(Stage::Table, TR_ECYCLE), true);
        assert_eq!(stage_code_ok(Stage::Table, TR_ESTATE), false);
        assert_eq!(stage_code_ok(Stage::Identity, TR_EUNIQ), true);
        assert_eq!(stage_code_ok(Stage::Stream, TR_ESKIP), true);
        assert_eq!(stage_code_ok(Stage::Text, TR_EVALUE), true);
        assert_eq!(stage_code_ok(Stage::Exit, TR_ECODE), true);
        assert_eq!(stage_code_ok(Stage::Leak, TR_ENOTE), true);
        assert_eq!(stage_code_ok(Stage::Leak, TR_EFLAKE), true);
        assert_eq!(stage_code_ok(Stage::Leak, TR_ESKIP), false);
        assert_eq!(stage_code_ok(Stage::Seal, TR_EBLOCK), true);
        assert_eq!(stage_code_ok(Stage::Seal, TR_ECOUNT), true);
        assert_eq!(stage_code_ok(Stage::Seal, TR_ECYCLE), false);
        // 语料 14 槽的红档逐条必须落在允许集内（TR08 的第二半）
        let mut i = 0usize;
        while i < CORPUS {
            let o = define(&sh(i));
            if o.ok {
                assert_eq!(o.code, 0u32);
                assert_eq!(o.stage, Stage::Seal);
                assert_eq!(o.note.is_fact(), true);
            } else {
                assert_eq!(
                    stage_code_ok(o.stage, o.code),
                    true,
                    "槽 {} 的码签在了不该签的闸上",
                    i
                );
                assert_eq!(o.note.is_fact(), true, "槽 {} 的归因没落在事实档", i);
                assert_eq!(o.default, false, "不成立的结局不许占默认位");
            }
            i = i.saturating_add(1usize);
        }
    }

    #[test]
    fn row_math_flags_impossible_arithmetic_only() {
        let r = Row::new(Tier::Green, 3u32, 3u32, 15u32, 2u32, 9u32);
        assert_eq!(r.lie(), false);
        assert_eq!(r.perfect(), true);
        assert_eq!(r.weighted(), 3u32);
        assert_eq!(r.seal_rate(), 666u32);
        assert_eq!(r.mean_value(), 5u32);
        // 分子大于分母、以及"有条数却一条没对齐"都是谎
        assert_eq!(Row::new(Tier::Green, 3u32, 4u32, 0u32, 0u32, 0u32).lie(), true);
        assert_eq!(Row::new(Tier::Blocked, 2u32, 0u32, 0u32, 0u32, 0u32).lie(), true);
        assert_eq!(Row::empty(Tier::Blocked).lie(), false);
        assert_eq!(Row::empty(Tier::Blocked).perfect(), false, "空档不算完全对齐");
        assert_eq!(Row::empty(Tier::Blocked).seal_rate(), 0u32, "空档不许除零");
        assert_eq!(Row::empty(Tier::Blocked).mean_value(), 0u32);
        assert_eq!(Row::empty(Tier::Blocked).weighted(), 0u32);
        // 档权决定加权代价：阻档一条顶绿档四条
        assert_eq!(Row::new(Tier::Blocked, 1u32, 1u32, 0u32, 0u32, 0u32).weighted(), 4u32);
        assert_eq!(Row::new(Tier::Flaked, 1u32, 1u32, 0u32, 0u32, 0u32).weighted(), 2u32);
        assert_eq!(Row::new(Tier::Skipped, 1u32, 1u32, 0u32, 0u32, 0u32).weighted(), 3u32);
        // 折进指纹必须逐列参与：改任一列都改指纹
        let h0 = r.fold(FNV_OFF);
        let mut rr = r;
        rr.value_sum = rr.value_sum.saturating_add(1u32);
        assert_ne!(rr.fold(FNV_OFF), h0);
        let mut rq = r;
        rq.sealed = rq.sealed.saturating_add(1u32);
        assert_ne!(rq.fold(FNV_OFF), h0);
        // 渲染面逐列可见
        let txt = r.render();
        assert_eq!(txt.contains("green n=3 aligned=3 value=15 sealed=2 spec=9"), true);
    }

    #[test]
    fn matrix_accumulates_and_detects_its_own_damage() {
        let m = Matrix::zero();
        assert_eq!(m.total(), 0u32);
        assert_eq!(m.aligned(), 0u32);
        assert_eq!(m.covered(), false);
        assert_eq!(m.perfect_rows(), 0u32);
        assert_eq!(m.any_lie(), false);
        assert_eq!(m.in_green(), 0u32);
        assert_eq!(m.halted(), 0u32);
        assert_eq!(m.sealed(), 0u32);
        assert_eq!(m.cost(), 0u32);
        // 逐档累加：四档各有条数才算覆盖
        let mut a = Matrix::zero();
        assert_eq!(a.bump(Tier::Green, true, 5u32, true, 5u32), Ok(()));
        assert_eq!(a.bump(Tier::Flaked, true, 4u32, true, 4u32), Ok(()));
        assert_eq!(a.bump(Tier::Skipped, true, 0u32, false, 0u32), Ok(()));
        assert_eq!(a.bump(Tier::Blocked, true, 1u32, false, 1u32), Ok(()));
        assert_eq!(a.total(), 4u32);
        assert_eq!(a.aligned(), 4u32);
        assert_eq!(a.covered(), true);
        assert_eq!(a.perfect_rows(), 4u32);
        assert_eq!(a.in_green(), 2u32);
        assert_eq!(a.halted(), 2u32);
        assert_eq!(a.sealed(), 2u32);
        assert_eq!(a.spec_total(), 10u32);
        assert_eq!(a.cost(), 10u32);
        assert_eq!(a.green_has_lie(), false);
        // 行档号与档位不符是矩阵自身坏了，不许静默丢一条
        let mut broken = Matrix {
            rows: [
                Row::empty(Tier::Blocked),
                Row::empty(Tier::Flaked),
                Row::empty(Tier::Skipped),
                Row::empty(Tier::Blocked),
            ],
        };
        assert_eq!(broken.bump(Tier::Green, true, 1u32, false, 1u32), Err(TR_ECYCLE));
        assert_eq!(broken.total(), 0u32);
        // 谎行只能被 any_lie 抓到，covered 抓不到
        let mut liar = Matrix::zero();
        let _ = liar.bump(Tier::Green, false, 1u32, false, 1u32);
        let _ = liar.bump(Tier::Flaked, true, 1u32, true, 1u32);
        let _ = liar.bump(Tier::Skipped, true, 1u32, false, 1u32);
        let _ = liar.bump(Tier::Blocked, true, 1u32, false, 1u32);
        assert_eq!(liar.covered(), true);
        assert_eq!(liar.any_lie(), true);
        assert_eq!(liar.green_has_lie(), true);
        assert_eq!(liar.perfect_rows(), 3u32);
        // 按行号取行与按档取行必须同源
        let x = match liar.row_at(0usize) {
            Some(r) => r,
            None => panic!("首行缺失"),
        };
        let y = match liar.row(Tier::Green) {
            Some(r) => r,
            None => panic!("绿行缺失"),
        };
        assert_eq!((x.total, x.aligned, x.spec_sum), (y.total, y.aligned, y.spec_sum));
        assert_eq!(x.tier, y.tier);
        assert_eq!(liar.row_at(9usize).is_none(), true);
        assert_eq!(liar.render().contains("green n=1 aligned=0"), true);
    }

    #[test]
    fn the_corpus_matrix_is_the_expected_four_face_aggregate() {
        let m = run().matrix;
        let g = match m.row(Tier::Green) {
            Some(x) => x,
            None => panic!("绿档行缺失"),
        };
        let f = match m.row(Tier::Flaked) {
            Some(x) => x,
            None => panic!("抖档行缺失"),
        };
        let s = match m.row(Tier::Skipped) {
            Some(x) => x,
            None => panic!("跳档行缺失"),
        };
        let b = match m.row(Tier::Blocked) {
            Some(x) => x,
            None => panic!("阻档行缺失"),
        };
        assert_eq!(
            (g.total, g.aligned, g.value_sum, g.sealed, g.spec_sum),
            (10u32, 10u32, 60u32, 10u32, 60u32)
        );
        assert_eq!(
            (f.total, f.aligned, f.value_sum, f.sealed, f.spec_sum),
            (1u32, 1u32, 5u32, 1u32, 5u32)
        );
        assert_eq!(
            (s.total, s.aligned, s.value_sum, s.sealed, s.spec_sum),
            (1u32, 1u32, 2u32, 0u32, 5u32)
        );
        assert_eq!(
            (b.total, b.aligned, b.value_sum, b.sealed, b.spec_sum),
            (2u32, 2u32, 3u32, 0u32, 12u32)
        );
        assert_eq!(m.total(), WANT_MATCHED);
        assert_eq!(m.aligned(), WANT_MATCHED);
        assert_eq!(m.in_green(), WANT_INGREEN);
        assert_eq!(m.halted(), WANT_HALTED);
        assert_eq!(m.sealed(), WANT_SEALED);
        assert_eq!(m.covered(), true);
        assert_eq!(m.any_lie(), false);
        assert_eq!(m.perfect_rows(), TIER_COUNT as u32);
        // 清单断言总数与"卡绑定子集"是两回事：矩阵只累加被 14 槽命中的行
        assert_eq!(m.spec_total(), 82u32);
        assert_eq!(m.spec_total() == WANT_ASSERTS, false);
        assert_eq!(bench_assert_total(), WANT_ASSERTS);
        // 折叠可重放：同矩阵两次折必等，改一格必不等
        let h = m.fold(FNV_OFF);
        assert_eq!(m.fold(FNV_OFF), h);
        let mut other = m;
        if let Some(cell) = other.rows.get_mut(0usize) {
            cell.value_sum = cell.value_sum.saturating_add(1u32);
        }
        assert_ne!(other.fold(FNV_OFF), h);
    }

    #[test]
    fn report_scalars_and_render_are_pinned_to_the_anchor() {
        let r = run();
        assert_eq!(r.total, WANT_MATCHED);
        assert_eq!(r.matched, WANT_MATCHED);
        assert_eq!(r.bench, BENCH as u32);
        assert_eq!(r.asserts, WANT_ASSERTS);
        assert_eq!(r.rate(), RATE_FULL);
        assert_eq!(r.green(), WANT_INGREEN);
        assert_eq!(r.sealed(), WANT_SEALED);
        assert_eq!(r.tier_green(), WANT_GREEN);
        assert_eq!(r.tier_flaked(), WANT_FLAKED);
        assert_eq!(r.tier_skipped(), WANT_SKIPPED);
        assert_eq!(r.tier_blocked(), WANT_BLOCKED);
        assert_eq!(r.flake_free(), false, "本卡有一档抖，不许谎称无抖");
        assert_eq!(r.cost(), r.matrix.cost());
        assert_eq!(r.gate(), Ok(()));
        assert_eq!(r.render(), Report::RENDER_OK);
        assert_eq!(run().render(), r.render(), "两次聚合的签发面必须逐字相同");
        // 空报告：一切判据都从它的分母闸开始崩
        let bad = Report::invalid();
        assert_eq!(bad.rate(), 0u32);
        assert_eq!(bad.gate(), Err(TR_ESTATE));
        assert_eq!(bad.green(), 0u32);
        assert_eq!(bad.cost(), 0u32);
        assert_eq!(bad.flake_free(), true);
        // 千分率的四舍五入偏移：半数对上的那条率是 500
        let mut half = r;
        half.matched = 7u32;
        half.total = 14u32;
        assert_eq!(half.rate(), 500u32);
        // 红档率锚：十四格里坏一格就是 929，本卡的 VALS 第 8 格记的就是它
        assert_eq!(RED_RATE, (13u32 * RATE_FULL + 14u32 / 2u32) / 14u32);
        assert_eq!(val(7usize), RED_RATE);
    }

    #[test]
    fn each_gate_in_the_report_has_its_own_code() {
        let r = run();
        // 分母三件
        let mut t1 = r;
        t1.total = t1.total.saturating_add(1u32);
        assert_eq!(t1.gate(), Err(TR_ESTATE));
        let mut t2 = r;
        t2.bench = 18u32;
        assert_eq!(t2.gate(), Err(TR_ESTAGE));
        let mut t3 = r;
        t3.asserts = t3.asserts.saturating_add(1u32);
        assert_eq!(t3.gate(), Err(TR_ECODE));
        // 分子两件
        let mut t4 = r;
        t4.matched = t4.matched.saturating_sub(1u32);
        assert_eq!(t4.gate(), Err(TR_EVALUE));
        // 矩阵被清空：四档不再各有条数
        let mut t5 = r;
        t5.matrix = Matrix::zero();
        assert_eq!(t5.gate(), Err(TR_EUNIQ));
        // 绿档谎行（分子大于分母）
        let mut t6 = r;
        if let Some(cell) = t6.matrix.rows.get_mut(0usize) {
            cell.aligned = cell.aligned.saturating_add(1u32);
        }
        assert_eq!(t6.gate(), Err(TR_ESKIP));
        // 绿档有一条没对齐：档形不再闭合
        let mut t7 = r;
        if let Some(cell) = t7.matrix.rows.get_mut(0usize) {
            cell.aligned = cell.aligned.saturating_sub(1u32);
        }
        assert_eq!(t7.gate(), Err(TR_ECYCLE));
        // 手工抬语料而不动标量：标量与矩阵各说各话
        let mut t8 = r;
        if let Some(cell) = t8.matrix.rows.get_mut(0usize) {
            cell.total = cell.total.saturating_add(1u32);
            cell.aligned = cell.aligned.saturating_add(1u32);
        }
        assert_eq!(t8.gate(), Err(TR_EFLAKE));
        // 走满条数被偷减
        let mut t9 = r;
        if let Some(cell) = t9.matrix.rows.get_mut(0usize) {
            cell.sealed = cell.sealed.saturating_sub(1u32);
        }
        assert_eq!(t9.gate(), Err(TR_EDEFAULT));
        // 抖档并成两条、阻档减成一条：档形错位先于档名
        let mut t10 = r;
        if let Some(cell) = t10.matrix.rows.get_mut(1usize) {
            cell.total = 2u32;
            cell.aligned = 1u32;
        }
        if let Some(cell) = t10.matrix.rows.get_mut(3usize) {
            cell.total = 1u32;
            cell.aligned = 1u32;
        }
        assert_eq!(t10.gate(), Err(TR_ECYCLE));
        // 四档仍条条对齐，只是分布挪了一条：落在抖档闸
        let mut t11 = r;
        if let Some(cell) = t11.matrix.rows.get_mut(1usize) {
            cell.total = 2u32;
            cell.aligned = 2u32;
        }
        if let Some(cell) = t11.matrix.rows.get_mut(3usize) {
            cell.total = 1u32;
            cell.aligned = 1u32;
        }
        assert_eq!(t11.gate(), Err(TR_EBLOCK));
        // 同样只挪跳/阻分布：落在槽位闸
        let mut t12 = r;
        if let Some(cell) = t12.matrix.rows.get_mut(2usize) {
            cell.total = 2u32;
            cell.aligned = 2u32;
        }
        if let Some(cell) = t12.matrix.rows.get_mut(3usize) {
            cell.total = 1u32;
            cell.aligned = 1u32;
        }
        assert_eq!(t12.gate(), Err(TR_ESLOT));
    }

    #[test]
    fn gate_chain_refuses_backwards_and_overreach() {
        let mut g = Gate::new();
        assert_eq!(g.at(), None);
        assert_eq!(g.is_closed(), false);
        // 起步只能落构建闸：从没核过掩码的报告不许直接进表闸
        assert_eq!(g.advance(Stage::Table), Err(TR_ESTAGE));
        assert_eq!(g.rejects(), 1u32);
        assert_eq!(g.moves(), 0u32);
        assert_eq!(g.advance(Stage::Build), Ok(()));
        // 越级即死——跳过泄漏闸正是本卡最常见的假绿
        assert_eq!(g.advance(Stage::Seal), Err(TR_ESTAGE));
        assert_eq!(g.rejects(), 2u32);
        assert_eq!(g.advance(Stage::Table), Ok(()));
        assert_eq!(g.moves(), 2u32);
        // 原地重复推进同闸也算越级
        assert_eq!(g.advance(Stage::Table), Err(TR_ESTAGE));
        // 拒过的账必须进指纹
        let mut clean = Gate::new();
        let _ = clean.advance(Stage::Build);
        let _ = clean.advance(Stage::Table);
        assert_ne!(g.fold(FNV_OFF), clean.fold(FNV_OFF));
        // 未走到封印闸不许收口
        assert_eq!(g.close(), Err(TR_ESTAGE));
        let mut h = Gate::new();
        assert_eq!(h.walk(), Ok(()));
        assert_eq!(h.moves(), GATE_COUNT as u32);
        assert_eq!(h.full_walk(), true);
        assert_eq!(h.is_closed(), true);
        assert_eq!(h.past_exit(), true);
        assert_eq!(h.at(), Some(Stage::Seal));
        // 收口后再推进/再收口都判死，且拒次继续累加
        assert_eq!(h.advance(Stage::Build), Err(TR_ESTATE));
        assert_eq!(h.close(), Err(TR_ESTATE));
        assert_eq!(h.rejects(), 2u32);
        assert_eq!(h.render(), "gate at=seal moves=8 rejects=2 closed=ok");
        assert_eq!(Gate::new().render(), "gate at=none moves=0 rejects=0 closed=not ok");
    }

    #[test]
    fn gate_walk_to_is_resumable_and_rejects_regression() {
        let mut g = Gate::new();
        // 停在文案闸的一条，链只能推到文案闸
        assert_eq!(g.walk_to(Stage::Text), Ok(()));
        assert_eq!(g.at(), Some(Stage::Text));
        assert_eq!(g.moves(), 5u32);
        assert_eq!(g.rejects(), 0u32);
        assert_eq!(g.past_exit(), false);
        // 重复查同一槽不算拒——否则复核者的重复动作会污染指纹
        assert_eq!(g.walk_to(Stage::Text), Ok(()));
        assert_eq!(g.moves(), 5u32);
        assert_eq!(g.rejects(), 0u32);
        // 接着已有进度继续推：这才是复核者的真实问法
        assert_eq!(g.walk_to(Stage::Seal), Ok(()));
        assert_eq!(g.moves(), 8u32);
        assert_eq!(g.at(), Some(Stage::Seal));
        assert_eq!(g.past_exit(), true);
        // 回退目标一律拒
        assert_eq!(g.walk_to(Stage::Build), Err(TR_ESTAGE));
        assert_eq!(g.rejects(), 1u32);
        // 停在码档闸的一条不可能到过封印闸
        let mut k = Gate::new();
        assert_eq!(k.walk_to(Stage::Exit), Ok(()));
        assert_eq!(k.moves(), 6u32);
        assert_eq!(k.walk_to(Stage::Exit), Ok(()));
        assert_eq!(k.is_closed(), false, "walk_to 不收口，收口是另一件事");
        // 逐槽交叉：每槽的链都能停在它的实测闸，且绿槽才推得到封印闸
        let mut i = 0usize;
        while i < CORPUS {
            let o = define(&sh(i));
            let mut gg = Gate::new();
            assert_eq!(gg.walk_to(o.stage), Ok(()), "槽 {} 推不到它的停留闸", i);
            assert_eq!(gg.at(), Some(o.stage));
            if o.ok {
                assert_eq!(gg.walk_to(Stage::Seal), Ok(()));
                assert_eq!(gg.past_exit(), true);
            } else {
                assert_eq!(o.stage == Stage::Seal, false, "不成立的结局不许停在封印闸");
                assert_eq!(gg.walk_to(Stage::Seal).is_ok(), true, "闸链能推满不等于这条绿");
                assert_eq!(gg.is_closed(), false);
            }
            i = i.saturating_add(1usize);
        }
    }

    #[test]
    fn ledger_register_priority_is_frozen_then_jump_then_overflow() {
        let mut l = Ledger::new();
        assert_eq!(l.len(), 0usize);
        assert_eq!(l.is_empty(), true);
        assert_eq!(l.rejects(), 0u32);
        assert_eq!(l.root(), None);
        // 插队：落格位不等于当前长度
        assert_eq!(l.register(Sample::stat(1usize, 1usize, 18u32)), Err(TR_ESLOT));
        assert_eq!(l.rejects(), 1u32);
        assert_eq!(l.len(), 0usize);
        // 空样本的 slot 故意越界，忘填也会被拒
        assert_eq!(l.register(Sample::blank()), Err(TR_ESLOT));
        // 正序登记才收
        assert_eq!(l.register(Sample::stat(0usize, 0usize, val(0usize))), Ok(()));
        assert_eq!(l.len(), 1usize);
        assert_eq!(l.is_empty(), false);
        assert_eq!(l.sample(0usize).map(|s| s.value), Some(val(0usize)));
        assert_eq!(l.sample(1usize).map(|s| s.ok), Some(false));
        assert_eq!(l.sample(99usize).is_none(), true);
        // 满账后再登记 = 溢出，同样判跳位
        let mut full = build_ledger();
        assert_eq!(full.len(), SAMPLES);
        assert_eq!(full.audit(), Ok(()));
        assert_eq!(full.register(Sample::stat(SAMPLES, 0usize, 1u32)), Err(TR_ESLOT));
        // 冻结排在最前：冻上之后再登记，报的是状态而不是跳位
        let mut fz = build_ledger();
        assert_eq!(fz.freeze().is_ok(), true);
        assert_eq!(fz.register(Sample::stat(SAMPLES, 0usize, 1u32)), Err(TR_ESTATE));
        // 一次干净的账本与手工重建的账本逐字相同
        assert_eq!(ledger_from(0u32).render(), build_ledger().render());
    }

    #[test]
    fn ledger_audit_seal_freeze_replay_and_poke() {
        let good = build_ledger();
        assert_eq!(good.ok_count(), ACCEPT as u32);
        assert_eq!(good.err_count(), REJECT as u32);
        assert_eq!(good.defect_codes(), REJECT_CODES);
        assert_eq!(good.rejects(), 0u32);
        // 红档率格必须在第 8 格，值是 929
        assert_eq!(good.sample(7usize).map(|s| s.op), Some(OP_RED_RATE));
        assert_eq!(good.sample(7usize).map(|s| s.value), Some(RED_RATE));
        assert_eq!(good.sample(0usize).map(|s| s.is_stat()), Some(true));
        assert_eq!(good.sample(9usize).map(|s| s.is_defect()), Some(true));
        assert_eq!(good.sample(9usize).map(|s| s.code), Some(cd(0usize)));
        assert_eq!(good.sample(9usize).map(|s| s.value), Some(0u32));
        // 未封存的账不许偷改，也不许重放
        let mut fresh = build_ledger();
        assert_eq!(fresh.poke(0usize, 1u32), Err(TR_ESTATE));
        assert_eq!(fresh.replay(), Err(TR_ESTATE));
        assert_eq!(fresh.root().is_none(), true);
        // 封存给出根，重放必须还原同一个根
        let root = match fresh.seal() {
            Ok(v) => v,
            Err(_) => panic!("满账封存应成立"),
        };
        assert_ne!(root, 0u64);
        assert_eq!(fresh.root(), Some(root));
        assert_eq!(fresh.replay(), Ok(root));
        // 偷改一格数值，重放立刻抓到
        assert_eq!(fresh.poke(2usize, val(2usize).saturating_add(1u32)), Ok(()));
        assert_eq!(fresh.replay(), Err(TR_EUNIQ));
        // 越界格不许改
        assert_eq!(fresh.poke(SAMPLES, 1u32), Err(TR_ESLOT));
        // 半路的账不许封存
        let mut part = Ledger::new();
        let _ = part.register(Sample::stat(0usize, 0usize, val(0usize)));
        assert_eq!(part.audit(), Err(TR_ECOUNT));
        assert_eq!(part.seal(), Err(TR_ECOUNT));
        // 统计格数值被整体改胖：核账先判数值维，封存因此判死
        let mut fat = ledger_from(1u32);
        assert_eq!(fat.audit(), Err(TR_EVALUE));
        assert_eq!(fat.seal(), Err(TR_EUNIQ));
        // 有拒次的账不许冻结——拒过的账不可能伪装成 pristine
        let mut dirty = build_ledger();
        let _ = dirty.register(Sample::blank());
        assert_eq!(dirty.rejects(), 1u32);
        assert_eq!(dirty.freeze(), Err(TR_ECOUNT));
        // 冻结后再封/再冻都判死，但重放仍守着冻结前的根
        let mut fz = build_ledger();
        let r2 = match fz.freeze() {
            Ok(v) => v,
            Err(_) => panic!("干净账应能冻结"),
        };
        assert_eq!(fz.is_frozen(), true);
        assert_eq!(fz.seal(), Err(TR_ESTATE));
        assert_eq!(fz.freeze(), Err(TR_ESTATE));
        assert_eq!(fz.replay(), Ok(r2));
        assert_eq!(fz.render().contains("frozen"), true);
        // 把缺陷格改成成立格是账本最容易的手脚：核账先看成败面
        let mut fake = Ledger::new();
        let mut n = 0usize;
        while n < ACCEPT {
            let v = val(n);
            let s = if n + 1usize == ACCEPT {
                Sample::red_rate(n, v)
            } else {
                Sample::stat(n, n, v)
            };
            assert_eq!(fake.register(s), Ok(()));
            n = n.saturating_add(1usize);
        }
        assert_eq!(fake.register(Sample::stat(ACCEPT, 0usize, 1u32)), Ok(()));
        assert_eq!(fake.audit(), Err(TR_ESTATE));
        assert_eq!(fake.seal(), Err(TR_EUNIQ));
    }

    #[test]
    fn ledger_render_carries_the_cell_by_cell_face() {
        let txt = build_ledger().render();
        assert_eq!(txt.contains("trim-ledger cells=14 ok=8 err=6 rejects=0"), true);
        assert_eq!(txt.contains("root none"), true);
        assert_eq!(txt.contains("op stat 1"), true);
        assert_eq!(txt.contains("op red-rate 8"), true);
        assert_eq!(txt.contains("op defect 9"), true);
        assert_eq!(txt.contains("code 00000"), true);
        assert_eq!(txt.contains("code 6d121"), true, "缺陷格码必须逐字可查");
        // 未登记时只剩表头与根行
        assert_eq!(
            Ledger::new().render().contains("cells=0 ok=0 err=0 rejects=0"),
            true
        );
        // 单格文本自证：槽位、操作名、成败、码、数值都在
        let one = Sample::stat(3usize, 3usize, 8u32).render();
        assert_eq!(one.contains("slot 3"), true);
        assert_eq!(one.contains("value 8"), true);
        assert_eq!(one.contains(" ok"), true);
        // 越界操作号落在 void 上，且不被认成任何一面
        let void = Sample {
            slot: 0usize,
            op: 99u8,
            ok: false,
            code: 0u32,
            value: 0u32,
        };
        assert_eq!(void.op_name(), "void");
        assert_eq!(void.op_in_range(), false);
        assert_eq!(void.is_stat(), false);
        assert_eq!(void.is_defect(), false);
        assert_eq!(Sample::blank().op_name(), "void");
        assert_eq!(Sample::red_rate(0usize, 1u32).op_name(), "red-rate");
        assert_eq!(Sample::defect(0usize, 0usize, TR_ESTATE).op_name(), "defect");
        assert_eq!(Sample::defect(0usize, 5usize, TR_EDEFAULT).op_in_range(), true);
        assert_eq!(Sample::defect(0usize, 6usize, TR_ESTATE).op_in_range(), false);
    }

    #[test]
    fn all_four_injection_routes_are_caught() {
        let v = inject_four();
        assert_eq!(v.len(), 4usize);
        assert_eq!(v[0usize], true, "清单表路伪造行未被抓住");
        assert_eq!(v[1usize], true, "推演路伪造走满未被抓住");
        assert_eq!(v[2usize], true, "签发面抬绿未被抓住");
        assert_eq!(v[3usize], true, "账本偷改数值未被重放抓住");
        // 四路各自还能单独复现，避免注入探针成为唯一证据
        let fake = Spec {
            card: 0u16,
            tag: "",
            title: "fake",
            harness: Harness::Noop,
            dep: CARD_MAX + 1u16,
            need_cfg: CFG_NONE,
            asserts: 0u16,
            rows_min: 0u16,
            default_suite: false,
        };
        assert_eq!(spec_row_ok(&fake), false);
        assert_eq!(manifest_rows_ok(), true);
        let skip_shape = match shape_at(4usize) {
            Some(s) => s,
            None => panic!("跳档槽缺失"),
        };
        let real = define(&skip_shape);
        assert_eq!(real.stage, Stage::Build);
        let forged = Claim {
            success: true,
            stage: Stage::Seal,
            code: 0u32,
            value: skip_shape.asserts as u32,
            note: Note::Clean,
            default: true,
        };
        assert_eq!(judge_against(&forged, &real), Fault::Estate);
        let mut tampered = run();
        if let Some(cell) = tampered.matrix.rows.get_mut(0usize) {
            cell.total = cell.total.saturating_add(1u32);
        }
        tampered.total = tampered.total.saturating_add(1u32);
        assert_eq!(tampered.gate().is_err(), true);
        assert_eq!(run().gate().is_ok(), true);
        let mut bad = build_ledger();
        let root = match bad.freeze() {
            Ok(x) => x,
            Err(_) => panic!("干净账应能冻结"),
        };
        assert_eq!(bad.poke(2usize, val(2usize).saturating_add(1u32)), Ok(()));
        assert_eq!(bad.replay().is_err(), true);
        assert_ne!(root, 0u64);
    }

    #[test]
    fn run_with_injects_only_inside_the_corpus() {
        assert_eq!(run_with(None).map(|r| r.matched), Some(WANT_MATCHED));
        assert_eq!(run_with(Some((CORPUS, Fault::Ok))), Err(TR_ESLOT));
        assert_eq!(run_with(Some((99usize, Fault::Value))), Err(TR_ESLOT));
        // 把活槽 0 的申报打断任一维：对齐数必须掉一，且掉在绿档行里
        let mut i = 0usize;
        while i < REJECT_FAULTS.len() {
            let f = match REJECT_FAULTS.get(i) {
                Some(x) => *x,
                None => break,
            };
            let r = match run_with(Some((0usize, f))) {
                Ok(x) => x,
                Err(_) => panic!("维 {:?} 注入失败", f),
            };
            assert_eq!(
                r.matched,
                WANT_MATCHED.saturating_sub(1u32),
                "维 {:?} 没改变对齐",
                f
            );
            assert_eq!(r.total, WANT_MATCHED, "注入不许改分母");
            assert_eq!(r.matrix.green_has_lie(), false);
            assert_eq!(r.gate().is_err(), true);
            i = i.saturating_add(1usize);
        }
        // 把死槽 8 的申报改回诚实：期望维是缺陷，改成就对不齐了
        let back = match run_with(Some((8usize, Fault::Estate))) {
            Ok(x) => x,
            Err(_) => panic!("死槽注入失败"),
        };
        assert_eq!(back.matched, WANT_MATCHED.saturating_sub(1u32));
        // 未登记的槽（本身已对齐）注入 Ok 等于不改，仍是满对齐
        let same = match run_with(Some((9usize, Fault::Ok))) {
            Ok(x) => x,
            Err(_) => panic!("空注入失败"),
        };
        assert_eq!(same.matched, WANT_MATCHED);
    }

    #[test]
    fn the_thirteen_checks_are_named_and_all_green() {
        let cs = run_unx11663_checks();
        assert_eq!(cs.len(), 13usize);
        assert_eq!(cs.failed_count(), 0usize);
        assert_eq!(cs.all_passed(), true, "判据项里有红：{}", cs.red_items_text());
        assert_eq!(cs.truncated(), false);
        assert_eq!(cs.dropped(), 0usize);
        assert_eq!(cs.domain, "unx11663_trimmed_applet_text");
        // 逐项名以 TRnn- 起头，与闸次序同序
        let mut i = 0usize;
        while i < 13usize {
            let c = match cs.get(i) {
                Some(x) => x,
                None => panic!("第 {} 项判据缺失", i),
            };
            assert_eq!(c.name.starts_with("TR"), true, "第 {} 项名字不合约定", i);
            assert_eq!(c.passed, true);
            assert_eq!(c.detail.len() > 0usize, true, "第 {} 项没有说明面", i);
            i = i.saturating_add(1usize);
        }
        // 自证层的每条闸都真的可重跑，而不是只在首次运行时成立
        assert_eq!(code_table_ok(), true);
        assert_eq!(chain_ok(), true);
        assert_eq!(form_ok(), true);
        assert_eq!(stream_ok_enums(), true);
        assert_eq!(note_ok(), true);
        assert_eq!(skip_ok(), true);
        assert_eq!(harness_ok(), true);
        assert_eq!(tier_ok(), true);
        assert_eq!(manifest_rows_ok(), true);
        assert_eq!(vals_match_ok(), true);
        assert_eq!(corpus_binding_ok(), true);
        assert_eq!(define_ok(), true);
        assert_eq!(judge_ok(), true);
        assert_eq!(matrix_ok(), true);
        assert_eq!(report_ok(), true);
        assert_eq!(gate_walk_ok(), true);
        assert_eq!(ledger_ok(), true);
        assert_eq!(inject_ok(), true);
    }

    #[test]
    fn detail_and_diagnostic_expose_every_face() {
        let d = detail();
        assert_eq!(d.contains("trim-known-miss slots="), true);
        let mut lines = 0usize;
        for b in d.as_bytes() {
            if *b == b'\n' {
                lines = lines.saturating_add(1usize);
            }
        }
        assert_eq!(lines, CORPUS.saturating_add(1usize), "明细行必须一槽一行");
        let mut i = 0usize;
        while i < CORPUS {
            let tag = match WANT_TAGS.get(i) {
                Some(t) => *t,
                None => break,
            };
            assert_eq!(d.contains(tag), true, "明细里查不到 {}", tag);
            let mut needle = String::from("card ");
            needle.push_str(dec_text(sh(i).card as u32).as_str());
            assert_eq!(d.contains(needle.as_str()), true);
            i = i.saturating_add(1usize);
        }
        assert_eq!(d.contains("want ok"), true);
        assert_eq!(d.contains("want estate"), true);
        assert_eq!(d.contains("want default"), true);
        assert_eq!(d.contains("stage build"), true);
        assert_eq!(d.contains("stage seal"), true);
        // 诊断面四张同给：签发单 / 横计矩阵 / 闸链 / 账本，外加逐槽明细
        let g = diagnostic();
        assert_eq!(g.contains("trim-known-miss total=14"), true);
        assert_eq!(g.contains("green n=10 aligned=10"), true);
        assert_eq!(g.contains("gate at=seal moves=8 rejects=0 closed=ok"), true);
        assert_eq!(g.contains("trim-ledger cells=14"), true);
        assert_eq!(g.contains("trim-known-miss slots="), true);
        assert_eq!(g.len() > d.len(), true);
    }

    #[test]
    fn folding_covers_every_face_of_a_case() {
        let shp = base();
        let o = define(&shp);
        let cl0 = cl(0usize);
        // 事实面折入卡号/短名/名字/形态/载体/流/掩码/两指纹/码档/字节/耗时
        assert_eq!(shp.fold(FNV_OFF), shp.fold(FNV_OFF));
        let mut s2 = shp;
        s2.name = "other";
        assert_ne!(s2.fold(FNV_OFF), shp.fold(FNV_OFF));
        let mut s3 = shp;
        s3.log_bytes = s3.log_bytes.saturating_add(1u32);
        assert_ne!(s3.fold(FNV_OFF), shp.fold(FNV_OFF));
        let mut s4 = shp;
        s4.anchor_hash = s4.anchor_hash.saturating_add(1u32);
        assert_ne!(s4.fold(FNV_OFF), shp.fold(FNV_OFF));
        // 结局面：六列逐列参与
        let ho = o.fold(FNV_OFF);
        let mut o2 = o;
        o2.value = o2.value.saturating_add(1u32);
        assert_ne!(o2.fold(FNV_OFF), ho);
        let mut o3 = o;
        o3.default = !o3.default;
        assert_ne!(o3.fold(FNV_OFF), ho);
        let mut o4 = o;
        o4.len = o4.len.saturating_add(1u32);
        assert_ne!(o4.fold(FNV_OFF), ho);
        // 申报面：六列逐列参与
        let hc = cl0.fold(FNV_OFF);
        let mut c2 = cl0;
        c2.note = Note::TableConflict;
        assert_ne!(c2.fold(FNV_OFF), hc);
        let mut c3 = cl0;
        c3.value = c3.value.saturating_add(2u32);
        assert_ne!(c3.fold(FNV_OFF), hc);
        // 零申报是占位，不许冒充一次真实申报
        let z = Claim::zero();
        assert_eq!(z.success(), false);
        assert_eq!(z.stage, Stage::Build);
        assert_eq!(judge_against(&z, &o).estate(), true, "拿占位申报对判必须撞成败维");
        // 结论面：绿与阻都算"走过执行面"，构建闸前停的不算
        assert_eq!(o.concluded(), true);
        assert_eq!(define(&sh(4usize)).concluded(), false);
        assert_eq!(define(&sh(7usize)).concluded(), true);
        assert_eq!(o.verdict(), "ok");
        assert_eq!(define(&sh(7usize)).verdict(), "not ok");
    }
}






