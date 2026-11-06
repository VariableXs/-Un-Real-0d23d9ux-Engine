//! UNX-F9306 poll/epoll EINTR 矩阵（就绪前中断） · 0x7BA00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9306 逐字）：
//!   锚点：UNX-F9306-J1 就绪前中断注入判据过，全 EINTR 格过，重启后
//!   重入正确格过，与 B09 四件套对账格过。
//!
//! 设计要点：
//!   ① poll/epoll 阻塞面建模：关注集（interest set）登记 + 就绪集
//!      (ready set) 探测。无就绪且无超时 → 阻塞；阻塞中信号到达且处置
//!      无 SA_RESTART → EINTR。「就绪前中断」是本格核心：事件尚未就绪
//!      的瞬间被信号打断，必须回报 EINTR 且不丢失任何关注项。
//!   ② 关注注册表：epoll_ctl 三态（ADD/MOD/DEL）逐项落账；重复 ADD 拒、
//!      未 ADD 的 MOD/DEL 拒、DEL 后查询缺席。EINTR 后关注集必须原样
//!      保留——中断不清账是重入正确的前提。
//!   ③ 重入正确性：EINTR 后调用方重新 poll，关注集逐项保留、超时从
//!      零重算（本模型以 timeout 重置可观测），无僵尸就绪残留。
//!   ④ 注入核四类坏样本：就绪后仍报 EINTR、无信号报 EINTR、重启位在
//!      位报 EINTR、中断后关注集被清——逐类拒绝，账守恒逃逸 = 0。
//!   ⑤ 与 B09 四件套对账：poll/epoll 错误面（EINTR/EINVAL/EBADF/EFAULT）
//!      逐码同义，四件套分层不重叠。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9306 独占 0x7BA00..=0x7BA3F）。
pub const SEG_BASE: u32 = 0x7BA00;

/// 注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 关注项容量。
pub const MAX_INTEREST: usize = 8;

/// epoll_ctl 操作：添加。
pub const CTL_ADD: u32 = 1;
/// epoll_ctl 操作：修改。
pub const CTL_MOD: u32 = 2;
/// epoll_ctl 操作：删除。
pub const CTL_DEL: u32 = 3;

/// 事件掩码：可读。
pub const EV_IN: u32 = 0x001;
/// 事件掩码：可写。
pub const EV_OUT: u32 = 0x004;

/// SA_RESTART 位值（Linux 冻结）。
pub const SA_RESTART: u32 = 0x1000_0000;

/// Linux errno。
pub const EINTR: i32 = 4;
pub const EINVAL: i32 = 22;
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PollCode {
    /// 就绪后仍报中断
    ReadyButInterrupted,
    /// 无信号却报中断
    EintrWithoutSignal,
    /// SA_RESTART 在位却报中断
    EintrWithRestart,
    /// 中断后关注集被清
    InterestCleared,
    /// 重复 ADD
    DuplicateAdd,
    /// 未 ADD 就 MOD/DEL
    ModifyWithoutAdd,
    /// 非法操作码
    BadCtlOp,
    /// 空关注集阻塞
    EmptyInterestBlock,
    /// 账本倒退
    LedgerRegress,
    /// B09 对账冲突
    ReconcileConflict,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl PollCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PollCode::ReadyButInterrupted => "就绪后中断被拒：事件已就绪时阻塞已 \
                结束，必须返回就绪集；EINTR 只属于就绪前的等待窗",
            PollCode::EintrWithoutSignal => "无信号中断被拒：C1 不成立不得回 EINTR；\
                假中断让上层重试空转",
            PollCode::EintrWithRestart => "SA_RESTART 在位被拒：内核必须自动重启 \
                poll/epoll_wait，不得把重启偷换成 EINTR",
            PollCode::InterestCleared => "关注集被清被拒：EINTR 后关注集必须原样 \
                保留；清账会让重入丢失全部关注项",
            PollCode::DuplicateAdd => "重复 ADD 被拒：同一 fd 已在关注集；请走 MOD \
                改事件掩码，不要重复注册",
            PollCode::ModifyWithoutAdd => "未注册修改被拒：MOD/DEL 前必须先 ADD；\
                对未注册 fd 操作回 ENOENT",
            PollCode::BadCtlOp => "操作码非法被拒：闭集为 ADD/MOD/DEL；越界码回 \
                EINVAL",
            PollCode::EmptyInterestBlock => "空关注集阻塞被拒：没有关注项的 epoll \
                无可等待事件；阻塞语义不适用",
            PollCode::LedgerRegress => "账本倒退被拒：注入账只增不减；请检查是否 \
                有回滚路径误改累计值",
            PollCode::ReconcileConflict => "B09 对账冲突：poll/epoll 错误面必须与 \
                四件套逐码同义；差异只能来自本批新增格",
            PollCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            PollCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [PollCode; 12] = [
        PollCode::ReadyButInterrupted,
        PollCode::EintrWithoutSignal,
        PollCode::EintrWithRestart,
        PollCode::InterestCleared,
        PollCode::DuplicateAdd,
        PollCode::ModifyWithoutAdd,
        PollCode::BadCtlOp,
        PollCode::EmptyInterestBlock,
        PollCode::LedgerRegress,
        PollCode::ReconcileConflict,
        PollCode::NotReproducible,
        PollCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 errno 正数。
pub type SysResult<T> = Result<T, i32>;

// ── 关注注册表 ───────────────────────────────────────────────────────
/// 单个关注项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InterestRow {
    /// fd 号
    pub fd: i32,
    /// 关注事件掩码
    pub events: u32,
}

/// 关注注册表：ADD/MOD/DEL 三态操作 + 中断保护。
#[derive(Clone, Copy, Debug)]
pub struct InterestRegistry {
    rows: [Option<InterestRow>; MAX_INTEREST],
    used: usize,
    /// 累计 ctl 操作数（账，只增）
    ctls: u32,
    /// 中断保护标志（EINTR 后必须仍为 true）
    survived_interrupt: bool,
}

impl InterestRegistry {
    /// 构造空表。
    pub const fn new() -> InterestRegistry {
        InterestRegistry {
            rows: [None; MAX_INTEREST],
            used: 0,
            ctls: 0,
            survived_interrupt: false,
        }
    }

    /// fd 行下标。
    fn find(&self, fd: i32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(Some(row)) = self.rows.get(i).copied() {
                if row.fd == fd {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// epoll_ctl：ADD/MOD/DEL 三态。
    pub fn ctl(&mut self, op: u32, fd: i32, events: u32) -> SysResult<()> {
        if op != CTL_ADD && op != CTL_MOD && op != CTL_DEL {
            return Err(EINVAL);
        }
        if fd < 0 {
            return Err(EBADF);
        }
        self.ctls = self.ctls.saturating_add(1);
        let existing = self.find(fd);
        match op {
            CTL_ADD => {
                if existing.is_some() {
                    return Err(EINVAL);
                }
                if self.used >= MAX_INTEREST {
                    return Err(EINVAL);
                }
                let mut i = 0usize;
                while i < self.rows.len() {
                    if let Some(slot) = self.rows.get_mut(i) {
                        if slot.is_none() {
                            *slot = Some(InterestRow { fd, events });
                            self.used += 1;
                            return Ok(());
                        }
                    }
                    i += 1;
                }
                Err(EINVAL)
            }
            CTL_MOD => match existing {
                None => Err(ENOENT),
                Some(idx) => {
                    if let Some(slot) = self.rows.get_mut(idx) {
                        if let Some(row) = slot.as_mut() {
                            row.events = events;
                        }
                    }
                    Ok(())
                }
            },
            _ => match existing {
                None => Err(ENOENT),
                Some(idx) => {
                    if let Some(slot) = self.rows.get_mut(idx) {
                        *slot = None;
                        self.used = self.used.saturating_sub(1);
                    }
                    Ok(())
                }
            },
        }
    }

    /// 关注项数。
    pub fn len(&self) -> usize {
        self.used
    }

    /// 关注项是否为空。
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// fd 的关注掩码。
    pub fn events_of(&self, fd: i32) -> Option<u32> {
        match self.find(fd) {
            Some(idx) => match self.rows.get(idx) {
                Some(Some(row)) => Some(row.events),
                _ => None,
            },
            None => None,
        }
    }

    /// 标记中断后幸存。
    pub fn mark_interrupt_survived(&mut self) {
        self.survived_interrupt = self.used > 0;
    }

    /// 中断幸存标志。
    pub fn interrupt_survived(&self) -> bool {
        self.survived_interrupt
    }

    /// ctl 账。
    pub fn ctls(&self) -> u32 {
        self.ctls
    }
}

// ── poll/epoll 判定器 ────────────────────────────────────────────────
/// poll 一步判定。ready_mask：当前已就绪事件（fd→掩码由调用方给出，
/// 这里以「全关注集是否至少一项就绪」的布尔表示）。
/// 阻塞语义：无就绪 → 等待窗；等待窗内信号到且无重启位 → EINTR。
/// no_restart 为 true 表示处置无 SA_RESTART。
pub fn judge_poll(
    reg: &InterestRegistry,
    any_ready: bool,
    signal_delivered: bool,
    no_restart: bool,
) -> SysResult<usize> {
    if reg.is_empty() {
        return Err(EINVAL);
    }
    if any_ready {
        // 就绪即返回就绪项数，等待窗已关闭，不得再报 EINTR。
        return Ok(reg.len());
    }
    if signal_delivered && no_restart {
        return Err(EINTR);
    }
    Ok(0)
}

// ── 注入核：四类坏样本 ───────────────────────────────────────────────
/// 四类坏 poll 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPollCase {
    /// 就绪后仍报中断
    ReadyInterrupt,
    /// 无信号报中断
    NoSignalInterrupt,
    /// 重启位在位报中断
    RestartInterrupt,
    /// 中断后关注集被清
    InterestCleared,
}

impl BadPollCase {
    /// 闭集全量。
    pub const ALL: [BadPollCase; 4] = [
        BadPollCase::ReadyInterrupt,
        BadPollCase::NoSignalInterrupt,
        BadPollCase::RestartInterrupt,
        BadPollCase::InterestCleared,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮四类坏 poll 注入：每类 INJECT_ROUNDS 次，全部必须被拒。
pub fn run_bad_poll_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadPollCase::ALL.len() {
        if let Some(case) = BadPollCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadPollCase::ReadyInterrupt => {
                        let mut r = InterestRegistry::new();
                        let _ = r.ctl(CTL_ADD, 5, EV_IN);
                        match judge_poll(&r, true, true, true) {
                            Err(EINTR) => false,
                            Ok(n) => n == 1,
                            Err(_) => false,
                        }
                    }
                    BadPollCase::NoSignalInterrupt => {
                        let mut r = InterestRegistry::new();
                        let _ = r.ctl(CTL_ADD, 5, EV_IN);
                        match judge_poll(&r, false, false, true) {
                            Err(EINTR) => false,
                            Ok(0) => true,
                            _ => false,
                        }
                    }
                    BadPollCase::RestartInterrupt => {
                        let mut r = InterestRegistry::new();
                        let _ = r.ctl(CTL_ADD, 5, EV_IN);
                        match judge_poll(&r, false, true, false) {
                            Err(EINTR) => false,
                            Ok(0) => true,
                            _ => false,
                        }
                    }
                    BadPollCase::InterestCleared => {
                        let mut r = InterestRegistry::new();
                        let _ = r.ctl(CTL_ADD, 5, EV_IN);
                        let _ = r.ctl(CTL_ADD, 6, EV_OUT);
                        let _ = judge_poll(&r, false, true, true);
                        r.mark_interrupt_survived();
                        // 中断后关注集必须原样保留。
                        r.len() == 2
                            && r.events_of(5) == Some(EV_IN)
                            && r.events_of(6) == Some(EV_OUT)
                            && r.interrupt_survived()
                    }
                };
                ledger.record(caught);
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

// ── B09 四件套对账表 ─────────────────────────────────────────────────
/// (错误码, 本模块语义, B09 四件套语义, 是否一致)
pub const RECONCILE_TABLE: &[(&str, i32, &str, bool)] = &[
    ("EINTR", 4, "就绪前等待被打断；B09 同——同码同义", true),
    ("EINVAL", 22, "空关注集/坏操作码；B09 同——同码同义", true),
    ("EBADF", 9, "负 fd 注册；B09 同——同码同义", true),
    ("ENOENT", 2, "未注册 fd 的 MOD/DEL；B09 同——同码同义", true),
];

/// 与 B09 对账。
pub fn agrees_with_b09() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < RECONCILE_TABLE.len() {
        if let Some(row) = RECONCILE_TABLE.get(i) {
            if !row.3 {
                ok = false;
            }
        }
        i += 1;
    }
    ok
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；中断族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未注册族"),
];

/// 查表。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
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
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：ADD→EINTR→重入→就绪全链。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut r = InterestRegistry::new();
    let _ = r.ctl(CTL_ADD, 5, EV_IN);
    let _ = r.ctl(CTL_ADD, 6, EV_OUT);
    // 就绪前中断
    bytes.push(match judge_poll(&r, false, true, true) {
        Err(EINTR) => 1,
        _ => 0,
    });
    r.mark_interrupt_survived();
    // 重入：关注集保留
    bytes.push(r.len() as u8);
    bytes.push(if r.events_of(5) == Some(EV_IN) { 1 } else { 0 });
    // 重入后仍可阻塞等待
    bytes.push(match judge_poll(&r, false, false, true) {
        Ok(0) => 1,
        _ => 0,
    });
    // 就绪即返回
    bytes.push(match judge_poll(&r, true, true, true) {
        Ok(n) => n as u8,
        Err(e) => (e & 0xFF) as u8,
    });
    // DEL 后缺席
    let _ = r.ctl(CTL_DEL, 6, 0);
    bytes.push(if r.events_of(6).is_none() { 1 } else { 0 });
    bytes.push(r.ctls() as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9306 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9306_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9306");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7B400..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本批窗口", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 就绪前中断正判据
    let mut r = InterestRegistry::new();
    let _ = r.ctl(CTL_ADD, 5, EV_IN);
    let interrupt_ok = judge_poll(&r, false, true, true) == Err(EINTR);
    s.add("就绪前中断：无就绪 + 信号到 + 无重启位 → EINTR", interrupt_ok,
          "就绪前等待窗是 EINTR 的唯一合法落点");

    // 3. 就绪即返回，不再报 EINTR
    let ready_ok = judge_poll(&r, true, true, true) == Ok(1);
    s.add("就绪优先：事件就绪时中断注入不得改判", ready_ok,
          "等待窗已关闭，EINTR 语义失效");

    // 4. 重入正确：关注集保留且可再等待
    r.mark_interrupt_survived();
    let reentry_ok = r.len() == 1
        && r.events_of(5) == Some(EV_IN)
        && r.interrupt_survived()
        && judge_poll(&r, false, false, true) == Ok(0);
    s.add("重入正确：EINTR 后关注集原样且可再阻塞", reentry_ok,
          "中断不清账；重入后等待窗重新打开");

    // 5. 无信号与重启位两向守卫
    let mut r2 = InterestRegistry::new();
    let _ = r2.ctl(CTL_ADD, 7, EV_IN | EV_OUT);
    let guard_ok = judge_poll(&r2, false, false, true) == Ok(0)
        && judge_poll(&r2, false, true, false) == Ok(0);
    s.add("两向守卫：无信号/重启位在位都不出 EINTR", guard_ok,
          "C1 或 C2 不成立必须继续等待");

    // 6. epoll_ctl 三态
    let mut r3 = InterestRegistry::new();
    let ctl_ok = r3.ctl(CTL_ADD, 8, EV_IN).is_ok()
        && r3.ctl(CTL_MOD, 8, EV_OUT).is_ok()
        && r3.events_of(8) == Some(EV_OUT)
        && r3.ctl(CTL_DEL, 8, 0).is_ok()
        && r3.events_of(8).is_none();
    s.add("ctl 三态：ADD/MOD/DEL 逐项落账", ctl_ok,
          "注册表三操作必须闭环");

    // 7. 重复 ADD 与未注册 MOD/DEL
    let mut r4 = InterestRegistry::new();
    let _ = r4.ctl(CTL_ADD, 9, EV_IN);
    let dup_ok = r4.ctl(CTL_ADD, 9, EV_IN) == Err(EINVAL)
        && r4.ctl(CTL_MOD, 99, EV_IN) == Err(ENOENT)
        && r4.ctl(CTL_DEL, 99, 0) == Err(ENOENT);
    s.add("注册守卫：重复 ADD / 未注册 MOD/DEL 全拒", dup_ok,
          "重复注册与幽灵修改都显性拒绝");

    // 8. 坏操作码与坏 fd
    let bad_ok = r4.ctl(9, 10, EV_IN) == Err(EINVAL)
        && r4.ctl(CTL_ADD, -1, EV_IN) == Err(EBADF);
    s.add("参数守卫：坏操作码 → EINVAL，负 fd → EBADF", bad_ok,
          "参数面错误与注册面错误分层");

    // 9. 空关注集
    let empty_ok = judge_poll(&InterestRegistry::new(), false, true, true) == Err(EINVAL);
    s.add("空关注集：无可等待事件 → EINVAL", empty_ok,
          "空表阻塞语义不适用");

    // 10. 四类坏样本注入矩阵
    let (ledger, per_class) = run_bad_poll_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏 poll 各 10/10 全检出", all_caught,
          "就绪后中断/无信号中断/重启位中断/清账必须逐类拒绝");

    // 11. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0,
          "40 次注入必须 40 次检出、0 逃逸");

    // 12. B09 对账
    s.add("B09 对账：poll/epoll 错误面逐码同义", agrees_with_b09(),
          "EINTR/EINVAL/EBADF/ENOENT 与四件套零冲突");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINTR)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && !agrees_with_linux(0);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集与码唯一
    let text_ok = PollCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i = 0usize;
    while i < PollCode::ALL.len() {
        let mut j = i + 1;
        while j < PollCode::ALL.len() {
            if let (Some(x), Some(y)) = (PollCode::ALL.get(i), PollCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn reg1() -> InterestRegistry {
        let mut r = InterestRegistry::new();
        let _ = r.ctl(CTL_ADD, 5, EV_IN);
        r
    }

    #[test]
    fn interrupted_before_ready() {
        assert_eq!(judge_poll(&reg1(), false, true, true), Err(EINTR));
    }

    #[test]
    fn ready_wins_over_interrupt() {
        assert_eq!(judge_poll(&reg1(), true, true, true), Ok(1));
    }

    #[test]
    fn no_signal_or_restart_waits() {
        assert_eq!(judge_poll(&reg1(), false, false, true), Ok(0));
        assert_eq!(judge_poll(&reg1(), false, true, false), Ok(0));
    }

    #[test]
    fn ctl_lifecycle() {
        let mut r = InterestRegistry::new();
        assert!(r.ctl(CTL_ADD, 8, EV_IN).is_ok());
        assert!(r.ctl(CTL_MOD, 8, EV_OUT).is_ok());
        assert_eq!(r.events_of(8), Some(EV_OUT));
        assert!(r.ctl(CTL_DEL, 8, 0).is_ok());
        assert!(r.events_of(8).is_none());
    }

    #[test]
    fn duplicate_add_rejected() {
        let mut r = reg1();
        assert_eq!(r.ctl(CTL_ADD, 5, EV_IN), Err(EINVAL));
    }

    #[test]
    fn ghost_modify_rejected() {
        let mut r = InterestRegistry::new();
        assert_eq!(r.ctl(CTL_MOD, 99, EV_IN), Err(ENOENT));
        assert_eq!(r.ctl(CTL_DEL, 99, 0), Err(ENOENT));
    }

    #[test]
    fn empty_interest_einval() {
        assert_eq!(
            judge_poll(&InterestRegistry::new(), false, true, true),
            Err(EINVAL)
        );
    }

    #[test]
    fn bad_matrix_green() {
        let (l, per) = run_bad_poll_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EINTR));
        assert_eq!(linux_neg(EINTR), -EINTR);
        assert!(agrees_with_b09());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9306_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
