//! UNX-F0285 · panic 后安全停机策略（停机戳/重试门限）
//!
//! 诊断段 0x9621~0x9628（panic 停机族第二段，0x9611 段归 F0284）。
//! 判据（施工书 J1）：停机戳写入/重试门限（同因 3 次）生效，死循环防护断言过。

use alloc::vec::Vec;

/// 停机策略码闭集（0x9621~0x9628 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopCode {
    /// 0x9621 停机戳写入成功。
    StampWritten = 0x9621,
    /// 0x9622 停机戳重复写入被拒（同 tick 同因只留一枚）。
    StampDupRejected = 0x9622,
    /// 0x9623 重试门限命中（同因第 3 次）。
    RetryLimitHit = 0x9623,
    /// 0x9624 重试门限未满（允许继续重试）。
    RetryBudgetOk = 0x9624,
    /// 0x9625 死循环防护触发（单位时间窗重试超密度）。
    LoopGuardTripped = 0x9625,
    /// 0x9626 停机路径选定（安全停机序列启动）。
    PathSelected = 0x9626,
    /// 0x9627 停机序列已完成（终态）。
    ShutdownDone = 0x9627,
    /// 0x9628 账面守恒破坏（内部一致性核失败）。
    LedgerBroken = 0x9628,
}

impl StopCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            StopCode::StampWritten => "停机戳已写入账面",
            StopCode::StampDupRejected => "同因同拍停机戳重复写入被拒",
            StopCode::RetryLimitHit => "同因重试已达门限 3 次",
            StopCode::RetryBudgetOk => "重试预算未用尽",
            StopCode::LoopGuardTripped => "单位时间窗重试密度超限",
            StopCode::PathSelected => "安全停机路径已选定",
            StopCode::ShutdownDone => "停机序列已走完",
            StopCode::LedgerBroken => "停机账守恒核失败",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            StopCode::StampWritten => "panic 因果首次落账",
            StopCode::StampDupRejected => "幂等闸拒绝重复戳",
            StopCode::RetryLimitHit => "同因三次重试消耗尽门限预算",
            StopCode::RetryBudgetOk => "重试次数在门限内",
            StopCode::LoopGuardTripped => "短窗高密度重试呈死循环特征",
            StopCode::PathSelected => "策略选择器给出确定路径",
            StopCode::ShutdownDone => "停机十步全部落账",
            StopCode::LedgerBroken => "写读不一致或守恒式断裂",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            StopCode::StampWritten => "无需处置",
            StopCode::StampDupRejected => "无需处置（幂等设计生效）",
            StopCode::RetryLimitHit => "进入安全停机，勿再重试同因",
            StopCode::RetryBudgetOk => "可继续重试并观察",
            StopCode::LoopGuardTripped => "强制停机并冻结重试账",
            StopCode::PathSelected => "按序列执行，勿插队",
            StopCode::ShutdownDone => "保持终态，等待复位",
            StopCode::LedgerBroken => "立即停机并上报 0x9628",
        }
    }
}

/// 停机戳：panic 因果指纹 + 首次落账 tick。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StopStamp {
    /// 同因指纹（FNV-1a of 因果串）。
    pub cause_fp: u64,
    /// 首次落账 tick。
    pub first_tick: u64,
}

/// 停机策略状态机：Observe（观察重试）→ Limit（门限命中）→ Shutting（停机中）→ Done（终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopFsm {
    /// 观察期：重试预算内。
    Observe,
    /// 门限命中：同因第 3 次。
    Limit,
    /// 停机执行中。
    Shutting,
    /// 终态。
    Done,
}

/// 停机十步闭集（安全停机序列，顺序严格）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopStep {
    /// 1 冻结调度。
    FreezeSched,
    /// 2 冲刷日志。
    FlushLog,
    /// 3 封账。
    SealLedgers,
    /// 4 通知他核（IPI 广播判定复用 F1863 语义）。
    NotifyPeers,
    /// 5 撤销未决 IO。
    RevokeIo,
    /// 6 恢复控制台。
    RestoreConsole,
    /// 7 写停机戳终态。
    SealStamp,
    /// 8 关中断。
    MaskIrq,
    /// 9 停看门狗。
    StopWatchdog,
    /// 10 进入停机点。
    Halt,
}

impl StopStep {
    /// 全序闭集。
    pub const ALL: [StopStep; 10] = [
        StopStep::FreezeSched,
        StopStep::FlushLog,
        StopStep::SealLedgers,
        StopStep::NotifyPeers,
        StopStep::RevokeIo,
        StopStep::RestoreConsole,
        StopStep::SealStamp,
        StopStep::MaskIrq,
        StopStep::StopWatchdog,
        StopStep::Halt,
    ];

    /// 步序号（1 起）。
    pub const fn ordinal(self) -> u8 {
        match self {
            StopStep::FreezeSched => 1,
            StopStep::FlushLog => 2,
            StopStep::SealLedgers => 3,
            StopStep::NotifyPeers => 4,
            StopStep::RevokeIo => 5,
            StopStep::RestoreConsole => 6,
            StopStep::SealStamp => 7,
            StopStep::MaskIrq => 8,
            StopStep::StopWatchdog => 9,
            StopStep::Halt => 10,
        }
    }
}

/// 死循环防护窗参数：单位窗内同因重试密度上限。
pub const LOOP_WINDOW_TICKS: u64 = 1024;
/// 窗内密度上限（次）。
pub const LOOP_WINDOW_MAX: usize = 8;
/// 同因重试门限（施工书：3 次）。
pub const RETRY_LIMIT: usize = 3;

/// 停机策略引擎。
pub struct PanicStopEngine {
    /// 当前状态机。
    fsm: StopFsm,
    /// 停机戳账（同因指纹 → 戳）。
    stamps: Vec<StopStamp>,
    /// 同因重试计数（指纹 → 次数）。
    retry_counts: Vec<(u64, usize)>,
    /// 死循环防护窗（指纹 → 窗内 tick 列表，容量封顶）。
    loop_win: Vec<(u64, Vec<u64>)>,
    /// 十步完成位图（bit i = 第 i+1 步完成）。
    steps_done: u16,
    /// 拦截/事件账。
    log: Vec<(StopCode, u64)>,
    /// tick。
    tick: u64,
    /// 账面守恒核基准。
    stamps_written: u64,
    stamps_rejected: u64,
}

impl PanicStopEngine {
    /// 新引擎。
    pub fn new() -> Self {
        PanicStopEngine {
            fsm: StopFsm::Observe,
            stamps: Vec::new(),
            retry_counts: Vec::new(),
            loop_win: Vec::new(),
            steps_done: 0,
            log: Vec::new(),
            tick: 0,
            stamps_written: 0,
            stamps_rejected: 0,
        }
    }

    /// FNV-1a 因果指纹。
    pub fn cause_fingerprint(cause: &str) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in cause.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    fn log(&mut self, c: StopCode) {
        self.log.push((c, self.tick));
    }

    /// 停机戳写入（幂等闸：同因同账只留一枚，重复写入拒）。
    pub fn write_stamp(&mut self, cause: &str) -> StopCode {
        let fp = Self::cause_fingerprint(cause);
        if self.stamps.iter().any(|s| s.cause_fp == fp) {
            self.stamps_rejected += 1;
            self.log(StopCode::StampDupRejected);
            return StopCode::StampDupRejected;
        }
        self.stamps.push(StopStamp { cause_fp: fp, first_tick: self.tick });
        self.stamps_written += 1;
        self.log(StopCode::StampWritten);
        StopCode::StampWritten
    }

    /// 重试申报：返回门限判定码。同因第 RETRY_LIMIT 次 → Limit。
    pub fn report_retry(&mut self, cause: &str) -> StopCode {
        let fp = Self::cause_fingerprint(cause);
        let n = match self.retry_counts.iter_mut().find(|(f, _)| *f == fp) {
            Some((_, c)) => {
                *c += 1;
                *c
            }
            None => {
                self.retry_counts.push((fp, 1));
                1
            }
        };
        // 死循环防护：窗内密度核。
        let had = self.loop_win.iter().any(|(f, _)| *f == fp);
        if !had {
            self.loop_win.push((fp, Vec::new()));
        }
        let win = match self.loop_win.iter_mut().find(|(f, _)| *f == fp) {
            Some((_, w)) => w,
            None => return StopCode::LedgerBroken,
        };
        win.push(self.tick);
        // 剔除窗外 tick（定容防线）。
        win.retain(|t| self.tick.saturating_sub(*t) <= LOOP_WINDOW_TICKS);
        if win.len() > LOOP_WINDOW_MAX {
            self.fsm = StopFsm::Limit;
            self.log(StopCode::LoopGuardTripped);
            return StopCode::LoopGuardTripped;
        }
        if n >= RETRY_LIMIT {
            self.fsm = StopFsm::Limit;
            self.log(StopCode::RetryLimitHit);
            StopCode::RetryLimitHit
        } else {
            self.log(StopCode::RetryBudgetOk);
            StopCode::RetryBudgetOk
        }
    }

    /// 门限命中 → 选定停机路径。
    pub fn select_path(&mut self) -> bool {
        if self.fsm != StopFsm::Limit || self.steps_done != 0 {
            return false;
        }
        self.fsm = StopFsm::Shutting;
        self.log(StopCode::PathSelected);
        true
    }

    /// 十步严格序执行：乱序/跳步拒绝。
    pub fn run_step(&mut self, s: StopStep) -> bool {
        if self.fsm != StopFsm::Shutting {
            return false;
        }
        let ord = s.ordinal();
        // 下一步必须是 (已完成的最高步 + 1)。
        let next = self.steps_done.count_ones() as u8 + 1;
        if ord != next || self.steps_done & (1 << (ord - 1)) != 0 {
            return false;
        }
        self.steps_done |= 1 << (ord - 1);
        if s == StopStep::Halt {
            self.fsm = StopFsm::Done;
            self.log(StopCode::ShutdownDone);
        }
        true
    }

    /// 状态。
    pub const fn fsm(&self) -> StopFsm {
        self.fsm
    }
    /// 已完成步数。
    pub const fn steps_done(&self) -> u8 {
        self.steps_done.count_ones() as u8
    }
    /// 事件账。
    pub fn events(&self) -> &[(StopCode, u64)] {
        &self.log
    }
    /// 停机戳数。
    pub fn stamp_count(&self) -> usize {
        self.stamps.len()
    }

    /// 账面守恒核：written - rejected == stamps.len()，且 Done 态必十步全完。
    pub fn ledger_consistent(&self) -> bool {
        if self.stamps_written != self.stamps.len() as u64 {
            return false;
        }
        if self.fsm == StopFsm::Done && self.steps_done != 0x3FF {
            return false;
        }
        true
    }

    /// 死循环注入判定（独立口径）：窗内密度核直读。
    pub fn loop_guard_active(&self, cause: &str) -> bool {
        let fp = Self::cause_fingerprint(cause);
        self.loop_win
            .iter()
            .find(|(f, _)| *f == fp)
            .map(|(_, w)| w.len() > LOOP_WINDOW_MAX)
            .unwrap_or(false)
    }
}

/// 判据测试（T284 系，段 0x9621~0x9628）。
pub fn run_t285_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t285");
    // c1 停机戳写入 + 幂等闸。
    let mut e = PanicStopEngine::new();
    let c1a = e.write_stamp("ovf@kheap") == StopCode::StampWritten;
    let c1b = e.write_stamp("ovf@kheap") == StopCode::StampDupRejected;
    let c1c = e.stamp_count() == 1;
    set.add("t285-c1-stamp-idem", c1a && c1b && c1c, "");
    // c2 异因异戳。
    let c2 = e.write_stamp("page@user") == StopCode::StampWritten && e.stamp_count() == 2;
    set.add("t285-c2-multi-cause", c2, "");
    // c3 重试门限：同因第 3 次命中。
    let mut e2 = PanicStopEngine::new();
    let r1 = e2.report_retry("deadlock@ipc");
    let r2 = e2.report_retry("deadlock@ipc");
    let r3 = e2.report_retry("deadlock@ipc");
    set.add(
        "t285-c3-retry-limit",
        r1 == StopCode::RetryBudgetOk && r2 == StopCode::RetryBudgetOk && r3 == StopCode::RetryLimitHit && e2.fsm() == StopFsm::Limit,
        "",
    );
    // c4 门限命中才能选路径，Observe 态拒绝。
    let mut e3 = PanicStopEngine::new();
    let deny = !e3.select_path();
    let _ = e3.report_retry("x");
    let _ = e3.report_retry("x");
    let _ = e3.report_retry("x");
    set.add("t285-c4-path-gate", deny && e3.select_path(), "");
    // c5 十步严格序：跳步/乱序拒绝。
    let mut ok = true;
    // 跳步尝试（直接 Halt）拒绝。
    ok &= !e3.run_step(StopStep::Halt);
    // 正序全走。
    for i in 0..10 {
        ok &= e3.run_step(StopStep::ALL[i]);
    }
    // 重放已完步拒绝。
    ok &= !e3.run_step(StopStep::FreezeSched);
    ok &= e3.steps_done() == 10 && e3.fsm() == StopFsm::Done;
    set.add("t285-c5-steps-strict", ok, "");
    // c6 死循环防护：窗内密度超限触发。
    let mut e4 = PanicStopEngine::new();
    let mut tripped = false;
    for _ in 0..(LOOP_WINDOW_MAX + 2) {
        match e4.report_retry("spin@idle") {
            StopCode::LoopGuardTripped => tripped = true,
            _ => {}
        }
    }
    set.add("t285-c6-loop-guard", tripped && e4.loop_guard_active("spin@idle"), "");
    // c7 守恒核 + 终态十步位图。
    let e3_ok = e3.ledger_consistent();
    let e_ok = e.ledger_consistent();
    set.add("t285-c7-ledger", e_ok && e3_ok, "");
    // c8 LCG 512 轮注入：k 次重试（k∈1..=3）时，第 k 次返回 Limit 当且仅当 k==3。
    let mut seed: u64 = 0x2850_9621_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut eng = PanicStopEngine::new();
        let causes = ["a", "b", "c", "d", "e"];
        let cause = causes[(lc() % 5) as usize];
        let k = 1 + (lc() % 3) as usize;
        let mut last = StopCode::RetryBudgetOk;
        for _ in 0..k {
            last = eng.report_retry(cause);
        }
        let expect = if k == 3 { StopCode::RetryLimitHit } else { StopCode::RetryBudgetOk };
        if last != expect || eng.fsm() != (if k == 3 { StopFsm::Limit } else { StopFsm::Observe }) {
            inj_ok = false;
        }
    }
    set.add("t285-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        StopCode::StampWritten,
        StopCode::StampDupRejected,
        StopCode::RetryLimitHit,
        StopCode::RetryBudgetOk,
        StopCode::LoopGuardTripped,
        StopCode::PathSelected,
        StopCode::ShutdownDone,
        StopCode::LedgerBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t285-c9-triad", tri_ok, "");
    set
}
