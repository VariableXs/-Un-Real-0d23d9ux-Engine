//! UNX-F11699 ENOSPC 双机快照验证
//!
//! B25 整批（UNX-F11681 ~ UNX-F11698）钉的是磁盘写满时的行为：cp 中断、dd 记账、
//! tee 半写、sort 临时文件、sed -i、mv 跨设备回滚、touch/mkdir、df 零可用、fsync、
//! du 只读统计、管道阻塞、ENOSPC 与 EDQUOT 的分界、文案逐字、释放后重试恢复、
//! dd conv=noerror 档位、以及 12 工具退出码矩阵。这些判据在单机上各自报绿并不够：
//! 本卡把同一批观察点在**两台机器**上各跑一遍，逐格比对，一致率必须是 1000‰，
//! 而且任何不一致都必须以 CONFLICT 的形式显式落在格里，不许被平均、不许被取其一。
//!
//! ## 闸门次序（这条纵线为什么不能倒）
//!
//! 1. 清单闸（Manifest）——先数够 18 行。少一行说明有一条判据没上双机；多一行说明
//!    别批的观察点混了进来。条数不平，后面的一致率算的是别人的账。
//! 2. 身份闸（Identity）——18 张身份证的卡号、短名、段基址各自唯一。双机快照最怕
//!    两行同名：比对时它们会互相当成对方的一致性证据，一致率凭空变高。
//! 3. 维度闸（Dims）——18 行 × 每行 7 维 = 126 个观察格，并集必须铺满 13 维。洞不补，
//!    一致率再高也只是在盯着同一小块地方反复确认。
//! 4. 前置闸（Ready）——A 机与 B 机的取数条件都得在。只有一台在跑就谈不上"双机"；
//!    本卡把缺机的行记 skip 并计入红灯，绝不用单机结果顶双机。
//! 5. 执行闸（Exec）——两机各跑一次，取各自的退出码与字节数。执行事实是比对的原点。
//! 6. 重试闸（Retry）——任何一机 attempts>1 就记 Flaked，并露出是哪一台在抖。
//!    把抖动抹平后报告"一致"，是把双机跑成了单机。
//! 7. 汇总闸（Tally）——逐格比对：一致就记 agree，不一致就记 CONFLICT 并露出维度号。
//!    CONFLICT 是账目事实，不是错误码：它必须出现在钱数里，也必须出现在红灯里。
//! 8. 封印闸（Seal）——按 A 机序再按 B 机序折叠指纹。两台机器各自的顺序都要钉，
//!    只钉一边等于允许另一边随意重排。
//!
//! ## 关键差异（本卡深度所在）
//!
//! - **双机不是两份单机**：本卡每行带 `exit_a`/`exit_b`、`bytes_a`/`bytes_b` 两套事实，
//!   判定时先看两机各自是否成功，再比两机是否同一结论。单机成功 + 单机失败 = 冲突行，
//!   不能用"多数票"判绿——双机的意义就是把分歧钉在墙上。
//! - **CONFLICT 有编号**：冲突不记成一个布尔，而记成维度号（1..13）。同一行若在第 4
//!   维（部分写入字节）分叉，账上写 4；写 1 就是把字节分歧误报成退出码分歧，排障会
//!   走错方向。0 表示无冲突，因此 0 不能当"未知"用。
//! - **一致率的分母是格不是行**：18 行 × 7 维 = 126 格，一致率 = agree*1000/cells。
//!   按行算会把一条行内多维冲突稀释成一个"行不一致"，看不出到底几条纵线在分叉。
//! - **冻结重放四注入**：台账被改一位（跳槽、改格数、藏冲突、抬一致）都必须被审计抓住，
//!   四次注入四次拒收，才算"快照可复核"。
//! - **红档率 929 独占账本一格**：B25 的红档率是比率，不并进计数格，否则比率被当成条数。
//! - 语料语义：`WANT_*` 抄的是现场真实结论；申报表只在 `WANT_NDX` 指定的那一维上分叉。
//!   判定器先按真实结论判一遍，再按分叉维核一遍，两遍都过，这一槽才算真的被这一维抓到。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本模块段位基址（全仓唯一）。
pub const SEG_BASE: u32 = 0x6d430;

// ---------------------------------------------------------------------------
// 13 个错误码
// ---------------------------------------------------------------------------

/// 成败互报：申报通过但两机实跑不成，或反之。
pub const TAG_OK: u32 = SEG_BASE;

/// 成败维。
pub const DS_ESTATE: u32 = SEG_BASE + 0x01;
/// 闸门次序：八闸停留档不符，或封印序不等于清单序。
pub const DS_ESTAGE: u32 = SEG_BASE + 0x02;
/// 退出码面：两机退出码与申报不符，或两机退出码彼此不等。
pub const DS_ECODE: u32 = SEG_BASE + 0x03;
/// 计数值面：格数、一致数、字节数、通过率与申报不符。
pub const DS_EVALUE: u32 = SEG_BASE + 0x04;
/// 归因面：归因档与申报不符。
pub const DS_ENOTE: u32 = SEG_BASE + 0x05;
/// 默认位面：默认必跑位与申报不符。
pub const DS_EDEFAULT: u32 = SEG_BASE + 0x06;
/// 条数面：清单行数或槽数与申报不符。
pub const DS_ECOUNT: u32 = SEG_BASE + 0x07;
/// 身份面：卡号、短名或段基址重复。
pub const DS_EUNIQ: u32 = SEG_BASE + 0x08;
/// 维度面：掩码越界、为空，或维度账留洞。
pub const DS_EDIMS: u32 = SEG_BASE + 0x09;
/// 前置面：缺机或缺 CONFIG 位，判据没跑。
pub const DS_ESKIP: u32 = SEG_BASE + 0x0a;
/// 抖动面：某一机重试过，双机比对含抖动样本。
pub const DS_EFLAKE: u32 = SEG_BASE + 0x0b;
/// 冲突面：两机结论显式分叉，或字节数不等。
pub const DS_ECONF: u32 = SEG_BASE + 0x0c;
/// 越界面：槽号越过语料长度。
pub const DS_ESLOT: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 规模常量
// ---------------------------------------------------------------------------

/// 双机快照行数（B25 的 18 条判据各一行）。
pub const ROWS: usize = 18usize;
/// 判定维度数。
pub const DIMS: usize = 13usize;
/// 断言槽数。
pub const CORPUS: usize = 14usize;
/// 样本数（与槽数相等，一槽一样本）。
pub const SAMPLES: usize = 14usize;
/// 接受槽数。
pub const ACCEPT: usize = 8usize;
/// 拒绝槽数。
pub const REJECT: usize = 6usize;
/// 四档数。
pub const TIER_COUNT: usize = 4usize;
/// 八闸数。
pub const GATE_COUNT: usize = 8usize;
/// 红档率（千分比，独占账本一格）。
pub const RED_RATE: u32 = 929u32;
/// 双机观察格总数：18 × 7 = 126。
pub const WANT_DIM_ASSERTS: u32 = 126u32;
/// 每行至少应钉住的维数（低于此数记作 fragile）。
pub const WANT_DIM_FLOOR: u32 = 7u32;
/// 十三维全集。
pub const DIM_FULL: u16 = 0x1fffu16;
/// 无分叉时的维度记号。
pub const DIM_NONE: usize = DIMS;
/// 无冲突记号。
pub const CONFLICT_NONE: u8 = 0u8;
/// 日志字节下限。
pub const LOG_FLOOR: u32 = 64u32;
/// 退出码：成功。
pub const EX_OK: u8 = 0u8;
/// 退出码：一般失败。
pub const EX_FAIL: u8 = 1u8;
/// 退出码：用法错误。
pub const EX_MISUSE: u8 = 2u8;
/// 退出码：命令未找到。
pub const EX_NOTFOUND: u8 = 127u8;

// ---------------------------------------------------------------------------
// CONFIG 位
// ---------------------------------------------------------------------------

/// A 机（基线，glibc 线）。
pub const CFG_A: u32 = 1u32 << 0;
/// B 机（目标，musl 线）。
pub const CFG_B: u32 = 1u32 << 1;
/// busybox 工具面。
pub const CFG_BUSYBOX: u32 = 1u32 << 2;
/// loop 设备（回环小盘，制造 ENOSPC）。
pub const CFG_LOOP: u32 = 1u32 << 3;
/// tmpfs（释放空间可控）。
pub const CFG_TMPFS: u32 = 1u32 << 4;
/// root 权限。
pub const CFG_ROOT: u32 = 1u32 << 5;
/// 空集合。
pub const CFG_NONE: u32 = 0u32;
/// 全量。
pub const CFG_ALL: u32 = CFG_A | CFG_B | CFG_BUSYBOX | CFG_LOOP | CFG_TMPFS | CFG_ROOT;

// ---------------------------------------------------------------------------
// 取数通道
// ---------------------------------------------------------------------------

/// 一条判据的取数通道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Harness {
    /// 双机同跑（A 与 B 都必须在线）。
    Both,
    /// 仅 A 机可跑（基线侧先取事实）。
    Glibc,
    /// 仅 B 机可跑（目标侧复跑）。
    Musl,
    /// B 机 busybox 应用层。
    Busybox,
    /// 回环盘（制造满盘）。
    Loop,
    /// 直接系统调用。
    Direct,
}

impl Harness {
    /// 全表。
    pub const fn all() -> [Harness; 6] {
        [
            Harness::Both,
            Harness::Glibc,
            Harness::Musl,
            Harness::Busybox,
            Harness::Loop,
            Harness::Direct,
        ]
    }

    /// 序号。
    pub const fn rank(self) -> usize {
        match self {
            Harness::Both => 0usize,
            Harness::Glibc => 1usize,
            Harness::Musl => 2usize,
            Harness::Busybox => 3usize,
            Harness::Loop => 4usize,
            Harness::Direct => 5usize,
        }
    }

    /// 档名。
    pub const fn name(self) -> &'static str {
        match self {
            Harness::Both => "both",
            Harness::Glibc => "glibc",
            Harness::Musl => "musl",
            Harness::Busybox => "busybox",
            Harness::Loop => "loop",
            Harness::Direct => "direct",
        }
    }

    /// 该通道自身需要的 CONFIG 位。
    pub const fn requires(self) -> u32 {
        match self {
            Harness::Both => CFG_A | CFG_B,
            Harness::Glibc => CFG_A,
            Harness::Musl => CFG_B,
            Harness::Busybox => CFG_B | CFG_BUSYBOX,
            Harness::Loop => CFG_LOOP,
            Harness::Direct => CFG_NONE,
        }
    }

    /// 是否需要 A 机。
    pub const fn needs_a(self) -> bool {
        match self {
            Harness::Both => true,
            Harness::Glibc => true,
            Harness::Musl => false,
            Harness::Busybox => false,
            Harness::Loop => false,
            Harness::Direct => false,
        }
    }

    /// 是否需要 B 机。
    pub const fn needs_b(self) -> bool {
        match self {
            Harness::Both => true,
            Harness::Glibc => false,
            Harness::Musl => true,
            Harness::Busybox => true,
            Harness::Loop => false,
            Harness::Direct => false,
        }
    }

    /// 取第 i 个通道。
    pub fn at(i: usize) -> Option<Harness> {
        match Harness::all().get(i) {
            Some(h) => Some(*h),
            None => None,
        }
    }

    /// 按序号反查。
    pub fn of_rank(i: usize) -> Option<Harness> {
        let mut k = 0usize;
        while k < 6usize {
            if let Some(h) = Harness::all().get(k) {
                if h.rank() == i {
                    return Some(*h);
                }
            }
            k += 1usize;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 八闸
// ---------------------------------------------------------------------------

/// 八道闸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 清单闸。
    Manifest,
    /// 身份闸。
    Identity,
    /// 维度闸。
    Dims,
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
    /// 全表（闸序）。
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

    /// 序号。
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

    /// 取第 i 道闸。
    pub fn at(i: usize) -> Option<Stage> {
        match Stage::all().get(i) {
            Some(s) => Some(*s),
            None => None,
        }
    }

    /// 按序号反查。
    pub fn of_rank(i: usize) -> Option<Stage> {
        let mut k = 0usize;
        while k < GATE_COUNT {
            if let Some(s) = Stage::all().get(k) {
                if s.rank() == i {
                    return Some(*s);
                }
            }
            k += 1usize;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 十三维
// ---------------------------------------------------------------------------

/// 双机比对的十三个观察维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    /// 退出码。
    ExitCode,
    /// errno。
    Errno,
    /// stderr 文案。
    StderrText,
    /// stdout 文案。
    StdoutText,
    /// 部分写入字节数。
    PartialBytes,
    /// fsync 结果。
    FsyncResult,
    /// df 可用空间。
    DfAvail,
    /// du 统计。
    DuStat,
    /// 管道阻塞档。
    PipeMode,
    /// ENOSPC 与 EDQUOT 的分界。
    QuotaSplit,
    /// 文件系统类型。
    FsType,
    /// 日志字节量。
    LogBytes,
    /// 封印序指纹。
    OrderPrint,
}

impl Dim {
    /// 全表。
    pub const fn all() -> [Dim; DIMS] {
        [
            Dim::ExitCode,
            Dim::Errno,
            Dim::StderrText,
            Dim::StdoutText,
            Dim::PartialBytes,
            Dim::FsyncResult,
            Dim::DfAvail,
            Dim::DuStat,
            Dim::PipeMode,
            Dim::QuotaSplit,
            Dim::FsType,
            Dim::LogBytes,
            Dim::OrderPrint,
        ]
    }

    /// 序号。
    pub const fn rank(self) -> usize {
        match self {
            Dim::ExitCode => 0usize,
            Dim::Errno => 1usize,
            Dim::StderrText => 2usize,
            Dim::StdoutText => 3usize,
            Dim::PartialBytes => 4usize,
            Dim::FsyncResult => 5usize,
            Dim::DfAvail => 6usize,
            Dim::DuStat => 7usize,
            Dim::PipeMode => 8usize,
            Dim::QuotaSplit => 9usize,
            Dim::FsType => 10usize,
            Dim::LogBytes => 11usize,
            Dim::OrderPrint => 12usize,
        }
    }

    /// 维名。
    pub const fn name(self) -> &'static str {
        match self {
            Dim::ExitCode => "exit-code",
            Dim::Errno => "errno",
            Dim::StderrText => "stderr-text",
            Dim::StdoutText => "stdout-text",
            Dim::PartialBytes => "partial-bytes",
            Dim::FsyncResult => "fsync-result",
            Dim::DfAvail => "df-avail",
            Dim::DuStat => "du-stat",
            Dim::PipeMode => "pipe-mode",
            Dim::QuotaSplit => "quota-split",
            Dim::FsType => "fs-type",
            Dim::LogBytes => "log-bytes",
            Dim::OrderPrint => "order-print",
        }
    }

    /// 位掩码。
    pub const fn bit(self) -> u16 {
        1u16 << self.rank()
    }

    /// 取第 i 维。
    pub fn at(i: usize) -> Option<Dim> {
        match Dim::all().get(i) {
            Some(d) => Some(*d),
            None => None,
        }
    }

    /// 按序号反查。
    pub fn of_rank(i: usize) -> Option<Dim> {
        let mut k = 0usize;
        while k < DIMS {
            if let Some(d) = Dim::all().get(k) {
                if d.rank() == i {
                    return Some(*d);
                }
            }
            k += 1usize;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 十种归因档
// ---------------------------------------------------------------------------

/// 归因档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    /// 清单缺行。
    MissingRow,
    /// 维度掩码坏。
    DimBad,
    /// 取数通道缺失。
    HarnessMissing,
    /// CONFIG 位缺失。
    CfgMissing,
    /// 某一机抖动。
    FlakeOnce,
    /// 退出码不符。
    ExitBad,
    /// 双机分叉已显式记账。
    ConflictShown,
    /// 两机字节数不等。
    BytesDiffer,
    /// 上游未达。
    BlockedUp,
    /// 已封印。
    Sealed,
}

impl Note {
    /// 全表。
    pub const fn all() -> [Note; 10] {
        [
            Note::MissingRow,
            Note::DimBad,
            Note::HarnessMissing,
            Note::CfgMissing,
            Note::FlakeOnce,
            Note::ExitBad,
            Note::ConflictShown,
            Note::BytesDiffer,
            Note::BlockedUp,
            Note::Sealed,
        ]
    }

    /// 序号。
    pub const fn rank(self) -> usize {
        match self {
            Note::MissingRow => 0usize,
            Note::DimBad => 1usize,
            Note::HarnessMissing => 2usize,
            Note::CfgMissing => 3usize,
            Note::FlakeOnce => 4usize,
            Note::ExitBad => 5usize,
            Note::ConflictShown => 6usize,
            Note::BytesDiffer => 7usize,
            Note::BlockedUp => 8usize,
            Note::Sealed => 9usize,
        }
    }

    /// 档名。
    pub const fn name(self) -> &'static str {
        match self {
            Note::MissingRow => "missing-row",
            Note::DimBad => "dim-bad",
            Note::HarnessMissing => "harness-missing",
            Note::CfgMissing => "cfg-missing",
            Note::FlakeOnce => "flake-once",
            Note::ExitBad => "exit-bad",
            Note::ConflictShown => "conflict-shown",
            Note::BytesDiffer => "bytes-differ",
            Note::BlockedUp => "blocked-up",
            Note::Sealed => "sealed",
        }
    }

    /// 该档是否允许判绿。
    pub const fn is_green(self) -> bool {
        match self {
            Note::Sealed => true,
            Note::MissingRow => false,
            Note::DimBad => false,
            Note::HarnessMissing => false,
            Note::CfgMissing => false,
            Note::FlakeOnce => false,
            Note::ExitBad => false,
            Note::ConflictShown => false,
            Note::BytesDiffer => false,
            Note::BlockedUp => false,
        }
    }

    /// 取第 i 档。
    pub fn at(i: usize) -> Option<Note> {
        match Note::all().get(i) {
            Some(n) => Some(*n),
            None => None,
        }
    }
}

// ---------------------------------------------------------------------------
// FNV-1a
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFF: u64 = 0xcbf29ce484222325u64;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100000001b3u64;

/// 折叠一字节。
pub const fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折叠一个 32 位字（小端序）。
pub const fn fold_word(h: u64, w: u32) -> u64 {
    let b0 = (w & 0x0000_00ffu32) as u8;
    let b1 = ((w >> 8) & 0x0000_00ffu32) as u8;
    let b2 = ((w >> 16) & 0x0000_00ffu32) as u8;
    let b3 = ((w >> 24) & 0x0000_00ffu32) as u8;
    let h1 = fold_byte(h, b0);
    let h2 = fold_byte(h1, b1);
    let h3 = fold_byte(h2, b2);
    fold_byte(h3, b3)
}

/// 折叠一个字符串（不含结尾 NUL）。
pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc = fold_byte(acc, *b);
    }
    fold_word(acc, s.as_bytes().len() as u32)
}

/// 折叠一个 i32（符号位先行，再折绝对值）。
pub const fn fold_i32(h: u64, v: i32) -> u64 {
    let sign = match v < 0 {
        true => 1u32,
        false => 0u32,
    };
    let mag = v.unsigned_abs();
    let h1 = fold_word(h, sign);
    fold_word(h1, mag)
}

// ---------------------------------------------------------------------------
// 文本
// ---------------------------------------------------------------------------

/// 十进制无符号。
pub fn dec_text(v: u32, out: &mut String) {
    if v == 0u32 {
        out.push_str("0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    let mut x = v;
    while x > 0u32 && n < 10usize {
        let d = (x % 10u32) as u8;
        if let Some(slot) = buf.get_mut(n) {
            *slot = b'0' + d;
        }
        x /= 10u32;
        n += 1usize;
    }
    while n > 0usize {
        n -= 1usize;
        if let Some(b) = buf.get(n) {
            match core::str::from_utf8(core::slice::from_ref(b)) {
                Ok(s) => out.push_str(s),
                Err(_) => out.push_str("?"),
            }
        }
    }
}

/// 十六进制（小写，四位补零）。
pub fn hex_text(v: u32, out: &mut String) {
    let mut i = 4i32;
    while i > 0i32 {
        i -= 1i32;
        let shift = (i as u32) * 4u32;
        let d = ((v >> shift) & 0x0fu32) as u8;
        let c = match d {
            0u8 => '0',
            1u8 => '1',
            2u8 => '2',
            3u8 => '3',
            4u8 => '4',
            5u8 => '5',
            6u8 => '6',
            7u8 => '7',
            8u8 => '8',
            9u8 => '9',
            10u8 => 'a',
            11u8 => 'b',
            12u8 => 'c',
            13u8 => 'd',
            14u8 => 'e',
            _ => 'f',
        };
        let mut tmp = [0u8; 1];
        if let Some(slot) = tmp.get_mut(0) {
            *slot = c as u8;
        }
        match core::str::from_utf8(&tmp) {
            Ok(s) => out.push_str(s),
            Err(_) => out.push_str("?"),
        }
    }
}

/// 带符号十进制。
pub fn i32_text(v: i32, out: &mut String) {
    if v < 0i32 {
        out.push_str("-");
    }
    dec_text(v.unsigned_abs(), out);
}

/// 拼接 `前缀=数值` 形式。
pub fn kv_num(key: &str, v: u32, out: &mut String) {
    out.push_str(key);
    out.push_str("=");
    dec_text(v, out);
}

/// 拼接 `前缀=真/假` 形式。
pub fn kv_bool(key: &str, v: bool, out: &mut String) {
    out.push_str(key);
    match v {
        true => out.push_str("=true"),
        false => out.push_str("=false"),
    }
}

/// 拼接 `前缀=档名` 形式。
pub fn kv_name(key: &str, v: &'static str, out: &mut String) {
    out.push_str(key);
    out.push_str("=");
    out.push_str(v);
}

/// 千分比文本。
pub fn permille_text(n: u32, d: u32, out: &mut String) {
    let r = match d {
        0u32 => 0u32,
        _ => (n.saturating_mul(1000u32).saturating_add(d / 2u32)) / d,
    };
    dec_text(r, out);
}

/// 一位半字节的 popcount。
pub const fn popcount_nibble(n: u8) -> u32 {
    let mut v = n & 0x0fu8;
    let mut c = 0u32;
    while v != 0u8 {
        c += (v & 1u8) as u32;
        v >>= 1u8;
    }
    c
}

/// 16 位 popcount。
pub const fn popcount16(v: u16) -> u32 {
    let lo = (v & 0x00ffu16) as u8;
    let hi = ((v >> 8) & 0x00ffu16) as u8;
    let a = popcount_nibble(lo & 0x0fu8) + popcount_nibble((lo >> 4) & 0x0fu8);
    let b = popcount_nibble(hi & 0x0fu8) + popcount_nibble((hi >> 4) & 0x0fu8);
    a + b
}

/// 半字节右移四位（无符号）。
pub const fn hi_nibble(b: u8) -> u8 {
    (b >> 4) & 0x0fu8
}

/// 半字节低位。
pub const fn lo_nibble(b: u8) -> u8 {
    b & 0x0fu8
}

// ---------------------------------------------------------------------------
// 观察形态
// ---------------------------------------------------------------------------

/// 一行双机观察的形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    /// 清单行号（同时是索引）。
    pub idx: usize,
    /// 该行观察的维度掩码。
    pub dims: u16,
    /// 取数通道。
    pub harness: Harness,
    /// 该行额外的 CONFIG 需要。
    pub need: u32,
    /// A 机尝试次数。
    pub attempts_a: u8,
    /// B 机尝试次数。
    pub attempts_b: u8,
    /// A 机退出码。
    pub exit_a: u8,
    /// B 机退出码。
    pub exit_b: u8,
    /// A 机写入字节数。
    pub bytes_a: u32,
    /// B 机写入字节数。
    pub bytes_b: u32,
    /// 显式冲突维度号（1..13；0 表示两机一致）。
    pub conflict: u8,
    /// 是否进默认必跑集。
    pub default: bool,
    /// 是否拿到封印锚。
    pub anchor: bool,
    /// 现场 CONFIG 位。
    pub cfg: u32,
}

impl Shape {
    /// 掩码是否合法：非空且不越界。
    pub fn mask_ok(&self) -> bool {
        self.dims != 0u16 && (self.dims & !DIM_FULL) == 0u16
    }

    /// 两机是否都有成功字面。
    pub fn both_ok(&self) -> bool {
        self.exit_a == EX_OK && self.exit_b == EX_OK
    }

    /// 是否有显式冲突。
    pub fn has_conflict(&self) -> bool {
        self.conflict != CONFLICT_NONE
    }

    /// 冲突维度。
    pub fn conflict_dim(&self) -> Option<Dim> {
        if self.conflict == 0u8 {
            return None;
        }
        Dim::of_rank((self.conflict as usize) - 1usize)
    }

    /// 字节差（饱和）。
    pub fn byte_gap(&self) -> u32 {
        self.bytes_a.abs_diff(self.bytes_b)
    }

    /// 两台机器的最大尝试次数。
    pub fn max_attempts(&self) -> u8 {
        match self.attempts_a >= self.attempts_b {
            true => self.attempts_a,
            false => self.attempts_b,
        }
    }

    /// 该行覆盖的维数。
    pub fn dim_count(&self) -> u32 {
        popcount16(self.dims)
    }
}

/// 一行观察的结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 停留档。
    pub stage: Stage,
    /// 码面。
    pub code: u32,
    /// 计数值（字节数、冲突号、尝试数等）。
    pub value: u32,
    /// 归因。
    pub note: Note,
    /// 默认位。
    pub default: bool,
}

impl Outcome {
    /// 是否判绿。
    pub const fn ok(&self) -> bool {
        self.code == TAG_OK
    }

    /// 是否停在某闸之前。
    pub fn before(&self, s: Stage) -> bool {
        self.stage.rank() < s.rank()
    }

    /// 归因是否允许绿。
    pub const fn note_green(&self) -> bool {
        self.note.is_green()
    }

    /// 是否双机一致（无冲突且两机字节相等）。
    pub fn agreed(&self) -> bool {
        self.code == TAG_OK && self.note == Note::Sealed
    }
}

/// 清单里的一行身份证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowSpec {
    /// 卡号。
    pub card: u16,
    /// 短名。
    pub tag: &'static str,
    /// 段基址。
    pub seg: u32,
    /// 维度掩码。
    pub dims: u16,
    /// 取数通道。
    pub harness: Harness,
    /// 该行自身的 CONFIG 需要。
    pub need: u32,
    /// 是否进默认必跑集。
    pub default: bool,
}

/// 卡号区间下限。
pub const CARD_MIN: u16 = 11681u16;
/// 卡号区间上限。
pub const CARD_MAX: u16 = 11698u16;

/// 十八条判据的身份证。
///
/// 掩码按十三条纵线轮转落位：第 i 行从基座 `0x007f` 左旋 `5i mod 13` 位，于是每一维
/// 恰好被 7 行钉住；后五行各多看一维（第 14 行看部分写、第 15 行看配额分界、
/// 第 16 行看 stdout、第 17 行看日志量、第 18 行看封印序），但沿用加一维前的轮转位，
/// 使得总量停在 126 而不是随手涨到一个说不清的数。
pub const SPECS: [RowSpec; ROWS] = [
    RowSpec {
        card: 11681u16,
        tag: "f11681-enospc-master",
        seg: 0x6d130u32,
        dims: 0x007fu16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        default: true,
    },
    RowSpec {
        card: 11682u16,
        tag: "f11682-cp-abort",
        seg: 0x6d140u32,
        dims: 0x0fe0u16,
        harness: Harness::Both,
        need: CFG_LOOP,
        default: true,
    },
    RowSpec {
        card: 11683u16,
        tag: "f11683-dd-records",
        seg: 0x6d150u32,
        dims: 0x1c0fu16,
        harness: Harness::Musl,
        need: CFG_NONE,
        default: true,
    },
    RowSpec {
        card: 11684u16,
        tag: "f11684-tee-half",
        seg: 0x6d160u32,
        dims: 0x01fcu16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        default: false,
    },
    RowSpec {
        card: 11685u16,
        tag: "f11685-sort-tmp",
        seg: 0x6d310u32,
        dims: 0x1f81u16,
        harness: Harness::Busybox,
        need: CFG_BUSYBOX,
        default: true,
    },
    RowSpec {
        card: 11686u16,
        tag: "f11686-sed-i",
        seg: 0x6d320u32,
        dims: 0x103fu16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        default: true,
    },
    RowSpec {
        card: 11687u16,
        tag: "f11687-mv-rollback",
        seg: 0x6d330u32,
        dims: 0x07f0u16,
        harness: Harness::Loop,
        need: CFG_LOOP,
        default: true,
    },
    RowSpec {
        card: 11688u16,
        tag: "f11688-touch-mkdir",
        seg: 0x6d350u32,
        dims: 0x1e07u16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        default: true,
    },
    RowSpec {
        card: 11689u16,
        tag: "f11689-df-zero",
        seg: 0x6d360u32,
        dims: 0x00feu16,
        harness: Harness::Glibc,
        need: CFG_NONE,
        default: false,
    },
    RowSpec {
        card: 11690u16,
        tag: "f11690-rename",
        seg: 0x6d370u32,
        dims: 0x1fc0u16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        default: true,
    },
    RowSpec {
        card: 11691u16,
        tag: "f11691-fsync",
        seg: 0x6d390u32,
        dims: 0x181fu16,
        harness: Harness::Loop,
        need: CFG_LOOP,
        default: true,
    },
    RowSpec {
        card: 11692u16,
        tag: "f11692-du-ro",
        seg: 0x6d3a0u32,
        dims: 0x03f8u16,
        harness: Harness::Both,
        need: CFG_ROOT,
        default: true,
    },
    RowSpec {
        card: 11693u16,
        tag: "f11693-pipe-band",
        seg: 0x6d3b0u32,
        dims: 0x1f03u16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        default: true,
    },
    RowSpec {
        card: 11694u16,
        tag: "f11694-text-verbatim",
        seg: 0x6d3d0u32,
        dims: 0x03f8u16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        default: false,
    },
    RowSpec {
        card: 11695u16,
        tag: "f11695-vs-edquot",
        seg: 0x6d3e0u32,
        dims: 0x1f03u16,
        harness: Harness::Loop,
        need: CFG_LOOP,
        default: true,
    },
    RowSpec {
        card: 11696u16,
        tag: "f11696-recover",
        seg: 0x6d3f0u32,
        dims: 0x007fu16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        default: true,
    },
    RowSpec {
        card: 11697u16,
        tag: "f11697-dd-noerror",
        seg: 0x6d410u32,
        dims: 0x0fe0u16,
        harness: Harness::Musl,
        need: CFG_NONE,
        default: true,
    },
    RowSpec {
        card: 11698u16,
        tag: "f11698-exit-matrix",
        seg: 0x6d420u32,
        dims: 0x1c0fu16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        default: true,
    },
];

/// 取第 i 张身份证。
pub fn spec_at(i: usize) -> Option<&'static RowSpec> {
    SPECS.get(i)
}

/// 按卡号取身份证下标。
pub fn spec_index_of(card: u16) -> Option<usize> {
    let mut i = 0usize;
    while i < ROWS {
        match SPECS.get(i) {
            Some(s) => {
                if s.card == card {
                    return Some(i);
                }
            }
            None => return None,
        }
        i += 1usize;
    }
    None
}

/// 清单条数是否正确。
pub fn manifest_count_ok() -> bool {
    SPECS.len() == ROWS
}

/// 清单是否按卡号升序且连续。
pub fn manifest_sorted_ok() -> bool {
    let mut i = 0usize;
    while i < ROWS {
        match SPECS.get(i) {
            Some(s) => {
                if s.card != CARD_MIN + (i as u16) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    true
}

/// 短名是否互不重复。
pub fn tags_unique_ok() -> bool {
    let mut i = 0usize;
    while i < ROWS {
        match SPECS.get(i) {
            Some(a) => {
                if a.tag.is_empty() {
                    return false;
                }
                let mut j = i + 1usize;
                while j < ROWS {
                    if let Some(b) = SPECS.get(j) {
                        if a.tag == b.tag {
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
    true
}

/// 段基址是否互不重复且非零。
pub fn segs_unique_ok() -> bool {
    let mut i = 0usize;
    while i < ROWS {
        match SPECS.get(i) {
            Some(a) => {
                if a.seg == 0u32 {
                    return false;
                }
                let mut j = i + 1usize;
                while j < ROWS {
                    if let Some(b) = SPECS.get(j) {
                        if a.seg == b.seg {
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
    true
}

/// 默认必跑集非空且非全量。
pub fn default_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ROWS {
        match SPECS.get(i) {
            Some(s) => {
                if s.default {
                    n += 1u32;
                }
            }
            None => return n,
        }
        i += 1usize;
    }
    n
}

/// 清单闸。
pub fn bench_ok() -> bool {
    if !manifest_count_ok() || !manifest_sorted_ok() {
        return false;
    }
    if !tags_unique_ok() || !segs_unique_ok() {
        return false;
    }
    let n = default_count();
    if n == 0u32 || n == ROWS as u32 {
        return false;
    }
    spec_index_of(CARD_MIN) == Some(0usize) && spec_index_of(CARD_MAX) == Some(ROWS - 1usize)
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一格申报。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    /// 申报的成败。
    pub success: bool,
    /// 申报的停留档。
    pub stage: Stage,
    /// 申报的码面。
    pub code: u32,
    /// 申报的计数值。
    pub value: u32,
    /// 申报的归因。
    pub note: Note,
    /// 申报的默认位。
    pub default: bool,
    /// 短名（与清单逐格对齐）。
    pub tag: &'static str,
}

impl Claim {
    /// 与现场结论的分叉维数（0..6）。
    pub fn divergence(&self, o: &Outcome) -> u32 {
        let mut n = 0u32;
        if self.success != o.ok() {
            n += 1u32;
        }
        if self.stage != o.stage {
            n += 1u32;
        }
        if self.code != o.code {
            n += 1u32;
        }
        if self.value != o.value {
            n += 1u32;
        }
        if self.note != o.note {
            n += 1u32;
        }
        if self.default != o.default {
            n += 1u32;
        }
        n
    }

    /// 申报在某一维上是否与现场不同。
    pub fn differs(&self, o: &Outcome, d: Dim) -> bool {
        match d {
            Dim::ExitCode => self.success != o.ok(),
            Dim::Errno => self.stage != o.stage,
            Dim::StderrText => self.code != o.code,
            Dim::StdoutText => self.value != o.value,
            Dim::PartialBytes => self.note != o.note,
            Dim::FsyncResult => self.default != o.default,
            _ => false,
        }
    }
}

/// 一格语料 = 形态 + 申报。
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

    /// 短名。
    pub fn tag(&self) -> &'static str {
        self.claim.tag
    }
}

/// 前置是否齐。
pub fn ready(sh: &Shape, cfg: u32) -> bool {
    let need = sh.need | sh.harness.requires();
    (cfg & need) == need
}

/// 从形态推出结论。
pub fn define(sh: &Shape) -> Outcome {
    if sh.idx >= ROWS {
        return Outcome {
            stage: Stage::Manifest,
            code: DS_ECOUNT,
            value: sh.idx as u32,
            note: Note::MissingRow,
            default: sh.default,
        };
    }
    if !sh.mask_ok() {
        return Outcome {
            stage: Stage::Dims,
            code: DS_EDIMS,
            value: sh.dims as u32,
            note: Note::DimBad,
            default: sh.default,
        };
    }
    if !ready(sh, sh.cfg) {
        let hneed = sh.harness.requires();
        let hmiss = (sh.cfg & hneed) != hneed;
        let cmiss = (sh.cfg & sh.need) != sh.need;
        let note = match (hmiss, cmiss) {
            (true, _) => Note::HarnessMissing,
            (false, true) => Note::CfgMissing,
            (false, false) => Note::HarnessMissing,
        };
        return Outcome {
            stage: Stage::Ready,
            code: DS_ESKIP,
            value: 0u32,
            note,
            default: sh.default,
        };
    }
    if !sh.both_ok() {
        let packed = ((sh.exit_b as u32) << 8) | (sh.exit_a as u32);
        return Outcome {
            stage: Stage::Exec,
            code: DS_ECODE,
            value: packed,
            note: Note::ExitBad,
            default: sh.default,
        };
    }
    if sh.max_attempts() > 1u8 {
        return Outcome {
            stage: Stage::Retry,
            code: DS_EFLAKE,
            value: sh.max_attempts() as u32,
            note: Note::FlakeOnce,
            default: sh.default,
        };
    }
    if sh.has_conflict() {
        return Outcome {
            stage: Stage::Tally,
            code: DS_ECONF,
            value: sh.conflict as u32,
            note: Note::ConflictShown,
            default: sh.default,
        };
    }
    if sh.bytes_a != sh.bytes_b {
        return Outcome {
            stage: Stage::Tally,
            code: DS_ECONF,
            value: sh.byte_gap(),
            note: Note::BytesDiffer,
            default: sh.default,
        };
    }
    if !sh.anchor {
        return Outcome {
            stage: Stage::Seal,
            code: DS_ESTATE,
            value: 0u32,
            note: Note::BlockedUp,
            default: sh.default,
        };
    }
    Outcome {
        stage: Stage::Seal,
        code: TAG_OK,
        value: sh.bytes_a,
        note: Note::Sealed,
        default: sh.default,
    }
}

// ---------------------------------------------------------------------------
// 判定
// ---------------------------------------------------------------------------

/// 判定结论（六维 + 通过）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 通过。
    Ok,
    /// 成败维。
    Estate,
    /// 停留档维。
    Stage,
    /// 码面维。
    Code,
    /// 计数维。
    Value,
    /// 归因维。
    Note,
    /// 默认位维。
    Default,
}

impl Fault {
    /// 全表。
    pub const fn all() -> [Fault; 7] {
        [
            Fault::Ok,
            Fault::Estate,
            Fault::Stage,
            Fault::Code,
            Fault::Value,
            Fault::Note,
            Fault::Default,
        ]
    }

    /// 序号。
    pub const fn rank(self) -> usize {
        match self {
            Fault::Ok => 0usize,
            Fault::Estate => 1usize,
            Fault::Stage => 2usize,
            Fault::Code => 3usize,
            Fault::Value => 4usize,
            Fault::Note => 5usize,
            Fault::Default => 6usize,
        }
    }

    /// 对应的错误码。
    pub const fn code(self) -> u32 {
        match self {
            Fault::Ok => TAG_OK,
            Fault::Estate => DS_ESTATE,
            Fault::Stage => DS_ESTAGE,
            Fault::Code => DS_ECODE,
            Fault::Value => DS_EVALUE,
            Fault::Note => DS_ENOTE,
            Fault::Default => DS_EDEFAULT,
        }
    }

    /// 维名。
    pub const fn name(self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Estate => "estate",
            Fault::Stage => "estage",
            Fault::Code => "ecode",
            Fault::Value => "evalue",
            Fault::Note => "enote",
            Fault::Default => "edefault",
        }
    }

    /// 取第 i 个结论。
    pub fn at(i: usize) -> Option<Fault> {
        match Fault::all().get(i) {
            Some(f) => Some(*f),
            None => None,
        }
    }
}

/// 按六维次序判定一格。
pub fn judge_against(c: &Claim, o: &Outcome) -> Fault {
    if c.success != o.ok() {
        return Fault::Estate;
    }
    if c.stage != o.stage {
        return Fault::Stage;
    }
    if c.code != o.code {
        return Fault::Code;
    }
    if c.value != o.value {
        return Fault::Value;
    }
    if c.note != o.note {
        return Fault::Note;
    }
    if c.default != o.default {
        return Fault::Default;
    }
    Fault::Ok
}

/// 判定一格语料。
pub fn judge(c: &Case) -> Fault {
    let a = c.actual();
    judge_against(&c.claim, &a)
}

// ---------------------------------------------------------------------------
// 十四格形态
// ---------------------------------------------------------------------------

/// 十四格现场形态。
pub const SHAPES: [Shape; CORPUS] = [
    Shape {
        idx: 0usize,
        dims: 0x007fu16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 640u32,
        bytes_b: 640u32,
        conflict: CONFLICT_NONE,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 1usize,
        dims: 0x0fe0u16,
        harness: Harness::Both,
        need: CFG_LOOP,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 576u32,
        bytes_b: 576u32,
        conflict: CONFLICT_NONE,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 2usize,
        dims: 0x1c0fu16,
        harness: Harness::Musl,
        need: CFG_NONE,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 512u32,
        bytes_b: 512u32,
        conflict: CONFLICT_NONE,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 3usize,
        dims: 0x01fcu16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 448u32,
        bytes_b: 448u32,
        conflict: CONFLICT_NONE,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 4usize,
        dims: 0x1f81u16,
        harness: Harness::Busybox,
        need: CFG_BUSYBOX,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 384u32,
        bytes_b: 384u32,
        conflict: CONFLICT_NONE,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 5usize,
        dims: 0x103fu16,
        harness: Harness::Loop,
        need: CFG_LOOP,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 320u32,
        bytes_b: 320u32,
        conflict: CONFLICT_NONE,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 6usize,
        dims: 0x07f0u16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 256u32,
        bytes_b: 256u32,
        conflict: CONFLICT_NONE,
        default: false,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 7usize,
        dims: 0x1e07u16,
        harness: Harness::Direct,
        need: CFG_NONE,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: RED_RATE,
        bytes_b: RED_RATE,
        conflict: CONFLICT_NONE,
        default: false,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 8usize,
        dims: 0x00feu16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        attempts_a: 1u8,
        attempts_b: 3u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 512u32,
        bytes_b: 512u32,
        conflict: CONFLICT_NONE,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 9usize,
        dims: 0x1fc0u16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_FAIL,
        bytes_a: 256u32,
        bytes_b: 0u32,
        conflict: CONFLICT_NONE,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 10usize,
        dims: 0x181fu16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 512u32,
        bytes_b: 512u32,
        conflict: 4u8,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 11usize,
        dims: 0x0000u16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 256u32,
        bytes_b: 256u32,
        conflict: CONFLICT_NONE,
        default: true,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 12usize,
        dims: 0x03f8u16,
        harness: Harness::Both,
        need: CFG_TMPFS,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 512u32,
        bytes_b: 768u32,
        conflict: CONFLICT_NONE,
        default: false,
        anchor: true,
        cfg: CFG_ALL,
    },
    Shape {
        idx: 13usize,
        dims: 0x1f03u16,
        harness: Harness::Both,
        need: CFG_ROOT,
        attempts_a: 1u8,
        attempts_b: 1u8,
        exit_a: EX_OK,
        exit_b: EX_OK,
        bytes_a: 0u32,
        bytes_b: 0u32,
        conflict: CONFLICT_NONE,
        default: true,
        anchor: true,
        cfg: CFG_ALL & !CFG_ROOT,
    },
];

// ---------------------------------------------------------------------------
// 十四格申报
// ---------------------------------------------------------------------------

/// 十四格申报表。
pub const CLAIMS: [Claim; CORPUS] = [
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 640u32,
        note: Note::Sealed,
        default: true,
        tag: "f11681-enospc-master",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 576u32,
        note: Note::Sealed,
        default: true,
        tag: "f11682-cp-abort",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 512u32,
        note: Note::Sealed,
        default: true,
        tag: "f11683-dd-records",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 448u32,
        note: Note::Sealed,
        default: true,
        tag: "f11684-tee-half",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 384u32,
        note: Note::Sealed,
        default: true,
        tag: "f11685-sort-tmp",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 320u32,
        note: Note::Sealed,
        default: true,
        tag: "f11686-sed-i",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: 256u32,
        note: Note::Sealed,
        default: false,
        tag: "f11687-mv-rollback",
    },
    Claim {
        success: true,
        stage: Stage::Seal,
        code: TAG_OK,
        value: RED_RATE,
        note: Note::Sealed,
        default: false,
        tag: "f11688-touch-mkdir",
    },
    Claim {
        success: false,
        stage: Stage::Seal,
        code: DS_EFLAKE,
        value: 3u32,
        note: Note::FlakeOnce,
        default: true,
        tag: "f11689-df-zero",
    },
    Claim {
        success: false,
        stage: Stage::Exec,
        code: DS_ECODE,
        value: 255u32,
        note: Note::ExitBad,
        default: true,
        tag: "f11690-rename",
    },
    Claim {
        success: true,
        stage: Stage::Tally,
        code: DS_ECONF,
        value: 4u32,
        note: Note::ConflictShown,
        default: true,
        tag: "f11691-fsync",
    },
    Claim {
        success: false,
        stage: Stage::Dims,
        code: DS_EDIMS,
        value: 0u32,
        note: Note::Sealed,
        default: true,
        tag: "f11692-du-ro",
    },
    Claim {
        success: false,
        stage: Stage::Tally,
        code: DS_ECONF,
        value: 256u32,
        note: Note::BytesDiffer,
        default: true,
        tag: "f11693-pipe-band",
    },
    Claim {
        success: false,
        stage: Stage::Seal,
        code: DS_ESKIP,
        value: 0u32,
        note: Note::CfgMissing,
        default: true,
        tag: "f11694-text-verbatim",
    },
];

// ---------------------------------------------------------------------------
// 十二张现场断言表
// ---------------------------------------------------------------------------

/// 现场成败（逐槽）。
pub const WANT_OK: [bool; CORPUS] = [
    true, true, true, true, true, true, true, true, false, false, false, false, false, false,
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
    Stage::Tally,
    Stage::Ready,
];

/// 现场码面（逐槽）。
pub const WANT_CODE: [u32; CORPUS] = [
    TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, DS_EFLAKE, DS_ECODE, DS_ECONF,
    DS_EDIMS, DS_ECONF, DS_ESKIP,
];

/// 现场计数（逐槽）。
pub const WANT_VALUE: [u32; CORPUS] = [
    640u32, 576u32, 512u32, 448u32, 384u32, 320u32, 256u32, RED_RATE, 3u32, 256u32, 4u32, 0u32,
    256u32, 0u32,
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
    Note::ConflictShown,
    Note::DimBad,
    Note::BytesDiffer,
    Note::CfgMissing,
];

/// 现场默认位（逐槽）。
pub const WANT_DEFAULT: [bool; CORPUS] = [
    true, true, true, true, true, true, false, false, true, true, true, true, false, true,
];

/// 现场四档（逐槽）。
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
    Tier::Unrun,
    Tier::Void,
];

/// 申报分叉维（前 8 槽无分叉）。
pub const WANT_NDX: [usize; CORPUS] = [
    DIM_NONE, DIM_NONE, DIM_NONE, DIM_NONE, DIM_NONE, DIM_NONE, DIM_NONE, DIM_NONE, 1usize, 3usize,
    0usize, 4usize, 5usize, 1usize,
];

/// 每槽应有的判定故障码。
pub const WANT_FAULT_CODE: [u32; CORPUS] = [
    TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, TAG_OK, DS_ESTAGE, DS_EVALUE, DS_ESTATE,
    DS_ENOTE, DS_EDEFAULT, DS_ESTAGE,
];

/// 拒绝槽的六维次序。
pub const REJECT_FAULTS: [Fault; REJECT] = [
    Fault::Stage,
    Fault::Value,
    Fault::Estate,
    Fault::Note,
    Fault::Default,
    Fault::Stage,
];

/// 拒绝槽的错误码。
pub const REJECT_CODES: [u32; REJECT] = [
    DS_ESTAGE, DS_EVALUE, DS_ESTATE, DS_ENOTE, DS_EDEFAULT, DS_ESTAGE,
];

/// 八格计数（第 8 格是红档率，不并进统计）。
pub const VALS: [u32; 8] = [
    640u32, 576u32, 512u32, 448u32, 384u32, 320u32, 256u32, RED_RATE,
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

/// 空格（不参与判定）。
pub const EMPTY_SHAPE: Shape = Shape {
    idx: 0usize,
    dims: 0u16,
    harness: Harness::Direct,
    need: CFG_NONE,
    attempts_a: 1u8,
    attempts_b: 1u8,
    exit_a: EX_OK,
    exit_b: EX_OK,
    bytes_a: 0u32,
    bytes_b: 0u32,
    conflict: CONFLICT_NONE,
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
    note: Note::MissingRow,
    default: false,
    tag: "",
};

/// 空格语料。
pub const EMPTY_CASE: Case = Case {
    shape: EMPTY_SHAPE,
    claim: EMPTY_CLAIM,
};

/// 取一格语料。
pub fn case_at(i: usize) -> Option<Case> {
    match (shape_at(i), claim_at(i)) {
        (Some(shape), Some(claim)) => Some(Case { shape, claim }),
        _ => None,
    }
}

/// 全部语料。
pub fn cases() -> [Case; CORPUS] {
    let mut out = [EMPTY_CASE; CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        match (shape_at(i), claim_at(i), out.get_mut(i)) {
            (Some(s), Some(c), Some(dst)) => {
                *dst = Case {
                    shape: s,
                    claim: c,
                };
            }
            _ => {}
        }
        i += 1usize;
    }
    out
}

/// 取一格语料（按位次）。
pub fn case(i: usize) -> Case {
    match case_at(i) {
        Some(c) => c,
        None => EMPTY_CASE,
    }
}

/// 期望判定结论。
pub fn expected_fault(i: usize) -> Fault {
    if i < ACCEPT {
        return Fault::Ok;
    }
    match REJECT_FAULTS.get(i - ACCEPT) {
        Some(f) => *f,
        None => Fault::Ok,
    }
}

// ---------------------------------------------------------------------------
// 四档
// ---------------------------------------------------------------------------

/// 双机台账上的四档。
///
/// 档不是"绿不绿"，而是"这行算不算数"：带锚的进默认必跑集，已申报但不在默认集
/// 的只记账不阻塞，前置缺失退回空位，其余一律算未跑——未跑的行不许悄悄算进分母。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 带锚：双机都跑通，且进默认必跑集。
    Anchored,
    /// 已申报：双机都跑通，但只在按需集里。
    Declared,
    /// 未跑：没有拿到封印结论。
    Unrun,
    /// 空位：前置缺失，整行作废。
    Void,
}

impl Tier {
    /// 四档全表（由重到轻）。
    pub const fn all() -> [Tier; TIER_COUNT] {
        [
            Tier::Anchored,
            Tier::Declared,
            Tier::Unrun,
            Tier::Void,
        ]
    }

    /// 序号。
    pub const fn rank(self) -> usize {
        match self {
            Tier::Anchored => 0usize,
            Tier::Declared => 1usize,
            Tier::Unrun => 2usize,
            Tier::Void => 3usize,
        }
    }

    /// 档名。
    pub const fn name(self) -> &'static str {
        match self {
            Tier::Anchored => "anchored",
            Tier::Declared => "declared",
            Tier::Unrun => "unrun",
            Tier::Void => "void",
        }
    }

    /// 取第 i 档。
    pub fn at(i: usize) -> Option<Tier> {
        match Tier::all().get(i) {
            Some(t) => Some(*t),
            None => None,
        }
    }

    /// 按序号反查。
    pub fn of_rank(i: usize) -> Option<Tier> {
        let mut k = 0usize;
        while k < TIER_COUNT {
            if let Some(t) = Tier::all().get(k) {
                if t.rank() == i {
                    return Some(*t);
                }
            }
            k += 1usize;
        }
        None
    }
}

/// 由结论分档。
///
/// 只有"停在封印闸且码面为 TAG_OK"的行才可能进前两档；前置缺失走 DS_ESKIP 的行
/// 是空位而不是未跑——空位会从分母里摘掉，未跑不会。
pub fn tier_of(o: &Outcome) -> Tier {
    if o.stage == Stage::Seal && o.code == TAG_OK {
        return match o.default {
            true => Tier::Anchored,
            false => Tier::Declared,
        };
    }
    if o.code == DS_ESKIP {
        return Tier::Void;
    }
    Tier::Unrun
}

/// 判一格的档。
pub fn tier_of_index(i: usize) -> Option<Tier> {
    match outcome_at(i) {
        Some(o) => Some(tier_of(&o)),
        None => None,
    }
}

/// 逐档计数，顺序与 `Tier::all()` 一致。
pub fn tier_counts() -> [u32; TIER_COUNT] {
    let mut out = [0u32; TIER_COUNT];
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            let t = tier_of(&c.actual());
            if let Some(slot) = out.get_mut(t.rank()) {
                *slot += 1u32;
            }
        }
        i += 1usize;
    }
    out
}

/// 取某档的计数。
pub fn tier_count(t: Tier) -> u32 {
    match tier_counts().get(t.rank()) {
        Some(n) => *n,
        None => 0u32,
    }
}

/// 四档矩阵：四档之和必须等于槽数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matrix {
    /// 带锚行数。
    pub anchored: u32,
    /// 已申报行数。
    pub declared: u32,
    /// 未跑行数。
    pub unrun: u32,
    /// 空位行数。
    pub void: u32,
}

impl Matrix {
    /// 空格矩阵。
    pub const EMPTY: Matrix = Matrix {
        anchored: 0u32,
        declared: 0u32,
        unrun: 0u32,
        void: 0u32,
    };

    /// 从计数表构造。
    pub fn from_counts(c: [u32; TIER_COUNT]) -> Matrix {
        Matrix {
            anchored: match c.get(0usize) {
                Some(n) => *n,
                None => 0u32,
            },
            declared: match c.get(1usize) {
                Some(n) => *n,
                None => 0u32,
            },
            unrun: match c.get(2usize) {
                Some(n) => *n,
                None => 0u32,
            },
            void: match c.get(3usize) {
                Some(n) => *n,
                None => 0u32,
            },
        }
    }

    /// 现场矩阵。
    pub fn live() -> Matrix {
        Matrix::from_counts(tier_counts())
    }

    /// 逐档取数。
    pub fn at(&self, i: usize) -> u32 {
        match i {
            0usize => self.anchored,
            1usize => self.declared,
            2usize => self.unrun,
            3usize => self.void,
            _ => 0u32,
        }
    }

    /// 四档之和。
    pub fn total(&self) -> u32 {
        self.anchored
            .saturating_add(self.declared)
            .saturating_add(self.unrun)
            .saturating_add(self.void)
    }

    /// 四档之和是否正好等于槽数。
    pub fn balanced(&self) -> bool {
        self.total() == CORPUS as u32
    }

    /// 有锚的档是否非空且非全量（空锚或全锚都说明分档没意义）。
    pub fn anchored_range_ok(&self) -> bool {
        self.anchored != 0u32 && self.anchored != CORPUS as u32
    }

    /// 逐档索引是否闭合：每个下标都能反查到同名档。
    pub fn indices_ok(&self) -> bool {
        let mut i = 0usize;
        while i < TIER_COUNT {
            match Tier::of_rank(i) {
                Some(t) => {
                    if t.rank() != i {
                        return false;
                    }
                    if Tier::at(i) != Some(t) {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1usize;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 清单行
// ---------------------------------------------------------------------------

/// 清单里的一行，连同它的维度自检与分档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    /// 身份证。
    pub spec: RowSpec,
    /// 掩码是否非空且不越界。
    pub dims_ok: bool,
    /// 该行在清单里的档（只看默认位）。
    pub tier: Tier,
}

impl Row {
    /// 该行钉住的维数。
    pub fn dim_count(&self) -> u32 {
        popcount16(self.spec.dims)
    }

    /// 该行的取数通道是否需要 A 机。
    pub fn touches_a(&self) -> bool {
        self.spec.harness.needs_a()
    }

    /// 该行的取数通道是否需要 B 机。
    pub fn touches_b(&self) -> bool {
        self.spec.harness.needs_b()
    }

    /// 该行是否需要双机都到齐。
    pub fn dual(&self) -> bool {
        self.touches_a() && self.touches_b()
    }

    /// 该行的 CONFIG 需求（通道与本行需求合并）。
    pub fn need(&self) -> u32 {
        self.spec.need | self.spec.harness.requires()
    }
}

/// 空格行。
pub const EMPTY_ROW: Row = Row {
    spec: SPECS[0],
    dims_ok: false,
    tier: Tier::Void,
};

/// 取一行。
pub fn row_at(i: usize) -> Option<Row> {
    match SPECS.get(i) {
        Some(s) => Some(Row {
            spec: *s,
            dims_ok: s.dims != 0u16 && (s.dims & !DIM_FULL) == 0u16,
            tier: match s.default {
                true => Tier::Anchored,
                false => Tier::Declared,
            },
        }),
        None => None,
    }
}

/// 全部行。
pub fn rows() -> [Row; ROWS] {
    let mut out = [EMPTY_ROW; ROWS];
    let mut i = 0usize;
    while i < ROWS {
        match (row_at(i), out.get_mut(i)) {
            (Some(r), Some(dst)) => {
                *dst = r;
            }
            _ => {}
        }
        i += 1usize;
    }
    out
}

/// 清单里有几行掩码合法。
pub fn dims_ok_rows() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ROWS {
        match row_at(i) {
            Some(r) => {
                if r.dims_ok {
                    n += 1u32;
                }
            }
            None => return n,
        }
        i += 1usize;
    }
    n
}

// ---------------------------------------------------------------------------
// 维度覆盖统计
// ---------------------------------------------------------------------------

/// 十八行钉住的总格数。
pub fn dim_total() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ROWS {
        match SPECS.get(i) {
            Some(s) => n = n.saturating_add(popcount16(s.dims)),
            None => return n,
        }
        i += 1usize;
    }
    n
}

/// 被至少一行钉住的维数。
pub fn dim_cover() -> u32 {
    let mut union = 0u16;
    let mut i = 0usize;
    while i < ROWS {
        match SPECS.get(i) {
            Some(s) => union |= s.dims & DIM_FULL,
            None => return popcount16(union),
        }
        i += 1usize;
    }
    popcount16(union)
}

/// 一次都没被钉住的维数。
pub fn dim_holes() -> u32 {
    (DIMS as u32).saturating_sub(dim_cover())
}

/// 钉得偏薄的行数（低于 `WANT_DIM_FLOOR`）。
pub fn dim_fragile() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ROWS {
        match SPECS.get(i) {
            Some(s) => {
                if popcount16(s.dims) < WANT_DIM_FLOOR {
                    n += 1u32;
                }
            }
            None => return n,
        }
        i += 1usize;
    }
    n
}

/// 钉满十三维的行数。
pub fn dim_dense() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ROWS {
        match SPECS.get(i) {
            Some(s) => {
                if s.dims & DIM_FULL == DIM_FULL {
                    n += 1u32;
                }
            }
            None => return n,
        }
        i += 1usize;
    }
    n
}

/// 能报出名字的维数（十三维表自身是否完整）。
pub fn dim_names() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < DIMS {
        match Dim::at(i) {
            Some(d) => {
                if d.rank() == i && !d.name().is_empty() && d.bit() == (1u16 << i) {
                    n += 1u32;
                }
            }
            None => return n,
        }
        i += 1usize;
    }
    n
}

// ---------------------------------------------------------------------------
// 台账
// ---------------------------------------------------------------------------

/// 双机台账的一次汇总。
///
/// `cells` 是十八行钉住的观察格总数，`agree` 是两机在格上看法一致的数目；
/// `conflicts` 只统计"判错了的槽"，被正确判红的槽不算冲突。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// 观察格总数。
    pub cells: u32,
    /// 两机一致的格数。
    pub agree: u32,
    /// 判错的槽数。
    pub conflicts: u32,
    /// 判对的槽数。
    pub matched: u32,
    /// 槽总数。
    pub total: u32,
    /// 槽位越界时记录的错码（0 表示没有越界）。
    pub slot: u32,
    /// 走到第几道闸。
    pub gate: u32,
    /// 台账是否已封印且重放一致。
    pub sealed: bool,
}

impl Report {
    /// 空格台账。
    pub const EMPTY: Report = Report {
        cells: 0u32,
        agree: 0u32,
        conflicts: 0u32,
        matched: 0u32,
        total: 0u32,
        slot: 0u32,
        gate: 0u32,
        sealed: false,
    };

    /// 千分比一致率（四舍五入）。
    pub fn rate(&self) -> u32 {
        match self.cells {
            0u32 => 0u32,
            _ => self
                .agree
                .saturating_mul(1000u32)
                .saturating_add(self.cells / 2u32)
                / self.cells,
        }
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.matched == self.total
            && self.total == CORPUS as u32
            && self.conflicts == 0u32
            && self.agree == self.cells
            && self.cells == WANT_DIM_ASSERTS
            && self.slot == 0u32
            && self.gate == GATE_COUNT as u32
            && self.sealed
    }

    /// 闸与封印是否都齐。
    pub fn gate(&self) -> bool {
        self.gate == GATE_COUNT as u32 && self.sealed
    }

    /// 有没有槽位越界。
    pub fn slot_fault(&self) -> bool {
        self.slot != 0u32
    }

    /// 渲染成五行文本。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("dual=DS99 ");
        kv_num("rows", ROWS as u32, &mut out);
        out.push_str(" ");
        kv_num("cells", self.cells, &mut out);
        out.push_str(" ");
        kv_num("agree", self.agree, &mut out);
        out.push_str(" ");
        kv_num("conflicts", self.conflicts, &mut out);
        out.push_str(" rate=");
        permille_text(self.agree, self.cells, &mut out);
        out.push_str("\n");
        kv_num("slots", self.total, &mut out);
        out.push_str(" ");
        kv_num("accept", ACCEPT as u32, &mut out);
        out.push_str(" ");
        kv_num("reject", REJECT as u32, &mut out);
        out.push_str(" ");
        kv_num("matched", self.matched, &mut out);
        out.push_str("\n");
        let m = Matrix::live();
        out.push_str("tiers");
        out.push_str(" anchored=");
        dec_text(m.anchored, &mut out);
        out.push_str(" declared=");
        dec_text(m.declared, &mut out);
        out.push_str(" void=");
        dec_text(m.void, &mut out);
        out.push_str(" unrun=");
        dec_text(m.unrun, &mut out);
        out.push_str("\n");
        out.push_str("dims cover=");
        dec_text(dim_total(), &mut out);
        out.push_str(" holes=");
        dec_text(dim_holes(), &mut out);
        out.push_str(" fragile=");
        dec_text(dim_fragile(), &mut out);
        out.push_str(" dense=");
        dec_text(dim_dense(), &mut out);
        out.push_str(" full=");
        dec_text(dim_cover(), &mut out);
        out.push_str("\n");
        out.push_str("gate=");
        dec_text(self.gate, &mut out);
        out.push_str("/");
        dec_text(GATE_COUNT as u32, &mut out);
        out.push_str(" seal=");
        match self.sealed {
            true => out.push_str("ok"),
            false => out.push_str("bad"),
        }
        out.push_str(" green=");
        match self.green() {
            true => out.push_str("true"),
            false => out.push_str("false"),
        }
        out
    }
}

/// 定稿渲染（绿账逐字节比对）。
pub const RENDER_OK: &str = "dual=DS99 rows=18 cells=126 agree=126 conflicts=0 rate=1000\nslots=14 accept=8 reject=6 matched=14\ntiers anchored=6 declared=2 void=1 unrun=5\ndims cover=126 holes=0 fragile=0 dense=0 full=13\ngate=8/8 seal=ok green=true";

/// 走一遍八闸，返回闸数。
pub fn gate_steps() -> u32 {
    let mut g = Gate::new();
    let mut i = 0usize;
    while i < GATE_COUNT {
        match Stage::at(i) {
            Some(s) => {
                g.note(s);
            }
            None => return g.seen_count(),
        }
        i += 1usize;
    }
    g.seen_count()
}

/// 台账是否封印且重放一致。
pub fn ledger_sealed() -> bool {
    let l = build_ledger();
    l.is_sealed() && l.replay()
}

/// 判一遍语料。
pub fn judge_all() -> [Fault; CORPUS] {
    let mut out = [Fault::Ok; CORPUS];
    let mut i = 0usize;
    while i < CORPUS {
        match (case_at(i), out.get_mut(i)) {
            (Some(c), Some(dst)) => {
                *dst = judge(&c);
            }
            _ => {}
        }
        i += 1usize;
    }
    out
}

/// 全跑一遍。
pub fn run() -> Report {
    run_with(None)
}

/// 带覆盖值的全跑：覆盖某槽的计数值，槽位越界记 DS_ESLOT。
pub fn run_with(over: Option<(usize, u32)>) -> Report {
    let mut matched = 0u32;
    let mut conflicts = 0u32;
    let mut slot = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        match (shape_at(i), claim_at(i)) {
            (Some(sh), Some(mut cl)) => {
                if let Some((at, v)) = over {
                    if at == i {
                        cl.value = v;
                    }
                }
                let o = define(&sh);
                match judge_against(&cl, &o) == expected_fault(i) {
                    true => matched = matched.saturating_add(1u32),
                    false => conflicts = conflicts.saturating_add(1u32),
                }
            }
            _ => conflicts = conflicts.saturating_add(1u32),
        }
        i += 1usize;
    }
    if let Some((at, _v)) = over {
        if at >= CORPUS {
            slot = DS_ESLOT;
        }
    }
    let cells = dim_total();
    let agree = cells.saturating_sub(conflicts.saturating_mul(WANT_DIM_FLOOR));
    Report {
        cells,
        agree,
        conflicts,
        matched,
        total: CORPUS as u32,
        slot,
        gate: gate_steps(),
        sealed: ledger_sealed(),
    }
}

// ---------------------------------------------------------------------------
// 八闸推进器
// ---------------------------------------------------------------------------

/// 八闸推进器。
///
/// 三条铁律：同一道闸不许进两次；闸号只许升不许降；封印之后一句都不许再写。
/// 违例不抛不炸，只回错误码并记账——现场要的是"看得见"，不是"跑不下去"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    seen: u16,
    top: usize,
    started: bool,
    closed: bool,
    steps: u32,
    regressions: u32,
    late: u32,
}

impl Gate {
    /// 空闸。
    pub const fn new() -> Gate {
        Gate {
            seen: 0u16,
            top: 0usize,
            started: false,
            closed: false,
            steps: 0u32,
            regressions: 0u32,
            late: 0u32,
        }
    }

    /// 进一道闸：成功回 0。
    pub fn note(&mut self, s: Stage) -> u32 {
        if self.closed {
            self.late = self.late.saturating_add(1u32);
            return DS_ESTATE;
        }
        let bit = 1u16 << s.rank();
        if (self.seen & bit) != 0u16 {
            self.regressions = self.regressions.saturating_add(1u32);
            return DS_ESTAGE;
        }
        if self.started && s.rank() < self.top {
            self.regressions = self.regressions.saturating_add(1u32);
            return DS_ESTAGE;
        }
        self.seen |= bit;
        if s.rank() > self.top {
            self.top = s.rank();
        }
        self.started = true;
        self.steps = self.steps.saturating_add(1u32);
        0u32
    }

    /// 封印：回已进闸数；重复封印回 DS_ESTATE。
    pub fn close(&mut self) -> u32 {
        if self.closed {
            self.late = self.late.saturating_add(1u32);
            return DS_ESTATE;
        }
        self.closed = true;
        self.seen_count()
    }

    /// 已进闸数。
    pub const fn seen_count(&self) -> u32 {
        self.seen.count_ones()
    }

    /// 步数。
    pub const fn step_count(&self) -> u32 {
        self.steps
    }

    /// 最高闸号。
    pub const fn top_rank(&self) -> usize {
        self.top
    }

    /// 是否八闸全到。
    pub fn complete(&self) -> bool {
        self.seen_count() == GATE_COUNT as u32 && self.steps == GATE_COUNT as u32
    }

    /// 是否一次没退。
    pub fn monotone(&self) -> bool {
        self.regressions == 0u32
    }

    /// 是否没在封印后写过。
    pub fn quiet_after_close(&self) -> bool {
        self.late == 0u32
    }

    /// 是否已封印。
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// 倒退次数。
    pub const fn regressions(&self) -> u32 {
        self.regressions
    }

    /// 封印后写入次数。
    pub const fn late_writes(&self) -> u32 {
        self.late
    }

    /// 每道闸是否各进一次。
    pub fn one_each(&self) -> bool {
        let mut i = 0usize;
        while i < GATE_COUNT {
            match Stage::at(i) {
                Some(s) => {
                    if (self.seen & (1u16 << s.rank())) == 0u16 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1usize;
        }
        self.seen_count() == GATE_COUNT as u32
    }

    /// 从头按序走完八闸。
    pub fn walk() -> Gate {
        let mut g = Gate::new();
        let mut i = 0usize;
        while i < GATE_COUNT {
            if let Some(s) = Stage::at(i) {
                g.note(s);
            }
            i += 1usize;
        }
        g.close();
        g
    }
}

// ---------------------------------------------------------------------------
// 台账格与台账
// ---------------------------------------------------------------------------

/// 台账里的一格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 槽号。
    pub idx: usize,
    /// 短名。
    pub tag: &'static str,
    /// 该槽判绿与否。
    pub ok: bool,
    /// 该槽的计数值。
    pub value: u32,
}

/// 空格。
pub const EMPTY_SAMPLE: Sample = Sample {
    idx: 0usize,
    tag: "",
    ok: false,
    value: 0u32,
};

/// 双机台账。
///
/// 槽号必须等于当前长度——这是台账唯一的结构约束，也是对"另一台机器"的信任边界：
/// 谁都不能往中间插一行。封印时打指纹，冻结后任何写入都会被重放抓出来。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    slots: [Sample; SAMPLES],
    len: usize,
    sealed: bool,
    frozen: bool,
    stamp: u64,
    rejected: u32,
}

impl Ledger {
    /// 空台账。
    pub const fn new() -> Ledger {
        Ledger {
            slots: [EMPTY_SAMPLE; SAMPLES],
            len: 0usize,
            sealed: false,
            frozen: false,
            stamp: 0u64,
            rejected: 0u32,
        }
    }

    /// 落一格：成功回 0。
    pub fn push(&mut self, s: Sample) -> u32 {
        if self.frozen {
            self.rejected = self.rejected.saturating_add(1u32);
            return DS_ESTATE;
        }
        if s.idx != self.len || self.len >= SAMPLES {
            self.rejected = self.rejected.saturating_add(1u32);
            return DS_ECOUNT;
        }
        if let Some(dst) = self.slots.get_mut(self.len) {
            *dst = s;
            self.len += 1usize;
            return 0u32;
        }
        self.rejected = self.rejected.saturating_add(1u32);
        DS_ECOUNT
    }

    /// 数一遍绿格。
    pub fn audit(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            match self.slots.get(i) {
                Some(s) => {
                    if s.ok && s.idx == i {
                        n = n.saturating_add(1u32);
                    }
                }
                None => return n,
            }
            i += 1usize;
        }
        n
    }

    /// 封印：打指纹。不满格不许封。
    pub fn seal(&mut self) -> u32 {
        if self.len != SAMPLES {
            return DS_ECOUNT;
        }
        if self.sealed {
            return DS_ESTATE;
        }
        self.stamp = self.root();
        self.sealed = true;
        0u32
    }

    /// 冻结：封印之后才许冻。
    pub fn freeze(&mut self) -> u32 {
        if !self.sealed {
            return DS_ESTATE;
        }
        self.frozen = true;
        0u32
    }

    /// 冻结后写入：改得动，但留痕。
    pub fn poke(&mut self, i: usize, v: u32) -> u32 {
        let mut code = 0u32;
        if self.frozen {
            self.rejected = self.rejected.saturating_add(1u32);
            code = DS_ECONF;
        }
        match self.slots.get_mut(i) {
            Some(s) => {
                s.value = v;
                code
            }
            None => DS_ESLOT,
        }
    }

    /// 指纹。
    pub fn root(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            match self.slots.get(i) {
                Some(s) => {
                    h = fold_i32(h, s.idx as i32);
                    h = fold_str(h, s.tag);
                    h = fold_word(h, s.value);
                    h = fold_byte(
                        h,
                        match s.ok {
                            true => 1u8,
                            false => 0u8,
                        },
                    );
                }
                None => return h,
            }
            i += 1usize;
        }
        h
    }

    /// 重放：冻结过且指纹未变。
    pub fn replay(&self) -> bool {
        self.frozen && self.sealed && self.root() == self.stamp
    }

    /// 槽数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否已封印。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 是否已冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 封印指纹。
    pub const fn stamp(&self) -> u64 {
        self.stamp
    }

    /// 被拒次数。
    pub const fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 取一格。
    pub fn sample_at(&self, i: usize) -> Option<Sample> {
        match self.slots.get(i) {
            Some(s) => {
                if i < self.len {
                    Some(*s)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 是否满格。
    pub fn full(&self) -> bool {
        self.len == SAMPLES
    }

    /// 是否全绿满格。
    pub fn all_green(&self) -> bool {
        self.full() && self.audit() == SAMPLES as u32
    }

    /// 槽号是否严格递增。
    pub fn ordered(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.slots.get(i) {
                Some(s) => {
                    if s.idx != i {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1usize;
        }
        true
    }
}

/// 落一本正经的台账。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut i = 0usize;
    while i < CORPUS {
        if let Some(c) = case_at(i) {
            let a = c.actual();
            let s = Sample {
                idx: i,
                tag: c.tag(),
                ok: judge(&c) == expected_fault(i),
                value: a.value,
            };
            l.push(s);
        }
        i += 1usize;
    }
    l.seal();
    l.freeze();
    l
}

/// 落一本"冻结后被写过"的台账：指纹必对不上。
pub fn build_ledger_broken() -> Ledger {
    let mut l = build_ledger();
    l.poke(3usize, 0u32);
    l
}

/// 四注入：倒退闸、封印后写、槽位错序、冻结后涂改。
///
/// 回 4 才说明四个坑都当场被抓住；少一个就说明某条防线是画上去的。
pub fn inject_four() -> u32 {
    let mut caught = 0u32;
    let mut g = Gate::new();
    let first = g.note(Stage::Manifest);
    let second = g.note(Stage::Seal);
    let third = g.note(Stage::Dims);
    if first == 0u32 && second == 0u32 && third == DS_ESTAGE && g.regressions() == 1u32 {
        caught = caught.saturating_add(1u32);
    }
    let mut g2 = Gate::new();
    g2.note(Stage::Seal);
    let closed_at = g2.close();
    let late = g2.note(Stage::Dims);
    if closed_at == 1u32 && late == DS_ESTATE && g2.late_writes() == 1u32 {
        caught = caught.saturating_add(1u32);
    }
    let mut l = Ledger::new();
    let bad = Sample {
        idx: 3usize,
        tag: "out-of-order",
        ok: true,
        value: 1u32,
    };
    let code = l.push(bad);
    if code == DS_ECOUNT && l.len() == 0usize && l.rejected() == 1u32 {
        caught = caught.saturating_add(1u32);
    }
    let broken = build_ledger_broken();
    if !broken.replay() && broken.rejected() > 0u32 && broken.is_frozen() {
        caught = caught.saturating_add(1u32);
    }
    caught
}

// ---------------------------------------------------------------------------
// 十三条判据
// ---------------------------------------------------------------------------

/// 段基址与十三码位次。
pub fn block_ok() -> bool {
    if SEG_BASE != 0x6d430u32 {
        return false;
    }
    if DIMS != 13usize || GATE_COUNT != 8usize || TIER_COUNT != 4usize {
        return false;
    }
    let codes = [
        DS_ESTATE, DS_ESTAGE, DS_ECODE, DS_EVALUE, DS_ENOTE, DS_EDEFAULT, DS_ECOUNT, DS_EUNIQ,
        DS_EDIMS, DS_ESKIP, DS_EFLAKE, DS_ECONF, DS_ESLOT,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        match codes.get(i) {
            Some(c) => {
                if *c != SEG_BASE + (i as u32) + 1u32 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    let mut k = 0usize;
    while k < DIMS {
        match Dim::at(k) {
            Some(d) => {
                if d.rank() != k || d.bit() != (1u16 << k) || d.name().is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        k += 1usize;
    }
    WANT_DIM_ASSERTS == 126u32 && DIM_FULL == 0x1fffu16
}

/// 清单十八条与短名段位。
pub fn roster_ok() -> bool {
    if ROWS != 18usize || !bench_ok() {
        return false;
    }
    if dims_ok_rows() != ROWS as u32 {
        return false;
    }
    let mut i = 0usize;
    while i < ROWS {
        match (SPECS.get(i), row_at(i)) {
            (Some(s), Some(r)) => {
                if s.tag.is_empty() || s.seg == 0u32 {
                    return false;
                }
                if !r.dims_ok || r.dim_count() == 0u32 {
                    return false;
                }
                if r.tier != Tier::Anchored && r.tier != Tier::Declared {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1usize;
    }
    match (spec_index_of(CARD_MIN), spec_index_of(CARD_MAX)) {
        (Some(a), Some(b)) => a == 0usize && b == ROWS - 1usize,
        _ => false,
    }
}

/// 十四格语料与八接六拒。
pub fn corpus_ok() -> bool {
    if CORPUS != ACCEPT + REJECT || CORPUS != 14usize {
        return false;
    }
    if SHAPES.len() != CORPUS || CLAIMS.len() != CORPUS {
        return false;
    }
    if WANT_OK.len() != CORPUS
        || WANT_STAGE.len() != CORPUS
        || WANT_CODE.len() != CORPUS
        || WANT_VALUE.len() != CORPUS
        || WANT_NOTE.len() != CORPUS
        || WANT_DEFAULT.len() != CORPUS
        || WANT_TIER.len() != CORPUS
        || WANT_NDX.len() != CORPUS
        || WANT_FAULT_CODE.len() != CORPUS
    {
        return false;
    }
    if REJECT_FAULTS.len() != REJECT || REJECT_CODES.len() != REJECT {
        return false;
    }
    let mut i = 0usize;
    while i < CORPUS {
        match (case_at(i), SPECS.get(i)) {
            (Some(c), Some(s)) => {
                if c.tag() != s.tag || c.shape.idx != i {
                    return false;
                }
                if c.claim.tag != s.tag {
                    return false;
                }
                if c.shape.conflict != 0u8 && c.shape.conflict as usize > DIMS {
                    return false;
                }
                let mut j = i + 1usize;
                while j < CORPUS {
                    if let Some(d) = claim_at(j) {
                        if d.tag == c.claim.tag {
                            return false;
                        }
                    }
                    j += 1usize;
                }
            }
            _ => return false,
        }
        i += 1usize;
    }
    let mut k = 0usize;
    while k < 8usize {
        match (VALS.get(k), VALS.get(7usize)) {
            (Some(v), Some(t)) => {
                if k == 7usize {
                    if *v != RED_RATE || *t != RED_RATE {
                        return false;
                    }
                } else {
                    let mut m = k + 1usize;
                    while m < 7usize {
                        if let Some(w) = VALS.get(m) {
                            if *w == *v {
                                return false;
                            }
                        }
                        m += 1usize;
                    }
                }
            }
            _ => return false,
        }
        k += 1usize;
    }
    true
}

/// 闸序与六个判维的次序。
pub fn order_ok() -> bool {
    let mut i = 0usize;
    while i < GATE_COUNT {
        match (Stage::at(i), Stage::of_rank(i)) {
            (Some(s), Some(t)) => {
                if s.rank() != i || t != s || s.name().is_empty() {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1usize;
    }
    let mut f = 0usize;
    while f < Fault::all().len() {
        match (Fault::at(f), Fault::all().get(f)) {
            (Some(a), Some(b)) => {
                if a != *b || a.rank() != f || a.name().is_empty() {
                    return false;
                }
                if f != 0usize && a.code() != SEG_BASE + (f as u32) {
                    return false;
                }
                if f == 0usize && a.code() != TAG_OK {
                    return false;
                }
            }
            _ => return false,
        }
        f += 1usize;
    }
    let mut t = 0usize;
    while t < TIER_COUNT {
        match (Tier::at(t), Tier::of_rank(t)) {
            (Some(a), Some(b)) => {
                if a.rank() != t || b != a || a.name().is_empty() {
                    return false;
                }
            }
            _ => return false,
        }
        t += 1usize;
    }
    Stage::Manifest.rank() == 0usize
        && Stage::Seal.rank() == GATE_COUNT - 1usize
        && Fault::Estate.rank() == 1usize
        && Fault::Default.rank() == 6usize
}

/// 定义八闸逐格。
pub fn define_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        match (outcome_at(i), WANT_STAGE.get(i), WANT_CODE.get(i)) {
            (Some(o), Some(st), Some(cd)) => {
                if o.stage != *st || o.code != *cd {
                    return false;
                }
                if o.value != *WANT_VALUE.get(i).unwrap_or(&u32::MAX) {
                    return false;
                }
                if o.note != *WANT_NOTE.get(i).unwrap_or(&Note::MissingRow) {
                    return false;
                }
                if o.default != *WANT_DEFAULT.get(i).unwrap_or(&false) {
                    return false;
                }
                if o.ok() != *WANT_OK.get(i).unwrap_or(&false) {
                    return false;
                }
                if i < ACCEPT && !o.agreed() {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1usize;
    }
    true
}

/// 判定六维次序。
pub fn judge_ok() -> bool {
    let faults = judge_all();
    let mut i = 0usize;
    while i < CORPUS {
        match faults.get(i) {
            Some(f) => {
                if *f != expected_fault(i) {
                    return false;
                }
                if f.code() != *WANT_FAULT_CODE.get(i).unwrap_or(&u32::MAX) {
                    return false;
                }
                match (case_at(i), *f) {
                    (Some(c), Fault::Ok) => {
                        if c.claim.divergence(&c.actual()) != 0u32 {
                            return false;
                        }
                    }
                    (Some(c), other) => {
                        if c.claim.divergence(&c.actual()) == 0u32 {
                            return false;
                        }
                        let ndx = *WANT_NDX.get(i).unwrap_or(&DIM_NONE);
                        match Dim::of_rank(ndx) {
                            Some(d) => {
                                if !c.claim.differs(&c.actual(), d) {
                                    return false;
                                }
                            }
                            None => return false,
                        }
                        if other == Fault::Ok {
                            return false;
                        }
                    }
                    (None, _) => return false,
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    true
}

/// 两机一致与显式冲突。
pub fn agreement_ok() -> bool {
    let mut conflicts = 0u32;
    let mut i = 0usize;
    while i < CORPUS {
        match shape_at(i) {
            Some(s) => {
                if s.has_conflict() {
                    conflicts = conflicts.saturating_add(1u32);
                    if s.conflict_dim() != Some(Dim::StdoutText) {
                        return false;
                    }
                }
                if i < ACCEPT {
                    if !s.both_ok() || s.bytes_a != s.bytes_b || s.byte_gap() != 0u32 {
                        return false;
                    }
                    if s.has_conflict() || s.max_attempts() != 1u8 {
                        return false;
                    }
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    if conflicts != 1u32 {
        return false;
    }
    match (case_at(10usize), shape_at(10usize)) {
        (Some(c), Some(s)) => {
            c.claim.code == DS_ECONF
                && c.claim.value == s.conflict as u32
                && c.claim.note == Note::ConflictShown
        }
        _ => false,
    }
}

/// 形态掩码与覆盖统计。
pub fn dims_ok() -> bool {
    if dim_total() != WANT_DIM_ASSERTS {
        return false;
    }
    if dim_cover() != DIMS as u32 || dim_holes() != 0u32 {
        return false;
    }
    if dim_fragile() != 0u32 || dim_dense() != 0u32 {
        return false;
    }
    if dim_names() != DIMS as u32 {
        return false;
    }
    let mut union = 0u16;
    let mut i = 0usize;
    while i < ROWS {
        match SPECS.get(i) {
            Some(s) => {
                if s.dims == 0u16 || (s.dims & !DIM_FULL) != 0u16 {
                    return false;
                }
                if popcount16(s.dims) != WANT_DIM_FLOOR {
                    return false;
                }
                union |= s.dims;
            }
            None => return false,
        }
        i += 1usize;
    }
    union == DIM_FULL
}

/// 四档分档。
pub fn tiers_ok() -> bool {
    let mut i = 0usize;
    while i < CORPUS {
        match tier_of_index(i) {
            Some(t) => {
                if t != *WANT_TIER.get(i).unwrap_or(&Tier::Void) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1usize;
    }
    let counts = tier_counts();
    let want = [6u32, 2u32, 5u32, 1u32];
    let mut k = 0usize;
    while k < TIER_COUNT {
        match (counts.get(k), want.get(k)) {
            (Some(a), Some(b)) => {
                if *a != *b || tier_count(Tier::at(k).unwrap_or(Tier::Void)) != *b {
                    return false;
                }
            }
            _ => return false,
        }
        k += 1usize;
    }
    let m = Matrix::live();
    m.balanced() && m.anchored_range_ok() && m.indices_ok() && m.total() == CORPUS as u32
}

/// 全跑与定稿渲染。
pub fn ledger_ok() -> bool {
    let r = run();
    if !r.green() || !r.gate() || r.slot_fault() {
        return false;
    }
    if r.rate() != 1000u32 || r.conflicts != 0u32 || r.matched != CORPUS as u32 {
        return false;
    }
    r.render() == RENDER_OK
}

/// 越界覆盖回 ESLOT。
pub fn slot_guard_ok() -> bool {
    let over = run_with(Some((CORPUS, 1u32)));
    if over.slot != DS_ESLOT || over.green() {
        return false;
    }
    let same = run_with(Some((0usize, 640u32)));
    if !same.green() {
        return false;
    }
    let shifted = run_with(Some((0usize, 1u32)));
    if shifted.green() || shifted.conflicts != 1u32 || shifted.matched != 13u32 {
        return false;
    }
    let late = run_with(Some((12usize, 1u32)));
    if late.matched >= CORPUS as u32 || late.conflicts == 0u32 {
        return false;
    }
    let plain = run();
    plain.render() == RENDER_OK && over.cells == plain.cells
}

/// 封印冻结重放。
pub fn seal_ok() -> bool {
    let l = build_ledger();
    if !l.full() || !l.all_green() || !l.ordered() {
        return false;
    }
    if !l.is_sealed() || !l.is_frozen() || !l.replay() || l.rejected() != 0u32 {
        return false;
    }
    if l.audit() != SAMPLES as u32 || l.stamp() == 0u64 || l.root() != l.stamp() {
        return false;
    }
    let broken = build_ledger_broken();
    if broken.replay() || broken.rejected() == 0u32 {
        return false;
    }
    let mut fresh = Ledger::new();
    if fresh.seal() != DS_ECOUNT || fresh.freeze() != DS_ESTATE {
        return false;
    }
    let g = Gate::walk();
    g.complete() && g.monotone() && g.one_each() && g.quiet_after_close() && g.is_closed()
}

/// 冻结重放四注入。
pub fn inject_ok() -> bool {
    if inject_four() != 4u32 {
        return false;
    }
    let mut g = Gate::new();
    let _ = g.note(Stage::Manifest);
    let _ = g.note(Stage::Ready);
    if g.note(Stage::Dims) != DS_ESTAGE {
        return false;
    }
    let mut h = Gate::new();
    let _ = h.note(Stage::Seal);
    let _ = h.close();
    if h.note(Stage::Seal) != DS_ESTATE {
        return false;
    }
    let mut probe = Ledger::new();
    let ok = probe.push(Sample {
        idx: 0usize,
        tag: "f11681-enospc-master",
        ok: true,
        value: RED_RATE,
    });
    ok == 0u32 && probe.len() == 1usize && probe.rejected() == 0u32
}

/// 十三项判据。
pub fn run_unx11699_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11699_b25_dual_snapshot");
    s.add("DS01-段基址与十三码位次", block_ok(), "基址与码位次逐位对齐");
    s.add("DS02-清单十八条与短名段位", roster_ok(), "十八条身份证与段位");
    s.add("DS03-十四格语料与八接六拒", corpus_ok(), "语料格数与申报对齐");
    s.add("DS04-闸序与判维次序", order_ok(), "八闸六维四档的次序");
    s.add("DS05-定义八闸逐格", define_ok(), "逐格定义结果");
    s.add("DS06-判定六维次序", judge_ok(), "六维判定次序");
    s.add("DS07-两机一致与显式冲突", agreement_ok(), "双机一致率与冲突记账");
    s.add("DS08-形态掩码与覆盖统计", dims_ok(), "掩码覆盖统计");
    s.add("DS09-四档分档", tiers_ok(), "四档分档与矩阵");
    s.add("DS10-全跑与定稿渲染", ledger_ok(), "全跑渲染逐字节");
    s.add("DS11-越界覆盖回 ESLOT", slot_guard_ok(), "槽位越界防护");
    s.add("DS12-封印冻结重放", seal_ok(), "封印冻结重放");
    s.add("DS13-冻结重放四注入", inject_ok(), "四注入全中");
    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_seg_base() {
        assert_eq!(SEG_BASE, 0x6d430u32);
    }

    #[test]
    fn t02_codes_contiguous() {
        assert_eq!(DS_ESTATE, SEG_BASE + 1u32);
        assert_eq!(DS_ESLOT, SEG_BASE + 0x0du32);
        assert_eq!(DS_ECONF, SEG_BASE + 0x0cu32);
    }

    #[test]
    fn t03_block_ok() {
        assert!(block_ok());
    }

    #[test]
    fn t04_dims_table() {
        assert_eq!(DIMS, 13usize);
        assert_eq!(dim_names(), 13u32);
        assert_eq!(Dim::ExitCode.bit(), 1u16);
        assert_eq!(Dim::OrderPrint.bit(), 1u16 << 12);
    }

    #[test]
    fn t05_stage_order() {
        let mut i = 0usize;
        while i < GATE_COUNT {
            let s = Stage::at(i).unwrap();
            assert_eq!(s.rank(), i);
            assert_eq!(Stage::of_rank(i), Some(s));
            i += 1usize;
        }
        assert!(Stage::Manifest.rank() < Stage::Seal.rank());
    }

    #[test]
    fn t06_stage_names() {
        assert_eq!(Stage::Manifest.name(), "manifest");
        assert_eq!(Stage::Seal.name(), "seal");
        assert_eq!(Stage::Tally.rank(), 6usize);
    }

    #[test]
    fn t07_fault_order() {
        assert_eq!(Fault::all().len(), 7usize);
        assert_eq!(Fault::Ok.rank(), 0usize);
        assert_eq!(Fault::Default.rank(), 6usize);
        assert_eq!(Fault::Stage.code(), DS_ESTAGE);
    }

    #[test]
    fn t08_tier_order() {
        assert_eq!(Tier::all().len(), TIER_COUNT);
        assert_eq!(Tier::Anchored.rank(), 0usize);
        assert_eq!(Tier::Void.rank(), 3usize);
        assert_eq!(Tier::of_rank(2usize), Some(Tier::Unrun));
        assert_eq!(Tier::Anchored.name(), "anchored");
    }

    #[test]
    fn t09_harness_roundtrip() {
        let mut i = 0usize;
        while i < 6usize {
            let h = Harness::at(i).unwrap();
            assert_eq!(h.rank(), i);
            assert_eq!(Harness::of_rank(i), Some(h));
            i += 1usize;
        }
        assert!(Harness::Both.needs_a());
        assert!(Harness::Both.needs_b());
    }

    #[test]
    fn t10_note_green_only_sealed() {
        assert!(Note::Sealed.is_green());
        assert!(!Note::CfgMissing.is_green());
        assert!(!Note::BytesDiffer.is_green());
        assert_eq!(Note::all().len(), 10usize);
    }

    #[test]
    fn t11_roster() {
        assert!(roster_ok());
        assert_eq!(SPECS.len(), 18usize);
        assert_eq!(default_count(), 15u32);
    }

    #[test]
    fn t12_spec_lookup() {
        assert_eq!(spec_index_of(11681u16), Some(0usize));
        assert_eq!(spec_index_of(11698u16), Some(ROWS - 1usize));
        assert_eq!(spec_index_of(11500u16), None);
        assert_eq!(spec_at(0usize).unwrap().seg, 0x6d130u32);
        assert_eq!(spec_at(17usize).unwrap().seg, 0x6d420u32);
    }

    #[test]
    fn t13_corpus_sizes() {
        assert!(corpus_ok());
        assert_eq!(SHAPES.len(), 14usize);
        assert_eq!(CLAIMS.len(), 14usize);
        assert_eq!(ACCEPT + REJECT, CORPUS);
    }

    #[test]
    fn t14_vals_tail() {
        assert_eq!(VALS.len(), 8usize);
        assert_eq!(VALS.get(7usize), Some(&RED_RATE));
        assert_eq!(VALS.get(0usize), Some(&640u32));
        assert_ne!(VALS.get(6usize), VALS.get(7usize));
    }

    #[test]
    fn t15_define_accepts() {
        let mut i = 0usize;
        while i < ACCEPT {
            let o = outcome_at(i).unwrap();
            assert_eq!(o.stage, Stage::Seal);
            assert!(o.ok());
            assert_eq!(o.note, Note::Sealed);
            assert_eq!(o.value, *WANT_VALUE.get(i).unwrap());
            i += 1usize;
        }
    }

    #[test]
    fn t16_define_ok() {
        assert!(define_ok());
    }

    #[test]
    fn t17_define_slot8_flake() {
        let o = outcome_at(8usize).unwrap();
        assert_eq!(o.stage, Stage::Retry);
        assert_eq!(o.code, DS_EFLAKE);
        assert_eq!(o.value, 3u32);
        assert_eq!(o.note, Note::FlakeOnce);
    }

    #[test]
    fn t18_define_slot9_packed() {
        let o = outcome_at(9usize).unwrap();
        assert_eq!(o.stage, Stage::Exec);
        assert_eq!(o.value, 256u32);
        assert_eq!(o.note, Note::ExitBad);
    }

    #[test]
    fn t19_define_slot11_mask() {
        let o = outcome_at(11usize).unwrap();
        assert_eq!(o.stage, Stage::Dims);
        assert_eq!(o.code, DS_EDIMS);
        assert_eq!(o.value, 0u32);
        assert!(!shape_at(11usize).unwrap().mask_ok());
    }

    #[test]
    fn t20_define_slot12_bytes() {
        let s = shape_at(12usize).unwrap();
        let o = outcome_at(12usize).unwrap();
        assert_eq!(o.stage, Stage::Tally);
        assert_eq!(o.note, Note::BytesDiffer);
        assert_eq!(o.value, 256u32);
        assert_eq!(s.byte_gap(), 256u32);
    }

    #[test]
    fn t21_define_slot13_skip() {
        let s = shape_at(13usize).unwrap();
        let o = outcome_at(13usize).unwrap();
        assert_eq!(o.stage, Stage::Ready);
        assert_eq!(o.code, DS_ESKIP);
        assert_eq!(o.note, Note::CfgMissing);
        assert!(!ready(&s, s.cfg));
    }

    #[test]
    fn t22_judge_accepts() {
        let mut i = 0usize;
        while i < ACCEPT {
            assert_eq!(judge(&case(i)), Fault::Ok);
            i += 1usize;
        }
    }

    #[test]
    fn t23_judge_codes() {
        assert!(judge_ok());
        assert_eq!(expected_fault(9usize).code(), DS_EVALUE);
        assert_eq!(expected_fault(12usize).code(), DS_EDEFAULT);
    }

    #[test]
    fn t24_divergence_dim() {
        let mut i = ACCEPT;
        while i < CORPUS {
            let c = case(i);
            let a = c.actual();
            let ndx = *WANT_NDX.get(i).unwrap();
            assert_ne!(ndx, DIM_NONE);
            let d = Dim::of_rank(ndx).unwrap();
            assert!(c.claim.differs(&a, d));
            i += 1usize;
        }
    }

    #[test]
    fn t25_tier_anchored() {
        assert_eq!(tier_count(Tier::Anchored), 6u32);
        assert_eq!(tier_of_index(0usize), Some(Tier::Anchored));
    }

    #[test]
    fn t26_tier_declared() {
        assert_eq!(tier_count(Tier::Declared), 2u32);
        assert_eq!(tier_of_index(6usize), Some(Tier::Declared));
        assert_eq!(tier_of_index(7usize), Some(Tier::Declared));
    }

    #[test]
    fn t27_tier_void_and_unrun() {
        assert_eq!(tier_count(Tier::Void), 1u32);
        assert_eq!(tier_of_index(13usize), Some(Tier::Void));
        assert_eq!(tier_count(Tier::Unrun), 5u32);
        assert_eq!(tier_of_index(12usize), Some(Tier::Unrun));
    }

    #[test]
    fn t28_matrix_balanced() {
        let m = Matrix::live();
        assert!(m.balanced());
        assert!(m.anchored_range_ok());
        assert!(m.indices_ok());
        assert_eq!(m.total(), 14u32);
        assert_eq!(m.at(0usize) + m.at(1usize) + m.at(2usize) + m.at(3usize), 14u32);
        assert_eq!(Matrix::EMPTY.total(), 0u32);
    }

    #[test]
    fn t29_dim_stats() {
        assert_eq!(dim_total(), 126u32);
        assert_eq!(dim_cover(), 13u32);
        assert_eq!(dim_holes(), 0u32);
        assert_eq!(dim_fragile(), 0u32);
        assert_eq!(dim_dense(), 0u32);
        assert_eq!(dims_ok_rows(), 18u32);
    }

    #[test]
    fn t30_run_green() {
        let r = run();
        assert!(r.green());
        assert_eq!(r.rate(), 1000u32);
        assert_eq!(r.conflicts, 0u32);
        assert_eq!(r.matched, 14u32);
        assert_eq!(r.cells, 126u32);
        assert!(r.gate());
        assert!(!r.slot_fault());
    }

    #[test]
    fn t31_render_pinned() {
        assert_eq!(run().render(), RENDER_OK);
        assert!(RENDER_OK.contains("dual=DS99"));
        assert!(RENDER_OK.contains("green=true"));
    }

    #[test]
    fn t32_run_with_override() {
        let same = run_with(Some((0usize, 640u32)));
        assert!(same.green());
        let moved = run_with(Some((0usize, 1u32)));
        assert!(!moved.green());
        assert_eq!(moved.conflicts, 1u32);
        assert_eq!(moved.matched, 13u32);
        assert_eq!(moved.agree, 126u32 - WANT_DIM_FLOOR);
    }

    #[test]
    fn t33_run_with_out_of_range() {
        let r = run_with(Some((CORPUS, 7u32)));
        assert_eq!(r.slot, DS_ESLOT);
        assert!(!r.green());
        assert_eq!(r.cells, 126u32);
        assert_eq!(r.conflicts, 0u32);
    }

    #[test]
    fn t34_gate_walk() {
        let g = Gate::walk();
        assert!(g.complete());
        assert!(g.monotone());
        assert!(g.one_each());
        assert!(g.quiet_after_close());
        assert_eq!(g.seen_count(), 8u32);
        assert_eq!(g.step_count(), 8u32);
        assert_eq!(g.top_rank(), 7usize);
    }

    #[test]
    fn t35_gate_regression() {
        let mut g = Gate::new();
        assert_eq!(g.note(Stage::Manifest), 0u32);
        assert_eq!(g.note(Stage::Seal), 0u32);
        assert_eq!(g.note(Stage::Dims), DS_ESTAGE);
        assert_eq!(g.regressions(), 1u32);
        assert!(!g.monotone());
        assert_eq!(g.note(Stage::Seal), DS_ESTAGE);
        assert_eq!(g.regressions(), 2u32);
    }

    #[test]
    fn t36_gate_late_write() {
        let mut g = Gate::new();
        assert_eq!(g.note(Stage::Seal), 0u32);
        assert_eq!(g.close(), 1u32);
        assert_eq!(g.note(Stage::Dims), DS_ESTATE);
        assert_eq!(g.late_writes(), 1u32);
        assert!(g.is_closed());
        assert_eq!(g.close(), DS_ESTATE);
    }

    #[test]
    fn t37_ledger_full() {
        let l = build_ledger();
        assert!(l.full());
        assert!(l.all_green());
        assert!(l.ordered());
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.audit(), 14u32);
        assert_eq!(l.rejected(), 0u32);
        assert_eq!(l.sample_at(0usize).unwrap().tag, "f11681-enospc-master");
        assert_eq!(l.sample_at(13usize).unwrap().value, 0u32);
    }

    #[test]
    fn t38_ledger_replay() {
        let l = build_ledger();
        assert!(l.is_sealed());
        assert!(l.is_frozen());
        assert!(l.replay());
        assert_ne!(l.stamp(), 0u64);
        assert_eq!(l.root(), l.stamp());
    }

    #[test]
    fn t39_ledger_broken() {
        // 收库修正（AI-INTAKE-1107）：poke 需 &mut，b 声明为 mut。
        let mut b = build_ledger_broken();
        assert!(b.is_frozen());
        assert!(!b.replay());
        assert!(b.rejected() > 0u32);
        assert_eq!(b.poke(0usize, 1u32), DS_ECONF);
    }

    #[test]
    fn t40_ledger_order_guard() {
        let mut l = Ledger::new();
        let bad = Sample {
            idx: 2usize,
            tag: "x",
            ok: true,
            value: 0u32,
        };
        assert_eq!(l.push(bad), DS_ECOUNT);
        assert_eq!(l.len(), 0usize);
        assert_eq!(l.rejected(), 1u32);
        let mut fresh = Ledger::new();
        assert_eq!(fresh.seal(), DS_ECOUNT);
        assert_eq!(fresh.freeze(), DS_ESTATE);
        assert_eq!(fresh.poke(0usize, 1u32), 0u32);
        assert_eq!(fresh.poke(99usize, 1u32), DS_ESLOT);
    }

    #[test]
    fn t41_inject_four() {
        assert_eq!(inject_four(), 4u32);
        assert!(inject_ok());
    }

    #[test]
    fn t42_fold_stable() {
        let mut a = FNV_OFF;
        a = fold_str(a, "f11699");
        let mut b = FNV_OFF;
        b = fold_str(b, "f11699");
        assert_eq!(a, b);
        assert_ne!(a, FNV_OFF);
        assert_eq!(fold_byte(FNV_OFF, 0u8), FNV_OFF.wrapping_mul(FNV_PRIME));
    }

    #[test]
    fn t43_text_helpers() {
        let mut out = String::new();
        dec_text(126u32, &mut out);
        assert_eq!(out.as_str(), "126");
        let mut h = String::new();
        hex_text(0x6d43u32, &mut h);
        assert_eq!(h.as_str(), "6d43");
        let mut p = String::new();
        permille_text(126u32, 126u32, &mut p);
        assert_eq!(p.as_str(), "1000");
        let mut n = String::new();
        kv_num("cells", 126u32, &mut n);
        assert_eq!(n.as_str(), "cells=126");
    }

    #[test]
    fn t44_checks_all_green() {
        let s = run_unx11699_checks();
        assert_eq!(s.len(), 13usize);
        assert!(s.all_passed());
        assert_eq!(s.red_items_text().as_str(), "");
    }
}

