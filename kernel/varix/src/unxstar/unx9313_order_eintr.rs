//! UNX-F9313 EINTR 判定次序协议（先完成后中断） · 0x7BBC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9313 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9313-J1 判序协议判据过，完成优先于
//!   中断逐格检出，与 Linux 语义一致格过；③ 完成时 API/软件写回：状态=已完成、
//!   结果摘要非空。
//!
//! 设计要点：
//!   ① 判定不是「一堆 if」，而是一条**带序的闸门链**：Param（入口错误）→
//!      Complete（已搬运字节 / 子进程状态变化）→ Eof（read 的 0 返回）→
//!      Interrupt（信号）→ Block（会阻塞）→ Return（零长完成）。每个闸门有
//!      唯一的秩，秩只能严格递增；每次判定把走过的闸门按位记进 mask，
//!      于是「次序」本身成为可观测量，而不是注释里的口头承诺。
//!   ② 「先完成后中断」的可执行含义：在 6 个完成格（xfer>0 或 eof）里，
//!      Interrupt 闸门**不得被求值**（mask 里不许出现 INTERRUPT 位）。只要
//!      实现把信号判断提到完成判断之前，即使返回值碰巧对，mask 也会暴露它。
//!   ③ 错误门最前且只认四个入口级 errno（EBADF/EINVAL/EFAULT/ENOSYS），
//!      表外 errno 一律归 EINVAL；err != 0 却 xfer > 0 是**自相矛盾证据**，
//!      回 EINVAL 并在审计里记一笔不变量违例（半程 EFAULT 属于 F9311 的
//!      语义，不在本模块冒充）。
//!   ④ 8 种典型越序/丢信号实现（中断优先、恒中断、忽略信号、会阻塞当成功、
//!      部分完成当失败、错误门后置、恒重启、唤醒后丢信号）各自作为一条独立
//!      策略函数，在 12 格矩阵上逐格比对：每种至少被检出 1 格，检出账本
//!      injected = detected + escaped 且 escaped = 0。
//!   ⑤ 与 Linux 语义对齐靠 10 行 syscall 规则表：read/write/recv/send/
//!      select/poll/semop 是字节型（部分完成即完成、EOF 是完成），wait4 是
//!      子进程型，nanosleep 是零进展型，connect 是状态机型。每种发一条探针
//!      证据，判定结果必须与规则表声明的形状一致。
//!   ⑥ 诊断码段内闭集 12 个（+0x00..=+0x0B），码值两两唯一，每码一句处置。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9313 独占 0x7BBC0..=0x7BBFF 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7BBC0;
/// 邻卡下界（F9312 = 0x7BB80）。
pub const SEG_PREV: u32 = 0x7BB80;
/// 族窗口上界（含）。
pub const SEG_LIMIT: u32 = 0x7D1C0;

/// errno：被信号中断。
pub const EINTR: i32 = 4;
/// errno：坏文件描述符。
pub const EBADF: i32 = 9;
/// errno：坏用户指针。
pub const EFAULT: i32 = 14;
/// errno：参数非法。
pub const EINVAL: i32 = 22;
/// errno：再次尝试（非阻塞或部分完成后的重试形态）。
pub const EAGAIN: i32 = 11;
/// errno：功能未实现（入口级错误的一族）。
pub const ENOSYS: i32 = 38;

/// 冻结矩阵格数。
pub const CELL_COUNT: usize = 12;
/// 缺陷策略数。
pub const POLICY_COUNT: usize = 8;
/// syscall 规则行数。
pub const RULE_COUNT: usize = 10;
/// 单格审计轮数（每格复算两次：一次取判定，一次取 mask）。
pub const AUDIT_ROUNDS: u32 = 2;

/// 判定类别编码：完成。
pub const CLASS_DONE: u32 = 0;
/// 判定类别编码：错误。
pub const CLASS_ERR: u32 = 1;
/// 判定类别编码：中断。
pub const CLASS_EINTR: u32 = 2;
/// 判定类别编码：会阻塞。
pub const CLASS_AGAIN: u32 = 3;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 判序诊断码（段内 +0x00..=+0x0B 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrderCode {
    /// 完成格仍走了中断门。
    CompleteMasked,
    /// 中断格却带着字节返回。
    EintrWithBytes,
    /// 闸门秩乱序。
    StageOrder,
    /// 完成类判定错误。
    ClassDrift,
    /// 字节数被截断或篡改。
    BytesDrift,
    /// 自相矛盾证据未被识别。
    Contradiction,
    /// 缺陷策略整条未被检出。
    PolicyEscaped,
    /// syscall 规则表与判定器不一致。
    RuleDrift,
    /// 表外 errno 未归一。
    ErrnoDomain,
    /// 双跑不一致。
    NotReproducible,
    /// 冻结矩阵行数漂移。
    MatrixDrift,
    /// 段位越界。
    SegmentViolation,
}

impl OrderCode {
    /// 段内码号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 绝对码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            OrderCode::CompleteMasked => "完成格仍求值了中断门：把 Compare→Complete→Eof→Interrupt 的闸序钉死",
            OrderCode::EintrWithBytes => "带字节返回 EINTR 会丢数据：只要有一字节搬运完成就必须返回字节数",
            OrderCode::StageOrder => "闸门秩必须严格递增：Param<Complete<Eof<Interrupt<Block<Return",
            OrderCode::ClassDrift => "判定类别与冻结矩阵不符：先确认闸序再看增减条件",
            OrderCode::BytesDrift => "返回字节被截断：Done 必须原样携带已提交字节数",
            OrderCode::Contradiction => "err != 0 与 xfer > 0 同时成立属自相矛盾证据：应显性回 EINVAL",
            OrderCode::PolicyEscaped => "缺陷策略逃过了矩阵：说明 12 格覆盖不足，补格而非补断言",
            OrderCode::RuleDrift => "syscall 规则表与判定器分叉：规则表是外部锚点，先改判据实现",
            OrderCode::ErrnoDomain => "入口级 errno 只认 EBADF/EINVAL/EFAULT/ENOSYS，表外归一为 EINVAL",
            OrderCode::NotReproducible => "同证据两次判定必须一致，禁止依赖遍历顺序",
            OrderCode::MatrixDrift => "冻结矩阵行数必须为 12，删格先改账",
            OrderCode::SegmentViolation => "段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: &[OrderCode] = &[
        OrderCode::CompleteMasked,
        OrderCode::EintrWithBytes,
        OrderCode::StageOrder,
        OrderCode::ClassDrift,
        OrderCode::BytesDrift,
        OrderCode::Contradiction,
        OrderCode::PolicyEscaped,
        OrderCode::RuleDrift,
        OrderCode::ErrnoDomain,
        OrderCode::NotReproducible,
        OrderCode::MatrixDrift,
        OrderCode::SegmentViolation,
    ];
}

// ── 闸门与判定 ────────────────────────────────────────────────────────
/// 判定闸门（次序即语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 入口错误门（EBADF/EINVAL/EFAULT/ENOSYS）。
    Param,
    /// 完成门（已搬运字节 / 子进程状态）。
    Complete,
    /// EOF 门（read 的 0 返回）。
    Eof,
    /// 中断门。
    Interrupt,
    /// 会阻塞门。
    Block,
    /// 零长完成门。
    Return,
}

impl Stage {
    /// 六个闸门的秩（严格递增，成对不重复）。
    pub fn rank(self) -> u8 {
        match self {
            Stage::Param => 0,
            Stage::Complete => 1,
            Stage::Eof => 2,
            Stage::Interrupt => 3,
            Stage::Block => 4,
            Stage::Return => 5,
        }
    }

    /// 位掩码（审计用）。
    pub fn bit(self) -> u32 {
        1u32 << self.rank()
    }

    /// 闭集全量（秩单调判据遍历它）。
    pub const ALL: [Stage; 6] = [
        Stage::Param,
        Stage::Complete,
        Stage::Eof,
        Stage::Interrupt,
        Stage::Block,
        Stage::Return,
    ];

    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Param => "PARAM",
            Stage::Complete => "COMPLETE",
            Stage::Eof => "EOF",
            Stage::Interrupt => "INTERRUPT",
            Stage::Block => "BLOCK",
            Stage::Return => "RETURN",
        }
    }
}

/// 信号到达的时刻（判定器只看「是否到达」，时刻仅用于策略缺陷建模）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SigAt {
    /// 未到达。
    None,
    /// 进入阻塞之前到达（此时尚未搬运任何字节）。
    BeforeBlock,
    /// 阻塞中到达。
    InBlock,
    /// 被唤醒之后到达。
    AfterWake,
}

impl SigAt {
    /// 信号是否已到达。
    pub fn arrived(self) -> bool {
        !matches!(self, SigAt::None)
    }
}

/// 一个判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 完成，携带已提交字节数（子进程完成按 1 计）。
    Done(u32),
    /// 错误（入口级 errno）。
    Err(i32),
    /// 被信号中断，零进展。
    Eintr,
    /// 会阻塞 / 需重试。
    Again,
}

impl Verdict {
    /// 类别编码（冻结矩阵比对用）。
    pub fn class(self) -> u32 {
        match self {
            Verdict::Done(_) => CLASS_DONE,
            Verdict::Err(_) => CLASS_ERR,
            Verdict::Eintr => CLASS_EINTR,
            Verdict::Again => CLASS_AGAIN,
        }
    }

    /// 携带的字节数（非完成形态为 0）。
    pub fn bytes(self) -> u32 {
        match self {
            Verdict::Done(n) => n,
            _ => 0,
        }
    }

    /// 是否为中断。
    pub fn is_eintr(self) -> bool {
        matches!(self, Verdict::Eintr)
    }

    /// 是否为完成。
    pub fn is_done(self) -> bool {
        matches!(self, Verdict::Done(_))
    }
}

// ── 证据 ──────────────────────────────────────────────────────────────
/// 一次系统调用返回前可观测到的全部证据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 已搬运/已产生的字节（子进程型为 0）。
    pub xfer: u32,
    /// 本次调用请求的字节数（0 表示零长调用）。
    pub want: u32,
    /// 信号到达时刻。
    pub sig_at: SigAt,
    /// 入口级错误（0 表示无）。
    pub err: i32,
    /// 是否观察到 EOF。
    pub eof: bool,
    /// 是否本应阻塞。
    pub would_block: bool,
    /// 是否观察到子进程状态变化。
    pub child: bool,
}

impl Evidence {
    /// 空白证据（表构造锚点）。
    pub const BLANK: Evidence = Evidence {
        xfer: 0,
        want: 0,
        sig_at: SigAt::None,
        err: 0,
        eof: false,
        would_block: false,
        child: false,
    };

    /// 信号是否已到达。
    pub fn signalled(&self) -> bool {
        self.sig_at.arrived()
    }

    /// 是否存在完成形态（字节或子进程状态）。
    pub fn completed(&self) -> bool {
        self.xfer > 0 || self.child
    }

    /// 自相矛盾：入口级错误不可能与已搬运字节并存。
    pub fn contradictory(&self) -> bool {
        self.err != 0 && self.xfer > 0
    }

    /// 部分完成：搬运了，但不足请求量。
    pub fn partial(&self) -> bool {
        self.xfer > 0 && self.xfer < self.want
    }
}

// ── 冻结矩阵（12 格） ─────────────────────────────────────────────────
/// 矩阵一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    /// 格号（1..=12）。
    pub id: u32,
    /// 证据。
    pub ev: Evidence,
    /// 期望类别。
    pub class: u32,
    /// 期望字节数。
    pub bytes: u32,
    /// 是否为「完成优先」格（mask 里不得出现 INTERRUPT 位）。
    pub priority_done: bool,
}

/// 十二格冻结矩阵。
///
/// 覆盖维度：完整完成 / 部分完成 / EOF / 零长 / 中断（三时刻）/
/// 会阻塞 / 两种入口错误（其一为 err 与 xfer 并存的自相矛盾证据格，
/// 审计按设计要点③在矩阵里记一笔不变量违例，同时给「错误门后置」
/// 缺陷策略一个见证格）；「完成优先」格 6 个（1,2,3,4,9,10）。
pub const CELLS: [Cell; CELL_COUNT] = [
    Cell {
        id: 1,
        ev: Evidence { xfer: 64, want: 64, sig_at: SigAt::None, err: 0, eof: false, would_block: false, child: false },
        class: CLASS_DONE,
        bytes: 64,
        priority_done: true,
    },
    Cell {
        id: 2,
        ev: Evidence { xfer: 64, want: 64, sig_at: SigAt::InBlock, err: 0, eof: false, would_block: false, child: false },
        class: CLASS_DONE,
        bytes: 64,
        priority_done: true,
    },
    Cell {
        id: 3,
        ev: Evidence { xfer: 64, want: 64, sig_at: SigAt::AfterWake, err: 0, eof: false, would_block: false, child: false },
        class: CLASS_DONE,
        bytes: 64,
        priority_done: true,
    },
    Cell {
        id: 4,
        ev: Evidence { xfer: 32, want: 64, sig_at: SigAt::InBlock, err: 0, eof: false, would_block: false, child: false },
        class: CLASS_DONE,
        bytes: 32,
        priority_done: true,
    },
    Cell {
        id: 5,
        ev: Evidence { xfer: 0, want: 64, sig_at: SigAt::InBlock, err: 0, eof: false, would_block: false, child: false },
        class: CLASS_EINTR,
        bytes: 0,
        priority_done: false,
    },
    Cell {
        id: 6,
        ev: Evidence { xfer: 0, want: 64, sig_at: SigAt::AfterWake, err: 0, eof: false, would_block: true, child: false },
        class: CLASS_EINTR,
        bytes: 0,
        priority_done: false,
    },
    Cell {
        id: 7,
        ev: Evidence { xfer: 0, want: 64, sig_at: SigAt::None, err: 0, eof: false, would_block: true, child: false },
        class: CLASS_AGAIN,
        bytes: 0,
        priority_done: false,
    },
    Cell {
        id: 8,
        ev: Evidence { xfer: 0, want: 0, sig_at: SigAt::None, err: 0, eof: false, would_block: false, child: false },
        class: CLASS_DONE,
        bytes: 0,
        priority_done: false,
    },
    Cell {
        id: 9,
        ev: Evidence { xfer: 0, want: 64, sig_at: SigAt::None, err: 0, eof: true, would_block: false, child: false },
        class: CLASS_DONE,
        bytes: 0,
        priority_done: true,
    },
    Cell {
        id: 10,
        ev: Evidence { xfer: 0, want: 64, sig_at: SigAt::InBlock, err: 0, eof: true, would_block: false, child: false },
        class: CLASS_DONE,
        bytes: 0,
        priority_done: true,
    },
    Cell {
        // 自相矛盾证据格：EBADF 与已搬运字节并存。正解在参数门内显性回
        // EINVAL（要点③）；错误门后置的缺陷策略会把它当 Done(8) 放行，
        // 这一格即该策略的见证格，也让审计的矛盾计数落到矩阵上。
        id: 11,
        ev: Evidence { xfer: 8, want: 64, sig_at: SigAt::None, err: EBADF, eof: false, would_block: false, child: false },
        class: CLASS_ERR,
        bytes: 0,
        priority_done: false,
    },
    Cell {
        id: 12,
        ev: Evidence { xfer: 0, want: 64, sig_at: SigAt::InBlock, err: EFAULT, eof: false, would_block: false, child: false },
        class: CLASS_ERR,
        bytes: 0,
        priority_done: false,
    },
];

// ── 正确判定器（闸序） ────────────────────────────────────────────────
/// 入口级 errno 归一：四个受认码之外一律 EINVAL。
pub fn normalize_entry_errno(err: i32) -> i32 {
    match err {
        EBADF | EFAULT | EINVAL | ENOSYS => err,
        _ => EINVAL,
    }
}

/// 正确闸序判定，同时返回走过的闸门掩码。
pub fn ordered_traced(ev: &Evidence) -> (Verdict, u32) {
    let mut mask = 0u32;

    // ① 入口错误门：任何数据动作之前。
    mask |= Stage::Param.bit();
    if ev.err != 0 {
        if ev.contradictory() {
            // 错误与字节并存是自相矛盾证据，不能靠「谁先判」蒙混过去。
            return (Verdict::Err(EINVAL), mask);
        }
        return (Verdict::Err(normalize_entry_errno(ev.err)), mask);
    }

    // ② 完成门：只要有字节或子进程状态变化，就是完成。
    mask |= Stage::Complete.bit();
    if ev.xfer > 0 {
        return (Verdict::Done(ev.xfer), mask);
    }
    if ev.child {
        return (Verdict::Done(1), mask);
    }

    // ③ EOF 门：read 的 0 返回是完成，而不是「无进展」。
    mask |= Stage::Eof.bit();
    if ev.eof {
        return (Verdict::Done(0), mask);
    }

    // ④ 中断门：到这里才允许看信号。
    mask |= Stage::Interrupt.bit();
    if ev.signalled() {
        return (Verdict::Eintr, mask);
    }

    // ⑤ 会阻塞门。
    mask |= Stage::Block.bit();
    if ev.would_block {
        return (Verdict::Again, mask);
    }

    // ⑥ 零长调用完成。
    mask |= Stage::Return.bit();
    (Verdict::Done(0), mask)
}

/// 正确闸序判定（只看结果）。
pub fn ordered_eval(ev: &Evidence) -> Verdict {
    ordered_traced(ev).0
}

/// 越序实现：先看信号，再看完成。
///
/// 这条路径**不是**用来当正确实现的，而是用来量化「次序」值多少格：
/// 它在 4 个格（2/3/4/10）上把已完成的调用错报成 EINTR，每格都是数据丢失。
pub fn eintr_first_eval(ev: &Evidence) -> Verdict {
    if ev.err != 0 {
        if ev.contradictory() {
            return Verdict::Err(EINVAL);
        }
        return Verdict::Err(normalize_entry_errno(ev.err));
    }
    if ev.signalled() {
        return Verdict::Eintr;
    }
    if ev.xfer > 0 {
        return Verdict::Done(ev.xfer);
    }
    if ev.child {
        return Verdict::Done(1);
    }
    if ev.eof {
        return Verdict::Done(0);
    }
    if ev.would_block {
        return Verdict::Again;
    }
    Verdict::Done(0)
}

// ── 缺陷策略族 ────────────────────────────────────────────────────────
/// 八种典型缺陷实现。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Policy {
    /// 中断优先（同 eintr_first_eval）。
    EintrFirst,
    /// 恒回 EINTR。
    AlwaysEintr,
    /// 永不看信号。
    IgnoreSignal,
    /// 会阻塞当成功。
    BlockAsSuccess,
    /// 部分完成当失败。
    PartialAsFailure,
    /// 错误门后置（先看完成）。
    ErrAfterData,
    /// 恒重启（把中断当可重试）。
    RestartAlways,
    /// 唤醒后丢信号。
    SignalLostAfterWake,
}

impl Policy {
    /// 策略全量。
    pub const ALL: [Policy; POLICY_COUNT] = [
        Policy::EintrFirst,
        Policy::AlwaysEintr,
        Policy::IgnoreSignal,
        Policy::BlockAsSuccess,
        Policy::PartialAsFailure,
        Policy::ErrAfterData,
        Policy::RestartAlways,
        Policy::SignalLostAfterWake,
    ];

    /// 策略名。
    pub fn name(self) -> &'static str {
        match self {
            Policy::EintrFirst => "中断优先",
            Policy::AlwaysEintr => "恒中断",
            Policy::IgnoreSignal => "忽略信号",
            Policy::BlockAsSuccess => "会阻塞当成功",
            Policy::PartialAsFailure => "部分完成当失败",
            Policy::ErrAfterData => "错误门后置",
            Policy::RestartAlways => "恒重启",
            Policy::SignalLostAfterWake => "唤醒后丢信号",
        }
    }

    /// 按该缺陷策略给出判定。
    pub fn verdict(self, ev: &Evidence) -> Verdict {
        match self {
            Policy::EintrFirst => eintr_first_eval(ev),
            Policy::AlwaysEintr => Verdict::Eintr,
            Policy::IgnoreSignal => {
                let mut e = *ev;
                e.sig_at = SigAt::None;
                ordered_eval(&e)
            }
            Policy::BlockAsSuccess => {
                let mut e = *ev;
                e.would_block = false;
                ordered_eval(&e)
            }
            Policy::PartialAsFailure => {
                if ev.partial() {
                    Verdict::Again
                } else {
                    ordered_eval(ev)
                }
            }
            Policy::ErrAfterData => {
                if ev.xfer > 0 || ev.child || ev.eof {
                    Verdict::Done(ev.xfer)
                } else if ev.err != 0 {
                    Verdict::Err(normalize_entry_errno(ev.err))
                } else {
                    ordered_eval(ev)
                }
            }
            Policy::RestartAlways => {
                if ev.signalled() && ev.xfer == 0 && !ev.eof && ev.err == 0 {
                    Verdict::Again
                } else {
                    ordered_eval(ev)
                }
            }
            Policy::SignalLostAfterWake => {
                if ev.sig_at == SigAt::AfterWake {
                    let mut e = *ev;
                    e.sig_at = SigAt::None;
                    ordered_eval(&e)
                } else {
                    ordered_eval(ev)
                }
            }
        }
    }
}

// ── syscall 规则表 ────────────────────────────────────────────────────
/// syscall 语义类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// 字节型：部分完成即完成，EOF 是完成。
    Bytes,
    /// 子进程型：状态变化即完成。
    Child,
    /// 零进展型：只有中断或完成。
    Sleep,
    /// 状态机型：建立连接前不产生字节。
    Connect,
}

impl Kind {
    /// 部分完成是否算完成。
    pub fn partial_completes(self) -> bool {
        matches!(self, Kind::Bytes)
    }

    /// EOF 是否算完成。
    pub fn eof_completes(self) -> bool {
        matches!(self, Kind::Bytes)
    }

    /// 子进程状态变化是否算完成。
    pub fn child_completes(self) -> bool {
        matches!(self, Kind::Child)
    }
}

/// 一条 syscall 规则。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule {
    /// 名字。
    pub name: &'static str,
    /// 语义类别。
    pub kind: Kind,
}

/// 十行规则表（Linux 语义锚点）。
pub const RULES: [Rule; RULE_COUNT] = [
    Rule { name: "read", kind: Kind::Bytes },
    Rule { name: "write", kind: Kind::Bytes },
    Rule { name: "recv", kind: Kind::Bytes },
    Rule { name: "send", kind: Kind::Bytes },
    Rule { name: "select", kind: Kind::Bytes },
    Rule { name: "poll", kind: Kind::Bytes },
    Rule { name: "semop", kind: Kind::Bytes },
    Rule { name: "wait4", kind: Kind::Child },
    Rule { name: "nanosleep", kind: Kind::Sleep },
    Rule { name: "connect", kind: Kind::Connect },
];

/// 为类别构造探针证据（部分完成 + 唤醒后信号：最强区分度）。
pub fn rule_probe(kind: Kind) -> Evidence {
    match kind {
        Kind::Bytes => Evidence {
            xfer: 16,
            want: 32,
            sig_at: SigAt::AfterWake,
            err: 0,
            eof: false,
            would_block: false,
            child: false,
        },
        Kind::Child => Evidence {
            xfer: 0,
            want: 0,
            sig_at: SigAt::AfterWake,
            err: 0,
            eof: false,
            would_block: false,
            child: true,
        },
        Kind::Sleep => Evidence {
            xfer: 0,
            want: 0,
            sig_at: SigAt::InBlock,
            err: 0,
            eof: false,
            would_block: false,
            child: false,
        },
        Kind::Connect => Evidence {
            xfer: 0,
            want: 0,
            sig_at: SigAt::BeforeBlock,
            err: 0,
            eof: false,
            would_block: false,
            child: false,
        },
    }
}

/// 该类别在探针下**应当**得到的形状（类别编码 + 字节数）。
pub fn rule_want(kind: Kind) -> (u32, u32) {
    match kind {
        Kind::Bytes => (CLASS_DONE, 16),
        Kind::Child => (CLASS_DONE, 1),
        Kind::Sleep => (CLASS_EINTR, 0),
        Kind::Connect => (CLASS_EINTR, 0),
    }
}

// ── 审计与账本 ────────────────────────────────────────────────────────
/// 次序审计结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Audit {
    /// 矩阵格数。
    pub cells: u32,
    /// 与冻结表逐格一致的格数。
    pub matched: u32,
    /// 完成格里仍求值了中断门的格数（必须为 0）。
    pub order_violations: u32,
    /// 矛盾证据被识别次数。
    pub contradictions: u32,
}

/// 缺陷策略注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PolicyLedger {
    /// 注入的缺陷策略数。
    pub injected: u32,
    /// 被矩阵检出的策略数。
    pub detected: u32,
    /// 逃逸数（必须为 0）。
    pub escaped: u32,
    /// 逐策略检出格数之和。
    pub witness_cells: u32,
}

impl PolicyLedger {
    /// 零账。
    pub const fn zero() -> PolicyLedger {
        PolicyLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
            witness_cells: 0,
        }
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 跑一遍 12 格审计。
pub fn run_audit() -> Audit {
    let mut cells = 0u32;
    let mut matched = 0u32;
    let mut violations = 0u32;
    let mut contradictions = 0u32;
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(cell) = CELLS.get(i) {
            cells = cells.saturating_add(1);
            let mut round = 0u32;
            while round < AUDIT_ROUNDS {
                let (v, mask) = ordered_traced(&cell.ev);
                if v.class() == cell.class && v.bytes() == cell.bytes {
                    matched = matched.saturating_add(1);
                }
                if cell.priority_done && (mask & Stage::Interrupt.bit()) != 0 {
                    violations = violations.saturating_add(1);
                }
                round += 1;
            }
            if cell.ev.contradictory() {
                contradictions = contradictions.saturating_add(1);
            }
        }
        i += 1;
    }
    Audit {
        cells,
        matched,
        order_violations: violations,
        contradictions,
    }
}

/// 把一个 defect 策略跑过整张矩阵，返回（检出格数, 总格数）。
pub fn policy_witness(p: Policy) -> (u32, u32) {
    let mut hits = 0u32;
    let mut total = 0u32;
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(cell) = CELLS.get(i) {
            total = total.saturating_add(1);
            if p.verdict(&cell.ev) != ordered_eval(&cell.ev) {
                hits = hits.saturating_add(1);
            }
        }
        i += 1;
    }
    (hits, total)
}

/// 八策略注入账。
pub fn run_policy_injection() -> PolicyLedger {
    let mut led = PolicyLedger::zero();
    let mut i = 0usize;
    while i < Policy::ALL.len() {
        if let Some(p) = Policy::ALL.get(i) {
            led.injected = led.injected.saturating_add(1);
            let (hits, _) = policy_witness(*p);
            led.witness_cells = led.witness_cells.saturating_add(hits);
            if hits > 0 {
                led.detected = led.detected.saturating_add(1);
            } else {
                led.escaped = led.escaped.saturating_add(1);
            }
        }
        i += 1;
    }
    led
}

/// 越序实现的整体分歧格数（应恰为 4）。
pub fn eintr_first_divergences() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(cell) = CELLS.get(i) {
            if eintr_first_eval(&cell.ev) != ordered_eval(&cell.ev) {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

/// 12 格判定指纹：逐格喂 (id, class, bytes, mask)。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(cell) = CELLS.get(i) {
            let (v, mask) = ordered_traced(&cell.ev);
            bytes.push((cell.id & 0xFF) as u8);
            bytes.push((v.class() & 0xFF) as u8);
            bytes.push((v.bytes() & 0xFF) as u8);
            bytes.push((mask & 0xFF) as u8);
            bytes.push(((mask >> 8) & 0xFF) as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9313 判据集（add ≥ 12；add_dyn ≤ 8）。
pub fn run_unx9313_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9313");

    // ① 段位。
    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE > SEG_PREV
        && SEG_BASE < SEG_LIMIT
        && SEG_BASE + 0x40 <= SEG_LIMIT;
    s.add(
        "段位守卫：0x40 对齐且不与 F9312 重叠",
        seg_ok,
        "段基址唯一且落在本批窗口，段间不得互相覆盖",
    );

    // ② 闸门秩严格单调。
    let mut rank_ok = Stage::ALL.len() == 6;
    let mut i = 0usize;
    while i < Stage::ALL.len() {
        if let Some(st) = Stage::ALL.get(i) {
            if st.rank() != i as u8 {
                rank_ok = false;
            }
            if st.bit() != 1u32 << (i as u32) {
                rank_ok = false;
            }
            let mut j = i + 1;
            while j < Stage::ALL.len() {
                if let Some(other) = Stage::ALL.get(j) {
                    if other.rank() <= st.rank() {
                        rank_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add(
        "闸序：六闸门秩严格递增且位掩码一一对应",
        rank_ok,
        "Param<Complete<Eof<Interrupt<Block<Return 是判序协议的全部内容",
    );

    // ③ 12 格逐格命中冻结矩阵（类别 + 字节）。
    let audit = run_audit();
    let matrix_ok = audit.cells == CELL_COUNT as u32
        && audit.matched == (CELL_COUNT as u32) * AUDIT_ROUNDS;
    s.add(
        "矩阵：12 格 × 2 轮全部命中冻结期望",
        matrix_ok,
        "每格的类别与字节数都必须与冻结表一致，任何一格漂移即失败",
    );

    // ④ 完成优先：6 个完成格不求值中断门。
    let mut priority_cells = 0u32;
    let mut priority_ok = true;
    let mut ci = 0usize;
    while ci < CELLS.len() {
        if let Some(cell) = CELLS.get(ci) {
            if cell.priority_done {
                priority_cells = priority_cells.saturating_add(1);
                let (v, mask) = ordered_traced(&cell.ev);
                if !v.is_done() || (mask & Stage::Interrupt.bit()) != 0 {
                    priority_ok = false;
                }
            }
        }
        ci += 1;
    }
    s.add(
        "完成优先：6 个完成格的判定路径不含中断门",
        priority_ok && priority_cells == 6 && audit.order_violations == 0,
        "「先完成后中断」的可执行判据是 mask 里没有 INTERRUPT 位，而不是返回值碰巧",
    );

    // ⑤ 中断只在零进展时出现。
    let mut eintr_ok = true;
    let mut eintr_cells = 0u32;
    let mut k = 0usize;
    while k < CELLS.len() {
        if let Some(cell) = CELLS.get(k) {
            let v = ordered_eval(&cell.ev);
            if v.is_eintr() {
                eintr_cells = eintr_cells.saturating_add(1);
                if cell.ev.xfer != 0 || cell.ev.eof || cell.ev.err != 0 {
                    eintr_ok = false;
                }
            }
        }
        k += 1;
    }
    s.add(
        "中断语义：EINTR 只出现在零进展格（2 格）",
        eintr_ok && eintr_cells == 2,
        "只要有过字节、EOF 或入口错误，就不得回 EINTR",
    );

    // ⑥ 部分完成算完成。
    let cell4 = CELLS.get(3).copied();
    let part_ok = match cell4 {
        Some(c) => {
            let v = ordered_eval(&c.ev);
            c.ev.partial() && v == Verdict::Done(32)
        }
        None => false,
    };
    s.add(
        "部分完成：32/64 字节 + 信号 → Done(32)",
        part_ok,
        "半程拷贝后中断不得丢数据，必须返回已拷贝字节数",
    );

    // ⑦ EOF 是完成，且优先于信号。
    let mut eof_ok = true;
    let mut eof_cells = 0u32;
    let mut m = 0usize;
    while m < CELLS.len() {
        if let Some(cell) = CELLS.get(m) {
            if cell.ev.eof {
                eof_cells = eof_cells.saturating_add(1);
                if ordered_eval(&cell.ev) != Verdict::Done(0) {
                    eof_ok = false;
                }
            }
        }
        m += 1;
    }
    s.add(
        "EOF：两个 EOF 格均为 Done(0)（含带信号格）",
        eof_ok && eof_cells == 2,
        "read 的 0 返回是完成形态，信号不得把它改写成 EINTR",
    );

    // ⑧ 错误门最前：带信号的 EFAULT 仍回 EFAULT。
    let cell12 = CELLS.get(11).copied();
    let err_ok = match cell12 {
        Some(c) => {
            let (v, mask) = ordered_traced(&c.ev);
            v == Verdict::Err(EFAULT) && (mask & Stage::Interrupt.bit()) == 0
        }
        None => false,
    };
    s.add(
        "错误门最前：EFAULT + 信号仍回 EFAULT 且不进中断门",
        err_ok,
        "入口级错误在任何数据动作与信号判断之前，否则会把 EFAULT 伪装成 EINTR",
    );

    // ⑨ 矛盾证据（err 与 xfer 并存）显性回 EINVAL。
    let contra = Evidence {
        xfer: 8,
        want: 16,
        sig_at: SigAt::InBlock,
        err: EFAULT,
        eof: false,
        would_block: false,
        child: false,
    };
    let contra_ok = contra.contradictory()
        && ordered_eval(&contra) == Verdict::Err(EINVAL)
        && Policy::ErrAfterData.verdict(&contra).class() == CLASS_DONE;
    s.add(
        "矛盾证据：err 与 xfer 并存回 EINVAL 且被策略反例命中",
        contra_ok,
        "自相矛盾证据必须显性拒绝；错误门后置的实现会把它当成功",
    );

    // ⑩ 越序实现恰在 4 格分歧。
    let div = eintr_first_divergences();
    let naive_ok = div == 4
        && eintr_first_eval(&Evidence { xfer: 10, want: 10, sig_at: SigAt::InBlock, ..Evidence::BLANK })
            == Verdict::Eintr
        && ordered_eval(&Evidence { xfer: 10, want: 10, sig_at: SigAt::InBlock, ..Evidence::BLANK })
            == Verdict::Done(10);
    s.add(
        "越序反例：中断优先实现在 4 个完成格上分歧",
        naive_ok,
        "分歧格数就是「次序」本身的价值：每格都是一次数据丢失",
    );

    // ⑪ 8 策略全部检出，逃逸为 0。
    let pl = run_policy_injection();
    let mut each_ok = true;
    let mut pi = 0usize;
    while pi < Policy::ALL.len() {
        if let Some(p) = Policy::ALL.get(pi) {
            let (hits, total) = policy_witness(*p);
            if hits == 0 || total != CELL_COUNT as u32 {
                each_ok = false;
            }
        }
        pi += 1;
    }
    s.add(
        "缺陷注入：8 种越序实现各自至少被检出 1 格",
        each_ok
            && pl.injected == POLICY_COUNT as u32
            && pl.detected == POLICY_COUNT as u32
            && pl.escaped == 0
            && pl.conserved(),
        "每种缺陷都必须有一个见证格，账本 injected = detected + escaped 且逃逸为零",
    );

    // ⑫ 10 行 syscall 规则与判定器一致。
    let mut rule_ok = RULES.len() == RULE_COUNT;
    let mut bytes_rows = 0u32;
    let mut child_rows = 0u32;
    let mut sleep_rows = 0u32;
    let mut connect_rows = 0u32;
    let mut ri = 0usize;
    while ri < RULES.len() {
        if let Some(rule) = RULES.get(ri) {
            let probe = rule_probe(rule.kind);
            let v = ordered_eval(&probe);
            let (want_class, want_bytes) = rule_want(rule.kind);
            if v.class() != want_class || v.bytes() != want_bytes {
                rule_ok = false;
            }
            // 类别谓词必须自洽：EOF 完成与部分完成同域（仅字节型），
            // 子进程完成只属于子进程型——两侧不一致说明规则表被改坏。
            if rule.kind.eof_completes() != rule.kind.partial_completes() {
                rule_ok = false;
            }
            if rule.kind.child_completes() != matches!(rule.kind, Kind::Child) {
                rule_ok = false;
            }
            match rule.kind {
                Kind::Bytes => bytes_rows = bytes_rows.saturating_add(1),
                Kind::Child => child_rows = child_rows.saturating_add(1),
                Kind::Sleep => sleep_rows = sleep_rows.saturating_add(1),
                Kind::Connect => connect_rows = connect_rows.saturating_add(1),
            }
        }
        ri += 1;
    }
    let shape_ok = bytes_rows == 7 && child_rows == 1 && sleep_rows == 1 && connect_rows == 1;
    s.add(
        "Linux 语义：10 行 syscall 规则表与判定器逐行一致",
        rule_ok && shape_ok,
        "规则表是外部锚点：字节型部分完成即完成，nanosleep 无完成形态",
    );

    // ⑬ 表外 errno 归一为 EINVAL。
    let odd = Evidence { err: 5, ..Evidence::BLANK };
    s.add(
        "errno 域：表外错误码归一为 EINVAL",
        normalize_entry_errno(5) == EINVAL
            && normalize_entry_errno(EBADF) == EBADF
            && normalize_entry_errno(EFAULT) == EFAULT
            && ordered_eval(&odd) == Verdict::Err(EINVAL),
        "入口级错误只认四个码，其余一律归一，避免伪造 errno",
    );

    // ⑭ 双跑一致。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：12 格判定指纹相等",
        fp_a == fp_b,
        "同证据两次判定必须逐字节相同，闸序不得依赖隐藏状态",
    );

    // ⑮ 码值唯一。
    let mut uniq = true;
    let mut a = 0usize;
    while a < OrderCode::ALL.len() {
        let mut b = a + 1;
        while b < OrderCode::ALL.len() {
            if let (Some(x), Some(y)) = (OrderCode::ALL.get(a), OrderCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    s.add(
        "诊断码：12 个码值两两唯一",
        uniq,
        "段内闭集码重复会让判据无法定位",
    );

    // ⑯ 文案非空。
    s.add(
        "文案闭集：12 个诊断码均有中文处置建议",
        OrderCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给出可操作处置",
    );

    // 动态明细：矩阵快照。
    let text_m = format!(
        "矩阵 {}/{}（{} 格 × {} 轮），次序违例 {}，矛盾格 {}",
        audit.matched,
        (CELL_COUNT as u32) * AUDIT_ROUNDS,
        audit.cells,
        AUDIT_ROUNDS,
        audit.order_violations,
        audit.contradictions
    );
    s.add_dyn(
        "矩阵快照",
        matrix_ok && audit.order_violations == 0,
        &text_m,
    );

    // 动态明细：缺陷策略账。
    let text_p = format!(
        "缺陷 {} 其中检出 {} 逃逸 {}，见证格合计 {}",
        pl.injected, pl.detected, pl.escaped, pl.witness_cells
    );
    s.add_dyn(
        "缺陷策略账",
        pl.conserved() && pl.escaped == 0,
        &text_p,
    );

    // 动态明细：分歧格数。
    let text_d = format!("越序实现分歧格 {} / {}，指纹 0x{:08X}", div, CELL_COUNT, fp_a);
    s.add_dyn("越序分歧快照", div == 4 && fp_a == fp_b, &text_d);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_ranks_are_strict() {
        assert_eq!(Stage::Param.rank(), 0);
        assert_eq!(Stage::Complete.rank(), 1);
        assert_eq!(Stage::Eof.rank(), 2);
        assert_eq!(Stage::Interrupt.rank(), 3);
        assert_eq!(Stage::Block.rank(), 4);
        assert_eq!(Stage::Return.rank(), 5);
        assert_eq!(Stage::Interrupt.bit(), 8);
    }

    #[test]
    fn completion_beats_signal() {
        let ev = Evidence {
            xfer: 64,
            want: 64,
            sig_at: SigAt::InBlock,
            ..Evidence::BLANK
        };
        let (v, mask) = ordered_traced(&ev);
        assert_eq!(v, Verdict::Done(64));
        assert_eq!(mask & Stage::Interrupt.bit(), 0);
        assert_eq!(mask & Stage::Complete.bit(), Stage::Complete.bit());
    }

    #[test]
    fn partial_completion_is_done() {
        let ev = Evidence {
            xfer: 32,
            want: 64,
            sig_at: SigAt::AfterWake,
            ..Evidence::BLANK
        };
        assert_eq!(ordered_eval(&ev), Verdict::Done(32));
        assert_eq!(Policy::PartialAsFailure.verdict(&ev), Verdict::Again);
    }

    #[test]
    fn zero_progress_signal_is_eintr() {
        let ev = Evidence {
            xfer: 0,
            want: 64,
            sig_at: SigAt::InBlock,
            ..Evidence::BLANK
        };
        assert_eq!(ordered_eval(&ev), Verdict::Eintr);
        let eof_ev = Evidence {
            xfer: 0,
            want: 64,
            sig_at: SigAt::InBlock,
            eof: true,
            ..Evidence::BLANK
        };
        assert_eq!(ordered_eval(&eof_ev), Verdict::Done(0));
    }

    #[test]
    fn error_gate_is_first() {
        let ev = Evidence {
            err: EFAULT,
            want: 64,
            sig_at: SigAt::InBlock,
            ..Evidence::BLANK
        };
        let (v, mask) = ordered_traced(&ev);
        assert_eq!(v, Verdict::Err(EFAULT));
        assert_eq!(mask & Stage::Interrupt.bit(), 0);
        assert_eq!(normalize_entry_errno(5), EINVAL);
    }

    #[test]
    fn contradiction_is_rejected() {
        let ev = Evidence {
            xfer: 8,
            want: 16,
            err: EFAULT,
            ..Evidence::BLANK
        };
        assert_eq!(ev.contradictory(), true);
        assert_eq!(ordered_eval(&ev), Verdict::Err(EINVAL));
    }

    #[test]
    fn naive_order_diverges_in_four_cells() {
        assert_eq!(eintr_first_divergences(), 4);
        let ev = Evidence {
            xfer: 10,
            want: 10,
            sig_at: SigAt::InBlock,
            ..Evidence::BLANK
        };
        assert_eq!(eintr_first_eval(&ev), Verdict::Eintr);
        assert_eq!(ordered_eval(&ev), Verdict::Done(10));
    }

    #[test]
    fn every_policy_has_a_witness() {
        let led = run_policy_injection();
        assert_eq!(led.injected, POLICY_COUNT as u32);
        assert_eq!(led.detected, POLICY_COUNT as u32);
        assert_eq!(led.escaped, 0);
        assert_eq!(led.conserved(), true);
        assert_eq!(led.witness_cells >= POLICY_COUNT as u32, true);
    }

    #[test]
    fn audit_matches_all_cells() {
        let a = run_audit();
        assert_eq!(a.cells, CELL_COUNT as u32);
        assert_eq!(a.matched, (CELL_COUNT as u32) * AUDIT_ROUNDS);
        assert_eq!(a.order_violations, 0);
    }

    #[test]
    fn syscall_rules_agree() {
        let mut i = 0usize;
        while i < RULES.len() {
            if let Some(r) = RULES.get(i) {
                let v = ordered_eval(&rule_probe(r.kind));
                let (c, b) = rule_want(r.kind);
                assert_eq!(v.class(), c);
                assert_eq!(v.bytes(), b);
            }
            i += 1;
        }
        assert_eq!(RULES.len(), RULE_COUNT);
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9313_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
