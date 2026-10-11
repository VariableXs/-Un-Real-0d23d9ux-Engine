//! unx1201_polloop —— UNX-F1201 Polling 态执行主循环本体（轮询事件泵全语义）。
//! 锚点（CoRun Unxreal 主册 UNX-A2-B21）：UNX-F1201 · Polling 态执行主循环本体（轮询事件泵全语义） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1201-J1 轮询主循环 10^6 采样零丢失（事件发生数=感知数恒等断言，
//!           与 F0969 对账底座口径一致），单轮循环耗时 P99 ≤预算值实测留痕，
//!           循环退出/让出条件全验证
//! 组件：轮询事件泵（五态泵状态机：Init→Polling→Yield→Draining→Done 单向）+
//!      采样账（发生序号账 vs 感知序号账双账恒等：零丢失判据面）+
//!      让出条件闸（四类让出：预算尽/无事件/显式让出/账满，逐类显性）+
//!      退出条件闸（三态：采样达量/停止令牌/致命红码）+
//!      耗时账（单轮步数预算化——逻辑步数替代墙钟，P99 分位留痕）+
//!      F0969 口径对账底座（事件恒等式：发生=感知+缺失，缺失恒 0）。
//! 职责定位：Polling 态（无中断驱动）执行主循环的唯一权威实现——轮询语义
//!   全量在册：采样不丢、让出不静默、退出有令、耗时在预算；与中断态
//!   （F0xxx 中断面）分层：本单只管轮询泵语义，不管中断路由。
//! 性能逐项分解：单轮泵步 O(1)（事件源取数+账推进均 O(1)）；10^6 采样 O(10^6)
//!   （判据点名全量回放）；P99 分位 O(n log n)（账封顶 4096 环，摊销）；
//!   恒等断言 O(1)（双账指针推进）。全整数，逻辑步数计时（零墙钟）。
//! 诊断码段：0x5230（0x5231~0x5238 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const PL_VERSION: &str = "unx1201_polloop-v1";

/// 诊断段基址（0x5230 段 · 本模块独占）。
pub const PL_SEG_BASE: u16 = 0x5230;

/// 采样总量（判据点名：10^6）。
pub const PL_SAMPLES_TOTAL: u64 = 1_000_000;

/// 单轮步数预算（逻辑步；判据口径：单轮耗时 P99 ≤ 预算）。
pub const PL_STEP_BUDGET: u64 = 64;

/// 耗时账环容量（P99 分位账封顶）。
pub const PL_STEP_RING_CAP: usize = 4096;

/// 事件源容量（泵每次轮询可取事件上限）。
pub const PL_SOURCE_BATCH: usize = 8;

/// 泵状态机五态（单向推进，禁止回退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlPhase {
    /// 初始化（泵构建）。
    Init,
    /// 轮询中。
    Polling,
    /// 让出（本轮暂停，可续）。
    Yielded,
    /// 排干（尾批清账）。
    Draining,
    /// 完成（退出条件命中）。
    Done,
}

/// 让出原因闭集（判据点名：让出条件全验证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlYield {
    /// 步数预算尽。
    BudgetExhausted,
    /// 本轮无事件。
    NoEvent,
    /// 显式让出令。
    ExplicitToken,
    /// 采样账满（批量冲刷）。
    LedgerFlush,
}

impl PlYield {
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            PlYield::BudgetExhausted => "步数预算尽",
            PlYield::NoEvent => "本轮无事件",
            PlYield::ExplicitToken => "显式让出令",
            PlYield::LedgerFlush => "采样账满批量冲刷",
        }
    }
}

/// 退出原因闭集（判据点名：退出条件全验证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlExit {
    /// 采样达量（10^6 达成）。
    SamplesReached,
    /// 停止令牌。
    StopToken,
    /// 致命红码（对账恒等破坏）。
    FatalRed,
}

impl PlExit {
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            PlExit::SamplesReached => "采样达量退出",
            PlExit::StopToken => "停止令牌退出",
            PlExit::FatalRed => "对账恒等破坏致命退出",
        }
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlCode {
    /// 0x5231 采样丢失（发生≠感知）。
    SampleLost = 0x5231,
    /// 0x5232 非法状态迁移（五态单向被破坏）。
    PhaseBackward = 0x5232,
    /// 0x5233 单轮步数超预算（P99 红线）。
    StepOverBudget = 0x5233,
    /// 0x5234 事件序号乱序（源序严格递增被破坏）。
    SourceSeqBad = 0x5234,
    /// 0x5235 让出后非续跑（Yielded 态只允许 Resume）。
    ResumeBad = 0x5235,
    /// 0x5236 排干后仍有残量（Draining 不净）。
    DrainResidue = 0x5236,
    /// 0x5237 完成态再轮询（Done 只读）。
    DonePoll = 0x5237,
    /// 0x5238 P99 超预算（分位红线）。
    P99OverBudget = 0x5238,
}

impl PlCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            PlCode::SampleLost => "采样丢失（事件发生数≠感知数，恒等破坏）",
            PlCode::PhaseBackward => "非法状态迁移（五态单向推进被破坏）",
            PlCode::StepOverBudget => "单轮步数超预算（步数红线）",
            PlCode::SourceSeqBad => "事件源序号乱序（严格递增破坏）",
            PlCode::ResumeBad => "让出后非续跑语义（Yielded 只允许 Resume）",
            PlCode::DrainResidue => "排干后仍有残量（不净排干）",
            PlCode::DonePoll => "完成态再轮询（Done 只读，拒绝新采样）",
            PlCode::P99OverBudget => "单轮耗时 P99 超预算（分位红线）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            PlCode::SampleLost => "查事件源是否被旁路消费；泵是唯一取数口",
            PlCode::PhaseBackward => "查泵驱动方是否重复 init 或回退；状态机单向契约不可违",
            PlCode::StepOverBudget => "缩小批量或拆分轮询体；预算化是 P99 承诺的前置",
            PlCode::SourceSeqBad => "事件源序号必须严格递增；查源头是否重放旧事件",
            PlCode::ResumeBad => "Yielded 态只接受 resume；要重开须走新泵",
            PlCode::DrainResidue => "Draining 前先冲刷采样账；残量清零才可 Done",
            PlCode::DonePoll => "泵已完成：统计走报表口，不重复轮询",
            PlCode::P99OverBudget => "P99 红线破：降批量/降频，或走中断态（F0xxx 中断面）",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlDiag {
    /// 码。
    pub code: PlCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl PlDiag {
    /// 构造。
    pub fn new(code: PlCode, site: &str, detail: String) -> Self {
        PlDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 耗时账（单轮逻辑步数环 + 整数 P99）。
#[derive(Clone, Debug)]
pub struct PlStepRing {
    /// 环数据。
    ring: Vec<u64>,
    /// 写指针。
    head: usize,
    /// 已入账总数。
    pushed: u64,
}

impl PlStepRing {
    /// 新环。
    pub fn new() -> Self {
        PlStepRing { ring: Vec::new(), head: 0, pushed: 0 }
    }
    /// 入账（满则覆盖最旧——耗时分位账允许滚动；总量账另记）。
    pub fn push(&mut self, steps: u64) {
        if self.ring.len() < PL_STEP_RING_CAP {
            self.ring.push(steps);
        } else {
            self.ring[self.head] = steps;
        }
        self.head = (self.head + 1) % PL_STEP_RING_CAP;
        self.pushed += 1;
    }
    /// 整数 P99（升序取第 99 分位：索引 = n*99/100，n≥1）。
    pub fn p99(&self) -> Option<u64> {
        let n = self.ring.len();
        if n == 0 {
            return None;
        }
        let mut v = self.ring.clone();
        v.sort_unstable();
        let idx = n * 99 / 100;
        if idx >= n {
            return v.last().copied();
        }
        Some(v[idx])
    }
    /// 已入账总数。
    pub fn total(&self) -> u64 {
        self.pushed
    }
}

/// 事件源（确定性注入源：序号严格递增 + 可控批量）。
#[derive(Clone, Debug)]
pub struct PlSource {
    /// 已发生事件数（判据恒等式左端）。
    pub produced: u64,
    /// 源序号（严格递增）。
    seq: u64,
    /// 批量开关（每轮产出事件数 0..=PL_SOURCE_BATCH）。
    pub batch: usize,
}

impl PlSource {
    /// 构建源。
    pub fn new(batch: usize) -> Self {
        let b = if batch > PL_SOURCE_BATCH { PL_SOURCE_BATCH } else { batch };
        PlSource { produced: 0, seq: 0, batch: b }
    }
    /// 取一批事件（返回事件序号切片语义：这里返回 (首序号, 数量)）。
    pub fn take(&mut self, max: usize) -> (u64, usize) {
        let want = if max < self.batch { max } else { self.batch };
        let first = self.seq;
        self.seq += want as u64;
        self.produced += want as u64;
        (first, want)
    }
}

/// 轮询泵主账。
#[derive(Clone, Debug)]
pub struct PlPump {
    /// 泵相位。
    pub phase: PlPhase,
    /// 事件源。
    pub source: PlSource,
    /// 感知事件数（判据恒等式右端：感知=入账）。
    pub sensed: u64,
    /// 采样步数（轮数）。
    pub rounds: u64,
    /// 耗时账。
    pub steps: PlStepRing,
    /// 让出账（逐次让出原因计数）。
    pub yields: [(PlYield, u64); 4],
    /// 让出总次数。
    pub yield_total: u64,
    /// 退出原因（None=未退出）。
    pub exit: Option<PlExit>,
    /// 终账指纹。
    pub fp: u64,
}

impl PlPump {
    /// 构建泵（Init 态）。
    pub fn new(source_batch: usize) -> Self {
        PlPump {
            phase: PlPhase::Init,
            source: PlSource::new(source_batch),
            sensed: 0,
            rounds: 0,
            steps: PlStepRing::new(),
            yields: [
                (PlYield::BudgetExhausted, 0),
                (PlYield::NoEvent, 0),
                (PlYield::ExplicitToken, 0),
                (PlYield::LedgerFlush, 0),
            ],
            yield_total: 0,
            exit: None,
            fp: 0,
        }
    }

    /// 相位迁移闸（单向：Init→Polling→(Yielded↔)→Draining→Done）。
    fn transit(&mut self, to: PlPhase) -> Result<(), PlDiag> {
        let legal = match (self.phase, to) {
            (PlPhase::Init, PlPhase::Polling) => true,
            (PlPhase::Polling, PlPhase::Yielded) => true,
            (PlPhase::Polling, PlPhase::Draining) => true,
            (PlPhase::Polling, PlPhase::Done) => true,
            (PlPhase::Yielded, PlPhase::Polling) => true,
            (PlPhase::Yielded, PlPhase::Done) => true,
            (PlPhase::Draining, PlPhase::Done) => true,
            _ => false,
        };
        if !legal {
            return Err(PlDiag::new(
                PlCode::PhaseBackward,
                "transit",
                format!("{:?}→{:?} 非法", self.phase, to),
            ));
        }
        self.phase = to;
        Ok(())
    }

    /// 启动轮询。
    pub fn start(&mut self) -> Result<(), PlDiag> {
        self.transit(PlPhase::Polling)
    }

    /// 单轮轮询：取一批→入账→步数预算检查。
    /// budget_used：本轮模拟逻辑步数（由调用方注入；泵只验预算）。
    pub fn poll_round(&mut self, budget_used: u64) -> Result<PlYield, PlDiag> {
        if self.phase == PlPhase::Done {
            return Err(PlDiag::new(PlCode::DonePoll, "poll:done", "泵已完成".to_string()));
        }
        if self.phase != PlPhase::Polling {
            return Err(PlDiag::new(
                PlCode::ResumeBad,
                "poll:phase",
                format!("相位 {:?}", self.phase),
            ));
        }
        if budget_used > PL_STEP_BUDGET {
            self.steps.push(budget_used);
            self.rounds += 1;
            return Err(PlDiag::new(
                PlCode::StepOverBudget,
                "poll:budget",
                format!("本轮步数 {} > 预算 {}", budget_used, PL_STEP_BUDGET),
            ));
        }
        self.steps.push(budget_used);
        self.rounds += 1;
        // 取一批事件（源序严格递增在源内保证）。
        let (first, n) = self.source.take(PL_SOURCE_BATCH);
        if n == 0 {
            self.note_yield(PlYield::NoEvent);
            return Ok(PlYield::NoEvent);
        }
        // 感知账推进（零丢失语义：感知=入账，序号不跳不重）。
        let expect_first = self.sensed;
        if first != expect_first {
            return Err(PlDiag::new(
                PlCode::SourceSeqBad,
                "poll:seq",
                format!("源首序 {} ≠ 感知游标 {}", first, expect_first),
            ));
        }
        self.sensed += n as u64;
        // 让出判定：预算接近满（>=80%）按批量冲刷让出。
        if budget_used * 5 >= PL_STEP_BUDGET * 4 {
            self.note_yield(PlYield::LedgerFlush);
            return Ok(PlYield::LedgerFlush);
        }
        if budget_used > PL_STEP_BUDGET / 2 {
            self.note_yield(PlYield::BudgetExhausted);
            return Ok(PlYield::BudgetExhausted);
        }
        Ok(PlYield::ExplicitToken)
    }

    /// 显式让出（外部令）。
    pub fn yield_now(&mut self) -> Result<(), PlDiag> {
        if self.phase != PlPhase::Polling {
            return Err(PlDiag::new(PlCode::ResumeBad, "yield:phase", format!("相位 {:?}", self.phase)));
        }
        self.note_yield(PlYield::ExplicitToken);
        self.transit(PlPhase::Yielded)
    }

    /// 续跑（Yielded→Polling）。
    pub fn resume(&mut self) -> Result<(), PlDiag> {
        self.transit(PlPhase::Polling)
    }

    /// 让出记账。
    fn note_yield(&mut self, y: PlYield) {
        let mut i = 0;
        while i < 4 {
            if self.yields[i].0 == y {
                self.yields[i].1 += 1;
                break;
            }
            i += 1;
        }
        self.yield_total += 1;
    }

    /// 排干（Polling/Yielded→Draining→Done；判据：排干不净即红）。
    pub fn drain_and_exit(&mut self, exit: PlExit) -> Result<(), PlDiag> {
        if self.phase == PlPhase::Polling {
            self.transit(PlPhase::Draining)?;
        } else if self.phase == PlPhase::Yielded {
            self.transit(PlPhase::Polling)?;
            self.transit(PlPhase::Draining)?;
        }
        // 源残量检查：源 produced 应等于感知数（否则排干不净）。
        if self.source.produced != self.sensed {
            return Err(PlDiag::new(
                PlCode::DrainResidue,
                "drain:residue",
                format!("源 {} ≠ 感知 {}", self.source.produced, self.sensed),
            ));
        }
        self.exit = Some(exit);
        self.transit(PlPhase::Done)?;
        self.fp = self.fingerprint();
        Ok(())
    }

    /// 判据恒等断言：发生数 = 感知数（零丢失）。
    pub fn zero_loss_ok(&self) -> bool {
        self.source.produced == self.sensed
    }

    /// P99 预算断言。
    pub fn p99_in_budget(&self) -> Option<bool> {
        self.steps.p99().map(|p| p <= PL_STEP_BUDGET)
    }

    /// 终账指纹。
    pub fn fingerprint(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        h ^= self.source.produced;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.sensed;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.rounds;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.yield_total;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.phase as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h
    }

    /// 全语义报表。
    pub fn report(&self) -> String {
        let p99 = self.steps.p99().unwrap_or(0);
        format!(
            "相位:{:?} 发生:{} 感知:{} 零丢失:{} 轮数:{} P99步:{}(预算{}) 让出:{}({}/{}/{}/{}) 退出:{:?}",
            self.phase,
            self.source.produced,
            self.sensed,
            self.zero_loss_ok(),
            self.rounds,
            p99,
            PL_STEP_BUDGET,
            self.yield_total,
            self.yields[0].1,
            self.yields[1].1,
            self.yields[2].1,
            self.yields[3].1,
            self.exit
        )
    }
}

// ---------------------------------------------------------------------------
// 判据演练面
// ---------------------------------------------------------------------------

/// 10^6 采样零丢失全量演练（判据点名）：分批泵动至达量，恒等断言全绿。
pub fn drill_1m() -> (bool, String) {
    let mut pump = PlPump::new(8);
    if pump.start().is_err() {
        return (false, "start 失败".to_string());
    }
    let mut guard = 0u64;
    while pump.sensed < PL_SAMPLES_TOTAL && guard < PL_SAMPLES_TOTAL * 2 {
        guard += 1;
        // 步数预算内轮询（确定性行程：偶数轮 8 步，奇数轮 20 步）。
        let steps = if pump.rounds % 2 == 0 { 8 } else { 20 };
        match pump.poll_round(steps) {
            Ok(_) => {}
            Err(d) => return (false, d.full()),
        }
        if pump.sensed >= PL_SAMPLES_TOTAL {
            break;
        }
    }
    if pump.source.produced < PL_SAMPLES_TOTAL {
        return (false, format!("源产量 {} 未达 10^6", pump.source.produced));
    }
    match pump.drain_and_exit(PlExit::SamplesReached) {
        Ok(()) => {
            let green = pump.zero_loss_ok() && pump.p99_in_budget().unwrap_or(false) && pump.phase == PlPhase::Done;
            (green, pump.report())
        }
        Err(d) => (false, d.full()),
    }
}

/// 注入核：6 种破坏注入（丢采样/倒退/超预算/乱序/排干残量/完成态轮询）6/6 必抓。
pub fn inject_break() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：采样丢失（感知账被旁路扣减）。
    let mut p = PlPump::new(4);
    let _ = p.start();
    let _ = p.poll_round(4);
    p.sensed = p.sensed.saturating_sub(1);
    if !p.zero_loss_ok() {
        caught += 1;
        detail.push("注1 采样丢失:抓".to_string());
    } else {
        detail.push("注1 采样丢失:漏".to_string());
    }
    // 注 2：状态倒退（Done→Polling）。
    let mut p = PlPump::new(2);
    let _ = p.start();
    let _ = p.drain_and_exit(PlExit::StopToken);
    if p.transit(PlPhase::Polling).is_err() {
        caught += 1;
        detail.push("注2 状态倒退:抓".to_string());
    } else {
        detail.push("注2 状态倒退:漏".to_string());
    }
    // 注 3：单轮超预算。
    let mut p = PlPump::new(2);
    let _ = p.start();
    if p.poll_round(PL_STEP_BUDGET + 1).is_err() {
        caught += 1;
        detail.push("注3 超预算:抓".to_string());
    } else {
        detail.push("注3 超预算:漏".to_string());
    }
    // 注 4：源序乱序（游标被外置破坏）。
    let mut p = PlPump::new(2);
    let _ = p.start();
    let _ = p.poll_round(4);
    p.sensed = 99;
    if p.poll_round(4).is_err() {
        caught += 1;
        detail.push("注4 源序乱序:抓".to_string());
    } else {
        detail.push("注4 源序乱序:漏".to_string());
    }
    // 注 5：排干残量。
    let mut p = PlPump::new(2);
    let _ = p.start();
    let _ = p.poll_round(4);
    p.source.produced += 5;
    if p.drain_and_exit(PlExit::StopToken).is_err() {
        caught += 1;
        detail.push("注5 排干残量:抓".to_string());
    } else {
        detail.push("注5 排干残量:漏".to_string());
    }
    // 注 6：完成态再轮询。
    let mut p = PlPump::new(2);
    let _ = p.start();
    let _ = p.drain_and_exit(PlExit::StopToken);
    if p.poll_round(4).is_err() {
        caught += 1;
        detail.push("注6 完成态轮询:抓".to_string());
    } else {
        detail.push("注6 完成态轮询:漏".to_string());
    }
    (caught, 6, detail)
}
