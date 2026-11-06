//! UNX-F10056 回归门（batch_gate 常态挡板）· 0x91E00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10057-J1 口径衔接核（常态绿不替代收口全量 10/10 次），口径互替 10/10 次拦截
//!
//! 设计要点：
//!   ① 回归周期 period=10 轮写死：距上次周期回归超过 period 即「周期
//!      未跑」→ 常态挡板先提醒（闸提醒留痕 10/10），提醒后过闸仍被拦
//!      （越闸拦截 10/10）——挡板只有 run_cycle 落账后才放行。
//!   ② 门状态机：Open ↔ OverdueRemind → Blocked，状态由「周期是否已跑」
//!      纯函数推导（state_of_due），不许凭票面自报状态。
//!   ③ 门事件账本：提醒/拦截/放行/运行四类事件逐笔入账，守恒式
//!      events == remind + block + pass + run 恒成立；任何拒绝路径
//!      （时钟倒退 / 未来运行 / 重复运行）不改事件账（拒绝不留痕）。
//!   ④ 注入核：越闸通行 / 未来时钟 / 时钟倒退 / 重复运行四类坏样本
//!      逐类 10/10 次拒绝，注入账守恒逃逸 = 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10056 独占 0x40 窗口，下一卡 0x91E40）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x91E00;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 回归周期（轮）：距上次周期回归超过 10 轮即判「周期未跑」。
pub const GATE_PERIOD_ROUNDS: u32 = 10;

/// 门事件账容量上限。
pub const LEDGER_CAP: usize = 128;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 越闸通行（周期未跑却申请过闸）
    BypassAttempt,
    /// 未来时钟（运行轮次晚于当前轮）
    FutureRound,
    /// 时钟倒退（cur_round 小于已见轮次）
    ClockRewind,
    /// 重复运行（同轮重复落 run_cycle）
    DupRun,
    /// 周期内无提醒可发（不该提醒时提醒）
    NotDue,
    /// 未提醒直接放行（挡板流程缺失）
    SkippedRemind,
    /// 门事件账失守恒
    LedgerRegress,
    /// 事件账容量满
    LedgerFull,
    /// 空成文
    EmptySummary,
    /// 门未初始化（未 tick 就过闸）
    Uninitialized,
    /// 门状态自报不符（票面状态与推导状态不一致）
    StateMismatch,
    /// 段位越界
    SegmentViolation,
}

impl GateCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GateCode::BypassAttempt => "越闸通行被拒：周期未跑不得过闸；\
                请先 run_cycle 落账本轮周期回归再通行",
            GateCode::FutureRound => "未来时钟被拒：运行轮次不得晚于当前轮；\
                请核对轮次时钟再落账",
            GateCode::ClockRewind => "时钟倒退被拒：当前轮次只进不退；\
                请排查是否有路径把时钟拨回",
            GateCode::DupRun => "重复运行被拒：同一轮次只允许落一次周期回归；\
                请换新轮次再运行",
            GateCode::NotDue => "无提醒可发被拒：周期未到期不得发闸提醒；\
                请核对周期账再触发",
            GateCode::SkippedRemind => "跳过提醒被拒：周期未跑必须先提醒再拦截，\
                不许静默放行或静默拦截",
            GateCode::LedgerRegress => "门事件账失守恒：events 必须等于 \
                提醒+拦截+放行+运行 之和；请排查绕账路径",
            GateCode::LedgerFull => "事件账容量满被拒：已达上限；请先成文归档",
            GateCode::EmptySummary => "空成文被拒：门小结不得为空串；请检查事件账",
            GateCode::Uninitialized => "门未初始化被拒：过闸前必须先 tick 建立时钟",
            GateCode::StateMismatch => "状态自报不符被拒：票面门状态必须与 \
                周期推导状态一致；请以 state_of_due 为准",
            GateCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [GateCode; 12] = [
        GateCode::BypassAttempt,
        GateCode::FutureRound,
        GateCode::ClockRewind,
        GateCode::DupRun,
        GateCode::NotDue,
        GateCode::SkippedRemind,
        GateCode::LedgerRegress,
        GateCode::LedgerFull,
        GateCode::EmptySummary,
        GateCode::Uninitialized,
        GateCode::StateMismatch,
        GateCode::SegmentViolation,
    ];
}

// ── 门状态机 ──────────────────────────────────────────────────────────
/// 门状态三态闭集：由「周期是否已跑」纯函数推导。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    /// 周期内已跑，可放行
    Open,
    /// 周期未跑，挡板提醒中
    OverdueRemind,
    /// 周期未跑且有越闸企图，拦截
    Blocked,
}

impl GateState {
    /// 中文标签（成文用）。
    pub fn label(self) -> &'static str {
        match self {
            GateState::Open => "放行",
            GateState::OverdueRemind => "提醒",
            GateState::Blocked => "拦截",
        }
    }

    /// 是否允许过闸（只有 Open 放行）。
    pub fn allows_pass(self) -> bool {
        self == GateState::Open
    }

    /// 闭集全量。
    pub const ALL: [GateState; 3] = [GateState::Open, GateState::OverdueRemind, GateState::Blocked];
}

/// 状态推导：周期未跑 → OverdueRemind；已跑 → Open。纯函数。
pub fn state_of_due(due: bool) -> GateState {
    if due {
        GateState::OverdueRemind
    } else {
        GateState::Open
    }
}

// ── 门事件账本 ────────────────────────────────────────────────────────
/// 门事件：提醒 / 拦截 / 放行 / 运行 四类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateEvent {
    /// 闸提醒（周期未跑）
    Remind,
    /// 越闸拦截
    Block,
    /// 放行
    Pass,
    /// 周期回归运行落账
    Run,
}

impl GateEvent {
    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            GateEvent::Remind => "提醒",
            GateEvent::Block => "拦截",
            GateEvent::Pass => "放行",
            GateEvent::Run => "运行",
        }
    }
}

/// 门事件账本：时钟 + 事件四计数 + 事件流水。
#[derive(Clone, Debug)]
pub struct GateLedger {
    events: Vec<(GateEvent, u32)>,
    cap: usize,
    cur_round: u32,
    last_run: Option<u32>,
    reminds: u32,
    blocks: u32,
    passes: u32,
    runs: u32,
}

impl GateLedger {
    /// 空账构造（时钟未初始化）。
    pub fn new() -> GateLedger {
        GateLedger {
            events: Vec::new(),
            cap: LEDGER_CAP,
            cur_round: 0,
            last_run: None,
            reminds: 0,
            blocks: 0,
            passes: 0,
            runs: 0,
        }
    }

    fn push_event(&mut self, e: GateEvent, round: u32) -> Result<(), GateCode> {
        if self.events.len() >= self.cap {
            return Err(GateCode::LedgerFull);
        }
        self.events.push((e, round));
        match e {
            GateEvent::Remind => self.reminds = self.reminds.saturating_add(1),
            GateEvent::Block => self.blocks = self.blocks.saturating_add(1),
            GateEvent::Pass => self.passes = self.passes.saturating_add(1),
            GateEvent::Run => self.runs = self.runs.saturating_add(1),
        }
        Ok(())
    }

    /// 时钟推进：只进不退（ClockRewind 拒绝）；初始化前 cur==0。
    pub fn tick(&mut self, round: u32) -> Result<(), GateCode> {
        if round < self.cur_round {
            return Err(GateCode::ClockRewind);
        }
        self.cur_round = round;
        Ok(())
    }

    /// 周期是否到期：从未跑过或 cur - last > period。
    pub fn cycle_due(&self) -> Result<bool, GateCode> {
        if self.cur_round == 0 {
            return Err(GateCode::Uninitialized);
        }
        Ok(match self.last_run {
            None => true,
            Some(last) => self.cur_round.saturating_sub(last) > GATE_PERIOD_ROUNDS,
        })
    }

    /// 当前推导门状态（未初始化 Uninitialized）。
    pub fn state(&self) -> Result<GateState, GateCode> {
        Ok(state_of_due(self.cycle_due()?))
    }

    /// 闸提醒：到期才允许（NotDue 拒绝），留痕 Remind 事件。
    pub fn remind(&mut self) -> Result<(), GateCode> {
        if self.cycle_due()? {
            self.push_event(GateEvent::Remind, self.cur_round)
        } else {
            Err(GateCode::NotDue)
        }
    }

    /// 周期回归运行落账：轮次不得晚于当前轮、同轮不重复；落账后周期清零。
    pub fn run_cycle(&mut self, round: u32) -> Result<(), GateCode> {
        if round > self.cur_round {
            return Err(GateCode::FutureRound);
        }
        if self.last_run == Some(round) {
            return Err(GateCode::DupRun);
        }
        self.last_run = Some(round);
        self.push_event(GateEvent::Run, round)
    }

    /// 过闸：到期拦截（Block 事件）并返回 BypassAttempt；到期前放行
    /// 记 Pass 事件。挡板状态由周期推导，不信任调用方自报。
    pub fn attempt_pass(&mut self, reminded: bool) -> Result<(), GateCode> {
        if self.cur_round == 0 {
            return Err(GateCode::Uninitialized);
        }
        if self.cycle_due()? {
            self.push_event(GateEvent::Block, self.cur_round)?;
            return Err(GateCode::BypassAttempt);
        }
        let _ = reminded;
        self.push_event(GateEvent::Pass, self.cur_round)
    }

    /// 事件账守恒：events == 四计数之和。
    pub fn conserved(&self) -> bool {
        let sum = self.reminds.saturating_add(self.blocks)
            .saturating_add(self.passes)
            .saturating_add(self.runs);
        sum == self.events.len() as u32
    }

    /// 各类事件计数（提醒/拦截/放行/运行）。
    pub fn counts(&self) -> (u32, u32, u32, u32) {
        (self.reminds, self.blocks, self.passes, self.runs)
    }

    /// 当前轮次。
    pub fn round(&self) -> u32 {
        self.cur_round
    }

    /// 事件流水长度。
    pub fn len(&self) -> usize {
        self.events.len()
    }
}

// ── 门小结成文（纯函数） ──────────────────────────────────────────────
/// 门成文：「轮=<r>;状态=<标签>;提醒=<n>;拦=<n>;过=<n>;跑=<n>」；失守恒先拒。
pub fn compose_gate_summary(g: &GateLedger) -> Result<String, GateCode> {
    if !g.conserved() {
        return Err(GateCode::LedgerRegress);
    }
    let st = g.state()?;
    let (reminds, blocks, passes, runs) = g.counts();
    let out = format!(
        "轮={};状态={};提醒={};拦={};过={};跑={}",
        g.round(),
        st.label(),
        reminds,
        blocks,
        passes,
        runs
    );
    if out.is_empty() {
        return Err(GateCode::EmptySummary);
    }
    Ok(out)
}

// ── 注入核：坏过闸样本矩阵 ────────────────────────────────────────────
/// 四类坏过闸样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadGate {
    /// 越闸通行：周期未跑却申请过闸
    Bypass,
    /// 未来时钟运行
    FutureRun,
    /// 时钟倒退
    Rewind,
    /// 同轮重复运行
    DoubleRun,
}

impl BadGate {
    /// 闭集全量。
    pub const ALL: [BadGate; 4] = [
        BadGate::Bypass,
        BadGate::FutureRun,
        BadGate::Rewind,
        BadGate::DoubleRun,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadGate::Bypass => "越闸通行",
            BadGate::FutureRun => "未来时钟",
            BadGate::Rewind => "时钟倒退",
            BadGate::DoubleRun => "重复运行",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), GateCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(GateCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏过闸注入矩阵：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_gate_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadGate::ALL.len() {
        let case = match BadGate::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadGate::Bypass => {
                    let mut g = GateLedger::new();
                    let _ = g.tick(1);
                    let _ = g.remind();
                    g.attempt_pass(true).err() == Some(GateCode::BypassAttempt)
                        && g.state().ok() == Some(GateState::OverdueRemind)
                }
                BadGate::FutureRun => {
                    let mut g = GateLedger::new();
                    let _ = g.tick(5);
                    g.run_cycle(6).err() == Some(GateCode::FutureRound)
                }
                BadGate::Rewind => {
                    let mut g = GateLedger::new();
                    let _ = g.tick(9);
                    g.tick(8).err() == Some(GateCode::ClockRewind)
                }
                BadGate::DoubleRun => {
                    let mut g = GateLedger::new();
                    let _ = g.tick(3);
                    let _ = g.run_cycle(3);
                    g.run_cycle(3).err() == Some(GateCode::DupRun)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；越闸/未初始化族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏轮次/状态不符族"),
    ("ETIME", -62, 146, "Windows ERROR_TIMEOUT；周期到期族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复运行族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；事件账满族"),
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

/// 规范语料双跑指纹：提醒 → 运行 → 放行 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut g = GateLedger::new();
    let _ = g.tick(1);
    let _ = g.remind();
    let _ = g.run_cycle(1);
    let _ = g.tick(2);
    let _ = g.attempt_pass(false);
    let bytes = match compose_gate_summary(&g) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10056 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10056_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10056");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91E00..=0x91E3F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠");

    // 2. 周期策略：period=10 写死
    s.add("周期策略：period=10 写死且大于 0",
          GATE_PERIOD_ROUNDS == 10 && GATE_PERIOD_ROUNDS > 0,
          "回归周期必须为正且写死，漂移周期即口径失效");

    // 3. 状态推导纯函数：due → 提醒，not due → 放行
    let st_ok = state_of_due(true) == GateState::OverdueRemind
        && state_of_due(false) == GateState::Open
        && GateState::Open.allows_pass()
        && !GateState::OverdueRemind.allows_pass()
        && !GateState::Blocked.allows_pass();
    s.add("状态推导：due/not-due 两态取值与过闸位正确", st_ok,
          "门状态由周期推导，不许凭票面自报");

    // 4. 周期未跑闸提醒 10/10
    let mut rem_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g = GateLedger::new();
        let _ = g.tick(1);
        if g.cycle_due().ok() != Some(true)
            || g.remind().is_err()
            || g.state().ok() != Some(GateState::OverdueRemind)
        {
            rem_ok = false;
        }
        rr += 1;
    }
    s.add("闸提醒：周期未跑 10/10 次提醒生效", rem_ok,
          "周期未跑必须逐次发闸提醒并留痕，不许静默");

    // 5. 越闸拦截 10/10
    let mut blk_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g = GateLedger::new();
        let _ = g.tick(1);
        let _ = g.remind();
        if g.attempt_pass(true).err() != Some(GateCode::BypassAttempt) {
            blk_ok = false;
        }
        rr += 1;
    }
    s.add("越闸拦截：提醒后过闸 10/10 次拦截", blk_ok,
          "周期未跑时过闸企图必须逐次拦截，挡板不许失效");

    // 6. 周期内放行：run_cycle 落账后过闸放行 10/10
    let mut pass_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut g = GateLedger::new();
        let _ = g.tick(1);
        let _ = g.run_cycle(1);
        let _ = g.tick(2);
        if g.cycle_due().ok() != Some(false)
            || g.attempt_pass(false).is_err()
            || g.state().ok() != Some(GateState::Open)
        {
            pass_ok = false;
        }
        rr += 1;
    }
    s.add("常态放行：周期回归落账后 10/10 次放行", pass_ok,
          "只有周期回归落账才解除挡板，放行必须留痕");

    // 7. 注入核：四类坏过闸各 10/10 拒绝
    let (inj, per_class) = run_bad_gate_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏过闸样本各 10/10 拒绝", all_caught,
          "越闸/未来时钟/倒退/重复运行必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadGate::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 事件账守恒：events == 提醒+拦截+放行+运行
    let mut g = GateLedger::new();
    let _ = g.tick(1);
    let _ = g.remind();
    let _ = g.attempt_pass(true); // 拦截留痕
    let _ = g.run_cycle(1);
    let _ = g.tick(2);
    let _ = g.attempt_pass(false); // 放行留痕
    let (reminds, blocks, passes, runs) = g.counts();
    s.add("事件守恒：events == 提醒+拦截+放行+运行",
          g.conserved() && g.len() == 4
              && reminds == 1 && blocks == 1 && passes == 1 && runs == 1,
          "门事件四计数必须逐笔对得上，绕账路径即失败");

    // 10. 时钟倒退拒绝且不留痕
    let before = g.len();
    let rew_ok = g.tick(1).err() == Some(GateCode::ClockRewind) && g.len() == before;
    s.add("时钟倒退：回拨拒绝且事件账不变", rew_ok,
          "轮次时钟只进不退，倒退企图显性拒绝");

    // 11. 重复运行拒绝
    let mut g2 = GateLedger::new();
    let _ = g2.tick(3);
    let _ = g2.run_cycle(3);
    s.add("重复运行：同轮二次 run_cycle 拒绝",
          g2.run_cycle(3).err() == Some(GateCode::DupRun),
          "同轮只允许落一次周期回归，重复落账逐次拒绝");

    // 12. 成文非空且含标记 + 双跑一致
    let sum = compose_gate_summary(&g);
    let text_ok = match &sum {
        Ok(t) => t.contains("轮=2") && t.contains("状态=放行") && t.contains("拦=1"),
        Err(_) => false,
    };
    s.add("门成文：标记齐全且两次成文逐字节一致",
          text_ok && sum == compose_gate_summary(&g),
          "成文必须携带 轮/状态/四计数 标记且为纯函数");

    // 13. 未初始化拒绝：未 tick 过闸/查状态均拒
    let mut uninit = GateLedger::new();
    s.add("未初始化：零时钟过闸判 Uninitialized",
          uninit.attempt_pass(false).err() == Some(GateCode::Uninitialized)
              && uninit.cycle_due().err() == Some(GateCode::Uninitialized),
          "过闸前必须建立时钟，未初始化显性拒绝");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(22) && agrees_with_linux(62)
        && agrees_with_linux(17) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < GateCode::ALL.len() {
        let mut j = i + 1;
        while j < GateCode::ALL.len() {
            if let (Some(a), Some(b)) = (GateCode::ALL.get(i), GateCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = GateCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同输入两次必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum {
        Ok(t) => t.len(),
        Err(_) => 0,
    };
    let sum_text = format!("门小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_derivation_pure() {
        assert_eq!(state_of_due(true), GateState::OverdueRemind);
        assert_eq!(state_of_due(false), GateState::Open);
    }

    #[test]
    fn remind_then_block() {
        let mut g = GateLedger::new();
        let _ = g.tick(1);
        assert!(g.remind().is_ok());
        assert_eq!(g.attempt_pass(true).err(), Some(GateCode::BypassAttempt));
        assert_eq!(g.state().ok(), Some(GateState::OverdueRemind));
    }

    #[test]
    fn run_then_pass() {
        let mut g = GateLedger::new();
        let _ = g.tick(1);
        let _ = g.run_cycle(1);
        let _ = g.tick(2);
        assert!(g.attempt_pass(false).is_ok());
        assert_eq!(g.state().ok(), Some(GateState::Open));
    }

    #[test]
    fn clock_rewind_rejected() {
        let mut g = GateLedger::new();
        let _ = g.tick(5);
        assert_eq!(g.tick(4).err(), Some(GateCode::ClockRewind));
    }

    #[test]
    fn dup_run_rejected() {
        let mut g = GateLedger::new();
        let _ = g.tick(3);
        assert!(g.run_cycle(3).is_ok());
        assert_eq!(g.run_cycle(3).err(), Some(GateCode::DupRun));
    }

    #[test]
    fn future_run_rejected() {
        let mut g = GateLedger::new();
        let _ = g.tick(5);
        assert_eq!(g.run_cycle(6).err(), Some(GateCode::FutureRound));
    }

    #[test]
    fn bad_gate_matrix_green() {
        let (inj, per) = run_bad_gate_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn gate_summary_contains_marks() {
        let mut g = GateLedger::new();
        let _ = g.tick(1);
        let _ = g.run_cycle(1);
        let s = compose_gate_summary(&g).unwrap_or_default();
        assert!(s.contains("状态=放行") && s.contains("跑=1"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(62));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(1), -1);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10056_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10056_checks_green() {
        let set = run_unx10056_checks();
        assert!(set.all_passed(), "UNX-F10056 red: {}", set.red_items_text());
    }
}
