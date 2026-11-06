//! UNX-F11660 B23 批 ktest 断言集聚合
//!
//! 本模块把 B23 批（UNX-F11641 ~ UNX-F11659，共 19 条判据）聚合成一次 ktest 生态面
//! 运行：一条命令把 19 条判据全跑完，通过率必须逐字达到 100%，任何 skip、任何
//! flake、任何"依赖没达成却报了过"都要显式挂在红灯上，不许被平均值吞掉。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 聚合的八道闸严格按下面这个次序走，前一道不过就根本没有后一道的事实输入：
//!
//! 1. 清单闸（Manifest）——先数够 19 条。条数是本批的身份证：少一条就意味着有判据
//!    被静默丢弃，多一条就意味着有人把别批的判据混进来。清单都不平，后面所有计数
//!    都是无根之数，所以它必须在最前。
//! 2. 身份闸（Identity）——卡号、短名、段基址三者各自唯一。清单平了但两条判据共用
//!    一个短名，汇总时它们会互相覆盖，"19 条全绿"其实只验了 18 个身份。
//! 3. 依赖闸（Deps）——每条判据声明它前置依赖哪一条（F11658 依赖 F11657，
//!    F11659 依赖 F11658）。依赖号必须落在本批内且不成环。环要在执行前判死，
//!    否则执行序无从排出，后面的次序闸就没有参照。
//! 4. 前置闸（Ready）——harness 与 CONFIG 位齐不齐。不齐就是 skip；本卡的铁律是
//!    skip 记红，不记绿。为什么它在执行之前：没跑就是没跑，任何"跑失败"的码位都
//!    不该盖在一条没跑的判据上。
//! 5. 执行闸（Exec）——真跑一次，取 exit code、首次尝试是否成功、耗时量级、日志字节
//!    量级。执行事实是后面一切归因的唯一来源，它必须紧跟前置。
//! 6. 重试闸（Retry）——attempts>1 且最终成功判 Flaked，必须显式。抖动被洗成一次通过
//!    是本卡最阴的假绿：生态面上看是 100%，实际上那条判据根本不稳。所以重试计数只能
//!    在这里结算，早了没有 attempts，晚了就会被汇总吞掉。
//! 7. 汇总闸（Tally）——四档计数相加必须等于语料槽数，通过率按 (预期结论命中数)*1000
//!    /槽数 结算，同时把 19 与 116 这两个身份数原样带到签发面上。
//!    它在最后是因为它要消费前面所有闸的产物；也正因为它最后，它才允许是那个
//!    "通过率不到 1000 就判死"的唯一出口。
//! 8. 封印闸（Seal）——按执行序折叠日志指纹。次序一变，指纹就变，即使绿集完全相同。
//!    这条钉的是"一次命令全跑"这个验收语本身：换个顺序重排绿表不许冒充同一次运行。
//!
//! ## 关键差异（本卡深度所在）
//!
//! - 本卡与 F11658（坏 locale 矩阵）、F11659（双机文案对照）正交：那两张判的是单条
//!   纵线上的事实，本卡判的是 19 条纵线拼成一个生态面运行时的**账**。判据本身红不红
//!   不由本卡决定，本卡只决定"报绿报得对不对"。
//! - skip ≠ pass，这是本卡与常见 CI 汇总最大的分野。ktest 生态面里一条没跑的判据
//!   不进分母就等于没这条判据；本卡把它单列一档并计入红灯，通过率因此不可能靠"跳过
//!   难跑的"刷到 100%。
//! - flake ≠ pass，也 ≠ fail。Flaked 是"跑过了但抖过"，它计入绿集（因为验收要求
//!   一次命令全跑通），但必须占一个独立档、必须在渲染行里露出 attempts 数。绿集里
//!   有 flake 时，`Report::green()` 仍真，`Report::flake_free()` 假——两个问句分开答。
//! - blocked ≠ fail。依赖判据自己没过，本条就不该被执行；它既不能算 fail（没跑），
//!   也不能算 pass（没验）。本卡用 default 位与 note 双重钉它。
//! - 身份维与执行维不同维：段基址撞了（KA_EUNIQ）是清单事实错，与那条判据跑得好不好
//!   毫无关系；把它们混在一格里就会用一条判据的执行绿去掩盖另一条的身份错。
//! - 通过率的分母有两个，必须分清：清单闸的分母是 19（本批判据条数，进渲染行的
//!   `bench=19`），判定闸的分母是 14（本模块的断言槽数，进渲染行的 `total=14`）。
//!   验收原文"通过率 100%"落在后者上，含义是"关于这 19 条的全部断言都产出了预期
//!   结论"；前者只用来钉身份，绝不允许被写成通过率的分子分母——因为语料必须含
//!   缺陷形态的槽，把 19 当分母就必然要删掉那些槽。这是本卡与 B05 批聚合锚的边界：
//!   B05 按实跑条数结算，本卡按断言槽结算。
//! - 红档率 929 独占账本一格，不并进统计格：并进统计就把一个比率当成了计数。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本模块段位基址（全仓唯一）。
pub const SEG_BASE: u32 = 0x6cf50;

// ---------------------------------------------------------------------------
// 13 个错误码
// ---------------------------------------------------------------------------

/// 成败互报：判据自报通过但实跑失败，或反之。
pub const KA_ESTATE: u32 = SEG_BASE + 0x01;
/// 闸门次序：八道闸的停留档不符，或执行序不等于依赖拓扑序。
pub const KA_ESTAGE: u32 = SEG_BASE + 0x02;
/// 错误码面：exit code 或 errno 位与申报不符。
pub const KA_ECODE: u32 = SEG_BASE + 0x03;
/// 计数值面：条数、重试数、通过率、日志量级与申报不符。
pub const KA_EVALUE: u32 = SEG_BASE + 0x04;
/// 归因档：skip 成因或 flake 归注与证据不符。
pub const KA_ENOTE: u32 = SEG_BASE + 0x05;
/// 默认位：是否计入 ktest 默认必跑集的标志与申报不符。
pub const KA_EDEFAULT: u32 = SEG_BASE + 0x06;
/// 清单条数不符：聚合到的判据数不等于 B23 批的 19 条。
pub const KA_ECOUNT: u32 = SEG_BASE + 0x07;
/// 身份不唯一：卡号、短名或段基址在本批内重复。
pub const KA_EUNIQ: u32 = SEG_BASE + 0x08;
/// 依赖成环：拓扑排序失败，执行序无从排出。
pub const KA_ECYCLE: u32 = SEG_BASE + 0x09;
/// skip 被记绿：没跑的判据混进了绿集。
pub const KA_ESKIP: u32 = SEG_BASE + 0x0a;
/// flake 被洗绿：attempts>1 却申报一次通过。
pub const KA_EFLAKE: u32 = SEG_BASE + 0x0b;
/// 依赖阻断仍报过：前置判据未达成，本条却计入绿集。
pub const KA_EBLOCK: u32 = SEG_BASE + 0x0c;
/// 越界槽：账本槽位不等于当前长度或超出样本数。
pub const KA_ESLOT: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 规模常量
// ---------------------------------------------------------------------------

/// 语料槽总数：8 活 + 6 死。
pub const CORPUS: usize = 14usize;
/// 账本样本格数。
pub const SAMPLES: usize = 14usize;
/// 活槽（申报成立）数。
pub const ACCEPT: usize = 8usize;
/// 死槽（申报不成立）数。
pub const REJECT: usize = 6usize;
/// 档位总数。
pub const TIER_COUNT: usize = 4usize;
/// B23 批判据条数——本卡通过率的分母，写死 19。
pub const BENCH: usize = 19usize;
/// 八道闸的级数。
pub const GATE_COUNT: usize = 8usize;
/// 归因档数。
pub const NOTE_COUNT: usize = 11usize;
/// harness 种类数。
pub const HARNESS_COUNT: usize = 7usize;
/// 双线差异锚（musl 固定行为 / musl 无符号版本）在本批内的条数。
pub const DUAL_ANCHORS: usize = 2usize;
/// 通过率满分值（千分制）。
pub const RATE_FULL: u32 = 1000u32;
/// 红档率显式值——账本第 8 格独占。
pub const RED_RATE: u32 = 929u32;

// ---------------------------------------------------------------------------
// 退出码与 errno 位（ktest 生态面常见值）
// ---------------------------------------------------------------------------

/// 正常退出。
pub const EX_OK: i32 = 0;
/// 通用失败（kselftest 的 FAIL）。
pub const EX_FAIL: i32 = 1;
/// harness 缺失可执行体。
pub const EX_NOTFOUND: i32 = 127;
/// 被信号杀死（128 + signo）。
pub const EX_SIG_BASE: i32 = 128;
/// SIGSEGV 位。
pub const SIG_SEGV: i32 = 11;
/// SIGALRM（超时）位。
pub const SIG_ALRM: i32 = 14;
/// 无此配置导致的跳过约定码（kselftest 的 SKIP）。
pub const EX_SKIP: i32 = 4;
/// ENOSPC。
pub const ENOSPC: i32 = 28;
/// ENOENT。
pub const ENOENT: i32 = 2;
/// EACCES。
pub const EACCES: i32 = 13;
/// ETIMEDOUT。
pub const ETIMEDOUT: i32 = 110;
/// 无 errno（成功或未到错误点）。
pub const ENONE: i32 = 0;

// ---------------------------------------------------------------------------
// CONFIG 位（前置闸消费的掩码）
// ---------------------------------------------------------------------------

/// 需要 CONFIG_MODULES。
pub const CFG_MODULES: u32 = 1u32 << 0;
/// 需要 CONFIG_DEBUG_FS。
pub const CFG_DEBUGFS: u32 = 1u32 << 1;
/// 需要 CONFIG_NET。
pub const CFG_NET: u32 = 1u32 << 2;
/// 需要 CONFIG_TMPFS。
pub const CFG_TMPFS: u32 = 1u32 << 3;
/// 需要运行期 root。
pub const CFG_ROOT: u32 = 1u32 << 4;
/// 需要双机（host + target）对照。
pub const CFG_TWOSYS: u32 = 1u32 << 5;
/// 需要 musl 侧目标。
pub const CFG_MUSL: u32 = 1u32 << 6;
/// 需要 glibc 侧目标。
pub const CFG_GLIBC: u32 = 1u32 << 7;
/// 需要可裁剪 applet 的 busybox。
pub const CFG_BUSYBOX: u32 = 1u32 << 8;
/// 需要 loop 设备。
pub const CFG_LOOP: u32 = 1u32 << 9;
/// 前置齐备（不消费任何位）。
pub const CFG_NONE: u32 = 0u32;

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

/// 折一个字（16 位切片，两次）。
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

/// 折一段字符串（逐字，含结尾长度字）。
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

/// CONFIG 位 → 文本名。
pub fn cfg_name(bit: u32) -> &'static str {
    match bit {
        1u32 => "CONFIG_MODULES",
        2u32 => "CONFIG_DEBUG_FS",
        4u32 => "CONFIG_NET",
        8u32 => "CONFIG_TMPFS",
        16u32 => "running-as-root",
        32u32 => "two-system",
        64u32 => "musl-target",
        128u32 => "glibc-target",
        256u32 => "busybox-applets",
        512u32 => "loop-device",
        _ => "none",
    }
}

// ---------------------------------------------------------------------------
// harness 种类：一条判据怎么被跑起来
// ---------------------------------------------------------------------------

/// 判据的运行载体。不同载体的 skip 语义完全不同，故先分型再谈成败。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Harness {
    /// kselftest C 体：跑在目标机上，靠 ksft_exit_* 收退出码。
    KselftestC,
    /// kselftest shell 库：source lib.sh，TAP 行直出。
    ShellLib,
    /// 裸 sh 脚本：无库，只看 $?。
    RawSh,
    /// busybox applet 探针：依赖裁剪后的 applet 表。
    AppletProbe,
    /// 双机对照：host 与 target 各跑一遍再逐字比。
    TwoSystem,
    /// musl 侧探针：固定行为锚。
    MuslProbe,
    /// 静态读节：objdump/readelf 面，不需要跑。
    StaticObj,
}

impl Harness {
    /// 稳定序号（进指纹与矩阵）。
    pub const fn rank(self) -> u8 {
        match self {
            Harness::KselftestC => 1u8,
            Harness::ShellLib => 2u8,
            Harness::RawSh => 3u8,
            Harness::AppletProbe => 4u8,
            Harness::TwoSystem => 5u8,
            Harness::MuslProbe => 6u8,
            Harness::StaticObj => 7u8,
        }
    }

    /// 由序号还原。越界给 None，不猜。
    pub const fn from_rank(r: u8) -> Option<Harness> {
        match r {
            1u8 => Some(Harness::KselftestC),
            2u8 => Some(Harness::ShellLib),
            3u8 => Some(Harness::RawSh),
            4u8 => Some(Harness::AppletProbe),
            5u8 => Some(Harness::TwoSystem),
            6u8 => Some(Harness::MuslProbe),
            7u8 => Some(Harness::StaticObj),
            _ => None,
        }
    }

    /// 逐字短名，进 TAP 行的注释位。
    pub const fn slug(self) -> &'static str {
        match self {
            Harness::KselftestC => "kselftest-c",
            Harness::ShellLib => "kselftest-sh-lib",
            Harness::RawSh => "raw-sh",
            Harness::AppletProbe => "busybox-applet",
            Harness::TwoSystem => "two-system-diff",
            Harness::MuslProbe => "musl-probe",
            Harness::StaticObj => "static-objdump",
        }
    }

    /// 该载体是否需要目标机真的把进程跑起来（静态读节不需要）。
    pub const fn needs_run(self) -> bool {
        match self {
            Harness::StaticObj => false,
            _ => true,
        }
    }

    /// 该载体是否天然要求双机在场。
    pub const fn needs_two_sys(self) -> bool {
        match self {
            Harness::TwoSystem => true,
            _ => false,
        }
    }

    /// 该载体是否要求 musl 侧目标。
    pub const fn needs_musl(self) -> bool {
        match self {
            Harness::MuslProbe => true,
            _ => false,
        }
    }

    /// 缺失该载体时的 errno 位（127 面的来源）。
    pub const fn miss_errno(self) -> i32 {
        match self {
            Harness::StaticObj => ENOENT,
            Harness::AppletProbe => ENOENT,
            Harness::RawSh => EACCES,
            _ => ENOENT,
        }
    }

    /// 一次通过时的量级基线（毫秒），用于把 ms 折成档位。
    pub const fn ms_band(self) -> u32 {
        match self {
            Harness::KselftestC => 250u32,
            Harness::ShellLib => 400u32,
            Harness::RawSh => 120u32,
            Harness::AppletProbe => 60u32,
            Harness::TwoSystem => 1800u32,
            Harness::MuslProbe => 90u32,
            Harness::StaticObj => 30u32,
        }
    }
}

// ---------------------------------------------------------------------------
// 八道闸
// ---------------------------------------------------------------------------

/// 聚合纵线上的一道闸。次序由 rank 钉死，任何"停在更早的闸却报更晚的事实"都是判死。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 清单闸。
    Manifest,
    /// 身份闸。
    Identity,
    /// 依赖闸。
    Deps,
    /// 前置闸。
    Ready,
    /// 执行闸。
    Exec,
    /// 重试闸。
    Retry,
    /// 汇总闸。
    Tally,
    /// 封印闸。
    Seal,
}

impl Stage {
    /// 闸序。
    pub const fn rank(self) -> u8 {
        match self {
            Stage::Manifest => 1u8,
            Stage::Identity => 2u8,
            Stage::Deps => 3u8,
            Stage::Ready => 4u8,
            Stage::Exec => 5u8,
            Stage::Retry => 6u8,
            Stage::Tally => 7u8,
            Stage::Seal => 8u8,
        }
    }

    /// 由序号还原。
    pub const fn from_rank(r: u8) -> Option<Stage> {
        match r {
            1u8 => Some(Stage::Manifest),
            2u8 => Some(Stage::Identity),
            3u8 => Some(Stage::Deps),
            4u8 => Some(Stage::Ready),
            5u8 => Some(Stage::Exec),
            6u8 => Some(Stage::Retry),
            7u8 => Some(Stage::Tally),
            8u8 => Some(Stage::Seal),
            _ => None,
        }
    }

    /// 短名。
    pub const fn slug(self) -> &'static str {
        match self {
            Stage::Manifest => "manifest",
            Stage::Identity => "identity",
            Stage::Deps => "deps",
            Stage::Ready => "ready",
            Stage::Exec => "exec",
            Stage::Retry => "retry",
            Stage::Tally => "tally",
            Stage::Seal => "seal",
        }
    }

    /// 下一闸；已在末闸就给 None（封印闸之后没有闸可走）。
    pub const fn next(self) -> Option<Stage> {
        match self {
            Stage::Manifest => Some(Stage::Identity),
            Stage::Identity => Some(Stage::Deps),
            Stage::Deps => Some(Stage::Ready),
            Stage::Ready => Some(Stage::Exec),
            Stage::Exec => Some(Stage::Retry),
            Stage::Retry => Some(Stage::Tally),
            Stage::Tally => Some(Stage::Seal),
            Stage::Seal => None,
        }
    }

    /// 是否已经走过执行闸（走过才有资格谈计数与抖动）。
    pub const fn past_exec(self) -> bool {
        self.rank() >= Stage::Retry.rank()
    }

    /// 是否走满全程。
    pub const fn is_end(self) -> bool {
        match self {
            Stage::Seal => true,
            _ => false,
        }
    }

    /// 该闸违约时对应的错误码——把"停在哪一闸"翻译成码位。
    pub const fn fault_code(self) -> u32 {
        match self {
            Stage::Manifest => KA_ECOUNT,
            Stage::Identity => KA_EUNIQ,
            Stage::Deps => KA_ECYCLE,
            Stage::Ready => KA_ESKIP,
            Stage::Exec => KA_ECODE,
            Stage::Retry => KA_EFLAKE,
            Stage::Tally => KA_EVALUE,
            Stage::Seal => KA_EBLOCK,
        }
    }
}

/// 八闸的固定次序表，逐字对齐文档头。
pub const STAGE_CHAIN: [Stage; GATE_COUNT] = [
    Stage::Manifest,
    Stage::Identity,
    Stage::Deps,
    Stage::Ready,
    Stage::Exec,
    Stage::Retry,
    Stage::Tally,
    Stage::Seal,
];

// ---------------------------------------------------------------------------
// skip 成因
// ---------------------------------------------------------------------------

/// 一条判据没跑起来的成因。成因决定它归哪一档，也决定渲染行里露哪个词。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Skip {
    /// 没跳过。
    None,
    /// CONFIG 位不足。
    NoConfig,
    /// 需要 root 而不是 root。
    NoRoot,
    /// 需要设备（loop/tmpfs）而设备不在。
    NoDevice,
    /// 需要双机而只有一台在场。
    NoPeer,
    /// 需要 musl 侧目标而目标只有 glibc。
    NoMusl,
    /// harness 自身缺失（可执行体找不到）。
    NoHarness,
}

impl Skip {
    /// 序号。
    pub const fn rank(self) -> u8 {
        match self {
            Skip::None => 0u8,
            Skip::NoConfig => 1u8,
            Skip::NoRoot => 2u8,
            Skip::NoDevice => 3u8,
            Skip::NoPeer => 4u8,
            Skip::NoMusl => 5u8,
            Skip::NoHarness => 6u8,
        }
    }

    /// 由序号还原。
    pub const fn from_rank(r: u8) -> Option<Skip> {
        match r {
            0u8 => Some(Skip::None),
            1u8 => Some(Skip::NoConfig),
            2u8 => Some(Skip::NoRoot),
            3u8 => Some(Skip::NoDevice),
            4u8 => Some(Skip::NoPeer),
            5u8 => Some(Skip::NoMusl),
            6u8 => Some(Skip::NoHarness),
            _ => None,
        }
    }

    /// 短名。
    pub const fn slug(self) -> &'static str {
        match self {
            Skip::None => "none",
            Skip::NoConfig => "no-config",
            Skip::NoRoot => "no-root",
            Skip::NoDevice => "no-device",
            Skip::NoPeer => "no-peer",
            Skip::NoMusl => "no-musl",
            Skip::NoHarness => "no-harness",
        }
    }

    /// 是否真的跳过了。
    pub const fn is_skip(self) -> bool {
        self.rank() > 0u8
    }

    /// 该成因对应的 errno 位——没跑成的机器面证据。
    pub const fn errno(self) -> i32 {
        match self {
            Skip::NoHarness => ENOENT,
            Skip::NoRoot => EACCES,
            Skip::NoDevice => ENOENT,
            Skip::NoConfig => ENOENT,
            Skip::NoPeer => ENOENT,
            Skip::NoMusl => ENOENT,
            Skip::None => ENONE,
        }
    }

    /// 该成因消费的 CONFIG 位（NoHarness 不吃位，它是载体本身不在）。
    pub const fn consumed_bit(self) -> u32 {
        match self {
            Skip::NoRoot => CFG_ROOT,
            Skip::NoDevice => CFG_LOOP,
            Skip::NoPeer => CFG_TWOSYS,
            Skip::NoMusl => CFG_MUSL,
            _ => CFG_NONE,
        }
    }
}

// ---------------------------------------------------------------------------
// 归因档
// ---------------------------------------------------------------------------

/// 判定归因。它是证据侧算出来的，不是申报侧写出来的——两者不符就是 Note 维的红灯。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 无归因（纯失败，还没到能谈成因的时候）。
    None,
    /// 一次通过。
    Clean,
    /// 抖动后恢复（attempts>1）。
    FlakeRecovered,
    /// CONFIG 缺位。
    ConfigAbsent,
    /// 非 root。
    RootAbsent,
    /// 设备不在。
    DeviceAbsent,
    /// 前置判据未达成，本条被阻断。
    DepBlocked,
    /// 载体与判据不匹配。
    HarnessMismatch,
    /// 断言计数短少。
    AssertShort,
    /// 身份（卡号/短名/段基址）撞车。
    DupIdentity,
    /// 依赖越界或成环。
    CycleLock,
}

impl Note {
    /// 序号。
    pub const fn rank(self) -> u8 {
        match self {
            Note::None => 0u8,
            Note::Clean => 1u8,
            Note::FlakeRecovered => 2u8,
            Note::ConfigAbsent => 3u8,
            Note::RootAbsent => 4u8,
            Note::DeviceAbsent => 5u8,
            Note::DepBlocked => 6u8,
            Note::HarnessMismatch => 7u8,
            Note::AssertShort => 8u8,
            Note::DupIdentity => 9u8,
            Note::CycleLock => 10u8,
        }
    }

    /// 由序号还原。
    pub const fn from_rank(r: u8) -> Option<Note> {
        match r {
            0u8 => Some(Note::None),
            1u8 => Some(Note::Clean),
            2u8 => Some(Note::FlakeRecovered),
            3u8 => Some(Note::ConfigAbsent),
            4u8 => Some(Note::RootAbsent),
            5u8 => Some(Note::DeviceAbsent),
            6u8 => Some(Note::DepBlocked),
            7u8 => Some(Note::HarnessMismatch),
            8u8 => Some(Note::AssertShort),
            9u8 => Some(Note::DupIdentity),
            10u8 => Some(Note::CycleLock),
            _ => None,
        }
    }

    /// 短名，逐字进 TAP 注释。
    pub const fn slug(self) -> &'static str {
        match self {
            Note::None => "",
            Note::Clean => "clean",
            Note::FlakeRecovered => "flake-recovered",
            Note::ConfigAbsent => "config-absent",
            Note::RootAbsent => "root-absent",
            Note::DeviceAbsent => "device-absent",
            Note::DepBlocked => "dep-blocked",
            Note::HarnessMismatch => "harness-mismatch",
            Note::AssertShort => "assert-short",
            Note::DupIdentity => "dup-identity",
            Note::CycleLock => "cycle-lock",
        }
    }

    /// 是否属于绿面归因（只有这两种允许 ok 为真）。
    pub const fn green_face(self) -> bool {
        match self {
            Note::Clean => true,
            Note::FlakeRecovered => true,
            _ => false,
        }
    }

    /// 是否属于 skip 面归因（没跑，而非跑坏）。
    pub const fn skip_face(self) -> bool {
        match self {
            Note::ConfigAbsent => true,
            Note::RootAbsent => true,
            Note::DeviceAbsent => true,
            Note::HarnessMismatch => true,
            _ => false,
        }
    }

    /// 由 skip 成因直接映射归因档；None 成因给无归因。
    pub const fn of_skip(s: Skip) -> Note {
        match s {
            Skip::NoConfig => Note::ConfigAbsent,
            Skip::NoRoot => Note::RootAbsent,
            Skip::NoDevice => Note::DeviceAbsent,
            Skip::NoHarness => Note::HarnessMismatch,
            Skip::NoPeer => Note::DeviceAbsent,
            Skip::NoMusl => Note::ConfigAbsent,
            Skip::None => Note::None,
        }
    }
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 四档。判据聚合的总账只能落进这四个格之一。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 一次通过。
    Green,
    /// 抖动后通过（计入绿集，但必须显式）。
    Flaked,
    /// 没跑（计入红灯）。
    Skipped,
    /// 跑坏或被阻断（计入红灯）。
    Blocked,
}

impl Tier {
    /// 由实测结局归档。唯一入口，申报侧不许自报档位。
    pub fn of(o: &Outcome) -> Tier {
        if !o.ok {
            if o.note.skip_face() {
                return Tier::Skipped;
            }
            return Tier::Blocked;
        }
        match o.note {
            Note::FlakeRecovered => Tier::Flaked,
            _ => Tier::Green,
        }
    }

    /// 序号。
    pub const fn rank(self) -> u8 {
        match self {
            Tier::Green => 0u8,
            Tier::Flaked => 1u8,
            Tier::Skipped => 2u8,
            Tier::Blocked => 3u8,
        }
    }

    /// 由序号还原。
    pub const fn from_rank(r: u8) -> Option<Tier> {
        match r {
            0u8 => Some(Tier::Green),
            1u8 => Some(Tier::Flaked),
            2u8 => Some(Tier::Skipped),
            3u8 => Some(Tier::Blocked),
            _ => None,
        }
    }

    /// 短名。
    pub const fn slug(self) -> &'static str {
        match self {
            Tier::Green => "green",
            Tier::Flaked => "flaked",
            Tier::Skipped => "skipped",
            Tier::Blocked => "blocked",
        }
    }

    /// 该档是否进绿集。注意 Skipped 不进——本卡的铁律。
    pub const fn in_green(self) -> bool {
        match self {
            Tier::Green => true,
            Tier::Flaked => true,
            _ => false,
        }
    }

    /// 该档是否占红灯格。
    pub const fn is_red(self) -> bool {
        match self {
            Tier::Green => false,
            Tier::Flaked => false,
            _ => true,
        }
    }

    /// 该档的权重千分值（绿=1000，抖=900，跳=0，坏=0）。抖档不满分是因为验收语要求
    /// "通过率 100%"，抖动条在通过率里要打折，但在门禁里不判死。
    pub const fn weight(self) -> u32 {
        match self {
            Tier::Green => 1000u32,
            Tier::Flaked => 900u32,
            Tier::Skipped => 0u32,
            Tier::Blocked => 0u32,
        }
    }
}

// ---------------------------------------------------------------------------
// 缺陷维
// ---------------------------------------------------------------------------

/// 槽级对判的结论。次序固定为 Estate → Stage → Code → Value → Note → Default。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 无缺陷。
    Ok,
    /// 成败互报。
    Estate,
    /// 闸门停留档不符。
    Stage,
    /// 错误码面不符。
    Code,
    /// 计数值面不符。
    Value,
    /// 归因档不符。
    Note,
    /// 默认位不符。
    Default,
}

impl Fault {
    /// 对应错误码。
    pub const fn code(self) -> u32 {
        match self {
            Fault::Ok => 0u32,
            Fault::Estate => KA_ESTATE,
            Fault::Stage => KA_ESTAGE,
            Fault::Code => KA_ECODE,
            Fault::Value => KA_EVALUE,
            Fault::Note => KA_ENOTE,
            Fault::Default => KA_EDEFAULT,
        }
    }

    /// 短名。
    pub const fn slug(self) -> &'static str {
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

    /// 是否属于计数/账目面（用于账本格的性质判定）。
    pub const fn accounting(self) -> bool {
        match self {
            Fault::Value => true,
            Fault::Default => true,
            _ => false,
        }
    }

    /// 是否属于成败面（优先于一切其它维）。
    pub const fn estate(self) -> bool {
        match self {
            Fault::Estate => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 输入维度：一条判据在本批聚合里的全部事实
// ---------------------------------------------------------------------------

/// 一条判据跑一次的完整事实面。申报侧的每一个维度都在这里有一个对应的事实维。
#[derive(Clone, Copy)]
pub struct Shape {
    /// 卡号（11641..11659 是本批内；越界即清单事实）。
    pub card: u16,
    /// 判据短名，逐字进 TAP 行。
    pub tag: &'static str,
    /// 运行载体。
    pub harness: Harness,
    /// 实跑最终停留的闸。
    pub stage: Stage,
    /// 是否真的跑过（静态读节也算跑过）。
    pub ran: bool,
    /// 首次尝试是否成功。
    pub first_ok: bool,
    /// 尝试次数（1 = 一次到位）。
    pub attempts: u8,
    /// 退出码。
    pub exit_code: i32,
    /// errno 位。
    pub errno_bit: i32,
    /// 耗时毫秒。
    pub ms: u32,
    /// 日志字节数。
    pub log_bytes: u32,
    /// 该判据内部断言总数。
    pub asserts: u16,
    /// 通过的断言数。
    pub pass_asserts: u16,
    /// skip 成因。
    pub skip: Skip,
    /// 前置卡号（0 = 无前置）。
    pub dep: u16,
    /// 前置判据是否已达成。
    pub dep_done: bool,
    /// 该判据需要的 CONFIG 位掩码。
    pub need_cfg: u32,
    /// 本机实际具备的 CONFIG 位掩码。
    pub have_cfg: u32,
    /// 是否申报进默认必跑集。
    pub default_suite: bool,
    /// 身份是否与批内其它条撞车。
    pub dup: bool,
    /// 日志是否把抖动洗成一次通过（申报 first_ok=true 而 attempts>1）。
    pub washed: bool,
}

impl Shape {
    /// 空事实（数组占位用；卡号为 0 天然落在本批号段外，会被清单闸判死）。
    pub const fn zero() -> Shape {
        Shape {
            card: 0u16,
            tag: "zero",
            harness: Harness::KselftestC,
            stage: Stage::Manifest,
            ran: false,
            first_ok: false,
            attempts: 0u8,
            exit_code: ENONE,
            errno_bit: ENONE,
            ms: 0u32,
            log_bytes: 0u32,
            asserts: 0u16,
            pass_asserts: 0u16,
            skip: Skip::None,
            dep: 0u16,
            dep_done: false,
            need_cfg: CFG_NONE,
            have_cfg: CFG_NONE,
            default_suite: false,
            dup: false,
            washed: false,
        }
    }

    /// 是否在 B23 批号段内。
    pub const fn in_bench(self: &Shape) -> bool {
        self.card >= CARD_MIN && self.card <= CARD_MAX
    }

    /// 需要的位是否全具备。
    pub const fn cfg_ready(self: &Shape) -> bool {
        (self.need_cfg & !self.have_cfg) == 0u32
    }

    /// 缺的那几位（渲染行要逐字露出）。
    pub fn cfg_missing(self: &Shape) -> u32 {
        self.need_cfg & !self.have_cfg
    }

    /// 短名是否非空且不含空格——TAP 行对这两个都敏感。
    pub fn tag_ok(self: &Shape) -> bool {
        if self.tag.len() == 0 {
            return false;
        }
        for b in self.tag.as_bytes() {
            match *b {
                // 空格截断名字、'#' 把行尾变成 TAP 注释、换行把一行劈成两行；
                // 连字符是 kselftest 短名的常规写法，必须放过。
                b' ' => return false,
                b'#' => return false,
                b'\n' => return false,
                b'\r' => return false,
                b'\t' => return false,
                _ => {}
            }
        }
        true
    }

    /// 断言是否全过。
    pub const fn all_asserts_pass(self: &Shape) -> bool {
        self.pass_asserts >= self.asserts
    }

    /// 有没有前置。
    pub const fn has_dep(self: &Shape) -> bool {
        self.dep > 0u16
    }

    /// 耗时落在哪个量级档（0 = 未跑）。
    pub const fn ms_band(self: &Shape) -> u32 {
        if !self.ran {
            return 0u32;
        }
        let base = self.harness.ms_band();
        if self.ms <= base {
            1u32
        } else if self.ms <= base.saturating_mul(4u32) {
            2u32
        } else {
            3u32
        }
    }
}

// ---------------------------------------------------------------------------
// 结局
// ---------------------------------------------------------------------------

/// 八闸走完之后一条判据的结算结果。
pub struct Outcome {
    /// 是否计入绿集。
    pub ok: bool,
    /// 停留闸。
    pub stage: Stage,
    /// 错误码（绿为 0）。
    pub code: u32,
    /// 计数值：通过的断言数。
    pub value: u32,
    /// 归因档。
    pub note: Note,
    /// 默认必跑位（只有走满全程且绿才为真）。
    pub default: bool,
    /// 该条日志折算的指纹长度（字节）。
    pub len: u32,
}

impl Outcome {
    /// 成败面——judge 的 Estate 维就比这个。
    pub const fn ok(self: &Outcome) -> bool {
        self.ok
    }

    /// 档位——唯一合法的归档入口。
    pub fn tier(self: &Outcome) -> Tier {
        Tier::of(self)
    }

    /// 把结局折进指纹。
    pub fn fold(self: &Outcome, h: u64) -> u64 {
        let a = fold_word(h, self.stage.rank() as u64);
        let b = fold_word(a, self.ok as u64);
        let c = fold_word(b, self.code as u64);
        let d = fold_word(c, self.value as u64);
        let e = fold_word(d, self.note.rank() as u64);
        let f = fold_word(e, self.default as u64);
        fold_word(f, self.len as u64)
    }

    /// 折短名。
    pub fn fold_tag(self: &Outcome, h: u64, tag: &str) -> u64 {
        fold_str(h, tag)
    }

    /// 该结局的文本面（逐字 TAP 行）。
    pub fn tap(self: &Outcome, idx: usize, name: &str) -> String {
        msg_tap(idx, name, &self)
    }
}

// ---------------------------------------------------------------------------
// 申报
// ---------------------------------------------------------------------------

/// 一条判据的申报（跑之前写在清单里的那一行）。
#[derive(Clone, Copy)]
pub struct Claim {
    /// 申报是否通过。
    pub success: bool,
    /// 申报停留闸。
    pub stage: Stage,
    /// 申报错误码。
    pub code: u32,
    /// 申报计数值。
    pub value: u32,
    /// 申报归因档。
    pub note: Note,
    /// 申报默认位。
    pub default: bool,
}

impl Claim {
    /// 空申报（数组占位用）。
    pub const fn zero() -> Claim {
        Claim {
            success: false,
            stage: Stage::Manifest,
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

    /// 申报成败面——judge 的 Estate 维比的就是它和 Outcome::ok。
    pub const fn success(self: &Claim) -> bool {
        self.success
    }

    /// 把申报折进指纹。
    pub fn fold(self: &Claim, h: u64) -> u64 {
        let a = fold_word(h, self.stage.rank() as u64);
        let b = fold_word(a, self.success as u64);
        let c = fold_word(b, self.code as u64);
        let d = fold_word(c, self.value as u64);
        let e = fold_word(d, self.note.rank() as u64);
        fold_word(e, self.default as u64)
    }

    /// 故意只坏一个维度——用于生成 6 个死槽的申报。
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
                    None => Stage::Manifest,
                };
            }
            Fault::Code => {
                c.code = if self.code == 0u32 {
                    KA_ECODE
                } else {
                    self.code.saturating_add(1u32)
                };
            }
            Fault::Value => {
                c.value = self.value.saturating_add(3u32);
            }
            Fault::Note => {
                c.note = match Note::from_rank((self.note.rank().saturating_add(1u8)) %
                                               NOTE_COUNT as u8) {
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
}

/// B23 批首卡号。
pub const CARD_MIN: u16 = 11641u16;
/// B23 批末卡号。
pub const CARD_MAX: u16 = 11659u16;

// ---------------------------------------------------------------------------
// 本批清单：19 条判据的申报事实（卡号/短名/标题/载体/前置/所需位/内部断言数）
// ---------------------------------------------------------------------------

/// 清单里的一行。它是"申报侧"的原始数据：跑之前就该长成这样。
#[derive(Clone, Copy)]
pub struct Spec {
    /// 卡号。
    pub card: u16,
    /// 判据短名（逐字进 TAP）。
    pub tag: &'static str,
    /// 卡标题（逐字，任务板原文）。
    pub title: &'static str,
    /// 运行载体。
    pub harness: Harness,
    /// 前置卡号（0 = 本批首条，无前置）。
    pub dep: u16,
    /// 需要的 CONFIG/环境位。
    pub need_cfg: u32,
    /// 该判据内部断言条数。
    pub asserts: u16,
    /// 是否进默认必跑集。
    pub default_suite: bool,
}

/// B23 批 19 条判据的清单。顺序即任务板顺序，前置逐字对齐 `- 前置:` 字段。
pub const SPECS: [Spec; BENCH] = [
    Spec {
        card: 11641u16,
        tag: "f11641-lcall-bad",
        title: "LC_ALL 坏值 setlocale 失败回退 \"C\" 判据",
        harness: Harness::KselftestC,
        dep: 0u16,
        need_cfg: CFG_GLIBC,
        asserts: 6u16,
        default_suite: true,
    },
    Spec {
        card: 11642u16,
        tag: "f11642-lang-baddir",
        title: "LANG 坏值与未安装 locale 目录降级判据",
        harness: Harness::KselftestC,
        dep: 11641u16,
        need_cfg: CFG_TMPFS | CFG_GLIBC,
        asserts: 5u16,
        default_suite: true,
    },
    Spec {
        card: 11643u16,
        tag: "f11643-setlocale-empty",
        title: "setlocale(LC_ALL,\"\") 空环境档判据",
        harness: Harness::KselftestC,
        dep: 11642u16,
        need_cfg: CFG_GLIBC,
        asserts: 7u16,
        default_suite: true,
    },
    Spec {
        card: 11644u16,
        tag: "f11644-lcnum-printf",
        title: "LC_NUMERIC 坏值 printf 小数点回退判据",
        harness: Harness::ShellLib,
        dep: 11643u16,
        need_cfg: CFG_GLIBC,
        asserts: 4u16,
        default_suite: true,
    },
    Spec {
        card: 11645u16,
        tag: "f11645-locale-return",
        title: "setlocale 返回串逐字判据（C/POSIX 别名）",
        harness: Harness::RawSh,
        dep: 11644u16,
        need_cfg: CFG_NONE,
        asserts: 8u16,
        default_suite: true,
    },
    Spec {
        card: 11646u16,
        tag: "f11646-archive-warn",
        title: "缺失 locale 归档时 glibc 警告文案判据",
        harness: Harness::ShellLib,
        dep: 11645u16,
        need_cfg: CFG_GLIBC | CFG_ROOT,
        asserts: 5u16,
        default_suite: false,
    },
    Spec {
        card: 11647u16,
        tag: "f11647-musl-locale",
        title: "musl 线 locale 固定行为判据（双线差异锚）",
        harness: Harness::MuslProbe,
        dep: 11646u16,
        need_cfg: CFG_MUSL,
        asserts: 4u16,
        default_suite: true,
    },
    Spec {
        card: 11648u16,
        tag: "f11648-gnuver-anchors",
        title: "符号版本机制总锚（.gnu.version 双节）判据",
        harness: Harness::StaticObj,
        dep: 11647u16,
        need_cfg: CFG_NONE,
        asserts: 9u16,
        default_suite: true,
    },
    Spec {
        card: 11649u16,
        tag: "f11649-linkver-missing",
        title: "链接期版本要求运行库缺版本报错判据",
        harness: Harness::ShellLib,
        dep: 11648u16,
        need_cfg: CFG_GLIBC,
        asserts: 6u16,
        default_suite: true,
    },
    Spec {
        card: 11650u16,
        tag: "f11650-dlvsym-null",
        title: "dlvsym 查询不存在版本 NULL 判据",
        harness: Harness::KselftestC,
        dep: 11649u16,
        need_cfg: CFG_GLIBC,
        asserts: 5u16,
        default_suite: true,
    },
    Spec {
        card: 11651u16,
        tag: "f11651-verneed-nosym",
        title: "VERNEED 指向不存在符号判据",
        harness: Harness::StaticObj,
        dep: 11650u16,
        need_cfg: CFG_NONE,
        asserts: 6u16,
        default_suite: true,
    },
    Spec {
        card: 11652u16,
        tag: "f11652-verhash-match",
        title: "版本哈希计算与匹配判据",
        harness: Harness::KselftestC,
        dep: 11651u16,
        need_cfg: CFG_NONE,
        asserts: 7u16,
        default_suite: true,
    },
    Spec {
        card: 11653u16,
        tag: "f11653-musl-no-symver",
        title: "musl 无符号版本机制静默忽略判据（双线差异锚）",
        harness: Harness::MuslProbe,
        dep: 11652u16,
        need_cfg: CFG_MUSL,
        asserts: 4u16,
        default_suite: true,
    },
    Spec {
        card: 11654u16,
        tag: "f11654-versection-oob",
        title: "版本节损坏（偏移越界）拒收判据",
        harness: Harness::StaticObj,
        dep: 11653u16,
        need_cfg: CFG_NONE,
        asserts: 8u16,
        default_suite: true,
    },
    Spec {
        card: 11655u16,
        tag: "f11655-check-map-ver",
        title: "版本校验失败路径判据（_dl_check_map_versions）",
        harness: Harness::KselftestC,
        dep: 11654u16,
        need_cfg: CFG_GLIBC,
        asserts: 6u16,
        default_suite: true,
    },
    Spec {
        card: 11656u16,
        tag: "f11656-weak-nover",
        title: "弱符号缺版本回退判据",
        harness: Harness::StaticObj,
        dep: 11655u16,
        need_cfg: CFG_NONE,
        asserts: 5u16,
        default_suite: true,
    },
    Spec {
        card: 11657u16,
        tag: "f11657-symver-verdef",
        title: ".symver 顶层版本定义判据",
        harness: Harness::KselftestC,
        dep: 11656u16,
        need_cfg: CFG_MODULES,
        asserts: 7u16,
        default_suite: true,
    },
    Spec {
        card: 11658u16,
        tag: "f11658-locale-matrix",
        title: "坏 locale × 工具链降级矩阵（20 工具抽样）",
        harness: Harness::AppletProbe,
        dep: 11657u16,
        need_cfg: CFG_BUSYBOX | CFG_ROOT,
        asserts: 8u16,
        default_suite: false,
    },
    Spec {
        card: 11659u16,
        tag: "f11659-errmsg-2sys",
        title: "符号版本不匹配报错文案双机逐字对照",
        harness: Harness::TwoSystem,
        dep: 11658u16,
        need_cfg: CFG_TWOSYS | CFG_GLIBC,
        asserts: 6u16,
        default_suite: true,
    },
];

/// 取清单一行。
pub fn spec_at(i: usize) -> Option<&'static Spec> {
    SPECS.get(i)
}

/// 按卡号找清单下标。
pub fn spec_index_of(card: u16) -> Option<usize> {
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = spec_at(i) {
            if s.card == card {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 本批内部断言总条数——聚合的第二层分母。
pub fn bench_assert_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = spec_at(i) {
            n = n.saturating_add(s.asserts as u32);
        }
        i += 1;
    }
    n
}

/// 声明了前置的条数（本批为 18：除首卡外逐字前置）。
pub const DEP_ROWS: usize = 18usize;

/// 统计清单里声明了前置的条数。
pub fn bench_dep_rows() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = spec_at(i) {
            if s.dep > 0u16 {
                n = n.saturating_add(1u32);
            }
        }
        i += 1;
    }
    n
}

/// 双线差异锚条数（musl 侧两条）。
pub fn bench_dual_anchors() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = spec_at(i) {
            if s.harness.needs_musl() {
                n = n.saturating_add(1u32);
            }
        }
        i += 1;
    }
    n
}

/// 进默认必跑集的条数。
pub fn bench_default_suite() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        if let Some(s) = spec_at(i) {
            if s.default_suite {
                n = n.saturating_add(1u32);
            }
        }
        i += 1;
    }
    n
}

/// 清单条数是否正好等于本批判据数。
pub fn manifest_count_ok() -> bool {
    SPECS.len() == BENCH
}

/// 清单里未被语料槽覆盖的条数。
///
/// 语料只有 14 槽（8 活 + 6 死），清单有 19 行，差的 5 条必须被显式数出来，
/// 否则"聚合 19 条"会悄悄退化成"聚合了手头的 14 条"——聚合单最常见的缩水
/// 不是造假，是漏条，而漏掉的条没人报红。
pub fn bench_uncovered() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < BENCH {
        let card = match spec_at(i) {
            Some(s) => s.card,
            None => return 0u32,
        };
        let mut found = false;
        let mut j = 0usize;
        while j < CORPUS {
            if let Some(sh) = shape_at(j) {
                if sh.card == card {
                    found = true;
                    break;
                }
            }
            j = j.saturating_add(1usize);
        }
        if !found {
            n = n.saturating_add(1u32);
        }
        i = i.saturating_add(1usize);
    }
    n
}

/// 卡号是否升序且无重复（本批前置就是前一张卡，故升序即拓扑序）。
pub fn manifest_sorted_ok() -> bool {
    let mut i = 1usize;
    while i < BENCH {
        let prev = match spec_at(i - 1usize) {
            Some(s) => s.card,
            None => return false,
        };
        let cur = match spec_at(i) {
            Some(s) => s.card,
            None => return false,
        };
        if cur <= prev {
            return false;
        }
        i += 1;
    }
    true
}

/// 短名唯一性。
pub fn manifest_tags_unique_ok() -> bool {
    let mut i = 0usize;
    while i < BENCH {
        let a = match spec_at(i) {
            Some(s) => s.tag,
            None => return false,
        };
        let mut j = i + 1usize;
        while j < BENCH {
            if let Some(b) = spec_at(j) {
                if b.tag == a {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 前置是否逐字等于"前一张卡"，且首卡无前置。任何一条不符都让执行序失去意义。
pub fn manifest_deps_ok() -> bool {
    let mut i = 0usize;
    while i < BENCH {
        let s = match spec_at(i) {
            Some(v) => v,
            None => return false,
        };
        if i == 0usize {
            if s.dep != 0u16 {
                return false;
            }
        } else {
            let prev = match spec_at(i - 1usize) {
                Some(v) => v.card,
                None => return false,
            };
            if s.dep != prev {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 依赖闸的执行序 = 卡号升序。这里显式做一次 Kahn 式的"无前置先出"检查，
/// 目的是让成环在结构上不可能被后面的次序闸掩盖。
pub fn topo_order() -> Option<[usize; BENCH]> {
    let mut out = [0usize; BENCH];
    let mut used = [false; BENCH];
    let mut k = 0usize;
    while k < BENCH {
        let mut found = None;
        let mut i = 0usize;
        while i < BENCH {
            let s = match spec_at(i) {
                Some(v) => v,
                None => return None,
            };
            let free = match s.dep {
                0u16 => true,
                d => match spec_index_of(d) {
                    Some(p) => *used.get(p).unwrap_or(&false),
                    None => return None,
                },
            };
            if !*used.get(i).unwrap_or(&false) && free {
                found = Some(i);
                break;
            }
            i += 1;
        }
        match found {
            Some(idx) => {
                if let Some(slot) = out.get_mut(k) {
                    *slot = idx;
                }
                if let Some(flag) = used.get_mut(idx) {
                    *flag = true;
                }
            }
            None => return None,
        }
        k += 1;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 文案族：ktest / kselftest 生态面的逐字原文
//
// 这里复刻的是真的输出行，大小写、空格、井号位置都不许改——聚合器判的就是这些行，
// 文案漂一个字，整条判据的绿就不是同一个绿。
// ---------------------------------------------------------------------------

/// TAP 头第一行。
pub const TAP_HEADER: &str = "TAP version 13\n";
/// TAP 计划行的前缀（后面跟 1..N）。
pub const TAP_PLAN: &str = "1..";
/// 结果行前缀（通过）。
pub const TAP_OK: &str = "ok ";
/// 结果行前缀（失败）。
pub const TAP_NOT_OK: &str = "not ok ";
/// 套件前缀，逐字对齐 kselftest 的 `selftests: <dir>: <name>`。
pub const TAP_SUITE: &str = " - selftests: unx: ";
/// skip 指令（TAP 语法里 skip 仍是 ok 行——本卡最大的陷阱）。
pub const TAP_SKIP_DIRECTIVE: &str = " # SKIP ";
/// xfail 指令（unexpected pass 会以 ok 行冒充绿）。
pub const TAP_XFAIL_DIRECTIVE: &str = " # XFAIL";
/// kselftest 经典计数行的前缀。
pub const KSFT_TOTALS: &str = "#Totals: pass:";
/// 计数行各段的原文。
pub const KSFT_FAIL_SEG: &str = " fail:";
/// 计数行 xfail 段。
pub const KSFT_XFAIL_SEG: &str = " xfail:";
/// 计数行 xpass 段（非零即为 unexpected pass，本卡判死）。
pub const KSFT_XPASS_SEG: &str = " xpass:";
/// 计数行 skip 段。
pub const KSFT_SKIP_SEG: &str = " skip:";
/// 计数行 error 段。
pub const KSFT_ERROR_SEG: &str = " error:";
/// 新版 kselftest 汇总行前缀。
pub const TAP_SUMMARY_OK: &str = "TAP test summary: ok:";
/// 新版汇总行的 not ok 段。
pub const TAP_SUMMARY_NOT: &str = " not ok:";
/// RUN 行前缀。
pub const KSFT_RUN: &str = "# RUN ";
/// 错误行前缀。
pub const KSFT_ERR: &str = "# ERROR: ";
/// ktest.pl 日志行原文。
pub const KTEST_LOG: &str = "Logging requested, log file = /tmp/ktest/B23.log\n";
/// ktest.pl 初始错误行原文。
pub const KTEST_INITIAL_ERR: &str = "Initial Error: ";
/// ktest.pl 跑完行原文。
pub const KTEST_DONE: &str = "Test Run Complete\n";
/// ktest.pl 重启行原文。
pub const KTEST_REBOOT: &str = "Rebooting machine...\n";
/// 本批日志文件名。
pub const KTEST_SRC_FILE: &str = "unx11660.c";

/// strerror 面：本批会真实碰到的四条，逐字对齐 glibc 的输出。
pub fn errno_text(e: i32) -> &'static str {
    match e {
        ENONE => "",
        ENOENT => "No such file or directory",
        EACCES => "Permission denied",
        ENOSPC => "No space left on device",
        ETIMEDOUT => "Connection timed out",
        _ => "Unknown error",
    }
}

/// 退出码文本：ktest 只关心数，但渲染行要写出 127 与 128+sig 的区别。
pub fn exit_text(code: i32) -> String {
    match code {
        EX_OK => String::from("0"),
        EX_FAIL => String::from("1"),
        EX_SKIP => String::from("4"),
        EX_NOTFOUND => String::from("127"),
        _ => i32_text(code),
    }
}

/// 一条判据的 TAP 结果行。
///
/// 这里必须逐字复刻 kselftest 的坑：skip 在 TAP 语法里写成 `ok N - name # SKIP 原因`，
/// 也就是"看着是绿行"。任何只数 `ok` 前缀的聚合器都会把没跑的判据计进通过率——
/// 本卡要的正是相反口径，所以绿集由 Tier::of 决定，与这一行的前缀无关。
/// 抖面写 `# XFAIL`，是同一族伪绿的第二个入口（unexpected pass 也长得像绿）。
pub fn msg_tap(idx: usize, name: &str, o: &Outcome) -> String {
    let mut s = String::new();
    let looks_ok = o.ok || o.note.skip_face();
    match looks_ok {
        true => s.push_str(TAP_OK),
        false => s.push_str(TAP_NOT_OK),
    }
    s.push_str(dec_text(idx as u32).as_str());
    s.push_str(TAP_SUITE);
    s.push_str(name);
    if o.ok && o.note == Note::FlakeRecovered {
        s.push_str(TAP_XFAIL_DIRECTIVE);
    } else if !o.ok && o.note.skip_face() {
        s.push_str(TAP_SKIP_DIRECTIVE);
        s.push_str(o.note.slug());
    }
    s.push('\n');
    s
}

/// TAP 头（版本行 + 计划行）。计划数永远是 BENCH，不是"实跑条数"。
pub fn msg_plan() -> String {
    let mut s = String::from(TAP_HEADER);
    s.push_str(TAP_PLAN);
    s.push_str(dec_text(BENCH as u32).as_str());
    s.push('\n');
    s
}

/// 经典计数行。
///
/// 六段必须逐段诚实，因为这一行是本批唯一的"总数守恒"证据：
/// `pass + fail + xfail + xpass + skip + error` 必须等于条数。
/// 本批没有"跑完且判负"的条（跑不完的算 error，没跑的算 skip），所以 fail 与
/// xfail 恒写 0；两条恢复成功的抖动在结果行里写成 `ok … # XFAIL`，
/// 严格 TAP 读法就是 unexpected pass，因此必须落进 xpass 而不是 pass——
/// 把它们混进 pass 正是"ktest 报 100% 但其实靠重试"的成因。
pub fn msg_totals(green: u32, flaked: u32, skipped: u32, blocked: u32) -> String {
    let mut s = String::from(KSFT_TOTALS);
    s.push_str(dec_text(green).as_str());
    s.push_str(KSFT_FAIL_SEG);
    s.push('0');
    s.push_str(KSFT_XFAIL_SEG);
    s.push('0');
    s.push_str(KSFT_XPASS_SEG);
    s.push_str(dec_text(flaked).as_str());
    s.push_str(KSFT_SKIP_SEG);
    s.push_str(dec_text(skipped).as_str());
    s.push_str(KSFT_ERROR_SEG);
    s.push_str(dec_text(blocked).as_str());
    s.push('\n');
    s
}

/// 新版汇总行。
///
/// 它与经典行的口径差必须原样保留：这一行是从结果行的前缀数出来的，
/// 而 skip 的结果行前缀就是 `ok`，所以 13/1 对经典行的 12/2。
/// 两行同屏互不相等，就是本卡存在的意义——只信其中一行必然被蒙。
pub fn msg_tap_summary(green: u32, flaked: u32, skipped: u32, blocked: u32) -> String {
    let mut s = String::from(TAP_SUMMARY_OK);
    s.push_str(
        dec_text(
            green
                .saturating_add(flaked)
                .saturating_add(skipped),
        )
        .as_str(),
    );
    s.push_str(TAP_SUMMARY_NOT);
    s.push_str(dec_text(blocked).as_str());
    s.push('\n');
    s
}

/// RUN 行。
pub fn msg_run(line: u32) -> String {
    let mut s = String::from(KSFT_RUN);
    s.push_str(KTEST_SRC_FILE);
    s.push(':');
    s.push_str(dec_text(line).as_str());
    s.push_str(" ...\n");
    s
}

/// 错误行：`# ERROR: <tag>: errno <n> (<strerror>)\n`，或 exit 面。
pub fn msg_error(tag: &str, errno: i32, code: i32) -> String {
    let mut s = String::from(KSFT_ERR);
    s.push_str(tag);
    s.push_str(": errno ");
    s.push_str(i32_text(errno).as_str());
    s.push_str(" (");
    s.push_str(errno_text(errno));
    s.push_str(") exit ");
    s.push_str(exit_text(code).as_str());
    s.push('\n');
    s
}

/// ktest.pl 面的收尾三行。
pub fn msg_ktest_tail(err: Option<&str>) -> String {
    let mut s = String::from(KTEST_LOG);
    if let Some(m) = err {
        s.push_str(KTEST_INITIAL_ERR);
        s.push_str(m);
        s.push('\n');
    }
    s.push_str(KTEST_REBOOT);
    s.push_str(KTEST_DONE);
    s
}

// ---------------------------------------------------------------------------
// 八闸纵线
// ---------------------------------------------------------------------------

/// 机上全位具备（本批所有 CONFIG/环境位的掩码）。
pub const CFG_ALL: u32 = CFG_MODULES | CFG_DEBUGFS | CFG_NET | CFG_TMPFS | CFG_ROOT
    | CFG_TWOSYS | CFG_MUSL | CFG_GLIBC | CFG_BUSYBOX | CFG_LOOP;

/// 缺一位的掩码（用于生成"没跑成"的事实）。
pub const CFG_NO_MUSL: u32 = CFG_ALL & !CFG_MUSL;
/// 缺 busybox 与 root。
pub const CFG_NO_BB: u32 = CFG_ALL & !(CFG_BUSYBOX | CFG_ROOT);

/// 从缺失位推出 skip 成因。成因只能由证据倒推，申报侧没有发言权。
fn derive_skip(missing: u32) -> Skip {
    if (missing & CFG_MUSL) != 0u32 {
        return Skip::NoMusl;
    }
    if (missing & CFG_TWOSYS) != 0u32 {
        return Skip::NoPeer;
    }
    if (missing & CFG_ROOT) != 0u32 {
        return Skip::NoRoot;
    }
    if (missing & (CFG_LOOP | CFG_TMPFS)) != 0u32 {
        return Skip::NoDevice;
    }
    if (missing & (CFG_BUSYBOX | CFG_MODULES | CFG_DEBUGFS | CFG_NET | CFG_GLIBC)) != 0u32 {
        return Skip::NoConfig;
    }
    // 一位不缺就不是"没跑成"。NoHarness 只能由载体自身缺席给出，
    // 从掩码反推永远推不出它——把它放在这里会让满配置的绿行集体误判成 skip。
    Skip::None
}

/// 结算一条判据：八闸顺序走完，任何一闸违约就在那里定局，后面的闸不再消费事实。
///
/// 这条纵线的不可倒置性见文档头：清单先于身份，身份先于依赖，依赖先于前置，
/// 前置先于执行，执行先于重试，重试先于汇总，汇总先于封印。
pub fn define(sh: &Shape) -> Outcome {
    // 闸一：清单——卡号必须落在本批号段，短名必须能进 TAP 行。
    if !sh.in_bench() || !sh.tag_ok() {
        return Outcome {
            ok: false,
            stage: Stage::Manifest,
            code: KA_ECOUNT,
            value: 0u32,
            note: Note::AssertShort,
            default: false,
            len: 0u32,
        };
    }
    // 闸二：身份——卡号/短名/段基址撞车是清单事实错，与跑得好坏无关。
    if sh.dup {
        return Outcome {
            ok: false,
            stage: Stage::Identity,
            code: KA_EUNIQ,
            value: sh.pass_asserts as u32,
            note: Note::DupIdentity,
            default: false,
            len: sh.log_bytes,
        };
    }
    // 闸三：依赖号——前置必须落在本批内，否则拓扑序无从排出。
    if sh.has_dep() && spec_index_of(sh.dep).is_none() {
        return Outcome {
            ok: false,
            stage: Stage::Deps,
            code: KA_ECYCLE,
            value: sh.pass_asserts as u32,
            note: Note::CycleLock,
            default: false,
            len: sh.log_bytes,
        };
    }
    // 闸四：前置——缺位就是没跑；申报"跑了"也不能把它抬进执行闸。
    if !sh.cfg_ready() {
        let why = derive_skip(sh.cfg_missing());
        return Outcome {
            ok: false,
            stage: Stage::Ready,
            code: KA_ESKIP,
            value: sh.pass_asserts as u32,
            note: Note::of_skip(why),
            default: false,
            len: sh.log_bytes,
        };
    }
    if sh.skip.is_skip() {
        return Outcome {
            ok: false,
            stage: Stage::Ready,
            code: KA_ESKIP,
            value: sh.pass_asserts as u32,
            note: Note::of_skip(sh.skip),
            default: false,
            len: sh.log_bytes,
        };
    }
    if sh.harness.needs_run() && !sh.ran {
        return Outcome {
            ok: false,
            stage: Stage::Ready,
            code: KA_ESKIP,
            value: 0u32,
            note: Note::HarnessMismatch,
            default: false,
            len: sh.log_bytes,
        };
    }
    // 闸五：执行——exit 与 errno 是硬事实，非零即红。
    if sh.exit_code != EX_OK || sh.errno_bit != ENONE {
        return Outcome {
            ok: false,
            stage: Stage::Exec,
            code: KA_ECODE,
            value: sh.pass_asserts as u32,
            note: Note::None,
            default: false,
            len: sh.log_bytes,
        };
    }
    // 闸六：重试——抖动必须显式；attempts>1 却说首次即中，就是洗绿。
    if sh.washed || (sh.attempts > 1u8 && sh.first_ok) {
        return Outcome {
            ok: false,
            stage: Stage::Retry,
            code: KA_EFLAKE,
            value: sh.attempts as u32,
            note: Note::FlakeRecovered,
            default: false,
            len: sh.log_bytes,
        };
    }
    // 闸七：汇总——前置未达成不得计绿；断言数不得短少。
    if sh.has_dep() && !sh.dep_done {
        return Outcome {
            ok: false,
            stage: Stage::Tally,
            code: KA_EBLOCK,
            value: sh.pass_asserts as u32,
            note: Note::DepBlocked,
            default: false,
            len: sh.log_bytes,
        };
    }
    if !sh.all_asserts_pass() {
        return Outcome {
            ok: false,
            stage: Stage::Tally,
            code: KA_EVALUE,
            value: sh.pass_asserts as u32,
            note: Note::AssertShort,
            default: false,
            len: sh.log_bytes,
        };
    }
    // 闸八：封印——走到这里才有资格谈默认必跑位。
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
        default: sh.default_suite,
        len: sh.log_bytes,
    }
}

/// 申报对实测的逐维对判。次序固定 Estate → Stage → Code → Value → Note → Default，
/// 成败互报优先于一切：先把"绿不绿"问清楚，再问绿在哪一闸、绿得对不对。
pub fn judge_against(claim: &Claim, actual: &Outcome) -> Fault {
    if claim.success() != actual.ok() {
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

/// 六个违约维，次序与 judge 一致。
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
    KA_ESTATE,
    KA_ESTAGE,
    KA_ECODE,
    KA_EVALUE,
    KA_ENOTE,
    KA_EDEFAULT,
];

/// 七个统计格 + 一个红档格（第 8 格必须是 929）。
///
/// 依次是：本批判据数 19、声明前置条数 18、归因档数 11、闸数 8、本模块判据项 13、
/// 载体种类 7、本批内部断言总条数 116、红档率 929。
///
/// 929 不是随手取的：它是"14 格里有 1 格对不上"的千分位通过率
/// （13×1000+7)/14 = 929。也就是说第 8 格记的正是本卡最核心的那件事——
/// 只要有一条判据被蒙进绿集，通过率就不再是 1000，而是 929。
pub const VALS: [u32; ACCEPT] = [19u32, 18u32, 11u32, 8u32, 13u32, 7u32, 116u32, RED_RATE];

/// 14 槽事实表：前 8 槽为申报成立的活槽，后 6 槽每槽只坏一个维度。
pub const SHAPES: [Shape; CORPUS] = [
    // 槽 0：活——清单首条，无前置，一次通过。
    Shape {
        card: 11641u16,
        tag: "f11641-lcall-bad",
        harness: Harness::KselftestC,
        stage: Stage::Seal,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 180u32,
        log_bytes: 512u32,
        asserts: 6u16,
        pass_asserts: 6u16,
        skip: Skip::None,
        dep: 0u16,
        dep_done: true,
        need_cfg: CFG_GLIBC,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 1：活——静态读节，不需要跑进程也有事实。
    Shape {
        card: 11648u16,
        tag: "f11648-gnuver-anchors",
        harness: Harness::StaticObj,
        stage: Stage::Seal,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 30u32,
        log_bytes: 240u32,
        asserts: 9u16,
        pass_asserts: 9u16,
        skip: Skip::None,
        dep: 11647u16,
        dep_done: true,
        need_cfg: CFG_NONE,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 2：活——musl 双线锚，第二次尝试才中，抖动必须显式。
    Shape {
        card: 11647u16,
        tag: "f11647-musl-locale",
        harness: Harness::MuslProbe,
        stage: Stage::Seal,
        ran: true,
        first_ok: false,
        attempts: 2u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 95u32,
        log_bytes: 300u32,
        asserts: 4u16,
        pass_asserts: 4u16,
        skip: Skip::None,
        dep: 11646u16,
        dep_done: true,
        need_cfg: CFG_MUSL,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 3：活——20 工具矩阵，跑通但不进默认必跑集（默认位为假）。
    Shape {
        card: 11658u16,
        tag: "f11658-locale-matrix",
        harness: Harness::AppletProbe,
        stage: Stage::Seal,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 1400u32,
        log_bytes: 4096u32,
        asserts: 8u16,
        pass_asserts: 8u16,
        skip: Skip::None,
        dep: 11657u16,
        dep_done: true,
        need_cfg: CFG_BUSYBOX | CFG_ROOT,
        have_cfg: CFG_ALL,
        default_suite: false,
        dup: false,
        washed: false,
    },
    // 槽 4：活——双机文案对照，本批最后一条。
    Shape {
        card: 11659u16,
        tag: "f11659-errmsg-2sys",
        harness: Harness::TwoSystem,
        stage: Stage::Seal,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 1800u32,
        log_bytes: 2048u32,
        asserts: 6u16,
        pass_asserts: 6u16,
        skip: Skip::None,
        dep: 11658u16,
        dep_done: true,
        need_cfg: CFG_TWOSYS | CFG_GLIBC,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 5：活——第三次才中，抖动两次的哈希判据。
    Shape {
        card: 11652u16,
        tag: "f11652-verhash-match",
        harness: Harness::KselftestC,
        stage: Stage::Seal,
        ran: true,
        first_ok: false,
        attempts: 3u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 260u32,
        log_bytes: 620u32,
        asserts: 7u16,
        pass_asserts: 7u16,
        skip: Skip::None,
        dep: 11651u16,
        dep_done: true,
        need_cfg: CFG_NONE,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 6：活——版本节越界拒收，静态面一次通过。
    Shape {
        card: 11654u16,
        tag: "f11654-versection-oob",
        harness: Harness::StaticObj,
        stage: Stage::Seal,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 28u32,
        log_bytes: 188u32,
        asserts: 8u16,
        pass_asserts: 8u16,
        skip: Skip::None,
        dep: 11653u16,
        dep_done: true,
        need_cfg: CFG_NONE,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 7：活——musl 目标不在场，诚实申报 skip（skip 仍判红，但申报没有撒谎）。
    Shape {
        card: 11653u16,
        tag: "f11653-musl-no-symver",
        harness: Harness::MuslProbe,
        stage: Stage::Ready,
        ran: false,
        first_ok: false,
        attempts: 1u8,
        exit_code: EX_SKIP,
        errno_bit: ENOENT,
        ms: 0u32,
        log_bytes: 96u32,
        asserts: 4u16,
        pass_asserts: 0u16,
        skip: Skip::NoMusl,
        dep: 11652u16,
        dep_done: true,
        need_cfg: CFG_MUSL,
        have_cfg: CFG_NO_MUSL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 8：死——只坏成败维：glibc 警告文案判据实跑权限不足，却申报通过。
    Shape {
        card: 11646u16,
        tag: "f11646-archive-warn",
        harness: Harness::ShellLib,
        stage: Stage::Exec,
        ran: true,
        first_ok: false,
        attempts: 1u8,
        exit_code: EX_FAIL,
        errno_bit: EACCES,
        ms: 600u32,
        log_bytes: 700u32,
        asserts: 5u16,
        pass_asserts: 2u16,
        skip: Skip::None,
        dep: 11645u16,
        dep_done: true,
        need_cfg: CFG_GLIBC | CFG_ROOT,
        have_cfg: CFG_ALL,
        default_suite: false,
        dup: false,
        washed: false,
    },
    // 槽 9：死——只坏闸序维：实跑到封印闸，日志却申报停在清单闸。
    Shape {
        card: 11650u16,
        tag: "f11650-dlvsym-null",
        harness: Harness::KselftestC,
        stage: Stage::Seal,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 210u32,
        log_bytes: 460u32,
        asserts: 5u16,
        pass_asserts: 5u16,
        skip: Skip::None,
        dep: 11649u16,
        dep_done: true,
        need_cfg: CFG_GLIBC,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 10：死——只坏码位维：绿账却挂了一个错误码。
    Shape {
        card: 11642u16,
        tag: "f11642-lang-baddir",
        harness: Harness::KselftestC,
        stage: Stage::Seal,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 320u32,
        log_bytes: 640u32,
        asserts: 5u16,
        pass_asserts: 5u16,
        skip: Skip::None,
        dep: 11641u16,
        dep_done: true,
        need_cfg: CFG_TMPFS | CFG_GLIBC,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 11：死——只坏计数维：4 条断言过 4 条，申报成过 6 条。
    Shape {
        card: 11644u16,
        tag: "f11644-lcnum-printf",
        harness: Harness::ShellLib,
        stage: Stage::Seal,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 400u32,
        log_bytes: 380u32,
        asserts: 4u16,
        pass_asserts: 4u16,
        skip: Skip::None,
        dep: 11643u16,
        dep_done: true,
        need_cfg: CFG_GLIBC,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 12：死——只坏归因维：一次通过却归注成抖动恢复。
    Shape {
        card: 11645u16,
        tag: "f11645-locale-return",
        harness: Harness::RawSh,
        stage: Stage::Seal,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 110u32,
        log_bytes: 260u32,
        asserts: 8u16,
        pass_asserts: 8u16,
        skip: Skip::None,
        dep: 11644u16,
        dep_done: true,
        need_cfg: CFG_NONE,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
    // 槽 13：死——只坏默认维：进了默认必跑集却申报没进。
    Shape {
        card: 11651u16,
        tag: "f11651-verneed-nosym",
        harness: Harness::StaticObj,
        stage: Stage::Seal,
        ran: true,
        first_ok: true,
        attempts: 1u8,
        exit_code: EX_OK,
        errno_bit: ENONE,
        ms: 25u32,
        log_bytes: 150u32,
        asserts: 6u16,
        pass_asserts: 6u16,
        skip: Skip::None,
        dep: 11650u16,
        dep_done: true,
        need_cfg: CFG_NONE,
        have_cfg: CFG_ALL,
        default_suite: true,
        dup: false,
        washed: false,
    },
];

/// 14 槽申报表——活槽逐字复刻诚实申报，死槽各坏一维。
pub const CLAIMS: [Claim; CORPUS] = [
    // 槽 0：诚实。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 6u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 1：诚实。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 9u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 2：诚实（抖动显式）。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 4u32,
        note: Note::FlakeRecovered,
        default: true,
    },
    // 槽 3：诚实（默认位为假）。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 8u32,
        note: Note::Clean,
        default: false,
    },
    // 槽 4：诚实。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 6u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 5：诚实（抖动显式）。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 7u32,
        note: Note::FlakeRecovered,
        default: true,
    },
    // 槽 6：诚实。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 8u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 7：诚实（skip 如实申报）。
    Claim {
        success: false,
        stage: Stage::Ready,
        code: KA_ESKIP,
        value: 0u32,
        note: Note::ConfigAbsent,
        default: false,
    },
    // 槽 8：坏成败——实跑失败说成通过。
    Claim {
        success: true,
        stage: Stage::Exec,
        code: KA_ECODE,
        value: 2u32,
        note: Note::None,
        default: false,
    },
    // 槽 9：坏闸序——走到封印闸却说停在清单闸。
    Claim {
        success: true,
        stage: Stage::Manifest,
        code: 0u32,
        value: 5u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 10：坏码位——绿账挂了码。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: KA_ECODE,
        value: 5u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 11：坏计数——多报两条断言。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 6u32,
        note: Note::Clean,
        default: true,
    },
    // 槽 12：坏归因——把干净通过说成抖动恢复。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 8u32,
        note: Note::FlakeRecovered,
        default: true,
    },
    // 槽 13：坏默认位——进了必跑集却说没进。
    Claim {
        success: true,
        stage: Stage::Seal,
        code: 0u32,
        value: 6u32,
        note: Note::Clean,
        default: false,
    },
];

// ---------------------------------------------------------------------------
// 期望表：把申报再用第二种形式复写一遍，任一处笔误都要在交叉校验里露出来
// ---------------------------------------------------------------------------

/// 期望成败。
pub const WANT_OK: [bool; CORPUS] = [
    true, true, true, true, true, true, true, false, false, true, true, true, true, true,
];

/// 期望停留闸。
pub const WANT_STAGE: [Stage; CORPUS] = [
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Ready,
    Stage::Exec,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
    Stage::Seal,
];

/// 期望错误码。
pub const WANT_CODE: [u32; CORPUS] = [
    0u32,
    0u32,
    0u32,
    0u32,
    0u32,
    0u32,
    0u32,
    KA_ESKIP,
    KA_ECODE,
    0u32,
    0u32,
    0u32,
    0u32,
    0u32,
];

/// 期望计数值（绿=内部断言条数，红=实过条数）。
pub const WANT_VALUE: [u32; CORPUS] = [
    6u32, 9u32, 4u32, 8u32, 6u32, 7u32, 8u32, 0u32, 2u32, 5u32, 5u32, 4u32, 8u32, 6u32,
];

/// 期望归因档。
pub const WANT_NOTE: [Note; CORPUS] = [
    Note::Clean,
    Note::Clean,
    Note::FlakeRecovered,
    Note::Clean,
    Note::Clean,
    Note::FlakeRecovered,
    Note::Clean,
    Note::ConfigAbsent,
    Note::None,
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::Clean,
    Note::Clean,
];

/// 期望默认位。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, true, false, true, true, true, false, false, true, true, true, true, true,
];

/// 期望档位。
pub const WANT_TIER: [Tier; CORPUS] = [
    Tier::Green,
    Tier::Green,
    Tier::Flaked,
    Tier::Green,
    Tier::Green,
    Tier::Flaked,
    Tier::Green,
    Tier::Skipped,
    Tier::Blocked,
    Tier::Green,
    Tier::Green,
    Tier::Green,
    Tier::Green,
    Tier::Green,
];

/// 期望缺陷维下标；`REJECT`（=6）表示该槽申报成立、无缺陷。
pub const WANT_NDX: [usize; CORPUS] = [
    REJECT, REJECT, REJECT, REJECT, REJECT, REJECT, REJECT, REJECT, 0usize, 1usize, 2usize,
    3usize, 4usize, 5usize,
];

/// 期望缺陷码；0 表示无缺陷。
pub const WANT_FAULT_CODE: [u32; CORPUS] = [
    0u32,
    0u32,
    0u32,
    0u32,
    0u32,
    0u32,
    0u32,
    0u32,
    KA_ESTATE,
    KA_ESTAGE,
    KA_ECODE,
    KA_EVALUE,
    KA_ENOTE,
    KA_EDEFAULT,
];

/// 槽标签：每一槽都写它实际占用的那条判据短名。
///
/// 死槽同样挂真判据的短名——缺陷形态（成败互报、档位前移……）是"这条真判据被
/// 报坏了"，不是另一条判据。形态名由 `Fault::slug()` 给，标签表只负责身份，
/// 两张表各管一面，才能在这里互查而不互相顶包。
pub const WANT_TAGS: [&'static str; CORPUS] = [
    "f11641-lcall-bad",
    "f11648-gnuver-anchors",
    "f11647-musl-locale",
    "f11658-locale-matrix",
    "f11659-errmsg-2sys",
    "f11652-verhash-match",
    "f11654-versection-oob",
    "f11653-musl-no-symver",
    "f11646-archive-warn",
    "f11650-dlvsym-null",
    "f11642-lang-baddir",
    "f11644-lcnum-printf",
    "f11645-locale-return",
    "f11651-verneed-nosym",
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
        i += 1;
    }
    out
}

/// 期望缺陷。
pub fn expected_fault(i: usize) -> Fault {
    match *WANT_NDX.get(i).unwrap_or(&REJECT) {
        n if n < REJECT => match REJECT_FAULTS.get(n) {
            Some(f) => *f,
            None => Fault::Ok,
        },
        _ => Fault::Ok,
    }
}

/// 死槽的"坏一维"申报表——用于交叉校验 CLAIMS 是否恰好坏在它声称的那一维。
pub const CLAIM_BROKEN: [Fault; REJECT] = REJECT_FAULTS;

/// 活槽计数。
pub fn live_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        if *WANT_NDX.get(i).unwrap_or(&REJECT) == REJECT {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 死槽计数。
pub fn dead_count() -> usize {
    CORPUS - live_count()
}

/// 档位计数。
pub fn tier_counts() -> [u32; TIER_COUNT] {
    let mut out = [0u32; TIER_COUNT];
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(o) = outcome_at(i) {
            let r = Tier::of(&o).rank() as usize;
            if let Some(slot) = out.get_mut(r) {
                *slot = slot.saturating_add(1u32);
            }
        }
        i += 1;
    }
    out
}

/// 绿集条数（Green + Flaked）。
pub fn green_count() -> u32 {
    let c = tier_counts();
    match (c.get(0usize), c.get(1usize)) {
        (Some(g), Some(f)) => g.saturating_add(*f),
        _ => 0u32,
    }
}

/// 红灯条数（Skipped + Blocked）。
pub fn red_count() -> u32 {
    let c = tier_counts();
    match (c.get(2usize), c.get(3usize)) {
        (Some(s), Some(b)) => s.saturating_add(*b),
        _ => 0u32,
    }
}

/// 走满全程的条数。
pub fn sealed_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(o) = outcome_at(i) {
            if o.stage == Stage::Seal && o.ok {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 计入默认必跑集的条数。
pub fn default_count() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(o) = outcome_at(i) {
            if o.default {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 一档的横计。
///
/// total 与 aligned 必须分开数：把"这条属于绿档"和"这条申报与实测对齐"
/// 混成一个数，就无法回答"绿档里有多少条是靠谎报蒙进来的"——而这正是
/// 聚合单最容易被做脏的地方（跑一绿就报 100%，中间几条靠 skip 混过去）。
#[derive(Clone, Copy)]
pub struct Row {
    /// 档位。
    pub tier: Tier,
    /// 该档条数。
    pub total: u32,
    /// 该档里申报与实测六维全对齐的条数。
    pub aligned: u32,
    /// 该档通过断言数合计。
    pub value_sum: u32,
    /// 该档走满八道闸的条数。
    pub sealed: u32,
    /// 该档内部断言总数合计（来自清单，不是实测）。
    pub spec_sum: u32,
}

impl Row {
    /// 空行。
    pub const fn new(t: Tier) -> Row {
        Row {
            tier: t,
            total: 0u32,
            aligned: 0u32,
            value_sum: 0u32,
            sealed: 0u32,
            spec_sum: 0u32,
        }
    }

    /// 该档是否"满对齐"——有条数且条数全对齐。
    ///
    /// 必须要求 total>0：空档的 aligned==total==0 会假装成满对齐，
    /// 那样四档只要有一条没人落地也能通过 perfect_rows()==4。
    pub const fn perfect(self: &Row) -> bool {
        self.total > 0u32 && self.aligned == self.total
    }

    /// 该行折进指纹。
    pub fn fold(self: &Row, h: u64) -> u64 {
        let a = fold_word(h, self.tier.rank() as u64);
        let b = fold_word(a, self.total as u64);
        let c = fold_word(b, self.aligned as u64);
        let d = fold_word(c, self.value_sum as u64);
        let e = fold_word(d, self.sealed as u64);
        fold_word(e, self.spec_sum as u64)
    }

    /// 该行文本。
    pub fn render(self: &Row) -> String {
        let mut s = String::from("  ");
        s.push_str(self.tier.slug());
        s.push_str(" total=");
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
#[derive(Clone, Copy)]
pub struct Matrix {
    /// 四行，按 Tier::rank() 落位。
    pub rows: [Row; TIER_COUNT],
}

impl Matrix {
    /// 全零矩阵。
    pub fn zero() -> Matrix {
        Matrix {
            rows: [
                Row::new(Tier::Green),
                Row::new(Tier::Flaked),
                Row::new(Tier::Skipped),
                Row::new(Tier::Blocked),
            ],
        }
    }

    /// 落一条。档位越界（不可能，但按契约取值必须走 match）返回错误码。
    pub fn bump(
        self: &mut Matrix,
        t: Tier,
        aligned: bool,
        value: u32,
        sealed: bool,
        spec: u32,
    ) -> Result<(), u32> {
        let r = match Tier::from_rank(t.rank()) {
            Some(x) => x.rank() as usize,
            None => return Err(KA_EUNIQ),
        };
        let slot = match self.rows.get_mut(r) {
            Some(x) => x,
            None => return Err(KA_EUNIQ),
        };
        slot.total = slot.total.saturating_add(1u32);
        if aligned {
            slot.aligned = slot.aligned.saturating_add(1u32);
        }
        slot.value_sum = slot.value_sum.saturating_add(value);
        if sealed {
            slot.sealed = slot.sealed.saturating_add(1u32);
        }
        slot.spec_sum = slot.spec_sum.saturating_add(spec);
        Ok(())
    }

    /// 取一行。
    pub fn row(self: &Matrix, t: Tier) -> Option<Row> {
        match self.rows.get(t.rank() as usize) {
            Some(r) => Some(*r),
            None => None,
        }
    }

    /// 满对齐行数。
    pub fn perfect_rows(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.perfect() {
                    n = n.saturating_add(1u32);
                }
            }
            i += 1;
        }
        n
    }

    /// 四档是否都有条落地。缺档说明语料根本没覆盖那条路径，聚合等于没验。
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
            i += 1;
        }
        true
    }

    /// 条数合计。
    pub fn total(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.total);
            }
            i += 1;
        }
        n
    }

    /// 对齐数合计。
    pub fn aligned(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.aligned);
            }
            i += 1;
        }
        n
    }

    /// 绿集条数（Green + Flaked）——通过率分子里"计入绿集"的那部分。
    pub fn in_green(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                if r.tier.in_green() {
                    n = n.saturating_add(r.total);
                }
            }
            i += 1;
        }
        n
    }

    /// 清单断言总数（四行 spec_sum 相加）。
    pub fn spec_total(self: &Matrix) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                n = n.saturating_add(r.spec_sum);
            }
            i += 1;
        }
        n
    }

    /// 是否发生过"绿档里藏未对齐条"——聚合单最阴的一种脏：总数看着全绿，
    /// 但绿档 aligned<total，说明有条靠 skip 指令或 XFAIL 混进绿集。
    pub fn green_has_lie(self: &Matrix) -> bool {
        match self.row(Tier::Green) {
            Some(r) => r.aligned < r.total,
            None => true,
        }
    }

    /// 折指纹。
    pub fn fold(self: &Matrix, h: u64) -> u64 {
        let mut out = h;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.rows.get(i) {
                out = r.fold(out);
            }
            i += 1;
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
            i += 1;
        }
        s
    }
}

/// 绿档条数——验收文案里的 green=。
pub const WANT_GREEN: u32 = 10u32;
/// 抖动档条数。
pub const WANT_FLAKED: u32 = 2u32;
/// 诚实 skip 档条数。
pub const WANT_SKIPPED: u32 = 1u32;
/// 前置阻塞档条数。
pub const WANT_BLOCKED: u32 = 1u32;
/// 全对齐条数（判据原文要求 100%）。
pub const WANT_MATCHED: u32 = CORPUS as u32;
/// 本批断言总数（19 条 × 各自内部断言，清单里逐项相加）。
pub const WANT_ASSERTS: u32 = 116u32;

/// 聚合报告：一次命令跑完 19 条判据之后要交出去的那张单子。
#[derive(Clone, Copy)]
pub struct Report {
    /// 矩阵。
    pub matrix: Matrix,
    /// 参与判定的槽数。
    pub total: u32,
    /// 六维全对齐的槽数。
    pub matched: u32,
    /// 本批判据条数（通过率分母，写死 19）。
    pub bench: u32,
    /// 本批内部断言总数。
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

    /// 通过率（千分）。四舍五入用 total/2 做偏移，与 B05 批锚同一套算法。
    pub fn rate(self: &Report) -> u32 {
        match self.total {
            0u32 => 0u32,
            n => {
                let num = self.matched.saturating_mul(RATE_FULL);
                num.saturating_add(n / 2u32) / n
            }
        }
    }

    /// 绿集条数。
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
    ///
    /// 判据原文只说通过率 100% 才算绿，没说不能抖；因此这里不并入 green()，
    /// 只作为独立问题暴露：flaky 条计入绿集，但不计入"稳定"。
    pub fn flake_free(self: &Report) -> bool {
        match self.matrix.row(Tier::Flaked) {
            Some(r) => r.total == 0u32,
            None => true,
        }
    }

    /// 走满全程条数。
    pub fn sealed(self: &Report) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TIER_COUNT {
            if let Some(r) = self.matrix.rows.get(i) {
                n = n.saturating_add(r.sealed);
            }
            i += 1;
        }
        n
    }

    /// 该报告是否可签发——八道独立闸，每道一个专属错误码，先撞先出。
    ///
    /// 顺序有讲究：先验分母（total/bench/asserts），再验分子（matched/rate），
    /// 最后才验档形（covered/perfect/各档条数）。反过来的话，一个把 total
    /// 改成 0 的假绿会先在 rate 上炸成除零保护，看起来像"分母没问题"。
    pub fn gate(self: &Report) -> Result<(), u32> {
        if self.total != WANT_MATCHED {
            return Err(KA_ESTATE);
        }
        if self.bench as usize != BENCH {
            return Err(KA_ESTAGE);
        }
        if self.asserts != WANT_ASSERTS {
            return Err(KA_ECODE);
        }
        if self.matched != self.total {
            return Err(KA_ECOUNT);
        }
        if self.rate() != RATE_FULL {
            return Err(KA_EVALUE);
        }
        if !self.matrix.covered() {
            return Err(KA_EUNIQ);
        }
        if self.matrix.green_has_lie() {
            return Err(KA_ESKIP);
        }
        if self.matrix.perfect_rows() as usize != TIER_COUNT {
            return Err(KA_EFLAKE);
        }
        if self.matrix.aligned() != self.total {
            return Err(KA_EBLOCK);
        }
        if self.sealed() == 0u32 {
            return Err(KA_EDEFAULT);
        }
        if self.tier_green() != WANT_GREEN {
            return Err(KA_ESLOT);
        }
        if self.tier_flaked() != WANT_FLAKED {
            return Err(KA_ESLOT);
        }
        if self.tier_skipped() != WANT_SKIPPED || self.tier_blocked() != WANT_BLOCKED {
            return Err(KA_ESLOT);
        }
        Ok(())
    }

    /// 折指纹（矩阵 + 三个标量）。
    pub fn fold(self: &Report, h: u64) -> u64 {
        let a = self.matrix.fold(h);
        let b = fold_word(a, self.total as u64);
        let c = fold_word(b, self.matched as u64);
        let d = fold_word(c, self.bench as u64);
        fold_word(d, self.asserts as u64)
    }

    /// 签发面单行——判据原文"通过率 100%"的逐字呈现。
    pub fn render(self: &Report) -> String {
        let mut s = String::from("ktest-b23-agg total=");
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
    pub const RENDER_OK: &'static str = "ktest-b23-agg total=14 accept=8 reject=6 \
                                         green=10 flaked=2 skipped=1 blocked=1 \
                                         bench=19 asserts=116 matched=14 rate=1000";
}

/// 构建聚合报告；`inj` 是"往某槽申报里故意坏一维"的注入槽位，用于证明
/// 六维对齐判定真的能抓到，而不是靠语料恰好干净。
///
/// 这里"对齐"的含义必须说清：一槽对齐 = 该槽判定结果等于期望结论
/// （活槽期望 Ok、死槽期望指定的那一维缺陷码）。这正是 ktest 语义——
/// 每条断言自带预期结论，聚合通过率问的是"预期结论全部命中"，
/// 而不是"全部申报为真"。若混成后者，14 槽里 6 个死槽会把通过率钉死在 571，
/// 那张单子就永远发不出去，于是必然有人把死槽从语料里删掉——假绿的源头。
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
        let spec_asserts = c.shape.asserts as u32;
        if m.bump(actual.tier(), aligned, actual.value, sealed, spec_asserts).is_err() {
            // 归档入口坏了就不出报告——出个"全零"报告，让 gate 的第一道闸
            // （total 不等于 14）把它判死，而不是继续累加出一个自洽的假数。
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

/// 带注入的聚合。槽位越界不当"没注入"处理——那是假绿的一种，
/// 直接判死。
pub fn run_with(inj: Option<(usize, Fault)>) -> Result<Report, u32> {
    if let Some((slot, _)) = inj {
        if slot >= CORPUS {
            return Err(KA_ESLOT);
        }
    }
    Ok(build_report(inj))
}

/// 错误码短名。
pub fn code_name(c: u32) -> &'static str {
    if c == 0u32 {
        return "none";
    }
    if c == KA_ESTATE {
        return "estate";
    }
    if c == KA_ESTAGE {
        return "estage";
    }
    if c == KA_ECODE {
        return "ecode";
    }
    if c == KA_EVALUE {
        return "evalue";
    }
    if c == KA_ENOTE {
        return "enote";
    }
    if c == KA_EDEFAULT {
        return "edefault";
    }
    if c == KA_ECOUNT {
        return "ecount";
    }
    if c == KA_EUNIQ {
        return "euniq";
    }
    if c == KA_ECYCLE {
        return "ecycle";
    }
    if c == KA_ESKIP {
        return "eskip";
    }
    if c == KA_EFLAKE {
        return "eflake";
    }
    if c == KA_EBLOCK {
        return "eblock";
    }
    if c == KA_ESLOT {
        return "eslot";
    }
    "unknown"
}

/// 缺陷明细（逐字进 TAP 诊断行）。
pub fn detail(slot: usize, f: Fault) -> String {
    let mut s = String::from("slot ");
    s.push_str(dec_text(slot as u32).as_str());
    s.push_str(" ");
    s.push_str(f.slug());
    s.push_str(" card ");
    if let Some(sh) = shape_at(slot) {
        s.push_str(dec_text(sh.card as u32).as_str());
        s.push_str(" tag ");
        s.push_str(sh.tag);
    } else {
        s.push_str("none");
    }
    s
}

/// 诊断行：把失败码与该槽真实 errno/exit 拼成 ktest 侧可读的一行。
pub fn diagnostic(slot: usize, f: Fault) -> String {
    let mut s = match shape_at(slot) {
        Some(sh) => msg_error("B23", sh.errno_bit, sh.exit_code),
        None => msg_error("B23", ENOENT, EX_FAIL),
    };
    s.push_str(" [");
    s.push_str(code_name(f.code()));
    s.push_str("] ");
    s.push_str(detail(slot, f).as_str());
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------
// 单调闸：一次运行的八道闸只能一道一道往前走
// ---------------------------------------------------------------------------

/// 逐闸推进的八道闸。
///
/// 存在的理由：聚合单里"这条判据跑到哪一步"是可以事后补写的，
/// 于是最常见的造假是把停在 Exec 的条说成走到了 Seal。本闸把
/// 停留位置变成运行时不变量——回退即死，越级即死，收口后再推进即死，
/// 且每一次被拒都进指纹，拒过的账不可能装作没发生过。
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

    /// 推进到指定闸。首步只能落清单闸，其后每步只能落在当前闸的下一道。
    pub fn advance(self: &mut Gate, s: Stage) -> Result<(), u32> {
        if self.closed {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(KA_ESTATE);
        }
        match self.cur {
            None => {
                // 闸序是 1 基：第一步只能落在清单闸，跳过清单闸就是没核过清单。
                if s.rank() != Stage::Manifest.rank() {
                    self.rejects = self.rejects.saturating_add(1u32);
                    return Err(KA_ESTAGE);
                }
            }
            Some(prev) => {
                // 只许走下一道闸，不许越闸：越级推进等于宣布"汇总已完成"却从没
                // 执行过——单调性只挡回退，挡不住跳过，而 ktest 聚合最常造的就是跳过。
                match prev.next() {
                    Some(nx) => {
                        if nx != s {
                            self.rejects = self.rejects.saturating_add(1u32);
                            return Err(KA_ESTAGE);
                        }
                    }
                    None => {
                        self.rejects = self.rejects.saturating_add(1u32);
                        return Err(KA_ESTAGE);
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
            return Err(KA_ESTATE);
        }
        match self.cur {
            Some(s) => {
                if !s.is_end() {
                    self.rejects = self.rejects.saturating_add(1u32);
                    return Err(KA_ESTAGE);
                }
            }
            None => {
                self.rejects = self.rejects.saturating_add(1u32);
                return Err(KA_ESTAGE);
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
    pub const fn closed(self: &Gate) -> bool {
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

    /// 是否恰好走满八道。
    pub const fn full_walk(self: &Gate) -> bool {
        self.moves == GATE_COUNT as u32 && self.closed
    }

    /// 按链表推进：STAGE_CHAIN 顺序逐一 advance。
    pub fn walk(self: &mut Gate) -> Result<(), u32> {
        let mut i = 0usize;
        while i < GATE_COUNT {
            let s = match STAGE_CHAIN.get(i) {
                Some(x) => *x,
                None => return Err(KA_EUNIQ),
            };
            if self.advance(s).is_err() {
                return Err(KA_ESTAGE);
            }
            i = i.saturating_add(1usize);
        }
        self.close()
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
/// 为什么必须有它：聚合报告里的 matched/rate 都是"算出来的"，算的过程
/// 一旦可以自便（比如跳格、补格），报告就只是叙事。账本把"第几格登了
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
    /// 冻结在后是因为一旦允许"冻结后还能拒"，拒次数就会污染已封存账，
    /// 那时候 replay 的失败原因会同时有两个来源，分不清是被改还是被动过。
    pub fn register(self: &mut Ledger, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(KA_ESTATE);
        }
        if s.slot != self.len {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(KA_ESLOT);
        }
        if self.len >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(KA_ESLOT);
        }
        if let Some(slot) = self.items.get_mut(self.len) {
            *slot = s;
        } else {
            self.rejects = self.rejects.saturating_add(1u32);
            return Err(KA_ESLOT);
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

    /// 逐格核账：统计格值必须落在 VALS 前 7 位、红档率格必须等于 RED_RATE、
    /// 缺陷格码必须落在 REJECT_CODES 且值为 0。任一不符即判死，返回首个不符码。
    pub fn audit(self: &Ledger) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(KA_ECOUNT);
        }
        let mut i = 0usize;
        while i < SAMPLES {
            let s = match self.sample(i) {
                Some(x) => x,
                None => return Err(KA_ESLOT),
            };
            if !s.op_in_range() {
                return Err(KA_EUNIQ);
            }
            if i < ACCEPT {
                let want = match VALS.get(i) {
                    Some(v) => *v,
                    None => return Err(KA_EUNIQ),
                };
                if !s.ok {
                    return Err(KA_ESTATE);
                }
                if s.code != 0u32 {
                    return Err(KA_ECODE);
                }
                if s.value != want {
                    return Err(KA_EVALUE);
                }
                if i == ACCEPT - 1 && s.op != OP_RED_RATE {
                    return Err(KA_EFLAKE);
                }
            } else {
                let idx = i - ACCEPT;
                let want_code = match REJECT_CODES.get(idx) {
                    Some(v) => *v,
                    None => return Err(KA_EUNIQ),
                };
                if s.ok {
                    return Err(KA_ESTATE);
                }
                if s.code != want_code {
                    return Err(KA_ECODE);
                }
                if s.value != 0u32 {
                    return Err(KA_EVALUE);
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
            return Err(KA_ESTATE);
        }
        if self.len != SAMPLES {
            return Err(KA_ECOUNT);
        }
        if self.audit().is_err() {
            return Err(KA_EUNIQ);
        }
        let r = self.fingerprint();
        self.root = Some(r);
        Ok(r)
    }

    /// 冻结——封存后再上锁；此后任何登记都会污染，而污染会立刻被 replay 抓到。
    pub fn freeze(self: &mut Ledger) -> Result<u64, u32> {
        if self.frozen {
            return Err(KA_ESTATE);
        }
        if self.rejects != 0u32 {
            return Err(KA_ECOUNT);
        }
        let mut r = 0u64;
        let mut have = false;
        if let Some(x) = self.root {
            r = x;
            have = true;
        }
        if !have {
            r = self.seal()?;
        }
        self.frozen = true;
        Ok(r)
    }

    /// 重放：从当前格序列重建指纹并与封存根比对。
    pub fn replay(self: &Ledger) -> Result<u64, u32> {
        let want = match self.root {
            Some(x) => x,
            None => return Err(KA_ESTATE),
        };
        if self.fingerprint() != want {
            return Err(KA_EUNIQ);
        }
        Ok(want)
    }

    /// 偷改一格数值——只为验证重放真的能抓到，不参与正常流程。
    pub fn poke(self: &mut Ledger, slot: usize, value: u32) -> Result<(), u32> {
        if !self.frozen && self.root.is_none() {
            return Err(KA_ESTATE);
        }
        if slot >= self.len {
            return Err(KA_ESLOT);
        }
        if let Some(s) = self.items.get_mut(slot) {
            s.value = value;
            Ok(())
        } else {
            Err(KA_ESLOT)
        }
    }

    /// 账本文本（含表头）。
    pub fn render(self: &Ledger) -> String {
        let mut s = String::from("ledger cells=");
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
                s.push_str(hex_text((r >> 32) as u32, 8).as_str());
                s.push_str(hex_text((r & 0xffff_ffffu64) as u32, 8).as_str());
                s.push_str(if self.frozen { " frozen\n" } else { " sealed\n" });
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
/// 这四路对应聚合单能被做脏的四个位置——清单本身、判定函数、签发面、
/// 逐格账。只测其中一路等于没测：本卡验收原文要求"通过率 100% 才算绿"，
/// 而通过率只在渲染面被读出来，所以渲染路必须单独注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // 表路：伪造一行清单（卡号越界、断言数为 0、短名为空），
    // 清单逐行闸必须判它不合格；反过来，真清单 19 行必须全合格。
    let fake = Spec {
        card: 0u16,
        tag: "",
        title: "fake",
        harness: Harness::KselftestC,
        dep: 9u16,
        need_cfg: CFG_NONE,
        asserts: 0u16,
        default_suite: false,
    };
    let table_caught = !spec_row_ok(&fake) && manifest_rows_ok();
    if let Some(s) = out.get_mut(0usize) {
        *s = table_caught;
    }

    // 判路：拿第 0 槽（活槽）逐维注坏，六次必须全部出码而不是 Ok。
    let mut judged = 0u32;
    if let Some(c) = case_at(0usize) {
        let a = c.actual();
        let mut k = 0usize;
        while k < REJECT {
            let f = match REJECT_FAULTS.get(k) {
                Some(x) => *x,
                None => break,
            };
            let tampered = c.claim.break_dim(f);
            if judge_against(&tampered, &a) != Fault::Ok {
                judged = judged.saturating_add(1u32);
            }
            k = k.saturating_add(1usize);
        }
    }
    if let Some(s) = out.get_mut(1usize) {
        *s = judged == REJECT as u32;
    }

    // 渲染路：干净报告的签发面必须逐字等于锚；注坏一档后签发面必须变。
    let clean = run();
    let clean_ok = clean.render().as_str() == Report::RENDER_OK;
    let dirty = match run_with(Some((0usize, Fault::Value))) {
        Ok(r) => r,
        Err(_) => Report::invalid(),
    };
    if let Some(s) = out.get_mut(2usize) {
        *s = clean_ok && dirty.render().as_str() != clean.render().as_str();
    }

    // 账本路：封存后偷改一格数值，重放必须失败。
    let mut l = build_ledger();
    let mut caught = false;
    if let Ok(r) = l.seal() {
        if let Some(v) = l.sample(0usize) {
            let shift = v.value.saturating_add(1u32);
            if l.poke(0usize, shift).is_ok() {
                caught = l.replay().is_err() && l.root() == Some(r);
            }
        }
    }
    if let Some(s) = out.get_mut(3usize) {
        *s = caught;
    }
    out
}

// ---------------------------------------------------------------------------
// 私判：13 项判据的谓词层
// ---------------------------------------------------------------------------

/// 清单单行自洽判据。
///
/// 这是表路注入的靶子：伪造一行（卡号越界 / 短名空 / 断言数 0）必须被拒，
/// 否则"聚合 19 条"可以靠往里塞空行凑数。
fn spec_row_ok(s: &Spec) -> bool {
    if s.card < CARD_MIN || s.card > CARD_MAX {
        return false;
    }
    if s.tag.is_empty() {
        return false;
    }
    if s.title.is_empty() {
        return false;
    }
    if s.asserts == 0u16 {
        return false;
    }
    if s.dep != 0u16 && (s.dep < CARD_MIN || s.dep > CARD_MAX) {
        return false;
    }
    if s.dep == s.card {
        return false;
    }
    // 载体序号是 1 基：合法区间 1..=HARNESS_COUNT，0 与越界都不是载体。
    if s.harness.rank() == 0u8 || s.harness.rank() > HARNESS_COUNT as u8 {
        return false;
    }
    true
}

/// 19 行逐行自洽。
fn manifest_rows_ok() -> bool {
    let mut i = 0usize;
    while i < BENCH {
        match spec_at(i) {
            Some(s) => {
                if !spec_row_ok(s) {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1usize);
    }
    true
}

/// 结局逐字段相等判定。
///
/// 只用七个字段逐项比，不用 derive：推演是否稳定是本卡的一条不变量，
/// 把它写成显式比较，将来给 Outcome 加字段时必须在这里补一格，
/// 而 derive 会静默地把新字段也变成"必须相等"，反过来掩盖推演漏算。
fn same_outcome(a: &Outcome, b: &Outcome) -> bool {
    a.ok == b.ok
        && a.stage == b.stage
        && a.code == b.code
        && a.value == b.value
        && a.note == b.note
        && a.default == b.default
        && a.len == b.len
}

/// KA01：段基址、13 条码的连续性与码名覆盖。
fn name_ok() -> bool {
    if SEG_BASE != 0x6cf50u32 {
        return false;
    }
    let codes: [u32; 13usize] = [
        KA_ESTATE,
        KA_ESTAGE,
        KA_ECODE,
        KA_EVALUE,
        KA_ENOTE,
        KA_EDEFAULT,
        KA_ECOUNT,
        KA_EUNIQ,
        KA_ECYCLE,
        KA_ESKIP,
        KA_EFLAKE,
        KA_EBLOCK,
        KA_ESLOT,
    ];
    let mut i = 0usize;
    while i < 13usize {
        let c = match codes.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if c != SEG_BASE.saturating_add(i as u32).saturating_add(1u32) {
            return false;
        }
        if code_name(c) == "unknown" {
            return false;
        }
        let mut j = i.saturating_add(1usize);
        while j < 13usize {
            if let Some(o) = codes.get(j) {
                if *o == c {
                    return false;
                }
            }
            j = j.saturating_add(1usize);
        }
        i = j;
    }
    if code_name(0u32) != "none" {
        return false;
    }
    if code_name(SEG_BASE.saturating_add(99u32)) != "unknown" {
        return false;
    }
    true
}

/// KA02：八道闸链与十一档归因的结构闭合。
fn enums_ok() -> bool {
    // 闸链必须逐级 +1，且最后一道没有后继。
    let mut i = 0usize;
    while i < GATE_COUNT {
        let s = match STAGE_CHAIN.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if s.rank() as usize != i.saturating_add(1usize) {
            return false;
        }
        if let Some(from_rank) = Stage::from_rank(s.rank()) {
            if from_rank != s {
                return false;
            }
        } else {
            return false;
        }
        match s.next() {
            Some(n) => {
                if i + 1usize >= GATE_COUNT || n.rank() != s.rank() + 1u8 {
                    return false;
                }
                if s.is_end() {
                    return false;
                }
            }
            None => {
                if !s.is_end() || i + 1usize != GATE_COUNT {
                    return false;
                }
            }
        }
        // 每道闸的专属缺陷码必须互不相同，否则停留档与错误码无法互推。
        let fc = s.fault_code();
        let mut j = 0usize;
        while j < i {
            if let Some(p) = STAGE_CHAIN.get(j) {
                if p.fault_code() == fc {
                    return false;
                }
            }
            j = j.saturating_add(1usize);
        }
        if fc == 0u32 || code_name(fc) == "unknown" {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 执行后判定：跨过执行闸之后（重试起）才有资格谈抖动与计数，
    // 正停在执行闸本身还不算"过来"——这条界线决定 Value 维能不能被消费。
    if let Some(ex) = Stage::from_rank(Stage::Exec.rank()) {
        if ex.past_exec() {
            return false;
        }
    } else {
        return false;
    }
    if let Some(rd) = Stage::from_rank(Stage::Ready.rank()) {
        if rd.past_exec() {
            return false;
        }
    } else {
        return false;
    }
    if let Some(rt) = Stage::from_rank(Stage::Retry.rank()) {
        if !rt.past_exec() {
            return false;
        }
    } else {
        return false;
    }
    // 归因档：绿面与 skip 面互斥，且恰好各有所属。
    let mut green_notes = 0u32;
    let mut skip_notes = 0u32;
    let mut k = 0u8;
    while (k as usize) < NOTE_COUNT {
        let n = match Note::from_rank(k) {
            Some(x) => x,
            None => return false,
        };
        if n.rank() != k {
            return false;
        }
        if n.green_face() && n.skip_face() {
            return false;
        }
        if n.green_face() {
            green_notes = green_notes.saturating_add(1u32);
        }
        if n.skip_face() {
            skip_notes = skip_notes.saturating_add(1u32);
        }
        // 无归因档没有短名可露（它不进 TAP 注释位），其余十档必须有非空且互异的短名。
        if n.rank() == 0u8 {
            if !n.slug().is_empty() {
                return false;
            }
        } else if n.slug().is_empty() {
            return false;
        } else {
            let mut m = 1u8;
            while (m as usize) < NOTE_COUNT {
                if m < n.rank() {
                    if let Some(p) = Note::from_rank(m) {
                        if p.slug() == n.slug() {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                m = m.saturating_add(1u8);
            }
        }
        k = k.saturating_add(1u8);
    }
    if let Some(f) = Note::from_rank(NOTE_COUNT as u8) {
        // 越界序号必须还原不出档位。
        let _ = f;
        return false;
    }
    green_notes == 2u32 && skip_notes == 4u32
}

/// KA03 头：七种载体与位掩码的自洽。
fn harness_ok() -> bool {
    let mut seen = 0u32;
    let mut i = 1u8;
    let mut ran_kinds = 0u32;
    let mut two_sys = 0u32;
    let mut musl = 0u32;
    let mut perm_kinds = 0u32;
    while (i as usize) <= HARNESS_COUNT {
        let h = match Harness::from_rank(i) {
            Some(x) => x,
            None => return false,
        };
        if h.rank() != i || h.slug().is_empty() {
            return false;
        }
        if h.miss_errno() == ENONE {
            return false;
        }
        if h.ms_band() == 0u32 {
            return false;
        }
        // 缺席证据只有两种硬事实：找不到可执行体（ENOENT）与没有权限跑（EACCES）。
        if h.miss_errno() != ENOENT && h.miss_errno() != EACCES {
            return false;
        }
        if h.miss_errno() == EACCES {
            perm_kinds = perm_kinds.saturating_add(1u32);
        }
        // 需要双机或需要 musl 的载体必然要真跑——静态读节不可能跨系统。
        if (h.needs_two_sys() || h.needs_musl()) && !h.needs_run() {
            return false;
        }
        if h.needs_run() {
            ran_kinds = ran_kinds.saturating_add(1u32);
        }
        if h.needs_two_sys() {
            two_sys = two_sys.saturating_add(1u32);
        }
        if h.needs_musl() {
            musl = musl.saturating_add(1u32);
        }
        let mut j = 1u8;
        while j < i {
            if let Some(p) = Harness::from_rank(j) {
                if p.slug() == h.slug() {
                    return false;
                }
            } else {
                return false;
            }
            j = j.saturating_add(1u8);
        }
        seen = seen.saturating_add(1u32);
        i = i.saturating_add(1u8);
    }
    if let Some(x) = Harness::from_rank(HARNESS_COUNT as u8) {
        // 序号 7 是 static-objdump，它有效；越界必须从 8 开始。
        let _ = x;
    }
    if Harness::from_rank(HARNESS_COUNT.saturating_add(1usize) as u8).is_some() {
        return false;
    }
    // 载体必须从 1 基连续排到 7，且 0 号永远还原不出东西。
    if Harness::from_rank(0u8).is_some() {
        return false;
    }
    // 七种载体全覆盖：只有静态读节不真跑，其余六种都要目标机把进程跑起来；
    // 双机与 musl 各恰好一种，合起来才是本批的两条"双线差异锚"。
    seen == HARNESS_COUNT as u32
        && ran_kinds.saturating_add(1u32) == HARNESS_COUNT as u32
        && two_sys.saturating_add(musl) == DUAL_ANCHORS as u32
        && perm_kinds == 1u32
}

/// KA03 尾：skip 成因与位、归因档的三方一致。
fn skip_ok() -> bool {
    let mut i = 0u8;
    let mut causes = 0u32;
    while (i as usize) < 7usize {
        let s = match Skip::from_rank(i) {
            Some(x) => x,
            None => return false,
        };
        if s.rank() != i {
            return false;
        }
        if s.is_skip() != (i > 0u8) {
            return false;
        }
        // 没跳过的成因不该有 errno；真跳过的必须带一条机器面硬证据。
        if s.is_skip() && s.errno() == ENONE {
            return false;
        }
        if !s.is_skip() && s.errno() != ENONE {
            return false;
        }
        let n = Note::of_skip(s);
        if s.is_skip() && !n.skip_face() {
            return false;
        }
        if !s.is_skip() && n != Note::None {
            return false;
        }
        // NoConfig 是"任一位缺失"的合集成因，本身不占单一位；NoHarness 是载体不在，
        // 与 CONFIG 无关。其余五种成因必须各自指向一个确切缺失位。
        let eats_bit = s != Skip::NoConfig && s != Skip::NoHarness;
        if s.is_skip() && eats_bit && s.consumed_bit() == CFG_NONE {
            return false;
        }
        if !eats_bit && s.consumed_bit() != CFG_NONE {
            return false;
        }
        if s.consumed_bit() != CFG_NONE && (s.consumed_bit() & CFG_ALL) == CFG_NONE {
            return false;
        }
        causes = causes.saturating_add(1u32);
        i = i.saturating_add(1u8);
    }
    if let Some(x) = Skip::from_rank(7u8) {
        let _ = x;
        return false;
    }
    // 反向：缺 musl 位必须倒推出 NoMusl。
    derive_skip(CFG_MUSL) == Skip::NoMusl
        && derive_skip(CFG_ROOT) == Skip::NoRoot
        && derive_skip(CFG_TWOSYS) == Skip::NoPeer
        && derive_skip(CFG_NONE) == Skip::None
        && causes == 7u32
}

/// KA04：清单 19 行的整体形状。
fn manifest_ok() -> bool {
    if !manifest_count_ok() {
        return false;
    }
    if !manifest_sorted_ok() {
        return false;
    }
    if !manifest_tags_unique_ok() {
        return false;
    }
    if !manifest_deps_ok() {
        return false;
    }
    if !manifest_rows_ok() {
        return false;
    }
    if bench_assert_total() != WANT_ASSERTS {
        return false;
    }
    if bench_dep_rows() != 18u32 {
        return false;
    }
    if bench_dual_anchors() != DUAL_ANCHORS as u32 {
        return false;
    }
    if bench_default_suite() != 17u32 {
        return false;
    }
    // 取越界项必须给 None，而不是回绕到首行。
    if spec_at(BENCH).is_some() {
        return false;
    }
    // 卡号→下标反查与下标→卡号必须互逆。
    let mut i = 0usize;
    while i < BENCH {
        let s = match spec_at(i) {
            Some(x) => x,
            None => return false,
        };
        match spec_index_of(s.card) {
            Some(j) => {
                if j != i {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1usize);
    }
    if spec_index_of(CARD_MIN.saturating_sub(1u16)).is_some() {
        return false;
    }
    // 拓扑序必须排得出来，且长度等于 19、逐项落在批内。
    let order = match topo_order() {
        Some(o) => o,
        None => return false,
    };
    let mut seen = [false; BENCH];
    let mut k = 0usize;
    while k < BENCH {
        let idx = match order.get(k) {
            Some(x) => *x,
            None => return false,
        };
        if idx >= BENCH {
            return false;
        }
        if let Some(slot) = seen.get_mut(idx) {
            if *slot {
                return false;
            }
            *slot = true;
        } else {
            return false;
        }
        k = k.saturating_add(1usize);
    }
    true
}

/// KA05：八闸纵线推演——14 槽事实逐维对齐期望表。
fn define_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let sh = match shape_at(i) {
            Some(x) => x,
            None => return false,
        };
        let o = match outcome_at(i) {
            Some(x) => x,
            None => return false,
        };
        if !sh.in_bench() {
            return false;
        }
        // 推演必须可重复：同事实两次推演逐字段相同。
        let again = define(&sh);
        if !same_outcome(&again, &o) {
            return false;
        }
        let want_ok = match WANT_OK.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_stage = match WANT_STAGE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if o.ok != want_ok || o.stage != want_stage {
            return false;
        }
        let want_code = match WANT_CODE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if o.code != want_code {
            return false;
        }
        let want_value = match WANT_VALUE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if o.value != want_value {
            return false;
        }
        let want_note = match WANT_NOTE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if o.note != want_note {
            return false;
        }
        let want_default = match WANT_DEFAULT.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if o.default != want_default {
            return false;
        }
        let want_tier = match WANT_TIER.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if o.tier() != want_tier {
            return false;
        }
        // 默认位只有在走满全程且绿时才可能为真。
        if o.default && !(o.stage == Stage::Seal && o.ok) {
            return false;
        }
        // 计数值不得越过该条内部断言总数。
        if o.value > sh.asserts as u32 {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 越界推演不得伪装成第 0 槽。
    if outcome_at(CORPUS).is_some() {
        return false;
    }
    true
}

/// KA06：六维判定——逐槽结论等于期望结论，且诚实申报必过。
fn judge_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let c = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        if judge(&c) != expected_fault(i) {
            return false;
        }
        // 关键不变量：把实测结局原样抄成申报，判定必须是 Ok。
        // 若这条不成立，说明判据在跟申报玩文字游戏，而不是在比事实。
        let honest = Claim::from_outcome(&c.actual());
        if judge_against(&honest, &c.actual()) != Fault::Ok {
            return false;
        }
        // 期望缺陷码表与期望缺陷维表必须互推。
        let ndx = match WANT_NDX.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_code = match WANT_FAULT_CODE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if ndx == REJECT {
            if want_code != 0u32 || expected_fault(i) != Fault::Ok {
                return false;
            }
        } else {
            let f = match REJECT_FAULTS.get(ndx) {
                Some(x) => *x,
                None => return false,
            };
            if expected_fault(i) != f || f.code() != want_code {
                return false;
            }
            if want_code != want_code_from(i) {
                return false;
            }
        }
        i = i.saturating_add(1usize);
    }
    true
}

/// 由槽位取期望结论对应的错误码（判定表与码表之间的第三条链）。
fn want_code_from(i: usize) -> u32 {
    match expected_fault(i) {
        Fault::Ok => 0u32,
        f => f.code(),
    }
}

/// KA07：四档矩阵——覆盖、满对齐、条数分布、绿档无谎。
fn matrix_ok() -> bool {
    let r = run();
    if r.total != WANT_MATCHED {
        return false;
    }
    if r.matrix.total() != CORPUS as u32 {
        return false;
    }
    if !r.matrix.covered() {
        return false;
    }
    if r.matrix.perfect_rows() as usize != TIER_COUNT {
        return false;
    }
    if r.matrix.green_has_lie() {
        return false;
    }
    if r.matrix.in_green() != WANT_GREEN.saturating_add(WANT_FLAKED) {
        return false;
    }
    let want: [u32; TIER_COUNT] = [WANT_GREEN, WANT_FLAKED, WANT_SKIPPED, WANT_BLOCKED];
    let mut i = 0usize;
    while i < TIER_COUNT {
        let t = match Tier::from_rank(i as u8) {
            Some(x) => x,
            None => return false,
        };
        let row = match r.matrix.row(t) {
            Some(x) => x,
            None => return false,
        };
        match want.get(i) {
            Some(w) => {
                if row.total != *w {
                    return false;
                }
            }
            None => return false,
        }
        if row.aligned != row.total {
            return false;
        }
        // 红档不得有走满全程的条——走满即绿，这是闸链的推论。
        if !t.in_green() && row.sealed != 0u32 {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 矩阵条数合计必须等于逐档直数，两条路径互验。
    if r.matrix.total() != tier_counts().iter().fold(0u32, |a, b| a.saturating_add(*b)) {
        return false;
    }
    if r.sealed() != sealed_count() as u32 {
        return false;
    }
    true
}

/// KA08：语料活死配平与档位分布。
fn corpus_ok() -> bool {
    if live_count() != ACCEPT || dead_count() != REJECT {
        return false;
    }
    let c = tier_counts();
    if let Some(v) = c.get(0usize) {
        if *v != WANT_GREEN {
            return false;
        }
    } else {
        return false;
    }
    if let Some(v) = c.get(1usize) {
        if *v != WANT_FLAKED {
            return false;
        }
    } else {
        return false;
    }
    if let Some(v) = c.get(2usize) {
        if *v != WANT_SKIPPED {
            return false;
        }
    } else {
        return false;
    }
    if let Some(v) = c.get(3usize) {
        if *v != WANT_BLOCKED {
            return false;
        }
    } else {
        return false;
    }
    if green_count() != WANT_GREEN.saturating_add(WANT_FLAKED) {
        return false;
    }
    if red_count() != WANT_SKIPPED.saturating_add(WANT_BLOCKED) {
        return false;
    }
    // 14 槽里进默认必跑集的是 11 条：3 条不入集（诚实 skip、依赖阻断、
    // 以及 F11658 那条申报默认位为假的绿判据）。
    if default_count() != 11usize {
        return false;
    }
    // 19 条清单里恰有 5 条只以清单身份存在、没有语料槽；这个差必须被数出来。
    if bench_uncovered() != (BENCH - CORPUS) as u32 {
        return false;
    }
    // cases() 与逐槽取用必须一致。
    let all = cases();
    let mut i = 0usize;
    while i < CORPUS {
        let a = match all.get(i) {
            Some(x) => x,
            None => return false,
        };
        let b = match case_at(i) {
            Some(x) => x,
            None => return false,
        };
        if a.shape.card != b.shape.card || a.claim.success != b.claim.success {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    true
}

/// KA08 尾：六条死槽的缺陷维与码必须两两不同。
fn reds_ok() -> bool {
    if REJECT_FAULTS.len() != REJECT || REJECT_CODES.len() != REJECT {
        return false;
    }
    let mut i = 0usize;
    while i < REJECT {
        let f = match REJECT_FAULTS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let c = match REJECT_CODES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if f == Fault::Ok || f.code() != c {
            return false;
        }
        if code_name(c) == "unknown" {
            return false;
        }
        let mut j = i.saturating_add(1usize);
        while j < REJECT {
            if let (Some(x), Some(y)) = (REJECT_FAULTS.get(j), REJECT_CODES.get(j)) {
                if *x == f || *y == c {
                    return false;
                }
            }
            j = j.saturating_add(1usize);
        }
        // 记账面与非记账面的划分不能反过来。
        if f.accounting() != (f == Fault::Value || f == Fault::Default) {
            return false;
        }
        if f.estate() != (f == Fault::Estate) {
            return false;
        }
        i = j;
    }
    // VALS 第 8 格必须是红档率，且前 7 格全非零。
    if let Some(v) = VALS.get(ACCEPT.saturating_sub(1usize)) {
        if *v != RED_RATE {
            return false;
        }
    } else {
        return false;
    }
    let mut k = 0usize;
    while k < ACCEPT.saturating_sub(1usize) {
        if let Some(v) = VALS.get(k) {
            if *v == 0u32 || *v == RED_RATE {
                return false;
            }
        } else {
            return false;
        }
        k = k.saturating_add(1usize);
    }
    // 13/14 的千分位恰为 929：红档率格的来历必须能重算。
    let demo = Report {
        matrix: Matrix::zero(),
        total: CORPUS as u32,
        matched: CORPUS as u32 - 1u32,
        bench: BENCH as u32,
        asserts: WANT_ASSERTS,
    };
    demo.rate() == RED_RATE
}

/// KA09：语料槽的卡号、短名与清单互查。
fn slots_ok() -> bool {
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
        if !sh.in_bench() {
            return false;
        }
        // 每一槽的短名都必须在清单里反查到同一条：活槽报对、死槽报坏，
        // 报坏的是"这条真判据"，不是凭空多出来的一条。载体与默认位同理。
        match spec_index_of(sh.card) {
            Some(j) => {
                if let Some(sp) = spec_at(j) {
                    if sp.tag != sh.tag || sp.harness != sh.harness {
                        return false;
                    }
                    if sp.default_suite != sh.default_suite {
                        return false;
                    }
                    // 断言总数取清单行，实跑通过数才是事实面。
                    if sh.asserts != sp.asserts {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            None => return false,
        }
        // 卡号互不相同：14 槽各占一条判据，不许复用同一条凑数。
        // 查重用自己的游标，不能借用外层游标——借了就只能查出"后一半不存在"，
        // 第 0 槽之后循环直接结束，整张身份表退化成只验一槽。
        let mut k = i.saturating_add(1usize);
        while k < CORPUS {
            if let Some(o) = shape_at(k) {
                if o.card == sh.card || o.tag == sh.tag {
                    return false;
                }
            }
            k = k.saturating_add(1usize);
        }
        // 申报侧的卡号不由 Claim 携带，但停留档必须在闸链内。
        if Stage::from_rank(sh.stage.rank()).is_none() {
            return false;
        }
        // 重试一致性：attempts==1 时不可能有 recovered 归因。
        if sh.attempts <= 1u8 && sh.washed {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    true
}

/// KA10：六族报错文案逐字。
fn message_ok() -> bool {
    // errno 面逐字。
    if errno_text(ENOENT) != "No such file or directory" {
        return false;
    }
    if errno_text(EACCES) != "Permission denied" {
        return false;
    }
    if errno_text(ENOSPC) != "No space left on device" {
        return false;
    }
    if errno_text(ETIMEDOUT) != "Connection timed out" {
        return false;
    }
    if errno_text(ENONE) != "" || errno_text(9999) != "Unknown error" {
        return false;
    }
    // 退出码面：127（找不到可执行体）与 128+sig（被信号杀）必须分得开。
    if exit_text(EX_NOTFOUND) != "127" || exit_text(EX_SKIP) != "4" {
        return false;
    }
    if exit_text(EX_SIG_BASE.saturating_add(SIG_SEGV)) != "139" {
        return false;
    }
    // TAP 头：计划数恒为 19，不是实跑条数——写成实跑数就是本卡要抓的第一颗雷。
    if msg_plan().as_str() != "TAP version 13\n1..19\n" {
        return false;
    }
    // 绿行 / skip 行 / xfail 行的文本差异必须存在。
    let green = match outcome_at(0usize) {
        Some(x) => x,
        None => return false,
    };
    let skipped = match outcome_at(7usize) {
        Some(x) => x,
        None => return false,
    };
    let flaked = match outcome_at(2usize) {
        Some(x) => x,
        None => return false,
    };
    let blocked = match outcome_at(8usize) {
        Some(x) => x,
        None => return false,
    };
    if msg_tap(1usize, WANT_TAGS[0usize], &green).as_str()
        != "ok 1 - selftests: unx: f11641-lcall-bad\n"
    {
        return false;
    }
    if !msg_tap(8usize, WANT_TAGS[7usize], &skipped)
        .as_str()
        .contains(" # SKIP ")
    {
        return false;
    }
    // 本卡最大的陷阱落地成断言：skip 行逐字就是 ok 行，
    // 只数 `ok` 前缀的聚合器会把 14 条里的 13 条算成绿，
    // 而真正的绿集只有 12 条（Green 10 + Flaked 2），差的正是那条诚实 skip。
    let skip_line = msg_tap(8usize, WANT_TAGS[7usize], &skipped);
    if !skip_line.as_str().starts_with(TAP_OK) {
        return false;
    }
    if skip_line.as_str().starts_with(TAP_NOT_OK) {
        return false;
    }
    let mut naive_ok = 0u32;
    let mut t = 0usize;
    while t < CORPUS {
        let o = match outcome_at(t) {
            Some(x) => x,
            None => return false,
        };
        let line = msg_tap(t.saturating_add(1usize), WANT_TAGS[t], &o);
        if line.as_str().starts_with(TAP_OK) {
            naive_ok = naive_ok.saturating_add(1u32);
        }
        t = t.saturating_add(1usize);
    }
    if naive_ok != 13u32 || run().green() != 12u32 || naive_ok == run().green() {
        return false;
    }
    if !msg_tap(3usize, WANT_TAGS[2usize], &flaked)
        .as_str()
        .contains(TAP_XFAIL_DIRECTIVE)
    {
        return false;
    }
    if blocked.ok {
        return false;
    }
    if !msg_tap(9usize, WANT_TAGS[8usize], &blocked)
        .as_str()
        .starts_with(TAP_NOT_OK)
    {
        return false;
    }
    // 经典计数行与新版汇总行逐字锚。两行必须同时出现且互不相等：
    // 经典行按性质分段（pass/fail/xfail/xpass/skip/error，六段守恒到 14），
    // 汇总行按结果行前缀数（ok 13 / not ok 1）。同一批判据、两个绿数、差 1，
    // 差的正是那条诚实 skip——谁把这两行合并成一行，谁就在这张单子上作弊。
    let totals = msg_totals(WANT_GREEN, WANT_FLAKED, WANT_SKIPPED, WANT_BLOCKED);
    if totals.as_str() != "#Totals: pass:10 fail:0 xfail:0 xpass:2 skip:1 error:1\n" {
        return false;
    }
    let sum = msg_tap_summary(WANT_GREEN, WANT_FLAKED, WANT_SKIPPED, WANT_BLOCKED);
    if sum.as_str() != "TAP test summary: ok:13 not ok:1\n" {
        return false;
    }
    if totals.as_str() == sum.as_str() {
        return false;
    }
    if run().green() != WANT_GREEN.saturating_add(WANT_FLAKED) {
        return false;
    }
    // RUN 行与错误行。
    if !msg_run(11641u32).as_str().starts_with(KSFT_RUN) {
        return false;
    }
    let err = msg_error("f11641-lcall-bad", EACCES, EX_FAIL);
    if !err.as_str().starts_with(KSFT_ERR) || !err.as_str().contains("(Permission denied)") {
        return false;
    }
    if !err.as_str().contains(" exit 1") {
        return false;
    }
    // ktest.pl 收尾：无错误时不得出现 Initial Error 行。
    if msg_ktest_tail(None).as_str().contains(KTEST_INITIAL_ERR) {
        return false;
    }
    if !msg_ktest_tail(None).as_str().contains(KTEST_REBOOT) {
        return false;
    }
    if !msg_ktest_tail(Some("initrd missing"))
        .as_str()
        .contains(KTEST_INITIAL_ERR)
    {
        return false;
    }
    if KTEST_SRC_FILE != "unx11660.c" {
        return false;
    }
    true
}

/// KA11：签发面、通过率与闸门序列。
fn tally_ok() -> bool {
    let r = run();
    if r.rate() != RATE_FULL {
        return false;
    }
    if r.render().as_str() != Report::RENDER_OK {
        return false;
    }
    if r.gate().is_err() {
        return false;
    }
    if r.green() != WANT_GREEN.saturating_add(WANT_FLAKED) {
        return false;
    }
    // 抖动必须被单独问出来，而不是并进绿。
    if r.flake_free() {
        return false;
    }
    // 逐维注入：注入活槽任一维都必须掉到 929 且闸门变红。
    let mut i = 0usize;
    while i < REJECT {
        let f = match REJECT_FAULTS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let d = match run_with(Some((0usize, f))) {
            Ok(x) => x,
            Err(_) => return false,
        };
        if d.rate() != RED_RATE || d.gate().is_ok() {
            return false;
        }
        if d.render().as_str() == r.render().as_str() {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 越界注入槽必须直接判死，而不是当成"没注入"。
    if run_with(Some((CORPUS, Fault::Estate))).is_ok() {
        return false;
    }
    if run_with(Some((999usize, Fault::Ok))).is_ok() {
        return false;
    }
    // 已知取舍：往已经判死的槽再注一维，固定闸序会先撞上原缺陷维，
    // 结论不变、通过率不变。这不是漏检（结论本来就错着），但必须写下来，
    // 免得后来人以为死槽可以随便加注。
    if let Ok(d) = run_with(Some((ACCEPT, Fault::Default))) {
        if d.rate() != RATE_FULL {
            return false;
        }
    } else {
        return false;
    }
    true
}

/// KA11 尾：明细、码名与诊断行的一致性。
fn detail_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        let f = expected_fault(i);
        let d = detail(i, f);
        if !d.as_str().contains(f.slug()) {
            return false;
        }
        if let Some(sh) = shape_at(i) {
            if !d.as_str().contains(sh.tag) {
                return false;
            }
        } else {
            return false;
        }
        let g = diagnostic(i, f);
        if !g.as_str().starts_with(KSFT_ERR) {
            return false;
        }
        if !g.as_str().contains(code_name(f.code())) {
            return false;
        }
        // Ok 槽的诊断行写的是 none，不给出可被误读成缺陷码的字符串。
        if f == Fault::Ok && !g.as_str().contains("[none]") {
            return false;
        }
        if f != Fault::Ok && g.as_str().contains("[none]") {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    // 越界槽的明细与诊断都必须落在 none 分支而不是回绕。
    if !detail(CORPUS, Fault::Estate).as_str().contains("none") {
        return false;
    }
    if !diagnostic(CORPUS, Fault::Estate).as_str().contains("[estate]") {
        return false;
    }
    true
}

/// KA12：账本——逐格核账、封存、冻结、重放。
fn ledger_ok() -> bool {
    let mut l = build_ledger();
    if l.len() != SAMPLES {
        return false;
    }
    if l.is_empty() {
        return false;
    }
    if l.rejects() != 0u32 {
        return false;
    }
    if l.audit().is_err() {
        return false;
    }
    if l.ok_count() != ACCEPT as u32 || l.err_count() != REJECT as u32 {
        return false;
    }
    // 操作号分段：前 7 格统计、第 8 格红档率、后 6 格缺陷。
    let mut i = 0usize;
    while i < SAMPLES {
        let s = match l.sample(i) {
            Some(x) => x,
            None => return false,
        };
        if s.slot != i {
            return false;
        }
        if !s.op_in_range() {
            return false;
        }
        if i < ACCEPT {
            if i + 1usize == ACCEPT {
                if s.op_name() != "red-rate" {
                    return false;
                }
            } else if s.op_name() != "stat" {
                return false;
            }
        } else if s.op_name() != "defect" {
            return false;
        }
        i = i.saturating_add(1usize);
    }
    if l.sample(SAMPLES).is_some() {
        return false;
    }
    let root = match l.seal() {
        Ok(r) => r,
        Err(_) => return false,
    };
    if l.root() != Some(root) {
        return false;
    }
    if l.replay().is_err() {
        return false;
    }
    // 冻结后不得再登记，且未封存时冻结要顺手封存。
    let mut m = build_ledger();
    if m.freeze().is_err() {
        return false;
    }
    if !m.is_frozen() {
        return false;
    }
    if m.register(Sample::defect(SAMPLES, 0usize, KA_ESTATE)).is_ok() {
        return false;
    }
    if m.rejects() != 1u32 {
        return false;
    }
    // 被拒过的账不许再冻结——否则"拒"就从指纹里消失了。
    let mut n = build_ledger();
    if n.register(Sample::blank()).is_ok() {
        return false;
    }
    if n.rejects() != 1u32 || n.len() != SAMPLES {
        return false;
    }
    if n.freeze().is_ok() {
        return false;
    }
    // 插队必须既被拒又不改变长度。
    let mut p = Ledger::new();
    if p.len() != 0usize || !p.is_empty() {
        return false;
    }
    let first = match VALS.get(0usize) {
        Some(x) => *x,
        None => return false,
    };
    if p.register(Sample::stat(1usize, 0usize, first)).is_ok() {
        return false;
    }
    if !p.is_empty() || p.rejects() != 1u32 {
        return false;
    }
    // 未封存就偷改是不允许的：偷改只对已封存的账才有"篡改"意义。
    let mut q = build_ledger();
    if q.poke(0usize, 1u32).is_ok() {
        return false;
    }
    true
}

/// KA12 尾：逐闸推进——回退即死、越闸即死、收口后再动即死、被拒进指纹。
fn gate_ok() -> bool {
    let mut g = Gate::new();
    if g.at().is_some() || g.closed() || g.moves() != 0u32 {
        return false;
    }
    // 首步不许跳过清单闸。
    if g.advance(Stage::Exec).is_ok() {
        return false;
    }
    if g.rejects() != 1u32 {
        return false;
    }
    if g.advance(Stage::Manifest).is_err() {
        return false;
    }
    // 同级重复也算回退。
    if g.advance(Stage::Manifest).is_ok() {
        return false;
    }
    // 越级不许：从清单直接跳执行。
    if g.advance(Stage::Exec).is_ok() {
        return false;
    }
    // 收口只许在封印闸之后。
    if g.close().is_ok() {
        return false;
    }
    let mut h = Gate::new();
    if h.walk().is_err() {
        return false;
    }
    if !h.full_walk() || !h.closed() {
        return false;
    }
    if h.moves() as usize != GATE_COUNT {
        return false;
    }
    // 收口后再推进或再收口，一律 KA_ESTATE。
    if h.advance(Stage::Seal).is_ok() {
        return false;
    }
    if h.close().is_ok() {
        return false;
    }
    if h.rejects() != 2u32 {
        return false;
    }
    // 收口后的拒绝必须进指纹：先比"干净走完"与"走完后又被拒两次"，
    // 两者根不同，才说明被动过；再比两次干净走完，两者根相同，才说明可重放。
    let dirty = h.fold(FNV_OFF);
    let mut h2 = Gate::new();
    if h2.walk().is_err() {
        return false;
    }
    let mut h3 = Gate::new();
    if h3.walk().is_err() {
        return false;
    }
    let clean = h2.fold(FNV_OFF);
    if h3.fold(FNV_OFF) != clean {
        return false;
    }
    if dirty == clean {
        return false;
    }
    // 同一条走完的闸，多一次被拒就要在指纹上留痕：拒绝次数是账，不是脚注。
    if h2.advance(Stage::Seal).is_ok() {
        return false;
    }
    h2.fold(FNV_OFF) != clean
}

/// KA13：四路注入全部被抓。
fn inject_ok() -> bool {
    let v = inject_four();
    let mut i = 0usize;
    while i < 4usize {
        match v.get(i) {
            Some(x) => {
                if !*x {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1usize);
    }
    true
}

/// 本卡判据集入口——一次命令全跑的那"一次命令"。
///
/// 十三项的顺序就是本模块的闸次序：先立身份（段基址/码表），再立结构
/// （闸链/载体/skip），再立清单（19 行逐行），然后才走推演、判定、矩阵，
/// 最后落到签发面、账本与四路注入。顺序不可调：清单没立住就去核签发面，
/// 核的是自己编的分母；账本没核就去跑注入，注入的是没封口的账。
pub fn run_unx11660_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11660_ktest_b23_agg");
    s.add(
        "KA01-名称与段基址",
        name_ok(),
        "SEG_BASE=0x6cf50 起 13 条码连续互斥且码名全覆盖",
    );
    s.add(
        "KA02-闸链与归因档",
        enums_ok(),
        "八闸 +1 递增闭合、每闸专属码互异、十一档绿面与 skip 面互斥",
    );
    s.add(
        "KA03-载体与skip成因",
        harness_ok() && skip_ok(),
        "七载体标志自洽（双机/musl 必真跑）、七成因与缺失位及归因档三方一致",
    );
    s.add(
        "KA04-清单19行",
        manifest_ok(),
        "19 行逐行自洽、卡号有序、短名唯一、前置成链、拓扑可排、断言合计 116",
    );
    s.add(
        "KA05-八闸推演逐槽",
        define_ok(),
        "14 槽事实推演六维全对齐期望表且可重复，默认位只在走满全程时为真",
    );
    s.add(
        "KA06-六维判定逐槽",
        judge_ok(),
        "逐槽结论等于期望结论，诚实申报必过，缺陷维下标与缺陷码互推",
    );
    s.add(
        "KA07-四档矩阵",
        matrix_ok(),
        "四档全覆盖且逐档满对齐，绿档无谎，红档无走满全程的条",
    );
    s.add(
        "KA08-语料活死配平",
        corpus_ok() && reds_ok(),
        "8 活 6 死、档位 10/2/1/1、六缺陷维两两不同、13/14 千分位恰为 929",
    );
    s.add(
        "KA09-身份交叉",
        slots_ok(),
        "槽短名与清单逐字互查、卡号不重复、载体一致、重试与洗绿互斥",
    );
    s.add(
        "KA10-报错文案逐字",
        message_ok(),
        "六族文案逐字锚；skip 行前缀就是 ok，两族计数行绿数差 1",
    );
    s.add(
        "KA11-签发面与诊断",
        tally_ok() && detail_ok(),
        "无注入通过率 1000 且签发面逐字等于锚；逐维注入必掉 929 并变红",
    );
    s.add(
        "KA12-账本与逐闸",
        ledger_ok() && gate_ok(),
        "14 格核账封存重放一致，插队/冻结后登记/未封存偷改一律被拒；闸回退与越闸即死、被拒必进指纹",
    );
    s.add(
        "KA13-冻结重放四注入",
        inject_ok(),
        "表路/判路/渲染路/账本路四路注入全部被对应防线抓住",
    );
    s
}

// ---------------------------------------------------------------------------
// 单元测试：每个测试钉一条本卡会踩的雷
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一条"干净跑过"的事实：把清单行的身份抄进事实面，其余全给绿值。
    fn green_shape(card: u16) -> Shape {
        let mut sh = Shape::zero();
        if let Some(sp) = spec_index_of(card).and_then(spec_at) {
            sh.card = sp.card;
            sh.tag = sp.tag;
            sh.harness = sp.harness;
            sh.asserts = sp.asserts;
            sh.pass_asserts = sp.asserts;
            sh.dep = sp.dep;
            sh.need_cfg = sp.need_cfg;
            sh.have_cfg = CFG_ALL;
            sh.default_suite = sp.default_suite;
        }
        sh.stage = Stage::Seal;
        sh.ran = true;
        // "干净跑过"含前置已达成：不设它，带前置的卡会先在汇总闸被判 DepBlocked。
        sh.dep_done = true;
        sh.first_ok = true;
        sh.attempts = 1u8;
        sh.exit_code = EX_OK;
        sh.errno_bit = ENONE;
        sh.ms = sh.harness.ms_band();
        sh.log_bytes = 512u32;
        sh
    }

    #[test]
    fn seg_base_and_code_table() {
        assert_eq!(SEG_BASE, 0x6cf50u32);
        assert_eq!(KA_ESTATE, SEG_BASE + 0x01u32);
        assert_eq!(KA_ESLOT, SEG_BASE + 0x0du32);
        let mut prev = 0u32;
        let codes = [
            KA_ESTATE, KA_ESTAGE, KA_ECODE, KA_EVALUE, KA_ENOTE, KA_EDEFAULT, KA_ECOUNT, KA_EUNIQ,
            KA_ECYCLE, KA_ESKIP, KA_EFLAKE, KA_EBLOCK, KA_ESLOT,
        ];
        for c in codes.iter() {
            assert!(*c > SEG_BASE && *c < SEG_BASE + 0x0eu32);
            assert_ne!(*c, prev);
            prev = *c;
            assert_ne!(code_name(*c), "unknown");
            assert_ne!(code_name(*c), "none");
        }
        assert_eq!(code_name(0u32), "none");
    }

    #[test]
    fn contract_constants() {
        assert_eq!(CORPUS, 14usize);
        assert_eq!(SAMPLES, 14usize);
        assert_eq!(ACCEPT, 8usize);
        assert_eq!(REJECT, 6usize);
        assert_eq!(ACCEPT + REJECT, CORPUS);
        assert_eq!(TIER_COUNT, 4usize);
        assert_eq!(BENCH, 19usize);
        assert_eq!(GATE_COUNT, 8usize);
        assert_eq!(NOTE_COUNT, 11usize);
        assert_eq!(HARNESS_COUNT, 7usize);
        assert_eq!(STAT_CELLS + 1usize + DEFECT_CELLS, SAMPLES);
        assert_eq!(DEP_ROWS, 18usize);
        assert_eq!(RATE_FULL, 1000u32);
        // 929 必须是"14 格里错一格"的千分位，不是拍来的数。
        assert_eq!(RED_RATE, (13u32 * 1000u32 + 7u32) / 14u32);
    }

    #[test]
    fn stage_chain_is_adjacent_and_1_based() {
        for (i, s) in STAGE_CHAIN.iter().enumerate() {
            assert_eq!(s.rank() as usize, i + 1usize);
            assert_eq!(Stage::from_rank(s.rank()), Some(*s));
        }
        assert_eq!(Stage::from_rank(0u8), None);
        assert_eq!(Stage::from_rank(9u8), None);
        assert_eq!(STAGE_CHAIN[0].next(), Some(Stage::Identity));
        assert!(STAGE_CHAIN[GATE_COUNT - 1].is_end());
        assert_eq!(STAGE_CHAIN[GATE_COUNT - 1].next(), None);
    }

    #[test]
    fn stage_fault_codes_are_pairwise_distinct() {
        for i in 0..GATE_COUNT {
            for j in 0..GATE_COUNT {
                if i != j {
                    assert_ne!(STAGE_CHAIN[i].fault_code(), STAGE_CHAIN[j].fault_code());
                }
            }
        }
        assert_eq!(Stage::Manifest.fault_code(), KA_ECOUNT);
        assert_eq!(Stage::Seal.fault_code(), KA_EBLOCK);
    }

    #[test]
    fn past_exec_boundary_is_after_exec() {
        assert!(!Stage::Ready.past_exec());
        assert!(!Stage::Exec.past_exec());
        assert!(Stage::Retry.past_exec());
        assert!(Stage::Seal.past_exec());
    }

    #[test]
    fn note_faces_are_disjoint_and_only_none_is_nameless() {
        let mut green = 0u32;
        let mut skip = 0u32;
        for k in 0..NOTE_COUNT as u8 {
            let n = Note::from_rank(k).expect("note rank");
            assert_eq!(n.rank(), k);
            assert!(!(n.green_face() && n.skip_face()));
            if n.green_face() {
                green += 1;
            }
            if n.skip_face() {
                skip += 1;
            }
            if k == 0u8 {
                assert!(n.slug().is_empty());
            } else {
                assert!(!n.slug().is_empty());
            }
        }
        assert_eq!(Note::from_rank(NOTE_COUNT as u8), None);
        assert_eq!(green, 2u32);
        assert_eq!(skip, 4u32);
    }

    #[test]
    fn harness_carrier_flags() {
        // 只有静态读节不真跑；其余六种都要目标机把进程跑起来。
        assert!(!Harness::StaticObj.needs_run());
        assert_eq!(Harness::from_rank(0u8), None);
        assert_eq!(Harness::from_rank(HARNESS_COUNT as u8), Some(Harness::StaticObj));
        assert_eq!(Harness::from_rank(8u8), None);
        let mut ran = 0u32;
        for k in 1..=HARNESS_COUNT as u8 {
            let h = Harness::from_rank(k).expect("harness rank");
            assert_eq!(h.rank(), k);
            assert!(!h.slug().is_empty());
            assert_ne!(h.ms_band(), 0u32);
            if h.needs_run() {
                ran += 1;
            }
            if h.needs_two_sys() || h.needs_musl() {
                assert!(h.needs_run(), "跨线载体不可能静态读节");
            }
        }
        assert_eq!(ran, 6u32);
    }

    #[test]
    fn skip_cause_maps_to_note_and_bit() {
        assert!(!Skip::None.is_skip());
        assert_eq!(Skip::None.errno(), ENONE);
        assert_eq!(Note::of_skip(Skip::None), Note::None);
        for k in 1..7u8 {
            let s = Skip::from_rank(k).expect("skip rank");
            assert!(s.is_skip());
            assert_ne!(s.errno(), ENONE);
            assert!(Note::of_skip(s).skip_face());
        }
        assert_eq!(Skip::from_rank(7u8), None);
        assert_eq!(Skip::NoRoot.consumed_bit(), CFG_ROOT);
        assert_eq!(Skip::NoDevice.consumed_bit(), CFG_LOOP);
        assert_eq!(Skip::NoPeer.consumed_bit(), CFG_TWOSYS);
        assert_eq!(Skip::NoMusl.consumed_bit(), CFG_MUSL);
        // 集成因与载体缺席不吃单一位。
        assert_eq!(Skip::NoConfig.consumed_bit(), CFG_NONE);
        assert_eq!(Skip::NoHarness.consumed_bit(), CFG_NONE);
    }

    #[test]
    fn derive_skip_prefers_the_hardest_absence() {
        assert_eq!(derive_skip(CFG_MUSL), Skip::NoMusl);
        assert_eq!(derive_skip(CFG_TWOSYS), Skip::NoPeer);
        assert_eq!(derive_skip(CFG_ROOT), Skip::NoRoot);
        assert_eq!(derive_skip(CFG_LOOP), Skip::NoDevice);
        assert_eq!(derive_skip(CFG_TMPFS), Skip::NoDevice);
        assert_eq!(derive_skip(CFG_MODULES), Skip::NoConfig);
        // 一位不缺就不是 skip：把 NoHarness 放在这里是满配置绿行集体误判的源头。
        assert_eq!(derive_skip(CFG_NONE), Skip::None);
        // CFG_* 是"缺席掩码"，不是"存在掩码"：全缺时最硬的那位说了算，
        // 所以 CFG_ALL 读作 musl 侧缺席——把掩码方向搞反是这条闸最容易翻的车。
        assert_eq!(derive_skip(CFG_ALL), Skip::NoMusl);
        assert_eq!(derive_skip(CFG_NO_MUSL), Skip::NoPeer);
        // musl 比 root 更硬：同时缺两位时报 musl 侧缺席。
        assert_eq!(derive_skip(CFG_MUSL | CFG_ROOT), Skip::NoMusl);
    }

    #[test]
    fn tag_accepts_hyphen_but_not_tap_breakers() {
        let mut sh = green_shape(11641u16);
        // 连字符是 kselftest 短名的常规写法，拒了它等于 19 条全判死。
        sh.tag = "f11641-lcall-bad";
        assert!(sh.tag_ok());
        sh.tag = "has space";
        assert!(!sh.tag_ok());
        sh.tag = "comment#here";
        assert!(!sh.tag_ok());
        sh.tag = "line\nbreak";
        assert!(!sh.tag_ok());
        sh.tag = "";
        assert!(!sh.tag_ok());
    }

    #[test]
    fn bench_manifest_counts() {
        assert!(manifest_count_ok());
        assert!(manifest_sorted_ok());
        assert!(manifest_tags_unique_ok());
        assert!(manifest_deps_ok());
        assert!(manifest_rows_ok());
        assert_eq!(bench_assert_total(), WANT_ASSERTS);
        assert_eq!(bench_assert_total(), 116u32);
        assert_eq!(bench_dep_rows(), DEP_ROWS as u32);
        assert_eq!(bench_dual_anchors(), DUAL_ANCHORS as u32);
        assert_eq!(bench_default_suite(), 17u32);
        assert_eq!(bench_uncovered(), (BENCH - CORPUS) as u32);
        assert!(spec_at(BENCH).is_none());
        assert_eq!(spec_at(0usize).map(|s| s.card), Some(CARD_MIN));
        assert_eq!(spec_at(BENCH - 1usize).map(|s| s.card), Some(CARD_MAX));
    }

    #[test]
    fn spec_row_rejects_out_of_batch_identity() {
        let fake = Spec {
            card: 0u16,
            tag: "",
            title: "",
            harness: Harness::KselftestC,
            dep: 0u16,
            need_cfg: CFG_NONE,
            asserts: 0u16,
            default_suite: false,
        };
        assert!(!spec_row_ok(&fake));
        // 载体序号 1 基，7 号 static-objdump 合法，0 与 8 非法。
        let mut ok = green_spec();
        assert!(spec_row_ok(&ok));
        ok.harness = match Harness::from_rank(HARNESS_COUNT as u8) {
            Some(x) => x,
            None => panic!("rank 7 must exist"),
        };
        assert!(spec_row_ok(&ok));
    }

    /// 取清单首行做原型，便于只改一个字段验证 spec_row_ok。
    fn green_spec() -> Spec {
        match spec_at(0usize) {
            Some(s) => Spec {
                card: s.card,
                tag: s.tag,
                title: s.title,
                harness: s.harness,
                dep: s.dep,
                need_cfg: s.need_cfg,
                asserts: s.asserts,
                default_suite: s.default_suite,
            },
            None => panic!("row 0 must exist"),
        }
    }

    #[test]
    fn topo_order_respects_deps() {
        let order = topo_order().expect("topo must resolve");
        let mut pos = [0usize; BENCH];
        for (p, idx) in order.iter().enumerate() {
            assert!(*idx < BENCH);
            pos[*idx] = p;
        }
        for i in 0..BENCH {
            let sp = match spec_at(i) {
                Some(x) => x,
                None => panic!("row missing"),
            };
            if let Some(j) = spec_index_of(sp.dep) {
                // 前置条必须排在依赖它的那条之前；F11658 -> F11657 -> … -> F11641。
                assert!(pos[j] < pos[i], "前置必须先于本条执行");
            }
        }
    }

    #[test]
    fn corpus_shape_and_claim_are_aligned() {
        assert_eq!(live_count(), ACCEPT);
        assert_eq!(dead_count(), REJECT);
        assert_eq!(tier_counts(), [WANT_GREEN, WANT_FLAKED, WANT_SKIPPED, WANT_BLOCKED]);
        assert_eq!(green_count(), WANT_GREEN + WANT_FLAKED);
        assert_eq!(red_count(), WANT_SKIPPED + WANT_BLOCKED);
        for i in 0..CORPUS {
            let sh = shape_at(i).expect("shape");
            let o = outcome_at(i).expect("outcome");
            assert!(same_outcome(&define(&sh), &o), "推演必须可重复");
            assert_eq!(o.ok, WANT_OK[i]);
            assert_eq!(o.stage, WANT_STAGE[i]);
            assert_eq!(o.code, WANT_CODE[i]);
            assert_eq!(o.value, WANT_VALUE[i]);
            assert_eq!(o.note, WANT_NOTE[i]);
            assert_eq!(o.default, WANT_DEFAULT[i]);
            assert_eq!(o.tier(), WANT_TIER[i]);
        }
    }

    #[test]
    fn honest_claim_always_passes() {
        for i in 0..CORPUS {
            let sh = match shape_at(i) {
                Some(x) => x,
                None => panic!("shape missing"),
            };
            let o = define(&sh);
            let honest = Claim::from_outcome(&o);
            assert_eq!(judge_against(&honest, &o), Fault::Ok);
        }
    }

    #[test]
    fn each_break_dim_yields_its_own_fault() {
        let sh = match shape_at(0usize) {
            Some(x) => x,
            None => panic!(),
        };
        let o = define(&sh);
        let base = Claim::from_outcome(&o);
        let dims = [
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ];
        for (i, f) in dims.iter().enumerate() {
            let broken = base.break_dim(*f);
            assert_eq!(judge_against(&broken, &o), *f);
            assert_eq!(REJECT_FAULTS[i], *f);
            assert_eq!(REJECT_CODES[i], f.code());
        }
        assert_eq!(CLAIM_BROKEN, REJECT_FAULTS);
    }

    #[test]
    fn judge_orders_estate_first() {
        // 成败互报优先：一次同时坏两维，先报 Estate。
        let sh = green_shape(11641u16);
        let o = define(&sh);
        let mut c = Claim::from_outcome(&o);
        c.success = !c.success;
        c.value = c.value + 1u32;
        assert_eq!(judge_against(&c, &o), Fault::Estate);
        let case = Case { shape: sh, claim: c };
        assert!(same_outcome(&case.actual(), &o));
        assert_eq!(judge(&case), Fault::Estate);
    }

    #[test]
    fn expected_fault_table_matches_slots() {
        for i in 0..ACCEPT {
            assert_eq!(expected_fault(i), Fault::Ok);
        }
        for i in 0..REJECT {
            let slot = ACCEPT + i;
            assert_eq!(expected_fault(slot), REJECT_FAULTS[i]);
            assert_eq!(WANT_NDX[slot], i);
            assert_eq!(WANT_FAULT_CODE[slot], REJECT_CODES[i]);
        }
    }

    #[test]
    fn dead_slots_are_broken_in_pairwise_distinct_dims() {
        let mut seen = [Fault::Ok; REJECT];
        for i in 0..REJECT {
            let slot = ACCEPT + i;
            let sh = match shape_at(slot) {
                Some(x) => x,
                None => panic!(),
            };
            let claim = match claim_at(slot) {
                Some(x) => x,
                None => panic!(),
            };
            let f = judge_against(&claim, &define(&sh));
            assert_ne!(f, Fault::Ok, "死槽必须判得出缺陷");
            seen[i] = f;
            for j in 0..i {
                assert_ne!(seen[j], f, "六维不得重复");
            }
        }
    }

    #[test]
    fn skip_slot_is_honest_and_stays_red() {
        let o = match outcome_at(7usize) {
            Some(x) => x,
            None => panic!(),
        };
        assert!(!o.ok);
        assert_eq!(o.stage, Stage::Ready);
        assert_eq!(o.code, KA_ESKIP);
        assert_eq!(o.note, Note::ConfigAbsent);
        assert_eq!(o.tier(), Tier::Skipped);
        // skip 行在文本面上就是 ok 行——这正是聚合器最容易上当的地方。
        let line = msg_tap(7usize, WANT_TAGS[7usize], &o);
        assert!(line.as_str().starts_with(TAP_OK));
        assert!(line.as_str().contains(TAP_SKIP_DIRECTIVE));
    }

    #[test]
    fn matrix_counts_cover_all_slots() {
        let r = run();
        assert!(r.matrix.covered());
        assert_eq!(r.matrix.total(), CORPUS as u32);
        assert_eq!(r.matrix.aligned(), WANT_MATCHED);
        assert_eq!(r.matrix.perfect_rows(), TIER_COUNT as u32);
        assert_eq!(r.matrix.in_green(), WANT_GREEN + WANT_FLAKED);
        // 矩阵只累计被语料引用的清单行；Report.asserts 记的是整批 19 行的清单数。
        // 两者的差必须正好等于未覆盖那几行的断言数，否则"覆盖率"是编出来的。
        let mut covered_spec = 0u32;
        let mut uncovered_spec = 0u32;
        let mut i = 0usize;
        while i < BENCH {
            let sp = match spec_at(i) {
                Some(x) => x,
                None => break,
            };
            let mut hit = false;
            let mut j = 0usize;
            while j < CORPUS {
                if let Some(sh) = shape_at(j) {
                    if sh.card == sp.card {
                        hit = true;
                        break;
                    }
                }
                j = j.saturating_add(1usize);
            }
            if hit {
                covered_spec = covered_spec.saturating_add(sp.asserts as u32);
            } else {
                uncovered_spec = uncovered_spec.saturating_add(sp.asserts as u32);
            }
            i = i.saturating_add(1usize);
        }
        assert_eq!(r.matrix.spec_total(), covered_spec);
        assert_eq!(
            covered_spec.saturating_add(uncovered_spec),
            bench_assert_total()
        );
        assert_eq!(r.asserts, WANT_ASSERTS);
        assert!(!r.matrix.green_has_lie());
        assert_eq!(r.matrix.row(Tier::Green).map(|x| x.total), Some(WANT_GREEN));
        assert_eq!(r.matrix.row(Tier::Blocked).map(|x| x.total), Some(WANT_BLOCKED));
    }

    #[test]
    fn matrix_row_perfection_needs_every_slot_aligned() {
        let mut m = Matrix::zero();
        assert_eq!(m.total(), 0u32);
        assert_eq!(m.perfect_rows(), 0u32);
        assert!(!m.covered());
        assert_eq!(m.bump(Tier::Green, true, 1u32, true, 1u32), Ok(()));
        assert_eq!(m.bump(Tier::Green, false, 0u32, false, 0u32), Ok(()));
        assert_eq!(m.row(Tier::Green).map(|x| x.total), Some(2u32));
        assert_eq!(m.row(Tier::Green).map(|x| x.aligned), Some(1u32));
        // 混进一条没对齐的，整档就不算满对齐档——"绿档里有条靠 skip 蒙过去"必须看得见。
        assert_eq!(m.bump(Tier::Green, true, 0u32, false, 0u32), Ok(()));
        assert_eq!(m.row(Tier::Green).map(|x| x.total), Some(3u32));
        assert_eq!(m.perfect_rows(), 0u32);
        let mut m2 = Matrix::zero();
        assert_eq!(m2.bump(Tier::Green, true, 1u32, true, 1u32), Ok(()));
        assert_eq!(m2.bump(Tier::Green, true, 0u32, false, 0u32), Ok(()));
        assert_eq!(m2.row(Tier::Green).map(|x| x.aligned), Some(2u32));
        assert_eq!(m2.perfect_rows(), 1u32);
        // 空档的 aligned==total==0 永远不算满对齐，否则四档能靠零条凑齐。
        assert_eq!(m2.row(Tier::Blocked).map(|x| x.perfect()), Some(false));
    }

    #[test]
    fn report_gate_and_anchor_render() {
        let r = run();
        assert_eq!(r.rate(), RATE_FULL);
        assert_eq!(r.matched, WANT_MATCHED);
        assert_eq!(r.bench, BENCH as u32);
        assert_eq!(r.asserts, WANT_ASSERTS);
        assert!(r.gate().is_ok());
        assert_eq!(r.render(), Report::RENDER_OK);
        assert!(!r.flake_free());
        assert_eq!(r.sealed(), 12u32);
        assert_eq!(Report::invalid().rate(), 0u32);
        assert_eq!(Report::invalid().gate(), Err(KA_ESTATE));
    }

    #[test]
    fn rate_formula_rounds_half_up() {
        let mut r = run();
        r.matched = 1u32;
        r.total = 14u32;
        assert_eq!(r.rate(), 71u32);
        r.matched = 13u32;
        assert_eq!(r.rate(), RED_RATE);
        r.matched = 14u32;
        assert_eq!(r.rate(), RATE_FULL);
    }

    #[test]
    fn single_dim_injection_on_live_slot_drops_to_red_rate() {
        for i in 0..ACCEPT {
            let r = match run_with(Some((i, Fault::Value))) {
                Ok(x) => x,
                Err(_) => panic!("slot in range must resolve"),
            };
            assert_eq!(r.rate(), RED_RATE);
            assert_eq!(r.gate(), Err(KA_ECOUNT));
            assert_ne!(r.render(), Report::RENDER_OK);
        }
        // 越界槽位不进结局表：聚合器不许把"没这条"读成"这条绿"。
        let oob = match run_with(Some((CORPUS, Fault::Value))) {
            Ok(_) => 0u32,
            Err(c) => c,
        };
        assert_eq!(oob, KA_ESLOT);
    }

    #[test]
    fn documented_tradeoff_second_injection_on_dead_slot_is_masked() {
        // 固定闸序只报第一个坏维：往已判死的槽再注一维，结论不变。
        // 这不是漏检，但必须钉住，免得后来人误以为死槽可以随便加注。
        let r = run_with(Some((ACCEPT, Fault::Default))).expect("in range");
        assert_eq!(r.rate(), RATE_FULL);
        assert!(r.gate().is_ok());
        // 反过来，往"缺陷维排在 Value 之后"的死槽注 Value，就必须被抓。
        let s = run_with(Some((CORPUS - 1usize, Fault::Value))).expect("in range");
        assert_eq!(s.rate(), RED_RATE);
    }

    #[test]
    fn tap_message_families_disagree_by_exactly_one() {
        let r = run();
        let totals = msg_totals(r.tier_green(), r.tier_flaked(), r.tier_skipped(), r.tier_blocked());
        assert_eq!(totals.as_str(), "#Totals: pass:10 fail:0 xfail:0 xpass:2 skip:1 error:1\n");
        let summary =
            msg_tap_summary(r.tier_green(), r.tier_flaked(), r.tier_skipped(), r.tier_blocked());
        assert_eq!(summary.as_str(), "TAP test summary: ok:13 not ok:1\n");
        // 前缀计数族给出的 13 比诚实绿集 12 多一条，多的正是那条诚实 skip。
        assert_eq!(r.green(), 12u32);
        assert_eq!(r.tier_green() + r.tier_flaked() + r.tier_skipped(), 13u32);
    }

    #[test]
    fn plan_line_pins_bench_not_run_count() {
        assert_eq!(msg_plan().as_str(), "TAP version 13\n1..19\n");
        assert_eq!(TAP_PLAN, "1..");
        assert_eq!(BENCH, 19usize);
    }

    #[test]
    fn errno_and_exit_text() {
        assert_eq!(errno_text(ENOENT), "No such file or directory");
        assert_eq!(errno_text(EACCES), "Permission denied");
        assert_eq!(errno_text(ENOSPC), "No space left on device");
        assert_eq!(errno_text(ETIMEDOUT), "Connection timed out");
        assert_eq!(errno_text(ENONE), "");
        assert_eq!(errno_text(9999), "Unknown error");
        assert_eq!(exit_text(EX_NOTFOUND), "127");
        assert_eq!(exit_text(EX_SIG_BASE + SIG_SEGV), "139");
        assert_eq!(exit_text(EX_SKIP), "4");
        let e = msg_error("f11641-lcall-bad", ENOENT, EX_NOTFOUND);
        assert!(e.as_str().starts_with(KSFT_ERR));
        assert!(e.as_str().contains("127"));
    }

    #[test]
    fn ktest_tail_families() {
        let t = msg_ktest_tail(Some("boom"));
        assert!(t.as_str().starts_with(KTEST_LOG));
        assert!(t.as_str().contains(KTEST_INITIAL_ERR));
        assert!(t.as_str().contains("boom"));
        assert!(t.as_str().contains(KTEST_REBOOT));
        assert!(t.as_str().ends_with(KTEST_DONE));
        let clean = msg_ktest_tail(None);
        assert!(!clean.as_str().contains(KTEST_INITIAL_ERR));
    }

    #[test]
    fn detail_and_diagnostic_carry_slot_identity() {
        for i in 0..CORPUS {
            let f = expected_fault(i);
            let d = detail(i, f);
            let sh = match shape_at(i) {
                Some(x) => x,
                None => panic!(),
            };
            assert!(d.as_str().contains(sh.tag));
            assert!(d.as_str().contains(f.slug()));
            let g = diagnostic(i, f);
            assert!(g.as_str().starts_with(KSFT_ERR));
            assert!(g.as_str().contains(code_name(f.code())));
            if f == Fault::Ok {
                assert!(g.as_str().contains("[none]"));
            } else {
                assert!(!g.as_str().contains("[none]"));
            }
        }
        assert!(detail(CORPUS, Fault::Estate).as_str().contains("none"));
    }

    #[test]
    fn fold_helpers_are_position_sensitive() {
        let a = fold_str(FNV_OFF, "ab");
        let b = fold_str(FNV_OFF, "ba");
        assert_ne!(a, b);
        let c = fold_word(FNV_OFF, 1u64);
        assert_ne!(c, fold_word(FNV_OFF, 2u64));
        assert_eq!(fold_byte(FNV_OFF, 0u8), fold_byte(FNV_OFF, 0u8));
        assert_ne!(fold_byte(FNV_OFF, 0u8), fold_byte(FNV_OFF, 1u8));
    }

    #[test]
    fn number_text_helpers() {
        assert_eq!(dec_text(0u32).as_str(), "0");
        assert_eq!(dec_text(929u32).as_str(), "929");
        assert_eq!(dec_text(116u32).as_str(), "116");
        assert_eq!(hex_text(0x6cf50u32, 5u32 as usize).as_str(), "6cf50");
        assert_eq!(hex_text(0u32, 5usize).as_str(), "00000");
        assert_eq!(u32_text(19u32).as_str(), "19");
        assert_eq!(i32_text(-1i32).as_str(), "-1");
        assert_eq!(i32_text(0i32).as_str(), "0");
        // 本卡的 bool_text 直接喂 TAP 裁决位，取的是 ktest 面上的 "ok"/"not ok"，
        // 不是 Rust 字面量——写成 "true" 会让 TAP 解析器把整行读成 unknown。
        assert_eq!(bool_text(true), "ok");
        assert_eq!(bool_text(false), "not ok");
        // cfg_name 只对真 Kconfig 位给 CONFIG_*，机器侧事实（root/two-system/musl…）
        // 用 kselftest 自己的环境名——把两者混成一个前缀，skip 注释就骗过人了。
        assert_eq!(cfg_name(CFG_MODULES), "CONFIG_MODULES");
        assert_eq!(cfg_name(CFG_MUSL), "musl-target");
        assert_eq!(cfg_name(CFG_ROOT), "running-as-root");
        assert_eq!(cfg_name(CFG_LOOP), "loop-device");
        assert_eq!(cfg_name(1u32 << 20), "none");
        assert_ne!(cfg_name(CFG_NONE), "CONFIG_MUSL");
    }

    #[test]
    fn ledger_cells_are_segmented() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.rejects(), 0u32);
        assert!(l.audit().is_ok());
        assert_eq!(l.ok_count(), ACCEPT as u32);
        assert_eq!(l.err_count(), REJECT as u32);
        for i in 0..ACCEPT - 1usize {
            let s = match l.sample(i) {
                Some(x) => x,
                None => panic!(),
            };
            assert_eq!(s.op_name(), "stat");
            assert_eq!(s.value, VALS[i]);
            assert_eq!(s.code, 0u32);
        }
        let red = match l.sample(ACCEPT - 1usize) {
            Some(x) => x,
            None => panic!(),
        };
        assert_eq!(red.op_name(), "red-rate");
        assert_eq!(red.op, OP_RED_RATE);
        assert_eq!(red.value, RED_RATE);
        for i in 0..DEFECT_CELLS {
            let slot = ACCEPT + i;
            let s = match l.sample(slot) {
                Some(x) => x,
                None => panic!(),
            };
            assert_eq!(s.op_name(), "defect");
            assert!(!s.ok);
            assert_eq!(s.value, 0u32);
            assert_eq!(s.code, REJECT_CODES[i]);
        }
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_seal_freeze_replay() {
        let mut l = build_ledger();
        let root = l.seal().expect("seal");
        assert_eq!(l.root(), Some(root));
        assert_eq!(l.replay(), Ok(root));
        let mut m = build_ledger();
        let r2 = m.freeze().expect("freeze seals on demand");
        assert!(m.is_frozen());
        assert_eq!(r2, root);
        // 冻结后登记一律被拒，并且拒迹进指纹。
        assert!(m.register(Sample::stat(0usize, 0usize, 1u32)).is_err());
        assert_eq!(m.rejects(), 1u32);
        let mut p = build_ledger();
        let _ = p.seal();
        let clean_root = p.root().expect("root");
        assert!(p.poke(0usize, 12345u32).is_ok());
        assert_eq!(p.replay(), Err(KA_EUNIQ));
        assert_eq!(p.root(), Some(clean_root));
        // 未封存就偷改：偷改只对封存的账有意义。
        let mut q = build_ledger();
        assert!(q.poke(0usize, 1u32).is_err());
    }

    #[test]
    fn ledger_rejects_out_of_order_registration() {
        let mut p = Ledger::new();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0usize);
        // 插队：第 0 格不许写 slot=1。
        assert_eq!(p.register(Sample::stat(1usize, 0usize, 1u32)), Err(KA_ESLOT));
        assert!(p.is_empty());
        assert_eq!(p.rejects(), 1u32);
        // 空样本的 slot 是越界值，忘了填也必然被拒。
        assert_eq!(p.register(Sample::blank()), Err(KA_ESLOT));
        assert_eq!(p.rejects(), 2u32);
        let mut n = build_ledger();
        assert_eq!(n.register(Sample::blank()), Err(KA_ESLOT));
        assert_eq!(n.len(), SAMPLES);
        // 被拒过的账不许再冻结，否则"拒"就从指纹里消失了。
        assert!(n.freeze().is_err());
    }

    #[test]
    fn ledger_render_is_readable_and_stable() {
        let l = build_ledger();
        let t = l.render();
        assert!(t.as_str().contains("ledger cells=14"));
        assert!(t.as_str().contains("red-rate"));
        assert!(t.as_str().contains("6cf51"));
        let s = match l.sample(0usize) {
            Some(x) => x,
            None => panic!(),
        };
        assert!(t.as_str().contains(s.render().as_str()));
    }

    #[test]
    fn gate_walks_only_adjacent_stages() {
        let mut g = Gate::new();
        assert_eq!(g.at(), None);
        assert_eq!(g.moves(), 0u32);
        // 首步必须落清单闸——闸序是 1 基，写死 0 会让八道闸一道也走不动。
        assert_eq!(g.advance(Stage::Exec), Err(KA_ESTAGE));
        assert_eq!(g.rejects(), 1u32);
        assert_eq!(g.advance(Stage::Manifest), Ok(()));
        // 同级重复与越级都必须被拒：越级就是"没执行却报汇总"。
        assert_eq!(g.advance(Stage::Manifest), Err(KA_ESTAGE));
        assert_eq!(g.advance(Stage::Exec), Err(KA_ESTAGE));
        assert_eq!(g.advance(Stage::Identity), Ok(()));
        assert!(!g.full_walk());
        assert!(g.close().is_err());
    }

    #[test]
    fn gate_full_walk_is_replayable_and_rejects_are_accounted() {
        let mut a = Gate::new();
        assert_eq!(a.walk(), Ok(()));
        assert!(a.full_walk());
        assert!(a.closed());
        assert_eq!(a.moves() as usize, GATE_COUNT);
        let mut b = Gate::new();
        assert_eq!(b.walk(), Ok(()));
        assert_eq!(a.fold(FNV_OFF), b.fold(FNV_OFF));
        // 收口后的动作一律 KA_ESTATE，并且必须留痕在指纹里。
        let clean = a.fold(FNV_OFF);
        assert_eq!(a.advance(Stage::Seal), Err(KA_ESTATE));
        assert_eq!(a.close(), Err(KA_ESTATE));
        assert_eq!(a.rejects(), 2u32);
        assert_ne!(a.fold(FNV_OFF), clean);
    }

    #[test]
    fn inject_four_routes_are_all_caught() {
        let v = inject_four();
        assert_eq!(v, [true, true, true, true]);
        assert!(inject_ok());
    }

    #[test]
    fn every_predicate_that_gates_the_suite_holds() {
        assert!(name_ok());
        assert!(enums_ok());
        assert!(harness_ok());
        assert!(skip_ok());
        assert!(manifest_ok());
        assert!(define_ok());
        assert!(judge_ok());
        assert!(matrix_ok());
        assert!(corpus_ok());
        assert!(reds_ok());
        assert!(slots_ok());
        assert!(message_ok());
        assert!(tally_ok());
        assert!(detail_ok());
        assert!(ledger_ok());
        assert!(gate_ok());
        assert!(inject_ok());
    }

    #[test]
    fn slots_reference_real_manifest_rows() {
        for i in 0..CORPUS {
            let sh = match shape_at(i) {
                Some(x) => x,
                None => panic!(),
            };
            assert_eq!(sh.tag, WANT_TAGS[i]);
            assert!(sh.in_bench());
            let sp = match spec_index_of(sh.card).and_then(spec_at) {
                Some(x) => x,
                None => panic!("每条槽都指真判据"),
            };
            assert_eq!(sp.tag, sh.tag);
            assert_eq!(sp.harness, sh.harness);
            assert_eq!(sp.asserts, sh.asserts);
            assert_eq!(sp.default_suite, sh.default_suite);
            for j in i + 1usize..CORPUS {
                let o = match shape_at(j) {
                    Some(x) => x,
                    None => panic!(),
                };
                assert_ne!(o.card, sh.card, "14 槽不得复用同一条判据");
                assert_ne!(o.tag, sh.tag);
            }
        }
    }

    #[test]
    fn default_bit_only_survives_a_full_walk() {
        assert_eq!(default_count(), 11usize);
        assert_eq!(sealed_count(), 12usize);
        for i in 0..CORPUS {
            let o = match outcome_at(i) {
                Some(x) => x,
                None => panic!(),
            };
            if o.default {
                assert!(o.ok);
                assert_eq!(o.stage, Stage::Seal);
            }
        }
        // 走不满全程就没有资格谈默认位：把停留档往前挪一格，默认位必须落false。
        let mut sh = match shape_at(0usize) {
            Some(x) => x,
            None => panic!(),
        };
        sh.stage = Stage::Tally;
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Seal, "停留档由事实推演，不由申报字段决定");
        // 真把执行摘掉，才轮到 Ready 判死。
        let mut sh2 = green_shape(11641u16);
        sh2.ran = false;
        sh2.have_cfg = CFG_NONE;
        let o2 = define(&sh2);
        assert_eq!(o2.stage, Stage::Ready);
        assert!(!o2.default);
    }

    #[test]
    fn washed_flake_is_caught_by_retry_gate() {
        let mut sh = green_shape(11641u16);
        sh.washed = true;
        let o = define(&sh);
        assert!(!o.ok);
        assert_eq!(o.stage, Stage::Retry);
        assert_eq!(o.code, KA_EFLAKE);
        // attempts>1 却申报首次即中，同样是洗绿。
        let mut sh2 = green_shape(11641u16);
        sh2.washed = false;
        sh2.attempts = 2u8;
        sh2.first_ok = true;
        assert_eq!(define(&sh2).code, KA_EFLAKE);
        // 诚实的抖动：attempts>1 且首次失败，判 Flaked 并计入绿集。
        let mut sh3 = green_shape(11641u16);
        sh3.attempts = 3u8;
        sh3.first_ok = false;
        let o3 = define(&sh3);
        assert!(o3.ok);
        assert_eq!(o3.tier(), Tier::Flaked);
        assert_eq!(o3.note, Note::FlakeRecovered);
    }

    #[test]
    fn dep_not_done_blocks_tally() {
        let mut sh = green_shape(11648u16);
        assert!(sh.has_dep());
        sh.dep_done = false;
        let o = define(&sh);
        assert!(!o.ok);
        assert_eq!(o.stage, Stage::Tally);
        assert_eq!(o.code, KA_EBLOCK);
        assert_eq!(o.note, Note::DepBlocked);
        assert_eq!(o.tier(), Tier::Blocked);
    }

    #[test]
    fn identity_collision_is_a_manifest_fact() {
        let mut sh = green_shape(11641u16);
        sh.dup = true;
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Identity);
        assert_eq!(o.code, KA_EUNIQ);
        assert_eq!(o.note, Note::DupIdentity);
        // 依赖号落在批外是依赖闸的事，与身份闸分档。
        let mut sh2 = green_shape(11641u16);
        sh2.dup = false;
        sh2.dep = 9999u16;
        let o2 = define(&sh2);
        assert_eq!(o2.stage, Stage::Deps);
        assert_eq!(o2.code, KA_ECYCLE);
    }

    #[test]
    fn exit_and_errno_facts_are_exec_gate() {
        let mut sh = green_shape(11641u16);
        sh.exit_code = EX_NOTFOUND;
        let o = define(&sh);
        assert_eq!(o.stage, Stage::Exec);
        assert_eq!(o.code, KA_ECODE);
        assert_eq!(o.tier(), Tier::Blocked);
        let mut sh2 = green_shape(11641u16);
        sh2.exit_code = EX_OK;
        sh2.errno_bit = ENOSPC;
        assert_eq!(define(&sh2).stage, Stage::Exec);
        // 信号杀的退出码与 127 面必须分得开。
        assert_eq!(exit_text(EX_SIG_BASE + SIG_SEGV), "139");
        assert_eq!(exit_text(EX_SIG_BASE + SIG_ALRM), "142");
    }

    #[test]
    fn assert_short_is_a_tally_value_fault() {
        let mut sh = green_shape(11654u16);
        assert_eq!(sh.asserts, 8u16);
        sh.pass_asserts = 7u16;
        let o = define(&sh);
        assert!(!o.ok);
        assert_eq!(o.stage, Stage::Tally);
        assert_eq!(o.code, KA_EVALUE);
        assert_eq!(o.note, Note::AssertShort);
        assert_eq!(o.value, 7u32);
    }

    #[test]
    fn cfg_band_and_missing_bits() {
        let sh = green_shape(11653u16);
        assert_eq!(sh.cfg_missing(), CFG_NONE);
        assert!(sh.cfg_ready());
        let mut sh2 = sh;
        sh2.have_cfg = CFG_NO_MUSL;
        assert_eq!(sh2.cfg_missing(), CFG_MUSL);
        assert!(!sh2.cfg_ready());
        assert_eq!(define(&sh2).code, KA_ESKIP);
        // ms 量级档：未跑是 0 档，基线内 1 档，四倍内 2 档，再慢 3 档。
        let mut sh3 = green_shape(11641u16);
        sh3.ran = false;
        assert_eq!(sh3.ms_band(), 0u32);
        sh3.ran = true;
        let base = Harness::KselftestC.ms_band();
        sh3.ms = base;
        assert_eq!(sh3.ms_band(), 1u32);
        sh3.ms = base.saturating_mul(4u32);
        assert_eq!(sh3.ms_band(), 2u32);
        sh3.ms = base.saturating_mul(5u32);
        assert_eq!(sh3.ms_band(), 3u32);
    }

    #[test]
    fn outcome_fold_is_field_complete() {
        let a = match outcome_at(0usize) {
            Some(x) => x,
            None => panic!(),
        };
        let b = match outcome_at(6usize) {
            Some(x) => x,
            None => panic!(),
        };
        assert_ne!(a.fold(FNV_OFF), b.fold(FNV_OFF));
        let tag_a = a.fold_tag(FNV_OFF, WANT_TAGS[0usize]);
        let tag_b = a.fold_tag(FNV_OFF, WANT_TAGS[1usize]);
        assert_ne!(tag_a, tag_b);
        assert_eq!(a.fold_tag(FNV_OFF, WANT_TAGS[0usize]), tag_a);
    }

    #[test]
    fn tier_of_is_the_only_arbiter() {
        let mut sh = green_shape(11641u16);
        let o = define(&sh);
        assert_eq!(Tier::of(&o), Tier::Green);
        sh.attempts = 2u8;
        sh.first_ok = false;
        assert_eq!(Tier::of(&define(&sh)), Tier::Flaked);
        sh.skip = Skip::NoRoot;
        // skip 一旦成立就退到前置闸，档位不再谈绿。
        assert_eq!(Tier::of(&define(&sh)), Tier::Skipped);
        sh.skip = Skip::None;
        sh.exit_code = EX_FAIL;
        assert_eq!(Tier::of(&define(&sh)), Tier::Blocked);
        let mut ranks = 0u32;
        for k in 0..TIER_COUNT {
            if Tier::from_rank(k as u8).is_some() {
                ranks += 1;
            }
        }
        assert_eq!(ranks, TIER_COUNT as u32);
        assert_eq!(Tier::from_rank(TIER_COUNT as u8), None);
        // 权重单调：绿 > 抖 > 跳 > 阻。
        assert!(Tier::Green.weight() >= Tier::Flaked.weight());
        assert!(Tier::Flaked.weight() >= Tier::Skipped.weight());
        assert!(Tier::Skipped.weight() >= Tier::Blocked.weight());
    }

    #[test]
    fn cases_accessor_matches_slotwise_accessors() {
        let all = cases();
        assert_eq!(all.len(), CORPUS);
        for i in 0..CORPUS {
            let c = match case_at(i) {
                Some(x) => x,
                None => panic!(),
            };
            assert!(same_outcome(&c.actual(), &all[i].actual()));
            assert_eq!(c.shape.card, all[i].shape.card);
            assert_eq!(c.claim.success(), all[i].claim.success());
        }
        assert!(case_at(CORPUS).is_none());
    }

    #[test]
    fn claim_zero_is_not_a_live_declaration() {
        let z = Claim::zero();
        assert!(!z.success());
        assert_eq!(z.stage, Stage::Manifest);
        assert_eq!(z.code, 0u32);
        assert_eq!(z.value, 0u32);
        assert_eq!(z.note, Note::None);
        assert!(!z.default);
        // 空申报对上任何真实绿结局都判 Estate——占位不许冒充申报。
        let o = match outcome_at(0usize) {
            Some(x) => x,
            None => panic!(),
        };
        assert_eq!(judge_against(&z, &o), Fault::Estate);
    }

    #[test]
    fn row_perfect_requires_nonempty_alignment() {
        let mut r = Row::new(Tier::Green);
        assert!(!r.perfect());
        assert_eq!(r.total, 0u32);
        r.total = 3u32;
        r.aligned = 3u32;
        r.value_sum = 7u32;
        r.sealed = 2u32;
        r.spec_sum = 11u32;
        assert!(r.perfect());
        assert_eq!(r.render().as_str().contains("total=3"), true);
        // 逐字段折叠：改动任意一格，指纹必须变——否则矩阵汇总形同虚设。
        let h = r.fold(FNV_OFF);
        assert_ne!(h, FNV_OFF);
        let mut s = r;
        s.sealed = 1u32;
        assert_ne!(s.fold(FNV_OFF), h);
        let mut v = r;
        v.aligned = 2u32;
        assert!(!v.perfect());
        assert_ne!(v.fold(FNV_OFF), h);
        let z = Row::new(Tier::Skipped);
        assert!(!z.perfect());
        assert_eq!(z.total, 0u32);
    }

    #[test]
    fn suite_has_thirteen_items_and_all_pass() {
        let s = run_unx11660_checks();
        assert_eq!(s.len(), 13usize);
        assert!(s.all_passed());
        assert_eq!(s.red_items_text().as_str(), "");
    }
}

