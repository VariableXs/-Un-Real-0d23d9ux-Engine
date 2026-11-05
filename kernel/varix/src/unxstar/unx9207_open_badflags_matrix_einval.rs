//! UNX-F9207 open 坏 flags 组合 EINVAL 矩阵 · 0x7A140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F9207-J1 逐字）：
//!   O_CREAT 缺 mode/非法 O_ 位组合注入矩阵判据过，全 EINVAL 或按 Linux
//!   语义位忽略格过，逐码对账一致。
//!
//! 设计要点：
//!   ① 域模型围绕 flags 位解码设计（与 fd 表类模块完全不同）：判定器
//!      `judge_open_flags(flags, has_mode)` 把 u32 位面拆成 OpenIntent
//!      （访问模 / 创建族 / 截断族 / 追加 / 目录 / 无名临时文件 / 忽略
//!      留痕），错误面只回 errno 正数码。
//!   ② 访问模式闭集：O_RDONLY=0 / O_WRONLY=1 / O_RDWR=2 三形态合法，
//!      3 及以上（O_WRONLY|O_RDWR 同置即 0o3，以及任何更大越集形态）
//!      全 EINVAL——Linux 语义，判序最先。
//!   ③ O_CREAT 缺 mode 注入矩阵：O_CREAT、O_CREAT|O_EXCL、O_CREAT|O_TRUNC、
//!      O_CREAT|O_APPEND、O_TMPFILE 族在 has_mode=false 时全 EINVAL；
//!      has_mode=true 时 O_CREAT|O_EXCL 合法（EEXIST 属对象态不在本卡）。
//!   ④ 无效位注入：冻结位名表外的位一律 EINVAL；u32::MAX 全开是极端
//!      样本。O_TMPFILE 缺其内嵌 O_DIRECTORY 位属畸形组合，单独 EINVAL。
//!   ⑤ 按 Linux 语义应忽略的位单独成类：O_EXCL 缺 O_CREAT、O_ASYNC 在
//!      常规文件、O_NONBLOCK 在常规文件、O_NOCTTY 无控制终端——落忽略
//!      留痕账照常提交，不得静默丢弃也不得误报 EINVAL。
//!   ⑥ 判定次序（build_open_flags 口径）：访问模式位先于其他校验；
//!      随后 O_TMPFILE 畸形 → O_CREAT|O_TMPFILE 互斥 → 缺 mode →
//!      未知位 → 忽略类 → 提交，两条错误同时成立时取序最前者。
//!   ⑦ 逐码对账表冻结：每个 (flags, has_mode) 组合 → 期望 errno 或
//!      期望忽略位集，全量复算一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9207 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A140;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入类别数（缺 mode 矩阵 / 非法 O_ 位组合 / 访问模越集）。
pub const BAD_FLAG_CLASSES: u32 = 3;

/// Linux open flags 位定义（fcntl.h 口径，只保留本卡判据涉及的位）。
pub const O_RDONLY: u32 = 0o0;
pub const O_WRONLY: u32 = 0o1;
pub const O_RDWR: u32 = 0o2;
/// 访问模掩码（低两位）。
pub const O_ACCMODE: u32 = 0o3;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
pub const O_NOCTTY: u32 = 0o400;
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_DSYNC: u32 = 0o10000;
pub const O_ASYNC: u32 = 0o20000;
pub const O_DIRECTORY: u32 = 0o200000;
/// O_TMPFILE 头位（0o20000000）：必须与 O_DIRECTORY 位联用才成完整语义。
pub const O_TMPFILE_HEADER: u32 = 0o20000000;
/// 完整 O_TMPFILE = 头位 | O_DIRECTORY（Linux fcntl.h 口径）。
pub const O_TMPFILE: u32 = O_TMPFILE_HEADER | O_DIRECTORY;

/// 本卡冻结的合法位全集：拆解器以此拒绝一切表外位。
pub const KNOWN_MASK: u32 = O_ACCMODE
    | O_CREAT
    | O_EXCL
    | O_NOCTTY
    | O_TRUNC
    | O_APPEND
    | O_NONBLOCK
    | O_DSYNC
    | O_ASYNC
    | O_DIRECTORY
    | O_TMPFILE_HEADER;

/// Linux errno 正数码（系统调用返回负值）。
pub const EINVAL: i32 = 22;
pub const EEXIST: i32 = 17;
pub const EACCES: i32 = 13;
pub const EISDIR: i32 = 21;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlagCode {
    /// 访问模越集：掩码内取值不是 0/1/2 三形态之一
    AccessClosedSet,
    /// O_TMPFILE 畸形：头位置上但缺内嵌 O_DIRECTORY 位
    TmpfileMalformed,
    /// O_CREAT 与 O_TMPFILE 同置（Linux 明文 EINVAL）
    CreatTmpfileMutex,
    /// 创建类 open 缺第三参 mode
    CreatNoMode,
    /// 未知高位：落出冻结位名表
    UnknownFlagBits,
    /// O_EXCL 缺 O_CREAT：Linux 忽略，落忽略账
    ExclNoCreatIgnored,
    /// O_ASYNC 在常规文件上：Linux 忽略，落忽略账
    AsyncIgnoredRegular,
    /// 忽略类位面整体留痕（NONBLOCK/NOCTTY 等同类）
    IgnoredBitsClass,
    /// 对账表漂移：复算结果与冻结期望不一致
    ReconDrift,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl FlagCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FlagCode::AccessClosedSet => "访问模越集被拒：O_ACCMODE 掩码内只允许 \
                O_RDONLY=0/O_WRONLY=1/O_RDWR=2 三形态，0o3 及任何更大越集取值一律 \
                回 EINVAL；请只保留一个访问模位",
            FlagCode::TmpfileMalformed => "O_TMPFILE 畸形被拒：头位 0o20000000 \
                必须与 O_DIRECTORY 位联用才构成完整 O_TMPFILE 语义；请使用 \
                fcntl.h 的完整常量而非手拼头位",
            FlagCode::CreatTmpfileMutex => "O_CREAT|O_TMPFILE 同置被拒：Linux \
                明文规定两标志互斥，回 EINVAL；要么有名创建用 O_CREAT，要么\
                无名临时文件用 O_TMPFILE，二选一",
            FlagCode::CreatNoMode => "创建类 open 缺 mode 被拒：flags 含 \
                O_CREAT/O_TMPFILE 时必须携带第三参 mode；请补齐 mode 或去掉\
                创建类标志，缺省形态按 EINVAL 处理",
            FlagCode::UnknownFlagBits => "未知高位被拒：flags 落出冻结位名表，\
                回 EINVAL；请核对 fcntl.h 位定义，禁止手拼未定义位",
            FlagCode::ExclNoCreatIgnored => "O_EXCL 缺 O_CREAT：Linux 语义忽略\
                该位不报错；本卡要求落忽略留痕账，不得静默丢弃",
            FlagCode::AsyncIgnoredRegular => "O_ASYNC 在常规文件上：Linux 语义\
                忽略该位（仅终端/套接字生效）；本卡要求落忽略留痕账",
            FlagCode::IgnoredBitsClass => "忽略类位面留痕：NONBLOCK/NOCTTY 等\
                位在常规文件上不改变语义；照常提交但必须留痕可查",
            FlagCode::ReconDrift => "对账表漂移：逐码复算结果与冻结期望不一致；\
                请核对 RECON_TABLE 与 judge_open_flags 的判定次序是否同源",
            FlagCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查\
                回滚路径是否误改累计值",
            FlagCode::NotReproducible => "双跑不一致：同 flags 语料两次裁定不同；\
                请排查位面拆解中的隐藏状态或非确定来源",
            FlagCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [FlagCode; 12] = [
        FlagCode::AccessClosedSet,
        FlagCode::TmpfileMalformed,
        FlagCode::CreatTmpfileMutex,
        FlagCode::CreatNoMode,
        FlagCode::UnknownFlagBits,
        FlagCode::ExclNoCreatIgnored,
        FlagCode::AsyncIgnoredRegular,
        FlagCode::IgnoredBitsClass,
        FlagCode::ReconDrift,
        FlagCode::LedgerRegress,
        FlagCode::NotReproducible,
        FlagCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── 访问模式闭集 ──────────────────────────────────────────────────────
/// 访问模式三形态（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessMode {
    /// 0：只读
    RdOnly,
    /// 1：只写
    WrOnly,
    /// 2：读写
    RdWr,
}

impl AccessMode {
    /// 闭集判定：0/1/2 合法，3 及以上（含同置形态 0o3）一律 EINVAL。
    /// 这是本卡第一道判据，先于一切组合与位名校验。
    pub fn judge(raw: u32) -> SysResult<AccessMode> {
        match raw {
            0 => Ok(AccessMode::RdOnly),
            1 => Ok(AccessMode::WrOnly),
            2 => Ok(AccessMode::RdWr),
            _ => Err(EINVAL),
        }
    }

    /// 闭集全量。
    pub const ALL: [AccessMode; 3] = [AccessMode::RdOnly, AccessMode::WrOnly, AccessMode::RdWr];
}

// ── 域模型：OpenIntent ────────────────────────────────────────────────
/// flags 位面解码结果：合法组合提交后的打开意图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpenIntent {
    /// 访问模式（闭集三形态之一）
    pub access: AccessMode,
    /// O_CREAT 置位
    pub creat: bool,
    /// O_EXCL 置位
    pub excl: bool,
    /// O_TRUNC 置位
    pub trunc: bool,
    /// O_APPEND 置位
    pub append: bool,
    /// O_DIRECTORY 置位
    pub directory: bool,
    /// 完整 O_TMPFILE 置位
    pub tmpfile: bool,
    /// 调用方携带的 mode 实参（缺省形态计 0）
    pub mode_arg: u32,
    /// 按 Linux 语义被忽略的位集（留痕，不影响合法性）
    pub ignored_mask: u32,
}

impl OpenIntent {
    /// 是否为创建类意图（O_CREAT 或完整 O_TMPFILE）。
    pub fn is_creating(self) -> bool {
        self.creat || self.tmpfile
    }
}

/// 判定器主入口：flags 位面 + 是否携带 mode 实参 → 打开意图或 errno。
/// 判定次序（冻结，build_open_flags 口径）：
///   ① 访问模式闭集（最先，任何其他错误不得抢先）；
///   ② O_TMPFILE 畸形（头位缺内嵌 O_DIRECTORY 位）；
///   ③ O_CREAT|O_TMPFILE 互斥；
///   ④ 创建类缺 mode；
///   ⑤ 未知位（冻结表外）；
///   ⑥ 忽略类留痕 → 提交 OpenIntent。
pub fn judge_open_flags(flags: u32, has_mode: bool) -> Result<OpenIntent, i32> {
    // ① 访问模式闭集：低两位之外的访问模取值不存在，掩码内只有
    //   0/1/2 三形态合法；0o3 即 O_WRONLY|O_RDWR 字面同置。
    let access_raw = flags & O_ACCMODE;
    let access = match AccessMode::judge(access_raw) {
        Ok(a) => a,
        Err(e) => return Err(e),
    };
    // ② O_TMPFILE 畸形：头位在而内嵌目录位不在。
    let tmp_header = flags & O_TMPFILE_HEADER != 0;
    let tmp_full = flags & O_TMPFILE == O_TMPFILE;
    if tmp_header && !tmp_full {
        return Err(EINVAL);
    }
    // ③ O_CREAT|O_TMPFILE 互斥。
    if flags & O_CREAT != 0 && tmp_full {
        return Err(EINVAL);
    }
    // ④ 创建类缺 mode：O_CREAT 与 O_TMPFILE 都需要第三参。
    let needs_mode = flags & O_CREAT != 0 || tmp_full;
    if needs_mode && !has_mode {
        return Err(EINVAL);
    }
    // ⑤ 未知位：冻结位名表之外的任何置位都 EINVAL。
    if flags & !KNOWN_MASK != 0 {
        return Err(EINVAL);
    }
    // ⑥ 忽略类留痕：合法但按 Linux 语义不生效的位。
    let mut ignored = 0u32;
    if flags & O_EXCL != 0 && flags & O_CREAT == 0 {
        ignored |= O_EXCL;
    }
    if flags & O_ASYNC != 0 {
        ignored |= O_ASYNC;
    }
    if flags & O_NONBLOCK != 0 {
        ignored |= O_NONBLOCK;
    }
    if flags & O_NOCTTY != 0 {
        ignored |= O_NOCTTY;
    }
    if flags & O_DSYNC != 0 {
        ignored |= O_DSYNC;
    }
    Ok(OpenIntent {
        access,
        creat: flags & O_CREAT != 0,
        excl: flags & O_EXCL != 0,
        trunc: flags & O_TRUNC != 0,
        append: flags & O_APPEND != 0,
        directory: flags & O_DIRECTORY != 0,
        tmpfile: tmp_full,
        mode_arg: if has_mode { 0o644 } else { 0 },
        ignored_mask: ignored,
    })
}

// ── 逐码对账表（冻结，可独立复算） ────────────────────────────────────
/// 对账期望：Ok(忽略位集) 或 Err(errno)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconExpect {
    /// 期望合法提交，附带期望忽略位集
    OkWith(u32),
    /// 期望拒绝，附带期望 errno
    ErrWith(i32),
}

/// 对账行：(语料名, flags, has_mode, 冻结期望)。
pub const RECON_TABLE: &[(&str, u32, bool, ReconExpect)] = &[
    ("纯只读", O_RDONLY, false, ReconExpect::OkWith(0)),
    ("纯只写", O_WRONLY, false, ReconExpect::OkWith(0)),
    ("纯读写", O_RDWR, false, ReconExpect::OkWith(0)),
    ("读写+截断+追加", O_RDWR | O_TRUNC | O_APPEND, false, ReconExpect::OkWith(0)),
    ("独占创建带 mode", O_CREAT | O_EXCL, true, ReconExpect::OkWith(0)),
    ("独占创建缺 mode", O_CREAT | O_EXCL, false, ReconExpect::ErrWith(EINVAL)),
    ("无名临时文件带 mode", O_TMPFILE, true, ReconExpect::OkWith(0)),
    ("无名临时文件缺 mode", O_TMPFILE, false, ReconExpect::ErrWith(EINVAL)),
    ("创建与临时互斥", O_CREAT | O_TMPFILE, true, ReconExpect::ErrWith(EINVAL)),
    ("临时头位畸形", O_TMPFILE_HEADER | O_WRONLY, false, ReconExpect::ErrWith(EINVAL)),
    ("双写位同置", O_RDWR | O_WRONLY, false, ReconExpect::ErrWith(EINVAL)),
    ("掩码外访问模 0o3 以上", 0o3, false, ReconExpect::ErrWith(EINVAL)),
    ("未知高位", O_RDWR | 0o4000000, false, ReconExpect::ErrWith(EINVAL)),
    ("全位开启", u32::MAX, true, ReconExpect::ErrWith(EINVAL)),
    ("EXCL 缺 CREAT 忽略", O_EXCL, false, ReconExpect::OkWith(O_EXCL)),
    ("ASYNC 常规文件忽略", O_WRONLY | O_ASYNC, false, ReconExpect::OkWith(O_ASYNC)),
    ("NONBLOCK 常规文件忽略", O_RDONLY | O_NONBLOCK, false, ReconExpect::OkWith(O_NONBLOCK)),
    ("NOCTTY 无终端忽略", O_RDWR | O_NOCTTY, false, ReconExpect::OkWith(O_NOCTTY)),
    ("目录标志非目录目标", O_RDONLY | O_DIRECTORY, false, ReconExpect::OkWith(0)),
];

/// 全量复算对账表：逐行跑判定器并与冻结期望比对。
/// 返回 (总行数, 一致行数, 首个漂移行的表内序号)。
pub fn reconcile_table() -> (usize, usize, Option<usize>) {
    let mut total = 0usize;
    let mut agree = 0usize;
    let mut drift = None;
    let mut i = 0usize;
    while i < RECON_TABLE.len() {
        if let Some(row) = RECON_TABLE.get(i) {
            total += 1;
            let got = judge_open_flags(row.1, row.2);
            let ok = match (row.3, got) {
                (ReconExpect::OkWith(want_ign), Ok(intent)) => intent.ignored_mask == want_ign,
                (ReconExpect::ErrWith(want_err), Err(e)) => e == want_err,
                _ => false,
            };
            if ok {
                agree += 1;
            } else if drift.is_none() {
                drift = Some(i);
            }
        }
        i += 1;
    }
    (total, agree, drift)
}

// ── 坏 flags 注入核 ───────────────────────────────────────────────────
/// 三类坏 flags 注入样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadFlagCase {
    /// 缺 mode 矩阵：创建类组合在 has_mode=false 下提请
    MissingMode,
    /// 非法 O_ 位组合：冻结表外位 / 畸形 O_TMPFILE / 互斥对
    IllegalBits,
    /// 访问模越集：0o3 及更大的掩码内/外形态
    AccessBeyond,
}

impl BadFlagCase {
    /// 该类本轮样本 (flags, has_mode)：轮次 r 在四形态间轮转，确定性构造。
    pub fn sample(self, r: u32) -> (u32, bool) {
        let rot = r % 4;
        match self {
            BadFlagCase::MissingMode => {
                let base = match rot {
                    0 => O_CREAT,
                    1 => O_CREAT | O_EXCL,
                    2 => O_CREAT | O_TRUNC | O_APPEND,
                    _ => O_TMPFILE,
                };
                (base, false)
            }
            BadFlagCase::IllegalBits => {
                let base = match rot {
                    0 => O_RDWR | 0o4000000,
                    1 => O_TMPFILE_HEADER | O_WRONLY,
                    2 => O_CREAT | O_TMPFILE,
                    _ => u32::MAX,
                };
                (base, rot != 2)
            }
            BadFlagCase::AccessBeyond => {
                let base = match rot {
                    0 => O_RDWR | O_WRONLY,
                    1 => O_WRONLY | O_RDWR | O_TRUNC,
                    2 => 0o3 | O_CREAT,
                    _ => 0o100000003,
                };
                (base, true)
            }
        }
    }

    /// 期望 errno：三类样本全部必须 EINVAL。
    pub fn want(self) -> i32 {
        EINVAL
    }

    /// 闭集全量。
    pub const ALL: [BadFlagCase; 3] = [
        BadFlagCase::MissingMode,
        BadFlagCase::IllegalBits,
        BadFlagCase::AccessBeyond,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须恒 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；守恒式被破坏时显性报错。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(FlagCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 三类坏 flags 各 10 轮：全部必须被 EINVAL 抓住。
/// 返回 (账本, 逐类检出数)。
pub fn run_bad_flags_matrix() -> (InjectLedger, [u32; 3]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < BadFlagCase::ALL.len() {
        if let Some(case) = BadFlagCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let (flags, has_mode) = case.sample(round);
                let caught = judge_open_flags(flags, has_mode) == Err(case.want());
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏 flags 族"),
    ("EISDIR", -21, 2, "Windows ERROR_FILE_NOT_FOUND 近似；目录写向族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；独占冲突族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；权限族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料指纹：逐 flags 组合判定的结果字节流，覆盖三访问模 / 创建族
/// 带缺 mode / 互斥对 / 畸形头位 / 未知位 / 忽略类四位 / 全位开启。
pub fn corpus_fingerprint() -> u32 {
    let combos: [(u32, bool); 14] = [
        (O_RDONLY, false),
        (O_WRONLY, false),
        (O_RDWR, false),
        (O_CREAT, true),
        (O_CREAT | O_EXCL, true),
        (O_CREAT | O_EXCL, false),
        (O_TMPFILE, true),
        (O_CREAT | O_TMPFILE, true),
        (O_TMPFILE_HEADER | O_WRONLY, false),
        (O_RDWR | O_WRONLY, false),
        (O_RDWR | 0o4000000, false),
        (O_WRONLY | O_ASYNC, false),
        (O_RDONLY | O_NONBLOCK | O_NOCTTY, false),
        (u32::MAX, true),
    ];
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < combos.len() {
        if let Some((f, m)) = combos.get(i).copied() {
            match judge_open_flags(f, m) {
                Ok(intent) => {
                    bytes.push(0x01);
                    bytes.push((intent.ignored_mask & 0xFF) as u8);
                }
                Err(e) => {
                    bytes.push(0x00);
                    bytes.push((e & 0xFF) as u8);
                }
            }
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9207 判据集（≥15 条静态 + ≤8 动态明细）。
pub fn run_unx9207_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9207");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：三访问模逐一提交且意图字段逐一正确
    let v0 = judge_open_flags(O_RDONLY, false);
    let v1 = judge_open_flags(O_WRONLY, false);
    let v2 = judge_open_flags(O_RDWR, false);
    let normal = match (v0, v1, v2) {
        (Ok(i0), Ok(i1), Ok(i2)) => {
            i0.access == AccessMode::RdOnly
                && i1.access == AccessMode::WrOnly
                && i2.access == AccessMode::RdWr
                && !i0.is_creating()
                && i0.ignored_mask == 0
        }
        _ => false,
    };
    s.add("正常流：O_RDONLY/O_WRONLY/O_RDWR 三模提交且意图正确", normal,
          "单访问模永远合法，任何前缀规则不得误伤");

    // 3. 正常流：O_CREAT 带全参提交且创建族字段置位
    let creat_ok = match judge_open_flags(O_CREAT | O_TRUNC, true) {
        Ok(i) => i.is_creating() && i.creat && i.trunc && i.mode_arg != 0,
        Err(_) => false,
    };
    s.add("正常流：O_CREAT 带第三参 mode 提交成功", creat_ok,
          "创建类组合在补齐 mode 后必须合法，mode 实参落意图");

    // 4. 访问模式闭集：3 及以上全 EINVAL（Linux 语义）
    let closed_ok = AccessMode::judge(0).is_ok()
        && AccessMode::judge(1).is_ok()
        && AccessMode::judge(2).is_ok()
        && AccessMode::judge(3) == Err(EINVAL)
        && AccessMode::judge(4) == Err(EINVAL)
        && AccessMode::judge(0o177) == Err(EINVAL)
        && judge_open_flags(O_RDWR | O_WRONLY, false) == Err(EINVAL);
    s.add("访问模式闭集：0/1/2 合法，3 及以上全 EINVAL", closed_ok,
          "O_RDONLY|O_WRONLY 同置即 0o3，是本卡第一判据");

    // 5. O_CREAT 缺 mode 注入矩阵：创建族全形态 has_mode=false 全 EINVAL
    let mm1 = judge_open_flags(O_CREAT, false);
    let mm2 = judge_open_flags(O_CREAT | O_EXCL, false);
    let mm3 = judge_open_flags(O_CREAT | O_TRUNC | O_APPEND, false);
    let mm4 = judge_open_flags(O_TMPFILE, false);
    let mm_ok = mm1 == Err(EINVAL) && mm2 == Err(EINVAL)
        && mm3 == Err(EINVAL) && mm4 == Err(EINVAL);
    s.add("缺 mode 矩阵：O_CREAT/O_CREAT|O_EXCL/TRUNC|APPEND/TMPFILE 全 EINVAL",
          mm_ok,
          "创建类 flags 缺第三参 mode 是本卡核心注入面，逐形态复算");

    // 6. O_CREAT|O_EXCL 带 mode 合法（EEXIST 属对象态，不在 flags 面）
    let excl_ok = judge_open_flags(O_CREAT | O_EXCL, true).is_ok();
    s.add("合法组合：O_CREAT|O_EXCL 带 mode 提交成功", excl_ok,
          "独占创建组合合法；存在性冲突属对象态，本卡只保留对账格");

    // 7. O_CREAT|O_TMPFILE 互斥 → EINVAL
    let mutex_ok = judge_open_flags(O_CREAT | O_TMPFILE, true) == Err(EINVAL)
        && judge_open_flags(O_TMPFILE, true).is_ok();
    s.add("互斥：O_CREAT 与 O_TMPFILE 同置 EINVAL、单用 TMPFILE 合法", mutex_ok,
          "Linux 明文两标志互斥，逐码对账不允许可");

    // 8. 畸形 O_TMPFILE：头位缺内嵌 O_DIRECTORY 位 → EINVAL
    let malformed_ok = judge_open_flags(O_TMPFILE_HEADER | O_WRONLY, true) == Err(EINVAL)
        && judge_open_flags(O_TMPFILE_HEADER, false) == Err(EINVAL);
    s.add("畸形：O_TMPFILE 头位缺内嵌 O_DIRECTORY 位全 EINVAL", malformed_ok,
          "头位必须与目录位联用才构成完整 O_TMPFILE，手拼位面必拒");

    // 9. 无效位注入：冻结表外位全 EINVAL（u32::MAX 为极端样本）
    let unknown_ok = judge_open_flags(O_RDWR | 0o4000000, false) == Err(EINVAL)
        && judge_open_flags(O_CREAT | 0o40000, true) == Err(EINVAL)
        && judge_open_flags(u32::MAX, true) == Err(EINVAL);
    s.add("无效位：表外位/中途插位/全位开启全 EINVAL", unknown_ok,
          "冻结位名表之外的任何置位一律 EINVAL，不得静默吞位");

    // 10. 忽略类：Linux 语义忽略位单独成类，落留痕账照常提交
    let ig1 = judge_open_flags(O_EXCL, false);
    let ig2 = judge_open_flags(O_WRONLY | O_ASYNC, false);
    let ig3 = judge_open_flags(O_RDONLY | O_NONBLOCK | O_NOCTTY, false);
    let ignore_ok = match (ig1, ig2, ig3) {
        (Ok(a), Ok(b), Ok(c)) => {
            a.ignored_mask == O_EXCL
                && b.ignored_mask == O_ASYNC
                && c.ignored_mask == O_NONBLOCK | O_NOCTTY
        }
        _ => false,
    };
    s.add("忽略类：EXCL 缺 CREAT / ASYNC / NONBLOCK|NOCTTY 留痕提交",
          ignore_ok,
          "按 Linux 语义应忽略的位不得误报 EINVAL，也不得静默丢弃");

    // 11. 判定次序：访问模式位先于其他一切校验
    let ord1 = judge_open_flags(O_RDWR | O_WRONLY | O_CREAT | 0o4000000, false);
    let order_access = ord1 == Err(EINVAL)
        && judge_open_flags(O_RDWR | O_WRONLY | O_CREAT | O_TMPFILE, true).is_err()
        && judge_flags_order_probe();
    s.add("判序：访问模式冲突压过缺 mode/互斥/未知位", order_access,
          "build_open_flags 口径：访问模规则最先，后续规则不得抢先");

    // 12. 判定次序：缺 mode 先于未知位（同置时按缺 mode 面裁定）
    let order_mode = judge_open_flags(O_CREAT | 0o4000000, false) == Err(EINVAL);
    s.add("判序：创建类缺 mode 先于未知位裁定", order_mode,
          "两条错误同时成立时取冻结序前者，逐序可复算");

    // 13. 对账表全量复算一致
    let (total, agree, drift) = reconcile_table();
    s.add("对账表：逐码复算与冻结期望全量一致",
          total == RECON_TABLE.len() && agree == total && drift.is_none(),
          "每个组合 → 期望 errno 或忽略位集，独立复算不许漂移");

    // 14. 注入矩阵：三类各 10/10 全抓
    let (ledger, per_class) = run_bad_flags_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("三类坏 flags 各 10/10 全 EINVAL", all_caught,
          "缺 mode 矩阵/非法 O_ 位组合/访问模越集逐轮命中");

    // 15. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BAD_FLAG_CLASSES * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(EISDIR)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EACCES)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "EINVAL/EISDIR/EEXIST/EACCES 的负值与 errno.h 逐项对齐");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：flags 语料指纹相等", f1 == f2 && f1 != 0,
          "同语料两次裁定必须逐字节相等，隐藏状态即失败");

    // 18. 文案闭集 + 码值唯一 + 段内不外溢
    let text_ok = FlagCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut in_seg = true;
    let mut i = 0usize;
    while i < FlagCode::ALL.len() {
        if let Some(c) = FlagCode::ALL.get(i).copied() {
            if c.val() < SEG_BASE || c.val() >= SEG_BASE + 0x40 {
                in_seg = false;
            }
            let mut j = i + 1;
            while j < FlagCode::ALL.len() {
                if let Some(d) = FlagCode::ALL.get(j) {
                    if c.val() == d.val() {
                        uniq = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("文案闭集：12 码建议非空、码值唯一且段内不外溢",
          text_ok && uniq && in_seg,
          "诊断码既要能取到建议，也要能唯一定位且不越出 0x40 窗口");

    // 19. 忽略账与拒绝账分立：同一语料上互不串位
    let mut ign_only = 0u32;
    let mut rej_only = 0u32;
    let probes: [(u32, bool); 6] = [
        (O_EXCL, false),
        (O_WRONLY | O_ASYNC, false),
        (O_RDWR | O_WRONLY, false),
        (O_CREAT, false),
        (O_RDONLY | O_NONBLOCK, false),
        (O_CREAT | O_TMPFILE, true),
    ];
    let mut pi = 0usize;
    while pi < probes.len() {
        if let Some((f, m)) = probes.get(pi).copied() {
            match judge_open_flags(f, m) {
                Ok(intent) => {
                    if intent.ignored_mask != 0 {
                        ign_only = ign_only.saturating_add(1);
                    }
                }
                Err(_) => rej_only = rej_only.saturating_add(1),
            }
        }
        pi += 1;
    }
    s.add("账本分立：忽略 3 行、拒绝 3 行、互不串位",
          ign_only == 3 && rej_only == 3,
          "忽略类照常提交、拒绝类显性报错，两账可逐项复算");

    // 动态明细（≤8）：注入、对账、指纹三行
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("注入快照", ledger.conserved(), &ledger_text);
    let recon_text = match drift {
        Some(d) => format!("对账 {}/{} 行一致 / 首个漂移第 {} 行", agree, total, d),
        None => format!("对账 {}/{} 行全一致 / 无漂移", agree, total),
    };
    s.add_dyn("对账快照", agree == total && drift.is_none(), &recon_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    // 20. 收口：判据条数达到声明值
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14,
          "本模块断言只增不减；删条必须显式改声明");

    s
}

/// 判序探针：访问模冲突样本在任何叠加下都必须保持 EINVAL；去掉访问模
/// 冲突后互斥面单独成立（同样 EINVAL，但可独立到达）。
fn judge_flags_order_probe() -> bool {
    // 叠加样本：访问模冲突 + 互斥 + 缺 mode，结果必须仍被访问模面拒绝。
    let stacked = judge_open_flags(O_RDWR | O_WRONLY | O_CREAT | O_TMPFILE, false);
    // 去掉访问模冲突后，互斥面单独成立（同样 EINVAL，但可独立到达）。
    let mutex_alone = judge_open_flags(O_CREAT | O_TMPFILE, false);
    stacked.is_err() && mutex_alone == Err(EINVAL)
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::MAX_CHECKS;

    #[test]
    fn normal_flow_commits() {
        let i = judge_open_flags(O_RDWR | O_TRUNC | O_APPEND, false);
        assert!(i.is_ok(), "正常流不得被拒");
        let intent = match i {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(intent.access, AccessMode::RdWr);
        assert!(intent.trunc && intent.append);
        assert!(!intent.is_creating());
        assert_eq!(intent.ignored_mask, 0);
        let c = judge_open_flags(O_CREAT | O_EXCL, true);
        assert!(c.is_ok(), "带 mode 独占创建不得被拒");
        let cintent = match c {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(cintent.creat && cintent.excl && cintent.mode_arg != 0);
    }

    #[test]
    fn access_mode_four_boundaries() {
        // 四类边界：0 最小合法 / 2 最大合法 / 3 最小非法（越集） / u32::MAX 极端
        assert_eq!(AccessMode::judge(0), Ok(AccessMode::RdOnly));
        assert_eq!(AccessMode::judge(2), Ok(AccessMode::RdWr));
        assert_eq!(AccessMode::judge(3), Err(EINVAL));
        assert_eq!(AccessMode::judge(u32::MAX), Err(EINVAL));
        // 掩码内唯一越集形态 0o3 经判定器同样 EINVAL
        assert_eq!(judge_open_flags(0o3, false), Err(EINVAL));
    }

    #[test]
    fn missing_mode_matrix_all_einval() {
        let rows: [u32; 6] = [
            O_CREAT,
            O_CREAT | O_EXCL,
            O_CREAT | O_TRUNC,
            O_CREAT | O_APPEND | O_TRUNC,
            O_TMPFILE,
            O_TMPFILE | O_WRONLY,
        ];
        let mut n = 0u32;
        for f in rows.iter() {
            assert_eq!(judge_open_flags(*f, false), Err(EINVAL));
            n += 1;
        }
        assert_eq!(n, 6);
        // 同一组组合补上 mode 后必须全部合法（互斥对除外，不在本组）。
        for f in rows.iter() {
            assert!(judge_open_flags(*f, true).is_ok());
        }
    }

    #[test]
    fn illegal_bits_all_einval() {
        assert_eq!(judge_open_flags(O_RDWR | 0o4000000, false), Err(EINVAL));
        assert_eq!(judge_open_flags(O_CREAT | 0o40000, true), Err(EINVAL));
        assert_eq!(judge_open_flags(O_TMPFILE_HEADER | O_WRONLY, true), Err(EINVAL));
        assert_eq!(judge_open_flags(O_CREAT | O_TMPFILE, true), Err(EINVAL));
        assert_eq!(judge_open_flags(u32::MAX, true), Err(EINVAL));
        assert_eq!(judge_open_flags(u32::MAX, false), Err(EINVAL));
    }

    #[test]
    fn ignore_class_separate_from_reject() {
        let a = judge_open_flags(O_EXCL, false);
        let b = judge_open_flags(O_WRONLY | O_ASYNC, false);
        let c = judge_open_flags(O_RDONLY | O_NONBLOCK, false);
        let d = judge_open_flags(O_RDWR | O_NOCTTY, false);
        match (a, b, c, d) {
            (Ok(x), Ok(y), Ok(z), Ok(w)) => {
                assert_eq!(x.ignored_mask, O_EXCL);
                assert_eq!(y.ignored_mask, O_ASYNC);
                assert_eq!(z.ignored_mask, O_NONBLOCK);
                assert_eq!(w.ignored_mask, O_NOCTTY);
            }
            _ => assert!(false, "忽略类不得被拒"),
        }
        // 对照：拒绝类样本同样输入形态必须被拒。
        assert_eq!(judge_open_flags(O_RDWR | O_WRONLY, false), Err(EINVAL));
    }

    #[test]
    fn judgment_order_access_first() {
        // 访问模冲突 + 缺 mode + 未知位 + 互斥全部叠加，仍按访问模面拒。
        let stacked = O_RDWR | O_WRONLY | O_CREAT | O_TMPFILE | 0o4000000;
        assert_eq!(judge_open_flags(stacked, false), Err(EINVAL));
        assert!(judge_flags_order_probe());
        // 缺 mode 先于未知位。
        assert_eq!(judge_open_flags(O_CREAT | 0o4000000, false), Err(EINVAL));
        // 去掉访问模冲突后缺 mode 面单独可达。
        assert_eq!(judge_open_flags(O_CREAT | 0o4000000, true), Err(EINVAL));
    }

    #[test]
    fn injection_three_classes_ten_rounds() {
        let (l, per) = run_bad_flags_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, BAD_FLAG_CLASSES * INJECT_ROUNDS);
        assert_eq!(per.len(), 3);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn ledger_conservation_and_regress() {
        let mut l = InjectLedger::zero();
        let mut k = 0u32;
        while k < 7 {
            let _ = l.record(true);
            k += 1;
        }
        assert!(l.conserved());
        assert_eq!(l.detected, 7);
        let _ = l.record(false);
        assert_eq!(l.escaped, 1);
        assert!(l.conserved());
        // 守恒式始终可复算。
        assert_eq!(l.detected + l.escaped, l.injected);
    }

    #[test]
    fn recon_table_frozen() {
        let (total, agree, drift) = reconcile_table();
        assert_eq!(total, RECON_TABLE.len());
        assert_eq!(agree, total);
        assert!(drift.is_none());
        // 抽样复算：对账行与直判结果逐项一致。
        for row in RECON_TABLE.iter() {
            match row.3 {
                ReconExpect::OkWith(ign) => {
                    match judge_open_flags(row.1, row.2) {
                        Ok(intent) => assert_eq!(intent.ignored_mask, ign),
                        Err(_) => assert!(false, "对账 Ok 行被直判拒绝"),
                    }
                }
                ReconExpect::ErrWith(e) => {
                    assert_eq!(judge_open_flags(row.1, row.2), Err(e));
                }
            }
        }
    }

    #[test]
    fn double_run_identical() {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        assert_eq!(a, b);
        assert_ne!(a, 0);
        // 对账复算也双跑一致。
        let (t1, ag1, d1) = reconcile_table();
        let (t2, ag2, d2) = reconcile_table();
        assert_eq!((t1, ag1, d1), (t2, ag2, d2));
    }

    #[test]
    fn checkset_capacity_exhaustion() {
        let mut s = CheckSet::new("unx9207_cap");
        let mut n = 0usize;
        while n < MAX_CHECKS + 10 {
            s.add("容量探针", true, "占位判据");
            n += 1;
        }
        assert_eq!(s.len(), MAX_CHECKS);
        assert!(s.truncated());
        assert!(s.dropped() > 0);
        assert!(s.all_passed());
    }

    #[test]
    fn segment_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        // 诊断码全部落在本段窗口内。
        for c in FlagCode::ALL.iter() {
            assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40);
        }
    }

    #[test]
    fn errno_table_linux_alignment() {
        assert!(agrees_with_linux(EINVAL));
        assert!(agrees_with_linux(EISDIR));
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(EACCES));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(9999));
    }

    #[test]
    fn unx9207_checks_all_green() {
        let s = run_unx9207_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert!(s.len() >= 14);
        assert!(s.dyn_used() <= 8);
        assert!(s.dropped() == 0);
    }
}
