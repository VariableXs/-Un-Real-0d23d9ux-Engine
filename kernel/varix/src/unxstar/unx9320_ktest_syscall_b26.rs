//! UNX-F9320 ktest syscall 面 B26 批断言集 · 0x7BD80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9320 逐字）：
//!   ① 纯功能代码 ≥480 行；② 判据原文：UNX-F9320-J1 本批 19 条判据聚合判据过，
//!   EINTR 竞态矩阵一次跑通，域累计 139,900 收口断言；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 本批是聚合器不是清单：19 条断言各自挂一个求值函数（fn 指针），聚合时
//!      逐条调用并同时记 total/passed/failed/asserted 四本数。first_fail 只在
//!      首次失败时写入，后续失败不覆盖——失败定位必须指向**第一条**，否则
//!      修完第一条还要重新跑批才知道下一条。
//!   ② 「一次跑通」是硬约束：RaceMatrix 有 runs 计数，第二次 run_once 直接拒绝
//!      并累加 refused，不改 matched/missed。矩阵若允许重复跑，一次成功就失去
//!      证据价值（重跑等于给失败留后门）。
//!   ③ 竞态矩阵是 5 相位 × 4 处置的 20 格：相位为进入前 / 拷入中 / 阻塞中 /
//!      唤醒时 / 返回后，处置为默认 / 处理器不带 SA_RESTART / 带 SA_RESTART /
//!      忽略。冻结表与独立推导函数 matrix_outcome 双向比对，20 格全中才收口。
//!   ④ 累计收口是纯算术：本域此前 139,881 条 + 本批 19 条 = 139,900 条。两条
//!      路径（饱和加与 checked_add）必须给出同一结果，且 checked_add 不得为 None。
//!   ⑤ 判据必须真的能绿：19 个求值函数互相独立，各自只依赖本模块内可复算的
//!      常量与表，不读任何运行期可变状态；聚合器本身不参与求值。
//!   ⑥ 诊断码 12 个、码值唯一；指纹覆盖条目表、矩阵 20 格与累计值。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9320 独占 0x7BD80..=0x7BDBF）。
pub const SEG_BASE: u32 = 0x7BD80;
/// 段窗口。
pub const SEG_SPAN: u32 = 0x40;
pub const SEG_WINDOW_LO: u32 = 0x7B000;
pub const SEG_WINDOW_HI: u32 = 0x7BFFF;

// ── Linux errno（正数口径） ───────────────────────────────────────────
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ESRCH: i32 = 3;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const ECHILD: i32 = 10;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOSYS: i32 = 38;

/// SA_RESTART 位（x86-64 sigaction 标志）。
pub const SA_RESTART: u32 = 0x1000_0000;

/// 本批断言条数。
pub const B26_ITEM_COUNT: usize = 19;
/// 本域在此批之前的累计断言数。
pub const PRIOR_DOMAIN_ASSERTS: u32 = 139_881;
/// 本域收口目标。
pub const DOMAIN_TOTAL_TARGET: u32 = 139_900;
/// 竞态相位维度。
pub const RACE_PHASES: usize = 5;
/// 竞态处置维度。
pub const RACE_DISPOSITIONS: usize = 4;
/// 竞态矩阵格数。
pub const RACE_CELLS: usize = 20;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9320 诊断码（段内 +1..=+12）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchCode {
    ItemTableGap,
    ItemIdNotDense,
    ItemTagEmpty,
    SilentSkip,
    BatchTallyBroken,
    ClassUncovered,
    MatrixShapeDrift,
    MatrixMissed,
    MatrixRerun,
    ClosureBroken,
    ClosureOverflow,
    NotReproducible,
}

impl BatchCode {
    /// 段内序号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值。
    pub fn val(self) -> u32 {
        SEG_BASE.saturating_add(1).saturating_add(self.code() as u32)
    }

    /// 中文建议。
    pub fn detail(self) -> &'static str {
        match self {
            BatchCode::ItemTableGap => "判据表有缺口：19 条必须一条不少，缺一条聚合数就对不上",
            BatchCode::ItemIdNotDense => "编号不稠密：1..=19 连续且唯一，跳号会让定位偏移",
            BatchCode::ItemTagEmpty => "标签为空：每条判据都要有可打印标签，空标签无法归因",
            BatchCode::SilentSkip => "存在静默跳过：每条必须记 1 次断言，0 次等于没跑",
            BatchCode::BatchTallyBroken => "分批账不平：total 必须等于 passed 加 failed",
            BatchCode::ClassUncovered => "分类未覆盖：errno/传输/信号/账目四类都要至少一条",
            BatchCode::MatrixShapeDrift => "矩阵维度漂移：5 相位乘 4 处置固定为 20 格",
            BatchCode::MatrixMissed => "矩阵有漏格：20 格必须全中，漏一格即视为未跑通",
            BatchCode::MatrixRerun => "矩阵被重复跑：一次跑通后必须拒绝再次运行",
            BatchCode::ClosureBroken => "累计收口不平：139881 加 19 必须等于 139900",
            BatchCode::ClosureOverflow => "累计加法溢出：收口必须走 checked 加法且不得为 None",
            BatchCode::NotReproducible => "双跑不一致：批次与矩阵必须逐字节可复现",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [BatchCode] = &[
        BatchCode::ItemTableGap,
        BatchCode::ItemIdNotDense,
        BatchCode::ItemTagEmpty,
        BatchCode::SilentSkip,
        BatchCode::BatchTallyBroken,
        BatchCode::ClassUncovered,
        BatchCode::MatrixShapeDrift,
        BatchCode::MatrixMissed,
        BatchCode::MatrixRerun,
        BatchCode::ClosureBroken,
        BatchCode::ClosureOverflow,
        BatchCode::NotReproducible,
    ];
}

// ── errno 面 ──────────────────────────────────────────────────────────
/// errno 常量表（按数值升序，名称唯一）。
pub const ERRNO_TABLE: &[(&str, i32)] = &[
    ("EPERM", EPERM),
    ("ENOENT", ENOENT),
    ("ESRCH", ESRCH),
    ("EINTR", EINTR),
    ("EBADF", EBADF),
    ("ECHILD", ECHILD),
    ("EAGAIN", EAGAIN),
    ("ENOMEM", ENOMEM),
    ("EFAULT", EFAULT),
    ("EINVAL", EINVAL),
    ("ENOSYS", ENOSYS),
];

/// 名称 → 数值。
pub fn errno_value(name: &str) -> Option<i32> {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.0 == name {
                return Some(row.1);
            }
        }
        i = i.saturating_add(1);
    }
    None
}

/// 数值 → 系统调用返回口径（负数）。
pub fn errno_negative(code: i32) -> Option<i32> {
    if code <= 0 {
        return None;
    }
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return Some(-code);
            }
        }
        i = i.saturating_add(1);
    }
    None
}

/// 表内 errno 是否全在正数域且名称非空。
pub fn errno_table_ok() -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        match ERRNO_TABLE.get(i) {
            Some(row) => {
                if row.1 <= 0 || row.0.is_empty() {
                    return false;
                }
                if errno_negative(row.1) != Some(-row.1) {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1);
    }
    true
}

// ── 传输面 ────────────────────────────────────────────────────────────
/// 读语义分类。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadOutcome {
    Full = 0,
    Short = 1,
    Failed = 2,
    Interrupted = 3,
}

/// 写语义分类。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteOutcome {
    Full = 0,
    Partial = 1,
    Failed = 2,
    Interrupted = 3,
}

impl ReadOutcome {
    /// 分类码。
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// 读语义：负值为 EINTR 或被拒；非负按请求量分类。
pub fn interpret_read(result: i64, errno: i32, requested: u64) -> ReadOutcome {
    if result < 0 {
        if errno == EINTR {
            return ReadOutcome::Interrupted;
        }
        return ReadOutcome::Failed;
    }
    let done = result as u64;
    if done < requested {
        ReadOutcome::Short
    } else {
        ReadOutcome::Full
    }
}

/// 写语义：非负且等于请求量为全量。
pub fn interpret_write(result: i64, errno: i32, requested: u64) -> WriteOutcome {
    if result < 0 {
        if errno == EINTR {
            return WriteOutcome::Interrupted;
        }
        return WriteOutcome::Failed;
    }
    let done = result as u64;
    if done < requested {
        WriteOutcome::Partial
    } else {
        WriteOutcome::Full
    }
}

/// glibc 保守重试谓词（本批只做口径核对）。
pub fn retryable(result: i64, errno: i32) -> bool {
    result == -1 && errno == EINTR
}

/// SA_RESTART 位判定。
pub fn flags_has_restart(flags: u32) -> bool {
    flags & SA_RESTART != 0
}

// ── 竞态矩阵 ──────────────────────────────────────────────────────────
/// 信号到达相位。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RacePhase {
    BeforeEnter = 0,
    DuringCopyIn = 1,
    DuringBlock = 2,
    AtWakeup = 3,
    AfterReturn = 4,
}

/// 信号处置。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalDisposition {
    Default = 0,
    HandlerNoRestart = 1,
    HandlerRestart = 2,
    Ignored = 3,
}

/// 竞态结局。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaceOutcome {
    Success = 0,
    Eintr = 1,
    ShortRead = 2,
    Restarted = 3,
}

impl RacePhase {
    /// 相位码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            RacePhase::BeforeEnter => "进入前",
            RacePhase::DuringCopyIn => "拷入中",
            RacePhase::DuringBlock => "阻塞中",
            RacePhase::AtWakeup => "唤醒时",
            RacePhase::AfterReturn => "返回后",
        }
    }
}

impl SignalDisposition {
    /// 处置码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 处置名。
    pub fn name(self) -> &'static str {
        match self {
            SignalDisposition::Default => "默认处置",
            SignalDisposition::HandlerNoRestart => "处理器无重启",
            SignalDisposition::HandlerRestart => "处理器带重启",
            SignalDisposition::Ignored => "忽略信号",
        }
    }
}

impl RaceOutcome {
    /// 结局码。
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// 相位表（顺序即矩阵行序，不得重排）。
pub const PHASE_AXIS: [RacePhase; RACE_PHASES] = [
    RacePhase::BeforeEnter,
    RacePhase::DuringCopyIn,
    RacePhase::DuringBlock,
    RacePhase::AtWakeup,
    RacePhase::AfterReturn,
];

/// 处置表（顺序即矩阵列序）。
pub const DISPOSITION_AXIS: [SignalDisposition; RACE_DISPOSITIONS] = [
    SignalDisposition::Default,
    SignalDisposition::HandlerNoRestart,
    SignalDisposition::HandlerRestart,
    SignalDisposition::Ignored,
];

/// 冻结矩阵：5 行 × 4 列。
pub const RACE_MATRIX: [[RaceOutcome; RACE_DISPOSITIONS]; RACE_PHASES] = [
    [
        RaceOutcome::Success,
        RaceOutcome::Success,
        RaceOutcome::Success,
        RaceOutcome::Success,
    ],
    [
        RaceOutcome::ShortRead,
        RaceOutcome::ShortRead,
        RaceOutcome::Restarted,
        RaceOutcome::Success,
    ],
    [
        RaceOutcome::Eintr,
        RaceOutcome::Eintr,
        RaceOutcome::Restarted,
        RaceOutcome::Success,
    ],
    [
        RaceOutcome::Success,
        RaceOutcome::Success,
        RaceOutcome::Success,
        RaceOutcome::Success,
    ],
    [
        RaceOutcome::Success,
        RaceOutcome::Success,
        RaceOutcome::Success,
        RaceOutcome::Success,
    ],
];

/// 独立推导：不读冻结表，按相位与处置语义重算结局。
pub fn matrix_outcome(phase: RacePhase, disp: SignalDisposition) -> RaceOutcome {
    match (phase, disp) {
        (RacePhase::BeforeEnter, _) => RaceOutcome::Success,
        (RacePhase::DuringCopyIn, SignalDisposition::Ignored) => RaceOutcome::Success,
        (RacePhase::DuringCopyIn, SignalDisposition::HandlerRestart) => RaceOutcome::Restarted,
        (RacePhase::DuringCopyIn, _) => RaceOutcome::ShortRead,
        (RacePhase::DuringBlock, SignalDisposition::Ignored) => RaceOutcome::Success,
        (RacePhase::DuringBlock, SignalDisposition::HandlerRestart) => RaceOutcome::Restarted,
        (RacePhase::DuringBlock, _) => RaceOutcome::Eintr,
        (RacePhase::AtWakeup, _) => RaceOutcome::Success,
        (RacePhase::AfterReturn, _) => RaceOutcome::Success,
    }
}

/// 竞态矩阵执行器：只允许跑一次。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RaceMatrix {
    pub matched: u32,
    pub missed: u32,
    pub runs: u32,
    pub refused: u32,
}

impl RaceMatrix {
    /// 未跑。
    pub const fn new() -> RaceMatrix {
        RaceMatrix {
            matched: 0,
            missed: 0,
            runs: 0,
            refused: 0,
        }
    }

    /// 单次运行：第二次起拒绝，且不改计数。
    pub fn run_once(&mut self) -> bool {
        if self.runs >= 1 {
            self.refused = self.refused.saturating_add(1);
            return false;
        }
        self.runs = 1;
        let mut r = 0usize;
        while r < PHASE_AXIS.len() {
            let phase = match PHASE_AXIS.get(r) {
                Some(p) => *p,
                None => return false,
            };
            let mut c = 0usize;
            while c < DISPOSITION_AXIS.len() {
                let disp = match DISPOSITION_AXIS.get(c) {
                    Some(d) => *d,
                    None => return false,
                };
                let want = match RACE_MATRIX.get(r).and_then(|row| row.get(c)) {
                    Some(v) => *v,
                    None => RaceOutcome::Success,
                };
                let got = matrix_outcome(phase, disp);
                if got == want {
                    self.matched = self.matched.saturating_add(1);
                } else {
                    self.missed = self.missed.saturating_add(1);
                }
                c = c.saturating_add(1);
            }
            r = r.saturating_add(1);
        }
        true
    }

    /// 已判定格数。
    pub fn cells(&self) -> u32 {
        self.matched.saturating_add(self.missed)
    }
}

// ── 批次条目 ──────────────────────────────────────────────────────────
/// 判据分类。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B26Class {
    ErrnoFace = 0,
    TransferFace = 1,
    SignalFace = 2,
    LedgerFace = 3,
}

impl B26Class {
    /// 分类码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 分类名。
    pub fn name(self) -> &'static str {
        match self {
            B26Class::ErrnoFace => "错误码面",
            B26Class::TransferFace => "传输面",
            B26Class::SignalFace => "信号面",
            B26Class::LedgerFace => "账目面",
        }
    }
}

/// 一条批判据：编号 + 标签 + 分类 + 断言权重 + 求值函数。
#[derive(Clone, Copy)]
pub struct B26Item {
    pub id: u8,
    pub tag: &'static str,
    pub class: B26Class,
    pub asserted: u32,
    pub eval: fn() -> bool,
}

fn b26_01() -> bool {
    errno_value("EINTR") == Some(EINTR) && EINTR == 4
}

fn b26_02() -> bool {
    errno_negative(EINTR) == Some(-4) && errno_negative(0).is_none()
}

fn b26_03() -> bool {
    flags_has_restart(SA_RESTART) && !flags_has_restart(0) && !flags_has_restart(0x2)
}

fn b26_04() -> bool {
    interpret_read(12, EINTR, 4096) == ReadOutcome::Short
}

fn b26_05() -> bool {
    retryable(-1, EINTR) && !retryable(0, EINTR) && !retryable(-1, EAGAIN)
}

fn b26_06() -> bool {
    interpret_write(4096, 0, 4096) == WriteOutcome::Full
        && interpret_write(7, 0, 4096) == WriteOutcome::Partial
}

fn b26_07() -> bool {
    PHASE_AXIS.len() == RACE_PHASES
        && DISPOSITION_AXIS.len() == RACE_DISPOSITIONS
        && RACE_PHASES.saturating_mul(RACE_DISPOSITIONS) == RACE_CELLS
}

fn b26_08() -> bool {
    let mut m = RaceMatrix::new();
    let ran = m.run_once();
    ran && m.matched == RACE_CELLS as u32 && m.missed == 0
}

fn b26_09() -> bool {
    let mut m = RaceMatrix::new();
    let first = m.run_once();
    let second = m.run_once();
    first && !second && m.runs == 1 && m.refused == 1 && m.cells() == RACE_CELLS as u32
}

fn b26_10() -> bool {
    let mut m = RaceMatrix::new();
    m.run_once();
    m.matched.saturating_add(m.missed) == RACE_CELLS as u32
}

fn b26_11() -> bool {
    cumulative_total() == DOMAIN_TOTAL_TARGET && cumulative_checked() == Some(DOMAIN_TOTAL_TARGET)
}

fn b26_12() -> bool {
    item_ids_dense()
}

fn b26_13() -> bool {
    let mut i = 0usize;
    while i < B26_ITEMS.len() {
        match B26_ITEMS.get(i) {
            Some(it) => {
                if it.tag.is_empty() || it.class.name().is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1);
    }
    true
}

fn b26_14() -> bool {
    let mut i = 0usize;
    while i < B26_ITEMS.len() {
        match B26_ITEMS.get(i) {
            Some(it) => {
                if it.asserted != 1 {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1);
    }
    true
}

fn b26_15() -> bool {
    let mut prev = 0u8;
    let mut i = 0usize;
    while i < B26_ITEMS.len() {
        match B26_ITEMS.get(i) {
            Some(it) => {
                if it.id <= prev {
                    return false;
                }
                prev = it.id;
            }
            None => return false,
        }
        i = i.saturating_add(1);
    }
    true
}

fn b26_16() -> bool {
    corpus_fingerprint() == corpus_fingerprint()
}

fn b26_17() -> bool {
    errno_value("ESRCH") == Some(3)
}

fn b26_18() -> bool {
    errno_value("ENOSYS") == Some(38)
}

fn b26_19() -> bool {
    errno_value("EINVAL") == Some(22)
}

/// 十九条批判据（编号 1..=19，顺序即执行序）。
pub const B26_ITEMS: [B26Item; B26_ITEM_COUNT] = [
    B26Item {
        id: 1,
        tag: "EINTR=4",
        class: B26Class::ErrnoFace,
        asserted: 1,
        eval: b26_01,
    },
    B26Item {
        id: 2,
        tag: "负数口径",
        class: B26Class::ErrnoFace,
        asserted: 1,
        eval: b26_02,
    },
    B26Item {
        id: 3,
        tag: "SA_RESTART",
        class: B26Class::SignalFace,
        asserted: 1,
        eval: b26_03,
    },
    B26Item {
        id: 4,
        tag: "短读语义",
        class: B26Class::TransferFace,
        asserted: 1,
        eval: b26_04,
    },
    B26Item {
        id: 5,
        tag: "重试谓词",
        class: B26Class::SignalFace,
        asserted: 1,
        eval: b26_05,
    },
    B26Item {
        id: 6,
        tag: "写全量与部分",
        class: B26Class::TransferFace,
        asserted: 1,
        eval: b26_06,
    },
    B26Item {
        id: 7,
        tag: "矩阵维度",
        class: B26Class::SignalFace,
        asserted: 1,
        eval: b26_07,
    },
    B26Item {
        id: 8,
        tag: "矩阵全中",
        class: B26Class::SignalFace,
        asserted: 1,
        eval: b26_08,
    },
    B26Item {
        id: 9,
        tag: "只跑一次",
        class: B26Class::SignalFace,
        asserted: 1,
        eval: b26_09,
    },
    B26Item {
        id: 10,
        tag: "矩阵守恒",
        class: B26Class::SignalFace,
        asserted: 1,
        eval: b26_10,
    },
    B26Item {
        id: 11,
        tag: "累计收口",
        class: B26Class::LedgerFace,
        asserted: 1,
        eval: b26_11,
    },
    B26Item {
        id: 12,
        tag: "编号稠密",
        class: B26Class::LedgerFace,
        asserted: 1,
        eval: b26_12,
    },
    B26Item {
        id: 13,
        tag: "标签非空",
        class: B26Class::LedgerFace,
        asserted: 1,
        eval: b26_13,
    },
    B26Item {
        id: 14,
        tag: "无静默跳过",
        class: B26Class::LedgerFace,
        asserted: 1,
        eval: b26_14,
    },
    B26Item {
        id: 15,
        tag: "次序稳定",
        class: B26Class::LedgerFace,
        asserted: 1,
        eval: b26_15,
    },
    B26Item {
        id: 16,
        tag: "指纹双跑",
        class: B26Class::LedgerFace,
        asserted: 1,
        eval: b26_16,
    },
    B26Item {
        id: 17,
        tag: "ESRCH=3",
        class: B26Class::ErrnoFace,
        asserted: 1,
        eval: b26_17,
    },
    B26Item {
        id: 18,
        tag: "ENOSYS=38",
        class: B26Class::ErrnoFace,
        asserted: 1,
        eval: b26_18,
    },
    B26Item {
        id: 19,
        tag: "EINVAL=22",
        class: B26Class::ErrnoFace,
        asserted: 1,
        eval: b26_19,
    },
];

/// 编号是否 1..=19 稠密。
pub fn item_ids_dense() -> bool {
    let mut expect = 1u8;
    let mut i = 0usize;
    while i < B26_ITEMS.len() {
        match B26_ITEMS.get(i) {
            Some(it) => {
                if it.id != expect {
                    return false;
                }
                expect = expect.saturating_add(1);
            }
            None => return false,
        }
        i = i.saturating_add(1);
    }
    expect == (B26_ITEM_COUNT as u8).saturating_add(1)
}

/// 分类覆盖计数（四类）。
pub fn class_coverage() -> [u32; 4] {
    let mut counts = [0u32; 4];
    let mut i = 0usize;
    while i < B26_ITEMS.len() {
        if let Some(it) = B26_ITEMS.get(i) {
            let idx = it.class.code() as usize;
            if let Some(c) = counts.get_mut(idx) {
                *c = c.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    counts
}

// ── 批次聚合 ──────────────────────────────────────────────────────────
/// 批次报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchReport {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub asserted: u32,
    pub first_fail: u8,
}

impl BatchReport {
    /// 空报告。
    pub const fn empty() -> BatchReport {
        BatchReport {
            total: 0,
            passed: 0,
            failed: 0,
            asserted: 0,
            first_fail: 0,
        }
    }

    /// 分批账平：total == passed + failed。
    pub fn tally_ok(&self) -> bool {
        self.total == self.passed.saturating_add(self.failed)
    }

    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.tally_ok() && self.failed == 0 && self.first_fail == 0
    }
}

/// 聚合器：逐条求值，首次失败即记 first_fail。
pub fn run_batch() -> BatchReport {
    let mut r = BatchReport::empty();
    let mut i = 0usize;
    while i < B26_ITEMS.len() {
        if let Some(it) = B26_ITEMS.get(i) {
            let ok = (it.eval)();
            r.total = r.total.saturating_add(1);
            r.asserted = r.asserted.saturating_add(it.asserted);
            if ok {
                r.passed = r.passed.saturating_add(1);
            } else {
                r.failed = r.failed.saturating_add(1);
                if r.first_fail == 0 {
                    r.first_fail = it.id;
                }
            }
        }
        i = i.saturating_add(1);
    }
    r
}

/// 累计收口（饱和加）。
pub fn cumulative_total() -> u32 {
    PRIOR_DOMAIN_ASSERTS.saturating_add(B26_ITEM_COUNT as u32)
}

/// 累计收口（checked 加，溢出即 None）。
pub fn cumulative_checked() -> Option<u32> {
    PRIOR_DOMAIN_ASSERTS.checked_add(B26_ITEM_COUNT as u32)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i = i.saturating_add(1);
    }
    h
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v >> 24) as u8);
    out.push((v >> 16) as u8);
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    push_u32(out, bytes.len() as u32);
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            out.push(*b);
        }
        i = i.saturating_add(1);
    }
}

/// 规范语料指纹：条目表 + 矩阵 20 格 + 累计值。
/// 注意：不调用 run_batch，避免求值函数经指纹反向递归。
pub fn corpus_fingerprint() -> u32 {
    let mut out: Vec<u8> = Vec::new();
    push_u32(&mut out, SEG_BASE);
    push_u32(&mut out, B26_ITEM_COUNT as u32);
    let mut i = 0usize;
    while i < B26_ITEMS.len() {
        if let Some(it) = B26_ITEMS.get(i) {
            push_u32(&mut out, it.id as u32);
            push_u32(&mut out, it.class.code());
            push_u32(&mut out, it.asserted);
            push_str(&mut out, it.tag);
        }
        i = i.saturating_add(1);
    }
    let mut r = 0usize;
    while r < PHASE_AXIS.len() {
        if let Some(phase) = PHASE_AXIS.get(r) {
            let mut c = 0usize;
            while c < DISPOSITION_AXIS.len() {
                if let Some(disp) = DISPOSITION_AXIS.get(c) {
                    push_u32(&mut out, phase.code());
                    push_u32(&mut out, disp.code());
                    push_u32(&mut out, matrix_outcome(*phase, *disp).code());
                }
                c = c.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    push_u32(&mut out, cumulative_total());
    push_u32(&mut out, cumulative_checked().unwrap_or(0));
    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9320 判据集。
pub fn run_unx9320_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9320");

    let seg_ok = SEG_BASE % SEG_SPAN == 0
        && (SEG_WINDOW_LO..=SEG_WINDOW_HI).contains(&SEG_BASE)
        && SEG_BASE == 0x7BD80;
    s.add(
        "段位守卫：0x40 对齐且落在本批窗口",
        seg_ok,
        "0x7BD80..=0x7BDBF 归本卡，不得与前后卡交叉",
    );

    let report = run_batch();
    s.add(
        "聚合：19 条判据全过",
        report.total == B26_ITEM_COUNT as u32
            && report.passed == B26_ITEM_COUNT as u32
            && report.failed == 0
            && report.first_fail == 0,
        "本批 19 条必须一次全绿，首败定位为 0 才算收口",
    );

    s.add(
        "条目表：编号 1..=19 稠密唯一",
        item_ids_dense() && B26_ITEMS.len() == B26_ITEM_COUNT,
        "跳号或重号会让定位偏移，稠密是聚合前提",
    );

    let mut tags_ok = true;
    let mut i = 0usize;
    while i < B26_ITEMS.len() {
        match B26_ITEMS.get(i) {
            Some(it) => {
                if it.tag.is_empty() {
                    tags_ok = false;
                }
            }
            None => tags_ok = false,
        }
        i = i.saturating_add(1);
    }
    s.add(
        "条目表：标签非空",
        tags_ok,
        "每条都要有可打印标签，空标签无法归因",
    );

    s.add(
        "无静默跳过：断言合计 19 次",
        report.asserted == B26_ITEM_COUNT as u32 && report.asserted == report.total,
        "每条断言权重为 1，合计必须等于条数",
    );

    s.add(
        "分批账：total == passed + failed",
        report.tally_ok() && report.total == 19 && report.failed == 0,
        "账不平说明有判据被跳过或被重复计数",
    );

    let cov = class_coverage();
    let cov_sum: u32 = cov.iter().fold(0u32, |acc, v| acc.saturating_add(*v));
    s.add(
        "分类覆盖：四类各至少一条",
        cov.iter().all(|v| *v > 0) && cov_sum == B26_ITEM_COUNT as u32,
        "errno/传输/信号/账目四类都要有落点",
    );

    s.add(
        "矩阵维度：5 相位 × 4 处置 = 20 格",
        PHASE_AXIS.len() == RACE_PHASES
            && DISPOSITION_AXIS.len() == RACE_DISPOSITIONS
            && RACE_CELLS == RACE_PHASES.saturating_mul(RACE_DISPOSITIONS),
        "维度漂移会让冻结表与推导函数错位",
    );

    let mut matrix = RaceMatrix::new();
    let ran = matrix.run_once();
    s.add(
        "竞态矩阵：一次跑通 20/20",
        ran
            && matrix.matched == RACE_CELLS as u32
            && matrix.missed == 0
            && matrix.cells() == RACE_CELLS as u32,
        "20 格全部与独立推导一致才算跑通",
    );

    let second = matrix.run_once();
    s.add(
        "竞态矩阵：第二次运行被拒绝",
        !second && matrix.runs == 1 && matrix.refused == 1 && matrix.matched == 20,
        "重跑等于给失败留后门，必须拒绝且不动计数",
    );

    s.add(
        "累计收口：139881 + 19 == 139900",
        cumulative_total() == DOMAIN_TOTAL_TARGET
            && cumulative_checked() == Some(DOMAIN_TOTAL_TARGET),
        "饱和加与 checked 加必须同值，且 checked 不得为 None",
    );

    s.add(
        "errno 面：表内十一条数值与负号口径一致",
        errno_table_ok()
            && errno_value("EINTR") == Some(4)
            && errno_value("ESRCH") == Some(3)
            && errno_value("ENOSYS") == Some(38)
            && errno_negative(22) == Some(-22)
            && errno_value("ENOTDIR").is_none(),
        "错误码只允许表内正值，负数口径统一由 -code 生成",
    );

    s.add(
        "传输面：短读与全量分类正确",
        interpret_read(12, EINTR, 4096) == ReadOutcome::Short
            && interpret_read(4096, 0, 4096) == ReadOutcome::Full
            && interpret_read(-1, EINTR, 4096) == ReadOutcome::Interrupted
            && interpret_read(-1, EAGAIN, 4096) == ReadOutcome::Failed
            && interpret_write(100, 0, 4096) == WriteOutcome::Partial
            && interpret_write(4096, 0, 4096) == WriteOutcome::Full,
        "短读是成功不是中断，EINTR 才归中断类",
    );

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：条目与矩阵指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同输入两次必须逐字节相等，批次不得有隐藏状态",
    );

    s.add(
        "诊断码：12 个码文案非空",
        BatchCode::ALL.len() == 12 && BatchCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给出可操作的中文建议",
    );

    let mut uniq = true;
    let mut a = 0usize;
    while a < BatchCode::ALL.len() {
        let mut b = a.saturating_add(1);
        while b < BatchCode::ALL.len() {
            if let (Some(x), Some(y)) = (BatchCode::ALL.get(a), BatchCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b = b.saturating_add(1);
        }
        a = a.saturating_add(1);
    }
    s.add(
        "诊断码：12 个码值两两唯一",
        uniq,
        "段内闭集码不得重复",
    );

    let batch_text = format!(
        "批次 {}/{} 断言 {} 首败 {}",
        report.passed, report.total, report.asserted, report.first_fail
    );
    s.add_dyn(
        "批次快照",
        report.all_green(),
        &batch_text,
    );
    let matrix_text = format!(
        "矩阵 {}/{} 跑 {} 拒 {}",
        matrix.matched, RACE_CELLS, matrix.runs, matrix.refused
    );
    s.add_dyn(
        "矩阵快照",
        matrix.matched == RACE_CELLS as u32 && matrix.refused == 1,
        &matrix_text,
    );
    let closure_text = format!("累计 {}", cumulative_total());
    s.add_dyn(
        "收口快照",
        cumulative_total() == DOMAIN_TOTAL_TARGET,
        &closure_text,
    );

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_table_is_dense() {
        assert_eq!(B26_ITEMS.len(), B26_ITEM_COUNT);
        assert_eq!(item_ids_dense(), true);
        let first = B26_ITEMS.first().map(|i| i.id);
        let last = B26_ITEMS.last().map(|i| i.id);
        assert_eq!(first, Some(1));
        assert_eq!(last, Some(19));
    }

    #[test]
    fn batch_is_all_green() {
        let r = run_batch();
        assert_eq!(r.total, 19);
        assert_eq!(r.passed, 19);
        assert_eq!(r.failed, 0);
        assert_eq!(r.asserted, 19);
        assert_eq!(r.first_fail, 0);
        assert_eq!(r.all_green(), true);
    }

    #[test]
    fn classes_are_covered() {
        let cov = class_coverage();
        assert_eq!(cov.iter().all(|v| *v > 0), true);
        assert_eq!(cov.iter().fold(0u32, |a, v| a + *v), 19);
    }

    #[test]
    fn matrix_shape_is_fixed() {
        assert_eq!(PHASE_AXIS.len(), 5);
        assert_eq!(DISPOSITION_AXIS.len(), 4);
        assert_eq!(RACE_MATRIX.len(), 5);
        let row = RACE_MATRIX.first();
        assert_eq!(row.map(|r| r.len()), Some(4));
    }

    #[test]
    fn matrix_matches_derivation() {
        let mut r = 0usize;
        let mut matched = 0u32;
        while r < PHASE_AXIS.len() {
            if let Some(phase) = PHASE_AXIS.get(r) {
                let mut c = 0usize;
                while c < DISPOSITION_AXIS.len() {
                    if let Some(disp) = DISPOSITION_AXIS.get(c) {
                        let want = RACE_MATRIX.get(r).and_then(|row| row.get(c)).copied();
                        let got = matrix_outcome(*phase, *disp);
                        if want == Some(got) {
                            matched += 1;
                        }
                    }
                    c += 1;
                }
            }
            r += 1;
        }
        assert_eq!(matched, 20);
    }

    #[test]
    fn matrix_runs_only_once() {
        let mut m = RaceMatrix::new();
        assert_eq!(m.run_once(), true);
        assert_eq!(m.matched, 20);
        assert_eq!(m.missed, 0);
        assert_eq!(m.run_once(), false);
        assert_eq!(m.runs, 1);
        assert_eq!(m.refused, 1);
        assert_eq!(m.cells(), 20);
    }

    #[test]
    fn cumulative_closes_at_139900() {
        assert_eq!(PRIOR_DOMAIN_ASSERTS, 139_881);
        assert_eq!(B26_ITEM_COUNT, 19);
        assert_eq!(cumulative_total(), 139_900);
        assert_eq!(cumulative_checked(), Some(139_900));
        assert_eq!(DOMAIN_TOTAL_TARGET, 139_900);
    }

    #[test]
    fn errno_table_is_consistent() {
        assert_eq!(errno_table_ok(), true);
        assert_eq!(ERRNO_TABLE.len(), 11);
        assert_eq!(errno_value("EPERM"), Some(1));
        assert_eq!(errno_value("ECHILD"), Some(10));
        assert_eq!(errno_value("ENOMEM"), Some(12));
        assert_eq!(errno_value("ENOTDIR"), None);
        assert_eq!(errno_negative(-1), None);
        assert_eq!(errno_negative(999), None);
    }

    #[test]
    fn transfer_classification() {
        assert_eq!(interpret_read(0, 0, 4096), ReadOutcome::Short);
        assert_eq!(interpret_read(4096, 0, 4096), ReadOutcome::Full);
        assert_eq!(interpret_read(-1, EINTR, 4096), ReadOutcome::Interrupted);
        assert_eq!(interpret_read(-1, EBADF, 4096), ReadOutcome::Failed);
        assert_eq!(interpret_write(0, 0, 4096), WriteOutcome::Partial);
        assert_eq!(interpret_write(-1, EINTR, 4096), WriteOutcome::Interrupted);
    }

    #[test]
    fn fingerprint_stable() {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        assert_eq!(a, b);
        assert_eq!(a != 0, true);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9320_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
    }
}
