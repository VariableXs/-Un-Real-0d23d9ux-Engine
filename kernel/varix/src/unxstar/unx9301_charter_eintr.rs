//! UNX-F9301 EINTR 语义总纲（慢系统调用中断面） · 0x7B8C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9301 逐字）：
//!   锚点：UNX-F9301-J1 慢系统调用中断面判据过，EINTR 触发三条件
//!   （信号到达/无 SA_RESTART/阻塞中）逐格落账，与 Linux 语义一致格过。
//!
//! 设计要点：
//!   ① EINTR 触发是三条件合取，缺一不可：C1 信号确实到达、C2 被打断的
//!      系统调用其信号处置未带 SA_RESTART、C3 系统调用当时确实处于阻塞
//!      中。三条件逐格落账：八种布尔组合全部可观测，只允许 111 组合出
//!      EINTR，其余七种组合各有明确归宿（完成/继续阻塞/重启）。
//!   ② 慢系统调用登记表：本总纲按 Linux 口径冻结「慢族」闭集（read/
//!      write 管道满空、wait、poll、flock、nanosleep、accept、connect）。
//!      非慢族（不阻塞的 get_pid 类）即使三条件齐备也不产生 EINTR，
//!      因为 C3 不可能为真——登记表把这一点显性化。
//!   ③ 信号处置注册表：每行 (信号号, 处置类型, flags)。SA_RESTART 位
//!      解析是总纲的钥匙位：处置缺省（SIG_DFL 致命/忽略）的信号不进入
//!      EINTR 判定——进程根本看不到它。
//!   ④ 中断面状态机：Runnable → Blocked(slow) → Interrupted | Completed。
//!      Interrupted 只能来自 111 组合；Completed 后再投递信号走正常
//!      处置，不得追溯改成 EINTR。
//!   ⑤ 逐格落账账本：八格 × INJECT_ROUNDS 次注入，账守恒；期望非 EINTR
//!      的格子若出了 EINTR 记逃逸，期望 EINTR 的格子没出也记逃逸。
//!   ⑥ 与 Linux 语义一致格：EINTR=4 对照冻结；SA_RESTART=0x10000000
//!      位值冻结；重启语义（111 以外的 1x1 组合自动重启）逐格核对。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9301 独占 0x7B8C0..=0x7B8FF）。
pub const SEG_BASE: u32 = 0x7B8C0;

/// 注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 布尔三条件组合数（2^3）。
pub const COMBOS: usize = 8;

/// SA_RESTART 位值（Linux 冻结）。
pub const SA_RESTART: u32 = 0x1000_0000;

/// Linux errno：中断。
pub const EINTR: i32 = 4;
/// Linux errno：无此进程（信号目标）。
pub const ESRCH: i32 = 3;
/// Linux errno：坏参数。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EintrCode {
    /// 无信号却报中断
    EintrWithoutSignal,
    /// SA_RESTART 在位却报中断
    EintrWithRestart,
    /// 未阻塞却报中断
    EintrNotBlocked,
    /// 三条件齐备却未中断（应重启或完成之外的场景）
    MissedInterruption,
    /// 信号号非法
    BadSignal,
    /// 处置类型越界
    BadDisposition,
    /// 慢族表外调用进入中断判定
    NotSlowSyscall,
    /// 落账账本倒退
    LedgerRegress,
    /// 完成后追溯改中断
    RetroactiveInterrupt,
    /// 双跑不一致
    NotReproducible,
    /// 文案缺失
    EmptyDetail,
    /// 段位越界
    SegmentViolation,
}

impl EintrCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            EintrCode::EintrWithoutSignal => "无信号中断被拒：C1 不成立时不许回 \
                EINTR；假中断会让上层重试逻辑空转",
            EintrCode::EintrWithRestart => "SA_RESTART 在位被拒：C2 不成立时内核 \
                必须自动重启系统调用，不得把重启语义偷换成 EINTR",
            EintrCode::EintrNotBlocked => "未阻塞中断被拒：C3 不成立（调用未阻塞 \
                或非慢族）时信号只能走正常处置，不得产生 EINTR",
            EintrCode::MissedInterruption => "中断缺失：三条件齐备（111）必须回 \
                EINTR；吞掉中断会让信号驱动的取消逻辑失效",
            EintrCode::BadSignal => "信号号非法被拒：闭集为 1..=31；0 号与负号 \
                显性拒绝",
            EintrCode::BadDisposition => "处置类型越界被拒：闭集为 \
                Default/Term/Ign/Handler，越界档拒绝入表",
            EintrCode::NotSlowSyscall => "非慢族被拒：中断面只服务慢系统调用；\
                请先在慢族登记表核对再谈 EINTR",
            EintrCode::LedgerRegress => "落账倒退被拒：逐格账只增不减；请检查是否 \
                有回滚路径误改累计值",
            EintrCode::RetroactiveInterrupt => "追溯改中断被拒：Completed 之后到达 \
                的信号走正常处置，不得把已完成调用改判 EINTR",
            EintrCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            EintrCode::EmptyDetail => "文案缺失被拒：每个诊断码必须带非空中文建议",
            EintrCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [EintrCode; 12] = [
        EintrCode::EintrWithoutSignal,
        EintrCode::EintrWithRestart,
        EintrCode::EintrNotBlocked,
        EintrCode::MissedInterruption,
        EintrCode::BadSignal,
        EintrCode::BadDisposition,
        EintrCode::NotSlowSyscall,
        EintrCode::LedgerRegress,
        EintrCode::RetroactiveInterrupt,
        EintrCode::NotReproducible,
        EintrCode::EmptyDetail,
        EintrCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── 信号处置注册表 ───────────────────────────────────────────────────
/// 处置类型（四档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// SIG_DFL：默认（Term/Ign 由信号语义决定）
    Default,
    /// 默认终止
    Term,
    /// 默认忽略
    Ign,
    /// 用户 handler
    Handler,
}

/// 单个信号处置行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SigAction {
    /// 信号号（1..=31）
    pub signum: u8,
    /// 处置类型
    pub disp: Disposition,
    /// flags 位集（可含 SA_RESTART）
    pub flags: u32,
}

/// 信号处置注册表（定容 32，一号一格）。
#[derive(Clone, Copy, Debug)]
pub struct SigRegistry {
    slots: [Option<SigAction>; 32],
    used: usize,
}

impl SigRegistry {
    /// 构造：预置 SIGINT/SIGTERM/SIGUSR1/SIGUSR2 四行。
    pub fn new() -> SigRegistry {
        let mut r = SigRegistry {
            slots: [None; 32],
            used: 0,
        };
        let _ = r.install(SigAction {
            signum: 2,
            disp: Disposition::Handler,
            flags: 0,
        });
        let _ = r.install(SigAction {
            signum: 15,
            disp: Disposition::Handler,
            flags: SA_RESTART,
        });
        let _ = r.install(SigAction {
            signum: 10,
            disp: Disposition::Handler,
            flags: 0,
        });
        let _ = r.install(SigAction {
            signum: 12,
            disp: Disposition::Handler,
            flags: SA_RESTART,
        });
        r
    }

    /// 安装处置：非法号/非法档/重复号拒绝。
    pub fn install(&mut self, act: SigAction) -> SysResult<()> {
        if act.signum == 0 || act.signum > 31 {
            return Err(EINVAL);
        }
        match act.disp {
            Disposition::Default | Disposition::Term | Disposition::Ign
            | Disposition::Handler => {}
        }
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(prev) = self.slots.get(i).copied().and_then(|o| o) {
                if prev.signum == act.signum {
                    // 覆盖安装合法（sigaction 语义），但 flags 解析要一致。
                    if let Some(slot) = self.slots.get_mut(i) {
                        *slot = Some(act);
                    }
                    return Ok(());
                }
            }
            i += 1;
        }
        if self.used >= self.slots.len() {
            return Err(EINVAL);
        }
        if let Some(slot) = self.slots.get_mut(self.used) {
            *slot = Some(act);
            self.used += 1;
        }
        Ok(())
    }

    /// 查询处置：未安装行回 None。
    pub fn action(&self, signum: u8) -> Option<SigAction> {
        if signum == 0 || signum > 31 {
            return None;
        }
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(Some(act)) = self.slots.get(i).copied() {
                if act.signum == signum {
                    return Some(act);
                }
            }
            i += 1;
        }
        None
    }

    /// C2 判定：处置存在且无 SA_RESTART 位。
    pub fn no_restart(&self, signum: u8) -> bool {
        match self.action(signum) {
            Some(act) => act.flags & SA_RESTART == 0,
            None => false,
        }
    }
}

// ── 慢系统调用登记表 ─────────────────────────────────────────────────
/// 慢族闭集（冻结行序）。
pub const SLOW_SYSCALLS: &[&str] = &[
    "read-pipe-empty",
    "write-pipe-full",
    "wait",
    "poll",
    "flock",
    "nanosleep",
    "accept",
    "connect",
];

/// C3 前提：调用必须属于慢族。
pub fn is_slow(name: &str) -> bool {
    let mut i = 0usize;
    while i < SLOW_SYSCALLS.len() {
        if let Some(row) = SLOW_SYSCALLS.get(i) {
            if *row == name {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 中断面状态机 ─────────────────────────────────────────────────────
/// 调用状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallState {
    /// 尚未阻塞
    Runnable,
    /// 阻塞中（仅慢族可达）
    Blocked,
    /// 被中断（EINTR）
    Interrupted,
    /// 完成
    Completed,
}

/// 中断面状态机：Runnable→Blocked→(Interrupted|Completed)。
#[derive(Clone, Copy, Debug)]
pub struct InterruptMachine {
    state: CallState,
    slow: bool,
}

impl InterruptMachine {
    /// 构造：按慢族登记初始化。
    pub fn new(syscall: &str) -> InterruptMachine {
        InterruptMachine {
            state: CallState::Runnable,
            slow: is_slow(syscall),
        }
    }

    /// 进入阻塞：非慢族拒绝。
    pub fn block(&mut self) -> SysResult<()> {
        if !self.slow {
            return Err(EINVAL);
        }
        if self.state != CallState::Runnable {
            return Err(EINVAL);
        }
        self.state = CallState::Blocked;
        Ok(())
    }

    /// 中断判定核心：三条件合取。返回新状态或 errno。
    /// C1=signal_delivered，C2=无 SA_RESTART，C3=当前 Blocked。
    pub fn deliver(
        &mut self,
        reg: &SigRegistry,
        signum: u8,
        signal_delivered: bool,
    ) -> SysResult<CallState> {
        if reg.action(signum).is_none() && signal_delivered {
            // 未安装处置的信号按 Default 看待：不进 EINTR 判定。
            return Ok(CallState::Blocked);
        }
        if self.state == CallState::Completed {
            return Err(EINVAL);
        }
        if signal_delivered && self.state == CallState::Blocked && reg.no_restart(signum) {
            self.state = CallState::Interrupted;
            return Err(EINTR);
        }
        // 其余组合：信号到但可重启 → 自动重启（仍在 Blocked）；未到 → 继续。
        Ok(self.state)
    }

    /// 完成：Blocked → Completed；之后不得追溯改判。
    pub fn complete(&mut self) -> SysResult<()> {
        if self.state != CallState::Blocked {
            return Err(EINVAL);
        }
        self.state = CallState::Completed;
        Ok(())
    }

    /// 当前状态。
    pub fn state(&self) -> CallState {
        self.state
    }
}

// ── 八格逐格落账 ─────────────────────────────────────────────────────
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

/// 八格真值表期望：仅 (C1,C2,C3)=(true,true,true) 出 EINTR。
fn expect_eintr(c1: bool, c2: bool, c3: bool) -> bool {
    c1 && c2 && c3
}

/// 八格逐格落账：每格 INJECT_ROUNDS 次，账守恒。
pub fn run_truth_table_matrix() -> (InjectLedger, [u32; COMBOS]) {
    let reg = SigRegistry::new();
    // C2=无 SA_RESTART 的信号取 SIGINT(2)；C2=有 SA_RESTART 取 SIGTERM(15)。
    let mut ledger = InjectLedger::zero();
    let mut per_combo = [0u32; COMBOS];
    let mut combo = 0usize;
    while combo < COMBOS {
        let c1 = combo & 1 != 0;
        let c2 = combo & 2 != 0;
        let c3 = combo & 4 != 0;
        let signum: u8 = if c2 { 2 } else { 15 };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut m = InterruptMachine::new("wait");
            if c3 {
                let _ = m.block();
            } else {
                // 非阻塞场景让机器停在 Runnable。
            }
            let observed = match m.deliver(&reg, signum, c1) {
                Err(e) => e == EINTR,
                Ok(_) => false,
            };
            let caught = observed == expect_eintr(c1, c2, c3);
            ledger.record(caught);
            if caught {
                if let Some(slot) = per_combo.get_mut(combo) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        combo += 1;
    }
    (ledger, per_combo)
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；中断族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ESRCH", -3, 87, "Windows ERROR_INVALID_PARAMETER；无此进程族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 规范语料双跑指纹：八格 × 慢族 × 追溯守卫。
pub fn corpus_fingerprint() -> u32 {
    let reg = SigRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut combo = 0usize;
    while combo < COMBOS {
        let c1 = combo & 1 != 0;
        let c2 = combo & 2 != 0;
        let c3 = combo & 4 != 0;
        let signum: u8 = if c2 { 2 } else { 15 };
        let mut m = InterruptMachine::new("wait");
        if c3 {
            let _ = m.block();
        }
        let r = m.deliver(&reg, signum, c1);
        bytes.push(match r {
            Ok(_) => 0,
            Err(e) => (e & 0xFF) as u8,
        });
        combo += 1;
    }
    // 慢族守卫
    bytes.push(if is_slow("wait") { 1 } else { 0 });
    bytes.push(if is_slow("getpid-like") { 1 } else { 0 });
    // 追溯守卫：完成后投信号不得改判
    let mut m = InterruptMachine::new("poll");
    let _ = m.block();
    let _ = m.complete();
    let retro = match m.deliver(&reg, 2, true) {
        Err(EINTR) => 1,
        _ => 0,
    };
    bytes.push(retro);
    bytes.push(if reg.no_restart(2) { 1 } else { 0 });
    bytes.push(if reg.no_restart(15) { 0 } else { 1 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9301 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9301_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9301");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7B400..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本批窗口", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 111 组合正判据
    let reg = SigRegistry::new();
    let mut m = InterruptMachine::new("wait");
    let _ = m.block();
    let hit = m.deliver(&reg, 2, true) == Err(EINTR);
    s.add("三条件合取：信号到+无重启位+阻塞中 → EINTR", hit,
          "111 是唯一中断组合，缺一即不产生 EINTR");

    // 3. SA_RESTART 在位自动重启
    let mut m2 = InterruptMachine::new("wait");
    let _ = m2.block();
    let restart_ok = m2.deliver(&reg, 15, true) == Ok(CallState::Blocked);
    s.add("重启语义：SA_RESTART 在位信号到 → 仍阻塞（自动重启）", restart_ok,
          "重启不得偷换成 EINTR，调用继续等");

    // 4. 未阻塞投信号
    let mut m3 = InterruptMachine::new("wait");
    let noblock_ok = m3.deliver(&reg, 2, true) == Ok(CallState::Runnable);
    s.add("未阻塞：Runnable 态投信号不得 EINTR", noblock_ok,
          "C3 不成立，信号走正常处置");

    // 5. 无信号投递
    let mut m4 = InterruptMachine::new("wait");
    let _ = m4.block();
    let nosig_ok = m4.deliver(&reg, 2, false) == Ok(CallState::Blocked);
    s.add("无信号：阻塞中但 C1 不成立 → 继续阻塞", nosig_ok,
          "C1 不成立不得产生假中断");

    // 6. 八格真值表逐格落账
    let (ledger, per_combo) = run_truth_table_matrix();
    let all_ok = per_combo.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("八格真值表：80 次注入逐格全对", all_ok,
          "仅 111 出 EINTR，其余七格各有明确归宿");

    // 7. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0,
          "80 次注入必须全部与期望一致、0 逃逸");

    // 8. 慢族守卫
    let slow_ok = is_slow("wait")
        && is_slow("nanosleep")
        && is_slow("poll")
        && !is_slow("getpid-like")
        && !is_slow("");
    s.add("慢族闭集：表内命中、表外拒绝", slow_ok,
          "非慢族没有阻塞态，中断面对它不适用");

    // 9. 非慢族 block 拒绝
    let mut m5 = InterruptMachine::new("getpid-like");
    let nonslow_ok = m5.block() == Err(EINVAL);
    s.add("非慢族：进入阻塞态被拒", nonslow_ok,
          "登记表外的调用不得伪造阻塞态");

    // 10. 追溯守卫
    let mut m6 = InterruptMachine::new("poll");
    let _ = m6.block();
    let _ = m6.complete();
    let retro_ok = match m6.deliver(&reg, 2, true) {
        Err(EINTR) => false,
        Err(_) => true,
        Ok(_) => true,
    };
    s.add("追溯守卫：Completed 后信号不得改判 EINTR", retro_ok,
          "完成时序先于信号到达，语义不得倒放");

    // 11. 处置注册表：SA_RESTART 位解析
    let flag_ok = reg.action(15).is_some()
        && reg.action(15).map(|a| a.flags & SA_RESTART != 0).unwrap_or(false)
        && reg.action(2).map(|a| a.flags & SA_RESTART == 0).unwrap_or(false)
        && reg.action(99).is_none()
        && reg.action(0).is_none();
    s.add("处置表：flags 位解析精确、表外号缺席", flag_ok,
          "C2 的钥匙位必须逐号可查");

    // 12. 非法信号号
    let mut bad = SigRegistry::new();
    let bad_ok = bad.install(SigAction {
        signum: 0,
        disp: Disposition::Handler,
        flags: 0,
    }) == Err(EINVAL)
        && bad.install(SigAction {
            signum: 32,
            disp: Disposition::Handler,
            flags: 0,
        }) == Err(EINVAL);
    s.add("信号号闭集：0/32 号拒绝安装", bad_ok,
          "号域 1..=31，越界显性拒绝");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINTR)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：EINTR=4 冻结且表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集与码唯一
    let text_ok = EintrCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i = 0usize;
    while i < EintrCode::ALL.len() {
        let mut j = i + 1;
        while j < EintrCode::ALL.len() {
            if let (Some(x), Some(y)) = (EintrCode::ALL.get(i), EintrCode::ALL.get(j)) {
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
    let combo_text = format!("八格检出 {} / 80 / 逃逸 {}", ledger.detected, ledger.escaped);
    s.add_dyn("真值表快照", ledger.conserved(), &combo_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triple_condition_eintr() {
        let reg = SigRegistry::new();
        let mut m = InterruptMachine::new("wait");
        let _ = m.block();
        assert_eq!(m.deliver(&reg, 2, true), Err(EINTR));
    }

    #[test]
    fn restart_flag_rejects_eintr() {
        let reg = SigRegistry::new();
        let mut m = InterruptMachine::new("wait");
        let _ = m.block();
        assert_eq!(m.deliver(&reg, 15, true), Ok(CallState::Blocked));
    }

    #[test]
    fn not_blocked_no_eintr() {
        let reg = SigRegistry::new();
        let mut m = InterruptMachine::new("wait");
        assert_eq!(m.deliver(&reg, 2, true), Ok(CallState::Runnable));
    }

    #[test]
    fn no_signal_no_eintr() {
        let reg = SigRegistry::new();
        let mut m = InterruptMachine::new("wait");
        let _ = m.block();
        assert_eq!(m.deliver(&reg, 2, false), Ok(CallState::Blocked));
    }

    #[test]
    fn truth_table_matrix_green() {
        let (l, per) = run_truth_table_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn slow_table_closed() {
        assert!(is_slow("wait"));
        assert!(!is_slow("getpid-like"));
    }

    #[test]
    fn retroactive_guard() {
        let reg = SigRegistry::new();
        let mut m = InterruptMachine::new("poll");
        let _ = m.block();
        let _ = m.complete();
        assert!(m.deliver(&reg, 2, true).is_err());
        assert_ne!(m.deliver(&reg, 2, true), Err(EINTR));
    }

    #[test]
    fn bad_signum_rejected() {
        let mut r = SigRegistry::new();
        assert!(r.install(SigAction {
            signum: 0,
            disp: Disposition::Handler,
            flags: 0,
        })
        .is_err());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EINTR));
        assert_eq!(linux_neg(EINTR), -EINTR);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9301_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
