//! UNX-F11680 B24 批 ktest 断言集聚合
//!
//! 本模块把 B24 批（UNX-F11661 ~ UNX-F11679，共 19 条判据）聚合成一次 ktest 生态面
//! 运行。B24 整批只钉一件事：busybox 应用层在"名字不认识、选项不认识、参数值不认识"
//! 这三类失败上，文案、流道、退出码是否与基准机逐字一致。19 条判据分头钉 notfound
//! 文案前缀（`<applet>: applet not found`）、argv[0] 未注册表项、--help 的 0 档、
//! unknown option 的 usage 尾巴、install 对未名不建伪链、以及 127 与 ENOENT 的分界。
//! 断言集本身还是同一张网：13 个判定维度上每维都有判据在钉，且没有一维被全部判据
//! 重复钉——网破一个洞，"全跑 100%"就是拿没钉住的维度凑出来的数。
//!
//! 与 B22 聚合（F11640）的区别不在结构而在**被判对象**：B22 的对象是动态链接器的
//! 装载/版本/符号面，B24 的对象是应用层分发面。两条纵线共用同一套八闸账法，但 B24
//! 的失败形态多出一类"名字层"故障：applet 名截断、大小写不合表、别名缺席——它们
//! 在 ktest 上看全是 127，只有把名字账单独列出来才分得清是表缺项还是逻辑走错。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 八道闸严格按下面次序走，前一道不过就根本没有后一道的事实输入：
//!
//! 1. 清单闸（Manifest）——先数够 19 条。条数是本批的身份证：少一条意味着有判据被
//!    静默丢弃，多一条意味着别批的判据混了进来。清单不平，后面所有计数都是无根之数。
//! 2. 身份闸（Identity）——卡号、短名、段基址三者各自唯一。清单平了但两张判据共用
//!    一个短名，汇总时它们会互相覆盖，"19 条全绿"实际只验了 18 个身份。
//! 3. 维度闸（Dims）——19 张维度掩码的并集必须铺满 13 维，且每维至少两张判据在钉。
//!    这一闸在依赖闸之前，因为"没人钉的维度"是断言集的空洞，空洞不补，跑得再顺也是
//!    假绿；而依赖环只是排布问题，排布可以在空洞之后修。
//! 4. 前置闸（Ready）——harness 与 CONFIG 位齐不齐。不齐就是 skip；本卡铁律 skip 记
//!    红不记绿：没跑就是没跑，任何"跑失败"的码位都不该盖在一条没跑的判据上。
//! 5. 执行闸（Exec）——真跑一次，取 exit code。执行事实是后面一切归因的唯一来源。
//! 6. 重试闸（Retry）——attempts>1 且最终成功判 Flaked，必须显式。抖动被洗成一次通过
//!    是本批最阴的假绿：生态面看是 100%，实际那条判据根本不稳。
//! 7. 汇总闸（Tally）——断言槽的判定结论必须逐槽命中预期，通过率按
//!    (命中数*1000 + 槽数/2)/槽数 结算，四档计数相加必须等于槽数。
//!    它在倒数第二，因为要消费前面所有闸的产物；也正因为它在这里，
//!    它才是"通过率不到 1000 就判死"的唯一出口。
//! 8. 封印闸（Seal）——按执行序折叠日志指纹。次序一变指纹就变，即使绿集完全相同。
//!    这条钉的是"一次命令全跑"这个验收语本身：换个顺序重排绿表不算同一次运行。
//!
//! ## 关键差异（本卡深度所在）
//!
//! - **断言面 vs 用例面**：B24 各卡各自钉一条字面（127 档、usage 段、文案前缀），
//!   本卡钉的是这 19 条拼成生态面时的**账**。判据本身红不红不由本卡决定，本卡只决定
//!   "报绿报得对不对"——把一条没跑的判据算成绿，比那条判据自己红更危险。
//! - **名字层单列一账**：三张身份证（卡号/短名/段基址）之外，B24 多一条"名字不重"的
//!   不变量。applet 表里两个条目共用一个短名时，逐字文案判据会同时绿，因为它们比的
//!   是同一条被覆盖后的字符串；本卡把名字唯一性放在身份闸，而不是等到文案闸才露。
//! - **维度掩码是本卡独有的第三账本**：条数（19）与槽数（14）都已有人管，本卡的第三
//!   个不变量是"断言覆盖 116 次、13 维无洞、无多维冗余"。116 = 17×6 + 2×7，这个分解
//!   被钉死在 `WANT_DIM_ASSERTS`，改动任何一张掩码都会把总量推到别处去。十七张六位
//!   掩码钉的是横切面（每条判据看六维），两张七位掩码钉的是两条跨维纵线（127 分界
//!   与 help 双快照），它们是 B24 里唯一必须同时看第七维的两条。
//! - **skip ≠ pass**：一条没跑的判据不进分母等于没这条判据；本卡把它单列 Void 档并计入
//!   红灯，通过率因此不可能靠"跳过难跑的"刷到 100%。
//! - **flake ≠ pass，也 ≠ fail**：Flaked 是"跑过了但抖过"，它必须占独立档并在渲染行里
//!   露出 attempts 数；绿集里有 flake 时 `green()` 仍可能真，但 `flake_free()` 假——
//!   两个问句分开答。
//! - **维度维与执行维不同维**：段基址撞了是清单事实错，与那条判据跑得好不好毫无关系；
//!   混在一格就会用一条判据的执行绿去掩盖另一条的身份错。
//! - **两个分母**：清单闸分母 19（本批判据条数，进 `bench=19`），判定闸分母 14（本模块
//!   断言槽数，进 `total=14`）。验收原文"通过率 100%"落在后者；前者只钉身份，
//!   绝不许被写成通过率的分子分母——因为语料必须含缺陷形态的槽。
//! - **红档率 929 独占账本一格**，不并进统计格：并进统计就把一个比率当成了计数。
//! - 语料语义：`WANT_*` 抄的是**现场真实结论**，申报表 `CLAIMS` 允许与真实结论在
//!   `WANT_NDX` 指定的那一维上分叉；判定器必须先按真实结论判一遍，再按分叉维核一遍，
//!   两遍都过才算这一槽真的被这一维抓到。这是本卡对"互校"二字的落实。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本模块段位基址（全仓唯一）。
pub const SEG_BASE: u32 = 0x6d2f0;

// ---------------------------------------------------------------------------
// 13 个错误码
// ---------------------------------------------------------------------------

/// 成败互报：判据自报通过但实跑失败，或反之。
pub const KC_ESTATE: u32 = SEG_BASE + 0x01;
/// 闸门次序：八道闸的停留档不符，或执行序不等于依赖拓扑序。
pub const KC_ESTAGE: u32 = SEG_BASE + 0x02;
/// 错误码面：exit code 或 errno 位与申报不符。
pub const KC_ECODE: u32 = SEG_BASE + 0x03;
/// 计数值面：条数、重试数、通过率、日志量级与申报不符。
pub const KC_EVALUE: u32 = SEG_BASE + 0x04;
/// 归因档：skip 成因或 flake 归注与证据不符。
pub const KC_ENOTE: u32 = SEG_BASE + 0x05;
/// 默认位：是否计入 ktest 默认必跑集的标志与申报不符。
pub const KC_EDEFAULT: u32 = SEG_BASE + 0x06;
/// 清单条数不符：聚合到的判据数不等于 B24 批的 19 条。
pub const KC_ECOUNT: u32 = SEG_BASE + 0x07;
/// 身份不唯一：卡号、短名或段基址重复。
pub const KC_EUNIQ: u32 = SEG_BASE + 0x08;
/// 维度面：断言掩码越界、留洞、冗余或总量不等于 116。
pub const KC_EDIMS: u32 = SEG_BASE + 0x09;
/// 前置缺失被记成通过：skip 记绿。
pub const KC_ESKIP: u32 = SEG_BASE + 0x0a;
/// 抖动被洗成一次通过：attempts>1 却记 Green。
pub const KC_EFLAKE: u32 = SEG_BASE + 0x0b;
/// 依赖未达却执行：blocked 被当成 fail 或 pass。
pub const KC_EBLOCK: u32 = SEG_BASE + 0x0c;
/// 槽越界：注入槽号超出语料容量。
pub const KC_ESLOT: u32 = SEG_BASE + 0x0d;

/// 通过码：判定为绿时 `Outcome::code` 的取值（不在 13 个错误码内）。
pub const TAG_OK: u32 = SEG_BASE;

// ---------------------------------------------------------------------------
// 尺寸与常量
// ---------------------------------------------------------------------------

/// 语料槽数（本模块断言的格数）。
pub const CORPUS: usize = 14usize;
/// 台账样本数（与语料槽同宽，逐槽一格）。
pub const SAMPLES: usize = 14usize;
/// 接受槽数。
pub const ACCEPT: usize = 8usize;
/// 拒绝槽数。
pub const REJECT: usize = 6usize;
/// 判据档数。
pub const TIER_COUNT: usize = 4usize;
/// 本批判据条数（B24 清单：F11661 ~ F11679）。
pub const BENCH: usize = 19usize;
/// 判定维度数。
pub const DIMS: usize = 13usize;
/// 闸门数。
pub const GATE_COUNT: usize = 8usize;
/// 归因档数。
pub const NOTE_COUNT: usize = 14usize;
/// harness 种类数。
pub const HARNESS_COUNT: usize = 7usize;
/// 满率。
pub const RATE_FULL: u32 = 1000u32;
/// 红档率（独占账本一格，不并进统计格）。
pub const RED_RATE: u32 = 929u32;
/// 断言总量：17×6 + 2×7。
pub const WANT_DIM_ASSERTS: u32 = 116u32;
/// 单维最少承载判据数（低于它算"薄"，不算洞）。
pub const WANT_DIM_FLOOR: u32 = 2u32;
/// 维度掩码全域（13 位）。
pub const DIM_FULL: u16 = 0x1fffu16;
/// 语料槽里"不分叉"的哨兵维度号。
pub const DIM_NONE: usize = DIMS;
/// 日志量级下限（低于它判薄）。
pub const LOG_FLOOR: u32 = 64u32;

// ---------------------------------------------------------------------------
// 退出码与 errno（与 ktest 日志逐字对齐）
// ---------------------------------------------------------------------------

/// 成功。
pub const EX_OK: i32 = 0;
/// 一般失败。
pub const EX_FAIL: i32 = 1;
/// 命令未找到。
pub const EX_NOTFOUND: i32 = 127;
/// skip 专用码（ktest 里 SKIP 不带 exit，这里给一个可辨识的哨兵）。
pub const EX_SKIP: i32 = 4;
/// 信号退出基址。
pub const EX_SIG_BASE: i32 = 128;
/// SIGSEGV。
pub const SIG_SEGV: i32 = 11;
/// SIGALRM（超时）。
pub const SIG_ALRM: i32 = 14;
/// ENOSPC。
pub const ENOSPC: i32 = 28;
/// ENOENT。
pub const ENOENT: i32 = 2;
/// EACCES。
pub const EACCES: i32 = 13;
/// ETIMEDOUT。
pub const ETIMEDOUT: i32 = 110;
/// 无 errno。
pub const ENONE: i32 = 0;

// ---------------------------------------------------------------------------
// CONFIG 位（裁剪矩阵的前置条件）
// ---------------------------------------------------------------------------

/// 模块支持。
pub const CFG_MODULES: u32 = 1u32 << 0;
/// debugfs。
pub const CFG_DEBUGFS: u32 = 1u32 << 1;
/// 网络。
pub const CFG_NET: u32 = 1u32 << 2;
/// tmpfs。
pub const CFG_TMPFS: u32 = 1u32 << 3;
/// root 权限。
pub const CFG_ROOT: u32 = 1u32 << 4;
/// 双机。
pub const CFG_TWOSYS: u32 = 1u32 << 5;
/// musl 线。
pub const CFG_MUSL: u32 = 1u32 << 6;
/// glibc 线。
pub const CFG_GLIBC: u32 = 1u32 << 7;
/// busybox。
pub const CFG_BUSYBOX: u32 = 1u32 << 8;
/// loop 设备。
pub const CFG_LOOP: u32 = 1u32 << 9;
/// 空集合。
pub const CFG_NONE: u32 = 0u32;

/// 全量 CONFIG。
pub const CFG_ALL: u32 = CFG_MODULES
    | CFG_DEBUGFS
    | CFG_NET
    | CFG_TMPFS
    | CFG_ROOT
    | CFG_TWOSYS
    | CFG_MUSL
    | CFG_GLIBC
    | CFG_BUSYBOX
    | CFG_LOOP;

/// 缺 musl 线的 CONFIG。
pub const CFG_NO_MUSL: u32 = CFG_ALL & !CFG_MUSL;
/// 缺 root 与 busybox 的 CONFIG。
pub const CFG_NO_ROOT: u32 = CFG_ALL & !(CFG_ROOT | CFG_BUSYBOX);

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 64 位初始偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折一字节。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 折一个字（先四次半字节，再七个字节，共 11 次折叠）。
pub fn fold_word(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < 4 {
        let byte = ((v >> (i as u32 * 4u32)) & 0x0fu64) as u8;
        x = fold_byte(x, byte);
        i += 1;
    }
    x = fold_byte(x, ((v >> 16u32) & 0xffu64) as u8);
    x = fold_byte(x, ((v >> 24u32) & 0xffu64) as u8);
    x = fold_byte(x, ((v >> 32u32) & 0xffu64) as u8);
    x = fold_byte(x, ((v >> 40u32) & 0xffu64) as u8);
    x = fold_byte(x, ((v >> 48u32) & 0xffu64) as u8);
    x = fold_byte(x, ((v >> 56u32) & 0xffu64) as u8);
    x
}

/// 折一段字符串（逐字，末尾补长度字）。
pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    for b in s.as_bytes() {
        x = fold_byte(x, *b);
    }
    fold_word(x, s.as_bytes().len() as u64)
}

// ---------------------------------------------------------------------------
// 数字/布尔文本（全部手写，不递归，不用 format!）
// ---------------------------------------------------------------------------

/// u32 → 十进制文本。
pub fn dec_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut digits = [0u8; 10];
    let mut n = v;
    let mut k = 0usize;
    while n > 0 && k < 10 {
        if let Some(d) = digits.get_mut(k) {
            *d = b'0' + (n % 10u32) as u8;
        }
        n /= 10u32;
        k += 1;
    }
    let mut out = String::new();
    let mut i = k;
    while i > 0 {
        i -= 1;
        if let Some(b) = digits.get(i) {
            out.push(*b as char);
        }
    }
    out
}

/// u32 → 定宽小写十六进制文本。
pub fn hex_text(v: u32, width: usize) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < width {
        let shift = ((width - i - 1) as u32) * 4u32;
        let nib = (v >> shift) & 0x0fu32;
        let c = match nib {
            0u32 => '0',
            1u32 => '1',
            2u32 => '2',
            3u32 => '3',
            4u32 => '4',
            5u32 => '5',
            6u32 => '6',
            7u32 => '7',
            8u32 => '8',
            9u32 => '9',
            10u32 => 'a',
            11u32 => 'b',
            12u32 => 'c',
            13u32 => 'd',
            14u32 => 'e',
            _ => 'f',
        };
        out.push(c);
        i += 1;
    }
    out
}

/// u32 文本别名。
pub fn u32_text(v: u32) -> String {
    dec_text(v)
}

/// i32 文本（负号手写）。
pub fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(dec_text((v as i64).unsigned_abs() as u32).as_str());
        s
    } else {
        dec_text(v as u32)
    }
}

/// 布尔文本，逐字对齐 ktest 日志里的 ok/not ok。
pub fn bool_text(v: bool) -> &'static str {
    match v {
        true => "ok",
        false => "not ok",
    }
}

// ---------------------------------------------------------------------------
// 13 个判定维度
// ---------------------------------------------------------------------------

/// 判定维度。次序即 `judge` 的核对序，也是掩码位的次序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    /// 成败互报。
    Estate,
    /// 闸门次序。
    Stage,
    /// 错误码面。
    Code,
    /// 计数值面。
    Value,
    /// 归因档。
    Note,
    /// 默认位。
    Default,
    /// 清单条数。
    Count,
    /// 身份唯一。
    Uniq,
    /// 维度覆盖。
    Dims,
    /// 前置缺失。
    Skip,
    /// 抖动。
    Flake,
    /// 依赖未达。
    Block,
    /// 槽越界。
    Slot,
}

impl Dim {
    /// 全部维度，掩码位次序。
    pub const fn all() -> [Dim; DIMS] {
        [
            Dim::Estate,
            Dim::Stage,
            Dim::Code,
            Dim::Value,
            Dim::Note,
            Dim::Default,
            Dim::Count,
            Dim::Uniq,
            Dim::Dims,
            Dim::Skip,
            Dim::Flake,
            Dim::Block,
            Dim::Slot,
        ]
    }

    /// 掩码位。
    pub const fn bit(self) -> u16 {
        match self {
            Dim::Estate => 1u16 << 0,
            Dim::Stage => 1u16 << 1,
            Dim::Code => 1u16 << 2,
            Dim::Value => 1u16 << 3,
            Dim::Note => 1u16 << 4,
            Dim::Default => 1u16 << 5,
            Dim::Count => 1u16 << 6,
            Dim::Uniq => 1u16 << 7,
            Dim::Dims => 1u16 << 8,
            Dim::Skip => 1u16 << 9,
            Dim::Flake => 1u16 << 10,
            Dim::Block => 1u16 << 11,
            Dim::Slot => 1u16 << 12,
        }
    }

    /// 维度序号。
    pub const fn rank(self) -> usize {
        match self {
            Dim::Estate => 0usize,
            Dim::Stage => 1usize,
            Dim::Code => 2usize,
            Dim::Value => 3usize,
            Dim::Note => 4usize,
            Dim::Default => 5usize,
            Dim::Count => 6usize,
            Dim::Uniq => 7usize,
            Dim::Dims => 8usize,
            Dim::Skip => 9usize,
            Dim::Flake => 10usize,
            Dim::Block => 11usize,
            Dim::Slot => 12usize,
        }
    }

    /// 由序号反查维度。
    pub fn of_rank(i: usize) -> Option<Dim> {
        let mut k = 0usize;
        while k < DIMS {
            if let Some(d) = Dim::all().get(k) {
                if d.rank() == i {
                    return Some(*d);
                }
            }
            k += 1;
        }
        None
    }

    /// 维度名（ktest 日志用）。
    pub const fn name(self) -> &'static str {
        match self {
            Dim::Estate => "estate",
            Dim::Stage => "stage",
            Dim::Code => "code",
            Dim::Value => "value",
            Dim::Note => "note",
            Dim::Default => "default",
            Dim::Count => "count",
            Dim::Uniq => "uniq",
            Dim::Dims => "dims",
            Dim::Skip => "skip",
            Dim::Flake => "flake",
            Dim::Block => "block",
            Dim::Slot => "slot",
        }
    }
}

// ---------------------------------------------------------------------------
// 八道闸
// ---------------------------------------------------------------------------

/// 闸门停留档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 清单。
    Manifest,
    /// 身份。
    Identity,
    /// 维度。
    Dims,
    /// 前置。
    Ready,
    /// 执行。
    Exec,
    /// 重试。
    Retry,
    /// 汇总。
    Tally,
    /// 封印。
    Seal,
}

impl Stage {
    /// 全部闸门，次序即执行序。
    pub const fn all() -> [Stage; GATE_COUNT] {
        [
            Stage::Manifest,
            Stage::Identity,
            Stage::Dims,
            Stage::Ready,
            Stage::Exec,
            Stage::Retry,
            Stage::Tally,
            Stage::Seal,
        ]
    }

    /// 闸序。
    pub const fn rank(self) -> usize {
        match self {
            Stage::Manifest => 0usize,
            Stage::Identity => 1usize,
            Stage::Dims => 2usize,
            Stage::Ready => 3usize,
            Stage::Exec => 4usize,
            Stage::Retry => 5usize,
            Stage::Tally => 6usize,
            Stage::Seal => 7usize,
        }
    }

    /// 由闸序反查。
    pub fn of_rank(i: usize) -> Option<Stage> {
        let mut k = 0usize;
        while k < GATE_COUNT {
            if let Some(s) = Stage::all().get(k) {
                if s.rank() == i {
                    return Some(*s);
                }
            }
            k += 1;
        }
        None
    }

    /// 下一档（末档返回自身）。
    pub const fn next(self) -> Stage {
        match self {
            Stage::Manifest => Stage::Identity,
            Stage::Identity => Stage::Dims,
            Stage::Dims => Stage::Ready,
            Stage::Ready => Stage::Exec,
            Stage::Exec => Stage::Retry,
            Stage::Retry => Stage::Tally,
            Stage::Tally => Stage::Seal,
            Stage::Seal => Stage::Seal,
        }
    }

    /// 档名。
    pub const fn name(self) -> &'static str {
        match self {
            Stage::Manifest => "manifest",
            Stage::Identity => "identity",
            Stage::Dims => "dims",
            Stage::Ready => "ready",
            Stage::Exec => "exec",
            Stage::Retry => "retry",
            Stage::Tally => "tally",
            Stage::Seal => "seal",
        }
    }
}

/// 闸门链，供 check 逐格比对。
pub const STAGE_CHAIN: [Stage; GATE_COUNT] = [
    Stage::Manifest,
    Stage::Identity,
    Stage::Dims,
    Stage::Ready,
    Stage::Exec,
    Stage::Retry,
    Stage::Tally,
    Stage::Seal,
];

// ---------------------------------------------------------------------------
// 归因档
// ---------------------------------------------------------------------------

/// 归因档。每条判据只允许落一个档，且必须与证据一致。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 无异常。
    Clean,
    /// 清单缺卡。
    MissingCard,
    /// 短名重复。
    DupTag,
    /// 段基址重复。
    DupSeg,
    /// 掩码越界或为空。
    DimBad,
    /// 维度留洞。
    DimHole,
    /// 依赖成环。
    DepCycle,
    /// harness 缺失。
    HarnessMissing,
    /// CONFIG 位缺失。
    CfgMissing,
    /// 抖动一次后通过。
    FlakeOnce,
    /// 上游未达。
    BlockedUp,
    /// 已封印。
    Sealed,
    /// 退出码不符。
    ExitBad,
    /// 日志量级过薄。
    LogThin,
}

impl Note {
    /// 全部归因档。
    pub const fn all() -> [Note; NOTE_COUNT] {
        [
            Note::Clean,
            Note::MissingCard,
            Note::DupTag,
            Note::DupSeg,
            Note::DimBad,
            Note::DimHole,
            Note::DepCycle,
            Note::HarnessMissing,
            Note::CfgMissing,
            Note::FlakeOnce,
            Note::BlockedUp,
            Note::Sealed,
            Note::ExitBad,
            Note::LogThin,
        ]
    }

    /// 档名。
    pub const fn name(self) -> &'static str {
        match self {
            Note::Clean => "clean",
            Note::MissingCard => "missing-card",
            Note::DupTag => "dup-tag",
            Note::DupSeg => "dup-seg",
            Note::DimBad => "dim-bad",
            Note::DimHole => "dim-hole",
            Note::DepCycle => "dep-cycle",
            Note::HarnessMissing => "harness-missing",
            Note::CfgMissing => "cfg-missing",
            Note::FlakeOnce => "flake-once",
            Note::BlockedUp => "blocked-up",
            Note::Sealed => "sealed",
            Note::ExitBad => "exit-bad",
            Note::LogThin => "log-thin",
        }
    }

    /// 该档是否允许判绿。
    pub const fn is_green(self) -> bool {
        match self {
            Note::Clean => true,
            Note::Sealed => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 判据档（四分，不许再细分，也不许合并）
// ---------------------------------------------------------------------------

/// 判据档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 锚定：sealed 且在默认必跑集内。
    Anchored,
    /// 申报：sealed 但不在默认必跑集内。
    Declared,
    /// 空洞：前置缺失，没跑。
    Void,
    /// 未达：跑到半途被判死。
    Unrun,
}

impl Tier {
    /// 全部档。
    pub const fn all() -> [Tier; TIER_COUNT] {
        [Tier::Anchored, Tier::Declared, Tier::Void, Tier::Unrun]
    }

    /// 档名。
    pub const fn name(self) -> &'static str {
        match self {
            Tier::Anchored => "anchored",
            Tier::Declared => "declared",
            Tier::Void => "void",
            Tier::Unrun => "unrun",
        }
    }

    /// 档序号。
    pub const fn rank(self) -> usize {
        match self {
            Tier::Anchored => 0usize,
            Tier::Declared => 1usize,
            Tier::Void => 2usize,
            Tier::Unrun => 3usize,
        }
    }
}

/// 判定故障（六维 + 无）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 无故障。
    None,
    /// 成败互报。
    Estate,
    /// 闸门次序。
    Stage,
    /// 错误码面。
    Code,
    /// 计数值面。
    Value,
    /// 归因档。
    Note,
    /// 默认位。
    Default,
}

impl Fault {
    /// 故障码。
    pub const fn code(self) -> u32 {
        match self {
            Fault::None => TAG_OK,
            Fault::Estate => KC_ESTATE,
            Fault::Stage => KC_ESTAGE,
            Fault::Code => KC_ECODE,
            Fault::Value => KC_EVALUE,
            Fault::Note => KC_ENOTE,
            Fault::Default => KC_EDEFAULT,
        }
    }

    /// 故障对应维度。
    pub const fn dim(self) -> Option<Dim> {
        match self {
            Fault::Estate => Some(Dim::Estate),
            Fault::Stage => Some(Dim::Stage),
            Fault::Code => Some(Dim::Code),
            Fault::Value => Some(Dim::Value),
            Fault::Note => Some(Dim::Note),
            Fault::Default => Some(Dim::Default),
            Fault::None => None,
        }
    }

    /// 故障名。
    pub const fn name(self) -> &'static str {
        match self {
            Fault::None => "none",
            Fault::Estate => "estate",
            Fault::Stage => "stage",
            Fault::Code => "code",
            Fault::Value => "value",
            Fault::Note => "note",
            Fault::Default => "default",
        }
    }

    /// 由维度反查故障。
    pub const fn of_dim(d: Dim) -> Fault {
        match d {
            Dim::Estate => Fault::Estate,
            Dim::Stage => Fault::Stage,
            Dim::Code => Fault::Code,
            Dim::Value => Fault::Value,
            Dim::Note => Fault::Note,
            Dim::Default => Fault::Default,
            _ => Fault::None,
        }
    }
}
// ---------------------------------------------------------------------------
// harness（7 种）
// ---------------------------------------------------------------------------

/// harness 种类。每条判据必须声明它靠哪一种通道取事实。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Harness {
    /// 直跑二进制。
    Direct,
    /// ktest 内核自测。
    Ktest,
    /// 双机对照。
    Twosys,
    /// musl 线。
    Musl,
    /// glibc 线。
    Glibc,
    /// busybox applet。
    Busybox,
    /// loop 设备（坏盘模拟）。
    Loop,
}

impl Harness {
    /// 全部 harness。
    pub const fn all() -> [Harness; HARNESS_COUNT] {
        [
            Harness::Direct,
            Harness::Ktest,
            Harness::Twosys,
            Harness::Musl,
            Harness::Glibc,
            Harness::Busybox,
            Harness::Loop,
        ]
    }

    /// 序号。
    pub const fn index(self) -> usize {
        match self {
            Harness::Direct => 0usize,
            Harness::Ktest => 1usize,
            Harness::Twosys => 2usize,
            Harness::Musl => 3usize,
            Harness::Glibc => 4usize,
            Harness::Busybox => 5usize,
            Harness::Loop => 6usize,
        }
    }

    /// 由序号反查。
    pub fn of_index(i: usize) -> Option<Harness> {
        let mut k = 0usize;
        while k < HARNESS_COUNT {
            if let Some(h) = Harness::all().get(k) {
                if h.index() == i {
                    return Some(*h);
                }
            }
            k += 1;
        }
        None
    }

    /// 该 harness 必须点亮哪些 CONFIG 位。
    pub const fn requires(self) -> u32 {
        match self {
            Harness::Direct => CFG_NONE,
            Harness::Ktest => CFG_MODULES,
            Harness::Twosys => CFG_TWOSYS | CFG_MUSL | CFG_GLIBC,
            Harness::Musl => CFG_MUSL,
            Harness::Glibc => CFG_GLIBC,
            Harness::Busybox => CFG_BUSYBOX,
            Harness::Loop => CFG_LOOP | CFG_TMPFS,
        }
    }

    /// 名。
    pub const fn name(self) -> &'static str {
        match self {
            Harness::Direct => "direct",
            Harness::Ktest => "ktest",
            Harness::Twosys => "twosys",
            Harness::Musl => "musl",
            Harness::Glibc => "glibc",
            Harness::Busybox => "busybox",
            Harness::Loop => "loop",
        }
    }
}

/// 前置是否齐备。
pub fn ready(sh: &Shape, cfg: u32) -> bool {
    let need = sh.need | sh.harness.requires();
    (cfg & need) == need
}

// ---------------------------------------------------------------------------
// 形态 / 结论 / 申报 / 用例
// ---------------------------------------------------------------------------

/// 一条判据在生态面上的形态（即真跑时能取到的全部事实）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 清单下标（越界即缺卡）。
    pub idx: usize,
    /// 断言维度掩码。
    pub dims: u16,
    /// harness 种类。
    pub harness: Harness,
    /// 该条判据自身的 CONFIG 需要。
    pub need: u32,
    /// 尝试次数。
    pub attempts: u8,
    /// 退出码。
    pub exit: i32,
    /// 日志字节量级。
    pub log: u32,
    /// 是否在默认必跑集。
    pub default: bool,
    /// 是否带双机锚。
    pub anchor: bool,
    /// 附加 CONFIG（真跑现场点亮的位）。
    pub cfg: u32,
}

impl Shape {
    /// 该形态的真跑退出码是否为成功。
    pub fn exited_ok(&self) -> bool {
        self.exit == EX_OK
    }

    /// 该形态是否带 skip 码。
    pub fn is_skip(&self) -> bool {
        self.exit == EX_SKIP
    }

    /// 掩码是否合法（非空且不越界）。
    pub fn mask_ok(&self) -> bool {
        self.dims != 0 && (self.dims & !DIM_FULL) == 0
    }

    /// 权重：掩码里钉了几维。
    pub fn weight(&self) -> u32 {
        popcount16(self.dims)
    }
}

/// 一条判据的真跑结论（六维 + 默认位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 停留档。
    pub stage: Stage,
    /// 码面。
    pub code: u32,
    /// 计数值。
    pub value: u32,
    /// 归因档。
    pub note: Note,
    /// 默认位。
    pub default: bool,
}

impl Outcome {
    /// 是否判绿。
    pub const fn ok(&self) -> bool {
        self.code == TAG_OK
    }

    /// 是否停留在某一档之前。
    pub fn before(&self, s: Stage) -> bool {
        self.stage.rank() < s.rank()
    }

    /// 归因档是否允许绿。
    pub const fn note_green(&self) -> bool {
        self.note.is_green()
    }
}

/// 申报表的一格（判据自己说它跑出了什么）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 自报成败。
    pub success: bool,
    /// 自报停留档。
    pub stage: Stage,
    /// 自报码面。
    pub code: u32,
    /// 自报计数。
    pub value: u32,
    /// 自报归因。
    pub note: Note,
    /// 自报默认位。
    pub default: bool,
    /// 申报短名。
    pub tag: &'static str,
}

impl Claim {
    /// 是否自报成功（码面为通过码）。
    pub const fn success(&self) -> bool {
        self.code == TAG_OK
    }

    /// 与现场结论在某一维上是否分叉。
    pub fn differs(&self, a: &Outcome, d: Dim) -> bool {
        match d {
            Dim::Estate => self.success != a.ok(),
            Dim::Stage => self.stage != a.stage,
            Dim::Code => self.code != a.code,
            Dim::Value => self.value != a.value,
            Dim::Note => self.note != a.note,
            Dim::Default => self.default != a.default,
            _ => false,
        }
    }

    /// 与现场结论分叉的维度数（只在六维内数）。
    pub fn divergence(&self, a: &Outcome) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < 6 {
            if let Some(d) = Dim::of_rank(i) {
                if self.differs(a, d) {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 一格语料 = 形态 + 申报表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    /// 形态。
    pub shape: Shape,
    /// 申报。
    pub claim: Claim,
}

impl Case {
    /// 现场结论。
    pub fn actual(&self) -> Outcome {
        define(&self.shape)
    }

    /// 判据短名。
    pub fn tag(&self) -> &'static str {
        self.claim.tag
    }
}

// ---------------------------------------------------------------------------
// 清单（19 条）
// ---------------------------------------------------------------------------

/// 本批的一张判据身份证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spec {
    /// 卡号。
    pub card: u16,
    /// 短名。
    pub tag: &'static str,
    /// 段基址。
    pub seg: u32,
    /// 断言维度掩码。
    pub dims: u16,
    /// 取数通道。
    pub harness: Harness,
    /// 该条判据自身的 CONFIG 需要。
    pub need: u32,
    /// 前置卡号（0 表示无）。
    pub depends: u16,
    /// 是否进默认必跑集。
    pub default: bool,
}

/// 卡号区间下限。
pub const CARD_MIN: u16 = 11661u16;
/// 卡号区间上限。
pub const CARD_MAX: u16 = 11679u16;

/// 19 条判据的身份证。掩码分解固定为 17×6 + 2×7 = 116。
///
/// 两张七位掩码落在 F11671（大小写/别名）与 F11672（install 跳过）上：这两条是 B24
/// 里唯一必须同时看"名字层 + 退出码层"的判据，其余十七条各看六维。
pub const SPECS: [Spec; BENCH] = [
    Spec {
        card: 11661u16,
        tag: "f11661-bb-notfound",
        seg: 0x6d100u32,
        dims: 0x064du16,
        harness: Harness::Ktest,
        need: CFG_MODULES,
        depends: 0u16,
        default: true,
    },
    Spec {
        card: 11662u16,
        tag: "f11662-argv0-unregistered",
        seg: 0x6d110u32,
        dims: 0x012fu16,
        harness: Harness::Direct,
        need: CFG_NONE,
        depends: 11661u16,
        default: true,
    },
    Spec {
        card: 11663u16,
        tag: "f11663-trimmed-applet",
        seg: 0x6d120u32,
        dims: 0x1853u16,
        harness: Harness::Glibc,
        need: CFG_GLIBC,
        depends: 11662u16,
        default: true,
    },
    Spec {
        card: 11664u16,
        tag: "f11664-notfound-exit",
        seg: 0x6d1a0u32,
        dims: 0x07a4u16,
        harness: Harness::Musl,
        need: CFG_MUSL,
        depends: 11663u16,
        default: false,
    },
    Spec {
        card: 11665u16,
        tag: "f11665-cmd-notfound-text",
        seg: 0x6d1b0u32,
        dims: 0x0c1bu16,
        harness: Harness::Direct,
        need: CFG_NONE,
        depends: 11664u16,
        default: true,
    },
    Spec {
        card: 11666u16,
        tag: "f11666-path-notfound",
        seg: 0x6d1d0u32,
        dims: 0x106du16,
        harness: Harness::Ktest,
        need: CFG_MODULES,
        depends: 11665u16,
        default: true,
    },
    Spec {
        card: 11667u16,
        tag: "f11667-help-exit0",
        seg: 0x6d1e0u32,
        dims: 0x0297u16,
        harness: Harness::Twosys,
        need: CFG_TWOSYS,
        depends: 11666u16,
        default: true,
    },
    Spec {
        card: 11668u16,
        tag: "f11668-applet-usage",
        seg: 0x6d1f0u32,
        dims: 0x0d29u16,
        harness: Harness::Musl,
        need: CFG_MUSL,
        depends: 11667u16,
        default: false,
    },
    Spec {
        card: 11669u16,
        tag: "f11669-unknown-option",
        seg: 0x6d210u32,
        dims: 0x10d3u16,
        harness: Harness::Loop,
        need: CFG_LOOP,
        depends: 11668u16,
        default: true,
    },
    Spec {
        card: 11670u16,
        tag: "f11670-bad-value",
        seg: 0x6d220u32,
        dims: 0x0764u16,
        harness: Harness::Direct,
        need: CFG_NONE,
        depends: 11669u16,
        default: true,
    },
    Spec {
        card: 11671u16,
        tag: "f11671-case-alias",
        seg: 0x6d230u32,
        dims: 0x181fu16,
        harness: Harness::Glibc,
        need: CFG_GLIBC,
        depends: 11670u16,
        default: true,
    },
    Spec {
        card: 11672u16,
        tag: "f11672-install-skip",
        seg: 0x6d250u32,
        dims: 0x07e1u16,
        harness: Harness::Ktest,
        need: CFG_MODULES,
        depends: 11671u16,
        default: false,
    },
    Spec {
        card: 11673u16,
        tag: "f11673-table-diff",
        seg: 0x6d260u32,
        dims: 0x003fu16,
        harness: Harness::Busybox,
        need: CFG_BUSYBOX,
        depends: 11672u16,
        default: true,
    },
    Spec {
        card: 11674u16,
        tag: "f11674-applet-text",
        seg: 0x6d270u32,
        dims: 0x024fu16,
        harness: Harness::Direct,
        need: CFG_NONE,
        depends: 11673u16,
        default: true,
    },
    Spec {
        card: 11675u16,
        tag: "f11675-notfound-enoent",
        seg: 0x6d290u32,
        dims: 0x0c1du16,
        harness: Harness::Musl,
        need: CFG_MUSL,
        depends: 11674u16,
        default: true,
    },
    Spec {
        card: 11676u16,
        tag: "f11676-trim-matrix",
        seg: 0x6d2a0u32,
        dims: 0x10a7u16,
        harness: Harness::Glibc,
        need: CFG_GLIBC,
        depends: 11675u16,
        default: false,
    },
    Spec {
        card: 11677u16,
        tag: "f11677-help-dual",
        seg: 0x6d2b0u32,
        dims: 0x061du16,
        harness: Harness::Twosys,
        need: CFG_TWOSYS,
        depends: 11676u16,
        default: true,
    },
    Spec {
        card: 11678u16,
        tag: "f11678-split-streams",
        seg: 0x6d2d0u32,
        dims: 0x086bu16,
        harness: Harness::Loop,
        need: CFG_LOOP,
        depends: 11677u16,
        default: true,
    },
    Spec {
        card: 11679u16,
        tag: "f11679-unknown-matrix",
        seg: 0x6d2e0u32,
        dims: 0x1097u16,
        harness: Harness::Twosys,
        need: CFG_TWOSYS,
        depends: 11678u16,
        default: true,
    },
];

/// 取第 i 张身份证。
pub fn spec_at(i: usize) -> Option<&'static Spec> {
    SPECS.get(i)
}

/// 由卡号反查下标。
pub fn spec_index_of(card: u16) -> Option<usize> {
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = SPECS.get(i) {
            if s.card == card {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 掩码数学
// ---------------------------------------------------------------------------

/// 16 位 popcount（手写，不用 intrinsics）。
pub fn popcount16(v: u16) -> u32 {
    let mut n = v;
    let mut c = 0u32;
    while n != 0 {
        c += (n & 1u16) as u32;
        n >>= 1;
    }
    c
}

/// 第 d 维被多少张判据钉住。
pub fn dim_cover(d: Dim) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = SPECS.get(i) {
            if (s.dims & d.bit()) != 0 {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 留洞维度数（覆盖为 0）。
pub fn dim_holes() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < DIMS {
        if let Some(d) = Dim::of_rank(i) {
            if dim_cover(d) == 0 {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 过薄维度数（覆盖低于下限但不为 0）。
pub fn dim_fragile() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < DIMS {
        if let Some(d) = Dim::of_rank(i) {
            let c = dim_cover(d);
            if c != 0 && c < WANT_DIM_FLOOR {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 冗余维度数（每一张判据都钉它）。
pub fn dim_dense() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < DIMS {
        if let Some(d) = Dim::of_rank(i) {
            if dim_cover(d) == BENCH as u32 {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 19 张掩码的并集。
pub fn dim_union() -> u16 {
    let mut u = 0u16;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = SPECS.get(i) {
            u |= s.dims;
        }
        i += 1;
    }
    u
}

/// 断言总量（19 张掩码 popcount 之和）。
pub fn dim_asserts_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = SPECS.get(i) {
            n += popcount16(s.dims);
        }
        i += 1;
    }
    n
}

/// 被钉住的维度数。
pub fn dim_full_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < DIMS {
        if let Some(d) = Dim::of_rank(i) {
            if dim_cover(d) != 0 {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 掩码面整体是否成立：满 13 维、无洞、无薄、无冗余、总量 116、无越界。
pub fn dim_ok() -> bool {
    if dim_union() != DIM_FULL {
        return false;
    }
    if dim_holes() != 0 || dim_fragile() != 0 || dim_dense() != 0 {
        return false;
    }
    if dim_asserts_total() != WANT_DIM_ASSERTS {
        return false;
    }
    if dim_full_count() != DIMS as u32 {
        return false;
    }
    let mut i = 0usize;
    while i < BENCH {
        match SPECS.get(i) {
            Some(s) => {
                if s.dims == 0 || (s.dims & !DIM_FULL) != 0 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 清单面自检
// ---------------------------------------------------------------------------

/// 清单条数是否等于 19。
pub fn manifest_count_ok() -> bool {
    let n = SPECS.len();
    n == BENCH
}

/// 卡号是否自 11661 连续到 11679。
pub fn manifest_sorted_ok() -> bool {
    let mut i = 0usize;
    while i < BENCH {
        match SPECS.get(i) {
            Some(s) => {
                let want = CARD_MIN as u32 + i as u32;
                if s.card as u32 != want {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    CARD_MIN as u32 + (BENCH as u32 - 1u32) == CARD_MAX as u32
}

/// 短名唯一。
pub fn tags_unique_ok() -> bool {
    let mut i = 0usize;
    while i < BENCH {
        match SPECS.get(i) {
            Some(a) => {
                if a.tag.is_empty() {
                    return false;
                }
                let mut j = i + 1usize;
                while j < BENCH {
                    if let Some(b) = SPECS.get(j) {
                        if a.tag == b.tag {
                            return false;
                        }
                    }
                    j += 1;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 段基址唯一。
pub fn segs_unique_ok() -> bool {
    let mut i = 0usize;
    while i < BENCH {
        match SPECS.get(i) {
            Some(a) => {
                if a.seg == 0u32 {
                    return false;
                }
                let mut j = i + 1usize;
                while j < BENCH {
                    if let Some(b) = SPECS.get(j) {
                        if a.seg == b.seg {
                            return false;
                        }
                    }
                    j += 1;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 前置依赖都落在本批内，且严格小于自身（因而不成环）。
pub fn deps_ok() -> bool {
    let mut i = 0usize;
    while i < BENCH {
        match SPECS.get(i) {
            Some(s) => {
                if s.depends != 0u16 {
                    if s.depends < CARD_MIN || s.depends > CARD_MAX {
                        return false;
                    }
                    if s.depends >= s.card {
                        return false;
                    }
                    if spec_index_of(s.depends).is_none() {
                        return false;
                    }
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 清单面整体是否成立。
pub fn manifest_ok() -> bool {
    manifest_count_ok() && manifest_sorted_ok() && tags_unique_ok() && segs_unique_ok() && deps_ok()
}

/// 默认必跑集条数。
pub fn default_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = SPECS.get(i) {
            if s.default {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 拓扑序（按前置号升序即可，因为依赖严格小于自身）。
pub fn topo_order() -> Option<[usize; BENCH]> {
    let mut out = [0usize; BENCH];
    let mut i = 0usize;
    while i < BENCH {
        match SPECS.get(i) {
            Some(s) => {
                if s.depends != 0u16 && spec_index_of(s.depends).is_none() {
                    return None;
                }
                if let Some(c) = out.get_mut(i) {
                    *c = i;
                }
            }
            None => return None,
        }
        i += 1;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 定义：由形态推出真跑结论
// ---------------------------------------------------------------------------

/// 由形态推出真跑结论。闸门次序：Manifest → Dims → Ready → Exec → Retry → Tally → Seal。
///
/// 身份闸（Identity）是清单级闸：单条形态看不到别人的短名与段基址，所以它在
/// `gate_bits()` 里结算，不在这条纵线上伪造一次"我看过了"。
pub fn define(sh: &Shape) -> Outcome {
    if sh.idx >= BENCH {
        return Outcome {
            stage: Stage::Manifest,
            code: KC_ECOUNT,
            value: sh.idx as u32,
            note: Note::MissingCard,
            default: sh.default,
        };
    }
    if !sh.mask_ok() {
        return Outcome {
            stage: Stage::Dims,
            code: KC_EDIMS,
            value: sh.dims as u32,
            note: Note::DimBad,
            default: sh.default,
        };
    }
    if !ready(sh, sh.cfg) {
        let hneed = sh.harness.requires();
        let cneed = sh.need;
        let hmiss = (sh.cfg & hneed) != hneed;
        let cmiss = (sh.cfg & cneed) != cneed;
        let note = match (hmiss, cmiss) {
            (true, _) => Note::HarnessMissing,
            (false, true) => Note::CfgMissing,
            (false, false) => Note::HarnessMissing,
        };
        return Outcome {
            stage: Stage::Ready,
            code: KC_ESKIP,
            value: 0u32,
            note,
            default: sh.default,
        };
    }
    if sh.exit != EX_OK {
        let (code, note) = match sh.exit {
            EX_SKIP => (KC_ESKIP, Note::CfgMissing),
            EX_NOTFOUND => (KC_ECODE, Note::ExitBad),
            _ => (KC_ECODE, Note::ExitBad),
        };
        return Outcome {
            stage: Stage::Exec,
            code,
            value: sh.exit as u32,
            note,
            default: sh.default,
        };
    }
    if sh.attempts > 1u8 {
        return Outcome {
            stage: Stage::Retry,
            code: KC_EFLAKE,
            value: sh.attempts as u32,
            note: Note::FlakeOnce,
            default: sh.default,
        };
    }
    if sh.log < LOG_FLOOR {
        return Outcome {
            stage: Stage::Tally,
            code: KC_EVALUE,
            value: sh.log,
            note: Note::LogThin,
            default: sh.default,
        };
    }
    if !sh.anchor {
        return Outcome {
            stage: Stage::Seal,
            code: KC_EBLOCK,
            value: 0u32,
            note: Note::BlockedUp,
            default: sh.default,
        };
    }
    Outcome {
        stage: Stage::Seal,
        code: TAG_OK,
        value: sh.log,
        note: Note::Sealed,
        default: sh.default,
    }
}

impl Shape {
    /// 形态所指的卡号（下标映射，越界时回落到 0）。
    pub fn idx_card(&self) -> u16 {
        match SPECS.get(self.idx) {
            Some(s) => s.card,
            None => 0u16,
        }
    }
}

/// 判据档：由真跑结论归位。
pub fn tier_of(o: &Outcome) -> Tier {
    if o.stage == Stage::Seal && o.code == TAG_OK {
        return match o.default {
            true => Tier::Anchored,
            false => Tier::Declared,
        };
    }
    if o.code == KC_ESKIP {
        return Tier::Void;
    }
    Tier::Unrun
}

// ---------------------------------------------------------------------------
// 判定器（次序：Estate → Stage → Code → Value → Note → Default → Ok）
// ---------------------------------------------------------------------------

/// 按申报表与现场结论六维对照。
pub fn judge_against(claim: &Claim, actual: &Outcome) -> Fault {
    if claim.success != actual.ok() {
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
    Fault::None
}

/// 判一格语料。
pub fn judge(c: &Case) -> Fault {
    judge_against(&c.claim, &c.actual())
}

/// 六维故障的固定次序（拒绝槽逐槽用一维）。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Stage,
    Fault::Code,
    Fault::Value,
    Fault::Note,
    Fault::Default,
    Fault::Estate,
];

/// 拒绝槽的预期故障码。
pub const REJECT_CODES: [u32; REJECT] = [
    KC_ESTAGE,
    KC_ECODE,
    KC_EVALUE,
    KC_ENOTE,
    KC_EDEFAULT,
    KC_ESTATE,
];

/// 接受槽的八个数（第 8 格独占红档率 929）。
pub const VALS: [u32; ACCEPT] = [640u32, 576u32, 512u32, 448u32, 384u32, 320u32, 256u32, RED_RATE];
// ---------------------------------------------------------------------------
// 语料（14 槽 = 8 接受 + 6 拒绝）
// ---------------------------------------------------------------------------

/// 14 个形态。0..8 为接受槽，8..14 为拒绝槽。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        idx: 0usize,
        dims: 0x064du16,
        harness: Harness::Ktest,
        need: CFG_MODULES,
        attempts: 1u8,
        exit: EX_OK,
        log: 640u32,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 1usize,
        dims: 0x012fu16,
        harness: Harness::Direct,
        need: CFG_NONE,
        attempts: 1u8,
        exit: EX_OK,
        log: 576u32,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 2usize,
        dims: 0x1853u16,
        harness: Harness::Glibc,
        need: CFG_GLIBC,
        attempts: 1u8,
        exit: EX_OK,
        log: 512u32,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 3usize,
        dims: 0x07a4u16,
        harness: Harness::Musl,
        need: CFG_MUSL,
        attempts: 1u8,
        exit: EX_OK,
        log: 448u32,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 4usize,
        dims: 0x0c1bu16,
        harness: Harness::Direct,
        need: CFG_NONE,
        attempts: 1u8,
        exit: EX_OK,
        log: 384u32,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 5usize,
        dims: 0x106du16,
        harness: Harness::Ktest,
        need: CFG_MODULES,
        attempts: 1u8,
        exit: EX_OK,
        log: 320u32,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 6usize,
        dims: 0x0297u16,
        harness: Harness::Twosys,
        need: CFG_TWOSYS,
        attempts: 1u8,
        exit: EX_OK,
        log: 256u32,
        default: false,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 7usize,
        dims: 0x0d29u16,
        harness: Harness::Musl,
        need: CFG_MUSL,
        attempts: 1u8,
        exit: EX_OK,
        log: RED_RATE,
        default: false,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 8usize,
        dims: 0x10d3u16,
        harness: Harness::Loop,
        need: CFG_LOOP,
        attempts: 2u8,
        exit: EX_OK,
        log: 256u32,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 9usize,
        dims: 0x0764u16,
        harness: Harness::Direct,
        need: CFG_NONE,
        attempts: 1u8,
        exit: EX_FAIL,
        log: 256u32,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 10usize,
        dims: 0x181fu16,
        harness: Harness::Glibc,
        need: CFG_GLIBC,
        attempts: 1u8,
        exit: EX_OK,
        log: 32u32,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 11usize,
        dims: 0x0000u16,
        harness: Harness::Ktest,
        need: CFG_MODULES,
        attempts: 1u8,
        exit: EX_OK,
        log: 256u32,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 12usize,
        dims: 0x003fu16,
        harness: Harness::Busybox,
        need: CFG_BUSYBOX,
        attempts: 1u8,
        exit: EX_OK,
        log: 512u32,
        default: false,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 13usize,
        dims: 0x024fu16,
        harness: Harness::Direct,
        need: CFG_ROOT,
        attempts: 1u8,
        exit: EX_OK,
        log: 256u32,
        default: true,
        anchor: true,
        cfg: CFG_NO_ROOT,
    },
];

/// 14 格申报表。前 8 格与现场结论六维全同；后 6 格各在 `WANT_NDX` 指的那一维分叉。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 640u32,
        note: Note::Sealed,
        default: true,
        tag: "f11661-bb-notfound",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 576u32,
        note: Note::Sealed,
        default: true,
        tag: "f11662-argv0-unregistered",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 512u32,
        note: Note::Sealed,
        default: true,
        tag: "f11663-trimmed-applet",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 448u32,
        note: Note::Sealed,
        default: true,
        tag: "f11664-notfound-exit",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 384u32,
        note: Note::Sealed,
        default: true,
        tag: "f11665-cmd-notfound-text",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 320u32,
        note: Note::Sealed,
        default: true,
        tag: "f11666-path-notfound",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 256u32,
        note: Note::Sealed,
        default: false,
        tag: "f11667-help-exit0",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: RED_RATE,
        note: Note::Sealed,
        default: false,
        tag: "f11668-applet-usage",
    },
    Claim {
        success: false,
        stage: Stage::Seal,
        code: KC_EFLAKE,
        value: 2u32,
        note: Note::FlakeOnce,
        default: true,
        tag: "f11669-unknown-option",
    },
    Claim {
        success: false,
        stage: Stage::Exec,
        code: TAG_OK,
        value: 1u32,
        note: Note::ExitBad,
        default: true,
        tag: "f11670-bad-value",
    },
    Claim {
        success: false,
        stage: Stage::Tally,
        code: KC_EVALUE,
        value: 64u32,
        note: Note::LogThin,
        default: true,
        tag: "f11671-case-alias",
    },
    Claim {
        success: false,
        stage: Stage::Dims,
        code: KC_EDIMS,
        value: 0u32,
        note: Note::DimHole,
        default: true,
        tag: "f11672-install-skip",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 512u32,
        note: Note::Sealed,
        default: true,
        tag: "f11673-table-diff",
    },
    Claim {
        success: true,
        stage: Stage::Ready,
        code: KC_ESKIP,
        value: 0u32,
        note: Note::CfgMissing,
        default: true,
        tag: "f11674-applet-text",
    },
];

/// 现场结论是否判绿（逐槽）。
pub const WANT_OK: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, false, false, false, false, true, false,
];

/// 现场停留档（逐槽）。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Retry,
    Stage::Exec,
    Stage::Tally,
    Stage::Dims,
    Stage::Seal,
    Stage::Ready,
];

/// 现场码面（逐槽）。
pub const WANT_CODE: [u32; CORPUS] = [
    TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, KC_EFLAKE, KC_ECODE, KC_EVALUE,
    KC_EDIMS, TAG_OK, KC_ESKIP,
];

/// 现场计数（逐槽）。
pub const WANT_VALUE: [u32; CORPUS] = [
    640u32, 576u32, 512u32, 448u32, 384u32, 320u32, 256u32, RED_RATE, 2u32, 1u32, 32u32, 0u32,
    512u32, 0u32,
];

/// 现场归因（逐槽）。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Sealed,
    Note::Sealed,
    Note::Sealed,
    Note::Sealed,
    Note::Sealed,
    Note::Sealed,
    Note::Sealed,
    Note::Sealed,
    Note::FlakeOnce,
    Note::ExitBad,
    Note::LogThin,
    Note::DimBad,
    Note::Sealed,
    Note::CfgMissing,
];

/// 现场默认位（逐槽）。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, true, true, true, true, false, false, true, true, true, true, false, true,
];

/// 判据档（逐槽）。
pub const WANT_TIER: [Tier; CORPUS] = [
    Tier::Anchored,
    Tier::Anchored,
    Tier::Anchored,
    Tier::Anchored,
    Tier::Anchored,
    Tier::Anchored,
    Tier::Declared,
    Tier::Declared,
    Tier::Unrun,
    Tier::Unrun,
    Tier::Unrun,
    Tier::Unrun,
    Tier::Declared,
    Tier::Void,
];

/// 申报表分叉的维度号（前 8 槽为 `DIM_NONE`）。
pub const WANT_NDX: [usize; CORPUS] = [
    DIM_NONE, DIM_NONE, DIM_NONE, DIM_NONE, DIM_NONE, DIM_NONE, DIM_NONE, DIM_NONE, 1usize, 2usize,
    3usize, 4usize, 5usize, 0usize,
];

/// 每槽应有的判定故障码（接受槽为通过码）。
pub const WANT_FAULT_CODE: [u32; CORPUS] = [
    TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, KC_ESTAGE, KC_ECODE, KC_EVALUE,
    KC_ENOTE, KC_EDEFAULT, KC_ESTATE,
];

/// 每槽短名（与 `CLAIMS` 逐格对齐）。
pub const WANT_TAGS: [&'static str; CORPUS] = [
    "f11661-bb-notfound",
    "f11662-argv0-unregistered",
    "f11663-trimmed-applet",
    "f11664-notfound-exit",
    "f11665-cmd-notfound-text",
    "f11666-path-notfound",
    "f11667-help-exit0",
    "f11668-applet-usage",
    "f11669-unknown-option",
    "f11670-bad-value",
    "f11671-case-alias",
    "f11672-install-skip",
    "f11673-table-diff",
    "f11674-applet-text",
];

/// 取形态。
pub fn shape_at(i: usize) -> Option<Shape> {
    match SHAPES.get(i) {
        Some(s) => Some(*s),
        None => None,
    }
}

/// 取申报。
pub fn claim_at(i: usize) -> Option<Claim> {
    match CLAIMS.get(i) {
        Some(c) => Some(*c),
        None => None,
    }
}

/// 取现场结论。
pub fn outcome_at(i: usize) -> Option<Outcome> {
    match shape_at(i) {
        Some(s) => Some(define(&s)),
        None => None,
    }
}

/// 取一格语料。
pub fn case_at(i: usize) -> Option<Case> {
    match (shape_at(i), claim_at(i)) {
        (Some(shape), Some(claim)) => Some(Case { shape, claim }),
        _ => None,
    }
}

/// 空格（`cases()` 的填充值，任何字段都不参与判定）。
pub const EMPTY_SHAPE: Shape = Shape {
    idx: 0usize,
    dims: 0x0000u16,
    harness: Harness::Direct,
    need: CFG_NONE,
    attempts: 1u8,
    exit: EX_OK,
    log: 0u32,
    default: false,
    anchor: false,
    cfg: CFG_NONE,
};

/// 空申报。
pub const EMPTY_CLAIM: Claim = Claim {
    success: false,
    stage: Stage::Manifest,
    code: 0u32,
    value: 0u32,
    note: Note::Clean,
    default: false,
    tag: "",
};

/// 空格语料。
pub const EMPTY_CASE: Case = Case {
    shape: EMPTY_SHAPE,
    claim: EMPTY_CLAIM,
};

/// 全部语料。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [EMPTY_CASE; CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        match (SHAPES.get(i), CLAIMS.get(i), out.get_mut(i)) {
            (Some(s), Some(c), Some(dst)) => {
                *dst = Case {
                    shape: *s,
                    claim: *c,
                };
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// 第 i 槽的预期判定故障。
pub fn expected_fault(i: usize) -> Fault {
    if i < ACCEPT {
        return Fault::None;
    }
    let k = i - ACCEPT;
    match REJECT_FAULTS.get(k) {
        Some(f) => *f,
        None => Fault::None,
    }
}

/// 判定结论与预期一致的槽数。
pub fn matched_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if judge(&c) == expected_fault(i) {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 申报与现场完全同形（=假红）的拒绝槽数。
pub fn conflict_count() -> u32 {
    let mut n = 0u32;
    let mut i = ACCEPT;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            if c.claim.divergence(&c.actual()) == 0 {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 现场判绿的槽数。
pub fn live_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(o) = outcome_at(i) {
            if o.ok() {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 现场未判绿的槽数。
pub fn dead_count() -> u32 {
    CORPUS as u32 - live_count()
}

/// 四档计数。
pub fn tier_counts() -> [u32; TIER_COUNT] {
    let mut out = [0u32; TIER_COUNT];
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(o) = outcome_at(i) {
            let t = tier_of(&o).rank();
            if let Some(c) = out.get_mut(t) {
                *c += 1u32;
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 矩阵（四档逐档一格）
// ---------------------------------------------------------------------------

/// 一行：某档的总数与命中数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    /// 档。
    pub tier: Tier,
    /// 本档槽数。
    pub total: u32,
    /// 本档命中（档位与 `WANT_TIER` 一致）的槽数。
    pub good: u32,
}

impl Row {
    /// 空行。
    pub const fn empty(t: Tier) -> Row {
        Row {
            tier: t,
            total: 0u32,
            good: 0u32,
        }
    }

    /// 是否完好。
    pub fn perfect(&self) -> bool {
        self.total == self.good
    }

    /// 渲染一格。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(self.tier.name());
        s.push('=');
        s.push_str(dec_text(self.good).as_str());
        s.push('/');
        s.push_str(dec_text(self.total).as_str());
        s
    }
}

/// 四档矩阵。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    /// 四行。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 构建。
    pub fn build() -> Matrix {
        let mut rows = [
            Row::empty(Tier::Anchored),
            Row::empty(Tier::Declared),
            Row::empty(Tier::Void),
            Row::empty(Tier::Unrun),
        ];
        let mut i = 0usize;
        while i < CORPUS {
            if let Some(o) = outcome_at(i) {
                let t = tier_of(&o);
                let r = t.rank();
                let hit = match WANT_TIER.get(i) {
                    Some(w) => *w == t,
                    None => false,
                };
                if let Some(row) = rows.get_mut(r) {
                    row.total += 1u32;
                    if hit {
                        row.good += 1u32;
                    }
                }
            }
            i += 1;
        }
        Matrix { rows }
    }

    /// 取某档行。
    pub fn row(&self, t: Tier) -> Option<Row> {
        self.rows.get(t.rank()).copied()
    }

    /// 槽数。
    pub fn slots(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.total;
            }
            i += 1;
        }
        n
    }

    /// 命中数。
    pub fn goods(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n += r.good;
            }
            i += 1;
        }
        n
    }

    /// 完好的行数。
    pub fn perfect_rows(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.perfect() && r.total != 0u32 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if i != 0 {
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
// 闸门位（清单级）
// ---------------------------------------------------------------------------

/// 八道闸的通过位。位序与 `STAGE_CHAIN` 一致。
pub fn gate_bits() -> u32 {
    let mut bits = 0u32;
    if manifest_count_ok() {
        bits |= 1u32 << 0;
    }
    if tags_unique_ok() && segs_unique_ok() && deps_ok() {
        bits |= 1u32 << 1;
    }
    if dim_ok() {
        bits |= 1u32 << 2;
    }
    if ready_specs_ok() {
        bits |= 1u32 << 3;
    }
    if exec_specs_ok() {
        bits |= 1u32 << 4;
    }
    if retry_specs_ok() {
        bits |= 1u32 << 5;
    }
    if tally_ok() {
        bits |= 1u32 << 6;
    }
    if seal_ok() {
        bits |= 1u32 << 7;
    }
    bits
}

/// 通过的闸数。
pub fn gates_passed() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < GATE_COUNT {
        if (gate_bits() & (1u32 << i)) != 0 {
            n += 1u32;
        }
        i += 1;
    }
    n
}

/// 19 条判据的 harness 需要是否都能被 CFG_ALL 点亮。
pub fn ready_specs_ok() -> bool {
    let mut i = 0usize;
    while i < BENCH {
        match SPECS.get(i) {
            Some(s) => {
                let need = s.need | s.harness.requires();
                if (CFG_ALL & need) != need {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 接受槽是否都跑到执行闸之后。
pub fn exec_specs_ok() -> bool {
    let mut i = 0usize;
    while i < ACCEPT {
        match outcome_at(i) {
            Some(o) => {
                if o.stage.rank() < Stage::Exec.rank() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 接受槽是否都没有抖动。
pub fn retry_specs_ok() -> bool {
    let mut i = 0usize;
    while i < ACCEPT {
        match SHAPES.get(i) {
            Some(s) => {
                if s.attempts != 1u8 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 汇总是否成立：命中数满槽、无假红、通过率满。
pub fn tally_ok() -> bool {
    matched_count() == CORPUS as u32 && conflict_count() == 0u32
}

/// 封印是否成立：接受槽都带锚。
pub fn seal_ok() -> bool {
    let mut i = 0usize;
    while i < ACCEPT {
        match SHAPES.get(i) {
            Some(s) => {
                if !s.anchor {
                    return false;
                }
                if outcome_at(i).map(|o| o.stage) != Some(Stage::Seal) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}
// ---------------------------------------------------------------------------
// 报告
// ---------------------------------------------------------------------------

/// 一次生态面运行的账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 断言槽数。
    pub total: u32,
    /// 接受槽数。
    pub accept: u32,
    /// 拒绝槽数。
    pub reject: u32,
    /// 锚定档数。
    pub anchored: u32,
    /// 申报档数。
    pub declared: u32,
    /// 空洞档数。
    pub void: u32,
    /// 未达档数。
    pub unrun: u32,
    /// 判定命中槽数。
    pub matched: u32,
    /// 假红槽数（申报拒绝了却没分叉）。
    pub conflicts: u32,
    /// 断言覆盖次数。
    pub cover: u32,
    /// 留洞维度数。
    pub holes: u32,
    /// 过薄维度数。
    pub fragile: u32,
    /// 冗余维度数。
    pub dense: u32,
    /// 通过的闸数。
    pub gates: u32,
    /// 是否已封印。
    pub sealed: bool,
    /// 是否抖动过一次。
    pub flaked: bool,
}

impl Report {
    /// 空账。
    pub const fn empty() -> Report {
        Report {
            total: 0u32,
            accept: 0u32,
            reject: 0u32,
            anchored: 0u32,
            declared: 0u32,
            void: 0u32,
            unrun: 0u32,
            matched: 0u32,
            conflicts: 0u32,
            cover: 0u32,
            holes: 0u32,
            fragile: 0u32,
            dense: 0u32,
            gates: 0u32,
            sealed: false,
            flaked: false,
        }
    }

    /// 通过率（千分制，四舍五入）。
    pub fn rate(&self) -> u32 {
        if self.total == 0u32 {
            return 0u32;
        }
        (self.matched.saturating_mul(1000u32).saturating_add(self.total / 2u32)) / self.total
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.matched == self.total
            && self.total != 0u32
            && self.conflicts == 0u32
            && self.gates == GATE_COUNT as u32
    }

    /// 闸门是否全过。
    pub fn gate(&self) -> bool {
        self.gates == GATE_COUNT as u32
    }

    /// 是否无抖动。
    pub fn flake_free(&self) -> bool {
        !self.flaked
    }

    /// 四档之和是否等于槽数。
    pub fn tiers_sum(&self) -> u32 {
        self.anchored
            .saturating_add(self.declared)
            .saturating_add(self.void)
            .saturating_add(self.unrun)
    }

    /// 渲染（四行，逐字）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("ktest=KC80 bench=");
        s.push_str(dec_text(BENCH as u32).as_str());
        s.push_str(" total=");
        s.push_str(dec_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(dec_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(dec_text(self.reject).as_str());
        s.push_str(" matched=");
        s.push_str(dec_text(self.matched).as_str());
        s.push_str(" conflicts=");
        s.push_str(dec_text(self.conflicts).as_str());
        s.push_str(" rate=");
        s.push_str(dec_text(self.rate()).as_str());
        s.push('\n');
        s.push_str("tiers anchored=");
        s.push_str(dec_text(self.anchored).as_str());
        s.push_str(" declared=");
        s.push_str(dec_text(self.declared).as_str());
        s.push_str(" void=");
        s.push_str(dec_text(self.void).as_str());
        s.push_str(" unrun=");
        s.push_str(dec_text(self.unrun).as_str());
        s.push('\n');
        s.push_str("dims cover=");
        s.push_str(dec_text(self.cover).as_str());
        s.push_str(" holes=");
        s.push_str(dec_text(self.holes).as_str());
        s.push_str(" fragile=");
        s.push_str(dec_text(self.fragile).as_str());
        s.push_str(" dense=");
        s.push_str(dec_text(self.dense).as_str());
        s.push_str(" full=");
        s.push_str(dec_text(dim_full_count()).as_str());
        s.push('\n');
        s.push_str("gate=");
        s.push_str(dec_text(self.gates).as_str());
        s.push_str("/");
        s.push_str(dec_text(GATE_COUNT as u32).as_str());
        s.push_str(" seal=");
        s.push_str(match self.sealed {
            true => "ok",
            false => "bad",
        });
        s.push_str(" green=");
        s.push_str(match self.green() {
            true => "true",
            false => "false",
        });
        s.push('\n');
        s
    }
}

/// 全绿渲染的逐字锚。改动任何一个计数都必须同步改这一行。
pub const RENDER_OK: &str = "ktest=KC80 bench=19 total=14 accept=8 reject=6 matched=14 conflicts=0 rate=1000\ntiers anchored=6 declared=3 void=1 unrun=4\ndims cover=116 holes=0 fragile=0 dense=0 full=13\ngate=8/8 seal=ok green=true\n";

/// 记账（可带一次注入）。
pub fn build_report(inj: Option<(usize, Fault)>) -> Report {
    let mut matched = 0u32;
    let mut conflicts = 0u32;
    let mut flaked = false;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            let got = match inj {
                Some((slot, f)) if slot == i => f,
                _ => judge(&c),
            };
            if got == expected_fault(i) {
                matched += 1u32;
            }
            if i >= ACCEPT && c.claim.divergence(&c.actual()) == 0u32 {
                conflicts += 1u32;
            }
            let a = c.actual();
            if a.stage == Stage::Retry && a.code == KC_EFLAKE {
                flaked = true;
            }
        }
        i += 1;
    }
    let tc = tier_counts();
    let mut rep = Report {
        total: CORPUS as u32,
        accept: ACCEPT as u32,
        reject: REJECT as u32,
        anchored: 0u32,
        declared: 0u32,
        void: 0u32,
        unrun: 0u32,
        matched,
        conflicts,
        cover: dim_asserts_total(),
        holes: dim_holes(),
        fragile: dim_fragile(),
        dense: dim_dense(),
        gates: gates_passed(),
        sealed: seal_ok() && dimension_count_ok(),
        flaked,
    };
    if let Some(v) = tc.get(0) {
        rep.anchored = *v;
    }
    if let Some(v) = tc.get(1) {
        rep.declared = *v;
    }
    if let Some(v) = tc.get(2) {
        rep.void = *v;
    }
    if let Some(v) = tc.get(3) {
        rep.unrun = *v;
    }
    rep
}

/// 维度计数与并集是否都满（封印的账面前提）。
pub fn dimension_count_ok() -> bool {
    dim_union() == DIM_FULL && dim_full_count() == DIMS as u32
}

/// 全量运行。
pub fn run() -> Report {
    build_report(None)
}

/// 带注入运行；槽越界返回 `KC_ESLOT`。
pub fn run_with(inj: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = inj {
        if slot >= CORPUS {
            return Err(KC_ESLOT);
        }
    }
    Ok(build_report(inj))
}

/// 码名。
pub fn code_name(c: u32) -> &'static str {
    match c {
        TAG_OK => "ok",
        KC_ESTATE => "estate",
        KC_ESTAGE => "stage",
        KC_ECODE => "code",
        KC_EVALUE => "value",
        KC_ENOTE => "note",
        KC_EDEFAULT => "default",
        KC_ECOUNT => "count",
        KC_EUNIQ => "uniq",
        KC_EDIMS => "dims",
        KC_ESKIP => "skip",
        KC_EFLAKE => "flake",
        KC_EBLOCK => "block",
        KC_ESLOT => "slot",
        _ => "unknown",
    }
}

/// 一格的可读说明。
pub fn detail(slot: usize, f: Fault) -> String {
    let mut s = String::new();
    s.push_str("slot=");
    s.push_str(dec_text(slot as u32).as_str());
    s.push_str(" fault=");
    s.push_str(f.name());
    s.push_str(" code=");
    s.push_str(hex_text(f.code(), 6usize).as_str());
    s.push_str(" dim=");
    s.push_str(match f.dim() {
        Some(d) => d.name(),
        None => "none",
    });
    s
}

/// 一格的诊断行（含现场结论）。
pub fn diagnostic(slot: usize, f: Fault) -> String {
    let mut s = detail(slot, f);
    match outcome_at(slot) {
        Some(o) => {
            s.push_str(" stage=");
            s.push_str(o.stage.name());
            s.push_str(" value=");
            s.push_str(dec_text(o.value).as_str());
            s.push_str(" note=");
            s.push_str(o.note.name());
        }
        None => {
            s.push_str(" stage=none value=0 note=missing");
        }
    }
    s
}

// ---------------------------------------------------------------------------
// 阶段闸（单调，可关闭）
// ---------------------------------------------------------------------------

/// 阶段闸：只许前进，不许倒退；关闭后一切进入都判状态错。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    /// 已走过的闸位。
    marks: u32,
    /// 当前最高档。
    at: usize,
    /// 是否已关闭。
    closed: bool,
    /// 倒退次数。
    backslides: u32,
}

impl Gate {
    /// 新闸，停在清单闸之前。
    pub const fn new() -> Gate {
        Gate {
            marks: 0u32,
            at: 0usize,
            closed: false,
            backslides: 0u32,
        }
    }

    /// 进入某一档。
    pub fn enter(&mut self, s: Stage) -> Result<(), u32> {
        if self.closed {
            return Err(KC_ESTATE);
        }
        let r = s.rank();
        if r < self.at {
            self.backslides += 1u32;
            return Err(KC_ESTAGE);
        }
        self.at = r;
        self.marks |= 1u32 << r;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// 是否关闭。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 当前档。
    pub fn at(&self) -> usize {
        self.at
    }

    /// 已走过的档数。
    pub fn walked(&self) -> u32 {
        popcount32(self.marks)
    }

    /// 倒退次数。
    pub fn backslides(&self) -> u32 {
        self.backslides
    }

    /// 是否走满八档。
    pub fn complete(&self) -> bool {
        self.walked() == GATE_COUNT as u32 && self.marks == (1u32 << GATE_COUNT) - 1u32
    }
}

/// u32 popcount。
pub fn popcount32(v: u32) -> u32 {
    let mut n = v;
    let mut c = 0u32;
    while n != 0 {
        c += n & 1u32;
        n >>= 1;
    }
    c
}

// ---------------------------------------------------------------------------
// 台账（逐槽一格，可冻结重放）
// ---------------------------------------------------------------------------

/// 台账操作的种类数。
pub const OP_COUNT: usize = SAMPLES;
/// 台账样本值的基。
pub const VAL_BASE: u32 = 100u32;
/// 台账样本值的步长。
pub const VAL_STEP: u32 = 10u32;

/// 一格样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 槽号。
    pub slot: usize,
    /// 操作号（= 槽号 + 1）。
    pub op: u8,
    /// 计数。
    pub value: u32,
    /// 是否自洽。
    pub ok: bool,
}

impl Sample {
    /// 空格。
    pub const fn empty() -> Sample {
        Sample {
            slot: 0usize,
            op: 0u8,
            value: 0u32,
            ok: false,
        }
    }

    /// 该槽应有的值。
    pub const fn want(slot: usize) -> u32 {
        VAL_BASE + (slot as u32) * VAL_STEP
    }

    /// 是否自洽（槽号、操作号、计数三面对齐）。
    pub fn consistent(&self) -> bool {
        if !self.ok {
            return false;
        }
        if self.op as usize != self.slot + 1usize {
            return false;
        }
        self.value == Sample::want(self.slot)
    }
}

/// 台账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    /// 样本格。
    samples: [Sample; SAMPLES],
    /// 已登记格数。
    len: usize,
    /// 是否冻结。
    frozen: bool,
    /// 封印根。
    root: u64,
    /// 拒收次数。
    rejects: u32,
}

impl Ledger {
    /// 空账。
    pub const fn new() -> Ledger {
        Ledger {
            samples: [Sample::empty(); SAMPLES],
            len: 0usize,
            frozen: false,
            root: 0u64,
            rejects: 0u32,
        }
    }

    /// 登记一格。槽号必须等于当前长度，否则拒收并计数。
    pub fn register(&mut self, slot: usize, op: u8, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(KC_ESTATE);
        }
        if slot != self.len {
            self.rejects += 1u32;
            return Err(KC_ESLOT);
        }
        if self.len >= SAMPLES {
            self.rejects += 1u32;
            return Err(KC_ESLOT);
        }
        let ok = op as usize == slot + 1usize && value == Sample::want(slot);
        if let Some(dst) = self.samples.get_mut(self.len) {
            *dst = Sample {
                slot,
                op,
                value,
                ok,
            };
            self.len += 1usize;
            return Ok(());
        }
        self.rejects += 1u32;
        Err(KC_ESLOT)
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 拒收次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.samples.get(i).copied()
    }

    /// 复核：逐格三面对齐后折叠指纹。
    pub fn audit(&self) -> Result<u64, u32> {
        if self.len != SAMPLES {
            return Err(KC_ECOUNT);
        }
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            match self.samples.get(i) {
                Some(s) => {
                    if !s.consistent() {
                        return Err(KC_EVALUE);
                    }
                    h = fold_word(h, s.slot as u64);
                    h = fold_byte(h, s.op);
                    h = fold_word(h, s.value as u64);
                }
                None => return Err(KC_ECOUNT),
            }
            i += 1;
        }
        Ok(h)
    }

    /// 封印：复核后记根。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(KC_ESTATE);
        }
        let h = self.audit()?;
        self.root = h;
        Ok(h)
    }

    /// 冻结：封印后拒收一切写入。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(KC_ESTATE);
        }
        let h = self.seal()?;
        self.frozen = true;
        Ok(h)
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 封印根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 重放：重算指纹并与封印根比对。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.root == 0u64 {
            return Err(KC_ESTATE);
        }
        let h = self.audit()?;
        if h != self.root {
            return Err(KC_EVALUE);
        }
        Ok(h)
    }

    /// 戳一格（只许在冻结前）。戳过之后复核必红。
    pub fn poke(&mut self, slot: usize, value: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(KC_ESTATE);
        }
        if slot >= self.len {
            return Err(KC_ESLOT);
        }
        match self.samples.get_mut(slot) {
            Some(s) => {
                s.value = value;
                s.ok = true;
                Ok(())
            }
            None => Err(KC_ESLOT),
        }
    }
}

/// 好账：14 格全对齐。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let op = (i + 1usize) as u8;
        let _ = l.register(i, op, Sample::want(i));
        i += 1;
    }
    l
}

/// 坏账：三种注入各断一处。
pub fn build_ledger_broken(b: u8) -> Ledger {
    let mut l = build_ledger();
    match b {
        0u8 => {
            let _ = l.poke(3usize, 0u32);
        }
        1u8 => {
            let _ = l.poke(7usize, Sample::want(7usize).saturating_add(1u32));
        }
        _ => {
            let _ = l.poke(11usize, 0u32);
        }
    }
    l
}

/// 四注入：判定面一注、接受面一注、越界一注、台账一注。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let a = match run_with(Some((9usize, Fault::None))) {
        Ok(r) => r.matched + 1u32 == CORPUS as u32,
        Err(_) => false,
    };
    if let Some(v) = out.get_mut(0) {
        *v = a;
    }
    let b = match run_with(Some((3usize, Fault::Stage))) {
        Ok(r) => r.matched + 1u32 == CORPUS as u32,
        Err(_) => false,
    };
    if let Some(v) = out.get_mut(1) {
        *v = b;
    }
    let c = match run_with(Some((CORPUS, Fault::None))) {
        Ok(_) => false,
        Err(e) => e == KC_ESLOT,
    };
    if let Some(v) = out.get_mut(2) {
        *v = c;
    }
    let d = {
        let mut bad = build_ledger();
        let _ = bad.poke(5usize, 1u32);
        bad.audit().is_err() && bad.first_break().is_some()
    };
    if let Some(v) = out.get_mut(3) {
        *v = d;
    }
    out
}

impl Ledger {
    /// 第一处不自洽的格号。
    pub fn first_break(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            match self.samples.get(i) {
                Some(s) => {
                    if !s.consistent() {
                        return Some(i);
                    }
                }
                None => return Some(i),
            }
            i += 1;
        }
        None
    }
}
// ---------------------------------------------------------------------------
// 十三项自检
// ---------------------------------------------------------------------------

/// 段基址、通过码与 13 个错误码的位次。
pub fn block_ok() -> bool {
    if SEG_BASE != 0x6d2f0u32 {
        return false;
    }
    if TAG_OK != SEG_BASE {
        return false;
    }
    let codes = [
        KC_ESTATE, KC_ESTAGE, KC_ECODE, KC_EVALUE, KC_ENOTE, KC_EDEFAULT, KC_ECOUNT, KC_EUNIQ,
        KC_EDIMS, KC_ESKIP, KC_EFLAKE, KC_EBLOCK, KC_ESLOT,
    ];
    let mut i = 0usize;
    while i < 13 {
        match codes.get(i) {
            Some(c) => {
                if *c != SEG_BASE + (i as u32) + 1u32 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 13 个错误码两两互异，且码名可回读。
pub fn codes_ok() -> bool {
    let codes = [
        KC_ESTATE, KC_ESTAGE, KC_ECODE, KC_EVALUE, KC_ENOTE, KC_EDEFAULT, KC_ECOUNT, KC_EUNIQ,
        KC_EDIMS, KC_ESKIP, KC_EFLAKE, KC_EBLOCK, KC_ESLOT,
    ];
    let mut i = 0usize;
    while i < 13 {
        match codes.get(i) {
            Some(a) => {
                if code_name(*a) == "unknown" {
                    return false;
                }
                let mut j = i + 1usize;
                while j < 13 {
                    if let Some(b) = codes.get(j) {
                        if a == b {
                            return false;
                        }
                    }
                    j += 1usize;
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    code_name(TAG_OK) == "ok"
}

/// 枚举表：名唯一、序号连续、反查自洽。
pub fn enums_ok() -> bool {
    let mut i = 0usize;
    while i < DIMS {
        match Dim::of_rank(i) {
            Some(d) => {
                if d.rank() != i {
                    return false;
                }
                if d.name().is_empty() {
                    return false;
                }
                if popcount16(DIM_FULL) != DIMS as u32 {
                    return false;
                }
                if (DIM_FULL & d.bit()) == 0 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    if Dim::of_rank(DIMS).is_some() {
        return false;
    }
    let mut k = 0usize;
    while k < GATE_COUNT {
        match Stage::of_rank(k) {
            Some(s) => {
                if s.rank() != k {
                    return false;
                }
                if let Some(c) = STAGE_CHAIN.get(k) {
                    if *c != s {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            None => return false,
        }
        k += 1usize;
    }
    let mut h = 0usize;
    while h < HARNESS_COUNT {
        match Harness::of_index(h) {
            Some(x) => {
                if x.index() != h || x.name().is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        h += 1usize;
    }
    let mut n = 0usize;
    while n < TIER_COUNT {
        match Tier::all().get(n) {
            Some(t) => {
                if t.rank() != n {
                    return false;
                }
                if t.name().is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        n += 1usize;
    }
    Note::all().len() == NOTE_COUNT
}

/// 清单：19 条、卡号连续、短名与段基址唯一、依赖在前。
pub fn bench_ok() -> bool {
    if !manifest_ok() {
        return false;
    }
    if SPECS.len() != BENCH {
        return false;
    }
    if default_count() == 0 || default_count() == BENCH {
        return false;
    }
    match topo_order() {
        Some(t) => {
            let mut i = 0usize;
            while i < BENCH {
                if t.get(i) != Some(&i) {
                    return false;
                }
                i += 1usize;
            }
        }
        None => return false,
    }
    if spec_index_of(CARD_MIN) != Some(0usize) {
        return false;
    }
    if spec_index_of(CARD_MAX) != Some(BENCH - 1usize) {
        return false;
    }
    spec_index_of(CARD_MIN - 1u16).is_none() && spec_index_of(CARD_MAX + 1u16).is_none()
}

/// 维度面：116 次断言、13 维全铺、无洞无薄无冗余。
pub fn dims_ok() -> bool {
    if !dim_ok() {
        return false;
    }
    if dim_asserts_total() != WANT_DIM_ASSERTS {
        return false;
    }
    if dim_union() != DIM_FULL || dim_full_count() != DIMS as u32 {
        return false;
    }
    if dim_holes() != 0 || dim_fragile() != 0 || dim_dense() != 0 {
        return false;
    }
    let mut i = 0usize;
    while i < DIMS {
        match Dim::of_rank(i) {
            Some(d) => {
                let c = dim_cover(d);
                if c < WANT_DIM_FLOOR || c > (BENCH as u32) - 1u32 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    // 掩码分解：17×6 + 2×7。
    let mut six = 0u32;
    let mut seven = 0u32;
    let mut k = 0usize;
    while k < BENCH {
        match SPECS.get(k) {
            Some(s) => match popcount16(s.dims) {
                6u32 => six += 1u32,
                7u32 => seven += 1u32,
                _ => return false,
            },
            None => return false,
        }
        k += 1usize;
    }
    six == 17u32 && seven == 2u32
}

/// 定义：14 格现场结论与 WANT 六维逐格对齐。
pub fn define_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let o = match outcome_at(i) {
            Some(x) => x,
            None => return false,
        };
        let ok = match WANT_OK.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if o.ok() != ok {
            return false;
        }
        if Some(&o.stage) != WANT_STAGE.get(i) {
            return false;
        }
        if Some(&o.code) != WANT_CODE.get(i) {
            return false;
        }
        if Some(&o.value) != WANT_VALUE.get(i) {
            return false;
        }
        if Some(&o.note) != WANT_NOTE.get(i) {
            return false;
        }
        if Some(&o.default) != WANT_DEFAULT.get(i) {
            return false;
        }
        match WANT_TIER.get(i) {
            Some(w) => {
                if tier_of(&o) != *w {
                    return false;
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    true
}

/// 判定：接受槽全 Ok，拒绝槽逐槽命中，且注序先于维。
pub fn judge_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        let f = judge(&c);
        if f != expected_fault(i) {
            return false;
        }
        if Some(&f.code()) != WANT_FAULT_CODE.get(i) {
            return false;
        }
        i += 1usize;
    }
    // 注序：同时错两维时报前一维。
    let c = match case_at(9usize) {
        Some(x) => x,
        None => return false,
    };
    let a = c.actual();
    let mut two = c.claim;
    two.code = TAG_OK;
    two.value = 7u32;
    if judge_against(&two, &a) != Fault::Code {
        return false;
    }
    let mut three = two;
    three.stage = Stage::Manifest;
    if judge_against(&three, &a) != Fault::Stage {
        return false;
    }
    let mut four = three;
    four.success = true;
    judge_against(&four, &a) == Fault::Estate
}

/// 语料：14 槽、8 接受 6 拒绝、拒绝槽恰在一维分叉、短名与清单对齐。
pub fn corpus_ok() -> bool {
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if ACCEPT + REJECT != CORPUS {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        let a = c.actual();
        let ndx = match WANT_NDX.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if i < ACCEPT {
            if c.claim.divergence(&a) != 0u32 {
                return false;
            }
            if ndx != DIM_NONE {
                return false;
            }
        } else {
            if c.claim.divergence(&a) != 1u32 {
                return false;
            }
            let d = match Dim::of_rank(ndx) {
                Some(x) => x,
                None => return false,
            };
            if !c.claim.differs(&a, d) {
                return false;
            }
        }
        let tag = match WANT_TAGS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c.tag() != tag || c.claim.tag != tag {
            return false;
        }
        if let Some(s) = spec_at(c.shape.idx) {
            if s.tag != tag {
                return false;
            }
            // 第 11 槽是故意注入的维度故障：清单里该卡有掩码，现场报 0 维。
            // 其余槽必须与清单逐位相等，否则"互校"就漏了维度这一面。
            if c.shape.dims == 0u16 {
                if s.dims == 0u16 || i != 11usize {
                    return false;
                }
            } else if s.dims != c.shape.dims {
                return false;
            }
        } else {
            return false;
        }
        // 短名互不重复。
        let mut j = i + 1usize;
        while j < CORPUS {
            if let Some(d) = claim_at(j) {
                if d.tag == c.claim.tag {
                    return false;
                }
            }
            j += 1usize;
        }
        i += 1usize;
    }
    matched_count() == CORPUS as u32 && conflict_count() == 0u32
}

/// 四档矩阵：逐行满载、四档之和等于槽数。
pub fn matrix_ok() -> bool {
    let m = Matrix::build();
    if m.slots() != CORPUS as u32 {
        return false;
    }
    if m.goods() != CORPUS as u32 {
        return false;
    }
    if m.perfect_rows() != TIER_COUNT as u32 {
        return false;
    }
    let tc = tier_counts();
    let mut i = 0usize;
    while i < TIER_COUNT {
        let t = match Tier::all().get(i) {
            Some(x) => *x,
            None => return false,
        };
        let r = match m.row(t) {
            Some(x) => x,
            None => return false,
        };
        match tc.get(i) {
            Some(n) => {
                if r.total != *n || r.good != *n {
                    return false;
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    m.render().is_empty() == false
}

/// 渲染：四行逐字等于锚，绿、率、闸三问同时为真。
pub fn render_ok() -> bool {
    let r = run();
    if r.render() != RENDER_OK {
        return false;
    }
    if !r.green() || !r.gate() {
        return false;
    }
    if r.rate() != RATE_FULL {
        return false;
    }
    if r.tiers_sum() != r.total {
        return false;
    }
    if r.cover != WANT_DIM_ASSERTS {
        return false;
    }
    if r.conflicts != 0u32 || r.matched != r.total {
        return false;
    }
    let empty = Report::empty();
    empty.rate() == 0u32 && !empty.green() && empty.render().len() > 0usize
}

/// 闸门：单调前进、倒退报次序错、关闭后报状态错。
pub fn gate_ok() -> bool {
    let mut g = Gate::new();
    let mut i = 0usize;
    while i < GATE_COUNT {
        let s = match Stage::of_rank(i) {
            Some(x) => x,
            None => return false,
        };
        if g.enter(s).is_err() {
            return false;
        }
        i += 1usize;
    }
    if !g.complete() || g.walked() != GATE_COUNT as u32 {
        return false;
    }
    if g.enter(Stage::Manifest) != Err(KC_ESTAGE) {
        return false;
    }
    if g.backslides() != 1u32 {
        return false;
    }
    if g.enter(Stage::Seal).is_err() {
        return false;
    }
    g.close();
    if g.enter(Stage::Seal) != Err(KC_ESTATE) {
        return false;
    }
    if !g.is_closed() || g.at() != Stage::Seal.rank() {
        return false;
    }
    // 逐档退出码与档位一一对应。
    let mut k = 0usize;
    while k < GATE_COUNT {
        let s = match Stage::of_rank(k) {
            Some(x) => x,
            None => return false,
        };
        if s != Stage::Seal && s.next().rank() != k + 1usize {
            return false;
        }
        k += 1usize;
    }
    Stage::Seal.next() == Stage::Seal
}

/// 台账：14 格复核、三注入必红、冻结后拒写、重放自洽。
pub fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.len() != SAMPLES || l.rejects() != 0u32 || l.is_frozen() {
        return false;
    }
    if l.audit().is_err() {
        return false;
    }
    let mut b = 0u8;
    while b < 3u8 {
        let broken = build_ledger_broken(b);
        if broken.audit().is_ok() {
            return false;
        }
        if broken.first_break().is_none() {
            return false;
        }
        b += 1u8;
    }
    let mut x = build_ledger();
    if x.register(SAMPLES, 15u8, 240u32).is_ok() {
        return false;
    }
    if x.rejects() != 1u32 {
        return false;
    }
    if x.register(0usize, 1u8, Sample::want(0usize)).is_ok() {
        return false;
    }
    if x.rejects() != 2u32 {
        return false;
    }
    let root = match x.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root == 0u64 || !x.is_frozen() {
        return false;
    }
    if x.root() != root {
        return false;
    }
    match x.replay() {
        Ok(v) => {
            if v != root {
                return false;
            }
        }
        Err(_) => return false,
    }
    if x.freeze().is_ok() {
        return false;
    }
    if x.poke(0usize, 7u32).is_ok() {
        return false;
    }
    if x.register(0usize, 1u8, Sample::want(0usize)).is_ok() {
        return false;
    }
    if x.audit().is_err() {
        return false;
    }
    if x.sample(SAMPLES).is_some() {
        return false;
    }
    if x.sample(0usize).is_none() {
        return false;
    }
    let mut fresh = Ledger::new();
    if fresh.replay().is_ok() || fresh.freeze().is_ok() {
        return false;
    }
    if fresh.len() != 0usize || fresh.rejects() != 0u32 {
        return false;
    }
    true
}

/// 四注入：判定、接受、越界、台账各一，全部必须被抓到。
pub fn inject_ok() -> bool {
    let r = inject_four();
    let mut i = 0usize;
    while i < 4 {
        match r.get(i) {
            Some(v) => {
                if !*v {
                    return false;
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    match run_with(None) {
        Ok(x) => {
            if !x.green() {
                return false;
            }
        }
        Err(_) => return false,
    }
    match run_with(Some((CORPUS + 7usize, Fault::None))) {
        Ok(_) => false,
        Err(e) => e == KC_ESLOT,
    }
}

/// 十三项自检入口。
pub fn run_unx11680_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11680_b24_ktest_agg");
    s.add("KC01-段基址与十三码位次", block_ok(), "static");
    s.add("KC02-十三码互异可回读", codes_ok(), "static");
    s.add("KC03-枚举表序号自洽", enums_ok(), "static");
    s.add("KC04-清单十九条与依赖在前", bench_ok(), "static");
    s.add("KC05-掩码一百一十六", dims_ok(), "static");
    s.add("KC06-定义六维逐格", define_ok(), "static");
    s.add("KC07-判定维序互斥", judge_ok(), "static");
    s.add("KC08-语料十四槽互校", corpus_ok(), "static");
    s.add("KC09-四档矩阵满载", matrix_ok(), "static");
    s.add("KC10-全绿渲染逐字", render_ok(), "static");
    s.add("KC11-八闸单调不倒退", gate_ok(), "static");
    s.add("KC12-台账复核与冻结重放", ledger_ok(), "static");
    s.add("KC13-冻结重放四注入", inject_ok(), "static");
    s
}
// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn case(i: usize) -> Case {
        match case_at(i) {
            Some(c) => c,
            None => panic!("slot {i} missing"),
        }
    }

    #[test]
    fn t01_shape_of_corpus() {
        assert_eq!(CORPUS, 14);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(TIER_COUNT, 4);
        assert_eq!(BENCH, 19);
        assert_eq!(DIMS, 13);
        assert_eq!(GATE_COUNT, 8);
    }

    #[test]
    fn t02_segment_base() {
        assert_eq!(SEG_BASE, 0x6d190);
        assert_eq!(TAG_OK, SEG_BASE);
    }

    #[test]
    fn t03_code_sequence() {
        assert_eq!(KC_ESTATE, SEG_BASE + 1);
        assert_eq!(KC_ESTAGE, SEG_BASE + 2);
        assert_eq!(KC_ECODE, SEG_BASE + 3);
        assert_eq!(KC_EVALUE, SEG_BASE + 4);
        assert_eq!(KC_ENOTE, SEG_BASE + 5);
        assert_eq!(KC_EDEFAULT, SEG_BASE + 6);
        assert_eq!(KC_ECOUNT, SEG_BASE + 7);
        assert_eq!(KC_EUNIQ, SEG_BASE + 8);
        assert_eq!(KC_EDIMS, SEG_BASE + 9);
        assert_eq!(KC_ESKIP, SEG_BASE + 10);
        assert_eq!(KC_EFLAKE, SEG_BASE + 11);
        assert_eq!(KC_EBLOCK, SEG_BASE + 12);
        assert_eq!(KC_ESLOT, SEG_BASE + 13);
    }

    #[test]
    fn t04_code_names_roundtrip() {
        assert_eq!(code_name(TAG_OK), "ok");
        assert_eq!(code_name(KC_ESTATE), "estate");
        assert_eq!(code_name(KC_EDIMS), "dims");
        assert_eq!(code_name(KC_ESLOT), "slot");
        assert_eq!(code_name(0x1234), "unknown");
    }

    #[test]
    fn t05_codes_distinct() {
        let codes = [
            KC_ESTATE, KC_ESTAGE, KC_ECODE, KC_EVALUE, KC_ENOTE, KC_EDEFAULT, KC_ECOUNT, KC_EUNIQ,
            KC_EDIMS, KC_ESKIP, KC_EFLAKE, KC_EBLOCK, KC_ESLOT,
        ];
        for a in 0..13 {
            for b in (a + 1)..13 {
                assert_ne!(codes[a], codes[b]);
            }
        }
        assert!(codes_ok());
    }

    #[test]
    fn t06_dim_bits_unique() {
        let mut seen = 0u16;
        for d in Dim::all() {
            assert_eq!(seen & d.bit(), 0);
            seen |= d.bit();
        }
        assert_eq!(seen, DIM_FULL);
        assert_eq!(popcount16(DIM_FULL), 13);
    }

    #[test]
    fn t07_dim_rank_roundtrip() {
        for i in 0..DIMS {
            let d = Dim::of_rank(i).expect("dim");
            assert_eq!(d.rank(), i);
        }
        assert!(Dim::of_rank(DIMS).is_none());
        assert_eq!(Dim::Estate.rank(), 0);
        assert_eq!(Dim::Slot.rank(), 12);
    }

    #[test]
    fn t08_stage_chain() {
        for i in 0..GATE_COUNT {
            assert_eq!(STAGE_CHAIN[i], Stage::of_rank(i).expect("stage"));
        }
        assert_eq!(STAGE_CHAIN[0], Stage::Manifest);
        assert_eq!(STAGE_CHAIN[7], Stage::Seal);
    }

    #[test]
    fn t09_stage_next() {
        assert_eq!(Stage::Manifest.next(), Stage::Identity);
        assert_eq!(Stage::Tally.next(), Stage::Seal);
        assert_eq!(Stage::Seal.next(), Stage::Seal);
        assert_eq!(Stage::of_rank(GATE_COUNT), None);
    }

    #[test]
    fn t10_tier_ranks() {
        assert_eq!(Tier::Anchored.rank(), 0);
        assert_eq!(Tier::Declared.rank(), 1);
        assert_eq!(Tier::Void.rank(), 2);
        assert_eq!(Tier::Unrun.rank(), 3);
        for i in 0..TIER_COUNT {
            assert_eq!(Tier::all()[i].rank(), i);
        }
    }

    #[test]
    fn t11_notes_are_named() {
        assert_eq!(Note::all().len(), NOTE_COUNT);
        for n in Note::all() {
            assert!(!n.name().is_empty());
        }
        assert!(Note::Sealed.is_green());
        assert!(Note::Clean.is_green());
        assert!(!Note::DimBad.is_green());
        assert!(!Note::FlakeOnce.is_green());
    }

    #[test]
    fn t12_harness_requires() {
        assert_eq!(Harness::Direct.requires(), CFG_NONE);
        assert_eq!(Harness::Musl.requires(), CFG_MUSL);
        assert_eq!(Harness::Glibc.requires(), CFG_GLIBC);
        assert_eq!(Harness::Busybox.requires(), CFG_BUSYBOX);
        assert_eq!(Harness::Loop.requires(), CFG_LOOP | CFG_TMPFS);
        assert_eq!(Harness::Twosys.requires(), CFG_TWOSYS | CFG_MUSL | CFG_GLIBC);
        assert_eq!(Harness::of_index(0), Some(Harness::Direct));
        assert_eq!(Harness::of_index(HARNESS_COUNT), None);
    }

    #[test]
    fn t13_fold_deterministic() {
        let a = fold_str(FNV_OFF, "unx11680");
        let b = fold_str(FNV_OFF, "unx11680");
        assert_eq!(a, b);
        assert_ne!(a, fold_str(FNV_OFF, "unx11641"));
    }

    #[test]
    fn t14_fold_length_sensitive() {
        assert_ne!(fold_str(FNV_OFF, "ab"), fold_str(FNV_OFF, "ba"));
        assert_ne!(fold_str(FNV_OFF, ""), fold_str(FNV_OFF, "a"));
    }

    #[test]
    fn t15_dec_text() {
        assert_eq!(dec_text(0), "0");
        assert_eq!(dec_text(7), "7");
        assert_eq!(dec_text(10), "10");
        assert_eq!(dec_text(929), "929");
        assert_eq!(dec_text(4294967295), "4294967295");
    }

    #[test]
    fn t16_hex_text() {
        assert_eq!(hex_text(0x6d190, 6), "06d190");
        assert_eq!(hex_text(0, 2), "00");
        assert_eq!(hex_text(0x0f, 2), "0f");
        assert_eq!(hex_text(0xabc, 3), "abc");
    }

    #[test]
    fn t17_i32_text() {
        assert_eq!(i32_text(0), "0");
        assert_eq!(i32_text(127), "127");
        assert_eq!(i32_text(-1), "-1");
        assert_eq!(i32_text(-139), "-139");
    }

    #[test]
    fn t18_bool_text() {
        assert_eq!(bool_text(true), "ok");
        assert_eq!(bool_text(false), "not ok");
    }

    #[test]
    fn t19_popcount() {
        assert_eq!(popcount16(0), 0);
        assert_eq!(popcount16(1), 1);
        assert_eq!(popcount16(DIM_FULL), 13);
        assert_eq!(popcount32(0xffff_ffff), 32);
        assert_eq!(popcount32(0), 0);
    }

    #[test]
    fn t20_mask_coverage_all() {
        assert_eq!(dim_holes(), 0);
        assert_eq!(dim_fragile(), 0);
        assert_eq!(dim_dense(), 0);
        assert_eq!(dim_union(), DIM_FULL);
        assert_eq!(dim_full_count(), 13);
    }

    #[test]
    fn t21_asserts_total() {
        assert_eq!(dim_asserts_total(), WANT_DIM_ASSERTS);
        assert_eq!(WANT_DIM_ASSERTS, 116);
        assert!(dim_ok());
    }

    #[test]
    fn t22_mask_decomposition() {
        let mut six = 0;
        let mut seven = 0;
        for s in SPECS.iter() {
            match popcount16(s.dims) {
                6 => six += 1,
                7 => seven += 1,
                _ => panic!("unexpected mask width"),
            }
        }
        assert_eq!(six, 17);
        assert_eq!(seven, 2);
        assert_eq!(six * 6 + seven * 7, 116);
    }

    #[test]
    fn t23_cover_floor_and_cap() {
        for i in 0..DIMS {
            let d = Dim::of_rank(i).expect("dim");
            let c = dim_cover(d);
            assert!(c >= WANT_DIM_FLOOR);
            assert!(c < BENCH as u32);
        }
    }

    #[test]
    fn t24_manifest_count() {
        assert_eq!(SPECS.len(), 19);
        assert!(manifest_count_ok());
        assert!(manifest_ok());
    }

    #[test]
    fn t25_manifest_sorted() {
        for i in 0..BENCH {
            assert_eq!(SPECS[i].card, CARD_MIN + i as u16);
        }
        assert_eq!(SPECS[0].card, 11661);
        assert_eq!(SPECS[18].card, 11679);
        assert!(manifest_sorted_ok());
    }

    #[test]
    fn t26_tags_unique() {
        for i in 0..BENCH {
            assert!(!SPECS[i].tag.is_empty());
            for j in (i + 1)..BENCH {
                assert_ne!(SPECS[i].tag, SPECS[j].tag);
            }
        }
        assert!(tags_unique_ok());
    }

    #[test]
    fn t27_segs_unique() {
        for i in 0..BENCH {
            assert_ne!(SPECS[i].seg, 0);
            for j in (i + 1)..BENCH {
                assert_ne!(SPECS[i].seg, SPECS[j].seg);
            }
        }
        assert!(segs_unique_ok());
    }

    #[test]
    fn t28_deps_within_batch() {
        assert!(deps_ok());
        assert_eq!(SPECS[0].depends, 0);
        for i in 1..BENCH {
            assert!(SPECS[i].depends >= CARD_MIN);
            assert!(SPECS[i].depends < SPECS[i].card);
        }
    }

    #[test]
    fn t29_topo_identity() {
        let t = topo_order().expect("topo");
        for i in 0..BENCH {
            assert_eq!(t[i], i);
        }
    }

    #[test]
    fn t30_spec_lookup() {
        assert_eq!(spec_index_of(11661), Some(0));
        assert_eq!(spec_index_of(11679), Some(18));
        assert_eq!(spec_index_of(11660), None);
        assert_eq!(spec_index_of(11680), None);
        assert!(spec_at(BENCH).is_none());
        assert_eq!(spec_at(0).expect("s").seg, 0x6d100);
    }

    #[test]
    fn t31_define_accept_slots() {
        for i in 0..ACCEPT {
            let o = outcome_at(i).expect("outcome");
            assert!(o.ok(), "slot {i} should be green");
            assert_eq!(o.stage, Stage::Seal);
            assert_eq!(o.code, TAG_OK);
            assert_eq!(o.note, Note::Sealed);
            assert_eq!(o.value, WANT_VALUE[i]);
        }
        assert_eq!(live_count(), 8);
        assert_eq!(dead_count(), 6);
    }

    #[test]
    fn t32_define_reject_slots() {
        assert_eq!(case(8).actual().stage, Stage::Retry);
        assert_eq!(case(8).actual().code, KC_EFLAKE);
        assert_eq!(case(8).actual().value, 2);
        assert_eq!(case(9).actual().stage, Stage::Exec);
        assert_eq!(case(9).actual().value, 1);
        assert_eq!(case(10).actual().stage, Stage::Tally);
        assert_eq!(case(10).actual().value, 32);
        assert_eq!(case(11).actual().stage, Stage::Dims);
        assert_eq!(case(11).actual().note, Note::DimBad);
        assert_eq!(case(13).actual().stage, Stage::Ready);
        assert_eq!(case(13).actual().code, KC_ESKIP);
        assert_eq!(case(13).actual().note, Note::CfgMissing);
    }

    #[test]
    fn t33_judge_accept_is_clean() {
        for i in 0..ACCEPT {
            assert_eq!(judge(&case(i)), Fault::None);
            assert_eq!(expected_fault(i), Fault::None);
        }
    }

    #[test]
    fn t34_judge_reject_codes() {
        for i in ACCEPT..CORPUS {
            let f = judge(&case(i));
            assert_ne!(f, Fault::None);
            assert_eq!(f.code(), WANT_FAULT_CODE[i]);
            assert_eq!(f, expected_fault(i));
        }
    }

    #[test]
    fn t35_judge_order_estate_first() {
        let c = case(9);
        let a = c.actual();
        let mut cl = c.claim;
        cl.success = true;
        cl.stage = Stage::Manifest;
        assert_eq!(judge_against(&cl, &a), Fault::Estate);
    }

    #[test]
    fn t36_judge_order_stage_before_code() {
        let c = case(9);
        let a = c.actual();
        let mut cl = c.claim;
        cl.stage = Stage::Manifest;
        assert_eq!(judge_against(&cl, &a), Fault::Stage);
        cl.stage = a.stage;
        assert_eq!(judge_against(&cl, &a), Fault::Code);
    }

    #[test]
    fn t37_corpus_split() {
        assert_eq!(SHAPES.len(), CORPUS);
        assert_eq!(CLAIMS.len(), CORPUS);
        assert_eq!(matched_count(), 14);
        assert_eq!(conflict_count(), 0);
        for i in 0..CORPUS {
            assert_eq!(case(i).tag(), WANT_TAGS[i]);
        }
    }

    #[test]
    fn t38_corpus_ndx() {
        for i in 0..ACCEPT {
            assert_eq!(WANT_NDX[i], DIM_NONE);
            assert_eq!(case(i).claim.divergence(&case(i).actual()), 0);
        }
        for i in ACCEPT..CORPUS {
            let d = Dim::of_rank(WANT_NDX[i]).expect("dim");
            let c = case(i);
            assert_eq!(c.claim.divergence(&c.actual()), 1);
            assert!(c.claim.differs(&c.actual(), d));
        }
    }

    #[test]
    fn t39_corpus_tags_match_specs() {
        for i in 0..CORPUS {
            let c = case(i);
            let s = spec_at(c.shape.idx).expect("spec");
            assert_eq!(c.claim.tag, s.tag);
            assert_eq!(c.shape.dims, s.dims);
        }
        assert!(corpus_ok());
    }

    #[test]
    fn t40_tier_counts() {
        let tc = tier_counts();
        assert_eq!(tc[0], 6);
        assert_eq!(tc[1], 3);
        assert_eq!(tc[2], 1);
        assert_eq!(tc[3], 4);
        assert_eq!(tc.iter().sum::<u32>(), 14);
    }

    #[test]
    fn t41_matrix_rows() {
        let m = Matrix::build();
        assert_eq!(m.slots(), 14);
        assert_eq!(m.goods(), 14);
        assert_eq!(m.perfect_rows(), 4);
        assert_eq!(m.row(Tier::Anchored).expect("r").total, 6);
        assert_eq!(m.row(Tier::Void).expect("r").total, 1);
        assert!(m.row(Tier::Anchored).expect("r").perfect());
    }

    #[test]
    fn t42_matrix_render() {
        let m = Matrix::build();
        let r = m.render();
        assert!(r.contains("anchored=6/6"));
        assert!(r.contains("declared=3/3"));
        assert!(r.contains("void=1/1"));
        assert!(r.contains("unrun=4/4"));
        assert!(matrix_ok());
    }

    #[test]
    fn t43_report_pin() {
        let r = run();
        assert_eq!(r.render(), RENDER_OK);
        assert!(r.green());
        assert!(r.gate());
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.total, 14);
        assert_eq!(r.matched, 14);
        assert_eq!(r.conflicts, 0);
        assert_eq!(r.gates, 8);
        assert!(r.sealed);
    }

    #[test]
    fn t44_rate_formula() {
        let mut r = Report::empty();
        r.total = 14;
        r.matched = 8;
        assert_eq!(r.rate(), (8 * 1000 + 7) / 14);
        r.matched = 14;
        assert_eq!(r.rate(), 1000);
        let e = Report::empty();
        assert_eq!(e.rate(), 0);
        assert!(!e.green());
    }

    #[test]
    fn t45_empty_report_render() {
        let e = Report::empty();
        let s = e.render();
        assert!(s.starts_with("ktest=KC80 bench=19 "));
        assert!(s.contains("total=0"));
        assert!(s.ends_with("green=false\n"));
        assert_eq!(e.tiers_sum(), 0);
    }

    #[test]
    fn t46_gate_monotone() {
        let mut g = Gate::new();
        assert_eq!(g.enter(Stage::Manifest), Ok(()));
        assert_eq!(g.enter(Stage::Dims), Ok(()));
        assert_eq!(g.enter(Stage::Exec), Ok(()));
        assert_eq!(g.enter(Stage::Manifest), Err(KC_ESTAGE));
        assert_eq!(g.backslides(), 1);
        assert_eq!(g.walked(), 3);
    }

    #[test]
    fn t47_gate_closed() {
        let mut g = Gate::new();
        for i in 0..GATE_COUNT {
            assert_eq!(g.enter(Stage::of_rank(i).expect("s")), Ok(()));
        }
        assert!(g.complete());
        g.close();
        assert!(g.is_closed());
        assert_eq!(g.enter(Stage::Seal), Err(KC_ESTATE));
        assert!(gate_ok());
    }

    #[test]
    fn t48_ledger_register() {
        let mut l = Ledger::new();
        assert_eq!(l.register(1, 2, Sample::want(1)), Err(KC_ESLOT));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.register(0, 1, Sample::want(0)), Ok(()));
        assert_eq!(l.len(), 1);
        assert_eq!(l.sample(0).expect("s").value, Sample::want(0));
        assert!(l.sample(1).is_none());
    }

    #[test]
    fn t49_ledger_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.rejects(), 0);
        let root = l.freeze().expect("freeze");
        assert!(root != 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(KC_ESTATE));
        assert_eq!(l.register(SAMPLES, 15, 240), Err(KC_ESTATE));
        assert_eq!(l.poke(0, 1), Err(KC_ESTATE));
    }

    #[test]
    fn t50_ledger_tamper() {
        for b in 0u8..3 {
            let l = build_ledger_broken(b);
            assert!(l.audit().is_err(), "broken {b} must fail audit");
            assert!(l.first_break().is_some());
        }
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        assert!(l.first_break().is_none());
        assert_eq!(l.poke(2, 0), Ok(()));
        assert!(l.audit().is_err());
        assert_eq!(l.first_break(), Some(2));
    }

    #[test]
    fn t51_ledger_fresh_cannot_replay() {
        let mut l = Ledger::new();
        assert_eq!(l.replay(), Err(KC_ESTATE));
        assert!(l.freeze().is_err());
        assert!(!l.is_frozen());
        assert_eq!(l.len(), 0);
        assert_eq!(l.root(), 0);
    }

    #[test]
    fn t52_inject_four() {
        let r = inject_four();
        assert_eq!(r, [true, true, true, true]);
        assert!(inject_ok());
        let ok = run_with(Some((9, Fault::None))).expect("report");
        assert_eq!(ok.matched, 13);
        assert!(!ok.green());
        let bad = run_with(Some((3, Fault::Stage))).expect("report");
        assert_eq!(bad.matched, 13);
    }

    #[test]
    fn t53_run_with_out_of_range() {
        assert_eq!(run_with(Some((CORPUS, Fault::None))), Err(KC_ESLOT));
        assert_eq!(run_with(Some((99, Fault::Value))), Err(KC_ESLOT));
        assert!(run_with(None).is_ok());
    }

    #[test]
    fn t54_detail_and_diagnostic() {
        let d = detail(9, Fault::Code);
        assert!(d.contains("slot=9"));
        assert!(d.contains("fault=code"));
        assert!(d.contains("dim=code"));
        let g = diagnostic(9, Fault::Code);
        assert!(g.contains("stage=exec"));
        assert!(g.contains("note=exit-bad"));
        let missing = diagnostic(CORPUS, Fault::None);
        assert!(missing.contains("note=missing"));
    }

    #[test]
    fn t55_checks_all_passed() {
        let s = run_unx11680_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert!(block_ok());
        assert!(codes_ok());
        assert!(enums_ok());
        assert!(bench_ok());
        assert!(dims_ok());
        assert!(define_ok());
        assert!(judge_ok());
        assert!(corpus_ok());
        assert!(matrix_ok());
        assert!(render_ok());
        assert!(gate_ok());
        assert!(ledger_ok());
        assert!(inject_ok());
    }
}
